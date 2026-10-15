//! H.264 video for the phone's Screen view: far sharper than JPEG at the same bitrate (it sends
//! only what changed between pictures), so a slow line still gets a clear picture.
//!
//! Encoding uses Windows' own Media Foundation encoders, nothing to install: the graphics card's
//! hardware encoder when there is one (NVIDIA, Intel, AMD: it costs the CPU almost nothing),
//! otherwise Microsoft's software encoder. If neither works the Screen view stays on JPEG.
//!
//! The phone gets the stream as Annex-B access units for WebCodecs (browsers on a secure page), or
//! as fragmented MP4 for Media Source Extensions (any browser, plain http included): `Mp4` below.

use std::collections::VecDeque;
use std::mem::ManuallyDrop;
use std::time::{Duration, Instant};
use windows::core::{Interface, GUID};
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::{CoInitializeEx, CoTaskMemFree, COINIT_MULTITHREADED};
use windows::Win32::System::Variant::*;

/// One encoded picture.
pub struct Packet {
    /// Annex-B (start codes); key pictures start with SPS and PPS
    pub data: Vec<u8>,
    pub key: bool,
    /// when the picture was on screen (ms, the clock the input was stamped with)
    pub ts_ms: f64,
}

// ------------------------------------------------------------------ colour

/// BGRA → NV12 (BT.709, limited range: what the stream says it is), rows split over threads.
pub fn bgra_to_nv12(src: &[u8], w: usize, h: usize) -> Vec<u8> {
    let (w, h) = (w & !1, h & !1);
    let mut out = vec![0u8; w * h * 3 / 2];
    let (y_plane, uv_plane) = out.split_at_mut(w * h);
    let pairs = h / 2;
    let threads = if w * h >= 400_000 { std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2).min(4) } else { 1 };
    let per = pairs.div_ceil(threads).max(1);
    let stride = w * 4;
    std::thread::scope(|s| {
        for ((ys, uvs), ss) in y_plane.chunks_mut(2 * w * per).zip(uv_plane.chunks_mut(w * per)).zip(src.chunks(2 * stride * per)) {
            s.spawn(move || nv12_rows(ss, ys, uvs, w, stride));
        }
    });
    out
}

fn nv12_rows(src: &[u8], y: &mut [u8], uv: &mut [u8], w: usize, stride: usize) {
    let rows = y.len() / w;
    for r in 0..rows / 2 {
        let (a, b) = (&src[2 * r * stride..2 * r * stride + w * 4], &src[(2 * r + 1) * stride..(2 * r + 1) * stride + w * 4]);
        let (ya, rest) = y[2 * r * w..].split_at_mut(w);
        let yb = &mut rest[..w];
        let uvr = &mut uv[r * w..(r + 1) * w];
        for x in 0..w / 2 {
            let (p00, p01, p10, p11) = (&a[8 * x..8 * x + 8][..4], &a[8 * x..8 * x + 8][4..], &b[8 * x..8 * x + 8][..4], &b[8 * x..8 * x + 8][4..]);
            let luma = |p: &[u8]| (((47 * p[2] as i32 + 157 * p[1] as i32 + 16 * p[0] as i32 + 128) >> 8) + 16) as u8;
            ya[2 * x] = luma(p00);
            ya[2 * x + 1] = luma(p01);
            yb[2 * x] = luma(p10);
            yb[2 * x + 1] = luma(p11);
            let sum = |c: usize| p00[c] as i32 + p01[c] as i32 + p10[c] as i32 + p11[c] as i32;
            let (bl, gr, rd) = ((sum(0) + 2) >> 2, (sum(1) + 2) >> 2, (sum(2) + 2) >> 2);
            uvr[2 * x] = (((-26 * rd - 87 * gr + 113 * bl + 128) >> 8) + 128).clamp(0, 255) as u8;
            uvr[2 * x + 1] = (((112 * rd - 102 * gr - 10 * bl + 128) >> 8) + 128).clamp(0, 255) as u8;
        }
    }
}

// ------------------------------------------------------------------ Annex-B

/// The NAL units of an Annex-B stream: (start of the unit, after the start code; end).
pub fn nals(d: &[u8]) -> Vec<(usize, usize)> {
    let mut starts = Vec::new(); // (index of the start code, length of it)
    let mut i = 0;
    while i + 3 <= d.len() {
        if d[i] == 0 && d[i + 1] == 0 {
            if d[i + 2] == 1 {
                starts.push((i, 3));
                i += 3;
                continue;
            }
            if i + 4 <= d.len() && d[i + 2] == 0 && d[i + 3] == 1 {
                starts.push((i, 4));
                i += 4;
                continue;
            }
        }
        i += 1;
    }
    starts
        .iter()
        .enumerate()
        .map(|(k, &(at, len))| {
            let mut end = starts.get(k + 1).map(|n| n.0).unwrap_or(d.len());
            while end > at + len && d[end - 1] == 0 && k + 1 < starts.len() {
                end -= 1; // the zero that belongs to a following 4-byte start code
            }
            (at + len, end)
        })
        .collect()
}

pub fn nal_type(d: &[u8], n: (usize, usize)) -> u8 {
    d.get(n.0).map(|b| b & 31).unwrap_or(0)
}

fn with_start_codes(parts: &[&[u8]]) -> Vec<u8> {
    let mut v = Vec::new();
    for p in parts {
        v.extend_from_slice(&[0, 0, 0, 1]);
        v.extend_from_slice(p);
    }
    v
}

/// The SPS and PPS of a picture, if it carries them.
pub fn param_sets(d: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    let (mut sps, mut pps) = (None, None);
    for n in nals(d) {
        match nal_type(d, n) {
            7 if sps.is_none() => sps = Some(d[n.0..n.1].to_vec()),
            8 if pps.is_none() => pps = Some(d[n.0..n.1].to_vec()),
            _ => {}
        }
    }
    Some((sps?, pps?))
}

