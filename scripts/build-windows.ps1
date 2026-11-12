#requires -Version 5.1
[CmdletBinding()]
param([switch]$RequireSignature)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
. (Join-Path $PSScriptRoot 'windows-release-resources.ps1')

function Invoke-MdBuildNative {
    param([string]$File, [string[]]$Arguments)
    & $File @Arguments
    $nativeExit = $LASTEXITCODE
    if ($nativeExit -ne 0) {
        throw "Build command failed (exit $nativeExit): $File $($Arguments -join ' ')"
    }
}

function Assert-MdBuiltExe {
    param([string]$Path)
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { throw "Expected output missing: $Path" }
    if ((Get-Item -LiteralPath $Path).Length -lt 65536) { throw "Output is too small to be a valid distribution binary: $Path" }
    $stream = [IO.File]::OpenRead($Path)
    try {
        if ($stream.ReadByte() -ne 77 -or $stream.ReadByte() -ne 90) { throw "Output has no Windows executable header: $Path" }
    } finally { $stream.Dispose() }
}

# Check the actual PE headers, not just the source attributes. A console
# subsystem (3) here would recreate the unwanted terminal after installation.
function Assert-MdExeSubsystem {
    param([string]$Path, [int]$Expected)
    $stream = [IO.File]::OpenRead($Path)
    $reader = [IO.BinaryReader]::new($stream)
    try {
        $stream.Position = 0x3c
        $pe = $reader.ReadInt32()
        $stream.Position = $pe
        if ($reader.ReadUInt32() -ne 0x4550) { throw "Invalid PE header: $Path" }
        $stream.Position = $pe + 24 + 68
        $subsystem = $reader.ReadUInt16()
        if ($subsystem -ne $Expected) { throw "Wrong PE subsystem in $Path : $subsystem, expected $Expected" }
    } finally { $reader.Dispose(); $stream.Dispose() }
}

