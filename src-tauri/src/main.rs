#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
// House style, on purpose: worker and protocol functions take their context as plain arguments,
// hardware code indexes registers by number, and a few settings structs are filled field by field
// after Default. CI denies every other warning (`cargo clippy -- -D warnings`).
#![allow(clippy::too_many_arguments, clippy::type_complexity, clippy::needless_range_loop, clippy::field_reassign_with_default)]

use crate::util::LockAnyway;
mod app;
mod apps;
mod audio;
mod audiocmd;
mod beam;
mod ccsync;
mod deaths;
mod deathscmd;
mod update;
mod updatecmd;
mod boost;
mod boostcmd;
mod calc;
mod cleanup;
mod clipboard;
mod disk;
mod diskcmd;
mod fsguard;
mod kdf;
mod log;
mod support;
mod mft;
mod display;
mod displaycmd;
mod elevation;
mod filecheck;
mod files;
mod gameping;
mod games;
mod gpu;
mod hotcorner;
mod hw;
mod hwcmd;
mod locksmith;
mod media;
mod net;
mod netcmd;
mod phoneauth;
mod phonemic;
mod phonetls;
mod phonekeys;
mod tweaks;
mod tweakscmd;
mod phonecmd;
mod picker;
mod power;
mod profcmd;
mod programs;
mod programscmd;
mod profiles;
mod rdesk;
mod h264;
mod screencast;
mod vpad;
mod remote;
mod osd;
mod pad;
mod padcmd;
mod rtss;
mod seal;
mod secret;
mod settings;
mod share;
mod startup;
mod sys;
mod syscmd;
mod util;
mod driver;
mod restart;
mod watchdog;
mod webview_guard;
mod winmgr;

use app::{App, Shared, Toast};
use remote::Remote;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::Arc;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder, WindowEvent};
use tauri_plugin_dialog::DialogExt;
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTOPRIMARY};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::{GetCursorPos, GetForegroundWindow, SetForegroundWindow};

type R<T> = Result<T, String>;
type A<'a> = State<'a, Arc<App>>;

/// The window that had focus before Deck opened (paste goes back there).
static PREV_FG: AtomicIsize = AtomicIsize::new(0);
/// A file dialog is open: losing focus must not hide Deck.
static DIALOG: AtomicBool = AtomicBool::new(false);
/// "Keep Deck open" pin in the UI.
static PINNED: AtomicBool = AtomicBool::new(false);
/// When Deck last started something (ms since boot): the launched app taking focus isn't
/// "clicking somewhere else" while hide_after_launch is off.
static LAUNCHED_AT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn launch_hold() -> bool {
    let at = LAUNCHED_AT.load(Ordering::Relaxed);
    at != 0 && unsafe { windows::Win32::System::SystemInformation::GetTickCount64() }.saturating_sub(at) < 8000
}
/// Where the pinned quick panel floats (screen pixels: x, y, width, height).
static PIN_RECT: parking_lot::Mutex<Option<[i32; 4]>> = parking_lot::Mutex::new(None);

// ------------------------------------------------------------ window control

fn monitor_info(p: POINT, primary: bool) -> (RECT, RECT, f64) {
    unsafe {
        let mon = MonitorFromPoint(p, if primary { MONITOR_DEFAULTTOPRIMARY } else { MONITOR_DEFAULTTONEAREST });
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(mon, &mut mi);
        let (mut dx, mut dy) = (96u32, 96u32);
        let _ = GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
        (mi.rcMonitor, mi.rcWork, dx as f64 / 96.0)
    }
}

fn monitor_at_cursor() -> (RECT, RECT, f64) {
    let mut p = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut p);
    }
    monitor_info(p, false)
}

fn show_deck(handle: &AppHandle, page: Option<&str>) {
    let Some(w) = handle.get_webview_window("main") else { return };
    let app = handle.state::<Arc<App>>().inner().clone();
    unsafe {
        let fg = GetForegroundWindow();
        let own = w.hwnd().map(|h| h.0 == fg.0).unwrap_or(false);
        if !own && !fg.0.is_null() {
            PREV_FG.store(fg.0 as isize, Ordering::Relaxed);
        }
    }
    let (mode, rect, max) = {
        let s = app.settings.lock();
        (s.mode.clone(), s.app_rect, s.app_max)
    };
    let style = if mode == "app" { "window" } else { "dim" };
    let (mon, work, scale) = monitor_at_cursor();
    if mode == "app" {
        // a normal window: title bar, taskbar button, resizable, where it was last time
        let _ = w.set_decorations(true);
        let _ = w.set_shadow(true);
        let _ = w.set_skip_taskbar(false);
        let _ = w.set_resizable(true);
        // the pin keeps the app window above other windows (in the quick panel it keeps it open)
        let _ = w.set_always_on_top(PINNED.load(Ordering::Relaxed));
        if !app.is_visible() {
            let fit = rect.map(|r| {
                let (_, wk, _) = monitor_info(POINT { x: r[0] + r[2] / 2, y: r[1] + r[3] / 2 }, false);
                let (ww, wh) = (r[2].max(700).min(wk.right - wk.left), r[3].max(500).min(wk.bottom - wk.top));
                [r[0].min(wk.right - ww).max(wk.left), r[1].min(wk.bottom - wh).max(wk.top), ww, wh]
            });
            let [x, y, ww, wh] = fit.unwrap_or_else(|| {
                let ww = ((1280.0 * scale) as i32).min(work.right - work.left - 40);
                let wh = ((900.0 * scale) as i32).min(work.bottom - work.top - 40);
                [work.left + (work.right - work.left - ww) / 2, work.top + (work.bottom - work.top - wh) / 2, ww, wh]
            });
            let _ = w.set_size(tauri::PhysicalSize::new(ww as u32, wh as u32));
            let _ = w.set_position(tauri::PhysicalPosition::new(x, y));
            if max {
                let _ = w.maximize();
            }
        }
    } else {
        // the quick panel: over the whole monitor the mouse is on, the screen dimmed behind it;
        // pinned, it floats just where the panel is, so the rest of the screen stays usable
        let _ = w.set_decorations(false);
        let _ = w.set_shadow(false);
        let _ = w.set_skip_taskbar(true);
        let _ = w.set_resizable(false);
        let _ = w.unmaximize();
        let float = PINNED.load(Ordering::Relaxed).then(|| *PIN_RECT.lock()).flatten();
        let [x, y, ww, wh] = float.unwrap_or([mon.left, mon.top, mon.right - mon.left, mon.bottom - mon.top]);
        let _ = w.set_position(tauri::PhysicalPosition::new(x, y));
        let _ = w.set_size(tauri::PhysicalSize::new(ww as u32, wh as u32));
        let _ = w.set_always_on_top(true);
    }
    app.visible.store(true, Ordering::Relaxed);
    let _ = w.show();
    let _ = w.unminimize();
    let _ = w.set_focus();
    webview_guard::nudge_if_blank(&w);
    let floating = mode != "app" && PINNED.load(Ordering::Relaxed) && PIN_RECT.lock().is_some();
    let _ = handle.emit("shown", json!({ "page": page, "style": style, "mode": mode, "float": floating }));
}

fn app_mode(handle: &AppHandle) -> bool {
    handle.state::<Arc<App>>().settings.lock().mode == "app"
}

/// Remembers where the app window is (not while minimized; maximized only as a flag).
fn remember_rect(handle: &AppHandle, w: &tauri::WebviewWindow) {
    if w.is_minimized().unwrap_or(false) {
        return;
    }
    let app = handle.state::<Arc<App>>();
    let max = w.is_maximized().unwrap_or(false);
    let mut s = app.settings.lock();
    s.app_max = max;
    if !max {
        if let (Ok(p), Ok(z)) = (w.outer_position(), w.outer_size()) {
            s.app_rect = Some([p.x, p.y, z.width as i32, z.height as i32]);
        }
    }
}

fn hide_deck(handle: &AppHandle) {
    if let Some(w) = handle.get_webview_window("main") {
        if app_mode(handle) {
            remember_rect(handle, &w);
            handle.state::<Arc<App>>().save_settings();
        }
        let app = handle.state::<Arc<App>>();
        app.visible.store(false, Ordering::Relaxed);
        let _ = handle.emit("hiding", json!({}));
        let _ = w.hide();
    }
}

/// After launching something: the quick panel gets out of the way; the app window stays open.
fn step_aside(handle: &AppHandle) {
    if !app_mode(handle) {
        hide_deck(handle);
    }
}

fn toggle_deck(handle: &AppHandle) {
    let visible = handle.state::<Arc<App>>().is_visible();
    if visible && app_mode(handle) {
        // the app window: to the front, or minimized when it already is in front
        if let Some(w) = handle.get_webview_window("main") {
            let front = w.is_focused().unwrap_or(false) && !w.is_minimized().unwrap_or(false);
            if front {
                let _ = w.minimize();
            } else {
                let _ = w.unminimize();
                let _ = w.show();
                let _ = w.set_focus();
            }
        }
    } else if visible {
        let front = handle.get_webview_window("main").and_then(|w| w.is_focused().ok()).unwrap_or(true);
        if front {
            hide_deck(handle);
        } else if let Some(w) = handle.get_webview_window("main") {
            let _ = w.set_focus();
        }
    } else {
        show_deck(handle, None);
    }
}

/// Makes a window never take focus and (optionally) let clicks through.
fn no_activate(w: &tauri::WebviewWindow, click_through: bool) {
    use windows::Win32::UI::WindowsAndMessaging::*;
    if let Ok(h) = w.hwnd() {
        let hwnd = HWND(h.0 as _);
        unsafe {
            let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            let mut add = WS_EX_NOACTIVATE.0 | WS_EX_TOOLWINDOW.0;
            if click_through {
                add |= WS_EX_TRANSPARENT.0;
            }
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex | add as isize);
        }
    }
}

fn show_no_activate(w: &tauri::WebviewWindow) {
    use windows::Win32::UI::WindowsAndMessaging::*;
    if let Ok(h) = w.hwnd() {
        let hwnd = HWND(h.0 as _);
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        }
    }
}

fn build_aux_windows(handle: &AppHandle) -> tauri::Result<()> {
    let t = WebviewWindowBuilder::new(handle, "toast", WebviewUrl::App("toast.html".into()))
        .additional_browser_args(&webview_guard::browser_args())
        .title("Deck notifications")
        .inner_size(380.0, 10.0)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .focused(false)
        .visible(false)
        .build()?;
    no_activate(&t, false);
    let z = WebviewWindowBuilder::new(handle, "zones", WebviewUrl::App("zones.html".into()))
        .additional_browser_args(&webview_guard::browser_args())
        .title("Deck zones")
        .inner_size(400.0, 300.0)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .focusable(false)
        .focused(false)
        .visible(false)
        .build()?;
    let _ = z.set_ignore_cursor_events(true);
    no_activate(&z, true);
    Ok(())
}

// ------------------------------------------------------------------ commands

#[tauri::command]
fn get_state(app: A, remote: State<Arc<Remote>>) -> Value {
    let s = app.settings.lock().clone();
    json!({
        "settings": s,
        "version": env!("CARGO_PKG_VERSION"),
        "pc": std::env::var("COMPUTERNAME").unwrap_or_default(),
        "user": std::env::var("USERNAME").unwrap_or_default(),
        "remote": remote::info(&app, &remote),
        "index": app.files.info(),
        "pinned": PINNED.load(Ordering::Relaxed),
        "home": util::home().to_string_lossy(),
        "admin": elevation::is_admin(),
        // the "shown" event of a window shown at start-up can fire before the page listens for it
        "visible": app.is_visible(),
        "hw": { "pages": hw::pages(app.hw.lock().as_deref(), app.gpu.lock().is_some()), "ready": app.hw.lock().is_some() },
    })
}

