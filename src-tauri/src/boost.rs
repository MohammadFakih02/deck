//! Game boost. "Full power" frees the PC for a game (closes the apps the user
//! picked, switches to the fastest power plan, lowers background apps'
//! priority, trims their memory) and puts everything back when the game
//! closes. "Clean up" closes the picked apps and frees memory, once.
//!
//! Works on any PC without administrator rights. Games, launchers,
//! anti-cheat, voice chat, hardware tools and Windows itself are never
//! touched: Deck doesn't even open a handle to them.

use crate::app::{App, Toast};
use crate::util::{now_s, process_path};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
use windows::core::{GUID, HSTRING, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM, WPARAM};
use windows::Win32::System::Threading::{
    GetCurrentProcess, GetPriorityClass, OpenProcess, SetPriorityClass, BELOW_NORMAL_PRIORITY_CLASS, NORMAL_PRIORITY_CLASS,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_INFORMATION, PROCESS_SET_QUOTA,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindow, GetWindowLongW, GetWindowTextLengthW, GetWindowThreadProcessId, IsWindowVisible, PostMessageW,
    GWL_EXSTYLE, GW_OWNER, WM_CLOSE, WS_EX_TOOLWINDOW,
};

static ON: AtomicBool = AtomicBool::new(false);
/// Full power with "turn off by itself" ends after this long (s) when no game ever showed up.
const NO_GAME_LIMIT: u64 = 3 * 3600;

/// Is Full power on? (Deck pauses its own background jobs meanwhile.)
pub fn is_on() -> bool {
    ON.load(Ordering::Relaxed)
}

// ------------------------------------------------------------ name lists
// Lower-case executable names without ".exe".

/// Never listed, never touched.
const NEVER: &[&str] = &[
    // Deck and MicDrop
    "deck", "micdrop",
    // launchers and their helpers (games need them running)
    "steam", "steamwebhelper", "steamservice", "gameoverlayui", "gameoverlayui64", "epicgameslauncher", "epicwebhelper",
    "epiconlineservices", "epiconlineserviceshost", "epiconlineservicesuserhelper", "riotclientservices", "riotclientux",
    "riotclientuxrender", "riotclientcrashhandler", "leagueclient", "leagueclientux", "leagueclientuxrender",
    "leaguecrashhandler", "leaguecrashhandler64", "battle.net", "blizzard battle.net", "agent", "eadesktop",
    "eabackgroundservice", "ealocalhostsvc", "eaconnect_microsoft", "origin", "originwebhelperservice", "upc", "ubisoftconnect",
    "uplaywebcore", "ubisoftgamelauncher", "upc_crashhandler", "galaxyclient", "galaxyclient helper", "goggalaxy",
    "gogalaxynotifications", "xboxpcapp", "xboxpcappft", "xboxpcapptray", "gamingservices", "gamingservicesnet", "xboxappservices",
    "gamebar", "gamebarftserver", "gamebarpresencewriter", "playnite.desktopapp", "playnite.fullscreenapp", "heroic",
    "amazon games ui", "amazongamesui", "rockstarservice", "socialclubhelper", "launcherpatcher", "rockstar games launcher",
    "bethesdanetlauncher", "paradox launcher", "wargaming.net game center", "wgc", "battlestate games launcher", "curseforge",
    "overwolf", "overwolfbrowser", "overwolfhelper", "overwolfhelper64", "minecraftlauncher", "minecraft",
    // anti-cheat
    "vgc", "vgtray", "vgk", "vanguard", "easyanticheat", "easyanticheat_eos", "easyanticheat_eos_setup", "beservice",
    "beservice_x64", "battleye", "faceit", "faceitclient", "faceitservice", "faceit ac", "esportal", "eseaclient", "esea",
    "gameguard", "gamemon", "gamemon64", "xigncode", "x3", "sguard64", "ace-tray", "anticheatexpert", "mhyprot", "ricochet",
    // hardware, drivers, overlays, recording
    "lghub", "lghub_agent", "lghub_updater", "lghub_system_tray", "logioptionsplus", "logioptionsplus_agent", "logitechg",
    "razer synapse", "razer synapse 3", "razer synapse service process", "razercentral", "razercentralservice", "rzsdkservice",
    "rzchromastreamserver", "razerappengine", "icue", "corsair.service", "steelseriesgg", "steelseriesggclient",
    "steelseriesengine", "steelseriessonar", "armourycrate.service", "armourycrate.usersessionhelper", "armourysocketserver",
    "asus_framework", "msiafterburner", "rtss", "rtsshooksloader", "rtsshooksloader64", "encoderserver", "encoderserver64",
    "hwinfo64", "hwinfo32", "nvcontainer", "nvidia overlay", "nvidia share", "nvidia app", "nvidia broadcast",
    "nvidia broadcast ui", "nvsphelper64", "nvdisplay.container", "nvtelemetrycontainer", "radeonsoftware", "amdrsserv",
    "amdrssrcext", "amdow", "atieclxx", "amddvr", "igcc", "igfxem", "rtkauduservice64", "realtekaudioconsole", "nahimic3",
    "nahimicsvc64", "voicemeeter", "voicemeeterpro", "voicemeeter8", "voicemeeter8x64", "voicemeeterpro_x64",
    "voicemeeter_x64", "wavelink", "elgato wave link", "streamdeck", "camerahub", "fancontrol", "signalrgb", "openrgb",
    "wallpaper32", "wallpaper64", "obs64", "obs32", "obs-ffmpeg-mux", "streamlabs obs", "medal", "outplayed",
    "shadowplay", "powertoys", "autohotkey", "autohotkey64", "autohotkeyux", "autohotkeyu64",
    // security
    "msmpeng", "nissrv", "securityhealthsystray", "mbam", "mbamtray", "mbamservice", "avastui", "avgui", "bdagent",
    "ekrn", "egui", "avp", "avpui", "nortonui", "ns", "mcuicnt", "mcshield", "webroot", "sophosui", "bitdefender",
    // runtimes, terminals and shells (might be doing work the user wants)
    "msedgewebview2", "conhost", "cmd", "powershell", "pwsh", "windowsterminal", "openconsole", "wt", "bash", "wsl", "wslhost",
    "wslservice", "vmmem", "vmmemwsl", "node", "python", "pythonw", "py", "java", "javaw", "dotnet", "ruby", "git", "ssh",
    "claude", "code-tunnel", "docker", "com.docker.backend", "com.docker.build", "vpnkit", "virtualbox", "vmware",
    "vmware-tray", "vmware-authd", "qemu",
    // VPN and network tools (closing them drops the connection)
    "openvpn", "openvpn-gui", "wireguard", "nordvpn", "expressvpn", "protonvpn", "surfshark", "mullvad vpn", "cloudflare warp",
    "warp-svc", "tailscale", "tailscale-ipn", "zerotier", "hamachi-2-ui", "radmin vpn", "exitlag", "noping", "wtfast",
    "netlimiter",
];

