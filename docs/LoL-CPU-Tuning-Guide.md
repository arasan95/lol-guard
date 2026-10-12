# LoL CPUチューニングガイド：単スレッド飽和の特定とコア隔離

タスクマネージャーではCPUに余裕があるのにLoLのFPSだけが落ちる場合の、原因特定と対処の実践手順書。
本リポジトリのツール（BAT/PS1）に対応している。

## 0. 要約

- LoLのFPS低下はGPUではなく**CPU単スレッド飽和**が原因の場合が多い。
- 対策は「メインスレッドの要求量を下げる」＋「使うコアを専用化する」の二本立て。
- LoLのPID・TIDは起動ごとに変わりaffinityはリセットされるため、隔離は自動化が必須。

## 1. ボトルネックはCPU・シングルスレッド性能に依存する

- 全体CPU 60%でも、特定スレッド1本が1コア換算99%なら**単スレッド飽和**。コア増設では直らない。
- 目安：8スレッドCPUでLoL全体が12%前後＝1コア飽和のサイン（100÷8）。
- 効くのはコア数ではなく単スレッド性能（クロック×IPC）。Eコアの数よりPコアの高クロックが重要。

## 2. コア共有が性能を出し切れない理由

- OSスケジューラは負荷分散のためスレッドを全コアに回す。実測ではメインスレッドが8コア全てを渡り歩いていた。
- 同居の実測例（ETW・12.5秒・core 7）：メイン15.7%＋MsMpEng 13.7%＋LeagueClient 10.1%。メインが最も長くいたコアを他プロセスと分け合っていた。
- 対策＝メイン専用コア化。本環境（8論理CPU）の割当：

| 論理CPU | 用途 |
|---|---|
| 7 | LoLメイン専用 |
| 6・4 | バッファ（誰も使わない） |
| 5 | LoLサブ |
| 0-3 | 他アプリ |

## 3. メインスレッド使用率の調べ方

- Process Explorer：`League of Legends.exe`→Threadsタブ→CPUソート。常に上位の1本が犯人（表示は全コア合計比のため8コアPCでは約12.5%が1コア分）。
- パフォーマンスモニター：`Thread`→`% Processor Time`。1コア=100%表示で直感的。
- TID対応付け：`Get-Counter '\Thread(League of Legends*)\ID Thread'` でカウンタ名とTIDを対応付ける。
- リポジトリツール：
  - `Check-CoreShare.bat`：メインTID・Affinity・同居負荷TOP10（変更なし）。
  - `Measure-HotCore.ps1`：最熱コア＋そのコアを使えるプロセスを名指し。
  - `Measure-Contention.ps1`：待ち行列・コンテキスト切替回数で競合判定。
  - `Capture-Contention.bat`＋`Analyze-CSwitch.exe`：ETW確定計測（要管理者、6章・8章参照）。

## 4. HWiNFOでの確認方法

- 見る項目：コア別Effective Clock（要求に対する実クロック）、コア別使用率、CPU Package温度、サーマルスロットリング有無、電力制限（PPT/PL1・PL2）。
- 判定：クロックが落ちていれば温度・電力が先。クロック維持でFPS低下ならスレッド飽和→本書の隔離へ。

## 5. 移動用BATと自動化

LoLは起動ごとにPID・TIDが変わりaffinityはリセットされるため、1回設定して終わりにはできない。

- 単発：`CoreGuard.bat`（通常）、`CoreGuard-Admin.bat`（管理者・UACあり）。
- 簡易起動：`LoL-Tune.bat`（1〜8の番号メニュー）。
- 常駐：`Watch-LoL.ps1`（45秒ごとに適用）、`Tray-Guard.ps1`（トレイ常駐・右クリック操作・復元付き）。
- 自動実行：`Setup.bat`（管理者のログオンタスク `LoLCoreGuard` を登録。旧スタートアップ方式は削除される）。
- 復元：`CoreGuard.ps1 -Restore`（全て全CPUに戻す。トレイの「元に戻す」も同じ）。
- 常駐はWatch-LoLとTray-Guardのどちらか一方にすること（二重適用は無駄）。

