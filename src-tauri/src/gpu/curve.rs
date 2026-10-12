//! The GPU's voltage/frequency curve, the way MSI Afterburner edits it: every point has a
//! voltage, a stock ("base") clock and a delta. A plain core offset is the same delta on every
//! core point; an undervolt flattens the curve from one voltage up. The memory clock offset is
//! a delta on the editable memory points that follow the core points in the same table.

use super::nvapi::{self, NvApi};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Pt {
    /// index in the driver's table
    pub index: usize,
    pub mv: f32,
    pub base_mhz: f32,
    pub delta_mhz: f32,
    pub editable: bool,
}

impl Pt {
    pub fn mhz(&self) -> f32 {
        self.base_mhz + self.delta_mhz
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Curve {
    pub core: Vec<Pt>,
    pub mem: Vec<Pt>,
}

/// Status buffer: entries of 7 u32 {freq kHz, voltage µV, ...} from 0x48.
const ST_BASE: usize = 0x48;
const ST_STRIDE: usize = 28;
/// Control buffer: entries of 9 u32 {type, ..., delta kHz at +20, ...} from 0x44.
const CT_BASE: usize = 0x44;
const CT_STRIDE: usize = 36;
const CT_DELTA: usize = 20;

fn bits(mask: &[u8]) -> Vec<usize> {
    (0..mask.len() * 8).filter(|i| mask[i / 8] & (1 << (i % 8)) != 0).collect()
}

/// Builds the curve from the raw status and control buffers (separated for tests).
pub fn parse(mask: &[u8], status: &[u8], control: &[u8]) -> Curve {
    let mut c = Curve::default();
    let mut core_done = false;
    let mut last_mv = 0.0f32;
    for i in bits(mask) {
        let so = ST_BASE + i * ST_STRIDE;
        let co = CT_BASE + i * CT_STRIDE;
        if so + 8 > status.len() || co + CT_STRIDE > control.len() {
            break;
        }
        let khz = nvapi::u32_at(status, so) as f32;
        let mv = nvapi::u32_at(status, so + 4) as f32 / 1000.0;
        let delta = nvapi::i32_at(control, co + CT_DELTA) as f32 / 1000.0;
        let editable = nvapi::u32_at(control, co) == 0;
        let p = Pt { index: i, mv, base_mhz: khz / 1000.0 - delta, delta_mhz: delta, editable };
        // the core points come first with rising voltages; the first drop starts the memory points
        if !core_done && mv > 0.0 && mv >= last_mv {
            last_mv = mv;
            c.core.push(p);
        } else {
            core_done = true;
            c.mem.push(p);
        }
    }
    c
}

pub fn read(a: &NvApi) -> Result<Curve, String> {
    let mask = a.vf_mask().map_err(|e| format!("curve mask: NvAPI error {e}"))?;
    let (_, status) = a.vf_curve_raw().map_err(|e| format!("curve: NvAPI error {e}"))?;
    let control = a.get_masked(nvapi::VF_TABLE_GET, nvapi::VF_TABLE_SIZE, 1, &mask).map_err(|e| format!("curve table: NvAPI error {e}"))?;
    Ok(parse(&mask, &status, &control))
}

/// Writes every delta of `c` (read-modify-write of the driver's own table, which keeps the
/// fields we don't know), then reads back. The driver snaps clocks to 15 MHz steps.
pub fn write(a: &NvApi, c: &Curve) -> Result<(), String> {
    let mask = a.vf_mask().map_err(|e| format!("curve mask: NvAPI error {e}"))?;
    let mut table = a.get_masked(nvapi::VF_TABLE_GET, nvapi::VF_TABLE_SIZE, 1, &mask).map_err(|e| format!("curve table: NvAPI error {e}"))?;
    for p in c.core.iter().chain(c.mem.iter().filter(|p| p.editable)) {
        let o = CT_BASE + p.index * CT_STRIDE + CT_DELTA;
        if o + 4 <= table.len() {
            table[o..o + 4].copy_from_slice(&((p.delta_mhz * 1000.0).round() as i32).to_le_bytes());
        }
    }
    a.set_raw(nvapi::VF_TABLE_SET, &mut table).map_err(|e| format!("The driver refused the curve (NvAPI error {e})."))?;
    let back = read(a)?;
    let bad = c.core.iter().chain(c.mem.iter().filter(|p| p.editable)).filter(|p| {
        let b = back.core.iter().chain(back.mem.iter()).find(|q| q.index == p.index);
        b.map(|b| (b.delta_mhz - p.delta_mhz).abs() > 16.0).unwrap_or(true)
    });
    match bad.count() {
        0 => Ok(()),
        n => Err(format!("The driver changed {n} curve points (they may be outside its limits).")),
    }
}

/// The card reports each point's clock after its own limits (a flattened curve is clamped), so
/// clock − delta can come out below the real stock clock near the knee. Stock curves never fall
/// with rising voltage: lift such points to the one before.
pub fn repair_base(c: &mut Curve) {
    let mut prev = 0.0f32;
    for p in &mut c.core {
        if p.base_mhz < prev {
            p.base_mhz = prev;
        }
        prev = p.base_mhz;
    }
}

/// All core deltas are 0: the reported clocks are the stock curve.
pub fn at_stock(c: &Curve) -> bool {
    !c.core.is_empty() && c.core.iter().all(|p| p.delta_mhz.abs() < 0.5)
}

/// Every core point gets the same delta (a plain core clock offset).
pub fn set_offset(c: &mut Curve, mhz: f32) {
    for p in &mut c.core {
        p.delta_mhz = mhz;
    }
}

/// Memory clock offset on the editable memory points.
pub fn set_mem_offset(c: &mut Curve, mhz: f32) {
    for p in c.mem.iter_mut().filter(|p| p.editable) {
        p.delta_mhz = mhz;
    }
}

/// Afterburner's undervolt: from `mv` up the clock is held at `mhz`; below it no point may be
/// faster than `mhz`.
#[cfg_attr(not(test), allow(dead_code))]
pub fn flatten(c: &mut Curve, mv: f32, mhz: f32) {
    for p in &mut c.core {
        if p.mv >= mv - 0.01 || p.mhz() > mhz {
            p.delta_mhz = mhz - p.base_mhz;
        }
    }
}

/// The core offset a curve amounts to when every delta is the same, else None (a shaped curve).
pub fn uniform_offset(c: &Curve) -> Option<f32> {
    let first = c.core.first()?.delta_mhz;
    c.core.iter().all(|p| (p.delta_mhz - first).abs() < 0.5).then_some(first)
}

// ------------------------------------------------------------- Afterburner's VFCurve

/// One point of Afterburner's saved curve: voltage, stock clock and delta.
#[derive(Clone, Debug, PartialEq)]
pub struct AbPt {
    pub mv: f32,
    pub mhz: f32,
    pub delta: f32,
}

/// Decodes `VFCurve=` from an Afterburner profile: 12-byte header (version, count, 0) then
/// `count` points of three little-endian floats {mV, MHz, delta MHz}; the rest is padding.
pub fn decode_ab(hex: &str) -> Option<Vec<AbPt>> {
    let hex = hex.trim();
    if hex.len() < 24 || hex.len() % 2 != 0 {
        return None;
    }
    let bytes: Vec<u8> = (0..hex.len() / 2).map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16)).collect::<Result<_, _>>().ok()?;
    let n = nvapi::u32_at(&bytes, 4) as usize;
    if n == 0 || n > 512 || bytes.len() < 12 + n * 12 {
        return None;
    }
    let f = |o: usize| f32::from_le_bytes([bytes[o], bytes[o + 1], bytes[o + 2], bytes[o + 3]]);
    Some((0..n).map(|i| AbPt { mv: f(12 + i * 12), mhz: f(16 + i * 12), delta: f(20 + i * 12) }).collect())
}

