//! The UWACS window: sidebar navigation, one page per job, every page explains itself.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use egui::{Color32, RichText, Vec2};

use crate::calc;
use crate::config::{self, Action, Config, OverlayCfg, Remap};
use crate::dcssetup;
use crate::devicemaps::{self, Source, Template};
use crate::dcsbios;
use crate::devices::{self, HidDev, Role, UsbDisplay};
use crate::displays::{self, Output, Rotation};
use crate::inputs;
use crate::log;
use crate::monitorsetup;
use crate::overlay;
use crate::pfp;
use crate::remap;
use crate::status::{get_status, Status};
use crate::ufc;

mod extra;

const GREEN: Color32 = Color32::from_rgb(90, 200, 120);
const RED: Color32 = Color32::from_rgb(235, 95, 85);
const AMBER: Color32 = Color32::from_rgb(235, 175, 50);
const GREY: Color32 = Color32::from_rgb(140, 140, 140);
const ACCENT: Color32 = Color32::from_rgb(70, 140, 220);
const DCSBIOS_URL: &str = "https://github.com/DCS-Skunkworks/dcs-bios/releases";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Overview,
    Panels,
    Remap,
    Screens,
    Overlay,
    Dcs,
    Settings,
    Log,
}

const PAGES: [(Page, &str, &str); 8] = [
    (Page::Overview, "Overview", "Status and checklist"),
    (Page::Panels, "Panel screens", "UFC and PFP from DCS"),
    (Page::Remap, "Button mapping", "Panel buttons to keys"),
    (Page::Screens, "Screens", "Arrange, rotate, flip"),
    (Page::Overlay, "MFD overlay", "Frame over exports"),
    (Page::Dcs, "DCS setup", "DCS-BIOS and exports"),
    (Page::Settings, "Settings", "Updates, export"),
    (Page::Log, "Log", "What happened"),
];

#[derive(Default)]
struct Worker {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
    last_start: Option<Instant>,
}

impl Worker {
    fn running(&self) -> bool {
        self.handle.as_ref().map(|h| !h.is_finished()).unwrap_or(false)
    }

    fn start(&mut self, path: &str, f: impl FnOnce(String, Arc<AtomicBool>) + Send + 'static) {
        self.stop_now();
        let stop = Arc::new(AtomicBool::new(false));
        self.stop = stop.clone();
        self.last_start = Some(Instant::now());
        let p = path.to_string();
        self.handle = Some(thread::spawn(move || f(p, stop)));
    }

    fn stop_now(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

struct Teach {
    path: String,
    template: String,
    ctrl: String,
    since: Instant,
}

/// An overlay with its expressions worked out.
#[derive(Clone, Debug, Default)]
struct Placed {
    output: String,
    /// output top-left on the desktop and its size
    screen: (f32, f32, f32, f32),
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

struct PendingDisplay {
    before: Vec<Output>,
    deadline: Instant,
}

pub struct App {
    cfg: Config,
    remaps: Arc<RwLock<Vec<Remap>>>,
    page: Page,
    dirty: bool,
    last_save: Instant,

    devs: Vec<HidDev>,
    usb_displays: Vec<UsbDisplay>,
    dl_active: Option<bool>,
    dcsbios_installed: Option<bool>,
    last_scan: Option<Instant>,

    ufc: Worker,
    pfp: Worker,
    pfp_auto: Arc<AtomicBool>,

    record_for: Option<usize>,

    outputs: Vec<Output>,
    outputs_edit: Vec<Output>,
    outputs_err: Option<String>,
    displays_msg: String,
    drag: Option<(usize, Vec2)>,
    pending: Option<PendingDisplay>,

    overlay_on: bool,
    overlay_tex: std::collections::HashMap<String, egui::TextureHandle>,
    /// picture path -> (size, see-through window)
    overlay_holes: std::collections::HashMap<String, ([usize; 2], Option<(usize, usize, usize, usize)>)>,
    overlay_place_at: Option<Instant>,
    overlay_msg: String,

    /// Button mapping pictures: (hidraw path, control id)
    selected: Option<(String, String)>,
    /// Waiting for the user to press the real button of this control.
    teach: Option<Teach>,
    /// A new mapping waiting for its keyboard shortcut.
    record_new: Option<Remap>,
    /// Teach-all walkthrough: (hidraw path, step)
    teach_all: Option<(String, usize)>,
    /// Profile tab shown per panel, and the switch position last seen
    profile_tab: std::collections::HashMap<String, Option<u32>>,
    profile_seen: std::collections::HashMap<String, Option<u32>>,
    /// last profile switch pressed per panel
    profile_last: std::collections::HashMap<String, u32>,
    last_press_seen: Option<Instant>,

    setup_dir: String,
    setup_bios: bool,
    setup_ms: bool,
    setup_prog: Option<dcssetup::Shared>,
    setup_pick: Arc<std::sync::Mutex<Option<Option<std::path::PathBuf>>>>,
    copied: Option<(String, Instant)>,

    aircraft_cache: Option<Vec<String>>,
    names_aircraft: String,
    names_search: String,
    dialog: extra::DialogSlot,
    settings_msg: String,

    ms_msg: String,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let _ = remap::EGUI_CTX.set(cc.egui_ctx.clone());
        setup_style(&cc.egui_ctx);
        let cfg = config::load();
        *crate::custom::SCREENS.write().unwrap() = cfg.custom_screens.clone();
        *monitorsetup::DCS_FOLDER.write().unwrap() = cfg.dcs_folder.clone();
        let remaps = Arc::new(RwLock::new(cfg.remaps.clone()));
        dcsbios::start(&cfg.dcsbios_host, cfg.dcsbios_port);
        remap::start(remaps.clone());
        let pfp_auto = Arc::new(AtomicBool::new(cfg.pfp_auto_buttons));
        let mut app = Self {
            cfg,
            remaps,
            page: Page::Overview,
            dirty: false,
            last_save: Instant::now(),
            devs: Vec::new(),
            usb_displays: Vec::new(),
            dl_active: None,
            dcsbios_installed: None,
            last_scan: None,
            ufc: Worker::default(),
            pfp: Worker::default(),
            pfp_auto,
            record_for: None,
            outputs: Vec::new(),
            outputs_edit: Vec::new(),
            outputs_err: None,
            displays_msg: String::new(),
            drag: None,
            pending: None,
            overlay_on: false,
            overlay_tex: Default::default(),
            overlay_holes: Default::default(),
            overlay_place_at: None,
            overlay_msg: String::new(),
            selected: None,
            teach: None,
            record_new: None,
            teach_all: None,
            profile_tab: Default::default(),
            profile_seen: Default::default(),
            profile_last: Default::default(),
            last_press_seen: None,
            setup_dir: monitorsetup::saved_games_dir().map(|p| p.display().to_string()).unwrap_or_default(),
            setup_bios: true,
            setup_ms: true,
            setup_prog: None,
            setup_pick: Default::default(),
            copied: None,
            aircraft_cache: None,
            names_aircraft: String::new(),
            names_search: String::new(),
            dialog: Default::default(),
            settings_msg: String::new(),
            ms_msg: String::new(),
        };
        app.reload_outputs();
        if let Ok(t) = std::env::var("UWACS_TAB") {
            app.page = match t.to_ascii_lowercase().as_str() {
                "panels" => Page::Panels,
                "remap" => Page::Remap,
                "displays" | "screens" => Page::Screens,
                "overlay" => Page::Overlay,
                "dcs" => Page::Dcs,
                "settings" => Page::Settings,
                "log" => Page::Log,
                _ => Page::Overview,
            };
        }
        log::log(format!("UWACS {} started, settings in {}", env!("CARGO_PKG_VERSION"), config::path().display()));
        app
    }

    fn scan(&mut self) {
        self.devs = devices::enumerate_hid();
        self.usb_displays = devices::enumerate_usb_displays();
        self.dl_active = devices::displaylink_service_active();
        self.dcsbios_installed = monitorsetup::dcsbios_installed();
        inputs::sync_devices(&self.devs);
        self.last_scan = Some(Instant::now());
        // tell the remapper which buttons are profile switches on every panel, whatever page is open
        let panels: Vec<(String, u16, Vec<u32>)> = inputs::with_hub(|h| h.values().map(|x| (x.dev.key(), x.dev.pid, x.layout.buttons.iter().map(|b| b.usage).collect())).collect());
        for (key, pid, usages) in panels {
            let t = devicemaps::template_for(pid, &usages);
            let sw: Vec<u32> = self.profile_switches(&t).iter().map(|s| s.0).collect();
            remap::PROFILE_SWITCHES.lock().unwrap().insert(key, sw);
        }

        let cooldown = |w: &Worker| w.last_start.map(|t| t.elapsed() > Duration::from_secs(15)).unwrap_or(true);
        if self.cfg.ufc_autostart && !self.ufc.running() && cooldown(&self.ufc) {
            if let Some(d) = self.first_ok(Role::Ufc) {
                self.ufc.start(&d, ufc::run);
            }
        }
        if self.cfg.pfp_autostart && !self.pfp.running() && cooldown(&self.pfp) {
            if let Some(d) = self.first_ok(Role::Pfp) {
                let auto = self.pfp_auto.clone();
                self.pfp.start(&d, move |p, s| pfp::run(p, s, auto));
            }
        }
    }

    fn first_ok(&self, role: Role) -> Option<String> {
        self.devs.iter().filter(|d| d.role == role && d.access.is_ok()).min_by_key(|d| d.interface).map(|d| d.path.clone())
    }

    fn has_role(&self, role: Role) -> bool {
        self.devs.iter().any(|d| d.role == role)
    }

    fn reload_outputs(&mut self) {
        match displays::read() {
            Ok(o) => {
                self.outputs = o.clone();
                self.outputs_edit = o;
                self.outputs_err = None;
            }
            Err(e) => self.outputs_err = Some(e.to_string()),
        }
    }

    fn save_soon(&mut self) {
        self.dirty = true;
    }

    fn sync_remaps(&mut self) {
        *self.remaps.write().unwrap() = self.cfg.remaps.clone();
        self.save_soon();
    }

    /// One entry per physical panel (a panel can expose several HID interfaces).
    fn panels(&self) -> Vec<&HidDev> {
        let mut seen = std::collections::HashSet::new();
        self.devs.iter().filter(|d| seen.insert((d.pid, d.serial.clone()))).collect()
    }

    #[allow(dead_code)]
    fn device_name(&self, key: &str) -> String {
        self.devs.iter().find(|d| d.key() == key).map(|d| d.display_name()).unwrap_or_else(|| key.to_string())
    }

    fn button_label(&self, dev_key: &str, usage: u32, report_id: u8, bit: u32) -> String {
        let is_pfp = self.devs.iter().any(|d| d.key() == dev_key && d.role == Role::Pfp);
        if is_pfp && report_id == 1 {
            if let Some(n) = pfp::key_name(bit) {
                return format!("{n} key (button {usage})");
            }
        }
        format!("Button {usage}")
    }

    fn usb_output_name(&self, i: usize) -> Option<String> {
        self.outputs.iter().filter(|o| o.is_usb_display()).nth(i).map(|o| o.name.clone())
    }
}

fn setup_style(ctx: &egui::Context) {
    use egui::{FontId, TextStyle};
    ctx.all_styles_mut(|s| {
        s.text_styles.insert(TextStyle::Heading, FontId::proportional(24.0));
        s.text_styles.insert(TextStyle::Body, FontId::proportional(15.0));
        s.text_styles.insert(TextStyle::Button, FontId::proportional(15.0));
        s.text_styles.insert(TextStyle::Small, FontId::proportional(12.5));
        s.text_styles.insert(TextStyle::Monospace, FontId::monospace(13.5));
        s.spacing.item_spacing = Vec2::new(8.0, 7.0);
        s.spacing.button_padding = Vec2::new(10.0, 5.0);
        s.spacing.interact_size.y = 26.0;
    });
}

impl eframe::App for App {
    fn clear_color(&self, _v: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if self.last_scan.map(|t| t.elapsed() > Duration::from_secs(2)).unwrap_or(true) {
            self.scan();
        }
        if self.dirty && self.last_save.elapsed() > Duration::from_millis(800) {
            self.cfg.remaps = self.remaps.read().unwrap().clone();
            if let Err(e) = config::save(&self.cfg) {
                log::log(format!("Settings: save failed: {e}"));
            }
            self.dirty = false;
            self.last_save = Instant::now();
        }
        self.handle_overlay_requests();
        self.handle_teach_and_presses();
        crate::update::tick(self.cfg.auto_update, &self.cfg.update_repo);
        self.handle_display_revert();
        self.capture_shortcut(&ctx);

        egui::Panel::left("nav").exact_size(230.0).resizable(false).show(ui, |ui| self.ui_nav(ui));

        egui::CentralPanel::default().show(ui, |ui| {
            let mut sa = egui::ScrollArea::vertical().auto_shrink([false, false]);
            // tests: UWACS_SCROLL=600 screenshots further down the page
            if let Some(y) = std::env::var("UWACS_SCROLL").ok().and_then(|v| v.parse::<f32>().ok()) {
                sa = sa.vertical_scroll_offset(y);
            }
            sa.show(ui, |ui| {
                ui.set_max_width(ui.available_width().min(1000.0));
                ui.add_space(6.0);
                match self.page {
                    Page::Overview => self.ui_overview(ui),
                    Page::Panels => self.ui_panels(ui),
                    Page::Remap => self.ui_remap(ui),
                    Page::Screens => self.ui_screens(ui),
                    Page::Overlay => self.ui_overlay(ui),
                    Page::Dcs => self.ui_dcs(ui),
                    Page::Settings => self.ui_settings(ui),
                    Page::Log => self.ui_log(ui),
                }
                ui.add_space(20.0);
            });
        });

        self.show_overlay(&ctx);
        self.test_screenshot(&ctx);
        ctx.request_repaint_after(Duration::from_millis(200));
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.cfg.remaps = self.remaps.read().unwrap().clone();
        let _ = config::save(&self.cfg);
        self.ufc.stop_now();
        self.pfp.stop_now();
    }
}

// ------------------------------------------------------------------ small widgets

fn dot(ui: &mut egui::Ui, color: Color32) {
    let (r, _) = ui.allocate_exact_size(Vec2::splat(14.0), egui::Sense::hover());
    ui.painter().circle_filled(r.center(), 5.5, color);
}

fn page_header(ui: &mut egui::Ui, title: &str, intro: &str) {
    ui.label(RichText::new(title).heading().strong());
    ui.add_space(2.0);
    ui.label(RichText::new(intro).color(ui.visuals().weak_text_color()));
    ui.add_space(10.0);
}

fn how_to(ui: &mut egui::Ui, id: &str, steps: &[&str]) {
    egui::CollapsingHeader::new(RichText::new("How do I use this?").color(ACCENT)).id_salt(id).default_open(false).show(ui, |ui| {
        for (i, s) in steps.iter().enumerate() {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(format!("{}.", i + 1)).strong());
                ui.label(*s);
            });
        }
    });
    ui.add_space(6.0);
}

