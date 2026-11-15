use crate::config::Config;
use crate::download::download_to;
use crate::error::AppError;
use crate::feed;
use crate::install::{self, InstallContext};
use crate::protocol::{emit, State};
use crate::state::UpdateState;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// 退出码约定：0 = 无更新；2 = 有更新 / 已就绪 / 更新成功；1 = 失败。
pub const EXIT_NO_UPDATE: i32 = 0;
pub const EXIT_UPDATE: i32 = 2;

pub enum Mode {
    /// 仅检查；`automatic` 为真时受最小检查间隔约束。
    Check { automatic: bool },
    /// 检查 + 下载 + 校验 + 解压暂存，停在 Ready，等主程序决定何时重启。
    Download,
    /// 主程序已退出：备份、覆盖、清理、拉起新版。
    Apply { parent_pid: Option<u32> },
    /// 清掉下载缓存与暂存目录。
    Clean,
}

fn build_client() -> Result<reqwest::blocking::Client, AppError> {
    reqwest::blocking::Client::builder()
        .user_agent("generic-updater/0.2")
        .timeout(Duration::from_secs(300))
        .build()
        .map_err(|e| AppError::Network(e.to_string()))
}

/// 上报失败帧并把错误包成统一类型，调用处只需 `Err(...)`。
fn fail(error: AppError, version: &str, notes: &str) -> AppError {
    emit(
        State::Failed,
        version,
        notes,
        0,
        0,
        Some(&error.to_string()),
        false,
    );
    error
}

pub fn run(cfg: &Config, mode: Mode) -> Result<i32, AppError> {
    match mode {
        Mode::Check { automatic } => check(cfg, automatic),
        Mode::Download => download(cfg),
        Mode::Apply { parent_pid } => apply(cfg, parent_pid),
        Mode::Clean => clean(cfg),
    }
}

fn check(cfg: &Config, automatic: bool) -> Result<i32, AppError> {
    let state_path = cfg.state_path();
    let mut state = UpdateState::load(&state_path);

    if automatic && !state.auto_check_due(cfg.automatic_check_interval_hours) {
        // 距离上次检查还不到间隔：按"无事发生"处理，避免每次启动都联网。
        emit(State::Idle, "", "", 0, 0, None, true);
        return Ok(EXIT_NO_UPDATE);
    }

    emit(State::Checking, "", "", 0, 0, None, automatic);

    let client = build_client()?;
    let feed = match feed::resolve(&client, cfg) {
        Ok(feed) => feed,
        Err(error) => return Err(fail(error, "", "")),
    };

    // 真正拿到元数据才算检查过；检查失败不更新时间戳，下次启动会重试。
    state.mark_checked();
    state.save(&state_path)?;

    let local = cfg.local_version()?;
    let version = feed.version.to_string();

    if feed.version <= local {
        emit(State::Idle, &version, "", 0, 0, None, automatic);
        return Ok(EXIT_NO_UPDATE);
    }

    emit(
        State::Available,
        &version,
        &feed.notes,
        0,
        feed.size,
        None,
        automatic,
    );
    Ok(EXIT_UPDATE)
}

fn download(cfg: &Config) -> Result<i32, AppError> {
    let state_path = cfg.state_path();
    let mut state = UpdateState::load(&state_path);

    emit(State::Checking, "", "", 0, 0, None, false);
    let client = build_client()?;
    let feed = match feed::resolve(&client, cfg) {
        Ok(feed) => feed,
        Err(error) => return Err(fail(error, "", "")),
    };

    state.mark_checked();
    state.save(&state_path)?;

    let local = cfg.local_version()?;
    let version = feed.version.to_string();
    let notes = feed.notes.clone();

    if feed.version <= local {
        emit(State::Idle, &version, &notes, 0, 0, None, false);
        return Ok(EXIT_NO_UPDATE);
    }

    emit(State::Available, &version, &notes, 0, feed.size, None, false);

    let cache = cfg.cache_dir();
    std::fs::create_dir_all(&cache)?;
    let dest = cache.join(feed.asset_name());

    // 上一次下载可能已经完整落地（比如用户点了取消又重试），哈希对得上就不重复下载。
    let cached = dest.exists()
        && crate::download::sha256_file(&dest)
            .map(|hash| hash.eq_ignore_ascii_case(&feed.sha256))
            .unwrap_or(false);

    if !cached {
        let downloaded = download_to(
            &client,
            &feed.source(),
            &dest,
            Some(&feed.sha256),
            feed.size,
            |received, total| {
                emit(
                    State::Downloading,
                    &version,
                    &notes,
                    received,
                    total,
                    None,
                    false,
                )
            },
        );
        if let Err(error) = downloaded {
            return Err(fail(error, &version, &notes));
        }
    }

    emit(State::Verifying, &version, &notes, 0, 0, None, false);

    let staging = cfg.staging_dir();
    let prepared = (|| -> Result<(), AppError> {
        if staging.exists() {
            std::fs::remove_dir_all(&staging)?;
        }
        install::prepare_staging(&dest, &staging)
    })();
    if let Err(error) = prepared {
        return Err(fail(error, &version, &notes));
    }

    state.pending_version = Some(version.clone());
    state.pending_notes = Some(notes.clone());
    state.pending_staging = Some(staging.to_string_lossy().to_string());
    state.save(&state_path)?;

    emit(State::Ready, &version, &notes, 1, 1, None, false);
    Ok(EXIT_UPDATE)
}

