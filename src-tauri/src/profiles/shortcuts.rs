//! Desktop and Start menu shortcuts that switch a profile with no UAC prompt and no window:
//! shortcut → `wscript run.vbs "\Deck\Profile cpu-<id>"` → that task runs
//! `Deck.exe --switch cpu:<id>` elevated → the running Deck switches (single instance).

use std::path::{Path, PathBuf};
use windows::core::{Interface, HSTRING, PCWSTR};
use windows::Win32::System::Com::{CoCreateInstance, IPersistFile, CLSCTX_INPROC_SERVER, STGM_READ};
use windows::Win32::UI::Shell::{FOLDERID_Desktop, FOLDERID_Programs, IShellLinkW, SHChangeNotify, SHGetKnownFolderPath, ShellLink, KF_FLAG_DEFAULT, SHCNE_ASSOCCHANGED, SHCNF_IDLIST};

pub fn task_name(kind: &str, id: &str) -> String {
    format!("\\Deck\\Profile {kind}-{id}")
}

fn known(id: &windows::core::GUID) -> Option<PathBuf> {
    unsafe {
        let p = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).ok()?;
        let s = p.to_string().ok();
        windows::Win32::System::Com::CoTaskMemFree(Some(p.0 as *const _));
        s.map(PathBuf::from)
    }
}

pub fn desktop() -> Option<PathBuf> {
    known(&FOLDERID_Desktop)
}

pub fn start_menu() -> Option<PathBuf> {
    known(&FOLDERID_Programs)
}

/// The script that starts a task hidden. Installed next to Deck.exe (admin-only folder); a
/// development build writes it to %APPDATA%\Deck instead.
fn runner() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    if let Some(p) = exe.parent().map(|d| d.join("run.vbs")).filter(|p| p.exists()) {
        return Ok(p);
    }
    let p = crate::settings::Settings::dir().join("run.vbs");
    std::fs::write(&p, include_str!("../../run.vbs")).map_err(|e| e.to_string())?;
    Ok(p)
}

/// "CPU Ultra" → a safe file name.
pub fn link_name(kind: &str, name: &str) -> String {
    let clean: String = name.chars().map(|c| if "<>:\"/\\|?*".contains(c) || c.is_control() { ' ' } else { c }).collect();
    let clean = clean.split_whitespace().collect::<Vec<_>>().join(" ");
    format!("{} {}.lnk", kind.to_uppercase(), if clean.is_empty() { "profile" } else { &clean })
}

/// Writes a .lnk.
pub fn write_link(path: &Path, target: &str, args: &str, icon: (&str, i32), desc: &str) -> Result<(), String> {
    crate::util::com();
    unsafe {
        let l: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).map_err(|e| e.to_string())?;
        l.SetPath(&HSTRING::from(target)).map_err(|e| e.to_string())?;
        l.SetArguments(&HSTRING::from(args)).map_err(|e| e.to_string())?;
        l.SetIconLocation(&HSTRING::from(icon.0), icon.1).map_err(|e| e.to_string())?;
        l.SetDescription(&HSTRING::from(desc)).map_err(|e| e.to_string())?;
        let pf: IPersistFile = l.cast().map_err(|e| e.to_string())?;
        pf.Save(&HSTRING::from(path.as_os_str()), true).map_err(|e| format!("couldn't save the shortcut: {e}"))
    }
}

/// A .lnk's arguments (to find Deck's own shortcuts again after a rename).
pub fn link_args(path: &Path) -> Option<String> {
    crate::util::com();
    unsafe {
        let l: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
        let pf: IPersistFile = l.cast().ok()?;
        pf.Load(&HSTRING::from(path.as_os_str()), STGM_READ).ok()?;
        let mut buf = [0u16; 1024];
        l.GetArguments(&mut buf).ok()?;
        Some(crate::util::from_wide(&buf))
    }
}

/// Deck's shortcuts for this profile in `dir` (matched by the task they start).
fn find(dir: &Path, kind: &str, id: &str) -> Vec<PathBuf> {
    let needle = format!("Profile {kind}-{id}\"");
    std::fs::read_dir(dir)
        .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.extension().map(|x| x.eq_ignore_ascii_case("lnk")).unwrap_or(false)).filter(|p| link_args(p).map(|a| a.contains(&needle)).unwrap_or(false)).collect())
        .unwrap_or_default()
}

