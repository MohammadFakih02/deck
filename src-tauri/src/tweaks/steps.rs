//! The steps a tweak is made of, each able to say whether the PC matches it, what is there now
//! (so it can be put back exactly) and how to change it.
//!
//! Registry access goes through `Reg`, which in tests (and the test profile) maps every path under
//! one scratch key (`HKCU\Software\Deck-test\tweaks…`), so nothing real is ever touched there.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use windows_sys::Win32::System::Registry::*;

use crate::util::{base64, unbase64, wide};

/// One step of a tweak's option, as written in `tweaks.json`.
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Step {
    #[serde(default)]
    pub reg: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default, rename = "type")]
    pub ty: Option<String>,
    #[serde(default)]
    pub value: Option<serde_json::Value>,
    #[serde(default)]
    pub delete: bool,
    #[serde(default)]
    pub key_delete: bool,
    #[serde(default)]
    pub key_create: bool,
    /// a dword's bits: set (or cleared) without touching the others
    #[serde(default)]
    pub bits: Option<u32>,
    #[serde(default)]
    pub set: Option<bool>,
    #[serde(default)]
    pub service: Option<String>,
    #[serde(default)]
    pub start: Option<String>,
    #[serde(default)]
    pub feature: Option<String>,
    #[serde(default)]
    pub on: Option<bool>,
    #[serde(default)]
    pub cmd: Option<String>,
    /// a key in DirectX's per-user settings string ("SwapEffectUpgradeEnable=1;")
    #[serde(default)]
    pub dx: Option<String>,
    /// DNS servers for the connection in use (empty: automatic)
    #[serde(default)]
    pub dns: Option<Vec<String>>,
}

/// What was there before a step changed it: enough to put it back byte for byte.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "k", rename_all = "snake_case")]
pub enum Before {
    /// a registry value (None: it wasn't there)
    Value { path: String, name: String, data: Option<RegData> },
    /// a whole key with everything under it (None: it wasn't there)
    Key { path: String, tree: Option<Tree> },
    /// the first key on a path that didn't exist: removed again on undo when it's left empty
    Created { path: String },
    Service { name: String, start: u32, delayed: Option<u32> },
    Feature { name: String, on: bool },
    Cmd { name: String, on: bool },
    Dns { iface: String, servers: Vec<String> },
}

impl Before {
    /// Two befores for the same thing (the journal keeps the first one Deck saw).
    pub fn same_target(&self, o: &Before) -> bool {
        use Before::*;
        match (self, o) {
            (Value { path: a, name: n, .. }, Value { path: b, name: m, .. }) => a.eq_ignore_ascii_case(b) && n.eq_ignore_ascii_case(m),
            (Key { path: a, .. }, Key { path: b, .. }) | (Created { path: a }, Created { path: b }) => a.eq_ignore_ascii_case(b),
            (Service { name: a, .. }, Service { name: b, .. }) | (Feature { name: a, .. }, Feature { name: b, .. }) | (Cmd { name: a, .. }, Cmd { name: b, .. }) => a.eq_ignore_ascii_case(b),
            (Dns { iface: a, .. }, Dns { iface: b, .. }) => a == b,
            _ => false,
        }
    }
}

/// A registry value as stored: its type and raw bytes (base64).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct RegData {
    pub ty: u32,
    pub data: String,
}

impl RegData {
    fn bytes(&self) -> Vec<u8> {
        unbase64(&self.data).unwrap_or_default()
    }
    fn dword(&self) -> Option<u32> {
        let b = self.bytes();
        (self.ty == REG_DWORD && b.len() >= 4).then(|| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn text(&self) -> Option<String> {
        if self.ty != REG_SZ && self.ty != REG_EXPAND_SZ {
            return None;
        }
        let b = self.bytes();
        let u: Vec<u16> = b.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).take_while(|c| *c != 0).collect();
        Some(String::from_utf16_lossy(&u))
    }
}

/// A key and everything under it.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Tree {
    pub values: Vec<(String, RegData)>,
    pub keys: Vec<(String, Tree)>,
}

