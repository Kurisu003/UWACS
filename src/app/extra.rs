//! Panel-screen customisation (custom UFC/PFP pages from DCS-BIOS values) and the Settings page.

use std::time::Instant;

use egui::{Color32, RichText, Vec2};

use super::{card, hint, how_to, page_header, App, ACCENT, AMBER, GREEN, GREY, RED};
use crate::config::{self, Config};
use crate::custom::{self, CustomPfp, CustomUfc};
use crate::dcsbios;
use crate::log;
use crate::update::{self, State};

/// What a zenity file dialog was opened for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DialogFor {
    Export,
    Import,
}

pub type DialogSlot = std::sync::Arc<std::sync::Mutex<Option<(DialogFor, Option<std::path::PathBuf>)>>>;

fn aircraft_label(a: &str) -> String {
    if a.is_empty() {
        "Every aircraft".into()
    } else {
        a.to_string()
    }
}

/// Combo listing "Every aircraft", the aircraft flying now and every aircraft DCS-BIOS knows.
fn aircraft_combo(ui: &mut egui::Ui, id: impl std::hash::Hash + std::fmt::Debug, value: &mut String, list: &[String], flying: &str) -> bool {
    let mut changed = false;
    egui::ComboBox::from_id_salt(id).selected_text(aircraft_label(value)).width(220.0).height(400.0).show_ui(ui, |ui| {
        changed |= ui.selectable_value(value, String::new(), "Every aircraft").changed();
        if !flying.is_empty() {
            changed |= ui.selectable_value(value, flying.to_string(), format!("{flying}  (flying now)")).changed();
        }
        for a in list {
            changed |= ui.selectable_value(value, a.clone(), a).changed();
        }
    });
    changed
}

fn tpl_field(ui: &mut egui::Ui, s: &mut String, width: f32, hint_text: &str) -> bool {
    ui.add(egui::TextEdit::singleline(s).font(egui::TextStyle::Monospace).desired_width(width).hint_text(hint_text)).changed()
}

fn pfp_color(c: &str) -> Color32 {
    match c {
        "white" => Color32::from_gray(235),
        "cyan" => Color32::from_rgb(80, 220, 230),
        "yellow" => Color32::from_rgb(240, 220, 70),
        "orange" => Color32::from_rgb(245, 150, 40),
        "red" => Color32::from_rgb(240, 70, 60),
        "magenta" => Color32::from_rgb(230, 80, 220),
        _ => Color32::from_rgb(80, 230, 110),
    }
}

impl App {
    pub(super) fn push_custom(&mut self) {
        *custom::SCREENS.write().unwrap() = self.cfg.custom_screens.clone();
        self.save_soon();
    }

