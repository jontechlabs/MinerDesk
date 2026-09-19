# MinerDesk 0.7.21 — Windows installation and tests

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


## Build from a new extracted folder

Open PowerShell in the folder containing package.json:

```powershell
Set-ExecutionPolicy -Scope Process Bypass
.\scripts\build-windows.ps1
```

Wait for `BUILD SUCCEEDED: MinerDesk 0.7.21`, then launch:

```powershell
.\publish\windows-x64\MinerDesk_0.7.21_x64-setup.exe
```

Accept maintenance for the previous installed version. Setup handles 0.7.18 as
well as the older supported 0.7.x registrations. AppData configuration, saved
manual schedule holds, and downloaded miners remain in place. The matching CLI
and windowless backend are rebuilt and embedded; the resources in this source
ZIP are placeholders, not ready-to-run binaries.

## Installed process layout

```text
MinerDesk.exe                     Desktop / tray
minerdesk-backend.exe             Privileged Desktop-owned backend, no terminal
minerdesk-headless.exe            Optional independent console CLI
```

The new backend is the actual runtime, not a wrapper. Its miners retain the
per-miner Job Object protection. Setup, Repair backend and elevated fallback
launch the new executable. Choosing not to install the privileged task still
allows the Desktop's UAC fallback to start the windowless backend.

## Verify the task and window behavior

```powershell
$task = Get-ScheduledTask -TaskName 'MinerDesk Privileged Backend' -ErrorAction Stop
$task.Actions | Format-List Execute, Arguments, WorkingDirectory
$task.Settings | Select-Object RestartCount, RestartInterval
```

Execute must end in `\minerdesk-backend.exe`, Arguments must be `--desktop-owned`.
RestartCount remains 3; RestartInterval remains PT1M. Desktop use should not open
a persistent console. The genuine CLI remains available explicitly:

```powershell
& "$env:ProgramFiles\MinerDesk\minerdesk-headless.exe" --port 17889
```

The separate port avoids the Desktop backend's default port during this test.
A CLI instance without `--desktop-owned` remains independent; Ctrl+C stops it
and its own miners. Existing enabled schedules from the shared configuration
can apply to this CLI too, so perform CLI tests only with schedules disabled or
on a deliberately controlled test configuration/GPU session.

## Verify manual scheduler Stop

Let a short schedule launch a profile. Stop it, then wait 10 seconds: STOP and
00:00:00 should persist with a manual-pause badge. Start resumes mining as a manual/user-owned session; the scheduler does not stop
that session when the configured window ends. Stop all holds all current scheduled
profiles, including ones not running at that instant. The next dated occurrence
runs normally. Overnight windows keep the same hold across midnight.

Pause state is beside config.json as scheduler-manual-stops.json. Restarting
only the backend inside the same window must not erase the hold. No permanent
schedule disablement is introduced. Overlapping windows are documented in README.


## Verify manual Start after schedule end and power-dialog focus

Create a short schedule with a sleep/hibernate post action. Let the schedule end.
When the confirmation modal is created, the native MinerDesk window should restore
from minimized/close-to-tray state and come to the foreground. **Cancel** is the
default focused action.

After cancelling the power action, press Start on the profile. The profile must
start as a manual session and stay running even though the schedule is already
finished. Also test clicking Start very close to the configured end boundary: the
next scheduler tick must not stop that manually started session. A manual Start or
Restart cancels any still-pending sleep/hibernate request.

## Build validation and diagnostics

The build runs the real PowerShell parser and synthetic installer/task tests,
then compiles and runs 15 pure production scheduler-policy tests with rustc.
Post-compilation checks require PE subsystem 2 for minerdesk-backend and 3 for
minerdesk-headless. Failing compilation or checks prevents publishing a new Setup.

```powershell
Get-Content -LiteralPath "$env:APPDATA\MinerDesk\backend.log" -Tail 100
Get-Content -LiteralPath "$env:ProgramData\MinerDesk\logs\setup-maintenance.log" -Tail 100
```

Use VALIDATION-0.7.21.md for the exact tests run here versus still requiring
Windows. No Windows installer or GPU-mining runtime was tested in the preparation
environment.

---

## Historical setup notes (0.7.18 and earlier; superseded by the instructions above)

# MinerDesk 0.7.18 — Windows build and installation

## Build with your existing workflow

Extract into a new directory. Open PowerShell in the folder containing
`package.json` and run:

