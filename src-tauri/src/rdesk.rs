//! Remote desktop with RustDesk (free, open source): see this PC's screen and control it from a
//! phone or another computer. Deck installs it, turns on direct connections (straight to this PC,
//! at home or over Tailscale, without RustDesk's public servers), sets its password, hides its
//! tray icon and shows the addresses to connect to. Tailscale (free) is the private network that
//! makes "away from home" work without opening anything to the internet.

use crate::util::run_hidden;
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::io::Read;
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// RustDesk's direct-connection port (Settings → Security → "Enable direct IP access").
const DIRECT_PORT: u16 = 21118;

fn program_files(rel: &str) -> Option<PathBuf> {
    let p = PathBuf::from(std::env::var("ProgramFiles").unwrap_or_else(|_| r"C:\Program Files".into())).join(rel);
    p.is_file().then_some(p)
}

fn rustdesk_exe() -> Option<PathBuf> {
    program_files(r"RustDesk\rustdesk.exe")
}

pub(crate) fn tailscale_exe() -> Option<PathBuf> {
    program_files(r"Tailscale\tailscale.exe")
}

/// Runs a PowerShell script with admin rights: directly when Deck has them, otherwise through one
/// UAC prompt. The script goes base64-encoded, so passwords and paths need no quoting. Returns what
/// the script printed (only when Deck is already admin; an elevated child's output can't be read).
fn admin_ps(script: &str) -> Result<String, String> {
    let enc = |s: &str| crate::util::base64(&s.encode_utf16().flat_map(u16::to_le_bytes).collect::<Vec<u8>>());
    let script = format!("$ErrorActionPreference = 'Stop'\ntry {{\n{script}\n}} catch {{ $_.Exception.Message; exit 1 }}");
    let out = if crate::elevation::is_admin() {
        run_hidden("powershell.exe", &["-NoProfile", "-NonInteractive", "-EncodedCommand", &enc(&script)])
    } else {
        let outer = format!(
            "try {{ $p = Start-Process powershell.exe -Verb RunAs -WindowStyle Hidden -Wait -PassThru -ArgumentList '-NoProfile','-NonInteractive','-EncodedCommand','{}'; exit $p.ExitCode }} catch {{ 'Admin rights were not given.'; exit 1 }}",
            enc(&script)
        );
        run_hidden("powershell.exe", &["-NoProfile", "-NonInteractive", "-EncodedCommand", &enc(&outer)])
    }
    .map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if out.status.success() {
        Ok(text)
    } else {
        Err(if text.is_empty() { "It didn't finish.".into() } else { text })
    }
}

/// A PowerShell single-quoted string.
fn ps_str(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

fn service_running() -> bool {
    run_hidden("sc.exe", &["query", "RustDesk"]).map(|o| String::from_utf8_lossy(&o.stdout).contains("RUNNING")).unwrap_or(false)
}

/// RustDesk answers on its direct port (direct connections are on and the service runs).
fn direct_open() -> bool {
    TcpStream::connect_timeout(&SocketAddr::from(([127, 0, 0, 1], DIRECT_PORT)), Duration::from_millis(300)).is_ok()
}

/// Tailscale: installed, signed in and connected, and this PC's address on it.
fn tailscale_now() -> Value {
    let Some(exe) = tailscale_exe() else { return json!({ "installed": false }) };
    let v: Value = run_hidden(&exe.to_string_lossy(), &["status", "--json"]).ok().and_then(|o| serde_json::from_slice(&o.stdout).ok()).unwrap_or(Value::Null);
    let ip = v["Self"]["TailscaleIPs"].as_array().and_then(|a| a.iter().filter_map(Value::as_str).find(|s| s.contains('.')).map(String::from));
    // the user's other devices on it (a phone has to be there to connect from away)
    let peers: Vec<Value> = v["Peer"]
        .as_object()
        .map(|m| {
            m.values()
                .map(|p| {
                    let ip = p["TailscaleIPs"].as_array().and_then(|a| a.iter().filter_map(Value::as_str).find(|s| s.contains('.')).map(String::from));
                    json!({ "name": p["HostName"], "os": p["OS"], "online": p["Online"], "ip": ip })
                })
                .collect()
        })
        .unwrap_or_default();
    // the account the phone has to sign in with
    let account = v["Self"]["UserID"].as_u64().and_then(|u| v["User"][u.to_string()]["LoginName"].as_str().map(String::from));
    json!({ "installed": true, "on": v["BackendState"] == "Running", "state": v["BackendState"], "ip": ip, "peers": peers, "account": account, "dns": v["Self"]["DNSName"] })
}

static TS: Mutex<Option<(Instant, Value)>> = Mutex::new(None);

/// Tailscale's state, read at most every 20 s (the phone page asks every second).
pub fn tailscale(fresh: bool) -> Value {
    let mut c = TS.lock();
    if let Some((t, v)) = c.as_ref() {
        if !fresh && t.elapsed() < Duration::from_secs(20) {
            return v.clone();
        }
    }
    let v = tailscale_now();
    *c = Some((Instant::now(), v.clone()));
    v
}

/// The user's other computers on Tailscale that are online right now (their addresses): PC to PC
/// and Claude Code sync try them, so the PCs find each other away from home too, like the phone.
pub fn tailscale_pcs() -> Vec<std::net::IpAddr> {
    let t = tailscale(false);
    if t["on"] != true {
        return vec![];
    }
    t["peers"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter(|p| p["online"] == true && matches!(p["os"].as_str().unwrap_or("").to_ascii_lowercase().as_str(), "windows" | "linux" | "macos"))
                .filter_map(|p| p["ip"].as_str()?.parse().ok())
                .collect()
        })
        .unwrap_or_default()
}

