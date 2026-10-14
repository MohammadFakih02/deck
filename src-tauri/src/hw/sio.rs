//! Motherboard fan / temperature chips ("Super I/O"), reached through the PawnIO LpcIO module.
//!
//! Supported families (register maps from LibreHardwareMonitor, MPL-2.0):
//! * Nuvoton NCT6683D/6686D/6687D(R) "EC" chips - MSI boards (6687D control verified on MS-7D43)
//! * Nuvoton NCT6779D..NCT6799D - ASUS, ASRock, older MSI
//! * ITE IT86xx / IT87xx - Gigabyte and others
//!
//! Anything that is not verified is monitoring-only unless the user switches on experimental control.
use crate::hw::pawn::*;
use std::thread::sleep;
use std::time::{Duration, Instant};

#[derive(Clone, PartialEq, Debug)]
pub enum Control {
    /// fan control known to work on this chip family
    Verified,
    /// registers are known, but untested on this board: only when the user allows it
    Experimental,
    /// monitoring only (reason)
    ReadOnly(String),
}

pub struct Reading {
    pub pct: Vec<Option<f32>>,
    pub rpm: Vec<u32>,
    pub temps: Vec<Option<f32>>,
    pub volts: Vec<(String, f32)>,
}

pub trait FanChip: Send {
    fn name(&self) -> String;
    fn fan_names(&self) -> Vec<String>;
    fn temp_names(&self) -> Vec<String>;
    /// index into temps of the board's own CPU temperature (used when the CPU itself can't be read)
    fn cpu_temp(&self) -> Option<usize>;
    fn control(&self) -> Control;
    fn read(&mut self) -> Option<Reading>;
    fn set_percent(&mut self, i: usize, pct: f32);
    fn release(&mut self, i: usize);
    fn release_all(&mut self) {
        for i in 0..self.fan_names().len() {
            self.release(i);
        }
    }
    /// raw register dump lines for the hardware report
    fn report(&self) -> String {
        String::new()
    }
}

// ---------------------------------------------------------------- port access
struct Lpc {
    m: Module,
    bus: BusLock,
}

impl Lpc {
    fn inb(&self, p: u16) -> u8 {
        self.m.exec("ioctl_pio_inb", &[p as i64], 1).map(|v| v[0] as u8).unwrap_or(0xFF)
    }
    fn outb(&self, p: u16, v: u8) {
        let _ = self.m.exec("ioctl_pio_outb", &[p as i64, v as i64], 0);
    }
    fn sio_in(&self, r: u8) -> u8 {
        self.m.exec("ioctl_superio_inb", &[r as i64], 1).map(|v| v[0] as u8).unwrap_or(0xFF)
    }
    fn sio_inw(&self, r: u8) -> u16 {
        self.m.exec("ioctl_superio_inw", &[r as i64], 1).map(|v| v[0] as u16).unwrap_or(0xFFFF)
    }
    fn sio_out(&self, r: u8, v: u8) {
        let _ = self.m.exec("ioctl_superio_outb", &[r as i64, v as i64], 0);
    }
}

