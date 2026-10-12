//! HP OMEN / Victus laptops: fan speed and fan control through the BIOS WMI interface
//! (root\wmi, class hpqBIntM, method hpqBIOSInt128), the same interface OMEN Gaming Hub uses.
//!
//! Commands (CommandType under 0x20008), as measured on a Victus 15-fa0xxx (board 8A4F):
//! * 0x2D  read both fan levels, in units of 100 rpm
//! * 0x2E  write both fan levels (128 bytes). The BIOS answers rc=46 but applies the levels. Level 0 = automatic.
//! * 0x10  keep-alive: user-defined fan state expires if it is not sent every few seconds
//!
//! All WMI calls run on one private COM thread, so they never collide with the UI's COM apartment.
use crate::hw::sio::{Control, FanChip, Reading};
use std::mem::ManuallyDrop;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant};
use windows::core::{w, Interface, BSTR, IUnknown};
use windows::Win32::System::Com::*;
use windows::Win32::System::Ole::*;
use windows::Win32::System::Variant::*;
use windows::Win32::System::Wmi::*;

/// Fan level that means 100 %. The firmware tops out near 58 (5800 rpm).
const MAX_LEVEL: f32 = 58.0;
/// Never ask for less than this (1800 rpm) while a manual speed is set.
const MIN_LEVEL: u8 = 18;

enum Op {
    Bios(u32, Vec<u8>),
    Thermal,
}
enum Out {
    Bios(u32, Vec<u8>),
    Temp(Option<f32>),
}
struct Req {
    op: Op,
    reply: Sender<Result<Out, String>>,
}

// ---------------------------------------------------------------- VARIANT helpers
fn bytes_variant(b: &[u8]) -> VARIANT {
    unsafe {
        let psa = SafeArrayCreateVector(VT_UI1, 0, b.len() as u32);
        let mut p: *mut core::ffi::c_void = std::ptr::null_mut();
        if !psa.is_null() && SafeArrayAccessData(psa, &mut p).is_ok() {
            std::ptr::copy_nonoverlapping(b.as_ptr(), p as *mut u8, b.len());
            let _ = SafeArrayUnaccessData(psa);
        }
        let mut v = VARIANT::default();
        let inner = &mut *v.Anonymous.Anonymous;
        inner.vt = VARENUM(VT_ARRAY.0 | VT_UI1.0);
        inner.Anonymous.parray = psa;
        v
    }
}

fn unknown_variant(u: &IUnknown) -> VARIANT {
    unsafe {
        let mut v = VARIANT::default();
        let inner = &mut *v.Anonymous.Anonymous;
        inner.vt = VT_UNKNOWN;
        inner.Anonymous.punkVal = ManuallyDrop::new(Some(u.clone()));
        v
    }
}

fn variant_bytes(v: &VARIANT) -> Vec<u8> {
    unsafe {
        let inner = &*v.Anonymous.Anonymous;
        if inner.vt.0 & VT_ARRAY.0 == 0 {
            return vec![];
        }
        let psa = inner.Anonymous.parray;
        if psa.is_null() {
            return vec![];
        }
        let Ok(ub) = SafeArrayGetUBound(psa, 1) else { return vec![] };
        let n = (ub + 1).max(0) as usize;
        let mut p: *mut core::ffi::c_void = std::ptr::null_mut();
        if SafeArrayAccessData(psa, &mut p).is_err() || p.is_null() {
            return vec![];
        }
        let out = std::slice::from_raw_parts(p as *const u8, n).to_vec();
        let _ = SafeArrayUnaccessData(psa);
        out
    }
}

// ---------------------------------------------------------------- WMI worker
struct Ctx {
    svc: IWbemServices,
    path: BSTR,
    in_cls: IWbemClassObject,
    in_sig: IWbemClassObject,
}

fn e<T: std::fmt::Display>(what: &str, err: T) -> String {
    format!("{}: {}", what, err)
}

