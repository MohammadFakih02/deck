//! Small Windows helpers shared by the modules.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};
use windows::core::{HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HWND};
use windows::Win32::System::Com::{CoInitializeEx, CoTaskMemFree, COINIT_APARTMENTTHREADED, COINIT_MULTITHREADED};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// A std mutex's guard even when another thread panicked while holding it. A failed hardware
/// worker or sync thread must not take down the next user of its lock (the tray menu, a page):
/// the data is still there, as the panicking thread left it.
pub trait LockAnyway<T> {
    fn lock_anyway(&self) -> std::sync::MutexGuard<'_, T>;
}

impl<T> LockAnyway<T> for std::sync::Mutex<T> {
    fn lock_anyway(&self) -> std::sync::MutexGuard<'_, T> {
        self.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// COM for the calling thread (multithreaded apartment). Harmless when the
/// thread already has COM in either mode.
pub fn com() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
}

/// COM in a single-threaded apartment, for threads that own shell objects.
pub fn com_sta() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
}

pub fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

pub fn now_s() -> u64 {
    now_ms() / 1000
}

pub fn env_path(var: &str) -> Option<PathBuf> {
    std::env::var_os(var).map(PathBuf::from)
}

pub fn home() -> PathBuf {
    env_path("USERPROFILE").unwrap_or_else(|| PathBuf::from("C:\\"))
}

pub fn local_appdata() -> PathBuf {
    env_path("LOCALAPPDATA").unwrap_or_else(|| home().join("AppData\\Local"))
}

pub fn appdata() -> PathBuf {
    env_path("APPDATA").unwrap_or_else(|| home().join("AppData\\Roaming"))
}

pub fn downloads() -> PathBuf {
    home().join("Downloads")
}

/// A Windows folder as Windows itself records it. Not from environment variables: the signed-in
/// user can set those (an elevated Deck inherits them), and the elevated Deck must not follow them
/// to programs, drivers or libraries.
fn known_folder(id: &windows::core::GUID, fallback: &str) -> PathBuf {
    use windows::Win32::UI::Shell::{SHGetKnownFolderPath, KF_FLAG_DEFAULT};
    unsafe { SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).ok().map(|p| take_pwstr(p)) }
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(fallback))
}

pub fn program_files() -> PathBuf {
    known_folder(&windows::Win32::UI::Shell::FOLDERID_ProgramFiles, r"C:\Program Files")
}

pub fn program_files_x86() -> PathBuf {
    known_folder(&windows::Win32::UI::Shell::FOLDERID_ProgramFilesX86, r"C:\Program Files (x86)")
}

pub fn program_data() -> PathBuf {
    known_folder(&windows::Win32::UI::Shell::FOLDERID_ProgramData, r"C:\ProgramData")
}

/// The Windows folder.
pub fn windows_dir() -> PathBuf {
    known_folder(&windows::Win32::UI::Shell::FOLDERID_Windows, r"C:\Windows")
}

/// Windows' System32 folder.
pub fn system_dir() -> PathBuf {
    known_folder(&windows::Win32::UI::Shell::FOLDERID_System, r"C:\Windows\System32")
}

/// Where a vendor library (GPU drivers' DLLs) may be loaded from: Windows' own folder, and for
/// NVIDIA's management library also NVIDIA's folder under Program Files (older drivers put it
/// there). Never a bare name: that would also search the current folder and PATH, which the
/// signed-in user controls, and Deck loads these with admin rights. Empty for anything that isn't
/// a plain file name.
pub fn trusted_dll_paths(name: &str) -> Vec<PathBuf> {
    let plain = !name.is_empty() && !name.contains(['\\', '/', ':']) && name != "." && name != "..";
    if !plain {
        return vec![];
    }
    let mut out = vec![system_dir().join(name)];
    if name.eq_ignore_ascii_case("nvml.dll") {
        out.push(program_files().join("NVIDIA Corporation").join("NVSMI").join(name));
    }
    out
}

