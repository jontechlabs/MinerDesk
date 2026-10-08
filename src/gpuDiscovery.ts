import type { GpuDevice, GpuDiscovery } from "./types";

const indexedEngines = new Set(["srbminer", "lolminer", "rigel", "bzminer", "npminer"]);

/** System inventory indices must never be sent as another engine's selectors. */
export function selectableGpuDevices(discovery: GpuDiscovery | null, engine: string): GpuDevice[] {
  if (!discovery || discovery.engine !== engine) return [];
  if (indexedEngines.has(engine) && discovery.selectors_verified !== true) return [];
  return discovery.devices;
}

export function gpuSelectorsUnverified(discovery: GpuDiscovery | null, engine: string): boolean {
  return Boolean(discovery && discovery.engine === engine && indexedEngines.has(engine) && discovery.selectors_verified !== true);
}
