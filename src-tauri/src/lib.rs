use axum::{
    body::Body,
    extract::{ConnectInfo, Path as AxumPath, Query, State as AxumState},
    http::{header, HeaderMap, Request, StatusCode, Uri},
    response::{IntoResponse, Response},
    middleware::{self, Next},
    routing::{get, post},
    Json, Router,
};
use chrono::{Datelike, Local, NaiveTime, Timelike};
use clap::Parser;
use flate2::read::GzDecoder;
use include_dir::{include_dir, Dir};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    fs,
    io::{BufRead, BufReader, Cursor, Read, Write},
    net::{IpAddr, SocketAddr, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, RwLock,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tauri::Manager;
#[cfg(target_os = "windows")]
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};
use tower_http::cors::CorsLayer;
mod schedule_control;
#[cfg(any(target_os = "windows", test))]
mod config_sync;
use schedule_control::{scheduler_should_stop_session, ActiveOccurrences, LocalMoment, ManualScheduleControl, StopSnapshot};

use uuid::Uuid;
use walkdir::WalkDir;
use zip::ZipArchive;

static FRONTEND: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../dist");
const DEFAULT_WEB_PORT: u16 = 17888;
const MAX_LOG_LINES: usize = 1500;
static CLOSE_TO_TRAY: AtomicBool = AtomicBool::new(false);
#[cfg(target_os = "windows")]
static DESKTOP_EXIT_SHUTDOWN_STARTED: AtomicBool = AtomicBool::new(false);

// Windows crash-safety guard for miner processes.
//
// Each miner gets its own private Job Object configured with
// JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE. Per-miner jobs are stronger than one
// process-wide job: stopping/restarting one miner can terminate that miner's
// entire process tree, while a backend crash still closes every remaining job
// handle and lets Windows terminate every protected miner automatically.
#[cfg(target_os = "windows")]
mod windows_miner_job {
    use std::{
        ffi::c_void,
        mem::{size_of, zeroed},
        os::windows::io::AsRawHandle,
        process::Child,
        ptr,
        sync::atomic::{AtomicBool, Ordering},
    };

    type Handle = *mut c_void;
    type Bool = i32;

    const JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: u32 = 0x0000_2000;
    const JOB_OBJECT_EXTENDED_LIMIT_INFORMATION_CLASS: i32 = 9;
    const TH32CS_SNAPTHREAD: u32 = 0x0000_0004;
    const THREAD_SUSPEND_RESUME: u32 = 0x0002;
    const RESUME_THREAD_FAILED: u32 = u32::MAX;

    static CRASH_GUARD_AVAILABLE: AtomicBool = AtomicBool::new(false);

    #[repr(C)]
    #[derive(Copy, Clone)]
    #[allow(non_snake_case)]
    struct IoCounters {
        ReadOperationCount: u64,
        WriteOperationCount: u64,
        OtherOperationCount: u64,
        ReadTransferCount: u64,
        WriteTransferCount: u64,
        OtherTransferCount: u64,
    }

    #[repr(C)]
    #[derive(Copy, Clone)]
    #[allow(non_snake_case)]
    struct JobObjectBasicLimitInformation {
        PerProcessUserTimeLimit: i64,
        PerJobUserTimeLimit: i64,
        LimitFlags: u32,
        MinimumWorkingSetSize: usize,
        MaximumWorkingSetSize: usize,
        ActiveProcessLimit: u32,
        Affinity: usize,
        PriorityClass: u32,
        SchedulingClass: u32,
    }

    #[repr(C)]
    #[derive(Copy, Clone)]
    #[allow(non_snake_case)]
    struct JobObjectExtendedLimitInformation {
        BasicLimitInformation: JobObjectBasicLimitInformation,
        IoInfo: IoCounters,
        ProcessMemoryLimit: usize,
        JobMemoryLimit: usize,
        PeakProcessMemoryUsed: usize,
        PeakJobMemoryUsed: usize,
    }

    #[repr(C)]
    #[allow(non_snake_case)]
    struct ThreadEntry32 {
        dwSize: u32,
        cntUsage: u32,
        th32ThreadID: u32,
        th32OwnerProcessID: u32,
        tpBasePri: i32,
        tpDeltaPri: i32,
        dwFlags: u32,
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn CreateJobObjectW(lp_job_attributes: *const c_void, lp_name: *const u16) -> Handle;
        fn SetInformationJobObject(
            h_job: Handle,
            job_object_information_class: i32,
            lp_job_object_information: *const c_void,
            cb_job_object_information_length: u32,
        ) -> Bool;
        fn AssignProcessToJobObject(h_job: Handle, h_process: Handle) -> Bool;
        fn TerminateJobObject(h_job: Handle, exit_code: u32) -> Bool;
        fn CreateToolhelp32Snapshot(flags: u32, process_id: u32) -> Handle;
        fn Thread32First(snapshot: Handle, entry: *mut ThreadEntry32) -> Bool;
        fn Thread32Next(snapshot: Handle, entry: *mut ThreadEntry32) -> Bool;
        fn OpenThread(desired_access: u32, inherit_handle: Bool, thread_id: u32) -> Handle;
        fn ResumeThread(thread: Handle) -> u32;
        fn CloseHandle(h_object: Handle) -> Bool;
    }

    pub struct WindowsMinerJob {
        // Store the raw handle value as usize so this owner is trivially Send.
        // The handle is never inherited by miners and is closed by Drop.
        handle: usize,
    }

    impl WindowsMinerJob {
        pub fn new() -> Result<Self, String> {
            let handle = unsafe { CreateJobObjectW(ptr::null(), ptr::null()) };
            if handle.is_null() {
                CRASH_GUARD_AVAILABLE.store(false, Ordering::SeqCst);
                return Err(format!("CreateJobObjectW failed: {}", std::io::Error::last_os_error()));
            }

            let mut limits: JobObjectExtendedLimitInformation = unsafe { zeroed() };
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let ok = unsafe {
                SetInformationJobObject(
                    handle,
                    JOB_OBJECT_EXTENDED_LIMIT_INFORMATION_CLASS,
                    &limits as *const _ as *const c_void,
                    size_of::<JobObjectExtendedLimitInformation>() as u32,
                )
            };
            if ok == 0 {
                let error = std::io::Error::last_os_error();
                unsafe { CloseHandle(handle) };
                CRASH_GUARD_AVAILABLE.store(false, Ordering::SeqCst);
                return Err(format!("SetInformationJobObject(KILL_ON_JOB_CLOSE) failed: {error}"));
            }

            CRASH_GUARD_AVAILABLE.store(true, Ordering::SeqCst);
            Ok(Self { handle: handle as usize })
        }

        fn raw(&self) -> Handle {
            self.handle as Handle
        }

        pub fn assign(&self, child: &Child) -> Result<(), String> {
            let process = child.as_raw_handle() as Handle;
            let ok = unsafe { AssignProcessToJobObject(self.raw(), process) };
            if ok == 0 {
                CRASH_GUARD_AVAILABLE.store(false, Ordering::SeqCst);
                return Err(format!("AssignProcessToJobObject failed: {}", std::io::Error::last_os_error()));
            }
            Ok(())
        }

        pub fn terminate(&self) -> Result<(), String> {
            let ok = unsafe { TerminateJobObject(self.raw(), 1) };
            if ok == 0 {
                return Err(format!("TerminateJobObject failed: {}", std::io::Error::last_os_error()));
            }
            Ok(())
        }
    }

    impl Drop for WindowsMinerJob {
        fn drop(&mut self) {
            // KILL_ON_JOB_CLOSE means this is also the final fail-safe for any
            // descendant left alive after a normal miner stop or unexpected root exit.
            unsafe { CloseHandle(self.raw()) };
        }
    }

    pub fn probe() -> Result<(), String> {
        // Create/close an empty job once at backend startup so /api/health can
        // report whether the Windows crash guard is available before mining starts.
        let job = WindowsMinerJob::new()?;
        drop(job);
        Ok(())
    }

    pub fn is_ready() -> bool {
        CRASH_GUARD_AVAILABLE.load(Ordering::SeqCst)
    }

    // Miner processes are created suspended, attached to their Job Object, then
    // resumed. This removes the race where a miner could spawn a child process
    // before MinerDesk had attached the root process to the job.
    pub fn resume_process(process_id: u32) -> Result<(), String> {
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
        let invalid_handle = usize::MAX as Handle;
        if snapshot.is_null() || snapshot == invalid_handle {
            CRASH_GUARD_AVAILABLE.store(false, Ordering::SeqCst);
            return Err(format!(
                "CreateToolhelp32Snapshot(threads) failed: {}",
                std::io::Error::last_os_error()
            ));
        }

        let mut entry: ThreadEntry32 = unsafe { zeroed() };
        entry.dwSize = size_of::<ThreadEntry32>() as u32;
        let mut more = unsafe { Thread32First(snapshot, &mut entry) };
        let mut resumed = 0usize;
        let mut last_error: Option<std::io::Error> = None;

        while more != 0 {
            if entry.th32OwnerProcessID == process_id {
                let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID) };
                if thread.is_null() {
                    last_error = Some(std::io::Error::last_os_error());
                } else {
                    let previous = unsafe { ResumeThread(thread) };
                    if previous == RESUME_THREAD_FAILED {
                        last_error = Some(std::io::Error::last_os_error());
                    } else {
                        resumed += 1;
                    }
                    unsafe { CloseHandle(thread) };
                }
            }
            more = unsafe { Thread32Next(snapshot, &mut entry) };
        }
        unsafe { CloseHandle(snapshot) };

        if resumed == 0 {
            CRASH_GUARD_AVAILABLE.store(false, Ordering::SeqCst);
            return Err(match last_error {
                Some(e) => format!("ResumeThread failed for miner PID {process_id}: {e}"),
                None => format!("No thread found to resume for miner PID {process_id}"),
            });
        }
        Ok(())
    }
}

struct ManagedChild {
    child: Child,
    #[cfg(target_os = "windows")]
    job: windows_miner_job::WindowsMinerJob,
}

impl ManagedChild {
    fn try_wait(&mut self) -> std::io::Result<Option<std::process::ExitStatus>> {
        self.child.try_wait()
    }

