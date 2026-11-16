//! Generic Updater Demo（Rust 版）—— 面向测试人员的「文件夹速取」小工具，
//! 同时验证外置更新器链路（检查 / 下载 / 替换 / 回滚）。
//!
//! 定位：不是占位页面，而是一个可以天天用的便携工具：
//!   * 多分组收藏文件夹，支持别名、备注、置顶
//!   * 输入即过滤；双击 / 回车打开；Ctrl+Alt+F 唤出
//!   * 最近打开历史；路径失效显性提示
//!   * 粘贴路径或拖入文件夹即可添加；识别剪贴板路径一键收藏
//!   * 一键按「日期_用例名」建子目录并打开
//!   * 收藏清单可导出 / 导入 JSON，便于团队共用目录表
//!
//! 更新器仍是独立进程：本程序只拉起同目录的 `generic-updater.exe`，读它 stdout
//! 上的一行一 JSON 状态帧，把状态映射到按钮与进度条；下载、校验、备份、替换、
//! 回滚、重启全部由更新器完成。契约与 C++ 示例一致。

#![cfg_attr(windows, windows_subsystem = "windows")]
#![allow(non_snake_case)]

mod win32;

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use win32::*;

const APP_NAME: &str = "Generic Updater Demo";
const APP_SUBTITLE: &str = "文件夹速取";
const HOMEPAGE: &str = "https://github.com/Luogoddes/openai-gemini";
const CLASS_NAME: &str = "GenericUpdaterDemo.MainWindow";
const MUTEX_NAME: &str = "Local\\GenericUpdaterDemo.SingleInstance";
const UPDATER_CONF: &str = include_str!("../../updater.conf");

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
const WM_APP_JOB: UINT = WM_APP + 1;
const WM_APP_ACTIVATE: UINT = WM_APP + 2;
const TIMER_STARTUP_CHECK: usize = 1;
const HOTKEY_ID: i32 = 1;

const CLIENT_WIDTH: i32 = 900;
const CLIENT_HEIGHT: i32 = 642;

const IDC_TITLE: i32 = 1001;
const IDC_VERSION: i32 = 1002;
const IDC_INPUT: i32 = 1003;
const IDC_ADD: i32 = 1004;
const IDC_PASTE: i32 = 1005;
const IDC_CASE: i32 = 1006;
const IDC_GROUPS: i32 = 1007;
const IDC_LIST: i32 = 1008;
const IDC_DETAIL: i32 = 1009;
const IDC_OPEN: i32 = 1010;
const IDC_COPY: i32 = 1011;
const IDC_TERMINAL: i32 = 1012;
const IDC_ALIAS: i32 = 1013;
const IDC_NOTE: i32 = 1014;
const IDC_PIN: i32 = 1015;
const IDC_DELETE: i32 = 1016;
const IDC_IMPORT: i32 = 1017;
const IDC_EXPORT: i32 = 1018;
const IDC_UPDATE_LINE: i32 = 1101;
const IDC_UPDATE_NOTES: i32 = 1102;
const IDC_STATUS: i32 = 1103;
const IDC_PROGRESS: i32 = 1104;
const IDC_SKIP: i32 = 1105;
const IDC_INSTALL: i32 = 1106;
const IDC_RESTART: i32 = 1107;
const IDC_CHECK: i32 = 1108;
const IDC_SAVE_STATE: i32 = 1109;

const IDM_IMPORT: i32 = 2001;
const IDM_EXPORT: i32 = 2002;
const IDM_DATA: i32 = 2003;
const IDM_LOG: i32 = 2004;
const IDM_EXIT: i32 = 2005;
const IDM_ADD: i32 = 2011;
const IDM_PASTE: i32 = 2012;
const IDM_CASE: i32 = 2013;
const IDM_PIN: i32 = 2014;
const IDM_DELETE: i32 = 2015;
const IDM_REFRESH: i32 = 2016;
const IDM_CHECK: i32 = 2021;
const IDM_ABOUT: i32 = 2022;

// ---------------------------------------------------------------- 清单模型

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
struct Folder {
    path: String,
    alias: String,
    note: String,
    pinned: bool,
    last_opened: u64,
}

impl Folder {
    fn new(path: &str) -> Self {
        Folder {
            path: path.to_string(),
            ..Folder::default()
        }
    }

    fn display_name(&self) -> String {
        if !self.alias.trim().is_empty() {
            return self.alias.clone();
        }
        Path::new(&self.path)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.path.clone())
    }

    fn exists(&self) -> bool {
        Path::new(&self.path).is_dir()
    }
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
struct Group {
    name: String,
    folders: Vec<Folder>,
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
struct RecentEntry {
    path: String,
    opened_at: u64,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
struct Store {
    schema: u32,
    groups: Vec<Group>,
    recent: Vec<RecentEntry>,
}

impl Default for Store {
    fn default() -> Self {
        Store {
            schema: 1,
            groups: Vec::new(),
            recent: Vec::new(),
        }
    }
}

impl Store {
    fn load(path: &Path) -> Option<Store> {
        let text = fs::read_to_string(path).ok()?;
        serde_json::from_str(&text).ok()
    }

    fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, serde_json::to_string_pretty(self).unwrap_or_default())
    }

    fn find(&self, path: &str) -> Option<&Folder> {
        self.groups
            .iter()
            .flat_map(|group| group.folders.iter())
            .find(|folder| same_path(&folder.path, path))
    }

    fn find_mut(&mut self, path: &str) -> Option<&mut Folder> {
        self.groups
            .iter_mut()
            .flat_map(|group| group.folders.iter_mut())
            .find(|folder| same_path(&folder.path, path))
    }

    fn contains(&self, path: &str) -> bool {
        self.find(path).is_some()
    }

    /// 加入指定分组；已存在则返回 false（不重复收藏）。
    fn add(&mut self, folder: Folder, group_name: &str) -> bool {
        if self.contains(&folder.path) {
            return false;
        }
        let index = match self.groups.iter().position(|group| group.name == group_name) {
            Some(index) => index,
            None => {
                self.groups.push(Group {
                    name: group_name.to_string(),
                    folders: Vec::new(),
                });
                self.groups.len() - 1
            }
        };
        self.groups[index].folders.push(folder);
        true
    }

    fn remove(&mut self, path: &str) -> bool {
        let mut removed = false;
        for group in &mut self.groups {
            let before = group.folders.len();
            group.folders.retain(|folder| !same_path(&folder.path, path));
            removed |= group.folders.len() != before;
        }
        self.recent.retain(|entry| !same_path(&entry.path, path));
        removed
    }

    fn touch_recent(&mut self, path: &str) {
        self.recent.retain(|entry| !same_path(&entry.path, path));
        self.recent.insert(
            0,
            RecentEntry {
                path: path.to_string(),
                opened_at: now_unix(),
            },
        );
        self.recent.truncate(20);
        if let Some(folder) = self.find_mut(path) {
            folder.last_opened = now_unix();
        }
    }

    /// 合并导入的清单：同路径视为同一条收藏，只补充缺失项与分组。
    fn merge(&mut self, other: Store) -> usize {
        let mut added = 0;
        for group in other.groups {
            for folder in group.folders {
                let target = if group.name.trim().is_empty() {
                    "未分组"
                } else {
                    &group.name
                };
                if self.add(folder, target) {
                    added += 1;
                }
            }
        }
        added
    }

    fn pinned_count(&self) -> usize {
        self.groups
            .iter()
            .flat_map(|group| group.folders.iter())
            .filter(|folder| folder.pinned)
            .count()
    }
}

fn same_path(left: &str, right: &str) -> bool {
    let trim = |value: &str| value.trim().trim_end_matches(['\\', '/']).to_string();
    trim(left).eq_ignore_ascii_case(&trim(right))
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[derive(Serialize, Deserialize)]
#[serde(default)]
struct Settings {
    schema: u32,
    check_updates_on_start: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            schema: 1,
            check_updates_on_start: true,
        }
    }
}

// ---------------------------------------------------------------- 更新作业

#[derive(Clone)]
enum Job {
    CheckAuto,
    CheckManual,
    Download,
    SkipVersion(String),
    UnskipVersion,
}

