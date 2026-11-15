mod config;
mod download;
mod error;
mod feed;
mod install;
mod protocol;
mod state;
mod update;

use std::env;
use update::Mode;

const USAGE: &str = "\
用法: generic-updater <模式> <config.json> [选项]

模式:
  --check [--auto]   检查更新；--auto 时受 automatic_check_interval_hours 约束
  --download         检查 + 下载 + 校验 + 解压暂存（停在 Ready）
  --apply            应用已下载的更新：备份、覆盖、清理、拉起新版
  --clean            清掉下载缓存与暂存目录

选项:
  --parent-pid <pid> 主程序 PID（保留，当前用文件占用重试代替）
  -c, --config <path> 配置文件路径
";

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();

    let automatic = args.iter().any(|arg| arg == "--auto");
    let mut config_path: Option<String> = None;
    let mut parent_pid: Option<u32> = None;
    let mut mode: Option<Mode> = None;
    let mut unknown: Option<String> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--check" => mode = Some(Mode::Check { automatic }),
            "--auto" => {
                mode.get_or_insert(Mode::Check { automatic: true });
            }
            "--download" | "--update" => mode = Some(Mode::Download),
            "--apply" => mode = Some(Mode::Apply { parent_pid: None }),
            "--clean" => mode = Some(Mode::Clean),
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

    let mode = match mode {
        Some(Mode::Apply { .. }) => Mode::Apply { parent_pid },
        Some(mode) => mode,
        None => {
            eprintln!("{USAGE}");
            std::process::exit(1);
        }
    };

    let config_path = match config_path {
        Some(path) => path,
        None => {
            eprintln!("缺少配置文件路径\n\n{USAGE}");
            std::process::exit(1);
        }
    };

    let cfg = match config::Config::load(&config_path) {
        Ok(cfg) => cfg,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    };

    match update::run(&cfg, mode) {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}
