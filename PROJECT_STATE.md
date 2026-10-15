# Deck — project state

Handoff notes so work can resume later. Last updated: 2026-10-15 (v2.5.3).

**Start with `DECK.md`** (full overview: features, hardware, files, build/test, status). This file is the dated history.

> **RULE: before every commit, update the docs.** Add today's dated entry here, add user-visible changes to `CHANGELOG.md`, update `DECK.md` / `README.md` when facts change, and stage them in the same commit. The pre-commit hook enforces it; never bypass it. No personal data in these files (that goes in the git-ignored `PRIVATE_NOTES.md`).

## 2026-10-15: v2.5.3 — the board lighting config is checked and re-sent

- Report (owner's PC, 2.5.2, support report attached): all lighting controls dead, any effect, on or off; the lights test didn't work either; the report said "Motherboard RGB connected". Restarting Deck did not help. RAM lighting "not found" in that report is irrelevant: this PC has no RGB RAM (the friend's has).
- Found by talking to the controller from a separate process (Python ctypes, `HidD_GetFeature`/`SetFeature` on `vid_1462&pid_7d43`): it answered instantly (not hung), but feature 0x52 held a foreign config (`01 ff 00 00 28 …`) instead of Deck's (`01 00 00 00 08 …`). Writing Deck's `config_packet(60, 60)` was accepted, read back identically in the first 60 bytes and then stayed for 36 s. While the board holds another config it ignores every 0x53 colour report, with no error, and Deck only wrote the config at connect/reconnect (start, wake, send failure), so it never noticed. What reset the config is not known (no other lighting program was running; the PC had been restarted at 21:44).
- Fix: `Board::config_intact()` reads 0x52 and compares `[..60]` with `config_packet` (bytes 62 and 82 are rewritten by the board, so a full compare never matches: checked on the real board). The board loop checks every 5 s and sets `reconnect` (which rewrites the config) when it differs. `config_packet()` split out of `send_config()`.
- Second fix: `hw::lights_test()` is the one place that runs the lights test (hold flag, `rgb_diag` on a thread with a 40 s limit, always releases the hold, reconnects). `hw_test_lights` and `support_report(lights)` use it. Before, a hung `HidD_GetFeature` or a panic left `rgb_hold` set: every lighting control frozen and "already running" for later tests. The board loop also drops a hold older than 60 s. (This was the first theory; restarting Deck didn't fix the report, so it was not the cause here, but it is a real weakness.)
- NOT tested: that the self-heal fires on a real config loss (the loss itself could not be reproduced); the build compiles and the packet compare was checked against the live board.

## 2026-10-15: v2.5.2 — the closing hook also works without the Quit task

- Report: the uninstaller still shows "Failed to kill Deck" after 2.5.1. Two reasons found by reading, not yet reproduced on a second PC: (1) `uninstall.exe` in the install folder is the one written by the installed version, so a Deck older than 2.5.1 still has the old one; (2) `\Deck\Quit` is only created by `--setup-task` or by an elevated Deck on startup, so a fresh 2.5.1 that never ran elevated has none, and the hook had nothing to start.
- Fix: `hooks.nsh` `DeckQuitIfRunning` now: runs the Quit task if Deck is running; if still running, `ExecShell "runas" deck.exe --quit` (one UAC prompt); waits up to 8 s after each step (`DeckRunning` / `DeckWait` macros). If both fail the template's own message appears as before.
- Tested on this PC (normal-user shell, Deck elevated from Program Files, NSIS copy per-user in %LOCALAPPDATA%\Deck): the setup (2.5.1 and 2.5.2) and the uninstaller both closed the elevated Deck and finished, with the Quit task; the uninstaller also finished with the Quit task deleted (the runas fallback; UAC is off here, so no prompt was seen). A freshly installed 2.5.2 Deck creates the Quit task itself within seconds of starting elevated.

## 2026-10-15: v2.5.1 — setup and uninstaller can close an elevated Deck

- Report (screenshot from a friend): uninstalling says "Deck is running! Click OK to kill it", then "Failed to kill Deck. Please close it first then try again". Cause: Deck runs elevated (its logon task); the NSIS setup/uninstaller runs as the normal user (`installMode: currentUser`), and Tauri's template kills with a user-level `taskkill`, which Windows refuses for an elevated process. `hooks.nsh` already had a PREUNINSTALL hook (Claude Code cleanup) but nothing closed Deck.
- Fix: `elevation::QUIT_TASK` (`\Deck\Quit`, no trigger, runs `deck.exe --quit` elevated). A normal user may start a task. `hooks.nsh` macro `DeckQuitIfRunning` (PREINSTALL and PREUNINSTALL, both run just before the template's own running-app check) starts the task and polls `tasklist` up to 8 s. `setup()` creates the task; `ensure_quit_task()` (thread at startup, admin only) adds it to installs that predate it and re-points it when Deck moved. A clean `--quit` also hands the fans back to the BIOS.
- Test: `quit_task_runs_quit_without_trigger`. Built with `cargo tauri build --bundles nsis` (2 jobs, idle priority, CS2 was running).

## 2026-10-15: v2.5.0 — PC to PC, Claude Code sync, share links, RivaTuner tray

- Released as **2.5.0** (2.0.1 was never released; its changes are part of 2.5.0).
- **RivaTuner tray icon:** the user didn't want it moved under the tray arrow (Windows' `NotifyIconSettings\IsPromoted`), but removed. RivaTuner has no setting for this; its strings only show `StartMinimized` and `SendToTrayOnClose`. Found by trying: the icon belongs to the window titled "RivaTunerStatisticsServer", and its id is that window's handle. Explorer accepts `NIM_DELETE` from another process. `rtss::set_tray_icon(false)` removes it and the rtss loop repeats this every 5 s while `rtss_tray_hidden` is on. `set_tray_icon(true)` posts `TaskbarCreated` to that window, and RivaTuner adds the icon back. Checked on the laptop: the icon is removed, RivaTuner keeps running, and Deck removes it again within 2 s after RivaTuner re-added it. `rtss_open` restarts RivaTuner (taskkill, then start). `ShowWindow` on either RivaTuner window from outside left it black, with most of the UI missing (user report). UI Automation finds the tray button but offers no Invoke on it. A fresh start opens the window properly (seen by the user). `start_rtss` now checks for RivaTuner's window, not its shared memory. Deck keeps the shared memory open after RivaTuner exits, so a RivaTuner that had closed was never started again; the first Open RivaTuner closed it and left it closed. Commands `rtss_tray` and `rtss_open`; ignored test `rtss_tray_icon`.
- Removed the phone's temporary debug logging (`vlog`, and the PC's `deck-phone.log` handler).

- **AppBeam and ClaudeSync are now part of Deck** (user's request: integrate them properly, Claude Sync off by default; chosen: native pages, Deck-only protocol). Engines copied into `src-tauri/src/beam/` and `src-tauri/src/ccsync/` with `crate::` paths rewritten; their egui UIs replaced by `ui/js/pcs.js` (sidebar: **PC to PC** above Phone; **Claude Code sync** under Desktop, hidden unless `claude_sync_nav` (Settings → Sidebar) or it is on). Deck-only: new ports (47833/47834, 47835/47836), hellos `DKT1`/`DKC1`, Claude Sync's key derivation string and hub folder renamed, so the standalone apps never mix with Deck's.
- Glue: `beam::set_on/state/act` and `ccsync::set_on/state/act` behind the commands `transfer_state`, `transfer_do`, `transfer_apps`, `transfer_setup_preview`, `claude_sync_state`, `claude_sync_do`, `pick_many`. Settings `transfer_on` (true) and `claude_sync_on` (false). Off = the `off` flag: no beacon, no answers, the accept loop drops connections; Claude Sync off also removes its hooks (`Engine::switch_off`). Offers and requests show as sticky toasts `xfer:<id>` with Accept/Decline (`App::toast_action`). `App::ui_changed` refreshes Deck's page without telling the phone. Firewall: `remote::allow_between_pcs` adds "Deck between your PCs" (program, any protocol, private/domain).
- Claude Code hooks: `deck.exe --claude-sync-hook` is handled first thing in `main()` (no Tauri); the hook runs a copy, `%LOCALAPPDATA%\Deck\claude-sync-hook.exe`, so a running hook never blocks an update. `is_ours` matches `--claude-sync-hook` only, so the standalone ClaudeSync's hooks are left alone (checked). NSIS `hooks.nsh` runs `--uninstall-cleanup` (not on updates) so Claude Code isn't left calling a missing program; `uninstall.ps1` does too and removes both firewall rules.
- `serde_json` now has `preserve_order` (Claude Code's settings.json keeps its key order when the hooks are edited); every test still passes.
- Tests: 118 pass (+27 ignored). New ignored loopback tests (`cargo test --release -- --ignored two_engines`): Transfer (5 MB file byte for byte, project folder without build output, request from a shared folder, an engine switched off isn't found) and Claude Sync (a session made on A arrives on B with A's home path rewritten to B's; no hooks while their settings are off; off gives phase "Off"). The hook checked with the real exe: blocked session exit 2 with Deck's message, others 0, no Deck started. UI checked with the preview harness (all tabs; `shot.mjs` now falls back to Chrome when Edge isn't installed). NOT tested yet: two real PCs (the laptop needs this Deck), the cloud folder, a real winget install, applying a setup, the NSIS uninstall hook (needs a full `cargo tauri build`).
- Later the same day: PC to PC trust (`Config.trusted`, `trusts()`, settings version `v` 1 turns `ask_before_receive` on; deletions always asked; toast "Trust this PC…" opens the page, whose select answers `trust_7|30|182|always`); "nobody answered in time" instead of "declined"; Tailscale peers tried by both engines (`rdesk::tailscale_pcs`, home address kept when also found by beacon, quiet errors); wrong-code log lines once per 10 min per address; the sync event gets a DACL (Everyone, medium label) so the hook can wake an elevated Deck (not tested at runtime); command line `beam/cli.rs` (`quiet` engine: no beacon, no listener; inbox.json merged, Deck reloads it on change) tested end to end against this PC's Deck (prompt, accept, file arrived); Claude Code skill `beam/skill.md` installed by a switch. Framerate limit key: `rtss-osd32 on|off|status <dir> 4` flips RTSSHOOKSFLAG_LIMITER_DISABLED (checked on the real RivaTuner: off→on→off), `limiter_hotkey`, `rtss_limiter` command, limit read/written in `Profiles\Global [Framerate] Limit`.
- Overlay preview colours (user report: some colours look right in game but not in the preview): the preview now follows `osd.rs render()` exactly: prefix and unit always C0, labels C0 or the group colour, graph C0 or C7, small units 62 % and raised, invalid colours fall back like `hex()`. The frame colour bar's preview was left as is (RTSS's own bar not checked).
- Phone remote: skip buttons seek 10 s when the player has no previous/next (browsers report `can_next:false`), taps add up; the home page shows CPU/GPU watts (`cpu_w`, `gpu_w` from the hardware engine's table, NVIDIA's reading as fallback) and the CPU package temperature (`cpu_t`).

## 2026-10-14: v2.0.1 — phone fixes, remote desktop, Tailscale

**H.264 for the phone Screen view (added last, uncommitted):** `src-tauri/src/h264.rs` (Media Foundation encoder, BGRA→NV12, Annex-B + fragmented-MP4 writer, `plan()` for width/fps per bitrate) and `screencast.rs` (`Video` enum, `spawn_h264` thread, `spawn_jpeg`, plan hysteresis: narrower after 1 s (3 s for MSE), wider after 10 s (20 s for MSE), sparse-input bitrate scaling because NVENC spends bits per nominal frame, fallback chain encoder → next encoder → JPEG). The phone asks with `/screen?v=h264w|h264m|jpeg` (WebCodecs when `VideoDecoder` exists, MSE otherwise; plain http has no WebCodecs). Message layout: 8-byte capture time, kind byte (0/1 Annex-B delta/key, 2 MP4 header, 3/4 fragment), data; `{"t":"video"}` text announces codec string/size; phone sends `{"t":"idr"}` on decode errors and `{"t":"codec"}` (saved as `remote_screen_codec`: auto/jpeg). Measured here: 1 Mb/s → 987 kb/s at 30 fps/640 px; 15 Mb/s 60 fps → 48 fps/12.8 Mb/s (the screen's own update rate); both decoders verified in headless Chrome (MSE without flags, WebCodecs with `--unsafely-treat-insecure-origin-as-secure`). Without WebCodecs the phone now decodes in the page: `phone/h264dec.js` (served as `/h264dec-2.js`, rename on change: cached a year) is tinyh264 0.0.7 (Apache-2.0, `phone/h264dec.LICENSE.txt`; npm package `es/TinyH264.js` + `es/TinyH264Decoder.js` with their `export default` lines removed) plus Deck's worker messages at the end; `/screen?v=h264s` → `Video::Sw`: Constrained Baseline (`Encoder::open_as(.., true)`, the decoder reads nothing else), Annex-B like WebCodecs, 8 in flight, answered once decoded; WebGL YUV→RGB (BT.709 limited), JS fallback; the phone asks `{"t":"vmax"}` for narrower pictures when decoding takes >70% of the time or 4 pictures wait for it (twice in a row, checked every 3 s). The phone server turns Nagle off (`tap_io` + `set_nodelay`). Checked: `baseline_stream_dump` (ignored test) + a Node run of the worker on both encoders' streams: 90/90 pictures, 45-50 dB PSNR against the source. MSE stays only for browsers without WebAssembly; on Firefox Android it stalled and replayed old pictures even after these fixes (its decoder drains when the buffer runs out, then decodes again from the last key picture). MSE pitfalls found on Firefox Android: `SourceBuffer.remove` also removes everything after the range up to the next key picture, so the phone trims only up to a key picture it recorded (and asks for one after 10 s without); seeking replays from the last key picture, so a phone that fell behind asks for a key picture and seeks onto it, otherwise playbackRate keeps a 0.1-0.3 s cushion; fragment durations are the real capture gaps (a 16.7 ms minimum made media time outrun real time and forced seeks; the last picture before a still screen gets 60 ms); a new `{"t":"video"}` with the player running appends the new init segment to the same SourceBuffer. Both Media Foundation encoders ignored `AVEncMPVGOPSize` set after the output type and made a key picture every 120 pictures; it's now set before and after (test `keys_when_bitrate_changes`). `--selftest-remote` takes `DECK_SELFTEST_SECS`. Sound: WASAPI loopback sends nothing in silence, which the phone counted as dropouts, so the PC now fills the first 1.5 s of silence. Live tests: `cargo test --release -- --ignored encoders_live keys_when_bitrate_changes baseline_stream_dump backwards_time_and_repeats sparse_input_rate real_screen_rate`.

- **Phone QR timed out:** no firewall rule for deck.exe, and an elevated Deck started by the logon task never gets
  Windows' "Allow access" prompt. `remote::allow_through_firewall` (when admin) adds "Deck phone remote" via netsh
  (this exe, TCP, private + domain), replacing it when the exe path changed.
- **File pickers behind Deck:** `main::file_dialog` sets Deck's window as the dialog owner.
- **Hot corner during screenshots:** `hotcorner::snipping_in_front` (ScreenClippingHost, SnippingTool, ShareX, Lightshot,
  Greenshot, Flameshot, Snagit, PicPick, Screenpresso) blocks the trigger; a mouse press in the corner sets `fired`
  so resting there after letting go doesn't open Deck.
- **Remote desktop (`rdesk.rs`):** RustDesk installed from its newest GitHub MSI (SHA-256 from the API `digest` +
  Authenticode check), service on/off, permanent password, `direct-server` (port 21118), `hide-tray` + Tailscale
  `--unattended` + their startup entries off. Admin work goes through `admin_ps` (base64 script; one UAC prompt when
  Deck isn't admin). Commands `rdesk_status`, `rdesk_do`. Status is cached for Tailscale (20 s).
- **Screen on your phone (`screencast.rs`):** `/screen?k=` WebSocket (separate from `/ws`). A capture thread per viewer:
  DXGI duplication of the chosen monitor on the adapter that drives it (matched by HMONITOR; laptops with two GPUs),
  GPU mip halving + `shrink` to the preset width, GDI (HALFTONE StretchBlt) for the first picture, HDR desktops and
  failures; GDI pictures are hashed so an unchanged screen sends nothing. JPEG via `image` (feature `jpeg`).
  Bitrate (`Rate`): `remote_screen_kbps` 0 = Auto, else a limit; `remote_screen_priority` smooth/sharp. JPEG quality
  follows each picture's share of the budget (log-sized steps), the width jumps to the one whose pictures fit when quality
  bottoms out or a picture is 3x too big, and back up to what the budget carries (`width_for`, a bytes-per-pixel model).
  Smooth: wanted fps, q down to 4, down to 160 px (1 Mb/s 30 fps ≈ 320 px). Sharp: plans `sharp_fps` (kbps/1500, 2-10),
  q ≥ 60, native size first, not below 480 px; q ≥ 80 encodes 4:4:4. Presets: "sharp" = Full (≤ 2560 px, default),
  "balanced" = Medium 1280, "fast" = Small 960. fps 15/24/30/60. Phone sound: one continuous stream (JS linear
  resampler carrying state, sample-exact scheduling, ±0.5 % speed for clock drift, cushion 40-300 ms)., a byte bucket (0.25 s) keeps the average under the limit, and budget left unused raises quality.
  Auto (`Rate::tune`, every 0.5 s): `Feedback` times each picture until the phone's ack; delay > 1.8x the connection's
  own + 80 ms (or a picture stuck > 1 s) cuts to 0.75x / 0.9x of what got through; delay near the baseline raises 15-30 %.
  Measured: 2 Mb/s smooth 25 fps at 640 px, sharp 12 fps; 1 Mb/s 12 fps at 480 px (JPEG's floor); Auto on the LAN reaches
  35 Mb/s in ~10 s. Pictures and sound packets start with their capture time (f64 ms on QPC: DXGI `LastPresentTime`,
  WASAPI's QPC position); the phone delays pictures to when their sound plays (`SA.off`) while Control is off.
  Sound packets: format byte (0 PCM, 1 IMA ADPCM), channels, rate, ADPCM states; `?q=best|good|low` on `/audio`.
  Pipeline: capture thread → two `screencast-jpeg` threads (alternate pictures) → `screencast-send` puts them back in order;
  the frame gap counts from the start of a grab (counting from its end capped 60 fps at ~45). Bytes are reserved in the
  bucket at grab time. `{"t":"diag"}` every 2 s: grabs / new / waits (credit, encoder, gap, bucket); the phone shows
  the screen's own rate from it. Smooth quality floor by width (25 at ≥854 px, 12 at ≥480, 4 below); 4:4:4 only for Sharp.
  Flow control: 4 frames in flight, one credit back per phone "ack". Presets sharp 1920/q85, balanced 1280/q75, fast
  960/q60; `remote_screen_fps` 30/60 sets the gap. JPEG by `jpeg-encoder` (AVX2, BGRA in): 3.7 ms at 1280 px (the
  `image` crate took 12.4 ms). Frames wait in `AcquireNextFrame(15 ms)`; only GDI pictures are hashed. Cursor sent as
  normalized `cur` messages. Sound: `/audio` WebSocket, `start_sound` = WASAPI loopback of the default output (follows
  device changes), 16-bit stereo 10 ms packets; the phone plays them with Web Audio and an adaptive cushion
  (30 ms up to 200 ms after dropouts, back down after 15 s smooth). Phone: Control off on every open (`SC.ctlOn`),
  full screen (`.scr.imm`), canvas drawing via `createImageBitmap`, tries the PC's LAN address (`st.lan`) before the
  Tailscale one, viewport `maximum-scale=1, interactive-widget=overlays-content` (Chrome zoomed into the hidden
  typing box and kept the zoom after rotating). Input (`input()`): SendInput absolute (VIRTUALDESK), buttons, wheel, unicode text, keys
  with mods; only in `control` mode, and not while `boost::game_in_front` (game folder, or fullscreen + GPU ≥ 60 %). Settings `remote_screen` (off/view/control, default off), `remote_screen_quality`.
  Measured on the dev PC: ~10 ms per 1280 px frame (capture + shrink + JPEG), ~85-95 KB; the frame rate follows how
  often the screen changes (Discord + a 30 fps stream: ~25-36 fps).
- **Tailscale:** `remote::info` adds `away` / `away_qr` (Deck's page on the Tailscale IP), `away_peers`,
  `away_account` and `away_get_qr` → `/tailscale`, a page served by Deck with store links plus the newest universal APK
  from pkgs.tailscale.com (the Play Store hides Tailscale in some countries). Tailscale's network is Private, so
  Deck's firewall rule covers it.

## 2026-10-14: v2.0.0 — the big update

Everything since 1.1.2, built and tested on the maintainer's PC between releases (fast build + exe swap each round).

- **Look:** 8 themes (`THEMES` in `pages3.js`, colours in `app.css` under `[data-theme]`), accent, opacity per pin state
  (`--deck-opacity`), opening animations (`:root[data-anim]` sets `--a-from/--a-to/--a-t/--a-ease`), low-spec mode
  (`html.low-spec`, tick every 3 s in `app.js`). First-start picker `themeWelcome` ("Make Deck yours", flag `setup_picked`;
  older flags `theme_picked` / `look_picked` stay in the struct so old settings files load).
- **Pin:** `set_pinned(on, rect)` floats the quick panel as its own always-on-top window (`PIN_RECT`); "shown" carries
  `float`; `body.pin-float`. In app mode: always on top. The click handler used `e.currentTarget` after an await (null).
- **Home grid:** free 12-column grid with 10 px rows (`pgEngine` / `pgResolve` / `pgSpot` in `layout.js`), push-down
  collision, snap guide, fold-away parts (`pgFit` / `pgFold`, `data-drop` marks). `pgKeep` carries place, size and fold
  across live redraws (stopped flicker). Defaults: `HOME_DEFAULT`. Stale `get_state` replies overwrote in-flight writes
  (resize "needed twice"): `localWrites` + `withLocalWrites` in `core.js`. `mouseHeld` covers long drags (2 min).
- **PC tile:** `PC_CARDS` in `home.js` (cpu, ram, gpu, vram, net), rows aligned with CSS subgrid; customize dialog
  `pcCustomize` (`pc_cards`, `pc_drives`). Backend: `live.cpu_now` (clock, package power, temps from the hw engine),
  NVML graphics clock and `vram_hist` in `sys.rs`.
- **Network rates:** sysinfo's adapter list counted the Npcap filter interface as well as the real adapter (double).
  `util::real_adapter` (hardware, not a filter, up, Ethernet/Wi-Fi/WWAN) + `util::NetCounter` (per-LUID deltas, so
  adapters add, a new one starts from its next reading, a removed one stops). Used by `sys.rs`, `rtss.rs`, `sysmon.rs`.
- **RAM RGB after a cold boot:** Deck starts ~17 s after boot, before the Fury controllers answer; the scan found
  nothing and never ran again (only wake rescanned). Now `FuryRam::scan_missing` retries (5 s doubling to 60 s, 10 min
  after start or wake) while fewer sticks than `fury_modules()` were found, without touching found sticks (a full
  rescan reset them: visible flashing).
- **Drives:** `hw/smart.rs` (NVMe log page 2, ATA 0xD0/0xD1/0xDA), history in `drive-health.json`, alerts gated by
  `drive_alerts` (default off), "past issue" notice once. Page in `hw.js` (`drivesCard`), support report includes it.
- **Disk space:** `mft.rs` reads the NTFS MFT directly (FindFirstFileEx fallback), squarified treemap, `disk_treemap`,
  `disk_types`, `disk_path`; UI in `pages2.js`.
- **Overlay:** own page; `rtss_install` (winget); the overlay key also flips RTSS's OSD switch via the embedded 32-bit
  helper `rtss32/` (RTSSHooks SetFlags + `WM_APP+100`). `build.rs` builds it into `target/rtss32` (short path: LNK1104
  in OUT_DIR). CI adds the i686 target.
- **Game boost:** trim only when memory ≥ 85 % (`memory_short`), auto-end after 3 h without a game (`NO_GAME_LIMIT`),
  GPU-busy foreground counts as a game.
- **Sidebar:** drag to reorder (`nav_order`, `navDnd`), Power & time last, Reset in Settings, all groups folded by
  default, width 180.
- **GPU wake on open (investigated, not fixable):** opening any window makes the NVIDIA driver jump P8 → P0 (~90 W for
  2–3 s); the Start menu does the same. NVML reads don't wake it, and `--disable-gpu` didn't help (DWM still composes).
  Kept: no repaint nudge on show unless the page reports no frame (`nudge_if_blank`).
- Tests: `cargo test --release` 95 pass (+15 ignored live). Live: `net_matches_windows`. UI checked with preview
  screenshots (the harness hard-codes Edge's path; Chrome works). Not tested: a cold boot with the RAM retry, laptops,
  AMD/Intel GPU cards in the PC tile.

## 2026-10-14: v1.1.2 — sidebar regrouped

- `NAV` in `ui/js/app.js`: Everyday / Power / Hardware / Screen / PC became Hardware (controls only) / Monitor
  (Sensors, Processes, Network, This PC) / Desktop (Windows, Display, Clipboard, Sound) / Power & time (all three
  Power subpages together) / Maintenance (Disk space, Clean up, Startup apps, Tools). `navShown` still hides hardware
  pages per PC, so Monitor just loses Sensors / This PC where there is no hardware. `nav_folded` holds old group
  names; they are ignored (folded groups reopen once).
- Removed the orphaned `files/index` route (nothing linked to it; `indexView` stays, used by Settings).
- Tests: `regress.mjs` 37 pass (run with Chrome; the harness hard-codes Edge's path). Installed and checked by the user.

## 2026-10-13: v1.1.1 — lighting survives sleep

- Bug: after sleep the Kingston Fury sticks fell back to their onboard effect. `FuryRam` keeps `per_led`/`sent`
  caches, so it never re-sent the direct-mode handshake (`set_direct`) and static colours weren't re-sent at all.
  Only `gpu/` listened for resume.
- Fix: `gpu::WAKES` (AtomicU64) counts `PBT_APMRESUMEAUTOMATIC`; `watch_resume` is now `Once`-guarded so several threads
  can call it. The board RGB thread sets `reconnect` and the RAM thread rebuilds + rescans `FuryRam` 4 s after a wake.
  Hardware-agnostic: on machines without these devices the rescan finds nothing, same as at startup.
- Tested: real S3 sleep/wake, RAM lights came back; no errors or crashes in the event logs. Not tested: hibernate, Modern Standby laptops.

## 2026-10-13: v1.1.0 — performance pass, units, Comet colours

- **Idle cost measured** (10 s samples, Deck hidden in the tray): ~19 % of one core, most of it the hidden WebView
  (renderer + GPU process ~7 % each). Cause: `ui/guard.js` ran a `requestAnimationFrame` loop forever, and the
  WebView2 flags (`--disable-renderer-backgrounding` etc., kept: they fix blank windows) stop Chromium from pausing it,
  so a hidden window ticked at the monitor's refresh rate. `guard.js` now requests one frame per 4 s heartbeat.
  The same guard is shared with the maintainer's other Tauri apps and was fixed there too.
- **Screen / Game lighting** (`hw/screen.rs`): DXGI desktop duplication with a GPU mip chain, reading back ~60x34 px.
  Measured 0.34 ms per sample vs 23 ms for the GDI `StretchBlt` (the GDI cost is a fixed DWM readback, not the size).
  No new frame = the last colours, free. GDI is the fallback (access lost, non-BGRA desktop such as HDR); retries
  duplication after 1-3 s. The session is dropped while no screen mode is on. Live check: `cargo test --release dxgi_live -- --ignored --nocapture`.
- Looked at and left alone: hot-corner 30 ms cursor poll, cooler-display loop (sends only on change), game audio meter
  (one peak read per poll), RTSS overlay (1 s). Board RGB at 30 Hz / RAM RGB over SMBus every 50-120 ms are the cost
  of animated effects; the user chose to keep them.
- **Units** setting: `unit_clock` (MHz/GHz), `unit_net` (Mb/s / MB/s), `unit_temp` (C/F) in `Settings`. JS helpers
  `fmtClock`, `fmtTemp`, `tempNum`, `fmtRate`, `mbpsParts` in `core.js`; the sensor table converts rows in
  `srUnits` (network rows only for transfer rates; tones still judged in °C). The overlay gets them through
  `Settings::osd_render_style()` (OsdStyle fields `ghz`, `fahrenheit`, `net_bytes`, not saved). The phone gets
  `sys.units`. Inputs (offsets, curves, limits) deliberately stay in MHz / °C.
- **Comet colours** (`L_CometColors`: My colour / Cycle / Rainbow) in `fx.rs`; the colour picker hides when the comet cycles.
- Sidebar badge (active CPU/GPU profile) now truncates with "…" instead of squeezing the page name.
- Tests: 86 pass (+3: overlay units, Comet colours, screen column reduce), 10 ignored live tests.

## 2026-10-11: hardware control (MoboCtl merged, Afterburner replaced)

Spec `docs/superpowers/specs/2026-10-11-hardware-control-design.md`, plan + ledger with every ruling in
`docs/superpowers/plans/` (`progress.md`).
- **Runs elevated** through scheduled tasks `\Deck\Deck` (logon, `--minimized`) and `\Deck\Open` (Start menu via `run.vbs`);
  installed to `C:\Program Files\Deck` by `install.ps1`; apps it opens still start as the normal user (`util::launch_as_user`).
- **Hardware group**: CPU, GPU, Fans, Lighting, Cooler display, Sensors, This PC (pages hide when the PC has no such hardware).
  Engine = MoboCtl's, in `src/hw/` (settings `%APPDATA%\Deck\hardware\settings.ini` + `curves.ini`, MoboCtl keys).
- **CPU profiles** = Windows power plans + extras (RAPL limits, min-clock lock, turbo OC, Game Mode off while capped);
  plan watcher (2 s), in-game clock check. `--cpu-probe` lists plans.
- **GPU profiles** = Afterburner slots: power %, temp limit, core/mem offset or VF curve (NvAPI VF table deltas), voltage %, fan
  auto/fixed/curve. Afterburner import (`profiles/afterburner.rs`, fixture = a real Afterburner cfg). Crash guard `%APPDATA%\Deck\gpu-applied.json`.
  Stock VF curve cached in `gpu-stock-curve.json`. `--gpu-probe [--write] --out f`.
- **Switching**: profile cards, tray submenus, RegisterHotKey keys, desktop/Start shortcuts (`\Deck\Profile <kind>-<id>` tasks),
  phone chips, Game boost. 25 icons in `src-tauri/icons/profiles` (generator `tools/icons/make.py`).
- **RTSS kept**: Deck writes its OSD slot + publishes `MAHMSharedMemory` (`rtss.rs`); items/key on the Sensors page.
- Tests: `cargo test --bin deck` (39). UI previews: `--dump-state` + `--dump-hw` (read-only) → mock `__TAURI__` → headless Edge.
- Debug: `--release-fans` (board + GPU fan + Game Mode back), `--quit` (graceful, used by `uninstall.ps1`), `--hw-selftest`.


## What it is

One tray app that combines a Raycast-style launcher, a live dashboard and the best
PowerToys / small-utility tools. Opened by a **hot corner** (mouse in a screen corner),
the tray icon, the Start menu or the phone. **No keyboard shortcuts** (user preference).

Built from research of 2026 "best Windows utilities" lists (PowerToys' 30 tools,
Raycast, EarTrumpet, WizTree, Ditto, LocalSend, Twinkle Tray, Stretchly, Auto Dark
Mode, LockHunter, BCUninstaller...). What made it in:

| Area | Features | Inspired by |
|---|---|---|
| Search box | Start-menu + Store apps (with icons), Deck actions, calculator, unit & currency conversion, number bases, games, files (own index), clipboard, web; commands `timer 10m`, `sleep in 30m`, `kill discord`, URLs | Raycast, PowerToys Run/Command Palette, Flow Launcher |
| Home tiles | Clock + calendar, now playing (SMTC, artwork), PC vitals (CPU/RAM/GPU via NVML, drives, sparklines), sound (outputs + per-app), pinned & frequent apps, quick actions, clipboard, power & timers, MicDrop, quick note, network | Dashboards |
| Clipboard | History of text/images/files, pins, search, click = paste into previous app, "paste as" (plain, case, trim, one line, clean link, pretty JSON); skips password-manager content | Ditto, Advanced Paste |
| Sound | Default output switch (IPolicyConfig), master + per-app volume with meters, all media sessions | EarTrumpet |
| Windows | Open windows list: pin on top, see-through, snap to zone, center, close; FancyZones-style drag snapping (tap right mouse button while dragging, or always); 6 layouts; workspaces (save window positions + apps to close + audio output + keep awake) | FancyZones, Always On Top, Workspaces |
| Files & disk | Drive/folder scan → treemap + list + biggest files, recycle; cleanup of temp, shader caches, crash dumps, browser/Discord/Spotify/Steam caches, thumbnails, old installers (to Recycle Bin), dev caches, Recycle Bin; file-search index folders | WizTree/TreeSize, PC Manager, CCleaner |
| Power | Keep awake (on / for N min / while app runs / until downloads finish), "do X when Y" (shutdown/sleep/restart/lock/screen off/notify when time, app closes, downloads finish, CPU idle, network quiet, media stops, away), 60 s cancel warning; timers, stopwatch, break reminders (never over fullscreen) | Awake, Shutdown timers, Stretchly |
| System | Processes grouped by app (end, not-responding flag), startup apps on/off (StartupApproved, same as Task Manager), network (IPs, ping, Cloudflare speed test), displays (refresh rate, DDC/CI brightness, HDR), dark/light theme + schedule (times or sunrise/sunset) | Task Manager, Twinkle Tray, Auto Dark Mode |
| Tools | Colour picker (loupe), copy text from screen (Windows OCR), pixel ruler, colour history, "what's locking this file" (Restart Manager), notes, shortcuts to Windows panels | Color Picker, Text Extractor, Screen Ruler, File Locksmith |
| Phone | LAN web app (installable PWA): media, volume, outputs, power, plans, timers, PC vitals, send text/links/files both ways, get PC clipboard | LocalSend, remotes |

## Preferences Deck follows

Key points:
- **No keyboard shortcuts** as the way in. Hot corner default top-left, 250 ms, off over fullscreen games.
- Deck now runs **elevated** (hardware control); the user accepted that. Profile hotkeys are the one keyboard exception they asked for.
- Picky about looks: check UI changes with offscreen renders before shipping.
- Must be safe with kernel anti-cheat (Vanguard, EAC, BattlEye). Deck uses **no input hooks**:
  hot corner polls the cursor, drag-snapping uses WinEvent hooks + GetAsyncKeyState.

## Current status

- Installed at `%LOCALAPPDATA%\Programs\Deck\Deck.exe` + Start Menu shortcut (`install.ps1`).
- **Not launched yet** by me (user may be gaming). First launch registers autostart (`--minimized`).
- Settings: `%APPDATA%\Deck\settings.json`; clipboard history in `%APPDATA%\Deck\clipboard\`.
- Phone port 47830 (MicDrop uses 47823). Files from phone → `Downloads\From phone`.

Verified: unit tests (calc, fuzzy search), read-only `--selftest` against the real PC
(audio, media, apps, games, windows, monitors, startup, ping, recycle bin), phone server
16/16 checks (`--selftest-remote` + node client), offscreen renders of every page and the phone page.

Not verified yet (needs the real GUI): hot corner feel, toast window placement, zones overlay
while dragging, colour picker / OCR overlay, paste-back into previous app, workspaces restore,
refresh-rate/HDR switching, firewall prompt for the phone.

## Code layout

| Path | What |
|---|---|
| `src-tauri/src/main.rs` | Tauri setup, ~80 commands, tray, show/hide (dim overlay or window), toast + zones windows, icon:// protocol, debug modes |
| `src-tauri/src/app.rs` | `App`: shared state, change events, toasts + toast actions, frecency |
| `settings.rs` | Settings struct (+ layouts, workspaces, notes), atomic save, `DECK_PROFILE` |
| `sys.rs` | sysinfo + NVML vitals, process groups, kill |
| `audio.rs` | outputs, IPolicyConfig default output, master/per-app volume, friendly exe names |
| `media.rs` | SMTC sessions, artwork, controls |
| `apps.rs` | AppsFolder enumeration, icon extraction (STA thread) → PNG |
| `calc.rs` | expression parser, units, currencies (open.er-api.com, cached), bases |
| `files.rs` | file-name index + fuzzy `score()` |
| `clipboard.rs` | message-only window listener, history, set text/image/files, `send_paste` |
| `power.rs` | keep awake thread, plans, timers, stopwatch, breaks |
| `winmgr.rs` | window list/actions, zones drag (WinEvent hook), workspaces |
| `disk.rs` / `cleanup.rs` | jwalk scan tree, recycle; cleanup targets |
| `startup.rs`, `display.rs`, `net.rs`, `locksmith.rs`, `games.rs` | as named (speed test can be cancelled mid-run) |
| `gameping.rs` | game server ping. Valve games: ICMP to every relay of each SDR city, best relay's median. Fortnite / Rocket League / Dead by Daylight: UDP echo to Amazon GameLift ping beacons `gamelift-ping.<region>.api.aws:7770`, grouped into each game's regions. TCP connect is useless here (router/ISP answers port 443 in ~5 ms). Riot etc. not listed (not measurable) |
| `picker.rs` | native fullscreen overlay: colour / OCR (Windows.Media.Ocr) / measure |
| `hotcorner.rs` | cursor polling + `fullscreen_in_front()` |
| `micdrop.rs` | WebSocket client to MicDrop's phone remote (only sends on clicks) |
| `remote.rs` + `phone/index.html` | phone server (axum) and page |
| `ui/` | `index.html`, `app.css`, `js/{icons,core,home,search,pages1-3,app}.js`, `toast.html`, `zones.html` |

## Build / test

```powershell
cd src-tauri; cargo test; cargo build --release
..\install.ps1          # refuses while Deck is running
```
Debug-only flags (`target\debug\deck.exe`), all use profile `%APPDATA%\Deck-test`
and change nothing: `--selftest`, `--dump-state`, `--dump-icons`, `--selftest-remote` (port 47839, localhost only).
UI previews: `--dump-state` + `--dump-icons` → mock `__TAURI__` → headless Edge (script lived in the
session scratchpad; recreate from this description). Never capture the real screen.

## Safety rules

1. Don't launch the GUI or change audio/display/power without asking; the user may be in a match.
2. Before tests check `Get-Process Deck`; test modes use the test profile and bind 127.0.0.1.
3. Deck never changes the default **recording** device (MicDrop owns it).

## Changes 2026-09-27 (second session)

- Speed test: Cancel button (restores the previous result; upload body aborts mid-request).
- System → Network → Game servers: only installed competitive games (CS2, Fortnite, Rocket League, Valorant, League).
  Verified live: Valve 43 cities, AWS 24 regions. Old Riot IPs (104.160.x) no longer answer.
- Phone upload fix: the file input lived inside the re-rendered `#app`, so after the picker returned the change event fired on a
  detached element. Input + progress box (with cancel) now live outside `#app`. Verified: headless Edge + CDP uploaded 40 MB, identical.
- Remote desktop from outside home: discussed, user declined for now (would need Cloudflare Tunnel or Tailscale; no admin).

- Sidebar redesign (2026-09-28): no more tab bars inside pages. 23 items in groups
  (top: Home/Apps/Games · Everyday · Power · Screen · PC · bottom: Phone/Settings), defined once in `NAV` in app.js
  as [page, sub, label, icon]; titles in `HEADS` (pages1.js). Tools and Power pages split by sub. File search moved to
  Settings. Sidebar badges show keep-awake On / plan Set / timer count. Panel height 900. One-click shutdown presets.

- NetLimiter-style limits: discussed and declined by the user (needs admin service + WinDivert driver; WinDivert 2.2.2's driver is signed by a third-party Chinese company; anti-cheat risk). Don't propose again unless asked.

## Backlog

- Background-diet "modes" beyond workspaces (start/stop services needs admin setup).
- Save guardian, death counter, backlog picker as Deck pages.
- Tile drag-to-reorder, custom layouts editor, bulk rename, image resizer, text snippets.
- Mouse side button trigger (needs a low-level hook; risky with anti-cheat).

## 2026-09-28: Game boost

New sidebar page **Game boost** (`ui/js/boost.js`, `src-tauri/src/boost.rs`), also in the tray
("Full power for gaming", "Clean up background apps") and on the phone (PC card). Built to work on
any PC (the user shares Deck with friends), unelevated.

- **Full power**: closes the ticked apps (WM_CLOSE to windows; background-only apps are ended),
  switches to the fastest power plan (picked by *personality*: Ultimate > High performance >
  any high-perf-based plan; else "Best performance" power-mode overlay on Balanced, via
  powrprof's undocumented overlay calls loaded with GetProcAddress), lowers background (no-window,
  non-voice) apps to below-normal priority, trims working sets. Everything is put back when the
  game closes (45 s with no game), from the Stop button, or on Deck quit. State is saved to
  `%APPDATA%\Deck\boost.json`; on startup a leftover file is restored (power plan always, apps and
  priorities only if the PC didn't reboot).
- **Clean up**: closes the ticked apps and trims memory; changes nothing else, reopens nothing.
- Game detection: game folders from Steam/Epic (games.rs), Riot (`ProgramData\Riot Games\Metadata\*.product_settings.yaml`),
  GOG/Ubisoft (HKLM), `X:\XboxGames`, plus any fullscreen program that isn't a browser/player.
- Never touched (no handle opened): NEVER list (launchers, anti-cheat, hardware/RGB/audio tools,
  recording, security, VPNs, runtimes/terminals), anything in a game folder, under %WINDIR%, other
  sessions, and **every console-subsystem exe** (read from the PE header). Voice/music apps are
  listed but never pre-ticked or slowed.
- Settings: `boost_close` (exe stem -> bool; missing = suggestion), `boost_power`,
  `boost_priority`, `boost_trim`, `boost_auto_end`.
- Tests: `cargo test --bin deck boost:: -- --include-ignored --nocapture` includes a read-only
  dry run (prints the app list and the plan choice) and a close/reopen/priority/trim test on a
  hidden throwaway `ping.exe`. The power-plan switch itself was not run live (it would change the
  user's plan); the WM_CLOSE path for windowed apps wasn't run live either.
- Checked with a Balanced-based custom plan active: Full power picks High performance and goes back to the custom plan afterwards.

## 2026-09-29: "asset not found: index.html"

Tauri's codegen caches brotli-compressed copies of `ui/` in
`<target>/release/build/<crate>-*/out/tauri-codegen-assets/<hash>.<ext>` and never rewrites an
existing file. The PC crashed mid-build on 2026-09-28, leaving broken copies, so the release exe
couldn't decompress any page (debug builds don't compress, so they looked fine). `install.ps1` now
deletes that folder before building, and `main.rs` has `asset_tests::pages_are_embedded`
(`cargo test --release --bin <crate> asset_tests`) to check a release build's pages.

## 2026-10-07: blank (white/black) window protection — v0.1.1

WebView2 sometimes shows a window that paints nothing (white or black) when a window created hidden is first shown, after a long hide, or when it
decides a window is covered. There is no official Microsoft fix, so `src-tauri/src/webview_guard.rs` (same file as FluxDM's) does four things:
- `apply_env()` (first thing in `main`) sets `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS` for every window: `CalculateNativeWinOcclusion` off, no background/renderer throttling, wry's default disable-features kept. An env var rather than per-window args because custom per-window args made other windows blank (tauri-apps/tauri #13092).
- `repaint_nudge()` resizes a just-shown window by 1 px and back after 350 ms (called from the show function).
- `ui/guard.js` (loaded first in index.html) calls `ui_heartbeat` every 4 s, but only while `requestAnimationFrame` is firing. `spawn_watchdog` nudges a visible window that has been quiet 15 s and reloads one that is quiet 90 s and focused.
- Not applied: `--disable-gpu` (last resort; costs smoothness).
Compiled, not run live. If a blank window still happens, check the command line of the app's `msedgewebview2.exe` for those flags first.
- Deck-specific: `show_deck` calls the nudge after every show (hot corner / tray); the watchdog watches the `main` window only (toast and zones are short-lived).

## 2026-10-12: cleanup + full hardware test pass
Tested live against HWiNFO (shared memory) with Cinebench/Heaven running; every test under 2 min.
- Passed: CPU profiles under all-core load (Full 4.6 GHz/125 W, Mid 3.67 GHz, Eco 2.7 GHz/37 W, Ultra 1.39 GHz/14 W),
  power limits (PL2 55 W held), min-clock lock (lands ~8% under target: Windows maps the % a bit low), fans fixed/curve/BIOS
  and release on `--quit`, GPU profiles under Heaven (P4 98 W power-limited, P1 259 W/2.05 GHz/mem +500), VF clock lock
  (exactly 1605 MHz at 850 mV) and its release, fixed GPU fan, RTSS overlay slot, open hotkey (panel + app mode),
  GPU profile hotkeys, Game boost on/off (plan Eco → High performance → Eco), phone remote (key check, WebSocket state, profile switch).
- Fixed:
  - WebView2 browser flags (anti-blank guard) never applied: wry's own args beat the env var. Now set on every window
    builder (`webview_guard::browser_args`); the main window is built in `setup` (`"create": false` in tauri.conf).
    Extra flags (a debug port) still come from WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS.
  - A 65 °C GPU temperature target left by Afterburner's linked limits stayed forever (Deck's profiles say "card's own"):
    stock throttled and the fans ran at 100%. The engine now learns the card default from Afterburner's `[Defaults]`.
  - The page sat frozen (no live ticks) after a start-up show whose "shown" event fired before the page listened:
    the page now asks `get_state.visible` at boot, on focus and on visibilitychange.
  - "Afterburner is running" warning kept showing after Stop tuning: new `afterburner_tunes` (startup/hotkeys/fan control).
  - Clean up showed 0 B for not-yet-measured items (size is now null until measured), RAM lighting toggle hidden on non-RGB
    kits, idle fan headers dimmed, cooler-display status wording, dark scrollbar for the nav, wider "When" select.
- Not changed: System Fan #1 runs flat out at any duty (3-pin fan on a PWM header?); EC channel 8 doesn't take writes
  (no header on this board). Clean up not run for real (would delete ~25 GB of caches).
- Testing tools: start Deck elevated with WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222, then drive
  it over CDP; synthetic keys must come from an elevated process (UIPI blocks them into the admin window otherwise).
- GPU default temperature target is now also saved to %APPDATA%\Deck\gpu-defaults.json (per PCI id), so Deck can still undo a leftover temp limit after MSI Afterburner is uninstalled (test default_temp_survives_uninstall).
- Later 2026-10-12: GPU profile re-applied ~5 s after wake from sleep (`gpu::watch_resume`, PowerRegisterSuspendResumeNotification);
  a CPU profile whose power plan was deleted gets a new "Deck · <name>" plan on apply/save (`cpu::ensure_plan`; `cpu_save`
  keeps the stored plan GUID; verified live); hardware report says "Deck" (Save report file still said MoboCtl).
  Afterburner + MoboCtl parity audit: nothing missing (details in DECK.md §4). DECK.md written.
- 2026-10-12 UI pass: Network page redesigned to fit one screen. It has a connection strip with a 60 s graph and click-to-copy IPs. Speed test and ping sit on the left; ping runs automatically every 5 s while the page is open and keeps a latency history strip. Game servers sit on the right and auto-test the first time a game is picked. Sensors page split into tabs (Graphs / All readings / Overlay & alerts). All readings is a device list plus a detail pane: per-core and per-fan rows fold into a grid (now + highest), other rows are grouped by unit, with search across everything. Checked with a mock-backend preview in headless Edge, because a game was running; not yet built or installed.
- 2026-10-12 UI pass over every page (checked with the fake-data preview, nothing built or installed yet because CS2 was running). Removed the Apps and Games pages (nav, code, styles). Home: new Performance tile (one-click CPU/GPU profile switch; `profiles` added to ALL_TILES in settings.rs, existing users get it once via a localStorage flag), shorter quick-action labels, two-line pinned labels. Game boost: one hero card (Full power + options + one-off Clean up), apps in a two-column grid with "Only suggested / None" and a memory total. Clipboard: kind icons, day groups, counts on filters, arrow keys/Enter/Delete. Sound: big master volume, device cards, two-column per-app mixer. Timers: progress bars. Keep awake: clear on/off state and one big button. Shut down when: quick picks first. CPU/GPU: 60 s sparklines in the live strip. Fans: fan rows on the left, curve editor on the right (rows linked to the curve, one "..." menu). This PC: identical memory modules grouped. Windows: column labels and "Not responding" chip. Display: hint when the monitor runs below its top refresh rate. Processes: usage bars. Startup: enabled first, summary line. Disk space: scanned drive highlighted. Clean up: sorted by size with bars. Workspaces: app icons and chips. Settings: tabs (General, Home, Clipboard & search, Profiles, About). Phone: allow switch on top, Android steps collapsed. Cooler display left as it was (this PC has none). Preview tool: tools/testing/ui-preview (shot.mjs, mock.js, mock-data.js, all.sh).

## 2026-10-12 (later) — overlay rework
- New `src-tauri/src/osd.rs`: overlay layout engine (grouped / list / one line, labels, units, columns, FPS figures on their own lines, warm/hot colours, per-part colours, clock 12/24 h). 47 items incl. per-thread and per-core rows (usage, clock, temperature), VRAM %, GPU limit, CPU VID, network, and this PC's own fans and board sensors (`fan:N`, `temp:N`).
- `rtss.rs`: frame statistics (avg / min / max / 1% / 0.1% lows) come from each RTSS app entry's own frametime ring (`dwStatFrameTimeBuf[1024]` at 924, next-write `dwStatFrameTimeBufPos` at 5020, shared memory v2.5+), read every 50 ms (2026-10-13; replaced the RTSS_Frametime pipe, which never received frames). RivaTuner's look (position, size, font, shadow, box, frametime graph, colour bar, fit) written to `Profiles\Global` and game profiles, originals in `Profiles\Deck backup`, reload via APPFLAG_PROFILE_UPDATE_REQUESTED.
- Sensors tabs: Graphs / All readings / Overlay / Alerts. `ui/js/overlay.js`: quick looks (incl. "Like MSI Afterburner"), reorderable items, colours/themes, preview.
- Hotkeys: plain Page Up/Down, Home, End, Insert, Delete, Pause, Scroll Lock, F-keys now allowed (like Afterburner).
- Not seen in a game yet: the tag rendering (<S0>, colours C3–C8, <APP>/<RES>/<API>) and the pipe stats.

## 2026-10-12 (evening) — UI audit: accessibility, lost clicks, small/large windows, phone remote
Full report with before/after numbers and screenshots: `docs/audits/2026-10-12-ui-audit.md`.
- **Fixed (each shown failing first with `tools/testing/ui-preview/regress.mjs`, now 17/17 PASS):**
  - Clicks were lost when a live page's 1-second redraw landed between mouse-down and mouse-up; redraws now wait while a button is held (`uiBusy`/`mouseHeld` in `app.js`, also Home tiles).
  - Keyboard focus is now kept across redraws (`focusSpot`/`refocus`).
  - Space no longer jumps to the search box.
  - Dialogs got role, a focus trap, focus return, and Escape resolves their promises.
  - Accessible names: switches, sliders, drop-downs and icon buttons (`nameControls` runs after each redraw), plus `aria-current`, `aria-pressed` and a live region for toasts.
  - Every control has a visible focus ring; reduced motion is respected; `--faint` raised to 4.5:1 contrast.
  - Clipboard toolbar: wraps, no twin Copy buttons. Fans list is wider. Settings is capped at 1120 px. The sidebar fades when it scrolls. Tall dialogs scroll.
  - Phone remote: zoom allowed, buttons and sliders named, bigger "All apps" target, album-art URL escaped, 640 px column on tablets.
- **New test tools:** `regress.mjs`, `a11y.mjs`, `harness.mjs`, `phone-shot.mjs`, `all-sizes.sh`. `shot.mjs` takes a `PORT` env. `mock.js` has Cooler display data.
- `restart-normal.ps1` must run **elevated**, or it leaves the debug instance running (it had been, since the earlier session). Noted in DECK.md.
- Builds this session used `-j 8`. `cargo test --release` gives 55 passed. Installed and checked live over CDP (all 28 pages, no errors), then restarted normally; port 9222 is closed.

## 2026-10-12 (night): feature audit, MicDrop removed from Deck
Full report: `docs/audits/2026-10-12-feature-audit.md` (screenshots kept locally).
- **MicDrop removed from Deck** (user request): Home tile, setting, `micdrop.rs` client, `micdrop_send`. MicDrop stays on Game boost's never-close list; Deck still never touches the recording device.
- **Fixed, with tests** (cargo 55 → 69, `regress.mjs` 17 → 26 checks, each new one failing on the old code):
  - Lost clicks: the sidebar was rebuilt every tick, and backend events redrew pages without waiting for the mouse. `render`/`renderSide` now hold while a button is down.
  - "Shut down when…": CPU and network readings froze while Deck was hidden (`sys` only sampled with the page open). Downloads and music conditions fired when nothing had started. Time plans missed during sleep ran on wake.
  - Keep awake "until downloads finish" ended before any download.
  - Crash guard disarmed by a hand quit. CPU plan-switch race. `rtss::stop` race. MAHM double writer when Afterburner starts after Deck.
  - Search: Enter ran the previous query's result; typo tolerance; games sorted and de-duplicated; kill/close/end now ask first.
  - Calculator: `200 + 10%` = 220.
  - Clean up: Recycle Bin no longer pre-ticked as "safe".
  - Network graph starts from backend history (`up_hist` added).
  - Timers and Phone no longer scroll sideways at 900×600.
- **Redesigned:** Keep awake and Shut down when… (two columns, live readings, Windows' own sleep times, plan sentence, status card with countdown).
- **Tools:** `mock.js` keeps power state (`window.__power`); `elev-install.ps1` logs the quit time (measured 2.1 s).
- **Installed and checked live over CDP; restarted normally; port 9222 closed.** Builds at `-j 8`.
- **Next:** toast-stack lost clicks; persist plans across restarts; Clipboard/Workspaces/Disk feature pass; single NVML handle.

- **Later the same night (installed at 19:20 once no game was open; toast hold and the all-users startup switch checked live over CDP; restarted normally, port 9222 closed):**
  - Toast window: toasts under the mouse stay and don't move; held changes run when the mouse leaves; the shutdown warning never waits (`toast-test.mjs`, 6 checks).
  - "Shut down when…" plans survive a restart (`plan.json`).
  - Maximized windows no longer count as games (`hw::game::covers_like_fullscreen`).
  - Paste-back only into the window it came from, never into a fullscreen game.
  - Clipboard: no silently cut text over 1 MB; at most 150 unpinned images (this PC has 90, so nothing deleted).
  - Workspaces: honest "partly ready" toast, no off-screen restore, apps asked to close before being ended.
  - Phone uploads stop before the drive has under 2 GB free.
  - Refresh rate is tested (`CDS_TEST`) before it's applied.
  - Search: "awake 2h" and "cancel shutdown".
  - Game boost crash recovery keeps `boost.json` until the restore is done.
  - Startup apps: all-users entries get an on/off switch when Deck runs as administrator (HKLM StartupApproved, like Task Manager); read-only otherwise.
  - Disk space: Windows' own folders and files (Windows tree, Program Files, Users, profile, drive roots, pagefile…) get a lock instead of a trash button, and the backend refuses them. Windows now warns before anything that can't go to the Recycle Bin is deleted for good (`FOF_WANTNUKEWARNING`).
  - Sound: only a connected playback device can be made default (never a mic, even from the phone); name matching refuses ambiguous names.
  - Screen tools: copy-text on very wide boxes no longer fails on the OCR size limit.
  - Installed again at 19:40 (quit 1.5 s); guards checked live over CDP; restarted normally, port 9222 closed.
  - Profile editors stay open when Deck hides (alt-tab), so a GPU "Try it" can be checked in a game; focus returns to the editor.
  - Dark mode schedule only acts at its switch times, so a manual Dark/Light choice holds until the next one.
  - Ending processes: anything Windows marks critical (`IsProcessCritical`) is never ended, from Processes, search or Game boost.
  - Windows page: setting a window back to 100% removes the see-through layering Deck added.
  - Installed again at 20:00 (no game open); see-through checked live on a Notepad window; restarted normally, port 9222 closed.
  - Small UI: Phone card shows only "off" while the connection is off; media without art shows a music note, not a play triangle (Home and Sound).
  - Final install 20:11 with every fix #1–#43 (no game open); restarted normally, port 9222 closed. One unplanned normal restart at about 20:09 (a restart chained after a build by mistake; no game was open).
  - cargo 75 passed; `regress.mjs` 29/29; `toast-test.mjs` 6/6; `a11y.mjs` 0 problems.
- **Mistake this session:** the second install ran while a game was open (the window list wasn't read first). It re-applied the same profiles and gave the fans to the BIOS for about 2 s. There have been no restarts since. A memory note now requires the game check as its own step.

## 2026-10-12 (night) — sidebar merge, help tips, ping Start/Stop, overlay font list
- **Notes removed** (page, Home tile, nav). `notes` dropped from ALL_TILES; the `notes` field stays in settings.json so saved notes aren't deleted. Settings' tile list skips tiles that no longer exist.
- **Sidebar merged:** Windows = tabs Open windows / Snap zones / Workspaces; Tools (in the PC group) = tabs Screen tools / Locked files / Windows panels. The Misc group is gone. `tabsHead()` in pages1.js; old `go("windows","layouts")` style links still open the right tab.
- **Network ping:** one round when the page opens, then live rounds only between Start and Stop (rounds run back to back, 4 pings each); leaving the page stops it. Strip keeps 60 rounds; row layout fixed so the values no longer overlap the strip.
- **"?" help tips** on every CPU and GPU setting (editors, curve tools, page sections): what it does, why use it, what changing it does, and a warning where there's a catch. `helpTip()` in core.js, texts in new `ui/js/help.js`. 24 px targets; hover/focus shows, click pins.
- **Overlay font:** was a text box with a suggestion list, which only offers entries matching what's typed, so it showed just "Unispace". Now a real dropdown: suggested fonts, then all fonts installed on this PC (new `fonts_list` command, GDI EnumFontFamiliesExW; 280 fonts here).
- Checked: cargo 76 passed (+1 ignored live font test run: 280 fonts); `regress.mjs` 34/34 (5 new checks); `toast-test.mjs` 6/6; `a11y.mjs` 0 problems. Installed at about 21:00 (no game open, checked on its own first; quit 1.6 s); live over CDP: new sidebar, 280-font dropdown, ping Start/Stop. Restarted normally; port 9222 closed.
- Version bumped to 0.2.0; the 0.2.0 setup built (NSIS, current-user install; admin mode is set up from inside Deck with "Set up"). 

## 2026-10-13 — frame stats fixed, power + frametime graphs, Customize, overlay grouping, more "?" tips

- Frame stats (avg, 1% / 0.1% lows, min, max) were never working: the pipe reader called DisconnectNamedPipe every 50 ms while waiting, kicking RivaTuner's hook off, and dropped frames whenever Deck itself was in front. Now read straight from RTSS's per-app frametime ring (see rtss.rs above); follows RTSS's foreground game while it draws, else the last app that drew. A "frame" over 1 s (game minimized / paused) starts the figures over. Checked live: DroidCam 30 fps, osu! ~985 fps.
- Sensors → Graphs: Power (CPU package + GPU board W, `History.cpu_w/gpu_w`), Frametimes (last 600 frames + Now / Average / 1% / 0.1% / Lowest / Highest, from `rtss_state.ft_recent/stats`).
- `layout.js`: Customize on Home and Sensors → Graphs: drag a panel by its bar, drag the corner to resize (twelfths of the width, height in 10 px), buttons for the same, hide / bring back, "Put everything back". Saved in `settings.panels[page]` (`layouts` was taken by snap zones); Home's order/visibility stays in `settings.tiles`. Sidebar width: drag its edge (`settings.nav_width`), double-click resets.
- Overlay "What to show": tick several (Shift-click ranges), move them together (up / down / top / bottom / drag), "Put on one line" with your own label (`osd_style.groups`: item → label; grouped and one-line layouts), remove together. Game RAM now sits on the RAM line.
- "?" tips added for Fans, Lighting, Cooler display, Sensors (alerts, CSV, power, frametimes), the overlay cards, Game boost and three Settings rows (86 entries in help.js).
- In-game frametime graph: RivaTuner's own `EnableFrametimeHistory` graph never showed beside Deck's text. Deck now writes an embedded graph object (RTSS_EMBEDDED_OBJECT_GRAPH 'GR00', FLAG_FRAMETIME, no samples: RTSS fills it) at the start of its OSD slot's object buffer (offset 4608) and ends its text with `<OBJ=00000000>`; the profile key is now always 0 so there is never a second graph. Slot contents checked with `dump_deck_slot`; whether it draws in a game still needs a look in osu!.
- Missing values ("–") were sent to RTSS as "?" (ANSI); now "-".
- Graph v2 (same night): RTSS's FLAG_FRAMETIME graph took a point per frame (unreadable at 1000 fps). Deck now sends its own points: `osd::GraphFeed` makes 120 points over `graph_secs` (5 s – 1 min, default 10), each the average of its frames (`graph_smooth`) or the slowest; `frame_loop` writes the object into Deck's slot each step (`write_graph`), the text loop with each text. `graph_fit` (default on) sizes it to the overlay's longest line (`osd::text_width`). Live: ~12 points/s, slot object 120 points, w −43, autoscaled top.
- Overlay on-delay (was 1–3 s: the loop slept `osd_ms` and took a fresh snapshot first): `rtss::refresh_now()` writes at once from the last snapshot and wakes the loop (`WAKE`); called by the toggle key and by set_setting for osd_on / osd_items / osd_style. Measured with `time_slot_appears`: slot written 4–7 ms after the switch. If RivaTuner itself isn't running, its own start still takes a few seconds.
- Reset key: `osd_reset_hotkey` (hotkey action ResetStats) and `rtss_reset_stats` (buttons on Sensors → Graphs Frametimes and Overlay → Behaviour) clear the figures and the graph.
- Installs: `Start-Process powershell -Verb RunAs -WindowStyle Hidden ... -WindowStyle Hidden -File elev-install.ps1` so no console appears.
- Tests: cargo 83 (+8 ignored, incl. `live_ring_stats`, `dump_rtss_apps`, `dump_deck_slot`); regress 37.
- Version 1.0.0 (2026-10-13): setup built with everything above.

## 2026-10-13 — published on GitHub
- Public repo https://github.com/MohammadFakih02/deck with README (screenshots from the fake-data preview, no real data), MIT `LICENSE`, `THIRD_PARTY_NOTICES.md` (PawnIO GPL-2.0 / modules LGPL-2.1, Fluent icons MIT), `CHANGELOG.md`, CI (`cargo test` + `node --check` on windows-latest), issue templates. Release v1.0.0 with the setup attached.
- Docs cleaned of personal data; personal facts moved to git-ignored `PRIVATE_NOTES.md`, untouched originals in `private/originals/`. Audit reports moved to `docs/audits/` (screenshots stay local in `artifacts/`).
- New rule, written into CLAUDE.md, DECK.md (§0, §9), this file and CONTRIBUTING.md: update the docs before every commit. Enforced by `.githooks/pre-commit` (`git config core.hooksPath .githooks`): code changes need `PROJECT_STATE.md` and `CHANGELOG.md` staged in the same commit.
- CI fix: the UI check step failed on the first run (it also looked for `src-tauri/phone/*.js`, which doesn't exist, so the loop ended on a false test). Now checks `ui/js/*.js` and `ui/guard.js` and stops on the first real error.