```powershell
Set-ExecutionPolicy -Scope Process Bypass
.\scripts\build-windows.ps1
```

The script runs the PowerShell parser, installer-safety checks and the new backend
task policy tests before compiling. These tests do not kill processes or register
scheduled tasks. Windows PowerShell 5.1+, Node/npm, Rust and MSVC build tools are
required. Build errors stop publication; an old installer is not a successful
output of this build.

Wait for `BUILD SUCCEEDED: MinerDesk 0.7.18`, then run:

```powershell
.\publish\windows-x64\MinerDesk_0.7.18_x64-setup.exe
```

Accept the installer maintenance prompt for your existing or partially installed
MinerDesk. There is no need to remove registry keys manually. Configuration in
AppData and downloaded miners are preserved. The fresh Setup has a valid backend
restart policy: three retry attempts at one-minute intervals (`PT1M`, not `PT15S`).
This is the Windows retry interval, not the Desktop heartbeat timeout.

## Verify the installed backend task

After a successful installation with the privileged backend selected, run:

```powershell
$task = Get-ScheduledTask -TaskName 'MinerDesk Privileged Backend' -ErrorAction Stop
$task.Settings | Select-Object RestartCount, RestartInterval, MultipleInstances, ExecutionTimeLimit
$task.Actions | Select-Object Execute, Arguments, WorkingDirectory
```

Expected: `RestartCount = 3`, `RestartInterval = PT1M` (or an equivalent 60-second
XML duration), executable under your chosen install directory and argument
`--desktop-owned`. Check the task settings, not the GPU state, to verify this
particular correction.

## Read diagnostics

```powershell
Get-Content -LiteralPath "$env:ProgramData\MinerDesk\logs\setup-maintenance.log" -Tail 100
```

Direct uninstall uses `uninstall-maintenance.log` in the same directory. The
installer and helper both append; earlier phases are no longer overwritten.
The error dialog identifies the phase and code. Codes 61 and 62 distinguish task
registration from task-start failures. A registered task's XML is exported on a
best-effort basis to `backend-task.xml` beside the log.

## Source package and testing

`src-tauri/resources/minerdesk-headless.exe` is a zero-byte placeholder in the
source ZIP. The build script must replace it with the freshly compiled headless
binary. This ZIP is not a precompiled installer.

The preparation environment validated the source and Interval XML constraint,
not Windows Task Scheduler registration or the full Setup. Refer to
`VALIDATION-0.7.18.md` for exact test scope.

---

## Historical setup notes (superseded where inconsistent with 0.7.18)

# MinerDesk 0.7.16 — Windows Setup Quick Reference



## Setup self-detection fix in 0.7.16

The NSIS installer normally appears as two related processes: a 32-bit bootstrap process and an elevated installer worker. Task Manager can display/group that chain under the MinerDesk product name. Older hooks used `Get-Process -Name MinerDesk`, which could therefore make Setup believe **its own installer process** was the installed Desktop application.

0.7.16 never decides ownership from a process name or Task Manager display label. It only treats these exact installed paths as the Desktop/backend:

```text
C:\Program Files\MinerDesk\MinerDesk.exe
C:\Program Files\MinerDesk\minerdesk-headless.exe
```

Managed miners are still identified by their trusted `C:\ProgramData\MinerDesk\miners\...` location. The NSIS Setup bootstrap/elevated child is excluded automatically because it runs from the Setup executable/temporary installer path, not from the installed Desktop path.

### Build command

Use the project build script exactly as follows:

```powershell
Set-ExecutionPolicy -Scope Process Bypass
.\scripts\build-windows.ps1
```

This is the canonical distribution build. It runs `npx tauri build --bundles nsis` internally after compiling and embedding the matching headless binary. The build script does **not** start the generated installer.


## Legacy registry-view and diagnostic-log fix in 0.7.15

Version 0.7.14 could successfully stop/remove the old application files and still fail on the final uninstall-registration check. The uninstall key belongs to the registry view selected by Tauri/NSIS; a PowerShell helper can see a different WOW64 view. In 0.7.15, PowerShell only handles processes/files/integrations and NSIS removes/verifies the old uninstall registration itself in both registry views.

The persistent diagnostic log is now:

```text
C:\ProgramData\MinerDesk\logs\legacy-upgrade.log
```

Open it from an elevated or normal PowerShell with:

