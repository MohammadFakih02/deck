//! Store apps without the Microsoft Store (Tweaks → Built-in apps → Get Store apps), the way
//! store.rg-adguard.net does it: Microsoft's own update service (FE3, the one Windows Update and the
//! Store use) is asked for an app's package files, and they're downloaded straight from Microsoft's
//! servers. No Store app, no sign-in, no Store install service.
//!
//! 1. The Store catalog gives the app's update category (`WuCategoryId`) and package family.
//! 2. FE3 gives a cookie, then the files in that category (`SyncUpdates`), then each file's link
//!    (`GetExtendedUpdateInfo2`). Protocol as in pull-microsoft-store (MIT).
//! 3. Deck picks the newest version of each package for this PC (its architecture or neutral; a
//!    bundle for the app), downloads them (the links are plain http on Microsoft's CDN, so each file's
//!    SHA-256 is checked against the one FE3 listed), then installs the dependencies and the app
//!    with Add-AppxPackage (Windows checks the packages' signatures).
//!
//! Or the links go to FluxDM (its local API), or are shown to copy, like rg-adguard's page.
//!
//! Two of FE3's servers use Microsoft roots that aren't in the usual web list (Windows trusts them):
//! Deck carries those two public root certificates (`tweaks/certs`) for these requests only.

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const FE3: &str = "https://fe3.delivery.mp.microsoft.com/ClientWebService/client.asmx";
const FE3CR: &str = "https://fe3cr.delivery.mp.microsoft.com/ClientWebService/client.asmx";
const MS_ROOT_2011: &[u8] = include_bytes!("../../tweaks/certs/MicRooCerAut2011.crt");
const MS_ECC_ROOT_2018: &[u8] = include_bytes!("../../tweaks/certs/MicEccProductRoot2018.crt");

fn agent() -> ureq::Agent {
    static A: std::sync::OnceLock<ureq::Agent> = std::sync::OnceLock::new();
    A.get_or_init(|| {
        use tokio_rustls::rustls;
        let mut roots = rustls::RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        for der in [MS_ROOT_2011, MS_ECC_ROOT_2018] {
            let _ = roots.add(rustls::pki_types::CertificateDer::from(der.to_vec()));
        }
        let cfg = rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .expect("tls versions")
            .with_root_certificates(roots)
            .with_no_client_auth();
        crate::log::http(ureq::AgentBuilder::new()).tls_config(Arc::new(cfg)).timeout_connect(Duration::from_secs(15)).timeout_read(Duration::from_secs(60)).build()
    })
    .clone()
}

/// The PC's Store market and language ("US", "en-US").
pub fn market() -> (String, String) {
    use windows::Win32::Globalization::GetUserDefaultLocaleName;
    let mut buf = [0u16; 85];
    let n = unsafe { GetUserDefaultLocaleName(&mut buf) };
    let loc = if n > 1 { String::from_utf16_lossy(&buf[..n as usize - 1]) } else { "en-US".into() };
    let market = loc.split('-').nth(1).filter(|m| m.len() == 2 && m.chars().all(|c| c.is_ascii_alphabetic())).unwrap_or("US").to_uppercase();
    (market, loc)
}

pub fn valid_id(id: &str) -> bool {
    id.len() == 12 && id.chars().all(|c| c.is_ascii_alphanumeric())
}

/// One package file of an app.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct PkgFile {
    /// "SpotifyAB.SpotifyMusic_1.2.3.0_x64__zpdnekdrzrea0.appx"
    pub name: String,
    pub package: String,
    pub version: String,
    pub arch: String,
    pub size: u64,
    /// the app itself (else a dependency: VCLibs, UI.Xaml…)
    pub main: bool,
    #[serde(skip)]
    sha1: String,
    #[serde(skip)]
    sha256: Option<String>,
    #[serde(skip)]
    update: (String, String),
    pub url: Option<String>,
    /// a dependency this PC already has (this version or newer): not downloaded
    #[serde(default)]
    pub have: bool,
}

/// Packages installed for you: (name, architecture) → versions. Dependencies are shared
/// (VCLibs, UI.Xaml, the App Runtime), so most PCs already have them.
fn installed() -> HashMap<(String, String), Vec<Vec<u64>>> {
    let mut m: HashMap<(String, String), Vec<Vec<u64>>> = HashMap::new();
    let Ok(o) = crate::tweaks::run(60, 
        "powershell.exe",
        &["-NoProfile", "-NonInteractive", "-Command", "Get-AppxPackage | Select-Object Name,@{n='V';e={$_.Version.ToString()}},@{n='A';e={$_.Architecture.ToString()}} | ConvertTo-Json -Compress"],
    ) else {
        return m;
    };
    let v: Value = serde_json::from_slice(&o.stdout).unwrap_or(Value::Null);
    let list = match v {
        Value::Array(a) => a,
        o @ Value::Object(_) => vec![o],
        _ => vec![],
    };
    for p in list {
        if let (Some(n), Some(ver), Some(a)) = (p["Name"].as_str(), p["V"].as_str(), p["A"].as_str()) {
            m.entry((n.to_lowercase(), a.to_lowercase())).or_default().push(version_key(ver));
        }
    }
    m
}

/// Marks the dependencies this PC already has.
fn mark_installed(files: &mut [PkgFile]) {
    let have = installed();
    for f in files.iter_mut().filter(|f| !f.main) {
        f.have = have.get(&(f.package.to_lowercase(), f.arch.clone())).is_some_and(|vs| vs.iter().any(|v| *v >= version_key(&f.version)));
    }
}

pub struct App {
    pub title: String,
    /// the package family ("Microsoft.Todos_8wekyb3d8bbwe")
    pub family: String,
    pub files: Vec<PkgFile>,
}

/// A request to Microsoft's servers is tried this many times when they answer with a server error
/// (500 and the like: overloaded, or turning requests away for a moment) or don't answer.
const REQUEST_TRIES: u32 = 4;

/// How long to wait before try number `attempt` (1-based): about 1.5 s, 4 s, 9 s, a little different
/// each time so several requests don't come back at the same moment.
fn backoff(attempt: u32) -> Duration {
    let jitter = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_millis() % 700) as u64;
    if cfg!(test) {
        return Duration::from_millis(5);
    }
    Duration::from_millis([0, 1500, 4000, 9000][(attempt as usize).min(3)] + jitter)
}

/// Whether trying again could help: the server's own trouble or no answer, not a refusal of the request.
fn transient(e: &ureq::Error) -> bool {
    match e {
        ureq::Error::Status(c, _) => matches!(c, 429 | 500 | 502 | 503 | 504),
        ureq::Error::Transport(_) => true,
    }
}

/// What went wrong, in words (no URL: it holds the app's id).
fn describe(e: &ureq::Error) -> String {
    match e {
        ureq::Error::Status(429, _) => "status code 429 (too many requests: Microsoft asked Deck to slow down)".into(),
        ureq::Error::Status(c @ (500 | 502 | 503 | 504), _) => format!("status code {c} (an error on Microsoft's side, usually for a moment)"),
        ureq::Error::Status(c, _) => format!("status code {c}"),
        ureq::Error::Transport(t) => match t.kind() {
            ureq::ErrorKind::Dns => "no internet, or the name can't be looked up".into(),
            ureq::ErrorKind::ConnectionFailed | ureq::ErrorKind::Io => "the connection broke or timed out".into(),
            _ => "no answer".into(),
        },
    }
}