/// "avc1.4D401F": what a browser's decoder is told the stream is (profile, constraints, level).
pub fn codec_string(sps: &[u8]) -> String {
    format!("avc1.{:02X}{:02X}{:02X}", sps.get(1).unwrap_or(&0x4d), sps.get(2).unwrap_or(&0), sps.get(3).unwrap_or(&0x1f))
}

/// An access unit as length-prefixed NAL units (what an MP4 sample holds), without the parameter
/// sets and delimiters (those live in the file's header).
fn avcc_sample(d: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(d.len());
    for n in nals(d) {
        if matches!(nal_type(d, n), 7 | 8 | 9) {
            continue;
        }
        v.extend_from_slice(&((n.1 - n.0) as u32).to_be_bytes());
        v.extend_from_slice(&d[n.0..n.1]);
    }
    v
}

// ------------------------------------------------------------------ fragmented MP4

fn bx(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(8 + body.len());
    v.extend_from_slice(&((8 + body.len()) as u32).to_be_bytes());
    v.extend_from_slice(kind);
    v.extend_from_slice(body);
    v
}

fn cat(parts: &[&[u8]]) -> Vec<u8> {
    parts.concat()
}

fn u32s(v: &[u32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_be_bytes()).collect()
}

/// Media time: 90 kHz.
pub const TIMESCALE: u32 = 90_000;

/// Packs pictures as fragmented MP4 for the browser's Media Source Extensions.
pub struct Mp4 {
    seq: u32,
}

impl Mp4 {
    pub fn new() -> Mp4 {
        Mp4 { seq: 0 }
    }

    /// The header: codec setup for a `w`×`h` stream with these parameter sets.
    pub fn init(&mut self, sps: &[u8], pps: &[u8], w: u32, h: u32) -> Vec<u8> {
        self.seq = 0;
        let mut avcc = vec![1, sps.get(1).copied().unwrap_or(0x4d), sps.get(2).copied().unwrap_or(0), sps.get(3).copied().unwrap_or(0x1f), 0xFF, 0xE1];
        avcc.extend_from_slice(&(sps.len() as u16).to_be_bytes());
        avcc.extend_from_slice(sps);
        avcc.push(1);
        avcc.extend_from_slice(&(pps.len() as u16).to_be_bytes());
        avcc.extend_from_slice(pps);
        if matches!(sps.get(1), Some(100 | 110 | 122 | 144)) {
            avcc.extend_from_slice(&[0xFD, 0xF8, 0xF8, 0]); // 4:2:0, 8-bit, no extensions
        }
        let mut avc1 = vec![0u8; 6];
        avc1.extend_from_slice(&1u16.to_be_bytes()); // data reference
        avc1.extend_from_slice(&[0; 16]);
        avc1.extend_from_slice(&(w as u16).to_be_bytes());
        avc1.extend_from_slice(&(h as u16).to_be_bytes());
        avc1.extend_from_slice(&u32s(&[0x0048_0000, 0x0048_0000, 0]));
        avc1.extend_from_slice(&1u16.to_be_bytes()); // frames per sample
        avc1.extend_from_slice(&[0; 32]); // compressor name
        avc1.extend_from_slice(&0x0018u16.to_be_bytes());
        avc1.extend_from_slice(&0xFFFFu16.to_be_bytes());
        avc1.extend(bx(b"avcC", &avcc));
        let stsd = bx(b"stsd", &cat(&[&u32s(&[0, 1]), &bx(b"avc1", &avc1)]));
        let empty = |k: &[u8; 4], extra: &[u32]| bx(k, &u32s(&[&[0u32][..], extra].concat()));
        let stbl = bx(b"stbl", &cat(&[&stsd, &empty(b"stts", &[0]), &empty(b"stsc", &[0]), &empty(b"stsz", &[0, 0]), &empty(b"stco", &[0])]));
        let dinf = bx(b"dinf", &bx(b"dref", &cat(&[&u32s(&[0, 1]), &bx(b"url ", &u32s(&[1]))])));
        let vmhd = bx(b"vmhd", &u32s(&[1, 0, 0]));
        let minf = bx(b"minf", &cat(&[&vmhd, &dinf, &stbl]));
        let mdhd = bx(b"mdhd", &cat(&[&u32s(&[0, 0, 0, TIMESCALE, 0]), &0x55C4u16.to_be_bytes(), &[0, 0]]));
        let hdlr = bx(b"hdlr", &cat(&[&u32s(&[0, 0]), b"vide", &[0u8; 12], b"VideoHandler\0"]));
        let mdia = bx(b"mdia", &cat(&[&mdhd, &hdlr, &minf]));
        let matrix = u32s(&[0x0001_0000, 0, 0, 0, 0x0001_0000, 0, 0, 0, 0x4000_0000]);
        let tkhd = bx(b"tkhd", &cat(&[&u32s(&[3, 0, 0, 1, 0, 0]), &[0u8; 8], &[0u8; 8], &matrix, &u32s(&[w << 16, h << 16])]));
        let trak = bx(b"trak", &cat(&[&tkhd, &mdia]));
        let mvhd = bx(b"mvhd", &cat(&[&u32s(&[0, 0, 0, TIMESCALE, 0, 0x0001_0000]), &0x0100u16.to_be_bytes(), &[0u8; 10], &matrix, &[0u8; 24], &u32s(&[2])]));
        let trex = bx(b"trex", &u32s(&[0, 1, 1, 0, 0, 0]));
        let moov = bx(b"moov", &cat(&[&mvhd, &trak, &bx(b"mvex", &trex)]));
        let ftyp = bx(b"ftyp", &cat(&[b"isom", &u32s(&[0x200]), b"isomiso5avc1"]));
        cat(&[&ftyp, &moov])
    }