```powershell
notepad "$env:ProgramData\MinerDesk\logs\legacy-upgrade.log"
```

`%TEMP%\MinerDesk-legacy-upgrade.log` is used only if ProgramData logging cannot be created. Setup writes the log before PowerShell starts.

## Legacy cleanup launcher fix in 0.7.14

0.7.13 could show `could not be cleaned up safely (code error)` before the PowerShell cleanup helper ever started. This was a launcher/quoting problem, not evidence that a MinerDesk process was still running. 0.7.14 launches the helper with NSIS `ExecWait` and creates `%TEMP%\MinerDesk-legacy-upgrade.log` before PowerShell starts.

If cleanup still fails, open the log immediately after the message:

```powershell
notepad "$env:TEMP\MinerDesk-legacy-upgrade.log"
```

The file now exists even for a PowerShell launch failure and contains the staged helper path plus the PowerShell executable selected by Setup.

## Upgrade cleanup fix in 0.7.13

0.7.13 avoids Tauri's old-uninstaller path for known MinerDesk versions 0.7.3 through 0.7.12. During installer GUI initialization, before the built-in reinstall page runs, Setup asks permission to remove the old application while preserving AppData settings and downloaded miners. The cleanup helper is executed directly by the new Setup; it does **not** replace `UninstallString`.

If cleanup fails, inspect:

```text
%TEMP%\MinerDesk-legacy-upgrade.log
```

The helper returns explicit codes: `20` for remaining MinerDesk/miner processes, `30` when the old `MinerDesk.exe` is still locked, and `31` when the old uninstall registry key could not be removed.


## Legacy upgrade fix in 0.7.12

If an installed 0.7.x build (0.7.3 through 0.7.11) has a broken legacy uninstaller, Setup no longer relies on that `uninstall.exe`. At GUI initialization it temporarily redirects the old Windows `UninstallString` to the embedded `legacy-upgrade-cleanup.ps1` helper. When Tauri asks to uninstall the previous version, that helper performs the cleanup and removes the old `MinerDesk.exe`, allowing Setup to continue. User configuration under AppData is preserved. Cancelling Setup before the helper runs restores the original `UninstallString`.

This specifically fixes the 0.7.9 message `Uninstall could not safely stop the MinerDesk backend` even when Task Manager shows no MinerDesk process.



## NSIS build fix in 0.7.11

Tauri/NSIS Modern UI generates its own `.onGUIInit` callback. MinerDesk 0.7.10 declared another function with that reserved name, causing `makensis` to abort with `Function named ".onGUIInit" already exists`. Version 0.7.11 registers the same early maintenance check through `MUI_CUSTOMFUNCTION_GUIINIT`, which is the supported MUI2 extension point. The runtime behavior is unchanged: the guard still runs before the reinstall page can invoke the previous uninstaller.

## Installer/uninstaller maintenance fix in 0.7.10

If MinerDesk is still running when a new Setup is opened, the new installer now checks and prompts **before** Tauri launches the previous version's `uninstall.exe`. Accepting the prompt stops `MinerDesk.exe`, the privileged backend task/process, and MinerDesk-managed miners, then verifies that they are gone before the previous uninstaller or file replacement continues.

This is especially important when upgrading from 0.7.9: its uninstaller could display `Uninstall could not safely stop the MinerDesk backend` while Desktop was loaded. The 0.7.10 Setup pre-closes the running components before that old uninstaller is invoked.

The 0.7.10 uninstaller also performs shutdown in separate checked stages instead of one long PowerShell command, which makes failures deterministic and avoids the generic backend-shutdown error.

Manual recovery, from an **Administrator PowerShell** in the source tree:

```powershell
.\scripts\stop-minerdesk-maintenance.ps1 -RemoveTask
```

## Mining-session fixes in 0.7.9

- STOP now resets the current profile uptime to `00:00:00`, including natural/crash exits detected by the backend.
- Start/Restart persists the current UI configuration first and aborts the launch if saving fails.
- lpminer receives the latest configured absolute core clock through `--lock-core-clock`, preventing a legacy value such as 2200 MHz from being reused after editing then Stop/Start.

## Miner crash protection in 0.7.8

On Windows, every miner process is started under a dedicated Job Object with `KILL_ON_JOB_CLOSE`. MinerDesk creates each miner suspended, attaches it to the Job Object, and resumes it only after the attachment succeeds. If `minerdesk-headless.exe` crashes or is force-killed, Windows terminates the protected miner process tree automatically. MinerDesk refuses to leave a miner running if the crash guard cannot be attached.