    /// Custom pages on the "Panel screens" page.
    pub(super) fn ui_custom_screens(&mut self, ui: &mut egui::Ui) {
        let snap = dcsbios::bios().snapshot();
        let flying = dcsbios::module_name(&snap);
        if self.aircraft_cache.is_none() {
            self.aircraft_cache = Some(custom::aircraft_list());
        }
        let list = self.aircraft_cache.clone().unwrap_or_default();

        card(ui, "Your own screen pages", |ui| {
            ui.label("Put anything DCS-BIOS knows on the UFC and the PFP. Write the text you want and put DCS-BIOS names in curly brackets; UWACS fills in the live value.");
            how_to(
                ui,
                "howto-custom",
                &[
                    "Add a UFC or PFP page below and pick the aircraft it is for (or Every aircraft).",
                    "Find the value you want under \"DCS-BIOS names\" at the bottom and click Copy, then paste it into a field. Example: FUEL {IFEI_FUEL_INT}.",
                    "{NAME%} shows a value as percent of its maximum, {NAME:5} makes it exactly 5 characters wide (numbers right-aligned).",
                    "A page for the exact aircraft wins over an Every aircraft page, and both win over UWACS's built-in pages. Untick \"Use it\" to go back to the built-in page.",
                ],
            );
            if custom::json_dir().is_none() {
                ui.colored_label(AMBER, "DCS-BIOS is not installed, so UWACS cannot look up names yet. Use \"Set everything up\" on the DCS setup page first.");
            }
        });

        // ---- UFC pages
        let mut changed = false;
        let mut remove_u = None;
        card(ui, &format!("Custom UFC pages ({})", self.cfg.custom_screens.ufc.len()), |ui| {
            for (i, u) in self.cfg.custom_screens.ufc.iter_mut().enumerate() {
                let title = format!("{}  ·  {}{}", u.name, aircraft_label(&u.aircraft), if u.enabled { "" } else { "  (off)" });
                egui::CollapsingHeader::new(title).id_salt(("cufc", i)).default_open(true).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Name");
                        changed |= ui.add(egui::TextEdit::singleline(&mut u.name).desired_width(160.0)).changed();
                        ui.label("for");
                        changed |= aircraft_combo(ui, ("ufcac", i), &mut u.aircraft, &list, &flying);
                        changed |= ui.checkbox(&mut u.enabled, "Use it").changed();
                        if ui.button("Remove").clicked() {
                            remove_u = Some(i);
                        }
                    });
                    ui.horizontal_top(|ui| {
                        egui::Grid::new(("ufcgrid", i)).num_columns(3).spacing([8.0, 6.0]).show(ui, |ui| {
                            ui.label("COMM 1 / 2");
                            ui.horizontal(|ui| {
                                changed |= tpl_field(ui, &mut u.comm1, 110.0, "1 character");
                                changed |= tpl_field(ui, &mut u.comm2, 110.0, "1 character");
                            });
                            ui.end_row();
                            ui.label("Scratchpad");
                            ui.horizontal(|ui| {
                                changed |= tpl_field(ui, &mut u.scratch_small, 80.0, "2 small");
                                changed |= tpl_field(ui, &mut u.scratch, 220.0, "7 large characters");
                            });
                            ui.end_row();
                            for k in 0..5 {
                                ui.label(format!("Option {}", k + 1));
                                ui.horizontal(|ui| {
                                    changed |= tpl_field(ui, &mut u.options[k], 160.0, "4 characters");
                                    ui.label("cue");
                                    changed |= tpl_field(ui, &mut u.cues[k], 140.0, "lit if not 0/empty");
                                });
                                ui.end_row();
                            }
                        });
                        ui.add_space(12.0);
                        let cat = custom::catalog(if u.aircraft.is_empty() { &flying } else { &u.aircraft });
                        ufc_preview(ui, &custom::ufc_package(u, &snap, &cat));
                    });
                });
                ui.add_space(4.0);
            }
            if ui.button("+ Add a UFC page").clicked() {
                self.cfg.custom_screens.ufc.push(CustomUfc { aircraft: flying.clone(), ..Default::default() });
                changed = true;
            }
        });
        if let Some(i) = remove_u {
            self.cfg.custom_screens.ufc.remove(i);
            changed = true;
        }

        // ---- PFP pages
        let mut remove_p = None;
        card(ui, &format!("Custom PFP pages ({})", self.cfg.custom_screens.pfp.len()), |ui| {
            for (i, p) in self.cfg.custom_screens.pfp.iter_mut().enumerate() {
                let title = format!("{}  ·  {}{}", p.name, aircraft_label(&p.aircraft), if p.enabled { "" } else { "  (off)" });
                egui::CollapsingHeader::new(title).id_salt(("cpfp", i)).default_open(true).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Name");
                        changed |= ui.add(egui::TextEdit::singleline(&mut p.name).desired_width(160.0)).changed();
                        ui.label("for");
                        changed |= aircraft_combo(ui, ("pfpac", i), &mut p.aircraft, &list, &flying);
                        changed |= ui.checkbox(&mut p.enabled, "Use it").changed();
                        if ui.button("Remove").clicked() {
                            remove_p = Some(i);
                        }
                    });
                    p.lines.resize(crate::pfp::ROWS, Default::default());
                    ui.horizontal_top(|ui| {
                        ui.vertical(|ui| {
                            for (k, line) in p.lines.iter_mut().enumerate() {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(format!("{:>2}", k + 1)).monospace().color(GREY));
                                    changed |= tpl_field(ui, &mut line.text, 300.0, "");
                                    egui::ComboBox::from_id_salt(("pfpc", i, k)).selected_text(RichText::new(&line.color).color(pfp_color(&line.color))).width(80.0).show_ui(ui, |ui| {
                                        for c in custom::PFP_COLORS {
                                            changed |= ui.selectable_value(&mut line.color, c.to_string(), RichText::new(c).color(pfp_color(c))).changed();
                                        }
                                    });
                                });
                            }
                        });
                        ui.add_space(12.0);
                        let cat = custom::catalog(if p.aircraft.is_empty() { &flying } else { &p.aircraft });
                        pfp_preview(ui, &custom::pfp_blocks(p, &snap, &cat));
                    });
                });
                ui.add_space(4.0);
            }
            if ui.button("+ Add a PFP page").clicked() {
                self.cfg.custom_screens.pfp.push(CustomPfp { aircraft: flying.clone(), ..Default::default() });
                changed = true;
            }
        });
        if let Some(i) = remove_p {
            self.cfg.custom_screens.pfp.remove(i);
            changed = true;
        }
        if changed {
            self.push_custom();
        }

        // ---- name browser
        card(ui, "DCS-BIOS names", |ui| {
            ui.horizontal(|ui| {
                ui.label("Aircraft");
                if self.names_aircraft.is_empty() && !flying.is_empty() {
                    self.names_aircraft = flying.clone();
                }
                aircraft_combo(ui, "namesac", &mut self.names_aircraft, &list, &flying);
                ui.label("Search");
                ui.add(egui::TextEdit::singleline(&mut self.names_search).hint_text("e.g. fuel, scratchpad, ifei").desired_width(220.0));
            });
            let cat = custom::catalog(&self.names_aircraft);
            if cat.is_empty() {
                hint(ui, "No names found. Is DCS-BIOS installed?");
                return;
            }
            let q = self.names_search.to_ascii_lowercase();
            let mut hits: Vec<&custom::Output> = cat
                .values()
                .filter(|o| q.is_empty() || o.name.to_ascii_lowercase().contains(&q) || o.description.to_ascii_lowercase().contains(&q) || o.category.to_ascii_lowercase().contains(&q))
                .collect();
            // displays and strings first, they are what screens usually want
            hits.sort_by_key(|o| (!matches!(o.kind, custom::Kind::Str { .. }), o.category.clone(), o.name.clone()));
            let total = hits.len();
            hint(ui, &format!("{total} names. Live values show while DCS runs that aircraft."));
            egui::Grid::new("names").striped(true).num_columns(4).spacing([12.0, 4.0]).show(ui, |ui| {
                for o in hits.iter().take(80) {
                    if ui.button("Copy").on_hover_text(format!("Copies {{{}}}", o.name)).clicked() {
                        ui.ctx().copy_text(format!("{{{}}}", o.name));
                        self.copied = Some((format!("name:{}", o.name), Instant::now()));
                    }
                    ui.label(RichText::new(&o.name).monospace());
                    ui.label(RichText::new(format!("{} · {}", o.category, o.description)).small().color(ui.visuals().weak_text_color()));
                    let live = if flying == self.names_aircraft || o.category == "Metadata" { custom::read(o, &snap) } else { String::new() };
                    let just = matches!(&self.copied, Some((k, t)) if *k == format!("name:{}", o.name) && t.elapsed().as_secs() < 3);
                    if just {
                        ui.colored_label(GREEN, "copied");
                    } else {
                        ui.label(RichText::new(live.trim_end()).monospace().color(ACCENT));
                    }
                    ui.end_row();
                }
            });
            if total > 80 {
                hint(ui, &format!("Showing 80 of {total}. Type more to narrow it down."));
            }
        });
    }

    // ------------------------------------------------------------------ Settings

    pub(super) fn ui_settings(&mut self, ui: &mut egui::Ui) {
        page_header(ui, "Settings", "Updates, and saving or restoring all of UWACS's settings.");

        // a file dialog finished
        let finished = self.dialog.lock().unwrap().take();
        if let Some((kind, path)) = finished {
            if let Some(p) = path {
                self.settings_msg = match kind {
                    DialogFor::Export => match export_to(&self.cfg, &p) {
                        Ok(()) => format!("Exported everything to {}", p.display()),
                        Err(e) => format!("Export failed: {e}"),
                    },
                    DialogFor::Import => match import_from(&p) {
                        Ok(c) => {
                            let backup = config::path().with_extension(format!("json.bak-import-{}", chrono_stamp()));
                            let _ = std::fs::copy(config::path(), &backup);
                            self.cfg = c;
                            *self.remaps.write().unwrap() = self.cfg.remaps.clone();
                            self.pfp_auto.store(self.cfg.pfp_auto_buttons, std::sync::atomic::Ordering::Relaxed);
                            self.push_custom();
                            format!("Imported {}. Your previous settings are saved as {}", p.display(), backup.display())
                        }
                        Err(e) => format!("Import failed: {e}"),
                    },
                };
                log::log(format!("Settings: {}", self.settings_msg));
            }
        }

        card(ui, "Updates", |ui| {
            ui.label(format!("This is UWACS {}.", env!("CARGO_PKG_VERSION")));
            if ui.checkbox(&mut self.cfg.auto_update, "Update automatically").on_hover_text("Checks GitHub shortly after start and every 6 hours. A new version is downloaded, tested and installed; it is used from the next start.").changed() {
                self.save_soon();
            }
            ui.horizontal(|ui| {
                let busy = matches!(update::state(), State::Checking | State::Downloading(_));
                if ui.add_enabled(!busy, egui::Button::new("Check now")).clicked() {
                    update::check_now(self.cfg.update_repo.clone(), true);
                }
                match update::state() {
                    State::Idle => hint(ui, if self.cfg.auto_update { "Not checked yet (checks 20 s after start)." } else { "Automatic updates are off." }),
                    State::Checking => {
                        ui.spinner();
                        ui.label("Checking…");
                    }
                    State::UpToDate(m) => {
                        ui.colored_label(GREEN, format!("Up to date ({m})."));
                    }
                    State::Downloading(t) => {
                        ui.spinner();
                        ui.label(format!("Downloading {t}…"));
                    }
                    State::Installed(t) => {
                        ui.colored_label(GREEN, format!("{t} is installed."));
                        if ui.button(RichText::new("Restart UWACS now").strong()).clicked() {
                            match update::restart() {
                                Ok(()) => ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close),
                                Err(e) => self.settings_msg = format!("Restart failed: {e}"),
                            }
                        }
                    }
                    State::Failed(e) => {
                        ui.colored_label(RED, e);
                    }
                }
            });
            egui::CollapsingHeader::new("Advanced: where updates come from").show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label("GitHub repository");
                    if ui.add(egui::TextEdit::singleline(&mut self.cfg.update_repo).desired_width(220.0)).changed() {
                        self.save_soon();
                    }
                });
                hint(ui, &format!("Releases tagged vX.Y.Z with a file called {} are used.", update::ASSET));
            });
        });

        card(ui, "Export and import", |ui| {
            ui.label("Export everything saves all UWACS settings into one file: button mappings and profiles, taught buttons, overlays, screen pages, DCS setup values and the rest. Import loads such a file again (your current settings are backed up first).");
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui.button(RichText::new("Export everything…").strong()).clicked() {
                    let default = dirs::home_dir().unwrap_or_default().join(format!("uwacs-settings-{}.json", chrono_stamp()));
                    self.open_dialog(DialogFor::Export, default);
                }
                if ui.button("Import…").clicked() {
                    self.open_dialog(DialogFor::Import, dirs::home_dir().unwrap_or_default());
                }
            });
            if !self.settings_msg.is_empty() {
                let col = if self.settings_msg.contains("failed") { RED } else { GREEN };
                ui.colored_label(col, &self.settings_msg);
            }
        });

        card(ui, "Settings file", |ui| {
            let p = config::path();
            ui.label(RichText::new(p.display().to_string()).monospace());
            if ui.button("Show in file manager").clicked() {
                let _ = std::process::Command::new("setsid").args(["-f", "dolphin", "--select"]).arg(&p).spawn();
            }
            hint(ui, "UWACS saves on its own a moment after every change.");
        });
    }

    fn open_dialog(&mut self, kind: DialogFor, start: std::path::PathBuf) {
        let slot = self.dialog.clone();
        std::thread::spawn(move || {
            let mut args = vec!["--file-selection".to_string()];
            match kind {
                DialogFor::Export => {
                    args.extend(["--save".into(), "--confirm-overwrite".into(), "--title=Export UWACS settings".into()]);
                }
                DialogFor::Import => {
                    args.extend(["--title=Import UWACS settings".into(), "--file-filter=UWACS settings | *.json".into()]);
                }
            }
            args.push(format!("--filename={}", start.display()));
            let out = std::process::Command::new("zenity").args(&args).output().ok();
            let p = out.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).filter(|s| !s.is_empty()).map(std::path::PathBuf::from);
            *slot.lock().unwrap() = Some((kind, p));
            if let Some(ctx) = crate::remap::EGUI_CTX.get() {
                ctx.request_repaint();
            }
        });
    }
}