impl Job {
    fn args(&self) -> Vec<String> {
        match self {
            Job::CheckAuto => vec!["--check".into(), "--auto".into()],
            Job::CheckManual => vec!["--check".into()],
            Job::Download => vec!["--download".into()],
            Job::SkipVersion(version) => vec!["--skip".into(), version.clone()],
            Job::UnskipVersion => vec!["--unskip".into()],
        }
    }

    fn status_text(&self) -> &'static str {
        match self {
            Job::Download => "正在下载更新…",
            Job::SkipVersion(_) => "正在记录跳过的版本…",
            Job::UnskipVersion => "正在恢复提示…",
            _ => "正在检查更新…",
        }
    }
}

struct Terminal {
    state: String,
    version: String,
    notes: String,
    error: Option<String>,
    skipped: bool,
    total: u64,
}

enum UiMsg {
    Progress { percent: u32, received: u64 },
    Done { job: Job, terminal: Option<Terminal> },
    StartFailed,
}

struct Frame {
    state: String,
    version: String,
    notes: String,
    error: Option<String>,
    received: u64,
    total: u64,
    skipped: bool,
}

// ---------------------------------------------------------------- 视图过滤

#[derive(Clone, Copy, PartialEq)]
enum Filter {
    All,
    Pinned,
    Recent,
    Group(usize),
}

// ---------------------------------------------------------------- 应用状态

struct App {
    hwnd: HWND,
    exe_dir: PathBuf,
    data_dir: PathBuf,
    store_path: PathBuf,
    log_path: PathBuf,
    updater_path: PathBuf,
    local_version: String,
    dpi: i32,

    font: HFONT,
    title_font: HFONT,

    title: HWND,
    version_label: HWND,
    input: HWND,
    groups: HWND,
    list: HWND,
    detail: HWND,
    save_state: HWND,
    update_line: HWND,
    update_notes: HWND,
    status: HWND,
    progress: HWND,
    check: HWND,
    skip: HWND,
    install: HWND,
    restart: HWND,

    store: Store,
    settings: Settings,
    view_paths: Vec<String>,

    busy: bool,
    skipped: bool,
    available_version: String,
    pending_version: String,
}

impl Drop for App {
    fn drop(&mut self) {
        unsafe {
            if !self.font.is_null() {
                DeleteObject(self.font);
            }
            if !self.title_font.is_null() {
                DeleteObject(self.title_font);
            }
        }
    }
}

impl App {
    fn new(exe_dir: PathBuf, dpi: i32) -> Self {
        let data_dir = exe_dir.join("data");
        let store_path = data_dir.join("folders.json");
        let settings_path = data_dir.join("settings.json");
        let log_path = exe_dir.join("logs").join("app.log");
        let version_path = exe_dir.join("version.txt");
        let updater_path = exe_dir.join("generic-updater.exe");

        let local_version = fs::read_to_string(&version_path)
            .ok()
            .map(|text| text.trim().to_string())
            .filter(|text| !text.is_empty())
            .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string());

        let mut app = App {
            hwnd: std::ptr::null_mut(),
            exe_dir: exe_dir.clone(),
            data_dir: data_dir.clone(),
            store_path: store_path.clone(),
            log_path,
            updater_path,
            local_version,
            dpi: if dpi > 0 { dpi } else { 96 },
            font: std::ptr::null_mut(),
            title_font: std::ptr::null_mut(),
            title: std::ptr::null_mut(),
            version_label: std::ptr::null_mut(),
            input: std::ptr::null_mut(),
            groups: std::ptr::null_mut(),
            list: std::ptr::null_mut(),
            detail: std::ptr::null_mut(),
            save_state: std::ptr::null_mut(),
            update_line: std::ptr::null_mut(),
            update_notes: std::ptr::null_mut(),
            status: std::ptr::null_mut(),
            progress: std::ptr::null_mut(),
            check: std::ptr::null_mut(),
            skip: std::ptr::null_mut(),
            install: std::ptr::null_mut(),
            restart: std::ptr::null_mut(),
            store: Store::default(),
            settings: Settings::default(),
            view_paths: Vec::new(),
            busy: false,
            skipped: false,
            available_version: String::new(),
            pending_version: String::new(),
        };

        app.cleanup_old_images();
        let _ = fs::create_dir_all(&data_dir);

        app.settings = fs::read_to_string(&settings_path)
            .ok()
            .and_then(|text| serde_json::from_str::<Settings>(&text).ok())
            .unwrap_or_default();
        if !settings_path.exists() {
            let _ = fs::write(
                &settings_path,
                serde_json::to_string_pretty(&app.settings).unwrap_or_default(),
            );
        }

