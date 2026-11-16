# Symantec Heur.AdvML.B 拦截取证与改造方案

> **更正（2026-11-16 晚）**：本文第 5 节与 §7.2 第 7 条关于"必须装进 Program Files、注册卸载项"的结论**已被推翻**。
> 用户本机实测 SnowShot 便携版（`D:\Program Files\snow-shot`，无卸载注册项、四个 exe 全部未签名，其中两个无任何元信息）在同一公司网络可正常运行。
> 最新对比与修订后的改造清单见 [snowshot-portable-vs-ours.md](snowshot-portable-vs-ours.md)。

- 日期：2026-11-16
- 对象：`generic-updater` v0.6.0 发行包（`generic-updater-0.6.0-windows-x64-portable.zip`）
- 场景：包在公司网络解压后运行 `GuiUpdaterDemo.exe`，被 Symantec Endpoint Protection 判为 `Heur.AdvML.B`
- 对照：`mg-chao/snow-shot`（SnowShot）同为公司网络可运行

---

## 0. 结论摘要

**不是更新逻辑里有"病毒动作"，而是产物在 AV 眼里是「零元信息 + 零信誉 + 未签名 + 自带下载/自我替换/拉起」的组合。** Symantec 的机器学习启发式（AdvML）对这种"未知的自我更新下载器"打分越过阈值，于是在执行时拦截。

三个可验证的事实：

1. 两个 exe 都**没有版本信息、没有清单、没有图标资源、没有 Authenticode 签名**（资源目录实测为空）。
2. 首次运行的完整链路是：从解压目录（通常是 Downloads）运行未知 exe → 30 秒后拉起同目录第二个未知 exe → HTTPS 拉 feed → 下载 zip 写进 `%LOCALAPPDATA%` → 解出可执行文件 → 把运行中的更新器改名 `.old-<pid>` 再覆盖 → 以 DETACHED 方式拉起新进程。
3. SnowShot 的更新架构与我们几乎一样（独立 Rust 更新器、RSA-3072 签名 feed、journal/backup/探测/回滚、GitHub+Gitee 双通道），**它也没做 Authenticode**——它自己的文档就写着"发布的 beta 安装包未签名，可能触发 SmartScreen"。差别在**安装形态（NSIS 装到 Program Files + 卸载注册）+ 体量/流行度信誉 + 软件源分发**。

所以"对标 SnowShot"在更新协议层是对的，但在**发布形态与信誉**这两层没有对标。

---

## 1. 取证范围与方法

### 1.1 实测（本次真正跑过的）

| 项 | 方法 |
|---|---|
| 发行包真实性 | 经 GitHub REST API 下载 v0.6.0 的 zip（1,219,177 B），解压 |
| PE 静态特征 | 自写 PowerShell 解析器：COFF/可选头、节区表、节区熵、导入表（DLL + 函数名）、资源目录顶层类型、overlay、Authenticode 数据目录、字符串标记 |
| 版本信息 | `[Diagnostics.FileVersionInfo]::GetVersionInfo()` 逐字段读取 |
| 签名状态 | `Get-AuthenticodeSignature` |
| 行为序列 | 读源码：`src/main.rs`、`src/update.rs`、`src/install.rs`、`src/journal.rs`、`src/lock.rs`、`src/config.rs`、`src/platform.rs`、`client/src/gui.cpp`、`updater.conf` |
| SnowShot 事实 | GitHub REST API（repo/ releases / 资产名 / 大小 / 下载计数）+ 其仓库内 `README.zh-CN.md`、`docs/snow-shot-releases.md`、`scripts/*` |

### 1.2 未能实测（明确标注为推断）

- **SnowShot 的二进制没有拿到**：本机对 `release-assets.githubusercontent.com` 的连接被阻断（两次下载均得到 0 字节），因此其 PE 元信息、图标、清单、导入表**全部为间接推断**（依据其打包链、发行结构、公开文档，而非逆向）。
- **Symantec 的实际判定不可见**：公司机器上没装 SEP 控制台，拿不到 Detection 详情（触发组件、策略名、云信誉值）。`Heur.AdvML.B` 的引擎行为解释来自公开资料与同类样本的通行认知，属于**合理推断**。
- 本机无 Symantec 客户端，**无法复现拦截**；本次只做"特征侧"取证。

