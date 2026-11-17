#![allow(non_snake_case)]
use std::{collections::HashMap, thread::sleep, time::Duration};
use phf::phf_map;

use crate::{dcsBiosHelper::{send_button_press, send_button_state_press, send_message_to_dcsbios}, inputHelper::is_button_pressed, types::TextBlock};


fn get_value_by_address(values: &HashMap<u16, [u8; 2]>, address: u16) -> [u8; 2] {
    values.get(&address).copied().unwrap_or([0, 0])
}

fn addrs_to_65k_num_as_string(values: &HashMap<u16, [u8; 2]>, adrs: Vec<u16>) -> String{
    let mut text = String::from("");


    for add in adrs{
        let raw = get_value_by_address(values, add);
        let formatted = (f32::from(u16::from_le_bytes(raw)) / 6553.5).round();
        text += &formatted.to_string();
    }

    return text;
}

fn map_byte_to_char(b: u8) -> char {
    match b {
        0xB0 => '.',                 // custom mapping
        0xA1 => 'x',                 // custom mapping
        0xAB => 'y',                 // custom mapping
        0xBB => 'z',                 // custom mapping
        0xA9 => 'u',                 // custom mapping
        0xAE => '^',                 // custom mapping
        0xB6 => '_',                 // custom mapping
        0xB1 => '~',                 // custom mapping
        0x20..=0x7E => b.to_ascii_uppercase() as char,    // printable ASCII
        _ => '?',                    // fallback for non-ASCII bytes
    }
}

pub fn get_string_by_addr_and_len(
    values: &HashMap<u16, [u8; 2]>,
    base_addr: u16,
    length: u16,                    // length in BYTES; each cell is 2 bytes
) -> String {
    let cells = (length / 2) as usize;
    let mut out = String::with_capacity(length as usize);

    for i in 0..cells {
        let addr = base_addr.wrapping_add((2 * i) as u16);
        let [b0, b1] = get_value_by_address(values, addr);
        out.push(map_byte_to_char(b0));
        out.push(map_byte_to_char(b1));
    }

    out
}

pub fn get_CH47F_text(values: &HashMap<u16, [u8; 2]>) -> Vec<TextBlock>{
    let mut cdu_text = String::new();
    let cplt_cdu_text_addrs: [u16; 14] = [0x9e4e, 0x9e66, 0x9e7e, 0x9e96, 0x9eae, 0x9ec6, 0x9ede, 0x9ef6, 0x9f0e, 0x9f26, 0x9f3e, 0x9f56, 0x9f6e, 0x9f86];
    let plt_cdu_text_addrs: [u16; 14] = [0x9cdc, 0x9cf4, 0x9d0c, 0x9d24, 0x9d3c, 0x9d54, 0x9d6c, 0x9d84, 0x9d9c, 0x9db4, 0x9dcc, 0x9de4, 0x9dfc, 0x9e14];
    // let cplt_cdu_text_addrs: [u16; 2] = [0x9e4e, 0x9e66];

    let mut text_raw = "".to_string();
    for addr in cplt_cdu_text_addrs{
        text_raw = get_string_by_addr_and_len(values, addr, 24);
        cdu_text.push_str(&text_raw);
    }
    cdu_text.push_str(&" ".repeat(24*14-text_raw.len()));


    let mut testVec: Vec<TextBlock> = Vec::new();
    testVec.push(
        TextBlock {
            text: cdu_text,
            bg: (String::from("black")),
            fg: (String::from("green"))
        }
    );
    // println!("{:?}", testVec);
    return testVec;
}

