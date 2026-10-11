import type { GpuTuning, MinerProfile } from "./types";

export const GPU_TUNING_FIELDS = ["core_clock", "memory_clock", "power_limit", "core_offset", "memory_offset", "fan"] as const;
export type GpuTuningField = typeof GPU_TUNING_FIELDS[number];

/** Only expose options with a documented CLI mapping for this engine. */
export function supportedGpuTuning(engine: string): readonly GpuTuningField[] {
  if (["srbminer", "lolminer", "bzminer", "rigel"].includes(engine)) return GPU_TUNING_FIELDS;
  if (engine === "npminer") return ["core_clock", "memory_clock", "power_limit"];
  if (engine === "lpminer") return ["core_clock"];
  return [];
}

export function effectiveGpuTuning(profile: MinerProfile, selector: string): GpuTuning {
  const saved = profile.gpu_tuning?.find(row => row.selector === selector);
  return {
    selector, ignore_defaults: true,
    ...Object.fromEntries(GPU_TUNING_FIELDS.map(field => [field,
      (saved?.ignore_defaults ? saved[field] : saved?.[field] ?? profile[field]) ?? null])) as Pick<GpuTuning, GpuTuningField>,
  };
}

export function changeGpuTuning(profile: MinerProfile, selector: string, patch: Partial<GpuTuning>): GpuTuning[] {
  // Snapshot the other effective fields before opting out of legacy inheritance.
  return [...(profile.gpu_tuning || []).filter(row => row.selector !== selector),
    {...effectiveGpuTuning(profile, selector), ...patch, ignore_defaults: true}];
}

export function clearGpuTuning(): Partial<MinerProfile> {
  return {core_clock: null, memory_clock: null, core_offset: null, memory_offset: null, power_limit: null, fan: null, gpu_tuning: []};
}

export function invalidGpuTuning(profile: MinerProfile): GpuTuningField | null {
  const selected = profile.gpu_ids.split(",").map(id => id.trim()).filter(Boolean);
  const rows = selected.length ? selected.map(id => effectiveGpuTuning(profile,id)) : [profile];
  return supportedGpuTuning(profile.engine).find(field => rows.some(row => {
    const value = row[field];
    if (value == null) return false;
    const offset = field === "core_offset" || field === "memory_offset";
    return !Number.isInteger(value) || value < (offset ? -2147483648 : 0) ||
      value > (offset ? 2147483647 : field === "fan" ? 100 : 4294967295);
  })) ?? null;
}

export function invalidSrbPower(profile: MinerProfile): number | null {
  if (profile.engine !== "srbminer") return null;
  const selected = profile.gpu_ids.split(",").map(id => id.trim()).filter(Boolean);
  const values = selected.length ? selected.map(id => effectiveGpuTuning(profile,id).power_limit) : [profile.power_limit];
  return values.find(value => value !== null && (!Number.isInteger(value) || value < 0 || value > 1000)) ?? null;
}
