//! Monitor layout and rotation/flip through KDE's kscreen-doctor (Plasma on Wayland or X11).

use serde_json::Value;
use std::process::Command;

#[derive(Clone, Debug, PartialEq)]
pub struct Output {
    pub name: String,
    pub enabled: bool,
    pub x: i32,
    pub y: i32,
    /// Current mode size in pixels, before rotation.
    pub mode_w: u32,
    pub mode_h: u32,
    pub scale: f32,
    pub rotation: Rotation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rotation {
    None,
    Left,
    Inverted,
    Right,
    Flipped,
    Flipped90,
    Flipped180,
    Flipped270,
}

impl Rotation {
    pub const ALL: [Rotation; 8] = [
        Rotation::None,
        Rotation::Left,
        Rotation::Inverted,
        Rotation::Right,
        Rotation::Flipped,
        Rotation::Flipped90,
        Rotation::Flipped180,
        Rotation::Flipped270,
    ];

    fn from_kscreen(v: u64) -> Self {
        match v {
            2 => Rotation::Left,
            4 => Rotation::Inverted,
            8 => Rotation::Right,
            16 => Rotation::Flipped,
            32 => Rotation::Flipped90,
            64 => Rotation::Flipped180,
            128 => Rotation::Flipped270,
            _ => Rotation::None,
        }
    }

    pub fn arg(self) -> &'static str {
        match self {
            Rotation::None => "none",
            Rotation::Left => "left",
            Rotation::Inverted => "inverted",
            Rotation::Right => "right",
            Rotation::Flipped => "flipped",
            Rotation::Flipped90 => "flipped90",
            Rotation::Flipped180 => "flipped180",
            Rotation::Flipped270 => "flipped270",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Rotation::None => "Normal",
            Rotation::Left => "Rotate 90° left",
            Rotation::Inverted => "Upside down (180°)",
            Rotation::Right => "Rotate 90° right",
            Rotation::Flipped => "Mirrored",
            Rotation::Flipped90 => "Mirrored + 90°",
            Rotation::Flipped180 => "Mirrored + 180° (flipped vertically)",
            Rotation::Flipped270 => "Mirrored + 270°",
        }
    }

    pub fn sideways(self) -> bool {
        matches!(self, Rotation::Left | Rotation::Right | Rotation::Flipped90 | Rotation::Flipped270)
    }
}

impl Output {
    /// Size on the desktop in logical pixels (after rotation and scale).
    pub fn logical_size(&self) -> (f32, f32) {
        let (w, h) = if self.rotation.sideways() { (self.mode_h, self.mode_w) } else { (self.mode_w, self.mode_h) };
        (w as f32 / self.scale, h as f32 / self.scale)
    }

    /// DisplayLink outputs come from the evdi driver and show up as DVI-I-n.
    pub fn is_usb_display(&self) -> bool {
        self.name.starts_with("DVI-I-")
    }
}

