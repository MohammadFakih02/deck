# Deck feature audit — 2026-10-12 (second pass)

> Screenshots and JSON files mentioned below are kept on the maintainer's machine and are not in the repo.

This pass looked at what Deck's features **do**, not only how they look. It also removed MicDrop from Deck at the user's request. Screenshots are in `features-2026-10-12/` (`before/`, `after/`, and `sweep/` with every page at 1260×900). Everything was rendered from Deck's own page against fake data; the real screen was never captured.

## How it was checked
- **Rust unit tests:** `cargo test --release` went from 55 to 69 passing. Each new test failed, or could not pass, against the old code.
- **UI regression script:** `tools/testing/ui-preview/regress.mjs` sends real mouse and keyboard events to the UI in headless Edge, against the fake backend. It went from 17 checks to 26, all passing. The new `toast-test.mjs` does the same for the notification window (6 checks). The new checks fail on the code from before this session.
- **Accessibility and layout sweep:** `a11y.mjs` across all 33 page views at 1260×900 and at 900×600 found 0 unnamed controls, 0 small targets, 0 sideways overflow and 0 script errors.
- **Read-only probes:** a temporary calculator probe (about 70 expressions, conversions and number bases).
- **Not done:** no live hardware writes, no real shutdowns or sleeps, no real cleanup, nothing in a game. The installed build was checked live only as described in "Live check" below.

## Removed: MicDrop inside Deck
- **Removed:** the Home tile, its setting, the WebSocket client (`micdrop.rs`, kept in the session scratchpad), the `micdrop_send` command, and the `micdrop` field in the live data.
- **Old settings still load:** `micdrop_tile` and a saved `"micdrop"` tile are ignored.
- **Kept:**
  - MicDrop stays on Game boost's never-close list; it is an audio tool that may still be running.
  - Deck still never changes the recording device.

## Defects found and fixed

