//! Tauri commands for the CPU and GPU pages, profile shortcuts and the RTSS overlay.

use crate::util::LockAnyway;
use crate::app::App;
use crate::profiles::{self, afterburner, cpu, hotkeys, icons, shortcuts, CpuProfile, GpuProfile};
use serde_json::{json, Value};
use std::sync::Arc;
use tauri::State;

type A<'a> = State<'a, Arc<App>>;
type R<T> = Result<T, String>;

fn sync_links(kind: &str, id: &str, name: &str, icon: &str, desktop: bool, start_menu: bool) -> Option<String> {
    shortcuts::sync(&shortcuts::Want { kind, id, name, icon, desktop, start_menu }).err()
}

fn icon_of(icon: &str) -> Option<String> {
    icons::preview(icon)
}

/// Everything the CPU page needs.
#[tauri::command]
pub async fn cpu_state(app: A<'_>) -> R<Value> {
    let a = app.inner().clone();
    tauri::async_runtime::spawn_blocking(move || cpu_state_now(&a)).await.map_err(|e| e.to_string())
}

fn cpu_state_now(app: &App) -> Value {
    let all_plans = profiles::plans::list();
    let s = app.settings.lock().clone();
    let lim = crate::hw::cpupower::limits();
    let hk = hotkeys::status();
    let act_plan = profiles::plans::active().map(|g| profiles::plans::guid_str(&g));
    let profs: Vec<Value> = s
        .cpu_profiles
        .iter()
        .map(|p| {
            let key = hotkeys::parse(&p.hotkey).map(|(m, v)| hotkeys::format(m, v));
            json!({
                "p": p, "summary": profiles::cpu_summary(p), "icon_url": icon_of(&p.icon),
                "hotkey_state": key.as_ref().and_then(|k| hk.get(k).cloned()),
                "plan_name": profiles::plans::parse(&p.plan).map(|g| profiles::plans::name(&g)),
                "plan_missing": profiles::plans::parse(&p.plan).map(|g| !all_plans.contains(&g)).unwrap_or(true),
            })
        })
        .collect();
    let hw = app.hw.lock().clone();
    json!({
        "profiles": profs,
        "active": s.active_cpu,
        "active_plan": act_plan,
        "hybrid": cpu::hybrid(),
        "limits": { "base": lim.base, "turbo": lim.turbo, "min": lim.min, "oc": lim.oc_possible, "oc_checked": lim.oc_checked, "oc_max": lim.turbo + crate::hw::cpupower::OC_MAX_EXTRA },
        "rapl": hw.is_some(),
        "power_msg": hw.as_ref().map(|h| h.power_msg.lock_anyway().clone()),
        "admin": crate::elevation::is_admin(),
        "clock": cpu::clock_check(),
        "game_mode_off": s.game_mode_orig != -1,
        "boost_cpu": s.boost_cpu,
        // a PC with a battery: the CPU page offers profiles for plugged in / on battery
        "battery": cpu::plugged_in().is_some(),
        "plugged": cpu::plugged_in(),
    })
}

#[tauri::command]
pub fn cpu_live(app: A) -> Value {
    let hw = app.hw.lock().clone();
    let l = hw.as_ref().map(|h| h.live.lock_anyway().clone());
    json!({
        "mhz": l.as_ref().and_then(|l| l.cpu_mhz), "temp": l.as_ref().and_then(|l| l.cpu_t), "load": l.as_ref().and_then(|l| l.cpu_l),
        "power": hw.as_ref().and_then(|h| h.table.lock_anyway().rows.iter().find(|r| r.group.starts_with("CPU") && r.name == "Package power").and_then(|r| r.cur)),
        "power_short": hw.as_ref().map(|h| h.power_short.lock_anyway().clone()),
        "clock": cpu::clock_check(),
        "active": app.settings.lock().active_cpu,
    })
}

#[tauri::command]
pub fn cpu_plans(app: A) -> Vec<Value> {
    cpu::plan_list(&app)
}

