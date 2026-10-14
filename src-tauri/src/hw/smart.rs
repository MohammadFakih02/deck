//! Drive health (S.M.A.R.T.), like CrystalDiskInfo: the NVMe health log and the SATA attribute
//! and threshold tables, judged Good / Caution / Bad, plus a check every 10 minutes that warns
//! when a drive gets worse. Standard NVMe and ATA commands that Windows passes through, so no
//! vendor code; drives behind USB bridges or RAID often hide them and show as "unknown".
//! Needs admin (raw drive handles), which Deck has.

use serde::{Deserialize, Serialize};
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Attr {
    pub id: u8,
    pub name: String,
    pub current: u8,
    pub worst: u8,
    pub threshold: u8,
    pub raw: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Health {
    pub disk: u32,
    pub model: String,
    pub serial: String,
    /// "NVMe", "SATA", "USB", "RAID", …
    pub bus: String,
    pub ssd: bool,
    pub size: u64,
    /// "good", "caution", "bad" or "unknown" (the drive gave no health data)
    pub status: String,
    /// why it isn't good, in plain words
    pub reasons: Vec<String>,
    /// problems that are over: a value that went past its limit once but is fine now (the drive
    /// is still "good"; SeaTools warns on these, CrystalDiskInfo doesn't). Told once.
    #[serde(default)]
    pub history: Vec<String>,
    /// life left, % (SSDs that report it)
    pub health: Option<u8>,
    pub temp: Option<f64>,
    pub hours: Option<u64>,
    pub cycles: Option<u64>,
    /// NVMe only (SATA drives count in vendor-specific units)
    pub written: Option<u64>,
    pub read: Option<u64>,
    pub unsafe_shutdowns: Option<u64>,
    pub media_errors: Option<u64>,
    /// the counters a warning compares: reallocated, pending, uncorrectable sectors (SATA)
    pub bad_sectors: Option<u64>,
    pub attrs: Vec<Attr>,
}

const BUS: [&str; 19] = ["Unknown", "SCSI", "ATAPI", "ATA", "FireWire", "SSA", "Fibre", "USB", "RAID", "iSCSI", "SAS", "SATA", "SD", "MMC", "Virtual", "Virtual", "Spaces", "NVMe", "SCM"];

/// Every physical drive this PC has, with its health.
pub fn read_all() -> Vec<Health> {
    (0..32).filter_map(read_one).collect()
}

/// The last reading (reading every drive takes a moment, and a sleeping hard drive may spin up).
static LAST: parking_lot::Mutex<Option<(std::time::Instant, Vec<Health>)>> = parking_lot::Mutex::new(None);

/// The drives, read again when the last reading is older than `max_age` seconds.
pub fn cached(max_age: u64) -> Vec<Health> {
    if let Some((t, v)) = LAST.lock().as_ref() {
        if t.elapsed().as_secs() < max_age {
            return v.clone();
        }
    }
    let v = read_all();
    *LAST.lock() = Some((std::time::Instant::now(), v.clone()));
    v
}

/// Checks every drive 90 s after start, then every 10 minutes, against the last check saved in
/// `%APPDATA%\Deck\drive-health.json`, and warns once for each thing that got worse (and once
/// for a drive already in trouble the first time Deck sees it). The warnings are opt-in (Drives
/// page, `drive_alerts`); the checks are saved either way, so turning them on later doesn't bring
/// up old news.
pub fn start(app: std::sync::Arc<crate::app::App>) {
    std::thread::Builder::new()
        .name("drive-health".into())
        .spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(90));
            let file = crate::util::appdata().join("Deck").join("drive-health.json");
            loop {
                let now = cached(0);
                if !now.is_empty() {
                    let before: Vec<Health> = std::fs::read(&file).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
                    let alerts = app.settings.lock().drive_alerts;
                    for d in now.iter().filter(|_| alerts) {
                        let old = before.iter().find(|o| same_drive(o, d));
                        let past = new_history(old, d);
                        if !past.is_empty() {
                            app.toast(crate::app::Toast {
                                actions: vec![("show".into(), "Show".into())],
                                sticky: true,
                                ..crate::app::Toast::simple(&format!("drive-past:{}", d.disk), &format!("Drive history: {}", d.model), &format!("Healthy now, but earlier: {}. Deck won't mention it again.", past.join(" · ")))
                            });
                        }
                        let news = match old {
                            Some(o) => worse(o, d),
                            None if rank(&d.status) >= 2 => vec![format!("Health is {}", d.status)],
                            None => vec![],
                        };
                        if !news.is_empty() {
                            let mut body = news.join(" · ");
                            if let Some(r) = d.reasons.first() {
                                body = format!("{body}. {r}.");
                            }
                            app.toast(crate::app::Toast {
                                actions: vec![("show".into(), "Show".into())],
                                sticky: true,
                                sound: true,
                                ..crate::app::Toast::simple(&format!("drive:{}", d.disk), &format!("Drive warning: {}", d.model), &body)
                            });
                        }
                    }
                    if let Ok(b) = serde_json::to_vec(&now) {
                        let _ = std::fs::write(&file, b);
                    }
                }
                std::thread::sleep(std::time::Duration::from_secs(600));
            }
        })
        .ok();
}

