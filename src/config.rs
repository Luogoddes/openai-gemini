use crate::error::AppError;
use serde::Deserialize;
use std::path::PathBuf;

fn default_feed_name() -> String {
    "latest-version-windows-x64.json".to_string()
}

fn default_api_base() -> String {
    "https://api.github.com".to_string()
}

fn default_variant() -> String {
    "portable".to_string()
}

fn default_platform() -> String {
    "windows-x64".to_string()
}

fn default_interval_hours() -> u64 {
    24
}

/// 每个被更新软件一份的配置，由主程序在拉起 updater 时传入路径。
#[derive(Debug, Deserialize)]
pub struct Config {
    /// GitHub 仓库，格式 `owner/name`
    pub repo: String,
    /// 本平台的 Release 元数据资产名，例如 `latest-version-windows-x64.json`
    #[serde(default = "default_feed_name")]
    pub feed_name: String,
    /// 元数据直链；本地文件路径或 http(s) URL。留空则从 Release 里找 `feed_name`。
    #[serde(default)]
    pub feed_url: Option<String>,
    /// Release API 基址，便于私有部署或测试替身
    #[serde(default = "default_api_base")]
    pub api_base: String,
    /// 本地版本号文件路径（一行 `v1.2.3` 或 `1.2.3`）
    pub version_file: String,
    /// 安装根目录（建议主程序传绝对路径）
    pub install_dir: String,
    /// 单 exe 资产时，目标 exe 文件名；zip 资产可留空
    #[serde(default)]
    pub exe_name: Option<String>,
    /// 要使用的包类型：`portable` / `online` / `offline`
    #[serde(default = "default_variant")]
    pub variant: String,
    /// 平台标识，写进元数据做一致性校验
    #[serde(default = "default_platform")]
    pub platform: String,
    /// 自动检查的最小间隔（小时），手动检查不受限
    #[serde(default = "default_interval_hours")]
    pub automatic_check_interval_hours: u64,
    /// 自动检查状态文件；留空则放在安装目录的 `.app-update/state.json`
    #[serde(default)]
    pub state_file: Option<String>,
}

impl Config {
    pub fn load(path: &str) -> Result<Self, AppError> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| AppError::Config(format!("读取配置失败 {path}: {e}")))?;
        let cfg: Config = serde_json::from_str(&text)
            .map_err(|e| AppError::Config(format!("配置 JSON 解析失败: {e}")))?;
        Ok(cfg)
    }

    pub fn install_dir(&self) -> PathBuf {
        PathBuf::from(&self.install_dir)
    }

    /// 下载缓存 / 解压暂存 / 状态文件都放在这里。
    pub fn cache_dir(&self) -> PathBuf {
        self.install_dir().join(".app-update")
    }

    pub fn state_path(&self) -> PathBuf {
        match &self.state_file {
            Some(path) => PathBuf::from(path),
            None => self.cache_dir().join("state.json"),
        }
    }

    pub fn staging_dir(&self) -> PathBuf {
        self.cache_dir().join("staging")
    }

    pub fn backup_dir(&self) -> PathBuf {
        self.cache_dir().join("backup")
    }

    /// 读取本地已安装版本；文件不存在时按 0.0.0 处理（首次安装场景）。
    pub fn local_version(&self) -> Result<semver::Version, AppError> {
        let raw = std::fs::read_to_string(&self.version_file).unwrap_or_default();
        let trimmed = raw.trim().trim_start_matches('v').to_string();
        if trimmed.is_empty() {
            return Ok(semver::Version::new(0, 0, 0));
        }
        semver::Version::parse(&trimmed)
            .map_err(|e| AppError::Config(format!("本地版本号无法解析 `{trimmed}`: {e}")))
    }

    pub fn write_local_version(&self, version: &semver::Version) -> Result<(), AppError> {
        std::fs::write(&self.version_file, format!("{version}\n"))
            .map_err(|e| AppError::Io(e))
    }
}
