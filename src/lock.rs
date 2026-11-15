//! 跨进程互斥：缓存目录下的 `updater.lock`。
//!
//! 主程序、GUI、计划任务、用户手点都可能同时拉起更新器；安装事务只能有一个进程在跑，
//! 否则两份 journal 会互相覆盖、备份互相踩踏。GUI 里的互斥体是 `Local\` 命名空间，
//! 只在同一登录会话内有效，挡不住"提权后的另一个进程"，所以这里用文件锁兜底。
//!
//! 锁文件内容：`<pid> <unix_ts>`。`create_new` 保证原子创建；陈旧锁（pid 已退出，
//! 或文件写了 5 秒还没有 pid）会被清理后重试。

use crate::error::AppError;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const LOCK_NAME: &str = "updater.lock";
const WAIT_LIMIT: Duration = Duration::from_secs(10);
const POLL_INTERVAL: Duration = Duration::from_millis(200);

pub struct Lock {
    path: PathBuf,
    pid: u32,
}

impl Lock {
    /// 取锁；最多等 10 秒，超时视为另一个更新进程正在运行。
    pub fn acquire(cache_dir: &Path) -> Result<Lock, AppError> {
        std::fs::create_dir_all(cache_dir)?;
        let path = cache_dir.join(LOCK_NAME);
        let pid = std::process::id();
        let started = Instant::now();

        loop {
            match try_create(&path, pid) {
                Ok(()) => return Ok(Lock { path, pid }),
                Err(CreateError::Busy) => {
                    if is_stale(&path) {
                        let _ = std::fs::remove_file(&path);
                        continue;
                    }
                    if started.elapsed() >= WAIT_LIMIT {
                        return Err(AppError::Other(
                            "另一个更新进程正在运行，请稍后重试".to_string(),
                        ));
                    }
                    std::thread::sleep(POLL_INTERVAL);
                }
                Err(CreateError::Io(error)) => return Err(AppError::Io(error)),
            }
        }
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        // 只删自己创建的那把锁，避免删掉别人刚接管的锁。
        let own_pid = self.pid.to_string();
        if let Ok(text) = std::fs::read_to_string(&self.path) {
            if text.split_whitespace().next() == Some(own_pid.as_str()) {
                let _ = std::fs::remove_file(&self.path);
            }
        }
    }
}

enum CreateError {
    Busy,
    Io(std::io::Error),
}

fn try_create(path: &Path, pid: u32) -> Result<(), CreateError> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                CreateError::Busy
            } else {
                CreateError::Io(error)
            }
        })?;
    let _ = writeln!(file, "{} {}", pid, crate::state::now_unix());
    Ok(())
}

fn is_stale(path: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(path) else {
        // 读不到说明锁刚被删掉，下一轮重试即可。
        return true;
    };
    if let Some(pid) = text.split_whitespace().next().and_then(|v| v.parse::<u32>().ok()) {
        if pid == std::process::id() {
            // 自己进程持有：正常不应走到这里（同一进程只取一次锁），按不陈旧处理。
            return false;
        }
        return !crate::platform::process_alive(pid);
    }
    // 文件刚创建、pid 还没写进去：给 5 秒宽限，别抢正在写入的锁。
    std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|modified| modified.elapsed().ok())
        .map(|age| age > Duration::from_secs(5))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("updater-lock-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn lock_is_exclusive_and_released_on_drop() {
        let dir = temp_dir("exclusive");
        let first = Lock::acquire(&dir).unwrap();
        assert!(dir.join(LOCK_NAME).exists());

        // 同一进程 pid 相同，is_stale 判为不陈旧，只能等超时；这里直接验证文件内容。
        let text = std::fs::read_to_string(dir.join(LOCK_NAME)).unwrap();
        assert!(text.starts_with(&std::process::id().to_string()));

        drop(first);
        assert!(!dir.join(LOCK_NAME).exists());
        // 释放后可以再次获取
        let _second = Lock::acquire(&dir).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stale_lock_with_dead_pid_is_taken_over() {
        let dir = temp_dir("stale");
        let path = dir.join(LOCK_NAME);
        std::fs::write(&path, "999999 1\n").unwrap();
        let lock = Lock::acquire(&dir).unwrap();
        assert!(lock.path.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
