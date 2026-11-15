#Requires -Version 5.1
<#
.SYNOPSIS
  Generate a 3072-bit RSA release signing key and the embedded public-key list.

.DESCRIPTION
  Works on Windows PowerShell 5.1 as well as PowerShell 7:
  .NET Framework has no ExportPkcs8PrivateKeyPem, so the PKCS#1 PEM is encoded here
  by hand from RSAParameters. The signing script (tools/sign-feed.ps1) imports it
  via ImportFromPem, which accepts both PKCS#1 and PKCS#8 PEM.

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File tools/new-release-key.ps1 `
    -PrivateKeyPath D:\keys\update-signing.pem `
    -PublicKeysPath resources/update-trusted-keys.json `
    -KeyId generic-updater-2026-01
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$PrivateKeyPath,
    [Parameter(Mandatory)][string]$PublicKeysPath,
    [string]$KeyId = 'generic-updater-2026-01'
)

$ErrorActionPreference = 'Stop'
$PrivateKeyPath = [IO.Path]::GetFullPath($PrivateKeyPath)
$PublicKeysPath = [IO.Path]::GetFullPath($PublicKeysPath)
$repoPrefix = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..')).TrimEnd('\', '/') + [IO.Path]::DirectorySeparatorChar

if ($PrivateKeyPath.StartsWith($repoPrefix, [StringComparison]::OrdinalIgnoreCase)) {
    throw 'Private key must live outside the repository.'
}
if ($PrivateKeyPath.Equals($PublicKeysPath, [StringComparison]::OrdinalIgnoreCase)) {
    throw 'Private key and public key output must be different paths.'
}
if (Test-Path -LiteralPath $PrivateKeyPath) { throw 'Private key already exists; refusing to overwrite.' }
if (Test-Path -LiteralPath $PublicKeysPath) { throw 'Public key file already exists; refusing to overwrite.' }

$privateDirectory = Split-Path -Parent $PrivateKeyPath
if ((Test-Path -LiteralPath $privateDirectory) -and @(Get-ChildItem -LiteralPath $privateDirectory -Force).Count -gt 0) {
    throw 'Use an empty dedicated directory for the private key.'
}
$null = New-Item -ItemType Directory -Force -Path $privateDirectory

# Lock the key directory down to the current user.
$acl = New-Object Security.AccessControl.DirectorySecurity
$acl.SetAccessRuleProtection($true, $false)
$identity = [Security.Principal.WindowsIdentity]::GetCurrent().User
$rule = New-Object Security.AccessControl.FileSystemAccessRule(
    $identity, 'FullControl', 'ContainerInherit,ObjectInherit', 'None', 'Allow')
$acl.AddAccessRule($rule)
Set-Acl -LiteralPath $privateDirectory -AclObject $acl

function Get-DerLength([int]$length) {
    if ($length -lt 0x80) { return ,([byte[]]@($length)) }
    $buffer = New-Object System.Collections.Generic.List[byte]
    $value = $length
    while ($value -gt 0) {
        $buffer.Insert(0, [byte]($value -band 0xFF))
        $value = $value -shr 8
    }
    $result = New-Object System.Collections.Generic.List[byte]
    $result.Add([byte](0x80 -bor $buffer.Count))
    $result.AddRange($buffer)
    return ,([byte[]]$result.ToArray())
}

function Get-DerInteger([byte[]]$bytes) {
    $start = 0
    while ($start -lt $bytes.Length - 1 -and $bytes[$start] -eq 0) { $start++ }
    $content = New-Object System.Collections.Generic.List[byte]
    for ($i = $start; $i -lt $bytes.Length; $i++) { $content.Add($bytes[$i]) }
    if (($content[0] -band 0x80) -ne 0) { $content.Insert(0, [byte]0) }
    $length = Get-DerLength $content.Count
    $result = New-Object System.Collections.Generic.List[byte]
    $result.Add([byte]0x02)
    $result.AddRange([byte[]]$length)
    $result.AddRange($content)
    return ,([byte[]]$result.ToArray())
}

function Get-Pkcs1Pem([System.Security.Cryptography.RSAParameters]$parameters) {
    $body = New-Object System.Collections.Generic.List[byte]
    $body.AddRange([byte[]](Get-DerInteger ([byte[]]@(0))))            # version 0
    $body.AddRange([byte[]](Get-DerInteger $parameters.Modulus))
    $body.AddRange([byte[]](Get-DerInteger $parameters.Exponent))
    $body.AddRange([byte[]](Get-DerInteger $parameters.D))
    $body.AddRange([byte[]](Get-DerInteger $parameters.P))
    $body.AddRange([byte[]](Get-DerInteger $parameters.Q))
    $body.AddRange([byte[]](Get-DerInteger $parameters.DP))
    $body.AddRange([byte[]](Get-DerInteger $parameters.DQ))
    $body.AddRange([byte[]](Get-DerInteger $parameters.InverseQ))

    $sequence = New-Object System.Collections.Generic.List[byte]
    $sequence.Add([byte]0x30)
    $sequence.AddRange([byte[]](Get-DerLength $body.Count))
    $sequence.AddRange($body)

    $base64 = [Convert]::ToBase64String([byte[]]$sequence.ToArray())
    $lines = New-Object System.Collections.Generic.List[string]
    for ($offset = 0; $offset -lt $base64.Length; $offset += 64) {
        $lines.Add($base64.Substring($offset, [Math]::Min(64, $base64.Length - $offset)))
    }
    $pem = "-----BEGIN RSA PRIVATE KEY-----`n" + ($lines -join "`n") + "`n-----END RSA PRIVATE KEY-----`n"
    return $pem
}

$rsa = [Security.Cryptography.RSA]::Create(3072)
try {
    $privateParameters = $rsa.ExportParameters($true)
    $pem = Get-Pkcs1Pem $privateParameters
    [IO.File]::WriteAllText($PrivateKeyPath, $pem, (New-Object Text.UTF8Encoding($false)))

    $public = $rsa.ExportParameters($false)
    $keys = [ordered]@{
        keys = @([ordered]@{
            id      = $KeyId
            modulus = [Convert]::ToBase64String($public.Modulus)
            exponent = [Convert]::ToBase64String($public.Exponent)
        })
    }
    [IO.File]::WriteAllText($PublicKeysPath, (($keys | ConvertTo-Json -Depth 5) + "`n"), (New-Object Text.UTF8Encoding($false)))
} finally {
    $rsa.Dispose()
}

Write-Output "Created signing key '$KeyId'."
Write-Output "Private key: $PrivateKeyPath  (never commit this file)"
Write-Output "Public keys: $PublicKeysPath  (commit this file)"
Write-Output 'Next: put the private key PEM into the UPDATE_SIGNING_KEY GitHub secret, commit the public key file, and set require_signature=true in updater.conf.'