/// Name fragments that also mean "never touch".
const NEVER_PARTS: &[&str] = &["anticheat", "anti-cheat", "battleye", "easyanticheat", "vanguard", "crashhandler", "crashpad"];

/// Folders whose programs are never touched (drivers and hardware tools).
const NEVER_DIRS: &[&str] =
    &["\\nvidia corporation\\", "\\amd\\", "\\ati technologies\\", "\\realtek\\", "\\intel\\", "\\windows defender\\", "\\common files\\"];

/// Shown, never picked for you, never slowed down: voice chat, music, calls.
const VOICE: &[&str] = &[
    "discord", "discordptb", "discordcanary", "vesktop", "teamspeak", "ts3client_win64", "ts3client_win32", "teamspeak 5",
    "mumble", "guilded", "element", "spotify", "youtube-music", "youtube music", "applemusic", "itunes", "tidal", "deezer",
    "amazon music", "foobar2000", "musicbee", "aimp", "winamp", "vlc", "zoom", "ms-teams", "teams", "skype", "slack",
    "whatsapp", "telegram", "signal", "messenger",
];

/// Picked for you (the user can untick them): sync, torrents, updaters and
/// helpers that do nothing for a game.
const SUGGEST: &[&str] = &[
    "onedrive", "dropbox", "googledrivefs", "google drive", "box", "boxui", "icloudservices", "icloudphotos", "iclouddrive",
    "megasync", "pcloud", "syncthing", "qbittorrent", "utorrent", "bittorrent", "transmission-qt", "deluge", "tixati", "vuze",
    "biglybt", "frostwire", "fdm", "idman", "jdownloader2", "adobearm", "acrotray", "adobe desktop service", "creative cloud",
    "creative cloud ui helper", "ccxprocess", "cclibrary", "coresync", "adobeipcbroker", "adobecollabsync", "adobe crash processor",
    "adobeupdateservice", "googleupdate", "googlecrashhandler", "googlecrashhandler64", "microsoftedgeupdate", "jusched",
    "jucheck", "phoneexperiencehost", "yourphone", "widgets", "widgetservice", "copilot", "cortana", "figma_agent",
    "grammarly.desktop", "ccleaner", "ccleaner64", "ccleanerbrowser", "epicgameslauncher_updater", "logitechupdater",
    "itunelper", "ituneshelper", "ipodservice", "cefsharp.browsersubprocess",
];

/// Picked for you only while they have no window open (running in the background).
const SUGGEST_IF_HIDDEN: &[&str] = &["msedge", "chrome", "brave", "firefox", "opera", "opera_gx", "vivaldi"];

/// Fullscreen apps that aren't games.
const NOT_GAMES: &[&str] = &[
    "msedge", "chrome", "brave", "firefox", "opera", "opera_gx", "vivaldi", "vlc", "mpc-hc64", "mpc-hc", "mpc-be64", "mpv",
    "potplayermini64", "potplayer", "microsoft.media.player", "video.ui", "netflix", "powerpnt", "applicationframehost",
    "explorer", "deck", "code", "photos", "microsoft.photos", "zoom", "ms-teams", "obs64",
];

fn stem_of(path_or_name: &str) -> String {
    let n = path_or_name.rsplit(['\\', '/']).next().unwrap_or(path_or_name).to_lowercase();
    n.strip_suffix(".exe").map(str::to_string).unwrap_or(n)
}

/// Console programs (compilers, scripts, servers) are someone's work in
/// progress, never clutter. Reads the subsystem from the exe's PE header.
fn is_console_exe(path: &str) -> bool {
    static CACHE: std::sync::OnceLock<Mutex<HashMap<String, bool>>> = std::sync::OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(v) = cache.lock().get(path) {
        return *v;
    }
    let v = (|| {
        use std::io::Read;
        let mut buf = vec![0u8; 4096];
        let n = std::fs::File::open(path).ok()?.read(&mut buf).ok()?;
        let pe = u32::from_le_bytes(buf.get(0x3c..0x40)?.try_into().ok()?) as usize;
        if buf.get(pe..pe + 4)? != b"PE\0\0" {
            return None;
        }
        let sub = pe + 24 + 68;
        (sub + 2 <= n).then(|| u16::from_le_bytes([buf[sub], buf[sub + 1]]) == 3)
    })()
    .unwrap_or(false);
    cache.lock().insert(path.to_string(), v);
    v
}

fn never(stem: &str, path_lower: &str) -> bool {
    NEVER.contains(&stem)
        || NEVER_PARTS.iter().any(|p| stem.contains(p))
        || NEVER_DIRS.iter().any(|d| path_lower.contains(d))
        || crate::sys::is_protected(&format!("{stem}.exe"))
}

// ------------------------------------------------------------ games

/// Folders games live in, from every launcher that says where it put them.
fn game_dirs() -> Vec<String> {
    let mut dirs: Vec<String> = crate::games::installed().into_iter().map(|g| g.dir).filter(|d| !d.is_empty()).collect();
    // Riot (League, Valorant, TFT): one settings file per game.
    if let Ok(rd) = std::fs::read_dir("C:\\ProgramData\\Riot Games\\Metadata") {
        for e in rd.flatten() {
            let Ok(inner) = std::fs::read_dir(e.path()) else { continue };
            for f in inner.flatten() {
                if !f.file_name().to_string_lossy().ends_with(".product_settings.yaml") {
                    continue;
                }
                let Ok(t) = std::fs::read_to_string(f.path()) else { continue };
                for line in t.lines() {
                    if let Some(p) = line.trim().strip_prefix("product_install_full_path:") {
                        dirs.push(p.trim().trim_matches('"').to_string());
                    }
                }
            }
        }
    }
    // GOG and Ubisoft keep their games in the registry.
    dirs.extend(hklm_subkey_values("SOFTWARE\\WOW6432Node\\GOG.com\\Games", "path"));
    dirs.extend(hklm_subkey_values("SOFTWARE\\WOW6432Node\\Ubisoft\\Launcher\\Installs", "InstallDir"));
    // Xbox app / Game Pass: X:\XboxGames on any drive.
    for d in b'C'..=b'Z' {
        let p = format!("{}:\\XboxGames", d as char);
        if Path::new(&p).is_dir() {
            dirs.push(p);
        }
    }
    let mut out: Vec<String> = dirs
        .into_iter()
        .map(|d| {
            let mut d = d.replace('/', "\\").to_lowercase();
            if !d.ends_with('\\') {
                d.push('\\');
            }
            d
        })
        .filter(|d| d.len() > 4) // never a whole drive
        .collect();
    out.sort();
    out.dedup();
    out
}

