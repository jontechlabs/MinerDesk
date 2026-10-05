#requires -Version 5.1
[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'windows-signing-common.ps1')
$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$version = (Get-Content -LiteralPath (Join-Path $root 'package.json') -Raw | ConvertFrom-Json).version
$build = Join-Path $root 'publish\windows-x64'
$info = Get-Content -LiteralPath (Join-Path $build 'build-info.json') -Raw | ConvertFrom-Json
if ($info.signing -ne 'authenticode-verified' -or $info.version -ne $version) { throw 'Only a verified signed build of the current version can be packaged.' }
$files = @('MinerDesk.exe', 'minerdesk-headless.exe', 'minerdesk-backend.exe', "MinerDesk_${version}_x64-setup.exe")
foreach ($name in $files) {
    $file = Join-Path $build $name
    Get-MdVerifiedSignature -Path $file -ExpectedPublisher $env:MD_SIGNING_PUBLISHER | Out-Null
    if ((Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash -ne $info.sha256.$name) { throw "Signed build hash changed: $name" }
}
$out = Join-Path $root 'publish\signed-release'
if (Test-Path -LiteralPath $out) { throw 'Signed release directory already exists. Use a fresh checkout.' }
New-Item -ItemType Directory -Path $out | Out-Null
$portable = @($files[0..2] | ForEach-Object { Join-Path $build $_ })
Compress-Archive -LiteralPath $portable -DestinationPath (Join-Path $out "MinerDesk_${version}_windows-x64.zip")
Copy-Item -LiteralPath (Join-Path $build $files[3]) -Destination $out
Copy-Item -LiteralPath (Join-Path $build 'build-info.json') -Destination (Join-Path $out 'WINDOWS-BUILD-INFO.json')
Copy-Item -LiteralPath (Join-Path $build 'signing-receipts.jsonl') -Destination (Join-Path $out 'SIGNING-RECEIPTS.jsonl')
$checksums = @(Get-ChildItem -LiteralPath $out -File | Sort-Object Name | ForEach-Object {
    (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant() + '  ' + $_.Name
})
[IO.File]::WriteAllLines((Join-Path $out 'WINDOWS-SHA256SUMS.txt'), $checksums, [Text.UTF8Encoding]::new($false))
