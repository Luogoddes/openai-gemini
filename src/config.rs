use std::path::PathBuf;
use std::fs;
use serde::Deserialize;
use crate::error::AppError;

/// 每个被更新软件一份的配置，由主程序在拉起 updater 时传入路径。
#[derive(Debug, Deserialize)]
pub struct Config {
    /// GitHub 仓库，格式 `owner/name`
    pub repo: String,
    /// 本平台要下载的资产文件名，例如 `myapp-windows-x64.zip`
    pub asset_name: String,
    /// 本地版本号文件路径（一行 `v1.2.3` 或 `1.2.3`）
    pub version_file: String,
    /// 安装根目录（建议主程序传绝对路径）
    pub install_dir: String,
    /// 单 exe 资产时，目标 exe 文件名；zip 资产可留空
    #[serde(default)]
    pub exe_name: Option<String>,
}

impl Config {
    pub fn load(path: &str) -> Result<Self, AppError> {
        let text = fs::read_to_string(path)
            .map_err(|e| AppError::Config(format!("读取配置失败 {path}: {e}")))?;
        let cfg: Config = serde_json::from_str(&text)
            .map_err(|e| AppError::Config(format!("配置 JSON 解析失败: {e}")))?;
        Ok(cfg)
    }

    pub fn install_dir(&self) -> PathBuf {
        PathBuf::from(&self.install_dir)
    }

    /// 读取本地已安装版本；文件不存在时按 0.0.0 处理（首次安装场景）。
    pub fn local_version(&self) -> Result<semver::Version, AppError> {
        let raw = fs::read_to_string(&self.version_file).unwrap_or_default();
        let trimmed = raw.trim().trim_start_matches('v').to_string();
        if trimmed.is_empty() {
            return Ok(semver::Version::new(0, 0, 0));
        }
        semver::Version::parse(&trimmed)
            .map_err(|e| AppError::Config(format!("本地版本号无法解析 `{trimmed}`: {e}")))
    }

    pub fn write_local_version(&self, version: &semver::Version) -> Result<(), AppError> {
        fs::write(&self.version_file, format!("{version}\n"))
            .map_err(|e| AppError::Io(e))
    }
}
