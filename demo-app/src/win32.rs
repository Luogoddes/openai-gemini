//! 手写的最小 Win32 绑定。
//!
//! 不引入 `windows` / `windows-sys` crate 是有意的：这个仓库要保持"离线可构建、
//! 依赖树不膨胀"。用到的 API 也就这几十个，声明一次即可。

#![allow(non_snake_case, non_camel_case_types, dead_code)]

use std::ffi::c_void;

pub type BOOL = i32;
pub type UINT = u32;
pub type DWORD = u32;
pub type WPARAM = usize;
pub type LPARAM = isize;
pub type LRESULT = isize;
pub type HWND = *mut c_void;
pub type HINSTANCE = *mut c_void;
pub type HICON = *mut c_void;
pub type HCURSOR = *mut c_void;
pub type HBRUSH = *mut c_void;
pub type HFONT = *mut c_void;
pub type HMENU = *mut c_void;
pub type HDC = *mut c_void;
pub type HGDIOBJ = *mut c_void;
pub type HMODULE = *mut c_void;
pub type LPCWSTR = *const u16;
pub type WNDPROC = unsafe extern "system" fn(HWND, UINT, WPARAM, LPARAM) -> LRESULT;

pub const WM_CREATE: UINT = 0x0001;
pub const WM_DESTROY: UINT = 0x0002;
pub const WM_CLOSE: UINT = 0x0010;
pub const WM_SETTEXT: UINT = 0x000C;
pub const WM_GETTEXT: UINT = 0x000D;
pub const WM_GETTEXTLENGTH: UINT = 0x000E;
pub const WM_COMMAND: UINT = 0x0111;
pub const WM_TIMER: UINT = 0x0113;
pub const WM_CTLCOLOREDIT: UINT = 0x0133;
pub const WM_CTLCOLORSTATIC: UINT = 0x0138;
pub const WM_SETFONT: UINT = 0x0030;
pub const WM_APP: UINT = 0x8000;
pub const WM_HOTKEY: UINT = 0x0312;
pub const WM_DROPFILES: UINT = 0x0233;
pub const EM_SETSEL: UINT = 0x00B1;

pub const LB_ADDSTRING: UINT = 0x0180;
pub const LB_RESETCONTENT: UINT = 0x0184;
pub const LB_SETCURSEL: UINT = 0x0186;
pub const LB_GETCURSEL: UINT = 0x0188;
pub const LB_GETCOUNT: UINT = 0x018B;
pub const LB_SETTOPINDEX: UINT = 0x0197;
pub const LBN_SELCHANGE: u16 = 1;
pub const LBN_DBLCLK: u16 = 2;

pub const MOD_ALT: UINT = 0x0001;
pub const MOD_CONTROL: UINT = 0x0002;
pub const MOD_SHIFT: UINT = 0x0004;
pub const MOD_WIN: UINT = 0x0008;
pub const VK_F: UINT = 0x46;

pub const CF_UNICODETEXT: UINT = 13;
pub const GMEM_MOVEABLE: UINT = 0x0002;

pub const WS_CHILD: DWORD = 0x4000_0000;
pub const WS_VISIBLE: DWORD = 0x1000_0000;
pub const WS_CAPTION: DWORD = 0x00C0_0000;
pub const WS_SYSMENU: DWORD = 0x0008_0000;
pub const WS_MINIMIZEBOX: DWORD = 0x0002_0000;
pub const WS_CLIPCHILDREN: DWORD = 0x0200_0000;
pub const WS_VSCROLL: DWORD = 0x0020_0000;
pub const WS_TABSTOP: DWORD = 0x0001_0000;
pub const WS_EX_APPWINDOW: DWORD = 0x0004_0000;
pub const WS_EX_CLIENTEDGE: DWORD = 0x0000_0200;

