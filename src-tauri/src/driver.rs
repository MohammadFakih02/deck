//! Graphics driver installs. While NVIDIA's, AMD's or Intel's setup replaces the driver, calls into
//! it (NVML, NvAPI) can fail in ways that end the calling process. So Deck watches for a driver
//! setup: while one runs every GPU call fails cleanly (`PAUSED`, see `gpu::nvml`/`gpu::nvapi`), and
//! when it has ended Deck restarts itself once so the driver is loaded fresh and the profile is
//! applied again.
//!
//! Recognising a driver setup is best effort (by where the program runs from); the restart after a
//! crash (`restart.rs`) covers a setup this list does not know.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// A driver install is running: no GPU call is made.
pub static PAUSED: AtomicBool = AtomicBool::new(false);

pub fn paused() -> bool {
    PAUSED.load(Ordering::Relaxed)
}

/// After the setup ended, wait this long before using the driver again.
const SETTLE: Duration = Duration::from_secs(20);
const POLL: Duration = Duration::from_secs(5);

/// Folders the vendors' driver setups run from (lower case).
const FOLDERS: &[&str] = &["\\nvidia\\displaydriver\\", "\\nvidia corporation\\installer2\\", "\\nvidia corporation\\downloader\\", "\\amd\\amd-software-installer\\", "\\amd\\amdsoftwareinstaller\\"];
/// Program names of the vendors' driver installers (lower case, no `.exe`).
const NAMES: &[&str] = &["amdsoftwareinstaller", "amdinstaller", "ccc-install", "nvidia app installer", "nvidiainstaller"];

/// Whether the program at `exe` (any case) is a graphics driver setup.
pub fn is_installer(exe: &str) -> bool {
    let p = exe.to_lowercase().replace('/', "\\");
    let stem = p.rsplit('\\').next().unwrap_or("").trim_end_matches(".exe");
    if NAMES.contains(&stem) {
        return true;
    }
    // the setup of a driver package, not whatever else happens to live in a vendor folder
    (stem == "setup" || stem.contains("install")) && FOLDERS.iter().any(|f| p.contains(f))
}

/// Whether a program of this name could be a setup at all (the cheap first look: only these have their
/// folder looked up).
fn could_be_setup(name: &str) -> bool {
    let n = name.to_lowercase();
    let stem = n.trim_end_matches(".exe");
    stem == "setup" || stem.contains("install") || NAMES.contains(&stem)
}

/// The running programs for which `want(path)` holds, found without reading every program's path.
pub fn running(sys: &mut sysinfo::System, want: impl Fn(&str) -> bool, name_hint: impl Fn(&str) -> bool) -> bool {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, UpdateKind};
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing());
    let pids: Vec<sysinfo::Pid> = sys.processes().iter().filter(|(_, p)| name_hint(&p.name().to_string_lossy())).map(|(pid, _)| *pid).collect();
    if pids.is_empty() {
        return false;
    }
    sys.refresh_processes_specifics(ProcessesToUpdate::Some(&pids), true, ProcessRefreshKind::nothing().with_exe(UpdateKind::OnlyIfNotSet));
    pids.iter().any(|pid| sys.process(*pid).and_then(|p| p.exe()).is_some_and(|e| want(&e.to_string_lossy())))
}

/// A graphics driver setup is running now.
pub fn installing() -> bool {
    installing_in(&mut sysinfo::System::new())
}

fn installing_in(sys: &mut sysinfo::System) -> bool {
    running(sys, is_installer, could_be_setup)
}

/// GPU calls may go again (the restart hands the GPU back on the way out, or no restart comes).
pub fn resume() {
    PAUSED.store(false, Ordering::SeqCst);
}

/// The watcher: pauses the GPU calls during a driver install, restarts Deck once after it.
/// Starts only when the PC has an NVIDIA card Deck talks to (nothing else is at risk).
/// `restart` returns false to be asked again (a game is in front); it calls `resume` itself.
pub fn start_watch(restart: impl Fn() -> bool + Send + 'static) {
    std::thread::Builder::new()
        .name("driver-watch".into())
        .spawn(move || {
            let mut seen_since: Option<Instant> = None;
            let mut ended: Option<Instant> = None;
            let mut sys = sysinfo::System::new();
            loop {
                std::thread::sleep(POLL);
                if installing_in(&mut sys) {
                    if seen_since.is_none() {
                        crate::log::info("a graphics driver install is running: GPU access paused");
                        seen_since = Some(Instant::now());
                    }
                    PAUSED.store(true, Ordering::SeqCst);
                    ended = None;
                } else if seen_since.is_some() {
                    let t = *ended.get_or_insert_with(Instant::now);
                    if t.elapsed() >= SETTLE && restart() {
                        crate::log::info("the driver install is over");
                        seen_since = None;
                        ended = None;
                    }
                }
            }
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_vendors_setups_are_recognised() {
        assert!(is_installer(r"C:\NVIDIA\DisplayDriver\572.60\Win11_Win10_64\International\setup.exe"));
        assert!(is_installer(r"C:\ProgramData\NVIDIA Corporation\Downloader\1a2b\NVI2\setup.exe"));
        assert!(is_installer(r"C:\Program Files\NVIDIA Corporation\Installer2\InstallerCore\NVI2.DLL\installer.exe"));
        assert!(is_installer(r"C:\Users\x\AppData\Local\Temp\AMDSoftwareInstaller.exe"));
        assert!(is_installer("c:/amd/amd-software-installer/setup.exe"));
    }

    #[test]
    fn only_setup_like_names_are_looked_up() {
        assert!(could_be_setup("setup.exe") && could_be_setup("Setup.EXE") && could_be_setup("installer.exe") && could_be_setup("AMDSoftwareInstaller.exe"));
        assert!(!could_be_setup("chrome.exe") && !could_be_setup("deck.exe") && !could_be_setup("nvcontainer.exe"));
    }

    #[test]
    fn everything_else_is_left_alone() {
        assert!(!is_installer(r"C:\Program Files\NVIDIA Corporation\NVIDIA app\CEF\NVIDIA app.exe"));
        assert!(!is_installer(r"C:\Windows\System32\nvcontainer.exe"));
        assert!(!is_installer(r"C:\Games\Foo\setup.exe"), "a game's setup is not a driver");
        assert!(!is_installer(r"C:\NVIDIA\DisplayDriver\572.60\Display.Driver\nvlddmkm.sys"), "a file, not a setup");
        assert!(!is_installer(""));
    }
}
