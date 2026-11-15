use serde::Deserialize;
use crate::error::AppError;

#[derive(Debug, Clone)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub size: u64,
    pub sha256: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ReleaseInfo {
    pub version: semver::Version,
    pub notes: String,
    pub asset: Asset,
}

#[derive(Deserialize)]
struct ApiRelease {
    tag_name: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    assets: Vec<ApiAsset>,
}

#[derive(Deserialize)]
struct ApiAsset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    digest: Option<String>,
}

/// 请求 GitHub Release latest，匹配指定资产名。
pub fn fetch_latest(
    client: &reqwest::blocking::Client,
    repo: &str,
    asset_name: &str,
) -> Result<ReleaseInfo, AppError> {
    let url = format!("https://api.github.com/repos/{repo}/releases/latest");
    let resp = client
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "generic-updater/0.1")
        .send()
        .map_err(|e| AppError::Network(format!("请求 Release 失败: {e}")))?;

    if !resp.status().is_success() {
        return Err(AppError::Network(format!(
            "Release 接口返回状态 {}",
            resp.status()
        )));
    }

    let release: ApiRelease = resp
        .json()
        .map_err(|e| AppError::Network(format!("Release JSON 解析失败: {e}")))?;

    let version = release
        .tag_name
        .trim_start_matches('v')
        .to_string();
    let version = semver::Version::parse(&version)
        .map_err(|e| AppError::Verify(format!("tag 版本号无法解析 `{}`: {e}", release.tag_name)))?;

    let asset = release
        .assets
        .into_iter()
        .find(|a| a.name == asset_name)
        .ok_or_else(|| {
            AppError::Verify(format!("Release 中找不到资产 `{asset_name}`"))
        })?;

    let sha256 = asset.digest.and_then(|d| {
        d.strip_prefix("sha256:").map(|s| s.to_string())
    });

    Ok(ReleaseInfo {
        version,
        notes: release.body.unwrap_or_default(),
        asset: Asset {
            name: asset.name,
            url: asset.browser_download_url,
            size: asset.size,
            sha256,
        },
    })
}
