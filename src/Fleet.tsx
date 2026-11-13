import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import type { ScheduleWindow } from "./types";
import { decodeFleetLogs, decodeFleetUpdateStatus, decodeFleetView, fleetEditablePatch, fleetHashrate, fleetPreflight, fleetRigFresh, fleetSummary, fleetUpdatePreflight, fleetUptime } from "./fleetTypes";
import type {
  FleetAction, FleetConfig, FleetDiscovery, FleetInvitation, FleetLogs, FleetMiner,
  FleetOperation, FleetProfile, FleetProfilePatch, FleetReceipt, FleetRequest, FleetRig, FleetRollout, FleetUpdateStatus, FleetView,
} from "./fleetTypes";

export type FleetProps = { language: string; request: FleetRequest; localName?: string; platform?: string };
type Text = (fr: string, en: string) => string;
type Target = { rig_id: string; profile_id: string };
type OperationResult = { rig_id: string; name: string; receipt?: FleetReceipt; error?: string; updateCheck?: boolean };
type Editor = { rig: FleetRig; config: FleetConfig; before: FleetProfile; draft: FleetProfile };
type ScheduleEditor = { rig: FleetRig; config: FleetConfig; schedules: ScheduleWindow[] };
type Review = { targets: Target[]; action: FleetAction };
const json = (body: unknown): RequestInit => ({ method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) });
const errorText = (error: unknown) => error instanceof Error ? error.message : String(error);
const fresh = fleetRigFresh;
const millis = (time: number) => time > 1e11 ? time : time * 1000;
function age(time: number, t: Text) {
  if (!time) return t("Jamais reçu", "Never received");
  const seconds = Math.max(0, Math.floor((Date.now() - millis(time)) / 1000));
  if (seconds < 10) return t("À l’instant", "Just now");
  if (seconds < 60) return t(`Il y a ${seconds} s`, `${seconds}s ago`);
  if (seconds < 3600) return t(`Il y a ${Math.floor(seconds / 60)} min`, `${Math.floor(seconds / 60)}m ago`);
  return t(`Il y a ${Math.floor(seconds / 3600)} h`, `${Math.floor(seconds / 3600)}h ago`);
}
function unavailableReason(rig: FleetRig, t: Text) {
  if (rig.validation_error === "snapshot") return t("L’agent n’a pas encore envoyé d’état complet et valide. Le minage reste inconnu et les commandes sont désactivées.", "The agent has not reported a complete valid snapshot yet. Mining remains unknown and commands are disabled.");
  if (rig.validation_error === "config") return t("La configuration reçue est incomplète ou invalide. L’état n’est pas confirmé ; les commandes et modifications sont désactivées.", "The received configuration is incomplete or invalid. State is unconfirmed; commands and edits are disabled.");
  return t("L’agent est injoignable. Le minage actuel est inconnu ; les derniers états restent consultables mais aucune commande n’est envoyée.", "The agent is unreachable. Current mining is unknown; previous snapshots remain inspectable, but no commands are sent.");
}
function connectionLabel(rig: FleetRig, t: Text) {
  if (fresh(rig)) return t("Connecté", "Connected");
  if (!rig.last_seen && !rig.snapshot) return t("En attente du premier état", "Waiting for first snapshot");
  return rig.validation_error ? t("État non confirmé", "Unconfirmed state") : t("Hors ligne", "Offline");
}
function Badge({ children, kind = "" }: { children: ReactNode; kind?: string }) {
  return <span className={`fleet-badge ${kind}`}>{children}</span>;
}
function mode(miner: FleetMiner, t: Text) {
  if (miner.running) return miner.mode === "forced" ? t("Forcé · session", "Forced session") : t("En minage", "Mining");
  if (miner.scheduler_paused) return t("Planning en pause", "Schedule paused");
  if (miner.mode === "waiting") return t("Planning · attente", "Schedule · waiting");
  return t("Arrêté", "Stopped");
}
function rigMode(rig: FleetRig, t: Text) {
  if (!fresh(rig)) return t("Minage inconnu", "Mining unknown");
  const statuses = rig.snapshot!.status;
  const running = statuses.filter(s => s.running);
  if (running.length) return running.some(s => s.mode === "forced") ? t("Session forcée", "Forced session") : t(`${running.length} en minage`, `${running.length} mining`);
  if (statuses.some(s => s.mode === "waiting")) return t("Planning · attente", "Schedule · waiting");
  return t("Arrêté", "Stopped");
}
function authText(value: FleetMiner["root_authorization"], t: Text) {
  if (value === "required") return t("Autorisation requise sur le rig", "Authorization required on the rig");
  if (value === "approval_saved_unverified") return t("Autorisation enregistrée · vérification au lancement", "Authorization saved · verified at launch");
  if (value === "not_requested") return t("Session utilisateur", "User session");
  return t("Sans autorisation root", "Root authorization not applicable");
}
function reason(code: string | null, t: Text) {
  return ({ offline: t("Hors ligne : aucun ordre envoyé", "Offline: no command will be sent"), missing_profile: t("Profil introuvable", "Profile not found"),
    disabled: t("Profil désactivé", "Profile disabled"), local_authorization: t("Validation root requise sur le rig", "Root approval required on the rig"),
    no_schedule: t("Ce profil n’a pas de planning", "This profile has no schedule") } as Record<string, string>)[code || ""] || "";
}
function actionLabel(action: FleetAction, t: Text) {
  return ({ start: t("Démarrer", "Start"), stop: t("Arrêter", "Stop"), restart: t("Redémarrer", "Restart"),
    resume: t("Reprendre le planning", "Resume schedule"), force_start: t("Forcer cette session", "Force this session") })[action];
}
function receiptLabel(receipt: FleetReceipt, t: Text) {
  return ({ queued: t("En file", "Queued"), running: t("En cours", "Running"), succeeded: t("Réussi", "Succeeded"),
    failed: t("Échec", "Failed"), expired: t("Expiré", "Expired") })[receipt.state];
}
function updateReason(code: string, t: Text): string {
  return ({
    offline: t("Hors ligne : exclu de la mise à jour.", "Offline: excluded from the update."),
    bootstrap: t("Installez d’abord cette nouvelle version localement avec l’outil de mise à jour de MinerDesk, puis activez les mises à jour distantes une fois sur ce rig.", "First install this new release locally using MinerDesk’s existing updater, then enable remote updates once on this rig."),
    disabled: t("Mises à jour distantes désactivées. Activez-les localement sur ce rig.", "Remote updates are off. Enable them locally on this rig."),
    busy: t("Une mise à jour est déjà en cours.", "An update is already in progress."),
    check_required: t("Vérifiez les mises à jour pour connaître la version disponible.", "Check updates to discover the available version."),
    version_changed: t("La version proposée ne correspond plus à la version examinée. Vérifiez à nouveau.", "The offered version no longer matches the reviewed version. Check again."),
    up_to_date: t("À jour selon la dernière vérification.", "Up to date according to the last check."),
    supported: t("Prêt pour une mise à jour distante.", "Ready for a remote update."),
    checking: t("Recherche de mises à jour…", "Checking for updates…"),
    unavailable: t("La version disponible n’a pas pu être vérifiée.", "The available release could not be verified."),
    development: t("Les versions de développement ne peuvent pas être mises à jour à distance.", "Development builds cannot be updated remotely."),
    platform: t("Plateforme non prise en charge pour les mises à jour distantes.", "This platform does not support remote updates."),
    installation: t("Cette installation ne permet pas les mises à jour distantes.", "This installation does not support remote updates."),
    portable: t("Cette installation portable ne permet pas les mises à jour distantes.", "This portable installation does not support remote updates."),
    readonly: t("L’installation est en lecture seule.", "The installation is read-only."),
    root_enrollment_required: t("Autorisation Linux initiale requise localement sur ce rig.", "Initial Linux authorization is required locally on this rig."),
    helper_missing: t("L’assistant de mise à jour doit être installé localement sur ce rig.", "The update helper must be installed locally on this rig."),
    policy_unavailable: t("La politique locale de mise à jour n’est pas disponible.", "The local update policy is unavailable."),
    administrator_required: t("Une autorisation administrateur Windows est requise localement sur ce rig.", "Windows administrator authorization is required locally on this rig."),
  } as Record<string, string>)[code] || t("Ce rig n’est pas prêt pour une mise à jour distante.", "This rig is not ready for a remote update.");
}
function updateStage(stage: string, t: Text): string {
  return ({ queued: t("En file · pas encore installé", "Queued · not installed"), checking: t("Vérification", "Checking"),
    downloading: t("Téléchargement", "Downloading"), verifying: t("Vérification de la signature", "Verifying signature"), ready: t("Préparation", "Preparing"),
    stopping: t("Arrêt du minage", "Stopping mining"), installing: t("Installation en cours", "Installing"),
    restarting: t("Redémarrage · attente de reconnexion", "Restarting · waiting for reconnect"),
    completed: t("Installation terminée · version à confirmer après reconnexion", "Installer completed · awaiting version confirmation after reconnect"),
    waiting_for_reconnect: t("Attente de reconnexion et confirmation de version", "Waiting for reconnect and version confirmation"),
    updated: t("Version confirmée après reconnexion", "Version confirmed after reconnect"),
    running: t("Déploiement en cours", "Rollout in progress"), failed: t("Échec · déploiement arrêté", "Failed · rollout stopped"),
    cancelled: t("Restant annulé", "Remaining cancelled") } as Record<string, string>)[stage] || stage;
}
function UpdateInfo({ rig, t }: { rig: FleetRig; t: Text }) {
  const updates = rig.snapshot?.updates;
  const job = updates?.job;
  return <div className="fleet-update-info">
    <strong>{t("Version MinerDesk", "MinerDesk version")} · {rig.snapshot?.version || "—"}</strong>
    {!fresh(rig) && <small>{t("Dernier état reçu · version actuelle non confirmée", "Last reported snapshot · current version unconfirmed")}</small>}
    {updates?.protocol === 1 ? <>
      {updates.available_version && <span>{t("Disponible", "Available")} · {updates.available_version}</span>}
      {job && <><span>{updateStage(job.stage, t)} · {job.expected_version}</span>
        {job.stage === "downloading" && <><progress aria-label={t(`Téléchargement sur ${rig.name}`, `Download on ${rig.name}`)} value={job.total && job.total > 0 ? Math.min(job.downloaded, job.total) : undefined} max={job.total && job.total > 0 ? job.total : undefined}/><small>{Math.round(job.downloaded / 1024 / 1024)} MiB{job.total ? ` / ${Math.round(job.total / 1024 / 1024)} MiB` : ""}</small></>}
        {job.error && <small className="fleet-error-text">{job.error}</small>}</>}
      <small>{updateReason(fleetUpdatePreflight(rig) || "supported", t)}</small>
    </> : <small>{updateReason("bootstrap", t)}</small>}
  </div>;
}
function resultSummary(value: unknown, t: Text) {
  if (value && typeof value === "object") {
    const result = value as Record<string, unknown>;
    if (typeof result.message === "string" && result.message.trim()) return result.message;
    if (result.mode === "waiting") return t("Planning respecté : en attente du prochain créneau.", "Schedule respected: waiting for the next window.");
    if (result.mode === "schedule") return t("Démarrage confirmé dans le planning.", "Start confirmed within the schedule.");
    if (result.mode === "manual") return t("Session lancée et confirmée par le rig.", "Session started and confirmed by the rig.");
  }
  return t("Action confirmée par le rig.", "Action confirmed by the rig.");
}
function Dialog({ title, children, footer, onClose, busy, t }: { title: string; children: ReactNode; footer?: ReactNode; onClose: () => void; busy?: boolean; t: Text }) {
  const ref = useRef<HTMLDivElement>(null);
  const interaction = useRef({ busy, onClose }); interaction.current = { busy, onClose };
  useEffect(() => {
    const previous = typeof document !== "undefined" ? document.activeElement : null;
    ref.current?.querySelector<HTMLButtonElement>("button")?.focus();
    const key = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !interaction.current.busy) interaction.current.onClose();
      if (event.key === "Tab") {
        const controls = ref.current?.querySelectorAll<HTMLElement>('button:not(:disabled), input:not(:disabled), select:not(:disabled), textarea:not(:disabled)');
        if (!controls?.length) return;
        const first = controls[0], last = controls[controls.length - 1];
        if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last.focus(); }
        else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first.focus(); }
      }
    };
    if (typeof document !== "undefined") document.addEventListener("keydown", key);
    return () => { if (typeof document !== "undefined") document.removeEventListener("keydown", key); if (typeof HTMLElement !== "undefined" && previous instanceof HTMLElement) previous.focus(); };
  }, []);
  return <div className="fleet-dialog-backdrop"><div ref={ref} className="fleet-dialog" role="dialog" aria-modal="true" aria-label={title}>
    <header className="fleet-dialog-head"><h2>{title}</h2><button type="button" className="fleet-btn fleet-small" onClick={onClose} disabled={busy}>{t("Fermer", "Close")}</button></header>
    <div className="fleet-dialog-body">{children}</div>{footer && <footer className="fleet-dialog-foot">{footer}</footer>}
  </div></div>;
}

