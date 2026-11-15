#Requires -Version 5.1
<#
.SYNOPSIS
  生成 3072 位 RSA 发布签名密钥，并输出可编译进客户端的公钥列表。

.DESCRIPTION
  与 snow-shot 的做法一致：
    * 私钥用 PKCS#8 PEM，必须放在仓库之外的独立目录，且目录里不能有别的东西；
    * 公钥写成 { "keys": [ { "id", "modulus", "exponent" } ] }，base64 编码；
    * 拒绝覆盖已存在的密钥，避免误操作导致老版本客户端再也验不过签名。

.EXAMPLE
  powershell -File tools/new-release-key.ps1 `
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
    throw '私钥不能放在仓库里。'
}
if ($PrivateKeyPath.Equals($PublicKeysPath, [StringComparison]::OrdinalIgnoreCase)) {
    throw '私钥和公钥输出路径不能相同。'
}
if (Test-Path -LiteralPath $PrivateKeyPath) { throw '私钥已存在，拒绝覆盖。' }
if (Test-Path -LiteralPath $PublicKeysPath) { throw '公钥文件已存在，拒绝覆盖。' }

$privateDirectory = Split-Path -Parent $PrivateKeyPath
if ((Test-Path -LiteralPath $privateDirectory) -and @(Get-ChildItem -LiteralPath $privateDirectory -Force).Count -gt 0) {
    throw '请给私钥用一个空目录。'
}
$null = New-Item -ItemType Directory -Force -Path $privateDirectory

$acl = [Security.AccessControl.DirectorySecurity]::new()
$acl.SetAccessRuleProtection($true, $false)
$identity = [Security.Principal.WindowsIdentity]::GetCurrent().User
$acl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new(
    $identity, 'FullControl', 'ContainerInherit,ObjectInherit', 'None', 'Allow'))
Set-Acl -LiteralPath $privateDirectory -AclObject $acl

$rsa = [Security.Cryptography.RSA]::Create(3072)
try {
    [IO.File]::WriteAllText($PrivateKeyPath, $rsa.ExportPkcs8PrivateKeyPem(), [Text.UTF8Encoding]::new($false))
    $parameters = $rsa.ExportParameters($false)
    $public = @{ keys = @(@{
        id = $KeyId
        modulus = [Convert]::ToBase64String($parameters.Modulus)
        exponent = [Convert]::ToBase64String($parameters.Exponent)
    }) } | ConvertTo-Json -Depth 5
    [IO.File]::WriteAllText($PublicKeysPath, $public + "`n", [Text.UTF8Encoding]::new($false))
} finally {
    $rsa.Dispose()
}

Write-Output "已生成签名密钥 '$KeyId'。私钥请离线备份，永远不要提交；公钥已写入 $PublicKeysPath。"
Write-Output '把私钥内容放进 GitHub Secret（例如 UPDATE_SIGNING_KEY），并把公钥文件提交后再构建客户端。'
