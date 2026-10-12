# LoL Guard

LoLのメインスレッドを専用コアに固定し、他アプリを残りのコアへ隔離するWindows常駐ツール（Rust製・単一exe）。

## 使い方

1. Releasesから `lol-guard.exe` を取得する
2. 実行するとタスクトレイに常駐する（初回は `cores.json` を自動生成）
3. LoL起動中は45秒ごとに自動適用される

トレイメニュー：状態を表示／今すぐ最適化／元に戻す／設定を開く（コア選択）／自動起動を登録（管理者）／ログを開く／終了

## ビルド

```sh
cargo build --release --manifest-path tools/lol-guard/Cargo.toml
```

成果物：`tools/lol-guard/target/release/lol-guard.exe`（約0.5MB、ランタイム不要）

## 注意

- Vanguard・Defender等の保護プロセスやSystem系はOS制約で移動できない（失敗は無視される）
- スレッドaffinityは読取不可のため「設定成功＝適用」とみなす
- LoL再起動でPID/TIDは変わるが、常駐が検出して再適用する
- 64論理CPU超の環境は先頭64のみ対象
- 使用は自己責任で（`--restore`／トレイ「元に戻す」で全CPUに戻せる）
