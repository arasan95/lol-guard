use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CoresConfig {
    /// Logical CPU indices reserved for the LoL main thread.
    pub main: Vec<u8>,
    /// Logical CPU indices for LoL sub threads.
    pub sub: Vec<u8>,
    /// Logical CPU indices kept as buffer (neither LoL nor background).
    pub buffers: Vec<u8>,
    /// Sweep interval in seconds.
    pub interval_secs: u64,
    /// Last detected main thread id (stability hysteresis).
    pub last_tid: u32,
}

impl CoresConfig {
    /// Default layout: last logical CPU = main, LP-2 = buffer,
    /// LP-3 = sub, LP-4 = buffer, rest = background apps.
    pub fn default_for(lp: u32) -> Self {
        if lp >= 5 {
            Self {
                main: vec![(lp - 1) as u8],
                sub: vec![(lp - 3) as u8],
                buffers: vec![(lp - 2) as u8, (lp - 4) as u8],
                interval_secs: 45,
                last_tid: 0,
            }
        } else {
            // Fallback for tiny machines: split in halves.
            let half = (lp / 2).max(1) as u8;
            Self {
                main: ((half as u32)..lp).map(|i| i as u8).collect(),
                sub: (0..half).map(|i| i as u8).collect(),
                buffers: vec![],
                interval_secs: 45,
                last_tid: 0,
            }
        }
    }

    fn bits(list: &[u8]) -> u64 {
        let mut m = 0u64;
        for &c in list {
            if c < 64 {
                m |= 1u64 << c;
            }
        }
        m
    }

    pub fn main_mask(&self) -> u64 {
        Self::bits(&self.main)
    }
    pub fn sub_mask(&self) -> u64 {
        Self::bits(&self.sub)
    }
    pub fn buffer_mask(&self) -> u64 {
        Self::bits(&self.buffers)
    }
    pub fn reserved_mask(&self) -> u64 {
        self.main_mask() | self.sub_mask() | self.buffer_mask()
    }
    pub fn bg_mask(&self, lp: u32) -> u64 {
        all_mask(lp) ^ self.reserved_mask()
    }

    /// Keep only CPUs that exist; drop overlaps (main wins over sub,
    /// sub wins over buffers). Returns a human-readable warning, if any.
    pub fn sanitize(&mut self, lp: u32) -> Option<String> {
        let mut warn = String::new();
        let keep = |v: &mut Vec<u8>| {
            v.retain(|&c| (c as u32) < lp);
            v.sort_unstable();
            v.dedup();
        };
        keep(&mut self.main);
        keep(&mut self.sub);
        keep(&mut self.buffers);
        if self.main.is_empty() || self.sub.is_empty() {
            return Some("main/sub must each contain at least one CPU".to_string());
        }
        let before_sub = self.sub.len();
        self.sub.retain(|c| !self.main.contains(c));
        let before_buf = self.buffers.len();
        self.buffers.retain(|c| !self.main.contains(c) && !self.sub.contains(c));
        if self.sub.len() != before_sub || self.buffers.len() != before_buf {
            warn.push_str("overlapping CPUs removed (main wins)");
        }
        if self.interval_secs == 0 {
            self.interval_secs = 45;
        }
        if warn.is_empty() {
            None
        } else {
            Some(warn)
        }
    }

    pub fn load(path: &PathBuf, lp: u32) -> Self {
        let mut cfg = fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str::<CoresConfig>(&s).ok())
            .unwrap_or_else(|| Self::default_for(lp));
        if let Some(w) = cfg.sanitize(lp) {
            let _ = w;
            cfg = Self::default_for(lp);
        }
        cfg
    }

    pub fn save(&self, path: &PathBuf) -> std::io::Result<()> {
        let s = serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_string());
        fs::write(path, s)
    }
}

pub fn all_mask(lp: u32) -> u64 {
    if lp >= 64 {
        u64::MAX
    } else if lp == 0 {
        0
    } else {
        (1u64 << lp) - 1
    }
}

pub fn fmt_mask(m: u64) -> String {
    format!("0x{m:X}")
}