fn valid_base(a: u16) -> bool {
    a >= 0x100 && (a & 0xF007) == 0
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Nct {
    Ec(bool), // NCT6683/6686/6687; true = NCT6687DR register map (MSI 800-series)
    N6779,
    N6791,
    N6792,
    N6793,
    N6795,
    N6796,
    N6796R, // 6796DR / 6796DS / 5585D
    N6797,
    N6798,
    N6799,
}

fn nct_kind(id: u8, rev: u8, msi800: bool) -> Option<(Nct, &'static str)> {
    Some(match (id, rev) {
        (0xC7, 0x32) => (Nct::Ec(false), "NCT6683D"),
        (0xD4, 0x40) | (0xD4, 0x41) => (Nct::Ec(false), "NCT6686D"),
        (0xD5, 0x92) if msi800 => (Nct::Ec(true), "NCT6687DR"),
        (0xD5, 0x92) => (Nct::Ec(false), "NCT6687D"),
        (0xC5, r) if r & 0xF0 == 0x60 => (Nct::N6779, "NCT6779D"),
        (0xC8, 0x03) => (Nct::N6791, "NCT6791D"),
        (0xC9, 0x11) | (0xC9, 0x13) => (Nct::N6792, "NCT6792D"),
        (0xD1, 0x21) => (Nct::N6793, "NCT6793D"),
        (0xD3, 0x52) => (Nct::N6795, "NCT6795D"),
        (0xD4, 0x23) => (Nct::N6796, "NCT6796D"),
        (0xD4, 0x2A) => (Nct::N6796R, "NCT6796D-R"),
        (0xD4, 0x51) => (Nct::N6797, "NCT6797D"),
        (0xD4, 0x2B) => (Nct::N6798, "NCT6798D"),
        (0xD8, 0x02) => (Nct::N6799, "NCT6799D"),
        _ => return None,
    })
}

fn ite_name(id: u16) -> Option<&'static str> {
    Some(match id {
        0x8613 => "IT8613E",
        0x8620 => "IT8620E",
        0x8625 => "IT8625E",
        0x8628 => "IT8628E",
        0x8631 => "IT8631E",
        0x8638 => "IT8638E",
        0x8655 => "IT8655E",
        0x8665 => "IT8665E",
        0x8686 => "IT8686E",
        0x8688 => "IT8688E",
        0x8689 => "IT8689E",
        0x8696 => "IT8696E",
        0x8705 => "IT8705F",
        0x8712 => "IT8712F",
        0x8716 => "IT8716F",
        0x8718 => "IT8718F",
        0x8720 => "IT8720F",
        0x8721 => "IT8721F",
        0x8726 => "IT8726F",
        0x8728 => "IT8728F",
        0x8771 => "IT8771E",
        0x8772 => "IT8772E",
        0x8790 => "IT8790E",
        0x8733 => "IT8792E",
        0x8695 => "IT87952E",
        _ => return None,
    })
}

/// Finds the board's fan chip. `info` is used to recognise MSI 800-series boards.
pub fn open(info: &crate::hw::smbios::Info) -> Result<Box<dyn FanChip>, String> {
    // HP laptops have no Super I/O chip: their fans are driven through the BIOS
    if let Some(c) = crate::hw::hp::open(info) {
        return Ok(c);
    }
    let lpc = Lpc { m: Module::load(LPC_IO)?, bus: BusLock::new("Global\\Access_ISABUS.HTP.Method") };
    let mut found: Vec<(String, u16, u8, Nct)> = vec![]; // nuvoton: name, base, slot
    let mut ites: Vec<(String, u16, u8, u8)> = vec![]; // ite: name, base, slot, version
    let mut seen: Vec<String> = vec![];
    {
        let _g = lpc.bus.lock(500).ok_or("hardware bus busy")?;
        for (slot, port) in [(0u8, 0x2Eu16), (1, 0x4E)] {
            let _ = lpc.m.exec("ioctl_select_slot", &[slot as i64], 0);
            // Winbond / Nuvoton / Fintek
            lpc.outb(port, 0x87);
            lpc.outb(port, 0x87);
            let (id, rev) = (lpc.sio_in(0x20), lpc.sio_in(0x21));
            if let Some((kind, name)) = nct_kind(id, rev, info.is_msi_800()) {
                let _ = lpc.m.exec("ioctl_find_bars", &[], 0);
                lpc.sio_out(0x07, 0x0B);
                let a = lpc.sio_inw(0x60);
                sleep(Duration::from_millis(1));
                let b = lpc.sio_inw(0x60);
                if !matches!(kind, Nct::Ec(_)) {
                    // disable the hardware-monitor I/O space lock on NCT679x
                    let o = lpc.sio_in(0x28);
                    if o & 0x10 != 0 {
                        lpc.sio_out(0x28, o & !0x10);
                    }
                }
                lpc.outb(port, 0xAA);
                let mut addr = a;
                if addr & 7 == 5 {
                    addr &= 0xFFF8;
                }
                if a == b && valid_base(addr) {
                    found.push((name.to_string(), addr, slot, kind));
                } else {
                    seen.push(format!("{} at 0x{:X}: bad base address 0x{:X}", name, port, a));
                }
                continue;
            }
            if id != 0 && id != 0xFF {
                seen.push(format!("unknown Nuvoton/Winbond chip {:02X}{:02X} at 0x{:X}", id, rev, port));
            }
            lpc.outb(port, 0xAA);
            // ITE
            let pre = lpc.sio_inw(0x20); // a bugged secondary chip at 0x4E may already be in config mode
            let chip = if port == 0x4E && pre != 0xFFFF {
                pre
            } else {
                lpc.outb(port, 0x87);
                lpc.outb(port, 0x01);
                lpc.outb(port, 0x55);
                lpc.outb(port, if port == 0x4E { 0xAA } else { 0x55 });
                lpc.sio_inw(0x20)
            };
            if let Some(name) = ite_name(chip) {
                let _ = lpc.m.exec("ioctl_find_bars", &[], 0);
                lpc.sio_out(0x07, 0x04); // environment controller
                let a = lpc.sio_inw(0x60);
                sleep(Duration::from_millis(1));
                let b = lpc.sio_inw(0x60);
                let version = lpc.sio_in(0x22) & 0x0F;
                if port != 0x4E {
                    lpc.outb(port, 0x02);
                    lpc.outb(port + 1, 0x02);
                }
                if a == b && valid_base(a) {
                    ites.push((name.to_string(), a, slot, version));
                } else {
                    seen.push(format!("{} at 0x{:X}: bad base address 0x{:X}", name, port, a));
                }
            } else {
                if chip != 0 && chip != 0xFFFF {
                    seen.push(format!("unknown ITE chip {:04X} at 0x{:X}", chip, port));
                }
                if port != 0x4E {
                    lpc.outb(port, 0x02);
                    lpc.outb(port + 1, 0x02);
                }
            }
        }
    }
    // probing the second port changes the module's slot (and the I/O ranges it allows), so go back to the
    // chosen chip and register its ranges again before using it
    let reselect = |slot: u8, ite: bool| {
        let port: u16 = if slot == 0 { 0x2E } else { 0x4E };
        let _g = lpc.bus.lock(500);
        let _ = lpc.m.exec("ioctl_select_slot", &[slot as i64], 0);
        if ite {
            for b in [0x87u8, 0x01, 0x55, if port == 0x4E { 0xAA } else { 0x55 }] {
                lpc.outb(port, b);
            }
        } else {
            lpc.outb(port, 0x87);
            lpc.outb(port, 0x87);
        }
        let _ = lpc.m.exec("ioctl_find_bars", &[], 0);
        if ite {
            if port != 0x4E {
                lpc.outb(port, 0x02);
                lpc.outb(port + 1, 0x02);
            }
        } else {
            lpc.outb(port, 0xAA);
        }
    };
    if let Some((name, base, slot, kind)) = found.into_iter().next() {
        reselect(slot, false);
        return match kind {
            Nct::Ec(dr) => Ok(Box::new(NctEc::new(lpc, base, name, dr))),
            k => Nct679::new(lpc, base, name, k).map(|c| Box::new(c) as Box<dyn FanChip>),
        };
    }
    if let Some((name, base, slot, version)) = ites.into_iter().next() {
        reselect(slot, true);
        return Ite::new(lpc, base, name, version).map(|c| Box::new(c) as Box<dyn FanChip>);
    }
    Err(if seen.is_empty() { "No supported fan controller chip found on this motherboard".into() } else { format!("No supported fan controller ({})", seen.join("; ")) })
}