/// Puts Afterburner's deltas onto this GPU's core points (matched by voltage).
pub fn apply_ab(c: &mut Curve, ab: &[AbPt]) {
    for p in &mut c.core {
        if let Some(q) = ab.iter().min_by(|a, b| (a.mv - p.mv).abs().partial_cmp(&(b.mv - p.mv).abs()).unwrap()) {
            if (q.mv - p.mv).abs() < 3.2 {
                p.delta_mhz = q.delta;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_never_falls() {
        // what the card reports for a curve flattened at 990 MHz: 637 mV comes out at 780
        let mut c = Curve { core: vec![pt(0, 618.8, 915.0, 0.0), pt(1, 637.5, 780.0, 210.0), pt(2, 656.2, 900.0, 90.0), pt(3, 675.0, 1020.0, -30.0)], mem: vec![] };
        repair_base(&mut c);
        let b: Vec<f32> = c.core.iter().map(|p| p.base_mhz).collect();
        assert_eq!(b, vec![915.0, 915.0, 915.0, 1020.0]);
        assert!(!at_stock(&c));
    }

    fn pt(i: usize, mv: f32, base: f32, d: f32) -> Pt {
        Pt { index: i, mv, base_mhz: base, delta_mhz: d, editable: true }
    }

    #[test]
    fn parse_splits_core_and_memory() {
        let mut mask = vec![0u8; 32];
        mask[0] = 0b1111; // points 0..3
        let mut st = vec![0u8; 0x48 + 4 * 28];
        let mut ct = vec![0u8; 0x44 + 4 * 36];
        let put = |b: &mut Vec<u8>, o: usize, v: u32| b[o..o + 4].copy_from_slice(&v.to_le_bytes());
        // two core points rising in voltage, then a memory point with a lower voltage
        put(&mut st, 0x48, 210_000);
        put(&mut st, 0x48 + 4, 450_000);
        put(&mut st, 0x48 + 28, 990_000);
        put(&mut st, 0x48 + 28 + 4, 1_237_500);
        put(&mut ct, 0x44 + 36 + 20, (-1_005_000i32) as u32);
        put(&mut st, 0x48 + 56, 6_501_000);
        put(&mut st, 0x48 + 56 + 4, 750_000);
        put(&mut ct, 0x44 + 72 + 20, (-500_000i32) as u32);
        put(&mut st, 0x48 + 84, 405_000);
        put(&mut st, 0x48 + 84 + 4, 650_000);
        put(&mut ct, 0x44 + 108, 1); // not editable
        let c = parse(&mask, &st, &ct);
        assert_eq!(c.core.len(), 2);
        assert_eq!(c.mem.len(), 2);
        assert_eq!(c.core[1].base_mhz, 1995.0);
        assert_eq!(c.core[1].mhz(), 990.0);
        assert_eq!(c.mem[0].base_mhz, 7001.0);
        assert!(c.mem[0].editable && !c.mem[1].editable);
    }

    #[test]
    fn flatten_holds_clock_from_voltage() {
        let mut c = Curve { core: vec![pt(0, 600.0, 900.0, 0.0), pt(1, 625.0, 1000.0, 0.0), pt(2, 700.0, 1400.0, 0.0), pt(3, 800.0, 1800.0, 0.0)], mem: vec![] };
        flatten(&mut c, 625.0, 975.0);
        let clocks: Vec<f32> = c.core.iter().map(Pt::mhz).collect();
        assert_eq!(clocks, vec![900.0, 975.0, 975.0, 975.0]);
        assert_eq!(uniform_offset(&c), None);
        set_offset(&mut c, 150.0);
        assert_eq!(uniform_offset(&c), Some(150.0));
    }

    #[test]
    fn decode_afterburner_curve() {
        // header (0x00020000, 2 points, 0) + {450 mV, 210 MHz, 0} + {456.25 mV, 210 MHz, 150}
        let mut b = vec![];
        for v in [0x0002_0000u32, 2, 0] {
            b.extend(v.to_le_bytes());
        }
        for f in [450.0f32, 210.0, 0.0, 456.25, 210.0, 150.0] {
            b.extend(f.to_le_bytes());
        }
        b.extend([0u8; 8]); // padding Afterburner leaves after the points
        let hex: String = b.iter().map(|x| format!("{x:02X}")).collect();
        let pts = decode_ab(&hex).unwrap();
        assert_eq!(pts.len(), 2);
        assert_eq!(pts[1], AbPt { mv: 456.25, mhz: 210.0, delta: 150.0 });
        assert!(decode_ab("zz").is_none());
    }

    #[test]
    fn decode_real_afterburner_prefix() {
        // the start of this PC's [Profile1] VFCurve: 127 points, first at 450 mV / 210 MHz / +150
        let hex = "000002007F000000000000000000E14300005243000016430020E443000052430000164300";
        let full = format!("{hex}{}", "00".repeat(127 * 12));
        let pts = decode_ab(&full).unwrap();
        assert_eq!(pts.len(), 127);
        assert_eq!(pts[0], AbPt { mv: 450.0, mhz: 210.0, delta: 150.0 });
        assert_eq!(pts[1].mv, 456.25);
    }
}
