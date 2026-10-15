# Deck

Deck is a single Windows tray app that brings together a launcher, a live dashboard, PowerToys-style tools, full hardware control, and a phone remote. It replaces **MoboCtl** (fans, RGB, cooler display, sensors) and **MSI Afterburner** (GPU tuning). **RivaTuner (RTSS)** stays installed because it draws the in-game overlay.

- **Version:** 2.5.4. Setup: `cargo tauri build --bundles nsis`, attached to each [GitHub release](https://github.com/MohammadFakih02/deck/releases).
- **Stack:** Tauri 2, Rust backend, vanilla JS UI. No frameworks, no build step for the UI.
- **Source:** https://github.com/MohammadFakih02/deck (public). Machine-specific notes (the maintainer's PC, local paths, personal profiles) live in `PRIVATE_NOTES.md`, which is git-ignored; read it first if it exists.
- **Detailed history:** session-by-session handoff notes are in `PROJECT_STATE.md`.
- **Hardware design:** the spec, plan and decision ledger are in `docs\superpowers\`. Audit reports are in `docs\audits\`.
- **User-facing changes:** `CHANGELOG.md` (also the release notes).

---

## 0. For Claude sessions working on Deck

Read this section first. `CLAUDE.md` in this folder points here, so it loads automatically. If `PRIVATE_NOTES.md` exists (the maintainer's machine only), read it too.

> **RULE: update the docs before every commit.** **Before every commit, update the Markdown docs** (`PROJECT_STATE.md` always, `CHANGELOG.md` for anything a user would notice, `DECK.md` / `README.md` when facts change) and stage them in the same commit. The pre-commit hook in `.githooks/` refuses code commits without them; never bypass it with `--no-verify`.

### Who Deck is for
- People who play online games with kernel anti-cheat (Vanguard, EAC, BattlEye), so Deck must never hook input or inject into games.
- Deck is shared with friends and is public, so everything must work on other PCs: no names, paths or ids specific to one PC in the code or the docs. Personal facts go in `PRIVATE_NOTES.md` only.
- Looks matter. Check UI changes with a CDP screenshot or an offscreen render before calling them done.

### Hard rules
1. **Never capture the real screen.** Screenshots of Deck's own page over CDP (`cdp.mjs shot`) are fine.
2. **Don't change hardware while the user may be in a game.** That includes fans, clocks, power plans, audio and display. Check what's running first (look for game processes and fullscreen windows), and ask if unsure. During an agreed test session it's fine; keep every load test under 2 minutes.
3. **No input hooks, no injection into games.** Use `RegisterHotKey`, cursor polling, WinEvent hooks or `GetAsyncKeyState` only. The one place Deck sends input is the phone's Screen control (SendInput, user-driven), and it pauses while `boost::game_in_front` sees a game.
4. **Never change the default recording device.** MicDrop owns it.
5. **Test modes use `DECK_PROFILE=test`** (`%APPDATA%\Deck-test`) and bind to 127.0.0.1. A server on 0.0.0.0 makes a Windows Firewall prompt pop up on the user's screen.
6. **Git:** Deck is on GitHub (`MohammadFakih02/deck`, branch `main`). Never add a Co-Authored-By Claude line or a "Generated with Claude Code" line. Commit only when asked.
   - **Before every commit, update the docs** (see the rule at the top of this section and §9). This is not optional: the pre-commit hook blocks code commits that don't stage `PROJECT_STATE.md` and `CHANGELOG.md`.
   - Never commit personal data: no local paths, hostnames, PC specs, game lists or screenshots of the real desktop. `artifacts/`, `private/` and `PRIVATE_NOTES.md` are git-ignored on purpose.
7. **Don't modify Claude's own settings files** to grant yourself permissions; the user adds permission rules themselves. `cargo build` is allowed.
8. **Hotkeys are an exception, not the main interaction.** Prefer menus, the tray and the panel. Hotkeys are only for profile switching and the optional open, overlay and boost keys.
9. Deleting user data (Clean up for real, purging settings) needs the user's OK first.
10. **Don't re-propose** NetLimiter-style limits; the user declined them. Away-from-home access goes through Tailscale (never by opening ports to the internet); Sunshine + Moonlight is a possible later option next to RustDesk.

### Workflow
- **Build:** `cargo build --release` in `src-tauri`. Set `CARGO_TARGET_DIR` to build elsewhere (the maintainer's value is in `PRIVATE_NOTES.md`). A build takes about 2 minutes. **The UI is embedded in the exe, so UI changes also need a rebuild and reinstall.**
- **Tests:** `cargo test --release`, run in `src-tauri`; 118 should pass (29 ignored live tests). Check the JS with `node --check ui/js/*.js`.
- **Install and run:** Deck runs elevated, so installing needs admin. `Start-Process powershell -Verb RunAs -WindowStyle Hidden -File tools\testing\elev-install.ps1` (one UAC prompt unless UAC is off). That script quits Deck, runs `install.ps1 -NoBuild`, and starts Deck **with the CDP debug port 9222**. Always finish with `tools\testing\restart-normal.ps1`, **run elevated as well** (`Start-Process powershell -Verb RunAs -File ...`; from a normal shell its `--quit` can't reach the elevated Deck, so the debug instance silently keeps running). It quits Deck and runs the `\Deck\Open` task. Then check that port 9222 is no longer listening.
  - In Git Bash, run `powercfg` and `schtasks` with `MSYS_NO_PATHCONV=1`, because Git Bash rewrites `/flags` and `\paths`.
- **Live testing** (`tools\testing\`):

  | Script | What it does |
  |---|---|
  | `cdp.mjs` | `node cdp.mjs eval "<js>"` runs JS in Deck's page; `invoke("cmd", {args})` calls a backend command and `go(page, sub)` navigates. `node cdp.mjs shot out.png` screenshots Deck's page. `S` (the UI state) is a top-level `const`, not `window.S`. The page only ticks when `body` has class `shown` and the document is visible. |
  | `ui-preview/regress.mjs` | UI regression checks (34) with real mouse and keyboard input: clicks not lost to live or event redraws or on the sidebar, search Enter uses the current query, Keep awake / Shut down when… flows, Recycle Bin never pre-ticked in Clean up, no trash button on Windows' own folders in Disk space, profile editors kept open when Deck hides, the merged sidebar, ping only between Start and Stop, a "?" on every CPU/GPU editor setting, the overlay font list, focus kept across redraws, named controls, dialog focus trap, Space on buttons. Run after UI changes; every line should say PASS. `DECK_UI=<folder>` runs them against another copy of `ui/`. |
  | `ui-preview/toast-test.mjs` | The notification window: toasts under the mouse stay put and don't time out; held changes run when the mouse leaves. |
  | `ui-preview/a11y.mjs [w h] [out.json]` | Visits every page and counts unnamed buttons, unlabelled fields, targets under 24 px, sideways overflow, script errors and page-switch time. |
  | `ui-preview/all-sizes.sh <dir> <w> <h> <port>`, `phone-shot.mjs <dir>` | Every page at one window size; the phone remote at 320–1024 px with a fake WebSocket. |
  | `ui-preview/shot.mjs` | Screenshots any page in headless Edge against a fake backend (`ui-preview/mock.js` plus `mock-data.js` for the other pages; `all.sh <folder>` shoots every page and prints script errors), with no Deck restart and no hardware access. Use it for UI work while the user is gaming. Example: `node shot.mjs sensors - s.png 1260 900 'S.cache.sensTab="readings";render()'`. Add fake commands to `mock.js` as pages need them. |
  | `hwi.py` | Reads HWiNFO's shared memory. HWiNFO must be running with Shared Memory Support enabled. `hwi.py list [regex]` or `hwi.py watch SECS "pattern"...`, where patterns match "sensor \| label" (for example `"\| GPU Clock$"`). |
  | `cputest.sh` / `gputest.sh` | Switch a profile, put load on, and record clocks and power. Pass profile ids, or `stock` for the GPU. `load.py N` spins every core for N seconds. For GPU load, run a GPU benchmark such as Unigine Heaven; restore its window if it is minimized, or it stops rendering. |
  | `keys.ps1 ctrl+alt+o` | Synthetic key presses. They **must come from an elevated process**; Windows blocks them into the admin window otherwise. |
  | `rtssmem.py` | Prints RTSS's overlay slots, to check Deck's slot fills and clears. |
  | `phone.mjs <key> <idA> <idB>` | Checks the phone remote: key check, WebSocket and profile switch. |
  | `setfans.js` | Sets every fan to one mode (see the comment at its top). |
- **Before ending a test session, put everything back:** settings (`mode`, `open_hotkey`, `osd_on`), the active CPU and GPU profiles, and fan modes. Restart Deck normally.
- **Notes (mandatory before every commit):** add a dated entry to `PROJECT_STATE.md` for each session, add user-facing changes to `CHANGELOG.md`, and update this file and `README.md` when facts change. See §9.

### Gotchas already learned
- **Shortcuts: `RegisterHotKey` plus a `GetAsyncKeyState` poll.** Games can turn registered hotkeys off (`RIDEV_NOHOTKEYS`); the poll in `profiles/hotkeys.rs` covers that. Never add a keyboard hook instead.
- **Phone connections are counted per IP** (`remote::CONNS`); every new phone WebSocket handler must call `admit()` and hold a `register()` guard.
- **A lights test must always release `rgb_hold`.** Use `hw::lights_test()` (thread + 40 s limit); `HidD_GetFeature` has no timeout. To debug the board from outside Deck, read/write feature 0x52 (config, 185 bytes) and 0x53 (colours, 725 bytes) on `vid_1462&pid_7d43` with Python ctypes; bytes 62 and 82 of the config read-back differ from what was written.
- **Never loop `requestAnimationFrame`.** The WebView2 flags keep hidden windows awake, so a frame loop renders at the monitor's refresh rate in the tray (it cost ~15 % of a core). `guard.js` requests a single frame per heartbeat.
- **WebView2 browser flags** must go on each window builder (`webview_guard::browser_args()`); the environment variable alone is overridden by wry. The main window is created in `setup` (`"create": false` in `tauri.conf.json`).
- **Tauri caches compressed UI assets** and never rewrites them. A crash mid-build leaves broken pages; `install.ps1` clears the cache.
- **The page can miss the "shown" event at startup.** `app.js` `catchUp()` asks `get_state.visible` at boot, on focus and on visibility change, so the page still starts ticking.
- **Afterburner running fights Deck.** The GPU drift and resume re-apply are skipped while it runs, and `afterburner_tunes` decides whether to show the warning.
- **NVML has no "default" temperature limit.** The factory value comes from Afterburner's `[Defaults]`, saved in `gpu-defaults.json`.
- **A CPU profile's `plan` GUID is managed by the backend**: `cpu_save` keeps the stored value, and `ensure_plan` re-creates a deleted plan.
- **The minimum-clock lock lands about 8% under target**, because Windows maps the percentage a little low.
- **RTSS shared memory:** read the header first, then map the real size; mapping too large a size is refused.

---

## 1. How it runs

| | |
|---|---|
| Install location | `C:\Program Files\Deck\Deck.exe` |
| Runs as | **Administrator**, with no UAC prompt. It starts through scheduled tasks: `\Deck\Deck` runs at logon with `--minimized`, and `\Deck\Open` is what the Start-menu entry runs (via `run.vbs`). Apps that Deck launches still start as the normal user (`util::launch_as_user`), so Steam, games and Discord are not elevated. |
| Settings | `%APPDATA%\Deck\settings.json`, plus the folders listed in §7. |
| Opened by | Hot corner (top-left by default, 250 ms, off over fullscreen games), the tray icon, the Start menu, or the phone. An optional open hotkey (`open_hotkey`) exists but is empty by default. |
| Window modes | `panel`: a quick panel over a dimmed screen (the default). `app`: a normal window. The pin: in `panel` mode the panel becomes a floating always-on-top window the size of the panel (`PIN_RECT`); in `app` mode the window stays on top. |
| Look | `theme` (8 themes, `data-theme` on `<html>`), `accent`, `opacity_pinned` / `opacity_unpinned`, `open_anim` (`data-anim`), `low_spec` (class `low-spec`: no transitions or soft shadows, live tick every 3 s). The first-start picker ("Make Deck yours") shows until `setup_picked`. |
| Defaults | Home tile places and sizes: `HOME_DEFAULT` in `ui/js/layout.js` (used until the user arranges Home). PC tile cards: `pc_cards`. Sidebar: every group folded (`nav_folded`), width 180. |
| Phone remote | LAN web app / PWA on port **47830** (MicDrop uses 47823). It has a key check and a WebSocket for live state. Deck adds its own firewall rule ("Deck phone remote", private + domain) when admin. Away from home: the same page on the PC's Tailscale address (Away tab); `/tailscale` is a get-Tailscale page for the phone. Phones are counted once each, split into home and away (Tailscale or outside addresses), with optional limits per side and "one side shuts out the other" switches (Phone → Who can connect). |
| Screen on your phone | `screencast.rs`, `/screen` WebSocket: the PC's screen in the phone remote (DXGI duplication, GDI fallback, `jpeg-encoder`, ack flow control, 30/60 fps) and, in `control` mode with Control on in the phone, taps/typing via SendInput. `/audio`: the PC's sound (WASAPI loopback, PCM). `remote_screen` off by default. |
| Remote desktop | RustDesk (`rdesk.rs`, Phone page): Deck installs it, sets direct connections (port 21118), the password and hidden tray icons. Addresses: home IP, Tailscale IP, RustDesk ID. |

### User rules Deck follows
- **Menus over shortcuts.** The tray, panel and hot corner are the main way in. Hotkeys exist only for CPU/GPU profile switching and the optional open, overlay and boost keys.
- **No input hooks** (anti-cheat: Vanguard, EAC, BattlEye). Keys use `RegisterHotKey`. The hot corner polls the cursor. Window snapping uses WinEvent hooks plus `GetAsyncKeyState`. Nothing is ever injected into games.
- **Friends use it too.** No PC-specific names or lists. Pages hide when the PC has no matching hardware, and unverified fan chips are monitor-only.
- **Never change hardware while the user is in a game.** That covers tests and anything else that changes fans, clocks, the power plan, audio or display.
- **Never change the default recording device.** MicDrop owns it.

---

## 2. Pages (sidebar)

| Group | Pages |
|---|---|
| Top | **Home** (clock and calendar, now playing, PC vitals with sparklines, Performance tile to switch CPU/GPU profiles, sound, pinned and frequent apps, quick actions, clipboard, power and timers, network), **Game boost**. The Apps and Games pages were removed on 2026-10-12 at the user's request (apps and games are still found through the search box and the Home tile). The MicDrop tile and Deck's MicDrop link were removed the same day, also at the user's request. |
| Hardware | **CPU**, **GPU** (every setting in both profile editors has a "?" that explains what it does, why to use it and what changing it does; text in `ui/js/help.js`), **Fans**, **Lighting**, **Cooler display**. A page is hidden when the PC has nothing for it. |
| Monitor | **Sensors** (opens on All readings), **Overlay** (the in-game overlay settings; can install RivaTuner), **Drives** (S.M.A.R.T. health of every HDD/SSD with history in `drive-health.json`; optional alerts, `drive_alerts`, off by default), **Processes**, **Network** (one screen: live rates with a 1-minute graph, click-to-copy IPs, speed test, ping with Start / Stop (one round when the page opens, then rounds back to back only while started; leaving the page stops it), game-server ping that tests a game the first time it's picked), **This PC** (hidden when the PC has nothing for it, like the hardware pages) |
| Desktop | **Windows**, with tabs: Open windows (pin on top, see-through, snap, center, close), Snap zones (FancyZones-style), Workspaces. **Display** (refresh rate, DDC/CI brightness, HDR, dark/light schedule that switches only at its set times, so a manual switch holds). **Clipboard** (history of text, images and files, pins, search, "paste as"; skips password managers), **Sound** (default output, master and per-app volume, media sessions) |
| Bottom / Desktop | **PC to PC** (above Phone; id `transfer`) (AppBeam's engine in `src-tauri/src/beam/`, on by default: send files, folders, installers, apps and setups between PCs running Deck with the same pairing code; tabs Send, Apps, Setup, Received, Settings, Log); **Claude Code sync** under Desktop (id `claudesync`, shown only with `claude_sync_nav` or while on; ClaudeSync's engine in `src-tauri/src/ccsync/`, off by default: Claude Code sessions, memory and settings kept the same on your PCs; tabs Status, Sessions, Projects, Settings, Log) |
| Power & time | **Keep awake** (shows Windows' own sleep and screen-off times; "until downloads finish" waits for a download to start, then ends a minute after it stops), **Timers & breaks**, **Shut down when…** (time, app closes, downloads finish, CPU idle, network quiet, media stops, away; with a 60 s cancel warning). Downloads and media conditions wait until something downloads or plays; a time missed while the PC was asleep is skipped, not run on wake. |
| Maintenance | **Disk space** (NTFS drives are read straight from the MFT (`mft.rs`), others with a FindFirstFileEx walk; WizTree-style squarified treemap with File types / Folders views and a back button; Windows' own folders and files are locked against the Recycle Bin), **Clean up** (caches, plus **Clear RAM**), **Startup apps**, **Tools**, with tabs: Screen tools (colour picker, text from screen via OCR, ruler), Locked files, Windows panels |
| Removed | **Notes** and its Home tile (2026-10-12, at the user's request). Saved notes stay in `settings.json` (`notes`), untouched. The unlinked **File search** route (`files/index`) was removed on 2026-10-14; its settings live in Settings → Clipboard & search. |
| Bottom | **Phone**, **Settings** |

The search box covers Start-menu and Store apps, Deck actions, a calculator, unit, currency and number-base conversion, games, files (Deck's own index), clipboard and web. It also understands commands such as `timer 10m`, `sleep in 30m` and `kill discord`.

---

## 3. Hardware control

### CPU profiles (the CPU page)
A CPU profile is **a real Windows power plan plus extras that Deck applies itself**:

- **Plan values:** max clock for P-cores and E-cores, max and min processor state, turbo/boost mode, minimum cores awake, and EPP.
- **Extras:**
  - RAPL power limits PL1/PL2, either the Saver/Balanced/Performance presets or custom watts. Deck re-asserts them if the firmware resets them.
  - A minimum-clock lock. It lands about 8% under target, and a **heat guard** releases it above 92 °C.
  - A turbo-ratio overclock (unlocked Intel CPUs only). Deck re-applies it after a restart.
  - "Hold the cap in games": turns off Windows Game Mode while the profile is active.
- **Switching** is instant, through `PowerSetActiveScheme`. Deck watches for plan changes made elsewhere (every 2 s) and marks the matching profile as active.
- **If a profile's plan gets deleted**, the CPU page shows "Plan missing". Applying or saving the profile then **rebuilds the plan** from the profile's saved values, as "Deck · <name>".
- The maintainer's own profiles are listed in `PRIVATE_NOTES.md`.
- Idle states are never disabled, because that makes a CPU run hot when it has nothing to do.

### GPU profiles (the GPU page, NVIDIA)
A GPU profile works like an Afterburner slot:

- **What it sets:** power limit %, temperature limit, core and memory clock offsets **or** a full voltage/frequency curve, voltage %, an optional clock lock at one curve point, and fan mode (auto, fixed, or curve).
- **Driver calls:** NVML for the documented calls, NvAPI for the voltage/frequency curve and voltage.
- **Crash guard** (`gpu-applied.json`): if the PC restarted while an overclock was on, the next boot starts at stock and shows an "Apply anyway" toast.
- **Re-applying:**
  - If a driver reset puts the card back to stock, Deck re-applies the profile once.
  - Deck **re-applies the profile about 5 s after the PC wakes from sleep**, as Afterburner's "restore after suspended mode" did.
  - GPU fan settings are re-written every 10 s.
- **Temperature limit:** "card's own" puts back the card's factory value (for example 83 °C on an RTX 3070), saved in `gpu-defaults.json`. That file means it still works after Afterburner is uninstalled.
- **Afterburner import:** brings in the 5 slots, the startup profile, the Ctrl+Alt hotkeys and the overlay items. A **Stop Afterburner from tuning** button backs up Afterburner's settings and clears its startup profile, hotkeys and fan control, with **Undo**. The GPU page warns only while Afterburner could still override Deck.
- Imported curves match Afterburner's file point for point (127 points on the test card).

### Switching profiles
Every way of switching runs the same code:
- profile cards in the app;
- tray submenus;
- hotkeys;
- desktop and Start-menu shortcuts, each with an icon picked from 25 built-in icons and run through a `\Deck\Profile <kind>-<id>` task, so there is no UAC prompt;
- phone chips;
- Game boost.

### Fans, lighting, display and sensors (the engine from MoboCtl, in `src-tauri/src/hw/`)
- **Fans:** Super I/O chips (NCT668x/679x, ITE) through the embedded **PawnIO** driver, and HP laptop fans over WMI.
  - Modes are Auto (BIOS), Fixed %, or Curve, with a temperature source: CPU, GPU, hotter, average, or a board sensor.
  - Shared options: hysteresis, delay, and "stop system fans below".
  - The tray has **All fans** Silent / Balanced / Performance.
  - **Fans always go back to the BIOS** on quit, panic, log-off, `--release-fans`, and in the uninstaller.
- **Lighting:** MSI Mystic Light board RGB and Kingston Fury RAM RGB. Effects include music and screen-colour reactions and an auto "Game" preset (which can react only to the game's sound; off by default). Comet can use your colour, cycle through colours, or draw a rainbow tail. Screen colours come from DXGI desktop duplication (GDI fallback), so Screen/Game lighting costs well under 1 % CPU. The board loop re-checks every 5 s that the Mystic Light controller still holds Deck's config (feature 0x52, first 60 bytes) and rewrites it when not: without it the board ignores every colour and Deck shows no error.
- **Units:** Settings → General → Units picks MHz/GHz, Mb/s / MB/s and °C/°F for everything shown (pages, sensor table, phone, overlay). Typed-in values stay in MHz and °C.
- **Cooler display:** LCD coolers with USB id 0416:8001. Content options are sensors, clock, text, paint design, animation, music meter and rotate. This PC has none.
- **Sensors:** three tabs. *Graphs*: 5-minute graphs. *All readings*: the HWiNFO-style table, one device at a time (list on the left with each device's temperature and load). Per-core and per-fan rows are folded into one grid showing the current and highest values. Other rows are grouped by unit. Search covers every device. *Overlay & alerts*: the RTSS overlay items and key, heat alerts as toasts, and CSV logging to `monitor.csv`.
- **This PC:** a hardware report (Copy, or Save) and Test lights.
- Settings live in `%APPDATA%\Deck\hardware\settings.ini` and `curves.ini`, using MoboCtl's key names. If `C:\Program Files\MoboCtl` exists, Deck offers a one-time import.

### In-game overlay (RTSS)
- Deck writes its text into one RTSS overlay slot (owner "Deck"). RTSS draws it and fills in FPS and frame time.
- Deck also publishes Afterburner's `MAHMSharedMemory`, so RTSS OverlayEditor's "MSI Afterburner" data source keeps working.
- The items and key are set on the **Sensors** page.
- Settings: `osd_on`, `rtss_autostart`, `rtss_tray_hidden`, `osd_hotkey`, `osd_reset_hotkey`.
- **RivaTuner's tray icon** (`rtss_tray_hidden`): RivaTuner has no setting for it, so Deck removes the icon itself with `Shell_NotifyIcon(NIM_DELETE)`. RivaTuner's icon belongs to its window titled "RivaTunerStatisticsServer", with that window's handle as the icon id. The rtss loop repeats this every 5 s, because RivaTuner adds the icon back when it or Explorer restarts. Posting `TaskbarCreated` to that window brings the icon back with no restart. **Open RivaTuner** restarts RivaTuner. Its window is drawn properly only when RivaTuner opens it itself, on start or from its tray icon; shown from outside it stays black, and the tray icon can't be clicked from outside. A running game's overlay blinks for a moment.

---

## 4. Afterburner and MoboCtl

**MSI Afterburner** can be uninstalled once its profiles are imported: profiles, curves, hotkeys, fan curve, startup profile, the restore after sleep, the temperature default and the overlay items are all in Deck.
- If its uninstaller offers to remove RivaTuner as well, say **no**; RivaTuner draws the overlay.
- After uninstalling, turn on **Deck overlay** and **start RTSS with Deck** for the in-game overlay.
- Stop tuning backs Afterburner's settings up to `%APPDATA%\Deck\backups\afterburner-<time>`.

**MoboCtl** (the earlier fan/RGB tool) is fully absorbed: all 75 settings, every control, the tray items, the heat guard and the PawnIO driver. MoboCtl profiles bundled fans, lights and CPU into one snapshot; in Deck, fans are set separately (All fans in the tray, or the Fans page).

---

## 5. Code layout

| Path | What |
|---|---|
| `src-tauri/src/main.rs` | Tauri setup, about 80 commands, tray, show/hide, toast and zones windows, `icon://` protocol, command-line modes |
| `app.rs` | Shared `App` state, change events, toasts, starting all workers |
| `settings.rs` | The settings struct with atomic save. `DECK_PROFILE=test` switches to `%APPDATA%\Deck-test`. |
| `profiles/` | CPU (`cpu.rs`, `plans.rs`), GPU profile model, Afterburner import (`afterburner.rs`), hotkeys, shortcuts, icons |
| `gpu/` | `engine.rs` (apply, fan loop, drift and resume re-apply), `nvml.rs`, `nvapi.rs`, `curve.rs`, `guard.rs` (crash guard) |
| `hw/` | MoboCtl's engine: `sio`, `hp`, `pawn` (+ `mods/`), `sensors`, `sysmon`, `smbios`, `fx`, `hid`, `devices`, `lcd`, `loopback`, `screen`, `game`, `cpupower`, `powermode`, `rapl`, `memclean`, `cfg`, `kv` |
| `profcmd.rs`, `hwcmd.rs` | Commands for the Hardware pages |
| `rtss.rs` | RTSS overlay slot and MAHM shared memory; installs RTSS (winget); flips RTSS's own OSD switch through the 32-bit helper |
| `rtss32/` | The 32-bit helper (`rtss-osd32.exe`) that calls RTSSHooks' SetFlags; `build.rs` builds it for i686 into `target/rtss32` and embeds it (needs `rustup target add i686-pc-windows-msvc`) |
| `mft.rs`, `disk.rs` | NTFS MFT reader, folder tree and treemap layout for Disk space |
| `hw/smart.rs` | Drive health (NVMe log page 2, ATA SMART attributes and thresholds), history and alerts |
| `support.rs` | The support report (This PC) |
| `boost.rs` | Game boost (close apps, power plan, priorities, trim; everything restored afterwards) |
| `beam/` | Transfer (from AppBeam): `net.rs` (UDP beacon 47833 + encrypted TCP 47834, hello `DKT1`; offers, files, resume, projects, requests), `config.rs` (settings, inbox, live state; `off` stops announcing, answering and accepting), `apps.rs` (winget), `setup.rs` + `snapshot.rs` (installers, setup snapshots, Windhawk), `mod.rs` (Deck's glue: `set_on`, `state`, `act`, notifications with Accept/Decline). Deck-only: the standalone AppBeam (47831/47832) doesn't talk to it |
| `ccsync/` | Claude Sync (from ClaudeSync): `engine.rs` (sync cycles, merge, forks, deletions, locks), `lan.rs` (UDP 47835 + TCP 47836, hello `DKC1`), `hub.rs` (cloud folder `Deck-ClaudeSync-hub`), `canon.rs` + `scan.rs` (path tokens per project), `locks.rs` (the hooks: `%LOCALAPPDATA%\Deck\claude-sync-hook.exe --claude-sync-hook`, a copy of deck.exe so updates aren't blocked), `mod.rs` (glue; imports the standalone app's settings once; refuses to start while `ClaudeSync.exe` runs; `--uninstall-cleanup` from `hooks.nsh`) |
| `webview_guard.rs` | WebView2 browser flags that prevent blank windows (applied to every window builder) plus the watchdog |
| Other modules | `sys`, `audio`, `media`, `apps`, `calc`, `files`, `clipboard`, `power`, `winmgr`, `disk`, `cleanup`, `startup`, `display`, `net`, `gameping`, `locksmith`, `games`, `picker`, `hotcorner`, `remote`, `screencast` (screen view and control for the phone), `rdesk` (RustDesk + Tailscale), `elevation`, `util` |
| `ui/` | `index.html`, `app.css`, `guard.js`, and `js/` (`core` helpers, `app` nav and boot, `home`, `search`, `pages1-3`, `hw`, `curve`, `profiles`, `help` (the "?" texts), `boost`, `icons`), plus `toast.html` and `zones.html` |

---

## 6. Build, install, test

```powershell
cd src-tauri
cargo test --release      # 118 tests (+29 ignored live tests)
cargo build --release     # set CARGO_TARGET_DIR to build elsewhere
..\install.ps1 -NoBuild   # elevates itself, quits Deck, copies to Program Files, (re)creates the tasks
..\uninstall.ps1 [-Purge] # quits Deck (fans back to the BIOS), removes tasks, shortcuts, files and the PawnIO driver if Deck installed it
```

- **UI changes need a rebuild**, because the UI is embedded in the exe. `install.ps1` deletes Tauri's cached asset copies first, since a crashed build once left them broken.
- **Setup:** `cargo tauri build --bundles nsis` makes `Deck_<version>_x64-setup.exe`; attach it to a GitHub release with the `CHANGELOG.md` section as the notes.
- **Command-line modes:**
  - `--quit`: graceful exit.
  - `--release-fans`.
  - `--minimized`.
  - `--switch`: profile switching, used by the shortcuts.
  - `--hw-selftest`, `--cpu-probe`, `--gpu-probe [--write] --out f`.
  - `--dump-state`, `--dump-hw`, `--dump-icons`: read-only, used for UI previews.
  - `--selftest` and `--selftest-remote`: these use the test profile and bind to 127.0.0.1.
- **Live testing:** see §0 and the scripts in `tools\testing\`.
- **Full live test pass on 2026-10-12:** CPU profiles under Cinebench, GPU profiles under Heaven, fans, clock lock, RTSS slot, hotkeys, Game boost, phone remote, and all 24 pages without JS errors.

---

## 7. Data files (`%APPDATA%\Deck\`)

| File | What |
|---|---|
| `settings.json` | All app settings and profiles |
| `hardware\settings.ini`, `curves.ini` | Fans, lighting, display, alerts, fan curves |
| `gpu-applied.json` | GPU crash guard |
| `drive-health.json` | Drive health history (Drives page) |
| `gpu-stock-curve.json` | Stock voltage/frequency curve cache |
| `gpu-defaults.json` | The card's factory temperature limit (per PCI id) |
| `boost.json` | Game boost state to restore |
| `backups\afterburner-*` | Afterburner settings saved before Stop tuning |
| `clipboard\` | Clipboard history |
| `monitor.csv` | Sensor log, when enabled |
| `transfer\` | Transfer: `config.json` (pairing code, shared folders, per-PC policy), `inbox.json`, `log.txt`, `outgoing\` (packed app lists and setups) |
| `claude-sync\` | Claude Sync: `config.json` (sync key, projects), `state.json` (merge bases), `hashcache.json`, `locks.json` + `identity.json` (read by the hooks), `trash\`, `conflicts\`, `claudesync.log` |
| `C:\ProgramData\Deck\pawnio\` | PawnIO driver files (the service is `DeckPawnIO`) |

---

## 8. Known issues

Hardware quirks of the maintainer's PC are in `PRIVATE_NOTES.md`.

**Deferred small items from code review** (2026-10-12 feature audit fixed the crash guard on quit, the MAHM double writer and the `rtss::stop` / CPU plan-switch races; see `docs/audits/2026-10-12-feature-audit.md`):
- The "Try it" preview is lost when you alt-tab away.
- The undervolt "risky" rule (a raised point at 850 mV or more) errs on the safe side; left as is.
- Quit measured at 2.1 s (Game boost off); `elev-install.ps1` now logs the quit time. The old "up to 20 s" note was not reproduced.
- NVML is opened in three places.
- A "Shut down when…" plan does not survive a Deck restart.
- The minimum-clock lock lands about 8% under target.
- `toggle_window` uses a special event.

**Ideas not built:**
- A built-in overlay that doesn't need RTSS: a transparent click-through window, with FPS read from Windows' frame-timing events. It would work in borderless and windowed games but not exclusive fullscreen.
- Letting a CPU profile switch the fans too.

**Turned down, don't re-propose unless asked:** NetLimiter-style bandwidth limits, and remote desktop from outside the home network.

**Backlog:**
- Save guardian, death counter and backlog picker as Deck pages.
- Tile reordering, a custom layout editor, text snippets.

---

## 9. Before every commit (mandatory)

**Claude: do this every time, without being asked, before running `git commit`.** It is the rule at the top of §0 and in `CLAUDE.md`, repeated here so it can't be missed.

1. `PROJECT_STATE.md`: add or extend today's dated entry: what changed, why, what was tested (and what wasn't).
2. `CHANGELOG.md`: add user-visible changes under the version being worked on (an `Unreleased` heading if no version is set). These become the release notes.
3. `DECK.md`: update any fact that changed (pages, settings, commands, test counts, file layout, gotchas).
4. `README.md`: update the feature list, screenshots or build steps if they changed.
5. Check none of them contains personal data (paths, hostnames, PC specs, games, IPs). Personal facts go in `PRIVATE_NOTES.md`.
6. Stage the docs in the **same** commit as the code.

The hook in `.githooks/pre-commit` (enabled with `git config core.hooksPath .githooks`) refuses a commit that changes code without staging `PROJECT_STATE.md` and `CHANGELOG.md`. If it blocks you, update the docs; never use `--no-verify`.
