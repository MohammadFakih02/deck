//! Built-in apps (Tweaks → Built-in apps): Win11Debloat's list with its safe / optional / unsafe
//! rating, only the ones installed for you. Removing is for your account (Remove-AppxPackage, as
//! Win11Debloat does); each removed app is remembered with its package family so the Store can
//! put it back (Reinstall opens its Store page). Edge isn't on the list: Deck runs on WebView2.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::{Duration, Instant};

const APPS: &str = include_str!("../../tweaks/apps.json");

#[derive(Deserialize, Clone)]
struct App {
    name: String,
    ids: Vec<String>,
    what: String,
    rating: String,
    method: String,
}

#[derive(Deserialize)]
struct Data {
    apps: Vec<App>,
}

fn list() -> &'static [App] {
    static D: std::sync::OnceLock<Data> = std::sync::OnceLock::new();
    &D.get_or_init(|| serde_json::from_str(APPS).expect("apps.json")).apps
}

/// An app Deck removed.
#[derive(Serialize, Deserialize, Clone)]
pub struct Removed {
    pub name: String,
    pub ids: Vec<String>,
    /// the Store's package family name ("Clipchamp.Clipchamp_yxz26nhyzhsrt"), for reinstalling
    pub family: Option<String>,
    pub at: u64,
}

#[derive(Deserialize, Clone)]
#[serde(rename_all = "PascalCase")]
struct Pkg {
    name: String,
    package_full_name: String,
    package_family_name: String,
}

static CACHE: parking_lot::Mutex<Option<(Instant, Vec<Pkg>)>> = parking_lot::const_mutex(None);

/// Your installed Store packages (asked at most every 30 s; a removal clears it).
fn packages() -> Vec<Pkg> {
    if let Some((t, v)) = CACHE.lock().as_ref() {
        if t.elapsed() < Duration::from_secs(30) {
            return v.clone();
        }
    }
    if crate::settings::is_test_profile() {
        return Vec::new();
    }
    let o = crate::tweaks::run(60, 
        "powershell.exe",
        &["-NoProfile", "-NonInteractive", "-Command", "Get-AppxPackage -PackageTypeFilter Main | Select-Object Name,PackageFullName,PackageFamilyName | ConvertTo-Json -Compress"],
    );
    let v: Vec<Pkg> = o
        .ok()
        .and_then(|o| {
            let t = String::from_utf8_lossy(&o.stdout).to_string();
            serde_json::from_str::<Vec<Pkg>>(&t).ok().or_else(|| serde_json::from_str::<Pkg>(&t).ok().map(|p| vec![p]))
        })
        .unwrap_or_default();
    *CACHE.lock() = Some((Instant::now(), v.clone()));
    v
}

fn onedrive_installed() -> bool {
    crate::util::local_appdata().join(r"Microsoft\OneDrive\OneDrive.exe").exists() || crate::util::program_files().join(r"Microsoft OneDrive\OneDrive.exe").exists()
}

fn installed_pkgs<'a>(a: &App, pkgs: &'a [Pkg]) -> Vec<&'a Pkg> {
    pkgs.iter().filter(|p| a.ids.iter().any(|id| id.eq_ignore_ascii_case(&p.name))).collect()
}

fn is_installed(a: &App, pkgs: &[Pkg]) -> bool {
    if a.method == "WinGet" {
        return a.ids.iter().any(|i| i.eq_ignore_ascii_case("Microsoft.OneDrive")) && onedrive_installed();
    }
    !installed_pkgs(a, pkgs).is_empty()
}

