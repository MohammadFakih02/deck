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

/// The task's XML in a new file with a random name, in Windows' own Temp folder (the user's TEMP
/// when Deck has no admin rights), returned open: while it is held nobody else can write, delete or
/// rename it, so schtasks (with admin rights) reads exactly what Deck wrote.
fn task_file(xml: &str) -> std::io::Result<(std::path::PathBuf, std::fs::File)> {
    use std::io::Write;
    use std::os::windows::fs::OpenOptionsExt;
    const FILE_SHARE_READ: u32 = 1;
    let mut bytes: Vec<u8> = vec![0xFF, 0xFE];
    for u in xml.encode_utf16() {
        bytes.extend_from_slice(&u.to_le_bytes());
    }
    let name = format!("deck_task_{}.xml", crate::util::random_token(24));
    let make = |dir: std::path::PathBuf| {
        let p = dir.join(&name);
        let mut f = std::fs::OpenOptions::new().write(true).create_new(true).share_mode(FILE_SHARE_READ).open(&p)?;
        f.write_all(&bytes)?;
        f.sync_all()?;
        drop(f);
        // held for reading only (schtasks may not share with a writer); a file Deck made in Windows'
        // Temp isn't the user's to open in between
        let held = std::fs::OpenOptions::new().read(true).share_mode(FILE_SHARE_READ).open(&p)?;
        Ok::<_, std::io::Error>((p, held))
    };
    make(crate::util::windows_dir().join("Temp")).or_else(|_| make(std::env::temp_dir()))
}

/// Registers (or replaces) a task from XML. Needs admin.
pub fn create_task(name: &str, xml: &str) -> Result<(), String> {
    let (path, held) = task_file(xml).map_err(|e| e.to_string())?;
    let ok = quiet("schtasks.exe", &["/create", "/tn", name, "/xml", &path.display().to_string(), "/f"]);
    drop(held);
    let _ = std::fs::remove_file(&path);
    if ok {
        crate::log::info(format!("scheduled task {name} set"));
        Ok(())
    } else {
        crate::log::warn(format!("scheduled task {name}: Windows refused to create it"));
        Err("Windows refused to create the scheduled task.".into())
    }
}

pub fn delete_task(name: &str) {
    if quiet("schtasks.exe", &["/delete", "/tn", name, "/f"]) {
        crate::log::info(format!("scheduled task {name} removed"));
    }
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

/// The trigger-less task that starts Deck elevated and hidden in the tray (`--minimized`): used by
/// the setup, the watchdog after a crash and the updater, so none of them puts a window in front.
pub const BACKGROUND_TASK: &str = "\\Deck\\Background";

fn create_background_task(exe: &Path) -> Result<(), String> {
    create_task(BACKGROUND_TASK, &task_xml(&exe.display().to_string(), "--minimized", false, "Starts Deck hidden with admin rights (setup, updates, restart after a crash)."))
}

/// Creates the Deck tasks for `exe`. Called from `Deck.exe --setup-task` (elevated).
pub fn setup(exe: &Path, autostart: bool) -> Result<(), String> {
    if crate::fsguard::user_could_change(exe).map_err(|e| format!("Couldn't check Deck's folder: {e}"))? {
        crate::log::warn(format!("set up refused: {} can be changed without admin rights", exe.display()));
        return Err(UNSAFE_PLACE.into());
    }
    ensure_task(exe, autostart)?;
    create_task(OPEN_TASK, &task_xml(&exe.display().to_string(), "", false, "Opens Deck with admin rights (used by the Start menu shortcut)."))?;
    create_background_task(exe)?;
    create_quit_task(exe)
}

/// Why Set up refuses (and why the tasks were taken away).
pub const UNSAFE_PLACE: &str = "Deck is installed in a folder your everyday account can change, so starting it with admin rights would let any program get admin rights too. Install Deck with the current setup (it goes to Program Files), then use Set up again.";

/// The task `name` starts `exe`.
fn task_runs(name: &str, exe: &Path) -> bool {
    let want = esc_xml(&exe.display().to_string().to_lowercase());
    let xml = Command::new("schtasks.exe").args(["/query", "/tn", name, "/xml"]).creation_flags(NO_WINDOW).output();
    xml.map(|o| o.status.success() && String::from_utf8_lossy(&o.stdout).to_lowercase().contains(&want)).unwrap_or(false)
}

/// At an elevated start: tasks that start this copy of Deck with admin rights while the everyday
/// account could change it (installed per user by an older setup) are taken away; Some(why) then.
/// Otherwise installs made before the Quit task existed get it (and again when Deck moved to
/// another folder, so the task never points at a missing exe).
pub fn ensure_quit_task() -> Option<String> {
    if !is_admin() || !task_exists(TASK) {
        return None;
    }
    let Ok(exe) = std::env::current_exe() else { return None };
    if task_runs(TASK, &exe) && crate::fsguard::user_could_change(&exe).unwrap_or(false) {
        for t in [TASK, OPEN_TASK, QUIT_TASK, BACKGROUND_TASK] {
            delete_task(t);
        }
        crate::log::warn(format!("admin tasks removed: {} can be changed without admin rights", exe.display()));
        return Some(UNSAFE_PLACE.into());
    }
    // only the copy the logon task runs, in a folder only admins can change, may own the other tasks:
    // a dev or portable copy started with admin rights must not point them at itself
    if !task_runs(TASK, &exe) || crate::fsguard::user_could_change(&exe).unwrap_or(true) {
        return None;
    }
    if !task_runs(QUIT_TASK, &exe) {
        let _ = create_quit_task(&exe);
    }
    if !task_runs(BACKGROUND_TASK, &exe) {
        let _ = create_background_task(&exe);
    }
    None
}

/// The setup runs as the account signed in at the screen. Not so when a standard user typed an
/// administrator's password at the UAC prompt: the setup then runs as that administrator, and tasks
/// made now would belong to the wrong account. Some(false) when this process's account has no
/// Explorer running while another one does; None when it can't be told.
pub fn runs_as_desktop_user() -> Option<bool> {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
    let mut sys = System::new();
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing().with_user(UpdateKind::Always));
    let me = sys.process(Pid::from_u32(std::process::id()))?.user_id()?.clone();
    let shells: Vec<_> = sys.processes().values().filter(|p| p.name().to_string_lossy().eq_ignore_ascii_case("explorer.exe")).filter_map(|p| p.user_id().cloned()).collect();
    if shells.is_empty() {
        return None;
    }
    Some(shells.contains(&me))
}

