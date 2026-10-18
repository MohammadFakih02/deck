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
    let p = crate::util::program_files().join(rel);
    p.is_file().then_some(p)
}

fn rustdesk_exe() -> Option<PathBuf> {
    program_files(r"RustDesk\rustdesk.exe")
}

pub(crate) fn tailscale_exe() -> Option<PathBuf> {
    program_files(r"Tailscale\tailscale.exe")
}

/// The environment variable that carries a secret to an already-elevated PowerShell.
const SECRET_VAR: &str = "DECK_PS_SECRET";

/// Windows PowerShell's own folder (from the Windows folder's known-folder path, not %SystemRoot%).
fn ps_home() -> PathBuf {
    crate::util::windows_dir().join(r"System32\WindowsPowerShell\v1.0")
}

/// The only folder an elevated script loads modules from. PowerShell otherwise looks in the user's
/// Documents first (and wherever the user's PSModulePath points), so a program of the user's could
/// put its own Start-Process or Get-AuthenticodeSignature there and have it run with admin rights.
fn ps_modules() -> PathBuf {
    ps_home().join("Modules")
}

/// Runs a PowerShell script with admin rights: directly when Deck has them, otherwise through one
/// UAC prompt. The script goes base64-encoded, so paths need no quoting. Returns what the script
/// printed (only when Deck is already admin; an elevated child's output can't be read).
fn admin_ps(script: &str) -> Result<String, String> {
    admin_ps_secret(script, None)
}

/// `admin_ps` for a script that needs a secret (a password): the script reads it from `$secret`,
/// and the secret itself never goes on a command line (see `ps_args`).
fn admin_ps_secret(script: &str, secret: Option<&str>) -> Result<String, String> {
    use std::os::windows::process::CommandExt;
    let admin = crate::elevation::is_admin();
    // through UAC the elevated PowerShell gets a fresh environment, so the secret waits in a file
    // only this run knows the name of; the script deletes it as soon as it has read it
    let file = (secret.is_some() && !admin).then(|| std::env::temp_dir().join(format!("deck-{}.tmp", crate::util::random_token(20))));
    if let (Some(f), Some(s)) = (&file, secret) {
        std::fs::write(f, s).map_err(|e| e.to_string())?;
    }
    let (args, env) = ps_args(script, secret, admin, file.as_deref());
    let mut cmd = std::process::Command::new(ps_home().join("powershell.exe"));
    cmd.args(&args).env("PSModulePath", ps_modules()).creation_flags(crate::util::CREATE_NO_WINDOW);
    if let Some(v) = env {
        cmd.env(SECRET_VAR, v);
    }
    let out = cmd.output();
    if let Some(f) = &file {
        let _ = std::fs::remove_file(f);
    }
    let out = out.map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if out.status.success() {
        Ok(text)
    } else {
        Err(if text.is_empty() { "It didn't finish.".into() } else { text })
    }
}

/// PowerShell's arguments for `script`, and the value for `SECRET_VAR` if the secret travels in
/// the environment. A secret is never in the arguments: process lists and audit logs show those.
/// Already admin: the child inherits it through `SECRET_VAR`. Through UAC: the elevated script reads
/// it from `secret_file` and deletes the file.
fn ps_args(script: &str, secret: Option<&str>, admin: bool, secret_file: Option<&std::path::Path>) -> (Vec<String>, Option<String>) {
    let enc = |s: &str| crate::util::base64(&s.encode_utf16().flat_map(u16::to_le_bytes).collect::<Vec<u8>>());
    let take = match (secret, admin, secret_file) {
        (None, _, _) => String::new(),
        (Some(_), true, _) => format!("$secret = $env:{SECRET_VAR}; Remove-Item Env:{SECRET_VAR}\n"),
        (Some(_), false, Some(f)) => {
            let f = ps_str(&f.to_string_lossy());
            format!("$secret = [IO.File]::ReadAllText({f}); Remove-Item -LiteralPath {f} -Force\n")
        }
        (Some(_), false, None) => "$secret = ''\n".into(),
    };
    // the module path again inside the script: through UAC the elevated copy gets a fresh environment
    let modules = ps_str(&ps_modules().to_string_lossy());
    let script = format!("$env:PSModulePath = {modules}\n$ErrorActionPreference = 'Stop'\ntry {{\n{take}{script}\n}} catch {{ $_.Exception.Message; exit 1 }}");
    let cmd = if admin {
        enc(&script)
    } else {
        enc(&format!(
            "$env:PSModulePath = {modules}\ntry {{ $p = Start-Process {} -Verb RunAs -WindowStyle Hidden -Wait -PassThru -ArgumentList '-NoProfile','-NonInteractive','-EncodedCommand','{}'; exit $p.ExitCode }} catch {{ 'Admin rights were not given.'; exit 1 }}",
            ps_str(&ps_home().join("powershell.exe").to_string_lossy()),
            enc(&script)
        ))
    };
    let env = if admin { secret.map(String::from) } else { None };
    (vec!["-NoProfile".into(), "-NonInteractive".into(), "-EncodedCommand".into(), cmd], env)
}