// ================================================================ Nuvoton NCT668x "EC" (MSI)
const EC_PAGE: u16 = 4;
const EC_INDEX: u16 = 5;
const EC_DATA: u16 = 6;
const EC_SELECT: u8 = 0xFF;
const MODE_REG: u16 = 0xA00;
const REQ_REG: u16 = 0xA01;
const REQ_START: u8 = 0x80;
const REQ_DONE: u8 = 0x40;

pub struct NctEc {
    lpc: Lpc,
    port: u16,
    name: String,
    dr: bool,
    rpm_regs: Vec<u16>,
    pwm_regs: Vec<u16>,
    saved: [Option<(u8, u8)>; 8],
}

const EC_TEMPS: [(&str, u16); 6] = [("CPU", 0x100), ("System", 0x102), ("VRM (MOS)", 0x104), ("Chipset", 0x106), ("CPU socket", 0x108), ("M.2 SSD", 0x10C)];
const EC_FANS: [&str; 8] = ["CPU Fan", "Pump Fan", "System Fan #1", "System Fan #2", "System Fan #3", "System Fan #4", "System Fan #5", "System Fan #6"];

impl NctEc {
    fn new(lpc: Lpc, port: u16, name: String, dr: bool) -> NctEc {
        let (rpm_regs, pwm_regs) = if dr {
            // NCT6687DR: CPU, Pump, then SYSFAN1..6 live in a second bank
            (vec![0x140, 0x142, 0x15E, 0x15C, 0x15A, 0x158, 0x156, 0x154], vec![0x160, 0x161, 0xE05, 0xE04, 0xE03, 0xE02, 0xE01, 0xE00])
        } else {
            ((0..8).map(|i| 0x140 + i * 2).collect(), (0..8).map(|i| 0x160 + i).collect())
        };
        let s = NctEc { lpc, port, name, dr, rpm_regs, pwm_regs, saved: [None; 8] };
        if let Some(_g) = s.lpc.bus.lock(500) {
            let d = s.ec_read(0x180);
            if d & 0x80 == 0 {
                s.ec_write(0x180, d | 0x80);
            }
        }
        s
    }

