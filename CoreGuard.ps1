param([int]$GuardSeconds = 0)
# CoreGuard: 末尾2コアをLoL用に確保し、他のプロセスを残りのコアへ寄せる(PC汎用版)
# 使い方: 1回だけ実行 / 常駐させる場合は -GuardSeconds 10
$ErrorActionPreference = 'SilentlyContinue'
$LP = [Environment]::ProcessorCount
if ($LP -lt 4) { $LP = 4 }
$gMain = $LP - 1; $gSub = $LP - 3; $gBuf = $LP - 2  # メイン / サブ / バッファ(HT兄弟のため空ける)
$MAIN_MASK = ([int64]1 -shl $gMain)   # メインスレッド -> 末尾コア
$SUB_MASK = ([int64]1 -shl $gSub)     # 他LoLスレッド -> 末尾3番目(固定=フラップ防止)
$ALL_MASK = ([int64]1 -shl $LP) - 1
$BG_MASK = $ALL_MASK -bxor ($MAIN_MASK -bor $SUB_MASK -bor ([int64]1 -shl $gBuf))  # 裏アプリ -> 残り
$GAME_MASK = $ALL_MASK
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$stateFile = Join-Path $here 'last_main.txt'
$Exclude = @('Idle','System','Registry','smss','csrss','wininit','winlogon','services','lsass','Secure System','Memory Compression','vgc','MsMpEng','MsMpEngCP','NisSrv','spoolsv')

function Invoke-Sweep {
    $log = @()
    $log += "Logical CPUs: $LP, main core: $gMain, sub core: $gSub, buffer: $gBuf"
    # 1) LoLメインスレッド(最繁忙)のみ末尾コアに固定。サブは末尾3番目に固定
    $lol = Get-Process -Name 'League of Legends' | Select-Object -First 1
    if ($lol) {
        try { $lol.PriorityClass = 'High' } catch {}
        try { $lol.ProcessorAffinity = [IntPtr]$GAME_MASK } catch {}
        $t1 = @{}
        foreach ($th in $lol.Threads) { $t1[$th.Id] = $th.TotalProcessorTime }
        Start-Sleep -Seconds 2
        $lol.Refresh()
        $top = $null; $topV = -1
        foreach ($th in $lol.Threads) {
            if ($t1.ContainsKey($th.Id)) {
                $v = ($th.TotalProcessorTime - $t1[$th.Id]).TotalMilliseconds
                if ($v -gt $topV) { $topV = $v; $top = $th }
            }
        }
        if ($top) {
            try {
                $prevTid = 0
                if (Test-Path $stateFile) { $prevTid = [int](Get-Content $stateFile -TotalCount 1) }
                $prev = $lol.Threads | Where-Object { $_.Id -eq $prevTid }
                if ($prev -and $t1.ContainsKey($prevTid)) {
                    $prevLoad = ($prev.TotalProcessorTime - $t1[$prevTid]).TotalMilliseconds
                    $topLoad = ($top.TotalProcessorTime - $t1[$top.Id]).TotalMilliseconds
                    if ($prevLoad -gt 800 -and $prevLoad -ge ($topLoad * 0.5)) { $top = $prev }
                }
                "$($top.Id)" | Set-Content $stateFile
            } catch {}
            try { $top.ProcessorAffinity = [IntPtr]$MAIN_MASK; $log += "LoL main TID $($top.Id) -> core $gMain OK" }
            catch { $log += "LoL main TID $($top.Id) -> FAILED" }
            $sn = 0
            foreach ($th in $lol.Threads) {
                if ($th.Id -eq $top.Id) { continue }
                try { $th.ProcessorAffinity = [IntPtr]$SUB_MASK; $sn++ } catch {}
            }
            $log += "LoL sub threads -> core $gSub : $sn pinned"
        }
    } else { $log += 'LoL not running' }
    # 2) 裏アプリをゲーム用コア以外へ
    $n = 0
    foreach ($p in (Get-Process)) {
        if ($Exclude -contains $p.ProcessName) { continue }
        if ($p.ProcessName -eq 'League of Legends') { continue }
        try {
            $aff = [int64]$p.ProcessorAffinity
            if ($aff -ne $BG_MASK) { $p.ProcessorAffinity = [IntPtr]$BG_MASK; $n++ }
        } catch {}
    }
    $log += "moved $n processes off cores $gSub,$gBuf,$gMain"
    return $log
}

if ($GuardSeconds -gt 0) {
    while ($true) { Invoke-Sweep | Out-Null; Start-Sleep -Seconds $GuardSeconds }
} else {
    Invoke-Sweep
}
