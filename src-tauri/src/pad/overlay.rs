//! The overlay thread: owns the overlay and toast windows, the raw-input window and the frame
//! clock. The clock is a high-resolution waitable timer the thread sleeps on together with its
//! messages: 60 Hz while something moves, 30 Hz faded, 20 Hz hidden, and no timer at all while
//! the overlay is off (the thread then only wakes for a command).

use super::art::{self, Art, Frame, Look};
use super::battery::Reading;
use super::cfg::PadSettings;
use super::input::{self, KbmState, PadState, Reader};
use super::place::{self, Monitor};
use super::window::{self, Ev, Layered};
use super::Shared;
use crate::app::App;
use serde_json::json;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{HANDLE, HWND, POINT};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

pub enum Cmd {
    /// settings changed: re-read them
    Settings,
    /// Move / resize on or off
    Edit,
    /// demo input on or off
    Demo,
    /// a message next to the overlay (text, accent border)
    Say(String, bool),
    Wake,
}

const FPS: f32 = 60.0;
const IDLE_FADE: Duration = Duration::from_secs(6);
const IDLE_OPACITY: f32 = 0.30;
const TOPMOST: Duration = Duration::from_millis(500);

struct Toast {
    win: Layered,
    until: Instant,
    sticky: bool,
    kind: &'static str,
    drawn: Option<(String, String, u32, bool)>,
}

impl Toast {
    fn showing(&self) -> bool {
        self.sticky || Instant::now() < self.until
    }
    fn tick(&mut self, dt: f32) {
        let target = if self.showing() { 1.0 } else { 0.0 };
        if self.win.alpha == target {
            return;
        }
        let step = dt * if target > self.win.alpha { 9.0 } else { 5.0 };
        let a = if (target - self.win.alpha).abs() <= step { target } else { self.win.alpha + step.copysign(target - self.win.alpha) };
        self.win.set_alpha(a);
        if a == 0.0 {
            self.kind = "";
        }
    }
}

/// The windows, made when the overlay comes on and dropped when it goes off.
struct Windows {
    main: Layered,
    toast: Toast,
}

struct Ov {
    app: Arc<App>,
    sh: Arc<Shared>,
    s: PadSettings,
    w: Option<Windows>,
    input_hwnd: HWND,
    kbm: Rc<RefCell<input::Kbm>>,
    reader: Option<Reader>,
    art: Option<Art>,
    mon: Option<Monitor>,
    auto_hmon: isize,
    geo: (i32, i32, i32, i32),
    edit: bool,
    demo_since: Option<Instant>,
    drag: Option<(i32, i32)>,
    was_connected: Option<bool>,
    last_input: Instant,
    last_tick: Instant,
    next_house: Instant,
    next_top: Instant,
    frame_ms: f32,
    mode: String,
    last_pad: Option<PadState>,
    last_kbm: Option<KbmState>,
}

fn cursor() -> (i32, i32) {
    let mut p = POINT { x: 0, y: 0 };
    unsafe { GetCursorPos(&mut p) };
    (p.x, p.y)
}

impl Ov {
    fn active(&self) -> bool {
        self.s.on || self.edit || self.sh.test_active()
    }