    fn terminate(&mut self) {
        #[cfg(target_os = "windows")]
        {
            // Terminate the whole miner job, not just the root process. This kills
            // miner helper/worker children during Stop/Restart as well.
            if self.job.terminate().is_err() {
                let _ = self.child.kill();
            }
            let _ = self.child.wait();
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

// ---------- Persisted configuration ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub miners: Vec<MinerProfile>,
    pub schedules: Vec<ScheduleWindow>,
    pub web: WebConfig,
    pub ui: UiConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            miners: vec![MinerProfile::default_prl()],
            schedules: vec![],
            web: WebConfig::default(),
            ui: UiConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WebConfig {
    /// Port used by the embedded HTTP API and web UI in desktop mode.
    pub desktop_api_port: u16,
    /// When true the desktop web UI also listens on all LAN interfaces.
    pub expose_lan: bool,
    /// Access token required for non-loopback clients when LAN access is enabled.
    pub token: String,
}

impl Default for WebConfig {
    fn default() -> Self {
        Self { desktop_api_port: DEFAULT_WEB_PORT, expose_lan: false, token: String::new() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct UiConfig {
    /// BCP-47-ish short language code used by the React UI. English is the default.
    pub language: String,
    /// Legacy 0.6.x/0.7.x setting kept for configuration compatibility.
    /// Since 0.7.5 the Windows Desktop always stops the privileged backend when
    /// the Desktop application really exits. Closing to tray is not an exit.
    pub stop_backend_on_desktop_exit: bool,
    /// Launch the non-elevated MinerDesk desktop application when the user signs in.
    pub start_with_windows: bool,
    /// Hide the Desktop window to the notification area instead of exiting on close.
    pub close_to_tray: bool,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            language: "en".into(),
            stop_backend_on_desktop_exit: true,
            start_with_windows: false,
            close_to_tray: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct GpuTuning {
    /// Engine selector (index for SRBMiner/lolMiner/Rigel, PCI UID for BzMiner).
    pub selector: String,
    pub core_clock: Option<u32>,
    pub power_limit: Option<u32>,
    pub fan: Option<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MinerProfile {
    pub id: String,
    pub name: String,
    pub engine: String,
    pub executable_path: String,
    pub enabled: bool,
    pub algorithm: String,
    pub pool: String,
    pub wallet: String,
    pub secondary_wallet: String,
    pub merge_secondary: bool,
    pub merge_separator: String,
    pub password: String,
    pub worker: String,
    pub gpu_ids: String,
    /// Per-GPU tuning. Legacy global values below remain as fallbacks for old profiles.
    pub gpu_tuning: Vec<GpuTuning>,
    pub core_clock: Option<u32>,
    pub power_limit: Option<u32>,
    pub fan: Option<u8>,
    pub api_port: Option<u16>,
    pub disable_cpu: bool,
    pub extra_args: String,
    pub allow_gpu_overlap: bool,
    /// Optional, explicit support for MinerDesk development. 0 = disabled.
    pub dev_tip_percent: f64,
}

impl Default for MinerProfile {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            name: "New miner".into(),
            engine: "srbminer".into(),
            executable_path: String::new(),
            enabled: true,
            algorithm: String::new(),
            pool: String::new(),
            wallet: String::new(),
            secondary_wallet: String::new(),
            merge_secondary: false,
            merge_separator: "+".into(),
            password: "x".into(),
            worker: "MinerDesk".into(),
            gpu_ids: String::new(),
            gpu_tuning: vec![],
            core_clock: None,
            power_limit: None,
            fan: None,
            api_port: None,
            disable_cpu: true,
            extra_args: String::new(),
            allow_gpu_overlap: false,
            dev_tip_percent: 0.0,
        }
    }
}

impl MinerProfile {
    fn default_prl() -> Self {
        Self {
            id: "prl-eco".into(),
            name: "PRL Eco".into(),
            engine: "srbminer".into(),
            algorithm: "pearlhash".into(),
            pool: "pearl-eu2.luckypool.io:3360".into(),
            // No GPU frequency/power/fan limit is applied by default.
            // Users can opt in to tuning per profile/per GPU.
            core_clock: None,
            api_port: Some(21550),
            merge_secondary: true,
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ScheduleWindow {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    /// ISO weekday numbers: Monday=1 ... Sunday=7.
    pub days: Vec<u32>,
    pub start: String,
    pub end: String,
    pub miner_ids: Vec<String>,
    /// Action requested after this schedule ends: none | sleep | hibernate.
    pub post_action: String,
    /// Countdown shown before the post action is executed.
    pub countdown_seconds: u32,
    /// When enabled, a Desktop/Web UI must remain open during the countdown.
    /// Closing/dismissing the prompt cancels the power action.
    pub require_confirmation: bool,
    /// Windows-only: create weekly wake timers before this mining window starts.
    pub wake_enabled: bool,
    /// Number of minutes before `start` at which the PC should wake.
    pub wake_minutes_before: u32,
}

impl Default for ScheduleWindow {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            name: "Mining window".into(),
            enabled: true,
            days: vec![1, 2, 3, 4, 5, 6, 7],
            start: "22:00".into(),
            end: "07:00".into(),
            miner_ids: vec![],
            post_action: "none".into(),
            countdown_seconds: 60,
            require_confirmation: true,
            wake_enabled: false,
            wake_minutes_before: 5,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingPowerAction {
    pub id: String,
    pub schedule_id: String,
    pub schedule_name: String,
    pub action: String,
    pub deadline_unix: u64,
    pub countdown_seconds: u32,
    pub require_confirmation: bool,
}

// ---------- Runtime ----------

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MinerMetrics {
    pub hashrate_hps: Option<f64>,
    pub power_w: Option<f64>,
    pub temperature_c: Option<f64>,
    pub fan_pct: Option<f64>,
    pub accepted: u64,
    pub rejected: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MinerRuntimeView {
    pub id: String,
    pub running: bool,
    pub pid: Option<u32>,
    pub started_at: Option<u64>,
    pub uptime_seconds: u64,
    pub started_by: String,
    pub last_error: String,
    pub metrics: MinerMetrics,
    /// True while the current miner process is running a voluntary MinerDesk dev-tip slice.
    pub dev_tip_active: bool,
}

impl Default for MinerRuntimeView {
    fn default() -> Self {
        Self {
            id: String::new(),
            running: false,
            pid: None,
            started_at: None,
            uptime_seconds: 0,
            started_by: String::new(),
            last_error: String::new(),
            metrics: MinerMetrics::default(),
            dev_tip_active: false,
        }
    }
}

fn mark_runtime_stopped(rt: &mut MinerRuntimeView) {
    rt.running = false;
    rt.pid = None;
    rt.started_at = None;
    // Uptime describes the current mining session only. A stopped profile must
    // immediately display 00:00:00 and the next start begins from zero.
    rt.uptime_seconds = 0;
    rt.dev_tip_active = false;
}

#[derive(Debug, Clone, Serialize)]
pub struct MinerStatus {
    pub profile: MinerProfile,
    pub runtime: MinerRuntimeView,
    pub scheduled_now: bool,
    /// Manual Stop has suspended the currently active schedule occurrence.
    pub scheduler_paused: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct LogLine {
    pub ts: u64,
    pub stream: String,
    pub text: String,
}

pub struct CoreState {
    // Serialize user actions, scheduled starts/stops and internal restarts. A
    // scheduler tick using an older status must not undo a completed Stop.
    lifecycle: Mutex<()>,
    shutting_down: AtomicBool,
    manual_schedule: Mutex<ManualScheduleControl>,
    config_path: PathBuf,
    config_io: Mutex<()>,
    #[cfg(target_os = "windows")]
    config_sync_sender: Mutex<Option<std::sync::mpsc::Sender<AppConfig>>>,
    config: RwLock<AppConfig>,
    children: Mutex<HashMap<String, ManagedChild>>,
    runtime: Mutex<HashMap<String, MinerRuntimeView>>,
    logs: Mutex<HashMap<String, VecDeque<LogLine>>>,
    pending_power: Mutex<Option<PendingPowerAction>>,
    last_power_ui_seen: Mutex<u64>,
    dev_tip_credit_seconds: Mutex<HashMap<String, f64>>,
    dev_tip_until: Mutex<HashMap<String, u64>>,
    dev_tip_last_tick: Mutex<HashMap<String, u64>>,
    dev_tip_switching: Mutex<HashSet<String>>,
}

impl CoreState {
    fn load() -> Result<Arc<Self>, String> {
        let config_path = config_file_path()?;
        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("Création config: {e}"))?;
        }
        let config = if config_path.is_file() {
            match fs::read_to_string(&config_path)
                .ok()
                .and_then(|s| serde_json::from_str::<AppConfig>(&s).ok())
            {
                Some(v) => v,
                None => AppConfig::default(),
            }
        } else {
            AppConfig::default()
        };
        let stops_path = config_path.with_file_name("scheduler-manual-stops.json");
        let stopped: StopSnapshot = if stops_path.is_file() {
            let text = fs::read_to_string(&stops_path).map_err(|e| format!("Lecture {}: {e}", stops_path.display()))?;
            serde_json::from_str(&text).map_err(|e| format!("État des pauses invalide ({}): {e}", stops_path.display()))?
        } else { StopSnapshot::new() };
        let state = Arc::new(Self {
            lifecycle: Mutex::new(()),
            shutting_down: AtomicBool::new(false),
            manual_schedule: Mutex::new(ManualScheduleControl::from_snapshot(stopped)),
            config_path,
            config_io: Mutex::new(()),
            #[cfg(target_os = "windows")]
            config_sync_sender: Mutex::new(None),
            config: RwLock::new(config),
            children: Mutex::new(HashMap::new()),
            runtime: Mutex::new(HashMap::new()),
            logs: Mutex::new(HashMap::new()),
            pending_power: Mutex::new(None),
            last_power_ui_seen: Mutex::new(0),
            dev_tip_credit_seconds: Mutex::new(HashMap::new()),
            dev_tip_until: Mutex::new(HashMap::new()),
            dev_tip_last_tick: Mutex::new(HashMap::new()),
            dev_tip_switching: Mutex::new(HashSet::new()),
        });
        state.save_config()?;
        // Windows integrations are initialized by the HTTP-owning backend, not
        // synchronously here (the Desktop also loads this configuration).
        Ok(state)
    }

    fn write_config_file(&self, cfg: &AppConfig) -> Result<(), String> {
        let bytes = serde_json::to_vec_pretty(cfg).map_err(|e| e.to_string())?;
        let temporary = self.config_path.with_extension(format!("tmp-{}", Uuid::new_v4().simple()));
        let result = (|| -> std::io::Result<()> {
            let mut file = fs::File::create(&temporary)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            drop(file);
            fs::rename(&temporary, &self.config_path)
        })();
        if result.is_err() { let _ = fs::remove_file(&temporary); }
        result.map_err(|e| format!("Sauvegarde config: {e}"))
    }

    fn save_config(&self) -> Result<(), String> {
        let _io = self.config_io.lock().map_err(|_| "Config I/O lock poisoned".to_string())?;
        let cfg = self.config.read().map_err(|_| "Config verrouillée".to_string())?.clone();
        self.write_config_file(&cfg)
    }

    fn config(&self) -> AppConfig {
        self.config.read().map(|g| g.clone()).unwrap_or_default()
    }

    fn replace_config(&self, cfg: AppConfig) -> Result<(), String> {
        // Only local serialization/disk I/O belongs on the save critical path.
        // Previously every Start saved, then synchronously ran PowerShell once
        // per wake weekday + firewall: the browser timed out before POST /start.
        let _io = self.config_io.lock().map_err(|_| "Config I/O lock poisoned".to_string())?;
        let mut current = self.config.write().map_err(|_| "Configuration lock poisoned".to_string())?;
        // A disk failure must leave the in-memory profile unchanged too.
        self.write_config_file(&cfg)?;
        *current = cfg.clone();
        drop(current);
        #[cfg(target_os = "windows")]
        if let Ok(sender) = self.config_sync_sender.lock() {
            if let Some(sender) = sender.as_ref() {
                if let Err(e) = sender.send(cfg) {
                    backend_diagnostic_log(&format!("config saved, but Windows integration queue is unavailable: {e}"));
                }
            }
        }
        Ok(())
    }

    #[cfg(target_os = "windows")]
    fn start_windows_config_sync(&self) {
        // Exactly one ordered worker in the HTTP backend. The Desktop's local
        // CoreState must not compete with it for wake tasks or firewall rules.
        let Ok(_io) = self.config_io.lock() else { return; };
        let Ok(mut sender) = self.config_sync_sender.lock() else { return; };
        if sender.is_some() { return; }
        let mut applied_web: Option<(bool, u16)> = None;
        let mut applied_schedules: Option<Vec<ScheduleWindow>> = None;
        match config_sync::start_worker(self.config(), move |cfg: AppConfig| {
            let web_key = (cfg.web.expose_lan, cfg.web.desktop_api_port);
            if applied_web != Some(web_key) {
                match refresh_windows_web_firewall(&cfg.web) {
                    Ok(()) => { applied_web = Some(web_key); }
                    Err(e) => backend_diagnostic_log(&format!("background web firewall sync failed: {e}")),
                }
            }
            // Editing clocks/wallets/profiles does not recreate Windows wake tasks.
            if applied_schedules.as_ref() != Some(&cfg.schedules) {
                match sync_windows_wake_tasks(&cfg.schedules) {
                    Ok(()) => {
                        applied_schedules = Some(cfg.schedules.clone());
                        backend_diagnostic_log("background wake timer sync completed");
                    }
                    Err(e) => backend_diagnostic_log(&format!("background wake timer sync failed: {e}")),
                }
            }
        }) {
            Ok(worker) => { *sender = Some(worker); }
            Err(e) => backend_diagnostic_log(&format!("cannot start Windows integration worker: {e}")),
        }
    }

    fn pending_power_action(&self) -> Option<PendingPowerAction> {
        self.pending_power.lock().ok().and_then(|p| p.clone())
    }

    fn touch_power_ui(&self) {
        if let Ok(mut seen) = self.last_power_ui_seen.lock() { *seen = unix_now(); }
    }

    fn cancel_power_action(&self) {
        if let Ok(mut pending) = self.pending_power.lock() { *pending = None; }
    }

    fn request_power_action(&self, schedule: &ScheduleWindow) {
        let action = schedule.post_action.trim().to_ascii_lowercase();
        if action != "sleep" && action != "hibernate" { return; }
        let seconds = schedule.countdown_seconds.clamp(5, 3600);
        let request = PendingPowerAction {
            id: Uuid::new_v4().to_string(),
            schedule_id: schedule.id.clone(),
            schedule_name: schedule.name.clone(),
            action,
            deadline_unix: unix_now().saturating_add(seconds as u64),
            countdown_seconds: seconds,
            require_confirmation: schedule.require_confirmation,
        };
        backend_diagnostic_log(&format!(
            "power action requested after schedule '{}': {} in {}s (confirmation={})",
            request.schedule_name, request.action, seconds, request.require_confirmation
        ));
        if let Ok(mut pending) = self.pending_power.lock() { *pending = Some(request); }
    }

    fn confirm_power_action(&self) -> Result<(), String> {
        let action = self.pending_power.lock().map_err(|_| "Power action lock poisoned".to_string())?
            .take().ok_or_else(|| "No pending power action".to_string())?;
        // A manual mining restart after the schedule ended always wins over a
        // previously queued sleep/hibernate action. Never suspend a machine that
        // has resumed mining, even if a remote client confirms an old modal.
        if self.any_miner_running() {
            backend_diagnostic_log("power action canceled because mining resumed before confirmation");
            return Err("Mining has resumed; the pending power action was canceled.".into());
        }
        execute_power_action(&action.action)
    }

    fn process_pending_power_action(&self) {
        let pending = self.pending_power_action();
        let Some(action) = pending else { return; };
        // Manual Start/Restart can happen while the countdown is visible. The
        // power action must then disappear and must never race the new session.
        if self.any_miner_running() {
            backend_diagnostic_log("power action canceled because mining resumed");
            self.cancel_power_action();
            return;
        }
        if unix_now() < action.deadline_unix { return; }
        if action.require_confirmation {
            let seen = self.last_power_ui_seen.lock().map(|v| *v).unwrap_or(0);
            // The UI polls every second. If it is no longer open, cancel instead of sleeping.
            if unix_now().saturating_sub(seen) > 3 {
                backend_diagnostic_log("power action canceled because no confirmation UI is active");
                self.cancel_power_action();
                return;
            }
        }
        self.cancel_power_action();
        if let Err(e) = execute_power_action(&action.action) {
            backend_diagnostic_log(&format!("power action '{}' failed: {e}", action.action));
        }
    }

    fn record_log(self: &Arc<Self>, id: &str, stream: &str, text: String) {
        if let Ok(mut logs) = self.logs.lock() {
            let q = logs.entry(id.to_string()).or_default();
            q.push_back(LogLine { ts: unix_now(), stream: stream.to_string(), text: text.clone() });
            while q.len() > MAX_LOG_LINES { q.pop_front(); }
        }
        self.update_metrics_from_line(id, &text);
    }

    fn update_metrics_from_line(&self, id: &str, line: &str) {
        let mut runtimes = match self.runtime.lock() { Ok(v) => v, Err(_) => return };
        let rt = runtimes.entry(id.to_string()).or_insert_with(|| MinerRuntimeView { id: id.into(), ..Default::default() });
        let lower = line.to_ascii_lowercase();

        // Hashrate: accept H/s..PH/s. Prefer a value on any current status line.
        if let Some(hps) = parse_hashrate_hps(line) {
            if hps > 0.0 { rt.metrics.hashrate_hps = Some(hps); }
        }
        if let Some(w) = parse_power_w(line) { rt.metrics.power_w = Some(w); }
        if let Some(t) = parse_temperature_c(line) { rt.metrics.temperature_c = Some(t); }
        if let Some(f) = parse_fan_pct(line) { rt.metrics.fan_pct = Some(f); }

        if lower.contains("accepted") || lower.contains("share accepted") || lower.contains("solution accepted") {
            if !lower.contains("accepted 0") && !lower.contains("accepted: 0") { rt.metrics.accepted = rt.metrics.accepted.saturating_add(1); }
        }
        if lower.contains("rejected") || lower.contains("share rejected") || lower.contains("solution rejected") {
            if !lower.contains("rejected 0") && !lower.contains("rejected: 0") { rt.metrics.rejected = rt.metrics.rejected.saturating_add(1); }
        }
    }

    fn logs_for(&self, id: &str, limit: usize) -> Vec<LogLine> {
        self.logs.lock().ok().and_then(|m| m.get(id).cloned()).unwrap_or_default()
            .into_iter().rev().take(limit.min(MAX_LOG_LINES)).collect::<Vec<_>>()
            .into_iter().rev().collect()
    }

    fn refresh_processes(&self) {
        // Keep child removal and runtime updates in the same lock order used by
        // startup (children -> runtime). Never mark a replacement PID stopped
        // because an older process completed just before it was launched.
        if let Ok(mut children) = self.children.lock() {
            let ended: Vec<(String, Option<i32>)> = children.iter_mut()
                .filter_map(|(id, child)| child.try_wait().ok().flatten().map(|status| (id.clone(), status.code())))
                .collect();
            if !ended.is_empty() {
                if let Ok(mut runtime) = self.runtime.lock() {
                    for (id, code) in &ended {
                        children.remove(id);
                        if let Some(rt) = runtime.get_mut(id) {
                            mark_runtime_stopped(rt);
                            if code.unwrap_or(0) != 0 { rt.last_error = format!("Process terminé avec le code {:?}", code); }
                        }
                    }
                }
            }
        }
    }

    fn persist_manual_schedule(&self) -> Result<(), String> {
        let snapshot = self.manual_schedule.lock().map_err(|_| "Scheduler lock poisoned".to_string())?.snapshot().clone();
        let bytes = serde_json::to_vec_pretty(&snapshot).map_err(|e| e.to_string())?;
        let path = self.config_path.with_file_name("scheduler-manual-stops.json");
        let temporary = path.with_extension(format!("tmp-{}", Uuid::new_v4().simple()));
        let result = (|| -> std::io::Result<()> {
            let mut file = fs::File::create(&temporary)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            drop(file);
            fs::rename(&temporary, &path)
        })();
        if result.is_err() { let _ = fs::remove_file(&temporary); }
        result.map_err(|e| format!("Sauvegarde des pauses du planificateur ({}): {e}", path.display()))
    }

    fn is_running(&self, id: &str) -> bool {
        self.refresh_processes();
        self.runtime.lock().ok().and_then(|r| r.get(id).map(|x| x.running)).unwrap_or(false)
    }

    fn any_miner_running(&self) -> bool {
        self.refresh_processes();
        self.runtime.lock().map(|r| r.values().any(|x| x.running)).unwrap_or(false)
    }

    fn start_miner(self: &Arc<Self>, id: &str, started_by: &str) -> Result<(), String> {
        let _lifecycle = self.lifecycle.lock().map_err(|_| "Lifecycle lock poisoned".to_string())?;
        let active = schedule_occurrences(&self.config().schedules, Local::now());
        if started_by == "schedule" && (!active.contains_key(id) || self.manual_schedule.lock()
            .map_err(|_| "Scheduler lock poisoned".to_string())?.is_paused(id, &active)) { return Ok(()); }

        if started_by == "manual" {
            // Explicit user intent has priority over both the scheduler and a
            // sleep/hibernate countdown created when a schedule just ended.
            // Keep it as a genuinely manual session: the scheduler may only stop
            // processes it started itself. This also removes the end-boundary race
            // where a Start click could be classified as scheduled one instant
            // before the window closed and be killed by the next scheduler tick.
            self.cancel_power_action();
        }
        self.start_miner_mode_locked(id, started_by, false)?;
        if started_by == "manual" {
            // Start all may explicitly take ownership of an already-running
            // scheduled session; do not leave its origin as "schedule".
            if let Ok(mut runtime) = self.runtime.lock() {
                if let Some(rt) = runtime.get_mut(id) { rt.started_by = "manual".into(); }
            }
            let changed = self.manual_schedule.lock().map_err(|_| "Scheduler lock poisoned".to_string())?.resume(id);
            if changed { self.persist_manual_schedule()?; }
            self.record_log(id, "system", "[Scheduler] Manual Start: this session is user-owned and is not stopped when a schedule window ends.".into());
        }
        Ok(())
    }

    // The caller must hold lifecycle for every start/restart/stop operation.
    fn start_miner_mode_locked(self: &Arc<Self>, id: &str, started_by: &str, dev_tip_active: bool) -> Result<(), String> {
        if self.shutting_down.load(Ordering::SeqCst) { return Err("MinerDesk is shutting down".into()); }
        self.refresh_processes();
        if self.is_running(id) { return Ok(()); }
        let cfg = self.config();
        let mut profile = cfg.miners.iter().find(|m| m.id == id).cloned().ok_or_else(|| format!("Profil introuvable: {id}"))?;
        if !profile.enabled { return Err(format!("{} est désactivé", profile.name)); }
        if dev_tip_active {
            apply_dev_tip_wallets(&mut profile).ok_or_else(|| format!("Dev tip indisponible: réseau non reconnu pour {}", profile.name))?;
        }
        let path = PathBuf::from(profile.executable_path.trim());
        if !path.is_file() { return Err(format!("Exécutable introuvable pour {}: {}", profile.name, path.display())); }
        self.check_gpu_conflict(&profile)?;
        let args = build_engine_args(&profile)?;

        #[cfg(target_os = "windows")]
        let job = windows_miner_job::WindowsMinerJob::new().map_err(|e| {
            format!(
                "Windows miner crash guard is unavailable; refusing to start {} unprotected: {e}",
                profile.name
            )
        })?;

        let mut command = Command::new(&path);
        command.args(&args)
            .current_dir(path.parent().unwrap_or_else(|| Path::new(".")))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            const CREATE_SUSPENDED: u32 = 0x00000004;
            command.creation_flags(CREATE_NO_WINDOW | CREATE_SUSPENDED);
        }
        let mut child = command.spawn().map_err(|e| format!("Démarrage {}: {e}", profile.name))?;
        let pid = child.id();
        #[cfg(target_os = "windows")]
        {
            if let Err(e) = job.assign(&child) {
                // Fail closed: a miner that cannot be protected by the Job Object is
                // never allowed to continue running, otherwise a backend crash could
                // leave it orphaned and mining in the background.
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "Windows miner crash guard could not attach to {}: {e}",
                    profile.name
                ));
            }
            if let Err(e) = windows_miner_job::resume_process(pid) {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "Windows miner crash guard attached {}, but the suspended process could not be resumed safely: {e}",
                    profile.name
                ));
            }
        }
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        {
            let mut children = self.children.lock().map_err(|_| "Processus verrouillés".to_string())?;
            let mut runtime = self.runtime.lock().map_err(|_| "Runtime verrouillé".to_string())?;
            children.insert(id.to_string(), ManagedChild {
                child,
                #[cfg(target_os = "windows")]
                job,
            });
            runtime.insert(id.to_string(), MinerRuntimeView {
                id: id.to_string(), running: true, pid: Some(pid), started_at: Some(unix_now()), uptime_seconds: 0,
                started_by: started_by.into(), last_error: String::new(), metrics: MinerMetrics::default(), dev_tip_active,
            });
        }
        let mode = if dev_tip_active { "DEV TIP" } else { "USER" };
        self.record_log(id, "system", format!("[MinerDesk] [{mode}] {} {}", path.display(), args.join(" ")));
        #[cfg(target_os = "windows")]
        self.record_log(id, "system", "[MinerDesk] Windows crash guard active: Job Object / KILL_ON_JOB_CLOSE".into());
        if let Some(out) = stdout { spawn_reader(out, Arc::clone(self), id.to_string(), "stdout"); }
        if let Some(err) = stderr { spawn_reader(err, Arc::clone(self), id.to_string(), "stderr"); }
        Ok(())
    }

    fn is_dev_tip_switching(&self, id: &str) -> bool {
        self.dev_tip_switching.lock().map(|s| s.contains(id)).unwrap_or(false)
    }

    fn restart_miner_mode(self: &Arc<Self>, id: &str, dev_tip_active: bool) -> Result<(), String> {
        let _lifecycle = self.lifecycle.lock().map_err(|_| "Lifecycle lock poisoned".to_string())?;
        // The dev-tip monitor may have captured a running status before the user
        // stopped it. Never resurrect that session from the stale snapshot.
        if self.shutting_down.load(Ordering::SeqCst) || !self.is_running(id) { return Ok(()); }
        let started_by = self.runtime.lock().ok().and_then(|r| r.get(id).map(|x| x.started_by.clone())).unwrap_or_else(|| "manual".into());
        {
            let mut switching = self.dev_tip_switching.lock().map_err(|_| "Dev tip lock poisoned".to_string())?;
            switching.insert(id.to_string());
        }
        let result = (|| {
            self.stop_miner_locked(id)?;
            thread::sleep(Duration::from_millis(350));
            self.start_miner_mode_locked(id, &started_by, dev_tip_active)
        })();
        if let Ok(mut switching) = self.dev_tip_switching.lock() { switching.remove(id); }
        result
    }

    fn stop_miner_locked(&self, id: &str) -> Result<(), String> {
        let managed = self.children.lock().map_err(|_| "Processus verrouillés".to_string())?.remove(id);
        if let Some(mut managed) = managed {
            managed.terminate();
        }
        if let Ok(mut runtime) = self.runtime.lock() {
            let rt = runtime.entry(id.to_string()).or_insert_with(|| MinerRuntimeView { id: id.into(), ..Default::default() });
            mark_runtime_stopped(rt);
        }
        Ok(())
    }

    fn record_command_error(self: &Arc<Self>, id: &str, action: &str, message: &str) {
        backend_diagnostic_log(&format!("miner command {action} failed for profile {id}: {message}"));
        self.record_log(id, "system", format!("[MinerDesk] {action} failed: {message}"));
        if let Ok(mut runtime) = self.runtime.lock() {
            let rt = runtime.entry(id.to_string()).or_insert_with(|| MinerRuntimeView { id: id.into(), ..Default::default() });
            rt.last_error = message.into();
        }
    }

    fn stop_miner_manually(self: &Arc<Self>, id: &str) -> Result<(), String> {
        let _lifecycle = self.lifecycle.lock().map_err(|_| "Lifecycle lock poisoned".to_string())?;
        let active = schedule_occurrences(&self.config().schedules, Local::now());
        let changed = self.manual_schedule.lock().map_err(|_| "Scheduler lock poisoned".to_string())?.pause(id, &active);
        // Always stop, including when persistence fails (in-memory pause still
        // prevents an immediate restart). Report any disk failure to the UI.
        let persisted = if changed { self.persist_manual_schedule() } else { Ok(()) };
        self.stop_miner_locked(id)?;
        if active.contains_key(id) {
            self.record_log(id, "system", "[Scheduler] Manual Stop: current schedule occurrence paused. Start resumes it; a later occurrence runs normally.".into());
        }
        persisted
    }

    fn stop_all_manually(self: &Arc<Self>) -> Result<(), String> {
        let _lifecycle = self.lifecycle.lock().map_err(|_| "Lifecycle lock poisoned".to_string())?;
        let active = schedule_occurrences(&self.config().schedules, Local::now());
        let changed = self.manual_schedule.lock().map_err(|_| "Scheduler lock poisoned".to_string())?.pause_all(&active);
        let persisted = if changed { self.persist_manual_schedule() } else { Ok(()) };
        let ids: Vec<String> = self.children.lock().map_err(|_| "Processus verrouillés".to_string())?.keys().cloned().collect();
        for id in ids { self.stop_miner_locked(&id)?; }
        for id in active.keys() {
            self.record_log(id, "system", "[Scheduler] Stop all: current schedule occurrence paused until its end or manual Start.".into());
        }
        persisted
    }

    fn restart_miner_manually(self: &Arc<Self>, id: &str) -> Result<(), String> {
        let _lifecycle = self.lifecycle.lock().map_err(|_| "Lifecycle lock poisoned".to_string())?;
        // Restart is an explicit manual override too. Cancel a pending power
        // action first so the countdown cannot suspend the newly restarted miner.
        self.cancel_power_action();
        self.stop_miner_locked(id)?;
        self.start_miner_mode_locked(id, "manual", false)?;
        let changed = self.manual_schedule.lock().map_err(|_| "Scheduler lock poisoned".to_string())?.resume(id);
        if changed { self.persist_manual_schedule()?; }
        self.record_log(id, "system", "[Scheduler] Manual Restart: this session is user-owned and is not stopped when a schedule window ends.".into());
        Ok(())
    }

    // Internal shutdown, not the user-facing Stop all command. Once shutdown
    // begins, no scheduler or dev-tip restart may create a new child process.
    fn stop_all(&self) {
        self.shutting_down.store(true, Ordering::SeqCst);
        if let Ok(_lifecycle) = self.lifecycle.lock() {
            let ids: Vec<String> = self.children.lock().map(|m| m.keys().cloned().collect()).unwrap_or_default();
            for id in ids { let _ = self.stop_miner_locked(&id); }
        }
    }

    fn statuses(&self) -> Vec<MinerStatus> {
        self.refresh_processes();
        let cfg = self.config();
        let now = Local::now();
        let active_occurrences = schedule_occurrences(&cfg.schedules, now);
        let active_sched: HashSet<String> = active_occurrences.keys().cloned().collect();
        let control = self.manual_schedule.lock().map(|x| x.clone()).unwrap_or_default();
        let runtime = self.runtime.lock().map(|x| x.clone()).unwrap_or_default();
        cfg.miners.into_iter().map(|p| {
            let mut rt = runtime.get(&p.id).cloned().unwrap_or_else(|| MinerRuntimeView { id: p.id.clone(), ..Default::default() });
            if rt.running {
                if let Some(start) = rt.started_at { rt.uptime_seconds = unix_now().saturating_sub(start); }
            } else {
                rt.started_at = None;
                rt.uptime_seconds = 0;
            }
            MinerStatus { scheduled_now: active_sched.contains(&p.id), scheduler_paused: control.is_paused(&p.id, &active_occurrences), profile: p, runtime: rt }
        }).collect()
    }

    fn check_gpu_conflict(&self, candidate: &MinerProfile) -> Result<(), String> {
        if candidate.allow_gpu_overlap { return Ok(()); }
        let cfg = self.config();
        let running: HashSet<String> = self.statuses().into_iter().filter(|s| s.runtime.running).map(|s| s.profile.id).collect();
        let cand = gpu_set(&candidate.gpu_ids);
        for other in cfg.miners.iter().filter(|p| running.contains(&p.id)) {
            if other.allow_gpu_overlap { continue; }
            let oth = gpu_set(&other.gpu_ids);
            let conflict = cand.is_none() || oth.is_none() || cand.as_ref().unwrap().iter().any(|g| oth.as_ref().unwrap().contains(g));
            if conflict {
                return Err(format!("Conflit GPU avec '{}'. Renseigne des GPU IDs distincts ou active 'Autoriser partage GPU'.", other.name));
            }
        }
        Ok(())
    }
}

fn gpu_set(input: &str) -> Option<HashSet<String>> {
    let s = input.trim();
    if s.is_empty() { return None; } // empty = all GPUs
    Some(s.split(|c| c == ',' || c == ' ' || c == ';').map(str::trim).filter(|x| !x.is_empty()).map(str::to_string).collect())
}

fn spawn_reader<R: Read + Send + 'static>(reader: R, state: Arc<CoreState>, id: String, stream: &'static str) {
    thread::spawn(move || {
        let buf = BufReader::new(reader);
        for line in buf.lines().map_while(Result::ok) { state.record_log(&id, stream, line); }
    });
}

fn unix_now() -> u64 { SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() }

fn config_file_path() -> Result<PathBuf, String> {
    let base = dirs_next::config_dir().ok_or_else(|| "Dossier de configuration introuvable".to_string())?;
    Ok(base.join("MinerDesk").join("config.json"))
}

fn backend_diagnostic_log(message: &str) {
    let Some(base) = dirs_next::config_dir() else { return; };
    let dir = base.join("MinerDesk");
    let _ = fs::create_dir_all(&dir);
    let path = dir.join("backend.log");
    if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "[{}] {}", Local::now().to_rfc3339(), message);
    }
}

fn parse_hashrate_hps(line: &str) -> Option<f64> {
    // SRBMiner prints historical averages (1 hr / 6 hr / 12 hr) as 0 H/s
    // during the first hours of a session. Those lines must never overwrite
    // the current hashrate that was parsed just above in the live statistics.
    let lower = line.to_ascii_lowercase();
    let historical = Regex::new(r"(?i)\b(1|6|12|24)\s*(min|hr|hour|hours)\b").ok()?;
    if historical.is_match(&lower) { return None; }

    let re = Regex::new(r"(?i)([0-9]+(?:\.[0-9]+)?)\s*(H/s|kH/s|MH/s|GH/s|TH/s|PH/s)").ok()?;
    let mut values = vec![];
    for c in re.captures_iter(line) {
        let v: f64 = c.get(1)?.as_str().parse().ok()?;
        // A zero value is normally a long-term average which has not been
        // populated yet. Keeping the last positive live value is safer for a
        // common multi-miner dashboard.
        if v <= 0.0 { continue; }
        let u = c.get(2)?.as_str().to_ascii_lowercase();
        let mul = match u.as_str() { "h/s" => 1.0, "kh/s" => 1e3, "mh/s" => 1e6, "gh/s" => 1e9, "th/s" => 1e12, "ph/s" => 1e15, _ => 1.0 };
        values.push(v * mul);
    }
    values.into_iter().reduce(f64::max)
}

fn parse_power_w(line: &str) -> Option<f64> {
    let re = Regex::new(r"(?i)\b([0-9]+(?:\.[0-9]+)?)\s*W\b").ok()?;
    re.captures_iter(line).filter_map(|c| c.get(1)?.as_str().parse::<f64>().ok()).reduce(f64::max)
}
fn parse_temperature_c(line: &str) -> Option<f64> {
    let re = Regex::new(r"(?i)\b([2-9][0-9]|1[01][0-9])\s*(?:°\s*)?C\b").ok()?;
    re.captures_iter(line).filter_map(|c| c.get(1)?.as_str().parse::<f64>().ok()).reduce(f64::max)
}
fn parse_fan_pct(line: &str) -> Option<f64> {
    let re = Regex::new(r"\b([0-9]{1,3})\s*%\b").ok()?;
    let fan = re
        .captures_iter(line)
        .filter_map(|c| c.get(1)?.as_str().parse::<f64>().ok())
        .find(|v| *v <= 100.0);
    fan
}

// ---------- Miner engines ----------

#[derive(Debug, Clone, Serialize)]
pub struct EngineSpec {
    pub id: &'static str,
    pub name: &'static str,
    pub github_repo: Option<&'static str>,
    pub auto_download: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuDevice {
    /// Value passed to the selected mining engine (e.g. "0" or "1:0").
    pub selector: String,
    pub name: String,
    pub vendor: String,
    pub pci_bus: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GpuDiscovery {
    pub engine: String,
    pub source: String,
    pub devices: Vec<GpuDevice>,
    pub raw_excerpt: String,
    pub selection_hint: String,
}

fn engine_specs() -> Vec<EngineSpec> {
    vec![
        EngineSpec { id: "srbminer", name: "SRBMiner-Multi", github_repo: Some("doktor83/SRBMiner-Multi"), auto_download: true },
        EngineSpec { id: "lolminer", name: "lolMiner", github_repo: Some("Lolliedieb/lolMiner-releases"), auto_download: true },
        EngineSpec { id: "bzminer", name: "BzMiner", github_repo: Some("bzminer/bzminer"), auto_download: true },
        EngineSpec { id: "rigel", name: "Rigel", github_repo: Some("rigelminer/rigel"), auto_download: true },
        EngineSpec { id: "lpminer", name: "lpminer", github_repo: Some("BaikalMine-Pools/pearl-miner"), auto_download: true },
        EngineSpec { id: "npminer", name: "NPMiner", github_repo: Some("nushypool/npminer"), auto_download: true },
        EngineSpec { id: "custom", name: "Mineur personnalisé", github_repo: None, auto_download: false },
    ]
}


fn gpu_tokens(input: &str) -> Vec<String> {
    input
        .split(|c| c == ',' || c == ';' || c == ' ' || c == '\t')
        .map(str::trim)
        .filter(|x| !x.is_empty())
        .map(str::to_string)
        .collect()
}

fn engine_gpu_hint(engine: &str) -> &'static str {
    match engine {
        "srbminer" => "SRBMiner: --gpu-id 0,1,2 (liste séparée par des virgules)",
        "lolminer" => "lolMiner: --devices 0,1,2 (liste séparée par des virgules)",
        "rigel" => "Rigel: -d 0,1,2 (liste séparée par des virgules)",
        "bzminer" => "BzMiner: --enable 1:0 3:0 (identifiants PCI passés comme arguments séparés)",
        "npminer" => "NPMiner: --devices 0,1,2 (Index de --list-gpus, pas le Bus ID)",
        "lpminer" => "lpminer: détection GPU via le système; Core clock est transmis avec --lock-core-clock. La sélection GPU reste à configurer via Arguments avancés si nécessaire.",
        _ => "Mineur personnalisé: les GPU sélectionnés sont conservés dans le profil; utilisez Arguments avancés si nécessaire.",
    }
}

fn engine_list_device_args(engine: &str) -> Option<Vec<&'static str>> {
    match engine {
        "srbminer" => Some(vec!["--list-devices"]),
        "lolminer" => Some(vec!["--list-devices"]),
        "rigel" => Some(vec!["--list-devices"]),
        "bzminer" => Some(vec!["--devices"]),
        "npminer" => Some(vec!["--list-gpus"]),
        _ => None,
    }
}

fn clean_gpu_name(name: &str) -> String {
    name.trim()
        .trim_matches('|')
        .trim()
        .replace('_', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn vendor_from_name(name: &str) -> String {
    let low = name.to_ascii_lowercase();
    if low.contains("nvidia") || low.contains("geforce") || low.contains("quadro") { "NVIDIA".into() }
    else if low.contains("amd") || low.contains("radeon") { "AMD".into() }
    else if low.contains("intel") || low.contains("arc") { "Intel".into() }
    else { "GPU".into() }
}

fn push_gpu_unique(out: &mut Vec<GpuDevice>, selector: String, name: String, pci_bus: Option<String>) {
    let selector = selector.trim().to_string();
    let name = clean_gpu_name(&name);
    if selector.is_empty() || name.len() < 3 { return; }
    if out.iter().any(|g| g.selector == selector) { return; }
    out.push(GpuDevice { vendor: vendor_from_name(&name), selector, name, pci_bus });
}

fn parse_engine_gpu_output(engine: &str, text: &str) -> Vec<GpuDevice> {
    let mut out = Vec::new();
    match engine {
        "srbminer" => {
            // Typical: GPU0 : nvidia_geforce_rtx_5060_ti [blackwell] ... [BUS: 01]
            let re = Regex::new(r"(?im)^\s*GPU\s*([0-9]+)\s*:\s*([^\r\n\[]+)(?:.*?\[BUS:\s*([^\]]+)\])?").unwrap();
            for c in re.captures_iter(text) {
                push_gpu_unique(&mut out, c[1].to_string(), c[2].to_string(), c.get(3).map(|m| m.as_str().trim().to_string()));
            }
        }
        "lolminer" | "rigel" => {
            // Covers "Device 0: ...", "GPU #0: ...", "0: NVIDIA ...".
            let patterns = [
                r"(?im)^\s*(?:Device|GPU)\s*#?\s*([0-9]+)\s*[:|-]\s*([^\r\n]+)$",
                r"(?im)^\s*([0-9]+)\s*[:|-]\s*((?:NVIDIA|AMD|Intel|GeForce|Radeon|Arc)[^\r\n]+)$",
            ];
            for pat in patterns {
                let re = Regex::new(pat).unwrap();
                for c in re.captures_iter(text) {
                    push_gpu_unique(&mut out, c[1].to_string(), c[2].to_string(), None);
                }
            }
        }
        "npminer" => {
            // NPMiner --list-gpus: "Index | Bus ID | Name". --devices uses Index.
            let re = Regex::new(r"(?im)^\s*([0-9]+)\s*\|\s*([^|\r\n]+)\s*\|\s*([^|\r\n]+)\s*$").unwrap();
            for c in re.captures_iter(text) {
                let idx = c[1].trim().to_string();
                let bus = c[2].trim().to_string();
                let name = c[3].trim().to_string();
                if name.to_ascii_lowercase().contains("name") { continue; }
                push_gpu_unique(&mut out, idx, name, (!bus.is_empty()).then_some(bus));
            }
        }
        "bzminer" => {
            // BzMiner selects devices using PCI bus:device (e.g. 1:0). Its device table
            // commonly contains rows like "| 1:0 | GeForce RTX ... |".
            let re = Regex::new(r"(?im)^.*?\b([0-9a-fA-F]+:[0-9a-fA-F]+)\b\s*\|?\s*((?:NVIDIA|AMD|Intel|GeForce|Radeon|Arc)[^|\r\n]+)").unwrap();
            for c in re.captures_iter(text) {
                push_gpu_unique(&mut out, c[1].to_string(), c[2].to_string(), Some(c[1].to_string()));
            }
        }
        _ => {}
    }
    out
}

// Commands used only for diagnostics must not create a new console when the
// real backend is a Windows GUI-subsystem executable. Their captured output
// remains available to the Web/UI; this does not hide a user's CLI terminal.
fn background_command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    #[allow(unused_mut)]
    let mut command = Command::new(program);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    command
}

#[cfg(target_os = "windows")]
fn system_gpu_fallback() -> Vec<GpuDevice> {
    let mut out = Vec::new();
    if let Ok(cmd) = background_command("nvidia-smi")
        .args(["--query-gpu=index,name,pci.bus_id", "--format=csv,noheader,nounits"])
        .output()
    {
        if cmd.status.success() {
            let text = String::from_utf8_lossy(&cmd.stdout);
            for line in text.lines() {
                let parts: Vec<_> = line.split(',').map(str::trim).collect();
                if parts.len() >= 2 {
                    push_gpu_unique(&mut out, parts[0].to_string(), parts[1].to_string(), parts.get(2).map(|x| x.to_string()));
                }
            }
        }
    }
    // Add AMD/Intel adapters when nvidia-smi cannot see them. Selectors here are
    // indicative only; the engine-specific discovery above remains preferred.
    let ps = r#"$g=Get-CimInstance Win32_VideoController | Select-Object Name,PNPDeviceID; $g | ConvertTo-Json -Compress"#;
    if let Ok(o) = powershell_command().args(["-NoProfile","-NonInteractive","-ExecutionPolicy","Bypass","-Command",ps]).output() {
        if o.status.success() {
            if let Ok(v) = serde_json::from_slice::<Value>(&o.stdout) {
                let items: Vec<Value> = match v { Value::Array(a) => a, other => vec![other] };
                let mut next = out.iter().filter_map(|g| g.selector.parse::<u32>().ok()).max().unwrap_or(0) + 1;
                for item in items {
                    let name = item.get("Name").and_then(Value::as_str).unwrap_or("");
                    if name.is_empty() || out.iter().any(|g| g.name.eq_ignore_ascii_case(name)) { continue; }
                    let low=name.to_ascii_lowercase();
                    if !(low.contains("amd")||low.contains("radeon")||low.contains("intel")||low.contains("arc")) { continue; }
                    push_gpu_unique(&mut out, next.to_string(), name.to_string(), None); next += 1;
                }
            }
        }
    }
    out
}

#[cfg(not(target_os = "windows"))]
fn system_gpu_fallback() -> Vec<GpuDevice> {
    let mut out = Vec::new();
    if let Ok(cmd) = background_command("nvidia-smi")
        .args(["--query-gpu=index,name,pci.bus_id", "--format=csv,noheader,nounits"])
        .output()
    {
        if cmd.status.success() {
            let text = String::from_utf8_lossy(&cmd.stdout);
            for line in text.lines() {
                let parts: Vec<_> = line.split(',').map(str::trim).collect();
                if parts.len() >= 2 { push_gpu_unique(&mut out, parts[0].into(), parts[1].into(), parts.get(2).map(|x| x.to_string())); }
            }
        }
    }
    out
}

fn discover_gpus(profile: &MinerProfile) -> GpuDiscovery {
    let mut raw = String::new();
    let mut devices = Vec::new();
    let mut source = "system".to_string();
    let path = PathBuf::from(profile.executable_path.trim());
    if path.is_file() {
        if let Some(args) = engine_list_device_args(&profile.engine) {
            if let Ok(out) = background_command(&path)
                .args(args)
                .current_dir(path.parent().unwrap_or_else(|| Path::new(".")))
                .output()
            {
                raw.push_str(&String::from_utf8_lossy(&out.stdout));
                raw.push_str(&String::from_utf8_lossy(&out.stderr));
                devices = parse_engine_gpu_output(&profile.engine, &raw);
                if !devices.is_empty() { source = format!("{} --list-devices", profile.engine); }
            }
        }
    }
    if devices.is_empty() { devices = system_gpu_fallback(); }
    let raw_excerpt = raw.lines().take(80).collect::<Vec<_>>().join("\n");
    GpuDiscovery { engine: profile.engine.clone(), source, devices, raw_excerpt, selection_hint: engine_gpu_hint(&profile.engine).into() }
}

fn tuning_for<'a>(p: &'a MinerProfile, selector: &str) -> Option<&'a GpuTuning> {
    p.gpu_tuning.iter().find(|g| g.selector == selector)
}

fn effective_core_clock(p: &MinerProfile, selector: &str) -> Option<u32> {
    tuning_for(p, selector).and_then(|g| g.core_clock).or(p.core_clock)
}
fn effective_power_limit(p: &MinerProfile, selector: &str) -> Option<u32> {
    tuning_for(p, selector).and_then(|g| g.power_limit).or(p.power_limit)
}
fn effective_fan(p: &MinerProfile, selector: &str) -> Option<u8> {
    tuning_for(p, selector).and_then(|g| g.fan).or(p.fan)
}

/// lpminer 0.1.x accepts a single absolute `--lock-core-clock` value.
/// For an explicit multi-GPU selection we only emit it when every selected GPU
/// resolves to the same value; this avoids silently applying one card's tuning
/// to another card that was configured differently.
fn lpminer_shared_core_clock(p: &MinerProfile, selected: &[String]) -> Result<Option<u32>, String> {
    if selected.is_empty() {
        return Ok(p.core_clock);
    }
    let values: Vec<Option<u32>> = selected.iter().map(|id| effective_core_clock(p, id)).collect();
    if values.iter().all(Option::is_none) {
        return Ok(None);
    }
    let first = values[0];
    if first.is_some() && values.iter().all(|v| *v == first) {
        return Ok(first);
    }
    Err(
        "lpminer supports one shared --lock-core-clock value for the current profile. Set the same Core clock MHz on all selected GPUs, or use Advanced arguments for a miner-specific configuration.".into(),
    )
}

/// Build a comma-separated per-GPU option for SRBMiner. SRBMiner maps the value
/// list to the explicit --gpu-id list, so every selected GPU must have a value.
fn srb_numeric_list<F>(p: &MinerProfile, selected: &[String], get: F) -> Option<String>
where F: Fn(&MinerProfile, &str) -> Option<u32> {
    if selected.is_empty() { return None; }
    let vals: Option<Vec<String>> = selected.iter().map(|id| get(p, id).map(|v| v.to_string())).collect();
    vals.map(|v| v.join(","))
}
fn srb_fan_list(p: &MinerProfile, selected: &[String]) -> Option<String> {
    if selected.is_empty() { return None; }
    let vals: Option<Vec<String>> = selected.iter().map(|id| effective_fan(p, id).map(|v| v.to_string())).collect();
    vals.map(|v| v.join(","))
}

/// lolMiner and Rigel index OC arrays by physical numeric GPU index. They both
/// support a skip marker, allowing MinerDesk to tune only the selected cards.
fn indexed_list_u32<F>(p: &MinerProfile, selected: &[String], skip: &str, get: F) -> Option<String>
where F: Fn(&MinerProfile, &str) -> Option<u32> {
    let numeric: Vec<usize> = selected.iter().filter_map(|s| s.parse::<usize>().ok()).collect();
    let max = *numeric.iter().max()?;
    let selected_set: HashSet<usize> = numeric.into_iter().collect();
    let mut any = false;
    let mut vals = Vec::with_capacity(max + 1);
    for idx in 0..=max {
        if selected_set.contains(&idx) {
            if let Some(v) = get(p, &idx.to_string()) { vals.push(v.to_string()); any = true; }
            else { vals.push(skip.to_string()); }
        } else { vals.push(skip.to_string()); }
    }
    any.then(|| vals.join(","))
}
fn indexed_list_fan(p: &MinerProfile, selected: &[String], skip: &str) -> Option<String> {
    let numeric: Vec<usize> = selected.iter().filter_map(|s| s.parse::<usize>().ok()).collect();
    let max = *numeric.iter().max()?;
    let selected_set: HashSet<usize> = numeric.into_iter().collect();
    let mut any = false;
    let mut vals = Vec::with_capacity(max + 1);
    for idx in 0..=max {
        if selected_set.contains(&idx) {
            if let Some(v) = effective_fan(p, &idx.to_string()) { vals.push(v.to_string()); any = true; }
            else { vals.push(skip.to_string()); }
        } else { vals.push(skip.to_string()); }
    }
    any.then(|| vals.join(","))
}


// ---------- Voluntary MinerDesk developer tip ----------
// Public receive-only addresses. No seed phrase or private key is stored in MinerDesk.
const DEV_WALLET_PRL: &str = "prl1pdgk8q6ll2xpuxaly8j20trwcuv9ame5w3wwgzvp3608hru2zdw5qugmezj";
const DEV_WALLET_NOCK: &str = "4noS1vZmmrR4dxihNDJfPpR59yzgR2e6FhQYxq4VXQB3X5umPeRuknZ";
const DEV_WALLET_EVM: &str = "0x6F24F64E8d928f895cdD98909f0C3BaEDe6D92Ed";
const DEV_WALLET_ALPH: &str = "1DHz1Q6ZqhHy3HNnSyAcF3swmTb113JgYUubR499FQE6u";
const DEV_WALLET_XELIS: &str = "xel:egw8y9dcqrtwpvzskd7uqvkadw9dc0a6028knul466jadeywy98sq3wnezx";
const DEV_WALLET_NIRMATA: &str = "Nn4LXk2ibTw921dpwnE8McEycY7wUhqxwXyjf4GNM41sitgG73DwU8Kb9axryEnhVwbesaoZPhTN4TZAghAjPdSh1LQ9f6M5j";
const DEV_WALLET_ERGO: &str = "9hX7KANFKMrVwLDwHs5jfrqurt7v5YoNaUAGAPQixGZzt7GLDrh";
const DEV_WALLET_KASPA: &str = "kaspa:qpsxzw40qqvsmaesauq88zf64h9y8aamjkdgf7mqx6g3t8wegtdyq7lps8h2a";
const DEV_WALLET_NEXA: &str = "nexa:nqtsq5g5dkpjw57p3us7reu3egaq8475vmk0y8wxae3uattt";
const DEV_WALLET_MONERO: &str = "47MSeMcp8uBcRbFTc8PpVTFy29UUGtT2uG6W6jRioyUsVxU2tcbzLHFhkgQQxE9CYUSLTpGtbWiWRX1Uw5LgfDWHNWXwxGF";
const DEV_WALLET_FLUX: &str = "t1YABDpWK7aaazKcji4r7Yxzicyk1ozJkuf";

fn detected_coin_id(p: &MinerProfile) -> Option<&'static str> {
    let wallet = p.wallet.trim().to_ascii_lowercase();
    let algo = p.algorithm.trim().to_ascii_lowercase();
    let pool = p.pool.trim().to_ascii_lowercase();
    if wallet.starts_with("prl1") || algo.contains("pearl") || pool.contains("pearl") { return Some("PRL"); }
    if wallet.starts_with("0x") { return Some("EVM"); }
    if algo == "alph" || algo == "aleph" || algo.contains("alephium") || pool.contains("alephium") { return Some("ALPH"); }
    if wallet.starts_with("xel:") || algo.contains("xelis") || pool.contains("xelis") { return Some("XELIS"); }
    if wallet.starts_with("kaspa:") || algo == "kaspa" || pool.contains("kaspa") { return Some("KASPA"); }
    if wallet.starts_with("nexa:") || algo.contains("nexa") || pool.contains("nexa") { return Some("NEXA"); }
    if wallet.starts_with("nn") || algo.contains("nirmata") || pool.contains("nirmata") { return Some("NIRMATA"); }
    if algo.contains("autolykos") || algo == "ergo" || pool.contains("ergo") { return Some("ERGO"); }
    if algo.contains("randomx") || algo == "monero" || pool.contains("monero") || pool.contains("xmr") { return Some("MONERO"); }
    if algo.contains("zelhash") || algo == "flux" || pool.contains("flux") { return Some("FLUX"); }
    None
}

