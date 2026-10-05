#requires -Version 5.1
[CmdletBinding()]
param([Parameter(Mandatory = $true)][string[]]$Path)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
. (Join-Path $PSScriptRoot 'windows-signing-common.ps1')

# The SignPath client uploads only these explicitly supplied files, through the
# official pinned GitHub actions. It never uploads a build directory or secrets.
if (-not $env:MD_SIGNING_PUBLISHER) { throw 'MD_SIGNING_PUBLISHER is required; unsigned fallback is forbidden.' }
$resolved = @($Path | ForEach-Object { (Resolve-Path -LiteralPath $_ -ErrorAction Stop).Path })
# Tauri also calls signCommand for upstream NSIS plugins. They are not MinerDesk
# source, so Foundation certificates must NOT be applied to them. Accept only
# the exact plugin copies Tauri took from its verified toolchain cache.
if ($resolved.Count -eq 1 -and [IO.Path]::GetExtension($resolved[0]) -ieq '.dll') {
    $pluginRoot = [IO.Path]::GetFullPath($env:MD_NSIS_PLUGIN_ROOT).TrimEnd('\') + '\'
    if (-not $resolved[0].StartsWith($pluginRoot, [StringComparison]::OrdinalIgnoreCase)) { throw 'Unexpected upstream DLL signing request.' }
    $relative = $resolved[0].Substring($pluginRoot.Length)
    if ($relative -notin @('NSISdl.dll', 'StartMenu.dll', 'System.dll', 'nsDialogs.dll', 'additional\nsis_tauri_utils.dll')) {
        throw 'Unrecognized upstream NSIS plugin.'
    }
    $original = Join-Path $env:LOCALAPPDATA ('tauri\NSIS\Plugins\x86-unicode\' + $relative)
    if ((Get-FileHash -LiteralPath $resolved[0] -Algorithm SHA256).Hash -ne (Get-FileHash -LiteralPath $original -Algorithm SHA256).Hash) {
        throw 'NSIS plugin copy does not match the original toolchain cache.'
    }
    Write-Host "Retaining upstream signature state: $relative (not signed as MinerDesk)."
    exit 0
}
$toSign = @()
foreach ($file in $resolved) {
    if ([IO.Path]::GetExtension($file) -ieq '.exe') {
        $metadata = [Diagnostics.FileVersionInfo]::GetVersionInfo($file)
        if ($metadata.ProductName -cne 'MinerDesk' -or $metadata.ProductVersion -cne $env:MD_SIGNING_VERSION) {
            throw "Unexpected product metadata in $file"
        }
    }
    $existing = Get-AuthenticodeSignature -LiteralPath $file
    try { Assert-MdSignatureResult -Signature $existing -ExpectedPublisher $env:MD_SIGNING_PUBLISHER -Path $file }
    catch { $toSign += $file }
}
$node = (Get-Command node.exe -ErrorAction Stop).Source
$request = Join-Path $env:RUNNER_TEMP ('md-sign-request-' + [guid]::NewGuid().ToString('N') + '.json')
try {
    if ($toSign.Count -gt 0) {
        ConvertTo-Json -InputObject $toSign | Set-Content -LiteralPath $request -Encoding UTF8
        & $node (Join-Path $PSScriptRoot 'signpath-sign.cjs') $request
        if ($LASTEXITCODE -ne 0) { throw "SignPath signing failed (exit $LASTEXITCODE)." }
    }
    foreach ($file in $resolved) {
        $record = Get-MdVerifiedSignature -Path $file -ExpectedPublisher $env:MD_SIGNING_PUBLISHER
        $record['role'] = switch ([IO.Path]::GetFileName($file)) {
            'minerdesk.exe' { 'desktop' }
            'minerdesk-headless.exe' { 'cli' }
            'minerdesk-backend.exe' { 'backend' }
            'maintenance.ps1' { 'maintenance' }
            'maintenance-common.ps1' { 'maintenance-common' }
            default {
                if ([IO.Path]::GetFileName($file) -like 'MinerDesk_*_x64-setup.exe') { 'installer' }
                elseif ([IO.Path]::GetExtension($file) -ieq '.exe') { 'nsis-uninstaller' }
                else { throw 'Unexpected first-party signing target.' }
            }
        }
        if (-not $env:MD_SIGNING_LOG) { throw 'MD_SIGNING_LOG is required.' }
        ConvertTo-Json -InputObject $record -Compress | Add-Content -LiteralPath $env:MD_SIGNING_LOG -Encoding UTF8
    }
} finally {
    if (Test-Path -LiteralPath $request) { Remove-Item -LiteralPath $request -Force }
}
