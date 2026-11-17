//! Per-aircraft PFP pages and DCS-BIOS key handling (was moduleDataProcessorHelper.rs).
//! Addresses, layouts and command names are unchanged.

use crate::dcsbios::{get_string_by_addr_and_len as gstr, get_value_by_address as gval, send_button_press, send_button_state_press, Snapshot};
use crate::pfp::{Keys, TextBlock, COLS, ROWS};

fn addrs_to_65k_num_as_string(v: &Snapshot, addrs: &[u16]) -> String {
    addrs
        .iter()
        .map(|&a| (f32::from(u16::from_le_bytes(gval(v, a))) / 6553.5).round().to_string())
        .collect()
}

fn word(v: &Snapshot, a: u16) -> u16 {
    u16::from_le_bytes(gval(v, a))
}

fn pad_to(s: &str, n: usize) -> String {
    " ".repeat(n.saturating_sub(s.len()))
}

pub fn get_ch47f_text(v: &Snapshot) -> Vec<TextBlock> {
    let cplt: [u16; 14] = [0x9e4e, 0x9e66, 0x9e7e, 0x9e96, 0x9eae, 0x9ec6, 0x9ede, 0x9ef6, 0x9f0e, 0x9f26, 0x9f3e, 0x9f56, 0x9f6e, 0x9f86];
    let mut t: String = cplt.iter().map(|&a| gstr(v, a, 24)).collect();
    t.truncate(COLS * ROWS);
    vec![TextBlock::new(t, "green")]
}

pub fn get_av8b_text(v: &Snapshot) -> Vec<TextBlock> {
    let h2o_text = format!("H2O : {}0", addrs_to_65k_num_as_string(v, &[0x78aa, 0x78ac]));
    let rpm_all = addrs_to_65k_num_as_string(v, &[0x7890, 0x7892, 0x7894, 0x7896]);
    let rpm_text = format!("RPM: {}", &rpm_all[..rpm_all.len().min(4)]);
    let fuel_text = format!("FUEL: {}", addrs_to_65k_num_as_string(v, &[0x78b2, 0x78b4, 0x78b6, 0x78b8, 0x78ba]));
    let noz = addrs_to_65k_num_as_string(v, &[0x78ae]).parse::<f32>().unwrap_or(0.0) * 12.5;
    let nozzle_text = format!("NOZ: {}", noz as i32);

    let h2o_lookup = ["LDG", "OFF", "TO "];
    let h2o_pos = (word(v, 0x783c) & 0x000c) >> 2;
    let h2o_switch_text = if h2o_pos < 3 { format!("H2O POS: {}", h2o_lookup[h2o_pos as usize]) } else { String::new() };

    let flaps_lookup = ["STOL", "AUTO", "CRSE"];
    let flaps_pos = (word(v, 0x783a) & 0x0180) >> 7;
    let flaps_text = if flaps_pos < 3 { format!("FLAPS POS: {}", flaps_lookup[flaps_pos as usize]) } else { String::new() };

    let combined = format!("{h2o_text}{}{rpm_text}{fuel_text}{}{nozzle_text}{}", " ".repeat(6), " ".repeat(4), pad_to(&nozzle_text, 9));

    let master_arm = (word(v, 0x7836) & 0x4000) >> 14;
    let master_arm_text = format!("MASTER ARM: {}", if master_arm == 1 { "ON " } else { "OFF" });
    let gear = (word(v, 0x783a) & 0x8000) >> 15;
    let gear_text = format!("GEAR: {}", if gear == 1 { "UP" } else { "DN" });
    let air_brk_text = format!("AIR BRK: {}", ((word(v, 0x794e) as f32) / 655.35).round() as u8);

    let mm = word(v, 0x7880);
    let master_mode = if (mm & 0x0800) >> 11 == 1 {
        "A/G "
    } else if (mm & 0x0200) >> 9 == 1 {
        "NAV "
    } else if (mm & 0x0400) >> 10 == 1 {
        "VTOL"
    } else {
        "    "
    };

    let switch_text = format!(
        "{h2o_switch_text}{}{flaps_text}{}{master_arm_text} {gear_text}{air_brk_text}{}{master_mode}",
        " ".repeat(12),
        " ".repeat(9),
        pad_to(&air_brk_text, 24)
    );

    vec![TextBlock::new(combined, "green"), TextBlock::new(switch_text, "red")]
}