    fn own(&self, h: HWND) -> bool {
        if let Some(w) = &self.w {
            if h == w.main.hwnd || h == w.toast.win.hwnd {
                return true;
            }
        }
        // Deck's own windows (its page, toasts, tray menus)
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(h, &mut pid) };
        pid == std::process::id()
    }

    fn class_of(h: HWND) -> String {
        let mut buf = [0u16; 64];
        let n = unsafe { GetClassNameW(h, buf.as_mut_ptr(), 64) };
        String::from_utf16_lossy(&buf[..n.max(0) as usize])
    }

    fn family(&self) -> &'static str {
        match self.s.labels.as_str() {
            "playstation" => "ps",
            "xbox" => "xbox",
            _ => self.reader.as_ref().map(|r| r.family).unwrap_or("ps"),
        }
    }

    /// Pick the monitor, rebuild the art if its look changed, move the window.
    fn place(&mut self) {
        self.sync_input();
        let mons = place::monitors();
        let fg = unsafe { GetForegroundWindow() };
        if !fg.is_null() && !place::ignore_fg(&Self::class_of(fg), self.own(fg)) {
            self.auto_hmon = unsafe { windows_sys::Win32::Graphics::Gdi::MonitorFromWindow(fg, windows_sys::Win32::Graphics::Gdi::MONITOR_DEFAULTTONEAREST) } as isize;
        }
        let Some(mon) = place::pick(&mons, &self.s, self.auto_hmon).cloned() else { return };
        let k = ((self.s.scale as f32 * mon.dk()) * 1000.0).round() / 1000.0;
        let look = if self.s.mode == "kbm" {
            Look { layout: "kbm".into(), theme: self.s.theme.clone(), style: self.s.style.clone(), k, family: "ps", keys: self.s.keys.clone(), mouse: self.s.mouse }
        } else {
            Look { layout: self.s.layout.clone(), theme: self.s.theme.clone(), style: self.s.style.clone(), k, family: self.family(), keys: vec![], mouse: false }
        };
        let rebuilt = self.art.as_ref().is_none_or(|a| a.look != look);
        if rebuilt {
            self.art = Some(Art::new(look));
        }
        let (w, h) = self.art.as_ref().map(|a| (a.w, a.h)).unwrap_or((1, 1));
        let (x, y) = place::geometry(&self.s, &mon, w, h);
        self.mon = Some(mon);
        if (w, h, x, y) != self.geo && self.drag.is_none() {
            self.geo = (w, h, x, y);
            if let Some(win) = &mut self.w {
                win.main.move_to(x, y);
            }
        }
        if rebuilt {
            self.draw(true);
        }
    }

    /// Raw keyboard & mouse input only while keyboard & mouse mode is on and the overlay runs.
    fn sync_input(&mut self) {
        let want = if self.active() { self.s.mode.clone() } else { String::new() };
        if want == self.mode {
            return;
        }
        self.mode = want;
        self.was_connected = None; // re-evaluated quietly, no toast
        self.kbm.borrow_mut().enable(self.input_hwnd, self.mode == "kbm" && self.demo_since.is_none());
    }

    fn pill(&self) -> Option<Reading> {
        if self.demo_since.is_some() {
            return Some(Reading { percent: Some(64), level: None, charging: false });
        }
        let r = self.reader.as_ref()?;
        if !r.connected() {
            return None;
        }
        let entries = self.sh.battery.lock();
        let by_path = (!r.path.is_empty()).then(|| entries.iter().find(|e| !e.path.is_empty() && e.path == r.path)).flatten();
        let by_id = || entries.iter().find(|e| r.vid != 0 && (e.vid, e.pid) == (r.vid, r.pid));
        by_path.or_else(by_id).map(|e| e.reading).or(r.sdl_battery)
    }

    /// Push a new frame to the window, only when the picture changed.
    fn draw(&mut self, force: bool) {
        let pill = self.pill();
        let note = self.reader.as_ref().map(|r| r.note.clone()).unwrap_or_default();
        let (Some(art), Some(w)) = (&mut self.art, &mut self.w) else { return };
        if force {
            art.invalidate();
        }
        let frame = if self.s.mode == "kbm" {
            let st = self.last_kbm.clone().unwrap_or_default();
            art.render(&Frame::Kbm(&st), self.edit).cloned()
        } else {
            match &self.last_pad {
                Some(st) => art.render(&Frame::Pad { st, battery: pill }, self.edit).cloned(),
                None => art.render(&Frame::NoPad { note: &note }, self.edit).cloned(),
            }
        };
        if let Some(img) = frame {
            w.main.set_image(&img);
            w.main.move_to(self.geo.2, self.geo.3);
        }
    }

    fn target_alpha(&self, now: Instant) -> f32 {
        let op = self.s.opacity as f32;
        if self.edit {
            return op.max(0.9);
        }
        if !self.s.on {
            return 0.0;
        }
        if self.sh.tweaking() {
            // the Controller page is open: always show it fully
            return op;
        }
        if self.s.hide_no_pad && self.was_connected != Some(true) {
            return 0.0;
        }
        if self.s.fade && now.duration_since(self.last_input) > IDLE_FADE {
            return IDLE_OPACITY.min(op);
        }
        op
    }

    fn fade_to(&mut self, target: f32, dt: f32) {
        let Some(w) = &mut self.w else { return };
        let a = w.main.alpha;
        if a == target {
            return;
        }
        // snap in fast when you touch the pad; drift out slowly when idle
        let step = dt * if target > a { 7.0 } else { 1.2 };
        w.main.set_alpha(if (target - a).abs() <= step { target } else { a + step.copysign(target - a) });
        self.sh.shown.store(w.main.alpha > 0.0, Ordering::Relaxed);
    }

    fn say(&mut self, text: &str, secs: f32, sticky: bool, kind: &'static str, accent: bool) {
        let (Some(w), Some(mon)) = (&mut self.w, &self.mon) else { return };
        let t = art::theme(&self.s.theme);
        let k = mon.dk();
        let key = (text.to_string(), self.s.theme.clone(), (k * 1000.0) as u32, accent);
        if w.toast.drawn.as_ref() != Some(&key) {
            let img = art::pill(&[(text, 13.0, "semibold", t.text)], t, k, accent.then_some(t.accent));
            w.toast.win.set_image(&img);
            w.toast.drawn = Some(key);
        }
        let (tw, th) = w.toast.win.size;
        let (ow, oh, ox, oy) = self.geo;
        let (l, top, r, b) = mon.rect;
        let gap = (2.0 * k).round() as i32;
        let x = ox + (ow - tw) / 2;
        let y = if oy + oh / 2 > (top + b) / 2 { oy - th - gap } else { oy + oh + gap };
        w.toast.win.move_to(x.min(r - tw).max(l), y.min(b - th).max(top));
        w.toast.kind = kind;
        w.toast.sticky = sticky;
        w.toast.until = Instant::now() + Duration::from_secs_f32(secs);
    }

    fn edit_hint(&mut self) {
        self.say("Drag to move  ·  scroll to resize  ·  right-click when done", 1.6, true, "edit", true);
    }

    /// Saves changed settings (debounced) and tells the page.
    fn store(&mut self) {
        self.app.settings.lock().pad = self.s.clone();
        self.app.save_settings();
    }

    fn set_edit(&mut self, on: bool) {
        if on == self.edit {
            return;
        }
        self.edit = on;
        self.sh.edit.store(on, Ordering::Relaxed);
        self.drag = None;
        if on && !self.s.on {
            self.s.on = true;
            self.store();
        }
        self.ensure_windows();
        let cur = if on { unsafe { LoadCursorW(std::ptr::null_mut(), IDC_SIZEALL) } } else { std::ptr::null_mut() };
        window::CURSOR.with(|c| *c.borrow_mut() = cur);
        if let Some(w) = &mut self.w {
            w.main.styles(!on);
        }
        self.place();
        self.draw(true); // show / hide the grab plate
        self.last_input = Instant::now();
        if on {
            self.edit_hint();
        } else {
            if let Some(w) = &mut self.w {
                w.toast.sticky = false;
                w.toast.until = Instant::now();
            }
            self.store();
            self.say("Position saved", 1.6, false, "toast", false);
        }
        self.sh.changed();
    }

    /// Move / resize: drag, wheel, right-click to finish. The drag polls the cursor each tick.
    fn mouse_events(&mut self) {
        let evs: Vec<(HWND, Ev)> = window::EVENTS.with(|e| std::mem::take(&mut *e.borrow_mut()));
        for (_, ev) in evs {
            if !self.edit {
                continue;
            }
            match ev {
                Ev::Down => {
                    let (x, y) = cursor();
                    self.drag = Some((x - self.geo.2, y - self.geo.3));
                }
                Ev::Right => self.set_edit(false),
                Ev::Wheel(d) => {
                    self.s.scale = ((self.s.scale + if d > 0 { 0.05 } else { -0.05 }).clamp(0.5, 3.0) * 100.0).round() / 100.0;
                    self.store();
                    self.place();
                    let pct = (self.s.scale * 100.0).round();
                    self.say(&format!("Size  ·  {pct}%"), 1.6, false, "toast", false);
                }
            }
        }
        let Some((ox, oy)) = self.drag else { return };
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON, VK_RBUTTON};
        let button = if unsafe { GetSystemMetrics(SM_SWAPBUTTON) } != 0 { VK_RBUTTON } else { VK_LBUTTON };
        let (cx, cy) = cursor();
        let (x, y) = (cx - ox, cy - oy);
        if let Some(w) = &mut self.w {
            w.main.move_to(x, y);
        }
        self.geo.2 = x;
        self.geo.3 = y;
        if unsafe { GetAsyncKeyState(button as i32) } as u16 & 0x8000 == 0 {
            // released: store it as offsets from the nearest edge
            self.drag = None;
            let (w, h) = (self.geo.0, self.geo.1);
            let mons = place::monitors();
            let (mx, my) = (x + w / 2, y + h / 2);
            if let Some((i, mon)) = mons.iter().enumerate().find(|(_, m)| m.contains(mx, my)).or_else(|| self.mon.as_ref().and_then(|cur| mons.iter().enumerate().find(|(_, m)| m.handle == cur.handle))) {
                let (ah, av, offx, offy) = place::anchor_from(x, y, w, h, mon);
                self.s.anchor_h = ah.into();
                self.s.anchor_v = av.into();
                self.s.off_x = offx;
                self.s.off_y = offy;
                if self.mon.as_ref().is_some_and(|m| m.handle != mon.handle) {
                    self.s.monitor = json!(i);
                }
                self.store();
            }
            self.place();
            self.edit_hint();
        }
    }

    fn ensure_windows(&mut self) {
        if self.w.is_some() {
            return;
        }
        let (Some(main), Some(toast)) = (Layered::new("Deck controller overlay"), Layered::new("Deck controller overlay message")) else {
            crate::log::warn("controller overlay: could not create its window");
            return;
        };
        main.exclude_from_capture(self.s.capture_hidden);
        toast.exclude_from_capture(self.s.capture_hidden);
        self.w = Some(Windows { main, toast: Toast { win: toast, until: Instant::now(), sticky: false, kind: "", drawn: None } });
        self.geo = (0, 0, i32::MIN, i32::MIN);
        self.art = None;
        if self.reader.is_none() {
            self.reader = Some(Reader::new());
        }
        self.was_connected = None;
        self.last_input = Instant::now();
        self.place();
    }

    fn drop_windows(&mut self) {
        self.w = None;
        self.art = None;
        self.reader = None;
        self.last_pad = None;
        self.sh.shown.store(false, Ordering::Relaxed);
        self.sync_input();
        self.publish(false);
    }

    fn apply_settings(&mut self) {
        let old = std::mem::replace(&mut self.s, self.app.settings.lock().pad.clone());
        if !self.active() {
            self.drop_windows();
            return;
        }
        if self.s.mode != old.mode {
            let msg = if self.s.mode == "kbm" { "Keyboard & mouse" } else { "Controller" };
            self.ensure_windows();
            self.place();
            self.say(msg, 1.6, false, "toast", false);
        }
        self.ensure_windows();
        if let Some(w) = &self.w {
            w.main.exclude_from_capture(self.s.capture_hidden);
            w.toast.win.exclude_from_capture(self.s.capture_hidden);
        }
        self.last_input = Instant::now(); // un-fade so the change shows
        self.place();
        self.draw(true);
    }

    /// The live state for the page (the Test tab and the status line).
    fn publish(&self, live: bool) {
        let r = self.reader.as_ref();
        let pad = self.last_pad.as_ref();
        let mut v = json!({
            "connected": r.is_some_and(|r| r.connected()) || self.demo_since.is_some(),
            "name": if self.demo_since.is_some() { "Demo controller".to_string() } else { r.map(|r| r.name.clone()).unwrap_or_default() },
            "note": r.map(|r| r.note.clone()).unwrap_or_default(),
            "vid": r.map(|r| r.vid).unwrap_or(0),
            "pid": r.map(|r| r.pid).unwrap_or(0),
            "sdl": super::sdl::api().is_some(),
            "battery": self.pill(),
            "running": self.w.is_some(),
            "shown": self.w.as_ref().is_some_and(|w| w.main.alpha > 0.0),
            "edit": self.edit,
            "demo": self.demo_since.is_some(),
        });
        if live {
            if let Some(st) = pad {
                v["buttons"] = json!(input::BUTTONS.iter().filter(|b| st.button(b)).collect::<Vec<_>>());
                v["axes"] = json!(input::AXES.iter().map(|a| (a.to_string(), json!((st.axis(a) * 1000.0).round() / 1000.0))).collect::<serde_json::Map<String, serde_json::Value>>());
            }
            if let Some(k) = &self.last_kbm {
                v["keys"] = json!(k.keys);
                v["mouse"] = json!(k.buttons);
            }
        }
        *self.sh.status.lock() = v;
    }

    fn tick(&mut self) {
        let now = Instant::now();
        let dt = now.duration_since(self.last_tick).as_secs_f32().min(0.1);
        self.last_tick = now;
        self.mouse_events();
        let demo_t = self.demo_since.map(|t| now.duration_since(t).as_secs_f32());
        let connected = if self.s.mode == "kbm" {
            let st = match demo_t {
                Some(t) => input::demo_kbm(t),
                None => self.kbm.borrow_mut().read(),
            };
            if input::kbm_active(&st) {
                self.last_input = now;
            }
            self.last_kbm = Some(st);
            true
        } else {
            let st = match demo_t {
                Some(t) => Some(input::demo(t)),
                None => self.reader.as_mut().and_then(|r| r.read()),
            };
            if st.as_ref().is_some_and(input::is_active) {
                self.last_input = now;
            }
            self.last_pad = st;
            self.last_pad.is_some()
        };
        if Some(connected) != self.was_connected {
            if self.was_connected.is_some() && self.s.mode != "kbm" {
                let msg = if connected { format!("Connected  ·  {}", self.reader.as_ref().map(|r| r.name.as_str()).unwrap_or("controller")) } else { "Controller disconnected".into() };
                self.say(&msg, 1.6, false, "toast", false);
            }
            self.was_connected = Some(connected);
            self.last_input = now;
            self.place(); // "auto" labels may have changed
            self.sh.changed();
        }
        let target = self.target_alpha(now);
        let shown = self.w.as_ref().is_some_and(|w| w.main.alpha > 0.0);
        if shown || target > 0.0 {
            self.draw(false);
        }
        self.fade_to(target, dt);
        if self.edit && self.w.as_ref().is_some_and(|w| !w.toast.showing()) {
            self.edit_hint();
        }
        if let Some(w) = &mut self.w {
            w.toast.tick(dt);
        }
        if now >= self.next_top {
            self.next_top = now + TOPMOST;
            let edit = self.edit;
            if let Some(w) = &mut self.w {
                if w.main.shown {
                    w.main.styles(!edit);
                    w.main.topmost();
                }
                if w.toast.win.shown {
                    w.toast.win.styles(true);
                    w.toast.win.topmost();
                }
            }
        }
        let test = self.sh.test_active();
        if now >= self.next_house {
            self.next_house = now + Duration::from_secs(1);
            self.place(); // follow the game across monitors
            if self.kbm.borrow().active {
                self.kbm.borrow_mut().resync();
            }
            if !test {
                self.publish(false);
            }
        }
        if test {
            self.publish(true);
        }
        let toast_on = self.w.as_ref().is_some_and(|w| w.toast.win.alpha > 0.0);
        let alpha = self.w.as_ref().map_or(0.0, |w| w.main.alpha);
        let visible = alpha > 0.0 || toast_on || target > 0.0;
        // full rate while something moves; faded, 30 Hz is enough to notice the next press; hidden, 20 Hz
        let lively = self.edit || toast_on || alpha != target || self.sh.tweaking() || test || now.duration_since(self.last_input) < IDLE_FADE + Duration::from_secs(1);
        self.frame_ms = if visible { if lively { 1000.0 / FPS } else { 33.0 } } else { 50.0 };
    }
}

