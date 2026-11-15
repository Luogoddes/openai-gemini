# generic-updater

外置通用更新器 + 一个仿「版本更新」弹窗的 Win32 客户端。任意语言的主程序拉起
`generic-updater.exe` 就能获得：**24h 自动检查 / 手动检查 / 下载进度 / SHA256 校验 /
RSA-PSS 元数据验签 / 备份替换 / 断电恢复 / UAC 提权 / 重启拉起 / 启动探测与回滚**。

更新契约（元数据形状、资产命名、状态机、stdout JSON 帧、签名信封）参考了
[snow-shot 的 `snow-shot-updater`](https://gitee.com/mg-chao/snow-apps/tree/main/snow_shot/rust/snow-shot-updater)，
代码是独立实现（对方的 `snow_shot/`、`snow-crates/` 是 GPL-3.0，只对齐接口，不复制实现）。

## 1. 组成

| 目录 | 作用 |
|---|---|
| `src/` | Rust 更新器：检查、验签、下载、校验、事务安装、回滚、提权、拉起、探测 |
| `app/` | C++ Win32 GUI 客户端：版本弹窗 + 进度条 + 立即安装 / 跳过此版本 / 重启软件 |
| `demo/` | 纯前端测试台，不开编译器也能把流程点一遍 |
| `tools/` | 密钥生成与元数据签名脚本 |
| `.github/workflows/build.yml` | 编译两端 → 打包 zip → 出 `.sha256` 与 feed（配了 secret 就签名） |

GUI 只做 UI 和进程协调，网络与文件操作全部交给 Rust 更新器，两者用 stdout 一行一条 JSON 通信。
「重启软件」是真的重启：客户端先拉起 `--apply` 再退出，更新器替换文件、清缓存，detached 拉起新版，
并等客户端写 `started.flag` 确认启动成功，超时自动回滚。

## 2. 最快的体验方式

### 2.1 浏览器测试台（不装工具链）

```powershell
cd demo
node server.mjs      # 打开 http://127.0.0.1:8124/
```

自动检查 → 立即更新（真实算 SHA256）→ 重启软件（模拟备份/覆盖/清理）→ 24h 节流验证。

### 2.2 真实链路（GitHub Actions）

1. 改 `Cargo.toml` 的 `version`，push，打 tag：`git tag v0.4.0; git push origin v0.4.0`
2. Actions 产出：`generic-updater-0.4.0-windows-x64-portable.zip`、同名 `.sha256`、`latest-version-windows-x64.json`
3. 下载解压运行 `GuiUpdaterDemo.exe`：首次自动检查（24h 内不重复），版本相同显示「当前已是最新版本」
4. 把 `Cargo.toml` 改成 `0.4.1` 打 `v0.4.1` tag，回到旧窗口点 **检查更新**（手动无视 24h）→ 出现新版 +
   更新日志 + 大小 → **立即安装** → **重启软件** → 窗口自动变成 0.4.1

### 2.3 本地编译

```powershell
cargo build --release            # target\release\generic-updater.exe

set MSVC_SETUP=E:\打包\NuitkaGUI\msvc\setup_x64.bat
app\build-msvc.bat               # dist\GuiUpdaterDemo.exe

app\build-mingw.bat              # 有全局 g++ 时（脚本会 cd 到仓库根用相对路径，避开中文路径）
```

## 3. 命令行

```powershell
generic-updater.exe --check --auto app.json    # 启动时自动检查（受 24h 间隔约束）
generic-updater.exe --check app.json           # 手动检查（无视间隔）
generic-updater.exe --download app.json        # 下载 + 验签 + 校验 + 解压暂存，停在 Ready
generic-updater.exe --apply app.json           # 备份→覆盖→清理→拉起→启动探测
generic-updater.exe --recover app.json         # 只做事务恢复（上次中断时按备份回滚）
generic-updater.exe --clean app.json           # 清缓存
```

退出码：`0` 无更新 / `2` 有更新·已就绪·更新成功 / `1` 失败。

## 4. 配置

GUI 读同目录 `app.ini`，翻译成更新器的 `updater.json`：

```ini
repo=Luogoddes/openai-gemini
feed_name=latest-version-windows-x64.json
feed_url=                       # 留空走 Release；本地测试可填 json 路径或直链
exe_name=GuiUpdaterDemo.exe
homepage=https://github.com/Luogoddes/openai-gemini
variant=portable
platform=windows-x64
interval_hours=24
require_signature=false         # 生产建议 true，配合下面的签名流程
```

## 5. 签名（RSA-PSS，与 snow-shot 同构）

格式：`{"schema":1,"keyId":"...","payload":"<base64>","signature":"<base64>"}`，
签名对象是 payload JSON 的**原始 UTF-8 字节**，算法 RSA-PSS + SHA-256 + salt 32，密钥 3072 位。
公钥编译进 `generic-updater.exe`（`resources/update-trusted-keys.json`），私钥只在发布机器上。

**snow-shot 的做法**（我照着对齐的）：

1. `scripts/new-snow-shot-release-key.ps1` 生成 3072 位 RSA，私钥 PKCS#8 PEM 写到仓库外的空目录并锁 ACL；
   公钥以 `{id, modulus(base64), exponent(base64)}` 写进 `snow_shot/resources/update-trusted-keys.json` 并提交。
2. 发布脚本 `publish-snow-shot-release.ps1` 组装 payload（schema/product/version/publishedAt/platform/packages，
   非安装器包还带 `files` 逐文件清单），用 .NET `RSA.SignData(..., SHA256, PSS)` 签，
   再 `VerifyData` 自验，输出紧凑信封。
3. 签名前会校验「私钥 modulus/exponent 必须匹配已嵌入的公钥」，且 `KeySize == 3072`。
4. PSS 的 salt 是随机的，所以重复发布同一版本时会复用线上已有的信封（版本 + 包清单一致才允许），保证幂等。
5. 上传前用编译好的 updater 跑 `--audit-release`，把签名、压缩包、启动探测全验一遍才发。

**本项目的步骤**：

```powershell
# 1) 生成密钥（私钥放仓库外！公钥写进仓库）
powershell -File tools/new-release-key.ps1 `
  -PrivateKeyPath D:\keys\update-signing.pem `
  -PublicKeysPath resources/update-trusted-keys.json `
  -KeyId generic-updater-2026-01

# 2) 把私钥内容放进 GitHub Secret：UPDATE_SIGNING_KEY
# 3) 提交公钥文件后重新构建客户端（公钥会被编译进去）
# 4) 打 tag 发布：workflow 检测到 secret 会自动签名，并把 zip 里的 require_signature 改成 true
```

客户端校验失败会直接拒绝更新：`校验失败: 元数据签名无效（内容被改过或私钥不对）`。

## 6. 元数据（feed）规范

| 资产 | 说明 |
|---|---|
| `latest-version-windows-x64.json` | 元数据（签名信封或明文） |
| `<product>-<version>-windows-x64-portable.zip` | 便携包 |
| `<product>-<version>-windows-x64-portable.zip.sha256` | 旁置校验，`<hash>  <文件名>` |

payload（验签后）结构：

```json
{
  "schema": 1,
  "product": "generic-updater",
  "version": "0.4.0",
  "publishedAt": "2026-11-15T10:00:00Z",
  "platform": "windows-x64",
  "notes": "- 新增：签名校验\n- 新增：断电恢复",
  "packages": [
    {
      "variant": "portable",
      "kind": "portable",
      "path": "generic-updater-0.4.0-windows-x64-portable.zip",
      "size": 1209805,
      "sha256": "b0b7…"
    }
  ]
}
```

## 7. 状态帧协议

stdout 一行一条 JSON：

```json
{"protocol":2,"type":"status","state":"Available","version":"0.4.1","notes":"…","received":0,"total":1209805,"error":null,"automatic":false}
{"protocol":2,"type":"status","state":"Downloading","version":"0.4.1","notes":"…","received":65536,"total":1209805,"error":null,"automatic":false}
{"protocol":2,"type":"status","state":"Ready","version":"0.4.1","notes":"…","received":1,"total":1,"error":null,"automatic":false}
{"protocol":2,"type":"status","state":"Failed","version":"0.4.1","notes":"","received":0,"total":0,"error":"元数据签名无效（内容被改过或私钥不对）","automatic":false}
```

状态与 snow-shot 的 `UpdateState` 一致，`automatic` 区分自动/手动检查。

## 8. 安装事务、进程协调与权限

| 能力 | 做法 |
|---|---|
| 24h 自动检查 | 时间戳存 `.app-update/state.json`，检查失败不刷新 |
| 等待旧进程 | `OpenProcess + WaitForSingleObject`，拿不到句柄视为已退出；另有文件占用重试兜底 |
| 断电恢复 | 覆盖前写 `journal.json`（目标 + 备份），装完标 `committed`；启动时 `--recover` 按未提交事务还原 |
| 失败回滚 | 备份目录 + 回滚 + 启动探测超时回滚（30s 内没等到 `started.flag` 或子进程提前退出） |
| UAC 提权 | 安装目录不可写且未提权时，`ShellExecuteW("runas")` 重启自身（带 `--elevated`） |
| 自更新 | 运行中的 `generic-updater.exe` 不能覆盖：先改名成 `*.old-<pid>` 再写新文件，下次启动清理 |
| 清理 | 成功后删 staging/backup/zip、旧镜像，缓存目录只留 `state.json`（检查时间戳） |

## 9. 已知边界

- **不支持增量**：每次整包下载（snow-shot 的 `files` 清单可做逐文件增量，这里没实现）。
- **没有代码签名证书**：SmartScreen 仍会提示未知发布者；要彻底消除需要 Authenticode（EV 证书）。
- **提权只覆盖"目录不可写"**：安装到用户目录时不会弹 UAC。
- **断电恢复依赖 journal 落盘**：极端情况下（journal 写了一半就断电）仍可能残留，需要手动 `--clean`。
- **平台**：Windows x64 已实测；Linux/macOS 只有裸编译路径，没有安装器。

## 10. 目录结构

```
├── Cargo.toml
├── app/
│   ├── src/gui.cpp          # Win32 界面 + 进程协调 + started.flag
│   ├── app.ini
│   ├── build-msvc.bat
│   └── build-mingw.bat
├── demo/                    # 浏览器测试台
├── resources/update-trusted-keys.json
├── tools/
│   ├── new-release-key.ps1
│   └── sign-feed.ps1
├── .github/workflows/build.yml
└── src/
    ├── main.rs              # 命令行
    ├── config.rs            # 配置与本地版本号
    ├── state.rs             # 24h 时间戳 + 待安装记录
    ├── contract.rs          # 签名信封校验（RSA-PSS）
    ├── journal.rs           # 事务日志与恢复
    ├── platform.rs          # 进程等待、UAC、可写探测、重试
    ├── feed.rs              # 元数据获取、验签、解析
    ├── download.rs          # 流式下载、SHA256、原子落地
    ├── install.rs           # 解压、备份覆盖、自更新改名、回滚、清理
    ├── update.rs            # 状态机编排
    ├── protocol.rs          # JSON 帧
    └── error.rs
```
