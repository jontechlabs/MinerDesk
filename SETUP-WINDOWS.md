# Windows installation and build

## Install

Download the versioned x64 setup executable from [Releases](https://github.com/jontechlabs/MinerDesk/releases/latest), compare its SHA-256 with `SHA256SUMS.txt`, and run it. The release is unsigned. Windows 10/11 x64 and Microsoft Edge WebView2 Runtime are required.

The per-machine installer places MinerDesk under `C:\Program Files\MinerDesk` and configures **MinerDesk Privileged Backend**, with three retries at a minimum interval of one minute. The task runs `minerdesk-backend.exe --desktop-owned`. Desktop stays non-elevated. Do not substitute the standalone CLI for the windowless backend.

The desktop uses the privileged backend for mining, scheduling, GPU operations and optional firewall/Defender integration. A real desktop exit stops its owned backend and managed miners; close-to-tray keeps them running. Standalone `minerdesk-headless.exe` is independent, keeps its console output and handles Ctrl+C.

The zip contains all three executables for experienced users. Merely extracting it does not install the privileged scheduled task; use the setup executable for the normal desktop experience. Downloaded miners are not bundled.

## Build prerequisites

- Windows 10/11 x64.
- Node.js LTS and npm.
- Rust stable MSVC via rustup.
- Visual Studio Build Tools 2022 with **Desktop development with C++** and a Windows SDK.
- Microsoft Edge WebView2 Runtime.

```powershell
winget install OpenJS.NodeJS.LTS
winget install --id Rustlang.Rustup
rustup default stable-msvc
```

Install the Visual Studio C++ workload using its installer, then open a new terminal. See [Tauri's Windows prerequisites](https://v2.tauri.app/start/prerequisites/) for vendor details.

## Canonical release build

From the repository root:

```powershell
npm ci --include=dev
Set-ExecutionPolicy -Scope Process Bypass
.\scripts\build-windows.ps1
```

Do not use an npm-only packaging command as a replacement for this script. It:

1. Checks package, Rust and Tauri version consistency.
2. Runs the existing installer, task-policy, scheduler, background-worker, windowless-backend and mining-command regressions.
3. Builds the shared frontend and both release backends.
4. Checks actual PE headers: standalone CLI = console subsystem 3; windowless backend = Windows subsystem 2.
5. Copies verified backends to ignored resource files, builds the desktop and NSIS setup, and checks expected outputs.
6. Copies results to `publish/windows-x64/` only after all steps succeed and writes SHA-256 build metadata.

```text
publish/windows-x64/
  MinerDesk.exe
  minerdesk-backend.exe
  minerdesk-headless.exe
  MinerDesk_0.7.22_x64-setup.exe
  build-info.json
```

`tauri.windows.conf.json` preserves Windows-only installer options, hooks and resources. Clean source checkouts do not contain `.exe` files. `prepare-windows-resources.mjs` creates ignored bootstrap files for the first Cargo compilation; the release script replaces them with real, verified executables before packaging.

## Development and checks

```powershell
npm ci --include=dev
npm test
npm run tauri:dev
```

The standalone source checks can run without starting miners:

```powershell
.\tests\installer-safety.tests.ps1
.\tests\backend-task-settings.tests.ps1
.\tests\scheduler-control.tests.ps1
.\tests\config-sync.tests.ps1
.\tests\windowless-backend.tests.ps1
```

The scheduler/worker tests require `rustc`. These checks use synthetic process data and do not stop real processes or change live tasks. The installer itself is not executed by CI.

## Diagnostics and upgrade data

- Settings → backend diagnostics; `%APPDATA%\MinerDesk\backend.log`.
- Setup: `C:\ProgramData\MinerDesk\logs\setup-maintenance.log`.
- Uninstall: `C:\ProgramData\MinerDesk\logs\uninstall-maintenance.log`.
- Configuration and holds: `%APPDATA%\MinerDesk\`.
- Managed miner downloads: `C:\ProgramData\MinerDesk\miners\`.

Upgrades preserve configuration and downloaded miners. Maintenance stops only recognized installed runtime/miner processes, respecting installation boundaries. Do not publish diagnostic logs without removing tokens, wallets, pool credentials and personal paths. Optional Defender changes can be refused by Tamper Protection or policy; do not treat an exclusion as a security guarantee.
