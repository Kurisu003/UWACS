//! PFP writer (was pfp_writer): shows aircraft pages on the WinWing PFP 3N / PFP 7 screen,
//! sends PFP keys to DCS-BIOS for the A-10C II CDU and the AH-64D KU, and has the RWR
//! "search mode" on the MENU key. Packet format and the custom font upload are unchanged.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use hidapi::HidDevice;
use phf::phf_map;

use crate::dcsbios;
use crate::devices;
use crate::inputs;
use crate::log::log;
use crate::pfp_aircraft as ac;
use crate::status::{set_status, Status};

const WRITE_DELAY_SHORT: Duration = Duration::from_millis(5);
const WRITE_DELAY_LONG: Duration = Duration::from_millis(10);
const THREAD_SLEEP: Duration = Duration::from_millis(10);
const REPEAT_AFTER: Duration = Duration::from_millis(450);
const REPEAT_EVERY: Duration = Duration::from_millis(120);
pub const COLS: usize = 24;
pub const ROWS: usize = 14;

/// The custom font + screen init captured from SimAppPro (was output4.txt next to the exe).
const INIT_PACKETS: &str = include_str!("../assets/pfp_init.txt");

#[derive(Clone, Debug, PartialEq)]
pub struct TextBlock {
    pub text: String,
    pub bg: &'static str,
    pub fg: &'static str,
}

impl TextBlock {
    pub fn new(text: impl Into<String>, fg: &'static str) -> Self {
        Self { text: text.into(), bg: "black", fg }
    }
}

/// Key presses seen this loop, with auto-repeat while held.
pub struct Keys {
    prev: [bool; 72],
    now: [bool; 72],
    held_since: [Option<Instant>; 72],
    last_repeat: [Option<Instant>; 72],
    fired: [bool; 72],
}

impl Keys {
    fn new() -> Self {
        Self { prev: [false; 72], now: [false; 72], held_since: [None; 72], last_repeat: [None; 72], fired: [false; 72] }
    }

    fn update(&mut self, state: [bool; 72]) {
        self.prev = self.now;
        self.now = state;
        let t = Instant::now();
        for i in 0..72 {
            self.fired[i] = false;
            if self.now[i] && !self.prev[i] {
                self.fired[i] = true;
                self.held_since[i] = Some(t);
                self.last_repeat[i] = Some(t);
            } else if self.now[i] {
                if let (Some(h), Some(r)) = (self.held_since[i], self.last_repeat[i]) {
                    if t - h >= REPEAT_AFTER && t - r >= REPEAT_EVERY {
                        self.fired[i] = true;
                        self.last_repeat[i] = Some(t);
                    }
                }
            } else {
                self.held_since[i] = None;
            }
        }
    }

    /// True once per press (and on auto-repeat while held).
    pub fn pressed(&self, name: &str) -> bool {
        PFP_KEYCODES.get(name).map(|&i| self.fired[i as usize]).unwrap_or(false)
    }

    /// True once per press only (no repeat), for mode toggles.
    pub fn just_pressed(&self, name: &str) -> bool {
        PFP_KEYCODES.get(name).map(|&i| self.now[i as usize] && !self.prev[i as usize]).unwrap_or(false)
    }

    /// The character key pressed this loop (digits, letters, '.', '/'), if any.
    pub fn typed_char(&self) -> Option<char> {
        for i in 29..=37usize {
            if self.fired[i] {
                return char::from_digit(i as u32 - 28, 10);
            }
        }
        if self.fired[38] {
            return Some('.');
        }
        if self.fired[39] {
            return Some('0');
        }
        for i in 41..=66usize {
            if self.fired[i] {
                return Some((b'A' + (i as u8 - 41)) as char);
            }
        }
        if self.fired[69] {
            return Some('/');
        }
        None
    }
}

