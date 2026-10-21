//! The phone as the PC's speaker and microphone (Deck → Phone → Speaker and mic).
//!
//! Speaker: the phone plays the PC's sound (the Screen view's `/audio` stream, WASAPI loopback).
//! Loopback is taken before the PC's own volume and mute, so "PC quiet" simply mutes the default
//! output while a phone plays and puts it back afterwards (`Quiet`).
//!
//! Microphone: Windows lists a microphone only when a driver provides one, and Deck has no signed
//! driver of its own, so the phone's mic goes into VB-CABLE (free, VB-Audio's): Deck plays it into
//! "CABLE Input" and apps hear it on "CABLE Output". VB-CABLE's licence doesn't allow building it
//! into another program's installation, so Deck only turns on one that is installed but disabled,
//! or downloads VB-Audio's package as it is and opens VB-Audio's own setup. With the switch on,
//! "CABLE Output" is the default mic while a phone talks, and the old default comes back after.
//!
//! What Deck changed is written down first (`phone-quiet.json`, `phone-mic.json`), so a crash or
//! a restart while a phone was playing or talking never leaves the PC muted or on the cable.

use crate::audio;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use windows::Win32::Media::Audio::{eCommunications, eConsole};

fn state_file(name: &str) -> std::path::PathBuf {
    crate::settings::Settings::dir().join(name)
}

fn save<T: Serialize>(name: &str, v: &T) {
    if let Ok(t) = serde_json::to_vec(v) {
        let _ = crate::util::write_atomic(&state_file(name), &t);
    }
}

fn load<T: for<'a> Deserialize<'a>>(name: &str) -> Option<T> {
    std::fs::read_to_string(state_file(name)).ok().and_then(|t| serde_json::from_str(&t).ok())
}

// ---------------------------------------------------------------- PC quiet while the phone plays

#[derive(Serialize, Deserialize, Clone)]
struct Muted {
    /// the output Deck muted, and whether it was muted already
    id: String,
    was: bool,
}

/// How many phones want the PC quiet, and what Deck muted for them.
struct Quiet {
    wanted: usize,
    muted: Option<Muted>,
}

static QUIET: Mutex<Quiet> = parking_lot::const_mutex(Quiet { wanted: 0, muted: None });
const QUIET_FILE: &str = "phone-quiet.json";

fn mute_default(q: &mut Quiet) {
    let Some(id) = audio::default_output_id() else { return };
    let was = audio::output_muted(&id).unwrap_or(false);
    let m = Muted { id: id.clone(), was };
    save(QUIET_FILE, &m);
    q.muted = Some(m);
    if !was {
        if let Err(e) = audio::set_output_muted(&id, true) {
            crate::log::warn(format!("phone speaker: couldn't mute the PC: {e}"));
        }
    }
}

fn unmute(q: &mut Quiet) {
    if let Some(m) = q.muted.take() {
        if !m.was {
            let _ = audio::set_output_muted(&m.id, false);
        }
    }
    let _ = std::fs::remove_file(state_file(QUIET_FILE));
}

/// One phone playing the PC's sound. `quiet(true)` asks for the PC's speakers to be muted while
/// it plays; dropping it gives its wish back (the last one unmutes).
pub struct Speaker {
    quiet: bool,
}

impl Speaker {
    pub fn new() -> Self {
        Self { quiet: false }
    }

    /// The phone's switch: at once both ways.
    pub fn quiet(&mut self, on: bool) {
        self.set(on, false);
    }

    /// `left`: the phone's connection ended (maybe a Wi-Fi blip) rather than the switch turned off.
    fn set(&mut self, on: bool, left: bool) {
        if on == self.quiet {
            return;
        }
        self.quiet = on;
        let mut q = QUIET.lock();
        if on {
            q.wanted += 1;
            // still muted from a phone that just reconnected (the grace below): keep what was saved
            if q.wanted == 1 && q.muted.is_none() {
                mute_default(&mut q);
            }
        } else {
            q.wanted = q.wanted.saturating_sub(1);
            if q.wanted == 0 && !left {
                unmute(&mut q);
            } else if q.wanted == 0 {
                // a Wi-Fi blip drops the phone's connection and it comes back 2 s later: the PC's
                // sound only comes back when no phone asked again within 5 s
                std::thread::spawn(|| {
                    std::thread::sleep(Duration::from_secs(5));
                    let mut q = QUIET.lock();
                    if q.wanted == 0 {
                        unmute(&mut q);
                    }
                });
            }
        }
    }

