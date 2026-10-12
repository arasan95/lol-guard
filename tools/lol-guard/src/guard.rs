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

fn excluded(name: &str) -> bool {
    EXCLUDE.iter().any(|e| e.eq_ignore_ascii_case(name))
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

/// One full isolation sweep. Returns log lines; updates cfg.last_tid.
pub fn sweep(cfg: &mut CoresConfig, lp: u32) -> SweepResult {
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
        engine::set_priority(pid, true);
        if let Err(c) = engine::set_process_affinity(pid, game) {
            log.push(format!("LoL process affinity FAILED code {c}"));
        }
        if let Some((tid, ms)) = pick_main(pid, cfg.last_tid) {
            main_tid = tid;
            cfg.last_tid = tid;
            // Re-query threads fresh (stale handles fail) and pin.
            match engine::set_thread_affinity(tid, cfg.main_mask()) {
                Ok(()) => log.push(format!(
                    "LoL main TID {tid} ({ms:.0}ms/2s) -> {} OK",
                    fmt_mask(cfg.main_mask())
                )),
                Err(c) => log.push(format!("LoL main TID {tid} -> FAILED code {c}")),
            }
            let mut n = 0u32;
            let mut ferr = 0u32;
            for t in engine::threads_of(pid) {
                if t == tid {
                    continue;
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
        } else {
            log.push("LoL threads unreadable".to_string());
        }
    } else {
        log.push("LoL not running".to_string());
    }

    let bg = cfg.bg_mask(lp);
    let mut moved = 0u32;
    for (pid, name) in engine::list_processes() {
        if pid <= 4 || excluded(&name) {
            continue;
        }
        if name.eq_ignore_ascii_case(LOL_EXE) {
            continue;
        }
        match engine::process_affinity(pid) {
            Some(a) if a != bg => {
                if engine::set_process_affinity(pid, bg).is_ok() {
                    moved += 1;
                }
            }
            _ => {}
        }
    }
    log.push(format!("moved {moved} processes to {}", fmt_mask(bg)));
    SweepResult {
        lines: log,
        main_tid,
        lol_running: running,
    }
}

/// Reset everything to all CPUs + Normal priority.
pub fn restore(_cfg: &CoresConfig, lp: u32) -> Vec<String> {
    let mut log = Vec::new();
    let all = all_mask(lp);
    log.push(format!("restore mode: everything back to {}", fmt_mask(all)));
    if let Some(pid) = engine::find_process(LOL_EXE) {
        engine::set_priority(pid, false);
        let _ = engine::set_process_affinity(pid, all);
        let mut n = 0u32;
        for t in engine::threads_of(pid) {
            if engine::set_thread_affinity(t, all).is_ok() {
                n += 1;
            }
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
                    n += 1;
                }
            }
            _ => {}
        }
    }
    log.push(format!("restored {n} processes to all CPUs"));
    log
}