pub fn run(path: String, stop: Arc<AtomicBool>, auto_buttons: Arc<AtomicBool>) {
    let dev = match devices::open(&path) {
        Ok(d) => d,
        Err(e) => {
            log(format!("Error: PFP_WRITER: No HID device ({e})"));
            set_status("pfp", Status::Error(e.to_string()));
            return;
        }
    };
    set_status("pfp", Status::Waiting("sending font".into()));
    send_init(&dev);
    log("PFP_WRITER: Sent init packets");
    set_status("pfp", Status::Running);

    let mut keys = Keys::new();
    let mut search = Search::default();
    let mut search_mode = false;
    let mut last_sent: Vec<TextBlock> = Vec::new();
    let mut last_send_time = Instant::now();

    while !stop.load(Ordering::Relaxed) {
        keys.update(inputs::pfp_pressed(&path));
        if keys.just_pressed("MENU") && auto_buttons.load(Ordering::Relaxed) {
            search_mode = !search_mode;
        }

        let blocks = if search_mode {
            search.display(&keys)
        } else {
            let snap = dcsbios::bios().snapshot();
            let module = dcsbios::module_name(&snap);
            let auto = auto_buttons.load(Ordering::Relaxed);
            if let Some(p) = (!module.is_empty()).then(|| crate::custom::pfp_for(&module)).flatten() {
                crate::custom::pfp_blocks(&p, &snap, &crate::custom::catalog(&module))
            } else if module.starts_with("A-10C_2") {
                if auto {
                    ac::handle_a10c2_input(&keys);
                }
                ac::get_a10c2_text(&snap)
            } else if module.starts_with("AV8B") {
                ac::get_av8b_text(&snap)
            } else if module.starts_with("CH-47F") {
                ac::get_ch47f_text(&snap)
            } else if module.starts_with("AH-64D_BLK_II") {
                if auto {
                    ac::handle_ah64d_input(&snap, &keys);
                }
                ac::get_ah64d_text(&snap)
            } else {
                idle_page(&module)
            }
        };

        if blocks != last_sent || last_send_time.elapsed() > Duration::from_secs(2) {
            send_text_to_disp(&dev, &blocks);
            last_sent = blocks;
            last_send_time = Instant::now();
        }
        thread::sleep(THREAD_SLEEP);
    }
    set_status("pfp", Status::Stopped);
    log("PFP_WRITER: stopped");
}

fn idle_page(module: &str) -> Vec<TextBlock> {
    let pad = |s: &str| format!("{s:<COLS$}");
    let mut t = String::new();
    t += &pad("");
    t += &pad("         UWACS");
    t += &pad("");
    match dcsbios::bios().connected() {
        None => {
            t += &pad(" WAITING FOR DCS-BIOS");
        }
        Some(_) if module.is_empty() => {
            t += &pad(" DCS-BIOS CONNECTED");
            t += &pad(" NO AIRCRAFT YET");
        }
        Some(_) => {
            t += &pad(" NO PFP PAGE FOR");
            t += &pad(&format!(" {}", module.chars().take(COLS - 1).collect::<String>()));
        }
    }
    t += &pad("");
    t += &pad(" MENU  RWR SEARCH");
    vec![TextBlock::new(t, "green")]
}

// ------------------------------------------------------------------ RWR search mode

#[derive(Default)]
struct Search {
    text: String,
}

impl Search {
    fn display(&mut self, keys: &Keys) -> Vec<TextBlock> {
        if keys.pressed("CLR") {
            self.text.clear();
        }
        if keys.pressed("DEL") {
            self.text.pop();
        }
        if let Some(c) = keys.typed_char() {
            if self.text.len() < 9 {
                self.text.push(c);
            }
        }
        let code = crate::rwr::search_string_to_lookup_code(&self.text);
        let info = crate::rwr::rwr_info_lut(&code);
        let line = |label: &str, v: &str| {
            let mut s = format!("{label}{v}");
            s.truncate(COLS);
            format!("{s:<COLS$}")
        };
        let body = [
            line("ABRV: ", info.abrv),
            line("NATO: ", info.nato_name),
            line("RWR: ", info.rwr_code),
            line("RNG NM: ", info.rng_nm),
            line("ALT FT: ", info.alt_ft),
            line("FLARE: ", info.flr),
            line("CHAFF: ", info.chf),
            line("ECM: ", info.ecm),
            line("LOCK TIME: ", info.lock_time),
            line("GUIDANCE: ", info.guidance),
        ]
        .concat();
        vec![
            TextBlock::new(format!("SEARCH STRING: {:<9}", self.text), "green"),
            TextBlock::new(body.to_uppercase(), "green"),
        ]
    }
}

// ------------------------------------------------------------------ HID packets

fn fix_text_length(mut blocks: Vec<TextBlock>) -> Vec<TextBlock> {
    let max_len = COLS * ROWS;
    let total: usize = blocks.iter().map(|b| b.text.len()).sum();
    if total > max_len {
        let mut remaining = max_len;
        for b in blocks.iter_mut() {
            if remaining == 0 {
                b.text.clear();
            } else if b.text.len() > remaining {
                b.text = b.text.chars().take(remaining).collect();
                remaining = 0;
            } else {
                remaining -= b.text.len();
            }
        }
    } else if total < max_len {
        if let Some(last) = blocks.last_mut() {
            last.text.push_str(&" ".repeat(max_len - total));
        }
    }
    blocks
}

fn fg_index(c: &str) -> u8 {
    match c {
        "orange" => 0,
        "white" => 1,
        "cyan" => 2,
        "green" => 3,
        "magenta" => 4,
        "red" => 5,
        "yellow" => 6,
        _ => 1,
    }
}

