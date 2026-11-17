//! One-click DCS setup: find the DCS "Saved Games" folder from whatever folder the user points at,
//! download and install the latest DCS-BIOS, hook it into Export.lua, write the monitor setup.
//! Everything it replaces is kept as a dated backup next to the original.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::log::log;
use crate::monitorsetup::{self, MonitorSetupCfg, Slot};

const RELEASES_API: &str = "https://api.github.com/repos/DCS-Skunkworks/dcs-bios/releases/latest";
pub const EXPORT_LINE: &str = r"dofile(lfs.writedir() .. [[Scripts\DCS-BIOS\BIOS.lua]])";

#[derive(Clone, Debug, PartialEq)]
pub enum StepState {
    Running,
    Done(String),
    Failed(String),
    Skipped(String),
}

#[derive(Clone, Debug)]
pub struct Step {
    pub title: String,
    pub state: StepState,
}

#[derive(Default)]
pub struct Progress {
    pub steps: Vec<Step>,
    pub finished: bool,
}

pub type Shared = Arc<Mutex<Progress>>;

fn begin(p: &Shared, title: &str) {
    p.lock().unwrap().steps.push(Step { title: title.into(), state: StepState::Running });
}

fn end(p: &Shared, state: StepState) {
    let mut g = p.lock().unwrap();
    if let Some(s) = g.steps.last_mut() {
        log(format!("DCS setup: {} -> {:?}", s.title, state));
        s.state = state;
    }
}

fn stamp() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    format!("uwacs-backup-{secs}")
}

fn is_dcs_saved_games(p: &Path) -> bool {
    p.file_name().map(|n| n.to_string_lossy().starts_with("DCS")).unwrap_or(false) && p.join("Config").is_dir()
}

fn dcs_dirs_in(saved_games: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(saved_games)
        .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| is_dcs_saved_games(p)).collect())
        .unwrap_or_default();
    // "DCS" before "DCS.openbeta"
    v.sort_by_key(|p| p.file_name().map(|n| n.len()).unwrap_or(0));
    v
}

/// Accepts the game folder, the Wine/Proton prefix, drive_c, "Saved Games" or "Saved Games/DCS"
/// and returns Saved Games/DCS (the folder DCS calls lfs.writedir()).
pub fn resolve_saved_games(input: &Path) -> Result<PathBuf, String> {
    if is_dcs_saved_games(input) {
        return Ok(input.to_path_buf());
    }
    if input.file_name().map(|n| n == "Saved Games").unwrap_or(false) {
        if let Some(d) = dcs_dirs_in(input).into_iter().next() {
            return Ok(d);
        }
    }
    // find drive_c: the folder itself, a child, or an ancestor
    let mut drive_c = None;
    if input.join("drive_c").is_dir() {
        drive_c = Some(input.join("drive_c"));
    } else if input.join("pfx/drive_c").is_dir() {
        drive_c = Some(input.join("pfx/drive_c"));
    } else {
        for a in input.ancestors() {
            if a.file_name().map(|n| n == "drive_c").unwrap_or(false) {
                drive_c = Some(a.to_path_buf());
                break;
            }
        }
    }
    let Some(drive_c) = drive_c else {
        return Err(format!(
            "{} is not inside a Wine/Proton prefix. Pick the DCS game folder, the prefix folder, or Saved Games/DCS.",
            input.display()
        ));
    };
    let mut candidates = Vec::new();
    if let Ok(rd) = std::fs::read_dir(drive_c.join("users")) {
        for u in rd.flatten() {
            candidates.extend(dcs_dirs_in(&u.path().join("Saved Games")));
        }
    }
    let beta = input.to_string_lossy().to_ascii_lowercase().contains("openbeta");
    candidates.sort_by_key(|p| {
        let n = p.file_name().map(|n| n.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        (beta != n.contains("openbeta"), p.to_string_lossy().contains("/Public/"), n.len())
    });
    candidates.dedup_by(|a, b| std::fs::canonicalize(&a).ok() == std::fs::canonicalize(&b).ok());
    candidates.into_iter().next().ok_or_else(|| {
        format!("Found the prefix {} but no Saved Games/DCS folder in it. Start DCS once so it creates its settings, then try again.", drive_c.display())
    })
}

/// Read the latest release: (tag, zip download url).
fn latest_release() -> anyhow::Result<(String, String)> {
    let v: serde_json::Value = ureq::get(RELEASES_API).header("User-Agent", "uwacs").header("Accept", "application/vnd.github+json").call()?.body_mut().read_json()?;
    let tag = v["tag_name"].as_str().unwrap_or("?").to_string();
    let url = v["assets"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| Some((a["name"].as_str()?, a["browser_download_url"].as_str()?)))
        .find(|(n, _)| n.starts_with("DCS-BIOS") && n.ends_with(".zip"))
        .map(|(_, u)| u.to_string())
        .ok_or_else(|| anyhow::anyhow!("release {tag} has no DCS-BIOS zip"))?;
    Ok((tag, url))
}

fn download(url: &str) -> anyhow::Result<Vec<u8>> {
    let mut buf = Vec::new();
    ureq::get(url).header("User-Agent", "uwacs").call()?.body_mut().as_reader().take(200_000_000).read_to_end(&mut buf)?;
    Ok(buf)
}

/// Extract the DCS-BIOS/ folder of the release zip into `scripts`.
pub fn install_zip(zip_bytes: &[u8], scripts: &Path) -> anyhow::Result<usize> {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(zip_bytes))?;
    let target = scripts.join("DCS-BIOS");
    if target.exists() {
        let bak = scripts.join(format!("DCS-BIOS.{}", stamp()));
        std::fs::rename(&target, &bak)?;
        log(format!("DCS setup: old DCS-BIOS moved to {}", bak.display()));
    }
    let mut n = 0;
    for i in 0..z.len() {
        let mut f = z.by_index(i)?;
        let Some(rel) = f.enclosed_name() else { continue };
        if !rel.starts_with("DCS-BIOS") {
            continue;
        }
        let out = scripts.join(&rel);
        if f.is_dir() {
            std::fs::create_dir_all(&out)?;
        } else {
            if let Some(d) = out.parent() {
                std::fs::create_dir_all(d)?;
            }
            let mut data = Vec::with_capacity(f.size() as usize);
            f.read_to_end(&mut data)?;
            std::fs::write(&out, data)?;
            n += 1;
        }
    }
    if !target.join("BIOS.lua").is_file() {
        anyhow::bail!("the zip did not contain DCS-BIOS/BIOS.lua");
    }
    Ok(n)
}