fn hklm_subkey_values(key: &str, value: &str) -> Vec<String> {
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegEnumKeyExW, RegGetValueW, RegOpenKeyExW, HKEY, HKEY_LOCAL_MACHINE, KEY_READ, RRF_RT_REG_SZ,
    };
    let mut out = Vec::new();
    unsafe {
        let mut k = HKEY::default();
        if RegOpenKeyExW(HKEY_LOCAL_MACHINE, &HSTRING::from(key), None, KEY_READ, &mut k) != ERROR_SUCCESS {
            return out;
        }
        for i in 0..500u32 {
            let mut name = [0u16; 256];
            let mut len = name.len() as u32;
            if RegEnumKeyExW(k, i, Some(PWSTR(name.as_mut_ptr())), &mut len, None, None, None, None) != ERROR_SUCCESS {
                break;
            }
            let sub = String::from_utf16_lossy(&name[..len as usize]);
            let mut buf = vec![0u16; 1024];
            let mut blen = (buf.len() * 2) as u32;
            if RegGetValueW(k, &HSTRING::from(sub), &HSTRING::from(value), RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr() as *mut _), Some(&mut blen))
                == ERROR_SUCCESS
            {
                let v = crate::util::from_wide(&buf);
                if !v.is_empty() {
                    out.push(v);
                }
            }
        }
        let _ = RegCloseKey(k);
    }
    out
}

fn in_game_dir(path_lower: &str, dirs: &[String]) -> bool {
    dirs.iter().any(|d| path_lower.starts_with(d.as_str()))
        || (path_lower.contains("\\riot games\\") && !path_lower.contains("\\riot client\\"))
}

// ------------------------------------------------------------ processes

fn session_of(pid: u32) -> Option<u32> {
    let mut s = 0u32;
    unsafe { windows::Win32::System::RemoteDesktop::ProcessIdToSessionId(pid, &mut s).ok().map(|_| s) }
}

fn scan(sys: &mut System, with_cmd: bool) {
    let mut kind = ProcessRefreshKind::nothing().with_memory().with_exe(UpdateKind::OnlyIfNotSet);
    if with_cmd {
        kind = kind.with_cmd(UpdateKind::OnlyIfNotSet);
    }
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, kind);
}

/// Visible, normal top-level windows by process id.
fn windows_by_pid() -> HashMap<u32, Vec<isize>> {
    unsafe extern "system" fn cb(h: HWND, l: LPARAM) -> windows::core::BOOL {
        let map = &mut *(l.0 as *mut HashMap<u32, Vec<isize>>);
        if IsWindowVisible(h).as_bool()
            && GetWindowTextLengthW(h) > 0
            && GetWindow(h, GW_OWNER).map(|o| o.0.is_null()).unwrap_or(true)
            && GetWindowLongW(h, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW.0 == 0
        {
            let mut pid = 0u32;
            GetWindowThreadProcessId(h, Some(&mut pid));
            map.entry(pid).or_default().push(h.0 as isize);
        }
        true.into()
    }
    let mut map: HashMap<u32, Vec<isize>> = HashMap::new();
    unsafe {
        let _ = EnumWindows(Some(cb), LPARAM(&mut map as *mut _ as isize));
    }
    map
}

/// One app (every process with the same executable name).
struct Group {
    key: String,
    name: String,
    exe: String,
    pids: Vec<u32>,
    mem: u64,
    window: bool,
    voice: bool,
    suggested: bool,
}

/// The user's apps that Deck may touch, biggest first. Everything from
/// NEVER, Windows, other users and games is left out.
fn groups(sys: &System, wins: &HashMap<u32, Vec<isize>>, dirs: &[String]) -> Vec<Group> {
    let me = std::process::id();
    let my_session = session_of(me);
    let windir = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into()).to_lowercase() + "\\";
    let mut map: HashMap<String, Group> = HashMap::new();
    for (pid, p) in sys.processes() {
        let pid = pid.as_u32();
        if pid == me || p.thread_kind().is_some() {
            continue;
        }
        let Some(exe) = p.exe().map(|e| e.to_string_lossy().to_string()) else { continue };
        let lower = exe.to_lowercase();
        let stem = stem_of(&exe);
        if stem.is_empty() || lower.starts_with(&windir) || never(&stem, &lower) || in_game_dir(&lower, dirs) || is_console_exe(&exe) {
            continue;
        }
        if my_session.is_none() || session_of(pid) != my_session {
            continue;
        }
        let g = map.entry(stem.clone()).or_insert_with(|| Group {
            key: stem.clone(),
            name: String::new(),
            exe: exe.clone(),
            pids: Vec::new(),
            mem: 0,
            window: false,
            // "discordsystemhelper" belongs to Discord too
            voice: VOICE.iter().any(|v| stem == *v || (v.len() >= 5 && stem.starts_with(v))),
            suggested: false,
        });
        g.pids.push(pid);
        g.mem += p.memory();
        g.window |= wins.contains_key(&pid);
    }
    let mut out: Vec<Group> = map.into_values().collect();
    for g in &mut out {
        g.name = friendly_name(&g.exe);
        g.suggested = SUGGEST.contains(&g.key.as_str()) || (!g.window && SUGGEST_IF_HIDDEN.contains(&g.key.as_str()));
    }
    out.sort_by(|a, b| b.mem.cmp(&a.mem));
    out
}

/// "Microsoft Edge" for msedge.exe (the file's description), else the name.
fn friendly_name(exe: &str) -> String {
    static CACHE: std::sync::OnceLock<Mutex<HashMap<String, String>>> = std::sync::OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(n) = cache.lock().get(exe) {
        return n.clone();
    }
    let n = file_description(exe).filter(|d| !d.trim().is_empty() && d.len() < 48).unwrap_or_else(|| crate::util::exe_stem(exe));
    cache.lock().insert(exe.to_string(), n.clone());
    n
}

fn file_description(exe: &str) -> Option<String> {
    use windows::Win32::Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW};
    unsafe {
        let path = HSTRING::from(exe);
        let size = GetFileVersionInfoSizeW(&path, None);
        if size == 0 {
            return None;
        }
        let mut data = vec![0u8; size as usize];
        GetFileVersionInfoW(&path, None, size, data.as_mut_ptr() as *mut _).ok()?;
        let mut p: *mut core::ffi::c_void = std::ptr::null_mut();
        let mut len = 0u32;
        let mut langs: Vec<(u16, u16)> = Vec::new();
        if VerQueryValueW(data.as_ptr() as *const _, &HSTRING::from("\\VarFileInfo\\Translation"), &mut p, &mut len).as_bool() && !p.is_null() {
            let n = len as usize / 4;
            let s = std::slice::from_raw_parts(p as *const u16, n * 2);
            langs = s.chunks(2).map(|c| (c[0], c[1])).collect();
        }
        langs.push((0x0409, 0x04b0));
        for (l, cp) in langs {
            let q = format!("\\StringFileInfo\\{l:04x}{cp:04x}\\FileDescription");
            let mut p: *mut core::ffi::c_void = std::ptr::null_mut();
            let mut len = 0u32;
            if VerQueryValueW(data.as_ptr() as *const _, &HSTRING::from(q), &mut p, &mut len).as_bool() && !p.is_null() && len > 1 {
                let s = std::slice::from_raw_parts(p as *const u16, len as usize);
                let t = crate::util::from_wide(s).trim().to_string();
                if !t.is_empty() {
                    return Some(t);
                }
            }
        }
        None
    }
}

