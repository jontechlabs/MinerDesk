//! Disposable root-process tests. Never install a policy or touch a real miner.
#[path = "../authorization.rs"]
mod authorization;
#[path = "../supervisor.rs"]
mod supervisor;
use std::{
    fs,
    io::{BufRead, BufReader},
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
fn main() {
    let args = std::env::args().collect::<Vec<_>>();
    if args.get(1).is_some_and(|s| s == "--child") {
        let code = supervisor::run(
            &PathBuf::from(&args[2]),
            vec!["30".into()],
            Some(PathBuf::from(&args[3])),
        )
        .unwrap();
        std::process::exit(code);
    }
    if unsafe { libc::geteuid() } != 0 {
        println!(
            "Root process fixtures skipped: run the compiled test executable as root in WSL/CI."
        );
        return;
    }
    let directory = tempfile::Builder::new()
        .prefix("md-root-process-fixture-")
        .tempdir_in("/root")
        .unwrap();
    let source = directory.path().join("source");
    fs::create_dir(&source).unwrap();
    fs::set_permissions(&source, fs::Permissions::from_mode(0o700)).unwrap();
    let exe = source.join("SRBMiner-MULTI");
    fs::copy("/bin/sleep", &exe).unwrap();
    fs::set_permissions(&exe, fs::Permissions::from_mode(0o700)).unwrap();
    let request = authorization::protocol::Enrollment {
        profile_id: "fixture".into(),
        profile_name: "Fixture".into(),
        engine: "srbminer".into(),
        executable: exe,
        user_args: vec!["30".into()],
        support_args: None,
        support_percent: 0.0,
    };
    let fingerprint = authorization::protocol::request_fingerprint(&request).unwrap();
    // Use internal root-only fixture storage, never Store::system().
    let store = authorization::Store::fixture(directory.path().join("store"));
    let grant = store.enroll(0, request, &fingerprint, || true).unwrap();
    let (snapshot, _, record) = store.approved(0, &grant.id, &fingerprint, "user").unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--child",
            snapshot.to_str().unwrap(),
            record.to_str().unwrap(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut reader = BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let pid: i32 = line
        .trim()
        .strip_prefix("MINERDESK_ELEVATION_READY ")
        .unwrap()
        .parse()
        .unwrap();
    assert!(fs::read_to_string(format!("/proc/{pid}/status"))
        .unwrap()
        .contains("Uid:\t0\t0\t0\t0"));
    store.revoke(0, &grant.id).unwrap();
    let deadline = Instant::now() + Duration::from_secs(6);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        assert!(
            Instant::now() < deadline,
            "revocation did not stop the root supervisor"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(
        unsafe { libc::kill(pid, 0) },
        -1,
        "root miner survived revocation"
    );
    println!("Passed: an approved root command starts from a protected snapshot; revocation stops its process group.");
}
