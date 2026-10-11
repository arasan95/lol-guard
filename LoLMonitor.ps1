param([int]$Seconds = 0)
# LoL CPUモニター: メインスレッド%(1コア換算) + コア別使用率を1秒ごとに表示
$ErrorActionPreference = 'SilentlyContinue'
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing

$form = New-Object Windows.Forms.Form
$form.Text = 'LoL CPUモニター (1秒更新)'
$NCORE = [Environment]::ProcessorCount
$form.Size = New-Object Drawing.Size(400, (200 + $NCORE * 25))
$form.TopMost = $true
$form.StartPosition = 'CenterScreen'

$y = 10
$lblTotal = New-Object Windows.Forms.Label
$lblTotal.Location = New-Object Drawing.Point(10, $y); $lblTotal.Size = New-Object Drawing.Size(360, 20)
$lblTotal.Font = New-Object Drawing.Font('Meiryo', 10)
$form.Controls.Add($lblTotal)
$y += 25

$lblMain = New-Object Windows.Forms.Label
$lblMain.Location = New-Object Drawing.Point(10, $y); $lblMain.Size = New-Object Drawing.Size(360, 40)
$lblMain.Font = New-Object Drawing.Font('Meiryo', 14, [Drawing.FontStyle]::Bold)
$lblMain.ForeColor = [Drawing.Color]::Red
$form.Controls.Add($lblMain)
$y += 45

$barMain = New-Object Windows.Forms.ProgressBar
$barMain.Location = New-Object Drawing.Point(10, $y); $barMain.Size = New-Object Drawing.Size(360, 20)
$barMain.Maximum = 100
$form.Controls.Add($barMain)
$y += 30

$lblSub = New-Object Windows.Forms.Label
$lblSub.Location = New-Object Drawing.Point(10, $y); $lblSub.Size = New-Object Drawing.Size(360, 40)
$lblSub.Font = New-Object Drawing.Font('Meiryo', 9)
$form.Controls.Add($lblSub)
$y += 45

$coreBars = @()
$coreLbls = @()
    for ($i = 0; $i -lt $NCORE; $i++) {
    $lb = New-Object Windows.Forms.Label
    $lb.Location = New-Object Drawing.Point(10, $y); $lb.Size = New-Object Drawing.Size(70, 20)
    $lb.Text = "Core $i"
    $form.Controls.Add($lb)
    $bar = New-Object Windows.Forms.ProgressBar
    $bar.Location = New-Object Drawing.Point(80, $y); $bar.Size = New-Object Drawing.Size(220, 20)
    $bar.Maximum = 100
    $form.Controls.Add($bar)
    $vl = New-Object Windows.Forms.Label
    $vl.Location = New-Object Drawing.Point(305, $y); $vl.Size = New-Object Drawing.Size(65, 20)
    $form.Controls.Add($vl)
    $coreBars += $bar; $coreLbls += $vl
    $y += 25
}

$lblNote = New-Object Windows.Forms.Label
$lblNote.Location = New-Object Drawing.Point(10, $y); $lblNote.Size = New-Object Drawing.Size(360, 30)
$lblNote.Font = New-Object Drawing.Font('Meiryo', 8)
$lblNote.ForeColor = [Drawing.Color]::Gray
$lblNote.Text = 'メイン=LoL最大スレッド%(1コア=100%)。100%近い=単スレッド飽和。'
$form.Controls.Add($lblNote)

$prev = @{}
$elapsed = 0
$timer = New-Object Windows.Forms.Timer
$timer.Interval = 1000
$timer.Add_Tick({
    $script:elapsed++
    $cs = (Get-Counter '\Processor(*)\% Processor Time' -MaxSamples 1).CounterSamples
    $tot = ($cs | Where-Object { $_.Path -match '\(_Total\)' } | Select-Object -First 1).CookedValue
    $lblTotal.Text = "CPU全体: $([math]::Round($tot,1))%"
for ($i = 0; $i -lt $NCORE; $i++) {
        $v = ($cs | Where-Object { $_.Path -match "\($i\)" } | Select-Object -First 1).CookedValue
        $v = [math]::Max(0, [math]::Min(100, [math]::Round($v)))
        $coreBars[$i].Value = $v
        $coreLbls[$i].Text = "$v%"
    }
    $lol = Get-Process -Name 'League of Legends' | Select-Object -First 1
    if ($lol) {
        $topTid = 0; $topPct = 0; $secondPct = 0
        foreach ($th in $lol.Threads) {
            $cur = $th.TotalProcessorTime
            if ($prev.ContainsKey($th.Id)) {
                $pct = ($cur - $prev[$th.Id]).TotalMilliseconds / 10
                if ($pct -gt $topPct) { $secondPct = $topPct; $topPct = $pct; $topTid = $th.Id }
                elseif ($pct -gt $secondPct) { $secondPct = $pct }
            }
            $prev[$th.Id] = $cur
        }
        $topPct = [math]::Max(0, [math]::Min(999, [math]::Round($topPct, 1)))
        $lblMain.Text = "メイン(TID $topTid): $topPct%"
        $barMain.Value = [math]::Min(100, [int]$topPct)
        $lblSub.Text = "2番手: $([math]::Round($secondPct,1))% / LoL合計: $([math]::Round($lol.CPU,0))s"
    } else {
        $lblMain.Text = 'LoLが起動していません'
        $barMain.Value = 0
        $lblSub.Text = ''
        $script:prev = @{}
    }
    if ($Seconds -gt 0 -and $script:elapsed -ge $Seconds) { $form.Close() }
})
$form.Add_Shown({ $timer.Start() })
[void]$form.ShowDialog()