/// Loads a vendor library from the first of its trusted places (see `trusted_dll_paths`) that
/// has it; its own dependencies come from its folder and System32 only. Null when none has it.
pub fn load_trusted_dll(name: &str) -> *mut std::ffi::c_void {
    use windows_sys::Win32::System::LibraryLoader::{LoadLibraryExW, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, LOAD_LIBRARY_SEARCH_SYSTEM32};
    use std::os::windows::ffi::OsStrExt;
    for p in trusted_dll_paths(name).into_iter().filter(|p| p.is_file()) {
        let w: Vec<u16> = p.as_os_str().encode_wide().chain(Some(0)).collect();
        let h = unsafe { LoadLibraryExW(w.as_ptr(), std::ptr::null_mut(), LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32) };
        if !h.is_null() {
            return h as *mut _;
        }
    }
    std::ptr::null_mut()
}

/// Takes ownership of a CoTaskMem string.
pub unsafe fn take_pwstr(p: PWSTR) -> String {
    if p.is_null() {
        return String::new();
    }
    let s = p.to_string().unwrap_or_default();
    CoTaskMemFree(Some(p.0 as *const _));
    s
}

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn from_wide(buf: &[u16]) -> String {
    let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..n])
}

/// Full path of a process's executable (works for other users' processes
/// only where Windows allows limited queries).
pub fn process_path(pid: u32) -> Option<String> {
    if pid == 0 {
        return None;
    }
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
        let _ = CloseHandle(h);
        ok.then(|| String::from_utf16_lossy(&buf[..len as usize]))
    }
}

/// "C:\...\Discord.exe" -> "Discord".
pub fn exe_stem(path: &str) -> String {
    Path::new(path).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| path.to_string())
}

/// Opens a file, folder, URL or shell: target like Explorer would, as the normal user.
pub fn shell_open(target: &str) -> Result<(), String> {
    launch_as_user(target, "", "")
}

fn shell_exec(file: &str, args: &str, dir: &str) -> Result<(), String> {
    let opt = |s: &str| if s.is_empty() { None } else { Some(HSTRING::from(s)) };
    let (a, d) = (opt(args), opt(dir));
    let r = unsafe {
        ShellExecuteW(
            None,
            &HSTRING::from("open"),
            &HSTRING::from(file),
            a.as_ref().map(|h| PCWSTR(h.as_ptr())).unwrap_or(PCWSTR::null()),
            d.as_ref().map(|h| PCWSTR(h.as_ptr())).unwrap_or(PCWSTR::null()),
            SW_SHOWNORMAL,
        )
    };
    if r.0 as isize > 32 {
        Ok(())
    } else {
        Err(format!("Windows couldn't open {file}"))
    }
}

/// Tries `attempt` up to `tries` times, `wait` apart (Explorer may be restarting, which Deck itself
/// does when it applies a setup). There is no fallback: the caller gets an error instead.
fn retry_as_user(mut attempt: impl FnMut() -> Result<(), String>, tries: u32, wait: std::time::Duration) -> Result<(), String> {
    let mut last = String::new();
    for i in 0..tries.max(1) {
        if i > 0 {
            std::thread::sleep(wait);
        }
        match attempt() {
            Ok(()) => {
                if i > 0 {
                    crate::log::info(format!("open as the user: Explorer answered on try {} (before: {last})", i + 1));
                }
                return Ok(());
            }
            Err(e) => last = e,
        }
    }
    crate::log::warn(format!("open as the user: Explorer didn't answer ({last})"));
    Err(format!("Windows Explorer isn't answering ({last}), and Deck won't open this with its admin rights. Try again in a moment."))
}