    fn ec_wait(&self) {
        let t = Instant::now();
        while self.lpc.inb(self.port + EC_PAGE) != EC_SELECT && t.elapsed() < Duration::from_millis(500) {
            sleep(Duration::from_millis(1));
        }
        self.lpc.outb(self.port + EC_PAGE, EC_SELECT); // force access after a timeout, like LibreHardwareMonitor
    }
    fn ec_read(&self, addr: u16) -> u8 {
        self.ec_wait();
        self.lpc.outb(self.port + EC_PAGE, (addr >> 8) as u8);
        self.lpc.outb(self.port + EC_INDEX, (addr & 0xFF) as u8);
        let v = self.lpc.inb(self.port + EC_DATA);
        self.lpc.outb(self.port + EC_PAGE, EC_SELECT);
        v
    }
    fn ec_write(&self, addr: u16, val: u8) {
        self.ec_wait();
        self.lpc.outb(self.port + EC_PAGE, (addr >> 8) as u8);
        self.lpc.outb(self.port + EC_INDEX, (addr & 0xFF) as u8);
        self.lpc.outb(self.port + EC_DATA, val);
        self.lpc.outb(self.port + EC_PAGE, EC_SELECT);
    }
    pub fn manual_bits(&self) -> Option<u8> {
        let _g = self.lpc.bus.lock(200)?;
        Some(self.ec_read(MODE_REG))
    }
}

impl FanChip for NctEc {
    fn name(&self) -> String {
        self.name.clone()
    }
    fn fan_names(&self) -> Vec<String> {
        EC_FANS.iter().map(|s| s.to_string()).collect()
    }
    fn temp_names(&self) -> Vec<String> {
        EC_TEMPS.iter().map(|t| t.0.to_string()).collect()
    }
    fn cpu_temp(&self) -> Option<usize> {
        Some(0)
    }
    fn control(&self) -> Control {
        if self.dr {
            Control::ReadOnly("MSI 800-series boards (NCT6687DR) use a different fan-control protocol that Deck does not drive yet".into())
        } else {
            Control::Verified
        }
    }
    fn read(&mut self) -> Option<Reading> {
        let _g = self.lpc.bus.lock(100)?;
        let mut r = Reading { pct: vec![], rpm: vec![], temps: vec![], volts: vec![] };
        for i in 0..8 {
            r.pct.push(Some((self.ec_read(self.pwm_regs[i]) as f32 / 2.55).round()));
            let hi = self.ec_read(self.rpm_regs[i]) as u32;
            let lo = self.ec_read(self.rpm_regs[i] + 1) as u32;
            let v = (hi << 8) | lo;
            r.rpm.push(if v >= 0xFFF0 { 0 } else { v });
        }
        for (_, reg) in EC_TEMPS {
            let v = self.ec_read(reg) as i8 as f32 + if self.ec_read(reg + 1) & 0x80 != 0 { 0.5 } else { 0.0 };
            r.temps.push((v > 0.0 && v < 125.0).then_some(v));
        }
        // voltages (scaling from LibreHardwareMonitor: 12 V and 5 V sit behind dividers, DRAM x2)
        for (name, reg, mul) in [("+12V", 0x120u16, 12.0f32), ("+5V", 0x122, 5.0), ("CPU core", 0x124, 1.0), ("DRAM", 0x128, 2.0), ("CPU I/O", 0x12A, 1.0), ("CPU SA", 0x12C, 1.0)] {
            let v = 0.001 * (16.0 * self.ec_read(reg) as f32 + (self.ec_read(reg + 1) >> 4) as f32) * mul;
            if v > 0.1 {
                r.volts.push((name.into(), v));
            }
        }
        Some(r)
    }
    fn set_percent(&mut self, i: usize, pct: f32) {
        if self.dr || i >= 8 {
            return;
        }
        let value = (pct.clamp(0.0, 100.0) * 2.55).round() as u8;
        let Some(_g) = self.lpc.bus.lock(200) else { return };
        let bit = 1u8 << i;
        if self.saved[i].is_none() {
            self.saved[i] = Some((self.ec_read(MODE_REG) & bit, self.ec_read(0xA28 + i as u16)));
        }
        self.ec_write(REQ_REG, REQ_START);
        sleep(Duration::from_millis(50));
        let mode = self.ec_read(MODE_REG);
        self.ec_write(MODE_REG, mode | bit);
        self.ec_write(0xA28 + i as u16, value);
        self.ec_write(REQ_REG, REQ_DONE);
        sleep(Duration::from_millis(50));
    }
    /// Always clears the manual bit: restoring what we found at start-up is wrong when an earlier
    /// MoboCtl was killed without releasing (it would "restore" manual mode).
    fn release(&mut self, i: usize) {
        if self.dr || i >= 8 {
            return;
        }
        let Some(_g) = self.lpc.bus.lock(500) else { return };
        let bit = 1u8 << i;
        if self.ec_read(MODE_REG) & bit == 0 && self.saved[i].is_none() {
            return;
        }
        self.ec_write(REQ_REG, REQ_START);
        sleep(Duration::from_millis(50));
        let mode = self.ec_read(MODE_REG);
        self.ec_write(MODE_REG, mode & !bit);
        if let Some((_, cmd)) = self.saved[i] {
            self.ec_write(0xA28 + i as u16, cmd);
        }
        self.ec_write(REQ_REG, REQ_DONE);
        sleep(Duration::from_millis(50));
        self.saved[i] = None;
    }
    fn report(&self) -> String {
        format!("manual-mode bits (0xA00): {}", self.manual_bits().map(|b| format!("{:08b}", b)).unwrap_or_else(|| "unreadable".into()))
    }
}

