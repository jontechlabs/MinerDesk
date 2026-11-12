//! Restricted Fleet adapter. Never forward remote traffic to the trusted local HTTP API.
use crate::{AppConfig, CoreState, GpuTuning, MinerProfile, ScheduleWindow};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    sync::{atomic::Ordering, Arc},
};

const EDITABLE: &[&str] = &[
    "name",
    "enabled",
    "algorithm",
    "pool",
    "wallet",
    "secondary_wallet",
    "merge_secondary",
    "merge_separator",
    "worker",
    "gpu_ids",
    "gpu_tuning",
    "core_clock",
    "memory_clock",
    "core_offset",
    "memory_offset",
    "power_limit",
    "fan",
    "disable_cpu",
    "allow_gpu_overlap",
];

pub struct FleetCoreBridge {
    core: Arc<CoreState>,
}
pub type CoreBridge = FleetCoreBridge;

impl FleetCoreBridge {
    pub(crate) fn new(core: Arc<CoreState>) -> Self {
        Self { core }
    }

    fn safe_config(&self) -> Value {
        config_view(&self.core.config())
    }

    fn snapshot_value(&self) -> Value {
        let cfg = self.core.config();
        let mut updates = serde_json::to_value(self.core.fleet_updates.status()).unwrap_or(Value::Null);
        if let Some(error) = updates.pointer("/job/error").and_then(Value::as_str) {
            let error = redact(error, &cfg);
            updates["job"]["error"] = json!(error);
        }
        let now = chrono::Local::now();
        let status: Vec<Value> = self.core.statuses().into_iter().map(|s| {
            let mode = if s.runtime.running {
                if s.runtime.started_by == "schedule" { "schedule" }
                else if s.has_schedule { "forced" } else { "manual" }
            } else if s.has_schedule && !s.scheduler_paused && s.profile.enabled { "waiting" }
            else { "stopped" };
            json!({
                "id":s.profile.id,"name":safe_label(&s.profile.name,128),"engine":s.profile.engine,
                "algorithm":safe_label(&s.profile.algorithm,64),"enabled":s.profile.enabled,
                "gpu_ids":safe_label(&s.profile.gpu_ids,512),"running":s.runtime.running,"mode":mode,
                "scheduled_now":s.scheduled_now,"has_schedule":s.has_schedule,
                "scheduler_paused":s.scheduler_paused,"uptime_seconds":s.runtime.uptime_seconds,
                "dev_tip_active":s.runtime.dev_tip_active,
                "metrics":s.runtime.metrics,"last_error":redact(&s.runtime.last_error,&cfg),
                "root_authorization":root_state(&s.profile),
            })
        }).collect();
        json!({
            "health":if self.core.shutting_down.load(Ordering::SeqCst) {"shutting_down"} else {"online"},
            "name":machine_name(),"os":std::env::consts::OS,"arch":std::env::consts::ARCH,
            "version":env!("CARGO_PKG_VERSION"),"local_time":now.to_rfc3339(),
            "timezone":now.offset().to_string(),"utc_offset_seconds":now.offset().local_minus_utc(),
            "revision":config_revision(&cfg),"status":status,
            // CoreState has no cached inventory. A status poll must never launch a miner
            // (including its device-list command) or invent system-to-engine ID mappings.
            "gpu_inventory":[],"gpu_inventory_available":false,
            "updates":updates,
        })
    }

    fn patch_profile(
        &self,
        id: &str,
        expected: &str,
        patch: &Value,
        expires_at: u64,
    ) -> Result<Value, String> {
        valid_id(id)?;
        let _io = self
            .core
            .config_io
            .lock()
            .map_err(|_| "Configuration is busy")?;
        let _life = self
            .core
            .lifecycle
            .lock()
            .map_err(|_| "Miner lifecycle is busy")?;
        ensure_not_expired(expires_at)?;
        if self.core.is_running(id) {
            return Err("Stop this profile before editing it remotely".into());
        }
        let mut current = self
            .core
            .config
            .write()
            .map_err(|_| "Configuration is busy")?;
        let mut next = current.clone();
        let profile = next
            .miners
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or("Profile was not found")?;
        *profile = patched_profile(profile, expected, patch)?;
        ensure_not_expired(expires_at)?;
        self.core
            .write_config_file(&next)
            .map_err(|_| "Could not save configuration on the rig")?;
        *current = next.clone();
        drop(current);
        self.queue_platform_sync(&next);
        let profile = next.miners.iter().find(|p| p.id == id).unwrap();
        Ok(
            json!({"ok":true,"profile_id":id,"profile_revision":profile_revision(profile),"revision":config_revision(&next),
            "requires_local_root_review":profile.linux_run_as_root,
            "message":if profile.linux_run_as_root {"Saved. Changed root commands require local authorization; no password is requested remotely."} else {"Saved. Start or resume the profile when ready."}}),
        )
    }

