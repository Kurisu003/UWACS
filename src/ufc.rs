//! F/A-18 UFC writer (was ufc_writer): mirrors the UFC scratchpad, option displays and comm
//! channels from DCS-BIOS onto the WinWing UFC. Works with the F/A-18C and the AV-8B.
//! Segment tables and packet format are unchanged from the original program.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use hidapi::HidDevice;
use phf::phf_map;

use crate::dcsbios::{self, get_string_by_addr_and_len, get_value_by_address, Snapshot};
use crate::devices;
use crate::log::log;
use crate::status::{set_status, Status};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UfcPackage {
    pub area_1: Vec<char>,
    pub odu: Vec<OduPackage>,
    pub comms: Vec<CommsPackage>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OduPackage {
    pub id: u8,
    pub is_selected: bool,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommsPackage {
    pub is_left: bool,
    pub ch: char,
}

/// Runs until `stop` is set. Waits for the UFC to appear, then writes whenever the data changes.
pub fn run(path: String, stop: Arc<AtomicBool>) {
    let dev = match devices::open(&path) {
        Ok(d) => d,
        Err(e) => {
            log(format!("Error: UFC_WRITER: No HID device ({e})"));
            set_status("ufc", Status::Error(e.to_string()));
            return;
        }
    };
    log("UFC_WRITER: HID device OK");
    set_status("ufc", Status::Running);

    let mut pkg = startup_package();
    write_package_to_ufc(&pkg, &dev);
    let mut last = pkg.clone();
    while !stop.load(Ordering::Relaxed) {
        let snap = dcsbios::bios().snapshot();
        let module = dcsbios::module_name(&snap);
        if let Some(u) = (!module.is_empty()).then(|| crate::custom::ufc_for(&module)).flatten() {
            pkg = crate::custom::ufc_package(&u, &snap, &crate::custom::catalog(&module));
        } else if module.starts_with("AV8B") {
            pkg = get_av8b_ufc_package(&snap);
        } else if module.starts_with("FA-18") {
            pkg = get_fa18_ufc_package(&snap);
        }
        if pkg != last {
            write_package_to_ufc(&pkg, &dev);
            last = pkg.clone();
        }
        thread::sleep(Duration::from_millis(10));
    }
    set_status("ufc", Status::Stopped);
    log("UFC_WRITER: stopped");
}

fn startup_package() -> UfcPackage {
    UfcPackage {
        area_1: vec!['X', 'X', '8', '8', '8', '8', '8', '8', '8'],
        odu: vec![
            OduPackage { id: 1, is_selected: true, text: "UFC".into() },
            OduPackage { id: 2, is_selected: false, text: "NOT".into() },
            OduPackage { id: 3, is_selected: true, text: "YET".into() },
            OduPackage { id: 4, is_selected: false, text: "INIT".into() },
            OduPackage { id: 5, is_selected: true, text: "XXXX".into() },
        ],
        comms: vec![CommsPackage { is_left: true, ch: 'X' }, CommsPackage { is_left: false, ch: 'X' }],
    }
}

// ------------------------------------------------------------------ DCS-BIOS -> package

fn ch_at(s: &str, i: usize) -> char {
    s.as_bytes().get(i).map(|&b| b as char).unwrap_or(' ')
}

pub fn get_fa18_ufc_package(v: &Snapshot) -> UfcPackage {
    let odu_addr = [0x7432, 0x7436, 0x743a, 0x743e, 0x7442];
    let sel_addr = [0x7428, 0x795e, 0x7964, 0x796a, 0x7970];
    let odu = (0..5)
        .map(|i| OduPackage {
            id: i as u8 + 1,
            is_selected: get_value_by_address(v, sel_addr[i])[0] == 58,
            text: get_string_by_addr_and_len(v, odu_addr[i], 4),
        })
        .collect();
    let s1 = get_string_by_addr_and_len(v, 0x744e, 2);
    let s2 = get_string_by_addr_and_len(v, 0x7446, 8);
    let mut area_1 = vec![ch_at(&s1, 0), ch_at(&s1, 1)];
    area_1.extend((0..7).map(|i| ch_at(&s2, i)));
    let comms = vec![
        CommsPackage { is_left: true, ch: ch_at(&get_string_by_addr_and_len(v, 0x7424, 2), 1) },
        CommsPackage { is_left: false, ch: ch_at(&get_string_by_addr_and_len(v, 0x7426, 2), 1) },
    ];
    UfcPackage { area_1, odu, comms }
}

pub fn get_av8b_ufc_package(v: &Snapshot) -> UfcPackage {
    let odu_addr = [0x795a, 0x7960, 0x7966, 0x796c, 0x7972];
    let sel_addr = [0x7958, 0x795e, 0x7964, 0x796a, 0x7970];
    let odu = (0..5)
        .map(|i| OduPackage {
            id: i as u8 + 1,
            is_selected: get_value_by_address(v, sel_addr[i])[0] == 58,
            text: get_string_by_addr_and_len(v, odu_addr[i], 4),
        })
        .collect();
    let s = get_string_by_addr_and_len(v, 0x7976, 12);
    let mut area_1 = vec![ch_at(&s, 0), ch_at(&s, 1)];
    area_1.extend((5..12).map(|i| ch_at(&s, i)));
    let comms = vec![
        CommsPackage { is_left: true, ch: ch_at(&get_string_by_addr_and_len(v, 0x7954, 2), 1) },
        CommsPackage { is_left: false, ch: ch_at(&get_string_by_addr_and_len(v, 0x7956, 2), 1) },
    ];
    UfcPackage { area_1, odu, comms }
}

// ------------------------------------------------------------------ package -> HID packets

fn parse_hex_line(line: &str) -> Vec<u8> {
    let s = line.split('#').next().unwrap_or("").trim();
    let t: String = s.chars().filter(|c| !c.is_whitespace() && *c != ',').collect();
    let t = t.strip_prefix("0x").unwrap_or(&t);
    (0..t.len() / 2).filter_map(|i| u8::from_str_radix(&t[2 * i..2 * i + 2], 16).ok()).collect()
}

fn le_bits_to_hex(bits: &str) -> String {
    let cleaned: String = bits.chars().filter(|c| !c.is_whitespace()).collect();
    let mut reordered = String::with_capacity(cleaned.len());
    for i in (0..cleaned.len()).step_by(8) {
        reordered.push_str(&cleaned[i + 4..i + 8]);
        reordered.push_str(&cleaned[i..i + 4]);
    }
    let value = u128::from_str_radix(&reordered, 2).unwrap_or(0);
    format!("{:0width$x}", value, width = reordered.len() / 4)
}

pub fn segments_to_bits(segments: &[String], area: &str) -> ((String, String, String), String) {
    let lut = if area.starts_with("AREA_1") {
        &AREA_1_BITS
    } else if area.starts_with("ODU") {
        &ODU_BITS
    } else {
        &COMMS_BITS
    };
    let p = |a: &str, b: &str, c: &str| (a.to_string(), b.to_string(), c.to_string());
    let prefixes = match area {
        s if s.starts_with("AREA_1") => p("00", "01", "02"),
        s if s.starts_with("ODU_1") => p("04", "05", "06"),
        s if s.starts_with("ODU_2") => p("08", "09", "0a"),
        s if s.starts_with("ODU_3") => p("0c", "0d", "0e"),
        s if s.starts_with("ODU_4") => p("10", "11", "12"),
        s if s.starts_with("ODU_5") => p("14", "15", "16"),
        _ => p("16", "17", "FF"),
    };
    let mut bits = [false; 97];
    for seg in segments {
        if let Some(&idx) = lut.get(seg.as_str()) {
            bits[idx as usize - 1] = true;
        }
    }
    let mut s: String = (0..97).rev().map(|i| if bits[i] { '1' } else { '0' }).collect();
    s.push_str(&"0".repeat(128 - 97));
    (prefixes, s)
}

pub fn segments_to_hex(segments: &[String], area: &str) -> Vec<String> {
    let ((p1, p2, p3), num) = segments_to_bits(segments, area);
    let pre = "02d0be0000064c";
    vec![
        format!("{pre}{p1}{}0000", le_bits_to_hex(&num[0..32])),
        format!("{pre}{p2}{}0000", le_bits_to_hex(&num[32..64])),
        format!("{pre}{p3}{}0000", le_bits_to_hex(&num[64..96])),
    ]
}

fn send_hex_lines(dev: &HidDevice, lines: &[String]) {
    for l in lines {
        let buf = parse_hex_line(l);
        if !buf.is_empty() {
            let _ = dev.write(&buf);
            thread::sleep(Duration::from_millis(10));
        }
    }
}

fn push_glyph(out: &mut Vec<String>, table: &phf::Map<&'static str, &'static str>, c: char, suffix: &str) {
    let key = c.to_ascii_uppercase().to_string();
    let segs = table.get(key.as_str()).copied().unwrap_or("");
    for s in segs.chars() {
        out.push(format!("{s}{suffix}"));
    }
}

fn write_area_1(chars: &[char], dev: &HidDevice) {
    let mut segs = Vec::new();
    for (i, &c) in chars.iter().take(2).enumerate() {
        push_glyph(&mut segs, &SIXTEEN_SEGMENT_LETTER_LOOKUP, c, &format!("_{}", i + 1));
    }
    for (i, &c) in chars.iter().enumerate().skip(2).take(7) {
        push_glyph(&mut segs, &SEVEN_SEGMENT_LETTER_LOOKUP, c, &format!("_{}", i + 1));
    }
    send_hex_lines(dev, &segments_to_hex(&segs, "AREA_1"));
}

/// Returns the third packet when it overlaps the comms packet (ODU 5), so its last letter
/// can be OR-ed into the comms write instead of being wiped.
fn write_odu(p: &OduPackage, dev: &HidDevice) -> Option<String> {
    let mut segs = Vec::new();
    for (i, c) in p.text.chars().enumerate() {
        push_glyph(&mut segs, &SIXTEEN_SEGMENT_LETTER_LOOKUP, c, &format!("_{}", i + 1));
    }
    if p.is_selected {
        segs.push(":".into());
    }
    let a = segments_to_hex(&segs, &format!("ODU_{}", p.id));
    let sendback = &a[2][14..16] == "16";
    send_hex_lines(dev, &a);
    sendback.then(|| a[2].clone())
}

fn write_comms(comms: &[CommsPackage], last_odu: &str, dev: &HidDevice) {
    let mut segs = Vec::new();
    for c in comms {
        push_glyph(&mut segs, &SIXTEEN_SEGMENT_LETTER_LOOKUP, c.ch, if c.is_left { "_Left" } else { "_Right" });
    }
    let mut a = segments_to_hex(&segs, "COMMS");
    if let (Ok(odu), Ok(first)) = (u128::from_str_radix(last_odu, 16), u128::from_str_radix(&a[0], 16)) {
        a[0] = format!("0{:x}", first | odu);
    }
    send_hex_lines(dev, &a);
}

pub fn write_package_to_ufc(p: &UfcPackage, dev: &HidDevice) {
    write_area_1(&p.area_1, dev);
    let mut last_odu = String::new();
    for o in &p.odu {
        if let Some(s) = write_odu(o, dev) {
            last_odu = s;
        }
    }
    write_comms(&p.comms, &last_odu, dev);
}

// ------------------------------------------------------------------ tables (unchanged)

static AREA_1_BITS: phf::Map<&'static str, u32> = phf_map! {
    "O_1" => 97, "J_1" => 96, "H_1" => 95, "C_1" => 94, "K_1" => 93, "L_1" => 92, "D_1" => 91, "A_1" => 90,
    "P_1" => 89, "M_1" => 88, "E_1" => 87, "B_1" => 86, "N_1" => 85, "I_1" => 84, "F_1" => 83, "G_1" => 82,
    "O_2" => 81, "J_2" => 80, "H_2" => 79, "C_2" => 78, "K_2" => 77, "L_2" => 76, "D_2" => 75, "A_2" => 74,
    "P_2" => 73, "M_2" => 72, "E_2" => 71, "B_2" => 70, "N_2" => 69, "I_2" => 68, "F_2" => 67, "G_2" => 66,
    "E_3" => 64, "G_3" => 63, "F_3" => 62, "D_3" => 61, "C_3" => 60, "B_3" => 59, "A_3" => 58,
    "E_4" => 56, "G_4" => 55, "F_4" => 54, "D_4" => 53, "C_4" => 52, "B_4" => 51, "A_4" => 50,
    "E_5" => 48, "G_5" => 47, "F_5" => 46, "D_5" => 45, "C_5" => 44, "B_5" => 43, "A_5" => 42,
    "E_6" => 40, "G_6" => 39, "F_6" => 38, "D_6" => 37, "C_6" => 36, "B_6" => 35, "A_6" => 34,
    "E_7" => 32, "G_7" => 31, "F_7" => 30, "D_7" => 29, "C_7" => 28, "B_7" => 27, "A_7" => 26,
    "E_8" => 24, "G_8" => 23, "F_8" => 22, "D_8" => 21, "C_8" => 20, "B_8" => 19, "A_8" => 18,
    "E_9" => 16, "G_9" => 15, "F_9" => 14, "D_9" => 13, "C_9" => 12, "B_9" => 11, "A_9" => 10,
};

static ODU_BITS: phf::Map<&'static str, u32> = phf_map! {
    ":" => 97,
    "O_1" => 93, "J_1" => 92, "H_1" => 91, "C_1" => 90, "K_1" => 89, "L_1" => 88, "D_1" => 87, "A_1" => 86,
    "P_1" => 85, "M_1" => 84, "E_1" => 83, "B_1" => 82, "N_1" => 81, "I_1" => 80, "F_1" => 79, "G_1" => 78,
    "O_2" => 77, "J_2" => 76, "H_2" => 75, "C_2" => 74, "K_2" => 73, "L_2" => 72, "D_2" => 71, "A_2" => 70,
    "P_2" => 69, "M_2" => 68, "E_2" => 67, "B_2" => 66, "N_2" => 65, "I_2" => 64, "F_2" => 63, "G_2" => 62,
    "O_3" => 61, "J_3" => 60, "H_3" => 59, "C_3" => 58, "K_3" => 57, "L_3" => 56, "D_3" => 55, "A_3" => 54,
    "P_3" => 53, "M_3" => 52, "E_3" => 51, "B_3" => 50, "N_3" => 49, "I_3" => 48, "F_3" => 47, "G_3" => 46,
    "O_4" => 45, "J_4" => 44, "H_4" => 43, "C_4" => 42, "K_4" => 41, "L_4" => 40, "D_4" => 39, "A_4" => 38,
    "P_4" => 37, "M_4" => 36, "E_4" => 35, "B_4" => 34, "N_4" => 33, "I_4" => 32, "F_4" => 31, "G_4" => 30,
};