/// Make sure Export.lua loads DCS-BIOS. Returns what was done.
pub fn hook_export_lua(scripts: &Path) -> anyhow::Result<String> {
    let p = scripts.join("Export.lua");
    let old = std::fs::read_to_string(&p).unwrap_or_default();
    let low = old.to_ascii_lowercase();
    if low.contains("dcs-bios\\bios.lua") || low.contains("dcs-bios/bios.lua") {
        return Ok("already loads DCS-BIOS, left unchanged".into());
    }
    if p.exists() {
        std::fs::copy(&p, scripts.join(format!("Export.lua.{}", stamp())))?;
    }
    let mut new = old.clone();
    if !new.is_empty() && !new.ends_with('\n') {
        new.push('\n');
    }
    new.push_str(EXPORT_LINE);
    new.push('\n');
    std::fs::write(&p, new)?;
    Ok(if old.is_empty() { "created Export.lua".into() } else { "added the DCS-BIOS line (old file backed up)".into() })
}

pub struct Options {
    pub install_bios: bool,
    pub write_monitor_setup: bool,
}

/// Runs every step, reporting into `p`. Meant for a background thread.
pub fn run(input: PathBuf, opts: Options, mut ms: MonitorSetupCfg, slots: Vec<Slot>, p: Shared) {
    begin(&p, "Find the DCS settings folder (Saved Games/DCS)");
    let saved = match resolve_saved_games(&input) {
        Ok(s) => {
            end(&p, StepState::Done(s.display().to_string()));
            s
        }
        Err(e) => {
            end(&p, StepState::Failed(e));
            p.lock().unwrap().finished = true;
            return;
        }
    };
    let scripts = saved.join("Scripts");

    if opts.install_bios {
        begin(&p, "Look up the newest DCS-BIOS release");
        match latest_release() {
            Ok((tag, url)) => {
                end(&p, StepState::Done(tag.clone()));
                begin(&p, &format!("Download DCS-BIOS {tag}"));
                match download(&url) {
                    Ok(bytes) => {
                        end(&p, StepState::Done(format!("{:.1} MB", bytes.len() as f64 / 1e6)));
                        begin(&p, "Install it into Scripts/DCS-BIOS");
                        let r = std::fs::create_dir_all(&scripts).map_err(anyhow::Error::from).and_then(|_| install_zip(&bytes, &scripts));
                        match r {
                            Ok(n) => end(&p, StepState::Done(format!("{n} files"))),
                            Err(e) => end(&p, StepState::Failed(e.to_string())),
                        }
                    }
                    Err(e) => end(&p, StepState::Failed(e.to_string())),
                }
            }
            Err(e) => end(&p, StepState::Failed(format!("{e} (is the internet reachable?)"))),
        }
        begin(&p, "Load DCS-BIOS from Scripts/Export.lua");
        match hook_export_lua(&scripts) {
            Ok(m) => end(&p, StepState::Done(m)),
            Err(e) => end(&p, StepState::Failed(e.to_string())),
        }
    } else {
        begin(&p, "Install DCS-BIOS");
        end(&p, StepState::Skipped("not selected".into()));
    }

    if opts.write_monitor_setup {
        begin(&p, "Write the MFD export monitor setup");
        ms.save_path = saved.join(format!("Config/MonitorSetup/{}.lua", if ms.name.trim().is_empty() { "UWACS" } else { ms.name.trim() })).to_string_lossy().into_owned();
        match monitorsetup::write(&ms, &slots) {
            Ok(path) => end(&p, StepState::Done(format!("{} (choose \"{}\" in DCS: Options > System > Monitors)", path.display(), ms.name))),
            Err(e) => end(&p, StepState::Failed(e.to_string())),
        }
    }
    p.lock().unwrap().finished = true;
}