/// Runs a request, trying again (with waits) while the answer is a server error or no answer.
/// `who`: whose answer it is, for the message when every try failed.
fn retrying<T>(who: &str, mut f: impl FnMut() -> Result<T, Box<ureq::Error>>) -> Result<T, String> {
    let mut last = String::new();
    for attempt in 0..REQUEST_TRIES {
        if attempt > 0 {
            std::thread::sleep(backoff(attempt));
        }
        match f() {
            Ok(v) => return Ok(v),
            Err(e) => {
                last = describe(&e);
                if !transient(&e) {
                    return Err(format!("{who} didn't answer: {last}"));
                }
            }
        }
    }
    Err(format!("{who} didn't answer: {last}, also after {REQUEST_TRIES} tries"))
}

fn soap_post(url: &str, body: &str) -> Result<String, String> {
    let r = retrying("Microsoft's update service", || agent().post(url).set("Content-Type", "application/soap+xml; charset=utf-8").send_string(body).map_err(Box::new))?;
    r.into_string().map_err(|e| e.to_string())
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&#39;", "'").replace("&amp;", "&")
}

fn tag_text(xml: &str, tag: &str) -> Option<String> {
    let doc = roxmltree::Document::parse(xml).ok()?;
    doc.descendants().find(|n| n.tag_name().name() == tag).and_then(|n| n.text()).map(String::from)
}

/// The update service's cookie, kept for a few minutes: one is enough for every app of a check
/// (asking for one per app doubled the requests and made Microsoft turn some away).
static COOKIE: Mutex<Option<(Instant, String)>> = parking_lot::const_mutex(None);

fn cookie() -> Result<String, String> {
    if let Some((at, c)) = COOKIE.lock().as_ref() {
        if at.elapsed() < Duration::from_secs(300) {
            return Ok(c.clone());
        }
    }
    let c = fresh_cookie()?;
    *COOKIE.lock() = Some((Instant::now(), c.clone()));
    Ok(c)
}

fn fresh_cookie() -> Result<String, String> {
    let body = format!(
        r#"<Envelope xmlns="http://www.w3.org/2003/05/soap-envelope" xmlns:a="http://www.w3.org/2005/08/addressing" xmlns:u="http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-wssecurity-utility-1.0.xsd"><Header><a:Action mustUnderstand="1">http://www.microsoft.com/SoftwareDistribution/Server/ClientWebService/GetCookie</a:Action><a:To mustUnderstand="1">{FE3CR}</a:To><Security mustUnderstand="1" xmlns="http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-wssecurity-secext-1.0.xsd"><WindowsUpdateTicketsToken xmlns="http://schemas.microsoft.com/msus/2014/10/WindowsUpdateAuthorization" u:id="ClientMSA"></WindowsUpdateTicketsToken></Security></Header><Body></Body></Envelope>"#
    );
    tag_text(&soap_post(FE3CR, &body)?, "EncryptedData").ok_or_else(|| "Microsoft's update service gave no cookie".into())
}

const NON_LEAF: &[u32] = &[
    1, 2, 3, 11, 19, 544, 549, 2359974, 2359977, 5169044, 8788830, 23110993, 23110994, 54341900, 54343656, 59830006, 59830007, 59830008, 60484010, 62450018, 62450019, 62450020, 66027979, 66053150, 97657898,
    98822896, 98959022, 98959023, 98959024, 98959025, 98959026, 104433538, 104900364, 105489019, 117765322, 129905029, 130040031, 132387090, 132393049, 133399034, 138537048, 140377312, 143747671,
    158941041, 158941042, 158941043, 158941044, 159123858, 159130928, 164836897, 164847386, 164848327, 164852241, 164852246, 164852252, 164852253,
];
const OTHER_CACHED: &[u32] = &[10, 17, 2359977, 5143990, 5169043, 5169047, 8806526, 9125350, 9154769, 10809856, 23110995, 23110996, 23110999, 23111000, 23111001, 23111002, 23111003, 23111004, 24513870, 28880263];

fn iso(t: std::time::SystemTime) -> String {
    let s = t.duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs() as i64;
    let (days, rem) = (s.div_euclid(86400), s.rem_euclid(86400));
    // civil from days (Howard Hinnant)
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

fn sync_updates(cookie: &str, category: &str) -> Result<String, String> {
    let now = std::time::SystemTime::now();
    let ints = |v: &[u32]| v.iter().map(|i| format!("<int>{i}</int>")).collect::<String>();
    let (_, loc) = market();
    let body = format!(
        r#"<s:Envelope xmlns:a="http://www.w3.org/2005/08/addressing" xmlns:s="http://www.w3.org/2003/05/soap-envelope"><s:Header><a:Action s:mustUnderstand="1">http://www.microsoft.com/SoftwareDistribution/Server/ClientWebService/SyncUpdates</a:Action><a:MessageID>urn:uuid:175df68c-4b91-41ee-b70b-f2208c65438e</a:MessageID><a:To s:mustUnderstand="1">{FE3}</a:To><o:Security s:mustUnderstand="1" xmlns:o="http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-wssecurity-secext-1.0.xsd"><Timestamp xmlns="http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-wssecurity-utility-1.0.xsd"><Created>{created}</Created><Expires>{expires}</Expires></Timestamp><wuws:WindowsUpdateTicketsToken wsu:id="ClientMSA" xmlns:wsu="http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-wssecurity-utility-1.0.xsd" xmlns:wuws="http://schemas.microsoft.com/msus/2014/10/WindowsUpdateAuthorization"><TicketType Name="MSA" Version="1.0" Policy="MBI_SSL">Retail</TicketType></wuws:WindowsUpdateTicketsToken></o:Security></s:Header><s:Body><SyncUpdates xmlns="http://www.microsoft.com/SoftwareDistribution/Server/ClientWebService"><cookie><Expiration>2045-03-11T02:02:48Z</Expiration><EncryptedData>{cookie}</EncryptedData></cookie><parameters><ExpressQuery>false</ExpressQuery><InstalledNonLeafUpdateIDs>{nl}</InstalledNonLeafUpdateIDs><OtherCachedUpdateIDs>{oc}</OtherCachedUpdateIDs><SkipSoftwareSync>false</SkipSoftwareSync><NeedTwoGroupOutOfScopeUpdates>true</NeedTwoGroupOutOfScopeUpdates><FilterAppCategoryIds><CategoryIdentifier><Id>{category}</Id></CategoryIdentifier></FilterAppCategoryIds><TreatAppCategoryIdsAsInstalled>true</TreatAppCategoryIdsAsInstalled><AlsoPerformRegularSync>false</AlsoPerformRegularSync><ComputerSpec /><ExtendedUpdateInfoParameters><XmlUpdateFragmentTypes><XmlUpdateFragmentType>Extended</XmlUpdateFragmentType></XmlUpdateFragmentTypes><Locales><string>{loc}</string><string>en-US</string><string>en</string></Locales></ExtendedUpdateInfoParameters><ClientPreferredLanguages><string>{loc}</string></ClientPreferredLanguages><ProductsParameters><SyncCurrentVersionOnly>false</SyncCurrentVersionOnly><DeviceAttributes>BranchReadinessLevel=CB;CurrentBranch=rs_prerelease;FlightRing=Retail;FlightingBranchName=external;IsFlightingEnabled=1;InstallLanguage={loc};OSUILocale={loc};InstallationType=Client;DeviceFamily=Windows.Desktop;</DeviceAttributes><CallerAttributes>Interactive=1;IsSeeker=0;</CallerAttributes><Products /></ProductsParameters></parameters></SyncUpdates></s:Body></s:Envelope>"#,
        created = iso(now),
        expires = iso(now + Duration::from_secs(300)),
        nl = ints(NON_LEAF),
        oc = ints(OTHER_CACHED),
    );
    Ok(unescape(&soap_post(FE3, &body)?))
}

fn file_url(update: &(String, String), sha1: &str) -> Result<String, String> {
    let (id, rev) = update;
    let body = format!(
        r#"<Envelope xmlns="http://www.w3.org/2003/05/soap-envelope" xmlns:a="http://www.w3.org/2005/08/addressing" xmlns:u="http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-wssecurity-utility-1.0.xsd"><Header><a:Action mustUnderstand="1">http://www.microsoft.com/SoftwareDistribution/Server/ClientWebService/GetExtendedUpdateInfo2</a:Action><a:To mustUnderstand="1">{FE3CR}/secured</a:To><Security mustUnderstand="1" xmlns="http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-wssecurity-secext-1.0.xsd"><WindowsUpdateTicketsToken xmlns="http://schemas.microsoft.com/msus/2014/10/WindowsUpdateAuthorization" u:id="ClientMSA"></WindowsUpdateTicketsToken></Security></Header><Body><GetExtendedUpdateInfo2 xmlns="http://www.microsoft.com/SoftwareDistribution/Server/ClientWebService"><updateIDs><UpdateIdentity><UpdateID>{id}</UpdateID><RevisionNumber>{rev}</RevisionNumber></UpdateIdentity></updateIDs><infoTypes><XmlUpdateFragmentType>FileUrl</XmlUpdateFragmentType><XmlUpdateFragmentType>FileDecryption</XmlUpdateFragmentType></infoTypes><deviceAttributes>FlightRing=Retail;</deviceAttributes></GetExtendedUpdateInfo2></Body></Envelope>"#
    );
    let xml = soap_post(&format!("{FE3CR}/secured"), &body)?;
    let doc = roxmltree::Document::parse(&xml).map_err(|e| e.to_string())?;
    let mut fallback = None;
    for loc in doc.descendants().filter(|n| n.tag_name().name() == "FileLocation") {
        let digest = loc.children().find(|c| c.tag_name().name() == "FileDigest").and_then(|c| c.text()).unwrap_or("");
        let Some(url) = loc.children().find(|c| c.tag_name().name() == "Url").and_then(|c| c.text()) else { continue };
        if digest == sha1 {
            return Ok(url.to_string());
        }
        // the other link is the package's block map (always 99 characters long)
        if url.len() != 99 {
            fallback.get_or_insert(url.to_string());
        }
    }
    fallback.ok_or_else(|| "Microsoft's update service gave no link for this file".into())
}

fn this_arch() -> &'static str {
    match std::env::var("PROCESSOR_ARCHITECTURE").unwrap_or_default().to_ascii_uppercase().as_str() {
        "ARM64" => "arm64",
        "X86" => "x86",
        _ => "x64",
    }
}