export default function Fleet({ language, request, localName }: FleetProps) {
  const fr = language.toLowerCase().startsWith("fr");
  const t: Text = (f, e) => fr ? f : e;
  const [view, setView] = useState<FleetView | null>(null), [loadError, setLoadError] = useState("");
  const [refreshedAt, setRefreshedAt] = useState(Date.now());
  const [busy, setBusy] = useState(false), [error, setError] = useState(""), [notice, setNotice] = useState("");
  const [layout, setLayout] = useState<"cards" | "table">("cards"), [query, setQuery] = useState(""), [filter, setFilter] = useState("all");
  const [selected, setSelected] = useState<string[]>([]), [detailId, setDetailId] = useState<string | null>(null);
  const [detailTab, setDetailTab] = useState<"profiles" | "gpu" | "schedules" | "logs" | "updates">("profiles");
  const [hubName, setHubName] = useState(localName || ""), [hubAddress, setHubAddress] = useState("");
  const [joinAddress, setJoinAddress] = useState(""), [joinKey, setJoinKey] = useState("");
  const [showSetup, setShowSetup] = useState(false), [invitation, setInvitation] = useState<FleetInvitation | null>(null);
  const [discovered, setDiscovered] = useState<FleetDiscovery[] | null>(null);
  const [review, setReview] = useState<Review | null>(null), [results, setResults] = useState<OperationResult[]>([]);
  const [editor, setEditor] = useState<Editor | null>(null), [scheduleEditor, setScheduleEditor] = useState<ScheduleEditor | null>(null);
  const [logsProfile, setLogsProfile] = useState(""), [logReceipt, setLogReceipt] = useState<string | null>(null), [logs, setLogs] = useState<FleetLogs | null>(null);
  const [disconnect, setDisconnect] = useState(false);
  const [revokeRig, setRevokeRig] = useState<FleetRig | null>(null);
  const [updateReview, setUpdateReview] = useState<string[] | null>(null);
  const [localUpdates, setLocalUpdates] = useState<FleetUpdateStatus | null>(null), [policyError, setPolicyError] = useState("");
  const [policyAction, setPolicyAction] = useState<string | null>(null);
  const requestRef = useRef(request); requestRef.current = request;
  const alive = useRef(true), polling = useRef(false), mutation = useRef(false);
  const refresh = useCallback(async (signal?: AbortSignal) => {
    if (polling.current) return;
    polling.current = true;
    try {
      const next = decodeFleetView(await requestRef.current<unknown>("/api/fleet", signal ? { signal } : {}, 12000));
      if (alive.current && !signal?.aborted) { setView(next); setLoadError(""); }
    } catch (e) { if (alive.current && !signal?.aborted) setLoadError(errorText(e)); }
    finally { polling.current = false; if (alive.current && !signal?.aborted) setRefreshedAt(Date.now()); }
  }, []);
  useEffect(() => {
    alive.current = true;
    const controller = new AbortController(); let timer: ReturnType<typeof setTimeout>;
    const poll = async () => { await refresh(controller.signal); if (!controller.signal.aborted) timer = setTimeout(() => void poll(), 5000); };
    void poll();
    return () => { alive.current = false; controller.abort(); clearTimeout(timer); };
  }, [refresh]);
  useEffect(() => {
    if (view?.setup.name) setHubName(old => old || view.setup.name);
  }, [view?.setup.name]);
  useEffect(() => { if (view?.setup.suggested_url) setHubAddress(old => old || view.setup.suggested_url!); }, [view?.setup.suggested_url]);
  const showLocalUpdates = !!view?.local_setup_allowed && (showSetup || view?.setup.mode === "standalone");
  useEffect(() => {
    if (!showLocalUpdates) return;
    const controller = new AbortController();
    void requestRef.current<unknown>("/api/fleet/updates", { signal: controller.signal }, 12000).then(value => {
      if (!alive.current || controller.signal.aborted) return;
      const decoded = decodeFleetUpdateStatus(value);
      if (!decoded) throw new Error(t("État de mise à jour invalide", "Invalid update status"));
      setLocalUpdates(decoded); setPolicyError("");
    }).catch(e => { if (alive.current && !controller.signal.aborted) { setLocalUpdates(null); setPolicyError(errorText(e)); } });
    return () => controller.abort();
  }, [showLocalUpdates]);
  useEffect(() => {
    const receipt = view?.commands.find(c => c.id === logReceipt);
    if (receipt?.state === "succeeded" && receipt.result && typeof receipt.result === "object") {
      const value = decodeFleetLogs(receipt.result);
      if (value) setLogs(value);
    }
  }, [view?.commands, logReceipt]);
  const rigs = view?.rigs || [];
  const detail = rigs.find(r => r.rig_id === detailId) || null;
  const detailSummary = detail ? fleetSummary([detail]) : null;
  const activeLogReceipt = view?.commands.find(c => c.id === logReceipt);
  const summary = useMemo(() => fleetSummary(rigs), [rigs, refreshedAt]);
  const shown = rigs.filter(r => `${r.name} ${r.snapshot?.os || ""} ${r.snapshot?.status.map(m => `${m.name} ${m.engine} ${m.algorithm}`).join(" ") || ""}`.toLowerCase().includes(query.toLowerCase()))
    .filter(r => filter === "all" || filter === "online" && fresh(r) || filter === "offline" && !fresh(r)
      || filter === "mining" && fresh(r) && r.snapshot!.status.some(m => m.running)
      || filter === "waiting" && fresh(r) && r.snapshot!.status.some(m => m.mode === "waiting"));
  const canControl = !!view?.controller_allowed;
  const localSetup = !!view?.local_setup_allowed;
  const canRevoke = (rig: FleetRig) => view?.setup.mode === "hub" && localSetup
    && rig.rig_id !== view.setup.rig_id && view.rigs.some(r => r.rig_id === rig.rig_id);
  async function mutate<T>(path: string, options: RequestInit, done?: (value: T) => void, timeoutMs = 20000) {
    if (mutation.current) return;
    mutation.current = true; setBusy(true); setError(""); setNotice("");
    try { const value = await requestRef.current<T>(path, options, timeoutMs); if (alive.current) done?.(value); await refresh(); }
    catch (e) { if (alive.current) setError(errorText(e).split(joinKey || "\u0000").join("[redacted]")); }
    finally { mutation.current = false; if (alive.current) setBusy(false); }
  }
  async function updatePolicy(action: "enable" | "disable" | "enroll" | "revoke") {
    if (!localSetup || mutation.current) return;
    setPolicyAction(action);
    await mutate<unknown>(`/api/fleet/updates/${action}`, json({}), value => {
      const decoded = decodeFleetUpdateStatus(value);
      if (!decoded) { setPolicyError(t("État de mise à jour invalide", "Invalid update status")); return; }
      setLocalUpdates(decoded); setPolicyError("");
      setNotice(t("Politique de mise à jour enregistrée sur cette machine.", "Update policy saved on this machine."));
    }, action === "enroll" ? 150000 : 20000);
    if (alive.current) setPolicyAction(null);
  }
  async function startRollout(rigIds: string[], expectedVersion: string) {
    if (!canControl || !rigIds.length || rigIds.some(id => { const rig = rigs.find(r => r.rig_id === id); return !rig || fleetUpdatePreflight(rig, expectedVersion); })) return;
    await mutate<unknown>("/api/fleet/updates/rollouts", json({ rig_ids: rigIds, expected_version: expectedVersion }), () => {
      setUpdateReview(null);
      setNotice(t("Déploiement accepté. Chaque version sera confirmée après reconnexion ; l’acceptation ne prouve pas l’installation.", "Rollout accepted. Each version is confirmed after reconnect; acceptance does not prove installation."));
    });
  }
  function openRig(rig: FleetRig) { setDetailId(rig.rig_id); setDetailTab("profiles"); setLogs(null); setLogReceipt(null); }
  function queueAction(rig: FleetRig, miner: FleetMiner, action: FleetAction) {
    if (action === "force_start") setReview({ targets: [{ rig_id: rig.rig_id, profile_id: miner.id }], action });
    else void sendOperations([{ rig, operation: { action, profile_id: miner.id } }]);
  }
  async function sendOperations(items: { rig: FleetRig; operation: FleetOperation }[]) {
    if (!canControl || mutation.current || !items.length) return;
    mutation.current = true; setBusy(true); setError(""); setNotice("");
    const next: OperationResult[] = [];
    for (const item of items) {
      if (!alive.current) break;
      const current = rigs.find(r => r.rig_id === item.rig.rig_id);
      if (!current || !fresh(current)) {
        next.push({ rig_id: item.rig.rig_id, name: item.rig.name, error: reason("offline", t) });
        continue;
      }
      const operation = item.operation;
      if (operation.action === "profile_patch" && current.snapshot?.status.some(m => m.id === operation.profile_id && m.running)) {
        next.push({ rig_id: item.rig.rig_id, name: item.rig.name, error: t("Arrêtez ce profil avant de modifier sa configuration.", "Stop this profile before editing its configuration.") });
        continue;
      }
      try {
        const receipts = await requestRef.current<FleetReceipt[]>("/api/fleet/commands", json({ rig_ids: [item.rig.rig_id], operation: item.operation }), 20000);
        for (const receipt of receipts) next.push({ rig_id: item.rig.rig_id, name: item.rig.name, receipt, updateCheck: item.operation.action === "update_check" });
        if (!receipts.length) next.push({ rig_id: item.rig.rig_id, name: item.rig.name, error: t("Aucun accusé de réception", "No command receipt returned") });
      } catch (e) { next.push({ rig_id: item.rig.rig_id, name: item.rig.name, error: errorText(e) }); }
    }
    if (alive.current) {
      setResults(next); setReview(null); setEditor(null); setScheduleEditor(null);
      setNotice(t("Consultez le résultat de chaque rig. Une commande en file n’est pas une réussite confirmée.", "Check each rig's result. A queued command is not a confirmed success."));
    }
    mutation.current = false; if (alive.current) setBusy(false); await refresh();
  }
  async function loadLogs(rig: FleetRig) {
    if (!logsProfile || !fresh(rig)) return;
    await mutate<FleetReceipt[]>("/api/fleet/commands", json({ rig_ids: [rig.rig_id], operation: { action: "logs", profile_id: logsProfile, limit: 250 } }), receipts => {
      const receipt = receipts[0]; setLogReceipt(receipt?.id || null); setLogs(null);
      if (receipt?.state === "succeeded") setLogs(decodeFleetLogs(receipt.result));
    });
  }
  function profileActions(rig: FleetRig, miner: FleetMiner) {
    return <div className="fleet-actions">
      {([miner.running ? "stop" : "start", "restart", "resume"] as FleetAction[]).map(action => <button type="button" key={action} className={`fleet-btn fleet-small ${action === "start" ? "fleet-primary" : ""}`} disabled={busy || !canControl || !!fleetPreflight(rig, miner.id, action)} onClick={() => queueAction(rig, miner, action)}>{actionLabel(action, t)}</button>)}
      <button type="button" className="fleet-btn fleet-small fleet-force" disabled={busy || !canControl || !!fleetPreflight(rig, miner.id, "force_start")} onClick={() => queueAction(rig, miner, "force_start")}>{t("Forcer…", "Force…")}</button>
    </div>;
  }
  const setupContent = view && <div className="fleet-setup">
    {localSetup && <section className="fleet-setup-panel fleet-update-policy"><h2>{t("Mises à jour distantes · cette machine", "Remote updates · this machine")}</h2>
      <p>{t("Désactivées par défaut. Après cette activation locale initiale sur chaque rig, vous pourrez lancer les futures mises à jour depuis n’importe quel PC associé autorisé.", "Off by default. After this initial local opt-in on each rig, you can start future updates from any authorized paired PC.")}</p>
      {policyAction === "enroll" && <p role="status">{t("En attente de l’approbation système Linux sur cette machine… La demande peut prendre jusqu’à deux minutes.", "Waiting for Linux system approval on this machine… This can take up to two minutes.")}</p>}
      {localUpdates ? <><label className="fleet-check"><input type="checkbox" checked={localUpdates.enabled} disabled={busy || localUpdates.protocol !== 1} onChange={e => void updatePolicy(e.target.checked ? "enable" : "disable")}/>{t("Autoriser les mises à jour distantes sur cette machine", "Allow remote updates on this machine")}</label>
        <small>{updateReason(localUpdates.reason, t)}</small>
        {localUpdates.root_required && <><p>{t("Linux : autorisez une fois l’assistant système sur cette machine. La demande d’approbation s’ouvre dans le système ; aucun mot de passe administrateur n’est transmis ou conservé par Fleet.", "Linux: authorize the system helper once on this machine. Approval opens in the operating system; Fleet never sends or stores an administrator password.")}</p><div className="fleet-actions"><Badge kind={localUpdates.root_enrolled ? "good" : "warning"}>{localUpdates.root_enrolled ? t("Assistant autorisé", "Helper authorized") : t("Autorisation initiale requise", "Initial authorization required")}</Badge><button type="button" className="fleet-btn fleet-small" disabled={busy} onClick={() => void updatePolicy(localUpdates.root_enrolled ? "revoke" : "enroll")}>{localUpdates.root_enrolled ? t("Révoquer l’autorisation Linux", "Revoke Linux authorization") : t("Autoriser une fois sur Linux", "Authorize once on Linux")}</button></div></>}
      </> : !policyError && <p role="status">{t("Chargement de la politique locale…", "Loading local policy…")}</p>}
      {policyError && <div className="fleet-callout error" role="alert">{t("La politique locale n’a pas pu être chargée. Aucune activation n’est présumée.", "The local policy could not be loaded. Opt-in is not assumed.")}<p>{policyError}</p></div>}
    </section>}
    {view.setup.mode === "standalone" && <section className="fleet-setup-panel"><h2>{t("Créer une flotte ici", "Create a fleet here")}</h2><p>{t("Cette instance devient le coordinateur. Les agents conservent leurs plannings et configurations locales.", "This instance becomes the coordinator. Agents retain their local schedules and configuration.")}</p>
      <form onSubmit={e => { e.preventDefault(); void mutate<FleetView>("/api/fleet/create", json({ name: hubName.trim(), advertise_url: hubAddress.trim(), listen: "0.0.0.0:17889" }), result => { setView(decodeFleetView(result)); setShowSetup(false); setNotice(t("Coordinateur créé. Générez une clé d’association pour ajouter une machine.", "Coordinator created. Generate a pairing key to add a machine.")); }); }}>
        <label className="fleet-field">{t("Nom du coordinateur", "Coordinator name")}<input value={hubName} onChange={e => setHubName(e.target.value)} required maxLength={80} disabled={!localSetup || busy}/></label>
        <label className="fleet-field">{t("Adresse HTTPS annoncée sur votre réseau", "Advertised HTTPS address on your network")}<input type="url" value={hubAddress} onChange={e => setHubAddress(e.target.value)} placeholder="https://192.168.1.10:17889" pattern="https://.*" required disabled={!localSetup || busy}/></label>
        <small>{t("Un service TLS dédié écoute sur le port 17889. Utilisez une adresse privée joignable par vos rigs.", "A dedicated TLS service listens on port 17889. Use a private address reachable by your rigs.")}</small>
        <button className="fleet-btn fleet-primary" disabled={!localSetup || busy || !hubName.trim() || !hubAddress.startsWith("https://")}>{t("Créer la flotte", "Create fleet")}</button>
      </form></section>}
    {view.setup.mode === "standalone" && <section className="fleet-setup-panel"><h2>{t("Rejoindre une flotte", "Join a fleet")}</h2><p>{t("Générez une clé sur le coordinateur et transférez-la par un canal de confiance. Aucun mot de passe administrateur n’est demandé ici.", "Generate a key on the coordinator and transfer it through a trusted channel. No administrator password is requested here.")}</p>
      <form onSubmit={e => { e.preventDefault(); void mutate<FleetView>("/api/fleet/join", json({ address: joinAddress.trim(), key: joinKey.trim() }), result => { setJoinKey(""); setView(decodeFleetView(result)); setShowSetup(false); }); }}>
        <label className="fleet-field">{t("Adresse HTTPS du coordinateur", "Coordinator HTTPS address")}<input type="url" value={joinAddress} onChange={e => setJoinAddress(e.target.value)} placeholder="https://192.168.1.10:17889" pattern="https://.*" required disabled={!localSetup || busy}/></label>
        <label className="fleet-field">{t("Clé d’association", "Pairing key")}<input type="password" autoComplete="off" value={joinKey} onChange={e => setJoinKey(e.target.value)} required disabled={!localSetup || busy}/></label>
        <small>{t("Le nom de cette machine est détecté automatiquement.", "This machine's name is detected automatically.")}</small>
        <button className="fleet-btn fleet-primary" disabled={!localSetup || busy || !joinKey.trim() || !joinAddress.startsWith("https://")}>{t("Rejoindre", "Join")}</button>
      </form></section>}
    <section className="fleet-setup-panel"><h2>{t("Découverte locale", "LAN discovery")}</h2><p>{t("La recherche n’est lancée qu’à votre demande. Les annonces sont des indications non vérifiées : contrôlez l’adresse et l’empreinte lors de l’association.", "Scanning runs only when requested. Announcements are unverified hints: check the address and fingerprint when pairing.")}</p>
      <button type="button" className="fleet-btn" disabled={busy || !localSetup} onClick={() => void mutate<FleetDiscovery[]>("/api/fleet/discover", json({}), setDiscovered)}>{t("Chercher sur le LAN", "Scan LAN")}</button>
      {discovered && <div className="fleet-discovery-list">{discovered.length === 0 ? <small>{t("Aucun coordinateur annoncé trouvé.", "No advertised coordinator found.")}</small> : discovered.map(d => <div className="fleet-discovery" key={d.fleet_id + d.address}><div><strong>{d.name}</strong><div className="fleet-rig-line">{d.address}</div><div className="fleet-rig-line">{d.certificate_fingerprint}</div></div>{view.setup.mode === "standalone" && <button type="button" className="fleet-btn fleet-small" onClick={() => setJoinAddress(d.address)}>{t("Choisir l’adresse", "Use address")}</button>}</div>)}</div>}
    </section>
    {view.setup.mode !== "standalone" && <section className="fleet-setup-panel"><h2>{t("Association actuelle", "Current association")}</h2><div className="fleet-info-value">{view.setup.hub_url || hubAddress || "—"}</div><small>{view.setup.tls_fingerprint || ""}</small><p>{t("Dissocier la flotte ne doit pas remplacer les plannings et profils locaux des agents.", "Disconnecting the fleet does not replace agents' local schedules and profiles.")}</p><button type="button" className="fleet-btn fleet-danger" disabled={busy || !localSetup} onClick={() => setDisconnect(true)}>{view.setup.mode === "hub" ? t("Désactiver le coordinateur…", "Disable coordinator…") : t("Quitter la flotte…", "Leave fleet…")}</button></section>}
  </div>;

  return <section className="fleet-page" aria-label={t("Flotte MinerDesk", "MinerDesk Fleet")}>
    <header className="fleet-heading"><div><div className="fleet-kicker">MinerDesk Fleet</div><h1>{t("Votre flotte, au même endroit.", "Your rigs, in one place.")}</h1><p>{view?.setup.mode === "hub" ? t("Coordinateur actif · agents autonomes", "Coordinator active · autonomous agents") : view?.setup.mode === "member" ? t("Connecté à une flotte · fonctionnement local préservé", "Fleet member · local operation preserved") : t("Commencez avec cette machine, puis associez vos rigs.", "Start with this machine, then pair your rigs.")}</p></div>
      <div className="fleet-actions"><button type="button" className="fleet-btn" disabled={busy} onClick={() => void refresh()}>{t("Actualiser", "Refresh")}</button>
        {view && <button type="button" className="fleet-btn" onClick={() => setShowSetup(v => !v)}>{showSetup ? t("Masquer les réglages", "Hide setup") : t("Connexion & flotte", "Connection & fleet")}</button>}
        {view?.setup.mode === "hub" && localSetup && <button type="button" className="fleet-btn fleet-primary" disabled={busy} onClick={() => void mutate<FleetInvitation>("/api/fleet/invite", json({ expires_in_seconds: 300 }), setInvitation)}>{t("Associer un rig", "Pair a rig")}</button>}
      </div></header>
    {loadError && <div className="fleet-callout error" role="alert">{t("La flotte n’a pas pu être actualisée. Les données affichées ne sont pas garanties actuelles.", "Fleet refresh failed. Displayed data is not guaranteed current.")}<p>{loadError}</p></div>}
    {!view && !loadError && <div className="fleet-empty" role="status">{t("Connexion au service Fleet…", "Connecting to the Fleet service…")}</div>}
    {view && <>
      {view.setup.error && <div className="fleet-callout warning">{view.setup.error}</div>}
      {!localSetup && <div className="fleet-callout">{t("L’association et les réglages du coordinateur sont réservés à l’accès local autorisé. Ouvrez MinerDesk sur la machine concernée.", "Pairing and coordinator setup require authorized local access. Open MinerDesk on that machine.")}</div>}
      {(showSetup || view.setup.mode === "standalone") && setupContent}
      <div className="fleet-kpis" style={{ marginTop: showSetup || view.setup.mode === "standalone" ? 22 : undefined }}>
        <div className="fleet-kpi"><div className="fleet-kpi-label">{t("Rigs connectés", "Connected rigs")}</div><div className="fleet-kpi-value">{summary.online}<span> / {rigs.length}</span></div><small>{summary.unknown} {t("état(s) inconnu(s)", "unknown state(s)")}</small></div>
        <div className="fleet-kpi"><div className="fleet-kpi-label">{t("En minage", "Mining")}</div><div className="fleet-kpi-value">{summary.running}<span> {t("rigs", "rigs")}</span></div><small>{summary.waiting} {t("en attente du planning", "waiting for schedule")}</small></div>
        <div className="fleet-kpi"><div className="fleet-kpi-label">{t("Puissance GPU rapportée", "Reported GPU power")}</div><div className="fleet-kpi-value">{summary.power === null ? "—" : Math.round(summary.power)}<span>{summary.power !== null ? " W" : ""}</span></div><small>{summary.powerReadings} {t("mesure(s) de mineurs · pas à la prise", "miner reading(s) · not wall power")}</small></div>
      </div>
      {Object.keys(summary.algorithms).length > 0 && <div className="fleet-hash-groups" aria-label={t("Débits par algorithme", "Hashrates by algorithm")}>{Object.entries(summary.algorithms).map(([algorithm, hps]) => <span key={algorithm}><strong>{algorithm}</strong> · {fleetHashrate(hps)}</span>)}</div>}
      <section className="fleet-updates-overview" aria-label={t("Mises à jour MinerDesk", "MinerDesk updates")}><div><h2>{t("Mises à jour MinerDesk", "MinerDesk updates")}</h2><p>{t("Déploiement rig par rig, coordinateur en dernier. L’étape suivante attend la reconnexion et la version confirmée ; le premier échec arrête le restant.", "Updates run one rig at a time, coordinator last. Each next step waits for reconnect and the confirmed version; the first failure stops the remaining rollout.")}</p></div><button type="button" className="fleet-btn" disabled={busy || !canControl || !rigs.some(r => fresh(r) && r.snapshot?.updates?.protocol === 1 && (!selected.length || selected.includes(r.rig_id)))} onClick={() => void sendOperations(rigs.filter(r => fresh(r) && r.snapshot?.updates?.protocol === 1 && (!selected.length || selected.includes(r.rig_id))).map(rig => ({ rig, operation: { action: "update_check" as const } })))}>{selected.length ? t("Vérifier la sélection", "Check selected updates") : t("Vérifier les mises à jour", "Check updates")}</button></section>
      {view.rollouts.map(rollout => <RolloutStatus key={rollout.id} rollout={rollout} rigs={rigs} t={t} busy={busy} canControl={canControl} onCancel={() => void mutate<unknown>(`/api/fleet/updates/rollouts/${encodeURIComponent(rollout.id)}/cancel`, json({}), () => setNotice(t("Le restant a été annulé. Le rig déjà en cours termine sa mise à jour.", "Remaining updates cancelled. A rig already updating continues.")))}/>)}
      <div className="fleet-section-head"><h2>{t("Flotte", "Fleet")}</h2><small>{t("Les états périmés sont exclus des totaux.", "Stale snapshots are excluded from totals.")}</small></div>
      <div className="fleet-toolbar"><label className="fleet-field fleet-search">{t("Rechercher", "Search")}<input type="search" placeholder={t("Rig, profil, moteur ou algorithme…", "Rig, profile, engine or algorithm…")} value={query} onChange={e => setQuery(e.target.value)}/></label>
        <label className="fleet-field fleet-filter">{t("Afficher", "Show")}<select value={filter} onChange={e => setFilter(e.target.value)}><option value="all">{t("Tous les rigs", "All rigs")}</option><option value="online">{t("Connectés", "Connected")}</option><option value="mining">{t("En minage", "Mining")}</option><option value="waiting">{t("Planning · attente", "Schedule · waiting")}</option><option value="offline">{t("Hors ligne / inconnus", "Offline / unknown")}</option></select></label>
        <div className="fleet-toggle" aria-label={t("Présentation", "Layout")}><button type="button" aria-pressed={layout === "cards"} onClick={() => setLayout("cards")}>{t("Cartes", "Cards")}</button><button type="button" aria-pressed={layout === "table"} onClick={() => setLayout("table")}>{t("Tableau", "Table")}</button></div>
      </div>
      {selected.length > 0 && <div className="fleet-bulk"><span>{selected.length} {t("rig(s) sélectionné(s)", "rig(s) selected")}</span><button type="button" className="fleet-btn fleet-small" onClick={() => setSelected([])}>{t("Effacer", "Clear")}</button><button type="button" className="fleet-btn fleet-small" disabled={!canControl || busy} onClick={() => setUpdateReview([...selected])}>{t("Mettre à jour la sélection…", "Update selected…")}</button><button type="button" className="fleet-btn fleet-primary fleet-small" disabled={!canControl || busy} onClick={() => setReview({ action: "start", targets: selected.map(id => ({ rig_id: id, profile_id: rigs.find(r => r.rig_id === id)?.snapshot?.status[0]?.id || "" })) })}>{t("Actions groupées…", "Bulk actions…")}</button></div>}
      {shown.length === 0 ? <div className="fleet-empty"><div className="fleet-empty-mark">M</div><h2>{rigs.length ? t("Aucun rig ne correspond", "No matching rigs") : t("Votre flotte commence ici", "Your fleet starts here")}</h2><p>{rigs.length ? t("Essayez un autre filtre ou terme de recherche.", "Try another filter or search term.") : t("Aucune machine associée n’a encore envoyé d’état. Créez un coordinateur ou rejoignez une flotte avec une clé d’association.", "No paired machine has reported a snapshot yet. Create a coordinator or join a fleet with a pairing key.")}</p></div>
        : layout === "cards" ? <div className="fleet-grid">{shown.map(r => { const online = fresh(r), totals = fleetSummary([r]); return <article className="fleet-rig" key={r.rig_id} aria-current={detailId === r.rig_id ? "true" : undefined}>
          <div className="fleet-rig-head"><div><button type="button" className="fleet-rig-name" onClick={() => openRig(r)}>{r.name}</button><small>{r.snapshot?.os || t("Système non reçu", "OS not reported")} · {r.snapshot?.version || "—"}</small></div><label className="fleet-select"><input type="checkbox" aria-label={t(`Sélectionner ${r.name}`, `Select ${r.name}`)} checked={selected.includes(r.rig_id)} onChange={e => setSelected(old => e.target.checked ? [...new Set([...old, r.rig_id])] : old.filter(id => id !== r.rig_id))}/></label></div>
          <div className="fleet-badges"><Badge kind={online ? "good" : "warning"}>{connectionLabel(r,t)}</Badge><Badge kind={online && totals.running ? "good" : ""}>{rigMode(r, t)}</Badge></div>
          <div className="fleet-rig-metrics"><div><small>{t("Débit par algorithme", "Hashrate by algorithm")}</small>{Object.keys(totals.algorithms).length ? Object.entries(totals.algorithms).map(([a, v]) => <div key={a}><strong>{fleetHashrate(v)}</strong><small>{a}</small></div>) : <strong>—</strong>}</div><div><small>{t("Puissance GPU", "GPU power")}</small><strong>{totals.power === null ? "—" : `${Math.round(totals.power)} W`}</strong></div></div>
          <div className="fleet-rig-line">{r.snapshot?.gpu_inventory_available && r.snapshot.gpu_inventory.length ? r.snapshot.gpu_inventory.map(g => g.name).join(" · ") : t("Inventaire GPU non disponible", "GPU inventory unavailable")}</div>
          <UpdateInfo rig={r} t={t}/>
          <div className="fleet-rig-foot"><span className="fleet-rig-seen">{t("Dernier état", "Last snapshot")} · {age(r.last_seen, t)}</span><button type="button" className="fleet-btn fleet-small fleet-quiet" onClick={() => openRig(r)}>{t("Ouvrir", "Open")}</button></div>
        </article>; })}</div> : <div className="fleet-table-wrap"><table className="fleet-table"><thead><tr><th><input type="checkbox" aria-label={t("Sélectionner les rigs affichés", "Select visible rigs")} checked={shown.length > 0 && shown.every(r => selected.includes(r.rig_id))} onChange={e => setSelected(e.target.checked ? [...new Set([...selected, ...shown.map(r => r.rig_id)])] : selected.filter(id => !shown.some(r => r.rig_id === id)))}/></th><th>Rig</th><th>{t("Connexion", "Connection")}</th><th>{t("Minage", "Mining")}</th><th>{t("Version / mise à jour", "Version / update")}</th><th>{t("Débit / algorithme", "Hashrate / algorithm")}</th><th>{t("Puissance GPU", "GPU power")}</th></tr></thead><tbody>{shown.map(r => { const totals = fleetSummary([r]); return <tr key={r.rig_id}><td><input type="checkbox" aria-label={t(`Sélectionner ${r.name}`, `Select ${r.name}`)} checked={selected.includes(r.rig_id)} onChange={e => setSelected(old => e.target.checked ? [...new Set([...old, r.rig_id])] : old.filter(id => id !== r.rig_id))}/></td><td><button type="button" className="fleet-rig-name" onClick={() => openRig(r)}>{r.name}</button><small>{r.snapshot?.os || "—"}</small></td><td><Badge kind={fresh(r) ? "good" : "warning"}>{connectionLabel(r,t)}</Badge><small>{age(r.last_seen, t)}</small></td><td>{rigMode(r, t)}</td><td><UpdateInfo rig={r} t={t}/></td><td>{Object.entries(totals.algorithms).map(([a, v]) => <div key={a}><strong>{fleetHashrate(v)}</strong><small>{a}</small></div>)}{!Object.keys(totals.algorithms).length && "—"}</td><td>{totals.power === null ? "—" : `${Math.round(totals.power)} W`}</td></tr>; })}</tbody></table></div>}
      {!canControl && <p className="fleet-rig-line" style={{ marginTop: 12 }}>{t("Le serveur n’autorise pas les commandes de flotte pour cet accès.", "The server does not allow fleet commands for this access.")}</p>}
    </>}
    {detail && <section className="fleet-detail" aria-label={t(`Détails de ${detail.name}`, `${detail.name} details`)}><header className="fleet-detail-head"><div><div className="fleet-kicker">{t("Rig sélectionné", "Selected rig")}</div><h2>{detail.name}</h2><p>{detail.snapshot?.os || "—"} · {detail.snapshot?.local_time || "—"} · {detail.snapshot?.timezone || "—"}</p></div><div className="fleet-actions">{canRevoke(detail) && <button type="button" className="fleet-btn fleet-small fleet-danger" disabled={busy} onClick={() => setRevokeRig(detail)}>{t("Retirer ce rig…", "Remove this rig…")}</button>}<button type="button" className="fleet-btn fleet-small" onClick={() => setDetailId(null)}>{t("Fermer les détails", "Close details")}</button></div></header>
      <div className="fleet-detail-tabs" role="tablist" aria-label={t("Détails du rig", "Rig details")}>{([['profiles', t("Profils", "Profiles")], ['gpu', 'GPU'], ['schedules', t("Plannings", "Schedules")], ['logs', t("Journal", "Logs")], ['updates', t("Mises à jour", "Updates")]] as const).map(([id, label]) => <button type="button" key={id} id={`fleet-tab-${id}`} role="tab" aria-controls="fleet-detail-panel" aria-selected={detailTab === id} className="fleet-btn fleet-small" onClick={() => setDetailTab(id)}>{label}</button>)}</div>
      <div className="fleet-detail-body" id="fleet-detail-panel" role="tabpanel" aria-labelledby={`fleet-tab-${detailTab}`}>
        {detailTab === "updates" && <><UpdateInfo rig={detail} t={t}/><div className="fleet-actions"><button type="button" className="fleet-btn" disabled={busy || !canControl || !fresh(detail) || detail.snapshot?.updates?.protocol !== 1} onClick={() => void sendOperations([{ rig: detail, operation: { action: "update_check" } }])}>{t("Vérifier ce rig", "Check this rig")}</button><button type="button" className="fleet-btn fleet-primary" disabled={busy || !canControl || !!fleetUpdatePreflight(detail)} onClick={() => setUpdateReview([detail.rig_id])}>{t("Mettre à jour ce rig…", "Update this rig…")}</button></div><div className="fleet-callout warning">{t("La mise à jour interrompt le minage et redémarre MinerDesk. Les plannings locaux reprennent selon leurs propres règles après le redémarrage.", "Updating interrupts mining and restarts MinerDesk. Local schedules resume according to their own rules after restart.")}</div></>}
        {detailTab === "logs" && activeLogReceipt && ["failed", "expired"].includes(activeLogReceipt.state) && <div className="fleet-callout error" role="alert">{receiptLabel(activeLogReceipt,t)} · {activeLogReceipt.error || t("Le journal n’a pas pu être reçu.", "The log could not be retrieved.")}</div>}
        {!fresh(detail) && <div className="fleet-callout warning">{unavailableReason(detail,t)}</div>}
        {detailTab === "profiles" && <>
          <div className="fleet-kpis fleet-detail-kpis" aria-label={t("Résumé du rig", "Rig summary")}>
            <div className="fleet-kpi"><div className="fleet-kpi-label">{t("Mineurs actifs", "Active miners")}</div><div className="fleet-kpi-value">{fresh(detail) ? detailSummary!.activeMiners : "—"}<span> / {detail.snapshot?.status.length ?? "—"}</span></div></div>
            <div className="fleet-kpi"><div className="fleet-kpi-label">{t("Puissance GPU totale", "Total GPU power")}</div><div className="fleet-kpi-value">{detailSummary!.power === null ? "—" : `${detailSummary!.power.toFixed(0)} W`}</div></div>
            <div className="fleet-kpi"><div className="fleet-kpi-label">{t("Température max.", "Max temperature")}</div><div className="fleet-kpi-value">{detailSummary!.temperature === null ? "—" : `${detailSummary!.temperature.toFixed(0)} °C`}</div></div>
            <div className="fleet-kpi"><div className="fleet-kpi-label">{t("Shares A/R", "Shares A/R")}</div><div className="fleet-kpi-value">{detailSummary!.accepted === null ? "—" : `${detailSummary!.accepted} / ${detailSummary!.rejected}`}</div></div>
          </div>
          <div className="fleet-section-head"><h3>{t("Tableau de bord des mineurs", "Miner dashboard")}</h3><small>{t("Actualisation automatique toutes les 5 s · derniers états reçus", "Refreshes automatically every 5s · latest agent snapshots")}</small></div>
          {detail.snapshot?.status.length ? <div className="fleet-table-wrap fleet-miner-table-wrap" role="region" aria-label={t("Mesures et commandes des mineurs", "Miner readings and controls")} tabIndex={0}><table role="table" className="fleet-table fleet-miner-table" aria-label={t("Tableau de bord des mineurs", "Miner dashboard")}>
            <thead role="rowgroup"><tr role="row"><th role="columnheader">{t("Profil", "Profile")}</th><th role="columnheader">{t("Moteur", "Engine")}</th><th role="columnheader">{t("Algo / GPU", "Algo / GPU")}</th><th role="columnheader">{t("État", "State")}</th><th role="columnheader">{t("Débit", "Hashrate")}</th><th role="columnheader">W</th><th role="columnheader">°C</th><th role="columnheader">{t("Ventilateur %", "Fan %")}</th><th role="columnheader">A/R</th><th role="columnheader">{t("Durée", "Uptime")}</th><th role="columnheader">{t("Actions", "Actions")}</th></tr></thead>
            <tbody role="rowgroup">{detail.snapshot.status.map(miner => {
              const current = fresh(detail), profile = detail.config?.profiles.find(p => p.id === miner.id);
              return <tr role="row" key={miner.id}>
                <td role="cell" data-label={t("Profil", "Profile")}><strong>{miner.name}</strong>{profile?.pool && <small>{profile.pool}</small>}{miner.last_error && <small className="fleet-profile-error">{miner.last_error}</small>}</td>
                <td role="cell" data-label={t("Moteur", "Engine")}>{miner.engine}</td><td role="cell" data-label={t("Algorithme / GPU", "Algorithm / GPU")}>{miner.algorithm || "—"}<small>GPU {miner.gpu_ids || t("Tous", "All")}</small></td>
                <td role="cell" data-label={t("État", "State")}><Badge kind={current && miner.running ? "good" : current ? "" : "warning"}>{current ? !miner.enabled && !miner.running ? t("Désactivé", "Disabled") : mode(miner, t) : t("Dernier état reçu", "Last reported state")}</Badge>
                  {current && miner.running && !miner.enabled && <small>{t("Profil désactivé", "Profile disabled")}</small>}
                  {current && miner.has_schedule && <small>{miner.scheduler_paused ? t("Planning en pause", "Schedule paused") : miner.scheduled_now ? t("Dans le créneau planifié", "Within schedule") : t("Hors créneau planifié", "Outside schedule")}</small>}
                  {current && miner.dev_tip_active && <small className="fleet-dev-tip">♥ {t("Soutien développeur actif", "Developer tip active")}</small>}
                  <small>{authText(miner.root_authorization, t)}</small></td>
                <td role="cell" data-label={t("Débit", "Hashrate")}>{current ? fleetHashrate(miner.metrics.hashrate_hps) : "—"}</td>
                <td role="cell" data-label={t("Puissance W", "Power W")}>{current && miner.metrics.power_w !== null ? miner.metrics.power_w.toFixed(0) : "—"}</td>
                <td role="cell" data-label={t("Température °C", "Temperature °C")}>{current && miner.metrics.temperature_c !== null ? miner.metrics.temperature_c.toFixed(0) : "—"}</td>
                <td role="cell" data-label={t("Ventilation %", "Fan %")}>{current && miner.metrics.fan_pct !== null ? miner.metrics.fan_pct.toFixed(0) : "—"}</td>
                <td role="cell" data-label={t("Shares A/R", "Shares A/R")}>{current ? `${miner.metrics.accepted}/${miner.metrics.rejected}` : "—"}</td>
                <td role="cell" data-label={t("Durée", "Uptime")}>{fleetUptime(current ? miner.uptime_seconds : null)}</td>
                <td role="cell" data-label={t("Commandes", "Controls")} className="fleet-miner-actions"><div className="fleet-actions">{profileActions(detail, miner)}<button type="button" className="fleet-btn fleet-small" disabled={busy || !canControl || !current || miner.running || !profile} onClick={() => { const before = profile!; setEditor({ rig: detail, config: detail.config!, before, draft: { ...before } }); }}>{t("Modifier", "Edit")}</button></div></td>
              </tr>;
            })}</tbody>
          </table></div> : <p>{t("Aucun état de profil reçu.", "No profile status received.")}</p>}
          <div className="fleet-callout">{t("Démarrer et Redémarrer respectent le planning. Forcer crée une dérogation explicite pour cette session. Les autorisations root se valident sur le rig.", "Start and Restart respect schedules. Force explicitly overrides them for this session. Root approvals are granted on the rig.")}</div>
          <small>{t("Arrêtez un profil avant d’en modifier les paramètres d’exécution. Les mesures de puissance viennent des moteurs, pas de la prise.", "Stop a profile before editing its execution settings. Power readings come from the engines, not the wall outlet.")}</small>
        </>}
        {detailTab === "gpu" && <>{detail.snapshot?.gpu_inventory_available && detail.snapshot.gpu_inventory.length ? <div className="fleet-info-grid">{detail.snapshot.gpu_inventory.map(g => <div key={g.selector}><div className="fleet-info-label">GPU {g.selector}</div><div className="fleet-info-value">{g.name}<br/>{g.pci_bus || "—"}</div></div>)}</div> : <div className="fleet-callout">{t("L’agent n’a pas d’inventaire GPU vérifié en cache. Aucun GPU ou identifiant n’est déduit des indices du système.", "The agent has no verified cached GPU inventory. No GPU or selector is inferred from system indices.")}</div>}<div className="fleet-info-grid">{detail.snapshot?.status.map(m => <div key={m.id}><div className="fleet-info-label">{m.name} · {m.algorithm}</div><div className="fleet-info-value">{fresh(detail) ? `${fleetHashrate(m.metrics.hashrate_hps)} · ${m.metrics.power_w === null ? '—' : m.metrics.power_w + ' W GPU'} · ${m.metrics.temperature_c === null ? '—' : m.metrics.temperature_c + ' °C'}` : t("Mesures périmées", "Stale readings")}</div></div>)}</div><small>{t("Les mesures rapportées par les moteurs ne sont pas une mesure à la prise.", "Engine-reported readings are not wall-power measurements.")}</small></>}
        {detailTab === "schedules" && <><div className="fleet-section-head"><h3>{t("Planning local", "Local schedules")}</h3><button type="button" className="fleet-btn fleet-small" disabled={busy || !canControl || !fresh(detail) || !detail.config} onClick={() => setScheduleEditor({ rig: detail, config: detail.config!, schedules: detail.config!.schedules.map(s => ({ ...s, days: [...s.days], miner_ids: [...s.miner_ids] })) })}>{t("Modifier les plannings", "Edit schedules")}</button></div><div className="fleet-profile-list">{detail.config?.schedules.map(s => <div className="fleet-profile" key={s.id}><div><h3>{s.name}</h3><small>{s.start}–{s.end} · {t("jours", "days")} {s.days.join(', ')} · {detail.config!.timezone}</small><small>{s.miner_ids.length ? s.miner_ids.map(id => detail.config!.profiles.find(p => p.id === id)?.name || id).join(', ') : t("Aucun profil associé · inactif", "No profiles assigned · inactive")}</small></div><Badge kind={s.enabled ? "good" : ""}>{s.enabled ? t("Actif", "Enabled") : t("Désactivé", "Disabled")}</Badge></div>)}{!detail.config ? <p>{t("Configuration non confirmée : plannings indisponibles.", "Unconfirmed configuration: schedules unavailable.")}</p> : !detail.config.schedules.length && <p>{t("Aucun planning enregistré sur ce rig.", "No schedule saved on this rig.")}</p>}</div><small>{t("Les horaires utilisent le fuseau et l’horloge de l’agent, pas ceux de ce navigateur.", "Schedules use the agent's clock and timezone, not this browser's.")}</small></>}
        {detailTab === "logs" && <><div className="fleet-toolbar"><label className="fleet-field fleet-search">{t("Profil", "Profile")}<select value={logsProfile} onChange={e => { setLogsProfile(e.target.value); setLogs(null); setLogReceipt(null); }}><option value="">{t("Choisir un profil", "Select profile")}</option>{detail.snapshot?.status.map(m => <option key={m.id} value={m.id}>{m.name}</option>)}</select></label><button type="button" className="fleet-btn" disabled={busy || !canControl || !fresh(detail) || !logsProfile} onClick={() => void loadLogs(detail)}>{t("Charger le journal", "Load logs")}</button></div>{logs ? <pre className="fleet-logs">{logs.lines.map(line => `${new Date(millis(line.ts)).toLocaleTimeString()} [${line.stream}] ${line.text}`).join('\n') || t("Journal vide", "Empty log")}</pre> : <div className="fleet-callout">{logReceipt ? t("Demande envoyée ; le résultat apparaîtra à réception de l’agent.", "Request queued; the result appears after the agent responds.") : t("Le journal est chargé uniquement à votre demande. Les données sensibles sont masquées par l’agent.", "Logs load only when requested. The agent redacts sensitive data.")}</div>}</>}
      </div></section>}
    {results.length > 0 && <section className="fleet-detail" aria-label={t("Résultats des commandes", "Command results")}><header className="fleet-detail-head"><h2>{t("Résultats par rig", "Results by rig")}</h2><button type="button" className="fleet-btn fleet-small" onClick={() => setResults([])}>{t("Masquer", "Dismiss")}</button></header><div className="fleet-detail-body fleet-results"><ul>{results.map((r, index) => { const receipt = view?.commands.find(c => c.id === r.receipt?.id) || r.receipt; return <li key={r.receipt?.id || index}><div className="fleet-section-head"><strong>{r.name}</strong><Badge kind={r.error || receipt?.state === "failed" || receipt?.state === "expired" ? "error" : receipt?.state === "succeeded" ? "good" : ""}>{r.error ? t("Requête refusée", "Request failed") : receipt ? receiptLabel(receipt, t) : "—"}</Badge></div>{(r.error || receipt?.error) && <p className="fleet-error-text">{r.error || receipt?.error}</p>}{receipt?.result !== null && receipt?.result !== undefined && receipt.state === "succeeded" && <div><p>{r.updateCheck ? t("Vérification reçue. Consultez la version proposée dans le dernier état du rig ; aucune installation n’a été demandée.", "Check received. See the offered version in the latest rig snapshot; no installation was requested.") : resultSummary(receipt.result,t)}</p><details><summary>{t("Détails du diagnostic", "Diagnostic details")}</summary><pre className="fleet-logs">{typeof receipt.result === "string" ? receipt.result : JSON.stringify(receipt.result, null, 2)}</pre></details></div>}</li>; })}</ul></div></section>}
    {(error || notice) && <div className={`fleet-callout ${error ? 'error' : 'success'}`} style={{ marginTop: 15 }} role={error ? "alert" : "status"}>{error || notice}</div>}
    {updateReview && view && <UpdateReview rigs={rigs} rigIds={updateReview} coordinatorId={view.coordinator_rig_id || (view.setup.mode === "hub" ? view.setup.rig_id : "")} t={t} busy={busy} canControl={canControl} error={error} onClose={() => setUpdateReview(null)} onConfirm={(ids, version) => void startRollout(ids, version)}/>}
    {review && view && <Dialog t={t} title={t("Vérifier les actions groupées", "Review bulk actions")} busy={busy} onClose={() => setReview(null)} footer={<><button type="button" className="fleet-btn" disabled={busy} onClick={() => setReview(null)}>{t("Annuler", "Cancel")}</button><button type="button" className="fleet-btn fleet-primary" disabled={busy || !review.targets.some(target => { const r = rigs.find(x => x.rig_id === target.rig_id); return r && !fleetPreflight(r, target.profile_id, review.action); })} onClick={() => void sendOperations(review.targets.flatMap(target => { const rig = rigs.find(r => r.rig_id === target.rig_id); return rig && !fleetPreflight(rig, target.profile_id, review.action) ? [{ rig, operation: { action: review.action, profile_id: target.profile_id } }] : []; }))}>{busy ? t("Envoi…", "Sending…") : t("Confirmer les rigs prêts", "Confirm ready rigs")}</button></>}>
      <label className="fleet-field">{t("Action", "Action")}<select value={review.action} disabled={busy} onChange={e => setReview({ ...review, action: e.target.value as FleetAction })}>{(["start", "stop", "restart", "resume", "force_start"] as FleetAction[]).map(a => <option key={a} value={a}>{actionLabel(a, t)}</option>)}</select></label>
      <div className={`fleet-callout ${review.action === "force_start" ? 'warning' : ''}`}>{review.action === "force_start" ? t("Dérogation explicite : les profils seront lancés hors planning pour cette session. Les plannings enregistrés restent inchangés.", "Explicit override: profiles will run outside schedules for this session. Saved schedules remain unchanged.") : t("Les démarrages et redémarrages respectent les plannings locaux. Les rigs exclus ne reçoivent aucune commande.", "Starts and restarts respect local schedules. Excluded rigs receive no command.")}</div>
      <div className="fleet-review-list">{review.targets.map((target, index) => { const r = rigs.find(x => x.rig_id === target.rig_id); if (!r) return null; const exclusion = fleetPreflight(r, target.profile_id, review.action); return <div className="fleet-review-row" key={r.rig_id}><div><strong>{r.name}</strong><small>{r.snapshot?.os || "—"}</small></div><div><label className="fleet-field">{t("Profil local", "Local profile")}<select value={target.profile_id} disabled={busy} onChange={e => setReview({ ...review, targets: review.targets.map((old, i) => i === index ? { ...old, profile_id: e.target.value } : old) })}><option value="">{t("Choisir", "Select")}</option>{r.snapshot?.status.map(m => <option value={m.id} key={m.id}>{m.name}</option>)}</select></label><small>{exclusion ? reason(exclusion, t) : t("Prêt · validation finale par l’agent", "Ready · agent validates before execution")}</small></div></div>; })}</div>
    </Dialog>}
    {editor && <ProfileEditor editor={editor} t={t} busy={busy} error={error} onClose={() => setEditor(null)} onChange={setEditor} onSave={patch => void sendOperations([{ rig: editor.rig, operation: { action: "profile_patch", profile_id: editor.before.id, expected_revision: editor.before.revision, patch } }])}/>}
    {scheduleEditor && <SchedulesEditor value={scheduleEditor} t={t} busy={busy} error={error} onClose={() => setScheduleEditor(null)} onChange={setScheduleEditor} onSave={() => void sendOperations([{ rig: scheduleEditor.rig, operation: { action: "schedules_patch", expected_revision: scheduleEditor.config.revision, schedules: scheduleEditor.schedules } }])}/>}
    {invitation && <Dialog t={t} title={t("Associer une nouvelle machine", "Pair another machine")} busy={busy} onClose={() => setInvitation(null)} footer={<button type="button" className="fleet-btn" onClick={() => setInvitation(null)}>{t("Fermer et masquer la clé", "Close and hide key")}</button>}><p>{t("Sur la machine à ajouter, ouvrez Flotte → Rejoindre et utilisez ces valeurs. Cette clé est temporaire et ne sera pas incluse dans les états de flotte.", "On the machine to add, open Fleet → Join and use these values. This temporary key is never included in fleet status responses.")}</p><div><div className="fleet-info-label">{t("Adresse", "Address")}</div><div className="fleet-info-value">{invitation.address}</div></div><div><div className="fleet-info-label">{t("Empreinte TLS", "TLS fingerprint")}</div><div className="fleet-info-value">{invitation.certificate_fingerprint}</div></div><div><div className="fleet-info-label">{t("Clé d’association à usage unique", "One-time pairing key")}</div><div className="fleet-key">{invitation.key}</div></div><small>{t("Expiration", "Expires")} · {new Date(millis(invitation.expires_at)).toLocaleString()}</small><button type="button" className="fleet-btn fleet-primary" disabled={Date.now() >= millis(invitation.expires_at)} onClick={() => { if (typeof navigator !== "undefined" && navigator.clipboard?.writeText) void navigator.clipboard.writeText(invitation.key).then(() => setNotice(t("Clé copiée.", "Key copied."))).catch(() => setNotice(t("Copiez la clé manuellement depuis le champ.", "Copy the key manually from the field."))); else setNotice(t("Copiez la clé manuellement depuis le champ.", "Copy the key manually from the field.")); }}>{t("Copier la clé", "Copy key")}</button></Dialog>}
    {revokeRig && <Dialog t={t} title={t(`Révoquer l’accès de ${revokeRig.name} ?`, `Revoke ${revokeRig.name}'s access?`)} busy={busy} onClose={() => setRevokeRig(null)} footer={<><button type="button" className="fleet-btn" disabled={busy} onClick={() => setRevokeRig(null)}>{t("Annuler", "Cancel")}</button><button type="button" className="fleet-btn fleet-danger" disabled={busy || !canRevoke(revokeRig)} onClick={() => void mutate<FleetView>(`/api/fleet/rigs/${encodeURIComponent(revokeRig.rig_id)}/revoke`, json({}), result => {
      const next = decodeFleetView(result);
      setView(next); setDetailId(id => id === revokeRig.rig_id ? null : id);
      setSelected(ids => ids.filter(id => id !== revokeRig.rig_id)); setRevokeRig(null);
      setNotice(t("Accès révoqué. Le rig continue son fonctionnement local sans accès à ce coordinateur.", "Access revoked. The rig continues local operation without access to this coordinator."));
    })}>{t("Révoquer l’accès", "Revoke access")}</button></>}><p>{t("L’accès de ce rig au coordinateur sera révoqué et il sera retiré de cette flotte. Son minage local et ses plannings continuent ; aucune commande d’arrêt n’est envoyée.", "This rig's coordinator access will be revoked and it will be removed from this fleet. Local mining and schedules continue; no stop command is sent.")}</p><div className="fleet-callout warning">{t("Pour le reconnecter, il faudra une nouvelle association avec une clé temporaire.", "Reconnecting requires a new pairing with a temporary key.")}</div></Dialog>}
    {disconnect && view && <Dialog t={t} title={t("Dissocier cette flotte ?", "Disconnect this fleet?")} busy={busy} onClose={() => setDisconnect(false)} footer={<><button type="button" className="fleet-btn" onClick={() => setDisconnect(false)} disabled={busy}>{t("Annuler", "Cancel")}</button><button type="button" className="fleet-btn fleet-danger" disabled={busy || !localSetup} onClick={() => void mutate<FleetView>(view.setup.mode === "hub" ? "/api/fleet/disable" : "/api/fleet/leave", json({}), result => { setView(decodeFleetView(result)); setDisconnect(false); setInvitation(null); })}>{t("Confirmer la déconnexion", "Confirm disconnect")}</button></>}><p>{t("La connexion de flotte sera supprimée sur cette machine. Les agents restent responsables de leurs profils et plannings locaux.", "This machine's fleet connection will be removed. Agents remain responsible for their local profiles and schedules.")}</p></Dialog>}
  </section>;
}

