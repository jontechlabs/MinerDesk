import { useEffect, useRef, useState } from "react";
import type { LinuxAuthorizationStatus, MinerProfile } from "./types";

export default function LinuxElevationControl({ profile, platform, available, language, onChange, onAuthorizationAction, local = true }: {
  profile: MinerProfile; platform?: string; available?: boolean; language: string;
  onChange: (patch: Partial<MinerProfile>) => void;
  onAuthorizationAction?: (action: "status" | "authorize" | "revoke", fingerprint?: string) => Promise<LinuxAuthorizationStatus>;
  local?: boolean;
}) {
  const [status, setStatus] = useState<LinuxAuthorizationStatus | null>(null);
  const [pending, setPending] = useState(false), [review, setReview] = useState(false), [error, setError] = useState("");
  const callback = useRef(onAuthorizationAction); callback.current = onAuthorizationAction;
  const sequence = useRef(0), requestKey = JSON.stringify(profile);
  useEffect(() => {
    const version = ++sequence.current;
    setStatus(null); setReview(false); setError("");
    if (platform !== "linux" || (!profile.linux_run_as_root && !profile.linux_root_authorization) || !callback.current) return;
    const timer = window.setTimeout(() => {
      void callback.current!("status").then(result => { if (sequence.current === version) setStatus(result); })
        .catch(e => { if (sequence.current === version) setError(String(e)); });
    }, 400);
    return () => { window.clearTimeout(timer); ++sequence.current; };
  }, [platform, requestKey]);
  async function action(operation: "authorize" | "revoke") {
    if (!callback.current || pending) return;
    setPending(true); setError("");
    try { setStatus(await callback.current(operation, operation === "authorize" ? status?.fingerprint || undefined : undefined)); setReview(false); }
    catch (e) { setError(String(e)); }
    finally { setPending(false); }
  }
  if (platform !== "linux") return null;
  const fr = language === "fr", checked = !!profile.linux_run_as_root;
  return <div className="per-gpu-box">
    <label className="check"><input type="checkbox" checked={checked} disabled={!available && !checked}
      onChange={event => onChange({ linux_run_as_root: event.target.checked })}/>
      {fr ? "Lancer ce mineur en administrateur (root)" : "Run this miner as administrator (root)"}
    </label>
    <p className="muted">{fr
      ? "Pour les réglages GPU nécessitant des droits administrateur. Le lancement root manuel demande une validation Linux. Pour les plannings et redémarrages automatiques, autorisez les commandes ci-dessous. Le pilote et la carte doivent prendre en charge les réglages."
      : "For GPU tuning that requires administrator rights. Interactive root starts ask for Linux approval. For schedules and automatic restarts, authorize the commands below. The driver and GPU must support the settings."}</p>
    {!available && <p className="muted">{fr
      ? "Lanceur ou pkexec indisponible : réinstallez le paquet MinerDesk récent, puis installez pkexec et un agent polkit graphique."
      : "Helper or pkexec unavailable: reinstall the current MinerDesk package, then install pkexec and a graphical polkit agent."}</p>}
    {onAuthorizationAction && (checked || profile.linux_root_authorization) && <div className="root-authorization">
      <strong>{fr ? "Démarrages root sans surveillance" : "Unattended root starts"}</strong>
      <p className="muted" role="status">{status?.message || (fr ? "Vérification de l’autorisation…" : "Checking authorization…")}</p>
      <p className="muted">{fr
        ? "Aucun mot de passe administrateur enregistré. La commande et la copie protégée du mineur sont liées à votre compte. Les modifications et mises à jour exigent une nouvelle validation ; un changement de mot de passe n’annule pas cette autorisation."
        : "No administrator password is stored. Approved commands and a protected miner copy are bound to your account. Changes and updates require approval again; changing your password does not revoke this authorization."}</p>
      <div className="row">
        <button type="button" className="mini-btn primary-lite" disabled={pending || !local || !available || !checked || !status?.eligible}
          onClick={() => setReview(true)}>{fr ? (status?.state === "approved" ? "Renouveler l’autorisation" : "Autoriser les démarrages planifiés") : (status?.state === "approved" ? "Renew authorization" : "Authorize scheduled starts")}</button>
        {profile.linux_root_authorization && <button type="button" className="mini-btn" disabled={pending || !local}
          onClick={() => void action("revoke")}>{fr ? "Révoquer et arrêter ce mineur" : "Revoke and stop this miner"}</button>}
      </div>
      {!local && <p className="muted">{fr ? "Ouvrez MinerDesk sur le PC Linux pour autoriser ou révoquer les commandes. Les démarrages déjà approuvés restent contrôlables depuis le LAN." : "Open MinerDesk on the Linux PC to approve or revoke commands. Previously approved starts can still be controlled from the LAN."}</p>}
      {review && status && <div className="root-authorization-review">
        <strong>{fr ? "Vérifier les commandes à autoriser" : "Review the commands to authorize"}</strong>
        <p>{fr
          ? "Cette action arrête ce profil et ouvre la validation administrateur Linux. Elle autorise ensuite ces commandes en root, même lorsque vous n’êtes pas devant le PC. Activez-la uniquement pour un mineur de confiance."
          : "This stops this profile and opens Linux administrator approval. It then permits these exact root commands while you are away. Enable it only for a miner you trust."}</p>
        <p className="muted">{fr ? "Dossier copié et protégé : " : "Folder copied into protected storage: "}{status.source_directory}</p>
        <p className="muted">{fr ? "Ce dossier doit contenir uniquement les fichiers de ce mineur. Les arguments avancés, scripts et liens symboliques ne sont pas admis." : "This folder must contain only this miner's files. Advanced arguments, scripts and symlinks are not eligible."}</p>
        {status.commands.map((command, i) => <pre key={i} className="command-preview">{command}</pre>)}
        {status.commands.length > 1 && <p className="muted">{fr ? "La seconde commande correspond au soutien volontaire que vous avez activé." : "The second command is for the voluntary support you enabled."}</p>}
        <button type="button" className="mini-btn primary-lite" disabled={pending} onClick={() => void action("authorize")}>{pending ? (fr ? "Validez la fenêtre Linux…" : "Approve the Linux dialog…") : (fr ? "Valider ces commandes avec Linux" : "Approve these commands with Linux")}</button>
        <button type="button" className="mini-btn" disabled={pending} onClick={() => setReview(false)}>{fr ? "Annuler" : "Cancel"}</button>
      </div>}
      {error && <p role="alert" className="muted">{error}</p>}
    </div>}
  </div>;
}
