# 桌面应用「软件更新机制」对照报告

- 调研日期：2026-11-15
- 调研对象：Tauri、Electron（electron-updater / Squirrel）、VS Code、Chrome（Omaha）、Firefox（Balrog/AUS），以及 Obsidian、Notion、Zed 等典型桌面应用
- 审阅对象：`e:\code\脚手架\openai-gemini` 当前更新器实现与配置（只读审阅，未改动代码）
- 证据标记：**[D]** 有官方文档/规范支撑；**[S]** 有源码支撑；**[I]** 经验推断（文档未明说，基于实现或行业惯例）
- 说明：文末「来源」列出可核查链接；正文中的 `[D1]` 等编号与来源表一一对应。

---

## 0. 结论摘要（TL;DR）

1. **业界已经形成的事实标准只有四件事**：HTTPS 拉取元数据、元数据/安装包做密码学校验、版本比较后「静默下载 + 用户确认重启」、安装前退出应用并保留可回退的旧版本。其余大量能力（差分、灰度、断点续传、常驻更新服务、遥测）属于「成熟大厂做法」，不是小型桌面应用的入场券。
2. **Tauri 的官方基线很轻**：`tauri-plugin-updater` 负责「读清单 → 校验 minisign 签名 → 下载全量包 → 调安装器」，没有内置 channel、灰度、差分、断点续传、遥测；这些都要服务端或应用自己补。严格说 Tauri v2 没有第一方托管的「tauri-updater 服务」，官方推荐自建动态服务或使用合作伙伴 CrabNebula Cloud。[D1][D4]
3. **Electron 生态的事实标准是 electron-updater（electron-builder）**，不是 Electron 内置的 Squirrel。它提供 `latest.yml` + sha512、NSIS blockmap 差分、`stagingPercentage` 灰度、channel、代码签名校验；但遥测仍要应用自己做。[D5][D6][D7]
4. **Chrome / Firefox 是「重型更新系统」的两个极端样本**：Omaha 用 cohort 做分数灰度、用 CUP 抗 TLS 被攻破、用 pingback 统计成功率；Balrog 用规则 + 百分比节流、partial MAR + 完整 MAR 兜底、BITS 断点续传、Mozilla Maintenance Service 提权。两者都把「更新」当成持续运营系统，而不是一个下载按钮。[D12][D17][D18]
5. **本项目当前的校验模型与主流同级**：签名清单 + SHA256 + journal 备份/断电恢复 + UAC 兜底 + 缓存外置，方向正确。按既定前提（通用更新器、不复用 Tauri 官方 updater、不发布代码签名），这套安全基线并不落伍——个人开源桌面项目不做 Authenticode 很常见（参考 SnowShot）。真正的短板是**发布控制**与**无签名条件下的误报运营**，不是校验模型。整体更像「一个可靠的单机安装器」，还不是「一个可运营的发布系统」。
6. **本项目最大的问题不是「没有差分」，而是「坏版本无法止损」**：启动探测失败回滚到旧版后，下一次检查仍会把同一个坏版本判定为「有更新」，形成升级—崩溃—回滚—再升级的循环；同时 `rollback()` 不删除新版本新增的文件，而且备份在 3 秒探测通过后就被删除，运行时崩溃已无旧版可回。详见 6.3 的 P0-1 / P0-2 / P0-6。
7. **一句话判断**：更新逻辑「可用、完整性校验达标，但不符合成熟产品的发布控制基线」。结论是**保留自研、补齐发布控制**，优先级为：坏版本止损 > 回滚完整性 > 渠道/灰度与失败可见性 > 无签名补偿（误报运营与手动兜底） > 断点续传/差分。
8. **不签名的边界必须写清楚**：清单签名 + SHA256 防的是「更新内容被篡改、更新源被伪造、发布资产被替换」，防不了「Windows 是否信任这个二进制」、首次安装被替换、以及 SmartScreen/AV 误报；未签名文件每个新版本都从零积累 hash 信誉。[D37] 无签名不是「安全不合格」，而是信任锚不同——详见 7.5。

---

## 1. 调研范围与证据分级

### 1.1 覆盖范围

| 类别 | 方案 | 为什么选它 |
|---|---|---|
| 框架自带 | Tauri `tauri-plugin-updater` | 与「Rust + Tauri」最直接相关 |
| 框架自带 | Electron 内置 `autoUpdater` + Squirrel.Mac / Squirrel.Windows | Electron 官方默认路径，历史包袱典型 |
| 生态事实标准 | `electron-updater`（electron-builder） | 大量 Electron 应用的实际上选择 |
| 大厂桌面 | VS Code | 用户级/系统级安装、企业策略、后台更新的典型 |
| 浏览器 | Chrome（Omaha / Google Update） | 灰度、差分、CUP、pingback 的工业级样本 |
| 浏览器 | Firefox（AUS / Balrog / MAR） | 更新状态机、partial MAR、提权服务的完整样本 |
| 闭源应用 | Obsidian、Notion | 大多数商业桌面应用的真实行为 |
| Rust 应用 | Zed | Rust 自研更新器、stable/preview/nightly 渠道 |
| 反例 | 依赖包管理器的 CLI/桌面工具（Alacritty 等） | 说明「不做自更新」也是合理选择 |

### 1.2 证据分级的原则

- **[D] 文档**：官方文档、协议规范、帮助中心，能直接引用。
- **[S] 源码**：能读到实现细节（如 Tauri updater 的 minisign 校验、本项目 journal）。
- **[I] 推断**：文档没有明说，但可以从 API、行为、行业惯例合理推断。报告中会明确标注，避免把推断当成事实。

---

## 2. 本项目现状（只读审阅）

### 2.1 一个需要先澄清的事实

当前仓库 **不是 Tauri 工程**：

- 没有 `tauri.conf.json`、`package.json`，`Cargo.toml` 也没有 `tauri` 依赖；
- 目录结构是「Rust 更新器核心 + C++ Win32 演示 GUI」；
- 配置来自 [`updater.conf`](../../../updater.conf)，由 `build.rs` 编译进二进制。

因此，本报告把本项目定位为「**可独立接入任意桌面应用的通用更新器**」，而不是「Tauri 内置 updater 的使用者」。按既定路线，它会复用到其他新老项目，且不改为 Tauri 官方 updater，所以第 7.4 节按「保留自研」展开；第 3.1 节和第 4 节的对照用于说明业界位置，不构成替换建议。

### 2.2 当前配置

| 配置项 | 值 | 含义 |
|---|---|---|
| `repo` | `Luogoddes/openai-gemini` | GitHub 仓库 |
| `feed_name` | `latest-version-windows-x64.json` | 静态清单文件名 |
| `exe_name` | `GuiUpdaterDemo.exe` | 安装目录内主程序 |
| `variant` | `portable` | 便携版，不写注册表/系统目录 |
| `platform` | `windows-x64` | 平台标识 |
| `interval_hours` | `24` | 自动检查间隔 |
| `startup_check_delay_seconds` | `30` | 启动后延迟检查 |
| `product` | `generic-updater` | 清单产品标识 |
| `require_signature` | `true` | 强制校验 feed 的 RSA-PSS 签名 |
| `config.example.json` | `http://127.0.0.1:8124/...`、`require_signature=false` | 仅本地调试示例 |

### 2.3 当前更新链路

1. GUI 启动后延迟 30 秒，调用更新器 `--check`；
2. 更新器从 GitHub Release 解析 feed（API 或 `feed_url`），校验 RSA-PSS 信封；
3. 比较 semver，若远端更新则输出 `Available`；
4. 用户确认后 `--download`：下载 zip 到缓存，校验 size + SHA256，解压到 `staging`；
5. 用户确认安装后 `--apply`：等待主进程退出 → 检查目录可写 → 不可写则 UAC 重新拉起自己 → 逐文件备份到 `backup` + 覆盖 → 写 `journal` 提交 → 拉起新版本；
6. 启动探测 3 秒：进程提前退出则 `rollback()` 还原备份，再拉起旧版；
7. 24 小时内不重复联网，但会复用缓存的「上次远端版本」提示。

### 2.4 关键设计点

| 设计点 | 当前做法 | 评价 |
|---|---|---|
| 清单来源 | GitHub Release 静态 JSON | 与 Tauri/Electron 的静态 feed 同级 |
| 清单完整性 | RSA-PSS 签名信封（3072 位，SHA-256，salt 32） | 与 Tauri/Sparkle 同类；Electron 常见做法是 HTTPS + sha512 + 代码签名 |
| 包完整性 | 清单内 SHA256 + size | 符合事实标准 |
| 安装事务 | 逐文件备份 + journal + 断电恢复 | 思路正确，但回滚边界不完整 |
| 提权 | 目录不可写时 UAC 重新拉起自身 | 便携版可用；UAC 会显示「未知发布者」 |
| 回滚触发 | 启动后 3 秒内退出 | 只能覆盖「立刻崩」，覆盖不了「运行一段时间后崩」 |
| 发布控制 | 单一 feed、单一渠道、无灰度、无拉黑 | 最大短板 |
| 遥测 | 无 | 无法知道更新成功率 |
| 代码签名 | 无 Authenticode（README 已说明，既定取舍） | 不影响更新链路完整性；影响首次运行信任与 AV 误报，见 7.5 |

---

## 3. 主流方案逐项拆解

### 3.1 Tauri：`tauri-plugin-updater`

**更新源与清单**

- 支持静态 JSON 与动态更新服务器两种模式。[D1]
- 静态清单形状：`version`、`notes`、`pub_date`、`platforms.{OS-ARCH}.{url, signature}`；`signature` 必须是 `.sig` 文件内容，不能填 URL。[D1]
- 动态服务器：endpoint 支持 `{{current_version}}`、`{{target}}`、`{{arch}}` 变量；无更新返回 `204 No Content`，有更新返回 `200` + JSON（`version/url/signature/notes/pub_date`）。[D1]
- 默认只接受 HTTPS，除非显式设置 `dangerousInsecureTransportProtocol`。[D1]

