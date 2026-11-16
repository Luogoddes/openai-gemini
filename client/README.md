# 旧 C++ 示例客户端（已归档）

这个目录是脚手架最早期的 Win32 示例客户端：一个只做「检查更新 → 下载 → 重启替换」
的小窗口，用 MSVC / MinGW 直接编译。

现在仓库里的示例客户端是 Rust 版：

- 源码：`demo-app/`（workspace 成员，产物 `GuiUpdaterDemo.exe`）
- 构建：`cargo build --release --workspace`
- CI：`.github/workflows/build.yml` 只构建 Rust 版本，不再调用 `build-msvc.bat`
- 资源：VERSIONINFO / 图标 / 清单在 `demo-app/resources/`

## 为什么保留

- 更新器契约（`--check` / `--download` / `--apply --parent-pid` + stdout JSON 帧）
  最早就是对着它定下来的，改 `src/protocol.rs` 时可以对照着读。
- 它只有一屏 UI，比现在的文件夹速取工具更容易看懂「主程序 ↔ 更新器」的分工。

## 不再维护

`build-msvc.bat`、`scripts/gen-app-config.ps1` 生成的 `build-config.h` 都保留原样，
但 CI 不再构建，脚本也可能随编译环境变化而失效。实际功能开发请改 `demo-app/`。

`build-config.h` 是构建期生成物（已被 `.gitignore` 忽略）。
