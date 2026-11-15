# generic-updater：外置通用更新器（Rust）

一个被任意语言主程序调用的独立更新器：基于 GitHub Release 做版本检查、下载、SHA256 校验、旁置替换、失败回滚。架构对标 snow-shot 的外置 updater。

## 0. 先回答最容易困惑的问题：我电脑没装 Rust，能开发/编译吗？

**分清两个角色：**

- **最终用户电脑**：永远不需要 Rust。updater 是开发者提前编译好的 `generic-updater.exe`，随软件一起分发，用户只运行这个成品 exe。
- **你（开发者）电脑**：要把 Rust 源码变成 exe，必须经过一次编译。但编译不一定要在你本地做，有两条路。

### 路径 A：本地装 Rust 工具链编译（一次性安装）

1. 装 Rust 工具链（rustup）：https://rustup.rs ，安装时选默认 stable 工具链；
2. Windows 还需要 **MSVC C++ 生成工具 + Windows SDK**：装 Visual Studio Installer，勾选"使用 C++ 的桌面开发"（rustup 的 msvc 目标依赖它）；
3. 打开终端，在项目目录执行：

```bash
cargo build --release
```

4. 产物在：

```
target\release\generic-updater.exe
```

> 想明确指定 Windows 目标：`cargo build --release --target x86_64-pc-windows-msvc`，产物在 `target\x86_64-pc-windows-msvc\release\`。

### 路径 B：GitHub Actions 云端编译（本地零环境，推荐不想装工具链时用）

仓库已带 `.github/workflows/build.yml`：

- 推一个 `v*` tag（或在 Actions 页面手动触发）；
- GitHub 的 `windows-latest` 云端机器自带 VS 工具链和 Rust 环境；
- 自动 `cargo build --release`，把 `generic-updater.exe` 挂到对应 Release 资产。

你本地只需要写代码、push，不用装 Rust、不用装 VS。

### 当前验证状态

- 本项目已在 Linux 环境 `cargo build --release` 通过，二进制可运行；
- Windows `.exe` 在 Windows runner（路径 B）或本地 MSVC 工具链（路径 A）产出；代码里 Windows 专属行为用标准库实现，未引入额外系统依赖。

## 1. 项目结构

```
rust-updater/
├── Cargo.toml              # 依赖与 release 优化配置
├── config.example.json      # 每软件一份的配置模板
├── .github/workflows/build.yml
└── src/
    ├── main.rs             # 入口：解析命令行参数
    ├── config.rs           # 配置读取、本地版本号读写
    ├── protocol.rs         # ↔ 主程序的 JSON 帧协议（状态/进度/版本/变更说明）
    ├── feed.rs             # GitHub Release API 拉取、版本匹配
    ├── download.rs         # 流式下载 .part、SHA256 校验、进度回调
    ├── install.rs          # 解压 staging、逐文件备份覆盖、回滚、清理
    ├── update.rs           # 编排：检查 → 下载 → 校验 → 安装 → 重启
    └── error.rs            # 统一错误类型
```

## 2. 命令行用法

```bash
# 只检查有没有新版本（不下载不安装）
generic-updater.exe --check  app.json

