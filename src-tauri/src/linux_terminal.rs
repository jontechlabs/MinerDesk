use std::{fs, io::Read, process::{Child, Command, Stdio}};
pub type ProcessReader = Box<dyn Read + Send>;

pub fn spawn(mut command: Command, stdin: Stdio) -> Result<(Child, ProcessReader, ProcessReader), String> {
    use std::os::fd::FromRawFd;
    fn terminal_pair() -> Result<(fs::File, fs::File), String> {
        use std::os::fd::AsRawFd;
        let (mut master_fd, mut slave_fd) = (-1, -1);
        // openpty creates owned descriptors. Convert each exactly once to a
        // File so every failure path closes it. No shell is involved.
        if unsafe { libc::openpty(&mut master_fd, &mut slave_fd, std::ptr::null_mut(), std::ptr::null(), std::ptr::null()) } != 0 {
            return Err(format!("Miner terminal capture: {}", std::io::Error::last_os_error()));
        }
        let master = unsafe { fs::File::from_raw_fd(master_fd) };
        let slave = unsafe { fs::File::from_raw_fd(slave_fd) };
        for file in [&master, &slave] {
            if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) } == -1 {
                return Err(std::io::Error::last_os_error().to_string());
            }
        }
        Ok((master, slave))
    }
    // Separate terminals retain the boundary between stdout and stderr, even
    // when a miner omits the last newline on one of its streams.
    let (stdout_master, stdout_slave) = terminal_pair()?;
    let (stderr_master, stderr_slave) = terminal_pair()?;
    let child = command.stdin(stdin).stdout(Stdio::from(stdout_slave)).stderr(Stdio::from(stderr_slave)).spawn().map_err(|e| e.to_string())?;
    // Command retains its Stdio handles after spawn. Close our slave copies so
    // the master reaches EOF when the child exits or is killed at the deadline.
    drop(command);
    struct TerminalReader(fs::File);
    impl Read for TerminalReader {
        fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
            match self.0.read(bytes) {
                Err(error) if error.raw_os_error() == Some(libc::EIO) => Ok(0), // Linux PTY EOF
                result => result,
            }
        }
    }
    Ok((child, Box::new(TerminalReader(stdout_master)), Box::new(TerminalReader(stderr_master))))
}