**版本、渠道、灰度**

- 插件只做 semver 比较；**没有内置 channel / staged rollout**。
- 需要灰度时，通常让服务端根据 `current_version/target/arch`（或额外请求头）决定返回 204 还是某个版本；也可以在 Rust 侧覆盖 `version_comparator`，甚至安装服务端指定的「更低版本」来实现回滚。[D1]
- 官方生态：`tauri-action` 生成静态 JSON；CrabNebula Cloud 是官方合作伙伴，提供动态更新服务与 CDN。[D1][D4]

**包与下载**

- 支持 Windows NSIS/MSI、macOS `.app.tar.gz`、Linux AppImage/deb/rpm。[D1]
- **全量包**：文档只描述完整更新包，没有差分配置；插件 API 也没有断点续传/恢复参数。[D1][I]
- 签名：**minisign 强制校验，不能关闭**；构建时生成 `.sig`，公钥写入配置。[D1][D2][S]

**安装与提权**

- Windows `installMode`：`passive`（默认，小进度条、无需交互）、`basicUi`（需要用户点）、`quiet`（无进度，不能自行请求管理员权限，只适合用户级安装或已提权）。[D1]
- macOS 通过替换 `.app` 完成；签名/公证仍由 Apple 的 Gatekeeper 体系约束。[D1][D27]

**回滚与降级**

- 没有内建「安装失败自动回滚」；回滚主要依赖安装器自身或服务端下发旧版本 + 覆盖版本比较。[D1][I]
- 服务端可以强制降级，这是它比「只升不降」的静态 feed 更灵活的地方。[D1]

**遥测**

- 插件本身不做成功率统计；需要应用在 `check/download/install` 各阶段自行上报。[D1][I]

**小结**：Tauri 的哲学是「安全的最小闭环」：签名清单 + 全量包 + 系统安装器。它把发布策略留给服务端，适合中小团队；但如果你要灰度、差分、断点续传、成功率监控，必须自己建。

### 3.2 Electron 内置 `autoUpdater` + Squirrel

**平台与清单**

- 内置 `autoUpdater` 只支持 macOS/Windows，Linux 建议交给包管理器。[D7]
- macOS 基于 Squirrel.Mac：读取 `releases.json`（或 `serverType: 'json'` 的 feed），条目包含 `url`、可选的 `sha256`、`size`、`delta`；应用**必须签名**才能自动更新。[D7][D8]
- Windows 基于 Squirrel.Windows：客户端读取 `/RELEASES`，里面是「包哈希 + `.nupkg` 文件名/URL + 大小」；Squirrel 自己比较版本、下载 full/delta 包、解压到版本目录、更新快捷方式。[D8][D29]
- 新版 Electron 还支持 MSIX：可直接给 MSIX 文件链接或 JSON feed，并可用 `allowAnyVersion` 允许降级。[D7]

**下载与差分**

- Squirrel.Mac：支持 `ETag`/`Last-Modified` + `Range` 断点续传；支持 `delta` 对象（Sparkle `BinaryDelta` 生成），差分失败自动回退整包；整包/差分都要过代码签名校验。[D7]
- Squirrel.Windows：RELEASES 同时列 full 与 delta `.nupkg`，客户端按效率选择。[D8]

**安装与提权**

- Squirrel.Windows 默认**按用户安装**到 `%LocalAppData%`，避免 UAC；通过 `Update.exe` 完成替换，首次启动带 `--squirrel-firstrun`，此时会有文件锁，更新检查需避开前几秒。[D7][D29][I]
- 安装完成后通常在下次启动生效；`quitAndInstall()` 可立即重启安装。[D7]

**回滚**

- Squirrel 保留按版本号命名的目录，理论上可回退；但官方 API 不提供「一键回滚」，更多是「旧版本目录还在」。[D29][I]

**小结**：Squirrel 是 Electron 的历史默认答案，解决了「按用户安装 + 差分 + 快捷方式」；但它正在被 NSIS/MSIX 替代（electron-builder 明确不支持 Squirrel.Windows）。[D5]

### 3.3 electron-updater（electron-builder）：Electron 生态的事实标准

**清单**

- 生成 `latest.yml` / `latest-mac.yml` / `latest-linux.yml`，包含版本、文件列表（`url`、`sha512`、`size`、`blockMapSize`）、`path`、`stagingPercentage` 等。[D5][D31]
- 支持 GitHub Releases、S3、Spaces、R2、Keygen、通用 HTTP(S) 服务器。[D5]

**差分**

- Windows NSIS 使用 `.blockmap`：v3 是三级 block map，更新器只通过 HTTP `Range` 拉取「相对已安装版本发生变化的块」；文档标注需要 electron-updater 7+。[D6]
- 差分失败/没有旧 blockmap 时回退整包。[D6]

**灰度与渠道**

- `latest.yml` 可带 `stagingPercentage: 10`，按用户唯一 ID 做**粘性**灰度：同一用户重启后仍保持同一分组。[D31][I]
- 支持 `channel` / prerelease 渠道文件；不同渠道可用不同 feed。[D5][I]

**签名**

- macOS 自动更新要求应用签名；Windows 会校验 Authenticode（`NsisUpdater.verifyUpdateFileAuthenticodeSignature`），并可校验 `publisherName`。[D5][D7]
- 清单本身可做签名（signed update manifests），但这不是所有项目的默认动作。[D5]

**安装**

- 默认 `autoInstallOnAppQuit = true`：下载完成后退出即安装；也可 `quitAndInstall()` 立即安装，或 `autoInstallEvent = "manual"` 完全手动。[D5]
- per-machine NSIS 会带 `isAdminRightsRequired`，通过 `elevate.exe` 提权。[D5]

**遥测**

- 提供 `checking-for-update`、`update-available`、`download-progress`、`update-downloaded`、`error` 等事件；**没有内置后端**，成功率要应用自己埋点。[D5][D7][I]

**小结**：electron-updater 把「清单 + 差分 + 灰度 + 签名校验 + 多 provider」做成了默认能力，是 Electron 应用最省事的路线；代价是清单格式与 electron-builder 绑定，且遥测/运营仍要自建。

### 3.4 VS Code

**更新源与节奏**

- 周更；更新从 `update.code.visualstudio.com` 下载。[D10]
- Windows 有三种安装形态：User setup、System setup、ZIP。[D9]
  - **User setup**：装到 `%LOCALAPPDATA%\Programs\Microsoft VS Code`，不需要管理员权限，官方明确说「提供最顺滑的后台更新体验」；但以管理员身份运行时更新会被禁用。[D9]
  - **System setup**：装到 `Program Files`，产品内更新也需要提权。[D9]
  - **ZIP**：手动更新。[D9]

**更新策略**

- 默认后台检查并下载，提示用户重启安装；`update.mode` 支持 `default` / `start` / `manual` / `none`。[D10][D11]
- 企业可通过 `UpdateMode` 策略集中控制；`none` 时应用内完全不能更新，交给软件分发工具。[D10]
- 扩展更新是独立通道（`extensions.autoUpdate`），与主程序更新解耦。[D10]

**特点**

- 用户级安装 + 后台更新是「无 UAC 顺滑更新」的教科书做法。
- 企业策略、网络白名单（`update.code.visualstudio.com`）文档齐全。[D10]
- 没有公开的灰度/差分细节；从产品行为看是「稳定版周更 + Insiders 独立渠道」。[D10][I]

### 3.5 Chrome：Omaha / Google Update

**协议**

- Omaha 3.1 是 HTTP + JSON 的三阶段会话：[D12]
  1. **Update Check**：客户端 POST 当前应用/版本/cohort/差分指纹等；服务端返回是否有更新、包 URL、差分信息；
  2. **Download**：HTTP GET，客户端可自由使用 `Range` 等 HTTP 能力；
  3. **Ping-Back**：POST 更新结果、下载 URL、错误码，形成成功率数据。
- 更新检查的完整性由 **CUP**（Client Update Protocol）保护，即使 TLS 被攻破也能发现篡改。[D12]

**灰度与渠道**

- `cohort` 是「机器可读的发布队列标识」，用于**分数灰度**；服务端可以在响应里返回新的 cohort，客户端下次检查时带上，从而实现「把用户迁移到另一个队列」。[D12]
- `cohortname` 是人类可读的队列名；`cohorthint` 是客户端请求换队列的提示。[D12]
- 支持 `rollback_allowed`：客户端声明是否接受降级，服务端可据此下发旧版本。[D12]
- Chrome 本身有 stable/beta/dev/canary 渠道，企业 MSI 可关闭自动更新。[D15][D16][I]

**差分**

- Omaha 包支持差分：服务端给 `fp`（differential fingerprint）、`hashdiff_sha256`、`sizediff`、`namediff`；客户端优先差分，失败回退整包。[D12]
- 差分格式以 Puffin（`.puff`）为代表；Zucchini 是 Chromium 生态常见的补丁算法，但当前协议文档明确提到的是 Puffin。[D12]

**安装与提权**

- Google Update 作为后台服务运行，per-machine 安装的更新由服务完成，用户通常看不到 UAC；浏览器只提示「重新启动」。[D12][D15][I]
- 用户关闭并重开 Chrome 时应用待安装更新；也可点「重新启动」立即应用。[D15]

**回滚**

- 服务端可通过 cohort/rollback 机制停止或回退；客户端本身以「只升」为主，但协议允许降级。[D12]

**遥测**

- Ping-back 天然包含成功/失败/错误码/下载 URL，是「更新成功率监控」的原始数据。[D12]