/// "Use my existing plans": adopts the chosen plans as profiles.
#[tauri::command]
pub fn cpu_adopt(app: A, guids: Vec<String>) -> R<usize> {
    let mut n = 0;
    for g in guids.iter().filter_map(|g| profiles::plans::parse(g)) {
        let used = app.settings.lock().cpu_profiles.iter().any(|p| profiles::plans::parse(&p.plan) == Some(g));
        if !used {
            let p = cpu::adopt(&g);
            app.settings.lock().cpu_profiles.push(p);
            n += 1;
        }
    }
    app.save_settings();
    app.changed("profiles");
    Ok(n)
}

#[tauri::command]
pub fn cpu_create(app: A, name: String) -> R<CpuProfile> {
    let name = if name.trim().is_empty() { "New profile".to_string() } else { name.trim().to_string() };
    let p = cpu::create(&name)?;
    app.settings.lock().cpu_profiles.push(p.clone());
    app.save_settings();
    app.changed("profiles");
    Ok(p)
}

/// Saves an edited profile (plan values, extras when active, shortcuts, hotkey).
#[tauri::command]
pub async fn cpu_save(app: A<'_>, p: CpuProfile) -> R<Value> {
    let mut p = p;
    p.hotkey = hotkeys::check(&app, &p.hotkey, &p.id)?;
    let prev = {
        let mut s = app.settings.lock();
        match s.cpu_profiles.iter_mut().find(|x| x.id == p.id) {
            Some(x) => {
                // the plan is Deck's to manage (it may have been re-made since the page loaded)
                p.plan = x.plan.clone();
                std::mem::replace(x, p.clone())
            }
            None => return Err("That profile no longer exists.".into()),
        }
    };
    let a = app.inner().clone();
    let (plan_err, link_err) = tauri::async_runtime::spawn_blocking(move || {
        let plan_err = cpu::save(&a, &p, Some(&prev)).err();
        let link_err = sync_links("cpu", &p.id, &p.name, &p.icon, p.desktop, p.start_menu);
        (plan_err, link_err)
    })
    .await
    .map_err(|e| e.to_string())?;
    app.save_settings();
    hotkeys::reload();
    app.changed("profiles");
    Ok(json!({ "plan_error": plan_err, "link_error": link_err }))
}

#[tauri::command]
pub async fn cpu_delete(app: A<'_>, id: String, delete_plan: bool) -> R<()> {
    let a = app.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        cpu::remove(&a, &id, delete_plan)?;
        shortcuts::remove("cpu", &id);
        Ok::<(), String>(())
    })
    .await
    .map_err(|e| e.to_string())??;
    app.save_settings();
    hotkeys::reload();
    app.changed("profiles");
    Ok(())
}

#[tauri::command]
pub fn cpu_duplicate(app: A, id: String) -> R<CpuProfile> {
    let src = app.settings.lock().cpu_profiles.iter().find(|p| p.id == id).cloned().ok_or("No such profile.")?;
    let g = profiles::plans::parse(&src.plan).ok_or("This profile has no Windows power plan.")?;
    let name = format!("{} copy", src.name);
    let ng = profiles::plans::duplicate(&g, &format!("Deck · {name}"))?;
    let p = CpuProfile { id: profiles::new_id(), name, plan: profiles::plans::guid_str(&ng), hotkey: String::new(), desktop: false, start_menu: false, ..src };
    app.settings.lock().cpu_profiles.push(p.clone());
    app.save_settings();
    app.changed("profiles");
    Ok(p)
}

/// Switch from the page / tray / phone.
#[tauri::command]
pub async fn profile_switch(app: A<'_>, kind: String, id: String) -> R<()> {
    let a = app.inner().clone();
    tauri::async_runtime::spawn_blocking(move || profiles::switch(&a, &kind, &id, "page")).await.map_err(|e| e.to_string())
}

// ------------------------------------------------------------------ GPU

fn gpu(app: &App) -> R<Arc<crate::gpu::Gpu>> {
    app.gpu.lock().clone().ok_or_else(|| "No NVIDIA graphics card was found (or its driver isn't loaded).".into())
}

