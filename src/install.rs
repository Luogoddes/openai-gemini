use crate::error::AppError;
use crate::journal::{self, EntryAction, Journal, JournalEntry};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;
use zip::ZipArchive;

/// 一次安装的上下文，回滚和清理都靠它。
pub struct InstallContext {
    pub staging: PathBuf,
    pub backup: PathBuf,
    /// 这次安装对应的事务（已标记 committed）。
    pub journal: Journal,
    pub journal_path: PathBuf,
    /// 运行中被改名成 `*.old-<pid>` 的镜像。
    pub produced_old_images: Vec<PathBuf>,
}

/// 把 zip 解压到 staging 目录；exe 资产则直接复制进去。
pub fn prepare_staging(asset_path: &Path, staging: &Path) -> Result<(), AppError> {
    std::fs::create_dir_all(staging)?;
    let ext = asset_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_lowercase();

    match ext.as_str() {
        "zip" => {
            let file = std::fs::File::open(asset_path)?;
            let mut archive = ZipArchive::new(file)
                .map_err(|e| AppError::Install(format!("zip 打开失败: {e}")))?;
            for i in 0..archive.len() {
                let mut entry = archive
                    .by_index(i)
                    .map_err(|e| AppError::Install(format!("zip 条目读取失败: {e}")))?;
                let out = staging.join(entry.mangled_name());
                if entry.is_dir() {
                    std::fs::create_dir_all(&out)?;
                } else {
                    if let Some(p) = out.parent() {
                        std::fs::create_dir_all(p)?;
                    }
                    let mut out_file = std::fs::File::create(&out)?;
                    std::io::copy(&mut entry, &mut out_file)
                        .map_err(|e| AppError::Install(format!("解压写入失败: {e}")))?;
                }
            }
        }
        "exe" => {
            let name = asset_path
                .file_name()
                .ok_or_else(|| AppError::Install("exe 资产缺少文件名".to_string()))?;
            std::fs::copy(asset_path, staging.join(name))?;
        }
        other => {
            return Err(AppError::Install(format!("不支持的资产类型: .{other}")));
        }
    }
    Ok(())
}

/// 安装清单跟着程序走（放在安装目录、隐藏属性），这样清缓存也不会丢失"该删哪些旧文件"。
const MANIFEST_NAME: &str = ".update-files.json";

fn installed_list_path(install_dir: &Path) -> PathBuf {
    install_dir.join(MANIFEST_NAME)
}

#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn SetFileAttributesW(name: *const u16, attributes: u32) -> i32;
}

fn hide_file(path: &Path) {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
        unsafe {
            SetFileAttributesW(wide.as_ptr(), FILE_ATTRIBUTE_HIDDEN);
        }
    }
    #[cfg(not(windows))]
    {
        let _ = path;
    }
}

fn read_installed(install_dir: &Path, cache_dir: &Path) -> Vec<String> {
    let current = installed_list_path(install_dir);
    if let Ok(text) = std::fs::read_to_string(&current) {
        if let Ok(list) = serde_json::from_str::<Vec<String>>(&text) {
            return list;
        }
    }
    // 从旧版本（清单在缓存里）迁移一次
    let legacy = cache_dir.join("installed.json");
    if let Ok(text) = std::fs::read_to_string(&legacy) {
        if let Ok(list) = serde_json::from_str::<Vec<String>>(&text) {
            return list;
        }
    }
    Vec::new()
}

fn write_installed(install_dir: &Path, files: &[String]) -> Result<(), AppError> {
    let text = serde_json::to_string_pretty(files)
        .map_err(|e| AppError::Other(format!("文件清单序列化失败: {e}")))?;
    let path = installed_list_path(install_dir);
    std::fs::write(&path, text)?;
    hide_file(&path);
    Ok(())
}

/// 把清单里的相对路径转成安全的 PathBuf；越界路径直接丢弃。
fn safe_relative(relative: &str) -> Option<PathBuf> {
    let mut clean = PathBuf::new();
    for part in relative.split(['/', '\\']) {
        if part.is_empty() || part == "." || part == ".." {
            return None;
        }
        clean.push(part);
    }
    if clean.as_os_str().is_empty() {
        None
    } else {
        Some(clean)
    }
}

