#![allow(non_snake_case)]
#![allow(unused_imports)]
#![allow(unused_parens)]
#![allow(non_camel_case_types)]

use std::{
    env,
    io::{BufRead, BufReader},
    path::PathBuf,
    process::{Command, Stdio},
    sync::{mpsc::{channel, Receiver}, Mutex},
    thread,
};
use once_cell::sync::Lazy;
use sysinfo::System;


static OUTPUT_CHANNELS: Lazy<Mutex<Option<(Receiver<String>, Receiver<String>)>>> = Lazy::new(|| Mutex::new(None));

pub fn kill_by_name(name: &String){
    let mut sys = System::new_all();
    sys.refresh_all();

    for (pid, process) in sys.processes() {
        if (process.name().to_string_lossy().starts_with(name)){
            println!("[{pid}] {:?} {:?}", process.name(), process.disk_usage());
            process.kill();
        }
    }
}

/// Spawn a child process and stream its output through channels.
/// This function starts the process located at `current_dir` and
/// stores receivers for both stdout and stderr in a global so the
/// output can be retrieved later without providing any arguments.
fn spawn_child_from_path(current_dir: &PathBuf) {

    let mut child = Command::new(current_dir)
        .args(["--flag", "value"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to init process");

    let stdout = child.stdout.take().expect("Failed to capture stdout");
    let stderr = child.stderr.take().expect("Failed to capture stderr");

    let (stdout_tx, stdout_rx) = channel::<String>();
    let (stderr_tx, stderr_rx) = channel::<String>();

    // Make receivers available to `read_available`.
    *OUTPUT_CHANNELS.lock().unwrap() = Some((stdout_rx, stderr_rx));


    // Read stdout in a dedicated thread
    thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            match line {
                Ok(line) => {
                    if stdout_tx.send(line).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    // Read stderr in a dedicated thread
    thread::spawn(move || {
        let reader = BufReader::new(stderr);
        for line in reader.lines() {
            match line {
                Ok(line) => {
                    if stderr_tx.send(line).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    // Keep the child process alive until it exits.
    thread::spawn(move || {
        let _ = child.wait();
    });
}

/// Drain all currently available lines from the stored receivers without blocking.
#[tauri::command]
fn read_available() -> (Vec<String>, Vec<String>) {
    let guard = OUTPUT_CHANNELS.lock().unwrap();
    if let Some((stdout_rx, stderr_rx)) = guard.as_ref() {
        let out: Vec<String> = stdout_rx.try_iter().collect();
        let err: Vec<String> = stderr_rx.try_iter().collect();
        (out, err)
    } else {
        (Vec::new(), Vec::new())
    }

}

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn start(program: String){
    // done for safety to prevent executing/killing
    // anything that gets sent from the frontend
    let mut program_actual = program.clone();
    if(program == "pfp_writer"){
        program_actual = "pfp_writer".to_string();
    }
    else if (program == "ufc_writer") {
        program_actual = "ufc_writer".to_string();
    }
    else if (program.starts_with("overlay")) {
        program_actual = "FluidPort".to_string();
    }

    // get path to program
    let mut current_dir: PathBuf = env::current_dir().unwrap();
    current_dir.pop();
    current_dir.push("children");

    // kill old program if running
    if(program != "overlay+"){
        let _ = kill_by_name(&program_actual);
    }

    // start new program
    if(program != "overlay-"){
        current_dir = current_dir.join(program_actual + ".exe");
        spawn_child_from_path(&current_dir);
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![start,read_available])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
