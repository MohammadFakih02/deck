# Changelog

All notable changes to Deck. Setups for each version are on the [Releases](https://github.com/MohammadFakih02/deck/releases) page.

## 2.7.9 — 2026-10-21

### Added
- **Apps** (Maintenance → Apps): everything installed on the PC, with search, sorting by name, size or date, and Desktop / Store filters.
  - **Uninstall** runs the app's own uninstaller and waits until it has really finished (also uninstallers that copy themselves to Temp first).
  - **Advanced** uninstall: the uninstaller first, then a search for what the app left behind: its folders (Program Files, ProgramData, AppData, Documents), its registry settings, Start menu and desktop shortcuts, and Windows services that run from its folder. You tick what goes; folders go to the Recycle Bin, and registry keys and services are saved to a .reg backup first. Only names that match the app exactly are offered, and nothing another installed app uses.
  - Apps whose uninstaller is missing can still have their entry and leftovers removed.
  - **Apps missing from Windows' list** (known to Windows Installer, but their entry was removed by a cleanup tool or a half-finished uninstall) are listed too and uninstall normally; apps whose folder is gone are marked **Files gone**. A **Needs attention** filter shows just these.
  - Built-in apps and Store apps moved here from Tweaks (tabs **Built-in apps** and **Store apps & updates**).
- **Store app updates** (Apps → Store apps & updates): checks every Store app against Microsoft's servers and lists the ones with a newer version and its size. **Update** downloads and installs it without the Store; **Update all here** goes one app at a time; **Update all with the Microsoft Store** asks the Store to do it. Each app shows its progress (size, speed, time left), can be stopped and carried on later from where it stopped, and has its download links to copy. The list and the downloads survive Deck closing or crashing. Apps that couldn't be checked, and why, are listed instead of being counted as up to date.
- **Phone as a controller without the screen** (phone remote → Screen → Use as a controller): just the controller, on its own connection, so a press reaches the PC with the least delay; the round trip is shown in ms. Several phones become several controllers (Player 1 to 4, shared with real controllers and DS4Windows), each phone showing its player number. Size, opacity, a background (colour or photo) and the layout (follow the phone, or always sideways) are remembered on each phone; holding the phone upright gives an upright layout.
- **Tweaks explain themselves:** every tweak says, in plain words, what you gain and what you give up.
- **Hide after opening an app** (Settings → General, quick panel): off keeps Deck open after starting apps, files or links from it.
- PC to PC: a transfer cut off by Deck closing (or cancelled) has **Carry on**, which sends only what the other PC is missing; the transfer list survives restarts.

### Changed
- **Tweaks are checkboxes** worded as what Deck does ("Show hidden files and folders"): ticked is Deck's change, unticked is Windows' own setting. Lists of choices mark Windows' default.
- Apps, files and links opened from Deck's search start in the background: Deck no longer stops responding while an app asks for administrator rights, and the UAC prompt comes to the front.
- Workspace Sync's problems say what happened and what to do (for example a cloud folder switched on with no folder chosen).
- Phone remote: full screen notices when the browser quietly left it, and says so when the browser doesn't allow it. Files sent to the phone stay listed after Deck restarts, and an interrupted download on the phone can continue (Deck now tells the browser the file is unchanged).
- An advanced uninstall that Deck was closed in the middle of is offered to finish on the Apps page.

### Fixed
- **Open RivaTuner** closed RivaTuner by force, which could leave Deck or a browser frozen. It now asks RivaTuner to close, and Deck's own windows are kept out of RivaTuner's hooks.
- "Update all with the Microsoft Store" seemed to do nothing when the "Don't update Store apps by themselves" tweak was on (Windows blocks the Store's background updates then). Deck now says so and offers what works; otherwise it reports which apps the Store started or cancelled.
- "Open in Store" for paid apps did nothing.

### Also in this version
- **Phone remote, Screen view:** an on-screen controller in the PlayStation layout (two sticks, D-pad, face buttons, L1/R1/L2/R2, L3/R3, Share, PS, Options; several fingers at once), with its size and opacity in Settings. The PC sees it as an Xbox controller (needs the ViGEmBus driver). **Hold click** keeps the left button down while your finger is on the screen. F1 to F24 in the Keys bar. **Control in games** lets taps and keys reach a game in front (off by default).
- **Workspace Sync for Claude Code** (was "Claude Code sync") carries more than conversations now, so a project can be picked up on another PC:
  - **Project files** (Settings → What to sync, off until you turn it on): each project's folder is kept the same on your PCs, without `.git` and build output. Folders over 40,000 files or 3 GB, a drive, or your user folder are never copied. Deleted files go to the trash folder on the other PCs, and switching it off deletes nothing. **Build files** (target, node_modules, .venv…) can be included too.
  - **Per project:** switch a project's sessions off and keep its memory and files syncing, to start a new session on the other PC and carry on with the project. Its files can be switched off on their own as well.
  - **Programs:** each PC shares which Rust toolchains, targets and components, cargo tools, Node and global npm packages, Python, pipx tools and Git it has. The Status tab lists what another PC has and this one doesn't, with Install (a PowerShell window with your normal rights; nothing installs on its own).
  - **Tailscale is visible:** Settings has an "Over Tailscale, from anywhere" switch with its state (not installed / not connected / which of your PCs are on it, with Get or Open Tailscale), the Status tab shows "Away from home", each PC says whether it is reached over the home network or Tailscale, and PC to PC shows the same. The cloud folder works with any synced folder (OneDrive, Google Drive, Dropbox, MEGA, pCloud, Syncthing).
  - **MCP servers** (yours and each project's, from `.claude.json`) sync with the settings, and wait for your OK like hooks and plugins do. Added on two PCs at once, both lists are kept.
  - **Git history:** files are copied without `.git`; on a PC whose copy has no git, Projects offers **Get git history** (connects the folder to the same remote and branch, files untouched).
  - **Safer with different PCs:** a project folder only guessed from another PC waits until you keep or move it before files arrive, and a guess under the other PC's user folder becomes the same place under this PC's. Build files aren't copied between an ARM and an Intel/AMD PC. A project stops copying when its drive has under 2 GB free. Rust toolchains are compared as "stable" and the like, so each PC installs its own kind. The program check also covers Microsoft's C++ build tools, which Rust needs.
  - **Far fewer disk writes:** the sync state is saved only when it changed, and the file-hash cache at most every 10 minutes (before, about 1.2 MB was rewritten every cycle even when nothing changed).
  - A project's own `.claude` settings, hooks and `.mcp.json` wait for your OK too; `.claude/settings.local.json` stays on each PC.

### Changed
- The design notes moved to `docs/design/`.

### Security
- Updated the TLS certificate checks (rustls-webpki 0.103.16, webpki-roots 1.0.10) for RUSTSEC-2026-0381 and RUSTSEC-2026-0382: distrusted emSign intermediates could have been accepted.

## 2.7.5 — 2026-10-20

### Added
- **Your phone as the PC's speaker or microphone** (Deck → Phone → Speaker and mic, both off until you turn them on; the phone remote gets a Speaker and mic card). **Speaker:** the phone plays the PC's sound, with a switch to turn the PC's own speakers off while it plays (they come back when it stops, even if Deck closes meanwhile). **Mic:** the phone's mic becomes a mic on the PC, with echo and noise cancelling, so Discord, games and calls can use it; pick "CABLE Output" in an app, or turn on "Make it the default mic while the phone talks" (your mic comes back after). Over Wi-Fi, about a tenth of a second late. The mic needs VB-CABLE, VB-Audio's free virtual cable, because Windows only lists a microphone that a driver makes: Deck turns on one that is installed but off, or downloads VB-Audio's own setup for you (its licence doesn't allow building it into Deck's setup).
- **Tweaks** (Maintenance → Tweaks): 132 Windows settings in 13 categories (privacy, Start and search, taskbar, File Explorer, right-click menu, appearance, multitasking, gaming, AI and Copilot, updates, network, system, advanced), from Win11Debloat and Chris Titus Tech's WinUtil, each checked, plus Deck's own. Every switch shows what Windows is set to right now ("Partly" when another tool set part of it), what it changes, its risk and whether it needs Explorer restarted, a sign-out or a restart. **Every change can be undone to exactly what it was before** (one by one or all at once), also months later; Deck notices when a Windows update changes a tweak back and offers to apply it again. Presets (Privacy, Clean Start & taskbar, Gaming, Quiet Windows) and "Recommended" open as checklists, never as one click that does everything. A restore point is offered before Advanced tweaks. Options that don't exist on this Windows version or this PC are hidden. Tweaks also turn up in Deck's search, and a PC to PC setup carries them to your other PC. Deck never removes Edge or WebView2, turns Windows Update or Defender off, or runs scripts from the internet.
- **Built-in apps:** remove Clipchamp, News, the Xbox apps and the rest (Win11Debloat's list with its safe / optional / needed rating), and reinstall any of them later.
- **Store apps without the Microsoft Store** (Tweaks → Get Store apps), like store.rg-adguard.net: search the Store, then Install, Download (to Downloads\Store apps), send to FluxDM, or copy the links. The files come straight from Microsoft's own servers, only the ones this PC is missing (usually just the app), checked against Microsoft's checksums, with a progress bar, speed and time left. Free apps only; desktop apps from the Store install with winget.
- **The phone remote also answers on https** (same address, `https://`). Phones allow the mic only on secure pages; Deck makes its own certificate, so the phone warns once ("not private") the first time.

### Changed
- **Store apps download about six times faster:** each file comes in 8 parts at once (Microsoft's servers give one connection only so much; WhatsApp's 311 MB took about 27 minutes on one). A cancelled or failed download has **Carry on** / **Try again** and continues where it stopped.

### Fixed
- PC to PC's "restart Explorer" after applying a setup started Explorer with Deck's admin rights; Windows now restarts it as you.
- Settings → Updates said "GitHub's answer wasn't understood" on every check after the first while no new version was out (GitHub's "nothing new" answer was read as a broken one). Updating itself wasn't affected.
- Turning the phone remote off and on moved it to the next port (the old one stayed open), so a phone's saved link stopped working.
- Undoing *Variable refresh rate* also undid *Windowed game optimizations* when both were changed (they share one DirectX setting); each now puts back only its own part.
- Deck's own updates (and the RustDesk setup) gave up after 60 seconds (5 minutes for RustDesk) of downloading, so on a slow line a release bigger than about 10 MB could never install by itself. Now only a connection that stalls for a minute ends them.
- Game boost could end an unrelated program that had just started and been given the ID of an app boost was closing (Windows reuses process IDs); it now checks the program first.
- RustDesk's, Tailscale's and winget's command lines that hang (a stuck service) no longer freeze the Remote desktop page or PC to PC: every program Deck runs has a time limit.
- Timers and Tweaks that shut down, restart or sign out said "done" even when Windows refused (another shutdown already scheduled); they now say why.
- The phone mic stayed "on" but silent when VB-CABLE went away mid-call; the phone now reconnects and says why. When Windows doesn't restart Explorer by itself (some debloat scripts turn that off), Deck starts it without its admin rights. Installing Store apps and VB-CABLE's setup no longer fail for a Windows user name with an apostrophe.

### Security
- **Tweaks' undo list can't be used to change Windows behind your back.** Undo writes the saved values back with Deck's admin rights, and the list lives in your own folder, where any program could edit it. Deck now keeps its fingerprint where only administrators can write; a list changed by something else is set aside (Tweaks says so) and never used, and undo only ever touches what that tweak itself changes.
- **"Unpin everything from Start" and Store downloads work with your rights only** in your folders (a folder swapped for a link can't make Deck read or write elsewhere).

## 2.7.0 — 2026-10-20

### Added
- **A new in-game death counter.** Pick how it looks: a skull with a big number (the default), a ring that fills toward the deaths your last boss took, or one huge number. It can count this boss, the whole game, or both side by side, and shows the game's total between fights (or nothing).
- **The card after a death.** When you die, the counter opens into a card for a few seconds: the boss, your next try, its deaths, how long that try lasted, your time on the boss, deaths this session, bars for your last 10 tries, and a highlight like "Your longest try yet" or "100th death in this game". Every line can be switched off, and so can the card.
- **Red edges on a death** (off by default): the screen's edges glow red for a moment.
- **A live preview** of the counter and the card next to their settings, drawn by the same code as in the game.
- **Play sessions.** Deck notes when you play each game; the History tab is now one card per session (how long, deaths, bosses beaten, deaths over time) with the events inside, instead of a long list. Older history gets sessions worked out from when your deaths were counted.
- **Boss cards** instead of the table: deaths, time, longest try, which try beat it, and a line of how long each try lasted, so you can see yourself learning a fight. Each boss can have a note (a strategy, a reminder), and the card after a death can show it.
- **Records** on the Now tab (longest try, quickest win, deadliest boss, most deaths in a session) and this session's numbers.
- **All games together** on the Games tab: total deaths, time played, bosses beaten, deaths per week, the deadliest games and bosses.
- **Share and export.** "Share" copies (or saves) a picture of a game's or a boss's numbers; "Export" saves a game's history, or everything, as a spreadsheet file.

## 2.6.1 — 2026-10-20

### Fixed
- **Death counter: dark death screens are read.** Windows' text recognition read nothing from a dim red banner on a dark screen ("KHAZAN HAS FALLEN"); Deck now makes the middle of the picture high-contrast before reading it, and a long phrase may have a letter or two misread. The First Berserker: Khazan is recognised outside Steam too.
- **Teaching a death screen works in dark games and when you count late.** Deck keeps the last 20 seconds (it was 10). When the whole screen fades at death, Deck learns the words or symbol that stand out on it instead of refusing it as "one flat colour", and ignores the character and scene behind them, which differ at every death.
- **Death screens keep their colour.** Pictures were turned grey by brightness, which made red words dull; Deck now uses each spot's brightest colour, and the teaching pictures are shown in colour.

### Changed
- **Games are known by their program only.** The death counter no longer uses Steam or Epic numbers or library lists: a game is its exe, the same in every store, named from the program's description, its install folder or its window title. Deck's list of games with their own death screens or save readers is now the 17 it knows by exe (an Easy Anti-Cheat launcher no longer counts as Elden Ring). Death screens taught under an older Steam key need teaching again.
## 2.6.0 — 2026-10-20

### Fixed
- **No more "Set up" after an install or an update.** The setup is the one place that asks for admin rights: it makes Deck's admin tasks itself and starts Deck hidden through them. Before, the setup started Deck as a normal user and every install showed the Set up banner again. A Deck started without admin rights (the setup's "Run Deck" box, a shortcut) while the tasks exist now hands over to the admin task by itself.
- **Deck comes back after a crash or a graphics driver update.** If something ends Deck, the watchdog gives the fans back to the BIOS and then starts Deck again, hidden. It waits for a running driver installer to finish first. After three such ends in ten minutes it stops and Deck starts with the fans on the BIOS and no overclock until you turn hardware control on (a toast says why). The log now says how Deck ended (crash kind and the DLL, or "ended by another program").
- **A driver install no longer hits Deck's GPU calls.** While NVIDIA's (or AMD's, Intel's) driver setup runs, Deck makes no GPU calls, and restarts itself once afterwards so the new driver is used and the profile is applied again.
- **The controller's real name in the "Connected" message** (a GameSir dongle said "XInput Controller #1").
- **The overlay's CPU clocks now really match MSI Afterburner.** 2.5.10 measured the average clock each core ran at, which comes out a few % below Afterburner's on a boosting CPU (5.1 GHz against 5.29), and the thread lines still used Windows' estimate. Afterburner shows each core's current speed step times the bus clock; the overlay now reads the same (Intel), so the CPU clock, every thread line and the core table show the same figures as Afterburner. The CPU page keeps the measured average.
- **Claude Sync no longer calls the same version "older".** It compares build times, so another build of the same version (a test build) showed "runs an older Deck (2.5.10; this PC has 2.5.10)". It now says the other PC runs a different build of that version.
- **A core's load in the overlay's core table is its busiest thread's.** The average of a P-core's two threads showed a game's main thread at 100 % as 50 %, hiding a CPU-bound game.

### Changed
- **"Like MSI Afterburner" looks like MSI Afterburner:** its layout (clock, CPU, a line per thread, RAM, FPS, GPU, MEM, then the frametime graph), its default colours, a white framerate, small thread numbers (CPU₁) and a small "Frametime" above the graph with the frametime after it. The framerate figures (min, average, lows) are left off, as Afterburner shows them only while benchmarking; add them back in Items.
- New look options: **Small thread numbers** (Layout), **Afterburner layout** for the graph (Graph), and a colour of its own for the framerate number.

### Added
- **Death counter: the same game in every store.** A game is known by its program, so a Steam, GOG, Game Pass or EA copy is the same game, with the same history and death screens. Fullscreen browsers and video players are never counted as games (a video of someone dying doesn't count), one death seen by the screen and pressed on your key counts once, and "WASTED" no longer matches "WANTED".
- **Updates from GitHub.** Deck looks once a day for a newer release and installs it by itself when no game or fullscreen program is in front: no prompt, no window, and it is back in the tray a moment later. The download is checked against the release's SHA-256 first. Settings → About shows the state, "Check now", "Install" and a switch to turn it off. A Deck without admin tasks only tells you and installs on your click (one admin prompt). The first install of this version is manual; updates come on their own after that.
- **Controller overlay** (Monitor → Controller). Controller Overlay, the separate app, is now part of Deck: your controller's buttons, sticks and triggers on screen over your games, in the same DualShock 4 look, with every theme, style, layout, size, corner and monitor setting it had, idle fade, Move / resize by dragging, demo input and keyboard & mouse mode. The tray menu has it under **Controller overlay**. Your Controller Overlay settings are brought over the first time Deck starts; uninstall the old app whenever you like.
- **Controller battery for any controller.** One tray icon per controller that reports its battery, phone style, and the same level on the overlay's battery pill. Reads GameSir dongles directly, PlayStation controllers from what they already send (nothing is written to them), and Xbox and Bluetooth controllers through Windows. Like a phone, the level never jumps up after switching the controller off and on; a warning comes once per charge at 15%. The Battery tab lists every controller and where its level comes from.
- **Test tab:** the controller the overlay reads, live (buttons, sticks, triggers), with or without the overlay on.
- The overlay stays out of screenshots and recordings unless you turn that off.
- **Death counter** (Monitor → Deaths). Count your deaths in any game, per boss. Start a boss fight, and every death counts toward it until you mark it defeated or abandon it, including deaths on the way back. Stats per boss: deaths, attempts, wins, deaths before the first kill and time. Count from the tray menu (Death counter), two keyboard shortcuts you choose, a controller combo (hold Select + D-pad Down for a second; Select + Up undoes), or the page; every count can be undone, and the history can be edited. A small counter shows over the game ("Malenia · 14"), click-through and hidden from recordings unless you want it in them. Any game works; Deck recognises Steam and Epic games by name and knows 320 hard games. **Counts by itself:** Deck watches the game's window for death screens ("YOU DIED", "GAME OVER", "WASTED"… plus a game's own, for a first set of games) and runs Windows' built-in text recognition only when the picture settles; Celeste and Dark Souls: Prepare to Die Edition are counted from their save files. Nothing it sees is recorded or saved, and both can be turned off. **Teach it a death screen:** for a game whose death screen has no words (Sekiro's), count the death yourself, then click the clearest of the last 10 seconds' pictures on the Deaths page; Deck counts that screen by itself from then on (up to 3 per game, shareable with friends as a code). **Shared library:** submit what you taught to everyone (a button that copies the code and opens a GitHub form; it is added after review), and Deck picks up new death screens from Deck's public GitHub page at most once a day (a plain download, nothing about you is sent; turn it off in Deaths → Settings).
- **GPU memory controller load** ("FB usage" in Afterburner) as an overlay item (NVIDIA).

## 2.5.10 — 2026-10-19

### Fixed
- **CPU clocks now match MSI Afterburner and HWiNFO.** Deck showed Windows' estimate of each thread's clock, which runs a few % low and jumps around (4.8-5.2 GHz on P-cores and 3.2-3.9 GHz on E-cores where Afterburner showed a steady 5.29 and 3.99). Deck now reads each core's own cycle counters, as Afterburner does, on Intel and AMD; Windows' figure is only used without the driver.

## 2.5.9 — 2026-10-18

### Fixed
- **CPU clocks read about 20 % low under some power plans.** Deck measured against Windows' "current" CPU clock, which a power plan lowers, instead of the CPU's fixed base clock. In a game, cores at 2.69 GHz (HWiNFO) showed 2.15 GHz on the overlay, the CPU page and Sensors. Now checked against HWiNFO core by core.
- **The overlay's framerate could come from a different program than its lows and average**, for example a launcher drawing faster than the game. FPS, frametime, game RAM and the framerate figures now all follow the same game.
- **The framerate no longer stays frozen** on a game that stopped drawing (alt-tabbed, paused or closed).
- **A new game no longer starts with the previous game's frames** in its average and lows.
- **The preview's API and resolution are the game's own.** They used to say "D3D12" and the desktop's size for every game. With no game, the preview's graph is empty instead of a made-up line.
- **Long overlays are no longer cut off in the middle.** RivaTuner 7.3.5 and newer get the 32 KB text field; on older versions, whole lines are left off the end instead of breaking the colours of everything after the cut.
- **Hiding RivaTuner's tray icon no longer starts a process every 5 seconds**, also while gaming.
- **RivaTuner installed outside its default folder is found.**

### Changed
- **The frametime graph is MSI Afterburner's:** RivaTuner's own graph of every frame, which it redraws with each frame, so it moves as smoothly as the game. Deck used to send it about 12 points a second, which looked choppy. It uses your style, height, width and top.
- **The graph sits on a line of its own**, labelled FT and lined up with the other lines, with the live frametime in front unless the overlay already shows it.
- **The graph's top on Auto is four average frames** (33 ms at 120 FPS): a normal frame sits a quarter of the way up and stutter stands out. RivaTuner's own autoscale followed the slowest frame on screen, so one hitch flattened the rest. A fixed top shows the framerate it stands for ("16 ms · 63 FPS").
- **"Shows the last" and "Smooth the line" are gone:** RivaTuner keeps its own frame history.

## 2.5.8 — 2026-10-18

### Changed
- **The CPU page is reworked.**
  - **A live column stays in view** while you scroll or edit: clock with its last minute, temperature, load, power, the power limit in force and who set it (your motherboard or the active profile), and every core's clock and load.
  - **"Held at the power limit"** appears when the CPU draws all the watts it may under load, so a low motherboard limit is no longer invisible.
  - **Profiles are edited on the page**, not in a popup: an edit button on each card opens the settings under the profiles, with an "Unsaved" mark and Save / Discard.
  - **Plain settings first:** clock caps, Turbo boost on/off, energy preference, power limits and extras. Windows' processor states, parked cores and the five turbo modes are folded under Advanced.
  - **A slider's end means off:** all the way right is "No cap", all the way left is "Off" for the clock floor (was "Keep the clock at least") and the overclock.
  - **"Plan's own" energy preference shows the plan's real value** ("Plan's own · 33"), with a note when it leans to saving.
  - **"Stock" power limits are now "Motherboard"** and show the board's own watts. Custom PL1/PL2 go above 253 W when the board's own limits do.
  - **Warnings** for settings that cancel each other (a cap below the overclock, a clock floor above the cap). "Hold the cap in games" only shows when the profile caps.

### Fixed
- **A clock cap at the CPU's top turbo no longer caps.** A profile saved with its cap slider at the top (5,300 MHz on an i5-14600KF) still held the CPU at that clock and blocked any overclock. Such caps now count as no cap when saving and applying, once Deck has read the CPU's real top turbo.
- **E-core clocks read about a quarter too low** in the overlay, on Sensors and in the per-thread rows (2.97 GHz for an E-core really at 4.0 GHz on an i5-14600KF). Windows measures every core's performance against the CPU's base clock, and Deck multiplied the E-cores' by their own lower one.
### Testing
- **`install.ps1` no longer does nothing when Deck is running.** It printed a warning and stopped, so a developer install often left the old version in place. It now closes Deck itself, retries the copy until the installed Deck.exe matches the build (and says so in a window that stays open if it can't), and starts Deck again.

## 2.5.7 — 2026-10-18

> **When you update:** if you typed your own PC to PC pairing code or Claude Code sync key, it keeps working. A new one you type must now pass the "too easy to guess" check.

### Security
- **Programs can't borrow Deck's admin rights through winget.** Deck ran `winget` (PC to PC → Apps) by name, which finds a copy in a folder any of your programs can write to. It now runs winget only from App Installer's own protected folder.
- **Remote desktop setup can't be hijacked through PowerShell.** Deck's admin PowerShell steps could load commands from your Documents folder before Windows' own. They now load commands only from Windows' folder, and take no folder from the environment.
- **RustDesk's installer is checked where it can't be swapped.** It is copied into a protected folder, kept locked while it is checked and installed, and must carry GitHub's checksum and RustDesk's own signature (not just any valid one).
- **Hardware settings can't skip the "changed outside Deck" check.** A test switch (`DECK_PROFILE`) that your programs could set made Deck read hardware settings from a fresh folder it then trusted. An admin Deck now ignores it.
- **The 32-bit RivaTuner helper** starts only from Deck's own folder when Deck runs as admin.
- **Keys on disk are sealed with Windows (DPAPI).** This PC's PC to PC key, the pairing code, the phone key and the sync key no longer sit in plain text in AppData, so a copied profile or backup doesn't carry them. Older files are sealed the first time Deck starts.
- **Typed pairing codes and sync keys that are easy to guess are turned down** (repeats, runs like 1234 or ABCD, or few different characters). PC to PC and Claude Code sync serve at most 64 connections at once, and a stranger has 10 seconds to prove the code.
- **Exploit mitigations:** Deck and its helper are built with Control Flow Guard, and Deck with CET shadow-stack support.

### Fixed
- **CPU clocks in the overlay and on Sensors show the real clock.** The main CPU clock and every P-core / E-core row read a register that stays at the top turbo step (5.30 / 4.00 GHz on a 14600KF) while a core naps, so they never moved. They now come from the same live Windows reading as the thread rows: a core shows its busiest thread's clock.

### Testing
- The UI test suites wait up to 30 seconds for headless Edge to start (a slow CI runner timed out at 10), and the notification-window test always closes its Edge, even when it fails.

## 2.5.6 — 2026-10-18

> **When you update:** update every PC that uses PC to PC or Claude Code sync, since old and new versions can't talk to each other. Then re-pair your PCs once (trusted PCs and "always allow" are asked again; "never" choices are kept). If Deck was installed per user before, click **Set up** once more after the update. The phone page reloads itself once.

### Security
- **The phone can no longer run programs on the PC.** A phone could ask Deck to "open" any file path by pretending to answer a "File from your phone" notification. The phone now answers only its own kind of notification (media, power, timers, text), and the "Open" button on a received file works only for a file that really arrived from that phone and only from the PC itself. Accepting transfers and applying a held GPU overclock are PC-only decisions now.
- **An elevated Deck ignores WebView2 flags from the environment.** When Deck runs as admin it no longer reads `WEBVIEW2_*` variables (any program running as you can set them, and a debugging port there would expose every command Deck has). The live-testing debug port now comes from Deck's own command line (`--cdp-port`).
- **Programs can't borrow Deck's admin rights through your files.** When Deck works in your folders (phone uploads, files from another PC, Claude Code sync, share links, Clean up, the Recycle Bin, its own settings), it does only what your everyday account could do. A folder swapped for a link can't redirect it into Windows' own files.
- **Deck won't share, send or serve a file your everyday account can't open**, and won't zip a folder that contains links.
- **Hardware settings changed outside Deck wait for you.** If another program edits Deck's fan, CPU or GPU settings, Deck keeps the fans on the BIOS and applies no overclock until you click **Apply them** on the PC.
- **The phone key no longer travels over the network.** The phone signs every request instead, and a recorded request can't be sent again. The phone page also refuses requests a web page sends by pointing one of its own internet names at your PC (DNS rebinding).
- **Each PC has its own key for PC to PC.** Trust and "always allow" now belong to that one PC, not to anything that knows the pairing code. Pairing codes and the Claude Code sync key are also much slower to guess.
- **Installers from another PC always ask on this PC**, even from a trusted PC, and run through Windows' normal admin prompt, never with Deck's own rights.
- **Claude Code sync holds risky changes.** If another PC's sync would change what Claude Code runs or may do without asking (hooks, plugins, status line, permissions), you see "Claude Code settings from <PC> are waiting" with **Apply** or **Keep this PC's**.
- **Share links:**
  - A one-time link can't be downloaded twice at the same time.
  - Home-only links can't be reached from the internet.
  - Funnel turns itself off at start unless an internet link is open.
- **Deck checks RivaTuner's shared memory before using it**, so a program pretending to be RivaTuner can't make Deck read or write outside it.
- **GPU and power libraries load only from Windows' own folder** (or NVIDIA's), never from a folder you or a program could write to.
- **"Open as your normal account" never falls back to admin.** If Explorer can't take a launch, Deck says so instead of starting the program with admin rights.
- **TLS library updated** (rustls 0.23.46) for four advisories published on 2026-10-14 (RUSTSEC-2026-0346 to 0349).
- **The RustDesk password no longer appears on PowerShell's command line.**

### Changed
- **Tools is one page.** Screen tools, Locked files and Windows panels are no longer separate tabs, and the page has a fresh look.
- **The setup installs for all users, in `C:\Program Files\Deck`.** It asks for admin once, and quietly removes an older per-user copy while keeping your settings.
- **Set up refuses an unsafe install.** If Deck sits in a folder your everyday account can change, Set up explains why it won't give it admin rights. Admin tasks that already point at such a copy are removed at start, with the notice "Deck's admin start was turned off".
- **PC to PC → Settings:** the switch for installing without asking from trusted PCs is gone, since installers always ask now.

### New
- **CPU profiles for plugged in and on battery** (CPU page, on PCs with a battery). Pick a profile for each; Deck switches when you plug in or unplug, and a profile you choose yourself stays until the next change. **Profiles also apply on battery** (on by default) makes a profile cap the CPU the same way on battery; turn it off to keep Windows' own battery values in each plan.
- **Fan watchdog.** If Deck is ended from Task Manager, the fans go back to the BIOS and the GPU fan back to the driver within seconds. While hardware control is on, a second small `deck.exe` in Task Manager is the watchdog.
- **Deck keeps a log** in `%LOCALAPPDATA%\Deck\logs`, and the This PC support report includes its last lines.
- **A "Shut down when…" plan survives a Deck restart.** It starts waiting again from the beginning.

### Fixed
- **Building from source** works with Rust 1.99 (its new clippy check failed the build in CI), and the tests pass when run as admin.
- **Overlay "Threads" table showed fake clocks.** Every thread read its base clock (for example 3500 MHz on P-cores and 2600 MHz on E-cores) whatever it was really doing, so a CPU cap looked like it did nothing. It now shows each thread's live clock, the way Task Manager measures it, with or without admin rights.
- **Overlay "Game RAM" measured the wrong program** when another window was in front (a screenshot tool, Discord). It now measures the game RivaTuner is drawing in, and shows nothing if the game's anti-cheat hides it.
- **The CPU clock without admin rights** read too high when an E-core was the busiest core.
- **Bigger click targets** on the Disk space tree arrows, the Home tile settings button and the overlay tick boxes; screen readers now name both "send text" boxes.
- **Settings survive a bad shutdown.** If `settings.json` is damaged (for example by a hard reset mid-save), Deck keeps the damaged file, restores the last good copy, and otherwise keeps every setting it can still read instead of silently resetting everything. Each save now keeps a backup of the previous version and is written so a crash leaves a whole file, never a half-written one.
- **The GPU/CPU crash guard fails safe.** If its record file is damaged, Deck now holds a risky overclock back at boot instead of re-applying it — the exact situation the guard is meant for.
- **Pairing codes and the phone key** are generated without the slight bias the old method had, and Deck now stops with a clear error if the system's random number generator fails rather than making a guessable token.
- **Themes:** button and chip borders on hover, switches, sliders, progress bars, scrollbars, the calendar, toasts and app placeholders kept the default theme's blue-grey in every theme. They now take each theme's own colours.
- **The overlay key could fail to switch RivaTuner** on a PC without Microsoft's Visual C++ runtime: Deck's small 32-bit RivaTuner helper needed it. The helper now carries its own copy, like Deck.
- **Clipboard page slow with a huge item.** A copied log of 200,000+ characters went onto the page in full and made the page take about 0.2 s to open. Cards now show the start of an item, and the preview the first 20,000 characters (with a note); Copy and Paste still use all of it.
- **Sidebar badges** (a running timer, Keep awake on, a plan set, a phone connected, a PC waiting) were squeezed out next to long page names. They now sit on the page's icon: a number for a count, a dot for on or set (hover it for the word).
- **More files survive a bad shutdown.** Your clipboard history, the list of items received from a phone or another PC, and the Game boost restore-state are now written the same crash-safe way as settings, so a hard reset mid-save can't truncate them.

## 2.5.5 — 2026-10-16

### Changed
- **Overlay page, redesigned.** It was one long column of seven sections (about four screens of scrolling).
  - **Top strip:** the overlay switch and RivaTuner's state, with the keys you use in game: show or hide, and the frame limit (limit, on/off and its key).
  - **Preview:** bigger, and it stays in view while you edit, with the quick looks under it.
  - **Settings:** seven short tabs: Items, Layout, Look, Colours, Graph, Warnings, Behaviour. Deck remembers the last tab you used.
  - **Denser lists:** the item list is compact, with move and remove buttons showing on hover. "Add readings" opens when you want it.

### Fixed
- **Docs:** the README describes the new Overlay page and the framerate limiter, and has a new Overlay screenshot. It also says the CPU readings need admin.
- **Home → PC tile on a PC where Deck isn't running as admin:** the CPU card showed only "20 threads", with no clock, power or temperature, and gave no reason. Those readings need admin, so the card now says *Needs admin* and sets it up when clicked. The tile's customize dialog says the same.
- **Open RivaTuner did nothing** (the mouse showed a busy cursor for a moment) when RivaTuner's own *Start minimized* option is on: it restarted straight into the tray, whose icon Deck may hide. Deck now turns that option off for this one start and puts it back once the window is open.
- **Framerate limit key only turned the limiter on:** when Deck couldn't read RivaTuner's switch it took it as "off", so every press meant "turn on". Deck now reads the switch from RivaTuner's 64-bit hooks (and remembers what it set last), so the key turns it off again.
- **Framerate limit didn't reach a running RivaTuner:** Deck wrote the limit into RivaTuner's profile file, which RivaTuner only reads when it starts (its window still showed 0). Deck now also sets it through RivaTuner's own profile interface, so RivaTuner and running games use it at once.
- **Shortcut keys on a laptop's built-in numpad:** Fn + 9 (Page Up) was saved as "Num9", which never fires because Windows gets Page Up. The recorder now saves the key the numpad key actually sends (Page Up, Home, End, arrows…). Fn itself never reaches Windows, so there's nothing to "allow": press Fn + the key and the recorder now saves the right one.
- **Phone page settings looked unchangeable:** a click on a Screen view option (picture size, smoothness, bitrate…) was saved, but the page kept showing the old choice until something else redrew it. The page now redraws right away.
- **Fold-out sections closed by themselves** whenever a page redrew (for example the Phone page when a phone connected). Open sections now stay open.
- A setting that **failed to save** no longer shows as if it had been changed.
- **Bitrate → Type…:** pressing Cancel (or typing something that isn't a number) set the bitrate to Auto; it now keeps the bitrate. Empty still means Auto.
- **Phone limits:** two phones connecting at the same moment could both get the last place; a phone that was turned away could still send and download files. Both closed.
- **Screen view:** tap then drag on a window's title bar or a file could act as a double-click (maximize, open) on the PC; a long press during tap-then-drag right-clicked instead of dragging.
- **Shortcuts in full-screen games:** quick taps are seen more reliably, and a shortcut held while a game came to the front no longer runs again.

## 2.5.4 — 2026-10-16

### New
- **Who can connect** (Phone page): how many phones may connect at home and how many away (no limit, 1, 2, 3 or 5 each), and two switches: a phone at home shuts out phones away, or a phone away shuts out phones at home. A phone that is turned away sees why and tries again every 10 seconds.

### Fixed
- **Overlay key and other Deck shortcuts didn't work in League of Legends** (and other games that switch off Windows shortcuts while they are in front). While a full-screen game is in front, Deck now also watches its shortcut keys itself and runs one Windows didn't deliver (the rest of the time it only checks once a second whether a game came to the front). Still no keyboard hooks.
- **Phones connected** counted connections, not phones (the remote, its Screen view and its sound each added one), and didn't say where they were. It now counts each phone once and shows how many are at home and how many away.
- **Screen view on the phone:** sliding a finger held the left mouse button down (dragging or selecting on the PC). Sliding now just moves the pointer, like a laptop touchpad; tap, then touch again and slide, to drag.

## 2.5.3 — 2026-10-15

### Fixed
- **Lighting controls did nothing** (every effect, on or off, and the lights test too) while Deck said "Motherboard RGB connected". The board's lighting controller had lost the configuration Deck writes when it connects, and then ignores every colour sent to it without any error. Deck now checks every 5 seconds that the board still holds its configuration and writes it again when it doesn't.
- A **lights test** that never finished (the controller stopped answering) kept all lighting frozen and refused a new test until Deck restarted. The hold is now always released: the test gives up after 40 seconds, and a hold older than a minute is cleared by the lighting loop.

## 2.5.2 — 2026-10-15

### Fixed
- **Setup and uninstaller still said "Failed to kill Deck"** when Deck had no `\Deck\Quit` task yet (2.5.1 only adds it once it has started with admin rights). The setup / uninstaller now first uses that task and, if it isn't there, closes Deck by running `deck.exe --quit` as admin (one Windows prompt). Note: an uninstaller belongs to the version that is installed, so a Deck older than 2.5.1 still needs to be closed from the tray once; installing 2.5.2 over it replaces that uninstaller.

## 2.5.1 — 2026-10-15

### Fixed
- **Setup and uninstaller could not close Deck** ("Failed to kill Deck. Please close it first"): Deck runs with admin rights, the setup does not, so Windows refused to end it. They now close Deck through a new admin task (`\Deck\Quit`), which also hands the fans back to the BIOS. Deck adds the task itself the next time it starts with admin rights.

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
