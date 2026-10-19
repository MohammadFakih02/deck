//! GameSir Nova 2 Lite on its 2.4 GHz dongle. Windows, XInput and SDL only see the dongle, which
//! always says "wired", so this asks the controller itself, the way GameSir Connect does:
//! GetDeviceInfo (`01 01`) on the dongle's vendor interface (`MI_02`, usage page 0xFF7A, 65-byte
//! reports). The reply starts `00 01`; bytes 2..8 are the controller's MAC (all zero: the dongle
//! answered for itself, the controller is off) and byte 19 the battery: 0-100 %, 0xFF charging
//! with no level, bit 7 charging on firmware that still sends a level.

use super::hid::{self, Dev};
use super::{Entry, Reading};

pub const VID: u16 = 0x3537;
pub const PID: u16 = 0x1098;
const IFACE: &str = "vid_3537&pid_1098&mi_02";
pub const NAME: &str = "GameSir Nova 2 Lite";
const REPORT: usize = 65;
const REPLY_MS: u32 = 500;

/// (percent or None while charging without a level, charging, MAC) from a reply; None for any
/// other report and for the dongle's own answer (no MAC).
pub fn parse(r: &[u8]) -> Option<(Option<u8>, bool, [u8; 6])> {
    if r.len() <= 19 || r[0] != 0x00 || r[1] != 0x01 {
        return None;
    }
    let mac: [u8; 6] = r[2..8].try_into().ok()?;
    if mac.iter().all(|b| *b == 0) {
        return None;
    }
    let b = r[19];
    if b == 0xFF {
        return Some((None, true, mac));
    }
    Some((Some((b & 0x7F).min(100)), b & 0x80 != 0, mac))
}

/// The open dongle between polls.
#[derive(Default)]
pub struct GameSir {
    dev: Option<(String, Dev)>,
}

impl GameSir {
    pub fn poll(&mut self, ifaces: &[hid::Iface]) -> Vec<Entry> {
        let Some(path) = ifaces.iter().map(|i| &i.path).find(|p| p.to_ascii_lowercase().contains(IFACE)) else {
            self.dev = None;
            return vec![];
        };
        if self.dev.as_ref().is_none_or(|(p, _)| p != path) {
            self.dev = Dev::open(path, true).map(|d| (path.clone(), d));
        }
        let Some((_, dev)) = &self.dev else { return vec![] };
        match ask(dev) {
            Ok(Some((percent, charging, mac))) => {
                let mac: String = mac.iter().map(|b| format!("{b:02x}")).collect();
                vec![Entry {
                    id: format!("gamesir:{mac}"),
                    name: NAME.into(),
                    vid: VID,
                    pid: PID,
                    source: "GameSir",
                    reading: Reading { percent, level: None, charging },
                    path: path.to_ascii_lowercase(),
                    virtual_pad: false,
                }]
            }
            Ok(None) => vec![],
            Err(_) => {
                // unplugged mid-poll: find it again next time
                self.dev = None;
                vec![]
            }
        }
    }
}

fn ask(dev: &Dev) -> Result<Option<(Option<u8>, bool, [u8; 6])>, u32> {
    dev.flush(); // drop queued stick reports and old replies
    let mut req = [0u8; REPORT];
    req[1] = 0x01;
    req[2] = 0x01;
    if dev.write(&req, REPLY_MS)?.is_none() {
        return Ok(None);
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(REPLY_MS as u64);
    let mut buf = [0u8; REPORT];
    while let Some(left) = deadline.checked_duration_since(std::time::Instant::now()) {
        let Some(n) = dev.read(&mut buf, (left.as_millis() as u32).max(10))? else { return Ok(None) };
        // the constant `00 f4 …` stream is stick and button state: skip it
        if let Some(r) = parse(&buf[..n]) {
            return Ok(Some(r));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reply(b19: u8, mac: [u8; 6]) -> Vec<u8> {
        let mut r = vec![0u8; 65];
        r[1] = 0x01;
        r[2..8].copy_from_slice(&mac);
        r[19] = b19;
        r
    }

    #[test]
    fn gamesir_reply_parses() {
        let mac = [0x4c, 0x4a, 0xc9, 0xf6, 0x19, 0x01];
        assert_eq!(parse(&reply(0x40, mac)), Some((Some(64), false, mac)));
        assert_eq!(parse(&reply(0xFF, mac)), Some((None, true, mac)));
        assert_eq!(parse(&reply(0xC8, mac)), Some((Some(72), true, mac)));
        assert_eq!(parse(&reply(0x7F, mac)), Some((Some(100), false, mac)));
    }

    #[test]
    fn zero_mac_is_off() {
        assert_eq!(parse(&reply(0x40, [0; 6])), None);
    }

    #[test]
    fn other_reports_are_ignored() {
        let mut r = reply(0x40, [1; 6]);
        r[1] = 0xf4;
        assert_eq!(parse(&r), None);
        assert_eq!(parse(&[0, 1, 2]), None);
    }
}
