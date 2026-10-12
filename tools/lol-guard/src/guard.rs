//! Sweep logic: pin LoL threads, isolate background apps, restore.
use crate::config::{all_mask, fmt_mask, CoresConfig};
use crate::engine;

/// Processes we never touch.
const EXCLUDE: &[&str] = &[
    "System",
    "Registry",
    "Secure System",
    "Memory Compression",
    "",
];

const LOL_EXE: &str = "League of Legends.exe";
const HIGH_PERF_GUID: &str = "8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c";

fn excluded(name: &str) -> bool {
    EXCLUDE.iter().any(|e| e.eq_ignore_ascii_case(name))
}

fn active_scheme() -> Option<String> {
    let out = engine::silent_cmd("powercfg").arg("/GETACTIVESCHEME").output().ok()?;
    let s = String::from_utf8_lossy(&out.stdout);
    s.split_whitespace()
        .find(|t| t.len() == 36 && t.chars().nth(8) == Some('-'))
        .map(|t| t.to_lowercase())
}

/// Switch to High Performance while LoL runs (records previous scheme).
/// Runs at most once per session to avoid spawning processes mid-game.
fn ensure_high_perf(cfg: &mut CoresConfig) -> Option<String> {
    if !cfg.power_plan || cfg.power_done {
        return None;
    }
    let cur = active_scheme().unwrap_or_default();
    if cur.eq_ignore_ascii_case(HIGH_PERF_GUID) {
        return None;
    }
    if cfg.power_scheme.is_none() {
        cfg.power_scheme = Some(cur);
    }
    match engine::silent_cmd("powercfg")
        .args(["/S", HIGH_PERF_GUID])
        .output()
    {
        Ok(o) if o.status.success() => {
            cfg.power_done = true;
            Some("power plan -> High Performance".to_string())
        }
        _ => Some("power plan change FAILED".to_string()),
    }
}

/// Disable Game DVR / Game Bar capture (HKCU, reversible via restore UI).
/// Runs at most once per session.
fn ensure_dvr_off(cfg: &mut CoresConfig) -> Option<String> {
    if cfg.dvr_done {
        return None;
    }
    cfg.dvr_done = true;
    let keys = [
        (r"HKCU\System\GameConfigStore", "GameDVR_Enabled"),
        (
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\GameDVR",
            "AppCaptureEnabled",
        ),
    ];
    let mut ok = true;
    for (key, val) in keys {
        let r = engine::silent_cmd("reg")
            .args(["add", key, "/v", val, "/t", "REG_DWORD", "/d", "0", "/f"])
            .output();
        if !matches!(r, Ok(o) if o.status.success()) {
            ok = false;
        }
    }
    if ok {
        Some("Game DVR off".to_string())
    } else {
        Some("Game DVR change FAILED".to_string())
    }
}

pub struct SweepResult {
    pub lines: Vec<String>,
    pub main_tid: u32,
    pub lol_running: bool,
}

fn pick_main(pid: u32, last_tid: u32) -> Option<(u32, f64)> {
    // 2s load sample, hottest thread wins, with hysteresis for the
    // previously pinned thread (same rule as the PowerShell version).
    let rows = crate::engine::sample_thread_load(pid, 2);
    if rows.is_empty() {
        return None;
    }
    let (mut top_tid, mut top_ms) = rows[0];
    if last_tid != 0 {
        if let Some(&(_, prev_ms)) = rows.iter().find(|(t, _)| *t == last_tid) {
            if prev_ms > 800.0 && prev_ms >= top_ms * 0.5 {
                top_tid = last_tid;
                top_ms = prev_ms;
            }
        }
    }
    Some((top_tid, top_ms))
}

