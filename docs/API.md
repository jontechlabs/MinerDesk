# HTTP API overview

The Axum backend serves the embedded frontend and JSON API on port 17888 by default. Loopback is trusted; remote calls require `X-MinerDesk-Token: YOUR_TOKEN` or `Authorization: Bearer YOUR_TOKEN`. See [SECURITY.md](../SECURITY.md) before enabling LAN access.

| Method | Route | Purpose |
| --- | --- | --- |
| GET | `/api/health` | Version, backend mode, scheduler and crash-guard status |
| GET | `/api/engines` | Built-in engine adapters |
| GET / PUT | `/api/config` | Read/replace complete configuration |
| GET | `/api/status` | Profile runtime/metrics |
| POST | `/api/miners/start-all`, `/api/miners/stop-all` | Bulk commands |
| POST | `/api/miners/:id/start`, `/stop`, `/restart` | Individual profile command |
| GET | `/api/miners/:id/logs?limit=100` | Recent stdout/stderr and command messages |
| GET | `/api/miners/:id/gpus` | Profile-specific GPU discovery |
| POST | `/api/gpus/discover` | GPU discovery for a supplied profile |
| POST | `/api/engines/:engine/download` | Download a managed engine |
| GET | `/api/power/peek`, `/api/power/pending` | Read power action; pending also renews UI presence |
| POST | `/api/power/confirm`, `/api/power/cancel` | Confirm/cancel a pending action |
| GET | `/api/power/diagnostics` | Platform power diagnostics |
| GET | `/api/security/status` | Windows integration state |
| POST | `/api/security/firewall`, `/api/security/defender` | Optional privileged integrations |
| POST | `/api/backend/shutdown` | Backend shutdown, subject to ownership checks |
| POST | `/api/backend/desktop-heartbeat` | Desktop ownership/lease integration |

`:id` is the URL-encoded profile identifier; abbreviated `/stop` and `/restart` use the same `/api/miners/:id` prefix. The Rust types in `src-tauri/src/lib.rs` and UI types in `src/types.ts` define request/response fields.

Individual commands acknowledge success with `{ "ok": true }`. Start all returns per-profile results; inspect every result rather than treating HTTP 200 as proof that every profile started. Do not automatically retry a timed-out command: it may already have been applied.

PUT configuration and POST commands have side effects. Back up private configuration before writing it. Diagnostic responses and logs can contain sensitive values; do not paste raw responses into public issues.