/// Starts a program, file or link the way Explorer would, **without** Deck's admin rights:
/// when Deck is elevated the request is handed to Explorer (which runs as the normal user)
/// through its shell automation object. If Explorer can't take it, this fails rather than start
/// the target elevated.
pub fn launch_as_user(file: &str, args: &str, dir: &str) -> Result<(), String> {
    if !crate::elevation::is_admin() {
        return shell_exec(file, args, dir);
    }
    retry_as_user(
        || {
            let (f, a, d) = (file.to_string(), args.to_string(), dir.to_string());
            // shell automation objects want a fresh single-threaded apartment
            std::thread::spawn(move || {
                com_sta();
                unsafe { via_explorer(&f, &a, &d) }
            })
            .join()
            .unwrap_or_else(|_| Err(windows::core::Error::from_hresult(windows::Win32::Foundation::E_FAIL)))
            .map_err(|e| e.message())
        },
        6,
        std::time::Duration::from_millis(500),
    )
}

unsafe fn via_explorer(file: &str, args: &str, dir: &str) -> windows::core::Result<()> {
    use windows::core::{Interface, BSTR};
    use windows::Win32::System::Com::{CoCreateInstance, IDispatch, IServiceProvider, CLSCTX_LOCAL_SERVER};
    use windows::Win32::System::Variant::VARIANT;
    use windows::Win32::UI::Shell::{
        IShellBrowser, IShellDispatch2, IShellFolderViewDual, IShellWindows, ShellWindows, CSIDL_DESKTOP, SID_STopLevelBrowser, SVGIO_BACKGROUND,
        SWC_DESKTOP, SWFO_NEEDDISPATCH,
    };
    let sw: IShellWindows = CoCreateInstance(&ShellWindows, None, CLSCTX_LOCAL_SERVER)?;
    let loc = VARIANT::from(CSIDL_DESKTOP as i32);
    let root = VARIANT::default();
    let mut hwnd = 0i32;
    let disp = sw.FindWindowSW(&loc, &root, SWC_DESKTOP, &mut hwnd, SWFO_NEEDDISPATCH)?;
    let sp: IServiceProvider = disp.cast()?;
    let browser: IShellBrowser = sp.QueryService(&SID_STopLevelBrowser)?;
    let view = browser.QueryActiveShellView()?;
    let bg: IDispatch = view.GetItemObject(SVGIO_BACKGROUND)?;
    let fv: IShellFolderViewDual = bg.cast()?;
    let sd: IShellDispatch2 = fv.Application()?.cast()?;
    sd.ShellExecute(
        &BSTR::from(file),
        &VARIANT::from(BSTR::from(args)),
        &VARIANT::from(BSTR::from(dir)),
        &VARIANT::from(BSTR::from("open")),
        &VARIANT::from(1i32),
    )
}

/// Opens a folder in Explorer with `path` selected.
pub fn reveal(path: &str) {
    if let Err(e) = Command::new("explorer").arg(format!("/select,{path}")).spawn() {
        crate::log::warn(format!("reveal: Explorer couldn't be started: {e}"));
    }
}

/// Runs a console program without flashing a window.
/// Runs a console program hidden and waits for it, at most 30 minutes (a winget install on a slow
/// line; nothing Deck runs may wait forever). `run_for` takes a shorter limit.
pub fn run_hidden(exe: &str, args: &[&str]) -> std::io::Result<std::process::Output> {
    run_for(1800, exe, args)
}