/// Native folder picker (zenity, which KDE also has); None if cancelled or unavailable.
pub fn pick_folder(start: &str) -> Option<PathBuf> {
    let mut args = vec!["--file-selection".to_string(), "--directory".to_string(), "--title=Pick your DCS folder (game, prefix or Saved Games)".to_string()];
    if !start.is_empty() {
        args.push(format!("--filename={}/", start.trim_end_matches('/')));
    }
    let o = std::process::Command::new("zenity").args(&args).output().ok()?;
    let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
    (!s.is_empty()).then(|| PathBuf::from(s))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_prefix(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("uwacs-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("drive_c/users/steamuser/Saved Games/DCS/Config")).unwrap();
        std::fs::create_dir_all(root.join("drive_c/Program Files/Eagle Dynamics/DCS World/bin")).unwrap();
        root
    }

    #[test]
    fn resolves_from_many_starting_points() {
        let root = fake_prefix("resolve");
        let want = root.join("drive_c/users/steamuser/Saved Games/DCS");
        assert_eq!(resolve_saved_games(&root).unwrap(), want);
        assert_eq!(resolve_saved_games(&root.join("drive_c/Program Files/Eagle Dynamics/DCS World")).unwrap(), want);
        assert_eq!(resolve_saved_games(&want).unwrap(), want);
        assert_eq!(resolve_saved_games(&root.join("drive_c/users/steamuser/Saved Games")).unwrap(), want);
        assert!(resolve_saved_games(Path::new("/usr/share")).is_err());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn export_lua_hook_is_idempotent() {
        let root = fake_prefix("export");
        let scripts = root.join("drive_c/users/steamuser/Saved Games/DCS/Scripts");
        std::fs::create_dir_all(&scripts).unwrap();
        std::fs::write(scripts.join("Export.lua"), "-- TacView\nlocal x = 1").unwrap();
        hook_export_lua(&scripts).unwrap();
        let once = std::fs::read_to_string(scripts.join("Export.lua")).unwrap();
        assert!(once.starts_with("-- TacView\nlocal x = 1\n"));
        assert!(once.contains(EXPORT_LINE));
        assert_eq!(hook_export_lua(&scripts).unwrap(), "already loads DCS-BIOS, left unchanged");
        assert_eq!(std::fs::read_to_string(scripts.join("Export.lua")).unwrap(), once);
        let _ = std::fs::remove_dir_all(root);
    }

    /// `UWACS_BIOS_ZIP=/path/DCS-BIOS_vX.zip cargo test install_real_zip -- --ignored`
    #[test]
    #[ignore]
    fn install_real_zip() {
        let zip = std::fs::read(std::env::var("UWACS_BIOS_ZIP").unwrap()).unwrap();
        let root = fake_prefix("zip");
        let scripts = root.join("drive_c/users/steamuser/Saved Games/DCS/Scripts");
        std::fs::create_dir_all(scripts.join("DCS-BIOS")).unwrap();
        let n = install_zip(&zip, &scripts).unwrap();
        assert!(n > 50);
        assert!(scripts.join("DCS-BIOS/BIOS.lua").is_file());
        // previous install was kept as a backup
        assert!(std::fs::read_dir(&scripts).unwrap().flatten().any(|e| e.file_name().to_string_lossy().starts_with("DCS-BIOS.uwacs-backup")));
        let _ = std::fs::remove_dir_all(root);
    }
}