function RolloutStatus({ rollout, rigs, t, busy, canControl, onCancel }: { rollout: FleetRollout; rigs: FleetRig[]; t: Text; busy: boolean; canControl: boolean; onCancel: () => void }) {
  const active = ["running", "waiting_for_reconnect"].includes(rollout.state);
  return <section className="fleet-rollout" aria-label={t(`Déploiement ${rollout.expected_version}`, `Rollout ${rollout.expected_version}`)}>
    <div className="fleet-section-head"><div><h3>{t("Déploiement vers", "Rollout to")} {rollout.expected_version}</h3><small>{rollout.state === "completed" ? t("Toutes les versions ont été confirmées après reconnexion.", "All versions were confirmed after reconnect.") : updateStage(rollout.state, t)}</small></div>
      {active && rollout.rigs.some(r => r.state === "queued") && <button type="button" className="fleet-btn fleet-small" disabled={busy || !canControl} onClick={onCancel}>{t("Annuler le restant", "Cancel remaining")}</button>}
    </div><ol className="fleet-rollout-list">{rollout.rigs.map(item => <li key={item.rig_id}><div><strong>{rigs.find(r => r.rig_id === item.rig_id)?.name || item.rig_id}</strong><small>{item.rig_id}</small></div><div><Badge kind={item.state === "updated" ? "good" : item.state === "failed" ? "error" : ""}>{updateStage(item.state, t)}</Badge>{item.error && <small className="fleet-error-text">{item.error}</small>}</div></li>)}</ol>
    {rollout.error && <p className="fleet-error-text">{rollout.error}</p>}
    <small>{t("L’annulation concerne uniquement les rigs encore en file. Une installation déjà en cours continue. Une commande acceptée ne confirme jamais la version installée.", "Cancellation applies only to rigs still queued. An installation already in progress continues. An accepted command never confirms the installed version.")}</small>
  </section>;
}

