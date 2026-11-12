import type { GpuTuning, LogLine, MinerMetrics, ScheduleWindow } from "./types";

export type FleetRequest = <T>(path: string, options?: RequestInit, timeoutMs?: number) => Promise<T>;
export type FleetRootAuthorization = "not_applicable" | "not_requested" | "required" | "approval_saved_unverified";
export type FleetUpdateStage = "queued" | "checking" | "downloading" | "verifying" | "ready" | "stopping" | "installing" | "restarting" | "completed" | "failed" | "cancelled";
export type FleetUpdateStatus = {
  protocol: number; current_version: string; available_version: string | null;
  can_install: boolean; enabled: boolean; reason: string;
  root_required?: boolean; root_enrolled?: boolean;
  job: null | { id: string; expected_version: string; stage: FleetUpdateStage; downloaded: number; total: number | null; error: string | null };
};
export type FleetRolloutItem = { rig_id: string; state: "queued" | "checking" | "installing" | "waiting_for_reconnect" | "updated" | "failed" | "cancelled"; error: string | null };
export type FleetRollout = {
  id: string; expected_version: string;
  state: "running" | "waiting_for_reconnect" | "failed" | "cancelled" | "completed";
  created_at: number; updated_at: number; rigs: FleetRolloutItem[]; error: string | null;
};
export type FleetMiner = {
  id: string; name: string; engine: string; algorithm: string; enabled: boolean; gpu_ids: string;
  running: boolean; mode: "schedule" | "forced" | "waiting" | "manual" | "stopped";
  scheduled_now: boolean; has_schedule: boolean; scheduler_paused: boolean;
  uptime_seconds: number; metrics: MinerMetrics; last_error: string;
  /** Older agents do not report the developer-tip session flag. */
  dev_tip_active?: boolean;
  root_authorization: FleetRootAuthorization;
};
export type FleetSnapshot = {
  health: "online" | "shutting_down"; name: string; os: string; arch: string; version: string;
  local_time: string; timezone: string; utc_offset_seconds: number; revision: string;
  status: FleetMiner[];
  gpu_inventory: { selector: string; name: string; vendor?: string; pci_bus?: string | null }[];
  gpu_inventory_available: boolean;
  /** Absent on agents that need a first local upgrade before remote updates. */
  updates?: FleetUpdateStatus;
};
export type FleetProfile = {
  id: string; revision: string; name: string; engine: string; enabled: boolean; algorithm: string;
  pool: string; wallet: string; secondary_wallet: string; merge_secondary: boolean; merge_separator: string;
  worker: string; gpu_ids: string; gpu_tuning: GpuTuning[];
  core_clock: number | null; memory_clock: number | null; core_offset: number | null;
  memory_offset: number | null; power_limit: number | null; fan: number | null;
  disable_cpu: boolean; allow_gpu_overlap: boolean; linux_run_as_root: boolean;
  root_authorization: FleetRootAuthorization;
};
export type FleetProfilePatch = Partial<Pick<FleetProfile,
  "name" | "enabled" | "algorithm" | "pool" | "wallet" | "secondary_wallet" | "merge_secondary" |
  "merge_separator" | "worker" | "gpu_ids" | "gpu_tuning" | "core_clock" | "memory_clock" |
  "core_offset" | "memory_offset" | "power_limit" | "fan" | "disable_cpu" | "allow_gpu_overlap">>;
export type FleetConfig = {
  revision: string; profiles: FleetProfile[]; schedules: ScheduleWindow[];
  local_time: string; timezone: string; utc_offset_seconds: number; editable_profile_fields: string[];
};
export type FleetRig = {
  rig_id: string; name: string; last_seen: number; online: boolean;
  snapshot: FleetSnapshot | null; config: FleetConfig | null;
  /** A paired agent may not have sent its first complete snapshot/config yet. */
  validation_error?: "snapshot" | "config";
};
export type FleetSetup = {
  rig_id: string; name: string; mode: "standalone" | "hub" | "member";
  hub_url: string | null; fleet_id: string | null; hub_enabled: boolean;
  tls_fingerprint: string | null; error: string | null; suggested_url?: string | null;
};
export type FleetAction = "start" | "stop" | "restart" | "resume" | "force_start";
export type FleetOperation = { action: FleetAction; profile_id: string }
  | { action: "profile_patch"; profile_id: string; expected_revision: string; patch: FleetProfilePatch }
  | { action: "schedules_patch"; expected_revision: string; schedules: ScheduleWindow[] }
  | { action: "logs"; profile_id: string; limit: number }
  | { action: "update_check" }
  | { action: "update_install"; expected_version: string };
