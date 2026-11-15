// Generic Updater Demo —— 仿 GuohеView 那种「版本更新」小窗的 Win32 客户端。
//
// 职责分工：
//   * 本程序只负责 UI 与进程协调：拉起 generic-updater.exe、读它的 stdout JSON 帧、
//     把状态映射到按钮/进度条；真正的下载、校验、替换、重启由 Rust 更新器完成。
//   * 点「重启软件」时：先把 --apply 以独立进程拉起，然后自己退出，让更新器能在
//     exe 释放后完成替换并重新拉起新版本（浏览器版做不到这一点，所以改用原生窗口）。
//
// 编译（MSVC）：
//   cl /nologo /O2 /EHsc /utf-8 /W4 client\src\gui.cpp /Fe:GuiUpdaterDemo.exe ^
//      /link user32.lib gdi32.lib comctl32.lib shell32.lib /SUBSYSTEM:WINDOWS
// 编译（MinGW）：g++ -O2 -municode -mwindows -o GuiUpdaterDemo.exe client\src\gui.cpp -lcomctl32 -lshell32

#define WIN32_LEAN_AND_MEAN
#define _CRT_SECURE_NO_WARNINGS
#include <windows.h>
#include <commctrl.h>
#include <shellapi.h>

#include <string>
#include <vector>
#include <cstdint>
#include <cstdlib>

// 构建期由 scripts/gen-app-config.ps1 从 updater.conf 生成，避免运行期再带 ini。
#include "../build-config.h"

#ifndef APP_HOMEPAGE
#define APP_HOMEPAGE L"https://github.com/Luogoddes/openai-gemini"
#endif
#ifndef APP_EXE_NAME
#define APP_EXE_NAME L"GuiUpdaterDemo.exe"
#endif
#ifndef APP_WINDOW_TITLE
#define APP_WINDOW_TITLE L"Generic Updater Demo — 版本更新"
#endif

#ifndef WM_DPICHANGED
#define WM_DPICHANGED 0x02E0
#endif

#pragma comment(lib, "comctl32.lib")
#pragma comment(lib, "user32.lib")
#pragma comment(lib, "gdi32.lib")
#pragma comment(lib, "shell32.lib")

namespace {

std::wstring gWindowTitle;
const wchar_t* kMutexName = L"Local\\GenericUpdaterDemo.SingleInstance";

const int WM_APP_DONE = WM_APP + 1;
const int WM_APP_PROGRESS = WM_APP + 2;

enum ControlId {
  IDC_LOCAL_VERSION = 1001,
  IDC_LATEST_VERSION = 1002,
  IDC_UPDATE_HINT = 1003,
  IDC_PACKAGE_SIZE = 1004,
  IDC_CHECK = 1005,
  IDC_NOTES = 1006,
  IDC_PROGRESS = 1007,
  IDC_STATUS = 1008,
  IDC_SKIP = 1009,
  IDC_HOMEPAGE = 1010,
  IDC_INSTALL = 1011,
  IDC_RESTART = 1012,
  IDC_BUILD_INFO = 1013,
};

enum JobMode {
  JOB_CHECK_AUTO = 1,
  JOB_CHECK_MANUAL = 2,
  JOB_DOWNLOAD = 3,
};

struct JobResult {
  int mode = 0;
  std::wstring state;
  std::wstring version;
  std::wstring notes;
  std::wstring error;
  unsigned long long size = 0;
  bool skipped = false;
};

struct AppState {
  std::wstring baseDir;
  std::wstring updaterPath;
  std::wstring versionPath;

