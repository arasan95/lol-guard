//! Logon-task registration (highest privileges) + self elevation.
use std::process::Command;
use windows::core::PCWSTR;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

const TASK_NAME: &str = "LoLCoreGuardRs";

pub fn exe_path() -> std::path::PathBuf {
    std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("lol-guard.exe"))
}

pub fn base_dir() -> std::path::PathBuf {
    let exe_dir = exe_path()
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    // Prefer the exe folder; fall back to %APPDATA% when not writable
    // (e.g. Program Files on other PCs).
    let probe = exe_dir.join(".writetest");
    if std::fs::write(&probe, b"1").is_ok() {
        let _ = std::fs::remove_file(&probe);
        exe_dir
    } else if let Ok(appdata) = std::env::var("APPDATA") {
        let d = std::path::PathBuf::from(appdata).join("LoLGuard");
        let _ = std::fs::create_dir_all(&d);
        d
    } else {
        exe_dir
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Relaunch self elevated (UAC). Returns after the prompt resolves.
pub fn relaunch_elevated(args: &str) -> bool {
    let exe = wide(&exe_path().to_string_lossy());
    let verb = wide("runas");
    let par = wide(args);
    unsafe {
        let r = ShellExecuteW(
            None,
            PCWSTR::from_raw(verb.as_ptr()),
            PCWSTR::from_raw(exe.as_ptr()),
            PCWSTR::from_raw(par.as_ptr()),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        );
        r.0 as usize > 32
    }
}

/// Register (or refresh) the logon task. Must run elevated.
pub fn register_task() -> Result<String, String> {
    let exe = exe_path();
    let tr = format!("\"{}\" --tray", exe.display());
    let out = crate::engine::silent_cmd("schtasks")
        .args([
            "/Create",
            "/TN",
            TASK_NAME,
            "/SC",
            "ONLOGON",
            "/RL",
            "HIGHEST",
            "/TR",
            &tr,
            "/F",
        ])
        .output()
        .map_err(|e| format!("schtasks launch failed: {e}"))?;
    if out.status.success() {
        Ok(format!("registered task '{TASK_NAME}' (highest, at logon)"))
    } else {
        Err(format!(
            "schtasks failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ))
    }
}

pub fn task_status() -> String {
    let out = crate::engine::silent_cmd("schtasks")
        .args(["/Query", "/TN", TASK_NAME, "/FO", "LIST"])
        .output();
    match out {
        Ok(o) if o.status.success() => {
            let t = String::from_utf8_lossy(&o.stdout);
            t.lines()
                .filter(|l| {
                    l.contains("Task Name")
                        || l.contains("Status")
                        || l.contains("Logon Mode")
                })
                .collect::<Vec<_>>()
                .join("\n")
        }
        _ => "task not registered".to_string(),
    }
}