/// An address on Tailscale (100.64.0.0/10).
pub fn is_tailscale(ip: std::net::IpAddr) -> bool {
    matches!(ip, std::net::IpAddr::V4(v) if v.octets()[0] == 100 && (64..128).contains(&v.octets()[1]))
}

/// This PC's Tailscale address while it's connected.
pub fn tailscale_ip() -> Option<String> {
    let t = tailscale(false);
    (t["on"] == true).then(|| t["ip"].as_str().map(String::from)).flatten()
}

/// The startup entries that bring back the tray icons (RustDesk's and Tailscale's).
fn tray_startups() -> Vec<crate::startup::Entry> {
    crate::startup::list().into_iter().filter(|e| {
        let n = e.name.to_lowercase();
        n.contains("rustdesk") || n == "tailscale" || e.command.to_lowercase().contains("tailscale-ipn")
    }).collect()
}

pub fn status() -> Value {
    let ts = tailscale(true);
    let Some(exe) = rustdesk_exe() else { return json!({ "installed": false, "tailscale": ts }) };
    let id = run_hidden(&exe.to_string_lossy(), &["--get-id"]).ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).filter(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()));
    let trays = tray_startups();
    json!({
        "installed": true,
        "on": service_running(),
        "id": id,
        "direct": direct_open(),
        "port": DIRECT_PORT,
        "lan": crate::net::lan_ip(),
        "tray_hidden": !trays.iter().any(|e| e.enabled),
        "tailscale": ts,
    })
}