  std::wstring localVersion;
  std::wstring availableVersion;
  std::wstring pendingVersion;
  bool skipped = false;
  bool busy = false;
};

AppState gState;

HWND gLocalVersion = nullptr;
HWND gLatestVersion = nullptr;
HWND gUpdateHint = nullptr;
HWND gPackageSize = nullptr;
HWND gNotes = nullptr;
HWND gProgress = nullptr;
HWND gStatus = nullptr;
HWND gCheck = nullptr;
HWND gSkip = nullptr;
HWND gHomepage = nullptr;
HWND gInstall = nullptr;
HWND gRestart = nullptr;
HWND gBuildInfo = nullptr;

HFONT gFont = nullptr;
HFONT gTitleFont = nullptr;

// ---------------------------------------------------------------- 文本工具

std::string WideToUtf8(const std::wstring& text) {
  if (text.empty()) return std::string();
  int size = WideCharToMultiByte(CP_UTF8, 0, text.c_str(), (int)text.size(), nullptr, 0, nullptr, nullptr);
  std::string out(size, '\0');
  WideCharToMultiByte(CP_UTF8, 0, text.c_str(), (int)text.size(), &out[0], size, nullptr, nullptr);
  return out;
}

std::wstring Utf8ToWide(const std::string& text) {
  if (text.empty()) return std::wstring();
  int size = MultiByteToWideChar(CP_UTF8, 0, text.c_str(), (int)text.size(), nullptr, 0);
  std::wstring out(size, L'\0');
  MultiByteToWideChar(CP_UTF8, 0, text.c_str(), (int)text.size(), &out[0], size);
  return out;
}

std::wstring ReadTextFile(const std::wstring& path) {
  HANDLE file = CreateFileW(path.c_str(), GENERIC_READ, FILE_SHARE_READ | FILE_SHARE_WRITE, nullptr,
                            OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, nullptr);
  if (file == INVALID_HANDLE_VALUE) return std::wstring();
  LARGE_INTEGER size = {};
  if (!GetFileSizeEx(file, &size) || size.QuadPart <= 0 || size.QuadPart > (1 << 20)) {
    CloseHandle(file);
    return std::wstring();
  }
  std::string buffer((size_t)size.QuadPart, '\0');
  DWORD read = 0;
  BOOL ok = ReadFile(file, &buffer[0], (DWORD)buffer.size(), &read, nullptr);
  CloseHandle(file);
  if (!ok) return std::wstring();
  buffer.resize(read);
  return Utf8ToWide(buffer);
}

bool WriteTextFile(const std::wstring& path, const std::wstring& text) {
  HANDLE file = CreateFileW(path.c_str(), GENERIC_WRITE,
                            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE, nullptr,
                            CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL, nullptr);
  if (file == INVALID_HANDLE_VALUE) return false;
  std::string utf8 = WideToUtf8(text);
  DWORD written = 0;
  BOOL ok = WriteFile(file, utf8.data(), (DWORD)utf8.size(), &written, nullptr);
  CloseHandle(file);
  return ok != FALSE;
}

std::wstring Trim(std::wstring text) {
  const wchar_t* spaces = L" \t\r\n";
  size_t begin = text.find_first_not_of(spaces);
  if (begin == std::wstring::npos) return std::wstring();
  size_t end = text.find_last_not_of(spaces);
  return text.substr(begin, end - begin + 1);
}

std::wstring FormatBytes(unsigned long long bytes) {
  if (bytes < 1024) return std::to_wstring(bytes) + L" B";
  double mb = (double)bytes / (1024.0 * 1024.0);
  wchar_t buffer[64];
  swprintf(buffer, 64, L"%.1f MB", mb);
  return buffer;
}

std::wstring DirectoryOf(const std::wstring& path) {
  size_t pos = path.find_last_of(L"\\/");
  return pos == std::wstring::npos ? std::wstring() : path.substr(0, pos);
}

// ---------------------------------------------------------------- JSON 帧解析

std::wstring JsonString(const std::string& line, const char* key) {
  std::string pattern = std::string("\"") + key + "\":\"";
  size_t pos = line.find(pattern);
  if (pos == std::string::npos) return std::wstring();
  pos += pattern.size();
  std::string out;
  while (pos < line.size()) {
    char ch = line[pos++];
    if (ch == '"') break;
    if (ch != '\\') {
      out.push_back(ch);
      continue;
    }
    if (pos >= line.size()) break;
    char escape = line[pos++];
    switch (escape) {
      case 'n': out.push_back('\n'); break;
      case 'r': out.push_back('\r'); break;
      case 't': out.push_back('\t'); break;
      case 'b': out.push_back('\b'); break;
      case 'f': out.push_back('\f'); break;
      case 'u': {
        if (pos + 4 > line.size()) { pos = line.size(); break; }
        unsigned code = (unsigned)strtoul(line.substr(pos, 4).c_str(), nullptr, 16);
        pos += 4;
        if (code < 0x80) {
          out.push_back((char)code);
        } else if (code < 0x800) {
          out.push_back((char)(0xC0 | (code >> 6)));
          out.push_back((char)(0x80 | (code & 0x3F)));
        } else {
          out.push_back((char)(0xE0 | (code >> 12)));
          out.push_back((char)(0x80 | ((code >> 6) & 0x3F)));
          out.push_back((char)(0x80 | (code & 0x3F)));
        }
        break;
      }
      default: out.push_back(escape); break;
    }
  }
  return Utf8ToWide(out);
}

unsigned long long JsonNumber(const std::string& line, const char* key) {
  std::string pattern = std::string("\"") + key + "\":";
  size_t pos = line.find(pattern);
  if (pos == std::string::npos) return 0;
  pos += pattern.size();
  unsigned long long value = 0;
  while (pos < line.size() && line[pos] >= '0' && line[pos] <= '9') {
    value = value * 10 + (unsigned long long)(line[pos] - '0');
    ++pos;
  }
  return value;
}


// ---------------------------------------------------------------- 进程调用

std::wstring Quote(const std::wstring& text) {
  return L"\"" + text + L"\"";
}

struct CaptureContext {
  HWND hwnd = nullptr;
  bool sawTerminal = false;
  JobResult result;
};

void HandleFrame(const std::string& line, CaptureContext* context) {
  std::wstring state = JsonString(line, "state");
  if (state.empty()) return;

  if (state == L"Downloading") {
    unsigned long long received = JsonNumber(line, "received");
    unsigned long long total = JsonNumber(line, "total");
    WPARAM percent = 0;
    if (total > 0) percent = (WPARAM)((received * 100) / total);
    if (context->hwnd) {
      PostMessageW(context->hwnd, WM_APP_PROGRESS, percent, (LPARAM)received);
    }
    context->result.size = total;
    return;
  }

  context->result.state = state;
  context->result.version = JsonString(line, "version");
  context->result.notes = JsonString(line, "notes");
  context->result.error = JsonString(line, "error");
  context->result.skipped = line.find("\"skipped\":true") != std::string::npos;
  unsigned long long total = JsonNumber(line, "total");
  if (total > 0) context->result.size = total;

  if (state == L"Available" || state == L"Idle" || state == L"Ready" || state == L"Failed") {
    context->sawTerminal = true;
  }
}

// 同步跑更新器并逐行读 stdout；返回退出码。
DWORD RunUpdaterCapture(const std::wstring& arguments, CaptureContext* context) {
  SECURITY_ATTRIBUTES sa = {};
  sa.nLength = sizeof(sa);
  sa.bInheritHandle = TRUE;

  HANDLE readPipe = nullptr;
  HANDLE writePipe = nullptr;
  if (!CreatePipe(&readPipe, &writePipe, &sa, 0)) return 1;
  SetHandleInformation(readPipe, HANDLE_FLAG_INHERIT, 0);

  STARTUPINFOW startup = {};
  startup.cb = sizeof(startup);
  startup.dwFlags = STARTF_USESTDHANDLES;
  startup.hStdOutput = writePipe;
  startup.hStdError = writePipe;
  startup.hStdInput = nullptr;

  PROCESS_INFORMATION process = {};
  std::wstring commandLine = Quote(gState.updaterPath) + L" " + arguments;

  BOOL created = CreateProcessW(nullptr, &commandLine[0], nullptr, nullptr, TRUE, 0,
                                nullptr, gState.baseDir.c_str(), &startup, &process);
  CloseHandle(writePipe);
  if (!created) {
    CloseHandle(readPipe);
    return 1;
  }

  std::string pending;
  char buffer[4096];
  DWORD read = 0;
  while (ReadFile(readPipe, buffer, sizeof(buffer), &read, nullptr) && read > 0) {
    pending.append(buffer, read);
    size_t newline = 0;
    while ((newline = pending.find('\n')) != std::string::npos) {
      std::string line = pending.substr(0, newline);
      pending.erase(0, newline + 1);
      if (!line.empty() && line.back() == '\r') line.pop_back();
      HandleFrame(line, context);
    }
  }
  if (!pending.empty()) HandleFrame(pending, context);

  WaitForSingleObject(process.hProcess, INFINITE);
  DWORD exitCode = 1;
  GetExitCodeProcess(process.hProcess, &exitCode);
  CloseHandle(process.hThread);
  CloseHandle(process.hProcess);
  CloseHandle(readPipe);
  return exitCode;
}

// 拉起 --apply 后立刻返回，不等待（父进程马上就要退出）。
bool SpawnApply() {
  wchar_t pidText[32];
  swprintf(pidText, 32, L"%lu", (unsigned long)GetCurrentProcessId());
  std::wstring args = L"--apply --parent-pid " + std::wstring(pidText);
  std::wstring commandLine = Quote(gState.updaterPath) + L" " + args;

  STARTUPINFOW startup = {};
  startup.cb = sizeof(startup);
  PROCESS_INFORMATION process = {};
  BOOL created = CreateProcessW(nullptr, &commandLine[0], nullptr, nullptr, FALSE, 0,
                                nullptr, gState.baseDir.c_str(), &startup, &process);
  if (created) {
    CloseHandle(process.hThread);
    CloseHandle(process.hProcess);
  }
  return created != FALSE;
}

DWORD WINAPI JobThread(LPVOID parameter) {
  JobMode mode = (JobMode)(INT_PTR)parameter;

  std::wstring arguments;
  if (mode == JOB_CHECK_AUTO) {
    arguments = L"--check --auto";
  } else if (mode == JOB_CHECK_MANUAL) {
    arguments = L"--check";
  } else {
    arguments = L"--download";
  }

  CaptureContext context;
  HWND window = GetParent(gCheck);
  context.hwnd = window;
  RunUpdaterCapture(arguments, &context);

  JobResult* result = new JobResult(context.result);
  result->mode = mode;
  PostMessageW(window, WM_APP_DONE, (WPARAM)mode, (LPARAM)result);
  return 0;
}

// ---------------------------------------------------------------- UI

void SetStatus(const std::wstring& text) {
  SetWindowTextW(gStatus, text.c_str());
}

void SetBusy(bool busy, const wchar_t* hint) {
  gState.busy = busy;
  EnableWindow(gCheck, !busy);
  EnableWindow(gSkip, !busy);
  EnableWindow(gHomepage, !busy);
  if (busy && hint) SetStatus(hint);
}

void ShowProgress(bool visible, int percent) {
  static bool shown = false;
  if (visible != shown) {
    ShowWindow(gProgress, visible ? SW_SHOW : SW_HIDE);
    shown = visible;
  }
  SendMessageW(gProgress, PBM_SETPOS, (WPARAM)(percent < 0 ? 0 : percent), 0);
}

void UpdateVersionFields() {
  SetWindowTextW(gLocalVersion, gState.localVersion.c_str());
}

void BeginJob(JobMode mode, const wchar_t* status) {
  if (gState.busy) return;
  SetBusy(true, status);
  ShowProgress(false, 0);
  HANDLE thread = CreateThread(nullptr, 0, JobThread, (LPVOID)(INT_PTR)mode, 0, nullptr);
  if (thread) CloseHandle(thread);
}

void StartInstall() {
  SetWindowTextW(gInstall, L"立即安装");
  EnableWindow(gInstall, TRUE);
  ShowWindow(gInstall, SW_SHOW);
  ShowWindow(gRestart, SW_HIDE);
  gState.pendingVersion.clear();
  BeginJob(JOB_DOWNLOAD, L"正在下载更新…");
}

void BeginApply(HWND window) {
  if (gState.pendingVersion.empty()) return;
  SetStatus(L"正在重启以完成更新…");
  if (!SpawnApply()) {
    SetStatus(L"拉起更新器失败，请手动重试");
    return;
  }
  // 自己先退出，更新器才能在 90 秒窗口内替换被占用的 exe 并重新拉起新版。
  DestroyWindow(window);
}

void OnJobDone(HWND window, JobResult* result) {
  SetBusy(false, nullptr);
  ShowProgress(false, 0);
  (void)window;

  if (result->state.empty()) {
    SetStatus(L"无法启动更新器 generic-updater.exe，请确认它和本程序在同一目录");
    EnableWindow(gInstall, result->mode == JOB_DOWNLOAD);
    delete result;
    return;
  }

  if (result->state == L"Failed") {
    std::wstring message = L"更新失败：" + (result->error.empty() ? L"未知错误" : result->error);
    SetStatus(message);
    if (result->mode == JOB_DOWNLOAD) {
      EnableWindow(gInstall, TRUE);
      ShowWindow(gInstall, SW_SHOW);
    }
    delete result;
    return;
  }

  if (result->mode == JOB_DOWNLOAD) {
    if (result->state != L"Ready") {
      SetStatus(L"下载未完成，请重试");
      delete result;
      return;
    }
    gState.pendingVersion = result->version;
    SetWindowTextW(gRestart, L"重启软件");
    ShowWindow(gInstall, SW_HIDE);
    ShowWindow(gRestart, SW_SHOW);
    EnableWindow(gRestart, TRUE);
    SetWindowTextW(gUpdateHint, L"（已下载，等待重启）");
    SetStatus(L"更新已就绪，点击「重启软件」完成安装");
    delete result;
    return;
  }

  // 检查结束
  if (result->state == L"Available") {
    gState.availableVersion = result->version;
    SetWindowTextW(gLatestVersion, result->version.c_str());
    SetWindowTextW(gNotes, result->notes.empty() ? L"（该版本没有填写更新日志）" : result->notes.c_str());
    SetWindowTextW(gPackageSize, FormatBytes(result->size).c_str());
    if (result->skipped) {
      // 用户之前点过"跳过"，但依然要显示"有更新"，只是不让他直接安装。
      gState.skipped = true;
      SetWindowTextW(gSkip, L"取消跳过");
      SetWindowTextW(gUpdateHint, L"（有更新 · 已跳过）");
      EnableWindow(gInstall, FALSE);
      SetStatus(L"版本 " + result->version + L" 已被跳过，点「取消跳过」恢复提示");
      delete result;
      return;
    }
    gState.skipped = false;
    SetWindowTextW(gSkip, L"跳过此版本");
    SetWindowTextW(gUpdateHint, L"（有更新）");
    EnableWindow(gInstall, TRUE);
    SetStatus(L"发现新版本 " + result->version);
    delete result;
    return;
  }
  if (result->state == L"Idle") {
    gState.skipped = false;
    gState.availableVersion.clear();
    SetWindowTextW(gSkip, L"跳过此版本");
    EnableWindow(gSkip, FALSE);
    SetWindowTextW(gUpdateHint, L"（已是最新）");
    if (!result->version.empty()) {
      SetWindowTextW(gLatestVersion, result->version.c_str());
    } else {
      SetWindowTextW(gLatestVersion, gState.localVersion.c_str());
    }
    SetWindowTextW(gPackageSize, L"");
    SetWindowTextW(gNotes, L"当前已是最新版本。");
    EnableWindow(gInstall, FALSE);
    ShowWindow(gInstall, SW_SHOW);
    SetStatus(L"当前已是最新版本");
  }
  delete result;
}

void OnProgress(WPARAM percent, LPARAM received) {
  ShowProgress(true, (int)percent);
  std::wstring status = L"正在下载更新… " + std::to_wstring((int)percent) + L"%";
  if (received > 0) status += L"（" + FormatBytes((unsigned long long)received) + L"）";
  SetStatus(status);
}

HWND CreateControl(const wchar_t* klass, const wchar_t* text, DWORD style, int x, int y, int width,
                   int height, int id, HWND parent) {
  HWND control = CreateWindowExW(0, klass, text, WS_CHILD | WS_VISIBLE | style, x, y, width, height,
                                 parent, (HMENU)(INT_PTR)id, GetModuleHandleW(nullptr), nullptr);
  SendMessageW(control, WM_SETFONT, (WPARAM)gFont, TRUE);
  return control;
}

void BuildUi(HWND window) {
  HDC screen = GetDC(nullptr);
  int dpi = GetDeviceCaps(screen, LOGPIXELSY);
  ReleaseDC(nullptr, screen);
  int height = -MulDiv(10, dpi, 72);

  gFont = CreateFontW(height, 0, 0, 0, FW_NORMAL, FALSE, FALSE, FALSE, DEFAULT_CHARSET,
                      OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY,
                      DEFAULT_PITCH | FF_DONTCARE, L"Microsoft YaHei UI");
  gTitleFont = CreateFontW(-MulDiv(12, dpi, 72), 0, 0, 0, FW_BOLD, FALSE, FALSE, FALSE,
                           DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS,
                           CLEARTYPE_QUALITY, DEFAULT_PITCH | FF_DONTCARE, L"Microsoft YaHei UI");

  CreateControl(L"STATIC", L"软件版本：", 0, 24, 26, 100, 22, 0, window);
  gLocalVersion = CreateControl(L"STATIC", L"-", 0, 130, 26, 200, 22, IDC_LOCAL_VERSION, window);
  CreateControl(L"STATIC", L"最新版本：", 0, 24, 56, 100, 22, 0, window);
  gLatestVersion = CreateControl(L"STATIC", L"-", 0, 130, 56, 180, 22, IDC_LATEST_VERSION, window);
  gUpdateHint = CreateControl(L"STATIC", L"", 0, 300, 56, 180, 22, IDC_UPDATE_HINT, window);
  CreateControl(L"STATIC", L"大小：", 0, 452, 56, 50, 22, 0, window);
  gPackageSize = CreateControl(L"STATIC", L"", 0, 496, 56, 120, 22, IDC_PACKAGE_SIZE, window);

  gCheck = CreateControl(L"BUTTON", L"检查更新", BS_PUSHBUTTON, 470, 24, 140, 34, IDC_CHECK, window);

  CreateControl(L"STATIC", L"更新日志", 0, 24, 96, 120, 22, 0, window);
  gNotes = CreateControl(L"EDIT", L"", WS_VSCROLL | ES_MULTILINE | ES_READONLY | ES_AUTOVSCROLL,
                         24, 122, 586, 210, IDC_NOTES, window);

  gProgress = CreateWindowExW(0, PROGRESS_CLASSW, L"", WS_CHILD | PBS_SMOOTH, 24, 344, 586, 16,
                              window, (HMENU)(INT_PTR)IDC_PROGRESS, GetModuleHandleW(nullptr), nullptr);
  SendMessageW(gProgress, PBM_SETRANGE, 0, MAKELPARAM(0, 100));
  ShowWindow(gProgress, SW_HIDE);

  gStatus = CreateControl(L"STATIC", L"", 0, 24, 372, 586, 40, IDC_STATUS, window);

  gBuildInfo = CreateControl(L"STATIC", L"", 0, 24, 470, 586, 20, IDC_BUILD_INFO, window);
  {
    std::wstring info = L"更新器 v" + std::wstring(APP_BUILD_VERSION) + L" · 配置编译期内置 · 元数据强制验签";
    SetWindowTextW(gBuildInfo, info.c_str());
  }

  gSkip = CreateControl(L"BUTTON", L"跳过此版本", BS_PUSHBUTTON, 24, 424, 130, 38, IDC_SKIP, window);
  gHomepage = CreateControl(L"BUTTON", L"官网下载", BS_PUSHBUTTON, 166, 424, 130, 38, IDC_HOMEPAGE, window);
  gInstall = CreateControl(L"BUTTON", L"立即安装", BS_DEFPUSHBUTTON, 470, 424, 140, 38, IDC_INSTALL, window);
  gRestart = CreateControl(L"BUTTON", L"重启软件", BS_DEFPUSHBUTTON, 470, 424, 140, 38, IDC_RESTART, window);
  ShowWindow(gRestart, SW_HIDE);
  EnableWindow(gInstall, FALSE);

  SendMessageW(gNotes, WM_SETFONT, (WPARAM)gFont, TRUE);
}

void LoadLocalVersion() {
  std::wstring text = Trim(ReadTextFile(gState.versionPath));
  if (text.empty()) text = L"0.0.0";
  gState.localVersion = text;
  SetWindowTextW(gLocalVersion, text.c_str());
}

// 跳过状态不落文件：交给更新器的 --skip / --unskip 写进它的 state.json。
void ToggleSkipCurrentVersion() {
  if (gState.availableVersion.empty()) return;
  CaptureContext context;
  context.hwnd = nullptr;
  if (gState.skipped) {
    RunUpdaterCapture(L"--unskip", &context);
    gState.skipped = false;
    SetWindowTextW(gSkip, L"跳过此版本");
    SetStatus(L"已恢复版本 " + gState.availableVersion + L" 的更新提示");
  } else {
    RunUpdaterCapture(L"--skip " + gState.availableVersion, &context);
    gState.skipped = true;
    SetWindowTextW(gSkip, L"取消跳过");
    SetStatus(L"已跳过版本 " + gState.availableVersion);
  }
  // 重新检查一次，让按钮/提示与更新器里的状态保持一致。
  BeginJob(JOB_CHECK_MANUAL, L"正在检查更新…");
}

// 更新完成后，旧镜像 `*.old-*` 可能还被执行中的更新器占用；这里在后台重试删除，
// 避免安装目录里留下文件（不额外拉进程，纯文件操作）。
DWORD WINAPI CleanupOldImages(LPVOID) {
  for (int attempt = 0; attempt < 20; ++attempt) {
    bool found = false;
    WIN32_FIND_DATAW data = {};
    std::wstring pattern = gState.baseDir + L"\\*.old-*";
    HANDLE find = FindFirstFileW(pattern.c_str(), &data);
    if (find != INVALID_HANDLE_VALUE) {
      do {
        found = true;
        std::wstring path = gState.baseDir + L"\\" + data.cFileName;
        DeleteFileW(path.c_str());
      } while (FindNextFileW(find, &data));
      FindClose(find);
    }
    if (!found) break;
    Sleep(300);
  }
  return 0;
}
LRESULT CALLBACK WindowProc(HWND window, UINT message, WPARAM wParam, LPARAM lParam) {
  switch (message) {
    case WM_CREATE:
      BuildUi(window);
      LoadLocalVersion();
      // 启动后先让用户看到界面，按配置延迟再做自动检查（默认 30 秒）。
      SetTimer(window, 1, (UINT)(APP_STARTUP_DELAY_SECONDS * 1000), nullptr);
      return 0;

    case WM_TIMER:
      if (wParam == 1) {
        KillTimer(window, 1);
        BeginJob(JOB_CHECK_AUTO, L"正在检查更新…");
      }
      return 0;

    case WM_COMMAND: {
      int id = LOWORD(wParam);
      if (id == IDC_CHECK) {
        BeginJob(JOB_CHECK_MANUAL, L"正在检查更新…");
      } else if (id == IDC_INSTALL) {
        StartInstall();
      } else if (id == IDC_RESTART) {
        BeginApply(window);
      } else if (id == IDC_SKIP) {
        ToggleSkipCurrentVersion();
      } else if (id == IDC_HOMEPAGE) {
        ShellExecuteW(nullptr, L"open", APP_HOMEPAGE, nullptr, nullptr, SW_SHOWNORMAL);
      }
      return 0;
    }

    case WM_APP_PROGRESS:
      OnProgress(wParam, lParam);
      return 0;

    case WM_APP_DONE: {
      JobResult* result = reinterpret_cast<JobResult*>(lParam);
      if (result) OnJobDone(window, result);
      return 0;
    }

    case WM_CTLCOLORSTATIC: {
      HDC dc = (HDC)wParam;
      SetBkMode(dc, TRANSPARENT);
      return (LRESULT)GetSysColorBrush(COLOR_BTNFACE);
    }

    case WM_CTLCOLOREDIT: {
      HDC dc = (HDC)wParam;
      SetTextColor(dc, GetSysColor(COLOR_WINDOWTEXT));
      SetBkColor(dc, GetSysColor(COLOR_WINDOW));
      return (LRESULT)GetSysColorBrush(COLOR_WINDOW);
    }

    case WM_CLOSE:
      DestroyWindow(window);
      return 0;

    case WM_DESTROY:
      PostQuitMessage(0);
      return 0;

    default:
      return DefWindowProcW(window, message, wParam, lParam);
  }
}

std::wstring ExecutablePath() {
  wchar_t buffer[MAX_PATH * 2] = {};
  DWORD length = GetModuleFileNameW(nullptr, buffer, MAX_PATH * 2);
  return std::wstring(buffer, length);
}





}  // namespace

