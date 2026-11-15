# generic-updater 更新流程审查报告

- 审查对象：`e:\code\脚手架\openai-gemini`（Rust 更新器 + Win32 C++ GUI + GitHub Actions 发布链）
- 审查日期：2026-11-15
- 审查方式：**静态代码审查**（本机 PATH 中没有 `cargo`/`rustc`，未能编译与跑单测；GUI 未重新编译）。因此下文所有结论分为两级证据：
  - **【代码可证】**：从代码路径直接推导，不需要运行即可确认。
  - **【需运行验证】**：取决于平台行为、竞态或第三方库实现，给出可复现的验证方法。
- 结论一句话：**更新主链路（检查 → 验签 → 下载 → 校验 → 暂存 → 备份覆盖 → 拉起 → 探测）设计是对的，方向和主流方案一致；真正的问题集中在"失败路径"——回滚链、自更新与事务恢复的相互踩踏、并发互斥、以及失败后应用不再启动。**

---

## 0. 结论摘要

### 0.1 严重度统计

| 级别 | 数量 | 含义 |
|---|---|---|
| P0 | 6 | 会破坏安装目录 / 让更新永久失效 / 用户应用"消失"，建议下个版本必修 |
| P1 | 9 | 特定条件必然出错，或与主流行为明显不符 |
| P2 | 10 | 体验、健壮性、可观测性问题，可排期 |
| 需运行验证 | 5 | 依赖平台/竞态，需实测确认 |

### 0.2 最该先看的 5 条

1. **事务恢复会先删掉自己要用的备份**：`journal::recover()` 在还原前调用 `cleanup_old_images()`，把 `*.old-*`（正是 journal 里记录的自更新备份）删掉，再还原时找不到备份 → 更新器文件丢失 / 回滚静默失效。
2. **回滚失败被 `let _ =` 吞掉，随后删除备份和 journal**：一旦回滚失败（典型原因就是"要还原的文件正在运行"，即更新器自己），错误不可见，且备份被清理，之后无法再恢复。
3. **`--apply` 失败（安装失败 / UAC 被拒 / 拉起失败）后不再启动应用**：GUI 已经 `DestroyWindow` 退出，用户在桌面上看到的是"应用自己关了再也没回来"。
4. **`install()` 的 90 秒重试是"整包重跑"**：重试会把"上一轮已经写入的新文件"当成旧文件备份，破坏备份内容；第二轮再失败时回滚会把新文件"还原"回去。
5. **stdout 写失败会 panic**：`protocol::emit` 用 `println!`，release 下 `panic = "abort"`。用户下载中关掉窗口（管道断开）会让更新器在写下一帧时直接 abort；双击运行（GUI 子系统无 stdout）也会 panic。

### 0.3 与主流方案的关系（详见第 5 节）

- **一致**：独立更新器进程、签名元数据 + 包哈希、先下载暂存再重启替换、启动探测、跳过版本、24h 节流、备份/事务日志。
- **缺失**：代码签名（Authenticode）、断点续传/差分、灰度发布与强制更新/回滚开关、运行期周期检查、跨进程互斥、更新器运行日志、取消下载、macOS/Linux 打包路径。
- 整体判断：**逻辑"符合大众"，但"失败面"比主流实现薄**。主流实现（electron-updater / Tauri / Sparkle / Squirrel / Omaha）在"失败后如何让用户继续用"上花的代码，通常比成功路径更多。

---

## 1. 项目结构与技术栈

```
openai-gemini/
├── Cargo.toml                 # generic-updater crate（Rust 2021，无 async）
├── Cargo.lock
├── build.rs                   # 读 updater.conf → 生成 OUT_DIR/embedded_config.rs
├── updater.conf               # 唯一发布配置，编译期固化
├── config.example.json        # 本地调试用覆盖配置（--config）
├── src/                       # Rust 更新器（约 1.6k 行）
│   ├── main.rs                # CLI 参数解析、模式分发、退出码
│   ├── update.rs              # check / download / apply / clean / recover / skip 的编排
│   ├── feed.rs                # 元数据获取（feed_url / GitHub Release）与解析选包
│   ├── contract.rs            # RSA-PSS 签名信封验签；内置公钥
│   ├── download.rs            # HTTP 文本、流式下载、SHA256、原子落盘
│   ├── install.rs             # zip 解压、文件清单、备份覆盖、回滚、清理
│   ├── journal.rs             # 安装事务日志与崩溃恢复
│   ├── platform.rs            # Win32 FFI：等进程/提权/目录可写/重试
│   ├── protocol.rs            # stdout JSON 状态帧（protocol=2）
│   ├── state.rs               # state.json（24h 时间戳、跳过版本、pending）
│   ├── config.rs              # 配置 + 缓存目录解析
│   └── error.rs               # AppError + is_transient
├── app/                       # C++ Win32 GUI 客户端（gui.cpp 约 770 行）
├── resources/update-trusted-keys.json   # 编译进二进制的可信公钥
├── tools/                     # 密钥生成、feed 签名、配置头生成、脚手架
├── .github/workflows/build.yml# tag → 编译 → 打包 → 哈希 → feed → 签名 → Release
├── demo/                      # 浏览器端交互测试台（含本地静态服务器）
└── docs/                      # 01 功能说明 / 02 接入指南 / 03 决策记录 / skill
```

技术要点：

