@echo off
setlocal
set "app=%~dp0lightdriver-fan-debug.exe"
if not exist "%app%" set "app=%~dp0target\release\lightdriver-fan-debug.exe"
if not exist "%app%" (
  echo Build the fan lab first: cargo build -p lightdriver-fan-debug --release --locked
  pause
  exit /b 1
)
start "" "%app%" %*
