export type GpuTuning = {
  selector: string;
  core_clock: number | null;
  power_limit: number | null;
  fan: number | null;
};

export type MinerProfile = {
  id: string;
  name: string;
  engine: string;
  executable_path: string;
  enabled: boolean;
  algorithm: string;
  pool: string;
  wallet: string;
  secondary_wallet: string;
  merge_secondary: boolean;
  merge_separator: string;
  password: string;
  worker: string;
  gpu_ids: string;
  gpu_tuning: GpuTuning[];
  /** Legacy/default values. Per-GPU values override these. */
  core_clock: number | null;
  power_limit: number | null;
  fan: number | null;
  api_port: number | null;
  disable_cpu: boolean;
  extra_args: string;
  allow_gpu_overlap: boolean;
  /** Voluntary MinerDesk developer tip as a percentage of mining time. */
  dev_tip_percent: number;
};

export type ScheduleWindow = {
  id: string;
  name: string;
  enabled: boolean;
  days: number[];
  start: string;
  end: string;
  miner_ids: string[];
  post_action: "none" | "sleep" | "hibernate";
  countdown_seconds: number;
  require_confirmation: boolean;
  wake_enabled: boolean;
  wake_minutes_before: number;
};

export type AppConfig = {
  miners: MinerProfile[];
  schedules: ScheduleWindow[];
  web: {
    desktop_api_port: number;
    expose_lan: boolean;
    token: string;
  };
  ui: {
    language: string;
    stop_backend_on_desktop_exit: boolean;
    start_with_windows: boolean;
    close_to_tray: boolean;
  };
};

export type MinerMetrics = {
  hashrate_hps: number | null;
  power_w: number | null;
  temperature_c: number | null;
  fan_pct: number | null;
  accepted: number;
  rejected: number;
};

export type MinerRuntime = {
  id: string;
  running: boolean;
  pid: number | null;
  started_at: number | null;
  uptime_seconds: number;
  started_by: string;
  last_error: string;
  metrics: MinerMetrics;
  dev_tip_active: boolean;
};

export type MinerStatus = {
  profile: MinerProfile;
  runtime: MinerRuntime;
  scheduled_now: boolean;
  scheduler_paused?: boolean;
};

export type EngineSpec = {
  id: string;
  name: string;
  github_repo: string | null;
  auto_download: boolean;
};

export type InstallResult = {
  engine: string;
  version: string;
  executable_path: string;
  asset_name: string;
  checksum_verified: boolean;
};

export type Health = {
  version: string;
  headless: boolean;
  desktop_owned: boolean;
  miner_crash_guard: boolean;
  config_path: string;
  scheduler: boolean;
  time: string;
};


export type BackendStatus = {
  supported: boolean;
  reachable: boolean;
  port: number;
  task_installed: boolean;
  task_state: string;
  process_running: boolean;
  listener_pid: number | null;
  listener_process: string;
  executable_path: string;
  last_task_result: string;
  log_tail: string;
};

export type LogLine = { ts: number; stream: string; text: string };

export type SecurityStatus = {
  supported: boolean;
  firewall_enabled: boolean;
  defender_exclusion_enabled: boolean;
  miner_root: string;
};

export type GpuDevice = {
  selector: string;
  name: string;
  vendor: string;
  pci_bus: string | null;
};

export type GpuDiscovery = {
  engine: string;
  source: string;
  devices: GpuDevice[];
  raw_excerpt: string;
  selection_hint: string;
};

export type PendingPowerAction = {
  id: string;
  schedule_id: string;
  schedule_name: string;
  action: "sleep" | "hibernate";
  deadline_unix: number;
  countdown_seconds: number;
  require_confirmation: boolean;
};

export type PowerDiagnostics = {
  supported: boolean;
  wake_timers: string;
};
