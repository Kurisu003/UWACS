//! DCS MonitorSetup generator (FluidPort shipped this as a hand-written mine.lua).
//! Main view on top, MFD exports in a row below it, per-aircraft like the original.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct MonitorSetupCfg {
    pub name: String,
    pub main_w: u32,
    pub main_h: u32,
    /// Top of the MFD row (DCS canvas pixels).
    pub mfd_y: i32,
    pub mfd_x: i32,
    pub outer_w: u32,
    pub outer_h: u32,
    /// The AH-64D exports only the inner screen area, inset by this much.
    pub inner_dx: i32,
    pub inner_dy: i32,
    pub inner_w: u32,
    pub inner_h: u32,
    pub save_path: String,
    /// Put the MFD exports where the MFD overlays are (AH-64D inset into the frame window).
    pub follow_overlays: bool,
    /// Where the DCS window's top-left corner is on the desktop (subtracted from overlay positions).
    pub dcs_origin_x: i32,
    pub dcs_origin_y: i32,
}

/// One export area taken from an overlay, in DCS window pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct Slot {
    pub outer: (i32, i32, i32, i32),
    /// The frame's see-through window, if the overlay picture has one.
    pub inner: Option<(i32, i32, i32, i32)>,
}

impl Default for MonitorSetupCfg {
    fn default() -> Self {
        // Values from FluidPort's mine.lua.
        Self {
            name: "UWACS".into(),
            main_w: 3440,
            main_h: 1686,
            mfd_y: 1686,
            mfd_x: 0,
            outer_w: 768,
            outer_h: 778,
            inner_dx: 65,
            inner_dy: 65,
            inner_w: 638,
            inner_h: 648,
            save_path: default_path().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(),
            follow_overlays: true,
            dcs_origin_x: 0,
            dcs_origin_y: 0,
        }
    }
}

/// DCS folder the user gave the one-click setup (any form resolve_saved_games accepts).
pub static DCS_FOLDER: std::sync::RwLock<String> = std::sync::RwLock::new(String::new());

/// The DCS "Saved Games/DCS" folder: the remembered one, else the first Wine/Proton prefix we find.
pub fn saved_games_dir() -> Option<PathBuf> {
    let remembered = DCS_FOLDER.read().unwrap().clone();
    if !remembered.trim().is_empty() {
        if let Ok(p) = crate::dcssetup::resolve_saved_games(std::path::Path::new(remembered.trim())) {
            return Some(p);
        }
    }
    let home = dirs::home_dir()?;
    let mut roots: Vec<PathBuf> = Vec::new();
    roots.push(home.join(".local/share/Steam/steamapps/compatdata/223750/pfx"));
    if let Ok(rd) = std::fs::read_dir(home.join("Games")) {
        roots.extend(rd.flatten().map(|e| e.path()));
    }
    for root in roots {
        let users = root.join("drive_c/users");
        let Ok(rd) = std::fs::read_dir(&users) else { continue };
        let mut names: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
        names.sort_by_key(|p| p.ends_with("steamuser"));
        for u in names {
            let sg = u.join("Saved Games");
            let Ok(rd) = std::fs::read_dir(&sg) else { continue };
            let mut dcs: Vec<PathBuf> = rd
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.file_name().map(|n| n.to_string_lossy().starts_with("DCS")).unwrap_or(false))
                .collect();
            dcs.sort();
            if let Some(d) = dcs.into_iter().find(|d| d.join("Config").is_dir()) {
                return Some(d);
            }
        }
    }
    None
}

/// Saved Games/DCS/Config/MonitorSetup/UWACS.lua
pub fn default_path() -> Option<PathBuf> {
    saved_games_dir().map(|d| d.join("Config/MonitorSetup/UWACS.lua"))
}

/// Some(true) if Saved Games/DCS/Scripts/Export.lua loads DCS-BIOS, None if no DCS folder found.
pub fn dcsbios_installed() -> Option<bool> {
    let d = saved_games_dir()?;
    let export = std::fs::read_to_string(d.join("Scripts/Export.lua")).unwrap_or_default().to_ascii_lowercase();
    Some(export.contains("dcs-bios") || export.contains("dcsbios") || d.join("Scripts/DCS-BIOS").is_dir())
}

