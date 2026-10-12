# Deck hardware control: MoboCtl merge, CPU and GPU profiles

Status: design, waiting for review. Written 2026-10-11.

## 1. Goal

One app. Deck takes over everything MoboCtl does and the tuning half of MSI Afterburner, then MoboCtl is uninstalled.

What the user asked for:
- Move Deck's little-used pages into a **Misc** group. Nothing is deleted.
- Bring **everything from MoboCtl** into Deck: fan control and curves, RGB lighting, cooler display, CPU clock limits and power modes, RAM cleanup, the sensor monitor, This PC and reports.
- **Afterburner-style GPU control** on NVIDIA cards: power limit, core clock offset, memory clock offset, voltage, voltage/frequency curve and fan speed. The user chose **full parity**, which includes the undocumented NVIDIA calls.
- **Separate CPU profiles and GPU profiles**, mixed and matched the way the user does today with plan shortcuts plus Afterburner hotkeys.
- Profiles are switched by:
  - **keyboard shortcuts**;
  - **desktop shortcuts** (and Start menu shortcuts), with an **icon picked from a built-in set**;
  - Deck's own menus.
- Deck **runs as admin**. The user said that is fine.
- ~~MSI Afterburner and RivaTuner stay installed for now.~~ **Updated: Afterburner is fully replaced and uninstalled; RTSS stays** (FPS counter/limit). Deck feeds RTSS's overlay like Afterburner did (§6.7b, §6.10). Deck draws no overlay of its own and injects nothing into games.

What success looks like:
- MoboCtl can be uninstalled and nothing it did is lost.
- The user can press one key, or double-click one desktop icon, to get "CPU Ultra + GPU Profile 5", without a UAC prompt or a window popping up.
- A CPU clock cap **holds while a game is in front**. Today it does not, because of Windows Game Mode.
- Deck stays safe to hand to friends. A page hides when their PC has nothing for it, and nothing is specific to this PC.

Non-goals:
- RTSS overlay text.
- AMD GPU tuning. AMD cards are monitored only, as MoboCtl does today.
- Laptop GPU switching.
- NetLimiter-style limits (declined before).

## 2. Constraints that shape the design

| Constraint | Consequence |
|---|---|
| Deck runs as admin | Anything Deck launches would also run as admin (Steam, games, Discord). Deck must start apps, files and links **as the normal user** (§4.3). |
| Autostart today uses the HKCU `Run` key (tauri-plugin-autostart) | `Run` cannot start an admin program without a UAC prompt. Autostart moves to a **scheduled task** that runs with highest privileges. |
| Admin exe in a user-writable folder | Any program the user runs could replace `Deck.exe` and gain admin. Deck moves to **`C:\Program Files\Deck`**. |
| Online games with anti-cheat (Vanguard, EAC, BattlEye) | Still **no input hooks**. Keyboard shortcuts use `RegisterHotKey`, which is a normal Windows API and not a hook. Afterburner's own hotkeys work the same way. |
| PCs can crash or reset unexpectedly | A GPU overclock must never be re-applied blindly after a crash (§6.6). |
| Afterburner stays installed | It can override Deck: its startup profile at logon, its Ctrl+Alt hotkeys, and its fan control. Deck **detects and warns**, and offers an opt-in "stop Afterburner from tuning" (§6.7). |
| The user may be in a match | Never test hardware writes without asking first. Read-only tests are fine. |
| Unelevated processes cannot message an admin process (UIPI) | Shortcuts reach the running Deck through an elevated scheduled task, not through a plain second instance (§7.3). |

## 3. Navigation

```
(top)     Home · Apps · Games · Game boost
Everyday  Clipboard · Sound · Timers & breaks
Power     Keep awake · Shut down when…
Hardware  CPU · GPU · Fans · Lighting · Cooler display · Sensors · This PC      <- new
Screen    Windows · Display
PC        Processes · Startup apps · Network · Disk space · Clean up
Misc      Snap zones · Workspaces · Screen tools · Notes · Locked files · Windows panels
(bottom)  Phone · Settings
```

- Hardware pages hide when the PC has nothing for them:
  - **Fans** hides when there is no fan chip and no HP WMI fans.
  - **Lighting** hides when there is no Mystic Light controller and no Fury RAM.
  - **Cooler display** hides when no display is found.
  - **GPU** hides when there is no NVIDIA card. On AMD it shows monitoring only.
