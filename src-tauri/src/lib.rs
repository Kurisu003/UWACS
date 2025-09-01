#![allow(non_snake_case)]
#![allow(unused_imports)]
#![allow(unused_parens)]
#![allow(non_camel_case_types)]

use std::{env, io::{stdout, Read}, os::windows::process::ExitStatusExt, path::PathBuf, process::{Command, ExitStatus, Output, Stdio}, ptr::null, sync::mpsc::Sender, thread};
use std::sync::mpsc::channel;

use tauri::ipc::Channel;

fn test(current_dir: &PathBuf) {
    let output = Command::new(current_dir)
        .args(["--flag", "value"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to init process");

    let std_out = match output.stdout {
        Some(a) => a,
        None =>   panic!("A"),
    };
    // println!("{:?}", std_out.read(buf));

    //Continously send to sender here

}

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn start(program: String) -> String {
    let mut current_dir: PathBuf = env::current_dir().unwrap();
    current_dir.pop();
    current_dir.push("children");

    current_dir = current_dir.join(program + ".exe");


    let handle = thread::spawn(move || {
        test(&current_dir)

    });

    // println!("{:?}", handle.join());
    // if (String::from_utf8_lossy(&handle.join().stderr).starts_with("Error: No HID interfaces with")){
    //     return "No HID found".to_string();
    // }
    // else if (String::from_utf8_lossy(&handle.stderr).starts_with("Error: Failed to open HID path")){
    //     return "Init file for PFP not found".to_string();
    // }
    //     println!("EXT: {}", output.status);
    //     println!("OUT:\n{}", String::from_utf8_lossy(&output.stdout));
    //     eprintln!("ERR\n{}", String::from_utf8_lossy(&output.stderr));

    format!("Hello, string")
}

#[tauri::command]
fn change_filepath(file_path: String){
    println!("{:?}", file_path);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![start, change_filepath])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}