/// The CPU's clock (MHz), package power (W) and temperature from the hardware engine, and the GPU's
/// temperature and load (any brand), for Home's PC tile (null while the engine hasn't started;
/// a value the PC doesn't report is null).
fn cpu_now(app: &App) -> Value {
    let Some(h) = app.hw_engine() else { return Value::Null };
    let (mhz, t, gpu_t, gpu_l) = h.live.lock().map(|l| (l.cpu_mhz, l.cpu_t, l.gpu_t, l.gpu_l)).unwrap_or_default();
    let watts = h.table.lock().ok().and_then(|t| t.rows.iter().find(|r| r.group.starts_with("CPU") && r.name == "Package power").and_then(|r| r.cur));
    json!({ "mhz": mhz, "w": watts, "t": t, "gpu_t": gpu_t, "gpu_l": gpu_l })
}

#[tauri::command]
async fn live(app: A<'_>) -> R<Value> {
    Ok(json!({
        "sys": app.sys.snapshot(),
        "cpu_now": cpu_now(&app),
        "media": app.media.state(),
        "master": audio::master(),
        "apps": audio::apps(),
        "power": app.power.state(),
        "toasts": *app.toasts.lock(),
        "boost": app.boost.brief(),
        "profiles": profile_badges(&app),
    }))
}

/// Active profile names for the sidebar badges.
fn profile_badges(app: &App) -> Value {
    let s = app.settings.lock();
    let name = |list: Vec<(&String, &String)>, id: &str| list.into_iter().find(|(i, _)| *i == id).map(|(_, n)| n.clone());
    json!({
        "cpu": name(s.cpu_profiles.iter().map(|p| (&p.id, &p.name)).collect(), &s.active_cpu),
        "gpu": name(s.gpu_profiles.iter().map(|p| (&p.id, &p.name)).collect(), &s.active_gpu),
    })
}

#[tauri::command]
fn hide(handle: AppHandle) {
    hide_deck(&handle);
}

/// The pin. In the app window (which never hides by itself) it keeps the window above other
/// windows. In the quick panel it turns the panel into a floating window where it is (`rect`: the
/// panel inside the window, physical pixels), on top of everything, with no dimmed screen around it,
/// so other apps can be used while it stays; unpinned, it's the full-screen panel again.
#[tauri::command]
fn set_pinned(handle: AppHandle, on: bool, rect: Option<[i32; 4]>) {
    PINNED.store(on, Ordering::Relaxed);
    let Some(w) = handle.get_webview_window("main") else { return };
    if app_mode(&handle) {
        let _ = w.set_always_on_top(on);
        return;
    }
    if on {
        let Some([rx, ry, rw, rh]) = rect else { return };
        let at = w.outer_position().map(|p| (p.x, p.y)).unwrap_or((0, 0));
        let r = [at.0 + rx, at.1 + ry, rw.max(400), rh.max(300)];
        *PIN_RECT.lock() = Some(r);
        let _ = w.set_position(tauri::PhysicalPosition::new(r[0], r[1]));
        let _ = w.set_size(tauri::PhysicalSize::new(r[2] as u32, r[3] as u32));
        let _ = w.set_always_on_top(true);
    } else {
        // back to the full screen of the monitor it floats on
        let at = PIN_RECT.lock().take().map(|r| POINT { x: r[0] + r[2] / 2, y: r[1] + r[3] / 2 });
        let (mon, _, _) = at.map(|p| monitor_info(p, false)).unwrap_or_else(monitor_at_cursor);
        let _ = w.set_position(tauri::PhysicalPosition::new(mon.left, mon.top));
        let _ = w.set_size(tauri::PhysicalSize::new((mon.right - mon.left) as u32, (mon.bottom - mon.top) as u32));
    }
}

#[tauri::command]
fn quit(handle: AppHandle) {
    quit_deck(&handle);
}

/// The page's own problems (a command that failed, a script error) into Deck's log: the page has
/// no log of its own. Cut short; the log masks secrets as for every line.
#[tauri::command]
fn ui_log(level: String, msg: String) {
    let msg: String = msg.chars().take(600).collect();
    // a page's script error is a warning: the page carries on, and Deck with it
    match level.as_str() {
        "info" => log::info(format!("ui: {msg}")),
        "debug" => log::debug(format!("ui: {msg}")),
        _ => log::warn(format!("ui: {msg}")),
    }
}

/// Ends Deck cleanly and starts it again, hidden, through the admin task: a helper process waits for
/// this one to be gone first (a second Deck would only hand over to this one). Without the admin
/// task there is nothing that could start it again, so it stays running.
/// Returns false when it should be asked again later (a game is in front: GPU calls stay paused
/// until then); true when it is settled (restarting, or not restarting at all).
fn restart_deck(handle: &AppHandle) -> bool {
    let app = handle.state::<Arc<App>>().inner().clone();
    if !app.settings.lock().restart_after_crash {
        driver::resume();
        log::info("restart after a driver update is off: the new driver is used from Deck's next start");
        return true;
    }
    if !elevation::is_admin() || !elevation::task_exists(elevation::BACKGROUND_TASK) {
        driver::resume();
        log::warn("restart wanted, but Deck has no admin task to start it again: keeping it running");
        return true;
    }
    // never in the middle of a game: the fans and the GPU profile would be let go and applied again
    if boost::game_in_front(&app).is_some() || hotcorner::fullscreen_in_front() {
        return false;
    }
    // the GPU is handed back to the driver on the way out, which needs the calls
    driver::resume();
    use std::os::windows::process::CommandExt;
    let started = std::env::current_exe().and_then(|exe| std::process::Command::new(exe).args(["--start-after", &std::process::id().to_string()]).creation_flags(util::CREATE_NO_WINDOW).spawn());
    if started.is_ok() {
        quit_deck(handle);
    }
    true
}

/// Puts things back first if Full power is on.
fn quit_deck(handle: &AppHandle) {
    let app = handle.state::<Arc<App>>().inner().clone();
    if app_mode(handle) && app.is_visible() {
        if let Some(w) = handle.get_webview_window("main") {
            remember_rect(handle, &w);
        }
    }
    log::info("Deck quitting");
    if let Err(e) = app.settings.lock().save() {
        log::warn(format!("quit: the settings couldn't be saved: {e}"));
    }
    if boost::is_on() {
        if let Err(e) = boost::stop(&app, "") {
            log::warn(format!("quit: Full power couldn't be undone: {e}"));
        }
    }
    shutdown_hardware(false);
    handle.exit(0);
}

/// Hands the fans back to the BIOS and the GPU fan back to the driver. Safe to call twice.
/// `session_end`: Windows is shutting down or signing out (not just Deck closing).
fn shutdown_hardware(session_end: bool) {
    // quick, important things first: Windows may end the process during log-off
    gpu::guard::on_exit(session_end);
    gpu::release();
    rtss::stop();
    hw::stop();
    // released: the watchdog has nothing to do
    watchdog::clean_exit();
    log::info(format!("hardware handed back{}", if session_end { " (Windows is signing out or shutting down)" } else { "" }));
    log::flush();
}

/// Fans back to the BIOS, the GPU fan back to the driver, Game Mode back to the user's setting
/// (after a crash, from uninstall.ps1, or by the watchdog when Deck was ended from outside).
fn release_everything() -> String {
    let msg = if elevation::is_admin() { hw::release_fans_direct() } else { "Needs admin rights.".into() };
    gpu::FAN_MANUAL.store(true, Ordering::SeqCst);
    gpu::CLOCK_LOCKED.store(true, Ordering::SeqCst);
    gpu::release();
    let s = settings::Settings::load();
    profiles::cpu::restore_game_mode(s.game_mode_orig);
    msg
}

/// Changes one setting (by key) and applies its side effects.
#[tauri::command]
fn set_setting(handle: AppHandle, app: A, remote: State<Arc<Remote>>, key: String, value: Value) -> R<()> {
    let old_style = app.settings.lock().osd_style.clone();
    {
        let mut s = app.settings.lock();
        // the games turned off are changed by the tray and the Now tab, never by the settings form:
        // the form's copy may be older than the tray's last change
        let off_games = (key == "deaths").then(|| s.deaths.off_games.clone());
        let mut v = serde_json::to_value(&*s).map_err(|e| e.to_string())?;
        if v.get(&key).is_none() {
            return Err(format!("unknown setting {key}"));
        }
        v[&key] = value.clone();
        *s = serde_json::from_value(v).map_err(|e| format!("bad value for {key}: {e}"))?;
        log::debug(format!("settings: {key} changed"));
        if let Some(off) = off_games {
            s.deaths.off_games = off;
        }
    }
    match key.as_str() {
        "autostart" => sync_autostart(&handle, value.as_bool().unwrap_or(true)),
        "clip_on" => *app.clip.enabled.lock() = value.as_bool().unwrap_or(true),
        "clip_max" => *app.clip.max.lock() = value.as_u64().unwrap_or(400) as usize,
        "clip_images" => *app.clip.images.lock() = value.as_bool().unwrap_or(true),
        "index_roots" | "index_on" => {
            let s = app.settings.lock();
            if s.index_on {
                app.files.rebuild(s.index_roots.clone());
            }
        }
        "remote_on" => {
            if value.as_bool().unwrap_or(true) {
                remote::start(app.inner().clone(), remote.inner().clone());
            } else {
                remote::stop(&remote);
                app.changed("remote");
            }
        }
        "theme_auto" | "light_at" | "dark_at" | "theme_mode" => display::theme_schedule_changed(&app),
        "hotkeys_on" | "osd_hotkey" | "osd_reset_hotkey" | "limiter_hotkey" | "open_hotkey" => profiles::hotkeys::reload(),
        "cpu_battery_same" if value.as_bool() == Some(true) => {
            let a = app.inner().clone();
            std::thread::spawn(move || profiles::cpu::rewrite_active(&a));
        }
        "mode" if app.is_visible() => {
            // re-open in the new mode
            let h = handle.clone();
            let _ = handle.run_on_main_thread(move || {
                if let Some(w) = h.get_webview_window("main") {
                    let _ = w.unmaximize();
                }
                h.state::<Arc<App>>().visible.store(false, Ordering::Relaxed);
                show_deck(&h, Some("settings"));
            });
        }
        "osd_style" => {
            let st = app.settings.lock().osd_style.clone();
            if (rtss::look_edits(&st), st.all_games) != (rtss::look_edits(&old_style), old_style.all_games) {
                if let Err(e) = rtss::apply_look(&st) {
                    log::warn(format!("overlay look: {e}"));
                }
            }
            rtss::refresh_now();
        }
        "osd_on" => {
            if value.as_bool() == Some(true) && !rtss::rtss_running() {
                let _ = rtss::start_rtss();
            }
            rtss::refresh_now();
        }
        "osd_items" | "unit_clock" | "unit_net" | "unit_temp" => rtss::refresh_now(),
        "pad" => pad::settings_changed(),
        "deaths" => {
            profiles::hotkeys::reload();
            app.changed("deaths");
        }
        _ => {}
    }
    app.save_settings();
    Ok(())
}

// ---- search / launch