fn card<R>(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let fill = if ui.visuals().dark_mode { Color32::from_gray(32) } else { Color32::from_gray(245) };
    let r = egui::Frame::new()
        .fill(fill)
        .stroke(egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color))
        .corner_radius(8.0)
        .inner_margin(egui::Margin::same(14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            if !title.is_empty() {
                ui.label(RichText::new(title).size(17.0).strong());
                ui.add_space(4.0);
            }
            add(ui)
        })
        .inner;
    ui.add_space(10.0);
    r
}

fn hint(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).small().color(ui.visuals().weak_text_color()));
}

/// A checklist row: dot, title, explanation underneath.
fn check_row(ui: &mut egui::Ui, color: Color32, title: &str, detail: &str) {
    ui.horizontal(|ui| {
        dot(ui, color);
        ui.label(RichText::new(title).strong());
    });
    if !detail.is_empty() {
        ui.horizontal_wrapped(|ui| {
            ui.add_space(22.0);
            ui.label(RichText::new(detail).color(ui.visuals().weak_text_color()));
        });
    }
    ui.add_space(4.0);
}

// ------------------------------------------------------------------ navigation

impl App {
    fn ui_nav(&mut self, ui: &mut egui::Ui) {
        ui.add_space(10.0);
        ui.label(RichText::new("UWACS").size(26.0).strong());
        ui.label(RichText::new("WinWing control for DCS").small().color(ui.visuals().weak_text_color()));
        ui.add_space(14.0);

        for (page, title, sub) in PAGES {
            let selected = self.page == page;
            let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 46.0), egui::Sense::click());
            let bg = if selected {
                ACCENT.gamma_multiply(0.35)
            } else if resp.hovered() {
                ui.visuals().widgets.hovered.weak_bg_fill
            } else {
                Color32::TRANSPARENT
            };
            ui.painter().rect_filled(rect, 6.0, bg);
            let fg = ui.visuals().strong_text_color();
            ui.painter().text(rect.left_top() + Vec2::new(12.0, 6.0), egui::Align2::LEFT_TOP, title, egui::FontId::proportional(15.5), fg);
            ui.painter().text(rect.left_top() + Vec2::new(12.0, 26.0), egui::Align2::LEFT_TOP, sub, egui::FontId::proportional(12.0), ui.visuals().weak_text_color());
            if let Some(c) = self.nav_badge(page) {
                ui.painter().circle_filled(rect.right_center() - Vec2::new(14.0, 0.0), 5.0, c);
            }
            if resp.clicked() {
                self.page = page;
            }
            ui.add_space(2.0);
        }

        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
            ui.add_space(10.0);
            let (t, c) = match dcsbios::bios().connected() {
                Some(_) => {
                    let m = dcsbios::bios().module_name();
                    (if m.is_empty() { "DCS connected".to_string() } else { format!("DCS: {m}") }, GREEN)
                }
                None => ("DCS not running".to_string(), GREY),
            };
            ui.horizontal(|ui| {
                dot(ui, c);
                ui.label(t);
            });
        });
    }

    /// Small coloured dot next to a menu entry when that page needs attention or is active.
    fn nav_badge(&self, page: Page) -> Option<Color32> {
        match page {
            Page::Overview if self.devs.iter().any(|d| d.access.is_err()) || self.dcsbios_installed == Some(false) => Some(AMBER),
            Page::Panels if self.ufc.running() || self.pfp.running() => Some(GREEN),
            Page::Overlay if self.overlay_on => Some(GREEN),
            Page::Screens if self.pending.is_some() => Some(AMBER),
            Page::Settings if matches!(crate::update::state(), crate::update::State::Installed(_)) => Some(GREEN),
            _ => None,
        }
    }
}

// ------------------------------------------------------------------ Overview

impl App {
    fn ui_overview(&mut self, ui: &mut egui::Ui) {
        page_header(
            ui,
            "Overview",
            "UWACS talks to your WinWing hardware directly: it shows DCS cockpit data on the panel screens, turns panel buttons into keyboard shortcuts, and manages the instrument display. The checklist below shows what is ready and what still needs doing.",
        );

        card(ui, "Checklist", |ui| {
            let panels = self.panels();
            if panels.is_empty() {
                check_row(ui, GREY, "No WinWing panels connected", "Plug in your UFC or PFP over USB. This list refreshes every 2 seconds.");
            } else {
                let names: Vec<String> = panels.iter().map(|d| d.display_name()).collect();
                check_row(ui, GREEN, &format!("{} WinWing panel(s) connected", panels.len()), &names.join(", "));
            }
            if self.devs.iter().any(|d| d.access.is_err()) {
                check_row(ui, RED, "Linux does not let UWACS open the panels yet", "This is a one-time fix: copy the command below, paste it into Konsole, enter your password, then unplug and replug the panels.");
                ui.horizontal(|ui| {
                    ui.add_space(22.0);
                    ui.vertical(|ui| self.command_box(ui, "udev", &udev_command()));
                });
            } else if !self.devs.is_empty() {
                check_row(ui, GREEN, "Panel permissions OK", "");
            }
            match (self.usb_displays.len(), self.dl_active) {
                (0, _) => check_row(ui, GREY, "No WinWing instrument display connected", "Optional. The USB instrument display shows up here when plugged in."),
                (_, Some(false)) => {
                    check_row(ui, RED, "Instrument display found, but its driver is not running", "Run this in Konsole:");
                    ui.horizontal(|ui| {
                        ui.add_space(22.0);
                        ui.vertical(|ui| self.command_box(ui, "dl", "sudo systemctl enable --now displaylink"));
                    });
                }
                (n, _) => {
                    let outs: Vec<String> = (0..n).filter_map(|i| self.usb_output_name(i)).collect();
                    if outs.is_empty() {
                        check_row(ui, AMBER, "Instrument display found", "It does not show up as a screen yet. Give the driver a few seconds, or replug it.");
                    } else {
                        check_row(ui, GREEN, "Instrument display working", &format!("It is screen {} on your desktop. Arrange or rotate it on the Screens page.", outs.join(", ")));
                    }
                }
            }
            match self.dcsbios_installed {
                Some(true) => check_row(ui, GREEN, "DCS-BIOS is installed in DCS", ""),
                Some(false) => {
                    check_row(ui, RED, "DCS-BIOS is not installed in DCS", "The panel screens need DCS-BIOS, a free DCS add-on that streams cockpit data. The DCS setup page explains how to install it.");
                    ui.horizontal(|ui| {
                        ui.add_space(22.0);
                        if ui.button("Go to DCS setup").clicked() {
                            self.page = Page::Dcs;
                        }
                    });
                }
                None => check_row(ui, GREY, "DCS install not found", "UWACS could not find your DCS Saved Games folder, so it cannot check for DCS-BIOS."),
            }
            match dcsbios::bios().connected() {
                Some(_) => {
                    let m = dcsbios::bios().module_name();
                    check_row(ui, GREEN, "Receiving live data from DCS", &format!("Aircraft: {}", if m.is_empty() { "none yet" } else { &m }));
                }
                None => check_row(ui, GREY, "No live data from DCS right now", "This turns green while you sit in a cockpit in DCS with DCS-BIOS installed."),
            }
            match remap::UINPUT_ERROR.lock().unwrap().clone() {
                None => check_row(ui, GREEN, "Button mapping ready", "UWACS created its virtual keyboard, so mapped panel buttons can press keys."),
                Some(e) => check_row(ui, RED, "Button mapping cannot press keys", &e),
            }
        });

        self.ui_hardware(ui);
    }

    fn ui_hardware(&mut self, ui: &mut egui::Ui) {
        let states: Vec<(Vec<u32>, u64, Option<String>)> = inputs::with_hub(|h| {
            self.devs
                .iter()
                .map(|d| {
                    let x = h.get(&d.path);
                    let pressed = x.map(|x| x.layout.buttons.iter().zip(&x.state).filter(|(_, s)| **s).map(|(b, _)| b.usage).collect()).unwrap_or_default();
                    (pressed, x.map(|x| x.layout.buttons.len() as u64).unwrap_or(0), x.and_then(|x| x.error.clone()))
                })
                .collect()
        });
        card(ui, "Connected hardware", |ui| {
            if self.devs.is_empty() && self.usb_displays.is_empty() {
                hint(ui, "Nothing from WinWing is plugged in right now.");
            }
            if !self.devs.is_empty() {
                hint(ui, "Press a button on a panel to see it light up in the last column. One panel can appear more than once if it has several USB interfaces.");
                egui::Grid::new("devs").striped(true).num_columns(4).spacing([18.0, 6.0]).show(ui, |ui| {
                    ui.strong("Panel");
                    ui.strong("USB ID");
                    ui.strong("Status");
                    ui.strong("Buttons pressed now");
                    ui.end_row();
                    for (d, (pressed, nbuttons, err)) in self.devs.iter().zip(&states) {
                        ui.label(d.display_name()).on_hover_text(format!("{}\nserial {}\ninterface {}", d.path, d.serial, d.interface));
                        ui.label(format!("{:04X}:{:04X}", d.vid, d.pid));
                        match (&d.access, err) {
                            (Ok(()), None) => ui.colored_label(GREEN, format!("OK, {nbuttons} buttons")),
                            (Ok(()), Some(e)) => ui.colored_label(RED, e),
                            (Err(_), _) => ui.colored_label(RED, "no permission (see checklist)"),
                        };
                        let p: Vec<String> = pressed.iter().map(|u| u.to_string()).collect();
                        ui.label(if p.is_empty() { "none".into() } else { p.join(", ") });
                        ui.end_row();
                    }
                });
            }
            if !self.usb_displays.is_empty() {
                ui.add_space(8.0);
                for (i, d) in self.usb_displays.iter().enumerate() {
                    ui.horizontal(|ui| {
                        dot(ui, if self.usb_output_name(i).is_some() { GREEN } else { AMBER });
                        ui.label(format!("{}  (USB {} Mb/s)", d.product, d.speed_mbps));
                        if let Some(o) = self.usb_output_name(i) {
                            ui.label(RichText::new(format!("desktop screen {o}")).color(ui.visuals().weak_text_color()));
                        }
                    });
                }
            }
            ui.add_space(4.0);
            if ui.button("Check again now").clicked() {
                self.scan();
                self.reload_outputs();
            }
        });
    }
}

impl App {
    /// A command shown in full, with a Copy button that confirms it copied.
    fn command_box(&mut self, ui: &mut egui::Ui, id: &str, cmd: &str) {
        egui::Frame::new().fill(Color32::from_gray(18)).stroke(egui::Stroke::new(1.0, Color32::from_gray(60))).corner_radius(6.0).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
            ui.add(egui::Label::new(RichText::new(cmd).monospace().color(Color32::from_gray(220))).selectable(true).wrap());
        });
        ui.horizontal(|ui| {
            if ui.button("Copy").clicked() {
                ui.ctx().copy_text(cmd.to_string());
                self.copied = Some((id.to_string(), Instant::now()));
            }
            if matches!(&self.copied, Some((i, t)) if i == id && t.elapsed() < Duration::from_secs(3)) {
                ui.colored_label(GREEN, "Copied. Paste it with Ctrl+Shift+V in Konsole.");
            }
        });
    }
}

