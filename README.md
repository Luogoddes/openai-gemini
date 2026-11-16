# generic-updater

外置通用更新器 + 一个仿「版本更新」弹窗的 Win32 客户端。任意语言的主程序拉起
`generic-updater.exe` 就能获得：**24h 自动检查 / 手动检查 / 下载进度 / SHA256 校验 /
RSA-PSS 元数据验签 / 备份替换 / 断电恢复 / UAC 提权 / 重启拉起 / 启动探测与回滚**。

更新契约（元数据形状、资产命名、状态机、stdout JSON 帧、签名信封）参考了
[snow-shot 的 `snow-shot-updater`](https://gitee.com/mg-chao/snow-apps/tree/main/snow_shot/rust/snow-shot-updater)，
代码是独立实现（对方的 `snow_shot/`、`snow-crates/` 是 GPL-3.0，只对齐接口，不复制实现）。

## 0. 文档与复用

| 文档 | 内容 |
|---|---|
| [docs/01-更新功能说明.md](docs/01-更新功能说明.md) | 架构、协议、检查机制、事务安装、签名、边界 |
| [docs/02-接入指南.md](docs/02-接入指南.md) | 一键脚手架、接入已有项目、发布/签名/排错清单 |
| [docs/03-设计决策记录.md](docs/03-设计决策记录.md) | 版本演进、关键取舍、踩坑记录、验证结果 |
| [scripts/scaffold-update.ps1](scripts/scaffold-update.ps1) | 一条命令把更新模块（含文档）生成到新项目 |
| [docs/skill/update-feature/SKILL.md](docs/skill/update-feature/SKILL.md) | 给 Agent 用的技能说明（可放进 DSH skills 目录） |

新项目接入（在源仓库里执行）：

```powershell
powershell -ExecutionPolicy Bypass -File scripts\scaffold-update.ps1 `
  -TargetDir D:\code\my-app -Repo "me/my-app" -ExeName "MyApp.exe" -Product "my-app"
```
## 1. 组成

| 目录 | 作用 |
|---|---|
| `src/` | Rust 更新器：检查、验签、下载、校验、事务安装、回滚、提权、拉起、探测 |
| `client/` | C++ Win32 GUI：版本弹窗 + 进度条 + 立即安装 / 跳过此版本 / 重启软件 |
| `examples/demo/` | 纯前端测试台，不开编译器也能点一遍 |
| `scripts/` | 密钥生成、元数据签名、GUI 配置头生成、脚手架 |
| `resources/` | 发布用资源（可信公钥表） |
| `docs/` | 功能说明、接入指南、设计决策；`docs/reviews/` 是历史审查归档 |
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
startup_check_delay_seconds=30
product=generic-updater
window_title=Generic Updater Demo — 版本更新
require_signature=true
```

- Rust 侧：`build.rs` 在编译时读它，生成常量编进 `generic-updater.exe`；
- C++ 侧：`scripts/gen-app-config.ps1` 生成 `client/build-config.h`，编进 `GuiUpdaterDemo.exe`；
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
- 下载的 zip、解压 staging、备份 backup 都在这下面；安装成功后只保留 `state.json`（24h 时间戳
  与跳过记录），文件清单是安装目录里的隐藏文件 `.update-files.json`，其余全删。
- 失败一律回滚：exe、版本号、新增文件、被删掉的旧文件都还原，zip 留着方便重试，
  用户不会看到半成品。

## 6. 安装事务、进程协调与权限

| 能力 | 做法 |
|---|---|
| 24h 自动检查 | 时间戳存缓存目录的 `state.json`，检查失败不刷新 |
| 等待旧进程 | `OpenProcess + WaitForSingleObject`，拿不到句柄视为已退出；另有占用重试兜底 |
| 断电恢复 | 覆盖前写 `journal.json`（先写临时文件再改名），装完标 `committed`；启动时 `--recover` 先回滚再清理 |
| 失败回滚 | journal 驱动回滚：备份文件、新增文件、被删文件全部还原；回滚失败保留备份等下次恢复 |
| 启动探测 | 拉起后 3 秒内进程存活即算成功；提前退出则回滚并重新拉起旧版 |
| 旧文件清理 | 记录安装清单，升级时把"上个版本有、这个版本没有"的文件移进备份区（回滚可还原） |
| UAC 提权 | 安装目录不可写且未提权时，`ShellExecuteW("runas")` 重启自身（带 `--elevated --cache <缓存目录>`） |
| 自更新 | 运行中的 updater 先改名成 `*.old-<pid>` 再写新文件，下次启动清理 |
| 单实例 | 缓存目录 `updater.lock` 跨进程互斥（10 秒等待，陈旧锁自动接管）；GUI 另外用互斥体 |

## 7. 命令行

```powershell
generic-updater.exe --check --auto          # 启动时自动检查（受 24h 约束）
generic-updater.exe --check                 # 手动检查
generic-updater.exe --download              # 下载 + 验签 + 校验 + 解压暂存
generic-updater.exe --apply --parent-pid 1234
generic-updater.exe --recover               # 事务恢复
generic-updater.exe --clean                 # 清缓存
generic-updater.exe --skip 0.5.0            # 跳过这个版本
generic-updater.exe --unskip                # 取消跳过
generic-updater.exe --verify-feed latest-version-windows-x64.json   # 本地校验签名
generic-updater.exe --check -c test.json    # 调试：覆盖内置配置
generic-updater.exe --apply --cache D:\cache\update   # 显式指定缓存目录（提权重启会自动带上）
```

退出码：`0` 无更新 / `2` 有更新·已就绪·更新成功 / `1` 失败。

## 8. 签名（RSA-PSS）

信封：`{"schema":1,"keyId":"...","payload":"<base64>","signature":"<base64>"}`，
签名对象是 payload 的原始 UTF-8 字节，RSA-PSS + SHA-256 + salt 32，密钥 3072 位。

### 密钥只需要配置一次

密钥是**一次性投入**，之后每次发版都不用再碰：

| 东西 | 存在哪 | 多久动一次 |
|---|---|---|
| 私钥 | GitHub Secret `UPDATE_SIGNING_KEY` | 生成时配置一次，之后不动 |
| 公钥 | `resources/update-trusted-keys.json`（编译进 exe） | 生成时提交一次，之后不动 |
| 发布流程 | 改 `CHANGELOG.md` → 打 tag → push | 每次发版 |

CI 每次都从同一个 secret 读私钥、用同一把公钥对应的 keyId 签名，所以**不需要每次上传重新配置**。

几条纪律：

- **私钥要备份**：客户端里烧的是公钥，私钥丢了就再也签不出老客户端认的包；
- **不要随手换密钥**：旧客户端只认旧公钥，换了就收不到更新；
- 要轮换时：先往 `resources/update-trusted-keys.json` 的 `keys` 数组里**追加**新公钥（新 `id`），
  发一个用新 keyId 签名的过渡版本，等用户升上来后再移除旧公钥。
**没配 secret 的时候 feed 就是明文 JSON**——这是默认状态，不是坏了。开启签名要四步：

```powershell
# 1) 生成密钥：私钥必须放仓库外，公钥写进仓库
powershell -File scripts/new-release-key.ps1 `
  -PrivateKeyPath D:\keys\update-signing.pem `
  -PublicKeysPath resources/update-trusted-keys.json `
  -KeyId generic-updater-2026-01
```

> 密钥生成脚本兼容 Windows PowerShell 5.1；`scripts/sign-feed.ps1` 需要 PowerShell 7（CI 用的就是 `pwsh`）。
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
cargo build --release --workspace   # target\release\generic-updater.exe + GuiUpdaterDemo.exe
cargo test                          # 单测：签名契约、journal 回滚、安装事务、下载、锁
```

## 12. 已知边界

- **不支持增量**：每次整包下载；断点续传只针对同一个包的下载中断。
- **没有 Authenticode 证书**：SmartScreen 仍提示未知发布者；元数据签名不等于代码签名。
- **断电恢复依赖 journal 落盘**：journal 已改为临时文件 + 改名，极端情况下仍是"最后一份完整
  journal"生效；若备份本身损坏可以用 `--clean` 重来。
- **平台**：Windows x64 已实测；其他平台只有裸编译路径。






