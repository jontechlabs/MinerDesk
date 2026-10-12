//! Interactive system-authorized supervisor and grant enrollment entry point.
mod authorization;
mod supervisor;
use std::{os::unix::fs::PermissionsExt, path::PathBuf};

fn run() -> Result<i32, String> {
    let mut arguments = std::env::args_os().skip(1);
    if arguments.next().as_deref() != Some(std::ffi::OsStr::new("--application-version")) {
        return Err("Internal helper: expected an application version".into());
    }
    if arguments.next().as_deref() != Some(std::ffi::OsStr::new(env!("CARGO_PKG_VERSION"))) {
        return Err(
            "Linux elevation helper version differs from MinerDesk; reinstall the package".into(),
        );
    }
    let operation = arguments.next();
    if operation.as_deref() == Some(std::ffi::OsStr::new("--authorize")) {
        if arguments.next().is_some() {
            return Err("Unexpected enrollment arguments".into());
        }
        let grant = authorization::enroll_from_stdin()?;
        println!(
            "MINERDESK_AUTHORIZATION {}",
            serde_json::to_string(&grant).map_err(|e| e.to_string())?
        );
        return Ok(0);
    }
    if operation.as_deref() != Some(std::ffi::OsStr::new("--miner")) {
        return Err("Internal helper: expected a miner executable".into());
    }
    let executable = PathBuf::from(arguments.next().ok_or("Missing miner executable")?);
    if !executable.is_absolute()
        || !executable.is_file()
        || executable
            .metadata()
            .map_err(|e| e.to_string())?
            .permissions()
            .mode()
            & 0o111
            == 0
    {
        return Err("The miner must be an absolute, executable file".into());
    }
    if arguments.next().as_deref() != Some(std::ffi::OsStr::new("--")) {
        return Err("Missing argument separator".into());
    }
    supervisor::run(&executable, arguments.collect(), None)
}
fn main() {
    if std::env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new("--version")) {
        println!("minerdesk-linux-helper {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    match run() {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("[MinerDesk Linux elevation] {error}");
            std::process::exit(125);
        }
    }
}