/// One line that works in fish, bash and zsh (fish has no heredocs).
fn udev_command() -> String {
    let lines: Vec<String> = devices::UDEV_RULE.lines().filter(|l| !l.trim().is_empty()).map(|l| format!("'{}'", l.replace('\'', ""))).collect();
    format!(
        "printf '%s\\n' {} | sudo tee /etc/udev/rules.d/70-winwing.rules >/dev/null && sudo udevadm control --reload && sudo udevadm trigger",
        lines.join(" ")
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn udev_command_is_one_line() {
        let c = super::udev_command();
        println!("CMD={c}");
        assert!(!c.contains('\n') && !c.contains("<<"));
        assert!(c.starts_with("printf '%s\\n' '# WinWing"));
    }
}

// ------------------------------------------------------------------ Panel screens

impl App {
    fn ui_panels(&mut self, ui: &mut egui::Ui) {
        page_header(
            ui,
            "Panel screens",
            "These replace your old ufc_writer and pfp_writer. While you fly, they read the cockpit displays from DCS (through DCS-BIOS) and draw them on the screens of your WinWing panels. They start by themselves when the panel is plugged in.",
        );
        how_to(
            ui,
            "howto-panels",
            &[
                "Install DCS-BIOS once (see the DCS setup page).",
                "Plug in the panel. Its writer starts automatically and the panel shows \"UWACS, waiting for DCS-BIOS\" (PFP) or the test pattern (UFC).",
                "Start DCS and jump into a supported aircraft. The panel switches to that aircraft's page within a second.",
            ],
        );

        let ufc_dev = self.first_ok(Role::Ufc);
        let pfp_dev = self.first_ok(Role::Pfp);
        let (mut start_ufc, mut stop_ufc, mut start_pfp, mut stop_pfp) = (false, false, false, false);

        card(ui, "UFC (F/A-18 up-front controller)", |ui| {
            ui.label("Shows the scratchpad, the five option windows and both comm channels.");
            hint(ui, "Aircraft: F/A-18C Hornet, AV-8B Harrier.");
            ui.add_space(4.0);
            writer_status(ui, self.has_role(Role::Ufc), ufc_dev.is_some(), "ufc", self.ufc.running());
            ui.horizontal(|ui| {
                if self.ufc.running() {
                    stop_ufc = ui.button("Stop").clicked();
                } else {
                    start_ufc = ui.add_enabled(ufc_dev.is_some(), egui::Button::new("Start")).clicked();
                }
                if ui.checkbox(&mut self.cfg.ufc_autostart, "Start automatically when the UFC is plugged in").changed() {
                    self.save_soon();
                }
            });
        });

        card(ui, "PFP (PFP 3N / PFP 7 screen and keyboard)", |ui| {
            ui.label("Shows a page per aircraft on the PFP screen:");
            for l in [
                "A-10C II: the CDU, and the PFP keys work as CDU keys.",
                "AH-64D: warnings, cautions, advisories and both crew keyboard lines; the PFP keys type on your KU.",
                "AV-8B: fuel, water, RPM, nozzles, flaps, gear, master arm and master mode.",
                "CH-47F: the CDU.",
            ] {
                ui.horizontal_wrapped(|ui| {
                    ui.add_space(10.0);
                    ui.label(format!("• {l}"));
                });
            }
            hint(ui, "MENU key: switches to RWR search. Type a threat (e.g. 10, SA11, GE) to see its range, altitude and what defeats it. CLR clears, DEL deletes, MENU goes back.");
            ui.add_space(4.0);
            writer_status(ui, self.has_role(Role::Pfp), pfp_dev.is_some(), "pfp", self.pfp.running());
            ui.horizontal(|ui| {
                if self.pfp.running() {
                    stop_pfp = ui.button("Stop").clicked();
                } else {
                    start_pfp = ui.add_enabled(pfp_dev.is_some(), egui::Button::new("Start")).clicked();
                }
                if ui.checkbox(&mut self.cfg.pfp_autostart, "Start automatically when the PFP is plugged in").changed() {
                    self.save_soon();
                }
            });
            if ui.checkbox(&mut self.cfg.pfp_auto_buttons, "PFP keys control the aircraft (A-10C CDU, AH-64D KU) and MENU opens RWR search").changed() {
                self.pfp_auto.store(self.cfg.pfp_auto_buttons, Ordering::Relaxed);
                self.save_soon();
            }
            hint(ui, "Turn this off if you prefer to bind the PFP keys yourself in DCS's control settings.");
        });

        if stop_ufc {
            self.ufc.stop_now();
            self.cfg.ufc_autostart = false;
            self.save_soon();
        }
        if start_ufc {
            if let Some(d) = ufc_dev {
                self.ufc.start(&d, ufc::run);
            }
        }
        if stop_pfp {
            self.pfp.stop_now();
            self.cfg.pfp_autostart = false;
            self.save_soon();
        }
        if start_pfp {
            if let Some(d) = pfp_dev {
                let auto = self.pfp_auto.clone();
                self.pfp.start(&d, move |p, s| pfp::run(p, s, auto));
            }
        }
        self.ui_custom_screens(ui);
    }
}

fn writer_status(ui: &mut egui::Ui, present: bool, accessible: bool, name: &str, running: bool) {
    ui.horizontal(|ui| {
        let (c, t) = if accessible {
            (GREEN, "Panel connected")
        } else if present {
            (RED, "Panel connected, but no permission (see Overview)")
        } else {
            (GREY, "Panel not connected")
        };
        dot(ui, c);
        ui.label(t);
        ui.add_space(12.0);
        let (c, t) = match get_status(name) {
            Status::Running if running => (GREEN, "Running".to_string()),
            Status::Waiting(w) if running => (AMBER, format!("Starting ({w})")),
            Status::Error(e) => (RED, format!("Error: {e}")),
            _ => (GREY, "Stopped".to_string()),
        };
        dot(ui, c);
        ui.label(t);
        ui.add_space(12.0);
        match dcsbios::bios().connected() {
            Some(_) => {
                dot(ui, GREEN);
                ui.label("DCS data arriving");
            }
            None => {
                dot(ui, GREY);
                ui.label("Waiting for DCS");
            }
        }
    });
}

// ------------------------------------------------------------------ Button mapping

impl App {
    fn ui_remap(&mut self, ui: &mut egui::Ui) {
        page_header(
            ui,
            "Button mapping",
            "Make any button on any WinWing panel press a keyboard shortcut. Every connected panel is drawn below; click a button on the drawing to give it a shortcut. Buttons light up yellow while you press them, only on the panel you are pressing. The key stays held for as long as you hold the button, so push-to-talk works too.",
        );
        how_to(
            ui,
            "howto-remap",
            &[
                "Click a button on the drawing of your panel (or just press it on the real panel, it gets selected for you).",
                "Press the keyboard shortcut it should send (for example Ctrl+Shift+F13). Esc cancels.",
                "Done, it is saved automatically. Green = has a shortcut. If DCS also has that joystick button bound, remove that binding in DCS.",
                "Dashed buttons: UWACS does not know their number yet. Click one, then press it on the real panel once; UWACS remembers it.",
            ],
        );

        if let Some(e) = remap::UINPUT_ERROR.lock().unwrap().clone() {
            ui.colored_label(RED, e);
        }
        let mut en = remap::ENABLED.load(Ordering::Relaxed);
        if ui.checkbox(&mut en, "Mappings are active").on_hover_text("Untick to pause all mappings without deleting them.").changed() {
            remap::ENABLED.store(en, Ordering::Relaxed);
        }
        ui.add_space(6.0);

        self.ui_device_pictures(ui);
        self.ui_mapping_table(ui);
        self.ui_all_buttons(ui);
        self.ui_device_gallery(ui);
    }

    /// Which profile switch is in force: of the held ones, the one pressed last (same rule as the remapper).
    fn live_profile(&mut self, key: &str, switches: &[(u32, String, String)], pressed: &std::collections::HashSet<u32>) -> Option<u32> {
        if let Some(lp) = remap::LAST_PRESS.lock().unwrap().as_ref() {
            if lp.dev_key == key && switches.iter().any(|s| s.0 == lp.usage) {
                self.profile_last.insert(key.to_string(), lp.usage);
            }
        }
        let last = self.profile_last.get(key).copied();
        let held: Vec<u32> = switches.iter().map(|s| s.0).filter(|u| pressed.contains(u)).collect();
        if let Some(l) = last.filter(|l| held.contains(l)) {
            return Some(l);
        }
        held.first().copied()
    }

    /// Buttons on this device type that select a profile: (button number, label, control id).
    fn profile_switches(&self, t: &Template) -> Vec<(u32, String, String)> {
        let ov = self.cfg.button_overrides.get(&t.id);
        let extra = self.cfg.profile_switches.get(&t.id);
        t.controls
            .iter()
            .filter(|c| c.profile_switch || extra.map(|e| e.contains(&c.id)).unwrap_or(false))
            .filter_map(|c| Some((t.button_of(c, ov)?, c.label.clone(), c.id.clone())))
            .collect()
    }

    /// One picture per connected panel (three MFDs = three pictures).
    fn ui_device_pictures(&mut self, ui: &mut egui::Ui) {
        struct Inst {
            path: String,
            key: String,
            name: String,
            pid: u16,
            usages: Vec<u32>,
            pressed: std::collections::HashSet<u32>,
            axes: Vec<f32>,
            alive: bool,
        }
        let mut insts: Vec<Inst> = inputs::with_hub(|h| {
            h.values()
                .filter(|x| !x.layout.buttons.is_empty())
                .map(|x| Inst {
                    path: x.dev.path.clone(),
                    key: x.dev.key(),
                    name: devicemaps::model_name(x.dev.pid).map(|s| s.to_string()).unwrap_or_else(|| x.dev.display_name()),
                    pid: x.dev.pid,
                    usages: x.layout.buttons.iter().map(|b| b.usage).collect(),
                    pressed: x.layout.buttons.iter().zip(&x.state).filter(|(_, s)| **s).map(|(b, _)| b.usage).collect(),
                    axes: x
                        .layout
                        .axes
                        .iter()
                        .zip(&x.axes)
                        .map(|(a, v)| if a.max > a.min { ((*v - a.min) as f32 / (a.max - a.min) as f32).clamp(0.0, 1.0) } else { 0.0 })
                        .collect(),
                    alive: x.running,
                })
                .collect()
        });
        insts.sort_by(|a, b| a.name.cmp(&b.name).then(a.path.cmp(&b.path)));
        if insts.is_empty() {
            card(ui, "Your panels", |ui| {
                hint(ui, "No panel connected. Plug one in and it appears here as a drawing within 2 seconds. Further down you can look at every WinWing device UWACS knows.");
            });
            return;
        }
        let mut counts: std::collections::HashMap<String, usize> = Default::default();
        for i in &insts {
            *counts.entry(i.name.clone()).or_default() += 1;
        }
        let mut nth: std::collections::HashMap<String, usize> = Default::default();
        for inst in &insts {
            let t = devicemaps::template_for(inst.pid, &inst.usages);
            let n = nth.entry(inst.name.clone()).or_default();
            *n += 1;
            let title = if counts[&inst.name] > 1 { format!("{}  #{}", inst.name, n) } else { inst.name.clone() };
            let title = if inst.alive { title } else { format!("{title}  (not reading)") };
            let ov = self.cfg.button_overrides.get(&t.id).cloned();

            // profiles: follow the switch when it moves, but let the user click another tab
            let switches = self.profile_switches(&t);
            remap::PROFILE_SWITCHES.lock().unwrap().insert(inst.key.clone(), switches.iter().map(|s| s.0).collect());
            let live = self.live_profile(&inst.key, &switches, &inst.pressed);
            if self.profile_seen.get(&inst.path) != Some(&live) {
                self.profile_seen.insert(inst.path.clone(), live);
                self.profile_tab.insert(inst.path.clone(), live);
            }
            let tab = if switches.is_empty() { None } else { self.profile_tab.get(&inst.path).copied().flatten() };

            let mapped: std::collections::HashMap<u32, String> =
                self.cfg.remaps.iter().filter(|r| r.device_key == inst.key && r.profile == tab).map(|r| (r.usage, r.action.describe())).collect();
            let switch_ids: std::collections::HashSet<String> = switches.iter().map(|s| s.2.clone()).collect();
            let sel = self.selected.as_ref().filter(|(p, _)| *p == inst.path).map(|(_, c)| c.clone());
            let teaching = self.teach.as_ref().filter(|t| t.path == inst.path).map(|t| t.ctrl.clone());
            let wizard = self.teach_all.as_ref().filter(|(p, _)| *p == inst.path).map(|(_, i)| *i);
            let mut clicked = None;
            card(ui, &title, |ui| {
                // teach-all walkthrough
                let seq = teach_sequence(&t);
                ui.horizontal(|ui| match wizard {
                    Some(i) => {
                        let label = seq.get(i).and_then(|id| t.controls.iter().find(|c| &c.id == id)).map(|c| c.label.clone()).unwrap_or_default();
                        ui.label(RichText::new(format!("Press {label} on the panel  ({} of {})", i + 1, seq.len())).strong().color(AMBER));
                        if ui.button("Skip").clicked() {
                            self.wizard_step(&t, &inst.path, i + 1);
                        }
                        if ui.button("Stop").clicked() {
                            self.teach_all = None;
                            self.teach = None;
                        }
                    }
                    None => {
                        if ui.button("Teach all buttons in order").on_hover_text("Walks through every button on the drawing: press each one on the real panel when it is highlighted. Use this when the numbers on the drawing are wrong.").clicked() {
                            self.record_for = None;
                            self.record_new = None;
                            self.wizard_step(&t, &inst.path, 0);
                        }
                    }
                });
                if !switches.is_empty() {
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Profile:");
                        let mut cur = tab;
                        if cur.is_none() && ui.selectable_label(true, "No switch held").on_hover_text("Mappings made here only work while none of the profile switches is held").clicked() {
                            cur = None;
                        }
                        for (u, label, _) in &switches {
                            let txt = if live == Some(*u) { format!("{label}  (active now)") } else { label.clone() };
                            if ui.selectable_label(cur == Some(*u), txt).on_hover_text(format!("Mappings made here only work while button {u} is held (switch in that position)")).clicked() {
                                cur = Some(*u);
                            }
                        }
                        if cur != tab {
                            self.profile_tab.insert(inst.path.clone(), cur);
                        }
                    });
                }
                ui.add_space(4.0);
                let wide = ui.available_width() > 820.0;
                let pic_w = if wide { (ui.available_width() - 330.0).min(600.0) } else { ui.available_width().min(600.0) };
                let st = Live { pressed: &inst.pressed, axes: &inst.axes, mapped: &mapped, selected: sel.as_deref(), teaching: teaching.as_deref(), switches: &switch_ids };
                if wide {
                    ui.horizontal_top(|ui| {
                        clicked = draw_device(ui, &t, ov.as_ref(), &st, pic_w);
                        ui.add_space(12.0);
                        ui.vertical(|ui| {
                            ui.set_width(300.0);
                            self.ui_control_editor(ui, &t, ov.as_ref(), &inst.path, &inst.key, &inst.name, &inst.pressed, tab, &switches);
                        });
                    });
                } else {
                    clicked = draw_device(ui, &t, ov.as_ref(), &st, pic_w);
                    ui.add_space(8.0);
                    self.ui_control_editor(ui, &t, ov.as_ref(), &inst.path, &inst.key, &inst.name, &inst.pressed, tab, &switches);
                }
            });
            if let Some(id) = clicked {
                if self.teach_all.is_none() {
                    self.click_control(&t, ov.as_ref(), &inst.path, &inst.key, &inst.name, &id, tab, &switches);
                }
            }
        }
    }

    /// Go to step `i` of the teach-all walkthrough (or finish it).
    fn wizard_step(&mut self, t: &Template, path: &str, i: usize) {
        let seq = teach_sequence(t);
        match seq.get(i) {
            Some(id) => {
                self.teach_all = Some((path.to_string(), i));
                self.teach = Some(Teach { path: path.into(), template: t.id.clone(), ctrl: id.clone(), since: Instant::now() });
                self.selected = Some((path.to_string(), id.clone()));
            }
            None => {
                self.teach_all = None;
                self.teach = None;
                log::log(format!("Taught all buttons of {}", t.name));
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn click_control(
        &mut self,
        t: &Template,
        ov: Option<&std::collections::BTreeMap<String, u32>>,
        path: &str,
        key: &str,
        dev_name: &str,
        id: &str,
        tab: Option<u32>,
        switches: &[(u32, String, String)],
    ) {
        let Some(c) = t.controls.iter().find(|c| c.id == id) else { return };
        self.selected = Some((path.to_string(), id.to_string()));
        self.record_for = None;
        self.record_new = None;
        self.teach = None;
        match t.button_of(c, ov) {
            None => self.teach = Some(Teach { path: path.into(), template: t.id.clone(), ctrl: id.into(), since: Instant::now() }),
            Some(b) => match self.cfg.remaps.iter().position(|r| r.device_key == key && r.usage == b && r.profile == tab) {
                Some(i) => self.record_for = Some(i),
                None => self.record_new = Some(new_remap(key, dev_name, b, &c.label, tab, switches, Action::Keys(String::new()))),
            },
        }
    }

    /// Right-hand side of a panel picture: what the selected button does.
    #[allow(clippy::too_many_arguments)]
    fn ui_control_editor(
        &mut self,
        ui: &mut egui::Ui,
        t: &Template,
        ov: Option<&std::collections::BTreeMap<String, u32>>,
        path: &str,
        key: &str,
        dev_name: &str,
        pressed: &std::collections::HashSet<u32>,
        tab: Option<u32>,
        switches: &[(u32, String, String)],
    ) {
        // buttons pressed right now that are not on the drawing
        let known: std::collections::HashSet<u32> = t.controls.iter().filter_map(|c| t.button_of(c, ov)).collect();
        let mut extra: Vec<u32> = pressed.iter().copied().filter(|b| !known.contains(b)).collect();
        if !t.generic && !extra.is_empty() {
            extra.sort();
            ui.label(RichText::new(format!("Down but not on the drawing: {}", extra.iter().map(|b| format!("button {b}")).collect::<Vec<_>>().join(", "))).small().color(AMBER))
                .on_hover_text("Switches that rest in a position read as held all the time. To put such a button on the drawing, click a button on the drawing, choose \"Teach it\" and press it.");
            ui.add_space(4.0);
        }
        let sel = self.selected.as_ref().filter(|(p, _)| p == path).map(|(_, c)| c.clone());
        let Some(c) = sel.and_then(|id| t.controls.iter().find(|c| c.id == id).cloned()) else {
            hint(ui, "Click a button on the drawing, or press one on the panel.");
            return;
        };
        ui.label(RichText::new(&c.label).size(18.0).strong());
        let num = t.button_of(&c, ov);
        let taught = ov.and_then(|o| o.get(&c.id)).map(|v| *v != 0).unwrap_or(false);
        let teaching = self.teach.as_ref().map(|x| x.path == path && x.ctrl == c.id).unwrap_or(false);
        let is_wheel = num.map(|n| n >= crate::hidreport::AXIS_PSEUDO_BASE).unwrap_or(false);
        match (num, c.source) {
            _ if teaching => {
                ui.colored_label(AMBER, "Now press this button on the real panel…");
            }
            _ if is_wheel => {
                ui.label("Turning the wheel one step taps the shortcut once.");
            }
            (Some(n), _) if taught => {
                ui.label(format!("Joystick button {n} (you taught this)"));
            }
            (Some(n), Source::Known) => {
                ui.label(format!("Joystick button {n}"));
            }
            (Some(n), _) => {
                ui.label(format!("Joystick button {n} (a guess: if pressing it lights up a different button, teach it)"));
            }
            (None, _) => {
                ui.colored_label(AMBER, "Number not known yet");
            }
        }
        if !is_wheel {
            ui.horizontal(|ui| {
                let label = if teaching { "Cancel" } else if num.is_some() { "Wrong button? Teach it" } else { "Teach it" };
                if ui.button(label).on_hover_text("Click, then press this button on the real panel once.").clicked() {
                    self.record_for = None;
                    self.record_new = None;
                    self.teach_all = None;
                    self.teach = if teaching { None } else { Some(Teach { path: path.into(), template: t.id.clone(), ctrl: c.id.clone(), since: Instant::now() }) };
                }
                if taught && ui.button("Forget").on_hover_text("Go back to UWACS' built-in number").clicked() {
                    if let Some(m) = self.cfg.button_overrides.get_mut(&t.id) {
                        m.remove(&c.id);
                    }
                    self.save_soon();
                }
            });
            if !c.profile_switch {
                let mut is_sw = self.cfg.profile_switches.get(&t.id).map(|s| s.contains(&c.id)).unwrap_or(false);
                if ui.checkbox(&mut is_sw, "Use as a profile switch").on_hover_text("While this button is held, the mappings of its profile are used. Good for switches that stay in a position.").changed() {
                    let set = self.cfg.profile_switches.entry(t.id.clone()).or_default();
                    if is_sw {
                        set.insert(c.id.clone());
                    } else {
                        set.remove(&c.id);
                    }
                    self.save_soon();
                }
            }
        }
        if c.profile_switch || switches.iter().any(|s| s.2 == c.id) {
            hint(ui, "This is a profile switch: while it is held, the mappings of its profile tab are used. It can still have a shortcut of its own.");
        }
        ui.add_space(8.0);
        let Some(n) = num else { return };
        let idx = self.cfg.remaps.iter().position(|r| r.device_key == key && r.usage == n && r.profile == tab);
        let tab_name = tab.and_then(|u| switches.iter().find(|s| s.0 == u)).map(|s| s.1.clone());
        ui.label(RichText::new(match &tab_name {
            Some(p) => format!("Does, in profile {p}"),
            None => "Does".into(),
        }).strong());
        match idx {
            Some(i) => {
                let (changed, rec, del) = remap_editor(ui, &mut self.cfg.remaps[i], ("pic", i), self.record_for == Some(i));
                if rec {
                    self.record_new = None;
                    self.record_for = if self.record_for == Some(i) { None } else { Some(i) };
                }
                if del {
                    self.cfg.remaps.remove(i);
                    self.record_for = None;
                }
                if changed || del {
                    self.sync_remaps();
                }
            }
            None => {
                let waiting = self.record_new.as_ref().map(|r| r.device_key == key && r.usage == n && r.profile == tab).unwrap_or(false);
                if waiting {
                    ui.add(egui::Button::new(RichText::new("Press the shortcut now… (Esc cancels)").color(Color32::BLACK)).fill(AMBER));
                } else {
                    hint(ui, if tab_name.is_some() { "Nothing in this profile yet." } else { "Nothing yet." });
                    ui.horizontal(|ui| {
                        if ui.button("Record a shortcut").clicked() {
                            self.record_for = None;
                            self.record_new = Some(new_remap(key, dev_name, n, &c.label, tab, switches, Action::Keys(String::new())));
                        }
                        if ui.button("Show/hide MFD overlays").clicked() {
                            self.cfg.remaps.push(new_remap(key, dev_name, n, &c.label, tab, switches, Action::OverlayToggle));
                            self.sync_remaps();
                        }
                    });
                }
            }
        }
    }

    /// Teach mode and "press a real button to select it on the drawing".
    fn handle_teach_and_presses(&mut self) {
        let lp = remap::LAST_PRESS.lock().unwrap().clone();
        let Some(lp) = lp else { return };
        if self.last_press_seen == Some(lp.at) {
            return;
        }
        self.last_press_seen = Some(lp.at);
        let (pid, usages) = inputs::with_hub(|h| h.get(&lp.dev_path).map(|x| (x.dev.pid, x.layout.buttons.iter().map(|b| b.usage).collect::<Vec<_>>()))).unwrap_or_default();
        let tpl = devicemaps::template_for(pid, &usages);
        if let Some(t) = &self.teach {
            if lp.dev_path == t.path && lp.at > t.since && lp.usage < crate::hidreport::AXIS_PSEUDO_BASE {
                let (tid, ctrl) = (t.template.clone(), t.ctrl.clone());
                let m = self.cfg.button_overrides.entry(tid.clone()).or_default();
                m.retain(|_, v| *v != lp.usage);
                // the built-in number of another control is now taken: point that one nowhere
                for c in tpl.controls.iter().filter(|c| c.id != ctrl && c.button == Some(lp.usage)) {
                    m.entry(c.id.clone()).or_insert(0);
                }
                m.insert(ctrl.clone(), lp.usage);
                log::log(format!("Taught: {tid} {ctrl} = button {}", lp.usage));
                self.teach = None;
                self.selected = Some((lp.dev_path.clone(), ctrl.clone()));
                self.save_soon();
                if let Some((p, i)) = self.teach_all.clone() {
                    if p == lp.dev_path {
                        self.wizard_step(&tpl, &p, i + 1);
                    }
                }
            }
            return;
        }
        if self.page != Page::Remap || self.record_for.is_some() || self.record_new.is_some() {
            return;
        }
        // select the pressed button on its drawing
        let ov = self.cfg.button_overrides.get(&tpl.id);
        if let Some(c) = tpl.controls.iter().find(|c| tpl.button_of(c, ov) == Some(lp.usage)) {
            self.selected = Some((lp.dev_path.clone(), c.id.clone()));
        }
    }

    /// Every device type UWACS has a drawing for, connected or not.
    fn ui_device_gallery(&mut self, ui: &mut egui::Ui) {
        card(ui, "Every WinWing device UWACS knows", |ui| {
            egui::CollapsingHeader::new("Show the drawings").id_salt("gallery").default_open(std::env::var("UWACS_GALLERY_OPEN").is_ok()).show(ui, |ui| {
                hint(ui, "When you plug one of these in, it appears above with live buttons. Solid = button number known, dashed = taught on first use.");
                let all = devicemaps::gallery();
                let per_row = ((ui.available_width() + 16.0) / 316.0).floor().max(1.0) as usize;
                for row in all.chunks(per_row) {
                    ui.horizontal_top(|ui| {
                        for t in row {
                            ui.allocate_ui(Vec2::new(300.0, 0.0), |ui| {
                                ui.vertical(|ui| {
                                    ui.set_width(300.0);
                                    ui.label(RichText::new(&t.name).strong());
                                    let ov = self.cfg.button_overrides.get(&t.id).cloned();
                                    let none = Default::default();
                                    let nomap = Default::default();
                                    let st = Live { pressed: &none, axes: &[0.5], mapped: &nomap, selected: None, teaching: None, switches: &Default::default() };
                                    draw_device(ui, t, ov.as_ref(), &st, 300.0);
                                });
                            });
                            ui.add_space(16.0);
                        }
                    });
                    ui.add_space(12.0);
                }
            });
        });
    }

    fn ui_mapping_table(&mut self, ui: &mut egui::Ui) {
        let held: Vec<bool> = inputs::with_hub(|h| {
            self.cfg
                .remaps
                .iter()
                .map(|r| {
                    h.values()
                        .find(|x| x.dev.key() == r.device_key)
                        .map(|x| x.layout.buttons.iter().zip(&x.state).any(|(b, s)| b.usage == r.usage && *s))
                        .unwrap_or(false)
                })
                .collect()
        });
        let mut delete = None;
        let mut changed = false;
        let record_for = self.record_for;
        let mut new_record_for = record_for;
        let n = self.cfg.remaps.len();
        card(ui, &format!("Your mappings ({n})"), |ui| {
            if self.cfg.remaps.is_empty() {
                hint(ui, "None yet. Press a panel button and click \"Map this button\".");
                return;
            }
            egui::Grid::new("remaps").striped(true).num_columns(3).spacing([14.0, 8.0]).show(ui, |ui| {
                ui.strong("");
                ui.strong("Panel button");
                ui.strong("Does");
                ui.end_row();
                for (i, r) in self.cfg.remaps.iter_mut().enumerate() {
                    dot(ui, if held.get(i).copied().unwrap_or(false) { AMBER } else { Color32::from_gray(70) });
                    ui.vertical(|ui| {
                        ui.label(&r.label);
                        ui.label(RichText::new(&r.device_name).small().color(ui.visuals().weak_text_color()));
                    });
                    let (ch, rec, del) = remap_editor(ui, r, ("tbl", i), record_for == Some(i));
                    changed |= ch;
                    if rec {
                        new_record_for = if record_for == Some(i) { None } else { Some(i) };
                    }
                    if del {
                        delete = Some(i);
                    }
                    ui.end_row();
                }
            });
            ui.add_space(4.0);
            hint(ui, "Key names: Ctrl, Shift, Alt, AltGr, Super, A–Z, 0–9, F1–F24, Num0–Num9, NumPlus, NumMinus, NumEnter, Up, Down, Left, Right, Home, End, PageUp, PageDown, Insert, Delete, Space, Enter, Esc, Tab. Tip: F13–F24 exist on no keyboard, so they never clash with anything; bind them in DCS.");
        });
        self.record_for = new_record_for;
        if let Some(i) = delete {
            self.cfg.remaps.remove(i);
            self.record_for = None;
            changed = true;
        }
        if changed {
            self.sync_remaps();
        }
    }

    fn ui_all_buttons(&mut self, ui: &mut egui::Ui) {
        let dev_buttons: Vec<(String, String, Vec<(u32, u8, u32, bool)>)> = inputs::with_hub(|h| {
            let mut v: Vec<_> = h
                .values()
                .filter(|x| !x.layout.buttons.is_empty())
                .map(|x| (x.dev.key(), x.dev.display_name(), x.layout.buttons.iter().zip(&x.state).map(|(b, s)| (b.usage, b.report_id, b.bit, *s)).collect()))
                .collect();
            v.sort_by(|a, b| a.1.cmp(&b.1));
            v
        });
        if dev_buttons.is_empty() {
            return;
        }
        card(ui, "All buttons, panel by panel", |ui| {
            hint(ui, "Another way to map: click a number. Yellow = pressed right now, green = already mapped.");
            for (key, name, buttons) in &dev_buttons {
                egui::CollapsingHeader::new(format!("{name}  ({} buttons)", buttons.len())).id_salt(key).show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        for (usage, rid, bit, on) in buttons {
                            let mapped = self.cfg.remaps.iter().position(|r| &r.device_key == key && r.usage == *usage);
                            let mut b = egui::Button::new(RichText::new(format!("{usage}")).monospace()).min_size(Vec2::new(38.0, 26.0));
                            if *on {
                                b = b.fill(AMBER);
                            } else if mapped.is_some() {
                                b = b.fill(Color32::from_rgb(40, 100, 60));
                            }
                            let label = self.button_label(key, *usage, *rid, *bit);
                            let hover = match mapped {
                                Some(i) => format!("{label}\nmapped to: {}", self.cfg.remaps[i].action.describe()),
                                None => format!("{label}\nclick to map"),
                            };
                            if ui.add(b).on_hover_text(hover).clicked() {
                                match mapped {
                                    Some(i) => self.record_for = Some(i),
                                    None => {
                                        self.cfg.remaps.push(Remap { device_key: key.clone(), device_name: name.clone(), usage: *usage, label, action: Action::Keys(String::new()), profile: None });
                                        self.record_for = Some(self.cfg.remaps.len() - 1);
                                        self.sync_remaps();
                                    }
                                }
                            }
                        }
                    });
                });
            }
        });
    }

    fn capture_shortcut(&mut self, ctx: &egui::Context) {
        if self.page != Page::Remap || (self.record_for.is_none() && self.record_new.is_none()) {
            return;
        }
        let ev = ctx.input(|inp| {
            inp.events.iter().find_map(|e| match e {
                egui::Event::Key { key, pressed: true, modifiers, .. } => Some((*key, *modifiers)),
                _ => None,
            })
        });
        let Some((key, m)) = ev else { return };
        if key == egui::Key::Escape && m.is_none() {
            self.record_for = None;
            self.record_new = None;
            return;
        }
        let Some(s) = remap::keys::from_egui(key, m) else { return };
        if let Some(mut r) = self.record_new.take() {
            r.action = Action::Keys(s);
            self.cfg.remaps.push(r);
        } else if let Some(r) = self.record_for.and_then(|i| self.cfg.remaps.get_mut(i)) {
            r.action = Action::Keys(s);
        }
        self.record_for = None;
        self.sync_remaps();
    }
}