int WINAPI wWinMain(HINSTANCE instance, HINSTANCE, PWSTR, int showCommand) {
  HANDLE mutex = CreateMutexW(nullptr, TRUE, kMutexName);
  if (mutex && GetLastError() == ERROR_ALREADY_EXISTS) {
    // 更新器重启新版时，旧进程的互斥体可能还没释放完，先重试几秒再判定重复启动。
    CloseHandle(mutex);
    mutex = nullptr;
    for (int attempt = 0; attempt < 40; ++attempt) {
      Sleep(150);
      HANDLE retry = CreateMutexW(nullptr, TRUE, kMutexName);
      if (retry && GetLastError() != ERROR_ALREADY_EXISTS) {
        mutex = retry;
        break;
      }
      if (retry) CloseHandle(retry);
    }
    if (!mutex) {
      MessageBoxW(nullptr, L"程序已经在运行了。", L"Generic Updater Demo", MB_OK | MB_ICONINFORMATION);
      return 0;
    }
  }

  gState.baseDir = DirectoryOf(ExecutablePath());
  gState.updaterPath = gState.baseDir + L"\\generic-updater.exe";
  gState.versionPath = gState.baseDir + L"\\version.txt";



  INITCOMMONCONTROLSEX controls = {};
  controls.dwSize = sizeof(controls);
  controls.dwICC = ICC_PROGRESS_CLASS;
  InitCommonControlsEx(&controls);

  WNDCLASSEXW windowClass = {};
  windowClass.cbSize = sizeof(windowClass);
  windowClass.lpfnWndProc = WindowProc;
  windowClass.hInstance = instance;
  windowClass.hCursor = LoadCursorW(nullptr, IDC_ARROW);
  windowClass.hbrBackground = GetSysColorBrush(COLOR_BTNFACE);
  windowClass.lpszClassName = L"GenericUpdaterDemoWindow";
  windowClass.hIcon = LoadIconW(nullptr, IDI_APPLICATION);
  RegisterClassExW(&windowClass);

  RECT rect = {0, 0, 640, 506};
  AdjustWindowRect(&rect, WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX, FALSE);
  gWindowTitle = std::wstring(APP_WINDOW_TITLE) + L"  ·  v" + APP_BUILD_VERSION;
  HWND window = CreateWindowExW(
      WS_EX_APPWINDOW, L"GenericUpdaterDemoWindow", gWindowTitle.c_str(),
      WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX | WS_CLIPCHILDREN, CW_USEDEFAULT, CW_USEDEFAULT,
      rect.right - rect.left, rect.bottom - rect.top, nullptr, nullptr, instance, nullptr);
  if (!window) return 1;

  ShowWindow(window, showCommand);
  UpdateWindow(window);
  {
    HANDLE thread = CreateThread(nullptr, 0, CleanupOldImages, nullptr, 0, nullptr);
    if (thread) CloseHandle(thread);
  }
  // 先告诉更新器「新版起来了」，它才会退出并释放被改名的旧镜像，
  // 更新器才会退出，把 `*.old-*` 旧镜像释放掉。


  MSG message = {};
  while (GetMessageW(&message, nullptr, 0, 0) > 0) {
    TranslateMessage(&message);
    DispatchMessageW(&message);
  }

  if (mutex) CloseHandle(mutex);
  return (int)message.wParam;
}





