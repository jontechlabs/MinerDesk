/** The same command pipeline is used by profile buttons and Start/Stop all.
 * Kept independent of React so the actual production path can be regression-tested.
 */
export type MiningAction = "start" | "force-start" | "stop" | "restart" | "start-all" | "stop-all";
export type CommandStage = "saving" | "sending";
export type CommandRequest = <T>(path: string, options?: RequestInit, token?: string, timeoutMs?: number) => Promise<T>;
export type StartMode = "manual" | "schedule" | "waiting";
export interface BulkStartResult { id: string; result: string | null; mode?: StartMode | null; }
export interface CommandOutcome { startedCount?: number; waitingCount?: number; mode?: StartMode; }

export class BulkStartError extends Error {
  constructor(readonly failures: BulkStartResult[], readonly startedCount: number) {
    super(failures.map(item => `${item.id}: ${item.result}`).join("\n"));
    this.name = "BulkStartError";
  }
}

export async function runMiningCommand(options: {
  action: MiningAction;
  id?: string;
  dirty: boolean;
  save: () => Promise<void>;
  request: CommandRequest;
  token: string;
  onStage?: (stage: CommandStage) => void;
  elevatedStarts?: number;
}): Promise<CommandOutcome> {
  const { action, id, dirty, save, request, token, onStage } = options;
  const individual = action === "start" || action === "force-start" || action === "stop" || action === "restart";
  if (individual && !id) throw new Error("Missing mining profile ID");
  // A clean profile goes directly to Start. Stop NEVER depends on a config save.
  // Unsaved tuning is still saved first; a failed save must not launch stale settings.
  if (dirty && (action === "start" || action === "force-start" || action === "restart" || action === "start-all")) {
    onStage?.("saving");
    await save();
  }
  onStage?.("sending");
  const starts = action === "start" || action === "force-start" || action === "restart" || action === "start-all";
  const count = Math.max(0, Math.floor(options.elevatedStarts || 0));
  const timeout = starts ? 30000 + 130000 * count : 30000;
  const path = individual ? `/api/miners/${encodeURIComponent(id!)}/${action}` : `/api/miners/${action}`;
  if (action === "start-all") {
    const results = await request<BulkStartResult[]>(path, { method: "POST" }, token, timeout);
    if (!Array.isArray(results)) throw new Error("Invalid Start all API response");
    if (results.some(item => !item || typeof item.id !== "string" || (item.result !== null && typeof item.result !== "string"))) {
      throw new Error("Invalid per-profile result in Start all API response");
    }
    const failures = results.filter(item => item.result !== null);
    const waitingCount = results.filter(item => item.result === null && item.mode === "waiting").length;
    const startedCount = results.length - failures.length - waitingCount;
    // HTTP 200 is NOT proof that every mining profile started.
    if (failures.length) throw new BulkStartError(failures, startedCount);
    return { startedCount, waitingCount };
  }
  const result = await request<{ ok: boolean; mode?: StartMode }>(path, { method: "POST" }, token, timeout);
  if (result?.ok !== true) throw new Error(`MinerDesk did not acknowledge ${action}`);
  return result.mode ? { mode: result.mode } : {};
}
