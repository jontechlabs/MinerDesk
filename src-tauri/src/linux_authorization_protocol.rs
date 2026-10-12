//! Shared, versioned authorization data. No administrator credentials.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    ffi::CString,
    fs::File,
    io::{Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{
            ffi::OsStrExt,
            fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
        },
    },
    path::{Component, Path, PathBuf},
};

pub const MAX_REQUEST_BYTES: usize = 64 * 1024;
const MAX_FILES: usize = 4096;
const MAX_BYTES: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Enrollment {
    pub profile_id: String,
    pub profile_name: String,
    pub engine: String,
    pub executable: PathBuf,
    pub user_args: Vec<String>,
    pub support_args: Option<Vec<String>>,
    pub support_percent: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GrantRef {
    pub id: String,
    pub fingerprint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub name: String,
    pub hash: String,
    pub executable: bool,
    pub size: u64,
}

pub fn valid_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub fn fingerprint(request: &Enrollment, entries: &[Entry]) -> Result<String, String> {
    let data = serde_json::to_vec(&(env!("CARGO_PKG_VERSION"), request, entries))
        .map_err(|e| e.to_string())?;
    Ok(format!("{:x}", Sha256::digest(data)))
}

pub fn validate_request(request: &Enrollment) -> Result<(), String> {
    if request.profile_id.is_empty()
        || request.profile_id.len() > 128
        || request.profile_name.len() > 256
        || !request.executable.is_absolute()
        || request.user_args.is_empty()
        || request
            .user_args
            .iter()
            .chain(request.support_args.iter().flatten())
            .any(|a| a.contains('\0'))
        || !request.support_percent.is_finite()
        || !(0.0..=100.0).contains(&request.support_percent)
    {
        return Err("Invalid miner authorization request".into());
    }
    let expected = match request.engine.as_str() {
        "srbminer" => "SRBMiner-MULTI",
        "lolminer" => "lolMiner",
        "t-rex" | "trex" => "t-rex",
        "xmrig" => "xmrig",
        "bzminer" => "bzminer",
        "gminer" => "miner",
        "lpminer" => "lpminer",
        "rigel" => "rigel",
        "npminer" => "npminer",
        _ => return Err("Unattended root mode is unavailable for this mining engine".into()),
    };
    if request.executable.file_name().and_then(|s| s.to_str()) != Some(expected) {
        return Err(
            "Use the mining engine's original Linux executable for unattended root mode".into(),
        );
    }
    if serde_json::to_vec(request)
        .map_err(|e| e.to_string())?
        .len()
        > MAX_REQUEST_BYTES
    {
        return Err("Miner authorization request is too large".into());
    }
    Ok(())
}

fn openat(directory: &File, name: &std::ffi::OsStr, flags: i32) -> Result<File, String> {
    let name = CString::new(name.as_bytes()).map_err(|e| e.to_string())?;
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            flags | libc::O_CLOEXEC | libc::O_NOFOLLOW,
        )
    };
    if fd == -1 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}

/// Traverse through directory descriptors: a renamed directory or symlink race
/// cannot redirect the privileged copy into another part of the filesystem.
pub fn open_directory(path: &Path) -> Result<File, String> {
    if !path.is_absolute() {
        return Err("Expected an absolute miner directory".into());
    }
    let mut directory = File::open("/").map_err(|e| e.to_string())?;
    for part in path.components() {
        match part {
            Component::RootDir => {}
            Component::Normal(name) => {
                directory = openat(&directory, name, libc::O_RDONLY | libc::O_DIRECTORY)?
            }
            _ => return Err("Non-normal miner directory".into()),
        }
    }
    Ok(directory)
}