#[tauri::command]
pub async fn gpu_state(app: A<'_>) -> R<Value> {
    let a = app.inner().clone();
    tauri::async_runtime::spawn_blocking(move || gpu_state_now(&a)).await.map_err(|e| e.to_string())
}

fn gpu_state_now(app: &App) -> Value {
    let s = app.settings.lock().clone();
    let hk = hotkeys::status();
    let g = app.gpu.lock().clone();
    let profs: Vec<Value> = s
        .gpu_profiles
        .iter()
        .map(|p| {
            let key = hotkeys::parse(&p.hotkey).map(|(m, v)| hotkeys::format(m, v));
            json!({ "p": p, "summary": profiles::gpu_summary(p), "icon_url": icon_of(&p.icon), "hotkey_state": key.as_ref().and_then(|k| hk.get(k).cloned()), "risky": crate::gpu::guard::risky(p) })
        })
        .collect();
    json!({
        "found": g.is_some(),
        "caps": g.as_ref().map(|g| g.caps.clone()),
        "current": g.as_ref().map(|g| g.current()),
        "applied": g.as_ref().and_then(|g| g.applied_id()),
        "profiles": profs,
        "active": s.active_gpu,
        "startup": s.gpu_startup,
        "afterburner": crate::gpu::afterburner_running(),
        // still able to override Deck (not just monitoring)
        "afterburner_tunes": crate::gpu::afterburner_running() && g.as_ref().map(|g| afterburner::tunes_card(&g.caps.pci)).unwrap_or(true),
        "afterburner_installed": afterburner::profiles_dir().is_some(),
        "admin": crate::elevation::is_admin(),
        "held_back": HELD.lock().clone(),
        "blank": GpuProfile::default(),
        "boost_gpu": s.boost_gpu,
    })
}

/// A risky startup profile the crash guard did not apply (shown with "Apply anyway").
pub static HELD: parking_lot::Mutex<Option<String>> = parking_lot::Mutex::new(None);

#[tauri::command]
pub fn gpu_live(app: A) -> R<Value> {
    Ok(gpu(&app)?.live())
}

/// The card's curve now: stock frequency and current delta per core point.
#[tauri::command]
pub fn gpu_curve(app: A) -> R<Value> {
    let g = gpu(&app)?;
    let c = g.curve().ok_or("The voltage/frequency curve can't be read on this card or driver.")?;
    Ok(json!({
        "points": c.core.iter().map(|p| json!({ "mv": p.mv, "base": p.base_mhz, "delta": p.delta_mhz })).collect::<Vec<_>>(),
        "mem": c.mem.iter().find(|p| p.editable).map(|p| p.delta_mhz),
    }))
}

/// Saves (adds when new) a GPU profile. Re-applies it when it is the active one.
#[tauri::command]
pub async fn gpu_save(app: A<'_>, mut p: GpuProfile) -> R<Value> {
    p.hotkey = hotkeys::check(&app, &p.hotkey, &p.id)?;
    if p.id.is_empty() {
        p.id = profiles::new_id();
    }
    if p.name.trim().is_empty() {
        p.name = "New profile".into();
    }
    let active = {
        let mut s = app.settings.lock();
        match s.gpu_profiles.iter_mut().find(|x| x.id == p.id) {
            Some(x) => *x = p.clone(),
            None => s.gpu_profiles.push(p.clone()),
        }
        s.active_gpu == p.id
    };
    app.save_settings();
    let link_err = sync_links("gpu", &p.id, &p.name, &p.icon, p.desktop, p.start_menu);
    hotkeys::reload();
    let mut apply_err = None;
    if active {
        let a = app.inner().clone();
        let p2 = p.clone();
        apply_err = tauri::async_runtime::spawn_blocking(move || a.gpu_engine().and_then(|g| g.apply(&p2).error())).await.map_err(|e| e.to_string())?;
    }
    app.changed("profiles");
    Ok(json!({ "id": p.id, "link_error": link_err, "apply_error": apply_err }))
}

