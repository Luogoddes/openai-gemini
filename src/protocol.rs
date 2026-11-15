use serde::Serialize;
use std::io::{self, Write};

pub const PROTOCOL: u32 = 2;

/// updater → 主程序的状态机阶段，与 snow-shot 的 UpdateState 对齐。
#[derive(Debug, Clone, Copy)]
pub enum State {
    /// 本平台/本分发方式不支持更新；留给主程序表达"不可用"。
    #[allow(dead_code)]
    Unavailable,
    Idle,
    Checking,
    Available,
    Downloading,
    Verifying,
    Ready,
    Applying,
    Failed,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            State::Unavailable => "Unavailable",
            State::Idle => "Idle",
            State::Checking => "Checking",
            State::Available => "Available",
            State::Downloading => "Downloading",
            State::Verifying => "Verifying",
            State::Ready => "Ready",
            State::Applying => "Applying",
            State::Failed => "Failed",
        }
    }
}

/// updater → 主程序的状态帧（stdout 一行一条 JSON）。
#[derive(Serialize)]
struct StatusEvent<'a> {
    protocol: u32,
    #[serde(rename = "type")]
    event_type: &'static str,
    state: &'a str,
    version: &'a str,
    notes: &'a str,
    received: u64,
    total: u64,
    error: Option<&'a str>,
    automatic: bool,
}

/// 向主程序推一条状态帧并立刻 flush，保证界面实时收到。
pub fn emit(
    state: State,
    version: &str,
    notes: &str,
    received: u64,
    total: u64,
    error: Option<&str>,
    automatic: bool,
) {
    let ev = StatusEvent {
        protocol: PROTOCOL,
        event_type: "status",
        state: state.as_str(),
        version,
        notes,
        received,
        total,
        error,
        automatic,
    };
    // 帧序列化失败不应中断更新；降级为纯文本行。
    match serde_json::to_string(&ev) {
        Ok(line) => println!("{line}"),
        Err(_) => println!("{} {version}", state.as_str()),
    }
    let _ = io::stdout().flush();
}
