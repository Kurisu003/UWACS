//! Automatic updates from GitHub releases.
//!
//! A release counts when its tag is vX.Y.Z and it carries the asset `uwacs-linux-x86_64`
//! (the raw binary, built by .github/workflows/release.yml). The new binary is downloaded next to
//! the running one, must answer `--version` with the expected version, and only then replaces it.
//! The previous binary stays as `<name>.old`. The new version runs from the next start.

use std::io::Read;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::log::log;

pub const DEFAULT_REPO: &str = "Kurisu003/UWACS";
pub const ASSET: &str = "uwacs-linux-x86_64";

#[derive(Clone, Debug, PartialEq)]
pub enum State {
    Idle,
    Checking,
    UpToDate(String),
    Downloading(String),
    /// installed, waiting for a restart
    Installed(String),
    Failed(String),
}

pub static STATE: Mutex<State> = Mutex::new(State::Idle);
static LAST_CHECK: Mutex<Option<Instant>> = Mutex::new(None);

fn set(s: State) {
    *STATE.lock().unwrap() = s;
    if let Some(ctx) = crate::remap::EGUI_CTX.get() {
        ctx.request_repaint();
    }
}

pub fn state() -> State {
    STATE.lock().unwrap().clone()
}

pub fn parse_version(s: &str) -> Option<(u32, u32, u32)> {
    let s = s.trim().trim_start_matches(['v', 'V']);
    let mut it = s.split(['.', '-', '+']).map(|p| p.parse::<u32>());
    let a = it.next()?.ok()?;
    let b = it.next().and_then(|x| x.ok()).unwrap_or(0);
    let c = it.next().and_then(|x| x.ok()).unwrap_or(0);
    Some((a, b, c))
}

pub fn current() -> (u32, u32, u32) {
    // UWACS_PRETEND_VERSION lets a test pretend to be older than the newest release
    std::env::var("UWACS_PRETEND_VERSION").ok().and_then(|v| parse_version(&v)).or_else(|| parse_version(env!("CARGO_PKG_VERSION"))).unwrap_or((0, 0, 0))
}

/// `uwacs --update`: check and install right now, printing what happens.
pub fn cli(repo: &str) -> anyhow::Result<()> {
    let cur = current();
    println!("this is {}.{}.{}", cur.0, cur.1, cur.2);
    match newest(repo)? {
        Some(r) if r.version > cur => {
            println!("installing {} from {}", r.tag, r.url);
            install(&r)?;
            println!("installed {}; it runs from the next start", r.tag);
        }
        Some(r) => println!("up to date (newest release is {})", r.tag),
        None => println!("no release with {ASSET} yet"),
    }
    Ok(())
}

#[derive(Clone, Debug)]
pub struct Release {
    pub tag: String,
    pub version: (u32, u32, u32),
    pub url: String,
}

/// Newest release with a Linux binary. Releases without the asset (like the old Tauri "V1") are ignored.
pub fn newest(repo: &str) -> anyhow::Result<Option<Release>> {
    let url = format!("https://api.github.com/repos/{repo}/releases?per_page=20");
    let v: serde_json::Value = ureq::get(&url).header("User-Agent", "uwacs-updater").header("Accept", "application/vnd.github+json").call()?.body_mut().read_json()?;
    let mut best: Option<Release> = None;
    for r in v.as_array().into_iter().flatten() {
        if r["draft"].as_bool().unwrap_or(false) || r["prerelease"].as_bool().unwrap_or(false) {
            continue;
        }
        let tag = r["tag_name"].as_str().unwrap_or("").to_string();
        let Some(version) = parse_version(&tag) else { continue };
        let Some(asset) = r["assets"].as_array().into_iter().flatten().find(|a| a["name"].as_str() == Some(ASSET)) else { continue };
        let Some(dl) = asset["browser_download_url"].as_str() else { continue };
        if best.as_ref().map(|b| version > b.version).unwrap_or(true) {
            best = Some(Release { tag, version, url: dl.to_string() });
        }
    }
    Ok(best)
}

fn exe_path() -> anyhow::Result<PathBuf> {
    Ok(std::fs::canonicalize(std::env::current_exe()?)?)
}