    /// Headphones plugged in, another output picked in Windows: the sound follows the new
    /// default (screencast), so the quiet moves with it.
    pub fn follow(&self) {
        if !self.quiet {
            return;
        }
        let mut q = QUIET.lock();
        let now = audio::default_output_id();
        if q.wanted > 0 && now.is_some() && q.muted.as_ref().map(|m| &m.id) != now.as_ref() {
            unmute(&mut q);
            mute_default(&mut q);
        }
    }
}

impl Drop for Speaker {
    fn drop(&mut self) {
        self.set(false, true);
    }
}

// ---------------------------------------------------------------- VB-CABLE

/// VB-CABLE's two ends: the playback device Deck plays the phone into, and the microphone apps hear.
#[derive(Clone)]
pub struct Cable {
    pub play: String,
    pub mic: String,
}

/// VB-CABLE's endpoints while it is installed and on.
pub fn cable() -> Option<Cable> {
    let is = |name: &str, end: &str| name.starts_with(end) && name.contains("VB-Audio Virtual Cable");
    let play = audio::outputs().into_iter().find(|d| is(&d.name, "CABLE Input"))?.id;
    let mic = audio::inputs().into_iter().find(|d| is(&d.name, "CABLE Output"))?.id;
    Some(Cable { play, mic })
}

static CABLE_SEEN: Mutex<Option<(Instant, bool)>> = parking_lot::const_mutex(None);

/// Whether VB-CABLE is there, asked at most every 5 s (for the phone's state).
pub fn cable_ready() -> bool {
    let mut c = CABLE_SEEN.lock();
    if let Some((t, ok)) = *c {
        if t.elapsed() < Duration::from_secs(5) {
            return ok;
        }
    }
    let ok = cable().is_some();
    *c = Some((Instant::now(), ok));
    ok
}

/// VB-CABLE's device in Device Manager (instance id, turned off), when it is installed.
fn cable_device() -> Option<(String, bool)> {
    let o = crate::util::run_hidden(
        "powershell.exe",
        &["-NoProfile", "-NonInteractive", "-Command", "Get-PnpDevice -FriendlyName 'VB-Audio Virtual Cable' -ErrorAction SilentlyContinue | Select-Object -First 1 InstanceId,Problem | ConvertTo-Json -Compress"],
    )
    .ok()?;
    let v: Value = serde_json::from_slice(&o.stdout).ok()?;
    let id = v["InstanceId"].as_str()?.to_string();
    // CM_PROB_DISABLED
    let off = v["Problem"].as_u64() == Some(22) || v["Problem"].as_str().is_some_and(|p| p.contains("DISABLED"));
    Some((id, off))
}

static SETUP: Mutex<Option<String>> = parking_lot::const_mutex(None);

/// For Deck's Phone page: "ready", "off" (installed, turned off in Device Manager), "missing";
/// what a setup step is doing or why it failed.
pub fn cable_state() -> Value {
    let state = if cable().is_some() {
        "ready"
    } else {
        match cable_device() {
            Some((_, true)) => "off",
            // installed but its devices aren't there yet: Windows wants the restart its setup asks for
            Some((_, false)) => "restart",
            None => "missing",
        }
    };
    // VB-CABLE's setup (or Windows, when it appears) can make "CABLE Input" the default speakers:
    // the PC goes silent, and the phone's voice would come back out of the phone speaker
    let cable_play = cable().map(|c| c.play);
    let taken = cable_play.is_some() && audio::default_output_id() == cable_play;
    let others: Vec<Value> = if taken {
        audio::outputs().into_iter().filter(|d| Some(&d.id) != cable_play.as_ref() && !d.name.contains("VB-Audio")).map(|d| json!({ "id": d.id, "name": d.name })).collect()
    } else {
        Vec::new()
    };
    json!({ "state": state, "busy": *SETUP.lock(), "admin": crate::elevation::is_admin(), "enable_failed": *ENABLE_FAILED.lock(), "cable_is_speakers": taken, "outputs": others })
}

