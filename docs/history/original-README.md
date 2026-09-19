# MinerDesk 0.7.21 — manual Start override and focused power confirmation

## 0.7.21 — Manual Start command pipeline

Clean Start/Restart no longer waits for an unnecessary configuration save. Dirty
settings are still persisted first, but Windows wake-task/firewall synchronization
now runs in a serial background worker instead of delaying the HTTP response.
Stop never waits for a save. Partial Start all failures and per-profile launch
errors are displayed in a persistent banner and recorded in backend.log.

Build with the usual PowerShell commands:

```powershell
Set-ExecutionPolicy -Scope Process Bypass
.\scripts\build-windows.ps1
```

Run `publish\windows-x64\MinerDesk_0.7.21_x64-setup.exe` after the build reports
success. The build runs the new command/HTTP tests and config-worker tests.
See `CORRECTIONS-0.7.21.md` for the reproduced bug, tests actually executed in the
preparation environment, and the Windows validation still required.


## Build and install

Extract this source archive into a new directory and use the existing Windows workflow:

```powershell
Set-ExecutionPolicy -Scope Process Bypass
.\scripts\build-windows.ps1
```

Wait for `BUILD SUCCEEDED: MinerDesk 0.7.21`, then run:

```powershell
.\publish\windows-x64\MinerDesk_0.7.21_x64-setup.exe
```

This is source code, not a precompiled Setup. The build replaces the resources'
placeholder executables, checks their actual PE subsystem values, and publishes
only after successful compilation and generation of the current-version Setup.
Do not copy just MinerDesk.exe over an older install: install the complete package.
Existing AppData settings and downloaded miners are preserved by the migration.

## No terminal required for Desktop use

Windows now has two separate entry points into the same backend implementation:

- `minerdesk-backend.exe`: Windows GUI subsystem, no console allocation; the actual
  backend owning the miner Job Objects, not a launcher/wrapper. Always Desktop-owned.
- `minerdesk-headless.exe`: the original console CLI. A plain invocation remains
  standalone and independent of Desktop; console output, redirection and Ctrl+C
  stay available. The internal `--desktop-owned` flag is still supported for compatibility.

Setup, the privileged Scheduled Task, Desktop's elevated fallback, Repair backend,
and the manual task-install script now use `minerdesk-backend.exe`. Cleanup knows
both the new binary and the old installed headless binary. GPU/power diagnostic
commands also request CREATE_NO_WINDOW. UAC permission requests are not bypassed.

The Desktop-managed backend still stops on a real Desktop exit; close-to-tray is
not an exit. Existing heartbeat/PID checks and per-miner Windows Job Objects are
retained. The windowless backend does not depend on console Ctrl+C registration.
Miner output remains in the application's Console tab. Backend diagnostics remain
in `%APPDATA%\MinerDesk\backend.log`.

The Task Scheduler `Hidden` setting is not used as a console-hiding technique:
it controls task visibility in the Task Scheduler UI, not console allocation.

## Manual Stop takes precedence over a schedule

A stopped scheduled profile previously restarted at the next one-second tick.
Stop now records a hold for each schedule occurrence active for that profile at
the time of the click. A hold is identified by the schedule ID and its local start
date, including the previous date for an overnight window.

- Stop: stop that profile and keep it stopped for its current held occurrence(s).
- Stop all: stop running profiles and also hold currently eligible scheduled
  profiles that happen to be stopped or failed, preventing the next automatic retry.
- Start / Restart: clear the hold on successful start and create an explicit
  user-owned session. A manual session is not stopped when the schedule window
  ends; only sessions started by the scheduler are stopped by the scheduler.
- Later occurrences: run normally. A hold from one day cannot suppress the next
  day just because the backend was offline when the first window ended.
- Overlap: while any occurrence held by Stop is still active, a new overlapping
  window cannot undo Stop. If several windows were active when Stop was clicked,
  they are all held until they end. A new window may run after all held ones end.

The dashboard shows `Scheduler paused manually` (French: `Planification suspendue
manuellement`). The schedule remains enabled; no need to delete/recreate it.
Holds are stored atomically in `scheduler-manual-stops.json` beside config.json,
so restarting the backend in the same window does not immediately restart mining.
A persistence failure is reported; the current process still stops and remains
held in memory. An unreadable/invalid hold file is logged as a load error rather
than silently ignored.

Start/Stop/scheduler/internal restart operations share a lifecycle lock. Shutdown
sets a gate preventing late starts. Stop no longer depends on saving an unfinished
profile form first; Start/Restart still saves edits before launching the miner.
The previous uptime-to-zero and lpminer clock fixes are preserved.


## Manual Start after a schedule ends

0.7.21 makes manual intent authoritative. Pressing **Start** or **Restart** always
creates a user-owned mining session, even if the click happens at the exact end
boundary of a schedule. The scheduler only stops sessions that it started itself.
This removes the boundary race where a manual Start could previously inherit the
`schedule` origin and be stopped by the next one-second scheduler tick.

A manual Start/Restart also cancels any pending sleep/hibernate countdown created
when the previous schedule ended. As an additional safety net, the backend cancels
a pending power action if it detects that mining has resumed before execution.

## Power confirmation comes to the foreground

On Windows, a native Rust watcher polls the read-only `/api/power/peek` endpoint
even if the WebView has been hidden for hours. That endpoint does **not** renew the
confirmation-UI lease; it only detects a new pending action. The watcher then
unminimizes, shows and focuses the main window. The React Desktop also keeps its
normal pending-action poll active while hidden and invokes `focus_main_window` as
a second path. The safe **Cancel** button receives keyboard focus by default.
Browser/headless web clients do not attempt native window focus.

## Expected distribution

```text
publish\windows-x64\
  MinerDesk.exe
  minerdesk-backend.exe
  minerdesk-headless.exe
  MinerDesk_0.7.21_x64-setup.exe
  build-info.json
```

## Verification on Windows

```powershell
$task = Get-ScheduledTask -TaskName 'MinerDesk Privileged Backend' -ErrorAction Stop
$task.Actions | Format-List Execute, Arguments, WorkingDirectory
$task.Settings | Select-Object RestartCount, RestartInterval
```

Expected action: the installed `minerdesk-backend.exe`, argument `--desktop-owned`.
The Windows failure-retry policy remains three attempts at PT1M; this is not the
mining scheduler's one-second evaluation interval or Desktop heartbeat timeout.

Test with a short non-critical schedule: let it start, press Stop, wait at least
10 seconds, and confirm STOP / 00:00:00 / manual-pause badge without a restart.
Start should resume it as a manual session and it must keep running after that
window ends until the user stops it. Stop all should also remain effective. A
later window should run normally. Test overnight windows and backend restarts separately.

For crash-guard regression, note one managed miner PID, then force-terminate only
the matching Desktop-owned backend PID on a non-critical test session. Its miner
should disappear via the existing Job Object protection. This is a destructive
Windows test to perform deliberately, not something the source preflight executes.

## Diagnostics and limits

```powershell
Get-Content -LiteralPath "$env:APPDATA\MinerDesk\backend.log" -Tail 100
Get-Content -LiteralPath "$env:ProgramData\MinerDesk\logs\setup-maintenance.log" -Tail 100
```