#[tauri::command]
async fn search(app: A<'_>, q: String) -> R<Value> {
    let q = q.trim().to_string();
    let mut apps: Vec<(i32, apps::AppEntry)> = app
        .apps
        .list(false)
        .into_iter()
        .filter_map(|e| files::score_loose(&e.name, &q).map(|s| (s + app.frecency(&format!("app:{}", e.id)), e)))
        .collect();
    apps.sort_by_key(|b| std::cmp::Reverse(b.0));
    apps.truncate(if q.is_empty() { 0 } else { 8 });
    let calc = calc::eval(&q).map(|v| json!({ "value": calc::fmt_num(v), "raw": v }));
    let conv = calc::base_convert(&q).or_else(|| calc::convert(&q, &app.rates)).map(|(a, d)| json!({ "value": a, "detail": d }));
    let files = if q.len() >= 2 { app.files.search(&q, 8) } else { Vec::new() };
    let clips: Vec<Value> = if q.len() >= 3 {
        app.clip.list(&q, 4).into_iter().filter(|c| c.kind == "text").map(|c| json!({ "id": c.id, "text": c.text.chars().take(200).collect::<String>() })).collect()
    } else {
        Vec::new()
    };
    let games: Vec<Value> = if q.len() >= 2 {
        // best matches first; a game that is already listed as an app (a Start-menu shortcut
        // Steam made) is shown once, as the app
        let shown: Vec<String> = apps.iter().map(|(_, e)| e.name.to_lowercase()).collect();
        let mut g: Vec<(i32, games::Game)> = games::installed()
            .into_iter()
            .filter(|g| !shown.contains(&g.name.to_lowercase()))
            // strong matches, or a one-typo match (never a loose letters-in-order one)
            .filter_map(|g| match files::score(&g.name, &q) {
                Some(s) => (s > 300).then_some((s, g)),
                None => files::score_loose(&g.name, &q).map(|s| (s, g)),
            })
            .collect();
        g.sort_by_key(|b| std::cmp::Reverse(b.0));
        g.into_iter().take(4).map(|(s, g)| json!({ "score": s, "game": g })).collect()
    } else {
        Vec::new()
    };
    Ok(json!({
        "q": q,
        "apps": apps.into_iter().map(|(s, e)| json!({ "score": s, "app": e })).collect::<Vec<_>>(),
        "calc": calc,
        "convert": conv,
        "files": files,
        "clips": clips,
        "games": games,
    }))
}

#[tauri::command]
async fn apps_list(app: A<'_>, force: bool) -> R<Vec<apps::AppEntry>> {
    Ok(app.apps.list(force))
}

/// After opening something: the panel steps aside, unless the user keeps it open.
fn launched(handle: &AppHandle) {
    if handle.state::<Arc<App>>().settings.lock().hide_after_launch {
        step_aside(handle);
    }
}

/// Starts `f` off the main thread: launching through Explorer waits until the program has
/// started, and one that asks for admin rights keeps it waiting for the whole UAC prompt.
/// On the main thread that froze Deck ("not responding"). Deck, still in front, first lets
/// Explorer bring its window forward; without that, Windows parks a UAC prompt in the taskbar.
async fn off_main(f: impl FnOnce() -> R<()> + Send + 'static) -> R<()> {
    LAUNCHED_AT.store(unsafe { windows::Win32::System::SystemInformation::GetTickCount64() }, Ordering::Relaxed);
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow(windows::Win32::UI::WindowsAndMessaging::ASFW_ANY);
    }
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())?
}

#[tauri::command]
async fn launch_app(handle: AppHandle, app: A<'_>, id: String) -> R<()> {
    app.bump(&format!("app:{id}"));
    let app = app.inner().clone();
    let r = off_main(move || app.apps.launch(&id)).await;
    launched(&handle);
    r
}

#[tauri::command]
async fn open_path(handle: AppHandle, app: A<'_>, path: String, keep: Option<bool>) -> R<()> {
    app.bump(&format!("file:{path}"));
    let r = off_main(move || util::shell_open(&path)).await;
    if !keep.unwrap_or(false) {
        launched(&handle);
    }
    r
}

#[tauri::command]
fn reveal(path: String) {
    util::reveal(&path);
}

#[tauri::command]
async fn open_url(handle: AppHandle, url: String, keep: Option<bool>) -> R<()> {
    let ok = ["https://", "http://", "steam://", "com.epicgames.launcher://", "ms-settings:", "discord://", "mailto:", "spotify:", "ms-screenclip:"]
        .iter()
        .any(|p| url.starts_with(p));
    if !ok {
        return Err("That kind of link isn't allowed".into());
    }
    let r = off_main(move || util::shell_open(&url)).await;
    if !keep.unwrap_or(false) {
        launched(&handle);
    }
    r
}

/// Opens a Windows tool (Task Manager, classic Control Panel parts ...).
#[tauri::command]
async fn open_system(handle: AppHandle, what: String) -> R<()> {
    let r = off_main(move || open_system_now(&what)).await;
    launched(&handle);
    r
}

fn open_system_now(what: &str) -> R<()> {
    let spawn = |exe: &str, args: &[&str]| util::launch_as_user(exe, &args.join(" "), "");
    match what {
        "taskmgr" => spawn("taskmgr.exe", &[]),
        "sound" => spawn("control.exe", &["mmsys.cpl,,0"]),
        "network" => spawn("control.exe", &["ncpa.cpl"]),
        "power" => spawn("control.exe", &["powercfg.cpl"]),
        "programs" => spawn("control.exe", &["appwiz.cpl"]),
        "envvars" => spawn("rundll32.exe", &["sysdm.cpl,EditEnvironmentVariables"]),
        "downloads" => util::shell_open(&util::downloads().to_string_lossy()),
        "inbox" => {
            let d = remote::inbox_dir();
            let _ = std::fs::create_dir_all(&d);
            util::shell_open(&d.to_string_lossy())
        }
        "terminal" => spawn("wt.exe", &[]).or_else(|_| spawn("cmd.exe", &[])),
        _ => Err("unknown".into()),
    }
}

#[tauri::command]
async fn games_list() -> R<Vec<games::Game>> {
    Ok(games::installed())
}

// ---- clipboard

#[tauri::command]
fn clip_list(app: A, q: String, limit: Option<usize>) -> Vec<clipboard::Item> {
    app.clip.list(&q, limit.unwrap_or(200))
}

/// Pastes into the window that was in front before Deck opened.
fn paste_back(handle: &AppHandle) {
    if app_mode(handle) {
        if let Some(w) = handle.get_webview_window("main") {
            let _ = w.minimize();
        }
        std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_millis(250));
            clipboard::send_paste();
        });
        return;
    }
    hide_deck(handle);
    let prev = PREV_FG.load(Ordering::Relaxed);
    let app = handle.state::<Arc<App>>().inner().clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(90));
        if prev != 0 {
            unsafe {
                let _ = SetForegroundWindow(HWND(prev as *mut _));
            }
            std::thread::sleep(std::time::Duration::from_millis(110));
        }
        // Ctrl+V only into the window it came from, and never into a fullscreen game: if that
        // window is gone or didn't come back to the front, the item just stays on the clipboard
        let back = prev != 0 && unsafe { GetForegroundWindow().0 as isize } == prev;
        if back && !hotcorner::fullscreen_in_front() {
            clipboard::send_paste();
        } else {
            app.toast(app::Toast::simple("paste", "Copied", "Press Ctrl+V where you want it."));
        }
    });
}

#[tauri::command]
fn clip_copy(handle: AppHandle, app: A, id: u64, paste: bool) -> R<()> {
    app.clip.copy_item(id)?;
    if paste {
        paste_back(&handle);
    }
    Ok(())
}

#[tauri::command]
fn set_clipboard(handle: AppHandle, text: String, paste: Option<bool>) -> R<()> {
    clipboard::set_text(&text)?;
    if paste.unwrap_or(false) {
        paste_back(&handle);
    }
    Ok(())
}

#[tauri::command]
fn clip_pin(app: A, id: u64, on: bool) {
    app.clip.pin(id, on)
}

#[tauri::command]
fn clip_remove(app: A, id: u64) {
    app.clip.remove(id)
}

#[tauri::command]
fn clip_clear(app: A) {
    app.clip.clear()
}

// ---- power

#[tauri::command]
fn set_awake(app: A, mode: String, minutes: Option<u64>, target: Option<String>, display: Option<bool>) {
    let mut a = app.power.awake.lock();
    if a.mode != mode {
        a.dl_seen = false;
        a.dl_quiet_since = None;
    }
    a.mode = mode;
    if let Some(m) = minutes {
        a.until = util::now_s() + m * 60;
    }
    if let Some(t) = target {
        a.app = t;
    }
    if let Some(d) = display {
        a.display = d;
    }
    drop(a);
    app.changed("power");
}

#[tauri::command]
fn set_plan(app: A, plan: power::Plan) {
    let mut p = plan;
    p.state = "waiting".into();
    p.created = util::now_s();
    p.progress.clear();
    *app.power.plan.lock() = Some(p);
    app.changed("power");
}

#[tauri::command]
fn cancel_plan(app: A) {
    *app.power.plan.lock() = None;
    app.dismiss_toast("plan");
    app.changed("power");
}

#[tauri::command]
fn run_power(action: String) -> R<()> {
    power::run_action(&action)
}

#[tauri::command]
fn timer_add(app: A, secs: u64, label: String) -> u64 {
    let id = app.power.add_timer(secs, &label);
    app.changed("power");
    id
}

#[tauri::command]
fn timer_cmd(app: A, id: u64, cmd: String) {
    app.power.timer_cmd(id, &cmd);
    app.changed("power");
}

#[tauri::command]
fn stopwatch(app: A, cmd: String) {
    app.power.stopwatch_cmd(&cmd);
    app.changed("power");
}

#[tauri::command]
fn toast_action(app: A, id: String, action: String) {
    app.toast_action_as(app::Caller::Local, &id, &action);
}

#[tauri::command]
fn toast_resize(handle: AppHandle, height: f64) {
    let Some(w) = handle.get_webview_window("toast") else { return };
    if height < 4.0 {
        let _ = w.hide();
        return;
    }
    let (_, work, scale) = monitor_info(POINT::default(), true);
    let (pw, ph) = ((380.0 * scale) as i32, (height * scale) as i32);
    let _ = w.set_size(tauri::PhysicalSize::new(pw as u32, ph as u32));
    let _ = w.set_position(tauri::PhysicalPosition::new(work.right - pw - (12.0 * scale) as i32, work.bottom - ph - (12.0 * scale) as i32));
    show_no_activate(&w);
}

// ---- windows

#[tauri::command]
async fn windows_list() -> R<Vec<winmgr::Win>> {
    Ok(winmgr::list())
}

#[tauri::command]
fn window_action(hwnd: i64, action: String, value: Option<f64>) -> R<()> {
    winmgr::action(hwnd, &action, value)
}

#[tauri::command]
fn snap_window(app: A, hwnd: i64, layout: String, zone: usize) -> R<()> {
    let l = app.settings.lock().layouts.iter().find(|l| l.id == layout).cloned().ok_or("No such layout")?;
    winmgr::snap(hwnd, &l, zone)
}

#[tauri::command]
fn ws_capture(app: A, name: String, icon: String) -> R<String> {
    let windows = winmgr::capture();
    if windows.is_empty() {
        return Err("No app windows are open".into());
    }
    let id = format!("ws{}", util::now_ms());
    app.settings.lock().workspaces.push(settings::Workspace { id: id.clone(), name, icon, windows, ..Default::default() });
    app.save_settings();
    Ok(id)
}

#[tauri::command]
fn ws_recapture(app: A, id: String) -> R<()> {
    let windows = winmgr::capture();
    let mut s = app.settings.lock();
    let ws = s.workspaces.iter_mut().find(|w| w.id == id).ok_or("Workspace not found")?;
    ws.windows = windows;
    drop(s);
    app.save_settings();
    Ok(())
}

