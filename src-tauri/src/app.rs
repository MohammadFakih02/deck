//! The app's shared state. The desktop UI, the tray, the phone and the
//! background loops all go through `App`.

use crate::settings::Settings;
use parking_lot::Mutex;
use serde::Serialize;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone, Serialize, Debug)]
pub struct Toast {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub body: String,
    /// (action id, button label)
    pub actions: Vec<(String, String)>,
    /// Stays until a button is pressed.
    pub sticky: bool,
    pub sound: bool,
}

impl Toast {
    pub fn simple(id: &str, title: &str, body: &str) -> Self {
        Self { id: id.into(), kind: "info".into(), title: title.into(), body: body.into(), actions: vec![], sticky: false, sound: false }
    }
}

/// Something the PC offered to the phone (text, a link or a file).
#[derive(Clone, Serialize, Debug)]
pub struct Shared {
    pub id: u64,
    /// "text" | "link" | "file"
    pub kind: String,
    pub text: String,
    pub path: Option<String>,
    pub name: String,
    pub size: u64,
    pub ts: u64,
    /// "pc" (sent to the phone) | "phone" (received from the phone)
    pub from: String,
}

type UiFn = Box<dyn Fn(&str, Value) + Send + Sync>;

/// The app itself, for work that must leave the calling thread.
static APP_SELF: Mutex<Option<std::sync::Weak<App>>> = Mutex::new(None);

pub struct App {
    pub settings: Mutex<Settings>,
    saver: Sender<()>,
    ui: Mutex<Option<UiFn>>,
    pub changes: tokio::sync::broadcast::Sender<String>,
    pub toasts: Mutex<Vec<Toast>>,
    pub shared: Mutex<Vec<Shared>>,
    pub remote_clients: AtomicUsize,
    /// phones with the Screen view open
    pub screen_viewers: AtomicUsize,
    pub visible: AtomicBool,

    pub sys: crate::sys::Sys,
    pub media: crate::media::Media,
    pub apps: crate::apps::Apps,
    pub files: Arc<crate::files::Index>,
    pub clip: Arc<crate::clipboard::Clip>,
    pub rates: crate::calc::Rates,
    pub power: crate::power::Power,
    pub disk: Arc<crate::disk::Disk>,
    pub cleanup: Arc<crate::cleanup::Cleanup>,
    pub net: Arc<crate::net::Net>,
    pub boost: crate::boost::Boost,
    /// MoboCtl's hardware engine (fans, RGB, cooler display, sensors); None until started or
    /// when Deck runs without admin rights.
    pub hw: Mutex<Option<Arc<crate::hw::Shared>>>,
    /// NVIDIA tuning (power limit, clocks, curve, fan); None without an NVIDIA card.
    pub gpu: Mutex<Option<Arc<crate::gpu::Gpu>>>,
    /// the hardware / GPU engines finished starting (found something or not)
    pub hw_done: AtomicBool,
    pub gpu_done: AtomicBool,
}

impl App {
    pub fn new() -> Arc<Self> {
        let settings = Settings::load();
        let (tx, rx) = channel::<()>();
        let (changes, _) = tokio::sync::broadcast::channel(64);
        let clip = crate::clipboard::Clip::new();
        *clip.max.lock() = settings.clip_max;
        *clip.enabled.lock() = settings.clip_on;
        *clip.images.lock() = settings.clip_images;
        let app = Arc::new(Self {
            settings: Mutex::new(settings),
            saver: tx,
            ui: Mutex::new(None),
            changes,
            toasts: Mutex::new(Vec::new()),
            shared: Mutex::new(Vec::new()),
            remote_clients: AtomicUsize::new(0),
            screen_viewers: AtomicUsize::new(0),
            visible: AtomicBool::new(false),
            sys: crate::sys::Sys::new(),
            media: crate::media::Media::new(),
            apps: crate::apps::Apps::new(),
            files: Arc::new(crate::files::Index::new()),
            clip,
            rates: crate::calc::Rates::new(),
            power: crate::power::Power::new(),
            disk: Arc::new(crate::disk::Disk::new()),
            cleanup: Arc::new(crate::cleanup::Cleanup::new()),
            net: Arc::new(crate::net::Net::new()),
            boost: crate::boost::Boost::new(),
            hw: Mutex::new(None),
            gpu: Mutex::new(None),
            hw_done: AtomicBool::new(false),
            gpu_done: AtomicBool::new(false),
        });
        *APP_SELF.lock() = Some(Arc::downgrade(&app));
        // Debounced settings saver.
        let weak = Arc::downgrade(&app);
        std::thread::Builder::new()
            .name("settings-saver".into())
            .spawn(move || {
                while rx.recv().is_ok() {
                    std::thread::sleep(Duration::from_millis(400));
                    while rx.try_recv().is_ok() {}
                    if let Some(a) = weak.upgrade() {
                        let s = a.settings.lock().clone();
                        let _ = s.save();
                    }
                }
            })
            .ok();
        {
            let weak = Arc::downgrade(&app);
            app.clip.set_notify(Box::new(move || {
                if let Some(a) = weak.upgrade() {
                    a.changed("clipboard");
                }
            }));
        }
        app
    }

