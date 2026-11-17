//! Button -> keyboard shortcut remapping through a virtual uinput keyboard
//! (works on Wayland and X11 and for games under Proton, since it is a real input device).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use std::time::Instant;

use crate::config::{Action, Remap};
use crate::inputs;
use crate::log::log;

/// What the remap screen shows as "last button pressed" (for learn mode).
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct LastPress {
    pub dev_key: String,
    pub dev_path: String,
    pub usage: u32,
    pub report_id: u8,
    pub bit: u32,
    pub at: Instant,
}

pub static LAST_PRESS: Mutex<Option<LastPress>> = Mutex::new(None);
pub static OVERLAY_TOGGLE: AtomicBool = AtomicBool::new(false);
pub static OVERLAY_NEXT: AtomicBool = AtomicBool::new(false);
pub static ENABLED: AtomicBool = AtomicBool::new(true);
pub static UINPUT_ERROR: Mutex<Option<String>> = Mutex::new(None);
pub static EGUI_CTX: std::sync::OnceLock<egui::Context> = std::sync::OnceLock::new();
/// Profile switch buttons per panel (device key -> button numbers), kept up to date by the window.
pub static PROFILE_SWITCHES: std::sync::LazyLock<Mutex<HashMap<String, Vec<u32>>>> = std::sync::LazyLock::new(Default::default);

/// The profile in force on a panel: of its held profile switches, the one pressed last.
/// None when the panel has no profile switch held.
fn active_profile(dev_key: &str, dev_path: &str, last_down: &HashMap<(String, u32), Instant>) -> Option<u32> {
    let switches = PROFILE_SWITCHES.lock().unwrap().get(dev_key).cloned().unwrap_or_default();
    switches
        .into_iter()
        .filter(|u| inputs::is_down(dev_path, *u))
        .max_by_key(|u| last_down.get(&(dev_key.to_string(), *u)).copied())
}

pub fn start(remaps: Arc<RwLock<Vec<Remap>>>) {
    let rx = inputs::subscribe();
    thread::Builder::new()
        .name("remap".into())
        .spawn(move || {
            let mut kb = Keyboard::new();
            // keys currently held down for a (device, usage)
            let mut held: HashMap<(String, u32), Vec<u16>> = HashMap::new();
            // when each button last went down (to pick between several held profile switches)
            let mut last_down: HashMap<(String, u32), Instant> = HashMap::new();
            for ev in rx {
                if ev.pressed {
                    last_down.insert((ev.dev_key.clone(), ev.usage), Instant::now());
                    *LAST_PRESS.lock().unwrap() = Some(LastPress {
                        dev_key: ev.dev_key.clone(),
                        dev_path: ev.dev_path.clone(),
                        usage: ev.usage,
                        report_id: ev.report_id,
                        bit: ev.bit,
                        at: Instant::now(),
                    });
                }
                if let Some(ctx) = EGUI_CTX.get() {
                    ctx.request_repaint();
                }
                let id = (ev.dev_key.clone(), ev.usage);
                if !ev.pressed {
                    if let Some(codes) = held.remove(&id) {
                        kb.release(&codes);
                    }
                    continue;
                }
                if !ENABLED.load(Ordering::Relaxed) {
                    continue;
                }
                let action = {
                    let list = remaps.read().unwrap();
                    // a mapping only works in the profile it was made in
                    let active = active_profile(&ev.dev_key, &ev.dev_path, &last_down);
                    list.iter()
                        .find(|r| r.device_key == ev.dev_key && r.usage == ev.usage && r.profile == active)
                        .map(|r| (r.action.clone(), r.label.clone()))
                };
                if let Some((a, label)) = &action {
                    if ev.usage < crate::hidreport::AXIS_PSEUDO_BASE {
                        log(format!("Remap: {label} -> {}", a.describe()));
                    }
                }
                let action = action.map(|(a, _)| a);
                match action {
                    Some(Action::Keys(combo)) => match keys::parse_combo(&combo) {
                        Ok(codes) => {
                            kb.press(&codes);
                            held.insert(id, codes);
                        }
                        Err(e) => log(format!("Remap: bad shortcut '{combo}': {e}")),
                    },
                    Some(Action::OverlayToggle) => OVERLAY_TOGGLE.store(true, Ordering::Relaxed),
                    Some(Action::OverlayNextMonitor) => OVERLAY_NEXT.store(true, Ordering::Relaxed),
                    None => {}
                }
            }
        })
        .ok();
}

/// For `uwacs --test-key`: new virtual keyboard, give the desktop a moment to pick it up, tap the combo.
pub fn test_key(combo: &str) -> anyhow::Result<()> {
    let codes = keys::parse_combo(combo).map_err(|e| anyhow::anyhow!(e))?;
    let mut kb = Keyboard::new();
    thread::sleep(std::time::Duration::from_millis(1500));
    kb.press(&codes);
    thread::sleep(std::time::Duration::from_millis(60));
    kb.release(&codes);
    thread::sleep(std::time::Duration::from_millis(300));
    println!("sent {combo} = {codes:?}");
    Ok(())
}