/// How long after a handover another one is refused (the task did not start Deck elevated).
const HANDOVER_GAP: std::time::Duration = std::time::Duration::from_secs(20);

/// Whether an unelevated start should be passed to the admin task: only a plain start (no arguments,
/// or just `--minimized`), only for the copy the task runs, and not again right after a handover
/// (a task that doesn't give admin rights would otherwise start Deck in a loop).
pub fn handover_task(admin: bool, args: &[String], opens_this_exe: bool, since_last: Option<std::time::Duration>) -> Option<&'static str> {
    if admin || !opens_this_exe || since_last.is_some_and(|d| d < HANDOVER_GAP) {
        return None;
    }
    match args {
        [] => Some(OPEN_TASK),
        [a] if a == "--minimized" => Some(BACKGROUND_TASK),
        _ => None,
    }
}

/// At the start of an unelevated Deck whose admin tasks exist: starts Deck through them and says so
/// (the caller then exits). The finish page of the setup, the Start menu and shortcuts all start Deck
/// as the user; this is what makes that the same as starting it with admin rights.
pub fn handover() -> bool {
    if is_admin() {
        return false;
    }
    let Ok(exe) = std::env::current_exe() else { return false };
    let args: Vec<String> = std::env::args().skip(1).collect();
    let marker = crate::util::local_appdata().join("Deck").join("handover");
    let since = std::fs::metadata(&marker).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok());
    // cheap checks first: the task query below starts a process
    if handover_task(false, &args, true, since).is_none() || !task_exists(OPEN_TASK) || !task_runs(OPEN_TASK, &exe) {
        return false;
    }
    let task = handover_task(false, &args, true, since).unwrap_or(OPEN_TASK);
    let _ = std::fs::create_dir_all(marker.parent().unwrap_or(Path::new(".")));
    let _ = std::fs::write(&marker, b"");
    if run_task(task) {
        crate::log::info(format!("started without admin rights: handed over to {task}"));
        true
    } else {
        false
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
mod task_file_tests {
    /// The task's XML is read by schtasks with admin rights: nobody may change it in between, and
    /// its name can't be guessed in advance.
    #[test]
    fn the_task_file_cannot_be_swapped() {
        let (p, held) = super::task_file("<Task/>").unwrap();
        let (q, held2) = super::task_file("<Task/>").unwrap();
        assert_ne!(p, q, "a fresh random name each time");
        assert!(p.file_name().unwrap().to_string_lossy().len() > 30);
        assert!(std::fs::OpenOptions::new().write(true).open(&p).is_err(), "can't be written while schtasks may read it");
        assert!(std::fs::remove_file(&p).is_err(), "nor deleted or replaced");
        let text = std::fs::read(&p).unwrap();
        assert_eq!(&text[..2], &[0xFF, 0xFE], "UTF-16 with its mark, as schtasks wants");
        drop((held, held2));
        std::fs::remove_file(&p).unwrap();
        std::fs::remove_file(&q).unwrap();
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
    fn an_unelevated_start_goes_to_the_admin_task() {
        let none: Vec<String> = vec![];
        let min = vec!["--minimized".to_string()];
        let other = vec!["--switch".to_string(), "cpu:a".to_string()];
        let secs = |n| Some(std::time::Duration::from_secs(n));
        assert_eq!(handover_task(false, &none, true, None), Some(OPEN_TASK));
        assert_eq!(handover_task(false, &min, true, None), Some(BACKGROUND_TASK), "hidden stays hidden");
        assert_eq!(handover_task(false, &none, true, secs(60)), Some(OPEN_TASK), "an old marker doesn't count");
        assert_eq!(handover_task(true, &none, true, None), None, "already admin");
        assert_eq!(handover_task(false, &none, false, None), None, "the task runs another copy");
        assert_eq!(handover_task(false, &other, true, None), None, "commands keep their own path");
        assert_eq!(handover_task(false, &none, true, secs(3)), None, "no loop when the task isn't elevated");
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