**小结**：Omaha 是「更新即运营」的样板：灰度、差分、断点、抗 TLS 篡改、成功率回传全都有；代价是需要常驻服务、服务端规则系统和较高工程投入。

### 3.6 Firefox：AUS / Balrog / MAR

**更新检查**

- 启动后不久请求 AUS，URL 携带产品、版本、build id、target、locale、channel、OS、发行版、系统能力等；Balrog 根据规则返回 XML。[D18]
- Balrog 规则可做：平台/版本/locale/channel 匹配、节流（throttling）、`backgroundRate`（后台更新比例）、watershed（必须先升到某个中间版本）。[D18][D20][D21]
- 手动检查会带 `force` 参数，绕过节流。[D18]

**包与差分**

- MAR 有两种：**complete MAR**（完整安装）与 **partial MAR**（只含差异，主要用 bspatch/Zucchini 二进制补丁）。[D19]
- partial 只对「最近四个版本（含点版本）」生成；更老的版本直接给 complete。[D19]
- 支持 bzip2/xz 压缩；MAR 内含签名与 manifest。[D19]

**下载与安装**

- 下载用 `nsIIncrementalDownloader`，支持**断点续传**；Firefox 关闭后下次启动继续下载。[D18]
- Windows 还有「关闭时后台更新」：任务计划程序每 7 小时触发一次后台任务，首次触发用 **BITS** 下载，第二次触发完成 staging；前提是安装了 Mozilla Maintenance Service。[D36]
- 默认开启 **staging**：应用运行期间把安装复制到另一个目录并先更新副本，重启时只做「替换」，减少启动等待。[D17]
- 更新状态文件驱动状态机：`downloading` → `pending` / `pending-service` / `pending-elevate` / `applied` → `applying` → `succeeded` / `failed`。[D18]
- Windows 上如果 `Program Files` 需要提权，优先走 **Mozilla Maintenance Service**（已安装的服务以高权限执行更新），服务不可用才弹 UAC；UAC 被拒会记录错误并重试，连续失败后提示用户下载安装包。[D18]
- macOS 可直接拉起提权 updater；Linux 交给发行版包管理器。[D18][D35]

**回滚与失败处理**

- partial 安装失败会自动改下 complete MAR 重试；complete 也失败才提示用户。[D18]
- 没有「运行 3 秒后崩溃就回滚」的通用机制，但状态文件会记录失败原因，便于诊断。[D18]

**遥测**

- 更新 ping（`reason=ready/success/failure`）进入 Mozilla 遥测体系，用于统计成功率与失败原因。[D18][D20]

**小结**：Firefox 的更新系统是「状态机 + 服务提权 + 差分 + 断点续传 + 遥测」的完整工程；它的复杂度来自浏览器必须覆盖极老版本、极多平台和严格的安全要求。

### 3.7 Obsidian、Notion：商业闭源应用的常见形态

**Obsidian**

- 桌面端定期检查更新；开启自动更新时，**在重启时应用**；设置里有「检查更新」和「关闭自动更新」。[D22]
- 有一个容易被忽略的点：**installer 版本与应用版本分离**。Electron 大版本升级需要用户手动下载安装器；应用内自动更新无法改 Electron 版本。[D22]
- 没有公开的清单格式、签名、灰度细节。[D22]

**Notion**

- 官方帮助只说「Notion 会自动更新」，并提供菜单里的「Check for Updates」；没有公开技术细节。[D23]
- 从行为看属于「后台下载、重启生效」的 Electron 类应用。[D23][I]

**共同点**

- 对大多数商业桌面应用，用户感知的「更新体验」就是：不打断、不丢工作、下次启动已是最新；内部用什么协议并不重要。
- 它们通常不做公开的灰度说明，但企业版/团队版往往有「延迟更新/固定版本」策略。[D22][D23][I]

### 3.8 Rust 系应用：Zed 与「不做自更新」派

**Zed**

- Rust 编写；提供 stable / preview / nightly 渠道，preview 比 stable 提前约一周。[D24][D25]
- 首次手动安装后，Zed 会周期性检查安装更新，并在有新版本时提示。[D24]
- 支持 Homebrew、winget、`install.sh`（可用 `ZED_CHANNEL=preview` / `ZED_VERSION`）等分发方式。[D24]
- 具体差分、签名、回滚实现没有公开文档；从「自研编辑器 + 自建更新」的形态看，属于自研更新器。[D24][I]

**Tauri 系应用**

- 开源项目最常见路径：`tauri-action` 构建 + 生成静态 JSON + 上传 GitHub Releases；客户端用 `tauri-plugin-updater` + minisign。[D1]
- 需要灰度时再换 CrabNebula Cloud 或自建动态服务。[D1][D4]

**不做自更新派**

- Alacritty 等工具把更新交给包管理器（Homebrew、winget、apt、cargo）；应用内不实现更新器。[I]
- 这不是落后，而是「分发渠道决定更新责任」：包管理器安装的软件不应绕过包管理器自更新，否则会破坏系统一致性。

---

## 4. 关键设计维度对照

为可读性，矩阵分两张表。列含义：

- **Tauri**：`tauri-plugin-updater`
- **E-Up**：`electron-updater`（electron-builder）
- **Squirrel**：Electron 内置 autoUpdater / Squirrel.Windows / Squirrel.Mac
- **VSC**：VS Code
- **Chrome**：Omaha / Google Update
- **FF**：Firefox / Balrog
- **O/N**：Obsidian / Notion
- **本项目**：当前通用更新器

### 4.1 发布与包

| 维度 | Tauri | E-Up | Squirrel | VSC | Chrome | FF | O/N | 本项目 |
|---|---|---|---|---|---|---|---|---|
| 更新源/清单 | 静态 JSON 或动态 204/200；`platforms` 按 OS-ARCH [D1] | `latest*.yml`，多 provider（GitHub/S3/R2/通用）[D5] | `releases.json`（Mac）/ `RELEASES`（Win）[D7][D8] | 微软更新服务 `update.code.visualstudio.com` [D10] | Omaha JSON，POST 检查 [D12] | AUS XML + Balrog 规则 [D18] | 未公开；后台下载/重启生效 [D22][D23][I] | GitHub Release 静态 JSON + 可选 `feed_url` |
| 版本比较 | semver；可覆盖 comparator 强制降级 [D1] | semver + channel/prerelease [D5] | 客户端比较版本 [D8] | 周更，渠道由产品控制 [D10] | 服务端决定，支持 rollback [D12] | Balrog 规则决定，支持 watershed [D20] | 未公开 | semver，只升不降；无强制回滚 |
| 渠道/灰度 | 无内置；服务端按变量分流 [D1] | `channel` + `stagingPercentage` 粘性灰度 [D5][D31][I] | 无内置灰度；可自建服务 [D8] | stable/Insiders；企业策略 [D10] | cohort 分数灰度 + 多 channel [D12] | Balrog 规则 + backgroundRate 节流 [D18][D21] | 未公开 | 无 channel、无灰度、无拉黑 |
| 增量 vs 全量 | 全量包 [D1][I] | NSIS blockmap 差分，失败回退整包 [D6] | full/delta `.nupkg` [D8] | 未公开；以全量为主 [I] | 差分（Puffin 等）+ 整包兜底 [D12] | partial MAR（近 4 版）+ complete MAR 兜底 [D19] | 未公开；大概率全量 [I] | 全量 zip |
| 下载/续传 | 一次性流式下载，未见续传 API [D1][I] | 支持 Range/多段与差分块拉取 [D6] | Mac 支持 ETag/Range 续传 [D7] | 后台下载，细节未公开 [D10][I] | HTTP Range 可用；客户端可续传 [D12] | `nsIIncrementalDownloader` + BITS 续传 [D18][D36] | 未公开 | 全量重下；`.part` 每次截断，无 Range |
| 清单完整性 | minisign 强制，不可关闭 [D1][D2] | sha512 + 可选清单签名 + Authenticode [D5] | RELEASES 包哈希；Mac 代码签名 [D7][D8] | 微软签名体系 [I] | CUP 抗 TLS 篡改 [D12] | MAR 签名 + HTTPS [D19] | 未公开 | RSA-PSS 签名信封 + SHA256 |
| 代码签名/公证 | 依赖安装器/平台签名 [D1] | macOS 必须签名；Windows Authenticode [D5] | macOS 必须签名 [D7] | 微软签名 [I] | Google 签名 [I] | Mozilla 签名 + macOS 公证 [D19][D27] | 未公开 | 无 Authenticode（既定取舍，见 7.5） |

### 4.2 安装与运维

