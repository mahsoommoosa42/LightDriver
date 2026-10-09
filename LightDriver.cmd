@echo off
setlocal
set "app=%~dp0lightdriver-desktop.exe"
if not exist "%app%" set "app=%~dp0target\release\lightdriver-desktop.exe"
if not exist "%app%" (
  echo Build LightDriver first: cargo build --workspace --release --locked
  pause
  exit /b 1
)
start "" "%app%" %*
