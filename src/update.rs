use crate::config::Config;
use crate::download::download_to;
use crate::error::AppError;
use crate::feed::fetch_latest;
use crate::install::{self, InstallContext};
use crate::protocol::{emit, State};
use std::path::PathBuf;

pub enum Mode {
    /// 只检查，不下载不安装。
    Check,
    /// 完整检查 + 下载 + 校验 + 安装。
    Update,
}

/// 退出码约定：0 = 无更新，2 = 有更新/更新成功，1 = 失败。
pub fn run(cfg: &Config, mode: Mode, parent_pid: Option<u32>) -> Result<i32, AppError> {
    let client = reqwest::blocking::Client::builder()
        .user_agent("generic-updater/0.1")
        .build()
        .map_err(|e| AppError::Network(e.to_string()))?;

    emit(State::Checking, "", "", 0, 0, None);

    let release = fetch_latest(&client, &cfg.repo, &cfg.asset_name)?;
    let local = cfg.local_version()?;

    if release.version <= local {
        emit(State::Idle, &release.version.to_string(), "", 0, 0, None);
        return Ok(0);
    }

    emit(
        State::Available,
        &release.version.to_string(),
        &release.notes,
        0,
        0,
        None,
    );

    if let Mode::Check = mode {
        // 仅检查：告诉主程序有新版本，退出码 2。
        return Ok(2);
    }

    // 1. 下载到暂存目录的 .part 文件，边下边校验。
    let cache = cfg.install_dir().join(".app-update");
    std::fs::create_dir_all(&cache)?;
    let dest = cache.join(&cfg.asset_name);

    let version = release.version.to_string();
    let notes = release.notes.clone();
    let asset = release.asset.clone();

    download_to(
        &client,
        &asset.url,
        &dest,
        asset.sha256.as_deref(),
        asset.size,
        |received, total| emit(State::Downloading, &version, &notes, received, total, None),
    )?;

    emit(State::Verifying, &version, &notes, 0, 0, None);

    // 2. 解压 / 展开到 staging。
    let staging = cache.join("staging");
    let backup = cache.join("backup");
    install::prepare_staging(&dest, &staging)?;

    // 3. 等待主程序退出，释放 exe 文件占用。
    //    生产版 Windows 下用 OpenProcess + WaitForSingleObject 轮询 parent_pid；
    //    这里先留钩子，主程序也可以在调用 apply 前自行退出。
    wait_for_parent(parent_pid);

    // 4. 备份旧文件并覆盖；失败立即回滚。
    let ctx = match install::install(&staging, &cfg.install_dir(), &backup) {
        Ok(ctx) => ctx,
        Err(e) => {
            emit(State::Failed, &version, &notes, 0, 0, Some(&e.to_string()));
            return Err(e);
        }
    };

    emit(State::Applying, &version, &notes, 0, 0, None);

    // 5. 记录新版本号。
    if let Err(e) = cfg.write_local_version(&release.version) {
        rollback_and_cleanup(&ctx);
        emit(State::Failed, &version, &notes, 0, 0, Some(&e.to_string()));
        return Err(e);
    }

    // 6. 拉起新版。生产版 Windows 应带 DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP。
    if let Err(e) = relaunch(cfg, &staging) {
        rollback_and_cleanup(&ctx);
        emit(State::Failed, &version, &notes, 0, 0, Some(&e.to_string()));
        return Err(e);
    }

    // 7. 启动成功（第一版以 spawn 成功为准；可扩展为启动标记文件确认）。
    install::cleanup(&ctx);
    emit(State::Ready, &version, &notes, 1, 1, None);
    Ok(2)
}

fn rollback_and_cleanup(ctx: &InstallContext) {
    let _ = install::rollback(ctx);
    install::cleanup(ctx);
}

fn wait_for_parent(_pid: Option<u32>) {
    // TODO(windows): OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION) 轮询等待旧进程退出。
    // 非 Windows 测试环境直接跳过。
}

fn relaunch(cfg: &Config, staging: &PathBuf) -> Result<(), AppError> {
    let exe_name = cfg
        .exe_name
        .as_ref()
        .map(|s| s.as_str())
        .unwrap_or_else(|| {
            // 从资产名推断主程序 exe 名。
            cfg.asset_name
                .strip_suffix(".zip")
                .unwrap_or(&cfg.asset_name)
        });

    // 单 exe 场景：目标 exe 在安装目录；zip 场景主程序名由 exe_name 指定。
    let exe = cfg.install_dir().join(exe_name);
    if !exe.exists() {
        // staging 里可能直接就是 exe。
        let staged = staging.join(exe_name);
        if staged.exists() {
            std::process::Command::new(&staged)
                .spawn()
                .map_err(|e| AppError::Install(format!("拉起新版失败: {e}")))?;
            return Ok(());
        }
        return Err(AppError::Install(format!("找不到新版程序: {exe_name}")));
    }

    std::process::Command::new(&exe)
        .spawn()
        .map_err(|e| AppError::Install(format!("拉起新版失败: {e}")))?;
    Ok(())
}
