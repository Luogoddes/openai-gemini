# generic-updater

外置通用更新器 + 一个仿「版本更新」弹窗的 Win32 客户端。任意语言的主程序拉起
`generic-updater.exe` 就能获得：**24h 自动检查 / 手动检查 / 下载进度 / SHA256 校验 /
备份替换 / 失败回滚 / 重启拉起**。

更新契约（元数据形状、资产命名、状态机、stdout JSON 帧）参考了
[snow-shot 的 `snow-shot-updater`](https://gitee.com/mg-chao/snow-apps/tree/main/snow_shot/rust/snow-shot-updater)，
代码是独立实现（对方的 `snow_shot/`、`snow-crates/` 是 GPL-3.0，只对齐接口，不复制实现）。

## 1. 组成

| 目录 | 作用 |
|---|---|
| `src/` | Rust 更新器：检查、下载、校验、安装、回滚、拉起 |
| `app/` | C++ Win32 GUI 客户端：版本弹窗 + 进度条 + 立即安装 / 跳过此版本 / 重启软件 |
| `demo/` | 纯前端测试台，不开编译器也能把流程点一遍 |
| `.github/workflows/build.yml` | 编译两端 → 打包 zip → 出 `.sha256` 与 `latest-version-windows-x64.json` |

GUI 只做 UI 和进程协调，真正的网络与文件操作全部交给 Rust 更新器，两者用 stdout
一行一条 JSON 通信。**「重启软件」是真的重启**：客户端先拉起 `--apply`，然后自己退出，
更新器在 exe 释放后替换文件、清缓存，再以 detached 方式重新拉起新版——浏览器版做不到这一点，
所以最终形态是原生窗口。

## 2. 最快的体验方式

### 2.1 浏览器测试台（不装任何工具链）

```powershell
cd demo
node server.mjs      # 打开 http://127.0.0.1:8124/
```

自动检查 → 立即更新（真实算 SHA256）→ 重启软件（模拟备份/覆盖/清理）→ 24h 节流验证，
右侧实时打印和 Rust 版一样的 JSON 帧。

> 本机安全软件会拦截 URL 以 `.zip` 结尾的浏览器请求（回 204 空响应），所以 demo 走
> `/feed/portable` 无扩展名路由；真实更新器用 reqwest，不受影响。

### 2.2 真实链路（GitHub Actions，推荐）

1. 改 `Cargo.toml` 的 `version`，push，然后打 tag：

   ```powershell
   git tag v0.3.0; git push origin v0.3.0
   ```

2. Actions 跑完后，Release `v0.3.0` 会有三个资产：
   `generic-updater-0.3.0-windows-x64-portable.zip`、同名 `.sha256`、`latest-version-windows-x64.json`。

3. 下载 zip 解压，运行 `GuiUpdaterDemo.exe`：首次打开会自动检查一次（24h 内不会重复）。
   因为版本相同，会显示「当前已是最新版本」。

4. 想看到更新：把 `Cargo.toml` 改成 `0.3.1` 并打 `v0.3.1` tag。等 CI 出包后，
   在旧版窗口里点 **检查更新**（手动检查无视 24h 限制）→ 出现「最新版本 0.3.1（有更新）」+
   更新日志 + 包大小 → **立即安装** → 进度条走满 → **重启软件** → 窗口自动变成 0.3.1。

### 2.3 本地编译

```powershell
# Rust 更新器
cargo build --release            # target\release\generic-updater.exe

# GUI 客户端（二选一）
set MSVC_SETUP=E:\你的路径\msvc\setup_x64.bat
app\build-msvc.bat               # 产物在 dist\GuiUpdaterDemo.exe

app\build-mingw.bat              # 有全局 g++ 时可用；会在仓库根目录里用相对路径编译
```

`setup_x64.bat` 就是准备 vcvars 环境的脚本（你本地那套 Nuitka 自带 MSVC 的入口），
`build-msvc.bat` 只负责调 `cl`。

## 3. 命令行

```powershell
generic-updater.exe --check --auto app.json    # 启动时自动检查（受 24h 间隔约束）
generic-updater.exe --check app.json           # 手动检查（无视间隔）
generic-updater.exe --download app.json        # 下载 + 校验 + 解压暂存，停在 Ready
generic-updater.exe --apply app.json           # 应用：备份→覆盖→清理→拉起
generic-updater.exe --clean app.json           # 清缓存
```

退出码：`0` 无更新 / `2` 有更新·已就绪·更新成功 / `1` 失败。

## 4. 配置文件

GUI 读同目录的 `app.ini`，再翻译成更新器要的 `updater.json`：

```ini
repo=Luogoddes/openai-gemini
feed_name=latest-version-windows-x64.json
feed_url=                       # 留空走 Release；本地测试可填 json 路径或直链
exe_name=GuiUpdaterDemo.exe
homepage=https://github.com/Luogoddes/openai-gemini
variant=portable
platform=windows-x64
interval_hours=24
```

更新器侧的 `updater.json` 字段：

| 字段 | 含义 |
|---|---|
| `repo` | Release 所在仓库 `owner/name` |
| `feed_name` | 元数据资产名 |
| `feed_url` | 元数据直链或本地路径（覆盖 Release 查找） |
| `api_base` | 默认 `https://api.github.com` |
| `version_file` | 本地版本号文件，一行 `1.2.3` |
| `install_dir` | 安装根目录 |
| `exe_name` | 安装后要拉起的程序 |
| `variant` / `platform` | 选包与平台校验 |
| `automatic_check_interval_hours` | 自动检查最小间隔，默认 24 |
| `state_file` | 可选，默认 `<install_dir>/.app-update/state.json` |

## 5. 元数据（feed）规范

| 资产 | 说明 |
|---|---|
| `latest-version-windows-x64.json` | Windows x64 元数据 |
| `<product>-<version>-windows-x64-portable.zip` | 便携包 |
| `<product>-<version>-windows-x64-portable.zip.sha256` | 旁置校验，`<hash>  <文件名>` |

```json
{
  "schema": 1,
  "product": "generic-updater",
  "version": "0.3.0",
  "publishedAt": "2026-11-15T09:20:00Z",
  "platform": "windows-x64",
  "notes": "- 新增：版本更新窗口\n- 新增：下载进度与 SHA256 校验",
  "packages": [
    {
      "variant": "portable",
      "kind": "portable",
      "path": "generic-updater-0.3.0-windows-x64-portable.zip",
      "size": 1180275,
      "sha256": "b0b7…"
    }
  ]
}
```

- `sha256` 必填且 64 位十六进制，否则拒绝更新；`size` 必填，先比大小再比哈希；
- `notes` 就是弹窗里的「更新日志」，Markdown 原文透传；
- `path` 相对元数据所在目录解析，也可用 `url` 写绝对直链；
- 多平台再加 `latest-version-windows-arm64.json` 等，客户端只认自己那份。

## 6. 状态帧协议

stdout 一行一条 JSON，GUI 按行解析：

```json
{"protocol":2,"type":"status","state":"Checking","version":"","notes":"","received":0,"total":0,"error":null,"automatic":true}
{"protocol":2,"type":"status","state":"Available","version":"0.3.1","notes":"…","received":0,"total":1180275,"error":null,"automatic":false}
{"protocol":2,"type":"status","state":"Downloading","version":"0.3.1","notes":"…","received":65536,"total":1180275,"error":null,"automatic":false}
{"protocol":2,"type":"status","state":"Verifying","version":"0.3.1","notes":"…","received":0,"total":0,"error":null,"automatic":false}
{"protocol":2,"type":"status","state":"Ready","version":"0.3.1","notes":"…","received":1,"total":1,"error":null,"automatic":false}
{"protocol":2,"type":"status","state":"Applying","version":"0.3.1","notes":"…","received":0,"total":0,"error":null,"automatic":false}
{"protocol":2,"type":"status","state":"Failed","version":"0.3.1","notes":"","received":0,"total":0,"error":"SHA256 不一致，下载文件已丢弃","automatic":false}
```

状态与 snow-shot 的 `UpdateState` 一致：`Unavailable / Idle / Checking / Available /
Downloading / Verifying / Ready / Applying / Failed`。`automatic` 用来区分自动/手动检查，
方便 UI 决定静默失败还是弹错。

## 7. 目录结构

```
├── Cargo.toml
├── config.example.json
├── app/
│   ├── src/gui.cpp          # Win32 界面 + 进程协调
│   ├── app.ini              # 客户端配置
│   ├── build-msvc.bat
│   └── build-mingw.bat
├── demo/                    # 浏览器测试台
├── .github/workflows/build.yml
└── src/
    ├── main.rs              # 命令行
    ├── config.rs            # 配置与本地版本号
    ├── state.rs             # 24h 时间戳 + 待安装记录
    ├── feed.rs              # 元数据获取与解析
    ├── download.rs          # 流式下载、SHA256、原子落地
    ├── install.rs           # 解压、备份覆盖、回滚、清理
    ├── update.rs            # 状态机编排
    ├── protocol.rs          # JSON 帧
    └── error.rs
```

## 8. 已知边界

- **不能更新自己**：`--apply` 会跳过正在运行的 `generic-updater.exe`（Windows 锁镜像）。
  客户端自身升级要「复制到临时目录再由副本替换」，snow-shot 用 broker/elevated 做的就是这个。
- **没有签名**：只校验 SHA256，元数据被篡改就能推任意包；snow-shot 用 RSA-PSS 信封防护。
- **没有增量**：整包下载。对方的 `files` 清单可以做逐文件增量。
- **占用重试**：运行中的 exe 靠「拒绝访问重试 90 秒」处理，不是 OpenProcess 精确等待。
- **UAC**：安装目录不可写时不会自动提权。