fn version_key(v: &str) -> Vec<u64> {
    v.split('.').map(|p| p.parse().unwrap_or(0)).collect()
}

/// The app's files for this PC: the newest of each package that runs here (a bundle for the app
/// when there is one), encrypted (paid/DRM) packages left out.
fn pick(all: Vec<PkgFile>) -> Vec<PkgFile> {
    let arch = this_arch();
    let rank = |a: &str| -> Option<u8> {
        if a == arch {
            Some(3)
        } else if a == "neutral" {
            Some(2)
        } else if a == "x86" && arch == "x64" {
            Some(1)
        } else {
            None
        }
    };
    let mut best: HashMap<String, PkgFile> = HashMap::new();
    for f in all {
        let lower = f.name.to_lowercase();
        if lower.ends_with(".eappx") || lower.ends_with(".emsix") || lower.ends_with(".eappxbundle") || lower.ends_with(".emsixbundle") || lower.ends_with(".blockmap") {
            continue;
        }
        if rank(&f.arch).is_none() {
            continue;
        }
        // the app: a bundle (all its parts) over a single package; then the newest; then this PC's own architecture
        let score = |x: &PkgFile| (x.main && x.name.to_lowercase().ends_with("bundle"), version_key(&x.version), rank(&x.arch).unwrap_or(0));
        let key = f.package.to_lowercase();
        if best.get(&key).is_none_or(|b| score(&f) > score(b)) {
            best.insert(key, f);
        }
    }
    let mut v: Vec<PkgFile> = best.into_values().collect();
    // dependencies first: they're installed before the app
    v.sort_by(|a, b| a.main.cmp(&b.main).then(a.name.cmp(&b.name)));
    v
}

/// Finds an app's package files (without links yet). Err when the app isn't a package (a desktop
/// app with its own installer: those install with winget instead).
pub fn find(id: &str) -> Result<App, String> {
    if !valid_id(id) {
        return Err("bad Store id".into());
    }
    let (market, loc) = market();
    let url = format!("https://storeedgefd.dsx.mp.microsoft.com/v9.0/products/{id}?market={market}&locale={loc}&deviceFamily=Windows.Desktop");
    let v: Value = retrying("The Store catalog", || agent().get(&url).call().map_err(Box::new))?.into_json().map_err(|e| e.to_string())?;
    let title = v["Payload"]["Title"].as_str().unwrap_or(id).to_string();
    let ful: Value = match &v["Payload"]["Skus"][0]["FulfillmentData"] {
        Value::String(s) => serde_json::from_str(s).unwrap_or(Value::Null),
        o => o.clone(),
    };
    let (Some(cat), Some(family)) = (ful["WuCategoryId"].as_str(), ful["PackageFamilyName"].as_str()) else {
        return Err("NOT_PACKAGED".into());
    };
    let prefix = family.rsplit_once('_').map(|(a, _)| a).unwrap_or(family).to_lowercase();
    let xml = sync_updates(&cookie()?, cat).inspect_err(|_| *COOKIE.lock() = None)?;
    let doc = roxmltree::Document::parse(&xml).map_err(|e| format!("Microsoft's answer couldn't be read: {e}"))?;
    let id_of = |n: roxmltree::Node| n.descendants().find(|d| d.tag_name().name() == "ID").and_then(|d| d.text()).map(String::from);
    // files by update id
    let mut files: HashMap<String, PkgFile> = HashMap::new();
    for node in doc.descendants().filter(|n| n.tag_name().name() == "Files") {
        let Some(holder) = node.parent().and_then(|p| p.parent()) else { continue };
        let Some(fid) = id_of(holder) else { continue };
        let Some(file) = node.children().find(|c| c.is_element()) else { continue };
        let (Some(isi), Some(fname)) = (file.attribute("InstallerSpecificIdentifier"), file.attribute("FileName")) else { continue };
        let parts: Vec<&str> = isi.split('_').collect();
        if parts.len() < 3 {
            continue;
        }
        let ext = fname.rsplit_once('.').map(|(_, e)| e).unwrap_or("appx");
        let sha256 = file.children().find(|c| c.tag_name().name() == "AdditionalDigest" && c.attribute("Algorithm") == Some("SHA256")).and_then(|c| c.text()).map(String::from);
        files.insert(
            fid,
            PkgFile {
                name: format!("{isi}.{ext}"),
                package: parts[0].to_string(),
                version: parts[1].to_string(),
                arch: parts[2].to_lowercase(),
                size: file.attribute("Size").and_then(|s| s.parse().ok()).unwrap_or(0),
                main: isi.to_lowercase().starts_with(&prefix),
                sha1: file.attribute("Digest").unwrap_or("").to_string(),
                sha256,
                update: (String::new(), String::new()),
                url: None,
                have: false,
            },
        );
    }
    // their update ids and revisions (the second list of the answer)
    for node in doc.descendants().filter(|n| n.tag_name().name() == "SecuredFragment") {
        let Some(holder) = node.parent().and_then(|p| p.parent()).and_then(|p| p.parent()) else { continue };
        let Some(fid) = id_of(holder) else { continue };
        let Some(f) = files.get_mut(&fid) else { continue };
        let Some(upd) = node.parent().and_then(|p| p.parent()).and_then(|p| p.children().find(|c| c.is_element())) else { continue };
        if let (Some(u), Some(r)) = (upd.attribute("UpdateID"), upd.attribute("RevisionNumber")) {
            f.update = (u.to_string(), r.to_string());
        }
    }
    let all: Vec<PkgFile> = files.into_values().filter(|f| !f.update.0.is_empty()).collect();
    if all.iter().any(|f| f.main) && !pick(all.clone()).iter().any(|f| f.main) {
        return Err("This app's package is protected (a paid or DRM app): only the Store can install it".into());
    }
    let mut chosen = pick(all);
    if !chosen.iter().any(|f| f.main) {
        return Err("Microsoft's update service has no package of this app for this PC".into());
    }
    mark_installed(&mut chosen);
    Ok(App { title, family: family.to_string(), files: chosen })
}

