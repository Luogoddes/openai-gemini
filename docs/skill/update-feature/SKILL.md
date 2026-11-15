---
name: update-feature
description: 给 Windows 桌面项目接入本仓库的自动更新能力（签名 feed、延迟检查、下载校验、事务替换、重启回滚）。当用户要求"给项目加自动更新""接入 updater""生成更新模块"或迁移更新功能时使用。
---

# 接入自动更新模块

## 什么时候用

用户要给 Windows 桌面程序加自动更新，或把已有项目的更新逻辑迁移到本方案时。

## 需要先问清的输入

- 目标仓库：`owner/repo`（Release 发布在这里）
- 主程序 exe 名：更新后要拉起的那个程序（**不是** `generic-updater.exe`）
- 产品名 `product`：小写短横线，必须与 feed 里的 `product` 一致
- 目标目录：新仓库根目录，或已有项目的 `updater/` 子目录
- 是否需要签名：需要则生成密钥并配 GitHub Secret `UPDATE_SIGNING_KEY`

## 步骤

1. **生成模块**（在源仓库执行）：

   ```powershell
   powershell -ExecutionPolicy Bypass -File scripts/scaffold-update.ps1 `
     -TargetDir <目标目录> -Repo "<owner/repo>" -ExeName "<MyApp.exe>" -Product "<my-app>"
   ```

2. **验证编译**：
   - `cargo build --release`
   - `set MSVC_SETUP=<msvc setup_x64.bat>` 后 `client\build-msvc.bat`

3. **改 `updater.conf`**：repo / exe_name / product / homepage / window_title /
   interval_hours / startup_check_delay_seconds。
   已有项目放子目录时，workflow 的 `working-directory` 与打包路径要加 `updater\` 前缀，
   并把打包内容换成项目真实产物。

4. **写 `CHANGELOG.md`**：当前版本一节，顺序固定「新增 → 修改 → 删除」，一行一条。
   CI 会把它同时写进 Release 说明和客户端「更新日志」。

5. **发布**：`git tag v0.1.0 && git push origin v0.1.0`。
   产出：`latest-version-<platform>.json`、`<product>-<version>-<platform>-portable.zip`、`.zip.sha256`。

6. **签名（可选）**：
   `scripts/new-release-key.ps1`（PS 5.1 可用）生成密钥 → 私钥整段放进 Secret
   `UPDATE_SIGNING_KEY` → 提交公钥 → `require_signature=true` → 打新 tag。
   发布后可用 `generic-updater.exe --verify-feed <json>` 自查。

7. **验证**：旧版本点「检查更新」→ 立即安装 → 重启 → 标题/`version.txt` 更新，
   安装目录只剩程序、`version.txt`、隐藏清单 `.update-files.json`。

## 约束（容易做错）

- `exe_name` 必须是**被更新的程序**；更新器不能覆盖正在运行的自己（已用改名绕过）。
- **版本只由 tag 决定**，不用改 `Cargo.toml` 的 version。
- 发版只走 tag：`git tag vX.Y.Z && git push origin vX.Y.Z`，CI 自动出包。
- 手动 Run workflow 会拿 `Cargo.toml` 的 version 当 tag；不改版本号就会和上一次撞同一个号
  （`v0.5.5`、`v0.5.8` 都是这么来的）。
- 私钥永远不要提交；公钥提交后必须重新构建客户端才生效。
- `require_signature=true` 时 CI 必须有 secret，否则构建失败（有意为之）。
- 配置是编译期的：改了 `updater.conf` 必须重新构建并重新发版。

## 参考

- 功能与设计：`docs/01-更新功能说明.md`
- 接入步骤与排错：`docs/02-接入指南.md`
- 为什么这么设计：`docs/03-设计决策记录.md`