function UpdateReview({ rigs, rigIds, coordinatorId, t, busy, canControl, error, onClose, onConfirm }: { rigs: FleetRig[]; rigIds: string[]; coordinatorId: string; t: Text; busy: boolean; canControl: boolean; error: string; onClose: () => void; onConfirm: (ids: string[], version: string) => void }) {
  const targets = rigIds.map(id => rigs.find(r => r.rig_id === id)).filter((r): r is FleetRig => !!r)
    .sort((a, b) => Number(a.rig_id === coordinatorId) - Number(b.rig_id === coordinatorId));
  const versions = [...new Set(targets.flatMap(r => r.snapshot?.updates?.available_version ? [r.snapshot.updates.available_version] : []))];
  const [version, setVersion] = useState(versions.length === 1 ? versions[0] : "");
  const [acknowledged, setAcknowledged] = useState("");
  const ready = version ? targets.filter(r => !fleetUpdatePreflight(r, version)) : [];
  const signature = JSON.stringify([version, ready.map(r => r.rig_id)]);
  useEffect(() => setAcknowledged(""), [signature]);
  const confirmed = acknowledged === signature;
  return <Dialog t={t} title={t("Vérifier la mise à jour distante", "Review remote update")} busy={busy} onClose={onClose} footer={<><button type="button" className="fleet-btn" disabled={busy} onClick={onClose}>{t("Annuler", "Cancel")}</button><button type="button" className="fleet-btn fleet-primary" disabled={busy || !canControl || !ready.length || !confirmed} onClick={() => onConfirm(ready.map(r => r.rig_id), version)}>{t("Confirmer le déploiement", "Confirm rollout")}</button></>}>
    <label className="fleet-field">{t("Version cible de MinerDesk", "Target MinerDesk version")}<select value={version} disabled={busy} onChange={e => { setVersion(e.target.value); setAcknowledged(""); }}><option value="">{t("Choisir la version vérifiée", "Select the checked version")}</option>{versions.map(v => <option key={v} value={v}>{v}</option>)}</select></label>
    <div className="fleet-callout warning">{t("Le minage sera interrompu sur chaque rig mis à jour et MinerDesk redémarrera. Les mises à jour s’exécutent une par une, le coordinateur en dernier. Le déploiement attend la reconnexion et la version attendue avant de passer au suivant ; le premier échec arrête le restant.", "Mining will be interrupted on each updated rig and MinerDesk will restart. Updates run one at a time, coordinator last. The rollout waits for reconnect and the expected version before continuing; the first failure stops the remaining rollout.")}</div>
    <h3>{t("Rigs examinés", "Reviewed rigs")} · {ready.length} {t("prêt(s)", "ready")}</h3>
    <div className="fleet-review-list">{targets.map(rig => { const exclusion = version ? fleetUpdatePreflight(rig, version) : "check_required"; return <div className="fleet-review-row" key={rig.rig_id}><div><strong>{rig.name}</strong><small>{rig.rig_id}</small><small>{rig.snapshot?.version || "—"} → {version || "—"}{rig.rig_id === coordinatorId ? t(" · coordinateur en dernier", " · coordinator last") : ""}</small></div><div><Badge kind={exclusion ? "warning" : "good"}>{exclusion ? t("Exclu", "Excluded") : t("Inclus", "Included")}</Badge><small>{exclusion ? updateReason(exclusion, t) : t("Prêt · dernière vérification par l’agent avant installation", "Ready · agent checks again before installation")}</small></div></div>; })}</div>
    {rigIds.some(id => !targets.some(r => r.rig_id === id)) && <p className="fleet-error-text">{t("Un rig sélectionné n’est plus associé et sera exclu.", "A selected rig is no longer paired and will be excluded.")}</p>}
    <label className="fleet-check fleet-update-confirm"><input type="checkbox" checked={confirmed} disabled={busy || !ready.length} onChange={e => setAcknowledged(e.target.checked ? signature : "")}/>{t(`Je confirme la version ${version || "—"} pour les ${ready.length} rig(s) inclus ci-dessus et l’interruption de leur minage.`, `I confirm version ${version || "—"} for the ${ready.length} included rig(s) listed above and the interruption of their mining.`)}</label>
    {error && <p className="fleet-error-text" role="alert">{error}</p>}
  </Dialog>;
}