pub struct Want<'a> {
    pub kind: &'a str,
    pub id: &'a str,
    pub name: &'a str,
    pub icon: &'a str,
    pub desktop: bool,
    pub start_menu: bool,
}

/// Makes the shortcuts and task match the profile: creates, renames, re-icons or removes.
pub fn sync(w: &Want) -> Result<(), String> {
    let task = task_name(w.kind, w.id);
    let fallback = if w.kind == "cpu" { "cpu-mid.ico" } else { "gpu-green.ico" };
    let want_any = w.desktop || w.start_menu;
    if want_any {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        crate::elevation::create_task(&task, &crate::elevation::task_xml(&exe.to_string_lossy(), &format!("--switch {}:{}", w.kind, w.id), false, &format!("Switches Deck to the {} profile \"{}\". Started by its shortcut.", w.kind.to_uppercase(), w.name)))?;
    } else {
        crate::elevation::delete_task(&task);
    }
    let vbs = runner()?;
    let wscript = format!("{}\\System32\\wscript.exe", std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into()));
    let args = format!("\"{}\" \"{}\"", vbs.display(), task);
    let icon = super::icons::location(w.icon, fallback);
    let mut err = None;
    for (dir, on) in [(desktop(), w.desktop), (start_menu(), w.start_menu)] {
        let Some(dir) = dir else { continue };
        let name = dir.join(link_name(w.kind, w.name));
        for old in find(&dir, w.kind, w.id) {
            if !on || old != name {
                let _ = std::fs::remove_file(old);
            }
        }
        if on {
            if let Err(e) = write_link(&name, &wscript, &args, (&icon.0, icon.1), &format!("Deck: switch to the {} profile {}", w.kind.to_uppercase(), w.name)) {
                err = Some(e);
            }
        }
    }
    unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None) };
    err.map_or(Ok(()), Err)
}

pub fn remove(kind: &str, id: &str) {
    let _ = sync(&Want { kind, id, name: "", icon: "", desktop: false, start_menu: false });
}

/// Recreates every profile's shortcuts (Settings → Repair shortcuts). Returns problems.
pub fn repair(app: &crate::app::App) -> Vec<String> {
    let (cpu, gpu) = {
        let s = app.settings.lock();
        (s.cpu_profiles.clone(), s.gpu_profiles.clone())
    };
    let mut errs = vec![];
    for p in &cpu {
        if let Err(e) = sync(&Want { kind: "cpu", id: &p.id, name: &p.name, icon: &p.icon, desktop: p.desktop, start_menu: p.start_menu }) {
            errs.push(format!("CPU {}: {e}", p.name));
        }
    }
    for p in &gpu {
        if let Err(e) = sync(&Want { kind: "gpu", id: &p.id, name: &p.name, icon: &p.icon, desktop: p.desktop, start_menu: p.start_menu }) {
            errs.push(format!("GPU {}: {e}", p.name));
        }
    }
    errs
}

#[allow(dead_code)]
fn _unused(_: PCWSTR) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_safe() {
        assert_eq!(link_name("cpu", "Ultra"), "CPU Ultra.lnk");
        assert_eq!(link_name("gpu", "Low: power?"), "GPU Low power.lnk");
        assert_eq!(link_name("gpu", "  "), "GPU profile.lnk");
    }

    #[test]
    fn link_round_trip() {
        let d = std::env::temp_dir().join(format!("deck-lnk-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("CPU Test.lnk");
        let args = "\"C:\\x\\run.vbs\" \"\\Deck\\Profile cpu-p123\"";
        write_link(&p, "C:\\Windows\\System32\\wscript.exe", args, ("C:\\Windows\\System32\\shell32.dll", 3), "test").unwrap();
        assert_eq!(link_args(&p).as_deref(), Some(args));
        assert_eq!(find(&d, "cpu", "p123"), vec![p.clone()]);
        assert!(find(&d, "cpu", "p12").is_empty(), "ids must match whole");
        let _ = std::fs::remove_dir_all(&d);
    }
}