| 层 | 关键实现 |
|---|---|
| HTTP | `reqwest 0.12` blocking + rustls，**单一 client 总超时 300s**（[update.rs L32-L38](file:///e:/code/脚手架/openai-gemini/src/update.rs#L32-L38)） |
| 版本 | `semver` crate，远端/本地都先 `trim_start_matches('v')` 再 `Version::parse` |
| 签名 | RSA-PSS / SHA-256 / salt=32 / 3072-bit，公钥编译期内嵌，签 payload 原始字节 |
| 安装 | zip → staging → 逐文件"备份+覆盖" → 按清单删除旧文件 → 写 journal |
| 事务 | `journal.json`（未提交/已提交），`--recover` 或任意 `--check` 时回滚 |
| 进程 | GUI 用 `Local\` 互斥体；updater 用 `OpenProcess + WaitForSingleObject` 等父进程；UAC 用 `ShellExecuteW("runas")` |
| 协议 | stdout 一行一条 JSON，GUI 用**手写字符串扫描**解析 |

---

## 2. 更新流程全景

### 2.1 进程与文件布局

```
GuiUpdaterDemo.exe (C++ GUI)                 generic-updater.exe (Rust)
   │  ① CreateProcess + 重定向 stdout ─────────▶ ② --check / --download
   │  ③ 逐行读 JSON 帧 ◀───────────────────────  stdout 状态帧
   │  ④ SpawnApply(--apply --parent-pid N) ───▶ ⑤ 等父进程退出 → 安装 → 拉起新版
   └─ ⑥ DestroyWindow 退出                         ⑦ spawn GuiUpdaterDemo.exe（detached）

缓存：%LOCALAPPDATA%\GenericUpdater\updates\<安装目录 SHA256 前 12 位>\
      state.json / staging/ / backup/ / journal.json / <包名>.zip / .part
安装目录：GuiUpdaterDemo.exe / generic-updater.exe / version.txt / .update-files.json(隐藏)
```

### 2.2 命令与退出码

| 命令 | 入口 | 行为 | 退出码 |
|---|---|---|---|
| `--check [--auto]` | [update.rs L118-L167](file:///e:/code/脚手架/openai-gemini/src/update.rs#L118-L167) | 恢复事务 → 读本地版本 → （auto 时看 24h）→ 取 feed → 验签 → 解析 → 比较版本 → 发帧 | 0 无更新 / 2 有更新 |
| `--download` | [update.rs L193-L278](file:///e:/code/脚手架/openai-gemini/src/update.rs#L193-L278) | 再取一次 feed → 下载（复用已存在的同哈希缓存）→ 校验 → 解压 staging → 写 pending | 0 / 2 / 1 |
| `--apply --parent-pid N` | [update.rs L308-L394](file:///e:/code/脚手架/openai-gemini/src/update.rs#L308-L394) | 恢复事务 → 读 pending staging → 等父进程 → 权限检查/UAC → 安装（90s 重试）→ 拉起 → 3s 存活探测 → 清理 | 2 / 1 |
| `--recover` | [update.rs L108-L116](file:///e:/code/脚手架/openai-gemini/src/update.rs#L108-L116) | 只做事务恢复 | 0 |
| `--clean` | [update.rs L404-L411](file:///e:/code/脚手架/openai-gemini/src/update.rs#L404-L411) | 删除整个缓存目录（含 state.json） | 0 |
| `--skip <ver>` | [update.rs L66-L73](file:///e:/code/脚手架/openai-gemini/src/update.rs#L66-L73) | 写 `state.skipped_version` | 0 |
| `--verify-feed <json>` | [update.rs L77-L106](file:///e:/code/脚手架/openai-gemini/src/update.rs#L77-L106) | 本地验签并打印 keyId/version | 0 / 1 |

### 2.3 检查更新（check）调用链

```
main()                                  main.rs L38-L133
 └ Config::embedded()                   config.rs L72-L93（安装目录 = 当前 exe 目录）
 └ update::run → check(cfg, auto)       update.rs L118
    ├ UpdateState::load(cache/state.json)                state.rs L37
    ├ journal::recover(cache, install_dir)               journal.rs L96
    │   ├ cleanup_old_images(install_dir)                journal.rs L68
    │   └ 若 journal 未提交 → 逐条 copy backup / 删除新增文件
    ├ cfg.local_version()  ← 读 install_dir/version.txt   config.rs L160
    ├ auto && !auto_check_due(24h) → report_cached()      update.rs L125-L128, L170
    ├ emit(Checking)
    ├ build_client()  ← 总超时 300s
    ├ feed::resolve()                                    feed.rs L171
    │   ├ 有 feed_url：http_text → contract::unwrap_feed → parse
    │   └ 无 feed_url：GET api.github.com/repos/{repo}/releases/latest
    │                 → 在 assets 里找 feed_name → 下载 feed → unwrap → parse
    ├ state.mark_checked() + save（失败不刷新时间戳）
    ├ 记录 last_remote_version / notes / size
    ├ feed.version <= local → emit(Idle)                 update.rs L150
    └ 否则 → emit(Available, skipped=…)                  update.rs L155-L166
```

关键实现点：

- **验签**：[contract.rs L108-L118](file:///e:/code/脚手架/openai-gemini/src/contract.rs#L108-L118)。`require_signature=true` 时，非信封的明文 feed 直接拒绝；信封签名对象是 payload 原始字节，公钥来自编译进二进制的 `resources/update-trusted-keys.json`。
- **选包**：[feed.rs L128-L135](file:///e:/code/脚手架/openai-gemini/src/feed.rs#L128-L135)。优先 `variant` 匹配且非 installer；找不到就退化为"任意非 installer"。
- **24h 缓存提示**：[update.rs L170-L191](file:///e:/code/脚手架/openai-gemini/src/update.rs#L170-L191)。不联网时用 `last_remote_version` 与本地版本比较，避免假报"已是最新"。

### 2.4 下载与暂存（download）

```
download()                                     update.rs L193
 ├ journal::recover
 ├ feed::resolve（重新取一次，可能和刚才 check 的版本不同）
 ├ state 记录 last_remote_*
 ├ feed.version <= local → Idle
 ├ dest = cache/<asset_name>
 ├ 若 dest 已存在且 SHA256 匹配 → 跳过下载      update.rs L229-L232
 ├ download_to(source, dest, sha256, size, 进度回调)   download.rs L46
 │   ├ 写 dest.part，边读边算 SHA256
 │   ├ size 不符 → 删 .part 报错
 │   ├ hash 不符 → 删 .part 报错
 │   └ 原子改名 .part → dest
 ├ emit(Verifying)
 ├ 清空 staging → prepare_staging(dest, staging)      install.rs L16
 │   ├ .zip → 逐 entry 解压（mangled_name 防穿越）
 │   ├ .exe → 直接 copy
 │   └ 其他扩展名 → 报错
 └ 写 state.pending_version/notes/staging → emit(Ready)
```

### 2.5 安装与重启（apply）

```
apply()                                                   update.rs L308
 ├ journal::recover
 ├ 从 state.pending_staging 取目录（不存在则报"没有待安装的更新"）
 ├ emit(Applying)
 ├ parent_pid → platform::wait_for_process(30s)            platform.rs L54
 ├ install_dir 不可写？
 │   ├ 已提权 → 报错
 │   └ 未提权 → ShellExecuteW("runas") 重启自己（追加 --elevated）→ 立即返回 2
 ├ platform::retry_until(90s, install::install)            platform.rs L160
 │   ├ 第一遍：遍历 staging，生成 Plan（source/target/backup/rename_old）
 │   ├ 写 journal（committed=false）
 │   ├ 第二遍：逐个 copy 旧文件→backup、copy 新文件→target
 │   ├ remove_obsolete(旧清单-新清单)
 │   ├ write_installed(.update-files.json)
 │   └ journal.committed = true
 ├ relaunch(cfg)  ← detached spawn exe_name                 update.rs L413
 ├ probe_started(3s)：进程还活着算成功                     update.rs L282
 │   └ 失败 → restore_previous() → 再拉起旧版 → Failed
 ├ cleanup(staging/backup/*.old-*) + clean_downloads
 └ state.clear_pending() → emit(Ready)
```

### 2.6 崩溃恢复（recover）

```
journal::recover(cache_dir, install_dir)      journal.rs L96
 ├ cleanup_old_images(install_dir)            ← 先删所有 *.old-*（问题所在，见 BUG-01）
 ├ journal 不存在 → Clean
 ├ journal.committed → 删 journal → Committed
 └ 未提交 → 逐条：
     ├ backup 存在 → copy 回 target（restored++）
     └ backup 为 None → 删除 target（新增文件）
   → 删 journal → RolledBack(n)
```

**触发时机**：`--recover`、每次 `--check`、每次 `--download`、每次 `--apply` 开头。也就是说**用户每次启动应用、30 秒后自动检查时都会走一遍回滚逻辑**。

---

## 3. 确定的 bug（按严重度）

> 编号规则：BUG-xx。每条包含：位置 / 触发条件 / 实际后果 / 依据 / 建议。
> "确定的 bug"= 不运行也能从代码路径推出；"需运行验证"另见第 4 节。

### P0

#### BUG-01 事务恢复先删除自己要用的备份（自更新场景会丢失更新器）

- 位置：[journal.rs L96-L97](file:///e:/code/脚手架/openai-gemini/src/journal.rs#L96-L97) 与 [journal.rs L110-L128](file:///e:/code/脚手架/openai-gemini/src/journal.rs#L110-L128)；备份命名与来源见 [install.rs L190-L199](file:///e:/code/脚手架/openai-gemini/src/install.rs#L190-L199)、[install.rs L227-L238](file:///e:/code/脚手架/openai-gemini/src/install.rs#L227-L238)。
- 触发条件：包内包含 `generic-updater.exe`（也就是每次更新器自更新），安装在写入 journal 之后、`std::fs::copy` 覆盖更新器之前中断（断电、任务管理器结束、杀软拦截、`remove_dir_all` 之外的任意 abort）。
- 机制：
  1. `install()` 对"正在运行的那个 exe"走 `rename_old` 分支，把 `generic-updater.exe` 改名为 `generic-updater.exe.old-<pid>`，并在 journal 里把 `backup` 记成这个 `.old-<pid>` 路径。
  2. 进程中断后，下次任意 `--check`/`--apply` 进入 `recover()`。
  3. `recover()` **第一步**调用 `cleanup_old_images()`，把所有 `*.old-*` 删掉——包括它马上要用来还原的备份。
  4. 随后 `if backup_path.exists()` 为假，静默跳过；最后删掉 journal。
- 实际后果：`generic-updater.exe` 缺失（或停留在半写状态），而 journal 已删除 → **用户此后再也无法更新，也没有任何提示**，只能重装。
- 依据：`cleanup_old_images` 用文件名 `contains(".old-")` 匹配，`recover` 在它之后才读 journal；两处无任何排除逻辑。
- 建议：`recover()` 先读 journal，再清理旧镜像；清理时跳过 journal 里引用到的备份路径；还原前对"目标是当前运行 exe"的情况也使用"改名再写"的策略。

#### BUG-02 回滚失败被静默吞掉，随后删除备份与 journal

- 位置：[update.rs L396-L402](file:///e:/code/脚手架/openai-gemini/src/update.rs#L396-L402)；[install.rs L268-L294](file:///e:/code/脚手架/openai-gemini/src/install.rs#L268-L294)。
- 触发条件：启动探测失败（或拉起失败）需要回滚，而回滚中有文件正被占用——最典型就是"新包的 `generic-updater.exe` 正在运行、要把它覆盖回旧版"。
- 机制：`restore_previous()` 是 `let _ = install::rollback(ctx);` → 复制失败被丢弃；紧接着 `install::cleanup(ctx)` 删掉 `backup/`；最后删掉 journal。
- 实际后果：安装目录处于"新更新器 + 旧主程序"的混合状态，错误完全不可见（stdout 无人读，stderr 在 GUI 子系统下也无处可去），且备份与 journal 都没了 → 不可恢复。
- 建议：回滚失败必须保留备份与 journal，并把失败原因写进状态帧/日志；对运行中的镜像使用"改名为 `.old-`，重启后再写"的两阶段策略。

#### BUG-03 `--apply` 失败后应用不会回来

- 位置：[gui.cpp L426-L435](file:///e:/code/脚手架/openai-gemini/app/src/gui.cpp#L426-L435)（`SpawnApply()` 成功后立刻 `DestroyWindow`）；[update.rs L359-L373](file:///e:/code/脚手架/openai-gemini/src/update.rs#L359-L373)（安装失败直接 `return Err`，不重启）；只有探测失败分支会 `relaunch`（[update.rs L375-L381](file:///e:/code/脚手架/openai-gemini/src/update.rs#L375-L381)）。
- 触发条件（任一）：
  - 安装阶段报错（文件被占用超过 90s、磁盘满、清单写入失败、包内缺少主 exe）；
  - 目录不可写且用户拒绝 UAC / 提权进程用不同管理员账户启动；
  - `relaunch()` 找不到 `cfg.exe_name`（包打包漏了主程序）。
- 实际后果：GUI 已退出，updater 失败后退出码 1，桌面上什么也不剩。用户感知是"点了重启软件，程序自己消失了"。
- 依据：`apply()` 的三个失败出口（UAC、install、relaunch）都只 `fail(...)`，没有 `relaunch(cfg)`。
- 建议：所有 apply 失败路径在返回前都尝试拉起旧版（幂等），并把失败原因通过 `--status`/日志文件留给下一次启动展示。

#### BUG-04 90 秒重试会破坏备份，"回滚"可能把新文件还原回去

- 位置：[update.rs L359-L364](file:///e:/code/脚手架/openai-gemini/src/update.rs#L359-L364)（整包重试）；[install.rs L185-L201](file:///e:/code/脚手架/openai-gemini/src/install.rs#L185-L201)（每轮重新规划 backup）；[install.rs L239-L245](file:///e:/code/脚手架/openai-gemini/src/install.rs#L239-L245)（无条件用当前 target 覆盖 backup）。
- 触发条件：第一轮安装已复制了部分文件后遇到可重试错误（`PermissionDenied`/`WouldBlock`），第二轮仍在 90s 窗口内再次失败。
- 机制：
  - 第一轮：`app.exe` 旧文件 → `backup/app.exe`；`app.exe` ← 新文件。
  - 第二轮重新规划：`app.exe` 现在存在 → `backup = backup/app.exe`；循环里 `std::fs::copy(target, backup)` 把**新文件**写进备份。
  - 第二轮失败退出，journal 仍未提交。
  - 下次 `recover()`（或 `--apply` 重试）把 `backup/app.exe`（新文件）复制回去。
- 实际后果：回滚"成功"但文件是新的，形成新 exe + 旧 DLL/资源的混合体，且 journal 被删除，不能再次回滚。
- 另外：第一轮新建的文件（原 target 不存在，`backup=None`）在第二轮变成"已存在"，于是也进入备份；回滚后这些新文件不会被删除（与 BUG-05 叠加）。
- 建议：把"备份"与"覆盖"拆成两个幂等阶段（先全量备份并记 journal，再做覆盖），重试时只重试覆盖；或者重试只针对失败的那个文件，而不是重跑 `install()`。

#### BUG-05 旧文件删除不入 journal，回滚不删除新增文件

- 位置：[install.rs L254](file:///e:/code/脚手架/openai-gemini/src/install.rs#L254)（`remove_obsolete` 在 commit 之前执行，但删除项没有写入 journal）；journal 只由 `plans`（staging 内文件）生成，见 [install.rs L204-L216](file:///e:/code/脚手架/openai-gemini/src/install.rs#L204-L216)；`rollback()` 只遍历 `backup/`，见 [install.rs L268-L285](file:///e:/code/脚手架/openai-gemini/src/install.rs#L268-L285)。
- 触发条件：
  - 在 `remove_obsolete` 之后、`journal.commit` 之前中断（断电/被杀）；
  - 或走 `restore_previous()` 的探测失败回滚。
- 实际后果：
  - 被 `remove_obsolete` 删除的旧文件**不会**被恢复 → 回滚后旧版本缺少文件，可能启动不了；
  - 新版本新增的文件**不会**被删除 → 目录里留下"新版本的残留文件"；
  - `restore_previous()` 随后还删除 journal（[update.rs L401](file:///e:/code/脚手架/openai-gemini/src/update.rs#L401)），使这两个问题无法再被补救。
- 依据：journal entry 只有 `{target, backup}`，且只覆盖 staging 中存在、target 将被写入的路径。
- 建议：journal 增加 `deleted` 列表（删除前把旧文件移动到 `backup/` 或记录其备份），回滚时恢复；回滚时补一遍"journal 中 backup=None 的 target 删除"。

#### BUG-06 更新器没有任何跨进程互斥

- 位置：全仓库无锁文件/命名互斥体（`src/` 内检索 `lock|create_new` 无结果，见 `work/证据索引.md`）；GUI 的互斥体是 `Local\GenericUpdaterDemo.SingleInstance`（[gui.cpp L50](file:///e:/code/脚手架/openai-gemini/app/src/gui.cpp#L50)），只覆盖**同一登录会话内**的 GUI。
- 触发条件（任一即可）：
  - 同一用户通过两个 RDP/会话各开一个 GUI，先后点"重启软件" → 两个 `--apply` 并发；
  - 命令行/脚本直接调用 `--download`/`--apply` 与 GUI 并发；
  - `--skip` 与 `--apply` 并发（`SkipCurrentVersion` 就在 UI 线程同步跑，不置 busy，[gui.cpp L586-L594](file:///e:/code/脚手架/openai-gemini/app/src/gui.cpp#L586-L594)）。
- 实际后果：
  - 两个 `--apply` 同时备份/覆盖 → journal 互相覆盖、备份被对方改写、最终回滚数据错乱；
  - 两个 `--download` 同时写同一个 `.part` → 内容交错，双方都哈希失败；
  - `state.json` 读改写竞争：`--skip` 可能把 `--apply` 刚清掉的 `pending_*` 又写回去。
- 建议：在缓存目录加独占锁（`OpenOptions::new().create_new(true)` 写 PID + 超时清理，或命名互斥体），所有会写状态的模式统一获取。

### P1

#### BUG-07 `--skip` 无法取消，GUI 提示语是错的

- 位置：[update.rs L66-L73](file:///e:/code/脚手架/openai-gemini/src/update.rs#L66-L73)（只会 `skipped_version = Some(version)`）；[gui.cpp L483-L490](file:///e:/code/脚手架/openai-gemini/app/src/gui.cpp#L483-L490)（提示"可点「跳过此版本」恢复提示"）。
- 触发条件：用户跳过某版本后想恢复。
- 实际后果：再点一次"跳过此版本"仍然是 `--skip <同一版本>`，状态不变；界面上没有任何"取消跳过"的路径。提示语与行为矛盾。
- 建议：增加 `--unskip`（或 `--skip ""` 清空），GUI 在 skipped 状态下把按钮文案改成"取消跳过"。

#### BUG-08 脚手架 / CI / 客户端三处 platform 与 feed 名不一致

- 位置：[tools/scaffold-update.ps1 L77](file:///e:/code/脚手架/openai-gemini/tools/scaffold-update.ps1#L77)（`feed_name=latest-version-$Platform.json`）；[build.yml L109](file:///e:/code/脚手架/openai-gemini/.github/workflows/build.yml#L109)（zip 名硬编码 `windows-x64`）；[build.yml L136-L153](file:///e:/code/脚手架/openai-gemini/.github/workflows/build.yml#L136-L153)（feed 文件名、`platform`、`variant` 全部硬编码）。
- 触发条件：`scaffold-update.ps1 -Platform windows-arm64`（文档宣称支持多平台，见 `docs/01` 第 3 节）。
- 实际后果：目标项目客户端去找 `latest-version-windows-arm64.json`，CI 永远只产出 `latest-version-windows-x64.json` → **更新永久失败**（"Release 中找不到元数据资产"）。
- 另外：`updater.conf` 的 `feed_name` 若被改成别的名字，CI 也不会跟随。
- 建议：CI 从 `updater.conf` 读 `platform`/`variant`/`feed_name` 生成文件名与 feed 内容；脚手架与 workflow 共用同一份变量。

#### BUG-09 下载无断点续传、无重试，且 300s 总超时会掐死大包

- 位置：[update.rs L32-L38](file:///e:/code/脚手架/openai-gemini/src/update.rs#L32-L38)（`ClientBuilder::timeout(300s)`，reqwest blocking 是**整请求**超时，含读完 body）；[download.rs L46-L121](file:///e:/code/脚手架/openai-gemini/src/download.rs#L46-L121)（无 `Range`、无重试）。
- 触发条件：网络抖动；或包体较大、带宽较低，使"连接+传输"超过 300s（粗略地，>300MB@1MB/s 必失败）。
- 实际后果：任意一次抖动都要从头再来；大包在慢网上**永远装不上**。下载失败后 `.part` 残留（虽会被下一次覆盖）。
- 建议：改用 `connect_timeout` + 读超时（`read_timeout`），下载失败按退避重试 N 次，支持 `Range` 续传（`.part` 已存在时带 `Range: bytes=<len>-` 并校验总长/哈希）。

#### BUG-10 UAC 提权后可能读不到原进程的 pending 状态

- 位置：[config.rs L114-L140](file:///e:/code/脚手架/openai-gemini/src/config.rs#L114-L140)（cache 根取 `LOCALAPPDATA`）；[update.rs L343-L355](file:///e:/code/脚手架/openai-gemini/src/update.rs#L343-L355)（提权重启自身）。
- 触发条件：安装目录在 `Program Files` 之类受保护位置，当前是标准用户，UAC 对话框里输入了**另一个管理员账户**的凭据（而不是同一账户提权）。
- 实际后果：提权后的进程 `LOCALAPPDATA` 指向另一个用户，`state_path()`/`staging_dir()` 全部变化 → 读不到 `pending_staging` → 报"没有待安装的更新"，更新失败且应用已被关掉（叠加 BUG-03）。
- 建议：把 staging 路径通过命令行显式传给提权进程（`--staging <path> --state <path>`），或把缓存目录固定在安装目录旁的受控位置。

#### BUG-11 选包回退过宽；feed 缺 platform/product 时不校验

- 位置：[feed.rs L128-L135](file:///e:/code/脚手架/openai-gemini/src/feed.rs#L128-L135)；[feed.rs L104-L123](file:///e:/code/脚手架/openai-gemini/src/feed.rs#L104-L123)。
- 触发条件：一个 feed 里同时放了多个 variant / 多架构包；或发布方漏写 `product`/`platform` 字段。
- 实际后果：`variant` 不匹配时会静默选"任意非 installer 包"，可能装上另一个分发形态甚至另一个架构的包；缺字段时签名仍通过（签名保护的是"这份 feed 没被改"，不是"字段齐全"）。
- 建议：去掉 variant 回退（找不到就报错）；`product`/`platform` 设为必填；`PackageJson` 增加 `platform`/`arch` 并逐包校验。

#### BUG-12 本地版本比远端新时，界面"最新版本"显示更旧的号

- 位置：[update.rs L150-L152](file:///e:/code/脚手架/openai-gemini/src/update.rs#L150-L152)（`emit(Idle, &version, …)` 里 `version` 是**远端**版本）；[gui.cpp L497-L503](file:///e:/code/脚手架/openai-gemini/app/src/gui.cpp#L497-L503)（`if (!result->version.empty()) SetWindowTextW(gLatestVersion, result->version)`）。
- 触发条件：用户手动装了比 Release 更新的构建（或 Release 回滚/热修了旧版本号），本地 `version.txt` > feed.version。
- 实际后果：界面显示"当前已是最新版本"，但"最新版本"一栏是比"软件版本"更低的号（如本地 0.5.9、最新版本显示 0.5.8），用户会认为更新坏了。
- 建议：`Idle` 帧带 `version` 时客户端取 `max(local, remote)` 显示，或 Idle 时显式回传本地版本。

#### BUG-13 检查失败但没发终态帧时，界面永久停在"正在检查更新…"

- 位置：[gui.cpp L437-L511](file:///e:/code/脚手架/openai-gemini/app/src/gui.cpp#L437-L511)（只处理 `Failed`/`Available`/`Idle`/`Ready`，没有 else）；[update.rs L130-L141](file:///e:/code/脚手架/openai-gemini/src/update.rs#L130-L141)（`emit(Checking)` 之后的 `state.save()?` 等失败会直接 `return Err`，不发 `Failed`）。
- 触发条件：缓存目录突然不可写（磁盘满、安全软件锁目录、权限被改）等发生在 `emit(Checking)` 之后的错误；或 updater 因 panic 中止。
- 实际后果：按钮恢复可用，但状态栏永远停在"正在检查更新…"，用户不知道发生了什么。
- 相关：`result->state.empty()` 时提示"无法启动更新器 generic-updater.exe"（[gui.cpp L442-L447](file:///e:/code/脚手架/openai-gemini/app/src/gui.cpp#L442-L447)），但"进程起来了、只是没输出帧"也会走到这里，提示误导。
- 建议：`OnJobDone` 增加"非终态 → 视为失败"的兜底；GUI 同时用退出码（非 0/2）判断失败。

#### BUG-14 `state.json` 非原子写，崩溃会丢 pending

- 位置：[state.rs L44-L59](file:///e:/code/脚手架/openai-gemini/src/state.rs#L44-L59)（`std::fs::write` 直接截断写入，无 temp+rename/fsync）。
- 触发条件：保存过程中断电/被杀。
- 实际后果：文件半写 → `load()` 解析失败 → 静默回落为 `Default` → 丢失 `pending_*`（已下载的更新作废，需要重新下载）、丢失 24h 时间戳与跳过记录。**没有任何告警**。
- 建议：写 `state.json.tmp` → `rename`；解析失败时备份坏文件并打日志。

#### BUG-15 已下载待安装的状态无法跨进程恢复

- 位置：GUI 只把 pending 放在内存 `gState.pendingVersion`（[gui.cpp L94](file:///e:/code/脚手架/openai-gemini/app/src/gui.cpp#L94)、[L466](file:///e:/code/脚手架/openai-gemini/app/src/gui.cpp#L466)），从不读取 updater 的 `state.json`；Rust 侧 `--check` 也不会报告"有一个已就绪的 pending"。
- 触发条件：下载完成（Ready）后用户关闭窗口，或下载完成前 GUI 崩溃 / 被结束。
- 实际后果：重新打开后界面显示"立即安装"，点一下又重新下载（有缓存时很快，但用户看到的是"上次白下了"）；如果 GUI 在 `StartInstall` 里已经 `pendingVersion.clear()`（[gui.cpp L422](file:///e:/code/脚手架/openai-gemini/app/src/gui.cpp#L422)）而下载失败，则彻底失去待安装状态。
- 建议：updater 增加 `--status`（输出 pending/ready/version/last_check），GUI 启动时先查一次；`Available` 帧带 `pending:true`。

### P2

#### BUG-16 `relaunch_elevated` 拼接参数不加引号

- 位置：[platform.rs L108-L123](file:///e:/code/脚手架/openai-gemini/src/platform.rs#L108-L123)（`arguments.join(" ")`）。
- 触发条件：调试时 `--config "C:\my dir\cfg.json"`（或任何含空格的参数）走到提权分支。
- 后果：参数被拆成多个，提权进程配置解析失败（`--config` 后面的路径被截断）。
- 建议：按 Windows 命令行规则逐个加引号并转义。

#### BUG-17 `directory_writable` 对不存在的目录直接判不可写

- 位置：[platform.rs L147-L157](file:///e:/code/脚手架/openai-gemini/src/platform.rs#L147-L157)（只 `fs::write` 探针，不像 `config.rs` 的 `writable()` 那样先 `create_dir_all`）。
- 触发条件：安装目录被删除/移动后仍从某处调用 `--apply`。
- 后果：误判为"不可写"→ 走 UAC 提权 → 提权后仍然失败并报"安装目录仍不可写"，错误信息偏离真实原因。
- 建议：先 `create_dir_all` 再探测。

#### BUG-18 旧文件删除后不清理空目录

- 位置：[install.rs L119-L136](file:///e:/code/脚手架/openai-gemini/src/install.rs#L119-L136)（只删文件）。
- 后果：升级多个版本后安装目录残留空目录（如已废弃的 `plugins/`）。
- 建议：删除文件后自底向上尝试 `remove_dir`。

#### BUG-19 zip 解压与 feed 响应没有大小上限

- 位置：[install.rs L16-L57](file:///e:/code/脚手架/openai-gemini/src/install.rs#L16-L57)（无解压总量上限）；[download.rs L21-L37](file:///e:/code/脚手架/openai-gemini/src/download.rs#L21-L37)（`resp.text()` 无长度上限）。
- 触发条件：`require_signature=false` 时被中间人替换 feed（见 RISK-01），或发布方误传超大包。
- 后果：zip bomb / 超大 feed 耗尽磁盘或内存。
- 建议：解压时累计 `entry.size()` 并设上限（如 ≤ 包大小 × 4 且 ≤ 2GB）；`http_text` 限制读取长度。

#### BUG-20 `is_envelope` 用字符串包含判断信封

- 位置：[contract.rs L47-L49](file:///e:/code/脚手架/openai-gemini/src/contract.rs#L47-L49)。
- 触发条件：明文 feed 的 `notes` 里恰好同时出现 `"payload"` 和 `"signature"` 两个片段。
- 后果：被误判为信封 → JSON 解析/schema 校验失败 → 检查报错（要求签名时应该报错，不要求签名时属于误伤）。
- 建议：解析 JSON 后按对象键判断，而不是字符串包含。

#### BUG-21 `--check` 对"已跳过"的版本仍返回退出码 2

- 位置：[update.rs L155-L166](file:///e:/code/脚手架/openai-gemini/src/update.rs#L155-L166)。
- 后果：脚本化调用方（按文档只看退出码）会认为"有更新"，忽略 `skipped` 字段直接下载。GUI 里是靠 `skipped` 字段禁用的，CLI 契约与 GUI 行为不一致。
- 建议：skipped 时退出码改为 0，或在文档中明确规定"调用方必须读 skipped"。

#### BUG-22 GUI 的 `Available`/`Idle` 分支按钮可见性不完整

- 位置：[gui.cpp L478-L509](file:///e:/code/脚手架/openai-gemini/app/src/gui.cpp#L478-L509)。
- 触发条件：先到 Ready（`gInstall` 被隐藏、`gRestart` 显示），再点"检查更新"得到 Available 或 Idle。
- 后果：两个按钮占同一位置（470,424），可能出现重叠或"有更新但看不到立即安装"。`Idle` 分支也不隐藏 `gRestart`。
- 建议：进入检查结果时统一重置两个按钮的可见性。

#### BUG-23 下载中关窗口会 abort 更新器（详见 BUG-24 的根因）

- 位置：[gui.cpp L672-L675](file:///e:/code/脚手架/openai-gemini/app/src/gui.cpp#L672-L675)（`WM_CLOSE` 直接销毁窗口，不检查是否 busy）；根因见 BUG-24。
- 后果：用户以为"取消"，实际 updater 会在下一次 `emit` 时 abort，留下 `.part`；如果发生在 `--apply` 阶段，可能停在"安装完成但未清理"。
- 建议：busy 时拦截关闭（提示"更新正在进行"），或让 updater 在 stdout 断开时优雅降级为"继续完成当前阶段"。

#### BUG-24 stdout 写失败会 panic/abort

- 位置：[protocol.rs L104-L109](file:///e:/code/脚手架/openai-gemini/src/protocol.rs#L104-L109)（`println!` + `flush`）；[Cargo.toml L32](file:///e:/code/脚手架/openai-gemini/Cargo.toml#L32)（`panic = "abort"`）。
- 触发条件：
  - GUI 在下载/安装中退出，子进程 stdout 管道读端关闭 → 下一次 `println!` 返回 EPIPE；
  - 用户双击 `generic-updater.exe`（GUI 子系统、无 std 句柄）→ 第一次 `println!` 就失败。
- 后果：Rust 的 `println!` 在写失败时 panic；`panic=abort` 直接终止进程。下载中断、`--verify-feed` 静默崩溃、安装流程可能停在任意阶段。
- 建议：用 `writeln!(io::stdout().lock(), …)` 并忽略错误；或把帧写到显式传入的 `--pipe`/日志文件；`--verify-feed` 在没有控制台时写文件而不是 stdout。

#### BUG-25 文档与实现漂移（会误导接入方）

| 文档位置 | 文档说法 | 实际代码 |
|---|---|---|
| [README.md L54](file:///e:/code/脚手架/openai-gemini/README.md#L54) | `require_signature=false` | [updater.conf L11](file:///e:/code/脚手架/openai-gemini/updater.conf#L11) 是 `true` |
| [README.md L85-L86](file:///e:/code/脚手架/openai-gemini/README.md#L85-L86) | 安装后保留缓存里的 `installed.json` | 已改为安装目录的隐藏文件 `.update-files.json`（[install.rs L64](file:///e:/code/脚手架/openai-gemini/src/install.rs#L64)） |
| [README.md L96](file:///e:/code/脚手架/openai-gemini/README.md#L96) | "8 秒内没起来"算失败 | 实际 3 秒（[update.rs L283](file:///e:/code/脚手架/openai-gemini/src/update.rs#L283)） |
| [README.md L140](file:///e:/code/脚手架/openai-gemini/README.md#L140) | "没配 secret 的时候 feed 就是明文 JSON——这是默认状态" | `require_signature=true` 且 CI 在缺 secret 时直接失败（[build.yml L71-L87](file:///e:/code/脚手架/openai-gemini/.github/workflows/build.yml#L71-L87)），不会产出明文 feed |
| [config.example.json L4](file:///e:/code/脚手架/openai-gemini/config.example.json#L4) | 调试配置 `require_signature=false` | 合理，但与 README 混用会让人以为生产默认不验签 |
| [app/build-config.h L8](file:///e:/code/脚手架/openai-gemini/app/build-config.h#L8) | 构建版本 0.5.5 | 项目已到 0.5.9（该文件被 `.gitignore` 且构建时重生成，但本地已有 `dist/GuiUpdaterDemo.exe` 是 0.5.5） |
| [CHANGELOG.md L6-L16](file:///e:/code/脚手架/openai-gemini/CHANGELOG.md#L6-L16) | 有 0.5.7 / 0.5.9 | 缺 0.5.8 小节；发布 0.5.8 时 CI 只能回退用 commit message 当 notes（[build.yml L131-L133](file:///e:/code/脚手架/openai-gemini/.github/workflows/build.yml#L131-L133)） |

建议：以 `updater.conf` 为唯一事实源，README 里改成引用；CHANGELOG 补齐 0.5.8；本地 `dist/` 不作为交付物。

---

## 4. 需运行验证的点

### V-01 `is_transient()` 是否真的能捕获"文件被占用"（决定 90s 重试有没有用）

- 位置：[error.rs L24-L34](file:///e:/code/脚手架/openai-gemini/src/error.rs#L24-L34)。
- 疑点：`std::fs::copy` 覆盖被占用文件时，Windows 返回 `ERROR_SHARING_VIOLATION (32)` / `ERROR_ACCESS_DENIED (5)`。Rust 的 `decode_error_kind` 对 32 的映射随版本有过变化；如果映射为 `Uncategorized`，`is_transient()` 为 false → **`retry_until(90s)` 一次都不会重试**，安装直接失败（再叠加 BUG-03，应用不回来）。
- 验证：写一个单元测试/小程序：打开一个文件并持有写句柄，另一个进程 `std::fs::copy` 覆盖它，打印 `io::ErrorKind`。或直接实测 `--apply` 时故意占用 `GuiUpdaterDemo.exe`。
- 影响：若映射不成立，则 README "文件占用重试 90s 兜底" 是无效描述，BUG-03 的触发概率显著上升。

### V-02 3 秒探测与 GUI 互斥体 6 秒重试的时序

- 位置：[update.rs L282-L285](file:///e:/code/脚手架/openai-gemini/src/update.rs#L282-L285)（3s）；[gui.cpp L697-L716](file:///e:/code/脚手架/openai-gemini/app/src/gui.cpp#L697-L716)（40 × 150ms = 6s 重试互斥体）。
- 疑点：新版 GUI 若在 6s 内拿不到互斥体（旧 GUI 因某个消息处理卡住未退干净），会弹"程序已经在运行了"并退出 → updater 3 秒探测看到进程已退出 → 判定失败 → 回滚。旧 GUI 正常路径是"销毁窗口后立即退出"，所以概率低，但 RDP/慢机器/杀软扫描期间可能触发。
- 验证：在旧 GUI `WM_CLOSE` 前人为 `Sleep(7000)`，再走完整更新，看是否回滚。

### V-03 `remove_obsolete` 与用户文件的边界

- 位置：[install.rs L119-L136](file:///e:/code/脚手架/openai-gemini/src/install.rs#L119-L136)。
- 疑点：只按上一份清单删除，理论上不会碰用户文件；但如果用户手动把 `version.txt`/清单替换，或从旧安装复制目录，可能误删。需实测"用户自建文件 + 旧清单"组合。

### V-04 UAC 提权分支的实际行为

- 位置：[update.rs L335-L355](file:///e:/code/脚手架/openai-gemini/src/update.rs#L335-L355)、[platform.rs L99-L140](file:///e:/code/脚手架/openai-gemini/src/platform.rs#L99-L140)。
- 需要实测：标准用户 + 安装在 `Program Files` 下；分别验证"同一账户提权"（应成功）与"另一管理员账户提权"（预期失败，见 BUG-10）；以及用户点"否"时应用是否消失（BUG-03）。

### V-05 24h 缓存与系统时钟回拨

- 位置：[state.rs L62-L70](file:///e:/code/脚手架/openai-gemini/src/state.rs#L62-L70)（`saturating_sub`）。
- 疑点：系统时间被调早后，`now - last` 饱和为 0，自动检查会被冻结直到时间追上，期间只有手动检查可用。是否可接受需产品决策。

---

## 5. 与主流更新方案对照

> 说明：以下是架构层面的通行做法（不引用具体版本号），用于判断"我们的更新逻辑是否符合大众"。

| 维度 | 本项目 | electron-updater | Tauri v2 updater | Sparkle 2 (macOS) | Squirrel / Omaha |
|---|---|---|---|---|---|
| 元数据 | 签名 JSON feed（RSA-PSS） | `latest.yml` + sha512，可选签名 | `latest.json`（含 platforms 映射）+ minisign 签名 | appcast RSS + EdDSA 签名 | RELEASES / Omaha XML |
| 包校验 | feed 内 sha256 | sha512（yml 内） | 签名 + 平台条目 | 归档签名 | 包哈希 |
| 增量 | 无 | blockmap 差分 | 无（整包） | delta | 差分/组件 |
| 下载 | 阻塞流式，无续传 | 支持断点/重试 | 整包，重试 | 支持 | 支持 |
| 安装 | 备份+覆盖+清单删除，journal | NSIS/Squirrel 安装器 | 安装器（NSIS/MSI） | 替换 .app | Update.exe/组件 |
| 回滚 | 3s 存活探测 + journal | 安装器事务 | 无（依赖安装器） | 保留旧版可回退 | 组件回滚 |
| 灰度/强制 | 无 | 有（staged rollout / channels） | 有限 | phased rollout / critical | 有 |
| 运行期周期检查 | 仅启动一次 | 有 | 应用自行安排 | 有 | 常驻 |
| 代码签名 | **无 Authenticode** | 依赖平台签名 | 依赖平台签名 | Apple 签名 + 公证 | 平台签名 |
| 日志 | **无**（stderr 在 GUI 下丢失） | 有 | 有 | 有 | 有 |
| 跨进程互斥 | **无** | 有 | 有 | 有 | 有 |

**判断**：

1. **符合大众的部分**：签名元数据 + 包哈希 + 独立更新器 + 先暂存后重启 + 备份/journal + 24h 节流 + 跳过版本，这些都是主流做法；"配置编译期固化"是少见但合理的选择（省掉运行期 ini 被改坏的一类问题）。
2. **偏离大众的部分**（也是风险最高的）：
   - 主流实现把 **"更新失败后应用仍可用"** 当第一优先级；本项目在多个失败分支直接让应用消失（BUG-03）。
   - 主流实现都有 **运行期日志**；本项目在 GUI 子系统下 stderr 等于黑洞，出了问题无法排查。
   - 主流实现都有 **下载重试/续传** 与 **灰度/强制更新**；本项目没有。
   - 主流实现不会在"恢复备份"时和"清理临时文件"互相踩（BUG-01/02/04）；这说明事务顺序需要重新设计。
3. **结论**：**逻辑骨架符合大众，工程质量集中在成功路径；失败路径的完备度明显低于主流。** 建议把下一版目标定为"失败可恢复、失败可见、失败后仍能用"，而不是加功能。

### 5.1 建议补充考虑的能力（按收益排序）

1. **代码签名（Authenticode）**：这是 SmartScreen 与杀软误报的根本解，更新器行为本身无法替代。
2. **失败可见**：`%LOCALAPPDATA%\GenericUpdater\logs\updater-YYYYMMDD.log` + 一次只保留 7 天；GUI 增加"查看日志/复制诊断信息"。
3. **失败可恢复**：统一"先改名、后写入、再清理"的两阶段安装；回滚失败不得删备份；`--doctor`（自检安装目录完整性/更新器是否存在/缓存是否可写）。
4. **并发安全**：锁文件 + 过期 PID 清理。
5. **下载可靠性**：Range 续传 + 重试 + 分阶段超时（连接/读）。
6. **灰度与止损**：feed 增加 `rollout`（百分比）、`mandatory`、`minVersion`、`severity`；客户端按安装 ID 稳定分桶；这样出问题能"停止推送"而不是只能发新版本。
7. **运行期检查**：应用常开时每 N 小时检查一次（现在只有启动后一次）。
8. **数据/配置迁移**：如果主程序在安装目录放用户配置，需要 `migrate()` 钩子（按 `from → to` 版本执行一次），否则升级后配置结构变化会崩。
9. **平台**：macOS 需要 `.app` 整包替换 + 签名/公证校验；Linux 需要 AppImage/包管理器策略。当前 `relaunch` 写死 `exe_name`（[update.rs L413-L443](file:///e:/code/脚手架/openai-gemini/src/update.rs#L413-L443)），非 Windows 只能"裸编译"。
10. **可观测的协议版本**：GUI 完全忽略帧里的 `protocol` 字段（[gui.cpp L257-L284](file:///e:/code/脚手架/openai-gemini/app/src/gui.cpp#L257-L284)），未来改协议没有协商能力。

---

## 6. 其他明显 bug 与高风险写法（单独一节）

### 6.1 Rust 侧

| 编号 | 位置 | 问题 | 后果 |
|---|---|---|---|
| R-01 | [update.rs L108](file:///e:/code/脚手架/openai-gemini/src/update.rs#L108) | `fn recover(...) {    match` 花括号与语句同行 | 仅格式问题，建议 `cargo fmt` |
| R-02 | [update.rs L108-L116](file:///e:/code/脚手架/openai-gemini/src/update.rs#L108-L116) | `--recover` 成功后只打印到 stderr，无状态帧 | GUI/脚本无法感知回滚发生过 |
| R-03 | [update.rs L122](file:///e:/code/脚手架/openai-gemini/src/update.rs#L122)、[L123](file:///e:/code/脚手架/openai-gemini/src/update.rs#L123) | `journal::recover(...)?` 与 `local_version()?` 失败时不发 `Failed` 帧 | GUI 状态卡住（BUG-13） |
| R-04 | [config.rs L160-L168](file:///e:/code/脚手架/openai-gemini/src/config.rs#L160-L168) | 本地 `version.txt` 内容非法（如 `1.5.7_snow-shot`）直接返回错误 | **更新永久失败**，且错误提示是"配置错误"，用户无解；建议降级为 0.0.0 + 告警 |
| R-05 | [update.rs L150-L153](file:///e:/code/脚手架/openai-gemini/src/update.rs#L150-L153) | `Idle` 帧回传远端版本 | BUG-12 |
| R-06 | [feed.rs L125](file:///e:/code/脚手架/openai-gemini/src/feed.rs#L125) | `trim_start_matches('v')` 会剥掉所有前导 v（`vv1.0.0` 也能过） | 极低风险，建议只剥一个 |
| R-07 | [feed.rs L197-L235](file:///e:/code/脚手架/openai-gemini/src/feed.rs#L197-L235) | 用 `/releases/latest`（按发布时间，不是最高 semver）；无认证、无 ETag | 热修旧版本会成为"最新"；私有仓库不可用；每次检查全量拉 feed |
| R-08 | [download.rs L21-L37](file:///e:/code/脚手架/openai-gemini/src/download.rs#L21-L37) | 对所有请求都加 `Accept: application/vnd.github+json` | 对非 GitHub 的 feed_url 无害但语义不对 |
| R-09 | [install.rs L59-L61](file:///e:/code/脚手架/openai-gemini/src/install.rs#L59-L61) | `current_exe().canonicalize()` 失败时静默退化为"不用改名" | 自更新会尝试覆盖运行中的 exe，触发 BUG-04 的整包重试 |
| R-10 | [install.rs L227-L238](file:///e:/code/脚手架/openai-gemini/src/install.rs#L227-L238) | 覆盖前 `let _ = remove_file(old)` 忽略错误 | 旧 `.old-*` 未删掉时 rename 失败，报错但不说明原因 |
| R-11 | [install.rs L254-L257](file:///e:/code/脚手架/openai-gemini/src/install.rs#L254-L257) | 清单写入与 commit 之间没有保护 | BUG-05 |
| R-12 | [journal.rs L110-L128](file:///e:/code/脚手架/openai-gemini/src/journal.rs#L110-L128) | 还原 `backup` 缺失时静默跳过且计数 +1 | 统计"已回滚 N 个文件"会虚高，掩盖失败 |
| R-13 | [platform.rs L160-L172](file:///e:/code/脚手架/openai-gemini/src/platform.rs#L160-L172) | 只按 `is_transient()` 判断重试 | 见 V-01 |
| R-14 | [protocol.rs L56-L80](file:///e:/code/脚手架/openai-gemini/src/protocol.rs#L56-L80) | 进度帧每 64KB 一条，无节流 | 大包时 GUI 消息队列压力（几十万条），建议 ≥1% 或 ≥200ms 节流 |
| R-15 | [state.rs L44-L59](file:///e:/code/脚手架/openai-gemini/src/state.rs#L44-L59) | 无原子写 | BUG-14 |
| R-16 | [update.rs L288-L306](file:///e:/code/脚手架/openai-gemini/src/update.rs#L288-L306) | `clean_downloads` 同时 `remove_dir_all` 和 `remove_file` 同名项 | 冗余但无害，可读性差 |

### 6.2 C++ GUI 侧

| 编号 | 位置 | 问题 | 后果 |
|---|---|---|---|
| C-01 | [gui.cpp L89-L96](file:///e:/code/脚手架/openai-gemini/app/src/gui.cpp#L89-L96)、[L497-L503](file:///e:/code/脚手架/openai-gemini/app/src/gui.cpp#L497-L503) | `gState.localVersion` 只在启动时读一次，`gLatestVersion` 可能比它旧 | BUG-12 |
| C-02 | [gui.cpp L188-L242](file:///e:/code/脚手架/openai-gemini/app/src/gui.cpp#L188-L242) | 手写 JSON 扫描：依赖字段顺序、不处理 `\uD83D\uDE00` 代理对、遇到 `null` 数字返回 0 | 当前 serde 输出中文/emoji 是原始 UTF-8，暂不触发；但协议一旦改成转义输出就乱码 |
| C-03 | [gui.cpp L257-L284](file:///e:/code/脚手架/openai-gemini/app/src/gui.cpp#L257-L284) | 忽略 `protocol` 字段；`sawTerminal` 标记从未使用 | 协议不兼容时静默错解析 |
| C-04 | [gui.cpp L371-L374](file:///e:/code/脚手架/openai-gemini/app/src/gui.cpp#L371-L374) | `context.hwnd = gCheck;` 紧接着被覆盖 | 无害的死代码 |
| C-05 | [gui.cpp L388-L394](file:///e:/code/脚手架/openai-gemini/app/src/gui.cpp#L388-L394) | busy 时只禁用检查/跳过/官网，不禁用安装/重启 | 检查进行中可点安装（BUG-22 叠加） |
| C-06 | [gui.cpp L586-L594](file:///e:/code/脚手架/openai-gemini/app/src/gui.cpp#L586-L594) | `--skip` 在 UI 线程同步等待子进程 | 短暂卡 UI；可双击并发（BUG-06） |
| C-07 | [gui.cpp L697-L716](file:///e:/code/脚手架/openai-gemini/app/src/gui.cpp#L697-L716) | 第二次 `CreateMutexW` 若返回 `NULL`，`GetLastError()` 可能读到旧值 | 极端情况下误判"已在运行" |
| C-08 | [gui.cpp L672-L675](file:///e:/code/脚手架/openai-gemini/app/src/gui.cpp#L672-L675) | 关闭不判断 busy | BUG-23 |
| C-09 | [gui.cpp L26](file:///e:/code/脚手架/openai-gemini/app/src/gui.cpp#L26) | 依赖 `app/build-config.h`；该文件被 `.gitignore`，若跳过 `build-msvc.bat` 直接按头注释的 `cl` 命令编译会用到过期版本（当前是 0.5.5） | 本地产物版本号不真实 |
| C-10 | [gui.cpp L754-L756](file:///e:/code/脚手架/openai-gemini/app/src/gui.cpp#L754-L756) | 注释残缺（"先告诉更新器…更新器才会退出"重复） | 可维护性 |

### 6.3 构建 / CI / 脚本

| 编号 | 位置 | 问题 | 后果 |
|---|---|---|---|
| B-01 | [build.yml L109](file:///e:/code/脚手架/openai-gemini/.github/workflows/build.yml#L109)、[L136-L153](file:///e:/code/脚手架/openai-gemini/.github/workflows/build.yml#L136-L153) | zip 名、feed 名、`platform`、`variant` 全部硬编码 `windows-x64` / `portable` | BUG-08 |
| B-02 | [build.yml L126-L133](file:///e:/code/脚手架/openai-gemini/.github/workflows/build.yml#L126-L133) | CHANGELOG 缺小节时回退成 commit message | 0.5.8 已发生；用户看到的更新日志变成提交信息 |
| B-03 | [build.yml L89-L96](file:///e:/code/脚手架/openai-gemini/.github/workflows/build.yml#L89-L96) | 无 `cargo test`/`cargo clippy` 步骤 | 单测存在但 CI 不跑；回归靠手工 |
| B-04 | [build.yml L5-L13](file:///e:/code/脚手架/openai-gemini/.github/workflows/build.yml#L5-L13) | `workflow_dispatch` 不带 tag 时用 `Cargo.toml` 版本造 tag | 文档已警告，但仍容易撞号 |
| B-05 | [scaffold-update.ps1 L61-L69](file:///e:/code/脚手架/openai-gemini/tools/scaffold-update.ps1#L61-L69) | 复制后删除 `app\build-config.h`，但 `dist/` 里可能带旧 exe | 目标项目首次构建前有一个"版本对不上"的 exe |
| B-06 | [build-msvc.bat L18-L21](file:///e:/code/脚手架/openai-gemini/app/build-msvc.bat#L18-L21) | `/Fo:"%ROOT%\dist\\"` 双反斜杠 | 无害 |
| B-07 | 仓库 | `dist/gui.obj`、`dist/GuiUpdaterDemo.exe`（0.5.5）存在；`.gitignore` 覆盖了 `/dist`，未被跟踪 | 本地 0.5.5 的 exe 容易被误当交付物 |
| B-08 | 测试覆盖 | 15 个单测全部集中在 feed/contract/install/journal/state | `update.rs`（编排、重试、回滚、UAC）与 `download.rs`（哈希/半包/大小不符）**没有任何单测**，恰好是本次 P0 的集中地 |

### 6.4 安全 / 供应链风险

| 编号 | 风险 | 说明 |
|---|---|---|
| RISK-01 | `require_signature=false` 时**没有任何真实性保证** | 包哈希来自同一份可被替换的 feed，等于没有校验；`config.example.json` 与脚手架默认都是 false。建议脚手架默认 `true` + CI 强制 |
| RISK-02 | 无 Authenticode | 文档已承认；这是 SmartScreen/杀软误报的主因，代码层无法替代 |
| RISK-03 | 公钥轮换只有文档流程，没有版本约束 | 旧客户端只认旧 key；若提前移除旧公钥，老用户永久失联。建议 feed 里带 `keyId` 并保留至少两个版本的过渡窗口（当前 `keyId` 只在信封外层，`--verify-feed` 会打印，但客户端不做策略） |
| RISK-04 | 私钥以明文 PEM 存 GitHub Secret，CI 写入 `RUNNER_TEMP` | 常规做法，但注意 `Remove-Item` 不是安全擦除；建议限制 fork PR 的 secret 可见性（默认已限制） |
| RISK-05 | 解压/文本读取无上限 | BUG-19 |

---

## 7. 建议的修复顺序（可直接排期）

| 顺序 | 事项 | 关联 | 理由 |
|---|---|---|---|
| 1 | 事务顺序重构：`recover` 先读 journal 再清旧镜像；回滚失败保留 backup/journal；还原运行中镜像走"改名" | BUG-01/02/05 | 目前唯一会造成"不可恢复"的一组问题 |
| 2 | apply 所有失败分支都尝试拉起旧版 | BUG-03 | 用户感知最差的一类故障 |
| 3 | `emit` 改成不 panic；GUI busy 时拦截关闭 | BUG-23/24 | 一次关闭窗口就能打断安装 |
| 4 | 安装重试改为幂等（备份一次、覆盖可重试）；或按文件重试 | BUG-04 | 避免"回滚成混血" |
| 5 | 加跨进程锁；`--skip` 与 `--apply` 串行 | BUG-06 | 低概率但后果不可控 |
| 6 | `local_version()` 容错；状态帧兜底；`state.json` 原子写 | BUG-13/14/R-04 | 提升"坏了还能自愈"的概率 |
| 7 | 下载：分段超时 + 重试 + Range 续传 | BUG-09 | 大包/弱网可用性 |
| 8 | feed 严格化（variant/platform/product 必填）+ CI/脚手架 platform 一致 | BUG-08/11 | 防止装错包/非 x64 直接不可用 |
| 9 | `--status` + GUI 恢复 pending；`--unskip`；日志文件 | BUG-07/15/可观测性 | 体验与排障 |
| 10 | 文档与 CHANGELOG 对齐；CI 加 `cargo test` | BUG-25/B-03 | 降低后续维护成本 |

---

## 8. 证据与核查说明

- 本报告所有行号基于审查时的文件内容；对应源码可通过行号链接直接定位。
- 未能运行的验证：`cargo test`、`cargo build`、C++ 重新编译、真实更新链路（本机无 Rust 工具链，且需要 Release/网络）。
- 建议的最小验证清单（修复后按序执行）：
  1. `cargo test`（重点：journal/install/feed/contract）。
  2. 本地起 `demo/server.mjs`（8124），用 `--config config.example.json --check` 跑通"检查 → 下载 → 校验 → Ready"。
  3. 手动构造"更新器自更新中途被杀"：在 `install` 的 rename 之后 `Sleep`/断点终止，确认 `--recover` 后 `generic-updater.exe` 仍存在（验证 BUG-01）。
  4. 占用 `GuiUpdaterDemo.exe` 句柄后 `--apply`，确认 90s 重试是否生效（验证 V-01）。
  5. 下载中关闭 GUI，确认 updater 不 abort（验证 BUG-24）。
  6. 用户账户无写权限 + 拒绝 UAC，确认旧版会被重新拉起（验证 BUG-03）。

- 中间记录见同目录 `../work/证据索引.md`。
