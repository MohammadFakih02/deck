//! Keeps the WebView2 window from going blank (white or black).
//!
//! Known causes this works around: WebView2 stops rendering windows it thinks are hidden/covered, a window
//! created hidden paints nothing when first shown, and the page sometimes just stops drawing with no crash.
//!  - `browser_args` turns off the renderer backgrounding / occlusion tracking for every window of the app.
//!  - `repaint_nudge` resizes a freshly shown window by one pixel and back, which forces a layout + paint.
//!  - the watchdog listens for the page's own "I'm painting" beats (ui/guard.js); a visible window that stays
//!    silent gets nudged, and one that stays silent while focused for a long time gets reloaded.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager, WebviewWindow};

/// Same list for every window, set before the first one is made (windows with different flags can't share a profile).
/// The disable-features list keeps wry's own defaults (msWebOOUI, msPdfOOUI, msSmartScreenProtection).
const BROWSER_ARGS: &str = "--disable-features=CalculateNativeWinOcclusion,msWebOOUI,msPdfOOUI,msSmartScreenProtection --disable-background-timer-throttling --disable-backgrounding-occluded-windows --disable-renderer-backgrounding";

static START: OnceLock<Instant> = OnceLock::new();
static LAST_BEAT: AtomicU64 = AtomicU64::new(0);
/// Frames the page has reported (only the page's own beats count, not `touch`).
static BEATS: AtomicU64 = AtomicU64::new(0);

fn now_ms() -> u64 {
    START.get_or_init(Instant::now).elapsed().as_millis() as u64
}

/// The browser flags every Deck window is built with. wry passes its own browser arguments, which win
/// over the WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS variable, so the flags go on each window builder;
/// anything that variable holds (a debugging port, say) is added on top.
pub fn browser_args() -> String {
    let args: Vec<String> = std::env::args().collect();
    browser_args_for(std::env::var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS").ok().as_deref(), crate::elevation::is_admin(), cdp_port(&args))
}

/// `browser_args` without the process around it (for tests).
/// An elevated Deck takes nothing from the environment: any program running as the user can set
/// that variable, and a debugging port there would hand it every command Deck has. The port for
/// live testing comes from Deck's own command line instead (`--cdp-port`), which only whoever
/// started this process could pass.
pub fn browser_args_for(env_extra: Option<&str>, elevated: bool, cli_port: Option<u16>) -> String {
    let mut a = BROWSER_ARGS.to_string();
    if let Some(v) = env_extra.map(str::trim).filter(|v| !v.is_empty() && !elevated) {
        a.push(' ');
        a.push_str(v);
    }
    if let Some(p) = cli_port {
        a.push_str(&format!(" --remote-debugging-port={p}"));
    }
    a
}

/// `--cdp-port=N` (or `--cdp-port N`) on Deck's own command line.
pub fn cdp_port(args: &[String]) -> Option<u16> {
    let i = args.iter().position(|a| a == "--cdp-port" || a.starts_with("--cdp-port="))?;
    let v = match args[i].strip_prefix("--cdp-port=") {
        Some(v) => v,
        None => args.get(i + 1)?,
    };
    v.parse::<u16>().ok().filter(|p| *p != 0)
}

/// Removes the WEBVIEW2_* variables from this process when it runs elevated, before any window
/// exists: WebView2 reads several of them itself (extra browser flags, which browser to run, where
/// its profile lives), and the user can set all of them without admin rights.
pub fn scrub_env(elevated: bool) {
    if !elevated {
        return;
    }
    let names: Vec<std::ffi::OsString> = std::env::vars_os().map(|(k, _)| k).filter(|k| k.to_string_lossy().to_ascii_uppercase().starts_with("WEBVIEW2_")).collect();
    for k in names {
        std::env::remove_var(k);
    }
}

/// The page reports that it is drawing frames.
#[tauri::command]
pub fn ui_heartbeat() {
    BEATS.fetch_add(1, Ordering::Relaxed);
    touch();
}

/// Restart the "has it gone quiet?" clock (call when the window is shown).
pub fn touch() {
    LAST_BEAT.store(now_ms(), Ordering::Relaxed);
}

/// A freshly shown window: nudged only if the page hasn't reported a drawn frame shortly after
/// (it reports its first frame on every show). A resize repaints the whole window, which on a
/// full-screen panel wakes the graphics card for a moment, so it is kept for windows that need it.
pub fn nudge_if_blank(w: &WebviewWindow) {
    touch();
    let before = BEATS.load(Ordering::Relaxed);
    let w = w.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(500));
        if BEATS.load(Ordering::Relaxed) == before {
            repaint_nudge(&w);
        }
    });
}

