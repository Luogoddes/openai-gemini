use crate::error::AppError;
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::Path;
use std::time::Duration;

/// 单个文件的下载尝试次数（含首次）。失败时保留 `.part`，下次从断点续传。
const DOWNLOAD_ATTEMPTS: usize = 3;

pub fn sha256_file(path: &Path) -> Result<String, AppError> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// 取一段文本（元数据、Release 接口）。
///
/// 客户端整体超时是给下载用的，文本请求单独收紧到 30 秒，避免接口卡住时长时间无响应。
pub fn http_text(client: &reqwest::blocking::Client, url: &str) -> Result<String, AppError> {
    let resp = client
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "generic-updater/0.5")
        .timeout(Duration::from_secs(30))
        .send()
        .map_err(|e| AppError::Network(format!("请求 {url} 失败: {e}")))?;

    if !resp.status().is_success() {
        return Err(AppError::Network(format!(
            "请求 {url} 返回状态 {}",
            resp.status()
        )));
    }
    resp.text()
        .map_err(|e| AppError::Network(format!("读取 {url} 响应失败: {e}")))
}

fn is_remote(source: &str) -> bool {
    source.starts_with("http://") || source.starts_with("https://")
}

/// 下载到 `dest` 的 `.part` 临时文件，完成并校验通过后再原子改名就位。
/// 来源既可以是 http(s) 直链，也可以是本地文件（测试用）。
/// `progress(received, total)` 用于上报进度条。
///
/// 支持断点续传：`.part` 已存在时发 `Range` 请求；服务端不支持（返回 200）就截断重下。
/// 完成后按整文件 SHA256 校验，所以续传拼出来的文件同样可信。
pub fn download_to(
    client: &reqwest::blocking::Client,
    source: &str,
    dest: &Path,
    expected_sha256: Option<&str>,
    expected_size: u64,
    mut progress: impl FnMut(u64, u64),
) -> Result<(), AppError> {
    let part = dest.with_extension("part");
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }

    if !is_remote(source) {
        std::fs::copy(source, &part)
            .map_err(|e| AppError::Network(format!("打开本地资产失败 {source}: {e}")))?;
        return finalize(&part, dest, expected_sha256, expected_size);
    }

    let mut last_error: Option<AppError> = None;
    for attempt in 0..DOWNLOAD_ATTEMPTS {
        if attempt > 0 {
            // 1s、2s 退避；已下载的部分保留，下一轮继续续传。
            std::thread::sleep(Duration::from_secs(attempt as u64));
        }
        let result = download_once(client, source, &part, expected_size, &mut progress)
            .and_then(|()| finalize(&part, dest, expected_sha256, expected_size));
        match result {
            Ok(()) => return Ok(()),
            Err(error) => {
                eprintln!("下载失败（第 {} 次）: {error}", attempt + 1);
                last_error = Some(error);
            }
        }
    }

    Err(last_error.unwrap_or_else(|| AppError::Network("下载失败".to_string())))
}

fn download_once(
    client: &reqwest::blocking::Client,
    source: &str,
    part: &Path,
    expected_size: u64,
    progress: &mut impl FnMut(u64, u64),
) -> Result<(), AppError> {
    let mut offset = part.metadata().map(|meta| meta.len()).unwrap_or(0);
    if expected_size > 0 && offset > expected_size {
        // .part 比目标还大，说明是坏文件或换了个包，重下。
        let _ = std::fs::remove_file(part);
        offset = 0;
    }

    let mut request = client
        .get(source)
        .header("User-Agent", "generic-updater/0.5");
    if offset > 0 {
        request = request.header("Range", format!("bytes={offset}-"));
    }
    let mut resp = request
        .send()
        .map_err(|e| AppError::Network(format!("下载请求失败: {e}")))?;
    let status = resp.status();

    let (mut file, mut received) = if offset > 0 {
        match status.as_u16() {
            206 => {
                // 必须确认服务端返回的正是我们要的起点，否则续传会拼坏文件。
                let expected_range = format!("bytes {offset}-");
                let matches = resp
                    .headers()
                    .get(reqwest::header::CONTENT_RANGE)
                    .and_then(|value| value.to_str().ok())
                    .map(|value| value.starts_with(&expected_range))
                    .unwrap_or(false);
                if !matches {
                    let _ = std::fs::remove_file(part);
                    return Err(AppError::Network(
                        "续传响应范围不匹配，已重置下载".to_string(),
                    ));
                }
                (
                    std::fs::OpenOptions::new().append(true).open(part)?,
                    offset,
                )
            }
            416 => {
                // 请求的起点已经越界：本地 .part 不可信，删掉重下。
                let _ = std::fs::remove_file(part);
                return Err(AppError::Network(
                    "服务器拒绝续传范围，已重置下载".to_string(),
                ));
            }
            _ => {
                if !status.is_success() {
                    return Err(AppError::Network(format!("下载状态码 {status}")));
                }
                // 服务端不支持 Range，返回了完整文件：从头写。
                (std::fs::File::create(part)?, 0)
            }
        }
    } else {
        if !status.is_success() {
            return Err(AppError::Network(format!("下载状态码 {status}")));
        }
        (std::fs::File::create(part)?, 0)
    };

    let total = expected_size;
    let mut buf = [0u8; 64 * 1024];
    progress(received, total);
    loop {
        let n = resp
            .read(&mut buf)
            .map_err(|e| AppError::Network(format!("下载读取中断: {e}")))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])?;
        received += n as u64;
        progress(received, total);
    }
    file.flush()?;
    Ok(())
}

/// 大小 + 整文件 SHA256 校验，通过后把 `.part` 改名就位。
fn finalize(
    part: &Path,
    dest: &Path,
    expected_sha256: Option<&str>,
    expected_size: u64,
) -> Result<(), AppError> {
    let length = std::fs::metadata(part).map(|meta| meta.len()).unwrap_or(0);
    if expected_size > 0 && length != expected_size {
        let _ = std::fs::remove_file(part);
        return Err(AppError::Verify(format!(
            "大小不符: 期望 {expected_size} 字节，实际 {length} 字节"
        )));
    }

    if let Some(expected) = expected_sha256 {
        let actual = sha256_file(part)?;
        if !actual.eq_ignore_ascii_case(expected) {
            let _ = std::fs::remove_file(part);
            return Err(AppError::Verify("SHA256 不一致，下载文件已丢弃".to_string()));
        }
    }

    // 原子就位
    if dest.exists() {
        let _ = std::fs::remove_file(dest);
    }
    std::fs::rename(part, dest)?;
    Ok(())
}