`build-windows.ps1` runs the actual PowerShell parser and read-only installer/task
checks, then compiles/runs 15 tests against the exact production scheduler-policy
module using rustc. It also checks the GUI/console PE subsystem in built binaries.
These preflight tests do not mine, kill real processes or alter real tasks.
See `VALIDATION-0.7.21.md` for checks actually run in the preparation environment;
Windows compilation, real task launch and installer execution were not run there.

## Comparison with the supplied 0.7.5

Both 0.7.5 and 0.7.18 build minerdesk-headless as a console executable. Both
contain a Desktop fallback launch with `-WindowStyle Hidden`; both also have a
Scheduled Task launch that directly executes the console binary. In 0.7.5, the
NSIS backend-registration call does not inspect its return code and still contains
the invalid 15-second restart interval. Therefore a successful-looking installation
was not proof that its privileged task was registered. The exact historical launch
path on a particular PC cannot be reconstructed from the source archive alone.
A hidden fallback is consistent with the reported old behavior, but is not proven.
0.7.21 removes that dependency on how the executable is launched.

References (external platform behavior, distinct from source observations):
- Rust Windows subsystem: https://doc.rust-lang.org/reference/runtime.html#the-windows_subsystem-attribute
- Task restart interval: https://learn.microsoft.com/en-us/windows/win32/taskschd/tasksettings-restartinterval
- Task Hidden property: https://learn.microsoft.com/en-us/windows/win32/taskschd/tasksettings-hidden

---

## Historical documentation (version-specific; the 0.7.21 section above takes precedence)

# MinerDesk 0.7.18

## InstallBackend / Task Scheduler fix

A 0.7.17 installation log identifies the failing operation as
`Register-ScheduledTask`, with `(42,25):Interval:PT15S`. Process inspection and
`PrepareInstall` had already completed successfully. This is a task-definition
error, not evidence that the installer or a miner is still running.

Windows requires `RestartOnFailure/Interval` to be at least **one minute** and at
most 31 days. All three registration paths now use:

```powershell
-RestartCount 3 -RestartInterval (New-TimeSpan -Minutes 1)
```

The paths fixed are `src-tauri/windows/maintenance.ps1` (Setup),
`scripts/install-privileged-backend.ps1` (manual backend installation), and the
PowerShell repair script generated by `src-tauri/src/lib.rs` (Desktop Repair
backend). The one-minute interval governs Windows retries after task failure;
it does not change the Desktop heartbeat or normal mining stop timing.

## Build and install

From this archive's root, keep using:

```powershell
Set-ExecutionPolicy -Scope Process Bypass
.\scripts\build-windows.ps1
```

Only after `BUILD SUCCEEDED: MinerDesk 0.7.18`, run:

```powershell
.\publish\windows-x64\MinerDesk_0.7.18_x64-setup.exe
```

Use a new extracted source directory and do not run an older Setup left from an
unsuccessful build. The source archive contains a zero-byte headless placeholder;
the script rebuilds both applications and bundles the fresh headless binary.
The migration helper now also recognizes a registered/partially installed
0.7.17; cleanup requires consent and preserves AppData configuration and the
managed-miners download directory.

## Diagnostics

The NSIS writers now explicitly seek to end-of-file before appending. Previously
`FileOpen ... a` preserved the file but left its pointer at byte zero, so a later
phase could overwrite the beginning of earlier diagnostics.

Logs remain at:

```text
C:\ProgramData\MinerDesk\logs\setup-maintenance.log
C:\ProgramData\MinerDesk\logs\uninstall-maintenance.log
```

The actual ProgramData path is used; if that location is unwritable, the dialog
reports the TEMP fallback. The helper logs the restart policy before registering.
Code **61** denotes task registration failure; **62** denotes failure to request
the start of an already registered task. Neither means that a miner is running.
A successful registration also attempts to save `backend-task.xml` next to the
log. That diagnostic export is optional and cannot by itself block installation.

## Regression coverage and limits

`build-windows.ps1` now runs `tests/backend-task-settings.tests.ps1` as well as the
existing installer-safety suite. The new test reads the actual settings commands
from all three source paths, validates the Interval constraint using .NET XML
Schema, rejects `PT15S`, and requires three retries one minute apart. It does not
register a task or start any program.

See `VALIDATION-0.7.18.md` for checks actually executed in the preparation
environment. **No Windows executable build, live task registration, or full
installer-runtime test was executed there.** The remaining mining-runtime code is
unchanged apart from version labels and the one repair-script settings literal.

References:
- Microsoft Task Scheduler interval constraint:
  https://learn.microsoft.com/en-us/windows/win32/taskschd/taskschedulerschema-interval-restarttype-element
- Microsoft restart policy:
  https://learn.microsoft.com/en-us/windows/win32/taskschd/tasksettings-restartinterval
- NSIS FileOpen / FileSeek:
  https://nsis.sourceforge.io/Docs/Chapter4.html#fileopen

---

## Historical release notes

The notes below describe prior releases and earlier diagnoses. Where inconsistent,
the 0.7.18 instructions and validation record above take precedence.


## 0.7.17 — repair the installer command layer, not just process matching

This release replaces the remaining inline PowerShell commands in **all** NSIS
maintenance phases. Older hooks surrounded a command with single quotes and used doubled single
quotes inside it. NSIS split that command into multiple plugin arguments (the
pre-install verifier became seven arguments; its first command ended at
`-Command "$desktop=`). The hooks then mapped every nonzero helper result to “MinerDesk is still running”. A parser/launch failure
is not a positive process detection. The old `$COMMONAPPDATA` NSIS variable
was also undefined, breaking the intended log/miner folder locations.

The installer now extracts `maintenance.ps1` and `maintenance-common.ps1`,
passes paths as data in a UTF-16 request file, and captures the exit code and
helper output. The same implementation is used for early migration, pre-install,
backend registration, optional integrations and uninstall. No `-Command` source
is embedded in the NSIS hook. Process ownership still uses exact installed paths,
plus the managed miner directory with a directory boundary. Setup/helper PIDs are
excluded, and PID creation times are checked again before stopping a process.

**Build using your existing workflow, from this version's root directory:**

```powershell
Set-ExecutionPolicy -Scope Process Bypass
.\scripts\build-windows.ps1
```

The build now parses the PowerShell scripts and runs synthetic-process regression
tests before compiling. It checks every native exit code, requires the exact
`MinerDesk_0.7.17_x64-setup.exe`, and publishes only after success. It removes
obsolete versioned Setup executables from the publish directory after a successful
build. `build-info.json` records the version, source directory and SHA-256 hashes.
Never run a 0.7.14/0.7.16 Setup as the result of a 0.7.17 build.

Persistent diagnostics (created by NSIS before starting the helper):

```text
C:\ProgramData\MinerDesk\logs\setup-maintenance.log
C:\ProgramData\MinerDesk\logs\uninstall-maintenance.log
```

The actual ProgramData location is resolved through NSIS `SetShellVarContext all`
and `$APPDATA`. If it is unwritable, the dialog shows the exact TEMP fallback.
If no log is writable, maintenance stops explicitly. Logs are appended in UTF-16.
Code 20 means **verified processes remain**, with PIDs and paths in the log.
Codes 41/42/43 mean process/path/Task Scheduler inspection failed; code 50 is a
helper exception; 9000/9001/9002/9003 are extraction, launch, log or timeout errors.
A parser failure (normally 1) is also reported as a helper error, never as a miner.