#[tauri::command]
pub async fn gpu_delete(app: A<'_>, id: String) -> R<()> {
    {
        let mut s = app.settings.lock();
        s.gpu_profiles.retain(|p| p.id != id);
        if s.active_gpu == id {
            s.active_gpu.clear();
        }
        if s.gpu_startup == id {
            s.gpu_startup.clear();
        }
    }
    let id2 = id.clone();
    let _ = tauri::async_runtime::spawn_blocking(move || shortcuts::remove("gpu", &id2)).await;
    app.save_settings();
    hotkeys::reload();
    app.changed("profiles");
    Ok(())
}

/// Tries a profile without saving it (the editor's Apply / Test button).
#[tauri::command]
pub async fn gpu_try(app: A<'_>, p: GpuProfile) -> R<Value> {
    let g = gpu(&app)?;
    // a test of an overclock counts for the crash guard like a real apply
    crate::gpu::guard::record(&p, true);
    let fields = tauri::async_runtime::spawn_blocking(move || g.apply_fields(&p)).await.map_err(|e| e.to_string())?;
    app.changed("profiles");
    Ok(json!(fields))
}

/// Everything back to the card's defaults.
#[tauri::command]
pub async fn gpu_stock(app: A<'_>) -> R<String> {
    let g = gpu(&app)?;
    let r = tauri::async_runtime::spawn_blocking(move || g.stock()).await.map_err(|e| e.to_string())?;
    app.settings.lock().active_gpu.clear();
    app.save_settings();
    app.changed("profiles");
    r
}

/// Reads Afterburner's profiles (preview; nothing is saved).
#[tauri::command]
pub fn ab_preview(app: A) -> R<Value> {
    let g = gpu(&app)?;
    Ok(afterburner::summary(&afterburner::import(&g.caps.pci)?))
}

/// Imports Afterburner's profiles (and optionally its startup profile, overlay items and key).
#[tauri::command]
pub fn ab_import(app: A, startup: bool, osd: bool) -> R<usize> {
    let g = gpu(&app)?;
    let i = afterburner::import(&g.caps.pci)?;
    let n = i.profiles.len();
    {
        let mut s = app.settings.lock();
        let taken: Vec<String> = s.cpu_profiles.iter().map(|p| p.hotkey.clone()).chain(s.gpu_profiles.iter().map(|p| p.hotkey.clone())).collect();
        for mut p in i.profiles {
            if let Some(ex) = s.gpu_profiles.iter().find(|x| x.same_settings(&p)) {
                // already here (imported before): keep it, point the startup choice at it
                if i.startup.as_deref() == Some(p.id.as_str()) && startup {
                    s.gpu_startup = ex.id.clone();
                }
                continue;
            }
            let canon = |k: &str| hotkeys::parse(k).map(|(m, v)| hotkeys::format(m, v));
            if taken.iter().any(|t| !t.is_empty() && canon(t) == canon(&p.hotkey)) {
                p.hotkey.clear();
            }
            if startup && i.startup.as_deref() == Some(p.id.as_str()) {
                s.gpu_startup = p.id.clone();
            }
            s.gpu_profiles.push(p);
        }
        if osd && !i.osd.is_empty() {
            s.osd_items = i.osd;
            if let Some(h) = i.limiter_hotkey.clone() {
                if s.limiter_hotkey.is_empty() {
                    s.limiter_hotkey = h;
                }
            }
            if let Some(h) = i.osd_hotkey {
                s.osd_hotkey = h;
            }
        }
    }
    app.save_settings();
    hotkeys::reload();
    app.changed("profiles");
    Ok(n)
}