        app.store = match Store::load(&store_path) {
            Some(store) => store,
            None => {
                let store = seed_store();
                let _ = store.save(&store_path);
                store
            }
        };
        app.log(&format!(
            "启动 {} v{}（清单 {}，数据目录 {}）",
            APP_NAME,
            app.local_version,
            app.store_path.display(),
            data_dir.display()
        ));
        app
    }

    // ------------------------------------------------------------ 数据

    fn save_store(&mut self) {
        if let Err(error) = self.store.save(&self.store_path) {
            self.set_status(&format!("保存清单失败：{error}"));
        }
    }

    fn log(&self, message: &str) {
        let line = format!("[{}] {}\n", SYSTEMTIME::now().timestamp(), message);
        if let Some(parent) = self.log_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_path)
            .and_then(|mut file| file.write_all(line.as_bytes()));
    }

    /// 更新完成后旧镜像可能被更新器短暂占用，后台尝试删除，别在安装目录里留垃圾。
    fn cleanup_old_images(&self) {
        let Ok(entries) = fs::read_dir(&self.exe_dir) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.contains(".old-") {
                let _ = fs::remove_file(entry.path());
            }
        }
    }

    fn message(&self, text: &str) {
        unsafe {
            MessageBoxW(
                self.hwnd,
                wide(text).as_ptr(),
                wide(APP_NAME).as_ptr(),
                MB_OK | MB_ICONINFORMATION,
            );
        }
    }

    fn warn(&self, text: &str) {
        unsafe {
            MessageBoxW(
                self.hwnd,
                wide(text).as_ptr(),
                wide(APP_NAME).as_ptr(),
                MB_OK | MB_ICONWARNING,
            );
        }
    }

    // ------------------------------------------------------------ UI 构建

    fn build_ui(&mut self) {
        let dpi = unsafe { GetDpiForWindow(self.hwnd) } as i32;
        if dpi > 0 {
            self.dpi = dpi;
        }
        let px = |value: i32| dpi_scale(value, self.dpi);
        self.font = create_font(px(10), FW_NORMAL);
        self.title_font = create_font(px(15), FW_BOLD);

        let parent = self.hwnd;
        let font = self.font;
        let title_text = format!("{} · {} v{}", APP_SUBTITLE, APP_NAME, self.local_version);
        self.title = create_child(
            parent, "STATIC", &title_text, SS_LEFT | SS_NOPREFIX, 0,
            (px(18), px(14), px(560), px(26)), IDC_TITLE, self.title_font,
        );
        self.version_label = create_child(
            parent, "STATIC", "", SS_RIGHT | SS_NOPREFIX, 0,
            (px(700), px(18), px(182), px(20)), IDC_VERSION, font,
        );

        create_child(parent, "STATIC", "输入（搜索 / 路径 / 别名 / 备注 / 用例名）",
            SS_LEFT | SS_NOPREFIX, 0,
            (px(18), px(46), px(420), px(16)), 0, font);
        self.input = create_child(
            parent, "EDIT", "", ES_AUTOHSCROLL, WS_EX_CLIENTEDGE,
            (px(18), px(64), px(420), px(30)), IDC_INPUT, font,
        );

        create_child(parent, "BUTTON", "添加收藏(&A)", BS_PUSHBUTTON, 0,
            (px(446), px(64), px(94), px(30)), IDC_ADD, font);
        create_child(parent, "BUTTON", "粘贴剪贴板路径", BS_PUSHBUTTON, 0,
            (px(548), px(64), px(120), px(30)), IDC_PASTE, font);
        create_child(parent, "BUTTON", "建用例目录", BS_PUSHBUTTON, 0,
            (px(676), px(64), px(104), px(30)), IDC_CASE, font);
        self.check = create_child(parent, "BUTTON", "检查更新(&U)", BS_PUSHBUTTON, 0,
            (px(788), px(64), px(94), px(30)), IDC_CHECK, font);

        create_child(parent, "STATIC", "分组", SS_LEFT | SS_NOPREFIX, 0,
            (px(18), px(102), px(176), px(16)), 0, font);
        create_child(parent, "STATIC", "收藏（双击打开；可把文件夹拖进窗口）", SS_LEFT | SS_NOPREFIX, 0,
            (px(204), px(102), px(678), px(16)), 0, font);

        self.groups = create_child(
            parent, "LISTBOX", "", WS_VSCROLL | LBS_NOTIFY | WS_TABSTOP, WS_EX_CLIENTEDGE,
            (px(18), px(120), px(176), px(228)), IDC_GROUPS, font,
        );
        self.list = create_child(
            parent, "LISTBOX", "", WS_VSCROLL | LBS_NOTIFY | WS_TABSTOP, WS_EX_CLIENTEDGE,
            (px(204), px(120), px(678), px(228)), IDC_LIST, font,
        );

        self.detail = create_child(parent, "STATIC", "", SS_LEFT | SS_NOPREFIX, 0,
            (px(18), px(354), px(864), px(38)), IDC_DETAIL, font);

        let y = px(396);
        let h = px(30);
        let button = |text: &str, x: i32, width: i32, id: i32, app_font: HFONT| {
            create_child(parent, "BUTTON", text, BS_PUSHBUTTON, 0,
                (px(x), y, px(width), h), id, app_font);
        };
        button("打开(&O)", 18, 88, IDC_OPEN, font);
        button("复制路径", 114, 96, IDC_COPY, font);
        button("终端打开", 218, 96, IDC_TERMINAL, font);
        button("改别名", 322, 84, IDC_ALIAS, font);
        button("改备注", 414, 84, IDC_NOTE, font);
        button("置顶/取消", 506, 100, IDC_PIN, font);
        button("删除", 614, 76, IDC_DELETE, font);
        button("导入清单", 698, 88, IDC_IMPORT, font);
        button("导出清单", 794, 88, IDC_EXPORT, font);

        create_child(parent, "BUTTON", "更新", BS_GROUPBOX, 0,
            (px(18), px(438), px(864), px(150)), 0, font);
        self.update_line = create_child(parent, "STATIC", "", SS_LEFT | SS_NOPREFIX, 0,
            (px(32), px(460), px(620), px(22)), IDC_UPDATE_LINE, font);
        self.update_notes = create_child(
            parent, "EDIT", "", ES_MULTILINE | ES_READONLY | ES_AUTOVSCROLL | WS_VSCROLL,
            WS_EX_CLIENTEDGE,
            (px(32), px(486), px(620), px(54)), IDC_UPDATE_NOTES, font,
        );
        self.status = create_child(parent, "STATIC", "就绪", SS_LEFT | SS_NOPREFIX, 0,
            (px(32), px(548), px(620), px(22)), IDC_STATUS, font);

        let progress_class = wide(PROGRESS_CLASS);
        self.progress = unsafe {
            CreateWindowExW(
                0,
                progress_class.as_ptr(),
                std::ptr::null(),
                WS_CHILD | PBS_SMOOTH,
                px(32), px(570), px(620), px(12),
                parent, ctrl_id(IDC_PROGRESS), GetModuleHandleW(std::ptr::null()),
                std::ptr::null_mut(),
            )
        };
        unsafe { SendMessageW(self.progress, PBM_SETRANGE, 0, (100i32 << 16) as isize) };

        self.skip = create_child(parent, "BUTTON", "跳过此版本", BS_PUSHBUTTON, 0,
            (px(714), px(458), px(152), px(30)), IDC_SKIP, font);
        self.install = create_child(parent, "BUTTON", "立即安装", BS_DEFPUSHBUTTON, 0,
            (px(714), px(496), px(152), px(32)), IDC_INSTALL, font);
        self.restart = create_child(parent, "BUTTON", "重启软件", BS_DEFPUSHBUTTON, 0,
            (px(714), px(496), px(152), px(32)), IDC_RESTART, font);
        unsafe {
            EnableWindow(self.skip, 0);
            EnableWindow(self.install, 0);
            ShowWindow(self.restart, SW_HIDE);
        }

        self.save_state = create_child(parent, "STATIC", "", SS_LEFT | SS_NOPREFIX, 0,
            (px(18), px(600), px(864), px(20)), IDC_SAVE_STATE, font);

        set_text(self.version_label, &format!("v{}", self.local_version));
        set_text(
            self.update_notes,
            "更新器与主程序分离：下载、校验、替换、回滚都在 generic-updater.exe 里完成。",
        );
        self.refresh_update_line();
        self.refresh_buttons();
        self.set_status("就绪");
        self.set_save_state(&format!(
            "热键 Ctrl+Alt+F 唤出 · 数据目录 {}",
            self.data_dir.display()
        ));
        self.refresh_all();

        unsafe {
            DragAcceptFiles(self.hwnd, 1);
            RegisterHotKey(self.hwnd, HOTKEY_ID, MOD_CONTROL | MOD_ALT, VK_F);
            if self.settings.check_updates_on_start && self.startup_delay_seconds() > 0 {
                SetTimer(
                    self.hwnd,
                    TIMER_STARTUP_CHECK,
                    (self.startup_delay_seconds() * 1000) as UINT,
                    std::ptr::null_mut(),
                );
            }
        }
    }

    fn startup_delay_seconds(&self) -> u64 {
        conf_number("startup_check_delay_seconds", 30)
    }

    fn refresh_all(&mut self) {
        self.refresh_groups();
        self.rebuild_list();
    }

    fn group_labels(&self) -> Vec<String> {
        let mut labels = vec![
            format!("全部收藏 ({})", self.store.groups.iter().map(|group| group.folders.len()).sum::<usize>()),
            format!("置顶 ({})", self.store.pinned_count()),
            format!("最近打开 ({})", self.store.recent.len()),
        ];
        for group in &self.store.groups {
            labels.push(format!("{} ({})", group.name, group.folders.len()));
        }
        labels
    }

    fn refresh_groups(&mut self) {
        let current = unsafe { SendMessageW(self.groups, LB_GETCURSEL, 0, 0) };
        let labels = self.group_labels();
        unsafe {
            SendMessageW(self.groups, LB_RESETCONTENT, 0, 0);
            for label in &labels {
                SendMessageW(self.groups, LB_ADDSTRING, 0, wide(label).as_ptr() as isize);
            }
            let target = if current < 0 {
                0
            } else {
                current.min(labels.len() as isize - 1)
            };
            SendMessageW(self.groups, LB_SETCURSEL, target as usize, 0);
        }
    }

    fn current_filter(&self) -> Filter {
        let index = unsafe { SendMessageW(self.groups, LB_GETCURSEL, 0, 0) };
        match index {
            0 => Filter::All,
            1 => Filter::Pinned,
            2 => Filter::Recent,
            value if value >= 3 => Filter::Group(value as usize - 3),
            _ => Filter::All,
        }
    }

    fn collect(&self, filter: Filter, search: &str) -> Vec<Folder> {
        let mut items: Vec<Folder> = Vec::new();
        match filter {
            Filter::All => {
                for group in &self.store.groups {
                    for folder in &group.folders {
                        if !items.iter().any(|item| same_path(&item.path, &folder.path)) {
                            items.push(folder.clone());
                        }
                    }
                }
                items.sort_by_key(|folder| !folder.pinned);
            }
            Filter::Pinned => {
                for folder in self.store.groups.iter().flat_map(|group| group.folders.iter()) {
                    if folder.pinned {
                        items.push(folder.clone());
                    }
                }
            }
            Filter::Recent => {
                for entry in &self.store.recent {
                    let folder = self
                        .store
                        .find(&entry.path)
                        .cloned()
                        .unwrap_or_else(|| Folder::new(&entry.path));
                    items.push(folder);
                }
            }
            Filter::Group(index) => {
                if let Some(group) = self.store.groups.get(index) {
                    items = group.folders.clone();
                    items.sort_by_key(|folder| !folder.pinned);
                }
            }
        }

        if !search.is_empty() {
            items.retain(|folder| {
                folder.alias.to_lowercase().contains(search)
                    || folder.path.to_lowercase().contains(search)
                    || folder.note.to_lowercase().contains(search)
                    || folder.display_name().to_lowercase().contains(search)
            });
        }
        items
    }

    fn folder_label(&self, folder: &Folder) -> String {
        let mut label = String::new();
        if folder.pinned {
            label.push_str("[置顶] ");
        }
        label.push_str(&folder.display_name());
        label.push_str("    ");
        label.push_str(&folder.path);
        if !folder.note.trim().is_empty() {
            label.push_str("    · ");
            label.push_str(folder.note.trim());
        }
        if !folder.exists() {
            label.push_str("    [路径不存在]");
        }
        if label.chars().count() > 120 {
            label = label.chars().take(120).collect::<String>();
            label.push('…');
        }
        label
    }

    fn rebuild_list(&mut self) {
        let previous = self.selected_path();
        let search = self.input_text().trim().to_lowercase();
        let items = self.collect(self.current_filter(), &search);

        unsafe {
            SendMessageW(self.list, LB_RESETCONTENT, 0, 0);
            for folder in &items {
                let label = self.folder_label(folder);
                SendMessageW(self.list, LB_ADDSTRING, 0, wide(&label).as_ptr() as isize);
            }
        }
        self.view_paths = items.iter().map(|folder| folder.path.clone()).collect();

        if !self.view_paths.is_empty() {
            let target = previous
                .and_then(|path| self.view_paths.iter().position(|item| same_path(item, &path)))
                .unwrap_or(0);
            unsafe { SendMessageW(self.list, LB_SETCURSEL, target, 0) };
        }
        self.update_detail();
    }

    fn selected_path(&self) -> Option<String> {
        let index = unsafe { SendMessageW(self.list, LB_GETCURSEL, 0, 0) };
        if index < 0 {
            return None;
        }
        self.view_paths.get(index as usize).cloned()
    }

    fn update_detail(&self) {
        let text = match self.selected_path().and_then(|path| self.store.find(&path).cloned()) {
            Some(folder) => {
                let mut text = folder.path.clone();
                if !folder.note.trim().is_empty() {
                    text.push_str("    · ");
                    text.push_str(folder.note.trim());
                }
                if folder.exists() {
                    text.push_str("    · 可访问");
                } else {
                    text.push_str("    · 路径不存在：目录可能被删除、盘符变化或网络位置不可用");
                }
                text
            }
            None => "未选中条目".to_string(),
        };
        set_text(self.detail, &text);
    }

    fn input_text(&self) -> String {
        get_text(self.input)
    }

    fn clear_input(&self) {
        set_text(self.input, "");
    }

    fn set_status(&self, text: &str) {
        set_text(self.status, text);
    }

    fn set_save_state(&self, text: &str) {
        set_text(self.save_state, text);
    }

    fn set_update_notes(&self, text: &str) {
        set_text(self.update_notes, text);
    }

    fn refresh_update_line(&self) {
        let line = if self.available_version.is_empty() {
            format!("本地版本 v{} · 最新版本 —", self.local_version)
        } else {
            format!(
                "本地版本 v{} · 最新版本 v{}",
                self.local_version, self.available_version
            )
        };
        set_text(self.update_line, &line);
    }

    fn show_progress(&self, visible: bool, percent: u32) {
        unsafe {
            ShowWindow(self.progress, if visible { SW_SHOW } else { SW_HIDE });
            SendMessageW(self.progress, PBM_SETPOS, percent as usize, 0);
        }
    }

    fn refresh_buttons(&self) {
        let idle = !self.busy;
        let has_update = !self.available_version.is_empty();
        let ready = !self.pending_version.is_empty();
        unsafe {
            EnableWindow(self.check, idle as i32);
            EnableWindow(self.skip, (idle && has_update && !ready) as i32);
            EnableWindow(self.install, (idle && has_update && !self.skipped && !ready) as i32);
        }
    }

    fn open_path(&self, path: &str) {
        let target = wide(path);
        unsafe {
            ShellExecuteW(
                self.hwnd,
                wide("open").as_ptr(),
                target.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                SW_SHOWNORMAL,
            );
        }
    }

    // ------------------------------------------------------------ 收藏操作

    fn add_input(&mut self) {
        let text = self.input_text().trim().to_string();
        if text.is_empty() {
            self.set_status("请先把文件夹路径粘贴到输入框");
            return;
        }
        let path = clean_path(&text);
        if !Path::new(&path).is_dir() {
            self.warn(&format!(
                "不是一个可访问的文件夹：\n{path}\n\n请确认路径存在（目录被删除、盘符变化或网络位置不可用都会导致失败）。"
            ));
            return;
        }
        self.add_folder(&path);
    }

    fn add_folder(&mut self, path: &str) {
        let group = match self.current_filter() {
            Filter::Group(index) => self
                .store
                .groups
                .get(index)
                .map(|group| group.name.clone())
                .unwrap_or_else(|| "未分组".to_string()),
            _ => "未分组".to_string(),
        };
        if self.store.add(Folder::new(path), &group) {
            self.save_store();
            self.clear_input();
            self.refresh_all();
            self.set_status(&format!("已添加到「{group}」：{path}"));
            self.log(&format!("添加收藏 {path}（分组 {group}）"));
        } else {
            self.set_status(&format!("这条路径已经在收藏里了：{path}"));
        }
    }

    fn add_paths(&mut self, paths: &[String]) {
        let mut added = 0;
        let mut skipped = 0;
        for path in paths {
            let path = clean_path(path);
            if Path::new(&path).is_dir() {
                if self.store.add(Folder::new(&path), "未分组") {
                    added += 1;
                } else {
                    skipped += 1;
                }
            } else {
                skipped += 1;
            }
        }
        if added > 0 {
            self.save_store();
            self.refresh_all();
            self.log(&format!("拖入 {added} 个文件夹"));
        }
        self.set_status(&format!("已添加 {added} 个文件夹，跳过 {skipped} 个"));
    }

    fn paste_clipboard(&mut self) {
        let Some(text) = read_clipboard_text(self.hwnd) else {
            self.set_status("剪贴板里没有文本");
            return;
        };
        let path = clean_path(&text);
        if !Path::new(&path).is_dir() {
            self.set_status("剪贴板内容不是可访问的文件夹路径");
            self.warn(&format!(
                "剪贴板里没有可用的文件夹路径：\n{}",
                shorten(&text, 200)
            ));
            return;
        }
        self.set_text_input(&path);
        self.add_folder(&path);
    }

    fn set_text_input(&self, text: &str) {
        set_text(self.input, text);
    }

    fn create_case_dir(&mut self) {
        let case = self.input_text().trim().to_string();
        if case.is_empty() {
            self.warn("请先在上方输入框里填写用例名，再点「建用例目录」。");
            return;
        }
        let Some(base) = self.selected_path() else {
            self.warn("请先在列表里选中一个收藏文件夹，作为用例目录的父目录。");
            return;
        };
        if !Path::new(&base).is_dir() {
            self.warn(&format!(
                "父目录不存在：\n{base}\n\n目录可能被删除、盘符变化或网络位置不可用。"
            ));
            return;
        }
        let name = format!("{}_{}", SYSTEMTIME::now().date(), sanitize(&case));
        let target = Path::new(&base).join(&name);
        match fs::create_dir_all(&target) {
            Ok(_) => {
                self.clear_input();
                self.refresh_all();
                self.set_status(&format!("已创建并打开：{}", target.display()));
                self.log(&format!("创建用例目录 {}", target.display()));
                self.open_path(&target.to_string_lossy());
            }
            Err(error) => self.warn(&format!("创建目录失败：{error}")),
        }
    }

    fn set_alias(&mut self) {
        let Some(path) = self.selected_path() else {
            self.set_status("请先选中一个收藏");
            return;
        };
        let alias = self.input_text().trim().to_string();
        if alias.is_empty() {
            self.warn("请先在上方输入框里填写新别名，再点「改别名」。留空即恢复目录名。");
            return;
        }
        if let Some(folder) = self.store.find_mut(&path) {
            folder.alias = alias.clone();
        }
        self.save_store();
        self.clear_input();
        self.refresh_all();
        self.set_status(&format!("别名已改为「{alias}」"));
    }

    fn set_note(&mut self) {
        let Some(path) = self.selected_path() else {
            self.set_status("请先选中一个收藏");
            return;
        };
        let note = self.input_text().trim().to_string();
        if note.is_empty() {
            self.warn("请先在上方输入框里填写备注，再点「改备注」。");
            return;
        }
        if let Some(folder) = self.store.find_mut(&path) {
            folder.note = note.clone();
        }
        self.save_store();
        self.clear_input();
        self.refresh_all();
        self.set_status("备注已更新");
    }

    fn toggle_pin(&mut self) {
        let Some(path) = self.selected_path() else {
            self.set_status("请先选中一个收藏");
            return;
        };
        let mut pinned = false;
        if let Some(folder) = self.store.find_mut(&path) {
            folder.pinned = !folder.pinned;
            pinned = folder.pinned;
        }
        self.save_store();
        self.refresh_all();
        self.set_status(if pinned { "已置顶" } else { "已取消置顶" });
    }

    fn delete_selected(&mut self) {
        let Some(path) = self.selected_path() else {
            self.set_status("请先选中一个收藏");
            return;
        };
        let answer = unsafe {
            MessageBoxW(
                self.hwnd,
                wide(&format!("从收藏里移除这条记录？\n\n{path}\n\n不会删除磁盘上的目录。")).as_ptr(),
                wide("删除收藏").as_ptr(),
                MB_YESNO | MB_ICONQUESTION,
            )
        };
        if answer != IDYES {
            return;
        }
        if self.store.remove(&path) {
            self.save_store();
            self.refresh_all();
            self.set_status(&format!("已移除收藏：{path}"));
            self.log(&format!("移除收藏 {path}"));
        }
    }

    fn open_selected(&mut self) {
        let Some(path) = self.selected_path() else {
            self.set_status("请先选中一个收藏，或双击列表条目");
            return;
        };
        if !Path::new(&path).is_dir() {
            self.warn(&format!(
                "无法打开：\n{path}\n\n目录不存在。可能是被删除、盘符变化，或网络位置当前不可用。"
            ));
            self.set_status(&format!("路径不存在：{path}"));
            return;
        }
        self.open_path(&path);
        self.store.touch_recent(&path);
        self.save_store();
        self.refresh_all();
        self.set_status(&format!("已打开：{path}"));
        self.log(&format!("打开 {path}"));
    }

    fn copy_selected_path(&mut self) {
        let Some(path) = self.selected_path() else {
            self.set_status("请先选中一个收藏");
            return;
        };
        if copy_text(self.hwnd, &path) {
            self.set_status(&format!("已复制：{path}"));
        } else {
            self.set_status("复制到剪贴板失败");
        }
    }

    fn open_terminal(&mut self) {
        let Some(path) = self.selected_path() else {
            self.set_status("请先选中一个收藏");
            return;
        };
        if !Path::new(&path).is_dir() {
            self.warn(&format!("无法在终端打开：\n{path}\n\n目录不存在。"));
            return;
        }
        let mut opened = Command::new("wt.exe")
            .arg("-d")
            .arg(&path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .is_ok();
        if !opened {
            // 没有 Windows Terminal 时直接起 PowerShell，不用 cmd /c start 这类包装。
            let script = format!("Set-Location -LiteralPath '{}'", path.replace('\'', "''"));
            opened = Command::new("powershell.exe")
                .args(["-NoExit", "-Command", &script])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .creation_flags(CREATE_NEW_CONSOLE)
                .spawn()
                .is_ok();
        }
        if opened {
            self.set_status(&format!("已在终端打开：{path}"));
            self.log(&format!("终端打开 {path}"));
        } else {
            self.set_status("没能拉起终端（Windows Terminal 与 PowerShell 都不可用）");
        }
    }

    fn export_store(&mut self) {
        let target = self.data_dir.join("folders-export.json");
        match self.store.save(&target) {
            Ok(_) => {
                self.set_status(&format!("已导出清单：{}", target.display()));
                self.log("导出清单");
                self.open_path(&self.data_dir.to_string_lossy());
                self.message(&format!(
                    "清单已导出到：\n{}\n\n把这个文件发给同事，对方点「导入清单」即可合并。",
                    target.display()
                ));
            }
            Err(error) => self.warn(&format!("导出失败：{error}")),
        }
    }

    fn import_store(&mut self) {
        let candidates = [
            self.data_dir.join("folders-import.json"),
            self.data_dir.join("folders-export.json"),
        ];
        let Some(source) = candidates.iter().find(|path| path.is_file()).cloned() else {
            self.warn(&format!(
                "没有找到待导入的清单。\n\n请把同事给的 JSON 放到：\n{}\n（或 folders-export.json），再点「导入清单」。",
                candidates[0].display()
            ));
            return;
        };
        let Some(incoming) = Store::load(&source) else {
            self.warn(&format!("清单格式无法解析：{}", source.display()));
            return;
        };
        let answer = unsafe {
            MessageBoxW(
                self.hwnd,
                wide(&format!(
                    "导入 {} ？\n\n同路径的收藏会跳过，只补充缺失条目。",
                    source.display()
                ))
                .as_ptr(),
                wide("导入清单").as_ptr(),
                MB_YESNO | MB_ICONQUESTION,
            )
        };
        if answer != IDYES {
            return;
        }
        let added = self.store.merge(incoming);
        self.save_store();
        self.refresh_all();
        self.set_status(&format!("已从 {} 导入 {added} 条收藏", source.display()));
        self.log(&format!("导入清单 {} 条", added));
    }

    fn focus_search(&self) {
        unsafe {
            ShowWindow(self.hwnd, SW_RESTORE);
            SetForegroundWindow(self.hwnd);
            SetFocus(self.input);
            SendMessageW(self.input, EM_SETSEL, 0, -1);
        }
    }

    fn show_about(&self) {
        let text = format!(
            "{APP_NAME} v{}\n\n\
             {APP_SUBTITLE}：给测试人员用的便携文件夹速取工具。\n\
             · 收藏按分组管理，支持别名、备注、置顶\n\
             · Ctrl+Alt+F 唤出搜索；双击条目在资源管理器打开\n\
             · 粘贴路径或把文件夹拖进窗口即可添加\n\
             · 选中收藏后可一键建「日期_用例名」子目录\n\
             · 「建用例目录」使用输入框里的文本作为用例名\n\n\
             清单：{}\n\
             日志：{}\n\n\
             更新由同目录的 generic-updater.exe 完成（检查/下载/替换/回滚）。\n\
             主页：{HOMEPAGE}\n\n\
             未做代码签名；更新元数据按项目配置验签。",
            self.local_version,
            self.store_path.display(),
            self.log_path.display()
        );
        unsafe {
            MessageBoxW(
                self.hwnd,
                wide(&text).as_ptr(),
                wide(&format!("关于 {APP_NAME}")).as_ptr(),
                MB_OK | MB_ICONINFORMATION,
            );
        }
    }

    // ------------------------------------------------------------ 更新流程

    fn begin_job(&mut self, job: Job) {
        if self.busy {
            return;
        }
        if !self.updater_path.is_file() {
            self.set_status("找不到 generic-updater.exe，请确认它和本程序在同一目录");
            return;
        }
        self.busy = true;
        self.show_progress(false, 0);
        self.set_status(job.status_text());
        self.refresh_buttons();
        if matches!(job, Job::Download) {
            self.set_update_notes("正在下载更新包…");
            self.log(&format!("开始下载 {}", self.available_version));
        } else if matches!(job, Job::CheckAuto | Job::CheckManual) {
            self.log("检查更新");
        }

        let hwnd = self.hwnd as usize;
        let updater = self.updater_path.clone();
        let work_dir = self.exe_dir.clone();
        std::thread::spawn(move || run_job(hwnd, updater, work_dir, job));
    }

    fn toggle_skip(&mut self) {
        if self.available_version.is_empty() {
            return;
        }
        if self.skipped {
            self.begin_job(Job::UnskipVersion);
        } else {
            self.begin_job(Job::SkipVersion(self.available_version.clone()));
        }
    }

    fn apply_update(&mut self) {
        if self.pending_version.is_empty() {
            return;
        }
        let mut command = Command::new(&self.updater_path);
        command
            .args([
                "--apply".to_string(),
                "--parent-pid".to_string(),
                std::process::id().to_string(),
            ])
            .current_dir(&self.exe_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW);
        match command.spawn() {
            Ok(_) => {
                self.log(&format!("安装 {}，进程退出交给更新器", self.pending_version));
                self.set_status("正在重启以完成更新…");
                // 自己先退出，更新器才能在 90 秒窗口内替换被占用的 exe 并重新拉起新版。
                unsafe { DestroyWindow(self.hwnd) };
            }
            Err(error) => self.set_status(&format!("拉起更新器失败：{error}")),
        }
    }

    fn handle_done(&mut self, job: Job, terminal: Option<Terminal>) {
        self.busy = false;
        self.show_progress(false, 0);
        self.refresh_buttons();

        if matches!(job, Job::SkipVersion(_) | Job::UnskipVersion) {
            // 更新器已经把跳过状态写进自己的缓存，重新检查一次让界面与它对齐。
            self.skipped = matches!(job, Job::SkipVersion(_));
            self.set_status(if self.skipped {
                "已跳过当前版本，后续检查不会再提示"
            } else {
                "已恢复该版本的更新提示"
            });
            self.begin_job(Job::CheckManual);
            return;
        }

        let Some(terminal) = terminal else {
            self.set_status("更新器没有返回状态，请重试");
            self.log("更新器无输出");
            return;
        };

        if terminal.state == "Failed" {
            let reason = terminal.error.unwrap_or_else(|| "未知错误".to_string());
            self.set_status(&format!("更新失败：{reason}"));
            self.log(&format!("更新失败：{reason}"));
            return;
        }

        if matches!(job, Job::Download) {
            if terminal.state != "Ready" {
                self.set_status("下载未完成，请重试");
                return;
            }
            self.pending_version = terminal.version;
            self.set_update_notes(&format!(
                "已下载 v{}（{}），点击「重启软件」完成安装。",
                self.pending_version,
                format_bytes(terminal.total)
            ));
            self.set_status("更新已就绪，点击「重启软件」完成安装");
            unsafe {
                ShowWindow(self.install, SW_HIDE);
                ShowWindow(self.restart, SW_SHOW);
                EnableWindow(self.restart, 1);
            }
            self.refresh_buttons();
            self.log(&format!("下载完成 v{}", self.pending_version));
            return;
        }

        match terminal.state.as_str() {
            "Available" => {
                self.available_version = terminal.version.clone();
                self.skipped = terminal.skipped;
                self.refresh_update_line();
                self.set_update_notes(if terminal.notes.trim().is_empty() {
                    "（该版本没有填写更新日志）"
                } else {
                    &terminal.notes
                });
                set_text(
                    self.skip,
                    if terminal.skipped {
                        "取消跳过"
                    } else {
                        "跳过此版本"
                    },
                );
                if terminal.skipped {
                    self.set_status(&format!(
                        "版本 v{} 已被跳过，点「取消跳过」恢复提示",
                        terminal.version
                    ));
                } else {
                    self.set_status(&format!(
                        "发现新版本 v{}（{}）",
                        terminal.version,
                        format_bytes(terminal.total)
                    ));
                }
                unsafe {
                    ShowWindow(self.install, SW_SHOW);
                    ShowWindow(self.restart, SW_HIDE);
                }
                self.refresh_buttons();
                self.log(&format!("发现新版本 v{}", terminal.version));
            }
            "Idle" => {
                self.available_version.clear();
                self.skipped = false;
                self.refresh_update_line();
                self.set_update_notes("当前已是最新版本。");
                self.set_status("当前已是最新版本");
                set_text(self.skip, "跳过此版本");
                unsafe { ShowWindow(self.restart, SW_HIDE) };
                self.refresh_buttons();
            }
            other => self.set_status(&format!("更新器返回了未预期的状态：{other}")),
        }
    }
}

