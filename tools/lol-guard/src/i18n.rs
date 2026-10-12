//! Japanese / English UI strings. English when the Windows UI language
//! is English, Japanese otherwise.
use std::sync::OnceLock;
use windows::Win32::Globalization::GetUserDefaultUILanguage;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Ja,
    En,
}

pub fn lang() -> Lang {
    static CACHED: OnceLock<Lang> = OnceLock::new();
    *CACHED.get_or_init(|| unsafe {
        if GetUserDefaultUILanguage() & 0x3FF == 9 {
            Lang::En
        } else {
            Lang::Ja
        }
    })
}

pub struct T {
    pub menu_status: &'static str,
    pub menu_optimize: &'static str,
    pub menu_restore: &'static str,
    pub menu_settings: &'static str,
    pub role_main: &'static str,
    pub role_sub: &'static str,
    pub role_buf: &'static str,
    pub menu_taskreg: &'static str,
    pub menu_log: &'static str,
    pub menu_quit: &'static str,
    pub dlg_title: &'static str,
    pub btn_apply: &'static str,
    pub btn_restore: &'static str,
    pub btn_close: &'static str,
    pub tip_idle: &'static str,
    pub tip_applied: &'static str,
    pub tip_restored: &'static str,
    pub tip_running: &'static str,
    pub state_title: &'static str,
    pub status_idle: &'static str,
    pub status_manual: &'static str,
    pub status_running: &'static str,
    pub status_restored: &'static str,
    pub status_nolol: &'static str,
    pub err_invalid: &'static str,
    pub err_save: &'static str,
    pub err_elevate: &'static str,
    pub err_register: &'static str,
    pub err_cfgfix: &'static str,
    pub tray_fail: &'static str,
    pub log_nolol: &'static str,
    pub log_unreadable: &'static str,
    pub restored_title: &'static str,
    pub settings_title: &'static str,
}

const JA: T = T {
    menu_status: "状態を表示",
    menu_optimize: "今すぐ最適化",
    menu_restore: "元に戻す",
    menu_settings: "設定を開く",
    role_main: "メインコア",
    role_sub: "サブコア",
    role_buf: "バッファ",
    menu_taskreg: "自動起動を登録(管理者)",
    menu_log: "ログを開く",
    menu_quit: "終了",
    dlg_title: "LoL Guard 設定",
    btn_apply: "適用して閉じる",
    btn_restore: "元に戻す",
    btn_close: "閉じる",
    tip_idle: "LoL Guard - 待機中",
    tip_applied: "LoL Guard - 適用済み",
    tip_restored: "LoL Guard - 復元済み",
    tip_running: "LoL Guard - 適用中",
    state_title: "LoL Guard 状態",
    status_idle: "待機中",
    status_manual: "手動適用 main TID ",
    status_running: "適用中 main TID ",
    status_restored: "復元済み",
    status_nolol: "LoLが起動していません",
    err_invalid: "設定が不正です: ",
    err_save: "保存に失敗: ",
    err_elevate: "昇格に失敗しました。右クリック→管理者として実行してください。",
    err_register: "登録に失敗: ",
    err_cfgfix: "設定を修正しました: ",
    tray_fail: "トレイアイコンの作成に失敗しました。",
    log_nolol: "LoL not running",
    log_unreadable: "LoL threads unreadable",
    restored_title: "LoL Guard 復元",
    settings_title: "LoL Guard 設定",
};

const EN: T = T {
    menu_status: "Show status",
    menu_optimize: "Optimize now",
    menu_restore: "Restore defaults",
    menu_settings: "Open settings",
    role_main: "Main cores",
    role_sub: "Sub cores",
    role_buf: "Buffers",
    menu_taskreg: "Register autostart (admin)",
    menu_log: "Open log",
    menu_quit: "Quit",
    dlg_title: "LoL Guard Settings",
    btn_apply: "Apply and close",
    btn_restore: "Restore",
    btn_close: "Close",
    tip_idle: "LoL Guard - Idle",
    tip_applied: "LoL Guard - Applied",
    tip_restored: "LoL Guard - Restored",
    tip_running: "LoL Guard - Running",
    state_title: "LoL Guard Status",
    status_idle: "Idle",
    status_manual: "Manual apply main TID ",
    status_running: "Applying main TID ",
    status_restored: "Restored",
    status_nolol: "LoL is not running",
    err_invalid: "Invalid config: ",
    err_save: "Save failed: ",
    err_elevate: "Elevation failed. Right-click and run as administrator.",
    err_register: "Registration failed: ",
    err_cfgfix: "Config adjusted: ",
    tray_fail: "Failed to create tray icon.",
    log_nolol: "LoL not running",
    log_unreadable: "LoL threads unreadable",
    restored_title: "LoL Guard Restore",
    settings_title: "LoL Guard Settings",
};

/// All user-facing strings for the current Windows UI language.
pub fn strings() -> &'static T {
    match lang() {
        Lang::Ja => &JA,
        Lang::En => &EN,
    }
}