fn dev_wallet_for_coin(coin: &str) -> Option<&'static str> {
    match coin {
        "PRL" => Some(DEV_WALLET_PRL),
        "NOCK" => Some(DEV_WALLET_NOCK),
        "EVM" => Some(DEV_WALLET_EVM),
        "ALPH" => Some(DEV_WALLET_ALPH),
        "XELIS" => Some(DEV_WALLET_XELIS),
        "NIRMATA" => Some(DEV_WALLET_NIRMATA),
        "ERGO" => Some(DEV_WALLET_ERGO),
        "KASPA" => Some(DEV_WALLET_KASPA),
        "NEXA" => Some(DEV_WALLET_NEXA),
        "MONERO" => Some(DEV_WALLET_MONERO),
        "FLUX" => Some(DEV_WALLET_FLUX),
        _ => None,
    }
}

/// Replaces only public payout addresses in a cloned profile. Returns the detected network.
/// Custom engines are deliberately excluded because their wallet may be embedded in arbitrary arguments.
fn apply_dev_tip_wallets(p: &mut MinerProfile) -> Option<&'static str> {
    if p.engine.eq_ignore_ascii_case("custom") { return None; }
    let coin = detected_coin_id(p)?;
    p.wallet = dev_wallet_for_coin(coin)?.to_string();

    if p.merge_secondary && !p.secondary_wallet.trim().is_empty() {
        // LuckyPool PRL merge mining uses PRL+NOCK. For other known secondary
        // wallets we try normal prefix/algorithm detection; otherwise disable
        // the secondary leg for the dev slice rather than redirecting user rewards.
        if coin == "PRL" {
            p.secondary_wallet = DEV_WALLET_NOCK.to_string();
        } else {
            let mut secondary = p.clone();
            secondary.wallet = p.secondary_wallet.clone();
            if let Some(secondary_coin) = detected_coin_id(&secondary) {
                if let Some(addr) = dev_wallet_for_coin(secondary_coin) {
                    p.secondary_wallet = addr.to_string();
                } else {
                    p.merge_secondary = false;
                }
            } else {
                p.merge_secondary = false;
            }
        }
    }
    Some(coin)
}