// ---------------------------------------------------------------- 更新器子进程

fn run_job(hwnd: usize, updater: PathBuf, work_dir: PathBuf, job: Job) {
    let mut command = Command::new(&updater);
    command
        .args(job.args())
        .current_dir(&work_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW);

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(_) => {
            post(hwnd, UiMsg::StartFailed);
            return;
        }
    };

    let mut terminal: Option<Terminal> = None;
    let mut last_percent = u32::MAX;
    if let Some(stdout) = child.stdout.take() {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let Some(frame) = parse_frame(&line) else {
                continue;
            };
            if frame.state == "Downloading" {
                let percent = if frame.total > 0 {
                    (frame.received.saturating_mul(100) / frame.total) as u32
                } else {
                    0
                };
                if percent != last_percent {
                    last_percent = percent;
                    post(
                        hwnd,
                        UiMsg::Progress {
                            percent,
                            received: frame.received,
                        },
                    );
                }
                continue;
            }
            if matches!(frame.state.as_str(), "Available" | "Idle" | "Ready" | "Failed") {
                terminal = Some(Terminal {
                    state: frame.state,
                    version: frame.version,
                    notes: frame.notes,
                    error: frame.error,
                    skipped: frame.skipped,
                    total: frame.total,
                });
            }
        }
    }

    let _ = child.wait();
    post(hwnd, UiMsg::Done { job, terminal });
}

