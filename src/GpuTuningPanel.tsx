import { tr } from "./i18n";
import { clearGpuTuning, effectiveGpuTuning, invalidGpuTuning, invalidSrbPower, supportedGpuTuning } from "./gpuTuning";
import type { GpuTuningField } from "./gpuTuning";
import type { GpuDevice, GpuTuning, MinerProfile } from "./types";

type Props = {profile: MinerProfile; devices: GpuDevice[]; language: string;
  onGpuChange: (selector: string, patch: Partial<GpuTuning>) => void;
  onProfileChange: (patch: Partial<MinerProfile>) => void};

export default function GpuTuningPanel({profile, devices, language, onGpuChange, onProfileChange}: Props) {
  const t = (key: string) => tr(language,key);
  const fields = supportedGpuTuning(profile.engine);
  const labelKeys: Record<GpuTuningField,string> = {core_clock:"coreClock", memory_clock:"memoryClock", core_offset:"coreOffset", memory_offset:"memoryOffset", power_limit:"powerLimit", fan:"fan"};
  const selected = profile.gpu_ids.split(",").map(id => id.trim()).filter(Boolean);
  const defaultsActive = fields.some(field => profile[field] != null);
  const invalid = invalidSrbPower(profile);
  const invalidField = invalidGpuTuning(profile);
  function number(field: GpuTuningField, value: number | null, label: string, change: (value: number | null) => void) {
    const offset = field === "core_offset" || field === "memory_offset";
    return <input className="mini-num" type="number" aria-label={label} min={offset ? undefined : 0} step={1}
      max={field === "fan" ? 100 : field === "power_limit" && profile.engine === "srbminer" ? 1000 : undefined}
      value={value ?? ""} placeholder="—" onChange={event => change(event.target.value === "" ? null : Number(event.target.value))}/>;
  }
  return <>
    <div className="per-gpu-box">
      <div className="gpu-picker-head"><div><strong>{t("perGpuTuning")}</strong><span>{t("gpuTuningUnits")}</span><span>{t("gpuTuningBlank")}</span></div>
        <button className="mini-btn" onClick={() => onProfileChange(clearGpuTuning())}>{t("clearGpuTuning")}</button></div>
      {profile.engine === "srbminer" && <p className="muted">{t("srbPowerHelp")}</p>}
      <p className="muted">{t(fields.length ? "gpuClockHelp" : "gpuTuningCustom")}</p>
      {profile.engine === "lpminer" && <p className="muted">{t("gpuLpShared")}</p>}
      {profile.engine === "lolminer" && <p className="muted">{t("gpuLolMemory")}</p>}
      {invalid !== null && <div className="notice danger-notice" role="alert">{t("srbPowerInvalid").replace("{value}",String(invalid))}</div>}
      {invalidField !== null && invalid === null && <div className="notice danger-notice" role="alert">{t("gpuTuningInvalid").replace("{field}",t(labelKeys[invalidField]))}</div>}
      <div className="per-gpu-table">
        {fields.length > 0 && devices.filter(device => !selected.length || selected.includes(device.selector)).map(device => {
          const values = effectiveGpuTuning(profile,device.selector);
          return <div className="gpu-tuning-card" key={device.selector}><div className="gpu-tuning-name"><strong>{device.name}</strong><small>GPU {device.selector}</small></div>
            <div className="gpu-tuning-fields">{fields.map(field => <label className="field" key={field}><span>{t(labelKeys[field])}</span>{number(field,values[field],`${t(labelKeys[field])} · GPU ${device.selector}`,value => onGpuChange(device.selector,{[field]:value}))}</label>)}</div>
          </div>;
        })}
      </div>
    </div>
    {fields.length > 0 && <details className="legacy-defaults" open={defaultsActive}>
      <summary>{t("gpuDefaults")}{defaultsActive ? ` · ${t("gpuDefaultsActive")}` : ""}</summary>
      <p>{t("gpuDefaultsHelp")}</p><div className="gpu-tuning-fields">{fields.map(field => <label className="field" key={field}><span>{t(labelKeys[field])}</span>
        {number(field,profile[field],`${t(labelKeys[field])} · ${t("gpuDefaults")}`,value => onProfileChange({[field]:value}))}</label>)}</div>
    </details>}
  </>;
}
