//! Low-level Win32 helpers: process/thread enumeration and affinity control.
use std::collections::HashMap;
use windows::Win32::Foundation::{CloseHandle, FILETIME, GetLastError};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, Thread32First,
    Thread32Next, CREATE_TOOLHELP_SNAPSHOT_FLAGS, PROCESSENTRY32W,
    TH32CS_SNAPPROCESS, TH32CS_SNAPTHREAD, THREADENTRY32,
};
use windows::Win32::System::SystemInformation::GetSystemInfo;
use windows::Win32::System::Threading::{
    GetProcessAffinityMask, GetThreadTimes, OpenProcess, OpenThread,
    SetPriorityClass, SetProcessAffinityMask, SetThreadAffinityMask,
    HIGH_PRIORITY_CLASS, NORMAL_PRIORITY_CLASS,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_INFORMATION,
    THREAD_QUERY_INFORMATION, THREAD_SET_INFORMATION, THREAD_SUSPEND_RESUME,
    THREAD_ACCESS_RIGHTS,
};
use windows::Win32::UI::Shell::IsUserAnAdmin;

pub fn logical_count() -> u32 {
    unsafe {
        let mut si = std::mem::zeroed();
        GetSystemInfo(&mut si);
        si.dwNumberOfProcessors
    }
}

pub fn is_admin() -> bool {
    unsafe { IsUserAnAdmin().as_bool() }
}

fn u16_to_string(buf: &[u16]) -> String {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}