/// Runs a console program hidden, ending it after `secs` (DISM, PowerShell, RustDesk's and
/// Tailscale's command lines can hang when their service is stuck).
pub fn run_for(secs: u64, exe: &str, args: &[&str]) -> std::io::Result<std::process::Output> {
    use std::io::Read;
    use std::os::windows::process::CommandExt;
    use std::process::Stdio;
    use std::time::{Duration, Instant};
    let started = Instant::now();
    // the program by name only: arguments can hold paths and keys
    let name = std::path::Path::new(exe).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| exe.to_string());
    let mut child = match Command::new(exe).args(args).creation_flags(CREATE_NO_WINDOW).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn() {
        Ok(c) => c,
        Err(e) => {
            crate::log::warn(format!("process: {name} couldn't be started: {e}"));
            return Err(e);
        }
    };
    let (mut so, mut se) = (child.stdout.take().unwrap(), child.stderr.take().unwrap());
    let ro = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = so.read_to_end(&mut b);
        b
    });
    let re = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = se.read_to_end(&mut b);
        b
    });
    let end = Instant::now() + Duration::from_secs(secs);
    let status = loop {
        if let Some(st) = child.try_wait()? {
            break st;
        }
        if Instant::now() >= end {
            let _ = child.kill();
            let _ = child.wait();
            crate::log::warn(format!("process: {name} took over {secs} s and was ended"));
            return Err(std::io::Error::new(std::io::ErrorKind::TimedOut, format!("{exe} didn't finish in {secs} s")));
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let out = std::process::Output { status, stdout: ro.join().unwrap_or_default(), stderr: re.join().unwrap_or_default() };
    // a failing exit is often expected (a query that finds nothing), so it is info; the first line
    // of what it said tells the rest
    if !out.status.success() {
        let said = String::from_utf8_lossy(&out.stderr);
        let said = said.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("").chars().take(200).collect::<String>();
        let code = out.status.code().map(|c| if c.unsigned_abs() < 1000 { c.to_string() } else { format!("{:#X}", c as u32) }).unwrap_or_else(|| "no code".into());
        let msg = format!("process: {name} exited with {code} after {}{}{said}", crate::log::took(started.elapsed()), if said.is_empty() { "" } else { ": " });
        // one that keeps failing the same way (a status query run often): once per 10 minutes
        if crate::log::every(&format!("exit {name} {code}"), Duration::from_secs(600)) {
            crate::log::info(msg);
        } else {
            crate::log::debug(msg);
        }
    } else {
        crate::log::debug_with(|| format!("process: {name} finished in {}", crate::log::took(started.elapsed())));
    }
    Ok(out)
}

pub fn hwnd_from(v: i64) -> HWND {
    HWND(v as isize as *mut _)
}

pub fn hwnd_to(h: HWND) -> i64 {
    h.0 as isize as i64
}

/// Human readable size, like Explorer.
pub fn fmt_bytes(b: u64) -> String {
    const U: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = b as f64;
    let mut i = 0;
    while v >= 1024.0 && i < U.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{b} B")
    } else if v >= 100.0 {
        format!("{v:.0} {}", U[i])
    } else {
        format!("{v:.1} {}", U[i])
    }
}

/// Total size and file count of a folder (follows no links, ignores errors).
pub fn dir_size(p: &Path) -> (u64, u64) {
    let mut size = 0u64;
    let mut files = 0u64;
    for e in jwalk::WalkDir::new(p).skip_hidden(false).follow_links(false).into_iter().flatten() {
        if e.file_type().is_file() {
            if let Ok(m) = e.metadata() {
                size += m.len();
                files += 1;
            }
        }
    }
    (size, files)
}

/// Free bytes on the drive that holds `path` (for the current user's quota).
pub fn free_bytes(path: &std::path::Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    let w: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut free = 0u64;
    unsafe { windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(windows::core::PCWSTR(w.as_ptr()), Some(&mut free), None, None).ok()? };
    Some(free)
}

const TOKEN_CHARS: &[u8] = b"abcdefghjkmnpqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789";

/// A random token of `n` characters. Panics if Windows' random number generator fails: a token made
/// without it would be guessable (the phone's key and share links are tokens).
pub fn random_token(n: usize) -> String {
    let mut out = String::with_capacity(n);
    let mut buf = [0u8; 64];
    while out.len() < n {
        getrandom::fill(&mut buf).expect("the system's random number generator failed");
        out.extend(buf.iter().filter_map(|b| token_char(*b)).take(n - out.len()));
    }
    out
}

/// The token character for one random byte, or None for the top bytes that would make the first
/// characters more likely than the others (256 isn't a multiple of the alphabet's size).
fn token_char(b: u8) -> Option<char> {
    let keep = 256 - 256 % TOKEN_CHARS.len();
    ((b as usize) < keep).then(|| TOKEN_CHARS[b as usize % TOKEN_CHARS.len()] as char)
}