/// The apps installed now, and the ones Deck removed (with whether they're back).
pub fn state() -> Value {
    let pkgs = packages();
    let removed = super::with_journal(|j| {
        // reinstalled since: no longer "removed"
        j.removed_apps.retain(|r| !list().iter().any(|a| a.name == r.name && is_installed(a, &pkgs)));
        j.removed_apps.clone()
    });
    let apps: Vec<Value> = list()
        .iter()
        .filter(|a| a.method != "WinGet" || a.ids.iter().any(|i| i.eq_ignore_ascii_case("Microsoft.OneDrive")))
        .map(|a| {
            let installed = is_installed(a, &pkgs);
            json!({ "name": a.name, "what": a.what, "rating": a.rating, "installed": installed, "removed": removed.iter().any(|r| r.name == a.name) })
        })
        .filter(|v| v["installed"] == true || v["removed"] == true)
        .collect();
    json!({ "apps": apps, "checked": !crate::settings::is_test_profile() })
}

pub fn remove(app: &crate::app::App, name: &str) -> Result<(), String> {
    let a = list().iter().find(|a| a.name == name).ok_or("unknown app")?;
    if let Some(g) = crate::boost::game_in_front(app) {
        return Err(format!("{g} is in front: Deck removes apps only outside games"));
    }
    let pkgs = packages();
    let family = installed_pkgs(a, &pkgs).first().map(|p| p.package_family_name.clone());
    if a.method == "WinGet" {
        // OneDrive: its own uninstaller (your files in the OneDrive folder stay on disk)
        let setup = [crate::util::system_dir().join("OneDriveSetup.exe"), crate::util::windows_dir().join(r"SysWOW64\OneDriveSetup.exe")].into_iter().find(|p| p.exists());
        let o = match setup {
            Some(s) => crate::tweaks::run(600, &s.display().to_string(), &["/uninstall"]),
            None => {
                let w = winget()?;
                crate::tweaks::run(600, &w, &["uninstall", "--id", "Microsoft.OneDrive", "-e", "--silent", "--accept-source-agreements"])
            }
        }
        .map_err(|e| e.to_string())?;
        if !o.status.success() && onedrive_installed() {
            return Err("OneDrive's uninstaller didn't finish".into());
        }
    } else {
        let full: Vec<String> = installed_pkgs(a, &pkgs).iter().map(|p| p.package_full_name.clone()).collect();
        if full.is_empty() {
            return Err("It isn't installed".into());
        }
        for f in &full {
            if f.contains('\'') {
                return Err("bad package name".into());
            }
            let o = crate::tweaks::run(300, "powershell.exe", &["-NoProfile", "-NonInteractive", "-Command", &format!("Remove-AppxPackage -Package '{f}'")]).map_err(|e| e.to_string())?;
            if !o.status.success() {
                return Err(format!("Windows didn't remove {name}: {}", String::from_utf8_lossy(&o.stderr).lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim()));
            }
        }
    }
    *CACHE.lock() = None;
    super::with_journal(|j| {
        j.removed_apps.retain(|r| r.name != name);
        j.removed_apps.push(Removed { name: name.into(), ids: a.ids.clone(), family, at: crate::util::now_s() });
    });
    super::save_journal();
    crate::log::info(format!("tweaks: removed the built-in app {name}"));
    Ok(())
}

/// Installs an app Deck removed again, from the Store's catalog without the Store app (winget);
/// when that can't be done, opens its Store page.
pub fn reinstall(name: &str) -> Result<(), String> {
    let a = list().iter().find(|a| a.name == name).ok_or("unknown app")?;
    let r = super::with_journal(|j| j.removed_apps.iter().find(|r| r.name == name).cloned());
    *CACHE.lock() = None;
    if a.method == "WinGet" {
        let w = winget()?;
        let o = crate::tweaks::run(1800, &w, &["install", "--id", "Microsoft.OneDrive", "-e", "--silent", "--accept-package-agreements", "--accept-source-agreements"]).map_err(|e| e.to_string())?;
        return if o.status.success() { Ok(()) } else { Err(winget_error(&o)) };
    }
    let family = r.and_then(|r| r.family).filter(|f| f.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c)));
    // straight from Microsoft's servers (no Store needed); its progress shows under Get Store apps
    if let Some(id) = family.as_deref().and_then(product_for_family) {
        if super::storedl::start(&id, "install").is_ok() {
            return Ok(());
        }
    }
    let url = match family {
        Some(f) => format!("ms-windows-store://pdp/?PFN={f}"),
        None => format!("ms-windows-store://search/?query={}", name.replace(' ', "%20")),
    };
    crate::util::launch_as_user(&url, "", "")
}