fn open(pid: u32, rights: windows::Win32::System::Threading::PROCESS_ACCESS_RIGHTS) -> Option<HANDLE> {
    unsafe { OpenProcess(rights, false, pid).ok() }
}

/// Store apps are started by their app id, not their exe.
fn app_user_model_id(pid: u32) -> Option<String> {
    use windows::Win32::Storage::Packaging::Appx::GetApplicationUserModelId;
    let h = open(pid, PROCESS_QUERY_LIMITED_INFORMATION)?;
    let mut buf = [0u16; 512];
    let mut len = buf.len() as u32;
    let r = unsafe { GetApplicationUserModelId(h, &mut len, Some(PWSTR(buf.as_mut_ptr()))) };
    unsafe {
        let _ = CloseHandle(h);
    }
    (r.0 == 0).then(|| crate::util::from_wide(&buf)).filter(|s| !s.is_empty())
}

// ------------------------------------------------------------ closing apps

/// How to start a closed app again.
#[derive(Clone, Serialize, Deserialize, Debug, Default)]
pub struct Closed {
    pub key: String,
    pub name: String,
    pub exe: String,
    pub args: Vec<String>,
    pub aumid: Option<String>,
}

struct CloseResult {
    closed: Vec<Closed>,
    /// Apps that still had a window open after asking (probably "save changes?").
    kept: Vec<String>,
}

fn close_apps(sys: &mut System, picked: &[&Group]) -> CloseResult {
    let wins = windows_by_pid();
    let mut closed = Vec::new();
    // How to reopen each app: its first process (the one the others started from).
    for g in picked {
        let set: HashSet<u32> = g.pids.iter().copied().collect();
        let mut roots: Vec<Closed> = Vec::new();
        for pid in &g.pids {
            let Some(p) = sys.process(Pid::from_u32(*pid)) else { continue };
            if p.parent().map(|pp| set.contains(&pp.as_u32())).unwrap_or(false) {
                continue;
            }
            let args: Vec<String> = p.cmd().iter().skip(1).map(|a| a.to_string_lossy().to_string()).collect();
            let c = Closed { key: g.key.clone(), name: g.name.clone(), exe: g.exe.clone(), args, aumid: app_user_model_id(*pid) };
            if !roots.iter().any(|r| r.args == c.args) {
                roots.push(c);
            }
        }
        roots.truncate(2);
        if roots.is_empty() {
            roots.push(Closed { key: g.key.clone(), name: g.name.clone(), exe: g.exe.clone(), args: vec![], aumid: None });
        }
        closed.extend(roots);
        // Ask windows to close like clicking X (the app saves its session);
        // background-only apps are ended the way Task Manager does it.
        let mut asked = false;
        for pid in &g.pids {
            for h in wins.get(pid).into_iter().flatten() {
                unsafe {
                    let _ = PostMessageW(Some(HWND(*h as *mut _)), WM_CLOSE, WPARAM(0), LPARAM(0));
                }
                asked = true;
            }
        }
        if !asked {
            kill_all(sys, &g.pids);
        }
    }
    // Wait for them. Apps that hid in the tray instead of closing are ended;
    // apps still showing a window (asking to save?) are left alone.
    let start = Instant::now();
    let mut kept = Vec::new();
    loop {
        std::thread::sleep(Duration::from_millis(400));
        scan(sys, false);
        let wins = windows_by_pid();
        let alive = |g: &Group| g.pids.iter().any(|p| sys.process(Pid::from_u32(*p)).is_some());
        let pending: Vec<&&Group> = picked.iter().filter(|g| alive(g)).collect();
        if pending.is_empty() {
            break;
        }
        let waited = start.elapsed();
        for g in &pending {
            let has_window = g.pids.iter().any(|p| wins.contains_key(p));
            if !has_window && waited > Duration::from_millis(1500) {
                kill_all(sys, &g.pids);
            }
        }
        if waited > Duration::from_secs(8) {
            for g in pending {
                if g.pids.iter().any(|p| wins.contains_key(p)) {
                    kept.push(g.name.clone());
                } else {
                    kill_all(sys, &g.pids);
                }
            }
            break;
        }
    }
    closed.retain(|c| !kept.contains(&c.name));
    CloseResult { closed, kept }
}

fn kill_all(sys: &System, pids: &[u32]) {
    for pid in pids {
        // never a process Windows marks critical (ending it is a blue screen)
        if crate::sys::is_critical(*pid) != Some(false) {
            continue;
        }
        if let Some(p) = sys.process(Pid::from_u32(*pid)) {
            p.kill();
        }
    }
}

/// Starts a closed app again, as the normal user (Deck itself runs as admin).
fn reopen(c: &Closed) -> bool {
    if let Some(id) = &c.aumid {
        return crate::util::launch_as_user(&format!("shell:AppsFolder\\{id}"), "", "").is_ok();
    }
    let dir = Path::new(&c.exe).parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("C:\\"));
    let args = c.args.iter().map(|a| if a.contains(' ') && !a.starts_with('"') { format!("\"{a}\"") } else { a.clone() }).collect::<Vec<_>>().join(" ");
    crate::util::launch_as_user(&c.exe, &args, &dir.to_string_lossy()).is_ok()
}

// ------------------------------------------------------------ memory & priority

/// Asks Windows to page out the idle memory of these processes. Returns bytes freed.
fn trim(sys: &mut System, pids: &[u32]) -> u64 {
    use windows::Win32::System::ProcessStatus::K32EmptyWorkingSet;
    let before: u64 = pids.iter().filter_map(|p| sys.process(Pid::from_u32(*p))).map(|p| p.memory()).sum();
    for pid in pids {
        if let Some(h) = open(*pid, PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SET_QUOTA) {
            unsafe {
                let _ = K32EmptyWorkingSet(h);
                let _ = CloseHandle(h);
            }
        }
    }
    unsafe {
        let _ = K32EmptyWorkingSet(GetCurrentProcess());
    }
    std::thread::sleep(Duration::from_millis(300));
    scan(sys, false);
    let after: u64 = pids.iter().filter_map(|p| sys.process(Pid::from_u32(*p))).map(|p| p.memory()).sum();
    before.saturating_sub(after)
}

