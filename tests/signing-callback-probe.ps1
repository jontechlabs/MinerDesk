#requires -Version 5.1
# Test-only Tauri callback: records packaging targets and DOES NOT sign files.
[CmdletBinding()]
param([Parameter(Mandatory = $true)][string]$Path)
$ErrorActionPreference = 'Stop'
if (-not $env:MD_SIGNING_PROBE_LOG) { throw 'This callback may only run inside the signing integration probe.' }
. (Join-Path $PSScriptRoot '..\scripts\windows-signing-common.ps1')
$file = (Resolve-Path -LiteralPath $Path).Path
$extension = [IO.Path]::GetExtension($file)
if ($extension -ieq '.dll') { Assert-MdUpstreamNsisPlugin -Path $file }
else {
    $metadata = [Diagnostics.FileVersionInfo]::GetVersionInfo($file)
    if ($metadata.ProductName -cne 'MinerDesk' -or $metadata.ProductVersion -cne $env:MD_SIGNING_VERSION) { throw "Unexpected callback product metadata: $file" }
}
@{ name = [IO.Path]::GetFileName($file); extension = $extension } | ConvertTo-Json -Compress | Add-Content -LiteralPath $env:MD_SIGNING_PROBE_LOG -Encoding UTF8
Write-Host "TEST ONLY: recorded Tauri signing callback for $([IO.Path]::GetFileName($file)); output remains unsigned."
