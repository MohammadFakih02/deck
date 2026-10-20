# Controller overlay + controller battery Implementation Plan

**Goal:** Move Controller Overlay (Python) into Deck completely and add a universal controller battery with one tray icon per controller.

**Architecture:** A new `src-tauri/src/pad/` module. One battery thread merges five sources and publishes readings; one overlay thread owns the native layered windows, a high-resolution waitable-timer frame clock and the Raw Input window; SDL3 is loaded at run time from an embedded, gzipped `SDL3.dll` (XInput when it can't load). Art is SVG strings rasterised once per look with `resvg`, then composited per frame. Settings are a `pad` object in `settings.json`; the UI is a new "Controller" page with tabs.

**Tech Stack:** Rust (Tauri 2, `windows` 0.62, `windows-sys` 0.61), `resvg` 0.48 (text only), SDL 3.4.16 (zlib licence), vanilla JS UI.

**Spec:** `docs/design/specs/2026-10-18-controller-overlay-design.md`

## Global Constraints

- No input hooks, no injection. Raw Input (`RIDEV_INPUTSINK`, never `RIDEV_NOLEGACY`) only while keyboard & mouse mode is on.
- Overlay CPU: < 1% of a core idle/faded, ~2% animating at 60 fps. Frame clock 60 Hz lively / 30 Hz faded / 20 Hz hidden; stopped when the overlay is off.
- Never activate: `WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE`, topmost re-asserted every 500 ms, `SWP_NOACTIVATE` everywhere.
- Persist through `util::write_atomic`; battery floors in `%APPDATA%\Deck\pad-battery.json`.
- Passive Sony: never send feature reports to a Sony pad; SDL hints `SDL_JOYSTICK_ENHANCED_REPORTS=0`, `SDL_JOYSTICK_ALLOW_BACKGROUND_EVENTS=1`.
- Defaults: `battery_tray` on, `battery_warn` on, overlay `on` off (on after an import of the old app's settings). Battery thread opens nothing while tray icons and overlay are both off.
- Old values kept exactly: deadzone 0.12, idle fade 6 s to 30%, smoothing reset at a 20-point rise, 2 misses = off, low warning at 15%, scale 0.5–3.0, opacity 0.15–1.0, offsets ±5000, monitor `auto` or 0–15.
- Public repo: nothing PC-specific. Builds: `CARGO_TARGET_DIR` as in `PRIVATE_NOTES.md`, `-j 6`, BelowNormal, hidden windows.

## Review Focus

1. **Controller off but dongle plugged in** → no tray icon, pill hidden (GameSir reply with a zero MAC, or no reply twice).
2. **Two identical controllers** → two tray icons (identity = device path / MAC), never one flickering icon.
3. **Monitor unplugged / DPI change while the overlay shows** → it re-places on the remaining monitor, never off-screen.
4. **Deck's own windows or the taskbar in front** → auto-monitor keeps the game's monitor.
5. **Settings file with junk in `pad`** → each bad field falls back to its default, the rest is kept.

Each has a unit test in the owning task (`battery::tests::zero_mac_is_off`, `battery::tests::two_pads_two_entries`, `place::tests::clamped_inside_monitor`, `place::tests::shell_and_own_windows_ignored` (pure predicate), `pad::tests::bad_fields_fall_back`).

---

### Task 1: Settings + battery engine

**Files:** Create `src/pad/mod.rs`, `src/pad/cfg.rs`, `src/pad/battery/{mod,hid,gamesir,sony,wgi,bt,xinput}.rs`. Modify `src/main.rs` (`mod pad;`), `src/settings.rs` (`pub pad: crate::pad::cfg::PadSettings`, `pub pad_imported: bool`).

**Interfaces (produces):**
- `cfg::PadSettings` (serde, `#[serde(default)]`), fields per spec §6.7; `fn clean(&mut self)`; `cfg::KEYS: [(&str, &str, u16, u8, f32, f32); 27]` (name, label, vk, row, x, w).
- `battery::Reading { percent: Option<u8>, coarse: Option<&'static str>, charging: bool }`
- `battery::Entry { id: String, name: String, vid: u16, pid: u16, source: &'static str, reading: Reading, seen: u64 }`
- `battery::Floors::steady(&mut self, id, Reading) -> Reading`
- `battery::merge(Vec<Entry>) -> Vec<Entry>` (dedup rules §5.3b)
- `battery::Misses::update(&mut self, polled: Vec<Entry>) -> Vec<Entry>` (2 silent polls)
- parsers: `gamesir::parse(&[u8]) -> Option<(Option<u8>, bool, [u8;6])>`, `sony::parse_ds4(&[u8]) -> Option<Reading>`, `sony::parse_dualsense(&[u8]) -> Option<Reading>`, `wgi::to_reading(remaining, full, status) -> Option<Reading>`, `xinput::to_reading(type, level) -> Option<Reading>`.
- `battery::start(shared: Arc<Shared>)`, `Shared.battery: Mutex<Vec<Entry>>`.

- [ ] Tests: `gamesir_reply_parses` (`00 01 <mac> … [19]=0x40` → 64%, `0xFF` → charging no level, `0xC8` → 72% charging), `zero_mac_is_off`, `ds4_usb_and_bt_offsets`, `dualsense_usb_and_bt_offsets`, `smoothing_never_rises_and_resets_at_20`, `charging_clears_floor`, `two_misses_hide`, `two_pads_two_entries`, `virtual_pad_deduped`, `wired_xinput_skipped`, `bad_fields_fall_back`.
- [ ] Implement, `cargo test pad::` passes.

### Task 2: Rendering (raster + art + icon)

**Files:** `src/pad/raster.rs`, `src/pad/art.rs`, `src/pad/icon.rs`; `Cargo.toml`: `resvg = { version = "0.48", default-features = false, features = ["text"] }`.

**Interfaces:** `raster::Img { w, h, px: Vec<u8> /* premultiplied RGBA */ }`, `raster::svg(body, defs, k, box) -> (Img, (i32,i32))`, `raster::halo(&Img, color, radius, strength) -> (Img, i32)`, `Img::blit(&mut self, &Img, x, y)`, `Img::blit_area`. `art::Art::new(Look) -> Art`, `art::Art::render(&mut self, &Frame) -> Option<&Img>`, `Look { layout, theme, style, k, family, keys, mouse }`, `art::pill(lines, theme, k, border) -> Img`. `icon::battery(size, percent, charging) -> Vec<u8> /* RGBA straight */`.

- [ ] Tests: `every_look_builds` (5 themes × 3 styles × 2 layouts × 2 families + kbm), `same_input_draws_once` (second render returns None), `battery_icon_is_size_squared`, `pill_wider_with_longer_text`.

### Task 3: Input (SDL3, XInput, demo, Raw Input)

**Files:** `src/pad/sdl.rs`, `src/pad/input.rs`, `vendor/sdl3/SDL3.dll.gz` + `LICENSE.txt`.

**Interfaces:** `input::PadState { buttons: [bool;16], lx, ly, rx, ry, l2, r2 }` keyed by `input::BUTTONS`; `input::Reader::read(&mut self) -> Option<PadState>`, fields `name`, `family`, `vid`, `pid`, `note`, `sdl_battery`; `input::demo(t) -> PadState`, `input::demo_kbm(t) -> KbmState`, `input::Kbm` (Raw Input, `enable(bool)`, `on_input(lparam)`, `read()`, `resync()`); `input::is_active`, `input::kbm_active`.

- [ ] Tests: `demo_covers_every_button`, `deadzone_centres_small_moves`, `family_from_name`, `raw_mouse_flags_map`.

### Task 4: Window, placement, overlay thread

**Files:** `src/pad/window.rs`, `src/pad/place.rs`, `src/pad/overlay.rs`.

**Interfaces:** `place::Monitor { handle, rect, dpi, primary }`, `place::geometry(&PadSettings, &Monitor, w, h) -> (i32,i32)`, `place::anchor_from(x,y,w,h,&Monitor) -> (h, v, off_x, off_y)`, `place::ignore_fg(class, own) -> bool`; `overlay::Cmd` (`Settings`, `Visible`, `Edit`, `Demo`, `Mode`, `Quit`); `Shared.post(Cmd)`.

- [ ] Tests: `clamped_inside_monitor`, `anchor_round_trip`, `shell_and_own_windows_ignored`, `fade_steps`.
- [ ] Live: overlay follows foreground across monitors, never takes focus, CPU per budget.

### Task 5: Deck integration (commands, tray, page, import)

**Files:** `src/padcmd.rs`, `src/main.rs` (submenu, `pad-<n>` tray icons, commands, `set_setting` "pad"), `ui/js/controller.js`, `ui/js/app.js` (NAV), `ui/js/main.js`, `ui/app.css`, `tools/testing/ui-preview/mock.js`.

- [ ] `pad_state`, `pad_action`, `pad_test` commands; tray submenu ids `pad:visible|edit|controller|kbm|demo|open`.
- [ ] Import `%APPDATA%\ControllerOverlay\settings.json` + `battery.json` once (`pad_imported`).
- [ ] Page renders in `shot.mjs` for every tab without script errors; `regress.mjs` all PASS.

### Task 6: Docs + parity pass

- [ ] `DECK.md` (rule 3: Raw Input allowed for keyboard & mouse mode only; code layout; data files), `README.md`, `CHANGELOG.md`, `PROJECT_STATE.md`, `THIRD_PARTY_NOTICES.md` (SDL3 zlib, resvg MIT/Apache), `hooks.nsh` + `uninstall.ps1` remove `SDL3.dll`.
- [ ] Walk spec §3 line by line against the running build.
