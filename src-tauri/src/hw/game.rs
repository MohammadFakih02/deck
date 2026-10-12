//! Game lighting helpers: finds the full-screen game in front, and measures how loud that one program is.
use windows::core::Interface;
use windows::Win32::Media::Audio::Endpoints::IAudioMeterInformation;
use windows::Win32::Media::Audio::*;
use windows::Win32::System::Com::*;
use windows_sys::Win32::Foundation::{CloseHandle, RECT};
use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST};
use windows_sys::Win32::System::Threading::{GetCurrentProcessId, OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION};
use windows_sys::Win32::UI::WindowsAndMessaging::{GetClassNameW, GetForegroundWindow, GetWindowLongW, GetWindowRect, GetWindowThreadProcessId, GWL_STYLE, WS_CAPTION};

/// A window that covers its whole monitor without a title bar: a fullscreen or borderless game
/// or video. A maximized app also covers the monitor when the taskbar auto-hides, but it keeps
/// its title bar. Rects are (left, top, right, bottom).
pub fn covers_like_fullscreen(win: (i32, i32, i32, i32), mon: (i32, i32, i32, i32), style: u32) -> bool {
    let covers = win.0 <= mon.0 && win.1 <= mon.1 && win.2 >= mon.2 && win.3 >= mon.3;
    covers && style & WS_CAPTION != WS_CAPTION
}

fn exe_name(pid: u32) -> String {
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return String::new();
        }
        let mut buf = [0u16; 520];
        let mut n = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut n) != 0;
        CloseHandle(h);
        if !ok {
            return String::new();
        }
        let p = String::from_utf16_lossy(&buf[..n as usize]);
        p.rsplit('\\').next().unwrap_or("").to_ascii_lowercase()
    }
}

/// Process id of the program covering a whole monitor (a game, or a full-screen video), if there is one.
pub fn foreground_game() -> Option<u32> {
    unsafe {
        let w = GetForegroundWindow();
        if w.is_null() {
            return None;
        }
        let mut cls = [0u16; 64];
        let n = GetClassNameW(w, cls.as_mut_ptr(), 64).max(0) as usize;
        let cls = String::from_utf16_lossy(&cls[..n]);
        if matches!(cls.as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd") {
            return None;
        }
        let mut r: RECT = std::mem::zeroed();
        if GetWindowRect(w, &mut r) == 0 {
            return None;
        }
        let mon = MonitorFromWindow(w, MONITOR_DEFAULTTONEAREST);
        let mut mi: MONITORINFO = std::mem::zeroed();
        mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(mon, &mut mi) == 0 {
            return None;
        }
        let m = mi.rcMonitor;
        let style = GetWindowLongW(w, GWL_STYLE) as u32;
        if !covers_like_fullscreen((r.left, r.top, r.right, r.bottom), (m.left, m.top, m.right, m.bottom), style) {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(w, &mut pid);
        if pid == 0 || pid == GetCurrentProcessId() {
            return None;
        }
        let exe = exe_name(pid);
        if matches!(exe.as_str(), "explorer.exe" | "searchhost.exe" | "startmenuexperiencehost.exe" | "lockapp.exe" | "") {
            return None;
        }
        Some(pid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_maximized_app_is_not_a_game() {
        let mon = (0, 0, 1920, 1080);
        let borderless = 0x1000_0000 | 0x8000_0000; // WS_VISIBLE | WS_POPUP
        let maximized = 0x1000_0000 | 0x00CF_0000 | 0x0100_0000; // WS_VISIBLE | WS_OVERLAPPEDWINDOW | WS_MAXIMIZE
        assert!(covers_like_fullscreen((0, 0, 1920, 1080), mon, borderless), "borderless game");
        // with an auto-hiding taskbar a maximized window reaches past every edge
        assert!(!covers_like_fullscreen((-8, -8, 1928, 1088), mon, maximized), "maximized browser");
        assert!(!covers_like_fullscreen((0, 0, 1280, 720), mon, borderless), "windowed");
        assert!(!covers_like_fullscreen((1920, 0, 3840, 1080), mon, borderless), "on the other monitor");
    }
}

/// Per-program loudness through the Windows audio sessions.
pub struct GameMeter {
    mgr: IAudioSessionManager2,
    dev_meter: IAudioMeterInformation,
    sessions: Vec<(u32, IAudioMeterInformation)>,
    refreshed: std::time::Instant,
}

impl GameMeter {
    pub fn open() -> Option<GameMeter> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
            let dev = en.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
            let mgr: IAudioSessionManager2 = dev.Activate(CLSCTX_ALL, None).ok()?;
            let dev_meter: IAudioMeterInformation = dev.Activate(CLSCTX_ALL, None).ok()?;
            let mut m = GameMeter { mgr, dev_meter, sessions: Vec::new(), refreshed: std::time::Instant::now() };
            m.refresh();
            Some(m)
        }
    }

    fn refresh(&mut self) {
        self.refreshed = std::time::Instant::now();
        self.sessions.clear();
        unsafe {
            let Ok(en) = self.mgr.GetSessionEnumerator() else { return };
            let n = en.GetCount().unwrap_or(0);
            for i in 0..n {
                let Ok(ctl) = en.GetSession(i) else { continue };
                let Ok(c2) = ctl.cast::<IAudioSessionControl2>() else { continue };
                let Ok(pid) = c2.GetProcessId() else { continue };
                if let Ok(m) = ctl.cast::<IAudioMeterInformation>() {
                    self.sessions.push((pid, m));
                }
            }
        }
    }

    /// Loudness 0..1 of the program `pid`; with `whole_device` the speaker output as a whole is used when it has no session.
    pub fn peak(&mut self, pid: Option<u32>, whole_device: bool) -> f32 {
        if self.refreshed.elapsed().as_millis() > 1500 {
            self.refresh();
        }
        unsafe {
            if let Some(pid) = pid {
                let mut best = 0f32;
                let mut found = false;
                for (p, m) in &self.sessions {
                    if *p == pid {
                        found = true;
                        best = best.max(m.GetPeakValue().unwrap_or(0.0));
                    }
                }
                if found {
                    return best;
                }
            }
            if whole_device {
                return self.dev_meter.GetPeakValue().unwrap_or(0.0);
            }
        }
        0.0
    }
}

/// Spots sudden loud moments (explosions, gunshots) in the game's sound.
#[derive(Default)]
pub struct Impact {
    avg: f32,
    pub impact: f32,
    last: Option<std::time::Instant>,
}

impl Impact {
    /// Feed the game loudness every ~15 ms; returns (impact 1..0, seconds since the last impact).
    pub fn update(&mut self, level: f32) -> (f32, f32) {
        let now = std::time::Instant::now();
        let spike = level > 0.3 && level - self.avg > 0.25 && self.last.map_or(true, |t| t.elapsed().as_millis() > 150);
        if spike {
            self.impact = 1.0;
            self.last = Some(now);
        } else {
            self.impact *= 0.9;
        }
        self.avg = self.avg * 0.97 + level * 0.03;
        (self.impact, self.last.map_or(9.0, |t| t.elapsed().as_secs_f32()))
    }
}
