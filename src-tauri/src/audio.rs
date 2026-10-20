//! Output devices, master volume and per-app volume (like EarTrumpet).
//!
//! The default output is switched through IPolicyConfig, the interface the
//! Sound control panel uses; it works without administrator rights. The default
//! mic is changed only by the phone mic's own switch, off unless the user turns
//! it on (`phonemic`), and put back when the phone mic stops.

#![allow(non_snake_case)]

use crate::util::{com, exe_stem, process_path, take_pwstr};
use parking_lot::Mutex;
use serde::Serialize;
use std::collections::HashMap;
use windows::core::{interface, Interface, IUnknown, IUnknown_Vtbl, GUID, HRESULT, HSTRING, PCWSTR};
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Foundation::{PROPERTYKEY, S_OK};
use windows::Win32::Media::Audio::Endpoints::{IAudioEndpointVolume, IAudioMeterInformation};
use windows::Win32::Media::Audio::{
    eCapture, eCommunications, eConsole, eMultimedia, eRender, AudioSessionStateExpired, ERole, IAudioSessionControl2,
    IAudioSessionManager2, IMMDevice, IMMDeviceEnumerator, ISimpleAudioVolume, MMDeviceEnumerator,
    DEVICE_STATE_ACTIVE,
};
use windows::Win32::System::Com::StructuredStorage::{PropVariantToStringAlloc, PROPVARIANT};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL, STGM_READ};

#[interface("f8679f50-850a-41cf-9c72-430f290290c8")]
unsafe trait IPolicyConfig: IUnknown {
    fn GetMixFormat(&self, id: PCWSTR, fmt: *mut *mut core::ffi::c_void) -> HRESULT;
    fn GetDeviceFormat(&self, id: PCWSTR, default: i32, fmt: *mut *mut core::ffi::c_void) -> HRESULT;
    fn ResetDeviceFormat(&self, id: PCWSTR) -> HRESULT;
    fn SetDeviceFormat(&self, id: PCWSTR, a: *mut core::ffi::c_void, b: *mut core::ffi::c_void) -> HRESULT;
    fn GetProcessingPeriod(&self, id: PCWSTR, default: i32, a: *mut i64, b: *mut i64) -> HRESULT;
    fn SetProcessingPeriod(&self, id: PCWSTR, a: *mut i64) -> HRESULT;
    fn GetShareMode(&self, id: PCWSTR, mode: *mut core::ffi::c_void) -> HRESULT;
    fn SetShareMode(&self, id: PCWSTR, mode: *mut core::ffi::c_void) -> HRESULT;
    fn GetPropertyValue(&self, id: PCWSTR, fx_store: i32, key: *const PROPERTYKEY, pv: *mut PROPVARIANT) -> HRESULT;
    fn SetPropertyValue(&self, id: PCWSTR, fx_store: i32, key: *const PROPERTYKEY, pv: *const PROPVARIANT) -> HRESULT;
    fn SetDefaultEndpoint(&self, id: PCWSTR, role: ERole) -> HRESULT;
    fn SetEndpointVisibility(&self, id: PCWSTR, visible: i32) -> HRESULT;
}

const CLSID_POLICY_CONFIG: GUID = GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);

#[derive(Serialize, Clone)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub default: bool,
}

#[derive(Serialize, Clone)]
pub struct AppVol {
    /// Grouping key: lower-case exe name, or "system".
    pub key: String,
    pub name: String,
    pub exe: Option<String>,
    pub vol: f32,
    pub muted: bool,
    pub peak: f32,
}

#[derive(Serialize, Clone)]
pub struct Master {
    pub vol: f32,
    pub muted: bool,
    pub peak: f32,
    pub device: Option<String>,
}

fn enumerator() -> windows::core::Result<IMMDeviceEnumerator> {
    com();
    unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }
}

fn default_render() -> windows::core::Result<IMMDevice> {
    unsafe { enumerator()?.GetDefaultAudioEndpoint(eRender, eConsole) }
}

fn device_id(d: &IMMDevice) -> String {
    unsafe { d.GetId().map(|p| take_pwstr(p)).unwrap_or_default() }
}