/// The same drive as last time: by serial number, or by model and slot when it has none.
fn same_drive(a: &Health, b: &Health) -> bool {
    if !a.serial.is_empty() || !b.serial.is_empty() {
        a.serial == b.serial
    } else {
        a.model == b.model && a.disk == b.disk
    }
}

fn read_one(i: u32) -> Option<Health> {
    use windows_sys::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE};
    use windows_sys::Win32::Storage::FileSystem::{CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING};
    let path: Vec<u16> = format!(r"\\.\PhysicalDrive{i}").encode_utf16().chain(Some(0)).collect();
    let h = unsafe { CreateFileW(path.as_ptr(), GENERIC_READ | GENERIC_WRITE, FILE_SHARE_READ | FILE_SHARE_WRITE, std::ptr::null(), OPEN_EXISTING, 0, std::ptr::null_mut()) };
    if h == INVALID_HANDLE_VALUE {
        return None;
    }
    let mut d = Health { disk: i, status: "unknown".into(), ..Default::default() };
    describe(h, &mut d);
    if d.bus == "NVMe" {
        if let Some(log) = nvme_log(h) {
            nvme_judge(&log, &mut d);
        }
    } else if let Some((data, thresholds)) = ata_tables(h, i as u8) {
        let ok = ata_status(h, i as u8);
        ata_judge(&data, &thresholds, ok, &mut d);
    }
    unsafe { CloseHandle(h) };
    Some(d)
}

fn query_property(h: HANDLE, id: i32, out: &mut [u8]) -> bool {
    use windows_sys::Win32::System::Ioctl::IOCTL_STORAGE_QUERY_PROPERTY;
    let mut q = [0u8; 12];
    q[..4].copy_from_slice(&id.to_le_bytes());
    let mut ret = 0u32;
    unsafe { windows_sys::Win32::System::IO::DeviceIoControl(h, IOCTL_STORAGE_QUERY_PROPERTY, q.as_ptr() as *const _, 12, out.as_mut_ptr() as *mut _, out.len() as u32, &mut ret, std::ptr::null_mut()) != 0 }
}

