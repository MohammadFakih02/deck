//! Music analysis for the music lighting / display modes: WASAPI loopback capture of whatever the speakers play,
//! split into bass / mids / treble with simple filters, plus beat detection.
use windows::Win32::Media::Audio::*;
use windows::Win32::System::Com::*;

/// Current music levels, each 0..1 (auto-gained).
#[derive(Clone, Copy, Default, Debug)]
pub struct Levels {
    pub level: f32,
    pub bass: f32,
    pub mid: f32,
    pub treble: f32,
    /// 1.0 right on a beat, fading to 0
    pub beat: f32,
    /// counts beats (effects change colour on each one)
    pub beats: u32,
    /// loudness of the game in front (0..1), 1.0 right at a sudden loud moment then fading, and seconds since that moment
    pub game: f32,
    pub impact: f32,
    pub since_impact: f32,
}

struct Band {
    peak: f32,
    out: f32,
}

impl Band {
    fn new() -> Band {
        Band { peak: 0.01, out: 0.0 }
    }
    /// auto-gain with a slowly falling peak, fast attack, slower release
    fn update(&mut self, rms: f32) -> f32 {
        self.peak = (self.peak * 0.996).max(rms).max(0.004);
        let target = (rms / self.peak).clamp(0.0, 1.0);
        self.out = if target > self.out { target } else { self.out * 0.82 + target * 0.18 };
        self.out
    }
}

pub struct Loopback {
    client: IAudioClient,
    capture: IAudioCaptureClient,
    channels: usize,
    float: bool,
    a_bass: f32, // one-pole low-pass coefficients for ~150 Hz and ~2 kHz
    a_mid: f32,
    lp1: f32,
    lp2: f32,
    bands: [Band; 4], // level, bass, mid, treble
    bass_avg: f32,
    beat: f32,
    beats: u32,
    since_beat: u32,
}

impl Loopback {
    /// Opens the default playback device in loopback mode. Call from a thread that stays alive.
    pub fn open() -> Option<Loopback> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
            let dev = en.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
            let client: IAudioClient = dev.Activate(CLSCTX_ALL, None).ok()?;
            let fmt = client.GetMixFormat().ok()?;
            let channels = (*fmt).nChannels.max(1) as usize;
            let bits = (*fmt).wBitsPerSample;
            let rate = (*fmt).nSamplesPerSec.max(8000) as f32;
            client.Initialize(AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK, 1_000_000, 0, fmt, None).ok()?;
            CoTaskMemFree(Some(fmt as *const _));
            let capture: IAudioCaptureClient = client.GetService().ok()?;
            client.Start().ok()?;
            let coef = |hz: f32| 1.0 - (-2.0 * std::f32::consts::PI * hz / rate).exp();
            Some(Loopback {
                client,
                capture,
                channels,
                float: bits == 32,
                a_bass: coef(150.0),
                a_mid: coef(2000.0),
                lp1: 0.0,
                lp2: 0.0,
                bands: [Band::new(), Band::new(), Band::new(), Band::new()],
                bass_avg: 0.0,
                beat: 0.0,
                beats: 0,
                since_beat: 0,
            })
        }
    }

    /// Drains captured audio and returns the current levels. Call every ~15 ms.
    pub fn poll(&mut self) -> Levels {
        let (mut s_all, mut s_bass, mut s_mid, mut s_tre) = (0f64, 0f64, 0f64, 0f64);
        let mut count = 0usize;
        unsafe {
            while let Ok(n) = self.capture.GetNextPacketSize() {
                if n == 0 {
                    break;
                }
                let mut data: *mut u8 = std::ptr::null_mut();
                let mut frames = 0u32;
                let mut flags = 0u32;
                if self.capture.GetBuffer(&mut data, &mut frames, &mut flags, None, None).is_err() {
                    break;
                }
                let silent = flags & (AUDCLNT_BUFFERFLAGS_SILENT.0 as u32) != 0;
                for s in 0..frames as usize {
                    let mut v = 0f32;
                    if !silent && !data.is_null() {
                        for c in 0..self.channels {
                            let idx = s * self.channels + c;
                            v += if self.float { *(data as *const f32).add(idx) } else { *(data as *const i16).add(idx) as f32 / 32768.0 };
                        }
                        v /= self.channels as f32;
                    }
                    self.lp1 += self.a_bass * (v - self.lp1);
                    self.lp2 += self.a_mid * (v - self.lp2);
                    let (b, m, t) = (self.lp1, self.lp2 - self.lp1, v - self.lp2);
                    s_all += (v * v) as f64;
                    s_bass += (b * b) as f64;
                    s_mid += (m * m) as f64;
                    s_tre += (t * t) as f64;
                    count += 1;
                }
                let _ = self.capture.ReleaseBuffer(frames);
            }
        }
        let rms = |s: f64| if count > 0 { (s / count as f64).sqrt() as f32 } else { 0.0 };
        let (r_all, r_bass, r_mid, r_tre) = (rms(s_all), rms(s_bass), rms(s_mid), rms(s_tre));
        let level = self.bands[0].update(r_all);
        let bass = self.bands[1].update(r_bass);
        let mid = self.bands[2].update(r_mid);
        let treble = self.bands[3].update(r_tre);
        // beat: bass clearly above its recent average, at most ~6 per second
        self.since_beat += 1;
        if r_bass > self.bass_avg * 1.45 && r_bass > 0.002 && self.since_beat > 10 {
            self.beat = 1.0;
            self.beats = self.beats.wrapping_add(1);
            self.since_beat = 0;
        } else {
            self.beat *= 0.86;
        }
        self.bass_avg = self.bass_avg * 0.97 + r_bass * 0.03;
        Levels { level, bass, mid, treble, beat: self.beat, beats: self.beats, ..Default::default() }
    }
}

impl Drop for Loopback {
    fn drop(&mut self) {
        unsafe {
            let _ = self.client.Stop();
        }
    }
}
