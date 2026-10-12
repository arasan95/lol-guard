@echo off
REM LoLCoreGuard setup: register logon task with highest privileges (admin required, self-elevates).
net session >nul 2>&1
if %errorlevel% neq 0 (
  echo Requesting admin rights...
  powershell -NoProfile -ExecutionPolicy Bypass -Command "Start-Process -FilePath '%~f0' -Verb RunAs"
  exit /b
)
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0Setup-Task.ps1"
pause
