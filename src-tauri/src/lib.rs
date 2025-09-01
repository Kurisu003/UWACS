#![allow(non_snake_case)]
#![allow(unused_imports)]
#![allow(unused_parens)]
#![allow(non_camel_case_types)]

use std::{
    env,
    io::{BufRead, BufReader},
    path::PathBuf,
    process::{Command, Stdio},
    sync::mpsc::{channel, Receiver},
    thread,
};

/// Spawn a child process and stream its output through channels.
///
/// This function starts the process located at `current_dir` and
/// returns receivers for both stdout and stderr. The process output
/// is read line by line on background threads so the caller can
/// consume the lines without blocking.
fn test(current_dir: &PathBuf) -> (Receiver<String>, Receiver<String>) {
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

    (stdout_rx, stderr_rx)
}

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn start(program: String) -> String {
    let mut current_dir: PathBuf = env::current_dir().unwrap();
    current_dir.pop();
    current_dir.push("children");

    current_dir = current_dir.join(program + ".exe");


    let (stdout_rx, stderr_rx) = test(&current_dir);

    // Example: forward process output to the console.
    thread::spawn(move || {
        for line in stdout_rx {
            println!("OUT: {}", line);
        }
    });
    thread::spawn(move || {
        for line in stderr_rx {
            eprintln!("ERR: {}", line);
        }
    });

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