unsafe fn setup() -> Result<Ctx, String> {
    let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    let _ = CoInitializeSecurity(None, -1, None, None, RPC_C_AUTHN_LEVEL_DEFAULT, RPC_C_IMP_LEVEL_IMPERSONATE, None, EOAC_NONE, None);
    let loc: IWbemLocator = CoCreateInstance(&WbemLocator, None, CLSCTX_INPROC_SERVER).map_err(|x| e("WMI locator", x))?;
    let svc = loc
        .ConnectServer(&BSTR::from("ROOT\\WMI"), &BSTR::new(), &BSTR::new(), &BSTR::new(), 0, &BSTR::new(), None::<&IWbemContext>)
        .map_err(|x| e("WMI connect", x))?;
    let _ = CoSetProxyBlanket(&svc, 10, 0, None, RPC_C_AUTHN_LEVEL_CALL, RPC_C_IMP_LEVEL_IMPERSONATE, None, EOAC_NONE);
    // the one instance of hpqBIntM: its object path is needed to run the method
    let en = svc.CreateInstanceEnum(&BSTR::from("hpqBIntM"), WBEM_FLAG_FORWARD_ONLY, None::<&IWbemContext>).map_err(|x| e("no HP BIOS interface", x))?;
    let mut objs: [Option<IWbemClassObject>; 1] = [None];
    let mut got = 0u32;
    let _ = en.Next(WBEM_INFINITE, &mut objs, &mut got);
    let obj = objs[0].take().ok_or("no HP BIOS interface instance")?;
    let mut v = VARIANT::default();
    obj.Get(w!("__RELPATH"), 0, &mut v, None, None).map_err(|x| e("HP BIOS path", x))?;
    let path = BSTR::try_from(&v).map_err(|x| e("HP BIOS path", x))?;
    let mut cls: Option<IWbemClassObject> = None;
    svc.GetObject(&BSTR::from("hpqBDataIn"), WBEM_FLAG_RETURN_WBEM_COMPLETE, None::<&IWbemContext>, Some(&mut cls), None).map_err(|x| e("hpqBDataIn", x))?;
    let in_cls = cls.ok_or("hpqBDataIn missing")?;
    let mut cls2: Option<IWbemClassObject> = None;
    svc.GetObject(&BSTR::from("hpqBIntM"), WBEM_FLAG_RETURN_WBEM_COMPLETE, None::<&IWbemContext>, Some(&mut cls2), None).map_err(|x| e("hpqBIntM", x))?;
    let (mut ins, mut outs): (Option<IWbemClassObject>, Option<IWbemClassObject>) = (None, None);
    cls2.ok_or("hpqBIntM missing")?.GetMethod(w!("hpqBIOSInt128"), 0, &mut ins, &mut outs).map_err(|x| e("hpqBIOSInt128", x))?;
    Ok(Ctx { svc, path, in_cls, in_sig: ins.ok_or("hpqBIOSInt128 has no input")? })
}

unsafe fn bios(c: &Ctx, ty: u32, data: &[u8]) -> Result<(u32, Vec<u8>), String> {
    let inp = c.in_cls.SpawnInstance(0).map_err(|x| e("spawn", x))?;
    inp.Put(w!("Sign"), 0, &bytes_variant(b"SECU"), 0).map_err(|x| e("Sign", x))?;
    inp.Put(w!("Command"), 0, &VARIANT::from(0x20008i32), 0).map_err(|x| e("Command", x))?;
    inp.Put(w!("CommandType"), 0, &VARIANT::from(ty as i32), 0).map_err(|x| e("CommandType", x))?;
    inp.Put(w!("Size"), 0, &VARIANT::from(data.len() as i32), 0).map_err(|x| e("Size", x))?;
    inp.Put(w!("hpqBData"), 0, &bytes_variant(data), 0).map_err(|x| e("hpqBData", x))?;
    let params = c.in_sig.SpawnInstance(0).map_err(|x| e("spawn", x))?;
    let unk: IUnknown = inp.cast().map_err(|x| e("cast", x))?;
    params.Put(w!("InData"), 0, &unknown_variant(&unk), 0).map_err(|x| e("InData", x))?;
    let mut out: Option<IWbemClassObject> = None;
    c.svc.ExecMethod(&c.path, &BSTR::from("hpqBIOSInt128"), WBEM_GENERIC_FLAG_TYPE(0), None::<&IWbemContext>, &params, Some(&mut out), None).map_err(|x| e("BIOS call", x))?;
    let out = out.ok_or("BIOS call returned nothing")?;
    let mut ov = VARIANT::default();
    out.Get(w!("OutData"), 0, &mut ov, None, None).map_err(|x| e("OutData", x))?;
    let od: IWbemClassObject = IUnknown::try_from(&ov).map_err(|x| e("OutData", x))?.cast().map_err(|x| e("OutData", x))?;
    let mut rv = VARIANT::default();
    od.Get(w!("rwReturnCode"), 0, &mut rv, None, None).map_err(|x| e("return code", x))?;
    let rc = u32::try_from(&rv).unwrap_or(u32::MAX);
    let mut dv = VARIANT::default();
    od.Get(w!("Data"), 0, &mut dv, None, None).map_err(|x| e("Data", x))?;
    Ok((rc, variant_bytes(&dv)))
}