#[tauri::command]
fn ws_open(handle: AppHandle, app: A, id: String) -> R<()> {
    let ws = app.settings.lock().workspaces.iter().find(|w| w.id == id).cloned().ok_or("Workspace not found")?;
    step_aside(&handle);
    winmgr::open_workspace(app.inner(), ws);
    Ok(())
}

/// A file dialog owned by Deck's window, so it opens in front of it (Deck can be always on top,
/// and a dialog without an owner then lands behind it).
fn file_dialog(handle: &AppHandle, title: String) -> tauri_plugin_dialog::FileDialogBuilder<tauri::Wry> {
    let d = handle.dialog().file().set_title(title);
    match handle.get_webview_window("main") {
        Some(w) => d.set_parent(&w),
        None => d,
    }
}

#[tauri::command]
async fn pick(handle: AppHandle, folder: bool, title: Option<String>) -> R<Option<String>> {
    DIALOG.store(true, Ordering::Relaxed);
    let d = file_dialog(&handle, title.unwrap_or_else(|| "Choose".into()));
    let r = if folder { d.blocking_pick_folder() } else { d.blocking_pick_file() };
    DIALOG.store(false, Ordering::Relaxed);
    Ok(r.and_then(|f| f.into_path().ok()).map(|p| p.to_string_lossy().to_string()))
}

/// Several files at once (Transfer's "Send files").
#[tauri::command]
async fn pick_many(handle: AppHandle, title: Option<String>) -> R<Vec<String>> {
    DIALOG.store(true, Ordering::Relaxed);
    let r = file_dialog(&handle, title.unwrap_or_else(|| "Choose".into())).blocking_pick_files();
    DIALOG.store(false, Ordering::Relaxed);
    Ok(r.unwrap_or_default().into_iter().filter_map(|f| f.into_path().ok()).map(|p| p.to_string_lossy().to_string()).collect())
}

#[tauri::command]
async fn fonts_list() -> R<Vec<String>> {
    Ok(display::fonts())
}

#[tauri::command]
fn index_rebuild(app: A) {
    let roots = app.settings.lock().index_roots.clone();
    app.files.rebuild(roots);
}

#[tauri::command]
fn index_info(app: A) -> Value {
    app.files.info()
}

// ---- screen tools

#[tauri::command]
async fn screen_tool(handle: AppHandle, app: A<'_>, mode: String) -> R<Value> {
    let m = match mode.as_str() {
        "color" => picker::Mode::Color,
        "text" => picker::Mode::Text,
        "measure" => picker::Mode::Measure,
        _ => return Err("unknown tool".into()),
    };
    let was_visible = app.is_visible();
    {
        let h2 = handle.clone();
        let _ = handle.run_on_main_thread(move || hide_deck(&h2));
    }
    // let Deck's window disappear before the screen is frozen
    std::thread::sleep(std::time::Duration::from_millis(250));
    let out = std::thread::spawn(move || picker::run(m)).join().unwrap_or(picker::Outcome::Cancelled);
    let res = match out {
        picker::Outcome::Color(r, g, b) => {
            let hex = format!("#{r:02X}{g:02X}{b:02X}");
            let _ = clipboard::set_text(&hex);
            {
                let mut s = app.settings.lock();
                s.colors.retain(|c| c != &hex);
                s.colors.insert(0, hex.clone());
                s.colors.truncate(40);
            }
            app.save_settings();
            app.toast(Toast::simple("color", &format!("Copied {hex}"), &format!("rgb({r}, {g}, {b})")));
            json!({ "kind": "color", "hex": hex, "rgb": [r, g, b] })
        }
        picker::Outcome::Text(t) => {
            let _ = clipboard::set_text(&t);
            let preview: String = t.chars().take(140).collect();
            app.toast(Toast::simple("ocr", &format!("Copied {} characters", t.chars().count()), &preview));
            json!({ "kind": "text", "text": t })
        }
        picker::Outcome::Measure(w, h) => {
            let _ = clipboard::set_text(&format!("{w} x {h}"));
            app.toast(Toast::simple("measure", &format!("{w} × {h} px"), "Copied to the clipboard."));
            json!({ "kind": "measure", "w": w, "h": h })
        }
        picker::Outcome::Failed(e) => {
            app.toast(Toast::simple("tool-err", "Didn't work", &e));
            json!({ "kind": "error", "error": e })
        }
        picker::Outcome::Cancelled => json!({ "kind": "cancelled" }),
    };
    if was_visible && res["kind"] == "cancelled" {
        let h2 = handle.clone();
        let _ = handle.run_on_main_thread(move || show_deck(&h2, None));
    }
    Ok(res)
}

// ---- phone

fn shared_file(p: std::path::PathBuf) -> Shared {
    let size = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
    Shared {
        id: 0,
        kind: "file".into(),
        text: String::new(),
        name: p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
        path: Some(p.to_string_lossy().to_string()),
        size,
        ts: util::now_ms(),
        from: "pc".into(),
    }
}

#[tauri::command]
async fn share_to_phone(handle: AppHandle, app: A<'_>, kind: String, text: Option<String>) -> R<()> {
    let item = match kind.as_str() {
        "file" => {
            DIALOG.store(true, Ordering::Relaxed);
            let picked = file_dialog(&handle, "Send a file to your phone".into()).blocking_pick_file();
            DIALOG.store(false, Ordering::Relaxed);
            let Some(p) = picked.and_then(|f| f.into_path().ok()) else { return Ok(()) };
            shared_file(p)
        }
        "path" => {
            let p = std::path::PathBuf::from(text.unwrap_or_default());
            if !p.is_file() {
                return Err("Not a file".into());
            }
            shared_file(p)
        }
        _ => {
            let t = text.unwrap_or_default();
            if t.trim().is_empty() {
                return Err("Nothing to send".into());
            }
            let k = if t.starts_with("http://") || t.starts_with("https://") { "link" } else { "text" };
            Shared { id: 0, kind: k.into(), size: t.len() as u64, text: t, path: None, name: String::new(), ts: util::now_ms(), from: "pc".into() }
        }
    };
    remote::push_shared(&app, item);
    Ok(())
}

// ------------------------------------------------------------------ tray

/// "Start with Windows" = the elevated logon task on or off. Unelevated (no task yet) there is
/// nothing to change; Set up creates the task with this setting.
fn sync_autostart(_handle: &AppHandle, on: bool) {
    if elevation::is_admin() {
        if let Err(e) = elevation::set_task_enabled(on) {
            log::warn(format!("autostart: {e}"));
        }
    }
}

fn tray_menu(handle: &AppHandle, app: &App) -> tauri::Result<Menu<tauri::Wry>> {
    use tauri::menu::{IsMenuItem, Submenu};
    let awake = app.power.awake.lock().mode != "off";
    let boost_on = boost::is_on();
    let (cpu, gpu, act_c, act_g) = {
        let s = app.settings.lock();
        (s.cpu_profiles.iter().map(|p| (p.id.clone(), p.name.clone())).collect::<Vec<_>>(), s.gpu_profiles.iter().map(|p| (p.id.clone(), p.name.clone())).collect::<Vec<_>>(), s.active_cpu.clone(), s.active_gpu.clone())
    };
    let sub = |label: &str, kind: &str, list: &[(String, String)], active: &str| -> tauri::Result<Option<Submenu<tauri::Wry>>> {
        if list.is_empty() {
            return Ok(None);
        }
        let items: Vec<CheckMenuItem<tauri::Wry>> = list.iter().map(|(id, name)| CheckMenuItem::with_id(handle, format!("prof:{kind}:{id}"), name, true, id == active, None::<&str>)).collect::<Result<_, _>>()?;
        let refs: Vec<&dyn IsMenuItem<tauri::Wry>> = items.iter().map(|i| i as &dyn IsMenuItem<tauri::Wry>).collect();
        Ok(Some(Submenu::with_items(handle, label, true, &refs)?))
    };
    let cpu_menu = sub("CPU profile", "cpu", &cpu, &act_c)?;
    let gpu_menu = sub("GPU profile", "gpu", &gpu, &act_g)?;
    let engine = app.hw_engine();
    let lights = engine.as_ref().filter(|sh| sh.status.lock_anyway().rgb.1).map(|sh| sh.lights_off.load(Ordering::Relaxed));
    let lights_item = match lights {
        Some(off) => Some(CheckMenuItem::with_id(handle, "lights", "Lights off", true, off, None::<&str>)?),
        None => None,
    };
    // MoboCtl's tray had these too: every fan to one mode, the cooler display, Clear RAM
    let fans_menu = match engine.as_ref().filter(|sh| sh.fan_control_allowed() && !sh.hw.lock_anyway().fan_names.is_empty()) {
        Some(sh) => {
            let (common, modes) = hw::cfg::fan_modes(sh);
            let items: Vec<CheckMenuItem<tauri::Wry>> = modes
                .iter()
                .map(|m| CheckMenuItem::with_id(handle, format!("fans:{m}"), m.strip_prefix("Curve: ").unwrap_or(m), true, common.as_deref() == Some(m.as_str()), None::<&str>))
                .collect::<Result<_, _>>()?;
            let refs: Vec<&dyn IsMenuItem<tauri::Wry>> = items.iter().map(|i| i as &dyn IsMenuItem<tauri::Wry>).collect();
            Some(Submenu::with_items(handle, "All fans", true, &refs)?)
        }
        None => None,
    };
    let disp_item = match engine.as_ref().filter(|sh| sh.status.lock_anyway().disp.1) {
        Some(sh) => Some(CheckMenuItem::with_id(handle, "display", "Cooler display off", true, sh.dp.lock_anyway().off, None::<&str>)?),
        None => None,
    };
    let ram_item = if elevation::is_admin() { Some(MenuItem::with_id(handle, "clearram", "Clear RAM", true, None::<&str>)?) } else { None };
    // the controller overlay (Controller Overlay's own tray menu, now a submenu of Deck's)
    let pad_menu = {
        let (on, kbm) = {
            let s = app.settings.lock();
            (s.pad.on, s.pad.mode == "kbm")
        };
        let (edit, demo) = pad::shared().map(|sh| (sh.edit.load(Ordering::Relaxed), sh.demo.load(Ordering::Relaxed))).unwrap_or_default();
        let show = CheckMenuItem::with_id(handle, "pad:visible", "Show overlay", true, on, None::<&str>)?;
        let mv = CheckMenuItem::with_id(handle, "pad:edit", "Move / resize", true, edit, None::<&str>)?;
        let sep = PredefinedMenuItem::separator(handle)?;
        let ctl = CheckMenuItem::with_id(handle, "pad:controller", "Controller", true, !kbm, None::<&str>)?;
        let km = CheckMenuItem::with_id(handle, "pad:kbm", "Keyboard && mouse", true, kbm, None::<&str>)?;
        let dm = CheckMenuItem::with_id(handle, "pad:demo", "Demo input", true, demo, None::<&str>)?;
        let sep2 = PredefinedMenuItem::separator(handle)?;
        let set = MenuItem::with_id(handle, "pad:open", "Settings…", true, None::<&str>)?;
        Submenu::with_items(handle, "Controller overlay", true, &[&show, &mv, &sep, &ctl, &km, &dm, &sep2, &set])?
    };
    // the death counter: the game in front (or last in front), its fight and the last bosses
    let deaths_menu = {
        let info = if app.settings.lock().deaths.on { deaths::tray_info(app) } else { None };
        match info {
            Some((game, total, open, names, can_undo, counted)) => {
                let mut owned: Vec<Box<dyn IsMenuItem<tauri::Wry>>> = vec![];
                let head = match &open {
                    Some(b) => format!("{game}  ·  {b}"),
                    None => format!("{game}  ·  {total} death{}", if total == 1 { "" } else { "s" }),
                };
                owned.push(Box::new(MenuItem::with_id(handle, "deaths:head", head, false, None::<&str>)?));
                owned.push(Box::new(MenuItem::with_id(handle, "deaths:death", "+1 death", counted, None::<&str>)?));
                owned.push(Box::new(MenuItem::with_id(handle, "deaths:undo", "Undo last death", counted && can_undo, None::<&str>)?));
                owned.push(Box::new(PredefinedMenuItem::separator(handle)?));
                if open.is_some() {
                    owned.push(Box::new(MenuItem::with_id(handle, "deaths:win", "Boss defeated", counted, None::<&str>)?));
                    owned.push(Box::new(MenuItem::with_id(handle, "deaths:abandon", "Abandon fight", counted, None::<&str>)?));
                }
                owned.push(Box::new(MenuItem::with_id(handle, "deaths:new", "Start boss fight…", counted, None::<&str>)?));
                for (i, n) in names.iter().enumerate().filter(|(_, n)| Some(*n) != open.as_ref()) {
                    owned.push(Box::new(MenuItem::with_id(handle, format!("deaths:fight:{i}"), format!("Fight {n}"), counted, None::<&str>)?));
                }
                owned.push(Box::new(PredefinedMenuItem::separator(handle)?));
                owned.push(Box::new(CheckMenuItem::with_id(handle, "deaths:count", "Count deaths in this game", true, counted, None::<&str>)?));
                owned.push(Box::new(MenuItem::with_id(handle, "deaths:open", "Open death counter…", true, None::<&str>)?));
                let refs: Vec<&dyn IsMenuItem<tauri::Wry>> = owned.iter().map(|b| b.as_ref()).collect();
                Some(Submenu::with_items(handle, "Death counter", true, &refs)?)
            }
            None if app.settings.lock().deaths.on => {
                let open = MenuItem::with_id(handle, "deaths:open", "Open death counter…", true, None::<&str>)?;
                Some(Submenu::with_items(handle, "Death counter", true, &[&open])?)
            }
            None => None,
        }
    };
    let open = MenuItem::with_id(handle, "open", "Open Deck", true, None::<&str>)?;
    let s1 = PredefinedMenuItem::separator(handle)?;
    let boost_i = CheckMenuItem::with_id(handle, "boost", "Full power for gaming", true, boost_on, None::<&str>)?;
    let clean = MenuItem::with_id(handle, "clean", "Clean up background apps", true, None::<&str>)?;
    let s2 = PredefinedMenuItem::separator(handle)?;
    let s3 = PredefinedMenuItem::separator(handle)?;
    let clip = MenuItem::with_id(handle, "clip", "Clipboard history", true, None::<&str>)?;
    let color = MenuItem::with_id(handle, "color", "Pick a colour from the screen", true, None::<&str>)?;
    let text = MenuItem::with_id(handle, "text", "Copy text from the screen", true, None::<&str>)?;
    let awake_i = CheckMenuItem::with_id(handle, "awake", "Keep the PC awake", true, awake, None::<&str>)?;
    let s4 = PredefinedMenuItem::separator(handle)?;
    let settings = MenuItem::with_id(handle, "settings", "Settings", true, None::<&str>)?;
    let quit = MenuItem::with_id(handle, "quit", "Quit Deck", true, None::<&str>)?;
    let mut items: Vec<&dyn IsMenuItem<tauri::Wry>> = vec![&open, &s1, &boost_i, &clean];
    let hw_part = cpu_menu.is_some() || gpu_menu.is_some() || lights_item.is_some() || fans_menu.is_some() || disp_item.is_some();
    if hw_part {
        items.push(&s2);
    }
    if let Some(m) = &cpu_menu {
        items.push(m);
    }
    if let Some(m) = &gpu_menu {
        items.push(m);
    }
    if let Some(m) = &fans_menu {
        items.push(m);
    }
    if let Some(m) = &lights_item {
        items.push(m);
    }
    if let Some(m) = &disp_item {
        items.push(m);
    }
    if let Some(m) = &ram_item {
        items.push(m);
    }
    items.push(&s3);
    if let Some(m) = &deaths_menu {
        items.push(m);
    }
    items.extend([&pad_menu as &dyn IsMenuItem<tauri::Wry>, &clip, &color, &text, &awake_i, &s4, &settings, &quit]);
    Menu::with_items(handle, &items)
}

