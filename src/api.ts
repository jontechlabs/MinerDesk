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