Legacy migration still covers 0.7.3–0.7.16. It requires confirmation and preserves
AppData settings and downloaded miners. It now deletes only the known MinerDesk
binaries, **not every file in an installation directory**. A cancellation after
approved legacy cleanup does not restore old binaries; rerun the new Setup.
Standalone backends outside the installed directory are not process-name targets.
A conflicting Scheduled Task targeting another directory is reported, not killed.

Runtime mining, Job Objects, uptime and clock logic are unchanged apart from the
version label. See `VALIDATION-0.7.17.md` for precisely what was checked in the
preparation environment and what requires Windows. Older release notes below are
historical; their earlier proposed diagnoses were not Windows-runtime proof.






## 0.7.16 — installer self-detection / exact-path process guard

- Fixes Setup falsely reporting that MinerDesk is still running while the only visible processes are the NSIS installer bootstrap/elevated worker. Windows Task Manager can group those Setup processes under the **MinerDesk** product name, so process-name-only checks were unsafe.
- Installer, upgrade helper and uninstaller now identify the installed Desktop/backend by their **exact executable paths** (`$INSTDIR\MinerDesk.exe` and `$INSTDIR\minerdesk-headless.exe`) instead of `Get-Process -Name MinerDesk`. The Setup executable and its elevated child are therefore never treated as the Desktop application.
- The same exact-path rule is used when stopping and verifying processes. A standalone `minerdesk-headless.exe` launched from another directory is no longer stopped just because it has the same process name.
- The legacy-upgrade diagnostic log records every MinerDesk-looking process it sees (PID, parent PID, name and path), while only exact installed paths are eligible for termination.
- The supported Windows distribution build remains the project script: `Set-ExecutionPolicy -Scope Process Bypass` followed by `.\scripts\build-windows.ps1`. That script already builds the frontend, headless binary, Tauri desktop and NSIS Setup; it does not launch Setup itself.


## 0.7.15 — robust legacy registry cleanup + persistent installer diagnostics

- Fixes the 0.7.14 migration failure `The previous MinerDesk uninstall registration could not be removed` that could appear even after process/file cleanup succeeded.
- Root cause addressed: the PowerShell helper could observe a different WOW64 registry view than Tauri's x64 NSIS installer. PowerShell no longer removes or validates the Tauri uninstall key; NSIS performs that step itself in both 64-bit and 32-bit registry views and retries with `reg.exe /reg:64` / `/reg:32` if needed.
- The helper now receives the install directory directly from Setup (`$INSTDIR`) instead of rediscovering it from PowerShell's registry view.
- On 64-bit Windows, Setup prefers native 64-bit Windows PowerShell through `Sysnative` when the NSIS host is 32-bit.
- Upgrade diagnostics are now persisted at `C:\ProgramData\MinerDesk\logs\legacy-upgrade.log`, with `%TEMP%` only as a fallback. The installer creates the log before launching PowerShell.
- The early migration path now covers installed versions through 0.7.14 while preserving AppData settings and downloaded miners.

## 0.7.14 — legacy cleanup launcher fix

- Fixes the `code error` shown by 0.7.13 before legacy cleanup actually started. The 0.7.13 NSIS hook used fragile quote escaping in `nsExec::ExecToStack`, so Windows could fail to launch PowerShell and no cleanup log was ever created.
- Legacy cleanup is now launched with NSIS `ExecWait`, using normal quoted arguments and a numeric exit code.
- Setup creates `%TEMP%\MinerDesk-legacy-upgrade.log` **before** launching PowerShell. Even if PowerShell cannot start, the log now exists and records the launcher stage, helper path and PowerShell path.
- The cleanup helper receives the exact log path from Setup, so NSIS and PowerShell always append to the same file.
- The early migration path now also covers an installed 0.7.13.

## 0.7.13 — deterministic legacy upgrade cleanup

- Fixes the Tauri reinstall-page failure `Impossible de désinstaller le programme !` seen with 0.7.12.
- The new Setup no longer rewrites the old registry `UninstallString` to a PowerShell command. Tauri appends its own NSIS arguments and then checks that the previous executable has disappeared, which made that bridge fragile.
- Before Tauri's reinstall page is shown, Setup now detects MinerDesk 0.7.3 through 0.7.12 and runs the cleanup helper directly from the new installer. It stops Desktop/headless/managed miners, removes the old scheduled task/integration entries and Program Files application files, then removes the old uninstall registry key.
- User settings in AppData and downloaded miners under ProgramData are preserved. Once the old uninstall key is gone, Tauri skips its previous-uninstaller path entirely and proceeds as a fresh install.
- The cleanup helper writes `%TEMP%\MinerDesk-legacy-upgrade.log` and returns explicit failure codes if a process, the old executable, or the uninstall key cannot be removed.


## 0.7.12 — legacy uninstaller bridge

- Upgrades from MinerDesk 0.7.3 through 0.7.11 no longer depend on the previous `uninstall.exe` being healthy. This specifically fixes the 0.7.9 generic `Uninstall could not safely stop the MinerDesk backend` failure that can appear even when Desktop, headless and miners are already stopped.
- Tauri invokes the previous uninstaller from its reinstall page before the new Setup reaches the normal pre-install hook. 0.7.12 therefore prepares a temporary cleanup helper at GUI initialization and temporarily redirects the legacy `UninstallString` to it.
- The helper performs deterministic cleanup of Desktop/headless/miners, the privileged task, wake tasks, optional firewall/Defender/PATH/startup integrations and old program files while preserving the user's roaming MinerDesk configuration. The new Setup then installs normally and writes a fresh uninstall registration.
- If Setup is cancelled before the helper actually runs, the original legacy `UninstallString` is restored automatically.


## 0.7.11 — NSIS Modern UI GUI-init compatibility

- Fixes Windows Setup bundling failure: `Function named ".onGUIInit" already exists`.
- The early running-process guard is still executed before Tauri can launch the previous uninstaller, but is now registered using the supported Modern UI hook `MUI_CUSTOMFUNCTION_GUIINIT` instead of redefining `.onGUIInit`.
- Keeps the 0.7.10 installer/uninstaller process shutdown logic unchanged.
- The Windows release build no longer emits the harmless `spawn_desktop_web is never used` warning; that helper is now compiled only for the non-Windows desktop fallback where it is used.

## 0.7.10 — reliable maintenance while MinerDesk is running

- Windows Setup now checks for a running Desktop/backend/miner **as soon as the installer GUI opens**, before Tauri can launch the previous installed version's uninstaller. This specifically fixes upgrades from 0.7.9 where the old uninstaller could fail if MinerDesk was still loaded.
- If MinerDesk is active, Setup asks whether it may stop Desktop, the privileged backend and MinerDesk-managed miners. Declining leaves the current installation untouched and cancels Setup.
- The early Setup shutdown deliberately kills `MinerDesk.exe` **without** `/T`; this avoids terminating a maintenance process that might have been launched from MinerDesk. The headless backend still uses `/T`, and managed miners are additionally cleaned by their trusted `C:\ProgramData\MinerDesk\miners` path.
- Uninstall shutdown is now split into short, independently verified stages: Desktop stop, Scheduled Task stop/disable, headless process-tree stop, orphan-miner cleanup, process verification, task removal, and final task verification. This replaces the previous single long PowerShell command that could collapse into the generic “could not safely stop” error.
- A manual recovery helper is included at `scripts/stop-minerdesk-maintenance.ps1`. Run it from an elevated PowerShell with `-RemoveTask` when diagnosing a broken installation/uninstallation.

