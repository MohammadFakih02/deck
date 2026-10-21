//! The controller overlay (Controller Overlay, merged into Deck) and the universal controller
//! battery: one tray icon per controller that reports a battery, and the battery pill on the
//! overlay. See `docs/design/specs/2026-10-18-controller-overlay-design.md`.
//!
//! Two threads: `battery` (polls every 2 s while the tray icons or the overlay are on) and
//! `overlay` (the native windows and the frame clock). Settings are `pad` in settings.json.

pub mod art;
pub mod battery;
pub mod cfg;
pub mod icon;
pub mod input;
pub mod overlay;
pub mod place;
pub mod raster;
pub mod sdl;
pub mod window;

use crate::app::{App, Toast};
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, OnceLock, Weak};
use std::time::{Duration, Instant};

pub struct Shared {
    /// the controllers with a battery reading, as the tray icons show them
    pub battery: Mutex<Vec<battery::Entry>>,
    /// the overlay's state for the page (and the Test tab's live input)
    pub status: Mutex<Value>,
    test_until: Mutex<Option<Instant>>,
    tweak_until: Mutex<Option<Instant>>,
    pub shown: AtomicBool,
    pub demo: AtomicBool,
    pub edit: AtomicBool,
    cmd: Mutex<Option<(Sender<overlay::Cmd>, u32)>>,
    app: Weak<App>,
}

impl Shared {
    pub fn test_active(&self) -> bool {
        self.test_until.lock().is_some_and(|t| Instant::now() < t)
    }
    pub fn tweaking(&self) -> bool {
        self.tweak_until.lock().is_some_and(|t| Instant::now() < t)
    }
    /// Tells Deck's page (and the tray) that the controller state changed.
    pub fn changed(&self) {
        if let Some(a) = self.app.upgrade() {
            a.ui_changed("pad");
        }
    }
}

static SHARED: OnceLock<Arc<Shared>> = OnceLock::new();

pub fn shared() -> Option<&'static Arc<Shared>> {
    SHARED.get()
}

/// Sends a command to the overlay thread and wakes it.
pub fn post(cmd: overlay::Cmd) {
    let Some(sh) = shared() else { return };
    if let Some((tx, tid)) = sh.cmd.lock().as_ref() {
        if tx.send(cmd).is_ok() {
            unsafe { windows_sys::Win32::UI::WindowsAndMessaging::PostThreadMessageW(*tid, windows_sys::Win32::UI::WindowsAndMessaging::WM_APP, 0, 0) };
        }
    }
}

/// Settings changed (from the page, the tray or an import).
pub fn settings_changed() {
    post(overlay::Cmd::Settings);
}

/// The standalone Controller Overlay's settings and battery levels, once, on the first start
/// that finds them. The overlay comes on when there were settings to import.
pub fn import_old(app: &App) {
    if app.settings.lock().pad_imported {
        return;
    }
    let dir = crate::util::appdata().join("ControllerOverlay");
    let read = |name: &str| -> Option<Value> {
        let f = crate::fsguard::open_read_for_user(&dir.join(name)).ok()?;
        serde_json::from_reader(std::io::BufReader::new(f)).ok()
    };
    if let Some(v) = read("settings.json").filter(Value::is_object) {
        app.settings.lock().pad = cfg::from_old_app(&v);
        crate::log::info("controller overlay: imported Controller Overlay's settings");
    }
    if let Some(Value::Object(m)) = read("battery.json") {
        let mut f = battery::load_floors();
        for (mac, v) in m {
            if let Some(p) = v.as_u64().filter(|p| *p <= 100) {
                f.map.entry(format!("gamesir:{}", mac.to_ascii_lowercase())).or_insert(battery::Floor { pct: p as u8, seen: 0 });
                f.dirty = true;
            }
        }
        battery::save_floors(&mut f);
    }
    app.settings.lock().pad_imported = true;
    app.save_settings();
}

