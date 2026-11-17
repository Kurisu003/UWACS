//! Custom UFC and PFP screens built from DCS-BIOS values.
//!
//! A screen is text with placeholders: `{UFC_SCRATCHPAD_NUMBER_DISPLAY}` is replaced by that
//! DCS-BIOS output. `{NAME%}` shows an integer as a percentage of its maximum, `{NAME:5}` pads or
//! cuts the value to 5 characters. Names come from DCS-BIOS's own control lists
//! (Scripts/DCS-BIOS/doc/json/<aircraft>.json in the DCS settings folder).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, RwLock};

use serde::{Deserialize, Serialize};

use crate::dcsbios::{get_string_by_addr_and_len, get_value_by_address, Snapshot};

// ------------------------------------------------------------------ settings

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct CustomUfc {
    pub name: String,
    /// DCS-BIOS aircraft name (e.g. "FA-18C_hornet"); empty = every aircraft.
    pub aircraft: String,
    pub enabled: bool,
    /// Two small characters left of the scratchpad.
    pub scratch_small: String,
    /// Seven large scratchpad characters.
    pub scratch: String,
    /// Five option windows, 4 characters each.
    pub options: [String; 5],
    /// Option cue (the ":" left of an option) is lit when this is not empty, 0 or a space.
    pub cues: [String; 5],
    pub comm1: String,
    pub comm2: String,
}

