//! Constrained runner: no enrollment, arbitrary executable, shell or arguments.
mod authorization;
mod supervisor;

fn run() -> Result<i32, String> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 8
        || args[0] != "--application-version"
        || args[2] != "--grant"
        || args[4] != "--fingerprint"
        || args[6] != "--mode"
    {
        return Err("Only an existing approved command identifier is accepted".into());
    }
    let uid = authorization::caller_uid()?;
    let store = authorization::Store::system();
    if !authorization::protocol::valid_hex(&args[3], 32)
        || !authorization::protocol::valid_hex(&args[5], 64)
    {
        return Err("Invalid authorization reference".into());
    }
    if args[7] == "revoke" {
        store.revoke(uid, &args[3])?;
        println!("{{\"state\":\"revoked\"}}");
        return Ok(0);
    }
    if args[1] != env!("CARGO_PKG_VERSION") {
        return Err("Approved helper version changed; authorize the profile again".into());
    }
    let (executable, arguments, record) = store.approved(uid, &args[3], &args[5], &args[7])?;
    if args[7] == "status" {
        println!("{{\"state\":\"approved\"}}");
        return Ok(0);
    }
    supervisor::run(
        &executable,
        arguments.into_iter().map(Into::into).collect(),
        Some(record),
    )
}
fn main() {
    if std::env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new("--version")) {
        println!("minerdesk-approved-helper {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    match run() {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("[MinerDesk approved miner] {error}");
            std::process::exit(125);
        }
    }
}
