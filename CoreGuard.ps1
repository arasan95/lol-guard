param([int]$GuardSeconds = 0)
# CoreGuard: LoLのメインスレッドとサブスレッドを別々の物理コアへ固定する
# 使い方: 1回だけ実行 / 常駐させる場合は -GuardSeconds 10
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
        return @([CoreTopology]::GetCoreMasks() | Where-Object { $_ -ne 0 })
    } catch { return @() }
}

$LP = [Environment]::ProcessorCount
$coreMasks = @(Get-PhysicalCoreMasks)
if ($coreMasks.Count -lt 2) {
    # API取得に失敗した場合の安全なフォールバック（従来方式）。
    $coreMasks = @([uint64]1 -shl ($LP - 1), [uint64]1 -shl ($LP - 2), [uint64]1 -shl ($LP - 3))
}
$allMask = [uint64]0
foreach ($coreMask in $coreMasks) { $allMask = $allMask -bor [uint64]$coreMask }
$mainMask = [uint64]$coreMasks[$coreMasks.Count - 1]
$subMask = [uint64]$coreMasks[$coreMasks.Count - 2]
$bufferMask = if ($coreMasks.Count -ge 3) { [uint64]$coreMasks[$coreMasks.Count - 3] } else { [uint64]0 }
$reservedMask = $mainMask -bor $subMask -bor $bufferMask
$bgMask = $allMask -bxor $reservedMask
$gameMask = $allMask

$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$stateFile = Join-Path $here 'last_main.txt'
$Exclude = @('Idle','System','Registry','smss','csrss','wininit','winlogon','services','lsass','Secure System','Memory Compression','vgc','MsMpEng','MsMpEngCP','NisSrv','spoolsv')

function Format-Mask([uint64]$mask) { return ('0x{0:X}' -f $mask) }

function Invoke-Sweep {
    $log = @()
    $log += "Logical CPUs: $LP, physical cores: $($coreMasks.Count)"
    $log += "main physical mask: $(Format-Mask $mainMask), sub physical mask: $(Format-Mask $subMask), buffer: $(Format-Mask $bufferMask)"
    $lol = Get-Process -Name 'League of Legends' | Select-Object -First 1
    if ($lol) {
        try { $lol.PriorityClass = 'High' } catch {}
        try { $lol.ProcessorAffinity = [IntPtr]$gameMask } catch {}
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
            try { $top.ProcessorAffinity = [IntPtr]$mainMask; $log += "LoL main TID $($top.Id) -> main physical core $(Format-Mask $mainMask) OK" }
            catch { $log += "LoL main TID $($top.Id) -> FAILED" }
            $sn = 0
            foreach ($th in $lol.Threads) {
                if ($th.Id -eq $top.Id) { continue }
                try { $th.ProcessorAffinity = [IntPtr]$subMask; $sn++ } catch {}
            }
            $log += "LoL sub threads -> separate physical core $(Format-Mask $subMask): $sn pinned"
        }
    } else { $log += 'LoL not running' }

    $n = 0
    foreach ($p in (Get-Process)) {
        if ($Exclude -contains $p.ProcessName) { continue }
        if ($p.ProcessName -eq 'League of Legends') { continue }
        try {
            $aff = [uint64]$p.ProcessorAffinity
            if ($aff -ne $bgMask) { $p.ProcessorAffinity = [IntPtr]$bgMask; $n++ }
        } catch {}
    }
    $log += "moved $n processes off reserved LoL physical cores"
    return $log
}

if ($GuardSeconds -gt 0) {
    while ($true) { Invoke-Sweep | Out-Null; Start-Sleep -Seconds $GuardSeconds }
} else {
    Invoke-Sweep
}