## 0.7.9 — uptime reset and lpminer core-clock restart fix

- A stopped miner profile now resets its session uptime immediately to `00:00:00`. The reset also happens when a miner exits on its own or crashes, so a stale `started_at` timestamp can no longer keep counting while the row is in STOP state.
- Start and Restart now save the current form snapshot before launching the miner. If the configuration save fails, MinerDesk does not continue with an older saved profile.
- The lpminer adapter now emits `--lock-core-clock <MHz>` from the latest effective **Core clock MHz** value. Per-GPU overrides take priority over the legacy profile-wide fallback, so changing an old `2200` value and doing Stop → Start uses the new value.
- Because lpminer 0.1.x exposes one shared `--lock-core-clock` value, MinerDesk accepts a multi-GPU lpminer profile only when all selected GPUs resolve to the same configured core clock. Different per-GPU core clocks return a clear error instead of silently applying the wrong value.

## 0.7.8 — Windows Job Object crash guard for miners

- Every mining engine started by `minerdesk-headless.exe` on Windows is now attached to its own dedicated Windows **Job Object** configured with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`.
- Miner processes are created **suspended**, attached to the Job Object, and only then resumed. This prevents a miner from creating descendants before the crash guard is active.
- If `minerdesk-headless.exe` exits normally, crashes, is terminated from Task Manager, or is killed with `taskkill /F`, Windows closes the Job Object and terminates any miners that are still associated with it.
- Child processes spawned by miners inherit the Job Object by default, so the protection covers miner process trees rather than only the root executable.
- MinerDesk now **fails closed**: if the Job Object cannot be created or a miner cannot be attached/resumed safely, that miner is terminated and startup is rejected instead of allowing unprotected background mining.
- `/api/health` exposes `miner_crash_guard`, and **Settings** shows whether the Windows miner crash guard is active.
- The Windows installer/upgrade check now also detects mining executables already running from `C:\ProgramData\MinerDesk\miners`, even if Desktop/headless has already disappeared, and asks before force-closing those orphaned miners.
- The protection applies both to the Desktop-owned backend and to a manually launched standalone `minerdesk-headless.exe`. The standalone instance remains independent from the Desktop heartbeat, but its miners cannot survive the standalone headless process itself.

## 0.7.7 — reliable uninstall shutdown

- Uninstall now detects `MinerDesk.exe`, `minerdesk-headless.exe`, and the Windows Scheduled Task `MinerDesk Privileged Backend` before files are removed.
- If any of them is active, the user is asked whether MinerDesk may stop the application/backend and all MinerDesk-managed mining processes. Choosing **No** cancels uninstall without removing components.
- Choosing **Yes** disables and ends the Scheduled Task first, terminates the Desktop/headless process trees, stops managed miners under `C:\ProgramData\MinerDesk\miners` as a fallback, waits for shutdown, then unregisters and verifies removal of the task before deleting files.
- Scheduled-task cleanup no longer depends on the historical `BackendTaskAdded` registry flag, so a stale task left by an older build is removed too.
- Setup also treats the Scheduled Task itself as active during upgrade checks and waits briefly after stopping it, reducing file-lock races on `minerdesk-headless.exe`.

## 0.7.6 — safer Windows upgrades

- Before replacing files, the Windows Setup checks whether `MinerDesk.exe` and/or `minerdesk-headless.exe` is running.
- If either process is active, Setup asks the user whether it may **force close MinerDesk and continue**. Choosing **No** cancels the installation instead of silently terminating anything.
- When the user accepts, Setup disables the privileged backend task temporarily and uses a process-tree termination so mining engines started under `minerdesk-headless.exe` are stopped as well. Setup verifies that both MinerDesk processes are gone before continuing.
- If the privileged backend option is declined during an upgrade, an older scheduled backend task is removed instead of being left behind.
- Desktop lifecycle behavior is unchanged: a real Desktop exit stops every miner managed by the Desktop-owned backend; close-to-tray is not an exit, while standalone CLI headless mode remains independent by design.

## 0.7.5 — robust Desktop-owned backend lifecycle and full-width console

- The Console now always spans the full available page width, including when its log buffer is empty.
- The privileged backend installed/started by MinerDesk Desktop now runs with the internal `--desktop-owned` flag. Only this Desktop-owned mode accepts Desktop heartbeat/exit ownership.
- Running `minerdesk-headless.exe` manually from a terminal remains standalone: Desktop heartbeat requests are ignored and closing MinerDesk Desktop does not terminate the CLI instance.
- Desktop heartbeat messages carry the Desktop PID. If heartbeats are delayed, the backend verifies that the owning Desktop process is still alive before taking any shutdown action. A transient heartbeat transport problem therefore no longer stops mining while MinerDesk is still running.
- Normal Desktop exit still performs an immediate graceful backend shutdown. Forced Desktop termination is handled by the PID-aware watchdog after the heartbeat grace period.
- The installer, repair action, development helper and elevated fallback all start the Desktop-managed backend with `--desktop-owned`.

## 0.7.4 — backend follows Desktop exit

- On Windows, every real MinerDesk Desktop exit now gracefully stops `minerdesk-headless.exe` and active miners. This applies to **Quit MinerDesk** from the tray, closing the window when close-to-tray is disabled, Alt+F4, logoff/session shutdown, and other normal Tauri exit paths.
- **Close to tray** still only hides the window; because the Desktop is still running, the headless backend remains active.
- A Desktop heartbeat watchdog also shuts the backend down after several missed heartbeats if the Desktop is force-terminated or crashes before it can send the graceful shutdown request. Standalone headless mode remains independent until a Desktop instance attaches.
- The old optional “stop backend on Desktop exit” setting is retained only for configuration-file compatibility and is no longer exposed in the UI or used to decide shutdown behavior.
- The Windows installer now stops an existing headless backend in a **pre-install** hook before replacing files, preventing upgrades from failing because `minerdesk-headless.exe` is locked.

## 0.7.3 — live mining information in the Windows tray

The notification-area menu now displays live information from the privileged backend: active miner/profile, hashrate, power, temperature, shares and uptime. With several miners it shows an aggregate summary and up to two profile/hashrate lines. The tray refreshes every 4 seconds and reports clearly when mining is stopped or when the backend is unavailable.


## 0.7.3 — Alephium developer-tip payout

- Added the public **Alephium (ALPH)** developer payout address.
- MinerDesk now detects ALPH profiles when the algorithm is `alph`, `aleph` or `alephium`, or when the pool explicitly contains `alephium`.
- EVM wallets (`0x...`) keep priority, so EVM-based coins using an Alephium-compatible mining algorithm are not redirected to the native ALPH address.
- Unknown/ambiguous networks remain skipped rather than guessed.

## 0.7.0 — voluntary developer tip

MinerDesk can now optionally redirect a user-selected percentage of mining time to public developer payout addresses. The setting is per profile, visually highlighted, transparent, and **0% by default**. It can be changed or disabled at any time. MinerDesk automatically detects supported payout networks from the configured wallet/algorithm/pool and supports PRL+NOCK merge-mining tips. Unknown networks are skipped rather than redirected.

The implementation stores **public receive addresses only**; no developer private keys or seed phrases are embedded. To reduce pool churn, support time is accumulated and paid in short slices rather than frequent per-share switching.

## 0.7.0 — lpminer and NPMiner

- Added **lpminer** (`BaikalMine-Pools/pearl-miner`) to **Miners & profiles**, including official GitHub auto-download on Windows/Linux and native Pearl pool/wallet command generation.
- Added **NPMiner** (`nushypool/npminer`) to **Miners & profiles**, including official GitHub auto-download, `--list-gpus` discovery, device selection, worker/password, local API port and supported CUDA core-clock/power-limit arguments.
- `pearlhash` entered in a NPMiner profile is normalized to NPMiner's documented `pearl` algorithm name.
- lpminer uses its `--lock-core-clock` option when **Core clock MHz** is configured. Other lpminer-specific tuning/device options remain available through **Advanced arguments**.

## 0.6.8 — Windows startup and close-to-tray

- Adds **Start MinerDesk with Windows** in Settings. It creates/removes a per-user `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` entry and is disabled by default.
- Adds **Minimize to the notification area when closing**. When enabled, the window close button hides MinerDesk instead of exiting; the tray icon can reopen MinerDesk or quit it explicitly. This option is disabled by default.
- The tray icon is hidden when close-to-tray is disabled and becomes visible immediately after the setting is saved.
- Uninstall removes the optional MinerDesk startup registry value.


## 0.6.7 — startup handshake and stable token generation

- The startup splash now stays visible until the privileged backend is actually reachable and the initial MinerDesk configuration has loaded.
- On Windows, the Desktop owns the startup handshake: it first checks the backend, starts the scheduled privileged backend when available, and falls back to an elevated headless launch (UAC) only when needed.
- The main application no longer appears before the Windows approval/backend startup step completes.
- **Generate token** no longer changes the active API token before settings are saved. The generated token remains visible in the field until the user saves it, preventing an API refresh from replacing it with the old backend value.
- Existing backend shutdown-on-desktop-close behavior is preserved.


## 0.6.7 — startup splash, privileged security actions, and reliable desktop shutdown

This maintenance release focuses on three Windows desktop issues:

- **Immediate startup feedback.** The Tauri window is created without waiting synchronously for the privileged backend. A lightweight animated mining splash appears immediately and stays visible while MinerDesk connects to the backend and loads profiles/schedules.
- **Microsoft Defender exclusion fixed.** Windows Security actions are now executed by the privileged `minerdesk-headless.exe` backend through `/api/security/*`. The non-elevated Tauri desktop no longer tries to run `Add-MpPreference` directly. Errors such as Tamper Protection or organization policy restrictions are now shown in the UI instead of failing silently.
- **Stop backend on Desktop exit fixed.** The Desktop lifecycle toggle is saved immediately. When enabled, the Tauri WebView sends a best-effort shutdown request during window teardown, while the Rust `RunEvent::Exit` handler remains as a second fallback. The backend stops active miners before exiting.

### Windows Security API

The headless backend now exposes local/authorized endpoints:

```text
GET  /api/security/status
POST /api/security/firewall   { "enabled": true|false }
POST /api/security/defender   { "enabled": true|false }
```

On Windows these operations should be performed through the privileged backend installed by the Setup package. If Microsoft Defender Tamper Protection, Group Policy, or endpoint-management software blocks an exclusion, MinerDesk displays the PowerShell error.

### Startup behavior

The Desktop no longer blocks window creation while Task Scheduler/UAC starts the privileged backend. The startup splash includes a small animated mining cart/pickaxe and status text. If the backend takes unusually long to start, MinerDesk eventually reveals the normal dashboard and backend controls rather than leaving the user with an unexplained blank/frozen window.

## 0.6.4 — larger Desktop window and optional backend shutdown on exit

- Default Desktop window increased to **1480×900** (minimum **1180×720**).
- Dashboard table minimum width reduced so common 1366/1440/1480 layouts no longer require horizontal scrolling in normal use.
- New **Settings → Desktop lifecycle** option: **Stop the privileged backend when MinerDesk Desktop closes**.
- The option is disabled by default because the headless backend is responsible for schedules, miners and the Web UI. When enabled, closing the Tauri Desktop sends a graceful shutdown request to the headless backend; active miners are stopped before the backend exits.
- The Windows scheduled task stays installed, so opening MinerDesk again can start the privileged backend normally.

## 0.6.4 — UI responsiveness / performance fix

This release focuses on keeping the desktop UI responsive while miners are running.

Changes include:

- the power-action countdown clock no longer forces a full React render every second when no countdown is active;
- backend health checks are lightweight during normal operation; expensive Windows PowerShell diagnostics run only when explicitly requested;
- backend health polling was reduced to every 10 seconds and pauses while the window is hidden;
- miner status polling adapts to the active page (fastest on Dashboard, slower on configuration pages);
- overlapping status/API polls are prevented;
- pending power-action polling no longer triggers unnecessary state changes;
- console polling is reduced to 250 lines every 3 seconds and only updates React when the log actually changed;
- smooth auto-scrolling was removed from the live console to avoid repeated WebView animations;
- the permanent `backdrop-filter` blur on the sticky header was removed to reduce WebView2/GPU composition cost.

The **Refresh** button in the privileged-backend banner now performs the full Windows diagnostics on demand.


## 0.6.4 development launcher fix

On Windows, Node.js can return `spawn EINVAL` when a `.cmd` shim such as `npx.cmd` is launched with `shell: false`. The development command no longer depends on that path. `npm run tauri:dev` now builds the debug headless backend first and then launches the local Tauri CLI through npm's normal script shell.

The Rust warning about `spawn_desktop_web` being unused is harmless and does not block the build.

# MinerDesk 0.6.4

MinerDesk is a cross-platform mining orchestrator built with **Tauri 2**, **React/TypeScript**, and a **Rust** backend.

## What's new in 0.6.4 — automatic privileged-backend lifecycle

Version 0.6.4 makes the Windows desktop actively manage the privileged `minerdesk-headless.exe` backend instead of only reporting `Failed to fetch`.

### Automatic startup

When `MinerDesk.exe` starts on Windows it now:

1. checks the configured local backend port (17888 by default);
2. verifies that `/api/health` is a real MinerDesk backend;
3. starts the existing **MinerDesk Privileged Backend** Scheduled Task when available;
4. waits for the backend to become ready;
5. when no Scheduled Task exists (for example during development), it can launch only `minerdesk-headless.exe` through UAC while keeping the Tauri/WebView2 desktop non-elevated.

The desktop never binds the mining/backend port itself on Windows. This preserves the WebView2 fix introduced in 0.5.x and keeps GPU tuning in the privileged process.

### Backend status and controls in the desktop UI

The desktop now shows a persistent backend status bar:

```text
Privileged backend
● Connected · 127.0.0.1:17888
[ Restart backend ] [ Repair backend ] [ Refresh ]
```

When the backend is offline it shows:

```text
Privileged backend
● Offline · task installed / task missing
[ Start backend ] [ Repair backend ] [ Refresh ]
```

The diagnostics panel includes:

- Scheduled Task state;
- last Task Scheduler result;
- backend executable path;
- process currently listening on the configured port;
- the tail of `%APPDATA%\MinerDesk\backend.log`.

**Start backend** first tries the installed Scheduled Task. If the task is unavailable or broken, MinerDesk can launch the headless executable elevated (UAC applies only to the backend).

**Repair backend** recreates the highest-privilege Scheduled Task using the currently installed/built `minerdesk-headless.exe` and starts it. This action intentionally triggers UAC.

### Development mode

`npm run tauri:dev` now uses `scripts/dev.mjs`. On Windows it first builds the debug `minerdesk-headless.exe`, then launches Tauri. This means the desktop can automatically find and start the development backend without requiring a second terminal.

Use a **normal (non-administrator) PowerShell** for `npm run tauri:dev`. If the development backend is not already installed as a task, MinerDesk may show one UAC prompt to start the headless process.

### Power-aware scheduling retained from 0.6.0

The power-aware scheduler introduced in 0.6.0 remains included. For each mining window you can configure:

- **After mining:** do nothing, sleep, or hibernate.
- **Countdown:** configurable from 5 seconds to 1 hour.
- **Confirmation safety:** optionally require a Desktop/Web UI to remain open during the countdown. If the confirmation dialog is closed or cancelled, the computer will **not** sleep/hibernate.
- **Wake before start (Windows):** create Windows Task Scheduler wake timers automatically, for example waking the PC 5 minutes before a mining window starts.

The scheduler only requests a post-mining power action after the **last active scheduled mining window** ends and no other miner is still running. A manually started miner therefore prevents an automatic sleep/hibernate.

Example:

```text
Schedule: Solar mining
Days: Monday → Friday
Start: 08:30
End: 16:00
Wake computer: 5 minutes before
After mining: Sleep
Countdown: 60 seconds
Require confirmation UI: Yes
```

At 16:00 MinerDesk stops the scheduled miners and displays a countdown. Closing or cancelling it prevents the power action.

### Windows wake timers

When **Wake computer before start** is enabled and the configuration is saved, the privileged backend creates Task Scheduler entries named:

```text
MinerDesk Wake - <schedule-id> - <weekday>
```

Inspect them with:

```powershell
Get-ScheduledTask | Where-Object TaskName -like 'MinerDesk Wake - *'
powercfg /waketimers
```

Wake-up requires Sleep/Hibernate support, wake timers enabled in the active Windows power plan, and compatible motherboard/firmware behavior. MinerDesk does not promise wake-from-shutdown; use BIOS/UEFI RTC wake or Wake-on-LAN for a fully powered-off PC.

### Headless power behavior

Power actions run in the Rust backend, so schedules continue when the Tauri desktop is closed. If **Require confirmation UI** is enabled, an open Desktop or Web UI is required during the countdown. If confirmation is disabled, headless MinerDesk can perform the configured power action unattended.

## Windows privileged-backend architecture

On Windows, `MinerDesk.exe` runs as a normal user. `minerdesk-headless.exe` runs through the **MinerDesk Privileged Backend** scheduled task with elevated rights and owns mining processes, GPU tuning, schedules, sleep/hibernate actions, wake-task creation, and the HTTP API.

This avoids WebView2 permission problems while preserving the privileges needed by NVIDIA/AMD tuning and Windows power management.

It can run several mining engines at the same time, assign different GPUs to different profiles, apply per-GPU tuning, schedule mining windows, download supported miners from their official GitHub releases, and expose the same dashboard through a local or LAN web interface.

MinerDesk ships as two applications:

- **`MinerDesk.exe`** — desktop GUI application.
- **`minerdesk-headless.exe`** — CLI/headless orchestrator with the same web dashboard and scheduler.

On Windows, the build also produces a normal **NSIS `Setup.exe`** installer.

> Mining applications are frequently detected as PUA/HackTool by antivirus products. MinerDesk downloads miners only from the configured official GitHub repositories, but you should still verify what you install. MinerDesk never needs your wallet seed or private key; only public mining addresses are used.

---

## 1. Features

### Multi-miner orchestration

Built-in adapters are provided for:

- SRBMiner-Multi
- lolMiner
- BzMiner
- Rigel
- lpminer
- NPMiner
- Custom executables / custom command-line arguments

Each mining profile has its own algorithm, pool, wallets, worker, selected GPUs, API port, schedule and advanced arguments.

### GPU selection and per-GPU tuning

MinerDesk discovers GPU names through the selected mining engine when possible, with a system fallback.

You can select one or more cards with checkboxes and configure **each selected GPU independently**:

- fixed/locked core clock
- power limit
- fan percentage

MinerDesk translates those values to the syntax expected by each miner.

Current mapping:

| Engine | GPU selection | Core clock | Power limit | Fan |
| --- | --- | --- | --- | --- |
| SRBMiner | `--gpu-id 0,1` | `--gpu-cclock0 2200,1800` | `--gpu-plimit0 ...` | `--gpu-fan0 ...` |
| lolMiner | `--devices 0,1` | `--cclk ...` | `--pl ...` | `--fan ...` |
| Rigel | `-d 0,1` | `--lock-cclock ...` | `--pl ...` | `--fan-control ...` |
| BzMiner | `--enable ...` | `--oc_lock_core_clock ...` | `--oc_power_limit ...` | `--oc_fan_speed ...` |
| lpminer | system discovery / advanced args | `--lock-core-clock <MHz>` | advanced args | advanced args |

SRBMiner documents comma-separated per-GPU OC values in the same order as `--gpu-id`. lolMiner uses comma-separated lists and `*` to skip a GPU. Rigel uses comma-separated lists and `_` to skip a GPU. BzMiner uses space-separated per-device OC values. lpminer 0.1.x exposes one shared absolute core lock through `--lock-core-clock`; MinerDesk uses the latest effective profile value and refuses conflicting per-GPU lpminer core clocks instead of guessing. MinerDesk handles these differences automatically.

The old profile-wide GPU values are retained as **fallback/default values** for backward compatibility. Per-GPU entries override them.

### Scheduling

Mining profiles can be attached to one or more weekly time windows. Overnight windows such as `22:00 → 07:00` are supported.

Each schedule supports:

| Setting | Description |
| --- | --- |
| Start / End | Weekly mining window |
| Profiles | One or more mining profiles to run |
| After mining | None, Sleep, or Hibernate |
| Countdown | Delay before the power action |
| Require confirmation UI | If enabled, closing/cancelling the popup prevents sleep |
| Wake computer before start | Windows wake timer |
| Wake minutes before | Lead time before schedule start |

The scheduler runs in the Rust backend, not in the visible browser page. It therefore works in both desktop and headless modes.

#### Safety rules

MinerDesk will not automatically sleep/hibernate when:

- another mining window is still active;
- a manually started miner is still running;
- the confirmation UI is required but is no longer open;
- the user closes/cancels the countdown dialog.

#### Windows wake timer troubleshooting

After saving a schedule with wake enabled:

```powershell
Get-ScheduledTask | Where-Object TaskName -like 'MinerDesk Wake - *'
powercfg /waketimers
```

If the task exists but the PC does not wake, check **Control Panel → Power Options → Change advanced power settings → Sleep → Allow wake timers** and BIOS/UEFI sleep/wake support.

### Desktop web interface

On Windows, the privileged `minerdesk-headless.exe` backend serves the local HTTP API/web UI and the Tauri desktop connects to it. On Linux/macOS development builds the desktop may host the backend directly. By default it is available only on the same machine:

```text
http://127.0.0.1:17888/
```

The port can be changed in **Settings → Web interface & headless**.

You can also enable **LAN access**. MinerDesk then listens on `0.0.0.0` and requires a remote access token. Local loopback requests remain usable by the desktop app without a token.

Example remote URL:

```text
http://192.168.1.50:17888/?token=YOUR_TOKEN
```

Changing the desktop web port or LAN binding requires restarting MinerDesk.

### Headless web interface

The headless binary serves exactly the same frontend and API:

```powershell
minerdesk-headless.exe --listen 127.0.0.1 --port 17888
```

For LAN access:

```powershell
minerdesk-headless.exe --listen 0.0.0.0 --port 17888 --token YOUR_LONG_RANDOM_TOKEN
```

If `--listen`, `--port` or `--token` are omitted, the headless application uses the values saved in MinerDesk settings when possible. If listening on a non-loopback address without a token, a random token is generated for the session and printed in the terminal.

Do **not** expose MinerDesk directly to the public Internet. Prefer Tailscale, WireGuard or another VPN.

### Languages

The default UI language is **English**.

Built-in language packs are available for:

- English
- French
- Spanish
- Portuguese
- German
- Simplified Chinese
- Hindi
- Bengali
- Urdu
- Indonesian
- Arabic
- Russian
- Japanese

The language is changed from **Settings → Language** and is shared by the desktop and web interfaces. English is used as a fallback for technical strings that are not present in a language pack. Arabic and Urdu switch the document to RTL mode.

---

# 2. Windows — prerequisites

These prerequisites are needed only when **building MinerDesk from source**. End users installing a generated `Setup.exe` do not need Node.js or Rust.

## 2.1 Node.js LTS

Open PowerShell:

```powershell
winget install OpenJS.NodeJS.LTS
```

Close and reopen the terminal, then verify:

```powershell
node -v
npm -v
```

## 2.2 Rust / Cargo

```powershell
winget install --id Rustlang.Rustup
```

Close and reopen PowerShell, then select the MSVC toolchain:

```powershell
rustup default stable-msvc
rustc -V
cargo -V
```

## 2.3 Microsoft C++ Build Tools

Install **Visual Studio Build Tools 2022** and select the workload:

```text
Desktop development with C++
```

The MSVC compiler and Windows SDK are required by Tauri/Rust on Windows.

## 2.4 Microsoft Edge WebView2

Tauri uses Microsoft Edge WebView2 on Windows. It is normally already installed on modern Windows 10 and Windows 11 systems.

If Tauri reports a missing WebView2 runtime, install the Microsoft Edge WebView2 Evergreen Runtime.

If a previous elevated MinerDesk build already created a broken WebView2 data directory, close MinerDesk and remove only its WebView2 cache before launching 0.6.4:

```powershell
Remove-Item -Recurse -Force "$env:LOCALAPPDATA\com.minerdesk.app\EBWebView" -ErrorAction SilentlyContinue
```

The directory is recreated automatically under the normal user context.

---

# 3. Windows — prepare the source tree

Extract the MinerDesk source archive, for example:

```text
F:\Mining\MinerDesk-Tauri-0.6.4
```

Open PowerShell:

```powershell
cd F:\Mining\MinerDesk-Tauri-0.7.16
```

Install the JavaScript development dependencies:

```powershell
npm install --include=dev
```

You can also run the provided environment check/setup script:

```powershell
Set-ExecutionPolicy -Scope Process Bypass
.\scripts\setup-windows.ps1
```

---

# 4. Run the desktop application in development mode

Run the desktop development build from a normal PowerShell. For privileged GPU tuning, run `minerdesk-headless.exe` separately from an Administrator terminal, or test the installed privileged background backend after building the Setup.

```powershell
cd F:\Mining\MinerDesk-Tauri-0.7.16
npm run tauri:dev
```

Tauri will:

1. start Vite for the React frontend;
2. compile the Rust backend;
3. open the native MinerDesk desktop window.

The first Rust build is much slower than subsequent builds.

If Tauri reports that it cannot choose between `minerdesk` and `minerdesk-headless`, make sure `src-tauri/Cargo.toml` contains:

```toml
default-run = "minerdesk"
```

---

# 5. Initial application setup

## 5.1 Download a miner

Open **Miners**, create/select a profile, choose the engine and click **Download from GitHub**.

Managed miners are stored on Windows below:

```text
C:\ProgramData\MinerDesk\miners\
```

Examples:

```text
C:\ProgramData\MinerDesk\miners\srbminer\current\SRBMiner-MULTI.exe
C:\ProgramData\MinerDesk\miners\lolminer\current\lolMiner.exe
C:\ProgramData\MinerDesk\miners\bzminer\current\bzminer.exe
C:\ProgramData\MinerDesk\miners\rigel\current\rigel.exe
```

You can also browse to an executable manually.

## 5.2 Configure a profile

Fill in:

- algorithm
- pool
- wallet / username
- optional secondary wallet and merge separator
- password
- worker
- GPUs
- per-GPU tuning
- miner API port when applicable
- advanced arguments if required

Save the configuration before starting the profile.

## 5.3 GPU tuning

Select explicit GPUs using the checkboxes. Each selected GPU gets its own row for:

- Core clock MHz
- Power limit W
- Fan %

Blank values are not explicitly changed unless a legacy/default profile value is set.

On Windows, NVIDIA clock changes often require administrator rights. In 0.6.4 the desktop UI deliberately stays non-elevated; the recommended Setup installs the separate `minerdesk-headless.exe` background backend with highest privileges. The desktop talks to that backend over localhost.

---

# 6. Desktop web access

Open **Settings → Web interface & headless**.

## Local access

Choose a web port, for example:

```text
17888
```

After restarting MinerDesk, open:

```text
http://127.0.0.1:17888/
```

## LAN access

Enable **Allow LAN access**. MinerDesk binds the HTTP server to `0.0.0.0` and uses the configured access token.

After saving and restarting MinerDesk, open from another machine:

```text
http://YOUR_MINING_PC_IP:17888/?token=YOUR_TOKEN
```

If MinerDesk-managed Windows firewall rules are enabled, saving LAN web settings updates an inbound **Private network** TCP rule for the configured web port.

Never expose this HTTP port directly to the public Internet.

---

# 7. Run headless from source

Development/release run:

```powershell
npm run headless:run -- --listen 127.0.0.1 --port 17888
```

LAN example:

```powershell
npm run headless:run -- --listen 0.0.0.0 --port 17888 --token YOUR_LONG_RANDOM_TOKEN
```

The headless and desktop applications use the same configuration file.

On Windows it is normally located at:

```text
%APPDATA%\MinerDesk\config.json
```

On Linux it is normally located at:

```text
~/.config/MinerDesk/config.json
```

---

# 8. Build Windows Desktop + CLI + Setup.exe

MinerDesk includes a build script that creates all Windows artifacts.

Open PowerShell in the source directory:

```powershell
cd F:\Mining\MinerDesk-Tauri-0.7.16
Set-ExecutionPolicy -Scope Process Bypass
.\scripts\build-windows.ps1
```

The script performs these steps:

1. installs npm packages if `node_modules` is missing;
2. compiles the React/TypeScript frontend;
3. builds `minerdesk-headless.exe` in release mode;
4. copies the headless binary into the Tauri bundle resources;
5. builds the Tauri desktop executable;
6. builds the NSIS Windows installer;
7. copies distribution artifacts into `publish\windows-x64`.

Expected output:

```text
publish\windows-x64\
  MinerDesk.exe
  minerdesk-headless.exe
  MinerDesk_0.6.4_x64-setup.exe   # exact Tauri filename may vary
```

### `MinerDesk.exe`

Portable/raw desktop executable. It still depends on the normal Tauri/WebView2 runtime environment.

### `minerdesk-headless.exe`

CLI/headless executable. It runs the scheduler, miner processes, HTTP API and web interface without opening the Tauri window.

### `*-setup.exe`

Recommended Windows installer for end users.

---

# 9. Install using Setup.exe

Run the generated installer:

```text
MinerDesk_0.6.4_x64-setup.exe
```

The NSIS installer runs per-machine and installs MinerDesk under Program Files.

Depending on your choices, it can also:

- copy `minerdesk-headless.exe` next to the desktop executable;
- add the MinerDesk installation directory to the system `PATH`;
- enable MinerDesk-managed outbound firewall rules for miner executables;
- add the managed miner directory to Microsoft Defender exclusions.

The Defender exclusion is optional. If enabled, do not store unrelated files inside the MinerDesk miner directory.

After a PATH change, close existing terminals and open a new PowerShell/CMD window.

Test the CLI:

```powershell
minerdesk-headless.exe --help
```

---

# 10. Headless CLI reference

Show help:

```powershell
minerdesk-headless.exe --help
```

Use saved web settings:

```powershell
minerdesk-headless.exe
```

Override only the port:

```powershell
minerdesk-headless.exe --port 18080
```

Local-only explicit configuration:

```powershell
minerdesk-headless.exe --listen 127.0.0.1 --port 17888
```

LAN explicit configuration:

```powershell
minerdesk-headless.exe --listen 0.0.0.0 --port 17888 --token YOUR_LONG_RANDOM_TOKEN
```

Stop the headless process with `Ctrl+C`.

---

# 11. Windows Firewall and Microsoft Defender

## Miner outbound rules

When enabled, MinerDesk creates separate outbound allow rules for downloaded miner executables.

## Web UI inbound rule

No web port is opened inbound by default. When LAN web access is enabled and MinerDesk firewall management is enabled, MinerDesk creates a TCP inbound rule named:

```text
MinerDesk - Web UI inbound
```

The rule is limited to the Windows **Private** network profile and the configured MinerDesk web port.

## Defender exclusion

The optional exclusion targets only:

```text
C:\ProgramData\MinerDesk\miners
```

The uninstaller removes MinerDesk-created firewall rules and the Defender exclusion when the corresponding installer preference was enabled.

---

# 12. Linux development/build

Tauri 2 requires WebKitGTK 4.1 and several system libraries. On Debian/Ubuntu:

```bash
sudo apt update
sudo apt install -y \
  libwebkit2gtk-4.1-dev \
  build-essential \
  curl \
  wget \
  file \
  libxdo-dev \
  libssl-dev \
  libayatana-appindicator3-dev \
  librsvg2-dev
```

Install Rust:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
```

Install Node.js LTS using your preferred distribution method, then verify:

```bash
node -v
npm -v
rustc -V
cargo -V
```

Install dependencies and run:

```bash
npm install --include=dev
npm run tauri:dev
```

Headless:

```bash
npm run headless:run -- --listen 127.0.0.1 --port 17888
```

Build desktop bundles supported by your Linux environment:

```bash
npm run tauri:build
```

For portable Linux AppImage distribution, build on a sufficiently old compatible base distribution to avoid raising the minimum required glibc version.

---

# 13. macOS development

Install Xcode Command Line Tools:

```bash
xcode-select --install
```

Install Rust and Node.js LTS, then:

```bash
npm install --include=dev
npm run tauri:dev
```

Miner-specific GPU tuning support depends on the mining engine and vendor driver. Most GPU miners supported by MinerDesk primarily target Windows/Linux.

---

# 14. Configuration and upgrades

MinerDesk uses `serde(default)` for persisted configuration, so older configuration files can be opened after new fields are introduced.

Version 0.5 adds:

- `gpu_tuning[]` per mining profile;
- configurable desktop web port;
- optional LAN web exposure;
- persisted web access token;
- persisted UI language.

Older profile-wide `core_clock`, `power_limit` and `fan` fields are kept as fallback values for compatibility.

Before a major upgrade, back up:

```text
%APPDATA%\MinerDesk\config.json
```

---

# 15. Troubleshooting

## `tauri is not recognized`

Run inside the project directory:

```powershell
npm install --include=dev
npx tauri --version
```

Then:

```powershell
npm run tauri:dev
```

## Rust asks which binary to run

Verify:

```toml
# src-tauri/Cargo.toml
[package]
default-run = "minerdesk"
```

## GPU tuning returns `NoPermission`

Do **not** elevate `MinerDesk.exe`. Check that the `MinerDesk Privileged Backend` Scheduled Task is running. You can start it with `Start-ScheduledTask -TaskName "MinerDesk Privileged Backend"`. Some GPU BIOS/driver combinations may still refuse specific power/clock operations.

## Miner is quarantined by Defender

Use only the official miner release or MinerDesk's GitHub downloader. If you trust the binary, enable the optional MinerDesk-managed Defender exclusion rather than disabling Defender globally.

## Web page works locally but not from another computer

Check all of the following:

1. **Allow LAN access** is enabled.
2. MinerDesk was restarted after changing the binding/port.
3. Use the mining PC's LAN IP, not `127.0.0.1`.
4. The access token is present in the URL or request header.
5. The Windows network profile is **Private** if using the automatic inbound rule.
6. The configured TCP port is not already used by another program.

Check a port from another Windows PC:

```powershell
Test-NetConnection MINING_PC_IP -Port 17888
```

## Dashboard has no hashrate

Open **Console** and confirm that the miner prints a positive live hashrate. MinerDesk ignores initial `0 H/s` historical averages from SRBMiner so they do not overwrite a valid live value.

## A per-GPU OC setting is ignored

Different miners have different list/skip semantics. Explicitly select the GPUs instead of using **All GPUs**, then configure each selected GPU row. Check the Console command line that MinerDesk generated.

---

# 16. Security notes

- Wallet fields are intended for **public addresses/usernames only**.
- Never paste a seed phrase or private key into MinerDesk.
- Remote web access can start/stop mining, so protect it with a strong token.
- Prefer a VPN for Internet access.
- The built-in HTTP server does not provide public Internet TLS termination.
- Do not store unrelated software in a Defender-excluded mining directory.

---

# 17. Source layout

```text
src/                     React/TypeScript UI
src/i18n.ts              language packs and fallback translations
src-tauri/src/lib.rs     Rust orchestrator, scheduler, API, web server, miner adapters
src-tauri/src/main.rs    desktop entry point
src-tauri/src/bin/       headless CLI entry point
src-tauri/windows/       NSIS installer hooks
scripts/                 development/build helpers
```

---

## License

See `LICENSE`.
### Performance troubleshooting

If an older MinerDesk build felt sluggish while mining, upgrade to 0.6.4 before changing WebView2 or Windows settings. The 0.6.4 UI avoids continuous full-app renders, overlapping API polling, repeated PowerShell diagnostics, and heavy smooth-scrolling of live console logs. Local API requests also time out after 5 seconds so a stalled backend cannot accumulate polling requests indefinitely.