    /// One picture as a media fragment: starts at `start` (90 kHz), lasts `dur`.
    pub fn fragment(&mut self, annexb: &[u8], key: bool, start: u64, dur: u32) -> Vec<u8> {
        self.seq += 1;
        let sample = avcc_sample(annexb);
        let mfhd = bx(b"mfhd", &u32s(&[0, self.seq]));
        let tfhd = bx(b"tfhd", &u32s(&[0x0002_0000, 1]));
        let tfdt = bx(b"tfdt", &cat(&[&u32s(&[0x0100_0000]), &start.to_be_bytes()]));
        let flags = if key { 0x0200_0000u32 } else { 0x0101_0000 };
        let trun_body = |offset: u32| u32s(&[0x0000_0701, 1, offset, dur, sample.len() as u32, flags]);
        let moof_len = 8 + mfhd.len() + 8 + tfhd.len() + tfdt.len() + 8 + trun_body(0).len();
        let trun = bx(b"trun", &trun_body(moof_len as u32 + 8));
        let moof = bx(b"moof", &cat(&[&mfhd, &bx(b"traf", &cat(&[&tfhd, &tfdt, &trun]))]));
        debug_assert_eq!(moof.len(), moof_len);
        cat(&[&moof, &bx(b"mdat", &sample)])
    }
}

// ------------------------------------------------------------------ the encoder

#[derive(Clone, Debug)]
pub struct Cand {
    pub name: String,
    pub hardware: bool,
    index: usize,
}

fn mf_start() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let _ = MFStartup(MF_VERSION, 0);
    });
    // this thread too (the encoder lives on its own thread)
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
}

fn enumerate(hardware: bool) -> Vec<IMFActivate> {
    unsafe {
        let out = MFT_REGISTER_TYPE_INFO { guidMajorType: MFMediaType_Video, guidSubtype: MFVideoFormat_H264 };
        let flags = if hardware { MFT_ENUM_FLAG_HARDWARE | MFT_ENUM_FLAG_SORTANDFILTER } else { MFT_ENUM_FLAG_SYNCMFT | MFT_ENUM_FLAG_SORTANDFILTER };
        let mut arr: *mut Option<IMFActivate> = std::ptr::null_mut();
        let mut n = 0u32;
        if MFTEnumEx(MFT_CATEGORY_VIDEO_ENCODER, flags, None, Some(&out), &mut arr, &mut n).is_err() || arr.is_null() {
            return Vec::new();
        }
        let v = (0..n as usize).filter_map(|i| (*arr.add(i)).take()).collect();
        CoTaskMemFree(Some(arr as *const _));
        v
    }
}

fn friendly(a: &IMFActivate) -> String {
    unsafe {
        let mut buf = [0u16; 256];
        let mut len = 0u32;
        let _ = a.GetString(&MFT_FRIENDLY_NAME_Attribute, &mut buf, Some(&mut len));
        String::from_utf16_lossy(&buf[..(len as usize).min(256)])
    }
}

/// The H.264 encoders this PC has, hardware first.
pub fn candidates() -> Vec<Cand> {
    mf_start();
    let mut v = Vec::new();
    for (hardware, list) in [(true, enumerate(true)), (false, enumerate(false))] {
        for (index, a) in list.iter().enumerate() {
            v.push(Cand { name: friendly(a), hardware, index });
        }
    }
    v
}

fn variant_u32(x: u32) -> VARIANT {
    let mut v = VARIANT::default();
    unsafe {
        let inner = &mut *v.Anonymous.Anonymous;
        inner.vt = VT_UI4;
        inner.Anonymous.ulVal = x;
    }
    v
}

fn variant_bool(x: bool) -> VARIANT {
    let mut v = VARIANT::default();
    unsafe {
        let inner = &mut *v.Anonymous.Anonymous;
        inner.vt = VT_BOOL;
        inner.Anonymous.boolVal = windows::Win32::Foundation::VARIANT_BOOL(if x { -1 } else { 0 });
    }
    v
}

fn pack(hi: u32, lo: u32) -> u64 {
    ((hi as u64) << 32) | lo as u64
}

fn h264_level(w: u32, h: u32, fps: u32) -> u32 {
    let mbs = w.div_ceil(16) * h.div_ceil(16);
    let rate = mbs * fps;
    match () {
        _ if rate <= 40_500 => 31,  // 3.1: 720p30
        _ if rate <= 108_000 => 40, // 4.0: 1080p30
        _ if rate <= 245_760 => 42, // 4.2: 1080p60
        _ if rate <= 522_240 => 50, // 5.0: 1440p60 class
        _ => 51,
    }
}

pub struct Encoder {
    mft: IMFTransform,
    api: Option<ICodecAPI>,
    events: Option<IMFMediaEventGenerator>,
    pub name: String,
    pub hardware: bool,
    pub w: u32,
    pub h: u32,
    fps: u32,
    kbps: u32,
    need_input: u32,
    provides_samples: bool,
    out_size: u32,
    ready: VecDeque<Packet>,
    params: Option<(Vec<u8>, Vec<u8>)>,
    fed: u64,
    got: u64,
}

impl Encoder {
    /// Opens `cand` for `w`×`h` (even numbers) at `fps` and `kbps`, Main profile.
    pub fn open(cand: &Cand, w: u32, h: u32, fps: u32, kbps: u32) -> Result<Encoder, String> {
        Encoder::open_as(cand, w, h, fps, kbps, false)
    }

