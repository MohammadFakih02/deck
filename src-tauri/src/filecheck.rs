//! Is a running program safe? (System → Processes.) Four checks, each optional and each run for
//! every program at once:
//! - signatures: who signed the file (Windows' own check, catalogue-signed system files included)
//! - AV engines: Team Cymru's Malware Hash Registry, asked by DNS with the file's SHA-1: the share
//!   of antivirus engines that flag it, or nothing when it isn't known malware. No key, no upload.
//! - Defender: Microsoft Defender scans the file (MpCmdRun, no changes made: -DisableRemediation)
//! - VirusTotal: with the user's own API key, a lookup by SHA-256 (the free key allows 4 a minute)
//!
//! Only fingerprints leave the PC, never a file. Results are kept per fingerprint
//! (`%APPDATA%\Deck\filecheck.json`), so a file is checked again only when it changes.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
pub struct Rep {
    /// "valid" | "unsigned" | "bad" (signed, but the signature doesn't hold: changed file,
    /// untrusted or revoked certificate)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sig: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signer: Option<String>,
    /// Windows' words for a bad signature
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sig_note: Option<String>,
    /// Team Cymru: Some(0) = not known malware, Some(n) = n % of engines flag it
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engines: Option<u32>,
    /// Defender: "clean" | "threat" | "error"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub defender: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub defender_note: Option<String>,
    /// VirusTotal: [malicious, suspicious, engines]; [0, 0, 0] = VirusTotal doesn't know the file
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vt: Option<[u32; 3]>,
    #[serde(default)]
    pub sha256: String,
}

#[derive(Serialize, Deserialize, Clone)]
struct FileId {
    len: u64,
    modified: u64,
    sha256: String,
    sha1: String,
}

#[derive(Serialize, Deserialize, Default)]
struct Store {
    #[serde(default)]
    files: HashMap<String, FileId>,
    #[serde(default)]
    reps: HashMap<String, Rep>,
    /// the VirusTotal key, sealed for this Windows account
    #[serde(default, with = "crate::secret::field")]
    vt_key: String,
}

/// The check running now (one at a time) and how far it got.
#[derive(Serialize, Clone, Default)]
pub struct Job {
    pub what: String,
    pub done: usize,
    pub total: usize,
    pub current: String,
    /// what happened, in words, when it ended (or why it stopped)
    pub result: String,
    pub running: bool,
}

static STORE: parking_lot::Mutex<Option<Store>> = parking_lot::const_mutex(None);
static JOB: parking_lot::Mutex<Option<Job>> = parking_lot::const_mutex(None);
static STOP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn file() -> PathBuf {
    crate::util::appdata().join("Deck").join("filecheck.json")
}

fn with_store<T>(f: impl FnOnce(&mut Store) -> T) -> T {
    let mut g = STORE.lock();
    let s = g.get_or_insert_with(|| std::fs::read(file()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default());
    f(s)
}

fn save() {
    let bytes = with_store(|s| serde_json::to_vec(s).unwrap_or_default());
    let p = file();
    let _ = std::fs::create_dir_all(p.parent().unwrap());
    let tmp = p.with_extension("tmp");
    if std::fs::write(&tmp, bytes).is_ok() {
        let _ = std::fs::rename(&tmp, &p);
    }
}

/// The file's fingerprints, worked out once per size and change time.
fn id_of(path: &str) -> Option<FileId> {
    use sha1::Digest as _;
    let meta = std::fs::metadata(path).ok()?;
    let modified = meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
    if let Some(f) = with_store(|s| s.files.get(&path.to_lowercase()).cloned()) {
        if f.len == meta.len() && f.modified == modified {
            return Some(f);
        }
    }
    let mut r = std::io::BufReader::with_capacity(1 << 20, std::fs::File::open(path).ok()?);
    let (mut a, mut b) = (sha2::Sha256::new(), sha1::Sha1::new());
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = std::io::Read::read(&mut r, &mut buf).ok()?;
        if n == 0 {
            break;
        }
        sha2::Digest::update(&mut a, &buf[..n]);
        b.update(&buf[..n]);
    }
    let hex = |d: &[u8]| d.iter().map(|x| format!("{x:02x}")).collect::<String>();
    let id = FileId { len: meta.len(), modified, sha256: hex(&sha2::Digest::finalize(a)), sha1: hex(&b.finalize()) };
    with_store(|s| s.files.insert(path.to_lowercase(), id.clone()));
    Some(id)
}