# 完整更新：检查 → 下载 → 校验 → 安装 → 拉起新版
generic-updater.exe --update app.json --parent-pid 12345
```

| 参数 | 含义 |
|---|---|
| `--check` | 仅检查模式 |
| `--update` | 完整更新模式（默认） |
| `<config.json>` | 本软件的配置文件路径 |
| `--parent-pid <pid>` | 主程序 PID，用于等待旧进程退出（Windows 生产版接 OpenProcess 等待） |

**退出码**：`0` = 无更新；`2` = 有更新 / 更新成功；`1` = 失败。

## 3. 配置文件字段

```json
{
  "repo": "your-name/your-app",
  "asset_name": "your-app-windows-x64.zip",
  "version_file": "version.txt",
  "install_dir": ".",
  "exe_name": "your-app.exe"
}
```

| 字段 | 含义 |
|---|---|
| `repo` | GitHub 仓库 `owner/name` |
| `asset_name` | 本平台资产文件名（`.zip` 目录包或 `.exe` 单文件） |
| `version_file` | 本地版本号文件，一行 `v1.2.3` |
| `install_dir` | 安装根目录，建议主程序传绝对路径 |
| `exe_name` | 单 exe 场景的目标程序名；zip 包用于定位主程序 |

## 4. 和主程序怎么通信（跨语言契约）

updater 启动后，**stdout 每行一条 JSON 状态帧**；主程序按行读取、解析后刷新自己的 UI：

```json
{"protocol":2,"type":"status","state":"Checking","version":"","notes":"","received":0,"total":0,"error":null}
{"protocol":2,"type":"status","state":"Available","version":"1.5.7","notes":"### 新功能\n- ...","received":0,"total":0,"error":null}
{"protocol":2,"type":"status","state":"Downloading","version":"1.5.7","notes":"...","received":1234567,"total":37764164,"error":null}
{"protocol":2,"type":"status","state":"Verifying","version":"1.5.7","notes":"...","received":0,"total":0,"error":null}
{"protocol":2,"type":"status","state":"Applying","version":"1.5.7","notes":"...","received":0,"total":0,"error":null}
{"protocol":2,"type":"status","state":"Ready","version":"1.5.7","notes":"...","received":1,"total":1,"error":null}
{"protocol":2,"type":"status","state":"Failed","version":"1.5.7","notes":"...","received":0,"total":0,"error":"失败原因"}
```

| 字段 | 用途 |
|---|---|
| `state` | 状态机阶段：Checking/Available/Downloading/Verifying/Ready/Applying/Failed |
| `version` | 检测到的新版本号 |
| `notes` | 变更说明（Release body，markdown），主程序渲染到更新弹窗 |
| `received` / `total` | 进度（下载阶段字节，安装阶段文件数） |
| `error` | 失败原因，成功为 null |

主程序侧只需要一个薄对接层：拉起进程 → 按行读 stdout JSON → 映射到自己的按钮/进度条/版本号/变更说明。

## 5. 主程序对接示例（任意语言伪代码）

```text
1. 主程序用自身路径 API 拿到安装目录（Windows: GetModuleFileNameW）
2. 写一份本软件的 app.json（repo / asset_name / install_dir / exe_name ...）
3. 拉起 generic-updater.exe --update app.json --parent-pid <自己的PID>
4. 读子进程 stdout 每行 JSON：
   - Available  → 弹窗显示版本号 + notes，按钮"立即更新"
   - Downloading→ 进度条 received/total
   - Ready       → 更新完成，准备重启
   - Failed      → 显示 error，可重试
```

## 6. 已实现 / 待扩展

**已实现**

- GitHub Release latest 检查 + semver 比较
- 流式下载到 `.part`，大小 + SHA256 校验，不符丢弃
- zip / exe 两种资产；解压到 staging
- 逐文件备份到 backup，覆盖安装；失败按备份回滚
- 状态帧回传版本号、notes、进度、错误
- 退出码约定；detached 拉起新版

**待扩展（生产化时再补）**

- 元数据签名验签（RSA-PSS 或 ed25519，公钥内置；当前只做 SHA256）
- Windows 挂起启动探测（CREATE_SUSPENDED 区分策略拦截 / 文件占用）
- 启动标记文件确认新版真的起来，超时回滚
- 断点续传（Range）、下载 stall 超时与退避重试
- 目录不可写时的 UAC 提权
- 多源兜底（站点 feed → GitHub）

## 7. 发布侧配合

1. 打 tag `v1.2.3`；
2. CI 编译主程序产物，打包 zip（或单 exe）；
3. 计算 SHA256，生成 `SHA256SUMS.txt`；
4. 上传到 GitHub Release，资产名与 `asset_name` 一致；
5. 本地 `version.txt` 随包分发，记录当前版本号。
