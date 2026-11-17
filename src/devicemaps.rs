//! Schematic pictures of every WinWing device type, with each button placed roughly where it sits
//! on the real panel. Drawn from scratch (no WinWing artwork).
//!
//! Button numbers: "known" ones come from captures published in dcs-signal-converter
//! (MIT, Cory Bass, data/devices/winctrl.json) and, for the PFP, from the old pfp_writer key table;
//! "guessed" ones are a sensible numbering not confirmed on hardware; the rest have no number until
//! the user teaches them (click the key, press the real button).

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Known,
    Guessed,
    Unknown,
}

#[derive(Clone, Debug)]
pub struct Ctrl {
    pub id: String,
    pub label: String,
    /// x, y, w, h in template units
    pub rect: [f32; 4],
    pub round: bool,
    pub button: Option<u32>,
    pub source: Source,
    /// Holding this button selects a mapping profile (e.g. the MFD side switch).
    pub profile_switch: bool,
}

#[derive(Clone, Debug)]
pub enum DecoKind {
    Body,
    Screen(&'static str),
    Knob(&'static str),
    Text(&'static str),
    /// Live value of axis n (a wheel or slider), drawn as a bar.
    #[allow(dead_code)]
    Axis(usize),
}

#[derive(Clone, Debug)]
pub struct Deco {
    pub rect: [f32; 4],
    pub kind: DecoKind,
}

#[derive(Clone, Debug)]
pub struct Template {
    /// Stable id for taught-button storage, e.g. "mfd", "pfp3n".
    pub id: String,
    pub name: String,
    pub size: [f32; 2],
    pub controls: Vec<Ctrl>,
    pub decos: Vec<Deco>,
    /// Built from the HID descriptor because no drawing exists for this device.
    pub generic: bool,
}

impl Template {
    fn new(id: &str, name: &str, w: f32, h: f32) -> Self {
        let mut t = Self { id: id.into(), name: name.into(), size: [w, h], controls: Vec::new(), decos: Vec::new(), generic: false };
        t.decos.push(Deco { rect: [0.0, 0.0, w, h], kind: DecoKind::Body });
        t
    }

    fn key(&mut self, id: &str, label: &str, x: f32, y: f32, w: f32, h: f32, button: Option<u32>, source: Source) {
        let source = if button.is_none() { Source::Unknown } else { source };
        self.controls.push(Ctrl { id: id.into(), label: label.into(), rect: [x, y, w, h], round: false, button, source, profile_switch: false });
    }

    fn known(&mut self, id: &str, label: &str, x: f32, y: f32, w: f32, h: f32, b: u32) {
        self.key(id, label, x, y, w, h, Some(b), Source::Known);
    }

    fn guess(&mut self, id: &str, label: &str, x: f32, y: f32, w: f32, h: f32, b: u32) {
        self.key(id, label, x, y, w, h, Some(b), Source::Guessed);
    }

    fn todo(&mut self, id: &str, label: &str, x: f32, y: f32, w: f32, h: f32) {
        self.key(id, label, x, y, w, h, None, Source::Unknown);
    }

    fn round_todo(&mut self, id: &str, label: &str, cx: f32, cy: f32, r: f32) {
        self.key(id, label, cx - r, cy - r, 2.0 * r, 2.0 * r, None, Source::Unknown);
        self.controls.last_mut().unwrap().round = true;
    }

    fn screen(&mut self, x: f32, y: f32, w: f32, h: f32, label: &'static str) {
        self.decos.push(Deco { rect: [x, y, w, h], kind: DecoKind::Screen(label) });
    }

    fn knob(&mut self, cx: f32, cy: f32, r: f32, label: &'static str) {
        self.decos.push(Deco { rect: [cx - r, cy - r, 2.0 * r, 2.0 * r], kind: DecoKind::Knob(label) });
    }

    fn text(&mut self, x: f32, y: f32, label: &'static str) {
        self.decos.push(Deco { rect: [x, y, 0.0, 0.0], kind: DecoKind::Text(label) });
    }

    /// Button number for a control after the user's taught corrections.
    pub fn button_of(&self, c: &Ctrl, overrides: Option<&std::collections::BTreeMap<String, u32>>) -> Option<u32> {
        // a taught 0 means "its built-in number belongs to another button now"
        match overrides.and_then(|o| o.get(&c.id).copied()) {
            Some(0) => None,
            Some(v) => Some(v),
            None => c.button,
        }
    }
}

// ------------------------------------------------------------------ device table

/// (USB product id, template builder, variant name). PIDs from dcs-signal-converter's winctrl.json.
pub fn known_pids() -> Vec<(u16, &'static str, &'static str)> {
    vec![
        (0xBEDE, "ufc", "CarrierAce UFC + HUD"),
        (0xBEE1, "mfd", "CarrierAce MFD L"),
        (0xBEE0, "mfd", "CarrierAce MFD C"),
        (0xBEE2, "mfd", "CarrierAce MFD R"),
        (0xBF06, "icp", "ViperAce ICP"),
        (0xBF05, "pto2", "CarrierAce PTO 2"),
        (0xBD64, "throttle", "Orion Throttle Base II"),
        (0xBEF0, "pedals", "Orion Combat Rudder Pedals"),
        (0xBB36, "mcdu", "MCDU Captain"),
        (0xBB3E, "mcdu", "MCDU Co-Pilot"),
        (0xBB3A, "mcdu", "MCDU Observer"),
        (0xBB35, "pfp3n", "PFP-3N Captain"),
        (0xBB3D, "pfp3n", "PFP-3N Co-Pilot"),
        (0xBB39, "pfp3n", "PFP-3N Observer"),
        (0xBB37, "pfp7", "PFP-7 Captain"),
        (0xBB3F, "pfp7", "PFP-7 Co-Pilot"),
        (0xBB3B, "pfp7", "PFP-7 Observer"),
        (0xBB38, "pfp4", "PFP-4 Captain"),
        (0xBB40, "pfp4", "PFP-4 Co-Pilot"),
        (0xBB3C, "pfp4", "PFP-4 Observer"),
    ]
}

pub fn model_name(pid: u16) -> Option<&'static str> {
    known_pids().into_iter().find(|(p, _, _)| *p == pid).map(|(_, _, n)| n)
}

/// Picture for a connected device. `usages` (from its HID descriptor) is used for devices
/// without a drawing.
pub fn template_for(pid: u16, usages: &[u32]) -> Template {
    let kind = known_pids().into_iter().find(|(p, _, _)| *p == pid).map(|(_, k, _)| k).unwrap_or("");
    let mut t = build(kind).unwrap_or_else(|| generic(&format!("WinWing {pid:04X}"), usages));
    if let Some(n) = model_name(pid) {
        t.name = n.to_string();
    }
    if t.generic && !usages.is_empty() {
        t = generic(&t.name, usages);
    }
    t
}

/// One picture per device type, for the gallery.
pub fn gallery() -> Vec<Template> {
    ["ufc", "mfd", "icp", "pto2", "pfp3n", "pfp7", "pfp4", "mcdu"].iter().filter_map(|k| build(k)).collect()
}

fn build(kind: &str) -> Option<Template> {
    Some(match kind {
        "ufc" => ufc(),
        "mfd" => mfd(),
        "icp" => icp(),
        "pto2" => pto2(),
        "pfp3n" => pfp(
            "pfp3n",
            "PFP-3N",
            [("INIT REF", 13), ("RTE", 14), ("CLB", 15), ("CRZ", 16), ("DES", 17)],
            [("MENU", 20), ("LEGS", 21), ("DEP ARR", 22), ("HOLD", 23), ("PROG", 24), ("EXEC", 25)],
            [("N1 LIMIT", 26), ("FIX", 27)],
        ),
        "pfp7" => pfp(
            "pfp7",
            "PFP-7",
            [("INIT REF", 13), ("RTE", 14), ("DEP ARR", 15), ("ALTN", 16), ("VNAV", 17)],
            [("FIX", 20), ("LEGS", 21), ("HOLD", 22), ("FMC COMM", 23), ("PROG", 24), ("EXEC", 25)],
            [("MENU", 26), ("NAV RAD", 27)],
        ),
        "pfp4" => pfp(
            "pfp4",
            "PFP-4",
            [("INIT REF", 13), ("RTE", 14), ("DEP ARR", 15), ("ATC", 16), ("VNAV", 17)],
            [("FIX", 20), ("LEGS", 21), ("HOLD", 22), ("FMC COMM", 23), ("PROG", 24), ("EXEC", 25)],
            [("MENU", 26), ("NAV RAD", 27)],
        ),
        "mcdu" => mcdu(),
        _ => return None,
    })
}

// ------------------------------------------------------------------ drawings

/// Boeing-style CDU keyboards. Numbering is the same for the whole family:
/// LSK 1L-6L = 1-6, 1R-6R = 7-12, function keys 13-29, digits 30-41, letters 42-67, SP DEL / CLR 68-71.
fn pfp(id: &str, name: &str, row1: [(&str, u32); 5], row2: [(&str, u32); 6], row3: [(&str, u32); 2]) -> Template {
    let mut t = Template::new(id, name, 440.0, 690.0);
    t.screen(78.0, 26.0, 284.0, 216.0, "");
    for i in 0..6 {
        let y = 40.0 + i as f32 * 33.0;
        t.known(&format!("lsk{}l", i + 1), &format!("{}L", i + 1), 18.0, y, 46.0, 24.0, i + 1);
        t.known(&format!("lsk{}r", i + 1), &format!("{}R", i + 1), 376.0, y, 46.0, 24.0, i + 7);
    }
    let fw = 62.0;
    for (i, (l, b)) in row1.iter().enumerate() {
        t.known(&key_id(l), l, 22.0 + i as f32 * 68.0, 262.0, fw, 30.0, *b);
    }
    t.known("dim", "DIM", 372.0, 262.0, 46.0, 15.0, 18);
    t.known("brt", "BRT", 372.0, 278.0, 46.0, 15.0, 19);
    for (i, (l, b)) in row2.iter().enumerate() {
        t.known(&key_id(l), l, 22.0 + i as f32 * 68.0, 302.0, fw, 30.0, *b);
    }
    for (i, (l, b)) in row3.iter().enumerate() {
        t.known(&key_id(l), l, 22.0 + i as f32 * 68.0, 342.0, fw, 30.0, *b);
    }
    t.known("prev_page", "PREV PAGE", 158.0, 342.0, fw, 30.0, 28);
    t.known("next_page", "NEXT PAGE", 226.0, 342.0, fw, 30.0, 29);
    // digits
    let digits = [("1", 30), ("2", 31), ("3", 32), ("4", 33), ("5", 34), ("6", 35), ("7", 36), ("8", 37), ("9", 38), (".", 39), ("0", 40), ("+/-", 41)];
    for (i, (l, b)) in digits.iter().enumerate() {
        let (c, r) = (i % 3, i / 3);
        t.known(&format!("d{}", key_id(l)), l, 24.0 + c as f32 * 44.0, 392.0 + r as f32 * 44.0, 38.0, 36.0, *b);
    }
    // letters
    let letters: Vec<(String, u32)> = (0..26u32).map(|i| (((b'A' + i as u8) as char).to_string(), 42 + i)).collect();
    let mut all: Vec<(String, u32)> = letters;
    all.extend([("SP".to_string(), 68), ("DEL".to_string(), 69), ("/".to_string(), 70), ("CLR".to_string(), 71)]);
    for (i, (l, b)) in all.iter().enumerate() {
        let (c, r) = (i % 5, i / 5);
        t.known(&format!("k{}", key_id(l)), l, 178.0 + c as f32 * 48.0, 392.0 + r as f32 * 44.0, 42.0, 36.0, *b);
    }
    t
}

fn mcdu() -> Template {
    let mut t = Template::new("mcdu", "MCDU", 440.0, 690.0);
    t.screen(78.0, 26.0, 284.0, 216.0, "");
    for i in 0..6u32 {
        let y = 40.0 + i as f32 * 33.0;
        t.known(&format!("lsk{}l", i + 1), &format!("{}L", i + 1), 18.0, y, 46.0, 24.0, i + 1);
        if i == 0 {
            t.known("lsk1r", "1R", 376.0, y, 46.0, 24.0, 7);
        } else {
            t.guess(&format!("lsk{}r", i + 1), &format!("{}R", i + 1), 376.0, y, 46.0, 24.0, i + 7);
        }
    }
    let rows: [&[&str]; 3] = [&["DIR", "PROG", "PERF", "INIT", "DATA"], &["F-PLN", "RAD NAV", "FUEL PRED", "SEC F-PLN", "ATC COMM", "MCDU MENU"], &["AIRPORT", "LEFT", "UP", "RIGHT", "DOWN"]];
    for (r, row) in rows.iter().enumerate() {
        for (i, l) in row.iter().enumerate() {
            let id = match *l {
                "←" => "left".to_string(),
                "↑" => "up".to_string(),
                "→" => "right".to_string(),
                "↓" => "down".to_string(),
                _ => key_id(l),
            };
            t.todo(&id, l, 22.0 + i as f32 * 68.0, 262.0 + r as f32 * 40.0, 62.0, 30.0);
        }
    }
    t.todo("brt", "BRT", 372.0, 262.0, 46.0, 15.0);
    t.todo("dim", "DIM", 372.0, 278.0, 46.0, 15.0);
    for (i, l) in ["1", "2", "3", "4", "5", "6", "7", "8", "9", ".", "0", "+/-"].iter().enumerate() {
        t.todo(&format!("d{}", key_id(l)), l, 24.0 + (i % 3) as f32 * 44.0, 392.0 + (i / 3) as f32 * 44.0, 38.0, 36.0);
    }
    let mut keys: Vec<String> = (0..26u8).map(|i| ((b'A' + i) as char).to_string()).collect();
    keys.extend(["/", "SP", "OVFY", "CLR"].iter().map(|s| s.to_string()));
    for (i, l) in keys.iter().enumerate() {
        t.todo(&format!("k{}", key_id(l)), l, 178.0 + (i % 5) as f32 * 48.0, 392.0 + (i / 5) as f32 * 44.0, 42.0, 36.0);
    }
    t
}

/// CarrierAce MFD, measured on an MFD R 2026-11-17 (tools/mfd_capture.py):
/// 9 buttons per edge numbered counter-clockwise (left 1-9 downwards, bottom 12-20 rightwards,
/// right 23-31 upwards, top 34-42 leftwards); the corner rockers fill the gaps.
fn mfd() -> Template {
    // bezel 500x500 sits below the wheel (top middle); side switch strip to the right
    const OY: f32 = 80.0;
    let mut t = Template::new("mfd", "CarrierAce MFD", 600.0, 500.0 + OY);
    t.decos[0].rect = [0.0, OY, 500.0, 500.0];
    t.screen(66.0, 66.0 + OY, 368.0, 368.0, "");
    let (c0, step, bw, bh) = (66.0, 368.0 / 9.0, 34.0, 32.0);
    let pos = |i: usize| c0 + step * (i as f32 + 0.5);
    for i in 0..9 {
        let p = pos(i);
        let left = 1 + i as u32; // top to bottom
        let bottom = 12 + i as u32; // left to right
        let right = 31 - i as u32; // top to bottom
        let top = 42 - i as u32; // left to right
        t.known(&format!("l{}", i + 1), &left.to_string(), 18.0, OY + p - bw / 2.0, bh, bw, left);
        t.known(&format!("b{}", i + 1), &bottom.to_string(), p - bw / 2.0, OY + 500.0 - 18.0 - bh, bw, bh, bottom);
        t.known(&format!("r{}", i + 1), &right.to_string(), 500.0 - 18.0 - bh, OY + p - bw / 2.0, bh, bw, right);
        t.known(&format!("t{}", i + 1), &top.to_string(), p - bw / 2.0, OY + 18.0, bw, bh, top);
    }
    // corner rockers, confirmed on hardware: SYM and CON count minus first
    let rockers = [
        ("day", "DAY", 43, "night", "NIGHT", 44, 6.0, 6.0),
        ("sym_up", "SYM+", 33, "sym_dn", "SYM-", 32, 442.0, 6.0),
        ("gain_up", "GAIN+", 10, "gain_dn", "GAIN-", 11, 6.0, 442.0),
        ("con_up", "CON+", 22, "con_dn", "CON-", 21, 442.0, 442.0),
    ];
    for (id1, l1, b1, id2, l2, b2, x, y) in rockers {
        t.known(id1, l1, x, OY + y, 52.0, 25.0, b1);
        t.known(id2, l2, x, OY + y + 27.0, 52.0, 25.0, b2);
    }
    // wheel on top in the middle: 49 counter-clockwise, 50 clockwise, 48 pressed
    t.decos.push(Deco { rect: [140.0, 4.0, 220.0, OY - 4.0], kind: DecoKind::Body });
    t.knob(250.0, 40.0, 30.0, "");
    t.known("wheel_ccw", "CCW", 150.0, 24.0, 58.0, 32.0, 49);
    t.known("wheel_push", "PUSH", 234.0, 24.0, 32.0, 32.0, 48);
    t.controls.last_mut().unwrap().round = true;
    t.known("wheel_cw", "CW", 292.0, 24.0, 58.0, 32.0, 50);
    // side switch 45/46/47, each position is a profile
    t.decos.push(Deco { rect: [512.0, OY + 120.0, 78.0, 190.0], kind: DecoKind::Body });
    t.text(526.0, OY + 130.0, "SIDE");
    t.known("side_45", "SWITCH 45", 520.0, OY + 150.0, 62.0, 34.0, 45);
    t.known("side_up", "SWITCH 46", 520.0, OY + 194.0, 62.0, 34.0, 46);
    t.known("side_dn", "SWITCH 47", 520.0, OY + 238.0, 62.0, 34.0, 47);
    for c in t.controls.iter_mut().filter(|c| c.id.starts_with("side_")) {
        c.profile_switch = true;
    }
    t
}

/// CarrierAce UFC with the HUD control panel underneath. A/P..BCN numbers are known (20-25).
fn ufc() -> Template {
    let mut t = Template::new("ufc", "CarrierAce UFC + HUD", 520.0, 640.0);
    // UFC
    t.knob(52.0, 54.0, 30.0, "COMM 1");
    t.knob(468.0, 54.0, 30.0, "COMM 2");
    t.round_todo("comm1_pull", "C1 PULL", 52.0, 54.0, 13.0);
    t.round_todo("comm2_pull", "C2 PULL", 468.0, 54.0, 13.0);
    t.screen(110.0, 22.0, 300.0, 56.0, "SCRATCHPAD");
    for i in 0..5 {
        t.todo(&format!("opt{}", i + 1), &format!("OPTION {}", i + 1), 300.0, 96.0 + i as f32 * 44.0, 150.0, 34.0);
    }
    let pad = ["1", "2", "3", "4", "5", "6", "7", "8", "9", "CLR", "0", "ENT"];
    for (i, l) in pad.iter().enumerate() {
        t.todo(&format!("ufc_{}", key_id(l)), l, 110.0 + (i % 3) as f32 * 58.0, 96.0 + (i / 3) as f32 * 50.0, 50.0, 40.0);
    }
    t.todo("ip", "I/P", 30.0, 110.0, 54.0, 30.0);
    t.todo("emcon", "EMCON", 30.0, 150.0, 54.0, 30.0);
    t.todo("adf1", "ADF 1", 30.0, 200.0, 54.0, 24.0);
    t.todo("adf2", "ADF 2", 30.0, 228.0, 54.0, 24.0);
    t.knob(56.0, 285.0, 20.0, "BRT");
    let fx = [("A/P", Some(20)), ("IFF", Some(21)), ("TCN", Some(22)), ("ILS", Some(23)), ("D/L", Some(24)), ("BCN", Some(25)), ("ON/OFF", None)];
    for (i, (l, b)) in fx.iter().enumerate() {
        let x = 30.0 + i as f32 * 66.0;
        match b {
            Some(b) => t.known(&key_id(l), l, x, 330.0, 58.0, 32.0, *b),
            None => t.todo(&key_id(l), l, x, 330.0, 58.0, 32.0),
        }
    }
    // HUD control panel
    t.decos.push(Deco { rect: [10.0, 385.0, 500.0, 245.0], kind: DecoKind::Screen("HUD CONTROL PANEL") });
    let col = |t: &mut Template, x: f32, title: &'static str, items: &[&str]| {
        t.text(x, 420.0, title);
        for (i, l) in items.iter().enumerate() {
            t.todo(&format!("hud_{}_{}", key_id(title), key_id(l)), l, x, 440.0 + i as f32 * 34.0, 74.0, 28.0);
        }
    };
    col(&mut t, 24.0, "SYM REJ", &["NORM", "REJ 1", "REJ 2"]);
    col(&mut t, 108.0, "SYM BRT", &["DAY", "NIGHT"]);
    col(&mut t, 192.0, "VIDEO", &["W/B", "VID", "OFF"]);
    col(&mut t, 276.0, "ALT", &["BARO", "RDR"]);
    col(&mut t, 360.0, "ATT", &["INS", "AUTO", "STBY"]);
    t.knob(470.0, 470.0, 22.0, "BLK LVL");
    t.knob(470.0, 560.0, 22.0, "AOA");
    t
}

/// F-16 ICP. COM 1, COM 2, IFF, LIST, A-A, A-G are known (1-6).
fn icp() -> Template {
    let mut t = Template::new("icp", "ViperAce ICP", 560.0, 330.0);
    let top = [("COM 1", 1), ("COM 2", 2), ("IFF", 3), ("LIST", 4), ("A-A", 5), ("A-G", 6)];
    for (i, (l, b)) in top.iter().enumerate() {
        t.known(&key_id(l), l, 70.0 + i as f32 * 72.0, 18.0, 62.0, 30.0, *b);
    }
    t.screen(150.0, 62.0, 260.0, 70.0, "DED");
    let pad = ["1 T-ILS", "2 ALOW", "3", "RCL", "4 STPT", "5 CRUS", "6 TIME", "ENTR", "7 MARK", "8 FIX", "9 A-CAL", "0 M-SEL"];
    for (i, l) in pad.iter().enumerate() {
        t.todo(&format!("icp_{}", key_id(l)), l, 130.0 + (i % 4) as f32 * 76.0, 150.0 + (i / 4) as f32 * 54.0, 68.0, 44.0);
    }
    t.text(450.0, 152.0, "DCS");
    t.todo("dcs_up", "UP", 460.0, 172.0, 44.0, 26.0);
    t.todo("dcs_rtn", "RTN", 420.0, 202.0, 44.0, 26.0);
    t.todo("dcs_seq", "SEQ", 500.0, 202.0, 44.0, 26.0);
    t.todo("dcs_dn", "DN", 460.0, 232.0, 44.0, 26.0);
    t.todo("drift_co", "DRIFT C/O", 18.0, 70.0, 80.0, 30.0);
    t.todo("warn_reset", "WARN RST", 18.0, 110.0, 80.0, 30.0);
    t.todo("flir_up", "FLIR +", 18.0, 170.0, 80.0, 28.0);
    t.todo("flir_dn", "FLIR -", 18.0, 202.0, 80.0, 28.0);
    t.knob(470.0, 92.0, 22.0, "SYM");
    t.knob(530.0, 92.0, 22.0, "BRT");
    t
}

/// Hornet take-off / left-console panel (approximate layout).
fn pto2() -> Template {
    let mut t = Template::new("pto2", "CarrierAce PTO 2", 560.0, 380.0);
    t.text(24.0, 18.0, "MASTER ARM");
    t.todo("arm", "ARM", 24.0, 38.0, 70.0, 30.0);
    t.todo("safe", "SAFE", 24.0, 72.0, 70.0, 30.0);
    t.todo("aa", "A/A", 110.0, 38.0, 60.0, 30.0);
    t.todo("ag", "A/G", 110.0, 72.0, 60.0, 30.0);
    t.todo("master_caution", "MASTER CAUTION", 190.0, 38.0, 120.0, 40.0);
    t.todo("emer_jett", "EMERG JETT", 330.0, 38.0, 100.0, 40.0);
    t.todo("fire_ext", "FIRE EXT", 450.0, 38.0, 90.0, 40.0);
    t.text(24.0, 130.0, "FLAPS");
    t.todo("flaps_auto", "AUTO", 24.0, 150.0, 70.0, 28.0);
    t.todo("flaps_half", "HALF", 24.0, 182.0, 70.0, 28.0);
    t.todo("flaps_full", "FULL", 24.0, 214.0, 70.0, 28.0);
    t.todo("launch_bar", "LAUNCH BAR", 110.0, 150.0, 100.0, 30.0);
    t.todo("hook_bypass", "HOOK BYPASS", 110.0, 190.0, 100.0, 30.0);
    t.todo("spin", "SPIN", 110.0, 230.0, 100.0, 30.0);
    t.text(240.0, 130.0, "SEL JETT");
    for (i, l) in ["L FUS", "SAFE", "R FUS", "RACK", "STORES"].iter().enumerate() {
        t.todo(&format!("jett_{}", key_id(l)), l, 240.0, 150.0 + i as f32 * 32.0, 80.0, 28.0);
    }
    t.text(440.0, 130.0, "GEAR");
    t.todo("gear_up", "GEAR UP", 430.0, 150.0, 110.0, 50.0);
    t.todo("gear_down", "GEAR DOWN", 430.0, 210.0, 110.0, 50.0);
    t.todo("anti_skid", "ANTI SKID", 340.0, 150.0, 80.0, 30.0);
    t.todo("hook", "HOOK", 340.0, 190.0, 80.0, 30.0);
    t.todo("down_lock", "DOWN LOCK OVRD", 340.0, 230.0, 80.0, 30.0);
    t.text(24.0, 322.0, "LAMPS");
    for (i, l) in ["NOSE", "LEFT", "RIGHT", "HALF", "FULL", "FLAPS", "HOOK"].iter().enumerate() {
        t.text(24.0 + i as f32 * 74.0, 340.0, l_static(l));
    }
    t
}

fn l_static(s: &str) -> &'static str {
    match s {
        "NOSE" => "NOSE",
        "LEFT" => "LEFT",
        "RIGHT" => "RIGHT",
        "HALF" => "HALF",
        "FULL" => "FULL",
        "FLAPS" => "FLAPS",
        _ => "HOOK",
    }
}

