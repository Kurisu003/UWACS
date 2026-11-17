//! Reads button reports from every connected WinWing panel and fans the press/release events out
//! to the remapper and the PFP writer.

use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Mutex;
use std::thread;
use std::time::Instant;

use crate::devices::{self, HidDev};
use crate::hidreport::{self, ButtonRef, Layout};
use crate::log::log;

#[derive(Clone, Debug)]
pub struct ButtonEvent {
    pub dev_key: String,
    pub dev_path: String,
    pub usage: u32,
    pub report_id: u8,
    pub bit: u32,
    pub pressed: bool,
}

pub struct DeviceInput {
    pub dev: HidDev,
    pub layout: Layout,
    pub state: Vec<bool>,
    /// Raw axis values (wheels, sliders), same order as layout.axes.
    pub axes: Vec<i64>,
    /// Last report with id 1, which the PFP code reads its legacy key bits from.
    pub report1: Vec<u8>,
    pub reports_seen: u64,
    pub last_report: Option<Instant>,
    pub error: Option<String>,
    pub running: bool,
}

static HUB: Mutex<Option<HashMap<String, DeviceInput>>> = Mutex::new(None);
static SUBS: Mutex<Vec<Sender<ButtonEvent>>> = Mutex::new(Vec::new());

pub fn subscribe() -> Receiver<ButtonEvent> {
    let (tx, rx) = channel();
    SUBS.lock().unwrap().push(tx);
    rx
}

fn publish(ev: ButtonEvent) {
    SUBS.lock().unwrap().retain(|tx| tx.send(ev.clone()).is_ok());
}

pub fn with_hub<T>(f: impl FnOnce(&mut HashMap<String, DeviceInput>) -> T) -> T {
    let mut g = HUB.lock().unwrap();
    f(g.get_or_insert_with(HashMap::new))
}

/// Is this button currently held on the device at `path`?
pub fn is_down(path: &str, usage: u32) -> bool {
    with_hub(|h| h.get(path).map(|x| x.layout.buttons.iter().zip(&x.state).any(|(b, s)| b.usage == usage && *s)).unwrap_or(false))
}

/// Legacy PFP key state: bits of report 1, bytes 1..10, as a 72-entry array.
pub fn pfp_pressed(path: &str) -> [bool; 72] {
    let mut out = [false; 72];
    with_hub(|h| {
        if let Some(d) = h.get(path) {
            for (i, o) in out.iter_mut().enumerate() {
                let byte = d.report1.get(1 + i / 8).copied().unwrap_or(0);
                *o = byte >> (i % 8) & 1 == 1;
            }
        }
    });
    out
}

/// Start a reader for every accessible device that does not have one yet.
pub fn sync_devices(devs: &[HidDev]) {
    for d in devs {
        if d.access.is_err() {
            continue;
        }
        let start = with_hub(|h| {
            let running = h.get(&d.path).map(|x| x.running).unwrap_or(false);
            if !running {
                h.insert(
                    d.path.clone(),
                    DeviceInput {
                        dev: d.clone(),
                        layout: Layout::default(),
                        state: Vec::new(),
                        axes: Vec::new(),
                        report1: Vec::new(),
                        reports_seen: 0,
                        last_report: None,
                        error: None,
                        running: true,
                    },
                );
            }
            !running
        });
        if start {
            let d = d.clone();
            thread::Builder::new()
                .name(format!("hid-read {}", d.path))
                .spawn(move || reader(d))
                .ok();
        }
    }
    // forget devices that are gone and not running
    with_hub(|h| h.retain(|p, x| x.running || devs.iter().any(|d| &d.path == p)));
}

fn set_error(path: &str, e: String) {
    with_hub(|h| {
        if let Some(x) = h.get_mut(path) {
            x.error = Some(e);
            x.running = false;
        }
    });
}

fn reader(d: HidDev) {
    let dev = match devices::open(&d.path) {
        Ok(x) => x,
        Err(e) => {
            set_error(&d.path, e.to_string());
            return;
        }
    };
    let desc = devices::report_descriptor(&d.path, Some(&dev));
    let mut layout = hidreport::parse(&desc);
    let parsed = !layout.buttons.is_empty();
    log(format!(
        "Input: {} ({}) {} buttons{}",
        d.display_name(),
        d.path,
        layout.buttons.len(),
        if parsed { "" } else { " in descriptor, using raw bits" }
    ));
    with_hub(|h| {
        if let Some(x) = h.get_mut(&d.path) {
            x.layout = layout.clone();
            x.state = vec![false; layout.buttons.len()];
        }
    });

    let key = d.key();
    let mut state: Vec<bool> = vec![false; layout.buttons.len()];
    let mut axes: Vec<Option<i64>> = vec![None; layout.axes.len()];
    let mut buf = [0u8; 256];
    loop {
        let n = match dev.read_timeout(&mut buf, 250) {
            Ok(n) => n,
            Err(e) => {
                log(format!("Input: {} disconnected ({e})", d.display_name()));
                set_error(&d.path, e.to_string());
                return;
            }
        };
        if n == 0 {
            continue;
        }
        let report = &buf[..n];

        // No usable descriptor: treat every payload bit of report 1 as a button.
        if !parsed && report[0] == 1 && layout.buttons.len() < (n - 1) * 8 {
            layout.uses_report_ids = true;
            for bit in layout.buttons.len() as u32..((n - 1) * 8) as u32 {
                layout.buttons.push(ButtonRef { report_id: 1, bit, usage: bit + 1 });
            }
            state.resize(layout.buttons.len(), false);
            let l = layout.clone();
            with_hub(|h| {
                if let Some(x) = h.get_mut(&d.path) {
                    x.layout = l;
                }
            });
        }

        let mut events = Vec::new();
        // a wheel that moved becomes one tap of "up" or "down" per report
        for (i, a) in layout.axes.iter().enumerate() {
            if let Some(v) = hidreport::axis_value(report, &layout, a) {
                if let Some(old) = axes[i] {
                    if v != old {
                        let usage = if v > old { hidreport::axis_up(i) } else { hidreport::axis_down(i) };
                        for pressed in [true, false] {
                            events.push(ButtonEvent { dev_key: key.clone(), dev_path: d.path.clone(), usage, report_id: a.report_id, bit: a.bit, pressed });
                        }
                    }
                }
                axes[i] = Some(v);
            }
        }
        for (i, b) in layout.buttons.iter().enumerate() {
            if let Some(on) = hidreport::bit_set(report, &layout, b) {
                if on != state[i] {
                    state[i] = on;
                    events.push(ButtonEvent {
                        dev_key: key.clone(),
                        dev_path: d.path.clone(),
                        usage: b.usage,
                        report_id: b.report_id,
                        bit: b.bit,
                        pressed: on,
                    });
                }
            }
        }
        with_hub(|h| {
            if let Some(x) = h.get_mut(&d.path) {
                x.state = state.clone();
                x.axes = axes.iter().map(|v| v.unwrap_or(0)).collect();
                x.reports_seen += 1;
                x.last_report = Some(Instant::now());
                if report[0] == 1 {
                    x.report1 = report.to_vec();
                }
            }
        });
        for ev in events {
            publish(ev);
        }
    }
}
