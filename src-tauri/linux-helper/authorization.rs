//! Only the interactive helper may enroll a grant. The separate approved
//! helper can run/revoke an existing UID-bound grant; it cannot enroll one.
#[path = "../src/linux_authorization_protocol.rs"]
pub mod protocol;
use protocol::{Enrollment, Entry, GrantRef};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::{BufRead, Read, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

pub const RUNNER: &str = "/usr/lib/minerdesk/minerdesk-approved-helper";
const POLICY: &str = "/usr/share/polkit-1/actions/com.minerdesk.approved-miner.policy";
const STORE: &str = "/var/lib/minerdesk/root-authorization";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Grant {
    version: String,
    uid: u32,
    id: String,
    request: Enrollment,
    fingerprint: String,
    entries: Vec<Entry>,
}

pub fn caller_uid() -> Result<u32, String> {
    if unsafe { libc::geteuid() } != 0 || unsafe { libc::getuid() } != 0 {
        return Err("This operation requires system authorization".into());
    }
    // pkexec supplies this value itself after clearing the caller's environment.
    // It is never read by an unprivileged process to make an access decision.
    match std::env::var("PKEXEC_UID") {
        Ok(value) => value
            .parse()
            .map_err(|_| "Invalid system caller identity".into()),
        Err(_) => Ok(0), // Direct root administration, not an app-provided UID.
    }
}

fn private_root_directory(path: &Path, mode: u32) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or("Missing authorization directory parent")?;
    if !path.exists() {
        check_root_directory(parent)?;
        fs::DirBuilder::new()
            .mode(mode)
            .create(path)
            .map_err(|e| e.to_string())?;
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).map_err(|e| e.to_string())?;
    }
    check_root_directory(path)?;
    if fs::symlink_metadata(path)
        .map_err(|e| e.to_string())?
        .mode()
        & 0o777
        != mode
    {
        return Err("Authorization directory has unexpected permissions".into());
    }
    Ok(())
}

fn check_root_directory(path: &Path) -> Result<(), String> {
    protocol::open_directory(path)?;
    let mut current = PathBuf::from("/");
    for component in path.components() {
        if let std::path::Component::Normal(name) = component {
            current.push(name);
        }
        let m = fs::symlink_metadata(&current).map_err(|e| e.to_string())?;
        if !m.is_dir() || m.uid() != 0 || m.mode() & 0o022 != 0 {
            return Err(
                "Authorization storage must be root-owned and not writable by other users".into(),
            );
        }
    }
    Ok(())
}

fn random_id() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .map_err(|e| e.to_string())?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