fn bg_index(c: &str) -> u8 {
    match c {
        "black" => 0,
        "green" => 1,
        "gray" => 2,
        "orange" => 3,
        "purple" => 4,
        _ => 0,
    }
}

pub fn text_to_packets(blocks: &[TextBlock]) -> Vec<Vec<u8>> {
    let mut payload = Vec::new();
    for b in fix_text_length(blocks.to_vec()) {
        let prefix = 0x21 + 0x21 * fg_index(b.fg) + 0xC * bg_index(b.bg);
        for c in b.text.chars() {
            payload.extend([prefix, 0x00, if c.is_ascii() { c as u8 } else { b'?' }]);
        }
    }
    let mut packets = Vec::new();
    for chunk in payload.chunks(63) {
        let mut c = chunk.to_vec();
        for _ in 0..(63 - c.len()) / 3 {
            c.extend_from_slice(&[0x42, 0x00, 0x20]);
        }
        c.resize(63, 0);
        let mut p = Vec::with_capacity(64);
        p.push(0xF2);
        p.extend_from_slice(&c);
        packets.push(p);
    }
    packets
}

pub fn send_text_to_disp(dev: &HidDevice, blocks: &[TextBlock]) {
    for p in text_to_packets(blocks) {
        let _ = dev.write(&p);
        thread::sleep(WRITE_DELAY_LONG);
    }
}

pub fn init_packets() -> Vec<Vec<u8>> {
    INIT_PACKETS
        .lines()
        .filter_map(|line| {
            let t: String = line.split('#').next().unwrap_or("").split_whitespace().collect();
            if t.is_empty() || t.len() % 2 != 0 {
                return None;
            }
            (0..t.len() / 2).map(|i| u8::from_str_radix(&t[2 * i..2 * i + 2], 16).ok()).collect()
        })
        .collect()
}

fn send_init(dev: &HidDevice) {
    for p in init_packets() {
        if let Err(e) = dev.write(&p) {
            log(format!("PFP_WRITER: init write failed: {e}"));
        }
        thread::sleep(WRITE_DELAY_SHORT);
    }
}

// ------------------------------------------------------------------ key table (unchanged)

pub static PFP_KEYCODES: phf::Map<&'static str, u8> = phf_map! {
    "L1" => 0, "L2" => 1, "L3" => 2, "L4" => 3, "L5" => 4, "L6" => 5,
    "R1" => 6, "R2" => 7, "R3" => 8, "R4" => 9, "R5" => 10, "R6" => 11,
    "INIT REF" => 12, "RTE" => 13, "CLB" => 14, "CRZ" => 15, "DES" => 16,
    "BRT-" => 17, "BRT+" => 18, "MENU" => 19, "LEGS" => 20, "DEP ARR" => 21,
    "HOLD" => 22, "PROG" => 23, "EXEC" => 24, "N1 LIMIT" => 25, "FIX" => 26,
    "PREV PAGE" => 27, "NEXT PAGE" => 28,
    "1" => 29, "2" => 30, "3" => 31, "4" => 32, "5" => 33, "6" => 34, "7" => 35, "8" => 36, "9" => 37,
    "." => 38, "0" => 39, "+-" => 40,
    "A" => 41, "B" => 42, "C" => 43, "D" => 44, "E" => 45, "F" => 46, "G" => 47, "H" => 48, "I" => 49,
    "J" => 50, "K" => 51, "L" => 52, "M" => 53, "N" => 54, "O" => 55, "P" => 56, "Q" => 57, "R" => 58,
    "S" => 59, "T" => 60, "U" => 61, "V" => 62, "W" => 63, "X" => 64, "Y" => 65, "Z" => 66,
    "SP" => 67, "DEL" => 68, "/" => 69, "CLR" => 70,
};

/// Name of a PFP key by its bit index in report 1, for the remap screen.
pub fn key_name(bit: u32) -> Option<&'static str> {
    static REV: std::sync::OnceLock<HashMap<u8, &'static str>> = std::sync::OnceLock::new();
    REV.get_or_init(|| PFP_KEYCODES.entries().map(|(k, v)| (*v, *k)).collect()).get(&(bit as u8)).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_packets_are_64_bytes_and_cover_screen() {
        let p = text_to_packets(&[TextBlock::new("HELLO", "green")]);
        assert!(p.iter().all(|x| x.len() == 64 && x[0] == 0xF2));
        // 24*14 chars * 3 bytes = 1008 bytes -> 16 packets of 63
        assert_eq!(p.len(), 16);
        assert_eq!(&p[0][1..4], &[0x21 + 0x21 * 3, 0x00, b'H']);
    }

    #[test]
    fn init_file_parses() {
        let p = init_packets();
        assert!(p.len() > 600, "{}", p.len());
        assert!(p.iter().all(|x| !x.is_empty()));
    }
}