fn io_error(error: std::io::Error, what: String) -> AppError {
    let kind = error.kind();
    AppError::Io(std::io::Error::new(kind, format!("{what}: {error}")))
}

/// 只做规划，不动文件：得出要写哪些、要删哪些、旧文件备份到哪。
///
/// 规划结果是幂等的：同一份 staging 与安装清单一再调用会得到同一份 journal，
/// 因此安装失败重试时可以复用上一次的计划，不会把「已经写进去的新文件」误当旧文件备份。
fn plan_journal(
    staging: &Path,
    target_root: &Path,
    backup_root: &Path,
    cache_dir: &Path,
    version: &str,
) -> Result<Journal, AppError> {
    let mut entries: Vec<JournalEntry> = Vec::new();
    let mut current: Vec<String> = Vec::new();

    for entry in WalkDir::new(staging) {
        let entry = entry.map_err(|e| AppError::Install(format!("遍历暂存目录失败: {e}")))?;
        if !entry.file_type().is_file() {
            continue;
        }
        let rel = entry
            .path()
            .strip_prefix(staging)
            .map_err(|e| AppError::Install(e.to_string()))?;
        let target = target_root.join(rel);
        let running = journal::is_running_image(&target);

        let backup = if running {
            Some(journal::old_image_path(&target))
        } else if target.exists() {
            Some(backup_root.join(rel))
        } else {
            None
        };

        current.push(rel.to_string_lossy().replace('\\', "/"));
        entries.push(JournalEntry {
            target: target.to_string_lossy().to_string(),
            backup: backup.map(|path| path.to_string_lossy().to_string()),
            action: EntryAction::Write,
            renamed_from_running: running,
        });
    }

    // 上个版本有、这个版本没有的文件：移到备份区，而不是直接删掉。
    for relative in read_installed(target_root, cache_dir) {
        if current.iter().any(|path| path == &relative) {
            continue;
        }
        let Some(rel) = safe_relative(&relative) else {
            continue;
        };
        let target = target_root.join(&rel);
        if !target.is_file() {
            continue;
        }
        let running = journal::is_running_image(&target);
        let backup = if running {
            journal::old_image_path(&target)
        } else {
            backup_root.join("__deleted__").join(&rel)
        };
        entries.push(JournalEntry {
            target: target.to_string_lossy().to_string(),
            backup: Some(backup.to_string_lossy().to_string()),
            action: EntryAction::Delete,
            renamed_from_running: running,
        });
    }

    Ok(Journal::new(version, entries))
}

/// 执行 journal。每一步都先看备份是否已经存在，因此可以安全地重复执行。
fn execute(
    journal: &Journal,
    staging: &Path,
    target_root: &Path,
) -> Result<Vec<PathBuf>, AppError> {
    let mut old_images: Vec<PathBuf> = Vec::new();

    for entry in &journal.entries {
        let target = PathBuf::from(&entry.target);

        match entry.action {
            EntryAction::Write => {
                let rel = target
                    .strip_prefix(target_root)
                    .map_err(|e| AppError::Install(e.to_string()))?;
                let source = staging.join(rel);

                if let Some(backup) = &entry.backup {
                    let backup_path = PathBuf::from(backup);
                    if !backup_path.exists() && target.exists() {
                        if let Some(parent) = backup_path.parent() {
                            std::fs::create_dir_all(parent)?;
                        }
                        if entry.renamed_from_running {
                            std::fs::rename(&target, &backup_path).map_err(|error| {
                                io_error(
                                    error,
                                    format!("重命名运行中的镜像 {} 失败", target.display()),
                                )
                            })?;
                            eprintln!(
                                "已把运行中的镜像改名为 {}，稍后写入新版本",
                                backup_path.display()
                            );
                        } else {
                            std::fs::copy(&target, &backup_path).map_err(|error| {
                                io_error(error, format!("备份 {} 失败", target.display()))
                            })?;
                        }
                    }
                }

                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::copy(&source, &target)
                    .map_err(|error| io_error(error, format!("写入 {} 失败", target.display())))?;
            }
            EntryAction::Delete => {
                let Some(backup) = &entry.backup else {
                    continue;
                };
                let backup_path = PathBuf::from(backup);
                if backup_path.exists() {
                    // 上一轮已经把文件移走了，这里只清掉可能残留的目标。
                    if target.is_file() {
                        std::fs::remove_file(&target).map_err(|error| {
                            io_error(error, format!("删除 {} 失败", target.display()))
                        })?;
                    }
                    continue;
                }
                if !target.is_file() {
                    continue;
                }
                if let Some(parent) = backup_path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::rename(&target, &backup_path)
                    .map_err(|error| io_error(error, format!("移走旧文件 {} 失败", target.display())))?;
                eprintln!("旧版本遗留文件已移入备份区: {}", target.display());
            }
        }
    }

    // 运行中镜像的备份就是 `*.old-<pid>`，需要记录给清理阶段。
    for entry in &journal.entries {
        if entry.renamed_from_running {
            if let Some(backup) = &entry.backup {
                old_images.push(PathBuf::from(backup));
            }
        }
    }

    Ok(old_images)
}

