//! Raw Win32 tray icon + popup menu (no third-party tray dependency).
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::{mpsc::Sender, OnceLock};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE,
    NIM_MODIFY, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DefWindowProcW, DestroyMenu, DestroyWindow,
    GetCursorPos, LoadIconW, PostMessageW, PostQuitMessage, RegisterClassW,
    SetForegroundWindow, TrackPopupMenu, TranslateMessage, DispatchMessageW,
    CreateWindowExW, GetMessageW, IDI_SHIELD, MF_CHECKED, MF_POPUP,
    MF_SEPARATOR, MF_STRING, TPM_RETURNCMD, TPM_RIGHTBUTTON,
    WINDOW_EX_STYLE, WM_APP, WM_CLOSE, WM_DESTROY, WNDCLASSW, WS_OVERLAPPED,
};

use crate::config::CoresConfig;

const WM_TRAY: u32 = WM_APP + 1;

// Menu command ids.
const ID_STATUS: u32 = 1;
const ID_OPTIMIZE: u32 = 2;
const ID_RESTORE: u32 = 3;
const ID_SETTINGS: u32 = 4;
const ID_TASKREG: u32 = 5;
const ID_LOG: u32 = 6;
const ID_QUIT: u32 = 7;
fn cpu_id(role: u32, cpu: u8) -> u32 {
    1000 + role * 64 + cpu as u32
}

#[derive(Clone, Copy, Debug)]
pub enum AppEvent {
    Status,
    Optimize,
    Restore,
    OpenSettings,
    TaskReg,
    Log,
    Toggle(u8, u8), // (role 0=main 1=sub 2=buf, cpu)
    Quit,
}

static CMD_TX: OnceLock<Sender<AppEvent>> = OnceLock::new();
static TRAY_HWND: AtomicUsize = AtomicUsize::new(0);
static CFG_PATH: OnceLock<PathBuf> = OnceLock::new();
static LP_COUNT: AtomicU32 = AtomicU32::new(8);

pub fn set_sender(tx: Sender<AppEvent>) {
    let _ = CMD_TX.set(tx);
}

fn send(ev: AppEvent) {
    if let Some(tx) = CMD_TX.get() {
        let _ = tx.send(ev);
    }
}

pub fn tray_hwnd() -> HWND {
    HWND(TRAY_HWND.load(Ordering::SeqCst) as *mut _)
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn nid(hwnd: HWND, tip: &str, icon: windows::Win32::UI::WindowsAndMessaging::HICON) -> NOTIFYICONDATAW {
    let mut sz = [0u16; 128];
    let w = wide(tip);
    let n = w.len().min(127);
    sz[..n].copy_from_slice(&w[..n]);
    NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
        uCallbackMessage: WM_TRAY,
        hIcon: icon,
        szTip: sz,
        ..Default::default()
    }
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    w: WPARAM,
    l: LPARAM,
) -> LRESULT {
    const WM_LBUTTONUP: u32 = 0x0202;
    const WM_RBUTTONUP: u32 = 0x0205;
    if msg == WM_TRAY {
        let kind = l.0 as u32;
        if kind == WM_RBUTTONUP {
            show_menu(hwnd);
        } else if kind == WM_LBUTTONUP {
            send(AppEvent::Status);
        }
        return LRESULT(0);
    }
    if msg == WM_CLOSE {
        let icon = LoadIconW(None, IDI_SHIELD).unwrap_or_default();
        let data = nid(hwnd, "", icon);
        let _ = Shell_NotifyIconW(NIM_DELETE, &data);
        DestroyWindow(hwnd);
        return LRESULT(0);
    }
    if msg == WM_DESTROY {
        PostQuitMessage(0);
        return LRESULT(0);
    }
    DefWindowProcW(hwnd, msg, w, l)
}

