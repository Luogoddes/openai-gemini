# 便携形态对比：SnowShot 1.5.7 实测 vs 我们的 v0.6.0

- 日期：2026-11-16
- 背景：用户本机 `D:\Program Files\snow-shot\bin` 有一份 **SnowShot 便携版**，在同一个公司网络、同一台 Symantec 受控机器上可以正常运行。此前文档里"必须装进 Program Files、注册卸载项才安全"的结论**作废**。
- 结论前置：**SnowShot 全部 4 个 exe 都没有 Authenticode 签名**，其中 `snow-shot-mcp.exe`、`crashpad_handler.exe` **既无版本信息也无任何资源**。所以"未签名 / 无元信息"单独并不足以让 Symantec 拦截；我们挨刀是**多因素叠加**，且其中"空壳形态"占很大比重。

---

## 1. 更正声明

`docs/reviews/2026-11-16/symantec-heur-advml-b-forensics.md` 中以下内容被本文件推翻：

| 原结论 | 现状 |
|---|---|
| "改成 NSIS 安装包、装进 Program Files + 注册卸载项"是中期最高收益项 | **作废**。SnowShot 便携版就在 `D:\Program Files\snow-shot`，没有卸载注册项（目录自带 `snow-shot-installation.json` 作为安装记录），照样不被拦 |
| 暗示"没有元信息"是主要扣分项 | **部分修正**。元信息/图标是加分项（SnowShot 主程序与更新器都有），但同目录下两个无元信息的 exe 也没被拦，说明它不是充分条件 |

保留有效的结论：签名不是 SnowShot 的护身符（它也没有）；信誉/流行度、真实内容体量、分发渠道仍然是真实差异。

---

## 2. 实测：SnowShot 便携目录（本机真实文件）

### 2.1 目录结构

```text
D:\Program Files\snow-shot\
├─ snow-shot-installation.json        3,542 B   ← 安装记录（schema=1, platform=windows-x64, version=1.5.7, variant=portable）
├─ share\snow-shot\licenses\LICENSES.txt   3,900,020 B（第三方许可证，3.7 MB）
└─ bin\
   ├─ snow_shot.exe                  83,513,344 B（83.5 MB，Qt 静态链接）
   ├─ snow-shot-updater.exe           1,829,888 B（1.83 MB，Rust）
   ├─ snow-shot-mcp.exe               2,716,672 B（2.7 MB）
   ├─ crashpad_handler.exe              968,192 B（崩溃上报）
   ├─ __data_directory                        8 B（内容 "portable"）
   ├─ assets\  audios\  portable\
   └─ portable\  （64 个文件，合计 143,814,828 B ≈ 137 MB）
      ├─ config.json                      32,682 B
      ├─ capture_history\  logs\  pinned_windows_v2\
      └─ assets\  （OCR 模型、运行时等，见安装记录）
```

### 2.2 安装记录 `snow-shot-installation.json`（实测）

顶层字段：`schema / platform / version / variant / files`。
`files` 共 **19 条**，每条含 `path / size / sha256`，覆盖"程序自身拥有"的文件（不含用户数据）：

| 路径 | 大小 |
|---|---|
| `bin/snow_shot.exe` | 83,513,344 |
| `bin/snow-shot-updater.exe` | 1,829,888 |
| `bin/snow-shot-mcp.exe` | 2,716,672 |
| `bin/crashpad_handler.exe` | 968,192 |
| `bin/assets/ocr/runtimes/.../snow-ocr-process-1.2.2-windows-x64.exe` | 19,818,496 |
| `bin/assets/ocr/models/.../PP-OCRv6_det_small.onnx` | 9,929,594 |
| `bin/assets/ocr/models/.../PP-OCRv6_rec_small.onnx` | 21,234,383 |
| `bin/assets/qrcode/*` | 965,430 + 42,656 + 23,929 + 5,984 |
| `bin/assets/ocr/**/.complete.json`、`asset-manifest.json`、`runtime-manifest.json`、`ppocrv6_dict.txt` | 若干 |
| `bin/audios/camera_shutter.mp3` | 7,725 |
| `share/snow-shot/licenses/LICENSES.txt` | 3,900,020 |