/// yyyy-mm-dd_hhmmss from the local clock via `date` (no extra crate).
fn chrono_stamp() -> String {
    std::process::Command::new("date")
        .arg("+%Y-%m-%d_%H%M%S")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "export".into())
}

/// One self-describing file with every setting.
pub fn export_json(cfg: &Config) -> anyhow::Result<String> {
    let v = serde_json::json!({
        "format": "uwacs-settings",
        "uwacs_version": env!("CARGO_PKG_VERSION"),
        "exported": chrono_stamp(),
        "settings": cfg,
    });
    Ok(serde_json::to_string_pretty(&v)?)
}

fn export_to(cfg: &Config, p: &std::path::Path) -> anyhow::Result<()> {
    std::fs::write(p, export_json(cfg)?)?;
    Ok(())
}

/// Accepts an export file or a plain config.json.
pub fn import_json(text: &str) -> anyhow::Result<Config> {
    let v: serde_json::Value = serde_json::from_str(text)?;
    let inner = if v.get("format").and_then(|f| f.as_str()) == Some("uwacs-settings") { v["settings"].clone() } else { v };
    if !inner.is_object() || inner.get("remaps").is_none() && inner.get("overlays").is_none() && inner.get("dcsbios_port").is_none() {
        anyhow::bail!("this is not a UWACS settings file");
    }
    Ok(serde_json::from_value(inner)?)
}

