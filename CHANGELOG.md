# 更新日志

发布说明只写三件事：**新增、修改、删除**，一行一条，顺序固定为新增 → 修改 → 删除。
CI 会按版本号从本文件里抽出对应小节，同时用于 Release 说明和客户端弹窗里的「更新日志」。

## 0.5.2

### 新增
- `--verify-feed <json>`：本地校验发布 feed 的签名，打印 keyId、版本和包数量
- CI 在配置了签名 secret 但公钥文件为空时提前失败，并提示先跑密钥生成脚本

### 修改
- 版本完全跟随 tag：资产名、`version.txt`、feed 里的 version 都用 tag 推导的版本
- 去掉 tag 与 feed 的交叉校验，避免打了 tag 但没改 Cargo.toml 时客户端拒更新
- 密钥生成脚本兼容 Windows PowerShell 5.1（手工编码 PKCS#1 PEM），脚本统一加 UTF-8 BOM，避免 5.1 按 ANSI 解析中文报错
- 签名脚本在 PowerShell 5.1 下给出明确提示（需要 PowerShell 7 / pwsh）

### 删除
- 无

## 0.5.0

### 新增
- 配置编译期内置：repo / 版本文件 / exe 名 / 检查间隔全部写进二进制，发布包只有一个 exe
- 版本号以 Release tag 为准，并与签名元数据里的 version 交叉校验
- 下载缓存改到系统缓存目录 `%LOCALAPPDATA%\GenericUpdater\updates\<hash>`，安装目录不放临时文件
- 升级时按文件清单删除旧版本遗留文件
- 发布说明从本文件自动提取到 Release 与客户端「更新日志」

### 修改
- 启动探测同时接受 `started.flag` 与"进程存活 8 秒"，避免误回滚引起的重复重启
- 自更新改名产生的 `*.old-*` 在下次启动时清理

### 删除
- 移除随包分发的 `app.ini` 与运行期 `updater.json`
- 移除安装目录下的下载缓存与日志残留

## 0.4.0

### 新增
- 元数据 RSA-PSS 验签（3072 位、SHA-256、salt 32）
- 安装事务 journal 与断电恢复
- UAC 提权、updater 自更新、启动探测与超时回滚

### 修改
- 等待旧进程改为 `OpenProcess + WaitForSingleObject`
- 回滚时恢复旧版本号并清理 journal

### 删除
- 移除"安装目录不可写时静默失败"的行为

