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

/// Starts a program, file or link the way Explorer would, **without** Deck's admin rights:
/// when Deck is elevated the request is handed to Explorer (which runs as the normal user)
/// through its shell automation object. Falls back to a plain ShellExecute.
pub fn launch_as_user(file: &str, args: &str, dir: &str) -> Result<(), String> {
    if !crate::elevation::is_admin() {
        return shell_exec(file, args, dir);
    }
    let (f, a, d) = (file.to_string(), args.to_string(), dir.to_string());
    // shell automation objects want a fresh single-threaded apartment
    let r = std::thread::spawn(move || {
        com_sta();
        unsafe { via_explorer(&f, &a, &d) }
    })
    .join()
    .unwrap_or_else(|_| Err(windows::core::Error::from_hresult(windows::Win32::Foundation::E_FAIL)));
    match r {
        Ok(()) => Ok(()),
        Err(e) => {
            eprintln!("launch_as_user via Explorer failed ({e}); starting with Deck's rights");
            shell_exec(file, args, dir)
        }
    }
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
    let _ = Command::new("explorer").arg(format!("/select,{path}")).spawn();
}

/// Runs a console program without flashing a window.
pub fn run_hidden(exe: &str, args: &[&str]) -> std::io::Result<std::process::Output> {
    use std::os::windows::process::CommandExt;
    Command::new(exe).args(args).creation_flags(CREATE_NO_WINDOW).output()
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

pub fn random_token(n: usize) -> String {
    const A: &[u8] = b"abcdefghjkmnpqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let mut buf = vec![0u8; n];
    let _ = getrandom::fill(&mut buf);
    buf.iter().map(|b| A[*b as usize % A.len()] as char).collect()
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
    #[test]
    fn free_space_is_read() {
        let free = super::free_bytes(&std::env::temp_dir()).expect("free space of the temp drive");
        assert!(free > 0);
        assert!(super::free_bytes(std::path::Path::new("Q:/no/such/drive")).is_none());
    }
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