After starting a miner, **Settings → Miner crash guard** should display `Active · Windows Job Object`. The headless console also prints:

```text
Miner crash guard: Windows Job Object (kill-on-close)
```

A useful destructive test on a non-critical mining session is to start one miner, note its PID with `nvidia-smi`, then force-kill only `minerdesk-headless.exe`. The miner should disappear automatically without needing a separate `taskkill` for the miner.

## Uninstall safety in 0.7.8

If MinerDesk Desktop, `minerdesk-headless.exe`, or the Windows task `MinerDesk Privileged Backend` is still active, uninstall now asks whether it may stop MinerDesk and all MinerDesk-managed mining processes. Choosing **Yes** disables/stops the task, ends Desktop/headless process trees, waits until the backend is gone, unregisters the task, and only then deletes the installed files. Choosing **No** cancels uninstall without removing components. The task cleanup is unconditional once uninstall proceeds, so stale tasks from older releases are also removed.

## Upgrade safety in 0.7.8

Before an install/upgrade replaces binaries, Setup checks `MinerDesk.exe`, `minerdesk-headless.exe`, the privileged Scheduled Task, **and any executable still running from `C:\ProgramData\MinerDesk\miners`**. This means an orphaned miner left by an older crash is detected even if Desktop/headless has already disappeared. Setup asks before force-closing anything; accepting stops the task/process trees and managed miners, while declining cancels Setup and leaves the running processes untouched.


## 0.7.0 Windows behavior options

Both **Start MinerDesk with Windows** and **Minimize to the notification area when closing** are disabled by default and can be enabled from **Settings → Windows behavior**. The startup option uses the current user Run registry key; close-to-tray provides Open/Quit actions from the MinerDesk tray icon.

In **0.7.5**, a real Desktop exit shuts down only the **Desktop-owned** privileged `minerdesk-headless.exe` backend. Closing to the tray is intentionally not treated as an exit. A `minerdesk-headless.exe` instance launched manually from a terminal is standalone and is not tied to the Desktop heartbeat.

## Desktop window and backend lifecycle

MinerDesk Desktop opens at 1480×900 by default. Starting with **0.7.5**, the installed privileged backend is started with `--desktop-owned`: quitting MinerDesk stops active miners and that managed backend. If **Minimize to the notification area when closing** is enabled, the close button only hides the window, so mining continues while MinerDesk remains active in the tray. Manual CLI/headless launches do not use the Desktop watchdog.

## Important Windows backend rule

On Windows, `MinerDesk.exe` is intentionally non-elevated. All mining processes and GPU tuning are owned by the privileged `minerdesk-headless.exe` backend installed as a Scheduled Task. The desktop must not own port 17888.

After installation, verify:

```powershell
Get-ScheduledTask -TaskName "MinerDesk Privileged Backend"
Get-Process minerdesk-headless -ErrorAction SilentlyContinue
$listener = Get-NetTCPConnection -LocalPort 17888 -State Listen -ErrorAction SilentlyContinue
$listener
if ($listener) { Get-Process -Id $listener.OwningProcess }
```

Expected owner: `minerdesk-headless`. If the task exists but the process is not running, inspect:

```text
%APPDATA%\MinerDesk\backend.log
```

and:

```powershell
Get-ScheduledTaskInfo -TaskName "MinerDesk Privileged Backend" | Format-List *
```

## Important change in 0.6.4

`MinerDesk.exe` is no longer marked `requireAdministrator`. It runs as a normal desktop application to keep WebView2 compatible with Windows 11 Administrator Protection.

During installation, accept **Install the privileged MinerDesk background backend (Recommended)**. The setup creates the scheduled task `MinerDesk Privileged Backend`, which launches `minerdesk-headless.exe` with highest privileges at logon.

The desktop automatically connects to this backend on the configured local Web port. If the backend is unavailable on Windows, the desktop reports a connection error instead of silently mining without the required privileges.



## Backend controls in 0.6.4

The Windows desktop now checks the privileged backend automatically at startup. If the Scheduled Task exists, MinerDesk starts it and waits for `http://127.0.0.1:<port>/api/health`. If no task exists (typical source-development case), MinerDesk can launch only `minerdesk-headless.exe` elevated while leaving `MinerDesk.exe` non-admin.