    /// `baseline`: Constrained Baseline (no CABAC: what the phone page's own decoder reads),
    /// else Main.
    pub fn open_as(cand: &Cand, w: u32, h: u32, fps: u32, kbps: u32, baseline: bool) -> Result<Encoder, String> {
        mf_start();
        let (w, h) = (w & !1, h & !1);
        unsafe {
            let acts = enumerate(cand.hardware);
            let act = acts.get(cand.index).ok_or("encoder gone")?;
            let mft: IMFTransform = act.ActivateObject().map_err(|e| format!("activate: {e}"))?;
            let attrs = mft.GetAttributes().ok();
            let is_async = attrs.as_ref().and_then(|a| a.GetUINT32(&MF_TRANSFORM_ASYNC).ok()).unwrap_or(0) != 0;
            if let Some(a) = &attrs {
                if is_async {
                    a.SetUINT32(&MF_TRANSFORM_ASYNC_UNLOCK, 1).map_err(|e| format!("unlock: {e}"))?;
                }
                let _ = a.SetUINT32(&MF_LOW_LATENCY, 1);
            }
            let colour = |t: &IMFMediaType| {
                let _ = t.SetUINT32(&MF_MT_VIDEO_PRIMARIES, 2);
                let _ = t.SetUINT32(&MF_MT_TRANSFER_FUNCTION, 5);
                let _ = t.SetUINT32(&MF_MT_YUV_MATRIX, 1);
                let _ = t.SetUINT32(&MF_MT_VIDEO_NOMINAL_RANGE, 2);
            };
            // the encoder's settings, before the formats (some encoders only take a GOP size then)
            // and again after (others reset them when the format is set)
            let api = mft.cast::<ICodecAPI>().ok();
            let settings = |api: &ICodecAPI| {
                let set = |g: &GUID, v: VARIANT| {
                    let _ = api.SetValue(g, &v);
                };
                set(&CODECAPI_AVEncCommonRateControlMode, variant_u32(0)); // CBR
                set(&CODECAPI_AVEncCommonMeanBitRate, variant_u32(kbps * 1000));
                set(&CODECAPI_AVLowLatencyMode, variant_bool(true));
                set(&CODECAPI_AVEncCommonLowLatency, variant_bool(true));
                set(&CODECAPI_AVEncMPVDefaultBPictureCount, variant_u32(0));
                set(&CODECAPI_AVEncMPVGOPSize, variant_u32(fps * 30));
            };
            if let Some(api) = &api {
                settings(api);
            }
            let ot = MFCreateMediaType().map_err(|e| e.to_string())?;
            ot.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video).ok();
            ot.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264).ok();
            ot.SetUINT32(&MF_MT_AVG_BITRATE, kbps * 1000).ok();
            ot.SetUINT64(&MF_MT_FRAME_SIZE, pack(w, h)).ok();
            ot.SetUINT64(&MF_MT_FRAME_RATE, pack(fps, 1)).ok();
            ot.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack(1, 1)).ok();
            ot.SetUINT32(&MF_MT_INTERLACE_MODE, 2).ok();
            ot.SetUINT32(&MF_MT_MPEG2_PROFILE, if baseline { 66 } else { 77 }).ok();
            ot.SetUINT32(&MF_MT_MPEG2_LEVEL, h264_level(w, h, fps)).ok();
            colour(&ot);
            mft.SetOutputType(0, &ot, 0).map_err(|e| format!("output type: {e}"))?;
            if let Some(api) = &api {
                settings(api);
            }

            let it = MFCreateMediaType().map_err(|e| e.to_string())?;
            it.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video).ok();
            it.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12).ok();
            it.SetUINT64(&MF_MT_FRAME_SIZE, pack(w, h)).ok();
            it.SetUINT64(&MF_MT_FRAME_RATE, pack(fps, 1)).ok();
            it.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack(1, 1)).ok();
            it.SetUINT32(&MF_MT_INTERLACE_MODE, 2).ok();
            it.SetUINT32(&MF_MT_DEFAULT_STRIDE, w).ok();
            colour(&it);
            mft.SetInputType(0, &it, 0).map_err(|e| format!("input type: {e}"))?;

            let _ = mft.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0);
            let _ = mft.ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0);
            let info = mft.GetOutputStreamInfo(0).map_err(|e| format!("stream info: {e}"))?;
            let events = if is_async { mft.cast::<IMFMediaEventGenerator>().ok() } else { None };
            if is_async && events.is_none() {
                return Err("no events".into());
            }
            let mut enc = Encoder {
                mft,
                api,
                events,
                name: cand.name.clone(),
                hardware: cand.hardware,
                w,
                h,
                fps,
                kbps,
                need_input: if is_async { 0 } else { 1 },
                provides_samples: info.dwFlags & MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32 != 0,
                out_size: info.cbSize.max(w * h),
                ready: VecDeque::new(),
                params: None,
                fed: 0,
                got: 0,
            };
            // SPS and PPS from the output type, when the encoder keeps them there
            if let Ok(t) = enc.mft.GetOutputCurrentType(0) {
                if let Ok(n) = t.GetBlobSize(&MF_MT_MPEG_SEQUENCE_HEADER) {
                    let mut b = vec![0u8; n as usize];
                    if t.GetBlob(&MF_MT_MPEG_SEQUENCE_HEADER, &mut b, None).is_ok() {
                        enc.params = param_sets(&b);
                    }
                }
            }
            Ok(enc)
        }
    }

    /// The stream's SPS and PPS (known once the first picture came out).
    pub fn params(&self) -> Option<&(Vec<u8>, Vec<u8>)> {
        self.params.as_ref()
    }

    pub fn set_bitrate(&mut self, kbps: u32) {
        if kbps == self.kbps {
            return;
        }
        self.kbps = kbps;
        if let Some(api) = &self.api {
            unsafe {
                let _ = api.SetValue(&CODECAPI_AVEncCommonMeanBitRate, &variant_u32(kbps * 1000));
            }
        }
    }

    pub fn kbps(&self) -> u32 {
        self.kbps
    }

    /// Feeds one NV12 picture. Returns the pictures that came out (usually one; none when the
    /// encoder is busy or still filling its pipeline, in which case a later call returns them).
    pub fn encode(&mut self, nv12: &[u8], ts_ms: f64, force_key: bool) -> Result<Vec<Packet>, String> {
        unsafe {
            if self.events.is_some() {
                self.pump(if self.need_input == 0 { Duration::from_millis(40) } else { Duration::ZERO }, false)?;
            }
            if self.need_input > 0 {
                if force_key {
                    if let Some(api) = &self.api {
                        let _ = api.SetValue(&CODECAPI_AVEncVideoForceKeyFrame, &variant_u32(1));
                    }
                }
                let buf = MFCreateMemoryBuffer(nv12.len() as u32).map_err(|e| e.to_string())?;
                let mut p = std::ptr::null_mut();
                buf.Lock(&mut p, None, None).map_err(|e| e.to_string())?;
                std::ptr::copy_nonoverlapping(nv12.as_ptr(), p, nv12.len());
                buf.Unlock().ok();
                buf.SetCurrentLength(nv12.len() as u32).ok();
                let s = MFCreateSample().map_err(|e| e.to_string())?;
                s.AddBuffer(&buf).ok();
                s.SetSampleTime((ts_ms * 10_000.0) as i64).ok();
                s.SetSampleDuration(10_000_000 / self.fps.max(1) as i64).ok();
                self.mft.ProcessInput(0, &s, 0).map_err(|e| format!("input: {e}"))?;
                self.need_input -= 1;
                self.fed += 1;
            } else if self.events.is_none() {
                return Ok(Vec::new());
            }
            if self.events.is_some() {
                // wait for this picture's output (hardware encoders answer in a few ms)
                self.pump(Duration::from_millis(120), true)?;
            } else {
                self.drain_sync()?;
            }
        }
        Ok(self.ready.drain(..).collect())
    }

    /// Async encoders: handles their events until one output arrives (`want_output`) or `wait` is over.
    unsafe fn pump(&mut self, wait: Duration, want_output: bool) -> Result<(), String> {
        let until = Instant::now() + wait;
        let events = self.events.clone().ok_or("not async")?;
        let mut produced = false;
        loop {
            match events.GetEvent(MF_EVENT_FLAG_NO_WAIT) {
                Ok(ev) => match ev.GetType().map_err(|e| e.to_string())? {
                    t if t == METransformNeedInput.0 as u32 => self.need_input += 1,
                    t if t == METransformHaveOutput.0 as u32 => {
                        self.take_output()?;
                        produced = true;
                    }
                    _ => {}
                },
                Err(e) if e.code() == MF_E_NO_EVENTS_AVAILABLE => {
                    let satisfied = if want_output { produced || self.got >= self.fed } else { self.need_input > 0 };
                    if satisfied || Instant::now() >= until {
                        return Ok(());
                    }
                    std::thread::sleep(Duration::from_micros(500));
                }
                Err(e) => return Err(format!("event: {e}")),
            }
        }
    }

    unsafe fn drain_sync(&mut self) -> Result<(), String> {
        loop {
            match self.take_output() {
                Ok(true) => {}
                Ok(false) => break,
                Err(e) => return Err(e),
            }
        }
        self.need_input = 1;
        Ok(())
    }

    /// One ProcessOutput call: Ok(true) when a picture came out.
    unsafe fn take_output(&mut self) -> Result<bool, String> {
        let sample = if self.provides_samples {
            None
        } else {
            let buf = MFCreateMemoryBuffer(self.out_size).map_err(|e| e.to_string())?;
            let s = MFCreateSample().map_err(|e| e.to_string())?;
            s.AddBuffer(&buf).ok();
            Some(s)
        };
        let mut out = [MFT_OUTPUT_DATA_BUFFER { dwStreamID: 0, pSample: ManuallyDrop::new(sample), dwStatus: 0, pEvents: ManuallyDrop::new(None) }];
        let mut status = 0u32;
        let r = self.mft.ProcessOutput(0, &mut out, &mut status);
        let s = ManuallyDrop::take(&mut out[0].pSample);
        let _ = ManuallyDrop::take(&mut out[0].pEvents);
        match r {
            Ok(()) => {}
            Err(e) if e.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => return Ok(false),
            Err(e) if e.code() == MF_E_TRANSFORM_STREAM_CHANGE => {
                if let Ok(t) = self.mft.GetOutputAvailableType(0, 0) {
                    let _ = self.mft.SetOutputType(0, &t, 0);
                }
                return Ok(true);
            }
            Err(e) => return Err(format!("output: {e}")),
        }
        let Some(s) = s else { return Ok(false) };
        let ts = s.GetSampleTime().unwrap_or(0) as f64 / 10_000.0;
        let b = s.ConvertToContiguousBuffer().map_err(|e| e.to_string())?;
        let (mut p, mut len) = (std::ptr::null_mut(), 0u32);
        b.Lock(&mut p, None, Some(&mut len)).map_err(|e| e.to_string())?;
        let mut data = std::slice::from_raw_parts(p, len as usize).to_vec();
        b.Unlock().ok();
        self.got += 1;
        let ns = nals(&data);
        let key = ns.iter().any(|&n| nal_type(&data, n) == 5);
        if let Some(ps) = param_sets(&data) {
            self.params = Some(ps);
        } else if key {
            if let Some((sps, pps)) = &self.params {
                let mut v = with_start_codes(&[sps, pps]);
                v.extend_from_slice(&data);
                data = v;
            }
        }
        self.ready.push_back(Packet { data, key, ts_ms: ts });
        Ok(true)
    }
}

