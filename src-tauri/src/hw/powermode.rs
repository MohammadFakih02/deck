//! Power saver / Balanced / Performance / custom watts: the CPU's sustained and burst power
//! limits (Intel RAPL) and Windows' own power mode slider. (The energy preference is a value in
//! each CPU profile's plan.) Balanced puts back exactly what the PC had before Deck changed it.
use crate::hw::rapl::Rapl;
use crate::hw::kv::Settings;

pub const SAVER: i32 = 0;
pub const BALANCED: i32 = 1;
pub const PERFORMANCE: i32 = 2;
/// Deck CPU profiles: the user's own sustained / burst watts (settings PowerPl1 / PowerPl2).
pub const CUSTOM: i32 = 3;
pub const NAMES: [&str; 3] = ["Power saver", "Balanced", "Performance"];

/// Power saver limits in watts: sustained / burst.
const SAVER_PL1: f32 = 15.0;
const SAVER_PL2: f32 = 25.0;
/// Short averaging time so the cap bites at once instead of after the firmware's ~30 s window.
const SAVER_WINDOW: f64 = 2.0;

use crate::profiles::plans::{set_overlay, OVERLAY_BALANCED, OVERLAY_BEST, OVERLAY_EFFICIENCY};

/// Stock power-limit register captured before MoboCtl first changed it (kept in settings so a restart or crash cannot lose it).
fn stock(st: &Settings, r: &Rapl) -> Option<u64> {
    if st.int("PowerStockSet", 0) == 1 {
        return Some((st.int("PowerStockLo", 0) as u32 as u64) | ((st.int("PowerStockHi", 0) as u32 as u64) << 32));
    }
    let raw = r.read()?.raw;
    st.set_int("PowerStockLo", raw as u32 as i32);
    st.set_int("PowerStockHi", (raw >> 32) as u32 as i32);
    st.set_int("PowerStockSet", 1);
    st.save();
    Some(raw)
}

/// Applies `mode`. Returns a one-line description of the result. Safe to call repeatedly (it re-asserts limits the firmware reset).
pub fn apply(mode: i32, st: &Settings, rapl: Option<&Rapl>) -> String {
    let mut parts: Vec<String> = vec![];
    match mode {
        SAVER => {
            if let Some(r) = rapl {
                if stock(st, r).is_some() {
                    match r.set(Some(SAVER_PL1), Some(SAVER_PL2), Some(SAVER_WINDOW)) {
                        Ok(l) => parts.push(format!("CPU limited to {:.0} W sustained / {:.0} W burst", l.pl1, l.pl2)),
                        Err(e) => parts.push(format!("CPU power limit not changed: {}", e)),
                    }
                }
            }
            if set_overlay(OVERLAY_EFFICIENCY) {
                parts.push("Windows: best power efficiency".into());
            }
            st.set_int("PowerTouched", 1);
        }
        CUSTOM => {
            let (pl1, pl2) = (st.int("PowerPl1", 0).max(0) as f32, st.int("PowerPl2", 0).max(0) as f32);
            set_overlay(OVERLAY_BALANCED); // saver / performance may have moved the slider
            if let Some(r) = rapl {
                if let (Some(raw), true) = (stock(st, r), pl1 >= 5.0) {
                    r.restore(raw); // back to the firmware's averaging window first (saver uses 2 s)
                    match r.set(Some(pl1), Some(pl2.max(pl1)), None) {
                        Ok(l) => parts.push(format!("CPU limited to {:.0} W sustained / {:.0} W burst", l.pl1, l.pl2)),
                        Err(e) => parts.push(format!("CPU power limit not changed: {}", e)),
                    }
                }
            }
            st.set_int("PowerTouched", 1);
        }
        PERFORMANCE => {
            if let Some(r) = rapl {
                if let Some(raw) = stock(st, r) {
                    r.restore(raw); // the firmware's own limits are already this CPU's maximum
                    if let Some(l) = r.read() {
                        parts.push(format!("CPU limits {:.0} W sustained / {:.0} W burst", l.pl1, l.pl2));
                    }
                }
            }
            if set_overlay(OVERLAY_BEST) {
                parts.push("Windows: best performance".into());
            }
            st.set_int("PowerTouched", 1);
        }
        _ => {
            // Balanced: only undo what an earlier mode changed
            if st.int("PowerTouched", 0) == 1 {
                if let Some(r) = rapl {
                    if let Some(raw) = stock(st, r) {
                        r.restore(raw);
                    }
                }
                set_overlay(OVERLAY_BALANCED);
                st.set_int("PowerTouched", 0);
            }
            if let Some(l) = rapl.and_then(|r| r.read()) {
                parts.push(format!("CPU limits {:.0} W sustained / {:.0} W burst (stock)", l.pl1, l.pl2));
            }
        }
    }
    st.save();
    if parts.is_empty() {
        "applied".into()
    } else {
        parts.join("  -  ")
    }
}

/// True when the CPU's limits no longer match what `mode` asked for (the firmware or a power-source change reset them).
pub fn drifted(mode: i32, rapl: &Rapl) -> bool {
    mode == SAVER && rapl.read().map(|l| (l.pl1 - SAVER_PL1).abs() > 0.6).unwrap_or(false)
}

/// Same for a custom (Deck profile) limit.
pub fn drifted_custom(pl1: f32, rapl: &Rapl) -> bool {
    pl1 >= 5.0 && rapl.read().map(|l| (l.pl1 - pl1).abs() > 0.6).unwrap_or(false)
}