## 6. 管理者でも移動できないもの

- 移動不可：System/Idle等のカーネル系、Defender（MsMpEng・保護プロセス）、Vanguard（vgc）、csrss等。設定は無視される（失敗しても処理は止まらない設計）。
- 実測：MsMpEngが1コア超（102.6%）を消費してもaffinity変更不可。対処はプロセス移動ではなく**Defender除外（`C:\Riot Games`フォルダ）**。
- 除外リストはカーネル系5件のみに縮小済み（`Idle,System,Registry,Memory Compression,Secure System`）。services/lsass等は管理者実行時に隔離を試みる。

## 7. CPU差異の注意点

- コア数可変への対応：論理CPU数を検出し末尾基準で相対割当。5論理CPU未満は物理コア方式にフォールバック。
- Eコア/Pコア注意：効率コアにメインが載ると遅い。末尾コアの種別を確認すること。
- SMT/HT：兄弟論理CPUは実行資源を共有するため、バッファを挟んで干渉を避ける設計。
- ノートPCの電力制限・X3D等特殊構成は、先にHWiNFO（4章）で確認する。

## 8. トラブルシュート（実例）

- PS5互換性：(1) BOMなしUTF-8の日本語コメント誤読→BOM付与、(2) `uint64⇔IntPtr`直接キャスト不可→`.ToInt64()`・`[IntPtr][int64]`、(3) `Refresh()`後の古いスレッドハンドル→IDで取り直し。
- BATはCRLF必須。LFのみだとcmd.exeが解釈できず黒画面で終了する。
- UAC・セッション問題：BAT経由の自動昇格が見えない場合は、スタート→powershell右クリック→管理者として実行→`powershell -NoProfile -ExecutionPolicy Bypass -File "C:\...\CoreGuard.ps1"`。
- ETW確定手順：試合中の重い場面で`Capture-Contention.bat`（12秒）→`Analyze-CSwitch.exe <etl> <TID>`で「何番コアに何%・同居は何か」を実測する。
- 解析器の再ビルド：TraceEvent 2.0.76（net45）＋System.Runtime.CompilerServices.Unsafe 4.5.3（net461）のDLLを同フォルダに置き、`C:\Windows\Microsoft.NET\Framework64\v4.0.30319\csc.exe /reference:TraceEvent.dll /out:Analyze-CSwitch.exe Analyzer.cs` でコンパイル。

## 付録：ファイル対応表

| ファイル | 役割 |
|---|---|
| CoreGuard.ps1 / .bat | 隔離本体（単発・常駐から呼ぶ） |
| CoreGuard-Admin.bat | 管理者で隔離を1回実行 |
| Check-CoreShare.ps1 / .bat | 診断（変更なし） |
| Measure-HotCore.ps1 | 最熱コア＋容疑者名指し |
| Measure-Contention.ps1 | 待ち行列・切替回数で判定 |
| Capture-Contention.ps1 / .bat | ETW記録（要管理者） |
| Analyze-CSwitch.exe | ETL解析（スレッド×CPU占有率。`%TEMP%\opencode\cswitch\`配下。再ビルドは8章のcsc手順） |
| Watch-LoL.ps1 | 45秒ごと自動適用 |
| Tray-Guard.ps1 | トレイ常駐（最適化・復元・ログ） |
| Tray-Guard-Admin.bat | トレイ常駐の管理者起動 |
| Setup.bat / Setup-Task.ps1 | 管理者ログオンタスク登録 |
| LoL-Tune.bat | 番号メニュー起動 |
| tools/Analyze-CSwitch.cs | ETL解析器のソース（再ビルド用） |
| last_main.txt / watch.log | メインTID引継ぎ・適用ログ（自動生成） |
