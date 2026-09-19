import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ApiTimeoutError, requestJson } from "./api";
import { BulkStartError, runMiningCommand, type MiningAction } from "./minerCommands";
import { dayNames, languageDirection, LANGUAGES, tr } from "./i18n";
import type {
  AppConfig, BackendStatus, EngineSpec, GpuDiscovery, GpuTuning, Health, InstallResult, LogLine,
  MinerProfile, MinerStatus, PendingPowerAction, ScheduleWindow, SecurityStatus
} from "./types";

type Tab = "dashboard" | "miners" | "schedules" | "console" | "settings";
const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in (window as unknown as Record<string, unknown>);
const DESKTOP_VERSION = "0.7.22";

function resolveApiBase() {
  // Important: Tauri 2 uses an HTTP(S)-looking origin such as
  // http://tauri.localhost on Windows. Treating every http(s) origin as the
  // headless web UI makes /api/* requests hit the Tauri asset protocol, which
  // returns index.html and causes: Unexpected token '<' / <!doctype ...
  if (isTauri) {
    const port = localStorage.getItem("minerdesk.web.port") || "17888";
    return `http://127.0.0.1:${port}`;
  }
  const http = location.protocol === "http:" || location.protocol === "https:";
  if (http) return ""; // browser UI is already served by MinerDesk backend
  const port = localStorage.getItem("minerdesk.web.port") || "17888";
  return `http://127.0.0.1:${port}`;
}
function queryToken() {
  const q = new URLSearchParams(location.search).get("token");
  if (q) localStorage.setItem("minerdesk.web.token", q);
  return q || localStorage.getItem("minerdesk.web.token") || "";
}
async function api<T>(path: string, options: RequestInit = {}, token = "", timeoutMs = 5000): Promise<T> {
  return requestJson<T>(`${resolveApiBase()}${path}`, options, token, timeoutMs);
}
function formatHashrate(hps: number | null) {
  if (hps === null || !Number.isFinite(hps)) return "—";
  const units = [[1e15, "PH/s"], [1e12, "TH/s"], [1e9, "GH/s"], [1e6, "MH/s"], [1e3, "kH/s"]] as const;
  for (const [d, u] of units) if (hps >= d) return `${(hps / d).toFixed(hps / d >= 100 ? 0 : 2)} ${u}`;
  return `${hps.toFixed(0)} H/s`;
}

function detectDevCoin(p: MinerProfile): string | null {
  const wallet = (p.wallet || "").trim().toLowerCase();
  const algo = (p.algorithm || "").trim().toLowerCase();
  const pool = (p.pool || "").trim().toLowerCase();
  if (wallet.startsWith("prl1") || algo.includes("pearl") || pool.includes("pearl")) return "PRL";
  if (wallet.startsWith("0x")) return "EVM";
  if (algo === "alph" || algo === "aleph" || algo.includes("alephium") || pool.includes("alephium")) return "ALPH";
  if (wallet.startsWith("xel:") || algo.includes("xelis") || pool.includes("xelis")) return "XELIS";
  if (wallet.startsWith("kaspa:") || algo === "kaspa" || pool.includes("kaspa")) return "KASPA";
  if (wallet.startsWith("nexa:") || algo.includes("nexa") || pool.includes("nexa")) return "NEXA";
  if (wallet.startsWith("nn") || algo.includes("nirmata") || pool.includes("nirmata")) return "NIRMATA";
  if (algo.includes("autolykos") || algo === "ergo" || pool.includes("ergo")) return "ERGO";
  if (algo.includes("randomx") || algo === "monero" || pool.includes("monero") || pool.includes("xmr")) return "MONERO";
  if (algo.includes("zelhash") || algo === "flux" || pool.includes("flux")) return "FLUX";
  return null;
}
function devTipLevel(v: number) {
  if (v <= 0) return "off";
  if (v < 0.75) return "spark";
  if (v < 2) return "heart";
  if (v < 4) return "rocket";
  return "super";
}
function devTipIcon(v: number) {
  const l = devTipLevel(v);
  return l === "off" ? "♡" : l === "spark" ? "✦" : l === "heart" ? "♥" : l === "rocket" ? "🚀" : "✨";
}

function fmtUptime(s: number) {
  const h = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60), x = s % 60;
  return `${String(h).padStart(2,"0")}:${String(m).padStart(2,"0")}:${String(x).padStart(2,"0")}`;
}
function uid(prefix = "id") {
  return typeof crypto !== "undefined" && crypto.randomUUID ? crypto.randomUUID() : `${prefix}-${Date.now()}-${Math.random().toString(16).slice(2)}`;
}
function newMiner(): MinerProfile {
  return {
    id: uid("miner"), name: "New miner", engine: "srbminer", executable_path: "", enabled: true,
    algorithm: "", pool: "", wallet: "", secondary_wallet: "", merge_secondary: false,
    merge_separator: "+", password: "x", worker: "MinerDesk", gpu_ids: "", gpu_tuning: [],
    core_clock: null, power_limit: null, fan: null, api_port: null, disable_cpu: true,
    extra_args: "", allow_gpu_overlap: false, dev_tip_percent: 0,
  };
}
function newSchedule(): ScheduleWindow {
  return {
    id: uid("schedule"), name: "Mining window", enabled: true, days: [1,2,3,4,5,6,7],
    start: "22:00", end: "07:00", miner_ids: [], post_action: "none", countdown_seconds: 60,
    require_confirmation: true, wake_enabled: false, wake_minutes_before: 5,
  };
}
function randToken() {
  return (crypto.randomUUID?.() || `${Date.now()}${Math.random()}`).replace(/-/g, "");
}
function setBootSplashStatus(text: string) {
  const el = document.getElementById("boot-status");
  if (el) el.textContent = text;
}
function hideBootSplash() {
  const el = document.getElementById("boot-splash");
  if (!el) return;
  el.classList.add("boot-hide");
  window.setTimeout(() => el.remove(), 320);
}

