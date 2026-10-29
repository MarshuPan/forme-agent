[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$PackagePath,
    [Parameter(Mandatory = $true)][ValidatePattern('^[0-9A-Fa-f]{40}$')][string]$CertificateThumbprint,
    [Parameter(Mandatory = $false)][ValidateSet('CurrentUser', 'LocalMachine')][string]$StoreLocation = 'CurrentUser'
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

if (-not [System.IO.Path]::IsPathRooted($PackagePath)) {
    throw 'Package path must be absolute'
}

$thumbprint = ($CertificateThumbprint -replace '[\s-]', '').ToUpperInvariant()
$null = Get-ChildItem -LiteralPath "Cert:\$StoreLocation\My" -ErrorAction Stop
$certificate = Get-Item -LiteralPath "Cert:\$StoreLocation\My\$thumbprint" -ErrorAction Stop
if (-not $certificate.HasPrivateKey) {
    throw 'The selected certificate has no private key'
}

$signed = Set-AuthenticodeSignature -LiteralPath $PackagePath -Certificate $certificate -HashAlgorithm SHA256 -ErrorAction Stop
$observed = Get-AuthenticodeSignature -LiteralPath $PackagePath -ErrorAction Stop
$signer = $observed.SignerCertificate
$signerThumbprint = if ($null -eq $signer) { '' } else { [string]$signer.Thumbprint }

[ordered]@{
    signature_type = [string]$observed.SignatureType
    signature_status = [string]$observed.Status
    status_message = [string]$observed.StatusMessage
    signer_thumbprint = $signerThumbprint
    certificate_subject = if ($null -eq $signer) { '' } else { [string]$signer.Subject }
    certificate_issuer = if ($null -eq $signer) { '' } else { [string]$signer.Issuer }
    signing_status = [string]$signed.Status
} | ConvertTo-Json -Compress