/// Model, serial, bus, SSD or not, size.
fn describe(h: HANDLE, d: &mut Health) {
    use windows_sys::Win32::System::Ioctl::{StorageDeviceProperty, StorageDeviceSeekPenaltyProperty, IOCTL_DISK_GET_LENGTH_INFO};
    let mut buf = vec![0u8; 1024];
    if query_property(h, StorageDeviceProperty, &mut buf) {
        let text = |at: usize| {
            let off = u32::from_le_bytes(buf[at..at + 4].try_into().unwrap()) as usize;
            if off == 0 || off >= buf.len() {
                return String::new();
            }
            let end = buf[off..].iter().position(|b| *b == 0).map(|p| off + p).unwrap_or(buf.len());
            String::from_utf8_lossy(&buf[off..end]).trim().to_string()
        };
        let (vendor, product, serial) = (text(12), text(16), text(24));
        d.model = if vendor.is_empty() || product.starts_with(&vendor) { product } else { format!("{vendor} {product}") };
        d.serial = serial;
        let bus = u32::from_le_bytes(buf[28..32].try_into().unwrap()) as usize;
        d.bus = BUS.get(bus).unwrap_or(&"Other").to_string();
    }
    if d.model.is_empty() {
        d.model = format!("Disk {}", d.disk);
    }
    // seek penalty: false = SSD (DEVICE_SEEK_PENALTY_DESCRIPTOR.IncursSeekPenalty at byte 8)
    let mut sp = [0u8; 12];
    d.ssd = d.bus == "NVMe" || (query_property(h, StorageDeviceSeekPenaltyProperty, &mut sp) && sp[8] == 0);
    let mut len = [0u8; 8];
    let mut ret = 0u32;
    if unsafe { windows_sys::Win32::System::IO::DeviceIoControl(h, IOCTL_DISK_GET_LENGTH_INFO, std::ptr::null(), 0, len.as_mut_ptr() as *mut _, 8, &mut ret, std::ptr::null_mut()) } != 0 {
        d.size = u64::from_le_bytes(len);
    }
}

// ---------------------------------------------------------------- NVMe

/// Log page 2, "SMART / Health Information" (512 bytes).
fn nvme_log(h: HANDLE) -> Option<Vec<u8>> {
    use windows_sys::Win32::System::Ioctl::*;
    let mut buf = vec![0u8; 8 + 40 + 512];
    let put = |b: &mut Vec<u8>, at: usize, v: u32| b[at..at + 4].copy_from_slice(&v.to_le_bytes());
    put(&mut buf, 0, StorageDeviceProtocolSpecificProperty as u32);
    put(&mut buf, 4, 0);
    put(&mut buf, 8, ProtocolTypeNvme as u32);
    put(&mut buf, 12, NVMeDataTypeLogPage as u32);
    put(&mut buf, 16, 2);
    put(&mut buf, 24, 40);
    put(&mut buf, 28, 512);
    let mut ret = 0u32;
    let len = buf.len() as u32;
    let ok = unsafe { windows_sys::Win32::System::IO::DeviceIoControl(h, IOCTL_STORAGE_QUERY_PROPERTY, buf.as_ptr() as *const _, len, buf.as_mut_ptr() as *mut _, len, &mut ret, std::ptr::null_mut()) } != 0;
    if !ok {
        return None;
    }
    let at = 8 + u32::from_le_bytes(buf[24..28].try_into().ok()?) as usize;
    buf.get(at..at + 512).map(|s| s.to_vec())
}

/// NVMe counters are 128-bit; the low 64 bits are plenty.
fn u128_at(log: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(log[at..at + 8].try_into().unwrap())
}

pub fn nvme_judge(log: &[u8], d: &mut Health) {
    let warn = log[0];
    let kelvin = u16::from_le_bytes([log[1], log[2]]) as f64;
    let (spare, spare_min, used) = (log[3], log[4], log[5]);
    d.temp = (kelvin > 273.0 && kelvin < 273.0 + 150.0).then_some(kelvin - 273.0);
    d.health = Some(100u8.saturating_sub(used.min(100)));
    // data units are 1000 × 512 bytes
    d.read = Some(u128_at(log, 32).saturating_mul(512_000));
    d.written = Some(u128_at(log, 48).saturating_mul(512_000));
    d.cycles = Some(u128_at(log, 112));
    d.hours = Some(u128_at(log, 128));
    d.unsafe_shutdowns = Some(u128_at(log, 144));
    d.media_errors = Some(u128_at(log, 160));
    let mut bad = vec![];
    let mut caution = vec![];
    if warn & 0x04 != 0 {
        bad.push("The drive reports its reliability is degraded".to_string());
    }
    if warn & 0x08 != 0 {
        bad.push("The drive has switched itself to read-only".to_string());
    }
    if warn & 0x01 != 0 || (spare_min > 0 && spare < spare_min) {
        bad.push(format!("Spare blocks are below the drive's own limit ({spare}% left, limit {spare_min}%)"));
    }
    if warn & 0x10 != 0 {
        caution.push("The drive's power-loss protection failed".to_string());
    }
    if warn & 0x02 != 0 {
        caution.push("The drive reports it is too hot".to_string());
    }
    if used >= 100 {
        caution.push(format!("The drive has used its rated life ({used}%)"));
    } else if used >= 90 {
        caution.push(format!("{used}% of the drive's rated life is used"));
    }
    if let Some(n) = d.media_errors.filter(|n| *n > 0) {
        caution.push(format!("{} the drive couldn't correct", plural(n, "data error")));
    }
    settle(d, bad, caution);
}

