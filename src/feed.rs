use crate::config::Config;
use crate::download::http_text;
use crate::error::AppError;
use serde::Deserialize;
use std::path::PathBuf;

/// 元数据里相对路径的解析基准：本地目录或远端目录。
#[derive(Debug, Clone)]
pub enum Base {
    Local(PathBuf),
    Remote(String),
}

impl Base {
    pub fn join(&self, relative: &str) -> String {
        match self {
            Base::Local(directory) => directory.join(relative).to_string_lossy().to_string(),
            Base::Remote(base) => format!(
                "{}/{}",
                base.trim_end_matches('/'),
                relative.trim_start_matches('/')
            ),
        }
    }
}

/// 一份解析完成的更新元数据，只保留当前平台要用的那个包。
#[derive(Debug, Clone)]
pub struct Feed {
    pub version: semver::Version,
    pub notes: String,
    pub package_path: String,
    pub package_url: Option<String>,
    pub size: u64,
    pub sha256: String,
    pub base: Base,
}

impl Feed {
    /// 包的实际来源：显式 url 优先，否则相对元数据基准解析。
    pub fn source(&self) -> String {
        match &self.package_url {
            Some(url) => url.clone(),
            None => self.base.join(&self.package_path),
        }
    }

    pub fn asset_name(&self) -> String {
        self.package_path
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(&self.package_path)
            .to_string()
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FeedJson {
    #[serde(default)]
    schema: u32,
    #[serde(default)]
    product: Option<String>,
    version: String,
    #[serde(default)]
    platform: Option<String>,
    #[serde(default)]
    notes: Option<String>,
    #[serde(default)]
    packages: Vec<PackageJson>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PackageJson {
    #[serde(default)]
    variant: String,
    #[serde(default)]
    kind: String,
    path: String,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    sha256: String,
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

/// 解析元数据 JSON，挑出本平台、本变体、非安装器的包。
pub fn parse(text: &str, cfg: &Config) -> Result<Feed, AppError> {
    let json: FeedJson = serde_json::from_str(text)
        .map_err(|e| AppError::Verify(format!("元数据 JSON 解析失败: {e}")))?;

    if json.schema != 0 && json.schema != 1 {
        return Err(AppError::Verify(format!(
            "不支持的元数据 schema: {}",
            json.schema
        )));
    }
    if let Some(product) = &json.product {
        if product.trim().is_empty() {
            return Err(AppError::Verify("元数据 product 为空".to_string()));
        }
        if let Some(expected) = &cfg.expected_product {
            if !expected.is_empty() && product != expected {
                return Err(AppError::Verify(format!(
                    "元数据 product 不匹配: 期望 {expected}，实际 {product}"
                )));
            }
        }
    }
    if let Some(platform) = &json.platform {
        if !platform.eq_ignore_ascii_case(&cfg.platform) {
            return Err(AppError::Verify(format!(
                "元数据平台不匹配: 期望 {}，实际 {platform}",
                cfg.platform
            )));
        }
    }

    let version = semver::Version::parse(json.version.trim().trim_start_matches('v'))
        .map_err(|e| AppError::Verify(format!("元数据版本号无法解析 `{}`: {e}", json.version)))?;

    let candidate = json
        .packages
        .iter()
        .find(|p| p.variant == cfg.variant && p.kind != "installer")
        .or_else(|| json.packages.iter().find(|p| p.kind != "installer"))
        .ok_or_else(|| {
            AppError::Verify(format!("元数据里没有可用的 `{}` 包", cfg.variant))
        })?;

    if candidate.path.trim().is_empty() {
        return Err(AppError::Verify("元数据包缺少 path".to_string()));
    }
    if !is_sha256(&candidate.sha256) {
        return Err(AppError::Verify(format!(
            "元数据包 sha256 非法: `{}`",
            candidate.sha256
        )));
    }
    if candidate.size == 0 {
        return Err(AppError::Verify("元数据包 size 为 0".to_string()));
    }

    Ok(Feed {
        version,
        notes: json.notes.unwrap_or_default(),
        package_path: candidate.path.clone(),
        package_url: candidate.url.clone(),
        size: candidate.size,
        sha256: candidate.sha256.to_ascii_lowercase(),
        base: Base::Local(PathBuf::from(".")),
    })
}

fn remote_base(url: &str) -> String {
    match url.rfind('/') {
        Some(index) => url[..index].to_string(),
        None => url.to_string(),
    }
}

/// 从 Release tag 解析版本号：`v0.4.0`、`v1.5.7_snow-shot`、`v1.5.7-beta` 都支持。
pub fn version_from_tag(tag: &str) -> Option<semver::Version> {
    let text = tag.trim().trim_start_matches(['v', 'V']);
    let text = text.split('_').next().unwrap_or(text);
    semver::Version::parse(text).ok()
}

/// 取回元数据：`feed_url` 支持本地文件或直链，否则从 Release 资产里找 `feed_name`。
pub fn resolve(
    client: &reqwest::blocking::Client,
    cfg: &Config,
) -> Result<Feed, AppError> {
    if let Some(feed_url) = cfg.feed_url.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        return if feed_url.starts_with("http://") || feed_url.starts_with("https://") {
            let raw = http_text(client, feed_url)?;
            let text = crate::contract::unwrap_feed(&raw, cfg.require_signature)?;
            let mut feed = parse(&text, cfg)?;
            feed.base = Base::Remote(remote_base(feed_url));
            Ok(feed)
        } else {
            let path = PathBuf::from(feed_url);
            let raw = std::fs::read_to_string(&path)
                .map_err(|e| AppError::Config(format!("读取元数据失败 {feed_url}: {e}")))?;
            let text = crate::contract::unwrap_feed(&raw, cfg.require_signature)?;
            let mut feed = parse(&text, cfg)?;
            feed.base = Base::Local(
                path.parent()
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from(".")),
            );
            Ok(feed)
        };
    }

    let api = format!(
        "{}/repos/{}/releases/latest",
        cfg.api_base.trim_end_matches('/'),
        cfg.repo
    );
    let body = http_text(client, &api)?;
    let release: serde_json::Value = serde_json::from_str(&body)
        .map_err(|e| AppError::Network(format!("Release JSON 解析失败: {e}")))?;

    if release
        .get("draft")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
    {
        return Err(AppError::Network("最新 Release 仍是草稿".to_string()));
    }

    let assets = release
        .get("assets")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| AppError::Network("Release 缺少 assets".to_string()))?;

    let url = assets
        .iter()
        .find(|asset| {
            asset.get("name").and_then(serde_json::Value::as_str) == Some(cfg.feed_name.as_str())
        })
        .and_then(|asset| asset.get("browser_download_url"))
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            AppError::Network(format!("Release 中找不到元数据资产 `{}`", cfg.feed_name))
        })?
        .to_string();

    let raw = http_text(client, &url)?;
    let text = crate::contract::unwrap_feed(&raw, cfg.require_signature)?;
    let mut feed = parse(&text, cfg)?;

    // 版本以 tag 为准，同时要求签名 payload 里的 version 与 tag 一致：
    // 防止有人把旧包挂到新 tag 上，或者反过来。
    let tag = release
        .get("tag_name")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    if let Some(tag_version) = version_from_tag(tag) {
        if feed.version != tag_version {
            return Err(AppError::Verify(format!(
                "tag `{tag}` 与元数据版本 `{}` 不一致",
                feed.version
            )));
        }
    }

    feed.base = Base::Remote(remote_base(&url));
    Ok(feed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> Config {
        Config {
            repo: "owner/app".to_string(),
            feed_name: "latest-version-windows-x64.json".to_string(),
            feed_url: None,
            api_base: "https://api.github.com".to_string(),
            version_file: None,
            install_dir: None,
            exe_name: Some("app.exe".to_string()),
            variant: "portable".to_string(),
            platform: "windows-x64".to_string(),
            automatic_check_interval_hours: 24,
            state_file: None,
            require_signature: false,
            expected_product: None,
        }
    }

    const FEED: &str = r#"{
        "schema": 1,
        "product": "app",
        "version": "1.2.0",
        "platform": "windows-x64",
        "notes": "- 修复 A\n- 新增 B",
        "packages": [
            {"variant": "online", "kind": "update", "path": "setup/app-online.zip", "size": 10, "sha256": "aa"},
            {"variant": "portable", "kind": "portable", "path": "setup/app-portable.zip", "size": 20,
             "sha256": "EC820637FE03C327B1C15468BB7CDF6F1565810270D3F3ADBE69A0A79A46D777"}
        ]
    }"#;

    #[test]
    fn picks_matching_variant_and_normalizes_hash() {
        let feed = parse(FEED, &cfg()).unwrap();
        assert_eq!(feed.version.to_string(), "1.2.0");
        assert_eq!(feed.notes, "- 修复 A\n- 新增 B");
        assert_eq!(feed.package_path, "setup/app-portable.zip");
        assert_eq!(feed.asset_name(), "app-portable.zip");
        assert_eq!(
            feed.sha256,
            "ec820637fe03c327b1c15468bb7cdf6f1565810270d3f3adbe69a0a79a46d777"
        );
    }

    #[test]
    fn rejects_bad_checksum_platform_and_missing_package() {
        let bad_hash = FEED.replace(
            "EC820637FE03C327B1C15468BB7CDF6F1565810270D3F3ADBE69A0A79A46D777",
            "xyz",
        );
        assert!(parse(&bad_hash, &cfg()).is_err());

        let bad_platform = FEED.replace("windows-x64", "windows-arm64");
        assert!(parse(&bad_platform, &cfg()).is_err());

        let no_package = r#"{"version": "1.0.0", "packages": [
            {"variant": "portable", "kind": "installer", "path": "a.exe", "size": 1, "sha256": "aa"}
        ]}"#;
        assert!(parse(no_package, &cfg()).is_err());

        let bad_version = FEED.replace("\"1.2.0\"", "\"not-a-version\"");
        assert!(parse(&bad_version, &cfg()).is_err());
    }

    #[test]
    fn trims_v_prefix_and_joins_local_base() {
        let feed = parse(&FEED.replace("\"1.2.0\"", "\"v1.2.0\""), &cfg()).unwrap();
        assert_eq!(feed.version.to_string(), "1.2.0");

        let base = Base::Local(PathBuf::from("demo/feed"));
        assert_eq!(
            PathBuf::from(base.join("portable")),
            PathBuf::from("demo/feed").join("portable")
        );
        let remote = Base::Remote("https://example.test/releases/download/v1".to_string());
        assert_eq!(
            remote.join("app.zip"),
            "https://example.test/releases/download/v1/app.zip"
        );
    }
}