// ================================================================ Nuvoton NCT6779D..NCT6799D (banked registers)
pub struct Nct679 {
    lpc: Lpc,
    base: u16,
    name: String,
    fans: usize,
    pwm_out: [u16; 7],
    temp_slots: Vec<(u16, u16, i32, String)>, // reg, half reg, half bit, label
    saved: [Option<(u8, u8)>; 7],
}

const NCT_COUNT: [u16; 7] = [0x4B0, 0x4B2, 0x4B4, 0x4B6, 0x4B8, 0x4BA, 0x4CC];
const NCT_CMD: [u16; 7] = [0x109, 0x209, 0x309, 0x809, 0x909, 0xA09, 0xB09];
const NCT_MODE: [u16; 7] = [0x102, 0x202, 0x302, 0x802, 0x902, 0xA02, 0xB02];

fn nct_source_name(s: u8) -> String {
    match s {
        1 => "System".into(),
        2 => "CPU socket".into(),
        3..=7 => format!("Aux {}", s - 3),
        8 | 9 => format!("SMBus {}", s - 8),
        10 => "T-sensor".into(),
        16 => "CPU".into(),
        17 => "CPU (PECI 1)".into(),
        18 => "PCH CPU max".into(),
        19 => "Chipset".into(),
        20 => "PCH CPU".into(),
        21 => "PCH MCH".into(),
        22..=25 => format!("DIMM {}", s - 22),
        _ => format!("Sensor {}", s),
    }
}

