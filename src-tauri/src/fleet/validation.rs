//! Untrusted telemetry is re-serialized through an explicit display allowlist.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Health {
    Online,
    ShuttingDown,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Mode {
    Schedule,
    Forced,
    Waiting,
    Manual,
    Stopped,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum RootState {
    NotApplicable,
    NotRequested,
    Required,
    ApprovalSavedUnverified,
}
#[derive(Serialize, Deserialize)]
struct Metrics {
    hashrate_hps: Option<f64>,
    power_w: Option<f64>,
    temperature_c: Option<f64>,
    fan_pct: Option<f64>,
    accepted: u64,
    rejected: u64,
}
#[derive(Serialize, Deserialize)]
struct Miner {
    id: String,
    name: String,
    engine: String,
    algorithm: String,
    enabled: bool,
    gpu_ids: String,
    running: bool,
    mode: Mode,
    scheduled_now: bool,
    has_schedule: bool,
    scheduler_paused: bool,
    uptime_seconds: u64,
    #[serde(default)]
    dev_tip_active: bool,
    metrics: Metrics,
    last_error: String,
    root_authorization: RootState,
}
#[derive(Serialize, Deserialize)]
struct Device {
    selector: String,
    name: String,
    vendor: Option<String>,
    pci_bus: Option<String>,
}
#[derive(Serialize, Deserialize)]
struct Snapshot {
    health: Health,
    name: String,
    os: String,
    arch: String,
    version: String,
    local_time: String,
    timezone: String,
    utc_offset_seconds: i32,
    revision: String,
    status: Vec<Miner>,
    gpu_inventory: Vec<Device>,
    gpu_inventory_available: bool,
}
#[derive(Serialize, Deserialize)]
struct Profile {
    id: String,
    revision: String,
    name: String,
    engine: String,
    enabled: bool,
    algorithm: String,
    pool: String,
    wallet: String,
    secondary_wallet: String,
    merge_secondary: bool,
    merge_separator: String,
    worker: String,
    gpu_ids: String,
    gpu_tuning: Vec<crate::GpuTuning>,
    core_clock: Option<u32>,
    memory_clock: Option<u32>,
    core_offset: Option<i32>,
    memory_offset: Option<i32>,
    power_limit: Option<u32>,
    fan: Option<u8>,
    disable_cpu: bool,
    allow_gpu_overlap: bool,
    linux_run_as_root: bool,
    root_authorization: RootState,
}
#[derive(Serialize, Deserialize)]
struct Config {
    revision: String,
    profiles: Vec<Profile>,
    schedules: Vec<crate::ScheduleWindow>,
    local_time: String,
    timezone: String,
    utc_offset_seconds: i32,
    editable_profile_fields: Vec<String>,
}
fn text(value: &str, max: usize) -> bool {
    value.len() <= max && !value.contains('\0')
}
fn ident(value: &str) -> bool {
    !value.is_empty() && text(value, 128)
}
fn clock(local: &str, timezone: &str, offset: i32) -> bool {
    text(local, 80) && text(timezone, 80) && (-86400..=86400).contains(&offset)
}
fn optional_metric(value: Option<f64>) -> bool {
    value.is_none_or(|n| n.is_finite() && n.abs() < 1e30)
}

pub fn snapshot(value: Value) -> Result<Value, String> {
    let parsed: Snapshot =
        serde_json::from_value(value).map_err(|_| "Invalid agent snapshot fields")?;
    if !text(&parsed.name, 128)
        || !text(&parsed.os, 80)
        || !text(&parsed.arch, 80)
        || !text(&parsed.version, 80)
        || !ident(&parsed.revision)
        || !clock(
            &parsed.local_time,
            &parsed.timezone,
            parsed.utc_offset_seconds,
        )
        || parsed.status.len() > 128
        || parsed.gpu_inventory.len() > 128
    {
        return Err("Agent snapshot exceeds display limits".into());
    }
    let mut ids = HashSet::new();
    for miner in &parsed.status {
        if !ident(&miner.id)
            || !ids.insert(&miner.id)
            || !text(&miner.name, 128)
            || !text(&miner.engine, 80)
            || !text(&miner.algorithm, 64)
            || !text(&miner.gpu_ids, 512)
            || !text(&miner.last_error, 2048)
            || ![
                miner.metrics.hashrate_hps,
                miner.metrics.power_w,
                miner.metrics.temperature_c,
                miner.metrics.fan_pct,
            ]
            .into_iter()
            .all(optional_metric)
        {
            return Err("Invalid agent miner telemetry".into());
        }
    }
    for device in &parsed.gpu_inventory {
        if !ident(&device.selector)
            || !text(&device.name, 256)
            || !device.vendor.as_ref().is_none_or(|v| text(v, 80))
            || !device.pci_bus.as_ref().is_none_or(|v| text(v, 80))
        {
            return Err("Invalid GPU inventory".into());
        }
    }
    serde_json::to_value(parsed).map_err(|_| "Cannot encode validated agent snapshot".into())
}
pub fn config(value: Value) -> Result<Value, String> {
    let parsed: Config =
        serde_json::from_value(value).map_err(|_| "Invalid agent configuration fields")?;
    if !ident(&parsed.revision)
        || parsed.profiles.len() > 128
        || parsed.schedules.len() > 128
        || parsed.editable_profile_fields.len() > 32
        || !parsed.editable_profile_fields.iter().all(|s| text(s, 64))
        || !clock(
            &parsed.local_time,
            &parsed.timezone,
            parsed.utc_offset_seconds,
        )
    {
        return Err("Agent configuration exceeds display limits".into());
    }
    let mut ids = HashSet::new();
    for p in &parsed.profiles {
        if !ident(&p.id)
            || !ids.insert(&p.id)
            || !ident(&p.revision)
            || !text(&p.name, 128)
            || !text(&p.engine, 80)
            || !text(&p.algorithm, 64)
            || !text(&p.pool, 512)
            || !text(&p.wallet, 512)
            || !text(&p.secondary_wallet, 512)
            || !text(&p.worker, 256)
            || !text(&p.gpu_ids, 512)
            || !text(&p.merge_separator, 16)
            || p.gpu_tuning.len() > 128
        {
            return Err("Invalid displayed agent profile".into());
        }
    }
    let mut schedules = HashSet::new();
    for s in &parsed.schedules {
        if !ident(&s.id)
            || !schedules.insert(&s.id)
            || !text(&s.name, 128)
            || s.days.len() > 7
            || s.days.iter().any(|d| !(1..=7).contains(d))
            || !text(&s.start, 8)
            || !text(&s.end, 8)
            || s.miner_ids.len() > 128
            || s.miner_ids.iter().any(|id| !ident(id))
            || !matches!(s.post_action.as_str(), "none" | "sleep" | "hibernate")
        {
            return Err("Invalid displayed agent schedule".into());
        }
    }
    serde_json::to_value(parsed).map_err(|_| "Cannot encode validated agent configuration".into())
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn snapshot_value() -> Value {
        json!({"health":"online","name":"rig","os":"linux","arch":"x86_64","version":"fixture","local_time":"2026-11-12T12:00:00Z","timezone":"+00:00","utc_offset_seconds":0,"revision":"1","status":[],"gpu_inventory":[],"gpu_inventory_available":false})
    }
    #[test]
    fn rejects_poisoned_display_shapes_and_strips_undeclared_fields() {
        for bad in [json!(null), json!({"status":null}), json!({"status":{}})] {
            assert!(snapshot(bad).is_err());
        }
        let mut value = snapshot_value();
        value["status"] = json!([{"metrics":"not an object"}]);
        assert!(snapshot(value).is_err());
        let mut value = snapshot_value();
        value["password"] = json!("must not be exported");
        assert!(snapshot(value).unwrap().get("password").is_none());
        assert!(config(json!({"profiles":null})).is_err());
    }
}