function ProfileEditor({ editor, t, busy, error, onClose, onChange, onSave }: { editor: Editor; t: Text; busy: boolean; error: string; onClose: () => void; onChange: (e: Editor) => void; onSave: (patch: FleetProfilePatch) => void }) {
  const patch = fleetEditablePatch(editor.config, editor.before, editor.draft);
  const allowed = (key: keyof FleetProfile) => editor.config.editable_profile_fields.includes(key);
  const set = (key: keyof FleetProfilePatch, value: unknown) => onChange({ ...editor, draft: { ...editor.draft, [key]: value } });
  const strings: [keyof FleetProfilePatch, string][] = [["name",t("Nom","Name")],["algorithm",t("Algorithme","Algorithm")],["pool","Pool"],["wallet",t("Adresse publique","Public address")],["worker","Worker"],["gpu_ids",t("Identifiants GPU du moteur","Engine GPU selectors")]];
  const numbers: [keyof FleetProfilePatch, string][] = [["core_clock",t("GPU core MHz","GPU core MHz")],["memory_clock",t("Mémoire MHz / lock","Memory MHz / lock")],["core_offset",t("Offset core MHz","Core offset MHz")],["memory_offset",t("Offset mémoire MHz","Memory offset MHz")],["power_limit",t("Puissance W","Power W")],["fan",t("Ventilation %","Fan %")]];
  return <Dialog t={t} title={t(`Modifier ${editor.before.name}`, `Edit ${editor.before.name}`)} busy={busy} onClose={onClose} footer={<><button type="button" className="fleet-btn" disabled={busy} onClick={onClose}>{t("Annuler", "Cancel")}</button><button type="button" className="fleet-btn fleet-primary" disabled={busy || !Object.keys(patch).length || !editor.draft.name.trim()} onClick={() => onSave(patch)}>{t("Enregistrer cette révision", "Save this revision")}</button></>}>
    <div className="fleet-callout">{editor.rig.name} · {editor.before.engine}<br/>{t("Le moteur, le chemin local, les arguments avancés et les autorisations ne sont pas modifiables depuis cette page. L’agent refuse les révisions périmées.", "Engine, local path, advanced arguments and grants cannot be changed here. The agent rejects stale revisions.")}</div>
    {editor.before.gpu_tuning.length > 0 && <div className="fleet-callout">{t("Les réglages GPU ci-dessous sont les valeurs communes / legacy. Les overrides par GPU existants sont conservés et peuvent avoir priorité.", "The GPU settings below are shared / legacy defaults. Existing per-GPU overrides are preserved and may take priority.")}</div>}
    <div className="fleet-editor-grid">{strings.map(([key,label]) => <label key={key} className={`fleet-field ${key === "pool" || key === "wallet" ? 'fleet-wide' : ''}`}>{label}<input value={String(editor.draft[key] ?? "")} disabled={busy || !allowed(key)} onChange={e => set(key,e.target.value)}/></label>)}
      {numbers.map(([key,label]) => <label key={key} className="fleet-field">{label}<input type="number" value={editor.draft[key] as number | null ?? ""} disabled={busy || !allowed(key)} onChange={e => set(key,e.target.value === "" ? null : Number(e.target.value))}/></label>)}
    </div><label className="fleet-check"><input type="checkbox" checked={editor.draft.enabled} disabled={busy || !allowed("enabled")} onChange={e => set("enabled",e.target.checked)}/>{t("Profil activé", "Profile enabled")}</label>
    <div className="fleet-callout warning">{t("Aucun lancement automatique. Les champs vides omettent les overrides ; ils ne réinitialisent pas le pilote. Si la commande root change, une nouvelle validation locale sera nécessaire.", "No automatic start. Blank fields omit overrides; they do not reset the driver. A changed root command requires local approval again.")}</div>
    {error && <p className="fleet-error-text" role="alert">{error}</p>}
  </Dialog>;
}
function SchedulesEditor({ value, t, busy, error, onClose, onChange, onSave }: { value: ScheduleEditor; t: Text; busy: boolean; error: string; onClose: () => void; onChange: (e: ScheduleEditor) => void; onSave: () => void }) {
  const update = (index: number, patch: Partial<ScheduleWindow>) => onChange({ ...value, schedules: value.schedules.map((s,i) => i === index ? { ...s, ...patch } : s) });
  const valid = value.schedules.every(s => s.name.trim() && s.days.length && /^\d{2}:\d{2}$/.test(s.start) && /^\d{2}:\d{2}$/.test(s.end) && (!s.enabled || s.miner_ids.length > 0));
  return <Dialog t={t} title={t(`Plannings de ${value.rig.name}`, `${value.rig.name} schedules`)} busy={busy} onClose={onClose} footer={<><button type="button" className="fleet-btn" disabled={busy} onClick={onClose}>{t("Annuler", "Cancel")}</button><button type="button" className="fleet-btn fleet-primary" disabled={busy || !valid} onClick={onSave}>{t("Enregistrer les plannings", "Save schedules")}</button></>}>
    <div className="fleet-callout">{t("Fuseau de l’agent", "Agent timezone")} : {value.config.timezone} · {value.config.local_time}<br/>{t("L’agent vérifie la révision et applique son propre planning. Les autorisations locales restent requises.", "The agent checks the revision and runs its own schedule. Local authorizations remain required.")}</div>
    {value.schedules.map((s,index) => <section className="fleet-editor" key={s.id}><div className="fleet-section-head"><label className="fleet-check"><input type="checkbox" checked={s.enabled} disabled={busy} onChange={e => update(index,{enabled:e.target.checked})}/>{t("Actif", "Enabled")}</label><button type="button" className="fleet-btn fleet-small fleet-danger" disabled={busy} onClick={() => onChange({ ...value, schedules:value.schedules.filter((_,i)=>i!==index) })}>{t("Retirer", "Remove")}</button></div>
      <label className="fleet-field">{t("Nom", "Name")}<input value={s.name} disabled={busy} onChange={e => update(index,{name:e.target.value})}/></label><div className="fleet-editor-grid"><label className="fleet-field">{t("Début", "Start")}<input type="time" value={s.start} disabled={busy} onChange={e => update(index,{start:e.target.value})}/></label><label className="fleet-field">{t("Fin", "End")}<input type="time" value={s.end} disabled={busy} onChange={e => update(index,{end:e.target.value})}/></label></div>
      <div className="fleet-days" aria-label={t("Jours", "Days")}>{(t("Lun,Mar,Mer,Jeu,Ven,Sam,Dim","Mon,Tue,Wed,Thu,Fri,Sat,Sun").split(',')).map((name,i) => <button type="button" className="fleet-btn fleet-small" key={i} aria-pressed={s.days.includes(i+1)} disabled={busy} onClick={() => update(index,{days:s.days.includes(i+1)?s.days.filter(d=>d!==i+1):[...s.days,i+1].sort()})}>{name}</button>)}</div>
      <label className="fleet-check"><input type="checkbox" checked={value.config.profiles.length>0&&value.config.profiles.every(p=>s.miner_ids.includes(p.id))} disabled={busy} onChange={e => update(index,{miner_ids:e.target.checked?value.config.profiles.map(p=>p.id):[]})}/>{t("Tous les profils de ce rig", "All profiles on this rig")}</label>
      {value.config.profiles.map(p => <label className="fleet-check" key={p.id}><input type="checkbox" checked={s.miner_ids.includes(p.id)} disabled={busy} onChange={e=>update(index,{miner_ids:e.target.checked?[...s.miner_ids,p.id]:s.miner_ids.filter(id=>id!==p.id)})}/>{p.name}</label>)}
      {s.miner_ids.length===0 && <p className={s.enabled?"fleet-error-text":"fleet-rig-line"}>{s.enabled?t("Choisissez au moins un profil ou désactivez ce créneau. Aucun profil n’est protégé par ce planning.","Choose at least one profile or disable this window. This schedule does not cover any profile."):t("Aucun profil associé · créneau inactif.","No profiles assigned · inactive window.")}</p>}
    </section>)}
    <button type="button" className="fleet-btn" disabled={busy || !value.config.profiles.length} onClick={() => onChange({ ...value, schedules:[...value.schedules,{id:crypto.randomUUID(),name:t("Nouveau créneau","New window"),enabled:false,days:[1,2,3,4,5,6,7],start:"22:00",end:"06:00",miner_ids:value.config.profiles.slice(0,1).map(p=>p.id),post_action:"none",countdown_seconds:60,require_confirmation:true,wake_enabled:false,wake_minutes_before:5}] })}>{t("Ajouter un créneau désactivé", "Add disabled window")}</button>
    {error && <p className="fleet-error-text" role="alert">{error}</p>}
  </Dialog>;
}
