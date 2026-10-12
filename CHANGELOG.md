# Changelog

All notable changes to Deck. Setups for each version are on the [Releases](https://github.com/MohammadFakih02/deck/releases) page.

## 1.0.0 — 2026-10-13

The first public release.

### New
- **Power graph** on Sensors → Graphs: CPU package and GPU board power over the last 5 minutes.
- **Frametimes panel**: the last 600 frames, with Now, Average, 1% low, 0.1% low, Lowest and Highest.
- **Customize** on Home and Sensors → Graphs: drag panels to move them, drag the corner to resize, hide panels and bring them back, or put everything back. Drag the sidebar's edge to change its width.
- **Overlay item picking and grouping**: tick several items (Shift-click picks a range), move them together, put them on one line under your own label, or remove them at once.
- **In-game frametime graph** drawn by Deck: 120 points over 5 s to 1 minute (default 10 s), smoothed or showing every hitch, and as wide as the overlay text (or a width you set).
- **Reset key** for the framerate figures, plus Reset buttons on the Frametimes panel and the overlay settings. Handy at the start of a match or a benchmark run.
- **"?" tips** on Fans, Lighting, Cooler display, Sensors, the overlay settings, Game boost and Settings (about 90 in total).

### Fixed
- Average FPS, 1% / 0.1% lows, lowest and highest never worked. They are now read straight from RivaTuner's per-game frametime history and follow the game you are playing.
- A pause (minimising, a loading screen) no longer drags the figures down.
- The overlay now shows within a few milliseconds of switching it on, instead of 1–3 seconds.
- Missing values showed as "?" in the overlay; they now show "-".
- Game RAM sits on the RAM line in the overlay.

## 0.2.0 — 2026-10-12

### New
- **Hardware control**, replacing MSI Afterburner and the motherboard fan tool:
  - CPU profiles (power plans plus clock caps, power limits, a minimum-clock lock with heat guard, turbo-ratio overclock).
  - NVIDIA GPU profiles (power and temperature limits, clock offsets or a voltage/frequency curve, voltage, fan), with Afterburner import and a crash guard.
  - Fans (curves, fixed speeds, presets, always released to the BIOS on quit), Mystic Light and Kingston Fury RGB, USB LCD coolers, sensors with graphs, alerts and CSV logging.
  - Switching from profile cards, the tray, hotkeys, desktop shortcuts, the phone and Game boost.
- **In-game overlay** through RivaTuner: 47 readings, layouts, colours, quick looks including "Like MSI Afterburner", and a live preview.
- **Keep awake** and **Shut down when…** redesigned, with live readings and a 60-second cancel warning. Plans survive a restart.
- **Network** page on one screen: live rates, speed test, ping with Start / Stop, game-server ping.
- **Windows** and **Tools** pages with tabs (open windows, snap zones, workspaces; screen tools, locked files, Windows panels).
- "?" tips on every CPU and GPU setting; a real font list for the overlay.

### Fixed
- Clicks lost when a page refreshed under the mouse; keyboard focus kept across refreshes; accessible names and focus rings everywhere.
- Search: Enter ran the previous result; typo tolerance; kill / close now ask first.
- Calculator: `200 + 10%` = 220.
- Clean up never pre-ticks the Recycle Bin; Disk space locks Windows' own folders.
- Processes that Windows marks critical can never be ended.
- Only a playback device can be made default, never a microphone.

### Removed
- The Apps, Games and Notes pages (apps and games are still in search and on Home). Saved notes are kept in the settings file.

## 0.1.1 — 2026-10-07

### Fixed
- Protection against blank white or black windows from WebView2: browser flags on every window, a repaint nudge after showing, and a watchdog that recovers a window that stops painting.

## 0.1.0 — 2026-09-28

- First version: tray app with hot corner, launcher and search, Home dashboard, clipboard history, sound mixer, timers, keep awake, shut down when…, windows and snap zones, display, processes, startup apps, network, disk space, clean up, screen tools, phone remote, and **Game boost**.