/// (pid, start time) of processes Deck moved to "below normal".
fn lower_priority(sys: &System, pids: &[u32]) -> Vec<(u32, u64)> {
    let mut out = Vec::new();
    for pid in pids {
        let Some(h) = open(*pid, PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SET_INFORMATION) else { continue };
        unsafe {
            if GetPriorityClass(h) == NORMAL_PRIORITY_CLASS.0 && SetPriorityClass(h, BELOW_NORMAL_PRIORITY_CLASS).is_ok() {
                let st = sys.process(Pid::from_u32(*pid)).map(|p| p.start_time()).unwrap_or(0);
                out.push((*pid, st));
            }
            let _ = CloseHandle(h);
        }
    }
    out
}

fn restore_priority(sys: &System, list: &[(u32, u64)]) {
    for (pid, st) in list {
        // The same process (ids get reused)?
        if sys.process(Pid::from_u32(*pid)).map(|p| p.start_time()) != Some(*st) {
            continue;
        }
        let Some(h) = open(*pid, PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SET_INFORMATION) else { continue };
        unsafe {
            if GetPriorityClass(h) == BELOW_NORMAL_PRIORITY_CLASS.0 {
                let _ = SetPriorityClass(h, NORMAL_PRIORITY_CLASS);
            }
            let _ = CloseHandle(h);
        }
    }
}

// ------------------------------------------------------------ power plan

use crate::profiles::plans as plan;

// ------------------------------------------------------------ state

#[derive(Clone, Serialize, Deserialize, Debug, Default)]
#[serde(default)]
struct Active {
    since: u64,
    closed: Vec<Closed>,
    kept: Vec<String>,
    prev_scheme: Option<String>,
    set_scheme: Option<String>,
    plan_from: String,
    plan_to: String,
    prev_overlay: Option<String>,
    set_overlay: bool,
    lowered: Vec<(u32, u64)>,
    freed: u64,
    game: Option<String>,
    game_pid: Option<u32>,
    seen_game: bool,
    last_game: u64,
    /// Deck profiles switched for Full power: (kind, profile before, profile set)
    profiles: Vec<(String, String, String)>,
}

pub struct Boost {
    active: Mutex<Option<Active>>,
    last_clean: Mutex<Option<Value>>,
    busy: AtomicBool,
    dirs: Mutex<(Vec<String>, Option<Instant>)>,
    /// Fullscreen programs seen while Full power was on (games Deck didn't know).
    fs_games: Mutex<HashMap<u32, String>>,
}

fn state_path() -> PathBuf {
    crate::settings::Settings::dir().join("boost.json")
}

fn save(a: &Option<Active>) {
    match a {
        Some(a) => {
            let _ = std::fs::create_dir_all(crate::settings::Settings::dir());
            let _ = std::fs::write(state_path(), serde_json::to_string_pretty(a).unwrap_or_default());
        }
        None => {
            let _ = std::fs::remove_file(state_path());
        }
    }
}

impl Boost {
    pub fn new() -> Self {
        Self {
            active: Mutex::new(None),
            last_clean: Mutex::new(None),
            busy: AtomicBool::new(false),
            dirs: Mutex::new((Vec::new(), None)),
            fs_games: Mutex::new(HashMap::new()),
        }
    }

    fn game_dirs(&self) -> Vec<String> {
        let mut d = self.dirs.lock();
        if d.1.map(|t| t.elapsed() > Duration::from_secs(600)).unwrap_or(true) {
            *d = (game_dirs(), Some(Instant::now()));
        }
        d.0.clone()
    }

    /// Small status for the sidebar, tray and phone.
    pub fn brief(&self) -> Value {
        let a = self.active.lock();
        json!({
            "on": a.is_some(),
            "game": a.as_ref().and_then(|a| a.game.clone()),
            "busy": self.busy.load(Ordering::Relaxed),
        })
    }

    /// Everything the Game boost page shows.
    pub fn state(&self) -> Value {
        let a = self.active.lock().clone();
        let plan = match plan::choose() {
            plan::Choice::Already(n) => json!({ "kind": "already", "name": n }),
            plan::Choice::Scheme(_, n) => json!({ "kind": "scheme", "name": n, "from": plan::active().map(|g| plan::name(&g)).unwrap_or_default() }),
            plan::Choice::Overlay => json!({ "kind": "overlay", "name": "Best performance" }),
            plan::Choice::Nothing => json!({ "kind": "none" }),
        };
        json!({
            "on": a.is_some(),
            "busy": self.busy.load(Ordering::Relaxed),
            "active": a.map(|a| json!({
                "since": a.since,
                "closed": a.closed.iter().map(|c| c.name.clone()).collect::<Vec<_>>().dedup_names(),
                "kept": a.kept,
                "plan_from": a.plan_from,
                "plan_to": a.plan_to,
                "lowered": a.lowered.len(),
                "freed": a.freed,
                "game": a.game,
                "seen_game": a.seen_game,
            })),
            "plan": plan,
            "last_clean": *self.last_clean.lock(),
            "ram": ram(),
        })
    }
}

trait DedupNames {
    fn dedup_names(self) -> Vec<String>;
}
impl DedupNames for Vec<String> {
    fn dedup_names(mut self) -> Vec<String> {
        let mut seen = HashSet::new();
        self.retain(|n| seen.insert(n.clone()));
        self
    }
}

fn ram() -> Value {
    let m = crate::hw::memclean::status();
    json!({ "total": m.map(|m| m.ullTotalPhys).unwrap_or(0), "free": m.map(|m| m.ullAvailPhys).unwrap_or(0) })
}

/// The apps list on the Game boost page.
pub fn apps(app: &App) -> Vec<Value> {
    let dirs = app.boost.game_dirs();
    let mut sys = System::new();
    scan(&mut sys, false);
    let wins = windows_by_pid();
    let picks = app.settings.lock().boost_close.clone();
    groups(&sys, &wins, &dirs)
        .into_iter()
        .map(|g| {
            let checked = picks.get(&g.key).copied().unwrap_or(g.suggested);
            json!({
                "key": g.key, "name": g.name, "exe": g.exe, "mem": g.mem, "count": g.pids.len(),
                "window": g.window, "voice": g.voice, "suggested": g.suggested, "checked": checked,
            })
        })
        .collect()
}

struct Prep {
    sys: System,
    groups: Vec<Group>,
    picked: Vec<usize>,
}

fn prepare(app: &App) -> Prep {
    let dirs = app.boost.game_dirs();
    let mut sys = System::new();
    scan(&mut sys, true);
    let wins = windows_by_pid();
    let picks = app.settings.lock().boost_close.clone();
    let groups = groups(&sys, &wins, &dirs);
    let picked = groups.iter().enumerate().filter(|(_, g)| picks.get(&g.key).copied().unwrap_or(g.suggested)).map(|(i, _)| i).collect();
    Prep { sys, groups, picked }
}