fn parse_frame(line: &str) -> Option<Frame> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    let state = value.get("state")?.as_str()?.to_string();
    let text = |key: &str| -> String {
        value
            .get(key)
            .and_then(|item| item.as_str())
            .unwrap_or_default()
            .to_string()
    };
    let number =
        |key: &str| -> u64 { value.get(key).and_then(|item| item.as_u64()).unwrap_or(0) };
    Some(Frame {
        state,
        version: text("version"),
        notes: text("notes"),
        error: value
            .get("error")
            .and_then(|item| item.as_str())
            .map(str::to_string),
        received: number("received"),
        total: number("total"),
        skipped: value
            .get("skipped")
            .and_then(|item| item.as_bool())
            .unwrap_or(false),
    })
}

fn post(hwnd: usize, message: UiMsg) {
    unsafe {
        let window = hwnd as HWND;
        let raw = Box::into_raw(Box::new(message)) as isize;
        if IsWindow(window) != 0 {
            PostMessageW(window, WM_APP_JOB, 0, raw);
        } else {
            drop(Box::from_raw(raw as *mut UiMsg));
        }
    }
}

// ---------------------------------------------------------------- 窗口过程

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: UINT,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    match message {
        WM_CREATE => {
            let create = l_param as *const CREATESTRUCTW;
            let app_ptr = (*create).lp_create_params as *mut App;
            if app_ptr.is_null() {
                return -1;
            }
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, app_ptr as isize);
            (*app_ptr).hwnd = hwnd;
            (*app_ptr).build_ui();
            0
        }
        WM_COMMAND => {
            let id = (w_param & 0xffff) as i32;
            let notification = ((w_param >> 16) & 0xffff) as u16;
            with_app(hwnd, |app| match id {
                IDC_INPUT if notification == EN_CHANGE => app.rebuild_list(),
                IDC_GROUPS if notification == LBN_SELCHANGE => app.rebuild_list(),
                IDC_LIST if notification == LBN_DBLCLK => app.open_selected(),
                IDC_LIST if notification == LBN_SELCHANGE => app.update_detail(),
                IDC_ADD | IDM_ADD => app.add_input(),
                IDC_PASTE | IDM_PASTE => app.paste_clipboard(),
                IDC_CASE | IDM_CASE => app.create_case_dir(),
                IDC_OPEN => app.open_selected(),
                IDC_COPY => app.copy_selected_path(),
                IDC_TERMINAL => app.open_terminal(),
                IDC_ALIAS => app.set_alias(),
                IDC_NOTE => app.set_note(),
                IDC_PIN | IDM_PIN => app.toggle_pin(),
                IDC_DELETE | IDM_DELETE => app.delete_selected(),
                IDC_IMPORT | IDM_IMPORT => app.import_store(),
                IDC_EXPORT | IDM_EXPORT => app.export_store(),
                IDC_CHECK | IDM_CHECK => app.begin_job(Job::CheckManual),
                IDC_INSTALL => app.begin_job(Job::Download),
                IDC_RESTART => app.apply_update(),
                IDC_SKIP => app.toggle_skip(),
                IDM_DATA => app.open_path(&app.data_dir.to_string_lossy()),
                IDM_LOG => app.open_path(&app.log_path.to_string_lossy()),
                IDM_REFRESH => {
                    app.refresh_all();
                    app.set_status("已刷新");
                }
                IDM_EXIT => {
                    DestroyWindow(hwnd);
                }
                IDM_ABOUT => app.show_about(),
                _ => {}
            });
            0
        }
        WM_HOTKEY => {
            with_app(hwnd, |app| app.focus_search());
            0
        }
        WM_APP_ACTIVATE => {
            with_app(hwnd, |app| app.focus_search());
            0
        }
        WM_DROPFILES => {
            let drop = w_param as *mut std::ffi::c_void;
            let count = DragQueryFileW(drop, u32::MAX, std::ptr::null_mut(), 0);
            let mut paths = Vec::new();
            for index in 0..count {
                let length = DragQueryFileW(drop, index, std::ptr::null_mut(), 0);
                if length == 0 {
                    continue;
                }
                let mut buffer = vec![0u16; length as usize + 1];
                DragQueryFileW(drop, index, buffer.as_mut_ptr(), buffer.len() as u32);
                paths.push(from_wide(&buffer));
            }
            DragFinish(drop);
            with_app(hwnd, |app| app.add_paths(&paths));
            0
        }
        WM_TIMER => {
            with_app(hwnd, |app| {
                if w_param == TIMER_STARTUP_CHECK {
                    KillTimer(hwnd, TIMER_STARTUP_CHECK);
                    app.begin_job(Job::CheckAuto);
                }
            });
            0
        }
        WM_APP_JOB => {
            let message = Box::from_raw(l_param as *mut UiMsg);
            with_app(hwnd, |app| match *message {
                UiMsg::Progress { percent, received } => {
                    app.show_progress(true, percent);
                    app.set_status(&format!(
                        "正在下载更新… {percent}%（{}）",
                        format_bytes(received)
                    ));
                }
                UiMsg::Done { job, terminal } => app.handle_done(job, terminal),
                UiMsg::StartFailed => {
                    app.busy = false;
                    app.refresh_buttons();
                    app.set_status("无法启动更新器 generic-updater.exe");
                }
            });
            0
        }
        WM_CTLCOLORSTATIC => {
            let control = l_param as HWND;
            let dc = w_param as HDC;
            let mut window_color = false;
            with_app(hwnd, |app| {
                window_color = control == app.update_notes;
            });
            if window_color {
                SetBkColor(dc, GetSysColor(COLOR_WINDOW) as DWORD);
                SetTextColor(dc, GetSysColor(COLOR_WINDOWTEXT) as DWORD);
                return GetSysColorBrush(COLOR_WINDOW) as LRESULT;
            }
            SetBkMode(dc, TRANSPARENT);
            GetSysColorBrush(COLOR_BTNFACE) as LRESULT
        }
        WM_CTLCOLOREDIT => {
            let dc = w_param as HDC;
            SetBkColor(dc, GetSysColor(COLOR_WINDOW) as DWORD);
            SetTextColor(dc, GetSysColor(COLOR_WINDOWTEXT) as DWORD);
            GetSysColorBrush(COLOR_WINDOW) as LRESULT
        }
        WM_CLOSE => {
            with_app(hwnd, |app| app.log("退出"));
            DestroyWindow(hwnd);
            0
        }
        WM_DESTROY => {
            UnregisterHotKey(hwnd, HOTKEY_ID);
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            if !ptr.is_null() {
                drop(Box::from_raw(ptr));
            }
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, message, w_param, l_param),
    }
}

