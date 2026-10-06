//! Application updates are desktop-only. The web API exposes notification data,
//! never an installer command, URL override, public key override or arbitrary file.
use serde::{Deserialize, Serialize};
use std::{sync::{atomic::{AtomicBool, Ordering}, Mutex, OnceLock}, time::{Duration, Instant}};
use tauri::{ipc::Channel, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

pub const RELEASES: &str = "https://github.com/jontechlabs/MinerDesk/releases/latest";
const API: &str = "https://api.github.com/repos/jontechlabs/MinerDesk/releases/latest";
const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

#[derive(Clone, Debug, Serialize)]
pub struct UpdateInfo {
    pub current_version: String,
    pub version: Option<String>,
    pub notes: String,
    pub release_url: String,
    pub can_install: bool,
    pub reason: String,
}
#[derive(Clone, Deserialize)]
struct Release { tag_name: String, html_url: String, #[serde(default)] body: Option<String>, draft: bool, prerelease: bool }
#[derive(Default)]
struct ReleaseCache { checked: Option<Instant>, result: Option<Result<Option<Release>, String>> }
static CACHE: OnceLock<tokio::sync::Mutex<ReleaseCache>> = OnceLock::new();

fn release_version(release: &Release, current: &str) -> Result<Option<String>, String> {
    if release.draft || release.prerelease { return Ok(None); }
    let version = semver::Version::parse(release.tag_name.trim_start_matches('v')).map_err(|_| "Invalid release version")?;
    if !version.pre.is_empty() || !version.build.is_empty() { return Ok(None); }
    let expected = format!("https://github.com/jontechlabs/MinerDesk/releases/tag/v{version}");
    if release.html_url != expected { return Err("Release URL does not belong to MinerDesk".into()); }
    let current = semver::Version::parse(current).map_err(|_| "Invalid installed version")?;
    Ok((version > current).then(|| version.to_string()))
}

async fn fetch_release() -> Result<Option<Release>, String> {
    let client = reqwest::Client::builder().timeout(Duration::from_secs(20)).https_only(true)
        .user_agent(concat!("MinerDesk/", env!("CARGO_PKG_VERSION"))).build().map_err(|e| e.to_string())?;
    let mut response = client.get(API).header("Accept", "application/vnd.github+json").send().await.map_err(|e| e.to_string())?;
    if response.status() == reqwest::StatusCode::NOT_FOUND { return Ok(None); }
    response.error_for_status_ref().map_err(|e| e.to_string())?;
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
        if body.len() + chunk.len() > 1024 * 1024 { return Err("Release metadata too large".into()); }
        body.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&body).map(Some).map_err(|e| e.to_string())
}

pub async fn notification(force: bool) -> Result<UpdateInfo, String> {
    // Shared by all connected web clients. Manual checks cannot flood GitHub.
    let mut cache = CACHE.get_or_init(Default::default).lock().await;
    let age = cache.checked.map(|t| t.elapsed());
    let ttl = if force || matches!(cache.result, Some(Err(_))) { Duration::from_secs(60) } else { CHECK_INTERVAL };
    if age.is_none_or(|age| age >= ttl) {
        cache.result = Some(fetch_release().await);
        cache.checked = Some(Instant::now());
    }
    let release = cache.result.clone().unwrap_or(Ok(None))?;
    let version = match &release { Some(r) => release_version(r, env!("CARGO_PKG_VERSION"))?, None => None };
    Ok(UpdateInfo { current_version: env!("CARGO_PKG_VERSION").into(), version,
        notes: release.as_ref().and_then(|r| r.body.clone()).unwrap_or_default().chars().take(8000).collect(),
        release_url: release.map(|r| r.html_url).unwrap_or_else(|| RELEASES.into()),
        can_install: false, reason: "web".into() })
}

#[derive(Default)]
pub struct DesktopUpdates { pending: Mutex<Option<Update>>, busy: AtomicBool }
struct BusyGuard<'a>(&'a AtomicBool);
impl Drop for BusyGuard<'_> { fn drop(&mut self) { self.0.store(false, Ordering::SeqCst); } }

