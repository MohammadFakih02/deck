//! Maintenance → Apps: every installed program (what Windows' "Installed apps" lists: the
//! registry's Uninstall entries, plus your Store apps), with two ways out:
//!
//! - Uninstall: the program's own uninstaller, as Windows runs it, and Deck waits until it and
//!   anything it started (Inno and NSIS copy themselves to Temp first) have finished.
//! - Advanced: the same, then a search for what it left behind (its folders, its registry keys,
//!   its Start menu shortcuts). Only what the search found can be removed, and only what you tick:
//!   folders and files go to the Recycle Bin, registry keys are saved to a .reg file first.
//!   A program whose uninstaller is gone can still have its entry and leftovers removed.
//!
//! What counts as a leftover is deliberately narrow: a name has to match the program's own name
//! (or its install folder's) exactly, after dropping versions and punctuation, and nothing that
//! another installed program still uses is offered.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use windows::core::{HSTRING, PWSTR};
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::{
    RegCloseKey, RegEnumKeyExW, RegGetValueW, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY,
    KEY_WOW64_64KEY, REG_ROUTINE_FLAGS, REG_SAM_FLAGS, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RRF_SUBKEY_WOW6432KEY, RRF_SUBKEY_WOW6464KEY,
};

const UNINSTALL: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall";

/// One installed program.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Program {
    /// "m64:<key>", "m32:<key>", "u:<key>" (the Uninstall entry) or "appx:<package full name>"
    pub id: String,
    pub name: String,
    pub publisher: String,
    pub version: String,
    /// bytes, as the program reported it (Store apps: their folder)
    pub size: Option<u64>,
    /// "YYYY-MM-DD"
    pub date: Option<String>,
    pub location: Option<String>,
    /// a file to take the icon from
    pub icon: Option<String>,
    pub store: bool,
    /// for every user (needs admin rights to remove) or only yours
    pub all_users: bool,
    /// it has an uninstaller and that uninstaller is on disk
    pub can_uninstall: bool,
    /// the entry is there but its uninstaller is gone
    pub broken: bool,
    /// known to Windows Installer but missing from Windows' list of programs (an old cleanup tool
    /// or a half-finished uninstall removed the entry); the classic Programs and Features still finds these
    #[serde(default)]
    pub unlisted: bool,
    /// its install folder is gone (a half-removed program: Advanced clears what's left)
    #[serde(default)]
    pub files_gone: bool,
    #[serde(skip)]
    uninstall: String,
    #[serde(skip)]
    family: String,
}

// ------------------------------------------------------------------------------------- registry

#[derive(Clone, Copy, PartialEq, Debug)]
enum View {
    M64,
    M32,
    User,
}

impl View {
    fn root(self) -> HKEY {
        if self == View::User {
            HKEY_CURRENT_USER
        } else {
            HKEY_LOCAL_MACHINE
        }
    }
    fn sam(self) -> REG_SAM_FLAGS {
        if self == View::M32 {
            KEY_WOW64_32KEY
        } else {
            KEY_WOW64_64KEY
        }
    }
    fn rrf(self) -> REG_ROUTINE_FLAGS {
        if self == View::M32 {
            RRF_SUBKEY_WOW6432KEY
        } else {
            RRF_SUBKEY_WOW6464KEY
        }
    }
    fn tag(self) -> &'static str {
        match self {
            View::M64 => "m64",
            View::M32 => "m32",
            View::User => "u",
        }
    }
    fn of(tag: &str) -> Option<View> {
        match tag {
            "m64" => Some(View::M64),
            "m32" => Some(View::M32),
            "u" => Some(View::User),
            _ => None,
        }
    }
    /// The root as `reg.exe` writes it, and the view switch it needs.
    fn reg_exe(self) -> (&'static str, &'static str) {
        match self {
            View::M64 => ("HKLM", "/reg:64"),
            View::M32 => ("HKLM", "/reg:32"),
            View::User => ("HKCU", "/reg:64"),
        }
    }
}

fn subkeys(view: View, path: &str) -> Vec<String> {
    let mut out = Vec::new();
    unsafe {
        let mut k = HKEY::default();
        if RegOpenKeyExW(view.root(), &HSTRING::from(path), None, KEY_READ | view.sam(), &mut k) != ERROR_SUCCESS {
            return out;
        }
        let mut i = 0u32;
        loop {
            let mut name = vec![0u16; 512];
            let mut len = name.len() as u32;
            if RegEnumKeyExW(k, i, Some(PWSTR(name.as_mut_ptr())), &mut len, None, None, None, None) != ERROR_SUCCESS {
                break;
            }
            out.push(String::from_utf16_lossy(&name[..len as usize]));
            i += 1;
        }
        let _ = RegCloseKey(k);
    }
    out
}

fn key_exists(view: View, path: &str) -> bool {
    unsafe {
        let mut k = HKEY::default();
        let ok = RegOpenKeyExW(view.root(), &HSTRING::from(path), None, KEY_READ | view.sam(), &mut k) == ERROR_SUCCESS;
        if ok {
            let _ = RegCloseKey(k);
        }
        ok
    }
}

fn get_str(view: View, path: &str, value: &str) -> Option<String> {
    let mut buf = vec![0u16; 2048];
    let mut len = (buf.len() * 2) as u32;
    let r = unsafe { RegGetValueW(view.root(), &HSTRING::from(path), &HSTRING::from(value), RRF_RT_REG_SZ | view.rrf(), None, Some(buf.as_mut_ptr() as *mut _), Some(&mut len)) };
    if r != ERROR_SUCCESS {
        return None;
    }
    let s = String::from_utf16_lossy(&buf[..(len as usize / 2).saturating_sub(1)]).trim().to_string();
    (!s.is_empty()).then_some(s)
}

fn get_dword(view: View, path: &str, value: &str) -> Option<u32> {
    let mut v = 0u32;
    let mut len = 4u32;
    let r = unsafe { RegGetValueW(view.root(), &HSTRING::from(path), &HSTRING::from(value), RRF_RT_REG_DWORD | view.rrf(), None, Some(&mut v as *mut u32 as *mut _), Some(&mut len)) };
    (r == ERROR_SUCCESS).then_some(v)
}

// ------------------------------------------------------------------------------------- the list

/// The program file a command line starts: "C:\x\unins000.exe" /SILENT, C:\Program Files\x\u.exe /S,
/// MsiExec.exe /X{...}, rundll32 ... (a path with spaces may come unquoted).
fn exe_of(cmd: &str) -> Option<PathBuf> {
    let cmd = cmd.trim();
    if let Some(rest) = cmd.strip_prefix('"') {
        return rest.split('"').next().map(|s| PathBuf::from(expand(s)));
    }
    let lower = cmd.to_lowercase();
    // the shortest prefix that ends in .exe and exists
    let mut from = 0;
    while let Some(i) = lower[from..].find(".exe") {
        let end = from + i + 4;
        let p = PathBuf::from(expand(&cmd[..end]));
        if p.is_file() || !p.is_absolute() {
            return Some(p);
        }
        from = end;
    }
    cmd.split_whitespace().next().map(|s| PathBuf::from(expand(s)))
}