/// One isolation sweep. Skips re-pinning when the same LoL instance is
/// already pinned with identical masks (avoids mid-game migration hitches).
/// `full_bg` controls the (heavier) background sweep; callers throttle it.
pub fn sweep(cfg: &mut CoresConfig, lp: u32, full_bg: bool) -> SweepResult {
    let t0 = std::time::Instant::now();
    let mut log = Vec::new();
    log.push(format!(
        "Logical CPUs: {lp}, main={}, sub={}, buffers={}, bg={}",
        fmt_mask(cfg.main_mask()),
        fmt_mask(cfg.sub_mask()),
        fmt_mask(cfg.buffer_mask()),
        fmt_mask(cfg.bg_mask(lp)),
    ));
    let game = all_mask(lp);
    let mut main_tid = 0u32;
    let mut running = false;

    if let Some(pid) = engine::find_process(LOL_EXE) {
        running = true;
        // Skip when this exact instance is already pinned with identical
        // masks: re-pinning live game threads forces migrations (hitches).
        let live = engine::threads_of(pid);
        if pid == cfg.swept_pid
            && cfg.last_tid != 0
            && live.contains(&cfg.last_tid)
            && cfg.swept_main == cfg.main_mask()
            && cfg.swept_sub == cfg.sub_mask()
        {
            main_tid = cfg.last_tid;
            log.push(format!(
                "LoL main TID {main_tid} already pinned, skipped"
            ));
        } else {
        engine::set_priority_class(pid, engine::priority_high());
        if let Err(c) = engine::set_process_affinity(pid, game) {
            log.push(format!("LoL process affinity FAILED code {c}"));
        }
        if let Some((tid, ms)) = pick_main(pid, cfg.last_tid) {
            main_tid = tid;
            cfg.last_tid = tid;
            // Re-query threads fresh (stale handles fail) and pin.
            let mut main_ok = false;
            match engine::set_thread_affinity(tid, cfg.main_mask()) {
                Ok(()) => {
                    main_ok = true;
                    log.push(format!(
                        "LoL main TID {tid} ({ms:.0}ms/2s) -> {} OK",
                        fmt_mask(cfg.main_mask())
                    ));
                    if cfg.thread_boost {
                        if engine::set_thread_priority(tid, engine::thread_prio_highest())
                        {
                            log.push("LoL main thread priority -> Highest".to_string());
                        } else {
                            log.push("LoL main thread priority FAILED".to_string());
                        }
                    }
                }
                Err(c) => log.push(format!("LoL main TID {tid} -> FAILED code {c}")),
            }
            let mut n = 0u32;
            let mut ferr = 0u32;
            for (i, t) in engine::threads_of(pid).into_iter().enumerate() {
                if t == tid {
                    continue;
                }
                // Spread syscalls to avoid a mid-game hitch burst.
                if i % 16 == 15 {
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                match engine::set_thread_affinity(t, cfg.sub_mask()) {
                    Ok(()) => n += 1,
                    Err(c) => ferr = c,
                }
            }
            log.push(format!(
                "LoL sub threads -> {}: {n} pinned (first err {ferr})",
                fmt_mask(cfg.sub_mask())
            ));
            if let Some(l) = ensure_high_perf(cfg) {
                log.push(l);
            }
            if cfg.game_dvr_off {
                if let Some(l) = ensure_dvr_off(cfg) {
                    log.push(l);
                }
            }
            // Only remember successful pins; failures retry next sweep.
            if main_ok {
                cfg.swept_pid = pid;
                cfg.swept_main = cfg.main_mask();
                cfg.swept_sub = cfg.sub_mask();
            }
        } else {
            log.push("LoL threads unreadable".to_string());
        }
        }
    } else {
        log.push("LoL not running".to_string());
    }

    let bg = cfg.bg_mask(lp);
    if full_bg {
    let mut moved = 0u32;
    for (i, (pid, name)) in engine::list_processes().into_iter().enumerate() {
        if pid <= 4 || excluded(&name) {
            continue;
        }
        if name.eq_ignore_ascii_case(LOL_EXE) {
            continue;
        }
        if i % 64 == 63 {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        match engine::process_affinity(pid) {
            Some(a) if a != bg => {
                if engine::set_process_affinity(pid, bg).is_ok() {
                    if cfg.bg_priority {
                        engine::set_priority_class(pid, engine::priority_below_normal());
                    }
                    moved += 1;
                }
            }
            _ => {}
        }
    }
    log.push(format!("moved {moved} processes to {}", fmt_mask(bg)));
    } else {
        log.push("bg sweep skipped (throttled)".to_string());
    }
    log.push(format!("sweep took {}ms", t0.elapsed().as_millis()));
    SweepResult {
        lines: log,
        main_tid,
        lol_running: running,
    }
}

/// Reset everything to all CPUs + Normal priority (+ previous power plan).
pub fn restore(cfg: &mut CoresConfig, lp: u32) -> Vec<String> {
    let mut log = Vec::new();
    let all = all_mask(lp);
    log.push(format!("restore mode: everything back to {}", fmt_mask(all)));
    if let Some(pid) = engine::find_process(LOL_EXE) {
        engine::set_priority_class(pid, engine::priority_normal());
        let _ = engine::set_process_affinity(pid, all);
        let mut n = 0u32;
        for t in engine::threads_of(pid) {
            if engine::set_thread_affinity(t, all).is_ok() {
                n += 1;
            }
            engine::set_thread_priority(t, engine::thread_prio_normal());
        }
        log.push(format!("LoL reset to all CPUs: {n} threads, priority Normal"));
    } else {
        log.push("LoL not running".to_string());
    }
    let mut n = 0u32;
    for (pid, name) in engine::list_processes() {
        if pid <= 4 || excluded(&name) {
            continue;
        }
        match engine::process_affinity(pid) {
            Some(a) if a != all => {
                if engine::set_process_affinity(pid, all).is_ok() {
                    engine::set_priority_class(pid, engine::priority_normal());
                    n += 1;
                }
            }
            _ => {}
        }
    }
    log.push(format!("restored {n} processes to all CPUs"));
    if let Some(prev) = cfg.power_scheme.take() {
        match engine::silent_cmd("powercfg")
            .args(["/S", &prev])
            .output()
        {
            Ok(o) if o.status.success() => {
                log.push("power plan restored".to_string())
            }
            _ => log.push("power plan restore FAILED".to_string()),
        }
    }
    log
}