/// Starts the battery and overlay threads (once).
pub fn start(app: &Arc<App>) {
    if SHARED.get().is_some() {
        return;
    }
    import_old(app);
    let sh = Arc::new(Shared {
        battery: Mutex::new(Vec::new()),
        status: Mutex::new(json!({})),
        test_until: Mutex::new(None),
        tweak_until: Mutex::new(None),
        shown: AtomicBool::new(false),
        demo: AtomicBool::new(false),
        edit: AtomicBool::new(false),
        cmd: Mutex::new(None),
        app: Arc::downgrade(app),
    });
    let _ = SHARED.set(sh.clone());

    // the battery
    {
        let weak = Arc::downgrade(app);
        let wanted = move || weak.upgrade().is_some_and(|a| {
            let s = a.settings.lock();
            s.pad.battery_tray || s.pad.on
        });
        let weak = Arc::downgrade(app);
        let sh2 = sh.clone();
        let mut low = battery::LowWarn::default();
        // (id, name): the id (a MAC address, a device path) stays out of the log
        let mut seen: Vec<(String, String)> = vec![];
        battery::start(wanted, move |entries: Vec<battery::Entry>| {
            // controllers coming and going (by name; the reading changes too often to log)
            let now: Vec<(String, String)> = entries.iter().map(|e| (e.id.clone(), e.name.clone())).collect();
            for e in entries.iter().filter(|e| !seen.iter().any(|x| x.0 == e.id)) {
                crate::log::info(format!("controller: {} found ({}, battery {})", e.name, e.source, e.reading.text()));
            }
            for (_, name) in seen.iter().filter(|x| !now.iter().any(|n| n.0 == x.0)) {
                crate::log::info(format!("controller: {name} gone"));
            }
            seen = now;
            *sh2.battery.lock() = entries.clone();
            let Some(a) = weak.upgrade() else { return };
            if a.settings.lock().pad.battery_warn {
                for e in low.check(&entries) {
                    crate::log::info(format!("controller: {} battery low ({})", e.name, e.reading.text()));
                    let text = format!("Controller battery low  ·  {}", e.reading.percent.map(|p| format!("{p}%")).unwrap_or_else(|| "low".into()));
                    if sh2.shown.load(Ordering::Relaxed) {
                        post(overlay::Cmd::Say(text, true));
                    } else {
                        a.toast(Toast::simple(&format!("pad-low:{}", e.id), "Controller battery low", &format!("{}  ·  {}", e.name, e.reading.text())));
                    }
                }
            }
            a.ui_changed("pad");
        });
    }

    // the overlay
    {
        let (tx, rx) = channel();
        let a = app.clone();
        let sh2 = sh.clone();
        if let Err(e) = std::thread::Builder::new().name("pad-overlay".into()).spawn(move || {
            let sh3 = sh2.clone();
            overlay::run(a, sh2, rx, move |tid| *sh3.cmd.lock() = Some((tx, tid)));
        }) {
            crate::log::error(format!("controller overlay: its thread couldn't be started: {e}"));
        }
    }
}

/// What the Controller page shows. Asking for it also keeps the overlay fully visible for a
/// moment (the page is open: you're tweaking it); `test` also keeps live input flowing.
pub fn state(test: bool) -> Value {
    let Some(sh) = shared() else { return json!({ "running": false }) };
    let until = Instant::now() + Duration::from_secs(3);
    *sh.tweak_until.lock() = Some(until);
    if test {
        let was = sh.test_active();
        *sh.test_until.lock() = Some(until);
        if !was {
            post(overlay::Cmd::Wake);
        }
    }
    let mut v = sh.status.lock().clone();
    v["controllers"] = json!(*sh.battery.lock());
    v["monitors"] = json!(place::monitors().iter().map(|m| json!({ "w": m.rect.2 - m.rect.0, "h": m.rect.3 - m.rect.1, "primary": m.primary })).collect::<Vec<_>>());
    v["keys"] = v.get("keys").cloned().unwrap_or(json!([]));
    v["key_list"] = json!(cfg::KEYS.iter().map(|k| json!({ "id": k.0, "label": k.1, "row": k.3, "x": k.4, "w": k.5 })).collect::<Vec<_>>());
    v["edit"] = json!(sh.edit.load(Ordering::Relaxed));
    v["demo"] = json!(sh.demo.load(Ordering::Relaxed));
    v
}

