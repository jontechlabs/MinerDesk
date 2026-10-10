import { tr } from "./i18n";
import { clearGpuTuning, effectiveGpuTuning, invalidSrbPower } from "./gpuTuning";
import type { GpuDevice, GpuTuning, MinerProfile } from "./types";

type Props = {profile: MinerProfile; devices: GpuDevice[]; language: string;
  onGpuChange: (selector: string, patch: Partial<GpuTuning>) => void;
  onProfileChange: (patch: Partial<MinerProfile>) => void};

export default function GpuTuningPanel({profile, devices, language, onGpuChange, onProfileChange}: Props) {
  const t = (key: string) => tr(language,key);
  const fields = ["core_clock", "power_limit", "fan"] as const;
  const labels = [t("coreClock"), t("powerLimit"), t("fan")];
  const selected = profile.gpu_ids.split(",").map(id => id.trim()).filter(Boolean);
  const defaultsActive = fields.some(field => profile[field] !== null);
  const invalid = invalidSrbPower(profile);
  function number(field: typeof fields[number], value: number | null, label: string, change: (value: number | null) => void) {
    return <input className="mini-num" type="number" aria-label={label} min={0} step={1}
      max={field === "fan" ? 100 : field === "power_limit" && profile.engine === "srbminer" ? 1000 : undefined}
      value={value ?? ""} placeholder="—" onChange={event => change(event.target.value === "" ? null : Number(event.target.value))}/>;
  }
  return <>
    <div className="per-gpu-box">
      <div className="gpu-picker-head"><div><strong>{t("perGpuTuning")}</strong><span>{t("gpuTuningUnits")}</span><span>{t("gpuTuningBlank")}</span></div>
        <button className="mini-btn" onClick={() => onProfileChange(clearGpuTuning())}>{t("clearGpuTuning")}</button></div>
      {profile.engine === "srbminer" && <p className="muted">{t("srbPowerHelp")}</p>}
      {invalid !== null && <div className="notice danger-notice" role="alert">{t("srbPowerInvalid").replace("{value}",String(invalid))}</div>}
      <div className="per-gpu-table"><div className="per-gpu-row header"><span>GPU</span>{labels.map(label => <span key={label}>{label}</span>)}</div>
        {devices.filter(device => !selected.length || selected.includes(device.selector)).map(device => {
          const values = effectiveGpuTuning(profile,device.selector);
          return <div className="per-gpu-row" key={device.selector}><div><strong>{device.name}</strong><small>GPU {device.selector}</small></div>
            {fields.map((field,index) => <div key={field}>{number(field,values[field],`${labels[index]} · GPU ${device.selector}`,value => onGpuChange(device.selector,{[field]:value}))}</div>)}
          </div>;
        })}
      </div>
    </div>
    <details className="legacy-defaults" open={defaultsActive}>
      <summary>{t("gpuDefaults")}{defaultsActive ? ` · ${t("gpuDefaultsActive")}` : ""}</summary>
      <p>{t("gpuDefaultsHelp")}</p><div className="form-grid three">{fields.map((field,index) => <label className="field" key={field}><span>{labels[index]}</span>
        {number(field,profile[field],`${labels[index]} · ${t("gpuDefaults")}`,value => onProfileChange({[field]:value}))}</label>)}</div>
    </details>
  </>;
}