impl Nct679 {
    fn new(lpc: Lpc, base: u16, name: String, kind: Nct) -> Result<Nct679, String> {
        let fans = match kind {
            Nct::N6779 => 5,
            Nct::N6796R | Nct::N6797 | Nct::N6798 | Nct::N6799 => 7,
            _ => 6,
        };
        let pwm_out = if matches!(kind, Nct::N6797 | Nct::N6798 | Nct::N6799 | Nct::N6796R) {
            [0x001, 0x003, 0x011, 0x013, 0x015, 0xA09, 0xB09]
        } else {
            [0x001, 0x003, 0x011, 0x013, 0x015, 0x017, 0x029]
        };
        let mut c = Nct679 { lpc, base, name, fans, pwm_out, temp_slots: vec![], saved: [None; 7] };
        {
            let _g = c.lpc.bus.lock(500).ok_or("hardware bus busy")?;
            let vendor = ((c.rd(0x804F) as u16) << 8) | c.rd(0x004F) as u16;
            if vendor != 0x5CA3 {
                return Err(format!("{}: vendor check failed (0x{:04X})", c.name, vendor));
            }
            // the BIOS decides what each monitoring slot measures; read the source of every slot once
            let slots: [(u16, u16, i32, u16); 7] = [(0x073, 0x074, 7, 0x100), (0x075, 0x076, 7, 0x200), (0x077, 0x078, 7, 0x300), (0x079, 0x07A, 7, 0x800), (0x07B, 0x07C, 7, 0x900), (0x07D, 0x07E, 7, 0xA00), (0x4A0, 0x49E, 6, 0xB00)];
            let mut seen = vec![];
            for (reg, half, bit, src) in slots {
                let s = c.rd(src) & 0x1F;
                if s == 0 || seen.contains(&s) {
                    continue;
                }
                seen.push(s);
                c.temp_slots.push((reg, half, bit, nct_source_name(s)));
            }
        }
        Ok(c)
    }
    fn rd(&self, reg: u16) -> u8 {
        self.lpc.outb(self.base + 5, 0x4E);
        self.lpc.outb(self.base + 6, (reg >> 8) as u8);
        self.lpc.outb(self.base + 5, (reg & 0xFF) as u8);
        self.lpc.inb(self.base + 6)
    }
    fn wr(&self, reg: u16, v: u8) {
        self.lpc.outb(self.base + 5, 0x4E);
        self.lpc.outb(self.base + 6, (reg >> 8) as u8);
        self.lpc.outb(self.base + 5, (reg & 0xFF) as u8);
        self.lpc.outb(self.base + 6, v);
    }
}

impl FanChip for Nct679 {
    fn name(&self) -> String {
        self.name.clone()
    }
    fn fan_names(&self) -> Vec<String> {
        (1..=self.fans).map(|i| format!("Fan {}", i)).collect()
    }
    fn temp_names(&self) -> Vec<String> {
        self.temp_slots.iter().map(|s| s.3.clone()).collect()
    }
    fn cpu_temp(&self) -> Option<usize> {
        let n = self.temp_names();
        n.iter().position(|s| s == "CPU").or_else(|| n.iter().position(|s| s == "CPU socket"))
    }
    fn control(&self) -> Control {
        Control::Experimental
    }
    fn read(&mut self) -> Option<Reading> {
        let _g = self.lpc.bus.lock(100)?;
        let mut r = Reading { pct: vec![], rpm: vec![], temps: vec![], volts: vec![] };
        for i in 0..self.fans {
            let hi = self.rd(NCT_COUNT[i]) as u32;
            let lo = self.rd(NCT_COUNT[i] + 1) as u32;
            let count = (hi << 5) | (lo & 0x1F);
            r.rpm.push(if (0x15..0x1FFF).contains(&count) { (1.35e6 / count as f64) as u32 } else { 0 });
            r.pct.push(Some((self.rd(self.pwm_out[i]) as f32 / 2.55).round()));
        }
        for (reg, half, bit, _) in &self.temp_slots {
            let mut v = (self.rd(*reg) as i8 as i32) << 1;
            if *bit >= 0 {
                v |= ((self.rd(*half) >> *bit) & 1) as i32;
            }
            let t = v as f32 * 0.5;
            r.temps.push((t > -55.0 && t < 125.0 && t != 0.0).then_some(t));
        }
        let vcore = 0.008 * self.rd(0x480) as f32;
        if vcore > 0.1 {
            r.volts.push(("CPU core".into(), vcore));
        }
        Some(r)
    }
    fn set_percent(&mut self, i: usize, pct: f32) {
        if i >= self.fans {
            return;
        }
        let Some(_g) = self.lpc.bus.lock(200) else { return };
        if self.saved[i].is_none() {
            self.saved[i] = Some((self.rd(NCT_MODE[i]), self.rd(NCT_CMD[i])));
        }
        self.wr(NCT_MODE[i], 0); // manual
        self.wr(NCT_CMD[i], (pct.clamp(0.0, 100.0) * 2.55).round() as u8);
    }
    fn release(&mut self, i: usize) {
        if i >= self.fans {
            return;
        }
        let Some((mode, cmd)) = self.saved[i] else { return };
        let Some(_g) = self.lpc.bus.lock(500) else { return };
        self.wr(NCT_MODE[i], mode);
        self.wr(NCT_CMD[i], cmd);
        self.saved[i] = None;
    }
}

// ================================================================ ITE IT86xx / IT87xx
pub struct Ite {
    lpc: Lpc,
    base: u16,
    name: String,
    fans: usize,
    controls: usize,
    temps: usize,
    pwm_ctrl: Vec<u8>,
    has_ext: bool,
    alt6: bool,
    bank_select: bool,
    disabled: Vec<bool>,
    saved: [Option<(u8, bool, u8)>; 6], // pwm ctrl, main-ctrl bit, ext pwm
}

