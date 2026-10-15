//! Deck runs as admin through a logon scheduled task (no UAC prompt). Double-clicking
//! Deck.exe starts it unelevated; the Hardware pages then offer "Set up", which asks for
//! admin once, creates the task and restarts Deck through it.

use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::Command;

/// The task that starts Deck elevated at logon (and from the Start menu shortcut).
pub const TASK: &str = "\\Deck\\Deck";

const NO_WINDOW: u32 = 0x0800_0000;

/// True when this process has an elevated (admin) token.
pub fn is_admin() -> bool {
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    static CACHE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *CACHE.get_or_init(|| unsafe {
        let mut tok = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut tok).is_err() {
            return false;
        }
        let mut el = TOKEN_ELEVATION::default();
        let mut len = 0u32;
        let ok = GetTokenInformation(tok, TokenElevation, Some(&mut el as *mut _ as *mut _), std::mem::size_of::<TOKEN_ELEVATION>() as u32, &mut len).is_ok();
        let _ = CloseHandle(tok);
        ok && el.TokenIsElevated != 0
    })
}

pub fn esc_xml(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Task Scheduler XML. `logon` adds a logon trigger for the current user; without it the
/// task only runs when started (shortcuts).
pub fn task_xml(exe: &str, args: &str, logon: bool, description: &str) -> String {
    let user = format!("{}\\{}", std::env::var("USERDOMAIN").unwrap_or_default(), std::env::var("USERNAME").unwrap_or_default());
    let trigger = if logon { format!("<Triggers><LogonTrigger><Enabled>true</Enabled><UserId>{}</UserId></LogonTrigger></Triggers>", esc_xml(&user)) } else { String::new() };
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-16\"?><Task version=\"1.2\" xmlns=\"http://schemas.microsoft.com/windows/2004/02/mit/task\">\
<RegistrationInfo><Description>{d}</Description></RegistrationInfo>{t}\
<Principals><Principal id=\"A\"><UserId>{u}</UserId><LogonType>InteractiveToken</LogonType><RunLevel>HighestAvailable</RunLevel></Principal></Principals>\
<Settings><MultipleInstancesPolicy>Parallel</MultipleInstancesPolicy><DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries><StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>\
<ExecutionTimeLimit>PT0S</ExecutionTimeLimit><Priority>5</Priority><Enabled>true</Enabled></Settings>\
<Actions Context=\"A\"><Exec><Command>{e}</Command><Arguments>{a}</Arguments></Exec></Actions></Task>",
        d = esc_xml(description),
        t = trigger,
        u = esc_xml(&user),
        e = esc_xml(exe),
        a = esc_xml(args)
    )
}

fn quiet(prog: &str, args: &[&str]) -> bool {
    Command::new(prog).args(args).creation_flags(NO_WINDOW).status().map(|s| s.success()).unwrap_or(false)
}

/// Registers (or replaces) a task from XML. Needs admin.
pub fn create_task(name: &str, xml: &str) -> Result<(), String> {
    let path = std::env::temp_dir().join(format!("deck_task_{}.xml", crate::util::now_ms()));
    let mut bytes: Vec<u8> = vec![0xFF, 0xFE];
    for u in xml.encode_utf16() {
        bytes.extend_from_slice(&u.to_le_bytes());
    }
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    let ok = quiet("schtasks.exe", &["/create", "/tn", name, "/xml", &path.display().to_string(), "/f"]);
    let _ = std::fs::remove_file(&path);
    if ok {
        Ok(())
    } else {
        Err("Windows refused to create the scheduled task.".into())
    }
}

pub fn delete_task(name: &str) {
    quiet("schtasks.exe", &["/delete", "/tn", name, "/f"]);
}

pub fn task_exists(name: &str) -> bool {
    quiet("schtasks.exe", &["/query", "/tn", name])
}

/// The logon task for this exe.
pub fn ensure_task(exe: &Path, autostart: bool) -> Result<(), String> {
    create_task(TASK, &task_xml(&exe.display().to_string(), "--minimized", true, "Starts Deck with admin rights at logon (hardware control)."))?;
    set_task_enabled(autostart)
}

