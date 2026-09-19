$ErrorActionPreference = 'SilentlyContinue'

$installDir = Join-Path $env:ProgramFiles 'MinerDesk'
$desktop = Join-Path $installDir 'MinerDesk.exe'
$headless = Join-Path $installDir 'minerdesk-headless.exe'
$backend = Join-Path $installDir 'minerdesk-backend.exe'
$minersRoot = ([System.IO.Path]::GetFullPath((Join-Path $env:ProgramData 'MinerDesk\miners'))).TrimEnd('\') + '\'

Write-Host "Expected installed Desktop: $desktop" -ForegroundColor Cyan
Write-Host "Expected windowless backend: $backend" -ForegroundColor Cyan
Write-Host "Optional console headless: $headless" -ForegroundColor Cyan
Write-Host "Managed miners root:       $minersRoot" -ForegroundColor Cyan
Write-Host ''

Get-CimInstance Win32_Process |
  Where-Object {
    $_.Name -like 'MinerDesk*' -or
    $_.Name -eq 'minerdesk-headless.exe' -or
    $_.Name -eq 'minerdesk-backend.exe' -or
    ($_.ExecutablePath -and ($_.ExecutablePath -like '*MinerDesk*' -or $_.ExecutablePath.StartsWith($minersRoot,[System.StringComparison]::OrdinalIgnoreCase)))
  } |
  Select-Object ProcessId, ParentProcessId, Name, ExecutablePath, CommandLine |
  Format-List

Write-Host 'Scheduled task:' -ForegroundColor Cyan
Get-ScheduledTask -TaskName 'MinerDesk Privileged Backend' -ErrorAction SilentlyContinue |
  Select-Object TaskName, State, Actions |
  Format-List