pub fn generate(c: &MonitorSetupCfg, slots: &[Slot]) -> String {
    let fmt = |(x, y, w, h): (i32, i32, i32, i32)| format!("{{ x = {x}, y = {y}, width = {w}, height = {h} }}");
    let row_outer = |i: i32| (c.mfd_x + i * c.outer_w as i32, c.mfd_y, c.outer_w as i32, c.outer_h as i32);
    let row_inner = |i: i32| {
        let (x, y, _, _) = row_outer(i);
        (x + c.inner_dx, y + c.inner_dy, c.inner_w as i32, c.inner_h as i32)
    };
    let use_slots = c.follow_overlays && !slots.is_empty();
    let outer = |i: i32| match slots.get(i as usize) {
        Some(s) if use_slots => fmt(s.outer),
        _ => fmt(row_outer(i)),
    };
    let inner = |i: i32| match slots.get(i as usize) {
        Some(s) if use_slots => fmt(s.inner.unwrap_or(s.outer)),
        _ => fmt(row_inner(i)),
    };
    let ah64_tedac = if use_slots && slots.len() >= 3 { inner(2) } else { outer(2) };
    let name = c.name.replace(['\'', '"', '\\'], "");
    format!(
        r#"_ = function(p) return p end
name = _('{name}')
description = 'Generated by UWACS: main view {mw}x{mh}, MFD exports below it'

function reconfigure_for_unit(unit_type)
  if unit_type == "AH-64D_BLK_II" then
    LEFT_MFCD = {i0}
    RIGHT_MFCD = {i1}
    TEDAC = {tedac}
    return
  end
  if unit_type == "AV8BNA" then
    LEFT_MFCD = {o0}
    RIGHT_MFCD = {o1}
    return
  end
  if unit_type == "FA-18C_hornet" then
    CENTER_MFCD = {o0}
    LEFT_MFCD = {o1}
    RIGHT_MFCD = {o2}
    return
  end
  if unit_type == "MiG-21Bis" then
    RADAR = {o0}
    MIG21_RADAR = {o1}
    return
  end
  if unit_type == "SA342M" or unit_type == "SA342L" or unit_type == "SA342Mistral" then
    NADIR_SCREEN = {o0}
    RWR = {o1}
    SA342_NADIR_SCREEN = {o0}
    SA342_RWR = {o1}
    return
  end
  if unit_type == "AJS37" then
    VIGGEN_TV = {o0}
    TV = {o0}
    return
  end
  -- everything else
  LEFT_MFCD = {o0}
  RIGHT_MFCD = {o1}
end

Viewports = {{
  Center = {{
    x = 0,
    y = 0,
    width = {mw},
    height = {mh},
    aspect = {mw}/{mh},
    dx = 0,
    dy = 0
  }}
}}
UIMainView = Viewports.Center
GU_MAIN_VIEWPORT = Viewports.Center
"#,
        mw = c.main_w,
        mh = c.main_h,
        o0 = outer(0),
        o1 = outer(1),
        o2 = outer(2),
        i0 = inner(0),
        i1 = inner(1),
        tedac = ah64_tedac,
    )
}

pub fn write(c: &MonitorSetupCfg, slots: &[Slot]) -> anyhow::Result<PathBuf> {
    let p = PathBuf::from(&c.save_path);
    if c.save_path.is_empty() {
        anyhow::bail!("no save path set");
    }
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::write(&p, generate(c, slots))?;
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_match_fluidport() {
        let s = generate(&MonitorSetupCfg::default(), &[]);
        // same numbers mine.lua used
        assert!(s.contains("LEFT_MFCD = { x = 65, y = 1751, width = 638, height = 648 }"));
        assert!(s.contains("RIGHT_MFCD = { x = 833, y = 1751, width = 638, height = 648 }"));
        assert!(s.contains("TEDAC = { x = 1536, y = 1686, width = 768, height = 778 }"));
        assert!(s.contains("width = 3440,"));
    }

    #[test]
    fn follows_overlays() {
        let slots = [
            Slot { outer: (4576, 255, 747, 762), inner: Some((4639, 319, 621, 635)) },
            Slot { outer: (10, 20, 30, 40), inner: None },
        ];
        let s = generate(&MonitorSetupCfg::default(), &slots);
        assert!(s.contains("LEFT_MFCD = { x = 4639, y = 319, width = 621, height = 635 }"));
        assert!(s.contains("RIGHT_MFCD = { x = 10, y = 20, width = 30, height = 40 }"));
        // only two overlays: TEDAC falls back to the row layout
        assert!(s.contains("TEDAC = { x = 1536, y = 1686, width = 768, height = 778 }"));
        // other aircraft get the full overlay area
        assert!(s.contains("CENTER_MFCD = { x = 4576, y = 255, width = 747, height = 762 }"));
    }
}
