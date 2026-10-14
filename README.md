<div align="center">

<img src="src-tauri/icons/128x128.png" width="96" alt="Deck icon">

# Deck

**One tray app for your Windows gaming PC.** A launcher, a live dashboard, PowerToys-style tools, full CPU / GPU / fan control, an in-game overlay and a phone remote. It replaces MSI Afterburner and your motherboard's fan software.

[![Release](https://img.shields.io/github/v/release/MohammadFakih02/deck?label=download&color=4f8cff)](https://github.com/MohammadFakih02/deck/releases/latest)
[![CI](https://github.com/MohammadFakih02/deck/actions/workflows/ci.yml/badge.svg)](https://github.com/MohammadFakih02/deck/actions/workflows/ci.yml)
![Windows 10/11](https://img.shields.io/badge/Windows-10%20%7C%2011-0078d4)
[![License: MIT](https://img.shields.io/badge/license-MIT-green)](LICENSE)

<img src="docs/screenshots/home.png" width="860" alt="Deck's Home page: clock, now playing, PC vitals, performance profiles, sound mixer and pinned apps">

</div>

## Download

Get **`Deck_2.0.0_x64-setup.exe`** from the [latest release](https://github.com/MohammadFakih02/deck/releases/latest) and run it. Deck lives in the tray; open it by resting the mouse in the top-left corner of the screen, or by clicking the tray icon.

> The setup isn't code-signed, so Windows SmartScreen may say "Windows protected your PC". Click **More info → Run anyway**.

Hardware control (fans, lighting, CPU power limits, GPU tuning) needs administrator rights. Open any hardware page and click **Set up**: one admin prompt, and from then on Deck starts with those rights through scheduled tasks, so it never asks again.

**Needs:** Windows 10 or 11 (64-bit). Optional: [RivaTuner Statistics Server](https://www.guru3d.com/download/rtss-rivatuner-statistics-server-download/) for the in-game overlay, an NVIDIA card for GPU tuning.

## What's inside

| | |
|---|---|
| **Home** | Clock and calendar, now playing, PC cards (usage, clock, power and temperature for CPU, GPU, memory and VRAM), one-click CPU/GPU profiles, sound mixer, pinned and frequent apps, quick actions, clipboard, timers and network. Customize it: drag tiles anywhere, resize them wider and taller, hide and bring them back. |
| **Your look** | 8 themes (including OLED and pure black), accent colours, opacity (separate for pinned), opening animations, and a low-spec mode for older PCs. The pin turns Deck into a floating window over your other apps. |
| **Search** | Start-menu and Store apps, files, Deck actions, a calculator, unit / currency / number-base conversion, games, clipboard and the web. Understands `timer 10m`, `sleep in 30m`, `awake 2h`, `kill discord`. |
| **Game boost** | Closes the apps you pick, switches to the fastest power plan, slows background apps and frees memory, then puts everything back when the game closes. Games, launchers, anti-cheat and voice chat are never touched. |
| **CPU** | Profiles built on real Windows power plans plus extras: P-core / E-core clock caps, PL1/PL2 power limits, a minimum-clock lock with a heat guard, turbo-ratio overclock on unlocked Intel chips. |
| **GPU** (NVIDIA) | Afterburner-style profiles: power and temperature limits, clock offsets or a full voltage/frequency curve editor, voltage, fan curve. Imports your Afterburner profiles and hotkeys. A crash guard starts at stock after an unexpected restart. |
| **Fans, Lighting, Cooler display** | Super I/O fan control (Nuvoton, ITE) with curves, fixed speeds and presets; MSI Mystic Light and Kingston Fury RGB; USB LCD coolers. Fans always go back to the BIOS when Deck quits. |
| **Drives** | The health of every HDD and SSD (S.M.A.R.T.): temperature, hours, data written, wear and warning signs, with history and optional alerts. |
| **Sensors** | 5-minute graphs for temperature, load, power, fans and frametimes (average, 1% and 0.1% lows); every reading with min / max / average; heat alerts; CSV logging. |
| **In-game overlay** | Through RivaTuner (Deck can install it for you): choose and group any reading, Afterburner-style looks, colours, a smooth frametime graph that fits the overlay, and keys to show it and to reset the figures. |
| **Everyday tools** | Clipboard history (skips password managers), per-app volume, timers and break reminders, keep awake, "shut down when…" (a time, an app closing, downloads finishing, idle, media stopping). |
| **Screen and PC** | Window pinning / see-through / FancyZones-style snap zones / workspaces, display refresh rate and brightness, processes, startup apps, network (speed test, ping, game-server ping), a WizTree-fast disk space treemap (reads the NTFS file table directly), clean up, colour picker, text from screen, locked files. |
| **Phone remote** | A web app on your home network (scan a QR code): media, volume, profiles, shared files. |

Every hardware setting has a **?** that explains what it does, why you'd change it and what to watch out for.

<table>
<tr>
<td><img src="docs/screenshots/sensors.png" alt="Sensors graphs: temperature, load, power and frametimes with 1% lows"></td>
<td><img src="docs/screenshots/overlay.png" alt="In-game overlay settings with a live preview"></td>
</tr>
<tr>
<td><img src="docs/screenshots/gpu.png" alt="GPU page with profiles and live clocks"></td>
<td><img src="docs/screenshots/fans.png" alt="Fans page with a draggable fan curve"></td>
</tr>
<tr>
<td><img src="docs/screenshots/boost.png" alt="Game boost page"></td>
<td><img src="docs/screenshots/network.png" alt="Network page with speed test and game-server ping"></td>
</tr>
</table>

## Safe with anti-cheat

Deck uses no input hooks and never injects into games. Keys use `RegisterHotKey`, the hot corner polls the cursor, and window snapping uses WinEvent hooks. The overlay is drawn by RivaTuner, which anti-cheat systems already allow.

## Hardware support

- **Fans:** Nuvoton NCT668x / NCT679x and ITE Super I/O chips (most MSI, ASUS, Gigabyte and ASRock boards), HP laptop fans. Chips that haven't been verified are monitor-only.
- **GPU tuning:** NVIDIA (NVML + NvAPI). AMD and Intel GPUs show sensors only.
- **CPU tuning:** any CPU for power-plan profiles; power limits and the clock lock on Intel; turbo ratios on unlocked Intel CPUs.
- Pages hide themselves when the PC has nothing for them.

> **Use hardware tuning at your own risk.** Overclocking, undervolting and changing fan behaviour can make a PC unstable or run hot. Deck has guards (crash guard, heat guard, fans released to the BIOS on quit) but can't protect against every setting.

## Building from source

You need [Rust](https://rustup.rs) (stable, MSVC), the Visual Studio C++ Build Tools, and the Tauri CLI:

```powershell
cargo install tauri-cli --version "^2" --locked
rustup target add i686-pc-windows-msvc   # the 32-bit RivaTuner helper build.rs embeds
cd src-tauri
cargo test --release           # unit tests
cargo tauri build --bundles nsis   # makes the setup in target\release\bundle\nsis
```

For day-to-day work, `install.ps1` builds Deck and installs it to `C:\Program Files\Deck` with the admin tasks; `uninstall.ps1` removes it (add `-Purge` to delete settings too).

The UI is plain HTML, CSS and JavaScript in [`ui/`](ui) with no build step; it is embedded in the exe, so UI changes need a rebuild. [`tools/testing/ui-preview`](tools/testing/ui-preview) renders any page in headless Edge against a fake backend (`node shot.mjs sensors - out.png`) and runs the UI regression checks (`node regress.mjs`), with no hardware access.

| Path | What |
|---|---|
| `src-tauri/src/` | Rust backend: Tauri commands, tray, settings, every feature module |
| `src-tauri/src/hw/` | Fans, lighting, sensors, cooler display (Super I/O through the PawnIO driver) |
| `src-tauri/src/gpu/`, `profiles/` | GPU engine (NVML, NvAPI, curves, crash guard), CPU/GPU profiles, Afterburner import, hotkeys |
| `src-tauri/src/rtss.rs`, `osd.rs` | RivaTuner overlay slot, frame statistics, overlay layout and graph |
| `ui/` | The app's pages (`js/`), styles and the toast / snap-zone windows |
| `src-tauri/phone/` | The phone remote web app |

Settings live in `%APPDATA%\Deck`.

**Contributing:** read [CONTRIBUTING.md](CONTRIBUTING.md). Every commit that changes code must update `PROJECT_STATE.md` and `CHANGELOG.md` too; run `git config core.hooksPath .githooks` once after cloning to turn on the check.

## Credits

- Built with [Tauri](https://tauri.app).
- Fan and sensor access through the signed [PawnIO](https://github.com/namazso/PawnIO) driver and [PawnIO.Modules](https://github.com/namazso/PawnIO.Modules).
- Profile icons from [Fluent UI System Icons](https://github.com/microsoft/fluentui-system-icons).
- In-game overlay drawn by [RivaTuner Statistics Server](https://www.guru3d.com/download/rtss-rivatuner-statistics-server-download/).

See [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) for licences. Deck is not affiliated with MSI, NVIDIA or Guru3D.

## License

[MIT](LICENSE) © 2026 Mohammad Fakih
