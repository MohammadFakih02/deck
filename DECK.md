# Deck

Deck is a single Windows tray app that brings together a launcher, a live dashboard, PowerToys-style tools, full hardware control, and a phone remote. It replaces **MoboCtl** (fans, RGB, cooler display, sensors) and **MSI Afterburner** (GPU tuning). **RivaTuner (RTSS)** stays installed because it draws the in-game overlay.

- **Version:** 2.7.5. Setup: `cargo tauri build --bundles nsis`, attached to each [GitHub release](https://github.com/MohammadFakih02/deck/releases).
- **Stack:** Tauri 2, Rust backend, vanilla JS UI. No frameworks, no build step for the UI.
- **Source:** https://github.com/MohammadFakih02/deck (public). Machine-specific notes (the maintainer's PC, local paths, personal profiles) live in `PRIVATE_NOTES.md`, which is git-ignored; read it first if it exists.
- **Detailed history:** dated notes are in `PROJECT_STATE.md`.
- **Hardware design:** the spec, plan and decision ledger are in `docs\design\`. Audit reports are in `docs\audits\`.
- **Security:** what the elevated process trusts and where each check lives is in `docs\trust-boundaries.md`; reporting is in `SECURITY.md`.
- **User-facing changes:** `CHANGELOG.md` (also the release notes).

---

## 0. For contributors

Read this section first. If `PRIVATE_NOTES.md` exists (the maintainer's machine only), read it too.

> **RULE: update the docs before every commit.** **Before every commit, update the Markdown docs** (`PROJECT_STATE.md` always, `CHANGELOG.md` for anything a user would notice, `DECK.md` / `README.md` when facts change) and stage them in the same commit. The pre-commit hook in `.githooks/` refuses code commits without them; never bypass it with `--no-verify`.

### Who Deck is for
- People who play online games with kernel anti-cheat (Vanguard, EAC, BattlEye), so Deck must never hook input or inject into games.
- Deck is shared with friends and is public, so everything must work on other PCs: no names, paths or ids specific to one PC in the code or the docs. Personal facts go in `PRIVATE_NOTES.md` only.
- Looks matter. Check UI changes with a CDP screenshot or an offscreen render before calling them done.

### Hard rules
1. **Never capture the real screen.** Screenshots of Deck's own page over CDP (`cdp.mjs shot`) are fine.
2. **Don't change hardware while the user may be in a game.** That includes fans, clocks, power plans, audio and display. Check what's running first (look for game processes and fullscreen windows), and ask if unsure. During an agreed test session it's fine; keep every load test under 2 minutes.
3. **No input hooks, no injection into games.** Use `RegisterHotKey`, cursor polling, WinEvent hooks or `GetAsyncKeyState` only. One exception, agreed with the user on 2026-10-18: the controller overlay's keyboard & mouse mode reads Windows **Raw Input**, receive-only (`RIDEV_INPUTSINK`, never `RIDEV_NOLEGACY`, so nothing is blocked or changed; games read input the same way), registered only while that mode is on (`pad/input.rs`). The one place Deck sends input is the phone's Screen control (SendInput, user-driven), and it pauses while `boost::game_in_front` sees a game.
4. **Never change the default recording device**, with one exception agreed with the user on 2026-10-20: the phone mic's own switch (`remote_mic_default`, off by default) makes VB-CABLE's "CABLE Output" the default mic while a phone talks and puts the old one back after (also after a crash, `phonemic::restore_after_crash`). Nothing else touches it.
5. **Test modes use `DECK_PROFILE=test`** (`%APPDATA%\Deck-test`) and bind to 127.0.0.1. A server on 0.0.0.0 makes a Windows Firewall prompt pop up on the user's screen.
6. **Git:** Deck is on GitHub (`MohammadFakih02/deck`, branch `main`). Commit messages say what changed. Commit only when asked.
   - **Before every commit, update the docs** (see the rule at the top of this section and §9). This is not optional: the pre-commit hook blocks code commits that don't stage `PROJECT_STATE.md` and `CHANGELOG.md`.
   - Never commit personal data: no local paths, hostnames, PC specs, game lists or screenshots of the real desktop. `artifacts/`, `private/` and `PRIVATE_NOTES.md` are git-ignored on purpose.
7. **Hotkeys are an exception, not the main interaction.** Prefer menus, the tray and the panel. Hotkeys are only for profile switching and the optional open, overlay and boost keys.
8. Deleting user data (Clean up for real, purging settings) needs the user's OK first.
9. **Don't re-propose** NetLimiter-style limits; the user declined them. Away-from-home access goes through Tailscale (never by opening ports to the internet); Sunshine + Moonlight is a possible later option next to RustDesk.

### Workflow
- **Build:** `cargo build --release` in `src-tauri`. Set `CARGO_TARGET_DIR` to build elsewhere (the maintainer's value is in `PRIVATE_NOTES.md`). A build takes about 2 minutes. **The UI is embedded in the exe, so UI changes also need a rebuild and reinstall.**
- **Tests:** `cargo test`, run in `src-tauri` (dev profile: dependencies are optimised once, so a rerun rebuilds only Deck); 344 should pass (35 ignored live tests, among them `ocr_reads_a_drawn_death_banner`, `a_drawn_death_screen_counts_once` and `a_wordless_death_screen_is_learned_from_captured_frames`, which check Windows' OCR, the whole screen-detection path and teaching on a window of Deck's own (run the last two one at a time); which are safe to run is in the T4 table of `docs/audits/2026-10-17-test-plan.md`). `cargo clippy --all-targets -- -D warnings` must be clean (CI fails otherwise). Check the JS with `node --check ui/js/*.js`.
- **Install and run:** Deck runs elevated, so installing needs admin. `Start-Process powershell -Verb RunAs -WindowStyle Hidden -File tools\testing\elev-install.ps1` (one UAC prompt unless UAC is off). That script quits Deck, runs `install.ps1 -NoBuild`, and starts Deck **with the CDP debug port 9222**. Always finish with `tools\testing\restart-normal.ps1`, **run elevated as well** (`Start-Process powershell -Verb RunAs -File ...`; from a normal shell its `--quit` can't reach the elevated Deck, so the debug instance silently keeps running). It quits Deck and runs the `\Deck\Open` task. Then check that port 9222 is no longer listening.
  - In Git Bash, run `powercfg` and `schtasks` with `MSYS_NO_PATHCONV=1`, because Git Bash rewrites `/flags` and `\paths`.
- **Live testing** (`tools\testing\`):

  | Script | What it does |
  |---|---|
  | `cdp.mjs` | `node cdp.mjs eval "<js>"` runs JS in Deck's page; `invoke("cmd", {args})` calls a backend command and `go(page, sub)` navigates. `node cdp.mjs shot out.png` screenshots Deck's page. The UI is ES modules, so its names aren't globals; `ui/js/main.js` puts the ones these scripts use on `window` (`S`, `T`, `Pages`, `invoke`, `go`, `render`, `tick`, `setSetting`, `confirmBox`, `modal`, `h`, `I`, `editCpu`, `editGpu`, `ovStyleNow`, `ovRefresh`). The page only ticks when `body` has class `shown` and the document is visible. |
  | `ui-preview/regress.mjs` | UI regression checks (40, about 15 s) with real mouse and keyboard input: clicks not lost to live or event redraws or on the sidebar, search Enter uses the current query, Keep awake / Shut down when… flows, Recycle Bin never pre-ticked in Clean up, no trash button on Windows' own folders in Disk space, profile editors kept open when Deck hides, the merged sidebar, ping only between Start and Stop, a "?" on every CPU/GPU editor setting, the overlay font list, focus kept across redraws, named controls, dialog focus trap, Space on buttons. Waits run at 30% (`SLEEP_SCALE=1` restores full waits on a slow PC). Run after UI changes; every line should say PASS. `DECK_UI=<folder>` runs them against another copy of `ui/`. |
  | `ui-preview/toast-test.mjs` | The notification window: toasts under the mouse stay put and don't time out; held changes run when the mouse leaves. |
  | `ui-preview/a11y.mjs [w h] [out.json]` | Visits every page and counts unnamed buttons, unlabelled fields, targets under 24 px, sideways overflow, script errors and page-switch time (about 5 s). |
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
- **The CDP debug port comes from `--cdp-port`, not the environment.** An elevated Deck scrubs every `WEBVIEW2_*` variable at the top of `main()` and ignores it in `browser_args` (a user-level program could otherwise set `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=…` and debug the admin process). Live-testing scripts pass `--cdp-port=9222` on Deck's command line instead; only an unelevated Deck still honours the env var.
- **Persist files through `util::write_atomic`** (unique temp + `sync_all` + rename by handle, through `fsguard::write_for_user`), not `fs::write`, so a crash mid-write can't truncate the file and an elevated Deck writes only where the signed-in user could. Every settings and state store uses it, including `ccsync` (`hub`, `config`, `locks`, `scan`, `engine` state) and the hardware `.ini` files.
- **The elevated process never acts on a user-writable path by name.** Use `fsguard` (`open_for_user`, `write_for_user`, `move_for_user`, `open_read_for_user`, `user_may_delete`, and `Dir` for work inside one folder). It checks the action against the user's limited token and works relative to an open folder handle, so a folder swapped for a junction can't redirect it. See `docs/trust-boundaries.md`.
- **Hardware settings are sealed.** After saving `settings.json` (its hardware part) or a hardware `.ini`, `seal::record` / `seal::record_ini` stores a fingerprint in `HKLM\SOFTWARE\Deck\Seals`. A new hardware setting belongs in `seal::hardware_part`, or an outside edit to it won't be noticed. At an admin start a mismatch holds the hardware (`HW_HELD`) until the user applies it.
- **Vendor DLLs load by full path** through `util::load_trusted_dll` (System32, or NVSMI for `nvml.dll`). NVML is opened once per process (`gpu::nvml::Nvml::shared()`); the `nvml-wrapper` crate is gone.
- **Per-thread clocks come from PDH, not `sysinfo`.** `sysinfo`'s `frequency()` on Windows is each core's nominal clock (`CurrentMhz`), never the live one. Use `hw::sensors::ThreadClocks` / `thread_mhz`: "Processor Frequency" × "% Processor Performance" per instance, as Task Manager does. With the driver, every thread's clock comes from its own cycle counters instead (APERF/MPERF over the TSC rate, `counted_mhz` in `Cpu::clock`), PDH fills in for a thread the counters have nothing for. That is the average clock a core ran at while awake (HWiNFO's "effective clock"); it feeds the CPU page, Sensors and the phone. **The overlay shows MSI Afterburner's figure instead:** each thread's current ratio (`IA32_PERF_STATUS` bits 15:8) × the bus clock (TSC MHz / `IA32_PLATFORM_INFO` base ratio = 99.85 on an i5-14600KF), `ratio_mhz` / `Cpu::ratio_clocks` → `Live::thread_ratio_mhz`, Intel with the driver only. It holds at the turbo step while the core works (5292) and drops to the lowest ratio (799) when the read lands on a sleeping core, exactly as Afterburner shows it; the cycle counters gave 4.8-5.2 GHz on the same CPU because the step moves within each second.
- **UI modules:** a top-level name another file uses must be `export`ed and `import`ed. Import cycles are fine for functions but not for values read while a module loads: `Pages` lives in `core.js` for that reason, and `boot()` runs from `main.js` after every module has loaded.
- **Tauri caches compressed UI assets** and never rewrites them. A crash mid-build leaves broken pages; `install.ps1` clears the cache.
- **The page can miss the "shown" event at startup.** `app.js` `catchUp()` asks `get_state.visible` at boot, on focus and on visibility change, so the page still starts ticking.
- **Afterburner running fights Deck.** The GPU drift and resume re-apply are skipped while it runs, and `afterburner_tunes` decides whether to show the warning.
- **NVML has no "default" temperature limit.** The factory value comes from Afterburner's `[Defaults]`, saved in `gpu-defaults.json`.
- **A CPU profile's `plan` GUID is managed by the backend**: `cpu_save` keeps the stored value, and `ensure_plan` re-creates a deleted plan.
- **The minimum-clock lock lands about 8% under target**, because Windows maps the percentage a little low.
- **RTSS shared memory:** read the header first, then map the real size; mapping too large a size is refused.
- **ureq 2 only makes 4xx/5xx errors.** A `304 Not Modified` (an `If-None-Match` request) arrives as `Ok` with an empty body: check `resp.status() == 304` in the `Ok` arm (the updater said "GitHub's answer wasn't understood" on every check until 2.7.5).
- **ureq's `.timeout()` covers the whole body.** For downloads use `timeout_read`/`timeout_write` (a stalled connection ends it) and keep `.timeout()` for small API answers: at 190 KB/s a 10 MB setup takes about a minute.
- **Microsoft's CDN gives one connection only so much** (about 190 KB/s here, eight together about 1.1 MB/s): `tweaks::storedl` downloads in 8 ranged parts.
- **Every program Deck runs has a time limit:** `util::run_for(secs, …)` (`run_hidden` = 30 min). DISM without admin, RustDesk's `--get-id` and a stuck `tailscale status` can wait forever.
- **PowerShell strings go through `rdesk::ps_str`**, which doubles every kind of single quote (PowerShell also takes the typographic ones): a user folder can be "O'Brien".
- **A process id can be reused within seconds.** Before ending a remembered pid, check it is still the same exe (`boost::same_app`).
- **Tweaks' undo journal is sealed** (`seal` name `tweaks-journal`) and undo only restores what the tweak's own recipe touches (`tweaks::before_allowed`): it writes with admin rights from a user-writable file. Registry tests run on the scratch key (`tweaks::steps::use_scratch`), never on the real registry.
- **A listener's accept task must end with its server.** `phonetls::Mixed` ends on `tx.closed()`; before 2.7.5 turning the phone remote off left the port open, and the next start took port + 1.

---

## 1. How it runs

| | |
|---|---|
| Install location | `C:\Program Files\Deck\Deck.exe`, from both the release setup (per-machine NSIS, one UAC prompt; `hooks.nsh` removes an older per-user copy in `%LOCALAPPDATA%\Deck`) and `install.ps1`. |
| Runs as | **Administrator**, with no UAC prompt. It starts through scheduled tasks: `\Deck\Deck` runs at logon with `--minimized`, `\Deck\Open` is what the Start-menu entry runs (via `run.vbs`), `\Deck\Background` starts it hidden (the setup, the watchdog's restart, the updater) and `\Deck\Quit` closes it for the setup. The setup creates all four itself (`hooks.nsh` runs `deck.exe --setup-task`; it is elevated), and an unelevated start while they exist hands over to `Open`/`Background` (`elevation::handover`). Set up refuses to create the tasks for a copy the user could change, and an admin start removes tasks that point at one ("Deck's admin start was turned off"). Apps that Deck launches still start as the normal user (`util::launch_as_user`, which never falls back to an elevated launch), so Steam, games and Discord are not elevated. |
| Watchdog | When the hardware engine starts, Deck starts a second `deck.exe --watchdog <pid>`. If Deck is ended from outside (Task Manager, `taskkill /f`), the watchdog gives fans back to the BIOS and the GPU fan back to the driver and logs it. A clean exit sets `Local\Deck-watchdog-<pid>` so it stands down. |
| Log | `%LOCALAPPDATA%\Deck\logs\deck.log` (`log.rs`; 2 MB per file, 4 older files kept). Lines: `UTC time.ms [level] [thread] area: message`; helper processes prefix their role (`watchdog/main`, `update/…`, `restart/…`, `setup/…`). Levels: `error` (written at once), `warn`, `info`, `debug` (only with a file named `debug` in the log folder or `DECK_LOG=debug`). Every line is masked on the way in (values after `token` / `password` / `secret` / `api_key` / `Bearer` / `pairing code` …, the user's name in paths), the same line again within 60 s is counted instead of written, and a background thread writes (`flush()` on crash, quit and every `exit_logged`). Failed web requests (`log::http`, host and path only), programs that exit with an error (`util::run_for`), failed UI commands and page script errors (`ui_log`, command name only), the phone remote's refusals, profile switches, updates, hardware hand-backs and share links are all logged. Its last 80 lines go into the This PC support report. Transfer (`log.txt`, 2 × 2 MB) and Claude Sync (`claudesync.log`) keep their own logs and copy them into this one at `debug`. |
| Settings | `%APPDATA%\Deck\settings.json`, plus the folders listed in §7. |
| Opened by | Hot corner (top-left by default, 250 ms, off over fullscreen games), the tray icon, the Start menu, or the phone. An optional open hotkey (`open_hotkey`) exists but is empty by default. |
| Window modes | `panel`: a quick panel over a dimmed screen (the default). `app`: a normal window. The pin: in `panel` mode the panel becomes a floating always-on-top window the size of the panel (`PIN_RECT`); in `app` mode the window stays on top. |
| Look | `theme` (8 themes, `data-theme` on `<html>`), `accent`, `opacity_pinned` / `opacity_unpinned`, `open_anim` (`data-anim`), `low_spec` (class `low-spec`: no transitions or soft shadows, live tick every 3 s). The first-start picker ("Make Deck yours") shows until `setup_picked`. |
| Defaults | Home tile places and sizes: `HOME_DEFAULT` in `ui/js/layout.js` (used until the user arranges Home). PC tile cards: `pc_cards` (cpu, ram, gpu, vram, net), chosen with the tile's settings icon. The CPU card's clock, power and temperature come from the hardware engine, which runs only as admin. Without it the card shows the thread count and a *Needs admin* link (`elevate_setup`). Sidebar: every group folded (`nav_folded`), width 180. |
| Phone remote | LAN web app / PWA on port **47830** (MicDrop uses 47823). The phone key never travels: the page opens a session and signs every request (HMAC-SHA256 over session, counter and path; each counter works once; `phoneauth.rs`). Requests must name this PC (Host check) and the page has a hash-based CSP. A WebSocket carries live state. Deck adds its own firewall rule ("Deck phone remote", private + domain) when admin. Away from home: the same page on the PC's Tailscale address (Away tab); `/tailscale` is a get-Tailscale page for the phone. Phones are counted once each, split into home and away (Tailscale or outside addresses), with optional limits per side and "one side shuts out the other" switches (Phone → Who can connect). |
| Screen on your phone | `screencast.rs`, `/screen` WebSocket: the PC's screen in the phone remote (DXGI duplication, GDI fallback, `jpeg-encoder`, ack flow control, 30/60 fps) and, in `control` mode with Control on in the phone, taps/typing via SendInput. `/audio`: the PC's sound (WASAPI loopback, PCM). `remote_screen` off by default. |
| Phone as speaker and mic | `phonemic.rs`, Phone page → Speaker and mic (`remote_speaker`, `remote_mic`, both off by default). Speaker: `/audio?spk=1`, the same loopback stream without the Screen view; the phone can ask for the PC to be quiet (`{"t":"quiet"}`), which mutes the default output while it plays (loopback is taken before the mute) and follows a new default output. Mic: `/mic?rate=` WebSocket, 16-bit mono 10 ms pieces from an AudioWorklet (`/mic-worklet-1.js`), played into VB-CABLE's "CABLE Input" (a cushion of 60 ms that grows 20 ms after each gap up to 200 ms and shrinks 10 ms after 30 s without one; what piles past it + 120 ms is dropped; the player ends when the cable stops answering, and the phone reconnects and says why); apps hear "CABLE Output". Windows only lists a mic a driver makes, Deck has no signed driver, and test signing breaks anti-cheats, so VB-CABLE it is; its licence forbids building it into another setup, so Deck turns on an installed one (`pnputil /enable-device`) or downloads VB-Audio's package unchanged, checks it is signed by VB-Audio's author and opens VB-Audio's own setup. What Deck muted or switched is written to `phone-quiet.json` / `phone-mic.json` first and undone at start. Over Wi-Fi only (a web page can't do Bluetooth audio). |
| Phone HTTPS | `phonetls.rs`: the phone port answers HTTP and HTTPS (first byte 22 = TLS), handshakes on their own tasks. Phones allow the mic (and AudioWorklet, WebCodecs, wake lock) only on secure pages, so Deck makes a self-signed certificate for this PC's addresses (`phone-tls.json`, key sealed with DPAPI; remade, keeping the old names, when the address isn't on it). The plain page offers "Mic: open the secure page", which opens `https://<same host>/#k=<key>`; the phone warns once. |
| Remote desktop | RustDesk (`rdesk.rs`, Phone page): Deck installs it, sets direct connections (port 21118), the password and hidden tray icons. Addresses: home IP, Tailscale IP, RustDesk ID. |

### User rules Deck follows
- **Menus over shortcuts.** The tray, panel and hot corner are the main way in. Hotkeys exist only for CPU/GPU profile switching and the optional open, overlay and boost keys.
- **No input hooks** (anti-cheat: Vanguard, EAC, BattlEye). Keys use `RegisterHotKey`. The hot corner polls the cursor. Window snapping uses WinEvent hooks plus `GetAsyncKeyState`. The controller overlay reads controllers through SDL3 and, in keyboard & mouse mode only, keys and mouse through receive-only Raw Input. Nothing is ever injected into games.
- **Friends use it too.** No PC-specific names or lists. Pages hide when the PC has no matching hardware, and unverified fan chips are monitor-only.
- **Never change hardware while the user is in a game.** That covers tests and anything else that changes fans, clocks, the power plan, audio or display.
- **Never change the default recording device**, with one exception agreed with the user on 2026-10-20: the phone mic's own switch (`remote_mic_default`, off by default) makes VB-CABLE's "CABLE Output" the default mic while a phone talks and puts the old one back after (also after a crash, `phonemic::restore_after_crash`). Nothing else touches it.

---

## 2. Pages (sidebar)

| Group | Pages |
|---|---|
| Top | **Home** (clock and calendar, now playing, PC vitals with sparklines, Performance tile to switch CPU/GPU profiles, sound, pinned and frequent apps, quick actions, clipboard, power and timers, network), **Game boost**. The Apps and Games pages were removed on 2026-10-12 at the user's request (apps and games are still found through the search box and the Home tile). The MicDrop tile and Deck's MicDrop link were removed the same day, also at the user's request. |
| Hardware | **CPU**, **GPU** (every setting in both profile editors has a "?" that explains what it does, why to use it and what changing it does; text in `ui/js/help.js`), **Fans**, **Lighting**, **Cooler display**. A page is hidden when the PC has nothing for it. |
| Monitor | **Sensors** (opens on All readings), **Overlay** (the in-game overlay settings; can install RivaTuner), **Controller** (the controller overlay and controller batteries, from the merged Controller Overlay app; tabs Overlay, Keys, Battery, Test), **Deaths** (the death counter: deaths per game and per boss in any game; tabs Now, Bosses, History, Games, Settings), **Drives** (S.M.A.R.T. health of every HDD/SSD with history in `drive-health.json`; optional alerts, `drive_alerts`, off by default), **Processes**, **Network** (one screen: live rates with a 1-minute graph, click-to-copy IPs, speed test, ping with Start / Stop (one round when the page opens, then rounds back to back only while started; leaving the page stops it), game-server ping that tests a game the first time it's picked), **This PC** (hidden when the PC has nothing for it, like the hardware pages) |
| Desktop | **Windows**, with tabs: Open windows (pin on top, see-through, snap, center, close), Snap zones (FancyZones-style), Workspaces. **Display** (refresh rate, DDC/CI brightness, HDR, dark/light schedule that switches only at its set times, so a manual switch holds). **Clipboard** (history of text, images and files, pins, search, "paste as"; skips password managers), **Sound** (default output, master and per-app volume, media sessions) |
| Bottom / Desktop | **PC to PC** (above Phone; id `transfer`) (AppBeam's engine in `src-tauri/src/beam/`, on by default: send files, folders, installers, apps and setups between PCs running Deck with the same pairing code; tabs Send, Apps, Setup, Received, Settings, Log); **Claude Code sync** under Desktop (id `claudesync`, shown only with `claude_sync_nav` or while on; ClaudeSync's engine in `src-tauri/src/ccsync/`, off by default: Claude Code sessions, memory and settings kept the same on your PCs; tabs Status, Sessions, Projects, Settings, Log) |
| Power & time | **Keep awake** (shows Windows' own sleep and screen-off times; "until downloads finish" waits for a download to start, then ends a minute after it stops), **Timers & breaks**, **Shut down when…** (time, app closes, downloads finish, CPU idle, network quiet, media stops, away; with a 60 s cancel warning). Downloads and media conditions wait until something downloads or plays; a time missed while the PC was asleep is skipped, not run on wake. |
| Maintenance | **Disk space** (NTFS drives are read straight from the MFT (`mft.rs`), others with a FindFirstFileEx walk; WizTree-style squarified treemap with File types / Folders views and a back button; Windows' own folders and files are locked against the Recycle Bin), **Clean up** (caches, plus **Clear RAM**), **Startup apps**, **Tweaks** (Windows settings with live state and exact undo, built-in apps, Store apps without the Store; see the table below), **Tools** (one page: Screen tools (colour picker, text from screen via OCR, ruler), Locked files, Windows panels; `go("tools", "locks")` scrolls to its part) |
| Removed | **Notes** and its Home tile (2026-10-12, at the user's request). Saved notes stay in `settings.json` (`notes`), untouched. The unlinked **File search** route (`files/index`) was removed on 2026-10-14; its settings live in Settings → Clipboard & search. |
| Bottom | **Phone**, **Settings** |

The search box covers Start-menu and Store apps, Deck actions, a calculator, unit, currency and number-base conversion, games, files (Deck's own index), clipboard and web. It also understands commands such as `timer 10m`, `sleep in 30m` and `kill discord`.

---

## 3. Hardware control

### CPU profiles (the CPU page)
A CPU profile is **a real Windows power plan plus extras that Deck applies itself**:

- **Plan values:** max clock for P-cores and E-cores, max and min processor state, turbo/boost mode, minimum cores awake, and EPP. A cap at or above the CPU's known top turbo is treated as no cap (`CpuProfile::without_silent_caps`, applied by `cpu::effective` on save, plan writes and extras).
- **Extras:**
  - RAPL power limits PL1/PL2, either the Saver/Balanced/Performance presets or custom watts. Deck re-asserts them if the firmware resets them.
  - A minimum-clock lock. It lands about 8% under target, and a **heat guard** releases it above 92 °C.
  - A turbo-ratio overclock (unlocked Intel CPUs only). Deck re-applies it after a restart.
  - "Hold the cap in games": turns off Windows Game Mode while the profile is active.
- **The page** (`ui/js/profiles.js`): profiles and an on-page editor (draft in `S.cache.cpuEdit`) on the left; a sticky live column on the right fed by `cpu_live` (clock, temperature, load, power, `limits` / `board` / `limits_by_deck` from the power thread's 2 s RAPL read, `cores` from the sensor table). "Held at the power limit" (`heldByPower`): load ≥ 60 % and power ≥ 95 % of PL1 (or of PL2 when higher). Each profile in `cpu_state` carries `plan_epp`, the plan's own EPP when the profile leaves it to the plan.
- **Switching** is instant, through `PowerSetActiveScheme`. Deck watches for plan changes made elsewhere (every 2 s) and marks the matching profile as active.
- **If a profile's plan gets deleted**, the CPU page shows "Plan missing". Applying or saving the profile then **rebuilds the plan** from the profile's saved values, as "Deck · <name>".
- The maintainer's own profiles are listed in `PRIVATE_NOTES.md`.
- Idle states are never disabled, because that makes a CPU run hot when it has nothing to do.
- **Plugged in / on battery** (PCs with a battery): `cpu_plugged_profile` / `cpu_battery_profile` are switched to when the power source changes (`cpu::power_tick`, every 2 s; never while hardware is held by the seal or Game boost is on). `cpu_battery_same` (default on) makes switching write the profile's values for battery too; off keeps each plan's own battery values (Deck edits still reach them). Both profile choices are part of the sealed hardware settings once set.

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
  - The text goes into `szOSDEx2` (32 KB, after the 256 KB object buffer) on shared memory v2.20+ (RTSS 7.3.5+), as RTSS's own Overlay Editor writes it, else `szOSDEx` (4 KB); both are cleared on every write (`text_field`). Text that doesn't fit loses whole lines at its end (`fit_lines`; `rtss_state.trimmed`), never half a tag.
  - **Which game:** the frame loop follows the app whose frametime ring moved in the last 2 s (RTSS's foreground app first) and keys its figures on (slot, process), so a new game in a reused slot starts over. `fps_at` takes FPS, frametime and the game RAM's process from that same app (else RTSS's foreground app, else the fastest), and skips any app whose last framerate period (`dwTime1`, `GetTickCount` ms) ended over 2.5 s ago (`STALE_MS`): RTSS keeps an app's last numbers after it stops drawing.
  - The preview's API and resolution are the game's own (`game_info_at`: the low word of the app entry's `dwFlags` at 264, `dwResolutionX/Y` at 9224 on v2.20+); "–" with no game. The 1 % / 0.1 % lows are the average of the slowest 1 % / 0.1 % of frames over `stats_secs` (checked against a 60 s count straight from RTSS's ring: 119 avg / 62 low both ways).
  - **The graph** (`osd::graph_object`) is RTSS's own frametime graph, MSI Afterburner's: an embedded object with `GRAPH_FLAG_FRAMETIME` and no data, which RTSS fills from every frame and redraws with each one. Deck writes it with the text and never feeds points (it used to: 12, then 60 points a second, choppy and then noisy). Style lines / filled (1) / bars (8). The top is `graph_max` ms, or on auto four average frames (`4000 / fps_avg` ms, 50 ms before there is an average; written with the text, so it follows the average once per update), × 1000: RTSS's frametimes are µs (the SDK sample's 0..50000). Never RTSS's `AUTOSCALE`: it followed the slowest frame on screen. Never `GRAPH_FLAG_BGND`: RTSS tints that box in the graph's colour.
  - **The graph's line** is a `Line` built in `osd::lines` like the others (label "FT", group fps), so align and columns apply: the frametime part (left out when the `frametime` item is on the overlay), then a `Part { graph: true }` that renders `<OBJ=00000000>` in the line's label colour, as wide as that line leaves of the widest other line (`osd_text_width`).
- Deck also publishes Afterburner's `MAHMSharedMemory`, so RTSS OverlayEditor's "MSI Afterburner" data source keeps working.
- The items and keys are set on the **Overlay** page (Monitor → Overlay). It has three parts. A strip with `osd_on`, RivaTuner's state, the show/hide key and the frame limiter (limit, on/off, key). The preview, pinned while the page scrolls, with the quick looks under it. Tabbed settings: Items · Layout · Look · Colours · Graph · Warnings · Behaviour. The open tab is kept per viewer in `localStorage` (`deck.ovTab`).
- Settings: `osd_on`, `rtss_autostart`, `rtss_tray_hidden`, `osd_hotkey`, `osd_reset_hotkey`, `limiter_hotkey`.
- **Framerate limiter:**
  - On/off is RTSSHooks' `LIMITER_DISABLED` flag. Deck sets it in both copies: the 64-bit RTSSHooks64.dll, which Deck loads itself, and the 32-bit one through `rtss-osd32.exe`. RivaTuner's window and 32-bit games use the 32-bit copy.
  - The state is read from the 64-bit hooks, then the helper, then what the key set last.
  - The limit is written to `Profiles\Global` (`[Framerate] Limit`), then set through the hooks' profile calls (`LoadProfile("")`, `SetProfileProperty("FramerateLimit")`, `SaveProfile`, `UpdateProfiles`), so a running RivaTuner uses it at once.
- **RivaTuner's tray icon** (`rtss_tray_hidden`): RivaTuner has no setting for it, so Deck removes the icon itself with `Shell_NotifyIcon(NIM_DELETE)`. RivaTuner's icon belongs to its window titled "RivaTunerStatisticsServer", with that window's handle as the icon id. The rtss loop repeats this every 5 s, because RivaTuner adds the icon back when it or Explorer restarts; the window is found by title with `EnumWindows` (it used to start `tasklist` each time). RTSS.exe is found through `HKLM\SOFTWARE\WOW6432Node\Unwinder\RTSS` `InstallPath`, else the default folder. Posting `TaskbarCreated` to that window brings the icon back with no restart. **Open RivaTuner** restarts RivaTuner. Its window is drawn properly only when RivaTuner opens it itself, on start or from its tray icon; shown from outside it stays black, and the tray icon can't be clicked from outside. With RivaTuner's own *Start minimized* on (`StartMinimized=1` under `[Settings]` in `Profiles\Config`) a fresh start stays in the tray, so Deck sets it to 0 for that start, waits until the main window is visible, and sets it back to 1. A running game's overlay blinks for a moment.

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
| `src-tauri/src/main.rs` | Tauri setup, `generate_handler!` registration, tray, show/hide, toast and zones windows, `icon://` protocol, command-line modes, and the ~40 commands still coupled to its statics/dialogs (window control, clipboard+paste-back, power/timer, workspaces, screen tools, file pickers, `share_to_phone`, `elevate_setup`). Thin JSON-only command adapters live in the `*cmd.rs` modules below. |
| `app.rs` | Shared `App` state, change events, toasts, starting all workers |
| `settings.rs` | The settings struct. Atomic save with a `.bak` of the previous version; a damaged `settings.json` is kept aside and recovered from the backup (or merged field-by-field) rather than reset. `DECK_PROFILE=test` switches to `%APPDATA%\Deck-test`. |
| `profiles/` | CPU (`cpu.rs`, `plans.rs`), GPU profile model, Afterburner import (`afterburner.rs`), hotkeys, shortcuts, icons |
| `gpu/` | `engine.rs` (apply, fan loop, drift and resume re-apply), `nvml.rs`, `nvapi.rs`, `curve.rs`, `guard.rs` (crash guard) |
| `hw/` | MoboCtl's engine: `sio`, `hp`, `pawn` (+ `mods/`), `sensors`, `sysmon`, `smbios`, `fx`, `hid`, `devices`, `lcd`, `loopback`, `screen`, `game`, `cpupower`, `powermode`, `rapl`, `memclean`, `cfg`, `kv` |
| `*cmd.rs` (command adapters) | Thin `#[tauri::command]` wrappers that only translate JSON and delegate to a service module, grouped by domain: `profcmd.rs`/`hwcmd.rs` (CPU/GPU/RTSS and Hardware pages), `audiocmd.rs`, `displaycmd.rs`, `netcmd.rs`, `diskcmd.rs` (Disk + Clean up), `syscmd.rs` (Processes/Startup/locks + This PC report), `boostcmd.rs`, `phonecmd.rs` (Phone, PC-to-PC, Transfer, Claude Sync). Each declares its own `type A`/`type R` aliases and is registered in `main.rs` as `module::command`. Commands that touch main's statics, tray or file dialogs stay in `main.rs`. |
| `pad/` | The controller overlay and the universal controller battery (from the standalone Controller Overlay, merged 2026-10-18; design in `docs/design/specs/2026-10-18-controller-overlay-design.md`). `battery/` asks five sources every 2 s while the tray icons or the overlay are on (`gamesir` vendor HID protocol, `sony` passive DS4/DualSense input reports, `wgi` Windows.Gaming.Input, `bt` the Bluetooth battery property, `xinput`), merges them per controller, smooths like a phone (`pad-battery.json`) and drops a controller after 2 silent polls. `overlay.rs` is the overlay thread (layered windows from `window.rs`, a high-resolution waitable-timer clock at 60/30/20 Hz, 4 Hz hidden with no controller, none while off), `art.rs` the SVG art rasterised by `resvg` (`raster.rs`), `input.rs` SDL3 / XInput / demo / Raw Input, `sdl.rs` the run-time SDL3 binding (the DLL is embedded gzipped and written next to deck.exe), `place.rs` monitors and anchors, `icon.rs` the battery tray icon, `cfg.rs` the `pad` settings. Tray icons (`pad-<hash>`) and the tray submenu live in `main.rs`; commands in `padcmd.rs`. |
| `deaths/` | The death counter (design: `docs/design/specs/2026-10-19-death-counter-design.md`). `book.rs` replays a game's events into fights and per-boss stats (attempts, wins, deaths before the first kill, time), `store.rs` the files, `ident.rs` names the game in front by its program alone, never a store's list or number (any fullscreen program or one the catalogue lists, keyed `exe:<stem>`; named from the exe's description, else its install folder, else the window title; emulators by window title), `catalog.rs` + `catalog.json` the games Deck has extras for, by exe name, kept by hand (extras only, never a whitelist), `pill.rs` the in-game counter's window (the controller overlay's layered window, fades a picture in), `look.rs` what it draws (skull / ring / huge number, the card after a death, labels, the share picture), `edge.rs` the red edge on a death, `stats.rs` tries, play sessions and records, `view.rs` the page's numbers and the CSV export, `cfg.rs` the `deaths` settings. One thread checks the game in front every second and draws the pill. Tray submenu "Death counter" in `main.rs`; commands in `deathscmd.rs`; +1 / undo hotkeys in `profiles/hotkeys.rs`; `combo.rs` the controller combo (the thread opens its own passive SDL reader only while the combo is on and a counted game is in front, and reads it 20 times a second). `save.rs` reads deaths from save files (Celeste's XML `TotalDeaths`, Dark Souls PtDE's BND4 slots; a rise of at most 20 on the same character counts, anything else is a new baseline). `screen/` watches the game in front when no save reader works: `capture.rs` (Windows Graphics Capture, from the capture spike, a ~1000 px frame read back from a GPU mip chain), `detect.rs` (the gate: OCR only when the middle has settled, at most 1/s, an unchanged picture every 3 s; death words matched per line with OCR slack; two reads in a row = a death, 8 s cooldown and the screen must clear first; 20 s of black = "blind"), `ocr.rs` (Windows.Media.Ocr, English first), `teach.rs` (taught death screens for screens without words: the last 20 s of frames, 2 a second, are kept in memory, a death counted by hand keeps them as a strip (shown in colour), the frame you pick becomes a fingerprint of the cells that differ from normal play, or, when the whole screen faded, of what stands out in a line within it (the words), matched on every frame by correlation; up to 3 per game, shareable as a code). Thumbnails are each cell's brightest colour, so a red banner stays bright, `mod.rs` (its own thread, 4 frames/s). `library.rs` is the shared death-screen library: every build embeds `data/death-screens/library.json` and, while the counter and `community_library` are on, looks for a newer copy of that file on GitHub at most once a day (one plain GET with ETag, 3 MB cap, the only background network request Deck makes; a copy is used only if newer and every print passes the same checks as one you taught); its prints are matched next to your own and never written to your taught list. People add to it with "Submit to the shared library" on the Now tab: the page copies the share code and opens the `death_screen.yml` issue form, `.github/workflows/death-screen-intake.yml` runs `tools/deaths-library/ingest.py` on the issue and opens a pull request for review (needs "Allow GitHub Actions to create pull requests" in the repo's Actions settings; untested until the first issue arrives). The catalogue's death words are added to the common ones, never replace them. Nothing seen is saved. |
| `rtss.rs` | RTSS overlay slot and MAHM shared memory; installs RTSS (winget); flips RTSS's own OSD and limiter switches (64-bit hooks in process, 32-bit through the helper); sets the framerate limit through RTSS's profile API; opens RTSS's window |
| `rtss32/` | The 32-bit helper (`rtss-osd32.exe`) that calls RTSSHooks' SetFlags; `build.rs` builds it for i686 into `target/rtss32` and embeds it, with the C runtime linked in (`+crt-static`, so it needs no Visual C++ runtime; test `the_32_bit_helper_needs_no_visual_cpp_runtime`). Needs `rustup target add i686-pc-windows-msvc`. Without that target `build.rs` only prints a warning, and the build ships without the helper; releases up to 2.5.4 did. Treat that warning as a release blocker. |
| `mft.rs`, `disk.rs` | NTFS MFT reader, folder tree and treemap layout for Disk space |
| `hw/smart.rs` | Drive health (NVMe log page 2, ATA SMART attributes and thresholds), history and alerts |
| `support.rs` | The support report (This PC) |
| `boost.rs` | Game boost (close apps, power plan, priorities, trim; everything restored afterwards) |
| `beam/` | Transfer (from AppBeam): `net.rs` (UDP beacon 47833 + encrypted TCP 47834, hello `DKT1`; offers, files, resume, projects, requests; frames over 256 MiB refused; names from the other PC cleaned by `clean_rel`/`clean_component`/`clean_name`), `device.rs` (each PC's Ed25519 key in `transfer\device.key`; the PC id is derived from it and every hello is signed, so trust stays with one PC), `config.rs` (settings, inbox, live state; `off` stops announcing, answering and accepting; settings v2 cleared trust once when ids became key-based), `apps.rs` (winget), `setup.rs` + `snapshot.rs` (installers, setup snapshots, Windhawk; a received installer always waits for a click here and runs as the user through `--run-installer`), `mod.rs` (Deck's glue: `set_on`, `state`, `act`, notifications with Accept/Decline). Deck-only: the standalone AppBeam (47831/47832) doesn't talk to it |
| `ccsync/` | Claude Sync (from ClaudeSync): `engine.rs` (sync cycles, merge, forks, deletions, locks; writes through `fsguard`), `guard.rs` (incoming changes to hooks, plugins, status line, helper commands, permissions or env wait for Apply / Keep this PC's), `lan.rs` (UDP 47835 + TCP 47836, hello `DKC1`), `hub.rs` (cloud folder `Deck-ClaudeSync-hub`), `canon.rs` + `scan.rs` (path tokens per project), `locks.rs` (the hooks: `%LOCALAPPDATA%\Deck\claude-sync-hook.exe --claude-sync-hook`, a copy of deck.exe so updates aren't blocked), `mod.rs` (glue; imports the standalone app's settings once; refuses to start while `ClaudeSync.exe` runs; `--uninstall-cleanup` from `hooks.nsh`) |
| `fsguard.rs` | The elevated process acting on user-writable paths: checks against the user's limited token, handle-relative opens/creates/renames/deletes, links out of the root refused. See `docs/trust-boundaries.md`. |
| `seal.rs` | Fingerprints of the hardware settings in `HKLM\SOFTWARE\Deck\Seals`; at an admin start a mismatch holds the hardware until the user applies it |
| `watchdog.rs` | The `--watchdog <pid>` process that releases the hardware if Deck is ended from outside, says how it ended (`Outcome`, `describe_exit`) and hands over to `restart.rs`; `module_at` names the DLL of a crash |
| `restart.rs` | After an unclean end: wait for a driver installer, start Deck again through `\Deck\Background`; 3 ends in 10 min = give up and write `safe.json` (next start holds the hardware like `HW_HELD`, toast `crash-loop`, 30 min). `crashes.json` keeps the history. Setting `restart_after_crash`. |
| `driver.rs` | Spots a graphics driver setup (by the folder/name it runs from), sets `PAUSED` (every NVML/NvAPI call then fails cleanly: `gpu::nvml::Gated`, `NvApi::func`) and restarts Deck once after it (`--start-after <pid>`) |
| `update.rs`, `updatecmd.rs` | Updates from GitHub releases: daily check (`api.github.com/.../releases/latest`, ETag), release must be a plain newer `X.Y.Z` with `*-setup.exe` + `.sha256` under this repo's release downloads; setup saved in `Program Files\Deck\updates` (admin) and run `/S /UPDATE`, waited for; without admin tasks it is checked in Temp and an elevated `deck.exe --install-update` (one UAC) copies it to `updates`, fetches the checksum again and runs it (never from a user-changeable folder); SHA-256 checked while held open; a 304 is "nothing new"; downloads end on a 60 s stall, not after a fixed time; a failed try waits 6 h (download) or a day (setup); `restart::mark_updating` keeps the update's close from counting as a crash; waits for no game in front; `update.json` keeps the state. Setting `auto_update`. |
| `kdf.rs` | Slow, memory-hard stretching (scrypt ROMix over BLAKE3, 32 MiB, about 0.25 s, cached per code) for the pairing code and the sync key |
| `phoneauth.rs` | Phone request signing (HMAC-SHA256, single-use counters), the Host check and the page's CSP |
| `phonetls.rs` | HTTPS on the phone port: the self-signed certificate and the listener that serves HTTP and HTTPS together |
| `phonemic.rs` | The phone as speaker (PC quiet) and mic (VB-CABLE: find, turn on, get; the player; the default-mic switch; crash restore) |
| `tweaks/` | Maintenance → Tweaks. `mod.rs`: the recipes (`src-tauri/tweaks/tweaks.json`, built by `tools/tweaks/build.py` from Win11Debloat and WinUtil at pinned commits plus Deck's own), state per tweak (the first option whose steps all match; Windows' default also when values are missing; otherwise "mixed" with the option most steps lean to), apply with the before-values kept once in the journal (`%APPDATA%\Deck\tweaks.json`, sealed in `HKLM\SOFTWARE\Deck\Seals`; one changed outside Deck is set aside as `tweaks.changed-outside.json`), exact undo / undo all (only of what the tweak's recipe touches, `before_allowed`; DirectX keys one by one, `Before::Dx`), "Windows changed this back" (6-hourly watch, one toast), after-effects (Explorer restart lets Winlogon start it as the user), restore point, the "Unpin everything from Start" action, `run()` with a time limit for every program. `steps.rs`: reg (values, whole keys exported/imported, bits in dwords and numeric strings), DirectX settings string, services (sc.exe), optional features (DISM, admin only), fixed commands (powercfg /h, reserved storage), DNS; tests map every path under a scratch HKCU key. `apps.rs`: built-in apps (Win11Debloat's list without Edge), remove for the user, reinstall from Microsoft's servers. `storedl.rs`: Store apps without the Store (FE3: catalog → cookie → SyncUpdates → GetExtendedUpdateInfo2), newest package per name for this architecture, dependencies the PC already has skipped, downloaded in 8 ranged parts at once into a held `fsguard::Dir` (`<name>.part` preallocated, `<name>.parts` the progress map, so Carry on resumes each part), SHA-256 checked (the CDN links are http), Add-AppxPackage; or links to FluxDM (`127.0.0.1:7878/api/add`) or to copy; winget for desktop Store apps. `tweakscmd.rs`: the commands. |
| `log.rs` | Deck's log in `%LOCALAPPDATA%\Deck\logs` (rotating); `tail` feeds the support report |
| `webview_guard.rs` | WebView2 browser flags that prevent blank windows (applied to every window builder) plus the page watchdog; scrubs `WEBVIEW2_*` when elevated; `--cdp-port` |
| Other modules | `sys`, `audio`, `media`, `apps`, `calc`, `files`, `clipboard`, `power`, `winmgr`, `disk`, `cleanup`, `startup`, `display`, `net`, `gameping`, `locksmith`, `games`, `picker`, `hotcorner`, `remote`, `screencast` (screen view and control for the phone), `rdesk` (RustDesk + Tailscale), `share` (share links, Funnel), `elevation` (tasks, Set up and its install-location check), `util` |
| `ui/` | `index.html`, `app.css`, `guard.js` (classic script), and `js/` as native ES modules: `main.js` (the entry: imports the rest, exposes test names on `window`, calls `boot()`), `core` (helpers, `S`, `Pages`), `app` (nav and boot), `home`, `layout`, `search`, `pages1-3`, `hw`, `curve`, `profiles`, `overlay`, `pcs`, `help` (the "?" texts), `boost`, `icons`. `toast.html` loads `js/toast.js` and `zones.html` loads `js/zones.js`, each as a module. |

---

## 6. Build, install, test

```powershell
cd src-tauri
cargo test                # 344 tests (+35 ignored live tests)
cargo clippy --all-targets -- -D warnings   # must be clean; CI enforces it
cargo build --release     # set CARGO_TARGET_DIR to build elsewhere
..\install.ps1 -NoBuild   # elevates itself, quits Deck, copies to Program Files, (re)creates the tasks
..\uninstall.ps1 [-Purge] # quits Deck (fans back to the BIOS), removes tasks, shortcuts, files and the PawnIO driver if Deck installed it
```

- **UI changes need a rebuild**, because the UI is embedded in the exe. `install.ps1` deletes Tauri's cached asset copies first, since a crashed build once left them broken.
- **Setup:** `cargo tauri build --bundles nsis` makes `Deck_<version>_x64-setup.exe`, a **per-machine** setup (`"installMode": "perMachine"` in `tauri.conf.json`; one UAC prompt; installs to Program Files; `hooks.nsh` removes an older per-user copy silently, keeping settings).
- **CI** (`.github/workflows/ci.yml`, every push to `main` and every PR, with `DECK_REQUIRE_RTSS32=1`): `node --check`, clippy `-D warnings`, `cargo test --locked`, the UI suites (`regress`, `a11y`, `toast-test`, `phone-shot`, `phone-sign`), `cargo audit`. Compiled dependencies are cached (also after a failed run) and a newer push cancels the older run; the NSIS setup is built only by `release.yml`.
- **Releases** (`.github/workflows/release.yml`): push a tag `v<version>`. It checks the tag matches `tauri.conf.json`, runs the tests, builds the setup, and publishes the release with the `## <version>` section of `CHANGELOG.md` as the notes and the SHA-256 beside the setup. Nothing is attached by hand.
- **`DECK_REQUIRE_RTSS32=1`** makes `build.rs` fail when the 32-bit RivaTuner helper didn't build (no i686 target), instead of warning.
- **Command-line modes:**
  - `--quit`: graceful exit.
  - `--release-fans`.
  - `--minimized`.
  - `--switch`: profile switching, used by the shortcuts.
  - `--setup-task [--start]`: creates the admin tasks (elevated; from Set up and `install.ps1`), refusing a user-changeable install.
  - `--watchdog <pid>`: the hardware watchdog (started by Deck itself); restarts Deck hidden after an unclean end.
  - `--start-after <pid>`: waits (up to 2 min) for that Deck to end, then starts Deck hidden through `\Deck\Background` unless one already runs (the restart after a driver install).
  - `--install-update <setup> <checksum url> <version>`: elevated, from the updater when Deck has no admin tasks: copies the checked setup into `updates`, checks it against the release's checksum again and runs it silently.
  - `--setup-task --from-setup`: from the installer; exits 2 without making tasks when the setup runs as another account than the one signed in (the setup then starts Deck as the user).
  - `--run-installer <job>`: runs a received installer as the signed-in user (started by Deck through Explorer).
  - `--cdp-port=<n>`: the WebView debug port for live testing (the only way to get one when elevated).
  - `--launch-test`: proves apps start as the normal user (used by `install.ps1`).
  - `--pc-list`, `--pc-send`, `--pc-get`: PC to PC from scripts; `--transfer-apply-windhawk` / `--transfer-undo-windhawk` (elevated helper).
  - `--claude-sync-hook` (the hook copy), `--uninstall-cleanup` (from the uninstaller).
  - `--hw-selftest`, `--cpu-probe`, `--gpu-probe [--write] --out f`, `--pad-probe [--out f]` (every controller battery source and 4 s of SDL input; read-only, no window).
  - `--dump-state`, `--dump-hw`, `--dump-icons`: read-only, used for UI previews (debug builds).
  - `--selftest` and `--selftest-remote`: these use the test profile and bind to 127.0.0.1 (debug builds).
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
| `deaths\<game>.jsonl`, `deaths\games.json`, `deaths\<game>.taught.json` | The death counter: one history per game (one event per line: fight start, death, undo, win, abandon, rename), each game's name and last use, and the death screens taught for a game (fingerprints of small grey thumbnails, no pictures) |
| `pad-battery.json` | The controller battery's lowest level since the last charge, per controller (phone-style smoothing) |
| `plan.json` | The "Shut down when…" plan, so it survives a Deck restart (it waits again from the start) |
| `tweaks.json` | Tweaks' undo journal: each tweak Deck changed, the option it set, the option it was at, and the exact before-values; the built-in apps Deck removed (for Reinstall). Sealed in `HKLM\SOFTWARE\Deck\Seals` (`tweaks-journal`); one changed outside Deck becomes `tweaks.changed-outside.json` and isn't used |
| `tweaks-start2.bin` | Your Start layout, kept by "Unpin everything from Start" for its undo |
| `phone-tls.json` | The phone remote's certificate (its names, the certificate, the key sealed with DPAPI) |
| `phone-quiet.json`, `phone-mic.json` | Only while a phone plays or talks: what Deck muted / which mics were the defaults, put back at the next start after a crash |
| `update.json` | The updater: last check, ETag, the release found, a failed version and when to try it again |
| `transfer\` | Transfer: `config.json` (pairing code, shared folders, per-PC policy), `device.key` (this PC's own key), `inbox.json`, `log.txt`, `outgoing\` (packed app lists and setups) |
| `claude-sync\` | Claude Sync: `config.json` (sync key, projects), `state.json` (merge bases), `hashcache.json`, `locks.json` + `identity.json` (read by the hooks), `trash\`, `conflicts\`, `claudesync.log` |
| `C:\Program Files\Deck\SDL3.dll` | Written by Deck from its embedded copy the first time the controller overlay runs (removed by the uninstallers) |
| `C:\ProgramData\Deck\pawnio\` | PawnIO driver files (the service is `DeckPawnIO`) |
| `%LOCALAPPDATA%\Deck\logs\` | Deck's log (`deck.log`, rotating) |
| `%LOCALAPPDATA%\Deck\store\<id>\` | Store app packages while they download and install (`<name>.part`, `<name>.parts`; removed after the install). **Download** saves to `Downloads\Store apps\<app>` instead |
| `HKLM\SOFTWARE\Deck\Seals` | Fingerprints of the hardware settings Deck last saved (`settings`, `hardware-settings`, `hardware-curves`); admin-only. The test profile uses `Deck-test`. Deleting the key only means the next admin start trusts the current files. |

---

## 8. Known issues

Hardware quirks of the maintainer's PC are in `PRIVATE_NOTES.md`.

**Deferred small items from code review** (2026-10-12 feature audit fixed the crash guard on quit, the MAHM double writer and the `rtss::stop` / CPU plan-switch races; see `docs/audits/2026-10-12-feature-audit.md`):
- The "Try it" preview is lost when you alt-tab away.
- The undervolt "risky" rule (a raised point at 850 mV or more) errs on the safe side; left as is.
- Quit measured at 2.1 s (Game boost off); `elev-install.ps1` now logs the quit time. The old "up to 20 s" note was not reproduced.
- The minimum-clock lock lands about 8% under target.
- `toggle_window` uses a special event.

**From the 2026-10-17 security remediation, verified only by unit and headless tests** (see `PROJECT_STATE.md`):
- running the per-machine setup over a per-user install;
- the watchdog releasing real fans after `taskkill /f`;
- phone uploads end to end with signed requests;
- the CI and release workflows on GitHub;
- the "changed outside Deck" hold on a real admin start;
- the ES-module UI in the real app (headless suites pass).
- Phone screen control reaches admin windows (S13, accepted; see `docs/trust-boundaries.md`).

**Ideas not built:**
- A built-in overlay that doesn't need RTSS: a transparent click-through window, with FPS read from Windows' frame-timing events. It would work in borderless and windowed games but not exclusive fullscreen.
- Letting a CPU profile switch the fans too.

**Turned down, don't re-propose unless asked:** NetLimiter-style bandwidth limits, and remote desktop from outside the home network.

**Backlog:**
- Save guardian, death counter and backlog picker as Deck pages.
- Tile reordering, a custom layout editor, text snippets.

---

## 9. Before every commit (mandatory)

**Do this every time before running `git commit`.** It is the rule at the top of §0, repeated here so it can't be missed.

1. `PROJECT_STATE.md`: add or extend today's dated entry: what changed, why, what was tested (and what wasn't).
2. `CHANGELOG.md`: add user-visible changes under the version being worked on (an `Unreleased` heading if no version is set). These become the release notes.
3. `DECK.md`: update any fact that changed (pages, settings, commands, test counts, file layout, gotchas).
4. `README.md`: update the feature list, screenshots or build steps if they changed.
5. Check none of them contains personal data (paths, hostnames, PC specs, games, IPs). Personal facts go in `PRIVATE_NOTES.md`.
6. Stage the docs in the **same** commit as the code.

The hook in `.githooks/pre-commit` (enabled with `git config core.hooksPath .githooks`) refuses a commit that changes code without staging `PROJECT_STATE.md` and `CHANGELOG.md`. If it blocks you, update the docs; never use `--no-verify`.