    fn patch_schedules(
        &self,
        expected: &str,
        schedules: &[ScheduleWindow],
        expires_at: u64,
    ) -> Result<Value, String> {
        let _io = self
            .core
            .config_io
            .lock()
            .map_err(|_| "Configuration is busy")?;
        let _life = self
            .core
            .lifecycle
            .lock()
            .map_err(|_| "Miner lifecycle is busy")?;
        ensure_not_expired(expires_at)?;
        let mut current = self
            .core
            .config
            .write()
            .map_err(|_| "Configuration is busy")?;
        let next = patched_schedules(&current, expected, schedules)?;
        ensure_not_expired(expires_at)?;
        self.core
            .write_config_file(&next)
            .map_err(|_| "Could not save schedules on the rig")?;
        *current = next.clone();
        drop(current);
        self.queue_platform_sync(&next);
        Ok(
            json!({"ok":true,"revision":config_revision(&next),"schedule_count":next.schedules.len(),
            "message":"Saved. Times remain in this rig's local timezone; existing local power-action settings are preserved."}),
        )
    }

    fn queue_platform_sync(&self, cfg: &AppConfig) {
        #[cfg(target_os = "windows")]
        if let Ok(sender) = self.core.config_sync_sender.lock() {
            if let Some(sender) = sender.as_ref() {
                let _ = sender.send(cfg.clone());
            }
        }
        #[cfg(not(target_os = "windows"))]
        let _ = cfg;
    }

    fn mining_command(&self, id: &str, action: &str, expires_at: u64) -> Result<Value, String> {
        valid_id(id)?;
        // Serializes a launch with local and remote saves. Do not let a local save
        // turn an approved profile into a different command between validation/start.
        let _io = self
            .core
            .config_io
            .lock()
            .map_err(|_| "Configuration is busy")?;
        let _life = self
            .core
            .lifecycle
            .lock()
            .map_err(|_| "Miner lifecycle is busy")?;
        ensure_not_expired(expires_at)?;
        let cfg = self.core.config();
        let profile = cfg
            .miners
            .iter()
            .find(|p| p.id == id)
            .ok_or("Profile was not found")?;
        if cfg!(target_os = "linux") && action != "stop" {
            validate_root_reference(profile)?;
        }
        let mode = match action {
            "start" | "resume" => Some(
                self.core
                    .start_miner_respecting_schedule_locked_with_interaction(
                        id,
                        false,
                        chrono::Local::now(),
                        false,
                    )?,
            ),
            "restart" => Some(
                self.core
                    .start_miner_respecting_schedule_locked_with_interaction(
                        id,
                        true,
                        chrono::Local::now(),
                        false,
                    )?,
            ),
            "force_start" => {
                self.core
                    .start_miner_locked_with_interaction(id, "manual", false)?;
                Some("manual")
            }
            "stop" => {
                self.core.stop_miner_manually_locked(id)?;
                None
            }
            _ => return Err("Unsupported mining action".into()),
        };
        Ok(json!({"ok":true,"profile_id":id,"mode":mode}))
    }
}

impl super::FleetCore for FleetCoreBridge {
    fn snapshot(&self) -> Result<Value, String> {
        Ok(self.snapshot_value())
    }
    fn config(&self) -> Result<Value, String> {
        Ok(self.safe_config())
    }
    fn execute(
        &self,
        operation: &super::protocol::FleetOperation,
        expires_at: u64,
    ) -> Result<Value, String> {
        use super::protocol::FleetOperation::*;
        ensure_not_expired(expires_at)?;
        let result = match operation {
            Start { profile_id } => self.mining_command(profile_id, "start", expires_at),
            Stop { profile_id } => self.mining_command(profile_id, "stop", expires_at),
            Restart { profile_id } => self.mining_command(profile_id, "restart", expires_at),
            Resume { profile_id } => self.mining_command(profile_id, "resume", expires_at),
            ForceStart { profile_id } => self.mining_command(profile_id, "force_start", expires_at),
            ProfilePatch {
                profile_id,
                expected_revision,
                patch,
            } => self.patch_profile(profile_id, expected_revision, patch, expires_at),
            SchedulesPatch {
                expected_revision,
                schedules,
            } => self.patch_schedules(expected_revision, schedules, expires_at),
            Logs { profile_id, limit } => {
                valid_id(profile_id)?;
                let cfg = self.core.config();
                if !cfg.miners.iter().any(|p| p.id == *profile_id) {
                    return Err("Profile was not found".into());
                }
                Ok(log_view(
                    profile_id,
                    self.core.logs_for(profile_id, usize::from(*limit).min(300)),
                    &cfg,
                ))
            }
            UpdateCheck => self.core.fleet_updates.check()
                .and_then(|state| serde_json::to_value(state).map_err(|_| "Cannot encode update status".to_string())),
            UpdateInstall { expected_version } => self.core.fleet_updates.install(self.core.clone(), expected_version)
                .and_then(|state| serde_json::to_value(state).map_err(|_| "Cannot encode update status".to_string())),
        };
        // Core errors can contain full command lines, paths and pool credentials.
        result.map_err(|error| redact(&error, &self.core.config()))
    }
}

