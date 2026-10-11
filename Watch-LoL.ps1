param([int]$IntervalSeconds = 45)
# Watch-LoL: LoL起動中は45秒ごとにCoreGuardを適用(メインスレッド固定+裏アプリ隔離)
$ErrorActionPreference = 'SilentlyContinue'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$log = Join-Path $here 'watch.log'
while ($true) {
    $lol = Get-Process -Name 'League of Legends' | Select-Object -First 1
    if ($lol) {
        try { & (Join-Path $here 'CoreGuard.ps1') | Out-Null } catch {}
        "$(Get-Date -Format 'yyyy-MM-dd HH:mm:ss') applied (PID $($lol.Id))" | Out-File $log -Append -Encoding UTF8
    }
    Start-Sleep -Seconds $IntervalSeconds
}
