#requires -Version 5.1
Set-StrictMode -Version 2.0

function Get-MdWindowsReleaseResources {
    param([string]$Root)
    $config = Get-Content -LiteralPath (Join-Path $Root 'src-tauri\tauri.windows.conf.json') -Raw | ConvertFrom-Json
    foreach ($resource in $config.bundle.resources) {
        Join-Path $Root ('src-tauri\' + $resource)
    }
}

function Copy-MdWindowsReleaseFiles {
    param([string]$Desktop, [string]$Setup, [string[]]$Resources, [string]$Destination)
    Copy-Item -LiteralPath $Desktop -Destination (Join-Path $Destination 'MinerDesk.exe')
    foreach ($resource in $Resources) {
        Copy-Item -LiteralPath $resource -Destination (Join-Path $Destination ([IO.Path]::GetFileName($resource)))
    }
    Copy-Item -LiteralPath $Setup -Destination (Join-Path $Destination ([IO.Path]::GetFileName($Setup)))
}