// ---------------------------------------------------------------- SATA / ATA

/// SMART READ DATA (0xD0) and READ THRESHOLDS (0xD1): two 512-byte tables.
fn ata_tables(h: HANDLE, disk: u8) -> Option<(Vec<u8>, Vec<u8>)> {
    Some((ata_cmd(h, disk, 0xD0)?, ata_cmd(h, disk, 0xD1)?))
}

fn ata_cmd(h: HANDLE, disk: u8, feature: u8) -> Option<Vec<u8>> {
    use windows_sys::Win32::System::Ioctl::SMART_RCV_DRIVE_DATA;
    let mut inp = [0u8; 33]; // SENDCMDINPARAMS (packed)
    inp[0..4].copy_from_slice(&512u32.to_le_bytes());
    inp[4..12].copy_from_slice(&[feature, 1, 1, 0x4F, 0xC2, 0xA0 | ((disk & 1) << 4), 0xB0, 0]);
    inp[12] = disk;
    let mut out = vec![0u8; 16 + 512];
    let mut ret = 0u32;
    let ok = unsafe { windows_sys::Win32::System::IO::DeviceIoControl(h, SMART_RCV_DRIVE_DATA, inp.as_ptr() as *const _, 33, out.as_mut_ptr() as *mut _, out.len() as u32, &mut ret, std::ptr::null_mut()) } != 0;
    (ok && out[16..].iter().any(|b| *b != 0)).then(|| out[16..].to_vec())
}

/// SMART RETURN STATUS (0xDA): the drive's own pass / fail. None when it can't say.
fn ata_status(h: HANDLE, disk: u8) -> Option<bool> {
    use windows_sys::Win32::System::Ioctl::SMART_SEND_DRIVE_COMMAND;
    let mut inp = [0u8; 33];
    inp[4..12].copy_from_slice(&[0xDA, 0, 0, 0x4F, 0xC2, 0xA0 | ((disk & 1) << 4), 0xB0, 0]);
    inp[12] = disk;
    let mut out = vec![0u8; 16 + 8];
    let mut ret = 0u32;
    let ok = unsafe { windows_sys::Win32::System::IO::DeviceIoControl(h, SMART_SEND_DRIVE_COMMAND, inp.as_ptr() as *const _, 32, out.as_mut_ptr() as *mut _, out.len() as u32, &mut ret, std::ptr::null_mut()) } != 0;
    if !ok {
        return None;
    }
    // the returned IDEREGS: cylinder low / high = 4F C2 passed, F4 2C threshold exceeded
    match (out[16 + 3], out[16 + 4]) {
        (0x4F, 0xC2) => Some(true),
        (0xF4, 0x2C) => Some(false),
        _ => None,
    }
}