pub fn get_AV8B_text(values: &HashMap<u16, [u8; 2]>) -> Vec<TextBlock>{
    let mut testVec: Vec<TextBlock> = Vec::new();


    let h2O_addrs = vec![0x78aa, 0x78ac];
    let mut h2O_text = "H2O : ".to_string();
    h2O_text += &addrs_to_65k_num_as_string(values, h2O_addrs);
    h2O_text += &"0".to_string();


    let rpm_addrs = vec![0x7890, 0x7892, 0x7894, 0x7896];
    let mut rpm_text = "RPM: ".to_string();
    rpm_text += &addrs_to_65k_num_as_string(values, rpm_addrs)[0..=3];

    let fuel_addrs = vec![0x78b2, 0x78b4, 0x78b6, 0x78b8, 0x78ba];
    let mut fuel_text = "FUEL: ".to_string();
    fuel_text += &addrs_to_65k_num_as_string(values, fuel_addrs);

    let nozzle_addrs = vec![0x78ae];
    let mut nozzle_text = "NOZ: ".to_string();
    nozzle_text += &format!("{}", (addrs_to_65k_num_as_string(values, nozzle_addrs).parse::<f32>().unwrap_or(0.0) * 12.5) as i32);

    // The space after TO is important
    let h2o_switch_lookup = ["LDG","OFF","TO "];
    let h2o_switch_pos_LE = get_value_by_address(values, 0x783c);
    let mut h2o_switch_pos_BE = u16::from_le_bytes(h2o_switch_pos_LE);
    h2o_switch_pos_BE = (h2o_switch_pos_BE & 0x000c)>>2;
    let h2o_switch_text =
        if (h2o_switch_pos_BE < 3)
            {"H2O POS: ".to_string() +
            h2o_switch_lookup[h2o_switch_pos_BE as usize]}
        else {"".to_string()};

    let flaps_switch_lookup = ["STOL","AUTO","CRSE"];
    let flaps_switch_pos_LE = get_value_by_address(values, 0x783a);
    let mut flaps_switch_pos_BE = u16::from_le_bytes(flaps_switch_pos_LE);
    flaps_switch_pos_BE = (flaps_switch_pos_BE & 0x0180)>>7;
    let flaps_switch_text =
        if (flaps_switch_pos_BE < 3)
            {"FLAPS POS: ".to_string() +
            flaps_switch_lookup[flaps_switch_pos_BE as usize]}
        else {"".to_string()};

    let combined_string = h2O_text + &" ".repeat(6) + &rpm_text + &fuel_text + &" ".repeat(4) + &nozzle_text + &" ".repeat(9-nozzle_text.len());
    // let combined_len = combined_string.len();


    // Master arm
    let master_arm = (u16::from_le_bytes(get_value_by_address(values, 0x7836))&0x4000)>>14;
    let master_arm_text = "MASTER ARM: ".to_string() + &{if(master_arm == 1) {"ON "} else {"OFF"}};

    // Landing gear
    let ldg_gear = (u16::from_le_bytes(get_value_by_address(values, 0x783a))&0x8000)>>15;
    let ldg_gear_text = "GEAR: ".to_string() + &{if(ldg_gear == 1) {"UP"} else {"DN"}};

    // Air brake
    let air_brk = (((u16::from_le_bytes(get_value_by_address(values, 0x794e)) as f32)/655.35).round() as u8).to_string();
    let air_brk_text = "AIR BRK: ".to_string() + &air_brk;

    let ldg_gear_text = "GEAR: ".to_string() + &{if(ldg_gear == 1) {"UP"} else {"DN"}};


    // A/G Master mode
    let ag_mms = (u16::from_le_bytes(get_value_by_address(values, 0x7880))&0x0800)>>11;
    let nav_mms = (u16::from_le_bytes(get_value_by_address(values, 0x7880))&0x0200)>>9;
    let vstol_mms = (u16::from_le_bytes(get_value_by_address(values, 0x7880))&0x0400)>>10;

    // The spaces here are important
    let master_mode_text = {
        if (ag_mms == 1) {"A/G "}
        else if (nav_mms   ==1) {"NAV "}
        else if (vstol_mms ==1) {"VTOL"}
        else {"    "}
    };

    let switch_text = h2o_switch_text + &" ".repeat(12) + &flaps_switch_text + &" ".repeat(9) + &master_arm_text + &" " + &ldg_gear_text + &air_brk_text + &" ".repeat(24-air_brk_text.len()) + &master_mode_text;

    testVec.push(
        TextBlock {
            text: combined_string,
            // +&" ".repeat(136-combined_len),
            bg: (String::from("black")),
            fg: (String::from("green"))
        }
    );
    testVec.push(
        TextBlock {
            text: switch_text,
            // +&" ".repeat(136-combined_len),
            bg: (String::from("black")),
            fg: (String::from("red"))
        }
    );

    return testVec;
}

