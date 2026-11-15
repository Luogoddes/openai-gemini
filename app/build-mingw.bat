@echo off
rem Build the Win32 GUI client with MinGW-w64 g++.
rem Configuration is compiled in from ..\updater.conf (no runtime ini).
rem Note: build from the repo root and use relative output paths; some older MinGW
rem linkers fail on absolute paths that contain non-ASCII characters.
setlocal
set ROOT=%~dp0..
cd /d "%ROOT%"
if not exist "dist" mkdir "dist"

powershell -NoProfile -ExecutionPolicy Bypass -File "%ROOT%\tools\gen-app-config.ps1" -ConfPath "%ROOT%\updater.conf" -OutPath "%ROOT%\app\build-config.h"
if errorlevel 1 exit /b 1

g++ -O2 -std=c++17 -municode -mwindows -DUNICODE -D_UNICODE ^
  -o dist\GuiUpdaterDemo.exe ^
  app\src\gui.cpp ^
  -lcomctl32 -lshell32 -luser32 -lgdi32
if errorlevel 1 exit /b 1
echo [build] ok: %ROOT%\dist\GuiUpdaterDemo.exe
