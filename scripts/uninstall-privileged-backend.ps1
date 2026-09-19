$ErrorActionPreference = "SilentlyContinue"

$taskName = "MinerDesk Privileged Backend"
$minersRoot = Join-Path $env:ProgramData "MinerDesk\miners"

$task = Get-ScheduledTask -TaskName $taskName -ErrorAction SilentlyContinue
$headlessPath = $null
if ($task -and $task.Actions) {
  $headlessPath = [string]$task.Actions[0].Execute
}
if ([string]::IsNullOrWhiteSpace($headlessPath)) {
  $headlessPath = Join-Path $env:ProgramFiles 'MinerDesk\minerdesk-backend.exe'
}
try { $headlessPath = [System.IO.Path]::GetFullPath($headlessPath.Trim('"')) } catch {}

function Get-InstalledHeadlessProcess {
  @(Get-CimInstance Win32_Process -ErrorAction SilentlyContinue | Where-Object {
    $_.ExecutablePath -and $_.ExecutablePath.Equals($headlessPath,[System.StringComparison]::OrdinalIgnoreCase)
  })
}

# Disable first so Windows cannot relaunch the backend while we are shutting it
# down. Use both ScheduledTasks and schtasks.exe because either API can lag behind
# the actual task/process state on some Windows builds.
Disable-ScheduledTask -TaskName $taskName | Out-Null
Stop-ScheduledTask -TaskName $taskName
& schtasks.exe /End /TN $taskName 2>$null | Out-Null
Start-Sleep -Milliseconds 300

# Terminate the complete backend process tree so child mining engines stop too.
Get-InstalledHeadlessProcess | ForEach-Object {
  & taskkill.exe /F /T /PID $_.ProcessId 2>$null | Out-Null
}

# Fallback for a miner that became detached from the backend process tree: only
# stop executables located in MinerDesk's managed ProgramData miner directory.
$root = [System.IO.Path]::GetFullPath($minersRoot).TrimEnd('\') + '\'
Get-CimInstance Win32_Process -ErrorAction SilentlyContinue |
  Where-Object {
    $_.ExecutablePath -and
    $_.ExecutablePath.StartsWith($root, [System.StringComparison]::OrdinalIgnoreCase)
  } |
  ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }

$deadline = (Get-Date).AddSeconds(10)
do {
  $running = @(Get-InstalledHeadlessProcess)
  if ($running.Count -eq 0) { break }
  Start-Sleep -Milliseconds 250
} while ((Get-Date) -lt $deadline)

if ($running.Count -gt 0) {
  throw "The backend executable is still running; the Scheduled Task was not removed."
}

Unregister-ScheduledTask -TaskName $taskName -Confirm:$false

$deadline = (Get-Date).AddSeconds(3)
do {
  $task = Get-ScheduledTask -TaskName $taskName -ErrorAction SilentlyContinue
  if ($null -eq $task) { break }
  Start-Sleep -Milliseconds 200
} while ((Get-Date) -lt $deadline)

if ($null -ne $task) {
  throw "The Scheduled Task '$taskName' is still present."
}

Write-Host "Stopped backend/miners and removed: $taskName" -ForegroundColor Green
