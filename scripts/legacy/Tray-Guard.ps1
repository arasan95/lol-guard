param([int]$TestSeconds = 0)
# Tray-Guard: タスクトレイ常駐のLoLコア隔離ガード
# 右クリックメニュー: 状態表示 / ログを開く / 今すぐ最適化 / 終了
$ErrorActionPreference = 'SilentlyContinue'
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$logPath = Join-Path $here 'watch.log'
$script:lastStatus = '待機中'
$script:wasRunning = $false

function Invoke-Guard {
    $lol = Get-Process -Name 'League of Legends' | Select-Object -First 1
    if ($lol) {
        try { & (Join-Path $here 'CoreGuard.ps1') | Out-Null } catch {}
        $script:lastStatus = "適用中 (PID $($lol.Id), $(Get-Date -Format 'HH:mm:ss'))"
        "$(Get-Date -Format 'yyyy-MM-dd HH:mm:ss') applied (PID $($lol.Id))" | Out-File $logPath -Append -Encoding UTF8
        return $true
    }
    $script:lastStatus = "待機中 ($(Get-Date -Format 'HH:mm:ss'))"
    return $false
}

$icon = New-Object Windows.Forms.NotifyIcon
$icon.Icon = [Drawing.SystemIcons]::Shield
$icon.Text = 'LoL CoreGuard'
$icon.Visible = $true

$menu = New-Object Windows.Forms.ContextMenuStrip
$miStatus = $menu.Items.Add('状態を表示')
$miStatus.Add_Click({
    [Windows.Forms.MessageBox]::Show($script:lastStatus, 'LoL CoreGuard 状態')
})
$miSweep = $menu.Items.Add('今すぐ最適化')
$miSweep.Add_Click({
    if (Invoke-Guard) {
        [Windows.Forms.MessageBox]::Show('最適化を適用しました', 'LoL CoreGuard')
    } else {
        [Windows.Forms.MessageBox]::Show('LoLが起動していません', 'LoL CoreGuard')
    }
})
$miRestore = $menu.Items.Add('元に戻す（通常affinity）')
$miRestore.Add_Click({
    $out = & (Join-Path $here 'CoreGuard.ps1') -Restore 2>&1 | Out-String
    $script:lastStatus = "復元済み ($(Get-Date -Format 'HH:mm:ss'))"
    "$(Get-Date -Format 'yyyy-MM-dd HH:mm:ss') restored" | Out-File $logPath -Append -Encoding UTF8
    [Windows.Forms.MessageBox]::Show($out, 'LoL CoreGuard 復元')
})
$miLog = $menu.Items.Add('ログを開く')
$miLog.Add_Click({
    if (-not (Test-Path $logPath)) { '' | Out-File $logPath -Encoding UTF8 }
    Start-Process notepad.exe -ArgumentList "`"$logPath`""
})
$menu.Items.Add('-') | Out-Null
$miExit = $menu.Items.Add('終了')
$miExit.Add_Click({
    $icon.Visible = $false
    [Windows.Forms.Application]::Exit()
})
$icon.ContextMenuStrip = $menu
$icon.Add_DoubleClick({
    [Windows.Forms.MessageBox]::Show($script:lastStatus, 'LoL CoreGuard 状態')
})

$timer = New-Object Windows.Forms.Timer
$timer.Interval = 45000
$script:elapsed = 0
$timer.Add_Tick({
    $running = Invoke-Guard
    if ($running -and -not $script:wasRunning) {
        $icon.ShowBalloonTip(3000, 'LoL CoreGuard', 'LoLを検出: 最適化を適用しました', [Windows.Forms.ToolTipIcon]::Info)
    }
    $script:wasRunning = $running
    if ($TestSeconds -gt 0) {
        $script:elapsed += 45
        if ($script:elapsed -ge $TestSeconds) {
            $icon.Visible = $false
            [Windows.Forms.Application]::Exit()
        }
    }
})
$timer.Start()
Invoke-Guard | Out-Null
[Windows.Forms.Application]::Run()