fn refresh_tray(handle: &AppHandle) {
    let app = handle.state::<Arc<App>>().inner().clone();
    if let (Some(tray), Ok(menu)) = (handle.tray_by_id("main"), tray_menu(handle, &app)) {
        let _ = tray.set_menu(Some(menu));
    }
    // the controller battery icons open the same menu
    for id in PAD_TRAYS.lock().iter().map(|(id, ..)| id.clone()).collect::<Vec<_>>() {
        if let (Some(tray), Ok(menu)) = (handle.tray_by_id(&id), tray_menu(handle, &app)) {
            let _ = tray.set_menu(Some(menu));
        }
    }
}

/// The battery tray icons shown now: (id, tooltip, pixels).
static PAD_TRAYS: parking_lot::Mutex<Vec<(String, String, Vec<u8>)>> = parking_lot::Mutex::new(Vec::new());

/// One tray icon per controller that reports a battery (the Controller page's Battery tab can
/// turn them off). Left-click opens the Controller page; right-click is Deck's tray menu.
fn sync_pad_trays(handle: &AppHandle) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSMICON};
    let app = handle.state::<Arc<App>>().inner().clone();
    let size = unsafe { GetSystemMetrics(SM_CXSMICON) }.max(16) as u32;
    let want = if app.settings.lock().pad.battery_tray { pad::tray_icons(size) } else { vec![] };
    let mut shown = PAD_TRAYS.lock();
    for (id, ..) in shown.iter().filter(|(id, ..)| !want.iter().any(|w| &w.0 == id)) {
        let _ = handle.remove_tray_by_id(id);
    }
    for (id, tip, px) in &want {
        match shown.iter().find(|s| &s.0 == id) {
            Some((_, old_tip, old_px)) => {
                if let Some(tray) = handle.tray_by_id(id) {
                    if old_px != px {
                        let _ = tray.set_icon(Some(tauri::image::Image::new_owned(px.clone(), size, size)));
                    }
                    if old_tip != tip {
                        let _ = tray.set_tooltip(Some(tip));
                    }
                }
            }
            None => {
                let Ok(menu) = tray_menu(handle, &app) else { continue };
                let built = TrayIconBuilder::with_id(id)
                    .icon(tauri::image::Image::new_owned(px.clone(), size, size))
                    .tooltip(tip)
                    .menu(&menu)
                    .show_menu_on_left_click(false)
                    .on_tray_icon_event(|tray, ev| {
                        if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = ev {
                            show_deck(tray.app_handle(), Some("controller"));
                        }
                    })
                    .build(handle);
                if let Err(e) = built {
                    log::warn(format!("controller battery tray icon: {e}"));
                }
            }
        }
    }
    *shown = want;
}

/// Full power / Clean up from the tray or the phone, with a toast saying what happened.
pub fn boost_from_tray(app: &App, what: &str) {
    let r = match what {
        "boost" if boost::is_on() => boost::stop(app, "turned off").map(|_| None),
        "boost" => boost::start(app).map(Some),
        _ => boost::clean(app).map(Some),
    };
    match r {
        Ok(Some(v)) => {
            let (title, body) = boost::summary(what, &v);
            app.toast(Toast::simple("boost", &title, &body));
        }
        Ok(None) => {}
        Err(e) => app.toast(Toast::simple("boost", "Game boost", &e)),
    }
}

fn run_tool_from_tray(handle: &AppHandle, mode: &str) {
    let h = handle.clone();
    let mode = mode.to_string();
    tauri::async_runtime::spawn(async move {
        let app = h.state::<Arc<App>>();
        let _ = screen_tool(h.clone(), app, mode).await;
    });
}

// ------------------------------------------------------------ debug modes

#[cfg(debug_assertions)]
/// Read-only checks of every system integration. Changes nothing.
fn selftest() {
    std::env::set_var("DECK_PROFILE", "test");
    let app = App::new();
    let _ = app.sys.snapshot();
    std::thread::sleep(std::time::Duration::from_millis(600));
    let s = app.sys.snapshot();
    println!("== sys cpu {:.0}%  ram {:.0}%  gpu {}", s["cpu"].as_f64().unwrap_or(0.0), s["ram_pct"].as_f64().unwrap_or(0.0), s["gpu"]);
    println!("== outputs {:?}", audio::outputs().iter().map(|d| (&d.name, d.default)).collect::<Vec<_>>());
    println!("== apps with sound {:?}", audio::apps().iter().map(|a| &a.name).collect::<Vec<_>>());
    println!("== media {}", app.media.state()["current"]["title"]);
    println!("== start menu apps {}", app.apps.list(false).len());
    println!("== games {:?}", games::installed().iter().map(|g| &g.name).take(10).collect::<Vec<_>>());
    println!("== windows {:?}", winmgr::list().iter().map(|w| (&w.app, w.topmost)).take(12).collect::<Vec<_>>());
    println!("== monitors {}", serde_json::to_string(&display::monitors()).unwrap());
    println!("== dark {}", display::is_dark());
    println!("== startup {:?}", startup::list().iter().map(|e| (&e.name, e.enabled, e.can_change)).collect::<Vec<_>>());
    println!("== ping {:?}", net::ping_many(&["1.1.1.1".into()]));
    println!("== game list {:?}", gameping::games().iter().map(|g| format!("{} {}", g["name"], g["installed"])).collect::<Vec<_>>());
    for prov in ["cs2", "fortnite", "rocketleague", "dbd"] {
        let t = std::time::Instant::now();
        match gameping::ping(prov) {
            Ok(v) => println!("== game ping {prov} ({:.1} s): {} rows, fastest {:?}", t.elapsed().as_secs_f64(), v.len(), v.iter().take(4).map(|x| format!("{} {:?} [{}]", x["name"], x["ms"].as_f64().map(|m| m.round()), x["sub"].as_str().unwrap_or(""))).collect::<Vec<_>>()),
            Err(e) => println!("== game ping {prov}: {e}"),
        }
    }
    println!("== lan {:?}", net::lan_ip());
    let (rb, n) = cleanup::recycle_bin();
    println!("== recycle bin {} ({n} items)", util::fmt_bytes(rb));
    println!("== fullscreen in front {}", hotcorner::fullscreen_in_front());
    println!("== idle {}s", power::idle_secs());
    for q in ["2+2*3", "5 km to mi", "20 usd to eur", "255 to hex"] {
        println!("   calc {q:>14} -> {:?} {:?}", calc::eval(q), calc::base_convert(q).or_else(|| calc::convert(q, &app.rates)));
    }
}

