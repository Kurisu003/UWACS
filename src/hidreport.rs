//! Minimal HID report descriptor parser: finds where every button bit lives in the input reports,
//! so any WinWing device can be remapped without a per-device table.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ButtonRef {
    /// Report id (0 when the device does not use report ids).
    pub report_id: u8,
    /// Bit offset into the report payload, i.e. after the report id byte.
    pub bit: u32,
    /// HID button usage number (Button 1, Button 2, ... as Windows/DCS number them).
    pub usage: u32,
}

/// A multi-bit value such as a slider, wheel or axis.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AxisRef {
    pub report_id: u8,
    pub bit: u32,
    pub size: u32,
    /// (usage page << 16) | usage, e.g. 0x10036 = Generic Desktop Slider
    pub usage: u32,
    pub min: i64,
    pub max: i64,
}

/// Pseudo button numbers for "axis i moved up / down" (wheels turned into key taps).
pub const AXIS_PSEUDO_BASE: u32 = 1000;
pub fn axis_up(i: usize) -> u32 {
    AXIS_PSEUDO_BASE + 1 + 2 * i as u32
}
pub fn axis_down(i: usize) -> u32 {
    AXIS_PSEUDO_BASE + 2 + 2 * i as u32
}

#[derive(Clone, Debug, Default)]
pub struct Layout {
    pub buttons: Vec<ButtonRef>,
    pub axes: Vec<AxisRef>,
    pub uses_report_ids: bool,
}

pub fn parse(desc: &[u8]) -> Layout {
    #[derive(Clone, Copy, Default)]
    struct Globals {
        page: u32,
        size: u32,
        count: u32,
        id: u8,
        lmin: i64,
        lmax: i64,
    }
    let mut g = Globals::default();
    let mut stack: Vec<Globals> = Vec::new();
    let mut usages: Vec<u32> = Vec::new();
    let (mut umin, mut umax): (Option<u32>, Option<u32>) = (None, None);
    let mut cursor: std::collections::HashMap<u8, u32> = Default::default();
    let mut out = Layout::default();

    let mut i = 0;
    while i < desc.len() {
        let b = desc[i];
        if b == 0xFE {
            let n = *desc.get(i + 1).unwrap_or(&0) as usize;
            i += 3 + n;
            continue;
        }
        let n = [0usize, 1, 2, 4][(b & 3) as usize];
        let kind = (b >> 2) & 3;
        let tag = b >> 4;
        let mut val: u32 = 0;
        for k in 0..n {
            val |= (*desc.get(i + 1 + k).unwrap_or(&0) as u32) << (8 * k);
        }
        i += 1 + n;
        match kind {
            // Main
            0 => {
                if tag == 8 {
                    // Input
                    let constant = val & 1 != 0;
                    let variable = val & 2 != 0;
                    let pos = cursor.entry(g.id).or_insert(0);
                    let is_buttons = g.page == 0x09 && g.size == 1 && variable && !constant;
                    if is_buttons {
                        for k in 0..g.count {
                            let usage = if !usages.is_empty() {
                                *usages.get(k as usize).unwrap_or(usages.last().unwrap())
                            } else if let Some(lo) = umin {
                                (lo + k).min(umax.unwrap_or(u32::MAX))
                            } else {
                                k + 1
                            };
                            out.buttons.push(ButtonRef { report_id: g.id, bit: *pos + k, usage: usage & 0xFFFF });
                        }
                    }
                    if !is_buttons && variable && !constant && g.size > 1 && matches!(g.page, 0x01 | 0x02) {
                        for k in 0..g.count {
                            let usage = if !usages.is_empty() {
                                *usages.get(k as usize).unwrap_or(usages.last().unwrap())
                            } else {
                                umin.map(|lo| lo + k).unwrap_or(0)
                            };
                            out.axes.push(AxisRef { report_id: g.id, bit: *pos + k * g.size, size: g.size, usage: (g.page << 16) | (usage & 0xFFFF), min: g.lmin, max: g.lmax });
                        }
                    }
                    *pos += g.size * g.count;
                } else if tag == 9 || tag == 11 {
                    // Output / Feature: they have their own bit cursors we don't need.
                }
                if matches!(tag, 8 | 9 | 10 | 11 | 12) {
                    // every main item (incl. Collection) ends the local usages
                    usages.clear();
                    umin = None;
                    umax = None;
                }
            }
            // Global
            1 => match tag {
                0 => g.page = val,
                1 => {
                    // logical minimum is signed
                    g.lmin = match n {
                        1 => val as u8 as i8 as i64,
                        2 => val as u16 as i16 as i64,
                        _ => val as i32 as i64,
                    }
                }
                2 => {
                    // logical maximum: signed, unless that would make it smaller than the minimum
                    let signed = match n {
                        1 => val as u8 as i8 as i64,
                        2 => val as u16 as i16 as i64,
                        _ => val as i32 as i64,
                    };
                    g.lmax = if signed < g.lmin { val as i64 } else { signed };
                }
                7 => g.size = val,
                8 => {
                    g.id = val as u8;
                    out.uses_report_ids = true;
                }
                9 => g.count = val,
                10 => stack.push(g),
                11 => {
                    if let Some(s) = stack.pop() {
                        g = s;
                    }
                }
                _ => {}
            },
            // Local
            2 => {
                let v = if n == 4 { val & 0xFFFF } else { val };
                match tag {
                    0 => usages.push(v),
                    1 => umin = Some(v),
                    2 => umax = Some(v),
                    _ => {}
                }
            }
            _ => {}
        }
    }
    out
}

