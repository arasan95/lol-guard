//! Config dialog window (own thread + message loop).
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
    GetMessageW, GetSystemMetrics, MessageBoxW, PostQuitMessage, RegisterClassW,
    SendMessageW, SetForegroundWindow, SetWindowLongPtrW, GetWindowLongPtrW,
    ShowWindow, TranslateMessage, BM_GETCHECK, BM_SETCHECK, BS_AUTOCHECKBOX,
    GWLP_USERDATA, MB_ICONINFORMATION, MB_OK,
    SM_CXSCREEN, SM_CYSCREEN, WINDOW_EX_STYLE, WM_CLOSE, WM_COMMAND,
    WM_DESTROY, WM_NCDESTROY, WNDCLASSW, WINDOW_STYLE, WS_CHILD, WS_VISIBLE, WS_CAPTION,
    WS_SYSMENU, HMENU,
};

use crate::config::CoresConfig;
use crate::guard;

const ID_APPLY: u32 = 101;
const ID_RESTORE: u32 = 102;
const ID_CLOSE: u32 = 103;
fn check_id(role: u8, cpu: u8) -> u32 {
    1000 + role as u32 * 64 + cpu as u32
}

struct DlgState {
    checks: Vec<(u8, u8, HWND)>,
    cfg_path: PathBuf,
    log_path: PathBuf,
    lp: u32,
    dirty: Arc<AtomicBool>,
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn log_line(path: &PathBuf, line: &str) {
    use std::fs::OpenOptions;
    use std::io::Write;
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let _ = writeln!(f, "[{secs}] {line}");
    }
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

fn state_of(hwnd: HWND) -> Option<*mut DlgState> {
    unsafe {
        let p = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
        if p == 0 {
            None
        } else {
            Some(p as *mut DlgState)
        }
    }
}

fn apply_from_dialog(st: &DlgState) -> Result<String, String> {
    let mut main = vec![];
    let mut sub = vec![];
    let mut buf = vec![];
    unsafe {
        for (role, cpu, h) in &st.checks {
            let on = SendMessageW(*h, BM_GETCHECK, None, None).0 == 1;
            if on {
                match role {
                    0 => main.push(*cpu),
                    1 => sub.push(*cpu),
                    _ => buf.push(*cpu),
                }
            }
        }
    }
    let old = CoresConfig::load(&st.cfg_path, st.lp);
    let mut cfg = CoresConfig {
        main,
        sub,
        buffers: buf,
        interval_secs: old.interval_secs,
        last_tid: old.last_tid,
    };
    if let Some(w) = cfg.sanitize(st.lp) {
        return Err(format!("設定が不正です: {w}"));
    }
    cfg.save(&st.cfg_path)
        .map_err(|e| format!("保存に失敗: {e}"))?;
    st.dirty.store(true, Ordering::SeqCst);
    Ok(format!(
        "saved main={:?} sub={:?} buf={:?}",
        cfg.main, cfg.sub, cfg.buffers
    ))
}

unsafe extern "system" fn dlg_proc(
    hwnd: HWND,
    msg: u32,
    w: WPARAM,
    l: LPARAM,
) -> LRESULT {
    match msg {
        WM_COMMAND => {
            let id = (w.0 & 0xFFFF) as u32;
            if id == ID_APPLY {
                if let Some(p) = state_of(hwnd) {
                    let r = unsafe { apply_from_dialog(&*p) };
                    match r {
                        Ok(s) => {
                            if let Some(q) = state_of(hwnd) {
                                unsafe {
                                    log_line(&(*q).log_path, &s);
                                }
                            }
                            unsafe { DestroyWindow(hwnd) };
                        }
                        Err(e) => msgbox("LoL Guard 設定", &e),
                    }
                }
            } else if id == ID_RESTORE {
                if let Some(p) = state_of(hwnd) {
                    unsafe {
                        let cfg = CoresConfig::load(&(*p).cfg_path, (*p).lp);
                        let lines = guard::restore(&cfg, (*p).lp);
                        for x in &lines {
                            log_line(&(*p).log_path, x);
                        }
                        msgbox("LoL Guard 復元", &lines.join("\n"));
                    }
                }
            } else if id == ID_CLOSE {
                DestroyWindow(hwnd);
            }
            return LRESULT(0);
        }
        WM_CLOSE => {
            DestroyWindow(hwnd);
            return LRESULT(0);
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            return LRESULT(0);
        }
        WM_NCDESTROY => {
            if let Some(p) = state_of(hwnd) {
                unsafe {
                    drop(Box::from_raw(p));
                }
            }
        }
        _ => {}
    }
    DefWindowProcW(hwnd, msg, w, l)
}

/// Open the settings window on its own thread. Returns immediately.
pub fn open(cfg_path: PathBuf, log_path: PathBuf, lp: u32, dirty: Arc<AtomicBool>) {
    std::thread::spawn(move || unsafe {
        let cls = wide("LoLGuardSettings");
        let hinst: Option<HINSTANCE> =
            GetModuleHandleW(None).ok().map(|h| HINSTANCE(h.0));
        let wc = WNDCLASSW {
            lpfnWndProc: Some(dlg_proc),
            hInstance: hinst.unwrap_or_default().into(),
            lpszClassName: PCWSTR::from_raw(cls.as_ptr()),
            ..Default::default()
        };
        // May already exist in this process; failure is fine.
        let _ = RegisterClassW(&wc);

        let cfg = CoresConfig::load(&cfg_path, lp);
        // Show every logical CPU the OS reports (up to the 64-bit mask limit).
        let ncpu = lp.min(64);
        let rows = ((ncpu + 3) / 4) as i32;
        let block = 30 + rows * 24;
        let (ww, wh) = (372i32, 10 + 3 * block + 52);
        let cx = GetSystemMetrics(SM_CXSCREEN);
        let cy = GetSystemMetrics(SM_CYSCREEN);
        let title = wide("LoL Guard 設定");
        let hwnd = match CreateWindowExW(
            WINDOW_EX_STYLE(0),
            PCWSTR::from_raw(cls.as_ptr()),
            PCWSTR::from_raw(title.as_ptr()),
            WS_CAPTION | WS_SYSMENU,
            (cx - ww) / 2,
            (cy - wh) / 2,
            ww,
            wh,
            None,
            None,
            hinst,
            None,
        ) {
            Ok(h) => h,
            Err(_) => return,
        };

        let btn_cls = wide("BUTTON");
        let stc_cls = wide("STATIC");
        let mut checks = Vec::new();
        let roles = [(0u8, "メインコア"), (1u8, "サブコア"), (2u8, "バッファ")];
        for (ri, &(role, name)) in roles.iter().enumerate() {
            let y0 = 10 + ri as i32 * block;
            let lb = wide(name);
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                PCWSTR::from_raw(stc_cls.as_ptr()),
                PCWSTR::from_raw(lb.as_ptr()),
                WS_CHILD | WS_VISIBLE,
                12,
                y0,
                200,
                20,
                Some(hwnd),
                None,
                hinst,
                None,
            );
            let sel: &Vec<u8> = match role {
                0 => &cfg.main,
                1 => &cfg.sub,
                _ => &cfg.buffers,
            };
            for cpu in 0..ncpu {
                let col = (cpu % 4) as i32;
                let row = (cpu / 4) as i32;
                let lb2 = wide(&format!("CPU{cpu}"));
                if let Ok(h) = CreateWindowExW(
                    WINDOW_EX_STYLE(0),
                    PCWSTR::from_raw(btn_cls.as_ptr()),
                    PCWSTR::from_raw(lb2.as_ptr()),
                    WS_CHILD | WS_VISIBLE | WINDOW_STYLE(BS_AUTOCHECKBOX as u32),
                    12 + col * 88,
                    y0 + 22 + row * 24,
                    82,
                    22,
                    Some(hwnd),
                    Some(HMENU(check_id(role, cpu as u8) as *mut _)),
                    hinst,
                    None,
                ) {
                    let on = sel.contains(&(cpu as u8));
                    SendMessageW(
                        h,
                        BM_SETCHECK,
                        Some(WPARAM(if on { 1 } else { 0 })),
                        None,
                    );
                    checks.push((role, cpu as u8, h));
                }
            }
        }
        let btny = 10 + 3 * block + 8;
        let mkbtn = |text: &str, id: u32, x: i32, style: WINDOW_STYLE| {
            let t = wide(text);
            let _ = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                PCWSTR::from_raw(btn_cls.as_ptr()),
                PCWSTR::from_raw(t.as_ptr()),
                WS_CHILD | WS_VISIBLE | style,
                x,
                btny,
                104,
                28,
                Some(hwnd),
                Some(HMENU(id as *mut _)),
                hinst,
                None,
            );
        };
        mkbtn("適用して閉じる", ID_APPLY, 12, WINDOW_STYLE(1));
        mkbtn("元に戻す", ID_RESTORE, 128, WINDOW_STYLE(0));
        mkbtn("閉じる", ID_CLOSE, 244, WINDOW_STYLE(0));

        let st = Box::new(DlgState {
            checks,
            cfg_path,
            log_path,
            lp,
            dirty,
        });
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(st) as isize);
        ShowWindow(hwnd, windows::Win32::UI::WindowsAndMessaging::SW_SHOW);
        SetForegroundWindow(hwnd);

        let mut msg = std::mem::zeroed();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    });
}
