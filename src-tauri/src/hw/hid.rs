//! Minimal raw HID access (hid.dll / setupapi).
use std::ptr::{null, null_mut};
use windows_sys::core::GUID;
use windows_sys::Win32::Devices::DeviceAndDriverInstallation::*;
use windows_sys::Win32::Devices::HumanInterfaceDevice::*;
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Storage::FileSystem::*;

pub struct HidDev(HANDLE);
unsafe impl Send for HidDev {}

impl HidDev {
    /// Opens the first HID interface whose path contains `vidpid` (lower case, e.g. "vid_1462&pid_7d43").
    pub fn open(vidpid: &str) -> Option<HidDev> {
        unsafe {
            let mut g: GUID = std::mem::zeroed();
            HidD_GetHidGuid(&mut g);
            let set = SetupDiGetClassDevsW(&g, null(), null_mut(), DIGCF_PRESENT | DIGCF_DEVICEINTERFACE);
            if set as isize == -1 {
                return None;
            }
            let mut result = None;
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
                if SetupDiGetDeviceInterfaceDetailW(set, &id, buf.as_mut_ptr() as *mut _, req, &mut req, null_mut()) == 0 {
                    continue;
                }
                let wide: Vec<u16> = buf[4..].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|c| *c != 0).collect();
                let path = String::from_utf16_lossy(&wide);
                if !path.to_lowercase().contains(vidpid) {
                    continue;
                }
                let mut p16: Vec<u16> = wide.clone();
                p16.push(0);
                let h = CreateFileW(p16.as_ptr(), GENERIC_READ | GENERIC_WRITE, FILE_SHARE_READ | FILE_SHARE_WRITE, null(), OPEN_EXISTING, 0, null_mut());
                if h != INVALID_HANDLE_VALUE {
                    result = Some(HidDev(h));
                    break;
                }
            }
            SetupDiDestroyDeviceInfoList(set);
            result
        }
    }

    pub fn set_feature(&self, data: &[u8], len: usize) -> bool {
        let mut b = vec![0u8; len];
        let n = data.len().min(len);
        b[..n].copy_from_slice(&data[..n]);
        unsafe { HidD_SetFeature(self.0, b.as_mut_ptr() as *mut _, len as u32) }
    }

    pub fn get_feature(&self, id: u8, len: usize) -> Option<Vec<u8>> {
        let mut b = vec![0u8; len];
        b[0] = id;
        unsafe { HidD_GetFeature(self.0, b.as_mut_ptr() as *mut _, len as u32) }.then_some(b)
    }

    /// Plain output report (data[0] is the report id).
    pub fn write(&self, data: &[u8]) -> bool {
        let mut w = 0u32;
        unsafe { WriteFile(self.0, data.as_ptr(), data.len() as u32, &mut w, null_mut()) != 0 }
    }
}

impl Drop for HidDev {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