// ------------------------------------------------------------------------------------- the Store's catalog

fn winget() -> Result<String, String> {
    crate::beam::apps::winget_exe().map(|p| p.display().to_string()).ok_or_else(|| "winget (App Installer) isn't installed: it comes with Windows; install \"App Installer\" once".to_string())
}

fn winget_error(o: &std::process::Output) -> String {
    let t = String::from_utf8_lossy(&o.stdout);
    let last = t.lines().map(str::trim).rfind(|l| !l.is_empty() && !l.chars().all(|c| "-|/ █▒".contains(c))).unwrap_or("").to_string();
    if last.is_empty() {
        format!("winget stopped (code {})", o.status.code().unwrap_or(-1))
    } else {
        last
    }
}

/// Searches the Store's catalog (Microsoft's own search, as the Store app uses).
pub fn store_search(query: &str) -> Result<Value, String> {
    let q = query.trim();
    if q.is_empty() {
        return Ok(json!({ "results": [] }));
    }
    let (market, loc) = super::storedl::market();
    let url = format!(
        "https://storeedgefd.dsx.mp.microsoft.com/v9.0/search?market={market}&locale={loc}&deviceFamily=windows.desktop&query={}",
        crate::remote::pct(q)
    );
    let v: Value = crate::log::http_agent().get(&url).timeout(Duration::from_secs(15)).call().map_err(|e| format!("The Store didn't answer: {e}"))?.into_json().map_err(|e| e.to_string())?;
    let results: Vec<Value> = v["Payload"]["SearchResults"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| {
                    let id = x["ProductId"].as_str()?;
                    // Store ids are 12 letters and digits; anything else isn't an app
                    (id.len() == 12 && id.chars().all(|c| c.is_ascii_alphanumeric())).then(|| {
                        json!({ "id": id, "title": x["Title"], "publisher": x["PublisherName"], "price": x["Price"], "display_price": x["DisplayPrice"],
                                "icon": x["Images"].as_array().and_then(|i| i.iter().find(|m| m["ImageType"] == "Logo" || m["ImageType"] == "Tile")).and_then(|m| m["Url"].as_str()) })
                    })
                })
                .take(24)
                .collect()
        })
        .unwrap_or_default();
    Ok(json!({ "results": results }))
}

/// The Store id of a package family (for reinstalling a removed app).
fn product_for_family(family: &str) -> Option<String> {
    let (market, loc) = super::storedl::market();
    let url = format!("https://displaycatalog.mp.microsoft.com/v7.0/products/lookup?alternateId=PackageFamilyName&value={family}&market={market}&languages={loc}&fieldsTemplate=details");
    let v: Value = crate::log::http_agent().get(&url).timeout(Duration::from_secs(15)).call().ok()?.into_json().ok()?;
    v["BigIds"].as_array()?.first()?.as_str().map(String::from)
}

/// Installs a free Store app by its id with winget, without the Store app (Windows' Store install
/// service does the download; nothing else is needed).
pub fn store_install(id: &str) -> Result<(), String> {
    if !(id.len() == 12 && id.chars().all(|c| c.is_ascii_alphanumeric())) {
        return Err("bad Store id".into());
    }
    if crate::settings::is_test_profile() {
        return Ok(());
    }
    let w = winget()?;
    let o = crate::tweaks::run(1800, &w, &["install", "--id", id, "--source", "msstore", "--silent", "--accept-package-agreements", "--accept-source-agreements", "--disable-interactivity"]).map_err(|e| e.to_string())?;
    *CACHE.lock() = None;
    if o.status.success() {
        crate::log::info(format!("tweaks: installed {id} from the Store catalog"));
        Ok(())
    } else {
        Err(winget_error(&o))
    }
}