pub fn get_A10C2_text(values: &HashMap<u16, [u8; 2]>) -> Vec<TextBlock>{
    let mut cdu_text = String::new();
    let cdu_addrs: [u16; 10] = [0x11c0, 0x11d8, 0x11f0, 0x1208, 0x1220, 0x1238, 0x1250, 0x1268, 0x1280, 0x1298];

    cdu_text.push_str(&" ".repeat(24*4));
    for addr in cdu_addrs{
        let text_raw = get_string_by_addr_and_len(values, addr, 24);
        cdu_text.push_str(&text_raw);
    }

    let mut testVec: Vec<TextBlock> = Vec::new();
    testVec.push(
        TextBlock {
            text: cdu_text,
            bg: (String::from("black")),
            fg: (String::from("green"))
        }
    );
    return testVec;
}

pub fn handle_A10C2_input(){
    let a10c2_button_keymappings: phf::Map<&'static str, &'static str> = phf_map! {
        "INIT REF" => "CDU_SYS TOGGLE",
        "RTE" => "CDU_NAV TOGGLE",
        "CLB" => "CDU_WP TOGGLE",
        "CRZ" => "CDU_OSET TOGGLE",
        "DES" => "CDU_MK TOGGLE",
        "PROG" => "CDU_FPM TOGGLE",
        "L3"=>"CDU_LSK_3L TOGGLE",
        "L4"=>"CDU_LSK_5L TOGGLE",
        "L5"=>"CDU_LSK_7L TOGGLE",
        "L6"=>"CDU_LSK_9L TOGGLE",
        "R1"=>"CDU_PREV TOGGLE",
        "R3"=>"CDU_LSK_3R TOGGLE",
        "R4"=>"CDU_LSK_5R TOGGLE",
        "R5"=>"CDU_LSK_7R TOGGLE",
        "R6"=>"CDU_LSK_9R TOGGLE",
        "."=>"CDU_POINT TOGGLE",
        "/"=>"CDU_SLASH TOGGLE",
        "A"=>"CDU_A TOGGLE",
        "B"=>"CDU_B TOGGLE",
        "C"=>"CDU_C TOGGLE",
        "D"=>"CDU_D TOGGLE",
        "E"=>"CDU_E TOGGLE",
        "F"=>"CDU_F TOGGLE",
        "G"=>"CDU_G TOGGLE",
        "H"=>"CDU_H TOGGLE",
        "I"=>"CDU_I TOGGLE",
        "J"=>"CDU_J TOGGLE",
        "K"=>"CDU_K TOGGLE",
        "L"=>"CDU_L TOGGLE",
        "M"=>"CDU_M TOGGLE",
        "N"=>"CDU_N TOGGLE",
        "O"=>"CDU_O TOGGLE",
        "P"=>"CDU_P TOGGLE",
        "Q"=>"CDU_Q TOGGLE",
        "R"=>"CDU_R TOGGLE",
        "S"=>"CDU_S TOGGLE",
        "T"=>"CDU_T TOGGLE",
        "U"=>"CDU_U TOGGLE",
        "V"=>"CDU_V TOGGLE",
        "W"=>"CDU_W TOGGLE",
        "X"=>"CDU_X TOGGLE",
        "Y"=>"CDU_Y TOGGLE",
        "Z"=>"CDU_Z TOGGLE",
        "1"=>"CDU_1 TOGGLE",
        "2"=>"CDU_2 TOGGLE",
        "3"=>"CDU_3 TOGGLE",
        "4"=>"CDU_4 TOGGLE",
        "5"=>"CDU_5 TOGGLE",
        "6"=>"CDU_6 TOGGLE",
        "7"=>"CDU_7 TOGGLE",
        "8"=>"CDU_8 TOGGLE",
        "9"=>"CDU_9 TOGGLE",
        "0"=>"CDU_0 TOGGLE",
        "SP"=>"CDU_SPC TOGGLE",
        "DEL"=>"CDU_BCK TOGGLE",
        "CLR"=>"CDU_CLR TOGGLE",
    };

    let a10c2_rocker_keymappings: phf::Map<&'static str, [&'static  str;2]> = phf_map! {
        "L1" => ["CDU_SCROLL 0", "CDU_SCROLL 1"],
        "L2" => ["CDU_SCROLL 2", "CDU_SCROLL 1"],
        "PREV PAGE" => ["CDU_PG 2", "CDU_PG 1"],
        "NEXT PAGE" => ["CDU_PG 0", "CDU_PG 1"],
        "BRT+" => ["CDU_DATA 2", "CDU_DATA 1"],
        "BRT-" => ["CDU_DATA 0", "CDU_DATA 1"],
    };

    let buttons = [
        "L3", "L4", "L5", "L6", "R1", "R3", "R4", "R5", "R6", "INIT REF", "RTE", "CLB", "CRZ", "DES", "PROG", ".", "/", "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z", "1", "2", "3", "4", "5", "6", "7", "8", "9", "0", "SP", "DEL", "CLR"
    ];

    let rockers = [
        "L1", "L2", "PREV PAGE", "NEXT PAGE", "BRT+", "BRT-"
    ];

    for button in buttons{
        if is_button_pressed(button){
            let message = match a10c2_button_keymappings.get(button) {
                Some(&i) => i,
                None => panic!("Wrong A10 index"), // unknown button
            };
            send_button_press(message);
        }
    }

    for rocker in rockers{
        if is_button_pressed(rocker){
            let message = match a10c2_rocker_keymappings.get(rocker) {
                Some(&i) => i,
                None => panic!("Wrong A10 index"), // unknown button
            };
            send_button_state_press(message[0], message[1]);
        }
    }
}