fn rep_mut<T>(sha256: &str, f: impl FnOnce(&mut Rep) -> T) -> T {
    with_store(|s| {
        let r = s.reps.entry(sha256.to_string()).or_default();
        r.sha256 = sha256.to_string();
        f(r)
    })
}

/// What's known about these programs (no checking), by path, plus the job and whether a
/// VirusTotal key is saved.
pub fn state(paths: &[String]) -> Value {
    let reps: serde_json::Map<String, Value> = with_store(|s| {
        paths
            .iter()
            .filter_map(|p| {
                let f = s.files.get(&p.to_lowercase())?;
                // a file changed since: its old results don't count
                let modified = std::fs::metadata(p).ok()?.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_secs();
                (modified == f.modified).then(|| s.reps.get(&f.sha256).map(|r| (p.clone(), json!(r))))?
            })
            .collect()
    });
    json!({ "reps": reps, "job": JOB.lock().clone(), "vt_key": with_store(|s| !s.vt_key.is_empty()) })
}

pub fn set_vt_key(key: &str) -> Result<(), String> {
    let k = key.trim();
    if !k.is_empty() && (k.len() != 64 || !k.chars().all(|c| c.is_ascii_hexdigit())) {
        return Err("That isn't a VirusTotal API key: it's 64 letters and digits (VirusTotal → your profile → API key).".into());
    }
    with_store(|s| s.vt_key = k.to_string());
    save();
    Ok(())
}