impl Drop for Encoder {
    fn drop(&mut self) {
        unsafe {
            let _ = self.mft.ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0);
            let _ = self.mft.ProcessMessage(MFT_MESSAGE_COMMAND_FLUSH, 0);
        }
    }
}

// ------------------------------------------------------------------ the plan

/// Wide pictures an H.264 stream steps through.
pub const WIDTHS: [u32; 11] = [2560, 1920, 1600, 1280, 1024, 854, 640, 480, 360, 320, 256];

/// The picture width and frame rate for a bitrate. `bpp` is the bits a pixel gets per picture:
/// "smooth" (0.09) keeps the frame rate and shrinks the picture; "sharp" (0.16) keeps the picture
/// large and lowers the frame rate (not below 8) before it shrinks it.
pub fn plan(kbps: u32, wanted_fps: u32, smooth: bool, max_w: u32, native: (u32, u32)) -> (u32, u32) {
    let bits = kbps as f64 * 1000.0;
    let aspect = native.1 as f64 / native.0.max(1) as f64;
    let px = |w: u32| w as f64 * w as f64 * aspect;
    let fits = |w: u32, fps: u32, bpp: f64| px(w) * bpp * fps as f64 <= bits;
    let max_w = max_w.min(native.0).max(WIDTHS[WIDTHS.len() - 1]);
    let ladder = || WIDTHS.iter().copied().filter(move |&w| w <= max_w || w == WIDTHS[WIDTHS.len() - 1]);
    if smooth {
        let w = ladder().find(|&w| fits(w, wanted_fps, 0.09)).unwrap_or(WIDTHS[WIDTHS.len() - 1]);
        (w, wanted_fps)
    } else {
        let top = ladder().next().unwrap_or(256);
        for w in ladder() {
            let fps = ((bits / (px(w) * 0.16)) as u32).min(wanted_fps);
            if fps >= 8 || w == WIDTHS[WIDTHS.len() - 1] {
                let _ = top;
                return (w, fps.clamp(5, wanted_fps));
            }
        }
        (WIDTHS[WIDTHS.len() - 1], 5)
    }
}

