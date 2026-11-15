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
| `app/` | C++ Win32 GUI：版本弹窗 + 进度条 + 立即安装 / 跳过此版本 / 重启软件 |
| `demo/` | 纯前端测试台，不开编译器也能点一遍 |
| `tools/` | 密钥生成、元数据签名、GUI 配置头生成 |
| `updater.conf` | **唯一的发布配置**，构建时分别编译进 Rust 与 C++ |
| `CHANGELOG.md` | 发布说明来源，CI 按版本号抽取 |
| `.github/workflows/build.yml` | 编译 → 打包 → 校验文件 → feed（可选签名）→ Release |

## 2. 配置是编译期的，发布只有一个 exe

`updater.conf`：

```ini
repo=Luogoddes/openai-gemini
feed_name=latest-version-windows-x64.json
exe_name=GuiUpdaterDemo.exe
homepage=https://github.com/Luogoddes/openai-gemini
variant=portable
platform=windows-x64
interval_hours=24
product=generic-updater
window_title=Generic Updater Demo — 版本更新
require_signature=false
```

- Rust 侧：`build.rs` 在编译时读它，生成常量编进 `generic-updater.exe`；
- C++ 侧：`tools/gen-app-config.ps1` 生成 `app/build-config.h`，编进 `GuiUpdaterDemo.exe`；
- 发布包里没有 ini/json，用户改不了也丢不了；本地调试可用 `--config xxx.json` 或
  `UPDATER_CONFIG` 环境变量覆盖。

## 3. 版本号只看 tag

打什么 tag 就发什么版本，CI 用 tag 推导出的版本命名 zip、写 `version.txt`、写 feed：

- `v0.5.2` → `0.5.2`
- `v1.5.7_snow-shot` → `1.5.7`（下划线后缀丢弃，兼容 snow-shot 的命名）

`Cargo.toml` 里的 version 只是这个工具自身的编号，不再参与发布；改不改都不影响出包版本。

## 4. 为什么既有 feed、又有 `.sha256`

- **feed（`latest-version-windows-x64.json`）**：装的是"这次发布的完整契约"——product、
  platform、version、notes、包的 path/size/sha256，并且**整体被 RSA-PSS 签名**。
  GitHub API 能告诉版本和资产列表，但告诉不了"这个包属于这个产品/这个平台/这个版本"，
  也无法离线验真（API 响应不签名）。snow-shot 同样保留 feed，还把逐文件清单放进去。
- **`.sha256` 旁置文件**：GitHub 现在确实给每个资产返回 `digest`，但 Gitee 镜像没有、
  老资产可能没有、手工下载和 WinGet/Scoop 清单都更认这个文本文件。snow-shot 也是两边都传。
  更新器本身只认 feed 里签过名的 sha256，旁置文件是给人和别的工具用的。

## 5. 下载放哪、什么时候删

- 缓存根目录：`%LOCALAPPDATA%\GenericUpdater\updates\<安装目录哈希>\`，**不在安装目录**；
  系统缓存不可写（受限账户/沙箱）时退回 `<install_dir>\.app-update` 并打日志。
- 下载的 zip、解压 staging、备份 backup 都在这下面；安装成功后只保留 `state.json`（24h 时间戳）
  和 `installed.json`（文件清单），其余全删。
- 失败一律回滚：exe、版本号、新增文件都还原，zip 留着方便重试，用户不会看到半成品。

## 6. 安装事务、进程协调与权限

| 能力 | 做法 |
|---|---|
| 24h 自动检查 | 时间戳存缓存目录的 `state.json`，检查失败不刷新 |
| 等待旧进程 | `OpenProcess + WaitForSingleObject`，拿不到句柄视为已退出；另有占用重试兜底 |
| 断电恢复 | 覆盖前写 `journal.json`，装完标 `committed`；启动时 `--recover` 还原未提交事务 |
| 失败回滚 | 备份目录 + 回滚 + 启动探测失败回滚（进程提前退出，或 8 秒内没起来） |
| 旧文件清理 | 记录安装清单，升级时删掉"上个版本有、这个版本没有"的文件 |
| UAC 提权 | 安装目录不可写且未提权时，`ShellExecuteW("runas")` 重启自身（带 `--elevated`） |
| 自更新 | 运行中的 updater 先改名成 `*.old-<pid>` 再写新文件，下次启动清理 |
| 单实例 | GUI 用互斥体，并带重试，避免重启瞬间误判 |

## 7. 命令行

```powershell
generic-updater.exe --check --auto          # 启动时自动检查（受 24h 约束）
generic-updater.exe --check                 # 手动检查
generic-updater.exe --download              # 下载 + 验签 + 校验 + 解压暂存
generic-updater.exe --apply --parent-pid 1234
generic-updater.exe --recover               # 事务恢复
generic-updater.exe --clean                 # 清缓存
generic-updater.exe --verify-feed latest-version-windows-x64.json   # 本地校验签名
generic-updater.exe --check -c test.json    # 调试：覆盖内置配置
```

退出码：`0` 无更新 / `2` 有更新·已就绪·更新成功 / `1` 失败。

## 8. 签名（RSA-PSS）

信封：`{"schema":1,"keyId":"...","payload":"<base64>","signature":"<base64>"}`，
签名对象是 payload 的原始 UTF-8 字节，RSA-PSS + SHA-256 + salt 32，密钥 3072 位。

**没配 secret 的时候 feed 就是明文 JSON**——这是默认状态，不是坏了。开启签名要四步：

```powershell
# 1) 生成密钥：私钥必须放仓库外，公钥写进仓库
powershell -File tools/new-release-key.ps1 `
  -PrivateKeyPath D:\keys\update-signing.pem `
  -PublicKeysPath resources/update-trusted-keys.json `
  -KeyId generic-updater-2026-01
```