// ------------------------------------------------------------------ virtual keyboard

#[cfg(target_os = "linux")]
struct Keyboard {
    dev: Option<evdev::uinput::VirtualDevice>,
}

#[cfg(target_os = "linux")]
impl Keyboard {
    fn new() -> Self {
        let dev = (|| -> std::io::Result<evdev::uinput::VirtualDevice> {
            let mut set = evdev::AttributeSet::<evdev::KeyCode>::new();
            for (_, code) in keys::TABLE {
                set.insert(evdev::KeyCode(*code));
            }
            evdev::uinput::VirtualDevice::builder()?.name("UWACS virtual keyboard").with_keys(&set)?.build()
        })();
        match dev {
            Ok(d) => {
                log("Remap: virtual keyboard ready");
                Self { dev: Some(d) }
            }
            Err(e) => {
                let msg = format!("cannot create virtual keyboard via /dev/uinput ({e}); add yourself to the 'input' group");
                log(format!("Remap: {msg}"));
                *UINPUT_ERROR.lock().unwrap() = Some(msg);
                Self { dev: None }
            }
        }
    }

    fn emit(&mut self, code: u16, value: i32) {
        if let Some(d) = self.dev.as_mut() {
            let ev = evdev::InputEvent::new(evdev::EventType::KEY.0, code, value);
            let _ = d.emit(&[ev]);
        }
    }

    fn press(&mut self, codes: &[u16]) {
        for &c in codes {
            self.emit(c, 1);
        }
    }

    fn release(&mut self, codes: &[u16]) {
        for &c in codes.iter().rev() {
            self.emit(c, 0);
        }
    }
}

#[cfg(not(target_os = "linux"))]
struct Keyboard;

#[cfg(not(target_os = "linux"))]
impl Keyboard {
    fn new() -> Self {
        *UINPUT_ERROR.lock().unwrap() = Some("key output is only implemented on Linux so far".into());
        Keyboard
    }
    fn press(&mut self, _: &[u16]) {}
    fn release(&mut self, _: &[u16]) {}
}

pub mod keys {
    //! Shortcut names <-> Linux key codes.

    /// (name, linux KEY_* code). Names are matched case-insensitively; aliases below.
    pub const TABLE: &[(&str, u16)] = &[
        ("Ctrl", 29), ("RCtrl", 97), ("Shift", 42), ("RShift", 54), ("Alt", 56), ("AltGr", 100), ("Super", 125),
        ("Esc", 1), ("1", 2), ("2", 3), ("3", 4), ("4", 5), ("5", 6), ("6", 7), ("7", 8), ("8", 9), ("9", 10), ("0", 11),
        ("Minus", 12), ("Equal", 13), ("Backspace", 14), ("Tab", 15),
        ("Q", 16), ("W", 17), ("E", 18), ("R", 19), ("T", 20), ("Y", 21), ("U", 22), ("I", 23), ("O", 24), ("P", 25),
        ("LeftBracket", 26), ("RightBracket", 27), ("Enter", 28),
        ("A", 30), ("S", 31), ("D", 32), ("F", 33), ("G", 34), ("H", 35), ("J", 36), ("K", 37), ("L", 38),
        ("Semicolon", 39), ("Apostrophe", 40), ("Grave", 41), ("Backslash", 43),
        ("Z", 44), ("X", 45), ("C", 46), ("V", 47), ("B", 48), ("N", 49), ("M", 50), ("Comma", 51), ("Dot", 52), ("Slash", 53),
        ("NumMultiply", 55), ("Space", 57), ("CapsLock", 58),
        ("F1", 59), ("F2", 60), ("F3", 61), ("F4", 62), ("F5", 63), ("F6", 64), ("F7", 65), ("F8", 66), ("F9", 67), ("F10", 68),
        ("NumLock", 69), ("ScrollLock", 70),
        ("Num7", 71), ("Num8", 72), ("Num9", 73), ("NumMinus", 74), ("Num4", 75), ("Num5", 76), ("Num6", 77), ("NumPlus", 78),
        ("Num1", 79), ("Num2", 80), ("Num3", 81), ("Num0", 82), ("NumDot", 83),
        ("F11", 87), ("F12", 88), ("NumEnter", 96), ("NumDivide", 98), ("PrintScreen", 99),
        ("Home", 102), ("Up", 103), ("PageUp", 104), ("Left", 105), ("Right", 106), ("End", 107), ("Down", 108),
        ("PageDown", 109), ("Insert", 110), ("Delete", 111), ("Pause", 119),
        ("F13", 183), ("F14", 184), ("F15", 185), ("F16", 186), ("F17", 187), ("F18", 188), ("F19", 189), ("F20", 190),
        ("F21", 191), ("F22", 192), ("F23", 193), ("F24", 194),
    ];