pub fn get_a10c2_text(v: &Snapshot) -> Vec<TextBlock> {
    let cdu: [u16; 10] = [0x11c0, 0x11d8, 0x11f0, 0x1208, 0x1220, 0x1238, 0x1250, 0x1268, 0x1280, 0x1298];
    let mut t = " ".repeat(24 * 4);
    for a in cdu {
        t.push_str(&gstr(v, a, 24));
    }
    vec![TextBlock::new(t, "green")]
}

pub fn handle_a10c2_input(keys: &Keys) {
    const BUTTONS: &[(&str, &str)] = &[
        ("INIT REF", "CDU_SYS TOGGLE"), ("RTE", "CDU_NAV TOGGLE"), ("CLB", "CDU_WP TOGGLE"), ("CRZ", "CDU_OSET TOGGLE"),
        ("DES", "CDU_MK TOGGLE"), ("PROG", "CDU_FPM TOGGLE"),
        ("L3", "CDU_LSK_3L TOGGLE"), ("L4", "CDU_LSK_5L TOGGLE"), ("L5", "CDU_LSK_7L TOGGLE"), ("L6", "CDU_LSK_9L TOGGLE"),
        ("R1", "CDU_PREV TOGGLE"), ("R3", "CDU_LSK_3R TOGGLE"), ("R4", "CDU_LSK_5R TOGGLE"), ("R5", "CDU_LSK_7R TOGGLE"),
        ("R6", "CDU_LSK_9R TOGGLE"), (".", "CDU_POINT TOGGLE"), ("/", "CDU_SLASH TOGGLE"),
        ("A", "CDU_A TOGGLE"), ("B", "CDU_B TOGGLE"), ("C", "CDU_C TOGGLE"), ("D", "CDU_D TOGGLE"), ("E", "CDU_E TOGGLE"),
        ("F", "CDU_F TOGGLE"), ("G", "CDU_G TOGGLE"), ("H", "CDU_H TOGGLE"), ("I", "CDU_I TOGGLE"), ("J", "CDU_J TOGGLE"),
        ("K", "CDU_K TOGGLE"), ("L", "CDU_L TOGGLE"), ("M", "CDU_M TOGGLE"), ("N", "CDU_N TOGGLE"), ("O", "CDU_O TOGGLE"),
        ("P", "CDU_P TOGGLE"), ("Q", "CDU_Q TOGGLE"), ("R", "CDU_R TOGGLE"), ("S", "CDU_S TOGGLE"), ("T", "CDU_T TOGGLE"),
        ("U", "CDU_U TOGGLE"), ("V", "CDU_V TOGGLE"), ("W", "CDU_W TOGGLE"), ("X", "CDU_X TOGGLE"), ("Y", "CDU_Y TOGGLE"),
        ("Z", "CDU_Z TOGGLE"), ("1", "CDU_1 TOGGLE"), ("2", "CDU_2 TOGGLE"), ("3", "CDU_3 TOGGLE"), ("4", "CDU_4 TOGGLE"),
        ("5", "CDU_5 TOGGLE"), ("6", "CDU_6 TOGGLE"), ("7", "CDU_7 TOGGLE"), ("8", "CDU_8 TOGGLE"), ("9", "CDU_9 TOGGLE"),
        ("0", "CDU_0 TOGGLE"), ("SP", "CDU_SPC TOGGLE"), ("DEL", "CDU_BCK TOGGLE"), ("CLR", "CDU_CLR TOGGLE"),
    ];
    const ROCKERS: &[(&str, &str, &str)] = &[
        ("L1", "CDU_SCROLL 0", "CDU_SCROLL 1"),
        ("L2", "CDU_SCROLL 2", "CDU_SCROLL 1"),
        ("PREV PAGE", "CDU_PG 2", "CDU_PG 1"),
        ("NEXT PAGE", "CDU_PG 0", "CDU_PG 1"),
        ("BRT+", "CDU_DATA 2", "CDU_DATA 1"),
        ("BRT-", "CDU_DATA 0", "CDU_DATA 1"),
    ];
    for (k, msg) in BUTTONS {
        if keys.pressed(k) {
            send_button_press(msg);
        }
    }
    for (k, a, b) in ROCKERS {
        if keys.pressed(k) {
            send_button_state_press(a, b);
        }
    }
}

