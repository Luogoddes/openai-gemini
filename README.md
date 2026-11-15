# generic-updater：外置通用更新器（Rust）

被任意语言的主程序拉起，基于 Release 元数据完成 **检查 → 下载 → 校验 → 旁置替换 → 回滚 → 重启** 的独立更新器。状态机与元数据形态参考了 [snow-shot 的 `snow-shot-updater`](https://gitee.com/mg-chao/snow-apps/tree/main/snow_shot/rust/snow-shot-updater)，但代码是独立实现（对方的 `snow_shot/`、`snow-crates/` 是 GPL-3.0，只借鉴契约与流程，不复制实现）。

## 1. 三十秒体验（不用装 Rust）

`demo/` 是一份纯前端测试台，把整条更新链路跑在浏览器里：

```powershell
cd demo
node server.mjs
# 打开 http://127.0.0.1:8124/
```

页面会自动触发一次"打开软件时检查"，然后可以点 **立即更新** 看下载进度 + SHA256 校验，再点 **重启软件** 走完备份/覆盖/清理/拉起。右上角的当前版本会从 `1.0.0` 变成 `1.2.0`。

> 为什么不用 `python -m http.server`：本机安全软件会拦截 URL 以 `.zip` 结尾的浏览器请求（直接回 204 空响应），所以 demo 走 `/feed/portable` 这个无扩展名路径返回同一份字节。真实更新器用 reqwest，不受这个浏览器侧拦截影响。

## 2. 更新流程

```
                          ┌────────────────────────────┐
 打开软件 ──(--check --auto)──► 距上次检查 ≥ 24h ? ──否──► Idle（不联网）
                          └───────────┬────────────────┘
                                     是
 点"刷新" ──(--check)──────────────────┤
                                      ▼
                                 Checking ──► Available(version + notes)
                                      │              │ 用户同意
                                      ▼              ▼
                                 Idle(已最新)    Downloading(received/total)
                                                     │
                                                     ▼
                                                 Verifying(SHA256)
                                                     │
                                                     ▼
                                                  Ready  ← 等用户点"重启软件"
                                                     │ 主程序退出
                                                     ▼
                                                 Applying（备份→覆盖→清理）
                                                     │
                                                     ▼
                                                 relaunch 新版
```

对应三个命令：

| 命令 | 做什么 | 退出码 |
|---|---|---|
| `--check --auto` | 自动检查，受 `automatic_check_interval_hours` 约束 | 0 无更新 / 2 有更新 |
| `--check` | 手动检查，无视间隔 | 同上 |
| `--download` | 检查 + 下载 + 校验 + 解压暂存，停在 `Ready` | 0 无更新 / 2 就绪 |
| `--apply` | 应用已暂存的更新：备份、覆盖、清理、拉起 | 2 成功 |
| `--clean` | 清掉下载缓存与暂存 | 0 |

主程序侧的最小对接：

1. 启动时跑 `--check --auto`，读到 `Available` 就弹更新提示（带版本号和 notes）；
2. 用户同意 → `--download`，按 `Downloading` 帧刷新进度条；
3. 拿到 `Ready` 后显示 **重启软件** 按钮；用户点击 → 主程序退出 → 拉起 `--apply`。

## 3. 配置文件

```json
{
  "repo": "Luogoddes/openai-gemini",
  "feed_name": "latest-version-windows-x64.json",
  "version_file": "version.txt",
  "install_dir": ".",
  "exe_name": "generic-updater.exe",
  "variant": "portable",
  "platform": "windows-x64",
  "automatic_check_interval_hours": 24
}
```

| 字段 | 含义 |
|---|---|
| `repo` | Release 所在仓库 `owner/name` |
| `feed_name` | 元数据资产名，按平台区分，见下节 |
| `feed_url` | 可选。元数据直链或本地路径，覆盖默认的 Release 查找逻辑（本地测试用） |
| `api_base` | 可选，默认 `https://api.github.com` |
| `version_file` | 本地版本号文件，一行 `1.2.3` |
| `install_dir` | 安装根目录，建议传绝对路径 |
| `exe_name` | 安装后要拉起的程序名 |
| `variant` | 用哪类包：`portable` / `online` / `offline` |
| `platform` | 与元数据里的 `platform` 比对，防止装错平台 |
| `automatic_check_interval_hours` | 自动检查最小间隔，默认 24 |
| `state_file` | 可选，检查记录文件，默认 `<install_dir>/.app-update/state.json` |

## 4. 元数据（feed）规范

资产名沿用 snow-shot 的约定：

| 资产 | 说明 |
|---|---|
| `latest-version-windows-x64.json` | Windows x64 元数据（本项目当前只发 Windows） |
| `<product>-<version>-windows-x64-portable.zip` | 便携包 |
| `<product>-<version>-windows-x64-portable.zip.sha256` | 旁置校验文件，`<hash>  <文件名>` |

多平台时再加 `latest-version-windows-arm64.json`、`latest-version-macos-arm64.json` 等即可，客户端只认自己那份。

```json
{
  "schema": 1,
  "product": "generic-updater",
  "version": "0.2.0",
  "publishedAt": "2026-11-15T06:30:12Z",
  "platform": "windows-x64",
  "notes": "## 0.2.0\n\n- 新增 24h 自动检查\n- 新增下载进度与断点清理",
  "packages": [
    {
      "variant": "portable",
      "kind": "portable",
      "path": "generic-updater-0.2.0-windows-x64-portable.zip",
      "size": 408,
      "sha256": "ec820637fe03c327b1c15468bb7cdf6f1565810270d3f3adbe69a0a79a46d777"
    }
  ]
}
```

- `sha256` 必填且必须是 64 位十六进制，否则直接拒绝更新；
- `size` 必填，下载时先比大小再比哈希；
- `notes` 就是主程序弹窗里要展示的"本次变更内容"，Markdown 原文透传；
- `path` 相对元数据所在目录解析，也可以给 `url` 字段写绝对直链；
- `platform` / `product` 用于一致性校验。

与 snow-shot 的差别：对方把元数据做了 RSA-PSS 签名（`payload` + `keyId` + `signature`），并带逐文件清单支持增量更新；本项目当前只做 SHA256，签名和增量是后续扩展项。

## 5. 状态帧协议

updater 往 stdout 每行写一条 JSON，主程序按行读：

```json
{"protocol":2,"type":"status","state":"Checking","version":"","notes":"","received":0,"total":0,"error":null,"automatic":true}
{"protocol":2,"type":"status","state":"Available","version":"1.2.0","notes":"## v1.2.0 更新内容…","received":0,"total":0,"error":null,"automatic":true}
{"protocol":2,"type":"status","state":"Downloading","version":"1.2.0","notes":"…","received":65536,"total":408,"error":null,"automatic":false}
{"protocol":2,"type":"status","state":"Verifying","version":"1.2.0","notes":"…","received":0,"total":0,"error":null,"automatic":false}
{"protocol":2,"type":"status","state":"Ready","version":"1.2.0","notes":"…","received":1,"total":1,"error":null,"automatic":false}
{"protocol":2,"type":"status","state":"Applying","version":"1.2.0","notes":"…","received":0,"total":0,"error":null,"automatic":false}
{"protocol":2,"type":"status","state":"Failed","version":"1.2.0","notes":"","received":0,"total":0,"error":"SHA256 不一致，下载文件已丢弃","automatic":false}
```

状态取值与 snow-shot 的 `UpdateState` 一致：`Unavailable / Idle / Checking / Available / Downloading / Verifying / Ready / Applying / Failed`。`automatic` 用来区分这次检查是自动还是手动，方便 UI 决定"静默失败"还是"弹错"。

## 6. 发布侧

`.github/workflows/build.yml` 一条龙：打 `v*` tag（或手动触发）→ 在 `windows-latest` 编译 → 打包 portable zip → 算 SHA256 → 生成 `latest-version-windows-x64.json` → 全部挂到 Release。

手工发版时照这个清单准备：

1. 改 `Cargo.toml` 的 `version`；
2. 打 tag `v<version>` 并推送；
3. 等 Actions 产出 `latest-version-windows-x64.json`、`*.zip`、`*.zip.sha256`。

## 7. 目录结构

```
├── Cargo.toml
├── config.example.json
├── demo/                       # 纯前端测试台（node server.mjs）
│   ├── index.html  app.js  styles.css  server.mjs
│   └── feed/latest-version-windows-x64.json
├── .github/workflows/build.yml
└── src/
    ├── main.rs                 # 命令行解析
    ├── config.rs               # 配置、路径、本地版本号
    ├── state.rs                # 24h 检查时间戳 + 待安装记录
    ├── feed.rs                 # 元数据获取与解析
    ├── download.rs             # 流式下载、SHA256、原子落地
    ├── install.rs              # 解压 staging、备份覆盖、回滚、清理
    ├── update.rs               # 状态机编排
    ├── protocol.rs             # 与主程序的 JSON 帧
    └── error.rs                # 统一错误类型（含可重试判定）
```

## 8. 已知边界

- **替换旧 exe 靠重试**：Windows 上运行中的 exe 会被占用，`--apply` 会对"拒绝访问"重试到 90 秒截止，取代 OpenProcess 轮询（零额外依赖）。主程序应在调用前自行退出。
- **不能自己更新自己**：`--apply` 要去覆盖的文件如果是正在运行的 updater 本身，会一直被占用。所以 `install_dir` / `exe_name` 必须指向被打包的那个 App；更新 updater 自身需要"复制一份到临时目录再执行"的做法（snow-shot 用独立的 broker/elevated 进程做这件事）。
- **没有签名**：只校验 SHA256，元数据被篡改就能推任意包。生产环境建议按 snow-shot 的做法加 RSA-PSS 或 ed25519 验签。
- **没有增量**：每次整包下载。
- **UAC**：安装目录不可写时不会自动提权。
