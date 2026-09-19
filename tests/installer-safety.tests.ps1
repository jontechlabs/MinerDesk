#requires -Version 5.1
# No process is stopped and no Scheduled Task/registry setting is modified.
[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$passed = 0
function Assert-MdTest {
    param([bool]$Condition, [string]$Description)
    if (-not $Condition) { throw "Installer regression failed: $Description" }
    $script:passed++
    Write-Host "PASS: $Description"
}

foreach ($directory in @('scripts', 'src-tauri\windows', 'tests')) {
    foreach ($file in Get-ChildItem -LiteralPath (Join-Path $root $directory) -Filter '*.ps1') {
        $tokens = $null
        $errorsFound = $null
        [System.Management.Automation.Language.Parser]::ParseFile($file.FullName, [ref]$tokens, [ref]$errorsFound) | Out-Null
        if (@($errorsFound).Count -gt 0) { throw "PowerShell parse failure in $($file.FullName): $($errorsFound -join [Environment]::NewLine)" }
        Assert-MdTest $true "PowerShell parser: $($file.Name)"
    }
}

# Demonstrate the actual invalid quoting pattern carried by the old NSIS hooks.
$oldCode = @'
$desktop=
'@
$tokens = $null
$oldErrors = $null
[System.Management.Automation.Language.Parser]::ParseInput($oldCode, [ref]$tokens, [ref]$oldErrors) | Out-Null
Assert-MdTest (@($oldErrors).Count -gt 0) 'The truncated assignment emitted by old NSIS quoting is rejected by the PowerShell parser'

$hooks = Get-Content -LiteralPath (Join-Path $root 'src-tauri\windows\hooks.nsh') -Raw
Assert-MdTest (-not ($hooks -match '(?im)^\s*(?:nsExec::\w+|ExecWait).*\s-Command\s')) 'No inline PowerShell -Command in installer/uninstaller hooks'
Assert-MdTest (-not ($hooks -match '\$COMMONAPPDATA\b')) 'No undefined COMMONAPPDATA NSIS variable'
Assert-MdTest (-not ($hooks -match '(?im)^\s*Function\s+\.onGUIInit')) 'No duplicate reserved MUI GUI-init callback'
Assert-MdTest ($hooks.Contains('Pop $MdRawCode') -and $hooks.Contains('Pop $MdOutput')) 'Both nsExec stack results are consumed'
Assert-MdTest ($hooks.Contains('FileWriteUTF16LE /BOM')) 'Log and request files have explicit Unicode encoding'
Assert-MdTest ($hooks.Contains('"0.7.17" md_legacy_found')) 'Migration covers a partially installed 0.7.17 as well as older versions'
Assert-MdTest (([regex]::Matches($hooks, 'FileSeek \$MdHandle 0 END \$MdLogOffset')).Count -eq 2) 'Both NSIS log writers seek to EOF before appending'
Assert-MdTest ($hooks.Contains('$MdCode == 61') -and $hooks.Contains('$MdCode == 62')) 'Task registration/start have distinct diagnostic codes'

. (Join-Path $root 'src-tauri\windows\maintenance-common.ps1')
$install = 'C:\Program Files\MinerDesk'
$miners = 'C:\ProgramData\MinerDesk\miners'
$created = [datetime]'2026-01-01T10:00:00'
function New-MdFakeProcess {
    param([int]$Id, [int]$Parent, [string]$Name, [string]$Path, [datetime]$Created = ([datetime]'2026-01-01T10:00:00'))
    [pscustomobject]@{ ProcessId = [uint32]$Id; ParentProcessId = [uint32]$Parent; Name = $Name; ExecutablePath = $Path; CreationDate = $Created }
}
$setup = New-MdFakeProcess 101 50 'MinerDesk_0.7.22_x64-setup.exe' 'F:\Build\MinerDesk_0.7.22_x64-setup.exe'
$worker = New-MdFakeProcess 102 101 'MinerDesk.exe' 'C:\Users\Test\AppData\Local\Temp\ns123\MinerDesk.exe'
$helper = New-MdFakeProcess 103 102 'powershell.exe' 'C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe'
$desktop = New-MdFakeProcess 201 50 'minerdesk.exe' 'c:\program files\MINERDESK\minerdesk.exe'
$backend = New-MdFakeProcess 202 60 'minerdesk-headless.exe' 'C:\Program Files\MinerDesk\minerdesk-headless.exe'
$miner = New-MdFakeProcess 203 202 'lpminer.exe' 'C:\ProgramData\MinerDesk\miners\lpminer\current\lpminer.exe'
$standalone = New-MdFakeProcess 204 60 'minerdesk-headless.exe' 'F:\Standalone\minerdesk-headless.exe'
$similarFolder = New-MdFakeProcess 205 60 'lpminer.exe' 'C:\ProgramData\MinerDesk\miners-other\lpminer.exe'
$setupOnly = @($setup, $worker, $helper)
$protected = @(Get-MdProtectedIds -Processes $setupOnly -HelperId 103 -InstallerId 102 -InstallDir $install)
Assert-MdTest (@(Select-MdRuntime -Processes $setupOnly -InstallDir $install -MinersRoot $miners -ExcludedIds $protected -InstallerPath $setup.ExecutablePath).Count -eq 0) 'Setup bootstrap + worker alone do not count as runtime'
Assert-MdTest (@(Select-MdRuntime -Processes @() -InstallDir $install -MinersRoot $miners).Count -eq 0) 'Empty process list is not busy'
$setupFromDesktop = New-MdFakeProcess 101 201 'MinerDesk_0.7.22_x64-setup.exe' $setup.ExecutablePath
$chain = @($setupFromDesktop, $worker, $helper, $desktop)
$chainProtected = @(Get-MdProtectedIds -Processes $chain -HelperId 103 -InstallerId 102 -InstallDir $install)
Assert-MdTest (-not (201 -in $chainProtected) -and (101 -in $chainProtected) -and (102 -in $chainProtected)) 'A real Desktop parent is not confused with the protected Setup chain'
$all = @($setup, $worker, $helper, $desktop, $backend, $miner, $standalone, $similarFolder)
$targets = @(Select-MdRuntime -Processes $all -InstallDir $install -MinersRoot $miners -ExcludedIds $protected -InstallerPath $setup.ExecutablePath)
Assert-MdTest (($targets.ProcessId | Sort-Object) -join ',' -eq '201,202,203') 'Only installed Desktop/backend and managed miner are selected'
Assert-MdTest (-not (204 -in $targets.ProcessId)) 'Standalone backend in a different directory remains independent'
Assert-MdTest (-not (205 -in $targets.ProcessId)) 'Managed directory prefix has a path boundary'
$customInstall = "C:\Mining\John's MinerDesk"
$custom = New-MdFakeProcess 301 50 'minerdesk.exe' ([IO.Path]::Combine($customInstall, 'MinerDesk.exe'))
Assert-MdTest (@(Select-MdRuntime -Processes @($custom) -InstallDir $customInstall -MinersRoot $miners).Count -eq 1) 'Spaces/apostrophes in an install path are ordinary data'
$tree = @(Select-MdDescendants -Processes $all -Roots @($backend) -ExcludedIds $protected)
Assert-MdTest (($tree.ProcessId | Sort-Object) -join ',' -eq '202,203') 'Only the backend-owned descendant tree is selected'
$reused = New-MdFakeProcess 203 50 'lpminer.exe' $miner.ExecutablePath ($created.AddHours(1))
Assert-MdTest (-not (Test-MdSameProcess $miner $reused)) 'Reused PID with another creation time is not stopped'

$hiddenPath = New-MdFakeProcess 401 50 'minerdesk-headless.exe' ''
$inspectionFailed = $false
try { Select-MdRuntime -Processes @($hiddenPath) -InstallDir $install -MinersRoot $miners | Out-Null }
catch { $inspectionFailed = $_.Exception.Message -like '*Cannot read the executable path*' }
Assert-MdTest $inspectionFailed 'Unreadable candidate path is an inspection error, not an empty/busy assertion'

# When invoked by build-windows.ps1, exercise its real native-exit checker.
if (Get-Command Invoke-MdBuildNative -ErrorAction SilentlyContinue) {
    $nativeFailureCaught = $false
    try { Invoke-MdBuildNative -File $env:ComSpec -Arguments @('/d', '/c', 'exit', '7') }
    catch { $nativeFailureCaught = $_.Exception.Message -like '*exit 7*' }
    Assert-MdTest $nativeFailureCaught 'A native command returning 7 terminates the build step'
}

# Test the exact UTF-16 request format used by NSIS (including non-ASCII text).
$tempFile = Join-Path ([IO.Path]::GetTempPath()) ('md-request-test-' + [guid]::NewGuid().ToString('N') + '.ini')
try {
    $unicodePath = 'F:\Mining\' + [char]0x00e9 + "quipe\John's MinerDesk"
    [IO.File]::WriteAllText($tempFile, "[Maintenance]`r`nAction=Inspect`r`nInstallDir=$unicodePath`r`nApproved=0`r`n", [Text.Encoding]::Unicode)
    $roundTrip = Read-MdRequest $tempFile
    Assert-MdTest ($roundTrip['InstallDir'] -ceq $unicodePath) 'UTF-16 request round-trip preserves Unicode, spaces and apostrophes'
} finally { Remove-Item -LiteralPath $tempFile -Force -ErrorAction SilentlyContinue }
Write-Host "Installer safety preflight passed: $passed assertions. No running process was modified." -ForegroundColor Green