/// Scan or make a private snapshot. Source ownership is enforced during root
/// enrollment; all reads are O_NOFOLLOW and only regular files are accepted.
pub fn inventory(
    source: &Path,
    destination: Option<&Path>,
    owner: Option<u32>,
) -> Result<Vec<Entry>, String> {
    fn walk(
        directory: File,
        relative: &Path,
        destination: Option<&Path>,
        owner: Option<u32>,
        entries: &mut Vec<Entry>,
        total: &mut u64,
        depth: usize,
    ) -> Result<(), String> {
        if depth > 20 {
            return Err("Miner folder nesting exceeds the limit".into());
        }
        let metadata = directory.metadata().map_err(|e| e.to_string())?;
        if owner == Some(0) && metadata.mode() & 0o022 != 0 {
            return Err("Protected miner directory is writable by other users".into());
        }
        if owner.is_some_and(|uid| {
            metadata.uid() != uid
                && !(uid != 0 && metadata.uid() == 0 && metadata.mode() & 0o005 == 0o005)
        }) {
            return Err("The miner folder must belong to your account or be a readable root-owned folder. Download the engine through MinerDesk first.".into());
        }
        let mut names = std::fs::read_dir(format!("/proc/self/fd/{}", directory.as_raw_fd()))
            .map_err(|e| e.to_string())?
            .map(|e| e.map(|e| e.file_name()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        names.sort();
        for name in names {
            let child_relative = relative.join(&name);
            let mut file = openat(&directory, &name, libc::O_RDONLY | libc::O_NONBLOCK)?;
            let meta = file.metadata().map_err(|e| e.to_string())?;
            if owner == Some(0) && meta.mode() & 0o022 != 0 {
                return Err("Protected miner file is writable by other users".into());
            }
            let public = if meta.is_dir() {
                meta.mode() & 0o005 == 0o005
            } else {
                meta.mode() & 0o004 != 0
            };
            if owner
                .is_some_and(|uid| meta.uid() != uid && !(uid != 0 && meta.uid() == 0 && public))
            {
                return Err(
                    "Miner files must belong to your account or be readable root-owned files"
                        .into(),
                );
            }
            if meta.is_dir() {
                if let Some(target) = destination {
                    std::fs::create_dir(target.join(&child_relative)).map_err(|e| e.to_string())?;
                    std::fs::set_permissions(
                        target.join(&child_relative),
                        std::fs::Permissions::from_mode(0o700),
                    )
                    .map_err(|e| e.to_string())?;
                }
                walk(
                    file,
                    &child_relative,
                    destination,
                    owner,
                    entries,
                    total,
                    depth + 1,
                )?;
            } else if meta.is_file() {
                *total = total
                    .checked_add(meta.len())
                    .ok_or("Miner folder exceeds the size limit")?;
                if *total > MAX_BYTES || entries.len() >= MAX_FILES {
                    return Err(
                        "Miner folder exceeds the authorization size limit (2 GiB / 4096 files)"
                            .into(),
                    );
                }
                let executable = meta.permissions().mode() & 0o111 != 0;
                let mut output = if let Some(target) = destination {
                    let p = target.join(&child_relative);
                    let file = File::options()
                        .write(true)
                        .create_new(true)
                        .open(&p)
                        .map_err(|e| e.to_string())?;
                    file.set_permissions(std::fs::Permissions::from_mode(if executable {
                        0o700
                    } else {
                        0o600
                    }))
                    .map_err(|e| e.to_string())?;
                    Some(file)
                } else {
                    None
                };
                let mut hash = Sha256::new();
                let mut buffer = [0u8; 65536];
                let mut count = 0u64;
                loop {
                    let read = file.read(&mut buffer).map_err(|e| e.to_string())?;
                    if read == 0 {
                        break;
                    }
                    count += read as u64;
                    if count > meta.len() {
                        return Err("Miner files changed while authorizing; retry".into());
                    }
                    hash.update(&buffer[..read]);
                    if let Some(ref mut out) = output {
                        out.write_all(&buffer[..read]).map_err(|e| e.to_string())?;
                    }
                }
                if count != meta.len() {
                    return Err("Miner files changed while authorizing; retry".into());
                }
                if let Some(out) = output {
                    out.sync_all().map_err(|e| e.to_string())?;
                }
                entries.push(Entry {
                    name: child_relative
                        .to_str()
                        .ok_or("Miner filenames must be UTF-8")?
                        .into(),
                    hash: format!("{:x}", hash.finalize()),
                    executable,
                    size: count,
                });
            } else {
                return Err(
                    "Symlinks and special files are not allowed in an approved miner folder".into(),
                );
            }
        }
        Ok(())
    }
    let mut entries = vec![];
    let mut total = 0;
    walk(
        open_directory(source)?,
        Path::new(""),
        destination,
        owner,
        &mut entries,
        &mut total,
        0,
    )?;
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(entries)
}

pub fn request_fingerprint(request: &Enrollment) -> Result<String, String> {
    validate_request(request)?;
    let mut executable = File::options()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&request.executable)
        .map_err(|e| e.to_string())?;
    if executable.metadata().map_err(|e| e.to_string())?.mode() & 0o111 == 0 {
        return Err("The miner executable needs Linux execute permission".into());
    }
    let mut header = [0u8; 4];
    executable
        .read_exact(&mut header)
        .map_err(|e| e.to_string())?;
    if &header != b"\x7fELF" {
        return Err(
            "Unattended root mode requires the original ELF miner executable, not a script".into(),
        );
    }
    fingerprint(
        request,
        &inventory(
            request
                .executable
                .parent()
                .ok_or("Missing miner directory")?,
            None,
            Some(unsafe { libc::getuid() }),
        )?,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fingerprints_bind_commands_and_every_file() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("SRBMiner-MULTI");
        std::fs::write(&path, b"\x7fELFfixture").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut request = Enrollment {
            profile_id: "p".into(),
            profile_name: "Rig".into(),
            engine: "srbminer".into(),
            executable: path,
            user_args: vec!["--algorithm".into(), "pearlhash".into()],
            support_args: None,
            support_percent: 0.0,
        };
        let first = request_fingerprint(&request).unwrap();
        request.user_args.push("changed".into());
        assert_ne!(first, request_fingerprint(&request).unwrap());
        request.user_args.pop();
        std::fs::write(folder.path().join("lib.so"), b"dependency").unwrap();
        assert_ne!(first, request_fingerprint(&request).unwrap());
    }
    #[test]
    fn symlinks_special_files_and_parent_links_are_rejected() {
        let folder = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink("/etc/passwd", folder.path().join("link")).unwrap();
        assert!(inventory(folder.path(), None, None).is_err());
        let parent = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(folder.path(), parent.path().join("directory")).unwrap();
        assert!(open_directory(&parent.path().join("directory")).is_err());
        assert!(!valid_hex("../../etc", 32));
        assert!(valid_hex(&"a".repeat(32), 32));
    }
}