const ITE_PWM_EXT: [u8; 6] = [0x63, 0x6b, 0x73, 0x7b, 0xa3, 0xab];
const ITE_TACH: [u8; 6] = [0x0d, 0x0e, 0x0f, 0x80, 0x82, 0x4c];
const ITE_TACH_EXT: [u8; 6] = [0x18, 0x19, 0x1a, 0x81, 0x83, 0x4d];
const ITE_TACH_ALT: [u8; 6] = [0x0d, 0x0e, 0x0f, 0x80, 0x82, 0x93];
const ITE_TACH_EXT_ALT: [u8; 6] = [0x18, 0x19, 0x1a, 0x81, 0x83, 0x94];
const ITE_MAIN_CTRL: u8 = 0x13;

impl Ite {
    fn new(lpc: Lpc, base: u16, name: String, _version: u8) -> Result<Ite, String> {
        let n = name.as_str();
        let (temps, fans, controls) = match n {
            "IT8613E" => (4, 5, 4),
            "IT8625E" => (3, 6, 6),
            "IT8628E" | "IT8665E" | "IT8689E" | "IT8696E" => (6, 6, 6),
            "IT8631E" | "IT8638E" => (2, 2, 2),
            "IT8686E" => (7, 6, 5),
            "IT8688E" => (6, 6, 5),
            "IT87952E" | "IT8792E" | "IT8705F" => (3, 3, 3),
            "IT8655E" => (6, 3, 3),
            "IT8620E" => (3, 5, 5),
            _ => (3, 5, 3),
        };
        let pwm_ctrl = match n {
            "IT8665E" | "IT8625E" => vec![0x15, 0x16, 0x17, 0x1e, 0x1f, 0x92],
            "IT8792E" => vec![0x15, 0x16, 0x17],
            _ => vec![0x15, 0x16, 0x17, 0x7f, 0xa7, 0xaf],
        };
        let has_ext = !matches!(n, "IT8705F" | "IT8712F" | "IT8716F" | "IT8718F" | "IT8720F" | "IT8726F");
        let mut c = Ite { lpc, base, name: name.clone(), fans, controls: controls.min(pwm_ctrl.len()), temps, pwm_ctrl, has_ext, alt6: matches!(n, "IT8665E" | "IT8625E"), bank_select: matches!(n, "IT8665E" | "IT8655E"), disabled: vec![false; fans], saved: [None; 6] };
        {
            let _g = c.lpc.bus.lock(500).ok_or("hardware bus busy")?;
            let (vendor, ok) = c.rd(0x58);
            if !ok || !(vendor == 0x90 || vendor == 0x7F) {
                return Err(format!("{}: vendor check failed", name));
            }
            let (cfg, ok) = c.rd(0x00);
            if !ok || (cfg & 0x10 == 0 && !matches!(n, "IT8655E" | "IT8665E")) {
                return Err(format!("{}: not in monitoring mode", name));
            }
            // fans 4..6 only count when their 16-bit counter is enabled
            let (modes, ok) = c.rd(0x0C);
            if ok {
                if fans >= 5 {
                    c.disabled[3] = modes & (1 << 4) == 0;
                    c.disabled[4] = modes & (1 << 5) == 0;
                }
                if fans >= 6 {
                    c.disabled[5] = if n == "IT8665E" { c.rd(0x0B).0 & (1 << 3) == 0 } else { modes & (1 << 2) == 0 };
                }
            }
        }
        Ok(c)
    }
    fn rd(&self, reg: u8) -> (u8, bool) {
        self.lpc.outb(self.base + 5, reg);
        let v = self.lpc.inb(self.base + 6);
        (v, self.lpc.inb(self.base + 5) == reg || self.name == "IT8688E")
    }
    fn wr(&self, reg: u8, v: u8) {
        self.lpc.outb(self.base + 5, reg);
        self.lpc.outb(self.base + 6, v);
        let _ = self.lpc.inb(self.base + 5);
    }
    fn bank0(&self) {
        if self.bank_select {
            let (v, ok) = self.rd(0x06);
            if ok {
                self.wr(0x06, v & 0x9F);
            }
        }
    }
}

