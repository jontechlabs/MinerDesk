import { useEffect, useRef, useState, type MouseEvent } from "react";
import { Channel, invoke } from "@tauri-apps/api/core";
import { tr } from "./i18n";
import { safeReleaseUrl, shouldCheckUpdate, shouldNotifyUpdate, UPDATE_SNOOZE_MS, type AppUpdate, type UpdateProgress } from "./updatePolicy";

type Props = { desktop: boolean; language: string; settings: boolean; dirty: boolean; busy: boolean; checkWeb: (force: boolean) => Promise<AppUpdate>; onInstalling: (value: boolean) => void };
const key = (name: string) => `minerdesk.updates.${name}`;
export default function AppUpdates({ desktop, language, settings, dirty, busy, checkWeb, onInstalling }: Props) {
  const t = (name: string) => tr(language, name);
  const [enabled, setEnabled] = useState(localStorage.getItem(key("enabled")) !== "false");
  const [update, setUpdate] = useState<AppUpdate | null>(null);
  const [visible, setVisible] = useState(false);
  const [confirm, setConfirm] = useState(false);
  const [checking, setChecking] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [progress, setProgress] = useState<UpdateProgress | null>(null);
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");
  const inFlight = useRef(false);
  const retryAfter = useRef(0);
  const props = useRef({checkWeb, onInstalling});
  props.current = {checkWeb, onInstalling};

  async function check(manual = false) {
    if (inFlight.current || installing) return;
    const now = Date.now();
    if (!manual && now < retryAfter.current) return;
    if (!manual && !shouldCheckUpdate(enabled, Number(localStorage.getItem(key("checked"))), now)) return;
    inFlight.current = true; setChecking(true);
    if (manual) { setMessage(""); setError(""); }
    try {
      const result = desktop ? await invoke<AppUpdate>("check_app_update", {force: manual}) : await props.current.checkWeb(manual);
      localStorage.setItem(key("checked"), String(Date.now()));
      retryAfter.current = 0;
      setUpdate(result);
      const notify = shouldNotifyUpdate(result, localStorage.getItem(key("snoozedVersion")) || "", Number(localStorage.getItem(key("snoozedUntil"))), Date.now());
      setVisible(Boolean(result.version) && (manual || notify));
      if (manual && !result.version) setMessage(t("upToDate"));
    } catch (e) {
      // Offline/rate-limited checks must not interfere with mining. Retry later.
      retryAfter.current = Date.now() + 60 * 60 * 1000;
      if (manual) setError(`${t("updateCheckFailed")} ${String(e)}`);
    } finally { inFlight.current = false; setChecking(false); }
  }
  useEffect(() => {
    // StrictMode cleanup cancels the first timer; one check runs after startup.
    const startup = window.setTimeout(() => void check(), 15000);
    const timer = window.setInterval(() => void check(), 60000);
    return () => { window.clearTimeout(startup); window.clearInterval(timer); };
  }, [enabled, desktop, installing, language]);

  function later() {
    localStorage.setItem(key("snoozedVersion"), update?.version || "");
    localStorage.setItem(key("snoozedUntil"), String(Date.now() + UPDATE_SNOOZE_MS));
    setVisible(false); setConfirm(false);
  }
  async function install() {
    if (!update?.version || !update.can_install || inFlight.current || installing || busy) return;
    if (dirty) { setError(t("updateSaveFirst")); return; }
    inFlight.current = true; setInstalling(true); props.current.onInstalling(true); setError("");
    setProgress({stage:"downloading", downloaded:0,total:null});
    const channel = new Channel<UpdateProgress>();
    channel.onmessage = setProgress;
    try {
      await invoke("install_app_update", {version: update.version, progress: channel});
    } catch (e) {
      setError(`${t("updateInstallFailed")} ${String(e)}`);
      setConfirm(false);
    } finally {
      inFlight.current = false; setInstalling(false); props.current.onInstalling(false); setProgress(null);
    }
  }
  const fallback = t(`updateReason_${update?.reason || "web"}`);
  const link = safeReleaseUrl(update?.release_url || "");
  async function openDownloads(event: MouseEvent<HTMLAnchorElement>) {
    if (!desktop) return;
    event.preventDefault();
    try { await invoke("open_github_downloads", { url: link }); }
    catch (e) { setError(`${t("updateBrowserFailed")} ${String(e)} ${link}`); }
  }
  function downloadLink(className?: string) {
    return <a className={className} href={link} target="_blank" rel="noreferrer" onClick={event => void openDownloads(event)}>{t("updateDownloadPage")}</a>;
  }
  const percent = progress?.total ? Math.min(100, Math.floor(100 * progress.downloaded / progress.total)) : null;
  return <>
    {settings && <div className="card update-settings">
      <div className="card-head"><div><h2>{t("appUpdates")}</h2><p>{t("updateHelp")}</p></div><button className="mini-btn" disabled={checking || installing || busy} onClick={() => void check(true)}>{checking ? t("updateChecking") : t("checkUpdates")}</button></div>
      <label className="windows-option"><input type="checkbox" checked={enabled} disabled={installing} onChange={e => {setEnabled(e.target.checked); localStorage.setItem(key("enabled"), String(e.target.checked));}}/><span><strong>{t("updateAutomaticChecks")}</strong><small>{t("updateCheckPrivacy")}</small></span></label>
      {message && <p role="status">{message}</p>}
      {update?.version && <p>{t("updateAvailable").replace("{version}", update.version)} {downloadLink()}</p>}
    </div>}
    {visible && update?.version && !confirm && !installing && <div className="update-banner" role="status">
      <div><strong>{t("updateAvailable").replace("{version}", update.version)}</strong><p>{update.can_install ? t("updateReady") : fallback}</p></div>
      <div className="update-actions">{update.can_install && <button className="btn primary" disabled={busy} onClick={() => {setConfirm(true); setError("");}}>{t("updateInstall")}</button>}{downloadLink("btn ghost")}<button className="mini-btn" onClick={later}>{t("updateLater")}</button></div>
    </div>}
    {error && <div className="command-error" role="alert"><div><strong>{t("appUpdates")}</strong><p>{error}</p>{downloadLink()}</div><button className="mini-btn" onClick={() => setError("")}>{t("dismissError")}</button></div>}
    {(confirm || installing) && <div className="power-modal-backdrop"><div className="power-modal update-modal" role="dialog" aria-modal="true" aria-labelledby="update-title">
      <h2 id="update-title">{t("updateAvailable").replace("{version}", update?.version || "")}</h2>
      {installing ? <><p role="status">{t(progress?.stage === "installing" ? "updateInstalling" : "updateDownloading")}{percent !== null ? ` ${percent}%` : ""}</p><progress aria-label={t("updateDownloading")} value={percent ?? undefined} max={100}/><p className="muted">{t("updateWait")}</p></> : <>
        <p>{t("updateConfirm")}</p>{dirty && <div className="notice danger-notice">{t("updateSaveFirst")}</div>}
        {update?.notes && <details className="update-notes"><summary>{t("updateReleaseNotes")}</summary><pre>{update.notes}</pre></details>}
        <div className="power-actions"><button className="btn ghost" autoFocus onClick={() => setConfirm(false)}>{t("cancel")}</button><button className="btn primary" disabled={dirty || busy} onClick={() => void install()}>{t("updateInstallRestart")}</button></div>
      </>}
    </div></div>}
  </>;
}
