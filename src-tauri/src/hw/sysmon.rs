//! HWiNFO-style sensor table: every reading with current / minimum / maximum / average,
//! plus the collectors that don't need the hardware driver (memory, drives, network, per-core load).
use std::time::Instant;

/// One reading taken this second.
pub struct Sample {
    pub group: String,
    pub name: String,
    pub unit: &'static str,
    pub dec: usize,
    pub v: Option<f64>,
}

pub fn s(group: &str, name: &str, unit: &'static str, dec: usize, v: Option<f64>) -> Sample {
    Sample { group: group.to_string(), name: name.to_string(), unit, dec, v }
}

#[derive(Clone)]
pub struct Row {
    pub group: String,
    pub name: String,
    pub unit: &'static str,
    pub dec: usize,
    pub cur: Option<f64>,
    pub min: f64,
    pub max: f64,
    pub sum: f64,
    pub n: u64,
}

impl Row {
    pub fn avg(&self) -> Option<f64> {
        (self.n > 0).then(|| self.sum / self.n as f64)
    }
    pub fn fmt(&self, v: Option<f64>) -> String {
        match v {
            Some(v) if v.is_finite() => format!("{:.*} {}", self.dec, v, self.unit).trim_end().to_string(),
            _ => "-".into(),
        }
    }
}

#[derive(Clone)]
pub struct Table {
    pub rows: Vec<Row>,
    pub since: std::time::SystemTime,
}

impl Default for Table {
    fn default() -> Self {
        Table { rows: vec![], since: std::time::SystemTime::now() }
    }
}

impl Table {
    pub fn update(&mut self, samples: Vec<Sample>) {
        for smp in samples {
            let i = match self.rows.iter().position(|r| r.group == smp.group && r.name == smp.name) {
                Some(i) => i,
                None => {
                    // keep groups together: insert after the last row of the same group
                    let at = self.rows.iter().rposition(|r| r.group == smp.group).map(|i| i + 1).unwrap_or(self.rows.len());
                    self.rows.insert(at, Row { group: smp.group.clone(), name: smp.name.clone(), unit: smp.unit, dec: smp.dec, cur: None, min: f64::MAX, max: f64::MIN, sum: 0.0, n: 0 });
                    at
                }
            };
            let r = &mut self.rows[i];
            r.cur = smp.v.filter(|v| v.is_finite());
            if let Some(v) = r.cur {
                r.min = r.min.min(v);
                r.max = r.max.max(v);
                r.sum += v;
                r.n += 1;
            }
        }
    }
    pub fn reset(&mut self) {
        for r in &mut self.rows {
            r.min = f64::MAX;
            r.max = f64::MIN;
            r.sum = 0.0;
            r.n = 0;
        }
        self.since = std::time::SystemTime::now();
    }
}

// ---------------------------------------------------------------- memory
pub fn memory() -> Vec<Sample> {
    let g = "Memory";
    let mut out = vec![];
    {
        if let Some(ms) = crate::hw::memclean::status() {
            let gb = |b: u64| b as f64 / (1u64 << 30) as f64;
            out.push(s(g, "Physical memory used", "GB", 1, Some(gb(ms.ullTotalPhys - ms.ullAvailPhys))));
            out.push(s(g, "Physical memory available", "GB", 1, Some(gb(ms.ullAvailPhys))));
            out.push(s(g, "Physical memory load", "%", 0, Some(ms.dwMemoryLoad as f64)));
            out.push(s(g, "Committed (incl. page file)", "GB", 1, Some(gb(ms.ullTotalPageFile - ms.ullAvailPageFile))));
            out.push(s(g, "Commit limit", "GB", 1, Some(gb(ms.ullTotalPageFile))));
        }
    }
    let st = crate::hw::memclean::stat();
    out.push(s(g, "Standby cache", "GB", 1, Some(st.standby_gb)));
    out.push(s(g, "Free (zeroed)", "GB", 1, Some(st.free_gb)));
    out
}

// ---------------------------------------------------------------- per logical-CPU load
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct ProcPerf {
    idle: i64,
    kernel: i64,
    user: i64,
    dpc: i64,
    interrupt: i64,
    count: u32,
    _pad: u32,
}

pub struct ThreadLoad {
    prev: Vec<ProcPerf>,
    query: Option<unsafe extern "system" fn(u32, *mut core::ffi::c_void, u32, *mut u32) -> i32>,
}