/// Clean up: close the picked apps and free memory. Changes nothing else.
pub fn clean(app: &App) -> Result<Value, String> {
    if app.boost.busy.swap(true, Ordering::SeqCst) {
        return Err("Deck is already on it".into());
    }
    app.changed("boost");
    let ram_before = ram()["free"].as_u64().unwrap_or(0);
    let mut p = prepare(app);
    let picked: Vec<&Group> = p.picked.iter().map(|i| &p.groups[*i]).collect();
    let res = close_apps(&mut p.sys, &picked);
    let rest: Vec<u32> = p.groups.iter().enumerate().filter(|(i, _)| !p.picked.contains(i)).flat_map(|(_, g)| g.pids.clone()).collect();
    let freed = trim(&mut p.sys, &rest);
    let ram_after = ram()["free"].as_u64().unwrap_or(0);
    let out = json!({
        "at": now_s(),
        "closed": res.closed.iter().map(|c| c.name.clone()).collect::<Vec<_>>().dedup_names(),
        "kept": res.kept,
        "freed": freed,
        "ram_gain": ram_after.saturating_sub(ram_before),
    });
    *app.boost.last_clean.lock() = Some(out.clone());
    app.boost.busy.store(false, Ordering::SeqCst);
    app.changed("boost");
    Ok(out)
}

/// Full power on.
pub fn start(app: &App) -> Result<Value, String> {
    if app.boost.active.lock().is_some() {
        return Err("Full power is already on".into());
    }
    if app.boost.busy.swap(true, Ordering::SeqCst) {
        return Err("Deck is already on it".into());
    }
    app.changed("boost");
    let s = app.settings.lock().clone();
    let mut p = prepare(app);
    let mut a = Active { since: now_s(), ..Default::default() };

    let picked: Vec<&Group> = p.picked.iter().map(|i| &p.groups[*i]).collect();
    let res = close_apps(&mut p.sys, &picked);
    a.closed = res.closed;
    a.kept = res.kept;

    // Deck's own CPU/GPU profiles, when chosen, replace "fastest power plan"
    for (kind, want, before) in [("cpu", &s.boost_cpu, &s.active_cpu), ("gpu", &s.boost_gpu, &s.active_gpu)] {
        let exists = if kind == "cpu" { s.cpu_profiles.iter().any(|p| &p.id == want) } else { s.gpu_profiles.iter().any(|p| &p.id == want) };
        if !want.is_empty() && exists && want != before {
            if kind == "cpu" {
                // the plan from before comes back through the normal plan restore
                a.prev_scheme = plan::active().map(|x| plan::guid_str(&x));
                a.plan_from = plan::active().map(|x| plan::name(&x)).unwrap_or_default();
                a.set_scheme = s.cpu_profiles.iter().find(|p| &p.id == want).map(|p| p.plan.clone());
            }
            crate::profiles::switch(app, kind, want, "boost");
            a.profiles.push((kind.to_string(), before.clone(), want.clone()));
            if kind == "cpu" {
                a.plan_to = s.cpu_profiles.iter().find(|p| &p.id == want).map(|p| format!("CPU {}", p.name)).unwrap_or_default();
            }
        }
    }
    let cpu_profile = !s.boost_cpu.is_empty() && s.cpu_profiles.iter().any(|p| p.id == s.boost_cpu);
    if s.boost_power && !cpu_profile {
        match plan::choose() {
            plan::Choice::Scheme(g, name) => {
                let prev = plan::active();
                if plan::set_active(&g).is_ok() {
                    a.plan_from = prev.map(|x| plan::name(&x)).unwrap_or_default();
                    a.plan_to = name;
                    a.prev_scheme = prev.map(|x| plan::guid_str(&x));
                    a.set_scheme = Some(plan::guid_str(&g));
                }
            }
            plan::Choice::Overlay => {
                let prev = plan::overlay();
                if plan::set_overlay(plan::OVERLAY_BEST) {
                    a.plan_from = "Balanced".into();
                    a.plan_to = "Best performance".into();
                    a.prev_overlay = prev.map(|x| plan::guid_str(&x));
                    a.set_overlay = true;
                }
            }
            _ => {}
        }
    }

    // What's left running in the background.
    let rest: Vec<&Group> = p.groups.iter().enumerate().filter(|(i, _)| !p.picked.contains(i)).map(|(_, g)| g).collect();
    if s.boost_priority {
        let pids: Vec<u32> = rest.iter().filter(|g| !g.window && !g.voice).flat_map(|g| g.pids.clone()).collect();
        a.lowered = lower_priority(&p.sys, &pids);
    }
    // trimming only helps when memory is short: with room to spare Windows already gives the game
    // what it needs, and trimmed apps would page their memory back in later (small hitches)
    if s.boost_trim && memory_short() {
        let pids: Vec<u32> = rest.iter().flat_map(|g| g.pids.clone()).collect();
        a.freed = trim(&mut p.sys, &pids);
    }
    if let Some((pid, name)) = find_game(app, &mut p.sys) {
        a.game = Some(name);
        a.game_pid = Some(pid);
        a.seen_game = true;
        a.last_game = now_s();
    }

    let report = json!({
        "closed": a.closed.iter().map(|c| c.name.clone()).collect::<Vec<_>>().dedup_names(),
        "kept": a.kept, "plan_to": a.plan_to, "lowered": a.lowered.len(), "freed": a.freed,
    });
    save(&Some(a.clone()));
    *app.boost.active.lock() = Some(a);
    ON.store(true, Ordering::Relaxed);
    app.boost.busy.store(false, Ordering::SeqCst);
    app.changed("boost");
    Ok(report)
}

/// Memory use at or over 85 %: then moving background apps' idle memory aside makes real room.
fn memory_short() -> bool {
    let mut m = System::new();
    m.refresh_memory();
    m.total_memory() > 0 && m.used_memory() * 100 / m.total_memory() >= 85
}

/// Full power off: everything back the way it was.
pub fn stop(app: &App, why: &str) -> Result<Value, String> {
    let Some(a) = app.boost.active.lock().take() else { return Err("Full power isn't on".into()) };
    ON.store(false, Ordering::Relaxed);
    let out = restore(&a, true);
    restore_profiles(app, &a);
    save(&None);
    app.boost.fs_games.lock().clear();
    app.changed("boost");
    let reopened = out["reopened"].as_array().map(|v| v.len()).unwrap_or(0);
    let mut body = String::from("Power plan and background apps are back to normal.");
    if reopened > 0 {
        body = format!("Reopened {reopened} app{} and put the power plan back.", if reopened == 1 { "" } else { "s" });
    }
    if !why.is_empty() {
        app.toast(Toast::simple("boost", &format!("Full power off: {why}"), &body));
    }
    Ok(out)
}

