//! HID plumbing for the battery readers: listing interfaces, what each one is, and overlapped
//! reads and writes that give up after a timeout (a controller that is off never answers).

use std::ptr::{null, null_mut};
use windows_sys::core::GUID;
use windows_sys::Win32::Devices::DeviceAndDriverInstallation::*;
use windows_sys::Win32::Devices::HumanInterfaceDevice::*;
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Storage::FileSystem::*;
use windows_sys::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
use windows_sys::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};

/// One HID interface: its path and the device node it belongs to.
#[derive(Clone, Debug)]
pub struct Iface {
    pub path: String,
    pub devinst: u32,
}

/// Every present HID interface.
pub fn list() -> Vec<Iface> {
    let mut out = Vec::new();
    unsafe {
        let mut g: GUID = std::mem::zeroed();
        HidD_GetHidGuid(&mut g);
        let set = SetupDiGetClassDevsW(&g, null(), null_mut(), DIGCF_PRESENT | DIGCF_DEVICEINTERFACE);
        if set as isize == -1 {
            return out;
        }
        let mut i = 0;
        loop {
            let mut id: SP_DEVICE_INTERFACE_DATA = std::mem::zeroed();
            id.cbSize = std::mem::size_of::<SP_DEVICE_INTERFACE_DATA>() as u32;
            if SetupDiEnumDeviceInterfaces(set, null(), &g, i, &mut id) == 0 {
                break;
            }
            i += 1;
            let mut req = 0u32;
            SetupDiGetDeviceInterfaceDetailW(set, &id, null_mut(), 0, &mut req, null_mut());
            if req == 0 {
                continue;
            }
            let mut buf = vec![0u8; req as usize + 8];
            // SP_DEVICE_INTERFACE_DETAIL_DATA_W: cbSize (4) then the wide path; cbSize is 8 on 64-bit
            buf[..4].copy_from_slice(&8u32.to_le_bytes());
            let mut dev: SP_DEVINFO_DATA = std::mem::zeroed();
            dev.cbSize = std::mem::size_of::<SP_DEVINFO_DATA>() as u32;
            if SetupDiGetDeviceInterfaceDetailW(set, &id, buf.as_mut_ptr() as *mut _, req, &mut req, &mut dev) == 0 {
                continue;
            }
            let wide: Vec<u16> = buf[4..].as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).take_while(|c| *c != 0).collect();
            out.push(Iface { path: String::from_utf16_lossy(&wide), devinst: dev.DevInst });
        }
        SetupDiDestroyDeviceInfoList(set);
    }
    out
}

fn hex_after(s: &str, key: &str, digits: usize) -> Option<u16> {
    let at = s.find(key)? + key.len();
    u16::from_str_radix(s.get(at..at + digits)?, 16).ok()
}

/// VID and PID from an interface path: `vid_054c&pid_09cc` (USB) or `vid&0002054c_pid&09cc`
/// (Bluetooth, where the VID carries a 2-byte source prefix).
pub fn ids(path: &str) -> Option<(u16, u16)> {
    let p = path.to_ascii_lowercase();
    if let (Some(v), Some(d)) = (hex_after(&p, "vid_", 4), hex_after(&p, "pid_", 4)) {
        return Some((v, d));
    }
    let v = p.find("vid&").and_then(|i| p.get(i + 4..i + 12)).and_then(|s| u32::from_str_radix(s, 16).ok())? as u16;
    Some((v, hex_after(&p, "pid&", 4)?))
}

/// Bluetooth (classic HID or BLE HOGP), from the interface path.
pub fn is_bluetooth(path: &str) -> bool {
    let p = path.to_ascii_lowercase();
    p.contains("{00001124-0000-1000-8000-00805f9b34fb}") || p.contains("{00001812-0000-1000-8000-00805f9b34fb}")
}