fn attr_name(id: u8) -> &'static str {
    match id {
        1 => "Read error rate",
        3 => "Spin-up time",
        4 => "Start / stop count",
        5 => "Reallocated sectors",
        7 => "Seek error rate",
        9 => "Power-on hours",
        10 => "Spin retry count",
        12 => "Power cycles",
        169 => "Remaining life",
        170 => "Available reserved space",
        171 => "Program fail count",
        172 => "Erase fail count",
        173 => "Wear levelling count",
        174 => "Unexpected power loss",
        177 => "Wear levelling count",
        179 => "Used reserved blocks",
        181 => "Program fail count",
        182 => "Erase fail count",
        183 => "Runtime bad blocks",
        184 => "End-to-end errors",
        187 => "Reported uncorrectable errors",
        188 => "Command timeouts",
        189 => "High fly writes",
        190 => "Airflow temperature",
        191 => "Shock events",
        192 => "Power-off retracts",
        193 => "Load / unload cycles",
        194 => "Temperature",
        195 => "Hardware ECC recovered",
        196 => "Reallocation events",
        197 => "Pending sectors",
        198 => "Uncorrectable sectors",
        199 => "Cable (CRC) errors",
        200 => "Write error rate",
        202 => "Percent lifetime remaining",
        231 => "SSD life left",
        232 => "Available reserved space",
        233 => "Media wearout indicator",
        234 => "Average erase count",
        235 => "Good block count",
        241 => "Total writes",
        242 => "Total reads",
        _ => "Vendor specific",
    }
}

pub fn parse_attrs(data: &[u8], thresholds: &[u8]) -> Vec<Attr> {
    let mut out = vec![];
    for k in 0..30 {
        let a = &data[2 + k * 12..2 + k * 12 + 12];
        if a[0] == 0 {
            continue;
        }
        let threshold = (0..30).map(|j| &thresholds[2 + j * 12..2 + j * 12 + 2]).find(|t| t[0] == a[0]).map(|t| t[1]).unwrap_or(0);
        let raw = a[5..11].iter().rev().fold(0u64, |v, b| (v << 8) | *b as u64);
        out.push(Attr { id: a[0], name: attr_name(a[0]).into(), current: a[3], worst: a[4], threshold, raw });
    }
    out
}

/// An SSD's life left, %, from one of the wear attributes. Some controllers (Silicon Motion,
/// e.g. T-FORCE) leave every normalised value at 100 and put the real figure in the raw value
/// of "remaining life" (169 / 202), as CrystalDiskInfo reads it.
fn life_left(a: &Attr) -> Option<u8> {
    if matches!(a.id, 169 | 202) && a.current == 100 && (1..100).contains(&a.raw) {
        return Some(a.raw as u8);
    }
    (a.current > 0 && a.current <= 100).then_some(a.current)
}

pub fn ata_judge(data: &[u8], thresholds: &[u8], passed: Option<bool>, d: &mut Health) {
    let attrs = parse_attrs(data, thresholds);
    let get = |id: u8| attrs.iter().find(|a| a.id == id);
    d.hours = get(9).map(|a| a.raw & 0xFFFF_FFFF);
    d.cycles = get(12).map(|a| a.raw & 0xFFFF_FFFF);
    d.temp = [194u8, 190].iter().filter_map(|id| get(*id)).map(|a| (a.raw & 0xFF) as f64).find(|t| *t > 0.0 && *t < 120.0);
    if d.ssd {
        d.health = [231u8, 233, 202, 169, 177, 173].iter().filter_map(|id| get(*id)).filter_map(life_left).next();
    }
    let count = |id: u8| get(id).map(|a| a.raw & 0xFFFF_FFFF).unwrap_or(0);
    let (realloc, pending, uncorrectable) = (count(5), count(197), count(198));
    d.bad_sectors = Some(realloc + pending + uncorrectable);
    let mut bad = vec![];
    let mut caution = vec![];
    d.history.clear();
    if passed == Some(false) {
        bad.push("The drive's own health check failed".to_string());
    }
    for a in &attrs {
        // a threshold of 0 means "never fails"; 0xFE / 0xFF are "always passing" markers
        if a.threshold == 0 || a.threshold >= 0xFE {
            continue;
        }
        if a.current > 0 && a.current <= a.threshold {
            bad.push(format!("{} is past the drive's failure limit ({} ≤ {})", a.name, a.current, a.threshold));
        } else if a.worst > 0 && a.worst <= a.threshold {
            // fine now, but its worst value ever went past the limit (smartctl: "failed in the
            // past"): history, not a warning; the drive is judged on how it is now
            d.history.push(if matches!(a.id, 190 | 194) {
                // these count down from 100 as the drive heats up (100 − °C)
                format!("The drive got too hot in the past (up to about {} °C, its limit is about {} °C)", 100 - a.worst as i32, 100 - a.threshold as i32)
            } else {
                format!("{} went past the drive's limit in the past (worst {}, limit {})", a.name, a.worst, a.threshold)
            });
        }
    }
    if realloc > 0 {
        caution.push(format!("{} (worn-out spots the drive replaced)", plural(realloc, "reallocated sector")));
    }
    if pending > 0 {
        caution.push(format!("{} waiting to be reallocated", plural(pending, "sector")));
    }
    if uncorrectable > 0 {
        caution.push(format!("{} that couldn't be read", plural(uncorrectable, "sector")));
    }
    if let Some(hp) = d.health.filter(|h| *h <= 10) {
        caution.push(format!("Only {hp}% of the drive's rated life is left"));
    }
    d.attrs = attrs;
    settle(d, bad, caution);
}