The Desktop UI displays a permanent **Privileged backend** strip with:

- **Start backend** — start the Scheduled Task, or use an elevated headless fallback;
- **Restart backend** — restart the installed task/backend;
- **Repair backend** — recreate the highest-privilege Scheduled Task (UAC prompt);
- **Refresh** — refresh task/process/port diagnostics.

When offline, expand **Backend diagnostics** to see the Scheduled Task state, last task result, executable path, port listener and the tail of `%APPDATA%\MinerDesk\backend.log`.

### Development

Run development mode from a normal, non-administrator PowerShell:

```powershell
npm install --include=dev
npm run tauri:dev
```

On Windows, `npm run tauri:dev` now builds the debug `minerdesk-headless.exe` first. If no privileged Scheduled Task is installed, the desktop can start that debug backend via UAC. A second PowerShell window is no longer required.

The canonical and complete build/install documentation is now in [`README.md`](README.md).

## Build all Windows artifacts

```powershell
cd F:\Mining\MinerDesk-Tauri-0.7.16
npm install --include=dev
Set-ExecutionPolicy -Scope Process Bypass
.\scripts\build-windows.ps1
```

Expected artifacts:

```text
publish\windows-x64\
  MinerDesk.exe
  minerdesk-headless.exe
  MinerDesk_0.7.16_x64-setup.exe   # exact Tauri-generated filename may vary
```

For prerequisites, desktop development, headless usage, LAN web access, firewall behavior, per-GPU tuning, localization, Linux/macOS builds and troubleshooting, read `README.md` from top to bottom.


## Important distribution-build note

For the Windows distribution package, use:

```powershell
Set-ExecutionPolicy -Scope Process Bypass
.\scripts\build-windows.ps1
```

Do not invoke `tauri build --bundles nsis` directly when preparing a distribution package. The MinerDesk build script first compiles the matching `minerdesk-headless.exe`, embeds it into the Tauri resources, and only then creates the NSIS installer.
## Power-aware schedules (0.6.4)

MinerDesk 0.6.4 can put Windows to Sleep or Hibernate after the last scheduled mining window ends and can create Windows wake timers before mining starts.

### Requirements

- Install the **MinerDesk Privileged Backend** from Setup. Power actions and wake-task creation require elevated rights.
- For wake-up, Windows **Allow wake timers** must be enabled in the active power plan.
- BIOS/UEFI and the selected sleep state must support timed wake-up.
- Wake timers resume from Sleep/Hibernate. They do not guarantee wake from a fully powered-off PC.

### Verify wake timers

After saving a MinerDesk schedule with **Wake computer before start** enabled:

```powershell
Get-ScheduledTask | Where-Object TaskName -like 'MinerDesk Wake - *'
powercfg /waketimers
```

### Confirmation countdown

If **Require an open Desktop/Web confirmation window** is enabled, MinerDesk displays a countdown popup at the end of the schedule. Closing or cancelling the popup prevents the power action. If no UI is connected, the backend cancels the pending power action when the countdown expires.

For unattended/headless operation, disable confirmation only if you explicitly want automatic Sleep/Hibernate without an open UI.


## 0.6.7 UI/backend fixes

Version 0.6.7 fixes scheduler checkbox sizing, the dashboard action-cell border artifact, and transient privileged-backend false-offline states. No additional Windows prerequisite is required.


## 0.6.7 notes

- Run `MinerDesk.exe` as a normal user. The privileged background backend performs GPU tuning, Windows Firewall changes and Microsoft Defender exclusion changes.
- The Desktop shows a startup splash while it connects to the privileged backend.
- If **Stop the privileged backend when MinerDesk Desktop closes** is enabled, the setting is saved immediately and the Desktop requests a graceful backend shutdown when the window closes.
- If the Defender button reports an error, check Windows Security / Tamper Protection and organization policies; MinerDesk now surfaces the real backend/PowerShell error.


## 0.6.7 startup behavior

When `MinerDesk.exe` starts, its splash screen remains visible while the Desktop checks `127.0.0.1:<configured-port>`, starts the privileged scheduled backend when necessary, waits for `/api/health`, and loads the initial configuration. If Windows elevation is required, the UAC prompt appears while the splash remains visible.

The Web access token shown by **Generate token** is intentionally not activated until **Save** is pressed. This keeps the current API session alive and prevents the generated value from being overwritten by an immediate backend refresh.
