//! Lighting hardware: MSI Mystic Light board headers (USB HID), Kingston Fury RAM (SMBus) and the Thermalright PA120 Digital display (USB HID).
use crate::hw::fx::*;
use crate::hw::hid::HidDev;
use crate::hw::pawn::*;
use std::thread::sleep;
use std::time::{Duration, Instant};

// ---------------------------------------------------------------- MSI Mystic Light
pub struct Board {
    h: Option<HidDev>,
    pub n1: usize,
    pub n2: usize,
}

impl Board {
    pub fn new() -> Board {
        Board { h: None, n1: 60, n2: 60 }
    }
    pub fn connected(&self) -> bool {
        self.h.is_some()
    }

    pub fn connect(&mut self, n1: usize, n2: usize) -> Option<String> {
        self.n1 = n1;
        self.n2 = n2;
        self.h = None;
        let h = match HidDev::open("vid_1462&pid_7d43") {
            Some(h) => h,
            None => return Some("Mystic Light controller (1462:7D43) not found".into()),
        };
        if h.get_feature(0x53, 725).is_none() {
            return Some("Board does not answer the per-LED protocol - refusing to write".into());
        }
        let ok = Self::send_config(&h, n1, n2);
        self.h = Some(h);
        if ok {
            None
        } else {
            Some("Config write failed".into())
        }
    }

    // Exact "efficiency" config packet from SignalRGB's MSI_185Byte_Controller.js (no flash save).
    fn send_config(h: &HidDev, n1: usize, n2: usize) -> bool {
        let mut p: Vec<u8> = Vec::with_capacity(185);
        p.push(0x52);
        let z = |p: &mut Vec<u8>, a: u8, t: u8, s: u8| p.extend_from_slice(&[a, 0, 0, 0, t, 0, 0, 0, s, 0]);
        z(&mut p, 0x01, 0x08, 0x80);
        z(&mut p, 0x01, 0x2a, 0x80);
        z(&mut p, 0x01, 0x2a, 0x80);
        z(&mut p, 0x01, 0x29, 0x80);
        p.push(n1.max(1) as u8);
        z(&mut p, 0x01, 0x29, 0x80);
        p.push(n2.max(1) as u8);
        z(&mut p, 0x01, 0x29, 0x82);
        p.push(1);
        z(&mut p, 0x01, 0x28, 0x80);
        z(&mut p, 0x25, 0xa9, 0xbf);
        for _ in 0..9 {
            z(&mut p, 0x01, 0x28, 0x80);
        }
        z(&mut p, 0x01, 0x2a, 0x80);
        p.push(0x00);
        p.len() == 185 && h.set_feature(&p, 185)
    }

    pub fn send(&mut self, c12: Rgb, a1: &[Rgb], a2: &[Rgb]) -> bool {
        let Some(h) = &self.h else { return false };
        let mut p: Vec<u8> = vec![0x53, 0x25, 0x06, 0x00, 0x00, c12.r, c12.g, c12.b];
        for c in a1.iter().chain(a2.iter()) {
            p.extend_from_slice(&[c.r, c.g, c.b]);
        }
        h.set_feature(&p, 725)
    }
}