static COMMS_BITS: phf::Map<&'static str, u32> = phf_map! {
    "N_Left" => 93, "P_Left" => 92, "L_Left" => 91, "O_Left" => 90, "M_Left" => 89, "I_Left" => 88,
    "K_Left" => 87, "J_Left" => 86, "G_Left" => 85, "F_Left" => 84, "H_Left" => 83, "C_Left" => 82,
    "B_Left" => 81, "E_Left" => 80, "D_Left" => 79, "A_Left" => 78,
    "N_Right" => 77, "P_Right" => 76, "L_Right" => 75, "O_Right" => 74, "M_Right" => 73, "I_Right" => 72,
    "K_Right" => 71, "J_Right" => 70, "G_Right" => 69, "F_Right" => 68, "H_Right" => 67, "C_Right" => 66,
    "B_Right" => 65, "E_Right" => 64, "D_Right" => 63, "A_Right" => 62,
};

static SIXTEEN_SEGMENT_LETTER_LOOKUP: phf::Map<&'static str, &'static str> = phf_map! {
    " " => "", "?" => "",
    "A" => "JCABGNHI", "B" => "ABIOPLEGN", "C" => "JCABOP", "D" => "ELABOPGN", "E" => "ABOPHCJ",
    "F" => "ABCJH", "G" => "ABCJOPNI", "H" => "CJGNHI", "I" => "ABOPEL", "J" => "GNOPJ",
    "K" => "CJHIFN", "L" => "CJOP", "M" => "JCDFGN", "N" => "JCDMNG", "O" => "ABGNOPJC",
    "P" => "JCABGHI", "Q" => "ABGNOPJCM", "R" => "JCABGIHM", "S" => "ABCHINPO", "T" => "LEAB",
    "U" => "CJOPNG", "V" => "CJKF", "W" => "CJKMNG", "X" => "DMKF", "Y" => "CHIGL", "Z" => "ABFKOP",
    "0" => "ABGNOPCJKF", "1" => "EL", "2" => "ABGHIJOP", "3" => "ABGINOP", "4" => "CHIGN",
    "5" => "ABCHINOP", "6" => "ABCJOPNHI", "7" => "ABGN", "8" => "ABHIOPJNCG", "9" => "ABCGHINOP",
    "/" => "KF",
};

