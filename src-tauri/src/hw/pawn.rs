//! Direct access to the PawnIO kernel driver (no PawnIOLib.dll needed): load a signed module, then call its ioctls.
use std::ptr::{null, null_mut};
use std::sync::OnceLock;
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Storage::FileSystem::*;
use windows_sys::Win32::System::Services::*;
use windows_sys::Win32::System::IO::DeviceIoControl;

const DEV_TYPE: u32 = 41394 << 16;
const IOCTL_LOAD: u32 = DEV_TYPE | (0x821 << 2);
const IOCTL_EXEC: u32 = DEV_TYPE | (0x841 << 2);
const NAME_LEN: usize = 32;

pub struct Module {
    h: HANDLE,
}
unsafe impl Send for Module {}
unsafe impl Sync for Module {}

// ---------------------------------------------------------------- built-in driver
// The Microsoft-signed PawnIO driver (GPL-2.0, https://github.com/namazso/PawnIO) is embedded in this exe.
// If no PawnIO driver is running, it is written to ProgramData and started as a kernel service (demand start).
// The driver cannot be unloaded while Windows runs, so it simply stays until the next reboot; uninstall removes the service.
pub static DRIVER_SYS: &[u8] = include_bytes!("mods/PawnIO.sys");
// The official signed driver package (INF + Microsoft-signed catalog) belonging to that .sys. PawnIO only creates its device
// when Windows has a "Root\PawnIO" device bound to this package, so a bare service start (below) is only a fallback.
static DRIVER_INF: &[u8] = include_bytes!("mods/PawnIO.inf");
static DRIVER_CAT: &[u8] = include_bytes!("mods/PawnIO.cat");
fn service_name() -> String {
    std::env::var("DECK_PAWN_SVC").unwrap_or_else(|_| "DeckPawnIO".into())
}

/// ProgramData\Deck: the driver files Deck writes (the driver store keeps its own copy).
fn data_dir() -> std::path::PathBuf {
    crate::util::program_data().join("Deck")
}
static READY: OnceLock<Result<(), String>> = OnceLock::new();