fn expand(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find('%') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        match after.find('%') {
            Some(j) => {
                let name = &after[..j];
                match std::env::var(name) {
                    Ok(v) if !name.is_empty() => out.push_str(&v),
                    _ => {
                        out.push('%');
                        out.push_str(name);
                        out.push('%');
                    }
                }
                rest = &after[j + 1..];
            }
            None => {
                out.push('%');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// A command's program is present (MsiExec and other programs on PATH count as present).
fn runnable(cmd: &str) -> bool {
    match exe_of(cmd) {
        Some(p) if p.is_absolute() => p.is_file(),
        Some(_) => true,
        None => false,
    }
}

fn desktop(view: View, key: &str) -> Option<Program> {
    let path = format!(r"{UNINSTALL}\{key}");
    let name = get_str(view, &path, "DisplayName")?;
    // parts of Windows and of other programs, and updates, aren't programs of their own
    if get_dword(view, &path, "SystemComponent") == Some(1) || get_str(view, &path, "ParentKeyName").is_some() {
        return None;
    }
    if let Some(rt) = get_str(view, &path, "ReleaseType") {
        if ["update", "hotfix", "security update", "update rollup"].contains(&rt.to_lowercase().as_str()) {
            return None;
        }
    }
    let uninstall = get_str(view, &path, "UninstallString").unwrap_or_default();
    let can = !uninstall.is_empty() && runnable(&uninstall);
    let location = get_str(view, &path, "InstallLocation").map(|l| expand(l.trim_matches('"')).trim_end_matches('\\').to_string()).filter(|l| l.len() > 3);
    let icon = get_str(view, &path, "DisplayIcon")
        .map(|i| expand(i.split(',').next().unwrap_or("").trim_matches('"')))
        .filter(|i| Path::new(i).is_file())
        .or_else(|| exe_of(&uninstall).filter(|p| p.is_file()).map(|p| p.display().to_string()));
    let date = get_str(view, &path, "InstallDate").filter(|d| d.len() == 8 && d.chars().all(|c| c.is_ascii_digit())).map(|d| format!("{}-{}-{}", &d[..4], &d[4..6], &d[6..]));
    Some(Program {
        id: format!("{}:{key}", view.tag()),
        name,
        publisher: get_str(view, &path, "Publisher").unwrap_or_default(),
        version: get_str(view, &path, "DisplayVersion").unwrap_or_default(),
        size: get_dword(view, &path, "EstimatedSize").filter(|&k| k > 0).map(|k| k as u64 * 1024),
        date,
        location,
        icon,
        store: false,
        all_users: view != View::User,
        can_uninstall: can,
        broken: !uninstall.is_empty() && !can,
        unlisted: false,
        files_gone: false,
        uninstall,
        family: String::new(),
    })
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Pkg {
    package_full_name: String,
    package_family_name: String,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    install_location: Option<String>,
    #[serde(default)]
    publisher_display_name: Option<String>,
    #[serde(default)]
    date: Option<String>,
}

/// Your Store apps that you can remove (not frameworks, not parts of Windows).
fn store_apps(names: &HashMap<String, (String, String)>) -> Vec<Program> {
    if crate::settings::is_test_profile() {
        return Vec::new();
    }
    let script = "Get-AppxPackage -PackageTypeFilter Main | Where-Object { -not $_.IsFramework -and -not $_.NonRemovable -and $_.SignatureKind -ne 'System' } | \
        ForEach-Object { [pscustomobject]@{ Name=$_.Name; PackageFullName=$_.PackageFullName; PackageFamilyName=$_.PackageFamilyName; Version=[string]$_.Version; InstallLocation=$_.InstallLocation; \
        PublisherDisplayName=$(try { (Get-AppxPackageManifest $_).Package.Properties.PublisherDisplayName } catch { $null }); \
        Date=$(try { (Get-Item -LiteralPath $_.InstallLocation -ErrorAction Stop).CreationTime.ToString('yyyy-MM-dd') } catch { $null }) } } | ConvertTo-Json -Compress";
    let Ok(o) = crate::util::run_for(90, "powershell.exe", &["-NoProfile", "-NonInteractive", "-Command", script]) else {
        *STORE_FAILED.lock() = true;
        return Vec::new();
    };
    let t = String::from_utf8_lossy(&o.stdout).to_string();
    *STORE_FAILED.lock() = !o.status.success() || t.trim().is_empty();
    let pkgs: Vec<Pkg> = serde_json::from_str::<Vec<Pkg>>(&t).ok().or_else(|| serde_json::from_str::<Pkg>(&t).ok().map(|p| vec![p])).unwrap_or_default();
    pkgs.into_iter()
        // only apps you can open: the Start menu has a name for them
        .filter_map(|p| {
            let (name, aumid) = names.get(&p.package_family_name.to_lowercase())?.clone();
            Some(Program {
                id: format!("appx:{}", p.package_full_name),
                name,
                publisher: p.publisher_display_name.unwrap_or_default(),
                version: p.version.unwrap_or_default(),
                size: None,
                date: p.date,
                location: p.install_location.clone(),
                // the Start menu's id: Deck's icon reader knows those
                icon: Some(aumid),
                store: true,
                all_users: false,
                can_uninstall: true,
                broken: false,
                unlisted: false,
                files_gone: false,
                uninstall: String::new(),
                family: p.package_family_name,
            })
        })
        .filter(|p| !p.name.is_empty() && p.name != p.family)
        .collect()
}

/// Windows Installer keeps product codes "packed": {6429D01F-...} is stored as F1D09246...
fn unpack_code(p: &str) -> Option<String> {
    if p.len() != 32 || !p.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let r = |s: &str| s.chars().rev().collect::<String>();
    let pairs = |s: &str| s.as_bytes().chunks(2).map(|c| format!("{}{}", c[1] as char, c[0] as char)).collect::<String>();
    let g = format!("{}{}{}{}", r(&p[0..8]), r(&p[8..12]), r(&p[12..16]), pairs(&p[16..32]));
    Some(format!("{{{}-{}-{}-{}-{}}}", &g[0..8], &g[8..12], &g[12..16], &g[16..20], &g[20..32]).to_uppercase())
}

/// Products Windows Installer has but Windows' list of programs doesn't (`listed`: the list's
/// key names, upper case). Parts of other programs that hide themselves stay hidden.
fn installer_only(listed: &std::collections::HashSet<String>) -> Vec<Program> {
    const UD: &str = r"Software\Microsoft\Windows\CurrentVersion\Installer\UserData";
    let mut out = Vec::new();
    for sid in subkeys(View::M64, UD) {
        let products = format!(r"{UD}\{sid}\Products");
        for packed in subkeys(View::M64, &products) {
            let Some(code) = unpack_code(&packed) else { continue };
            if listed.contains(&code) {
                continue;
            }
            let path = format!(r"{products}\{packed}\InstallProperties");
            let Some(name) = get_str(View::M64, &path, "DisplayName") else { continue };
            if get_dword(View::M64, &path, "SystemComponent") == Some(1) {
                continue;
            }
            let package = get_str(View::M64, &path, "LocalPackage").filter(|p| Path::new(p).is_file());
            let location = get_str(View::M64, &path, "InstallLocation").map(|l| expand(l.trim_matches('"')).trim_end_matches('\\').to_string()).filter(|l| l.len() > 3);
            let date = get_str(View::M64, &path, "InstallDate").filter(|d| d.len() == 8 && d.chars().all(|c| c.is_ascii_digit())).map(|d| format!("{}-{}-{}", &d[..4], &d[4..6], &d[6..]));
            let icon = get_str(View::M64, &path, "DisplayIcon").map(|i| expand(i.split(',').next().unwrap_or("").trim_matches('"'))).filter(|i| Path::new(i).is_file());
            out.push(Program {
                id: format!("msi:{code}"),
                name,
                publisher: get_str(View::M64, &path, "Publisher").unwrap_or_default(),
                version: get_str(View::M64, &path, "DisplayVersion").unwrap_or_default(),
                size: get_dword(View::M64, &path, "EstimatedSize").filter(|&k| k > 0).map(|k| k as u64 * 1024),
                date,
                files_gone: location.as_ref().is_some_and(|l| !Path::new(l).exists()),
                location,
                icon,
                store: false,
                all_users: sid == "S-1-5-18",
                // its uninstaller is Windows Installer's saved copy of the package
                can_uninstall: package.is_some(),
                broken: package.is_none(),
                unlisted: true,
                uninstall: format!("MsiExec.exe /X{code}"),
                family: String::new(),
            });
        }
    }
    out
}

static CACHE: parking_lot::Mutex<Option<(Instant, Vec<Program>)>> = parking_lot::const_mutex(None);
/// Windows didn't list the Store apps the last time (they're missing from the list, not gone)
static STORE_FAILED: parking_lot::Mutex<bool> = parking_lot::const_mutex(false);

/// Every installed program, by name (read again after 20 s or when `force`).
pub fn list(app: &crate::app::App, force: bool) -> Vec<Program> {
    if !force {
        if let Some((t, v)) = CACHE.lock().as_ref() {
            if t.elapsed() < Duration::from_secs(20) {
                return v.clone();
            }
        }
    }
    let mut out: Vec<Program> = Vec::new();
    let mut listed = std::collections::HashSet::new();
    for view in [View::M64, View::M32, View::User] {
        for key in subkeys(view, UNINSTALL) {
            listed.insert(key.to_uppercase());
            if let Some(p) = desktop(view, &key) {
                out.push(p);
            }
        }
    }
    out.extend(installer_only(&listed));
    // a program registered twice (both views, or per user and for all) shows once
    let mut seen = std::collections::HashSet::new();
    out.retain(|p| seen.insert((p.name.to_lowercase(), p.version.to_lowercase())));
    // Store apps: their names as the Start menu shows them ("Family!App" ids)
    let names: HashMap<String, (String, String)> = app
        .apps
        .list(false)
        .into_iter()
        .filter(|a| a.store)
        .filter_map(|a| a.id.split_once('!').map(|(fam, _)| (fam.to_lowercase(), (a.name.clone(), a.id.clone()))))
        .collect();
    out.extend(store_apps(&names));
    out.sort_by_key(|p| p.name.to_lowercase());
    *CACHE.lock() = Some((Instant::now(), out.clone()));
    out
}

fn find(app: &crate::app::App, id: &str) -> Result<Program, String> {
    list(app, false).into_iter().find(|p| p.id == id).or_else(|| list(app, true).into_iter().find(|p| p.id == id)).ok_or_else(|| "That program isn't installed any more".into())
}

// ------------------------------------------------------------------------------------- uninstalling

/// What the program looked like before it was uninstalled, for the leftovers search afterwards
/// (its entry, and with it its name and folder, may be gone by then).
static BEFORE: parking_lot::Mutex<Option<Program>> = parking_lot::const_mutex(None);

fn pending_file() -> PathBuf {
    crate::util::appdata().join("Deck").join("uninstall-pending.json")
}

/// Remembers the program an advanced uninstall is working on (on disk too: if Deck is closed
/// between the uninstaller and the leftovers, the Apps page offers to finish).
fn set_before(p: Option<Program>) {
    match &p {
        Some(p) => {
            let v = json!({ "program": p, "uninstall": p.uninstall, "family": p.family, "at": crate::util::now_s() });
            let _ = crate::util::write_atomic(&pending_file(), &serde_json::to_vec(&v).unwrap_or_default());
        }
        None => {
            let _ = std::fs::remove_file(pending_file());
        }
    }
    *BEFORE.lock() = p;
}

/// An advanced uninstall Deck was closed in the middle of: (name, when).
pub fn unfinished() -> Option<(String, u64)> {
    let v: Value = serde_json::from_slice(&std::fs::read(pending_file()).ok()?).ok()?;
    let mut p: Program = serde_json::from_value(v["program"].clone()).ok()?;
    p.uninstall = v["uninstall"].as_str().unwrap_or("").into();
    p.family = v["family"].as_str().unwrap_or("").into();
    let name = p.name.clone();
    let mut b = BEFORE.lock();
    if b.is_none() {
        *b = Some(p);
    }
    Some((name, v["at"].as_u64().unwrap_or(0)))
}

/// Forgets the unfinished one (the user doesn't want its leftovers looked for).
pub fn drop_unfinished() {
    set_before(None);
}

/// Runs the program's own uninstaller and waits for it and everything it started. `Ok(true)` when
/// the program is gone afterwards.
pub fn uninstall(app: &crate::app::App, id: &str, advanced: bool) -> Result<Outcome, String> {
    let r = uninstall_run(app, id, advanced);
    // an uninstall that failed or was refused removed nothing: no leftovers to look for, and no
    // "Deck was closed in the middle of it" warning to show later
    // (only this program's own record: a refusal before it started must not drop an older one)
    if r.is_err() && BEFORE.lock().as_ref().is_some_and(|b| b.id == id) {
        set_before(None);
    }
    r
}

fn uninstall_run(app: &crate::app::App, id: &str, advanced: bool) -> Result<Outcome, String> {
    let p = find(app, id)?;
    if let Some(g) = crate::boost::game_in_front(app) {
        return Err(format!("{g} is in front: Deck uninstalls programs only outside games"));
    }
    // every uninstall looks for leftovers afterwards (saved, so a restart in between still offers them)
    let _ = advanced;
    set_before(Some(p.clone()));
    crate::log::info(format!("apps: uninstalling {} ({})", p.name, p.id));
    if p.store {
        if p.id.contains('\'') {
            return Err("bad package name".into());
        }
        let full = &p.id["appx:".len()..];
        let o = crate::util::run_for(300, "powershell.exe", &["-NoProfile", "-NonInteractive", "-Command", &format!("Remove-AppxPackage -Package '{full}'")]).map_err(|e| e.to_string())?;
        *CACHE.lock() = None;
        if !o.status.success() {
            return Err(format!("Windows didn't remove {}: {}", p.name, String::from_utf8_lossy(&o.stderr).lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim()));
        }
        return Ok(Outcome { gone: true, detail: None });
    }
    if !p.can_uninstall {
        return Err(format!("{}'s uninstaller is missing. Use Advanced to remove what's left.", p.name));
    }
    let cmd = msi_uninstall(&p.uninstall);
    let detail = match msi_code(&cmd) {
        Some(code) => Some(msi_remove(&p.name, &code)?),
        None => {
            run_and_wait(&cmd)?;
            None
        }
    };
    *CACHE.lock() = None;
    let gone = !list(app, true).iter().any(|x| x.id == p.id);
    crate::log::info(format!("apps: {}'s uninstaller finished; {}{}", p.name, if gone { "it's gone" } else { "it's still installed" }, detail.as_deref().map(|d| format!(" ({d})")).unwrap_or_default()));
    Ok(Outcome { gone, detail })
}

/// What an uninstall did: whether the program is gone, and for Windows Installer programs what
/// Windows said (the reason it failed, or what Deck repaired on the way).
pub struct Outcome {
    pub gone: bool,
    pub detail: Option<String>,
}

/// The product code of an "MsiExec ... /X{GUID}" command.
fn msi_code(cmd: &str) -> Option<String> {
    let l = cmd.to_lowercase();
    // positions in `l` are positions in `cmd` only while lowercasing kept every length
    if !l.contains("msiexec") || l.len() != cmd.len() {
        return None;
    }
    let i = l.find("/x")?;
    let rest = cmd[i + 2..].trim_start();
    let end = rest.find('}')?;
    let code = &rest[..=end];
    (code.starts_with('{') && code.len() == 38 && code[1..37].chars().all(|c| c.is_ascii_hexdigit() || c == '-')).then(|| code.to_string())
}

/// Another Windows Installer job is running right now (Windows allows one at a time).
fn installer_busy() -> Option<String> {
    let k = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Installer\InProgress";
    key_exists(View::M64, k).then(|| get_str(View::M64, k, "").or_else(|| get_str(View::M64, k, "ProductName")).unwrap_or_default())
}

/// The Windows Installer service is stuck between states ("Install server not responding").
fn restart_installer_service() {
    let _ = crate::util::run_for(60, "net.exe", &["stop", "msiserver"]);
    let _ = crate::util::run_for(60, "net.exe", &["start", "msiserver"]);
}

/// Uninstalls a Windows Installer product with a full log, so a failure can be explained
/// instead of Windows' generic "There is a problem with this Windows Installer package". Repairs
/// the commonest cause (the program's folder was deleted by hand or a cleaner, and its uninstall
/// needs a program that was in it) and tries again. `Ok` = a plain-language report of what happened.
fn msi_remove(name: &str, code: &str) -> Result<String, String> {
    if let Some(other) = installer_busy() {
        let what = if other.trim().is_empty() { "another program".to_string() } else { format!("\"{}\"", other.trim()) };
        return Err(format!("Windows Installer is busy with {what}, and it runs one install or uninstall at a time. Let that finish (or restart the PC if nothing is installing), then try again."));
    }
    let log = crate::util::appdata().join("Deck").join("backups").join("uninstall").join(format!("{}-{}.log", safe_file(name), code.trim_matches(['{', '}'])));
    let _ = std::fs::create_dir_all(log.parent().unwrap());
    let run = |quiet: bool| -> Result<Option<u32>, String> {
        let _ = std::fs::remove_file(&log);
        run_and_wait(&format!("msiexec.exe /x{code}{} /norestart /L*v \"{}\"", if quiet { " /qn" } else { "" }, log.display()))
    };
    let mut rc = run(false)?;
    if rc == Some(1601) {
        restart_installer_service();
        rc = run(false)?;
    }
    let text = read_log(&log);
    let fail = msi_failure(&text);
    if rc == Some(1603) {
        if let Some((_, Some(exe))) = &fail {
            if !exe.exists() {
                let fixed = stand_in(exe, || run(true));
                let text2 = read_log(&log);
                return match fixed {
                    Ok(Some(0 | 3010 | 1641)) => Ok(format!(
                        "Windows' first try failed: its uninstall needs {}, which was deleted earlier (probably by hand or a cleanup tool). Deck put a stand-in there for a moment, Windows then finished the uninstall, and Deck removed the stand-in.{}",
                        exe.display(),
                        restart_note(fixed.ok().flatten())
                    )),
                    Ok(rc2) => Err(explain(name, rc2, &text2, &log)),
                    Err(e) => Err(format!("{name}'s uninstall needs {}, which was deleted earlier, and Deck couldn't put a stand-in there: {e}", exe.display())),
                };
            }
        }
    }
    match rc {
        Some(0 | 3010 | 1641) => Ok(format!("Windows Installer finished and reported \"Removal completed successfully\" for {name}. Its window closes by itself when it's done.{}", restart_note(rc))),
        _ => Err(explain(name, rc, &text, &log)),
    }
}

fn restart_note(rc: Option<u32>) -> &'static str {
    if matches!(rc, Some(3010 | 1641)) { " Restart the PC to finish removing files that were in use." } else { "" }
}

/// An MSI log (UTF-16 or ANSI) as text.
fn read_log(p: &Path) -> String {
    let b = std::fs::read(p).unwrap_or_default();
    if b.starts_with(&[0xFF, 0xFE]) {
        let w: Vec<u16> = b[2..].as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
        String::from_utf16_lossy(&w)
    } else {
        String::from_utf8_lossy(&b).into_owned()
    }
}

/// The first real error in an MSI log: its message, and the program a failed custom action
/// tried to run ("... location: C:\x\y.exe, command: ...").
fn msi_failure(log: &str) -> Option<(String, Option<PathBuf>)> {
    let line = log.lines().find(|l| {
        let t = l.trim_start();
        t.starts_with("Error 1") || t.starts_with("Error 2") || t.contains("-- Error 1") || t.contains("-- Error 2")
    })?;
    let msg = line.split_once("-- ").map(|(_, m)| m).unwrap_or(line).trim().to_string();
    let exe = msg.split_once("location: ").and_then(|(_, r)| r.split_once(", command:").map(|(p, _)| p.trim())).filter(|p| p.to_lowercase().ends_with(".exe")).map(PathBuf::from);
    Some((msg, exe))
}

/// Explains a failed Windows Installer uninstall in plain words.
fn explain(name: &str, rc: Option<u32>, log: &str, log_path: &Path) -> String {
    let why = match rc {
        None => "the uninstaller didn't report how it ended".to_string(),
        Some(1602) => return format!("The uninstall of {name} was cancelled, so nothing was removed."),
        Some(1605) => return format!("Windows Installer says {name} isn't installed any more (code 1605). Only its entry was left: use Advanced to clear what's left."),
        Some(1618) => "another install or uninstall was running (code 1618). Windows runs one at a time: wait for it, or restart the PC, then try again".to_string(),
        Some(1601) => "the Windows Installer service didn't answer, even after Deck restarted it (code 1601). Restart the PC and try again".to_string(),
        Some(1612 | 1620) => format!("Windows' saved copy of {name}'s installer package is missing (code {}), so Windows can't uninstall it. Advanced can still remove what's left", rc.unwrap()),
        Some(1625) => "a Group Policy on this PC forbids uninstalling it (code 1625)".to_string(),
        Some(1730) => "it needs admin rights (code 1730). Start Deck as administrator and try again".to_string(),
        Some(c) => match msi_failure(log) {
            Some((m, _)) => format!("Windows Installer stopped with code {c}: {m}"),
            None => format!("Windows Installer stopped with code {c} without naming an error"),
        },
    };
    format!("{name} wasn't uninstalled: {why}. Windows' full log: {}", log_path.display())
}

/// Puts a do-nothing copy of Deck at `exe` (with a marker so it only exits), runs `f`, then
/// removes the stand-in and any folders it created.
fn stand_in<T>(exe: &Path, f: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    // the path comes from the package's own log: never put a program inside Windows' own folders
    if crate::disk::protected(&exe.display().to_string()).is_some() {
        return Err("that place belongs to Windows, so Deck won't write there".into());
    }
    let mut made = Vec::new();
    let mut d = exe.parent().ok_or("bad path")?.to_path_buf();
    while !d.exists() {
        made.push(d.clone());
        d = d.parent().ok_or("bad path")?.to_path_buf();
    }
    std::fs::create_dir_all(exe.parent().unwrap()).map_err(|e| format!("can't create its folder ({e})"))?;
    let marker = stand_in_marker(exe);
    let r = std::fs::write(&marker, b"Deck stand-in: exits at once").and_then(|_| std::fs::copy(&me, exe).map(|_| ())).map_err(|e| format!("can't write there ({e})"));
    let out = r.and_then(|_| f());
    let _ = std::fs::remove_file(exe);
    let _ = std::fs::remove_file(&marker);
    for d in made.iter() {
        let _ = std::fs::remove_dir(d);
    }
    out
}

fn stand_in_marker(exe: &Path) -> PathBuf {
    let mut m = exe.as_os_str().to_owned();
    m.push(".deck-stand-in");
    PathBuf::from(m)
}

/// This process is a stand-in made by `stand_in`: it must do nothing and succeed.
pub fn is_stand_in() -> bool {
    std::env::current_exe().map(|e| stand_in_marker(&e).exists()).unwrap_or(false)
}

/// "MsiExec.exe /I{GUID}" (some programs list the repair command) uninstalls with /X.
fn msi_uninstall(cmd: &str) -> String {
    let l = cmd.to_lowercase();
    // positions in `l` are positions in `cmd` only while lowercasing kept every length ("İ" doesn't)
    if l.contains("msiexec") && l.len() == cmd.len() {
        if let Some(i) = l.find("/i{").or_else(|| l.find("/i {")) {
            let mut s = cmd.to_string();
            s.replace_range(i..i + 2, "/X");
            return s;
        }
    }
    cmd.to_string()
}

/// Starts a command line as written (its program elevated with Deck's rights, as Windows' own
/// "Uninstall" button does it) inside a job, and waits until every process in the job is done.
/// Returns the program's exit code (`None` when it didn't end within the limit).
fn run_and_wait(cmd: &str) -> Result<Option<u32>, String> {
    use std::os::windows::io::AsRawHandle;
    use std::os::windows::process::CommandExt;
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::JobObjects::{AssignProcessToJobObject, CreateJobObjectW, JobObjectBasicAccountingInformation, QueryInformationJobObject, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION};
    let exe = exe_of(cmd).ok_or("The uninstall command is empty")?;
    let cmd = cmd.trim();
    // the arguments after the program, exactly as the program wrote them
    let args = if let Some(rest) = cmd.strip_prefix('"') {
        rest.split_once('"').map(|(_, a)| a.trim_start().to_string()).unwrap_or_default()
    } else {
        let lower = cmd.to_lowercase();
        let exe_s = exe.display().to_string().to_lowercase();
        let raw_end = lower.find(".exe").map(|i| i + 4).unwrap_or(cmd.len());
        let end = if lower.starts_with(&exe_s) { exe_s.len().max(raw_end) } else { raw_end };
        cmd.get(end..).unwrap_or("").trim_start().to_string()
    };
    let dir = exe.parent().filter(|d| d.is_dir()).map(Path::to_path_buf).unwrap_or_else(std::env::temp_dir);
    let mut c = std::process::Command::new(&exe);
    c.raw_arg(&args).current_dir(dir);
    let child = c.spawn().map_err(|e| format!("Couldn't start the uninstaller ({}): {e}", exe.display()))?;
    let job = unsafe { CreateJobObjectW(None, &HSTRING::new()) }.map_err(|e| e.to_string())?;
    let in_job = unsafe { AssignProcessToJobObject(job, HANDLE(child.as_raw_handle())) }.is_ok();
    let mut child = child;
    let start = Instant::now();
    let limit = Duration::from_secs(3 * 3600);
    let mut code = None;
    loop {
        std::thread::sleep(Duration::from_millis(500));
        if let Ok(Some(s)) = child.try_wait() {
            code = code.or(s.code().map(|c| c as u32));
        }
        let main_done = code.is_some();
        let active = if in_job {
            let mut info = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
            unsafe { QueryInformationJobObject(Some(job), JobObjectBasicAccountingInformation, &mut info as *mut _ as *mut _, std::mem::size_of_val(&info) as u32, None) }
                .map(|_| info.ActiveProcesses)
                .unwrap_or(0)
        } else {
            u32::from(!main_done)
        };
        if (main_done && active == 0) || start.elapsed() > limit {
            break;
        }
    }
    unsafe {
        let _ = CloseHandle(job);
    }
    Ok(code)
}

// ------------------------------------------------------------------------------------- leftovers

/// Something a program left behind.
#[derive(Serialize, Clone, Debug)]
pub struct Leftover {
    pub id: usize,
    /// "folder" | "file" | "key"
    pub kind: &'static str,
    /// a path, or a registry key as "HKLM\SOFTWARE\x"
    pub path: String,
    pub size: Option<u64>,
    pub why: String,
    /// ticked to start with: exact matches; a publisher's folder or key isn't
    pub sure: bool,
    #[serde(skip)]
    view: Option<View>,
    #[serde(skip)]
    sub: String,
}

static FOUND: parking_lot::Mutex<Vec<Leftover>> = parking_lot::const_mutex(Vec::new());

/// "Visual Studio Code 1.94.2 (x64)" → "visualstudiocode": letters and digits, versions and
/// architecture words dropped.
fn norm(s: &str) -> String {
    let lower = s.to_lowercase();
    let mut words: Vec<String> = lower
        .split(|c: char| c.is_whitespace() || "()[],".contains(c))
        .map(|w| w.trim_matches(|c: char| "-_.".contains(c)))
        .filter(|w| !w.is_empty())
        .filter(|w| !["x64", "x86", "64-bit", "32-bit", "64bit", "32bit", "amd64", "arm64", "win64", "win32", "version", "v", "-"].contains(w))
        // language tags ("en-US")
        .filter(|w| !(w.len() == 5 && w.as_bytes()[2] == b'-' && w.chars().filter(|c| *c != '-').all(|c| c.is_ascii_lowercase())))
        // version numbers ("1.94.2", "24.08", "v8.6", "2024"), not names with digits ("7-Zip")
        .filter(|w| {
            let n = w.strip_prefix('v').filter(|r| !r.is_empty()).unwrap_or(w);
            !n.chars().all(|c| c.is_ascii_digit() || c == '.')
        })
        .map(String::from)
        .collect();
    if words.is_empty() {
        words.push(lower);
    }
    words.concat().chars().filter(|c| c.is_alphanumeric()).collect()
}

/// "Autodesk, Inc." → "autodesk": a publisher's name without the company form.
fn company(s: &str) -> String {
    let words: Vec<&str> = s.split(|c: char| c.is_whitespace() || c == ',').filter(|w| !w.is_empty()).collect();
    let kept: Vec<&str> = words.iter().copied().filter(|w| !["inc", "inc.", "corp", "corp.", "corporation", "ltd", "ltd.", "llc", "gmbh", "co.", "limited", "s.a.", "ag", "plc", "b.v."].contains(&w.to_lowercase().as_str())).collect();
    norm(&kept.join(" "))
}

/// Names that are never a single program's own folder or key.
const SHARED: &[&str] = &[
    "microsoft", "windows", "commonfiles", "common", "packages", "temp", "tmp", "programs", "microsoftnet", "windowsapps", "windowspowershell",
    "internetexplorer", "windowsdefender", "windowsmail", "windowsmediaplayer", "windowsnt", "windowsphotoviewer", "desktop", "documents",
    "downloads", "nvidia", "nvidiacorporation", "intel", "amd", "realtek", "classes", "policies", "clients", "wow6432node", "registeredapplications",
    "odbc", "google", "mozilla", "adobe", "apple", "oracle", "java", "python", "steam", "valve", "epicgames", "app", "apps", "data", "cache", "config",
    "settings", "users", "default", "public", "system", "system32", "drivers", "fonts", "installer", "setup", "update", "updates", "uninstall", "tools",
];

/// An installed program (normalised name, shown name) whose name is longer than `n` and contains
/// it: what is called `n` may be that program's ("Foo" for "Foo Pro").
fn longer_sibling<'a>(n: &str, others: &[(String, &'a str)]) -> Option<&'a str> {
    if n.chars().count() < 4 {
        return None;
    }
    others.iter().find(|(o, _)| o.len() > n.len() && o.contains(n)).map(|(_, shown)| *shown)
}