/// Hottest ACPI thermal zone in C (the laptop's own CPU-area sensor).
unsafe fn thermal(c: &Ctx) -> Option<f32> {
    let en = c.svc.CreateInstanceEnum(&BSTR::from("MSAcpi_ThermalZoneTemperature"), WBEM_FLAG_FORWARD_ONLY, None::<&IWbemContext>).ok()?;
    let mut best: Option<f32> = None;
    loop {
        let mut objs: [Option<IWbemClassObject>; 1] = [None];
        let mut got = 0u32;
        let _ = en.Next(2000, &mut objs, &mut got);
        let Some(o) = objs[0].take() else { break };
        let mut v = VARIANT::default();
        if o.Get(w!("CurrentTemperature"), 0, &mut v, None, None).is_ok() {
            if let Ok(k) = i32::try_from(&v) {
                let t = k as f32 / 10.0 - 273.15;
                if t > 0.0 && t < 125.0 {
                    best = Some(best.map_or(t, |b: f32| b.max(t)));
                }
            }
        }
    }
    best
}

fn worker(rx: Receiver<Req>, ready: Sender<Result<(), String>>) {
    let ctx = match unsafe { setup() } {
        Ok(c) => {
            let _ = ready.send(Ok(()));
            c
        }
        Err(er) => {
            let _ = ready.send(Err(er));
            return;
        }
    };
    while let Ok(r) = rx.recv() {
        let res = unsafe {
            match r.op {
                Op::Bios(ty, d) => bios(&ctx, ty, &d).map(|(rc, data)| Out::Bios(rc, data)),
                Op::Thermal => Ok(Out::Temp(thermal(&ctx))),
            }
        };
        let _ = r.reply.send(res);
    }
}

// ---------------------------------------------------------------- the fan chip
pub struct HpFans {
    tx: Sender<Req>,
    n: usize,
    manual: [Option<u8>; 2],
    last_keep: Instant,
    temp: Option<f32>,
    temp_at: Instant,
    verified: bool,
    board: String,
    report: String,
}

impl HpFans {
    fn ask(&self, op: Op) -> Result<Out, String> {
        let (rtx, rrx) = channel();
        self.tx.send(Req { op, reply: rtx }).map_err(|_| "HP BIOS thread stopped".to_string())?;
        rrx.recv_timeout(Duration::from_secs(5)).map_err(|_| "HP BIOS did not answer".to_string())?
    }
    fn bios(&self, ty: u32, data: &[u8]) -> Result<(u32, Vec<u8>), String> {
        match self.ask(Op::Bios(ty, data.to_vec()))? {
            Out::Bios(rc, d) => Ok((rc, d)),
            _ => Err("unexpected reply".into()),
        }
    }
    fn levels(&self) -> Option<(u8, u8)> {
        let (rc, d) = self.bios(0x2D, &[0, 0, 0, 0]).ok()?;
        (rc == 0 && d.len() >= 2).then(|| (d[0], d[1]))
    }
    /// Writes both fan levels. The BIOS answers rc=46 for this command even when it applies them.
    fn write(&self) {
        let mut b = vec![0u8; 128];
        for i in 0..2 {
            b[i] = self.manual[i].unwrap_or(0); // 0 = the BIOS decides
        }
        if self.manual.iter().any(|m| m.is_some()) {
            let _ = self.bios(0x10, &[0, 0, 0, 0]);
        }
        let _ = self.bios(0x2E, &b);
    }
}