// ------------------------------------------------------------------------------------- registry

static SCRATCH: AtomicBool = AtomicBool::new(false);
thread_local! {
    static SCRATCH_BASE: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// Tests: every path lives under `base` in HKCU instead (this thread only).
#[cfg(test)]
pub fn use_scratch(base: &str) {
    SCRATCH.store(true, Ordering::SeqCst);
    SCRATCH_BASE.with(|b| *b.borrow_mut() = Some(base.to_string()));
}

fn scratch_base() -> Option<String> {
    if crate::settings::is_test_profile() {
        return Some(r"Software\Deck-test\tweaks".into());
    }
    if !SCRATCH.load(Ordering::SeqCst) {
        return None;
    }
    SCRATCH_BASE.with(|b| b.borrow().clone())
}

/// (root, sub key) for a "HKCU\…" path.
fn split(path: &str) -> Option<(HKEY, String)> {
    let (root, rest) = path.split_once('\\').unwrap_or((path, ""));
    if let Some(base) = scratch_base() {
        let r = root.to_ascii_uppercase();
        return Some((HKEY_CURRENT_USER, if rest.is_empty() { format!(r"{base}\{r}") } else { format!(r"{base}\{r}\{rest}") }));
    }
    let h = match root.to_ascii_uppercase().as_str() {
        "HKCU" | "HKEY_CURRENT_USER" => HKEY_CURRENT_USER,
        "HKLM" | "HKEY_LOCAL_MACHINE" => HKEY_LOCAL_MACHINE,
        "HKCR" | "HKEY_CLASSES_ROOT" => HKEY_CLASSES_ROOT,
        "HKU" | "HKEY_USERS" => HKEY_USERS,
        _ => return None,
    };
    Some((h, rest.to_string()))
}

struct Key(HKEY);
impl Drop for Key {
    fn drop(&mut self) {
        unsafe { RegCloseKey(self.0) };
    }
}

fn open(path: &str, access: u32) -> Option<Key> {
    let (root, sub) = split(path)?;
    let mut h: HKEY = std::ptr::null_mut();
    (unsafe { RegOpenKeyExW(root, wide(&sub).as_ptr(), 0, access | KEY_WOW64_64KEY, &mut h) } == 0).then_some(Key(h))
}

fn err(code: u32, what: &str) -> String {
    if code == 5 {
        format!("Windows refused to change {what} (it protects this setting, or Deck needs admin rights)")
    } else {
        format!("Couldn't change {what} (error {code})")
    }
}

pub fn key_exists(path: &str) -> bool {
    open(path, KEY_READ).is_some()
}

pub fn read_value(path: &str, name: &str) -> Option<RegData> {
    let k = open(path, KEY_READ)?;
    let n = wide(name);
    let (mut ty, mut len) = (0u32, 0u32);
    if unsafe { RegQueryValueExW(k.0, n.as_ptr(), std::ptr::null(), &mut ty, std::ptr::null_mut(), &mut len) } != 0 {
        return None;
    }
    let mut buf = vec![0u8; len as usize];
    if unsafe { RegQueryValueExW(k.0, n.as_ptr(), std::ptr::null(), &mut ty, buf.as_mut_ptr(), &mut len) } != 0 {
        return None;
    }
    buf.truncate(len as usize);
    Some(RegData { ty, data: base64(&buf) })
}

/// The first key on `path` that doesn't exist yet (None: it all exists).
fn first_missing(path: &str) -> Option<String> {
    let parts: Vec<&str> = path.split('\\').collect();
    (2..=parts.len()).map(|i| parts[..i].join("\\")).find(|p| !key_exists(p))
}

fn create(path: &str) -> Result<Key, String> {
    let (root, sub) = split(path).ok_or("bad registry path")?;
    let mut h: HKEY = std::ptr::null_mut();
    let r = unsafe { RegCreateKeyExW(root, wide(&sub).as_ptr(), 0, std::ptr::null(), 0, KEY_ALL_ACCESS | KEY_WOW64_64KEY, std::ptr::null(), &mut h, std::ptr::null_mut()) };
    if r != 0 {
        return Err(err(r, path));
    }
    Ok(Key(h))
}

pub fn write_value(path: &str, name: &str, d: &RegData) -> Result<(), String> {
    let k = create(path)?;
    let b = d.bytes();
    let r = unsafe { RegSetValueExW(k.0, wide(name).as_ptr(), 0, d.ty, b.as_ptr(), b.len() as u32) };
    if r != 0 {
        return Err(err(r, &format!("{path}\\{name}")));
    }
    Ok(())
}

pub fn delete_value(path: &str, name: &str) -> Result<(), String> {
    let Some(k) = open(path, KEY_SET_VALUE) else { return Ok(()) };
    let r = unsafe { RegDeleteValueW(k.0, wide(name).as_ptr()) };
    if r != 0 && r != 2 {
        return Err(err(r, &format!("{path}\\{name}")));
    }
    Ok(())
}

pub fn delete_key(path: &str) -> Result<(), String> {
    let (root, sub) = split(path).ok_or("bad registry path")?;
    let (parent, leaf) = sub.rsplit_once('\\').unwrap_or(("", &sub));
    let mut h: HKEY = std::ptr::null_mut();
    if unsafe { RegOpenKeyExW(root, wide(parent).as_ptr(), 0, KEY_ALL_ACCESS | KEY_WOW64_64KEY, &mut h) } != 0 {
        return Ok(());
    }
    let p = Key(h);
    let r = unsafe { RegDeleteTreeW(p.0, wide(leaf).as_ptr()) };
    if r != 0 && r != 2 {
        return Err(err(r, path));
    }
    let r = unsafe { RegDeleteKeyExW(p.0, wide(leaf).as_ptr(), KEY_WOW64_64KEY, 0) };
    if r != 0 && r != 2 {
        return Err(err(r, path));
    }
    Ok(())
}

fn enum_names(k: &Key, values: bool) -> Vec<String> {
    let mut out = Vec::new();
    for i in 0.. {
        let mut buf = vec![0u16; 16384];
        let mut len = buf.len() as u32;
        let r = unsafe {
            if values {
                RegEnumValueW(k.0, i, buf.as_mut_ptr(), &mut len, std::ptr::null(), std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut())
            } else {
                RegEnumKeyExW(k.0, i, buf.as_mut_ptr(), &mut len, std::ptr::null(), std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut())
            }
        };
        if r != 0 {
            break;
        }
        out.push(String::from_utf16_lossy(&buf[..len as usize]));
    }
    out
}

/// A key and everything under it (at most `budget` values, so a huge key can't fill the journal).
pub fn export(path: &str, budget: &mut usize) -> Option<Tree> {
    let k = open(path, KEY_READ)?;
    let mut t = Tree { values: Vec::new(), keys: Vec::new() };
    for n in enum_names(&k, true) {
        if *budget == 0 {
            return None;
        }
        *budget -= 1;
        if let Some(d) = read_value(path, &n) {
            t.values.push((n, d));
        }
    }
    for n in enum_names(&k, false) {
        t.keys.push((n.clone(), export(&format!("{path}\\{n}"), budget)?));
    }
    Some(t)
}

pub fn import(path: &str, t: &Tree) -> Result<(), String> {
    drop(create(path)?);
    for (n, d) in &t.values {
        write_value(path, n, d)?;
    }
    for (n, sub) in &t.keys {
        import(&format!("{path}\\{n}"), sub)?;
    }
    Ok(())
}

/// Removes `path` and its parents up to `top` while they hold nothing (keys Deck created).
fn remove_if_empty(path: &str, top: &str) {
    let mut p = path.to_string();
    loop {
        let empty = open(&p, KEY_READ).map(|k| enum_names(&k, true).is_empty() && enum_names(&k, false).is_empty()).unwrap_or(false);
        if !empty {
            return;
        }
        let _ = delete_key(&p);
        if p.eq_ignore_ascii_case(top) {
            return;
        }
        match p.rsplit_once('\\') {
            Some((parent, _)) if parent.len() >= top.len() => p = parent.to_string(),
            _ => return,
        }
    }
}

// ------------------------------------------------------------------------------------- values

fn wanted_data(s: &Step) -> Option<RegData> {
    let v = s.value.as_ref()?;
    let ty = s.ty.as_deref().unwrap_or("dword");
    let bytes: Vec<u8> = match ty {
        "dword" => (v.as_u64()? as u32).to_le_bytes().to_vec(),
        "qword" => v.as_u64()?.to_le_bytes().to_vec(),
        "string" | "expand" => wide(v.as_str()?).iter().flat_map(|c| c.to_le_bytes()).collect(),
        "multi" => {
            let mut w: Vec<u16> = Vec::new();
            for s in v.as_array()? {
                w.extend(s.as_str()?.encode_utf16());
                w.push(0);
            }
            w.push(0);
            w.iter().flat_map(|c| c.to_le_bytes()).collect()
        }
        "binary" => {
            let h = v.as_str()?;
            (0..h.len() / 2).map(|i| u8::from_str_radix(&h[i * 2..i * 2 + 2], 16).unwrap_or(0)).collect()
        }
        _ => return None,
    };
    let ty = match ty {
        "dword" => REG_DWORD,
        "qword" => REG_QWORD,
        "string" => REG_SZ,
        "expand" => REG_EXPAND_SZ,
        "multi" => REG_MULTI_SZ,
        _ => REG_BINARY,
    };
    Some(RegData { ty, data: base64(&bytes) })
}

/// A number kept as a dword or as text ("506").
fn number(d: &RegData) -> Option<u64> {
    d.dword().map(u64::from).or_else(|| d.text().and_then(|t| t.trim().parse().ok()))
}

/// Equal as Windows reads them: numbers by value, text without its end zero and case.
fn same(have: &RegData, want: &RegData) -> bool {
    if let (Some(a), Some(b)) = (have.dword(), want.dword()) {
        return a == b;
    }
    if let (Some(a), Some(b)) = (have.text(), want.text()) {
        return a.eq_ignore_ascii_case(&b);
    }
    have.ty == want.ty && have.bytes() == want.bytes()
}

const DX_KEY: &str = r"HKCU\Software\Microsoft\DirectX\UserGpuPreferences";
const DX_NAME: &str = "DirectXUserGlobalSettings";

fn dx_pairs() -> Vec<(String, String)> {
    let s = read_value(DX_KEY, DX_NAME).and_then(|d| d.text()).unwrap_or_default();
    s.split(';').filter_map(|p| p.split_once('=')).map(|(k, v)| (k.trim().to_string(), v.trim().to_string())).filter(|(k, _)| !k.is_empty()).collect()
}

fn dx_write(pairs: &[(String, String)]) -> Result<(), String> {
    if pairs.is_empty() {
        return delete_value(DX_KEY, DX_NAME);
    }
    let s: String = pairs.iter().map(|(k, v)| format!("{k}={v};")).collect();
    write_value(DX_KEY, DX_NAME, &RegData { ty: REG_SZ, data: base64(&wide(&s).iter().flat_map(|c| c.to_le_bytes()).collect::<Vec<u8>>()) })
}

// ------------------------------------------------------------------------------------- the slow kinds

/// Services: their start type from the registry (2 automatic, 3 manual, 4 disabled).
fn service_start(name: &str) -> Option<(u32, Option<u32>)> {
    let p = format!(r"HKLM\SYSTEM\CurrentControlSet\Services\{name}");
    let start = read_value(&p, "Start")?.dword()?;
    Some((start, read_value(&p, "DelayedAutostart").and_then(|d| d.dword())))
}

fn set_service(name: &str, start: u32, delayed: Option<u32>) -> Result<(), String> {
    if scratch_base().is_some() {
        let p = format!(r"HKLM\SYSTEM\CurrentControlSet\Services\{name}");
        write_value(&p, "Start", &RegData { ty: REG_DWORD, data: base64(&start.to_le_bytes()) })?;
        return Ok(());
    }
    let mode = match (start, delayed) {
        (2, Some(1)) => "delayed-auto",
        (2, _) => "auto",
        (3, _) => "demand",
        _ => "disabled",
    };
    let o = crate::tweaks::run(60, "sc.exe", &["config", name, "start=", mode]).map_err(|e| e.to_string())?;
    if !o.status.success() {
        return Err(format!("Windows didn't change the {name} service: {}", String::from_utf8_lossy(&o.stdout).trim()));
    }
    if start == 4 {
        let _ = crate::tweaks::run(60, "sc.exe", &["stop", name]);
    }
    Ok(())
}

/// Windows optional features (DISM's English output, the same on every language of Windows).
pub fn feature_on(name: &str) -> Option<bool> {
    if scratch_base().is_some() {
        return read_value(r"HKLM\Features", name).and_then(|d| d.dword()).map(|v| v == 1).or(Some(false));
    }
    let o = crate::tweaks::run(60, "dism.exe", &["/online", "/English", "/get-featureinfo", &format!("/featurename:{name}")]).ok()?;
    let t = String::from_utf8_lossy(&o.stdout);
    let state = t.lines().find(|l| l.trim_start().starts_with("State :"))?.split(':').nth(1)?.trim().to_string();
    Some(state.starts_with("Enabled") || state.contains("Enable Pending"))
}

fn set_feature(name: &str, on: bool) -> Result<(), String> {
    if scratch_base().is_some() {
        return write_value(r"HKLM\Features", name, &RegData { ty: REG_DWORD, data: base64(&(on as u32).to_le_bytes()) });
    }
    let fname = format!("/featurename:{name}");
    let args: Vec<&str> = if on { vec!["/online", "/English", "/enable-feature", &fname, "/all", "/norestart"] } else { vec!["/online", "/English", "/disable-feature", &fname, "/norestart"] };
    let o = crate::tweaks::run(900, "dism.exe", &args).map_err(|e| e.to_string())?;
    // 3010: done, needs a restart
    match o.status.code() {
        Some(0) | Some(3010) => Ok(()),
        _ => Err(format!("Windows couldn't turn {name} {}: {}", if on { "on" } else { "off" }, String::from_utf8_lossy(&o.stdout).lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("").trim())),
    }
}

/// The fixed commands Deck knows (nothing else is ever run).
pub fn cmd_on(name: &str) -> Option<bool> {
    if scratch_base().is_some() {
        return read_value(r"HKLM\Cmd", name).and_then(|d| d.dword()).map(|v| v == 1).or(Some(true));
    }
    match name {
        "hibernate" => read_value(r"HKLM\SYSTEM\CurrentControlSet\Control\Power", "HibernateEnabled").and_then(|d| d.dword()).map(|v| v == 1),
        "reserved_storage" => {
            let o = crate::tweaks::run(60, "dism.exe", &["/online", "/English", "/Get-ReservedStorageState"]).ok()?;
            let t = String::from_utf8_lossy(&o.stdout).to_lowercase();
            if t.contains("reserved storage is enabled") {
                Some(true)
            } else if t.contains("reserved storage is disabled") {
                Some(false)
            } else {
                None
            }
        }
        _ => None,
    }
}

fn set_cmd(name: &str, on: bool) -> Result<(), String> {
    if scratch_base().is_some() {
        return write_value(r"HKLM\Cmd", name, &RegData { ty: REG_DWORD, data: base64(&(on as u32).to_le_bytes()) });
    }
    let o = match name {
        "hibernate" => crate::tweaks::run(60, "powercfg.exe", &["/hibernate", if on { "on" } else { "off" }]),
        "reserved_storage" => crate::tweaks::run(600, "dism.exe", &["/online", "/English", "/Set-ReservedStorageState", if on { "/State:Enabled" } else { "/State:Disabled" }]),
        _ => return Err(format!("unknown command {name}")),
    }
    .map_err(|e| e.to_string())?;
    if !o.status.success() {
        return Err(format!("Windows didn't change it: {}", String::from_utf8_lossy(&o.stdout).trim()));
    }
    Ok(())
}

/// The connection in use: its adapter GUID and interface index (the one with the default route).
pub fn dns_iface() -> Option<(String, u32)> {
    if scratch_base().is_some() {
        return Some(("{test}".into(), 1));
    }
    use windows_sys::Win32::NetworkManagement::IpHelper::GetBestInterface;
    let mut idx = 0u32;
    // the interface the PC would use to reach the internet
    if unsafe { GetBestInterface(u32::from_ne_bytes([1, 1, 1, 1]), &mut idx) } != 0 {
        return None;
    }
    let o = crate::tweaks::run(60, "powershell.exe", &["-NoProfile", "-NonInteractive", "-Command", &format!("(Get-NetAdapter -InterfaceIndex {idx}).InterfaceGuid")]).ok()?;
    let g = String::from_utf8_lossy(&o.stdout).trim().to_string();
    g.starts_with('{').then_some((g, idx))
}

/// The DNS servers set by hand on the interface (empty: automatic).
pub fn dns_servers(iface: &str) -> Vec<String> {
    let p = format!(r"HKLM\SYSTEM\CurrentControlSet\Services\Tcpip\Parameters\Interfaces\{iface}");
    let s = read_value(&p, "NameServer").and_then(|d| d.text()).unwrap_or_default();
    s.split([',', ' ']).filter(|x| !x.is_empty()).map(String::from).collect()
}

fn set_dns(iface: &str, idx: u32, servers: &[String]) -> Result<(), String> {
    if servers.iter().any(|s| s.parse::<std::net::IpAddr>().is_err()) {
        return Err("bad DNS address".into());
    }
    if scratch_base().is_some() {
        let p = format!(r"HKLM\SYSTEM\CurrentControlSet\Services\Tcpip\Parameters\Interfaces\{iface}");
        return write_value(&p, "NameServer", &RegData { ty: REG_SZ, data: base64(&wide(&servers.join(",")).iter().flat_map(|c| c.to_le_bytes()).collect::<Vec<u8>>()) });
    }
    let cmd = if servers.is_empty() {
        format!("Set-DnsClientServerAddress -InterfaceIndex {idx} -ResetServerAddresses")
    } else {
        format!("Set-DnsClientServerAddress -InterfaceIndex {idx} -ServerAddresses ({})", servers.iter().map(|s| format!("'{s}'")).collect::<Vec<_>>().join(","))
    };
    let o = crate::tweaks::run(60, "powershell.exe", &["-NoProfile", "-NonInteractive", "-Command", &cmd]).map_err(|e| e.to_string())?;
    if !o.status.success() {
        return Err(format!("Windows didn't change the DNS servers: {}", String::from_utf8_lossy(&o.stderr).trim()));
    }
    Ok(())
}

// ------------------------------------------------------------------------------------- one step

/// Whether the PC matches `s`. `absent_ok`: a value that isn't there counts (Windows' default).
/// None: it can't be told (a slow kind not read yet, or Windows didn't answer).
pub fn matches(s: &Step, absent_ok: bool) -> Option<bool> {
    if let Some(path) = &s.reg {
        if s.key_delete {
            return Some(!key_exists(path));
        }
        if s.key_create {
            return Some(key_exists(path));
        }
        let name = s.name.as_deref().unwrap_or("");
        let have = read_value(path, name);
        if s.delete {
            return Some(have.is_none());
        }
        if let Some(mask) = s.bits {
            let v = have.as_ref().and_then(number).unwrap_or(0);
            return Some((v & mask as u64 != 0) == s.set.unwrap_or(true));
        }
        let want = wanted_data(s)?;
        return Some(match have {
            Some(h) => same(&h, &want),
            None => absent_ok,
        });
    }
    if let Some(k) = &s.dx {
        let have = dx_pairs().into_iter().find(|(n, _)| n.eq_ignore_ascii_case(k)).map(|(_, v)| v);
        let want = s.value.as_ref().and_then(|v| v.as_str()).map(String::from);
        return Some(match (have, want) {
            (None, None) => true,
            (Some(h), Some(w)) => h == w,
            (None, Some(_)) => absent_ok,
            (Some(_), None) => false,
        });
    }
    if let Some(name) = &s.service {
        let (start, delayed) = service_start(name)?;
        let want = match s.start.as_deref() {
            Some("auto") => 2,
            Some("manual") => 3,
            _ => 4,
        };
        let _ = delayed;
        return Some(start == want);
    }
    if let Some(name) = &s.feature {
        return Some(super::slow(&format!("feature:{name}"))? == s.on.unwrap_or(true));
    }
    if let Some(name) = &s.cmd {
        return Some(super::slow(&format!("cmd:{name}"))? == s.on.unwrap_or(true));
    }
    if let Some(servers) = &s.dns {
        let (iface, _) = super::iface()?;
        let have = dns_servers(&iface);
        return Some(have.len() >= servers.len() && have[..servers.len()].iter().zip(servers).all(|(a, b)| a == b) && (servers.is_empty() == have.is_empty()));
    }
    None
}

/// What is there now for `s` (before changing it).
pub fn before(s: &Step) -> Vec<Before> {
    let mut out = Vec::new();
    if let Some(path) = &s.reg {
        if s.key_delete || s.key_create {
            let mut budget = 2000;
            out.push(Before::Key { path: path.clone(), tree: export(path, &mut budget) });
            if s.key_create {
                if let Some(p) = first_missing(path) {
                    out.push(Before::Created { path: p });
                }
            }
            return out;
        }
        let name = s.name.clone().unwrap_or_default();
        if !s.delete {
            if let Some(p) = first_missing(path) {
                out.push(Before::Created { path: p });
            }
        }
        out.push(Before::Value { path: path.clone(), data: read_value(path, &name), name });
        return out;
    }
    if s.dx.is_some() {
        out.push(Before::Value { path: DX_KEY.into(), name: DX_NAME.into(), data: read_value(DX_KEY, DX_NAME) });
    } else if let Some(name) = &s.service {
        if let Some((start, delayed)) = service_start(name) {
            out.push(Before::Service { name: name.clone(), start, delayed });
        }
    } else if let Some(name) = &s.feature {
        if let Some(on) = feature_on(name) {
            out.push(Before::Feature { name: name.clone(), on });
        }
    } else if let Some(name) = &s.cmd {
        if let Some(on) = cmd_on(name) {
            out.push(Before::Cmd { name: name.clone(), on });
        }
    } else if s.dns.is_some() {
        if let Some((iface, _)) = super::iface() {
            out.push(Before::Dns { servers: dns_servers(&iface), iface });
        }
    }
    out
}

/// Makes the PC match `s`.
pub fn apply(s: &Step) -> Result<(), String> {
    if let Some(path) = &s.reg {
        if s.key_delete {
            return delete_key(path);
        }
        if s.key_create {
            return create(path).map(drop);
        }
        let name = s.name.as_deref().unwrap_or("");
        if s.delete {
            return delete_value(path, name);
        }
        if let Some(mask) = s.bits {
            let have = read_value(path, name);
            let v = have.as_ref().and_then(number).unwrap_or(0);
            let nv = if s.set.unwrap_or(true) { v | mask as u64 } else { v & !(mask as u64) };
            // the value keeps its type: Windows keeps some flags as text ("2147483650")
            let d = match have {
                Some(h) if h.ty == REG_SZ => RegData { ty: REG_SZ, data: base64(&wide(&nv.to_string()).iter().flat_map(|c| c.to_le_bytes()).collect::<Vec<u8>>()) },
                _ => RegData { ty: REG_DWORD, data: base64(&(nv as u32).to_le_bytes()) },
            };
            return write_value(path, name, &d);
        }
        let d = wanted_data(s).ok_or_else(|| format!("bad value for {path}\\{name}"))?;
        return write_value(path, name, &d);
    }
    if let Some(k) = &s.dx {
        let mut pairs: Vec<(String, String)> = dx_pairs().into_iter().filter(|(n, _)| !n.eq_ignore_ascii_case(k)).collect();
        if let Some(v) = s.value.as_ref().and_then(|v| v.as_str()) {
            pairs.push((k.clone(), v.to_string()));
        }
        return dx_write(&pairs);
    }
    if let Some(name) = &s.service {
        let start = match s.start.as_deref() {
            Some("auto") => 2,
            Some("manual") => 3,
            _ => 4,
        };
        return set_service(name, start, None);
    }
    if let Some(name) = &s.feature {
        let r = set_feature(name, s.on.unwrap_or(true));
        super::forget_slow(&format!("feature:{name}"));
        return r;
    }
    if let Some(name) = &s.cmd {
        let r = set_cmd(name, s.on.unwrap_or(true));
        super::forget_slow(&format!("cmd:{name}"));
        return r;
    }
    if let Some(servers) = &s.dns {
        let (iface, idx) = super::iface().ok_or("No network connection to set DNS servers for")?;
        return set_dns(&iface, idx, servers);
    }
    Err("unknown step".into())
}

/// Puts one thing back as it was.
pub fn restore(b: &Before) -> Result<(), String> {
    match b {
        Before::Value { path, name, data } => match data {
            Some(d) => write_value(path, name, d),
            None => delete_value(path, name),
        },
        Before::Key { path, tree } => match tree {
            Some(t) => {
                delete_key(path)?;
                import(path, t)
            }
            None => delete_key(path),
        },
        Before::Created { path } => {
            remove_if_empty(path, path);
            // its sub keys Deck made empty: remove them bottom-up, then the key itself
            fn prune(p: &str) {
                if let Some(k) = open(p, KEY_READ) {
                    for n in enum_names(&k, false) {
                        prune(&format!("{p}\\{n}"));
                    }
                    drop(k);
                    remove_if_empty(p, p);
                }
            }
            prune(path);
            Ok(())
        }
        Before::Service { name, start, delayed } => set_service(name, *start, *delayed),
        Before::Feature { name, on } => {
            let r = set_feature(name, *on);
            super::forget_slow(&format!("feature:{name}"));
            r
        }
        Before::Cmd { name, on } => {
            let r = set_cmd(name, *on);
            super::forget_slow(&format!("cmd:{name}"));
            r
        }
        Before::Dns { iface, servers } => {
            let idx = super::iface().filter(|(g, _)| g == iface).map(|(_, i)| i).ok_or("That network connection isn't in use any more")?;
            set_dns(iface, idx, servers)
        }
    }
}

/// Whether a step needs administrator rights.
pub fn needs_admin(s: &Step) -> bool {
    match &s.reg {
        Some(p) => {
            let up = p.to_ascii_uppercase();
            !up.starts_with("HKCU") || up.contains(r"\POLICIES\")
        }
        None => s.dx.is_none(),
    }
}

/// Whether a step changes the signed-in user's own settings (HKCU).
pub fn per_user(s: &Step) -> bool {
    s.reg.as_deref().is_some_and(|p| p.to_ascii_uppercase().starts_with("HKCU")) || s.dx.is_some()
}
