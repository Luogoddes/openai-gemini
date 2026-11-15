//! 安装事务日志。
//!
//! 安装前先把「要覆盖哪些文件、旧文件备份在哪」写进 journal.json，再开始动文件。
//! 中途掉电/崩溃时，下次启动 `--recover` 能看到未提交的 journal 并按备份还原，
//! 避免出现「一半新一半旧」的文件树。

use crate::error::AppError;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalEntry {
    pub target: String,
    /// 旧文件备份路径；`None` 表示这个文件原本不存在（回滚时要删掉）。
    pub backup: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Journal {
    pub version: String,
    pub entries: Vec<JournalEntry>,
    pub committed: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Recovery {
    /// 没有 journal，无事可做。
    Clean,
    /// 上次安装已完成，只清掉残留。
    Committed,
    /// 上次安装中断，已按备份回滚。
    RolledBack(usize),
}

impl Journal {
    pub fn path(cache_dir: &Path) -> PathBuf {
        cache_dir.join("journal.json")
    }

    pub fn load(path: &Path) -> Option<Journal> {
        let text = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&text).ok()
    }

    pub fn save(&self, path: &Path) -> Result<(), AppError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| AppError::Other(format!("journal 序列化失败: {e}")))?;
        std::fs::write(path, text)?;
        Ok(())
    }

    pub fn commit(path: &Path) -> Result<(), AppError> {
        if let Some(mut journal) = Journal::load(path) {
            journal.committed = true;
            journal.save(path)?;
        }
        Ok(())
    }
}

/// 清理上次安装留下的 `<name>.old-<pid>` 旧镜像。
///
/// 刚更新完时被改名的那份镜像还在运行（就是 updater 自己），删不掉；下次启动时
/// 进程已经退出，这里带一点重试等文件句柄释放。
fn cleanup_old_images(root: &Path) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let is_old_image = path.is_file()
            && path
                .file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.contains(".old-"))
                .unwrap_or(false);
        if !is_old_image {
            continue;
        }
        for attempt in 0..15 {
            if std::fs::remove_file(&path).is_ok() {
                break;
            }
            if attempt == 14 {
                eprintln!("旧镜像暂时删不掉，下次启动再试: {}", path.display());
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
    }
}

/// 启动时调用：有未提交事务就按备份回滚。
pub fn recover(cache_dir: &Path, install_dir: &Path) -> Result<Recovery, AppError> {
    cleanup_old_images(install_dir);

    let path = Journal::path(cache_dir);
    let Some(journal) = Journal::load(&path) else {
        return Ok(Recovery::Clean);
    };

    if journal.committed {
        let _ = std::fs::remove_file(&path);
        return Ok(Recovery::Committed);
    }

    let mut restored = 0usize;
    for entry in &journal.entries {
        let target = PathBuf::from(&entry.target);
        match &entry.backup {
            Some(backup) => {
                let backup_path = PathBuf::from(backup);
                if backup_path.exists() {
                    if let Some(parent) = target.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    std::fs::copy(&backup_path, &target)?;
                    restored += 1;
                }
            }
            None => {
                let _ = std::fs::remove_file(&target);
                restored += 1;
            }
        }
    }

    let _ = std::fs::remove_file(&path);
    Ok(Recovery::RolledBack(restored))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("updater-journal-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn committed_journal_is_dropped_without_touching_files() {
        let dir = temp_dir("committed");
        let cache = dir.join(".app-update");
        std::fs::create_dir_all(&cache).unwrap();
        let path = Journal::path(&cache);
        Journal {
            version: "1.0.0".into(),
            entries: vec![JournalEntry {
                target: dir.join("app.exe").to_string_lossy().to_string(),
                backup: None,
            }],
            committed: true,
        }
        .save(&path)
        .unwrap();

        assert_eq!(recover(&cache, &dir).unwrap(), Recovery::Committed);
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn uncommitted_journal_restores_backups_and_removes_new_files() {
        let dir = temp_dir("rollback");
        let cache = dir.join(".app-update");
        std::fs::create_dir_all(&cache).unwrap();
        let backup = dir.join("backup-app.exe");
        let target = dir.join("app.exe");
        let added = dir.join("new-file.txt");
        std::fs::write(&backup, b"old").unwrap();
        std::fs::write(&target, b"new").unwrap();
        std::fs::write(&added, b"added").unwrap();

        Journal {
            version: "2.0.0".into(),
            entries: vec![
                JournalEntry {
                    target: target.to_string_lossy().to_string(),
                    backup: Some(backup.to_string_lossy().to_string()),
                },
                JournalEntry {
                    target: added.to_string_lossy().to_string(),
                    backup: None,
                },
            ],
            committed: false,
        }
        .save(&Journal::path(&cache))
        .unwrap();

        assert_eq!(recover(&cache, &dir).unwrap(), Recovery::RolledBack(2));
        assert_eq!(std::fs::read(&target).unwrap(), b"old");
        assert!(!added.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