fn device_name(d: &IMMDevice) -> String {
    unsafe {
        let Ok(store) = d.OpenPropertyStore(STGM_READ) else { return String::new() };
        let Ok(pv) = store.GetValue(&PKEY_Device_FriendlyName) else { return String::new() };
        PropVariantToStringAlloc(&pv).map(|p| take_pwstr(p)).unwrap_or_default()
    }
}

pub fn outputs() -> Vec<Device> {
    let mut out = Vec::new();
    let Ok(e) = enumerator() else { return out };
    let def = default_render().ok().map(|d| device_id(&d));
    unsafe {
        let Ok(col) = e.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE) else { return out };
        for i in 0..col.GetCount().unwrap_or(0) {
            let Ok(d) = col.Item(i) else { continue };
            let id = device_id(&d);
            out.push(Device { name: device_name(&d), default: def.as_deref() == Some(id.as_str()), id });
        }
    }
    out.sort_by_key(|a| a.name.to_lowercase());
    out
}

/// Makes `id` the default playback device for every role. Only a current
/// playback device is accepted, so a stale or wrong id (from the phone, say)
/// can never change the microphone.
pub fn set_default_output(id: &str) -> Result<(), String> {
    if !outputs().iter().any(|d| d.id == id) {
        return Err("That output isn't connected any more".into());
    }
    com();
    let pc: IPolicyConfig =
        unsafe { CoCreateInstance(&CLSID_POLICY_CONFIG, None, CLSCTX_ALL) }.map_err(|e| format!("IPolicyConfig: {e}"))?;
    let h = HSTRING::from(id);
    for role in [eConsole, eMultimedia, eCommunications] {
        unsafe { pc.SetDefaultEndpoint(PCWSTR(h.as_ptr()), role) }
            .ok()
            .map_err(|e| format!("Couldn't switch output: {e}"))?;
    }
    Ok(())
}

fn device_by_id(id: &str) -> windows::core::Result<IMMDevice> {
    unsafe { enumerator()?.GetDevice(&HSTRING::from(id)) }
}

/// The default playback device's id.
pub fn default_output_id() -> Option<String> {
    default_render().ok().map(|d| device_id(&d))
}

/// Whether the playback device `id` is muted.
pub fn output_muted(id: &str) -> Option<bool> {
    unsafe {
        let v = device_by_id(id).ok()?.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None).ok()?;
        v.GetMute().ok().map(|b| b.as_bool())
    }
}

/// Mutes or unmutes the playback device `id` (not only the default one).
pub fn set_output_muted(id: &str, muted: bool) -> Result<(), String> {
    unsafe {
        let v = device_by_id(id).and_then(|d| d.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None)).map_err(|e| e.to_string())?;
        v.SetMute(muted, std::ptr::null()).map_err(|e| e.to_string())
    }
}

/// Recording devices that are connected now (`default`: the default for apps, eConsole).
pub fn inputs() -> Vec<Device> {
    let mut out = Vec::new();
    let Ok(e) = enumerator() else { return out };
    let def = default_input_id(eConsole);
    unsafe {
        let Ok(col) = e.EnumAudioEndpoints(eCapture, DEVICE_STATE_ACTIVE) else { return out };
        for i in 0..col.GetCount().unwrap_or(0) {
            let Ok(d) = col.Item(i) else { continue };
            let id = device_id(&d);
            out.push(Device { name: device_name(&d), default: def.as_deref() == Some(id.as_str()), id });
        }
    }
    out.sort_by_key(|a| a.name.to_lowercase());
    out
}

/// The default recording device for `role` (eConsole: apps and games; eCommunications: calls).
pub fn default_input_id(role: ERole) -> Option<String> {
    unsafe { enumerator().ok()?.GetDefaultAudioEndpoint(eCapture, role).ok().map(|d| device_id(&d)) }
}

