import type { GpuTuning, MinerProfile } from "./types";

export function effectiveGpuTuning(profile: MinerProfile, selector: string): GpuTuning {
  const saved = profile.gpu_tuning?.find(row => row.selector === selector);
  return {
    selector, ignore_defaults: true,
    core_clock: saved?.ignore_defaults ? saved.core_clock : saved?.core_clock ?? profile.core_clock,
    power_limit: saved?.ignore_defaults ? saved.power_limit : saved?.power_limit ?? profile.power_limit,
    fan: saved?.ignore_defaults ? saved.fan : saved?.fan ?? profile.fan,
  };
}

export function changeGpuTuning(profile: MinerProfile, selector: string, patch: Partial<GpuTuning>): GpuTuning[] {
  // Snapshot the other effective fields before opting out of legacy inheritance.
  return [...(profile.gpu_tuning || []).filter(row => row.selector !== selector),
    {...effectiveGpuTuning(profile, selector), ...patch, ignore_defaults: true}];
}

export function clearGpuTuning(): Partial<MinerProfile> {
  return {core_clock: null, power_limit: null, fan: null, gpu_tuning: []};
}

export function invalidSrbPower(profile: MinerProfile): number | null {
  if (profile.engine !== "srbminer") return null;
  const selected = profile.gpu_ids.split(",").map(id => id.trim()).filter(Boolean);
  const values = selected.length ? selected.map(id => effectiveGpuTuning(profile,id).power_limit) : [profile.power_limit];
  return values.find(value => value !== null && (!Number.isInteger(value) || value < 0 || value > 1000)) ?? null;
}