/// Replaces `path` with `data` so that a crash leaves either the old file or the new one, never a
/// cut-off mix. Deck's own files live in the user's folders while Deck has admin rights, so this
/// writes only where the signed-in user could write themselves (see crate::fsguard).
/// One of at most `max` connections a listener serves at once (PC to PC, Claude Sync): anyone on
/// the network could otherwise open thousands, each holding a thread until it times out.
pub struct Slot(&'static std::sync::atomic::AtomicUsize);

impl Slot {
    pub fn take(count: &'static std::sync::atomic::AtomicUsize, max: usize) -> Option<Slot> {
        use std::sync::atomic::Ordering::{AcqRel, Relaxed};
        count.try_update(AcqRel, Relaxed, |n| (n < max).then_some(n + 1)).ok().map(|_| Slot(count))
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
    }
}

#[cfg(test)]
mod slot_tests {
    #[test]
    fn connections_past_the_limit_are_turned_away_until_one_ends() {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let a = super::Slot::take(&N, 2).unwrap();
        let _b = super::Slot::take(&N, 2).unwrap();
        assert!(super::Slot::take(&N, 2).is_none());
        drop(a);
        assert!(super::Slot::take(&N, 2).is_some());
    }
}

pub fn write_atomic(path: &Path, data: &[u8]) -> std::io::Result<()> {
    crate::fsguard::write_for_user(path, data)
}

pub fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let b = [c[0], *c.get(1).unwrap_or(&0), *c.get(2).unwrap_or(&0)];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

/// Standard base64 back to bytes (padding and line breaks allowed); None for anything else.
pub fn unbase64(s: &str) -> Option<Vec<u8>> {
    let val = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        } as u32)
    };
    let clean: Vec<u8> = s.bytes().filter(|c| !c.is_ascii_whitespace()).collect();
    let body = clean.strip_suffix(b"==").or_else(|| clean.strip_suffix(b"=")).unwrap_or(&clean);
    let mut out = Vec::with_capacity(body.len() * 3 / 4);
    for c in body.chunks(4) {
        let mut n = 0u32;
        for (i, &b) in c.iter().enumerate() {
            n |= val(b)? << (18 - 6 * i);
        }
        let bytes = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
        match c.len() {
            4 => out.extend_from_slice(&bytes),
            3 => out.extend_from_slice(&bytes[..2]),
            2 => out.push(bytes[0]),
            _ => return None,
        }
    }
    Some(out)
}