    const ALIASES: &[(&str, &str)] = &[
        ("control", "Ctrl"), ("lctrl", "Ctrl"), ("leftctrl", "Ctrl"), ("rightctrl", "RCtrl"),
        ("lshift", "Shift"), ("leftshift", "Shift"), ("rightshift", "RShift"),
        ("lalt", "Alt"), ("leftalt", "Alt"), ("ralt", "AltGr"), ("rightalt", "AltGr"),
        ("meta", "Super"), ("win", "Super"), ("cmd", "Super"),
        ("escape", "Esc"), ("return", "Enter"), ("del", "Delete"), ("ins", "Insert"),
        ("arrowup", "Up"), ("arrowdown", "Down"), ("arrowleft", "Left"), ("arrowright", "Right"),
        ("pgup", "PageUp"), ("pgdn", "PageDown"), ("equals", "Equal"), ("period", "Dot"), ("backtick", "Grave"),
        ("openbracket", "LeftBracket"), ("closebracket", "RightBracket"), ("quote", "Apostrophe"),
        ("-", "Minus"), ("=", "Equal"), (",", "Comma"), (".", "Dot"), ("/", "Slash"), (";", "Semicolon"),
        ("'", "Apostrophe"), ("`", "Grave"), ("\\", "Backslash"), ("[", "LeftBracket"), ("]", "RightBracket"),
        ("plus", "Equal"), ("kp0", "Num0"), ("kp1", "Num1"), ("kp2", "Num2"), ("kp3", "Num3"), ("kp4", "Num4"),
        ("kp5", "Num5"), ("kp6", "Num6"), ("kp7", "Num7"), ("kp8", "Num8"), ("kp9", "Num9"),
    ];

    pub fn lookup(name: &str) -> Option<u16> {
        let n = name.trim();
        let canon = ALIASES.iter().find(|(a, _)| a.eq_ignore_ascii_case(n)).map(|(_, c)| *c).unwrap_or(n);
        TABLE.iter().find(|(k, _)| k.eq_ignore_ascii_case(canon)).map(|(_, c)| *c)
    }

    /// "Ctrl+Shift+F13" -> key codes in press order.
    pub fn parse_combo(s: &str) -> Result<Vec<u16>, String> {
        if s.trim().is_empty() {
            return Err("empty".into());
        }
        // allow a literal "+" key written as "Ctrl++"
        let mut parts: Vec<String> = Vec::new();
        for p in s.split('+') {
            if p.is_empty() {
                if parts.last().map(|x| x.is_empty()).unwrap_or(false) {
                    continue;
                }
                parts.push(String::new());
            } else {
                parts.push(p.to_string());
            }
        }
        parts
            .iter()
            .map(|p| if p.is_empty() { "plus".to_string() } else { p.clone() })
            .map(|p| lookup(&p).ok_or_else(|| format!("unknown key '{}'", p.trim())))
            .collect()
    }

    /// Build a shortcut string from an egui key event (for "press a key to record").
    pub fn from_egui(key: egui::Key, m: egui::Modifiers) -> Option<String> {
        let name = key.name();
        let main = lookup(name).map(|_| name.to_string()).or_else(|| {
            let alt = match key {
                egui::Key::Num0 => "0",
                egui::Key::Num1 => "1",
                egui::Key::Num2 => "2",
                egui::Key::Num3 => "3",
                egui::Key::Num4 => "4",
                egui::Key::Num5 => "5",
                egui::Key::Num6 => "6",
                egui::Key::Num7 => "7",
                egui::Key::Num8 => "8",
                egui::Key::Num9 => "9",
                _ => return None,
            };
            Some(alt.to_string())
        })?;
        // Use the table's own spelling, e.g. egui "ArrowUp" -> "Up".
        let code = lookup(&main)?;
        let canon = TABLE.iter().find(|(_, c)| *c == code).map(|(k, _)| k.to_string()).unwrap_or(main);
        let mut out = Vec::new();
        if m.ctrl {
            out.push("Ctrl".to_string());
        }
        if m.shift {
            out.push("Shift".to_string());
        }
        if m.alt {
            out.push("Alt".to_string());
        }
        out.push(canon);
        Some(out.join("+"))
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn combos() {
            assert_eq!(parse_combo("Ctrl+Shift+F13").unwrap(), vec![29, 42, 183]);
            assert_eq!(parse_combo("alt+ArrowUp").unwrap(), vec![56, 103]);
            assert_eq!(parse_combo("RAlt+kp5").unwrap(), vec![100, 76]);
            assert!(parse_combo("Ctrl+Bogus").is_err());
            assert_eq!(from_egui(egui::Key::F13, egui::Modifiers { ctrl: true, ..Default::default() }).unwrap(), "Ctrl+F13");
        }
    }
}
