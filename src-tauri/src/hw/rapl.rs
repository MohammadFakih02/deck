//! CPU package power limits (Intel RAPL, MSR 0x610): the sustained limit PL1 and the short-burst limit PL2.
//! Used for the power-saver / performance modes. Needs the PawnIO driver; does nothing where the CPU or firmware locks the register.
use crate::hw::pawn::{Module, INTEL_MSR};

const MSR_UNITS: i64 = 0x606;
const MSR_PKG_LIMIT: i64 = 0x610;

pub struct Rapl {
    m: Module,
    unit: f64, // watts per register step
    time_unit: f64, // seconds per time-window step
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Limits {
    pub pl1: f32,
    pub pl2: f32,
    pub locked: bool,
    pub raw: u64,
}

/// The power-limit register as watts (`unit` = watts per register step).
fn limits_from(raw: u64, unit: f64) -> Limits {
    Limits { pl1: ((raw & 0x7FFF) as f64 * unit) as f32, pl2: (((raw >> 32) & 0x7FFF) as f64 * unit) as f32, locked: raw >> 63 != 0, raw }
}

impl Rapl {
    pub fn open() -> Option<Rapl> {
        let (vendor, _, _) = crate::hw::sensors::cpu_id();
        if !matches!(vendor, crate::hw::sensors::Vendor::Intel) {
            return None;
        }
        let m = Module::load(INTEL_MSR).ok()?;
        let u = m.exec("ioctl_read_msr", &[MSR_UNITS], 1).ok()?[0] as u64;
        let unit = 1.0 / (1u64 << (u & 0xF)) as f64;
        let time_unit = 1.0 / (1u64 << ((u >> 16) & 0xF)) as f64;
        m.exec("ioctl_read_msr", &[MSR_PKG_LIMIT], 1).ok()?;
        Some(Rapl { m, unit, time_unit })
    }

    pub fn read(&self) -> Option<Limits> {
        let raw = self.m.exec("ioctl_read_msr", &[MSR_PKG_LIMIT], 1).ok()?[0] as u64;
        Some(limits_from(raw, self.unit))
    }

    /// A raw register value (such as the stock one kept in settings) as watts.
    pub fn decode(&self, raw: u64) -> Limits {
        limits_from(raw, self.unit)
    }

    /// Time-window field (bits 21:17 = Y, 23:22 = Z; seconds = 2^Y * (1 + Z/4) * unit) closest to `secs`, already shifted into place.
    fn window_bits(&self, secs: f64) -> u64 {
        let mut best = (f64::MAX, 0u64);
        for y in 0..32u64 {
            for z in 0..4u64 {
                let t = (1u64 << y) as f64 * (1.0 + z as f64 / 4.0) * self.time_unit;
                let d = (t - secs).abs();
                if d < best.0 {
                    best = (d, (y << 17) | (z << 22));
                }
            }
        }
        best.1
    }

    /// Sets PL1 / PL2 in watts (None = leave as is), keeping every other bit (enable, clamp). `window` = PL1 averaging time in seconds
    /// (a short one makes the cap act at once; the stock ~30 s lets short bursts through). Returns the value read back.
    pub fn set(&self, pl1: Option<f32>, pl2: Option<f32>, window: Option<f64>) -> Result<Limits, String> {
        let cur = self.read().ok_or("cannot read the power limits")?;
        if cur.locked {
            return Err("the CPU's power limits are locked by the firmware".into());
        }
        let step = |w: f32| ((w as f64 / self.unit).round() as u64).clamp(1, 0x7FFF);
        let mut v = cur.raw;
        if let Some(w) = pl1 {
            v = (v & !0x7FFF) | step(w) | (1 << 15); // value + enable
        }
        if let Some(s) = window {
            v = (v & !(0x7Fu64 << 17)) | self.window_bits(s);
        }
        if let Some(w) = pl2 {
            v = (v & !(0x7FFFu64 << 32)) | (step(w) << 32) | (1u64 << 47);
        }
        self.m.exec("ioctl_write_msr", &[MSR_PKG_LIMIT, v as i64], 0).map_err(|e| format!("write refused (0x{:X})", e))?;
        self.read().ok_or_else(|| "cannot read back".to_string())
    }

    /// Puts a previously read raw register value back.
    pub fn restore(&self, raw: u64) {
        let _ = self.m.exec("ioctl_write_msr", &[MSR_PKG_LIMIT, raw as i64], 0);
    }
}

#[cfg(test)]
mod tests {
    /// The register an MSI B660M sets for an i5-14600KF: 130 W sustained, 130 W burst, units of 1/8 W.
    #[test]
    fn limits_decode_from_the_register() {
        let l = super::limits_from(0x0042_8410_00DF_8410, 0.125);
        assert_eq!((l.pl1, l.pl2, l.locked), (130.0, 130.0, false));
    }
}