unsafe fn with_app<R>(hwnd: HWND, action: impl FnOnce(&mut App) -> R) -> Option<R> {
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
    if ptr.is_null() {
        None
    } else {
        Some(action(&mut *ptr))
    }
}

// ---------------------------------------------------------------- 入口

fn main() {
    std::process::exit(run());
}

fn run() -> i32 {
    unsafe {
        if InitCommonControlsEx(&INITCOMMONCONTROLSEX {
            dw_size: std::mem::size_of::<INITCOMMONCONTROLSEX>() as DWORD,
            dw_icc: ICC_PROGRESS_CLASS,
        }) == 0
        {
            return 1;
        }

        let mutex_name = wide(MUTEX_NAME);
        let mut mutex = CreateMutexW(std::ptr::null_mut(), 1, mutex_name.as_ptr());
        if !mutex.is_null() && GetLastError() == ERROR_ALREADY_EXISTS {
            CloseHandle(mutex);
            mutex = std::ptr::null_mut();
            // 更新后重启时旧进程可能还没退干净，短暂重试再判定重复启动。
            for _ in 0..10 {
                std::thread::sleep(std::time::Duration::from_millis(150));
                let retry = CreateMutexW(std::ptr::null_mut(), 1, mutex_name.as_ptr());
                if retry.is_null() {
                    continue;
                }
                if GetLastError() != ERROR_ALREADY_EXISTS {
                    mutex = retry;
                    break;
                }
                CloseHandle(retry);
            }
            if mutex.is_null() {
                // 已经在运行：把已有窗口叫到前台，而不是再弹一个。
                let class_name = wide(CLASS_NAME);
                let existing = FindWindowW(class_name.as_ptr(), std::ptr::null());
                if !existing.is_null() {
                    PostMessageW(existing, WM_APP_ACTIVATE, 0, 0);
                } else {
                    MessageBoxW(
                        std::ptr::null_mut(),
                        wide("程序已经在运行了。").as_ptr(),
                        wide(APP_NAME).as_ptr(),
                        MB_OK | MB_ICONINFORMATION,
                    );
                }
                return 0;
            }
        }

        let screen = GetDC(std::ptr::null_mut());
        let mut dpi = GetDeviceCaps(screen, LOGPIXELSY);
        ReleaseDC(std::ptr::null_mut(), screen);
        if dpi <= 0 {
            dpi = 96;
        }

        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|path| path.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| PathBuf::from("."));
        let app = Box::new(App::new(exe_dir, dpi));
        let app_ptr = Box::into_raw(app);

        let instance = GetModuleHandleW(std::ptr::null());
        let icon = LoadIconW(instance, 1 as usize as LPCWSTR);
        let class_name = wide(CLASS_NAME);
        let window_class = WNDCLASSEXW {
            cb_size: std::mem::size_of::<WNDCLASSEXW>() as UINT,
            style: 0,
            lpfn_wnd_proc: Some(window_proc),
            cb_cls_extra: 0,
            cb_wnd_extra: 0,
            h_instance: instance,
            h_icon: icon,
            h_cursor: LoadCursorW(std::ptr::null_mut(), 32512 as usize as LPCWSTR),
            hbr_background: GetSysColorBrush(COLOR_BTNFACE),
            lpsz_menu_name: std::ptr::null(),
            lpsz_class_name: class_name.as_ptr(),
            h_icon_sm: icon,
        };
        if RegisterClassExW(&window_class) == 0 {
            drop(Box::from_raw(app_ptr));
            return 1;
        }

        let menu = build_menu();
        let style = WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX | WS_CLIPCHILDREN;
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: dpi_scale(CLIENT_WIDTH, dpi),
            bottom: dpi_scale(CLIENT_HEIGHT, dpi),
        };
        AdjustWindowRectExForDpi(&mut rect, style, 1, WS_EX_APPWINDOW, dpi as UINT);

        let title = wide(&format!(
            "{} · {} v{}",
            APP_SUBTITLE, APP_NAME, (*app_ptr).local_version
        ));
        let hwnd = CreateWindowExW(
            WS_EX_APPWINDOW,
            class_name.as_ptr(),
            title.as_ptr(),
            style,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            rect.right - rect.left,
            rect.bottom - rect.top,
            std::ptr::null_mut(),
            menu,
            instance,
            app_ptr as *mut std::ffi::c_void,
        );
        if hwnd.is_null() {
            drop(Box::from_raw(app_ptr));
            if !mutex.is_null() {
                CloseHandle(mutex);
            }
            return 1;
        }

        ShowWindow(hwnd, SW_SHOW);
        UpdateWindow(hwnd);

        let mut message = MSG {
            hwnd: std::ptr::null_mut(),
            message: 0,
            w_param: 0,
            l_param: 0,
            time: 0,
            pt: POINT { x: 0, y: 0 },
            l_private: 0,
        };
        while GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }

        if !mutex.is_null() {
            CloseHandle(mutex);
        }
        message.w_param as i32
    }
}

