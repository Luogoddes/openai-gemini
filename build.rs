//! 构建期读取 `updater.conf`，把发布配置编译进二进制。
//!
//! 这样发布包只需要一个 exe：不用再带 ini/json，也不会出现"用户改了配置导致更新逻辑不一致"。
//! 本地调试仍然可以用 `--config xxx.json` 覆盖。

use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let conf_path = env::var("UPDATER_CONF")
        .map(PathBuf::from)
        .unwrap_or_else(|_| manifest_dir.join("updater.conf"));

    println!("cargo:rerun-if-changed={}", conf_path.display());
    println!("cargo:rerun-if-env-changed=UPDATER_CONF");

    let text = fs::read_to_string(&conf_path)
        .unwrap_or_else(|error| panic!("读取 {} 失败: {error}", conf_path.display()));

    let mut values: HashMap<String, String> = HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with('[') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            values.insert(key.trim().to_string(), value.trim().to_string());
        }
    }

    let get = |key: &str, fallback: &str| -> String {
        values
            .get(key)
            .map(|value| value.to_string())
            .unwrap_or_else(|| fallback.to_string())
    };

    let generate = format!(
        "#[allow(dead_code)]\npub const REPO: &str = {repo:?};\n\
         #[allow(dead_code)]\npub const FEED_NAME: &str = {feed:?};\n\
         #[allow(dead_code)]\npub const EXE_NAME: &str = {exe:?};\n\
         #[allow(dead_code)]\npub const HOMEPAGE: &str = {homepage:?};\n\
         #[allow(dead_code)]\npub const VARIANT: &str = {variant:?};\n\
         #[allow(dead_code)]\npub const PLATFORM: &str = {platform:?};\n\
         #[allow(dead_code)]\npub const PRODUCT: &str = {product:?};\n\
         #[allow(dead_code)]\npub const INTERVAL_HOURS: u64 = {interval};\n\
         #[allow(dead_code)]\npub const REQUIRE_SIGNATURE: bool = {require_signature};\n         #[allow(dead_code)]\npub const STARTUP_CHECK_DELAY_SECONDS: u64 = {startup_delay};\n",
        repo = get("repo", "owner/app"),
        feed = get("feed_name", "latest-version-windows-x64.json"),
        exe = get("exe_name", "app.exe"),
        homepage = get("homepage", ""),
        variant = get("variant", "portable"),
        platform = get("platform", "windows-x64"),
        product = get("product", "app"),
        interval = get("interval_hours", "24").parse::<u64>().unwrap_or(24),
        require_signature = get("require_signature", "false").trim() == "true",
        startup_delay = get("startup_check_delay_seconds", "30").parse::<u64>().unwrap_or(30),
    );

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    fs::write(out_dir.join("embedded_config.rs"), generate).unwrap();
}