---

## 2. 产物静态分析（实测）

发行包解压后只有三个文件，没有安装器、没有卸载注册、没有清单文件：

```text
generic-updater-0.6.0-windows-x64-portable.zip  (1,219,177 B)
├─ generic-updater.exe   2,037,248 B
├─ GuiUpdaterDemo.exe      207,872 B
└─ version.txt                   7 B
```

### 2.1 主表

| 特征 | GuiUpdaterDemo.exe | generic-updater.exe |
|---|---|---|
| 体积 | 207,872 B（203 KB） | 2,037,248 B（1.99 MB） |
| 架构 / 子系统 | AMD64 / `WINDOWS_GUI`(2) | AMD64 / `WINDOWS_GUI`(2) |
| 链接器 | 14.51（MSVC 2022） | 14.51（Rust + MSVC 链接） |
| **版本信息** | **全空**（公司/产品/描述/版本/版权/原始文件名均为 `''`） | **全空** |
| **清单（manifest）** | **无**（`requestedExecutionLevel`/`Common-Controls`/`dpiAware` 一个都没有） | **无** |
| **图标** | **无**（资源目录为空） | **无** |
| 资源目录顶层类型 | 空 | 空 |
| **Authenticode** | **NotSigned** | **NotSigned** |
| 节区 | `.text .rdata .data .pdata .fptable .reloc` | `.text .rdata .data .pdata .reloc` |
| 节区熵 | 6.47 / 5.21 / 2.04 / 5.07 / 0 / 5.01 | 6.40 / **6.91** / 1.01 / 6.07 / 5.42 |
| overlay | 0 | 0 |
| 导入 DLL 数 | 5（COMCTL32、GDI32、KERNEL32、SHELL32、USER32） | 15（含 `ws2_32`、`ntdll`、`bcrypt`、`VCRUNTIME140`、api-ms-win-crt-*） |

### 2.2 敏感导入（实测）

| 关注函数 | Demo | Updater |
|---|---|---|
| `CreateProcessW` | ✅ | ✅ |
| `ShellExecuteW`（含 `runas` 提权路径） | ✅ | ✅ |
| `CreatePipe` | ✅ | ❌ |
| `CreateMutexW` | ✅ | ❌（用 `CreateMutexA` 做锁） |
| `DeleteFileW` / `MoveFileExW` | ✅ / ❌ | ✅ / ✅ |
| `CopyFileExW` | ❌ | ✅ |
| `GetProcAddress` / `LoadLibraryExW` | ✅ / ✅ | ✅ / ✅ |
| `URLDownloadToFileW` | ❌ | ❌ |
| `WinINet` / `WinHTTP` | ❌ | ❌（直接用 `ws2_32` socket） |
| `WriteProcessMemory` / `CreateRemoteThread` | ❌ | ❌ |
| `OpenProcess` / `SetFileAttributesW` | ❌ | ✅ / ✅ |

**这一点很重要：我们没有任何注入、进程镂空、远程线程、`URLDownloadToFile` 这类"具体恶意 API"。** 被拦不是因为命中了某个恶意函数，而是整体画像。

### 2.3 其他实测观察

- `generic-updater.exe` 字符串里命中 `cmd.exe`，但源码中没有任何 `cmd` 调用（`src/` 全量检索无结果）。判断来自依赖或工具链常量，**未逐字节定位，标注为推断**。
- Demo 的 `.fptable` 是非常规节区（MSVC 链接器产物），本身不表示恶意，但会让"节区布局"偏离常见样本，对 ML 特征而言是陌生度加分项。
- Demo 导入了 `LoadIconW` 却**没有 `RT_ICON` 资源**——运行时只能落到系统默认图标；配合 `MessageBoxW` + `SetTimer`（30 秒自动检查），整体观感更像"内部测试程序"而不是可发布产品。
- Demo 导入 `IsDebuggerPresent`，两个 exe 都导入 `FlsAlloc`/`FlsSetValue` 等——这些都是 MSVC CRT / Rust 运行时的标准样板导入，本身不代表反调试。
- 两个 exe 都是 GUI 子系统；更新器由父进程以管道拉起、自身不显示窗口，这一点在 AV 看来接近"无界面下载器"。

---

## 3. 首次运行行为序列（源码实测 + AV 视角）

