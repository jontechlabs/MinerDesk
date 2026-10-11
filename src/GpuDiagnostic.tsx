import type { GpuDiscovery } from "./types";
import { tr } from "./i18n";

export default function GpuDiagnostic({ discovery, language }: { discovery: GpuDiscovery | null; language: string }) {
  if (!discovery?.diagnostic_code) return null;
  // Use the backend OS: a Windows browser can display a remote Linux backend.
  const code = discovery.diagnostic_code === "launch_failed" && ["windows", "linux"].includes(discovery.platform || "")
    ? `launch_failed_${discovery.platform}` : discovery.diagnostic_code;
  return <div className="gpu-diagnostic" role="status">
    <strong>{tr(language, "gpuDetection")}</strong>
    <p>{tr(language, `gpuDiagnostic_${code}`)}</p>
    {discovery.error_detail && <pre>{discovery.error_detail}</pre>}
  </div>;
}
