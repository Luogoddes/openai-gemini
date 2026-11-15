use crate::config::Config;
use crate::download::download_to;
use crate::error::AppError;
use crate::feed;
use crate::install::{self, InstallContext};
use crate::journal;
use crate::platform;
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
    /// 只做事务恢复（上次安装中途崩溃时按备份回滚）。
    Recover,
}

fn build_client() -> Result<reqwest::blocking::Client, AppError> {
    reqwest::blocking::Client::builder()
        .user_agent("generic-updater/0.4")
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

pub fn run(cfg: &Config, mode: Mode, args: &[String]) -> Result<i32, AppError> {
    match mode {
        Mode::Check { automatic } => check(cfg, automatic),
        Mode::Download => download(cfg),
        Mode::Apply { parent_pid } => apply(cfg, parent_pid, args),
        Mode::Clean => clean(cfg),
        Mode::Recover => recover(cfg),
    }
}

/// 本地校验一份已发布的 feed：签名是否有效、属于哪个 key、版本号多少。
/// 用于发布后自查，等价于客户端 `--check` 里做的验签步骤，但不联网、不需要配置。
pub fn verify_feed(path: &str) -> Result<i32, AppError> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| AppError::Config(format!("读取元数据失败 {path}: {e}")))?;

    let key_id = serde_json::from_str::<serde_json::Value>(&raw)
        .ok()
        .and_then(|outer| {
            outer
                .get("keyId")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| "<未签名>".to_string());

    let payload = crate::contract::verify_envelope(&raw)?;
    let value: serde_json::Value = serde_json::from_str(&payload)
        .map_err(|e| AppError::Verify(format!("payload 不是合法 JSON: {e}")))?;
    let version = value
        .get("version")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let packages = value
        .get("packages")
        .and_then(serde_json::Value::as_array)
        .map(|items| items.len())
        .unwrap_or(0);

    println!("签名有效: keyId={key_id} version={version} packages={packages}");
    Ok(EXIT_NO_UPDATE)
}