fn log_view(profile_id: &str, source: Vec<crate::LogLine>, cfg: &AppConfig) -> Value {
    let total = source.len();
    let mut lines = Vec::new();
    let mut budget = 6 * 1024;
    // Keep the newest useful lines inside the transport's bounded receipt size.
    // A long console must not turn every Logs result into an omitted-result notice.
    for line in source.into_iter().rev() {
        let value = json!({"ts":line.ts,"stream":safe_label(&line.stream,16),"text":safe_label(&redact(&line.text,cfg),512)});
        let bytes = serde_json::to_vec(&value)
            .map(|v| v.len() + 1)
            .unwrap_or(usize::MAX);
        if bytes > budget {
            break;
        }
        budget -= bytes;
        lines.push(value);
    }
    lines.reverse();
    json!({"profile_id":profile_id,"truncated":lines.len()<total,"lines":lines})
}

fn ensure_not_expired(expires_at: u64) -> Result<(), String> {
    if crate::unix_now() >= expires_at {
        Err("Fleet command expired before local execution; it was not applied".into())
    } else {
        Ok(())
    }
}

fn machine_name() -> String {
    super::persistence::local_name()
}

fn root_state(p: &MinerProfile) -> &'static str {
    if !cfg!(target_os = "linux") {
        "not_applicable"
    } else if !p.linux_run_as_root {
        "not_requested"
    } else if p.linux_root_authorization.is_some() {
        "approval_saved_unverified"
    } else {
        "required"
    }
}

fn validate_root_reference(p: &MinerProfile) -> Result<(), String> {
    if !p.linux_run_as_root {
        return Ok(());
    }
    let valid = p.linux_root_authorization.as_ref().is_some_and(|grant| {
        grant.id.len() == 32
            && grant.id.bytes().all(|b| b.is_ascii_hexdigit())
            && grant.fingerprint.len() == 64
            && grant.fingerprint.bytes().all(|b| b.is_ascii_hexdigit())
    });
    if valid {
        Ok(())
    } else {
        Err("Root authorization required: review and authorize this profile locally on the Linux rig. Fleet never opens a password dialog.".into())
    }
}

fn profile_content(p: &MinerProfile) -> Value {
    json!({"id":p.id,"name":p.name,"engine":p.engine,"enabled":p.enabled,"algorithm":p.algorithm,
        "pool":redact_pool_userinfo(&p.pool),"wallet":p.wallet,"secondary_wallet":p.secondary_wallet,
        "merge_secondary":p.merge_secondary,"merge_separator":p.merge_separator,"worker":p.worker,
        "gpu_ids":p.gpu_ids,"gpu_tuning":p.gpu_tuning,"core_clock":p.core_clock,
        "memory_clock":p.memory_clock,"core_offset":p.core_offset,"memory_offset":p.memory_offset,
        "power_limit":p.power_limit,"fan":p.fan,"disable_cpu":p.disable_cpu,
        "allow_gpu_overlap":p.allow_gpu_overlap,"linux_run_as_root":p.linux_run_as_root,
        "root_authorization":root_state(p)})
}

fn hash(value: &Value) -> String {
    format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(value).unwrap_or_default())
    )
}
fn profile_revision(p: &MinerProfile) -> String {
    hash(&profile_content(p))
}
fn profile_view(p: &MinerProfile) -> Value {
    let mut value = profile_content(p);
    value["revision"] = Value::String(profile_revision(p));
    // Public wallet identifiers are intentionally available only in the operator
    // configuration response, never in periodic telemetry. Pool userinfo is not.
    value["pool"] = Value::String(redact_pool_userinfo(&p.pool));
    value
}
fn config_revision(cfg: &AppConfig) -> String {
    hash(
        &json!({"profiles":cfg.miners.iter().map(profile_content).collect::<Vec<_>>(),"schedules":cfg.schedules}),
    )
}
fn config_view(cfg: &AppConfig) -> Value {
    let now = chrono::Local::now();
    json!({"revision":config_revision(cfg),"profiles":cfg.miners.iter().map(profile_view).collect::<Vec<_>>(),
        "schedules":cfg.schedules,"local_time":now.to_rfc3339(),"timezone":now.offset().to_string(),
        "utc_offset_seconds":now.offset().local_minus_utc(),"editable_profile_fields":EDITABLE})
}