以下路径按"用户把 zip 解压到 Downloads 下的文件夹并双击 Demo"还原。

| # | 进程 | 动作 | 落盘 / 网络 | AV 视角 |
|---|---|---|---|---|
| 1 | 用户 | 双击 `Downloads\...\GuiUpdaterDemo.exe` | 文件带 Mark-of-the-Web（浏览器下载的 zip 解压后传播到子文件） | 来源未知 + 下载目录执行 |
| 2 | Demo | `CreateMutexW` 单实例、读 `version.txt`、建窗口 | 无 | 正常启动 |
| 3 | Demo | 30 秒后 `CreatePipe` + `CreateProcessW` 拉起 `generic-updater.exe --check --auto`（同目录、stdout 接管道） | 子进程 | **一个未知 exe 静默拉起另一个未知 exe** |
| 4 | Updater | 抢锁 `%LOCALAPPDATA%\GenericUpdater\updater.lock`（`create_new` 原子创建）、读 `state.json`、HTTPS 拉签名 feed | 网络：`ws2_32` 直连 | **无 UI 的 GUI 程序主动外联** |
| 5 | Updater | 有更新时输出 JSON 帧，Demo 弹对话框 | 无 | 正常 |
| 6 | Updater | 用户点更新 → `--download`：流式写 `.part` → 原子改名 zip → SHA256 → 解压到 `staging/` | `%LOCALAPPDATA%\GenericUpdater\updates\<hash>\`：`.part`、`zip`、`staging\` 里的 exe | **下载 → 在 AppData 隐藏目录落可执行文件** |
| 7 | Updater | 用户点重启 → `--apply --parent-pid <pid>`：等父进程退出 → 写 `journal.json` → 备份到 `backup/` | 同上 + `journal.json` | 事务写盘，正常 |
| 8 | Updater | 把运行中的自己改名 `generic-updater.exe.old-<pid>`，再写入新镜像 | 安装目录（即解压目录） | **自我替换 / 自改名** |
| 9 | Updater | 覆盖安装目录、旧文件移入 `backup\__deleted__\`、重写 `.update-files.json`（隐藏清单） | 安装目录 | **批量改写自身所在目录 + 隐藏清单** |
| 10 | Updater | `CreateProcessW`（`DETACHED_PROCESS｜CREATE_NEW_PROCESS_GROUP`）拉起新版，3 秒探测存活 | 子进程 | **落盘后立即执行** |
| 11 | 失败路径 | journal 还原（backup 拷回、被删文件放回、`version.txt` 回退），拉起旧版，`exit 1` | 安装目录 | 与"释放器"行为相似，进一步加分 |

**AV 打分最集中的是第 3、4、6、8、10 步**：无元信息 exe → 拉起第二个无元信息 exe → 外联 → 在 AppData 落 exe → 自我替换 → 立刻执行。这条链与"下载器/释放器"的典型行为高度重合。

---

## 4. 为什么是 `Heur.AdvML.B`

`Heur.AdvML.*` 是 Symantec 的机器学习启发式族（不是特征串命中，也不是行为拦截后的 SONAR 命名）。它把 PE 结构、导入、字符串、熵、元信息、执行上下文等喂给模型打分，分数越过策略阈值就报。

我们的产物踩中的特征：

| 特征 | 我们 | 正常商业软件 |
|---|---|---|
| 公司 / 产品 / 版权 | 空 | 完整 |
| 文件版本 | 空 | 完整 |
| 图标 | 无 | 有 |
| 清单（UAC 级别、DPI、Common-Controls） | 无 | 有 |
| Authenticode | 无 | 通常有（我们没有） |
| 首次出现时间 / 云端流行度 | 几分钟前的新哈希 / 0 | 长期积累 |
| 行为组合 | 外联 + 写 AppData + 落 exe + 自改名 + DETACHED 拉起 | 安装器写 Program Files |
| 运行位置 | 解压目录（常为 Downloads） | Program Files |
| 体积/熵观感 | 2 MB 静态链接 Rust，`.rdata` 熵 6.91 | 大体积、多资源、多语言 |

注意：**"没签名"不是唯一原因**，但它是"没有身份 + 没有信誉"的放大器。模型无法确认"这是谁发的"，于是只能靠行为画像判断，而我们的行为画像恰好是最敏感的那一类。

---

## 5. 与 SnowShot 的对比

### 5.1 实测事实（GitHub API + 其仓库文档）

| 项目 | 数据 |
|---|---|
| 仓库 | `mg-chao/snow-shot`，C++/Qt + Rust（CMake），7,145 stars / 463 forks |
| 创建时间 | 2025-02-26；最近推送 2026-11-16 |
| 发版节奏 | v1.5.2 → 1.5.8 在 8 天内（11-08 ~ 11-15），每版 62 个资产 |
| 分发渠道 | GitHub + Gitee + winget + scoop + homebrew + MSIX（Store）+ 官网 |
| 安装形态 | NSIS 安装包（offline/online）、便携 zip、MSIX；注册卸载项、保留"安装记录" |
| 更新器 | 独立 Rust 程序 `snow-shot-updater.exe`（有体积基线脚本 `report-snow-shot-updater-size.ps1`） |
| 更新协议 | 签名 JSON envelope（RSA-3072 / SHA-256 / SemVer）、GitHub 与 Gitee 双通道竞速、断点续传绑定 URL |
| 事务模型 | journal + backup + 启动探测 + 恢复（`docs/snow-shot-releases.md`：U3 契约） |
| 签名态度 | **明确未做 Authenticode**。文档原文：发布的 beta 安装包未签名，可能触发 SmartScreen 提示；"Manifest authenticity is separate from Windows Authenticode. This feature does not add an Authenticode certificate, remove SmartScreen prompts, or establish installer reputation." |

GitHub 下载计数（v1.5.8 部分资产）：`x64-offline.exe` 362、`x64-online.exe` 228、`x64-portable.zip` 211。按 API 返回的 6 个版本粗算，Windows 主力资产合计约 **9,000~10,000 次**（仅 GitHub；未含 Gitee / 官网 / winget / scoop / Store 渠道）。

### 5.2 结论：同构，但不同"身份"

| 维度 | SnowShot | 我们 | 是否差异来源 |
|---|---|---|---|
| 独立更新器 | 有 | 有 | 否 |
| 签名 feed（RSA）/ 校验 | 有 | 有 | 否 |
| journal + backup + 回滚 | 有 | 有 | 否 |
| 断点续传 | 有 | 有 | 否 |
| Authenticode | **无** | 无 | 否 |
| 安装形态 | NSIS 装到 Program Files + 卸载注册 + 快捷方式 | 解压即运行，无安装器、无卸载项 | **是** |
| 版本信息 / 图标 / 清单 | 有（Qt/NSIS 打包链默认产出）——推断 | **无（实测）** | **是** |
| 流行度 / 安装基数 | GitHub 单渠道约 1 万次/6 版，另有 Gitee/官网/winget/scoop（量级更高，未实测） | 个位数下载 | **是** |
| 分发渠道可信度 | winget/scoop/homebrew/MSIX 审核 + 官网/Gitee | 仅 GitHub Release，链接直发 | **是** |

一句话：SnowShot 用"正规安装的软件"的样子被大规模使用，信誉早已建立；我们目前是"匿名 exe 在下载目录里自我替换"，模型只能按可疑打分。

> 标注：SnowShot 的 PE 元信息/图标/清单**未实测**（下载被网络阻断），上表对应行为推断；其余（发版、渠道、更新协议、未签名）为其实测公开信息。

---

## 6. 企业网络这一层：为什么家里没事、公司被拦

### 6.1 SEP 的判定链路（推断，基于公开机制）

1. **Download Insight / 文件信誉**：文件从浏览器或解压落地时，按哈希查云端（Symantec Global Intelligence Network）。新哈希 = 无记录 = 低信誉。
2. **Auto-Protect 静态扫描**：读文件时跑特征 + 启发式（Bloodhound / AdvML）。
3. **SONAR**：运行时行为评分（写可执行文件、创建进程、自替换等）。
4. **策略决策**：命中后按管理端策略执行——隔离、删除、仅记录、或提示用户。

`Heur.AdvML.B` 属于第 2 步的 ML 启发式，可在**执行瞬间**触发，不需要等行为发生。

### 6.2 为什么同一份文件在家里可能没事

- **策略不同**：消费版 Norton 与企业版 SEP 的默认启发式级别不同；企业管理员常开"未知/高风险文件拦截"。
- **MOTW 传播**：浏览器下载的 zip 带 `Zone.Identifier`，Windows 解压会给子文件打标记；从 Downloads 首次执行会触发更严格的信誉检查。家里的副本可能已经被"放行并记住"。
- **云查询可达性**：公司代理/DNS 可能让云查询失败或走不同区域，失败时策略常按"未知"处理（更保守）。
- **首次 vs 二次**：同一哈希第一次被拦后，家里再运行也可能被拦；反过来，家里若从未触发扫描，看起来就"没事"。
- **分发差异**：SnowShot 的安装包在 winget/scoop/Store 渠道被大量校验、下载、安装，哈希早已"预热"。

### 6.3 企业里可用的白名单路径

| 方式 | 可行性 | 说明 |
|---|---|---|
| 提交误报（False Positive） | 高 | Symantec Security Response / SEP 控制台提交样本与哈希，等待重新评级（通常 1~3 天，可多次） |
| 控制台按 **SHA-256** 加例外 | 高 | **我们没有签名，不能按"发布者"加白**，只能按哈希或路径 |
| 按目录/进程例外 | 中 | 例如只放行 `%ProgramFiles%\<Product>\*`，范围要尽量窄 |
| 应用控制白名单 | 中 | 企业有 App Control 时按哈希入库 |
| 让 IT 临时关闭启发式 | 低 | 通常不允许，且不应作为方案 |

**建议随每个 Release 提供 SHA-256 清单**（我们已经生成 `.sha256`），方便企业 IT 直接加白。

---

## 7. 改造方案（按收益 / 代价排序）

前提约束（不违背既定决策）：**不做代码签名；不做混淆/加壳/反调试；不关闭或绕过任何 AV；不放松现有校验。**

### 7.1 立即能做（收益高、代价低，1~2 天内）

| # | 措施 | 收益 | 代价 |
|---|---|---|---|
| 1 | **给两个 exe 加 VERSIONINFO**：`CompanyName`、`ProductName`、`FileDescription`、`FileVersion`、`ProductVersion`、`LegalCopyright`、`OriginalFilename`、`InternalName` | 极高：直接消除"匿名二进制"这一最大扣分项 | 低：C++ 加 `.rc`；Rust 用 `embed-resource`/`winres` 在 build.rs 嵌入 |
| 2 | **加应用图标 + 清单**：图标（ICO，含多尺寸）；清单声明 `asInvoker`、Common-Controls v6、`PerMonitorV2` DPI、`longPathAware` | 高：回归"正常桌面软件"画像 | 低：`.rc` + `/MANIFEST`；Rust 同样走 `.rc` |
| 3 | **统一产品身份**：exe 名、窗口标题、`ProductName`、`homepage`、说明文案与仓库一致；去掉 `GuiUpdaterDemo` 这类"测试样本"命名 | 中：可归属、可检索；名字本身也是模型特征 | 低：改名 + 配置 |
| 4 | **发布说明附 SHA-256**（已有 `.sha256`，需在 Release 正文中显式列出） | 中：企业加白与用户校验都靠它 | 低：CI 模板 + 文档 |
| 5 | **避免从下载目录运行**：首次启动检测自身路径，若在 `Downloads`/`Temp`/压缩包临时目录，提示"请移动到固定目录或使用安装包" | 中：切断 MOTW + 下载目录执行组合 | 低：路径判断 + 提示 |
| 6 | **发版后提交误报申诉**（Symantec + Microsoft Defender），附哈希、下载页、行为说明 | 中：企业网络里最直接的解封路径 | 低：每次发版几分钟 |

### 7.2 中期（收益高、代价中，1~2 周）

| # | 措施 | 收益 | 代价 |
|---|---|---|---|
| 7 | **改成安装包形态（NSIS 或 MSIX）**：装到 `%ProgramFiles%\<Product>` 或 `%LOCALAPPDATA%\Programs\<Product>`，写卸载注册表项、开始菜单/桌面快捷方式；更新发生在安装目录 | 极高：这是与 SnowShot 最大的形态差异，直接改变整体行为画像 | 中：打包脚本、UAC 处理、卸载逻辑、CI 产物结构都要改 |
| 8 | **缓存目录只放数据，不放长期可执行文件**：`zip`/`.part`/`journal`/`state` 留在 `%LOCALAPPDATA%`；`staging/`、`backup/` 移到安装目录下的隐藏子目录（不可写时用 `%ProgramData%\<Product>\staging`） | 高：消除"AppData 里出现 exe" | 中：`config.rs` 路径、journal 记录、回滚、`--clean` 都要跟着改并回归测试 |
| 9 | **去掉"运行中自改名"**：更新器放到 `<install>\updater\` 独占目录，采用双槽（`updater-a.exe` / `updater-b.exe` 交替），或由主程序在下次启动时替换未运行的更新器 | 中-高：删掉 `.old-<pid>` 这一条典型自替换特征 | 中-高：需要改安装/自更新流程与锁语义 |
| 10 | **网络层用 WinHTTP（或 Rust 侧 winhttp 后端）**，显式 User-Agent，遵循系统代理 | 中：更像常规软件；同时修复公司代理下更新失败 | 低-中：替换 HTTP 客户端，回归 206/超时 |
| 11 | **上架 winget / scoop**（安装包 + 清单 + 哈希），官网/Gitee 镜像 | 中-高：多一个"被审核过"的分发渠道，信誉累积更快 | 中：清单维护、版本自动化 |
| 12 | **降低"无界面下载器"观感**：更新器提供可见的进度窗口（或复用主程序 UI），不要"GUI 子系统但永不显示窗口" | 中：行为更像正规更新器 | 低-中：UI 工作量 |

### 7.3 长期（只能靠时间和体量）

| # | 措施 | 说明 |
|---|---|---|
| 13 | **稳定发布节奏**：固定产品名与文件命名，避免一天多版、频繁改哈希 | 信誉需要时间沉淀；频繁变更等于每次从零开始 |
| 14 | **积累安装基数**：winget/scoop/Store/官网多渠道分发 | SnowShot 过关的根本原因是"很多机器都装过且没出事" |
| 15 | **可归属身份**：正式官网（HTTPS + 备案）、隐私政策、README、Issue 模板、发布说明规范 | 让安全厂商和 IT 能查到"这是谁做的" |
| 16 | **企业白名单资料包**：哈希清单、行为说明（下载→校验→事务替换→回滚）、目录约定 | 企业 IT 审核时最需要的东西 |

### 7.4 明确不做（会加重误报或违背约束）

| 项 | 原因 |
|---|---|
| 代码签名 | 既定产品决策不做；本文所有方案均不依赖它 |
| UPX / 加壳 / 混淆 / 反调试 | 直接抬升熵与"可疑度"，是 AdvML 的高权重特征 |
| 关闭 AV / 加白脚本 / 静默绕过 | 不可接受 |
| 自解压 EXE / 临时目录落 exe 再执行 | 正是当前被打分的行为 |
| 隐藏窗口子进程（`CREATE_NO_WINDOW` 常驻） | v0.5.6 已移除，不要回退 |

---

## 8. 改造后的验证清单

改完后按这个顺序自测，能确认"画像"确实变了：

1. **元信息**：`(Get-Item .\GuiUpdaterDemo.exe).VersionInfo` 每个字段非空；`Get-AuthenticodeSignature` 仍为 NotSigned（符合预期）。
2. **资源**：用本文附录的解析脚本复跑，`RESOURCES` 应包含 `ICON`、`GROUP_ICON`、`VERSION`、`MANIFEST`。
3. **清单**：`requestedExecutionLevel` 出现且为 `asInvoker`；`Microsoft.Windows.Common-Controls` 存在。
4. **安装形态**：卸载注册表项存在（`HKLM\...\Uninstall\<Product>` 或 HKCU 对应项）；安装目录不在 `Downloads`/`Temp`。
5. **缓存布局**：`%LOCALAPPDATA%\GenericUpdater` 下不再出现 `staging\*.exe` 或 `backup\*.exe`（按 7.2 第 8 条改造后）。
6. **MOTW 模拟**：从浏览器下载 zip → 解压 → `Get-Item -Stream Zone.Identifier` 能看到标记；确认程序会提示移动到固定目录。
7. **行为回归**：正常更新 / 中途取消续传 / 启动失败回滚 三条路径全部重跑（对应现有 e2e 场景 A/B/C 的 Windows 实机版本）。
8. **企业侧**：把新哈希提交 SEP 误报申诉 / 加例外，观察 24~72 小时后的信誉与拦截结果。

---

## 附录 A：原始测量数据（实测）

### A.1 `GuiUpdaterDemo.exe`

```text
SIZE 207,872 B   PE32+ AMD64   subsystem=WINDOWS_GUI   sections=6   unsigned
SECTIONS  .text(6.47) .rdata(5.21) .data(2.04) .pdata(5.07) .fptable(0) .reloc(5.01)
RESOURCES (empty)          MANIFEST MARKERS: NONE
VERSION INFO: all fields empty
IMPORTS: COMCTL32.dll, GDI32.dll, KERNEL32.dll, SHELL32.dll, USER32.dll
SENSITIVE: CreateProcessW, GetProcAddress, ShellExecuteW
KERNEL32 extras: CreatePipe, CreateMutexW, DeleteFileW, TerminateProcess, WriteFile
STRING MARKERS: MZ | This program cannot be run in DOS mode
```

### A.2 `generic-updater.exe`

```text
SIZE 2,037,248 B   PE32+ AMD64   subsystem=WINDOWS_GUI   sections=5   unsigned
SECTIONS  .text(6.40) .rdata(6.91) .data(1.01) .pdata(6.07) .reloc(5.42)
RESOURCES (empty)          MANIFEST MARKERS: NONE
VERSION INFO: all fields empty
IMPORTS(15): ADVAPI32, api-ms-win-crt-*, bcrypt, bcryptprimitives, KERNEL32,
             ntdll, SHELL32, VCRUNTIME140, ws2_32