/// 逐文件备份旧版本到 backup，再把 staging 覆盖到 target_root。
///
/// 覆盖前先把「计划」写入 journal（未提交），装完再标记 committed：
/// 中途掉电可以由 `journal::recover` 按备份还原。
///
/// 如果发布包里带了正在运行的 `generic-updater.exe`，Windows 不允许覆盖运行中的镜像，
/// 这里改成「先把旧镜像改名成 `*.old-<pid>`，再写新文件」，下次启动时清掉改名残留。
pub fn install(
    staging: &Path,
    target_root: &Path,
    backup: &Path,
    cache_dir: &Path,
    version: &str,
) -> Result<InstallContext, AppError> {
    std::fs::create_dir_all(target_root)?;
    std::fs::create_dir_all(backup)?;

    let journal_path = Journal::path(cache_dir);
    // 同一版本、未提交的事务说明上一轮安装失败：复用它，别重新规划。
    // 否则第二轮会把「已经写进去的新文件」当成旧文件备份，回滚就回不去了。
    let journal = if journal_path.exists() {
        match Journal::load(&journal_path) {
            Some(existing) if !existing.committed && existing.version == version => existing,
            Some(existing) if !existing.committed => {
                return Err(AppError::Install(
                    "检测到其他版本未完成的安装事务，请先运行 --recover".to_string(),
                ));
            }
            Some(_) => {
                let _ = std::fs::remove_file(&journal_path);
                plan_journal(staging, target_root, backup, cache_dir, version)?
            }
            None => {
                return Err(AppError::Install(
                    "journal.json 无法解析，请先运行 --recover 或人工检查缓存目录".to_string(),
                ));
            }
        }
    } else {
        plan_journal(staging, target_root, backup, cache_dir, version)?
    };
    journal.save(&journal_path)?;

    let produced_old_images = execute(&journal, staging, target_root)?;

    let installed: Vec<String> = journal
        .entries
        .iter()
        .filter(|entry| entry.action == EntryAction::Write)
        .filter_map(|entry| {
            Path::new(&entry.target)
                .strip_prefix(target_root)
                .ok()
                .map(|relative| relative.to_string_lossy().replace('\\', "/"))
        })
        .collect();
    write_installed(target_root, &installed)?;

    let committed = Journal {
        committed: true,
        ..journal.clone()
    };
    committed.save(&journal_path)?;

    Ok(InstallContext {
        staging: staging.to_path_buf(),
        backup: backup.to_path_buf(),
        journal: committed,
        journal_path,
        produced_old_images,
    })
}

/// 按 journal 把旧文件还原回 target_root。
pub fn rollback(ctx: &InstallContext) -> Result<usize, AppError> {
    journal::rollback(&ctx.journal)
}