    /// Starts the background work that runs whether or not the window is open.
    pub fn start(self: &Arc<Self>) {
        self.clip.start();
        crate::power::start(self.clone());
        let roots = {
            let s = self.settings.lock();
            s.index_on.then(|| s.index_roots.clone())
        };
        if let Some(r) = roots {
            self.files.rebuild(r);
        }
        crate::boost::start_watch(self.clone());
        // hardware engine: finding the fan chip and loading the driver takes a moment
        if crate::elevation::is_admin() {
            let me = self.clone();
            std::thread::Builder::new()
                .name("hw-start".into())
                .spawn(move || {
                    let weak = Arc::downgrade(&me);
                    let notify = Box::new(move |title: &str, body: &str| {
                        if let Some(a) = weak.upgrade() {
                            a.toast(Toast::simple("hw-alert", title, body));
                        }
                    });
                    let sh = crate::hw::start(notify);
                    *me.hw.lock() = sh.clone();
                    me.hw_done.store(true, Ordering::SeqCst);
                    me.changed("hw");
                    // the lights, RAM and cooler display are found a little later: tell the tray
                    // and the pages when what was found changes (for the first minute)
                    if let Some(sh) = sh {
                        let seen = |sh: &crate::hw::Shared| {
                            let st = sh.status.lock().unwrap_or_else(|e| e.into_inner());
                            (st.rgb.1, st.disp.1, st.ram.clone())
                        };
                        let mut last = seen(&sh);
                        for _ in 0..30 {
                            std::thread::sleep(std::time::Duration::from_secs(2));
                            let now = seen(&sh);
                            if now != last {
                                last = now;
                                me.changed("hw");
                            }
                        }
                    }
                })
                .ok();
        }
        // GPU engine: the startup profile is applied unless the crash guard holds it back
        {
            let me = self.clone();
            std::thread::Builder::new()
                .name("gpu-start".into())
                .spawn(move || {
                    crate::gpu::guard::rearm();
                    let Some(g) = crate::gpu::Gpu::new() else {
                        me.gpu_done.store(true, Ordering::SeqCst);
                        return;
                    };
                    g.start_loop(me.clone());
                    *me.gpu.lock() = Some(g);
                    let (startup, active) = {
                        let s = me.settings.lock();
                        (s.gpu_profiles.iter().find(|p| p.id == s.gpu_startup).cloned(), s.active_gpu.clone())
                    };
                    match startup {
                        Some(p) if crate::elevation::is_admin() => {
                            if crate::gpu::guard::hold_back(&p) {
                                *crate::profcmd::HELD.lock() = Some(p.id.clone());
                                me.settings.lock().active_gpu.clear();
                                me.toast(Toast {
                                    actions: vec![("apply".into(), "Apply anyway".into())],
                                    sticky: true,
                                    ..Toast::simple("gpu-held", "GPU started at stock", &if crate::gpu::guard::held_after_quit() {
                                        format!("Deck was closed while the overclock \"{}\" was on, so it can't tell whether the PC then shut down normally or crashed.", p.name)
                                    } else {
                                        format!("The PC restarted unexpectedly while the overclock \"{}\" was on.", p.name)
                                    })
                                });
                            } else {
                                crate::profiles::switch(&me, "gpu", &p.id, "startup");
                            }
                        }
                        _ => {
                            // after a restart the card is at stock: only keep "active" if Deck applied it this boot
                            if !active.is_empty() && crate::gpu::guard::applied_this_boot().as_deref() != Some(active.as_str()) {
                                me.settings.lock().active_gpu.clear();
                                me.save_settings();
                            }
                        }
                    }
                    // only now: a boost recovery or a shortcut waits for this, and must come after
                    // the startup profile
                    me.gpu_done.store(true, Ordering::SeqCst);
                    me.changed("profiles");
                })
                .ok();
        }
        crate::profiles::cpu::start_watch(self.clone());
        crate::profiles::hotkeys::start(self.clone());
        crate::rtss::start(self.clone());
        crate::hw::smart::start(self.clone());
        std::thread::spawn(|| {
            crate::profiles::icons::ensure();
        });
        // Re-index files every 30 minutes, and warm the app list and icons.
        let me = self.clone();
        std::thread::Builder::new()
            .name("housekeeping".into())
            .spawn(move || {
                let _ = me.apps.list(false);
                loop {
                    std::thread::sleep(Duration::from_secs(60));
                    let roots = {
                        let s = me.settings.lock();
                        s.index_on.then(|| s.index_roots.clone())
                    };
                    if let (Some(r), Some(age)) = (roots, me.files.age_secs()) {
                        // not while Full power is on (a game is running)
                        if age > 1800 && !crate::boost::is_on() {
                            me.files.rebuild(r);
                        }
                    }
                    crate::display::theme_tick(&me);
                }
            })
            .ok();
    }

    pub fn set_ui(&self, f: UiFn) {
        *self.ui.lock() = Some(f);
    }

    fn emit(&self, event: &str, payload: Value) {
        if let Some(f) = self.ui.lock().as_ref() {
            f(event, payload);
        }
    }