/// Action picker + shortcut field + Record + Remove for one mapping. Returns (changed, record clicked, remove clicked).
fn remap_editor(ui: &mut egui::Ui, r: &mut Remap, id: (&str, usize), recording: bool) -> (bool, bool, bool) {
    let (mut changed, mut rec, mut del) = (false, false, false);
    ui.horizontal_wrapped(|ui| {
        let kind = match r.action {
            Action::Keys(_) => 0,
            Action::OverlayToggle => 1,
            Action::OverlayNextMonitor => 2,
        };
        let names = ["Keyboard shortcut", "Show/hide MFD overlays", "First overlay to next screen"];
        let mut k = kind;
        egui::ComboBox::from_id_salt(("kind", id)).selected_text(names[k]).width(190.0).show_ui(ui, |ui| {
            for (n, name) in names.iter().enumerate() {
                ui.selectable_value(&mut k, n, *name);
            }
        });
        if k != kind {
            r.action = match k {
                0 => Action::Keys(String::new()),
                1 => Action::OverlayToggle,
                _ => Action::OverlayNextMonitor,
            };
            changed = true;
        }
        if let Action::Keys(s) = &mut r.action {
            let ok = s.is_empty() || remap::keys::parse_combo(s).is_ok();
            let color = if ok { ui.visuals().text_color() } else { RED };
            if ui.add(egui::TextEdit::singleline(s).hint_text("e.g. Ctrl+Shift+F13").desired_width(150.0).text_color(color)).changed() {
                changed = true;
            }
            let btn = if recording { egui::Button::new(RichText::new("Press keys now…").color(Color32::BLACK)).fill(AMBER) } else { egui::Button::new("Record") };
            if ui.add(btn).on_hover_text("Click, then press the shortcut on your keyboard. Esc cancels.").clicked() {
                rec = true;
            }
            if !ok {
                ui.colored_label(RED, "unknown key name");
            }
        }
        if ui.button("Remove").clicked() {
            del = true;
        }
    });
    (changed, rec, del)
}

