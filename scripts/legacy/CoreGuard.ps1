param([int]$GuardSeconds = 0, [switch]$Restore)
# CoreGuard: LoLのメインスレッドを最終論理CPUに専用固定し、サブ・バッファ・他アプリを分離する
# 使い方: 1回だけ実行 / 常駐させる場合は -GuardSeconds 10 / 元に戻す場合は -Restore
$ErrorActionPreference = 'SilentlyContinue'

# 論理CPU番号の並び順には依存せず、Windowsが返す物理コア単位のマスクを使う。
# SMT/HTなら1物理コアに兄弟論理CPUが複数含まれるため、コア全体を予約する。
function Get-PhysicalCoreMasks {
    $type = @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;

public static class CoreTopology {
    [DllImport("kernel32.dll", SetLastError=true)]
    static extern bool GetLogicalProcessorInformationEx(int relationship, IntPtr buffer, ref uint returnedLength);

    public static ulong[] GetCoreMasks() {
        uint length = 0;
        GetLogicalProcessorInformationEx(0, IntPtr.Zero, ref length);
        if (length == 0) return new ulong[0];
        IntPtr buffer = Marshal.AllocHGlobal((int)length);
        try {
            if (!GetLogicalProcessorInformationEx(0, buffer, ref length)) return new ulong[0];
            var result = new List<ulong>();
            int offset = 0;
            while (offset < length) {
                IntPtr item = IntPtr.Add(buffer, offset);
                int size = Marshal.ReadInt32(item, 4);
                if (size < 32) break;
                // PROCESSOR_RELATIONSHIP.GroupMask[0].Mask
                result.Add((ulong)Marshal.ReadInt64(item, 32));
                offset += size;
            }
            return result.ToArray();
        } finally { Marshal.FreeHGlobal(buffer); }
    }
}
'@
    try {
        if (-not ('CoreTopology' -as [type])) { Add-Type -TypeDefinition $type }
        $raw = [CoreTopology]::GetCoreMasks()
        $out = @()
        foreach ($m in $raw) {
            if ([uint64]$m -ne 0) { $out += [uint64]$m }
        }
        return $out
    } catch { return @() }
}

$LP = [Environment]::ProcessorCount
$coreMasks = @(Get-PhysicalCoreMasks)
if ($coreMasks.Count -lt 2) {
    # API取得に失敗した場合の安全なフォールバック（従来方式）。
    $coreMasks = @([uint64]1 -shl ($LP - 1), [uint64]1 -shl ($LP - 2), [uint64]1 -shl ($LP - 3))
}
$allMask = ([uint64]1 -shl $LP) - 1
if ($LP -ge 5) {
    # 論理CPU単位の割当（8スレッドなら: 7=メイン専用, 6=バッファ, 5=サブ, 4=バッファ, 0-3=他アプリ）。
    $mainMask = [uint64]1 -shl ($LP - 1)
    $bufHiMask = [uint64]1 -shl ($LP - 2)
    $subMask = [uint64]1 -shl ($LP - 3)
    $bufLoMask = [uint64]1 -shl ($LP - 4)
    $bufferMask = $bufHiMask -bor $bufLoMask
} else {
    # 5論理CPU未満のフォールバック（物理コア単位）。
    $mainMask = [uint64]$coreMasks[$coreMasks.Count - 1]
    $subMask = [uint64]$coreMasks[$coreMasks.Count - 2]
    $bufferMask = if ($coreMasks.Count -ge 3) { [uint64]$coreMasks[$coreMasks.Count - 3] } else { [uint64]0 }
    $bufHiMask = [uint64]0
    $bufLoMask = $bufferMask
}
$reservedMask = $mainMask -bor $subMask -bor $bufferMask
$bgMask = $allMask -bxor $reservedMask
$gameMask = $allMask

$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$stateFile = Join-Path $here 'last_main.txt'
$Exclude = @('Idle','System','Registry','Memory Compression','Secure System')
# 上記以外は隔離を試みる（管理者実行ならservices/lsass/Defender等も対象。失敗は無視される）。

function Format-Mask([uint64]$mask) { return ('0x{0:X}' -f $mask) }

function Invoke-Sweep {
    $log = @()
    $log += "Logical CPUs: $LP, physical cores: $($coreMasks.Count)"
    $log += "main mask: $(Format-Mask $mainMask), sub mask: $(Format-Mask $subMask), buffer: $(Format-Mask $bufferMask), bg: $(Format-Mask $bgMask)"
    if ($Restore) {
        $log += "restore mode: everything back to $(Format-Mask $allMask)"
        try { if (Test-Path $stateFile) { Remove-Item $stateFile -Force } } catch { }
        $rlol = Get-Process -Name 'League of Legends' | Select-Object -First 1
        if ($rlol) {
            try { $rlol.PriorityClass = 'Normal' } catch { }
            try { $rlol.ProcessorAffinity = [IntPtr][int64]$allMask } catch { }
            $rn = 0
            foreach ($th in $rlol.Threads) {
                try { $th.ProcessorAffinity = [IntPtr][int64]$allMask; $rn++ } catch { }
            }
            $log += "LoL reset to all CPUs: $rn threads, priority Normal"
        } else { $log += 'LoL not running' }
        $n = 0
        foreach ($p in (Get-Process)) {
            if ($Exclude -contains $p.ProcessName) { continue }
            try {
                $aff = [uint64]$p.ProcessorAffinity.ToInt64()
                if ($aff -ne $allMask) { $p.ProcessorAffinity = [IntPtr][int64]$allMask; $n++ }
            } catch { }
        }
        $log += "restored $n processes to all CPUs"
        return $log
    }
    $lol = Get-Process -Name 'League of Legends' | Select-Object -First 1
    if ($lol) {
        try { $lol.PriorityClass = 'High' } catch {}
        try { $lol.ProcessorAffinity = [IntPtr][int64]$gameMask } catch {}
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
            # Refresh後のThreadsは新しいオブジェクトのためIDで取り直す（古いハンドルだと設定が失敗する）。
            $topFresh = $lol.Threads | Where-Object { $_.Id -eq $top.Id } | Select-Object -First 1
            if ($topFresh) { $top = $topFresh }
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
            try { $top.ProcessorAffinity = [IntPtr][int64]$mainMask; $log += "LoL main TID $($top.Id) -> main physical core $(Format-Mask $mainMask) OK" }
            catch { $log += "LoL main TID $($top.Id) -> FAILED" }
            $sn = 0
            foreach ($th in $lol.Threads) {
                if ($th.Id -eq $top.Id) { continue }
                try { $th.ProcessorAffinity = [IntPtr][int64]$subMask; $sn++ } catch {}
            }
            $log += "LoL sub threads -> separate physical core $(Format-Mask $subMask): $sn pinned"
        }
    } else { $log += 'LoL not running' }

    $n = 0
    foreach ($p in (Get-Process)) {
        if ($Exclude -contains $p.ProcessName) { continue }
        if ($p.ProcessName -eq 'League of Legends') { continue }
        try {
            $aff = [uint64]$p.ProcessorAffinity.ToInt64()
            if ($aff -ne $bgMask) { $p.ProcessorAffinity = [IntPtr][int64]$bgMask; $n++ }
        } catch {}
    }
    $log += "moved $n processes off reserved LoL physical cores"
    return $log
}

if ($GuardSeconds -gt 0 -and -not $Restore) {
    while ($true) { Invoke-Sweep | Out-Null; Start-Sleep -Seconds $GuardSeconds }
} else {
    Invoke-Sweep
}