export type FleetReceipt = {
  id: string; rig_id: string; created_at: number; expires_at: number;
  state: "queued" | "running" | "succeeded" | "failed" | "expired";
  result: unknown | null; error: string | null;
};
export type FleetView = {
  setup: FleetSetup; rigs: FleetRig[]; commands: FleetReceipt[];
  rollouts: FleetRollout[];
  coordinator_rig_id?: string | null;
  local_setup_allowed: boolean; controller_allowed: boolean;
};
export type FleetInvitation = { address: string; key: string; expires_at: number; certificate_fingerprint: string };
export type FleetDiscovery = { fleet_id: string; name: string; address: string; certificate_fingerprint: string };
export type FleetLogs = { profile_id: string; lines: LogLine[] };

type JsonRecord = Record<string, unknown>;
const record = (value: unknown): value is JsonRecord => !!value && typeof value === "object" && !Array.isArray(value);
const finite = (value: unknown): value is number => typeof value === "number" && Number.isFinite(value);
const nonnegative = (value: unknown): value is number => finite(value) && value >= 0;
const textFields = (value: JsonRecord, fields: readonly string[]) => fields.every(key => typeof value[key] === "string");
const boolFields = (value: JsonRecord, fields: readonly string[]) => fields.every(key => typeof value[key] === "boolean");
const nullableNumbers = (value: JsonRecord, fields: readonly string[]) => fields.every(key => value[key] === null || finite(value[key]));
const rootStates = ["not_applicable", "not_requested", "required", "approval_saved_unverified"];
const enumValue = (value: unknown, values: readonly string[]) => typeof value === "string" && values.includes(value);
function validMetrics(value: unknown): value is MinerMetrics {
  return record(value) && nullableNumbers(value, ["hashrate_hps", "power_w", "temperature_c", "fan_pct"])
    && (value.hashrate_hps === null || nonnegative(value.hashrate_hps))
    && (value.power_w === null || nonnegative(value.power_w))
    && nonnegative(value.accepted) && Number.isInteger(value.accepted)
    && nonnegative(value.rejected) && Number.isInteger(value.rejected);
}
function validMiner(value: unknown): value is FleetMiner {
  return record(value) && textFields(value, ["id", "name", "engine", "algorithm", "gpu_ids", "last_error"])
    && value.id !== "" && boolFields(value, ["enabled", "running", "scheduled_now", "has_schedule", "scheduler_paused"])
    && enumValue(value.mode, ["schedule", "forced", "waiting", "manual", "stopped"])
    && enumValue(value.root_authorization, rootStates) && nonnegative(value.uptime_seconds) && validMetrics(value.metrics)
    && (value.dev_tip_active === undefined || typeof value.dev_tip_active === "boolean");
}
function validSnapshot(value: unknown): value is FleetSnapshot {
  return record(value) && enumValue(value.health, ["online", "shutting_down"])
    && textFields(value, ["name", "os", "arch", "version", "local_time", "timezone", "revision"])
    && value.revision !== "" && finite(value.utc_offset_seconds)
    && Array.isArray(value.status) && value.status.every(validMiner)
    && typeof value.gpu_inventory_available === "boolean" && Array.isArray(value.gpu_inventory)
    && value.gpu_inventory.every(g => record(g) && textFields(g, ["selector", "name"])
      && (g.vendor === undefined || typeof g.vendor === "string")
      && (g.pci_bus === undefined || g.pci_bus === null || typeof g.pci_bus === "string"));
}
function validTuning(value: unknown): value is GpuTuning {
  return record(value) && typeof value.selector === "string"
    && (value.ignore_defaults === undefined || typeof value.ignore_defaults === "boolean")
    && nullableNumbers(value, ["core_clock", "memory_clock", "core_offset", "memory_offset", "power_limit", "fan"]);
}
function validProfile(value: unknown): value is FleetProfile {
  return record(value) && textFields(value, ["id", "revision", "name", "engine", "algorithm", "pool", "wallet",
    "secondary_wallet", "merge_separator", "worker", "gpu_ids"])
    && value.id !== "" && value.revision !== ""
    && boolFields(value, ["enabled", "merge_secondary", "disable_cpu", "allow_gpu_overlap", "linux_run_as_root"])
    && nullableNumbers(value, ["core_clock", "memory_clock", "core_offset", "memory_offset", "power_limit", "fan"])
    && enumValue(value.root_authorization, rootStates) && Array.isArray(value.gpu_tuning) && value.gpu_tuning.every(validTuning);
}
function validSchedule(value: unknown): value is ScheduleWindow {
  return record(value) && textFields(value, ["id", "name", "start", "end"])
    && value.id !== "" && boolFields(value, ["enabled", "require_confirmation", "wake_enabled"])
    && Array.isArray(value.days) && value.days.every(day => finite(day) && Number.isInteger(day) && day >= 1 && day <= 7)
    && Array.isArray(value.miner_ids) && value.miner_ids.every(id => typeof id === "string")
    && enumValue(value.post_action, ["none", "sleep", "hibernate"])
    && nonnegative(value.countdown_seconds) && nonnegative(value.wake_minutes_before);
}
function validConfig(value: unknown): value is FleetConfig {
  return record(value) && textFields(value, ["revision", "local_time", "timezone"])
    && value.revision !== "" && finite(value.utc_offset_seconds)
    && Array.isArray(value.profiles) && value.profiles.every(validProfile)
    && Array.isArray(value.schedules) && value.schedules.every(validSchedule)
    && Array.isArray(value.editable_profile_fields) && value.editable_profile_fields.every(field => typeof field === "string");
}
function validReceipt(value: unknown): value is FleetReceipt {
  return record(value) && textFields(value, ["id", "rig_id"])
    && nonnegative(value.created_at) && nonnegative(value.expires_at)
    && enumValue(value.state, ["queued", "running", "succeeded", "failed", "expired"])
    && (value.error === null || typeof value.error === "string");
}

