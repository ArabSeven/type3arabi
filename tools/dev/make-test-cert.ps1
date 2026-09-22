# Creates a LOCAL test code-signing certificate for development builds (docs/07 §4).
# NEVER use for public releases. Run in an elevated PowerShell.
param([string]$Subject = "CN=Type3arabi Dev Test")
$cert = New-SelfSignedCertificate -Type CodeSigningCert -Subject $Subject `
    -CertStoreLocation Cert:\CurrentUser\My -KeyUsage DigitalSignature -KeyExportPolicy Exportable `
    -NotAfter (Get-Date).AddYears(2)
$tmp = Join-Path $env:TEMP "t3a-dev.cer"
Export-Certificate -Cert $cert -FilePath $tmp | Out-Null
Import-Certificate -FilePath $tmp -CertStoreLocation Cert:\LocalMachine\Root | Out-Null
Import-Certificate -FilePath $tmp -CertStoreLocation Cert:\LocalMachine\TrustedPublisher | Out-Null
Write-Host "Thumbprint: $($cert.Thumbprint)"
Write-Host "Sign:  signtool sign /sha1 $($cert.Thumbprint) /fd sha256 /tr http://timestamp.digicert.com /td sha256 <file>"
