//! Running what arrived: finds the installer in a received folder, recognises common installer
//! frameworks (so they can be run silently), starts them and reports how it went.
//! Scripts (.bat, .ps1, .cmd, ...) are never run automatically.
use crate::beam::config::Shared;
use crate::beam::winutil::wide;
use std::io::Read;
use std::path::Path;
use std::sync::Arc;
use windows_sys::Win32::Foundation::CloseHandle;
use windows_sys::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject, INFINITE};
use windows_sys::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

fn ext(p: &str) -> String {
    Path::new(p).extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default()
}

fn is_installer(p: &str) -> bool {
    matches!(ext(p).as_str(), "exe" | "msi" | "msix" | "appx" | "msixbundle" | "appxbundle" | "zip")
}

/// which received file is "the installer": the only one, or one called setup/install
pub fn find_entry(files: &[(String, u64)]) -> Option<String> {
    let cands: Vec<&String> = files.iter().map(|(r, _)| r).filter(|r| is_installer(r) && r.matches('/').count() <= 1).collect();
    if cands.len() == 1 {
        return Some(cands[0].clone());
    }
    let named: Vec<&&String> = cands
        .iter()
        .filter(|r| {
            let n = r.rsplit('/').next().unwrap_or("").to_ascii_lowercase();
            n.starts_with("setup") || n.starts_with("install")
        })
        .collect();
    if named.len() == 1 {
        return Some((*named[0]).clone());
    }
    None
}

/// "inno", "nsis", "burn", "msi", "msix", "zip", "exe" or "" for things that can't be run
pub fn kind_of(path: &Path) -> String {
    let p = path.to_string_lossy();
    match ext(&p).as_str() {
        "msi" => "msi".into(),
        "msix" | "appx" | "msixbundle" | "appxbundle" => "msix".into(),
        "zip" => "zip".into(),
        "exe" => {
            let mut head = Vec::new();
            if let Ok(f) = std::fs::File::open(path) {
                let _ = f.take(8 << 20).read_to_end(&mut head);
            }
            let has = |needle: &[u8]| memchr::memmem::find(&head, needle).is_some();
            if has(b".wixburn") {
                "burn".into()
            } else if has(b"Nullsoft") {
                "nsis".into()
            } else if has(b"Inno Setup") {
                "inno".into()
            } else {
                "exe".into()
            }
        }
        _ => String::new(),
    }
}

pub fn can_silent(kind: &str) -> bool {
    matches!(kind, "inno" | "nsis" | "burn" | "msi")
}

pub fn set_note(sh: &Shared, dir: &str, note: String) {
    if let Some(i) = sh.inbox.lock().unwrap().iter_mut().find(|i| i.dir == dir) {
        i.note = note;
    }
    sh.repaint();
}

/// starts a file and waits for it; Ok(exit code), or Ok(0) when it was only opened (no process to wait for)
pub fn shell_run(verb: Option<&str>, file: &str, params: &str, cwd: &str) -> Result<u32, String> {
    unsafe {
        let (v, f, p, d) = (verb.map(wide), wide(file), wide(params), wide(cwd));
        let mut info: SHELLEXECUTEINFOW = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
        info.fMask = SEE_MASK_NOCLOSEPROCESS;
        info.lpVerb = v.as_ref().map(|v| v.as_ptr()).unwrap_or(std::ptr::null());
        info.lpFile = f.as_ptr();
        info.lpParameters = p.as_ptr();
        info.lpDirectory = d.as_ptr();
        info.nShow = SW_SHOWNORMAL;
        if ShellExecuteExW(&mut info) == 0 {
            let e = std::io::Error::last_os_error();
            return Err(if e.raw_os_error() == Some(1223) { "permission was declined".into() } else { e.to_string() });
        }
        if info.hProcess.is_null() {
            return Ok(0);
        }
        WaitForSingleObject(info.hProcess, INFINITE);
        let mut code = 0u32;
        GetExitCodeProcess(info.hProcess, &mut code);
        CloseHandle(info.hProcess);
        Ok(code)
    }
}