/** Optional update metadata is decoded separately: a bad extension cannot make
 * an old peer eligible for an installer or erase otherwise valid telemetry. */
export function decodeFleetUpdateStatus(value: unknown): FleetUpdateStatus | null {
  if (!record(value) || !finite(value.protocol) || !textFields(value, ["current_version", "reason"])
    || !(value.available_version === null || typeof value.available_version === "string")
    || !boolFields(value, ["can_install", "enabled"])
    || !["root_required", "root_enrolled"].every(key => value[key] === undefined || typeof value[key] === "boolean")) return null;
  const job = value.job;
  if (job !== null && (!record(job) || !textFields(job, ["id", "expected_version"])
    || !enumValue(job.stage, ["queued", "checking", "downloading", "verifying", "ready", "stopping", "installing", "restarting", "completed", "failed", "cancelled"])
    || !nonnegative(job.downloaded) || !(job.total === null || nonnegative(job.total))
    || !(job.error === null || typeof job.error === "string"))) return null;
  return value as FleetUpdateStatus;
}
function validRollout(value: unknown): value is FleetRollout {
  return record(value) && textFields(value, ["id", "expected_version"]) && value.id !== "" && value.expected_version !== ""
    && enumValue(value.state, ["running", "waiting_for_reconnect", "failed", "cancelled", "completed"])
    && nonnegative(value.created_at) && nonnegative(value.updated_at)
    && (value.error === null || typeof value.error === "string") && Array.isArray(value.rigs)
    && value.rigs.every(item => record(item) && typeof item.rig_id === "string" && item.rig_id !== ""
      && enumValue(item.state, ["queued", "checking", "installing", "waiting_for_reconnect", "updated", "failed", "cancelled"])
      && (item.error === null || typeof item.error === "string"));
}