| 维度 | Tauri | E-Up | Squirrel | VSC | Chrome | FF | O/N | 本项目 |
|---|---|---|---|---|---|---|---|---|
| 安装时机 | 下载后调安装器；`passive` 默认无交互 [D1] | 下载后退出即装 / `quitAndInstall` / 手动 [D5] | 下次启动或 `quitAndInstall` [D7] | 后台下载，提示重启 [D10] | 关闭/重启时应用；可点 Relaunch [D15] | 关闭时应用；staging 减少等待 [D17] | 重启时应用 [D22][D23][I] | 用户点安装 → 退出 → 覆盖 → 拉起 |
| 提权 | `quiet` 不能自行提权；`passive/basicUi` 由安装器处理 [D1] | per-machine 用 `elevate.exe`；否则用户级 [D5] | 默认按用户安装，通常无 UAC [D7] | User setup 无提权；System setup 需提权 [D9] | Google Update 服务后台提权 [D12][I] | Maintenance Service 优先，UAC 兜底 [D18] | 未公开 | 目录不可写时 UAC 重新拉起自身 |
| 回滚/降级 | 无自动回滚；可服务端强制降级 [D1] | 无自动回滚；可 `allowDowngrade` [D5][I] | 旧版本目录保留 [D29][I] | 安装器体系 [I] | cohort/rollback_allowed [D12] | partial 失败→complete；状态文件记录 [D18] | 未公开 | journal + backup；启动 3 秒探测失败回滚（不完整） |
| 企业/离线 | 无内置策略；可自建 feed [D1] | 私有 repo/通用服务器/认证头 [D5] | 可自建 Squirrel 服务 [D8] | `UpdateMode` 策略、白名单、离线部署 [D10] | MSI + 企业策略 + 关闭自动更新 [D16] | ESR/发行版包管理器/策略 [D18] | 企业版策略（未公开细节）[I] | 无策略、无代理配置、无离线包导入 |
| 遥测/成功率 | 无内置 [D1][I] | 事件齐全，无后端 [D5][I] | 无内置 [I] | 产品遥测（未公开细节）[I] | pingback 含结果/错误码/URL [D12] | update ping（ready/success/failure）[D18] | 未公开 | 无 |
| 失败重试 | 由应用/服务端决定 [I] | 差分失败回退整包；网络重试需应用处理 [D6][I] | 客户端自身逻辑 [I] | 后台重试 [I] | 客户端调度 + 抖动 [D12] | 下载续传、UAC 失败重试、partial 兜底 [D18] | 未公开 | 安装重试 90 秒；下载无重试/退避 |
| 更新器自更新 | 随应用包一起更新 [D1] | 随应用包一起更新 [D5] | Update.exe 自管理 [D29] | 随应用更新 [D10] | Google Update 自更新 [I] | updater 随安装包 [D18] | 未公开 | 更新器在主程序包内，靠 `.old-<pid>` 重命名替换 |
| 观测/可诊断性 | 应用日志 [I] | 事件 + 日志 [D5] | 日志 [I] | 日志/遥测 [I] | pingback + Omaha 日志 [D12] | update.status + 遥测 [D18] | 未公开 | stdout JSON 帧 + state.json；无集中上报 |

### 4.3 签名与完整性体系对照

「签名」在不同方案里保护的对象并不相同，不能简单比较「有没有签名」。

| 机制 | 保护对象 | 代表 | 说明 |
|---|---|---|---|
| minisign（Ed25519） | 更新包字节 | Tauri | 公钥内置客户端，`.sig` 与包一起发布；签名不可关闭。[D1][D2][S] |
| RSA-PSS 信封 | feed 的 payload 字节 | 本项目 | 清单整体签名，包哈希在签名范围内；等价于「签名 appcast」。[S] |
| SHA-512 清单 | 安装包 | electron-updater | `latest.yml` 记录 `sha512`/`size`；清单本身可选签名。[D5] |
| Authenticode | exe / 安装器 | Windows 生态 | 影响 SmartScreen/AV 信任；应有时间戳，避免证书过期后失效。[D32] |
| Apple codesign + 公证 | `.app` / `.dmg` | macOS 生态 | Squirrel.Mac 明确要求签名；公证是 Gatekeeper 的现实门槛。[D7][D27] |
| MAR 签名 | MAR 归档 | Firefox | 更新器在应用前校验；partial/complete 都受保护。[D19] |
| CUP | 更新检查/下载元数据 | Chrome/Omaha | 即使 TLS 被攻破，也能发现响应被篡改。[D12] |
| RELEASES 哈希 | `.nupkg` | Squirrel.Windows | 客户端按 RELEASES 校验包；代码签名仍是额外一层。[D8] |

**要点**：

- 「清单签名 + 包哈希」和「包本身签名」是两种不同模型；前者更灵活（清单可动态生成），后者更直接（安装器可独立校验）。
- 代码签名（Authenticode/codesign）解决的是「操作系统是否信任这个二进制」，元数据签名解决的是「更新器是否信任这个包」；两者不能互相替代。本项目属于前者，并已明确不做 Authenticode——Windows 侧的信任只能靠发布纪律与信誉积累，见 7.5。
- 对 macOS，签名、公证、`.app` 打包方式（保留符号链接/权限）必须一起做对，否则更新后可能无法启动。

---

## 5. 业界「大众做法」基线

### 5.1 事实标准（成熟产品几乎都做）

| 做法 | 普及度 | 说明 |
|---|---|---|
| HTTPS 拉取元数据 | 几乎 100% | Tauri 默认拒绝非 HTTPS；Omaha 还加 CUP。[D1][D12] |
| 元数据/包做密码学校验 | 几乎 100% | minisign、RSA/Ed25519 签名、sha512、MAR 签名、Authenticode 都是不同形态的同一件事。[D1][D5][D19] |
| semver 版本比较 | 几乎 100% | 客户端判断「是否更新」；服务端决定「发给谁」。 |
| 后台检查 + 静默下载 + 用户确认重启 | 绝大多数 | Chrome/VS Code/Firefox/Obsidian 都是这个交互。[D10][D15][D17][D22] |
| 安装前退出应用 | 绝大多数 | Windows 文件锁决定必须如此；Firefox 用 staging 规避长时间等待。[D17] |
| 保留旧版本/可回退 | 成熟产品 | Squirrel 版本目录、Firefox 状态机、Chrome cohort 回退。[D12][D18][D29] |
| macOS 签名 + 公证 | macOS 产品 | Squirrel.Mac 明确要求签名；公证是 Gatekeeper 的现实要求。[D7][D27] |
| Windows 代码签名 | 商业产品普遍；个人开源常见例外 | 商业产品几乎都签；个人开源项目大量不签（SnowShot 等）。未签名时每个新版本从零积累 SmartScreen hash 信誉，首发必然出现「Windows protected your PC」。[D37][D40] |

### 5.2 大众做法 vs 少数派

| 做法 | 普及度 | 代表 | 判断 |
|---|---|---|---|
| 静态 JSON/YAML 放 GitHub Releases/CDN | 高 | Tauri-action、electron-updater、大量开源应用 | 大众基线；够用、便宜、易审计 |
| 24 小时检查一次 | 高 | 本项目、很多 Electron 应用 | 大众基线 |
| 全量包 | 高 | Tauri、多数小型应用 | 包 < 50MB 时完全合理 |
| 用户级安装避免 UAC | 高 | VS Code User setup、Squirrel.Windows | 体验最优解之一 |
| 提示重启，不强制 | 高 | Chrome/VS Code/Obsidian | 大众基线 |
| 手动「检查更新」入口 | 高 | 几乎所有产品 | 必备兜底 |
| 可关闭自动更新 | 高 | VS Code/Obsidian/企业产品 | 企业环境必需 |
| channel/beta | 中 | VS Code Insiders、Zed、Chrome、Firefox | 有持续交付需求才做 |
| 灰度发布 | 中低 | Chrome cohort、electron-updater `stagingPercentage`、Balrog | 用户量大/风险高时值得 |
| 二进制差分 | 低 | Chrome、Firefox、electron-updater NSIS | 包大 + 更新频繁才划算 |
| 断点续传 | 低 | Firefox BITS、Squirrel.Mac、Chrome | 大包/弱网才有明显收益 |
| 常驻更新服务提权 | 低 | Chrome Google Update、Firefox Maintenance Service | 工程与运维成本高 |
| 自建动态更新服务 | 低 | 大厂/商业化产品 | 需要灰度、审计、风控时 |
| 遥测/成功率监控 | 中 | Chrome/Firefox 内建；Electron 需自建 | 运营成熟度分水岭 |
| MSIX/App Installer | 低 | 部分 Windows 应用 | 受分发渠道约束 |

### 5.3 常见踩坑

**Windows**

1. **per-machine 安装导致每次更新弹 UAC**：用户级安装可以彻底绕开；VS Code 的 User setup 就是为此存在。[D9]
2. **覆盖运行中的 exe 失败**：必须先退出/重命名；本项目用 `.old-<pid>` 处理，是正确方向。[S]
3. **更新后快捷方式/AUMID 失效**：Squirrel 用 `com.squirrel.<id>.<exe>` 固定 AUMID；自研安装器要自己维护。[D7]
4. **SmartScreen/AV 误报**：无 Authenticode 时每个新版本从零积累 hash 信誉，首发几乎必然出现「Windows protected your PC」；企业策略可禁止绕过，Smart App Control 可能直接拦截，且没有面向消费者的人工加信誉通道。[D37] 个人开源项目通常不靠签名解决，而靠发布纪律 + 灰度 + 误报运营（见 7.5）。
5. **文件锁/杀软扫描导致安装中途失败**：需要重试 + 幂等事务；重试前必须先回到干净状态，否则会污染备份。
6. **以管理员身份运行应用**：VS Code 明确说明 User setup 在管理员模式下会禁用更新；自研更新器也要处理。[D9]

**macOS**

7. **未签名/未公证**：Squirrel.Mac 直接拒绝；Gatekeeper 会拦截。签名、公证、hardened runtime 缺一不可。[D7][D27]
8. **zip 丢失符号链接/权限**：`.app` 打包要用 `ditto`/正确工具，否则更新后签名失效。
9. **bundle id/安装路径变化**：TCC 权限、登录项、钥匙串访问可能重置。
10. **差分补丁与签名**：Sparkle/Squirrel 的 delta 应用后仍需代码签名校验；不能只校验哈希。[D7]

**跨平台**

