# 更新模块文档

| 文档 | 内容 | 适合谁 |
|---|---|---|
| [01-更新功能说明.md](01-更新功能说明.md) | 架构、协议、检查机制、事务安装、签名、缓存、边界 | 要理解/维护实现的人 |
| [02-接入指南.md](02-接入指南.md) | 一键脚手架、接入已有项目、发布、签名、排错、清单 | 要把更新接进项目的人 |
| [03-设计决策记录.md](03-设计决策记录.md) | 版本演进、关键取舍、踩过的坑、验证记录 | 回头看"为什么这样"的人 |
| [skill/update-feature/SKILL.md](skill/update-feature/SKILL.md) | 给 Agent 用的操作说明（可放进 DSH skills 目录） | 想让 Agent 一键接入的人 |

## 最短路径

```powershell
# 在源仓库里执行：把更新模块生成到目标项目
powershell -ExecutionPolicy Bypass -File scripts\scaffold-update.ps1 `
  -TargetDir D:\code\my-app -Repo "me/my-app" -ExeName "MyApp.exe" -Product "my-app"

# 目标项目里验证
cd D:\code\my-app
cargo build --release
client\build-msvc.bat

# 发版
git tag v0.1.0; git push origin v0.1.0
```

生成之后只需要维护两件事：`updater.conf`（一次性）和 `CHANGELOG.md`（每次发版）。