/** Decode untrusted JSON, including registered agents awaiting their first poll.
 * Incomplete per-rig state never turns into an empty-but-healthy profile list. */
export function decodeFleetView(value: unknown): FleetView {
  if (!record(value) || !record(value.setup) || !Array.isArray(value.rigs)) throw new Error("Invalid Fleet response");
  const setup = value.setup;
  if (!textFields(setup, ["rig_id", "name"]) || !enumValue(setup.mode, ["standalone", "hub", "member"])
    || typeof setup.hub_enabled !== "boolean"
    || !["hub_url", "fleet_id", "tls_fingerprint", "error"].every(key => setup[key] === null || typeof setup[key] === "string")
    || !(setup.suggested_url === undefined || setup.suggested_url === null || typeof setup.suggested_url === "string")) {
    throw new Error("Invalid Fleet setup response");
  }
  const rigs: FleetRig[] = [];
  for (const raw of value.rigs) {
    if (!record(raw) || !textFields(raw, ["rig_id", "name"]) || raw.rig_id === "") continue;
    const snapshot = validSnapshot(raw.snapshot) ? { ...raw.snapshot } : null;
    if (snapshot) {
      const updates = decodeFleetUpdateStatus(snapshot.updates);
      if (updates) snapshot.updates = updates;
      else delete snapshot.updates;
    }
    const config = validConfig(raw.config) ? raw.config : null;
    const validSeen = nonnegative(raw.last_seen);
    rigs.push({ rig_id: raw.rig_id as string, name: raw.name as string,
      last_seen: validSeen ? raw.last_seen as number : 0,
      online: raw.online === true && snapshot !== null && config !== null && validSeen,
      snapshot, config, ...(!snapshot ? { validation_error: "snapshot" as const } : !config ? { validation_error: "config" as const } : {}) });
  }
  return { setup: setup as FleetSetup, rigs,
    commands: Array.isArray(value.commands) ? value.commands.filter(validReceipt) : [],
    rollouts: Array.isArray(value.rollouts) ? value.rollouts.filter(validRollout) : [],
    coordinator_rig_id: typeof value.coordinator_rig_id === "string" ? value.coordinator_rig_id : null,
    local_setup_allowed: value.local_setup_allowed === true, controller_allowed: value.controller_allowed === true };
}

export function decodeFleetLogs(value: unknown): FleetLogs | null {
  if (!record(value) || typeof value.profile_id !== "string" || !Array.isArray(value.lines)
    || !value.lines.every(line => record(line) && nonnegative(line.ts) && textFields(line, ["stream", "text"]))) return null;
  return value as FleetLogs;
}

export function fleetRigFresh(rig: FleetRig, now = Date.now()) {
  const seen = rig.last_seen > 1e11 ? rig.last_seen : rig.last_seen * 1000;
  return rig.online && rig.snapshot?.health === "online" && seen > 0 && now - seen <= 20000;
}

export function fleetUpdatePreflight(rig: FleetRig, expectedVersion?: string): string | null {
  if (!fleetRigFresh(rig)) return "offline";
  const updates = rig.snapshot?.updates;
  if (!updates || updates.protocol !== 1) return "bootstrap";
  if (!updates.enabled) return "disabled";
  if (updates.job && !["completed", "failed", "cancelled"].includes(updates.job.stage)) return "busy";
  if (!updates.available_version) return updates.reason === "supported" ? "check_required" : updates.reason || "check_required";
  if (expectedVersion && updates.available_version !== expectedVersion) return "version_changed";
  if (rig.snapshot?.version === (expectedVersion || updates.available_version)) return "up_to_date";
  if (!updates.can_install) return updates.reason === "supported" ? "unavailable" : updates.reason || "unavailable";
  return null;
}