/// Makes the recording device `id` the default for `role`. Only a connected recording device is
/// accepted, so a playback id can never end up here. Used by the phone mic's switch alone.
pub fn set_default_input(id: &str, role: ERole) -> Result<(), String> {
    if !inputs().iter().any(|d| d.id == id) {
        return Err("That microphone isn't connected any more".into());
    }
    com();
    let pc: IPolicyConfig =
        unsafe { CoCreateInstance(&CLSID_POLICY_CONFIG, None, CLSCTX_ALL) }.map_err(|e| format!("IPolicyConfig: {e}"))?;
    let h = HSTRING::from(id);
    let roles: &[ERole] = if role == eConsole { &[eConsole, eMultimedia] } else { &[role] };
    for r in roles {
        unsafe { pc.SetDefaultEndpoint(PCWSTR(h.as_ptr()), *r) }.ok().map_err(|e| format!("Couldn't switch the microphone: {e}"))?;
    }
    Ok(())
}

/// Switches output by (part of) the device name.
pub fn set_output_by_name(name: &str) -> Result<(), String> {
    let all = outputs();
    let d = pick_output(&all, name)?;
    set_default_output(&d.id)
}

/// The exact name wins; otherwise a part of a name, but only if it fits one
/// device ("Speakers" with two speakers plugged in is refused, not guessed).
fn pick_output<'a>(all: &'a [Device], name: &str) -> Result<&'a Device, String> {
    let n = name.trim().to_lowercase();
    if n.is_empty() {
        return Err("No output name given".into());
    }
    if let Some(d) = all.iter().find(|d| d.name.to_lowercase() == n) {
        return Ok(d);
    }
    let part: Vec<&Device> = all.iter().filter(|d| d.name.to_lowercase().contains(&n)).collect();
    match part.as_slice() {
        [d] => Ok(d),
        [] => Err(format!("No output called {name}")),
        _ => Err(format!("More than one output matches {name}")),
    }
}

fn endpoint_volume() -> windows::core::Result<IAudioEndpointVolume> {
    unsafe { default_render()?.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None) }
}

pub fn master() -> Option<Master> {
    let d = default_render().ok()?;
    unsafe {
        let v = d.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None).ok()?;
        let peak = d
            .Activate::<IAudioMeterInformation>(CLSCTX_ALL, None)
            .ok()
            .and_then(|m| m.GetPeakValue().ok())
            .unwrap_or(0.0);
        Some(Master {
            vol: v.GetMasterVolumeLevelScalar().unwrap_or(0.0),
            muted: v.GetMute().map(|b| b.as_bool()).unwrap_or(false),
            peak,
            device: Some(device_name(&d)),
        })
    }
}

