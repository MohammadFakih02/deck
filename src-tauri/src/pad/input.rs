//! What the overlay shows: the active controller (SDL3, or XInput without it), scripted demo
//! input, and keyboard & mouse through Windows Raw Input (receive-only, `RIDEV_INPUTSINK`,
//! registered only while keyboard & mouse mode is on).

use super::art;
use super::battery::{xinput, Reading};
use super::sdl;
use std::collections::{HashMap, HashSet};
use std::ffi::c_void;
use std::time::{Duration, Instant};

/// Buttons, PlayStation-named whatever the pad.
pub const BUTTONS: [&str; 16] = ["cross", "circle", "square", "triangle", "up", "down", "left", "right", "l1", "r1", "l3", "r3", "create", "options", "ps", "touchpad"];
pub const AXES: [&str; 6] = ["lx", "ly", "rx", "ry", "l2", "r2"];

/// SDL3's SDL_GamepadButton for each of BUTTONS.
const SDL_BUTTON: [i32; 16] = [0, 1, 2, 3, 11, 12, 13, 14, 9, 10, 7, 8, 4, 6, 5, 20];

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PadState {
    pub b: [bool; 16],
    /// sticks -1..1 (y down), triggers 0..1
    pub ax: [f32; 6],
}

impl PadState {
    pub fn button(&self, name: &str) -> bool {
        BUTTONS.iter().position(|b| *b == name).is_some_and(|i| self.b[i])
    }
    pub fn axis(&self, name: &str) -> f32 {
        AXES.iter().position(|a| *a == name).map_or(0.0, |i| self.ax[i])
    }
    pub fn set(&mut self, name: &str, on: bool) {
        if let Some(i) = BUTTONS.iter().position(|b| *b == name) {
            self.b[i] = on;
        }
    }
    pub fn set_axis(&mut self, name: &str, v: f32) {
        if let Some(i) = AXES.iter().position(|a| *a == name) {
            self.ax[i] = v;
        }
    }
}