fn import_from(p: &std::path::Path) -> anyhow::Result<Config> {
    import_json(&std::fs::read_to_string(p)?)
}

/// The UFC as UWACS will draw it.
fn ufc_preview(ui: &mut egui::Ui, pkg: &crate::ufc::UfcPackage) {
    let seg = Color32::from_rgb(110, 240, 120);
    egui::Frame::new().fill(Color32::from_gray(14)).stroke(egui::Stroke::new(1.0, Color32::from_gray(70))).corner_radius(6.0).inner_margin(egui::Margin::same(10)).show(ui, |ui| ui.vertical(|ui| {
        ui.label(RichText::new("Preview").small().color(GREY));
        let ch = |c: Option<&crate::ufc::CommsPackage>| c.map(|c| c.ch.to_string()).unwrap_or_default();
        let area: String = pkg.area_1.iter().collect();
        let (small, big) = area.split_at(area.char_indices().nth(2).map(|x| x.0).unwrap_or(area.len()));
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("[{}]", ch(pkg.comms.first()))).monospace().size(20.0).color(seg));
            ui.label(RichText::new(small).monospace().size(14.0).color(seg));
            ui.label(RichText::new(big).monospace().size(22.0).color(seg));
            ui.label(RichText::new(format!("[{}]", ch(pkg.comms.get(1)))).monospace().size(20.0).color(seg));
        });
        for o in &pkg.odu {
            ui.label(RichText::new(format!("{} {}", if o.is_selected { ":" } else { " " }, o.text)).monospace().size(18.0).color(seg));
        }
    }));
}