/// Device without a drawing: every button from its descriptor in a grid.
pub fn generic(name: &str, usages: &[u32]) -> Template {
    let cols = 10usize;
    let rows = usages.len().div_ceil(cols).max(1);
    let mut t = Template::new("generic", name, 20.0 + cols as f32 * 44.0, 20.0 + rows as f32 * 44.0);
    t.generic = true;
    for (i, u) in usages.iter().enumerate() {
        t.known(&format!("b{u}"), &format!("{u}"), 14.0 + (i % cols) as f32 * 44.0, 14.0 + (i / cols) as f32 * 44.0, 38.0, 38.0, *u);
        t.controls.last_mut().unwrap().round = true;
    }
    t
}

fn key_id(l: &str) -> String {
    l.to_ascii_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pfp_matches_old_key_table() {
        // pfp_writer's PFP_KEYCODES are bit indices; HID button = bit + 1
        let t = template_for(0xBB35, &[]);
        let b = |id: &str| t.controls.iter().find(|c| c.id == id).unwrap().button.unwrap();
        assert_eq!(b("lsk1l"), 1);
        assert_eq!(b("menu"), 20); // MENU bit 19
        assert_eq!(b("next_page"), 29); // NEXT PAGE bit 28
        assert_eq!(b("d0"), 40); // "0" bit 39
        assert_eq!(b("kclr"), 71); // CLR bit 70
        assert_eq!(b("dim"), 18); // BRT- bit 17
    }

    #[test]
    fn templates_are_sane() {
        for t in gallery() {
            let mut ids = std::collections::HashSet::new();
            for c in &t.controls {
                assert!(ids.insert(c.id.clone()), "{} duplicate id {}", t.id, c.id);
                let [x, y, w, h] = c.rect;
                assert!(x >= 0.0 && y >= 0.0 && x + w <= t.size[0] + 0.1 && y + h <= t.size[1] + 0.1, "{} {} out of bounds", t.id, c.id);
            }
            let mut nums = std::collections::HashSet::new();
            for c in t.controls.iter().filter_map(|c| c.button) {
                assert!(nums.insert(c), "{} duplicate button {}", t.id, c);
            }
        }
        assert_eq!(template_for(0xBEE1, &[]).name, "CarrierAce MFD L");
        assert!(template_for(0x1234, &[1, 2, 3]).generic);
    }
}
