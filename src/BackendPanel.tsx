import type { BackendStatus } from "./types";
import { tr } from "./i18n";

export type BackendAction = "start_privileged_backend" | "restart_privileged_backend" | "repair_privileged_backend";

export default function BackendPanel({ status, busy, language, onAction, onRefresh }: {
  status: BackendStatus;
  busy: boolean;
  language: string;
  onAction: (command: BackendAction) => void;
  onRefresh: () => void;
}) {
  const t = (key: string) => tr(language, key);
  const offlineDetail = status.supported
    ? status.task_state === "Unknown" ? t("backendChecking") : status.task_installed ? t("taskInstalled") : t("taskMissing")
    : t("desktopBackendOffline");
  return <div className={`backend-strip ${status.reachable ? "online" : "offline"}`}>
    <div className="backend-main"><span className="backend-dot"/><div>
      <strong>{t(status.supported ? "privilegedBackend" : "desktopBackend")}</strong>
      <small>{status.reachable ? `${t("backendConnected")} · 127.0.0.1:${status.port}` : `${t("backendOffline")} · ${offlineDetail}`}
        {status.listener_process && !status.reachable ? ` · ${t("portUsedBy")} ${status.listener_process}` : ""}</small>
    </div></div>
    <div className="backend-actions">
      {!status.reachable && <button className="mini-btn primary-lite" disabled={busy} onClick={() => onAction("start_privileged_backend")}>{busy ? t("startingBackend") : t("startBackend")}</button>}
      {status.supported && status.reachable && <button className="mini-btn" disabled={busy} onClick={() => onAction("restart_privileged_backend")}>{t("restartBackend")}</button>}
      {status.supported && <button className="mini-btn" disabled={busy} onClick={() => onAction("repair_privileged_backend")}>{t("repairBackend")}</button>}
      <button className="mini-btn" disabled={busy} onClick={onRefresh}>{t("refresh")}</button>
    </div>
    {!status.reachable && <details className="backend-details"><summary>{t("backendDiagnostics")}</summary>
      <div className="backend-diag-grid">
        {status.supported && <><span>{t("taskState")}</span><code>{status.task_state || "—"}</code><span>{t("lastTaskResult")}</span><code>{status.last_task_result || "—"}</code></>}
        <span>{t("backendExecutable")}</span><code>{status.executable_path || "—"}</code>
        <span>{t("listener")}</span><code>{status.listener_pid ? `${status.listener_process || "process"} · PID ${status.listener_pid}` : "—"}</code>
      </div>
      {status.log_tail && <pre className="backend-log">{status.log_tail}</pre>}
    </details>}
  </div>;
}