impl FanChip for Ite {
    fn name(&self) -> String {
        self.name.clone()
    }
    fn fan_names(&self) -> Vec<String> {
        (1..=self.fans).map(|i| format!("Fan {}", i)).collect()
    }
    fn temp_names(&self) -> Vec<String> {
        (1..=self.temps).map(|i| format!("Sensor {}", i)).collect()
    }
    fn cpu_temp(&self) -> Option<usize> {
        None // which ITE input is the CPU differs per board
    }
    fn control(&self) -> Control {
        Control::Experimental
    }
    fn read(&mut self) -> Option<Reading> {
        let _g = self.lpc.bus.lock(100)?;
        self.bank0();
        let mut r = Reading { pct: vec![], rpm: vec![], temps: vec![], volts: vec![] };
        let gain = match self.name.as_str() {
            "IT8625E" | "IT8792E" | "IT87952E" => 0.011,
            "IT8655E" | "IT8665E" => 0.0109,
            "IT8705F" | "IT8712F" | "IT8716F" | "IT8718F" | "IT8720F" | "IT8726F" => 0.016,
            _ => 0.012,
        };
        let (v0, ok) = self.rd(0x20);
        if ok && v0 > 0 {
            r.volts.push(("CPU core".into(), gain * v0 as f32));
        }
        for i in 0..self.temps {
            let (v, ok) = self.rd(0x29 + i as u8);
            let t = v as i8;
            r.temps.push((ok && t > 0 && t < 127).then_some(t as f32));
        }
        for i in 0..self.fans {
            if self.disabled[i] {
                r.rpm.push(0);
            } else {
                let (lo, ok1) = self.rd(if self.alt6 { ITE_TACH_ALT[i] } else { ITE_TACH[i] });
                let (hi, ok2) = self.rd(if self.alt6 { ITE_TACH_EXT_ALT[i] } else { ITE_TACH_EXT[i] });
                let v = (lo as u32) | ((hi as u32) << 8);
                r.rpm.push(if ok1 && ok2 && v > 0x3f && v < 0xffff { (1.35e6 / (v as f64 * 2.0)) as u32 } else { 0 });
            }
            if i < self.controls {
                let (v, ok) = self.rd(self.pwm_ctrl[i]);
                r.pct.push(if !ok || v & 0x80 != 0 {
                    None // automatic mode: the chip doesn't expose the duty
                } else if self.has_ext {
                    Some((self.rd(ITE_PWM_EXT[i]).0 as f32 * 100.0 / 255.0).round())
                } else {
                    Some(((v & 0x7F) as f32 * 100.0 / 127.0).round())
                });
            } else {
                r.pct.push(None);
            }
        }
        Some(r)
    }
    fn set_percent(&mut self, i: usize, pct: f32) {
        if i >= self.controls {
            return;
        }
        let Some(_g) = self.lpc.bus.lock(200) else { return };
        let value = (pct.clamp(0.0, 100.0) * 2.55).round() as u8;
        if self.saved[i].is_none() {
            let ctrl = self.rd(self.pwm_ctrl[i]).0;
            let main = i < 3 && self.rd(ITE_MAIN_CTRL).0 & (1 << i) != 0;
            let ext = if self.has_ext { self.rd(ITE_PWM_EXT[i]).0 } else { 0 };
            self.saved[i] = Some((ctrl, main, ext));
        }
        let (ctrl, main, _) = self.saved[i].unwrap();
        if i < 3 && !main {
            let m = self.rd(ITE_MAIN_CTRL).0;
            self.wr(ITE_MAIN_CTRL, m | (1 << i));
        }
        if self.has_ext {
            self.wr(self.pwm_ctrl[i], if self.name == "IT8689E" { 0x7F } else { ctrl & 0x7F });
            self.wr(ITE_PWM_EXT[i], value);
        } else {
            self.wr(self.pwm_ctrl[i], value >> 1);
        }
    }
    fn release(&mut self, i: usize) {
        if i >= self.controls {
            return;
        }
        let Some((ctrl, main, ext)) = self.saved[i] else { return };
        let Some(_g) = self.lpc.bus.lock(500) else { return };
        self.wr(self.pwm_ctrl[i], ctrl);
        if i < 3 {
            let m = self.rd(ITE_MAIN_CTRL).0;
            if (m & (1 << i) != 0) != main {
                self.wr(ITE_MAIN_CTRL, m ^ (1 << i));
            }
        }
        if self.has_ext {
            self.wr(ITE_PWM_EXT[i], ext);
        }
        self.saved[i] = None;
    }
}
