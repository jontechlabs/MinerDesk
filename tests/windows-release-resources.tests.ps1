#requires -Version 5.1
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
$repository = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
. (Join-Path $repository 'scripts\windows-release-resources.ps1')
$fixture = Join-Path ([IO.Path]::GetTempPath()) ('minerdesk-release-stage-fixture-' + [guid]::NewGuid().ToString('N'))
try {
    $resourcesDirectory = Join-Path $fixture 'src-tauri\resources'
    $unusedTarget = Join-Path $fixture 'src-tauri\target\release'
    $first = Join-Path $fixture 'first'
    $second = Join-Path $fixture 'second'
    foreach ($directory in @($resourcesDirectory, $unusedTarget, $first, $second)) {
        New-Item -ItemType Directory -Path $directory -Force | Out-Null
    }
    Copy-Item -LiteralPath (Join-Path $repository 'src-tauri\tauri.windows.conf.json') -Destination (Join-Path $fixture 'src-tauri\tauri.windows.conf.json')
    $resources = @(Get-MdWindowsReleaseResources -Root $fixture)
    if ($resources.Count -ne 5) { throw 'Expected the complete current Windows resource contract.' }
    foreach ($path in $resources) {
        $name = [IO.Path]::GetFileName($path)
        [IO.File]::WriteAllText($path, "Packaged fixture: $name")
        [IO.File]::WriteAllText((Join-Path $unusedTarget $name), "Initial target fixture: $name")
    }
    $desktop = Join-Path $fixture 'desktop.exe'
    $setup = Join-Path $fixture 'MinerDesk_0.8.2_x64-setup.exe'
    [IO.File]::WriteAllText($desktop, 'Desktop fixture only; never executed.')
    [IO.File]::WriteAllText($setup, 'Installer fixture only; never executed.')
    Copy-MdWindowsReleaseFiles -Desktop $desktop -Setup $setup -Resources $resources -Destination $first
    # Reproduce Tauri's later rebuild without touching the files it embedded.
    foreach ($path in $resources) {
        $name = [IO.Path]::GetFileName($path)
        [IO.File]::WriteAllText((Join-Path $unusedTarget $name), "Rebuilt target fixture with different features: $name")
    }
    Copy-MdWindowsReleaseFiles -Desktop $desktop -Setup $setup -Resources $resources -Destination $second
    foreach ($destination in @($first, $second)) {
        if (@(Get-ChildItem -LiteralPath $destination -File).Count -ne ($resources.Count + 2)) { throw 'Incomplete release staging.' }
        foreach ($path in $resources) {
            $published = Join-Path $destination ([IO.Path]::GetFileName($path))
            if ((Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash -ne (Get-FileHash -LiteralPath $published -Algorithm SHA256).Hash) {
                throw "Published resource does not match packaged bytes: $published"
            }
        }
    }
    Write-Host 'Windows release staging fixture passed: every resource keeps packaged bytes after unused target rebuilds.'
} finally {
    $resolved = [IO.Path]::GetFullPath($fixture)
    $temporary = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\')
    if ([IO.Path]::GetDirectoryName($resolved) -ne $temporary -or -not [IO.Path]::GetFileName($resolved).StartsWith('minerdesk-release-stage-fixture-')) {
        throw 'Refusing to remove a fixture outside the temporary directory.'
    }
    if (Test-Path -LiteralPath $resolved) { Remove-Item -LiteralPath $resolved -Recurse -Force }
}
