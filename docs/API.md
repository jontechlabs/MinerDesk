# HTTP API overview

The Axum backend serves the embedded frontend and JSON API on port 17888 by default. Same-machine calls are trusted only from the local MinerDesk browser/Tauri origins or non-browser clients without an Origin header. Remote calls require a configured `X-MinerDesk-Token: YOUR_TOKEN` or `Authorization: Bearer YOUR_TOKEN`; no token never means anonymous remote access. See [SECURITY.md](../SECURITY.md) before enabling LAN access.

| Method | Route | Purpose |
| --- | --- | --- |
| GET | `/api/health` | Version, backend mode, scheduler and crash-guard status |
| GET | `/api/updates?force=false` | Cached stable-release notification; `force=true` requests a manual refresh, rate-limited to once a minute. This ordinary update banner cannot install remotely. |
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

## Fleet local interface

The [Fleet guide](FLEET.md) describes pairing and trust. These routes belong to the existing local UI/API. Creation, pairing, invitations, disconnection and revocation require an approved same-machine origin; authenticated remote web views can inspect and operate an already paired fleet. Browser origin checks also apply to ordinary local mining endpoints.

| Method | Route | Purpose |
| --- | --- | --- |
| GET | `/api/fleet` | Setup, validated rig snapshots/configurations, and recent command receipts |
| POST | `/api/fleet/create`, `/disable` | Start/remove the optional HTTPS coordinator |
| POST | `/api/fleet/invite`, `/join`, `/leave` | Temporary invitation, verified TLS pairing, self-revoking disconnection |
| POST | `/api/fleet/discover` | Explicit UDP LAN discovery; hints do not authorize pairing |
| GET | `/api/fleet/rigs/:id`, `/api/fleet/rigs/:id/config` | Last confirmed rig data |
| POST | `/api/fleet/commands` | Typed commands with target IDs and expiry; inspect each receipt |
| GET | `/api/fleet/commands/:id` | Command result; a timeout is not proof of failure |
| POST | `/api/fleet/rigs/:id/revoke` | Revoke a rig's credentials on the coordinator |

`src-tauri/src/fleet/protocol.rs` defines the versioned command contract. Profile edits carry a profile revision; schedule edits carry a configuration revision. Paths, arbitrary arguments, passwords, system security and root enrollment are outside the Fleet command allowlist. The separate HTTPS machine protocol (normally 17889) authenticates agent and controller credentials separately, rejects browser-origin traffic and never forwards requests to the localhost API. Do not expose plain HTTP port 17888 to the public Internet.

`POST /api/fleet/join` accepts `{"address":"https://coordinator:17889","key":"INVITATION"}`. From 0.8.1, an omitted or blank `name` uses the local computer name; an explicit legacy name remains supported. Runtime telemetry uses the same metrics as `/api/status`; `dev_tip_active` is optional for older peers and defaults to false.

From 0.8.2, `snapshot.updates` optionally advertises update protocol 1. The local-only `/api/fleet/updates` GET and `/api/fleet/updates/{enable,disable,enroll,revoke}` POST routes manage the separate policy/system authorization; same-machine peer and Origin checks apply. Ordinary configuration PUTs cannot enable it. A typed `update_check` command refreshes eligible agents' offers. Direct `update_install` controller commands are rejected: use a confirmed rollout.

`POST /api/fleet/updates/rollouts` takes `{"rig_ids":["UUID"],"expected_version":"0.8.3"}`. Paired members forward that request to the pinned coordinator. `/api/fleet/updates/rollouts/:id/cancel` cancels remaining targets only. `FleetView.coordinator_rig_id` identifies the coordinator even when `setup.rig_id` belongs to the member caller; `FleetView.rollouts` exposes bounded persisted progress. Queue acceptance is not installation success. See [Fleet updates](FLEET-UPDATES.md).

The headless CLI supports `--config /absolute/path/to/instance/config.json` for an explicit configuration. Give independent instances separate configuration directories; Fleet identities and scheduler state are stored beside that file.

## Existing local controls

`:id` is the URL-encoded profile identifier; abbreviated `/stop` and `/restart` use the same `/api/miners/:id` prefix. The Rust types in `src-tauri/src/lib.rs` and UI types in `src/types.ts` define request/response fields.

Individual commands acknowledge success with `{ "ok": true }`. Start all returns per-profile results; inspect every result rather than treating HTTP 200 as proof that every profile started. Do not automatically retry a timed-out command: it may already have been applied.

GPU discovery includes `selectors_verified`: true when IDs were parsed from the selected engine, false for system inventory fallback. From 0.7.26, nullable `diagnostic_code` explains missing executables, launch failures, timeouts and failed/unrecognized listings; `raw_excerpt` retains the exact error. System/CUDA inventory IDs are not interchangeable with SRBMiner or other engine selectors. Engine diagnostics run in a blocking worker with a 60-second process limit; clients should allow 75 seconds for this request. Engine downloads can take longer than ordinary API calls; allow 300 seconds and do not retry timed-out writes automatically. See [GPU selection](GPU-SELECTION.md).

PUT configuration and POST commands have side effects. Back up private configuration before writing it. Diagnostic responses and logs can contain sensitive values; do not paste raw responses into public issues.
