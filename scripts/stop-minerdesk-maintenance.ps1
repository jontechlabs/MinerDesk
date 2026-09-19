#requires -Version 5.1
[CmdletBinding(SupportsShouldProcess = $true, ConfirmImpact = 'High')]
param([switch]$RemoveTask, [string]$InstallDir = (Join-Path $env:ProgramFiles 'MinerDesk'))
$ErrorActionPreference = 'Stop'
if ($PSCmdlet.ShouldProcess($InstallDir, 'Stop MinerDesk and its managed miners')) {
    $action = if ($RemoveTask) { 'PrepareInstall' } else { 'Stop' }
    $helper = Join-Path $PSScriptRoot '..\src-tauri\windows\maintenance.ps1'
    & "$PSHOME\powershell.exe" -NoLogo -NoProfile -ExecutionPolicy Bypass -File $helper -Action $action -InstallDir $InstallDir -Force
    if ($LASTEXITCODE -ne 0) { throw "MinerDesk maintenance failed (exit $LASTEXITCODE). See the printed log path." }
}
