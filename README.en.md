[日本語](README.md)

# LoL Guard

A Windows tray tool (single Rust exe, no runtime needed) that pins the LoL main thread to a dedicated core and moves other apps to the remaining cores. The UI is English on English Windows, Japanese on Japanese Windows.

## Download (no build needed)

1. Download the latest `lol-guard.exe` from [Releases](../../releases)
2. Put it in any folder and double-click it (no install)
3. It stays in the task tray (a `cores.json` settings file is auto-created on first run)

Building from source is only needed for developers (see below).

## Basic usage

1. Start LoL
2. Press "Optimize now" in the tray menu (or wait for the automatic apply every 45 seconds)
3. The main thread is pinned to its dedicated core, other apps move to the remaining cores
4. When done, "Restore defaults" in the tray menu puts everything back on all CPUs

## What it does

- LoL main thread → pinned to a dedicated core (default: last logical CPU)
- LoL sub threads → pinned to a separate core
- Other apps → moved to the remaining cores (off the game cores)
- While LoL runs, it re-applies every 45 seconds (follows PID/TID changes)
- "Open settings" changes the core assignment; "Register autostart" enables admin residency at logon
- Extra dedication (on by default): main thread priority Highest, moved background processes BelowNormal, High Performance power plan, 1ms system timer
- Optional: Game DVR disable via `game_dvr_off` in the config file

## Build

```sh
cargo build --release --manifest-path tools/lol-guard/Cargo.toml
```

Output: `tools/lol-guard/target/release/lol-guard.exe` (~0.5MB, no runtime needed)

## Notes

- Protected processes (Vanguard, Defender) and System processes cannot be moved due to OS restrictions (failures are ignored)
- Thread affinity is write-only, so "set success = applied"
- LoL PIDs/TIDs change on restart, but the resident guard detects and re-applies
- Machines with over 64 logical CPUs use only the first 64
- Use at your own risk (`--restore` or tray "Restore defaults" puts everything back)