#[cfg(debug_assertions)]
/// Prints the UI state as JSON for offscreen UI previews. Changes nothing.
fn dump_state() {
    std::env::set_var("DECK_PROFILE", "test");
    let app = App::new();
    let _ = app.sys.snapshot();
    std::thread::sleep(std::time::Duration::from_millis(700));
    let windowed: std::collections::HashMap<u32, (String, bool)> = winmgr::list().into_iter().map(|w| (w.pid, (w.title, w.hung))).collect();
    let procs = app.sys.processes(&windowed);
    let v = json!({
        "state": {
            "settings": *app.settings.lock(),
            "version": env!("CARGO_PKG_VERSION"),
            "pc": std::env::var("COMPUTERNAME").unwrap_or_default(),
            "user": std::env::var("USERNAME").unwrap_or_default(),
            "remote": { "enabled": true, "lan": "http://192.168.1.42:47830/#k=preview", "origin": "http://192.168.1.42:47830", "qr": remote::qr_svg("http://192.168.1.42:47830/#k=preview"), "port": 47830, "clients": 0, "shared": [] },
            "index": app.files.info(),
            "home": util::home().to_string_lossy(),
        },
        "live": {
            "sys": app.sys.snapshot(),
            "media": app.media.state(),
            "master": audio::master(),
            "apps": audio::apps(),
            "power": app.power.state(),
            "toasts": [],
        },
        "outputs": audio::outputs(),
        "applist": app.apps.list(false),
        "games": games::installed(),
        "windows": winmgr::list(),
        "display": display::state(&app),
        "startup": startup::list(),
        "procs": procs.into_iter().take(40).collect::<Vec<_>>(),
        "net": app.net.info(),
    });
    println!("{v}");
}

#[cfg(debug_assertions)]
/// Read-only data for the UI previews of the hardware pages (nothing is written anywhere).
fn dump_hw() {
    std::env::set_var("DECK_PROFILE", "test");
    let g = gpu::Gpu::new();
    let lim = hw::cpupower::limits();
    let plans: Vec<Value> = profiles::plans::list()
        .iter()
        .map(|gd| {
            let mut p = profiles::CpuProfile::default();
            profiles::cpu::read_plan(gd, &mut p);
            json!({ "guid": profiles::plans::guid_str(gd), "name": profiles::plans::name(gd), "active": profiles::plans::active() == Some(*gd), "used": false, "summary": profiles::cpu_summary(&p), "profile": p })
        })
        .collect();
    let ab = g.as_ref().and_then(|g| profiles::afterburner::import(&g.caps.pci).ok());
    let v = json!({
        "lists": hw::cfg::lists(),
        "geometry": hw::lcd::geometry(),
        "cfg": hw::cfg::HwCfg::default(),
        "icons": profiles::icons::previews().iter().map(|(n, u)| json!({ "name": n, "url": u })).collect::<Vec<_>>(),
        "rtss": rtss::status(),
        "plans": plans,
        "hybrid": profiles::cpu::hybrid(),
        "limits": { "base": lim.base, "turbo": lim.turbo, "min": lim.min, "oc": lim.oc_possible, "oc_checked": lim.oc_checked, "oc_max": lim.turbo + hw::cpupower::OC_MAX_EXTRA },
        "gpu": g.as_ref().map(|g| json!({ "caps": g.caps, "current": g.current(), "live": g.live(),
            "curve": g.curve().map(|c| c.core.iter().map(|p| json!({ "mv": p.mv, "base": p.base_mhz, "delta": p.delta_mhz })).collect::<Vec<_>>()) })),
        "ab": ab.as_ref().map(profiles::afterburner::summary),
        "ab_profiles": ab.map(|a| a.profiles),
        "board": hw::smbios::read().board_name,
    });
    println!("{v}");
}

#[cfg(debug_assertions)]
/// Runs the phone server alone for 20 s (DECK_SELFTEST_SECS) on port 47839 with a test profile
/// (DECK_SELFTEST_SCREEN: Screen view on).
/// Nothing on the PC is changed by it.
fn selftest_remote() {
    std::env::set_var("DECK_PROFILE", "test");
    let app = App::new();
    app.settings.lock().remote_port = 47839;
    // a test profile whose key can't be unsealed here (made by another account) reads as empty
    if app.settings.lock().remote_token.len() < 16 {
        app.settings.lock().remote_token = util::random_token(22);
    }
    // DECK_SELFTEST_SCREEN: the Screen view on (it's off in a new profile), for stream tests
    if std::env::var("DECK_SELFTEST_SCREEN").is_ok() {
        app.settings.lock().remote_screen = "view".into();
    }
    // DECK_SELFTEST_AUDIO: the phone as speaker and mic on (both off in a new profile)
    if std::env::var("DECK_SELFTEST_AUDIO").is_ok() {
        let mut s = app.settings.lock();
        s.remote_speaker = true;
        s.remote_mic = true;
    }
    let tmp = std::env::temp_dir().join("deck-share-test.txt");
    let _ = std::fs::write(&tmp, "hello from the PC");
    remote::push_shared(&app, shared_file(tmp));
    let r = Remote::new();
    remote::start(app.clone(), r.clone());
    std::thread::sleep(std::time::Duration::from_millis(600));
    println!("PORT {}", r.port.lock().unwrap_or(0));
    println!("TOKEN {}", app.settings.lock().remote_token);
    use std::io::Write;
    let _ = std::io::stdout().flush();
    std::thread::sleep(std::time::Duration::from_secs(std::env::var("DECK_SELFTEST_SECS").ok().and_then(|v| v.parse().ok()).unwrap_or(20)));
    println!("TIMERS {}", app.power.timers.lock().len());
    println!("SHARED {}", app.shared.lock().len());
}

#[cfg(debug_assertions)]
/// Prints {"app:<id>" | "file:<path>": data URL} for UI previews.
fn dump_icons() {
    std::env::set_var("DECK_PROFILE", "test");
    let a = apps::Apps::new();
    let mut keys: Vec<String> = a.list(false).iter().map(|e| format!("app:{}", e.id)).collect();
    keys.extend(winmgr::list().into_iter().filter_map(|w| w.exe).map(|e| format!("file:{e}")));
    keys.extend(audio::apps().into_iter().filter_map(|x| x.exe).map(|e| format!("file:{e}")));
    keys.extend(startup::list().into_iter().filter_map(|x| x.exe).map(|e| format!("file:{e}")));
    let mut out = serde_json::Map::new();
    for k in keys {
        if out.contains_key(&k) {
            continue;
        }
        if let Some(png) = a.icon(&k) {
            out.insert(k, Value::String(format!("data:image/png;base64,{}", util::base64(&png))));
        }
    }
    println!("{}", Value::Object(out));
}

/// `--switch <cpu|gpu>:<profile id>` from a shortcut (through its elevated task). Returns
/// true when the arguments were a request that must not show the window.
fn handle_args(h: &AppHandle, args: &[String]) -> bool {
    if args.iter().any(|a| a == "--quit") {
        quit_deck(h);
        return true;
    }
    // a hidden start (the logon task, \Deck\Background) while Deck already runs: nothing to show
    if args.iter().any(|a| a == "--minimized") {
        return true;
    }
    let Some(i) = args.iter().position(|a| a == "--switch") else { return false };
    let Some((kind, id)) = args.get(i + 1).and_then(|v| v.split_once(':')) else { return true };
    let app = h.state::<Arc<App>>().inner().clone();
    let (kind, id) = (kind.to_string(), id.to_string());
    std::thread::spawn(move || profiles::switch(&app, &kind, &id, "shortcut"));
    true
}

#[tauri::command]
fn elevate_setup(handle: AppHandle) -> R<()> {
    elevation::elevate_setup()?;
    // the elevated copy starts through the task; this one makes room for it
    let h = handle.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(1500));
        quit_deck(&h);
    });
    Ok(())
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("zz"), 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

/// Ends the process once the log has written what is queued (its writer is a background thread).
fn exit_logged(code: i32) -> ! {
    log::flush();
    std::process::exit(code)
}

