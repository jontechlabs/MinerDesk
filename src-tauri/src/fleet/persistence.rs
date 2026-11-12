//! Local credentials are separate from AppConfig and never exported by DTOs.
use super::protocol::{CommandReceipt, FleetCommand};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct PendingInvitation {
    pub hash: String,
    pub expires_at: u64,
}
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct RegisteredRig {
    pub rig_id: String,
    pub name: String,
    pub agent_hash: String,
    pub controller_hash: String,
    #[serde(skip, default)]
    pub last_seen: u64,
    #[serde(skip, default)]
    pub snapshot: Value,
    #[serde(skip, default)]
    pub config: Value,
}
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct StoredCommand {
    pub command: FleetCommand,
    pub receipt: CommandReceipt,
}
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct HubRecord {
    pub enabled: bool,
    pub fleet_id: String,
    pub name: String,
    pub address: String,
    pub listen: String,
    pub certificate_pem: String,
    pub private_key_pem: String,
    pub fingerprint: String,
    pub invitations: Vec<PendingInvitation>,
    pub rigs: BTreeMap<String, RegisteredRig>,
    pub commands: BTreeMap<String, StoredCommand>,
}
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Membership {
    pub fleet_id: String,
    pub address: String,
    pub certificate_pem: String,
    pub fingerprint: String,
    pub agent_token: String,
    pub controller_token: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Executed {
    pub fingerprint: String,
    pub receipt: CommandReceipt,
    #[serde(default)]
    pub delivered: bool,
}
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Persistent {
    pub schema: u32,
    pub rig_id: String,
    pub name: String,
    pub hub: Option<HubRecord>,
    pub member: Option<Membership>,
    pub executed: BTreeMap<String, Executed>,
}

pub(crate) struct Store {
    pub path: PathBuf,
    pub data: Persistent,
}
impl Store {
    pub fn load(config_path: &Path) -> Result<Self, String> {
        let parent = config_path
            .parent()
            .ok_or("Missing configuration directory")?;
        let directory = parent.join("fleet-private");
        private_directory(&directory)?;
        let path = directory.join("state.json");
        let data = if path.exists() {
            let meta = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
            if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > 16 * 1024 * 1024 {
                return Err("Unsafe or oversized Fleet state file".into());
            }
            #[cfg(windows)]
            protect_windows_path(&path, false)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if meta.permissions().mode() & 0o077 != 0 {
                    return Err("Fleet credentials require mode 0600".into());
                }
            }
            let parsed: Persistent = serde_json::from_slice(
                &fs::read(&path).map_err(|e| e.to_string())?,
            )
            .map_err(|_| {
                "Fleet state is invalid; restore its backup instead of resetting identities"
                    .to_string()
            })?;
            if parsed.schema != 1 {
                return Err("Unsupported Fleet state version".into());
            }
            parsed
        } else {
            Persistent {
                schema: 1,
                rig_id: uuid::Uuid::new_v4().to_string(),
                name: local_name(),
                hub: None,
                member: None,
                executed: BTreeMap::new(),
            }
        };
        let store = Self { path, data };
        store.save(&store.data)?;
        Ok(store)
    }
    pub fn save(&self, data: &Persistent) -> Result<(), String> {
        let encoded = serde_json::to_vec(data).map_err(|e| e.to_string())?;
        if encoded.len() > 16 * 1024 * 1024 {
            return Err(
                "Fleet state reached its 16 MiB storage limit; no change was applied".into(),
            );
        }
        let directory = self.path.parent().ok_or("Missing Fleet directory")?;
        let metadata = fs::symlink_metadata(directory).map_err(|e| e.to_string())?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err("Unsafe Fleet credential directory".into());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if metadata.permissions().mode() & 0o077 != 0 {
                return Err("Fleet credential directory permissions changed; restart MinerDesk to repair them".into());
            }
        }
        let mut temporary =
            tempfile::NamedTempFile::new_in(directory).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            temporary
                .as_file()
                .set_permissions(fs::Permissions::from_mode(0o600))
                .map_err(|e| e.to_string())?;
        }
        temporary.write_all(&encoded).map_err(|e| e.to_string())?;
        temporary.as_file().sync_all().map_err(|e| e.to_string())?;
        temporary
            .persist(&self.path)
            .map_err(|e| format!("Saving Fleet state: {}", e.error))?;
        #[cfg(unix)]
        {
            fs::File::open(directory)
                .and_then(|d| d.sync_all())
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }
    /// A failed disk write never publishes the corresponding in-memory state.
    pub fn transaction<T>(
        &mut self,
        change: impl FnOnce(&mut Persistent) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut updated = self.data.clone();
        let result = change(&mut updated)?;
        self.save(&updated)?;
        self.data = updated;
        Ok(result)
    }
}
fn local_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "MinerDesk rig".into())
        .chars()
        .take(80)
        .collect()
}
fn private_directory(path: &Path) -> Result<(), String> {
    if path.exists() {
        let meta = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
        if !meta.is_dir() || meta.file_type().is_symlink() {
            return Err("Fleet credential directory must be a real directory".into());
        }
    } else {
        fs::create_dir(path).map_err(|e| e.to_string())?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
    }
    #[cfg(windows)]
    protect_windows_path(path, true)?;
    Ok(())
}
#[cfg(windows)]
fn protect_windows_path(path: &Path, directory: bool) -> Result<(), String> {
    use std::{
        os::windows::{ffi::OsStrExt, process::CommandExt},
        process::Command,
        ptr,
    };
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::Authorization::{
            ConvertStringSecurityDescriptorToSecurityDescriptorW, SetNamedSecurityInfoW,
            SE_FILE_OBJECT,
        },
        Security::{
            GetSecurityDescriptorDacl, DACL_SECURITY_INFORMATION,
            PROTECTED_DACL_SECURITY_INFORMATION,
        },
    };
    let output = Command::new("whoami.exe")
        .args(["/user", "/fo", "csv", "/nh"])
        .creation_flags(0x08000000)
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(
            "Cannot identify the current Windows account for Fleet credential protection".into(),
        );
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let sid = text
        .trim()
        .rsplit(',')
        .next()
        .unwrap_or("")
        .trim_matches('"');
    if !sid.starts_with("S-1-")
        || !sid
            .bytes()
            .all(|b| b.is_ascii_digit() || b == b'S' || b == b'-')
    {
        return Err("Invalid Windows account identity".into());
    }
    // Replace the entire DACL, including any previous explicit grants. Appending
    // allow entries or merely disabling inheritance would leave old grants alive.
    let inherit = if directory { "OICI" } else { "" };
    let sddl: Vec<u16> = format!("D:P(A;{inherit};FA;;;{sid})(A;{inherit};FA;;;SY)")
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let mut descriptor = ptr::null_mut();
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            1,
            &mut descriptor,
            ptr::null_mut(),
        )
    } == 0
    {
        return Err("Cannot create private Windows Fleet credential ACL".into());
    }
    let result = (|| {
        let mut present = 0;
        let mut defaulted = 0;
        let mut dacl = ptr::null_mut();
        if unsafe { GetSecurityDescriptorDacl(descriptor, &mut present, &mut dacl, &mut defaulted) }
            == 0
            || present == 0
            || dacl.is_null()
        {
            return Err("Invalid private Windows credential DACL".into());
        }
        let name: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let status = unsafe {
            SetNamedSecurityInfoW(
                name.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                dacl,
                ptr::null_mut(),
            )
        };
        if status != 0 {
            return Err(format!(
                "Cannot protect Fleet credential ACL (Windows error {status})"
            ));
        }
        Ok(())
    })();
    unsafe { LocalFree(descriptor) };
    result
}
