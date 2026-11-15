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
    /// 这个版本是否已被用户跳过（仅 Available 有意义）
    skipped: bool,
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
    emit_full(state, version, notes, received, total, error, automatic, false);
}

/// 带「已跳过」标记的状态帧。
pub fn emit_with_skip(
    state: State,
    version: &str,
    notes: &str,
    received: u64,
    total: u64,
    error: Option<&str>,
    automatic: bool,
    skipped: bool,
) {
    emit_full(state, version, notes, received, total, error, automatic, skipped);
}

fn emit_full(
    state: State,
    version: &str,
    notes: &str,
    received: u64,
    total: u64,
    error: Option<&str>,
    automatic: bool,
    skipped: bool,
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
        skipped,
    };
    // 帧序列化失败不应中断更新；降级为纯文本行。
    match serde_json::to_string(&ev) {
        Ok(line) => write_line(&line),
        Err(_) => write_line(&format!("{} {version}", state.as_str())),
    }
}

/// 写一行并 flush。
///
/// 调用方（GUI / 主程序）可能提前关闭管道，这时 `println!` 会 panic，
/// 而 release 是 `panic = "abort"`，会把整个更新流程打断；这里忽略写入错误。
fn write_line(line: &str) {
    let mut stdout = io::stdout().lock();
    let _ = stdout.write_all(line.as_bytes());
    let _ = stdout.write_all(b"\n");
    let _ = stdout.flush();
}