/// (pid, exe_name) for every process.
pub fn list_processes() -> Vec<(u32, String)> {
    let mut out = Vec::new();
    unsafe {
        let snap = match CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) {
            Ok(h) => h,
            Err(_) => return out,
        };
        let mut pe: PROCESSENTRY32W = std::mem::zeroed();
        pe.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        if Process32FirstW(snap, &mut pe).is_ok() {
            loop {
                out.push((pe.th32ProcessID, u16_to_string(&pe.szExeFile)));
                if !Process32NextW(snap, &mut pe).is_ok() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
    }
    out
}

/// Thread ids owned by `pid`.
pub fn threads_of(pid: u32) -> Vec<u32> {
    let mut out = Vec::new();
    unsafe {
        let snap = match CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) {
            Ok(h) => h,
            Err(_) => return out,
        };
        let mut te: THREADENTRY32 = std::mem::zeroed();
        te.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
        if Thread32First(snap, &mut te).is_ok() {
            loop {
                if te.th32OwnerProcessID == pid {
                    out.push(te.th32ThreadID);
                }
                if !Thread32Next(snap, &mut te).is_ok() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
    }
    out
}

/// Suppress unused-import warnings if API surface shifts.
#[allow(dead_code)]
pub fn snapshot_flags_process() -> CREATE_TOOLHELP_SNAPSHOT_FLAGS {
    TH32CS_SNAPPROCESS
}

fn ft_to_u64(ft: &FILETIME) -> u64 {
    ((ft.dwHighDateTime as u64) << 32) | (ft.dwLowDateTime as u64)
}

/// Total CPU time (100ns units) consumed by a thread, or None.
pub fn thread_cpu_100ns(tid: u32) -> Option<u64> {
    unsafe {
        let h = OpenThread(THREAD_QUERY_INFORMATION, false, tid).ok()?;
        let mut c: FILETIME = std::mem::zeroed();
        let mut e: FILETIME = std::mem::zeroed();
        let mut k: FILETIME = std::mem::zeroed();
        let mut u: FILETIME = std::mem::zeroed();
        let ok = GetThreadTimes(h, &mut c, &mut e, &mut k, &mut u).is_ok();
        let _ = CloseHandle(h);
        if ok {
            Some(ft_to_u64(&k) + ft_to_u64(&u))
        } else {
            None
        }
    }
}

/// Process affinity mask, or None when unreadable (protected/system).
pub fn process_affinity(pid: u32) -> Option<u64> {
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut proc_mask: usize = 0;
        let mut sys_mask: usize = 0;
        let ok = GetProcessAffinityMask(h, &mut proc_mask, &mut sys_mask).is_ok();
        let _ = CloseHandle(h);
        if ok {
            Some(proc_mask as u64)
        } else {
            None
        }
    }
}

pub fn set_process_affinity(pid: u32, mask: u64) -> Result<(), u32> {
    unsafe {
        let h = match OpenProcess(PROCESS_SET_INFORMATION, false, pid) {
            Ok(h) => h,
            Err(_) => return Err(GetLastError().0),
        };
        let r = SetProcessAffinityMask(h, mask as usize);
        let code = GetLastError().0;
        let _ = CloseHandle(h);
        if r.is_ok() {
            Ok(())
        } else {
            Err(code)
        }
    }
}

pub fn set_thread_affinity(tid: u32, mask: u64) -> Result<(), u32> {
    // NOTE: SET alone fails with ACCESS_DENIED on protected-game threads;
    // QUERY|SET succeeds (matches .NET behavior). Verified 2026-10-12.
    unsafe {
        let h = match OpenThread(
            THREAD_QUERY_INFORMATION | THREAD_SET_INFORMATION,
            false,
            tid,
        ) {
            Ok(h) => h,
            Err(_) => return Err(GetLastError().0),
        };
        let prev = SetThreadAffinityMask(h, mask as usize);
        let code = GetLastError().0;
        let _ = CloseHandle(h);
        if prev != 0 {
            Ok(())
        } else {
            Err(code)
        }
    }
}

pub fn set_priority(pid: u32, high: bool) -> bool {
    unsafe {
        let h = match OpenProcess(PROCESS_SET_INFORMATION, false, pid) {
            Ok(h) => h,
            Err(_) => return false,
        };
        let ok = SetPriorityClass(h, if high {
            HIGH_PRIORITY_CLASS
        } else {
            NORMAL_PRIORITY_CLASS
        })
        .is_ok();
        let _ = CloseHandle(h);
        ok
    }
}

/// Diagnostic: try opening a thread with various rights and setting
/// affinity; returns human-readable lines. Does not change anything
/// except a successful set (caller passes a harmless mask).
pub fn probe_thread(tid: u32, mask: u64) -> Vec<String> {
    let mut out = Vec::new();
    let combos: &[(&str, THREAD_ACCESS_RIGHTS)] = &[
        ("QUERY", THREAD_QUERY_INFORMATION),
        ("SET", THREAD_SET_INFORMATION),
        ("QUERY|SET", THREAD_QUERY_INFORMATION | THREAD_SET_INFORMATION),
        (
            "QUERY|SET|SUSPEND",
            THREAD_QUERY_INFORMATION | THREAD_SET_INFORMATION | THREAD_SUSPEND_RESUME,
        ),
    ];
    for (name, rights) in combos {
        unsafe {
            match OpenThread(*rights, false, tid) {
                Ok(h) => {
                    let prev = SetThreadAffinityMask(h, mask as usize);
                    let code = GetLastError().0;
                    let _ = CloseHandle(h);
                    out.push(format!(
                        "open {name}: OK, set->{prev:#X} err={code}"
                    ));
                }
                Err(_) => out.push(format!(
                    "open {name}: DENIED err={}",
                    GetLastError().0
                )),
            }
        }
    }
    out
}
pub fn find_process(exe: &str) -> Option<u32> {
    list_processes()
        .into_iter()
        .find(|(_, n)| n.eq_ignore_ascii_case(exe))
        .map(|(p, _)| p)
}

/// Sample per-thread CPU deltas over `secs`, return (tid, delta_ms) sorted desc.
pub fn sample_thread_load(pid: u32, secs: u64) -> Vec<(u32, f64)> {
    let mut t0: HashMap<u32, u64> = HashMap::new();
    for tid in threads_of(pid) {
        if let Some(v) = thread_cpu_100ns(tid) {
            t0.insert(tid, v);
        }
    }
    std::thread::sleep(std::time::Duration::from_secs(secs));
    let mut out = Vec::new();
    for tid in threads_of(pid) {
        if let (Some(a), Some(b)) = (t0.get(&tid), thread_cpu_100ns(tid)) {
            if b >= *a {
                out.push((tid, (b - a) as f64 / 10_000.0));
            }
        }
    }
    out.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    out
}