/// installs (or opens) the entry of an inbox item on a background thread; `silent` is used when the kind supports it
pub fn install_async(sh: Arc<Shared>, dir: String, silent: bool) {
    std::thread::spawn(move || {
        let item = sh.inbox.lock().unwrap().iter().find(|i| i.dir == dir).cloned();
        let Some(item) = item else { return };
        let Some(entry) = item.entry.clone() else {
            set_note(&sh, &dir, "No installer found in this folder".into());
            return;
        };
        let file = Path::new(&dir).join(entry.replace('/', "\\")).display().to_string();
        let silent = silent && can_silent(&item.kind);
        sh.log(format!("Starting {} ({})", item.name, if silent { "silent" } else { "with its own window" }));
        set_note(&sh, &dir, if silent { "Installing silently...".into() } else { "Installer opened - finish it in its window".into() });
        let result = match item.kind.as_str() {
            "zip" => {
                let stem = Path::new(&file).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "extracted".into());
                let target = Path::new(&dir).join(&stem);
                let _ = std::fs::create_dir_all(&target);
                let ok = {
                    use std::os::windows::process::CommandExt;
                    std::process::Command::new("tar.exe").arg("-xf").arg(&file).arg("-C").arg(&target).creation_flags(0x0800_0000).status().map(|s| s.success()).unwrap_or(false)
                };
                if ok {
                    crate::beam::winutil::open_folder(&target);
                    Ok(format!("Extracted to {}", target.display()))
                } else {
                    Err("couldn't extract the zip".to_string())
                }
            }
            "msi" => {
                let args = if silent { format!("/i \"{file}\" /qn /norestart") } else { format!("/i \"{file}\"") };
                shell_run(silent.then_some("runas"), "msiexec.exe", &args, &dir).map(|c| code_note(c, silent))
            }
            "inno" if silent => shell_run(Some("runas"), &file, "/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /SP-", &dir).map(|c| code_note(c, true)),
            "nsis" if silent => shell_run(Some("runas"), &file, "/S", &dir).map(|c| code_note(c, true)),
            "burn" if silent => shell_run(Some("runas"), &file, "/quiet /norestart", &dir).map(|c| code_note(c, true)),
            "msix" => shell_run(None, &file, "", &dir).map(|_| "Opened in App Installer - confirm there".to_string()),
            _ => {
                let cwd = Path::new(&file).parent().map(|p| p.display().to_string()).unwrap_or(dir.clone());
                shell_run(None, &file, "", &cwd).map(|c| code_note(c, false))
            }
        };
        match result {
            Ok(n) => {
                sh.log(format!("{}: {n}", item.name));
                set_note(&sh, &dir, n);
            }
            Err(e) => {
                sh.log(format!("{}: {e}", item.name));
                set_note(&sh, &dir, format!("Couldn't start: {e}"));
            }
        }
    });
}

fn code_note(code: u32, silent: bool) -> String {
    match code {
        0 => if silent { "Installed".into() } else { "Installer finished".into() },
        3010 | 1641 => "Installed - restart needed to finish".into(),
        c => format!("Installer closed with code {c}"),
    }
}

/// applies the chosen parts of a received setup snapshot (backing up first); Windhawk's part runs elevated
pub fn apply_setup_async(sh: Arc<Shared>, dir: String, look: bool, wallpaper: bool, windhawk: bool, restart: bool) {
    std::thread::spawn(move || {
        let d = Path::new(&dir);
        let Some(m) = crate::beam::snapshot::read_manifest(d) else {
            set_note(&sh, &dir, "This isn't a setup snapshot".into());
            return;
        };
        set_note(&sh, &dir, "Applying - backing up your current settings first...".into());
        let (mut done, mut errs) = (vec![], vec![]);
        if windhawk && m.windhawk_version.is_some() {
            match elevated_windhawk(d, false) {
                Ok(s) => {
                    done.push(s);
                    crate::beam::snapshot::restart_windhawk();
                }
                Err(e) => errs.push(e),
            }
        }
        if look || wallpaper {
            match crate::beam::snapshot::apply_look(d, &m, look, wallpaper, restart) {
                Ok(s) => done.push(s),
                Err(e) => errs.push(e),
            }
        }
        let note = done.iter().cloned().chain(errs.iter().map(|e| format!("Problem: {e}"))).collect::<Vec<_>>().join(" - ");
        sh.log(format!("Setup apply: {note}"));
        set_note(&sh, &dir, note);
    });
}

pub fn undo_setup_async(sh: Arc<Shared>, dir: String, restart: bool) {
    std::thread::spawn(move || {
        let d = Path::new(&dir);
        let mut out = vec![];
        if d.join("_backup").join("windhawk").exists() {
            match elevated_windhawk(d, true) {
                Ok(s) => {
                    out.push(s);
                    crate::beam::snapshot::restart_windhawk();
                }
                Err(e) => out.push(format!("Problem: {e}")),
            }
        }
        if d.join("_backup").join("look.json").exists() {
            out.push(crate::beam::snapshot::undo_look(d, restart).unwrap_or_else(|e| format!("Problem: {e}")));
        }
        let note = if out.is_empty() { "Nothing to undo".to_string() } else { out.join(" - ") };
        sh.log(format!("Setup undo: {note}"));
        set_note(&sh, &dir, note);
    });
}

fn elevated_windhawk(dir: &Path, undo: bool) -> Result<String, String> {
    let _ = std::fs::remove_file(dir.join("_wh_result.txt"));
    // Deck usually runs as admin already: no second process needed
    let code = if crate::elevation::is_admin() {
        crate::beam::snapshot::windhawk_elevated(dir, undo) as u32
    } else {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?.display().to_string();
        let flag = if undo { "--transfer-undo-windhawk" } else { "--transfer-apply-windhawk" };
        shell_run(Some("runas"), &exe, &format!("{flag} \"{}\"", dir.display()), &dir.display().to_string())?
    };
    let msg = std::fs::read_to_string(dir.join("_wh_result.txt")).unwrap_or_default();
    if code == 0 {
        Ok(msg)
    } else {
        Err(if msg.is_empty() { format!("Windhawk step failed (code {code})") } else { msg })
    }
}