#[tauri::command]
pub async fn ab_stop_tuning() -> R<String> {
    tauri::async_runtime::spawn_blocking(afterburner::stop_tuning).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn ab_undo_stop() -> R<()> {
    tauri::async_runtime::spawn_blocking(afterburner::undo_stop).await.map_err(|e| e.to_string())?
}

/// "Apply anyway" for the overclock the crash guard held back (banner and toast).
pub fn apply_held(app: &Arc<App>) -> R<std::thread::JoinHandle<()>> {
    let id = HELD.lock().take().ok_or("Nothing is held back.")?;
    app.dismiss_toast("gpu-held");
    let a = app.clone();
    Ok(std::thread::spawn(move || profiles::switch(&a, "gpu", &id, "page")))
}

#[tauri::command]
pub async fn gpu_apply_held(app: A<'_>) -> R<()> {
    let h = apply_held(app.inner())?;
    tauri::async_runtime::spawn_blocking(move || h.join()).await.map_err(|e| e.to_string())?.map_err(|_| "The switch failed.".to_string())
}

// ------------------------------------------------------------------ shortcuts, icons, keys

#[tauri::command]
pub fn icons_list() -> Vec<Value> {
    icons::previews().iter().map(|(n, u)| json!({ "name": n, "url": u })).collect()
}

#[tauri::command]
pub fn icon_custom(file: String) -> R<Value> {
    let v = icons::custom(&file)?;
    Ok(json!({ "icon": v, "url": icons::preview(&v) }))
}

#[tauri::command]
pub fn icon_preview(icon: String) -> Option<String> {
    icons::preview(&icon)
}

#[tauri::command]
pub async fn shortcuts_repair(app: A<'_>) -> R<Vec<String>> {
    let a = app.inner().clone();
    tauri::async_runtime::spawn_blocking(move || shortcuts::repair(&a)).await.map_err(|e| e.to_string())
}

/// Checks a key for the capture field: normalized text, or why it can't be used.
#[tauri::command]
pub fn hotkey_check(app: A, key: String, id: String) -> Value {
    match hotkeys::check(&app, &key, &id) {
        Ok(k) => json!({ "ok": true, "key": k }),
        Err(why) => json!({ "ok": false, "why": why }),
    }
}

// ------------------------------------------------------------------ overlay

#[tauri::command]
pub fn rtss_state() -> Value {
    crate::rtss::status()
}

/// RivaTuner's framerate limiter: "state", "toggle", or "set" a limit (`fps`). Returns { on, fps }.
#[tauri::command]
pub async fn rtss_limiter(app: A<'_>, what: String, fps: Option<u32>) -> R<Value> {
    let app = app.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        match what.as_str() {
            "toggle" => crate::rtss::toggle_limiter(&app),
            "set" => crate::rtss::set_limit_fps(fps.unwrap_or(0))?,
            _ => {}
        }
        Ok(json!({ "on": crate::rtss::limiter_on(), "fps": crate::rtss::limit_fps() }))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn rtss_restore_look() -> R<usize> {
    crate::rtss::restore_look()
}

/// Starts the average, lows, lowest and highest (and the graph) over.
#[tauri::command]
pub fn rtss_reset_stats() {
    crate::rtss::reset_stats();
}

#[tauri::command]
pub fn rtss_start() -> R<()> {
    crate::rtss::start_rtss()
}

/// Opens RivaTuner's own window.
#[tauri::command]
pub async fn rtss_open() -> R<()> {
    tauri::async_runtime::spawn_blocking(crate::rtss::open_rtss_window).await.map_err(|e| e.to_string())?
}

/// Removes RivaTuner's tray icon (it keeps running) or brings it back.
#[tauri::command]
pub async fn rtss_tray(app: A<'_>, hidden: bool) -> R<()> {
    let app = app.inner().clone();
    app.settings.lock().rtss_tray_hidden = hidden;
    app.save_settings();
    tauri::async_runtime::spawn_blocking(move || crate::rtss::set_tray_icon(!hidden)).await.map_err(|e| e.to_string())
}

/// Downloads and installs RivaTuner (winget, from Guru3D): takes up to a minute.
#[tauri::command]
pub async fn rtss_install() -> R<()> {
    tauri::async_runtime::spawn_blocking(crate::rtss::install_rtss).await.map_err(|e| e.to_string())?
}