- If Deck is not running as admin, the Hardware pages show a single line, "Hardware control needs Deck to run as admin", with a **Set up** button. That button gives one UAC prompt, creates the task and restarts Deck elevated.
- Sidebar badges: CPU and GPU show the active profile's short name.
- MoboCtl's "Clear RAM now" goes on the **Clean up** page.

## 4. Running as admin

### 4.1 Install (`install.ps1`)
- Asks for admin itself (one UAC prompt per install).
- Builds the release as today, including deleting the codegen asset cache.
- Copies to `C:\Program Files\Deck\Deck.exe`. If the old per-user install exists, it moves it: deletes `%LOCALAPPDATA%\Programs\Deck` and the old HKCU `Run` value for Deck. Settings stay in `%APPDATA%\Deck`, untouched.
- Creates the scheduled task `\Deck\Deck`:
  - trigger: at logon of this user;
  - action: `Deck.exe --minimized`, run with highest privileges and interactive token;
  - no time limit, runs on battery, normal priority (not the task default of below normal);
  - only one instance at a time; a second start goes to the running Deck through single-instance.
- Start menu `Deck.lnk` points at the hidden runner (§7.3), which starts that task. So opening Deck from Start gives no UAC prompt.
- New `uninstall.ps1`:
  - quits Deck, which releases the fans;
  - removes every `\Deck\*` task, Deck's desktop and Start menu shortcuts, and `C:\Program Files\Deck`;
  - leaves settings in place unless `-Purge` is passed;
  - never touches power plans other than Deck's own (and only after a prompt).

### 4.2 Manifest and fallback
- The manifest stays `asInvoker`. Double-clicking `Deck.exe` directly starts it **unelevated**. Everything still works except the Hardware pages, which show **Set up** (§3).
- `elevation.rs` provides:
  - `is_admin()`;
  - `ensure_task()`, which creates or repairs the logon task, used by **Set up**;
  - `relaunch_elevated()`, which runs the task and exits this copy.
- `tauri-plugin-autostart` is removed. The **Start with Windows** setting now turns the task on or off (`schtasks /change /enable|/disable`).

### 4.3 Opening things as the normal user
- New `util::launch_as_user(target, args, dir)` asks Explorer to start the program, through `IShellWindows`, then the desktop folder view, then `IShellDispatch2::ShellExecute`. Explorer runs unelevated, so the child does too.
- If Explorer is not running, it falls back to a plain `ShellExecute` and shows a toast warning that the app may start as admin.
- Every place that starts something for the user switches to it:
  - `launch_app`, `open_path`, `open_url` (`steam://` and so on), `open_system`. `taskmgr` stays elevated, which is fine;
  - search results;
  - games' Play button;
  - workspace restore (`winmgr.rs:449`);
  - Game boost's reopen of closed apps (`boost.rs:507-515`);
  - `util::reveal`.
- Test: a debug flag launches `cmd.exe /c whoami /groups > file` through `launch_as_user`. The test checks that the file reports Medium Mandatory Level, then closes it.

### 4.4 Side effects checked
- **Clipboard paste-back:** `SendInput` from an admin process into a normal window is allowed. No change needed.
- **Snap zones and window actions:** WinEvent hooks and moving windows work, and now also work on admin windows.
- **Drag and drop into Deck:** blocked by Windows for admin windows. Deck's UI has no drop targets (checked), so nothing breaks.
- **WebView2:** runs elevated with the same user data folder. The blank-window guard is unchanged.
- **MicDrop link and the phone server:** unchanged. The phone server binds the same port. Windows Firewall may ask again once for the new exe path.

## 5. MoboCtl → Deck

### 5.1 Backend move
- MoboCtl's engine is copied into `src-tauri/src/hw/`. The modules keep their logic, and only the glue changes:

