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
    let mut produced_old_images = Vec::new();
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
    }

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