impl Default for CustomUfc {
    fn default() -> Self {
        Self {
            name: "My UFC page".into(),
            aircraft: String::new(),
            enabled: true,
            scratch_small: String::new(),
            scratch: "UWACS".into(),
            options: Default::default(),
            cues: Default::default(),
            comm1: String::new(),
            comm2: String::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct PfpLine {
    pub text: String,
    pub color: String,
}

impl Default for PfpLine {
    fn default() -> Self {
        Self { text: String::new(), color: "green".into() }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct CustomPfp {
    pub name: String,
    pub aircraft: String,
    pub enabled: bool,
    /// 14 lines of 24 characters.
    pub lines: Vec<PfpLine>,
}

impl Default for CustomPfp {
    fn default() -> Self {
        let mut lines = vec![PfpLine::default(); crate::pfp::ROWS];
        lines[0].text = "MY PFP PAGE".into();
        lines[0].color = "white".into();
        Self { name: "My PFP page".into(), aircraft: String::new(), enabled: true, lines }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct CustomScreens {
    pub ufc: Vec<CustomUfc>,
    pub pfp: Vec<CustomPfp>,
}

/// What the writer threads read; the window updates it whenever the settings change.
pub static SCREENS: RwLock<CustomScreens> = RwLock::new(CustomScreens { ufc: Vec::new(), pfp: Vec::new() });

pub const PFP_COLORS: [&str; 7] = ["green", "white", "cyan", "yellow", "orange", "red", "magenta"];

pub fn color_static(c: &str) -> &'static str {
    PFP_COLORS.iter().find(|x| **x == c).copied().unwrap_or("green")
}

/// First enabled screen for this aircraft: an exact aircraft match beats "every aircraft".
fn pick<'a, T>(list: &'a [T], aircraft: &str, get: impl Fn(&T) -> (&str, bool)) -> Option<&'a T> {
    let ok = |t: &&T| get(t).1;
    list.iter().filter(ok).find(|t| !aircraft.is_empty() && get(t).0 == aircraft).or_else(|| list.iter().filter(ok).find(|t| get(t).0.is_empty()))
}

pub fn ufc_for(aircraft: &str) -> Option<CustomUfc> {
    let s = SCREENS.read().unwrap();
    pick(&s.ufc, aircraft, |u| (u.aircraft.as_str(), u.enabled)).cloned()
}

pub fn pfp_for(aircraft: &str) -> Option<CustomPfp> {
    let s = SCREENS.read().unwrap();
    pick(&s.pfp, aircraft, |p| (p.aircraft.as_str(), p.enabled)).cloned()
}

// ------------------------------------------------------------------ DCS-BIOS control lists

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Int { mask: u16, shift: u8, max: u32 },
    Str { len: u16 },
}

#[derive(Clone, Debug)]
pub struct Output {
    pub name: String,
    pub category: String,
    pub description: String,
    pub address: u16,
    pub kind: Kind,
}

pub type Catalog = HashMap<String, Output>;

static CATALOGS: Mutex<Option<HashMap<String, std::sync::Arc<Catalog>>>> = Mutex::new(None);

/// The doc/json folder of the installed DCS-BIOS.
pub fn json_dir() -> Option<PathBuf> {
    let d = crate::monitorsetup::saved_games_dir()?.join("Scripts/DCS-BIOS/doc/json");
    d.is_dir().then_some(d)
}

/// Aircraft that DCS-BIOS has control lists for.
pub fn aircraft_list() -> Vec<String> {
    let Some(d) = json_dir() else { return Vec::new() };
    let mut v: Vec<String> = std::fs::read_dir(d)
        .map(|rd| rd.flatten().filter_map(|e| e.file_name().to_str().and_then(|n| n.strip_suffix(".json")).map(|s| s.to_string())).collect())
        .unwrap_or_default();
    v.retain(|n| !matches!(n.as_str(), "CommonData" | "MetadataStart" | "MetadataEnd" | "NS430" | "SuperCarrier" | "VNAO_Room"));
    v.sort_by_key(|n| n.to_ascii_lowercase());
    v
}

pub fn parse_catalog(json: &str, into: &mut Catalog) {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else { return };
    let Some(cats) = v.as_object() else { return };
    for (cat, controls) in cats {
        let Some(controls) = controls.as_object() else { continue };
        for (name, c) in controls {
            let Some(out) = c["outputs"].as_array().and_then(|a| a.first()) else { continue };
            let Some(address) = out["address"].as_u64() else { continue };
            let kind = match out["type"].as_str() {
                Some("string") => Kind::Str { len: out["max_length"].as_u64().unwrap_or(1) as u16 },
                _ => Kind::Int { mask: out["mask"].as_u64().unwrap_or(0xFFFF) as u16, shift: out["shift_by"].as_u64().unwrap_or(0) as u8, max: out["max_value"].as_u64().unwrap_or(65535) as u32 },
            };
            into.insert(
                name.clone(),
                Output { name: name.clone(), category: cat.clone(), description: c["description"].as_str().unwrap_or("").to_string(), address: address as u16, kind },
            );
        }
    }
}

/// Controls for an aircraft plus the common ones (cached).
pub fn catalog(aircraft: &str) -> std::sync::Arc<Catalog> {
    let mut g = CATALOGS.lock().unwrap();
    let map = g.get_or_insert_with(HashMap::new);
    if let Some(c) = map.get(aircraft) {
        return c.clone();
    }
    let mut cat = Catalog::new();
    if let Some(d) = json_dir() {
        for f in ["CommonData", "MetadataStart"].iter().map(|s| s.to_string()).chain((!aircraft.is_empty()).then(|| aircraft.to_string())) {
            if let Ok(s) = std::fs::read_to_string(d.join(format!("{f}.json"))) {
                parse_catalog(&s, &mut cat);
            }
        }
    }
    let arc = std::sync::Arc::new(cat);
    map.insert(aircraft.to_string(), arc.clone());
    arc
}

pub fn read(o: &Output, snap: &Snapshot) -> String {
    match o.kind {
        Kind::Str { len } => {
            let mut s = get_string_by_addr_and_len(snap, o.address, len + len % 2);
            s.truncate(len as usize);
            s
        }
        Kind::Int { mask, shift, .. } => int_value(o, snap, mask, shift).to_string(),
    }
}

fn int_value(o: &Output, snap: &Snapshot, mask: u16, shift: u8) -> u32 {
    let [lo, hi] = get_value_by_address(snap, o.address);
    ((u16::from_le_bytes([lo, hi]) & mask) >> shift) as u32
}

// ------------------------------------------------------------------ templates

/// Replace every {NAME}, {NAME%}, {NAME:N} in `tpl`. Unknown names stay visible as "?NAME".
pub fn render(tpl: &str, snap: &Snapshot, cat: &Catalog) -> String {
    let mut out = String::new();
    let mut rest = tpl;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find('}') else {
            out.push_str(&rest[start..]);
            return out;
        };
        let inner = after[..end].trim();
        let (name, width) = match inner.rsplit_once(':') {
            Some((n, w)) if w.trim().parse::<usize>().is_ok() => (n.trim(), Some(w.trim().parse::<usize>().unwrap())),
            _ => (inner, None),
        };
        let (name, pct) = match name.strip_suffix('%') {
            Some(n) => (n, true),
            None => (name, false),
        };
        let mut val = match cat.get(name) {
            Some(o) => match (&o.kind, pct) {
                (Kind::Int { mask, shift, max }, true) => {
                    let v = int_value(o, snap, *mask, *shift);
                    format!("{}", (v as f64 * 100.0 / (*max).max(1) as f64).round() as u32)
                }
                _ => read(o, snap),
            },
            None => format!("?{name}"),
        };
        if let Some(w) = width {
            let n = val.chars().count();
            if n > w {
                val = val.chars().take(w).collect();
            } else {
                // numbers right-aligned, text left-aligned
                let numeric = val.trim().parse::<f64>().is_ok();
                let pad = " ".repeat(w - n);
                val = if numeric { format!("{pad}{val}") } else { format!("{val}{pad}") };
            }
        }
        out.push_str(&val);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

/// Exactly `n` characters, padded with spaces.
pub fn fit(s: &str, n: usize) -> String {
    let mut v: String = s.chars().take(n).collect();
    while v.chars().count() < n {
        v.push(' ');
    }
    v
}

/// The custom UFC page as the UFC writer needs it.
pub fn ufc_package(u: &CustomUfc, snap: &Snapshot, cat: &Catalog) -> crate::ufc::UfcPackage {
    let r = |t: &str| render(t, snap, cat).to_uppercase();
    let small = fit(&r(&u.scratch_small), 2);
    let big = fit(&r(&u.scratch), 7);
    let area_1: Vec<char> = small.chars().chain(big.chars()).collect();
    let odu = (0..5)
        .map(|i| {
            let cue = r(&u.cues[i]);
            let lit = !cue.trim().is_empty() && cue.trim() != "0";
            crate::ufc::OduPackage { id: i as u8 + 1, is_selected: lit, text: fit(&r(&u.options[i]), 4) }
        })
        .collect();
    let c = |t: &str| fit(&r(t), 1).chars().next().unwrap_or(' ');
    let comms = vec![crate::ufc::CommsPackage { is_left: true, ch: c(&u.comm1) }, crate::ufc::CommsPackage { is_left: false, ch: c(&u.comm2) }];
    crate::ufc::UfcPackage { area_1, odu, comms }
}

/// The custom PFP page as the PFP writer needs it.
pub fn pfp_blocks(p: &CustomPfp, snap: &Snapshot, cat: &Catalog) -> Vec<crate::pfp::TextBlock> {
    (0..crate::pfp::ROWS)
        .map(|i| {
            let line = p.lines.get(i).cloned().unwrap_or_default();
            let text = fit(&render(&line.text, snap, cat).to_uppercase(), crate::pfp::COLS);
            crate::pfp::TextBlock::new(text, color_static(&line.color))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cat() -> Catalog {
        let json = r#"{"UFC": {
            "UFC_SCRATCHPAD_NUMBER_DISPLAY": {"description": "scratchpad", "outputs": [{"address": 29766, "max_length": 8, "type": "string"}]},
            "UFC_OPTION_CUEING_1": {"description": "cue 1", "outputs": [{"address": 29736, "mask": 1, "shift_by": 0, "max_value": 1, "type": "integer"}]},
            "HMD_OFF_BRT": {"description": "hmd brightness", "outputs": [{"address": 29700, "mask": 65535, "shift_by": 0, "max_value": 65535, "type": "integer"}]}
        }}"#;
        let mut c = Catalog::new();
        parse_catalog(json, &mut c);
        c
    }

    fn snap() -> Snapshot {
        let mut s = Snapshot::new();
        // "1234567 " at 29766
        for (i, pair) in b"1234567 ".chunks(2).enumerate() {
            s.insert(29766 + 2 * i as u16, [pair[0], pair[1]]);
        }
        s.insert(29736, [1, 0]);
        s.insert(29700, [0x00, 0x80]); // 32768
        s
    }

    #[test]
    fn templates() {
        let (c, s) = (cat(), snap());
        assert_eq!(render("SP {UFC_SCRATCHPAD_NUMBER_DISPLAY}", &s, &c), "SP 1234567 ");
        assert_eq!(render("{UFC_SCRATCHPAD_NUMBER_DISPLAY:3}", &s, &c), "123");
        assert_eq!(render("{HMD_OFF_BRT}", &s, &c), "32768");
        assert_eq!(render("HMD {HMD_OFF_BRT%:4}%", &s, &c), "HMD   50%");
        assert_eq!(render("{NOPE}", &s, &c), "?NOPE");
        assert_eq!(render("no braces", &s, &c), "no braces");
        assert_eq!(render("half {open", &s, &c), "half {open");
    }

    #[test]
    fn ufc_and_pfp_pages() {
        let (c, s) = (cat(), snap());
        let mut u = CustomUfc { scratch: "{UFC_SCRATCHPAD_NUMBER_DISPLAY}".into(), ..Default::default() };
        u.options[0] = "abc".into();
        u.cues[0] = "{UFC_OPTION_CUEING_1}".into();
        let pkg = ufc_package(&u, &s, &c);
        assert_eq!(pkg.area_1.iter().collect::<String>(), "  1234567");
        assert!(pkg.odu[0].is_selected && !pkg.odu[1].is_selected);
        assert_eq!(pkg.odu[0].text, "ABC ");
        let p = CustomPfp::default();
        let b = pfp_blocks(&p, &s, &c);
        assert_eq!(b.len(), crate::pfp::ROWS);
        assert!(b.iter().all(|x| x.text.chars().count() == crate::pfp::COLS));
        assert_eq!(b[0].fg, "white");
    }

    #[test]
    fn picks_aircraft_before_any() {
        let list = vec![CustomPfp { name: "any".into(), ..Default::default() }, CustomPfp { name: "hornet".into(), aircraft: "FA-18C_hornet".into(), ..Default::default() }];
        fn g(p: &CustomPfp) -> (&str, bool) {
            (p.aircraft.as_str(), p.enabled)
        }
        assert_eq!(pick(&list, "FA-18C_hornet", g).unwrap().name, "hornet");
        assert_eq!(pick(&list, "AV8BNA", g).unwrap().name, "any");
    }
}
