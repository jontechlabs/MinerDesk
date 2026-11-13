/** Shared HTTP transport. A timeout covers both headers AND the response body.
 * A timed-out write is never retried automatically: the backend may have applied it.
 */
export class ApiTimeoutError extends Error {
  readonly url: string;
  readonly timeoutMs: number;
  constructor(url: string, timeoutMs: number) {
    super(`MinerDesk API timeout after ${timeoutMs / 1000}s: ${url}`);
    this.name = "ApiTimeoutError";
    this.url = url;
    this.timeoutMs = timeoutMs;
  }
}

export type NativeJsonRequest = {
  connectionId: string;
  path: string;
  method: string;
  bodyJson?: string;
  timeoutMs: number;
};
export type NativeJsonBridge = <T>(request: NativeJsonRequest) => Promise<T>;

/** Freeze the native session for this App mount. A late save→start continuation
 * keeps its original identity and cannot be redirected to a newly selected rig.
 * Native requests already sent may finish after an abort; never retry a write.
 */
export function createNativeJsonClient(bridge: NativeJsonBridge, connectionId: string) {
  return async function nativeJson<T>(path: string, options: RequestInit = {}, _token = "", timeoutMs = 5000): Promise<T> {
    if (!connectionId || !path.startsWith("/api/") || path.startsWith("//") || /[\r\n\\#]/.test(path)) {
      throw new Error("Select a MinerDesk connection before sending this request.");
    }
    const method = (options.method || "GET").toUpperCase();
    if (!["GET", "POST", "PUT", "DELETE"].includes(method) || (options.body != null && typeof options.body !== "string")) {
      throw new Error("This request is not supported by the mobile client.");
    }
    if (!Number.isFinite(timeoutMs) || timeoutMs < 1 || timeoutMs > 300000) throw new Error("Invalid request timeout.");
    if (options.signal?.aborted) throw new DOMException("The operation was aborted.", "AbortError");
    let timer: ReturnType<typeof setTimeout> | undefined;
    let onAbort: (() => void) | undefined;
    const interrupted = new Promise<never>((_, reject) => {
      timer = globalThis.setTimeout(() => reject(new ApiTimeoutError(path, timeoutMs)), timeoutMs);
      onAbort = () => reject(new DOMException("The operation was aborted.", "AbortError"));
      options.signal?.addEventListener("abort", onAbort, { once: true });
    });
    try {
      return await Promise.race([bridge<T>({ connectionId, path, method,
        ...(typeof options.body === "string" ? { bodyJson: options.body } : {}), timeoutMs }), interrupted]);
    } finally {
      if (timer !== undefined) globalThis.clearTimeout(timer);
      if (onAbort) options.signal?.removeEventListener("abort", onAbort);
    }
  };
}

export async function requestJson<T>(
  url: string, options: RequestInit = {}, token = "", timeoutMs = 5000,
): Promise<T> {
  const headers = new Headers(options.headers || {});
  if (options.body && !headers.has("Content-Type")) headers.set("Content-Type", "application/json");
  if (token) headers.set("X-MinerDesk-Token", token);
  const controller = new AbortController();
  const externalSignal = options.signal;
  const cancelFromCaller = () => controller.abort();
  if (externalSignal?.aborted) cancelFromCaller();
  else externalSignal?.addEventListener("abort", cancelFromCaller, { once: true });
  let timedOut = false;
  const timer = globalThis.setTimeout(() => { timedOut = true; controller.abort(); }, timeoutMs);
  try {
    const response = await fetch(url, { ...options, headers, signal: controller.signal });
    const text = await response.text();
    let body: unknown = {};
    if (text) {
      try { body = JSON.parse(text); }
      catch {
        throw new Error(`MinerDesk API returned non-JSON data for ${url}: ${text.replace(/\s+/g, " ").slice(0, 120)}`);
      }
    }
    if (!response.ok) {
      const error = body && typeof body === "object" && "error" in body ? String(body.error) : "";
      throw new Error(error || `${response.status} ${response.statusText}`);
    }
    return body as T;
  } catch (error) {
    if (timedOut) throw new ApiTimeoutError(url, timeoutMs);
    throw error;
  } finally {
    globalThis.clearTimeout(timer);
    externalSignal?.removeEventListener("abort", cancelFromCaller);
  }
}
