# Changelog

All notable changes to Deck. Setups for each version are on the [Releases](https://github.com/MohammadFakih02/deck/releases) page.

## 2.5.0 — 2026-10-15

### New
- **PC to PC** (bottom of the sidebar, above Phone; on by default): send files, folders, installers, apps and your Windows setup between your PCs, with Deck on both and the same pairing code. They find each other on the home network by themselves (or by address), and everything is encrypted with the pairing code.
  - **Send**: files and installers (optionally **installed on arrival**: silently when the installer allows it; the other PC asks first unless it trusts this one), whole folders (shows what goes first; skips build output such as `target` or `node_modules`; can send only new or changed files, and delete on the other PC what's gone here, after its user agrees), or a piece of text.
  - **Browse** the other PC's shared folders and copy from them; nothing is shared until you add a folder, and each PC is asked, always allowed or never allowed.
  - **Apps**: pick apps installed here; the other PC installs them itself with winget, skipping the ones it has (retries network errors; "Retry failed").
  - **Setup**: sends this PC's look (dark mode, accent, taskbar and Explorer options), wallpaper and Windhawk mods; the other PC picks what to apply, backs up first and can undo.
  - **Received**: install, install silently, extract, apply a setup, show or delete the files. Offers and requests waiting for an answer show as notifications with Accept / Decline.
  - Transfers resume after a dropped connection; progress, speed and time left show live.
  - **Everything asks first** (Accept / Decline, also as a notification), unless you trust the sending PC: **for a week, a month, 6 months or always**, from the prompt or Settings → Trusted PCs. Deleting files on your PC is always asked.
  - **Away from home**: your other PCs on Tailscale (the same private network the phone remote uses) are found by themselves; no cloud folder needed.
  - **Command line** for scripts and Claude Code: `deck.exe --pc-list`, `--pc-send <path> [--to PC] [--all] [--sync] [--install]`, `--pc-get <path> --from PC`; prints progress and a clear result (exit code 0 = done). **Let Claude Code use PC to PC** (Settings) installs a short Claude Code skill so Claude can send files or a whole project, dependencies included, when asked.
  - A single file arrives in a folder named without its extension.
  - **Phone transfers continue after a dropped connection**: the phone sends in 8 MB pieces the PC keeps, retries by itself and goes on from what the PC already has (picking the same file again later continues too), with speed and time left. Downloads to the phone and share-link downloads can be continued by the browser (byte ranges). The command line shows time left.
  - **Share link**: a file or folder for someone who isn't one of your PCs. They open a link in any browser (no Deck needed) and download; the link works once or until it ends (1 hour, 1 day or 7 days), on this Wi-Fi, or over the internet through Tailscale Funnel (only the shared file is published, and only while a link is open). A folder is zipped first; a download that breaks off doesn't use the link up; you get a notification when it's downloaded, and can stop a link any time.
- **Claude Code sync** (under Desktop, hidden until shown in Settings → Sidebar or switched on; off by default): for Claude Code on more than one PC. Keeps its conversations, memory and settings the same on your PCs (directly on the network, or through a cloud-synced folder), rewriting project paths so /resume works on each PC.
  - Encrypted with a sync key only your PCs know; sign-in details and per-PC files are never synced.
  - **Session lock**: a conversation open on one PC can't be continued on another by mistake (Claude Code is stopped with an explanation; **Take over** overrides it). **Tell Claude** which PC it runs on.
  - **Sessions** (search, where each one lives, delete here or on all PCs), **Projects** (each project's folder on this PC; move, stop syncing, delete everywhere), **Status** and **Log**.
  - It adds two small hooks to Claude Code's settings while on, and takes them out when switched off or when Deck is uninstalled. Settings from the standalone ClaudeSync app are taken over the first time; it won't start while that app runs.
- **Screen on your phone** (Phone page, off by default): see the PC's screen live in the phone remote, at home or away (Tailscale), and with **View and control** use it too.
  - **Control starts off** on the phone every time, so watching can't click anything by accident; the Control button turns it on.
  - Tap to click, hold for right-click, drag to drag, two fingers to scroll; pinch to zoom in on small text.
  - **Full screen**: hides the bars until you leave it, with small buttons on the edge for control, the keyboard and leaving.
  - **Sound** (off until you turn it on on the phone): the PC's sound plays on the phone, as one continuous stream (no clicks between pieces; the PC's and phone's clocks are evened out without skipping), with a small delay that adapts to the connection and to how the phone plays sound (from 120 ms; after a dropout it grows by as long as the sound was missing, up to 1 s while just watching, since the picture waits for the sound, and 0.3 s while controlling the PC; it shrinks again after half a minute without one; sound that falls behind catches up by leaving out silent moments; dropouts are counted in the status line). The PC also stops sending pictures while earlier ones are still queued on the line, so a bitrate above what the connection carries can't hold up the sound or your taps. **Best** (uncompressed, ~1.5 Mb/s), **Good** (~400 kb/s), **Medium** (24 kHz stereo, ~200 kb/s) or **Low** (16 kHz mono, ~70 kb/s); **Auto** picks one from the connection.
  - **Lip sync**: while only watching, each picture waits for the moment its sound plays on the phone. With Control on the picture stays instant instead.
  - **Bitrate**: **Auto** (measures the connection and follows it, from a slow DSL line up to full quality at home), a fixed limit from 30 Mb/s down to 0.5 Mb/s, or one typed in. When it's tight: **Smooth** keeps the chosen frame rate, even on a 1 Mb/s line (the picture gets smaller and blurrier: 30 fps at 1 Mb/s is about 320 px); **Sharp** keeps text readable: the screen's own resolution at good quality, with fewer pictures (about 5 a second at 8 Mb/s, 10 from 15 Mb/s, 2 on a slow line).
  - **H.264 video** (Video: H.264, the default; JPEG stays as a fallback): the same 30 fps picture takes a fraction of the data, so a 1 Mb/s DSL line shows 30 fps at about 640 px (60 fps at 480 px, text still readable at Smooth). Uses the graphics card's encoder (NVIDIA, AMD, Intel) and the Windows software one if there is none; if an encoder fails Deck moves to the next, then to JPEG, and tells the phone. The phone decodes with WebCodecs when its browser allows it (secure pages), and otherwise with its own decoder in the page (WebAssembly: any browser, plain home address included), drawing each picture as it arrives like JPEG, so nothing stalls, jumps or replays old pictures; a phone that decodes too slowly is sent narrower pictures. Browsers without WebAssembly use the browser's video player. Key pictures (several times a normal one) come only when needed. Smooth keeps the frame rate and shrinks the picture; Sharp keeps the picture and lowers the frame rate (not under 8).
  - **15, 24, 30 or 60 fps**. Pictures only go out when the screen changes, so 60 shows in games, scrolling and 60 fps video.
  - At home it uses the home network even when the phone opened the Tailscale code.
  - Type with the phone's keyboard, plus a bar of keys and shortcuts (Esc, Win, arrows, Copy, Paste, Alt+Tab, Task Manager…).
  - Every monitor can be picked; picture size **Full** (the screen's own resolution, the default), **Medium** or **Small**. Full keeps colour detail at high quality, so small text stays readable.
  - Pictures are captured on one thread and compressed on two others, so the frame rate follows the screen (the status line shows both: "49 fps (screen 59)").
  - Light on the PC and the connection: nothing is sent while the screen doesn't change, and a picture only goes out once the phone has shown the last one.
  - Control pauses while a game is in front (watching still works): Deck never sends input into games, because anti-cheat systems watch for that.
  - The PC shows a notice while a phone is watching, and the Phone page shows how many are. Windows hides the lock screen and admin prompts from every app, so the phone says so instead.
- **See and control this PC** (Phone page): remote desktop through RustDesk (free, open source), from your phone or another computer.
  - Deck installs RustDesk for you. The download is checked against GitHub's fingerprint and RustDesk's signature.
  - It shows the addresses to type in: **At home** (your home network) and **Away from home** (Tailscale), plus the RustDesk ID.
  - Switches for **Allow remote desktop** (off stops RustDesk completely, also after a restart), **Connect by address** (straight to this PC, not through RustDesk's servers) and **Hide tray icons** (RustDesk and Tailscale keep working; their state shows in Deck instead).
  - Set the password from Deck, with short steps for the phone.
- **The phone remote works away from home** through Tailscale, a free private network between your own devices; nothing is opened to the internet.
  - The Phone page's code has **At home** and **Away (Tailscale)** tabs.
  - Until a phone is on your Tailscale, the Away tab shows how to put it there. Its code opens a "Get Tailscale" page: the App Store on iPhones; on Android the Play Store and a direct download from Tailscale's own site, for countries where the Play Store doesn't offer it. It also shows the account to sign in with.
  - Once the phone is on it, its name shows under the code, connected or offline.
- **Hide RivaTuner's tray icon** (Overlay → Behaviour): RivaTuner keeps running with no icon in the taskbar corner, not even under the arrow, and Deck keeps it gone when RivaTuner or Explorer restarts. Turn the switch off and the icon comes back straight away. **Open RivaTuner** next to it opens RivaTuner's own settings. It restarts RivaTuner to do that, so a game's overlay blinks for a moment.

### Fixed
- **Framerate limit key** (Overlay → Behaviour): turns RivaTuner's framerate limiter on and off in games, like Afterburner's limiter key did, with the limit right next to it and a switch. Afterburner's limiter key is taken over by the Afterburner import.
- **Overlay closer to Afterburner**: the framerate and frametime (filled in by RivaTuner) line up in the same column as the other numbers; RivaTuner's own statistics line ("0:27:42 (5 cur, …)") is switched off, as Deck shows those figures itself; the "Like MSI Afterburner" look has a slim graph.
- **Overlay: numbers no longer touch their units** (°C, %, MHz): small units get a small gap of their own size, in the game and the preview.
- **Overlay preview colours now match the game**: a number's prefix and its unit are in the label colour (they took the part's colour in the preview), small units are drawn at RivaTuner's size and raised like in the game, the frametime graph takes the label colour (or the framerate colour with part colours), and a colour that isn't valid falls back to the default as it does in the game.
- Phone remote: the skip buttons next to pause jump 10 s back and forward when the player has no previous/next track (a video in a browser), and quick taps add up; the home page shows CPU and GPU power in watts, and the CPU's package temperature instead of its usage.
- Phone Screen view: the bar keeps the actions (close, full screen, control, sound, keyboard, keys); bitrate, frame rate, slow-connection choice, picture size, video and sound quality are in one Settings panel, each with a line saying what it does.
- Phone Screen view, Auto bitrate: measures the connection first (a short burst the phone answers piece by piece; under a second on a 1 Mb/s line) and uses 80 % of it, leaving 20 % free; on a slow line (under 3 Mb/s) it never climbs past that, so it no longer overshoots and lags while finding the limit. Measured again every 5 minutes; the status line shows the measured line. A set bitrate is the most H.264 uses; it still backs off when the line is busy. The phone's sound takes its share out of the picture's.
- Phone remote away from home was slow (buttons lagged, sent text didn't arrive): every update carried the playing track's artwork, ~170 KB every 1.5 s, more than a 1 Mb/s home upload. The phone now fetches the artwork once a track; updates are ~2 KB.
- Phone sound no longer counts a dropout at every pause in the PC's sound (the first 1.5 s of silence is now sent as silence), and its smallest delay is 80 ms.
- **The phone couldn't connect: scanning the code loaded forever.** Windows Firewall blocked it. When Deck starts with admin rights at logon, Windows never shows its "Allow access" question, so nothing allowed the phone in. Deck now adds its own firewall rule (home and work networks only, not public ones).
- **File pickers opened behind Deck** ("Send a file", Browse buttons) while Deck was on top. They now open in front of it.
- **Deck opened while taking a screenshot** with Win+Shift+S, Snipping Tool or tools like ShareX, Lightshot and Greenshot, when the box started or ended in the hot corner. The corner now ignores screenshot tools, and after a click or drag in the corner it waits until the pointer leaves.

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
