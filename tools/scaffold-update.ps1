#Requires -Version 5.1
<#
.SYNOPSIS
  Scaffold the update feature into a target directory.

.DESCRIPTION
  Copies the updater crate, the Win32 GUI client, the release workflow and the
  key/signing tools into -TargetDir, then rewrites updater.conf for the target
  project. After that the target only needs: a CHANGELOG entry, a tag, and a push.

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File tools/scaffold-update.ps1 `
    -TargetDir D:\code\my-app `
    -Repo "me/my-app" `
    -ExeName "MyApp.exe" `
    -Product "my-app"
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$TargetDir,
    [Parameter(Mandatory)][string]$Repo,
    [Parameter(Mandatory)][string]$ExeName,
    [string]$Product,
    [string]$Homepage,
    [string]$WindowTitle,
    [int]$IntervalHours = 24,
    [string]$Variant = 'portable',
    [string]$Platform = 'windows-x64',
    [switch]$Force
)

$ErrorActionPreference = 'Stop'
$source = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$target = [IO.Path]::GetFullPath($TargetDir)

if (-not $Product) {
    $Product = ($Repo -split '/')[-1]
    if (-not $Product) { $Product = 'my-app' }
}
if (-not $Homepage) { $Homepage = "https://github.com/$Repo" }
if (-not $WindowTitle) { $WindowTitle = "$Product — 版本更新" }

if (-not (Test-Path -LiteralPath $target)) {
    $null = New-Item -ItemType Directory -Force -Path $target
} elseif (-not $Force -and @(Get-ChildItem -LiteralPath $target -Force).Count -gt 0) {
    throw "目标目录非空：$target（确认覆盖请加 -Force）"
}

function Copy-Tree([string]$relative) {
    $from = Join-Path $source $relative
    $to = Join-Path $target $relative
    if (-not (Test-Path -LiteralPath $from)) { return }
    $parent = Split-Path -Parent $to
    if ($parent -and -not (Test-Path -LiteralPath $parent)) {
        $null = New-Item -ItemType Directory -Force -Path $parent
    }
    Copy-Item -LiteralPath $from -Destination $to -Recurse -Force
}

Write-Output "source: $source"
Write-Output "target: $target"

foreach ($relative in @(
        'Cargo.toml',
        'Cargo.lock',
        'build.rs',
        'src',
        'resources',
        'app',
        'tools'
    )) {
    Copy-Tree $relative
}

# 生成物不进发布包
foreach ($junk in @('app\build-config.h', 'dist', 'build')) {
    $path = Join-Path $target $junk
    if (Test-Path -LiteralPath $path) { Remove-Item -LiteralPath $path -Recurse -Force }
}

$workflowDir = Join-Path $target '.github\workflows'
$null = New-Item -ItemType Directory -Force -Path $workflowDir
Copy-Item -LiteralPath (Join-Path $source '.github\workflows\build.yml') `
    -Destination (Join-Path $workflowDir 'update-release.yml') -Force

$config = @"
repo=$Repo
feed_name=latest-version-$Platform.json
exe_name=$ExeName
homepage=$Homepage
variant=$Variant
platform=$Platform
interval_hours=$IntervalHours
product=$Product
window_title=$WindowTitle
require_signature=false
"@
[IO.File]::WriteAllText((Join-Path $target 'updater.conf'), $config + "`r`n", (New-Object Text.UTF8Encoding($false)))

$changelog = @"
# 更新日志

发布说明只写三件事：**新增、修改、删除**，一行一条，顺序固定为新增 → 修改 → 删除。
CI 会按版本号从本文件里抽出对应小节，同时用于 Release 说明和客户端弹窗里的「更新日志」。

## 0.1.0

### 新增
- 接入自动更新：24 小时检查、手动刷新、下载校验、重启完成

### 修改
- 无

### 删除
- 无
"@
[IO.File]::WriteAllText((Join-Path $target 'CHANGELOG.md'), $changelog + "`r`n", (New-Object Text.UTF8Encoding($false)))

$readme = @"
# $Product 更新模块

由 scaffold-update.ps1 生成。完整说明见源仓库的 `docs/01-更新功能说明.md` 与 `docs/02-接入指南.md`。

## 本地编译

    cargo build --release
    set MSVC_SETUP=<你的 msvc setup_x64.bat>
    app\build-msvc.bat

## 发布

1. 在 CHANGELOG.md 写当前版本小节（新增/修改/删除）
2. 打 tag 并推送：`git tag v0.1.0; git push origin v0.1.0`
3. CI 产出 zip + sha256 + latest-version-windows-x64.json

## 开启签名

    powershell -File tools/new-release-key.ps1 -PrivateKeyPath D:\keys\update-signing.pem -PublicKeysPath resources/update-trusted-keys.json
    私钥放入 GitHub Secret UPDATE_SIGNING_KEY；updater.conf 改 require_signature=true

## 配置

所有配置都在 `updater.conf`，构建时编译进 exe，发布包不需要 ini。
"@
[IO.File]::WriteAllText((Join-Path $target 'README-UPDATE.md'), $readme + "`r`n", (New-Object Text.UTF8Encoding($false)))

Write-Output ''
Write-Output "已生成："
Get-ChildItem -LiteralPath $target -Force | Select-Object -ExpandProperty Name | ForEach-Object { Write-Output "  $_" }
Write-Output ''
Write-Output '下一步：'
Write-Output '  1) cargo build --release 验证更新器能编译'
Write-Output '  2) app\build-msvc.bat 验证 GUI 能编译（记得 set MSVC_SETUP=...）'
Write-Output '  3) 改 CHANGELOG.md，打 tag，push，等 CI 出包'
Write-Output '  4) 需要签名时跑 tools/new-release-key.ps1，并把 require_signature 改成 true'