pub const BS_PUSHBUTTON: DWORD = 0x0000_0000;
pub const BS_DEFPUSHBUTTON: DWORD = 0x0000_0001;
pub const BS_GROUPBOX: DWORD = 0x0000_0007;
pub const SS_LEFT: DWORD = 0x0000_0000;
pub const SS_RIGHT: DWORD = 0x0000_0002;
pub const SS_NOPREFIX: DWORD = 0x0000_0080;
pub const ES_MULTILINE: DWORD = 0x0000_0004;
pub const ES_AUTOHSCROLL: DWORD = 0x0000_0080;
pub const ES_AUTOVSCROLL: DWORD = 0x0000_0040;
pub const ES_WANTRETURN: DWORD = 0x0000_1000;
pub const ES_READONLY: DWORD = 0x0000_0800;
pub const LBS_NOTIFY: DWORD = 0x0000_0001;
pub const PBS_SMOOTH: DWORD = 0x0000_0001;
pub const PBM_SETRANGE: UINT = 0x0401;
pub const PBM_SETPOS: UINT = 0x0402;

pub const CW_USEDEFAULT: i32 = 0x8000_0000u32 as i32;
pub const SW_HIDE: i32 = 0;
pub const SW_SHOW: i32 = 5;
pub const SW_SHOWNORMAL: i32 = 1;
pub const SW_RESTORE: i32 = 9;

pub const MF_STRING: UINT = 0x0000_0000;
pub const MF_POPUP: UINT = 0x0000_0010;
pub const MF_SEPARATOR: UINT = 0x0000_0800;

pub const MB_OK: UINT = 0x0000_0000;
pub const MB_YESNO: UINT = 0x0000_0004;
pub const MB_ICONQUESTION: UINT = 0x0000_0020;
pub const MB_ICONINFORMATION: UINT = 0x0000_0040;
pub const MB_ICONWARNING: UINT = 0x0000_0030;
pub const IDYES: i32 = 6;

pub const COLOR_WINDOW: i32 = 5;
pub const COLOR_WINDOWTEXT: i32 = 8;
pub const COLOR_BTNFACE: i32 = 15;

pub const GWLP_USERDATA: i32 = -21;
pub const ERROR_ALREADY_EXISTS: u32 = 183;
pub const ICC_PROGRESS_CLASS: DWORD = 0x0000_0020;
pub const PROGRESS_CLASS: &str = "msctls_progress32";
pub const EN_CHANGE: u16 = 0x0300;

#[repr(C)]
pub struct POINT {
    pub x: i32,
    pub y: i32,
}

#[repr(C)]
pub struct RECT {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

#[repr(C)]
pub struct MSG {
    pub hwnd: HWND,
    pub message: UINT,
    pub w_param: WPARAM,
    pub l_param: LPARAM,
    pub time: DWORD,
    pub pt: POINT,
    pub l_private: DWORD,
}

#[repr(C)]
pub struct WNDCLASSEXW {
    pub cb_size: UINT,
    pub style: UINT,
    pub lpfn_wnd_proc: Option<WNDPROC>,
    pub cb_cls_extra: i32,
    pub cb_wnd_extra: i32,
    pub h_instance: HINSTANCE,
    pub h_icon: HICON,
    pub h_cursor: HCURSOR,
    pub hbr_background: HBRUSH,
    pub lpsz_menu_name: LPCWSTR,
    pub lpsz_class_name: LPCWSTR,
    pub h_icon_sm: HICON,
}

#[repr(C)]
pub struct CREATESTRUCTW {
    pub lp_create_params: *mut c_void,
    pub h_instance: HINSTANCE,
    pub h_menu: HMENU,
    pub hwnd_parent: HWND,
    pub cy: i32,
    pub cx: i32,
    pub y: i32,
    pub x: i32,
    pub style: i32,
    pub lpsz_name: LPCWSTR,
    pub lpsz_class: LPCWSTR,
    pub dw_ex_style: DWORD,
}

#[repr(C)]
pub struct INITCOMMONCONTROLSEX {
    pub dw_size: DWORD,
    pub dw_icc: DWORD,
}

#[repr(C)]
pub struct SYSTEMTIME {
    pub year: u16,
    pub month: u16,
    pub day_of_week: u16,
    pub day: u16,
    pub hour: u16,
    pub minute: u16,
    pub second: u16,
    pub milliseconds: u16,
}

impl SYSTEMTIME {
    pub fn now() -> Self {
        let mut value = SYSTEMTIME {
            year: 0,
            month: 0,
            day_of_week: 0,
            day: 0,
            hour: 0,
            minute: 0,
            second: 0,
            milliseconds: 0,
        };
        unsafe { GetLocalTime(&mut value) };
        value
    }

    pub fn timestamp(&self) -> String {
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
            self.year, self.month, self.day, self.hour, self.minute, self.second
        )
    }