/// 安装成功后清理：暂存、备份、以及改名留下的旧镜像。
pub fn cleanup(ctx: &InstallContext) {
    let _ = std::fs::remove_dir_all(&ctx.staging);
    let _ = std::fs::remove_dir_all(&ctx.backup);
    for old in &ctx.produced_old_images {
        let _ = std::fs::remove_file(old);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("updater-install-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn install_moves_obsolete_files_into_backup_and_restores_them() {
        let dir = temp_dir("obsolete");
        let staging = dir.join("staging");
        let install_dir = dir.join("install");
        let backup = dir.join("backup");
        let cache = dir.join("cache");
        std::fs::create_dir_all(&staging).unwrap();
        std::fs::create_dir_all(&install_dir).unwrap();
        std::fs::write(staging.join("app.exe"), b"v2").unwrap();
        std::fs::write(install_dir.join("app.exe"), b"v1").unwrap();
        std::fs::write(install_dir.join("legacy.dll"), b"old-dll").unwrap();
        write_installed(
            &install_dir,
            &["app.exe".to_string(), "legacy.dll".to_string()],
        )
        .unwrap();

        let ctx = install(&staging, &install_dir, &backup, &cache, "2.0.0").unwrap();

        assert_eq!(std::fs::read(install_dir.join("app.exe")).unwrap(), b"v2");
        // 旧文件不是直接删掉，而是移到备份区
        assert!(!install_dir.join("legacy.dll").exists());
        let moved = backup.join("__deleted__").join("legacy.dll");
        assert_eq!(std::fs::read(&moved).unwrap(), b"old-dll");
        assert_eq!(
            read_installed(&install_dir, &cache),
            vec!["app.exe".to_string()]
        );

        // 回滚要把旧文件和旧版本都还回来
        assert_eq!(rollback(&ctx).unwrap(), 2);
        assert_eq!(std::fs::read(install_dir.join("app.exe")).unwrap(), b"v1");
        assert_eq!(
            std::fs::read(install_dir.join("legacy.dll")).unwrap(),
            b"old-dll"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// BUG-04 回归：重试时必须复用未提交的 journal，备份不能被新文件覆盖。
    #[test]
    fn retried_install_reuses_journal_and_keeps_original_backup() {
        let dir = temp_dir("retry");
        let staging = dir.join("staging");
        let install_dir = dir.join("install");
        let backup = dir.join("backup");
        let cache = dir.join("cache");
        std::fs::create_dir_all(&staging).unwrap();
        std::fs::create_dir_all(&install_dir).unwrap();
        std::fs::write(staging.join("app.exe"), b"v2").unwrap();
        std::fs::write(install_dir.join("app.exe"), b"v1").unwrap();

        // 模拟第一轮：规划 + 执行了，但还没来得及提交就崩了
        let journal = plan_journal(&staging, &install_dir, &backup, &cache, "2.0.0").unwrap();
        journal.save(&Journal::path(&cache)).unwrap();
        execute(&journal, &staging, &install_dir).unwrap();
        assert_eq!(std::fs::read(install_dir.join("app.exe")).unwrap(), b"v2");

        // 第二轮安装复用 journal：备份必须仍是 v1
        let ctx = install(&staging, &install_dir, &backup, &cache, "2.0.0").unwrap();
        assert_eq!(
            std::fs::read(backup.join("app.exe")).unwrap(),
            b"v1",
            "备份被第二轮安装覆盖成了新文件"
        );

        rollback(&ctx).unwrap();
        assert_eq!(std::fs::read(install_dir.join("app.exe")).unwrap(), b"v1");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn installed_inventory_round_trips() {
        let dir = temp_dir("inventory");
        let cache = dir.join("cache");
        write_installed(&dir, &["app.exe".to_string(), "version.txt".to_string()]).unwrap();
        assert_eq!(read_installed(&dir, &cache), vec!["app.exe", "version.txt"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unsafe_relative_paths_are_rejected() {
        assert!(safe_relative("../updater-outside.txt").is_none());
        assert!(safe_relative("a/../../b").is_none());
        assert!(safe_relative("/abs/path").is_none());
        assert!(safe_relative("").is_none());
        assert_eq!(
            safe_relative("dir/file.dll"),
            Some(PathBuf::from("dir").join("file.dll"))
        );
    }
}