fn atomic_file(path: &Path, data: &[u8], mode: u32) -> Result<(), String> {
    let parent = path.parent().ok_or("Missing protected file parent")?;
    check_root_directory(parent)?;
    if let Ok(meta) = fs::symlink_metadata(path) {
        if !meta.is_file() || meta.uid() != 0 || meta.mode() & 0o022 != 0 {
            return Err("Unsafe existing protected file".into());
        }
    }
    let temporary = parent.join(format!(".minerdesk-{}", random_id()?));
    let result = (|| {
        let mut file = File::options()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)
            .map_err(|e| e.to_string())?;
        file.write_all(data).map_err(|e| e.to_string())?;
        file.set_permissions(fs::Permissions::from_mode(mode))
            .map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        fs::rename(&temporary, path).map_err(|e| e.to_string())?;
        File::open(parent)
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

fn install_runner() -> Result<(), String> {
    let source = std::env::current_exe()
        .map_err(|e| e.to_string())?
        .parent()
        .ok_or("Missing helper directory")?
        .join("minerdesk-approved-helper");
    let mut file = File::options()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(source)
        .map_err(|e| e.to_string())?;
    if !file.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err("Approved helper is not a regular file".into());
    }
    let size = file.metadata().map_err(|e| e.to_string())?.len();
    if size > 32 * 1024 * 1024 {
        return Err("Approved helper exceeds the packaged executable size limit".into());
    }
    let mut bytes = vec![];
    std::io::Read::by_ref(&mut file)
        .take(size + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 != size {
        return Err("Approved helper changed while installing; retry authorization".into());
    }
    if !bytes.starts_with(b"\x7fELF") {
        return Err("Approved helper is not the packaged Linux executable".into());
    }
    private_root_directory(Path::new("/usr/lib/minerdesk"), 0o755)?;
    atomic_file(Path::new(RUNNER), &bytes, 0o755)?;
    // This action is deliberately ONLY for the constrained runner. The
    // interactive helper has no permissive action, and still needs auth_admin.
    let policy = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE policyconfig PUBLIC "-//freedesktop//DTD PolicyKit Policy Configuration 1.0//EN" "http://www.freedesktop.org/standards/PolicyKit/1/policyconfig.dtd">
<policyconfig><vendor>MinerDesk</vendor><action id="com.minerdesk.run-approved-miner">
<description>Run a previously approved MinerDesk command</description>
<message>Run a previously approved MinerDesk command</message>
<defaults><allow_any>yes</allow_any><allow_inactive>yes</allow_inactive><allow_active>yes</allow_active></defaults>
<annotate key="org.freedesktop.policykit.exec.path">{RUNNER}</annotate>
</action></policyconfig>
"#
    );
    atomic_file(Path::new(POLICY), policy.as_bytes(), 0o644)
}

pub struct Store {
    base: PathBuf,
}
impl Store {
    #[cfg(test)]
    pub fn fixture(parent: PathBuf) -> Self {
        private_root_directory(&parent, 0o755).unwrap();
        Self {
            base: parent.join("grants"),
        }
    }
    pub fn system() -> Self {
        Self { base: STORE.into() }
    }
    fn owner_dir(&self, uid: u32) -> PathBuf {
        self.base.join(uid.to_string())
    }
    fn grant_dir(&self, uid: u32, id: &str) -> Result<PathBuf, String> {
        if !protocol::valid_hex(id, 32) {
            return Err("Invalid authorization identifier".into());
        }
        Ok(self.owner_dir(uid).join(id))
    }
    fn prepare(&self, uid: u32) -> Result<(), String> {
        private_root_directory(self.base.parent().ok_or("Missing storage parent")?, 0o755)?;
        private_root_directory(&self.base, 0o700)?;
        private_root_directory(&self.owner_dir(uid), 0o700)
    }
    pub fn enroll(
        &self,
        uid: u32,
        request: Enrollment,
        expected: &str,
        owner_alive: impl Fn() -> bool,
    ) -> Result<GrantRef, String> {
        protocol::validate_request(&request)?;
        if !protocol::valid_hex(expected, 64) {
            return Err("Invalid authorization fingerprint".into());
        }
        self.prepare(uid)?;
        let id = random_id()?;
        let directory = self.grant_dir(uid, &id)?;
        private_root_directory(&directory, 0o700)?;
        let result = (|| {
            let snapshot = directory.join("miner");
            private_root_directory(&snapshot, 0o700)?;
            let entries = protocol::inventory(
                request.executable.parent().ok_or("Missing miner folder")?,
                Some(&snapshot),
                Some(uid),
            )?;
            let actual = protocol::fingerprint(&request, &entries)?;
            if actual != expected {
                return Err("Miner files or commands changed during authorization; retry".into());
            }
            let executable = snapshot.join(
                request
                    .executable
                    .file_name()
                    .ok_or("Missing miner filename")?,
            );
            let mut header = [0u8; 4];
            File::open(&executable)
                .and_then(|mut f| f.read_exact(&mut header))
                .map_err(|e| e.to_string())?;
            if &header != b"\x7fELF" {
                return Err("Unattended authorization requires the original ELF miner executable, not a script".into());
            }
            if !owner_alive() {
                return Err("MinerDesk cancelled authorization".into());
            }
            let grant = Grant {
                version: env!("CARGO_PKG_VERSION").into(),
                uid,
                id: id.clone(),
                request,
                fingerprint: actual.clone(),
                entries,
            };
            let bytes = serde_json::to_vec(&grant).map_err(|e| e.to_string())?;
            if bytes.len() > 2 * 1024 * 1024 {
                return Err("Authorization record exceeds the metadata size limit".into());
            }
            atomic_file(&directory.join("grant.json"), &bytes, 0o600)?;
            if !owner_alive() {
                return Err("MinerDesk cancelled authorization".into());
            }
            Ok(GrantRef {
                id: id.clone(),
                fingerprint: actual,
            })
        })();
        if result.is_err() {
            let _ = fs::remove_dir_all(directory);
        }
        result
    }
    fn load(&self, uid: u32, id: &str) -> Result<(Grant, PathBuf), String> {
        let directory = self.grant_dir(uid, id)?;
        check_root_directory(&directory)?;
        let path = directory.join("grant.json");
        let mut file = File::options()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&path)
            .map_err(|_| {
                "Authorization is missing or revoked; authorize this profile again".to_string()
            })?;
        let metadata = file.metadata().map_err(|e| e.to_string())?;
        if !metadata.is_file()
            || metadata.uid() != 0
            || metadata.mode() & 0o077 != 0
            || metadata.len() > 2 * 1024 * 1024
        {
            return Err("Unsafe authorization record".into());
        }
        let mut bytes = vec![];
        file.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
        let grant: Grant = serde_json::from_slice(&bytes)
            .map_err(|_| "Authorization record is invalid; authorize again".to_string())?;
        if grant.uid != uid || grant.id != id {
            return Err("Authorization belongs to another Linux account".into());
        }
        protocol::validate_request(&grant.request)?;
        if grant.entries.iter().any(|e| {
            e.name.is_empty()
                || Path::new(&e.name)
                    .components()
                    .any(|c| !matches!(c, std::path::Component::Normal(_)))
        }) {
            return Err("Unsafe snapshot filename".into());
        }
        Ok((grant, path))
    }
    pub fn approved(
        &self,
        uid: u32,
        id: &str,
        expected: &str,
        mode: &str,
    ) -> Result<(PathBuf, Vec<String>, PathBuf), String> {
        let (grant, record) = self.load(uid, id)?;
        if grant.version != env!("CARGO_PKG_VERSION")
            || grant.fingerprint != expected
            || protocol::fingerprint(&grant.request, &grant.entries)? != expected
        {
            return Err("MinerDesk, the miner files or the launch settings changed; authorize this profile again".into());
        }
        let directory = record.parent().unwrap().join("miner");
        check_root_directory(&directory)?;
        let entries = protocol::inventory(&directory, None, Some(0))?;
        // The trusted miner may create logs/cache. Code and libraries must still
        // match the approved bytes; unknown executable files fail closed.
        let code = |e: &Entry| e.executable || e.name.ends_with(".so") || e.name.contains(".so.");
        for entry in entries.iter().filter(|e| code(e)) {
            if !grant.entries.contains(entry) {
                return Err("The approved miner copy changed; authorize again".into());
            }
        }
        for entry in grant.entries.iter().filter(|e| code(e)) {
            if !entries.contains(entry) {
                return Err("An approved executable or library is missing; authorize again".into());
            }
        }
        let arguments = match mode {
            "user" => grant.request.user_args,
            "support" => grant
                .request
                .support_args
                .ok_or("Voluntary-support command was not authorized")?,
            "status" => vec![],
            _ => return Err("Invalid approved launch mode".into()),
        };
        Ok((
            directory.join(grant.request.executable.file_name().unwrap()),
            arguments,
            record,
        ))
    }
    pub fn revoke(&self, uid: u32, id: &str) -> Result<(), String> {
        let directory = self.grant_dir(uid, id)?;
        if !directory.exists() {
            return Ok(());
        }
        self.load(uid, id)?; // Ownership checked before touching protected files.
        fs::remove_file(directory.join("grant.json")).map_err(|e| e.to_string())?;
        // Supervisors see the missing record and stop; remove the root snapshot.
        fs::remove_dir_all(directory).map_err(|e| e.to_string())
    }
}

