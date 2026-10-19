//! DualShock 4 and DualSense, read passively from the input reports the pad already sends
//! (the layouts and battery rules of Linux's hid-playstation driver). Nothing is ever written
//! to a Sony pad: switching a Bluetooth pad into its extended report can break a game's input
//! until it reconnects. A Bluetooth pad still in its simple report has no battery in it and
//! falls through to the other sources.

use super::hid::{self, Dev};
use super::{Entry, Reading};
use std::collections::HashMap;

pub const SONY: u16 = 0x054C;
const DS4: [u16; 3] = [0x05C4, 0x09CC, 0x0BA0];
const DUALSENSE: [u16; 2] = [0x0CE6, 0x0DF2];
const READ_MS: u32 = 120;

pub fn is_sony_pad(vid: u16, pid: u16) -> bool {
    vid == SONY && (DS4.contains(&pid) || DUALSENSE.contains(&pid))
}

pub fn name(pid: u16) -> &'static str {
    match pid {
        0x0CE6 => "DualSense",
        0x0DF2 => "DualSense Edge",
        _ => "DualShock 4",
    }
}

/// DualShock 4: USB report 0x01 (status at byte 30) or Bluetooth report 0x11 (byte 32).
/// Low nibble = level 0-10 (11 = full on the cable), bit 4 = cable. On the wireless adapter,
/// status byte 2 bit 2 says no controller is connected to it.
pub fn parse_ds4(r: &[u8], adapter: bool) -> Option<Reading> {
    let at = match r.first()? {
        0x01 if r.len() >= 64 => 30,
        0x11 if r.len() >= 78 => 32,
        _ => return None,
    };
    if adapter && r[at + 1] & 0x04 != 0 {
        return None;
    }
    let level = r[at] & 0x0F;
    let cable = r[at] & 0x10 != 0;
    let (percent, charging) = match (cable, level) {
        (true, l) if l < 10 => (l * 10 + 5, true),
        (true, 10) => (100, true),
        (true, 11) => (100, false),
        (true, _) => return None,
        (false, l) if l < 10 => (l * 10 + 5, false),
        (false, _) => (100, false),
    };
    Some(Reading { percent: Some(percent), level: None, charging })
}

/// DualSense: USB report 0x01 (status at byte 53) or Bluetooth report 0x31 (byte 54).
/// Low nibble = level 0-10, high nibble = 0 discharging, 1 charging, 2 full; others are errors.
pub fn parse_dualsense(r: &[u8]) -> Option<Reading> {
    let at = match r.first()? {
        0x01 if r.len() >= 64 => 53,
        0x31 if r.len() >= 78 => 54,
        _ => return None,
    };
    let level = r[at] & 0x0F;
    let percent = (level * 10 + 5).min(100);
    match r[at] >> 4 {
        0x0 => Some(Reading { percent: Some(percent), level: None, charging: false }),
        0x1 => Some(Reading { percent: Some(percent), level: None, charging: true }),
        0x2 => Some(Reading { percent: Some(100), level: None, charging: false }),
        _ => None,
    }
}

/// Sony pads open between polls, by interface path.
#[derive(Default)]
pub struct Sony {
    open: HashMap<String, Dev>,
}

impl Sony {
    pub fn poll(&mut self, ifaces: &[hid::Iface]) -> Vec<Entry> {
        let mut out = Vec::new();
        let mut seen = Vec::new();
        for i in ifaces {
            let Some((vid, pid)) = hid::ids(&i.path) else { continue };
            if !is_sony_pad(vid, pid) {
                continue;
            }
            // only the game pad collection carries the input report
            if !matches!(hid::describe(&i.path), Some((p, u, _)) if hid::is_gamepad(p, u)) {
                continue;
            }
            seen.push(i.path.clone());
            if !self.open.contains_key(&i.path) {
                match Dev::open(&i.path, false) {
                    Some(d) => {
                        self.open.insert(i.path.clone(), d);
                    }
                    None => continue,
                }
            }
            let dev = &self.open[&i.path];
            dev.flush();
            let mut buf = [0u8; 128];
            let reading = match dev.read(&mut buf, READ_MS) {
                Ok(Some(n)) if DUALSENSE.contains(&pid) => parse_dualsense(&buf[..n]),
                Ok(Some(n)) => parse_ds4(&buf[..n], pid == 0x0BA0),
                Ok(None) => None,
                Err(_) => {
                    self.open.remove(&i.path);
                    None
                }
            };
            if let Some(reading) = reading {
                out.push(Entry {
                    id: format!("hid:{}", i.path.to_ascii_lowercase()),
                    name: name(pid).into(),
                    vid,
                    pid,
                    source: "PlayStation",
                    reading,
                    path: i.path.to_ascii_lowercase(),
                    virtual_pad: hid::is_virtual(i.devinst),
                });
            }
        }
        self.open.retain(|p, _| seen.contains(p));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(id: u8, len: usize, at: usize, status: u8) -> Vec<u8> {
        let mut r = vec![0u8; len];
        r[0] = id;
        r[at] = status;
        r
    }

    #[test]
    fn ds4_usb_and_bt_offsets() {
        let usb = report(0x01, 64, 30, 0x06);
        assert_eq!(parse_ds4(&usb, false), Some(Reading { percent: Some(65), level: None, charging: false }));
        let bt = report(0x11, 78, 32, 0x13);
        assert_eq!(parse_ds4(&bt, false), Some(Reading { percent: Some(35), level: None, charging: true }));
        assert_eq!(parse_ds4(&report(0x01, 64, 30, 0x1B), false).unwrap(), Reading { percent: Some(100), level: None, charging: false });
        // the simple Bluetooth report has no battery
        assert_eq!(parse_ds4(&report(0x01, 10, 5, 0), false), None);
    }

    #[test]
    fn ds4_adapter_without_a_pad_is_off() {
        let mut r = report(0x01, 64, 30, 0x05);
        r[31] = 0x04;
        assert_eq!(parse_ds4(&r, true), None);
        assert!(parse_ds4(&r, false).is_some());
    }

    #[test]
    fn dualsense_usb_and_bt_offsets() {
        assert_eq!(parse_dualsense(&report(0x01, 64, 53, 0x07)), Some(Reading { percent: Some(75), level: None, charging: false }));
        assert_eq!(parse_dualsense(&report(0x31, 78, 54, 0x14)), Some(Reading { percent: Some(45), level: None, charging: true }));
        assert_eq!(parse_dualsense(&report(0x01, 64, 53, 0x2A)), Some(Reading { percent: Some(100), level: None, charging: false }));
        assert_eq!(parse_dualsense(&report(0x01, 64, 53, 0xF0)), None);
        assert_eq!(parse_dualsense(&report(0x01, 10, 5, 0)), None);
    }
}
