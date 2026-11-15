//! 平台相关的小能力：等待父进程退出、判断提权、UAC 提权重启、目录可写性探测。
//!
//! 这里直接用 kernel32/advapi32/shell32 的 FFI，避免为了几个调用引入额外 crate。

use std::path::Path;
use std::time::{Duration, Instant};

#[cfg(windows)]
mod win {
    use std::ffi::c_void;

    pub type Handle = *mut c_void;

    #[link(name = "kernel32")]
    extern "system" {
        pub fn OpenProcess(access: u32, inherit: i32, pid: u32) -> Handle;
        pub fn WaitForSingleObject(handle: Handle, milliseconds: u32) -> u32;
        pub fn CloseHandle(handle: Handle) -> i32;
        pub fn GetCurrentProcess() -> Handle;
        pub fn OpenProcessToken(process: Handle, access: u32, token: *mut Handle) -> i32;
    }

    #[link(name = "advapi32")]
    extern "system" {
        pub fn GetTokenInformation(
            token: Handle,
            class: u32,
            info: *mut c_void,
            len: u32,
            returned: *mut u32,
        ) -> i32;
    }

    #[link(name = "shell32")]
    extern "system" {
        pub fn ShellExecuteW(
            hwnd: Handle,
            operation: *const u16,
            file: *const u16,
            parameters: *const u16,
            directory: *const u16,
            show: i32,
        ) -> Handle;
    }

    pub const SYNCHRONIZE: u32 = 0x0010_0000;
    pub const TOKEN_QUERY: u32 = 0x0008;
    pub const TOKEN_ELEVATION: u32 = 20;
    pub const SW_SHOWNORMAL: i32 = 1;
}

/// 阻塞等待 `pid` 退出，最多 `timeout`；拿不到句柄（进程已退出/无权限）时立刻返回。
#[cfg(windows)]
pub fn wait_for_process(pid: u32, timeout: Duration) {
    if pid == 0 {
        return;
    }
    unsafe {
        let handle = win::OpenProcess(win::SYNCHRONIZE, 0, pid);
        if handle.is_null() {
            return;
        }
        let milliseconds = timeout.as_millis().min(u32::MAX as u128) as u32;
        win::WaitForSingleObject(handle, milliseconds);
        win::CloseHandle(handle);
    }
}

#[cfg(not(windows))]
pub fn wait_for_process(_pid: u32, _timeout: Duration) {}

/// 当前进程是否已经提权。
#[cfg(windows)]
pub fn is_elevated() -> bool {
    unsafe {
        let mut token: win::Handle = std::ptr::null_mut();
        if win::OpenProcessToken(win::GetCurrentProcess(), win::TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let mut elevated: u32 = 0;
        let mut returned: u32 = 0;
        let ok = win::GetTokenInformation(
            token,
            win::TOKEN_ELEVATION,
            &mut elevated as *mut u32 as *mut std::ffi::c_void,
            std::mem::size_of::<u32>() as u32,
            &mut returned,
        );
        win::CloseHandle(token);
        ok != 0 && elevated != 0
    }
}

#[cfg(not(windows))]
pub fn is_elevated() -> bool {
    true
}

/// 用 UAC 提权重启自己（同一个 exe + 同样的参数）。
#[cfg(windows)]
pub fn relaunch_elevated(executable: &Path, arguments: &[String]) -> bool {
    use std::os::windows::ffi::OsStrExt;

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    let file: Vec<u16> = executable
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let operation = wide("runas");
    let parameters = wide(&arguments.join(" "));
    let directory = executable
        .parent()
        .map(|path| {
            path.as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect::<Vec<u16>>()
        })
        .unwrap_or_default();

    let result = unsafe {
        win::ShellExecuteW(
            std::ptr::null_mut(),
            operation.as_ptr(),
            file.as_ptr(),
            parameters.as_ptr(),
            if directory.is_empty() {
                std::ptr::null()
            } else {
                directory.as_ptr()
            },
            win::SW_SHOWNORMAL,
        )
    };
    result as usize > 32
}

#[cfg(not(windows))]
pub fn relaunch_elevated(_executable: &Path, _arguments: &[String]) -> bool {
    false
}

/// 目录是否可写：真的写一个临时文件，比看 ACL 更可靠。
pub fn directory_writable(directory: &Path) -> bool {
    let probe = directory.join(format!(".write-probe-{}", std::process::id()));
    match std::fs::write(&probe, b"probe") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

/// 简单退避重试：等待文件占用释放时用。
pub fn retry_until<T>(deadline: Duration, mut action: impl FnMut() -> Result<T, crate::error::AppError>)
    -> Result<T, crate::error::AppError>
{
    let started = Instant::now();
    loop {
        match action() {
            Ok(value) => return Ok(value),
            Err(error) if error.is_transient() && started.elapsed() < deadline => {
                std::thread::sleep(Duration::from_millis(400));
            }
            Err(error) => return Err(error),
        }
    }
}
