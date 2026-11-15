@echo off
rem Build the Win32 GUI client with MSVC.
rem Configuration is compiled in from ..\updater.conf (no runtime ini).
rem Set MSVC_SETUP to a vcvars/setup script to prepare the compiler environment, e.g.
rem   set MSVC_SETUP=E:\NuitkaGUI\msvc\setup_x64.bat
setlocal
set ROOT=%~dp0..
if not exist "%ROOT%\dist" mkdir "%ROOT%\dist"

powershell -NoProfile -ExecutionPolicy Bypass -File "%ROOT%\scripts\gen-app-config.ps1" -ConfPath "%ROOT%\updater.conf" -OutPath "%ROOT%\client\build-config.h"
if errorlevel 1 exit /b 1

if defined MSVC_SETUP (
  echo [build] calling %MSVC_SETUP%
  call "%MSVC_SETUP%"
)

cl /nologo /O2 /EHsc /utf-8 /W3 /DUNICODE /D_UNICODE ^
  "%ROOT%\client\src\gui.cpp" ^
  /Fo:"%ROOT%\dist\\" /Fe:"%ROOT%\dist\GuiUpdaterDemo.exe" ^
  /link /SUBSYSTEM:WINDOWS user32.lib gdi32.lib comctl32.lib shell32.lib
if errorlevel 1 exit /b 1
echo [build] ok: %ROOT%\dist\GuiUpdaterDemo.exe
