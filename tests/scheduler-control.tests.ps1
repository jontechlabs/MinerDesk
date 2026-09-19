#requires -Version 5.1
# Executes the exact production scheduling policy, not a translated model.
# Pure tests: no GPU miner, real scheduled task, registry entry or GUI is used.
[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$rustc = (Get-Command rustc.exe -ErrorAction Stop).Source
$testDir = Join-Path ([IO.Path]::GetTempPath()) ('minerdesk-policy-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $testDir | Out-Null
try {
    $testExe = Join-Path $testDir 'scheduler-policy-tests.exe'
    & $rustc '--edition=2021' '--test' (Join-Path $root 'src-tauri\src\schedule_control.rs') '-o' $testExe
    if ($LASTEXITCODE -ne 0) { throw 'The production scheduler policy could not be compiled for regression tests.' }
    & $testExe
    if ($LASTEXITCODE -ne 0) { throw 'Scheduler Stop/Start policy regression failed.' }
    Write-Host 'Scheduler policy preflight passed (actual Rust policy module).' -ForegroundColor Green
} finally {
    Remove-Item -LiteralPath $testDir -Recurse -Force -ErrorAction SilentlyContinue
}