| # | Area | What went wrong | Cause | Fix | Proof |
|---|---|---|---|---|---|
| 1 | Clicks (all pages) | Clicks were still lost on the sidebar, and on any page redrawn by a backend event (network ping every 5 s, speed test, timers, clipboard, disk, cleanup, boost) | The earlier fix covered only the 1-second tick. The sidebar was rebuilt every second even when nothing had changed, and event handlers called `render()` directly. | `render()` and `renderSide()` wait while a mouse button is held and run once it is released; an unchanged sidebar is not rebuilt | regress: "sidebar click is not lost to a tick" and "click is not lost to an event-driven redraw" fail on the old code and pass now |
| 2 | Shut down when… | "PC goes idle", "network goes quiet" and "downloads finish" read CPU and network figures **frozen at the moment Deck was hidden** | `sys.snapshot()` (the only sampler) runs only while the page is open or a phone is connected | `cpu_now`/`down_now` take a fresh light sample when the last one is more than 2 s old | `sys::tests::readings_stay_fresh_while_deck_is_hidden` |
| 3 | Shut down when… | "Shut down when downloads finish" shut the PC down 2 minutes after you set it if **nothing was downloading yet** | No "a download was seen" state | The plan waits until it sees a download, then needs the quiet time ("Waiting for a download to start") | `power::tests::downloads_must_start_before_they_can_finish`, `a_new_download_resets_the_quiet_time` |
| 4 | Shut down when… | "Sleep when music stops" fired even if nothing had played | Same | Waits until something plays | `music_must_play_before_it_can_stop` |
| 5 | Shut down when… | A time plan missed while the PC was asleep or off ran right after wake: "Shut down at 23:00" hit the PC the next morning | No lateness check | More than 10 minutes late: skipped, with a toast saying why | `a_time_missed_while_asleep_is_skipped` |
| 6 | Keep awake | "Until downloads finish" switched itself off within 5 s if no download had started yet, and between two files | Same | Off one minute after the last download it saw ends | logic shared with #3; covered by the same tests' rules (not a separate test) |
| 7 | Keep awake and Shut down when… | They used different "is downloading" limits (300 vs 150 KB/s) | Two copies of the rule | One `downloading()` | code |
| 8 | GPU and CPU crash guard | **Quitting Deck by hand disarmed the guard**, but the overclock stays on the card until reboot. A crash after that re-applied the risky overclock at the next boot. | `mark_clean` ran on every exit | Only a Windows shutdown, restart or sign-out marks it clean; a hand quit keeps it armed, and the toast explains which case happened | `gpu::guard::tests::closing_deck_keeps_the_guard_armed` |
| 9 | CPU profiles | Race: the 2 s plan watcher could act on the in-between plan during a re-apply (wrong profile marked active or wrong extras), and two switches could interleave | An atomic flag that was checked once, not held | One lock held for the whole switch, save or delete; the watcher skips while it is held | reasoning plus the existing tests (races are not unit-testable here) |
| 10 | RTSS overlay | Race: quitting could leave frozen numbers in RTSS | The loop checked "stop", then wrote; `stop()` could clear the slot in between | A write lock shared by the loop and `stop()` | code |
| 11 | RTSS / Afterburner | Two programs wrote the same MAHM block when Afterburner started **after** Deck | The check only ran when the mapping was created | Deck steps aside as soon as Afterburner is running | code |
| 12 | Search | Enter right after typing ran the **previous** query's result: "2+23" plus a fast Enter copied "4" | Previous backend results were kept while the new ones loaded | Stale answers are dropped, and Enter waits for the current query's results | regress: "Enter after typing uses the new query's answer" |
| 13 | Search | No typo tolerance ("dsicord", "steeam" found nothing); games came in install order, not best match; a Steam game could appear twice | Prefix and in-order matching only; no sort; no de-duplication | One swapped or extra letter is tolerated (always ranked below real matches); games sorted and de-duplicated against apps | `files::tests::one_typo_still_finds_the_app` |
| 14 | Search | "kill / close / end <name>" ended a program on Enter with no question. "end note" would end OneNote. | No confirmation | Same confirmation as the Processes page | code (dialog) |
| 15 | Calculator | "200 + 10%" gave 200.1 | `%` was always "/100" | `a ± b%` adds or subtracts b percent of a, like a desk calculator; "10% of", "a*b%" and modulo are unchanged | `calc::tests::math` (4 new asserts) |
| 16 | Clean up | **Recycle Bin was ticked by default under "Safe to clean"**, so one click emptied it for good, and the confirmation didn't say so | Default selection | Unticked by default, in its own group "Your deleted files (gone for good)"; the confirmation names it and turns red when it's included | regress: "clean up: Recycle Bin is not ticked or called safe by default" |
| 17 | Network page | The graph was empty when the page opened, its scale ignored the current rate (axis 0–4 while showing 6.8 Mb/s), and coming back later joined old points to new ones | History lived only in the page, from the first visit | Starts from the backend's last minute (new `up_hist`), includes the current value in the scale, and starts again after a gap | screenshot `after/network.png` |
| 18 | Layout at 900×600 | Timers and Phone scrolled sideways (47 px and 95 px) | `1fr` grid columns cannot shrink below their content | `minmax(0, 1fr)`; stacks under 1000 px; the Phone button row wraps | a11y sweep at 900×600: 0 overflow |
| 19 | Notifications (toast window) | A toast under the mouse still timed out, and when one above it went, the rest moved under the pointer. The measured shift was 97 px, and the click hit nothing. | Removals and new toasts reflowed the stack and resized the window immediately | While the mouse is over the stack, removals and new toasts wait until it leaves (at most 15 s). The shutdown warning never waits. A toast you click yourself still closes at once. | new `toast-test.mjs`: 3 checks fail on the old `toast.js`, 6/6 pass now |
| 20 | Shut down when… | A plan was lost when Deck restarted (update, crash, quit) | Kept only in memory | Saved to `plan.json` whenever it changes, and restored on start as "waiting", which means a fresh 60 s warning and the download or playback has to be seen again | `power::tests::a_restored_plan_starts_waiting_again` |
| 21 | Game detection | On a PC with an auto-hiding taskbar (this one) **any maximized window counted as a game**. That affected the CPU in-game clock check, the lighting "Game" preset (it reacted to the wrong program's audio) and the `in_game` flag. | `hw::game::foreground_game` had no title-bar test (the hot corner's own check had one) | One shared rule, `covers_like_fullscreen` (covers the monitor **and** has no title bar) | `hw::game::tests::a_maximized_app_is_not_a_game` |
| 22 | Clipboard paste-back | If the previous window had closed or Windows refused to bring it back, Ctrl+V went to whatever was in front, which could be a game | Pressed Ctrl+V without checking | Ctrl+V only if that exact window is in front again and it isn't fullscreen; otherwise the item stays on the clipboard and a toast says "Press Ctrl+V" | code (needs the real desktop to exercise) |
| 23 | Clipboard | Text over 1 MB was cut to 1 MB and kept, so pasting it back was silently incomplete | Truncation | Text over 1 MB isn't kept | code |
| 24 | Clipboard | Image history had no limit of its own: up to 400 PNGs in `%APPDATA%` on a nearly full C: drive | Only the overall item limit | At most 150 unpinned images. This PC has 90 (19 MB), so nothing is deleted now. A lower limit would delete images, which needs the user's OK. | `clipboard::tests::history_keeps_its_limits_and_pins` (also caught a counting bug in the first version) |
| 25 | Workspaces | The toast always said every window was placed, even when an app didn't open or was uninstalled. Windows saved on a monitor that's no longer there were moved off-screen. | No result tracking; no monitor check | The toast says which apps couldn't start or didn't open in 20 s ("partly ready"). A window whose saved spot is on no current monitor stays where it is. | `winmgr::tests::workspace_toast_says_what_went_wrong` |
| 26 | Phone uploads | No size or free-space limit: a big upload could fill the C: drive | No check | Stops, and deletes the partial file, before the drive would drop under 2 GB free | `util::free_space_tests::free_space_is_read` (the upload path itself is compile-tested only) |
| 27 | Phone "keep awake" | It did not reset the download tracking the way the PC side now does | Duplicate code path | Same reset | code |
| 28 | Workspaces | The "apps to close" were ended outright, losing unsaved work | `kill_name` | They are asked to close like clicking X; after up to 8 s only apps without a window are ended; apps still showing a window ("save changes?") are left open and named in the toast | `winmgr::tests::closing_matches_the_exe_file_name` (matching); the close itself needs the real desktop |
| 29 | Display | A refresh-rate change went straight to the driver | No test first | `CDS_TEST` first; a mode the monitor can't show is refused with a clear message | compile only (changing the display is not allowed in this session) |
| 30 | Search | Keep awake for a time couldn't be typed (only "sleep in 30m"), and a set plan couldn't be cancelled from search | Missing commands | "awake 2h" / "keep awake for 90m", and "cancel shutdown / sleep / plan" when one is set | regress: "search: \"awake 2h\" keeps the PC awake 2 hours" |
| 31 | Game boost recovery | After a Deck crash with Full power on, `boost.json` was deleted **before** the plan was put back (which can wait up to 25 s). A second crash in that gap left the PC on the gaming power plan for good. | Order of operations | The file is removed only after everything is restored (and not if Full power was turned on again meanwhile) | code |
| 32 | Startup apps | Apps that start for all users (HKLM Run, Run32, the all-users Startup folder) were always read-only, though Deck normally runs as administrator and Task Manager can switch them | Hard-coded `can_change: false` | When Deck is elevated those rows get the same on/off switch, written to the matching all-users StartupApproved key; without admin they stay read-only and the backend refuses. Unknown ids are refused rather than guessed. | `startup::tests::each_entry_maps_to_its_own_switch` (no real entry was switched) |
| 33 | Disk space | The trash button worked on anything the scan showed: `C:\Windows`, Program Files, the Users folder, `pagefile.sys`, even a drive root. Moving those to the Recycle Bin breaks Windows. | No guard | `disk::protected()` refuses drive roots, the Windows tree, the Program Files / ProgramData / Users / profile / AppData folders themselves, and root system files. Those rows show a lock instead of a trash button, and the backend refuses them anyway. | `disk::tests::system_folders_are_never_recycled`; regress "disk space: system folders get a lock"; live: three refusals over CDP |
| 34 | Disk space | "Move to Recycle Bin" used `FOF_NOCONFIRMATION`. When something can't go to the bin (bigger than the bin, or a drive without one), Windows then **deletes it for good without asking**, while Deck's dialog promised it could be restored. | Missing `FOF_WANTNUKEWARNING` | Windows now asks before a permanent delete (Cancel leaves the file). The confirmation says so. | code (deleting real files is not allowed in this session) |
| 35 | Disk space | After a delete, folder sizes updated but file counts did not | `forget()` only subtracted bytes | File counts drop too | code |
| 36 | Sound | `set_output` (also reachable from the phone) accepted any device id, including a microphone's. A name switch matched an empty name, or picked one of two "Speakers" at random. | No check | Only a connected playback device is accepted. A name must match one device exactly or uniquely, otherwise it's refused. | `audio::tests::output_names_match_one_device_or_none`; live: a made-up id is refused |
| 37 | Screen tools | Copy-text on a wide box (a strip across two screens) failed with a cryptic error: the 2-3x upscale went past Windows OCR's size limit | Fixed scale | The scale steps down to fit. A box too big even at 1x gets "drag a smaller one". The box is clamped to the captured screen. | code |
| 38 | CPU/GPU profile editors | Alt-tabbing away (Deck hides) closed the editor. Unsaved edits were lost, and a GPU "Try it" was undone at once, so you couldn't try a profile in a game | Deck's hide event closed every dialog | Profile editors stay open while Deck is hidden, with the trial still on. Coming back puts the focus in the editor. Plain questions (confirmations) still close. | regress: "alt-tab keeps an open profile editor and its trial" and "both profile editors are kept while hidden" (both fail on the old UI) |
| 39 | Display: dark mode schedule | With the schedule on, the theme was forced every minute. Switching to light by hand (even with Deck's own Home tile or Display switch) was undone within a minute. Times like "25:00" were accepted. | The schedule compared the current theme, not the period | The schedule acts only when it reaches a switch time, or right after Deck starts or the schedule is edited, like Windows' Night light. A manual switch holds until the next switch time. Impossible times are ignored. | `display::tests::a_manual_theme_switch_holds_until_the_next_change`, `times_must_be_real` |
| 40 | Processes / Game boost | "End" was blocked only by a list of names. Deck runs as administrator, so it could end a process Windows marks **critical** that wasn't on the list, which is an instant blue screen. | Name list only | Before ending anything (Processes page, search "kill", Game boost), Deck asks Windows whether the process is critical (`IsProcessCritical`). Critical, or unknown, means it is not ended. Critical processes also lose their End button in the list. | `sys::tests::processes_windows_needs_are_never_ended` |
| 41 | Windows: see-through | Setting a window back to 100% left the `WS_EX_LAYERED` style Deck had added. Layered windows draw the slow way: no VRR and no fullscreen optimisations, e.g. on a game you made see-through for a moment. | Style never removed | Deck remembers which windows it made layered and removes the style at 100%. Windows that were layered by their own app are left alone. | live: a Notepad window went 255 → 128 → 255 and its extended style ended as `0x110` (no `0x80000`). Notepad was then closed normally; its session tab was saved. |
| 42 | Phone page | With the phone connection off, the card still showed "No phone connected" and "Make a new code" | Shown regardless of state | While it's off, the card only says it's off | screenshot `after/phone-off.png` |
| 43 | Sound page, Home | Media without album art showed a play triangle as its picture, next to the real play/pause button. A playing track looked paused. | Placeholder icon | A music note (new `music` icon) | screenshots `after/live/sound.png` |

## Redesigned: Keep awake and Shut down when…
Both pages were a narrow card in a big empty panel, and the buttons on Keep awake did the same thing twice. Now:
- **Keep awake:**
  - A status card shows a live countdown and progress bar, with "+30 min" and "Turn off".
  - Durations are one-click tiles that start it (or switch it) immediately. There is also a custom minutes field, "while an app runs", and the screen option with a plain explanation.
  - The side column shows **what Windows would do without Deck** (sleep and screen-off times, read-only from the active plan) and links to related pages.
- **Shut down when…:**
  - An action row, then condition cards that show **the live reading** ("7.4 Mb/s down now", "CPU 23% now", "Playing: …").
  - The details the chosen condition needs, and a plain sentence of what will happen ("Sleep in 30 min").
  - Once set: a status card with a countdown or progress, and Cancel.
  - Quick picks sit in the side column and replace the current plan.
- **Proof:**
  - `before/` and `after/` screenshots.
  - regress: "keep awake: one click on 1 hour starts it", "Turn off stops it", "builder sets the plan it shows", "Cancel clears it".

## Checked, no change needed
- **Game boost lists:** NVIDIA Broadcast, Docker, launchers, anti-cheat, voice and music are excluded or never pre-ticked in the real lists. The preview only showed them as "Suggested" because of fake data.
- **Processes "End":** asks first.
- **Calculator:**
  - "1/0", "0/0", "171!" and "sqrt(-1)" give no answer rather than a wrong one.
  - "sin(90)" is in degrees, consistently.
  - Units and temperatures are correct.
  - "1,5" is read as 15 (thousands separator). That is a known ambiguity, left as is.
- **Undervolt "risky" rule:** a raised point at 850 mV or above counts as risky. Classic undervolts that raise mid-voltage points are therefore held back after a crash. This errs on the safe side, so it is unchanged.

## Live check and one mistake
- **First install (16:15):** quit measured at 2.1 s. Checked over CDP:
  - Keep awake and Shut down when… with real readings ("sleeps: never, screen off: 10 min", "409 kb/s down now", "CPU 13% now").
  - MicDrop gone from the Home tiles.
  - The Network graph full on open.
  - Screenshots are in `after/live/`. Deck was then restarted normally; port 9222 closed.
- **Second install (16:37) was a mistake.** The window list was printed in the same command as the install, so it wasn't read first, and **a game was running**.
  - The restart re-applied the same CPU/GPU profiles and handed the fans to the BIOS for about 2 s while Deck quit.
  - One read-only CDP check followed, then a normal restart (port 9222 closed), and no further restarts.
  - A memory note now requires the game check as its own step.
- **Third install (19:20), after the game had closed:** the window list was checked on its own first (no game). Quit measured at 2.1 s again. The installed build now has every fix, #1–#32. Checked over CDP:
  - **Toast window:** a 4 s timer toast rendered (`after/live/toast-timer.png`). With the mouse over it, a second timer finished: the new toast was held and the first toast's OK button did not move (70.48 px before and after). On mouse leave the held toast appeared. Both were closed with their own OK buttons and no timers were left.
  - **Startup apps:** the all-users entry "Windows Security notification icon" is now switchable (`can_change: true`); nothing was toggled (`after/live/startup-admin.png`).
  - Deck was then restarted normally; port 9222 closed.
  - `cdp.mjs` gained `CDP_PAGE=toast` to drive the notifications window.
- **Fourth install (19:40), no game open (checked on its own first; VS Code and Outlook were open):** quit 1.5 s. Over CDP, `recycle` refused `C:/Windows`, `C:/` and `C:/Users`, and `set_output` refused a made-up id; nothing was changed. Sound page live: the music icon shows for a track without art (`after/live/sound.png`). Restarted normally; port 9222 closed.
- **Fifth install (19:49), no game open (checked on its own first):** quit 2.1 s. The GPU profile editor was opened over CDP (Try it not pressed, nothing sent to the card). Then Deck's real `hide` command was invoked, which sends the same `hiding` event as alt-tab. The editor was still open afterwards (`after/live/gpu-editor-kept.png`). It was removed by hand. Restarted normally; port 9222 closed.
- **Sixth install (20:00), no game open (checked on its own first):** quit 3.1 s. See-through check above (#41). Restarted normally; port 9222 closed.
- **One unplanned restart (about 20:09):** a build command also had `restart-normal.ps1` chained after it by mistake. The build failed (a locked file; the next try passed with no change) and Deck was restarted normally anyway. The window list taken a minute earlier showed no game (VS Code, FluxDM, Windows Setup), so no hardware state changed hands mid-game. Builds and restarts are separate steps again.
- **Seventh install (20:11), no game open (checked on its own first):** quit 2.1 s. This build has every fix, #1–#43. Phone card checked live (connection on: QR, chip and button shown). Restarted normally at 20:11; port 9222 closed.

## Not verified / remaining
- **Hardware:** the CPU lock and RTSS changes are reasoned fixes for races. They were compiled and unit-tested around, not reproduced live.
- **Overlay in a real game:** still not seen (unchanged from before).
- **Quit time:** the "can block up to 20 s" note has no recorded source. Every worker sleeps at most 1 s, so the likely suspects are boost restore and the session-end path. Not measured.
- **NVML opened in three places:** left as is, on purpose. NVML's init and `LoadLibrary` are both reference-counted, so the three users share one loaded driver library. Merging them would rewrite working hardware paths for no change you could see.
- **Code-level passes done:** Clipboard, Workspaces, phone remote (#22–#28), Startup (#32), Disk space (#33–#35), Sound (#36, #43), Screen tools (#37), Display theme schedule (#39), Processes (#40), Windows (#41). **Not yet:** Lighting and Cooler display beyond the layout and accessibility sweep. The OCR and colour picker capture the screen by design, so they were read, not run.
- **Sound:** a track from a WebView2 app (YouTube Music) is labelled "Web app": Windows reports only `msedgewebview2.exe` as the source.
- **Compatibility on other PCs:** no new machine-specific assumptions were added. The new Windows-timeout read returns "Couldn't read" instead of failing.

## Next, in order
1. Check paste-back live (it sends Ctrl+V, which this session may not do).
2. Live-check the Workspaces graceful close and the refresh-rate test on the real desktop.
3. With you present: delete one throwaway file from Disk space that's bigger than the Recycle Bin, to see Windows' permanent-delete warning (#34). Then toggle one harmless all-users startup entry and back (#32).
4. Measure quit time with Game boost on (the remaining suspect for the old "20 s" note).

## Asked for after the audit (same night)
- **Notes removed** (page, Home tile). Saved notes are left in `settings.json`, not deleted.
- **Windows** now holds Snap zones and Workspaces as tabs; **Tools** holds Screen tools, Locked files and Windows panels.
- **Network ping** has Start and Stop. It pings once when the page opens, and live only while started.
- **CPU and GPU:** a "?" next to every setting explains what it does, why you'd use it and what changing it does, with a warning where there's a risk (overclocks, offsets).
- **Overlay font list** showed only the current font (a suggestion list filters by what's typed). It's now a dropdown of every installed font. Verified live: 280 fonts.
- Screenshots: `features-2026-10-12/merge/`. Tests: cargo 76, regress 34/34, toast 6/6, a11y 0.