/// The thread's body. `rx` carries commands; each send is followed by a WM_APP to wake it.
pub fn run(app: Arc<App>, sh: Arc<Shared>, rx: Receiver<Cmd>, ready: impl FnOnce(u32)) {
    use windows_sys::Win32::System::Threading::{CreateWaitableTimerExW, GetCurrentThreadId, SetWaitableTimer, CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, TIMER_ALL_ACCESS};
    use windows_sys::Win32::UI::HiDpi::{SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
    unsafe {
        SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        // makes this thread's message queue, so commands can wake it
        let mut msg: MSG = std::mem::zeroed();
        PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_NOREMOVE);
    }
    ready(unsafe { GetCurrentThreadId() });
    let mut timer: HANDLE = unsafe { CreateWaitableTimerExW(std::ptr::null(), std::ptr::null(), CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, TIMER_ALL_ACCESS) };
    if timer.is_null() {
        // older Windows: an ordinary waitable timer (rounded to the system tick)
        timer = unsafe { CreateWaitableTimerExW(std::ptr::null(), std::ptr::null(), 0, TIMER_ALL_ACCESS) };
    }
    let kbm = Rc::new(RefCell::new(input::Kbm::default()));
    {
        let k = kbm.clone();
        window::RAW.with(|r| *r.borrow_mut() = Some(Box::new(move |lp| k.borrow_mut().on_input(lp))));
    }
    let now = Instant::now();
    let s = app.settings.lock().pad.clone();
    let mut ov = Ov {
        app,
        sh,
        s,
        w: None,
        input_hwnd: window::input_window(),
        kbm,
        reader: None,
        art: None,
        mon: None,
        auto_hmon: 0,
        geo: (0, 0, 0, 0),
        edit: false,
        demo_since: None,
        drag: None,
        was_connected: None,
        last_input: now,
        last_tick: now,
        next_house: now,
        next_top: now,
        frame_ms: 1000.0 / FPS,
        mode: String::new(),
        last_pad: None,
        last_kbm: None,
    };
    if ov.active() {
        ov.ensure_windows();
    }
    let mut next = Instant::now();
    loop {
        let running = ov.active();
        unsafe {
            if running {
                let wait = next.saturating_duration_since(Instant::now());
                let due: i64 = -((wait.as_nanos() / 100) as i64).max(1);
                SetWaitableTimer(timer, &due, 0, None, std::ptr::null(), 0);
                MsgWaitForMultipleObjects(1, &timer, 0, 1000, QS_ALLINPUT);
            } else {
                MsgWaitForMultipleObjects(0, std::ptr::null(), 0, windows_sys::Win32::System::Threading::INFINITE, QS_ALLINPUT);
            }
            let mut msg: MSG = std::mem::zeroed();
            while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        while let Ok(cmd) = rx.try_recv() {
            match cmd {
                Cmd::Settings => ov.apply_settings(),
                Cmd::Edit => {
                    let on = !ov.edit;
                    ov.set_edit(on);
                }
                Cmd::Demo => {
                    ov.demo_since = if ov.demo_since.is_some() { None } else { Some(Instant::now()) };
                    ov.mode.clear(); // re-evaluate raw input (demo needs none)
                    ov.was_connected = None;
                    ov.last_input = Instant::now();
                    if ov.active() {
                        ov.ensure_windows();
                        ov.place();
                        let msg = if ov.demo_since.is_some() { "Demo input on  ·  no controller needed" } else { "Demo input off" };
                        ov.say(msg, 1.6, false, "toast", false);
                    }
                    ov.sh.demo.store(ov.demo_since.is_some(), Ordering::Relaxed);
                    ov.sh.changed();
                }
                Cmd::Say(text, accent) => {
                    ov.last_input = Instant::now();
                    ov.say(&text, 4.0, false, "toast", accent);
                }
                Cmd::Wake => {}
            }
        }
        if !ov.active() {
            if ov.w.is_some() {
                ov.drop_windows();
            }
            continue;
        }
        ov.ensure_windows();
        if Instant::now() >= next {
            ov.tick();
            next += Duration::from_secs_f32(ov.frame_ms / 1000.0);
            let now = Instant::now();
            if next < now {
                next = now; // fell behind: no burst of catch-up frames
            }
        }
    }
}