pub fn stop() {
    STOP.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// Starts one check ("sig" | "engines" | "defender" | "vt") for these programs in the background.
pub fn start(app: std::sync::Arc<crate::app::App>, what: &str, paths: Vec<String>) -> Result<(), String> {
    if !["sig", "engines", "defender", "vt"].contains(&what) {
        return Err(format!("unknown check {what}"));
    }
    if what == "vt" && with_store(|s| s.vt_key.is_empty()) {
        return Err("Add your VirusTotal API key first (the key button next to VirusTotal).".into());
    }
    {
        let mut j = JOB.lock();
        if j.as_ref().is_some_and(|j| j.running) {
            return Err("Another check is still running: wait for it or stop it.".into());
        }
        let mut seen = std::collections::HashSet::new();
        let paths: Vec<String> = paths.into_iter().filter(|p| !p.is_empty() && seen.insert(p.to_lowercase())).collect();
        *j = Some(Job { what: what.into(), total: paths.len(), running: true, ..Default::default() });
        STOP.store(false, std::sync::atomic::Ordering::Relaxed);
        let what = what.to_string();
        std::thread::Builder::new()
            .name("filecheck".into())
            .spawn(move || {
                let result = run(&app, &what, &paths);
                save();
                if let Some(j) = JOB.lock().as_mut() {
                    j.running = false;
                    j.current.clear();
                    j.result = result;
                }
                crate::log::info(format!("filecheck: {what} done"));
                app.ui_changed("filecheck");
            })
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn progress(app: &crate::app::App, done: usize, current: &str) {
    if let Some(j) = JOB.lock().as_mut() {
        j.done = done;
        j.current = current.to_string();
    }
    app.ui_changed("filecheck");
}

fn stopped() -> bool {
    STOP.load(std::sync::atomic::Ordering::Relaxed)
}

fn name(p: &str) -> String {
    p.rsplit(['\\', '/']).next().unwrap_or(p).to_string()
}

/// Runs one check over every path and says what it found.
fn run(app: &crate::app::App, what: &str, paths: &[String]) -> String {
    let mut unread = 0;
    let mut ids = Vec::new();
    for (i, p) in paths.iter().enumerate() {
        if stopped() {
            return "Stopped.".into();
        }
        progress(app, i, &format!("Fingerprinting {}", name(p)));
        match id_of(p) {
            Some(id) => ids.push((p.clone(), id)),
            None => unread += 1,
        }
    }
    let unread_note = if unread > 0 { format!(" {unread} couldn't be read (Windows protects them, or they're gone).") } else { String::new() };
    let out = match what {
        "sig" => signatures(app, &ids),
        "engines" => engines(app, &ids),
        "defender" => defender(app, &ids),
        _ => virustotal(app, &ids),
    };
    format!("{out}{unread_note}")
}

// ------------------------------------------------------------------------------------- signatures

fn signatures(app: &crate::app::App, ids: &[(String, FileId)]) -> String {
    progress(app, 0, "Asking Windows who signed each program");
    // one PowerShell for all of them; the paths go in a file, never into the script
    let list = std::env::temp_dir().join(format!("deck-sig-{}.txt", std::process::id()));
    if std::fs::write(&list, ids.iter().map(|(p, _)| p.as_str()).collect::<Vec<_>>().join("\n")).is_err() {
        return "Couldn't write the list of programs to check.".into();
    }
    let script = format!(
        "[Console]::OutputEncoding=[Text.Encoding]::UTF8; Get-Content -LiteralPath '{}' -Encoding UTF8 | ForEach-Object {{ $s = Get-AuthenticodeSignature -LiteralPath $_; [pscustomobject]@{{ p = [string]$_; s = [string]$s.Status; m = [string]$s.StatusMessage; n = if ($s.SignerCertificate) {{ $s.SignerCertificate.GetNameInfo('SimpleName', $false) }} else {{ '' }} }} }} | ConvertTo-Json -Compress",
        list.display().to_string().replace('\'', "''")
    );
    let o = crate::util::run_for(600, "powershell.exe", &["-NoProfile", "-NonInteractive", "-Command", &script]);
    let _ = std::fs::remove_file(&list);
    let Ok(o) = o else { return "Windows' signature check didn't run.".into() };
    let v: Value = serde_json::from_slice(&o.stdout).unwrap_or(Value::Null);
    let rows = match v {
        Value::Array(a) => a,
        Value::Object(_) => vec![v],
        _ => return format!("Windows' signature check gave no answer: {}", String::from_utf8_lossy(&o.stderr).lines().next().unwrap_or("").trim()),
    };
    let (mut ok, mut none, mut bad) = (0, 0, 0);
    for r in rows {
        let p = r["p"].as_str().unwrap_or("");
        let Some((_, id)) = ids.iter().find(|(x, _)| x.eq_ignore_ascii_case(p)) else { continue };
        let status = r["s"].as_str().unwrap_or("");
        let signer = r["n"].as_str().filter(|s| !s.is_empty()).map(str::to_string);
        let (sig, note) = match status {
            "Valid" => {
                ok += 1;
                ("valid", None)
            }
            "NotSigned" => {
                none += 1;
                ("unsigned", None)
            }
            _ => {
                bad += 1;
                ("bad", r["m"].as_str().map(str::to_string))
            }
        };
        rep_mut(&id.sha256, |r| {
            r.sig = Some(sig.into());
            r.signer = signer;
            r.sig_note = note;
        });
    }
    format!("Signatures: {ok} signed, {none} unsigned{}.", if bad > 0 { format!(", {bad} with a signature that doesn't hold (look at those first)") } else { String::new() })
}

// ------------------------------------------------------------------------------------- AV engines

/// Team Cymru's answer for a SHA-1: Ok(Some(percent)) known malware, Ok(None) not known.
fn cymru(sha1: &str) -> Result<Option<u32>, String> {
    use windows_sys::Win32::NetworkManagement::Dns::{DnsFree, DnsFreeRecordList, DnsQuery_W, DNS_QUERY_STANDARD, DNS_RECORDA, DNS_RECORDW, DNS_TYPE_TEXT};
    let q: Vec<u16> = format!("{sha1}.malware.hash.cymru.com").encode_utf16().chain([0]).collect();
    // the binding names the A record type; DnsQuery_W fills in wide (W) records
    let mut raw: *mut DNS_RECORDA = std::ptr::null_mut();
    let rc = unsafe { DnsQuery_W(q.as_ptr(), DNS_TYPE_TEXT, DNS_QUERY_STANDARD, std::ptr::null_mut(), &mut raw, std::ptr::null_mut()) };
    let rec = raw as *mut DNS_RECORDW;
    // DNS_ERROR_RCODE_NAME_ERROR: no such name = not known malware
    if rc == 9003 || rc == 9501 {
        return Ok(None);
    }
    if rc != 0 || rec.is_null() {
        return Err(format!("the lookup failed (DNS error {rc})"));
    }
    let mut text = String::new();
    unsafe {
        let mut r = rec;
        while !r.is_null() {
            if (*r).wType == DNS_TYPE_TEXT {
                let t = &(*r).Data.TXT;
                let arr = t.pStringArray.as_ptr();
                for i in 0..t.dwStringCount as usize {
                    let s = *arr.add(i);
                    if !s.is_null() {
                        let len = (0..).take_while(|&k| *s.add(k) != 0).count();
                        text.push_str(&String::from_utf16_lossy(std::slice::from_raw_parts(s, len)));
                        text.push(' ');
                    }
                }
            }
            r = (*r).pNext;
        }
        DnsFree(rec as _, DnsFreeRecordList);
    }
    // "<last seen, unix time> <percent of engines>"
    Ok(text.split_whitespace().nth(1).and_then(|p| p.parse().ok()).or(Some(0)).filter(|_| !text.trim().is_empty()))
}

fn engines(app: &crate::app::App, ids: &[(String, FileId)]) -> String {
    let (mut flagged, mut failed) = (Vec::new(), 0);
    for (i, (p, id)) in ids.iter().enumerate() {
        if stopped() {
            return "Stopped.".into();
        }
        progress(app, i, &format!("Looking up {}", name(p)));
        match cymru(&id.sha1) {
            Ok(v) => {
                let pct = v.unwrap_or(0);
                if pct > 0 {
                    flagged.push(format!("{} ({pct}%)", name(p)));
                }
                rep_mut(&id.sha256, |r| r.engines = Some(pct));
            }
            Err(_) => failed += 1,
        }
    }
    let mut s = if flagged.is_empty() { format!("AV engines: none of the {} programs is known malware.", ids.len() - failed) } else { format!("AV engines: {} known as malware: {}.", flagged.len(), flagged.join(", ")) };
    if failed > 0 {
        s.push_str(&format!(" {failed} lookups failed (no internet, or DNS blocks hash.cymru.com)."));
    }
    s
}

// ------------------------------------------------------------------------------------- Defender

fn mpcmdrun() -> Option<PathBuf> {
    // the newest platform folder first (Defender updates itself there), then the old place
    let plat = PathBuf::from(std::env::var("ProgramData").unwrap_or_else(|_| r"C:\ProgramData".into())).join(r"Microsoft\Windows Defender\Platform");
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(&plat).map(|r| r.flatten().map(|e| e.path()).collect()).unwrap_or_default();
    dirs.sort();
    dirs.into_iter().rev().map(|d| d.join("MpCmdRun.exe")).chain([crate::util::program_files().join(r"Windows Defender\MpCmdRun.exe")]).find(|p| p.is_file())
}

fn defender(app: &crate::app::App, ids: &[(String, FileId)]) -> String {
    let Some(mp) = mpcmdrun() else { return "Microsoft Defender isn't on this PC (another antivirus may have replaced it).".into() };
    let (mut threats, mut errors) = (Vec::new(), 0);
    for (i, (p, id)) in ids.iter().enumerate() {
        if stopped() {
            return "Stopped.".into();
        }
        progress(app, i, &format!("Defender is scanning {}", name(p)));
        let o = crate::util::run_for(120, &mp.to_string_lossy(), &["-Scan", "-ScanType", "3", "-File", p, "-DisableRemediation"]);
        let (verdict, note) = match o {
            Ok(o) => {
                let out = String::from_utf8_lossy(&o.stdout).to_string();
                match o.status.code() {
                    Some(0) => ("clean", None),
                    Some(2) => {
                        let t = out.lines().find_map(|l| l.trim().strip_prefix("Threat").map(|r| r.trim_start_matches([' ', ':']).trim().to_string())).filter(|t| !t.is_empty());
                        threats.push(format!("{} ({})", name(p), t.as_deref().unwrap_or("threat")));
                        ("threat", t)
                    }
                    c => {
                        errors += 1;
                        ("error", Some(out.lines().map(str::trim).rfind(|l| !l.is_empty()).map_or_else(|| format!("Defender stopped with code {c:?}"), str::to_string)))
                    }
                }
            }
            Err(e) => {
                errors += 1;
                ("error", Some(e.to_string()))
            }
        };
        rep_mut(&id.sha256, |r| {
            r.defender = Some(verdict.into());
            r.defender_note = note;
        });
    }
    let mut s = if threats.is_empty() { format!("Defender: no threats in {} programs.", ids.len() - errors) } else { format!("Defender found threats in {}: {}. Nothing was removed; Defender's own app can quarantine them.", threats.len(), threats.join(", ")) };
    if errors > 0 {
        s.push_str(&format!(" {errors} couldn't be scanned (Defender off or replaced by another antivirus?)."));
    }
    s
}

// ------------------------------------------------------------------------------------- VirusTotal

/// VirusTotal's answer for one file.
enum Vt {
    /// [malicious, suspicious, engines]
    Found([u32; 3]),
    Unknown,
}

enum VtErr {
    Key,
    /// too many requests: the minute's or the day's
    Limit,
    /// the v2 API isn't there (any more): use v3
    NoV2,
    Other(String),
}

fn vt_status(e: ureq::Error) -> VtErr {
    match e {
        ureq::Error::Status(401 | 403, _) => VtErr::Key,
        ureq::Error::Status(204 | 429, _) => VtErr::Limit,
        ureq::Error::Status(c, _) => VtErr::Other(format!("VirusTotal answered with error {c}")),
        // no answer: said in words, without the request (it holds the fingerprint, and once the key)
        ureq::Error::Transport(t) => VtErr::Other(match t.kind() {
            ureq::ErrorKind::Dns => "no internet, or the name virustotal.com can't be looked up".into(),
            ureq::ErrorKind::ConnectionFailed | ureq::ErrorKind::Io => "the connection to VirusTotal timed out or broke".into(),
            _ => "VirusTotal couldn't be reached".into(),
        }),
    }
}

/// Up to 4 files in one request (the v2 API takes 4 fingerprints per request on a free key).
fn vt_v2(agent: &ureq::Agent, key: &str, hashes: &[&str]) -> Result<Vec<(String, Vt)>, VtErr> {
    let r = agent.get("https://www.virustotal.com/vtapi/v2/file/report").query("apikey", key).query("resource", &hashes.join(",")).call();
    let r = match r {
        Ok(r) if r.status() == 204 => return Err(VtErr::Limit),
        Ok(r) => r,
        Err(ureq::Error::Status(404 | 410 | 400, _)) => return Err(VtErr::NoV2),
        Err(e) => return Err(vt_status(e)),
    };
    let v: Value = r.into_json().map_err(|_| VtErr::NoV2)?;
    let rows = match v {
        Value::Array(a) => a,
        o @ Value::Object(_) => vec![o],
        _ => return Err(VtErr::NoV2),
    };
    Ok(rows
        .iter()
        .filter_map(|x| {
            let res = x["resource"].as_str()?.to_lowercase();
            let a = if x["response_code"].as_i64() == Some(1) { Vt::Found([x["positives"].as_u64().unwrap_or(0) as u32, 0, x["total"].as_u64().unwrap_or(0) as u32]) } else { Vt::Unknown };
            Some((res, a))
        })
        .collect())
}

fn vt_v3(agent: &ureq::Agent, key: &str, sha256: &str) -> Result<Vt, VtErr> {
    match agent.get(&format!("https://www.virustotal.com/api/v3/files/{sha256}")).set("x-apikey", key).call() {
        Ok(r) => {
            let v: Value = r.into_json().unwrap_or(Value::Null);
            let st = &v["data"]["attributes"]["last_analysis_stats"];
            let n = |k: &str| st[k].as_u64().unwrap_or(0) as u32;
            let total = ["malicious", "suspicious", "undetected", "harmless", "timeout", "failure", "type-unsupported"].iter().map(|k| n(k)).sum();
            Ok(Vt::Found([n("malicious"), n("suspicious"), total]))
        }
        Err(ureq::Error::Status(404, _)) => Ok(Vt::Unknown),
        Err(e) => Err(vt_status(e)),
    }
}

fn virustotal(app: &crate::app::App, ids: &[(String, FileId)]) -> String {
    let key = with_store(|s| s.vt_key.clone());
    let agent = crate::log::http(ureq::AgentBuilder::new()).timeout(Duration::from_secs(30)).build();
    // already looked up: not again (the free key allows 500 a day). The likeliest trouble first:
    // a signature that doesn't hold, then unsigned, then not checked, then signed.
    let mut todo: Vec<&(String, FileId)> = ids.iter().filter(|(_, id)| with_store(|s| s.reps.get(&id.sha256).is_none_or(|r| r.vt.is_none()))).collect();
    let rank = |id: &FileId| with_store(|s| match s.reps.get(&id.sha256).and_then(|r| r.sig.as_deref()) {
        Some("bad") => 0,
        Some("unsigned") => 1,
        None => 2,
        _ => 3,
    });
    todo.sort_by_key(|(_, id)| rank(id));
    let (mut flagged, mut unknown, mut looked) = (Vec::new(), 0, 0);
    let mut v2 = true;
    let mut last = SystemTime::UNIX_EPOCH;
    let mut i = 0;
    let mut limit_waits = 0;
    let mut net_tries = 0;
    while i < todo.len() {
        let batch: Vec<&(String, FileId)> = todo[i..].iter().take(if v2 { 4 } else { 1 }).copied().collect();
        let names = batch.iter().map(|(p, _)| name(p)).collect::<Vec<_>>().join(", ");
        // one request every 15 s (4 a minute)
        loop {
            if stopped() {
                return format!("Stopped after {looked} lookups.");
            }
            let wait = Duration::from_secs(15).saturating_sub(last.elapsed().unwrap_or(Duration::MAX));
            if wait.is_zero() {
                break;
            }
            progress(app, i, &format!("VirusTotal next: {names} (free key: 4 requests a minute; {} s)", wait.as_secs() + 1));
            std::thread::sleep(wait.min(Duration::from_secs(1)));
        }
        progress(app, i, &format!("VirusTotal: looking up {names}"));
        last = SystemTime::now();
        let answers: Result<Vec<(String, Vt)>, VtErr> = if v2 {
            vt_v2(&agent, &key, &batch.iter().map(|(_, id)| id.sha256.as_str()).collect::<Vec<_>>())
        } else {
            let id = &batch[0].1;
            vt_v3(&agent, &key, &id.sha256).map(|a| vec![(id.sha256.clone(), a)])
        };
        match answers {
            Ok(list) => {
                limit_waits = 0;
                net_tries = 0;
                for (p, id) in &batch {
                    let a = list.iter().find(|(h, _)| h.eq_ignore_ascii_case(&id.sha256)).map(|(_, a)| a);
                    let v = match a {
                        Some(Vt::Found(v)) => *v,
                        _ => [0, 0, 0],
                    };
                    if v[2] == 0 {
                        unknown += 1;
                    } else if v[0] > 0 {
                        flagged.push(format!("{} ({}/{})", name(p), v[0], v[2]));
                    }
                    rep_mut(&id.sha256, |r| r.vt = Some(v));
                    looked += 1;
                }
                i += batch.len();
                save();
            }
            Err(VtErr::NoV2) => {
                crate::log::info("filecheck: VirusTotal's v2 API didn't answer; one file per request (v3) from here");
                v2 = false;
            }
            Err(VtErr::Key) => return "VirusTotal refused the key: check it on virustotal.com (your profile → API key).".into(),
            // the minute's limit: wait a minute once; still refused, it's the day's
            Err(VtErr::Limit) if limit_waits == 0 => {
                limit_waits += 1;
                for s in (1..=60).rev() {
                    if stopped() {
                        return format!("Stopped after {looked} lookups.");
                    }
                    progress(app, i, &format!("VirusTotal asked Deck to slow down: trying again in {s} s"));
                    std::thread::sleep(Duration::from_secs(1));
                }
            }
            Err(VtErr::Limit) => return format!("VirusTotal's limit for this key is used up for today ({looked} looked up now; the free key allows 500 a day). The rest can be looked up tomorrow: results so far are kept."),
            // a dropped connection: try again a few times before giving up
            Err(VtErr::Other(e)) if net_tries < 3 => {
                net_tries += 1;
                for s in (1..=20).rev() {
                    if stopped() {
                        return format!("Stopped after {looked} lookups.");
                    }
                    progress(app, i, &format!("{e}: trying again in {s} s (try {net_tries} of 3)"));
                    std::thread::sleep(Duration::from_secs(1));
                }
            }
            Err(VtErr::Other(e)) => return format!("VirusTotal stopped: {e}, 4 times in a row, after {looked} lookups. Results so far are kept; press VirusTotal again to carry on with the rest."),
        }
    }
    let skipped = ids.len() - todo.len();
    let mut s = if flagged.is_empty() { format!("VirusTotal: {looked} looked up, none flagged as malicious.") } else { format!("VirusTotal flags {}: {}.", flagged.len(), flagged.join(", ")) };
    if unknown > 0 {
        s.push_str(&format!(" {unknown} unknown to VirusTotal (never uploaded by anyone: usually your own or rare programs)."));
    }
    if skipped > 0 {
        s.push_str(&format!(" {skipped} were looked up before."));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "asks Team Cymru over the internet"]
    fn cymru_knows_the_eicar_test_file_and_not_notepad() {
        assert!(cymru("3395856ce81f2b7382dee72602f798b642f14140").unwrap().is_some_and(|p| p > 50));
        assert_eq!(cymru("0000000000000000000000000000000000000000").unwrap(), None);
    }

    #[test]
    fn a_vt_key_is_checked_before_it_is_kept() {
        assert!(set_vt_key("not a key").is_err());
    }
}