11. **版本号来源不一致**：tag、`Cargo.toml`、`package.json`、`version.txt`、清单各说各话；本项目明确规定「只看 tag」，是正确做法。[S]
12. **发布后替换同名资产**：会导致已下载用户哈希不一致；应「只增不改」。
13. **GitHub API 限流**：未认证 60 次/小时/IP；大量用户共享出口 IP 时会失败。静态 `releases/latest/download/...` 或 CDN 更稳。[D26]
14. **CDN 缓存/发布草稿**：新版本刚发布时边缘节点可能 404；客户端要有重试与「稍后再试」。
15. **灰度不粘性**：每次随机分组会导致用户体验抖动；electron-updater 用用户唯一 ID 保持粘性。[D31][I]
16. **回滚循环**：回滚后仍把坏版本当更新，是最常见也最致命的设计缺陷（本项目当前就中招，见 6.3）。
17. **配置/用户数据被覆盖**：安装目录内的配置在升级时可能被删；用户数据应放 `%APPDATA%`/`~/Library`，不要放安装目录。
18. **无遥测**：只知道「发出去了」，不知道「装成功了没有」；至少要有本地失败日志和可选上报。
19. **代理/企业网络**：只信任系统根证书或只走裸 TLS，都可能在 MITM 代理环境失败；要允许配置代理/自定义 CA。
20. **签名密钥管理**：私钥进 CI secret 是底线；要做 keyId/公钥轮换、旧公钥保留、密钥丢失预案。[D1][S]

---

## 6. 本项目评审：是否 OK、是否符合大众

### 6.1 做对了什么

1. **签名清单 + 包哈希**：RSA-PSS 3072 位，签名对象是 payload 原始字节；包 SHA256 来自签名清单。这个模型和 Tauri 的 minisign、Sparkle 的 EdDSA appcast 是同一类设计，安全性达标。[S]
2. **安装事务化**：先写 journal 再动文件，提交后才清理；断电/崩溃可恢复。多数小型 Electron 应用没有这个。[S]
3. **缓存外置**：staging/backup/journal 放缓存目录，不污染安装目录；便携版友好。[S]
4. **UAC 兜底**：目录不可写时重新拉起自身提权，避免「安装到 Program Files 就失败」。[S]
5. **更新器自替换**：运行中的 exe 先改名为 `.old-<pid>`，下次启动清理；这是 Windows 自更新的正确姿势。[S]
6. **24 小时缓存 + 手动检查**：减少无效请求，同时保留用户主动检查。[S]
7. **跳过版本**：用户可以跳过某个版本，下个版本仍会提示。[S]
8. **CI 两道保险**：`require_signature=true` 但没配 secret 直接失败；配了 secret 但公钥为空会提示。这个设计比「默认明文」安全得多。[S]
9. **文档明确「没有 Authenticode」**：没有假装安全等级，这是诚实且重要的。[S]

### 6.2 与大众基线的差距

| 维度 | 本项目 | 大众基线 | 判断 |
|---|---|---|---|
| 元数据签名 | RSA-PSS | minisign/EdDSA/代码签名 | **达标**，形态不同 |
| 包哈希 | SHA256 | sha512/SHA256 | **达标** |
| 代码签名 | 无 Authenticode（既定取舍） | 商业产品签；个人开源常不签 | **与商业产品有差距，但不按缺陷计**，见 7.5 |
| 安装事务 | journal + backup | 安装器/版本目录 | **高于平均**，但回滚有缺口 |
| 坏版本止损 | 无 | 服务端拉黑/暂停/cohort | **不达标**（最严重） |
| 渠道/灰度 | 无 | channel/stagingPercentage | **不达标** |
| 差分 | 无 | 大厂有，小应用可无 | **可接受**（包小） |
| 断点续传 | 无 | 浏览器有，桌面应用多数没有 | **可接受**，弱网体验差 |
| 重试/退避 | 仅安装重试 | 下载重试 + 抖动 | **偏弱** |
| 遥测 | 无 | 大厂有，Electron 多数自建 | **偏弱** |
| 企业/离线 | 无 | 策略/代理/镜像 | **偏弱** |
| 安装体验 | 提示 + 进度 + 重启 | 后台下载 + 提示重启 | **达标** |

### 6.3 风险与潜在 Bug（按优先级）

> 以下均为静态阅读结论，未做运行时验证；行号以当前工作区为准。审阅范围是更新链路（check/download/apply/recover），不包含 C++ GUI、构建脚本和 CI 的全面审计。

#### P0-1：坏版本回滚后会被再次推送，形成升级循环