fn start_dev_tip_monitor(state: Arc<CoreState>) {
    const MIN_DEV_SLICE_SECONDS: f64 = 30.0;
    const MAX_DEV_SLICE_SECONDS: f64 = 180.0;
    thread::spawn(move || loop {
        let now = unix_now();
        state.refresh_processes();
        let cfg = state.config();
        let runtime = state.runtime.lock().map(|r| r.clone()).unwrap_or_default();

        for profile in &cfg.miners {
            let pct = profile.dev_tip_percent.clamp(0.0, 10.0);
            let Some(rt) = runtime.get(&profile.id) else { continue; };
            if !rt.running {
                if let Ok(mut last) = state.dev_tip_last_tick.lock() { last.insert(profile.id.clone(), now); }
                if let Ok(mut until) = state.dev_tip_until.lock() { until.remove(&profile.id); }
                continue;
            }
            let mut dev_probe = profile.clone();
            let supported = apply_dev_tip_wallets(&mut dev_probe).is_some();
            if rt.dev_tip_active && (pct <= 0.0 || !supported) {
                if !state.is_dev_tip_switching(&profile.id) {
                    let _ = state.restart_miner_mode(&profile.id, false);
                    if let Ok(mut until) = state.dev_tip_until.lock() { until.remove(&profile.id); }
                }
                continue;
            }
            if pct <= 0.0 || !supported {
                if let Ok(mut last) = state.dev_tip_last_tick.lock() { last.insert(profile.id.clone(), now); }
                continue;
            }

            if rt.dev_tip_active {
                let deadline = state.dev_tip_until.lock().ok().and_then(|m| m.get(&profile.id).copied()).unwrap_or(0);
                if deadline > 0 && now >= deadline && state.is_running(&profile.id) && !state.is_dev_tip_switching(&profile.id) {
                    if let Err(e) = state.restart_miner_mode(&profile.id, false) {
                        state.record_log(&profile.id, "system", format!("[Dev tip] Unable to return to user wallet: {e}"));
                    } else {
                        state.record_log(&profile.id, "system", "[Dev tip] Back to your wallet. Thank you for supporting MinerDesk ♥".into());
                        if let Ok(mut until) = state.dev_tip_until.lock() { until.remove(&profile.id); }
                        if let Ok(mut last) = state.dev_tip_last_tick.lock() { last.insert(profile.id.clone(), now); }
                    }
                }
                continue;
            }

            let last = {
                let mut ticks = match state.dev_tip_last_tick.lock() { Ok(v) => v, Err(_) => continue };
                let previous = ticks.insert(profile.id.clone(), now).unwrap_or(now);
                previous
            };
            let elapsed = now.saturating_sub(last) as f64;
            if elapsed <= 0.0 { continue; }
            // Accrue dev seconds only while the user's wallet is mining. p/(100-p)
            // makes dev_time / total_time converge to the selected percentage.
            let ratio = pct / (100.0 - pct);
            let credit_now = {
                let mut credits = match state.dev_tip_credit_seconds.lock() { Ok(v) => v, Err(_) => continue };
                let credit = credits.entry(profile.id.clone()).or_insert(0.0);
                *credit += elapsed * ratio;
                *credit
            };
            if credit_now < MIN_DEV_SLICE_SECONDS || !state.is_running(&profile.id) || state.is_dev_tip_switching(&profile.id) { continue; }

            let slice = credit_now.floor().clamp(MIN_DEV_SLICE_SECONDS, MAX_DEV_SLICE_SECONDS) as u64;
            if let Err(e) = state.restart_miner_mode(&profile.id, true) {
                state.record_log(&profile.id, "system", format!("[Dev tip] Dev slice skipped: {e}"));
            } else {
                if let Ok(mut credits) = state.dev_tip_credit_seconds.lock() {
                    let credit = credits.entry(profile.id.clone()).or_insert(0.0);
                    *credit = (*credit - slice as f64).max(0.0);
                }
                if let Ok(mut until) = state.dev_tip_until.lock() { until.insert(profile.id.clone(), now.saturating_add(slice)); }
                let coin = detected_coin_id(profile).unwrap_or("supported network");
                state.record_log(&profile.id, "system", format!("[Dev tip] ♥ Mining for the MinerDesk developer for {slice}s ({pct:.2}% target, {coin})."));
            }
        }
        thread::sleep(Duration::from_secs(5));
    });
}

fn build_engine_args(p: &MinerProfile) -> Result<Vec<String>, String> {
    let algo = p.algorithm.trim();
    let pool = p.pool.trim();
    let wallet = if p.merge_secondary && !p.secondary_wallet.trim().is_empty() {
        format!("{}{}{}", p.wallet.trim(), p.merge_separator, p.secondary_wallet.trim())
    } else { p.wallet.trim().to_string() };
    let selected_gpus = gpu_tokens(&p.gpu_ids);
    let selected_gpus_csv = selected_gpus.join(",");
    let extra = || -> Result<Vec<String>, String> {
        if p.extra_args.trim().is_empty() { Ok(vec![]) } else { shlex::split(p.extra_args.trim()).ok_or_else(|| "Invalid advanced arguments".into()) }
    };

    let mut args = match p.engine.as_str() {
        "srbminer" => {
            if algo.is_empty() || pool.is_empty() || wallet.is_empty() { return Err("Algorithm, pool and wallet are required".into()); }
            let mut a = vec!["--algorithm".into(), algo.into(), "--pool".into(), pool.into(), "--wallet".into(), wallet];
            if !p.password.trim().is_empty() { a.extend(["--password".into(), p.password.trim().into()]); }
            if p.disable_cpu { a.push("--disable-cpu".into()); }
            if !p.worker.trim().is_empty() { a.extend(["--worker".into(), p.worker.trim().into()]); }
            if !selected_gpus.is_empty() { a.extend(["--gpu-id".into(), selected_gpus_csv.clone()]); }
            // SRBMiner requires per-GPU OC values to match the --gpu-id order.
            if let Some(v) = srb_numeric_list(p, &selected_gpus, effective_core_clock) { a.extend(["--gpu-cclock0".into(), v]); }
            else if selected_gpus.is_empty() { if let Some(v) = p.core_clock { a.extend(["--gpu-cclock0".into(), v.to_string()]); } }
            if let Some(v) = srb_numeric_list(p, &selected_gpus, effective_power_limit) { a.extend(["--gpu-plimit0".into(), v]); }
            else if selected_gpus.is_empty() { if let Some(v) = p.power_limit { a.extend(["--gpu-plimit0".into(), v.to_string()]); } }
            if let Some(v) = srb_fan_list(p, &selected_gpus) { a.extend(["--gpu-fan0".into(), v]); }
            else if selected_gpus.is_empty() { if let Some(v) = p.fan { a.extend(["--gpu-fan0".into(), v.to_string()]); } }
            if let Some(port) = p.api_port { a.extend(["--api-enable".into(), "--api-port".into(), port.to_string(), "--api-rig-name".into(), p.name.clone()]); }
            a
        }
        "lolminer" => {
            if algo.is_empty() || pool.is_empty() || wallet.is_empty() { return Err("Algorithm, pool and wallet are required".into()); }
            let mut a = vec!["--algo".into(), algo.into(), "--pool".into(), pool.into(), "--user".into(), wallet];
            if !p.password.trim().is_empty() { a.extend(["--pass".into(), p.password.trim().into()]); }
            if !p.worker.trim().is_empty() { a.extend(["--worker".into(), p.worker.trim().into()]); }
            if !selected_gpus.is_empty() { a.extend(["--devices".into(), selected_gpus_csv.clone()]); }
            if !selected_gpus.is_empty() {
                if let Some(v) = indexed_list_u32(p, &selected_gpus, "*", effective_core_clock) { a.extend(["--cclk".into(), v]); }
                if let Some(v) = indexed_list_u32(p, &selected_gpus, "*", effective_power_limit) { a.extend(["--pl".into(), v]); }
                if let Some(v) = indexed_list_fan(p, &selected_gpus, "*") { a.extend(["--fan".into(), v]); }
            } else {
                if let Some(v) = p.core_clock { a.extend(["--cclk".into(), v.to_string()]); }
                if let Some(v) = p.power_limit { a.extend(["--pl".into(), v.to_string()]); }
                if let Some(v) = p.fan { a.extend(["--fan".into(), v.to_string()]); }
            }
            a
        }
        "bzminer" => {
            if algo.is_empty() || pool.is_empty() || wallet.is_empty() { return Err("Algorithm, pool and wallet are required".into()); }
            let mut pool_value = pool.to_string();
            if !pool_value.contains("://") { pool_value = format!("stratum+tcp://{pool_value}"); }
            let mut a = vec!["-a".into(), algo.into(), "-p".into(), pool_value, "-w".into(), wallet];
            if !p.password.trim().is_empty() { a.extend(["--pool_password".into(), p.password.trim().into()]); }
            if !p.worker.trim().is_empty() { a.extend(["-r".into(), p.worker.trim().into()]); }
            if p.disable_cpu { a.extend(["--cpu".into(), "0".into()]); }
            if !selected_gpus.is_empty() { a.push("--enable".into()); a.extend(selected_gpus.iter().cloned()); }
            if !selected_gpus.is_empty() {
                let cores: Vec<String> = selected_gpus.iter().map(|id| effective_core_clock(p,id).unwrap_or(0).to_string()).collect();
                let powers: Vec<String> = selected_gpus.iter().map(|id| effective_power_limit(p,id).unwrap_or(0).to_string()).collect();
                let fans: Vec<String> = selected_gpus.iter().map(|id| effective_fan(p,id).map(|v| v.to_string()).unwrap_or_else(|| "-1".into())).collect();
                if cores.iter().any(|v| v != "0") { a.push("--oc_lock_core_clock".into()); a.extend(cores); }
                if powers.iter().any(|v| v != "0") { a.push("--oc_power_limit".into()); a.extend(powers); }
                if fans.iter().any(|v| v != "-1") { a.push("--oc_fan_speed".into()); a.extend(fans); }
            } else {
                if let Some(v) = p.core_clock { a.extend(["--oc_lock_core_clock".into(), v.to_string()]); }
                if let Some(v) = p.power_limit { a.extend(["--oc_power_limit".into(), v.to_string()]); }
                if let Some(v) = p.fan { a.extend(["--oc_fan_speed".into(), v.to_string()]); }
            }
            if let Some(port) = p.api_port { a.extend(["--http_enabled".into(), "1".into(), "--http_port".into(), port.to_string()]); }
            a
        }
        "rigel" => {
            if algo.is_empty() || pool.is_empty() || wallet.is_empty() { return Err("Algorithm, pool and wallet are required".into()); }
            let mut pool_value = pool.to_string();
            if !pool_value.contains("://") { pool_value = format!("stratum+tcp://{pool_value}"); }
            let mut a = vec!["-a".into(), algo.into(), "-o".into(), pool_value, "-u".into(), wallet];
            if !p.password.trim().is_empty() { a.extend(["-p".into(), p.password.trim().into()]); }
            if !p.worker.trim().is_empty() { a.extend(["-w".into(), p.worker.trim().into()]); }
            if !selected_gpus.is_empty() { a.extend(["-d".into(), selected_gpus_csv.clone()]); }
            if !selected_gpus.is_empty() {
                if let Some(v) = indexed_list_u32(p, &selected_gpus, "_", effective_core_clock) { a.extend(["--lock-cclock".into(), v]); }
                if let Some(v) = indexed_list_u32(p, &selected_gpus, "_", effective_power_limit) { a.extend(["--pl".into(), v]); }
                if let Some(v) = indexed_list_fan(p, &selected_gpus, "_") { a.extend(["--fan-control".into(), v]); }
            } else {
                if let Some(v) = p.core_clock { a.extend(["--lock-cclock".into(), v.to_string()]); }
                if let Some(v) = p.power_limit { a.extend(["--pl".into(), v.to_string()]); }
                if let Some(v) = p.fan { a.extend(["--fan-control".into(), v.to_string()]); }
            }
            a
        }
        "lpminer" => {
            if pool.is_empty() || wallet.is_empty() { return Err("Pool and wallet are required".into()); }
            let mut pool_value = pool.to_string();
            if !pool_value.contains("://") { pool_value = format!("stratum+tcp://{pool_value}"); }
            let wallet_value = if p.worker.trim().is_empty() {
                wallet
            } else {
                format!("{}.{}", wallet, p.worker.trim())
            };
            let mut a = Vec::new();
            // lpminer documents Pearl as `--algo pearl`. Accept MinerDesk's
            // generic `pearlhash` profile name and translate it to the miner's
            // canonical value.
            let algo_value = if algo.eq_ignore_ascii_case("pearlhash") { "pearl" } else { algo };
            if !algo_value.is_empty() { a.extend(["--algo".into(), algo_value.into()]); }
            a.extend(["--pool".into(), pool_value, "--wallet".into(), wallet_value]);
            // lpminer 0.1.x supports an absolute core lock through
            // --lock-core-clock. Always rebuild this argument from the latest saved
            // profile so Stop -> edit -> Start cannot reuse a stale legacy value.
            if let Some(v) = lpminer_shared_core_clock(p, &selected_gpus)? {
                a.extend(["--lock-core-clock".into(), v.to_string()]);
            }
            a
        }
        "npminer" => {
            if algo.is_empty() || pool.is_empty() || wallet.is_empty() { return Err("Algorithm, pool and wallet are required".into()); }
            let algo_value = if algo.eq_ignore_ascii_case("pearlhash") { "pearl".to_string() } else { algo.to_string() };
            let mut pool_value = pool.to_string();
            if !pool_value.contains("://") { pool_value = format!("stratum+tcp://{pool_value}"); }
            let mut a = vec!["-a".into(), algo_value, "-o".into(), pool_value, "-u".into(), wallet];
            if !p.password.trim().is_empty() { a.extend(["-p".into(), p.password.trim().into()]); }
            if !p.worker.trim().is_empty() { a.extend(["-w".into(), p.worker.trim().into()]); }
            if !selected_gpus.is_empty() { a.extend(["--devices".into(), selected_gpus_csv.clone()]); }
            // NPMiner exposes CUDA core-clock and power-limit lists. We only emit
            // them when every explicitly selected GPU has a value, avoiding an
            // ambiguous partial list. Fan control is not part of NPMiner's CLI.
            if !selected_gpus.is_empty() {
                if let Some(v) = srb_numeric_list(p, &selected_gpus, effective_core_clock) { a.extend(["--cuda-lock-core-clocks".into(), v]); }
                if let Some(v) = srb_numeric_list(p, &selected_gpus, effective_power_limit) { a.extend(["--cuda-power-limits".into(), v]); }
            } else {
                if let Some(v) = p.core_clock { a.extend(["--cuda-lock-core-clocks".into(), v.to_string()]); }
                if let Some(v) = p.power_limit { a.extend(["--cuda-power-limits".into(), v.to_string()]); }
            }
            if let Some(port) = p.api_port { a.extend(["--api-port".into(), port.to_string()]); }
            a
        }
        "custom" => vec![],
        x => return Err(format!("Unknown miner engine: {x}")),
    };
    args.extend(extra()?);
    Ok(args)
}

#[cfg(test)]
mod regression_tests_079 {
    use super::*;

    #[test]
    fn stopped_runtime_resets_session_uptime() {
        let mut rt = MinerRuntimeView {
            id: "test".into(),
            running: true,
            pid: Some(1234),
            started_at: Some(1),
            uptime_seconds: 999,
            started_by: "manual".into(),
            last_error: String::new(),
            metrics: MinerMetrics::default(),
            dev_tip_active: true,
        };
        mark_runtime_stopped(&mut rt);
        assert!(!rt.running);
        assert_eq!(rt.pid, None);
        assert_eq!(rt.started_at, None);
        assert_eq!(rt.uptime_seconds, 0);
        assert!(!rt.dev_tip_active);
    }

    #[test]
    fn lpminer_uses_latest_per_gpu_core_clock_override() {
        let mut p = MinerProfile::default();
        p.engine = "lpminer".into();
        p.algorithm = "pearlhash".into();
        p.pool = "sg.pearl.herominers.com:1200".into();
        p.wallet = "prl-test".into();
        p.worker = "MinerDesk".into();
        p.gpu_ids = "0".into();
        p.core_clock = Some(2200);
        p.gpu_tuning = vec![GpuTuning {
            selector: "0".into(),
            core_clock: Some(2450),
            power_limit: None,
            fan: None,
        }];

        let args = build_engine_args(&p).expect("lpminer args");
        let pos = args.iter().position(|a| a == "--lock-core-clock").expect("core clock flag");
        assert_eq!(args.get(pos + 1).map(String::as_str), Some("2450"));
        assert!(!args.windows(2).any(|w| w[0] == "--lock-core-clock" && w[1] == "2200"));
    }

    #[test]
    fn lpminer_rejects_conflicting_per_gpu_core_clocks() {
        let mut p = MinerProfile::default();
        p.engine = "lpminer".into();
        p.algorithm = "pearlhash".into();
        p.pool = "sg.pearl.herominers.com:1200".into();
        p.wallet = "prl-test".into();
        p.gpu_ids = "0,1".into();
        p.gpu_tuning = vec![
            GpuTuning { selector: "0".into(), core_clock: Some(2300), power_limit: None, fan: None },
            GpuTuning { selector: "1".into(), core_clock: Some(2450), power_limit: None, fan: None },
        ];

        let err = build_engine_args(&p).expect_err("different lpminer core clocks must be explicit");
        assert!(err.contains("one shared --lock-core-clock"));
    }
}