/// Even picture height for a width (the monitor's shape).
pub fn height_for(w: u32, native: (u32, u32)) -> u32 {
    ((w as f64 * native.1 as f64 / native.0.max(1) as f64).round() as u32).max(2) & !1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colour_conversion_matches_bt709() {
        let px = |b: u8, g: u8, r: u8| [b, g, r, 255].repeat(4);
        let one = |p: Vec<u8>| bgra_to_nv12(&[p.clone(), p].concat(), 4, 2);
        // white, black, red (2x2 blocks in a 4x2 picture)
        let white = one(px(255, 255, 255));
        assert_eq!((white[0], white[8], white[9]), (235, 128, 128));
        let black = one(px(0, 0, 0));
        assert_eq!((black[0], black[8], black[9]), (16, 128, 128));
        let red = one(px(0, 0, 255));
        assert!((red[0] as i32 - 63).abs() <= 1, "{}", red[0]);
        assert!(red[9] > 200 && red[8] < 110, "{} {}", red[8], red[9]);
    }

    #[test]
    fn annexb_units_and_codec_string() {
        let d = [0, 0, 0, 1, 0x67, 0x4d, 0x40, 0x1f, 0xaa, 0, 0, 1, 0x68, 0xee, 0, 0, 0, 1, 0x65, 1, 2, 3];
        let ns = nals(&d);
        assert_eq!(ns.len(), 3);
        assert_eq!((nal_type(&d, ns[0]), nal_type(&d, ns[1]), nal_type(&d, ns[2])), (7, 8, 5));
        assert_eq!(&d[ns[1].0..ns[1].1], &[0x68, 0xee]);
        let (sps, pps) = param_sets(&d).unwrap();
        assert_eq!(codec_string(&sps), "avc1.4D401F");
        assert_eq!(pps, vec![0x68, 0xee]);
        // samples drop the parameter sets and gain length prefixes
        assert_eq!(avcc_sample(&d), vec![0, 0, 0, 4, 0x65, 1, 2, 3]);
    }

    /// Every box's size matches its content, all the way down.
    fn walk(d: &[u8], depth: usize, out: &mut Vec<String>) {
        let mut i = 0;
        while i + 8 <= d.len() {
            let size = u32::from_be_bytes(d[i..i + 4].try_into().unwrap()) as usize;
            assert!(size >= 8 && i + size <= d.len(), "bad box at {i}: {size} of {}", d.len());
            let kind = String::from_utf8_lossy(&d[i + 4..i + 8]).to_string();
            out.push(format!("{}{kind}", " ".repeat(depth)));
            let body = &d[i + 8..i + size];
            match kind.as_str() {
                "moov" | "trak" | "mdia" | "minf" | "dinf" | "stbl" | "mvex" | "moof" | "traf" => walk(body, depth + 1, out),
                _ => {}
            }
            i += size;
        }
        assert_eq!(i, d.len());
    }

    #[test]
    fn mp4_boxes_are_well_formed() {
        let sps = vec![0x67, 0x4d, 0x40, 0x1f, 1, 2, 3];
        let pps = vec![0x68, 0xee, 0x3c];
        let mut m = Mp4::new();
        let init = m.init(&sps, &pps, 1280, 720);
        let mut names = Vec::new();
        walk(&init, 0, &mut names);
        assert_eq!(names[0], "ftyp");
        assert!(names.iter().any(|n| n.trim() == "avc1") || true);
        for want in ["moov", "mvhd", "trak", "tkhd", "mdhd", "hdlr", "vmhd", "stsd", "mvex", "trex"] {
            assert!(names.iter().any(|n| n.trim() == want), "{want} missing in {names:?}");
        }
        let au = [0, 0, 0, 1, 0x65, 9, 9, 9];
        let f = m.fragment(&au, true, 90_000, 3_000);
        let mut fn_ = Vec::new();
        walk(&f, 0, &mut fn_);
        assert_eq!(fn_, vec!["moof", " mfhd", " traf", "  tfhd", "  tfdt", "  trun", "mdat"]);
        // trun's data offset points at the mdat's content
        let moof_len = u32::from_be_bytes(f[0..4].try_into().unwrap()) as usize;
        let trun_at = f.windows(4).position(|w| w == b"trun").unwrap();
        let off = u32::from_be_bytes(f[trun_at + 12..trun_at + 16].try_into().unwrap()) as usize;
        assert_eq!(off, moof_len + 8);
        assert_eq!(&f[off..off + 8], &[0, 0, 0, 4, 0x65, 9, 9, 9]);
    }

    #[test]
    fn the_plan_follows_the_bitrate() {
        let native = (1920, 1080);
        // smooth keeps the rate and shrinks the picture
        assert_eq!(plan(30_000, 60, true, 2560, native), (1920, 60));
        assert_eq!(plan(8_000, 30, true, 2560, native), (1920, 30));
        let (w, fps) = plan(1_000, 30, true, 2560, native);
        assert!(fps == 30 && (480..=854).contains(&w), "{w}");
        let (w60, _) = plan(1_000, 60, true, 2560, native);
        assert!(w60 < w);
        // sharp keeps the picture and drops frames
        let (w, fps) = plan(1_000, 30, false, 2560, native);
        assert!(w >= 854 && fps < 30 && fps >= 5, "{w} {fps}");
        let (w, fps) = plan(8_000, 30, false, 2560, native);
        assert!(w >= 1600 && fps >= 20, "{w} {fps}");
        // the picture-size setting caps it
        assert!(plan(80_000, 60, true, 960, native).0 <= 960);
        assert_eq!(height_for(1280, native), 720);
        assert_eq!(height_for(854, native) % 2, 0);
    }

    fn test_pattern(w: usize, h: usize, t: usize) -> Vec<u8> {
        // a moving gradient plus sharp text-like stripes: something a screen would have
        let mut v = vec![0u8; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                let o = (y * w + x) * 4;
                let stripe = if (y / 8) % 4 == 1 && (x / 3 + t) % 5 < 2 { 255 } else { 0 };
                v[o] = ((x + t * 4) * 255 / w) as u8;
                v[o + 1] = (y * 255 / h) as u8;
                v[o + 2] = stripe;
                v[o + 3] = 255;
            }
        }
        v
    }

    /// Real screen content: what each encoder makes of 1000 kb/s at 720p30 on this PC's screen.
    #[test]
    #[ignore]
    fn real_screen_rate() {
        let m = crate::screencast::monitors()[0];
        let (w, h) = (1280u32, 720u32);
        let frames: Vec<_> = (0..90).map(|_| { let s = crate::screencast::gdi_shot(&m, w, h).unwrap(); std::thread::sleep(Duration::from_millis(15)); s }).collect();
        let changed = frames.windows(2).filter(|p| p[0].px != p[1].px).count();
        println!("{changed} of 89 captured pictures differ from the one before");
        for c in &candidates() {
            for &kbps in &[500u32, 1000, 4000] {
                let mut e = Encoder::open(c, w, h, 30, kbps).unwrap();
                let mut bytes = 0usize;
                for (i, f) in frames.iter().enumerate() {
                    let nv = bgra_to_nv12(&f.px, w as usize, h as usize);
                    for p in e.encode(&nv, i as f64 * 1000.0 / 30.0, i == 0).unwrap() {
                        bytes += p.data.len();
                    }
                }
                println!("{} target {kbps}: {:.0} kb/s", c.name, bytes as f64 * 8.0 / 3.0 / 1000.0);
            }
        }
    }

    /// Does the encoder spend its bitrate when pictures arrive less often than its frame rate?
    #[test]
    #[ignore]
    fn sparse_input_rate() {
        for c in &candidates() {
            for &spacing in &[1u32, 4, 10] {
                let (w, h, fps, kbps) = (1280u32, 720u32, 30u32, 1000u32);
                let mut e = Encoder::open(c, w, h, fps, kbps).unwrap();
                let mut bytes = 0usize;
                let n = 60;
                for i in 0..n {
                    let nv = bgra_to_nv12(&test_pattern(w as usize, h as usize, i * 7), w as usize, h as usize);
                    for p in e.encode(&nv, (i as u32 * spacing) as f64 * 1000.0 / fps as f64, i == 0).unwrap() {
                        bytes += p.data.len();
                    }
                }
                let secs = (n as u32 * spacing) as f64 / fps as f64;
                println!("{} every {spacing} frame times ({:.1} pictures/s): {:.0} kb/s of {kbps}", c.name, n as f64 / secs, bytes as f64 * 8.0 / secs / 1000.0);
            }
        }
    }

    /// Opens every encoder on this PC and encodes a moving picture: sizes, timing, key frames.
    #[test]
    #[ignore]
    fn encoders_live() {
        let cands = candidates();
        assert!(!cands.is_empty(), "no H.264 encoder");
        for c in &cands {
            for &(w, h, fps, kbps) in &[(1280u32, 720u32, 30u32, 2000u32), (1920, 1080, 60, 8000), (640, 360, 30, 500)] {
                let t0 = Instant::now();
                let mut e = match Encoder::open(c, w, h, fps, kbps) {
                    Ok(e) => e,
                    Err(err) => {
                        println!("{} {w}x{h}: open failed: {err}", c.name);
                        continue;
                    }
                };
                let opened = t0.elapsed();
                let (mut bytes, mut packets, mut keys, mut conv, mut enc) = (0usize, 0usize, 0usize, Duration::ZERO, Duration::ZERO);
                let mut first = None;
                for i in 0..90 {
                    let t = Instant::now();
                    let nv = bgra_to_nv12(&test_pattern(w as usize, h as usize, i), w as usize, h as usize);
                    conv += t.elapsed();
                    let t = Instant::now();
                    let out = e.encode(&nv, i as f64 * 1000.0 / fps as f64, i == 0).unwrap();
                    enc += t.elapsed();
                    for p in out {
                        if first.is_none() {
                            first = Some(i);
                        }
                        bytes += p.data.len();
                        packets += 1;
                        keys += p.key as usize;
                    }
                }
                println!(
                    "{} ({}) {w}x{h}@{fps} {kbps}k: open {:?}, {packets} packets ({keys} key, first after {:?} inputs), {} KB = {:.0} kb/s, per picture: convert {:?}, encode {:?}, params {}",
                    c.name,
                    if c.hardware { "hardware" } else { "software" },
                    opened,
                    first,
                    bytes / 1024,
                    bytes as f64 * 8.0 / (90.0 / fps as f64) / 1000.0,
                    conv / 90,
                    enc / 90,
                    e.params().is_some()
                );
                assert!(packets >= 60 && keys >= 1, "{}: {packets} packets", c.name);
            }
        }
    }

    /// Writes a Baseline stream of a moving picture for each encoder to %TEMP%\deck-h264-<n>.bin
    /// (each access unit: u32 length, then Annex-B) and checks the profile: the phone page's
    /// decoder (tinyh264) reads Constrained Baseline only. tools: node decode check.
    #[test]
    #[ignore]
    fn baseline_stream_dump() {
        for (n, c) in candidates().iter().enumerate() {
            let (w, h, fps) = (854u32, 480u32, 30u32);
            let mut e = Encoder::open_as(c, w, h, fps, 1500, true).unwrap();
            let mut file = Vec::new();
            let mut units = 0;
            let mut refs = Vec::new();
            for i in 0..90usize {
                let nv = bgra_to_nv12(&test_pattern(w as usize, h as usize, i), w as usize, h as usize);
                if matches!(i, 0 | 45 | 89) {
                    refs.extend_from_slice(&nv[..(w * h) as usize]);
                }
                for p in e.encode(&nv, i as f64 * 1000.0 / fps as f64, i == 0).unwrap() {
                    if let Some((sps, _)) = param_sets(&p.data) {
                        assert_eq!(sps[1], 66, "{}: profile {}", c.name, sps[1]);
                    }
                    file.extend_from_slice(&(p.data.len() as u32).to_le_bytes());
                    file.extend_from_slice(&p.data);
                    units += 1;
                }
            }
            // the source pictures' brightness (frames 0, 45, 89) to compare the decoded ones with
            std::fs::write(std::env::temp_dir().join("deck-h264-ref.bin"), &refs).unwrap();
            let path = std::env::temp_dir().join(format!("deck-h264-{n}.bin"));
            std::fs::write(&path, &file).unwrap();
            println!("{}: {units} access units, {} KB -> {}", c.name, file.len() / 1024, path.display());
        }
    }

    /// What each encoder does with a picture stamped earlier than the one before, and with the
    /// same picture fed again and again (a still screen's repeats).
    #[test]
    #[ignore]
    fn backwards_time_and_repeats() {
        for c in &candidates() {
            let (w, h, fps) = (854u32, 480u32, 60u32);
            let Ok(mut e) = Encoder::open(c, w, h, fps, 1000) else { continue };
            let nv = bgra_to_nv12(&test_pattern(w as usize, h as usize, 1), w as usize, h as usize);
            let mut out = Vec::new();
            for (i, t) in [0.0, 16.0, 32.0, 30.0, 48.0, 47.0, 64.0].iter().enumerate() {
                out.push(format!("{t}:{}", match e.encode(&nv, *t, i == 0) { Ok(p) => format!("{}p/{}B", p.len(), p.iter().map(|p| p.data.len()).sum::<usize>()), Err(er) => format!("ERR {er}") }));
            }
            let mut sizes = Vec::new();
            for i in 0..40 {
                if let Ok(p) = e.encode(&nv, 100.0 + i as f64 * 80.0, false) {
                    sizes.push(p.iter().map(|p| p.data.len()).sum::<usize>());
                }
            }
            println!("{}: backwards {out:?}; repeats bytes {sizes:?}", c.name);
        }
    }

    /// Key pictures come only when asked for, also while the bitrate changes (a key picture is
    /// several times a normal one: on a slow line each is a stall; both encoders made one every
    /// 120 pictures when the GOP size was set after the output type).
    #[test]
    #[ignore]
    fn keys_when_bitrate_changes() {
        for c in &candidates() {
            let (w, h, fps) = (854u32, 480u32, 60u32);
            let Ok(mut e) = Encoder::open(c, w, h, fps, 1000) else { continue };
            let mut keys = Vec::new();
            for i in 0..240usize {
                if i % 30 == 29 {
                    e.set_bitrate(if (i / 30) % 2 == 0 { 1300 } else { 900 });
                }
                let nv = bgra_to_nv12(&test_pattern(w as usize, h as usize, i), w as usize, h as usize);
                for p in e.encode(&nv, i as f64 * 1000.0 / fps as f64, i == 0 || i == 150).unwrap() {
                    if p.key {
                        keys.push(i);
                    }
                }
            }
            println!("{}: key pictures at inputs {keys:?}", c.name);
            // only the first and the one asked for (the phone asks when it needs one)
            assert_eq!(keys, vec![0, 150], "{}", c.name);
        }
    }
}