/// `--rgbtest`: shows what the board's lighting controller says and walks red/green/blue through every zone.
pub fn rgb_diag() -> String {
    let mut o = String::new();
    let hex = |v: &[u8]| v.iter().map(|b| format!("{:02X}", b)).collect::<Vec<_>>().join(" ");
    let Some(h) = HidDev::open("vid_1462&pid_7d43") else { return "Mystic Light controller 1462:7D43 not found
".into() };
    for id in [0x52u8, 0x53, 0x50, 0x51] {
        match h.get_feature(id, 725) {
            Some(v) => o += &format!("feature 0x{:02X}: {} bytes: {}
", id, v.len(), hex(&v[..v.len().min(120)])),
            None => o += &format!("feature 0x{:02X}: no answer
", id),
        }
    }
    o += &format!("config write: {}
", Board::send_config(&h, 60, 60));
    if let Some(v) = h.get_feature(0x52, 185) {
        o += &format!("config after: {}
", hex(&v[..v.len().min(185)]));
    }
    let mut b = Board { h: Some(h), n1: 60, n2: 60 };
    for (name, c) in [("red", Rgb::new(255, 0, 0)), ("green", Rgb::new(0, 255, 0)), ("blue", Rgb::new(0, 0, 255))] {
        let ok = b.send(c, &[c; 60], &[c; 60]);
        o += &format!("{} -> write ok: {}
", name, ok);
        sleep(Duration::from_secs(3));
    }
    o
}

// ---------------------------------------------------------------- SMBus (Intel i801) + Kingston Fury RAM
struct Smbus {
    m: Module,
    bus: BusLock,
}

impl Smbus {
    fn open() -> Result<Smbus, String> {
        let m = Module::load(SMBUS_I801)?;
        m.exec("ioctl_identity", &[], 3).map_err(|e| format!("Intel SMBus controller not found (0x{:X})", e))?;
        Ok(Smbus { m, bus: BusLock::new("Global\\Access_SMBUS.HTP.Method") })
    }

    // proto: 0 quick, 1 byte, 2 byte-data, 3 word-data
    fn xfer(&self, addr: u8, read: bool, cmd: u8, proto: i64, data: i64) -> Result<i64, u32> {
        let _g = self.bus.lock(250).ok_or(0xFFFF_FFFFu32)?;
        let mut inp = vec![addr as i64, read as i64, cmd as i64, proto];
        if !read && proto != 0 {
            inp.push(data);
        }
        self.m.exec("ioctl_smbus_xfer", &inp, 5).map(|o| o[0])
    }
    fn write_quick(&self, a: u8) -> bool {
        self.xfer(a, false, 0, 0, 0).is_ok()
    }
    fn write_byte(&self, a: u8, reg: u8, v: u8) -> bool {
        self.xfer(a, false, reg, 2, v as i64).is_ok()
    }
    fn read_byte(&self, a: u8, reg: u8) -> i32 {
        self.xfer(a, true, reg, 2, 0).map(|v| (v & 0xFF) as i32).unwrap_or(-1)
    }
    fn read_word(&self, a: u8, reg: u8) -> i32 {
        self.xfer(a, true, reg, 3, 0).map(|v| (v & 0xFFFF) as i32).unwrap_or(-1)
    }
}

pub struct FuryRam {
    smb: Option<Smbus>,
    pub sticks: Vec<u8>,
    pub status: String,
    per_led: [bool; 128],
    sent: Vec<(u8, [u8; 36])>,
    last_single: Option<(Rgb, Instant)>,
}

impl FuryRam {
    pub fn new() -> FuryRam {
        FuryRam { smb: None, sticks: vec![], status: "RAM: not scanned".into(), per_led: [false; 128], sent: vec![], last_single: None }
    }

    fn system_has_fury() -> bool {
        Self::fury_modules() > 0
    }

    /// How many Kingston Fury / HyperX modules Windows reports (each may carry an RGB controller).
    pub fn fury_modules() -> usize {
        crate::hw::smbios::read().dimms.iter().filter(|(m, pn)| m.to_ascii_lowercase().starts_with("kingston") && (pn.starts_with("KF") || pn.starts_with("HP") || pn.starts_with("HX"))).count()
    }

    pub fn scan(&mut self) {
        self.sticks.clear();
        if !Self::system_has_fury() {
            self.status = "RAM: no Kingston Fury/HyperX modules reported by Windows".into();
            return;
        }
        let smb = match Smbus::open() {
            Ok(s) => s,
            Err(e) => {
                self.status = format!("RAM: {}", e);
                return;
            }
        };
        for a in [0x60u8, 0x61, 0x62, 0x63, 0x58, 0x59, 0x5a, 0x5b] {
            if !smb.write_quick(a) {
                continue;
            }
            sleep(Duration::from_millis(30));
            if Self::is_fury(&smb, a) {
                self.sticks.push(a);
            }
        }
        if self.sticks.is_empty() {
            self.status = "RAM: Kingston modules found by Windows but no RGB controller answered on the SMBus (non-RGB kit?)".into();
            self.smb = Some(smb);
            return;
        }
        self.status = format!("RAM: {} RGB stick(s) at {}", self.sticks.len(), self.sticks.iter().map(|a| format!("0x{:02X}", a)).collect::<Vec<_>>().join(", "));
        self.smb = Some(smb);
        for a in self.sticks.clone() {
            self.set_direct(a);
        }
    }

    /// Looks again for sticks that didn't answer before (just after a cold boot they can be late),
    /// without touching the ones already found: those keep showing what they show.
    pub fn scan_missing(&mut self) {
        if self.smb.is_none() {
            self.scan();
            return;
        }
        let mut found = vec![];
        if let Some(smb) = &self.smb {
            for a in [0x60u8, 0x61, 0x62, 0x63, 0x58, 0x59, 0x5a, 0x5b] {
                if self.sticks.contains(&a) || !smb.write_quick(a) {
                    continue;
                }
                sleep(Duration::from_millis(30));
                if Self::is_fury(smb, a) {
                    found.push(a);
                }
            }
        }
        for a in found {
            self.sticks.push(a);
            self.set_direct(a);
            self.last_single = None; // the new stick gets the current colour on the next frame
        }
        if !self.sticks.is_empty() {
            self.status = format!("RAM: {} RGB stick(s) at {}", self.sticks.len(), self.sticks.iter().map(|a| format!("0x{:02X}", a)).collect::<Vec<_>>().join(", "));
        }
    }

    fn is_fury(s: &Smbus, a: u8) -> bool {
        let pwm = s.read_byte(a, 0xdd);
        if pwm == 0x72 || pwm == 0x78 {
            return false; // voltage regulator, not RAM
        }
        s.write_byte(a, 0x08, 0x53);
        let mut ok = false;
        for _ in 0..5 {
            if ok {
                break;
            }
            let mut nm = [0i32; 4];
            for (i, slot) in nm.iter_mut().enumerate() {
                sleep(Duration::from_millis(30));
                let w = s.read_word(a, (i + 1) as u8);
                *slot = if w < 0 { -1 } else { (w >> 8) & 0xff };
            }
            ok = matches!(nm, [70, 85, 82, 89] | [6, 85, 82, 89] | [7, 224, 255, 255]);
        }
        s.write_byte(a, 0x08, 0x44);
        ok
    }

    fn set_direct(&mut self, a: u8) {
        let Some(s) = &self.smb else { return };
        s.write_byte(a, 0x08, 0x53);
        s.write_byte(a, 0x0b, 0x00);
        s.write_byte(a, 0x09, 0x10);
        s.write_byte(a, 0x0c, 0x01);
        s.write_byte(a, 0x20, 0x50);
        s.write_byte(a, 0x08, 0x44);
        self.per_led[a as usize & 0x7f] = true;
        self.sent.retain(|(x, _)| *x != a);
    }

    pub fn apply_single(&mut self, c: Rgb) {
        if self.smb.is_none() || self.sticks.is_empty() {
            return;
        }
        if let Some((lc, t)) = self.last_single {
            if lc == c && t.elapsed().as_secs() < 3 {
                return;
            }
        }
        self.last_single = Some((c, Instant::now()));
        let s = self.smb.as_ref().unwrap();
        for &a in &self.sticks {
            s.write_byte(a, 0x08, 0x53);
            s.write_byte(a, 0x09, 0x00);
            s.write_byte(a, 0x30, 0x01);
            s.write_byte(a, 0x31, c.r);
            s.write_byte(a, 0x32, c.g);
            s.write_byte(a, 0x33, c.b);
            s.write_byte(a, 0x08, 0x44);
            self.per_led[a as usize & 0x7f] = false;
        }
    }

    /// 12 LEDs per stick.
    pub fn apply_per_led(&mut self, leds: &[Rgb; 12]) {
        if self.smb.is_none() || self.sticks.is_empty() {
            return;
        }
        self.last_single = None;
        for a in self.sticks.clone() {
            if !self.per_led[a as usize & 0x7f] {
                self.set_direct(a);
            }
            let idx = match self.sent.iter().position(|(x, _)| *x == a) {
                Some(i) => i,
                None => {
                    self.sent.push((a, [0xff; 36]));
                    self.sent.len() - 1
                }
            };
            let s = self.smb.as_ref().unwrap();
            s.write_byte(a, 0x08, 0x53);
            for (i, c) in leds.iter().enumerate() {
                let v = [c.r, c.g, c.b];
                for k in 0..3 {
                    if self.sent[idx].1[i * 3 + k] != v[k] && s.write_byte(a, (0x50 + i * 3 + k) as u8, v[k]) {
                        self.sent[idx].1[i * 3 + k] = v[k];
                    }
                }
            }
            s.write_byte(a, 0x08, 0x44);
        }
    }

    pub fn frame(&mut self, p: &EffectParams, t: f64, inp: &FxIn) {
        if self.smb.is_none() || self.sticks.is_empty() {
            return;
        }
        if !p.ram {
            self.apply_single(Rgb::BLACK);
            return;
        }
        if per_led(&p.mode) {
            let mut leds = [Rgb::BLACK; 12];
            for (i, l) in leds.iter_mut().enumerate() {
                *l = pixel_in(p, t, i, 12, inp);
            }
            self.apply_per_led(&leds);
        } else {
            self.apply_single(pixel_in(p, t, 0, 1, inp));
        }
    }
}

// ---------------------------------------------------------------- Thermalright Peerless Assassin 120 Digital (USB HID 0416:8001)
pub use crate::hw::lcd::DisplayParams;

pub struct CoolerDisplay {
    h: Option<HidDev>,
    pub status: String,
}

const HEADER: [u8; 20] = [0xda, 0xdb, 0xdc, 0xdd, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xfc, 0, 0, 0xff];

impl CoolerDisplay {
    pub fn new() -> CoolerDisplay {
        CoolerDisplay { h: None, status: "Cooler display: not connected".into() }
    }
    pub fn connected(&self) -> bool {
        self.h.is_some()
    }
    pub fn connect(&mut self) -> bool {
        self.h = HidDev::open("vid_0416&pid_8001");
        self.status = if self.h.is_none() { "No cooler display found (Deck drives 0416:8001 LCD coolers)".into() } else { "Cooler display connected. Close TRCC - it fights over the same display.".into() };
        self.h.is_some()
    }

    pub fn send(&mut self, led: &[u8]) -> bool {
        let Some(h) = &self.h else { return false };
        let mut first = [0u8; 65];
        first[1..21].copy_from_slice(&HEADER);
        first[21..65].copy_from_slice(&led[..44]);
        if !h.write(&first) {
            self.h = None;
            return false;
        }
        let mut off = 44;
        while off < led.len() {
            let mut pkt = [0u8; 65];
            let n = 64.min(led.len() - off);
            pkt[1..1 + n].copy_from_slice(&led[off..off + n]);
            sleep(Duration::from_millis(2));
            if !h.write(&pkt) {
                self.h = None;
                return false;
            }
            off += 64;
        }
        true
    }
}
