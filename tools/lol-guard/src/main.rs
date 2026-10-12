//! LoL Guard: tray-resident LoL core isolator (Rust, single exe).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
mod engine;
mod guard;
mod i18n;
mod settings;
mod task;
mod win_tray;

use std::fs::OpenOptions;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
use windows::Win32::Media::timeBeginPeriod;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::WindowsAndMessaging::{
    MessageBoxW, MB_ICONINFORMATION, MB_OK,
};
use win_tray::AppEvent;

fn base_files() -> (std::path::PathBuf, std::path::PathBuf) {
    let dir = task::base_dir();
    (dir.join("cores.json"), dir.join("guard.log"))
}

fn log_line(path: &std::path::PathBuf, line: &str) {
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let _ = writeln!(f, "[{secs}] {line}");
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn msgbox(title: &str, body: &str) {
    let t = wide(title);
    let b = wide(body);
    unsafe {
        MessageBoxW(
            None,
            PCWSTR::from_raw(b.as_ptr()),
            PCWSTR::from_raw(t.as_ptr()),
            MB_OK | MB_ICONINFORMATION,
        );
    }
}

fn single_instance() -> bool {
    unsafe {
        match CreateMutexW(None, true, windows::core::w!("LoLGuardRsSingleInstance")) {
            Ok(_) => GetLastError() != ERROR_ALREADY_EXISTS,
            Err(_) => true,
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).map(|s| s.as_str()).unwrap_or("--tray");
    let lp_raw = engine::logical_count().max(1);
    // Affinity masks are 64-bit (single processor group). Clamp for exotic machines.
    let lp = lp_raw.min(64);
    let (cfg_path, log_path) = base_files();

    let lp_log = log_path.clone();
    let log_path2 = log_path.clone();
    std::panic::set_hook(Box::new(move |info| {
        log_line(&lp_log, &format!("PANIC: {info}"));
    }));
    if lp_raw > 64 {
        log_line(&log_path, &format!("WARN: {lp_raw} CPUs seen, using first 64"));
    }

    match mode {
        "--register-task" => {
            if !engine::is_admin() {
                if !task::relaunch_elevated("--register-task") {
                    msgbox("LoL Guard", i18n::strings().err_elevate);
                }
                return;
            }
            match task::register_task() {
                Ok(s) => {
                    log_line(&log_path, &s);
                    msgbox("LoL Guard", &format!("{s}\n{}", task::task_status()));
                }
                Err(e) => msgbox(
                    "LoL Guard",
                    &format!("{}{e}", i18n::strings().err_register),
                ),
            }
            return;
        }
        "--optimize-once" => {
            let mut cfg = config::CoresConfig::load(&cfg_path, lp);
            let r = guard::sweep(&mut cfg, lp, true);
            let _ = cfg.save(&cfg_path);
            for l in &r.lines {
                println!("{l}");
                log_line(&log_path, l);
            }
            return;
        }
        "--restore" => {
            let mut cfg = config::CoresConfig::load(&cfg_path, lp);
            let lines = guard::restore(&mut cfg, lp);
            let _ = cfg.save(&cfg_path);
            for l in &lines {
                println!("{l}");
                log_line(&log_path, l);
            }
            return;
        }
        "--probe" => {
            let tid: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
            let mask: u64 = if args.len() > 3 && args[3].starts_with("0x") {
                u64::from_str_radix(args[3].trim_start_matches("0x"), 16).unwrap_or(0x80)
            } else {
                0x80
            };
            for l in engine::probe_thread(tid, mask) {
                println!("{l}");
            }
            return;
        }
        _ => {}
    }

    // ---- tray mode ----
    if !single_instance() {
        return;
    }
    let (tx, rx) = mpsc::channel::<AppEvent>();
    win_tray::set_sender(tx);
    let quit = Arc::new(AtomicBool::new(false));
    let dirty = Arc::new(AtomicBool::new(false));

    // Worker thread: commands + periodic sweeps.
    let w_cfg_path = cfg_path.clone();
    let w_log = log_path.clone();
    let w_quit = quit.clone();
    let w_dirty = dirty.clone();
    let worker = std::thread::spawn(move || {
        let mut cfg = config::CoresConfig::load(&w_cfg_path, lp);
        let _ = cfg.save(&w_cfg_path);
        let mut last_status = i18n::strings().status_idle.to_string();
        let mut was_running = false;
        let mut last_sweep = Instant::now() - Duration::from_secs(3600);
    let mut last_bg = Instant::now() - Duration::from_secs(3600);
    log_line(
        &log_path,
        &format!("tray started (LP={lp}, admin={})", engine::is_admin()),
    );
    // 1ms system timer while resident (improves frame pacing; ends on exit).
    unsafe {
        timeBeginPeriod(1);
    }
        win_tray::set_tip(i18n::strings().tip_idle);
        loop {
            if w_quit.load(Ordering::SeqCst) {
                break;
            }
            if w_dirty.swap(false, Ordering::SeqCst) {
                let keep_tid = cfg.last_tid;
                cfg = config::CoresConfig::load(&w_cfg_path, lp);
                if cfg.last_tid == 0 {
                    cfg.last_tid = keep_tid;
                }
            }
            while let Ok(ev) = rx.try_recv() {
                match ev {
                    AppEvent::Quit => {
                        win_tray::request_quit();
                        w_quit.store(true, Ordering::SeqCst);
                    }
                    AppEvent::Status => {
                        msgbox(i18n::strings().state_title, &last_status)
                    }
                    AppEvent::Optimize => {
                        let r = guard::sweep(&mut cfg, lp, true);
                        last_bg = Instant::now();
                        let _ = cfg.save(&w_cfg_path);
                        for l in &r.lines {
                            log_line(&w_log, l);
                        }
                        let tx = i18n::strings();
                        last_status = if r.lol_running {
                            format!("{} {}", tx.status_manual, r.main_tid)
                        } else {
                            tx.status_nolol.to_string()
                        };
                        win_tray::set_tip(tx.tip_applied);
                        msgbox("LoL Guard", &r.lines.join("\n"));
                    }
                    AppEvent::Restore => {
                        let mut rcfg = config::CoresConfig::load(&w_cfg_path, lp);
                        let lines = guard::restore(&mut rcfg, lp);
                        let _ = rcfg.save(&w_cfg_path);
                        for l in &lines {
                            log_line(&w_log, l);
                        }
                        let tx = i18n::strings();
                        last_status = tx.status_restored.to_string();
                        win_tray::set_tip(tx.tip_restored);
                        msgbox(tx.restored_title, &lines.join("\n"));
                    }
                    AppEvent::OpenSettings => {
                        settings::open(
                            w_cfg_path.clone(),
                            w_log.clone(),
                            lp,
                            w_dirty.clone(),
                        );
                    }
                    AppEvent::TaskReg => {
                        if !engine::is_admin() {
                            if !task::relaunch_elevated("--register-task") {
                                msgbox("LoL Guard", i18n::strings().err_elevate);
                            }
                        } else {
                            match task::register_task() {
                                Ok(s) => msgbox("LoL Guard", &s),
                                Err(e) => msgbox(
                                    "LoL Guard",
                                    &format!("{}{e}", i18n::strings().err_register),
                                ),
                            }
                        }
                    }
                    AppEvent::Log => {
                        let _ = std::process::Command::new("notepad.exe")
                            .arg(&w_log)
                            .spawn();
                    }
                    AppEvent::Toggle(role, cpu) => {
                        {
                            let list = match role {
                                0 => &mut cfg.main,
                                1 => &mut cfg.sub,
                                _ => &mut cfg.buffers,
                            };
                            if list.contains(&cpu) {
                                list.retain(|&x| x != cpu);
                            } else {
                                list.push(cpu);
                            }
                        }
                        if let Some(w) = cfg.sanitize(lp) {
                            msgbox(
                                "LoL Guard",
                                &format!("{}{w}", i18n::strings().err_cfgfix),
                            );
                        }
                        let _ = cfg.save(&w_cfg_path);
                    }
                }
            }
            if w_quit.load(Ordering::SeqCst) {
                break;
            }
            if last_sweep.elapsed() >= Duration::from_secs(cfg.interval_secs) {
                last_sweep = Instant::now();
                let found = engine::find_process("League of Legends.exe").is_some();
                if found {
                    // Full background sweep at most every ~3 minutes;
                    // LoL pins are skipped when already applied.
                    let full_bg = last_bg.elapsed() >= Duration::from_secs(180);
                    if full_bg {
                        last_bg = Instant::now();
                    }
                    let r = guard::sweep(&mut cfg, lp, full_bg);
                    let _ = cfg.save(&w_cfg_path);
                    for l in &r.lines {
                        log_line(&w_log, l);
                    }
                    let tx = i18n::strings();
                    last_status = format!("{} {}", tx.status_running, r.main_tid);
                    if !was_running {
                        win_tray::set_tip(tx.tip_running);
                    }
                } else {
                    let tx = i18n::strings();
                    last_status = tx.status_idle.to_string();
                    win_tray::set_tip(tx.tip_idle);
                }
                was_running = found;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    });

    // Main thread: tray window + message pump.
    match win_tray::create(cfg_path, lp, "LoL Guard") {
        Some(_) => win_tray::pump(),
        None => {
            log_line(&log_path2, "tray icon creation failed");
            msgbox("LoL Guard", i18n::strings().tray_fail);
        }
    }
    quit.store(true, Ordering::SeqCst);
    let _ = worker.join();
}