> 密钥生成脚本兼容 Windows PowerShell 5.1；`tools/sign-feed.ps1` 需要 PowerShell 7（CI 用的就是 `pwsh`）。
> 本地只生成密钥的话，`powershell` 就够。

2. 打开私钥文件，把**整段 PEM**（含 `-----BEGIN RSA PRIVATE KEY-----`）复制出来；
3. GitHub 仓库 → Settings → Secrets and variables → Actions → New repository secret，
   名称 `UPDATE_SIGNING_KEY`，值粘贴 PEM；
4. 把 `resources/update-trusted-keys.json` 提交，并把 `updater.conf` 的
   `require_signature` 改成 `true`，然后打 tag。

发布后这样确认签名生效（不用装客户端）：

```powershell
# 下载 release 里的 latest-version-windows-x64.json，首字符应该是 { 且含 "payload"/"signature"
# 然后本地校验：
generic-updater.exe --verify-feed latest-version-windows-x64.json
# 输出：签名有效: keyId=generic-updater-2026-01 version=0.5.2 packages=1
```

CI 有两道保险：`require_signature=true` 但没配 secret → 直接失败；配了 secret 但公钥文件为空
→ 提示先跑密钥生成脚本。所以不会出现"发了签名包却装不上"或"设了 secret 却还是明文"。

## 9. 发布说明

`CHANGELOG.md` 按版本分节，只写 **新增 / 修改 / 删除**，一行一条，顺序固定。
CI 抽出当前版本小节，同时写进 Release 说明和 feed 的 `notes`，也就是客户端弹窗里显示的内容。

## 10. 状态帧协议

stdout 一行一条 JSON：

```json
{"protocol":2,"type":"status","state":"Available","version":"0.5.0","notes":"…","received":0,"total":1209805,"error":null,"automatic":false}
{"protocol":2,"type":"status","state":"Ready","version":"0.5.0","notes":"…","received":1,"total":1,"error":null,"automatic":false}
{"protocol":2,"type":"status","state":"Failed","version":"0.5.0","notes":"","received":0,"total":0,"error":"元数据签名无效（内容被改过或私钥不对）","automatic":false}
```

## 11. 本地编译与测试

```powershell
cargo build --release                 # target\release\generic-updater.exe
set MSVC_SETUP=E:\打包\NuitkaGUI\msvc\setup_x64.bat
app\build-msvc.bat                    # dist\GuiUpdaterDemo.exe
cargo test                            # 15 个单测（含签名、journal、文件清单）
```

## 12. 已知边界

- **不支持增量**：每次整包下载。
- **没有 Authenticode 证书**：SmartScreen 仍提示未知发布者；元数据签名不等于代码签名。
- **断电恢复依赖 journal 落盘**：极端情况（journal 写一半断电）仍需 `--clean`。
- **平台**：Windows x64 已实测；其他平台只有裸编译路径。



