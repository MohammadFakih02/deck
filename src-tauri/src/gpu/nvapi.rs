//! NVIDIA's NvAPI (nvapi64.dll) for what NVML can't do: the voltage/frequency curve and the
//! core voltage boost. These calls are undocumented (IDs and layouts from the community
//! nvapi-sys bindings and Afterburner-compatible tools), so every write is read back.

use std::ffi::c_void;
use windows::core::PCSTR;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::System::LibraryLoader::GetProcAddress;

pub const OK: i32 = 0;

// public
const INITIALIZE: u32 = 0x0150_E828;
const ENUM_PHYSICAL_GPUS: u32 = 0xE5AC_921F;
// undocumented (nvapi-sys nvid.rs)
pub const VF_MASK: u32 = 0x507B_4B59; // ClockBoostMask / ClkVfPointsGetInfo
pub const VF_TABLE_GET: u32 = 0x23F1_B133; // ClockBoostTable / ClkVfPointsGetControl
pub const VF_TABLE_SET: u32 = 0x0733_E009; // ClockBoostTable / ClkVfPointsSetControl
pub const VF_CURVE: u32 = 0x2153_7AD4; // VFPCurve / ClkVfPointsGetStatus
pub const VOLT_BOOST_GET: u32 = 0x9DF2_3CA1;
pub const VOLT_BOOST_SET: u32 = 0xB930_6D9B;
pub const CUR_VOLTAGE: u32 = 0x465F_9BCF;

/// Sizes of the versioned structs we use. The version word is `size | ver << 16`.
pub const VF_MASK_SIZE: usize = 0x182C; // v1
pub const VF_TABLE_SIZE: usize = 0x2420; // v1
pub const VF_CURVE_SIZES: [(u32, usize); 3] = [(1, 0x1C28), (2, 0x1C28), (3, 0x15B0C)];
pub const VOLT_BOOST_SIZE: usize = 40; // v1
pub const CUR_VOLTAGE_SIZE: usize = 0x4C; // v1

type QueryFn = unsafe extern "C" fn(u32) -> *mut c_void;

pub struct NvApi {
    q: QueryFn,
    pub gpu: *mut c_void,
}

unsafe impl Send for NvApi {}
unsafe impl Sync for NvApi {}

pub fn ver(size: usize, v: u32) -> u32 {
    size as u32 | (v << 16)
}

impl NvApi {
    pub fn load() -> Option<NvApi> {
        unsafe {
            // from System32 only: Deck runs with admin rights (see util::trusted_dll_paths)
            let m = HMODULE(crate::util::load_trusted_dll("nvapi64.dll"));
            if m.is_invalid() {
                return None;
            }
            let q: QueryFn = std::mem::transmute(GetProcAddress(m, PCSTR(c"nvapi_QueryInterface".as_ptr() as *const u8))?);
            let init: unsafe extern "C" fn() -> i32 = std::mem::transmute(non_null(q(INITIALIZE))?);
            if init() != OK {
                return None;
            }
            let enum_gpus: unsafe extern "C" fn(*mut *mut c_void, *mut u32) -> i32 = std::mem::transmute(non_null(q(ENUM_PHYSICAL_GPUS))?);
            let mut handles = [std::ptr::null_mut::<c_void>(); 64];
            let mut n = 0u32;
            if enum_gpus(handles.as_mut_ptr(), &mut n) != OK || n == 0 {
                return None;
            }
            Some(NvApi { q, gpu: handles[0] })
        }
    }

    fn func(&self, id: u32) -> Option<unsafe extern "C" fn(*mut c_void, *mut u8) -> i32> {
        unsafe { non_null((self.q)(id)).map(|p| std::mem::transmute(p)) }
    }

    pub fn has(&self, id: u32) -> bool {
        self.func(id).is_some()
    }

    /// Calls a "get" function with a zeroed buffer of `size` whose first word is the version.
    pub fn get_raw(&self, id: u32, size: usize, v: u32) -> Result<Vec<u8>, i32> {
        let f = self.func(id).ok_or(-3)?;
        let mut buf = vec![0u8; size];
        buf[..4].copy_from_slice(&ver(size, v).to_le_bytes());
        let r = unsafe { f(self.gpu, buf.as_mut_ptr()) };
        if r == OK {
            Ok(buf)
        } else {
            Err(r)
        }
    }

    /// Like `get_raw`, but with the point mask (32 bytes at offset 4) filled in first: the
    /// VF table/curve calls only return the points the mask asks for.
    pub fn get_masked(&self, id: u32, size: usize, v: u32, mask: &[u8]) -> Result<Vec<u8>, i32> {
        let f = self.func(id).ok_or(-3)?;
        let mut buf = vec![0u8; size];
        buf[..4].copy_from_slice(&ver(size, v).to_le_bytes());
        buf[4..4 + mask.len()].copy_from_slice(mask);
        let r = unsafe { f(self.gpu, buf.as_mut_ptr()) };
        if r == OK {
            Ok(buf)
        } else {
            Err(r)
        }
    }

    /// The 32-byte mask of curve points this GPU has.
    pub fn vf_mask(&self) -> Result<Vec<u8>, i32> {
        self.get_raw(VF_MASK, VF_MASK_SIZE, 1).map(|b| b[4..36].to_vec())
    }

    /// Calls a "set" function with a prepared buffer (version word already in place).
    pub fn set_raw(&self, id: u32, buf: &mut [u8]) -> Result<(), i32> {
        let f = self.func(id).ok_or(-3)?;
        let r = unsafe { f(self.gpu, buf.as_mut_ptr()) };
        if r == OK {
            Ok(())
        } else {
            Err(r)
        }
    }

    /// The VF curve status (version detected on first success).
    pub fn vf_curve_raw(&self) -> Result<(u32, Vec<u8>), i32> {
        let mask = self.vf_mask()?;
        let mut last = -3;
        for (v, size) in VF_CURVE_SIZES {
            match self.get_masked(VF_CURVE, size, v, &mask) {
                Ok(b) => return Ok((v, b)),
                Err(e) => last = e,
            }
        }
        Err(last)
    }

    pub fn volt_boost(&self) -> Result<u32, i32> {
        self.get_raw(VOLT_BOOST_GET, VOLT_BOOST_SIZE, 1).map(|b| u32_at(&b, 4))
    }

    pub fn set_volt_boost(&self, pct: u32) -> Result<(), i32> {
        let mut b = vec![0u8; VOLT_BOOST_SIZE];
        b[..4].copy_from_slice(&ver(VOLT_BOOST_SIZE, 1).to_le_bytes());
        b[4..8].copy_from_slice(&pct.min(100).to_le_bytes());
        self.set_raw(VOLT_BOOST_SET, &mut b)
    }

    /// Core voltage right now (millivolts).
    pub fn voltage_mv(&self) -> Option<f32> {
        let b = self.get_raw(CUR_VOLTAGE, CUR_VOLTAGE_SIZE, 1).ok()?;
        let uv = u32_at(&b, 0x28);
        (uv > 0).then(|| uv as f32 / 1000.0)
    }
}

pub fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

pub fn i32_at(b: &[u8], o: usize) -> i32 {
    u32_at(b, o) as i32
}

fn non_null(p: *mut c_void) -> Option<*mut c_void> {
    (!p.is_null()).then_some(p)
}
