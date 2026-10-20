# Deck controller overlay and controller battery: design

Status: design, all user questions answered; waiting for the user's read-through. Written 2026-10-18.

## 1. Goal

Merge the standalone **Controller Overlay** app (Python, pygame-ce + tkinter + ctypes,
v4.2) into Deck completely, and add a **universal controller battery** reader. When Deck's
version matches the old app feature for feature, the old app is uninstalled.

What the user asked for:
- A **full merge**: everything Controller Overlay does moves into Deck. No standalone app.
- The controller battery becomes **universal**: any controller that reports a battery, not
  only the GameSir Nova 2 Lite the old app supports.
- **One tray icon per controller** that reports a battery.
- The battery shows **in the controller overlay's battery pill**, as in the old app.
  **Not** in RivaTuner's in-game overlay.

What success looks like:
- Every feature in §3 works in Deck, and the old app can be uninstalled with nothing lost.
- A PS4, PS5, Xbox or GameSir controller shows its battery in the tray and on the overlay.
- Deck stays light while a game runs (§2) and safe with kernel anti-cheat.
- Nothing is specific to one PC: friends get the same feature.

Non-goals:
- The boss death counter. It comes later and reuses this overlay engine; its plan is kept
  outside the repo for now.
- Any in-game (injected / present-chain) drawing. The overlay is an ordinary top-level window.
- Console modes of the old app (`--detect`, `--diagnose`, `--version`); see §6.8 for what
  replaces them.

## 2. Constraints that shape the design

| Constraint | Consequence |
|---|---|
| Kernel anti-cheat (Vanguard, EAC, BattlEye) | No input hooks, no injection. Controller input and battery come from device APIs. Keyboard & mouse input: see §7, question 1. |
| Deck runs elevated, from Program Files | The overlay window belongs to an elevated process: fine for drawing over games. Files it writes go through `util::write_atomic`. |
| The maintainer's PC hard-resets under heavy CPU load | Overlay budget: < 1% of a core while idle or faded, ~2% while animating at 60 fps. No busy loops, no `requestAnimationFrame` loops. |
| Deck is public and shared | No PC-specific names, ids or lists. Vendor protocols are keyed by USB VID/PID, which is fine. |
| UI is vanilla JS embedded in the exe | Settings become a Deck page; the overlay itself is native (not a WebView) so it can be click-through, per-pixel alpha and cheap. |
| Lessons from the Python app | Keep: thread-paced frame clock (no busy timer spin), tray windows dispatched by hwnd, battery smoothing, a dongle's battery applies only to that dongle's pad, debounced "controller off". |

## 3. Feature inventory (parity checklist)

Everything below exists in Controller Overlay 4.2 and must exist in Deck.

**Display**
- Click-through, per-pixel-alpha, always-on-top overlay window that never takes focus
  (`WS_EX_LAYERED | TRANSPARENT | TOOLWINDOW | NOACTIVATE`), topmost re-asserted every 500 ms.