/// One-pixel resize and back, shortly after the window is shown.
pub fn repaint_nudge(w: &WebviewWindow) {
    touch();
    let w = w.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(350));
        if w.is_maximized().unwrap_or(false) || w.is_fullscreen().unwrap_or(false) {
            return;
        }
        if let Ok(sz) = w.inner_size() {
            let _ = w.set_size(tauri::PhysicalSize::new(sz.width + 1, sz.height));
            std::thread::sleep(Duration::from_millis(40));
            let _ = w.set_size(sz);
        }
    });
}

pub fn reload(w: &WebviewWindow) {
    let _ = w.eval("location.reload()");
    touch();
}

/// Watches the window labelled `label`.
pub fn spawn_watchdog(app: AppHandle, label: &'static str) {
    touch();
    std::thread::spawn(move || {
        let mut nudged_at = 0u64;
        loop {
            std::thread::sleep(Duration::from_secs(5));
            let Some(w) = app.get_webview_window(label) else { continue };
            let showing = w.is_visible().unwrap_or(false) && !w.is_minimized().unwrap_or(false);
            if !showing {
                touch(); // a hidden window owes us nothing; the clock starts when it's shown again
                continue;
            }
            let now = now_ms();
            let quiet = now.saturating_sub(LAST_BEAT.load(Ordering::Relaxed));
            // Focus matters for the reload: a locked screen or a sleeping display also silences the page.
            if quiet > 90_000 && w.is_focused().unwrap_or(false) {
                reload(&w);
            } else if quiet > 15_000 && now.saturating_sub(nudged_at) > 15_000 {
                nudged_at = now;
                repaint_nudge(&w);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_elevated_deck_ignores_debug_flags_from_the_environment() {
        let a = browser_args_for(Some("--remote-debugging-port=9222 --load-extension=C:/x"), true, None);
        assert!(!a.contains("remote-debugging") && !a.contains("load-extension"), "{a}");
        assert!(a.starts_with(BROWSER_ARGS));
    }

    #[test]
    fn an_unelevated_deck_still_takes_them() {
        assert!(browser_args_for(Some("--remote-debugging-port=9222"), false, None).contains("--remote-debugging-port=9222"));
    }

    #[test]
    fn the_debug_port_comes_from_decks_own_command_line() {
        let args = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(cdp_port(&args(&["deck.exe", "--cdp-port=9222"])), Some(9222));
        assert_eq!(cdp_port(&args(&["deck.exe", "--minimized", "--cdp-port", "9333"])), Some(9333));
        assert_eq!(cdp_port(&args(&["deck.exe", "--cdp-port=0"])), None);
        assert_eq!(cdp_port(&args(&["deck.exe", "--cdp-port=x"])), None);
        assert_eq!(cdp_port(&args(&["deck.exe"])), None);
        assert!(browser_args_for(None, true, Some(9222)).contains("--remote-debugging-port=9222"));
    }

    #[test]
    fn elevated_start_drops_every_webview2_variable() {
        std::env::set_var("WEBVIEW2_DECK_TEST_SCRUB", "1");
        scrub_env(false);
        assert!(std::env::var("WEBVIEW2_DECK_TEST_SCRUB").is_ok(), "unelevated: left alone");
        scrub_env(true);
        assert!(std::env::var("WEBVIEW2_DECK_TEST_SCRUB").is_err(), "elevated: removed");
    }
}