**意义**：更新器不是"从网上拉 exe 覆盖"，而是按一份**逐文件 sha256 的安装记录**做所有权管理与校验；每个文件都能解释来源与用途。

### 2.3 四个 exe 的 PE 事实（实测）

| 文件 | 体积 | 版本信息 | 资源 | 签名 | 敏感导入 |
|---|---|---|---|---|---|
| `snow_shot.exe` | 83.5 MB | ✅ `Snow Apps` / `Snow Shot` / `Snow Shot screenshot utility` / `1.5.7.0` / `Copyright (C) 2025-2026 mg-chao` | ICON, GROUP_ICON, VERSION（**无 MANIFEST**） | **NotSigned** | `CreateProcessW`、`ShellExecuteExW`、`LoadLibraryW`；`WINHTTP.dll`、`WS2_32.dll` |
| `snow-shot-updater.exe` | 1.83 MB | ✅ 同上，`desc='Snow Shot update service'` | ICON, GROUP_ICON, VERSION, **MANIFEST** | **NotSigned** | `ShellExecuteExW`；`ws2_32.dll`、`crypt32.dll`、`secur32.dll`（**无 CreateProcessW**） |
| `snow-shot-mcp.exe` | 2.7 MB | ❌ 全空 | ❌ 无 | **NotSigned** | `CreateProcessW`、`GetProcAddress` |
| `crashpad_handler.exe` | 0.97 MB | ❌ 全空 | ❌ 无 | **NotSigned** | `CreateProcessW`、`LoadLibraryW`、`GetProcAddress` |

补充实测：

- `snow-shot-updater.exe` 与我们的更新器同源特征明显：Rust、1.8~2.0 MB、无签名、`ws2_32` 直连、字符串里同样含 `cmd.exe`。
- `snow_shot.exe` 是 **Qt 静态链接单体**（83 MB，导入 64 个 DLL，含 `icu.dll`、`WINHTTP.dll`、`UIAutomationCore`），不是 Electron。
- 便携版**没有**卸载注册表项，也没有 `Uninstall` 记录；只有安装根目录下的 `snow-shot-installation.json`。

### 2.4 更新器契约（从其安装记录/目录推断 + 文档实测）

- 独立进程 `snow-shot-updater.exe`，与主程序同目录；主程序调用、更新器执行。
- 自己维护安装记录（逐文件 sha256）、备份与回滚、启动探测；更新包为 online/offline 两种 zip。
- 数据目录在 `bin/portable`（由 `bin\__data_directory` 指向），配置、日志、截图历史、置顶窗口状态都在里面。

标注：以上契约细节为**目录结构 + 其公开文档**的推断；未做二进制反编译。

---

## 3. 逐项对比（我们的 v0.6.0 zip vs SnowShot 便携版）

| 维度 | 我们（0.6.0） | SnowShot 1.5.7 便携 | 是否差异来源 |
|---|---|---|---|
| 文件数 | **3**（2 exe + version.txt） | 19 个自有文件 + 64 个数据文件 | **是**（体量/完整度） |
| 总体积 | **1.16 MB** | **约 231 MB**（83.5 + 19.8 + 31 + 3.9 + 数据 137 …） | **是** |
| 主程序 | 207 KB（C++，只有窗口 + 拉更新器） | 83.5 MB（Qt，截图/OCR/贴图/快捷键/设置…） | **是** |
| 更新器 | 1.99 MB Rust | 1.83 MB Rust | 否 |
| 版本信息 | **两个都空** | 主程序 + 更新器完整；mcp/crashpad 空 | 部分是 |
| 图标 | **无** | 主程序 + 更新器有；mcp/crashpad 无 | 部分是 |
| 清单 | **无** | 更新器有（asInvoker）；主程序无 | 否（SnowShot 主程序也没有） |
| Authenticode | 无 | **全部无** | 否（决定性证据） |
| 敏感导入 | 主程序 `CreateProcessW`+`ShellExecuteW`+`CreatePipe`；更新器 `CreateProcessW`+`ShellExecuteW`+`ws2_32` | 主程序 `CreateProcessW`+`ShellExecuteExW`+`WinHTTP`；更新器仅 `ShellExecuteExW`+`ws2_32` | 部分是 |
| 安装记录 | 无（只有隐藏 `.update-files.json` 在安装目录） | 根目录 `snow-shot-installation.json`（19 文件逐条 sha256） | **是**（可解释性） |
| 数据目录 | 无（不写任何自己的数据） | `bin/portable`：config/logs/历史/模型 | **是**（行为完整度） |
| 运行位置 | 用户解压到 Downloads 后直接双击 | 用户主动放到 `D:\Program Files\snow-shot` | 部分是（MOTW/路径） |
| 分发/流行度 | 最新版个位数下载 | GitHub 约 1 万次/6 版 + Gitee/官网/winget/scoop/MSIX | **是** |