fn w(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn open_device() -> HANDLE {
    unsafe {
        CreateFileW(w(r"\\?\GLOBALROOT\Device\PawnIO").as_ptr(), GENERIC_READ | GENERIC_WRITE, FILE_SHARE_READ | FILE_SHARE_WRITE, null(), OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, null_mut())
    }
}

pub(crate) fn device_present() -> bool {
    let h = open_device();
    if h == INVALID_HANDLE_VALUE {
        return false;
    }
    unsafe { CloseHandle(h) };
    true
}

fn start_builtin_driver() -> Result<(), String> {
    unsafe {
        let dir = data_dir();
        let _ = std::fs::create_dir_all(&dir);
        let sys = dir.join(format!("{}.sys", service_name())); // unique file name: the kernel refuses to load the same image twice
        let same = std::fs::metadata(&sys).map(|m| m.len() == DRIVER_SYS.len() as u64).unwrap_or(false);
        if !same {
            std::fs::write(&sys, DRIVER_SYS).map_err(|e| format!("cannot write driver file: {}", e))?;
        }
        let scm = OpenSCManagerW(null(), null(), SC_MANAGER_ALL_ACCESS);
        if scm.is_null() {
            return Err(format!("cannot open service manager (0x{:X}) - run as administrator", GetLastError()));
        }
        let mut svc = CreateServiceW(scm, w(&service_name()).as_ptr(), w("Deck PawnIO driver").as_ptr(), SERVICE_ALL_ACCESS, SERVICE_KERNEL_DRIVER, SERVICE_DEMAND_START, SERVICE_ERROR_NORMAL, w(&sys.display().to_string()).as_ptr(), null(), null_mut(), null(), null(), null());
        if svc.is_null() && GetLastError() == ERROR_SERVICE_EXISTS {
            svc = OpenServiceW(scm, w(&service_name()).as_ptr(), SERVICE_ALL_ACCESS);
        }
        if svc.is_null() {
            let e = GetLastError();
            CloseServiceHandle(scm);
            return Err(format!("cannot create driver service (0x{:X})", e));
        }
        let ok = StartServiceW(svc, 0, null());
        let err = GetLastError();
        CloseServiceHandle(svc);
        CloseServiceHandle(scm);
        if ok == 0 && err != ERROR_SERVICE_ALREADY_RUNNING {
            return Err(format!("cannot start the built-in driver (0x{:X})", err));
        }
        for _ in 0..40 {
            if device_present() {
                return Ok(());
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        Err("driver started but its device did not appear".into())
    }
}

/// A "Root\PawnIO" device exists (present or not) in the SoftwareDevice class.
unsafe fn root_device_exists(class: &windows_sys::core::GUID) -> bool {
    use windows_sys::Win32::Devices::DeviceAndDriverInstallation::*;
    let set = SetupDiGetClassDevsW(class, null(), null_mut(), 0);
    if set as isize == -1 {
        return false;
    }
    let mut found = false;
    let mut i = 0;
    loop {
        let mut data: SP_DEVINFO_DATA = std::mem::zeroed();
        data.cbSize = std::mem::size_of::<SP_DEVINFO_DATA>() as u32;
        if SetupDiEnumDeviceInfo(set, i, &mut data) == 0 {
            break;
        }
        let mut buf = [0u16; 256];
        let mut need = 0u32;
        if SetupDiGetDeviceRegistryPropertyW(set, &data, SPDRP_HARDWAREID, null_mut(), buf.as_mut_ptr() as *mut u8, (buf.len() * 2) as u32, &mut need) != 0 {
            let s = String::from_utf16_lossy(&buf);
            if s.split('\0').any(|id| id.eq_ignore_ascii_case("Root\\PawnIO")) {
                found = true;
                break;
            }
        }
        i += 1;
    }
    SetupDiDestroyDeviceInfoList(set);
    found
}

/// Installs the official PawnIO driver package and creates its "Root\PawnIO" device (what PawnIO's own setup does).
/// A marker file records that Deck did it, so its uninstaller can take the driver out again.
fn install_package() -> Result<(), String> {
    use windows_sys::Win32::Devices::DeviceAndDriverInstallation::*;
    let dir = data_dir().join("pawnio");
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {}", dir.display(), e))?;
    for (name, bytes) in [("pawnio.inf", DRIVER_INF), ("PawnIO.cat", DRIVER_CAT), ("PawnIO.sys", DRIVER_SYS)] {
        std::fs::write(dir.join(name), bytes).map_err(|e| format!("cannot write {}: {}", name, e))?;
    }
    unsafe {
        let class = windows_sys::core::GUID { data1: 0x62f9c741, data2: 0xb25a, data3: 0x46ce, data4: [0xb5, 0x4c, 0x9b, 0xcc, 0xce, 0x08, 0xb6, 0xf2] }; // SoftwareDevice
        // a Root\PawnIO device from an earlier attempt is reused: one more each start would pile up
        let created = if root_device_exists(&class) {
            None
        } else {
            let set = SetupDiCreateDeviceInfoList(&class, null_mut());
            if set as isize == -1 {
                return Err(format!("device list failed (0x{:X})", GetLastError()));
            }
            let mut data: SP_DEVINFO_DATA = std::mem::zeroed();
            data.cbSize = std::mem::size_of::<SP_DEVINFO_DATA>() as u32;
            let mut hwid = w("Root\\PawnIO");
            hwid.push(0); // REG_MULTI_SZ ends with a second zero
            let made = SetupDiCreateDeviceInfoW(set, w("PawnIO").as_ptr(), &class, null(), null_mut(), DICD_GENERATE_ID, &mut data) != 0;
            let ok = made
                && SetupDiSetDeviceRegistryPropertyW(set, &mut data, SPDRP_HARDWAREID, hwid.as_ptr() as *const u8, (hwid.len() * 2) as u32) != 0
                && SetupDiCallClassInstaller(DIF_REGISTERDEVICE, set, &data) != 0;
            if !ok {
                let e = GetLastError();
                SetupDiDestroyDeviceInfoList(set);
                return Err(format!("cannot create the PawnIO device (0x{:X})", e));
            }
            Some((set, data))
        };
        let mut reboot = 0;
        let installed = UpdateDriverForPlugAndPlayDevicesW(null_mut(), w("Root\\PawnIO").as_ptr(), w(&dir.join("pawnio.inf").display().to_string()).as_ptr(), INSTALLFLAG_FORCE, &mut reboot) != 0;
        let e = GetLastError();
        if let Some((set, data)) = created {
            if !installed {
                // the driver was refused (policy, antivirus): don't leave an empty device behind
                SetupDiCallClassInstaller(DIF_REMOVE, set, &data);
            }
            SetupDiDestroyDeviceInfoList(set);
        }
        if !installed {
            return Err(format!("cannot install the PawnIO driver package (0x{:X})", e));
        }
    }
    let _ = std::fs::write(dir.join("installed-by-deck"), b"1");
    for _ in 0..60 {
        if device_present() {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    Err("PawnIO installed but its device did not appear".into())
}

/// MoboCtl (whose engine Deck took over) may have installed the driver package and would take it
/// out again when it is uninstalled, leaving Deck without fans and sensors. Deck takes over that
/// ownership: its own uninstaller removes the package instead.
fn adopt_moboctl_driver() {
    let pd = crate::util::program_data();
    let theirs = pd.join("MoboCtl").join("pawnio").join("installed-by-moboctl");
    if theirs.exists() {
        let mine = data_dir().join("pawnio");
        if std::fs::create_dir_all(&mine).is_ok() && std::fs::write(mine.join("installed-by-deck"), b"1").is_ok() {
            let _ = std::fs::remove_file(theirs);
        }
    }
}

/// Makes sure a PawnIO driver is running (an existing install is used as-is).
pub fn ensure_driver() -> Result<(), String> {
    READY
        .get_or_init(|| {
            if device_present() && std::env::var("DECK_FORCE_BUILTIN").is_err() {
                adopt_moboctl_driver();
                return Ok(());
            }
            // proper install first; the bare service start stays as a fallback
            match install_package() {
                Ok(()) => Ok(()),
                Err(e) => start_builtin_driver().map_err(|e2| format!("{} / {}", e, e2)),
            }
        })
        .clone()
}

impl Module {
    pub fn load(bin: &[u8]) -> Result<Module, String> {
        ensure_driver()?;
        unsafe {
            let h = open_device();
            if h == INVALID_HANDLE_VALUE {
                return Err("PawnIO driver not available".into());
            }
            let mut ret = 0u32;
            if DeviceIoControl(h, IOCTL_LOAD, bin.as_ptr() as *const _, bin.len() as u32, null_mut(), 0, &mut ret, null_mut()) == 0 {
                let e = GetLastError();
                CloseHandle(h);
                return Err(format!("PawnIO module load failed (0x{:X})", e));
            }
            Ok(Module { h })
        }
    }

    /// Calls an ioctl. Err carries the Win32 error code.
    pub fn exec(&self, name: &str, input: &[i64], out_len: usize) -> Result<Vec<i64>, u32> {
        let mut inb = vec![0u8; NAME_LEN + input.len() * 8];
        let nb = name.as_bytes();
        let n = nb.len().min(NAME_LEN - 1);
        inb[..n].copy_from_slice(&nb[..n]);
        for (i, v) in input.iter().enumerate() {
            inb[NAME_LEN + i * 8..NAME_LEN + i * 8 + 8].copy_from_slice(&v.to_le_bytes());
        }
        let mut outb = vec![0u8; out_len * 8];
        let mut read = 0u32;
        let ok = unsafe {
            DeviceIoControl(
                self.h,
                IOCTL_EXEC,
                inb.as_ptr() as *const _,
                inb.len() as u32,
                if outb.is_empty() { null_mut() } else { outb.as_mut_ptr() as *mut _ },
                outb.len() as u32,
                &mut read,
                null_mut(),
            )
        };
        if ok == 0 {
            return Err(unsafe { GetLastError() });
        }
        let cnt = (read as usize) / 8;
        let mut out = Vec::with_capacity(out_len);
        for i in 0..cnt.min(out_len) {
            out.push(i64::from_le_bytes(outb[i * 8..i * 8 + 8].try_into().unwrap()));
        }
        while out.len() < out_len {
            out.push(0);
        }
        Ok(out)
    }
}

impl Drop for Module {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.h);
        }
    }
}

pub static LPC_IO: &[u8] = include_bytes!("mods/LpcIO.bin");
pub static INTEL_MSR: &[u8] = include_bytes!("mods/IntelMSR.bin");
pub static SMBUS_I801: &[u8] = include_bytes!("mods/SmbusI801.bin");
pub static AMD_FAMILY17: &[u8] = include_bytes!("mods/AMDFamily17.bin");

/// Named mutex shared with other hardware tools (LibreHardwareMonitor, HWiNFO, ...) so bus access never interleaves.
pub struct BusLock(HANDLE);
unsafe impl Send for BusLock {}
unsafe impl Sync for BusLock {}
pub struct BusGuard<'a>(&'a BusLock);

impl BusLock {
    pub fn new(name: &str) -> BusLock {
        let w: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
        BusLock(unsafe { windows_sys::Win32::System::Threading::CreateMutexW(null(), 0, w.as_ptr()) })
    }
    pub fn lock(&self, ms: u32) -> Option<BusGuard<'_>> {
        if self.0.is_null() {
            return Some(BusGuard(self));
        }
        let r = unsafe { windows_sys::Win32::System::Threading::WaitForSingleObject(self.0, ms) };
        if r == WAIT_OBJECT_0 || r == WAIT_ABANDONED {
            Some(BusGuard(self))
        } else {
            None
        }
    }
}
impl Drop for BusGuard<'_> {
    fn drop(&mut self) {
        if !(self.0).0.is_null() {
            unsafe {
                windows_sys::Win32::System::Threading::ReleaseMutex((self.0).0);
            }
        }
    }
}
