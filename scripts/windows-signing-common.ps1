#requires -Version 5.1
Set-StrictMode -Version 2.0

function Assert-MdUpstreamNsisPlugin {
    param([string]$Path)
    $pluginRoot = [IO.Path]::GetFullPath($env:MD_NSIS_PLUGIN_ROOT).TrimEnd('\') + '\'
    if (-not $Path.StartsWith($pluginRoot, [StringComparison]::OrdinalIgnoreCase)) { throw 'Unexpected upstream DLL signing request.' }
    $relative = $Path.Substring($pluginRoot.Length)
    if ($relative -notin @('NSISdl.dll', 'StartMenu.dll', 'System.dll', 'nsDialogs.dll', 'additional\nsis_tauri_utils.dll')) {
        throw 'Unrecognized upstream NSIS plugin.'
    }
    $original = Join-Path $env:LOCALAPPDATA ('tauri\NSIS\Plugins\x86-unicode\' + $relative)
    if ((Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash -ne (Get-FileHash -LiteralPath $original -Algorithm SHA256).Hash) {
        throw 'NSIS plugin copy does not match the original toolchain cache.'
    }
    Write-Host "Retaining upstream signature state: $relative (not signed as MinerDesk)."
}

function Assert-MdSignatureResult {
    param($Signature, [string]$ExpectedPublisher, [string]$Path)
    if ([string]::IsNullOrWhiteSpace($ExpectedPublisher)) { throw 'The expected signing publisher must be configured.' }
    if ([string]$Signature.Status -ne 'Valid') { throw "Untrusted or missing Authenticode signature in $Path : $($Signature.Status)" }
    if (-not $Signature.SignerCertificate) { throw "No signing certificate in $Path" }
    $publisher = $Signature.SignerCertificate.GetNameInfo([Security.Cryptography.X509Certificates.X509NameType]::SimpleName, $false)
    if ($publisher -cne $ExpectedPublisher) { throw "Unexpected signing publisher in $Path : $publisher" }
    if (-not $Signature.TimeStamperCertificate) { throw "No trusted timestamp in $Path" }
}

function Get-MdVerifiedSignature {
    param([string]$Path, [string]$ExpectedPublisher)
    $signature = Get-AuthenticodeSignature -LiteralPath $Path -ErrorAction Stop
    Assert-MdSignatureResult -Signature $signature -ExpectedPublisher $ExpectedPublisher -Path $Path
    [ordered]@{
        file = [IO.Path]::GetFileName($Path)
        status = [string]$signature.Status
        publisher = $ExpectedPublisher
        subject = $signature.SignerCertificate.Subject
        thumbprint = $signature.SignerCertificate.Thumbprint
        timestamp_subject = $signature.TimeStamperCertificate.Subject
        sha256 = (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash
    }
}
