# Changelog

All notable changes to Deck. Setups for each version are on the [Releases](https://github.com/MohammadFakih02/deck/releases) page.

## 2.0.0 — 2026-10-14

The big update. It installs over Deck 1.x and keeps all your settings, profiles and layouts.

### New
- **Make Deck yours**: on first start (and once after this update) Deck asks for your theme, accent colour, opacity, opening animation and low-spec mode. All of it can be changed later in Settings → General.
- **8 themes** (Settings → General → Colour): Midnight (the default), Graphite, OLED black, Pure black, Nord, Forest, Plum and Ember. Your accent colour stays the same.
- **Opacity**: how see-through Deck is, set separately for pinned and not pinned (30–100 %).
- **Opening animations**: Zoom (the default), Fade, Pop, Rise, Drop, Flip, Swirl, Blur in, or None, with a Preview button.
- **Low-spec mode** (Settings → General → Performance, off by default): for older or slower PCs. No animations or soft shadows, and live readings and graphs update every 3 seconds instead of every second. Everything still works.
- **The pin works in both modes.**
  - Quick panel: Deck becomes a small floating window on top of everything, with nothing dimmed around it, so you can use other apps while it stays open.
  - App window: Deck stays on top of other windows.
- **Home, arranged your way**:
  - Drag any tile anywhere, including under each other.
  - Resize tiles both wider and taller from the corner.
  - A guide line shows where a tile will snap to line up with its neighbours.
  - Tiles never overlap: the ones in the way move down.
  - When a tile is made small, its less important parts fold away neatly instead of overflowing.
- **New default Home layout** for new installs and for anyone who never arranged Home.
- **PC tile cards** (Home): CPU, Memory, GPU and VRAM cards, all in the same style: usage, then clock and power (for example "5.3 GHz · 33 W"), then temperature, and a graph. Their rows line up across cards. The new button in the tile's corner turns cards on or off, reorders them, adds a Network card, and hides the drives list.
- **Drives** (Monitor → Drives): the health of every HDD and SSD (S.M.A.R.T.).
  - Shows health, temperature, power-on hours, data written, wear and the warning signs (reallocated, pending and uncorrectable sectors, media errors).
  - It remembers past readings, so a problem that came and went still shows.
  - Optional notifications when a drive's health gets worse (off by default).
- **Disk space, much faster**:
  - On NTFS drives Deck reads the drive's file table directly (the way WizTree does), so a full drive scans in seconds.
  - The treemap is laid out like WizTree's, with **File types** and **Folders** views and a back button.
  - The drive picker switches drives instantly.
- **Overlay has its own page** (Monitor → Overlay).
  - It can install RivaTuner for you.
  - The overlay key now also flips RivaTuner's own "Show on-screen display" switch, so the overlay turns on and off reliably.
- **Sidebar categories can be reordered**: drag a group or a page to move it. Settings → General has a Reset to put the sidebar back. Every group starts folded on a new install, for a short menu.
- **Support report** (This PC) now includes:
  - drive health and past drive problems;
  - the PC's maker, model and type (laptop, desktop…);
  - a quick lights test.

### Changed
- **Power & time** is now the last sidebar group, after Maintenance.
- **Sensors** opens on All readings.
- **Game boost**:
  - "Free memory" now only works when memory is actually short (85 % or more in use). With room to spare it does nothing, since trimming would only make background apps slow to come back.
  - "Turn off by itself" also ends Full power after 3 hours if no game ever showed up.
  - Deck now also spots games it doesn't know, when the program in front keeps the graphics card busy.
  - Picking a CPU profile under "Switch profiles while it's on" turns off "Fastest power plan", because the CPU profile brings its own power plan. The page now says so, instead of the switch just going off.
- **Overlay settings fit narrow windows**: button groups, sliders and the font list move under their label instead of spilling past the card.
- The default sidebar is narrower (180 px).

### Fixed
- **Network speeds were wrong (often double).** Windows lists some adapters twice: packet-capture tools such as Npcap and Wireshark add a filter layer that repeats the same traffic. Deck now counts only real network adapters, each once, using Windows' own counters. With Ethernet and Wi-Fi both connected their traffic adds up. An adapter you plug in starts counting from its next reading, with no false spike. One you remove simply stops counting.
- **RAM lighting ignored Deck after a cold boot**, for example staying lit when lights were set to off. Right after Windows starts, the memory's RGB controllers can be too slow to answer, and Deck never looked for them again. Deck now keeps looking for 10 minutes after startup or wake, and never disturbs sticks it already found.
- **Resizing a Home tile sometimes jumped back** and had to be done twice. A settings refresh in the middle of a drag overwrote the new size.
- **A resized tile went back to full size** after you let go.
- Tiles flickered while dragging.
- Numbers overlapped labels in small tiles.
- Long app names, like "Microsoft Edge WebView2", pushed the volume number out of the Sound tile.
- **The pin did nothing**: its button crashed when clicked.
- The overlay key (Page Up) didn't toggle RivaTuner's on-screen display.
- Deck no longer forces an extra full-window repaint every time it opens, unless the window actually comes up blank.

## 1.1.2 — 2026-10-14

### Changed
- **Sidebar regrouped** so each group has one job: **Hardware** (CPU, GPU, Fans, Lighting, Cooler display), **Monitor** (Sensors, Processes, Network, This PC), **Desktop** (Windows, Display, Clipboard, Sound), **Power & time** (Keep awake, Timers & breaks, Shut down when…) and **Maintenance** (Disk space, Clean up, Startup apps, Tools). The pages themselves are unchanged. Folded groups open again once, since the group names changed.

## 1.1.1 — 2026-10-13

### Fixed
- **RAM and motherboard lighting stopped following Deck after sleep.** RAM RGB sticks lose power in sleep and go back to their own built-in effect, and Deck never took them over again. Deck now re-detects the RAM and reconnects the board lighting a few seconds after the PC wakes from sleep or hibernate.

## 1.1.0 — 2026-10-13

### New
- **Units** (Settings → General → Units): clock speeds in MHz or GHz, network speed in Mb/s (bits) or MB/s (bytes), temperatures in °C or °F. They apply to every page, the sensor table, the phone and the in-game overlay. Numbers you type in yourself (GPU offsets, curve editors, fan curves, temperature limits) stay in MHz and °C.
- **Comet colours** (Lighting → Comet): your own colour, **Cycle** (the comet runs through the colours) or **Rainbow** (a rainbow tail that turns as it flies).

### Faster
- **Hidden windows no longer draw frames.** The page heartbeat used a frame loop that kept the hidden window rendering at the monitor's refresh rate (up to 300 times a second) the whole time Deck sat in the tray. It now checks one frame every 4 seconds. Idle CPU use drops from about 15–20 % of one core to a few percent.
- **Screen and Game lighting** read the screen with DXGI desktop duplication: about 0.3 ms per sample instead of about 23 ms with GDI (20 samples a second while a game runs), and nothing at all while the screen doesn't change. GDI stays as the fallback.

### Fixed
- A long profile name next to CPU or GPU in the sidebar squeezed the page name to "G…" and wrapped onto two lines. The name now shrinks with "…" (hover shows it in full).
- The in-game overlay showed network speed in MB/s while the app showed Mb/s; both now follow the Units setting.

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