    /// `HH:MM:SS`，用于状态栏这类只需要时刻的地方。
    pub fn clock(&self) -> String {
        format!("{:02}:{:02}:{:02}", self.hour, self.minute, self.second)
    }

    /// `YYYY-MM-DD`，用于按日期建用例目录。
    pub fn date(&self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

/// UTF-8 → 以 NUL 结尾的 UTF-16。
pub fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn from_wide(buffer: &[u16]) -> String {
    let end = buffer.iter().position(|unit| *unit == 0).unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..end])
}

#[link(name = "kernel32")]
extern "system" {
    pub fn CreateMutexW(attributes: *mut c_void, owner: BOOL, name: LPCWSTR) -> *mut c_void;
    pub fn CloseHandle(handle: *mut c_void) -> BOOL;
    pub fn GetLastError() -> DWORD;
    pub fn GetModuleHandleW(name: LPCWSTR) -> HMODULE;
    pub fn GetLocalTime(value: *mut SYSTEMTIME);
    pub fn MulDiv(number: i32, numerator: i32, denominator: i32) -> i32;
    pub fn GlobalAlloc(flags: UINT, bytes: usize) -> *mut c_void;
    pub fn GlobalLock(handle: *mut c_void) -> *mut c_void;
    pub fn GlobalUnlock(handle: *mut c_void) -> BOOL;
    pub fn GlobalFree(handle: *mut c_void) -> *mut c_void;
}

#[link(name = "user32")]
extern "system" {
    pub fn RegisterClassExW(class: *const WNDCLASSEXW) -> u16;
    pub fn CreateWindowExW(
        ex_style: DWORD,
        class_name: LPCWSTR,
        window_name: LPCWSTR,
        style: DWORD,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        parent: HWND,
        menu: HMENU,
        instance: HINSTANCE,
        param: *mut c_void,
    ) -> HWND;
    pub fn DefWindowProcW(hwnd: HWND, message: UINT, w_param: WPARAM, l_param: LPARAM) -> LRESULT;
    pub fn DestroyWindow(hwnd: HWND) -> BOOL;
    pub fn ShowWindow(hwnd: HWND, command: i32) -> BOOL;
    pub fn UpdateWindow(hwnd: HWND) -> BOOL;
    pub fn GetMessageW(message: *mut MSG, hwnd: HWND, min: UINT, max: UINT) -> BOOL;
    pub fn TranslateMessage(message: *const MSG) -> BOOL;
    pub fn DispatchMessageW(message: *const MSG) -> LRESULT;
    pub fn PostQuitMessage(code: i32);
    pub fn PostMessageW(hwnd: HWND, message: UINT, w_param: WPARAM, l_param: LPARAM) -> BOOL;
    pub fn SendMessageW(hwnd: HWND, message: UINT, w_param: WPARAM, l_param: LPARAM) -> LRESULT;
    pub fn SetWindowTextW(hwnd: HWND, text: LPCWSTR) -> BOOL;
    pub fn GetWindowTextW(hwnd: HWND, buffer: *mut u16, count: i32) -> i32;
    pub fn GetWindowTextLengthW(hwnd: HWND) -> i32;
    pub fn EnableWindow(hwnd: HWND, enable: BOOL) -> BOOL;
    pub fn SetTimer(hwnd: HWND, id: usize, millis: UINT, callback: *mut c_void) -> usize;
    pub fn KillTimer(hwnd: HWND, id: usize) -> BOOL;
    pub fn MessageBoxW(hwnd: HWND, text: LPCWSTR, caption: LPCWSTR, flags: UINT) -> i32;
    pub fn GetClientRect(hwnd: HWND, rect: *mut RECT) -> BOOL;
    pub fn MoveWindow(hwnd: HWND, x: i32, y: i32, width: i32, height: i32, repaint: BOOL) -> BOOL;
    pub fn SetWindowLongPtrW(hwnd: HWND, index: i32, value: isize) -> isize;
    pub fn GetWindowLongPtrW(hwnd: HWND, index: i32) -> isize;
    pub fn LoadIconW(instance: HINSTANCE, name: LPCWSTR) -> HICON;
    pub fn LoadCursorW(instance: HINSTANCE, name: LPCWSTR) -> HCURSOR;
    pub fn GetDC(hwnd: HWND) -> HDC;
    pub fn ReleaseDC(hwnd: HWND, dc: HDC) -> i32;
    pub fn GetSysColorBrush(index: i32) -> HBRUSH;
    pub fn GetDpiForWindow(hwnd: HWND) -> UINT;
    pub fn AdjustWindowRectExForDpi(
        rect: *mut RECT,
        style: DWORD,
        menu: BOOL,
        ex_style: DWORD,
        dpi: UINT,
    ) -> BOOL;
    pub fn IsWindow(hwnd: HWND) -> BOOL;
    pub fn CreatePopupMenu() -> HMENU;
    pub fn CreateMenu() -> HMENU;
    pub fn AppendMenuW(menu: HMENU, flags: UINT, item: usize, text: LPCWSTR) -> BOOL;
    pub fn SetMenu(hwnd: HWND, menu: HMENU) -> BOOL;
    pub fn RegisterHotKey(hwnd: HWND, id: i32, modifiers: UINT, key: UINT) -> BOOL;
    pub fn UnregisterHotKey(hwnd: HWND, id: i32) -> BOOL;
    pub fn SetForegroundWindow(hwnd: HWND) -> BOOL;
    pub fn SetFocus(hwnd: HWND) -> HWND;
    pub fn FindWindowW(class_name: LPCWSTR, window_name: LPCWSTR) -> HWND;
    pub fn GetSysColor(index: i32) -> DWORD;
    pub fn OpenClipboard(hwnd: HWND) -> BOOL;
    pub fn EmptyClipboard() -> BOOL;
    pub fn CloseClipboard() -> BOOL;
    pub fn GetClipboardData(format: UINT) -> *mut c_void;
    pub fn SetClipboardData(format: UINT, data: *mut c_void) -> *mut c_void;
    pub fn IsClipboardFormatAvailable(format: UINT) -> BOOL;
}

