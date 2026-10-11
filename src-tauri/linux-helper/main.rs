//! A one-session, pipe-owned privileged miner supervisor. No network API,
//! password handling, setuid bit or persistent authorization policy.
#[path = "../src/linux_terminal.rs"]
mod terminal;
use std::{io::{self, Write}, os::unix::{fs::PermissionsExt, process::{CommandExt, ExitStatusExt}}, path::PathBuf,
    process::{Child, Command, ExitStatus, Stdio}, sync::atomic::{AtomicBool, Ordering}, time::{Duration, Instant}};

static STOP: AtomicBool = AtomicBool::new(false);
extern "C" fn stop_signal(_: libc::c_int) { STOP.store(true, Ordering::SeqCst); }

fn stop_group(child: &mut Child) -> io::Result<ExitStatus> {
    let group = -(child.id() as libc::pid_t);
    unsafe { libc::kill(group, libc::SIGTERM); }
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if unsafe { libc::kill(group, 0) } == -1 { break; }
        std::thread::sleep(Duration::from_millis(50));
    }
    unsafe { libc::kill(group, libc::SIGKILL); }
    let status = child.wait()?;
    // Reap adopted descendants after killing the whole miner process group.
    while unsafe { libc::waitpid(-1, std::ptr::null_mut(), libc::WNOHANG) } > 0 {}
    Ok(status)
}

struct MinerGuard { child: Child, cleaned: bool }
impl Drop for MinerGuard {
    fn drop(&mut self) { if !self.cleaned { let _ = stop_group(&mut self.child); } }
}

fn has_exited(child: &Child) -> io::Result<bool> {
    // Leave the leader unreaped until the entire group is stopped. This reserves
    // its PID, preventing a cleanup signal from targeting a reused process group.
    let mut info = std::mem::MaybeUninit::<libc::siginfo_t>::zeroed();
    let result = unsafe { libc::waitid(libc::P_PID, child.id(), info.as_mut_ptr(), libc::WEXITED | libc::WNOHANG | libc::WNOWAIT) };
    if result == -1 { return Err(io::Error::last_os_error()); }
    Ok(unsafe { info.assume_init().si_pid() } != 0)
}

fn run() -> Result<i32, String> {
    let mut arguments = std::env::args_os().skip(1);
    if arguments.next().as_deref() != Some(std::ffi::OsStr::new("--application-version")) {
        return Err("Internal helper: expected an application version".into());
    }
    if arguments.next().as_deref() != Some(std::ffi::OsStr::new(env!("CARGO_PKG_VERSION"))) {
        return Err("Linux elevation helper version differs from MinerDesk; reinstall the package".into());
    }
    if arguments.next().as_deref() != Some(std::ffi::OsStr::new("--miner")) {
        return Err("Internal helper: expected a miner executable".into());
    }
    let executable = PathBuf::from(arguments.next().ok_or("Missing miner executable")?);
    if !executable.is_absolute() || !executable.is_file() || executable.metadata().map_err(|e|e.to_string())?.permissions().mode() & 0o111 == 0 {
        return Err("The miner must be an absolute, executable file".into());
    }
    if arguments.next().as_deref() != Some(std::ffi::OsStr::new("--")) { return Err("Missing argument separator".into()); }
    if unsafe { libc::geteuid() } != 0 { return Err("Linux miner elevation was not authorized: this helper must run as root".into()); }
    let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
    if unsafe { libc::fstat(libc::STDIN_FILENO, stat.as_mut_ptr()) } != 0 || unsafe { stat.assume_init() }.st_mode & libc::S_IFMT != libc::S_IFIFO {
        return Err("The Linux helper requires MinerDesk's private control pipe".into());
    }
    // A closed owner pipe must never start a miner after a delayed approval.
    let mut owner = libc::pollfd { fd: libc::STDIN_FILENO, events: libc::POLLIN | libc::POLLHUP, revents: 0 };
    if unsafe { libc::poll(&mut owner, 1, 0) } > 0 { return Err("MinerDesk cancelled this elevated start".into()); }
    unsafe {
        libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0);
        for signal in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP] { libc::signal(signal, stop_signal as *const () as libc::sighandler_t); }
    }
    let supervisor = unsafe { libc::getpid() };
    let mut command = Command::new(&executable);
    command.args(arguments).current_dir(executable.parent().ok_or("Missing miner directory")?).process_group(0);
    unsafe { command.pre_exec(move || {
        if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL, 0, 0, 0) == -1 { return Err(io::Error::last_os_error()); }
        if libc::getppid() != supervisor { libc::_exit(125); }
        Ok(())
    }); }
    // Real terminals preserve SRBMiner's live output. The private owner pipe is
    // NOT the miner's stdin, so no miner can read credentials/control messages.
    let (child, mut stdout, mut stderr) = terminal::spawn(command, Stdio::null())?;
    let mut miner = MinerGuard { child, cleaned: false };
    writeln!(io::stdout().lock(), "MINERDESK_ELEVATION_READY {}", miner.child.id()).map_err(|e|e.to_string())?;
    io::stdout().flush().map_err(|e|e.to_string())?;
    let output = std::thread::spawn(move || { let _ = io::copy(&mut stdout, &mut io::stdout().lock()); });
    let errors = std::thread::spawn(move || { let _ = io::copy(&mut stderr, &mut io::stderr().lock()); });
    let requested_code = loop {
        if STOP.load(Ordering::SeqCst) { break Some(0); }
        if has_exited(&miner.child).map_err(|e|e.to_string())? { break None; }
        owner.revents = 0;
        let result = unsafe { libc::poll(&mut owner, 1, 100) };
        if result > 0 && owner.revents != 0 { break Some(0); } // Stop bytes, EOF or pipe error.
        if result < 0 && io::Error::last_os_error().kind() != io::ErrorKind::Interrupted { break Some(125); }
    };
    let status = stop_group(&mut miner.child).map_err(|e|e.to_string())?;
    miner.cleaned = true;
    let _ = output.join(); let _ = errors.join();
    Ok(requested_code.unwrap_or_else(|| status.code().unwrap_or_else(|| 128 + status.signal().unwrap_or(0))))
}

fn main() {
    if std::env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new("--version")) {
        println!("minerdesk-linux-helper {}", env!("CARGO_PKG_VERSION")); return;
    }
    match run() {
        Ok(code) => std::process::exit(code),
        Err(error) => { eprintln!("[MinerDesk Linux elevation] {error}"); std::process::exit(125); }
    }
}