SENSITIVE: CreateProcessW, GetProcAddress, ShellExecuteW
KERNEL32 extras: CopyFileExW, MoveFileExW, OpenProcess, SetFileAttributesW,
                 GetFinalPathNameByHandleW, GetEnvironmentVariableW
ws2_32: connect/recv/send/setsockopt/WSAStartup/WSAIoctl ...
ntdll: NtCreateFile, NtOpenFile, NtReadFile, NtWriteFile, NtCreateNamedPipeFile
STRING MARKERS: cmd.exe (源码无 cmd 调用，推断来自依赖) | MZ | DOS stub
```

### A.3 发行包结构

```text
generic-updater.exe   2,037,248 B
GuiUpdaterDemo.exe      207,872 B
version.txt                   7 B
（无安装器、无卸载脚本、无 .manifest 外部文件、无图标文件）
```

### A.4 SnowShot（实测公开信息）

```text
repo: mg-chao/snow-shot   stars 7,145   forks 463   created 2025-02-26
latest: v1.5.8 (2026-11-15)  assets 62
x64 offline.exe 59,338,999 B dl=362 | online.exe 25,261,942 B dl=228
x64 portable.zip 75,647,580 B dl=211 | 六版累计约 1.5 万（仅 GitHub）
updater: snow-shot-updater.exe（Rust），RSA-3072 签名 feed，journal/backup/probe/recover
windows 包：NSIS offline/online + 便携 zip + MSIX；winget/scoop/homebrew 清单
签名立场：未做 Authenticode；文档明示 beta 安装包可能触发 SmartScreen
```

---

## 附录 B：不确定项与后续取证建议

| 项 | 状态 | 建议 |
|---|---|---|
| SnowShot 二进制的 PE 元信息 | **未实测**（下载被网络阻断） | 在公司网络之外的机器上下载 `snow-shot-1.5.8-windows-x64-portable.zip`，用同一脚本跑一遍，确认 VERSIONINFO/图标/清单 |
| `cmd.exe` 字符串来源 | **未定位** | 在 `target/release/generic-updater.exe` 上做字符串偏移回溯，确认是哪个依赖引入；若可用 `dumpbin /imports` 交叉验证 |
| SEP 的具体触发组件与策略 | **不可见** | 让 IT 在 SEP 控制台导出该事件的 Detection 详情（组件名、策略名、信誉分、动作） |
| `Heur.AdvML.B` 的权重细节 | **公开资料推断** | 只能通过"改造后重新提交申诉"的结果反推有效性 |
| 公司代理是否影响云查询 | **未测** | 在断网/代理环境下对比扫描结果，判断是否"云查询失败→按未知处理" |