#[derive(Debug, Deserialize)]
struct GithubRelease { tag_name: String, body: Option<String>, assets: Vec<GithubAsset> }
#[derive(Debug, Deserialize)]
struct GithubAsset { name: String, browser_download_url: String }
#[derive(Debug, Serialize)]
pub struct InstallResult { engine: String, version: String, executable_path: String, asset_name: String, checksum_verified: bool }

fn managed_miners_root() -> Result<PathBuf, String> {
    #[cfg(target_os = "windows")]
    {
        let program_data = std::env::var_os("PROGRAMDATA").ok_or_else(|| "PROGRAMDATA introuvable".to_string())?;
        return Ok(PathBuf::from(program_data).join("MinerDesk").join("miners"));
    }
    #[cfg(not(target_os = "windows"))]
    {
        let base = dirs_next::data_local_dir().ok_or_else(|| "Dossier de données introuvable".to_string())?;
        Ok(base.join("MinerDesk").join("miners"))
    }
}

fn engine_executable_names(engine: &str) -> Vec<&'static str> {
    let win = cfg!(target_os = "windows");
    match engine {
        "srbminer" => if win { vec!["SRBMiner-MULTI.exe"] } else { vec!["SRBMiner-MULTI"] },
        "lolminer" => if win { vec!["lolMiner.exe"] } else { vec!["lolMiner"] },
        "bzminer" => if win { vec!["bzminer.exe"] } else { vec!["bzminer"] },
        "rigel" => if win { vec!["rigel.exe"] } else { vec!["rigel"] },
        "lpminer" => if win { vec!["lpminer.exe", "pearl-miner.exe"] } else { vec!["lpminer", "pearl-miner"] },
        "npminer" => if win { vec!["npminer.exe"] } else { vec!["npminer"] },
        _ => vec![],
    }
}

fn asset_matches(engine: &str, name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    let is_zip = n.ends_with(".zip");
    let is_tar = n.ends_with(".tar.gz") || n.ends_with(".tgz");
    if cfg!(target_os = "windows") {
        match engine {
            "srbminer" => is_zip && n.contains("win64"),
            "lolminer" => is_zip && n.contains("win64") && !n.contains("cln"),
            "bzminer" => is_zip && (n.contains("windows") || n.contains("win64")),
            "rigel" => is_zip && (n.contains("win") || n.contains("windows")),
            "lpminer" => is_zip && n.contains("lpminer"),
            // NPMiner publishes its Windows archive as .tar.gz.
            "npminer" => is_tar && n.contains("npminer-windows-x86_64") && !n.contains("hive") && !n.contains("mmpos"),
            _ => false,
        }
    } else {
        if !is_tar { return false; }
        match engine {
            "srbminer" => n.contains("linux"),
            "lolminer" => n.contains("lin64") || n.contains("linux"),
            "bzminer" => n.contains("linux"),
            "rigel" => n.contains("linux"),
            "lpminer" => n.contains("lpminer"),
            "npminer" => (n.contains("npminer-linux-x86_64") || n.contains("npminer-linux-ubuntu20-x86_64")) && !n.contains("hive") && !n.contains("mmpos"),
            _ => false,
        }
    }
}

fn find_engine_executable(root: &Path, engine: &str) -> Option<PathBuf> {
    let names: Vec<String> = engine_executable_names(engine).into_iter().map(|x| x.to_ascii_lowercase()).collect();
    WalkDir::new(root).into_iter().filter_map(Result::ok).find(|e| {
        e.file_type().is_file() && names.contains(&e.file_name().to_string_lossy().to_ascii_lowercase())
    }).map(|e| e.path().to_path_buf())
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let mut p = fs::metadata(path).map_err(|e| e.to_string())?.permissions(); p.set_mode(0o755); fs::set_permissions(path, p).map_err(|e| e.to_string())
}
#[cfg(not(unix))]
fn make_executable(_: &Path) -> Result<(), String> { Ok(()) }

fn copy_dir_contents(src: &Path, dst: &Path) -> Result<(), String> {
    fs::create_dir_all(dst).map_err(|e| e.to_string())?;
    for entry in WalkDir::new(src).into_iter().filter_map(Result::ok) {
        let rel = entry.path().strip_prefix(src).map_err(|e| e.to_string())?;
        if rel.as_os_str().is_empty() { continue; }
        let out = dst.join(rel);
        if entry.file_type().is_dir() { fs::create_dir_all(&out).map_err(|e| e.to_string())?; }
        else if entry.file_type().is_file() { if let Some(p) = out.parent() { fs::create_dir_all(p).map_err(|e| e.to_string())?; } fs::copy(entry.path(), &out).map_err(|e| e.to_string())?; }
    }
    Ok(())
}

fn expected_md5(body: &str, asset_name: &str) -> Option<String> {
    let pattern = format!(r"(?im)^([a-f0-9]{{32}})\s+\*?{}\s*$", regex::escape(asset_name));
    Regex::new(&pattern).ok()?.captures(body)?.get(1).map(|m| m.as_str().to_lowercase())
}

async fn download_engine(engine: &str) -> Result<InstallResult, String> {
    let spec = engine_specs().into_iter().find(|s| s.id == engine).ok_or_else(|| "Moteur inconnu".to_string())?;
    let repo = spec.github_repo.ok_or_else(|| "Téléchargement automatique indisponible pour ce moteur".to_string())?;
    let client = reqwest::Client::builder().user_agent("MinerDesk/0.7.22").build().map_err(|e| e.to_string())?;
    let release: GithubRelease = client.get(format!("https://api.github.com/repos/{repo}/releases/latest"))
        .send().await.map_err(|e| format!("GitHub: {e}"))?.error_for_status().map_err(|e| format!("GitHub: {e}"))?
        .json().await.map_err(|e| format!("Release GitHub invalide: {e}"))?;
    let asset = release.assets.iter().find(|a| asset_matches(engine, &a.name)).ok_or_else(|| format!("Aucun asset compatible trouvé dans la dernière release de {}", spec.name))?;
    let bytes = client.get(&asset.browser_download_url).send().await.map_err(|e| format!("Téléchargement: {e}"))?
        .error_for_status().map_err(|e| format!("Téléchargement: {e}"))?.bytes().await.map_err(|e| format!("Téléchargement: {e}"))?;
    let mut checksum_verified = false;
    if engine == "srbminer" {
        if let Some(expected) = release.body.as_deref().and_then(|b| expected_md5(b, &asset.name)) {
            let actual = format!("{:x}", md5::compute(bytes.as_ref()));
            if actual != expected { return Err(format!("MD5 invalide: attendu {expected}, obtenu {actual}")); }
            checksum_verified = true;
        }
    }
    let root = managed_miners_root()?.join(engine);
    let staging = root.join("_staging");
    let current = root.join("current");
    if staging.exists() { let _ = fs::remove_dir_all(&staging); }
    fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
    if asset.name.to_ascii_lowercase().ends_with(".zip") {
        let mut zip = ZipArchive::new(Cursor::new(bytes.to_vec())).map_err(|e| format!("ZIP: {e}"))?;
        zip.extract(&staging).map_err(|e| format!("Extraction ZIP: {e}"))?;
    } else {
        let decoder = GzDecoder::new(Cursor::new(bytes.to_vec()));
        let mut archive = tar::Archive::new(decoder);
        archive.unpack(&staging).map_err(|e| format!("Extraction: {e}"))?;
    }
    let staged_exe = find_engine_executable(&staging, engine).ok_or_else(|| "Exécutable introuvable après extraction".to_string())?;
    let payload = staged_exe.parent().ok_or_else(|| "Dossier payload invalide".to_string())?;
    let exe_name = staged_exe.file_name().ok_or_else(|| "Nom exécutable invalide".to_string())?.to_os_string();
    if current.exists() { fs::remove_dir_all(&current).map_err(|e| e.to_string())?; }
    copy_dir_contents(payload, &current)?;
    let _ = fs::remove_dir_all(&staging);
    let executable = current.join(exe_name);
    make_executable(&executable)?;
    #[cfg(target_os = "windows")]
    refresh_windows_firewall_for_executable(engine, &executable);
    Ok(InstallResult { engine: engine.into(), version: release.tag_name, executable_path: executable.to_string_lossy().to_string(), asset_name: asset.name.clone(), checksum_verified })
}

// ---------- Scheduling ----------

fn parse_time(s: &str) -> Option<NaiveTime> { NaiveTime::parse_from_str(s, "%H:%M").ok() }
fn schedule_moment(now: chrono::DateTime<Local>) -> LocalMoment {
    LocalMoment { day: now.date_naive().num_days_from_ce(), weekday: now.weekday().number_from_monday(), second: now.num_seconds_from_midnight() }
}
fn schedule_window(s: &ScheduleWindow) -> schedule_control::Window<'_> {
    schedule_control::Window { id: &s.id, enabled: s.enabled, days: &s.days, start: &s.start, end: &s.end, miners: &s.miner_ids }
}
fn schedule_active(s: &ScheduleWindow, now: chrono::DateTime<Local>) -> bool {
    schedule_control::active_occurrence(schedule_window(s), schedule_moment(now)).is_some()
}
fn active_schedule_ids(schedules: &[ScheduleWindow], now: chrono::DateTime<Local>) -> HashSet<String> {
    schedules.iter().filter(|s| schedule_active(s, now)).map(|s| s.id.clone()).collect()
}
fn schedule_occurrences(schedules: &[ScheduleWindow], now: chrono::DateTime<Local>) -> ActiveOccurrences {
    schedule_control::active_miners(schedules.iter().map(schedule_window), schedule_moment(now))
}

#[cfg(target_os = "windows")]
fn iso_weekday_name(day: u32) -> &'static str {
    match day { 1 => "Monday", 2 => "Tuesday", 3 => "Wednesday", 4 => "Thursday", 5 => "Friday", 6 => "Saturday", _ => "Sunday" }
}

