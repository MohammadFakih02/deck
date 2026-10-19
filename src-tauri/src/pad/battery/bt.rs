//! The battery level Windows shows in Bluetooth settings (DEVPKEY_Bluetooth_Battery), for
//! Bluetooth controllers that report it. It lives on the Bluetooth device node a level or two
//! above the controller's HID interface.

use super::hid;
use super::{Entry, Reading};
use windows_sys::core::GUID;
use windows_sys::Win32::Devices::DeviceAndDriverInstallation::{CM_Get_DevNode_PropertyW, CR_SUCCESS};
use windows_sys::Win32::Devices::Properties::DEVPROPTYPE;
use windows_sys::Win32::Foundation::DEVPROPKEY;

const BATTERY: DEVPROPKEY = DEVPROPKEY {
    fmtid: GUID::from_u128(0x104ea319_6ee2_4701_bd47_8ddbf425bbe5),
    pid: 2,
};

fn battery_of(devinst: u32) -> Option<u8> {
    let mut kind: DEVPROPTYPE = 0;
    let mut v = 0u8;
    let mut size = 1u32;
    let r = unsafe { CM_Get_DevNode_PropertyW(devinst, &BATTERY, &mut kind, &mut v, &mut size, 0) };
    (r == CR_SUCCESS && size == 1 && v <= 100).then_some(v)
}

pub fn poll(ifaces: &[hid::Iface]) -> Vec<Entry> {
    let mut out: Vec<Entry> = Vec::new();
    for i in ifaces.iter().filter(|i| hid::is_bluetooth(&i.path)) {
        let Some((page, usage, product)) = hid::describe(&i.path) else { continue };
        if !hid::is_gamepad(page, usage) {
            continue;
        }
        let (vid, pid) = hid::ids(&i.path).unwrap_or((0, 0));
        let mut d = i.devinst;
        for _ in 0..4 {
            let Some(p) = hid::parent(d) else { break };
            d = p;
            if let Some(percent) = battery_of(d) {
                let id = format!("bt:{}", hid::instance_id(d).to_ascii_lowercase());
                if !out.iter().any(|e| e.id == id) {
                    out.push(Entry {
                        id,
                        name: if product.is_empty() { "Bluetooth controller".into() } else { product.clone() },
                        vid,
                        pid,
                        source: "Bluetooth",
                        reading: Reading { percent: Some(percent), level: None, charging: false },
                        path: i.path.to_ascii_lowercase(),
                        virtual_pad: false,
                    });
                }
                break;
            }
        }
    }
    out
}