/// A tray or page action: "edit", "demo", "visible", "controller", "kbm".
pub fn action(app: &App, what: &str) -> Result<(), String> {
    match what {
        "edit" => post(overlay::Cmd::Edit),
        "demo" => post(overlay::Cmd::Demo),
        "visible" | "controller" | "kbm" => {
            {
                let mut s = app.settings.lock();
                match what {
                    "visible" => s.pad.on = !s.pad.on,
                    m => s.pad.mode = m.into(),
                }
            }
            app.save_settings();
            settings_changed();
        }
        _ => return Err(format!("unknown controller action {what}")),
    }
    Ok(())
}

/// One tray icon per controller with a reading: (tray id, tooltip, RGBA, size).
pub fn tray_icons(size: u32) -> Vec<(String, String, Vec<u8>)> {
    let Some(sh) = shared() else { return vec![] };
    sh.battery
        .lock()
        .iter()
        .map(|e| {
            let id = format!("pad-{}", &blake3::hash(e.id.as_bytes()).to_hex()[..12]);
            let tip = format!("{}  ·  {}", e.name, e.reading.text());
            (id, tip, icon::battery(size, &e.reading))
        })
        .collect()
}

/// `deck.exe --pad-probe [--out file]`: what every battery source sees and what SDL reads from
/// the first controller for a few seconds. Read-only; opens no window.
pub fn probe(secs: u64) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    crate::util::com();
    let ifaces = battery::hid::list();
    let _ = writeln!(out, "HID interfaces: {}", ifaces.len());
    for i in &ifaces {
        if let Some((v, p)) = battery::hid::ids(&i.path) {
            if let Some((page, usage, name)) = battery::hid::describe(&i.path) {
                if battery::hid::is_gamepad(page, usage) || v == battery::gamesir::VID {
                    let _ = writeln!(out, "  {v:04x}:{p:04x} page {page:#x} usage {usage:#x} {name:?} virtual={} bt={}", battery::hid::is_virtual(i.devinst), battery::hid::is_bluetooth(&i.path));
                }
            }
        }
    }
    let mut gs = battery::gamesir::GameSir::default();
    let mut sony = battery::sony::Sony::default();
    let mut all = gs.poll(&ifaces);
    all.extend(sony.poll(&ifaces));
    all.extend(battery::wgi::poll());
    all.extend(battery::bt::poll(&ifaces));
    all.extend(battery::xinput::poll());
    let _ = writeln!(out, "battery sources:");
    for e in &all {
        let _ = writeln!(out, "  {} [{}] {:04x}:{:04x} {} id={}", e.name, e.source, e.vid, e.pid, e.reading.text(), e.id);
    }
    let _ = writeln!(out, "merged: {:?}", battery::merge(all).iter().map(|e| format!("{} {}", e.name, e.reading.text())).collect::<Vec<_>>());
    let _ = writeln!(out, "SDL3: {}", if sdl::api().is_some() { "loaded" } else { "not loaded (XInput)" });
    let mut r = input::Reader::new();
    let end = Instant::now() + Duration::from_secs(secs);
    let mut last = String::new();
    while Instant::now() < end {
        let st = r.read();
        let line = match &st {
            Some(st) => format!("{} {:04x}:{:04x} path={:?} buttons={:?} lx={:.2} ly={:.2} r2={:.2} sdl_battery={:?}", r.name, r.vid, r.pid, r.path, input::BUTTONS.iter().filter(|b| st.button(b)).collect::<Vec<_>>(), st.axis("lx"), st.axis("ly"), st.axis("r2"), r.sdl_battery.map(|b| b.text())),
            None => format!("no controller (note {:?})", r.note),
        };
        if line != last {
            let _ = writeln!(out, "  {line}");
            last = line;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    out
}