/** Offline snapshots remain inspectable, but never contribute to current totals. */
export function fleetSummary(rigs: FleetRig[]) {
  const online = rigs.filter(r => fleetRigFresh(r));
  const running = online.filter(r => r.snapshot!.status.some(m => m.running));
  const waiting = online.filter(r => !r.snapshot!.status.some(m => m.running)
    && r.snapshot!.status.some(m => m.mode === "waiting"));
  const totals = new Map<string, number>();
  let power = 0, powerReadings = 0, temperature: number | null = null;
  let activeMiners = 0, accepted = 0, rejected = 0;
  for (const r of online) for (const m of r.snapshot!.status) if (m.running) {
    ++activeMiners;
    accepted += m.metrics.accepted;
    rejected += m.metrics.rejected;
    const algorithm = m.algorithm.trim() || "?";
    if (m.metrics.hashrate_hps !== null && Number.isFinite(m.metrics.hashrate_hps)) {
      totals.set(algorithm, (totals.get(algorithm) || 0) + m.metrics.hashrate_hps);
    }
    if (m.metrics.power_w !== null && Number.isFinite(m.metrics.power_w)) {
      power += m.metrics.power_w; ++powerReadings;
    }
    if (m.metrics.temperature_c !== null && Number.isFinite(m.metrics.temperature_c)) {
      temperature = temperature === null ? m.metrics.temperature_c : Math.max(temperature, m.metrics.temperature_c);
    }
  }
  return { online: online.length, running: running.length, waiting: waiting.length,
    unknown: rigs.length - online.length, power: powerReadings ? power : null, powerReadings,
    activeMiners, temperature, accepted: activeMiners ? accepted : null, rejected: activeMiners ? rejected : null,
    algorithms: Object.fromEntries(totals) };
}
export function fleetHashrate(value: number | null | undefined): string {
  if (value === null || value === undefined || !Number.isFinite(value)) return "—";
  for (const [scale, unit] of [[1e15,"PH/s"],[1e12,"TH/s"],[1e9,"GH/s"],[1e6,"MH/s"],[1e3,"kH/s"]] as const)
    if (value >= scale) return `${(value / scale).toFixed(value / scale >= 100 ? 0 : 2)} ${unit}`;
  return `${value.toFixed(0)} H/s`;
}
export function fleetUptime(seconds: number | null | undefined): string {
  if (seconds === null || seconds === undefined || !Number.isFinite(seconds) || seconds < 0) return "—";
  const whole = Math.floor(seconds);
  return [Math.floor(whole / 3600), Math.floor(whole % 3600 / 60), whole % 60]
    .map(part => String(part).padStart(2, "0")).join(":");
}
export function fleetPreflight(rig: FleetRig, profileId: string, action: FleetAction): string | null {
  if (!fleetRigFresh(rig)) return "offline";
  const miner = rig.snapshot!.status.find(m => m.id === profileId);
  if (!miner) return "missing_profile";
  if (action !== "stop" && action !== "resume" && !miner.enabled) return "disabled";
  if (["start", "restart", "force_start"].includes(action) && miner.root_authorization === "required") return "local_authorization";
  if (action === "resume" && !miner.has_schedule) return "no_schedule";
  return null;
}
export function fleetEditablePatch(config: FleetConfig, before: FleetProfile, after: FleetProfile): FleetProfilePatch {
  const patch: Record<string, unknown> = {};
  // The server advertises an allowlist, and this client has its own fixed allowlist.
  const safe: (keyof FleetProfilePatch)[] = ["name","enabled","algorithm","pool","wallet","secondary_wallet",
    "merge_secondary","merge_separator","worker","gpu_ids","gpu_tuning","core_clock","memory_clock",
    "core_offset","memory_offset","power_limit","fan","disable_cpu","allow_gpu_overlap"];
  for (const field of safe) if (config.editable_profile_fields.includes(field)
    && JSON.stringify(before[field]) !== JSON.stringify(after[field])) patch[field] = after[field];
  return patch as FleetProfilePatch;
}
