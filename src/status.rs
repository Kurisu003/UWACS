//! Small shared status board for the GUI (writer states etc.).

use std::collections::HashMap;
use std::sync::Mutex;

#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    Stopped,
    Waiting(String),
    Running,
    Error(String),
}

static BOARD: Mutex<Option<HashMap<String, Status>>> = Mutex::new(None);

pub fn set_status(name: &str, s: Status) {
    BOARD.lock().unwrap().get_or_insert_with(HashMap::new).insert(name.to_string(), s);
}

pub fn get_status(name: &str) -> Status {
    BOARD.lock().unwrap().as_ref().and_then(|m| m.get(name).cloned()).unwrap_or(Status::Stopped)
}