- **Layouts:** `controller` (DS4-shaped, 409×272 design units) and `compact` (304×124).
- **Themes:** `ds4`, `midnight`, `dualsense`, `gold`, `mono` (colour tables in the old source).
- **Styles:** `glass`, `solid`, `minimal`.
- **Button labels:** `playstation`, `xbox`, `auto` (guessed from the pad's name).
- Live sticks with deadzone 0.12, analog triggers, pressed-button glow (`halo`), light bar.
- **Battery pill** under the pad: battery icon + `NN%` (bold), bolt while charging, red when
  low/empty, green when charging/wired; hidden when there's no reading.
- **Scale** 0.5–3.0 on top of the monitor's DPI; **opacity** 0.15–1.0.
- **Position:** corner (top/bottom × left/center/right) + pixel offsets; **monitor**: `auto`
  (the monitor the foreground game is on, ignoring the overlay, Deck's own windows, the
  taskbar and the desktop) or a fixed index.
- **Idle fade** after 6 s without input to 30% opacity; **hide without a controller**.
- **Toasts:** a glass pill next to the overlay ("Connected · <name>", "Controller
  disconnected", "Controller battery low · 12%", "Position saved", mode changes).
- **Edit mode** ("Move / resize"): overlay stops being click-through; drag to move, mouse
  wheel to resize, right-click to finish; saves the position.
- **Demo mode:** animated fake input, no controller needed.

**Keyboard & mouse mode**
- A keyboard block (28 keys: Esc, 1–5, Tab, Q–T, Caps, A–G, Shift, Z–B, Ctrl, Alt, Space),
  showing only the keys the user picked, plus a mouse (left, right, middle, X1, X2, wheel,
  movement).
- Stuck-key resync once a second (a key released while a secure desktop had input).

**Controller input**
- Any SDL-recognised pad (DS4 and clones, DualSense, Xbox, others), first mapped pad wins;
  unmapped pads are named ("not recognised: <name>").

**Battery (old: GameSir only)** — becomes §5.

**Tray (old app)**
- Main icon: left-click → Settings; right-click → Settings, Show overlay, Move / resize,
  Controller / Keyboard & mouse, Demo input, Quit.
- Battery icon(s): see §5.4.

**Settings window (old app)** — becomes the Deck page in §6.6: Look (theme, style, layout,
labels, scale, opacity), Keys (keyboard picker, show mouse), Position (corner, monitor,
Drag to place…), Behaviour (fade, hide without pad, start with Windows), status line,
Reset to defaults.

## 4. Architecture

New backend module `src-tauri/src/pad/`:

| File | What |
|---|---|
| `mod.rs` | Glue: starts the workers, owns the shared state, Tauri commands via a new `padcmd.rs` adapter |
| `input.rs` | Controller input (SDL3), keyboard & mouse input, demo input → one `InputState` per frame |
| `battery/` | `mod.rs` (merge + smoothing), `gamesir.rs`, `sony.rs` (DS4 / DualSense HID), `wgi.rs` (Windows.Gaming.Input), `bt.rs` (Bluetooth battery property), `xinput.rs` |
| `art.rs` | Builds the SVG scene (port of `Art` / `KbmArt`): static base layer + per-button overlays, cached per (layout, theme, style, scale, family) |
| `raster.rs` | SVG → BGRA via `resvg` / `tiny-skia`; blur for halos/shadows |
| `window.rs` | The layered overlay window + toast window: styles, `UpdateLayeredWindow`, topmost, click-through toggle for edit mode, `WDA_EXCLUDEFROMCAPTURE` |
| `place.rs` | Monitors, DPI, auto-monitor from the foreground window, anchor/offset maths |
| `tray.rs` | Per-controller battery tray icons (icon pixels port of `battery_icon_pixels`) |
| `settings.rs` part | A `pad` section in Deck's `settings.json` (§6.7) |

Threads:
- **Overlay thread** (one, owns the Win32 windows and its message loop): frame clock on a
  high-resolution waitable timer — 60 Hz while something moves, 30 Hz faded, 20 Hz hidden,
  stopped entirely when the overlay is off. Reads input, draws only when the state changed.
- **Battery thread**: polls every 2 s (vendor HID requests are tiny), publishes readings.
- Tauri commands only read/write settings and the published state.

## 5. Universal battery

### 5.1 Sources, in order (first useful answer per controller wins)

| # | Source | Covers | Gives |
|---|---|---|---|
| 1 | **Vendor HID protocols** keyed by VID/PID | GameSir Nova 2 Lite dongle (`3537:1098`, interface `MI_02`, usage page `0xFF7A`): write 65 bytes `00 01 01…`, reply starting `00 01`, MAC at [2..8], battery at [19]: 0–100 %, `0xFF` = charging (no level), bit 7 = charging | exact % |
| 2 | **Sony HID input reports** | DS4 (054C:05C4/09CC and the wireless adapter 0BA0) and DualSense (054C:0CE6/0DF2), USB and Bluetooth report layouts | % + charging |
| 3 | **Windows.Gaming.Input** `RawGameController.TryGetBatteryReport` | Xbox controllers and others Windows knows | remaining/full → %, charging status |
| 4 | **Bluetooth battery device property** (the % Windows shows in Bluetooth settings) | Bluetooth controllers that report it | % |
| 5 | **XInput** `XInputGetBatteryInformation` | XInput pads | empty / low / medium / full only |

- A source that says "wired" or "unknown" doesn't win over a lower source with a real level.
- If SDL3 (§6.1) exposes `SDL_GetGamepadPowerInfo` with a percent, it slots in as source 2b.
- New vendor protocols are small files in `battery/`, added when someone cracks one.

### 5.2 Identity
- A controller's identity is its device instance path (or MAC when the protocol gives one),
  so two identical pads stay two entries.
- The display name comes from the device (product string), not a hard-coded list; vendor
  readers may supply a nicer name (e.g. "GameSir Nova 2 Lite").

### 5.3 Behaviour (carried over)
- **Smoothing:** on battery the shown % never rises; it resets on charging or a rise of 20+
  points. The floor is remembered per controller in `%APPDATA%\Deck\pad-battery.json`.
- **Off = gone:** a controller counts as off after 2 silent polls in a row (one dropped reply
  doesn't hide it).
- **Charging:** bolt; with a level when the source gives one, without when it doesn't.
- **Low warning:** a Deck toast once per discharge when a controller crosses 15% (my call).
- **Overlay pairing:** the pill shows the battery of the controller the overlay is
  displaying (matched by VID/PID / device path), never another pad's.

### 5.3b Decisions from the design review (builder's calls, marked so the user can check them)
- **Passive Sony reading (my call):** never send the feature request that switches a
  Bluetooth DS4/DualSense into its extended report; read the battery only from reports the
  pad already sends (USB always; Bluetooth when a game, Steam or SDL already switched it).
  SDL3 gets the hints that keep it passive too (no enhanced-report switching, no rumble/LED
  writes). Reason: switching can break a game's or another tool's input until the pad
  reconnects. A Bluetooth Sony pad in simple mode falls through to sources 3–4.
- **Virtual pads (my call):** DS4Windows-style tools (ViGEm virtual Xbox pads, HidHide) make
  one controller appear twice or hide the real one. Deduplicate: a ViGEm virtual pad never
  gets its own tray icon or battery when a physical controller is visible; the overlay prefers
  the physical device for its battery pill but may display the virtual pad's input (that is
  what the game sees). If HidHide hides the physical device from Deck, show no battery rather
  than a wrong one.
- **Defaults for new users (my call):** battery tray icons **on**, low warning **on**, overlay
  **off**. The battery thread runs only while tray icons are on or the overlay is on; with both
  off it opens no devices.

### 5.4 Tray icons
- **One tray icon per controller with a reading**, created when it appears, removed when it
  goes off. Phone-style: dark battery, outline in the state colour, level fill, `NN` inside,
  bolt when charging with no level, tiny controller silhouette on the lower-right edge.
- Tooltip: `<name> · 64%` / `<name> · charging`.
- Click: opens Deck on the controller page. Right-click: Deck's normal tray menu.
- Built with Tauri's `TrayIconBuilder::with_id("pad-<n>")` and a generated RGBA image.

## 6. The rest of the port

### 6.1 Controller input (decided in Phase 0)
- **Preferred: SDL3** (`sdl3` crate, prebuilt `SDL3.dll` shipped next to `deck.exe`),
  because it's what the old app used (SDL2), it maps DS4 clones, DualSense and Xbox the same
  way, it reads in the background with `SDL_HINT_JOYSTICK_ALLOW_BACKGROUND_EVENTS`, and it
  can add a battery source.
- Fallback if SDL3 can't be shipped cleanly: `gilrs` (pure Rust, SDL mapping database).
- Pick the first mapped pad, like the old app; name unmapped ones.

### 6.2 Rendering
- Port the SVG builders (`body_path`, `theme_defs`, buttons, sticks, triggers, glyphs,
  battery pill, keyboard/mouse art) as string builders, rasterise with `resvg`.
- Cache: the static base per look/scale; per-frame work = composite cached sprites (pressed
  caps, halos, stick positions) into one BGRA buffer, then `UpdateLayeredWindow`.
- Text: Segoe UI from `%WINDIR%\Fonts` through `resvg`'s font database (fallback: any sans).

### 6.3 Window behaviour
- Same styles and topmost handling as the old app; never activates.
- `SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)` on the overlay and its toast (keeps the
  overlay out of screenshots/recordings of the game, and out of the future death counter's
  capture). Make it a setting ("Hide from screen capture", default on) in case users *want*
  it in recordings.
- Known limit, documented: exclusive fullscreen / DWM independent flip can hide any external
  overlay (the old app's notes H3).

### 6.4 Toasts
- The overlay's own glass toast pill (native, next to the overlay), not Deck's WebView toast,
  so it shows over games with the overlay. Deck's normal toast is used for the low-battery
  warning when the overlay is off.

### 6.5 Tray menu
- No second "Controller Overlay" tray icon. Deck's tray menu gains a **Controller overlay**
  submenu: Show overlay, Move / resize, Controller / Keyboard & mouse, Demo input, Settings…

### 6.6 Deck page
- New page **Controller** in the **Monitor** group (next to Overlay, which is RivaTuner's).
- Tabs: **Overlay** (on/off, theme, style, layout, labels, scale, opacity, corner, monitor,
  Drag to place…, fade, hide without controller, hide from screen capture),
  **Keys** (keyboard picker, show mouse), **Battery** (every controller seen, its source,
  level, charging, last seen; tray icons on/off; low warning on/off), **Test** (live input
  view of the active pad: buttons, sticks, triggers, raw numbers — replaces `--detect`).
- Same look and test tooling as other Deck pages (`ui-preview/shot.mjs`, `regress.mjs`).

### 6.7 Settings and migration
- New `pad` object in `settings.json` with the old keys (`anchor_h`, `anchor_v`, `off_x`,
  `off_y`, `monitor`, `scale`, `opacity`, `theme`, `style`, `layout`, `labels`, `fade`,
  `hide_no_pad`, `mode`, `keys`, `mouse`) plus `on`, `capture_hidden`, `battery_tray`,
  `battery_warn`. Validation identical to the old `Settings.CHECKS`.
- **One-time import** of `%APPDATA%\ControllerOverlay\settings.json` and `battery.json` on
  first start, if present.
- The overlay is **off by default** for new users (friends), on after an import.

### 6.8 Diagnostics
- The Test tab replaces `--detect`. A `--pad-diagnose` command-line mode prints the z-order /
  foreground verdict like the old `--diagnose` (debug aid, optional, my call).

## 7. Questions for the user (all answered 2026-10-18)

1. **Keyboard & mouse mode input.** The old app reads keys and mouse through Windows **Raw
   Input** (`RegisterRawInputDevices` with `RIDEV_INPUTSINK`). It isn't a hook and games use
   it themselves, but it isn't on Deck's allowed list (`RegisterHotKey`, cursor polling,
   WinEvent hooks, `GetAsyncKeyState`). Options:
   - **A. Raw Input** (full parity: keys, mouse buttons, wheel, movement).
   - **B. `GetAsyncKeyState` polling only** (inside Deck's rules: keys and mouse buttons;
     **no wheel and no mouse movement** while a game holds the cursor).

   **Answered: A.** Raw Input, receive-only (`RIDEV_INPUTSINK`, no `RIDEV_NOLEGACY`, never
   blocks or alters input), registered only while keyboard & mouse mode is on and unregistered
   when it's off. Phase 7 adds Raw Input to Deck's allowed-input list in `DECK.md`
   with this reasoning. `GetAsyncKeyState` stays for the stuck-key resync.
2. **Retiring the old app:** answered — **the user uninstalls it themselves.** Deck never
   removes or touches the old install; it only imports its settings once (§6.7).
3. **Test hardware:** answered — the user has **only the GameSir** (Nova 2 Lite on its
   dongle). It is the one live pass test; Sony, Xbox-over-WGI, Bluetooth-property and XInput
   sources are verified with unit tests against recorded/spec reports and must fail quietly
   (no icon) rather than guess.

## 8. Phases (each ends with a test)

0. **Spikes (throwaway):** SDL3 from the elevated Deck process — background input from a
   DS4/Xbox/GameSir pad while another app has focus, and its battery info; `resvg` renders the
   DS4 scene into a layered window at < 2% CPU. *Pass:* both work; else take the fallbacks.
1. **Battery engine + tray icons** (§5), with its unit tests (parsers against recorded
   reports, smoothing). *Pass:* the GameSir shows a correct icon that appears/disappears with
   the controller; the other sources pass their unit tests (no other pads to test with).
2. **Overlay window engine** (§6.3, placement, DPI, auto-monitor, fade, toasts). *Pass:* a
   test scene follows the foreground game across monitors, never takes focus, < 1% CPU idle.
3. **Art port** (§6.2), every theme × style × layout × label family. *Pass:* side-by-side
   renders against the old app match.
4. **Controller mode + demo + battery pill.** *Pass:* live play shows input with no lag;
   the pill matches the tray.
5. **Keyboard & mouse mode** via Raw Input (question 1, answer A). *Pass:* keys, buttons,
   wheel and movement show in a game that locks the cursor; no stuck keys after alt-tab.
6. **Deck page, tray submenu, edit mode, settings import.** *Pass:* UI regression suite
   green; every old setting reachable.
7. **Parity pass + docs** (the user retires the old app themselves). Walk §3 line by line; update `DECK.md`,
   `README.md`, `CHANGELOG.md`, `PROJECT_STATE.md`, `THIRD_PARTY_NOTICES.md` (SDL3, resvg).

## 9. Left to the builder
- Exact crate versions; how sprites are cached; the overlay thread's message-loop shape.
- Whether battery polling shares a thread with other `hw` work.
- Icon pixel details, as long as they match the old icons.