unsafe fn build_menu() -> HMENU {
    let file = CreatePopupMenu();
    AppendMenuW(file, MF_STRING, IDM_IMPORT as usize, wide("导入清单(&I)").as_ptr());
    AppendMenuW(file, MF_STRING, IDM_EXPORT as usize, wide("导出清单(&E)").as_ptr());
    AppendMenuW(file, MF_SEPARATOR, 0, std::ptr::null());
    AppendMenuW(file, MF_STRING, IDM_DATA as usize, wide("打开数据目录(&D)").as_ptr());
    AppendMenuW(file, MF_STRING, IDM_LOG as usize, wide("打开运行日志(&L)").as_ptr());
    AppendMenuW(file, MF_SEPARATOR, 0, std::ptr::null());
    AppendMenuW(file, MF_STRING, IDM_EXIT as usize, wide("退出(&X)").as_ptr());

    let favorites = CreatePopupMenu();
    AppendMenuW(favorites, MF_STRING, IDM_ADD as usize, wide("添加输入框中的路径(&A)").as_ptr());
    AppendMenuW(favorites, MF_STRING, IDM_PASTE as usize, wide("粘贴剪贴板路径(&P)").as_ptr());
    AppendMenuW(favorites, MF_STRING, IDM_CASE as usize, wide("新建用例目录(&C)").as_ptr());
    AppendMenuW(favorites, MF_SEPARATOR, 0, std::ptr::null());
    AppendMenuW(favorites, MF_STRING, IDM_PIN as usize, wide("置顶 / 取消置顶(&T)").as_ptr());
    AppendMenuW(favorites, MF_STRING, IDM_DELETE as usize, wide("删除选中收藏(&R)").as_ptr());
    AppendMenuW(favorites, MF_SEPARATOR, 0, std::ptr::null());
    AppendMenuW(favorites, MF_STRING, IDM_REFRESH as usize, wide("刷新(&F5)").as_ptr());

    let help = CreatePopupMenu();
    AppendMenuW(help, MF_STRING, IDM_CHECK as usize, wide("检查更新(&U)").as_ptr());
    AppendMenuW(help, MF_STRING, IDM_ABOUT as usize, wide("关于(&A)").as_ptr());

    let bar = CreateMenu();
    AppendMenuW(bar, MF_POPUP, file as usize, wide("文件(&F)").as_ptr());
    AppendMenuW(bar, MF_POPUP, favorites as usize, wide("收藏(&C)").as_ptr());
    AppendMenuW(bar, MF_POPUP, help as usize, wide("帮助(&H)").as_ptr());
    bar
}

