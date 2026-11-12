//! The remote caller may choose a version, never a publisher, URL, command or identity.
//! The privileged executable obtains identity from the OS and repeats verification.
use base64::{engine::general_purpose::STANDARD, Engine};
use minisign_verify::{PublicKey, Signature};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io::{BufRead, Read},
    path::{Path, PathBuf},
    time::Duration,
};

pub const UPDATER_PUBLIC_KEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IEU1NzgyOEMyNEJGMTIwRjQKUldUMElQRkx3aWg0NWZ0M2ZiakE1UFNOSHpZZDR0OHpaQlhZb1NQUnhsMml3U0FrUk9xczZVVEUK";
pub const RELEASE_API: &str = "https://api.github.com/repos/jontechlabs/MinerDesk/releases/latest";
pub const ROOT_HELPER: &str = "/usr/lib/minerdesk/minerdesk-update-helper";
pub const REQUEST_LIMIT: usize = 8192;
pub const PACKAGE_LIMIT: u64 = 1024 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: u32,
    pub job_id: String,
    pub expected_version: String,
    pub package_path: PathBuf,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ack {
    pub schema: u32,
    pub job_id: String,
    pub apply: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerEvent {
    pub schema: u32,
    pub job_id: String,
    pub stage: String,
    pub error: Option<String>,
}

pub fn valid_job_id(id: &str) -> bool {
    id.len() == 36 && uuid::Uuid::parse_str(id).is_ok_and(|v| v.hyphenated().to_string() == id)
}
pub fn stable_version(version: &str) -> Result<semver::Version, String> {
    let v = semver::Version::parse(version).map_err(|_| "Invalid stable version")?;
    if !v.pre.is_empty() || !v.build.is_empty() || v.to_string() != version {
        return Err("A canonical stable version is required".into());
    }
    Ok(v)
}
impl Request {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != 1 || !valid_job_id(&self.job_id) {
            return Err("Invalid update request".into());
        }
        stable_version(&self.expected_version)?;
        if !self.package_path.is_absolute()
            || self.package_path.file_name() != Some(std::ffi::OsStr::new("package"))
            || self.package_path.as_os_str().len() > 4096
            || self.package_path.components().any(|c| {
                matches!(
                    c,
                    std::path::Component::ParentDir | std::path::Component::CurDir
                )
            })
        {
            return Err("Invalid downloaded package path".into());
        }
        Ok(())
    }
}
pub fn read_message<T: serde::de::DeserializeOwned>(
    reader: &mut impl BufRead,
) -> Result<T, String> {
    let mut bytes = Vec::new();
    reader
        .take((REQUEST_LIMIT + 1) as u64)
        .read_until(b'\n', &mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.is_empty() || bytes.len() > REQUEST_LIMIT || !bytes.ends_with(b"\n") {
        return Err("Missing or oversized worker message".into());
    }
    serde_json::from_slice(&bytes).map_err(|_| "Invalid worker message".into())
}

pub fn valid_artifact_url(url: &str, version: &str, target: &str) -> bool {
    if stable_version(version).is_err() {
        return false;
    }
    let name = match target {
        "windows-x86_64" => format!("MinerDesk_{version}_x64-setup.exe"),
        "linux-x86_64-deb" => format!("MinerDesk_{version}_amd64.deb"),
        "linux-x86_64-appimage" => format!("MinerDesk_{version}_amd64.AppImage"),
        _ => return false,
    };
    url == format!("https://github.com/jontechlabs/MinerDesk/releases/download/v{version}/{name}")
}
fn decode(value: &str) -> Result<String, String> {
    String::from_utf8(
        STANDARD
            .decode(value.trim())
            .map_err(|_| "Invalid signature encoding")?,
    )
    .map_err(|_| "Invalid signature encoding".into())
}
pub fn verify_signature(bytes: &[u8], signature: &str) -> Result<(), String> {
    verify_with_key(bytes, signature, UPDATER_PUBLIC_KEY).map(|_| ())
}
fn verify_with_key(bytes: &[u8], signature: &str, key: &str) -> Result<String, String> {
    let key = PublicKey::decode(&decode(key)?).map_err(|_| "Invalid embedded publisher key")?;
    let signature =
        Signature::decode(&decode(signature)?).map_err(|_| "Invalid update signature")?;
    key.verify(bytes, &signature, true)
        .map_err(|_| "Update publisher signature does not match")?;
    Ok(signature.trusted_comment().to_owned())
}
pub fn verify_package(bytes: &[u8], signature: &str, version: &str) -> Result<(), String> {
    let comment = verify_with_key(bytes, signature, UPDATER_PUBLIC_KEY)?;
    require_signed_version(&comment, version)
}
fn require_signed_version(comment: &str, version: &str) -> Result<(), String> {
    stable_version(version)?;
    let signed = comment
        .split('\t')
        .find_map(|x| x.strip_prefix("version:"))
        .ok_or("Remote updates require a signature with an authenticated version")?;
    if signed.trim_start_matches('v') != version {
        return Err("Signed update version differs from selected version".into());
    }
    Ok(())
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    html_url: String,
    draft: bool,
    prerelease: bool,
}
#[derive(Clone, Debug, Deserialize)]
pub struct Artifact {
    pub url: String,
    pub signature: String,
}
#[derive(Deserialize)]
struct Manifest {
    version: String,
    platforms: BTreeMap<String, Artifact>,
}
pub fn official_artifact(
    expected: &str,
    installed: &str,
    target: &str,
) -> Result<Artifact, String> {
    let expected_v = stable_version(expected)?;
    if expected_v <= stable_version(installed)? {
        return Err("Update must be newer than the installed version".into());
    }
    let client = reqwest::blocking::Client::builder()
        .https_only(true)
        .timeout(Duration::from_secs(30))
        .user_agent(concat!(
            "MinerDesk-update-helper/",
            env!("CARGO_PKG_VERSION")
        ))
        .build()
        .map_err(|e| e.to_string())?;
    let release: Release = serde_json::from_slice(&fetch(&client, RELEASE_API)?)
        .map_err(|_| "Invalid official release")?;
    validate_release(&release, expected)?;
    let manifest: Manifest = serde_json::from_slice(&fetch(
        &client,
        &format!(
            "https://github.com/jontechlabs/MinerDesk/releases/download/v{expected}/latest.json"
        ),
    )?)
    .map_err(|_| "Invalid official update manifest")?;
    let artifact = manifest
        .platforms
        .get(target)
        .ok_or("Official package is unavailable for this installation")?;
    if manifest.version != expected
        || !valid_artifact_url(&artifact.url, expected, target)
        || artifact.signature.len() > 16384
    {
        return Err("Official manifest does not match the selected package".into());
    }
    Ok(artifact.clone())
}
fn validate_release(release: &Release, expected: &str) -> Result<(), String> {
    if release.draft
        || release.prerelease
        || release.tag_name != format!("v{expected}")
        || release.html_url
            != format!("https://github.com/jontechlabs/MinerDesk/releases/tag/v{expected}")
    {
        return Err("Selected version is no longer the official latest stable release".into());
    }
    Ok(())
}
fn fetch(client: &reqwest::blocking::Client, url: &str) -> Result<Vec<u8>, String> {
    let response = client
        .get(url)
        .send()
        .and_then(|r| r.error_for_status())
        .map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    response
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 1024 * 1024 {
        return Err("Official metadata exceeds size limit".into());
    }
    Ok(bytes)
}

#[cfg(unix)]
pub mod unix;
#[cfg(windows)]
pub mod windows;
#[cfg(unix)]
pub use unix::{enrolled, helper_path, progress_path, target_and_reason};
#[cfg(windows)]
pub use windows::{enrolled, helper_path, progress_path, target_and_reason};
pub fn run() -> Result<(), String> {
    #[cfg(unix)]
    {
        unix::run()
    }
    #[cfg(windows)]
    {
        windows::run()
    }
}

/// Once the original agent has exited, every ordinary error must still reach
/// its relaunch attempt. Keep status writes inside these captured operations.
pub(crate) fn finish_after_parent_exit(
    install: impl FnOnce() -> Result<(), String>,
    report_restarting: impl FnOnce() -> Result<(), String>,
    relaunch: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    let installed = install();
    let reported = report_restarting();
    let restarted = relaunch();
    let mut errors = Vec::new();
    if let Err(error) = installed {
        errors.push(error);
    }
    if let Err(error) = restarted {
        errors.push(format!("Original agent restart failed: {error}"));
    }
    if let Err(error) = reported {
        errors.push(format!("Restart progress could not be saved: {error}"));
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

pub(crate) fn regular_bytes(path: &Path) -> Result<Vec<u8>, String> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options
        .open(path)
        .map_err(|e| format!("Cannot open update package: {e}"))?;
    let meta = file.metadata().map_err(|e| e.to_string())?;
    if !meta.is_file() || meta.len() == 0 || meta.len() > PACKAGE_LIMIT {
        return Err("Update package must be a bounded regular file".into());
    }
    let mut bytes = Vec::new();
    file.take(PACKAGE_LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 != meta.len() {
        return Err("Update package changed during staging".into());
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    const KEY: &str = include_str!("../tests/fixtures/test.pub");
    const DATA: &[u8] = include_bytes!("../tests/fixtures/payload.txt");
    const SIGNATURE: &str = include_str!("../tests/fixtures/payload.txt.versioned.sig");
    const LEGACY: &str = include_str!("../tests/fixtures/payload.txt.sig");
    #[test]
    fn tauri_signed_bytes_and_authenticated_version_are_required() {
        let comment = verify_with_key(DATA, SIGNATURE, KEY).unwrap();
        require_signed_version(&comment, "0.8.3").unwrap();
        assert!(require_signed_version(&comment, "0.8.4").is_err());
        assert!(
            require_signed_version(&verify_with_key(DATA, LEGACY, KEY).unwrap(), "0.8.3").is_err()
        );
        assert!(verify_with_key(b"tampered", SIGNATURE, KEY).is_err());
        assert!(
            verify_signature(DATA, SIGNATURE).is_err(),
            "test key must never be a production publisher"
        );
        let forged = decode(SIGNATURE)
            .unwrap()
            .replace("version:0.8.3", "version:0.8.4");
        assert!(
            verify_with_key(DATA, &STANDARD.encode(forged), KEY).is_err(),
            "global signature must authenticate version comment"
        );
        assert!(verify_signature(DATA, "not-base64").is_err());
    }
    #[test]
    fn publisher_and_platform_url_cannot_be_overridden() {
        let url = "https://github.com/jontechlabs/MinerDesk/releases/download/v0.8.3/MinerDesk_0.8.3_amd64.deb";
        assert!(valid_artifact_url(url, "0.8.3", "linux-x86_64-deb"));
        for bad in [
            url.replace("jontechlabs", "attacker"),
            format!("{url}?download=1"),
            url.replace("https:", "http:"),
            url.replace("v0.8.3/", "v0.8.2/"),
            url.replace("amd64.deb", "amd64.AppImage"),
        ] {
            assert!(!valid_artifact_url(&bad, "0.8.3", "linux-x86_64-deb"));
        }
        for bad in ["0.8.3-rc.1", "0.8.3+build", "v0.8.3", "0.8.3\n"] {
            assert!(stable_version(bad).is_err());
        }
    }
    #[test]
    fn release_must_still_be_the_frozen_latest_stable() {
        let mut r = Release {
            tag_name: "v0.8.3".into(),
            html_url: "https://github.com/jontechlabs/MinerDesk/releases/tag/v0.8.3".into(),
            draft: false,
            prerelease: false,
        };
        assert!(validate_release(&r, "0.8.3").is_ok());
        assert!(validate_release(&r, "0.8.4").is_err());
        r.draft = true;
        assert!(validate_release(&r, "0.8.3").is_err());
        r.draft = false;
        r.prerelease = true;
        assert!(validate_release(&r, "0.8.3").is_err());
        r.prerelease = false;
        r.html_url = "https://example.com".into();
        assert!(validate_release(&r, "0.8.3").is_err());
    }
    #[test]
    fn wire_protocol_is_bounded_and_has_no_privileged_override_fields() {
        let good = format!("{{\"schema\":1,\"job_id\":\"ad243da8-6f11-4c19-b7b4-165aec9b11d8\",\"expected_version\":\"0.8.3\",\"package_path\":{}}}", serde_json::to_string(&std::env::temp_dir().join("package")).unwrap());
        read_message::<Request>(&mut Cursor::new(format!("{good}\n")))
            .unwrap()
            .validate()
            .unwrap();
        for field in [
            "url",
            "pubkey",
            "pid",
            "user",
            "executable",
            "progress_path",
            "command",
        ] {
            let bad = good.replacen("{", &format!("{{\"{field}\":\"arbitrary\","), 1);
            assert!(read_message::<Request>(&mut Cursor::new(format!("{bad}\n"))).is_err());
        }
        assert!(read_message::<Request>(&mut Cursor::new(good)).is_err());
        assert!(read_message::<Request>(&mut Cursor::new(vec![b' '; REQUEST_LIMIT + 1])).is_err());
        assert!(!valid_job_id("../../etc/shadow"));
        let mut request = Request {
            schema: 1,
            job_id: uuid::Uuid::new_v4().to_string(),
            expected_version: "0.8.3".into(),
            package_path: "relative/package".into(),
        };
        assert!(request.validate().is_err());
        request.package_path = std::env::temp_dir().join("../package");
        assert!(request.validate().is_err());
    }
    #[test]
    fn only_regular_bounded_downloads_are_read() {
        let temp = tempfile::tempdir().unwrap();
        let p = temp.path().join("package");
        std::fs::write(&p, DATA).unwrap();
        assert_eq!(regular_bytes(&p).unwrap(), DATA);
        assert!(regular_bytes(temp.path()).is_err());
        std::fs::write(&p, []).unwrap();
        assert!(regular_bytes(&p).is_err());
        #[cfg(unix)]
        {
            let link = temp.path().join("link");
            std::os::unix::fs::symlink(&p, &link).unwrap();
            assert!(regular_bytes(&link).is_err());
        }
    }
    #[test]
    fn failed_installing_progress_still_relaunches_the_original_agent() {
        let temp = tempfile::tempdir().unwrap();
        let missing_status = temp.path().join("missing/status.json");
        let installer = temp.path().join("fake-installer-ran");
        let relaunched = temp.path().join("fake-relauncher-ran");
        let result = finish_after_parent_exit(
            || {
                // Fault injection: the first post-exit status write fails,
                // before the fake installer gets a chance to run.
                std::fs::write(&missing_status, b"installing")
                    .map_err(|_| "status write failed".to_string())?;
                std::fs::write(&installer, b"fixture only").map_err(|e| e.to_string())
            },
            || {
                std::fs::write(&missing_status, b"restarting")
                    .map_err(|_| "status write failed".to_string())
            },
            || std::fs::write(&relaunched, b"fixture only").map_err(|e| e.to_string()),
        );
        assert!(result.unwrap_err().contains("status write failed"));
        assert!(!installer.exists());
        assert!(relaunched.is_file());
    }
    #[test]
    fn failed_installer_and_progress_preserve_the_relaunch_attempt_and_errors() {
        let temp = tempfile::tempdir().unwrap();
        let trace = std::cell::RefCell::new(Vec::new());
        let result = finish_after_parent_exit(
            || {
                trace.borrow_mut().push("install");
                Err("fake installer failed".into())
            },
            || {
                trace.borrow_mut().push("report");
                Err("fake status failed".into())
            },
            || {
                trace.borrow_mut().push("relaunch");
                std::fs::write(temp.path().join("relaunch-attempted"), b"fixture only").unwrap();
                Err("fake relaunch failed".into())
            },
        )
        .unwrap_err();
        assert_eq!(*trace.borrow(), ["install", "report", "relaunch"]);
        assert!(temp.path().join("relaunch-attempted").exists());
        for expected in [
            "fake installer failed",
            "fake status failed",
            "fake relaunch failed",
        ] {
            assert!(result.contains(expected));
        }
    }
}
