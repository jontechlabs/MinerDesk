#[path = "../src/linux_terminal.rs"]
mod terminal;
use std::{
    io::{self, Write},
    os::unix::{
        fs::MetadataExt,
        process::{CommandExt, ExitStatusExt},
    },
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

static STOP: AtomicBool = AtomicBool::new(false);
extern "C" fn stop_signal(_: libc::c_int) {
    STOP.store(true, Ordering::SeqCst);
}
fn stop_group(child: &mut Child) -> io::Result<ExitStatus> {
    let group = -(child.id() as libc::pid_t);
    unsafe {
        libc::kill(group, libc::SIGTERM);
    }
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if unsafe { libc::kill(group, 0) } == -1 {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    unsafe {
        libc::kill(group, libc::SIGKILL);
    }
    let status = child.wait()?;
    while unsafe { libc::waitpid(-1, std::ptr::null_mut(), libc::WNOHANG) } > 0 {}
    Ok(status)
}
struct MinerGuard {
    child: Child,
    cleaned: bool,
}
impl Drop for MinerGuard {
    fn drop(&mut self) {
        if !self.cleaned {
            let _ = stop_group(&mut self.child);
        }
    }
}
fn has_exited(child: &Child) -> io::Result<bool> {
    let mut info = std::mem::MaybeUninit::<libc::siginfo_t>::zeroed();
    let result = unsafe {
        libc::waitid(
            libc::P_PID,
            child.id(),
            info.as_mut_ptr(),
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        )
    };
    if result == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { info.assume_init().si_pid() } != 0)
}
pub fn run(
    executable: &Path,
    args: Vec<std::ffi::OsString>,
    record: Option<PathBuf>,
) -> Result<i32, String> {
    if unsafe { libc::geteuid() } != 0 {
        return Err(
            "Linux miner elevation was not authorized: this helper must run as root".into(),
        );
    }
    let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
    if unsafe { libc::fstat(0, stat.as_mut_ptr()) } != 0
        || unsafe { stat.assume_init() }.st_mode & libc::S_IFMT != libc::S_IFIFO
    {
        return Err("The Linux helper requires MinerDesk's private control pipe".into());
    }
    let mut owner = libc::pollfd {
        fd: 0,
        events: libc::POLLIN | libc::POLLHUP,
        revents: 0,
    };
    if unsafe { libc::poll(&mut owner, 1, 0) } > 0 {
        return Err("MinerDesk cancelled this elevated start".into());
    }
    let identity = record
        .as_ref()
        .map(|p| std::fs::symlink_metadata(p).map(|m| (m.dev(), m.ino())))
        .transpose()
        .map_err(|e| e.to_string())?;
    unsafe {
        libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0);
        for signal in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP] {
            libc::signal(signal, stop_signal as *const () as libc::sighandler_t);
        }
    }
    let supervisor = unsafe { libc::getpid() };
    let mut command = Command::new(executable);
    command
        .args(args)
        .current_dir(executable.parent().ok_or("Missing miner directory")?)
        .process_group(0);
    if record.is_some() {
        command
            .env_clear()
            .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
            .env("HOME", executable.parent().unwrap())
            .env("LANG", "C.UTF-8");
    }
    unsafe {
        command.pre_exec(move || {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL, 0, 0, 0) == -1 {
                return Err(io::Error::last_os_error());
            }
            if libc::getppid() != supervisor {
                libc::_exit(125);
            }
            Ok(())
        });
    }
    let (child, mut stdout, mut stderr) = terminal::spawn(command, Stdio::null())?;
    let mut miner = MinerGuard {
        child,
        cleaned: false,
    };
    writeln!(
        io::stdout().lock(),
        "MINERDESK_ELEVATION_READY {}",
        miner.child.id()
    )
    .map_err(|e| e.to_string())?;
    io::stdout().flush().map_err(|e| e.to_string())?;
    let output = std::thread::spawn(move || {
        let _ = io::copy(&mut stdout, &mut io::stdout().lock());
    });
    let errors = std::thread::spawn(move || {
        let _ = io::copy(&mut stderr, &mut io::stderr().lock());
    });
    let requested_code = loop {
        if STOP.load(Ordering::SeqCst) {
            break Some(0);
        }
        if let Some(ref p) = record {
            if std::fs::symlink_metadata(p)
                .ok()
                .map(|m| (m.dev(), m.ino()))
                != identity
            {
                break Some(0);
            }
        }
        if has_exited(&miner.child).map_err(|e| e.to_string())? {
            break None;
        }
        owner.revents = 0;
        let result = unsafe { libc::poll(&mut owner, 1, 100) };
        if result > 0 && owner.revents != 0 {
            break Some(0);
        }
        if result < 0 && io::Error::last_os_error().kind() != io::ErrorKind::Interrupted {
            break Some(125);
        }
    };
    let status = stop_group(&mut miner.child).map_err(|e| e.to_string())?;
    miner.cleaned = true;
    let _ = output.join();
    let _ = errors.join();
    Ok(requested_code.unwrap_or_else(|| {
        status
            .code()
            .unwrap_or_else(|| 128 + status.signal().unwrap_or(0))
    }))
}
