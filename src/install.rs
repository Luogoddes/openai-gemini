use crate::error::AppError;
use crate::journal::{Journal, JournalEntry};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;
use zip::ZipArchive;

/// 一次安装的上下文，回滚和清理都靠它。
pub struct InstallContext {
    pub staging: PathBuf,
    pub backup: PathBuf,
    pub target_root: PathBuf,
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

fn running_exe() -> Option<PathBuf> {
    std::env::current_exe().ok().and_then(|path| path.canonicalize().ok())
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

/// 删除"上个版本有、这个版本没有"的文件，避免升级后残留旧文件。
fn remove_obsolete(root: &Path, previous: &[String], current: &[String]) {
    for relative in previous {
        if current.iter().any(|path| path == relative) {
            continue;
        }
        let path = Path::new(relative);
        let safe = !path.is_absolute()
            && !relative.split(['/', '\\']).any(|part| part == ".." || part.is_empty());
        if !safe {
            continue;
        }
        let target = root.join(path);
        if target.is_file() {
            let _ = std::fs::remove_file(&target);
            eprintln!("删除旧版本遗留文件: {}", target.display());
        }
    }
}

fn old_image_path(target: &Path) -> PathBuf {
    let name = target
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("file");
    target.with_file_name(format!("{name}.old-{}", std::process::id()))
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

    let running = running_exe();

    // 第一遍：只做规划，不动文件。
    struct Plan {
        source: PathBuf,
        target: PathBuf,
        backup: Option<PathBuf>,
        rename_old: bool,
    }
    let mut plans: Vec<Plan> = Vec::new();

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

        let is_running = match (&running, target.canonicalize()) {
            (Some(running), Ok(canonical)) => *running == canonical,
            _ => false,
        };

        plans.push(Plan {
            source: entry.path().to_path_buf(),
            backup: if is_running {
                Some(old_image_path(&target))
            } else if target.exists() {
                Some(backup.join(rel))
            } else {
                None
            },
            rename_old: is_running,
            target,
        });
    }

    let journal = Journal {
        version: version.to_string(),
        entries: plans
            .iter()
            .map(|plan| JournalEntry {
                target: plan.target.to_string_lossy().to_string(),
                backup: plan.backup.as_ref().map(|path| path.to_string_lossy().to_string()),
            })
            .collect(),
        committed: false,
    };
    let journal_path = Journal::path(cache_dir);
    journal.save(&journal_path)?;

    // 第二遍：备份 + 覆盖。
    let previous_files = read_installed(target_root, cache_dir);
    let mut produced_old_images = Vec::new();
    let mut installed_files: Vec<String> = Vec::new();
    for plan in &plans {
        if let Some(parent) = plan.target.parent() {
            std::fs::create_dir_all(parent)?;
        }

        if plan.rename_old {
            if let Some(old) = &plan.backup {
                let _ = std::fs::remove_file(old);
                std::fs::rename(&plan.target, old).map_err(|e| {
                    AppError::Install(format!("重命名运行中的镜像失败 {}: {e}", plan.target.display()))
                })?;
                produced_old_images.push(old.clone());
                eprintln!(
                    "已把运行中的更新器改名为 {}，稍后写入新版本",
                    old.display()
                );
            }
        } else if let Some(backup_file) = &plan.backup {
            if plan.target.exists() {
                if let Some(parent) = backup_file.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::copy(&plan.target, backup_file)?;
            }
        }

        std::fs::copy(&plan.source, &plan.target)?;
        if let Ok(relative) = plan.target.strip_prefix(target_root) {
            installed_files.push(relative.to_string_lossy().replace('\\', "/"));
        }
    }

    remove_obsolete(target_root, &previous_files, &installed_files);
    write_installed(target_root, &installed_files)?;

    Journal::commit(&journal_path)?;

    Ok(InstallContext {
        staging: staging.to_path_buf(),
        backup: backup.to_path_buf(),
        target_root: target_root.to_path_buf(),
        produced_old_images,
    })
}

/// 按备份目录把旧文件还原回 target_root。
pub fn rollback(ctx: &InstallContext) -> Result<(), AppError> {
    for entry in WalkDir::new(&ctx.backup) {
        let entry = entry.map_err(|e| AppError::Install(format!("遍历备份目录失败: {e}")))?;
        if !entry.file_type().is_file() {
            continue;
        }
        let rel = entry
            .path()
            .strip_prefix(&ctx.backup)
            .map_err(|e| AppError::Install(e.to_string()))?;
        let target = ctx.target_root.join(rel);
        if let Some(p) = target.parent() {
            std::fs::create_dir_all(p)?;
        }
        std::fs::copy(entry.path(), &target)?;
    }
    Ok(())
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
    fn removes_files_missing_from_the_new_version() {
        let dir = temp_dir("obsolete");
        for name in ["a.txt", "b.txt", "c.txt"] {
            std::fs::write(dir.join(name), b"x").unwrap();
        }

        remove_obsolete(
            &dir,
            &["a.txt".to_string(), "b.txt".to_string()],
            &["a.txt".to_string(), "version.txt".to_string()],
        );

        assert!(dir.join("a.txt").exists());
        assert!(!dir.join("b.txt").exists());
        // 不在旧清单里的文件不动（用户自己放的文件不该被删）
        assert!(dir.join("c.txt").exists());
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
    fn inventory_ignores_unsafe_paths() {
        let dir = temp_dir("unsafe");
        let outside = dir.parent().unwrap().join("updater-outside.txt");
        std::fs::write(&outside, b"x").unwrap();

        remove_obsolete(
            &dir,
            &["../updater-outside.txt".to_string(), "keep.txt".to_string()],
            &[],
        );

        // 越界路径必须被忽略，不能删到安装目录外面
        assert!(outside.exists());
        let _ = std::fs::remove_file(&outside);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