fn reg_name(view: View, sub: &str) -> String {
    let (root, _) = view.reg_exe();
    let wow = if view == View::M32 && sub.to_lowercase().starts_with("software\\") { format!("SOFTWARE\\WOW6432Node\\{}", &sub[9..]) } else { sub.to_string() };
    format!("{root}\\{wow}")
}

/// What a folder holds: its size (None if the search ran out of time) and how many files and
/// programs (.exe) are in it.
struct Contents {
    size: Option<u64>,
    files: u32,
    exes: u32,
}

/// From this many files on, a folder found only by its name is probably more than one program's
/// leftover (a browser profile, a game library, the user's own data): offered, not ticked. The
/// same for this many programs (.exe) in a data folder; in Program Files they are expected.
const MANY_EXES: u32 = 5;
const MANY_FILES: u32 = 3000;

/// How much of what is in a folder counts when deciding whether to tick it.
#[derive(Clone, Copy)]
enum Look {
    No,
    Files,
    FilesAndPrograms,
}

fn folder_contents(p: &Path, deadline: Instant) -> Contents {
    let mut c = Contents { size: Some(0), files: 0, exes: 0 };
    let mut stack = vec![p.to_path_buf()];
    while let Some(d) = stack.pop() {
        if Instant::now() > deadline {
            c.size = None;
            return c;
        }
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_dir() && !ft.is_symlink() {
                stack.push(e.path());
            } else if let Ok(m) = e.metadata() {
                c.size = c.size.map(|s| s + m.len());
                c.files += 1;
                if e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("exe")) {
                    c.exes += 1;
                }
            }
        }
    }
    c
}

