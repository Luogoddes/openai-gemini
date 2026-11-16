//! 构建期把 `.rc` 资源编译成 `.res` 并链接进 exe。
//!
//! 为什么需要它：没有 VERSIONINFO / 图标 / 清单的 PE，在公司终端上既不像一个
//! 正常软件，也没法被人一眼看出"这是什么、哪个版本、谁发布的"。资源不是免杀
//! 开关，但它是产品身份的最小组成。
//!
//! 用法（各包自己的 build.rs）：
//! ```ignore
//! fn main() {
//!     let root = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
//!     build_common::compile_rc(&root.join("resources/demo.rc"));
//! }
//! ```
//!
//! `.rc` 里可用两个占位符，构建时按下文优先级替换：
//!   * `@FILEVERSION@`   → 四段数字，如 `0,6,0,0`（VERSIONINFO 的 FILEVERSION）
//!   * `@VERSION_STRING@` → 点分字符串，如 `0.6.0`（VERSIONINFO 的字符串值）
//!
//! 版本来源：`RELEASE_VERSION` 环境变量（CI 由 tag 传入）> `CARGO_PKG_VERSION`。

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// 编译一个 `.rc` 文件并把生成的 `.res` 交给链接器。
///
/// 找不到 `rc.exe` 时直接 panic：静默产出一个"没有身份"的 exe 比构建失败更糟。
pub fn compile_rc(rc_file: &Path) {
    println!("cargo:rerun-if-changed={}", rc_file.display());
    println!("cargo:rerun-if-env-changed=RELEASE_VERSION");
    println!("cargo:rerun-if-env-changed=RC");

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let version = release_version();

    // 资源文件只放 ASCII，避免 rc.exe 的代码页问题；中文一律写在 Rust 代码里。
    let source = fs::read_to_string(rc_file)
        .unwrap_or_else(|error| panic!("读取 {} 失败: {error}", rc_file.display()));
    let expanded = source
        .replace("@FILEVERSION@", &numeric_version(&version))
        .replace("@VERSION_STRING@", &version);

    let generated_rc = out_dir.join("resource.rc");
    fs::write(&generated_rc, expanded).expect("写出生成的 .rc 失败");
    let res_file = out_dir.join("resource.res");

    let rc_exe = find_rc().unwrap_or_else(|| {
        panic!(
            "找不到 rc.exe（Windows SDK 资源编译器）。\n\
             请在 MSVC 环境里构建（vcvars/setup_x64.bat），或设置 RC 环境变量指向 rc.exe。\n\
             需要编译的资源文件: {}",
            rc_file.display()
        )
    });

    // include 目录指向 .rc 所在目录，这样 .rc 里可以直接写 "app.ico" / "app.manifest"。
    let include_dir = rc_file.parent().unwrap_or(&manifest_dir).to_path_buf();
    let status = Command::new(&rc_exe)
        .arg("/nologo")
        .arg("/fo")
        .arg(&res_file)
        .arg(format!("/I{}", include_dir.display()))
        .arg(&generated_rc)
        .status()
        .unwrap_or_else(|error| panic!("启动 {} 失败: {error}", rc_exe.display()));
    if !status.success() {
        panic!("rc.exe 编译 {} 失败（退出码 {:?}）", rc_file.display(), status.code());
    }

    // rustc 会把这条参数作为独立 argv 交给链接器，路径带空格也会被正确引用；
    // 自己再加引号反而会被当成文件名的一部分（LNK1104）。
    println!("cargo:rustc-link-arg={}", res_file.display());
}

fn release_version() -> String {
    let raw = env::var("RELEASE_VERSION")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION"));
    raw.trim().trim_start_matches('v').to_string()
}

fn numeric_version(version: &str) -> String {
    let core = version.split(['-', '+']).next().unwrap_or(version);
    let mut parts: Vec<u64> = core
        .split('.')
        .map(|part| part.parse::<u64>().unwrap_or(0))
        .collect();
    parts.resize(4, 0);
    format!("{},{},{},{}", parts[0], parts[1], parts[2], parts[3])
}

/// 依次尝试：RC 环境变量 → PATH 里的 rc.exe → vswhere 问 Visual Studio → 常见安装目录。
fn find_rc() -> Option<PathBuf> {
    if let Ok(value) = env::var("RC") {
        let path = PathBuf::from(value);
        if path.is_file() {
            return Some(path);
        }
    }

    if let Some(path) = search_path("rc.exe") {
        return Some(path);
    }

    if let Some(path) = vswhere_rc() {
        return Some(path);
    }

    scan_msvc_rc()
}

fn search_path(name: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    for dir in env::split_paths(&path) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn vswhere_rc() -> Option<PathBuf> {
    let program_files = env::var_os("ProgramFiles(x86)").or_else(|| env::var_os("ProgramFiles"))?;
    let vswhere = PathBuf::from(program_files)
        .join("Microsoft Visual Studio")
        .join("Installer")
        .join("vswhere.exe");
    if !vswhere.is_file() {
        return None;
    }
    let output = Command::new(vswhere)
        .args([
            "-latest",
            "-products",
            "*",
            "-requires",
            "Microsoft.VisualStudio.Component.VC.Tools.x86.x64",
            "-find",
            r"**\Hostx64\x64\rc.exe",
        ])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let first = text.lines().map(str::trim).find(|line| !line.is_empty())?;
    let path = PathBuf::from(first);
    path.is_file().then_some(path)
}

fn scan_msvc_rc() -> Option<PathBuf> {
    let program_files = env::var_os("ProgramFiles")?;
    let mut editions = Vec::new();
    for year in ["2022", "2019"] {
        for edition in ["Community", "Professional", "Enterprise", "BuildTools"] {
            editions.push(
                PathBuf::from(&program_files)
                    .join("Microsoft Visual Studio")
                    .join(year)
                    .join(edition)
                    .join("VC")
                    .join("Tools")
                    .join("MSVC"),
            );
        }
    }
    for tools_root in editions {
        let Ok(entries) = fs::read_dir(&tools_root) else {
            continue;
        };
        for entry in entries.flatten() {
            let candidate = entry
                .path()
                .join("bin")
                .join("Hostx64")
                .join("x64")
                .join("rc.exe");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}
