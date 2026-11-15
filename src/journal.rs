//! 安装事务日志（schema v2）。
//!
//! 安装前先把「要覆盖/删除哪些文件、旧文件备份在哪」写进 journal.json，再开始动文件。
//! 中途掉电/崩溃时，下次启动 `--recover` 能看到未提交的 journal 并按备份还原，
//! 避免出现「一半新一半旧」的文件树。
//!
//! v2 相对 v1 的变化：
//! * 每个条目带 `action`（write/delete），旧版本有、新版本没有的文件也会被移到备份区，
//!   因此回滚能把它还原回来，而不是直接删掉；
//! * 回滚完全由 journal 驱动，不再遍历备份目录猜测；
//! * 回滚失败时保留 journal 与备份，下次启动继续尝试，绝不先清理证据。

use crate::error::AppError;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// 当前事务格式版本。v1 没有 `schema` 字段，按 0 处理。
pub const SCHEMA: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryAction {
    /// 写入/覆盖 target。
    Write,
    /// 新版不再包含 target：文件被移到备份区，回滚时还原。
    Delete,
}

fn default_action() -> EntryAction {
    EntryAction::Write
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalEntry {
    pub target: String,
    /// 旧文件备份路径。
    /// * write + 原本存在：普通备份文件；
    /// * write + 原本不存在：`None`，回滚时删掉新文件；
    /// * delete：被移走的旧文件（备份区 `__deleted__/<rel>` 或 `*.old-<pid>`）。
    pub backup: Option<String>,
    #[serde(default = "default_action")]
    pub action: EntryAction,
    /// 目标是运行中的更新器自身：用「改名 → 写新文件」而不是覆盖。
    #[serde(default)]
    pub renamed_from_running: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Journal {
    #[serde(default)]
    pub schema: u32,
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
    pub fn new(version: &str, entries: Vec<JournalEntry>) -> Journal {
        Journal {
            schema: SCHEMA,
            version: version.to_string(),
            entries,
            committed: false,
        }
    }

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
        // 先写临时文件再改名，避免 journal 本身写一半断电。
        let temporary = path.with_extension("json.tmp");
        std::fs::write(&temporary, text)?;
        std::fs::rename(&temporary, path)?;
        Ok(())
    }
}

/// `<name>.old-<pid>`：运行中镜像的旁置改名。
pub(crate) fn old_image_path(target: &Path) -> PathBuf {
    let name = target
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("file");
    target.with_file_name(format!("{name}.old-{}", std::process::id()))
}

/// `target` 是否是当前正在运行的镜像（更新器自己）。
pub(crate) fn is_running_image(target: &Path) -> bool {
    let Some(running) = std::env::current_exe()
        .ok()
        .and_then(|path| path.canonicalize().ok())
    else {
        return false;
    };
    match target.canonicalize() {
        Ok(canonical) => canonical == running,
        Err(_) => false,
    }
}

/// 按 journal 把文件树还原。
///
/// 任何一步失败都直接返回错误，**不删除 journal、不删除备份**：
/// 下次启动还能继续恢复，也不会丢掉唯一的旧版本副本。
pub fn rollback(journal: &Journal) -> Result<usize, AppError> {
    let mut restored = 0usize;
    for entry in &journal.entries {
        let target = PathBuf::from(&entry.target);

        let Some(backup) = &entry.backup else {
            // write + 原本不存在：回滚就是删掉这次新增的文件。
            if entry.action == EntryAction::Write && target.is_file() {
                std::fs::remove_file(&target).map_err(|error| {
                    AppError::Io(std::io::Error::new(
                        error.kind(),
                        format!("回滚时删除 {} 失败: {error}", target.display()),
                    ))
                })?;
                restored += 1;
            }
            continue;
        };

        let backup_path = PathBuf::from(backup);
        if !backup_path.exists() {
            // 备份没落地（安装还没走到这一步），当前文件就是旧文件，无需处理。
            continue;
        }

        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // 正在运行的更新器不能被覆盖：先改名，再把备份复制回去。
        // 改名残留由下次启动的 cleanup_old_images 清掉。
        if target.exists() && is_running_image(&target) {
            let aside = old_image_path(&target);
            let _ = std::fs::remove_file(&aside);
            std::fs::rename(&target, &aside).map_err(|error| {
                AppError::Install(format!(
                    "回滚时无法移开运行中的镜像 {}: {error}",
                    target.display()
                ))
            })?;
        }

        std::fs::copy(&backup_path, &target).map_err(|error| {
            AppError::Io(std::io::Error::new(
                error.kind(),
                format!("回滚 {} 失败: {error}", target.display()),
            ))
        })?;
        restored += 1;
    }
    Ok(restored)
}

/// 清理上次安装留下的 `<name>.old-<pid>` 旧镜像。
///
/// 刚更新完时被改名的那份镜像还在运行（就是 updater 自己），删不掉；下次启动时
/// 进程已经退出，这里带一点重试等文件句柄释放。当前进程自己的 `.old-<pid>` 跳过，
/// 它是正在执行的镜像，删也删不掉，只会白白重试。
fn cleanup_old_images(root: &Path) {
    let own_suffix = format!(".old-{}", std::process::id());
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let is_old_image = path.is_file() && name.contains(".old-");
        if !is_old_image || name.ends_with(&own_suffix) {
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

/// 启动时调用：有未提交事务就先回滚，回滚成功后才清理旧镜像。
pub fn recover(cache_dir: &Path, install_dir: &Path) -> Result<Recovery, AppError> {
    let path = Journal::path(cache_dir);
    let journal = match Journal::load(&path) {
        Some(journal) => journal,
        None => {
            if path.exists() {
                // journal 内容坏了：宁可停下，也不要在看不清事务的情况下动文件。
                return Err(AppError::Install(
                    "journal.json 无法解析，已保留备份等待人工处理".to_string(),
                ));
            }
            cleanup_old_images(install_dir);
            return Ok(Recovery::Clean);
        }
    };

    if journal.committed {
        let _ = std::fs::remove_file(&path);
        cleanup_old_images(install_dir);
        return Ok(Recovery::Committed);
    }

    // 顺序很关键：先回滚，再清理 `*.old-*`。反过来会把回滚要用的备份删掉。
    let restored = rollback(&journal)?;
    let _ = std::fs::remove_file(&path);
    cleanup_old_images(install_dir);
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

    fn write_journal(cache: &Path, journal: &Journal) -> PathBuf {
        let path = Journal::path(cache);
        journal.save(&path).unwrap();
        path
    }

    #[test]
    fn committed_journal_is_dropped_without_touching_files() {
        let dir = temp_dir("committed");
        let cache = dir.join(".app-update");
        std::fs::create_dir_all(&cache).unwrap();
        let path = write_journal(
            &cache,
            &Journal {
                schema: SCHEMA,
                version: "1.0.0".into(),
                entries: vec![JournalEntry {
                    target: dir.join("app.exe").to_string_lossy().to_string(),
                    backup: None,
                    action: EntryAction::Write,
                    renamed_from_running: false,
                }],
                committed: true,
            },
        );

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

        write_journal(
            &cache,
            &Journal {
                schema: SCHEMA,
                version: "2.0.0".into(),
                entries: vec![
                    JournalEntry {
                        target: target.to_string_lossy().to_string(),
                        backup: Some(backup.to_string_lossy().to_string()),
                        action: EntryAction::Write,
                        renamed_from_running: false,
                    },
                    JournalEntry {
                        target: added.to_string_lossy().to_string(),
                        backup: None,
                        action: EntryAction::Write,
                        renamed_from_running: false,
                    },
                ],
                committed: false,
            },
        );

        assert_eq!(recover(&cache, &dir).unwrap(), Recovery::RolledBack(2));
        assert_eq!(std::fs::read(&target).unwrap(), b"old");
        assert!(!added.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// BUG-01 回归：备份恰好是 `*.old-*` 时，必须先回滚再清理，不能先删备份。
    #[test]
    fn recovery_restores_old_image_backup_before_cleaning() {
        let dir = temp_dir("old-image");
        let cache = dir.join(".app-update");
        std::fs::create_dir_all(&cache).unwrap();
        let target = dir.join("updater.exe");
        let backup = dir.join("updater.exe.old-999999");
        std::fs::write(&backup, b"old").unwrap();
        std::fs::write(&target, b"new").unwrap();

        write_journal(
            &cache,
            &Journal {
                schema: SCHEMA,
                version: "3.0.0".into(),
                entries: vec![JournalEntry {
                    target: target.to_string_lossy().to_string(),
                    backup: Some(backup.to_string_lossy().to_string()),
                    action: EntryAction::Write,
                    renamed_from_running: true,
                }],
                committed: false,
            },
        );

        assert_eq!(recover(&cache, &dir).unwrap(), Recovery::RolledBack(1));
        assert_eq!(std::fs::read(&target).unwrap(), b"old");
        // 回滚完成后备份才被当作旧镜像清掉
        assert!(!backup.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// BUG-05 回归：新版删掉的旧文件在回滚时要还原。
    #[test]
    fn delete_entries_are_restored_from_backup() {
        let dir = temp_dir("deleted");
        let cache = dir.join(".app-update");
        std::fs::create_dir_all(cache.join("backup").join("__deleted__")).unwrap();
        let target = dir.join("legacy.dll");
        let backup = cache
            .join("backup")
            .join("__deleted__")
            .join("legacy.dll");
        std::fs::write(&backup, b"old-dll").unwrap();
        assert!(!target.exists());

        write_journal(
            &cache,
            &Journal {
                schema: SCHEMA,
                version: "4.0.0".into(),
                entries: vec![JournalEntry {
                    target: target.to_string_lossy().to_string(),
                    backup: Some(backup.to_string_lossy().to_string()),
                    action: EntryAction::Delete,
                    renamed_from_running: false,
                }],
                committed: false,
            },
        );

        assert_eq!(recover(&cache, &dir).unwrap(), Recovery::RolledBack(1));
        assert_eq!(std::fs::read(&target).unwrap(), b"old-dll");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// v1 journal（没有 action/schema 字段）仍能读出来。
    #[test]
    fn legacy_journal_without_action_is_still_readable() {
        let dir = temp_dir("legacy");
        let path = dir.join("journal.json");
        std::fs::write(
            &path,
            r#"{"version":"1.0.0","entries":[{"target":"a","backup":null}],"committed":false}"#,
        )
        .unwrap();
        let journal = Journal::load(&path).unwrap();
        assert_eq!(journal.schema, 0);
        assert_eq!(journal.entries[0].action, EntryAction::Write);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
