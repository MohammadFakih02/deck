# Tweaks — design

Date: 2026-10-20. Status: **built** (all stages at once, at the user's request; see PROJECT_STATE.md). Page: **Maintenance → Tweaks**. Added while building: **Get Store apps** (Store apps without the Store, rg-adguard style, with FluxDM hand-off).

## Intent

One place to set Windows the way you want it: privacy, Start and taskbar, Explorer, gaming,
AI features, updates, built-in apps. It takes the *content* of the well-known debloat tools
and puts it in a page that is better than Winaero Tweaker's: every option shows what Windows
is set to **right now**, says exactly what it changes, and can be **undone to the exact value it
had before**, one by one or all at once.

## Goals

- Every option reads the PC's real state (not "what Deck last did").
- Every change is undoable to the *exact* previous value (or "it wasn't there"), even months
  later, after updates and Deck restarts.
- Nothing runs without the user seeing what it is: presets are checklists, not buttons that do
  forty things.
- When Windows puts a setting back (feature updates do this), Deck notices and says so.
- Works on a friend's PC: options are shown only when that Windows build has them; nothing is
  tied to this PC's hardware.

## Non-goals

- Activation scripts, licence bypasses (WinScript's "Run MAS"), anything piracy-related.
- A general script runner. Deck applies its own, reviewed recipes; no free-form PowerShell
  from a list downloaded at run time.
- Driver installs, BIOS settings, overclocking (those belong to the CPU/GPU pages).

## Anti-goals (failure even if it works)

- **Breaking Deck itself.** Deck's window is WebView2 (Edge's engine): "Remove Edge" style
  options that can take WebView2 with them are never offered.
- A PC that can no longer update or protect itself: no "Windows Update off", no "Defender
  off", no blanket "all services to Manual".
- A tweak that fights another Deck page (two places changing Game Mode or the power plan).
- Changing anything while a game is in front (Deck's rule 2): apply waits, or asks.

## Where the content comes from

| Source | Licence | What Deck takes |
|---|---|---|
| Chris Titus Tech **WinUtil** (`config/tweaks.json`) | MIT | Recipes: registry values, services, original values. Credited in `THIRD_PARTY_NOTICES.md`; each tweak's "Details" names its source. |
| Raphire **Win11Debloat** (`Config/Apps.json`, `Config/Features.json`, `Regfiles/`) | MIT | The built-in app list with its safe / optional / unsafe rating, and recipes with the Windows build each needs (`MinVersion`). Credited the same way. |
| ravendevteam **Talon** | BSD-3-Clause | Nothing directly: it runs WinUtil and Win11Debloat in one go (and asks to exclude C: from Defender). Covered by taking from those two. |
| flick9000 **WinScript** | GPL-3.0 | **Ideas only.** Its code and lists can't be copied into MIT Deck. Where it has an option the others don't, Deck writes its own recipe from Microsoft's documentation. |
| **Winaero Tweaker** | closed freeware | Only the idea of a category list with an explanation per option. |

The recipes become Deck's own data file (`src-tauri/tweaks/tweaks.json`, built into the exe),
reviewed one by one: values checked against Microsoft's docs where they exist, and every
recipe live-tested (apply, read, undo, compare) before it ships.

## How it works

### A tweak

```json
{
  "id": "explorer.show_extensions",
  "cat": "explorer",
  "name": "Show file extensions",
  "what": "Shows .exe, .png, .txt after file names, so a \"photo.jpg.exe\" can't hide what it is.",
  "risk": "safe",                      // safe | behaviour | advanced
  "after": "explorer",                 // none | explorer | signout | restart
  "min_build": 10240, "max_build": null,
  "admin": false,                      // true when anything is under HKLM, a service or a task
  "kind": "switch",                    // switch | choice (several values, like "Explorer opens to")
  "on":  [{ "reg": "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Advanced", "name": "HideFileExt", "type": "dword", "value": 0 }],
  "off": [{ "reg": "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Advanced", "name": "HideFileExt", "type": "dword", "value": 1 }],
  "windows_default": "off",
  "source": "WinUtil (WPFToggleShowExt), Win11Debloat (ShowKnownFileExt)"
}
```

Step types, all done by Deck's own code (no PowerShell unless noted):

| Step | What |
|---|---|
| `reg` | Set a value (dword, qword, string, expand string, multi string, binary) or `"delete": true`. HKCU means the signed-in user (see Elevation). |
| `service` | Start type (auto, manual, disabled) of one named service; never a list of dozens. |
| `task` | A scheduled task on or off (`\Microsoft\Windows\...`). |
| `feature` | A Windows optional feature on or off (DISM API; Sandbox, WSL, IE mode…). |
| `appx` | Remove a built-in app for this user (PowerShell `Remove-AppxPackage`, as Win11Debloat does), reinstall from the Store. |
| `cmd` | One fixed command from a short list Deck knows: `powercfg /h on|off`, `bcdedit /set {current} …` (verbose boot), `fsutil storagereserve …`. Its undo is the opposite command, also fixed. |

### Reading the state

A switch is **on** when every step of `on` matches the PC, **off** when every step of `off`
matches (a missing value counts as Windows' default when the recipe says so), and **mixed**
otherwise ("Partly set: something else changed part of this"). Read when the page opens and
after every change; nothing is cached as truth.

### Undo that is exact

Before a step runs, Deck writes what was there into `tweaks.json` in its folder (atomic, like
every store): the value with its type, or "absent", or the service's start type, or the task's
state. Undo writes that back. So undo returns the PC to *its* old state, not to a textbook
default. The journal keeps the first value Deck saw (applying twice doesn't overwrite it).
**Undo all** walks the journal backwards.

### Windows changed it back

A daily check (and every page open) compares each applied tweak with the PC. One that no
longer matches moves to **"Windows changed this back"** with *Apply again* or *Forget it*.
Feature updates are the usual cause; the card says so.

### After-effects, batched

Changes that need Explorer restarted, a sign-out or a restart are collected: one bar at the
bottom, "3 changes need File Explorer restarted · Restart Explorer now / Later". Explorer is
never restarted while a game is in front or a file copy is running (Deck already knows both).

### Safety

- **Restore point** offered before the first Advanced tweak of a session (and before app
  removal), created with Windows' own System Restore; skipped with a note when System Restore
  is off.
- **Elevation:** HKLM, services, tasks and features need admin; Deck's admin task has it. HKCU
  must be the signed-in user's: when Deck runs as a different account (an admin password typed
  for a standard user, `elevation::runs_as_desktop_user` is false), per-user tweaks are
  disabled with that reason instead of changing the wrong account.
- **Games:** applying waits while `boost::game_in_front` sees a game, and says so.
- **Build gates:** each recipe has `min_build`/`max_build` (Win11Debloat's numbers, e.g. Copilot
  22621, Start "All apps" 26200); an option Windows doesn't have is hidden, not greyed.
- **Policies:** recipes under `SOFTWARE\Policies` are marked "policy" in Details (Windows then
  shows "Some settings are managed by your organization"; the Details explain that line).
- **PC to PC:** the Setup tab can carry the tweaks you applied to your other PC (as a list of
  tweak ids, applied there through the same engine, never as raw registry).

## Screen design

Winaero's layout (categories left, options right) with Deck's look, and the parts it lacks:
a search, live state, risk and after-effect badges, and the Changed / Changed back views.

```
┌ Tweaks ──────────────────────────────────────────────────────────────────────────────┐
│ Set Windows the way you want it. Every change can be undone.        [ Presets ▾ ]    │
├──────────────────────┬───────────────────────────────────────────────────────────────┤
│ ◉ Overview           │ 🔍 Search tweaks…                 [All] [Changed] [Safe only]  │
│ ─────────────        │                                                               │
│ Privacy        4/18  │ PRIVACY                                                       │
│ Start & search 2/12  │ ┌───────────────────────────────────────────────────────────┐ │
│ Taskbar        3/14  │ │ Telemetry and diagnostic data                    [  ●  ]  │ │
│ File Explorer  5/17  │ │ Sends Microsoft only the required diagnostic data.         │ │
│ Right-click    1/5   │ │ SAFE · admin                                   Details ▾  │ │
│ Appearance     0/8   │ ├───────────────────────────────────────────────────────────┤ │
│ Multitasking   0/7   │ │ Advertising ID                                   [ ●   ]  │ │
│ Gaming         2/9   │ │ Apps can't show you ads based on what you do.              │ │
│ AI & Copilot   0/9   │ │ SAFE                                           Details ▾  │ │
│ Updates        1/6   │ ├───────────────────────────────────────────────────────────┤ │
│ Network        0/4   │ │ Explorer opens to          ( Home | This PC | Downloads )  │ │
│ System         2/12  │ │ BEHAVIOUR · restarts Explorer                  Details ▾  │ │
│ Built-in apps  6 off │ └───────────────────────────────────────────────────────────┘ │
│ Advanced  ⚠    0/9   │                                                               │
│ ─────────────        │   Details: what it changes                                    │
│ Changed by Deck (19) │   HKCU\…\Explorer\Advanced  HideFileExt  1 → 0                │
│ Changed back (2) ⚠   │   Before Deck: 1 (dword) · Source: WinUtil, Win11Debloat      │
│                      │   [ Undo ]                                                    │
├──────────────────────┴───────────────────────────────────────────────────────────────┤
│ 3 changes need File Explorer restarted.               [ Restart Explorer now ] [Later]│
└──────────────────────────────────────────────────────────────────────────────────────┘
```

**Overview** (the first view):

```
┌ Changed by Deck ─────┐ ┌ Windows changed back ─┐ ┌ Recommended for this PC ─────────┐
│ 19 tweaks            │ │ 2 after an update     │ │ 6 safe ones you haven't set:     │
│ [ See list ] [Undo all]│ │ [ Apply again ]      │ │ ads ID, Start suggestions, …     │
└──────────────────────┘ └───────────────────────┘ │ [ Review and apply ]            │
                                                    └──────────────────────────────────┘
Presets: Privacy · Clean Start & taskbar · Gaming · Quiet Windows (no tips, no nagging)
```

**A preset** opens a checklist (every tweak it contains, ticked, with its badges); untick any,
then *Apply 11 changes*. A preset is never applied blind.

**Built-in apps** (its own view):

```
 🔍 Search apps…          Show: [Installed] [Removed by Deck]       ☐ Show apps Windows needs
 SAFE TO REMOVE                                  OPTIONAL
 ┌──────────────┐ ┌──────────────┐               ┌──────────────┐ ┌──────────────┐
 │ Clipchamp    │ │ News         │               │ Xbox app     │ │ Phone Link   │
 │ Video editor │ │ MSN news     │               │ Game Pass,   │ │ Phone on PC  │
 │ 120 MB       │ │ 30 MB        │               │ Xbox games   │ │              │
 │ [ Remove ]   │ │ [ Remove ]   │               │ [ Remove ]   │ │ [ Remove ]   │
 └──────────────┘ └──────────────┘               └──────────────┘ └──────────────┘
 Removed apps can be reinstalled from here (Microsoft Store), one by one.
```

Only installed apps are listed. "Unsafe" ones (the Store, Calculator's runtime, Xbox Identity
Provider that Game Pass games need, Windows Web Experience Pack…) stay hidden unless the box is
ticked, and then carry a red line saying what breaks. Reinstall opens the Store page by the
app's package family name (`ms-windows-store://pdp/?PFN=…`), or `winget install` where
Win11Debloat removes it with WinGet.

Every row has a "?" like the CPU/GPU editors (`help.js` style) for the longer explanation.

## The tweaks

Risk: **S** safe (cosmetic or privacy, nothing breaks), **B** changes how something behaves
(you may miss it), **A** advanced (can break a feature; restore point offered). After: **E**
Explorer restart, **O** sign out, **R** restart. Source: **C** WinUtil, **W** Win11Debloat,
**D** Deck's own recipe (idea from WinScript or Microsoft docs). Build: lowest Windows build.

### Privacy (18)

| Tweak | What it does | Risk | After | Src |
|---|---|---|---|---|
| Telemetry: required only | Diagnostic data at the minimum Windows allows; tailored experiences off | S | – | C W |
| Advertising ID off | No ad ID for apps | S | – | C W |
| Activity history off | Windows stops recording and uploading what you do (clipboard history unaffected) | S | – | C |
| Location off | Windows location service and app access to location | B | – | C W |
| Find My Device off | No location reports for Find My Device | B | – | W |
| Suggestions and tips off | "Get tips", suggested content in Settings, "finish setting up" nags | S | – | W |
| Lock screen tips off | Fun facts and tips on the lock screen | S | – | W |
| Settings ads off | Microsoft 365 / Copilot ads on Settings → Home (22000) | S | – | W |
| Promoted apps off | Windows installing suggested Store apps (consumer features) | S | – | C |
| Online speech recognition off | Speech isn't sent to Microsoft | S | – | C D |
| Inking & typing personalisation off | No typing/ink history collected | S | – | C |
| Feedback requests off | Windows never asks for feedback | S | – | D |
| App access: camera / microphone / contacts / call history / messages / email / tasks / calendar / radios / documents / pictures / videos | One **choice** per item: Allowed / Ask (Windows' own) / Off for apps | B | – | D |
| Edge ads and suggestions off | New tab news, shopping, "sidebar" prompts | S | – | C W |
| Device companion apps off | No vendor apps installed when a device is plugged in | S | – | C W |
| Cloud content in search off | Search doesn't show signed-in cloud content | S | – | D |
| Error reporting: ask first | Crash reports wait for your OK | B | – | D |
| Windows Platform Binary Table off | Firmware can't make Windows run vendor programs at boot | A | R | C |

### Start & search (12)

| Tweak | What it does | Risk | After | Src |
|---|---|---|---|---|
| Bing in search off | Start search stays on this PC | S | E | C W |
| Store suggestions in search off | (22621) | S | – | C W |
| Recommended section | **choice**: Shown / Hidden (22621) | S | E | C W |
| Recently added apps hidden | | S | – | D |
| Phone Link in Start off | (22621) | S | – | W |
| All apps view | **choice**: Category / Grid / List (26200) | S | – | W |
| Hide All apps | (26200) | B | – | W |
| Start previous layout | Only where Windows still allows it (max build checked) | B | E | C |
| Unpin everything from Start | One action, not a switch; undo restores the saved pin layout | B | E | W |
| Search history on this device off | | S | – | D |
| Web search in the search box off (policy) | Firmer version of "Bing off" for builds that ignore the user switch | B | O | D |
| Search indexing: only your folders | Indexes Start menu and Users only (not whole drives) | B | – | D |

### Taskbar (14)

| Tweak | What it does | Risk | After | Src |
|---|---|---|---|---|
| Alignment | **choice**: Centre / Left (22000) | S | – | C W |
| Search | **choice**: Box / Icon and label / Icon / Hidden | S | – | C W |
| Task view button | | S | – | C W |
| Widgets | Off removes the button and the widget board on the lock screen | S | – | C W |
| Chat / Meet now | (Windows 10, early 11) | S | – | W |
| End task on right-click | (22631) | S | – | C W |
| Last active click | Clicking a grouped app opens its last window | S | – | W |
| Combine buttons (main display) | **choice**: Always / When full / Never | S | E | W |
| Combine buttons (other displays) | **choice** | S | E | W |
| Show apps on | **choice**: All taskbars / Main and where open / Where open | S | E | W |
| Battery percentage | Laptops only (hidden on desktops) | S | – | C |
| Copilot button | | S | – | W |
| Notifications and calendar flyout off | | A | O | C |
| Seconds in the clock | (22621) | S | – | D |

### File Explorer (17)

| Tweak | What it does | Risk | After | Src |
|---|---|---|---|---|
| Show file extensions | | S | E | C W |
| Show hidden files | | S | E | C W |
| Show protected system files | Warns that deleting them breaks Windows | A | E | D |
| Explorer opens to | **choice**: Home / This PC / Downloads / OneDrive | S | – | W |
| Home in the side pane | (22000) | S | E | C W |
| Gallery in the side pane | (22000) | S | E | C W |
| OneDrive in the side pane | | S | E | W |
| Duplicate removable drives off | USB drives listed once | S | E | W |
| Folders back under This PC | (22000) | S | E | W |
| 3D Objects / Music under This PC | Windows 10 | S | E | W |
| Drive letters | **choice**: After name / Before name / Network first / Hidden | S | E | W |
| Automatic folder type discovery off | Stops Explorer guessing "Pictures" for big folders (faster) | B | E | C |
| Long paths | Paths over 260 characters for programs that support it | B | – | C |
| Compact view | Less padding in lists | S | E | D |
| Full path in the title bar | | S | E | D |
| Recent files and frequent folders off | In Home / Quick access | S | E | D |
| Check boxes to select files | | S | E | D |

### Right-click menu (5)

| Tweak | What it does | Risk | After | Src |
|---|---|---|---|---|
| Classic right-click menu | Windows 10 style, no "Show more options" (22000) | B | E | C W |
| "Include in library" off | | S | E | W |
| "Give access to" off | | S | E | W |
| "Share" off | | S | E | W |
| "Ask Copilot" / AI entries off | (26100) | S | E | D |

### Appearance (8)

| Tweak | What it does | Risk | After | Src |
|---|---|---|---|---|
| Dark mode | Links to Display (Deck already schedules it); no second switch | – | – | – |
| Transparency off | | S | – | W |
| Animations off | Window and menu animations | S | – | W |
| Visual effects: best performance | All of sysdm.cpl's effects off, font smoothing kept | B | O | C |
| Desktop Spotlight | **choice**: On with "Learn about this picture" / On without it / Off | S | – | W |
| Scrollbars always visible | | S | – | C |
| Accent colour on title bars | | S | – | D |
| Sign-in screen blur off | | S | – | C |

### Multitasking (7)

| Tweak | What it does | Risk | After | Src |
|---|---|---|---|---|
| Window snapping | | B | – | C W |
| Snap assist suggestions | (22000) | S | – | W |
| Snap layouts flyout | (22000) | S | – | W |
| Alt+Tab shows browser tabs | **choice**: None / 3 / 5 / 20 (22000) | S | – | W |
| Shake to minimise | | S | – | D |
| Sticky Keys shortcut (5× Shift) off | (26100) | S | – | C W |
| Num Lock on at start | | S | R | C |

### Gaming (9)

| Tweak | What it does | Risk | After | Src |
|---|---|---|---|---|
| Game Mode | **Shown, not changed here:** CPU profiles set it; links to the CPU page | – | – | – |
| Game DVR / background recording off | Xbox capture | S | – | C W |
| Game Bar shortcut (Win+G) and controller button off | | B | – | W |
| Mouse acceleration off | "Enhance pointer precision" | B | – | C W |
| Fullscreen optimisations off (all games) | Old exclusive-fullscreen path | A | – | D |
| Optimisations for windowed games | (22621) **choice**: On / Off | B | – | D |
| Hardware-accelerated GPU scheduling | Restart; shown only when the GPU supports it | B | R | D |
| Multiplane overlay off | Fixes flicker on some GPUs; can raise latency | A | R | C |
| Variable refresh rate for windowed games | | S | – | D |

### AI & Copilot (9)

| Tweak | What it does | Risk | After | Src |
|---|---|---|---|---|
| Copilot off | (22621) | S | E | W |
| Recall off | (22621; Copilot+ PCs) | S | R | W |
| Click to Do off | (22621) | S | – | W |
| AI service doesn't start by itself | (22621) | B | R | W |
| Edge AI features off | | S | – | W |
| Chrome AI features off | Only when Chrome is installed | S | – | W |
| Paint AI off | | S | – | W |
| Notepad AI off | | S | – | W |
| Copilot in Explorer and right-click off | | S | E | D |

### Updates & Store (6)

| Tweak | What it does | Risk | After | Src |
|---|---|---|---|---|
| Don't get updates as soon as they're out | | S | – | W |
| No automatic restart while signed in | | S | – | W |
| Delivery optimisation off | No uploading updates to other PCs | S | – | C W |
| Drivers through Windows Update off | Keeps your GPU driver Windows won't replace | B | – | D |
| Store apps update by themselves | **choice**: On / Off | B | – | D |
| Pause updates up to | **choice**: Windows' limit / 1 year (sets the allowed pause length, not a block) | A | – | D |

### Network (4)

| Tweak | What it does | Risk | After | Src |
|---|---|---|---|---|
| Prefer IPv4 over IPv6 | Keeps IPv6 working, IPv4 first | B | R | C |
| Teredo off | Can lower latency; some Xbox/Game Pass multiplayer needs it (warned) | A | – | C |
| DNS server | **choice** per adapter: Automatic / Cloudflare / Google / Quad9 / AdGuard; the Network page shows it too | B | – | C D |
| Network throttling index off | Multimedia scheduler stops capping network for games | A | R | D |

### System (12)

| Tweak | What it does | Risk | After | Src |
|---|---|---|---|---|
| Hibernation | Off frees the space of `hiberfil.sys` (Disk space shows it) | B | – | C |
| Fast start-up off | Real shutdowns (fixes dual-boot and some driver issues) | B | – | W |
| Detailed blue screens | Shows the driver/stop code details | S | – | C |
| Detailed sign-in messages | | S | – | C |
| Storage Sense | **choice**: On / Off | B | – | C W |
| Reserved storage off | Frees 7-10 GB; turn back on before a big update (Details say why) | A | – | C |
| BitLocker automatic encryption off | Only stops *automatic* encryption on new installs; never decrypts | B | – | W |
| Modern Standby networking off | Laptops: no network while asleep | B | – | C W |
| Clock in UTC (dual boot with Linux) | | A | R | C |
| Settings Home page | | S | – | C W |
| Windows Sandbox | Optional feature (Pro, 22483) | B | R | W |
| Windows Subsystem for Linux | Optional feature (22000) | B | R | W |

### Advanced (9, restore point offered)

| Tweak | What it does | Risk | After | Src |
|---|---|---|---|---|
| Core isolation (memory integrity) off | Some old drivers need it off; **FACEIT and some anti-cheats require it on**, and it protects against kernel malware (Details say both) | A | R | D |
| SysMain (Superfetch) off | Only for HDD-only PCs; hidden when the system drive is an SSD | A | – | D |
| Windows Search service off | Start search gets slow; Deck's own search keeps working | A | – | D |
| Background apps off for all Store apps | | A | – | C |
| Razer / Logitech auto-install blocked | Stops Windows installing their apps with the driver | A | – | C |
| OneDrive removed | Uninstalls OneDrive (your files in it stay on disk); reinstall link | A | – | C |
| Brave debloat | Rewards, Wallet, VPN, Leo off; only when Brave is installed | S | – | C W |
| Edge debloat (policies) | Telemetry, shopping, first-run off; Edge and WebView2 stay | S | – | C |
| Unsigned RDP file warnings off | | A | – | C |

### Built-in apps

Win11Debloat's 139 apps with its rating: 84 safe, 48 optional, 7 unsafe. Only installed ones
are listed. Groups: Microsoft apps (Clipchamp, News, Weather, Solitaire, To Do, Teams personal,
Bing apps…), Xbox and gaming, OEM software (HP, Dell, Lenovo, LG), third-party promos (Spotify,
Disney+, TikTok, Candy Crush…). Edge isn't in the list at all (anti-goal).

## Left out, and why

| Option (from) | Why not |
|---|---|
| Remove Edge (WinUtil, Win11Debloat, WinScript) | Can take WebView2 with it: Deck, Teams, Widgets and many apps run on it |
| Disable Windows Update / security updates only (WinScript) | A PC that can't patch itself; "pause up to" and "no auto-restart" cover the need |
| Defender limits / off | Security; Defender's own settings already exist |
| Services to Manual, ~100 services (WinUtil) | Large, opaque, little gain; single services are offered where they matter |
| Remove Microsoft Store (WinScript) | Reinstalling anything (and Deck's app reinstall) needs it |
| Remove Xbox Identity Provider (in "unsafe") | Game Pass and Xbox-signed-in PC games stop working |
| Run O&O ShutUp10++ (WinUtil) | Another program; Deck covers its common switches itself |
| Run MAS (WinScript) | Activation crack |
| Ultimate Performance plan (WinUtil, WinScript) | The CPU page manages plans; it can add Ultimate there |
| Disk Cleanup / Temp files / Empty Recycle Bin (WinUtil, WinScript) | Clean up already does this; Tweaks links to it |
| Dark mode (all) | Display already schedules it |
| DISM / SFC / network reset (WinScript) | Repairs, not tweaks: a later "Repair" part of Tools |
| IPv6 off entirely (WinUtil) | Breaks Tailscale's IPv6 and some ISPs; "Prefer IPv4" instead |

## With the rest of Deck

- **CPU page** owns Game Mode and power plans: Tweaks shows them read-only with a link.
- **Display** owns dark mode; **Clean up** owns temp files; **Startup apps** owns startup;
  **Network** shows the DNS chosen here.
- **Search box:** every tweak is a search result ("file extensions" → the switch).
- **PC to PC → Setup:** "Tweaks" joins what a setup can carry (ids, applied by the engine).
- **Phone:** nothing (tweaks stay on the PC).

## Files

| File | What |
|---|---|
| `src-tauri/tweaks/tweaks.json` | The recipes (built into the exe) |
| `src-tauri/src/tweaks/mod.rs` | Load, read state, apply, undo, journal, changed-back check |
| `src-tauri/src/tweaks/steps.rs` | reg / service / task / feature / appx / cmd steps |
| `src-tauri/src/tweaks/apps.rs` | Built-in apps: list installed, remove, reinstall |
| `src-tauri/src/tweakscmd.rs` | Tauri commands |
| `ui/js/tweaks.js` | The page |
| `%APPDATA%\Deck\tweaks.json` | The journal (what was there before each change) |

## Stages

1. **Engine + 4 categories:** reg/service/task steps, state, journal, undo, Undo all, changed
   back, after-effects bar, restore point; Privacy, File Explorer, Taskbar, Gaming. Tests: the
   engine against a scratch key (`HKCU\Software\Deck-test\tweaks`, the test profile maps
   every path there), then every recipe live on this PC: apply, read back, undo, and a `reg
   export` before and after must be identical.
2. **The other categories**, feature and cmd steps, build gates checked on Windows 10 and 11
   (the friend's PC or a VM).
3. **Built-in apps:** list, remove, reinstall; restore point first.
4. **Overview cards and presets**, search integration, PC to PC Setup.

Each stage: clippy clean, tests, UI regress/a11y, installed and checked before the next.

## Open questions for review

1. Are any tweaks above unwanted, or missing ones you use?
2. Presets: are Privacy, Clean Start & taskbar, Gaming, Quiet Windows the right four?
3. Should "Recommended for this PC" pick only **safe** tweaks, or also suggest behaviour ones
   (like "Explorer opens to This PC")?