/// Order of the teach-all walkthrough: every control except the wheel.
fn teach_sequence(t: &Template) -> Vec<String> {
    t.controls.iter().filter(|c| c.button.map(|b| b < crate::hidreport::AXIS_PSEUDO_BASE).unwrap_or(true)).map(|c| c.id.clone()).collect()
}

fn new_remap(key: &str, dev_name: &str, usage: u32, label: &str, profile: Option<u32>, switches: &[(u32, String, String)], action: Action) -> Remap {
    let pname = profile.and_then(|u| switches.iter().find(|s| s.0 == u)).map(|s| format!(", profile {}", s.1)).unwrap_or_default();
    let num = if usage >= crate::hidreport::AXIS_PSEUDO_BASE { String::new() } else { format!(" (button {usage}{pname})") };
    Remap { device_key: key.into(), device_name: dev_name.into(), usage, label: format!("{label}{num}"), action, profile }
}

/// What a drawing shows live.
struct Live<'a> {
    pressed: &'a std::collections::HashSet<u32>,
    /// axis values 0..1
    axes: &'a [f32],
    mapped: &'a std::collections::HashMap<u32, String>,
    selected: Option<&'a str>,
    teaching: Option<&'a str>,
    switches: &'a std::collections::HashSet<String>,
}

/// Draws a device template scaled to `max_w`. Returns the id of a clicked control.
fn draw_device(ui: &mut egui::Ui, t: &Template, ov: Option<&std::collections::BTreeMap<String, u32>>, st: &Live, max_w: f32) -> Option<String> {
    let (pressed, mapped, selected, teaching) = (st.pressed, st.mapped, st.selected, st.teaching);
    let k = (max_w / t.size[0]).min(1.4);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(t.size[0] * k, t.size[1] * k), egui::Sense::hover());
    let r = |a: [f32; 4]| egui::Rect::from_min_size(rect.min + Vec2::new(a[0] * k, a[1] * k), Vec2::new(a[2] * k, a[3] * k));
    let painter = ui.painter_at(rect.expand(2.0));
    let fit = |text: &str, box_w: f32, max: f32| -> egui::FontId {
        let mut size = max;
        while size > 6.0 {
            let g = painter.layout_no_wrap(text.to_string(), egui::FontId::proportional(size), Color32::WHITE);
            if g.size().x <= box_w - 4.0 {
                break;
            }
            size -= 0.5;
        }
        egui::FontId::proportional(size)
    };
    for d in &t.decos {
        let rr = r(d.rect);
        match &d.kind {
            devicemaps::DecoKind::Body => {
                painter.rect(rr, 14.0 * k, Color32::from_gray(34), egui::Stroke::new(2.0, Color32::from_gray(75)), egui::StrokeKind::Inside);
            }
            devicemaps::DecoKind::Screen(label) => {
                painter.rect(rr, 4.0 * k, Color32::from_rgb(14, 24, 18), egui::Stroke::new(1.5, Color32::from_gray(60)), egui::StrokeKind::Inside);
                if !label.is_empty() {
                    painter.text(rr.left_top() + Vec2::new(8.0, 6.0) * k, egui::Align2::LEFT_TOP, *label, egui::FontId::proportional((11.0 * k).max(7.0)), Color32::from_rgb(90, 150, 100));
                }
            }
            devicemaps::DecoKind::Knob(label) => {
                painter.circle(rr.center(), rr.width() / 2.0, Color32::from_gray(55), egui::Stroke::new(1.5, Color32::from_gray(110)));
                painter.text(rr.center_bottom() + Vec2::new(0.0, 3.0 * k), egui::Align2::CENTER_TOP, *label, egui::FontId::proportional((10.0 * k).max(7.0)), Color32::from_gray(170));
            }
            devicemaps::DecoKind::Text(label) => {
                painter.text(rr.min, egui::Align2::LEFT_TOP, *label, egui::FontId::proportional((10.5 * k).max(7.0)), Color32::from_gray(170));
            }
            devicemaps::DecoKind::Axis(i) => {
                painter.rect(rr, 4.0 * k, Color32::from_gray(22), egui::Stroke::new(1.0, Color32::from_gray(90)), egui::StrokeKind::Inside);
                if let Some(v) = st.axes.get(*i) {
                    let fill_h = rr.height() * v;
                    let bar = egui::Rect::from_min_max(egui::pos2(rr.min.x + 2.0, rr.max.y - fill_h), egui::pos2(rr.max.x - 2.0, rr.max.y - 1.0));
                    painter.rect_filled(bar, 2.0, ACCENT);
                    painter.text(rr.center(), egui::Align2::CENTER_CENTER, format!("{:.0}", v * 100.0), egui::FontId::proportional((9.0 * k).max(7.0)), Color32::from_gray(190));
                }
            }
        }
    }
    let pulse = (ui.input(|i| i.time) * 4.0).sin() as f32 * 0.5 + 0.5;
    let mut clicked = None;
    for c in &t.controls {
        let rr = r(c.rect);
        let resp = ui.interact(rr, ui.scope_id().with((&t.id, &c.id, rect.min.x as i32, rect.min.y as i32)), egui::Sense::click());
        let num = t.button_of(c, ov);
        let taught = ov.and_then(|o| o.get(&c.id)).map(|v| *v != 0).unwrap_or(false);
        let is_down = num.map(|n| pressed.contains(&n)).unwrap_or(false);
        let mapping = num.and_then(|n| mapped.get(&n));
        let (fill, text) = if is_down {
            (AMBER, Color32::BLACK)
        } else if mapping.is_some() {
            (Color32::from_rgb(40, 110, 64), Color32::WHITE)
        } else if num.is_none() {
            (Color32::from_gray(44), Color32::from_gray(150))
        } else {
            (Color32::from_gray(70), Color32::from_gray(235))
        };
        let fill = if resp.hovered() && !is_down { fill.gamma_multiply(1.35) } else { fill };
        let radius = if c.round { rr.height() / 2.0 } else { 4.0 * k };
        painter.rect_filled(rr, radius, fill);
        if num.is_none() {
            let pts = [rr.left_top(), rr.right_top(), rr.right_bottom(), rr.left_bottom(), rr.left_top()];
            for w in pts.windows(2) {
                painter.add(egui::Shape::dashed_line(&[w[0], w[1]], egui::Stroke::new(1.0, Color32::from_gray(130)), 4.0, 3.0));
            }
        }
        if st.switches.contains(&c.id) {
            let bar = egui::Rect::from_min_max(egui::pos2(rr.min.x + 3.0, rr.max.y - 3.0), egui::pos2(rr.max.x - 3.0, rr.max.y - 1.0));
            painter.rect_filled(bar, 1.0, ACCENT);
        }
        if teaching == Some(c.id.as_str()) {
            painter.rect_stroke(rr.expand(2.0), radius, egui::Stroke::new(3.0, AMBER.gamma_multiply(0.4 + 0.6 * pulse)), egui::StrokeKind::Outside);
        } else if selected == Some(c.id.as_str()) {
            painter.rect_stroke(rr.expand(2.0), radius, egui::Stroke::new(2.5, ACCENT), egui::StrokeKind::Outside);
        }
        painter.text(rr.center(), egui::Align2::CENTER_CENTER, &c.label, fit(&c.label, rr.width(), (12.0 * k).clamp(7.0, 14.0)), text);
        let how = match (num, c.source) {
            (Some(n), _) if taught => format!("button {n} (taught)"),
            (Some(n), Source::Known) => format!("button {n}"),
            (Some(n), _) => format!("button {n} (probably)"),
            (None, _) => "number not known yet: click, then press it on the panel".into(),
        };
        let does = mapping.map(|m| format!("\nsends: {m}")).unwrap_or_else(|| "\nclick to give it a shortcut".into());
        if resp.on_hover_text(format!("{}\n{how}{does}", c.label)).clicked() {
            clicked = Some(c.id.clone());
        }
    }
    if teaching.is_some() {
        ui.ctx().request_repaint();
    }
    clicked
}

// ------------------------------------------------------------------ Screens

fn friendly_output(o: &Output) -> String {
    if o.is_usb_display() {
        format!("{}  ·  WinWing display", o.name)
    } else {
        o.name.clone()
    }
}