/// A PowerShell single-quoted string. PowerShell also takes the typographic single quotes as
/// quotes, so every kind is doubled.
fn ps_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        if matches!(c, '\'' | '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}') {
            out.push(c);
        }
        out.push(c);
    }
    out.push('\'');
    out
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
    let url = asset["browser_download_url"].as_str().ok_or("No download link")?;
    let sha = asset["digest"].as_str().and_then(|d| d.strip_prefix("sha256:")).filter(|d| d.len() == 64).ok_or("GitHub gave no checksum for RustDesk's setup")?;
    let mut body = Vec::new();
    agent.get(url).set("User-Agent", "Deck").call().map_err(|e| format!("Download failed ({e})"))?.into_reader().read_to_end(&mut body).map_err(|e| format!("Download failed ({e})"))?;
    let file = std::env::temp_dir().join(format!("deck-rustdesk-{}.msi", crate::util::random_token(16)));
    std::fs::write(&file, &body).map_err(|e| e.to_string())?;
    let stage = crate::util::windows_dir().join("Temp").join(format!("deck-rd-{}", crate::util::random_token(20)));
    let r = admin_ps(&install_script(&file, &stage, sha));
    let _ = std::fs::remove_file(&file);
    r.map(|_| ())
}

/// RustDesk's publisher, as its setup's code-signing certificate names it (an EV certificate).
const RUSTDESK_SIGNER: &str = "PURSLANE";