export default function App() {
  const [tab, setTab] = useState<Tab>("dashboard");
  const [token, setToken] = useState(queryToken());
  const [health, setHealth] = useState<Health | null>(null);
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [statuses, setStatuses] = useState<MinerStatus[]>([]);
  const [engines, setEngines] = useState<EngineSpec[]>([]);
  const [selectedMiner, setSelectedMiner] = useState("");
  const [consoleMiner, setConsoleMiner] = useState("");
  const [logs, setLogs] = useState<LogLine[]>([]);
  const [statusText, setStatusText] = useState("Connecting to MinerDesk…");
  const [dirty, setDirty] = useState(false);
  const [filterText, setFilterText] = useState("");
  const [filterEngine, setFilterEngine] = useState("all");
  const [filterState, setFilterState] = useState("all");
  const [security, setSecurity] = useState<SecurityStatus | null>(null);
  const [securityBusy, setSecurityBusy] = useState<"firewall"|"defender"|null>(null);
  const [securityError, setSecurityError] = useState("");
  const [busy, setBusy] = useState(false);
  const [commandError, setCommandError] = useState("");
  const [commandProgress, setCommandProgress] = useState("");
  const commandBusyRef = useRef(false);
  const configRef = useRef<AppConfig | null>(null);
  const lastNativeUi = useRef<{ startup: boolean; tray: boolean } | null>(null);
  const [backendStatus, setBackendStatus] = useState<BackendStatus | null>(null);
  const [backendBusy, setBackendBusy] = useState(false);
  const [gpuDiscovery, setGpuDiscovery] = useState<GpuDiscovery | null>(null);
  const [gpuBusy, setGpuBusy] = useState(false);
  const [pendingPower, setPendingPower] = useState<PendingPowerAction | null>(null);
  const [powerNow, setPowerNow] = useState(Math.floor(Date.now()/1000));
  const logsEnd = useRef<HTMLDivElement>(null);
  const backendPollBusy = useRef(false);
  const backendFailureCount = useRef(0);
  const backendStatusRef = useRef<BackendStatus | null>(null);
  const statusPollBusy = useRef(false);
  const powerPollBusy = useRef(false);
  const logPollBusy = useRef(false);
  const lastLogSignature = useRef("");
  const bootstrapStarted = useRef(false);
  const lastAppliedToken = useRef(token);

  const language = config?.ui?.language || localStorage.getItem("minerdesk.language") || "en";
  const t = (key: string) => tr(language, key);
  const days = dayNames(language);

  useEffect(() => {
    document.documentElement.lang = language;
    document.documentElement.dir = languageDirection(language);
    localStorage.setItem("minerdesk.language", language);
  }, [language]);

  useEffect(() => { backendStatusRef.current = backendStatus; }, [backendStatus]);
  useEffect(() => { configRef.current = config; }, [config]);

  async function refreshBase(hideSplashOnFailure = true): Promise<boolean> {
    try {
      setBootSplashStatus("Connecting to MinerDesk backend…");
      const [h, c, e, s] = await Promise.all([
        api<Health>("/api/health", {}, token), api<AppConfig>("/api/config", {}, token),
        api<EngineSpec[]>("/api/engines", {}, token), api<MinerStatus[]>("/api/status", {}, token),
      ]);
      let loaded = c;
      const legacyRaw = localStorage.getItem("minerdesk.settings.v0.3");
      if (legacyRaw && c.miners.length === 1 && !c.miners[0].wallet) {
        try {
          const old = JSON.parse(legacyRaw) as Record<string, unknown>;
          const m = { ...c.miners[0],
            executable_path: String(old.minerPath || c.miners[0].executable_path),
            algorithm: String(old.algorithm || c.miners[0].algorithm), pool: String(old.pool || c.miners[0].pool),
            wallet: String(old.prlWallet || ""), secondary_wallet: String(old.nockWallet || ""),
            merge_secondary: Boolean(old.mergeNock ?? true), worker: String(old.worker || "MinerDesk"), gpu_ids: String(old.gpuIds || ""),
            core_clock: old.coreClock ? Number(old.coreClock) : c.miners[0].core_clock,
            power_limit: old.powerLimit ? Number(old.powerLimit) : null, fan: old.fan ? Number(old.fan) : null,
            api_port: old.apiPort ? Number(old.apiPort) : c.miners[0].api_port,
            extra_args: String(old.extraArgs || ""), disable_cpu: Boolean(old.disableCpu ?? true), gpu_tuning: [],
          };
          loaded = { ...c, miners: [m] };
          await api("/api/config", { method: "PUT", body: JSON.stringify(loaded) }, token);
          localStorage.setItem("minerdesk.migrated.v0.5", "1");
        } catch { /* optional legacy migration */ }
      }
      localStorage.setItem("minerdesk.web.port", String(loaded.web?.desktop_api_port || 17888));
      if (loaded.web?.token && !localStorage.getItem("minerdesk.web.token")) localStorage.setItem("minerdesk.web.token", loaded.web.token);
      setBootSplashStatus("Loading miners and schedules…");
      setHealth(h); setConfig(loaded); configRef.current = loaded; setEngines(e); setStatuses(s);
      if (!lastNativeUi.current) lastNativeUi.current = {
        startup: loaded.ui.start_with_windows ?? false, tray: loaded.ui.close_to_tray ?? false,
      };
      if (isTauri && h.version !== DESKTOP_VERSION) {
        setStatusText(`Backend version ${h.version} detected; desktop is ${DESKTOP_VERSION}. Restart/reinstall the privileged backend.`);
      } else {
        setStatusText(h.headless ? t("headlessMode") : t("desktopMode"));
      }
      if (!selectedMiner && loaded.miners[0]) setSelectedMiner(loaded.miners[0].id);
      if (!consoleMiner && loaded.miners[0]) setConsoleMiner(loaded.miners[0].id);
      api<SecurityStatus>("/api/security/status", {}, token).then(setSecurity).catch(() => null);
      hideBootSplash();
      return true;
    } catch (e) {
      setStatusText(`Connection failed: ${String(e)}`);
      setBootSplashStatus("Waiting for the privileged backend…");
      if (isTauri) void refreshBackendStatus(false);
      if (hideSplashOnFailure) window.setTimeout(hideBootSplash, 1200);
      return false;
    }
  }

  async function refreshBackendStatus(fullDiagnostics = false) {
    if (!isTauri || backendPollBusy.current) return;
    backendPollBusy.current = true;
    try {
      const command = fullDiagnostics ? "get_backend_diagnostics" : "get_backend_status";
      let next = await invoke<BackendStatus>(command);

      // A busy localhost backend can occasionally miss one very short health probe.
      // Do not flash "offline / task missing" for a single transient miss.
      if (!fullDiagnostics && !next.reachable) {
        backendFailureCount.current += 1;
        const previous = backendStatusRef.current;
        if (previous?.reachable && backendFailureCount.current < 3) return;

        // Confirm a persistent miss with the slower Windows diagnostics before
        // changing the visible state. This also gives accurate task information.
        if (backendFailureCount.current >= 3) {
          try { next = await invoke<BackendStatus>("get_backend_diagnostics"); } catch { /* keep quick result */ }
        }
      } else if (next.reachable) {
        backendFailureCount.current = 0;
      }

      backendStatusRef.current = next;
      setBackendStatus(next);
    } catch { /* diagnostics are optional */ }
    finally { backendPollBusy.current = false; }
  }

  async function runBackendAction(command: "start_privileged_backend" | "restart_privileged_backend" | "repair_privileged_backend") {
    if (!isTauri) return;
    setBackendBusy(true);
    try {
      const next = await invoke<BackendStatus>(command);
      backendFailureCount.current = next.reachable ? 0 : backendFailureCount.current;
      backendStatusRef.current = next;
      setBackendStatus(next);
      setStatusText(next.reachable ? t("backendConnected") : t("backendOffline"));
      if (next.reachable) await refreshBase();
    } catch (e) {
      setStatusText(`${t("backendActionFailed")}: ${String(e)}`);
      await refreshBackendStatus();
    } finally { setBackendBusy(false); }
  }

  useEffect(() => {
    if (bootstrapStarted.current) return;
    bootstrapStarted.current = true;
    let cancelled = false;

    const bootstrap = async () => {
      setBootSplashStatus("Checking privileged backend…");
      if (await refreshBase(false)) return;
      if (cancelled) return;

      if (!isTauri) {
        setBootSplashStatus("MinerDesk backend is unavailable.");
        window.setTimeout(hideBootSplash, 1200);
        return;
      }

      setBootSplashStatus("Starting privileged backend… Approve the Windows prompt if requested.");
      setBackendBusy(true);
      try {
        const next = await invoke<BackendStatus>("start_privileged_backend");
        if (cancelled) return;
        backendFailureCount.current = next.reachable ? 0 : backendFailureCount.current;
        backendStatusRef.current = next;
        setBackendStatus(next);

        if (!next.reachable) throw new Error("The privileged backend did not become reachable.");

        setBootSplashStatus("Privileged backend connected. Loading miners and schedules…");
        for (let attempt = 0; attempt < 12 && !cancelled; attempt += 1) {
          if (await refreshBase(false)) return;
          await new Promise(resolve => window.setTimeout(resolve, 250));
        }
        throw new Error("The backend started but MinerDesk could not load its configuration.");
      } catch (e) {
        if (cancelled) return;
        setStatusText(`${t("backendActionFailed")}: ${String(e)}`);
        setBootSplashStatus("Backend startup failed. Opening MinerDesk diagnostics…");
        await refreshBackendStatus(true);
        window.setTimeout(hideBootSplash, 1400);
      } finally {
        if (!cancelled) setBackendBusy(false);
      }
    };

    void bootstrap();
    return () => { cancelled = true; };
    // Startup deliberately runs once. Token changes are handled separately below.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    if (!bootstrapStarted.current || lastAppliedToken.current === token) return;
    lastAppliedToken.current = token;
    void refreshBase(true);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [token]);
  useEffect(() => {
    if (!isTauri) return;
    void refreshBackendStatus(false);
    const timer = window.setInterval(() => {
      if (document.visibilityState === "visible") void refreshBackendStatus(false);
    }, 10000);
    return () => window.clearInterval(timer);
  }, []);
  useEffect(() => {
    if (isTauri && backendStatus?.reachable && !health) void refreshBase();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [backendStatus?.reachable]);
  useEffect(() => {
    const intervalMs = tab === "dashboard" ? 2500 : tab === "console" ? 5000 : 6000;
    const load = async () => {
      if (statusPollBusy.current || document.visibilityState !== "visible") return;
      statusPollBusy.current = true;
      try { setStatuses(await api<MinerStatus[]>("/api/status", {}, token)); } catch { /* keep last known state */ }
      finally { statusPollBusy.current = false; }
    };
    void load();
    const timer = window.setInterval(() => void load(), intervalMs);
    return () => window.clearInterval(timer);
  }, [token, tab]);
  useEffect(() => {
    const load = async () => {
      // The native Desktop must keep polling while minimized/hidden so a schedule
      // end can restore the window and show the sleep/hibernate confirmation. A
      // regular browser tab keeps the old visibility optimization.
      if (powerPollBusy.current || (!isTauri && document.visibilityState !== "visible")) return;
      powerPollBusy.current = true;
      try {
        const next = await api<PendingPowerAction | null>("/api/power/pending", {}, token);
        setPendingPower(current => {
          if (!current && !next) return current;
          if (current && next && current.id === next.id && current.deadline_unix === next.deadline_unix && current.action === next.action) return current;
          return next;
        });
      } catch { /* keep last pending power action */ }
      finally { powerPollBusy.current = false; }
    };
    void load();
    const timer = window.setInterval(() => void load(), isTauri ? 1000 : 2500);
    return () => window.clearInterval(timer);
  }, [token]);
  useEffect(() => {
    if (!pendingPower) return;
    // Restore/unminimize and focus the native application as soon as a new power
    // confirmation modal appears. This also brings close-to-tray windows back.
    if (isTauri) void invoke("focus_main_window").catch(() => undefined);
    setPowerNow(Math.floor(Date.now()/1000));
    const timer = window.setInterval(() => setPowerNow(Math.floor(Date.now()/1000)), 1000);
    return () => window.clearInterval(timer);
  }, [pendingPower?.id]);
  useEffect(() => {
    if (!pendingPower) return;
    const cancelOnClose = () => {
      const headers: Record<string,string> = {};
      if (token) headers["X-MinerDesk-Token"] = token;
      void fetch(`${resolveApiBase()}/api/power/cancel`, { method: "POST", headers, keepalive: true });
    };
    window.addEventListener("beforeunload", cancelOnClose);
    return () => window.removeEventListener("beforeunload", cancelOnClose);
  }, [pendingPower?.id, token]);
  useEffect(() => {
    if (tab !== "console" || !consoleMiner) return;
    lastLogSignature.current = "";
    const load = async () => {
      if (logPollBusy.current || document.visibilityState !== "visible") return;
      logPollBusy.current = true;
      try {
        const next = await api<LogLine[]>(`/api/miners/${encodeURIComponent(consoleMiner)}/logs?limit=250`, {}, token);
        const last = next[next.length - 1];
        const signature = `${next.length}:${last?.ts ?? 0}:${last?.stream ?? ""}:${last?.text ?? ""}`;
        if (signature !== lastLogSignature.current) {
          lastLogSignature.current = signature;
          setLogs(next);
        }
      } catch { /* keep last console buffer */ }
      finally { logPollBusy.current = false; }
    };
    void load();
    const timer = window.setInterval(() => void load(), 3000);
    return () => window.clearInterval(timer);
  }, [tab, consoleMiner, token]);
  useEffect(() => {
    if (tab === "console") logsEnd.current?.scrollIntoView({ behavior: "auto" });
  }, [logs.length, tab]);
  useEffect(() => {
    if (tab !== "miners" || !selectedMiner) return;
    const timer = window.setTimeout(() => { void loadGpus(selectedMiner); }, 250);
    return () => clearTimeout(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tab, selectedMiner, config?.miners.find(m=>m.id===selectedMiner)?.engine, config?.miners.find(m=>m.id===selectedMiner)?.executable_path]);

  const selected = config?.miners.find(m => m.id === selectedMiner) || null;
  const filtered = useMemo(() => statuses.filter(s => {
    const text = `${s.profile.name} ${s.profile.algorithm} ${s.profile.pool} ${s.profile.gpu_ids}`.toLowerCase();
    const stateMatches = filterState === "all" ||
      (filterState === "running" && s.runtime.running) ||
      (filterState === "stopped" && !s.runtime.running && s.profile.enabled) ||
      (filterState === "disabled" && !s.profile.enabled);
    return (!filterText || text.includes(filterText.toLowerCase())) &&
      (filterEngine === "all" || s.profile.engine === filterEngine) &&
      stateMatches;
  }), [statuses, filterText, filterEngine, filterState]);
  const summary = useMemo(() => {
    const run = filtered.filter(x => x.runtime.running);
    return {
      running: run.length,
      power: run.reduce((a,b)=>a+(b.runtime.metrics.power_w||0),0),
      temp: run.reduce((a,b)=>Math.max(a,b.runtime.metrics.temperature_c||0),0),
      accepted: run.reduce((a,b)=>a+b.runtime.metrics.accepted,0),
      rejected: run.reduce((a,b)=>a+b.runtime.metrics.rejected,0),
    };
  }, [filtered]);

  function mutateMiner(id: string, patch: Partial<MinerProfile>) {
    if (!config) return;
    setConfig({ ...config, miners: config.miners.map(m => m.id === id ? { ...m, ...patch } : m) }); setDirty(true);
  }
  function mutateSchedule(id: string, patch: Partial<ScheduleWindow>) {
    if (!config) return;
    setConfig({ ...config, schedules: config.schedules.map(s => s.id === id ? { ...s, ...patch } : s) }); setDirty(true);
  }
  function mutateGpuTuning(m: MinerProfile, selector: string, patch: Partial<GpuTuning>) {
    const existing = m.gpu_tuning || [];
    const current = existing.find(x => x.selector === selector) || { selector, core_clock: null, power_limit: null, fan: null };
    const next = [...existing.filter(x => x.selector !== selector), { ...current, ...patch }];
    let gpuIds = m.gpu_ids;
    // Per-GPU tuning requires an explicit stable GPU order. If the profile was in
    // "all GPUs" mode, promote the currently discovered devices to an explicit selection.
    if (!gpuIds.trim() && gpuDiscovery?.devices?.length) gpuIds = gpuDiscovery.devices.map(g => g.selector).join(",");
    mutateMiner(m.id, { gpu_tuning: next, gpu_ids: gpuIds });
  }
  function gpuOverride(m: MinerProfile, selector: string): GpuTuning {
    return (m.gpu_tuning || []).find(g => g.selector === selector) || { selector, core_clock: null, power_limit: null, fan: null };
  }
  function describeCommandError(error: unknown): string {
    if (error instanceof ApiTimeoutError) return `${t("commandTimeout")} (${error.timeoutMs / 1000}s · ${error.url})`;
    if (error instanceof BulkStartError) {
      const lines = error.failures.map(failure => {
        const name = config?.miners.find(profile => profile.id === failure.id)?.name || failure.id;
        return `${name}: ${failure.result}`;
      });
      return `${t("bulkStartFailed")} (${error.startedCount} ${t("bulkStartSucceeded")})\n${lines.join("\n")}`;
    }
    return error instanceof Error ? error.message : String(error);
  }

  function syncNativeUiPreferences(next: AppConfig) {
    if (!isTauri) return;
    const previous = lastNativeUi.current;
    const startup = next.ui.start_with_windows ?? false;
    const tray = next.ui.close_to_tray ?? false;
    if (previous?.startup === startup && previous?.tray === tray) return;
    // Windows startup registration is unrelated to miner launch. Apply only
    // changed preferences and never wait for PowerShell before sending Start.
    void (async () => {
      if (previous?.startup !== startup) await invoke("set_start_with_windows", { enabled: startup });
      if (previous?.tray !== tray) await invoke("set_close_to_tray", { enabled: tray });
      lastNativeUi.current = { startup, tray };
    })().catch(error => setCommandError(current =>
      [current, `${t("windowsIntegrationError")}: ${String(error)}`].filter(Boolean).join("\n")));
  }

  async function saveConfig(next = config, manageBusy = true): Promise<boolean> {
    if (!next) {
      if (!manageBusy) throw new Error(t("configUnavailable"));
      return false;
    }
    if (manageBusy) { setBusy(true); setCommandError(""); }
    try {
      // The backend now acknowledges disk persistence without waiting for wake
      // task/firewall synchronization. Keep unsaved edits if persistence fails.
      await api("/api/config", { method: "PUT", body: JSON.stringify(next) }, token, 15000);
      localStorage.setItem("minerdesk.web.port", String(next.web.desktop_api_port || 17888));
      localStorage.setItem("minerdesk.web.token", next.web.token || "");
      localStorage.setItem("minerdesk.language", next.ui.language || "en");
      setToken(next.web.token || token);
      // Edits made while the request was in flight are not silently discarded.
      setDirty(JSON.stringify(configRef.current) !== JSON.stringify(next));
      setStatusText(t("saveOk"));
      syncNativeUiPreferences(next);
      return true;
    } catch (error) {
      const message = `${t("save")}: ${describeCommandError(error)}`;
      setStatusText(message);
      setCommandError(message);
      if (!manageBusy) throw error;
      return false;
    } finally { if (manageBusy) setBusy(false); }
  }
  async function changeSecurity(kind: "firewall"|"defender", enabled: boolean) {
    setSecurityBusy(kind);
    setSecurityError("");
    try {
      const next = await api<SecurityStatus>(`/api/security/${kind}`, { method: "POST", body: JSON.stringify({ enabled }) }, token);
      setSecurity(next);
      setStatusText(`${kind === "defender" ? "Microsoft Defender" : "Windows Firewall"}: ${enabled ? "enabled" : "disabled"}`);
    } catch (e) {
      const msg = String(e);
      setSecurityError(msg);
      setStatusText(`Security: ${msg}`);
    } finally {
      setSecurityBusy(null);
    }
  }
  async function performMiningAction(action: MiningAction, id?: string) {
    // A ref closes the double-click gap before React renders disabled buttons.
    if (commandBusyRef.current || busy) return;
    commandBusyRef.current = true;
    setBusy(true);
    setCommandError("");
    try {
      const outcome = await runMiningCommand({
        action, id, dirty, token, request: api,
        save: async () => { await saveConfig(config, false); },
        onStage: stage => setCommandProgress(stage === "saving" ? t("savingBeforeStart") : `${t("commandInProgress")}: ${action}`),
      });
      setStatusText(action === "start-all" && outcome.startedCount === 0
        ? t("noEnabledProfiles") : `${t("commandSent")}: ${action}`);
    } catch (error) {
      const message = describeCommandError(error);
      setCommandError(message);
      setStatusText(message);
    } finally {
      commandBusyRef.current = false;
      setBusy(false);
      setCommandProgress("");
      // A refresh failure must not turn a successful Start into "Start failed".
      // Likewise, a poll must not clear an actionable command error banner.
      void api<MinerStatus[]>("/api/status", {}, token).then(setStatuses).catch(() => undefined);
    }
  }
  async function minerAction(id: string, action: "start"|"stop"|"restart") {
    await performMiningAction(action, id);
  }
  async function allAction(action: "start-all"|"stop-all") {
    await performMiningAction(action);
  }
  async function download(engine: string, minerId: string) {
    setBusy(true); setStatusText(`${t("download")}: ${engine}…`);
    try {
      const r = await api<InstallResult>(`/api/engines/${engine}/download`, { method:"POST" }, token);
      mutateMiner(minerId, { executable_path: r.executable_path }); setStatusText(`${engine} ${r.version} installed`);
    } catch(e){setStatusText(`${t("download")}: ${String(e)}`);} finally{setBusy(false);}
  }
  async function pickExecutable(minerId: string) {
    if (!isTauri) return;
    try { const p = await invoke<string | null>("pick_miner_file"); if (p) mutateMiner(minerId, { executable_path:p }); }
    catch(e){setStatusText(String(e));}
  }
  async function loadGpus(minerId: string) {
    const profile = config?.miners.find(m=>m.id===minerId); if (!profile) return;
    setGpuBusy(true);
    try { setGpuDiscovery(await api<GpuDiscovery>("/api/gpus/discover", { method:"POST", body:JSON.stringify(profile) }, token)); }
    catch (e) { setGpuDiscovery(null); setStatusText(`${t("gpuDetection")}: ${String(e)}`); }
    finally { setGpuBusy(false); }
  }
  function selectedGpuTokens(m: MinerProfile) { return m.gpu_ids.split(/[;,\s]+/).map(x=>x.trim()).filter(Boolean); }
  function toggleGpu(m: MinerProfile, selector: string, checked: boolean) {
    const current = selectedGpuTokens(m);
    const next = checked ? Array.from(new Set([...current, selector])) : current.filter(x=>x!==selector);
    if (!checked && current.length === 1) { setStatusText(t("keepGpu")); return; }
    mutateMiner(m.id, { gpu_ids: next.join(",") });
  }
  function addMiner() {
    if (!config) return; const m = newMiner(); setConfig({ ...config, miners:[...config.miners,m] });
    setSelectedMiner(m.id); setDirty(true); setTab("miners");
  }
  function duplicateMiner(m: MinerProfile) {
    if (!config) return; const n={...m,id:uid("miner"),name:`${m.name} copy`,enabled:false,gpu_tuning:[...(m.gpu_tuning||[])]};
    setConfig({...config,miners:[...config.miners,n]});setSelectedMiner(n.id);setDirty(true);
  }
  function removeMiner(id: string) {
    if (!config || !confirm(t("confirmDelete"))) return;
    setConfig({...config,miners:config.miners.filter(m=>m.id!==id),schedules:config.schedules.map(s=>({...s,miner_ids:s.miner_ids.filter(x=>x!==id)}))});
    setSelectedMiner(config.miners.find(m=>m.id!==id)?.id||""); setDirty(true);
  }

  async function cancelPendingPower() {
    await api("/api/power/cancel", { method: "POST" }, token).catch(()=>null);
    setPendingPower(null);
  }
  async function confirmPendingPower() {
    await api("/api/power/confirm", { method: "POST" }, token).catch(e=>setStatusText(String(e)));
    setPendingPower(null);
  }
  const powerSecondsLeft = pendingPower ? Math.max(0, pendingPower.deadline_unix - powerNow) : 0;

  const title = tab === "dashboard" ? t("dashboardTitle") : tab === "miners" ? t("minersTitle") : tab === "schedules" ? t("schedulesTitle") : tab === "console" ? t("console") : t("settingsTitle");

  return <div className="app-shell">
    <aside className="sidebar">
      <div className="brand"><div className="brand-mark">M</div><div><strong>MinerDesk</strong><span>v0.7.22 · multi-miner</span></div></div>
      <nav>{(["dashboard","miners","schedules","console","settings"] as Tab[]).map(x=><button key={x} className={tab===x?"nav-active":""} onClick={()=>setTab(x)}><span className="nav-dot"/>{t(x)}</button>)}</nav>
      <div className="sidebar-foot"><div className={`status-pill ${summary.running?"on":"off"}`}><span/>{summary.running} {t("activeMiners").toUpperCase()}</div><div className={`status-pill ${backendStatus?.reachable?"on":"off"}`}><span/>{backendStatus?.reachable?t("backendOnline").toUpperCase():t("backendOffline").toUpperCase()}</div><small>{health?.headless?(health.desktop_owned?"HEADLESS / DESKTOP":"HEADLESS / STANDALONE"):"TAURI DESKTOP"}</small></div>
    </aside>
    <main className={tab==="console"?"console-main":""}>
      <header className="topbar"><div><h1>{title}</h1><p>{statusText}{dirty?` · ${t("unsaved")}`:""}</p></div><div className="top-actions">{dirty&&<button className="btn primary" onClick={()=>saveConfig()} disabled={busy}>{t("save")}</button>}<button className="btn ghost" onClick={()=>allAction("start-all")} disabled={busy||!health}>{t("startAll")}</button><button className="btn danger" onClick={()=>allAction("stop-all")} disabled={busy||!health}>{t("stopAll")}</button></div></header>

      {commandProgress && <div className="command-progress" role="status">{commandProgress}</div>}
      {commandError && <div className="command-error" role="alert">
        <div><strong>{t("commandNeedsAttention")}</strong><pre>{commandError}</pre></div>
        <button className="mini-btn" onClick={() => setCommandError("")}>{t("dismissError")}</button>
      </div>}

      {isTauri&&backendStatus&&<div className={`backend-strip ${backendStatus.reachable?"online":"offline"}`}>
        <div className="backend-main"><span className="backend-dot"/><div><strong>{t("privilegedBackend")}</strong><small>{backendStatus.reachable?`${t("backendConnected")} · 127.0.0.1:${backendStatus.port}`:`${t("backendOffline")} · ${backendStatus.task_state==="Unknown"?t("backendChecking"):(backendStatus.task_installed?t("taskInstalled"):t("taskMissing"))}`}{backendStatus.listener_process&&!backendStatus.reachable?` · ${t("portUsedBy")} ${backendStatus.listener_process}`:""}</small></div></div>
        <div className="backend-actions">
          {!backendStatus.reachable&&<button className="mini-btn primary-lite" disabled={backendBusy} onClick={()=>void runBackendAction("start_privileged_backend")}>{backendBusy?t("startingBackend"):t("startBackend")}</button>}
          {backendStatus.reachable&&<button className="mini-btn" disabled={backendBusy} onClick={()=>void runBackendAction("restart_privileged_backend")}>{t("restartBackend")}</button>}
          <button className="mini-btn" disabled={backendBusy} onClick={()=>void runBackendAction("repair_privileged_backend")}>{t("repairBackend")}</button>
          <button className="mini-btn" disabled={backendBusy} onClick={()=>void refreshBackendStatus(true)}>{t("refresh")}</button>
        </div>
        {!backendStatus.reachable&&<details className="backend-details"><summary>{t("backendDiagnostics")}</summary><div className="backend-diag-grid"><span>{t("taskState")}</span><code>{backendStatus.task_state||"—"}</code><span>{t("lastTaskResult")}</span><code>{backendStatus.last_task_result||"—"}</code><span>{t("backendExecutable")}</span><code>{backendStatus.executable_path||"—"}</code><span>{t("listener")}</span><code>{backendStatus.listener_pid?`${backendStatus.listener_process||"process"} · PID ${backendStatus.listener_pid}`:"—"}</code></div>{backendStatus.log_tail&&<pre className="backend-log">{backendStatus.log_tail}</pre>}</details>}
      </div>}

      {tab==="dashboard"&&<section className="page">
        <div className="metric-grid"><Metric title={t("activeMiners")} value={`${summary.running} / ${filtered.length}`} accent/><Metric title={t("totalPower")} value={`${summary.power.toFixed(0)} W`}/><Metric title={t("maxTemp")} value={summary.temp?`${summary.temp.toFixed(0)} °C`:"—"}/><Metric title={t("sharesAR")} value={`${summary.accepted} / ${summary.rejected}`}/></div>
        <div className="card filterbar"><input placeholder={t("filterPlaceholder")} value={filterText} onChange={e=>setFilterText(e.target.value)}/><select value={filterEngine} onChange={e=>setFilterEngine(e.target.value)}><option value="all">{t("allMiners")}</option>{engines.map(e=><option key={e.id} value={e.id}>{e.name}</option>)}</select><select value={filterState} onChange={e=>setFilterState(e.target.value)}><option value="all">{t("allStates")}</option><option value="running">{t("running")}</option><option value="stopped">{t("stopped")}</option><option value="disabled">{t("disabled")}</option></select></div>
        <div className="card table-card"><div className="table-wrap"><table><thead><tr><th>{t("profile")}</th><th>{t("engine")}</th><th>{t("algoGpu")}</th><th>{t("state")}</th><th>{t("hashrate")}</th><th>W</th><th>°C</th><th>A/R</th><th>{t("uptime")}</th><th/></tr></thead><tbody>{filtered.map(s=><tr key={s.profile.id}><td><strong>{s.profile.name}</strong><small>{s.profile.pool}</small>{s.runtime.last_error&&<small className="miner-runtime-error">{s.runtime.last_error}</small>}</td><td>{engines.find(e=>e.id===s.profile.engine)?.name||s.profile.engine}</td><td>{s.profile.algorithm||"—"}<small>GPU {s.profile.gpu_ids||t("allGpus")}</small></td><td>{s.runtime.running?<span className="state run">● {t("mining")}</span>:!s.profile.enabled?<><span className="state disabled">○ {t("disabled").toUpperCase()}</span><small className="disabled-note">{t("minerDisabledHint")}</small></>:<span className="state stop">○ {t("stop").toUpperCase()}</span>}{s.runtime.running&&!s.profile.enabled&&<small className="disabled-note">{t("minerDisabledHint")}</small>}{s.runtime.dev_tip_active&&<small className="dev-tip-active-badge">♥ {t("devTipActive")}</small>}{s.scheduler_paused?<small className="scheduler-paused" title={t("schedulerPausedHelp")}>⏸ {t("schedulerPaused")}</small>:s.scheduled_now&&<small className="scheduled">⏱ {t("scheduled")}</small>}</td><td>{formatHashrate(s.runtime.metrics.hashrate_hps)}</td><td>{s.runtime.metrics.power_w?.toFixed(0)||"—"}</td><td>{s.runtime.metrics.temperature_c?.toFixed(0)||"—"}</td><td>{s.runtime.metrics.accepted}/{s.runtime.metrics.rejected}</td><td>{fmtUptime(s.runtime.uptime_seconds)}</td><td className="row-actions"><div className="row-actions-inner">{s.runtime.running?<button disabled={busy||!health} onClick={()=>minerAction(s.profile.id,"stop")}>{t("stop")}</button>:<button disabled={busy||!health||!s.profile.enabled} title={!s.profile.enabled?t("minerDisabledHint"):undefined} onClick={()=>minerAction(s.profile.id,"start")}>{t("start")}</button>}<button onClick={()=>{setSelectedMiner(s.profile.id);setTab("miners")}}>{t("config")}</button></div></td></tr>)}</tbody></table></div></div>
      </section>}

      {tab==="miners"&&config&&<section className="page miner-layout">
        <div className="card miner-list"><div className="card-head"><div><h2>{t("profiles")}</h2><p>{t("profilesHelp")}</p></div><button className="mini-btn" onClick={addMiner}>+ {t("add")}</button></div>{config.miners.map(m=><button key={m.id} className={selectedMiner===m.id?"miner-item active":"miner-item"} onClick={()=>setSelectedMiner(m.id)}><span className={`dot ${statuses.find(s=>s.profile.id===m.id)?.runtime.running?"on":""}`}/><div><strong>{m.name}</strong><small>{m.engine} · GPU {m.gpu_ids||t("allGpus")}</small></div></button>)}</div>
        {selected&&<div className="card miner-editor"><div className="card-head"><div><h2>{selected.name}</h2><p>{t("profileConfigHelp")}</p></div><div className="inline-actions"><button className="mini-btn" onClick={()=>duplicateMiner(selected)}>{t("duplicate")}</button><button className="mini-btn danger-lite" onClick={()=>removeMiner(selected.id)}>{t("remove")}</button></div></div>
          <div className="form-grid three"><Field label={t("name")} value={selected.name} onChange={v=>mutateMiner(selected.id,{name:v})}/><SelectField label={t("engine")} value={selected.engine} onChange={v=>mutateMiner(selected.id,{engine:v})} options={engines.map(e=>[e.id,e.name])}/><label className="switch field-check"><input type="checkbox" checked={selected.enabled} onChange={e=>mutateMiner(selected.id,{enabled:e.target.checked})}/> {t("enabled")}</label></div>
          <div className="form-grid path-grid"><Field label={t("executable")} value={selected.executable_path} onChange={v=>mutateMiner(selected.id,{executable_path:v})} wide/><button className="mini-btn" onClick={()=>pickExecutable(selected.id)} disabled={!isTauri}>{t("browse")}</button>{engines.find(e=>e.id===selected.engine)?.auto_download&&<button className="mini-btn primary-lite" onClick={()=>download(selected.engine,selected.id)} disabled={busy}>{t("downloadGithub")}</button>}</div>
          <div className="form-grid three"><Field label={t("algorithm")} value={selected.algorithm} onChange={v=>mutateMiner(selected.id,{algorithm:v})}/><Field label={t("pool")} value={selected.pool} onChange={v=>mutateMiner(selected.id,{pool:v})}/><Field label={t("wallet")} value={selected.wallet} onChange={v=>mutateMiner(selected.id,{wallet:v})}/><Field label={t("secondaryWallet")} value={selected.secondary_wallet} onChange={v=>mutateMiner(selected.id,{secondary_wallet:v})}/><Field label={t("mergeSeparator")} value={selected.merge_separator} onChange={v=>mutateMiner(selected.id,{merge_separator:v})}/><label className="check wide"><input type="checkbox" checked={selected.merge_secondary} onChange={e=>mutateMiner(selected.id,{merge_secondary:e.target.checked})}/> {t("mergeSecondary")}</label><Field label={t("poolPassword")} value={selected.password} onChange={v=>mutateMiner(selected.id,{password:v})}/><Field label={t("worker")} value={selected.worker} onChange={v=>mutateMiner(selected.id,{worker:v})}/></div>

          {(()=>{const tip=Math.min(10,Math.max(0,Number(selected.dev_tip_percent??0)||0));const level=devTipLevel(tip);const coin=detectDevCoin(selected);return <div className={`dev-tip-card level-${level}`}>
            <div className="dev-tip-visual" aria-hidden="true"><div className="dev-tip-orb"><span>{devTipIcon(tip)}</span><i className="spark s1"/><i className="spark s2"/><i className="spark s3"/></div></div>
            <div className="dev-tip-copy"><div className="dev-tip-kicker">{t("devTipKicker")}</div><h3>{t("devTipTitle")}</h3><p>{t("devTipHelp")}</p><div className={`dev-tip-network ${coin?"ok":"unknown"}`}>{coin?`${t("devTipDetected")}: ${coin}${coin==="PRL"&&selected.merge_secondary&&selected.secondary_wallet.trim()?" + NOCK":""}`:t("devTipUnsupported")}</div></div>
            <label className="dev-tip-input"><span>{t("devTipPercent")}</span><div><input type="number" min="0" max="10" step="0.1" value={tip} onChange={e=>{const v=Math.min(10,Math.max(0,Number(e.target.value)||0));mutateMiner(selected.id,{dev_tip_percent:v})}}/><b>%</b></div><small>{tip===0?t("devTipOff"):tip<1?t("devTipThanksSmall"):tip<3?t("devTipThanks"):t("devTipThanksBig")}</small></label>
          </div>})()}

          <div className="section-title">{t("gpuTuning")}</div>
          <div className="gpu-picker">
            <div className="gpu-picker-head"><div><strong>{t("gpuUse")}</strong><span>{gpuDiscovery?.selection_hint || t("gpuUseHelp")}</span></div><button className="mini-btn" disabled={gpuBusy} onClick={()=>loadGpus(selected.id)}>{gpuBusy?t("detecting"):t("refresh")}</button></div>
            <label className="gpu-choice all"><input type="checkbox" checked={!selected.gpu_ids.trim()} onChange={e=>{ if(e.target.checked) mutateMiner(selected.id,{gpu_ids:""}); }}/><span><strong>{t("allGpus")}</strong><small>{t("allGpusHelp")}</small></span></label>
            {gpuDiscovery?.devices?.length ? <div className="gpu-options">{gpuDiscovery.devices.map(g=>{const checked=selectedGpuTokens(selected).includes(g.selector);return <label className={`gpu-choice ${checked?"selected":""}`} key={`${g.selector}-${g.name}`}><input type="checkbox" checked={checked} onChange={e=>toggleGpu(selected,g.selector,e.target.checked)}/><span><strong>GPU {g.selector} · {g.name}</strong><small>{g.vendor}{g.pci_bus?` · PCI ${g.pci_bus}`:""}</small></span></label>})}</div> : <div className="gpu-empty">{gpuBusy?t("detecting"):t("noGpu")}</div>}
            <details className="gpu-manual"><summary>{t("manualIds")}</summary><div className="gpu-manual-body"><Field label={t("savedGpuIds")} value={selected.gpu_ids} onChange={v=>mutateMiner(selected.id,{gpu_ids:v})} placeholder="0,1 or 1:0,3:0"/><p>{t("source")}: {gpuDiscovery?.source||"—"}. SRBMiner/lolMiner/Rigel/NPMiner use indexed selectors; BzMiner may use PCI selectors. lpminer uses system detection unless you provide a supported selector in Advanced arguments.</p>{gpuDiscovery?.raw_excerpt&&<pre>{gpuDiscovery.raw_excerpt}</pre>}</div></details>
          </div>

          <div className="per-gpu-box"><div className="gpu-picker-head"><div><strong>{t("perGpuTuning")}</strong><span>{t("perGpuHelp")}</span></div></div>
            <div className="per-gpu-table"><div className="per-gpu-row header"><span>GPU</span><span>{t("coreClock")}</span><span>{t("powerLimit")}</span><span>{t("fan")}</span></div>
            {(gpuDiscovery?.devices || []).filter(g=>!selected.gpu_ids.trim() || selectedGpuTokens(selected).includes(g.selector)).map(g=>{const tv=gpuOverride(selected,g.selector);return <div className="per-gpu-row" key={`tune-${g.selector}`}><div><strong>{g.name}</strong><small>GPU {g.selector}</small></div><MiniNum value={tv.core_clock} placeholder={selected.core_clock?.toString()||"—"} onChange={v=>mutateGpuTuning(selected,g.selector,{core_clock:v})}/><MiniNum value={tv.power_limit} placeholder={selected.power_limit?.toString()||"—"} onChange={v=>mutateGpuTuning(selected,g.selector,{power_limit:v})}/><MiniNum value={tv.fan} placeholder={selected.fan?.toString()||"—"} onChange={v=>mutateGpuTuning(selected,g.selector,{fan:v})}/></div>})}</div>
          </div>

          <details className="legacy-defaults"><summary>Default / legacy GPU tuning</summary><p>These values are used as fallbacks for old profiles or GPUs without an explicit override.</p><div className="form-grid three"><NumField label={t("coreClock")} value={selected.core_clock} onChange={v=>mutateMiner(selected.id,{core_clock:v})}/><NumField label={t("powerLimit")} value={selected.power_limit} onChange={v=>mutateMiner(selected.id,{power_limit:v})}/><NumField label={t("fan")} value={selected.fan} onChange={v=>mutateMiner(selected.id,{fan:v})}/></div></details>
          <div className="form-grid three"><NumField label={t("minerApiPort")} value={selected.api_port} onChange={v=>mutateMiner(selected.id,{api_port:v})}/><label className="check field-check"><input type="checkbox" checked={selected.disable_cpu} onChange={e=>mutateMiner(selected.id,{disable_cpu:e.target.checked})}/> {t("disableCpu")}</label></div><label className="check"><input type="checkbox" checked={selected.allow_gpu_overlap} onChange={e=>mutateMiner(selected.id,{allow_gpu_overlap:e.target.checked})}/> {t("gpuOverlap")}</label>
          <div className="section-title">{t("advancedArgs")}</div><textarea value={selected.extra_args} onChange={e=>mutateMiner(selected.id,{extra_args:e.target.value})} placeholder={t("advancedPlaceholder")}/>
        </div>}
      </section>}

      {tab==="schedules"&&config&&<section className="page form-page"><div className="card"><div className="card-head"><div><h2>{t("scheduler")}</h2><p>{t("schedulerHelp")}</p></div><button className="mini-btn" onClick={()=>{const s=newSchedule();setConfig({...config,schedules:[...config.schedules,s]});setDirty(true)}}>+ {t("addWindow")}</button></div>
        {config.schedules.length===0&&<div className="empty">{t("noSchedule")}</div>}
        {config.schedules.map(s=><div key={s.id} className="schedule">
          <div className="schedule-head"><input className="schedule-name" value={s.name} onChange={e=>mutateSchedule(s.id,{name:e.target.value})}/><label className="switch"><input type="checkbox" checked={s.enabled} onChange={e=>mutateSchedule(s.id,{enabled:e.target.checked})}/> {t("active")}</label><button className="mini-btn danger-lite" onClick={()=>{setConfig({...config,schedules:config.schedules.filter(x=>x.id!==s.id)});setDirty(true)}}>{t("delete")}</button></div>
          <div className="days">{days.map((d,i)=><button key={`${d}-${i}`} className={s.days.includes(i+1)?"day on":"day"} onClick={()=>mutateSchedule(s.id,{days:s.days.includes(i+1)?s.days.filter(x=>x!==i+1):[...s.days,i+1].sort()})}>{d}</button>)}</div>
          <div className="schedule-grid"><label><span>{t("startTime")}</span><input type="time" value={s.start} onChange={e=>mutateSchedule(s.id,{start:e.target.value})}/></label><label><span>{t("endTime")}</span><input type="time" value={s.end} onChange={e=>mutateSchedule(s.id,{end:e.target.value})}/></label><div className="schedule-miners"><span>{t("profilesAffected")}</span>{config.miners.map(m=><label key={m.id} className="check"><input type="checkbox" checked={s.miner_ids.includes(m.id)} onChange={e=>mutateSchedule(s.id,{miner_ids:e.target.checked?[...s.miner_ids,m.id]:s.miner_ids.filter(x=>x!==m.id)})}/>{m.name}</label>)}</div></div>
          <div className="schedule-power-grid">
            <label className="field"><span>{t("afterMining")}</span><select value={s.post_action||"none"} onChange={e=>mutateSchedule(s.id,{post_action:e.target.value as ScheduleWindow["post_action"]})}><option value="none">{t("doNothing")}</option><option value="sleep">{t("sleepPc")}</option><option value="hibernate">{t("hibernatePc")}</option></select></label>
            <NumField label={t("countdownSeconds")} value={s.countdown_seconds ?? 60} onChange={v=>mutateSchedule(s.id,{countdown_seconds:Math.max(5,v||60)})}/>
            <label className="check field-check"><input type="checkbox" checked={s.require_confirmation ?? true} onChange={e=>mutateSchedule(s.id,{require_confirmation:e.target.checked})}/> {t("requirePowerConfirmation")}</label>
            <label className="check field-check"><input type="checkbox" checked={s.wake_enabled ?? false} onChange={e=>mutateSchedule(s.id,{wake_enabled:e.target.checked})}/> {t("wakeBeforeStart")}</label>
            {s.wake_enabled&&<NumField label={t("wakeMinutesBefore")} value={s.wake_minutes_before ?? 5} onChange={v=>mutateSchedule(s.id,{wake_minutes_before:Math.max(0,v||0)})}/>}
          </div>
          <p className="muted">{t("crossesMidnight")} {t("wakeWindowsNote")}</p>
        </div>)}
      </div></section>}

      {tab==="console"&&<section className="page console-page"><div className="console-toolbar"><select value={consoleMiner} onChange={e=>setConsoleMiner(e.target.value)}>{config?.miners.map(m=><option key={m.id} value={m.id}>{m.name}</option>)}</select><span>{logs.length} {t("lines")}</span></div><pre className="console">{logs.map((l,i)=><div key={`${l.ts}-${i}`} className={l.stream==="stderr"?"log-err":l.stream==="system"?"log-system":""}>{new Date(l.ts*1000).toLocaleTimeString()} {l.text}</div>)}<div ref={logsEnd}/></pre></section>}

      {tab==="settings"&&config&&<section className="page form-page">
        <div className="card"><div className="card-head"><div><h2>{t("language")}</h2><p>{t("languageHelp")}</p></div></div><SelectField label={t("language")} value={config.ui.language||"en"} onChange={v=>{setConfig({...config,ui:{...config.ui,language:v}});setDirty(true)}} options={LANGUAGES.map(([v,l])=>[v,l])}/></div>

        {isTauri&&<div className="card"><div className="card-head"><div><h2>{t("windowsBehavior")}</h2><p>{t("windowsBehaviorHelp")}</p></div></div>
          <div className="windows-option-list">
            <label className="windows-option"><input type="checkbox" checked={config.ui.start_with_windows ?? false} onChange={e=>{setConfig({...config,ui:{...config.ui,start_with_windows:e.target.checked}});setDirty(true)}}/><span><strong>{t("startWithWindows")}</strong><small>{t("startWithWindowsHelp")}</small></span></label>
            <label className="windows-option"><input type="checkbox" checked={config.ui.close_to_tray ?? false} onChange={e=>{setConfig({...config,ui:{...config.ui,close_to_tray:e.target.checked}});setDirty(true)}}/><span><strong>{t("closeToTray")}</strong><small>{t("closeToTrayHelp")}</small></span></label>
          </div>
          <div className="notice">{t("windowsBehaviorSaveHint")}</div>
        </div>}


        <div className="card"><div className="card-head"><div><h2>{t("webTitle")}</h2><p>{t("webHelp")}</p></div></div>
          <dl className="settings-dl"><div><dt>{t("currentMode")}</dt><dd>{health?.headless?(health.desktop_owned?"Headless (Desktop-managed)":"Headless (standalone)"):"Desktop Tauri"}</dd></div><div><dt>{t("minerCrashGuard")}</dt><dd>{health?.miner_crash_guard?t("minerCrashGuardActive"):t("minerCrashGuardUnavailable")}</dd></div><div><dt>{t("localUrl")}</dt><dd><code>http://127.0.0.1:{config.web.desktop_api_port||17888}/</code></dd></div><div><dt>{t("configFile")}</dt><dd>{health?.config_path||"—"}</dd></div></dl>
          <div className="form-grid three"><NumField label={t("webPort")} value={config.web.desktop_api_port} onChange={v=>{if(v){setConfig({...config,web:{...config.web,desktop_api_port:v}});setDirty(true)}}}/><label className="check field-check"><input type="checkbox" checked={config.web.expose_lan} onChange={e=>{let nextToken=config.web.token;if(e.target.checked&&!nextToken)nextToken=randToken();setConfig({...config,web:{...config.web,expose_lan:e.target.checked,token:nextToken}});setDirty(true)}}/> {t("lanAccess")}</label></div>
          <p className="muted">{t("lanAccessHelp")}</p>
          <div className="token-row"><Field label={t("accessToken")} value={config.web.token||""} onChange={v=>{setConfig({...config,web:{...config.web,token:v}});setDirty(true)}} placeholder={t("emptyLocal")} wide/><button className="mini-btn" onClick={()=>{const v=randToken();setConfig({...config,web:{...config.web,token:v}});setDirty(true);setStatusText("New access token generated. Save settings to activate it.")}}>{t("generateToken")}</button></div>
          <div className="notice">{t("restartRequired")}<br/>{t("lanUrl")}: <code>http://&lt;PC-IP&gt;:{config.web.desktop_api_port||17888}/?token={config.web.token||"TOKEN"}</code><br/>{t("tokenRemoteHint")}</div>
        </div>

        {security?.supported&&<div className="card"><div className="card-head"><div><h2>{t("windowsSecurity")}</h2><p>{t("windowsSecurityHelp")}</p></div></div><div className="security-list"><div className="security-item"><div><strong>{t("firewall")}</strong><span>{t("firewallHelp")}</span></div><button className={`mini-btn ${security.firewall_enabled?"security-on":""}`} disabled={securityBusy!==null} onClick={()=>void changeSecurity("firewall",!security.firewall_enabled)}>{securityBusy==="firewall"?"…":(security.firewall_enabled?t("enabledState"):t("enable"))}</button></div><div className="security-item"><div><strong>{t("defender")}</strong><span>{t("defenderHelp")}</span></div><button className={`mini-btn ${security.defender_exclusion_enabled?"security-on":""}`} disabled={securityBusy!==null} onClick={()=>void changeSecurity("defender",!security.defender_exclusion_enabled)}>{securityBusy==="defender"?"…":(security.defender_exclusion_enabled?t("enabledState"):t("enable"))}</button></div></div>{securityError&&<div className="notice danger-notice">{securityError}</div>}<div className="managed-path"><span>{t("folder")}</span><code>{security.miner_root}</code></div></div>}
        <div className="card"><div className="card-head"><div><h2>{t("supportedEngines")}</h2><p>{t("supportedHelp")}</p></div></div><div className="engine-grid">{engines.map(e=><div key={e.id}><strong>{e.name}</strong><span>{e.github_repo||t("manualPath")}</span></div>)}</div></div>
      </section>}
    </main>
    {pendingPower&&<div className="power-modal-backdrop" role="presentation">
      <div className="power-modal" role="dialog" aria-modal="true" aria-labelledby="power-title">
        <button className="power-close" aria-label={t("cancel")} onClick={()=>void cancelPendingPower()}>×</button>
        <div className="power-icon">⏻</div>
        <h2 id="power-title">{pendingPower.action==="hibernate"?t("hibernateCountdownTitle"):t("sleepCountdownTitle")}</h2>
        <p>{t("scheduleEnded").replace("{name}", pendingPower.schedule_name)}</p>
        <div className="power-countdown">{powerSecondsLeft}</div>
        <p className="muted">{t("closeCancelsSleep")}</p>
        <div className="power-actions"><button className="btn ghost" autoFocus onClick={()=>void cancelPendingPower()}>{t("cancel")}</button><button className="btn primary" onClick={()=>void confirmPendingPower()}>{pendingPower.action==="hibernate"?t("hibernateNow"):t("sleepNow")}</button></div>
      </div>
    </div>}
  </div>;
}

function Metric({title,value,accent=false}:{title:string;value:string;accent?:boolean}){return <div className={`metric ${accent?"metric-accent":""}`}><span>{title}</span><strong>{value}</strong><i/></div>}
function Field({label,value,onChange,placeholder="",wide=false}:{label:string;value:string;onChange:(v:string)=>void;placeholder?:string;wide?:boolean}){return <label className={`field ${wide?"wide":""}`}><span>{label}</span><input value={value} onChange={e=>onChange(e.target.value)} placeholder={placeholder}/></label>}
function NumField({label,value,onChange}:{label:string;value:number|null;onChange:(v:number|null)=>void}){return <label className="field"><span>{label}</span><input type="number" value={value??""} onChange={e=>onChange(e.target.value===""?null:Number(e.target.value))}/></label>}
function MiniNum({value,onChange,placeholder=""}:{value:number|null;onChange:(v:number|null)=>void;placeholder?:string}){return <input className="mini-num" type="number" value={value??""} placeholder={placeholder} onChange={e=>onChange(e.target.value===""?null:Number(e.target.value))}/>}
function SelectField({label,value,onChange,options}:{label:string;value:string;onChange:(v:string)=>void;options:Array<[string,string]>}){return <label className="field"><span>{label}</span><select value={value} onChange={e=>onChange(e.target.value)}>{options.map(([v,l])=><option key={v} value={v}>{l}</option>)}</select></label>}