impl App {
    fn ui_screens(&mut self, ui: &mut egui::Ui) {
        page_header(
            ui,
            "Screens",
            "Arrange your monitors, including the WinWing instrument display, and rotate or mirror any of them, for example when the display is mounted sideways or upside down in your pit.",
        );
        how_to(
            ui,
            "howto-screens",
            &[
                "Drag a screen in the picture to move it. It snaps to the edges of the other screens.",
                "Pick a rotation in the list below if a screen is mounted turned or mirrored. The orange dot in the picture marks the top edge.",
                "Click Apply. If the result is wrong, do nothing: it switches back after 15 seconds. Click Keep if it is right.",
            ],
        );

        if let Some(e) = self.outputs_err.clone() {
            card(ui, "", |ui| {
                ui.colored_label(RED, "Could not read your screen setup.");
                hint(ui, &format!("{e}. This page uses KDE Plasma's kscreen-doctor."));
                if ui.button("Try again").clicked() {
                    self.reload_outputs();
                }
            });
            return;
        }

        card(ui, "Layout", |ui| self.ui_layout_canvas(ui));

        let changed = self.outputs_edit != self.outputs;
        let mut apply = false;
        card(ui, "Screens", |ui| {
            egui::Grid::new("outs").striped(true).num_columns(5).spacing([16.0, 8.0]).show(ui, |ui| {
                ui.strong("Screen");
                ui.strong("On");
                ui.strong("Resolution");
                ui.strong("Rotation / mirror");
                ui.strong("Position");
                ui.end_row();
                for (i, o) in self.outputs_edit.iter_mut().enumerate() {
                    ui.label(friendly_output(o));
                    ui.checkbox(&mut o.enabled, "").on_hover_text("Turn this screen on or off");
                    ui.label(format!("{}×{}{}", o.mode_w, o.mode_h, if (o.scale - 1.0).abs() > 0.01 { format!(", scale {}", o.scale) } else { String::new() }));
                    egui::ComboBox::from_id_salt(("rot", i)).selected_text(o.rotation.label()).width(260.0).show_ui(ui, |ui| {
                        for r in Rotation::ALL {
                            ui.selectable_value(&mut o.rotation, r, r.label());
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(&mut o.x).prefix("x "));
                        ui.add(egui::DragValue::new(&mut o.y).prefix("y "));
                    });
                    ui.end_row();
                }
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.add_enabled(changed && self.pending.is_none(), egui::Button::new(RichText::new("Apply").strong())).clicked() {
                    apply = true;
                }
                if ui.add_enabled(changed, egui::Button::new("Undo my edits")).clicked() {
                    self.outputs_edit = self.outputs.clone();
                }
                if ui.button("Reload from system").clicked() {
                    self.reload_outputs();
                }
                if !changed && self.pending.is_none() {
                    hint(ui, "No changes yet.");
                }
            });
        });
        if apply {
            self.apply_screens();
        }

        if let Some(p) = &self.pending {
            let left = p.deadline.saturating_duration_since(Instant::now()).as_secs();
            let (mut keep, mut revert) = (false, false);
            card(ui, "", |ui| {
                ui.horizontal(|ui| {
                    ui.colored_label(AMBER, RichText::new(format!("Keep this layout? It switches back in {left} s.")).strong());
                    keep = ui.button("Keep").clicked();
                    revert = ui.button("Switch back now").clicked();
                });
            });
            if keep {
                self.pending = None;
            } else if revert {
                self.revert_displays();
            }
        }
        if !self.displays_msg.is_empty() {
            ui.colored_label(RED, &self.displays_msg);
        }
    }

    fn apply_screens(&mut self) {
        let mut new = self.outputs_edit.clone();
        displays::normalize(&mut new);
        let args = displays::diff_args(&self.outputs, &new);
        match displays::apply(&args) {
            Ok(_) => {
                log::log(format!("Screens: applied {}", args.join(" ")));
                self.pending = Some(PendingDisplay { before: self.outputs.clone(), deadline: Instant::now() + Duration::from_secs(15) });
                self.displays_msg.clear();
            }
            Err(e) => self.displays_msg = e.to_string(),
        }
        self.reload_outputs();
    }

    fn ui_layout_canvas(&mut self, ui: &mut egui::Ui) {
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 280.0), egui::Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 6.0, ui.visuals().extreme_bg_color);
        let en: Vec<usize> = (0..self.outputs_edit.len()).filter(|&i| self.outputs_edit[i].enabled).collect();
        if en.is_empty() {
            return;
        }
        let o = &self.outputs_edit;
        let minx = en.iter().map(|&i| o[i].x as f32).fold(f32::MAX, f32::min);
        let miny = en.iter().map(|&i| o[i].y as f32).fold(f32::MAX, f32::min);
        let maxx = en.iter().map(|&i| o[i].x as f32 + o[i].logical_size().0).fold(f32::MIN, f32::max);
        let maxy = en.iter().map(|&i| o[i].y as f32 + o[i].logical_size().1).fold(f32::MIN, f32::max);
        let pad = 24.0;
        let scale = ((rect.width() - 2.0 * pad) / (maxx - minx + 300.0)).min((rect.height() - 2.0 * pad) / (maxy - miny + 150.0));
        let origin = rect.min + Vec2::new(pad, pad);
        let to_screen = |x: f32, y: f32| origin + Vec2::new((x - minx) * scale, (y - miny) * scale);

        let pointer = resp.interact_pointer_pos();
        if resp.drag_started() {
            if let Some(p) = pointer {
                for &i in en.iter().rev() {
                    let (w, h) = self.outputs_edit[i].logical_size();
                    let r = egui::Rect::from_min_size(to_screen(self.outputs_edit[i].x as f32, self.outputs_edit[i].y as f32), Vec2::new(w * scale, h * scale));
                    if r.contains(p) {
                        self.drag = Some((i, p - r.min));
                        break;
                    }
                }
            }
        }
        if let (Some((i, off)), Some(p)) = (self.drag, pointer) {
            if resp.dragged() {
                let tl = p - off - origin;
                let mut nx = tl.x / scale + minx;
                let mut ny = tl.y / scale + miny;
                let (w, h) = self.outputs_edit[i].logical_size();
                let snap = 12.0 / scale;
                for &j in &en {
                    if j == i {
                        continue;
                    }
                    let o = &self.outputs_edit[j];
                    let (ow, oh) = o.logical_size();
                    let (ox, oy) = (o.x as f32, o.y as f32);
                    for cand in [ox + ow, ox - w, ox, ox + ow - w] {
                        if (nx - cand).abs() < snap {
                            nx = cand;
                        }
                    }
                    for cand in [oy + oh, oy - h, oy, oy + oh - h] {
                        if (ny - cand).abs() < snap {
                            ny = cand;
                        }
                    }
                }
                self.outputs_edit[i].x = nx.round() as i32;
                self.outputs_edit[i].y = ny.round() as i32;
            }
        }
        if resp.drag_stopped() {
            self.drag = None;
        }
        if resp.hovered() {
            ui.ctx().set_cursor_icon(if self.drag.is_some() { egui::CursorIcon::Grabbing } else { egui::CursorIcon::Grab });
        }

        for &i in &en {
            let o = &self.outputs_edit[i];
            let (w, h) = o.logical_size();
            let r = egui::Rect::from_min_size(to_screen(o.x as f32, o.y as f32), Vec2::new(w * scale, h * scale));
            let fill = if o.is_usb_display() { Color32::from_rgb(45, 75, 120) } else { Color32::from_rgb(58, 58, 66) };
            painter.rect_filled(r.shrink(1.5), 5.0, fill);
            painter.rect_stroke(r.shrink(1.5), 5.0, egui::Stroke::new(1.5, Color32::from_gray(170)), egui::StrokeKind::Inside);
            let title = if o.is_usb_display() { format!("{}\nWinWing display", o.name) } else { o.name.clone() };
            painter.text(r.center() - Vec2::new(0.0, 6.0), egui::Align2::CENTER_CENTER, title, egui::FontId::proportional(14.0), Color32::WHITE);
            let sub = if o.rotation == Rotation::None { format!("{}×{}", o.mode_w, o.mode_h) } else { format!("{}×{}, {}", o.mode_w, o.mode_h, o.rotation.label()) };
            painter.text(r.center_bottom() - Vec2::new(0.0, 12.0), egui::Align2::CENTER_CENTER, sub, egui::FontId::proportional(11.5), Color32::LIGHT_GRAY);
            let up = match o.rotation {
                Rotation::None | Rotation::Flipped => r.center_top() + Vec2::new(0.0, 9.0),
                Rotation::Inverted | Rotation::Flipped180 => r.center_bottom() - Vec2::new(0.0, 26.0),
                Rotation::Left | Rotation::Flipped270 => r.left_center() + Vec2::new(9.0, 0.0),
                Rotation::Right | Rotation::Flipped90 => r.right_center() - Vec2::new(9.0, 0.0),
            };
            painter.circle_filled(up, 4.0, AMBER);
        }
    }

    fn revert_displays(&mut self) {
        if let Some(p) = self.pending.take() {
            let args = displays::diff_args(&self.outputs, &p.before);
            match displays::apply(&args) {
                Ok(_) => log::log("Screens: switched back to the previous layout"),
                Err(e) => self.displays_msg = format!("Switching back failed: {e}"),
            }
            self.reload_outputs();
        }
    }

    fn handle_display_revert(&mut self) {
        if self.pending.as_ref().map(|p| Instant::now() >= p.deadline).unwrap_or(false) {
            self.revert_displays();
        }
    }
}

// ------------------------------------------------------------------ Overlay

