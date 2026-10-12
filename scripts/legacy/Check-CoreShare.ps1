param()
# Check-CoreShare: LoLメインスレッドの使用率・使用コア・同居アプリを一発表示
$ErrorActionPreference = 'SilentlyContinue'
function Mask-ToCores([int64]$mask) {
    $c = @()
    for ($i = 0; $i -lt 16; $i++) { if (($mask -band ([int64]1 -shl $i)) -ne 0) { $c += $i } }
    return $c
}
$lol = Get-Process -Name 'League of Legends' | Select-Object -First 1
if (-not $lol) { Write-Host 'LoLが起動していません'; pause; exit }
$LP = [Environment]::ProcessorCount
$t1 = @{}
foreach ($th in $lol.Threads) { $t1[$th.Id] = $th.TotalProcessorTime }
Start-Sleep -Seconds 3
$lol.Refresh()
$top = $null; $topV = -1
foreach ($th in $lol.Threads) {
    if ($t1.ContainsKey($th.Id)) {
        $v = ($th.TotalProcessorTime - $t1[$th.Id]).TotalMilliseconds / 30
        if ($v -gt $topV) { $topV = $v; $top = $th }
    }
}
$mask = 0
try { $mask = [int64]$top.ProcessorAffinity.ToInt64() } catch { $mask = [int64]$lol.ProcessorAffinity.ToInt64() }
$cores = Mask-ToCores $mask
Write-Host ''
Write-Host ('メインスレッド: TID {0} = {1}% (1コア=100%)' -f $top.Id, [math]::Round($topV,1))
Write-Host ('現在のAffinity: {0} (論理CPU)' -f ($cores -join ', '))
Write-Host '※この診断ではAffinityを変更しません。固定はCoreGuardが行います。'
Write-Host ''
Write-Host '--- 同じコアを使えるアプリ(負荷順TOP10, 5秒計測) ---'
$s1 = @{}
foreach ($p in (Get-Process)) { try { $s1[$p.Id] = $p.CPU } catch {} }
Start-Sleep -Seconds 5
$rows = foreach ($p in (Get-Process)) {
    try {
        $a = [int64]$p.ProcessorAffinity.ToInt64()
        $share = $false
        foreach ($c in $cores) { if (($a -band ([int64]1 -shl $c)) -ne 0) { $share = $true; break } }
        if ($share -and $s1.ContainsKey($p.Id)) {
            $d = [math]::Round(($p.CPU - $s1[$p.Id]) / 5 / $LP * 100, 2)
            if ($d -ge 0.1) { [pscustomobject]@{Proc = $p.ProcessName; PID = $p.Id; Load = $d; Mask = $a }
            }
        }
    } catch {}
}
$rows | Sort-Object Load -Descending | Select-Object -First 10 | Format-Table -AutoSize
Write-Host ('※ Loadは全体CPUに対する割合。{0}%=1コア分。' -f [math]::Round(100 / $LP, 1))
pause