/// The user's own places and the folders the search looks in: never a leftover themselves, and
/// nothing that holds one is either (an installer that recorded Documents, or a drive, as its
/// folder would otherwise offer all of it).
fn personal_places() -> Vec<String> {
    let home = crate::util::home();
    let mut v = crate::util::personal_folders();
    v.extend(["Documents", "Desktop", "Downloads", "Pictures", "Music", "Videos", "Saved Games", "OneDrive", "Favorites", "Contacts", "Links", "Searches", "AppData", r"AppData\LocalLow", r"AppData\Local\Programs"].iter().map(|n| home.join(n)));
    v.extend([home, crate::util::appdata(), crate::util::local_appdata(), crate::util::appdata().join(r"Microsoft\Windows\Start Menu\Programs"), crate::util::program_data().join(r"Microsoft\Windows\Start Menu\Programs")]);
    if let Some(public) = crate::util::env_path("PUBLIC") {
        v.push(public.join("Desktop"));
        v.push(public);
    }
    v.into_iter().map(|p| p.display().to_string().trim_end_matches('\\').to_lowercase()).filter(|s| s.len() > 3).collect()
}

/// `path` is one of `personal_places`, or holds one.
fn personal(path: &str, places: &[String]) -> bool {
    places.iter().any(|d| under(d, path))
}