/// Toast text for a Full power / Clean up result ("boost" | "clean").
pub fn summary(what: &str, v: &Value) -> (String, String) {
    let names: Vec<&str> = v["closed"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
    let closed = match names.len() {
        0 => "No apps to close".to_string(),
        1..=3 => format!("Closed {}", names.join(", ")),
        n => format!("Closed {}, {} and {} more", names[0], names[1], n - 2),
    };
    let freed = v["freed"].as_u64().unwrap_or(0);
    let mut parts = vec![closed];
    if freed > 50 << 20 {
        parts.push(format!("moved {} of background apps' idle memory aside", crate::util::fmt_bytes(freed)));
    }
    if what == "boost" {
        if let Some(p) = v["plan_to"].as_str().filter(|p| !p.is_empty()) {
            parts.push(format!("power plan: {p}"));
        }
    }
    if let Some(k) = v["kept"].as_array().filter(|k| !k.is_empty()) {
        let k: Vec<&str> = k.iter().filter_map(Value::as_str).collect();
        parts.push(format!("{} stayed open (it may be asking to save)", k.join(", ")));
    }
    let title = if what == "boost" { "Full power is on" } else { "Cleaned up" };
    (title.to_string(), parts.join(" · "))
}

/// Puts back the CPU/GPU profiles that were on before Full power (unless the user switched meanwhile).
fn restore_profiles(app: &App, a: &Active) {
    for (kind, before, set) in &a.profiles {
        let now = if kind == "cpu" { app.settings.lock().active_cpu.clone() } else { app.settings.lock().active_gpu.clone() };
        if &now != set {
            continue;
        }
        if before.is_empty() {
            if kind == "gpu" {
                if let Some(g) = app.gpu_engine() {
                    let _ = g.stock();
                    app.settings.lock().active_gpu.clear();
                    app.save_settings();
                    app.changed("profiles");
                }
            }
            // a CPU plan that belongs to no profile was active before: the plan part of "restore" covers it
        } else {
            crate::profiles::switch(app, kind, before, "boost");
        }
    }
}

fn restore(a: &Active, reopen_apps: bool) -> Value {
    if let (Some(prev), Some(set)) = (a.prev_scheme.as_deref().and_then(plan::parse), a.set_scheme.as_deref().and_then(plan::parse)) {
        // Only if the user didn't pick another plan meanwhile.
        if plan::active() == Some(set) {
            let _ = plan::set_active(&prev);
        }
    }
    if a.set_overlay && plan::overlay() == Some(plan::OVERLAY_BEST) {
        plan::set_overlay(a.prev_overlay.as_deref().and_then(plan::parse).unwrap_or(GUID::zeroed()));
    }
    let mut sys = System::new();
    scan(&mut sys, false);
    restore_priority(&sys, &a.lowered);
    let mut reopened = Vec::new();
    if reopen_apps {
        let running: HashSet<String> = sys.processes().values().filter_map(|p| p.exe()).map(|e| stem_of(&e.to_string_lossy())).collect();
        let mut done = HashSet::new();
        for c in &a.closed {
            if running.contains(&c.key) || done.contains(&c.key) && c.args.is_empty() {
                continue;
            }
            if reopen(c) {
                done.insert(c.key.clone());
                if !reopened.contains(&c.name) {
                    reopened.push(c.name.clone());
                }
            }
        }
    }
    json!({ "reopened": reopened })
}

// ------------------------------------------------------------ game watch

/// The game that's running, if Deck can tell: a program from a game folder,
/// or a fullscreen program that isn't a browser or video player.
fn find_game(app: &App, sys: &mut System) -> Option<(u32, String)> {
    let dirs = app.boost.game_dirs();
    let mut best: Option<(u64, u32, String)> = None;
    for (pid, p) in sys.processes() {
        let Some(exe) = p.exe() else { continue };
        let lower = exe.to_string_lossy().to_lowercase();
        let stem = stem_of(&lower);
        if !in_game_dir(&lower, &dirs) || NEVER_PARTS.iter().any(|x| stem.contains(x)) || stem.contains("launcher") && !stem.contains("client") {
            continue;
        }
        if best.as_ref().map(|b| p.memory() > b.0).unwrap_or(true) {
            best = Some((p.memory(), pid.as_u32(), friendly_name(&exe.to_string_lossy())));
        }
    }
    if let Some((_, pid, name)) = best {
        return Some((pid, name));
    }
    // Games Deck doesn't know about: remember fullscreen programs.
    let mut fs = app.boost.fs_games.lock();
    fs.retain(|pid, _| sys.process(Pid::from_u32(*pid)).is_some());
    let gpu_busy = app.gpu_engine().and_then(|g| g.nvml.live().util).is_some_and(|u| u >= 60);
    if crate::hotcorner::fullscreen_in_front() || gpu_busy {
        use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
        let mut pid = 0u32;
        unsafe {
            GetWindowThreadProcessId(GetForegroundWindow(), Some(&mut pid));
        }
        if let Some(path) = process_path(pid) {
            let stem = stem_of(&path);
            let windir = std::env::var("WINDIR").unwrap_or_default().to_lowercase();
            if pid != std::process::id() && !NOT_GAMES.contains(&stem.as_str()) && !(!windir.is_empty() && path.to_lowercase().starts_with(&windir)) {
                fs.insert(pid, friendly_name(&path));
            }
        }
    }
    fs.iter().next().map(|(p, n)| (*p, n.clone()))
}

/// Watches for the game while Full power is on, and turns it off a little
/// after the game closes. Also puts things back if Deck was closed (or
/// crashed) while Full power was on.
pub fn start_watch(app: Arc<App>) {
    if let Some(a) = std::fs::read_to_string(state_path()).ok().and_then(|t| serde_json::from_str::<Active>(&t).ok()) {
        // After a restart the power plan is still switched; apps and
        // priorities started fresh anyway. The file is removed only once everything is back, so
        // a second crash in between still finds it.
        let me = app.clone();
        std::thread::spawn(move || {
            // after the engines (and the GPU startup profile)
            let t0 = Instant::now();
            while crate::elevation::is_admin() && !(me.gpu_done.load(Ordering::SeqCst) && me.hw_done.load(Ordering::SeqCst)) && t0.elapsed() < Duration::from_secs(25) {
                std::thread::sleep(Duration::from_millis(250));
            }
            let rebooted = System::boot_time() > a.since;
            restore(&a, !rebooted);
            restore_profiles(&me, &a);
            if !is_on() {
                save(&None);
            }
            me.toast(Toast::simple("boost", "Full power was still on", "Deck put your power plan and apps back."));
        });
    }
    std::thread::Builder::new()
        .name("boost-watch".into())
        .spawn(move || {
            let mut sys = System::new();
            loop {
                std::thread::sleep(Duration::from_secs(3));
                if !is_on() {
                    continue;
                }
                scan(&mut sys, false);
                let found = find_game(&app, &mut sys);
                let auto_end = app.settings.lock().boost_auto_end;
                let mut end = false;
                let mut no_game = false;
                {
                    let mut g = app.boost.active.lock();
                    let Some(a) = g.as_mut() else { continue };
                    let before = (a.game.clone(), a.seen_game);
                    match found {
                        Some((pid, name)) => {
                            a.game = Some(name);
                            a.game_pid = Some(pid);
                            a.seen_game = true;
                            a.last_game = now_s();
                        }
                        None => {
                            a.game = None;
                            // A short gap is a launcher handing over to the game, or a
                            // match loading; 45 s of nothing means you stopped playing.
                            if auto_end && a.seen_game && now_s().saturating_sub(a.last_game) > 45 {
                                end = true;
                            }
                            // no game ever spotted: it doesn't stay on forever
                            if auto_end && !a.seen_game && now_s().saturating_sub(a.since) > NO_GAME_LIMIT {
                                end = true;
                                no_game = true;
                            }
                        }
                    }
                    if (a.game.clone(), a.seen_game) != before {
                        save(&Some(a.clone()));
                        drop(g);
                        app.changed("boost");
                    }
                }
                if end {
                    let _ = stop(&app, if no_game { "no game was found for 3 hours" } else { "your game closed" });
                }
            }
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(stem_of("C:\\A\\Discord.exe"), "discord");
        assert_eq!(stem_of("Battle.net.exe"), "battle.net");
        assert!(never("easyanticheat_eos", "c:\\x\\easyanticheat_eos.exe"));
        assert!(never("nvcontainer", "c:\\program files\\nvidia corporation\\nvcontainer.exe"));
        assert!(never("vgc", "c:\\program files\\riot vanguard\\vgc.exe"));
        assert!(!never("onedrive", "c:\\users\\x\\appdata\\local\\microsoft\\onedrive\\onedrive.exe"));
    }

    #[test]
    fn game_folders() {
        let dirs = vec!["d:\\steamlibrary\\steamapps\\common\\counter-strike global offensive\\".to_string()];
        assert!(in_game_dir("d:\\steamlibrary\\steamapps\\common\\counter-strike global offensive\\game\\bin\\win64\\cs2.exe", &dirs));
        assert!(in_game_dir("e:\\riot games\\valorant\\live\\valorant.exe", &dirs));
        assert!(!in_game_dir("e:\\riot games\\riot client\\riotclientservices.exe", &dirs));
        assert!(!in_game_dir("d:\\steamlibrary\\steamapps\\common\\wallpaper_engine\\wallpaper32.exe", &dirs));
    }

    #[test]
    fn guid_round_trip() {
        let s = plan::guid_str(&plan::HIGH);
        assert_eq!(plan::parse(&s), Some(plan::HIGH));
    }

    /// Runs the real close / reopen / priority / trim code on a hidden
    /// throwaway process. Touches nothing else.
    #[test]
    #[ignore]
    fn close_and_reopen_dummy() {
        use std::os::windows::process::CommandExt;
        let windir = std::env::var("WINDIR").unwrap();
        let exe = format!("{windir}\\System32\\PING.EXE");
        let child = std::process::Command::new(&exe)
            .args(["-n", "300", "127.0.0.1"])
            .creation_flags(crate::util::CREATE_NO_WINDOW)
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let pid = child.id();
        std::thread::sleep(Duration::from_millis(300));
        let mut sys = System::new();
        scan(&mut sys, true);
        let g = Group { key: "ping".into(), name: "ping".into(), exe: exe.clone(), pids: vec![pid], mem: 0, window: false, voice: false, suggested: true };

        let lowered = lower_priority(&sys, &[pid]);
        assert_eq!(lowered.len(), 1, "priority lowered");
        let prio = |pid| {
            let h = open(pid, PROCESS_QUERY_LIMITED_INFORMATION).unwrap();
            let p = unsafe { GetPriorityClass(h) };
            unsafe { CloseHandle(h).ok() };
            p
        };
        assert_eq!(prio(pid), BELOW_NORMAL_PRIORITY_CLASS.0);
        restore_priority(&sys, &lowered);
        assert_eq!(prio(pid), NORMAL_PRIORITY_CLASS.0);
        println!("trim freed {} KB", trim(&mut sys, &[pid]) / 1024);

        let res = close_apps(&mut sys, &[&g]);
        assert!(res.kept.is_empty());
        assert_eq!(res.closed.len(), 1);
        println!("closed: {:?}", res.closed[0]);
        assert_eq!(res.closed[0].args, vec!["-n", "300", "127.0.0.1"]);
        scan(&mut sys, false);
        assert!(sys.process(Pid::from_u32(pid)).is_none(), "dummy closed");

        assert!(reopen(&res.closed[0]));
        std::thread::sleep(Duration::from_millis(500));
        scan(&mut sys, true);
        let again: Vec<u32> = sys
            .processes()
            .iter()
            .filter(|(_, p)| p.exe().map(|e| e.to_string_lossy().eq_ignore_ascii_case(&exe)).unwrap_or(false))
            .filter(|(_, p)| p.cmd().iter().any(|a| a == "300"))
            .map(|(p, _)| p.as_u32())
            .collect();
        println!("reopened as {again:?}");
        assert_eq!(again.len(), 1, "reopened once");
        kill_all(&sys, &again);
    }

    /// Read-only: prints what Full power would do on this PC.
    #[test]
    #[ignore]
    fn dry_run() {
        let dirs = game_dirs();
        println!("game dirs: {dirs:#?}");
        let mut sys = System::new();
        scan(&mut sys, true);
        let wins = windows_by_pid();
        for g in groups(&sys, &wins, &dirs) {
            println!(
                "{:>6} MB  {:<32} {:<22} win={} voice={} suggested={}",
                g.mem / 1_048_576, g.name, g.key, g.window, g.voice, g.suggested
            );
        }
        let cur = plan::active().unwrap();
        println!("plan now: {} ({:?}) personality {:?}", plan::name(&cur), cur, plan::personality(&cur));
        for s in plan::list() {
            println!("  scheme {} {:?} personality {:?}", plan::name(&s), s, plan::personality(&s));
        }
        println!("overlay: {:?}", plan::overlay());
        match plan::choose() {
            plan::Choice::Already(n) => println!("choice: already {n}"),
            plan::Choice::Scheme(g, n) => println!("choice: switch to {n} {g:?}"),
            plan::Choice::Overlay => println!("choice: overlay best"),
            plan::Choice::Nothing => println!("choice: nothing"),
        }
    }
}
