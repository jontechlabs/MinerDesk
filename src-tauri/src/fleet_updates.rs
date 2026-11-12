//! Local opt-in and asynchronous, signed Fleet update jobs. Configuration PUTs
//! cannot grant this capability: its policy is a separate local-only store.
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const CHECK_INTERVAL: Duration = Duration::from_secs(60);
const RELEASE_API: &str = "https://api.github.com/repos/jontechlabs/MinerDesk/releases/latest";
const MAX_METADATA: usize = 1024 * 1024;
const MAX_PACKAGE: usize = 1024 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateJob {
    pub id: String,
    pub expected_version: String,
    pub stage: String,
    pub downloaded: u64,
    pub total: Option<u64>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UpdateStatus {
    pub protocol: u32,
    pub current_version: String,
    pub available_version: Option<String>,
    pub can_install: bool,
    pub enabled: bool,
    pub reason: String,
    pub root_required: bool,
    pub root_enrolled: bool,
    pub job: Option<UpdateJob>,
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Policy {
    #[serde(default)]
    enabled: bool,
}
#[derive(Clone)]
struct Offer {
    version: String,
    target: String,
    url: String,
    signature: String,
}
struct State {
    policy: Policy,
    policy_available: bool,
    checked: Option<Instant>,
    checking: bool,
    offer: Option<Offer>,
    reason: String,
    job: Option<UpdateJob>,
    reconnected_job: Option<String>,
}
type DesktopExit = Arc<dyn Fn() + Send + Sync>;

pub struct FleetUpdates {
    policy_path: PathBuf,
    job_path: PathBuf,
    state: Mutex<State>,
    // Keep the autonomous/headless service independent of Wry's destructor.
    // Only a desktop attachment captures an AppHandle behind this callback.
    desktop_exit: Mutex<Option<DesktopExit>>,
    eligibility: Mutex<(Result<&'static str, String>, bool, bool)>,
    enrollment: Mutex<()>,
    policy_transition: Mutex<()>,
}

fn terminal(stage: &str) -> bool {
    matches!(stage, "completed" | "failed" | "cancelled")
}

fn read_progress_at(path: &Path, job_id: &str) -> Option<minerdesk_update_helper::WorkerEvent> {
    if !fs::symlink_metadata(path).ok()?.is_file() {
        return None;
    }
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options.open(path).ok()?;
    let metadata = file.metadata().ok()?;
    if !metadata.is_file() || metadata.len() > 8192 {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(8193).read_to_end(&mut bytes).ok()?;
    if bytes.len() > 8192 {
        return None;
    }
    let event: minerdesk_update_helper::WorkerEvent = serde_json::from_slice(&bytes).ok()?;
    (event.schema == 1
        && event.job_id == job_id
        && (event.stage == "failed" || event.error.is_none())
        && matches!(event.stage.as_str(),
            "verifying" | "ready" | "installing" | "restarting" | "completed" | "failed"))
    .then_some(event)
}

fn read_worker_progress(job_id: &str) -> Option<minerdesk_update_helper::WorkerEvent> {
    let path = minerdesk_update_helper::progress_path(job_id).ok()?;
    read_progress_at(&path, job_id)
}

fn reconcile_job(
    job: &mut UpdateJob,
    current_version: &str,
    worker: Option<&minerdesk_update_helper::WorkerEvent>,
    reconnected: bool,
) {
    let worker = worker.filter(|event| event.schema == 1 && event.job_id == job.id);
    if worker.is_some_and(|event| event.stage == "failed") {
        job.stage = "failed".into();
        job.error = Some("The signed update failed; inspect the local update log".into());
    } else if worker.is_some_and(|event| event.stage == "completed")
        && job.expected_version == current_version
    {
        job.stage = "completed".into();
        job.error = None;
    } else if reconnected && job.expected_version == current_version {
        // A new executable can exist even when the package manager later fails.
        // Wait for the worker's final transaction result, including the brief
        // interval between relaunch and its completed/failed status write.
        if !matches!(job.stage.as_str(), "failed" | "cancelled") {
            job.stage = "restarting".into();
            job.error = None;
        }
    } else if (reconnected && !terminal(&job.stage))
        || worker.is_some_and(|event| event.stage == "completed")
    {
        job.stage = "failed".into();
        job.error = Some("The previous update did not reconnect at the expected version; it was not retried".into());
    }
}
fn stable_version(value: &str) -> Result<semver::Version, String> {
    let parsed =
        semver::Version::parse(value).map_err(|_| "Invalid stable update version".to_string())?;
    if !parsed.pre.is_empty() || !parsed.build.is_empty() || parsed.to_string() != value {
        return Err("Only exact stable update versions are accepted".into());
    }
    Ok(parsed)
}

fn save<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or("Update state directory is unavailable")?;
    fs::create_dir_all(parent).map_err(|_| "Cannot create local update state")?;
    let mut temporary =
        tempfile::NamedTempFile::new_in(parent).map_err(|_| "Cannot save local update state")?;
    serde_json::to_writer(&mut temporary, value)
        .map_err(|_| "Cannot serialize local update state")?;
    temporary
        .as_file_mut()
        .sync_all()
        .map_err(|_| "Cannot sync local update state")?;
    temporary
        .persist(path)
        .map_err(|_| "Cannot persist local update state")?;
    #[cfg(unix)]
    fs::File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| "Cannot sync local update state directory")?;
    Ok(())
}

impl FleetUpdates {
    pub fn new(config_path: &Path) -> Arc<Self> {
        let policy_path = config_path.with_file_name("local-update-policy.json");
        let job_path = config_path.with_file_name("fleet-update-job.json");
        let policy = match fs::read(&policy_path) {
            Ok(bytes) if bytes.len() <= 4096 => serde_json::from_slice(&bytes).ok(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Some(Policy::default()),
            _ => None,
        };
        let mut job: Option<UpdateJob> = fs::read(&job_path)
            .ok()
            .filter(|b| b.len() <= 8192)
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .filter(|job: &UpdateJob| {
                minerdesk_update_helper::valid_job_id(&job.id)
                    && stable_version(&job.expected_version).is_ok()
                    && matches!(
                        job.stage.as_str(),
                        "queued"
                            | "checking"
                            | "downloading"
                            | "verifying"
                            | "ready"
                            | "stopping"
                            | "installing"
                            | "restarting"
                            | "completed"
                            | "failed"
                            | "cancelled"
                    )
            });
        if let Some(job) = &mut job {
            // Error details from a worker or a hand-edited state file are never
            // included in a Fleet snapshot (they may contain local paths).
            if job.error.is_some() {
                job.error = Some("The previous update failed; inspect the local update log".into());
            }
            let worker = read_worker_progress(&job.id);
            reconcile_job(job, env!("CARGO_PKG_VERSION"), worker.as_ref(), true);
            let _ = save(&job_path, job);
        }
        let reconnected_job = job.as_ref().map(|job| job.id.clone());
        Arc::new(Self {
            policy_path,
            job_path,
            state: Mutex::new(State {
                policy_available: policy.is_some(),
                policy: policy.unwrap_or_default(),
                checked: None,
                checking: false,
                offer: None,
                reason: "unavailable".into(),
                job,
                reconnected_job,
            }),
            desktop_exit: Mutex::new(None),
            eligibility: Mutex::new(platform_status()),
            enrollment: Mutex::new(()),
            policy_transition: Mutex::new(()),
        })
    }

    pub fn attach_app(&self, app: tauri::AppHandle) {
        self.set_desktop_exit(Arc::new(move || app.exit(0)));
    }

    fn set_desktop_exit(&self, exit: DesktopExit) {
        if let Ok(mut current) = self.desktop_exit.lock() {
            *current = Some(exit);
        }
    }

    fn request_desktop_exit(&self) -> bool {
        let exit = self.desktop_exit.lock().ok().and_then(|exit| exit.clone());
        if let Some(exit) = exit {
            // App exit may reenter application state; release the lock first.
            exit();
            true
        } else {
            false
        }
    }

    pub fn status(&self) -> UpdateStatus {
        // Status polls never invoke a package manager or start a helper. Fresh
        // platform checks run during a check/install or local policy action.
        let (eligible, root_required, root_enrolled) = self
            .eligibility
            .lock()
            .map(|cached| cached.clone())
            .unwrap_or((Err("unavailable".into()), false, false));
        let Ok(mut state) = self.state.lock() else {
            return UpdateStatus {
                protocol: 1,
                current_version: env!("CARGO_PKG_VERSION").into(),
                available_version: None,
                can_install: false,
                enabled: false,
                reason: "policy_unavailable".into(),
                root_required,
                root_enrolled,
                job: None,
            };
        };
        let reconnected_job = state.reconnected_job.clone();
        if let Some(job) = &mut state.job {
            let before = job.clone();
            let worker = read_worker_progress(&job.id);
            let reconnected = reconnected_job.as_deref() == Some(job.id.as_str());
            reconcile_job(job, env!("CARGO_PKG_VERSION"), worker.as_ref(), reconnected);
            if *job != before {
                let _ = save(&self.job_path, job);
            }
        }
        let busy = state.job.as_ref().is_some_and(|j| !terminal(&j.stage));
        let reason = if !state.policy_available {
            "policy_unavailable"
        } else if !state.policy.enabled {
            "disabled"
        } else if let Err(reason) = &eligible {
            reason
        } else if busy {
            "busy"
        } else if state.checking {
            "checking"
        } else {
            &state.reason
        };
        UpdateStatus {
            protocol: 1,
            current_version: env!("CARGO_PKG_VERSION").into(),
            available_version: state.offer.as_ref().map(|offer| offer.version.clone()),
            can_install: state.policy_available
                && state.policy.enabled
                && eligible.is_ok()
                && state.offer.is_some()
                && !busy
                && !state.checking,
            enabled: state.policy_available && state.policy.enabled,
            reason: reason.into(),
            root_required,
            root_enrolled,
            job: state.job.clone(),
        }
    }

    /// Invoked only by the guarded local policy endpoint, never AppConfig.
    pub fn set_enabled(&self, enabled: bool) -> Result<UpdateStatus, String> {
        let _transition = self
            .policy_transition
            .lock()
            .map_err(|_| "Local update policy is unavailable")?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| "Local update policy is unavailable")?;
        let policy = Policy { enabled };
        save(&self.policy_path, &policy)?;
        state.policy = policy;
        state.policy_available = true;
        drop(state);
        self.refresh_platform();
        Ok(self.status())
    }

    pub fn check(self: &Arc<Self>) -> Result<UpdateStatus, String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "Update state is unavailable")?;
        if !state.policy.enabled || !state.policy_available {
            return Err("Remote updates must be enabled on this computer first".into());
        }
        if state.checking
            || state
                .checked
                .is_some_and(|at| at.elapsed() < CHECK_INTERVAL)
            || state.job.as_ref().is_some_and(|job| !terminal(&job.stage))
        {
            drop(state);
            return Ok(self.status());
        }
        state.checking = true;
        state.checked = Some(Instant::now());
        drop(state);
        let service = Arc::clone(self);
        std::thread::spawn(move || {
            let result = tokio::runtime::Runtime::new()
                .map_err(|_| "Update runtime unavailable".into())
                .and_then(|runtime| runtime.block_on(fetch_offer()));
            if let Ok(mut state) = service.state.lock() {
                state.checking = false;
                match result {
                    Ok(offer) => {
                        state.reason = if offer.is_some() {
                            "supported"
                        } else {
                            "up_to_date"
                        }
                        .into();
                        state.offer = offer;
                    }
                    Err(_) => {
                        state.reason = "unavailable".into();
                        state.offer = None;
                    }
                }
            }
            service.refresh_platform();
        });
        Ok(self.status())
    }

    /// Freeze the exact checked release and return promptly. A package download,
    /// signature check, or helper failure cannot stop an active miner.
    pub fn install(
        self: &Arc<Self>,
        core: Arc<super::CoreState>,
        expected_version: &str,
    ) -> Result<UpdateStatus, String> {
        let requested = stable_version(expected_version)?;
        if requested <= stable_version(env!("CARGO_PKG_VERSION"))? {
            return Err("The update version must be newer than the installed version".into());
        }
        let target = eligibility()?;
        self.refresh_platform();
        let (offer, job_id) = self.queue_install(expected_version, target)?;
        let service = Arc::clone(self);
        std::thread::spawn(move || {
            let result = tokio::runtime::Runtime::new()
                .map_err(|_| "Update runtime unavailable".to_string())
                .and_then(|runtime| {
                    runtime.block_on(service.prepare_and_handoff(&core, &offer, &job_id))
                });
            if let Err(error) = result {
                service.progress("failed", 0, None, Some(error));
            }
        });
        Ok(self.status())
    }

    fn queue_install(
        &self,
        expected_version: &str,
        target: &str,
    ) -> Result<(Offer, String), String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "Update state is unavailable")?;
        if !state.policy.enabled || !state.policy_available {
            return Err("Remote updates must be enabled on this computer first".into());
        }
        if state.checking || state.job.as_ref().is_some_and(|job| !terminal(&job.stage)) {
            return Err("An update operation is already in progress".into());
        }
        let offer = state
            .offer
            .clone()
            .filter(|offer| offer.version == expected_version && offer.target == target)
            .ok_or("Check for the exact expected update version first")?;
        let job = UpdateJob {
            id: uuid::Uuid::new_v4().to_string(),
            expected_version: expected_version.into(),
            stage: "queued".into(),
            downloaded: 0,
            total: None,
            error: None,
        };
        save(&self.job_path, &job)?;
        let job_id = job.id.clone();
        state.job = Some(job);
        state.reconnected_job = None;
        drop(state);
        Ok((offer, job_id))
    }

    fn progress(&self, stage: &str, downloaded: u64, total: Option<u64>, error: Option<String>) {
        if let Ok(mut state) = self.state.lock() {
            if let Some(job) = &mut state.job {
                job.stage = stage.into();
                job.downloaded = downloaded;
                job.total = total;
                job.error = error;
                let _ = save(&self.job_path, job);
            }
        }
    }

    fn enabled(&self) -> bool {
        self.state
            .lock()
            .is_ok_and(|state| state.policy_available && state.policy.enabled)
    }

    async fn prepare_and_handoff(
        &self,
        core: &Arc<super::CoreState>,
        offer: &Offer,
        job_id: &str,
    ) -> Result<(), String> {
        let client = client()?;
        self.progress("downloading", 0, None, None);
        let mut response = client
            .get(&offer.url)
            .timeout(Duration::from_secs(15 * 60))
            .send()
            .await
            .map_err(|_| "The official update package could not be downloaded")?;
        response
            .error_for_status_ref()
            .map_err(|_| "The official update package is unavailable")?;
        let total = response.content_length();
        if total.is_some_and(|length| length > MAX_PACKAGE as u64) {
            return Err("The update package exceeds the size limit".into());
        }
        let directory = tempfile::Builder::new()
            .prefix("minerdesk-fleet-update-")
            .tempdir()
            .map_err(|_| "Cannot stage the update package")?;
        let mut bytes = Vec::new();
        let mut last_report = Instant::now();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| "The update download was interrupted")?
        {
            if !self.enabled() {
                self.progress("cancelled", bytes.len() as u64, total, None);
                return Ok(());
            }
            if bytes.len().saturating_add(chunk.len()) > MAX_PACKAGE {
                return Err("The update package exceeds the size limit".into());
            }
            bytes.extend_from_slice(&chunk);
            if last_report.elapsed() >= Duration::from_millis(500) {
                self.progress("downloading", bytes.len() as u64, total, None);
                last_report = Instant::now();
            }
        }
        self.handoff_download(core, offer, job_id, directory, bytes, total)
    }

    fn handoff_download(
        &self,
        core: &Arc<super::CoreState>,
        offer: &Offer,
        job_id: &str,
        directory: tempfile::TempDir,
        bytes: Vec<u8>,
        total: Option<u64>,
    ) -> Result<(), String> {
        let package = directory.path().join("package");
        self.progress("verifying", bytes.len() as u64, total, None);
        minerdesk_update_helper::verify_package(&bytes, &offer.signature, &offer.version)
            .map_err(|_| "The update signature is invalid; mining continues")?;
        fs::write(&package, &bytes).map_err(|_| "Cannot stage the verified update package")?;
        let downloaded = bytes.len() as u64;
        drop(bytes);
        // The privileged worker revalidates the official metadata, signature,
        // install identity and caller independently before reporting ready.
        let mut worker = worker_command(&offer.target)?;
        let mut child = worker
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| "The installed update helper could not start")?;
        let mut input = child
            .stdin
            .take()
            .ok_or("Update helper input is unavailable")?;
        let output = child
            .stdout
            .take()
            .ok_or("Update helper output is unavailable")?;
        let request = minerdesk_update_helper::Request {
            schema: 1,
            job_id: job_id.into(),
            expected_version: offer.version.clone(),
            package_path: package,
        };
        serde_json::to_writer(&mut input, &request).map_err(|_| "Cannot prepare update helper")?;
        input
            .write_all(b"\n")
            .and_then(|_| input.flush())
            .map_err(|_| "Cannot contact update helper")?;
        let (send, receive) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut line = String::new();
            let result = BufReader::new(output)
                .take(8192)
                .read_line(&mut line)
                .map(|_| line);
            let _ = send.send(result);
        });
        let ready = receive
            .recv_timeout(Duration::from_secs(120))
            .ok()
            .and_then(Result::ok)
            .and_then(|line| {
                serde_json::from_str::<minerdesk_update_helper::WorkerEvent>(&line).ok()
            });
        if !ready.is_some_and(|event| {
            event.schema == 1 && event.job_id == job_id && event.stage == "ready"
        }) {
            let _ = child.kill();
            return Err(
                "The update helper could not safely prepare this installation; mining continues"
                    .into(),
            );
        }
        // Serialize the final handoff with local opt-out. A completed Disable
        // prevents any later stop/ack; an already committed handoff finishes
        // before that local action returns.
        let _transition = self
            .policy_transition
            .lock()
            .map_err(|_| "Local update policy is unavailable")?;
        if !self.enabled() {
            let _ = child.kill();
            self.progress("cancelled", downloaded, total, None);
            return Ok(());
        }
        self.progress("ready", downloaded, Some(downloaded), None);
        self.progress("stopping", downloaded, Some(downloaded), None);
        core.cancel_power_action();
        core.stop_all();
        let ack = minerdesk_update_helper::Ack {
            schema: 1,
            job_id: job_id.into(),
            apply: true,
        };
        let acknowledged = serde_json::to_writer(&mut input, &ack)
            .map_err(|_| "Cannot authorize the prepared update helper")
            .and_then(|_| {
                input
                    .write_all(b"\n")
                    .and_then(|_| input.flush())
                    .map_err(|_| "The prepared update helper disconnected")
            });
        if let Err(error) = acknowledged {
            core.shutting_down
                .store(false, std::sync::atomic::Ordering::SeqCst);
            return Err(format!("{error}; restart mining locally when ready"));
        }
        self.progress("installing", downloaded, Some(downloaded), None);
        // Ready means the worker owns an independently verified private copy.
        // Remove our download before a headless process::exit skips destructors.
        drop(input);
        drop(directory);
        if !self.request_desktop_exit() {
            std::process::exit(0);
        }
        Ok(())
    }

    pub fn enrollment(&self, revoke: bool) -> Result<UpdateStatus, String> {
        let _operation = self
            .enrollment
            .try_lock()
            .map_err(|_| "Local update authorization is already in progress")?;
        // Opt out before the optional OS prompt: cancelling grant removal still
        // leaves this agent unable to accept another remote installation.
        if revoke {
            self.set_enabled(false)?;
        }
        #[cfg(target_os = "linux")]
        {
            let helper = minerdesk_update_helper::helper_path()
                .map_err(|_| "Install the current MinerDesk Debian package locally first")?;
            if helper != Path::new("/usr/lib/minerdesk/minerdesk-update-helper") {
                return Err(
                    "Root enrollment is available only for an installed Linux Debian package"
                        .into(),
                );
            }
            let mut command = Command::new("/usr/bin/pkexec");
            command
                .arg("--disable-internal-agent")
                .arg(&helper)
                .arg(if revoke { "--revoke" } else { "--enroll" });
            let mut child = command
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|_| "Linux administrator approval is unavailable")?;
            let deadline = Instant::now() + Duration::from_secs(120);
            loop {
                match child
                    .try_wait()
                    .map_err(|_| "Cannot check Linux approval")?
                {
                    Some(status) if status.success() => break,
                    Some(_) => {
                        return Err("Linux administrator approval was cancelled or denied".into())
                    }
                    None if Instant::now() >= deadline => {
                        let _ = child.kill();
                        return Err("Linux administrator approval timed out".into());
                    }
                    None => std::thread::sleep(Duration::from_millis(50)),
                }
            }
            self.refresh_platform();
            return Ok(self.status());
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = revoke;
            Err("Root enrollment is available only for an installed Linux Debian package".into())
        }
    }

    fn refresh_platform(&self) {
        let platform = platform_status();
        if let Ok(mut cached) = self.eligibility.lock() {
            *cached = platform;
        }
    }
}

