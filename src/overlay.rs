//! FluidPort overlay: an always-on-top, borderless, click-through image window on a chosen monitor
//! (by default the AH-64D MFD leader-line frame). On Wayland a client cannot place its own window,
//! so on KDE we ask KWin through a tiny script, same trick the heyclaude status window uses.

use crate::displays::run_timeout;

pub const APP_ID: &str = "uwacs-overlay";

/// Window class of overlay number `i` (each needs its own so KWin can place it).
pub fn app_id(i: usize) -> String {
    format!("{APP_ID}-{i}")
}

/// The transparent window in the middle of a frame picture, in picture pixels (x, y, w, h).
/// Found by walking out from the centre until the first non-transparent pixel on each side.
pub fn detect_hole(img: &egui::ColorImage) -> Option<(usize, usize, usize, usize)> {
    let [w, h] = img.size;
    if w < 3 || h < 3 {
        return None;
    }
    let a = |x: usize, y: usize| img.pixels[y * w + x].a();
    let (cx, cy) = (w / 2, h / 2);
    if a(cx, cy) > 8 {
        return None;
    }
    let mut l = cx;
    while l > 0 && a(l - 1, cy) <= 8 {
        l -= 1;
    }
    let mut r = cx;
    while r + 1 < w && a(r + 1, cy) <= 8 {
        r += 1;
    }
    let mut t = cy;
    while t > 0 && a(cx, t - 1) <= 8 {
        t -= 1;
    }
    let mut b = cy;
    while b + 1 < h && a(cx, b + 1) <= 8 {
        b += 1;
    }
    // a hole touching the picture edge is not a frame window
    if l == 0 || t == 0 || r + 1 == w || b + 1 == h {
        return None;
    }
    Some((l, t, r - l + 1, b - t + 1))
}
pub const DEFAULT_IMAGE: &[u8] = include_bytes!("../assets/AH-64D_MFCD_leaderLine.png");

pub fn load_image(path: &str) -> anyhow::Result<egui::ColorImage> {
    let bytes = if path.is_empty() { DEFAULT_IMAGE.to_vec() } else { std::fs::read(path)? };
    let img = image::load_from_memory(&bytes)?.to_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    Ok(egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw()))
}

/// Ask KWin to put the overlay at (x, y) on `output`, keep it above, without border.
pub fn place_with_kwin(class: &str, output: &str, x: i32, y: i32, w: u32, h: u32) -> anyhow::Result<()> {
    let script = format!(
        r#"const want = {out};
const s = workspace.screens.find(s => s.name === want) || workspace.screens[0];
for (const w of workspace.windowList()) {{
    if (w.resourceClass !== {cls} && w.resourceName !== {cls}) continue;
    w.noBorder = true;
    w.keepAbove = true;
    w.skipTaskbar = true;
    w.skipPager = true;
    w.skipSwitcher = true;
    w.onAllDesktops = true;
    if (w.output !== s) workspace.sendClientToScreen(w, s);
    w.frameGeometry = {{ x: s.geometry.x + {x}, y: s.geometry.y + {y}, width: {w}, height: {h} }};
}}
"#,
        out = serde_json::to_string(output)?,
        cls = serde_json::to_string(class)?,
    );
    let dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
    let path = format!("{dir}/{class}-place.js");
    std::fs::write(&path, script)?;
    let q = qdbus();
    let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<String>>();
    let id = run_timeout(&q, &a(&["org.kde.KWin", "/Scripting", "org.kde.kwin.Scripting.loadScript", &path, class]), 4)?;
    let sid = String::from_utf8_lossy(&id.stdout).trim().to_string();
    if let Ok(n) = sid.parse::<i64>() {
        if n >= 0 {
            let _ = run_timeout(&q, &a(&["org.kde.KWin", &format!("/Scripting/Script{n}"), "org.kde.kwin.Script.run"]), 4);
        }
    }
    let _ = run_timeout(&q, &a(&["org.kde.KWin", "/Scripting", "org.kde.kwin.Scripting.unloadScript", class]), 4);
    if sid.is_empty() {
        anyhow::bail!("KWin scripting not reachable (not a KDE session?)");
    }
    Ok(())
}

fn qdbus() -> String {
    for c in ["qdbus6", "qdbus", "qdbus-qt6"] {
        if run_timeout(c, &["--version".to_string()], 3).is_ok() {
            return c.to_string();
        }
    }
    "qdbus".into()
}

#[cfg(test)]
mod tests {
    #[test]
    fn ah64_frame_hole_matches_fluidport() {
        let img = super::load_image("").unwrap();
        // FluidPort's mine.lua used x 65, y 65, 638 x 648 for the AH-64D inner screen
        assert_eq!(super::detect_hole(&img), Some((65, 65, 638, 648)));
    }
}
