#Requires -Version 5.1
<#
.SYNOPSIS
  给更新元数据签上 RSA-PSS 信封（schema / keyId / payload / signature）。

.DESCRIPTION
  与 snow-shot 的发布脚本一致：
    * 签名对象是 payload JSON 的原始 UTF-8 字节；
    * 算法 RSA-PSS + SHA-256，salt 长度 32，密钥 3072 位（.NET 的 PSS 默认 salt 即哈希长度）；
    * 先校验私钥和公钥列表里的 modulus/exponent 匹配，再签，再自验；
    * 输出紧凑 JSON：{"schema":1,"keyId":"...","payload":"base64","signature":"base64"}。

.EXAMPLE
  powershell -File tools/sign-feed.ps1 `
    -FeedPath build/out/latest-version-windows-x64.json `
    -PrivateKeyPath D:\keys\update-signing.pem `
    -PublicKeysPath resources/update-trusted-keys.json
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$FeedPath,
    [Parameter(Mandatory)][string]$PrivateKeyPath,
    [Parameter(Mandatory)][string]$PublicKeysPath,
    [string]$OutPath
)

$ErrorActionPreference = 'Stop'

# .NET Framework (Windows PowerShell 5.1) 没有 ImportFromPem / PSS 支持，签名统一在 PS7 下做。
if ($PSVersionTable.PSVersion.Major -lt 7) {
    throw 'sign-feed.ps1 需要 PowerShell 7（pwsh）。CI 用的是 pwsh；本地可用 `pwsh -File tools/sign-feed.ps1 ...`。'
}
$FeedPath = [IO.Path]::GetFullPath($FeedPath)
$PrivateKeyPath = [IO.Path]::GetFullPath($PrivateKeyPath)
$PublicKeysPath = [IO.Path]::GetFullPath($PublicKeysPath)
if (-not $OutPath) { $OutPath = $FeedPath } else { $OutPath = [IO.Path]::GetFullPath($OutPath) }

if (-not (Test-Path -LiteralPath $PrivateKeyPath)) { throw "找不到私钥: $PrivateKeyPath" }
if (-not (Test-Path -LiteralPath $PublicKeysPath)) { throw "找不到公钥列表: $PublicKeysPath" }

# 只取原始字节参与签名，避免序列化差异。
$payloadBytes = [IO.File]::ReadAllBytes($FeedPath)
$payloadText = [Text.Encoding]::UTF8.GetString($payloadBytes)
# 线上 feed 必须是紧凑 JSON；带换行的美化版会让 payload 变成另一个字节序列。
$payloadBytes = [Text.Encoding]::UTF8.GetBytes(($payloadText | ConvertFrom-Json | ConvertTo-Json -Depth 12 -Compress))
$payloadText = [Text.Encoding]::UTF8.GetString($payloadBytes)

$rsa = [Security.Cryptography.RSA]::Create()
try {
    $rsa.ImportFromPem((Get-Content -Raw -LiteralPath $PrivateKeyPath))
    if ($rsa.KeySize -ne 3072) { throw "签名密钥必须是 3072 位，当前 $($rsa.KeySize)。" }

    $public = $rsa.ExportParameters($false)
    $modulus = [Convert]::ToBase64String($public.Modulus)
    $exponent = [Convert]::ToBase64String($public.Exponent)
    $matched = @((Get-Content -Raw -LiteralPath $PublicKeysPath | ConvertFrom-Json).keys |
        Where-Object { $_.modulus -ceq $modulus -and $_.exponent -ceq $exponent })
    if ($matched.Count -ne 1) {
        throw '私钥和公钥列表不匹配：请确认 resources/update-trusted-keys.json 已经包含这把公钥。'
    }

    $signature = $rsa.SignData($payloadBytes, [Security.Cryptography.HashAlgorithmName]::SHA256,
        [Security.Cryptography.RSASignaturePadding]::Pss)
    if (-not $rsa.VerifyData($payloadBytes, $signature, [Security.Cryptography.HashAlgorithmName]::SHA256,
            [Security.Cryptography.RSASignaturePadding]::Pss)) {
        throw '签名自验失败。'
    }

    $envelope = [ordered]@{
        schema    = 1
        keyId     = $matched[0].id
        payload   = [Convert]::ToBase64String($payloadBytes)
        signature = [Convert]::ToBase64String($signature)
    } | ConvertTo-Json -Compress

    [IO.File]::WriteAllText($OutPath, $envelope, [Text.UTF8Encoding]::new($false))
    Write-Output "已签名: $OutPath (keyId=$($matched[0].id))"
} finally {
    $rsa.Dispose()
}
