$ErrorActionPreference = "Stop"

$installDir = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$headless = Join-Path $installDir "src-tauri\target\release\minerdesk-backend.exe"
if (-not (Test-Path $headless)) {
  $installed = Join-Path ${env:ProgramFiles} "MinerDesk\minerdesk-backend.exe"
  if (Test-Path $installed) { $headless = $installed }
}
if (-not (Test-Path $headless)) {
  throw "minerdesk-backend.exe was not found. Build MinerDesk first or install the Setup package."
}

$taskName = "MinerDesk Privileged Backend"
Stop-ScheduledTask -TaskName $taskName -ErrorAction SilentlyContinue
Get-CimInstance Win32_Process -ErrorAction SilentlyContinue | Where-Object { $_.ExecutablePath -and $_.ExecutablePath.Equals($headless,[System.StringComparison]::OrdinalIgnoreCase) } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }

# Use the interactive desktop user rather than the identity that happened to
# elevate PowerShell/Setup. This matters when UAC credentials belong to a
# different administrator account.
$user = (Get-CimInstance Win32_ComputerSystem).UserName
if ([string]::IsNullOrWhiteSpace($user)) {
  $user = [System.Security.Principal.WindowsIdentity]::GetCurrent().Name
}

$workingDir = Split-Path -Parent $headless
$action = New-ScheduledTaskAction -Execute $headless -Argument '--desktop-owned' -WorkingDirectory $workingDir
$trigger = New-ScheduledTaskTrigger -AtLogOn -User $user
$principal = New-ScheduledTaskPrincipal -UserId $user -LogonType Interactive -RunLevel Highest
$settings = New-ScheduledTaskSettingsSet `
  -StartWhenAvailable `
  -RestartCount 3 `
  -RestartInterval (New-TimeSpan -Minutes 1) `
  -ExecutionTimeLimit ([TimeSpan]::Zero) `
  -MultipleInstances IgnoreNew

Register-ScheduledTask `
  -TaskName $taskName `
  -Action $action `
  -Trigger $trigger `
  -Principal $principal `
  -Settings $settings `
  -Description "MinerDesk privileged mining backend" `
  -Force | Out-Null

Start-ScheduledTask -TaskName $taskName
Start-Sleep -Seconds 2

$process = @(Get-CimInstance Win32_Process -ErrorAction SilentlyContinue | Where-Object { $_.ExecutablePath -and $_.ExecutablePath.Equals($headless,[System.StringComparison]::OrdinalIgnoreCase) })
if ($process.Count -gt 0) {
  Write-Host "Installed and started: $taskName" -ForegroundColor Green
  Write-Host "Backend executable: $headless"
  Write-Host "Backend PID: $($process.ProcessId -join ', ')"
} else {
  Write-Warning "The Scheduled Task was created but minerdesk-backend.exe is not running."
  Write-Warning "Check: $env:APPDATA\MinerDesk\backend.log"
  Write-Warning "And run: Get-ScheduledTaskInfo -TaskName '$taskName' | Format-List *"
}