/// Run a command but give up after `secs` (kscreen-doctor hangs forever outside KDE).
pub fn run_timeout(cmd: &str, args: &[String], secs: u64) -> anyhow::Result<std::process::Output> {
    use std::process::Stdio;
    let mut child = Command::new(cmd).args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(secs);
    loop {
        if child.try_wait()?.is_some() {
            return Ok(child.wait_with_output()?);
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!("{cmd} did not answer within {secs} s (is this a KDE Plasma session?)");
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

pub fn read() -> anyhow::Result<Vec<Output>> {
    let o = run_timeout("kscreen-doctor", &["-j".into()], 4)?;
    if !o.status.success() {
        anyhow::bail!("kscreen-doctor -j failed: {}", String::from_utf8_lossy(&o.stderr));
    }
    parse(&String::from_utf8_lossy(&o.stdout))
}

pub fn parse(json: &str) -> anyhow::Result<Vec<Output>> {
    let v: Value = serde_json::from_str(json)?;
    let mut out = Vec::new();
    for o in v["outputs"].as_array().cloned().unwrap_or_default() {
        if !o["connected"].as_bool().unwrap_or(false) {
            continue;
        }
        out.push(Output {
            name: o["name"].as_str().unwrap_or("?").to_string(),
            enabled: o["enabled"].as_bool().unwrap_or(false),
            x: o["pos"]["x"].as_i64().unwrap_or(0) as i32,
            y: o["pos"]["y"].as_i64().unwrap_or(0) as i32,
            mode_w: o["size"]["width"].as_u64().unwrap_or(0) as u32,
            mode_h: o["size"]["height"].as_u64().unwrap_or(0) as u32,
            scale: o["scale"].as_f64().unwrap_or(1.0) as f32,
            rotation: Rotation::from_kscreen(o["rotation"].as_u64().unwrap_or(1)),
        });
    }
    out.sort_by_key(|o| (o.y, o.x));
    Ok(out)
}

/// kscreen-doctor arguments that turn `old` into `new` (only what changed).
pub fn diff_args(old: &[Output], new: &[Output]) -> Vec<String> {
    let mut args = Vec::new();
    for n in new {
        let o = old.iter().find(|o| o.name == n.name);
        if o.map(|o| o.enabled != n.enabled).unwrap_or(true) {
            args.push(format!("output.{}.{}", n.name, if n.enabled { "enable" } else { "disable" }));
        }
        if !n.enabled {
            continue;
        }
        if o.map(|o| o.rotation != n.rotation).unwrap_or(true) {
            args.push(format!("output.{}.rotation.{}", n.name, n.rotation.arg()));
        }
        if o.map(|o| (o.x, o.y) != (n.x, n.y)).unwrap_or(true) {
            args.push(format!("output.{}.position.{},{}", n.name, n.x, n.y));
        }
    }
    args
}

pub fn apply(args: &[String]) -> anyhow::Result<String> {
    if args.is_empty() {
        return Ok("nothing to change".into());
    }
    let o = run_timeout("kscreen-doctor", args, 10)?;
    let msg = format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)).trim().to_string();
    if !o.status.success() {
        anyhow::bail!("kscreen-doctor failed: {msg}");
    }
    Ok(if msg.is_empty() { "applied".into() } else { msg })
}

/// Move outputs so the layout has no gaps at the top-left (KDE wants the union to start at 0,0).
pub fn normalize(outs: &mut [Output]) {
    let en: Vec<&Output> = outs.iter().filter(|o| o.enabled).collect();
    if en.is_empty() {
        return;
    }
    let minx = en.iter().map(|o| o.x).min().unwrap();
    let miny = en.iter().map(|o| o.y).min().unwrap();
    for o in outs.iter_mut() {
        o.x -= minx;
        o.y -= miny;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{"outputs":[
      {"name":"DVI-I-1","enabled":true,"connected":true,"pos":{"x":4567,"y":0},"size":{"width":1024,"height":768},"rotation":1,"scale":1},
      {"name":"DP-3","enabled":true,"connected":true,"pos":{"x":0,"y":0},"size":{"width":3440,"height":1440},"rotation":1,"scale":1.3},
      {"name":"HDMI-A-1","enabled":false,"connected":false,"pos":{"x":0,"y":0},"size":{"width":0,"height":0},"rotation":1,"scale":1}
    ]}"#;

    #[test]
    fn parse_and_diff() {
        let outs = parse(SAMPLE).unwrap();
        assert_eq!(outs.len(), 2);
        assert_eq!(outs[0].name, "DP-3");
        let (w, _) = outs[0].logical_size();
        assert!((w - 2646.15).abs() < 0.1);
        assert!(outs[1].is_usb_display());
        let mut new = outs.clone();
        new[1].rotation = Rotation::Inverted;
        new[1].x = 100;
        assert_eq!(diff_args(&outs, &new), vec!["output.DVI-I-1.rotation.inverted", "output.DVI-I-1.position.100,0"]);
        new[1].rotation = Rotation::Left;
        assert_eq!(new[1].logical_size(), (768.0, 1024.0));
    }
}
