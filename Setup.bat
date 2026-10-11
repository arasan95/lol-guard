@echo off
REM LoLCoreGuard セットアップ: スタートアップに監視を登録(管理者権限不要)
set SCRIPT_DIR=%~dp0
powershell -NoProfile -ExecutionPolicy Bypass -Command "$ws = New-Object -ComObject WScript.Shell; $lnk = $ws.CreateShortcut(\"$env:APPDATA\Microsoft\Windows\Start Menu\Programs\Startup\LoLCoreGuard.lnk\"); $lnk.TargetPath = \"$env:SystemRoot\System32\WindowsPowerShell\v1.0\powershell.exe\"; $lnk.Arguments = '-NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File \""%SCRIPT_DIR%Watch-LoL.ps1\""'; $lnk.WindowStyle = 7; $lnk.WorkingDirectory = \"%SCRIPT_DIR%\"; $lnk.Save(); Write-Host 'startup registered'"
pause