#[cfg(test)]
mod base64_tests {
    #[test]
    fn round_trips() {
        for n in 0..40usize {
            let data: Vec<u8> = (0..n).map(|i| (i * 37 + 11) as u8).collect();
            assert_eq!(super::unbase64(&super::base64(&data)).as_deref(), Some(&data[..]), "{n} bytes");
        }
        assert_eq!(super::unbase64("aGVs
bG8="), Some(b"hello".to_vec()));
        assert_eq!(super::unbase64("a$=="), None);
    }
}

#[cfg(test)]
mod free_space_tests {
}

/// Whether a row of Windows' adapter table is a real network adapter whose traffic counts once:
/// hardware (Ethernet, Wi-Fi, mobile broadband), connected, and not a filter layer stacked on
/// another adapter (Npcap, WFP, QoS, VPN and VM filters repeat the same traffic).
pub fn real_adapter(r: &windows_sys::Win32::NetworkManagement::IpHelper::MIB_IF_ROW2) -> bool {
    let bits = r.InterfaceAndOperStatusFlags._bitfield;
    let (hardware, filter) = (bits & 1 != 0, bits & 2 != 0);
    hardware && !filter && r.OperStatus == 1 && matches!(r.Type, 6 | 71 | 243 | 244)
}

/// Network rates on the PC's real adapters (see `real_adapter`), counted per adapter: only what
/// each adapter moved since the last reading, and only for adapters there both times. Several
/// connected adapters add up (that is the PC's traffic); one plugged in or reconnected starts
/// counting from its next reading (its lifetime total would be a false spike); one removed just stops.
#[derive(Default)]
pub struct NetCounter {
    prev: std::collections::HashMap<u64, (u64, u64)>,
}

impl NetCounter {
    pub fn new() -> Self {
        let mut c = Self::default();
        c.sample();
        c
    }

    /// Bytes (in, out) since the last call.
    pub fn sample(&mut self) -> (u64, u64) {
        use windows_sys::Win32::NetworkManagement::IpHelper::{FreeMibTable, GetIfTable2, MIB_IF_TABLE2};
        let (mut rx, mut tx) = (0u64, 0u64);
        let mut now = std::collections::HashMap::new();
        unsafe {
            let mut t: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
            if GetIfTable2(&mut t) != 0 || t.is_null() {
                return (0, 0);
            }
            let rows = std::slice::from_raw_parts((*t).Table.as_ptr(), (*t).NumEntries as usize);
            for r in rows.iter().filter(|r| real_adapter(r)) {
                let id = r.InterfaceLuid.Value;
                if let Some((pi, po)) = self.prev.get(&id) {
                    rx += r.InOctets.saturating_sub(*pi);
                    tx += r.OutOctets.saturating_sub(*po);
                }
                now.insert(id, (r.InOctets, r.OutOctets));
            }
            FreeMibTable(t as *const _);
        }
        self.prev = now;
        (rx, tx)
    }
}

#[cfg(test)]
mod token_tests {
    #[test]
    fn every_character_is_equally_likely() {
        // 256 isn't a multiple of the alphabet's 55 characters: the top bytes must be thrown away,
        // or the first characters come up more often than the rest
        let keep = 256 - 256 % super::TOKEN_CHARS.len();
        assert!(super::token_char((keep - 1) as u8).is_some());
        for b in keep..256 {
            assert!(super::token_char(b as u8).is_none(), "byte {b} would bias the token");
        }
        let t = super::random_token(64);
        assert_eq!(t.chars().count(), 64);
        assert!(t.bytes().all(|c| super::TOKEN_CHARS.contains(&c)));
    }
}

#[cfg(test)]
mod atomic_tests {
    /// A reader that looks while the file is being replaced sees the whole old or the whole new
    /// file, never a cut-off one (what a crash in the middle of a save would leave behind).
    #[test]
    fn a_reader_never_sees_a_half_written_file() {
        let dir = std::env::temp_dir().join(format!("deck-atomic-{}", super::now_ms()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("a.json");
        let (a, b) = (vec![b'a'; 4 << 20], vec![b'b'; 4 << 20]);
        super::write_atomic(&f, &a).unwrap();
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let reader = {
            let (f, stop) = (f.clone(), stop.clone());
            std::thread::spawn(move || {
                let mut torn = 0;
                while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                    if let Ok(d) = std::fs::read(&f) {
                        if d.len() != 4 << 20 {
                            torn += 1;
                        }
                    }
                }
                torn
            })
        };
        for i in 0..30 {
            super::write_atomic(&f, if i % 2 == 0 { &b } else { &a }).unwrap();
        }
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        assert_eq!(reader.join().unwrap(), 0, "the reader saw a cut-off file");
        let names: Vec<String> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
        assert_eq!(names, vec!["a.json".to_string()], "no temp file left over");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod as_user_tests {
    use std::cell::Cell;
    use std::time::Duration;

    /// When Explorer can't take the request (not running, restarting), the launch fails after a
    /// few tries; it never falls back to starting the target with Deck's admin rights.
    #[test]
    fn a_failing_explorer_is_retried_then_refused() {
        let tries = Cell::new(0);
        let r = super::retry_as_user(
            || {
                tries.set(tries.get() + 1);
                Err("no Explorer".into())
            },
            4,
            Duration::ZERO,
        );
        assert_eq!(tries.get(), 4);
        assert!(r.unwrap_err().contains("Explorer"));
    }

    #[test]
    fn explorer_coming_back_is_waited_for() {
        let tries = Cell::new(0);
        let r = super::retry_as_user(
            || {
                tries.set(tries.get() + 1);
                if tries.get() < 3 {
                    Err("restarting".into())
                } else {
                    Ok(())
                }
            },
            6,
            Duration::ZERO,
        );
        assert!(r.is_ok());
        assert_eq!(tries.get(), 3);
    }
}

#[cfg(test)]
mod trusted_path_tests {
    use std::path::Path;

    /// The folders come from Windows, not from environment variables the signed-in user can set
    /// (an elevated Deck inherits the user's variables).
    #[test]
    fn system_folders_ignore_the_environment() {
        let fake = std::env::temp_dir().join("deck-fake-pf86");
        let old = std::env::var_os("ProgramFiles(x86)");
        std::env::set_var("ProgramFiles(x86)", &fake);
        let pf86 = super::program_files_x86();
        match old {
            Some(v) => std::env::set_var("ProgramFiles(x86)", v),
            None => std::env::remove_var("ProgramFiles(x86)"),
        }
        assert_ne!(pf86, fake);
        for p in [super::program_files(), super::program_files_x86(), super::program_data(), super::system_dir()] {
            assert!(p.is_absolute() && p.is_dir(), "{p:?}");
        }
        assert!(super::system_dir().join("kernel32.dll").is_file());
    }

    /// Vendor libraries are looked for only in Windows' own folder (and NVIDIA's management folder
    /// under Program Files), never by a bare name that would search the current folder or PATH.
    #[test]
    fn vendor_libraries_come_only_from_trusted_folders() {
        let sys = super::system_dir();
        let nvsmi = super::program_files().join("NVIDIA Corporation").join("NVSMI");
        let c = super::trusted_dll_paths("nvml.dll");
        assert_eq!(c, vec![sys.join("nvml.dll"), nvsmi.join("nvml.dll")]);
        assert_eq!(super::trusted_dll_paths("nvapi64.dll"), vec![sys.join("nvapi64.dll")]);
        for bad in [r"..\nvml.dll", "sub/nvml.dll", r"C:\x\nvml.dll", "nvml.dll:evil", ""] {
            assert!(super::trusted_dll_paths(bad).is_empty(), "{bad} must be refused");
        }
        assert!(!super::load_trusted_dll("kernel32.dll").is_null());
        assert!(super::load_trusted_dll("deck-no-such-library.dll").is_null());
        assert!(super::load_trusted_dll(r"..\kernel32.dll").is_null());
        assert!(Path::new(&sys).is_absolute());
    }
}

#[cfg(test)]
mod poison_tests {
    use super::LockAnyway;

    /// A thread that panicked while holding a lock (a hardware worker, the sync engine) doesn't
    /// take the next user of that lock down with it (the tray menu, a page refresh).
    #[test]
    fn a_poisoned_lock_still_opens() {
        let m = std::sync::Arc::new(std::sync::Mutex::new(5));
        let m2 = m.clone();
        let _ = std::thread::spawn(move || {
            let _g = m2.lock().unwrap();
            panic!("a worker failed while holding the lock");
        })
        .join();
        assert!(m.is_poisoned());
        assert_eq!(*m.lock_anyway(), 5);
        *m.lock_anyway() = 6;
        assert_eq!(*m.lock_anyway(), 6);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_program_that_runs_too_long_is_ended() {
        let t = std::time::Instant::now();
        let r = super::run_for(1, "ping.exe", &["-n", "30", "127.0.0.1"]);
        assert_eq!(r.map(|_| ()).unwrap_err().kind(), std::io::ErrorKind::TimedOut);
        assert!(t.elapsed() < std::time::Duration::from_secs(5));
        let o = super::run_for(30, "cmd.exe", &["/c", "echo", "hi"]).unwrap();
        assert!(o.status.success() && String::from_utf8_lossy(&o.stdout).contains("hi"));
    }
}