$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$stage = $null
$signingConfig = $null
Push-Location $root
try {
    $package = Get-Content -LiteralPath 'package.json' -Raw | ConvertFrom-Json
    $tauri = Get-Content -LiteralPath 'src-tauri\tauri.conf.json' -Raw | ConvertFrom-Json
    $version = [string]$package.version
    $cargoText = Get-Content -LiteralPath 'src-tauri\Cargo.toml' -Raw
    $cargoVersion = [regex]::Match($cargoText, '(?m)^version\s*=\s*"([^"]+)"').Groups[1].Value
    if ($version -ne $tauri.version -or $version -ne $cargoVersion) { throw 'package.json, Cargo.toml and tauri.conf.json versions disagree.' }
    Write-Host "Building MinerDesk $version from: $root" -ForegroundColor Cyan
    if ($RequireSignature) {
        if (-not $env:MD_SIGNING_PUBLISHER -or -not $env:SIGNPATH_API_TOKEN) { throw 'Signing is required, but the provider is not configured.' }
        . (Join-Path $PSScriptRoot 'windows-signing-common.ps1')
        $env:MD_SIGNING_VERSION = $version
        New-Item -ItemType Directory -Path (Join-Path $root 'publish') -Force | Out-Null
        $env:MD_SIGNING_LOG = Join-Path $root 'publish\signing-receipts.jsonl'
        [IO.File]::WriteAllText($env:MD_SIGNING_LOG, '')
        $signingConfig = Join-Path $root ('publish\signing-' + [guid]::NewGuid().ToString('N') + '.json')
        # Object notation preserves paths with spaces; %1 is replaced by Tauri.
        @{ bundle = @{ windows = @{ signCommand = @{
            cmd = 'powershell.exe'
            args = @('-NoLogo', '-NoProfile', '-File', (Join-Path $PSScriptRoot 'sign-windows.ps1'), '-Path', '%1')
        } } } } | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $signingConfig -Encoding UTF8
    }

    # Parse with the ACTUAL Windows PowerShell parser before expensive compilation.
    # Regression tests use synthetic processes; nothing running is stopped.
    & (Join-Path $root 'tests\installer-safety.tests.ps1')
    & (Join-Path $root 'tests\backend-task-settings.tests.ps1')
    & (Join-Path $root 'tests\scheduler-control.tests.ps1')
    & (Join-Path $root 'tests\config-sync.tests.ps1')
    & (Join-Path $root 'tests\windowless-backend.tests.ps1')
    & (Join-Path $root 'tests\windows-signing.tests.ps1')
    & (Join-Path $root 'tests\windows-release-resources.tests.ps1')

    $npm = (Get-Command npm.cmd -ErrorAction Stop).Source
    $npx = (Get-Command npx.cmd -ErrorAction Stop).Source
    $cargo = (Get-Command cargo.exe -ErrorAction Stop).Source
    if (-not (Test-Path -LiteralPath 'node_modules')) {
        Invoke-MdBuildNative -File $npm -Arguments @('ci', '--include=dev')
    }
    Invoke-MdBuildNative -File $npm -Arguments @('run', 'test:mining-controls')
    Write-Host 'Building the shared React frontend...' -ForegroundColor Cyan
    Invoke-MdBuildNative -File $npm -Arguments @('run', 'build')

    # Cargo's Tauri build hook resolves Windows resources before these binaries exist.
    # Bootstrap files stay ignored and are replaced by verified binaries before packaging.
    Invoke-MdBuildNative -File (Get-Command node.exe).Source -Arguments @('scripts/prepare-windows-resources.mjs')

    $targetDir = Join-Path $root 'src-tauri\target'
    if ($env:CARGO_TARGET_DIR) { $targetDir = [IO.Path]::GetFullPath($env:CARGO_TARGET_DIR) }
    $releaseDir = Join-Path $targetDir 'release'
    $headless = Join-Path $releaseDir 'minerdesk-headless.exe'
    $resource = Join-Path $root 'src-tauri\resources\minerdesk-headless.exe'
    Write-Host 'Building the console CLI and windowless backend...' -ForegroundColor Cyan
    Invoke-MdBuildNative -File $cargo -Arguments @('build', '--release', '--manifest-path', 'src-tauri\Cargo.toml', '--bin', 'minerdesk-headless', '--bin', 'minerdesk-backend')
    Assert-MdBuiltExe $headless
    Assert-MdExeSubsystem -Path $headless -Expected 3
    $backend = Join-Path $releaseDir 'minerdesk-backend.exe'
    Assert-MdBuiltExe $backend
    Assert-MdExeSubsystem -Path $backend -Expected 2
    Write-Host 'Building the independent update worker...' -ForegroundColor Cyan
    Invoke-MdBuildNative -File $cargo -Arguments @('build', '--locked', '--release', '--manifest-path', 'src-tauri\update-helper\Cargo.toml')
    $helperTarget = Join-Path $root 'src-tauri\update-helper\target'
    if ($env:CARGO_TARGET_DIR) { $helperTarget = [IO.Path]::GetFullPath($env:CARGO_TARGET_DIR) }
    $updateHelper = Join-Path $helperTarget 'release\minerdesk-update-helper.exe'
    Assert-MdBuiltExe $updateHelper
    Assert-MdExeSubsystem -Path $updateHelper -Expected 3
    $maintenance = Join-Path $root 'src-tauri\windows\maintenance.ps1'
    $maintenanceCommon = Join-Path $root 'src-tauri\windows\maintenance-common.ps1'
    if ($RequireSignature) {
        $env:MD_NSIS_PLUGIN_ROOT = Join-Path $releaseDir 'nsis\x64\Plugins\x86-unicode'
        # Sign resources and installer helpers BEFORE embedding them in Setup.
        & (Join-Path $PSScriptRoot 'sign-windows.ps1') -Path @($headless, $backend, $updateHelper, $maintenance, $maintenanceCommon)
    }
    Copy-Item -LiteralPath $headless -Destination $resource -Force
    Copy-Item -LiteralPath $backend -Destination (Join-Path $root 'src-tauri\resources\minerdesk-backend.exe') -Force
    # Copies are taken after signing: bundled resources have the verified bytes.
    foreach ($file in @($updateHelper, $maintenance, $maintenanceCommon)) {
        Copy-Item -LiteralPath $file -Destination (Join-Path $root ('src-tauri\resources\' + [IO.Path]::GetFileName($file))) -Force
    }
    # Tauri rebuilds secondary binaries with its desktop features. The packaged
    # resources remain the independently built and (when required) signed files.
    $packagedResources = @(Get-MdWindowsReleaseResources -Root $root)

    $expectedName = "MinerDesk_${version}_x64-setup.exe"
    $setup = Join-Path $releaseDir "bundle\nsis\$expectedName"
    # An older file with the same name must not masquerade as this build's output.
    if (Test-Path -LiteralPath $setup) { Remove-Item -LiteralPath $setup -Force }
    Write-Host 'Building Desktop and NSIS Setup...' -ForegroundColor Cyan
    $tauriArgs = @('tauri', 'build', '--bundles', 'nsis')
    if ($RequireSignature) { $tauriArgs += @('--config', $signingConfig) }
    Invoke-MdBuildNative -File $npx -Arguments $tauriArgs
    $desktop = Join-Path $releaseDir 'minerdesk.exe'
    Assert-MdBuiltExe $desktop
    Assert-MdBuiltExe $setup
    $signatures = @()
    if ($RequireSignature) {
        foreach ($file in (@($desktop, $setup) + $packagedResources)) {
            $signatures += Get-MdVerifiedSignature -Path $file -ExpectedPublisher $env:MD_SIGNING_PUBLISHER
        }
        $receipts = @(Get-Content -LiteralPath $env:MD_SIGNING_LOG | ForEach-Object { $_ | ConvertFrom-Json })
        foreach ($role in @('desktop', 'cli', 'backend', 'update-helper', 'maintenance', 'maintenance-common', 'installer', 'nsis-uninstaller')) {
            if (-not ($receipts | Where-Object { $_.role -eq $role })) { throw "Missing verified signing receipt: $role" }
        }
    }

    # Publish only after EVERY step and the expected versioned Setup have passed.
    $publish = Join-Path $root 'publish\windows-x64'
    $stage = Join-Path $root ('publish\.staging-' + [guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $stage -Force | Out-Null
    Copy-MdWindowsReleaseFiles -Desktop $desktop -Setup $setup -Resources $packagedResources -Destination $stage
    $hashes = @{}
    foreach ($file in Get-ChildItem -LiteralPath $stage -File) {
        $hashes[$file.Name] = (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash
    }
    $signingStatus = 'unsigned'
    if ($RequireSignature) {
        $signingStatus = 'authenticode-verified'
        Copy-Item -LiteralPath $env:MD_SIGNING_LOG -Destination (Join-Path $stage 'signing-receipts.jsonl')
    }
    [ordered]@{ version = $version; built_utc = (Get-Date).ToUniversalTime().ToString('o'); source = $root; sha256 = $hashes; signing = $signingStatus; signatures = $signatures } |
        ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $stage 'build-info.json') -Encoding UTF8
    New-Item -ItemType Directory -Path $publish -Force | Out-Null
    # Only obsolete generated Setup files are removed; other user files remain.
    Get-ChildItem -LiteralPath $publish -Filter 'MinerDesk_*_x64-setup.exe' | Remove-Item -Force
    Get-ChildItem -LiteralPath $stage | Copy-Item -Destination $publish -Force
    Write-Host "BUILD SUCCEEDED: MinerDesk $version" -ForegroundColor Green
    Write-Host ('Run this NEW installer: ' + (Join-Path $publish $expectedName)) -ForegroundColor Green
} catch {
    Write-Host 'BUILD FAILED. Do not use an older Setup file as the result of this build.' -ForegroundColor Red
    throw
} finally {
    if ($signingConfig -and (Test-Path -LiteralPath $signingConfig)) { Remove-Item -LiteralPath $signingConfig -Force }
    if ($stage -and (Test-Path -LiteralPath $stage)) {
        $resolvedStage = [IO.Path]::GetFullPath($stage)
        $expectedParent = [IO.Path]::GetFullPath((Join-Path $root 'publish'))
        if ([IO.Path]::GetDirectoryName($resolvedStage) -ne $expectedParent -or -not [IO.Path]::GetFileName($resolvedStage).StartsWith('.staging-')) {
            throw 'Refusing to remove a staging directory outside this build workspace.'
        }
        Remove-Item -LiteralPath $resolvedStage -Recurse -Force
    }
    Pop-Location
}
