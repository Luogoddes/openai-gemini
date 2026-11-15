use crate::error::AppError;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 跨进程保存的更新状态：上次检查时间 + 已下载待安装的版本。
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct UpdateState {
    #[serde(default)]
    pub last_check_unix: Option<u64>,
    #[serde(default)]
    pub pending_version: Option<String>,
    #[serde(default)]
    pub pending_notes: Option<String>,
    #[serde(default)]
    pub pending_staging: Option<String>,
    /// 用户主动跳过的版本号
    #[serde(default)]
    pub skipped_version: Option<String>,
    /// 上次检查到的远端版本（用于 24h 内免联网也能提示"有更新"）
    #[serde(default)]
    pub last_remote_version: Option<String>,
    #[serde(default)]
    pub last_remote_notes: String,
    #[serde(default)]
    pub last_remote_size: u64,
}

impl UpdateState {
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> Result<(), AppError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                AppError::Config(format!(
                    "创建状态目录 {} 失败: {e}",
                    parent.display()
                ))
            })?;
        }
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| AppError::Other(format!("状态序列化失败: {e}")))?;
        // 先写临时文件再改名：中途断电不会留下半个 JSON，跳过记录/待安装版本不会凭空丢失。
        let temporary = path.with_extension("json.tmp");
        std::fs::write(&temporary, text).map_err(|e| {
            AppError::Config(format!("写状态文件 {} 失败: {e}", temporary.display()))
        })?;
        std::fs::rename(&temporary, path).map_err(|e| {
            AppError::Config(format!("替换状态文件 {} 失败: {e}", path.display()))
        })?;
        Ok(())
    }

    /// 自动检查是否到期：从未检查过视为到期；否则按间隔小时数判断。
    pub fn auto_check_due(&self, interval_hours: u64) -> bool {
        match self.last_check_unix {
            None => true,
            Some(last) => {
                let interval = interval_hours.saturating_mul(3600);
                now_unix().saturating_sub(last) >= interval
            }
        }
    }

    pub fn mark_checked(&mut self) {
        self.last_check_unix = Some(now_unix());
    }

    pub fn clear_pending(&mut self) {
        self.pending_version = None;
        self.pending_notes = None;
        self.pending_staging = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn never_checked_is_always_due() {
        let state = UpdateState::default();
        assert!(state.auto_check_due(24));
    }

    #[test]
    fn interval_gates_automatic_checks() {
        let mut state = UpdateState::default();
        state.mark_checked();
        assert!(!state.auto_check_due(24));

        // 间隔 0 小时等价于"每次都要检查"
        assert!(state.auto_check_due(0));

        // 上一次检查是 25 小时前
        state.last_check_unix = Some(now_unix().saturating_sub(25 * 3600));
        assert!(state.auto_check_due(24));
    }

    #[test]
    fn state_round_trips_through_disk() {
        let directory = std::env::temp_dir().join(format!("updater-state-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("state.json");

        let mut state = UpdateState::default();
        state.mark_checked();
        state.pending_version = Some("1.2.0".to_string());
        state.pending_staging = Some("staging".to_string());
        state.save(&path).unwrap();

        let loaded = UpdateState::load(&path);
        assert_eq!(loaded.pending_version.as_deref(), Some("1.2.0"));
        assert!(loaded.last_check_unix.is_some());

        let mut loaded = loaded;
        loaded.clear_pending();
        assert!(loaded.pending_version.is_none());
        let _ = std::fs::remove_dir_all(&directory);
    }
}
