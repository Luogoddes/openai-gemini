use crate::error::AppError;
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::Path;

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

/// 下载到 `dest` 的 `.part` 临时文件，完成并校验通过后再原子改名就位。
/// `progress(received, total)` 用于上报进度条。
pub fn download_to(
    client: &reqwest::blocking::Client,
    url: &str,
    dest: &Path,
    expected_sha256: Option<&str>,
    expected_size: u64,
    mut progress: impl FnMut(u64, u64),
) -> Result<(), AppError> {
    let part = dest.with_extension("part");
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let resp = client
        .get(url)
        .header("User-Agent", "generic-updater/0.1")
        .send()
        .map_err(|e| AppError::Network(format!("下载请求失败: {e}")))?;

    if !resp.status().is_success() {
        return Err(AppError::Network(format!("下载状态码 {}", resp.status())));
    }

    let total = if expected_size > 0 { expected_size } else { resp.content_length().unwrap_or(0) };
    let mut file = std::fs::File::create(&part)?;
    let mut hasher = Sha256::new();
    let mut received: u64 = 0;
    let mut reader = resp;
    let mut buf = [0u8; 64 * 1024];

    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| AppError::Network(format!("下载读取中断: {e}")))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])?;
        hasher.update(&buf[..n]);
        received += n as u64;
        progress(received, total);
    }
    file.flush()?;

    // 大小校验
    if expected_size > 0 && received != expected_size {
        let _ = std::fs::remove_file(&part);
        return Err(AppError::Verify(format!(
            "大小不符: 期望 {expected_size} 字节，实际 {received} 字节"
        )));
    }

    // 哈希校验
    if let Some(expected) = expected_sha256 {
        let actual = hex::encode(hasher.finalize());
        if !actual.eq_ignore_ascii_case(expected) {
            let _ = std::fs::remove_file(&part);
            return Err(AppError::Verify("SHA256 不一致，下载文件已丢弃".to_string()));
        }
    }

    // 原子就位
    std::fs::rename(&part, dest)?;
    Ok(())
}
