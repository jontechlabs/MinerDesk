#requires -Version 5.1
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string[]]$Path,
    [Parameter(Mandatory = $true)][string]$ExpectedPublisher
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'windows-signing-common.ps1')
foreach ($file in $Path) {
    Get-MdVerifiedSignature -Path $file -ExpectedPublisher $ExpectedPublisher | ConvertTo-Json -Compress | Write-Output
}