/// "1 sector", "3 sectors"
fn plural(n: u64, what: &str) -> String {
    format!("{n} {what}{}", if n == 1 { "" } else { "s" })
}

fn settle(d: &mut Health, bad: Vec<String>, caution: Vec<String>) {
    d.status = if !bad.is_empty() { "bad" } else if !caution.is_empty() { "caution" } else { "good" }.into();
    d.reasons = bad.into_iter().chain(caution).collect();
}

fn rank(status: &str) -> u8 {
    match status {
        "good" => 1,
        "caution" => 2,
        "bad" => 3,
        _ => 0,
    }
}

/// Past problems the last check didn't know about (told once, then kept in the saved check).
pub fn new_history(old: Option<&Health>, new: &Health) -> Vec<String> {
    new.history.iter().filter(|h| !old.is_some_and(|o| o.history.contains(h))).cloned().collect()
}

/// What got worse since the last check, in words (empty: nothing to warn about).
pub fn worse(old: &Health, new: &Health) -> Vec<String> {
    let mut v = vec![];
    if rank(&new.status) > rank(&old.status) && rank(&old.status) > 0 {
        v.push(format!("Health dropped from {} to {}", old.status, new.status));
    }
    if let (Some(a), Some(b)) = (old.bad_sectors, new.bad_sectors) {
        if b > a {
            v.push(plural(b - a, "new bad sector"));
        }
    }
    if let (Some(a), Some(b)) = (old.media_errors, new.media_errors) {
        if b > a {
            v.push(plural(b - a, "new data error"));
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nvme(warn: u8, spare: u8, spare_min: u8, used: u8, media: u64) -> Vec<u8> {
        let mut log = vec![0u8; 512];
        log[0] = warn;
        log[1..3].copy_from_slice(&(273u16 + 41).to_le_bytes());
        (log[3], log[4], log[5]) = (spare, spare_min, used);
        log[48..56].copy_from_slice(&2_000_000u64.to_le_bytes());
        log[128..136].copy_from_slice(&12_345u64.to_le_bytes());
        log[160..168].copy_from_slice(&media.to_le_bytes());
        log
    }

    #[test]
    fn nvme_health_rules() {
        let mut d = Health::default();
        nvme_judge(&nvme(0, 100, 10, 3, 0), &mut d);
        assert_eq!((d.status.as_str(), d.health, d.temp, d.hours), ("good", Some(97), Some(41.0), Some(12_345)));
        assert_eq!(d.written, Some(1_024_000_000_000), "2 million data units of 512,000 bytes");
        nvme_judge(&nvme(0, 100, 10, 3, 2), &mut d);
        assert_eq!(d.status, "caution", "media errors");
        nvme_judge(&nvme(0, 5, 10, 40, 0), &mut d);
        assert_eq!(d.status, "bad", "spare under the drive's limit");
        nvme_judge(&nvme(0x08, 100, 10, 3, 0), &mut d);
        assert_eq!(d.status, "bad", "read-only");
    }

    fn ata(attrs: &[(u8, u8, u64, u8)]) -> (Vec<u8>, Vec<u8>) {
        let (mut data, mut thr) = (vec![0u8; 512], vec![0u8; 512]);
        for (k, (id, cur, raw, t)) in attrs.iter().enumerate() {
            let a = &mut data[2 + k * 12..2 + k * 12 + 12];
            (a[0], a[3], a[4]) = (*id, *cur, *cur);
            a[5..11].copy_from_slice(&raw.to_le_bytes()[..6]);
            (thr[2 + k * 12], thr[3 + k * 12]) = (*id, *t);
        }
        (data, thr)
    }

    #[test]
    fn sata_health_rules() {
        let mut d = Health { ssd: true, ..Default::default() };
        let (data, thr) = ata(&[(5, 100, 0, 10), (9, 100, 5000, 0), (194, 100, 0x0028_0014_0021, 0), (231, 95, 0, 0)]);
        ata_judge(&data, &thr, Some(true), &mut d);
        assert_eq!((d.status.as_str(), d.hours, d.temp, d.health), ("good", Some(5000), Some(33.0), Some(95)));
        // a Silicon Motion SSD: every value stays 100, the life left is the raw value of 169
        let (data, thr) = ata(&[(169, 100, 98, 50), (9, 100, 5677, 50)]);
        ata_judge(&data, &thr, Some(true), &mut d);
        assert_eq!((d.status.as_str(), d.health), ("good", Some(98)));
        let (data, thr) = ata(&[(5, 100, 8, 10), (197, 100, 1, 0)]);
        ata_judge(&data, &thr, Some(true), &mut d);
        assert_eq!((d.status.as_str(), d.bad_sectors), ("caution", Some(9)));
        let (data, thr) = ata(&[(5, 9, 900, 10)]);
        ata_judge(&data, &thr, Some(true), &mut d);
        assert_eq!(d.status, "bad", "current value at the failure limit");
        let (data, thr) = ata(&[(9, 100, 1, 0)]);
        ata_judge(&data, &thr, Some(false), &mut d);
        assert_eq!(d.status, "bad", "the drive's own check failed");
        // a Seagate that once ran at 68 °C: airflow temperature now 59, worst 32, limit 40
        let (mut data, thr) = ata(&[(190, 59, 41, 40), (5, 100, 0, 10)]);
        data[2 + 4] = 32; // worst
        ata_judge(&data, &thr, Some(true), &mut d);
        assert_eq!(d.status, "good", "healthy now: {:?}", d.reasons);
        assert!(d.history[0].contains("68 °C") && d.history[0].contains("60 °C"), "{:?}", d.history);
        assert_eq!(new_history(None, &d).len(), 1, "told the first time");
        assert!(new_history(Some(&d.clone()), &d).is_empty(), "and not again");
    }

    #[test]
    fn warns_only_when_worse() {
        let base = Health { status: "good".into(), bad_sectors: Some(0), media_errors: None, ..Default::default() };
        assert!(worse(&base, &base).is_empty());
        let w = worse(&base, &Health { status: "caution".into(), bad_sectors: Some(3), ..Default::default() });
        assert_eq!(w.len(), 2, "{w:?}");
        assert!(worse(&Health { status: "unknown".into(), ..Default::default() }, &base).is_empty(), "first reading is not a warning");
    }

    /// Live: prints this PC's drives (needs admin).
    #[test]
    #[ignore]
    fn live_drives() {
        for d in read_all() {
            println!("{} [{}{}] {} health {:?} temp {:?} hours {:?} written {:?} reasons {:?} attrs {}", d.model, d.bus, if d.ssd { " SSD" } else { "" }, d.status, d.health, d.temp, d.hours, d.written, d.reasons, d.attrs.len());
        }
    }
}
