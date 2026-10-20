# Deck hardware control — Implementation Plan

**Goal:** Deck absorbs MoboCtl (fans, RGB, cooler display, sensors, CPU power) and Afterburner-style GPU tuning, with CPU and GPU profiles switched by hotkeys, desktop/Start shortcuts (pickable icons), tray, phone and Game boost; Deck runs elevated.

**Architecture:** Deck (Tauri 2 + vanilla JS) gains `src-tauri/src/hw/` (MoboCtl's engine, ported nearly verbatim), `gpu/` (NVML official calls loaded with GetProcAddress + NvAPI undocumented VF-curve/volt calls), `profiles/` (CPU = Windows power plans + extras, GPU = NVML/NvAPI settings, switching, hotkeys, shortcuts, icons, Afterburner import) and `elevation.rs` (logon task, launch-as-user). UI pages go in `ui/js/hw.js`, `ui/js/profiles.js`, `ui/js/curve.js`.

**Tech Stack:** Rust 2021, Tauri 2.12, windows 0.62 + windows-sys 0.61, nvml-wrapper 0.13 (reads), raw nvml.dll / nvapi64.dll (writes), PowerShell install scripts, vanilla JS UI.

**Spec:** `docs/design/specs/2026-10-11-hardware-control-design.md`

## Global Constraints

- Build: (set `CARGO_TARGET_DIR` to build elsewhere); `cd src-tauri; cargo test; cargo build --release`; install with `..\install.ps1` (now self-elevating).
- No input hooks. Hotkeys = `RegisterHotKey` only.
- Never write hardware (fans, GPU, plans, Game Mode) in tests without asking the user; read-only probes are fine. Test runs use `DECK_PROFILE=test`.
- Never launch Deck's GUI on the user's desktop without asking; check `Get-Process Deck` before installing.
- Hardware pages hide when the hardware is absent; nothing PC-specific (Deck is shared with friends).
- Afterburner + RTSS stay installed; Deck never edits Afterburner files except via the opt-in "Stop Afterburner from tuning" with a backup.
- Apps/files/links Deck opens must run as the normal user (`util::launch_as_user`).
- Fans are released to BIOS on quit, panic and session end.
- GPU positive OC is not auto-applied after an unexpected restart (crash guard).
- No git repo: no commit steps. Backup of the pre-change tree is in `D:\Backups\deck-before-hw-20261011-1817`.
- Text style: plain, short sentences in UI copy (match existing Deck pages).

## Review Focus

1. Deck started unelevated (double-click exe) → Hardware pages show "Set up", no crash, everything else works.
2. A hotkey already held by Afterburner → profile saves, chip says "In use by another program", no panic.
3. A GPU write the driver refuses or clamps → read-back mismatch shown per field; profile not marked active.
4. Profile renamed/deleted while its shortcut/task exists → shortcut/task renamed/removed, no orphans; `Repair shortcuts` recreates.
5. Deck quits or panics while fans are on a curve → fans back to BIOS auto.

---

## File map

| File | Responsibility |
|---|---|
| `src-tauri/src/elevation.rs` (new) | `is_admin`, logon task create/enable/disable/run, `relaunch_elevated` |
| `src-tauri/src/util.rs` (modify) | `launch_as_user` (IShellDispatch2 via Explorer) |
| `src-tauri/src/hw/*` (new, ported) | MoboCtl engine; `hw/mod.rs` = `Hw` (was `Shared`), `spawn_all`, `shutdown`, `report` |
| `src-tauri/src/hw/kv.rs` (new) | Tiny key/value store replacing MoboCtl's `Settings` so ported modules compile unchanged; persisted at `%APPDATA%\Deck\hw.ini` |
| `src-tauri/src/gpu/nvml.rs` (new) | raw NVML: power limit get/set/range, clock offsets get/set/range, fans count/set/default, temp threshold, live readings |
| `src-tauri/src/gpu/nvapi.rs` (new) | NvAPI QueryInterface, VF points info/status/control, volt rails boost |
| `src-tauri/src/gpu/curve.rs` (new) | VF curve model + Afterburner VFCurve hex decode/encode |
| `src-tauri/src/gpu/mod.rs` (new) | `Gpu` state: apply profile, read-back, fan-curve loop, driver-reset watch, crash guard |
| `src-tauri/src/profiles/mod.rs` (new) | `CpuProfile`, `GpuProfile`, `switch()`, active tracking, summaries |
| `src-tauri/src/profiles/plans.rs` (new, from boost.rs `plan`) | powrprof: list/active/set/duplicate/rename/delete, read/write AC/DC values incl. hidden PROCFREQMAX |
| `src-tauri/src/profiles/cpu.rs` (new) | apply CPU profile (plan + RAPL + min lock + OC + Game Mode), plan watcher, in-game clock check |
| `src-tauri/src/profiles/afterburner.rs` (new) | parse Afterburner cfg → `GpuProfile`s + hotkeys; stop-tuning backup/undo |
| `src-tauri/src/profiles/hotkeys.rs` (new) | hotkey string parse/format, RegisterHotKey thread |
| `src-tauri/src/profiles/shortcuts.rs` (new, from MoboCtl) | runner vbs, per-profile tasks, .lnk on Desktop/Start |
| `src-tauri/src/profiles/icons.rs` (new) | built-in icon set (embedded .ico), write to disk, PNG→ICO, exe icon |
| `src-tauri/icons/profiles/*.ico` (new) | ~24 generated icons |
| `src-tauri/src/main.rs` (modify) | mods, commands, tray submenus, single-instance `--switch`, autostart via task, debug flags |
| `src-tauri/src/settings.rs` (modify) | new fields |
| `src-tauri/src/boost.rs` (modify) | use `profiles::plans`; boost uses chosen profiles; reopen via launch_as_user |
| `src-tauri/src/winmgr.rs`, `apps.rs` (modify) | launch_as_user |
| `src-tauri/src/remote.rs` + `phone/index.html` (modify) | profile chips + `/api/profile` |
| `ui/js/app.js` (modify) | NAV groups (Hardware, Misc), badges |
| `ui/js/curve.js` (new) | canvas curve editor (fan curves, GPU fan curve, VF curve) |
| `ui/js/hw.js` (new) | Fans, Lighting, Cooler display, Sensors, This PC pages |
| `ui/js/profiles.js` (new) | CPU and GPU pages, profile editor, hotkey capture, icon picker, shortcuts |
| `ui/index.html`, `ui/app.css` (modify) | script tags, styles |
| `install.ps1` (rewrite), `uninstall.ps1`, `src-tauri/run.vbs` (new) | Program Files install, tasks, shortcuts |

---

### Task 1: Elevation, launch-as-user, install

**Files:** Create `src-tauri/src/elevation.rs`, `uninstall.ps1`; modify `util.rs`, `main.rs`, `apps.rs`, `winmgr.rs`, `boost.rs`, `Cargo.toml`, `install.ps1`.

**Interfaces — Produces:**
- `elevation::is_admin() -> bool`
- `elevation::TASK: &str = "\\Deck\\Deck"`
- `elevation::ensure_task(exe: &Path) -> Result<(), String>` (needs admin)
- `elevation::set_task_enabled(on: bool) -> Result<(), String>`
- `elevation::run_task(name: &str) -> bool`
- `util::launch_as_user(file: &str, args: &str, dir: &str) -> Result<(), String>`
- Tauri command `elevate_setup()` → UAC once (`ShellExecute runas` of `Deck.exe --setup-task`), the elevated copy creates the task, runs it, exits; the unelevated Deck quits.

- [ ] Step 1: Unit test for the task XML builder (pure function `task_xml(exe, args) -> String` contains `<RunLevel>HighestAvailable</RunLevel>`, `<LogonTrigger>`, escaped exe path, `<Priority>5</Priority>`, `<ExecutionTimeLimit>PT0S`).
- [ ] Step 2: Implement `elevation.rs` (`is_admin` via `IsUserAnAdmin`-equivalent token elevation check `GetTokenInformation(TokenElevation)`; tasks via `schtasks.exe /create /xml` UTF-16 file like MoboCtl `shortcuts.rs`; `/change /enable|/disable`; `/run`).
- [ ] Step 3: Implement `launch_as_user`: `CoCreateInstance(CLSID_ShellWindows)` → `FindWindowSW(CSIDL_DESKTOP, SWC_DESKTOP, SWFO_NEEDDISPATCH)` → `IServiceProvider::QueryService(SID_STopLevelBrowser, IShellBrowser)` → `QueryActiveShellView` → `IShellView::GetItemObject(SVGIO_BACKGROUND, IDispatch)` → `IShellFolderViewDual::Application` → `IShellDispatch2::ShellExecute(file, args, dir, "open", SW_SHOWNORMAL)`. If not admin, just `ShellExecuteW` (no Explorer round-trip). Fallback on any COM error: `ShellExecuteW` + `log`.
- [ ] Step 4: Route `util::shell_open`, `apps.launch`, `winmgr` workspace spawn, `boost::reopen` through it (keep `open_system` taskmgr as is).
- [ ] Step 5: Replace tauri-plugin-autostart: remove plugin + dependency; `sync_autostart(on)` → `elevation::set_task_enabled(on)` when admin. Single-instance callback: if args contain `--switch <k>:<id>` → `profiles::switch` (stub in this task: log), else `show_deck`.
- [ ] Step 6: Debug flag `--launch-test`: `launch_as_user("cmd.exe", "/c whoami /groups > %TEMP%\\deck-launch-test.txt", "")`, wait 3 s, print whether the file contains `Medium Mandatory Level`.
- [ ] Step 7: `install.ps1`: self-elevate (`Start-Process powershell -Verb RunAs` if not admin), build, stop if Deck running, copy to `C:\Program Files\Deck\Deck.exe` + `run.vbs`, remove old `%LOCALAPPDATA%\Programs\Deck` and HKCU Run value `Deck`, run `Deck.exe --setup-task` (creates task), Start menu `Deck.lnk` → `wscript.exe "C:\Program Files\Deck\run.vbs" "\Deck\Deck"` with icon from Deck.exe. `uninstall.ps1`: quit Deck, `schtasks /delete` every `\Deck\*`, remove shortcuts (Start `Deck.lnk`, every `.lnk` whose target args contain `\Deck\Profile`), remove Program Files folder.
- [ ] Step 8: `cargo test elevation`; `cargo build`; `deck.exe --launch-test` from an elevated shell → expect `Medium Mandatory Level` line.

### Task 2: Navigation (Hardware + Misc)

**Files:** modify `ui/js/app.js`, `ui/js/pages1.js` (HEADS).

- [ ] Step 1: Replace `NAV` with the grouping in spec §3; Hardware items `["cpu",null,"CPU","cpu"]`, `["gpu",null,"GPU","gpu"]`, `["fans",null,"Fans","fan"]`, `["lighting",null,"Lighting","bulb"]`, `["lcd",null,"Cooler display","display"]`, `["sensors",null,"Sensors","gauge"]`, `["thispc",null,"This PC","info"]`. Items are filtered by `S.state.hw.pages` (list of page ids the backend says exist; when not admin: `["cpu","gpu","fans","lighting","lcd","sensors","thispc"]` all present and render the "Set up" panel).
- [ ] Step 2: Add missing icons to `icons.js` (gpu, fan, bulb, gauge, info, keyboard, image).
- [ ] Step 3: Offscreen render check of the sidebar (existing dump-state + headless Edge flow).

### Task 3: GPU probe (spike S1, read-only part)

**Files:** create `src-tauri/src/gpu/{mod.rs,nvml.rs,nvapi.rs}`.

**Interfaces — Produces:**
- `nvml::Nvml::load() -> Option<Nvml>`; `dev(i) -> Option<Dev>`; on `Dev`: `power_limit() -> Option<(u32 mW cur, u32 min, u32 max, u32 default)>`, `set_power_limit(mw) -> Result<(),i32>`, `offsets() -> Option<OffsetInfo{core:(cur,min,max), mem:(cur,min,max)}>`, `set_core_offset(mhz)`, `set_mem_offset(mhz)` (try `nvmlDeviceSetClockOffsets` v1 then `SetGpcClkVfOffset`/`SetMemClkVfOffset`), `fans() -> u32`, `fan_speed(i)`, `set_fan(i,pct)`, `fan_auto(i)`, `temp_limit() / set_temp_limit(c)` (acoustic threshold), `live() -> GpuLive{core_mhz, mem_mhz, power_w, temp_c, fan_pct, fan_rpm, util, limit_reason}`.
- `nvapi::NvApi::load() -> Option<NvApi>`; `vf_info()`, `vf_status() -> Vec<VfPoint{mv, base_mhz}>`, `vf_control_get() -> Vec<u8>` (raw 0x2420), `vf_control_set(raw)`, `volt_boost() / set_volt_boost(pct)`.
- Debug flag `--gpu-probe` prints every read and which calls returned NVAPI_OK/NVML_SUCCESS, plus struct-version detection (try ver 1..3 per struct).

- [ ] Step 1: Implement loaders with `LoadLibraryW("nvml.dll")` / `nvapi64.dll` + `GetProcAddress` (`nvapi_QueryInterface`).
- [ ] Step 2: Run `--gpu-probe` (read-only, safe while Afterburner runs). Record results in `PROJECT_STATE.md` under "GPU probe".
- [ ] Step 3: Ask the user for a quiet moment (no game) and Afterburner closed; run `--gpu-probe --write` doing: core −100 (10 s, revert), mem −500 (10 s, revert), power limit 90% (revert), fan 40% 5 s → auto, VF flatten 975 MHz from 625 mV 10 s → reset. Each verified by read-back and `nvidia-smi`. Drop UI fields whose write fails.

### Task 4: Port the MoboCtl engine

**Files:** copy `MoboCtl-Source/src/{pawn,sio,hp,sensors,sysmon,smbios,fx,hid,devices,screen,game,cpupower,powermode,rapl,uv,memclean,winutil}.rs` → `src-tauri/src/hw/`; `audio.rs`→`hw/loopback.rs`; `display.rs`→`hw/lcd.rs`; `shared.rs`→`hw/mod.rs`; `mods/`→`hw/mods/`; create `hw/kv.rs`.

Port rules (mechanical):
1. `crate::X` → `crate::hw::X` for every ported module; `crate::audio`→`crate::hw::loopback`; `crate::display`→`crate::hw::lcd`; `crate::settings::{Settings, dir}` → `crate::hw::kv::{Kv, dir}`.
2. `hw/kv.rs` exposes MoboCtl's Settings API unchanged: `load()`, `get/int/bool/set/set_int/set_bool/save`, `dir()` = `%APPDATA%\Deck` (or test profile dir), file `hw.ini`; `Curve::load_all/save_all` stores `fancurves.ini` there.
3. In `cpupower.rs` delete `PLAN_NAME`, `plan()`, `keep_active()`, `parse_guid`, `active_plan`; every `plan()` → `"SCHEME_CURRENT"`.
4. In `hw/mod.rs` delete the plan-keeper thread and `Cmd`; `winutil::notify` calls go through a `notify` callback set by Deck (toasts).
5. `include_bytes!` paths for PawnIO modules adjusted to `hw/mods/`.
6. Add windows-sys 0.61 (MoboCtl feature list) and `image` to `Cargo.toml`.

**Interfaces — Produces:** `hw::Hw` (MoboCtl `Shared` fields), `hw::start(notify: Box<dyn Fn(&str,&str)+Send+Sync>) -> Option<Arc<Hw>>` (None when not admin), `hw::stop()` (releases fans), `hw::report(&Hw) -> String`, `hw::pages(&Hw) -> Vec<&'static str>`.

- [ ] Step 1: Copy files, apply rules, `cargo build` until clean.
- [ ] Step 2: Panic hook in `main` calls `hw::stop()`; `RunEvent::Exit` calls `hw::stop()`.
- [ ] Step 3: Debug flags `--hw-selftest` (read-only: board, chip, control, fan names, temps after 2 s, RGB status, RAM status, display status, GPU name) and `--release-fans`.
- [ ] Step 4: Unit test `kv` round trip (set/save/load in test profile dir).
- [ ] Step 5: Run `--hw-selftest` elevated with MoboCtl **quit first** (ask user) — they fight over the chip.

### Task 5: Hardware pages

**Files:** create `ui/js/curve.js`, `ui/js/hw.js`; modify `main.rs` (commands), `index.html`, `app.css`, `pages3.js` (Clean up card goes in pages2? — Clean up lives in `pages2.js`).

**Commands:** `hw_state() -> {admin, pages, hw: HwInfo, status, control, sources, curves, cfg}`, `hw_live() -> {live, hist?}`, `hw_set(section, value)` (fans/lighting/lcd/alerts), `hw_apply(section)`, `hw_lights_off(on)`, `hw_display_off(on)`, `hw_report(save: bool) -> String`, `hw_test_lights()`, `hw_clear_ram() -> String`, `hw_sensor_table() -> Table`, `hw_reset_table()`, `hw_import_moboctl() -> String`, `elevate_setup()`.

- [ ] Step 1: `curve.js`: `curveEditor({points, xRange, yRange, xUnit, yUnit, live, onchange, minY})` → element; drag/add/remove points; live marker.
- [ ] Step 2: Pages per spec §5.2 with MoboCtl's labels and hover texts.
- [ ] Step 3: Tray `Lights off` check item when RGB present.
- [ ] Step 4: MoboCtl import reads `C:\Program Files\MoboCtl\settings.ini` + `curves.ini` into `hw.ini` / `fancurves.ini` (same format → copy keys except `Profile*`, `PlanOwned`, `PlanLock`, `Cpu*`).
- [ ] Step 5: Offscreen renders of the five pages with mocked `hw_*` data; fix layout.

### Task 6: CPU profiles backend

**Files:** create `profiles/{mod.rs,plans.rs,cpu.rs}`; modify `boost.rs` (use `profiles::plans`), `settings.rs`.

**Interfaces — Produces:**
```rust
pub struct CpuProfile { id, name, icon, hotkey: String, desktop: bool, start_menu: bool, plan: String /*guid*/,
  cap_p: Option<u32>, cap_e: Option<u32>, max_state: u32, min_state: u32, turbo: u32 /*PERFBOOSTMODE*/,
  min_cores: u32, epp: Option<u32>, power: Option<CpuPower /*preset|pl1,pl2*/>, min_lock: Option<u32>, oc: Option<u32>, hold_in_games: bool }
plans::{list() -> Vec<(GUID,String)>, active(), set(&GUID), duplicate(&GUID, name) -> GUID, rename, delete, read(&GUID, sub, setting) -> Option<(u32 ac,u32 dc)>, write(&GUID, sub, setting, ac, dc)}
cpu::{read_profile(guid) -> CpuProfile, write_plan(&CpuProfile), apply(&App,&CpuProfile) -> Result<String,String>, start_watch(app), clock_check() -> Value}
```
- [ ] Step 1: Tests: `CpuProfile` serde defaults; summary text (`"1500 MHz · turbo off · 35 W"`); GUID constants for PROCFREQMAX e100/e101, PERFBOOSTMODE, PROCTHROTTLEMAX/MIN, CPMINCORES, PERFEPP.
- [ ] Step 2: `plans.rs` (PowerReadACValueIndex / PowerWriteACValueIndex for both AC/DC; hidden settings work through the same API); re-activate after write (switch to Balanced then back).
- [ ] Step 3: `cpu::apply`: write plan if dirty, `set` active, RAPL via `hw::powermode`/`rapl` if Hw present, min lock / OC via `hw::cpupower`, Game Mode `HKCU\Software\Microsoft\GameBar\AutoGameModeEnabled` (remember original in settings `game_mode_orig`).
- [ ] Step 4: Watcher thread (2 s): active plan changed → find profile by plan → apply extras + mark active; else active.cpu = None. Clock check: while `hw::game::foreground_game()` (or `hotcorner::fullscreen_in_front()`) sample `\Processor Information(_Total)\% Processor Performance` × base MHz (PDH) every 2 s; keep current/last session avg/min/max.
- [ ] Step 5: `--cpu-probe` debug flag: list plans + caps + active, read-only.

### Task 7: CPU page UI

**Files:** `ui/js/profiles.js` (CPU part), commands `cpu_state`, `cpu_profile_save`, `cpu_profile_delete`, `cpu_import_plans(guids)`, `profile_switch(kind,id)`.
- [ ] Profile cards, editor (sliders for P/E caps with presets, states, turbo seg, min cores, EPP, power seg + W fields, min lock, OC when possible, hold-in-games toggle), clock check panel, "Use my existing plans" dialog. Shortcut/hotkey/icon section is shared with GPU (Task 10 fills it; here render it disabled).
- [ ] Offscreen render.

### Task 8: GPU backend

**Files:** `gpu/{mod.rs,curve.rs}`, `profiles/afterburner.rs`, settings fields.

**Interfaces — Produces:**
```rust
pub struct GpuProfile { id, name, icon, hotkey, desktop, start_menu, power_pct: u32, temp_limit: Option<u32>,
  core_mhz: i32, mem_mhz: i32, volt_pct: Option<u32>, curve: Option<Vec<CurvePt>>, fan: Fan }
pub enum Fan { Auto, Fixed(u32), Curve(Vec<(f32,f32)>) }
gpu::Gpu::{new() -> Option<Arc<Gpu>>, caps() -> Caps, live() -> GpuLive, apply(&GpuProfile) -> ApplyReport{ok: bool, fields: Vec<(String,String)>}, stock(), start_loops(app)}
curve::{decode_ab(hex) -> Option<Vec<AbPt{mv,mhz,delta}>>, encode_ab(..), flatten_from(&mut pts, mv, mhz), hold(mhz, mv)}
afterburner::{find_cfg() -> Option<PathBuf>, import(path) -> Vec<(String /*section*/, GpuProfile)>, hotkeys(main_cfg) -> HashMap<u32,String>, stop_tuning() -> Result<PathBuf,String>, undo(backup)}
```
- [ ] Step 1: Tests on fixture `src-tauri/tests/fixtures/ab-3070.cfg` (copy of the real file): 7 sections; Profile1 → power 112, core +150, mem +500, volt 100, curve Some (non-zero deltas); Profile4 → power 41, core −493, mem 0, fan Fixed? (FanMode=1 → **Auto**? verify by probe: Afterburner FanMode 0=auto,1=manual-fixed; FanSpeed used only when manual) ; hotkey `0006004Fh` → `"Ctrl+Alt+O"`; `0x0006 00DD` → `"Ctrl+Alt+]"`. VFCurve decode: header 12 bytes, 127 points, first point mv=450.
- [ ] Step 2: Implement; `apply` diff + read-back; fan loop (1 s, hysteresis 2 °C), reset watch (5 s), crash guard (`gpu_last_applied` + boot time from `GetTickCount64`).
- [ ] Step 3: Afterburner banner data: `afterburner_running()`.

### Task 9: GPU page UI
- [ ] Live panel, profile cards, editor (power % slider with W, temp limit, core/mem offset sliders with ranges from caps, volt boost, fan seg + fixed slider + curve editor, VF curve editor with Flatten/Hold/Shift/Reset), startup profile select, Afterburner import button + banner + stop-tuning/undo. Offscreen render.

### Task 10: Switching surfaces
**Files:** `profiles/{hotkeys.rs,shortcuts.rs,icons.rs}`, icons, tray, remote, boost.
- [ ] Step 1: Tests: `hotkeys::parse("Ctrl+Alt+]") == Some((MOD_CONTROL|MOD_ALT, VK_OEM_6))`, `format` round trip for letters, digits, F1–F24, OEM keys, numpad; reject no-modifier non-F13+ keys. Shortcut `.lnk` create/read/delete in temp dir. PNG→ICO produces valid ICO header with 5 sizes.
- [ ] Step 2: `profiles::switch(app, kind, id, source)`; toast; tray submenus; `--switch` handled; hotkey thread with conflict status `HashMap<String /*kind:id*/, HotkeyState{Ok,InUse,Invalid}>`; retry 30 s.
- [ ] Step 3: Shortcuts: runner `run.vbs` in Program Files (or `%APPDATA%\Deck` if not installed), task per profile `\Deck\Profile cpu-<id>`, `.lnk` on `FOLDERID_Desktop` and Start `Programs\Deck\`; rename/delete sync; `Repair shortcuts` command.
- [ ] Step 4: Icons: generate 24 icons (headless Edge render of SVG Fluent glyphs on gradient tiles → PNG 256 → ICO via `icons::png_to_ico`), embed, write to `<install>\icons` / `%APPDATA%\Deck\icons`; picker command `icon_list()`, `icon_pick_file()`.
- [ ] Step 5: Phone: PC card chip rows + `POST /api/profile {kind,id}`; Game boost: settings `boost_cpu`, `boost_gpu` used by `boost::start/stop`.
- [ ] Step 6: UI: shared shortcut section in editor (hotkey capture, Desktop/Start toggles, icon grid).

### Task 10b: RTSS overlay feed + MAHM publishing (spec §6.7b, §6.10)
**Files:** create `src-tauri/src/rtss.rs`; modify `hw/` (CPU package power from MSR 0x611 energy counter), `main.rs`, `ui/js/hw.js` (Sensors: "Show in overlay" ticks, overlay settings), settings (`osd_on`, `osd_items`, `osd_ms`, `osd_hotkey`, `rtss_autostart`).

**Interfaces — Produces:** `rtss::Rtss::{open() -> Option<Rtss>, write_osd(&str) -> bool, clear()}`, `rtss::Mahm::{create() -> Option<Mahm>, update(&[MahmEntry])}`, `rtss::osd_text(&OsdData, &[String]) -> String`, `rtss::start(app)` (thread), `rtss::running() -> bool`, `rtss::launch()`.
- [ ] Step 1: Tests: `osd_text` for a fixed `OsdData` produces the expected Afterburner-like lines; missing values are skipped; `MahmEntry` buffer layout: header 32 bytes (`dwHeaderSize=32`), entry size 1324 (5×260 + 6×4), GPU entry size 1304; signature `0x4D41484D` ('MAHM'); test by building into a Vec and reading fields at offsets.
- [ ] Step 2: Implement with `OpenFileMappingW`/`CreateFileMappingW` + `MapViewOfFile`; slot claim loop over `arrOSD[1..dwOSDArrSize]` by owner.
- [ ] Step 3: Afterburner import also brings OSD items + `OSDToggleHotkey`.

### Task 11: Ship + retire
- [ ] `cargo test`, release build, asset test, offscreen renders of every new page, ask the user before installing (Deck must be quit; MoboCtl must be quit).
- [ ] After user confirms Deck works: `MoboCtl.exe --uninstall` (ask), remove old CPU plan shortcuts (ask), update `PROJECT_STATE.md`, `GPU-CPU-POWER-PROFILES.md`, memory.
