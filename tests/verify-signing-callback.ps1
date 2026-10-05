#requires -Version 5.1
$ErrorActionPreference = 'Stop'
$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$config = Join-Path $root 'publish\signing-probe-config.json'
$env:MD_SIGNING_PROBE_LOG = Join-Path $root 'publish\signing-probe.jsonl'
$env:MD_SIGNING_VERSION = (Get-Content (Join-Path $root 'package.json') -Raw | ConvertFrom-Json).version
$env:MD_NSIS_PLUGIN_ROOT = Join-Path $root 'src-tauri\target\release\nsis\x64\Plugins\x86-unicode'
[IO.File]::WriteAllText($env:MD_SIGNING_PROBE_LOG, '')
try {
    @{ bundle = @{ windows = @{ signCommand = @{
        cmd = 'powershell.exe'; args = @('-NoProfile', '-File', (Join-Path $PSScriptRoot 'signing-callback-probe.ps1'), '-Path', '%1')
    } } } } | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $config -Encoding UTF8
    & npx.cmd tauri bundle --bundles nsis --config $config
    if ($LASTEXITCODE -ne 0) { throw 'Tauri custom signing callback integration probe failed.' }
    $records = @(Get-Content -LiteralPath $env:MD_SIGNING_PROBE_LOG | ForEach-Object { $_ | ConvertFrom-Json })
    foreach ($name in @('minerdesk.exe', 'minerdesk-headless.exe', 'minerdesk-backend.exe', "MinerDesk_$($env:MD_SIGNING_VERSION)_x64-setup.exe", 'NSISdl.dll', 'StartMenu.dll', 'System.dll', 'nsDialogs.dll', 'nsis_tauri_utils.dll')) {
        if (-not ($records | Where-Object { $_.name -ieq $name })) { throw "Missing Tauri callback: $name" }
    }
    $uninstaller = @($records | Where-Object { $_.extension -ine '.dll' -and $_.name -inotlike 'minerdesk*' })
    if ($uninstaller.Count -ne 1) { throw 'Expected exactly one temporary NSIS uninstaller signing callback.' }
    if ($uninstaller[0].extension -ine '.exe') { throw "Uninstaller callback extension needs handling: $($uninstaller[0].extension)" }
    Write-Host 'PASS: real Tauri desktop/resources/installer/uninstaller callbacks and upstream plugin handling. Outputs are unsigned test fixtures.'
} finally {
    if (Test-Path -LiteralPath $config) { Remove-Item -LiteralPath $config -Force }
    $env:MD_SIGNING_PROBE_LOG = $null
}