#[cfg(target_os = "windows")]
fn sync_windows_wake_tasks(schedules: &[ScheduleWindow]) -> Result<(), String> {
    powershell_run(
        "$ErrorActionPreference='SilentlyContinue'; Get-ScheduledTask | Where-Object {$_.TaskName -like 'MinerDesk Wake - *'} | Unregister-ScheduledTask -Confirm:$false -ErrorAction SilentlyContinue"
    )?;
    for schedule in schedules.iter().filter(|s| s.enabled && s.wake_enabled) {
        let Some(start) = parse_time(&schedule.start) else { continue; };
        let lead = schedule.wake_minutes_before.min(24 * 60) as i64;
        let start_minutes = (start.hour() * 60 + start.minute()) as i64;
        for original_day in &schedule.days {
            let mut wake_minutes = start_minutes - lead;
            let mut wake_day = *original_day as i64;
            while wake_minutes < 0 { wake_minutes += 24 * 60; wake_day -= 1; }
            while wake_day < 1 { wake_day += 7; }
            let hh = wake_minutes / 60;
            let mm = wake_minutes % 60;
            let task_name = format!("MinerDesk Wake - {} - {}", schedule.id, original_day).replace('\\', "_").replace('\'', "_");
            let day_name = iso_weekday_name(wake_day as u32);
            let script = format!(
                "$ErrorActionPreference='Stop'; \
                 $a=New-ScheduledTaskAction -Execute 'cmd.exe' -Argument '/c exit 0'; \
                 $t=New-ScheduledTaskTrigger -Weekly -WeeksInterval 1 -DaysOfWeek {day_name} -At '{hh:02}:{mm:02}'; \
                 $s=New-ScheduledTaskSettingsSet -WakeToRun -StartWhenAvailable -ExecutionTimeLimit (New-TimeSpan -Minutes 2); \
                 $p=New-ScheduledTaskPrincipal -UserId ([System.Security.Principal.WindowsIdentity]::GetCurrent().Name) -LogonType Interactive -RunLevel Highest; \
                 Register-ScheduledTask -TaskName '{task_name}' -Action $a -Trigger $t -Settings $s -Principal $p -Description 'MinerDesk wake timer for mining schedule' -Force | Out-Null"
            );
            powershell_run(&script)?;
        }
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn sync_windows_wake_tasks(_schedules: &[ScheduleWindow]) -> Result<(), String> { Ok(()) }

fn execute_power_action(action: &str) -> Result<(), String> {
    match action {
        "sleep" => {
            #[cfg(target_os = "windows")]
            {
                let script = r#"Add-Type -Namespace MinerDesk -Name Power -MemberDefinition '[DllImport("powrprof.dll", SetLastError=true)] public static extern bool SetSuspendState(bool hibernate, bool forceCritical, bool disableWakeEvent);'; if(-not [MinerDesk.Power]::SetSuspendState($false,$false,$false)){ throw 'SetSuspendState failed' }"#;
                return powershell_run(script);
            }
            #[cfg(target_os = "linux")]
            { return Command::new("systemctl").arg("suspend").status().map_err(|e| e.to_string()).and_then(|s| if s.success(){Ok(())}else{Err("systemctl suspend failed".into())}); }
            #[cfg(target_os = "macos")]
            { return Command::new("pmset").arg("sleepnow").status().map_err(|e| e.to_string()).and_then(|s| if s.success(){Ok(())}else{Err("pmset sleepnow failed".into())}); }
        }
        "hibernate" => {
            #[cfg(target_os = "windows")]
            { return background_command("shutdown.exe").arg("/h").status().map_err(|e| e.to_string()).and_then(|s| if s.success(){Ok(())}else{Err("shutdown /h failed".into())}); }
            #[cfg(target_os = "linux")]
            { return Command::new("systemctl").arg("hibernate").status().map_err(|e| e.to_string()).and_then(|s| if s.success(){Ok(())}else{Err("systemctl hibernate failed".into())}); }
            #[cfg(target_os = "macos")]
            { return Err("Hibernate is not exposed as a direct MinerDesk action on macOS".into()); }
        }
        _ => return Err(format!("Unsupported power action: {action}")),
    }
    #[allow(unreachable_code)]
    Err(format!("Unsupported power action on this platform: {action}"))
}

fn start_scheduler(state: Arc<CoreState>) {
    thread::spawn(move || {
        let mut previous_active_schedules: HashSet<String> = HashSet::new();
        loop {
            {
                let Ok(_lifecycle) = state.lifecycle.lock() else { return; };
                if state.shutting_down.load(Ordering::SeqCst) { return; }
                state.refresh_processes();
                let cfg = state.config();
                let now = Local::now();
                let active_schedule_set = active_schedule_ids(&cfg.schedules, now);
                let occurrences = schedule_occurrences(&cfg.schedules, now);
                let active: HashSet<String> = occurrences.keys().cloned().collect();
                let (control, changed) = {
                    let Ok(mut control) = state.manual_schedule.lock() else { return; };
                    let changed = control.prune(&occurrences);
                    (control.clone(), changed)
                };
                if changed {
                    if let Err(e) = state.persist_manual_schedule() { backend_diagnostic_log(&e); }
                }
                for id in &active {
                    if !control.is_paused(id, &occurrences) && !state.is_running(id) && !state.is_dev_tip_switching(id) {
                        if let Err(e) = state.start_miner_mode_locked(id, "schedule", false) { state.record_log(id, "system", format!("[Scheduler] {e}")); }
                    }
                }
                let scheduled_running: Vec<String> = state.runtime.lock().map(|r| r.values()
                    .filter(|x| scheduler_should_stop_session(x.running, &x.started_by, active.contains(&x.id)))
                    .map(|x| x.id.clone()).collect()).unwrap_or_default();
                for id in scheduled_running { let _ = state.stop_miner_locked(&id); }

                let ended: Vec<String> = previous_active_schedules.difference(&active_schedule_set).cloned().collect();
                if !ended.is_empty() && active.is_empty() {
                    state.refresh_processes();
                    let any_running = state.runtime.lock().map(|r| r.values().any(|x| x.running)).unwrap_or(false);
                    if !any_running && state.pending_power_action().is_none() {
                        if let Some(schedule) = ended.iter().filter_map(|id| cfg.schedules.iter().find(|s| &s.id == id)).find(|s| s.post_action == "hibernate" || s.post_action == "sleep") {
                            state.request_power_action(schedule);
                        }
                    }
                }
                previous_active_schedules = active_schedule_set;
            }
            state.process_pending_power_action();
            thread::sleep(Duration::from_secs(1));
        }
    });
}

// ---------- Web API ----------

#[derive(Debug, Clone, Copy)]
struct DesktopLease {
    last_heartbeat: Instant,
    warned_stale: bool,
}

#[derive(Debug, Default)]
struct DesktopLeaseState {
    ever_attached: bool,
    clients: HashMap<u32, DesktopLease>,
}

#[derive(Clone)]
struct WebState {
    core: Arc<CoreState>,
    token: Option<String>,
    headless: bool,
    desktop_owned: bool,
    desktop_leases: Arc<Mutex<DesktopLeaseState>>,
}
#[derive(Deserialize)]
struct LogsQuery { limit: Option<usize> }
#[derive(Serialize)]
struct Health { version: &'static str, headless: bool, desktop_owned: bool, miner_crash_guard: bool, config_path: String, scheduler: bool, time: String }

fn api_auth(headers: &HeaderMap, state: &WebState) -> Result<(), Response> {
    // Requests originating from the same machine are tagged by a middleware and
    // may use the API without a token. Remote/LAN clients must provide the token.
    if headers.get("x-minerdesk-local").and_then(|v| v.to_str().ok()) == Some("1") { return Ok(()); }
    let Some(expected) = state.token.as_deref() else { return Ok(()); };
    let header_token = headers.get("x-minerdesk-token").and_then(|v| v.to_str().ok());
    let bearer = headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer "));
    if header_token == Some(expected) || bearer == Some(expected) { Ok(()) }
    else { Err((StatusCode::UNAUTHORIZED, Json(json!({"error":"MinerDesk access token required"}))).into_response()) }
}

async fn mark_local_request(ConnectInfo(addr): ConnectInfo<SocketAddr>, mut req: Request<Body>, next: Next) -> Response {
    // This is a server-owned trust marker, never a client assertion. Remove every
    // incoming value before deriving local access from the actual socket peer.
    req.headers_mut().remove("x-minerdesk-local");
    if addr.ip().is_loopback() {
        req.headers_mut().insert("x-minerdesk-local", "1".parse().unwrap());
    }
    next.run(req).await
}

#[cfg(test)]
mod local_request_tests {
    use super::*;
    use axum::Extension;
    use tower::ServiceExt;

    // Exercise the actual production middleware using synthetic socket metadata.
    // No real API, configuration, miner process, or machine setting is touched.
    async fn probe(peer: &str, supplied: &[&str]) -> StatusCode {
        let app = Router::new()
            .route("/probe", get(|headers: HeaderMap| async move {
                if headers.get("x-minerdesk-local").and_then(|v| v.to_str().ok()) == Some("1") {
                    StatusCode::OK
                } else {
                    StatusCode::UNAUTHORIZED
                }
            }))
            .layer(middleware::from_fn(mark_local_request))
            .layer(Extension(ConnectInfo(peer.parse::<SocketAddr>().unwrap())));
        let mut request = Request::builder().uri("/probe").body(Body::empty()).unwrap();
        for value in supplied {
            request.headers_mut().append("x-minerdesk-local", value.parse().unwrap());
        }
        app.oneshot(request).await.unwrap().status()
    }

    #[tokio::test]
    async fn remote_client_cannot_assert_local_privileges() {
        assert_eq!(probe("192.0.2.10:50000", &["1"]).await, StatusCode::UNAUTHORIZED);
        assert_eq!(probe("[2001:db8::10]:50000", &["1"]).await, StatusCode::UNAUTHORIZED);
        assert_eq!(probe("192.0.2.10:50000", &["1", "1"]).await, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn loopback_clients_keep_existing_access() {
        assert_eq!(probe("127.0.0.1:50000", &[]).await, StatusCode::OK);
        assert_eq!(probe("[::1]:50000", &["0"]).await, StatusCode::OK);
    }

    #[tokio::test]
    async fn remote_client_without_marker_still_requires_authentication() {
        assert_eq!(probe("192.0.2.10:50000", &[]).await, StatusCode::UNAUTHORIZED);
    }
}

async fn api_health(AxumState(s): AxumState<WebState>, headers: HeaderMap) -> Response {
    if let Err(r) = api_auth(&headers, &s) { return r; }
    Json(Health {
        version: "0.7.22",
        headless: s.headless,
        desktop_owned: s.desktop_owned,
        miner_crash_guard: {
            #[cfg(target_os = "windows")]
            { windows_miner_job::is_ready() }
            #[cfg(not(target_os = "windows"))]
            { false }
        },
        config_path: s.core.config_path.to_string_lossy().to_string(),
        scheduler: true,
        time: Local::now().to_rfc3339(),
    }).into_response()
}
async fn api_engines(AxumState(s): AxumState<WebState>, headers: HeaderMap) -> Response {
    if let Err(r) = api_auth(&headers, &s) { return r; } Json(engine_specs()).into_response()
}
async fn api_get_config(AxumState(s): AxumState<WebState>, headers: HeaderMap) -> Response {
    if let Err(r) = api_auth(&headers, &s) { return r; } Json(s.core.config()).into_response()
}
async fn api_put_config(AxumState(s): AxumState<WebState>, headers: HeaderMap, Json(cfg): Json<AppConfig>) -> Response {
    if let Err(r) = api_auth(&headers, &s) { return r; }
    let core = Arc::clone(&s.core);
    match tokio::task::spawn_blocking(move || core.replace_config(cfg)).await {
        Ok(Ok(())) => Json(json!({"ok":true})).into_response(),
        Ok(Err(e)) => (StatusCode::BAD_REQUEST, Json(json!({"error":e}))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error":format!("Config worker failed: {e}")}))).into_response(),
    }
}
async fn api_status(AxumState(s): AxumState<WebState>, headers: HeaderMap) -> Response {
    if let Err(r) = api_auth(&headers, &s) { return r; } Json(s.core.statuses()).into_response()
}
async fn api_logs(AxumPath(id): AxumPath<String>, Query(q): Query<LogsQuery>, AxumState(s): AxumState<WebState>, headers: HeaderMap) -> Response {
    if let Err(r) = api_auth(&headers, &s) { return r; } Json(s.core.logs_for(&id, q.limit.unwrap_or(300))).into_response()
}
async fn api_gpus(AxumPath(id): AxumPath<String>, AxumState(s): AxumState<WebState>, headers: HeaderMap) -> Response {
    if let Err(r) = api_auth(&headers, &s) { return r; }
    let cfg = s.core.config();
    let Some(profile) = cfg.miners.iter().find(|m| m.id == id) else {
        return (StatusCode::NOT_FOUND, Json(json!({"error":"Profil mineur introuvable"}))).into_response();
    };
    Json(discover_gpus(profile)).into_response()
}
async fn api_discover_gpus(AxumState(s): AxumState<WebState>, headers: HeaderMap, Json(profile): Json<MinerProfile>) -> Response {
    if let Err(r) = api_auth(&headers, &s) { return r; }
    Json(discover_gpus(&profile)).into_response()
}
async fn miner_command_response(core: Arc<CoreState>, id: String, action: &'static str) -> Response {
    match tokio::task::spawn_blocking(move || {
        let began = Instant::now();
        backend_diagnostic_log(&format!("miner command {action} requested for profile {id}"));
        let result = match action {
            "start" => core.start_miner(&id, "manual"),
            "stop" => core.stop_miner_manually(&id),
            "restart" => core.restart_miner_manually(&id),
            _ => Err(format!("Unknown mining command: {action}")),
        };
        if let Err(ref e) = result { core.record_command_error(&id, action, e); }
        backend_diagnostic_log(&format!("miner command {action} finished for profile {id}; ok={}; elapsed_ms={}", result.is_ok(), began.elapsed().as_millis()));
        result
    }).await {
        Ok(Ok(())) => Json(json!({"ok":true})).into_response(),
        Ok(Err(e)) => (StatusCode::BAD_REQUEST, Json(json!({"error":e}))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error":format!("Mining command worker failed: {e}")}))).into_response(),
    }
}

async fn api_start(AxumPath(id): AxumPath<String>, AxumState(s): AxumState<WebState>, headers: HeaderMap) -> Response {
    if let Err(r) = api_auth(&headers, &s) { return r; }
    miner_command_response(s.core, id, "start").await
}
async fn api_stop(AxumPath(id): AxumPath<String>, AxumState(s): AxumState<WebState>, headers: HeaderMap) -> Response {
    if let Err(r) = api_auth(&headers, &s) { return r; }
    miner_command_response(s.core, id, "stop").await
}
async fn api_restart(AxumPath(id): AxumPath<String>, AxumState(s): AxumState<WebState>, headers: HeaderMap) -> Response {
    if let Err(r) = api_auth(&headers, &s) { return r; }
    miner_command_response(s.core, id, "restart").await
}
async fn api_start_all(AxumState(s): AxumState<WebState>, headers: HeaderMap) -> Response {
    if let Err(r) = api_auth(&headers, &s) { return r; }
    match tokio::task::spawn_blocking(move || {
        let cfg = s.core.config();
        let mut results = vec![];
        for m in cfg.miners.iter().filter(|m| m.enabled) {
            let error = s.core.start_miner(&m.id, "manual").err();
            if let Some(ref e) = error { s.core.record_command_error(&m.id, "start-all", e); }
            results.push(json!({"id":m.id,"result":error}));
        }
        // Keep the established API contract; the UI now checks each result.
        Value::Array(results)
    }).await {
        Ok(results) => Json(results).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error":format!("Start all worker failed: {e}")}))).into_response(),
    }
}
async fn api_stop_all(AxumState(s): AxumState<WebState>, headers: HeaderMap) -> Response {
    if let Err(r) = api_auth(&headers, &s) { return r; }
    match tokio::task::spawn_blocking(move || s.core.stop_all_manually()).await {
        Ok(Ok(())) => Json(json!({"ok":true})).into_response(),
        Ok(Err(e)) => (StatusCode::BAD_REQUEST, Json(json!({"error":e}))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error":format!("Stop all worker failed: {e}")}))).into_response(),
    }
}
async fn api_download_engine(AxumPath(engine): AxumPath<String>, AxumState(s): AxumState<WebState>, headers: HeaderMap) -> Response {
    if let Err(r) = api_auth(&headers, &s) { return r; }
    match download_engine(&engine).await {
        Ok(result) => Json(result).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, Json(json!({"error":e}))).into_response(),
    }
}

async fn api_power_peek(AxumState(s): AxumState<WebState>, headers: HeaderMap) -> Response {
    if let Err(r) = api_auth(&headers, &s) { return r; }
    // Native Desktop uses this endpoint only to discover that a confirmation
    // exists and bring its window forward. It deliberately does NOT mark the
    // confirmation UI as seen; only the rendered UI may renew that lease.
    Json(s.core.pending_power_action()).into_response()
}
async fn api_power_pending(AxumState(s): AxumState<WebState>, headers: HeaderMap) -> Response {
    if let Err(r) = api_auth(&headers, &s) { return r; }
    s.core.touch_power_ui();
    Json(s.core.pending_power_action()).into_response()
}
async fn api_power_cancel(AxumState(s): AxumState<WebState>, headers: HeaderMap) -> Response {
    if let Err(r) = api_auth(&headers, &s) { return r; }
    s.core.cancel_power_action();
    Json(json!({"ok":true})).into_response()
}
async fn api_power_confirm(AxumState(s): AxumState<WebState>, headers: HeaderMap) -> Response {
    if let Err(r) = api_auth(&headers, &s) { return r; }
    match s.core.confirm_power_action() {
        Ok(_) => Json(json!({"ok":true})).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, Json(json!({"error":e}))).into_response(),
    }
}
#[derive(Serialize)]
struct PowerDiagnostics { supported: bool, wake_timers: String }
#[derive(Deserialize)]
struct ToggleSecurity { enabled: bool }

async fn api_security_status(AxumState(s): AxumState<WebState>, headers: HeaderMap) -> Response {
    if let Err(r) = api_auth(&headers, &s) { return r; }
    match security_status_impl() {
        Ok(v) => Json(v).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, Json(json!({"error":e}))).into_response(),
    }
}
async fn api_security_firewall(AxumState(s): AxumState<WebState>, headers: HeaderMap, Json(req): Json<ToggleSecurity>) -> Response {
    if let Err(r) = api_auth(&headers, &s) { return r; }
    let web = s.core.config().web;
    match set_firewall_exception_impl(req.enabled, Some(&web)) {
        Ok(v) => Json(v).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, Json(json!({"error":e}))).into_response(),
    }
}
async fn api_security_defender(AxumState(s): AxumState<WebState>, headers: HeaderMap, Json(req): Json<ToggleSecurity>) -> Response {
    if let Err(r) = api_auth(&headers, &s) { return r; }
    match set_defender_exception_impl(req.enabled) {
        Ok(v) => Json(v).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, Json(json!({"error":e}))).into_response(),
    }
}

async fn api_power_diagnostics(AxumState(s): AxumState<WebState>, headers: HeaderMap) -> Response {
    if let Err(r) = api_auth(&headers, &s) { return r; }
    #[cfg(target_os = "windows")]
    {
        let out = background_command("powercfg.exe").arg("/waketimers").output();
        let text = out.ok().map(|o| String::from_utf8_lossy(&o.stdout).to_string()).unwrap_or_default();
        return Json(PowerDiagnostics { supported: true, wake_timers: text }).into_response();
    }
    #[cfg(not(target_os = "windows"))]
    { Json(PowerDiagnostics { supported: false, wake_timers: String::new() }).into_response() }
}

async fn api_backend_shutdown(AxumState(s): AxumState<WebState>, headers: HeaderMap) -> Response {
    if let Err(r) = api_auth(&headers, &s) { return r; }
    if !s.headless {
        return Json(json!({"ok":true,"ignored":true,"reason":"desktop-owned backend"})).into_response();
    }
    if !s.desktop_owned {
        // A CLI-started standalone headless instance is intentionally independent
        // from MinerDesk Desktop. Closing the GUI must never terminate it.
        return Json(json!({"ok":true,"ignored":true,"reason":"standalone headless backend"})).into_response();
    }

    // 0.7.5 identifies Desktop instances by PID. This avoids one Desktop instance
    // tearing down a managed backend while another Desktop window is still alive.
    if let Some(pid) = desktop_pid_from_headers(&headers) {
        let other_clients = if let Ok(mut leases) = s.desktop_leases.lock() {
            leases.clients.remove(&pid);
            !leases.clients.is_empty()
        } else {
            false
        };
        if other_clients {
            backend_diagnostic_log(&format!(
                "Desktop PID {pid} requested backend shutdown, but another Desktop instance is still attached"
            ));
            return Json(json!({"ok":true,"ignored":true,"reason":"other desktop instances remain"})).into_response();
        }
    }

    backend_diagnostic_log("backend shutdown requested by MinerDesk Desktop");
    s.core.cancel_power_action();
    s.core.stop_all();
    thread::spawn(|| {
        thread::sleep(Duration::from_millis(250));
        std::process::exit(0);
    });
    Json(json!({"ok":true})).into_response()
}

fn desktop_pid_from_headers(headers: &HeaderMap) -> Option<u32> {
    headers
        .get("x-minerdesk-desktop-pid")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|pid| *pid > 0)
}

async fn api_backend_desktop_heartbeat(AxumState(s): AxumState<WebState>, headers: HeaderMap) -> Response {
    // The endpoint is loopback-only. In 0.7.5 it is additionally enabled only for
    // a backend explicitly started with --desktop-owned. Plain CLI/headless mode
    // therefore stays completely independent from the Desktop lifecycle.
    if headers.get("x-minerdesk-local").and_then(|v| v.to_str().ok()) != Some("1") {
        return (StatusCode::FORBIDDEN, Json(json!({"error":"Desktop heartbeat is local-only"}))).into_response();
    }
    if !s.headless {
        return Json(json!({"ok":true,"ignored":true,"reason":"desktop process hosts backend"})).into_response();
    }
    if !s.desktop_owned {
        return Json(json!({"ok":true,"ignored":true,"reason":"standalone headless backend"})).into_response();
    }
    let Some(pid) = desktop_pid_from_headers(&headers) else {
        return (StatusCode::BAD_REQUEST, Json(json!({"error":"Desktop PID is required"}))).into_response();
    };

    if let Ok(mut leases) = s.desktop_leases.lock() {
        let is_new = !leases.clients.contains_key(&pid);
        leases.ever_attached = true;
        leases.clients.insert(pid, DesktopLease { last_heartbeat: Instant::now(), warned_stale: false });
        if is_new {
            backend_diagnostic_log(&format!("Desktop heartbeat attached to minerdesk-headless (PID {pid})"));
        }
    }
    Json(json!({"ok":true,"pid":pid})).into_response()
}

#[cfg(target_os = "windows")]
fn desktop_process_is_alive(pid: u32) -> Option<bool> {
    // This is only queried after heartbeats have already been missing for the
    // grace period, so it does not add continuous PowerShell overhead. Returning
    // None on a probe failure is deliberately fail-safe: an uncertain liveness
    // check must never stop a miner while the Desktop could still be running.
    let script = format!(
        "$p=Get-Process -Id {pid} -ErrorAction SilentlyContinue; if($p){{Write-Output 'ALIVE'}}else{{Write-Output 'DEAD'}}"
    );
    let output = powershell_command()
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", script.as_str()])
        .output()
        .ok()?;
    if !output.status.success() { return None; }
    match String::from_utf8_lossy(&output.stdout).trim() {
        "ALIVE" => Some(true),
        "DEAD" => Some(false),
        _ => None,
    }
}

#[cfg(not(target_os = "windows"))]
fn desktop_process_is_alive(pid: u32) -> Option<bool> {
    Some(Path::new(&format!("/proc/{pid}")).exists())
}

fn start_desktop_heartbeat_watchdog(core: Arc<CoreState>, leases: Arc<Mutex<DesktopLeaseState>>) {
    const HEARTBEAT_GRACE: Duration = Duration::from_secs(15);
    const WATCH_INTERVAL: Duration = Duration::from_secs(3);

    thread::spawn(move || loop {
        thread::sleep(WATCH_INTERVAL);
        let now = Instant::now();
        let stale_pids: Vec<u32> = match leases.lock() {
            Ok(state) => {
                if !state.ever_attached {
                    // The scheduled Desktop-owned backend may start at logon before
                    // MinerDesk Desktop. It must stay alive until a Desktop attaches.
                    continue;
                }
                state.clients.iter()
                    .filter_map(|(pid, lease)| (now.duration_since(lease.last_heartbeat) >= HEARTBEAT_GRACE).then_some(*pid))
                    .collect()
            }
            Err(_) => continue,
        };

        for pid in stale_pids {
            let process_alive = desktop_process_is_alive(pid);
            let mut state = match leases.lock() { Ok(state) => state, Err(_) => continue };
            let still_stale = state.clients.get(&pid)
                .map(|lease| Instant::now().duration_since(lease.last_heartbeat) >= HEARTBEAT_GRACE)
                .unwrap_or(false);
            if !still_stale {
                // A heartbeat arrived while process liveness was being checked.
                continue;
            }

            match process_alive {
                Some(true) => {
                    if let Some(lease) = state.clients.get_mut(&pid) {
                        // The old 0.7.4 watchdog treated a heartbeat transport gap as
                        // if the Desktop had exited. 0.7.5 never stops mining while
                        // the owning Desktop PID is still alive.
                        if !lease.warned_stale {
                            backend_diagnostic_log(&format!(
                                "Desktop heartbeat delayed for PID {pid}, but the Desktop process is still alive; keeping backend running"
                            ));
                            lease.warned_stale = true;
                        }
                        // Avoid a process probe every three seconds while a temporary
                        // heartbeat transport problem persists.
                        lease.last_heartbeat = Instant::now();
                    }
                }
                Some(false) => {
                    state.clients.remove(&pid);
                    backend_diagnostic_log(&format!(
                        "Desktop PID {pid} is no longer running; releasing its backend lease"
                    ));
                }
                None => {
                    if let Some(lease) = state.clients.get_mut(&pid) {
                        if !lease.warned_stale {
                            backend_diagnostic_log(&format!(
                                "Desktop heartbeat delayed for PID {pid}; process liveness could not be verified, keeping backend running"
                            ));
                            lease.warned_stale = true;
                        }
                        lease.last_heartbeat = Instant::now();
                    }
                }
            }
        }

        let should_stop = leases.lock()
            .map(|state| state.ever_attached && state.clients.is_empty())
            .unwrap_or(false);
        if should_stop {
            backend_diagnostic_log(
                "No MinerDesk Desktop process remains; stopping miners and desktop-owned minerdesk-headless",
            );
            core.cancel_power_action();
            core.stop_all();
            thread::sleep(Duration::from_millis(250));
            std::process::exit(0);
        }
    });
}

async fn static_handler(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let key = if path.is_empty() { "index.html" } else { path };
    let file = FRONTEND.get_file(key).or_else(|| FRONTEND.get_file("index.html"));
    match file {
        Some(f) => {
            let mime = mime_guess::from_path(key).first_or_octet_stream();
            Response::builder().status(StatusCode::OK).header(header::CONTENT_TYPE, mime.as_ref()).body(Body::from(f.contents().to_vec())).unwrap()
        }
        None => (StatusCode::NOT_FOUND, "MinerDesk UI introuvable").into_response(),
    }
}

async fn serve_web(core: Arc<CoreState>, listen: String, port: u16, token: Option<String>, headless: bool, desktop_owned: bool) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    core.start_windows_config_sync();
    let desktop_leases = Arc::new(Mutex::new(DesktopLeaseState::default()));
    if headless && desktop_owned {
        start_desktop_heartbeat_watchdog(Arc::clone(&core), Arc::clone(&desktop_leases));
    }
    let state = WebState { core, token, headless, desktop_owned, desktop_leases };
    let app = Router::new()
        .route("/api/health", get(api_health))
        .route("/api/engines", get(api_engines))
        .route("/api/config", get(api_get_config).put(api_put_config))
        .route("/api/status", get(api_status))
        .route("/api/miners/start-all", post(api_start_all))
        .route("/api/miners/stop-all", post(api_stop_all))
        .route("/api/miners/:id/start", post(api_start))
        .route("/api/miners/:id/stop", post(api_stop))
        .route("/api/miners/:id/restart", post(api_restart))
        .route("/api/miners/:id/logs", get(api_logs))
        .route("/api/miners/:id/gpus", get(api_gpus))
        .route("/api/gpus/discover", post(api_discover_gpus))
        .route("/api/engines/:engine/download", post(api_download_engine))
        .route("/api/power/peek", get(api_power_peek))
        .route("/api/power/pending", get(api_power_pending))
        .route("/api/power/confirm", post(api_power_confirm))
        .route("/api/power/cancel", post(api_power_cancel))
        .route("/api/power/diagnostics", get(api_power_diagnostics))
        .route("/api/security/status", get(api_security_status))
        .route("/api/security/firewall", post(api_security_firewall))
        .route("/api/security/defender", post(api_security_defender))
        .route("/api/backend/shutdown", post(api_backend_shutdown))
        .route("/api/backend/desktop-heartbeat", post(api_backend_desktop_heartbeat))
        .fallback(static_handler)
        .layer(middleware::from_fn(mark_local_request))
        .layer(CorsLayer::permissive())
        .with_state(state);
    let addr = format!("{listen}:{port}");
    let listener = tokio::net::TcpListener::bind(&addr).await.map_err(|e| format!("Web server {addr}: {e}"))?;
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await.map_err(|e| e.to_string())
}

#[cfg(not(target_os = "windows"))]
fn spawn_desktop_web(core: Arc<CoreState>, listen: String, port: u16, token: Option<String>) {
    thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("MinerDesk web runtime");
        let _ = rt.block_on(serve_web(core, listen, port, token, false, false));
    });
}

// ---------- Desktop Windows integration ----------

#[cfg(target_os = "windows")]
fn sync_windows_startup(enabled: bool) -> Result<(), String> {
    let key = "HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Run";
    if enabled {
        let exe = std::env::current_exe().map_err(|e| format!("Unable to resolve MinerDesk executable: {e}"))?;
        let exe = exe.to_string_lossy().replace('\'', "''");
        let value = format!("\"{exe}\"");
        powershell_run(&format!(
            "$ErrorActionPreference='Stop'; New-Item -Path '{key}' -Force | Out-Null; New-ItemProperty -Path '{key}' -Name 'MinerDesk' -PropertyType String -Value '{value}' -Force | Out-Null"
        ))
    } else {
        powershell_run(&format!(
            "Remove-ItemProperty -Path '{key}' -Name 'MinerDesk' -ErrorAction SilentlyContinue"
        ))
    }
}

#[cfg(not(target_os = "windows"))]
fn sync_windows_startup(_enabled: bool) -> Result<(), String> { Ok(()) }

#[tauri::command]
async fn set_start_with_windows(enabled: bool) -> Result<(), String> {
    tokio::task::spawn_blocking(move || sync_windows_startup(enabled)).await
        .map_err(|e| format!("Windows startup worker failed: {e}"))?
}

#[tauri::command]
fn set_close_to_tray(app: tauri::AppHandle, enabled: bool) -> Result<(), String> {
    CLOSE_TO_TRAY.store(enabled, Ordering::SeqCst);
    #[cfg(target_os = "windows")]
    if let Some(tray) = app.tray_by_id("minerdesk-tray") {
        tray.set_visible(enabled).map_err(|e| e.to_string())?;
    }
    #[cfg(not(target_os = "windows"))]
    let _ = app;
    Ok(())
}