| MoboCtl file | Deck file | Notes |
|---|---|---|
| `pawn.rs` + `mods\` | `hw/pawn.rs` + `hw/mods/` | Embedded PawnIO driver. Loaded only when running as admin. |
| `sio.rs`, `hp.rs` | `hw/sio.rs`, `hw/hp.rs` | Fan chips and HP WMI fans. |
| `sensors.rs`, `sysmon.rs`, `smbios.rs` | `hw/…` | Merged with Deck's `sys.rs` NVML reader so the GPU is opened once. |
| `fx.rs`, `hid.rs`, `devices.rs`, `screen.rs`, `game.rs`, `audio.rs` | `hw/…`, with `audio.rs` renamed `hw/loopback.rs` | Lighting, music and screen-colour capture. |
| `display.rs` | `hw/lcd.rs` | Renamed to avoid a clash with Deck's `display.rs`. |
| `cpupower.rs`, `powermode.rs`, `rapl.rs`, `uv.rs` | `hw/…` | Used by CPU profiles (§6). The MoboCtl-plan code (`plan()`, `keep_active`, PlanLock) is **dropped**. |
| `memclean.rs`, `winutil.rs` | `hw/…` | `winutil::notify` is replaced by Deck toasts. |
| `shared.rs` | `hw/mod.rs` (`Hw` struct + `spawn_all` + `shutdown`) | The settings key/value store is replaced by typed fields in Deck's `Settings` (§5.3). |
| `ui.rs`, `flyout.rs`, `installer.rs`, `startup.rs`, `profiles.rs`, `shortcuts.rs`, `settings.rs` | **not ported** | Replaced by Deck's UI, install, profiles and settings. |

- Dependencies added: `windows-sys 0.61` (with MoboCtl's feature list) next to Deck's `windows 0.62`, and `image` (ico/png), which is also used for shortcut icons.
- The workers start in `App::start()` when running as admin. The loops are unchanged: sensors, RAPL power mode, fans, music level, screen colours, game detection, board RGB, RAM RGB and cooler display.
- **Fans always go back to the BIOS**, in four cases:
  1. On normal quit, `hw::shutdown` joins the fan thread, which calls `release_all`.
  2. On a panic, a panic hook calls `release_all` directly. Deck does not use `panic=abort`.
  3. On Windows shutdown or log-off, the same shutdown runs on Tauri's exit event.
  4. Debug flag `--release-fans`, as in MoboCtl.
- The **heat guard** stays: if the CPU is above 92 °C for 4 s, Deck drops any minimum-clock lock.

### 5.2 Pages (new JS: `ui/js/hw.js`, shared curve editor `ui/js/curve.js`)
- **Fans:**
  - Each fan has a mode (Auto (BIOS), Fixed %, or Curve) and a temperature source (CPU, GPU, hotter, average, or a board sensor). Live % and RPM are shown.
  - Settings for all fans: hysteresis, delay, and "stop below" for system fans.
  - An "allow experimental control" checkbox for chips that aren't verified. Read-only chips show the reason.
  - Curve editor (canvas): drag points, click to add, right-click to remove, with a live marker. Curves have Save as, Delete and Defaults.
  - **Apply** button, as in MoboCtl.
- **Lighting:** mode, brightness, speed, LED counts for ARGB 1 and 2, zones (12V, ARGB 1, ARGB 2, RAM), auto Game preset in games, game sound and music options, a colour wheel with swatches, a Lights on/off button, and controller status lines.
- **Cooler display:**
  - Content (Sensors, Clock, Text, Design, Animation, Music meter, Rotate) and the options for each. Unit, on/off, colour mode, brightness, speed, night dimming and heat flash.
  - A live preview, which is also the paint canvas in Design mode. The LED geometry is sent once from `lcd::geometry()`.
- **Sensors:** MoboCtl's Monitor page.
  - An HWiNFO-style table (current, min, max, average, folding groups) and five-minute graphs.
  - Heat alerts, shown as Deck toasts at most every 5 minutes per sensor, and CSV logging (`%APPDATA%\Deck\monitor.csv`).
- **This PC:** board, CPU, graphics, fans, board RGB, RAM RGB, cooler display and memory rows in green or yellow, plus **Copy report**, **Save report** and **Test lights**.
- **Clean up:** gains a **Clear RAM now** card (memclean).
- **Tray:** adds a **Lights off** check item when RGB is present.
- The UI asks for live data through one `hw_live` command, polled while a Hardware page is open (1 s; 50 ms for the display preview). Settings changes use `hw_set` plus `hw_apply`.

### 5.3 Settings
- New typed sections in `Settings`:
  - `fans` (modes, sources, hysteresis, delay, stop, allow_experimental);
  - `fan_curves`;
  - `lighting`;
  - `lcd`;
  - `alerts`;
  - `log_csv`;
  - `cpu_profiles`, `gpu_profiles` and `active` (§6);
  - `hotkeys_on`;
  - `profile_toast`.
- **One-time MoboCtl import:** on first start, if `C:\Program Files\MoboCtl\settings.ini` or `curves.ini` exists, Deck offers "Import fan curves, lighting and display settings from MoboCtl". MoboCtl profiles are not imported, because CPU profiles replace them.

## 6. CPU and GPU profiles

### 6.1 CPU profile
Each CPU profile is a real Windows power plan that Deck owns (named "Deck · <name>"), plus a few extras Deck applies itself.

| Field | Where it lives | Notes |
|---|---|---|
| Max clock, P-cores / E-cores (MHz, or "no cap") | plan: PROCFREQMAX `…e100` and `…e101` | Both are written, since one alone does nothing on hybrid CPUs. Non-hybrid CPUs show one slider. |
| Max processor state % | plan: PROCTHROTTLEMAX | |
| Min processor state % | plan: PROCTHROTTLEMIN | |
| Turbo (off / on / aggressive / efficient) | plan: PERFBOOSTMODE | |
| Min cores awake % | plan: CPMINCORES | |
| Energy preference (EPP) | plan: PERFEPP | |
| Power limit PL1 / PL2 (W), or the Saver / Balanced / Performance preset | RAPL via `rapl.rs` and `powermode.rs`, applied by Deck | Re-asserted if the firmware resets it, as in MoboCtl. |
| Minimum-clock lock | `cpupower::apply_min` | The heat guard applies. |
| Turbo-ratio overclock | `cpupower::apply_oc` (MSR) | Shown only when the CPU allows it. Lost at restart, so Deck re-applies it. |
| Hold the cap in games | turns off Windows Game Mode (`HKCU\Software\Microsoft\GameBar\AutoGameModeEnabled=0`) while this profile is active, and restores it after | On by default for any profile with a cap. Must be proven by spike S2. |

- **Switching** = `PowerSetActiveScheme`, plus applying the extras. It is instant, and the plan part keeps working if Deck is closed.
- **Watching:** if something else switches plans (Windows Settings, an old shortcut), Deck sees the change within 2 s. If the new plan belongs to a profile, Deck applies that profile's extras and marks it active. Otherwise no Deck CPU profile is shown as active.
- **Editing** a profile writes the plan values and then re-activates the plan (switch away and back), which Windows needs.
- **Importing your plans:** "Use my existing plans" lists every power plan. Picking existing custom plans **adopts** them:
  - the same GUIDs, so the old Start menu shortcuts keep working;
  - their current values are read into the profile, including the hidden PROCFREQMAX read from the registry.
- **New profile** duplicates the active plan (`powercfg /duplicatescheme`) and renames it.
- **Delete** asks whether to delete the Windows plan too. The active plan can't be deleted, so Deck switches to another first.
- **Clock check:** while a fullscreen game is in front, Deck samples the real clock (the `Actual Frequency` counter) every 2 s. The CPU page shows "In game: avg 1,480 MHz (cap 1,500)" for the current or last game session. The user can then check the cap without alt-tabbing.

### 6.2 GPU profile

| Field | NVIDIA interface | Notes |
|---|---|---|
| Power limit % | NVML `nvmlDeviceSetPowerManagementLimit` (official) | Range from the driver, 100 W to 270 W on this card. Shown as % and W. |
| Temperature limit °C | NvAPI thermal policies (undocumented) | Shown only if spike S1 shows it works. |
| Core clock offset MHz | NvAPI `SetPstates20` (undocumented) | Range from the driver. |
| Memory clock offset MHz | NvAPI `SetPstates20` | |
| Voltage boost % | NvAPI volt-rails control (undocumented) | Shown only if S1 shows it works. |
| Voltage/frequency curve | NvAPI ClkVfPoints control (undocumented) | Off by default. When on, it replaces the core offset (Afterburner does the same). |
| Fan | Auto, Fixed %, or Curve vs GPU temperature | NvAPI fan coolers control for RTX 30, or NVML `SetFanSpeed_v2`, whichever works in S1. The curve runs in Deck's loop (1 s, with the same hysteresis as the case fans). Auto goes back to the driver on quit or crash. |

- **Curve editor:** the same canvas component as the fan curves, showing the stock curve with your curve on top. Its tools are:
  - drag one point;
  - shift the whole curve up or down;
  - **Flatten from here** (every point at or above a voltage gets that frequency, the Afterburner "L" trick);
  - **Hold X MHz from Y mV**, a two-number shortcut that builds Profile 5's shape;
  - **Reset**.
- **Live panel** on the GPU page: core and memory clock, power (W and % of limit), temperature, fan % and RPM, voltage, and the performance-limit reason (power, thermal, voltage or none).
- **Applying** writes only the fields that differ from what is set now. After writing, Deck reads the values back and shows any field the driver refused ("Memory offset was limited to +1000"). A profile is only marked active once the read-back matches.
- **Startup GPU profile:** applied when Deck starts (default: none). It replaces Afterburner's [Startup] once the user is ready (§6.7).
- **Driver reset recovery:** a driver reset (event 4101) sets the GPU back to stock. Deck checks every 5 s that the offsets still match. If they don't, it re-applies once and toasts "GPU driver reset: re-applied <profile>". It does not re-apply if a crash guard (§6.6) is active.

### 6.3 Afterburner import
- Reads `C:\Program Files (x86)\MSI Afterburner\Profiles\VEN_10DE&DEV_<this GPU>…cfg`. Reading is fine while Afterburner runs.
- Each `[ProfileN]`, plus `[Startup]`, becomes a GPU profile named "AB Profile N" (renamable). Field mapping:
  - `PowerLimit` % → power limit;
  - `CoreClkBoost` and `MemClkBoost` kHz → MHz offsets;
  - `CoreVoltageBoost` → voltage boost;
  - `FanMode` and `FanSpeed` → fan;
  - `ThermalLimit` → temperature limit;
  - `VFCurve` hex → curve (a 12-byte header, then 127 × {mV, MHz, delta} floats).
- If a profile's curve has any non-zero delta, the curve is turned on for it. Otherwise the plain offset is used.
- Hotkeys come from `MSIAfterburner.cfg` `ProfileNHotkey=0006xxxx` (0x0006 = Ctrl+Alt) and are copied onto the imported profiles. They stay **inactive while Afterburner holds the same keys** (§7.1).
- Unit tests use a copy of this PC's real `.cfg` as the test fixture.

### 6.4 Data model (in `settings.json`)
```text
CpuProfile { id, name, icon, hotkey, desktop, start_menu,
             plan_guid, cap_p_mhz?, cap_e_mhz?, max_state, min_state, turbo, min_cores, epp,
             power?: { preset | pl1_w, pl2_w }, min_lock_mhz?, oc_mhz?, hold_in_games }