impl App {
    fn ui_overlay(&mut self, ui: &mut egui::Ui) {
        page_header(
            ui,
            "MFD overlay",
            "Your old FluidPort. Each overlay puts a picture on top of everything on one screen, by default the AH-64D MFD frame with the button leader lines, so the MFDs DCS exports there look like the real thing. You can have several, on different screens. Mouse clicks go through them.",
        );
        how_to(
            ui,
            "howto-overlay",
            &[
                "Pick the screen for each overlay (Automatic = the WinWing display).",
                "x and y are where the picture's top-left corner goes, counted from the screen's top-left corner. Width and height are the picture size.",
                "Every number field takes sums: 778/2, (1080-762)/2, W-747 (W and H are that screen's width and height).",
                "Tick \"Show overlays\". On the DCS setup page, \"Put the MFDs where the overlays are\" makes DCS draw the MFDs exactly inside them; for the AH-64D the picture is shrunk into the frame's see-through window.",
            ],
        );

        if ui.checkbox(&mut self.overlay_on, RichText::new("Show overlays").strong().size(16.0)).changed() {
            self.overlay_place_at = Some(Instant::now() + Duration::from_millis(500));
        }
        if !self.overlay_msg.is_empty() {
            ui.colored_label(RED, &self.overlay_msg);
        }
        ui.add_space(4.0);

        let outputs = self.outputs.clone();
        let mut changed = false;
        let mut remove = None;
        let n = self.cfg.overlays.len();
        for i in 0..n {
            let placed = self.place(&self.cfg.overlays[i].clone());
            self.ensure_hole(&self.cfg.overlays[i].image.clone());
            let hole = self.overlay_holes.get(&self.cfg.overlays[i].image).cloned();
            let title = format!("Overlay {}: {}", i + 1, self.cfg.overlays[i].name);
            card(ui, &title, |ui| {
                let o = &mut self.cfg.overlays[i];
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        egui::Grid::new(("ovgrid", i)).num_columns(2).spacing([14.0, 8.0]).show(ui, |ui| {
                            ui.label("Name");
                            changed |= ui.add(egui::TextEdit::singleline(&mut o.name).desired_width(200.0)).changed();
                            ui.end_row();
                            ui.label("");
                            changed |= ui.checkbox(&mut o.enabled, "Show this one").changed();
                            ui.end_row();
                            ui.label("Screen");
                            let cur = if o.output.is_empty() {
                                format!("Automatic ({})", placed.as_ref().map(|p| p.output.clone()).unwrap_or_default())
                            } else {
                                outputs.iter().find(|x| x.name == o.output).map(friendly_output).unwrap_or_else(|| format!("{} (not connected)", o.output))
                            };
                            egui::ComboBox::from_id_salt(("ovout", i)).selected_text(cur).width(250.0).show_ui(ui, |ui| {
                                changed |= ui.selectable_value(&mut o.output, String::new(), "Automatic (WinWing display)").changed();
                                for out in outputs.iter().filter(|x| x.enabled) {
                                    changed |= ui.selectable_value(&mut o.output, out.name.clone(), friendly_output(out)).changed();
                                }
                            });
                            ui.end_row();
                            let (sw, sh) = placed.as_ref().map(|p| (p.screen.2 as f64, p.screen.3 as f64)).unwrap_or((0.0, 0.0));
                            ui.label("Top-left corner").on_hover_text("Distance from the screen's top-left corner to the picture's top-left corner");
                            ui.horizontal(|ui| {
                                changed |= expr_field(ui, "x", &mut o.x, sw, sh);
                                changed |= expr_field(ui, "y", &mut o.y, sw, sh);
                            });
                            ui.end_row();
                            ui.label("Size");
                            ui.horizontal(|ui| {
                                changed |= expr_field(ui, "w", &mut o.width, sw, sh);
                                changed |= expr_field(ui, "h", &mut o.height, sw, sh);
                            });
                            ui.end_row();
                            ui.label("Picture");
                            changed |= ui.add(egui::TextEdit::singleline(&mut o.image).hint_text("empty = built-in AH-64D MFD frame").desired_width(300.0)).changed();
                            ui.end_row();
                            ui.label("");
                            changed |= ui.checkbox(&mut o.click_through, "Mouse clicks go through it").changed();
                            ui.end_row();
                        });
                        ui.horizontal(|ui| {
                            if ui.button("Reset to the default values").on_hover_text("WinWing display, x 9, y 255, 747 × 762").clicked() {
                                let name = o.name.clone();
                                *o = OverlayCfg { name, enabled: o.enabled, ..Default::default() };
                                changed = true;
                            }
                            if n > 1 && ui.button("Remove").clicked() {
                                remove = Some(i);
                            }
                        });
                    });
                    ui.add_space(12.0);
                    if let Some(p) = &placed {
                        overlay_preview(ui, p, hole);
                    }
                });
            });
        }
        ui.horizontal(|ui| {
            if ui.button("+ Add another overlay").clicked() {
                let mut o = OverlayCfg { name: format!("MFD frame {}", self.cfg.overlays.len() + 1), ..Default::default() };
                o.enabled = true;
                self.cfg.overlays.push(o);
                changed = true;
            }
            if ui.button("Put them in place again").on_hover_text("Use this if the window manager moved an overlay").clicked() {
                self.overlay_place_at = Some(Instant::now());
            }
        });
        if let Some(i) = remove {
            self.cfg.overlays.remove(i);
            changed = true;
        }
        if changed {
            self.overlay_place_at = Some(Instant::now() + Duration::from_millis(300));
            self.save_soon();
        }
    }

    /// The screen an overlay goes on: its own choice, else the first WinWing display, else the first screen.
    fn overlay_output(&self, o: &OverlayCfg) -> Option<Output> {
        let enabled = || self.outputs.iter().filter(|x| x.enabled);
        if !o.output.is_empty() {
            if let Some(x) = self.outputs.iter().find(|x| x.name == o.output) {
                return Some(x.clone());
            }
        }
        enabled().find(|x| x.is_usb_display()).or_else(|| enabled().next()).cloned()
    }

    fn place(&self, o: &OverlayCfg) -> Option<Placed> {
        let out = self.overlay_output(o);
        let (name, sx, sy, sw, sh) = match &out {
            Some(x) => {
                let (w, h) = x.logical_size();
                (x.name.clone(), x.x as f32, x.y as f32, w, h)
            }
            None => (String::new(), 0.0, 0.0, 1920.0, 1080.0),
        };
        let e = |s: &str| calc::eval_i(s, sw as f64, sh as f64).ok();
        Some(Placed { output: name, screen: (sx, sy, sw, sh), x: e(&o.x)?, y: e(&o.y)?, w: e(&o.width)?.max(16), h: e(&o.height)?.max(16) })
    }

    fn ensure_hole(&mut self, image: &str) {
        if self.overlay_holes.contains_key(image) {
            return;
        }
        let v = overlay::load_image(image).map(|img| (img.size, overlay::detect_hole(&img))).unwrap_or(([0, 0], None));
        self.overlay_holes.insert(image.to_string(), v);
    }

    /// MFD export areas for the monitor setup, one per shown overlay, in DCS window pixels.
    fn overlay_slots(&mut self) -> Vec<monitorsetup::Slot> {
        let (ox, oy) = (self.cfg.monitor_setup.dcs_origin_x, self.cfg.monitor_setup.dcs_origin_y);
        let list: Vec<OverlayCfg> = self.cfg.overlays.iter().filter(|o| o.enabled).cloned().collect();
        let mut v = Vec::new();
        for o in list {
            let Some(p) = self.place(&o) else { continue };
            self.ensure_hole(&o.image);
            let x = p.screen.0 as i32 + p.x - ox;
            let y = p.screen.1 as i32 + p.y - oy;
            let inner = match self.overlay_holes.get(&o.image) {
                Some((size, Some((hx, hy, hw, hh)))) if size[0] > 0 && size[1] > 0 => {
                    let (fx, fy) = (p.w as f64 / size[0] as f64, p.h as f64 / size[1] as f64);
                    Some((x + (*hx as f64 * fx).round() as i32, y + (*hy as f64 * fy).round() as i32, (*hw as f64 * fx).round() as i32, (*hh as f64 * fy).round() as i32))
                }
                _ => None,
            };
            v.push(monitorsetup::Slot { outer: (x, y, p.w, p.h), inner });
        }
        v
    }

    fn handle_overlay_requests(&mut self) {
        if remap::OVERLAY_TOGGLE.swap(false, Ordering::Relaxed) {
            self.overlay_on = !self.overlay_on;
            self.overlay_place_at = Some(Instant::now() + Duration::from_millis(500));
        }
        if remap::OVERLAY_NEXT.swap(false, Ordering::Relaxed) {
            let names: Vec<String> = self.outputs.iter().filter(|o| o.enabled).map(|o| o.name.clone()).collect();
            if let (false, Some(first)) = (names.is_empty(), self.cfg.overlays.first().cloned()) {
                let curname = self.overlay_output(&first).map(|o| o.name).unwrap_or_default();
                let cur = names.iter().position(|n| *n == curname);
                self.cfg.overlays[0].output = names[cur.map(|i| (i + 1) % names.len()).unwrap_or(0)].clone();
                self.save_soon();
                self.overlay_place_at = Some(Instant::now());
            }
        }
        if let Some(t) = self.overlay_place_at {
            if Instant::now() >= t && self.overlay_on {
                self.overlay_place_at = None;
                self.overlay_msg.clear();
                for (i, o) in self.cfg.overlays.clone().iter().enumerate().filter(|(_, o)| o.enabled) {
                    let Some(p) = self.place(o) else {
                        self.overlay_msg = format!("Overlay {}: a number field has a mistake", i + 1);
                        continue;
                    };
                    if let Err(e) = overlay::place_with_kwin(&overlay::app_id(i), &p.output, p.x, p.y, p.w as u32, p.h as u32) {
                        self.overlay_msg = e.to_string();
                    }
                }
            }
        }
    }

    fn show_overlay(&mut self, ctx: &egui::Context) {
        if !self.overlay_on {
            return;
        }
        let list: Vec<(usize, OverlayCfg)> = self.cfg.overlays.iter().cloned().enumerate().filter(|(_, o)| o.enabled).collect();
        let mut close = false;
        for (i, o) in list {
            let Some(p) = self.place(&o) else { continue };
            if !self.overlay_tex.contains_key(&o.image) {
                match overlay::load_image(&o.image) {
                    Ok(img) => {
                        let tex = ctx.load_texture(format!("overlay-{}", o.image), img, egui::TextureOptions::LINEAR);
                        self.overlay_tex.insert(o.image.clone(), tex);
                    }
                    Err(e) => {
                        self.overlay_msg = format!("Overlay {}: cannot load the picture: {e}", i + 1);
                        continue;
                    }
                }
            }
            let tex_id = self.overlay_tex[&o.image].id();
            let id = overlay::app_id(i);
            let builder = egui::ViewportBuilder::default()
                .with_title(format!("UWACS overlay {}", i + 1))
                .with_app_id(id.clone())
                .with_decorations(false)
                .with_transparent(true)
                .with_always_on_top()
                .with_taskbar(false)
                .with_resizable(false)
                .with_mouse_passthrough(o.click_through)
                .with_inner_size(Vec2::new(p.w as f32, p.h as f32));
            ctx.show_viewport_immediate(egui::ViewportId::from_hash_of(&id), builder, |ui, _class| {
                egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| {
                    let r = ui.max_rect();
                    ui.painter().image(tex_id, r, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
                });
                if ui.ctx().input(|i| i.viewport().close_requested()) {
                    close = true;
                }
            });
        }
        if close {
            self.overlay_on = false;
        }
    }
}

/// A number field that takes sums. Shows "= value" (or what is wrong) next to it.
fn expr_field(ui: &mut egui::Ui, label: &str, s: &mut String, w: f64, h: f64) -> bool {
    let res = calc::eval_i(s, w, h);
    let mut changed = false;
    ui.label(label);
    let te = egui::TextEdit::singleline(s).desired_width(72.0).text_color(if res.is_ok() { ui.visuals().text_color() } else { RED });
    let resp = ui.add(te);
    changed |= resp.changed();
    // pressing Enter replaces the sum with its result
    if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        if let Ok(v) = res {
            if *s != v.to_string() {
                *s = v.to_string();
                changed = true;
            }
        }
    }
    let plain = s.trim().parse::<i64>().is_ok();
    match calc::eval_i(s, w, h) {
        Ok(v) if !plain => {
            ui.label(RichText::new(format!("= {v}")).color(GREEN));
        }
        Ok(_) => {}
        Err(e) => {
            ui.label(RichText::new(e).color(RED).small());
        }
    }
    changed
}

/// The screen with the overlay drawn at its top-left position, plus the frame's see-through window.
fn overlay_preview(ui: &mut egui::Ui, p: &Placed, hole: Option<([usize; 2], Option<(usize, usize, usize, usize)>)>) {
    let (_, _, sw, sh) = p.screen;
    let k = (260.0 / sw).min(220.0 / sh);
    ui.vertical(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::new(sw * k, sh * k), egui::Sense::hover());
        let painter = ui.painter_at(rect.expand(1.0));
        painter.rect_filled(rect, 3.0, Color32::from_gray(50));
        let o = egui::Rect::from_min_size(rect.min + Vec2::new(p.x as f32 * k, p.y as f32 * k), Vec2::new(p.w as f32 * k, p.h as f32 * k));
        painter.rect_filled(o, 2.0, Color32::from_rgb(45, 75, 120));
        if let Some((size, Some((hx, hy, hw, hh)))) = hole {
            let (fx, fy) = (o.width() / size[0] as f32, o.height() / size[1] as f32);
            let inner = egui::Rect::from_min_size(o.min + Vec2::new(hx as f32 * fx, hy as f32 * fy), Vec2::new(hw as f32 * fx, hh as f32 * fy));
            painter.rect_filled(inner, 1.0, Color32::from_rgb(14, 24, 18));
            painter.text(inner.center(), egui::Align2::CENTER_CENTER, "AH-64D\npicture", egui::FontId::proportional(10.0), Color32::from_rgb(90, 150, 100));
        }
        // top-left anchor marker
        painter.line_segment([rect.min, o.min], egui::Stroke::new(1.0, AMBER));
        painter.circle_filled(o.min, 3.0, AMBER);
        ui.label(RichText::new(format!("{}: {} × {}  ·  overlay at {}, {}", p.output, sw as i32, sh as i32, p.x, p.y)).small().color(ui.visuals().weak_text_color()));
    });
}

// ------------------------------------------------------------------ DCS setup

