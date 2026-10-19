//! Windows.Gaming.Input: the battery report Windows itself keeps for the controllers it knows
//! (Xbox controllers above all). Remaining / full charge in milliwatt-hours → percent.

use super::{Entry, Reading};

/// The report's numbers as a reading: None when there is no battery (wired, virtual, unknown).
/// `status`: 0 not present, 1 discharging, 2 idle, 3 charging (Windows.System.Power.BatteryStatus).
pub fn to_reading(remaining: Option<i32>, full: Option<i32>, status: i32) -> Option<Reading> {
    if status == 0 {
        return None;
    }
    let (r, f) = (remaining?, full?);
    if f <= 0 || r < 0 {
        return None;
    }
    let percent = ((r as f64 / f as f64) * 100.0).round().clamp(0.0, 100.0) as u8;
    Some(Reading { percent: Some(percent), level: None, charging: status == 3 })
}

pub fn poll() -> Vec<Entry> {
    use windows::Gaming::Input::RawGameController;
    crate::util::com();
    let mut out = Vec::new();
    let Ok(list) = RawGameController::RawGameControllers() else { return out };
    for c in list {
        let (Ok(vid), Ok(pid)) = (c.HardwareVendorId(), c.HardwareProductId()) else { continue };
        let Ok(rep) = c.TryGetBatteryReport() else { continue };
        let remaining = rep.RemainingCapacityInMilliwattHours().ok().and_then(|v| v.Value().ok());
        let full = rep.FullChargeCapacityInMilliwattHours().ok().and_then(|v| v.Value().ok());
        let status = rep.Status().map(|s| s.0).unwrap_or(0);
        let Some(reading) = to_reading(remaining, full, status) else { continue };
        let id = c.NonRoamableId().map(|s| s.to_string()).unwrap_or_default();
        let name = c.DisplayName().map(|s| s.to_string()).unwrap_or_default();
        out.push(Entry {
            id: format!("wgi:{}", id.to_ascii_lowercase()),
            name: if name.is_empty() { "Controller".into() } else { name },
            vid,
            pid,
            source: "Windows",
            reading,
            path: String::new(),
            // a ViGEm virtual Xbox 360 pad (and a wired 360 pad, which has no battery anyway)
            virtual_pad: (vid, pid) == (0x045E, 0x028E),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wgi_report_to_percent() {
        assert_eq!(to_reading(Some(1500), Some(3000), 1), Some(Reading { percent: Some(50), level: None, charging: false }));
        assert!(to_reading(Some(3000), Some(3000), 3).unwrap().charging);
        assert_eq!(to_reading(Some(10), Some(3000), 0), None);
        assert_eq!(to_reading(None, Some(3000), 1), None);
        assert_eq!(to_reading(Some(10), Some(0), 1), None);
    }
}