impl ThreadLoad {
    pub fn new() -> ThreadLoad {
        let f = unsafe {
            let lib: Vec<u16> = "ntdll.dll".encode_utf16().chain(Some(0)).collect();
            let h = windows_sys::Win32::System::LibraryLoader::LoadLibraryW(lib.as_ptr());
            windows_sys::Win32::System::LibraryLoader::GetProcAddress(h, b"NtQuerySystemInformation\0".as_ptr()).map(|p| std::mem::transmute(p))
        };
        ThreadLoad { prev: vec![], query: f }
    }
    /// Load % of every logical processor since the last call.
    pub fn sample(&mut self) -> Vec<f64> {
        let Some(q) = self.query else { return vec![] };
        let n = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
        let mut buf = vec![ProcPerf::default(); n];
        let mut ret = 0u32;
        if unsafe { q(8, buf.as_mut_ptr() as *mut _, (n * std::mem::size_of::<ProcPerf>()) as u32, &mut ret) } != 0 {
            return vec![];
        }
        let out = if self.prev.len() == n {
            buf.iter()
                .zip(&self.prev)
                .map(|(c, p)| {
                    let total = (c.kernel - p.kernel) + (c.user - p.user); // kernel includes idle
                    let idle = c.idle - p.idle;
                    if total > 0 { (100.0 * (1.0 - idle as f64 / total as f64)).clamp(0.0, 100.0) } else { 0.0 }
                })
                .collect()
        } else {
            vec![]
        };
        self.prev = buf;
        out
    }
}

/// Physical cores: (label, logical-processor mask). Hybrid CPUs get P-core / E-core labels.
pub fn cores() -> Vec<(String, u64)> {
    use windows_sys::Win32::System::SystemInformation::{GetLogicalProcessorInformationEx, RelationProcessorCore};
    let mut raw: Vec<(u8, u64)> = vec![];
    unsafe {
        let mut len = 0u32;
        GetLogicalProcessorInformationEx(RelationProcessorCore, std::ptr::null_mut(), &mut len);
        if len == 0 {
            return vec![];
        }
        let mut buf = vec![0u8; len as usize];
        if GetLogicalProcessorInformationEx(RelationProcessorCore, buf.as_mut_ptr() as *mut _, &mut len) == 0 {
            return vec![];
        }
        let mut off = 0usize;
        while off + 48 <= len as usize {
            let size = u32::from_le_bytes(buf[off + 4..off + 8].try_into().unwrap()) as usize;
            let class = buf[off + 9];
            let group = u16::from_le_bytes(buf[off + 40..off + 42].try_into().unwrap());
            let mask = u64::from_le_bytes(buf[off + 32..off + 40].try_into().unwrap());
            if group == 0 {
                raw.push((class, mask));
            }
            if size == 0 {
                break;
            }
            off += size;
        }
    }
    let hybrid = raw.iter().any(|r| r.0 != raw[0].0);
    let top = raw.iter().map(|r| r.0).max().unwrap_or(0);
    let (mut p, mut e) = (0, 0);
    raw.iter()
        .map(|(class, mask)| {
            let label = if !hybrid {
                p += 1;
                format!("Core {}", p - 1)
            } else if *class == top {
                p += 1;
                format!("P-core {}", p - 1)
            } else {
                e += 1;
                format!("E-core {}", e - 1)
            };
            (label, *mask)
        })
        .collect()
}

// ---------------------------------------------------------------- drives: temperature, read / write speed, activity
pub struct Drive {
    h: windows_sys::Win32::Foundation::HANDLE,
    pub name: String,
    index: u8,
    prev: Option<(i64, i64, i64, Instant)>, // bytes read, bytes written, idle time (100 ns), when
}
unsafe impl Send for Drive {}

impl Drop for Drive {
    fn drop(&mut self) {
        unsafe { windows_sys::Win32::Foundation::CloseHandle(self.h) };
    }
}

fn query_property(h: windows_sys::Win32::Foundation::HANDLE, id: i32, out: &mut [u8]) -> bool {
    use windows_sys::Win32::System::Ioctl::IOCTL_STORAGE_QUERY_PROPERTY;
    let mut q = [0u8; 12];
    q[..4].copy_from_slice(&id.to_le_bytes()); // PropertyId; QueryType 0 = standard
    let mut ret = 0u32;
    unsafe { windows_sys::Win32::System::IO::DeviceIoControl(h, IOCTL_STORAGE_QUERY_PROPERTY, q.as_ptr() as *const _, 12, out.as_mut_ptr() as *mut _, out.len() as u32, &mut ret, std::ptr::null_mut()) != 0 }
}