- 位置：[`src/update.rs`](../../../src/update.rs#L150-L166) 只判断 `feed.version <= local`；[`src/update.rs`](../../../src/update.rs#L375-L381) 探测失败后 `rollback` + 拉起旧版，但**没有记录「这个版本安装失败过」**。
- 后果：1.2.0 启动即崩 → 回滚到 1.1.0 → 24 小时后（或用户手动检查）仍看到 1.2.0 有更新 → 再次下载安装 → 再次崩溃。用户如果没有点「跳过」，就陷入循环。
- 为什么主流不会这样：Chrome 有 cohort/rollback 与服务端停止发放；Balrog 可以暂停规则；electron-updater 可以撤下 `latest.yml` 或把 `stagingPercentage` 设为 0；Tauri 动态服务可以对特定 `current_version` 返回 204。[D1][D12][D18][D31]
- 建议：`state.failed_version` + 失败时间；自动检查时跳过该版本，手动检查可强制重试；服务端清单增加 `blocked_versions` / `min_supported_version`。这是**最高优先级**。

#### P0-2：`rollback()` 不删除新版本新增的文件，回滚不彻底

- 位置：[`src/install.rs`](../../../src/install.rs#L268-L285) 只遍历 `backup` 目录把旧文件复制回去；新版本新增的、旧版本不存在的文件（`Plan.backup == None`）不会被删除。
- 对比：[`src/journal.rs`](../../../src/journal.rs#L110-L128) 的崩溃恢复逻辑反而会删除 `backup: None` 的 target，说明设计意图是「新增文件也应删除」，但 `rollback()` 没实现。
- 后果：回滚后目录里残留新版本文件；如果这些文件影响旧版启动（插件、DLL、配置），回滚可能无效；`installed.json` 也仍是新版本清单，下一次安装的「清理旧文件」会基于错误清单。
- 文档还写着「新增文件都还原」（[`README.md`](../../../README.md#L87)），与实现不一致。
- 建议：让 `rollback()` 复用 journal 的语义（删除 `backup: None` 的 target），并恢复 `installed.json`；只有在回滚成功后才清理 backup/journal。

#### P0-3：安装重试会覆盖旧备份，污染回滚基线

- 位置：[`src/update.rs`](../../../src/update.rs#L359-L364) 用 `retry_until(90s, || install::install(...))`；[`src/install.rs`](../../../src/install.rs#L239-L245) 每次重试都重新规划 `target.exists()` 并 `copy(target, backup)`。
- 后果：第一次安装已经覆盖了部分文件后失败；第二次重试把「已被新版本覆盖的文件」当成旧文件备份，覆盖掉第一次备份的真旧版。此时回滚会回到「半新半旧」状态，而不是原始版本。
- 建议：安装事务必须幂等——重试前先按 journal 回滚到干净状态，或者在第一次失败后直接 `rollback` 再重试；备份已存在时不得覆盖。

#### P0-4：安装失败后应用保持关闭，没有自动拉起旧版

- 位置：[`src/update.rs`](../../../src/update.rs#L357-L364) 安装失败直接 `return Err(fail(...))`；只有启动探测失败路径（L375-L381）会 `relaunch`。
- 后果：用户点了安装，应用退出；安装因权限/磁盘/文件锁失败；更新器报错退出，用户没有界面可用。
- 建议：失败路径先 `rollback`，再 `relaunch` 旧版；如果回滚也失败，至少保留错误界面/日志并提示手动重装。

#### P0-5：回滚失败仍删除 journal 和 backup，失去第二次恢复机会

- 位置：[`src/update.rs`](../../../src/update.rs#L396-L402) `let _ = install::rollback(ctx); install::cleanup(ctx); remove journal`。
- 后果：如果某个文件被占用导致回滚失败，backup 和 journal 都被删掉；下次启动无法再恢复。
- 建议：只有 `rollback` 返回 `Ok` 才清理；失败时保留 journal/backup，并在下次启动继续尝试。

#### P0-6：备份在 3 秒探测通过后立即删除，运行时崩溃无法回滚

- 位置：[`src/update.rs`](../../../src/update.rs#L383) 在 `probe_started` 通过后立刻 `install::cleanup(&ctx)`；[`src/install.rs`](../../../src/install.rs#L288-L292) 会删除整个 `backup` 目录。
- 后果：新版启动 3 秒内没退出，备份就被清掉；如果它在 10 秒后、用户点某个功能时、或下次启动时才崩，已经没有旧文件可回滚。所谓「失败回滚」实际只覆盖「启动即崩」这一种情况。
- 对比：Squirrel 保留按版本号命名的旧目录；Chrome/Omaha 可在服务端停止发放并回退；成熟产品通常保留上一个可用版本至少一个发布周期。[D12][D29][I]
- 建议：把「探测通过」降级为「候选健康」，主程序在完成初始化/进入主界面后写健康标记；更新器下次启动确认健康后再删备份；或保留备份 N 天/直到下一个版本成功运行。

#### P1-1：没有 channel / 灰度 / 坏版本拉黑

- 位置：[`updater.conf`](../../../updater.conf) 只有单 `feed_name`；[`src/feed.rs`](../../../src/feed.rs#L94-L159) 没有 channel/rollout/blocked 字段。
- 后果：无法先给 5% 用户试新版本；出问题只能发新版本或让用户手动跳过；无法「停止发放」某个版本。
- 建议：见 7.2 的 feed v2 设计。

#### P1-2：下载无断点续传、无重试/退避，300 秒总超时

- 位置：[`src/download.rs`](../../../src/download.rs#L54-L104) 每次 `File::create(&part)` 截断，不使用 `Range`/`If-Range`；`reqwest` 客户端总超时 300 秒。
- 后果：弱网/大包下载中断后从头再来；慢速链路可能因总超时永远装不上；没有指数退避，服务端抖动时直接失败。
- 建议：`Range: bytes=<part_len>-` + `If-Range`/ETag；下载重试 3 次 + 指数退避 + 抖动；超时改为「连接超时 + 读取空闲超时」，不要卡死总时长。

#### P1-3：feed 的 `product` / `platform` 缺失时放行

- 位置：[`src/feed.rs`](../../../src/feed.rs#L104-L123) 只有字段存在时才校验；缺字段直接跳过。
- 后果：签名密钥正确的前提下，错误产品的清单也能被接受；虽然需要签名，但这削弱了「多产品共用一套签名/发布流水线」时的隔离。
- 建议：编译期内置 `expected_product` 时，要求 feed 必须带匹配的 `product`；`platform` 同理；`schema` 只接受明确的版本。

#### P1-4：默认走 GitHub API，未认证 60 次/小时/IP

- 位置：[`src/feed.rs`](../../../src/feed.rs#L160-L240)（`feed::resolve` 的 API 分支）。
- 后果：企业 NAT/校园网大量用户共享出口 IP 时，检查更新会 403；发布瞬间 API 也可能抖动。
- 建议：优先 `https://github.com/<repo>/releases/latest/download/<feed>` 静态地址；API 只作为兜底；或使用 CDN/对象存储。[D26]

#### P1-5：`state.json` 并发写没有锁

- 位置：[`src/state.rs`](../../../src/state.rs)（加载/保存）+ GUI/更新器两个进程。
- 后果：GUI 写 `skipped_version` 与更新器写 `last_check` 可能互相覆盖；用户「跳过此版本」偶尔失效。
- 建议：文件锁（Windows `LockFileEx` / 跨平台 `fs2`）或单写者模型（GUI 不直接写，统一由更新器写）。

#### P1-6：无 Authenticode 的 SmartScreen/AV 误报风险（既定取舍，不是待修 Bug）

- 位置：[`README.md`](../../../README.md#L198) 已明确说明无代码签名。
- 事实：未签名文件每个新版本从零积累 hash 信誉；首发大概率出现「Windows protected your PC」；企业策略可禁止用户绕过；Smart App Control 可能直接阻止；自签名证书与无签名行为相同；EV 证书已不再自动绕过。[D37]
- 影响：首次安装、以及每次更新后的首次启动，是用户流失与误报反馈的高发点；**清单签名与 SHA256 无法改善这一点**。
- 处理：不按「补签名」处理，按「运营风险」处理——灰度首发、保持更新器二进制稳定、发布物可预期、提供手动下载兜底、建立误报收集与 WDSI 提交流程。详见 7.5。[D37][D38]

#### P1-7：无遥测/更新成功率

- 位置：全局无上报；只有 stdout JSON 帧和 `state.json`。
- 后果：无法回答「多少人看到了更新」「多少人下载成功」「多少人安装失败」「失败在哪个阶段」。
- 建议：本地事件日志 + 可选匿名上报；见 7.2。

#### P1-8：3 秒启动探测覆盖不了「启动后崩溃」

- 位置：[`src/update.rs`](../../../src/update.rs#L282-L285) `sleep(3s)` + `try_wait`。
- 后果：新版启动 5 秒后崩溃、或用户操作到某个功能才崩，不会回滚。
- 建议：主程序写「健康标记」（首次成功进入主界面/完成初始化后写文件），更新器下次启动检查；或连续 N 次异常退出后回滚。不要把探测时间无限拉长，改用「健康确认」。
- 注意：README 写「8 秒内没起来」（[`README.md`](../../../README.md#L96)），代码是 3 秒，文档与实现不一致。

#### P2 级问题

| 问题 | 位置 | 说明 |
|---|---|---|
| README 配置块 `require_signature=false`，实际 `updater.conf=true` | [`README.md`](../../../README.md#L54) / [`updater.conf`](../../../updater.conf#L11) | 文档漂移；照抄会关闭验签 |
| `config.example.json` 用 HTTP + 不验签 | [`config.example.json`](../../../config.example.json) | 仅本地调试，但容易被误用于生产 |
| 无 ETag/If-None-Match | [`src/feed.rs`](../../../src/feed.rs) | 每次全量拉清单；小文件影响不大 |
| 检查时间无抖动 | [`src/state.rs`](../../../src/state.rs) | 大量用户同时启动会形成小高峰；加随机抖动 |
| 无代理/自定义 CA 配置 | [`src/update.rs`](../../../src/update.rs) | 企业 MITM 代理环境可能失败；提供配置项 |
| zip 解压无总大小上限 | [`src/install.rs`](../../../src/install.rs) | 签名包才可信，但磁盘耗尽仍是风险 |
| 旧版本清理可能删用户改过的配置文件 | [`src/install.rs`](../../../src/install.rs#L254) | 如果配置文件在安装目录且被移出清单，会被当 obsolete 删除 |
| 无卸载/修复 | 全局 | 便携版可接受；若转安装版需补 |
| 无最低 OS/架构门槛 | [`src/feed.rs`](../../../src/feed.rs) | 新版本可能装到不支持的系统上 |

### 6.4 结论：是否符合大众？

| 维度 | 评价 | 理由 |
|---|---|---|
| 安全基线 | **达标**（在明确不签名前提下） | 签名清单 + SHA256 与 Tauri/Sparkle 同级；保护更新内容，不承诺发布者身份 |
| 安装可靠性 | **高于平均，但回滚有洞** | journal 很好；`rollback()`、重试逻辑、备份保留窗口都不完整 |
| 发布控制 | **低于平均** | 无 channel/灰度/拉黑，坏版本会循环 |
| 下载体验 | **平均偏下** | 全量可接受；无续传/重试/退避 |
| 企业适配 | **偏弱** | 无策略、代理、离线镜像 |
| 运营观测 | **偏弱** | 无成功率数据 |
| 用户交互 | **达标** | 进度、跳过、提示重启都有 |
| 无签名影响 | **既定取舍** | SmartScreen/AV 首次拦截属预期；用灰度、手动兜底、误报运营管理（7.5） |

**一句话**：这套逻辑「不是一个错误的更新器」，而是一个「完整性做得不错、发布运营还没做完」的更新器。它符合「小型开源桌面应用」的大众水平，但不符合「有持续用户、需要灰度与止损」的成熟产品基线。最该补的不是差分，也不是代码签名，而是 **坏版本止损 + 回滚完整性 + 发布控制**；无签名是既定取舍，用 7.5 的补偿措施管理，而不是写成待办。

**四个问题的直接回答**：

| 问题 | 回答 |
|---|---|
| 现有逻辑是否站得住？ | **站得住**。校验模型、安装事务、失败拉起方向正确，优于多数小型桌面应用；不成立的是「坏版本止损」和「回滚完整性」这两个具体实现细节。 |
| 与业界大众做法的差距在哪？ | 差距集中在**发布侧**：无 channel/灰度/拉黑、无失败版本记忆、无成功率数据；其次是下载可靠性（续传/重试/超时）与企业适配。校验模型本身不落后。 |
| 哪些是必须补的？ | P0-1～P0-6（升级循环、回滚残留、重试污染备份、失败不拉起、回滚失败仍清理、3 秒探测后删备份）+ 失败可见性/手动下载兜底。这些不做会真实伤害用户。 |
| 哪些可以不做？ | 代码签名（既定取舍）、二进制差分（包仅 ~1.2MB）、常驻更新服务、MSIX、自建动态更新服务（当前用户量不需要）。渠道/灰度与遥测建议做轻量版，不必一步到位。 |

---

## 7. 建议演进路线

### 7.1 第一优先级（不做会出事故）

1. **失败版本抑制**
   - `state.json` 增加 `failed_version`、`failed_at`、`failure_stage`；
   - 自动检查时：`feed.version == failed_version` 且未超过冷却期 → 不提示；手动检查 → 允许强制重试；
   - 服务端清单增加 `blocked_versions` 与 `min_supported_version`，客户端优先信任签名清单。
2. **修复回滚完整性**
   - `rollback()` 删除 `backup: None` 的新增文件；
   - 恢复 `installed.json` 到旧版本；
   - 回滚失败时保留 journal/backup，下次启动继续；
   - 只有回滚成功才 `cleanup`。
3. **修复安装重试**
   - 重试前先 `journal::recover` 回到干净状态，或把 `install()` 做成可重入事务；
   - 备份文件已存在时拒绝覆盖。
4. **安装失败也要拉起旧版**
   - 失败路径：`rollback` → `relaunch` → 报错；
   - 如果回滚失败，保留备份并提示「请重新安装」。
5. **失败可见性与手动兜底**
   - 每个失败阶段给出明确错误码 + 日志路径，不要让更新器「报错就退出」；
   - 提供官网 / GitHub Releases 手动下载入口与 SHA256 公示，作为自动更新被 SmartScreen 或企业策略拦截时的逃生通道；
   - 这是无签名前提下的 P0，而不是「补代码签名」。

### 7.2 第二优先级（发布运营）

1. **feed v2：渠道 + 灰度 + 拉黑**

```json
{
  "schema": 2,
  "product": "generic-updater",
  "channel": "stable",
  "version": "1.2.3",
  "min_supported_version": "1.0.0",
  "blocked_versions": ["1.2.0"],
  "rollout": { "percent": 10, "salt": "2026-11" },
  "notes": "...",
  "packages": [
    { "variant": "portable", "kind": "archive", "path": "...", "size": 123, "sha256": "..." }
  ]
}
```

   - 客户端 `install_id` 随机生成并持久化；`bucket = hash(install_id + salt) % 100`；
   - `bucket < rollout.percent` 才提示；粘性由 `install_id` 保证；
   - `blocked_versions` 命中 → 不提示；`version < min_supported_version` → 强制更新提示；
   - 静态 GitHub JSON 无法按用户变化：要么接受「全量/不灰度」，要么加一层动态服务（返回同样的签名 JSON），要么发布多个 feed 文件（`stable-10.json`、`stable-100.json`）由客户端按 bucket 选择。

2. **下载可靠性与体验**
   - `Range`/`If-Range` 续传；`.part` 保留；
   - 3 次指数退避 + 抖动；
   - 连接超时/读取空闲超时分离；
   - 支持 `ETag` 清单缓存。

3. **遥测（匿名、可选）**
   - 本地 append-only 事件：`check/available/download_start/download_ok/install_start/install_ok/probe_ok/probe_fail/rollback`；
   - 字段：版本、OS、arch、阶段、错误码、耗时、是否自动；
   - 不上报用户标识；采样 10%–100%；隐私政策明示；企业环境可关闭。
   - 没有遥测，就无法验证灰度是否真的降低了失败率。

4. **渠道**
   - 至少 `stable` / `beta` 两个 feed；
   - beta 用户显式选择（设置项或启动参数），默认 stable；
   - 与灰度正交：beta 是「愿意尝鲜」，rollout 是「随机抽样」。

### 7.3 第三优先级（按规模决定）

1. **差分更新**：只有当安装包 > 20–30MB 且更新频繁时才值得；实现 blockmap/bsdiff + 整包兜底 + 双签名。当前 ~1.2MB 的包，收益远小于复杂度。
2. **常驻更新服务/系统服务提权**：只有 per-machine 安装且不想每次 UAC 时才需要；Chrome/Firefox 的路线，工程与运维成本高。
3. **MSIX/App Installer**：适合 Windows 企业分发，但会改变安装/卸载模型，便携版产品不一定适合。
4. **企业策略/离线镜像**：提供 `UPDATER_POLICY` 文件或注册表策略，支持 `feed_url` 覆盖、禁用自动更新、代理、离线包导入。VS Code 的 `UpdateMode` 是很好的参考。[D10]

### 7.4 自研更新器 vs 采用 Tauri plugin-updater

| 维度 | 保留自研 | 采用 Tauri plugin-updater |
|---|---|---|
| 代码量/维护 | 自己维护 OS 安装、提权、回滚 | 插件维护，升级跟随 Tauri |
| 签名 | RSA-PSS 清单（已实现）；不依赖 Authenticode | minisign 强制；同样不解决 Authenticode/SmartScreen |
| 安装事务/回滚 | 有 journal，可做到强事务 | 依赖系统安装器，回滚能力弱 |
| 便携版/自定义 GUI | 完全可控 | 受插件安装模式约束 |
| 灰度/渠道/遥测 | 可自己加 | 仍要自己加（插件不提供） |
| 适用前提 | 任意桌面应用、需要强控制 | 应用本身就是 Tauri |

**结论（既定路线）**：

- **保留自研**：这个仓库要复用给其他新老项目，且不采用 Tauri 官方 updater。独立 exe + 文件协议 + 强事务 + 便携版友好，比插件更适配「任意桌面应用」。
- 从 plugin-updater 借鉴两点，而不是替换：① 强制清单签名（已具备）；② 安装模式枚举（passive/basicUi/quiet），用于适配不同宿主。
- 不建议「插件负责下载、自研负责安装」的混合模式：插件内部会调安装器，绕过它做自定义安装很脆弱。

### 7.5 无代码签名条件下的可信更新与误报治理

**前提**：本项目明确不做 Windows Authenticode / macOS Developer ID 签名，沿用 SnowShot 这类个人开源项目的做法。[D40] 本项目的签名信封格式也明确对齐 SnowShot（`{schema, keyId, payload, signature}`）。[S] 这不是「忽略安全」，而是把信任锚从「操作系统信任发布者」换成「更新器信任签名清单」。两者覆盖的威胁不同，必须在文档和发布流程里写清楚。

#### 7.5.1 实际信任链

无 Authenticode 时，更新链路实际依赖这五层：

1. **传输层**：HTTPS + GitHub/对象存储的证书 → 防链路窃听与简单篡改；
2. **元数据层**：RSA-PSS 签名 feed（公钥编译进更新器）→ 防伪造 feed、防替换包哈希；
3. **包层**：签名清单内的 SHA256 + size → 防下载损坏/替换；
4. **落盘层**：staging 解压 + journal + 备份 → 防安装中断；
5. **执行层**：**没有签名** → Windows 不提供任何发布者身份保证，SmartScreen/AV 只能靠信誉与启发式。[D37]

关键点：第 2/3 层保护的是「更新器会不会装错包」，第 5 层缺的是「Windows 和用户敢不敢运行这个 exe」。二者不能互相替代，也不能用自签名证书糊弄——Microsoft 明确把自签名与无签名归为同一档行为。[D37]

#### 7.5.2 能覆盖 / 不能覆盖

| 威胁 | 清单签名 + SHA256 能否覆盖 | 说明 |
|---|---|---|
| 网络 MITM / CDN 篡改 | ✅ 能 | feed 被改则签名失败；包被改则 SHA256 不匹配 |
| 伪造更新服务器（DNS/域名劫持） | ✅ 能（公钥内置且不远程更新） | 攻击者没有私钥就签不出 feed |
| 替换发布资产（同名覆盖） | ✅ 能 | 哈希在签名清单内；改资产会导致校验失败 |
| 重放旧版本 feed | ⚠️ 部分 | 签名仍然有效；需客户端做「版本单调 + 发布时间上限」检查 |
| 发布私钥泄露 | ❌ 不能 | 攻击者可签任意 feed；只能靠密钥离线/轮换/撤销预案 |
| 构建机被入侵（私钥未泄露但产物被换） | ❌ 不能 | 签名的是「构建出来的坏包」；需要构建来源证明与人工核验（7.5.5） |
| 本地攻击者已能改安装目录 | ❌ 不能 | 更新器无法防御同等或更高权限的本地写权限 |
| 用户从第三方站下载了篡改包 | ❌ 不能 | 更新器只保护「自己发起的更新」，不保护首次安装 |
| SmartScreen / AV 误报 | ❌ 不能 | 与清单哈希无关，属于操作系统信誉体系 [D37] |
| 企业白名单/策略拦截 | ❌ 不能 | 未签名二进制在受管环境通常直接被拒 [D37] |

结论：**清单签名解决「更新内容可信」，不解决「发布者可信」**。把它写成「已签名所以安全」是过度承诺。

#### 7.5.3 无签名对 SmartScreen / AV 的实际影响

有明确文档支撑的事实 [D37]：

- 未签名文件**每个新版本都从零积累 hash 信誉**，无法从上一版本继承；
- 首次下载大概率出现「Windows protected your PC」，用户必须点「仍要运行」；
- **企业策略可以完全禁止用户绕过**；
- Windows 11 的 **Smart App Control 可能直接阻止**没有正面信誉的未签名可执行文件，且不限于「从网络下载」的文件；
- EV 证书**已不再**自动绕过 SmartScreen；
- SmartScreen 信誉**没有面向消费者的人工加速通道**，只能靠下载量与行为自然积累；WDSI 提交入口用于「误报/漏报」与企业管理场景。[D37][D38]

经验推断 [I]：

- 更新器本身（小体积、行为固定、长期少变）通常比主程序更容易积累稳定信誉；主程序每次发版都归零，首发几天到几周是误报高发窗口。
- 「无签名 + 自更新 + 拉起子进程 + 写用户目录」这几个行为组合容易被启发式规则盯上；更新器越小、越像正常程序，越安全。

#### 7.5.4 无签名前提下降低误报与体验损失

按性价比排序：

1. **不要为了「防误报」做可疑行为**：不混淆/不加壳、不动态加载 shellcode、不写 `Run` 注册表自启（除非产品确实需要）、不静默添加 Defender 排除项、不调用 `cmd.exe`/PowerShell 做安装动作。这些是 AV 规则的高危特征，收益为负。
2. **保持更新器二进制稳定**：更新器本身尽量少改，版本资源（公司名/产品名/文件描述/图标/版本号）齐全且前后一致；频繁变形的小 exe 最容易被判可疑。
3. **发布物可预期**：固定的产物命名、固定的下载域名、固定的目录结构；不要在运行时释放随机名 exe。
4. **处理 Mark-of-the-Web**：用户从浏览器手动下载的 zip 会带 MOTW，解压/运行时可能触发额外拦截；自动更新用 HTTP 客户端下载不继承 MOTW，这是自动更新相对手动安装的优势 [I]。
5. **灰度 + 快速撤回**：无签名时误报往往是「全体首发用户同时中招」；先给 5%–10% 用户，观察 24–48 小时再全量，把损失压到可接受（见 7.2）。
6. **手动下载兜底**：自动更新被 SmartScreen/企业策略拦住时，提供官网 / GitHub Releases 手动下载与校验说明（公示 SHA256）。这是无签名项目的必备逃生通道。
7. **误报运营**：收到「被杀软报毒」反馈时，收集杀软名称/版本/检测名，向对应厂商提交误报；Microsoft 侧用 WDSI 提交。[D38] 注意：这解决的是「误报」，不能提升 SmartScreen 信誉。[D37]
8. **不要把「跳过 SmartScreen」写进产品文案**：教用户关 Smart App Control / 关 Defender 会显著提高被标记为 PUA 的概率，也会劝退正常用户。
9. **文档诚实**：README 已写明无 Authenticode，应继续保持，并补充「首次运行可能出现 SmartScreen 提示」与校验方法。

#### 7.5.5 发布密钥与供应链（无签名时更重要）

没有 Authenticode 时，feed 私钥就是唯一能证明「这是我们发布的更新」的东西：

- 私钥只放 CI secret 或离线介质，绝不进仓库；本地构建默认不签名，发布流水线才签；
- 公钥内置在更新器里，支持 `keyId` 列表以便轮换；旧公钥至少保留一个发布周期；
- 发布资产「只增不改」：同一 tag 下替换同名文件会让已下载用户校验失败；
- 建议开启 GitHub Artifact Attestations（构建来源证明，SLSA Build L2），让用户可以核验「包确实由该仓库的 CI 构建」；它**不能替代代码签名**，但能补一部分「构建机被入侵」的信任。[D39]
- 发布前人工核对：tag、`Cargo.toml` 版本、feed 版本、产物 SHA256 四者一致。

#### 7.5.6 结论

不签名是既定取舍，应该被当作「产品约束」而不是「待修复漏洞」：

- **不要**把「上代码签名」写成路线图上的 TODO，也不要暗示用户「我们已经签名」；
- **要**把「清单签名 + 私钥保护 + 发布纪律 + 灰度 + 误报运营 + 手动兜底」当作正式的安全与发布策略；
- 对外表述统一为：「更新包经过签名清单与 SHA256 校验；安装包未做商业代码签名，首次运行可能出现系统提示。」

---

## 8. 来源

### 8.1 Tauri

- [D1] Tauri v2 Updater 文档：<https://v2.tauri.app/plugin/updater/>
- [D2] Tauri updater release notes（minisign-verify）：<https://tauri.app/release/updater/>
- [S] Tauri updater 源码（minisign、endpoint、installMode）：<https://github.com/tauri-apps/plugins-workspace/blob/v2/plugins/updater/src/updater.rs>
- [D4] CrabNebula Cloud 自动更新指南：<https://docs.crabnebula.dev/cloud/guides/auto-updates-tauri/>
- [D34] Tauri 分发文档（CrabNebula 为官方合作伙伴）：<https://v2.tauri.app/distribute/pipelines/crabnebula-cloud/>

### 8.2 Electron / Squirrel / electron-updater

- [D5] electron-builder Auto Update：<https://www.electron.build/docs/features/auto-update/>
- [D6] electron-builder Configuration（blockmap v3、Range）：<https://www.electron.build/docs/api/electron-builder.interface.configuration/>
- [D7] Electron autoUpdater：<https://www.electronjs.org/docs/latest/api/auto-updater>
- [D8] Electron Updating Applications（RELEASES、releases.json）：<https://www.electronjs.org/docs/latest/tutorial/updates>
- [D29] Squirrel.Windows：<https://github.com/Squirrel/Squirrel.Windows>
- [D30] Squirrel.Mac：<https://github.com/Squirrel/Squirrel.Mac>
- [D31] electron-builder v26 Auto Update（stagingPercentage 示例）：<https://www.electron.build/v26/docs/features/auto-update/>

### 8.3 VS Code

- [D9] VS Code Windows 安装（User setup / System setup / ZIP）：<https://code.visualstudio.com/docs/setup/windows>
- [D10] VS Code 企业更新策略（UpdateMode、update.code.visualstudio.com）：<https://code.visualstudio.com/docs/enterprise/updates>
- [D11] VS Code FAQ（关闭自动更新）：<https://code.visualstudio.com/docs/supporting/faq>

### 8.4 Chrome / Omaha

- [D12] Omaha Protocol 3.1（update check / download / pingback / CUP / cohort / differential / rollback）：<https://github.com/chromium/chromium/blob/main/docs/updater/protocol_3_1.md>
- [D13] Omaha 项目主页：<https://github.com/google/omaha>
- [D14] Chromium Chronicle #16（Google Update / Omaha Server）：<https://developer.chrome.com/blog/chromium-chronicle-16>
- [D15] Chrome 更新帮助（自动更新、重新启动）：<https://support.google.com/chrome/answer/95414>
- [D16] Chromium 企业部署常见问题（MSI、自动更新、关闭自动更新）：<https://www.chromium.org/administrators/common-problems-and-solutions/>

### 8.5 Firefox / Balrog

- [D17] Firefox Application Update Concepts（staging、update directory）：<https://firefox-source-docs.mozilla.org/toolkit/mozapps/update/docs/Concepts.html>
- [D18] Firefox In-App Update Process（AUS、状态机、提权、续传、失败兜底）：<https://raw.githubusercontent.com/mozilla-firefox/firefox/main/toolkit/mozapps/update/docs/InAppUpdateProcess.md>
- [D19] Firefox MAR Files（complete/partial、bspatch/Zucchini、签名）：<https://firefox-source-docs.mozilla.org/toolkit/mozapps/update/docs/MarFiles.html>
- [D20] Mozilla Update Infrastructure（Balrog、watershed）：<https://firefox-source-docs.mozilla.org/update-infrastructure/>
- [D21] Mozilla Release Process Checklist（backgroundRate）：<https://wiki.mozilla.org/Release_Management/Release_Process_Checklist_Documentation>
- [D33] Firefox 后台更新（Windows）：<https://support.mozilla.org/en-US/kb/enable-background-updates-firefox-windows>
- [D35] Firefox 更新帮助（Linux 由包管理器管理）：<https://support.mozilla.org/en-US/kb/update-firefox-latest-release>
- [D36] Firefox Background Updates（任务计划 + BITS）：<https://firefox-source-docs.mozilla.org/toolkit/mozapps/update/docs/BackgroundUpdates.html>

### 8.6 商业应用 / Rust 应用

- [D22] Obsidian 更新帮助（自动更新、 installer 更新）：<https://help.obsidian.md/Getting+started/Update+Obsidian>
- [D23] Notion 桌面版帮助（自动更新、Check for Updates）：<https://www.notion.com/help/notion-for-desktop>
- [D24] Zed 安装文档（周期检查、渠道、包管理器）：<https://zed.dev/docs/installation>
- [D25] Zed FAQ（Nightly/Preview/Stable）：<https://zed.dev/faq>
- [D40] SnowShot（个人开源桌面项目，GPL-3.0-or-later；本项目「不签名」参考）：<https://github.com/mg-chao/snow-shot> 、<https://snowshot.top/>

### 8.7 平台与通用

- [D26] GitHub REST API rate limits（未认证 60 次/小时/IP）：<https://docs.github.com/en/rest/using-the-rest-api/rate-limits-for-the-rest-api>
- [D27] Apple 公证文档：<https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution>
- [D28] Sparkle 文档（EdDSA、appcast、delta）：<https://sparkle-project.org/documentation/>
- [D32] Microsoft SmartScreen（代码签名与信誉）：<https://learn.microsoft.com/en-us/windows/security/operating-system-security/virus-and-threat-protection/microsoft-defender-smartscreen/>
- [D37] Microsoft Learn：SmartScreen reputation for Windows app developers（未签名每版本从零积累信誉、企业策略可阻止、Smart App Control、EV 不再绕过）：<https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/smartscreen-reputation>
- [D38] Microsoft Security Intelligence 文件提交（误报/漏报）：<https://www.microsoft.com/en-us/wdsi/filesubmission>
- [D39] GitHub Docs：Artifact attestations（构建来源证明，SLSA Build L2）：<https://docs.github.com/en/actions/concepts/security/artifact-attestations>

### 8.8 本项目（只读审阅）

- [`updater.conf`](../../../updater.conf)
- [`config.example.json`](../../../config.example.json)
- [`src/update.rs`](../../../src/update.rs)
- [`src/feed.rs`](../../../src/feed.rs)
- [`src/download.rs`](../../../src/download.rs)
- [`src/install.rs`](../../../src/install.rs)
- [`src/journal.rs`](../../../src/journal.rs)
- [`src/contract.rs`](../../../src/contract.rs)
- [`src/state.rs`](../../../src/state.rs)
- [`README.md`](../../../README.md)

---

## 附录 A：本项目关键行号索引

| 主题 | 位置 |
|---|---|
| 检查与版本比较 | [`src/update.rs`](../../../src/update.rs#L118-L167) |
| 下载入口 | [`src/update.rs`](../../../src/update.rs#L193-L280) |
| 启动探测（3 秒） | [`src/update.rs`](../../../src/update.rs#L282-L285) |
| 安装/UAC/重试 | [`src/update.rs`](../../../src/update.rs#L308-L394) |
| 回滚与清理 | [`src/update.rs`](../../../src/update.rs#L396-L402) |
| 安装事务 | [`src/install.rs`](../../../src/install.rs#L146-L265) |
| `rollback()` | [`src/install.rs`](../../../src/install.rs#L268-L285) |
| journal 崩溃恢复 | [`src/journal.rs`](../../../src/journal.rs#L96-L132) |
| feed 校验 | [`src/feed.rs`](../../../src/feed.rs#L94-L159) |
| 下载写入/校验 | [`src/download.rs`](../../../src/download.rs#L46-L115) |

## 附录 B：证据边界

- 本报告没有运行项目，也没有做网络抓包、签名验证或安装实测；P0/P1 结论来自静态代码阅读，建议用一次真实的「坏包」演练验证。
- 未公开技术细节的商业应用（Obsidian/Notion）与 Zed 的内部实现，按要求标注为「推断」；它们的用户可见行为来自官方帮助文档。
- 浏览器（Chrome/Firefox）的更新系统包含大量未公开的服务端策略；本报告只描述公开协议与文档能支撑的部分。
- 「不做代码签名」是任务给定前提，不是本报告对项目安全性的独立判断；SnowShot 仅作为「个人开源项目不签名」的形态参考，其内部更新实现未做核实。[D40]
- SmartScreen 的行为受 Windows 版本、企业策略、Smart App Control 状态影响；本文引用的是 Microsoft 面向开发者的公开说明，具体用户环境可能不同。[D37]