/// Is the player touching anything right now? Drives the idle fade.
pub fn is_active(st: &PadState) -> bool {
    st.b.iter().any(|b| *b)
        || st.axis("l2") > 0.05
        || st.axis("r2") > 0.05
        || st.axis("lx").hypot(st.axis("ly")) > art::STICK_DEADZONE
        || st.axis("rx").hypot(st.axis("ry")) > art::STICK_DEADZONE
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct KbmState {
    pub keys: Vec<&'static str>,
    pub buttons: Vec<&'static str>,
    /// -1, 0, 1
    pub wheel: i8,
    /// recent mouse speed
    pub mv: (f32, f32),
}

pub fn kbm_active(st: &KbmState) -> bool {
    !st.keys.is_empty() || !st.buttons.is_empty() || st.wheel != 0 || st.mv.0.hypot(st.mv.1) > 0.5
}

// ------------------------------------------------------------------ the controller

/// The pad the overlay displays: SDL's first mapped game pad, else XInput's first slot.
pub struct Reader {
    sdl: Option<&'static sdl::Api>,
    pad: *mut c_void,
    xinput: Option<u32>,
    pub name: String,
    pub family: &'static str,
    pub vid: u16,
    pub pid: u16,
    /// the HID path SDL reads it through (lower case), when it has one
    pub path: String,
    /// a pad SDL sees but has no mapping for: its name
    pub note: String,
    /// SDL's own battery answer (source 2b), refreshed every 5 s
    pub sdl_battery: Option<Reading>,
    next_connect: Instant,
    next_power: Instant,
    seen: i32,
}

unsafe impl Send for Reader {}

impl Reader {
    pub fn new() -> Reader {
        let now = Instant::now();
        Reader { sdl: sdl::api(), pad: std::ptr::null_mut(), xinput: None, name: "no controller".into(), family: "ps", vid: 0, pid: 0, path: String::new(), note: String::new(), sdl_battery: None, next_connect: now, next_power: now, seen: -1 }
    }

    pub fn connected(&self) -> bool {
        !self.pad.is_null() || self.xinput.is_some()
    }

    fn disconnect(&mut self) {
        if let (Some(api), false) = (self.sdl, self.pad.is_null()) {
            unsafe { (api.CloseGamepad)(self.pad) };
        }
        self.pad = std::ptr::null_mut();
        self.xinput = None;
        self.name = "no controller".into();
        self.vid = 0;
        self.pid = 0;
        self.path.clear();
        self.sdl_battery = None;
    }

    fn connect(&mut self) -> bool {
        match self.sdl {
            Some(api) => {
                // a real pad before a virtual one a tool like DS4Windows made (ViGEm's Xbox 360 pad)
                let mut ids = sdl::ids(api, api.GetGamepads);
                ids.sort_by_key(|id| unsafe { ((api.GetGamepadVendorForID)(*id), (api.GetGamepadProductForID)(*id)) } == (0x045E, 0x028E));
                for id in ids {
                    let p = unsafe { (api.OpenGamepad)(id) };
                    if p.is_null() {
                        continue;
                    }
                    self.pad = p;
                    self.name = Some(sdl::cstr(unsafe { (api.GetGamepadName)(p) })).filter(|s| !s.is_empty()).unwrap_or_else(|| "controller".into());
                    self.vid = unsafe { (api.GetGamepadVendor)(p) };
                    self.pid = unsafe { (api.GetGamepadProduct)(p) };
                    self.path = sdl::cstr(unsafe { (api.GetGamepadPath)(p) }).to_ascii_lowercase();
                    if self.name.starts_with("XInput Controller") || self.name == "controller" {
                        if let Some(n) = super::battery::known_name(self.vid, self.pid) {
                            self.name = n.into();
                        }
                    }
                    self.family = art::family_of(&self.name);
                    self.note.clear();
                    self.seen = -1;
                    self.next_power = Instant::now();
                    return true;
                }
                // something is plugged in that SDL can't map: name it, once per change
                let joys = sdl::ids(api, api.GetJoysticks);
                if joys.len() as i32 != self.seen {
                    self.seen = joys.len() as i32;
                    self.note = joys.first().map(|id| sdl::cstr(unsafe { (api.GetJoystickNameForID)(*id) })).map(|s| if s.is_empty() { "unknown pad".into() } else { s }).unwrap_or_default();
                }
                false
            }
            None => {
                let slot = (0..4).find(|u| xinput::state(*u).is_some());
                if let Some(u) = slot {
                    self.xinput = Some(u);
                    self.name = "Xbox controller".into();
                    self.family = "xbox";
                    self.note.clear();
                }
                slot.is_some()
            }
        }
    }

    /// The pad's state now, or None with no pad attached.
    pub fn read(&mut self) -> Option<PadState> {
        let now = Instant::now();
        if let Some(api) = self.sdl {
            unsafe { (api.UpdateGamepads)() };
            if !self.pad.is_null() && !unsafe { (api.GamepadConnected)(self.pad) } {
                self.disconnect();
            }
        }
        if !self.connected() {
            if now < self.next_connect {
                return None;
            }
            self.next_connect = now + Duration::from_secs(1);
            if !self.connect() {
                return None;
            }
        }
        let mut st = PadState::default();
        if let Some(api) = self.sdl.filter(|_| !self.pad.is_null()) {
            for (i, b) in SDL_BUTTON.iter().enumerate() {
                st.b[i] = unsafe { (api.GetGamepadButton)(self.pad, *b) };
            }
            for i in 0..6 {
                let raw = unsafe { (api.GetGamepadAxis)(self.pad, i as i32) } as f32 / 32767.0;
                st.ax[i] = if i >= 4 { raw.clamp(0.0, 1.0) } else { raw.clamp(-1.0, 1.0) };
            }
            if now >= self.next_power {
                self.next_power = now + Duration::from_secs(5);
                let mut pct = -1;
                let state = unsafe { (api.GetGamepadPowerInfo)(self.pad, &mut pct) };
                let percent = (0..=100).contains(&pct).then_some(pct as u8);
                self.sdl_battery = match state {
                    1 => percent.map(|p| Reading { percent: Some(p), level: None, charging: false }),
                    3 => Some(Reading { percent, level: None, charging: true }),
                    4 => Some(Reading { percent: Some(100), level: None, charging: false }),
                    _ => None,
                };
            }
        } else if let Some(u) = self.xinput {
            let Some(x) = xinput::state(u) else {
                self.disconnect();
                return None;
            };
            let w = u16::from_le_bytes([x[4], x[5]]);
            let bits: [(usize, u16); 14] = [(0, 0x1000), (1, 0x2000), (2, 0x4000), (3, 0x8000), (4, 0x1), (5, 0x2), (6, 0x4), (7, 0x8), (8, 0x100), (9, 0x200), (10, 0x40), (11, 0x80), (12, 0x20), (13, 0x10)];
            for (i, m) in bits {
                st.b[i] = w & m != 0;
            }
            let s = |o: usize| i16::from_le_bytes([x[o], x[o + 1]]) as f32 / 32767.0;
            st.ax = [s(8).clamp(-1.0, 1.0), (-s(10)).clamp(-1.0, 1.0), s(12).clamp(-1.0, 1.0), (-s(14)).clamp(-1.0, 1.0), x[6] as f32 / 255.0, x[7] as f32 / 255.0];
        }
        Some(st)
    }
}

impl Drop for Reader {
    fn drop(&mut self) {
        self.disconnect();
    }
}

// ------------------------------------------------------------------ demo input

const DEMO_SEQ: [&str; 14] = ["cross", "circle", "triangle", "square", "up", "right", "down", "left", "l1", "r1", "create", "options", "touchpad", "ps"];

/// Scripted input, so the overlay can be seen and styled without a pad: buttons one after
/// another, then sticks and triggers, then a combo. `t` = seconds since demo mode started.
pub fn demo(t: f32) -> PadState {
    let t = t % 12.0;
    let mut st = PadState::default();
    if t < 5.0 {
        st.set(DEMO_SEQ[((t / 5.0 * DEMO_SEQ.len() as f32) as usize).min(DEMO_SEQ.len() - 1)], true);
    } else if t < 9.0 {
        let a = (t - 5.0) * std::f32::consts::PI;
        st.ax = [0.95 * a.cos(), 0.95 * a.sin(), 0.9 * (2.0 * a).sin(), -0.9 * a.cos(), (1.0 - a.cos()) / 2.0, (1.0 + a.sin()) / 2.0];
    } else {
        let u = t - 9.0;
        st.set_axis("ly", -1.0);
        st.set_axis("lx", 0.3 * (u * 4.0).sin());
        st.set_axis("r2", (u / 1.5).min(1.0));
        st.set("circle", 0.8 < u && u < 1.1);
        st.set("r1", (u * 4.0) as i32 % 2 == 0);
        st.set("l3", u > 2.0);
        st.set("r3", 2.4 < u && u < 2.9);
    }
    st
}

/// Scripted keyboard & mouse: walking with WASD, sprinting, key presses, clicks, scrolling and
/// a mouse circling.
pub fn demo_kbm(t: f32) -> KbmState {
    let t = t % 10.0;
    let mut keys = vec![["w", "w", "a", "w", "d", "s", "w", "w"][(t / 1.25) as usize % 8]];
    if 2.0 < t && t < 4.0 {
        keys.push("shift");
    }
    if 5.0 < t && t < 9.0 {
        keys.push(["q", "e", "r", "f", "1", "2", "tab", "space", "ctrl", "c"][((t - 5.0) / 0.4) as usize % 10]);
    }
    let mut buttons = Vec::new();
    if (t * 3.0) as i32 % 5 == 0 {
        buttons.push("left");
    }
    for (name, a, b) in [("right", 6.0, 7.0), ("middle", 3.0, 3.3), ("x1", 8.0, 8.3), ("x2", 8.5, 8.8)] {
        if a < t && t < b {
            buttons.push(name);
        }
    }
    let wheel = if 4.2 < t && t < 4.8 { 1 } else if 4.9 < t && t < 5.5 { -1 } else { 0 };
    let a = t * 2.2;
    let speed = 40.0 * (0.5 + 0.5 * (t * 1.3).sin());
    KbmState { keys, buttons, wheel, mv: (speed * a.cos(), speed * a.sin()) }
}

// ------------------------------------------------------------------ keyboard & mouse

/// (name, down flag, up flag) in RAWMOUSE.usButtonFlags.
pub const MOUSE_FLAGS: [(&str, u16, u16); 5] = [("left", 0x0001, 0x0002), ("right", 0x0004, 0x0008), ("middle", 0x0010, 0x0020), ("x1", 0x0040, 0x0080), ("x2", 0x0100, 0x0200)];
const RI_MOUSE_WHEEL: u16 = 0x0400;
const TAP: Duration = Duration::from_millis(90);
const WHEEL: Duration = Duration::from_millis(150);

/// Keys and mouse buttons held now, quick taps kept on screen a moment, and the mouse's speed.
/// Nothing is stored beyond that.
pub struct Kbm {
    pub active: bool,
    held: HashSet<&'static str>,
    until: HashMap<&'static str, Instant>,
    dx: f32,
    dy: f32,
    vx: f32,
    vy: f32,
    wheel: (Instant, i8),
}

impl Default for Kbm {
    fn default() -> Self {
        Kbm { active: false, held: HashSet::new(), until: HashMap::new(), dx: 0.0, dy: 0.0, vx: 0.0, vy: 0.0, wheel: (Instant::now(), 0) }
    }
}

impl Kbm {
    /// Registers (or drops) raw keyboard + mouse input for `hwnd`.
    pub fn enable(&mut self, hwnd: windows_sys::Win32::Foundation::HWND, on: bool) {
        use windows_sys::Win32::UI::Input::{RegisterRawInputDevices, RAWINPUTDEVICE, RIDEV_INPUTSINK, RIDEV_REMOVE};
        if on == self.active {
            return;
        }
        let (flags, target) = if on { (RIDEV_INPUTSINK, hwnd) } else { (RIDEV_REMOVE, std::ptr::null_mut()) };
        let devs = [
            RAWINPUTDEVICE { usUsagePage: 1, usUsage: 2, dwFlags: flags, hwndTarget: target },
            RAWINPUTDEVICE { usUsagePage: 1, usUsage: 6, dwFlags: flags, hwndTarget: target },
        ];
        if unsafe { RegisterRawInputDevices(devs.as_ptr(), 2, std::mem::size_of::<RAWINPUTDEVICE>() as u32) } == 0 {
            crate::log::warn(format!("controller overlay: raw input {} failed", if on { "start" } else { "stop" }));
            return;
        }
        *self = Kbm { active: on, ..Kbm::default() };
    }

    fn press(&mut self, name: &'static str, down: bool) {
        if down {
            self.held.insert(name);
            self.until.insert(name, Instant::now() + TAP);
        } else {
            self.held.remove(name);
        }
    }

    pub fn key(&mut self, vk: u16, up: bool) {
        if let Some(k) = super::cfg::KEYS.iter().find(|k| k.2 == vk) {
            self.press(k.0, !up);
        }
    }

    pub fn mouse(&mut self, flags: u16, buttons: u16, data: u16, dx: i32, dy: i32) {
        if flags & 1 == 0 {
            // relative movement (absolute = tablets / remote desktop: skipped)
            self.dx += dx as f32;
            self.dy += dy as f32;
        }
        if buttons != 0 {
            for (name, down, up) in MOUSE_FLAGS {
                if buttons & down != 0 {
                    self.press(name, true);
                }
                if buttons & up != 0 {
                    self.press(name, false);
                }
            }
            if buttons & RI_MOUSE_WHEEL != 0 {
                let step = data as i16;
                if step != 0 {
                    self.wheel = (Instant::now() + WHEEL, if step > 0 { 1 } else { -1 });
                }
            }
        }
    }

    /// One WM_INPUT message.
    pub fn on_input(&mut self, lparam: isize) {
        use windows_sys::Win32::UI::Input::{GetRawInputData, RAWINPUT, RAWINPUTHEADER, RID_INPUT, RIM_TYPEKEYBOARD, RIM_TYPEMOUSE};
        let mut ri: RAWINPUT = unsafe { std::mem::zeroed() };
        let mut size = std::mem::size_of::<RAWINPUT>() as u32;
        let n = unsafe { GetRawInputData(lparam as _, RID_INPUT, &mut ri as *mut _ as *mut c_void, &mut size, std::mem::size_of::<RAWINPUTHEADER>() as u32) };
        if n == u32::MAX || n == 0 {
            return;
        }
        unsafe {
            if ri.header.dwType == RIM_TYPEMOUSE {
                let m = ri.data.mouse;
                self.mouse(m.usFlags, m.Anonymous.Anonymous.usButtonFlags, m.Anonymous.Anonymous.usButtonData, m.lLastX, m.lLastY);
            } else if ri.header.dwType == RIM_TYPEKEYBOARD {
                let k = ri.data.keyboard;
                self.key(k.VKey, k.Flags & 1 != 0);
            }
        }
    }

    pub fn read(&mut self) -> KbmState {
        let now = Instant::now();
        let mut shown: HashSet<&'static str> = self.held.clone();
        shown.extend(self.until.iter().filter(|(_, t)| **t > now).map(|(n, _)| *n));
        self.until.retain(|_, t| *t > now);
        // smoothed speed: follows the mouse quickly, settles in ~0.2 s at rest
        self.vx = self.vx * 0.55 + self.dx;
        self.vy = self.vy * 0.55 + self.dy;
        self.dx = 0.0;
        self.dy = 0.0;
        let wheel = if self.wheel.0 > now { self.wheel.1 } else { 0 };
        let buttons: Vec<&'static str> = art::MOUSE_BUTTONS.iter().copied().filter(|b| shown.contains(b)).collect();
        let mut keys: Vec<&'static str> = shown.into_iter().filter(|n| !buttons.contains(n)).collect();
        keys.sort();
        KbmState { keys, buttons, wheel, mv: (self.vx, self.vy) }
    }

    /// A key released while another desktop had the input (UAC, the lock screen) never reports
    /// its release: drop keys that aren't really down.
    pub fn resync(&mut self) {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
        use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_SWAPBUTTON};
        let swap = unsafe { GetSystemMetrics(SM_SWAPBUTTON) } != 0;
        let mouse_vk = |n: &str| match n {
            // raw input reports physical buttons, the key state logical ones
            "left" => Some(if swap { 0x02 } else { 0x01 }),
            "right" => Some(if swap { 0x01 } else { 0x02 }),
            "middle" => Some(0x04),
            "x1" => Some(0x05),
            "x2" => Some(0x06),
            _ => None,
        };
        let held: Vec<&'static str> = self.held.iter().copied().collect();
        for name in held {
            let vk = super::cfg::key_info(name).map(|k| k.2 as i32).or_else(|| mouse_vk(name));
            if let Some(vk) = vk {
                if unsafe { GetAsyncKeyState(vk) } as u16 & 0x8000 == 0 {
                    self.held.remove(name);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_covers_every_button() {
        let mut seen = HashSet::new();
        for i in 0..1200 {
            let st = demo(i as f32 / 100.0);
            for b in BUTTONS {
                if st.button(b) {
                    seen.insert(b);
                }
            }
        }
        for b in BUTTONS {
            assert!(seen.contains(b), "{b}");
        }
    }

    #[test]
    fn deadzone_centres_small_moves() {
        let mut st = PadState::default();
        st.set_axis("lx", 0.1);
        assert!(!is_active(&st));
        st.set_axis("lx", 0.2);
        assert!(is_active(&st));
    }

    #[test]
    fn raw_mouse_flags_map() {
        let mut k = Kbm::default();
        k.mouse(0, 0x0001 | 0x0400, 120, 5, -3);
        let st = k.read();
        assert_eq!(st.buttons, vec!["left"]);
        assert_eq!(st.wheel, 1);
        assert_eq!(st.mv, (5.0, -3.0));
        k.mouse(0, 0x0002, 0, 0, 0);
        k.key(0x57, false);
        k.mouse(1, 0, 0, 900, 900);
        std::thread::sleep(TAP + Duration::from_millis(10));
        let st = k.read();
        assert!(st.buttons.is_empty());
        assert_eq!(st.keys, vec!["w"]);
        assert!(st.mv.0 < 5.0, "absolute moves are skipped");
        k.key(0x57, true);
        std::thread::sleep(TAP + Duration::from_millis(10));
        assert!(k.read().keys.is_empty());
    }

    #[test]
    fn demo_kbm_moves() {
        assert!((0..100).any(|i| kbm_active(&demo_kbm(i as f32 / 10.0))));
    }
}
