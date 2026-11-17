//! UWACS - Universal Winwing Accessories Control System.
//! One program for: seeing connected WinWing panels and instrument displays, remapping any panel
//! button to a keyboard shortcut, arranging/rotating/flipping monitors, the UFC and PFP DCS-BIOS
//! writers, the FluidPort overlay and the DCS MonitorSetup file.

mod app;
mod calc;
mod config;
mod custom;
mod dcssetup;
mod devicemaps;
mod dcsbios;
mod devices;
mod displays;
mod hidreport;
mod inputs;
mod log;
mod monitorsetup;
mod overlay;
mod pfp;
mod pfp_aircraft;
mod remap;
mod rwr;
mod status;
mod ufc;
mod update;

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, RwLock};
use std::time::Duration;

const HELP: &str = "uwacs [--list | --headless | --help]

  (no args)    open the UWACS window
  --version    print the version
  --update     check GitHub for a newer UWACS and install it
  --list       print connected WinWing panels, instrument displays and monitors, then exit
  --headless   run the UFC/PFP writers and button remapping without a window (Ctrl+C to stop)
  --dcs-setup FOLDER
               install DCS-BIOS and the monitor setup into the DCS at FOLDER (prefix, game or Saved Games/DCS)
";

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(|s| s.as_str()) {
        Some("--help") | Some("-h") => {
            print!("{HELP}");
            Ok(())
        }
        Some("--version") | Some("-V") => {
            println!("uwacs {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some("--list") => list(),
        Some("--update") => update::cli(&config::load().update_repo),
        Some("--headless") => headless(),
        Some("--test-key") => {
            // presses and releases a shortcut through the same virtual keyboard the mappings use
            let combo = args.get(1).cloned().unwrap_or_else(|| "F13".into());
            remap::test_key(&combo)
        }
        Some("--dcs-setup") => {
            let dir = args.get(1).ok_or_else(|| anyhow::anyhow!("--dcs-setup needs a folder"))?;
            let prog: dcssetup::Shared = Default::default();
            let opts = dcssetup::Options { install_bios: true, write_monitor_setup: true };
            dcssetup::run(dir.into(), opts, config::load().monitor_setup, Vec::new(), prog.clone());
            let g = prog.lock().unwrap();
            for s in &g.steps {
                println!("{:<50} {:?}", s.title, s.state);
            }
            Ok(())
        }
        Some(other) => anyhow::bail!("unknown argument {other}\n\n{HELP}"),
        None => gui(),
    }
}

fn gui() -> anyhow::Result<()> {
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("UWACS")
            .with_app_id("uwacs")
            .with_inner_size([1100.0, 760.0])
            .with_min_inner_size([700.0, 450.0]),
        ..Default::default()
    };
    eframe::run_native("UWACS", opts, Box::new(|cc| Ok(Box::new(app::App::new(cc)))))
        .map_err(|e| anyhow::anyhow!("{e}"))
}

fn list() -> anyhow::Result<()> {
    let devs = devices::enumerate_hid();
    println!("WinWing HID devices: {}", devs.len());
    for d in &devs {
        let desc = devices::report_descriptor(&d.path, None);
        let layout = hidreport::parse(&desc);
        println!(
            "  {}  {:04X}:{:04X} if{}  serial '{}'  {}  {} buttons  {}",
            d.display_name(),
            d.vid,
            d.pid,
            d.interface,
            d.serial,
            d.path,
            layout.buttons.len(),
            match &d.access {
                Ok(()) => "access OK".to_string(),
                Err(e) => e.clone(),
            }
        );
    }
    let disp = devices::enumerate_usb_displays();
    println!("USB instrument displays (DisplayLink): {}", disp.len());
    for d in &disp {
        println!("  {} ({})  17E9:{:04X}  {} Mb/s  serial {}", d.product, d.manufacturer, d.pid, d.speed_mbps, d.serial);
    }
    if !disp.is_empty() {
        println!("  displaylink.service active: {:?}", devices::displaylink_service_active());
    }
    match displays::read() {
        Ok(outs) => {
            println!("Monitors:");
            for o in outs {
                let (w, h) = o.logical_size();
                println!(
                    "  {:10} {}  {}x{} @{}  at {},{}  logical {:.0}x{:.0}  {}{}",
                    o.name,
                    if o.enabled { "on " } else { "off" },
                    o.mode_w,
                    o.mode_h,
                    o.scale,
                    o.x,
                    o.y,
                    w,
                    h,
                    o.rotation.arg(),
                    if o.is_usb_display() { "  (USB instrument display)" } else { "" }
                );
            }
        }
        Err(e) => println!("Monitors: {e}"),
    }
    Ok(())
}

fn headless() -> anyhow::Result<()> {
    let cfg = config::load();
    *custom::SCREENS.write().unwrap() = cfg.custom_screens.clone();
    *monitorsetup::DCS_FOLDER.write().unwrap() = cfg.dcs_folder.clone();
    dcsbios::start(&cfg.dcsbios_host, cfg.dcsbios_port);
    remap::start(Arc::new(RwLock::new(cfg.remaps.clone())));
    let auto = Arc::new(AtomicBool::new(cfg.pfp_auto_buttons));
    let mut ufc: Option<std::thread::JoinHandle<()>> = None;
    let mut pfp: Option<std::thread::JoinHandle<()>> = None;
    log::log("UWACS headless: writers + remapping running, Ctrl+C to stop");
    loop {
        let devs = devices::enumerate_hid();
        inputs::sync_devices(&devs);
        let first = |r: devices::Role| devs.iter().filter(|d| d.role == r && d.access.is_ok()).min_by_key(|d| d.interface).map(|d| d.path.clone());
        if cfg.ufc_autostart && ufc.as_ref().map(|h| h.is_finished()).unwrap_or(true) {
            if let Some(p) = first(devices::Role::Ufc) {
                let stop = Arc::new(AtomicBool::new(false));
                ufc = Some(std::thread::spawn(move || ufc::run(p, stop)));
            }
        }
        if cfg.pfp_autostart && pfp.as_ref().map(|h| h.is_finished()).unwrap_or(true) {
            if let Some(p) = first(devices::Role::Pfp) {
                let stop = Arc::new(AtomicBool::new(false));
                let a = auto.clone();
                pfp = Some(std::thread::spawn(move || pfp::run(p, stop, a)));
            }
        }
        std::thread::sleep(Duration::from_secs(5));
    }
}