pub fn handle_AH64D_input(values: &HashMap<u16, [u8; 2]>){
    let is_cpg = AH64D_isCpg(values);

    let ah64d_pilot_kdu_keymappings: phf::Map<&'static str, &'static str> = phf_map! {
        "EXEC" => "_KU_EXEC TOGGLE",
        "."=>"_KU_DOT TOGGLE",
        "+-"=>"_KU_SIGN TOGGLE",
        "/"=>"_KU_SLASH TOGGLE",
        "A"=>"_KU_A TOGGLE",
        "B"=>"_KU_B TOGGLE",
        "C"=>"_KU_C TOGGLE",
        "D"=>"_KU_D TOGGLE",
        "E"=>"_KU_E TOGGLE",
        "F"=>"_KU_F TOGGLE",
        "G"=>"_KU_G TOGGLE",
        "H"=>"_KU_H TOGGLE",
        "I"=>"_KU_I TOGGLE",
        "J"=>"_KU_J TOGGLE",
        "K"=>"_KU_K TOGGLE",
        "L"=>"_KU_L TOGGLE",
        "M"=>"_KU_M TOGGLE",
        "N"=>"_KU_N TOGGLE",
        "O"=>"_KU_O TOGGLE",
        "P"=>"_KU_P TOGGLE",
        "Q"=>"_KU_Q TOGGLE",
        "R"=>"_KU_R TOGGLE",
        "S"=>"_KU_S TOGGLE",
        "T"=>"_KU_T TOGGLE",
        "U"=>"_KU_U TOGGLE",
        "V"=>"_KU_V TOGGLE",
        "W"=>"_KU_W TOGGLE",
        "X"=>"_KU_X TOGGLE",
        "Y"=>"_KU_Y TOGGLE",
        "Z"=>"_KU_Z TOGGLE",
        "1"=>"_KU_1 TOGGLE",
        "2"=>"_KU_2 TOGGLE",
        "3"=>"_KU_3 TOGGLE",
        "4"=>"_KU_4 TOGGLE",
        "5"=>"_KU_5 TOGGLE",
        "6"=>"_KU_6 TOGGLE",
        "7"=>"_KU_7 TOGGLE",
        "8"=>"_KU_8 TOGGLE",
        "9"=>"_KU_9 TOGGLE",
        "0"=>"_KU_0 TOGGLE",
        "SP"=>"_KU_SPC TOGGLE",
        "DEL"=>"_KU_BKS TOGGLE",
        "CLR"=>"_KU_CLR TOGGLE",
        "NEXT PAGE"=>"_KU_RIGHT TOGGLE",
        "PREV PAGE"=>"_KU_LEFT TOGGLE",
    };

    let buttons = [".", "/", "+-", "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z", "1", "2", "3", "4", "5", "6", "7", "8", "9", "0", "SP", "DEL", "CLR", "PREV PAGE", "NEXT PAGE", "EXEC"
    ];

    let prefix = if (is_cpg) {"CPG"} else {"PLT"};

    for button in buttons{
        if is_button_pressed(button){
            let message = match ah64d_pilot_kdu_keymappings.get(button) {
                Some(&i) => i,
                None => panic!("Wrong AH64D index"), // unknown button
            };
            send_button_press(&format!("{prefix}{message}"));
        }
    }
}