/// A folder right on a drive ("D:\Games"): can be a program's, but often holds more than it.
fn on_drive_root(path: &Path) -> bool {
    path.parent().is_some_and(|p| p.parent().is_none())
}

fn under(p: &str, base: &str) -> bool {
    let (p, b) = (p.to_lowercase(), base.to_lowercase().trim_end_matches('\\').to_string());
    !b.is_empty() && (p == b || p.starts_with(&format!("{b}\\")))
}

/// Searches for what `before` left behind. The other installed programs' names and folders are
/// never offered.
pub fn scan(app: &crate::app::App) -> Result<Vec<Leftover>, String> {
    let p = BEFORE.lock().clone().ok_or("Uninstall a program first")?;
    let others: Vec<Program> = list(app, true).into_iter().filter(|x| x.id != p.id).collect();
    let out = scan_for(&p, &others);
    *FOUND.lock() = out.clone();
    crate::log::info(format!("apps: {} leftovers found for {}", out.len(), p.name));
    Ok(out)
}

/// What `p` left behind, the `others` still installed. Only looks; removes nothing.
fn scan_for(p: &Program, others: &[Program]) -> Vec<Leftover> {
    let other_names: std::collections::HashSet<String> = others.iter().map(|o| norm(&o.name)).collect();
    let other_dirs: Vec<String> = others.iter().filter_map(|o| o.location.clone()).collect();

    // the names that count as this program's
    let mut keys: Vec<String> = vec![norm(&p.name)];
    // its install folder's name counts only when it is a form of its own name ("Tool5" for
    // Tool); an add-on installed into another program's folder (a game's level editor in the
    // game's folder) must not take that program's name
    let loc_name = p.location.as_deref().and_then(|l| Path::new(l.trim_end_matches(['\\', '/'])).file_name()).map(|n| norm(&n.to_string_lossy())).unwrap_or_default();
    let own = norm(&p.name);
    // compared without digits ("Game 2 - Remastered" vs "Game 2: Remastered"); the folder may add to
    // the name ("Tool5") but not be a shorter one ("Some Game" for "Some Game: Level Editor")
    let letters = |s: &str| s.chars().filter(|c| !c.is_ascii_digit()).collect::<String>();
    let (ll, lo) = (letters(&loc_name), letters(&own));
    // ...or its name without the publisher's in front ("Acrobat DC" for "Adobe Acrobat DC")
    let bare = [norm(&p.publisher), company(&p.publisher)].iter().filter(|x| x.len() >= 3).find_map(|x| lo.strip_prefix(x.as_str())).map(str::to_string);
    let loc_is_named_after_it = ll.len() >= 4 && lo.len() >= 4 && (ll.contains(&lo) || bare.is_some_and(|b| b.len() >= 4 && ll == b));
    if loc_is_named_after_it {
        keys.push(loc_name.clone());
    }
    // its uninstaller lives in the folder: the strongest sign the folder is its own
    let uninstaller_inside = p.location.as_deref().is_some_and(|l| exe_of(&p.uninstall).is_some_and(|e| under(&e.display().to_string(), l.trim_end_matches(['\\', '/']))));
    let publisher = norm(&p.publisher);
    // a publisher's name is never one program's ("C:\Program Files\Autodesk" recorded as the
    // install folder of one Autodesk app holds the others too)
    let publishers: std::collections::HashSet<String> = others.iter().map(|o| o.publisher.as_str()).chain([p.publisher.as_str()]).flat_map(|s| [norm(s), company(s)]).filter(|n| !n.is_empty()).collect();
    let vendor_word = |k: &str| publishers.contains(k);
    keys.retain(|k| k.chars().count() >= 4 && !SHARED.contains(&k.as_str()) && !other_names.contains(k) && !vendor_word(k));
    keys.sort();
    keys.dedup();
    let short = company(&p.publisher);
    let publisher_ok = publisher.chars().count() >= 3 && !SHARED.contains(&publisher.as_str()) && !SHARED.contains(&short.as_str());
    let is_publisher = |n: &str| n == publisher || n == short;
    let matches = |name: &str| keys.contains(&norm(name));

    let mut out: Vec<Leftover> = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(20);
    let places = personal_places();
    let safe_dir = |path: &Path| -> bool {
        let s = path.display().to_string();
        crate::disk::protected(&s).is_none() && !personal(&s, &places) && !other_dirs.iter().any(|d| under(d, &s) || under(&s, d))
    };
    // an installed program with a longer name that contains this name ("Foo" for "Foo Pro"): the
    // folder or key may be that one's, so it is offered but not ticked
    let longer: Vec<(String, &str)> = others.iter().map(|o| (norm(&o.name), o.name.as_str())).collect();
    let vet_name = |name: &str, why: String, sure: bool| -> (String, bool) {
        match longer_sibling(&norm(name), &longer) {
            Some(other) if sure => (format!("{why}. {other} is still installed and its name contains this one, so it may be {other}'s (left unticked)"), false),
            _ => (why, sure),
        }
    };
    // `look`: a folder found only by its name is judged by what is in it too
    let add_dir = |out: &mut Vec<Leftover>, path: PathBuf, why: String, sure: bool, look: Look| {
        if !path.is_dir() || !safe_dir(&path) || out.iter().any(|l| under(&path.display().to_string(), &l.path)) {
            return;
        }
        let c = folder_contents(&path, deadline);
        let many = match look {
            Look::No => false,
            Look::Files => c.files >= MANY_FILES,
            Look::FilesAndPrograms => c.files >= MANY_FILES || c.exes >= MANY_EXES,
        };
        let (why, sure) = if many && sure {
            (format!("{why}. It holds {} files, {} of them programs: a lot for one leftover, and it may hold your own data or another program, so check it first (left unticked)", c.files, c.exes), false)
        } else {
            (why, sure)
        };
        out.push(Leftover { id: 0, kind: "folder", path: path.display().to_string(), size: c.size, why, sure, view: None, sub: String::new() });
    };

    // its install folder, if the uninstaller left it
    if let Some(l) = &p.location {
        let l = PathBuf::from(l);
        let name = l.file_name().map(|n| norm(&n.to_string_lossy())).unwrap_or_default();
        if vendor_word(&name) {
            // only the parts of the publisher's folder named after it (found below) are its own
        } else if loc_is_named_after_it || uninstaller_inside {
            let sure = !on_drive_root(&l);
            add_dir(&mut out, l, "Its install folder".into(), sure, Look::No);
        } else {
            add_dir(&mut out, l, "Recorded as its install folder, but not named after it and its uninstaller isn't in it: it may belong to something else (left unticked)".into(), false, Look::No);
        }
    }
    // folders named after it in the usual places, also inside its publisher's folder
    let docs = crate::util::home().join("Documents");
    let roots = [
        crate::util::program_files(),
        crate::util::program_files_x86(),
        crate::util::program_data(),
        crate::util::appdata(),
        crate::util::local_appdata(),
        crate::util::local_appdata().join("Programs"),
        crate::util::home().join("AppData").join("LocalLow"),
        docs.clone(),
    ];
    // programs are expected in the program folders, not in the data ones
    let program_roots = [crate::util::program_files(), crate::util::program_files_x86(), crate::util::local_appdata().join("Programs")];
    for root in roots.iter() {
        let look = if program_roots.contains(root) { Look::Files } else { Look::FilesAndPrograms };
        let Ok(rd) = std::fs::read_dir(root) else { continue };
        for e in rd.flatten() {
            if !e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            let name = e.file_name().to_string_lossy().to_string();
            if matches(&name) {
                // a folder in Documents is often the user's own work (a notes vault, saves): unticked
                let (why, sure) = vet_name(&name, format!("Named after it, in {}", root.display()), *root != docs);
                add_dir(&mut out, e.path(), why, sure, look);
            } else if publisher_ok && is_publisher(&norm(&name)) {
                // the publisher's folder: only the part named after this program
                let Ok(inner) = std::fs::read_dir(e.path()) else { continue };
                let kids: Vec<_> = inner.flatten().collect();
                for k in &kids {
                    if k.file_type().map(|t| t.is_dir()).unwrap_or(false) && matches(&k.file_name().to_string_lossy()) {
                        // in Documents it's often the user's own files (saves, settings): unticked
                        add_dir(&mut out, k.path(), format!("Named after it, in {}'s folder", p.publisher), *root != docs, look);
                    }
                }
                // nothing else in it: the publisher's folder itself, unticked
                if !kids.is_empty() && kids.iter().all(|k| out.iter().any(|l| l.path.eq_ignore_ascii_case(&k.path().display().to_string()))) {
                    add_dir(&mut out, e.path(), format!("{}'s folder, empty without it", p.publisher), false, Look::No);
                }
            }
        }
    }
    // Start menu and desktop shortcuts and folders named after it
    let start_dirs = [
        crate::util::appdata().join(r"Microsoft\Windows\Start Menu\Programs"),
        crate::util::program_data().join(r"Microsoft\Windows\Start Menu\Programs"),
        crate::util::home().join("Desktop"),
        PathBuf::from(std::env::var("PUBLIC").unwrap_or_default()).join("Desktop"),
    ];
    for d in start_dirs.iter() {
        let Ok(rd) = std::fs::read_dir(d) else { continue };
        for e in rd.flatten() {
            let path = e.path();
            let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
            let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            if is_dir && matches(&e.file_name().to_string_lossy()) {
                let (why, sure) = vet_name(&e.file_name().to_string_lossy(), "Its Start menu folder".into(), true);
                add_dir(&mut out, path, why, sure, Look::No);
            } else if !is_dir && path.extension().is_some_and(|x| x.eq_ignore_ascii_case("lnk")) && matches(&stem) && crate::disk::protected(&path.display().to_string()).is_none() {
                out.push(Leftover { id: 0, kind: "file", size: e.metadata().ok().map(|m| m.len()), path: path.display().to_string(), why: "A shortcut to it".into(), sure: true, view: None, sub: String::new() });
            }
        }
    }
    // registry: its Uninstall entry if still there, and Software\<it> or Software\<publisher>\<it>
    if let Some((tag, key)) = p.id.split_once(':').filter(|_| !p.store) {
        if let Some(view) = View::of(tag) {
            let sub = format!(r"{UNINSTALL}\{key}");
            if key_exists(view, &sub) {
                out.push(Leftover { id: 0, kind: "key", path: reg_name(view, &sub), size: None, why: "Its entry in Windows' list of programs".into(), sure: true, view: Some(view), sub });
            }
        }
    }
    for view in [View::User, View::M64, View::M32] {
        for name in subkeys(view, "Software") {
            let n = norm(&name);
            if SHARED.contains(&n.as_str()) {
                continue;
            }
            if matches(&name) {
                let sub = format!(r"Software\{name}");
                let (why, sure) = vet_name(&name, "Its settings".into(), true);
                out.push(Leftover { id: 0, kind: "key", path: reg_name(view, &sub), size: None, why, sure, view: Some(view), sub });
            } else if publisher_ok && is_publisher(&n) {
                let inner = subkeys(view, &format!(r"Software\{name}"));
                let mine: Vec<&String> = inner.iter().filter(|k| matches(k)).collect();
                for k in &mine {
                    let sub = format!(r"Software\{name}\{k}");
                    out.push(Leftover { id: 0, kind: "key", path: reg_name(view, &sub), size: None, why: format!("Its settings, in {}'s key", p.publisher), sure: true, view: Some(view), sub });
                }
                if !mine.is_empty() && mine.len() == inner.len() {
                    let sub = format!(r"Software\{name}");
                    out.push(Leftover { id: 0, kind: "key", path: reg_name(view, &sub), size: None, why: format!("{}'s key, empty without it", p.publisher), sure: false, view: Some(view), sub });
                }
            }
        }
    }
    // Windows services that run a program from its folder (they'd keep starting, and failing, at boot)
    if let Some(loc) = p.location.as_deref().filter(|l| crate::disk::protected(l).is_none()) {
        const SVC: &str = r"SYSTEM\CurrentControlSet\Services";
        for name in subkeys(View::M64, SVC) {
            let key = format!(r"{SVC}\{name}");
            let Some(image) = get_str(View::M64, &key, "ImagePath") else { continue };
            let exe = exe_of(&image).map(|e| e.display().to_string()).unwrap_or_default();
            if under(&exe, loc) && !other_dirs.iter().any(|d| under(&exe, d)) {
                let shown = get_str(View::M64, &key, "DisplayName").filter(|d| !d.starts_with('@')).unwrap_or_else(|| name.clone());
                out.push(Leftover { id: 0, kind: "service", path: format!("{shown} ({name})"), size: None, why: "A Windows service that runs a program from its folder".into(), sure: true, view: Some(View::M64), sub: name });
            }
        }
    }
    // the same key seen from both machine views is one key on a 64-bit PC's shared parts
    let mut seen = std::collections::HashSet::new();
    out.retain(|l| seen.insert(l.path.to_lowercase()));
    for (i, l) in out.iter_mut().enumerate() {
        l.id = i;
    }
    out
}