fn recover(cfg: &Config) -> Result<i32, AppError> {    match journal::recover(&cfg.cache_dir(), &cfg.install_dir())? {
        journal::Recovery::RolledBack(count) => {
            eprintln!("检测到上次中断的安装，已回滚 {count} 个文件");
            emit(State::Idle, "", "", 0, 0, None, false);
        }
        journal::Recovery::Committed | journal::Recovery::Clean => {}
    }
    Ok(EXIT_NO_UPDATE)
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

    // 上一次安装如果中断过，先恢复，避免在半个新半个旧的目录上继续。
    journal::recover(&cfg.cache_dir(), &cfg.install_dir())?;

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

/// 拉起新版并确认它真的起来了：
/// 主程序启动后会写 `started.flag`；只要进程还活着超过几秒也算启动成功，
/// 避免标记文件因为时序问题误判，导致"重启两次"。
fn probe_started(child: &mut std::process::Child, marker: &Path) -> bool {
    let started = Instant::now();
    loop {
        if marker.exists() {
            return true;
        }
        match child.try_wait() {
            Ok(Some(_)) => return false, // 子进程提前退出
            Ok(None) => {}
            Err(_) => return false,
        }
        if started.elapsed() > Duration::from_secs(8) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// 只清下载产物，保留 state.json / installed.json，避免下次失去 24h 时间戳和文件清单。
fn clean_downloads(cache: &Path) {
    for name in ["staging", "backup", "started.flag", "journal.json"] {
        let _ = std::fs::remove_dir_all(cache.join(name));
        let _ = std::fs::remove_file(cache.join(name));
    }
    if let Ok(entries) = std::fs::read_dir(cache) {
        for entry in entries.flatten() {
            let path = entry.path();
            let extension = path
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or_default()
                .to_lowercase();
            if extension == "zip" || extension == "part" {
                let _ = std::fs::remove_file(&path);
            }
        }
    }
}

fn apply(cfg: &Config, parent_pid: Option<u32>, args: &[String]) -> Result<i32, AppError> {
    let cache = cfg.cache_dir();
    // 上次安装若中途崩溃，先按 journal 回滚。
    journal::recover(&cache, &cfg.install_dir())?;

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

    // 精确等待旧进程退出（拿不到句柄就当作已退出）。
    if let Some(pid) = parent_pid {
        platform::wait_for_process(pid, Duration::from_secs(30));
    }

    // 目录不可写且当前没提权：用 UAC 重新拉起自己再装。
    if !platform::directory_writable(&cfg.install_dir()) {
        if platform::is_elevated() {
            let error = AppError::Install(format!(
                "安装目录仍不可写: {}",
                cfg.install_dir().display()
            ));
            return Err(fail(error, &version, &notes));
        }
        let executable = std::env::current_exe()
            .map_err(|e| AppError::Other(format!("取自身路径失败: {e}")))?;
        let mut elevated_args = args.to_vec();
        if !elevated_args.iter().any(|arg| arg == "--elevated") {
            elevated_args.push("--elevated".to_string());
        }
        if platform::relaunch_elevated(&executable, &elevated_args) {
            eprintln!("安装目录不可写，已请求 UAC 提权后重试");
            return Ok(EXIT_UPDATE);
        }
        let error = AppError::Install("安装目录不可写，且提权请求被拒绝".to_string());
        return Err(fail(error, &version, &notes));
    }

    let install_dir = cfg.install_dir();
    let backup = cfg.backup_dir();
    // 旧版本号必须在覆盖之前读：发布包里通常自带 version.txt，装完就变了。
    let previous_version = cfg.local_version().ok();
    let ctx = match platform::retry_until(Duration::from_secs(90), || {
        install::install(&staging, &install_dir, &backup, &cache, &version)
    }) {
        Ok(ctx) => ctx,
        Err(error) => return Err(fail(error, &version, &notes)),
    };

    if let Err(error) = cfg.write_local_version(&parsed) {
        restore_previous(&ctx, cfg, previous_version.as_ref());
        return Err(fail(error, &version, &notes));
    }

    // 启动探测：主程序起来后会写 started.flag；没写且进程提前退出就回滚。
    let marker = cfg.started_marker();
    let _ = std::fs::remove_file(&marker);
    if let Some(parent) = marker.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut child = match relaunch(cfg) {
        Ok(child) => child,
        Err(error) => {
            restore_previous(&ctx, cfg, previous_version.as_ref());
            return Err(fail(error, &version, &notes));
        }
    };

    if !probe_started(&mut child, &marker) {
        let error = AppError::Install("新版启动探测失败，已回滚到旧版本".to_string());
        restore_previous(&ctx, cfg, previous_version.as_ref());
        // 旧版本已经还原，重新拉起给用户一个能用的界面。
        let _ = relaunch(cfg);
        return Err(fail(error, &version, &notes));
    }

    let _ = std::fs::remove_file(&marker);
    install::cleanup(&ctx);
    clean_downloads(&cache);
    // started.flag 所在的目录如果没有别的内容，也一并清掉。
    let marker_dir = cfg.install_dir().join(".app-update");
    let _ = std::fs::remove_dir(&marker_dir);

    state.clear_pending();
    let _ = state.save(&state_path);

    emit(State::Ready, &version, &notes, 1, 1, None, false);
    Ok(EXIT_UPDATE)
}

fn restore_previous(
    ctx: &InstallContext,
    cfg: &Config,
    previous: Option<&semver::Version>,
) {
    let _ = install::rollback(ctx);
    if let Some(previous) = previous {
        let _ = cfg.write_local_version(previous);
    }
    install::cleanup(ctx);
    // 回滚完成后 journal 就没用了，删掉免得下次启动又回滚一遍。
    let _ = std::fs::remove_file(journal::Journal::path(&cfg.cache_dir()));
}

fn clean(cfg: &Config) -> Result<i32, AppError> {
    let cache = cfg.cache_dir();
    if cache.exists() {
        std::fs::remove_dir_all(&cache)?;
    }
    emit(State::Idle, "", "", 0, 0, None, false);
    Ok(EXIT_NO_UPDATE)
}

fn relaunch(cfg: &Config) -> Result<std::process::Child, AppError> {
    let name = match &cfg.exe_name {
        Some(name) => name.clone(),
        None => format!("{}.exe", cfg.repo.rsplit('/').next().unwrap_or("app")),
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
        .map_err(|e| AppError::Install(format!("拉起新版失败: {e}")))
}