/// Download, verify and swap in the new binary.
pub fn install(rel: &Release) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let exe = exe_path()?;
    let dir = exe.parent().ok_or_else(|| anyhow::anyhow!("no folder for {}", exe.display()))?;
    let name = exe.file_name().unwrap().to_string_lossy().to_string();
    let new = dir.join(format!("{name}.new"));
    let old = dir.join(format!("{name}.old"));

    let mut buf = Vec::new();
    ureq::get(&rel.url).header("User-Agent", "uwacs-updater").call()?.body_mut().as_reader().take(300_000_000).read_to_end(&mut buf)?;
    if buf.len() < 100_000 || &buf[..4] != b"\x7fELF" {
        anyhow::bail!("the download is not a Linux program ({} bytes)", buf.len());
    }
    std::fs::write(&new, &buf)?;
    std::fs::set_permissions(&new, std::fs::Permissions::from_mode(0o755))?;

    // the new binary must start and report the version the release promised
    let out = crate::displays::run_timeout(&new.to_string_lossy(), &["--version".to_string()], 15);
    let said = out.as_ref().map(|o| String::from_utf8_lossy(&o.stdout).to_string()).unwrap_or_default();
    if parse_version(said.split_whitespace().last().unwrap_or("")) != Some(rel.version) {
        let _ = std::fs::remove_file(&new);
        anyhow::bail!("the downloaded program did not start correctly (said {:?})", said.trim());
    }
    let _ = std::fs::remove_file(&old);
    std::fs::rename(&exe, &old)?;
    if let Err(e) = std::fs::rename(&new, &exe) {
        let _ = std::fs::rename(&old, &exe);
        return Err(e.into());
    }
    Ok(())
}

/// Check (and install if `apply`) once, in the background.
pub fn check_now(repo: String, apply: bool) {
    if matches!(state(), State::Checking | State::Downloading(_)) {
        return;
    }
    *LAST_CHECK.lock().unwrap() = Some(Instant::now());
    set(State::Checking);
    std::thread::spawn(move || {
        let cur = current();
        match newest(&repo) {
            Ok(Some(r)) if r.version > cur => {
                if !apply {
                    set(State::UpToDate(format!("{} is available", r.tag)));
                    return;
                }
                set(State::Downloading(r.tag.clone()));
                match install(&r) {
                    Ok(()) => {
                        log(format!("Update: installed {}, restart UWACS to use it", r.tag));
                        set(State::Installed(r.tag));
                    }
                    Err(e) => {
                        log(format!("Update: {} failed: {e}", r.tag));
                        set(State::Failed(e.to_string()));
                    }
                }
            }
            Ok(Some(r)) => set(State::UpToDate(format!("newest is {}", r.tag))),
            Ok(None) => set(State::UpToDate("no releases with a Linux build yet".into())),
            Err(e) => set(State::Failed(format!("could not reach GitHub: {e}"))),
        }
    });
}

/// Called every frame: checks 20 s after start, then every 6 hours, when automatic updates are on.
pub fn tick(enabled: bool, repo: &str) {
    static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    let start = *START.get_or_init(Instant::now);
    if !enabled || start.elapsed() < Duration::from_secs(20) || matches!(state(), State::Installed(_)) {
        return;
    }
    let due = LAST_CHECK.lock().unwrap().map(|t| t.elapsed() > Duration::from_secs(6 * 3600)).unwrap_or(true);
    if due {
        check_now(repo.to_string(), true);
    }
}

/// Start the (possibly new) binary again and let the caller close this one.
pub fn restart() -> anyhow::Result<()> {
    let exe = exe_path()?;
    let mut cmd = std::process::Command::new("setsid");
    cmd.arg("-f").arg(&exe);
    if cmd.spawn().is_err() {
        std::process::Command::new(&exe).spawn()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn versions() {
        assert_eq!(parse_version("v0.4.0"), Some((0, 4, 0)));
        assert_eq!(parse_version("V1"), Some((1, 0, 0)));
        assert_eq!(parse_version("0.10.2-beta"), Some((0, 10, 2)));
        assert_eq!(parse_version("latest"), None);
        assert!(parse_version("v0.10.0") > parse_version("v0.9.9"));
    }
}