fn AH64D_isCpg(values:&HashMap<u16, [u8;2]>)->bool{
    return (u16::from_le_bytes(get_value_by_address(values, 0x8750))&0x0100) == 256;
}

fn CH47F_isCpg(values:&HashMap<u16, [u8;2]>)->bool{
    // NOT CORRECT
    return (u16::from_le_bytes(get_value_by_address(values, 0x8750))&0x0100) == 256;
}

pub fn get_AH64D_text(values: &HashMap<u16, [u8;2]>)-> Vec<TextBlock>{
    // checks
    let is_cpg = AH64D_isCpg(values);
    let own_kdu_adr: u16 = if is_cpg {0x80ac} else {0x808e};
    let other_kdu_adr: u16 = if !is_cpg {0x80ac} else {0x808e};
    let mut adv_string = String::new();
    let mut warn_str = String::new();
    let mut caut_str = String::new();
    let mut own_kdu_str = String::new();
    let mut lines: Vec<String> = Vec::new();
    let line_base_id = 0x80c2;
    let mut blocks_vec: Vec<TextBlock> = Vec::new();

    for i in 0..=6{
        let id = line_base_id + i*56;
        lines.push(get_string_by_addr_and_len(values, id, 56));
    }

    // Advisories
    for i in 0..6{
        let adv = lines[i][38..].to_string();
        adv_string += &adv;
        adv_string += &" ".repeat(24-adv.len());
    }
    blocks_vec.push(
        TextBlock {
            text: adv_string,
            bg: (String::from("black")),
            fg: (String::from("yellow"))
        }
    );

    // Warnings
    for i in 0..3{
        let warn = lines[i][19..37].to_string();
        warn_str += &warn;
        warn_str += &" ".repeat(24-warn.len());
    }
    blocks_vec.push(
        TextBlock {
            text: warn_str,
            bg: (String::from("black")),
            fg: (String::from("red"))
        }
    );

    // Cautions
    for i in 0..3{
        let caut = lines[i][0..18].to_string();
        caut_str += &caut;
        caut_str += &" ".repeat(24-caut.len());
    }
    blocks_vec.push(
        TextBlock {
            text: caut_str,
            bg: (String::from("black")),
            fg: (String::from("orange"))
        }
    );

    // Keyboard Display Unit
    own_kdu_str += &get_string_by_addr_and_len(values, own_kdu_adr, 22);
    own_kdu_str += "  ";
    blocks_vec.push(
        TextBlock {
            text: own_kdu_str,
            bg: (String::from("black")),
            fg: (String::from("green"))
        }
    );

    let other_kdu_string = get_string_by_addr_and_len(values, other_kdu_adr, 22);
    blocks_vec.push(
        TextBlock {
            text: other_kdu_string,
            bg: (String::from("black")),
            fg: (String::from("white"))
        }
    );
    return blocks_vec;
}

pub fn get_module_name(values: &HashMap<u16, [u8; 2]>) -> String{
    return get_string_by_addr_and_len(values, 0x0000, 24);
}