pub fn drives() -> Vec<Drive> {
    use windows_sys::Win32::Foundation::*;
    use windows_sys::Win32::Storage::FileSystem::*;
    let mut v = vec![];
    for i in 0..16 {
        let path: Vec<u16> = format!(r"\\.\PhysicalDrive{}", i).encode_utf16().chain(Some(0)).collect();
        let h = unsafe { CreateFileW(path.as_ptr(), GENERIC_READ | GENERIC_WRITE, FILE_SHARE_READ | FILE_SHARE_WRITE, std::ptr::null(), OPEN_EXISTING, 0, std::ptr::null_mut()) };
        if h == INVALID_HANDLE_VALUE {
            continue;
        }
        let mut buf = vec![0u8; 1024];
        let mut name = format!("Disk {}", i);
        if query_property(h, windows_sys::Win32::System::Ioctl::StorageDeviceProperty, &mut buf) {
            let off = u32::from_le_bytes(buf[16..20].try_into().unwrap()) as usize; // ProductIdOffset
            if off > 0 && off < buf.len() {
                let end = buf[off..].iter().position(|b| *b == 0).map(|p| off + p).unwrap_or(off);
                let s = String::from_utf8_lossy(&buf[off..end]).trim().to_string();
                if !s.is_empty() {
                    name = format!("{} (Disk {})", s, i);
                }
            }
        }
        v.push(Drive { h, name, index: i as u8, prev: None });
    }
    v
}

/// NVMe: composite temperature from the SMART / health log page (Kelvin at bytes 1-2).
fn nvme_temp(h: windows_sys::Win32::Foundation::HANDLE) -> Option<f64> {
    use windows_sys::Win32::System::Ioctl::*;
    let mut buf = vec![0u8; 8 + 40 + 512];
    let put = |b: &mut Vec<u8>, at: usize, v: u32| b[at..at + 4].copy_from_slice(&v.to_le_bytes());
    put(&mut buf, 0, StorageDeviceProtocolSpecificProperty as u32);
    put(&mut buf, 4, 0); // PropertyStandardQuery
    put(&mut buf, 8, ProtocolTypeNvme as u32);
    put(&mut buf, 12, NVMeDataTypeLogPage as u32);
    put(&mut buf, 16, 2); // log page 2 = SMART / health information
    put(&mut buf, 24, 40); // data follows the 40-byte protocol header
    put(&mut buf, 28, 512);
    let mut ret = 0u32;
    let len = buf.len() as u32;
    let ok = unsafe { windows_sys::Win32::System::IO::DeviceIoControl(h, IOCTL_STORAGE_QUERY_PROPERTY, buf.as_ptr() as *const _, len, buf.as_mut_ptr() as *mut _, len, &mut ret, std::ptr::null_mut()) } != 0;
    if !ok {
        return None;
    }
    let off = u32::from_le_bytes(buf[24..28].try_into().ok()?) as usize; // ProtocolDataOffset (from byte 8)
    let at = 8 + off;
    let k = u16::from_le_bytes([*buf.get(at + 1)?, *buf.get(at + 2)?]) as f64;
    let c = k - 273.0;
    (c > 0.0 && c < 150.0).then_some(c)
}

/// SATA: SMART attribute 194 (or 190) through the classic SMART read-data command.
fn ata_temp(h: windows_sys::Win32::Foundation::HANDLE, disk: u8) -> Option<f64> {
    use windows_sys::Win32::System::Ioctl::SMART_RCV_DRIVE_DATA;
    let mut inp = [0u8; 33]; // SENDCMDINPARAMS (packed)
    inp[0..4].copy_from_slice(&512u32.to_le_bytes());
    inp[4..12].copy_from_slice(&[0xD0, 1, 1, 0x4F, 0xC2, 0xA0 | ((disk & 1) << 4), 0xB0, 0]); // SMART READ DATA
    inp[12] = disk;
    let mut out = vec![0u8; 16 + 512];
    let mut ret = 0u32;
    let ok = unsafe { windows_sys::Win32::System::IO::DeviceIoControl(h, SMART_RCV_DRIVE_DATA, inp.as_ptr() as *const _, 33, out.as_mut_ptr() as *mut _, out.len() as u32, &mut ret, std::ptr::null_mut()) } != 0;
    if !ok {
        return None;
    }
    let attrs = &out[16 + 2..16 + 2 + 30 * 12];
    for id in [194u8, 190u8] {
        for a in attrs.chunks_exact(12) {
            if a[0] == id {
                let t = a[5] as f64;
                if t > 0.0 && t < 120.0 {
                    return Some(t);
                }
            }
        }
    }
    None
}