fn valid_id(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
    {
        Err("Invalid profile or schedule identifier".into())
    } else {
        Ok(())
    }
}
fn text(value: &str, max: usize, spaces: bool) -> Result<(), String> {
    if value.len() > max
        || value
            .chars()
            .any(|c| c.is_control() || (!spaces && c.is_whitespace()))
    {
        return Err("A profile field contains invalid text or exceeds its size limit".into());
    }
    Ok(())
}
fn safe_label(value: &str, max: usize) -> String {
    value
        .chars()
        .filter(|c| !c.is_control())
        .take(max)
        .collect()
}

fn typed<T: serde::de::DeserializeOwned>(value: &Value) -> Result<T, String> {
    serde_json::from_value(value.clone()).map_err(|_| "Invalid value type in profile edit".into())
}

fn validate_pool(pool: &str) -> Result<(), String> {
    text(pool, 1024, false)?;
    if pool.is_empty() || pool.starts_with('-') {
        return Err("A pool host and port are required".into());
    }
    let with_scheme = if pool.contains("://") {
        pool.to_string()
    } else {
        format!("stratum+tcp://{pool}")
    };
    let url = reqwest::Url::parse(&with_scheme).map_err(|_| "Invalid pool address")?;
    if ![
        "stratum+tcp",
        "stratum+ssl",
        "stratum+tls",
        "tcp",
        "ssl",
        "tls",
    ]
    .contains(&url.scheme())
        || url.host_str().is_none()
        || url.port().is_none_or(|p| p == 0)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !matches!(url.path(), "" | "/")
    {
        return Err("Use a pool host:port or stratum URL without embedded credentials".into());
    }
    Ok(())
}

fn validate_tuning(p: &MinerProfile) -> Result<(), String> {
    fn values(
        core: Option<u32>,
        memory: Option<u32>,
        co: Option<i32>,
        mo: Option<i32>,
        power: Option<u32>,
        fan: Option<u8>,
    ) -> bool {
        core.is_none_or(|n| n <= 10000)
            && memory.is_none_or(|n| n <= 50000)
            && co.is_none_or(|n| (-10000..=10000).contains(&n))
            && mo.is_none_or(|n| (-10000..=10000).contains(&n))
            && power.is_none_or(|n| n <= 1000)
            && fan.is_none_or(|n| n <= 100)
    }
    if !values(
        p.core_clock,
        p.memory_clock,
        p.core_offset,
        p.memory_offset,
        p.power_limit,
        p.fan,
    ) || p.gpu_tuning.len() > 32
        || p.gpu_tuning.iter().any(|g| {
            !values(
                g.core_clock,
                g.memory_clock,
                g.core_offset,
                g.memory_offset,
                g.power_limit,
                g.fan,
            )
        })
    {
        return Err("Invalid GPU tuning units or value range; verify the engine and hardware limits locally".into());
    }
    let mut ids = HashSet::new();
    for g in &p.gpu_tuning {
        text(&g.selector, 64, false)?;
        if g.selector.is_empty()
            || !g
                .selector
                .bytes()
                .all(|b| b.is_ascii_hexdigit() || matches!(b, b':' | b'.'))
            || !ids.insert(&g.selector)
        {
            return Err("Invalid or duplicate GPU tuning selector".into());
        }
    }
    Ok(())
}

