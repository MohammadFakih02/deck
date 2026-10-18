//! SMBIOS (the firmware's hardware table): the PC's maker/model and form factor, motherboard
//! maker/model, BIOS version and every DIMM's maker/part number.
use windows_sys::Win32::System::SystemInformation::GetSystemFirmwareTable;

#[derive(Clone, Default, Debug)]
pub struct Info {
    pub board_vendor: String,
    pub board_name: String,
    pub bios: String,
    pub dimms: Vec<(String, String)>, // (manufacturer, part number) of populated slots
    /// the whole PC's maker and model (a laptop's "HP" / "Victus 16"; often a placeholder on self-built PCs)
    pub sys_vendor: String,
    pub sys_product: String,
    /// SMBIOS chassis type (3 desktop, 9 laptop, 10 notebook, …; 0 unknown)
    pub chassis: u8,
}

impl Info {
    pub fn is_msi(&self) -> bool {
        let v = self.board_vendor.to_ascii_lowercase();
        v.contains("micro-star") || v.contains("msi")
    }
    /// "Laptop", "Desktop", … from the chassis type.
    pub fn form_factor(&self) -> &'static str {
        match self.chassis {
            8 | 9 | 10 | 14 => "Laptop",
            30..=32 => "Tablet / 2-in-1",
            3 | 4 | 5 | 6 | 7 | 15 | 16 | 35 | 36 => "Desktop",
            13 => "All-in-one",
            17 | 23 | 28 | 29 => "Server",
            _ => "Unknown",
        }
    }
    /// MSI AM5 / LGA1851 boards whose fan chip (NCT6687DR) uses a different register map.
    pub fn is_msi_800(&self) -> bool {
        let n = self.board_name.to_ascii_uppercase();
        self.is_msi() && ["B840", "B850", "B860", "X870", "Z890"].iter().any(|m| n.contains(m))
    }
}

pub fn read() -> Info {
    let mut info = Info::default();
    let sig = u32::from_le_bytes(*b"BMSR"); // 'RSMB'
    unsafe {
        let n = GetSystemFirmwareTable(sig, 0, std::ptr::null_mut(), 0);
        if n < 8 {
            return info;
        }
        let mut buf = vec![0u8; n as usize];
        if GetSystemFirmwareTable(sig, 0, buf.as_mut_ptr() as *mut _, n) == 0 {
            return info;
        }
        let len = u32::from_le_bytes(buf[4..8].try_into().unwrap()) as usize;
        let t = &buf[8..(8 + len).min(buf.len())];
        let mut i = 0;
        while i + 4 <= t.len() {
            let (ty, l) = (t[i], t[i + 1] as usize);
            if l < 4 || i + l > t.len() {
                break;
            }
            let mut j = i + l; // the string set follows the formatted area, ended by a double zero
            let mut strs: Vec<String> = Vec::new();
            if j + 1 < t.len() && t[j] == 0 && t[j + 1] == 0 {
                j += 2;
            } else {
                loop {
                    let s = j;
                    while j < t.len() && t[j] != 0 {
                        j += 1;
                    }
                    strs.push(String::from_utf8_lossy(&t[s..j]).trim().to_string());
                    j += 1;
                    if j >= t.len() || t[j] == 0 {
                        j += 1;
                        break;
                    }
                }
            }
            let get = |idx: u8| if idx == 0 { String::new() } else { strs.get(idx as usize - 1).cloned().unwrap_or_default() };
            match ty {
                0 if l > 5 => info.bios = format!("{} {}", get(t[i + 4]), get(t[i + 5])).trim().to_string(),
                1 if l > 5 => {
                    info.sys_vendor = get(t[i + 4]);
                    info.sys_product = get(t[i + 5]);
                }
                3 if l > 5 => info.chassis = t[i + 5] & 0x7F,
                2 if l > 5 => {
                    info.board_vendor = get(t[i + 4]);
                    info.board_name = get(t[i + 5]);
                }
                17 if l > 0x1A => {
                    let size = u16::from_le_bytes([t[i + 0x0C], t[i + 0x0D]]);
                    if size != 0 && size != 0xFFFF {
                        info.dimms.push((get(t[i + 0x17]), get(t[i + 0x1A])));
                    }
                }
                127 => break,
                _ => {}
            }
            i = j;
        }
    }
    info
}