/// The elevated install. The download sits in the user's Temp folder, where the user's programs
/// could swap it between a check and the install, so the script first copies it into a new folder
/// under Windows\Temp (users can't change what an administrator puts there), keeps that copy open
/// read-only, checks it (GitHub's SHA-256 and RustDesk's own signature, not just any valid one)
/// and installs exactly that file.
fn install_script(download: &std::path::Path, stage: &std::path::Path, sha: &str) -> String {
    let p = |x: &std::path::Path| ps_str(&x.to_string_lossy());
    let msiexec = crate::util::windows_dir().join(r"System32\msiexec.exe");
    let rd = crate::util::program_files().join(r"RustDesk\rustdesk.exe");
    format!(
        "$d = {stage}
New-Item -ItemType Directory -Path $d | Out-Null
$f = Join-Path $d 'rustdesk.msi'
Copy-Item -LiteralPath {src} -Destination $f
$hold = [IO.File]::Open($f, 'Open', 'Read', 'Read')
try {{
if ((Get-FileHash -LiteralPath $f -Algorithm SHA256).Hash -ne {sha}) {{ throw 'The download is damaged. Try again.' }}
$s = Get-AuthenticodeSignature -LiteralPath $f
if ($s.Status -ne 'Valid' -or $s.SignerCertificate.Subject -notmatch '(^|, )O={signer}(,|$)') {{ throw 'The download is not signed by RustDesk.' }}
$p = Start-Process {msiexec} -ArgumentList '/i', ('\"' + $f + '\"'), '/qn', '/norestart' -Wait -PassThru
if ($p.ExitCode -ne 0 -and $p.ExitCode -ne 3010) {{ throw \"RustDesk's setup failed (code $($p.ExitCode)).\" }}
}} finally {{ $hold.Close(); Remove-Item -LiteralPath $d -Recurse -Force -ErrorAction SilentlyContinue }}
for ($i = 0; $i -lt 30 -and -not (Get-Service RustDesk -ErrorAction SilentlyContinue); $i++) {{ Start-Sleep 1 }}
Start-Sleep 3
& {rd} --option direct-server Y | Out-Null",
        stage = p(stage),
        src = p(download),
        sha = ps_str(&sha.to_uppercase()),
        signer = RUSTDESK_SIGNER,
        msiexec = p(&msiexec),
        rd = p(&rd),
    )
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
    // RustDesk only takes the password as an argument; it is kept off PowerShell's own command line
    let exe = ps_str(&rustdesk_exe().map(|p| p.to_string_lossy().to_string()).unwrap_or_default());
    admin_ps_secret(&format!("& {exe} '--password' $secret | Out-Null"), Some(pw)).map(|_| ())
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

#[cfg(test)]
mod ps_tests {
    fn dec(b64: &str) -> String {
        let b = crate::util::unbase64(b64).unwrap();
        String::from_utf16_lossy(&b.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect::<Vec<_>>())
    }

    fn decoded(args: &[String]) -> String {
        let i = args.iter().position(|a| a == "-EncodedCommand").expect("encoded command");
        dec(&args[i + 1])
    }

    /// PowerShell also ends a single-quoted string at the typographic quotes; each must be doubled.
    #[test]
    fn every_kind_of_single_quote_is_escaped() {
        for q in ['\'', '\u{2018}', '\u{2019}', '\u{201A}', '\u{201B}'] {
            assert_eq!(super::ps_str(&format!("a{q}b")), format!("'a{q}{q}b'"));
        }
        assert_eq!(super::ps_str("plain"), "'plain'");
    }

    /// A password never goes on PowerShell's command line (process lists and audit logs show
    /// those): with admin rights it travels in the child's environment, through UAC in a file that
    /// the elevated script reads and deletes.
    #[test]
    fn a_secret_stays_off_the_command_line() {
        let pw = "hunter2-Sekr1t";
        let (args, env) = super::ps_args("Write-Output $secret", Some(pw), true, None);
        assert!(!args.iter().any(|a| a.contains(pw)) && !decoded(&args).contains(pw));
        assert_eq!(env.as_deref(), Some(pw));
        let f = std::env::temp_dir().join("deck-secret-test.tmp");
        let (args, env) = super::ps_args("Write-Output $secret", Some(pw), false, Some(&f));
        let outer = decoded(&args);
        assert!(!args.iter().any(|a| a.contains(pw)) && !outer.contains(pw));
        assert!(env.is_none());
        // the inner (elevated) script is encoded inside the outer one; it names the file instead
        let inner_b64 = outer.split('\'').find(|s| s.len() > 40 && crate::util::unbase64(s).is_some()).expect("inner script");
        let inner = dec(inner_b64);
        assert!(!inner.contains(pw) && inner.contains("deck-secret-test.tmp"));
    }

    /// The script really gets the secret both ways (run without elevation: the elevated hop is
    /// Windows' job, what runs after it is the inner script tested here).
    #[test]
    fn the_script_receives_the_secret() {
        // (ASCII only: the console's code page would mangle other characters on the way back)
        let pw = "it's a \"test\" $pw;";
        let (args, env) = super::ps_args("Write-Output $secret", Some(pw), true, None);
        let out = std::process::Command::new("powershell.exe").args(&args).env(super::SECRET_VAR, env.unwrap()).output().unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), pw, "{}", String::from_utf8_lossy(&out.stderr));
        let f = std::env::temp_dir().join(format!("deck-secret-{}.tmp", crate::util::random_token(8)));
        std::fs::write(&f, "plain-Secret_1").unwrap();
        let (args, _) = super::ps_args("Write-Output $secret", Some("plain-Secret_1"), false, Some(&f));
        let outer = decoded(&args);
        let inner = outer.split('\'').find(|s| s.len() > 40 && crate::util::unbase64(s).is_some()).unwrap().to_string();
        let out = std::process::Command::new("powershell.exe").args(["-NoProfile", "-NonInteractive", "-EncodedCommand", &inner]).output().unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "plain-Secret_1");
        assert!(!f.exists(), "the secret file is deleted once read");
    }
    /// A program of the user's can put a module first on PowerShell's module path (the user's own
    /// Documents folder is first by default) that replaces a built-in command; the elevated script
    /// must still get Windows' own.
    #[test]
    fn a_planted_module_is_not_loaded() {
        let root = std::env::temp_dir().join(format!("deck-psm-{}", crate::util::random_token(8)));
        let m = root.join("Probe");
        std::fs::create_dir_all(&m).unwrap();
        std::fs::write(m.join("Probe.psm1"), "function Get-AuthenticodeSignature { 'PLANTED' }
Export-ModuleMember -Function *").unwrap();
        std::fs::write(m.join("Probe.psd1"), "@{ ModuleVersion='1.0'; RootModule='Probe.psm1'; FunctionsToExport=@('Get-AuthenticodeSignature') }").unwrap();
        let poisoned = format!("{};{}", root.display(), std::env::var("PSModulePath").unwrap_or_default());
        // what the elevated script sees: the poisoned path inherited (as through UAC), its own line first
        let (args, _) = super::ps_args("(Get-Command Get-AuthenticodeSignature).Source", None, true, None);
        let out = std::process::Command::new(super::ps_home().join("powershell.exe")).args(&args).env("PSModulePath", &poisoned).output().unwrap();
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "Microsoft.PowerShell.Security", "{}", String::from_utf8_lossy(&out.stderr));
    }

    /// The install checks and runs a copy in an admin-only folder, needs GitHub's checksum and
    /// RustDesk's own signature, and takes no folder from the environment.
    #[test]
    fn the_install_checks_the_copy_it_installs() {
        let dl = std::env::temp_dir().join("deck-rustdesk-x.msi");
        let stage = crate::util::windows_dir().join(r"Temp\deck-rd-x");
        let s = super::install_script(&dl, &stage, &"ab".repeat(32));
        assert!(!s.contains("$env:"), "{s}");
        assert!(s.contains(&"AB".repeat(32)) && s.contains("O=PURSLANE"), "{s}");
        // the download is only ever copied; everything else uses the staged copy
        let src = super::ps_str(&dl.to_string_lossy());
        assert_eq!(s.matches(src.as_str()).count(), 1);
        assert!(s.contains(&format!("Copy-Item -LiteralPath {src} -Destination $f")));
        assert!(s.contains("Start-Process 'C:\\") && s.contains("msiexec.exe'"), "{s}");
    }

    /// Holding the staged copy open read-only leaves Get-FileHash and the signature check working.
    #[test]
    fn the_held_copy_can_still_be_checked() {
        // a signed program (Windows' own), copied: its signature travels with it
        let f = std::env::temp_dir().join(format!("deck-hold-{}.exe", crate::util::random_token(8)));
        std::fs::copy(crate::util::windows_dir().join(r"System32\msiexec.exe"), &f).unwrap();
        let fp = super::ps_str(&f.to_string_lossy());
        let script = format!("$hold = [IO.File]::Open({fp}, 'Open', 'Read', 'Read')
try {{ (Get-FileHash -LiteralPath {fp} -Algorithm SHA256).Hash.Length; [string](Get-AuthenticodeSignature -LiteralPath {fp}).Status }} finally {{ $hold.Close() }}");
        let (args, _) = super::ps_args(&script, None, true, None);
        let out = std::process::Command::new(super::ps_home().join("powershell.exe")).args(&args).output().unwrap();
        let _ = std::fs::remove_file(&f);
        let text = String::from_utf8_lossy(&out.stdout);
        assert_eq!(text.lines().map(str::trim).collect::<Vec<_>>(), ["64", "Valid"], "{}", String::from_utf8_lossy(&out.stderr));
    }
}
