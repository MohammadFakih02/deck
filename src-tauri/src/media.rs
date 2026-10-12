//! "Now playing" for whatever app is playing media (Spotify, browsers,
//! YouTube Music, ...), through Windows' own media controls (SMTC).

use crate::util::com;
use parking_lot::Mutex;
use serde_json::{json, Value};
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as Session, GlobalSystemMediaTransportControlsSessionManager as Manager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
};
use windows::Storage::Streams::DataReader;

pub struct Media {
    mgr: Mutex<Option<Manager>>,
    /// (key, data URL) of the last artwork, fetched once per track.
    art: Mutex<(String, Option<String>)>,
}

fn status_name(s: Status) -> &'static str {
    match s {
        Status::Playing => "playing",
        Status::Paused => "paused",
        Status::Stopped => "stopped",
        Status::Changing => "changing",
        Status::Opened => "opened",
        _ => "closed",
    }
}

/// "Spotify.exe" -> "Spotify", "chrome.exe" -> "Chrome", store app ids -> app name.
pub fn app_label(aumid: &str) -> String {
    let lower = aumid.to_ascii_lowercase();
    let known = [
        ("msedgewebview2", "Web app"),
        ("spotify", "Spotify"),
        ("chrome", "Chrome"),
        ("msedge", "Edge"),
        ("firefox", "Firefox"),
        ("brave", "Brave"),
        ("opera", "Opera"),
        ("zunemusic", "Media Player"),
        ("vlc", "VLC"),
        ("discord", "Discord"),
        ("youtube", "YouTube Music"),
    ];
    for (k, v) in known {
        if lower.contains(k) {
            return v.to_string();
        }
    }
    let base = aumid.rsplit(['\\', '!']).next().unwrap_or(aumid);
    base.trim_end_matches(".exe").trim_end_matches(".EXE").to_string()
}

/// 100 ns ticks -> seconds.
fn secs(ticks: i64) -> f64 {
    ticks as f64 / 1e7
}

/// Windows FILETIME-style time (100 ns since 1601) for "now".
fn now_ticks() -> i64 {
    let unix_100ns = (crate::util::now_ms() as i64) * 10_000;
    unix_100ns + 116_444_736_000_000_000
}

impl Media {
    pub fn new() -> Self {
        Self { mgr: Mutex::new(None), art: Mutex::new((String::new(), None)) }
    }

    fn manager(&self) -> Option<Manager> {
        let mut g = self.mgr.lock();
        if g.is_none() {
            com();
            *g = Manager::RequestAsync().ok().and_then(|op| op.join().ok());
        }
        g.clone()
    }

    fn sessions(&self) -> Vec<Session> {
        let Some(m) = self.manager() else { return Vec::new() };
        let mut out = Vec::new();
        let current = m.GetCurrentSession().ok();
        if let Some(c) = &current {
            out.push(c.clone());
        }
        if let Ok(all) = m.GetSessions() {
            for s in all {
                let same = current.as_ref().map(|c| c.SourceAppUserModelId().ok() == s.SourceAppUserModelId().ok()).unwrap_or(false);
                if !same {
                    out.push(s);
                }
            }
        }
        out
    }