/// Read the bit for `b` out of a raw report as returned by hidapi (report id first when used).
pub fn bit_set(report: &[u8], layout: &Layout, b: &ButtonRef) -> Option<bool> {
    let payload = if layout.uses_report_ids {
        if report.first() != Some(&b.report_id) {
            return None;
        }
        &report[1..]
    } else {
        report
    };
    let byte = payload.get((b.bit / 8) as usize)?;
    Some(byte >> (b.bit % 8) & 1 == 1)
}

/// Raw value of an axis, None if this report does not carry it.
pub fn axis_value(report: &[u8], layout: &Layout, a: &AxisRef) -> Option<i64> {
    let payload = if layout.uses_report_ids {
        if report.first() != Some(&a.report_id) {
            return None;
        }
        &report[1..]
    } else {
        report
    };
    let mut v: u64 = 0;
    for k in 0..a.size.min(32) {
        let bit = a.bit + k;
        let byte = *payload.get((bit / 8) as usize)?;
        if byte >> (bit % 8) & 1 == 1 {
            v |= 1 << k;
        }
    }
    if a.min < 0 && a.size < 64 && v >> (a.size - 1) & 1 == 1 {
        return Some(v as i64 - (1i64 << a.size));
    }
    Some(v as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_joystick() {
        // Report id 1: 32 buttons (usage 1..32), then a 16-bit X axis.
        let desc = [
            0x05, 0x01, 0x09, 0x04, 0xA1, 0x01, 0x85, 0x01, // page GD, joystick, collection, id 1
            0x05, 0x09, 0x19, 0x01, 0x29, 0x20, 0x15, 0x00, 0x25, 0x01, // buttons 1..32
            0x75, 0x01, 0x95, 0x20, 0x81, 0x02, // 32 x 1 bit input
            0x05, 0x01, 0x09, 0x30, 0x75, 0x10, 0x95, 0x01, 0x81, 0x02, // X axis
            0xC0,
        ];
        let l = parse(&desc);
        assert!(l.uses_report_ids);
        assert_eq!(l.buttons.len(), 32);
        assert_eq!(l.buttons[0], ButtonRef { report_id: 1, bit: 0, usage: 1 });
        assert_eq!(l.buttons[31].usage, 32);
        let report = [1u8, 0b0000_0100, 0, 0, 0x80, 0xff, 0xff];
        assert_eq!(bit_set(&report, &l, &l.buttons[2]), Some(true));
        assert_eq!(bit_set(&report, &l, &l.buttons[31]), Some(true));
        assert_eq!(bit_set(&report, &l, &l.buttons[0]), Some(false));
    }

    #[test]
    fn winwing_mfd_wheel() {
        // CarrierAce MFD R descriptor, captured 2026-11-17
        let hex = "05 01 09 04 a1 01 85 01 05 09 19 01 29 32 15 00 25 01 35 00 45 01 75 01 95 32 81 02 75 01 95 06 81 01 05 01 09 36 15 00 27 ff ff 00 00 35 00 47 ff ff 00 00 75 10 95 01 81 02 85 02 06 ff 00 09 01 15 00 26 ff 00 35 00 46 ff 00 75 08 95 0d 81 02 09 02 91 02 c0";
        let desc: Vec<u8> = hex.split(' ').map(|h| u8::from_str_radix(h, 16).unwrap()).collect();
        let l = parse(&desc);
        assert_eq!(l.buttons.len(), 50);
        assert_eq!(l.axes, vec![AxisRef { report_id: 1, bit: 56, size: 16, usage: 0x10036, min: 0, max: 65535 }]);
        let report = [0x01u8, 0, 0, 0, 0, 0, 0x40, 0, 0x00, 0x80];
        assert_eq!(axis_value(&report, &l, &l.axes[0]), Some(32768));
        assert_eq!(bit_set(&report, &l, &l.buttons[46]), Some(true)); // button 47, the side switch
    }
}

#[cfg(test)]
mod live {
    /// `cargo test live_descriptors -- --ignored --nocapture`: parse every hidraw descriptor on this PC.
    #[test]
    #[ignore]
    fn live_descriptors() {
        for e in std::fs::read_dir("/sys/class/hidraw").unwrap().flatten() {
            let p = e.path().join("device/report_descriptor");
            let name = std::fs::read_to_string(e.path().join("device/uevent")).unwrap_or_default();
            let name = name.lines().find(|l| l.starts_with("HID_NAME=")).unwrap_or("").to_string();
            if let Ok(d) = std::fs::read(&p) {
                let l = super::parse(&d);
                let usages: Vec<u32> = l.buttons.iter().map(|b| b.usage).collect();
                println!("{:?} {name}: {} buttons ids={} first={:?}", e.file_name(), l.buttons.len(), l.uses_report_ids, &usages[..usages.len().min(8)]);
            }
        }
    }
}