fn patched_profile(
    before: &MinerProfile,
    expected: &str,
    patch: &Value,
) -> Result<MinerProfile, String> {
    if expected != profile_revision(before) {
        return Err("Configuration conflict: refresh the profile before saving".into());
    }
    let fields = patch.as_object().ok_or("Profile edit must be an object")?;
    if fields.is_empty()
        || fields.len() > EDITABLE.len()
        || fields.keys().any(|key| !EDITABLE.contains(&key.as_str()))
    {
        return Err("This edit includes a field that can only be changed locally".into());
    }
    let mut p = before.clone();
    for (key, value) in fields {
        match key.as_str() {
            "name" => p.name = typed(value)?,
            "enabled" => p.enabled = typed(value)?,
            "algorithm" => p.algorithm = typed(value)?,
            "pool" => p.pool = typed(value)?,
            "wallet" => p.wallet = typed(value)?,
            "secondary_wallet" => p.secondary_wallet = typed(value)?,
            "merge_secondary" => p.merge_secondary = typed(value)?,
            "merge_separator" => p.merge_separator = typed(value)?,
            "worker" => p.worker = typed(value)?,
            "gpu_ids" => p.gpu_ids = typed(value)?,
            "core_clock" => p.core_clock = typed(value)?,
            "memory_clock" => p.memory_clock = typed(value)?,
            "core_offset" => p.core_offset = typed(value)?,
            "memory_offset" => p.memory_offset = typed(value)?,
            "power_limit" => p.power_limit = typed(value)?,
            "fan" => p.fan = typed(value)?,
            "disable_cpu" => p.disable_cpu = typed(value)?,
            "allow_gpu_overlap" => p.allow_gpu_overlap = typed(value)?,
            "gpu_tuning" => {
                let items = value.as_array().ok_or("GPU tuning must be an array")?;
                for item in items {
                    let fields = item.as_object().ok_or("GPU tuning must contain objects")?;
                    if fields.keys().any(|key| {
                        ![
                            "selector",
                            "ignore_defaults",
                            "core_clock",
                            "memory_clock",
                            "core_offset",
                            "memory_offset",
                            "power_limit",
                            "fan",
                        ]
                        .contains(&key.as_str())
                    }) {
                        return Err("Unknown field in GPU tuning".into());
                    }
                }
                p.gpu_tuning = typed::<Vec<GpuTuning>>(value)?;
            }
            _ => unreachable!(),
        }
    }
    text(&p.name, 128, true)?;
    text(&p.algorithm, 64, false)?;
    text(&p.wallet, 4096, false)?;
    text(&p.secondary_wallet, 4096, false)?;
    text(&p.worker, 128, true)?;
    text(&p.merge_separator, 8, false)?;
    if p.name.trim().is_empty()
        || p.wallet.is_empty()
        || p.algorithm.is_empty()
        || p.algorithm.starts_with('-')
        || !p
            .algorithm
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'/'))
    {
        return Err("A name, supported algorithm and public receiving address are required".into());
    }
    validate_pool(&p.pool)?;
    text(&p.gpu_ids, 512, true)?;
    if !p.gpu_ids.trim().is_empty()
        && !p.gpu_ids.eq_ignore_ascii_case("all")
        && !p
            .gpu_ids
            .bytes()
            .all(|b| b.is_ascii_hexdigit() || matches!(b, b':' | b'.' | b',' | b' '))
    {
        return Err("Invalid GPU selector; confirm IDs in the engine console".into());
    }
    validate_tuning(&p)?;
    // Generate arguments for validation only. Never launch the executable here.
    crate::build_engine_args_with_language(&p, false).map_err(|_| {
        "The engine cannot use these profile settings; review the configuration locally"
    })?;
    Ok(p)
}

fn patched_schedules(
    cfg: &AppConfig,
    expected: &str,
    schedules: &[ScheduleWindow],
) -> Result<AppConfig, String> {
    if expected != config_revision(cfg) {
        return Err("Configuration conflict: refresh schedules before saving".into());
    }
    if schedules.len() > 128 {
        return Err("Too many schedules".into());
    }
    let mut ids = HashSet::new();
    let mut result = Vec::new();
    for incoming in schedules {
        valid_id(&incoming.id)?;
        text(&incoming.name, 128, true)?;
        if !ids.insert(&incoming.id) || incoming.name.trim().is_empty() {
            return Err("Invalid or duplicate schedule".into());
        }
        if incoming.enabled && incoming.miner_ids.is_empty() {
            return Err("Enabled schedule must select at least one profile".into());
        }
        let time = |s: &str| -> bool {
            s.len() == 5
                && s.as_bytes()[2] == b':'
                && s[..2].parse::<u8>().is_ok_and(|h| h < 24)
                && s[3..].parse::<u8>().is_ok_and(|m| m < 60)
        };
        if !incoming.start.is_ascii()
            || !incoming.end.is_ascii()
            || !time(&incoming.start)
            || !time(&incoming.end)
            || incoming.start == incoming.end
            || incoming.days.is_empty()
            || incoming.days.len() > 7
            || incoming.days.iter().any(|d| !(1..=7).contains(d))
            || incoming.days.iter().collect::<HashSet<_>>().len() != incoming.days.len()
            || incoming.miner_ids.len() > cfg.miners.len()
            || incoming.miner_ids.iter().collect::<HashSet<_>>().len() != incoming.miner_ids.len()
            || incoming
                .miner_ids
                .iter()
                .any(|id| !cfg.miners.iter().any(|p| p.id == *id))
        {
            return Err("Invalid schedule times, weekdays or assigned profiles".into());
        }
        let mut saved = cfg
            .schedules
            .iter()
            .find(|s| s.id == incoming.id)
            .cloned()
            .unwrap_or_default();
        // Host sleep/wake controls are local-only. New remote windows cannot add
        // them; existing local settings must be echoed unchanged and preserved.
        if incoming.post_action != saved.post_action
            || incoming.countdown_seconds != saved.countdown_seconds
            || incoming.require_confirmation != saved.require_confirmation
            || incoming.wake_enabled != saved.wake_enabled
            || incoming.wake_minutes_before != saved.wake_minutes_before
        {
            return Err(
                "Sleep, wake and power-confirmation settings can only be changed locally".into(),
            );
        }
        saved.id = incoming.id.clone();
        saved.name = incoming.name.clone();
        saved.enabled = incoming.enabled;
        saved.days = incoming.days.clone();
        saved.start = incoming.start.clone();
        saved.end = incoming.end.clone();
        saved.miner_ids = incoming.miner_ids.clone();
        result.push(saved);
    }
    let mut next = cfg.clone();
    next.schedules = result;
    Ok(next)
}

