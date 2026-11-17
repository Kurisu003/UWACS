//! Finding WinWing hardware: HID panels (vendor 0x4098) and DisplayLink instrument displays.

use std::sync::{Mutex, OnceLock};

use hidapi::{HidApi, HidDevice};

pub const WINWING_VID: u16 = 0x4098;
pub const DISPLAYLINK_VID: u16 = 0x17e9;

pub const PID_UFC: u16 = 0xBEDE;
pub const PID_PFP3N: u16 = 0xBB35;
pub const PID_PFP7: u16 = 0xBB37;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Ufc,
    Pfp,
    Other,
}

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct HidDev {
    pub path: String,
    pub vid: u16,
    pub pid: u16,
    pub product: String,
    pub manufacturer: String,
    pub serial: String,
    pub interface: i32,
    pub usage_page: u16,
    pub usage: u16,
    pub role: Role,
    /// Ok(()) when we may open it, Err(reason) otherwise.
    pub access: Result<(), String>,
}

impl HidDev {
    /// Stable key for config files: serial when the panel has one, else vid:pid:interface.
    pub fn key(&self) -> String {
        if self.serial.is_empty() {
            format!("{:04x}:{:04x}:{}", self.vid, self.pid, self.interface)
        } else {
            format!("{:04x}:{:04x}:{}:{}", self.vid, self.pid, self.interface, self.serial)
        }
    }

    pub fn display_name(&self) -> String {
        let base = if self.product.is_empty() { format!("WinWing {:04X}", self.pid) } else { self.product.clone() };
        match self.role {
            Role::Ufc => format!("{base}  (UFC)"),
            Role::Pfp => format!("{base}  (PFP)"),
            Role::Other => base,
        }
    }
}

pub fn role_for(pid: u16, product: &str) -> Role {
    let p = product.to_ascii_uppercase();
    if pid == PID_PFP3N || pid == PID_PFP7 || p.contains("PFP") {
        Role::Pfp
    } else if pid == PID_UFC || p.contains("UFC") {
        Role::Ufc
    } else {
        Role::Other
    }
}

static API: OnceLock<Mutex<Option<HidApi>>> = OnceLock::new();

fn with_api<T>(f: impl FnOnce(&mut HidApi) -> T) -> Option<T> {
    let m = API.get_or_init(|| Mutex::new(HidApi::new().ok()));
    let mut g = m.lock().unwrap();
    if g.is_none() {
        *g = HidApi::new().ok();
    }
    g.as_mut().map(f)
}

pub fn enumerate_hid() -> Vec<HidDev> {
    let infos: Vec<(String, u16, u16, String, String, String, i32, u16, u16)> = with_api(|api| {
        let _ = api.refresh_devices();
        api.device_list()
            .filter(|d| d.vendor_id() == WINWING_VID)
            .map(|d| {
                (
                    d.path().to_string_lossy().into_owned(),
                    d.vendor_id(),
                    d.product_id(),
                    d.product_string().unwrap_or("").to_string(),
                    d.manufacturer_string().unwrap_or("").to_string(),
                    d.serial_number().unwrap_or("").to_string(),
                    d.interface_number(),
                    d.usage_page(),
                    d.usage(),
                )
            })
            .collect()
    })
    .unwrap_or_default();

    let mut out: Vec<HidDev> = infos
        .into_iter()
        .map(|(path, vid, pid, product, manufacturer, serial, interface, usage_page, usage)| {
            let access = check_access(&path);
            HidDev { role: role_for(pid, &product), path, vid, pid, product, manufacturer, serial, interface, usage_page, usage, access }
        })
        .collect();
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out.dedup_by(|a, b| a.path == b.path);
    out
}

fn check_access(path: &str) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        match std::fs::OpenOptions::new().read(true).write(true).open(path) {
            Ok(_) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                Err("no permission (install the udev rule, see Devices tab)".into())
            }
            Err(e) => Err(e.to_string()),
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = path;
        Ok(())
    }
}

pub fn open(path: &str) -> anyhow::Result<HidDevice> {
    let c = std::ffi::CString::new(path)?;
    with_api(|api| api.open_path(&c))
        .ok_or_else(|| anyhow::anyhow!("HID API unavailable"))?
        .map_err(|e| anyhow::anyhow!("failed to open {path}: {e}"))
}

/// Raw HID report descriptor for a device path.
pub fn report_descriptor(path: &str, dev: Option<&HidDevice>) -> Vec<u8> {
    #[cfg(target_os = "linux")]
    {
        if let Some(node) = path.strip_prefix("/dev/") {
            let sys = format!("/sys/class/hidraw/{node}/device/report_descriptor");
            if let Ok(d) = std::fs::read(sys) {
                return d;
            }
        }
    }
    if let Some(d) = dev {
        let mut buf = vec![0u8; 4096];
        if let Ok(n) = d.get_report_descriptor(&mut buf) {
            buf.truncate(n);
            return buf;
        }
    }
    let _ = path;
    Vec::new()
}

// ------------------------------------------------------------------ DisplayLink displays

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct UsbDisplay {
    pub product: String,
    pub manufacturer: String,
    pub serial: String,
    pub pid: u16,
    pub speed_mbps: String,
    pub sysfs: String,
}

pub fn enumerate_usb_displays() -> Vec<UsbDisplay> {
    let mut out = Vec::new();
    #[cfg(target_os = "linux")]
    {
        let rd = |p: &std::path::Path, f: &str| std::fs::read_to_string(p.join(f)).map(|s| s.trim().to_string()).unwrap_or_default();
        if let Ok(entries) = std::fs::read_dir("/sys/bus/usb/devices") {
            for e in entries.flatten() {
                let p = e.path();
                if rd(&p, "idVendor") != format!("{DISPLAYLINK_VID:04x}") {
                    continue;
                }
                let product = rd(&p, "product");
                out.push(UsbDisplay {
                    pid: u16::from_str_radix(&rd(&p, "idProduct"), 16).unwrap_or(0),
                    manufacturer: rd(&p, "manufacturer"),
                    serial: rd(&p, "serial"),
                    speed_mbps: rd(&p, "speed"),
                    sysfs: p.to_string_lossy().into_owned(),
                    product,
                });
            }
        }
    }
    out
}

/// Whether DisplayLinkManager is running (needed for the instrument displays on Linux).
pub fn displaylink_service_active() -> Option<bool> {
    #[cfg(target_os = "linux")]
    {
        let o = std::process::Command::new("systemctl").args(["is-active", "displaylink"]).output().ok()?;
        Some(String::from_utf8_lossy(&o.stdout).trim() == "active")
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

pub const UDEV_RULE: &str = r#"# WinWing / WinCtrl panels: let the logged-in user open them (hidraw) for UWACS
KERNEL=="hidraw*", ATTRS{idVendor}=="4098", MODE="0660", TAG+="uaccess"
SUBSYSTEM=="usb", ATTRS{idVendor}=="4098", MODE="0660", TAG+="uaccess"
"#;