pub fn enroll_from_stdin() -> Result<GrantRef, String> {
    let uid = caller_uid()?;
    let owner_alive = || {
        let mut p = libc::pollfd {
            fd: 0,
            events: libc::POLLHUP | libc::POLLERR,
            revents: 0,
        };
        (unsafe { libc::poll(&mut p, 1, 0) }) >= 0
            && p.revents & (libc::POLLHUP | libc::POLLERR) == 0
    };
    let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
    if unsafe { libc::fstat(0, stat.as_mut_ptr()) } != 0
        || unsafe { stat.assume_init() }.st_mode & libc::S_IFMT != libc::S_IFIFO
    {
        return Err("Authorization requires MinerDesk's private pipe".into());
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Input {
        request: Enrollment,
        fingerprint: String,
    }
    let mut line = String::new();
    std::io::stdin()
        .lock()
        .take(protocol::MAX_REQUEST_BYTES as u64 + 128)
        .read_line(&mut line)
        .map_err(|e| e.to_string())?;
    let input: Input =
        serde_json::from_str(&line).map_err(|_| "Invalid authorization request".to_string())?;
    if !owner_alive() {
        return Err("MinerDesk cancelled authorization".into());
    }
    let reference = Store::system().enroll(uid, input.request, &input.fingerprint, owner_alive)?;
    if let Err(error) = install_runner() {
        let _ = Store::system().revoke(uid, &reference.id);
        return Err(error);
    }
    Ok(reference)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    fn root_store() -> (tempfile::TempDir, Store) {
        assert_eq!(
            unsafe { libc::geteuid() },
            0,
            "Run root-store tests as root in disposable WSL/CI fixtures"
        );
        let d = tempfile::Builder::new()
            .prefix("md-authorization-")
            .tempdir_in("/root")
            .unwrap();
        let base = d.path().join("data");
        fs::create_dir(&base).unwrap();
        fs::set_permissions(&base, fs::Permissions::from_mode(0o755)).unwrap();
        let store = Store {
            base: base.join("grants"),
        };
        (d, store)
    }
    fn fixture(path: &Path) -> Enrollment {
        fs::create_dir(path).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
        let executable = path.join("SRBMiner-MULTI");
        fs::write(&executable, b"\x7fELFfake miner bytes").unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        Enrollment {
            profile_id: "fixture".into(),
            profile_name: "Fixture".into(),
            engine: "srbminer".into(),
            executable,
            user_args: vec![
                "--algorithm".into(),
                "pearlhash".into(),
                "Rig with spaces".into(),
            ],
            support_args: None,
            support_percent: 0.0,
        }
    }
    #[test]
    #[ignore = "requires root in disposable WSL/CI fixture storage"]
    fn uid_commands_snapshot_tampering_and_revocation_are_enforced() {
        let (d, store) = root_store();
        let request = fixture(&d.path().join("source"));
        let hash = protocol::request_fingerprint(&request).unwrap();
        let grant = store.enroll(0, request.clone(), &hash, || true).unwrap();
        let (exe, args, _) = store.approved(0, &grant.id, &hash, "user").unwrap();
        assert_ne!(exe, request.executable);
        assert_eq!(args[2], "Rig with spaces");
        assert!(store.approved(1000, &grant.id, &hash, "user").is_err());
        assert!(store
            .approved(0, &grant.id, &"f".repeat(64), "user")
            .is_err());
        assert!(store.approved(0, &grant.id, &hash, "support").is_err());
        fs::write(&request.executable, b"replaced source").unwrap();
        assert_eq!(fs::read(&exe).unwrap(), b"\x7fELFfake miner bytes");
        fs::write(exe.parent().unwrap().join("runtime.log"), b"log").unwrap();
        store.approved(0, &grant.id, &hash, "user").unwrap();
        fs::write(&exe, b"tampered snapshot").unwrap();
        assert!(store.approved(0, &grant.id, &hash, "user").is_err());
        store.revoke(0, &grant.id).unwrap();
        assert!(store.approved(0, &grant.id, &hash, "user").is_err());
    }
    #[test]
    #[ignore = "requires root in disposable WSL/CI fixture storage"]
    fn cancellation_changed_sources_scripts_and_unsafe_storage_leave_no_grant() {
        let (d, store) = root_store();
        let request = fixture(&d.path().join("source"));
        let hash = protocol::request_fingerprint(&request).unwrap();
        assert!(store.enroll(0, request.clone(), &hash, || false).is_err());
        assert!(store
            .enroll(0, request.clone(), &"f".repeat(64), || true)
            .is_err());
        assert_eq!(fs::read_dir(store.owner_dir(0)).unwrap().count(), 0);
        fs::write(&request.executable, b"#!/bin/sh\n").unwrap();
        let hash = protocol::fingerprint(
            &request,
            &protocol::inventory(request.executable.parent().unwrap(), None, None).unwrap(),
        )
        .unwrap();
        assert!(store.enroll(0, request, &hash, || true).is_err());
        let fake = d.path().join("unsafe");
        symlink("/tmp", &fake).unwrap();
        assert!(check_root_directory(&fake).is_err());
        fs::set_permissions(&store.base, fs::Permissions::from_mode(0o777)).unwrap();
        assert!(store.prepare(0).is_err());
    }
}