/// The PFP screen as UWACS will send it.
fn pfp_preview(ui: &mut egui::Ui, blocks: &[crate::pfp::TextBlock]) {
    egui::Frame::new().fill(Color32::from_gray(8)).stroke(egui::Stroke::new(1.0, Color32::from_gray(70))).corner_radius(6.0).inner_margin(egui::Margin::same(10)).show(ui, |ui| ui.vertical(|ui| {
        ui.label(RichText::new("Preview").small().color(GREY));
        ui.spacing_mut().item_spacing.y = 0.0;
        for b in blocks {
            ui.add(egui::Label::new(RichText::new(&b.text).monospace().size(13.0).color(pfp_color(b.fg))).extend());
        }
    }));
    let _ = Vec2::ZERO;
}

#[cfg(test)]
mod tests {
    #[test]
    fn export_import_round_trip() {
        let mut c = crate::config::Config::default();
        c.remaps.push(crate::config::Remap { device_key: "k".into(), device_name: "MFD".into(), usage: 33, label: "SYM+".into(), action: crate::config::Action::Keys("F13".into()), profile: Some(45) });
        c.custom_screens.pfp.push(Default::default());
        c.auto_update = false;
        let text = super::export_json(&c).unwrap();
        let back = super::import_json(&text).unwrap();
        assert_eq!(back.remaps.len(), 1);
        assert_eq!(back.remaps[0].profile, Some(45));
        assert_eq!(back.custom_screens.pfp.len(), 1);
        assert!(!back.auto_update);
        // a plain config.json is accepted too
        let plain = serde_json::to_string(&c).unwrap();
        assert_eq!(super::import_json(&plain).unwrap().remaps.len(), 1);
        assert!(super::import_json("{\"hello\": 1}").is_err());
    }
}