fn install_target(app: &tauri::AppHandle) -> Result<&'static str, &'static str> {
    if cfg!(debug_assertions) { return Err("development"); }
    if std::env::consts::ARCH != "x86_64" { return Err("platform"); }
    #[cfg(target_os = "windows")]
    {
        // A portable executable must not accidentally update a different install.
        let script = r#"$ErrorActionPreference='Stop'; [Console]::OutputEncoding=[Text.UTF8Encoding]::new($false); $p=Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\MinerDesk' -ErrorAction Stop; [Console]::Write($p.InstallLocation)"#;
        let output = super::background_command("powershell.exe").args(["-NoLogo", "-NoProfile", "-Command", script]).output().map_err(|_| "portable")?;
        let installed = std::path::PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
        let exe = std::env::current_exe().map_err(|_| "portable")?;
        if !output.status.success() || installed.as_os_str().is_empty() { return Err("portable"); }
        let installed = installed.canonicalize().map_err(|_| "portable")?;
        let parent = exe.parent().ok_or("portable")?.canonicalize().map_err(|_| "portable")?;
        if installed.to_string_lossy().to_lowercase() != parent.to_string_lossy().to_lowercase() { return Err("portable"); }
        return Ok("windows-x86_64");
    }
    #[cfg(target_os = "linux")]
    {
        if tauri::utils::platform::bundle_type() == Some(tauri::utils::config::BundleType::Deb) {
            let exe = std::env::current_exe().map_err(|_| "package")?.canonicalize().map_err(|_| "package")?;
            let installed = std::path::Path::new("/usr/bin/minerdesk").canonicalize().map_err(|_| "package")?;
            if exe != installed || !std::path::Path::new("/usr/bin/pkexec").is_file() || !std::path::Path::new("/usr/bin/dpkg").is_file() {
                return Err("package");
            }
            let package = std::process::Command::new("/usr/bin/dpkg-query")
                .args(["--show", "--showformat=${Status} ${Version}", "miner-desk"]).output().map_err(|_| "package")?;
            if !package.status.success() || String::from_utf8_lossy(&package.stdout) != format!("install ok installed {}", env!("CARGO_PKG_VERSION")) { return Err("package"); }
            return Ok("linux-x86_64-deb");
        }
        let path = std::path::PathBuf::from(app.env().appimage.ok_or("package")?);
        let metadata = std::fs::metadata(&path).map_err(|_| "readonly")?;
        let parent = path.parent().ok_or("readonly")?;
        if metadata.permissions().readonly() || !metadata.is_file() { return Err("readonly"); }
        // Atomic replacement needs permission to create a file in this directory.
        let probe = parent.join(format!(".minerdesk-update-probe-{}", uuid::Uuid::new_v4()));
        let file = std::fs::OpenOptions::new().write(true).create_new(true).open(&probe).map_err(|_| "readonly")?;
        drop(file);
        std::fs::remove_file(probe).map_err(|_| "readonly")?;
        return Ok("linux-x86_64-appimage");
    }
    #[allow(unreachable_code)]
    { let _ = app; Err("platform") }
}

fn valid_artifact_url(url: &str, version: &str, target: &str) -> bool {
    let name = match target {
        "windows-x86_64" => format!("MinerDesk_{version}_x64-setup.exe"),
        "linux-x86_64-appimage" => format!("MinerDesk_{version}_amd64.AppImage"),
        "linux-x86_64-deb" => format!("MinerDesk_{version}_amd64.deb"),
        _ => return false,
    };
    url == format!("https://github.com/jontechlabs/MinerDesk/releases/download/v{version}/{name}")
}

#[tauri::command]
pub async fn check_app_update(app: tauri::AppHandle, state: tauri::State<'_, DesktopUpdates>, force: bool) -> Result<UpdateInfo, String> {
    if state.busy.swap(true, Ordering::SeqCst) { return Err("An update is already in progress".into()); }
    let _guard = BusyGuard(&state.busy);
    let mut info = notification(force).await?;
    *state.pending.lock().map_err(|_| "Update lock unavailable")? = None;
    let target = match install_target(&app) { Ok(t) => t, Err(reason) => { info.reason = reason.into(); return Ok(info); } };
    let Some(version) = &info.version else { info.reason = "supported".into(); return Ok(info); };
    let endpoint = format!("https://github.com/jontechlabs/MinerDesk/releases/download/v{version}/latest.json");
    let checked = app.updater_builder().target(target).endpoints(vec![endpoint.parse().map_err(|_| "Invalid update endpoint")?])
        .map_err(|e| e.to_string())?.timeout(Duration::from_secs(20)).build().map_err(|e| e.to_string())?.check().await;
    match checked {
        Ok(Some(mut update)) if update.version == *version && valid_artifact_url(update.download_url.as_str(), version, target) => {
            // Keep metadata checks short, but allow a full package to download.
            update.timeout = Some(Duration::from_secs(15 * 60));
            *state.pending.lock().map_err(|_| "Update lock unavailable")? = Some(update);
            info.can_install = true;
            info.reason = "supported".into();
        }
        _ => info.reason = "unavailable".into(),
    }
    Ok(info)
}

