//! Settings, saved as JSON in ~/.config/uwacs/config.json.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum Action {
    /// Keyboard shortcut such as "Ctrl+Shift+F13". Held for as long as the button is held.
    Keys(String),
    OverlayToggle,
    OverlayNextMonitor,
}

impl Action {
    pub fn describe(&self) -> String {
        match self {
            Action::Keys(k) => k.clone(),
            Action::OverlayToggle => "Show/hide MFD overlays".into(),
            Action::OverlayNextMonitor => "Move first MFD overlay to next screen".into(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Remap {
    pub device_key: String,
    pub device_name: String,
    pub usage: u32,
    pub label: String,
    pub action: Action,
    /// Only active while this button (a profile switch) is held on the same panel; None = always.
    #[serde(default)]
    pub profile: Option<u32>,
}

/// One overlay window. Position/size are expressions ("778/2", "W-768"), measured from the
/// top-left corner of the chosen screen to the top-left corner of the picture.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct OverlayCfg {
    pub name: String,
    pub enabled: bool,
    /// Output name; empty = the first WinWing (USB) display, else the first screen.
    pub output: String,
    pub x: String,
    pub y: String,
    pub width: String,
    pub height: String,
    pub image: String,
    pub click_through: bool,
}

impl Default for OverlayCfg {
    fn default() -> Self {
        // placement measured on a WinWing instrument display with the AH-64D frame (2026-11-17)
        Self {
            name: "MFD frame".into(),
            enabled: true,
            output: String::new(),
            x: "9".into(),
            y: "255".into(),
            width: "747".into(),
            height: "762".into(),
            image: String::new(),
            click_through: true,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub dcsbios_host: String,
    pub dcsbios_port: u16,
    pub ufc_autostart: bool,
    pub pfp_autostart: bool,
    pub pfp_auto_buttons: bool,
    pub remaps: Vec<Remap>,
    pub overlays: Vec<OverlayCfg>,
    /// Teach-mode corrections: device type -> control id -> HID button number.
    pub button_overrides: std::collections::BTreeMap<String, std::collections::BTreeMap<String, u32>>,
    /// Extra profile switches the user marked: device type -> control ids.
    pub profile_switches: std::collections::BTreeMap<String, std::collections::BTreeSet<String>>,
    pub monitor_setup: crate::monitorsetup::MonitorSetupCfg,
    /// Check GitHub for new versions and install them (used from the next start).
    pub auto_update: bool,
    /// GitHub "owner/repo" the updates come from.
    pub update_repo: String,
    pub custom_screens: crate::custom::CustomScreens,
    /// DCS folder given to the one-click setup (prefix, game folder or Saved Games/DCS).
    pub dcs_folder: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            dcsbios_host: "127.0.0.1".into(),
            dcsbios_port: 7778,
            ufc_autostart: true,
            pfp_autostart: true,
            pfp_auto_buttons: true,
            remaps: Vec::new(),
            overlays: vec![OverlayCfg::default()],
            button_overrides: Default::default(),
            profile_switches: Default::default(),
            monitor_setup: Default::default(),
            auto_update: true,
            update_repo: crate::update::DEFAULT_REPO.into(),
            custom_screens: Default::default(),
            dcs_folder: String::new(),
        }
    }
}

pub fn path() -> PathBuf {
    if let Ok(p) = std::env::var("UWACS_CONFIG") {
        return PathBuf::from(p);
    }
    dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")).join("uwacs").join("config.json")
}

pub fn load() -> Config {
    let Some(text) = std::fs::read_to_string(path()).ok() else { return Config::default() };
    let mut c: Config = serde_json::from_str(&text).unwrap_or_default();
    // v0.2 had a single "overlay" with numbers; carry it over once.
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
        if v.get("overlays").is_none() {
            if let Some(o) = v.get("overlay") {
                let n = |k: &str, d: &str| o.get(k).map(|x| x.to_string().trim_matches('"').to_string()).unwrap_or_else(|| d.to_string());
                let mut ov = OverlayCfg::default();
                ov.output = n("output", "");
                ov.x = n("x", "9");
                ov.y = n("y", "255");
                ov.width = n("width", "747");
                ov.height = n("height", "762");
                ov.image = n("image", "");
                c.overlays = vec![ov];
            }
        }
    }
    c
}

pub fn save(c: &Config) -> anyhow::Result<()> {
    let p = path();
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d)?;
    }
    let tmp = p.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(c)?)?;
    std::fs::rename(tmp, p)?;
    Ok(())
}