impl App {
    fn ui_dcs(&mut self, ui: &mut egui::Ui) {
        page_header(ui, "DCS setup", "Two things DCS needs: the DCS-BIOS add-on (so the panel screens get data), and a monitor setup that puts the MFDs on your instrument display. The button below does both.");

        // folder picked in the zenity thread
        if let Some(pick) = self.setup_pick.lock().unwrap().take() {
            if let Some(p) = pick {
                self.setup_dir = p.display().to_string();
            }
        }
        let running = self.setup_prog.as_ref().map(|p| !p.lock().unwrap().finished).unwrap_or(false);
        let mut start = false;
        card(ui, "One-click setup", |ui| {
            ui.label("Point UWACS at your DCS. Any of these works: the Wine/Proton prefix, the DCS game folder, or Saved Games/DCS.");
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.setup_dir).desired_width(460.0));
                if ui.button("Browse…").clicked() {
                    let slot = self.setup_pick.clone();
                    let startdir = self.setup_dir.clone();
                    let ctx = ui.ctx().clone();
                    thread::spawn(move || {
                        let r = dcssetup::pick_folder(&startdir);
                        *slot.lock().unwrap() = Some(r);
                        ctx.request_repaint();
                    });
                }
            });
            let resolved = dcssetup::resolve_saved_games(std::path::Path::new(self.setup_dir.trim()));
            match &resolved {
                Ok(p) => check_row(ui, GREEN, "DCS settings folder found", &p.display().to_string()),
                Err(e) => check_row(ui, RED, "Not a DCS folder", e),
            }
            ui.add_space(4.0);
            ui.checkbox(&mut self.setup_bios, "Download and install the newest DCS-BIOS, and load it from Export.lua")
                .on_hover_text("An existing Scripts/DCS-BIOS folder and Export.lua are kept as backups next to the new ones. Other things in Export.lua (TacView, SRS…) stay.");
            ui.checkbox(&mut self.setup_ms, "Write the MFD monitor setup (section 2 below)");
            ui.add_space(6.0);
            let ok = resolved.is_ok() && !running && (self.setup_bios || self.setup_ms);
            if ui.add_enabled(ok, egui::Button::new(RichText::new("Set everything up").strong().size(16.0)).min_size(Vec2::new(220.0, 34.0))).clicked() {
                start = true;
            }
            if let Some(prog) = &self.setup_prog {
                let g = prog.lock().unwrap();
                ui.add_space(6.0);
                for st in &g.steps {
                    let (c, txt) = match &st.state {
                        dcssetup::StepState::Running => (AMBER, "working…".to_string()),
                        dcssetup::StepState::Done(m) => (GREEN, m.clone()),
                        dcssetup::StepState::Failed(m) => (RED, m.clone()),
                        dcssetup::StepState::Skipped(m) => (GREY, m.clone()),
                    };
                    check_row(ui, c, &st.title, &txt);
                }
                if g.finished {
                    let failed = g.steps.iter().any(|s| matches!(s.state, dcssetup::StepState::Failed(_)));
                    if failed {
                        ui.colored_label(RED, "Something failed, see above. Nothing was deleted; old files are kept as backups.");
                    } else {
                        ui.colored_label(GREEN, format!("All done. Start DCS, pick \"{}\" in Options > System > Monitors, and restart DCS once.", self.cfg.monitor_setup.name));
                    }
                }
            }
        });
        if start {
            // remember it, so the rest of UWACS finds this DCS too
            self.cfg.dcs_folder = self.setup_dir.trim().to_string();
            *monitorsetup::DCS_FOLDER.write().unwrap() = self.cfg.dcs_folder.clone();
            self.save_soon();
            let prog: dcssetup::Shared = Default::default();
            self.setup_prog = Some(prog.clone());
            let slots = self.overlay_slots();
            let ms = self.cfg.monitor_setup.clone();
            let dir = std::path::PathBuf::from(self.setup_dir.trim());
            let opts = dcssetup::Options { install_bios: self.setup_bios, write_monitor_setup: self.setup_ms };
            let ctx = ui.ctx().clone();
            thread::spawn(move || {
                dcssetup::run(dir, opts, ms, slots, prog);
                ctx.request_repaint();
            });
        }
        if let Some(p) = &self.setup_prog {
            let fin = p.lock().unwrap().finished;
            if fin && self.dcsbios_installed != Some(true) {
                self.dcsbios_installed = monitorsetup::dcsbios_installed();
            }
        }

        let installed = self.dcsbios_installed;
        card(ui, "1. DCS-BIOS", |ui| {
            ui.label("DCS-BIOS is a free add-on for DCS. It streams what is on the cockpit displays out of the game, and UWACS draws that on your UFC and PFP. Without it, the panel screens stay on their waiting page.");
            ui.add_space(4.0);
            match installed {
                Some(true) => check_row(ui, GREEN, "Installed", ""),
                Some(false) => check_row(ui, RED, "Not installed yet", "Use \"Set everything up\" above."),
                None => check_row(ui, GREY, "DCS settings folder not found", "Point the one-click setup above at your DCS."),
            }
            egui::CollapsingHeader::new("Doing it by hand instead").show(ui, |ui| {
                ui.label("Unzip the DCS-BIOS release into Saved Games/DCS/Scripts, then add this line to Scripts/Export.lua:");
                self.command_box(ui, "export", dcssetup::EXPORT_LINE);
                ui.hyperlink_to("DCS-BIOS download page", DCSBIOS_URL);
            });
            egui::CollapsingHeader::new("Advanced: where UWACS listens").show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Host");
                    if ui.text_edit_singleline(&mut self.cfg.dcsbios_host).changed() {
                        self.dirty = true;
                    }
                    ui.label("Port");
                    if ui.add(egui::DragValue::new(&mut self.cfg.dcsbios_port)).changed() {
                        self.dirty = true;
                    }
                });
                hint(ui, "Default 127.0.0.1 port 7778. UWACS also listens to the DCS-BIOS network broadcast. Changes apply after restarting UWACS.");
            });
        });

        card(ui, "2. MFD exports (monitor setup)", |ui| {
            ui.label("DCS can draw the MFDs into separate areas of its window, so they can sit on another screen, for example under your MFD overlays. This writes the setup file (what FluidPort shipped as mine.lua).");
            ui.add_space(6.0);
            let mut ch = false;
            ch |= ui.checkbox(&mut self.cfg.monitor_setup.follow_overlays, RichText::new("Put the MFDs where the MFD overlays are").strong()).changed();
            hint(ui, "Overlay 1 = MFD 1, overlay 2 = MFD 2, and so on. The AH-64D picture is shrunk to fit inside each frame's see-through window; other aircraft fill the whole overlay. Without this, the MFDs go in a row below the normal view.");
            let slots = self.overlay_slots();
            ui.add_space(6.0);
            if self.cfg.monitor_setup.follow_overlays {
                ui_follow_preview(ui, &self.cfg.monitor_setup, &self.outputs, &slots);
            } else {
                ui_monitor_preview(ui, &self.cfg.monitor_setup);
            }
            ui.add_space(6.0);
            let follow = self.cfg.monitor_setup.follow_overlays;
            let c = &mut self.cfg.monitor_setup;
            egui::Grid::new("ms").num_columns(2).spacing([14.0, 8.0]).show(ui, |ui| {
                ui.label("Name shown in DCS");
                ch |= ui.text_edit_singleline(&mut c.name).changed();
                ui.end_row();
                ui.label("Normal view (width × height)");
                ui.horizontal(|ui| {
                    ch |= ui.add(egui::DragValue::new(&mut c.main_w)).changed();
                    ui.label("×");
                    ch |= ui.add(egui::DragValue::new(&mut c.main_h)).changed();
                });
                ui.end_row();
                if follow {
                    ui.label("DCS window top-left on the desktop").on_hover_text("Usually 0, 0: the top-left of your leftmost screen. Overlay positions are measured from here.");
                    ui.horizontal(|ui| {
                        ch |= ui.add(egui::DragValue::new(&mut c.dcs_origin_x).prefix("x ")).changed();
                        ch |= ui.add(egui::DragValue::new(&mut c.dcs_origin_y).prefix("y ")).changed();
                    });
                    ui.end_row();
                }
                ui.label("Where the MFD row starts");
                ui.horizontal(|ui| {
                    ch |= ui.add(egui::DragValue::new(&mut c.mfd_x).prefix("x ")).changed();
                    ch |= ui.add(egui::DragValue::new(&mut c.mfd_y).prefix("y ")).changed();
                });
                ui.end_row();
                ui.label("Size of each MFD");
                ui.horizontal(|ui| {
                    ch |= ui.add(egui::DragValue::new(&mut c.outer_w)).changed();
                    ui.label("×");
                    ch |= ui.add(egui::DragValue::new(&mut c.outer_h)).changed();
                });
                ui.end_row();
                ui.label("AH-64D picture inside the frame").on_hover_text("The Apache exports only the picture inside the MFD frame: offset from the frame corner, then size.");
                ui.horizontal(|ui| {
                    ch |= ui.add(egui::DragValue::new(&mut c.inner_dx).prefix("dx ")).changed();
                    ch |= ui.add(egui::DragValue::new(&mut c.inner_dy).prefix("dy ")).changed();
                    ch |= ui.add(egui::DragValue::new(&mut c.inner_w).prefix("w ")).changed();
                    ch |= ui.add(egui::DragValue::new(&mut c.inner_h).prefix("h ")).changed();
                });
                ui.end_row();
                ui.label("File");
                ch |= ui.add(egui::TextEdit::singleline(&mut c.save_path).desired_width(560.0)).changed();
                ui.end_row();
            });
            if ch {
                self.dirty = true;
            }
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui.button(RichText::new("Write the file").strong()).clicked() {
                    self.ms_msg = match monitorsetup::write(&self.cfg.monitor_setup, &slots) {
                        Ok(p) => format!("Written to {}. Now in DCS: Options > System > Monitors > \"{}\", then restart DCS.", p.display(), self.cfg.monitor_setup.name),
                        Err(e) => format!("Failed: {e}"),
                    };
                    log::log(format!("Monitor setup: {}", self.ms_msg));
                }
                if ui.button("Reset to FluidPort values").clicked() {
                    let path = self.cfg.monitor_setup.save_path.clone();
                    self.cfg.monitor_setup = monitorsetup::MonitorSetupCfg { save_path: path, ..Default::default() };
                    self.dirty = true;
                }
            });
            if !self.ms_msg.is_empty() {
                ui.label(&self.ms_msg);
            }
            egui::CollapsingHeader::new("Show the file contents").show(ui, |ui| {
                let mut s = monitorsetup::generate(&self.cfg.monitor_setup, &slots);
                ui.add(egui::TextEdit::multiline(&mut s).code_editor().desired_width(f32::INFINITY).interactive(false));
            });
        });
    }
}

/// The desktop with the DCS normal view and the MFD areas taken from the overlays.
fn ui_follow_preview(ui: &mut egui::Ui, c: &monitorsetup::MonitorSetupCfg, outputs: &[Output], slots: &[monitorsetup::Slot]) {
    if slots.is_empty() {
        ui.colored_label(AMBER, "No overlay is switched on (\"Show this one\" on the MFD overlay page), so the MFDs go in the row below the normal view.");
        return;
    }
    let (ox, oy) = (c.dcs_origin_x as f32, c.dcs_origin_y as f32);
    let mut rects: Vec<(egui::Rect, Color32, String)> = Vec::new();
    for o in outputs.iter().filter(|o| o.enabled) {
        let (w, h) = o.logical_size();
        rects.push((egui::Rect::from_min_size(egui::pos2(o.x as f32 - ox, o.y as f32 - oy), Vec2::new(w, h)), Color32::from_gray(48), o.name.clone()));
    }
    rects.push((egui::Rect::from_min_size(egui::pos2(0.0, 0.0), Vec2::new(c.main_w as f32, c.main_h as f32)), Color32::from_rgba_unmultiplied(70, 70, 90, 140), "Normal view".into()));
    let mut bb = rects.iter().fold(egui::Rect::NOTHING, |a, r| a.union(r.0));
    for s in slots {
        bb = bb.union(egui::Rect::from_min_size(egui::pos2(s.outer.0 as f32, s.outer.1 as f32), Vec2::new(s.outer.2 as f32, s.outer.3 as f32)));
    }
    let k = (ui.available_width().min(620.0) / bb.width()).min(240.0 / bb.height());
    let (rect, _) = ui.allocate_exact_size(bb.size() * k, egui::Sense::hover());
    let p = ui.painter_at(rect.expand(1.0));
    let map = |r: egui::Rect| egui::Rect::from_min_max(rect.min + (r.min - bb.min) * k, rect.min + (r.max - bb.min) * k);
    for (r, col, name) in &rects {
        let m = map(*r);
        p.rect(m, 2.0, *col, egui::Stroke::new(1.0, Color32::from_gray(90)), egui::StrokeKind::Inside);
        p.text(m.left_top() + Vec2::new(4.0, 3.0), egui::Align2::LEFT_TOP, name, egui::FontId::proportional(10.0), Color32::from_gray(200));
    }
    for (i, s) in slots.iter().enumerate() {
        let o = map(egui::Rect::from_min_size(egui::pos2(s.outer.0 as f32, s.outer.1 as f32), Vec2::new(s.outer.2 as f32, s.outer.3 as f32)));
        p.rect_filled(o, 1.0, Color32::from_rgb(45, 75, 120));
        if let Some(inn) = s.inner {
            p.rect_filled(map(egui::Rect::from_min_size(egui::pos2(inn.0 as f32, inn.1 as f32), Vec2::new(inn.2 as f32, inn.3 as f32))), 1.0, Color32::from_rgb(20, 40, 28));
        }
        p.text(o.center(), egui::Align2::CENTER_CENTER, format!("MFD {}", i + 1), egui::FontId::proportional(11.0), Color32::WHITE);
    }
    let need_w = bb.max.x.max(0.0).round() as i32;
    let need_h = bb.max.y.max(0.0).round() as i32;
    ui.label(format!("Set the DCS resolution (Options > System) to at least {need_w} × {need_h} so its window covers the MFD screen."));
    for (i, s) in slots.iter().enumerate() {
        let inner = s.inner.map(|i| format!("  ·  AH-64D picture {} × {} at {}, {}", i.2, i.3, i.0, i.1)).unwrap_or_default();
        hint(ui, &format!("MFD {}: {} × {} at {}, {}{inner}", i + 1, s.outer.2, s.outer.3, s.outer.0, s.outer.1));
    }
}

/// Small drawing of the DCS window: main view and the three MFD areas.
fn ui_monitor_preview(ui: &mut egui::Ui, c: &monitorsetup::MonitorSetupCfg) {
    let total_w = (c.main_w as f32).max(c.mfd_x as f32 + 3.0 * c.outer_w as f32).max(1.0);
    let total_h = (c.main_h as f32).max(c.mfd_y as f32 + c.outer_h as f32).max(1.0);
    let w = ui.available_width().min(520.0);
    let scale = (w / total_w).min(220.0 / total_h);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(total_w * scale, total_h * scale), egui::Sense::hover());
    let p = ui.painter_at(rect.expand(1.0));
    let r = |x: f32, y: f32, ww: f32, hh: f32| egui::Rect::from_min_size(rect.min + Vec2::new(x * scale, y * scale), Vec2::new(ww * scale, hh * scale));
    let main = r(0.0, 0.0, c.main_w as f32, c.main_h as f32);
    p.rect_filled(main, 3.0, Color32::from_rgb(58, 58, 66));
    p.text(main.center(), egui::Align2::CENTER_CENTER, "Normal view", egui::FontId::proportional(13.0), Color32::WHITE);
    for (i, name) in ["MFD 1", "MFD 2", "MFD 3"].iter().enumerate() {
        let m = r(c.mfd_x as f32 + i as f32 * c.outer_w as f32, c.mfd_y as f32, c.outer_w as f32, c.outer_h as f32);
        p.rect_filled(m.shrink(1.0), 3.0, Color32::from_rgb(45, 75, 120));
        p.text(m.center(), egui::Align2::CENTER_CENTER, *name, egui::FontId::proportional(12.0), Color32::WHITE);
    }
    hint(ui, "MFD 1/2/3: F/A-18C = center/left/right, AH-64D = left/right/TEDAC, AV-8B and most others = left/right.");
}

// ------------------------------------------------------------------ Log

impl App {
    fn ui_log(&mut self, ui: &mut egui::Ui) {
        page_header(ui, "Log", "Everything UWACS did, newest first. Useful when something does not work; you can copy it and send it to someone.");
        ui.horizontal(|ui| {
            if ui.button("Copy all").clicked() {
                ui.ctx().copy_text(log::lines().join("\n"));
            }
            if ui.button("Clear").clicked() {
                log::clear();
            }
        });
        ui.add_space(6.0);
        card(ui, "", |ui| {
            let lines = log::lines();
            if lines.is_empty() {
                hint(ui, "Empty.");
            }
            for l in lines.iter().rev().take(500) {
                ui.label(RichText::new(l).monospace());
            }
        });
    }

    /// UWACS_SCREENSHOT=out.png: save a screenshot of the window after ~3 s and quit (for tests).
    fn test_screenshot(&mut self, ctx: &egui::Context) {
        let Ok(path) = std::env::var("UWACS_SCREENSHOT") else { return };
        static STARTED: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
        static ASKED: AtomicBool = AtomicBool::new(false);
        static DONE: AtomicBool = AtomicBool::new(false);
        let t0 = *STARTED.get_or_init(Instant::now);
        if t0.elapsed() > Duration::from_secs(3) && !ASKED.swap(true, Ordering::Relaxed) {
            let cb = egui::ScreenshotCallback::new(move |img| {
                let [w, h] = img.size;
                let rgba: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
                if let Some(buf) = image::RgbaImage::from_raw(w as u32, h as u32, rgba) {
                    let _ = buf.save(&path);
                }
                DONE.store(true, Ordering::Relaxed);
            });
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(cb));
        }
        if DONE.load(Ordering::Relaxed) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ctx.request_repaint();
    }
}