fn eligibility() -> Result<&'static str, String> {
    if cfg!(debug_assertions) {
        return Err("development".into());
    }
    minerdesk_update_helper::target_and_reason().map_err(|error| {
        // Keep OS, filesystem, and subprocess error details on this computer.
        let lower = error.to_ascii_lowercase();
        if lower.contains("root_enrollment_required") {
            "root_enrollment_required"
        } else if lower.contains("helper") {
            "helper_missing"
        } else if lower.contains("administrator")
            || lower.contains("elevated")
            || lower.contains("privileged backend")
        {
            "administrator_required"
        } else if lower.contains("writ")
            || lower.contains("readonly")
            || lower.contains("ownership")
            || lower.contains("permissions")
        {
            "readonly"
        } else if lower.contains("x86_64") || lower.contains("support") {
            "platform"
        } else if lower.contains("portable") {
            "portable"
        } else {
            "installation"
        }
        .into()
    })
}
fn platform_status() -> (Result<&'static str, String>, bool, bool) {
    let eligible = eligibility();
    #[cfg(target_os = "linux")]
    let root_required = std::env::var_os("APPIMAGE").is_none()
        && std::env::current_exe().is_ok_and(|exe| {
            matches!(
                exe.to_str(),
                Some("/usr/bin/minerdesk" | "/usr/bin/minerdesk-headless")
            )
        });
    #[cfg(not(target_os = "linux"))]
    let root_required = false;
    let root_enrolled = root_required && minerdesk_update_helper::enrolled();
    (eligible, root_required, root_enrolled)
}
fn worker_command(target: &str) -> Result<Command, String> {
    #[cfg(target_os = "linux")]
    if target == "linux-x86_64-deb" {
        let mut command = Command::new("/usr/bin/sudo");
        command.args([
            "-n",
            "/usr/lib/minerdesk/minerdesk-update-helper",
            "--apply",
        ]);
        return Ok(command);
    }
    let _ = target;
    let helper = minerdesk_update_helper::helper_path()?;
    let mut command = Command::new(helper);
    command.arg("--apply");
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x01000000 | 0x08000000);
    }
    Ok(command)
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .https_only(true)
        .timeout(Duration::from_secs(20))
        .user_agent(concat!("MinerDesk/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|_| "Update network client is unavailable".into())
}
async fn metadata<T: serde::de::DeserializeOwned>(
    client: &reqwest::Client,
    url: &str,
) -> Result<T, String> {
    let mut response = client
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|_| "Official update metadata is unavailable")?;
    response
        .error_for_status_ref()
        .map_err(|_| "Official update metadata is unavailable")?;
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "Update metadata was interrupted")?
    {
        if bytes.len().saturating_add(chunk.len()) > MAX_METADATA {
            return Err("Update metadata exceeds the size limit".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|_| "Invalid official update metadata".into())
}
#[derive(Deserialize)]
struct Release {
    tag_name: String,
    html_url: String,
    draft: bool,
    prerelease: bool,
}
#[derive(Deserialize)]
struct Manifest {
    version: String,
    platforms: std::collections::BTreeMap<String, Artifact>,
}
#[derive(Deserialize)]
struct Artifact {
    url: String,
    signature: String,
}
async fn fetch_offer() -> Result<Option<Offer>, String> {
    let target = eligibility()?;
    let client = client()?;
    let release: Release = metadata(&client, RELEASE_API).await?;
    if release.draft || release.prerelease {
        return Err("Only official stable releases are accepted".into());
    }
    let version = release
        .tag_name
        .strip_prefix('v')
        .ok_or("Invalid official release tag")?;
    let parsed = stable_version(version)?;
    if release.html_url
        != format!("https://github.com/jontechlabs/MinerDesk/releases/tag/v{version}")
    {
        return Err("Invalid official release identity".into());
    }
    if parsed <= stable_version(env!("CARGO_PKG_VERSION"))? {
        return Ok(None);
    }
    let endpoint = format!(
        "https://github.com/jontechlabs/MinerDesk/releases/download/v{version}/latest.json"
    );
    let manifest: Manifest = metadata(&client, &endpoint).await?;
    if manifest.version != version {
        return Err("The official manifest version changed".into());
    }
    let artifact = manifest
        .platforms
        .get(target)
        .ok_or("No official package for this installation")?;
    if !minerdesk_update_helper::valid_artifact_url(&artifact.url, version, target)
        || artifact.signature.is_empty()
        || artifact.signature.len() > 16384
    {
        return Err("Invalid official update artifact".into());
    }
    Ok(Some(Offer {
        version: version.into(),
        target: target.into(),
        url: artifact.url.clone(),
        signature: artifact.signature.clone(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn desktop_exit_callback_runs_without_holding_the_service_lock() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let directory = tempfile::tempdir().unwrap();
        let service = FleetUpdates::new(&directory.path().join("config.json"));
        assert!(!service.request_desktop_exit());
        let weak = Arc::downgrade(&service);
        let calls = Arc::new(AtomicUsize::new(0));
        let callback_calls = Arc::clone(&calls);
        service.set_desktop_exit(Arc::new(move || {
            let service = weak.upgrade().unwrap();
            assert!(service.desktop_exit.try_lock().is_ok());
            callback_calls.fetch_add(1, Ordering::SeqCst);
        }));
        assert!(service.request_desktop_exit());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn local_policy_is_off_and_independent_of_remote_config() {
        let directory = tempfile::tempdir().unwrap();
        let config = directory.path().join("config.json");
        fs::write(
            &config,
            br#"{"updates":{"enabled":true},"fleet_updates":true}"#,
        )
        .unwrap();
        let service = FleetUpdates::new(&config);
        assert!(!service.status().enabled);
        service.set_enabled(true).unwrap();
        assert!(FleetUpdates::new(&config).status().enabled);
        fs::write(&config, b"{}").unwrap();
        assert!(FleetUpdates::new(&config).status().enabled);
        service.set_enabled(false).unwrap();
        assert!(!FleetUpdates::new(&config).status().enabled);
    }
    #[test]
    fn corrupt_policy_fails_closed_without_blocking_local_mining() {
        let directory = tempfile::tempdir().unwrap();
        let config = directory.path().join("config.json");
        fs::write(directory.path().join("local-update-policy.json"), b"broken").unwrap();
        let status = FleetUpdates::new(&config).status();
        assert!(!status.enabled);
        assert!(!status.can_install);
        assert_eq!(status.reason, "policy_unavailable");
    }
    #[test]
    fn restart_reconciles_version_and_never_replays_jobs() {
        let directory = tempfile::tempdir().unwrap();
        let config = directory.path().join("config.json");
        let mut job = UpdateJob {
            id: uuid::Uuid::new_v4().to_string(),
            expected_version: env!("CARGO_PKG_VERSION").into(),
            stage: "installing".into(),
            downloaded: 10,
            total: Some(10),
            error: None,
        };
        save(&config.with_file_name("fleet-update-job.json"), &job).unwrap();
        assert_eq!(
            FleetUpdates::new(&config).status().job.unwrap().stage,
            "restarting"
        );
        job.expected_version = "999.0.0".into();
        save(&config.with_file_name("fleet-update-job.json"), &job).unwrap();
        assert_eq!(
            FleetUpdates::new(&config).status().job.unwrap().stage,
            "failed"
        );
    }
    fn reconciliation_fixture() -> UpdateJob {
        UpdateJob {
            id: uuid::Uuid::new_v4().to_string(),
            expected_version: "0.8.3".into(),
            stage: "installing".into(),
            downloaded: 10,
            total: Some(10),
            error: None,
        }
    }
    fn worker_fixture(job: &UpdateJob, stage: &str) -> minerdesk_update_helper::WorkerEvent {
        minerdesk_update_helper::WorkerEvent {
            schema: 1,
            job_id: job.id.clone(),
            stage: stage.into(),
            error: (stage == "failed")
                .then(|| "private local path /secret/package and installer output".into()),
        }
    }
    #[test]
    fn a_partial_installer_failure_wins_even_after_the_new_binary_reconnects() {
        let mut job = reconciliation_fixture();
        let worker = worker_fixture(&job, "failed");
        reconcile_job(&mut job, "0.8.3", Some(&worker), true);
        assert_eq!(job.stage, "failed");
        assert!(!job.error.as_deref().unwrap().contains("/secret"));
        let mut old_binary = job.clone();
        reconcile_job(&mut old_binary, "0.8.2", Some(&worker), true);
        assert_eq!(old_binary.stage, "failed");
    }
    #[test]
    fn reconnect_waits_for_the_matching_workers_final_result() {
        let mut job = reconciliation_fixture();
        let worker = worker_fixture(&job, "restarting");
        reconcile_job(&mut job, "0.8.3", Some(&worker), true);
        assert_eq!(job.stage, "restarting");
        // Missing/invalid final progress cannot manufacture a successful package
        // transaction, including a previously persisted version-only success.
        job.stage = "completed".into();
        reconcile_job(&mut job, "0.8.3", None, true);
        assert_eq!(job.stage, "restarting");
        let mut other = worker_fixture(&job, "completed");
        other.job_id = uuid::Uuid::new_v4().to_string();
        reconcile_job(&mut job, "0.8.3", Some(&other), true);
        assert_eq!(job.stage, "restarting");
        let complete = worker_fixture(&job, "completed");
        reconcile_job(&mut job, "0.8.3", Some(&complete), true);
        assert_eq!(job.stage, "completed");
        assert!(job.error.is_none());
        let mut old_binary = job.clone();
        reconcile_job(&mut old_binary, "0.8.2", Some(&complete), true);
        assert_eq!(old_binary.stage, "failed");
    }
    #[test]
    fn worker_progress_reader_is_bounded_and_rejects_other_jobs_or_schemas() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("progress.json");
        let job = reconciliation_fixture();
        let mut event = worker_fixture(&job, "completed");
        fs::write(&path, serde_json::to_vec(&event).unwrap()).unwrap();
        assert_eq!(read_progress_at(&path, &job.id).unwrap().stage, "completed");
        assert!(read_progress_at(&path, &uuid::Uuid::new_v4().to_string()).is_none());
        event.schema = 2;
        fs::write(&path, serde_json::to_vec(&event).unwrap()).unwrap();
        assert!(read_progress_at(&path, &job.id).is_none());
        fs::write(&path, b"not JSON").unwrap();
        assert!(read_progress_at(&path, &job.id).is_none());
        fs::write(&path, vec![b' '; 8193]).unwrap();
        assert!(read_progress_at(&path, &job.id).is_none());
        #[cfg(target_os = "linux")]
        {
            let link = directory.path().join("linked-progress.json");
            std::os::unix::fs::symlink(&path, &link).unwrap();
            assert!(read_progress_at(&link, &job.id).is_none());
        }
    }
    #[test]
    fn stable_expected_version_is_exact_and_safe_status_has_no_local_paths() {
        for invalid in ["v1.2.3", "1.2.3-beta.1", "1.2.3+build", "../1.2.3", "1.2"] {
            assert!(stable_version(invalid).is_err());
        }
        let directory = tempfile::tempdir().unwrap();
        let service = FleetUpdates::new(&directory.path().join("secret-config.json"));
        let value = serde_json::to_string(&service.status()).unwrap();
        assert!(!value.contains("secret-config"));
        assert!(!value.contains("policy_path"));
    }
    #[test]
    fn accepted_job_is_prompt_durable_and_freezes_the_reviewed_release() {
        let directory = tempfile::tempdir().unwrap();
        let config = directory.path().join("config.json");
        let service = FleetUpdates::new(&config);
        service.set_enabled(true).unwrap();
        let fake_offer = |version: &str| Offer {
            version: version.into(),
            target: "fixture".into(),
            url: "https://example.invalid/disposable-package".into(),
            signature: "fixture".into(),
        };
        service.state.lock().unwrap().offer = Some(fake_offer("999.0.0"));
        assert!(service.queue_install("999.0.1", "fixture").is_err());
        assert!(service.status().job.is_none());
        let started = Instant::now();
        let (frozen, id) = service.queue_install("999.0.0", "fixture").unwrap();
        assert!(started.elapsed() < Duration::from_secs(1));
        assert_eq!(service.status().job.unwrap().stage, "queued");
        let persisted: UpdateJob =
            serde_json::from_slice(&fs::read(&service.job_path).unwrap()).unwrap();
        assert_eq!(persisted.id, id);
        assert_eq!(persisted.expected_version, "999.0.0");
        service.state.lock().unwrap().offer = Some(fake_offer("999.0.1"));
        assert_eq!(frozen.version, "999.0.0");
        assert!(service.queue_install("999.0.1", "fixture").is_err());
        // No network, miner, installer, supervisor, or real configuration is used.
    }
    #[test]
    fn a_shared_check_cache_prevents_repeated_remote_checks() {
        let directory = tempfile::tempdir().unwrap();
        let service = FleetUpdates::new(&directory.path().join("config.json"));
        service.set_enabled(true).unwrap();
        let checked = Instant::now();
        service.state.lock().unwrap().checked = Some(checked);
        for _ in 0..100 {
            service.check().unwrap();
        }
        let state = service.state.lock().unwrap();
        assert_eq!(state.checked, Some(checked));
        assert!(!state.checking);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_tampered_disposable_package_never_stops_the_supervised_process() {
        struct Session(Arc<super::super::CoreState>);
        impl Drop for Session {
            fn drop(&mut self) {
                self.0.stop_all();
            }
        }
        let directory = tempfile::tempdir().unwrap();
        let core =
            super::super::CoreState::load_from_path(directory.path().join("fixture-config.json"))
                .unwrap();
        let _session = Session(Arc::clone(&core));
        let child = Command::new("/bin/sleep")
            .arg("30")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        core.children.lock().unwrap().insert(
            "disposable-fixture".into(),
            super::super::ManagedChild {
                child,
                elevated: false,
                control: None,
            },
        );
        let service = Arc::clone(&core.fleet_updates);
        service.set_enabled(true).unwrap();
        let offer = Offer {
            version: "999.0.0".into(),
            target: "fixture".into(),
            url: "https://example.invalid/never-requested".into(),
            signature: "invalid-publisher-signature".into(),
        };
        let result = service.handoff_download(
            &core,
            &offer,
            &uuid::Uuid::new_v4().to_string(),
            tempfile::tempdir_in(directory.path()).unwrap(),
            b"disposable fake update package".to_vec(),
            Some(30),
        );
        assert!(result.unwrap_err().contains("signature is invalid"));
        assert!(!core.shutting_down.load(std::sync::atomic::Ordering::SeqCst));
        assert!(core
            .children
            .lock()
            .unwrap()
            .get_mut("disposable-fixture")
            .unwrap()
            .try_wait()
            .unwrap()
            .is_none());
    }
}