    fn describe(&self, s: &Session, with_art: bool) -> Value {
        let aumid = s.SourceAppUserModelId().map(|h| h.to_string()).unwrap_or_default();
        let props = s.TryGetMediaPropertiesAsync().ok().and_then(|op| op.join().ok());
        let title = props.as_ref().and_then(|p| p.Title().ok()).map(|h| h.to_string()).unwrap_or_default();
        let artist = props.as_ref().and_then(|p| p.Artist().ok()).map(|h| h.to_string()).unwrap_or_default();
        let album = props.as_ref().and_then(|p| p.AlbumTitle().ok()).map(|h| h.to_string()).unwrap_or_default();
        let info = s.GetPlaybackInfo().ok();
        let status = info.as_ref().and_then(|i| i.PlaybackStatus().ok()).unwrap_or(Status::Closed);
        let controls = info.as_ref().and_then(|i| i.Controls().ok());
        let can = |f: &dyn Fn(&windows::Media::Control::GlobalSystemMediaTransportControlsSessionPlaybackControls) -> windows::core::Result<bool>| {
            controls.as_ref().and_then(|c| f(c).ok()).unwrap_or(false)
        };
        let (pos, len) = match s.GetTimelineProperties() {
            Ok(t) => {
                let end = t.EndTime().map(|d| d.Duration).unwrap_or(0);
                let start = t.StartTime().map(|d| d.Duration).unwrap_or(0);
                let mut p = t.Position().map(|d| d.Duration).unwrap_or(0);
                if status == Status::Playing {
                    if let Ok(upd) = t.LastUpdatedTime() {
                        let drift = now_ticks() - upd.UniversalTime;
                        if drift > 0 && drift < 36_000_000_000 {
                            p += drift;
                        }
                    }
                }
                let len = (end - start).max(0);
                (secs(p.clamp(0, len.max(p))), secs(len))
            }
            Err(_) => (0.0, 0.0),
        };
        let art = if with_art && !title.is_empty() {
            let key = format!("{aumid}|{title}|{artist}");
            let mut a = self.art.lock();
            if a.0 != key {
                *a = (key, props.as_ref().and_then(read_thumbnail));
            }
            a.1.clone()
        } else {
            None
        };
        json!({
            "id": aumid,
            "app": app_label(&aumid),
            "title": title,
            "artist": artist,
            "album": album,
            "status": status_name(status),
            "pos": pos,
            "len": len,
            "can_prev": can(&|c| c.IsPreviousEnabled()),
            "can_next": can(&|c| c.IsNextEnabled()),
            "can_toggle": can(&|c| c.IsPlayPauseToggleEnabled()),
            "can_seek": can(&|c| c.IsPlaybackPositionEnabled()),
            "art": art,
        })
    }

    /// The current session (with artwork) plus any others.
    pub fn state(&self) -> Value {
        let ss = self.sessions();
        let mut it = ss.iter();
        let current = it.next().map(|s| self.describe(s, true));
        let others: Vec<Value> = it.map(|s| self.describe(s, false)).collect();
        json!({ "current": current, "others": others })
    }

    pub fn is_playing(&self) -> bool {
        self.sessions().iter().any(|s| {
            s.GetPlaybackInfo().ok().and_then(|i| i.PlaybackStatus().ok()) == Some(Status::Playing)
        })
    }

    /// `action`: "toggle" | "next" | "prev" | "play" | "pause" | "seek".
    /// `id` picks a session (empty = current).
    pub fn control(&self, id: &str, action: &str, pos: Option<f64>) -> Result<(), String> {
        let ss = self.sessions();
        let s = if id.is_empty() {
            ss.first()
        } else {
            ss.iter().find(|s| s.SourceAppUserModelId().map(|h| h.to_string()).unwrap_or_default() == id)
        }
        .ok_or("Nothing is playing")?;
        let r = match action {
            "toggle" => s.TryTogglePlayPauseAsync(),
            "next" => s.TrySkipNextAsync(),
            "prev" => s.TrySkipPreviousAsync(),
            "play" => s.TryPlayAsync(),
            "pause" => s.TryPauseAsync(),
            "seek" => s.TryChangePlaybackPositionAsync((pos.unwrap_or(0.0).max(0.0) * 1e7) as i64),
            _ => return Err("unknown media action".into()),
        };
        r.and_then(|op| op.join()).map(|_| ()).map_err(|e| e.to_string())
    }
}

fn read_thumbnail(p: &windows::Media::Control::GlobalSystemMediaTransportControlsSessionMediaProperties) -> Option<String> {
    let r = p.Thumbnail().ok()?;
    let stream = r.OpenReadAsync().ok()?.join().ok()?;
    let size = stream.Size().ok()? as u32;
    if size == 0 || size > 4_000_000 {
        return None;
    }
    let ct = stream.ContentType().map(|h| h.to_string()).unwrap_or_else(|_| "image/png".into());
    let reader = DataReader::CreateDataReader(&stream).ok()?;
    reader.LoadAsync(size).ok()?.join().ok()?;
    let mut buf = vec![0u8; size as usize];
    reader.ReadBytes(&mut buf).ok()?;
    let ct = if ct.starts_with("image/") { ct } else { "image/png".into() };
    Some(format!("data:{ct};base64,{}", crate::util::base64(&buf)))
}