#[link(name = "gdi32")]
extern "system" {
    pub fn CreateFontW(
        height: i32,
        width: i32,
        escapement: i32,
        orientation: i32,
        weight: i32,
        italic: BOOL,
        underline: BOOL,
        strike_out: BOOL,
        charset: DWORD,
        out_precision: DWORD,
        clip_precision: DWORD,
        quality: DWORD,
        pitch_and_family: DWORD,
        face: LPCWSTR,
    ) -> HFONT;
    pub fn DeleteObject(object: HGDIOBJ) -> BOOL;
    pub fn GetDeviceCaps(dc: HDC, index: i32) -> i32;
    pub fn SetBkMode(dc: HDC, mode: i32) -> i32;
    pub fn SetBkColor(dc: HDC, color: DWORD) -> DWORD;
    pub fn SetTextColor(dc: HDC, color: DWORD) -> DWORD;
}

#[link(name = "comctl32")]
extern "system" {
    pub fn InitCommonControlsEx(controls: *const INITCOMMONCONTROLSEX) -> BOOL;
}

#[link(name = "shell32")]
extern "system" {
    pub fn ShellExecuteW(
        hwnd: HWND,
        operation: LPCWSTR,
        file: LPCWSTR,
        parameters: LPCWSTR,
        directory: LPCWSTR,
        show: i32,
    ) -> HINSTANCE;
    pub fn DragAcceptFiles(hwnd: HWND, accept: BOOL);
    pub fn DragQueryFileW(drop: *mut c_void, index: UINT, buffer: *mut u16, count: UINT) -> UINT;
    pub fn DragFinish(drop: *mut c_void);
}

pub const LOGPIXELSY: i32 = 90;
pub const FW_NORMAL: i32 = 400;
pub const FW_BOLD: i32 = 700;
pub const DEFAULT_CHARSET: DWORD = 1;
pub const OUT_DEFAULT_PRECIS: DWORD = 0;
pub const CLIP_DEFAULT_PRECIS: DWORD = 0;
pub const CLEARTYPE_QUALITY: DWORD = 5;
pub const DEFAULT_PITCH: DWORD = 0;
pub const FF_DONTCARE: DWORD = 0;
pub const TRANSPARENT: i32 = 1;

/// 屏幕 DPI 折算到 96 基准。
pub fn dpi_scale(value: i32, dpi: i32) -> i32 {
    unsafe { MulDiv(value, dpi, 96) }
}