    /// The hardware engine, if it runs. The lock is released before this returns, so callers never
    /// hold it across slow hardware calls or UI callbacks (it isn't reentrant).
    pub fn hw_engine(&self) -> Option<Arc<crate::hw::Shared>> {
        self.hw.lock().clone()
    }
    /// The NVIDIA engine, if there is one (same rule as `hw_engine`).
    pub fn gpu_engine(&self) -> Option<Arc<crate::gpu::Gpu>> {
        self.gpu.lock().clone()
    }

    /// Opens Deck at a page (a notification's "Show" button).
    pub fn show_page(&self, page: &str) {
        self.emit("show-deck", json!({ "page": page }));
    }

    /// Opens Deck, or closes it when it is in front (the open shortcut).
    pub fn toggle_window(&self) {
        self.emit("toggle-deck", json!({}));
    }

    /// Tells only Deck's own page that part of the state changed (not the phone: Transfer progress
    /// changes several times a second).
    pub fn ui_changed(&self, topic: &str) {
        self.emit("changed", json!({ "topic": topic }));
    }

    /// Tells the UI and the phone that part of the state changed.
    pub fn changed(&self, topic: &str) {
        self.emit("changed", json!({ "topic": topic }));
        let _ = self.changes.send(topic.to_string());
    }

    pub fn save_settings(&self) {
        let _ = self.saver.send(());
        self.changed("settings");
    }

    pub fn toast(&self, t: Toast) {
        {
            let mut ts = self.toasts.lock();
            ts.retain(|x| x.id != t.id);
            ts.push(t.clone());
            if ts.len() > 6 {
                ts.remove(0);
            }
        }
        self.emit("toast", serde_json::to_value(&t).unwrap_or_default());
        let _ = self.changes.send("toast".into());
    }

    pub fn dismiss_toast(&self, id: &str) {
        self.toasts.lock().retain(|x| x.id != id);
        self.emit("toast-dismiss", json!({ "id": id }));
    }

    /// A toast button was pressed (on the PC or the phone).
    pub fn toast_action(&self, id: &str, action: &str) {
        self.dismiss_toast(id);
        if let Some(n) = id.strip_prefix("xfer:").and_then(|s| s.parse::<u64>().ok()) {
            match action {
                "show" => self.show_page("transfer"),
                a => crate::beam::answer(n, a),
            }
            return;
        }
        if id == "plan" {
            match action {
                "cancel" => {
                    *self.power.plan.lock() = None;
                }
                "now" => {
                    let a = self.power.plan.lock().take().map(|p| p.action);
                    if let Some(a) = a {
                        let _ = crate::power::run_action(&a);
                    }
                }
                _ => {}
            }
            self.changed("power");
        } else if let Some(t) = id.strip_prefix("timer:").and_then(|s| s.parse::<u64>().ok()) {
            self.power.timer_cmd(t, if action == "snooze" { "snooze" } else { "remove" });
            self.changed("power");
        } else if id == "break" && action == "snooze" {
            crate::power::snooze_break(self);
        } else if let Some(path) = id.strip_prefix("recv:") {
            match action {
                "open" => {
                    let _ = crate::util::shell_open(path);
                }
                "show" => crate::util::reveal(path),
                _ => {}
            }
        } else if id == "gpu-held" && action == "apply" {
            if let Some(a) = APP_SELF.lock().as_ref().and_then(|w| w.upgrade()) {
                let _ = crate::profcmd::apply_held(&a);
            }
        } else if (id.starts_with("drive:") || id.starts_with("drive-past:")) && action == "show" {
            self.show_page("drives");
        } else if id == "phone-text" && action == "copy" {
            if let Some(t) = self.shared.lock().iter().rev().find(|s| s.from == "phone" && s.kind != "file") {
                let _ = crate::clipboard::set_text(&t.text);
            }
        }
    }

    pub fn is_visible(&self) -> bool {
        self.visible.load(Ordering::Relaxed)
    }

    /// Records a launch for ranking search results.
    pub fn bump(&self, id: &str) {
        let mut s = self.settings.lock();
        let e = s.frecency.entry(id.to_string()).or_insert((0, 0));
        e.0 += 1;
        e.1 = crate::util::now_s();
        if s.frecency.len() > 800 {
            // forget the least used
            let mut v: Vec<(String, (u32, u64))> = s.frecency.drain().collect();
            v.sort_by(|a, b| b.1 .1.cmp(&a.1 .1));
            v.truncate(600);
            s.frecency = v.into_iter().collect();
        }
        drop(s);
        let _ = self.saver.send(());
    }

    /// Frecency bonus: used often and recently -> higher.
    pub fn frecency(&self, id: &str) -> i32 {
        let s = self.settings.lock();
        let Some((n, last)) = s.frecency.get(id) else { return 0 };
        let age_days = crate::util::now_s().saturating_sub(*last) as f32 / 86400.0;
        let recency = if age_days < 1.0 {
            1.0
        } else if age_days < 7.0 {
            0.7
        } else if age_days < 30.0 {
            0.4
        } else {
            0.2
        };
        ((*n as f32).ln_1p() * 60.0 * recency) as i32
    }
}