### 3.1 便携形态下，它靠什么不挨刀

按证据强度排序：

1. **它从来不是"下载壳"**：主程序 83.5 MB，带 OCR 模型（31 MB）、OCR 运行时（19.8 MB）、音效、3.9 MB 许可证、崩溃上报器、MCP 服务进程。机器学习看到的是"一个完整应用"，而不是"一个只会拉更新器的 200 KB 空壳"。这是实测事实，不依赖推断。
2. **可解释的文件所有权**：`snow-shot-installation.json` 逐文件 sha256；更新只动清单内的自有文件，用户数据（`bin/portable`）单独隔离。行为链条自洽，不需要"往 AppData 里丢 exe"。
3. **多数二进制有身份**：主程序与更新器都有完整 VERSIONINFO + 图标，更新器还带 asInvoker 清单；公司 IT 与用户都能看出"这是什么软件、哪个版本"。
4. **两个无元信息的 exe 也没被拦**：`snow-shot-mcp.exe`、`crashpad_handler.exe` 与我们一样"空元信息 + 无资源 + 未签名"，但它们由主程序在正常上下文里拉起、有明确用途、且整个目录已被大量用户使用。→ 说明 **AV 判定看的是整体画像与上下文，不是"缺元信息"这一条**。
5. **信誉**：多渠道分发 + 长期发版 + 较大装机量。这是 SnowShot 真正的护城河，也是我们短期补不上的部分。
6. **签名完全无关**：四个 exe 全部 NotSigned，实测结论。

> 需要注意：SnowShot 的 `snow_shot.exe` **没有** manifest 资源。所以"补清单"是工程正确性（DPI/UAC/Common-Controls）的收益，而不是"过 Symantec 的开关"。

---

## 4. 参考实现 `tahhans/update` 的取舍

实测：0 star、Rust、更新器 crate（`feed/install/journal/protocol/signature/update`）+ `demo-app`（egui/eframe，单文件 `main.rs` 约 14.7 KB）。其 README 明确写：更新器是独立进程，主程序只负责拉起与读 stdout。

| 做法 | 评价 | 是否采用 |
|---|---|---|
| 独立 updater 进程 + stdout JSON 行协议 | 与我们一致，边界清晰 | ✅ 已采用 |
| demo 用 egui/eframe | 能快速做出"像样"的 UI，依赖树真实、体积到 MB 级 | ⚠️ 可选；本仓库为离线可构建，改用原生 Win32 手写绑定（零新增依赖） |
| 配置放 `app.json` 随包分发 | 灵活，但客户端可改、可被替换 | ❌ 我们保持编译期固化（`updater.conf`），调试用 `--config` |
| 用 `.app-update/.launch-marker` 做启动探测 | 需要客户端配合写标记 | ❌ 我们保持"3 秒进程存活探测"，无文件副作用 |
| 拉起子进程用 `Command`（默认带控制台窗口） | GUI 父进程会闪黑框 | ⚠️ 我们对子进程隐藏控制台，但不使用隐藏窗口的常驻进程 |
| 资源/元信息 | 参考项目完全没做 | ➕ 我们补齐（超出参考） |

结论：进程模型与协议照抄；配置与探测机制不照抄；UI 技术栈按"离线可构建"取舍。

---

## 5. C++ vs Rust，以及"demo 太简单"的权重

### 5.1 语言本身：PE 实测证据