/// "Start with Windows": the logon task on or off. The task still exists for the Start
/// menu shortcut, so Deck is started through `run.vbs` + a second, trigger-less task.
pub fn set_task_enabled(on: bool) -> Result<(), String> {
    if !task_exists(TASK) {
        return Err("The Deck task is missing. Use Set up on a Hardware page.".into());
    }
    if quiet("schtasks.exe", &["/change", "/tn", TASK, if on { "/enable" } else { "/disable" }]) {
        Ok(())
    } else {
        Err("Couldn't change the Deck startup task.".into())
    }
}

/// Starts a task (it runs elevated if it was registered that way).
pub fn run_task(name: &str) -> bool {
    quiet("schtasks.exe", &["/run", "/tn", name])
}

/// The trigger-less task the Start menu shortcut and `--relaunch` use to start Deck elevated.
pub const OPEN_TASK: &str = "\\Deck\\Open";

/// The trigger-less task Deck's setup and uninstaller start to close an elevated Deck: they run
/// as a normal user, which may start this task but may not end an elevated process.
pub const QUIT_TASK: &str = "\\Deck\\Quit";

fn create_quit_task(exe: &Path) -> Result<(), String> {
    create_task(QUIT_TASK, &task_xml(&exe.display().to_string(), "--quit", false, "Closes Deck (used by its setup and uninstaller)."))
}

/// Creates the Deck tasks for `exe`. Called from `Deck.exe --setup-task` (elevated).
pub fn setup(exe: &Path, autostart: bool) -> Result<(), String> {
    ensure_task(exe, autostart)?;
    create_task(OPEN_TASK, &task_xml(&exe.display().to_string(), "", false, "Opens Deck with admin rights (used by the Start menu shortcut)."))?;
    create_quit_task(exe)
}

/// Installs made before the Quit task existed get it the next time an elevated Deck starts
/// (and again when Deck moved to another folder, so the task never points at a missing exe).
pub fn ensure_quit_task() {
    if !is_admin() || !task_exists(TASK) {
        return;
    }
    let Ok(exe) = std::env::current_exe() else { return };
    let want = exe.display().to_string().to_lowercase();
    let current = Command::new("schtasks.exe").args(["/query", "/tn", QUIT_TASK, "/xml"]).creation_flags(NO_WINDOW).output();
    let ok = current.map(|o| o.status.success() && String::from_utf8_lossy(&o.stdout).to_lowercase().contains(&esc_xml(&want))).unwrap_or(false);
    if !ok {
        let _ = create_quit_task(&exe);
    }
}

/// From an unelevated Deck: one UAC prompt runs `Deck.exe --setup-task`, which creates the
/// tasks and starts Deck elevated. The caller should quit afterwards.
pub fn elevate_setup() -> Result<(), String> {
    use windows::core::HSTRING;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let r = unsafe { ShellExecuteW(None, &HSTRING::from("runas"), &HSTRING::from(exe.as_os_str()), &HSTRING::from("--setup-task --start"), None, SW_HIDE) };
    if r.0 as isize > 32 {
        Ok(())
    } else {
        Err("Admin rights were not given.".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logon_task_xml() {
        let x = task_xml(r"C:\Program Files\Deck\Deck.exe", "--minimized", true, "a & b");
        assert!(x.contains("<RunLevel>HighestAvailable</RunLevel>"));
        assert!(x.contains("<LogonTrigger>"));
        assert!(x.contains(r"<Command>C:\Program Files\Deck\Deck.exe</Command>"));
        assert!(x.contains("<Arguments>--minimized</Arguments>"));
        assert!(x.contains("<Priority>5</Priority>"));
        assert!(x.contains("<ExecutionTimeLimit>PT0S</ExecutionTimeLimit>"));
        assert!(x.contains("a &amp; b"));
    }

    #[test]
    fn shortcut_task_has_no_trigger() {
        let x = task_xml("D.exe", "--switch \"cpu:a\"", false, "x");
        assert!(!x.contains("Trigger"));
        assert!(x.contains("--switch &quot;cpu:a&quot;"));
    }

    #[test]
    fn quit_task_runs_quit_without_trigger() {
        let x = task_xml(r"C:\Program Files\Deck\Deck.exe", "--quit", false, "x");
        assert!(!x.contains("Trigger"));
        assert!(x.contains("<RunLevel>HighestAvailable</RunLevel>"));
        assert!(x.contains("<Arguments>--quit</Arguments>"));
        assert_eq!(QUIT_TASK, "\\Deck\\Quit");
    }
}