fn wide_z(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// Usage page and usage of the interface's top-level collection, and its product string.
/// Opened for no access at all, so it works on devices another program holds.
pub fn describe(path: &str) -> Option<(u16, u16, String)> {
    unsafe {
        let w = wide_z(path);
        let h = CreateFileW(w.as_ptr(), 0, FILE_SHARE_READ | FILE_SHARE_WRITE, null(), OPEN_EXISTING, 0, null_mut());
        if h == INVALID_HANDLE_VALUE {
            return None;
        }
        let mut pre: PHIDP_PREPARSED_DATA = 0;
        let mut usage = None;
        if HidD_GetPreparsedData(h, &mut pre) {
            let mut caps: HIDP_CAPS = std::mem::zeroed();
            if HidP_GetCaps(pre, &mut caps) == HIDP_STATUS_SUCCESS {
                usage = Some((caps.UsagePage, caps.Usage));
            }
            HidD_FreePreparsedData(pre);
        }
        let mut name = [0u16; 128];
        let product = if HidD_GetProductString(h, name.as_mut_ptr() as *mut _, (name.len() * 2) as u32) {
            String::from_utf16_lossy(&name[..name.iter().position(|c| *c == 0).unwrap_or(name.len())]).trim().to_string()
        } else {
            String::new()
        };
        CloseHandle(h);
        usage.map(|(p, u)| (p, u, product))
    }
}

/// The device node's parent, or None at the top.
pub fn parent(devinst: u32) -> Option<u32> {
    let mut p = 0u32;
    (unsafe { CM_Get_Parent(&mut p, devinst, 0) } == CR_SUCCESS).then_some(p)
}

/// The device instance id ("USB\VID_054C&PID_09CC\…"), upper case.
pub fn instance_id(devinst: u32) -> String {
    let mut buf = [0u16; 512];
    if unsafe { CM_Get_Device_IDW(devinst, buf.as_mut_ptr(), buf.len() as u32, 0) } != CR_SUCCESS {
        return String::new();
    }
    String::from_utf16_lossy(&buf[..buf.iter().position(|c| *c == 0).unwrap_or(buf.len())]).to_ascii_uppercase()
}

/// A virtual pad made by ViGEm (DS4Windows and similar tools): its USB device hangs off the
/// ViGEm bus, a root-enumerated device, instead of a real USB hub.
pub fn is_virtual(devinst: u32) -> bool {
    let mut d = devinst;
    for _ in 0..3 {
        let Some(p) = parent(d) else { return false };
        let id = instance_id(p);
        if id.contains("VIGEM") || id.starts_with("ROOT\\SYSTEM\\") {
            return true;
        }
        d = p;
    }
    false
}

/// A game pad or joystick collection (generic desktop page, usage 4 or 5).
pub fn is_gamepad(usage_page: u16, usage: u16) -> bool {
    usage_page == 1 && (usage == 4 || usage == 5)
}

/// An open interface for overlapped reads and writes.
pub struct Dev {
    h: HANDLE,
    ev: HANDLE,
}
unsafe impl Send for Dev {}

impl Dev {
    /// Shared with every other program; `write` asks for write access too.
    pub fn open(path: &str, write: bool) -> Option<Dev> {
        unsafe {
            let w = wide_z(path);
            let access = if write { GENERIC_READ | GENERIC_WRITE } else { GENERIC_READ };
            let h = CreateFileW(w.as_ptr(), access, FILE_SHARE_READ | FILE_SHARE_WRITE, null(), OPEN_EXISTING, FILE_FLAG_OVERLAPPED, null_mut());
            if h == INVALID_HANDLE_VALUE {
                return None;
            }
            let ev = CreateEventW(null(), 1, 0, null());
            if ev.is_null() {
                CloseHandle(h);
                return None;
            }
            Some(Dev { h, ev })
        }
    }

    /// Drops queued input reports, so the next read is fresh.
    pub fn flush(&self) {
        unsafe {
            HidD_FlushQueue(self.h);
        }
    }

    /// One overlapped transfer: Ok(Some(bytes)), Ok(None) on timeout, Err when the device went away.
    fn io(&self, buf: *mut u8, len: usize, write: bool, timeout_ms: u32) -> Result<Option<usize>, u32> {
        unsafe {
            let mut ov: OVERLAPPED = std::mem::zeroed();
            ov.hEvent = self.ev;
            let mut n = 0u32;
            let ok = if write { WriteFile(self.h, buf, len as u32, &mut n, &mut ov) } else { ReadFile(self.h, buf, len as u32, &mut n, &mut ov) };
            if ok == 0 {
                let err = GetLastError();
                if err != ERROR_IO_PENDING {
                    return Err(err);
                }
                if WaitForSingleObject(self.ev, timeout_ms) != WAIT_OBJECT_0 {
                    CancelIoEx(self.h, &ov);
                    GetOverlappedResult(self.h, &ov, &mut n, 1);
                    return Ok(None);
                }
                if GetOverlappedResult(self.h, &ov, &mut n, 0) == 0 {
                    return Err(GetLastError());
                }
            }
            Ok(Some(n as usize))
        }
    }

    pub fn read(&self, buf: &mut [u8], timeout_ms: u32) -> Result<Option<usize>, u32> {
        self.io(buf.as_mut_ptr(), buf.len(), false, timeout_ms)
    }

    pub fn write(&self, data: &[u8], timeout_ms: u32) -> Result<Option<usize>, u32> {
        let mut copy = data.to_vec();
        self.io(copy.as_mut_ptr(), copy.len(), true, timeout_ms)
    }
}

impl Drop for Dev {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.h);
            CloseHandle(self.ev);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_from_usb_and_bluetooth_paths() {
        assert_eq!(ids(r"\\?\hid#vid_054c&pid_09cc&mi_03#8&1f&0&0000#{4d1e55b2-f16f-11cf-88cb-001111000030}"), Some((0x054c, 0x09cc)));
        assert_eq!(ids(r"\\?\hid#{00001124-0000-1000-8000-00805f9b34fb}_vid&0002054c_pid&05c4#9&2&0&0000#{4d1e55b2}"), Some((0x054c, 0x05c4)));
        assert_eq!(ids(r"\\?\hid#vid_3537&pid_1098&mi_02#x"), Some((0x3537, 0x1098)));
        assert_eq!(ids("nothing here"), None);
    }

    #[test]
    fn bluetooth_paths_are_recognised() {
        assert!(is_bluetooth(r"\\?\hid#{00001124-0000-1000-8000-00805f9b34fb}_vid&0002054c_pid&05c4#x"));
        assert!(!is_bluetooth(r"\\?\hid#vid_054c&pid_09cc#x"));
    }
}