/// HP laptops only; None on anything else or when the BIOS interface does not answer.
pub fn open(info: &crate::hw::smbios::Info) -> Option<Box<dyn FanChip>> {
    let v = info.board_vendor.to_ascii_lowercase();
    if !(v == "hp" || v.contains("hewlett")) {
        return None;
    }
    let (tx, rx) = channel();
    let (rtx, rrx) = channel();
    std::thread::spawn(move || worker(rx, rtx));
    rrx.recv_timeout(Duration::from_secs(10)).ok()?.ok()?;
    let mut f = HpFans { tx, n: 2, manual: [None; 2], last_keep: Instant::now(), temp: None, temp_at: Instant::now() - Duration::from_secs(60), verified: info.board_name.to_ascii_uppercase().contains("8A4F"), board: info.board_name.clone(), report: String::new() };
    let (l0, l1) = f.levels()?; // the read command must work, or this is not a controllable HP board
    f.n = if l1 > 0 { 2 } else { 1 };
    f.report = format!("HP BIOS fan interface: levels {} / {} (x100 rpm), board {}", l0, l1, f.board);
    Some(Box::new(f))
}

impl FanChip for HpFans {
    fn name(&self) -> String {
        "HP laptop fans (BIOS)".into()
    }
    fn fan_names(&self) -> Vec<String> {
        ["CPU fan", "GPU fan"].iter().take(self.n).map(|s| s.to_string()).collect()
    }
    fn temp_names(&self) -> Vec<String> {
        vec!["Laptop thermal zone".into()]
    }
    fn cpu_temp(&self) -> Option<usize> {
        Some(0)
    }
    fn control(&self) -> Control {
        if self.verified {
            Control::Verified
        } else {
            Control::Experimental
        }
    }
    fn read(&mut self) -> Option<Reading> {
        let (l0, l1) = self.levels()?;
        if self.last_keep.elapsed().as_secs() >= 2 && self.manual.iter().any(|m| m.is_some()) {
            let _ = self.bios(0x10, &[0, 0, 0, 0]); // keep the manual speed alive
            self.last_keep = Instant::now();
        }
        if self.temp_at.elapsed().as_secs() >= 3 {
            self.temp = match self.ask(Op::Thermal) {
                Ok(Out::Temp(t)) => t,
                _ => None,
            };
            self.temp_at = Instant::now();
        }
        let lv = [l0, l1];
        Some(Reading {
            pct: (0..self.n).map(|i| Some((lv[i] as f32 / MAX_LEVEL * 100.0).min(100.0))).collect(),
            rpm: (0..self.n).map(|i| lv[i] as u32 * 100).collect(),
            temps: vec![self.temp],
            volts: vec![],
        })
    }
    fn set_percent(&mut self, i: usize, pct: f32) {
        if i >= self.n {
            return;
        }
        let level = (pct.clamp(0.0, 100.0) / 100.0 * MAX_LEVEL).round() as u8;
        self.manual[i] = Some(level.clamp(MIN_LEVEL, MAX_LEVEL as u8));
        self.write();
    }
    fn release(&mut self, i: usize) {
        if i < 2 {
            self.manual[i] = None;
        }
        self.write();
    }
    fn release_all(&mut self) {
        self.manual = [None; 2];
        self.write();
    }
    fn report(&self) -> String {
        self.report.clone()
    }
}

impl Drop for HpFans {
    fn drop(&mut self) {
        self.release_all(); // never leave the fans on a manual speed
    }
}
