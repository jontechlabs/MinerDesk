#requires -Version 5.1
# Read-only source/ownership preflight. Actual PE headers are also checked after
# compilation by build-windows.ps1. No runtime is started or stopped here.
[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$checks = 0
function Assert-MdWindowlessTest {
    param([bool]$Condition, [string]$Description)
    if (-not $Condition) { throw "Windowless backend regression: $Description" }
    $script:checks++
    Write-Host "PASS: $Description"
}
$backend = Get-Content -LiteralPath (Join-Path $root 'src-tauri\src\bin\minerdesk-backend.rs') -Raw
$cli = Get-Content -LiteralPath (Join-Path $root 'src-tauri\src\bin\minerdesk-headless.rs') -Raw
Assert-MdWindowlessTest ($backend.Contains('windows_subsystem = "windows"') -and $backend.Contains('run_desktop_backend()')) 'Backend uses GUI subsystem and the actual shared backend (no launcher wrapper)'
Assert-MdWindowlessTest (-not $cli.Contains('windows_subsystem') -and $cli.Contains('run_headless()')) 'Standalone CLI retains the console subsystem'
$library = Get-Content -LiteralPath (Join-Path $root 'src-tauri\src\lib.rs') -Raw
Assert-MdWindowlessTest ($library.Contains('let desktop_owned = windowless || args.desktop_owned;')) 'Windowless backend is always Desktop-owned; CLI still opts in explicitly'
Assert-MdWindowlessTest ($library.Contains('if windowless { std::future::pending::<()>().await; }')) 'Windowless runtime does not depend on a console Ctrl+C handler'
Assert-MdWindowlessTest ($library.Contains('background_command("nvidia-smi")') -and $library.Contains('background_command("powercfg.exe")')) 'GPU/power diagnostics do not open extra consoles'
Assert-MdWindowlessTest ($library.Contains('fn focus_main_window(app: tauri::AppHandle)') -and $library.Contains('window.set_focus()')) 'Desktop exposes a native foreground/focus command for schedule power confirmation'
Assert-MdWindowlessTest ($library.Contains('start_power_action_focus_watch') -and $library.Contains('/api/power/peek') -and $library.Contains('It deliberately does NOT mark the')) 'Native Rust watcher raises hidden Desktop without falsely renewing the confirmation UI lease'
$appSource = Get-Content -LiteralPath (Join-Path $root 'src\App.tsx') -Raw
Assert-MdWindowlessTest ($appSource.Contains('(!isTauri && document.visibilityState !== "visible")') -and $appSource.Contains('invoke("focus_main_window")')) 'Native Desktop keeps power polling while hidden and raises the confirmation modal'
Assert-MdWindowlessTest ($library.Contains('self.start_miner_mode_locked(id, started_by, false)?;') -and -not $library.Contains('let origin = if started_by == "manual"')) 'Explicit manual Start remains user-owned instead of becoming schedule-owned'
$config = Get-Content -LiteralPath (Join-Path $root 'src-tauri\tauri.windows.conf.json') -Raw | ConvertFrom-Json
Assert-MdWindowlessTest ($config.bundle.resources -contains 'resources/minerdesk-backend.exe' -and $config.bundle.resources -contains 'resources/minerdesk-headless.exe') 'Installer bundles both backend and standalone CLI'
$hooks = Get-Content -LiteralPath (Join-Path $root 'src-tauri\windows\hooks.nsh') -Raw
Assert-MdWindowlessTest ($hooks.Contains('"$INSTDIR\resources\minerdesk-backend.exe" "$INSTDIR\minerdesk-backend.exe"')) 'Installer copies the new backend beside the CLI'
Assert-MdWindowlessTest ($hooks.Contains('Delete "$INSTDIR\minerdesk-backend.exe"')) 'Uninstaller removes the new root binary'
Assert-MdWindowlessTest ($hooks.Contains('"0.7.19" md_legacy_found')) 'Migration handles the installed 0.7.19'
$build = Get-Content -LiteralPath (Join-Path $root 'scripts\build-windows.ps1') -Raw
Assert-MdWindowlessTest ($build.Contains('Assert-MdExeSubsystem -Path $backend -Expected 2') -and $build.Contains('Assert-MdExeSubsystem -Path $headless -Expected 3')) 'Distribution checks actual GUI/console PE subsystem values'
. (Join-Path $root 'src-tauri\windows\maintenance-common.ps1')
$install = 'C:\Program Files\MinerDesk'
$miners = 'C:\ProgramData\MinerDesk\miners'
$items = @(
    [pscustomobject]@{ ProcessId=901; Name='minerdesk-backend.exe'; ExecutablePath=(Join-Path $install 'minerdesk-backend.exe') },
    [pscustomobject]@{ ProcessId=902; Name='minerdesk-backend.exe'; ExecutablePath=(Join-Path $install 'resources\minerdesk-backend.exe') },
    [pscustomobject]@{ ProcessId=903; Name='minerdesk-backend.exe'; ExecutablePath='F:\Other\minerdesk-backend.exe' },
    [pscustomobject]@{ ProcessId=904; Name='minerdesk-headless.exe'; ExecutablePath=(Join-Path $install 'minerdesk-headless.exe') }
)
$selected = @(Select-MdRuntime -Processes $items -InstallDir $install -MinersRoot $miners)
Assert-MdWindowlessTest (($selected.ProcessId | Sort-Object) -join ',' -eq '901,902,904') 'Maintenance includes both installed backend forms and legacy CLI, not unrelated directories'
Write-Host "Windowless backend preflight passed: $checks assertions." -ForegroundColor Green