fn ah64d_is_cpg(v: &Snapshot) -> bool {
    word(v, 0x8750) & 0x0100 == 0x0100
}

pub fn handle_ah64d_input(v: &Snapshot, keys: &Keys) {
    const KU: &[(&str, &str)] = &[
        ("EXEC", "_KU_EXEC TOGGLE"), (".", "_KU_DOT TOGGLE"), ("+-", "_KU_SIGN TOGGLE"), ("/", "_KU_SLASH TOGGLE"),
        ("A", "_KU_A TOGGLE"), ("B", "_KU_B TOGGLE"), ("C", "_KU_C TOGGLE"), ("D", "_KU_D TOGGLE"), ("E", "_KU_E TOGGLE"),
        ("F", "_KU_F TOGGLE"), ("G", "_KU_G TOGGLE"), ("H", "_KU_H TOGGLE"), ("I", "_KU_I TOGGLE"), ("J", "_KU_J TOGGLE"),
        ("K", "_KU_K TOGGLE"), ("L", "_KU_L TOGGLE"), ("M", "_KU_M TOGGLE"), ("N", "_KU_N TOGGLE"), ("O", "_KU_O TOGGLE"),
        ("P", "_KU_P TOGGLE"), ("Q", "_KU_Q TOGGLE"), ("R", "_KU_R TOGGLE"), ("S", "_KU_S TOGGLE"), ("T", "_KU_T TOGGLE"),
        ("U", "_KU_U TOGGLE"), ("V", "_KU_V TOGGLE"), ("W", "_KU_W TOGGLE"), ("X", "_KU_X TOGGLE"), ("Y", "_KU_Y TOGGLE"),
        ("Z", "_KU_Z TOGGLE"), ("1", "_KU_1 TOGGLE"), ("2", "_KU_2 TOGGLE"), ("3", "_KU_3 TOGGLE"), ("4", "_KU_4 TOGGLE"),
        ("5", "_KU_5 TOGGLE"), ("6", "_KU_6 TOGGLE"), ("7", "_KU_7 TOGGLE"), ("8", "_KU_8 TOGGLE"), ("9", "_KU_9 TOGGLE"),
        ("0", "_KU_0 TOGGLE"), ("SP", "_KU_SPC TOGGLE"), ("DEL", "_KU_BKS TOGGLE"), ("CLR", "_KU_CLR TOGGLE"),
        ("NEXT PAGE", "_KU_RIGHT TOGGLE"), ("PREV PAGE", "_KU_LEFT TOGGLE"),
    ];
    let prefix = if ah64d_is_cpg(v) { "CPG" } else { "PLT" };
    for (k, msg) in KU {
        if keys.pressed(k) {
            send_button_press(&format!("{prefix}{msg}"));
        }
    }
}

pub fn get_ah64d_text(v: &Snapshot) -> Vec<TextBlock> {
    let is_cpg = ah64d_is_cpg(v);
    let own_kdu: u16 = if is_cpg { 0x80ac } else { 0x808e };
    let other_kdu: u16 = if is_cpg { 0x808e } else { 0x80ac };
    let lines: Vec<String> = (0..=6u16).map(|i| gstr(v, 0x80c2 + i * 56, 56)).collect();
    let cell = |s: &str| format!("{s}{}", pad_to(s, 24));

    let adv: String = (0..6).map(|i| cell(&lines[i][38..])).collect();
    let warn: String = (0..3).map(|i| cell(&lines[i][19..37])).collect();
    let caut: String = (0..3).map(|i| cell(&lines[i][0..18])).collect();
    let own = format!("{}  ", gstr(v, own_kdu, 22));
    let other = gstr(v, other_kdu, 22);

    vec![
        TextBlock::new(adv, "yellow"),
        TextBlock::new(warn, "red"),
        TextBlock::new(caut, "orange"),
        TextBlock::new(own, "green"),
        TextBlock::new(other, "white"),
    ]
}