/// The links of the chosen files (they work for a while; FE3 makes new ones each time).
pub fn links(app: &mut App) -> Result<(), String> {
    for f in app.files.iter_mut().filter(|f| !f.have) {
        f.url = Some(file_url(&f.update, &f.sha1)?);
    }
    Ok(())
}

// ------------------------------------------------------------------------------------- jobs

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Job {
    pub id: String,
    pub title: String,
    /// install (download, then install) or save (download to Downloads\Store apps)
    pub mode: String,
    /// finding, downloading, installing, done, failed, cancelled
    pub phase: String,
    pub file: String,
    pub done: u64,
    pub total: u64,
    pub speed: f64,
    pub eta: Option<u64>,
    pub error: Option<String>,
    pub saved_to: Option<String>,
    pub files: Vec<PkgFile>,
}

static JOBS: Mutex<Vec<Job>> = parking_lot::const_mutex(Vec::new());
static CANCEL: Mutex<Vec<(String, Arc<AtomicBool>)>> = parking_lot::const_mutex(Vec::new());

fn update(id: &str, f: impl FnOnce(&mut Job)) {
    let mut changed_phase = false;
    if let Some(j) = loaded().iter_mut().find(|j| j.id == id) {
        let before = j.phase.clone();
        f(j);
        changed_phase = j.phase != before;
    }
    save_jobs(changed_phase);
}

fn jobs_file() -> std::path::PathBuf {
    crate::util::local_appdata().join("Deck").join("store").join("jobs.json")
}

/// The job list, read from disk the first time (after Deck starts).
fn loaded() -> parking_lot::MutexGuard<'static, Vec<Job>> {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let mut saved: Vec<Job> = std::fs::read(jobs_file()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        for j in saved.iter_mut().filter(|j| ACTIVE.contains(&j.phase.as_str())) {
            j.phase = "interrupted".into();
            j.speed = 0.0;
            j.eta = None;
        }
        *JOBS.lock() = saved;
    });
    JOBS.lock()
}

const ACTIVE: &[&str] = &["finding", "downloading", "installing"];

fn save_jobs(now: bool) {
    static LAST: Mutex<Option<Instant>> = parking_lot::const_mutex(None);
    {
        let mut last = LAST.lock();
        if !now && last.is_some_and(|t| t.elapsed() < Duration::from_secs(2)) {
            return;
        }
        *last = Some(Instant::now());
    }
    let list = JOBS.lock().clone();
    let path = jobs_file();
    let _ = std::fs::create_dir_all(path.parent().unwrap());
    let _ = crate::util::write_atomic(&path, &serde_json::to_vec(&list).unwrap_or_default());
}

pub fn jobs() -> Vec<Job> {
    loaded().clone()
}

pub fn cancel(id: &str) {
    if let Some((_, c)) = CANCEL.lock().iter().find(|(i, _)| i == id) {
        c.store(true, Ordering::SeqCst);
    }
}

pub fn clear_finished() {
    loaded().retain(|j| !matches!(j.phase.as_str(), "done" | "failed" | "cancelled" | "interrupted"));
    save_jobs(true);
}