/// Removes the leftovers with these ids from the last search: files and folders to the Recycle
/// Bin, registry keys saved to Deck's backups folder first. Returns what failed.
pub fn remove_leftovers(ids: &[usize]) -> Result<Vec<String>, String> {
    let found = FOUND.lock().clone();
    let name = BEFORE.lock().as_ref().map(|p| p.name.clone()).unwrap_or_default();
    let mut failed = Vec::new();
    let backups = crate::util::appdata().join("Deck").join("backups").join("uninstall");
    let stamp = crate::util::now_s();
    let picked: Vec<&Leftover> = found.iter().filter(|l| ids.contains(&l.id)).collect();
    // a folder inside another picked one goes with it
    let picked: Vec<&Leftover> = picked.iter().filter(|l| !picked.iter().any(|o| o.id != l.id && o.kind == "folder" && l.kind != "key" && under(&l.path, &o.path))).copied().collect();
    for (n, l) in picked.iter().enumerate() {
        let r = match (l.kind, l.view) {
            ("service", _) => remove_service(&l.sub, &backups.join(format!("{stamp}-{n}-service-{}.reg", safe_file(&l.sub)))),
            ("key", Some(view)) => remove_key(view, &l.sub, &backups.join(format!("{stamp}-{n}-{}.reg", safe_file(&name)))),
            ("folder" | "file", _) => recycle(&l.path),
            _ => Err("unknown".into()),
        };
        if let Err(e) = r {
            failed.push(format!("{}: {e}", l.path));
        }
    }
    *CACHE.lock() = None;
    set_before(None);
    crate::log::info(format!("apps: removed {} leftovers of {name}, {} failed", picked.len() - failed.len(), failed.len()));
    Ok(failed)
}