GpuProfile { id, name, icon, hotkey, desktop, start_menu,
             power_pct, temp_limit?, core_mhz, mem_mhz, volt_pct?, curve?: [ {mv, delta_mhz} ; 127 ],
             fan: auto | fixed(pct) | curve([ {c, pct} ]) }
active { cpu: id?, gpu: id? }   gpu_startup: id?   gpu_last_applied { id, at, clean_exit }
```

### 6.5 Switching (one code path)
- `profiles::switch(kind, id, source)`:
  1. Apply the profile.
  2. Update `active`.
  3. Emit `changed: profiles`.
  4. Refresh the tray.
  5. Show an optional toast ("CPU → Ultra", source shown only if it failed).
- Every surface calls it: profile cards on the CPU and GPU pages, the tray submenus **CPU profile ▸** and **GPU profile ▸** (check items), keyboard shortcuts, desktop and Start menu shortcuts, the phone (PC card gets two chip rows, endpoint `POST /api/profile`), and Game boost (§6.8).
- Profile cards show the icon, the name, a one-line summary ("1500 MHz · turbo off · 35 W", "PL 100 W · −493 MHz · fan 30%") and a hotkey chip. The active card is in the accent colour. **⋯** opens edit, duplicate, shortcuts and delete.

### 6.6 Crash guard (GPU)
- `gpu_last_applied` records what was applied and whether Deck exited cleanly.
- At startup, if the last session did **not** exit cleanly **and** the PC restarted since, **and** the profile to apply has a positive core or memory offset, a curve above stock, or a voltage boost, then:
  - Deck starts the GPU at **stock**;
  - it toasts "The PC restarted unexpectedly while GPU profile <X> was on. Started at stock. [Apply anyway]".
- Lower-power profiles (negative offsets, power limit at or below 100%) are still applied, since they can't be the cause.

### 6.7 Living with Afterburner
- The GPU page shows a banner whenever `MSIAfterburner.exe` is running: "MSI Afterburner is running. Its startup profile, hotkeys and fan control can override Deck."
- The banner has a button, **Stop Afterburner from tuning…**, which is **opt-in and never automatic**. It:
  1. backs up `Profiles\*.cfg` and `MSIAfterburner.cfg` to `%APPDATA%\Deck\backups\afterburner-<time>`;
  2. closes Afterburner;
  3. clears `[Startup]`, sets the `ProfileNHotkey` values to 0 and sets its fan to auto;
  4. restarts Afterburner (the user keeps it for monitoring).
  It also offers **Undo**, which restores the backup.
- If a Deck hotkey can't be registered because Afterburner holds it, the hotkey chip shows "In use by another program".

### 6.7b Replacing Afterburner completely (added 2026-10-11)
The user decided: **uninstall MSI Afterburner, keep RivaTuner Statistics Server (RTSS)**. RTSS keeps doing what it does
itself (FPS counter, FPS limiter, benchmark, frametime graph). Everything Afterburner did must exist in Deck:

| Afterburner feature | Deck |
|---|---|
| Power/temp limit, core/mem offsets, voltage, VF curve, fan | GPU profiles (§6.2) |
| Profiles 1–5 + Ctrl+Alt hotkeys, startup profile | GPU profiles + hotkeys + startup profile (§6.2, §7) |
| Monitoring graphs | Sensors page (§5.2), with GPU rows from §6.2 live data |
| Logging (HML) | Sensors CSV log |
| **On-screen display through RTSS** (GPU temp/usage/mem/clocks/power/fan/voltage, CPU temp/usage/clock/power, per-core usage, RAM, process RAM, commit, framerate, frametime) | **RTSS overlay feed (§6.10)** |
| OSD toggle hotkey | Deck hotkey "Overlay on/off" |
| Starts RTSS | Deck starts RTSS at its own start if "Start RTSS with Deck" is on (Deck is admin, RTSS needs admin) |
| Third-party readers of Afterburner's monitoring memory (RTSS OverlayEditor "MSI Afterburner" data source, etc.) | Deck publishes the same `MAHMSharedMemory` mapping (v2.0 layout from the RTSS SDK `MAHMSharedMemory.h`) |
| Screen/video capture | Not replaced (Windows Game Bar / NVIDIA app do this) |

### 6.10 RTSS overlay feed
- `rtss.rs` opens `RTSSSharedMemoryV2` (layout: RTSS SDK `RTSSSharedMemory.h`, v2.x). It claims a free OSD slot (owner `"Deck"`; slot 0 is never used so RTSS's own OSD keeps working) and writes `szOSDEx` (4096 chars) with RTSS text tags, using `dwBusy` bit 0 as the write lock and incrementing `dwOSDFrame`.
- Text: one line per group like Afterburner's default layout: `GPU  65°C  98%  1905 MHz  7001 MHz  180 W  45%`, `CPU  60°C  30%  4800 MHz  80 W`, `RAM  12000 MB`, `<FR> FPS  <FT> ms` (RTSS fills the framerate macros itself), plus per-core usage line when enabled. Colours via `<C0=…>` tags, sizes via `<S0=…>`.
- Items are ticked on the Sensors page ("Show in overlay"), defaults imported from Afterburner's `[Source …] ShowInOSD=1` entries.
- Update every 1 s (setting 250–2000 ms). Overlay hotkey toggles the slot text on/off.
- If RTSS isn't running: page shows "RTSS isn't running" + Start button; option "Start RTSS with Deck".
- On quit Deck clears its slot and releases ownership; `MAHMSharedMemory` signature set to 0xDEAD and unmapped.
- CPU package power comes from the RAPL energy counter (MSR 0x611 via PawnIO, as MoboCtl's `rapl.rs` reads limits); hidden if unreadable.

### 6.8 Game boost
- The "Switch to the fastest power plan" option becomes "While Full power is on, use CPU profile [ ▾ ] and GPU profile [ ▾ ]". Both are put back afterwards.
- With no profiles set up (friends' PCs), it keeps today's "fastest plan" behaviour.

## 7. Shortcuts, hotkeys and icons

### 7.1 Keyboard shortcuts
- `hotkeys.rs` runs one message-only window thread with `RegisterHotKey`, and re-registers whenever profiles change.
- Any modifier combination with a key is allowed. A combination needs at least one of Ctrl, Alt or Win, unless it's an F13–F24 key.
- Capture field in the profile editor: click it, press the keys, and it shows "Ctrl + Alt + ]". Esc cancels and Backspace clears.
- Conflicts:
  - inside Deck (two profiles with one key): refused with a message;
  - with another program: the key is saved but the chip says "In use by another program", and Deck retries registering it every 30 s.
- Settings → **Profile keyboard shortcuts on/off** (default on).
- Works while a game is in front. No hooks are used.

### 7.2 Desktop and Start menu shortcuts
- Each profile has a **Desktop** checkbox and a **Start menu** checkbox, and both are created or removed at once.
- Names: "CPU Ultra", "GPU Low power", editable. The desktop folder comes from `FOLDERID_Desktop`, so a OneDrive-redirected desktop works.
- Renaming a profile renames its shortcuts. Deleting a profile removes its shortcuts and tasks.
- **Repair shortcuts** in Settings recreates every missing one.

### 7.3 How a shortcut switches with no UAC prompt and no window
1. Shortcut target: `wscript.exe "C:\Program Files\Deck\run.vbs" "Deck\Profile cpu-<id>"`.
2. The script runs `schtasks /run /tn <task>` hidden. The folder is admin-writable, so the script can't be swapped.
3. Each profile has a task `\Deck\Profile <kind>-<id>` (no trigger, highest privileges) that runs `Deck.exe --switch <kind>:<id>`.
4. That copy is elevated, so the single-instance plugin can pass the arguments to the running Deck. The running Deck switches **without showing its window**. The single-instance handler learns to read `--switch`.
5. If Deck isn't running, the copy starts as Deck (minimized to the tray) and switches.
- Expected delay: about 0.5 to 1 s (wscript plus Task Scheduler). Keyboard shortcuts are instant.
- This is the same mechanism as MoboCtl's `shortcuts.rs`, which gets reused.

### 7.4 Icons
- About 24 built-in `.ico` icons, each with 16, 24, 32, 48 and 256 px sizes, in the existing style (white glyph on a glossy gradient rounded tile):
  - the four current CPU icons: moon, battery-and-leaf, gauge, rocket;
  - GPU: chip in green, blue, amber and red; flame (overclock); snowflake (quiet); leaf (eco);
  - general: fan, bolt, gamepad, film, moon, sun, shield, turtle, rabbit, headphones, code, battery.
  - Glyphs come from Fluent UI System Icons (MIT), as before.
- They are embedded in the exe and written to `C:\Program Files\Deck\icons\` at install, and to `%APPDATA%\Deck\icons\` if that is missing.
- **Icon picker** in the profile editor: a grid of the built-ins plus **Choose file…** (.ico, .png or .exe). A PNG is converted to a multi-size .ico. An .exe uses its first icon.
- Changing the icon updates the existing shortcuts and refreshes the icon cache (`SHChangeNotify`).

## 8. Spikes before building (read-only first, writes only with the user's OK)

| # | Question | How | Pass |
|---|---|---|---|
| S1 | Do the undocumented NvAPI calls work on the test card and driver? | Debug flag `--gpu-probe`. **Read-only:** read offsets, ranges, the curve, power policies, fan coolers and volt rails, and compare with Afterburner's current values. **Writes** (with Afterburner closed and the user's OK): core −100 for 10 s then back; memory −500 then back; power limit 90% then back; fan fixed 40% for 5 s then auto; curve "975 MHz from 625 mV" for 10 s then reset. Each is checked with `nvidia-smi`. | Each write reads back right and goes back to stock. Any call that fails is dropped from the UI. |
| S2 | Does turning off Game Mode make a cap hold in games? | Test: a capped plan plus Game Mode off, in a game. Deck's clock check (§6.1) records the in-game clock, so no alt-tab is needed. | In-game average within about 5% of the cap. If it fails, look at the GameMode PPM profile override in HKLM instead (admin; global for Balanced-based plans). |
| S3 | Elevated Deck plus launching as the user. | Build, install, and run the `whoami` check in §4.3. Launch Steam from search. | Children run at Medium level. Hot corner, clipboard paste and the phone still work. |
| S4 | Does `RegisterHotKey` fire in fullscreen games? | User presses a test hotkey in a game. Deck logs it. | Fires. |

## 9. Build order (each step ends with a check the user can do)

1. **Elevation and install (§4) plus the nav change (§3).** Check: Deck starts at logon with no UAC prompt, apps launch normally, and the pages are in their new groups.
2. **Spikes S1 and S3.** Report results. Reduce §6.2 to whatever works.
3. **MoboCtl port (§5).** Check: fans follow curves and go back to auto on quit, lights and display work, the sensor table is filled, and MoboCtl settings are imported. Then MoboCtl can be quit.
4. **CPU profiles (§6.1)** including the PT plan import and the clock check. Then spike **S2**.
5. **GPU profiles (§6.2 to 6.7)** including the Afterburner import and crash guard.
6. **Switching surfaces (§7):** keyboard shortcuts (then spike **S4**), desktop and Start menu shortcuts, icons, tray, phone and Game boost.
7. **Retire MoboCtl:**
   - run `MoboCtl.exe --uninstall`;
   - delete the "MoboCtl" plan if it is back;
   - remove the old CPU plan shortcuts once the new ones exist (ask first);
   - archive `MoboCtl-Source` (don't delete it).
   - Update `PROJECT_STATE.md`, `GPU-CPU-POWER-PROFILES.md` and memory.

## 10. Testing

- **Unit tests** (`cargo test`):
  - Afterburner `.cfg` parsing and VFCurve decode/encode round trip on the real fixture;
  - the hotkey string ⇄ (mods, vk) conversion;
  - profile serde, including defaults for missing fields;
  - plan value read from a captured registry export;
  - shortcut `.lnk` creation into a temp folder (created, read back, deleted);
  - icon PNG → ICO conversion;
  - `launch_as_user` level check (ignored by default).
- **Read-only self-tests** (debug flags, test profile):
  - `--hw-selftest` reports found hardware, sensors, fan chip and control level, RGB, display and GPU ranges;
  - `--gpu-probe` (S1, read-only part);
  - `--cpu-probe` lists plans with their caps and turbo and the active profile.
- **UI previews:** offscreen renders of each new page (the existing `--dump-state` mock plus headless Edge) before every install, because the user is picky about looks. MoboCtl's egui screenshots are the reference for content, not for style; the style is Deck's.
- **Live writes** (fans, GPU, plans) only after asking the user, and never while a game is running.

## 11. Risks

| Risk | Mitigation |
|---|---|
| Undocumented NvAPI calls change with a driver update | Every call is checked by reading the value back. A failed field is hidden and a toast says why. NVML covers the power limit even if NvAPI breaks. |
| A GPU overclock plus this PC's existing crashes | Crash guard (§6.6). Positive offsets are never auto-applied after an unexpected restart. |
| Fans stuck on manual | Release on quit, panic and shutdown, plus `--release-fans`. A BIOS-level fallback is not possible from software. |
| Admin Deck is a bigger target | Program Files location, admin-writable runner script, and tasks only run Deck with fixed `--switch` arguments. The phone API is unchanged (token) and gains only profile switching. |
| Afterburner overrides Deck | Banner, hotkey conflict display, and the opt-in "Stop Afterburner from tuning" with Undo. |
| Scope: this is a lot of code | The build order delivers something usable at each step, and steps 3 to 6 can each be shipped and installed alone. |

## 12. Open points (defaults chosen; say if you want otherwise)

- **Fans and lighting are not part of profiles.** CPU and GPU profiles only, as chosen. The GPU fan is in GPU profiles.
- **Profile toast:** on by default. Off in Settings.
- **Shortcut names:** "CPU <name>" and "GPU <name>".
- **Starting state:** no default hotkeys for new profiles. Imported Afterburner profiles bring their Ctrl+Alt keys.
- **No git repository** in `deck\`, so this spec is not committed. Say if you want `git init`.