fn main() {
    // before anything is loaded or any window made: an elevated Deck takes nothing that steers
    // WebView2 from the environment, which the user's own programs can set
    webview_guard::scrub_env(elevation::is_admin());
    // no libraries from the current folder (vendor DLLs load by full path, see util::load_trusted_dll)
    unsafe {
        windows_sys::Win32::System::LibraryLoader::SetDllDirectoryW([0u16].as_ptr());
    }
    // a stand-in Deck put where a broken uninstall expects a deleted program: do nothing, succeed
    if programs::is_stand_in() {
        std::process::exit(0);
    }
    // Claude Code's hook (Claude Sync): answers in milliseconds, never starts Deck
    if std::env::args().any(|a| a == "--claude-sync-hook") {
        exit_logged(ccsync::hook_main());
    }
    // the uninstaller (hooks.nsh): Claude Code must not keep calling a hook program that is gone
    if std::env::args().any(|a| a == "--uninstall-cleanup") {
        ccsync::uninstall_cleanup();
        exit_logged(0);
    }
    // PC to PC from the command line (scripts, Claude Code): prints, then exits
    {
        let args: Vec<String> = std::env::args().collect();
        if beam::cli::wanted(&args) {
            exit_logged(beam::cli::main(&args));
        }
    }
    // Transfer: a received installer, started as the user (see beam::setup::run_installer)
    {
        let args: Vec<String> = std::env::args().collect();
        if let Some(job) = args.iter().position(|a| a == "--run-installer").and_then(|i| args.get(i + 1)) {
            exit_logged(beam::setup::run_job(std::path::Path::new(job)));
        }
    }
    // Transfer: applying a received Windhawk setup needs admin when Deck runs without it
    for (flag, undo) in [("--transfer-apply-windhawk", false), ("--transfer-undo-windhawk", true)] {
        let args: Vec<String> = std::env::args().collect();
        if let Some(d) = args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)) {
            exit_logged(beam::snapshot::windhawk_elevated(std::path::Path::new(d), undo));
        }
    }
    #[cfg(debug_assertions)]
    {
        let arg = |a: &str| std::env::args().any(|x| x == a);
        if arg("--selftest") {
            return selftest();
        }
        if arg("--dump-state") {
            return dump_state();
        }
        if arg("--dump-icons") {
            return dump_icons();
        }
        if arg("--dump-hw") {
            return dump_hw();
        }
        if arg("--selftest-remote") {
            return selftest_remote();
        }
    }
    // `Deck.exe --setup-task [--start]` (elevated, from Set up or install.ps1): create the tasks
    if std::env::args().any(|a| a == "--setup-task") {
        log::set_role("setup");
        // from the installer: tasks made for another account (an administrator's password typed by a
        // standard user) would never start this user's Deck; the setup then starts it as the user
        if std::env::args().any(|a| a == "--from-setup") && elevation::runs_as_desktop_user() == Some(false) {
            log::warn("the setup runs as another account than the one signed in: Deck's admin tasks were not made (use Set up in Deck)");
            exit_logged(2);
        }
        let code = match std::env::current_exe().map_err(|e| e.to_string()).and_then(|exe| elevation::setup(&exe, settings::Settings::load().autostart)) {
            Ok(()) => {
                if std::env::args().any(|a| a == "--start") {
                    // wait (up to 15 s) for the unelevated Deck to quit, or the new one would hand over to it
                    let me = std::process::id();
                    for _ in 0..30 {
                        let mut sys = sysinfo::System::new();
                        sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
                        let others = sys.processes().values().any(|p| p.pid().as_u32() != me && p.name().to_string_lossy().eq_ignore_ascii_case("deck.exe"));
                        if !others {
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(500));
                    }
                    elevation::run_task(elevation::OPEN_TASK);
                }
                0
            }
            Err(e) => {
                log::warn(format!("setup: Deck's admin tasks couldn't be made: {e}"));
                eprintln!("{e}");
                1
            }
        };
        exit_logged(code);
    }
    // also in release builds: install.ps1 runs it elevated to prove apps start as the normal user
    if std::env::args().any(|a| a == "--launch-test") {
        let out = std::env::temp_dir().join("deck-launch-test.txt");
        let _ = std::fs::remove_file(&out);
        println!("admin: {}", elevation::is_admin());
        let r = util::launch_as_user("cmd.exe", &format!("/c %SystemRoot%\\System32\\whoami.exe /groups > \"{}\" 2>&1", out.display()), "");
        println!("launch: {r:?}");
        let mut t = String::new();
        for _ in 0..40 {
            std::thread::sleep(std::time::Duration::from_millis(250));
            t = std::fs::read(&out).map(|b| String::from_utf8_lossy(&b).to_string()).unwrap_or_default();
            if t.contains("Mandatory Level") {
                break;
            }
        }
        let level = ["High", "Medium"].iter().find(|l| t.contains(&format!("{l} Mandatory Level"))).unwrap_or(&"unknown");
        println!("child level: {level}");
        // release builds have no console: leave the answer in a file
        let _ = std::fs::write(std::env::temp_dir().join("deck-launch-test.result"), format!("admin={} child={level}", elevation::is_admin()));
        return;
    }
    // diagnostics, also in release builds (install.ps1 runs them elevated): --gpu-probe [--write],
    // --hw-selftest; the report goes to --out <file> when given (release builds have no console)
    {
        let args: Vec<String> = std::env::args().collect();
        let has = |a: &str| args.iter().any(|x| x == a);
        let report = if has("--gpu-probe") {
            Some(gpu::probe(has("--write")))
        } else if has("--cpu-probe") {
            Some(profiles::cpu::probe())
        } else if has("--hw-selftest") {
            Some(hw::selftest())
        } else if has("--pad-probe") {
            Some(pad::probe(4))
        } else {
            None
        };
        if let Some(text) = report {
            match args.iter().position(|a| a == "--out").and_then(|i| args.get(i + 1)) {
                Some(p) => {
                    let _ = std::fs::write(p, &text);
                }
                None => print!("{text}"),
            }
            return;
        }
    }
    // also in release builds (uninstall.ps1 uses it): fans back to the BIOS after a crash
    if std::env::args().any(|a| a == "--release-fans") {
        println!("{}", release_everything());
        return;
    }
    // the hardware fail-safe: waits for Deck to end, releases the hardware if it was ended from outside
    let args: Vec<String> = std::env::args().collect();
    if let Some(pid) = args.iter().position(|a| a == "--watchdog").and_then(|i| args.get(i + 1)).and_then(|p| p.parse::<u32>().ok()) {
        log::set_role("watchdog");
        if let watchdog::Outcome::Unclean { code } = watchdog::wait_for(pid) {
            let msg = release_everything();
            log::warn(format!("Deck (process {pid}) was ended from outside ({}); watchdog released the hardware: {msg}", watchdog::describe_exit(code)));
            // then Deck comes back by itself (hidden), unless it keeps ending
            restart::after_unclean_end(code);
        }
        log::flush();
        return;
    }
    // `--install-update <setup> <checksum url> <version>`: an update the user allowed with the UAC prompt
    if let Some(i) = args.iter().position(|a| a == "--install-update") {
        log::set_role("update");
        exit_logged(update::install_helper(&args[i + 1..]));
    }
    // a restart Deck asked for (after a driver install): once the old Deck is gone, start it hidden
    if let Some(pid) = args.iter().position(|a| a == "--start-after").and_then(|i| args.get(i + 1)).and_then(|p| p.parse::<u32>().ok()) {
        log::set_role("restart");
        // starting while the old Deck still quits would hand the start to it, and then nothing runs
        if !watchdog::wait_for_exit(pid) {
            log::warn("restart: the old Deck did not quit within 2 minutes; not starting another");
        } else if restart::other_deck_running() {
            log::info("restart: Deck is already running again");
        } else {
            elevation::run_task(elevation::BACKGROUND_TASK);
        }
        log::flush();
        return;
    }
    // started without admin rights (setup's finish page, a shortcut) while the admin tasks exist: let the task start it
    if elevation::handover() {
        return;
    }
    log::session_start();
    let minimized = std::env::args().any(|a| a == "--minimized" || a == "--switch" || a == "--quit");
    // `--quit` with no Deck running: nothing to do
    let quit_only = std::env::args().any(|a| a == "--quit");
    // a crash must not leave the fans on a fixed speed or the GPU fan pinned
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // the main thread ends the app; a hardware worker leaves its hardware unattended. Other
        // threads (a command, a background task) fail on their own and Deck carries on.
        let name = std::thread::current().name().map(str::to_string);
        // error: written at once, the process may be about to end
        log::error(format!("panic on thread {}: {info}", name.as_deref().unwrap_or("?")));
        let main = name.as_deref() == Some("main");
        if main || hw::on_worker_thread() {
            hw::on_panic();
        }
        if main || name.as_deref() == Some("gpu") {
            gpu::release();
        }
        default_hook(info);
    }));
    // a native crash (access violation) skips the panic hook: release the fans there too
    unsafe extern "system" fn on_crash(info: *const windows_sys::Win32::System::Diagnostics::Debug::EXCEPTION_POINTERS) -> i32 {
        let _ = hw::release_fans_direct();
        gpu::release();
        // what crashed and where (a driver's DLL, Deck itself), so the next report names the cause
        if let Some(rec) = info.as_ref().and_then(|i| i.ExceptionRecord.as_ref()) {
            log::error(format!("native crash {:#X} at {:p} in {}", rec.ExceptionCode as u32, rec.ExceptionAddress, watchdog::module_at(rec.ExceptionAddress as *const _)));
        }
        0 // EXCEPTION_CONTINUE_SEARCH: Windows reports the crash as usual
    }
    unsafe {
        windows_sys::Win32::System::Diagnostics::Debug::SetUnhandledExceptionFilter(Some(on_crash));
    }
    hw::winutil::on_session_end(Box::new(|| shutdown_hardware(true)));
    let app_state = App::new();
    let remote_state = Remote::new();

    let icon_app = app_state.clone();
    let tauri_app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|h, args, _cwd| {
            if !handle_args(h, &args) {
                show_deck(h, None)
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .register_asynchronous_uri_scheme_protocol("icon", move |_ctx, req, responder| {
            let app = icon_app.clone();
            std::thread::spawn(move || {
                let path = req.uri().path().trim_start_matches('/').to_string();
                let (kind, rest) = path.split_once('/').unwrap_or(("", ""));
                let rest = percent_decode(rest);
                let bytes = match kind {
                    "app" => app.apps.icon(&format!("app:{rest}")),
                    "file" => app.apps.icon(&format!("file:{rest}")),
                    "clip" if !rest.contains(['/', '\\']) && rest.ends_with(".png") => std::fs::read(app.clip.image_path(&rest)).ok(),
                    _ => None,
                };
                let resp = match bytes {
                    Some(b) => tauri::http::Response::builder()
                        .header("Content-Type", "image/png")
                        .header("Cache-Control", "max-age=3600")
                        .header("Access-Control-Allow-Origin", "*")
                        .body(b),
                    None => tauri::http::Response::builder().status(404).header("Access-Control-Allow-Origin", "*").body(Vec::new()),
                };
                if let Ok(r) = resp {
                    responder.respond(r);
                }
            });
        })
        .manage(app_state.clone())
        .manage(remote_state.clone())
        .setup(move |tapp| {
            let handle = tapp.handle().clone();
            let app = app_state.clone();
            // before the page first reads the settings: it would show the defaults until the next change
            pad::import_old(&app);
            // made here rather than by the config so it gets the same browser flags as the other windows
            let main_cfg = tapp.config().app.windows.iter().find(|w| w.label == "main").cloned().expect("main window config");
            WebviewWindowBuilder::from_config(&handle, &main_cfg)?.additional_browser_args(&webview_guard::browser_args()).build()?;
            webview_guard::spawn_watchdog(handle.clone(), "main");

            // events to the UI (main, toast and zones windows)
            {
                let h = handle.clone();
                app.set_ui(Box::new(move |event, payload| {
                    if event == "show-deck" {
                        let (h2, page) = (h.clone(), payload["page"].as_str().map(str::to_string));
                        let _ = h.run_on_main_thread(move || show_deck(&h2, page.as_deref()));
                        return;
                    }
                    if event == "toggle-deck" {
                        let h2 = h.clone();
                        let _ = h.run_on_main_thread(move || toggle_deck(&h2));
                        return;
                    }
                    let _ = h.emit(event, payload.clone());
                    if event == "changed" && (payload["topic"] == "pad" || payload["topic"] == "settings") {
                        let h2 = h.clone();
                        std::thread::spawn(move || {
                            let h3 = h2.clone();
                            let _ = h2.run_on_main_thread(move || sync_pad_trays(&h3));
                        });
                    }
                    if event == "changed" && (payload["topic"] == "power" || payload["topic"] == "boost" || payload["topic"] == "profiles" || payload["topic"] == "hw" || payload["topic"] == "pad" || payload["topic"] == "deaths") {
                        // posted from another thread: run_on_main_thread runs inline when called on
                        // the main thread, and the caller may still hold a lock tray_menu needs
                        let h2 = h.clone();
                        std::thread::spawn(move || {
                            let h3 = h2.clone();
                            let _ = h2.run_on_main_thread(move || refresh_tray(&h3));
                        });
                    }
                }));
            }

            let menu = tray_menu(&handle, &app)?;
            TrayIconBuilder::with_id("main")
                .icon(tapp.default_window_icon().cloned().expect("icon"))
                .tooltip("Deck")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|h, ev| {
                    let app = h.state::<Arc<App>>().inner().clone();
                    match ev.id().as_ref() {
                        "open" => show_deck(h, None),
                        "settings" => show_deck(h, Some("settings")),
                        "clip" => show_deck(h, Some("clipboard")),
                        "awake" => {
                            let mut a = app.power.awake.lock();
                            a.mode = if a.mode == "off" { "on".into() } else { "off".into() };
                            drop(a);
                            app.changed("power");
                        }
                        "color" => run_tool_from_tray(h, "color"),
                        "text" => run_tool_from_tray(h, "text"),
                        "boost" | "clean" => {
                            let what = ev.id().as_ref().to_string();
                            std::thread::spawn(move || boost_from_tray(&app, &what));
                        }
                        "quit" => quit_deck(h),
                        "lights" => {
                            if let Some(sh) = app.hw_engine() {
                                let off = !sh.lights_off.load(Ordering::Relaxed);
                                hw::cfg::set_lights_off(&sh, off);
                                app.changed("hw");
                            }
                        }
                        "display" => {
                            if let Some(sh) = app.hw_engine() {
                                let off = !sh.dp.lock_anyway().off;
                                hw::cfg::set_display_off(&sh, off);
                                app.changed("hw");
                            }
                        }
                        "clearram" => {
                            let app = app.clone();
                            std::thread::spawn(move || {
                                let msg = hw::memclean::clear();
                                app.toast(Toast::simple("clearram", "Clear RAM", &msg));
                            });
                        }
                        "pad:open" => show_deck(h, Some("controller")),
                        "deaths:open" | "deaths:new" => show_deck(h, Some("deaths")),
                        id if id.starts_with("deaths:") => {
                            let what = &id[7..];
                            let r = if what == "count" {
                                let on = deaths::tray_info(&app).map(|i| i.5).unwrap_or(true);
                                deaths::act(&app, if on { "off" } else { "on" }, None, None, "manual")
                            } else if let Some(i) = what.strip_prefix("fight:").and_then(|i| i.parse::<usize>().ok()) {
                                let boss = deaths::tray_info(&app).and_then(|t| t.3.get(i).cloned());
                                deaths::act(&app, "start", boss.as_deref(), None, "manual")
                            } else {
                                deaths::act(&app, what, None, None, "manual")
                            };
                            if let Err(e) = r {
                                app.toast(Toast::simple("deaths", "Death counter", &e));
                            }
                        }
                        id if id.starts_with("pad:") => {
                            if let Err(e) = pad::action(&app, &id[4..]) {
                                log::warn(e);
                            }
                            let h2 = h.clone();
                            std::thread::spawn(move || {
                                std::thread::sleep(std::time::Duration::from_millis(150));
                                let h3 = h2.clone();
                                let _ = h2.run_on_main_thread(move || refresh_tray(&h3));
                            });
                        }
                        id if id.starts_with("fans:") => {
                            if let Some(sh) = app.hw_engine() {
                                match hw::cfg::set_all_fans(&sh, &id[5..]) {
                                    Ok(()) => app.changed("hw"),
                                    Err(e) => app.toast(Toast::simple("fans", "Fans", &e)),
                                }
                            }
                        }
                        id if id.starts_with("prof:") => {
                            let mut p = id.splitn(3, ':').skip(1);
                            if let (Some(kind), Some(pid)) = (p.next(), p.next()) {
                                let (kind, pid) = (kind.to_string(), pid.to_string());
                                std::thread::spawn(move || profiles::switch(&app, &kind, &pid, "tray"));
                            }
                        }
                        _ => {}
                    }
                })
                .on_tray_icon_event(|tray, ev| {
                    if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = ev {
                        toggle_deck(tray.app_handle());
                    }
                })
                .build(tapp)?;

            build_aux_windows(&handle)?;

            // Zones overlay while dragging windows.
            {
                let h = handle.clone();
                winmgr::start_zones(
                    app.clone(),
                    Box::new(move |v: Option<Value>| {
                        let h2 = h.clone();
                        let _ = h.run_on_main_thread(move || {
                            let Some(w) = h2.get_webview_window("zones") else { return };
                            match v {
                                Some(v) => {
                                    let wk = &v["work"];
                                    let (x, y, ww, hh) = (wk[0].as_i64().unwrap_or(0), wk[1].as_i64().unwrap_or(0), wk[2].as_i64().unwrap_or(800), wk[3].as_i64().unwrap_or(600));
                                    let _ = w.set_position(tauri::PhysicalPosition::new(x as i32, y as i32));
                                    let _ = w.set_size(tauri::PhysicalSize::new(ww as u32, hh as u32));
                                    let _ = h2.emit_to("zones", "zones", v);
                                    show_no_activate(&w);
                                }
                                None => {
                                    let _ = h2.emit_to("zones", "zones", Value::Null);
                                    let _ = w.hide();
                                }
                            }
                        });
                    }),
                );
            }

            // Hot corner.
            {
                let h = handle.clone();
                hotcorner::start(app.clone(), move || {
                    let h2 = h.clone();
                    if app_mode(&h2) {
                        return; // the corner belongs to the quick panel
                    }
                    let _ = h.run_on_main_thread(move || toggle_deck(&h2));
                });
            }

            if quit_only {
                exit_logged(0);
            }
            app.start();
            pad::start(&app);
            deaths::start(&app);
            update::start(&app);
            {
                let app = app.clone();
                let handle = handle.clone();
                std::thread::spawn(move || {
                    if let Some(why) = elevation::ensure_quit_task() {
                        app.toast(app::Toast { id: "unsafe-install".into(), kind: "warn".into(), title: "Deck's admin start was turned off".into(), body: why, actions: vec![("ok".into(), "OK".into())], sticky: true, sound: false });
                    }
                    // with the admin tasks in place Deck can come back by itself: the watchdog starts it
                    // again after it was ended from outside (it also hands the hardware back)
                    if elevation::is_admin() && elevation::task_exists(elevation::BACKGROUND_TASK) {
                        watchdog::start();
                        // an NVIDIA card Deck talks to is what a driver install can hurt
                        if gpu::nvml::Nvml::shared().is_some() {
                            driver::start_watch(move || restart_deck(&handle));
                        }
                    }
                });
            }
            handle_args(&handle, &std::env::args().collect::<Vec<_>>());
            // a Deck that ended while a phone played or talked: the PC's sound and mic as they were
            std::thread::spawn(phonemic::restore_after_crash);
            // Tweaks Windows changed back (feature updates): told once
            tweaks::start_watch(app.clone());
            if app.settings.lock().remote_on {
                remote::start(app.clone(), remote_state.clone());
            }
            // one-time share links still open from before
            {
                let app = app.clone();
                std::thread::spawn(move || share::init(app));
            }
            // your other PCs: Transfer (on by default) and Claude Sync (off by default)
            {
                let app = app.clone();
                std::thread::spawn(move || {
                    let (t, c) = {
                        let s = app.settings.lock();
                        (s.transfer_on, s.claude_sync_on)
                    };
                    beam::set_on(&app, t);
                    if c {
                        if let Err(e) = ccsync::set_on(&app, true) {
                            app.toast(app::Toast::simple("claude-sync", "Workspace Sync didn't start", &e));
                        }
                    } else {
                        ccsync::cleanup_when_off();
                    }
                });
            }
            let autostart = app.settings.lock().autostart;
            sync_autostart(&handle, autostart);

            if let Some(w) = tapp.get_webview_window("main") {
                let h = handle.clone();
                w.on_window_event(move |e| match e {
                    WindowEvent::CloseRequested { api, .. } => {
                        api.prevent_close();
                        hide_deck(&h);
                    }
                    WindowEvent::Moved(_) | WindowEvent::Resized(_) if app_mode(&h) && h.state::<Arc<App>>().is_visible() => {
                        if let Some(w) = h.get_webview_window("main") {
                            remember_rect(&h, &w);
                        }
                    }
                    WindowEvent::Focused(false) => {
                        let app = h.state::<Arc<App>>();
                        if app.is_visible() && { let s = app.settings.lock(); s.mode != "app" && s.hide_on_blur && (s.hide_after_launch || !launch_hold()) } && !DIALOG.load(Ordering::Relaxed) && !PINNED.load(Ordering::Relaxed) {
                            hide_deck(&h);
                        }
                    }
                    _ => {}
                });
                if !minimized {
                    show_deck(&handle, None);
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            webview_guard::ui_heartbeat, get_state, live, hide, set_pinned, quit, ui_log, set_setting, padcmd::pad_state, padcmd::pad_action, deathscmd::deaths_state, deathscmd::deaths_act,
            deathscmd::deaths_strip,
            deathscmd::deaths_teach,
            deathscmd::deaths_bar,
            deathscmd::deaths_note,
            deathscmd::deaths_reset,
            deathscmd::deaths_preview,
            deathscmd::deaths_share,
            deathscmd::deaths_export,
            updatecmd::update_state,
            programscmd::programs_list,
            programscmd::programs_do,
            updatecmd::update_check,
            updatecmd::update_install,
            search, apps_list, launch_app, open_path, reveal, open_url, open_system, games_list,
            fonts_list, audiocmd::audio_state, audiocmd::set_output, audiocmd::set_master, audiocmd::set_app_volume, audiocmd::media_control,
            clip_list, clip_copy, set_clipboard, clip_pin, clip_remove, clip_clear,
            set_awake, set_plan, cancel_plan, run_power, timer_add, timer_cmd, stopwatch, toast_action, toast_resize,
            windows_list, window_action, snap_window, ws_capture, ws_recapture, ws_open,
            diskcmd::disk_scan, diskcmd::disk_status, diskcmd::disk_cancel, diskcmd::disk_treemap, diskcmd::disk_types, diskcmd::disk_path, syscmd::support_report, diskcmd::disk_children, diskcmd::disk_big, diskcmd::recycle, diskcmd::cleanup_scan, diskcmd::cleanup_state, diskcmd::cleanup_run,
            syscmd::processes, syscmd::filecheck_state, syscmd::filecheck_start, syscmd::filecheck_stop, syscmd::filecheck_once_start, syscmd::filecheck_once_state, syscmd::filecheck_once_stop, syscmd::filecheck_once_forget, syscmd::filecheck_key, syscmd::kill, syscmd::kill_pid, syscmd::startup_list, syscmd::startup_set, syscmd::who_uses, pick,
            displaycmd::display_state, displaycmd::set_refresh, displaycmd::set_brightness, displaycmd::set_hdr, displaycmd::set_dark, displaycmd::locate,
            netcmd::net_info, netcmd::net_public_ip, netcmd::net_ping, netcmd::net_speed, netcmd::net_speed_cancel, netcmd::game_ping_games, netcmd::game_ping, index_rebuild, index_info,
            boostcmd::boost_state, boostcmd::boost_apps, boostcmd::boost_run, boostcmd::boost_pick,
            screen_tool, phonecmd::remote_info, phonecmd::phone_keys_state, phonecmd::phone_keys_do, phonecmd::remote_reset_token, phonecmd::rdesk_status, phonecmd::phone_mic_state, phonecmd::phone_mic_do, tweakscmd::tweaks_state, tweakscmd::tweaks_set, tweakscmd::tweaks_do, tweakscmd::tweaks_apps, tweakscmd::tweaks_app, tweakscmd::tweaks_store, phonecmd::rdesk_do, phonecmd::transfer_state, phonecmd::transfer_do, pick_many, phonecmd::share_state, phonecmd::share_create, phonecmd::share_stop, phonecmd::transfer_apps, phonecmd::transfer_setup_preview, phonecmd::claude_sync_state, phonecmd::claude_sync_do, share_to_phone, phonecmd::shared_remove,
            elevate_setup,
            hwcmd::hw_state, hwcmd::drive_health, hwcmd::hw_live, hwcmd::hw_hist, hwcmd::hw_table, hwcmd::hw_table_reset, hwcmd::hw_apply, hwcmd::hw_lights_off,
            hwcmd::hw_display_off, hwcmd::hw_report, hwcmd::hw_test_lights, hwcmd::hw_clear_ram, hwcmd::hw_mem, hwcmd::hw_import_moboctl, hwcmd::hw_open_folder,
            hwcmd::hw_lcd_preview, hwcmd::hw_lcd_grab,
            profcmd::cpu_state, profcmd::cpu_live, profcmd::cpu_plans, profcmd::cpu_adopt, profcmd::cpu_create, profcmd::cpu_save,
            profcmd::cpu_delete, profcmd::cpu_duplicate, profcmd::profile_switch,
            profcmd::gpu_state, profcmd::gpu_live, profcmd::gpu_curve, profcmd::gpu_save, profcmd::gpu_delete, profcmd::gpu_try,
            profcmd::gpu_stock, profcmd::ab_preview, profcmd::ab_import, profcmd::ab_stop_tuning, profcmd::ab_undo_stop, profcmd::gpu_apply_held,
            profcmd::icons_list, profcmd::icon_custom, profcmd::icon_preview, profcmd::shortcuts_repair, profcmd::hotkey_check,
            profcmd::rtss_state, profcmd::rtss_start, profcmd::rtss_open, profcmd::rtss_tray, profcmd::rtss_install, profcmd::rtss_reset_stats, profcmd::rtss_restore_look, profcmd::rtss_limiter,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Deck");

    tauri_app.run(|_h, e| {
        if let tauri::RunEvent::Exit = e {
            shutdown_hardware(false);
        }
    });
}

#[cfg(test)]
mod asset_tests {
    /// The window pages must be compiled into the exe.
    #[test]
    fn pages_are_embedded() {
        let ctx: tauri::Context<tauri::Wry> = tauri::generate_context!();
        for p in ["index.html", "toast.html", "zones.html", "js/boost.js", "js/curve.js", "js/hw.js", "js/profiles.js", "js/help.js", "js/layout.js", "app.css"] {
            let a = ctx.assets().get(&p.into());
            println!("{p}: {:?} bytes", a.as_ref().map(|b| b.len()));
            assert!(a.is_some(), "{p} missing");
        }
    }
}