/// 等待旧进程退出并完成替换：Windows 上运行中的 exe 会被占用，
/// 这里用"重试到截止时间"代替 OpenProcess 轮询，零额外依赖。
fn install_with_retry(
    staging: &Path,
    install_dir: &Path,
    backup: &Path,
    deadline: Duration,
) -> Result<InstallContext, AppError> {
    let started = Instant::now();
    loop {
        match install::install(staging, install_dir, backup) {
            Ok(ctx) => return Ok(ctx),
            Err(error) if error.is_transient() && started.elapsed() < deadline => {
                std::thread::sleep(Duration::from_millis(500));
            }
            Err(error) => return Err(error),
        }
    }
}

fn apply(cfg: &Config, _parent_pid: Option<u32>) -> Result<i32, AppError> {
    let state_path = cfg.state_path();
    let mut state = UpdateState::load(&state_path);

    let staging = state
        .pending_staging
        .clone()
        .map(PathBuf::from)
        .filter(|path| path.exists())
        .ok_or_else(|| AppError::Install("没有待安装的更新，请先下载".to_string()))?;

    let version = state.pending_version.clone().unwrap_or_default();
    let notes = state.pending_notes.clone().unwrap_or_default();
    let parsed = semver::Version::parse(&version)
        .map_err(|e| AppError::Install(format!("待安装版本号非法 `{version}`: {e}")))?;

    emit(State::Applying, &version, &notes, 0, 0, None, false);

    let ctx = match install_with_retry(
        &staging,
        &cfg.install_dir(),
        &cfg.backup_dir(),
        Duration::from_secs(90),
    ) {
        Ok(ctx) => ctx,
        Err(error) => return Err(fail(error, &version, &notes)),
    };

    // 先落版本号再拉起新版；任何一步失败都按备份回滚，避免留下半新半旧的文件树。
    if let Err(error) = cfg.write_local_version(&parsed) {
        let _ = install::rollback(&ctx);
        install::cleanup(&ctx);
        return Err(fail(error, &version, &notes));
    }
    if let Err(error) = relaunch(cfg) {
        let _ = install::rollback(&ctx);
        install::cleanup(&ctx);
        return Err(fail(error, &version, &notes));
    }

    // 安装成功：暂存、备份、下载的压缩包都没用了。
    install::cleanup(&ctx);

    // 清掉整个缓存目录，避免把上一次的下载和 staging 留在用户机器上。
    let _ = std::fs::remove_dir_all(cfg.cache_dir());
    state.clear_pending();
    let _ = state.save(&state_path);

    emit(State::Ready, &version, &notes, 1, 1, None, false);
    Ok(EXIT_UPDATE)
}

fn clean(cfg: &Config) -> Result<i32, AppError> {
    let cache = cfg.cache_dir();
    if cache.exists() {
        std::fs::remove_dir_all(&cache)?;
    }
    emit(State::Idle, "", "", 0, 0, None, false);
    Ok(EXIT_NO_UPDATE)
}

fn relaunch(cfg: &Config) -> Result<(), AppError> {
    let name = match &cfg.exe_name {
        Some(name) => name.clone(),
        None => format!(
            "{}.exe",
            cfg.repo.rsplit('/').next().unwrap_or("app")
        ),
    };
    let exe = cfg.install_dir().join(&name);
    if !exe.exists() {
        return Err(AppError::Install(format!(
            "找不到新版程序: {}",
            exe.display()
        )));
    }

    let mut command = std::process::Command::new(&exe);
    command.current_dir(cfg.install_dir());
    // 不要把自己的 stdout/stderr 管道句柄继承给新进程，否则调用方读管道会一直等不到 EOF。
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP：updater 退出后新进程继续活着。
        command.creation_flags(0x0000_0008 | 0x0000_0200);
    }
    command
        .spawn()
        .map_err(|e| AppError::Install(format!("拉起新版失败: {e}")))?;
    Ok(())
}
