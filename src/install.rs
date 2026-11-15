use crate::error::AppError;
use std::path::Path;
use walkdir::WalkDir;
use zip::ZipArchive;

/// 一次安装的上下文，回滚和清理都靠它。
pub struct InstallContext {
    pub staging: std::path::PathBuf,
    pub backup: std::path::PathBuf,
    pub target_root: std::path::PathBuf,
}

/// 把 zip 解压到 staging 目录；exe 资产则直接复制进去。
pub fn prepare_staging(
    asset_path: &Path,
    staging: &Path,
) -> Result<(), AppError> {
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

/// 正在运行的 updater 自身的 exe 路径（规范化后用于比较）。
fn running_exe() -> Option<std::path::PathBuf> {
    std::env::current_exe().ok().and_then(|path| path.canonicalize().ok())
}

/// 逐文件备份旧版本到 backup，再把 staging 覆盖到 target_root。
/// 任何一步出错都可以用 [`rollback`] 还原。
///
/// 注意：如果发布包里也带了 `generic-updater.exe`，它此刻正被运行中的本进程占用。
/// Windows 不允许覆盖正在执行的镜像，所以这里跳过它——updater 自身的升级需要
/// "先复制到临时目录再由副本替换"的方案，等后续再补。
pub fn install(
    staging: &Path,
    target_root: &Path,
    backup: &Path,
) -> Result<InstallContext, AppError> {
    std::fs::create_dir_all(target_root)?;
    std::fs::create_dir_all(backup)?;

    let running = running_exe();

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
        let backup_file = backup.join(rel);

        if let (Some(running), Ok(target_canonical)) = (&running, target.canonicalize()) {
            if *running == target_canonical {
                eprintln!("跳过正在运行的更新器自身: {}", target.display());
                continue;
            }
        }

        if let Some(p) = target.parent() {
            std::fs::create_dir_all(p)?;
        }
        // 旧文件先备份，再覆盖。
        if target.exists() {
            if let Some(p) = backup_file.parent() {
                std::fs::create_dir_all(p)?;
            }
            std::fs::copy(&target, &backup_file)?;
        }
        std::fs::copy(entry.path(), &target)?;
    }

    Ok(InstallContext {
        staging: staging.to_path_buf(),
        backup: backup.to_path_buf(),
        target_root: target_root.to_path_buf(),
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

/// 安装并启动成功后，清理暂存与备份。
pub fn cleanup(ctx: &InstallContext) {
    let _ = std::fs::remove_dir_all(&ctx.staging);
    let _ = std::fs::remove_dir_all(&ctx.backup);
}