static SEVEN_SEGMENT_LETTER_LOOKUP: phf::Map<&'static str, &'static str> = phf_map! {
    " " => "", "?" => "",
    "A" => "EFABCG", "B" => "FEDCG", "C" => "AFED", "D" => "GEDCB", "E" => "AFGED", "F" => "AFEDCG",
    "G" => "FECB", "H" => "FEGC", "I" => "FE", "J" => "BCDE", "K" => "FEGCB", "L" => "FED",
    "M" => "EGCA", "N" => "EFABC", "O" => "FABCDE", "P" => "EFABG", "Q" => "CFABG", "R" => "EG",
    "S" => "AFGCD", "T" => "FGED", "U" => "FEDCB", "V" => "FDB", "W" => "FGBD", "X" => "AGD",
    "Y" => "FGBCD", "Z" => "ABGED",
    "0" => "ABCDEF", "1" => "BC", "2" => "ABGED", "3" => "ABCDG", "4" => "FGBC", "5" => "AFGCD",
    "6" => "AFEDCG", "7" => "ABC", "8" => "ABCDEFG", "9" => "ABCFG", "." => "CDEG",
};

#[cfg(test)]
mod tests {
    use super::*;

    // The original program's own self-test values (src/test.rs).
    #[test]
    fn legacy_segment_tests() {
        let a = segments_to_bits(&["O_1".into(), "P_1".into()], "AREA_1");
        assert_eq!(a.0, ("00".into(), "01".into(), "02".into()));
        assert_eq!(a.1, format!("1000000010{}", "0".repeat(118)));
        let o = segments_to_bits(&["C_1".into(), "H_1".into()], "ODU_1");
        assert_eq!(o.1, format!("00000011{}", "0".repeat(120)));
        let h = segments_to_hex(&["O_1".into(), "P_1".into()], "AREA_1");
        assert_eq!(h, vec!["02d0be0000064c00080800000000", "02d0be0000064c01000000000000", "02d0be0000064c02000000000000"]);
    }
}
