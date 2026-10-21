//! The phone's on-screen controller as a real controller on the PC: an Xbox 360 pad through the
//! ViGEmBus driver (games read it like any wired controller). One per phone Screen connection,
//! plugged in on its first press and unplugged when the connection ends.
use serde_json::Value;
use vigem_client::{Client, TargetId, XButtons, XGamepad, Xbox360Wired};

pub struct VPad {
    t: Xbox360Wired<Client>,
}

impl VPad {
    /// Plugs in a new pad. Err: the driver isn't installed (or isn't running).
    pub fn new() -> Result<VPad, String> {
        let client = Client::connect().map_err(|_| "The phone controller needs the ViGEmBus driver on this PC (github.com/nefarius/ViGEmBus/releases).".to_string())?;
        let mut t = Xbox360Wired::new(client, TargetId::XBOX360_WIRED);
        t.plugin().map_err(|e| format!("Couldn't add the controller: {e}"))?;
        // some ViGEmBus setups answer "wait until ready" with an error although the pad works: give
        // it a moment instead
        if t.wait_ready().is_err() {
            std::thread::sleep(std::time::Duration::from_millis(300));
        }
        Ok(VPad { t })
    }

    /// `{"t":"gp","b":buttons,"lx","ly","rx","ry":-1..1 (up = +),"lt","rt":0..1}`
    pub fn update(&mut self, v: &Value) {
        let _ = self.t.update(&report(v));
    }
}

impl Drop for VPad {
    fn drop(&mut self) {
        let _ = self.t.update(&XGamepad::default());
        let _ = self.t.unplug();
    }
}

/// The phone's message as the pad's state.
pub fn report(v: &Value) -> XGamepad {
    let f = |k: &str| v[k].as_f64().unwrap_or(0.0);
    let axis = |k: &str| (f(k).clamp(-1.0, 1.0) * 32767.0).round() as i16;
    let trig = |k: &str| (f(k).clamp(0.0, 1.0) * 255.0).round() as u8;
    XGamepad {
        buttons: XButtons { raw: v["b"].as_u64().unwrap_or(0) as u16 },
        left_trigger: trig("lt"),
        right_trigger: trig("rt"),
        thumb_lx: axis("lx"),
        thumb_ly: axis("ly"),
        thumb_rx: axis("rx"),
        thumb_ry: axis("ry"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn phone_message_to_pad_state() {
        let r = report(&json!({ "t": "gp", "b": 0x1000 | 0x0001, "lx": 1.0, "ly": -1.0, "rx": 0.5, "ry": 9, "lt": 1, "rt": -3 }));
        assert_eq!(r.buttons.raw, 0x1001);
        assert_eq!((r.thumb_lx, r.thumb_ly, r.thumb_rx, r.thumb_ry), (32767, -32767, 16384, 32767));
        assert_eq!((r.left_trigger, r.right_trigger), (255, 0));
        assert_eq!(report(&json!({})), XGamepad::default());
    }

    /// Plugs a pad into this PC (needs ViGEmBus): Windows gives it a controller slot.
    #[test]
    #[ignore = "plugs a virtual controller into this PC"]
    fn plugs_in_here() {
        let mut p = VPad::new().expect("ViGEmBus");
        p.update(&json!({ "b": 0x1000, "lx": 0.5 }));
        let slot = p.t.get_user_index().expect("an XInput slot");
        println!("controller slot {slot}");
        assert!(slot < 4);
    }
}