/// Starts downloading (and installing) Store app `id` in Deck. mode: "install" or "save".
pub fn start(id: &str, mode: &str) -> Result<(), String> {
    if !valid_id(id) {
        return Err("bad Store id".into());
    }
    if loaded().iter().any(|j| j.id == id && ACTIVE.contains(&j.phase.as_str())) {
        return Err("Already downloading".into());
    }
    let stop = Arc::new(AtomicBool::new(false));
    {
        let mut jobs = loaded();
        jobs.retain(|j| j.id != id);
        jobs.push(Job { id: id.into(), mode: mode.into(), phase: "finding".into(), title: id.into(), ..Default::default() });
        CANCEL.lock().retain(|(i, _)| i != id);
        CANCEL.lock().push((id.into(), stop.clone()));
    }
    save_jobs(true);
    let (id, mode) = (id.to_string(), mode.to_string());
    std::thread::Builder::new()
        .name("store-download".into())
        .spawn(move || {
            let op = crate::log::Op::start("store apps", format!("{mode} {id}"));
            let r = run(&id, &mode, &stop);
            match &r {
                Ok(()) => op.ok(""),
                Err(e) if stop.load(Ordering::SeqCst) => op.skipped(format!("cancelled ({e})")),
                Err(e) => op.fail(e),
            }
            update(&id, |j| match r {
                Ok(()) => j.phase = "done".into(),
                Err(e) if stop.load(Ordering::SeqCst) => {
                    j.phase = "cancelled".into();
                    let _ = e;
                }
                Err(e) => {
                    j.phase = "failed".into();
                    j.error = Some(e);
                }
            });
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn run(id: &str, mode: &str, stop: &AtomicBool) -> Result<(), String> {
    if crate::settings::is_test_profile() {
        return Ok(());
    }
    let mut app = find(id).map_err(|e| if e == "NOT_PACKAGED" { "This is a desktop app with its own installer: use Install (winget) instead".to_string() } else { e })?;
    links(&mut app)?;
    // only what this PC is missing (usually just the app itself)
    let needed: Vec<PkgFile> = app.files.iter().filter(|f| !f.have).cloned().collect();
    let total: u64 = needed.iter().map(|f| f.size).sum();
    update(id, |j| {
        j.title = app.title.clone();
        j.files = app.files.clone();
        j.total = total;
        j.phase = "downloading".into();
    });
    let dir = if mode == "save" {
        crate::util::downloads().join("Store apps").join(safe(&app.title))
    } else {
        crate::util::local_appdata().join("Deck").join("store").join(id)
    };
    // a folder of yours, written in with your rights only (Deck may be admin; see fsguard)
    let d = crate::fsguard::open_for_user(&dir).map_err(|e| format!("Couldn't use {}: {e}", dir.display()))?;
    let (mut done_before, mut fresh_before) = (0u64, 0u64);
    let started = Instant::now();
    let mut paths = Vec::new();
    for f in &needed {
        update(id, |j| j.file = f.name.clone());
        fresh_before += download(f, &d, stop, |got, fresh| {
            let done = done_before + got;
            // the speed of this run (bytes kept from before a cancel don't count)
            let secs = started.elapsed().as_secs_f64().max(0.5);
            let speed = (fresh_before + fresh) as f64 / secs;
            update(id, |j| {
                j.done = done;
                j.speed = speed;
                j.eta = (speed > 1.0).then(|| ((total.saturating_sub(done)) as f64 / speed) as u64);
            });
        })?;
        done_before += f.size;
        paths.push((f.main, f.name.clone()));
    }
    if mode == "save" {
        update(id, |j| j.saved_to = Some(dir.display().to_string()));
        return Ok(());
    }
    update(id, |j| {
        j.phase = "installing".into();
        j.eta = None;
    });
    for (main, name) in &paths {
        install(&d.real().join(name), *main)?;
    }
    for (_, name) in &paths {
        let _ = d.remove(name);
    }
    drop(d);
    let _ = std::fs::remove_dir(&dir);
    crate::log::info(format!("tweaks: installed {} ({id}) from Microsoft's servers", app.title));
    Ok(())
}

fn safe(s: &str) -> String {
    let t: String = s.chars().map(|c| if "<>:\"/\\|?*".contains(c) || c.is_control() { '_' } else { c }).collect();
    t.trim().trim_end_matches('.').chars().take(80).collect::<String>()
}

/// How many parts of one file download at once. Microsoft's CDN gives each connection only so much
/// (about 190 KB/s on a slow line); eight together were six times faster, as FluxDM does it.
const PARTS: u64 = 8;
/// Smaller files take fewer parts.
const MIN_PART: u64 = 4 << 20;
/// A part that fails this many times in a row (without getting anything) stops the download.
const TRIES: u32 = 5;
const NO_RANGES: &str = "the server doesn't send parts of files";

#[derive(Serialize, serde::Deserialize, Clone, Copy, Debug, PartialEq)]
struct Seg {
    start: u64,
    end: u64,
    done: u64,
}

fn segments(size: u64, parts: u64) -> Vec<Seg> {
    let n = (size / MIN_PART).clamp(1, parts.max(1));
    let each = size.div_ceil(n);
    (0..n).map(|i| Seg { start: i * each, end: ((i + 1) * each).min(size), done: 0 }).filter(|s| s.end > s.start).collect()
}

/// A saved part map belongs to a file of `size` when its parts cover it end to end.
fn map_fits(segs: &[Seg], size: u64) -> bool {
    !segs.is_empty() && segs[0].start == 0 && segs.last().is_some_and(|s| s.end == size) && segs.windows(2).all(|w| w[0].end == w[1].start) && segs.iter().all(|s| s.start + s.done <= s.end)
}

/// Downloads one file in parts at once (resuming each part after a cancel or a dropped connection),
/// then checks its SHA-256 (or SHA-1) against FE3's. Returns the bytes this call downloaded.
fn download(f: &PkgFile, d: &crate::fsguard::Dir, stop: &AtomicBool, progress: impl Fn(u64, u64)) -> Result<u64, String> {
    use sha2::Digest;
    let url = f.url.as_deref().ok_or("no link")?;
    if f.size == 0 {
        return Err(format!("Microsoft's update service didn't give the size of {}", f.name));
    }
    let (part, map) = (format!("{}.part", f.name), format!("{}.parts", f.name));
    let read_all = |name: &str| -> Option<Vec<u8>> {
        let mut b = Vec::new();
        d.edit(name).ok()?.read_to_end(&mut b).ok()?;
        Some(b)
    };
    // what an earlier run got: only a part file of exactly this size with its map
    let saved: Option<Vec<Seg>> = read_all(&map).and_then(|b| serde_json::from_slice(&b).ok()).filter(|v: &Vec<Seg>| map_fits(v, f.size) && d.len(&part).is_ok_and(|n| n == f.size));
    let mut segs = saved.unwrap_or_else(|| segments(f.size, PARTS));
    let mut fresh = 0u64;
    for attempt in 0..2 {
        let resuming = segs.iter().any(|s| s.done > 0);
        let file = if resuming { d.edit(&part) } else { d.create(&part, true) }.map_err(|e| e.to_string())?;
        file.set_len(f.size).map_err(|e| e.to_string())?;
        let resumed: u64 = segs.iter().map(|s| s.done).sum();
        let shared = Mutex::new(segs.clone());
        let failed: Mutex<Option<String>> = Mutex::new(None);
        let save_map = |v: &[Seg]| {
            if let Ok(mut m) = d.create(&map, true) {
                let _ = std::io::Write::write_all(&mut m, &serde_json::to_vec(v).unwrap_or_default());
            }
        };
        save_map(&segs);
        std::thread::scope(|sc| {
            let (file, shared, failed) = (&file, &shared, &failed);
            let workers: Vec<_> = (0..segs.len()).map(|i| sc.spawn(move || part_worker(url, file, shared, i, stop, failed))).collect();
            let mut saved_at = Instant::now();
            while workers.iter().any(|w| !w.is_finished()) {
                std::thread::sleep(Duration::from_millis(250));
                let v = shared.lock().clone();
                let got: u64 = v.iter().map(|s| s.done).sum();
                progress(got, fresh + got - resumed);
                if saved_at.elapsed() > Duration::from_secs(2) {
                    save_map(&v);
                    saved_at = Instant::now();
                }
            }
        });
        segs = shared.into_inner();
        save_map(&segs);
        let got: u64 = segs.iter().map(|s| s.done).sum();
        fresh += got - resumed;
        progress(got, fresh);
        drop(file);
        if stop.load(Ordering::SeqCst) {
            return Err("cancelled".into());
        }
        match failed.into_inner() {
            // a server that ignores ranges: once more, in one piece from the start
            Some(e) if e == NO_RANGES && attempt == 0 && segs.len() > 1 => {
                segs = segments(f.size, 1);
                continue;
            }
            Some(e) => return Err(format!("Download stopped: {e} (Install again carries on where it stopped)")),
            None => break,
        }
    }
    // the links are plain http: the file must be exactly what FE3 listed
    let mut file = d.edit(&part).map_err(|e| e.to_string())?;
    let (mut h256, mut h1) = (sha2::Sha256::new(), sha1::Sha1::new());
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = file.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        h256.update(&buf[..n]);
        h1.update(&buf[..n]);
    }
    drop(file);
    let _ = d.remove(&map);
    let ok = match &f.sha256 {
        Some(want) => crate::util::base64(&h256.finalize()) == *want,
        None => crate::util::base64(&h1.finalize()) == f.sha1,
    };
    if !ok {
        let _ = d.remove(&part);
        return Err(format!("{} didn't arrive intact (its checksum is wrong); try again", f.name));
    }
    d.rename(&part, &f.name, true).map_err(|e| e.to_string())?;
    Ok(fresh)
}

/// Downloads part `i` into its place in the file, trying again after a dropped connection.
fn part_worker(url: &str, file: &std::fs::File, segs: &Mutex<Vec<Seg>>, i: usize, stop: &AtomicBool, failed: &Mutex<Option<String>>) {
    use std::os::windows::fs::FileExt;
    let mut tries = 0;
    let mut buf = vec![0u8; 128 * 1024];
    loop {
        let s = segs.lock()[i];
        let mut pos = s.start + s.done;
        if pos >= s.end || stop.load(Ordering::SeqCst) || failed.lock().is_some() {
            return;
        }
        let before = pos;
        let r = (|| -> Result<(), String> {
            let resp = agent().get(url).set("Range", &format!("bytes={pos}-{}", s.end - 1)).call().map_err(|e| e.to_string())?;
            // a whole file (200) is fine only for a part that starts at the beginning
            if resp.status() != 206 && pos != 0 {
                return Err(NO_RANGES.into());
            }
            let mut rd = resp.into_reader();
            while pos < s.end {
                if stop.load(Ordering::SeqCst) {
                    return Ok(());
                }
                let n = rd.read(&mut buf).map_err(|e| e.to_string())?;
                if n == 0 {
                    return Err("the connection closed early".into());
                }
                let n = n.min((s.end - pos) as usize);
                let mut w = 0;
                while w < n {
                    w += file.seek_write(&buf[w..n], pos + w as u64).map_err(|e| e.to_string())?;
                }
                pos += n as u64;
                segs.lock()[i].done = pos - s.start;
            }
            Ok(())
        })();
        if let Err(e) = r {
            if e == NO_RANGES {
                failed.lock().get_or_insert(e);
                return;
            }
            // only tries that got nothing count
            tries = if pos > before { 1 } else { tries + 1 };
            if tries >= TRIES {
                failed.lock().get_or_insert(e);
                return;
            }
            for _ in 0..(tries * 8) {
                if stop.load(Ordering::SeqCst) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(250));
            }
        }
    }
}

/// Installs a package for you. A dependency that's already there in this or a newer version is fine.
fn install(path: &std::path::Path, main: bool) -> Result<(), String> {
    // quoted for PowerShell (a user folder can be "O'Brien")
    let p = crate::rdesk::ps_str(&path.display().to_string());
    let o = crate::tweaks::run(900, "powershell.exe", &["-NoProfile", "-NonInteractive", "-Command", &format!("Add-AppxPackage -Path {p} -ForceUpdateFromAnyVersion")]).map_err(|e| e.to_string())?;
    if o.status.success() {
        return Ok(());
    }
    let e = String::from_utf8_lossy(&o.stderr).to_string();
    // 0x80073D06: a higher version is already installed
    if !main && (e.contains("0x80073D06") || e.contains("higher version")) {
        return Ok(());
    }
    Err(format!("Windows didn't install {}: {}", path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(), e.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim()))
}

/// The links for copying or FluxDM (each with the file's real name; FE3's links have none).
pub fn link_list(id: &str) -> Result<Value, String> {
    let mut app = find(id).map_err(|e| if e == "NOT_PACKAGED" { "This is a desktop app with its own installer: it has no package files to download".to_string() } else { e })?;
    links(&mut app)?;
    Ok(json!({ "title": app.title, "family": app.family, "files": app.files }))
}

/// Sends the files to FluxDM (its local API opens its Add dialog with the link and the name).
pub fn to_fluxdm(id: &str) -> Result<usize, String> {
    let list = link_list(id)?;
    // only what this PC is missing (usually just the app itself)
    let files: Vec<Value> = list["files"].as_array().cloned().unwrap_or_default().into_iter().filter(|f| f["have"] != true).collect();
    let mut n = 0;
    for f in &files {
        let (Some(url), Some(name)) = (f["url"].as_str(), f["name"].as_str()) else { continue };
        crate::log::http_agent().post("http://127.0.0.1:7878/api/add")
            .timeout(Duration::from_secs(5))
            .send_json(json!({ "urls": [url], "fileName": name }))
            .map_err(|_| "FluxDM isn't running (or its browser bridge is off in its settings)".to_string())?;
        n += 1;
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(code: u16) -> Box<ureq::Error> {
        Box::new(ureq::Error::Status(code, ureq::Response::new(code, "x", "").unwrap()))
    }

    /// 2026-10-23: 24 apps came back "status code 500" in one check; they were fine a minute later.
    #[test]
    fn a_server_error_is_tried_again_and_a_refusal_is_not() {
        let mut calls = 0;
        let r = retrying("Microsoft's update service", || {
            calls += 1;
            if calls < 3 { Err(status(500)) } else { Ok(calls) }
        });
        assert_eq!(r, Ok(3), "two 500s, then it answers");

        let mut calls = 0;
        let r: Result<(), String> = retrying("Microsoft's update service", || {
            calls += 1;
            Err(status(404))
        });
        assert_eq!(calls, 1, "a 404 is an answer: no second try");
        assert!(r.unwrap_err().contains("status code 404"));

        let mut calls = 0;
        let e: String = retrying("Microsoft's update service", || -> Result<(), Box<ureq::Error>> {
            calls += 1;
            Err(status(500))
        })
        .unwrap_err();
        assert_eq!(calls, REQUEST_TRIES);
        assert!(e.contains("status code 500") && e.contains(&format!("after {REQUEST_TRIES} tries")) && e.contains("Microsoft's side"), "{e}");
        assert!(their_trouble(&e), "so the app is asked about again later");
        assert!(!their_trouble("NOT_PACKAGED") && !their_trouble("This app's package is protected (a paid or DRM app)"));
    }

    /// Runs the real update check of this PC's Store apps and prints what came back (needs the internet).
    /// `cargo test store_update_check_here -- --ignored --nocapture`
    #[test]
    #[ignore = "asks Microsoft's servers about every Store app on this PC"]
    fn store_update_check_here() {
        let t = std::time::Instant::now();
        check_updates().unwrap();
        std::thread::sleep(Duration::from_millis(500));
        while update_state().checking {
            std::thread::sleep(Duration::from_millis(500));
        }
        let c = update_state();
        println!("{} apps in {:.0}s: {} updates, {} store-only, {} not in the Store, {} failed", c.total, t.elapsed().as_secs_f32(), c.updates.len(), c.store_only.len(), c.not_in_store, c.failed.len());
        for (n, why) in &c.failed {
            println!("FAILED {n}: {why}");
        }
        for u in &c.updates {
            println!("UPDATE {} {} -> {}", u.title, u.installed, u.latest);
        }
    }

    #[test]
    fn parts_cover_the_file_end_to_end() {
        for size in [1u64, 100, MIN_PART - 1, MIN_PART, MIN_PART * 3 + 7, 311_000_000, 1 << 40] {
            let v = segments(size, PARTS);
            assert!(map_fits(&v, size), "{size}: {v:?}");
            assert!(v.len() as u64 <= PARTS && !v.is_empty());
        }
        assert_eq!(segments(311_000_000, PARTS).len(), 8);
        assert_eq!(segments(MIN_PART * 3 + 7, PARTS).len(), 3);
        assert_eq!(segments(311_000_000, 1).len(), 1);
        let mut v = segments(MIN_PART * 4, PARTS);
        assert!(!map_fits(&v, MIN_PART * 4 + 1), "another size");
        v[1].done = v[1].end - v[1].start + 1;
        assert!(!map_fits(&v, MIN_PART * 4), "a part past its end");
    }

    /// Live: downloads the smallest file of a Store app (DECK_STORE_IDS, default WhatsApp) that takes
    /// all the parts, cancels it a third of the way, carries on, and checks the checksum. Nothing is
    /// installed.
    #[test]
    #[ignore]
    fn downloads_in_parts_and_carries_on_after_a_cancel() {
        let id = std::env::var("DECK_STORE_IDS").unwrap_or("9NKSQGP7F2NH".into());
        let mut app = find(&id).unwrap();
        app.files.retain(|f| f.size >= PARTS * MIN_PART);
        app.files.sort_by_key(|f| f.size);
        app.files.truncate(1);
        app.files[0].have = false;
        links(&mut app).unwrap();
        let f = app.files[0].clone();
        let dir = std::env::temp_dir().join(format!("deck-storedl-test-{}", std::process::id()));
        let d = crate::fsguard::open_for_user(&dir).unwrap();
        let path = dir.join(&f.name);
        let t = Instant::now();
        let stop = AtomicBool::new(false);
        let r = download(&f, &d, &stop, |got, _| {
            if got > f.size / 3 {
                stop.store(true, Ordering::SeqCst);
            }
        });
        assert_eq!(r, Err("cancelled".into()));
        let half: Vec<Seg> = serde_json::from_slice(&std::fs::read(dir.join(format!("{}.parts", f.name))).unwrap()).unwrap();
        println!("cancelled after {:.1} s with {} of {} MB in {} parts", t.elapsed().as_secs_f64(), half.iter().map(|s| s.done).sum::<u64>() / 1_000_000, f.size / 1_000_000, half.len());
        let stop = AtomicBool::new(false);
        let fresh = download(&f, &d, &stop, |_, _| {}).unwrap();
        println!("done after {:.1} s ({} MB this time, {:.0} KB/s overall)", t.elapsed().as_secs_f64(), fresh / 1_000_000, f.size as f64 / 1000.0 / t.elapsed().as_secs_f64());
        assert!(fresh < f.size, "carried on rather than started again");
        assert_eq!(std::fs::metadata(&path).unwrap().len(), f.size);
        assert!(!dir.join(format!("{}.parts", f.name)).exists() && !dir.join(format!("{}.part", f.name)).exists());
        drop(d);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn picks_the_newest_package_for_this_pc_and_a_bundle_for_the_app() {
        let f = |name: &str, pkg: &str, ver: &str, arch: &str, main: bool| PkgFile {
            name: name.into(), package: pkg.into(), version: ver.into(), arch: arch.into(), size: 1, main, sha1: String::new(), sha256: None, update: (String::new(), String::new()), url: None, have: false,
        };
        let v = pick(vec![
            f("App_1.0.0.0_x64__p.appx", "App", "1.0.0.0", "x64", true),
            f("App_2.0.0.0_neutral_~_p.msixbundle", "App", "2.0.0.0", "neutral", true),
            f("App_3.0.0.0_arm__p.appx", "App", "3.0.0.0", "arm", true),
            f("Microsoft.VCLibs.140.00_14.0.1_x86__8we.appx", "Microsoft.VCLibs.140.00", "14.0.1", "x86", false),
            f("Microsoft.VCLibs.140.00_14.0.1_x64__8we.appx", "Microsoft.VCLibs.140.00", "14.0.1", "x64", false),
            f("Microsoft.VCLibs.140.00_14.0.0_x64__8we.appx", "Microsoft.VCLibs.140.00", "14.0.0", "x64", false),
            f("Paid_1.0.0.0_x64__p.eappx", "Paid", "1.0.0.0", "x64", false),
        ]);
        let names: Vec<&str> = v.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["Microsoft.VCLibs.140.00_14.0.1_x64__8we.appx", "App_2.0.0.0_neutral_~_p.msixbundle"]);
    }

    /// Live: asks Microsoft's servers for two free apps' files and links (nothing is downloaded).
    #[test]
    #[ignore]
    fn finds_real_apps_and_their_links() {
        let ids = std::env::var("DECK_STORE_IDS").unwrap_or("9NCBCSZSJRSB,9NBLGGH5R558".into());
        for id in ids.split(',') {
            let mut app = find(id).unwrap();
            links(&mut app).unwrap();
            println!("{} ({})", app.title, app.family);
            for f in &app.files {
                println!("  {} {} MB main={} have={} sha256={} url={}", f.name, f.size / 1_000_000, f.main, f.have, f.sha256.is_some(), f.url.as_deref().unwrap_or(""));
            }
        }
    }
}

// ------------------------------------------------------------------------------------- updates

/// A Store app of yours with a newer version on Microsoft's servers.
#[derive(Serialize, Deserialize, Clone)]
pub struct Update {
    pub id: String,
    pub title: String,
    pub family: String,
    pub installed: String,
    pub latest: String,
    /// what the update downloads (the app, and dependencies this PC lacks)
    pub size: u64,
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
pub struct UpdateCheck {
    pub checking: bool,
    pub done: usize,
    pub total: usize,
    pub updates: Vec<Update>,
    /// apps only the Store can check (paid or protected ones)
    pub store_only: Vec<String>,
    /// apps Microsoft's servers didn't answer for, also after Deck tried again: (name, why)
    pub failed: Vec<(String, String)>,
    /// how many apps Microsoft turned away are being asked about again, after the first round
    pub retrying: usize,
    /// apps that aren't in the Store's catalog (installed some other way)
    pub not_in_store: usize,
    pub at: u64,
    pub error: Option<String>,
}

static CHECK: Mutex<Option<UpdateCheck>> = parking_lot::const_mutex(None);
/// package family → Store id (None: not in the Store's catalog), kept while Deck runs
static IDS: std::sync::LazyLock<Mutex<HashMap<String, Option<String>>>> = std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

fn check_file() -> std::path::PathBuf {
    crate::util::local_appdata().join("Deck").join("store").join("updates.json")
}

pub fn update_state() -> UpdateCheck {
    let mut c = CHECK.lock();
    if c.is_none() {
        // the last check, from before Deck restarted
        *c = std::fs::read(check_file()).ok().and_then(|b| serde_json::from_slice::<UpdateCheck>(&b).ok()).map(|mut u| {
            u.checking = false;
            u
        });
    }
    c.clone().unwrap_or_default()
}

/// Your apps from the Store: (package family, version). Frameworks and Windows' own parts aren't.
fn store_installed() -> Vec<(String, String)> {
    let Ok(o) = crate::tweaks::run(
        90,
        "powershell.exe",
        &[
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Get-AppxPackage -PackageTypeFilter Main | Where-Object { $_.SignatureKind -eq 'Store' -and -not $_.IsFramework } | Select-Object PackageFamilyName,@{n='V';e={$_.Version.ToString()}} | ConvertTo-Json -Compress",
        ],
    ) else {
        return Vec::new();
    };
    let v: Value = serde_json::from_slice(&o.stdout).unwrap_or(Value::Null);
    let list = match v {
        Value::Array(a) => a,
        o @ Value::Object(_) => vec![o],
        _ => vec![],
    };
    list.iter().filter_map(|p| Some((p["PackageFamilyName"].as_str()?.to_string(), p["V"].as_str()?.to_string()))).collect()
}

/// Starts checking every Store app of yours against Microsoft's servers (the same service the
/// Store asks), a few at a time. `update_state` shows how far it is.
pub fn check_updates() -> Result<(), String> {
    {
        let mut c = CHECK.lock();
        if c.as_ref().is_some_and(|c| c.checking) {
            return Ok(());
        }
        *c = Some(UpdateCheck { checking: true, ..Default::default() });
    }
    std::thread::Builder::new()
        .name("store-updates".into())
        .spawn(|| {
            let apps = if crate::settings::is_test_profile() { Vec::new() } else { store_installed() };
            if let Some(c) = CHECK.lock().as_mut() {
                c.total = apps.len();
            }
            let next = std::sync::atomic::AtomicUsize::new(0);
            // the apps Microsoft's servers couldn't answer for (their trouble, not the app's): asked again below
            let turned_away: Mutex<Vec<Turned>> = Mutex::new(Vec::new());
            // four at a time: six at once made Microsoft's service answer 500 to some
            std::thread::scope(|s| {
                for _ in 0..4 {
                    s.spawn(|| loop {
                        let i = next.fetch_add(1, Ordering::SeqCst);
                        let Some((family, version)) = apps.get(i) else { break };
                        if let Err(t) = check_one(family, version) {
                            turned_away.lock().push(t);
                        }
                        if let Some(c) = CHECK.lock().as_mut() {
                            c.done += 1;
                        }
                    });
                }
            });
            // those again, one at a time with a pause (twice at most), so a busy moment has passed
            let mut again = turned_away.into_inner();
            for round in 0..2 {
                if again.is_empty() {
                    break;
                }
                if let Some(c) = CHECK.lock().as_mut() {
                    c.retrying = again.len();
                }
                std::thread::sleep(Duration::from_secs(if cfg!(test) { 0 } else { 4 + 6 * round }));
                let mut still = Vec::new();
                for t in again {
                    if let Err(t) = check_one(&t.family, &t.installed) {
                        still.push(t);
                    }
                    std::thread::sleep(Duration::from_millis(if cfg!(test) { 0 } else { 700 }));
                }
                again = still;
            }
            if let Some(c) = CHECK.lock().as_mut() {
                c.retrying = 0;
                c.failed.extend(again.into_iter().map(|t| (t.short, t.why)));
                c.checking = false;
                c.at = crate::util::now_s();
                c.updates.sort_by_key(|u| u.title.to_lowercase());
                let path = check_file();
                let _ = std::fs::create_dir_all(path.parent().unwrap());
                let _ = crate::util::write_atomic(&path, &serde_json::to_vec(&*c).unwrap_or_default());
                crate::log::info(format!("tweaks: Store update check: {} of {} apps have updates", c.updates.len(), c.total));
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// An app Microsoft's servers couldn't answer for (so it can be asked about again).
struct Turned {
    family: String,
    installed: String,
    short: String,
    why: String,
}

/// An answer that is the servers' own trouble or silence, which a later try may get past.
fn their_trouble(e: &str) -> bool {
    e.contains("didn't answer")
}

/// Checks one app. `Err`: Microsoft's servers didn't answer for it, even after the tries of one request.
fn check_one(family: &str, installed: &str) -> Result<(), Turned> {
    let short = family.rsplit_once('_').map(|(n, _)| n).unwrap_or(family).to_string();
    let turned = |why: String| Turned { family: family.into(), installed: installed.into(), short: short.clone(), why };
    let cached = IDS.lock().get(family).cloned();
    let id = match cached {
        Some(id) => id,
        None => match super::apps::product_lookup(family) {
            Ok(id) => {
                let id = id.filter(|i| valid_id(i));
                IDS.lock().insert(family.to_string(), id.clone());
                id
            }
            Err(e) if their_trouble(&e) => return Err(turned(e)),
            Err(e) => {
                if let Some(c) = CHECK.lock().as_mut() {
                    c.failed.push((short, e));
                }
                return Ok(());
            }
        },
    };
    // not in the catalog (sideloaded, or a part of Windows the Store updates itself)
    let Some(id) = id else {
        if let Some(c) = CHECK.lock().as_mut() {
            c.not_in_store += 1;
        }
        return Ok(());
    };
    match find(&id) {
        Ok(app) => {
            let Some(main) = app.files.iter().find(|f| f.main) else { return Ok(()) };
            if version_key(&main.version) > version_key(installed) {
                let size = app.files.iter().filter(|f| !f.have).map(|f| f.size).sum();
                if let Some(c) = CHECK.lock().as_mut() {
                    c.updates.push(Update { id, title: app.title, family: family.into(), installed: installed.into(), latest: main.version.clone(), size });
                }
            }
        }
        Err(e) if e.contains("protected") => {
            if let Some(c) = CHECK.lock().as_mut() {
                c.store_only.push(short);
            }
        }
        Err(e) if their_trouble(&e) => return Err(turned(e)),
        Err(e) => {
            if let Some(c) = CHECK.lock().as_mut() {
                c.failed.push((short, e));
            }
        }
    }
    Ok(())
}

/// The Microsoft Store way: Windows' own "check for updates" (as Store → Downloads → Get updates
/// does), which then updates apps in the background, paid ones included. Needs admin rights.
/// The Store only reports into its event log, so Deck reads that afterwards and says what the
/// Store really did: which apps it started, and which it cancelled.
pub fn store_update_all() -> Result<Value, String> {
    if !crate::elevation::is_admin() {
        return Err("Deck needs admin rights for this. Open the Store's Downloads page and press \"Get updates\" instead.".into());
    }
    // "Don't update Store apps by themselves" (Tweaks → Updates) is a policy the background update obeys
    let policy = crate::util::run_for(20, "reg.exe", &["query", r"HKLM\SOFTWARE\Policies\Microsoft\WindowsStore", "/v", "AutoDownload"])
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("0x2"))
        .unwrap_or(false);
    if policy {
        return Ok(json!({ "blocked_by_policy": true }));
    }
    let since = chrono_like_now();
    let o = crate::tweaks::run(
        120,
        "powershell.exe",
        &[
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Get-CimInstance -Namespace 'root\\cimv2\\mdm\\dmmap' -ClassName 'MDM_EnterpriseModernAppManagement_AppManagement01' -ErrorAction Stop | Invoke-CimMethod -MethodName UpdateScanMethod -ErrorAction Stop | Out-Null",
        ],
    )
    .map_err(|e| e.to_string())?;
    if !o.status.success() {
        return Err(format!("Windows didn't start the Store's update: {}", String::from_utf8_lossy(&o.stderr).lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim()));
    }
    // the Store decides within a few seconds whether it goes ahead with each app
    std::thread::sleep(Duration::from_secs(12));
    let r = store_log_since(&since);
    crate::log::info(format!("tweaks: asked the Store to update every app: {r}"));
    Ok(r)
}

/// Local time as the event log filter wants it.
fn chrono_like_now() -> String {
    crate::util::run_for(10, "powershell.exe", &["-NoProfile", "-NonInteractive", "-Command", "(Get-Date).AddSeconds(-2).ToString('o')"])
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

/// What the Store logged since `since`: the apps it worked on and the ones it cancelled.
fn store_log_since(since: &str) -> Value {
    if since.is_empty() || since.contains('\'') {
        return json!({ "known": false });
    }
    let script = format!(
        "Get-WinEvent -FilterHashtable @{{LogName='Microsoft-Windows-Store/Operational'; StartTime=[datetime]'{since}'}} -ErrorAction SilentlyContinue | ForEach-Object {{ $_.Message }} | ConvertTo-Json -Compress"
    );
    let Ok(o) = crate::util::run_for(30, "powershell.exe", &["-NoProfile", "-NonInteractive", "-Command", &script]) else { return json!({ "known": false }) };
    let msgs: Vec<String> = match serde_json::from_slice::<Value>(&o.stdout).unwrap_or(Value::Null) {
        Value::Array(a) => a.into_iter().filter_map(|m| m.as_str().map(String::from)).collect(),
        Value::String(s) => vec![s],
        _ => vec![],
    };
    // "PFN = Microsoft.GamingApp_8wekyb3d8bbwe" → "Microsoft.GamingApp"
    let pfn = |m: &str| m.split("PFN = ").nth(1).and_then(|r| r.split_whitespace().next()).map(|p| p.rsplit_once('_').map(|(n, _)| n).unwrap_or(p).to_string());
    let mut worked = std::collections::BTreeSet::new();
    let mut cancelled = std::collections::BTreeSet::new();
    for m in &msgs {
        let Some(p) = pfn(m) else { continue };
        if m.contains("AbortedInstallation") || m.contains("NewState = BlockedOnUser") || m.contains("NewState = Error") {
            cancelled.insert(p);
        } else if m.contains("StateTransition") {
            worked.insert(p);
        }
    }
    let started: Vec<&String> = worked.iter().filter(|p| !cancelled.contains(*p)).collect();
    json!({ "known": true, "started": started, "cancelled": cancelled })
}
