#requires -Version 5.1
# Runs the exact std-only production worker. No Windows settings/processes touched.
[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$rustc = (Get-Command rustc.exe -ErrorAction Stop).Source
$directory = Join-Path ([IO.Path]::GetTempPath()) ('minerdesk-config-tests-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $directory | Out-Null
try {
    $testExe = Join-Path $directory 'config-sync-tests.exe'
    & $rustc '--edition=2021' '--test' (Join-Path $root 'src-tauri\src\config_sync.rs') '-o' $testExe
    if ($LASTEXITCODE -ne 0) { throw 'Config synchronization worker could not be compiled.' }
    & $testExe
    if ($LASTEXITCODE -ne 0) { throw 'Config synchronization concurrency regression failed.' }
    Write-Host 'Config synchronization concurrency tests passed.' -ForegroundColor Green
} finally {
    Remove-Item -LiteralPath $directory -Recurse -Force -ErrorAction SilentlyContinue
}
