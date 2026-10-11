import type { MinerProfile } from "./types";

export default function LinuxElevationControl({ profile, platform, available, language, onChange }: {
  profile: MinerProfile; platform?: string; available?: boolean; language: string;
  onChange: (patch: Partial<MinerProfile>) => void;
}) {
  if (platform !== "linux") return null;
  const fr = language === "fr", checked = !!profile.linux_run_as_root;
  return <div className="per-gpu-box">
    <label className="check"><input type="checkbox" checked={checked} disabled={!available && !checked}
      onChange={event => onChange({ linux_run_as_root: event.target.checked })}/>
      {fr ? "Lancer ce mineur en administrateur (root)" : "Run this miner as administrator (root)"}
    </label>
    <p className="muted">{fr
      ? "Pour les réglages GPU nécessitant des droits administrateur. Une autorisation système peut être demandée à chaque démarrage. Activez uniquement pour un mineur de confiance ; le pilote et la carte doivent prendre en charge les réglages."
      : "For GPU tuning that requires administrator rights. System approval may be requested at each start. Enable only for a trusted miner; the driver and GPU must support the settings."}</p>
    {!available && <p className="muted">{fr
      ? "Lanceur ou pkexec indisponible : réinstallez le paquet MinerDesk récent, puis installez pkexec et un agent polkit graphique."
      : "Helper or pkexec unavailable: reinstall the current MinerDesk package, then install pkexec and a graphical polkit agent."}</p>}
  </div>;
}