fn redact_pool_userinfo(value: &str) -> String {
    if let Some(at) = value.find('@') {
        let begin = value.find("://").map_or(0, |i| i + 3);
        if at >= begin {
            return format!("{}[redacted]{}", &value[..begin], &value[at..]);
        }
    }
    value.to_string()
}

fn redact(value: &str, cfg: &AppConfig) -> String {
    let lower = value.to_ascii_lowercase();
    // Full command lines/arguments are never relayed. They can contain secrets
    // unknown to MinerDesk (custom arguments, pool login formats, script output).
    if lower.contains("--")
        || lower.contains("authorization:")
        || lower.contains("bearer ")
        || lower.contains("x-minerdesk-token")
        || lower.contains("password")
        || lower.contains("passphrase")
        || lower.contains("mnemonic")
        || lower.contains("seed phrase")
        || lower.contains("private key")
        || lower.contains("token=")
        || lower.contains("api_key")
        || lower.contains("api-key")
    {
        return "[Sensitive command or credential-bearing output withheld]".into();
    }
    let mut result = value.to_string();
    let mut secrets = Vec::new();
    if !cfg.web.token.is_empty() {
        secrets.push(cfg.web.token.clone());
    }
    for p in &cfg.miners {
        secrets.extend([
            p.wallet.clone(),
            p.secondary_wallet.clone(),
            p.password.clone(),
            p.executable_path.clone(),
            p.extra_args.clone(),
        ]);
        if let Some(grant) = &p.linux_root_authorization {
            secrets.extend([grant.id.clone(), grant.fingerprint.clone()]);
        }
        if let Some(at) = p.pool.find('@') {
            secrets.push(p.pool[..at].to_string());
        }
    }
    secrets.sort_by_key(|v| std::cmp::Reverse(v.len()));
    for secret in secrets.into_iter().filter(|s| !s.is_empty()) {
        result = result.replace(&secret, "[redacted]");
    }
    // Suppress arbitrary absolute local paths, including paths not currently in
    // the profile. No miner launch or user profile directory belongs in telemetry.
    if regex::Regex::new(r"(?i)(?:[a-z]:[\\/]|/(?:home|root|usr|var|tmp|opt|etc)/)")
        .unwrap()
        .is_match(&result)
    {
        return "[Local path-bearing output withheld; inspect the rig console]".into();
    }
    safe_label(&result, 2048)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn profile() -> MinerProfile {
        MinerProfile {
            id: "rig-test".into(),
            name: "PRL test".into(),
            algorithm: "pearlhash".into(),
            pool: "pool.example:3360".into(),
            wallet: "public-fixture-address".into(),
            password: "secret-pool-password".into(),
            executable_path: "/private/miner.exe".into(),
            extra_args: "--private-flag local-only-value".into(),
            ..MinerProfile::default()
        }
    }
    fn cfg() -> AppConfig {
        let mut c = AppConfig::default();
        c.miners = vec![profile()];
        c.web.token = "secret-web-token".into();
        c
    }
    #[tokio::test]
    async fn running_local_dashboard_metrics_survive_fleet_snapshot_validation() {
        // Represent an already-running miner without launching any executable.
        // Exercise the actual local status handler as well as the Fleet allowlist.
        let directory = tempfile::tempdir().unwrap();
        let core = CoreState::load_from_path(directory.path().join("config.json")).unwrap();
        *core.config.write().unwrap() = cfg();
        core.runtime.lock().unwrap().insert(
            "rig-test".into(),
            crate::MinerRuntimeView {
                id: "rig-test".into(),
                running: true,
                started_at: Some(crate::unix_now() - 3900),
                started_by: "manual".into(),
                dev_tip_active: true,
                metrics: crate::MinerMetrics {
                    hashrate_hps: Some(78.42e12),
                    power_w: Some(453.5),
                    temperature_c: Some(62.0),
                    fan_pct: Some(71.0),
                    accepted: 4321,
                    rejected: 7,
                },
                ..Default::default()
            },
        );
        let state = crate::WebState {
            core: core.clone(),
            fleet: None,
            token: Some("fixture-access".into()),
            headless: true,
            desktop_owned: false,
            desktop_leases: Arc::new(std::sync::Mutex::new(crate::DesktopLeaseState::default())),
        };
        let mut headers = axum::http::HeaderMap::new();
        headers.insert("x-minerdesk-token", "fixture-access".parse().unwrap());
        let response = crate::api_status(axum::extract::State(state), headers).await;
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .unwrap();
        let local: Value = serde_json::from_slice(&bytes).unwrap();
        let local_miner = &local[0];
        let bridge = FleetCoreBridge::new(core.clone());
        let snapshot = super::super::validation::snapshot(bridge.snapshot_value()).unwrap();
        let fleet_miner = &snapshot["status"][0];
        assert_eq!(fleet_miner["metrics"], local_miner["runtime"]["metrics"]);
        for field in ["running", "dev_tip_active"] {
            assert_eq!(fleet_miner[field], local_miner["runtime"][field]);
        }
        for field in ["id", "name", "engine", "algorithm", "gpu_ids", "enabled"] {
            assert_eq!(fleet_miner[field], local_miner["profile"][field]);
        }
        let local_uptime = local_miner["runtime"]["uptime_seconds"].as_u64().unwrap();
        let fleet_uptime = fleet_miner["uptime_seconds"].as_u64().unwrap();
        assert!(local_uptime >= 3900 && fleet_uptime.abs_diff(local_uptime) <= 1);
        let encoded = snapshot.to_string();
        for private in [
            "secret-pool-password",
            "secret-web-token",
            "/private/miner.exe",
            "local-only-value",
        ] {
            assert!(!encoded.contains(private), "leaked {private}");
        }
        assert!(core.children.lock().unwrap().is_empty());

        // Older agents have the same metrics but no developer-tip display flag.
        let mut old_snapshot = snapshot.clone();
        old_snapshot["status"][0]
            .as_object_mut()
            .unwrap()
            .remove("dev_tip_active");
        let compatible = super::super::validation::snapshot(old_snapshot).unwrap();
        assert_eq!(compatible["status"][0]["dev_tip_active"], false);
        let mut invalid = snapshot;
        invalid["status"][0]["dev_tip_active"] = json!("true");
        assert!(super::super::validation::snapshot(invalid).is_err());
    }
    #[test]
    fn safe_configuration_never_serializes_private_fields() {
        let mut c = cfg();
        c.miners[0].linux_root_authorization = Some(crate::GrantRef {
            id: "private-grant-id".into(),
            fingerprint: "private-grant-fingerprint".into(),
        });
        let s = config_view(&c).to_string();
        for forbidden in [
            "secret-pool-password",
            "secret-web-token",
            "/private/miner.exe",
            "local-only-value",
            "executable_path",
            "extra_args",
            "linux_root_authorization",
            "private-grant-id",
            "private-grant-fingerprint",
        ] {
            assert!(!s.contains(forbidden), "leaked {forbidden}");
        }
        assert!(s.contains("public-fixture-address"));
    }
    #[test]
    fn public_revisions_are_not_an_oracle_for_hidden_pool_credentials() {
        let mut a = profile();
        a.pool = "stratum+tcp://user:one-private-secret@pool.example:3360".into();
        let mut b = a.clone();
        b.pool = "stratum+tcp://user:another-private-secret@pool.example:3360".into();
        assert_eq!(profile_revision(&a), profile_revision(&b));
        let encoded = profile_view(&a).to_string();
        assert!(!encoded.contains("one-private-secret"));
        assert!(!encoded.contains("user:"));
    }
    #[test]
    fn patch_preserves_local_execution_and_rejects_privilege_edits() {
        let mut p = profile();
        p.linux_root_authorization = Some(crate::GrantRef {
            id: "local-only-grant".into(),
            fingerprint: "old-command-fingerprint".into(),
        });
        let r = profile_revision(&p);
        let edited =
            patched_profile(&p, &r, &json!({"worker":"Fleet worker","memory_clock":405})).unwrap();
        assert_eq!(edited.executable_path, p.executable_path);
        assert_eq!(edited.extra_args, p.extra_args);
        assert_eq!(edited.password, p.password);
        assert_eq!(edited.linux_root_authorization, p.linux_root_authorization);
        assert_eq!(edited.memory_clock, Some(405));
        for field in [
            "engine",
            "executable_path",
            "extra_args",
            "password",
            "linux_run_as_root",
            "linux_root_authorization",
            "api_port",
            "dev_tip_percent",
        ] {
            assert!(patched_profile(&p, &r, &json!({field:"not-allowed"})).is_err());
        }
    }
    #[test]
    fn stale_profile_and_schedule_revisions_are_rejected() {
        let p = profile();
        let r = profile_revision(&p);
        let mut changed = p.clone();
        changed.worker = "local-edit".into();
        assert!(patched_profile(&changed, &r, &json!({"memory_clock":405}))
            .unwrap_err()
            .contains("conflict"));
        let c = cfg();
        let old = config_revision(&c);
        let mut changed = c.clone();
        changed.miners[0].enabled = false;
        assert!(patched_schedules(&changed, &old, &[])
            .unwrap_err()
            .contains("conflict"));
    }
    #[test]
    fn invalid_inputs_and_nested_fields_are_rejected() {
        let p = profile();
        let r = profile_revision(&p);
        for patch in [
            json!({"fan":101}),
            json!({"power_limit":2100}),
            json!({"memory_clock":-1}),
            json!({"worker":"bad\nworker"}),
            json!({"pool":"file:///tmp/config"}),
            json!({"pool":"stratum+tcp://user:password@pool.example:3360"}),
            json!({"gpu_tuning":[{"selector":"0","executable_path":"evil"}]}),
        ] {
            assert!(patched_profile(&p, &r, &patch).is_err(), "accepted {patch}");
        }
    }
    #[test]
    fn blank_tuning_is_explicit_and_keeps_root_reference_local() {
        let mut p = profile();
        p.extra_args.clear();
        p.memory_clock = Some(810);
        p.linux_run_as_root = true;
        let changed =
            patched_profile(&p, &profile_revision(&p), &json!({"memory_clock":null})).unwrap();
        assert_eq!(changed.memory_clock, None);
        assert!(changed.linux_run_as_root);
        assert!(changed.linux_root_authorization.is_none());
    }
    #[test]
    fn missing_or_malformed_root_approval_is_rejected_without_a_process() {
        let mut p = profile();
        p.linux_run_as_root = true;
        assert!(validate_root_reference(&p).unwrap_err().contains("locally"));
        p.linux_root_authorization = Some(crate::GrantRef {
            id: "untrusted-id".into(),
            fingerprint: "untrusted-fingerprint".into(),
        });
        assert!(validate_root_reference(&p).is_err());
        p.linux_run_as_root = false;
        assert!(validate_root_reference(&p).is_ok());
    }
    #[test]
    fn expiry_is_checked_when_local_locks_are_acquired() {
        assert!(ensure_not_expired(0).is_err());
        assert!(ensure_not_expired(crate::unix_now()).is_err());
        assert!(ensure_not_expired(crate::unix_now() + 60).is_ok());
    }
    #[test]
    fn logs_and_errors_redact_commands_credentials_and_paths() {
        let c = cfg();
        for line in [
            "miner --wallet private-unknown-value --pass unknown",
            "secret-web-token",
            "secret-pool-password",
            "public-fixture-address",
            "root file /home/alice/private/config.json",
            "Authorization: Bearer unknown-secret",
        ] {
            let redacted = redact(line, &c);
            for forbidden in [
                "private-unknown-value",
                "secret-web-token",
                "secret-pool-password",
                "public-fixture-address",
                "/home/alice",
                "unknown-secret",
            ] {
                assert!(!redacted.contains(forbidden), "leaked in {redacted}");
            }
        }
        assert_eq!(redact("Share accepted 23", &c), "Share accepted 23");
    }
    #[test]
    fn remote_logs_fit_the_receipt_limit_and_keep_recent_lines() {
        let c = cfg();
        let source = (0..300)
            .map(|n| crate::LogLine {
                ts: n,
                stream: "stdout".into(),
                text: "A".repeat(2048),
            })
            .collect();
        let value = log_view("rig-test", source, &c);
        assert!(serde_json::to_vec(&value).unwrap().len() < 8 * 1024);
        assert_eq!(value["truncated"], true);
        assert_eq!(
            value["lines"].as_array().unwrap().last().unwrap()["ts"],
            299
        );
    }
    #[test]
    fn schedules_validate_ids_days_times_and_keep_power_controls_local() {
        let c = cfg();
        let r = config_revision(&c);
        let mut s = ScheduleWindow::default();
        s.id = "solar".into();
        s.miner_ids = vec!["rig-test".into()];
        assert!(patched_schedules(&c, &r, &[s.clone()]).is_ok());
        s.days = vec![0];
        assert!(patched_schedules(&c, &r, &[s.clone()]).is_err());
        s.days = vec![1];
        s.start = "25:00".into();
        assert!(patched_schedules(&c, &r, &[s.clone()]).is_err());
        s.start = "08:00".into();
        s.wake_enabled = true;
        assert!(patched_schedules(&c, &r, &[s.clone()]).is_err());
        s.wake_enabled = false;
        s.post_action = "sleep".into();
        assert!(patched_schedules(&c, &r, &[s]).is_err());
    }
    #[test]
    fn enabled_schedules_require_explicit_profiles_and_all_ids_stay_explicit() {
        let mut c = cfg();
        let mut second = profile();
        second.id = "second-profile".into();
        c.miners.push(second);
        let r = config_revision(&c);
        let mut s = ScheduleWindow::default();
        s.id = "solar".into();
        assert_eq!(
            patched_schedules(&c, &r, &[s.clone()]).unwrap_err(),
            "Enabled schedule must select at least one profile"
        );
        s.enabled = false;
        let disabled = patched_schedules(&c, &r, &[s.clone()]).unwrap();
        assert!(disabled.schedules[0].miner_ids.is_empty());
        s.enabled = true;
        s.miner_ids = c.miners.iter().map(|p| p.id.clone()).collect();
        let explicit_all = patched_schedules(&c, &r, &[s.clone()]).unwrap();
        assert_eq!(explicit_all.schedules[0].miner_ids, s.miner_ids);
    }
}