/// Downloads the newest RustDesk from its GitHub releases, checks it (GitHub's SHA-256 and the
/// publisher's signature), installs it silently and turns on direct connections.
pub fn install() -> Result<(), String> {
    if rustdesk_exe().is_some() {
        return Ok(());
    }
    let agent = ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(10)).timeout(Duration::from_secs(300)).build();
    let rel: Value = agent.get("https://api.github.com/repos/rustdesk/rustdesk/releases/latest").set("User-Agent", "Deck").call().map_err(|e| format!("Couldn't reach GitHub ({e})"))?.into_json().map_err(|e| e.to_string())?;
    let asset = rel["assets"].as_array().and_then(|a| a.iter().find(|x| x["name"].as_str().is_some_and(|n| n.ends_with("-x86_64.msi")))).ok_or("No Windows setup in RustDesk's newest release")?;
    let (name, url) = (asset["name"].as_str().unwrap_or("rustdesk.msi"), asset["browser_download_url"].as_str().ok_or("No download link")?);
    let sha = asset["digest"].as_str().and_then(|d| d.strip_prefix("sha256:")).unwrap_or("");
    let mut body = Vec::new();
    agent.get(url).set("User-Agent", "Deck").call().map_err(|e| format!("Download failed ({e})"))?.into_reader().read_to_end(&mut body).map_err(|e| format!("Download failed ({e})"))?;
    let dir = std::env::temp_dir().join("Deck-rustdesk");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let file = dir.join(name);
    std::fs::write(&file, &body).map_err(|e| e.to_string())?;
    let f = ps_str(&file.to_string_lossy());
    let r = admin_ps(&format!(
        "$f = {f}
if ({sha} -and (Get-FileHash $f -Algorithm SHA256).Hash -ne {sha}) {{ 'The download is damaged. Try again.'; exit 3 }}
if ((Get-AuthenticodeSignature $f).Status -ne 'Valid') {{ 'The download is not signed by RustDesk.'; exit 3 }}
$p = Start-Process msiexec.exe -ArgumentList '/i', ('\"' + $f + '\"'), '/qn', '/norestart' -Wait -PassThru
if ($p.ExitCode -ne 0 -and $p.ExitCode -ne 3010) {{ \"RustDesk's setup failed (code $($p.ExitCode)).\"; exit 4 }}
for ($i = 0; $i -lt 30 -and -not (Get-Service RustDesk -ErrorAction SilentlyContinue); $i++) {{ Start-Sleep 1 }}
Start-Sleep 3
& (Join-Path $env:ProgramFiles 'RustDesk\\rustdesk.exe') --option direct-server Y | Out-Null",
        sha = ps_str(&sha.to_uppercase())
    ));
    let _ = std::fs::remove_file(&file);
    r.map(|_| ())
}

fn rd_cmd(args: &[&str]) -> String {
    let exe = ps_str(&rustdesk_exe().map(|p| p.to_string_lossy().to_string()).unwrap_or_default());
    format!("& {exe} {} | Out-Null", args.iter().map(|a| ps_str(a)).collect::<Vec<_>>().join(" "))
}

/// Remote desktop on (RustDesk's service runs and starts with Windows) or off (stopped, and it
/// stays off after a restart).
pub fn set_on(on: bool) -> Result<(), String> {
    rustdesk_exe().ok_or("RustDesk isn't installed")?;
    admin_ps(if on {
        "Set-Service RustDesk -StartupType Automatic; Start-Service RustDesk"
    } else {
        "Stop-Service RustDesk -Force; Set-Service RustDesk -StartupType Disabled"
    })
    .map(|_| ())
}

/// RustDesk's permanent password (otherwise it makes a new one-time password each time).
pub fn set_password(pw: &str) -> Result<(), String> {
    rustdesk_exe().ok_or("RustDesk isn't installed")?;
    if pw.chars().count() < 8 {
        return Err("Use at least 8 characters".into());
    }
    admin_ps(&rd_cmd(&["--password", pw])).map(|_| ())
}

/// Direct connections: straight to this PC by its address (home network or Tailscale), instead
/// of through RustDesk's servers by ID.
pub fn set_direct(on: bool) -> Result<(), String> {
    rustdesk_exe().ok_or("RustDesk isn't installed")?;
    admin_ps(&rd_cmd(&["--option", "direct-server", if on { "Y" } else { "N" }])).map(|_| ())
}

/// Hides RustDesk's and Tailscale's tray icons (Deck shows their state instead) or brings them
/// back. Both keep working without them: RustDesk runs as a service, and Tailscale is set to
/// stay connected without its tray app ("unattended").
pub fn set_tray_hidden(hide: bool) -> Result<(), String> {
    let mut script = String::new();
    if rustdesk_exe().is_some() {
        script += &rd_cmd(&["--option", "hide-tray", if hide { "Y" } else { "N" }]);
        script += "\n";
    }
    if hide {
        if let Some(ts) = tailscale_exe() {
            script += &format!("& {} set --unattended=true | Out-Null\n", ps_str(&ts.to_string_lossy()));
        }
        script += "Get-CimInstance Win32_Process -Filter \"Name='rustdesk.exe'\" | Where-Object { $_.CommandLine -match '--tray' } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force }\n";
        script += "Get-Process tailscale-ipn -ErrorAction SilentlyContinue | Stop-Process -Force\n";
    }
    if !script.is_empty() {
        admin_ps(&script)?;
    }
    for e in tray_startups() {
        let _ = crate::startup::set_enabled(&e.id, !hide);
    }
    if !hide {
        if let Some(rd) = rustdesk_exe() {
            let _ = crate::util::launch_as_user(&rd.to_string_lossy(), "--tray", "");
        }
        open_tailscale();
    }
    Ok(())
}

pub fn open_rustdesk() -> Result<(), String> {
    let rd = rustdesk_exe().ok_or("RustDesk isn't installed")?;
    crate::util::launch_as_user(&rd.to_string_lossy(), "", "")
}

/// Tailscale's own window (to sign in or switch accounts); without Tailscale, its download page.
pub fn open_tailscale() {
    match program_files(r"Tailscale\tailscale-ipn.exe") {
        Some(p) => {
            let _ = crate::util::launch_as_user(&p.to_string_lossy(), "", "");
        }
        None => {
            let _ = crate::util::shell_open("https://tailscale.com/download");
        }
    }
    *TS.lock() = None;
}

#[cfg(test)]
mod live {
    /// Prints what the Phone page would show on this PC (needs RustDesk/Tailscale installed).
    #[test]
    #[ignore]
    fn status_on_this_pc() {
        let t = std::time::Instant::now();
        println!("{}", serde_json::to_string_pretty(&super::status()).unwrap());
        println!("took {:?}", t.elapsed());
    }
}