/// Bring the native MinerDesk window back to the foreground. The power-action
/// modal calls this when a schedule ends so a hidden/minimized Desktop does not
/// silently wait behind other applications.
#[tauri::command]
fn focus_main_window(app: tauri::AppHandle) -> Result<(), String> {
    let window = app.get_webview_window("main").ok_or_else(|| "MinerDesk main window is unavailable".to_string())?;
    window.unminimize().map_err(|e| e.to_string())?;
    window.show().map_err(|e| e.to_string())?;
    window.set_focus().map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn show_main_window(app: &tauri::AppHandle) {
    let _ = focus_main_window(app.clone());
}

#[cfg(target_os = "windows")]
fn start_power_action_focus_watch(app: tauri::AppHandle, port: u16) {
    thread::spawn(move || {
        let mut last_pending_id = String::new();
        loop {
            match tray_backend_json(port, "/api/power/peek") {
                Some(Value::Object(map)) => {
                    if let Some(id) = map.get("id").and_then(Value::as_str) {
                        if id != last_pending_id {
                            last_pending_id = id.to_string();
                            show_main_window(&app);
                        }
                    }
                }
                Some(Value::Null) => last_pending_id.clear(),
                _ => {}
            }
            thread::sleep(Duration::from_millis(750));
        }
    });
}

#[cfg(target_os = "windows")]
fn tray_backend_json(port: u16, path: &str) -> Option<Value> {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_millis(450)).ok()?;
    let _ = stream.set_read_timeout(Some(Duration::from_millis(900)));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(450)));
    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\nAccept: application/json\r\n\r\n"
    );
    stream.write_all(request.as_bytes()).ok()?;

    let mut response = Vec::with_capacity(32 * 1024);
    let mut buf = [0u8; 8192];
    loop {
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                response.extend_from_slice(&buf[..n]);
                if response.len() > 2 * 1024 * 1024 { break; }
            }
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => break,
            Err(_) => return None,
        }
    }
    let raw = String::from_utf8_lossy(&response);
    if !raw.starts_with("HTTP/1.1 200") { return None; }
    let body = raw.split_once("\r\n\r\n")?.1;
    serde_json::from_str(body).ok()
}

#[cfg(target_os = "windows")]
fn format_tray_hashrate(hps: f64) -> String {
    let (value, unit) = if hps >= 1e15 { (hps / 1e15, "PH/s") }
        else if hps >= 1e12 { (hps / 1e12, "TH/s") }
        else if hps >= 1e9 { (hps / 1e9, "GH/s") }
        else if hps >= 1e6 { (hps / 1e6, "MH/s") }
        else if hps >= 1e3 { (hps / 1e3, "kH/s") }
        else { (hps, "H/s") };
    if value >= 100.0 { format!("{value:.0} {unit}") }
    else if value >= 10.0 { format!("{value:.1} {unit}") }
    else { format!("{value:.2} {unit}") }
}

#[cfg(target_os = "windows")]
fn format_tray_uptime(seconds: u64) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let secs = seconds % 60;
    if hours >= 24 {
        format!("{}d {:02}:{:02}", hours / 24, hours % 24, minutes)
    } else {
        format!("{:02}:{:02}:{:02}", hours, minutes, secs)
    }
}

#[cfg(target_os = "windows")]
fn shorten_tray_text(input: &str, max_chars: usize) -> String {
    if input.chars().count() <= max_chars { return input.to_string(); }
    let mut out: String = input.chars().take(max_chars.saturating_sub(1)).collect();
    out.push('…');
    out
}

#[cfg(target_os = "windows")]
fn tray_mining_lines(port: u16, french: bool) -> (String, String, String) {
    let Some(value) = tray_backend_json(port, "/api/status") else {
        return if french {
            ("● Backend indisponible".into(), "État du minage indisponible".into(), "Ouvrez MinerDesk pour le diagnostic".into())
        } else {
            ("● Backend unavailable".into(), "Mining status unavailable".into(), "Open MinerDesk for diagnostics".into())
        };
    };
    let Some(statuses) = value.as_array() else {
        return if french {
            ("● Backend connecté".into(), "État du minage indisponible".into(), "Ouvrez MinerDesk pour le diagnostic".into())
        } else {
            ("● Backend connected".into(), "Mining status unavailable".into(), "Open MinerDesk for diagnostics".into())
        };
    };

    let running: Vec<&Value> = statuses.iter()
        .filter(|s| s.pointer("/runtime/running").and_then(Value::as_bool).unwrap_or(false))
        .collect();
    if running.is_empty() {
        return if french {
            ("⛏ Minage arrêté".into(), "Aucun mineur actif".into(), "MinerDesk est prêt".into())
        } else {
            ("⛏ Mining stopped".into(), "No active miner".into(), "MinerDesk is ready".into())
        };
    }

    let total_power: f64 = running.iter()
        .filter_map(|s| s.pointer("/runtime/metrics/power_w").and_then(Value::as_f64))
        .sum();
    let max_temp = running.iter()
        .filter_map(|s| s.pointer("/runtime/metrics/temperature_c").and_then(Value::as_f64))
        .fold(None::<f64>, |acc, v| Some(acc.map_or(v, |m| m.max(v))));
    let accepted: u64 = running.iter().filter_map(|s| s.pointer("/runtime/metrics/accepted").and_then(Value::as_u64)).sum();
    let rejected: u64 = running.iter().filter_map(|s| s.pointer("/runtime/metrics/rejected").and_then(Value::as_u64)).sum();
    let max_uptime = running.iter().filter_map(|s| s.pointer("/runtime/uptime_seconds").and_then(Value::as_u64)).max().unwrap_or(0);
    let dev_tip_active = running.iter().any(|s| s.pointer("/runtime/dev_tip_active").and_then(Value::as_bool).unwrap_or(false));

    let summary = if running.len() == 1 {
        let s = running[0];
        let name = s.pointer("/profile/name").and_then(Value::as_str).unwrap_or("Miner");
        let hash = s.pointer("/runtime/metrics/hashrate_hps").and_then(Value::as_f64)
            .map(format_tray_hashrate).unwrap_or_else(|| "—".into());
        let power = s.pointer("/runtime/metrics/power_w").and_then(Value::as_f64)
            .map(|w| format!("{w:.0} W")).unwrap_or_else(|| "— W".into());
        format!("⛏ {} · {} · {}", shorten_tray_text(name, 28), hash, power)
    } else if total_power > 0.0 {
        if french { format!("⛏ {} mineurs actifs · {:.0} W", running.len(), total_power) }
        else { format!("⛏ {} active miners · {:.0} W", running.len(), total_power) }
    } else if french {
        format!("⛏ {} mineurs actifs", running.len())
    } else {
        format!("⛏ {} active miners", running.len())
    };

    let detail = if running.len() == 1 {
        let s = running[0];
        let fan = s.pointer("/runtime/metrics/fan_pct").and_then(Value::as_f64);
        match (max_temp, fan) {
            (Some(t), Some(f)) => format!("🌡 {t:.0}°C · Ventilo {f:.0}%"),
            (Some(t), None) => format!("🌡 {t:.0}°C"),
            _ => if french { "🌡 Température —".into() } else { "🌡 Temperature —".into() },
        }
    } else {
        let mut parts: Vec<String> = running.iter().take(2).map(|s| {
            let name = s.pointer("/profile/name").and_then(Value::as_str).unwrap_or("Miner");
            let hash = s.pointer("/runtime/metrics/hashrate_hps").and_then(Value::as_f64)
                .map(format_tray_hashrate).unwrap_or_else(|| "—".into());
            format!("{} {}", shorten_tray_text(name, 18), hash)
        }).collect();
        if running.len() > 2 {
            parts.push(if french { format!("+{} autre(s)", running.len() - 2) } else { format!("+{} more", running.len() - 2) });
        }
        shorten_tray_text(&parts.join(" · "), 72)
    };

    let tip_prefix = if dev_tip_active { "♥ DEV TIP · " } else { "" };
    let stats = match max_temp {
        Some(t) if running.len() > 1 => {
            format!("{tip_prefix}🌡 {t:.0}°C max · Shares {accepted}/{rejected} · {}", format_tray_uptime(max_uptime))
        }
        _ => format!("{tip_prefix}✓ Shares {accepted}/{rejected} · {}", format_tray_uptime(max_uptime)),
    };
    (shorten_tray_text(&summary, 78), shorten_tray_text(&detail, 78), shorten_tray_text(&stats, 78))
}

#[cfg(target_os = "windows")]
fn configure_windows_tray(app: &mut tauri::App, cfg: &AppConfig) -> tauri::Result<()> {
    let french = cfg.ui.language == "fr";
    let open_label = if french { "Ouvrir MinerDesk" } else { "Open MinerDesk" };
    let quit_label = if french { "Quitter MinerDesk" } else { "Quit MinerDesk" };
    let summary_item = MenuItem::with_id(app, "mining-summary", if french { "⛏ Chargement de l’état…" } else { "⛏ Loading mining status…" }, false, None::<&str>)?;
    let detail_item = MenuItem::with_id(app, "mining-detail", " ", false, None::<&str>)?;
    let stats_item = MenuItem::with_id(app, "mining-stats", " ", false, None::<&str>)?;
    let open_item = MenuItem::with_id(app, "open-minerdesk", open_label, true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit-minerdesk", quit_label, true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&summary_item, &detail_item, &stats_item, &open_item, &quit_item])?;
    let tray = TrayIconBuilder::with_id("minerdesk-tray")
        .icon(tauri::include_image!("./icons/32x32.png"))
        .tooltip("MinerDesk")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open-minerdesk" => show_main_window(app),
            "quit-minerdesk" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show_main_window(tray.app_handle());
            }
        })
        .build(app)?;
    tray.set_visible(cfg.ui.close_to_tray)?;

    let port = cfg.web.desktop_api_port;
    let summary_for_thread = summary_item.clone();
    let detail_for_thread = detail_item.clone();
    let stats_for_thread = stats_item.clone();
    thread::spawn(move || loop {
        let (summary, detail, stats) = tray_mining_lines(port, french);
        let _ = summary_for_thread.set_text(summary);
        let _ = detail_for_thread.set_text(detail);
        let _ = stats_for_thread.set_text(stats);
        thread::sleep(Duration::from_secs(4));
    });
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn configure_windows_tray(_app: &mut tauri::App, _cfg: &AppConfig) -> tauri::Result<()> { Ok(()) }

// ---------- Windows security ----------

#[derive(Debug, Serialize)]
struct SecurityStatus { supported: bool, firewall_enabled: bool, defender_exclusion_enabled: bool, miner_root: String }

#[cfg(target_os = "windows")]
fn powershell_command() -> Command {
    background_command("powershell.exe")
}
#[cfg(target_os = "windows")]
fn powershell_run(script: &str) -> Result<(), String> {
    let out = powershell_command().args(["-NoProfile","-NonInteractive","-ExecutionPolicy","Bypass","-Command",script]).output().map_err(|e| e.to_string())?;
    if out.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&out.stderr).to_string()) }
}
#[cfg(target_os = "windows")]
fn powershell_bool(script: &str) -> bool {
    powershell_command().args(["-NoProfile","-NonInteractive","-ExecutionPolicy","Bypass","-Command",script]).status().map(|s| s.success()).unwrap_or(false)
}
#[cfg(target_os = "windows")]
fn firewall_pref_enabled() -> bool {
    powershell_bool("$v=(Get-ItemProperty -Path 'HKLM:\\SOFTWARE\\MinerDesk\\Security' -Name 'FirewallRulesAdded' -ErrorAction SilentlyContinue).FirewallRulesAdded; if($v -eq 1){exit 0}else{exit 1}")
}
#[cfg(target_os = "windows")]
fn refresh_windows_firewall_for_executable(engine: &str, exe: &Path) {
    if !firewall_pref_enabled() { return; }
    let name = format!("MinerDesk - {engine} outbound");
    let p = exe.to_string_lossy().replace('\'', "''");
    let _ = powershell_run(&format!("Get-NetFirewallRule -DisplayName '{name}' -ErrorAction SilentlyContinue | Remove-NetFirewallRule -ErrorAction SilentlyContinue; New-NetFirewallRule -DisplayName '{name}' -Direction Outbound -Action Allow -Profile Any -Program '{p}' | Out-Null"));
}

#[cfg(target_os = "windows")]
fn refresh_windows_web_firewall(web: &WebConfig) -> Result<(), String> {
    if !firewall_pref_enabled() { return Ok(()); }
    let name = "MinerDesk - Web UI inbound";
    let create = if web.expose_lan {
        format!("New-NetFirewallRule -DisplayName '{name}' -Direction Inbound -Action Allow -Profile Private -Protocol TCP -LocalPort {} | Out-Null", web.desktop_api_port)
    } else { String::new() };
    powershell_run(&format!("Get-NetFirewallRule -DisplayName '{name}' -ErrorAction SilentlyContinue | Remove-NetFirewallRule -ErrorAction SilentlyContinue; {create}"))
}

fn security_status_impl() -> Result<SecurityStatus, String> {
    let root = managed_miners_root()?;
    #[cfg(target_os = "windows")]
    {
        let r = root.to_string_lossy().replace('\'', "''");
        let fw = firewall_pref_enabled();
        let def = powershell_bool(&format!("$p=(Get-MpPreference).ExclusionPath; if($p -contains '{r}'){{exit 0}}else{{exit 1}}"));
        return Ok(SecurityStatus { supported: true, firewall_enabled: fw, defender_exclusion_enabled: def, miner_root: root.to_string_lossy().to_string() });
    }
    #[cfg(not(target_os = "windows"))]
    Ok(SecurityStatus { supported: false, firewall_enabled: false, defender_exclusion_enabled: false, miner_root: root.to_string_lossy().to_string() })
}

fn set_firewall_exception_impl(enabled: bool, web: Option<&WebConfig>) -> Result<SecurityStatus, String> {
    #[cfg(target_os = "windows")]
    {
        if enabled {
            powershell_run("$ErrorActionPreference='Stop'; New-Item -Path 'HKLM:\\SOFTWARE\\MinerDesk\\Security' -Force | Out-Null; New-ItemProperty -Path 'HKLM:\\SOFTWARE\\MinerDesk\\Security' -Name 'FirewallRulesAdded' -PropertyType DWord -Value 1 -Force | Out-Null")?;
            for spec in engine_specs() {
                let root = managed_miners_root()?.join(spec.id).join("current");
                if let Some(exe) = find_engine_executable(&root, spec.id) { refresh_windows_firewall_for_executable(spec.id, &exe); }
            }
            if let Some(web) = web { refresh_windows_web_firewall(web)?; }
        } else {
            powershell_run("$ErrorActionPreference='Stop'; Get-NetFirewallRule -DisplayName 'MinerDesk - * outbound' -ErrorAction SilentlyContinue | Remove-NetFirewallRule -ErrorAction SilentlyContinue; Get-NetFirewallRule -DisplayName 'MinerDesk - Web UI inbound' -ErrorAction SilentlyContinue | Remove-NetFirewallRule -ErrorAction SilentlyContinue; New-Item -Path 'HKLM:\\SOFTWARE\\MinerDesk\\Security' -Force | Out-Null; New-ItemProperty -Path 'HKLM:\\SOFTWARE\\MinerDesk\\Security' -Name 'FirewallRulesAdded' -PropertyType DWord -Value 0 -Force | Out-Null")?;
        }
    }
    let _ = (enabled, web);
    security_status_impl()
}

fn set_defender_exception_impl(enabled: bool) -> Result<SecurityStatus, String> {
    #[cfg(target_os = "windows")]
    {
        let root = managed_miners_root()?;
        let p = root.to_string_lossy().replace('\'', "''");
        if enabled {
            powershell_run(&format!("$ErrorActionPreference='Stop'; New-Item -ItemType Directory -Path '{p}' -Force | Out-Null; Add-MpPreference -ExclusionPath '{p}'; New-Item -Path 'HKLM:\\SOFTWARE\\MinerDesk\\Security' -Force | Out-Null; New-ItemProperty -Path 'HKLM:\\SOFTWARE\\MinerDesk\\Security' -Name 'DefenderExclusionAdded' -PropertyType DWord -Value 1 -Force | Out-Null"))?;
        } else {
            powershell_run(&format!("$ErrorActionPreference='Stop'; Remove-MpPreference -ExclusionPath '{p}' -ErrorAction SilentlyContinue; New-Item -Path 'HKLM:\\SOFTWARE\\MinerDesk\\Security' -Force | Out-Null; New-ItemProperty -Path 'HKLM:\\SOFTWARE\\MinerDesk\\Security' -Name 'DefenderExclusionAdded' -PropertyType DWord -Value 0 -Force | Out-Null"))?;
        }
    }
    let _ = enabled;
    security_status_impl()
}

#[tauri::command]
fn get_security_status() -> Result<SecurityStatus, String> { security_status_impl() }

#[tauri::command]
fn set_firewall_exception(state: tauri::State<'_, Arc<CoreState>>, enabled: bool) -> Result<SecurityStatus, String> {
    let web = state.config().web;
    set_firewall_exception_impl(enabled, Some(&web))
}

#[tauri::command]
fn set_defender_exception(enabled: bool) -> Result<SecurityStatus, String> { set_defender_exception_impl(enabled) }

#[tauri::command]
fn pick_miner_file() -> Option<String> {
    let dialog = rfd::FileDialog::new().set_title("Sélectionner un mineur");
    let picked = if cfg!(target_os = "windows") { dialog.add_filter("Executable", &["exe"]).pick_file() } else { dialog.pick_file() };
    picked.map(|p| p.to_string_lossy().to_string())
}

#[tauri::command]
fn get_system_info() -> Value { json!({"os":std::env::consts::OS,"arch":std::env::consts::ARCH,"apiPort":DEFAULT_WEB_PORT}) }

// ---------- Tauri desktop ----------

fn request_backend_shutdown(port: u16) -> Result<(), String> {
    let addr = format!("127.0.0.1:{port}");
    let socket_addr: SocketAddr = addr.parse().map_err(|e| format!("invalid backend address: {e}"))?;
    let mut stream = TcpStream::connect_timeout(
        &socket_addr,
        Duration::from_millis(800),
    ).map_err(|e| format!("connect backend for shutdown: {e}"))?;
    let desktop_pid = std::process::id();
    let request = format!(
        "POST /api/backend/shutdown HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-MinerDesk-Desktop-Pid: {desktop_pid}\r\nConnection: close\r\nContent-Length: 0\r\n\r\n"
    );
    stream.write_all(request.as_bytes()).map_err(|e| e.to_string())?;
    let _ = stream.flush();
    Ok(())
}

#[cfg(target_os = "windows")]
fn request_backend_desktop_heartbeat(port: u16) -> Result<(), String> {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_millis(350))
        .map_err(|e| format!("connect backend heartbeat: {e}"))?;
    let _ = stream.set_write_timeout(Some(Duration::from_millis(350)));
    let desktop_pid = std::process::id();
    let request = format!(
        "POST /api/backend/desktop-heartbeat HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-MinerDesk-Desktop-Pid: {desktop_pid}\r\nConnection: close\r\nContent-Length: 0\r\n\r\n"
    );
    stream.write_all(request.as_bytes()).map_err(|e| e.to_string())?;
    let _ = stream.flush();
    Ok(())
}

#[cfg(target_os = "windows")]
fn start_backend_desktop_heartbeat(port: u16) {
    thread::spawn(move || {
        let mut consecutive_failures = 0_u32;
        let mut failure_logged = false;
        loop {
            match request_backend_desktop_heartbeat(port) {
                Ok(()) => {
                    if failure_logged {
                        backend_diagnostic_log(&format!(
                            "Desktop heartbeat connection restored on 127.0.0.1:{port}"
                        ));
                    }
                    consecutive_failures = 0;
                    failure_logged = false;
                }
                Err(e) => {
                    consecutive_failures = consecutive_failures.saturating_add(1);
                    if consecutive_failures >= 10 && !failure_logged {
                        backend_diagnostic_log(&format!(
                            "Desktop heartbeat cannot reach backend on 127.0.0.1:{port}: {e}"
                        ));
                        failure_logged = true;
                    }
                }
            }
            thread::sleep(Duration::from_secs(2));
        }
    });
}

#[cfg(target_os = "windows")]
fn shutdown_backend_for_desktop_exit(port: u16) {
    // Run exactly once for every real Desktop process exit. This covers the tray
    // Quit action, the window close button (when close-to-tray is disabled),
    // Alt+F4, Windows session shutdown/logoff and any other normal Tauri exit.
    if DESKTOP_EXIT_SHUTDOWN_STARTED.swap(true, Ordering::SeqCst) {
        return;
    }

    backend_diagnostic_log(&format!(
        "Desktop is exiting; requesting minerdesk-headless shutdown on port {port}"
    ));
    if let Err(e) = request_backend_shutdown(port) {
        // A connection failure usually means the backend is already stopped. Keep
        // the exit path best-effort and never prevent the Desktop from closing.
        backend_diagnostic_log(&format!("Desktop backend shutdown request: {e}"));
        return;
    }

    // The headless endpoint stops all miners first, then exits after a short grace
    // period. Waiting briefly here makes upgrades much less likely to find the EXE
    // still locked immediately after the Desktop disappears.
    for _ in 0..20 {
        thread::sleep(Duration::from_millis(100));
        if !backend_health_once(port) {
            backend_diagnostic_log("minerdesk-headless stopped with Desktop");
            return;
        }
    }
    backend_diagnostic_log(
        "minerdesk-headless did not stop within 2 seconds after Desktop exit request",
    );
}

