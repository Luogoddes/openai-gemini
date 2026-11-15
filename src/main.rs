mod config;
mod contract;
mod download;
mod error;
mod feed;
mod install;
mod journal;
mod platform;
mod protocol;
mod state;
mod update;

use std::env;
use update::Mode;

const USAGE: &str = "\
用法: generic-updater <模式> [config.json] [选项]

模式:
  --check [--auto]   检查更新；--auto 时受 automatic_check_interval_hours 约束
  --download         检查 + 下载 + 校验 + 解压暂存（停在 Ready）
  --apply            应用已下载的更新：备份、覆盖、清理、拉起新版
  --recover          只做事务恢复（上次安装中途崩溃时按备份回滚）
  --clean            清掉下载缓存与暂存目录
  --verify-feed <json> 本地校验发布 feed 的签名，打印 keyId 与版本

选项:
  --parent-pid <pid> 主程序 PID，安装前精确等待它退出
  -c, --config <path> 调试用：用 JSON 配置覆盖编译期内置配置
  --elevated         内部参数：本进程由 UAC 提权拉起

不带 config.json 时使用编译进二进制的 updater.conf 配置。
";

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();

    let automatic = args.iter().any(|arg| arg == "--auto");
    let mut config_path: Option<String> = None;
    let mut parent_pid: Option<u32> = None;
    let mut mode: Option<Mode> = None;
    let mut unknown: Option<String> = None;
    let mut verify_feed: Option<String> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--verify-feed" => {
                i += 1;
                verify_feed = args.get(i).cloned();
            }
            "--check" => mode = Some(Mode::Check { automatic }),
            "--auto" => {
                mode.get_or_insert(Mode::Check { automatic: true });
            }
            "--download" | "--update" => mode = Some(Mode::Download),
            "--apply" => mode = Some(Mode::Apply { parent_pid: None }),
            "--recover" => mode = Some(Mode::Recover),
            "--clean" => mode = Some(Mode::Clean),
            "--elevated" => {}
            "--parent-pid" => {
                i += 1;
                parent_pid = args.get(i).and_then(|value| value.parse().ok());
            }
            "--config" | "-c" => {
                i += 1;
                config_path = args.get(i).cloned();
            }
            value if value.ends_with(".json") => config_path = Some(value.to_string()),
            value => unknown = Some(value.to_string()),
        }
        i += 1;
    }

    if let Some(value) = unknown {
        eprintln!("未知参数: {value}\n\n{USAGE}");
        std::process::exit(1);
    }

    if let Some(path) = verify_feed {
        match update::verify_feed(&path) {
            Ok(code) => std::process::exit(code),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
    }

    let mode = match mode {
        Some(Mode::Apply { .. }) => Mode::Apply { parent_pid },
        Some(mode) => mode,
        None => {
            eprintln!("{USAGE}");
            std::process::exit(1);
        }
    };

    let config_path = config_path.or_else(|| std::env::var("UPDATER_CONFIG").ok());
    let cfg = match config_path {
        Some(path) => match config::Config::load(&path) {
            Ok(cfg) => cfg,
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        },
        None => match config::Config::embedded() {
            Ok(cfg) => cfg,
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        },
    };

    match update::run(&cfg, mode, &args) {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}