/// Turns VB-CABLE back on when it was turned off in Device Manager (needs admin).
pub fn enable_cable() -> Result<(), String> {
    let (id, off) = cable_device().ok_or("VB-CABLE isn't installed")?;
    if !off {
        return Ok(());
    }
    if !crate::elevation::is_admin() {
        return Err("Turning VB-CABLE on needs Deck's admin rights (Settings → About → Set up)".into());
    }
    let o = crate::util::run_hidden("pnputil.exe", &["/enable-device", &id]).map_err(|e| e.to_string())?;
    // the endpoints come a moment after the device
    for _ in 0..20 {
        if cable().is_some() {
            crate::log::info("phone mic: VB-CABLE turned on");
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    if o.status.success() {
        Ok(())
    } else {
        // an old VB-CABLE (made for Windows 7) whose driver Windows no longer loads: "not connected"
        *ENABLE_FAILED.lock() = true;
        Err("Windows couldn't turn this VB-CABLE on (it may be an old version). Get VB-CABLE installs the current one over it.".into())
    }
}

/// Turning the installed VB-CABLE on failed: the Phone page offers the current one instead.
static ENABLE_FAILED: Mutex<bool> = parking_lot::const_mutex(false);

const CABLE_ZIP: &str = "https://download.vb-audio.com/Download_CABLE/VBCABLE_Driver_Pack45.zip";

/// Downloads VB-Audio's package as it is and opens VB-Audio's own setup (the user installs it
/// there and restarts). The setup is run only when Windows says VB-Audio signed it.
pub fn get_cable() -> Result<(), String> {
    *SETUP.lock() = Some("Downloading VB-CABLE…".into());
    // the new one may need turning on again (an old one turned off keeps that on its device)
    *ENABLE_FAILED.lock() = false;
    let r = get_cable_inner();
    *SETUP.lock() = None;
    r
}

fn get_cable_inner() -> Result<(), String> {
    // its own new folder: nothing else is in it
    let dir = std::env::temp_dir().join(format!("deck-vbcable-{}", crate::util::random_token(8)));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let zip = dir.join("VBCABLE_Driver_Pack.zip");
    let resp = crate::log::http_agent().get(CABLE_ZIP).timeout(Duration::from_secs(60)).call().map_err(|e| format!("Couldn't download VB-CABLE: {e}"))?;
    let mut body = Vec::new();
    use std::io::Read;
    resp.into_reader().take(20 << 20).read_to_end(&mut body).map_err(|e| e.to_string())?;
    std::fs::write(&zip, &body).map_err(|e| e.to_string())?;
    let out = dir.join("VBCABLE");
    // quoted for PowerShell (a user folder can be "O'Brien")
    let q = |p: &std::path::Path| crate::rdesk::ps_str(&p.display().to_string());
    let ps = format!(
        "$ErrorActionPreference='Stop'; Expand-Archive -LiteralPath {} -DestinationPath {} -Force; $s = Get-AuthenticodeSignature -LiteralPath {}; \"$($s.Status)|$($s.SignerCertificate.Subject)\"",
        q(&zip),
        q(&out),
        q(&out.join("VBCABLE_Setup_x64.exe"))
    );
    let o = crate::util::run_hidden("powershell.exe", &["-NoProfile", "-NonInteractive", "-Command", &ps]).map_err(|e| e.to_string())?;
    let sig = String::from_utf8_lossy(&o.stdout).trim().to_string();
    if !signed_by_vb_audio(&sig) {
        return Err(format!("The downloaded VB-CABLE setup isn't signed by VB-Audio ({sig}); it wasn't opened"));
    }
    let setup = out.join("VBCABLE_Setup_x64.exe");
    // VB-Audio's setup asks for admin rights itself
    crate::util::shell_open(&setup.display().to_string())?;
    crate::log::info("phone mic: VB-CABLE's setup opened");
    Ok(())
}

/// "Valid|CN=BUREL VINCENT …": Windows checked the signature and it is VB-Audio's author's.
fn signed_by_vb_audio(sig: &str) -> bool {
    let (status, subject) = sig.split_once('|').unwrap_or((sig, ""));
    status == "Valid" && subject.to_uppercase().contains("CN=BUREL VINCENT")
}

// ---------------------------------------------------------------- the default mic while a phone talks

#[derive(Serialize, Deserialize, Clone, Default)]
struct MicWas {
    console: Option<String>,
    comms: Option<String>,
    /// the cable's mic Deck made the default (a role the user moved elsewhere meanwhile is left alone)
    #[serde(default)]
    cable: Option<String>,
}

struct DefaultMic {
    talking: usize,
    was: Option<MicWas>,
    /// the cable's mic Deck made the default
    cable: String,
}

static DEFAULT_MIC: Mutex<DefaultMic> = parking_lot::const_mutex(DefaultMic { talking: 0, was: None, cable: String::new() });
const MIC_FILE: &str = "phone-mic.json";

fn take_default(cable_mic: &str) {
    let mut d = DEFAULT_MIC.lock();
    d.talking += 1;
    if d.talking > 1 {
        return;
    }
    let was = MicWas { console: audio::default_input_id(eConsole), comms: audio::default_input_id(eCommunications), cable: Some(cable_mic.to_string()) };
    save(MIC_FILE, &was);
    d.was = Some(was);
    d.cable = cable_mic.to_string();
    for role in [eConsole, eCommunications] {
        if let Err(e) = audio::set_default_input(cable_mic, role) {
            crate::log::warn(format!("phone mic: couldn't make it the default mic: {e}"));
        }
    }
}

fn give_back_default() {
    let mut d = DEFAULT_MIC.lock();
    d.talking = d.talking.saturating_sub(1);
    if d.talking > 0 {
        return;
    }
    if let Some(was) = d.was.take() {
        restore_mic(&was);
    }
    let _ = std::fs::remove_file(state_file(MIC_FILE));
}

fn restore_mic(was: &MicWas) {
    // only a role that is still on the cable: one the user changed meanwhile stays theirs
    // (and a mic unplugged meanwhile: Windows picks one itself)
    let still = |role| match &was.cable {
        Some(c) => audio::default_input_id(role).as_deref() == Some(c.as_str()),
        None => true,
    };
    if let Some(id) = was.console.as_ref().filter(|_| still(eConsole)) {
        let _ = audio::set_default_input(id, eConsole);
    }
    if let Some(id) = was.comms.as_ref().filter(|_| still(eCommunications)) {
        let _ = audio::set_default_input(id, eCommunications);
    }
}

/// At start: what a Deck that ended mid-way (a crash, a restart) left muted or switched goes back.
pub fn restore_after_crash() {
    if let Some(m) = load::<Muted>(QUIET_FILE) {
        if !m.was {
            let _ = audio::set_output_muted(&m.id, false);
        }
        let _ = std::fs::remove_file(state_file(QUIET_FILE));
        crate::log::info("phone speaker: the PC's sound was unmuted (Deck ended while a phone played it)");
    }
    if let Some(was) = load::<MicWas>(MIC_FILE) {
        restore_mic(&was);
        let _ = std::fs::remove_file(state_file(MIC_FILE));
        crate::log::info("phone mic: the default mic was put back (Deck ended while a phone talked)");
    }
}

// ---------------------------------------------------------------- the phone's voice into the cable

/// One phone talking: its samples (16-bit mono at `rate`) are played into VB-CABLE.
pub struct Mic {
    tx: std::sync::mpsc::SyncSender<Vec<i16>>,
    stop: Arc<AtomicBool>,
    /// the player ended (the cable went away, or nothing came for 30 s)
    ended: Arc<AtomicBool>,
    took_default: bool,
}

impl Mic {
    /// `make_default`: "CABLE Output" is the default mic while this phone talks.
    pub fn start(rate: u32, make_default: bool) -> Result<Self, String> {
        let c = cable().ok_or("The PC needs VB-CABLE for the phone mic: set it up in Deck → Phone")?;
        if audio::default_output_id().as_deref() == Some(c.play.as_str()) {
            // the PC's sound and the phone's voice would go into the cable together (and the voice back to a phone speaker)
            return Err("VB-CABLE is this PC's speakers right now: pick your own speakers in Deck → Phone or the Sound card first".into());
        }
        if !(8_000..=96_000).contains(&rate) {
            return Err("unsupported sample rate".into());
        }
        let (tx, rx) = std::sync::mpsc::sync_channel::<Vec<i16>>(200);
        let stop = Arc::new(AtomicBool::new(false));
        let ended = Arc::new(AtomicBool::new(false));
        let (s2, e2) = (stop.clone(), ended.clone());
        let (otx, orx) = std::sync::mpsc::channel::<Result<(), String>>();
        let play = c.play.clone();
        std::thread::Builder::new()
            .name("phone-mic".into())
            .spawn(move || {
                let (played, dry) = play_run(&play, rate, rx, s2, otx);
                e2.store(true, Ordering::SeqCst);
                if played > 0 {
                    crate::log::info(format!("phone mic: {:.1} s played, ran dry {dry} times", played as f64 / rate as f64));
                }
            })
            .map_err(|e| e.to_string())?;
        // the phone hears at once whether the cable could be opened, not only by silence later
        match orx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(())) => {}
            Ok(Err(e)) => return Err(e),
            Err(_) => {
                stop.store(true, Ordering::SeqCst);
                return Err("VB-CABLE didn't answer: restart the PC once after installing it".into());
            }
        }
        if make_default {
            take_default(&c.mic);
        }
        Ok(Self { tx, stop, ended, took_default: make_default })
    }

    /// The player is still running (the phone reconnects when it isn't).
    pub fn alive(&self) -> bool {
        !self.ended.load(Ordering::SeqCst)
    }

    /// A piece of the phone's voice; dropped when the player is far behind (it catches up).
    pub fn push(&self, samples: Vec<i16>) {
        let _ = self.tx.try_send(samples);
    }
}