impl Drive {
    pub fn sample(&mut self) -> Vec<Sample> {
        let g = format!("Drive: {}", self.name);
        let mut out = vec![];
        let mut buf = vec![0u8; 256];
        let temp = query_property(self.h, windows_sys::Win32::System::Ioctl::StorageDeviceTemperatureProperty, &mut buf)
            .then(|| {
                let count = u16::from_le_bytes([buf[12], buf[13]]);
                (count > 0).then(|| i16::from_le_bytes([buf[26], buf[27]]) as f64)
            })
            .flatten()
            .filter(|t| *t > 0.0 && *t < 150.0)
            .or_else(|| nvme_temp(self.h))
            .or_else(|| ata_temp(self.h, self.index));
        out.push(s(&g, "Temperature", "°C", 0, temp));
        // read / write totals from the disk performance counters
        let mut perf = [0u8; 88];
        let mut ret = 0u32;
        let ok = unsafe { windows_sys::Win32::System::IO::DeviceIoControl(self.h, windows_sys::Win32::System::Ioctl::IOCTL_DISK_PERFORMANCE, std::ptr::null(), 0, perf.as_mut_ptr() as *mut _, perf.len() as u32, &mut ret, std::ptr::null_mut()) } != 0;
        if ok {
            let rd = |o: usize| i64::from_le_bytes(perf[o..o + 8].try_into().unwrap());
            let (br, bw, idle) = (rd(0), rd(8), rd(32));
            let now = Instant::now();
            if let Some((pr, pw, pi, at)) = self.prev {
                let dt = now.duration_since(at).as_secs_f64().max(0.001);
                out.push(s(&g, "Read rate", "MB/s", 1, Some((br - pr) as f64 / dt / 1e6)));
                out.push(s(&g, "Write rate", "MB/s", 1, Some((bw - pw) as f64 / dt / 1e6)));
                let idle_s = (idle - pi) as f64 / 1e7;
                out.push(s(&g, "Total activity", "%", 0, Some((100.0 * (1.0 - idle_s / dt)).clamp(0.0, 100.0))));
            }
            self.prev = Some((br, bw, idle, now));
        }
        out
    }
}

// ---------------------------------------------------------------- network adapters
pub struct Net {
    prev: Vec<(u64, u64, u64, Instant)>, // luid, in, out
    total: Vec<(u64, u64, u64)>,         // luid, bytes in, bytes out since start
}

impl Net {
    pub fn new() -> Net {
        Net { prev: vec![], total: vec![] }
    }
    pub fn sample(&mut self) -> Vec<Sample> {
        use windows_sys::Win32::NetworkManagement::IpHelper::*;
        let mut out = vec![];
        unsafe {
            let mut t: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
            if GetIfTable2(&mut t) != 0 || t.is_null() {
                return out;
            }
            let n = (*t).NumEntries as usize;
            let rows = std::slice::from_raw_parts((*t).Table.as_ptr(), n);
            let now = Instant::now();
            for r in rows {
                let hardware = r.InterfaceAndOperStatusFlags._bitfield & 1 != 0;
                if !hardware || r.OperStatus != 1 || (r.Type != 6 && r.Type != 71) {
                    continue; // only real, connected Ethernet / Wi-Fi adapters
                }
                let luid = r.InterfaceLuid.Value;
                let name = String::from_utf16_lossy(&r.Alias[..r.Alias.iter().position(|c| *c == 0).unwrap_or(0)]);
                let g = format!("Network: {}", name);
                if let Some(p) = self.prev.iter_mut().find(|p| p.0 == luid) {
                    let dt = now.duration_since(p.3).as_secs_f64().max(0.001);
                    let (din, dout) = (r.InOctets.saturating_sub(p.1), r.OutOctets.saturating_sub(p.2));
                    out.push(s(&g, "Download rate", "KB/s", 1, Some(din as f64 / dt / 1024.0)));
                    out.push(s(&g, "Upload rate", "KB/s", 1, Some(dout as f64 / dt / 1024.0)));
                    if let Some(tt) = self.total.iter_mut().find(|t| t.0 == luid) {
                        tt.1 += din;
                        tt.2 += dout;
                        out.push(s(&g, "Downloaded (since start)", "MB", 1, Some(tt.1 as f64 / 1048576.0)));
                        out.push(s(&g, "Uploaded (since start)", "MB", 1, Some(tt.2 as f64 / 1048576.0)));
                    }
                    *p = (luid, r.InOctets, r.OutOctets, now);
                } else {
                    self.prev.push((luid, r.InOctets, r.OutOctets, now));
                    self.total.push((luid, 0, 0));
                }
            }
            FreeMibTable(t as *const _);
        }
        out
    }
}
