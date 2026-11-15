use crate::error::AppError;
use serde::Deserialize;
use std::path::PathBuf;

include!(concat!(env!("OUT_DIR"), "/embedded_config.rs"));

fn default_feed_name() -> String {
    FEED_NAME.to_string()
}

fn default_api_base() -> String {
    "https://api.github.com".to_string()
}

fn default_variant() -> String {
    VARIANT.to_string()
}

fn default_platform() -> String {
    PLATFORM.to_string()
}

fn default_interval_hours() -> u64 {
    INTERVAL_HOURS
}

/// 每个被更新软件一份的配置。发布时通过 `updater.conf` 编译进二进制，
/// 本地测试仍然可以用 `--config xxx.json` 覆盖。
#[derive(Debug, Deserialize)]
pub struct Config {
    pub repo: String,
    #[serde(default = "default_feed_name")]
    pub feed_name: String,
    /// 元数据直链；本地文件路径或 http(s) URL。留空则从 Release 里找 `feed_name`。
    #[serde(default)]
    pub feed_url: Option<String>,
    /// Release API 基址，便于私有部署或测试替身
    #[serde(default = "default_api_base")]
    pub api_base: String,
    /// 本地版本号文件路径（一行 `1.2.3`），默认 `<install_dir>/version.txt`
    #[serde(default)]
    pub version_file: Option<String>,
    #[serde(default)]
    pub install_dir: Option<String>,
    #[serde(default)]
    pub exe_name: Option<String>,
    #[serde(default = "default_variant")]
    pub variant: String,
    #[serde(default = "default_platform")]
    pub platform: String,
    #[serde(default = "default_interval_hours")]
    pub automatic_check_interval_hours: u64,
    #[serde(default)]
    pub state_file: Option<String>,
    #[serde(default)]
    pub require_signature: bool,
    /// 期望的 product（编译期内置时自动填），用于拒绝拿到别的产品的包
    #[serde(default)]
    pub expected_product: Option<String>,
}

impl Config {
    pub fn load(path: &str) -> Result<Self, AppError> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| AppError::Config(format!("读取配置失败 {path}: {e}")))?;
        let cfg: Config = serde_json::from_str(&text)
            .map_err(|e| AppError::Config(format!("配置 JSON 解析失败: {e}")))?;
        Ok(cfg)
    }

    /// 用编译期内置的配置构造：安装目录取当前 exe 所在目录。
    pub fn embedded() -> Result<Self, AppError> {
        let executable = std::env::current_exe()
            .map_err(|e| AppError::Config(format!("取自身路径失败: {e}")))?;
        if executable.parent().is_none() {
            return Err(AppError::Config("无法确定安装目录".to_string()));
        }
        Ok(Config {
            repo: REPO.to_string(),
            feed_name: FEED_NAME.to_string(),
            feed_url: None,
            api_base: default_api_base(),
            version_file: None,
            install_dir: None,
            exe_name: Some(EXE_NAME.to_string()),
            variant: VARIANT.to_string(),
            platform: PLATFORM.to_string(),
            automatic_check_interval_hours: INTERVAL_HOURS,
            state_file: None,
            require_signature: REQUIRE_SIGNATURE,
            expected_product: Some(PRODUCT.to_string()),
        })
    }

    pub fn install_dir(&self) -> PathBuf {
        match &self.install_dir {
            Some(dir) => PathBuf::from(dir),
            None => std::env::current_exe()
                .ok()
                .and_then(|exe| exe.parent().map(PathBuf::from))
                .unwrap_or_else(|| PathBuf::from(".")),
        }
    }

    pub fn version_file_path(&self) -> PathBuf {
        match &self.version_file {
            Some(path) => PathBuf::from(path),
            None => self.install_dir().join("version.txt"),
        }
    }

    /// 下载缓存、暂存、备份、state 都放在系统缓存目录，安装目录里不留垃圾。
    /// 如果系统缓存目录不可写（受限账户、受控文件夹、沙箱），退回安装目录下的隐藏目录。
    pub fn cache_dir(&self) -> PathBuf {
        if let Some(dir) = std::env::var_os("UPDATER_CACHE_DIR") {
            if !dir.is_empty() {
                return PathBuf::from(dir);
            }
        }

        static RESOLVED: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
        RESOLVED
            .get_or_init(|| {
                if let Some(base) = system_cache_root() {
                    let candidate = base
                        .join("GenericUpdater")
                        .join("updates")
                        .join(short_hash(&self.install_dir().to_string_lossy()));
                    if writable(&candidate) {
                        return candidate;
                    }
                    eprintln!(
                        "系统缓存目录不可写，改用安装目录下的 .app-update: {}",
                        candidate.display()
                    );
                }
                self.install_dir().join(".app-update")
            })
            .clone()
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

    /// 启动探测标记：固定放在安装目录，客户端和更新器都认这个位置。
    pub fn started_marker(&self) -> PathBuf {
        self.install_dir().join(".app-update").join("started.flag")
    }

    /// 读取本地已安装版本；文件不存在时按 0.0.0 处理（首次安装场景）。
    pub fn local_version(&self) -> Result<semver::Version, AppError> {
        let raw = std::fs::read_to_string(self.version_file_path()).unwrap_or_default();
        let trimmed = raw.trim().trim_start_matches('v').to_string();
        if trimmed.is_empty() {
            return Ok(semver::Version::new(0, 0, 0));
        }
        semver::Version::parse(&trimmed)
            .map_err(|e| AppError::Config(format!("本地版本号无法解析 `{trimmed}`: {e}")))
    }

    pub fn write_local_version(&self, version: &semver::Version) -> Result<(), AppError> {
        std::fs::write(self.version_file_path(), format!("{version}\n"))
            .map_err(|e| AppError::Io(e))
    }
}

fn system_cache_root() -> Option<PathBuf> {
    if let Some(base) = std::env::var_os("LOCALAPPDATA") {
        return Some(PathBuf::from(base));
    }
    if let Some(base) = std::env::var_os("XDG_CACHE_HOME") {
        return Some(PathBuf::from(base));
    }
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache"))
}

fn short_hash(text: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(text.as_bytes());
    hex::encode(digest)[..12].to_string()
}

/// 真的写一个探针文件判断目录可写，比看 ACL 靠谱。
fn writable(dir: &std::path::Path) -> bool {
    if std::fs::create_dir_all(dir).is_err() {
        return false;
    }
    let probe = dir.join(format!(".write-probe-{}", std::process::id()));
    match std::fs::write(&probe, b"probe") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