impl Drop for Mic {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if self.took_default {
            give_back_default();
        }
    }
}

/// The player: keeps a cushion of the phone's voice ahead of the cable (Wi-Fi delivers in bursts;
/// 60 ms, growing after each gap), fills a gap with silence and waits for the cushion again, and
/// drops what piles up beyond it so the delay can't grow. Windows converts 16-bit mono to the cable's own format.
/// Returns the samples played and the times it ran dry (for the log).
fn play_run(device: &str, rate: u32, rx: std::sync::mpsc::Receiver<Vec<i16>>, stop: Arc<AtomicBool>, opened: std::sync::mpsc::Sender<Result<(), String>>) -> (u64, u32) {
    use windows::core::HSTRING;
    use windows::Win32::Media::Audio::*;
    use windows::Win32::System::Com::*;
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    let open = || -> windows::core::Result<(IAudioClient, IAudioRenderClient, u32)> {
        unsafe {
            let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
            let dev = en.GetDevice(&HSTRING::from(device))?;
            let client: IAudioClient = dev.Activate(CLSCTX_ALL, None)?;
            let fmt = WAVEFORMATEX {
                wFormatTag: WAVE_FORMAT_PCM as u16,
                nChannels: 1,
                nSamplesPerSec: rate,
                nAvgBytesPerSec: rate * 2,
                nBlockAlign: 2,
                wBitsPerSample: 16,
                cbSize: 0,
            };
            // 100 ms of room in Windows' buffer; Deck keeps it about a third full
            client.Initialize(AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY, 1_000_000, 0, &fmt, None)?;
            let size = client.GetBufferSize()?;
            let render: IAudioRenderClient = client.GetService()?;
            client.Start()?;
            Ok((client, render, size))
        }
    };
    let (client, render, size) = match open() {
        Ok(x) => x,
        Err(e) => {
            crate::log::warn(format!("phone mic: couldn't open VB-CABLE: {e}"));
            let _ = opened.send(Err(format!("VB-CABLE couldn't be opened ({}): restart the PC once after installing it", e.message())));
            return (0, 0);
        }
    };
    let _ = opened.send(Ok(()));
    let ms = |n: u32| (rate as usize * n as usize) / 1000;
    // the cushion adapts like the phone speaker's: 60 ms to start, 20 ms more after each gap
    // (up to 200 ms), 10 ms less after 30 s without one; what piles up beyond it + 120 ms is dropped
    let device_fill = ms(30) as u32;
    let (floor, cap) = (ms(60), ms(200));
    let mut prime = floor;
    let mut calm = Instant::now();
    let mut q: VecDeque<i16> = VecDeque::with_capacity(ms(400));
    let mut primed = false;
    let mut last = Instant::now();
    let (mut played, mut dry) = (0u64, 0u32);
    while !stop.load(Ordering::SeqCst) {
        let mut got = false;
        while let Ok(piece) = rx.try_recv() {
            q.extend(piece);
            got = true;
        }
        if got {
            last = Instant::now();
        }
        let most = prime + ms(120);
        if q.len() > most {
            let extra = q.len() - prime;
            q.drain(..extra);
        }
        if !primed && q.len() >= prime {
            primed = true;
        }
        // the cable went away (turned off, its driver reset): end, so the phone hears it and reconnects
        let padding = match unsafe { client.GetCurrentPadding() } {
            Ok(p) => p,
            Err(e) => {
                crate::log::warn(format!("phone mic: VB-CABLE stopped answering: {}", e.message()));
                break;
            }
        };
        if padding < device_fill {
            let free = (device_fill - padding).min(size - padding);
            let n = if primed { free.min(q.len() as u32) } else { 0 };
            // nothing to play (a Wi-Fi gap, or still filling): silence, then fill up to 40 ms again
            let silence = if n == 0 && padding < device_fill / 3 { free } else { 0 };
            if n == 0 && primed && silence > 0 {
                primed = false;
                dry += 1;
                prime = (prime + ms(20)).min(cap);
                calm = Instant::now();
            }
            played += n as u64;
            let frames = n + silence;
            if frames > 0 {
                unsafe {
                    if let Ok(buf) = render.GetBuffer(frames) {
                        let out = std::slice::from_raw_parts_mut(buf as *mut i16, frames as usize);
                        for (i, s) in out.iter_mut().enumerate() {
                            *s = if (i as u32) < n { q.pop_front().unwrap_or(0) } else { 0 };
                        }
                        let _ = render.ReleaseBuffer(frames, 0);
                    }
                }
            }
        }
        if calm.elapsed() > Duration::from_secs(30) && prime > floor {
            prime = (prime - ms(10)).max(floor);
            calm = Instant::now();
        }
        // the phone stopped sending (it went to sleep): don't hold the cable forever
        if last.elapsed() > Duration::from_secs(30) {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    unsafe {
        let _ = client.Stop();
    }
    (played, dry)
}

/// The phone's mic message: 16-bit little-endian mono samples.
pub fn samples(bytes: &[u8]) -> Vec<i16> {
    bytes.as_chunks::<2>().0.iter().map(|b| i16::from_le_bytes(*b)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_vb_audio_signed_setups_run() {
        assert!(signed_by_vb_audio("Valid|CN=BUREL VINCENT Entrepreneur individuel, O=BUREL VINCENT Entrepreneur individuel, C=FR"));
        assert!(!signed_by_vb_audio("NotSigned|"));
        assert!(!signed_by_vb_audio("HashMismatch|CN=BUREL VINCENT Entrepreneur individuel"));
        assert!(!signed_by_vb_audio("Valid|CN=Someone Else"));
        assert!(!signed_by_vb_audio(""));
    }

    /// The player on the default output (quietly): 1.5 s of 4.2 kHz sent like Wi-Fi does, 10 ms
    /// pieces in bursts of up to 60 ms. Record the PC's sound meanwhile to hear it come out whole.
    #[test]
    #[ignore]
    fn player_plays_a_bursty_stream_whole() {
        crate::util::com();
        let dev = audio::default_output_id().expect("an output");
        let (tx, rx) = std::sync::mpsc::sync_channel::<Vec<i16>>(200);
        let stop = Arc::new(AtomicBool::new(false));
        let s2 = stop.clone();
        let (otx, _orx) = std::sync::mpsc::channel();
        let t = std::thread::spawn(move || play_run(&dev, 48_000, rx, s2, otx));
        // opening the device takes a moment; what piles up meanwhile is dropped (no added delay)
        std::thread::sleep(Duration::from_millis(1500));
        let mut n = 0usize;
        for burst in 0..40 {
            let pieces = [1, 6, 3, 5][burst % 4];
            for _ in 0..pieces {
                let p: Vec<i16> = (0..480).map(|i| (((n + i) as f32 * 2.0 * std::f32::consts::PI * 4200.0 / 48_000.0).sin() * 3000.0) as i16).collect();
                n += 480;
                tx.send(p).unwrap();
            }
            std::thread::sleep(Duration::from_millis(10 * pieces as u64));
        }
        std::thread::sleep(Duration::from_millis(300));
        stop.store(true, Ordering::SeqCst);
        let (played, dry) = t.join().unwrap();
        println!("sent {} ms, played {} ms, ran dry {dry} times", n / 48, played / 48);
    }

    #[test]
    fn mic_bytes_read_as_little_endian_samples() {
        assert_eq!(samples(&[0x01, 0x00, 0xff, 0x7f, 0x00, 0x80, 0x7]), vec![1, 32767, -32768]);
    }
}