| 证据 | 说明 |
|---|---|
| SnowShot 的更新器是 **Rust**（1.83 MB、无签名、`ws2_32`、含 `cmd.exe` 字符串）却不被拦 | 说明 "Rust 编译" 不是通过 Symantec 的原因 |
| 我们的更新器也是 **Rust**（1.99 MB、无签名、`ws2_32`）却被拦 | 说明语言不是决定性变量 |
| SnowShot 的主程序是 **C++/Qt**（83.5 MB、无签名、无清单）却不被拦 | 说明 "C++ 更容易被查" 不成立 |
| 我们的 demo 是 **C++/MSVC**（207 KB，无资源）被拦 | 变量是"体积 + 资源 + 内容 + 行为"，不是编译器 |

因此：**换语言不会直接改变 ML 判分**。但换 Rust 对我们有间接收益——更容易嵌入资源、能自然引入真实依赖与真实功能、二进制不再是"200 KB 手写壳"的形态。这属于"顺带改善画像"，不是"换语言就安全"。

### 5.2 "demo 太简单"占多大份量

无法给出百分比（Symantec 模型不公开权重），但可以用对照说明它确实显著：

- 两个 exe 加起来的有效载荷只有 1.16 MB；主程序 207 KB，除窗口与拉更新器外**不产生任何自己的数据**（不写配置、不写日志、不写历史）。
- AV 的 ML 特征包含体积、资源数量、字符串丰富度、导入组合、"是否为启动器"等；一个 200 KB、无图标、无版本、只做 `CreateProcess` 的 GUI 程序，是**加载器/释放器**的经典形状。
- 对照 SnowShot：同样无签名，但主程序 83.5 MB、有真实业务、有 19 个自有文件与逐文件哈希、有用户数据目录。
- 同目录下两个"空元信息"的 SnowShot exe 没被拦，进一步说明：**单纯"小"不是罪，"小 + 空 + 拉起另一个未知 exe + 往 AppData 写 exe" 才是。**

结论：语言不是重点；**"空壳 + 启动器行为"是高权重项**，值得用 Rust 重写成"有真实内容的小应用"。补齐 VERSIONINFO/图标/清单是必要但不充分的工程卫生。

---

## 6. 据此修订的改造清单

| 优先级 | 措施 | 依据 |
|---|---|---|
| P0 | demo 用 Rust 重写，做成"有真实功能的小应用"（便签 + 设置 + 日志 + 数据目录 + 关于/许可），不再只是拉更新器 | §5.2 |
| P0 | 两个 exe 补 VERSIONINFO + 图标；更新器补清单（asInvoker / Common-Controls / DPI / longPathAware） | §2.3（主程序无清单也可，但属工程卫生） |
| P0 | 发布内容更像产品：许可证、README、设置文件、数据目录约定 | §3.1 |
| P1 | 保留"安装记录/文件清单"思路（我们已有 `.update-files.json`），并在发布包里提供可读的清单 | §2.2 |
| P1 | 缓存目录不放长期可执行文件（staging/backup 移入安装目录） | 前一份文档 §7.2，仍有效 |
| P1 | 首次运行路径提示（Downloads/Temp） | 仍有效 |
| P2 | 上架 winget/scoop、稳定发版、积累装机量 | 信誉，短期无法替代 |
| ❌ | NSIS/Program Files/卸载注册项 | **本次实测推翻** |
| ❌ | 代码签名 / 加壳 / 混淆 | 既定约束与反效果 |

---

## 附录：实测命令与数据来源

- 目录清单：`Get-ChildItem 'D:\Program Files\snow-shot' -Recurse`
- 版本信息 / 签名：`[Diagnostics.FileVersionInfo]::GetVersionInfo()`、`Get-AuthenticodeSignature`
- PE 结构/节区/导入/资源：自写解析脚本（`%TEMP%\av-forensics\pe-analyze.ps1`），解析 COFF/可选头、节区表、导入表、资源目录顶层类型
- 安装记录：`D:\Program Files\snow-shot\snow-shot-installation.json`
- 参考实现：GitHub API 读取 `tahhans/update`（README、Cargo.toml、demo-app/src）
