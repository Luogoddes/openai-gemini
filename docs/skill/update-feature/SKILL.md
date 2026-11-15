---
name: update-feature
description: 给 Windows 桌面项目接入本仓库的自动更新能力（签名 feed、24h 检查、下载校验、备份替换、重启回滚）。当用户要求"给项目加自动更新""接入 updater""生成更新模块"或迁移更新功能时使用。
---

# 接入自动更新模块

## 什么时候用

用户要给一个 Windows 桌面程序加自动更新，或把已有项目的更新逻辑迁移到本方案时。

## 输入（缺什么问什么）

- 目标仓库：`owner/repo`（Release 发布在这里）
- 主程序 exe 名：更新后要拉起的那个程序（**不是** generic-updater.exe）
- 产品名（product）：和 feed 里的必填字段一致，小写短横线
- 目标目录：把更新模块放到哪里（新仓库根目录，或已有项目的 `updater/` 子目录）
- 是否要签名：要签名则需生成密钥并配 GitHub Secret

## 步骤

1. **生成模块**（源仓库里执行）：

   ```powershell
   powershell -ExecutionPolicy Bypass -File tools/scaffold-update.ps1 `
     -TargetDir <目标目录> -Repo "<owner/repo>" -ExeName "<MyApp.exe>" -Product "<my-app>"
   ```

2. **验证编译**：
   - `cargo build --release`（生成 `generic-updater.exe`）
   - `set MSVC_SETUP=<msvc setup_x64.bat>` 后 `app\build-msvc.bat`（生成 GUI）

3. **改 `updater.conf`**：repo / exe_name / product / homepage / window_title / interval_hours。
   已有项目放在子目录时，把 workflow 的 `working-directory` 和打包路径改成子目录前缀，
   并把打包内容换成项目真实产物。

4. **写 `CHANGELOG.md`**：当前版本一节，固定顺序「新增 → 修改 → 删除」，一行一条。

5. **发布**：`git tag v0.1.0 && git push origin v0.1.0`，CI 产出
   `latest-version-<platform>.json` + `<product>-<version>-<platform>-portable.zip` + `.sha256`。

6. **签名（可选）**：
   `tools/new-release-key.ps1` 生成密钥 → 私钥进 Secret `UPDATE_SIGNING_KEY` →
   提交公钥 → `require_signature=true` → 打新 tag。

7. **验证**：旧版本点「检查更新」→ 立即安装 → 重启 → 版本号与标题都更新；
   发布后可跑 `generic-updater.exe --verify-feed <json>` 确认签名。

## 约束（不要做错）

- `exe_name` 必须是**被更新的程序**；`generic-updater.exe` 自己不能覆盖正在运行的自己。
- 版本由 tag 决定，不用改 `Cargo.toml` 的 version。
- 私钥永远不要提交；公钥提交后必须重新编译客户端才生效。
- `require_signature=true` 时 CI 必须有 secret，否则构建会失败（这是有意的）。
- 配置文件是编译期的：改了 `updater.conf` 必须重新构建并重新发版。

## 参考

- 功能与设计：仓库 `docs/01-更新功能说明.md`
- 详细接入步骤与排错：仓库 `docs/02-接入指南.md`