#[derive(Clone, Serialize)]
pub struct UpdateProgress { stage: &'static str, downloaded: u64, total: Option<u64> }

#[tauri::command]
pub async fn install_app_update(app: tauri::AppHandle, state: tauri::State<'_, DesktopUpdates>, core: tauri::State<'_, std::sync::Arc<super::CoreState>>, version: String, progress: Channel<UpdateProgress>) -> Result<(), String> {
    if state.busy.swap(true, Ordering::SeqCst) { return Err("An update is already in progress".into()); }
    let _guard = BusyGuard(&state.busy);
    let target = install_target(&app).map_err(|_| "Use the GitHub download page for this installation")?;
    let update = state.pending.lock().map_err(|_| "Update lock unavailable")?.clone().ok_or("Check for an update first")?;
    if update.version != version || !valid_artifact_url(update.download_url.as_str(), &version, target) { return Err("Update changed; check again".into()); }
    let mut downloaded = 0u64;
    let bytes = update.download(|length, total| {
        downloaded += length as u64;
        let _ = progress.send(UpdateProgress { stage: "downloading", downloaded, total });
    }, || {}).await.map_err(|e| format!("Download/signature verification failed: {e}"))?;
    // Only verified bytes reach installation. No miner is stopped on download failure.
    let _ = progress.send(UpdateProgress { stage: "installing", downloaded, total: Some(downloaded) });
    #[cfg(target_os = "windows")]
    {
        let port = super::CoreState::load()?.config().web.desktop_api_port;
        // Suppress heartbeats/automatic backend starts during the handover.
        super::DESKTOP_EXIT_SHUTDOWN_STARTED.store(true, Ordering::SeqCst);
        if super::backend_health_once(port) {
            if let Err(e) = super::request_backend_shutdown(port) {
                super::DESKTOP_EXIT_SHUTDOWN_STARTED.store(false, Ordering::SeqCst);
                return Err(format!("Cannot safely stop the backend: {e}"));
            }
            for _ in 0..50 {
                if !super::backend_health_once(port) { break; }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            if super::backend_health_once(port) {
                super::DESKTOP_EXIT_SHUTDOWN_STARTED.store(false, Ordering::SeqCst);
                return Err("Backend is still running; update cancelled".into());
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    { core.cancel_power_action(); core.stop_all(); }
    #[cfg(target_os = "linux")]
    let result = if target == "linux-x86_64-deb" {
        install_deb_verified(&bytes)
    } else { update.install(bytes).map_err(|e| e.to_string()) };
    #[cfg(not(target_os = "linux"))]
    let result = update.install(bytes).map_err(|e| e.to_string());
    if let Err(e) = result {
        #[cfg(target_os = "windows")]
        super::DESKTOP_EXIT_SHUTDOWN_STARTED.store(false, Ordering::SeqCst);
        #[cfg(not(target_os = "windows"))]
        core.shutting_down.store(false, Ordering::SeqCst);
        return Err(format!("Installation failed; restart mining manually if needed: {e}"));
    }
    let _ = &core;
    #[cfg(not(target_os = "windows"))]
    app.restart();
    Ok(())
}

#[cfg(target_os = "linux")]
fn install_deb_verified(bytes: &[u8]) -> Result<(), String> {
    // Delegate authentication to the OS; never ask for or retain a sudo password.
    if !bytes.starts_with(b"!<arch>\n") { return Err("Invalid Debian package".into()); }
    let directory = tempfile::Builder::new().prefix("minerdesk-update-").tempdir().map_err(|e| e.to_string())?;
    let package = directory.path().join("MinerDesk.deb");
    std::fs::write(&package, bytes).map_err(|e| e.to_string())?;
    let status = std::process::Command::new("/usr/bin/pkexec").args(["/usr/bin/dpkg", "-i"]).arg(package).status().map_err(|e| e.to_string())?;
    if status.success() { Ok(()) } else { Err("The system package installer failed or administrator approval was cancelled".into()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn release(tag: &str) -> Release { Release { tag_name: tag.into(), html_url: format!("https://github.com/jontechlabs/MinerDesk/releases/tag/{tag}"), body: None, draft: false, prerelease: false } }
    #[test] fn stable_versions_only_no_downgrades() {
        assert_eq!(release_version(&release("v0.7.25"), "0.7.24").unwrap(), Some("0.7.25".into()));
        assert!(release_version(&release("v0.7.24"), "0.7.24").unwrap().is_none());
        assert!(release_version(&release("v0.7.23"), "0.7.24").unwrap().is_none());
        assert!(release_version(&release("v0.8.0-rc.1"), "0.7.24").unwrap().is_none());
        assert!(release_version(&release("garbage"), "0.7.24").is_err());
    }
    #[test] fn draft_prerelease_and_foreign_urls_rejected() {
        let mut r = release("v0.8.0"); r.draft = true;
        assert!(release_version(&r,"0.7.24").unwrap().is_none());
        r.draft = false; r.prerelease = true;
        assert!(release_version(&r,"0.7.24").unwrap().is_none());
        r.prerelease = false; r.html_url = "https://example.com".into();
        assert!(release_version(&r,"0.7.24").is_err());
    }
    #[test] fn exact_release_and_platform_artifact_required() {
        let good = "https://github.com/jontechlabs/MinerDesk/releases/download/v0.7.25/MinerDesk_0.7.25_x64-setup.exe";
        assert!(valid_artifact_url(good,"0.7.25","windows-x86_64"));
        for bad in [format!("{good}?redirect=1"), good.replace("jontechlabs","evil"), good.replace("https:","http:"), good.replace("0.7.25", "0.7.24")] {
            assert!(!valid_artifact_url(&bad,"0.7.25","windows-x86_64"));
        }
        assert!(!valid_artifact_url(good,"0.7.25","linux-x86_64-appimage"));
        assert!(valid_artifact_url("https://github.com/jontechlabs/MinerDesk/releases/download/v0.7.25/MinerDesk_0.7.25_amd64.deb","0.7.25","linux-x86_64-deb"));
    }
}