fn show_menu(hwnd: HWND) {
    unsafe {
        let lp = LP_COUNT.load(Ordering::SeqCst);
        let cfg_path = match CFG_PATH.get() {
            Some(p) => p.clone(),
            None => return,
        };
        let cfg = CoresConfig::load(&cfg_path, lp);
        let menu = match CreatePopupMenu() {
            Ok(m) => m,
            Err(_) => return,
        };
        let item = |text: &str, id: u32, checked: bool| {
            let t = wide(text);
            let mut f = MF_STRING;
            if checked {
                f |= MF_CHECKED;
            }
            let _ = AppendMenuW(menu, f, id as usize, PCWSTR::from_raw(t.as_ptr()));
            // NOTE: `t` must outlive the call only (copied by user32).
        };
        item("状態を表示", ID_STATUS, false);
        item("今すぐ最適化", ID_OPTIMIZE, false);
        item("元に戻す", ID_RESTORE, false);
        item("設定を開く", ID_SETTINGS, false);
        for (role, title, sel) in [
            (0u32, "メインコア", &cfg.main),
            (1u32, "サブコア", &cfg.sub),
            (2u32, "バッファ", &cfg.buffers),
        ] {
            let sub = match CreatePopupMenu() {
                Ok(m) => m,
                Err(_) => continue,
            };
            for cpu in 0..lp.min(64) {
                let t = wide(&format!("CPU {cpu}"));
                let mut f = MF_STRING;
                if sel.contains(&(cpu as u8)) {
                    f |= MF_CHECKED;
                }
                let _ = AppendMenuW(sub, f, cpu_id(role, cpu as u8) as usize, PCWSTR::from_raw(t.as_ptr()));
            }
            let t = wide(title);
            let _ = AppendMenuW(menu, MF_STRING | MF_POPUP, sub.0 as usize, PCWSTR::from_raw(t.as_ptr()));
        }
        item("自動起動を登録(管理者)", ID_TASKREG, false);
        item("ログを開く", ID_LOG, false);
        let sep = wide("");
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::from_raw(sep.as_ptr()));
        item("終了", ID_QUIT, false);

        let mut pt = POINT { x: 0, y: 0 };
        if GetCursorPos(&mut pt).is_err() {
            let _ = DestroyMenu(menu);
            return;
        }
        let _ = SetForegroundWindow(hwnd);
        let picked = TrackPopupMenu(
            menu,
            TPM_RIGHTBUTTON | TPM_RETURNCMD,
            pt.x,
            pt.y,
            None,
            hwnd,
            None,
        );
        let _ = PostMessageW(Some(hwnd), 0, WPARAM(0), LPARAM(0));
        let id = picked.0 as u32;
        let _ = DestroyMenu(menu);
        if id == 0 {
            return;
        }
        let ev = match id {
            ID_STATUS => AppEvent::Status,
            ID_OPTIMIZE => AppEvent::Optimize,
            ID_RESTORE => AppEvent::Restore,
            ID_SETTINGS => AppEvent::OpenSettings,
            ID_TASKREG => AppEvent::TaskReg,
            ID_LOG => AppEvent::Log,
            ID_QUIT => AppEvent::Quit,
            _ if id >= 1000 => {
                let v = id - 1000;
                AppEvent::Toggle((v / 64) as u8, (v % 64) as u8)
            }
            _ => return,
        };
        send(ev);
    }
}

/// Create the tray icon on the calling thread (must pump messages).
/// Returns the hidden window handle.
pub fn create(cfg_path: PathBuf, lp: u32, tip: &str) -> Option<HWND> {
    let _ = CFG_PATH.set(cfg_path);
    LP_COUNT.store(lp, Ordering::SeqCst);
    unsafe {
        let cls = wide("LoLGuardTray");
        let hinst: Option<HINSTANCE> =
            GetModuleHandleW(None).ok().map(|h| HINSTANCE(h.0));
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wnd_proc),
            hInstance: hinst.unwrap_or_default().into(),
            lpszClassName: PCWSTR::from_raw(cls.as_ptr()),
            ..Default::default()
        };
        // Class may already exist from a previous run in this process.
        let _ = RegisterClassW(&wc);
        let title = wide("LoLGuardTray");
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            PCWSTR::from_raw(cls.as_ptr()),
            PCWSTR::from_raw(title.as_ptr()),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            None,
            None,
            hinst,
            None,
        )
        .ok()?;
        let icon = LoadIconW(None, IDI_SHIELD).unwrap_or_default();
        let data = nid(hwnd, tip, icon);
        if Shell_NotifyIconW(NIM_ADD, &data).as_bool() {
            TRAY_HWND.store(hwnd.0 as usize, Ordering::SeqCst);
            Some(hwnd)
        } else {
            None
        }
    }
}

pub fn set_tip(text: &str) {
    unsafe {
        let hwnd = tray_hwnd();
        if hwnd.0.is_null() {
            return;
        }
        let icon = LoadIconW(None, IDI_SHIELD).unwrap_or_default();
        let data = nid(hwnd, text, icon);
        let _ = Shell_NotifyIconW(NIM_MODIFY, &data);
    }
}

/// Ask the tray window (main thread) to close.
pub fn request_quit() {
    unsafe {
        let hwnd = tray_hwnd();
        if !hwnd.0.is_null() {
            let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
    }
}

/// Standard message pump; returns when WM_QUIT arrives.
pub fn pump() {
    unsafe {
        let mut msg = std::mem::zeroed();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}
