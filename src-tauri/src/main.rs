// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{env, path::PathBuf};


fn main() {
    let mut current_dir: PathBuf = env::current_dir().unwrap();
    current_dir.pop();
    current_dir.push("children");
    current_dir = current_dir.join("AH-64D_MFCD_leaderLine.png");

    uwacs_lib::run()
}