fn safe_file(s: &str) -> String {
    s.chars().map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' { c } else { '_' }).collect::<String>().chars().take(60).collect()
}

fn remove_key(view: View, sub: &str, backup: &Path) -> Result<(), String> {
    // only what the search can find: Software\x, Software\x\y, or an Uninstall entry
    let parts: Vec<&str> = sub.split('\\').collect();
    let entry = sub.to_lowercase().starts_with(&format!("{}\\", UNINSTALL.to_lowercase())) && parts.len() == 6;
    let own = parts.first().is_some_and(|p| p.eq_ignore_ascii_case("software")) && (parts.len() == 2 || parts.len() == 3) && !SHARED.contains(&norm(parts[1]).as_str());
    let ok = entry || own;
    if !ok {
        return Err("not a key Deck removes".into());
    }
    let _ = std::fs::create_dir_all(backup.parent().unwrap());
    let (root, sw) = view.reg_exe();
    let full = format!("{root}\\{sub}");
    let o = crate::util::run_for(60, "reg.exe", &["export", &full, &backup.display().to_string(), "/y", sw]).map_err(|e| e.to_string())?;
    if !o.status.success() {
        return Err("couldn't save a copy first, so it was left".into());
    }
    let o = crate::util::run_for(60, "reg.exe", &["delete", &full, "/f", sw]).map_err(|e| e.to_string())?;
    if o.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&o.stderr).trim().to_string())
    }
}

/// Stops and removes a Windows service, its settings saved to a .reg file first.
fn remove_service(name: &str, backup: &Path) -> Result<(), String> {
    if name.is_empty() || name.contains(['\\', '/', '"']) {
        return Err("bad service name".into());
    }
    let _ = std::fs::create_dir_all(backup.parent().unwrap());
    let key = format!(r"HKLM\SYSTEM\CurrentControlSet\Services\{name}");
    let o = crate::util::run_for(60, "reg.exe", &["export", &key, &backup.display().to_string(), "/y"]).map_err(|e| e.to_string())?;
    if !o.status.success() {
        return Err("couldn't save a copy first, so it was left".into());
    }
    let _ = crate::util::run_for(60, "sc.exe", &["stop", name]);
    let o = crate::util::run_for(60, "sc.exe", &["delete", name]).map_err(|e| e.to_string())?;
    if o.status.success() {
        Ok(())
    } else {
        Err(format!("Windows didn't remove the service: {}", String::from_utf8_lossy(&o.stdout).lines().rfind(|l| !l.trim().is_empty()).unwrap_or("").trim()))
    }
}

/// To the Recycle Bin with Deck's rights (leftovers in Program Files need them), refusing Windows'
/// own folders and anything that resolves somewhere else through a link.
fn recycle(path: &str) -> Result<(), String> {
    use windows::Win32::UI::Shell::{SHFileOperationW, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, FOF_WANTNUKEWARNING, FO_DELETE, SHFILEOPSTRUCTW};
    let real = crate::fsguard::real_path(Path::new(path)).map_err(|e| e.to_string())?;
    let real_s = real.display().to_string();
    if !real_s.eq_ignore_ascii_case(path.trim_end_matches('\\')) {
        return Err("it leads somewhere else (a link), so it was left".into());
    }
    if let Some(why) = crate::disk::protected(&real_s) {
        return Err(why.into());
    }
    if personal(&real_s, &personal_places()) {
        return Err("it is one of your own folders, or holds one, so it was left".into());
    }
    let mut from: Vec<u16> = real_s.encode_utf16().collect();
    from.extend([0, 0]);
    let mut op = SHFILEOPSTRUCTW {
        wFunc: FO_DELETE,
        pFrom: windows::core::PCWSTR(from.as_ptr()),
        fFlags: (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_WANTNUKEWARNING | FOF_SILENT | FOF_NOERRORUI).0 as u16,
        ..Default::default()
    };
    let r = unsafe { SHFileOperationW(&mut op) };
    if r != 0 || op.fAnyOperationsAborted.as_bool() {
        return Err(format!("Windows couldn't move it to the Recycle Bin (code {r}); it may be in use"));
    }
    Ok(())
}

/// Advanced for a program whose uninstaller is gone: nothing to run, so the search starts at once
/// (its own entry is among what it finds).
pub fn forget(app: &crate::app::App, id: &str) -> Result<(), String> {
    let p = find(app, id)?;
    set_before(Some(p));
    Ok(())
}

