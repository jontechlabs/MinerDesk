#requires -Version 5.1
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot '..\scripts\windows-signing-common.ps1')
$publisher = 'SignPath Foundation'
$certificate = [pscustomobject]@{ Subject = 'CN=SignPath Foundation'; Thumbprint = 'TEST' }
$certificate | Add-Member ScriptMethod GetNameInfo { param($kind, $issuer); return 'SignPath Foundation' }
$valid = [pscustomobject]@{ Status = 'Valid'; SignerCertificate = $certificate; TimeStamperCertificate = $certificate }
Assert-MdSignatureResult -Signature $valid -ExpectedPublisher $publisher -Path 'synthetic.exe'
$passed = 1
foreach ($status in @('NotSigned', 'HashMismatch', 'NotTrusted', 'UnknownError', 'NotSupportedFileFormat')) {
    $bad = [pscustomobject]@{ Status = $status; SignerCertificate = $certificate; TimeStamperCertificate = $certificate }
    $rejected = $false
    try { Assert-MdSignatureResult -Signature $bad -ExpectedPublisher $publisher -Path 'synthetic.exe' } catch { $rejected = $true }
    if (-not $rejected) { throw "Accepted invalid signature status: $status" }
    $passed++
}
foreach ($case in @('timestamp', 'publisher', 'configuration', 'certificate')) {
    $candidate = [pscustomobject]@{ Status = 'Valid'; SignerCertificate = $certificate; TimeStamperCertificate = $certificate }
    $expected = $publisher
    switch ($case) {
        'timestamp' { $candidate.TimeStamperCertificate = $null }
        'publisher' { $expected = 'Wrong publisher' }
        'configuration' { $expected = '' }
        'certificate' { $candidate.SignerCertificate = $null }
    }
    $rejected = $false
    try { Assert-MdSignatureResult -Signature $candidate -ExpectedPublisher $expected -Path 'synthetic.exe' } catch { $rejected = $true }
    if (-not $rejected) { throw "Accepted invalid signing $case" }
    $passed++
}
# Verify a real unsigned file through the operating system's Authenticode API.
$fixture = Join-Path ([IO.Path]::GetTempPath()) ('md-unsigned-' + [guid]::NewGuid().ToString('N') + '.ps1')
try {
    [IO.File]::WriteAllText($fixture, '# unsigned test fixture')
    $rejected = $false
    try { Get-MdVerifiedSignature -Path $fixture -ExpectedPublisher $publisher | Out-Null } catch { $rejected = $true }
    if (-not $rejected) { throw 'Accepted a real unsigned file.' }
    $passed++
} finally { Remove-Item -LiteralPath $fixture -Force }
Write-Host "Windows signing checks passed: $passed assertions. No trust store or certificate was modified."