// ---------------------------------------------------------------- 小工具

fn ctrl_id(id: i32) -> HMENU {
    id as usize as *mut std::ffi::c_void
}

fn create_child(
    parent: HWND,
    class_name: &str,
    text: &str,
    style: DWORD,
    ex_style: DWORD,
    rect: (i32, i32, i32, i32),
    id: i32,
    font: HFONT,
) -> HWND {
    let class_name = wide(class_name);
    let text = wide(text);
    let hwnd = unsafe {
        CreateWindowExW(
            ex_style,
            class_name.as_ptr(),
            text.as_ptr(),
            WS_CHILD | WS_VISIBLE | style,
            rect.0,
            rect.1,
            rect.2,
            rect.3,
            parent,
            ctrl_id(id),
            GetModuleHandleW(std::ptr::null()),
            std::ptr::null_mut(),
        )
    };
    if !font.is_null() {
        unsafe { SendMessageW(hwnd, WM_SETFONT, font as usize, 1) };
    }
    hwnd
}

fn create_font(height: i32, weight: i32) -> HFONT {
    let face = wide("Microsoft YaHei UI");
    unsafe {
        CreateFontW(
            -height,
            0,
            0,
            0,
            weight,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            DEFAULT_PITCH | FF_DONTCARE,
            face.as_ptr(),
        )
    }
}

fn set_text(hwnd: HWND, text: &str) {
    unsafe { SetWindowTextW(hwnd, wide(text).as_ptr()) };
}

fn get_text(hwnd: HWND) -> String {
    unsafe {
        let length = GetWindowTextLengthW(hwnd);
        if length <= 0 {
            return String::new();
        }
        let mut buffer = vec![0u16; length as usize + 1];
        let read = GetWindowTextW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32);
        if read <= 0 {
            return String::new();
        }
        buffer.truncate(read as usize);
        from_wide(&buffer)
    }
}

fn copy_text(hwnd: HWND, text: &str) -> bool {
    unsafe {
        let data = wide(text);
        let bytes = data.len() * 2;
        let handle = GlobalAlloc(GMEM_MOVEABLE, bytes);
        if handle.is_null() {
            return false;
        }
        let target = GlobalLock(handle) as *mut u16;
        if target.is_null() {
            GlobalFree(handle);
            return false;
        }
        std::ptr::copy_nonoverlapping(data.as_ptr(), target, data.len());
        GlobalUnlock(handle);

        if OpenClipboard(hwnd) == 0 {
            GlobalFree(handle);
            return false;
        }
        EmptyClipboard();
        let stored = !SetClipboardData(CF_UNICODETEXT, handle).is_null();
        CloseClipboard();
        if !stored {
            GlobalFree(handle);
        }
        stored
    }
}

fn read_clipboard_text(hwnd: HWND) -> Option<String> {
    unsafe {
        if OpenClipboard(hwnd) == 0 {
            return None;
        }
        let mut text = None;
        if IsClipboardFormatAvailable(CF_UNICODETEXT) != 0 {
            let handle = GetClipboardData(CF_UNICODETEXT);
            if !handle.is_null() {
                let pointer = GlobalLock(handle) as *const u16;
                if !pointer.is_null() {
                    let mut length = 0usize;
                    while *pointer.add(length) != 0 {
                        length += 1;
                    }
                    let slice = std::slice::from_raw_parts(pointer, length);
                    text = Some(String::from_utf16_lossy(slice));
                    GlobalUnlock(handle);
                }
            }
        }
        CloseClipboard();
        text
    }
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

fn clean_path(text: &str) -> String {
    text.trim()
        .trim_matches('"')
        .trim_matches('\'')
        .trim()
        .to_string()
}

fn sanitize(name: &str) -> String {
    name.chars()
        .map(|ch| match ch {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            _ => ch,
        })
        .collect()
}

fn shorten(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let mut out: String = text.chars().take(limit).collect();
    out.push('…');
    out
}

/// 从编译进二进制的 updater.conf 里取一个数字配置。
fn conf_number(key: &str, fallback: u64) -> u64 {
    UPDATER_CONF
        .lines()
        .filter_map(|line| line.split_once('='))
        .find(|(name, _)| name.trim() == key)
        .and_then(|(_, value)| value.trim().parse().ok())
        .unwrap_or(fallback)
}

/// 首次运行给测试人员准备几个真实存在的目录，省得打开是空列表。
fn seed_store() -> Store {
    let mut store = Store::default();
    let mut group = Group {
        name: "常用".to_string(),
        folders: Vec::new(),
    };
    if let Some(home) = std::env::var_os("USERPROFILE").map(PathBuf::from) {
        for (alias, sub) in [
            ("桌面", "Desktop"),
            ("下载", "Downloads"),
            ("文档", "Documents"),
        ] {
            let path = home.join(sub);
            if path.is_dir() {
                group.folders.push(Folder {
                    path: path.to_string_lossy().into_owned(),
                    alias: alias.to_string(),
                    ..Folder::default()
                });
            }
        }
    }
    if !group.folders.is_empty() {
        store.groups.push(group);
    }
    store
}