pub fn to_json(v: &[Program]) -> Value {
    let total: u64 = v.iter().filter_map(|p| p.size).sum();
    json!({ "programs": v, "total": total, "admin": crate::elevation::is_admin(), "store_failed": *STORE_FAILED.lock(), "unfinished": unfinished().map(|(n, at)| json!({ "name": n, "at": at })) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_compare_without_versions() {
        assert_eq!(norm("Visual Studio Code 1.94.2 (x64)"), "visualstudiocode");
        assert_eq!(norm("7-Zip 24.08 (x64)"), "7zip");
        assert_eq!(norm("Notepad++ v8.6"), "notepad");
        assert_eq!(norm("VLC media player"), "vlcmediaplayer");
        assert_eq!(norm("WinRAR 7.01 (64-bit)"), "winrar");
        assert_eq!(norm("Mozilla Firefox (x64 en-US)"), "mozillafirefox");
    }

    /// What this PC's Windows Installer has that Windows' list lacks (prints them).
    #[test]
    #[ignore = "reads this PC's registry"]
    fn unlisted_programs_here() {
        let mut listed = std::collections::HashSet::new();
        for view in [View::M64, View::M32, View::User] {
            for key in subkeys(view, UNINSTALL) {
                listed.insert(key.to_uppercase());
            }
        }
        let v = installer_only(&listed);
        for p in &v {
            println!("{} | can_uninstall={} files_gone={} | {:?}", p.name, p.can_uninstall, p.files_gone, p.location);
        }
        println!("{} unlisted", v.len());
    }

    #[test]
    fn installer_codes_unpack() {
        // Radmin VPN's, as Windows Installer stores it and as its uninstall command names it
        assert_eq!(unpack_code("F1D0924654EA36A4C913D6A421006785").as_deref(), Some("{64290D1F-AE45-4A63-9C31-6D4A12007658}"));
        assert_eq!(unpack_code("nothex"), None);
    }

    #[test]
    fn your_own_folders_and_what_holds_them_are_never_leftovers() {
        let places = vec![r"c:\users\me".to_string(), r"c:\users\me\documents".to_string(), r"d:\docs".to_string()];
        assert!(personal(r"C:\Users\me\Documents", &places));
        assert!(personal(r"C:\Users\me", &places));
        assert!(personal(r"D:\", &places), "a drive that holds moved Documents");
        assert!(personal(r"D:\Docs", &places));
        assert!(!personal(r"C:\Users\me\Documents\Obsidian", &places), "inside is only unticked, not refused");
        assert!(!personal(r"C:\Program Files\Foo", &places));
        assert!(on_drive_root(Path::new(r"D:\Games")));
        assert!(!on_drive_root(Path::new(r"D:\Games\Foo")));
        // a lowercase that changes the length leaves the command alone instead of cutting it wrong
        assert_eq!(msi_uninstall("İ MsiExec.exe /I{X}"), "İ MsiExec.exe /I{X}");
    }

    #[test]
    fn msi_repair_commands_become_uninstall() {
        assert_eq!(msi_uninstall("MsiExec.exe /I{1234}"), "MsiExec.exe /X{1234}");
        assert_eq!(msi_uninstall("MsiExec.exe /X{1234}"), "MsiExec.exe /X{1234}");
        assert_eq!(msi_uninstall(r#""C:\x\unins000.exe""#), r#""C:\x\unins000.exe""#);
    }

    #[test]
    fn installer_codes_and_errors_are_read() {
        assert_eq!(msi_code("MsiExec.exe /X{A3985C05-7386-411F-A4BF-32A73F37EB44}").as_deref(), Some("{A3985C05-7386-411F-A4BF-32A73F37EB44}"));
        assert_eq!(msi_code("MsiExec.exe /X {A3985C05-7386-411F-A4BF-32A73F37EB44} /qn").as_deref(), Some("{A3985C05-7386-411F-A4BF-32A73F37EB44}"));
        assert_eq!(msi_code("C:\\x\\unins000.exe /X{bad}"), None);
        let log = "Action ended 3:40:55: InstallFinalize. Return value 3.\r\nMSI (s) (E8:98) [03:40:55:871]: Product: Apple Software Update -- Error 1721. There is a problem with this Windows Installer package. A program required for this install to complete could not be run. Contact your support personnel or package vendor. Action: SoftwareUpdate_UnregServer, location: C:\\Program Files (x86)\\Apple Software Update\\SoftwareUpdate.exe, command: /UnregServer \r\n";
        let (msg, exe) = msi_failure(log).unwrap();
        assert!(msg.starts_with("Error 1721."));
        assert_eq!(exe, Some(PathBuf::from(r"C:\Program Files (x86)\Apple Software Update\SoftwareUpdate.exe")));
        assert!(msi_failure("Return value 1.").is_none());
        assert!(explain("X", Some(1618), "", Path::new("l.log")).contains("one at a time"));
    }

    /// Dry run on this PC: what the leftover search would offer for every installed program as if
    /// it had just been uninstalled. Looks only. `cargo test -- --ignored --nocapture dry_run`
    #[test]
    #[ignore = "reads this PC's disk and registry"]
    fn dry_run_leftovers_here() {
        let mut all = Vec::new();
        let mut listed = std::collections::HashSet::new();
        for view in [View::M64, View::M32, View::User] {
            for key in subkeys(view, UNINSTALL) {
                listed.insert(key.to_uppercase());
                all.extend(desktop(view, &key));
            }
        }
        all.extend(installer_only(&listed));
        for p in &all {
            let others: Vec<Program> = all.iter().filter(|x| x.id != p.id).cloned().collect();
            for l in scan_for(p, &others) {
                println!("{}\t{}\t{}\t{}\t{}\t{}", p.name, p.publisher, if l.sure { "TICKED" } else { "offered" }, l.kind, l.path, l.why);
            }
        }
    }

    #[test]
    fn a_name_inside_another_installed_programs_name_is_not_surely_ours() {
        let others = vec![(norm("Tool Pro 2.1 (x64)"), "Tool Pro 2.1 (x64)"), (norm("Barn"), "Barn")];
        assert_eq!(longer_sibling("tool", &others), Some("Tool Pro 2.1 (x64)"));
        assert_eq!(longer_sibling("toolpro", &others), None, "the same name is not a longer one");
        assert_eq!(longer_sibling("pro", &others), None, "too short to judge");
        assert_eq!(longer_sibling("other", &others), None);
    }

    #[test]
    fn folders_are_counted_by_files_and_programs() {
        let base = std::env::temp_dir().join(format!("deck-contents-{}", std::process::id()));
        std::fs::create_dir_all(base.join("sub")).unwrap();
        std::fs::write(base.join("a.exe"), b"12345").unwrap();
        std::fs::write(base.join("sub").join("B.EXE"), b"123").unwrap();
        std::fs::write(base.join("sub").join("c.txt"), b"12").unwrap();
        let c = folder_contents(&base, Instant::now() + Duration::from_secs(5));
        let _ = std::fs::remove_dir_all(&base);
        assert_eq!((c.size, c.files, c.exes), (Some(10), 3, 2));
    }

    #[test]
    fn publishers_are_named_without_the_company_form() {
        assert_eq!(company("Autodesk, Inc."), "autodesk");
        assert_eq!(company("Apple Inc."), "apple");
        assert_eq!(company("Notepad++ Team"), "notepadteam");
        assert_eq!(company("Mozilla Corporation"), "mozilla");
    }

    #[test]
    fn a_stand_in_is_never_put_inside_windows() {
        let exe = crate::util::windows_dir().join("System32").join("deck-stand-in-test.exe");
        let r = stand_in(&exe, || Ok(()));
        assert!(r.is_err() && !exe.exists());
    }

    #[test]
    fn a_stand_in_is_removed_with_the_folders_it_made() {
        let base = std::env::temp_dir().join(format!("deck-standin-{}", std::process::id()));
        let exe = base.join("gone").join("deeper").join("Thing.exe");
        let seen = stand_in(&exe, || Ok((exe.exists(), stand_in_marker(&exe).exists()))).unwrap();
        assert_eq!(seen, (true, true));
        assert!(!exe.exists() && !base.exists());
    }

    #[test]
    fn the_program_of_a_command_line() {
        assert_eq!(exe_of(r#""C:\Program Files\x\u.exe" /S"#), Some(PathBuf::from(r"C:\Program Files\x\u.exe")));
        assert_eq!(exe_of("MsiExec.exe /X{1}"), Some(PathBuf::from("MsiExec.exe")));
    }

    #[test]
    fn only_narrow_keys_are_removed() {
        let tmp = std::env::temp_dir().join("deck-test.reg");
        assert!(remove_key(View::User, r"Software\Microsoft", &tmp).is_err());
        assert!(remove_key(View::User, r"Software\Classes", &tmp).is_err());
        assert!(remove_key(View::User, r"Software\a\b\c\d", &tmp).is_err());
        assert!(remove_key(View::User, r"System\Foo", &tmp).is_err());
        // the whole Uninstall key, not one entry in it
        assert!(remove_key(View::User, UNINSTALL, &tmp).is_err());
    }
}
