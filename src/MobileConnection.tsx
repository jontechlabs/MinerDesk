import { useEffect, useRef, useState, type FormEvent } from "react";
import App from "./App";
import { mobileConnect, mobileConnectionStatus, mobileDisconnect, type MobileSession } from "./runtime";

const addressKey = "minerdesk.mobile.address";
export function mobileSavedAddress(value: string): string {
  try {
    const url = new URL(value.trim());
    return ["http:", "https:"].includes(url.protocol) && !url.username && !url.password ? url.origin : "";
  } catch { return ""; }
}
function activeSession(value: MobileSession): value is MobileSession & { connectionId: string; address: string } {
  return value.connected === true && typeof value.connectionId === "string" && value.connectionId.length > 0
    && value.connectionId.length <= 128 && typeof value.address === "string" && mobileSavedAddress(value.address) !== "";
}

/** The native process owns the access key for this session. Only a sanitized
 * address and the language are remembered by the WebView between launches.
 */
export default function MobileConnection() {
  const [session, setSession] = useState<MobileSession | null>(null);
  const [address, setAddress] = useState(() => mobileSavedAddress(localStorage.getItem(addressKey) || ""));
  const [key, setKey] = useState("");
  const [allowInsecureLan, setAllowInsecureLan] = useState(false);
  const [language, setLanguage] = useState(localStorage.getItem("minerdesk.language") || "en");
  const [checking, setChecking] = useState(true), [busy, setBusy] = useState(false), [error, setError] = useState("");
  const [reviewDisconnect, setReviewDisconnect] = useState(false);
  const operation = useRef(0);
  const dialog = useRef<HTMLElement>(null), dialogBusy = useRef(busy); dialogBusy.current = busy;
  const fr = language === "fr";
  const t = (english: string, french: string) => fr ? french : english;
  useEffect(() => {
    let alive = true;
    void mobileConnectionStatus().then(value => {
      if (!alive) return;
      if (activeSession(value)) setSession(value);
      else if (value.connecting) {
        setBusy(true);
        setError(t("A previous connection attempt is still running. Cancel it before connecting again.", "Une tentative de connexion est encore en cours. Annulez-la avant de recommencer."));
      }
    })
      .catch(() => { if (alive) setError(t("The mobile connection service is unavailable. Reopen the app.", "Le service de connexion mobile est indisponible. Rouvrez l’application.")); })
      .finally(() => { if (alive) setChecking(false); });
    return () => { alive = false; ++operation.current; };
  }, []);
  useEffect(() => {
    if (!reviewDisconnect) return;
    const previous = document.activeElement;
    dialog.current?.querySelector<HTMLButtonElement>("button")?.focus();
    const keyboard = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !dialogBusy.current) setReviewDisconnect(false);
      if (event.key !== "Tab") return;
      const buttons = dialog.current?.querySelectorAll<HTMLButtonElement>("button:not(:disabled)");
      if (!buttons?.length) { event.preventDefault(); return; }
      const first = buttons[0], last = buttons[buttons.length - 1];
      if (!dialog.current?.contains(document.activeElement) || (!event.shiftKey && document.activeElement === last)) {
        event.preventDefault(); first.focus();
      } else if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last.focus(); }
    };
    document.addEventListener?.("keydown", keyboard);
    return () => {
      document.removeEventListener?.("keydown", keyboard);
      if (typeof HTMLElement !== "undefined" && previous instanceof HTMLElement) previous.focus();
    };
  }, [reviewDisconnect]);
  async function connect(event: FormEvent) {
    event.preventDefault();
    if (busy || checking) return;
    const attempt = ++operation.current;
    setBusy(true); setError("");
    try {
      const next = await mobileConnect(address.trim(), key.trim(), allowInsecureLan);
      if (attempt !== operation.current) return;
      if (!activeSession(next)) {
        await mobileDisconnect().catch(() => undefined);
        throw new Error(t("The connection could not be verified.", "La connexion n’a pas pu être vérifiée."));
      }
      const clean = mobileSavedAddress(next.address);
      localStorage.setItem(addressKey, clean);
      localStorage.setItem("minerdesk.language", language);
      setAddress(clean); setKey(""); setSession(next);
    } catch (reason) {
      if (attempt === operation.current) setError(String(reason instanceof Error ? reason.message : reason));
    } finally { if (attempt === operation.current) setBusy(false); }
  }
  async function disconnect() {
    ++operation.current; setBusy(true); setError("");
    try {
      await mobileDisconnect();
      setSession(null); setKey(""); setReviewDisconnect(false);
    } catch { setError(t("Could not close the connection. Try again.", "Impossible de fermer la connexion. Réessayez.")); }
    finally { setBusy(false); }
  }
  if (checking) return <main className="mobile-connect-page"><div className="mobile-connect-card" role="status">{t("Preparing MinerDesk Mobile…", "Préparation de MinerDesk Mobile…")}</div></main>;
  if (session && activeSession(session)) return <div className="mobile-client-shell">
    <div className="mobile-connection-strip"><span className="mobile-connection-name"><small>{t("Remote instance", "Instance distante")}</small><strong>{session.name || session.address}</strong></span><button className="mini-btn" onClick={() => setReviewDisconnect(true)}>{t("Connections", "Connexions")}</button></div>
    {error && <div className="mobile-connect-error" role="alert">{error}</div>}
    <App key={session.connectionId} mobileConnectionId={session.connectionId}/>
    {reviewDisconnect && <div className="fleet-dialog-backdrop"><section ref={dialog} className="fleet-dialog" role="dialog" aria-modal="true" aria-labelledby="mobile-disconnect-title">
      <div className="fleet-dialog-head"><h2 id="mobile-disconnect-title">{t("Change connection?", "Changer de connexion ?")}</h2></div>
      <div className="fleet-dialog-body"><p>{t("Disconnecting this phone does not stop mining on your rigs. Unsaved edits will be discarded. Commands already sent may still finish; check the rig before trying them again.", "Déconnecter ce téléphone n’arrête pas le minage sur vos rigs. Les modifications non enregistrées seront perdues. Les commandes déjà envoyées peuvent se terminer ; vérifiez le rig avant de les relancer.")}</p></div>
      <div className="fleet-dialog-foot"><button className="fleet-btn" autoFocus disabled={busy} onClick={() => setReviewDisconnect(false)}>{t("Keep connection", "Rester connecté")}</button><button className="fleet-btn fleet-danger" disabled={busy} onClick={() => void disconnect()}>{busy ? "…" : t("Disconnect", "Déconnecter")}</button></div>
    </section></div>}
  </div>;
  return <main className="mobile-connect-page"><section className="mobile-connect-card" aria-labelledby="mobile-connect-title">
    <div className="mobile-connect-brand"><div className="brand-mark" aria-hidden="true">M</div><span>MinerDesk <small>Mobile</small></span></div>
    <h1 id="mobile-connect-title">{t("Your rigs. Wherever you are.", "Vos rigs. Où que vous soyez.")}</h1>
    <p className="mobile-connect-help">{t("Connect to an existing MinerDesk instance to view miners, manage schedules and control Fleet.", "Connectez-vous à une instance MinerDesk pour suivre les mineurs, gérer les horaires et piloter Fleet.")}</p>
    <form className="mobile-connect-form" onSubmit={event => void connect(event)}>
      <label className="field"><span>{t("MinerDesk URL", "URL MinerDesk")}</span><input type="url" inputMode="url" autoCapitalize="none" autoCorrect="off" spellCheck={false} autoComplete="off" value={address} disabled={busy} required placeholder="http://192.168.1.50:17888" onChange={event => setAddress(event.target.value)}/></label>
      <label className="field"><span>{t("Web access key", "Clé d’accès web")}</span><input type="password" autoCapitalize="none" autoCorrect="off" autoComplete="off" spellCheck={false} value={key} disabled={busy} onChange={event => setKey(event.target.value)} aria-describedby="mobile-key-help"/></label>
      <p id="mobile-key-help" className="mobile-connect-help">{t("Find the URL and access token in Settings → Web / LAN access on your PC. You can also paste its full URL containing ?token=. This is not a one-use Fleet pairing key.", "Retrouvez l’URL et le jeton dans Réglages → Accès web / LAN sur le PC. Vous pouvez aussi coller son URL complète contenant ?token=. Il ne s’agit pas d’une clé d’association Fleet à usage unique.")}</p>
      <label className="check mobile-http-warning"><input type="checkbox" checked={allowInsecureLan} disabled={busy} onChange={event => setAllowInsecureLan(event.target.checked)}/><span>{t("Allow unencrypted HTTP on my trusted local network", "Autoriser HTTP non chiffré sur mon réseau local de confiance")}</span></label>
      <p className="mobile-connect-help">{t("For access over the Internet, use HTTPS with a valid certificate or a private VPN. Public HTTP connections are refused.", "Pour un accès Internet, utilisez HTTPS avec un certificat valide ou un VPN privé. Les connexions HTTP publiques sont refusées.")}</p>
      <label className="field"><span>{t("Language", "Langue")}</span><select value={language} disabled={busy} onChange={event => {setLanguage(event.target.value);localStorage.setItem("minerdesk.language", event.target.value);}}><option value="en">English</option><option value="fr">Français</option></select></label>
      {error && <p className="mobile-connect-error" role="alert">{error}</p>}
      <button className="btn primary" type="submit" disabled={busy || !address.trim()}>{busy ? t("Connecting…", "Connexion…") : t("Connect to MinerDesk", "Se connecter à MinerDesk")}</button>
      {busy && <button type="button" className="btn ghost" onClick={() => void disconnect()}>{t("Cancel connection", "Annuler la connexion")}</button>}
    </form>
    <p className="mobile-session-note">{t("Your access key stays in memory for this app session. Only the server address is remembered; enter the key again after a cold launch. Closing the phone app does not stop your rigs.", "Votre clé reste en mémoire pendant cette session. Seule l’adresse du serveur est mémorisée ; ressaisissez la clé après un démarrage à froid. Fermer l’application mobile n’arrête pas vos rigs.")}</p>
  </section></main>;
}