pub fn set_master(vol: Option<f32>, muted: Option<bool>) -> Result<(), String> {
    let v = endpoint_volume().map_err(|e| e.to_string())?;
    unsafe {
        if let Some(x) = vol {
            v.SetMasterVolumeLevelScalar(x.clamp(0.0, 1.0), std::ptr::null()).map_err(|e| e.to_string())?;
        }
        if let Some(m) = muted {
            v.SetMute(m, std::ptr::null()).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

struct Session {
    key: String,
    exe: Option<String>,
    vol: ISimpleAudioVolume,
    meter: Option<IAudioMeterInformation>,
}

fn sessions() -> Vec<Session> {
    let mut out = Vec::new();
    let Ok(d) = default_render() else { return out };
    unsafe {
        let Ok(mgr) = d.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) else { return out };
        let Ok(en) = mgr.GetSessionEnumerator() else { return out };
        for i in 0..en.GetCount().unwrap_or(0) {
            let Ok(c) = en.GetSession(i) else { continue };
            if c.GetState().map(|s| s == AudioSessionStateExpired).unwrap_or(true) {
                continue;
            }
            let Ok(c2) = c.cast::<IAudioSessionControl2>() else { continue };
            let Ok(vol) = c.cast::<ISimpleAudioVolume>() else { continue };
            let system = c2.IsSystemSoundsSession() == S_OK;
            let pid = c2.GetProcessId().unwrap_or(0);
            let (key, exe) = if system {
                ("system".to_string(), None)
            } else {
                let Some(path) = process_path(pid) else { continue };
                let file = std::path::Path::new(&path).file_name().map(|f| f.to_string_lossy().to_lowercase());
                (file.unwrap_or_else(|| pid.to_string()), Some(path))
            };
            out.push(Session { key, exe, vol, meter: c.cast::<IAudioMeterInformation>().ok() });
        }
    }
    out
}

static NAMES: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

/// Friendly program name from the exe's version info ("Google Chrome"),
/// falling back to the file name.
pub fn friendly_name(exe: &str) -> String {
    if let Some(n) = NAMES.lock().get_or_insert_with(HashMap::new).get(exe) {
        return n.clone();
    }
    let n = file_description(exe).filter(|d| !d.trim().is_empty() && d.len() < 48).unwrap_or_else(|| exe_stem(exe));
    NAMES.lock().get_or_insert_with(HashMap::new).insert(exe.to_string(), n.clone());
    n
}

fn file_description(path: &str) -> Option<String> {
    use windows::Win32::Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW};
    let p = HSTRING::from(path);
    unsafe {
        let size = GetFileVersionInfoSizeW(&p, None);
        if size == 0 {
            return None;
        }
        let mut buf = vec![0u8; size as usize];
        GetFileVersionInfoW(&p, None, size, buf.as_mut_ptr() as *mut _).ok()?;
        // First language/codepage pair.
        let mut ptr = std::ptr::null_mut();
        let mut len = 0u32;
        let tr = HSTRING::from("\\VarFileInfo\\Translation");
        if !VerQueryValueW(buf.as_ptr() as *const _, &tr, &mut ptr, &mut len).as_bool() || len < 4 {
            return None;
        }
        let lang = *(ptr as *const u16);
        let cp = *(ptr as *const u16).add(1);
        let q = HSTRING::from(format!("\\StringFileInfo\\{lang:04x}{cp:04x}\\FileDescription"));
        if !VerQueryValueW(buf.as_ptr() as *const _, &q, &mut ptr, &mut len).as_bool() || len == 0 {
            return None;
        }
        let s = std::slice::from_raw_parts(ptr as *const u16, len as usize);
        Some(crate::util::from_wide(s).trim().to_string())
    }
}

/// Apps currently holding an audio stream on the default output.
pub fn apps() -> Vec<AppVol> {
    let mut map: HashMap<String, AppVol> = HashMap::new();
    for s in sessions() {
        unsafe {
            let vol = s.vol.GetMasterVolume().unwrap_or(1.0);
            let muted = s.vol.GetMute().map(|b| b.as_bool()).unwrap_or(false);
            let peak = s.meter.as_ref().and_then(|m| m.GetPeakValue().ok()).unwrap_or(0.0);
            let e = map.entry(s.key.clone()).or_insert_with(|| AppVol {
                name: match &s.exe {
                    Some(p) => friendly_name(p),
                    None => "System sounds".into(),
                },
                key: s.key.clone(),
                exe: s.exe.clone(),
                vol,
                muted,
                peak: 0.0,
            });
            e.peak = e.peak.max(peak);
        }
    }
    let mut v: Vec<AppVol> = map.into_values().collect();
    v.sort_by(|a, b| (a.key == "system").cmp(&(b.key == "system")).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    v
}

pub fn set_app(key: &str, vol: Option<f32>, muted: Option<bool>) -> Result<(), String> {
    let mut hit = false;
    for s in sessions().into_iter().filter(|s| s.key == key) {
        hit = true;
        unsafe {
            if let Some(v) = vol {
                let _ = s.vol.SetMasterVolume(v.clamp(0.0, 1.0), std::ptr::null());
            }
            if let Some(m) = muted {
                let _ = s.vol.SetMute(m, std::ptr::null());
            }
        }
    }
    if hit {
        Ok(())
    } else {
        Err("That app isn't playing sound any more".into())
    }
}

#[cfg(test)]
mod tests {
    use super::{pick_output, Device};

    #[test]
    fn output_names_match_one_device_or_none() {
        let d = |n: &str| Device { id: n.into(), name: n.into(), default: false };
        let all = [d("Speakers (Realtek)"), d("Speakers (USB DAC)"), d("Headphones (Arctis)")];
        assert_eq!(pick_output(&all, "headphones").unwrap().id, "Headphones (Arctis)");
        assert_eq!(pick_output(&all, "speakers (usb dac)").unwrap().id, "Speakers (USB DAC)");
        assert!(pick_output(&all, "Speakers").is_err(), "two speakers: don't guess");
        assert!(pick_output(&all, "").is_err());
        assert!(pick_output(&all, "  ").is_err());
        assert!(pick_output(&all, "Monitor").is_err());
    }
}
