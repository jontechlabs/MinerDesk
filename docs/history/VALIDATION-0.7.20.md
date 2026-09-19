# MinerDesk 0.7.20 validation record

## Changes validated statically

- `start_miner(..., "manual")` passes the manual origin directly to the process runtime instead of reclassifying it as `schedule` inside an active window.
- `restart_miner_manually` launches with origin `manual`.
- `scheduler_should_stop_session` returns true only for running sessions whose origin is `schedule` and whose profile is no longer scheduled.
- Manual Start/Restart cancels a pending power action.
- Pending power processing cancels itself if mining has resumed before execution.
- A Windows-native watcher polls `/api/power/peek` without touching the UI-confirmation lease, so hidden WebView timer throttling cannot prevent foreground restoration.
- Native Tauri power polling also continues while the Desktop document is hidden/minimized.
- A new pending power action invokes `focus_main_window`; the command unminimizes, shows and focuses the main Tauri window.
- The Cancel button in the power modal is the autofocus/default keyboard target.
- Windows upgrade maintenance recognizes an installed 0.7.19.

## Checks executed in the preparation environment

`python tests/verify_installer_sources.py` completed successfully:
- balanced NSIS quoting;
- correct nsExec argument shape;
- balanced plugin stack use;
- valid NSIS runtime variables;
- PowerShell delimiter checks;
- package/Tauri/Cargo version agreement on 0.7.20;
- build safety/deferred publishing checks.

TypeScript syntax/transpilation was checked with TypeScript `transpileModule` for:
- `src/App.tsx`;
- `src/types.ts`;
- `src/i18n.ts`.

Additional source assertions verified:
- manual Start remains manual;
- manual Restart remains manual;
- pending power cancellation on mining resume;
- focus command registration;
- scheduler stop helper usage;
- hidden native power polling and focus invocation.

## Not executed here

The preparation environment does not contain the Windows Rust/MSVC/Tauri toolchain, so these were not executed here:
- Rust compilation;
- the 15 production scheduler tests with `rustc`;
- Tauri Windows compilation;
- NSIS installation;
- real miner/GPU execution;
- real Windows foreground-window behavior.

Your canonical Windows build command runs the production PowerShell/Rust preflight before producing the Setup:

```powershell
Set-ExecutionPolicy -Scope Process Bypass
.\scripts\build-windows.ps1
```
