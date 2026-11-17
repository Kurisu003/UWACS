//! Shared log console (the old "debug console" in the Tauri UI), also printed to stderr.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::SystemTime;

static LINES: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());
const MAX_LINES: usize = 2000;

pub fn log(msg: impl Into<String>) {
    let msg = msg.into();
    let secs = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (h, m, s) = ((secs / 3600 + local_offset_hours()) % 24, (secs / 60) % 60, secs % 60);
    let line = format!("{h:02}:{m:02}:{s:02}  {msg}");
    eprintln!("{line}");
    let mut g = LINES.lock().unwrap();
    g.push_back(line);
    while g.len() > MAX_LINES {
        g.pop_front();
    }
}

pub fn lines() -> Vec<String> {
    LINES.lock().unwrap().iter().cloned().collect()
}

pub fn clear() {
    LINES.lock().unwrap().clear();
}

fn local_offset_hours() -> u64 {
    // Cheap local-time offset from the TZ database via `date +%z`, computed once.
    static OFF: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *OFF.get_or_init(|| {
        std::process::Command::new("date")
            .arg("+%z")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .and_then(|s| {
                let s = s.trim();
                let sign = if s.starts_with('-') { -1i64 } else { 1 };
                s.get(1..3)?.parse::<i64>().ok().map(|h| (24 + sign * h) as u64)
            })
            .unwrap_or(0)
    })
}