fn backend_health_once(port: u16) -> bool {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let Ok(mut stream) = TcpStream::connect_timeout(&addr, Duration::from_millis(500)) else { return false; };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(900)));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(500)));
    let request = format!(
        "GET /api/health HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\nAccept: application/json\r\n\r\n"
    );
    if stream.write_all(request.as_bytes()).is_err() { return false; }

    // The health response is tiny. Reading one chunk avoids waiting for a full
    // connection close, which could occasionally time out while the backend is
    // busy processing miner output.
    let mut buf = [0u8; 4096];
    let mut response = Vec::with_capacity(4096);
    for _ in 0..4 {
        let Ok(n) = stream.read(&mut buf) else { break; };
        if n == 0 { break; }
        response.extend_from_slice(&buf[..n]);
        let text = String::from_utf8_lossy(&response);
        if text.starts_with("HTTP/1.1 200")
            && text.contains("application/json")
            && text.contains("\"scheduler\":true")
            && text.contains("\"version\"")
        {
            return true;
        }
        if response.len() >= 16 * 1024 { break; }
    }
    false
}

fn backend_health_ok(port: u16) -> bool {
    if backend_health_once(port) { return true; }
    // One missed localhost probe should not make the Desktop report the
    // privileged backend as offline. Retry once after a very short pause.
    thread::sleep(Duration::from_millis(80));
    backend_health_once(port)
}


#[derive(Debug, Clone, Serialize)]
struct BackendStatus {
    supported: bool,
    reachable: bool,
    port: u16,
    task_installed: bool,
    task_state: String,
    process_running: bool,
    listener_pid: Option<u32>,
    listener_process: String,
    executable_path: String,
    last_task_result: String,
    log_tail: String,
}

fn backend_log_tail(max_lines: usize) -> String {
    let Some(base) = dirs_next::config_dir() else { return String::new(); };
    let path = base.join("MinerDesk").join("backend.log");
    let Ok(text) = fs::read_to_string(path) else { return String::new(); };
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(max_lines);
    lines[start..].join("\n")
}

fn locate_desktop_backend_executable() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(current) = std::env::current_exe() {
        if let Some(dir) = current.parent() {
            candidates.push(dir.join(if cfg!(target_os = "windows") { "minerdesk-backend.exe" } else { "minerdesk-headless" }));
            candidates.push(dir.join("resources").join(if cfg!(target_os = "windows") { "minerdesk-backend.exe" } else { "minerdesk-headless" }));
        }
    }
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    candidates.push(manifest.join("target").join("debug").join(if cfg!(target_os = "windows") { "minerdesk-backend.exe" } else { "minerdesk-headless" }));
    candidates.push(manifest.join("target").join("release").join(if cfg!(target_os = "windows") { "minerdesk-backend.exe" } else { "minerdesk-headless" }));
    candidates.into_iter().find(|p| p.is_file())
}

#[cfg(target_os = "windows")]
fn powershell_text(script: &str) -> String {
    match powershell_command()
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", script])
        .output()
    {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).trim().to_string(),
        _ => String::new(),
    }
}

#[cfg(target_os = "windows")]
fn windows_backend_task_installed() -> bool {
    powershell_bool("if(Get-ScheduledTask -TaskName 'MinerDesk Privileged Backend' -ErrorAction SilentlyContinue){exit 0}else{exit 1}")
}

#[cfg(target_os = "windows")]
fn backend_status_for_port(port: u16) -> BackendStatus {
    let listener_pid = powershell_text(&format!(
        "$c=Get-NetTCPConnection -LocalPort {port} -State Listen -ErrorAction SilentlyContinue | Select-Object -First 1; if($c){{$c.OwningProcess}}"
    )).parse::<u32>().ok();
    let listener_process = listener_pid
        .map(|pid| powershell_text(&format!("$p=Get-Process -Id {pid} -ErrorAction SilentlyContinue; if($p){{$p.ProcessName}}")))
        .unwrap_or_default();
    let task_installed = windows_backend_task_installed();
    let task_state = powershell_text("$t=Get-ScheduledTask -TaskName 'MinerDesk Privileged Backend' -ErrorAction SilentlyContinue; if($t){$t.State}else{'Missing'}");
    let last_task_result = powershell_text("$i=Get-ScheduledTaskInfo -TaskName 'MinerDesk Privileged Backend' -ErrorAction SilentlyContinue; if($i){$i.LastTaskResult}else{''}");
    let process_running = powershell_bool("if(Get-Process minerdesk-backend,minerdesk-headless -ErrorAction SilentlyContinue){exit 0}else{exit 1}");
    BackendStatus {
        supported: true,
        reachable: backend_health_ok(port),
        port,
        task_installed,
        task_state,
        process_running,
        listener_pid,
        listener_process,
        executable_path: locate_desktop_backend_executable().map(|p| p.to_string_lossy().to_string()).unwrap_or_default(),
        last_task_result,
        log_tail: backend_log_tail(30),
    }
}

#[cfg(not(target_os = "windows"))]
fn backend_status_for_port(port: u16) -> BackendStatus {
    BackendStatus {
        supported: false,
        reachable: backend_health_ok(port),
        port,
        task_installed: false,
        task_state: "Not applicable".into(),
        process_running: backend_health_ok(port),
        listener_pid: None,
        listener_process: String::new(),
        executable_path: locate_desktop_backend_executable().map(|p| p.to_string_lossy().to_string()).unwrap_or_default(),
        last_task_result: String::new(),
        log_tail: backend_log_tail(30),
    }
}

fn wait_for_backend(port: u16, attempts: usize, delay_ms: u64) -> bool {
    for _ in 0..attempts {
        if backend_health_ok(port) { return true; }
        thread::sleep(Duration::from_millis(delay_ms));
    }
    backend_health_ok(port)
}

#[cfg(target_os = "windows")]
fn launch_headless_elevated(port: u16) -> Result<(), String> {
    let exe = locate_desktop_backend_executable().ok_or_else(|| "minerdesk-backend.exe was not found. Build the windowless backend or reinstall MinerDesk.".to_string())?;
    let exe_ps = exe.to_string_lossy().replace('\'', "''");
    let wd_ps = exe.parent().unwrap_or_else(|| Path::new(".")).to_string_lossy().replace('\'', "''");
    let script = format!(
        "$a=@('--desktop-owned','--listen','127.0.0.1','--port','{port}'); Start-Process -FilePath '{exe_ps}' -WorkingDirectory '{wd_ps}' -ArgumentList $a -Verb RunAs -WindowStyle Hidden"
    );
    powershell_run(&script)
}

#[cfg(target_os = "windows")]
fn repair_backend_task_elevated() -> Result<(), String> {
    let exe = locate_desktop_backend_executable().ok_or_else(|| "minerdesk-backend.exe was not found. Build the windowless backend or reinstall MinerDesk.".to_string())?;
    let Some(base) = dirs_next::config_dir() else { return Err("MinerDesk configuration directory was not found".into()); };
    let dir = base.join("MinerDesk");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let script_path = dir.join("repair-privileged-backend.ps1");
    let exe_ps = exe.to_string_lossy().replace('\'', "''");
    let wd_ps = exe.parent().unwrap_or_else(|| Path::new(".")).to_string_lossy().replace('\'', "''");
    let repair = format!(r#"$ErrorActionPreference='Stop'
$taskName='MinerDesk Privileged Backend'
Stop-ScheduledTask -TaskName $taskName -ErrorAction SilentlyContinue
$owned=@('{exe_ps}',(Join-Path '{wd_ps}' 'minerdesk-headless.exe'),(Join-Path '{wd_ps}' 'resources\minerdesk-headless.exe'))
Get-CimInstance Win32_Process -ErrorAction Stop | Where-Object {{ $_.ExecutablePath -and $_.ExecutablePath -in $owned }} | ForEach-Object {{ Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }}
$user=(Get-CimInstance Win32_ComputerSystem).UserName
if([string]::IsNullOrWhiteSpace($user)){{$user=[System.Security.Principal.WindowsIdentity]::GetCurrent().Name}}
$action=New-ScheduledTaskAction -Execute '{exe_ps}' -Argument '--desktop-owned' -WorkingDirectory '{wd_ps}'
$trigger=New-ScheduledTaskTrigger -AtLogOn -User $user
$principal=New-ScheduledTaskPrincipal -UserId $user -LogonType Interactive -RunLevel Highest
$settings=New-ScheduledTaskSettingsSet -StartWhenAvailable -RestartCount 3 -RestartInterval (New-TimeSpan -Minutes 1) -ExecutionTimeLimit ([TimeSpan]::Zero) -MultipleInstances IgnoreNew
Register-ScheduledTask -TaskName $taskName -Action $action -Trigger $trigger -Principal $principal -Settings $settings -Description 'MinerDesk privileged mining backend' -Force | Out-Null
Start-ScheduledTask -TaskName $taskName
"#);
    fs::write(&script_path, repair).map_err(|e| e.to_string())?;
    let path_ps = script_path.to_string_lossy().replace('\'', "''");
    let launcher = format!(
        "$a=@('-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File','{path_ps}'); $p=Start-Process -FilePath 'powershell.exe' -ArgumentList $a -Verb RunAs -WindowStyle Hidden -Wait -PassThru; exit $p.ExitCode"
    );
    powershell_run(&launcher)
}

fn backend_status_quick_for_port(port: u16) -> BackendStatus {
    let reachable = backend_health_ok(port);
    BackendStatus {
        supported: cfg!(target_os = "windows"),
        reachable,
        port,
        // Keep the hot path cheap. Full Windows task/process diagnostics spawn
        // PowerShell and are intentionally only collected on demand.
        task_installed: reachable,
        task_state: if reachable { "Running".into() } else { "Unknown".into() },
        process_running: reachable,
        listener_pid: None,
        listener_process: if reachable { "minerdesk-backend".into() } else { String::new() },
        executable_path: locate_desktop_backend_executable().map(|p| p.to_string_lossy().to_string()).unwrap_or_default(),
        last_task_result: String::new(),
        log_tail: String::new(),
    }
}

#[tauri::command]
fn get_backend_status(state: tauri::State<'_, Arc<CoreState>>) -> Result<BackendStatus, String> {
    let port = state.config().web.desktop_api_port;
    Ok(backend_status_quick_for_port(port))
}

#[tauri::command]
fn get_backend_diagnostics(state: tauri::State<'_, Arc<CoreState>>) -> Result<BackendStatus, String> {
    let port = state.config().web.desktop_api_port;
    Ok(backend_status_for_port(port))
}

#[tauri::command]
fn start_privileged_backend(state: tauri::State<'_, Arc<CoreState>>) -> Result<BackendStatus, String> {
    let port = state.config().web.desktop_api_port;
    if backend_health_ok(port) { return Ok(backend_status_for_port(port)); }
    #[cfg(target_os = "windows")]
    {
        let status = backend_status_for_port(port);
        if status.listener_pid.is_some() && !status.reachable {
            return Err(format!("Port {port} is already used by {} (PID {:?}).", status.listener_process, status.listener_pid));
        }
        if status.task_installed {
            try_start_windows_privileged_backend_task();
            if wait_for_backend(port, 40, 125) { return Ok(backend_status_for_port(port)); }
        }
        launch_headless_elevated(port)?;
        if !wait_for_backend(port, 80, 125) {
            return Err(format!("The privileged backend did not become reachable on 127.0.0.1:{port}. Check the backend log."));
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        return Err("Use the built-in desktop backend on this platform.".into());
    }
    Ok(backend_status_for_port(port))
}

#[tauri::command]
fn restart_privileged_backend(state: tauri::State<'_, Arc<CoreState>>) -> Result<BackendStatus, String> {
    let port = state.config().web.desktop_api_port;
    #[cfg(target_os = "windows")]
    {
        if windows_backend_task_installed() {
            let _ = powershell_run("Stop-ScheduledTask -TaskName 'MinerDesk Privileged Backend' -ErrorAction SilentlyContinue; Start-Sleep -Milliseconds 500; Start-ScheduledTask -TaskName 'MinerDesk Privileged Backend' -ErrorAction Stop");
            if wait_for_backend(port, 50, 150) { return Ok(backend_status_for_port(port)); }
        }
        // If the task is missing/broken, fall back to a direct elevated launch.
        launch_headless_elevated(port)?;
        if !wait_for_backend(port, 80, 125) {
            return Err("Backend restart failed. Use Repair backend to recreate the privileged task.".into());
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        return Err("Backend restart is managed by the desktop process on this platform.".into());
    }
    Ok(backend_status_for_port(port))
}

#[tauri::command]
fn repair_privileged_backend(state: tauri::State<'_, Arc<CoreState>>) -> Result<BackendStatus, String> {
    let port = state.config().web.desktop_api_port;
    #[cfg(target_os = "windows")]
    {
        repair_backend_task_elevated()?;
        if !wait_for_backend(port, 80, 150) {
            return Err("The scheduled task was repaired but the backend is still unavailable. Check backend.log.".into());
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        return Err("Privileged backend repair is only required on Windows.".into());
    }
    Ok(backend_status_for_port(port))
}

#[cfg(target_os = "windows")]
fn try_start_windows_privileged_backend_task() {
    // The installer can create this scheduled task with RunLevel=Highest.
    // Starting an existing task does not elevate the Tauri/WebView2 desktop process.
    let _ = powershell_command()
        .args([
            "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command",
            "Start-ScheduledTask -TaskName 'MinerDesk Privileged Backend' -ErrorAction SilentlyContinue"
        ])
        .status();
}

#[cfg(not(target_os = "windows"))]
fn try_start_windows_privileged_backend_task() {}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let core = CoreState::load().expect("configuration MinerDesk");
    let mut cfg = core.config();
    if cfg.web.expose_lan && cfg.web.token.trim().is_empty() {
        cfg.web.token = Uuid::new_v4().simple().to_string();
        let _ = core.replace_config(cfg.clone());
    }

    let port = cfg.web.desktop_api_port;
    CLOSE_TO_TRAY.store(cfg.ui.close_to_tray, Ordering::SeqCst);
    #[cfg(target_os = "windows")]
    if let Err(e) = sync_windows_startup(cfg.ui.start_with_windows) {
        backend_diagnostic_log(&format!("Windows startup integration sync failed: {e}"));
    }
    #[cfg(target_os = "windows")]
    start_backend_desktop_heartbeat(port);

    // Windows: the desktop UI never binds the mining/backend port itself.
    // The React startup splash explicitly asks Tauri to ensure the privileged
    // backend is reachable. Keeping startup ownership in one place prevents the
    // splash from disappearing before the UAC/backend startup sequence is done.

    // Non-Windows/dev fallback: the desktop process can host the backend itself.
    #[cfg(not(target_os = "windows"))]
    {
        start_scheduler(Arc::clone(&core));
    start_dev_tip_monitor(Arc::clone(&core));
        let listen = if cfg.web.expose_lan { "0.0.0.0" } else { "127.0.0.1" }.to_string();
        let token = if cfg.web.expose_lan { Some(cfg.web.token.clone()) } else { None };
        spawn_desktop_web(Arc::clone(&core), listen, port, token);
    }

    let app = tauri::Builder::default()
        .manage(Arc::clone(&core))
        .invoke_handler(tauri::generate_handler![pick_miner_file, get_system_info, get_security_status, set_firewall_exception, set_defender_exception, get_backend_status, get_backend_diagnostics, start_privileged_backend, restart_privileged_backend, repair_privileged_backend, set_start_with_windows, set_close_to_tray, focus_main_window])
        .setup(move |app| {
            configure_windows_tray(app, &cfg)?;
            #[cfg(target_os = "windows")]
            start_power_action_focus_watch(app.handle().clone(), port);
            Ok(())
        })
        .on_window_event(|window, event| {
            #[cfg(target_os = "windows")]
            {
                if window.label() != "main" { return; }
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    if CLOSE_TO_TRAY.load(Ordering::SeqCst) {
                        api.prevent_close();
                        let _ = window.hide();
                    }
                }
            }
            #[cfg(not(target_os = "windows"))]
            let _ = (window, event);
        })
        .build(tauri::generate_context!())
        .expect("error while building MinerDesk");

    app.run(move |_app_handle, event| {
        #[cfg(not(target_os = "windows"))]
        if matches!(event, tauri::RunEvent::Exit) {
            core.stop_all();
        }

        #[cfg(target_os = "windows")]
        if matches!(event, tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit) {
            // Since 0.7.5 the privileged backend follows the Desktop lifetime.
            // Reload from disk because the live backend is the authoritative writer
            // and may have changed the configured API port while the UI was open.
            let exit_port = CoreState::load()
                .map(|latest| latest.config().web.desktop_api_port)
                .unwrap_or(port);
            shutdown_backend_for_desktop_exit(exit_port);
        }
    });
}

// ---------- Headless CLI ----------

#[derive(Parser, Debug)]
#[command(name = "minerdesk-headless", version = "0.7.22", about = "MinerDesk headless miner orchestrator with web dashboard")]
struct HeadlessArgs {
    /// Listen interface. 127.0.0.1 = local only, 0.0.0.0 = LAN.
    #[arg(long)]
    listen: Option<String>,
    /// Web/API port. If omitted, the value saved in MinerDesk settings is used.
    #[arg(long)]
    port: Option<u16>,
    /// Access token for remote API/web clients. A token is generated automatically for non-loopback listening.
    #[arg(long)]
    token: Option<String>,
    /// Internal lifecycle flag used only by the MinerDesk Desktop managed backend.
    /// Plain command-line/headless launches intentionally leave this disabled.
    #[arg(long, hide = true)]
    desktop_owned: bool,
}

pub fn run_headless() { run_backend(false); }

/// Entry point for minerdesk-backend.exe (Windows GUI subsystem, no console).
/// No wrapper process is involved: this is the actual owner of the miner jobs.
pub fn run_desktop_backend() {
    std::panic::set_hook(Box::new(|info| backend_diagnostic_log(&format!("Backend panic: {info}"))));
    run_backend(true);
}

fn run_backend(windowless: bool) {
    let args = match HeadlessArgs::try_parse() {
        Ok(args) => args,
        Err(error) => {
            if windowless {
                backend_diagnostic_log(&format!("Backend command line: {error}"));
                std::process::exit(error.exit_code());
            }
            error.exit();
        }
    };
    let desktop_owned = windowless || args.desktop_owned;
    backend_diagnostic_log(&format!(
        "{} 0.7.22 starting ({})",
        if windowless { "minerdesk-backend / windowless" } else { "minerdesk-headless" },
        if desktop_owned { "desktop-owned" } else { "standalone CLI" }
    ));
    let core = match CoreState::load() {
        Ok(core) => core,
        Err(e) => {
            backend_diagnostic_log(&format!("configuration error: {e}"));
            eprintln!("MinerDesk configuration: {e}");
            return;
        }
    };
    #[cfg(target_os = "windows")]
    match windows_miner_job::probe() {
        Ok(()) => backend_diagnostic_log("Windows miner crash guard ready: Job Object KILL_ON_JOB_CLOSE"),
        Err(e) => {
            backend_diagnostic_log(&format!("Windows miner crash guard initialization failed: {e}"));
            eprintln!("WARNING: Windows miner crash guard is unavailable: {e}");
            eprintln!("MinerDesk will refuse to start miners until the Job Object can be created.");
        }
    }
    let cfg = core.config();
    let listen = args.listen.unwrap_or_else(|| if cfg.web.expose_lan { "0.0.0.0".into() } else { "127.0.0.1".into() });
    let port = args.port.unwrap_or(cfg.web.desktop_api_port);
    backend_diagnostic_log(&format!("binding web/API backend on {listen}:{port}; config={}", core.config_path.display()));
    let ip: IpAddr = listen.parse().unwrap_or_else(|_| "127.0.0.1".parse().unwrap());
    let mut token = args.token.or_else(|| (!cfg.web.token.trim().is_empty()).then(|| cfg.web.token.clone()));
    if !ip.is_loopback() && token.is_none() { token = Some(Uuid::new_v4().simple().to_string()); }
    start_scheduler(Arc::clone(&core));
    start_dev_tip_monitor(Arc::clone(&core));
    if !windowless {
        println!("MinerDesk Headless 0.7.22");
        println!("Mode: {}", if desktop_owned { "Desktop-managed backend" } else { "standalone headless (Desktop watchdog disabled)" });
        #[cfg(target_os = "windows")]
        println!("Miner crash guard: Windows Job Object (kill-on-close)");
        println!("Configuration: {}", core.config_path.display());
        println!("Web: http://{}:{}/", listen, port);
        if let Some(t) = token.as_deref() {
            println!("Token: {t}");
            println!("Local URL: http://127.0.0.1:{}/?token={t}", port);
        }
    }
    let rt = tokio::runtime::Runtime::new().expect("headless runtime");
    let result = rt.block_on(async {
        tokio::select! {
            r = serve_web(Arc::clone(&core), listen.clone(), port, token, true, desktop_owned) => r,
            _ = async {
                if windowless { std::future::pending::<()>().await; }
                else { let _ = tokio::signal::ctrl_c().await; }
            } => Ok(()),
        }
    });
    core.stop_all();
    match result {
        Ok(()) => backend_diagnostic_log("backend stopped normally"),
        Err(e) => {
            backend_diagnostic_log(&format!("backend stopped with error: {e}"));
            eprintln!("{e}");
        }
    }
}
