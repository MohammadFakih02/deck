# Deck — project state

The dated history of the work. Last updated: 2026-10-22.

**Start with `DECK.md`** (full overview: features, hardware, files, build/test, status). This file is the dated history.

> **RULE: before every commit, update the docs.** Add today's dated entry here, add user-visible changes to `CHANGELOG.md`, update `DECK.md` / `README.md` when facts change, and stage them in the same commit. The pre-commit hook enforces it; never bypass it. No personal data in these files (that goes in the git-ignored `PRIVATE_NOTES.md`).

## 2026-10-22: v2.8.1 — process safety checks, uninstall reports, safer leftovers, death counter scope

Released at the user's request as **2.8.1**.

- **Process safety checks** (new `filecheck.rs`, commands `filecheck_state/start/stop/key` in `syscmd.rs`, UI `checksBar`/`safetyCell`/`risk` in `pages2.js`, `.sc`/`.safety`/`table.grid.procs` in `app.css`). Four optional checks, each a "check all" job on its own thread (`JOB`, `STOP`; progress through `changed("filecheck")`, throttled in `app.js`): signatures (one `Get-AuthenticodeSignature` PowerShell for all paths, read from a temp file; PS 5.1 must get `[string]$_` or the path serializes as an object), AV engines (Team Cymru Malware Hash Registry: `DnsQuery_W` TXT on `<sha1>.malware.hash.cymru.com`, "<time> <percent>"; NXDOMAIN = not known; new windows-sys feature `Win32_NetworkManagement_Dns`), Defender (`MpCmdRun -Scan -ScanType 3 -File -DisableRemediation`, newest Platform folder first; exit 0 clean, 2 threat), VirusTotal (user's key, sealed via `secret::field`; v2 `file/report` with 4 resources per request every 15 s, falls back to v3 one at a time on 400/404/410; 204/429 waits a minute once, then stops as the daily limit; transport errors retried 3× 20 s apart; order: bad signature, unsigned, unchecked, signed). Fingerprints (SHA-256 + SHA-1) cached per path by size and mtime; results per SHA-256 in `%APPDATA%\Deck\filecheck.json`. Fixed column widths for the table (Memory was cut off), one-line badges.
- **Uninstall** (`programs.rs`): MSI products are removed by `msi_remove` with `/L*v` into `%APPDATA%\Deck\backups\uninstall`; `installer_busy` (Installer\InProgress) refuses to start; `explain` turns exit codes and the log's first `Error NNNN` into words; 1601 restarts msiserver once; 1603 with a 1721/1722 custom action whose exe is missing gets `stand_in` (a copy of deck.exe plus `<exe>.deck-stand-in`; `main` exits 0 at once when that marker exists) and a quiet retry, folders it made removed after. `run_and_wait` returns the exit code; `uninstall` returns `Outcome { gone, detail }`. Every uninstall records `BEFORE` on disk, and the plain Uninstall now ends in the leftover list too; `programs_do` calls `show_page("programs")` when the uninstaller ends (its window took the focus and Deck hid); result boxes use `keep: true`.
- **Leftovers** compared with Bulk Crap Uninstaller's confidence rules (Apache-2.0; read, not copied). `company()` strips company forms; publisher names (this app's and every installed program's) never count as an app's name; a recorded install folder is ticked only when named after the app (digits ignored, the publisher prefix and language tags dropped) or when its uninstaller is inside it, otherwise offered unticked with the reason; publisher subfolders in Documents are unticked. `scan_for` split out of `scan`; ignored test `dry_run_leftovers_here` prints what the search would offer for every installed program (looked only). The leftover dialog is `wide`, paths wrap.
- **Death counter**: `boost::not_a_game` also takes every browser Windows registers (`Clients\StartMenuInternet\*\shell\open\command`, read once) and more browsers, players and chat apps in `NOT_GAMES` (a fullscreen browser not on the list was counted as a game and its screen read). New setting `deaths.watch_in` ("known" default / "all"): the screen watcher starts only for catalogue games, games with taught or shared prints, or games with deaths counted.
- **Checked**: `cargo clippy --all-targets -D warnings` clean; `cargo test --bin deck` all pass except `rtss::tests::an_elevated_deck_never_runs_the_helper_from_a_user_folder`, which fails on the maintainer's PC before these changes too; `regress.mjs` and `a11y.mjs` pass. In the real app: Signatures, AV engines, Defender and VirusTotal ran over all running programs (VirusTotal measured at 16 files a minute with the batching); the stand-in idea was proven by hand on a real MSI whose folder had been deleted (error 1721: a stub exe at the missing path, quiet retry succeeded), while Deck's own `stand_in` is covered by a unit test only; several MSI uninstalls ran through `msi_remove` and reported success; the leftover dry run before/after the new rules was compared item by item. Not tried: a 1601 restart, the v3 fallback, a Defender threat.

## 2026-10-22: v2.8.0 — merged with 2.7.9, update progress, safer leftovers

Released at the user's request as **2.8.0** (tag `v2.8.0`, built and published by `release.yml`): the friend's 2.7.9 (5 commits: Apps page, Store app updates, phone controller page, Workspace Sync, TLS crates) merged with the local work below (committed first on `merge-2.8`, then `origin/main` merged in). Conflicts: `rtss.rs` (`start_rtss`: their `keep_out_of_hooking()` + our log lines), `tweaks/apps.rs` (their 404-aware Store lookup through our `log::http_agent`), `PROJECT_STATE.md` (both entries kept). The CHANGELOG auto-merge put our Unreleased lines into 2.7.9's section; moved to 2.8.0.

- **Update progress** (user request): `Status.got` / `size` (Content-Length; 0 when missing); `download` reads in 64 KB pieces and sets `got` every 250 ms (`PROGRESS_EVERY`); each `set` emits `changed("update")`. UI: `app.js` handles the `update` topic (the page only polled every 1.5 s before); `updateProgress` in `pages3.js` moves `#upd-bar` / `#upd-pct` in place, a new state redraws the card. Unknown size: `.bar.indet` stripe (a still bar under reduced motion).
- **Install button back after Check now** (user report): `check_now` set "checking" unconditionally, so `apply_found`'s keep-while-installing test saw "checking" and set "available". It now returns the current state while `BUSY` (an install runs).
- **Review of 2.7.9** (user: "try to fix anything you find"): `programs::scan` could offer a personal folder as "Its install folder", ticked (`disk::protected` covers Windows' folders, not Documents/Desktop/Downloads or a drive holding them). `personal_places` (home, its usual folders, the known folders via `util::personal_folders` also when redirected, AppData roots, Start menu roots, Public) is refused in `scan` and again in `recycle`, also for a folder that holds one; finds in Documents and drive-root install folders are unticked (`on_drive_root`). `msi_uninstall` sliced `cmd` with an index from its lowercase (a panic when lowercasing changes a byte length); guarded. `VPad::new` (sleeps up to ~2 s for the player number) ran on the phone server's async threads; `new_pad` uses `spawn_blocking`. Test `your_own_folders_and_what_holds_them_are_never_leftovers`.
- **Checked** (-j 6, low priority; Warframe and League open): `cargo clippy --all-targets -D warnings` clean, `cargo test --bin deck` 476 passed, 0 failed (51 ignored); `regress.mjs` all pass; the Updates card shot in three states (35% · 3.2 of 9.3 MB, starting, size unknown) and `updateProgress` moved it in place (91%).

## 2026-10-21: logging across Deck, overlay key timing

- **Overlay key late in Helldivers 2** (user report): the key wrote the text at once, but RivaTuner's own OSD switch (off while Deck's overlay is off) was only flipped by the `rtss` loop after its next `snapshot()` (NVML calls, the sensor tables' locks) and after starting and waiting for `rtss-osd32.exe`; the 64-bit `SetFlags` came last. Now `toggle` calls `follow_switch` (shared with the loop under `SWITCH`, reading `osd_on` under that lock so quick presses can't end out of order) right after `refresh_now`, and `set_rtss_flag` sets the 64-bit hooks before running the 32-bit helper. A per-press trace (`trace_key` from `hotkeys.rs`: how the key arrived, queue wait, each step's ms) and a launch line (`frame_loop`: how old a game was when RivaTuner listed it, when its first frame came) go to the log, to confirm in the game. Not yet built or tried in a game.
- **Logging** (one pass over the whole app, user request): `log.rs` rewritten. Levels error/warn/info/debug (debug via a `debug` file in the log folder or `DECK_LOG=debug`); `time.ms [level] [thread]` lines, a role prefix for helper processes (`set_role`); masking at the one door (`redact`: secret key=value patterns, `Bearer`, the user's profile path; `support::private` now uses it); repeats within 60 s counted, not written; a background writer (`flush` on crash, quit and `exit_logged`; enqueue waits at most 500 ms so a crash handler can't hang; a full queue counts dropped lines; failed writes are reported once writing works again); `Op` for start → outcome with an id and duration (a dropped `Op` logs "ended without an outcome"); `log::every` for once-in-a-while lines; `http`/`http_agent` (ureq middleware: method, host and path, status or the error chain) on every web request. Instrumented: startup line (`session_start`), panics/native crashes as `error`, quit and hardware hand-back, settings recovery, safe mode and hardware held, GPU engine caps, resume re-apply, drift, fan-curve failures and `gpu::release` results (hardware first, log after), hardware engine start/stop and fan chip, worker panics seen at join, profile switches (`Op`), update check/install (`Op`), RivaTuner start and switch failures, hotkey registration (busy keys), phone remote (port, stop, connections, turned-away phones, refused signatures by path only, failed actions by name), death counter (watching start/stop, capture recovery, each counted event and its source, unreadable history lines, failed picture saves), Game boost and Clean up results, power actions (logged and flushed before shutdown/sign-out), startup-app switches, display changes, Tweaks rollbacks that themselves fail (`error`), page reloads by the webview watchdog, controllers found/gone/low (by name, never the MAC or path), Screen view encoder choice and duplication fallback, Store downloads and disk scans (`Op`), share links (made/downloaded/ended, by creation time, never the token), `run_for` (spawn failures, non-zero exits with the first stderr line, once per 10 min per program and code), `launch_as_user` retries. UI: `core.js`'s `invoke` logs failed commands by name (never arguments) through the new `ui_log` command, plus `error`/`unhandledrejection`; the toast window too. Transfer's `log.txt` was unbounded: now 2 × 2 MB. Transfer and Claude Sync copy their lines into Deck's log at debug. Support report: last 80 lines, debug state and log folder. New tests in `log.rs` (masking, URL stripping, repeats, dropped lines, unwritable folder, `Op` outcomes, error chains, `every`). Not built or tested yet (the user asked for no build).
- **Performance run** (installed 2.7.5; a game, then the game plus Cinebench R23 multi-core at 100% CPU; per-process and per-named-thread CPU sampled every 2 s): Deck and its webviews used 0.16% of the whole CPU idle, about 0.45% in the game, and the same at 100% CPU; no Deck thread was seen waiting for CPU. Per thread in the game: `deaths-screen` 13 ms/s (35 under Cinebench, likely E-cores or a shared core), `pad-overlay` 16, `rtss` 15. One unnamed thread (the OCR pool) used up to 0.43 of a core for about 45 s right after the game started, with `deaths-screen`'s own CPU tripling in that minute. Likely cause, from the code: a bar with nothing to read that flickers (in and out within 3 frames) starts naming over and over, 6 reads × up to 6 OCR passes each time. Fix: `boss.rs` remembers a spot whose bar had no name (`Quiet`); a bar there waits before naming (2 s, doubling to 16 s, forgotten 2 min after the wait) and gets 2 reads, and the fight still starts from the first sighting. Tests `a_flickering_bar_with_no_name_is_read_less_and_less`, `a_boss_in_a_quiet_spot_still_opens_its_fight_from_when_its_bar_showed_up`. The watch loop logs its text reads per minute at debug, to confirm. Memory went from 141 to 199 MB when the game started (capture, OCR), then about +1 MB/min over 9 min. A 30-minute run with the game open: private 213 → 221 MB, threads and handles flat, working set trimmed from 94 to 49 MB (no leak seen), but the OCR-pool bursts came back through the whole run (0.3 of a core at worst), so the quiet wait now applies to any spot outside a fight (an enemy's bar moves), not only the one that failed (`quiet_now`; test `a_bar_with_no_name_that_moves_is_waited_for_too`).
- **Boss names** (`clean_name`): `hud_text` drops reads with a game-message word (`HUD_WORDS`: "Bot Drop Detected" in Helldivers 2), `garbled` drops words with a dot between letters or a case flip plus a digit ("Ei.atL1"), `plain_accent` folds a single accented letter in an otherwise plain name ("Mutatéd"). Test `hud_messages_noise_and_stray_accents_are_not_names`.
- **Smaller costs**: the controller overlay's clock is 4 Hz while hidden with no controller (was 20 Hz); the `rtss` loop parks until `wake_loop` (from `refresh_now` and `stop`) instead of a 25 ms `WAKE` check, and logs one reading's duration at debug every 10 min, to see what its 8–15 ms/s goes to.
- **Deaths → Now, laid out again** (user: "looks very bad"): `.dc-big` was defined twice in `app.css` (the boss-bar picture's black box and border landed on the big numbers); the numbers are `.dc-num` now. The game line (on/off, name, last tries) is the top of the main card, then the fight (an eyebrow line, the boss or "Between bosses", both counters), the buttons, and the fight controls under a divider; the session is one card laid out like Records (`sessionNow`) instead of four tiles that sat flush against Records; "deadliest boss" in Records says "deaths". Checked in the preview (`shot.mjs`, wide and narrow, fight open and not).
- **Checked, everything in this entry together** (-j 6, low priority, a game open): `cargo test --bin deck` 457 passed, 0 failed (46 ignored); `cargo clippy --all-targets -D warnings` clean (one `allow(clippy::result_large_err)` on `log::http_logger`, whose signature is ureq's); `node regress.mjs` 41 PASS. The first compile caught one slip from the logging pass (`rsplit('\')` written as `rsplit('')` in `rtss.rs`'s launch line). Not yet tried in a game: the overlay key trace, the launch line, the quiet naming wait and the new Now tab in the real app.
- **PC name out of the log**: `log::private` also replaces COMPUTERNAME, whole word and any case, with `<pc>` (`replace_word_ci`; a Transfer line had it). Test `the_pc_name_is_taken_out_as_a_whole_word`; `cargo test log::` 15 passed.
- **Overlay key still late now and then** (user report, from the earlier runs): the key lines logged at 21:21 local (Roblox, 11 presses) all finished Deck's part in 48-61 ms, the 64-bit switch within 2 ms of the press, and the presses came 150-200 ms apart (pressed again because nothing showed). So the wait is after Deck: RivaTuner or the game. `watch_drawn` (thread `osd-drawn`) now logs, after an "on" press, when the game in front took the overlay: it polls the game's `dwOSDFrame` (app entry offset 332, SDK header) until it reaches the global frame number Deck's write set (header offset 32), up to 5 s, with the game's frame time. Also: the 32-bit helper exits 1 for "changed", which Deck logged as a failure on every press; 0 and 1 are both success now. Not yet seen in a game; Roblox's anti-cheat may keep RivaTuner out entirely, so check in Khazan.
- **Not covered by the pass** (failures there still reach the log through the UI's failed-command line): audio, media, apps, files index, clipboard (never logged: private), window tools, picker, calculator, games list, locksmith, MFT reader internals, sensor reads (`hw/*` drivers), lighting devices, Transfer and Claude Sync internals (their own logs), the death counter's OCR/boss reading internals.

## 2026-10-21: v2.7.9 — Apps page, Store app updates, restart-safe transfers, phone controller page

Released as **2.7.9** (tag `v2.7.9`, built and published by `release.yml`), together with the two entries below.

- **RivaTuner:** `open_rtss_window` posts WM_CLOSE instead of `taskkill /F` (a killed RTSS leaves its hooks in hooked processes; Deck's WebView2 hung). `keep_out_of_hooking` writes `deck.exe.cfg` and `msedgewebview2.exe.cfg` with `EnableHooking=0` (no `[OSD]` section, so `apply_look` leaves them alone). Reports when its window doesn't open.
- **Launching:** `launch_app`, `open_path`, `open_url`, `open_system` are async (`off_main` = spawn_blocking). A sync command runs on the main thread, and launching an elevated app through Explorer waits for the whole UAC prompt. `AllowSetForegroundWindow(ASFW_ANY)` before launching; Deck steps aside after the launch. Setting `hide_after_launch` (default on); the blur-hide ignores focus loss for 8 s after a launch when it's off.
- **Apps** (`programs.rs`, `programscmd.rs`, `ui/js/programs.js`, page id `programs`): the three Uninstall views plus Store apps (Start menu names/AUMIDs), plus Windows Installer products missing from the Uninstall keys (`UserData\<SID>\Products\<packed>\InstallProperties`, packed codes unpacked; SystemComponent ones stay hidden). Uninstall runs the command in a job object and waits until the job is empty. Leftovers: exact normalized-name matches only (versions and arch words dropped, shared names refused, other programs' folders excluded), services whose ImagePath is inside the install folder; folders to the Recycle Bin, keys and services exported with `reg export` before deletion. The program being advanced-uninstalled is kept in `uninstall-pending.json` until its leftovers are dealt with.
- **Tweaks:** every switch has `tick` (the action, ticked = away from `default`), `good` and `lose` (plain words). Built-in apps and Store apps moved to Apps tabs; old `tweaks/apps` and `tweaks/store` links redirect.
- **Store updates** (`storedl.rs`): installed Store-signed packages, family → product id (displaycatalog; a network failure is reported, not treated as "not in the Store"), FE3 newest version compared, 6 at a time. Jobs saved to `store/jobs.json` and the last check to `store/updates.json`; active jobs come back as `interrupted`. The UI queues "Update all here" one app at a time. "With the Store" = MDM `UpdateScanMethod`; the `AutoDownload=2` policy (the store_auto tweak) makes the Store abort, so it is checked first, and the Store's event log is read afterwards for started/cancelled apps.
- **PC to PC:** `transfers.json` with each send's `Resume` (peer id, paths, options); unfinished ones load as Failed with an explanation, `resume` resends (the receiver's `.part` resume does the rest).
- **Phone:** `/pad` WebSocket (no capture; `gp` and `ping`), presses sent without waiting for a frame; one ViGEm pad per connection. `vigem-client`'s `get_user_index` returned 0 for every pad, so `VPad` finds its slot by comparing `XInputGetState` before and after plug-in (serialized). Portrait layout (`#padonly.pt`), per-phone look in localStorage. `serve_file` sends ETag/Last-Modified and honours If-Range. The phone list is saved to `phone-shared.json`. Full screen checks the real page size (`reallyFull`) instead of `document.fullscreenElement` alone.
- Workspace Sync's eight error messages now give the cause and the fix.
- Tests: programs (names, MSI codes, command lines, key limits), http dates, beam, tweaks, `share::tests::one_time_link` (ranges); controller tests with `--ignored --test-threads=1` (3 pads got 3 different players). Not tested on real hardware yet: a real Store update end to end, carrying on a PC to PC transfer after a restart, the controller page on a phone.

## 2026-10-21: Phone remote controls (released in 2.7.9)

- `vpad.rs`: Xbox 360 target through `vigem-client` (default features off), plugged in on the first `gp` message of a /screen connection and unplugged on drop; `wait_ready` errors are ignored with a 300 ms pause (seen with ViGEmBus 1.21 although the pad works). Test `plugs_in_here` (ignored) got XInput slot 0 on the dev PC.
- `remote.rs`: `gp` (control mode only, not paused by a game in front), `games` (per connection: taps/keys reach a game in front).
- Phone: controller layout (`#gpad`, CSS `zoom` for size, opacity variable, both in localStorage), Hold click, F1-F24 (`vk` accepts up to F24), `var` for `GP`/`gamesOk` so an early `setMode` can't hit them before they exist.

## 2026-10-21: Workspace Sync — project files, programs, MCP servers, per-project sessions (released in 2.7.9)

- "Claude Code sync" is now **Workspace Sync for Claude Code** in the UI (internal ids, ports, data folder and hook names unchanged, so existing setups keep working).
- New sync group `Class::Code`: each project's folder as `code/@alias/<path>`, raw bytes (no path tokens). `code_wanted` is the single rule for sending, receiving and passing on deletions: project code on, project `sync` + `code` on, never the home alias, never `.git/.svn/.hg/.vs`, `BUILD_DIRS` only with `sync_build`. Projects over the limits, or whose folder is a drive / the home folder or above / in the Claude folder, are skipped (shown on Projects); a missing folder or a skipped project never creates tombstones. `remap` and `delete_project` forget the project's code bookkeeping so nothing is deleted elsewhere.
- `ProjectMap.sessions` / `.code` (default true): sessions off keeps memory and files syncing.
- `mcp.rs`: `.claude.json`'s `mcpServers` (user and per project) mirrored to `deck-mcp.json` (Config group, needs approval in `guard.rs`); first import unions, later ones replace.
- `tools.rs`: installed programs published in `Manifest.tools` (refreshed every 15 min, PATH re-read from the registry); Status → Programs lists what's missing here; Install writes a validated PowerShell script and opens it unelevated via `launch_as_user`.
- Review pass, same day: conflict's `.jsonl` handling limited to Claude's own logs (project `.jsonl` files are ordinary files); `deck-mcp.json` conflicts merge with `mcp::union`, newer side winning a shared name, the same on both PCs; `HashCache` saved only when changed (`dirty`); projects too big to copy re-walked every 10 min, not every cycle; `code_wanted` refuses `auto` (guessed) projects until the user keeps/moves the folder (Projects → Keep this folder = `remap` to the same path), and per-PC files (`.claude/settings.local.json`, desktop.ini, Thumbs.db); learned paths under the other PC's home are rebased onto this home (`rebase_home`); guard covers project `.claude/settings.json`, `.claude/hooks/`, `.mcp.json`; `git.rs` publishes each project's origin URL + branch (`Manifest.git`) and `git_setup` writes `git-setup.ps1` (init, fetch, `reset FETCH_HEAD`, upstream; inputs validated); `Manifest.arch` (native, from the registry, also when Deck runs emulated) — build paths aren't received from another architecture; a project stops receiving below 2 GB free; toolchains published by channel; `msvc` (vswhere) added to programs, installed first. Tailscale switch (`Config.tailscale`) + `rdesk::tailscale_summary()` shown on Workspace Sync (Settings, Status "Away from home", per-PC "Tailscale"/"LAN") and PC to PC (peer line, empty state, Pairing).
- Disk writes: `State` maps are BTreeMap/BTreeSet (same bytes for the same content) and `save_state` skips unchanged content; `HashCache::save` writes when dirty, at most every 10 min. Measured on the dev PC over one 5-min cycle: state.json no longer rewritten (was 368 KB every cycle).
- CI: cargo audit started failing on 2026-10-20 advisories (RUSTSEC-2026-0381/0382), not on this work. rustls-webpki -> 0.103.16, webpki-roots -> 1.0.10, Deck depends on webpki-roots 1 directly; `src-tauri/.cargo/audit.toml` ignores 0381 because ureq 2.x still pulls the 0.26.11 shim, which only re-exports 1.x roots.
- Tests: `code_choices`, `sessions_off_for_one_project`, `project_paths_follow_the_home_folder`, git `reads_origin_and_branch` / `nothing_odd_reaches_the_script`, mcp and tools unit tests, `two_engines_sync_project_code` (ignored; opens ports), `detect_here` (ignored; ran on the dev PC and listed rustup, 3 extra targets, tauri-cli, node + 1 npm global, Python, Git).

## 2026-10-20: v2.7.5 — phone speaker and mic, Tweaks, Store apps, bug hunt, a sealed undo journal

Released at the user's request as **2.7.5** (tag `v2.7.5`, built and published by `release.yml`): everything in the three entries below plus this one.
- **Security (found while updating `docs/trust-boundaries.md` for the release):** Tweaks' undo writes the journal's before-values with admin rights, and `%APPDATA%\Deck	weaks.json` is user-writable, so a program running as the user could have planted, say, `HKCR\ms-gamebar\shell\open\command` (a recipe key) or any HKLM value and had it written at the next Undo. Now: the journal is sealed (`seal` name `tweaks-journal`, recorded on every save, checked at the first load; a mismatch is renamed `tweaks.changed-outside.json`, `journal_set_aside` in the state, a notice on the overview), and `before_allowed` refuses any before-value outside the tweak's own recipe (test `undo_only_touches_what_the_tweaks_recipe_touches`), and an entry for an unknown tweak. "Unpin everything from Start" no longer trusts the journal's backup path (fixed `tweaks-start2.bin`, read and written with the user's rights). Store downloads write through `fsguard::Dir` (`open_for_user`), so a junction can't redirect the elevated writes. Not live-tested: a real tampered journal at an admin start (the seal path is the hardware settings' one).
- **Docs for the release:** README rewritten around every feature (Everyday, Gaming, Hardware, Windows, Your other devices, Quality of life), new screenshots `tweaks.png` and `phone.png` from the fake-data preview, credits for Win11Debloat / WinUtil / VB-CABLE / RustDesk / Tailscale; `trust-boundaries.md` (journal row, VB-CABLE and Store downloads, phone speaker/mic row, https residual risk, tweaks residual risk); `SECURITY.md` scope; `CONTRIBUTING.md` (registry scratch key, `run_for`, stall timeouts); `DECK.md` (version, gotchas, data files, the phone mic cushion, Tweaks/Store/updater rows). GitHub's About text and topics are repo settings (not in git). After the tag: the contributor notes in DECK.md §0/§9 and CONTRIBUTING were reworded, and the design notes moved to `docs/design/` (every link updated).
- **Checked:** clippy `-D warnings` clean; `cargo test --locked` 404 pass (the known rtss user-folder test fails only in this checkout's location); regress 40/40; a11y clean; toast-test, phone-shot, phone-sign pass.

## 2026-10-20: bug hunt after the first real use

- **Updates "GitHub's answer wasn't understood"** (the user's screenshot): ureq 2 makes only 4xx/5xx errors, so GitHub's 304 to `If-None-Match` arrived as `Ok` with an empty body and failed the JSON read; every check after the first showed it until a new release. `update.rs` handles 304 in `Ok`; `deaths/library.rs` had the same pattern (harmless there, fixed too).
- **Store download "Cancelled" at 1.4 MB** (the user's screenshot): one connection to Microsoft's CDN measured 190 KB/s here, eight ranged ones 1.1 MB/s together. `storedl::download` now fetches 8 parts at once (`part_worker`, positional writes into a preallocated `.part`, progress map `.parts` saved every 2 s, a part retried 5 times without progress, one-piece fallback if ranges are refused), speed counts only this run. Live test (`downloads_in_parts_and_carries_on_after_a_cancel`, ignored): 48 MB, cancelled at a third, carried on with only the rest, SHA-256 OK, 924 KB/s. The job card has Carry on / Try again.
- **Phone remote off → on moved ports:** `phonetls::Mixed`'s accept task kept the listener after the server was aborted (port stayed open, next start took port+1). It ends on `tx.closed()`; test `a_stopped_server_frees_its_port` (fails without the fix). The certificate's names now put today's addresses first before the 16-name cut.
- **DirectX tweaks:** `gaming.vrr` and `gaming.windowed` share `DirectXUserGlobalSettings`; the before-value was the whole string, so undoing one reverted the other. New `Before::Dx { key, value }` restores just that key (old journal entries still restore as before).
- **Also:** the mic player ends when `GetCurrentPadding` fails (cable gone) instead of looping silently while the phone keeps sending; Explorer fallback (no AutoRestartShell) starts it through a one-off LeastPrivilege task `\Deck\Explorer` (checked: a task's process survives the task's deletion); "Unpin everything from Start" no longer undoes to a stale saved layout; apostrophes in paths are doubled for PowerShell (Add-AppxPackage, VB-CABLE's unzip) instead of refused.
- **Second pass (the user: "reread all old and new features"),** swept by bug class over all 61k lines, and read closely where a bug costs most (updater, phone remote routes, Game boost closing, power actions):
  - `update.rs`/`rdesk.rs`: ureq's `.timeout()` is the whole request *including the body*; the 10.6 MB 2.7.0 setup takes ~56 s at the 190 KB/s measured here, so self-update was at the edge of its 60 s and bigger setups could never install on slow lines. Now `timeout_read`/`timeout_write` 60 s (stalls end it), the API call keeps an overall 60 s.
  - `util::run_hidden` waited forever: now `util::run_for(secs, …)` (moved from `tweaks::run`, which delegates) with a 30 min ceiling for `run_hidden`; RustDesk `--get-id`, `tailscale status`, `sc query` 15–20 s; PC to PC's winget calls go through it. Test `a_program_that_runs_too_long_is_ended`.
  - `boost::kill_all`/the close wait now check the pid is still the same exe (`same_app`): a closed app's reused pid could have been killed.
  - `power::shutdown_exe` checks shutdown.exe's exit code (timers and Tweaks' sign out / restart reported success on refusal).
  - PowerShell quoting uses `rdesk::ps_str` (also doubles typographic quotes, which PowerShell treats as quotes) for Add-AppxPackage and VB-CABLE's unzip.
  - Read and fine: the phone routes (`/mic`, `/audio` signed + switch + admit), Explorer-hand-off launches (`explorer.exe <folder>`), the updater's checksum/held-file/admin-folder design.
- **Checked:** clippy clean; 403 tests pass (the known rtss user-folder test fails as before); regress 40/40; a11y clean on the Tweaks views.

## 2026-10-20: Tweaks, built-in apps, Store apps without the Store (released in 2.7.5)

Design: `docs/design/specs/2026-10-20-tweaks-design.md` (the user asked for everything at once and will judge the look at the end).
- **Data:** `tools/tweaks/build.py` reads Win11Debloat (`a2d63c7`) and WinUtil (`7514faf`) at pinned commits (cache in `tools/tweaks/.cache`, git-ignored) and writes `src-tauri/tweaks/tweaks.json` (132 tweaks, 4 presets), `apps.json` (138 apps, Edge left out) and `start2.bin`. Undo files are in `Regfiles/Undo` (one in `Regfiles`), names differ in case (resolved through the repo tree). A value two tweaks write goes to the first that claims it (Activity history, Feedback, Edge ads before Telemetry, Edge extras). WinScript (GPL-3.0): ideas only. Left out: Edge/WebView2 removal, Windows Update/Defender off, services to Manual, Razer/Logitech folder-ACL scripts, MAS.
- **This PC was debloated before** (WinUtil-like): the first read had 17 tweaks "mixed". Sticky Keys and Num Lock are bit flags in text values (`bits` on numeric strings), animations use MinAnimate + TaskbarAnimations instead of comparing the whole UserPreferencesMask; the rest are honestly partial and show "Partly · n of m" with the switch where most steps lean.
- **Bugs found while testing:** DISM run without admin hangs (it sat 10 minutes): every program the engine runs now has a time limit (`tweaks::run`) and DISM isn't asked without admin. A first `sed` with `\|` in its pattern rewrote six strings in `apps.rs` (reverted). PC to PC's own Explorer restart (`beam/snapshot.rs restart_explorer`) started `explorer.exe` from the elevated Deck (an elevated shell): it now uses `tweaks::restart_explorer` (Winlogon restarts it as the user).
- **Store apps:** FE3 needs Microsoft's two roots (not in webpki), embedded and checked against Windows' store (thumbprints 8F43288A…, 06F1AA33…). Live: Spotify gave the x64 app (221 MB) + 3 dependencies, To Do the msixbundle (90 MB) + 7; dependencies the PC has are marked "have" and not downloaded (the user: "it's 1 file"). FluxDM 2.2's local API takes `{"urls":[…],"fileName":…}` on `/api/add`.
- **Checked:** unit tests (scratch HKCU key: exact value/type undo, key trees, bits, DirectX string, state), live read of every state (no hang, 0.7 s), live apply → undo on three HKCU tweaks with `reg export` identical before and after, package picking, live FE3 lookup. UI: regress 40/40, a11y clean on six Tweaks views at 1260×900 and 900×600 (`all.sh` lists them; the preview's data is `mock-tweaks.js`, a real PC's state). clippy clean; 359 tests pass (the known rtss user-folder test fails as before).
- **The user's first use (installed build, elevated):** 19 tweaks applied (7 one by one, 12 through the Recommended checklist), all logged, none rolled back; read back afterwards, every one is at the option set and none "changed back". Edge/Chrome policies make the browsers say "managed by your organization": those four tweaks now say so in their Details.
- **Merged under 2.7.0:** the friend released 2.6.1 and 2.7.0 while this was uncommitted, and Deck's updater had already tried to install 2.6.1 over the custom build (it timed out). Our work was stashed, 2.7.0 pulled, the work put back (CHANGELOG/PROJECT_STATE conflicts kept both sides), so the installed build is 2.7.0 + this.
- **Not checked yet:** an Explorer restart, a restore point, removing/reinstalling an app, a full Store download and install, FluxDM hand-off, the 6-hourly watch, a PC to PC setup with tweaks.

## 2026-10-20: Phone as speaker and mic, HTTPS for the phone remote (released in 2.7.5)

- **Why no own driver:** Windows lists a microphone only when a driver makes one; Deck's would need Microsoft-attested signing (an EV certificate), and test signing stops Vanguard, FACEIT and many EAC/BattlEye games, so the user chose VB-CABLE. Bluetooth isn't possible from a web page (and Android won't act as a headset); USB isn't either. The user's VB-CABLE here is 1.0.3.5 (2014, Windows 7 inf) and disabled; `pnputil /enable-device` answers "The device is not connected" (its driver no longer loads), so the Phone page then offers the current one (Pack45, signed "BUREL VINCENT"). VB-CABLE's licence: copying the package as is is allowed, building it into another installation is not, hence "download and open VB-Audio's setup", no `-i -h`.
- **Checked live:** loopback still captures with the PC muted and at volume 0 (pycaw + pyaudiowpatch, 2026-10-20). `--selftest-remote` with `DECK_SELFTEST_AUDIO=1` (new; it also gives a test profile whose key can't be unsealed a new one): http and https pages 200, the worklet over https, `wss /audio?spk=1` 332 packets in 4 s, the PC muted while asked, unmuted when asked and after the phone left; `/mic` refused with the VB-CABLE message while there is none. The player on the default output (ignored test `player_plays_a_bursty_stream_whole`, bursts of 10-60 ms): 1500 of 1500 ms played; opening the device takes ~0.7 s and what piles up meanwhile is dropped, not delayed.
- **VB-CABLE installed here (3.3.1.7):** its setup put the new driver on the old, disabled device (ROOT\MEDIA\0001), which stays disabled (the flag is on the device), so the user had to disable/enable it; a PC without an old disabled one won't need that. The setup also made "CABLE Input" the default speakers: the Phone page now says so with buttons for the real outputs, and the mic refuses to start while it is (the voice would mix with the PC's sound and come back out of a phone speaker). The endpoints are "CABLE Input" / "CABLE In 16ch" / "CABLE Output" (Device Manager calls the first "Speakers (VB-Audio Virtual Cable)").
- **Mic checked through the real cable:** `/mic` with a 440 Hz tone sent in real time, recorded from "CABLE Output": 3.0 of 3.0 s, no 10 ms window missing, ran dry once (at the end). A sender paced by Node's 10 ms timer (~15 ms on Windows) gave gaps: a test artifact.
- **Fixes after a review of the first version:** the player reports whether it could open the cable (the phone hears why at once, not by silence); `Mic::alive` closes the socket when the player ended, so the phone reconnects; the default mic is put back only for a role still on the cable (one the user changed meanwhile stays theirs; `cable` in `phone-mic.json`); "PC quiet" waits 5 s after a phone's connection ends before unmuting (a Wi-Fi blip reconnects in 2 s), but the phone's switch acts at once; the phone's state asks for VB-CABLE at most every 5 s (`cable_ready`). Phone page: the sound player is kept across reconnects (Android only starts sound from a tap, so one made by a reconnect stayed silent) and any tap resumes a held one; the mic's player is made in the tap, before the permission prompt; a phone that pauses its mic (a call) says so; the speaker away (Tailscale) starts at the smaller sound; the card always shows, saying where to turn it on; Deck's card has a "How to use it" fold with fixes for the usual problems.
- **The user's phone mic:** works; one 237 s session ran dry 44 times (Wi-Fi bursts longer than the fixed 40 ms cushion). The player's cushion now adapts like the speaker's: 60 ms, +20 ms after each gap up to 200 ms, −10 ms after 30 s without one.
- **Not checked yet:** the default-mic switch, the crash restore of either file.
- **Tests:** `cargo clippy --all-targets -D warnings` clean; new tests `phonetls::a_made_certificate_reads_back_for_rustls`, `phonemic::only_vb_audio_signed_setups_run`, `phonemic::mic_bytes_read_as_little_endian_samples`; `regress.mjs` 40/40, `a11y.mjs` clean, `phone-shot.mjs` (now with the Speaker and mic card) clean, `phone-sign.mjs` passes. The UI harness and three suites hard-coded Edge; they now fall back to Chrome like `shot.mjs` (`harness.mjs` exports `BROWSER`).

## 2026-10-20: v2.7.0 — death counter: new in-game look, sessions, boss cards, records

The user found the counter "very underwhelming" and the History tab bad. Designed with them from browser mockups (spec `docs/design/specs/2026-10-20-death-counter-look-design.md`, plan `docs/design/plans/2026-10-20-death-counter-look.md`): "B while you play, C after you die", plus ring and huge-number styles, an optional red edge, everything switchable; then "do whatever you think is best" for the page.
- **History model:** `Ev::Play` with `Event.ms` (a play session's length; one line per start, rewritten in place every 5 min by `put_play`); `Book.deaths` now keeps each death's time, `Book.wins`, `Book.plays`. Old Deck versions skip the play lines (unknown lines are skipped on load).
- **`deaths/stats.rs`:** tries (fight start or last death/win to a death/win; > 30 min = unknown), sessions (play lines < 15 min apart merge; history before the first play line is estimated, split at 1 h gaps), records, `death_note` (milestones 1, 10, 50, 100, every 100th; "Your longest try yet" from 3 tries).
- **Sessions are recorded** by `session_step` in the counter's loop (the counted game in front; another game ends it at once; nothing in front for 15 min ends it).
- **`deaths/view.rs`:** the page's JSON (`tries`, `session`, `bosses` with `best_try_ms`/`tries_ms`/`won_on`/`note`, `sessions` with events, `records`) and `overall` (cached 10 s in `St`), the CSV export (`local_iso`, a civil-date function; no date crate). A 40,000-event history once took 2.3 s (a boss list re-keyed per death): a test pins it.
- **Overlay:** `deaths/look.rs` draws the counter (skull / ring / huge; boss, game or both), the card, labels and the 600x315 share picture with the overlay's SVG rasteriser; `pill.rs` shows any picture with a 150 ms fade-in; `edge.rs` four click-through strips glowing red for 800 ms. The loop: label > card (for `card_secs`) > counter; the counter pops (x1.15 for 120 ms) when its number changes; ticks 33 ms only while animating. A death no longer flashes "+1 · …" text.
- **Settings:** `pill_style`, `pill_counts`, `pill_outside`, `card`, `card_secs`, `card_lines` (`CardLines`), `red_edge`. Commands `deaths_note`, `deaths_preview`, `deaths_share` (clipboard via `clipboard::set_png`, or a save dialog), `deaths_export`.
- **Page:** Now (both counters, last tries, session tiles, records, share), Bosses (cards with a try-length line and notes), History (session cards, timeline with folded runs and a "⋯" menu to delete a line), Games (all games, export), Settings (new rows, card section, live preview). Charts in `ui/js/deathcharts.js` (inline SVG; bars use `.dc-b` because the global `.bar` sets `height`, which overrides SVG rect heights).
- **Checks:** clippy clean; 391 Rust tests; UI `regress.mjs` 40/40, `a11y.mjs` clean on every Deaths tab. Drawn samples checked by eye (`look::tests::draw_samples`, `DECK_LOOK_DIR`). **Not checked in a game yet:** the card, the fade and pop, the red edge, session recording.

## 2026-10-20: v2.6.1 — teaching fixed on Khazan's real death screen; no Steam ids

- **Teaching refused Khazan's death screen** ("mostly one flat colour"): the screen fades nearly black everywhere, so "cells that changed from play" was the whole picture, and its spread was flat. Now, when more than 40 % of the cells changed, the print is the cells that stand out from their 5x5 neighbourhood (>= 9) *and* lie in a row run of >= 5 (one gap allowed): the words; the figure behind them differs at every death. The flat-colour check is skipped then (>= 8 such cells are required instead). The "too close to normal play" check now ignores matches in the run of pictures around the picked one (the death screen fading in and staying).
- **Grey thumbnails dulled red words:** `capture::thumb` is now each cell's brightest channel (`capture::colour` averages RGB); the strip previews are colour PNGs. Taught prints from before need teaching again (none existed yet; the shared library is empty).
- **Checked on the user's real pictures:** ignored test `teaching_learns_real_pictures` (`DECK_TEACH_IMAGES`): taught after synthetic bright play, the print matches the picture 0.8x and 1.25x as bright and with the scene moved 9 cells, and refuses the scene without the words and a black screen; both pictures pass.
- **No Steam ids (user: "forgo all Steam ids, stick to the process, and do screen detection"):** `ident` no longer reads Steam/Epic manifests (`installed_dirs`, `match_dir` and the miss cache are gone); a game is any fullscreen program or one the catalogue lists by exe, named from `boost::file_description`, else its install folder (build folders such as `Binaries\Win64`, library folders such as `steamapps\common`, and a folder named like the exe are skipped), else the title. `catalog.json` keeps only the 17 entries with exe names, without `steam` (the 302 others carried only a name and boss/run/level; the two Dark Souls II entries merged; `start_protected_game.exe` removed, it is Easy Anti-Cheat's launcher for many games). `by_steam` and `steam:` keys are gone; the library and `ingest.py` accept only `exe:` and `emu:` keys. `tools/deaths-catalog/` (SteamSpy-based) deleted; the catalogue is kept by hand.
- Checks: 353 tests, clippy clean.

## 2026-10-20: Death counter in a real game (Khazan): fixes (in v2.6.1)

The user's first real test, The First Berserker: Khazan (a copy outside Steam, `BBQ-Win64-Shipping.exe`): a death wasn't counted and teaching refused ("Deck needs a few seconds of the game before the death screen").
- **Why it wasn't counted:** (1) the catalogue knew Khazan only by its Steam number, so outside Steam it was an unknown game "KZ" with only the common words; now its exe is listed (`bbq-win64-shipping.exe`). (2) Even with the words, Windows OCR read **nothing** from the dim red banner: tried on the user's two pictures of the death screen, darkened to 0.2-0.35 as in the game (ignored test `ocr_reads_a_picture`, `DECK_OCR_IMAGE`), plain gave "" both times. `capture::middle_for_ocr` now hands OCR the brightest channel stretched (2 % black, 0.2 % white, range floor 48): "KHAZAN HAS FALLEN" and "(HAZAN HAS FALLEh". (3) The second read failed the review's no-dropped-letter rule; phrases of 12+ letters may now be off by one letter per 8 (Levenshtein), shorter ones still only by substitution.
- **Why teaching refused:** the strip was 10 s and the user counted ~9 s after the banner, so most frames were the death screen, and "normal play" (whole-picture mean difference >= 12) found too few frames: the game is as dark as its death screen. Now a frame is also "other" when >= 24 cells differ by >= 24 (`teach::other_picture`), the strip keeps 20 s at 2 frames a second, and a print's mask takes a one-cell ring around the change (dim words are all one grey; their edges against the screen make the shape). Test `a_dark_games_death_screen_is_learned_from_a_mostly_death_strip`.
- **The page** kept showing the generic "Deck watches..." text: watching stops while Deck itself is in front; the last status is now kept.
- Checks: clippy clean, 352 tests.

## 2026-10-20: v2.6.0 — controller overlay, death counter, updates from GitHub, crash protection (built, tested, installed)

- **Checks:** `cargo clippy --all-targets -D warnings` clean (two style fixes: `checked_div` in `screen::aspect_of`, `contains` in `elevation::runs_as_desktop_user`); `cargo test` 350 passed, 35 ignored; UI `regress.mjs` 40/40, `a11y.mjs` clean; `ingest.py --selftest` ok.
- **Install while a game runs:** `install.ps1 -Quiet` skips the launch test (it opens a console through Explorer) and the script's elevated re-run is hidden; every install now starts Deck through `\Deck\Background` (hidden, in the tray) instead of `\Deck\Open`. The log showed the four tasks made and the watchdog up.
- **Measured (60 s, a game open in the background, 20 logical cores):** Deck 3.9 % of one core (0.2 % of the PC), 69 MB private, 98 threads, 1,340 handles; its WebView about 1 % of a core, 303 MB; the watchdog 0 %, 2 MB. By thread: `rtss` 1.25 % (it takes a full sensor snapshot every `osd_ms` even with the overlay off, so switching it on shows at once), `driver-watch` 0.16 %, `gpu` 0.16 %, the rest under 0.5 % each. The death counter wasn't measured in a game (it only watches the game in front). Ideas not done: a slower snapshot while the overlay and the Sensors page are off (~1 % of a core); reading back a small mip level for the 64x36 thumbnail and the 1280-wide one only for OCR.
- **Not live-checked yet:** the restart after `taskkill /f`, the updater against a real release, the driver-install pause.

## 2026-10-19: Death counter: second review, fixes (unreleased)

Built and tested 2026-10-20 (see above). Fixed:
- **Fullscreen browsers and video players were games:** `ident::not_a_game` now also uses `boost::not_a_game` (boost's `NOT_GAMES` + Windows' folder). A fullscreen YouTube video of Elden Ring counted its "YOU DIED"s against "exe:chrome".
- **An unchecked save reader silenced the screen watcher:** `Tracker::proven()` = working and (catalogue `verified` or it has counted a death); only then is screen detection off for the game.
- **One death counted twice (your key + the screen):** `act` merges a death from another way of counting within 10 s when one of the two is automatic (`same_death`); the pill says "Already counted".
- **"WASTED" matched "WANTED":** one-word phrases are exact; longer ones allow one letter read wrong, never a dropped one.
- **Celeste:** a save can bring up to 500 deaths at once (`save::max_step`), the Souls games 20.
- **Library updates could be skipped:** two PRs merged from the same base carry the same version; `adopt` now takes the same version with other content (never a lower one), and the GitHub copy wins a tie with the embedded one.
- **Shared exe names:** `Game.exe`, `nw.exe`, launchers… are keyed `exe:<folder>-<stem>` (`ident::exe_key`); the library refuses bare generic keys.
- **Rescans:** a program not found in a fresh list of installed games isn't looked for again for 10 min (`ident::looked_for_lately`); before, every app in front made Deck read every Steam/Epic manifest every 30 s.
- **Cost:** games Deck knows nothing special about are watched 2 times a second, not 4; the teaching strip keeps the last 10 s whatever the rate.
- **English OCR missing:** the page says so (`screen::NO_ENGLISH`, "screen_lang").
- **Taught prints and screen shape:** `Print.aspect` (width/height x 1000, 0 = any), matched within 3 % (`Taught::fits`); the intake script keeps it and draws each print as text in the PR so the reviewer sees what will be matched; 6 prints per game (what Deck uses), at most 20 waiting PRs.
- **Smaller:** a +1 hotkey with no game yet says why (a toast; nothing is in front then); taught prints are read from disk once (`store` cache); the controller combo's text says the game sees the buttons too (it was already off by default with its own switch).
- **Every game is known by its exe (user, 2026-10-19: "just use the exe for those, even for Steam games"):** the key is `exe:<stem>` in every store (`catalog::stem_key` drops `.exe` and Unreal's `-Win64-Shipping` / `-WinGDK-Shipping`), so a Steam, GOG, Game Pass or EA copy is the same game with the same history, catalogue extras and shared-library prints. Steam/Epic folders still give the store name and, for Steam, the catalogue entry by number; otherwise `catalog::by_exe` (now also Celeste's `celeste.exe`). `catalog::by_key` finds the entry for a key; `steam:` keys are still read.
- **Not done:** death words are English only (localised games); the CPU cost in games is still unmeasured.

## 2026-10-19: Updates from GitHub, one-step setup, crash protection (unreleased)

Spec and plan: `docs/design/specs/2026-10-19-updates-and-crash-protection-design.md`. Not installed, not committed, not pushed.

- **Why "Set up" came back after every install:** the NSIS finish page (`RunMainBinary`) and `/R` start Deck with `RunAsUser`, so Deck ran unelevated even though the admin tasks survived the update. The 2026-10-18/19 log shows the tasks being re-created each time.
- **One UAC, in the setup:** `hooks.nsh` `NSIS_HOOK_POSTINSTALL` runs `deck.exe --setup-task` (the setup is elevated: per-machine) and, for silent, passive and update installs, starts Deck through the new `\Deck\Background` task (`--minimized`: no window, no focus). An uninstall now removes the four tasks and `updates`. A GUI install's "Run Deck" box still starts it as the user; `elevation::handover` (called in `main` before the window exists) passes any plain unelevated start (no args: `\Deck\Open`; `--minimized`: `\Deck\Background`) to the admin task when it runs this exe; a 20 s marker file stops a loop. `ensure_quit_task` also makes the Background task for installs that lack it.
- **Crash restart:** `watchdog::wait_for` now returns `Outcome` (`Clean`, `Unclean { code }`, `NothingToWatch`); `describe_exit` names the code. After the hardware release `restart::after_unclean_end` waits 15 s and for a driver installer or Deck's own setup/uninstaller, then runs `\Deck\Background`. 3 unclean ends in 10 min (`crashes.json`) = give up and write `safe.json`; the next start holds the hardware like `HW_HELD` (fans on the BIOS, no overclock) with the `crash-loop` toast ("Turn hardware control on" clears it); safe mode lapses after 30 min. The watchdog now starts for every admin Deck with tasks (not only with the hardware engine). The native-crash filter logs the exception code and the module (`watchdog::module_at`). Setting `restart_after_crash` (default on).
- **Driver installs:** `driver.rs` recognises a graphics driver setup by the folder or name it runs from (NVIDIA `DisplayDriver`/`Installer2`/`Downloader`, AMD software installer; best effort) with a cheap name-first process scan every 5 s. While one runs `PAUSED` is set: `gpu::nvml::Nvml.f` is a `Gated` wrapper whose `Deref` returns an empty function table, and `NvApi::func` returns None, so every GPU call fails cleanly with no call-site changes. Twenty seconds after it ends Deck restarts itself once (`restart_deck` spawns `--start-after <pid>`, then quits cleanly) so NVML is initialised fresh and the profile is applied again.
- **Updater:** `update.rs` (+ `updatecmd.rs`, Settings → About → Updates): daily check of `api.github.com/repos/MohammadFakih02/deck/releases/latest` (ETag, first look 2 min after start, retry in an hour), accepts only a published plain `X.Y.Z` greater than this build with a `*-setup.exe` and its `.sha256` under this repo's release downloads; downloads (150 MB cap) to `Program Files\Deck\updates` when Deck is admin with tasks in an admin-only folder (`silent_ok`) or to a fresh Temp folder; SHA-256 checked while the file is held open without write sharing; runs `setup /S /UPDATE` (no prompt when elevated; UAC via `runas` otherwise, only on the user's click or toast button); waits while `boost::game_in_front` or a fullscreen app; toasts "Updating Deck to X" and "Deck updated to X" (once, via `update.json`); a setup that did not replace Deck within 4 min is not retried for a day. Setting `auto_update` (default on; off = no background requests).
- **Tests:** 344 pass (16 new: handover decision, restart verdicts, own-setup and driver-installer recognition, exit codes and module names, version compare, release picking and URL allowlist, checksum parsing, hashing); clippy clean; UI regress 40/40 and a11y clean; `makensis` compiles the installer with the new hooks. **Not verified live:** the restart after `taskkill /f`, the silent setup over an install, the updater against a real release and the driver-install pause need an install on the PC (check for games first).
- **Limits:** the SHA-256 comes from the same release (no signing); driver-setup recognition is a path list, backed by the restart.
- **Two code reviews, all findings fixed (built and tested 2026-10-20):** a failed download/start now waits 6 h, a setup that didn't install a day (it used to retry every minute); `update::set` locked the status twice (a deadlock); an error stays on the page for the same version and the minute loop only tells the page when something changed; `silent_ok` is remembered 10 min; the silent setup is waited for (30 min) instead of a 4-minute guess; without admin tasks the setup no longer runs elevated from Temp (`--install-update`, see DECK.md) and Temp folders are cleaned; the uninstaller keeps `updates` during an update; the update's close isn't counted as a crash (`restart::mark_updating`) and the watchdog doesn't start a second Deck (`other_deck_running`); `ensure_quit_task` only points the Quit/Background tasks at the copy `\Deck\Deck` runs, in an admin-only folder; a `--minimized` start forwarded to a running Deck no longer opens the window; `--start-after` waits 2 min and never starts a second Deck; the restart after a driver install honours "Start again after a crash" and waits for no game in front (GPU calls stay paused meanwhile); safe mode's button goes through the "changed outside Deck" check (`App::resume_hardware` shared with hw-changed); the setup makes no tasks when it runs as another account (`--from-setup`).

## 2026-10-19: Death counter: code review of the feature, 9 findings fixed (unreleased)

- **Counting:** (1) the screen watcher no longer ends for good on a capture error: it opens the capture
  again (20 bad frames in a row, or a failed open, then every 5 s) until the deaths thread stops it;
  (2) the OCR matcher re-arms when the picture has moved at least 3 s after a count (`Gate::moving`,
  `Matcher::moved`), not only on a read without the death text, which action games rarely give;
  (3) a save read that hits a half-written file no longer starts a screen watcher (`Tracker::working`,
  15 s grace) so one death can't be counted by both; (4) `act` refuses "death" and "start" for a game
  turned off, whatever the source (the shortcuts bypassed the tray's check).
- **Library:** `made` is capped to what the Rust side reads (`ingest.py`), `library::parse` skips a bad
  game or print instead of failing the whole file (a 1e30 `made` used to turn every Deck's library, the
  embedded one too, into an empty one), names are cut on a character (the old `truncate` could panic) and
  cleaned (`clean_name`), the import of a pasted code uses the same key and name rules (`unshare`).
- **Workflow:** one trigger (`labeled` with the `death-screen` label), a per-issue concurrency group and a
  "handled already" check, so an issue can't start two runs or a second PR.
- **Page/settings:** the share code is remembered with its game; the settings form can't overwrite the
  games turned off (`set_setting` keeps `off_games`); the loop reads save files before taking the lock the
  page and tray share.
- **Tests:** 7 new (`tracker working`, matcher moved x2, gate moving, off-games rule, library parse x2,
  unshare cleaning, capture retry); the three live screen tests pass again after the watcher rewrite.
  Clippy clean, UI suites clean, `ingest.py --selftest` covers the huge `made`.

## 2026-10-19: Death counter: the shared death-screen library (unreleased)

- **Why:** user: "can we make it so teaching the app teaches the whole repo? so we have stuff from online,
  instead of having to manually". A game is taught once by anyone; everyone gets it.
- **Repo:** `data/death-screens/library.json` ({version, games: {key: {name, prints}}}, seeded empty),
  issue form `.github/ISSUE_TEMPLATE/death_screen.yml` (label `death-screen`), workflow
  `.github/workflows/death-screen-intake.yml` (issue -> `tools/deaths-library/ingest.py` -> PR; the issue
  text goes in only through an env var; names are stripped of @ and markup), CI runs `ingest.py --selftest`.
  **Untested on GitHub** (needs Settings -> Actions -> General -> "Allow GitHub Actions to create and
  approve pull requests"); the script itself is tested locally, both directions: a share code made by
  Deck's Rust code passes `ingest.py`, and the file it writes loads in `library::parse`.
- **App:** `deaths/library.rs` embeds the file (`include_str!("../../../data/...")`), refreshes it from
  `raw.githubusercontent.com/MohammadFakih02/deck/main/data/death-screens/library.json` at most once a
  day (first look 60 s after start, ETag, 3 MB cap, retry after an hour on failure, last check kept in
  `deaths\library.meta.json`, copy in `deaths\library.json`, used only when its version is higher and
  every print passes `Taught::new`); setting `deaths.community_library` (default on; Settings tab). The
  watcher matches user prints + library prints; library prints are never saved to the user's list.
  Now tab: "Submit to the shared library" (copies the code, opens the form via `open_url`), a card when
  only the library knows the game, a count; Games tab: library totals.
- **This is Deck's first background network request** (everything else is user-triggered). Flagged to the
  user; one toggle turns it off.
- **Checks:** 321 tests (4 new in `library::`), clippy clean, UI suites clean.
- **To do on GitHub:** push, enable the Actions setting above, submit one test issue to check the
  workflow, then it fills as people teach games.

## 2026-10-19: Death counter: teaching a death screen (unreleased)

- **What:** for death screens OCR can't read (Sekiro's 死, a symbol, a flash): the watcher keeps the last
  40 frames (10 s; 64x36 thumbnail + 160x90 grey preview) in memory (`screen::RECENT`, never written).
  A death counted by hand (tray, page, hotkey, combo) within 60 s of the last frame keeps them as a strip
  (`St.strip`); the Now tab offers "Did Deck miss that death?", you click the clearest frame, and
  `screen/teach.rs` learns it: cells differing >= 24 from the median of frames unlike the picked one
  (normal play) become the mask; refused when fewer than 24 cells, a flat colour, too few play frames, or
  when it matches any play frame. Matching: correlation >= 0.85 and mean difference <= 30 over the mask,
  on every frame, its own matcher (same pair + cooldown rules; either matcher counting cools the other).
  Up to 3 per game in `deaths\<key>.taught.json`; Forget; Share code `deck-death-screen:<base64 json>`,
  added on the Games tab. Commands `deaths_strip`, `deaths_teach`. Works without an OCR language.
- **Checks:** 317 tests after the merge (3 new in `teach::`), clippy clean; new ignored live test `a_wordless_death_screen_is_learned_from_captured_frames`
  (a red disc on Deck's own test window: not counted by OCR, learned from captured frames, then matches it
  and not play) passed; run the two screen live tests one at a time (they share the frame buffer).
- **Merged with the friend's 16ca8af** (pulled 2026-10-19): CHANGELOG and this file had conflicts; his
  commit turned this file's line endings to LF, kept.

## 2026-10-19: Death counter, stages 2 and 3 built without a game (unreleased)

- **Why theory:** no soulslike installed (user: "keep it theory for now", "finish the core logic",
  "a basic catalogue, about 10 games, I will work on it with you later").
- **Save readers** (`deaths/save.rs`): Celeste (`Saves\N.celeste` in the install folder, XML
  `<TotalDeaths>`; format from github.com/frissyn/celeste-saves) and Dark Souls PtDE
  (`Documents\NBGI\DarkSouls\*\DRAKS0005.sl2`, BND4, name +0x100, deaths +0x1F128 from dsfp's
  datasheet, MIT). `Tracker` checks modified times every 2 s, first read is the baseline, a rise of
  at most 20 on the same slot + name counts. `ident::Game` now carries `dir` (install or exe folder).
  Elden Ring's count has no fixed offset (variable-length slot sections): needs two real saves.
  DSR/DS3 are AES-CBC per entry with public keys, but their death offsets are unknown.
- **Screen detection** (`deaths/screen/`): WGC capture of the front game's window at 4 fps (code from
  the capture spike, reading back a mip level at most 1280 px wide), 64x36 thumbnail gate, OCR of the
  middle 3/4 x 2/3 with `Windows.Media.Ocr` (English engine first), line-wise phrase matching with
  edit slack and I/l/1 O/0 folding, matcher (pair within 3 s, 8 s cooldown, must clear first).
  Runs only while a counted game is in front, `auto_screen` on, and no healthy save reader.
  The page shows how the game is counted (`auto`: save / save_missing / screen / screen_blind /
  screen_failed) and the presets on the Games tab; Settings has the two automatic toggles.
  Teaching (taught fingerprints) is not built yet.
- **Presets** (catalogue, all `verified: false`): FromSoftware (ER, Nightreign, DSR, PtDE + save,
  DS2, SotFS, DS3, Bloodborne, Demon's Souls) "YOU DIED"; Cuphead "YOU DIED"; Nioh 1/2
  "FREED FROM THIS MORTAL COIL"; Wo Long "COMPLETE FAILURE"; The First Berserker: Khazan "KHAZAN HAS FALLEN"
  (the publisher's Steam Awards post says those words are "burned into your screen"; added 2026-10-19 after a
  web search for the wording; the other ~300 games found nothing, see the research note in DECK-GAMING-PLAN.md);
  Celeste (save). 15 in all.
- **Checks:** 312 tests (+2 new ignored live tests that passed here: Windows OCR reads a drawn banner;
  a drawn "YOU DIED" on Deck's own test window counts exactly once end to end), clippy `-D warnings`
  clean, `regress.mjs` all PASS, `a11y.mjs` clean.
- **Next (needs games):** a real death in each preset game; Elden Ring save diff; CPU check in game;
  teaching; Sekiro's 死 fingerprint.

## 2026-10-19: Death counter, stage 1 (unreleased)

- **What:** Monitor → **Deaths** + tray submenu "Death counter" + in-game pill + two hotkeys.
  Design: `docs/design/specs/2026-10-19-death-counter-design.md` (stages: 1 manual counting
  [this], 2 save-file readers starting with Elden Ring, 3 screen detection after the capture spike).
- **User's choices (2026-10-19):** counting automatic + hotkeys + controller combo, shown on Deck's own
  pill; any game counts ("don't want it to just detect these games"), the catalogue only adds extras;
  100+ known games.
- **Code:** `src-tauri/src/deaths/` (`book.rs` from the earlier `deck-deaths` worktree, now in main),
  `deathscmd.rs`, `ui/js/deaths.js`, `.dc-*` styles, skull icon, mocks + `all.sh` entries. Game
  recognition reuses `games::installed()` (Steam/Epic install folders) and `hw::game::covers_like_fullscreen`.
  Pill = `pad::window::Layered` + `pad::art::pill` on its own thread (1 s tick, 200 ms while flashing).
- **Catalogue:** `tools/deaths-catalog/` builds `catalog.json` (320 games): hand-picked list (Steam
  store search) + SteamSpy tag lists (Souls-like, Boss Rush, Precision Platformer, Action Roguelike,
  Metroidvania, Hack and Slash) with ≥ 10,000 reviews, filtered by hand. Steam's own review API
  rate-limited (429) at 1 request / 1.3 s, so SteamSpy (one request per tag, a minute apart) was used.
  Death words only for FromSoftware ("YOU DIED", unverified in-game); Bloodborne (shadPS4) and Demon's
  Souls (RPCS3) by emulator + window title.
- **Controller combo:** `deaths/combo.rs`, hold Select + D-pad Down 1 s = +1, Select + Up = undo,
  once per hold. The deaths thread opens its own passive `pad::input::Reader` (the overlay's only
  runs while the overlay is on; SDL ref-counts a pad opened twice) only while the combo is on and a
  counted game is in front, polling at 50 ms; otherwise it keeps its 1 s tick. Toggle on the
  Settings tab.
- **Not done yet:** save readers, screen detection, a live test in a game.
- **Checks:** 301 tests (20 in `deaths::`), clippy clean, `regress.mjs` all PASS, `a11y.mjs` clean.
  Also fixed: the "Connected" toast now names a GameSir dongle ("XInput Controller #1" before),
  `battery::known_name`.

## 2026-10-19: Controller Overlay merged into Deck + universal controller battery (unreleased)

The standalone Controller Overlay (Python, pygame-ce + tkinter) is now a Deck feature: Monitor →
**Controller**. Design: `docs/design/specs/2026-10-18-controller-overlay-design.md`; plan:
`docs/design/plans/2026-10-18-controller-overlay.md`.

- **Backend `src-tauri/src/pad/`** (see DECK.md §5). Battery thread: GameSir vendor protocol (dongle
  3537:1098 MI_02, `01 01`, byte 19), Sony DS4/DualSense input reports read passively (never a feature
  write), Windows.Gaming.Input, the Bluetooth battery property, XInput; merged per controller (a higher
  source wins per VID/PID, ViGEm virtual pads dropped, XInput only without WGI), phone-style smoothing in
  `pad-battery.json`, 2 silent polls = off, low warning once per charge at 15%. Runs only while tray icons or
  the overlay are on.
- **Overlay thread:** native layered windows (`WS_EX_LAYERED|TRANSPARENT|TOOLWINDOW|NOACTIVATE`, topmost
  re-asserted every 500 ms, `WDA_EXCLUDEFROMCAPTURE` unless turned off), high-resolution waitable timer +
  `MsgWaitForMultipleObjects` (60 Hz lively, 30 faded, 20 hidden, no timer while off), toast pill, Move /
  resize (drag, wheel, right-click), auto-monitor from the foreground window (ignores Deck's own windows and
  the shell), idle fade, demo input.
- **Art:** the old SVG builders ported to Rust and rasterised once per look with `resvg` (text feature only;
  Segoe UI loaded from Windows' fonts). The old renderer (SDL_image's nanosvg) drew every gradient as its end
  colour, so `theme_defs` does the same: side-by-side pixel checks against the Python app now differ by a few
  shades at most.
- **Input:** SDL 3.4.16 loaded at run time (`sdl.rs`; the official DLL embedded gzipped, written next to
  deck.exe, loaded by full path), hints keep it passive (background events on, enhanced Sony reports off, no
  rumble/LEDs); XInput fallback. Real pads are opened before a ViGEm Xbox 360 pad. Keyboard & mouse: Raw Input,
  receive-only, only while that mode is on (user's choice "A", 2026-10-18; DECK.md §0 rule 3 updated).
- **Deck integration:** `pad` + `pad_imported` in settings.json (field-by-field validation, the old
  `Settings.CHECKS`); one-time import of `%APPDATA%\ControllerOverlay\settings.json` + `battery.json` (overlay on
  after an import, off for new users); tray submenu **Controller overlay**; one tray icon per controller
  (`pad-<hash>`, left-click opens the page, right-click is Deck's menu); `pad_state` / `pad_action`;
  `--pad-probe` diagnostic; `hooks.nsh` removes `SDL3.dll` on uninstall.
- **UI:** `ui/js/controller.js` (tabs Overlay, Keys, Battery, Test; the Test tab polls 8×/s with a timer, no
  frame loop), new `joystick` icon, mock commands for `shot.mjs`; Controller tabs added to `all.sh` (a11y).
- **Checks:** 281 tests pass (47 new in `pad::`), clippy `-D warnings` clean, `regress.mjs` 40/40, `a11y.mjs`
  clean on every Controller tab (also at 900×600). `--pad-probe` on this PC: SDL3 loads; the GameSir dongle
  shows as 3537:2106 (one vendor interface) while the controller is off, as expected.
- **Not yet done live** (the user was in a game the whole session, so Deck wasn't restarted): the overlay
  window on screen, the tray icons, the GameSir reading with the controller on, CPU per the budget, Raw Input
  next to tao's own registration. Do these first next time; then the user uninstalls the old app.
## 2026-10-19 (later): overlay clocks = Afterburner's (ratio x bus), Afterburner look, busiest-thread core load (not released)

- **Why:** the user (i5-14600KF, the "friend" of the 2.5.10 entry) still saw 2.5.10's overlay differ from Afterburner: Deck's thread lines 4931-5199 (P) / 3487-3666 (E), Afterburner a flat 5292 / 3994 with an occasional 799. Two causes: (1) the overlay's thread table never used 2.5.10's counters: `rtss::snapshot` filled `thread_reads` from its own PDH `ThreadClocks`; only the CPU line used `Cpu::clock`. (2) APERF/MPERF is the average clock while awake; Afterburner shows the current ratio (`IA32_PERF_STATUS` 15:8) x bus. Its 799 is a read that landed on a sleeping core (ratio 8), which only a ratio read gives. On a freely boosting CPU in a light game load the step moves within the second, so the average sits 2-4 % under the snapshot; with the clock locked (a fixed ratio) both agree, which is why the earlier HWiNFO check matched. Cinebench R23 multi on the same PC: ~23,700, normal for the part.
- **Fix:** `sensors::ratio_mhz(bus, perf_status)`, `sensors::bus_mhz(tsc, platform_info)` (TSC / 0xCE bits 15:8); `Cpu.bus` read once at open (Intel), `Cpu::counted_clocks` reads 0x198 in the same `on_cpu` pass as MPERF/APERF and fills `Cpu::ratio_clocks`; `hw::Live::thread_ratio_mhz`; `rtss::snapshot` uses it for `cpu_clock` (max), the thread lines and the core table's clocks (`core_mhz` over `core_masks()`), PDH when empty (AMD, no driver). CPU page, Sensors and phone keep the counted average. Checked in a game against Afterburner side by side: identical on every thread (3395 P / 3494 E under a lower profile).
- **Core table load:** `rtss::snapshot` sets each core's load to its busiest thread's (`core_masks()` over `s.cores`), overlay only.
- **Afterburner look** (user: "look more like the msi look in photo with colors and everything"): items and order taken from the user's `MSIAfterburner.cfg` `Sources=` and OSD flags; colours sampled from an in-game screenshot = Afterburner's defaults (values/units/clock FF8000, CPU/RAM 0080C0, GPU/MEM 008040, FPS label/graph/frametime C08080, framerate FFFFFF). New `OsdStyle` fields: `fps_value_color` (C9, number + unit), `sub_numbers` (thread-table numbers `<S1=-50>`, padded with small spaces to whole characters; `label_width` counts them as half characters so `align` and `text_width` stay right), `graph_title` (lines `fttitle` = small "Frametime", `ftgraph` = the graph then the frametime small, both outside columns/align via `free()`). `Part` gained `color` and `small`; the preview JSON carries `sub`/`pad`/`small`/`c`/`sm` and `ovScreen` draws them (`.ov-subn`, `.sm`). Other looks reset the three fields (`NOT_AB`). New item `gpu_fb` (NVML `utilization.memory`, `Live::mem_util`, `Snap::gpu_fb`). Switches: Layout "Small thread numbers" (with the thread table), Graph "Afterburner layout".
- **Not checked in a game yet:** that RivaTuner draws `<S1=-50>` low (subscript, as assumed) and the new graph line. The user reported the graph "looks slow" before; it's already RTSS's per-frame FLAG_FRAMETIME graph (2.5.9), so that needs their description.
- **Claude Sync version warning:** `ccsync::config::compare_app` compares only the build time after `+`, so a dev build of 2.5.10 made the other PC's official 2.5.10 "older (2.5.10; this PC has 2.5.10)". `ccsync/mod.rs` now also sends `same` (equal `app_version`), and `pcs.js` says "runs a different build of Deck X (built earlier/later than this PC's)". PC to PC (beam) has no version warning on purpose: it only fails on an incompatible handshake (`device.rs` OLD).
- **Tests:** `afterburner_clock_is_the_ratio_times_the_bus`, `afterburner_numbers_framerate_and_graph`. All pass except `an_elevated_deck_never_runs_the_helper_from_a_user_folder`, which fails before this change too when the clone sits under `%LOCALAPPDATA%` (the exe's own folder counts as a helper folder).

## 2026-10-19: v2.5.10 — CPU clocks from each core's cycle counters (APERF/MPERF)

- **Released as 2.5.10:** version bumped in `tauri.conf.json`, `Cargo.toml`, `Cargo.lock`, DECK.md and README.md; the CHANGELOG's clock fix became `## 2.5.10`; tag `v2.5.10` pushed so `release.yml` builds and publishes the setup. Only the clock fix: the controller overlay and the death counter (still uncommitted) come in a later release. DECK.md's PDH gotcha now says APERF/MPERF, not `IA32_PERF_STATUS`.

- **Why:** a user's friend (same i5-14600KF) saw different clocks than MSI Afterburner after 2.5.9. A read-only check script (Windows' inputs + Afterburner's `MAHMSharedMemory`) in his game: Afterburner 5292 on every P-core (53 x 99.85), 3994 on every E-core (799 for a core that slept); Deck (PDH `% Processor Performance` x 3500) 4836-5227 and 3225-3878, max 5227. Windows' counter is an estimate from its own sampling; Afterburner's figures fit APERF/MPERF (not IA32_PERF_STATUS, which stays at the top step while a core naps, the 2.5.x bug).
- **Fix (`hw/sensors.rs`):** `Cpu::clock` reads every logical processor's MPERF/APERF (Intel 0xE7/0xE8, AMD's read-only 0xC00000E7/0xC00000E8; both PawnIO modules allow them) each tick; `counted_mhz` = TSC MHz x dAPERF / dMPERF (TSC rate measured once with `rdtsc`, `tsc_mhz`; base clock otherwise); a thread that ran under 1 ms gives nothing and `merge_clocks` takes Windows' value for it. Without the driver (or if every read fails once) it's Windows' clocks as before. Feeds the overlay, the CPU page, Sensors and the core table.
- **Tests:** `the_clock_comes_from_the_cores_own_cycle_counters` (the friend's figures), `a_thread_that_slept_has_no_counted_clock`, `windows_fills_in_where_the_counters_have_nothing`.

## 2026-10-18: v2.5.9 — overlay backend audit — readings checked against HWiNFO, Afterburner graph

- **Why:** user: "I'm sick and tired of finding bugs in this app, or having fake readings, or incorrect readings", focus on how the overlay works in the backend; the graph "looks ugly", make a better one but keep the classic one. Part of a whole-app UI audit (hardware pages the way the CPU page was done, overlay first).
- **Method:** read `rtss.rs` / `osd.rs` against RivaTuner's SDK (`RTSSSharedMemory.h`, its shared-memory sample, the Overlay Editor plugin's writer) and checked every reading live, mid-game, with read-only PowerShell readers of RTSS's and HWiNFO's shared memory (`HWiNFO_SENS_SM2`) and `nvidia-smi`. Matched: GPU temp/clock/power/load, memory clock (7001 = nvidia-smi's convention), CPU temp/power/load, RAM, game RAM, FPS (RTSS 120.1), avg 118.9 vs 119 and 1 % low 62.5 vs 62 from a 60 s count of RTSS's ring. VRAM matches HWiNFO's "Memory Allocated" (nvidia-smi's "used" leaves out driver-reserved memory, ~100 MB less): left as is.
- **Fixed — CPU clocks 20 % low:** `thread_mhz` used Windows' "Processor Frequency" as the base, but that counter is the *current* nominal clock and a power plan moves it (2784 P / 2600 E in a game under the Mid profile; 3500 when idle, which is why the 2.5.8 check passed). Every clock read 2150 where HWiNFO showed 27 × 99.8 = 2693. The base is now `base_mhz()`: the highest `MaxMhz` from `CallNtPowerInformation`, fixed (3500). After: P-cores 2679–2697 vs HWiNFO 2693; E-cores ~2700 vs HWiNFO's ratio-based 2793 (Windows counts the clock while running). Same code feeds the CPU page, sensors, core table and overlay.
- **Fixed — FPS from one app, lows from another; frozen FPS:** `fps_at` picked RTSS's foreground app else the fastest, the lows followed the app whose ring moved (`pick_app`); a launcher drawing faster could give the FPS. It now follows the stats' app (`FRAMES.1`), and skips apps whose last period ended over 2.5 s ago (RTSS keeps them). `FRAMES` is keyed on (slot, pid): a new game in a reused slot started with the old one's frames.
- **Fixed — fake preview values:** "API" was always "D3D12" and "Res" the desktop's size; now the game's own from RTSS's app entry (D3D11 1920x1080 for the game in the test), "–" with none. The preview graph drew a made-up trace with no game; now empty.
- **Fixed — overlay text over 4 KB was cut mid-tag:** text goes to `szOSDEx2` (32 KB) on RTSS 7.3.5+, as RTSS's own Overlay Editor does; older RTSS loses whole lines (`fit_lines`), `rtss_state.trimmed` says so.
- **Fixed — `tasklist` every 5 s:** the tray-icon removal found RivaTuner's window through a `tasklist` process each time, also in games; now `EnumWindows` by title. RTSS.exe is found through its registry `InstallPath` (custom install folders counted as not installed).
- **Graph: now RivaTuner's own (MSI Afterburner's), both looks.** After the user's in-game screenshots: the first modern graph (RTSS's background flag) was a red-tinted box with a 1-px scribble and a value outside the columns ("looks ugly as fuck"); a filled area fed 60 points a second was "way too many points"; the user asked for "exactly like the msi afterburner one", then "just replace both with the msi afterburner one with the same configs". Deck now writes an embedded graph with `GRAPH_FLAG_FRAMETIME` and no data: RTSS fills and redraws it every frame. `GraphFeed`, Deck's 60 Hz graph writes, `graph_secs` and `graph_smooth` are gone. Modern = the graph on an aligned FT `Line` (frametime once); classic = the graph under the text. Installed mid-game at the user's request (throttled builds); read back: flags 132 (frametime + autoscale), 0 points.
- **Graph: one look, auto top = four average frames.** User: "remove the options, modern and classic, keep the better one" (picked the FT line) and "make auto follow quadruple the current frame time average (if average is 120fps so about 8ms ... 32ms)". `graph_look` is gone; `graph_object(st, width, avg_fps)` sets the top to `graph_max` or `4000 / fps_avg` ms (no RTSS autoscale). The Graph tab's top slider shows "Auto · 33 ms" / "16 ms · 63 FPS" (`ovTopRow`, help `ov.graph_top`). Read back in a game at ~89 FPS: flags 4, top 45 ms.
- **Tests:** `the_base_is_fixed_whatever_windows_calls_the_current_clock`, `the_framerate_comes_from_the_game_the_figures_follow`, `a_game_that_stopped_drawing_has_no_framerate`, `the_api_and_resolution_are_the_games_own`, `long_overlays_go_to_rivatuners_big_text_field`, `a_too_long_overlay_loses_whole_lines`, `the_preview_never_makes_up_the_api_or_resolution`, `the_graph_is_rivatuners_own_on_a_line_like_the_others`, `the_frametime_is_shown_once`, `the_top_on_auto_is_four_average_frames` (231 total). Regress 40/40, clippy clean.

## 2026-10-18: v2.5.8 — CPU page rework, E-core clock fix, install.ps1 fix

- **Released as 2.5.8:** version bumped in `tauri.conf.json`, `Cargo.toml`, `Cargo.lock`, DECK.md and README.md; the CHANGELOG's Unreleased section became `## 2.5.8`; tag `v2.5.8` pushed so `release.yml` builds and publishes the setup.

### CPU page rework

- **Installed on moudi's PC (24c5ae1 + install fix):** user report "a lot of times the install does nothing, I have to use the GitHub installer". Checked: a UI-only change does rebuild (Tauri tracks `ui/`), and WebView2's cache holds no Deck UI files. The cause was `install.ps1`: with Deck running it printed a yellow warning in the elevated window, waited 4 s and exited 1 without copying. It now quits Deck (`--quit`, then kills Deck and its watchdog), retries the copy until the installed exe's hash matches the build (`Fail` keeps the window open 20 s otherwise), and starts `\Deck\Open` at the end. Verified: installed == build, Deck restarted from Program Files, launch test admin=true child=Medium.

- **Why:** this round's findings: a cap toggled on at the slider's top still capped (the friend's 5,300 MHz "Normal" cap), "Plan's own" EPP hid Balanced's saving lean, and the board's 130 W PL1/PL2 cost ~6.5 % all-core with nothing on the page showing it. Done with the impeccable skill (no PRODUCT.md/DESIGN.md: the existing app is the visual reference). User's choices: editor on the page (not a modal), plain settings first with Windows plan details under Advanced, power-limit status, no silent caps, plan's own values shown, per-core live view. Nothing CPU- or board-specific (user rule).
- **Backend:** `CpuProfile::without_silent_caps(top)` + `cpu::effective` (only when `limits().turbo_known`) in `cpu_save`, `write_plan`, `apply_extras`, `cpu_state`. `Shared.power_now` / `power_board` set by the power thread every 2 s (`powermode::board`: the captured stock register or the current one; `Rapl::decode`). `cpu_live` gains `limits`, `board`, `limits_by_deck`, `cores` (`profcmd::core_rows` from the sensor table). `cpu_state` profiles gain `plan_epp`. Tests: `a_cap_at_the_top_is_no_cap`, `limits_decode_from_the_register`, `cores_come_from_the_sensor_table` (223 total).
- **UI:** `ui/js/profiles.js` (Pages.cpu, `cpuRail`, `cpuEditor` / `editorBody`, `endSlider`: a slider whose end is off), `.cpu-page` / `.rail-*` / `.ce-*` in `app.css`, help texts `cpu.limit`, `cpu.turbo_mode` and updated cap / EPP / power / clock-floor entries. `editCpu(x)` now opens the on-page editor (still exported to `window` for tests). Regress tests 14b and 17 updated for the on-page editor; mock data has limits, board limits, cores and a plan-own EPP profile. Screenshots checked at 1260 and 900 wide; impeccable's detector findings in the new CSS fixed (bar fills animate transform, chevron icon).

## 2026-10-18: E-core clock fix, Cinebench investigation (shipped in 2.5.8)

- **Bug:** `thread_mhz` multiplied each thread's PDH `% Processor Performance` by its own `Processor Frequency`. On hybrid Intel the percentage is against the base clock (the TSC rate, 3500 on an i5-14600KF) on every core, so E-cores (2600 nominal) read 26 % low: 2.97 GHz under Cinebench while APERF/MPERF and IA32_PERF_STATUS (40x) said 4.0. Fixed: base = the highest `Processor Frequency`. Test `every_thread_counts_against_the_base_clock`.
- **User report "Deck lowers Cinebench scores / clocks" (moudi's PC, MSI MAG B660M Bazooka, i5-14600KF), measured with Cinebench R23 multi, one pass each, via an elevated throwaway test reading MSRs:**
  - The board sets PL1 = PL2 = 130 W (MSR 0x610; also what Deck captured as "stock"). At 130 W: P-cores 4.81 GHz, E 4.0, ~130 W, 73 °C, scores 21,370 / 21,893 / 21,989 / 22,030 / 22,049 / 22,003.
  - Deck closed vs running: no difference (21,893 / 22,049 off, 21,989 / 22,030 on; window open on Sensors 22,003). One early pass with Deck on scored 20,773: noise.
  - PL1 = PL2 = 253 W (written temporarily, then put back to 130): P 5.30, E 4.0, ~175 W, 88 °C, scores 23,411 / 23,446 (one earlier outlier 21,204).
  - Conclusion: Deck costs nothing; the board's 130 W cap costs ~6.5 % in all-core loads. Deck's CPU profile "Custom" power limits (PL1/PL2 sliders, any Intel CPU) lift it; the user wants it optional and nothing CPU- or board-specific (a "Rated 181 W" option was written and dropped). The BIOS cap was left as is.
  - The friend's lower game clocks: his "Normal" profile has Energy preference "Plan's own" (Balanced's) and a 5,300 MHz cap on; moudi's "Full" has EPP 0, no cap, Aggressive turbo.

## 2026-10-18: v2.5.7 — CPU clock fix, thick-client audit fixes

- **Released as 2.5.7:** version bumped in `tauri.conf.json`, `Cargo.toml`, `Cargo.lock`, DECK.md and README.md; the CHANGELOG's Unreleased section became `## 2.5.7`; tag `v2.5.7` pushed so `release.yml` builds and publishes the setup. Ships the CPU clock fix below and every thick-client audit fix (F1–F8).

### CPU clock fix

- **Bug (user report, overlay in Warframe):** "CPU" clock and the P-core/E-core rows sat at exactly 5.30 / 4.00 GHz while the thread rows (and Afterburner) moved. Cause: `Cpu::clock` and the per-core rows in `Cpu::details` read IA32_PERF_STATUS (MSR 0x198) bits 15:8, the current P-state ratio, which stays at the top step while the core is in C-states. Thread rows use PDH `Processor Frequency` × `% Processor Performance`.
- **Fix (`hw/sensors.rs`):** `Cpu::clock(&mut self)` reads the PDH thread clocks once per tick into `Cpu.mhz` and returns the fastest. `details` gives each core `core_mhz(mask, &mhz)` (its fastest thread) for every vendor. MSR 0x198 is now only used for the VID. Test `a_core_shows_the_live_clock_of_its_threads`.

## 2026-10-18: Thick-client audit fixes (shipped in 2.5.7)

A thick-client audit (OWASP Desktop App Top 10, TCASVS 5.0; report kept out of the repo until released) found the elevated
process still trusting user-controlled locations. All fixed with tests:
- **F1 (High):** `winget.exe` was started by name, which resolves to the user-writable alias in
  `%LOCALAPPDATA%\Microsoft\WindowsApps`. Now `beam::apps::winget_exe`: App Installer's package folder
  (`GetPackagesByPackageFamily` + `GetPackagePathByFullName`), only if `fsguard::user_could_change` is false.
  Test `winget_never_comes_from_a_folder_the_user_can_change`.
- **F2 (High):** elevated PowerShell auto-loaded modules from the user's Documents first (reproduced: a planted module
  replaced `Get-AuthenticodeSignature`). `rdesk.rs` now starts System32's `powershell.exe` with `PSModulePath` pinned to
  System32's modules, set again on the script's first line (UAC path); `$env:ProgramFiles` replaced by Rust's known-folder
  path. Test `a_planted_module_is_not_loaded`.
- **F3 (Medium):** RustDesk's MSI was hashed, signature-checked and installed from `%TEMP%\Deck-rustdesk` (swappable), any
  valid signer passed, and a missing digest skipped the hash. `install_script` copies it into a new random folder under
  `Windows\Temp`, holds it read-only, requires the digest and the publisher `O=PURSLANE` (read from RustDesk 1.5.0's EV
  certificate). Tests `the_install_checks_the_copy_it_installs`, `the_held_copy_can_still_be_checked`.
- **F4 (Medium):** `DECK_PROFILE` (user-settable) moved the settings folder and the HKLM seal key; a fresh key is a "first
  start" and trusted, bypassing the S8 hold. `settings::profile()` ignores it in an elevated release build; every reader
  (settings, seal, log, remote, share) uses it. Test `an_elevated_release_build_ignores_the_test_profile`.
- **F5 (Low):** an elevated Deck no longer falls back to `%LOCALAPPDATA%\Deck` for the 32-bit helper (`rtss::helper_dirs`).
- **F6 (Low):** `src-tauri/.cargo/config.toml`: `-Ccontrol-flow-guard` and `/CETCOMPAT` for x64; the helper gets CFG via
  `build.rs`. First build after the change rebuilds every dependency (about 4 min at 12 jobs). Test
  `deck_and_its_helper_are_built_with_exploit_mitigations` reads the PE headers. **Not yet run as the installed app.**
- **F7 (Low):** new `secret.rs` (DPAPI, CurrentUser): `device.key`, `remote_token`, the pairing `code` and `sync_key` sealed
  on disk (`dpapi:` prefix, `#[serde(with = "crate::secret::field")]`); plain values read and re-sealed on the next save; an
  unopenable value is replaced (pair again). Tests in `secret`, `the_key_file_is_sealed_and_an_old_plain_one_is_kept`,
  `the_phone_key_is_sealed_in_the_file`.
- **F8 (Low):** beacons carry a code fingerprint, so reordering the handshake would gain nothing. Instead `kdf::too_easy`
  turns down typed codes with repeats, runs or under 10 distinct characters (`kdf::gen_code` replaces both generators,
  rejection-sampled), and `util::Slot` caps each listener at 64 connections with 10 s to prove the code.
- 219 passed / 31 ignored; clippy `-D warnings` clean.
- **Installed and checked live:** the installed exe matches the build; Deck and its watchdog start through `\Deck\Open`;
  debug port closed; `dumpbin` shows Control Flow Guard and CET compatible; `remote_token` and the pairing code read
  `dpapi:` in their files and `device.key` is no longer plain PKCS#8; no "changed outside Deck" hold in the log. The
  installed `rtss-osd32.exe` is still the older copy until the overlay key is first used (Deck rewrites it then).
  PC to PC pairing, the phone page and the overlay not exercised by hand yet.
- **CI:** the first run failed in `toast-test.mjs`: headless Edge took longer than the 10 s wait, and the crash skipped the
  test's cleanup, leaving Edge running. Locally a crashed run left 8 Edge processes, after which reruns failed the same
  way. All four suites (`harness`, `a11y`, `phone-shot`, `toast-test`) now wait up to 30 s; `toast-test` kills its Edge on
  exit and fails with a clear message. Rust tests and clippy passed in that run.

## 2026-10-18: v2.5.6 — Clock and battery fixes, security remediation, faster tests, UI theme pass

- **Released as 2.5.6:** version bumped in `tauri.conf.json`, `Cargo.toml`, `Cargo.lock`, DECK.md and README.md; the
  CHANGELOG's Unreleased section became `## 2.5.6`; tag `v2.5.6` pushed so `release.yml` builds and publishes the setup.
  The six `docs/audits/2026-10-17-*` files can be committed once the release is out.

- **Overlay clocks (a friend's report):** the "Threads" table showed each core's base clock (3500 P / 2600 E) because sysinfo and
  `CallNtPowerInformation` only return the nominal value. Now PDH `Processor Frequency` x `% Processor Performance` per thread
  (`hw::sensors::ThreadClocks`, as Task Manager does). "Game RAM" now measures RivaTuner's game pid, not the foreground window.
  The non-admin overall clock uses the same per-thread math.
- **Battery:** profiles write both AC and DC values unless "Profiles also apply on battery" is off (`cpu_battery_same`);
  optional profiles for plugged in / on battery switch on a power-source change (`profiles::cpu::power_tick`; skipped while
  hardware is held or Full power is on). Sealed only when chosen.
- **Tools** is one page (owner's request). Other pages keep their tabs (owner: merge only pages they name).
- **Theme pass:** hard-coded Midnight greys replaced by tokens (`--line3`, `--knob` derived from each theme). Sidebar badges
  moved onto the icon (count bubble or status dot).
- **Tests are faster:** `cargo test` (dev profile, dependencies at opt-level 2, `debug = "line-tables-only"`) replaces
  `cargo test --release` in docs and CI: 201 passed / 31 ignored; first build 254 s, a rerun after one edit **8 s** (release
  rebuilt in minutes). Clippy runs in the same profile (99 s cold), clean. Two redundant tests removed (`boost::guid_round_trip`,
  `util::free_space_is_read`). UI suites wait for the page instead of fixed sleeps: `regress` 31 s -> 13 s (39/39, stable over
  3 runs), `a11y` 27 s -> 5 s (0 issues); harness files are deleted after a run.
- **Independent review of the newest Rust (a separate read-only review) found, now fixed with tests:**
  - A plug/unplug during Game boost or a hardware hold was used up and never applied; it now waits and switches once the
    hold ends (`power_step`, tests `a_power_change_while_held_waits_until_the_hold_ends`, `a_deleted_profile_is_not_switched_to`).
  - `rewrite_active` (turning "Profiles also apply on battery" on) ignored the seal hold and re-activated a plan the user may
    have left; it now returns while held and only re-activates the plan in use.
  - Deleting a CPU profile left it chosen for plugged in / on battery; delete clears both.
  - PDH capped `% Processor Performance` at 100, so per-thread clocks never showed boost; `PDH_FMT_NOCAP100` added (live,
    under load: E-cores read 2748 MHz against a 2600 nominal). `Cpu` now closes its two PDH queries on drop.
  - RivaTuner's shared-memory header was trusted (offsets/counts); a program creating the mapping first could make the
    elevated Deck read or write outside it. All readers/writers now use `rtss::array()` (bounds from `VirtualQuery`, read
    each time). Test `a_lying_rivatuner_header_stays_inside_the_mapping`; live dump unchanged (256 apps, 8 OSD slots).
  - Not changed: Game RAM now needs RivaTuner's game entry (by design: the overlay only exists inside RivaTuner).
- **Release check:** the 32-bit RivaTuner helper imported `VCRUNTIME140.dll` (Deck itself links the runtime in), so on a PC
  without the x86 Visual C++ runtime it couldn't start. `build.rs` now builds it with `+crt-static`; test
  `the_32_bit_helper_needs_no_visual_cpp_runtime` checks the embedded bytes. `dumpbin /dependents`: Deck and the helper import
  only Windows DLLs. The `STATIC_VCRUNTIME is deprecated` build warning is cosmetic (the static runtime still applies).
- **CI on Rust 1.99:** the first push failed clippy: 1.99 added `chunks_exact_to_as_chunks` (10 places; local was 1.97).
  Now `as_chunks::<N>()` (same whole chunks, remainder dropped as before). Local toolchain updated to 1.99.1 to match CI;
  clippy clean, 205 tests pass.
- **CI runs tests as admin:** four fsguard tests failed there (an admin may write to System32). The tests now check with a
  copy of their own token where Administrators only denies, so they test an everyday user on any runner.
- **Faster CI:** the cache was never saved (it saves only after a passing run, and every run had failed), so each run compiled
  everything; now `cache-on-failure`. The setup is built only on a `v*` tag (release.yml, Tauri CLI as a ready-made binary),
  and a newer push cancels the older run.
- **cargo audit (first run, in CI):** rustls 0.23.45 had RUSTSEC-2026-0346..0349; updated to 0.23.46 (and the yanked yoke-derive
  0.8.3 to 0.8.4). Remaining warnings are Linux-only Tauri dependencies (glib, zbus/zvariant) and proc-macro-error.
- **Final artifact:** `cargo tauri build --bundles nsis` with `DECK_REQUIRE_RTSS32=1` (2 m 40 s at 12 jobs): `Deck_2.5.5_x64-setup.exe`,
  7.6 MB; `deck.exe` holds 3 PE images (Deck, the RivaTuner helper and one more, as before) and no `vcruntime140.dll` import
  anywhere. Not installed (Phase 4: no install without the owner).
- **Performance check (running build, hidden, no game):** Deck 1.88 % of one core over 60 s (P1 was 1.88 %, Phase 1 ~3 %),
  watchdog 0 %, WebView2 0.03 %. Memory: three read-only tours of every page over CDP left private bytes at 323-327 MB
  and handles at 1225-1227 (no growth; the one-time rise from 221 MB is caches filled on first visit). Live page switches
  9-100 ms, except Clipboard at 150-206 ms: one 236,000-character item went onto the page in full. Fixed (cards show 600
  characters, the preview 20,000; regress check added, 40/40). Not measured: startup/shutdown (needs an elevated restart).

## 2026-10-17/18: Finishing the engineering audit — every finding addressed (not committed)

After Phases 1–3 the owner said to finish everything. Every finding in
`docs/audits/2026-10-17-findings-register.md` is now fixed, documented or decided; the register has a
row for each, with its tests. **Nothing is committed or pushed.** The CHANGELOG `Unreleased` section
has the user-visible list.

**Owner decisions:**
- **S10:** hold synced hooks and permissions and ask on each PC.
- **S6:** installers from another PC always ask on this PC.
- **Compatibility:** breaking it is fine; update both PCs and re-pair once.
- **S12:** keep custom codes and add a slow KDF.

**Security (all with tests):**
- **S1, install location.**
  - The setup is per-machine (Program Files); `hooks.nsh` removes the per-user copy.
  - Set up refuses an exe the user could change; an admin start removes such tasks, with a notice.
  - Task XML can't be swapped while `schtasks` reads it.
- **S2, DLLs.** Vendor DLLs load by full path from System32 or NVSMI. System folders come from Windows,
  not the environment. `SetDllDirectoryW("")`.
- **S4, launching.** `launch_as_user` never falls back to an elevated launch.
- **S6, PC to PC.** Each PC has its own Ed25519 key, and its id is derived from that key. Trust is
  per key. Received installers always ask and run as the user (`--run-installer`).
- **S7 and S8, file work.** New `fsguard.rs`: every file action the elevated process takes in user
  folders is checked against the user's limited token and done relative to an open folder handle.
  Sharing, sending and serving refuse what the user can't read.
- **S8, hardware settings.** New `seal.rs`: changes made outside Deck hold the hardware until the user
  clicks **Apply them**.
- **S9 and U5, phone.** The phone signs each request (HMAC, single-use counters), with a Host check and a
  hash CSP. The key never travels.
- **S10, Claude Code sync.** `ccsync/guard.rs` holds risky Claude Code settings.
- **S11, share links.** Downloads reserve their slot. Home links aren't served over Funnel. Funnel is
  reconciled at start.
- **S12, codes.** `kdf.rs` makes pairing codes and sync keys slow to guess.
- **S13.** Documented as an accepted risk.
- **S14, quoting and secrets.** Every quote form is escaped. The RustDesk password stays off
  PowerShell's command line.

**Reliability:**
- **R3:** `parking_lot` or poison-tolerant locks.
- **R4:** the watchdog process (`watchdog.rs`, `--watchdog <pid>`).
- **R5:** `log.rs`, whose tail goes into the support report.
- **R6:** `plan.json` keeps the "Shut down when…" plan.

**Engineering:**
- **T1:** CI with clippy `-D warnings`, the UI suites, `cargo audit` and the NSIS setup with its
  SHA-256. `release.yml` builds releases from `v*` tags. `DECK_REQUIRE_RTSS32`.
- **T2:** boundary tests, including the new `beam::net::boundary_tests`.
- **T4:** the ignored-test risk table.
- **U1–U3:** accessibility fixes.
- **A6:** clippy warnings 112 → 0.
- **A7:** ES modules.
- **P2:** one NVML instance; the `nvml-wrapper` dependency is removed.
- **P1:** measured; no change needed.

**Docs:**
- `SECURITY.md` and `docs/trust-boundaries.md` (D3) are new.
- README: install location, PC to PC row, CI, ES modules.
- DECK.md:
  - §0: tests, clippy, the `window` names, new gotchas.
  - §1: install, watchdog, log.
  - §5: new modules and the UI module list.
  - §6: CI, releases, command-line modes.
  - §7: `plan.json`, `device.key`, logs, seals.
  - §8: known issues.
- The migration report (Steps 4–7 and decisions), the test plan (the T2 map and the T4 table), the
  findings register and open questions.

**Migration notes for the owner and friends:**
- Update **every** PC that uses PC to PC or Claude Code sync; old and new versions don't talk. Then
  re-pair once: trusted PCs and "always allow" are asked again, and "off" is kept.
- A per-user install: run the new setup (one UAC prompt), then click **Set up** again.
- The phone page reloads once.
- A second `deck.exe` in Task Manager while hardware control is on is the watchdog.
- The first admin start with this version records the seals; nothing is held.

**Verification:**
- **Latest full run (before the three `boundary_tests`):**
  - `cargo test --release` 194 passed, 0 failed, 29 ignored;
  - clippy `-D warnings` clean;
  - `regress.mjs` 39/39, `a11y.mjs` clean;
  - `toast-test`, `phone-shot` and `phone-sign` pass;
  - all 19 UI modules resolve.
- **Ignored tests run here:** `share::one_time_link` and the shared NVML test passed. The NSIS setup
  builds, and is per-machine in the generated script.
- **Measured:** P1 at 1.88 % of one core.
- **The three `beam::net::boundary_tests` are written but not yet compiled.** The final build and test
  run comes next.

**Not exercised live:**
- running the setup;
- the watchdog on real fans;
- phone uploads with signing;
- the "changed outside Deck" hold on a real admin start;
- the ES-module UI in the real app;
- the CI and release workflows on GitHub;
- the two-engine network tests (they bind 0.0.0.0).

**Still open (optional or policy):**
- the rest of the `main.rs` extraction (A1);
- a generic store with migrations (A4);
- statics and crate convergence as modules are touched (A3, A5);
- a hardware critical-temperature override check (the rest of R4);
- a per-session confirmation for phone screen control (S13).

## 2026-10-17: Incremental architecture refactor — thin `main.rs`, step 1 (Phase 3)

First migration step of the approved architecture plan (`docs/audits/2026-10-17-architecture-plan.md`,
item 4 "Thin `main.rs`"; tracked in `docs/audits/2026-10-17-architecture-migration.md`). **Pure code
moves, no behaviour change, no user-visible change (so no `CHANGELOG` entry). Not committed.**

- **Problem (finding A1).** `main.rs` was 2,122 lines holding 98 inline `#[tauri::command]` adapters
  across a dozen unrelated domains, mixed with tray/window/setup wiring. Finding one domain meant
  reading the whole file.
- **Change.** Extracted the command adapters that only translate JSON and delegate to a service module
  into per-domain `*cmd.rs` files, following the existing `hwcmd.rs`/`profcmd.rs` convention (a
  deliberate deviation from the plan's sketched `cmd/` subdir — the flat convention already exists, so
  less churn). New modules: `audiocmd.rs` (5), `displaycmd.rs` (6), `netcmd.rs` (7), `diskcmd.rs` (13),
  `syscmd.rs` (7, incl. the This-PC report), `boostcmd.rs` (4), `phonecmd.rs` (14). 56 commands moved.
  `main.rs`: 2,122 → 1,734 lines. Each command's signature, name and `generate_handler!` registration
  are unchanged (registered as `module::command`).
- **Deliberately left in `main.rs`:** the ~40 commands coupled to main's statics, tray, window helpers
  or file dialogs (window control, clipboard + `paste_back`, power/timer, toast, workspaces, screen
  tools, `pick`/`pick_many`, `share_to_phone`, `elevate_setup`). Moving those needs the helpers/statics
  restructured first — a separate, more careful step, not done here.
- **Verification.** `cargo check` clean after each domain (no new warnings — only the 7 pre-existing
  dead-code ones). Full `cargo test --release -j 6`: **140 passed, 0 failed, 29 ignored** — identical to
  the Phase 2 baseline. No new tests (behaviour-preserving move; the compiler + existing suite are the
  guard). `DECK.md` code-layout table updated.

Then, same day, three more architecture steps (all **not committed**):

- **Atomic-write consolidation (plan item 3, partial).** Pointed the costly-to-lose JSON stores at the
  single `util::write_atomic`: `clipboard::save` (was tmp+rename), `beam/config` `save_cfg`+`save_inbox`
  and `boost::save` (were plain `fs::write`, so a crash mid-write truncated them). No success-path change;
  gains crash atomicity + `sync_all`. Left the `ccsync` content-addressed chunk store (own `write_atomic`
  with parent-dir creation and `.ccsync-tmp` filtering — not worth the coupling) and the regenerated
  currency-rate / app-list caches as-is. 140/140 still green.
- **`never_loop` clippy error fixed (finding A6).** `beam/net.rs::request_job` wrapped a single
  request/answer in a `loop` whose every arm returned (a no-op loop); removed the wrapper, behaviour
  identical. `cargo clippy` now completes with no errors (100 pre-existing warnings remain), unblocking a
  future clippy CI gate.
- **Decisions (evidence-based deviations from the plan, see migration report):** (1) *No unified
  `actions::` layer* — the phone's capabilities are already a single explicit allow-list in
  `remote::handle` and the one cross-tier hole (S5) is fixed, so a layer would be abstraction without
  benefit; added a trust-boundary doc comment to `remote::handle` instead. (2) *Frontend ES-module
  conversion deferred* — 18 globals-sharing scripts, ranked last by the plan, and the headless suites
  can't validate focus/interaction, so it's unsafe to do autonomously. Recommended approach is in the
  report.
- **Verification across all steps:** full `cargo test --release -j 6` **140 passed, 0 failed**; `cargo
  clippy` no errors; `regress.mjs` **37/37** incl. "no script errors"; command names unchanged so the UI
  `invoke` contract is intact. Report: `docs/audits/2026-10-17-architecture-migration.md`.
- **Still open:** finish extracting the window/dialog-coupled commands (optional — they belong with main's
  lifecycle); a generic `store::` + settings migrations (only when a real migration is needed); the
  frontend ES modules; clearing the 100 clippy warnings to enable a `-D warnings` gate.

## 2026-10-17: Security & reliability remediation (Phase 2 of the engineering audit)

Fixed the confirmed high-priority findings from `docs/audits/2026-10-17-*`. TDD throughout: every fix
has a test that failed first (watched 13 fail), then passed. **Not committed** (user asked to hold).

- **S5 — phone could run any program (High).** `app::toast_action` acted on a toast id/action with no
  check that the toast existed. The phone's `"toast"` handler passed its id/action straight through,
  and a `recv:<path>` id made Deck `shell_open` that path. New `app::toast_allowed(caller, id, action,
  received)` + `toast_action_as(caller, …)`: PC UI = `Caller::Local`, phone = `Caller::Phone`. The
  phone may answer only its own notifications (media/power/timers/text, and `dismiss` on any). A
  received-file "open"/"show" needs a path that is actually in `app.shared` from that phone, and
  "open" is PC-only. Transfers (`xfer:`) and `gpu-held` are PC-only. 4 tests in `app::toast_rules_tests`.
- **S3 — elevated WebView took flags from the environment (High).** `browser_args` appended
  `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS`, user-settable, so a local program could add a DevTools
  port into the admin process. Now `browser_args_for(env, elevated, cli_port)` drops the env var when
  elevated; `scrub_env(elevated)` removes every `WEBVIEW2_*` var at the very top of `main()`; the
  debug port comes from `--cdp-port` (new `cdp_port(args)`), which only the launcher passes. Test
  scripts `elev-install.ps1` / `restart-debug.ps1` now pass `--cdp-port=9222`. 4 tests in `webview_guard::tests`.
- **R1 — settings silently reset on a damaged file (High).** `Settings::load_from(dir)` now returns a
  note; a damaged file is kept as `settings.json.corrupt-<time>`, the `.bak` is tried, else fields that
  still parse are merged into defaults (`merge_into_default`). `save_to` keeps the previous good file as
  `settings.json.bak` and writes atomically. The note is toasted once in `App::start` ("Settings
  recovered"). 5 tests in `settings::durability_tests`.
- **R2 — crash guard failed open (Medium).** `gpu::guard` read/parse errors meant "don't hold". New
  `read_file` → Missing / Mark / Damaged; a Damaged (cut-off) mark now **holds** a risky overclock.
  Marks are written atomically. 3 tests in `gpu::guard::damaged_file_tests`.
- **S15 + atomic writes (Low/High).** `util::random_token` uses rejection sampling (no modulo bias) and
  panics on RNG failure instead of making `aaaa…`. New `util::write_atomic` (unique temp + `sync_all`
  + rename-with-retry) backs R1/R2. Tests in `util::token_tests`, `util::atomic_tests`.
- **T3 — stale/incomplete UI checks.** `regress.mjs` check 18 opens the Look tab (the font list moved
  there in 2.5.5) and check 19 pins the Items tab, so tab order no longer matters. `all.sh` gained
  Overlay, Drives, PC to PC and Claude Code sync, so `a11y.mjs` now covers them (0 issues).

**Deferred, documented in the audit (not done this pass):** S1 (release setup is per-user → elevated
tasks point at a user-writable exe) — needs a per-machine installer **and** a folder-ACL guard
together, plus live installer testing on a UAC-on VM, which can't be done here; the maintainer's own
install is Program Files, so it's unaffected. S2 (DLL search path), S4, S6–S14, R3–R6, A6's
`never_loop` clippy error, and the T1 CI work remain open — see the register.

**Tested:** `cargo test --release` 140 passed / 0 failed / 29 ignored (was 122; +18 new). `cargo
clippy` — no new warnings in the 5 changed files (whole-crate: still the 1 pre-existing `never_loop`
error). `node --check` all 20 scripts. `regress.mjs` 37/37. `toast-test.mjs` 6/6. `a11y.mjs` 1260×900
clean incl. the 4 added pages. **Not tested (no safe environment):** live phone remote, live GPU/CPU
crash-guard behaviour on a real reset, installers. No hardware was changed.

## 2026-10-16: v2.5.5 — Phone page redraws, code review fixes

- **Overlay page redesign** (owner: "very ugly, very difficult to use, especially with this many things to edit and this much scrolling"; done with /impeccable). Kept Deck's look; changed the structure.
  - Before: seven stacked cards, about 4,300 px tall at 1260×900, with a small preview that scrolled away.
  - Now: `ovStrip` (the switch, state, show/hide key, compact limiter), then `.ov-cols`: `ovStage` (sticky preview plus quick looks) beside `ovEditor` (tabs: Items, Layout, Look, Colours, Graph, Warnings, Behaviour). Tabs use the ARIA tablist roles and arrow keys. The open tab is kept in `S.cache.ovTab` and `localStorage` `deck.ovTab`.
  - Rows (`ovRow`) are flex: the control stays beside its name and wraps only when there is no room. The old container query that put every control under its label is gone. Items rows are 34 px, with actions shown on hover or focus. "Add readings" opens on demand (`S.cache.ovAdding`), and is open when the list is empty.
  - Removed the "large preview" toggle (`deck.ovBig`); the preview is large now.
  - Checked with the mock harness at 1260×900 and 900×800, across every tab, a selection and the open adder. The Impeccable detector reports nothing.
- UI harness (`tools/testing/ui-preview`):
  - Every script now picks the `about:blank` tab. Edge opened a sync dialog first and the scripts timed out on it.
  - `mock.js` gained `fonts_list` and `rtss_limiter`. A null reply made the Overlay page fetch again on every render, an endless loop.
- **Home → PC tile without admin** (friend's screenshot): the CPU card showed "20 threads" with no clock, power or temperature, and the tile looked uncustomizable.
  - Cause: `hw::start` runs only as admin, so `cpu_now` is null. The GPU card comes from NVML and was complete.
  - Now: the CPU card's last line is a *Needs admin* link, and the customize dialog shows a banner. Both call `elevate_setup`, the same as the CPU and GPU pages' Set up.
  - The cards were already customizable, from the sliders icon in the tile header.
- Docs pass after the release (owner asked to update the docs and the README).
  - README: the Overlay row (live preview, limiter keys and limit), the Home row (network card, the settings icon), the admin paragraph (CPU readings, *Needs admin*), and the `rtss.rs` row.
  - DECK.md: the limiter mechanics, `limiter_hotkey`, the `rtss.rs` and `rtss32/` rows (a missing-helper warning blocks a release), and `pc_cards` with the admin fallback.
  - `docs/screenshots/overlay.png` regenerated from the mock harness at 1260×860.
- **32-bit RivaTuner helper was never in the setups.** The i686 Rust target wasn't installed on the build PC, so `build.rs` skipped `rtss-osd32.exe` with a warning. Released builds could neither read nor flip RivaTuner's 32-bit switch, the one its own window shows; that is the root of the limiter-key report. The target is now installed (`rustup target add i686-pc-windows-msvc`). The 64-bit read added above works without the helper anyway.
- Report (owner): Open RivaTuner showed a busy cursor for a moment and nothing opened. RivaTuner had restarted (new RTSS.exe), and its main window existed but was hidden: its config had `StartMinimized=1` (Profiles\Config, [Settings]), so the fresh start that `open_rtss_window` relies on stayed in the tray, with the tray icon hidden by Deck. Now: set it to 0 before the start, wait for `IsWindowVisible` on the "RivaTunerStatisticsServer" window (up to 5 s), read the file again and set it back to 1. Not tested live (owner can't test now).
- Report (owner, screenshot): on the Phone page some Screen view options "can't be changed". They were saved; the page didn't redraw. The `changed` listener in `app.js` only calls `renderSide()` for the "settings" topic, and `Pages.phone.tick` only redraws when `remote_info` changes. The Phone page's controls now do `setSetting(k, v).then(render)` (the codebase's idiom).
- A review of that change, then a full review of 0fc0f76..working tree (2.5.4 + this). Fixed:
  - `render()` keeps `<details>` fold cards open across redraws of the same page/sub-tab (by summary text); before, every redraw (also the live tick) folded them shut. A card that is open by default and was closed by the user opens again (unchanged).
  - `setSetting()` puts the previous value back when `set_setting` fails (only if no newer write for that key is pending).
  - Bitrate "Type…": Cancel / not a number keeps the old value (was saved as 0 = Auto).
  - Phone limits: `admit()` now checks and registers under one `CONNS` lock and returns the `Registered` guard, moved into the upgrade closure (two phones at once could both pass a limit of 1). The rule is a pure `decide(list, ip, Limits)`; `allowed()` applies it to `/upload`, `/upload` GET, `/file/{id}` and `/art` (403 with the reason; the phone's upload shows it). Refused sockets get a Close frame. The guard is dropped before the last `changed("remote")`, so the count is right at once. The "Screen view is off" check now runs before admitting.
  - Screen view tap-then-drag: `md` goes out at the finger's position once it has moved past the 10 px start, not where the second touch landed (the tap had just clicked there: Windows made it a double-click, maximizing a title bar or opening a file). No long-press right-click timer during tap-then-drag.
  - Hotkey poll: 15 ms in a game (one timer tick, ~15.6 ms; 30 ms missed quick taps), `held` refilled from the real key state on reload / game switch (clearing it fired a held key again), the id returned by `SetTimer` is kept.
  - Limit select shows a value that isn't in its list.
  - Tests: `conn_tests` (`is_online` for home/away/IPv4-mapped/IPv6 addresses, a phone counts once, `decide` limits and shut-outs).
- Friend's laptop report (RTSS 7.3.4; can't be tested here or on the owner's PC):
  - **Limiter key on, never off:** `toggle_limiter` used `limiter_on().unwrap_or(false)`; `limiter_on` only asked the 32-bit helper, so when that read failed every press was "turn on". Now `limiter_on` reads `GetFlags` from `RTSSHooks64.dll` first (Deck sets both copies; 64-bit games use this one), then the helper, then `LIMITER_SET` (what the key set last).
  - **Limit not synced (Deck 5, RivaTuner window 0):** `set_limit_fps` only rewrote `Profiles\Global`; a running RivaTuner keeps its own copy. Now, after the file write (which also sets `LimitDenominator=1`, the API has no property for it), `hooks64()` calls `LoadProfile("")`, `SetProfileProperty("FramerateLimit", u32)`, `SaveProfile("")`, `UpdateProfiles()` — the sequence of the SDK's RTSSSharedMemorySample `IncProfileProperty` (exports and the `FramerateLimit` property name checked in this PC's RTSSHooks64.dll, 7.3.7) — and posts `WM_RTSS_UPDATESETTINGS` (WM_APP+100) to RivaTuner's window. `hooks64()` keeps the DLL once loaded and retries until then.
  - **Fn + numpad 9 recorded as "Num9":** `keyName` used `e.code` ("Numpad9") even when the key sends Page Up (`e.key`); a numpad key that sends a navigation key is now recorded as that key (`VK_PRIOR` etc., what `RegisterHotKey` and `GetAsyncKeyState` see).
- Not done (reviewed, left): a WebSocket ping to drop dead phone connections sooner (a phone switching Wi-Fi → Tailscale can be refused by its own stale connection until TCP gives up; the 10 s retry recovers); unit tests for the hotkey de-duplication state machine.

## 2026-10-16: v2.5.4 — shortcuts in League, phone connections, touchpad-style Screen view

- **Shortcuts in League (owner, top priority):** the overlay showed in League but PageUp did nothing in a match. Deck's shortcuts are `RegisterHotKey`; Windows doesn't deliver them while the foreground app registered raw input with `RIDEV_NOHOTKEYS` (what League appears to do; RivaTuner's own keys still work because RTSS runs inside the game). `profiles/hotkeys.rs` now also polls every bound key with `GetAsyncKeyState` on the same thread while a full-screen game is in front (`hw::game::foreground_game`, checked once a second; a second thread timer next to the 30 s retry runs at 1 s and switches to 30 ms with a game, so idle cost is one wake a second; a windowed game is not covered): on a press with exactly its modifiers (`combo_down`, so Ctrl+PageUp is not PageUp) it waits 100 ms for `WM_HOTKEY`; if none came it runs the action itself (`run()`), and a `WM_HOTKEY` arriving within 600 ms after a polled run is ignored, so a press never toggles twice. Keys another program owns ("busy") are not polled. No hooks, no injection (DECK.md rule).
- **Phone connections (friend's report):** the count was raw WebSockets (`remote_clients`): the remote page, the Screen view and its sound each added one. `remote.rs` now keeps `CONNS` (one entry per open socket with the peer IP, from `ConnectInfo`; the server is served with `into_make_service_with_connect_info::<SocketAddr>()`), counts distinct IPs as phones and splits them: away = Tailscale (`rdesk::is_tailscale`) or any non-private address; home = private, loopback, link-local. `remote_info` adds `local` and `online` (`clients` = their sum). New settings `remote_max_local`, `remote_max_online` (0 = no limit), `remote_local_blocks_online`, `remote_online_blocks_local`, checked by `admit()` in `/ws`, `/screen` and `/audio`; a phone already connected is always let in (another page or view of the same phone). A refused WebSocket is accepted, sent `{"t":"refused","text":…}` and closed (a browser can't read an HTTP refusal); the phone shows the reason and retries every 10 s. Limits apply to new connections only.
- **Screen view dragging (friend's report):** one-finger slide sent `md` at the start, so it always dragged. Now slide = `mm` only; tap then touch again within 350 ms and 40 px = drag with the left button (`md` … `mu`); hold = right-click as before. Help text on the phone updated.
- Tested on this PC against the installed 2.5.4: with "phones at home" = 1, a WebSocket from 127.0.0.1 was let in, one from the LAN address was refused with the limit text, and a second page from 127.0.0.1 was let in (same phone); `remote_info` reports `local`/`online`. NOT tested on a real phone (drag gestures, the refusal screen) or in a League match (the shortcut fallback).

## 2026-10-15: v2.5.3 — the board lighting config is checked and re-sent

- Report (owner's PC, 2.5.2, support report attached): all lighting controls dead, any effect, on or off; the lights test didn't work either; the report said "Motherboard RGB connected". Restarting Deck did not help. RAM lighting "not found" in that report is irrelevant: this PC has no RGB RAM (the friend's has).
- Found by talking to the controller from a separate process (Python ctypes, `HidD_GetFeature`/`SetFeature` on `vid_1462&pid_7d43`): it answered instantly (not hung), but feature 0x52 held a foreign config (`01 ff 00 00 28 …`) instead of Deck's (`01 00 00 00 08 …`). Writing Deck's `config_packet(60, 60)` was accepted, read back identically in the first 60 bytes and then stayed for 36 s. While the board holds another config it ignores every 0x53 colour report, with no error, and Deck only wrote the config at connect/reconnect (start, wake, send failure), so it never noticed. What reset the config is not known (no other lighting program was running; the PC had been restarted at 21:44).
- Fix: `Board::config_intact()` reads 0x52 and compares `[..60]` with `config_packet` (bytes 62 and 82 are rewritten by the board, so a full compare never matches: checked on the real board). The board loop checks every 5 s and sets `reconnect` (which rewrites the config) when it differs. `config_packet()` split out of `send_config()`.
- Second fix: `hw::lights_test()` is the one place that runs the lights test (hold flag, `rgb_diag` on a thread with a 40 s limit, always releases the hold, reconnects). `hw_test_lights` and `support_report(lights)` use it. Before, a hung `HidD_GetFeature` or a panic left `rgb_hold` set: every lighting control frozen and "already running" for later tests. The board loop also drops a hold older than 60 s. (This was the first theory; restarting Deck didn't fix the report, so it was not the cause here, but it is a real weakness.)
- NOT tested: that the self-heal fires on a real config loss (the loss itself could not be reproduced); the build compiles and the packet compare was checked against the live board.

## 2026-10-15: v2.5.2 — the closing hook also works without the Quit task

- Report: the uninstaller still shows "Failed to kill Deck" after 2.5.1. Two reasons found by reading, not yet reproduced on a second PC: (1) `uninstall.exe` in the install folder is the one written by the installed version, so a Deck older than 2.5.1 still has the old one; (2) `\Deck\Quit` is only created by `--setup-task` or by an elevated Deck on startup, so a fresh 2.5.1 that never ran elevated has none, and the hook had nothing to start.
- Fix: `hooks.nsh` `DeckQuitIfRunning` now: runs the Quit task if Deck is running; if still running, `ExecShell "runas" deck.exe --quit` (one UAC prompt); waits up to 8 s after each step (`DeckRunning` / `DeckWait` macros). If both fail the template's own message appears as before.
- Tested on this PC (normal-user shell, Deck elevated from Program Files, NSIS copy per-user in %LOCALAPPDATA%\Deck): the setup (2.5.1 and 2.5.2) and the uninstaller both closed the elevated Deck and finished, with the Quit task; the uninstaller also finished with the Quit task deleted (the runas fallback; UAC is off here, so no prompt was seen). A freshly installed 2.5.2 Deck creates the Quit task itself within seconds of starting elevated.

## 2026-10-15: v2.5.1 — setup and uninstaller can close an elevated Deck

- Report (screenshot from a friend): uninstalling says "Deck is running! Click OK to kill it", then "Failed to kill Deck. Please close it first then try again". Cause: Deck runs elevated (its logon task); the NSIS setup/uninstaller runs as the normal user (`installMode: currentUser`), and Tauri's template kills with a user-level `taskkill`, which Windows refuses for an elevated process. `hooks.nsh` already had a PREUNINSTALL hook (Claude Code cleanup) but nothing closed Deck.
- Fix: `elevation::QUIT_TASK` (`\Deck\Quit`, no trigger, runs `deck.exe --quit` elevated). A normal user may start a task. `hooks.nsh` macro `DeckQuitIfRunning` (PREINSTALL and PREUNINSTALL, both run just before the template's own running-app check) starts the task and polls `tasklist` up to 8 s. `setup()` creates the task; `ensure_quit_task()` (thread at startup, admin only) adds it to installs that predate it and re-points it when Deck moved. A clean `--quit` also hands the fans back to the BIOS.
- Test: `quit_task_runs_quit_without_trigger`. Built with `cargo tauri build --bundles nsis` (2 jobs, idle priority, CS2 was running).

## 2026-10-15: v2.5.0 — PC to PC, Claude Code sync, share links, RivaTuner tray

- Released as **2.5.0** (2.0.1 was never released; its changes are part of 2.5.0).
- **RivaTuner tray icon:** the user didn't want it moved under the tray arrow (Windows' `NotifyIconSettings\IsPromoted`), but removed. RivaTuner has no setting for this; its strings only show `StartMinimized` and `SendToTrayOnClose`. Found by trying: the icon belongs to the window titled "RivaTunerStatisticsServer", and its id is that window's handle. Explorer accepts `NIM_DELETE` from another process. `rtss::set_tray_icon(false)` removes it and the rtss loop repeats this every 5 s while `rtss_tray_hidden` is on. `set_tray_icon(true)` posts `TaskbarCreated` to that window, and RivaTuner adds the icon back. Checked on the laptop: the icon is removed, RivaTuner keeps running, and Deck removes it again within 2 s after RivaTuner re-added it. `rtss_open` restarts RivaTuner (taskkill, then start). `ShowWindow` on either RivaTuner window from outside left it black, with most of the UI missing (user report). UI Automation finds the tray button but offers no Invoke on it. A fresh start opens the window properly (seen by the user). `start_rtss` now checks for RivaTuner's window, not its shared memory. Deck keeps the shared memory open after RivaTuner exits, so a RivaTuner that had closed was never started again; the first Open RivaTuner closed it and left it closed. Commands `rtss_tray` and `rtss_open`; ignored test `rtss_tray_icon`.
- Removed the phone's temporary debug logging (`vlog`, and the PC's `deck-phone.log` handler).

- **AppBeam and ClaudeSync are now part of Deck** (user's request: integrate them properly, Claude Sync off by default; chosen: native pages, Deck-only protocol). Engines copied into `src-tauri/src/beam/` and `src-tauri/src/ccsync/` with `crate::` paths rewritten; their egui UIs replaced by `ui/js/pcs.js` (sidebar: **PC to PC** above Phone; **Claude Code sync** under Desktop, hidden unless `claude_sync_nav` (Settings → Sidebar) or it is on). Deck-only: new ports (47833/47834, 47835/47836), hellos `DKT1`/`DKC1`, Claude Sync's key derivation string and hub folder renamed, so the standalone apps never mix with Deck's.
- Glue: `beam::set_on/state/act` and `ccsync::set_on/state/act` behind the commands `transfer_state`, `transfer_do`, `transfer_apps`, `transfer_setup_preview`, `claude_sync_state`, `claude_sync_do`, `pick_many`. Settings `transfer_on` (true) and `claude_sync_on` (false). Off = the `off` flag: no beacon, no answers, the accept loop drops connections; Claude Sync off also removes its hooks (`Engine::switch_off`). Offers and requests show as sticky toasts `xfer:<id>` with Accept/Decline (`App::toast_action`). `App::ui_changed` refreshes Deck's page without telling the phone. Firewall: `remote::allow_between_pcs` adds "Deck between your PCs" (program, any protocol, private/domain).
- Claude Code hooks: `deck.exe --claude-sync-hook` is handled first thing in `main()` (no Tauri); the hook runs a copy, `%LOCALAPPDATA%\Deck\claude-sync-hook.exe`, so a running hook never blocks an update. `is_ours` matches `--claude-sync-hook` only, so the standalone ClaudeSync's hooks are left alone (checked). NSIS `hooks.nsh` runs `--uninstall-cleanup` (not on updates) so Claude Code isn't left calling a missing program; `uninstall.ps1` does too and removes both firewall rules.
- `serde_json` now has `preserve_order` (Claude Code's settings.json keeps its key order when the hooks are edited); every test still passes.
- Tests: 118 pass (+27 ignored). New ignored loopback tests (`cargo test --release -- --ignored two_engines`): Transfer (5 MB file byte for byte, project folder without build output, request from a shared folder, an engine switched off isn't found) and Claude Sync (a session made on A arrives on B with A's home path rewritten to B's; no hooks while their settings are off; off gives phase "Off"). The hook checked with the real exe: blocked session exit 2 with Deck's message, others 0, no Deck started. UI checked with the preview harness (all tabs; `shot.mjs` now falls back to Chrome when Edge isn't installed). NOT tested yet: two real PCs (the laptop needs this Deck), the cloud folder, a real winget install, applying a setup, the NSIS uninstall hook (needs a full `cargo tauri build`).
- Later the same day: PC to PC trust (`Config.trusted`, `trusts()`, settings version `v` 1 turns `ask_before_receive` on; deletions always asked; toast "Trust this PC…" opens the page, whose select answers `trust_7|30|182|always`); "nobody answered in time" instead of "declined"; Tailscale peers tried by both engines (`rdesk::tailscale_pcs`, home address kept when also found by beacon, quiet errors); wrong-code log lines once per 10 min per address; the sync event gets a DACL (Everyone, medium label) so the hook can wake an elevated Deck (not tested at runtime); command line `beam/cli.rs` (`quiet` engine: no beacon, no listener; inbox.json merged, Deck reloads it on change) tested end to end against this PC's Deck (prompt, accept, file arrived); Claude Code skill `beam/skill.md` installed by a switch. Framerate limit key: `rtss-osd32 on|off|status <dir> 4` flips RTSSHOOKSFLAG_LIMITER_DISABLED (checked on the real RivaTuner: off→on→off), `limiter_hotkey`, `rtss_limiter` command, limit read/written in `Profiles\Global [Framerate] Limit`.
- Overlay preview colours (user report: some colours look right in game but not in the preview): the preview now follows `osd.rs render()` exactly: prefix and unit always C0, labels C0 or the group colour, graph C0 or C7, small units 62 % and raised, invalid colours fall back like `hex()`. The frame colour bar's preview was left as is (RTSS's own bar not checked).
- Phone remote: skip buttons seek 10 s when the player has no previous/next (browsers report `can_next:false`), taps add up; the home page shows CPU/GPU watts (`cpu_w`, `gpu_w` from the hardware engine's table, NVIDIA's reading as fallback) and the CPU package temperature (`cpu_t`).

## 2026-10-14: v2.0.1 — phone fixes, remote desktop, Tailscale

**H.264 for the phone Screen view (added last, uncommitted):** `src-tauri/src/h264.rs` (Media Foundation encoder, BGRA→NV12, Annex-B + fragmented-MP4 writer, `plan()` for width/fps per bitrate) and `screencast.rs` (`Video` enum, `spawn_h264` thread, `spawn_jpeg`, plan hysteresis: narrower after 1 s (3 s for MSE), wider after 10 s (20 s for MSE), sparse-input bitrate scaling because NVENC spends bits per nominal frame, fallback chain encoder → next encoder → JPEG). The phone asks with `/screen?v=h264w|h264m|jpeg` (WebCodecs when `VideoDecoder` exists, MSE otherwise; plain http has no WebCodecs). Message layout: 8-byte capture time, kind byte (0/1 Annex-B delta/key, 2 MP4 header, 3/4 fragment), data; `{"t":"video"}` text announces codec string/size; phone sends `{"t":"idr"}` on decode errors and `{"t":"codec"}` (saved as `remote_screen_codec`: auto/jpeg). Measured here: 1 Mb/s → 987 kb/s at 30 fps/640 px; 15 Mb/s 60 fps → 48 fps/12.8 Mb/s (the screen's own update rate); both decoders verified in headless Chrome (MSE without flags, WebCodecs with `--unsafely-treat-insecure-origin-as-secure`). Without WebCodecs the phone now decodes in the page: `phone/h264dec.js` (served as `/h264dec-2.js`, rename on change: cached a year) is tinyh264 0.0.7 (Apache-2.0, `phone/h264dec.LICENSE.txt`; npm package `es/TinyH264.js` + `es/TinyH264Decoder.js` with their `export default` lines removed) plus Deck's worker messages at the end; `/screen?v=h264s` → `Video::Sw`: Constrained Baseline (`Encoder::open_as(.., true)`, the decoder reads nothing else), Annex-B like WebCodecs, 8 in flight, answered once decoded; WebGL YUV→RGB (BT.709 limited), JS fallback; the phone asks `{"t":"vmax"}` for narrower pictures when decoding takes >70% of the time or 4 pictures wait for it (twice in a row, checked every 3 s). The phone server turns Nagle off (`tap_io` + `set_nodelay`). Checked: `baseline_stream_dump` (ignored test) + a Node run of the worker on both encoders' streams: 90/90 pictures, 45-50 dB PSNR against the source. MSE stays only for browsers without WebAssembly; on Firefox Android it stalled and replayed old pictures even after these fixes (its decoder drains when the buffer runs out, then decodes again from the last key picture). MSE pitfalls found on Firefox Android: `SourceBuffer.remove` also removes everything after the range up to the next key picture, so the phone trims only up to a key picture it recorded (and asks for one after 10 s without); seeking replays from the last key picture, so a phone that fell behind asks for a key picture and seeks onto it, otherwise playbackRate keeps a 0.1-0.3 s cushion; fragment durations are the real capture gaps (a 16.7 ms minimum made media time outrun real time and forced seeks; the last picture before a still screen gets 60 ms); a new `{"t":"video"}` with the player running appends the new init segment to the same SourceBuffer. Both Media Foundation encoders ignored `AVEncMPVGOPSize` set after the output type and made a key picture every 120 pictures; it's now set before and after (test `keys_when_bitrate_changes`). `--selftest-remote` takes `DECK_SELFTEST_SECS`. Sound: WASAPI loopback sends nothing in silence, which the phone counted as dropouts, so the PC now fills the first 1.5 s of silence. Live tests: `cargo test --release -- --ignored encoders_live keys_when_bitrate_changes baseline_stream_dump backwards_time_and_repeats sparse_input_rate real_screen_rate`.

- **Phone QR timed out:** no firewall rule for deck.exe, and an elevated Deck started by the logon task never gets
  Windows' "Allow access" prompt. `remote::allow_through_firewall` (when admin) adds "Deck phone remote" via netsh
  (this exe, TCP, private + domain), replacing it when the exe path changed.
- **File pickers behind Deck:** `main::file_dialog` sets Deck's window as the dialog owner.
- **Hot corner during screenshots:** `hotcorner::snipping_in_front` (ScreenClippingHost, SnippingTool, ShareX, Lightshot,
  Greenshot, Flameshot, Snagit, PicPick, Screenpresso) blocks the trigger; a mouse press in the corner sets `fired`
  so resting there after letting go doesn't open Deck.
- **Remote desktop (`rdesk.rs`):** RustDesk installed from its newest GitHub MSI (SHA-256 from the API `digest` +
  Authenticode check), service on/off, permanent password, `direct-server` (port 21118), `hide-tray` + Tailscale
  `--unattended` + their startup entries off. Admin work goes through `admin_ps` (base64 script; one UAC prompt when
  Deck isn't admin). Commands `rdesk_status`, `rdesk_do`. Status is cached for Tailscale (20 s).
- **Screen on your phone (`screencast.rs`):** `/screen?k=` WebSocket (separate from `/ws`). A capture thread per viewer:
  DXGI duplication of the chosen monitor on the adapter that drives it (matched by HMONITOR; laptops with two GPUs),
  GPU mip halving + `shrink` to the preset width, GDI (HALFTONE StretchBlt) for the first picture, HDR desktops and
  failures; GDI pictures are hashed so an unchanged screen sends nothing. JPEG via `image` (feature `jpeg`).
  Bitrate (`Rate`): `remote_screen_kbps` 0 = Auto, else a limit; `remote_screen_priority` smooth/sharp. JPEG quality
  follows each picture's share of the budget (log-sized steps), the width jumps to the one whose pictures fit when quality
  bottoms out or a picture is 3x too big, and back up to what the budget carries (`width_for`, a bytes-per-pixel model).
  Smooth: wanted fps, q down to 4, down to 160 px (1 Mb/s 30 fps ≈ 320 px). Sharp: plans `sharp_fps` (kbps/1500, 2-10),
  q ≥ 60, native size first, not below 480 px; q ≥ 80 encodes 4:4:4. Presets: "sharp" = Full (≤ 2560 px, default),
  "balanced" = Medium 1280, "fast" = Small 960. fps 15/24/30/60. Phone sound: one continuous stream (JS linear
  resampler carrying state, sample-exact scheduling, ±0.5 % speed for clock drift, cushion 40-300 ms)., a byte bucket (0.25 s) keeps the average under the limit, and budget left unused raises quality.
  Auto (`Rate::tune`, every 0.5 s): `Feedback` times each picture until the phone's ack; delay > 1.8x the connection's
  own + 80 ms (or a picture stuck > 1 s) cuts to 0.75x / 0.9x of what got through; delay near the baseline raises 15-30 %.
  Measured: 2 Mb/s smooth 25 fps at 640 px, sharp 12 fps; 1 Mb/s 12 fps at 480 px (JPEG's floor); Auto on the LAN reaches
  35 Mb/s in ~10 s. Pictures and sound packets start with their capture time (f64 ms on QPC: DXGI `LastPresentTime`,
  WASAPI's QPC position); the phone delays pictures to when their sound plays (`SA.off`) while Control is off.
  Sound packets: format byte (0 PCM, 1 IMA ADPCM), channels, rate, ADPCM states; `?q=best|good|low` on `/audio`.
  Pipeline: capture thread → two `screencast-jpeg` threads (alternate pictures) → `screencast-send` puts them back in order;
  the frame gap counts from the start of a grab (counting from its end capped 60 fps at ~45). Bytes are reserved in the
  bucket at grab time. `{"t":"diag"}` every 2 s: grabs / new / waits (credit, encoder, gap, bucket); the phone shows
  the screen's own rate from it. Smooth quality floor by width (25 at ≥854 px, 12 at ≥480, 4 below); 4:4:4 only for Sharp.
  Flow control: 4 frames in flight, one credit back per phone "ack". Presets sharp 1920/q85, balanced 1280/q75, fast
  960/q60; `remote_screen_fps` 30/60 sets the gap. JPEG by `jpeg-encoder` (AVX2, BGRA in): 3.7 ms at 1280 px (the
  `image` crate took 12.4 ms). Frames wait in `AcquireNextFrame(15 ms)`; only GDI pictures are hashed. Cursor sent as
  normalized `cur` messages. Sound: `/audio` WebSocket, `start_sound` = WASAPI loopback of the default output (follows
  device changes), 16-bit stereo 10 ms packets; the phone plays them with Web Audio and an adaptive cushion
  (30 ms up to 200 ms after dropouts, back down after 15 s smooth). Phone: Control off on every open (`SC.ctlOn`),
  full screen (`.scr.imm`), canvas drawing via `createImageBitmap`, tries the PC's LAN address (`st.lan`) before the
  Tailscale one, viewport `maximum-scale=1, interactive-widget=overlays-content` (Chrome zoomed into the hidden
  typing box and kept the zoom after rotating). Input (`input()`): SendInput absolute (VIRTUALDESK), buttons, wheel, unicode text, keys
  with mods; only in `control` mode, and not while `boost::game_in_front` (game folder, or fullscreen + GPU ≥ 60 %). Settings `remote_screen` (off/view/control, default off), `remote_screen_quality`.
  Measured on the dev PC: ~10 ms per 1280 px frame (capture + shrink + JPEG), ~85-95 KB; the frame rate follows how
  often the screen changes (Discord + a 30 fps stream: ~25-36 fps).
- **Tailscale:** `remote::info` adds `away` / `away_qr` (Deck's page on the Tailscale IP), `away_peers`,
  `away_account` and `away_get_qr` → `/tailscale`, a page served by Deck with store links plus the newest universal APK
  from pkgs.tailscale.com (the Play Store hides Tailscale in some countries). Tailscale's network is Private, so
  Deck's firewall rule covers it.

## 2026-10-14: v2.0.0 — the big update

Everything since 1.1.2, built and tested on the maintainer's PC between releases (fast build + exe swap each round).

- **Look:** 8 themes (`THEMES` in `pages3.js`, colours in `app.css` under `[data-theme]`), accent, opacity per pin state
  (`--deck-opacity`), opening animations (`:root[data-anim]` sets `--a-from/--a-to/--a-t/--a-ease`), low-spec mode
  (`html.low-spec`, tick every 3 s in `app.js`). First-start picker `themeWelcome` ("Make Deck yours", flag `setup_picked`;
  older flags `theme_picked` / `look_picked` stay in the struct so old settings files load).
- **Pin:** `set_pinned(on, rect)` floats the quick panel as its own always-on-top window (`PIN_RECT`); "shown" carries
  `float`; `body.pin-float`. In app mode: always on top. The click handler used `e.currentTarget` after an await (null).
- **Home grid:** free 12-column grid with 10 px rows (`pgEngine` / `pgResolve` / `pgSpot` in `layout.js`), push-down
  collision, snap guide, fold-away parts (`pgFit` / `pgFold`, `data-drop` marks). `pgKeep` carries place, size and fold
  across live redraws (stopped flicker). Defaults: `HOME_DEFAULT`. Stale `get_state` replies overwrote in-flight writes
  (resize "needed twice"): `localWrites` + `withLocalWrites` in `core.js`. `mouseHeld` covers long drags (2 min).
- **PC tile:** `PC_CARDS` in `home.js` (cpu, ram, gpu, vram, net), rows aligned with CSS subgrid; customize dialog
  `pcCustomize` (`pc_cards`, `pc_drives`). Backend: `live.cpu_now` (clock, package power, temps from the hw engine),
  NVML graphics clock and `vram_hist` in `sys.rs`.
- **Network rates:** sysinfo's adapter list counted the Npcap filter interface as well as the real adapter (double).
  `util::real_adapter` (hardware, not a filter, up, Ethernet/Wi-Fi/WWAN) + `util::NetCounter` (per-LUID deltas, so
  adapters add, a new one starts from its next reading, a removed one stops). Used by `sys.rs`, `rtss.rs`, `sysmon.rs`.
- **RAM RGB after a cold boot:** Deck starts ~17 s after boot, before the Fury controllers answer; the scan found
  nothing and never ran again (only wake rescanned). Now `FuryRam::scan_missing` retries (5 s doubling to 60 s, 10 min
  after start or wake) while fewer sticks than `fury_modules()` were found, without touching found sticks (a full
  rescan reset them: visible flashing).
- **Drives:** `hw/smart.rs` (NVMe log page 2, ATA 0xD0/0xD1/0xDA), history in `drive-health.json`, alerts gated by
  `drive_alerts` (default off), "past issue" notice once. Page in `hw.js` (`drivesCard`), support report includes it.
- **Disk space:** `mft.rs` reads the NTFS MFT directly (FindFirstFileEx fallback), squarified treemap, `disk_treemap`,
  `disk_types`, `disk_path`; UI in `pages2.js`.
- **Overlay:** own page; `rtss_install` (winget); the overlay key also flips RTSS's OSD switch via the embedded 32-bit
  helper `rtss32/` (RTSSHooks SetFlags + `WM_APP+100`). `build.rs` builds it into `target/rtss32` (short path: LNK1104
  in OUT_DIR). CI adds the i686 target.
- **Game boost:** trim only when memory ≥ 85 % (`memory_short`), auto-end after 3 h without a game (`NO_GAME_LIMIT`),
  GPU-busy foreground counts as a game.
- **Sidebar:** drag to reorder (`nav_order`, `navDnd`), Power & time last, Reset in Settings, all groups folded by
  default, width 180.
- **GPU wake on open (investigated, not fixable):** opening any window makes the NVIDIA driver jump P8 → P0 (~90 W for
  2–3 s); the Start menu does the same. NVML reads don't wake it, and `--disable-gpu` didn't help (DWM still composes).
  Kept: no repaint nudge on show unless the page reports no frame (`nudge_if_blank`).
- Tests: `cargo test --release` 95 pass (+15 ignored live). Live: `net_matches_windows`. UI checked with preview
  screenshots (the harness hard-codes Edge's path; Chrome works). Not tested: a cold boot with the RAM retry, laptops,
  AMD/Intel GPU cards in the PC tile.

## 2026-10-14: v1.1.2 — sidebar regrouped

- `NAV` in `ui/js/app.js`: Everyday / Power / Hardware / Screen / PC became Hardware (controls only) / Monitor
  (Sensors, Processes, Network, This PC) / Desktop (Windows, Display, Clipboard, Sound) / Power & time (all three
  Power subpages together) / Maintenance (Disk space, Clean up, Startup apps, Tools). `navShown` still hides hardware
  pages per PC, so Monitor just loses Sensors / This PC where there is no hardware. `nav_folded` holds old group
  names; they are ignored (folded groups reopen once).
- Removed the orphaned `files/index` route (nothing linked to it; `indexView` stays, used by Settings).
- Tests: `regress.mjs` 37 pass (run with Chrome; the harness hard-codes Edge's path). Installed and checked by the user.

## 2026-10-13: v1.1.1 — lighting survives sleep

- Bug: after sleep the Kingston Fury sticks fell back to their onboard effect. `FuryRam` keeps `per_led`/`sent`
  caches, so it never re-sent the direct-mode handshake (`set_direct`) and static colours weren't re-sent at all.
  Only `gpu/` listened for resume.
- Fix: `gpu::WAKES` (AtomicU64) counts `PBT_APMRESUMEAUTOMATIC`; `watch_resume` is now `Once`-guarded so several threads
  can call it. The board RGB thread sets `reconnect` and the RAM thread rebuilds + rescans `FuryRam` 4 s after a wake.
  Hardware-agnostic: on machines without these devices the rescan finds nothing, same as at startup.
- Tested: real S3 sleep/wake, RAM lights came back; no errors or crashes in the event logs. Not tested: hibernate, Modern Standby laptops.

## 2026-10-13: v1.1.0 — performance pass, units, Comet colours

- **Idle cost measured** (10 s samples, Deck hidden in the tray): ~19 % of one core, most of it the hidden WebView
  (renderer + GPU process ~7 % each). Cause: `ui/guard.js` ran a `requestAnimationFrame` loop forever, and the
  WebView2 flags (`--disable-renderer-backgrounding` etc., kept: they fix blank windows) stop Chromium from pausing it,
  so a hidden window ticked at the monitor's refresh rate. `guard.js` now requests one frame per 4 s heartbeat.
  The same guard is shared with the maintainer's other Tauri apps and was fixed there too.
- **Screen / Game lighting** (`hw/screen.rs`): DXGI desktop duplication with a GPU mip chain, reading back ~60x34 px.
  Measured 0.34 ms per sample vs 23 ms for the GDI `StretchBlt` (the GDI cost is a fixed DWM readback, not the size).
  No new frame = the last colours, free. GDI is the fallback (access lost, non-BGRA desktop such as HDR); retries
  duplication after 1-3 s. The session is dropped while no screen mode is on. Live check: `cargo test --release dxgi_live -- --ignored --nocapture`.
- Looked at and left alone: hot-corner 30 ms cursor poll, cooler-display loop (sends only on change), game audio meter
  (one peak read per poll), RTSS overlay (1 s). Board RGB at 30 Hz / RAM RGB over SMBus every 50-120 ms are the cost
  of animated effects; the user chose to keep them.
- **Units** setting: `unit_clock` (MHz/GHz), `unit_net` (Mb/s / MB/s), `unit_temp` (C/F) in `Settings`. JS helpers
  `fmtClock`, `fmtTemp`, `tempNum`, `fmtRate`, `mbpsParts` in `core.js`; the sensor table converts rows in
  `srUnits` (network rows only for transfer rates; tones still judged in °C). The overlay gets them through
  `Settings::osd_render_style()` (OsdStyle fields `ghz`, `fahrenheit`, `net_bytes`, not saved). The phone gets
  `sys.units`. Inputs (offsets, curves, limits) deliberately stay in MHz / °C.
- **Comet colours** (`L_CometColors`: My colour / Cycle / Rainbow) in `fx.rs`; the colour picker hides when the comet cycles.
- Sidebar badge (active CPU/GPU profile) now truncates with "…" instead of squeezing the page name.
- Tests: 86 pass (+3: overlay units, Comet colours, screen column reduce), 10 ignored live tests.

## 2026-10-11: hardware control (MoboCtl merged, Afterburner replaced)

Spec `docs/design/specs/2026-10-11-hardware-control-design.md`, plan + ledger with every ruling in
`docs/design/plans/` (`progress.md`).
- **Runs elevated** through scheduled tasks `\Deck\Deck` (logon, `--minimized`) and `\Deck\Open` (Start menu via `run.vbs`);
  installed to `C:\Program Files\Deck` by `install.ps1`; apps it opens still start as the normal user (`util::launch_as_user`).
- **Hardware group**: CPU, GPU, Fans, Lighting, Cooler display, Sensors, This PC (pages hide when the PC has no such hardware).
  Engine = MoboCtl's, in `src/hw/` (settings `%APPDATA%\Deck\hardware\settings.ini` + `curves.ini`, MoboCtl keys).
- **CPU profiles** = Windows power plans + extras (RAPL limits, min-clock lock, turbo OC, Game Mode off while capped);
  plan watcher (2 s), in-game clock check. `--cpu-probe` lists plans.
- **GPU profiles** = Afterburner slots: power %, temp limit, core/mem offset or VF curve (NvAPI VF table deltas), voltage %, fan
  auto/fixed/curve. Afterburner import (`profiles/afterburner.rs`, fixture = a real Afterburner cfg). Crash guard `%APPDATA%\Deck\gpu-applied.json`.
  Stock VF curve cached in `gpu-stock-curve.json`. `--gpu-probe [--write] --out f`.
- **Switching**: profile cards, tray submenus, RegisterHotKey keys, desktop/Start shortcuts (`\Deck\Profile <kind>-<id>` tasks),
  phone chips, Game boost. 25 icons in `src-tauri/icons/profiles` (generator `tools/icons/make.py`).
- **RTSS kept**: Deck writes its OSD slot + publishes `MAHMSharedMemory` (`rtss.rs`); items/key on the Sensors page.
- Tests: `cargo test --bin deck` (39). UI previews: `--dump-state` + `--dump-hw` (read-only) → mock `__TAURI__` → headless Edge.
- Debug: `--release-fans` (board + GPU fan + Game Mode back), `--quit` (graceful, used by `uninstall.ps1`), `--hw-selftest`.


## What it is

One tray app that combines a Raycast-style launcher, a live dashboard and the best
PowerToys / small-utility tools. Opened by a **hot corner** (mouse in a screen corner),
the tray icon, the Start menu or the phone. **No keyboard shortcuts** (user preference).

Built from research of 2026 "best Windows utilities" lists (PowerToys' 30 tools,
Raycast, EarTrumpet, WizTree, Ditto, LocalSend, Twinkle Tray, Stretchly, Auto Dark
Mode, LockHunter, BCUninstaller...). What made it in:

| Area | Features | Inspired by |
|---|---|---|
| Search box | Start-menu + Store apps (with icons), Deck actions, calculator, unit & currency conversion, number bases, games, files (own index), clipboard, web; commands `timer 10m`, `sleep in 30m`, `kill discord`, URLs | Raycast, PowerToys Run/Command Palette, Flow Launcher |
| Home tiles | Clock + calendar, now playing (SMTC, artwork), PC vitals (CPU/RAM/GPU via NVML, drives, sparklines), sound (outputs + per-app), pinned & frequent apps, quick actions, clipboard, power & timers, MicDrop, quick note, network | Dashboards |
| Clipboard | History of text/images/files, pins, search, click = paste into previous app, "paste as" (plain, case, trim, one line, clean link, pretty JSON); skips password-manager content | Ditto, Advanced Paste |
| Sound | Default output switch (IPolicyConfig), master + per-app volume with meters, all media sessions | EarTrumpet |
| Windows | Open windows list: pin on top, see-through, snap to zone, center, close; FancyZones-style drag snapping (tap right mouse button while dragging, or always); 6 layouts; workspaces (save window positions + apps to close + audio output + keep awake) | FancyZones, Always On Top, Workspaces |
| Files & disk | Drive/folder scan → treemap + list + biggest files, recycle; cleanup of temp, shader caches, crash dumps, browser/Discord/Spotify/Steam caches, thumbnails, old installers (to Recycle Bin), dev caches, Recycle Bin; file-search index folders | WizTree/TreeSize, PC Manager, CCleaner |
| Power | Keep awake (on / for N min / while app runs / until downloads finish), "do X when Y" (shutdown/sleep/restart/lock/screen off/notify when time, app closes, downloads finish, CPU idle, network quiet, media stops, away), 60 s cancel warning; timers, stopwatch, break reminders (never over fullscreen) | Awake, Shutdown timers, Stretchly |
| System | Processes grouped by app (end, not-responding flag), startup apps on/off (StartupApproved, same as Task Manager), network (IPs, ping, Cloudflare speed test), displays (refresh rate, DDC/CI brightness, HDR), dark/light theme + schedule (times or sunrise/sunset) | Task Manager, Twinkle Tray, Auto Dark Mode |
| Tools | Colour picker (loupe), copy text from screen (Windows OCR), pixel ruler, colour history, "what's locking this file" (Restart Manager), notes, shortcuts to Windows panels | Color Picker, Text Extractor, Screen Ruler, File Locksmith |
| Phone | LAN web app (installable PWA): media, volume, outputs, power, plans, timers, PC vitals, send text/links/files both ways, get PC clipboard | LocalSend, remotes |

## Preferences Deck follows

Key points:
- **No keyboard shortcuts** as the way in. Hot corner default top-left, 250 ms, off over fullscreen games.
- Deck now runs **elevated** (hardware control); the user accepted that. Profile hotkeys are the one keyboard exception they asked for.
- Picky about looks: check UI changes with offscreen renders before shipping.
- Must be safe with kernel anti-cheat (Vanguard, EAC, BattlEye). Deck uses **no input hooks**:
  hot corner polls the cursor, drag-snapping uses WinEvent hooks + GetAsyncKeyState.

## Current status

- Installed at `%LOCALAPPDATA%\Programs\Deck\Deck.exe` + Start Menu shortcut (`install.ps1`).
- **Not launched yet** by me (user may be gaming). First launch registers autostart (`--minimized`).
- Settings: `%APPDATA%\Deck\settings.json`; clipboard history in `%APPDATA%\Deck\clipboard\`.
- Phone port 47830 (MicDrop uses 47823). Files from phone → `Downloads\From phone`.

Verified: unit tests (calc, fuzzy search), read-only `--selftest` against the real PC
(audio, media, apps, games, windows, monitors, startup, ping, recycle bin), phone server
16/16 checks (`--selftest-remote` + node client), offscreen renders of every page and the phone page.

Not verified yet (needs the real GUI): hot corner feel, toast window placement, zones overlay
while dragging, colour picker / OCR overlay, paste-back into previous app, workspaces restore,
refresh-rate/HDR switching, firewall prompt for the phone.

## Code layout

| Path | What |
|---|---|
| `src-tauri/src/main.rs` | Tauri setup, ~80 commands, tray, show/hide (dim overlay or window), toast + zones windows, icon:// protocol, debug modes |
| `src-tauri/src/app.rs` | `App`: shared state, change events, toasts + toast actions, frecency |
| `settings.rs` | Settings struct (+ layouts, workspaces, notes), atomic save, `DECK_PROFILE` |
| `sys.rs` | sysinfo + NVML vitals, process groups, kill |
| `audio.rs` | outputs, IPolicyConfig default output, master/per-app volume, friendly exe names |
| `media.rs` | SMTC sessions, artwork, controls |
| `apps.rs` | AppsFolder enumeration, icon extraction (STA thread) → PNG |
| `calc.rs` | expression parser, units, currencies (open.er-api.com, cached), bases |
| `files.rs` | file-name index + fuzzy `score()` |
| `clipboard.rs` | message-only window listener, history, set text/image/files, `send_paste` |
| `power.rs` | keep awake thread, plans, timers, stopwatch, breaks |
| `winmgr.rs` | window list/actions, zones drag (WinEvent hook), workspaces |
| `disk.rs` / `cleanup.rs` | jwalk scan tree, recycle; cleanup targets |
| `startup.rs`, `display.rs`, `net.rs`, `locksmith.rs`, `games.rs` | as named (speed test can be cancelled mid-run) |
| `gameping.rs` | game server ping. Valve games: ICMP to every relay of each SDR city, best relay's median. Fortnite / Rocket League / Dead by Daylight: UDP echo to Amazon GameLift ping beacons `gamelift-ping.<region>.api.aws:7770`, grouped into each game's regions. TCP connect is useless here (router/ISP answers port 443 in ~5 ms). Riot etc. not listed (not measurable) |
| `picker.rs` | native fullscreen overlay: colour / OCR (Windows.Media.Ocr) / measure |
| `hotcorner.rs` | cursor polling + `fullscreen_in_front()` |
| `micdrop.rs` | WebSocket client to MicDrop's phone remote (only sends on clicks) |
| `remote.rs` + `phone/index.html` | phone server (axum) and page |
| `ui/` | `index.html`, `app.css`, `js/{icons,core,home,search,pages1-3,app}.js`, `toast.html`, `zones.html` |

## Build / test

```powershell
cd src-tauri; cargo test; cargo build --release
..\install.ps1          # refuses while Deck is running
```
Debug-only flags (`target\debug\deck.exe`), all use profile `%APPDATA%\Deck-test`
and change nothing: `--selftest`, `--dump-state`, `--dump-icons`, `--selftest-remote` (port 47839, localhost only).
UI previews: `--dump-state` + `--dump-icons` → mock `__TAURI__` → headless Edge (script lived in the
session scratchpad; recreate from this description). Never capture the real screen.

## Safety rules

1. Don't launch the GUI or change audio/display/power without asking; the user may be in a match.
2. Before tests check `Get-Process Deck`; test modes use the test profile and bind 127.0.0.1.
3. Deck never changes the default **recording** device (MicDrop owns it).

## Changes 2026-09-27 (second session)

- Speed test: Cancel button (restores the previous result; upload body aborts mid-request).
- System → Network → Game servers: only installed competitive games (CS2, Fortnite, Rocket League, Valorant, League).
  Verified live: Valve 43 cities, AWS 24 regions. Old Riot IPs (104.160.x) no longer answer.
- Phone upload fix: the file input lived inside the re-rendered `#app`, so after the picker returned the change event fired on a
  detached element. Input + progress box (with cancel) now live outside `#app`. Verified: headless Edge + CDP uploaded 40 MB, identical.
- Remote desktop from outside home: discussed, user declined for now (would need Cloudflare Tunnel or Tailscale; no admin).

- Sidebar redesign (2026-09-28): no more tab bars inside pages. 23 items in groups
  (top: Home/Apps/Games · Everyday · Power · Screen · PC · bottom: Phone/Settings), defined once in `NAV` in app.js
  as [page, sub, label, icon]; titles in `HEADS` (pages1.js). Tools and Power pages split by sub. File search moved to
  Settings. Sidebar badges show keep-awake On / plan Set / timer count. Panel height 900. One-click shutdown presets.

- NetLimiter-style limits: discussed and declined by the user (needs admin service + WinDivert driver; WinDivert 2.2.2's driver is signed by a third-party Chinese company; anti-cheat risk). Don't propose again unless asked.

## Backlog

- Background-diet "modes" beyond workspaces (start/stop services needs admin setup).
- Save guardian, death counter, backlog picker as Deck pages.
- Tile drag-to-reorder, custom layouts editor, bulk rename, image resizer, text snippets.
- Mouse side button trigger (needs a low-level hook; risky with anti-cheat).

## 2026-09-28: Game boost

New sidebar page **Game boost** (`ui/js/boost.js`, `src-tauri/src/boost.rs`), also in the tray
("Full power for gaming", "Clean up background apps") and on the phone (PC card). Built to work on
any PC (the user shares Deck with friends), unelevated.

- **Full power**: closes the ticked apps (WM_CLOSE to windows; background-only apps are ended),
  switches to the fastest power plan (picked by *personality*: Ultimate > High performance >
  any high-perf-based plan; else "Best performance" power-mode overlay on Balanced, via
  powrprof's undocumented overlay calls loaded with GetProcAddress), lowers background (no-window,
  non-voice) apps to below-normal priority, trims working sets. Everything is put back when the
  game closes (45 s with no game), from the Stop button, or on Deck quit. State is saved to
  `%APPDATA%\Deck\boost.json`; on startup a leftover file is restored (power plan always, apps and
  priorities only if the PC didn't reboot).
- **Clean up**: closes the ticked apps and trims memory; changes nothing else, reopens nothing.
- Game detection: game folders from Steam/Epic (games.rs), Riot (`ProgramData\Riot Games\Metadata\*.product_settings.yaml`),
  GOG/Ubisoft (HKLM), `X:\XboxGames`, plus any fullscreen program that isn't a browser/player.
- Never touched (no handle opened): NEVER list (launchers, anti-cheat, hardware/RGB/audio tools,
  recording, security, VPNs, runtimes/terminals), anything in a game folder, under %WINDIR%, other
  sessions, and **every console-subsystem exe** (read from the PE header). Voice/music apps are
  listed but never pre-ticked or slowed.
- Settings: `boost_close` (exe stem -> bool; missing = suggestion), `boost_power`,
  `boost_priority`, `boost_trim`, `boost_auto_end`.
- Tests: `cargo test --bin deck boost:: -- --include-ignored --nocapture` includes a read-only
  dry run (prints the app list and the plan choice) and a close/reopen/priority/trim test on a
  hidden throwaway `ping.exe`. The power-plan switch itself was not run live (it would change the
  user's plan); the WM_CLOSE path for windowed apps wasn't run live either.
- Checked with a Balanced-based custom plan active: Full power picks High performance and goes back to the custom plan afterwards.

## 2026-09-29: "asset not found: index.html"

Tauri's codegen caches brotli-compressed copies of `ui/` in
`<target>/release/build/<crate>-*/out/tauri-codegen-assets/<hash>.<ext>` and never rewrites an
existing file. The PC crashed mid-build on 2026-09-28, leaving broken copies, so the release exe
couldn't decompress any page (debug builds don't compress, so they looked fine). `install.ps1` now
deletes that folder before building, and `main.rs` has `asset_tests::pages_are_embedded`
(`cargo test --release --bin <crate> asset_tests`) to check a release build's pages.

## 2026-10-07: blank (white/black) window protection — v0.1.1

WebView2 sometimes shows a window that paints nothing (white or black) when a window created hidden is first shown, after a long hide, or when it
decides a window is covered. There is no official Microsoft fix, so `src-tauri/src/webview_guard.rs` (same file as FluxDM's) does four things:
- `apply_env()` (first thing in `main`) sets `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS` for every window: `CalculateNativeWinOcclusion` off, no background/renderer throttling, wry's default disable-features kept. An env var rather than per-window args because custom per-window args made other windows blank (tauri-apps/tauri #13092).
- `repaint_nudge()` resizes a just-shown window by 1 px and back after 350 ms (called from the show function).
- `ui/guard.js` (loaded first in index.html) calls `ui_heartbeat` every 4 s, but only while `requestAnimationFrame` is firing. `spawn_watchdog` nudges a visible window that has been quiet 15 s and reloads one that is quiet 90 s and focused.
- Not applied: `--disable-gpu` (last resort; costs smoothness).
Compiled, not run live. If a blank window still happens, check the command line of the app's `msedgewebview2.exe` for those flags first.
- Deck-specific: `show_deck` calls the nudge after every show (hot corner / tray); the watchdog watches the `main` window only (toast and zones are short-lived).

## 2026-10-12: cleanup + full hardware test pass
Tested live against HWiNFO (shared memory) with Cinebench/Heaven running; every test under 2 min.
- Passed: CPU profiles under all-core load (Full 4.6 GHz/125 W, Mid 3.67 GHz, Eco 2.7 GHz/37 W, Ultra 1.39 GHz/14 W),
  power limits (PL2 55 W held), min-clock lock (lands ~8% under target: Windows maps the % a bit low), fans fixed/curve/BIOS
  and release on `--quit`, GPU profiles under Heaven (P4 98 W power-limited, P1 259 W/2.05 GHz/mem +500), VF clock lock
  (exactly 1605 MHz at 850 mV) and its release, fixed GPU fan, RTSS overlay slot, open hotkey (panel + app mode),
  GPU profile hotkeys, Game boost on/off (plan Eco → High performance → Eco), phone remote (key check, WebSocket state, profile switch).
- Fixed:
  - WebView2 browser flags (anti-blank guard) never applied: wry's own args beat the env var. Now set on every window
    builder (`webview_guard::browser_args`); the main window is built in `setup` (`"create": false` in tauri.conf).
    Extra flags (a debug port) still come from WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS.
  - A 65 °C GPU temperature target left by Afterburner's linked limits stayed forever (Deck's profiles say "card's own"):
    stock throttled and the fans ran at 100%. The engine now learns the card default from Afterburner's `[Defaults]`.
  - The page sat frozen (no live ticks) after a start-up show whose "shown" event fired before the page listened:
    the page now asks `get_state.visible` at boot, on focus and on visibilitychange.
  - "Afterburner is running" warning kept showing after Stop tuning: new `afterburner_tunes` (startup/hotkeys/fan control).
  - Clean up showed 0 B for not-yet-measured items (size is now null until measured), RAM lighting toggle hidden on non-RGB
    kits, idle fan headers dimmed, cooler-display status wording, dark scrollbar for the nav, wider "When" select.
- Not changed: System Fan #1 runs flat out at any duty (3-pin fan on a PWM header?); EC channel 8 doesn't take writes
  (no header on this board). Clean up not run for real (would delete ~25 GB of caches).
- Testing tools: start Deck elevated with WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222, then drive
  it over CDP; synthetic keys must come from an elevated process (UIPI blocks them into the admin window otherwise).
- GPU default temperature target is now also saved to %APPDATA%\Deck\gpu-defaults.json (per PCI id), so Deck can still undo a leftover temp limit after MSI Afterburner is uninstalled (test default_temp_survives_uninstall).
- Later 2026-10-12: GPU profile re-applied ~5 s after wake from sleep (`gpu::watch_resume`, PowerRegisterSuspendResumeNotification);
  a CPU profile whose power plan was deleted gets a new "Deck · <name>" plan on apply/save (`cpu::ensure_plan`; `cpu_save`
  keeps the stored plan GUID; verified live); hardware report says "Deck" (Save report file still said MoboCtl).
  Afterburner + MoboCtl parity audit: nothing missing (details in DECK.md §4). DECK.md written.
- 2026-10-12 UI pass: Network page redesigned to fit one screen. It has a connection strip with a 60 s graph and click-to-copy IPs. Speed test and ping sit on the left; ping runs automatically every 5 s while the page is open and keeps a latency history strip. Game servers sit on the right and auto-test the first time a game is picked. Sensors page split into tabs (Graphs / All readings / Overlay & alerts). All readings is a device list plus a detail pane: per-core and per-fan rows fold into a grid (now + highest), other rows are grouped by unit, with search across everything. Checked with a mock-backend preview in headless Edge, because a game was running; not yet built or installed.
- 2026-10-12 UI pass over every page (checked with the fake-data preview, nothing built or installed yet because CS2 was running). Removed the Apps and Games pages (nav, code, styles). Home: new Performance tile (one-click CPU/GPU profile switch; `profiles` added to ALL_TILES in settings.rs, existing users get it once via a localStorage flag), shorter quick-action labels, two-line pinned labels. Game boost: one hero card (Full power + options + one-off Clean up), apps in a two-column grid with "Only suggested / None" and a memory total. Clipboard: kind icons, day groups, counts on filters, arrow keys/Enter/Delete. Sound: big master volume, device cards, two-column per-app mixer. Timers: progress bars. Keep awake: clear on/off state and one big button. Shut down when: quick picks first. CPU/GPU: 60 s sparklines in the live strip. Fans: fan rows on the left, curve editor on the right (rows linked to the curve, one "..." menu). This PC: identical memory modules grouped. Windows: column labels and "Not responding" chip. Display: hint when the monitor runs below its top refresh rate. Processes: usage bars. Startup: enabled first, summary line. Disk space: scanned drive highlighted. Clean up: sorted by size with bars. Workspaces: app icons and chips. Settings: tabs (General, Home, Clipboard & search, Profiles, About). Phone: allow switch on top, Android steps collapsed. Cooler display left as it was (this PC has none). Preview tool: tools/testing/ui-preview (shot.mjs, mock.js, mock-data.js, all.sh).

## 2026-10-12 (later) — overlay rework
- New `src-tauri/src/osd.rs`: overlay layout engine (grouped / list / one line, labels, units, columns, FPS figures on their own lines, warm/hot colours, per-part colours, clock 12/24 h). 47 items incl. per-thread and per-core rows (usage, clock, temperature), VRAM %, GPU limit, CPU VID, network, and this PC's own fans and board sensors (`fan:N`, `temp:N`).
- `rtss.rs`: frame statistics (avg / min / max / 1% / 0.1% lows) come from each RTSS app entry's own frametime ring (`dwStatFrameTimeBuf[1024]` at 924, next-write `dwStatFrameTimeBufPos` at 5020, shared memory v2.5+), read every 50 ms (2026-10-13; replaced the RTSS_Frametime pipe, which never received frames). RivaTuner's look (position, size, font, shadow, box, frametime graph, colour bar, fit) written to `Profiles\Global` and game profiles, originals in `Profiles\Deck backup`, reload via APPFLAG_PROFILE_UPDATE_REQUESTED.
- Sensors tabs: Graphs / All readings / Overlay / Alerts. `ui/js/overlay.js`: quick looks (incl. "Like MSI Afterburner"), reorderable items, colours/themes, preview.
- Hotkeys: plain Page Up/Down, Home, End, Insert, Delete, Pause, Scroll Lock, F-keys now allowed (like Afterburner).
- Not seen in a game yet: the tag rendering (<S0>, colours C3–C8, <APP>/<RES>/<API>) and the pipe stats.

## 2026-10-12 (evening) — UI audit: accessibility, lost clicks, small/large windows, phone remote
Full report with before/after numbers and screenshots: `docs/audits/2026-10-12-ui-audit.md`.
- **Fixed (each shown failing first with `tools/testing/ui-preview/regress.mjs`, now 17/17 PASS):**
  - Clicks were lost when a live page's 1-second redraw landed between mouse-down and mouse-up; redraws now wait while a button is held (`uiBusy`/`mouseHeld` in `app.js`, also Home tiles).
  - Keyboard focus is now kept across redraws (`focusSpot`/`refocus`).
  - Space no longer jumps to the search box.
  - Dialogs got role, a focus trap, focus return, and Escape resolves their promises.
  - Accessible names: switches, sliders, drop-downs and icon buttons (`nameControls` runs after each redraw), plus `aria-current`, `aria-pressed` and a live region for toasts.
  - Every control has a visible focus ring; reduced motion is respected; `--faint` raised to 4.5:1 contrast.
  - Clipboard toolbar: wraps, no twin Copy buttons. Fans list is wider. Settings is capped at 1120 px. The sidebar fades when it scrolls. Tall dialogs scroll.
  - Phone remote: zoom allowed, buttons and sliders named, bigger "All apps" target, album-art URL escaped, 640 px column on tablets.
- **New test tools:** `regress.mjs`, `a11y.mjs`, `harness.mjs`, `phone-shot.mjs`, `all-sizes.sh`. `shot.mjs` takes a `PORT` env. `mock.js` has Cooler display data.
- `restart-normal.ps1` must run **elevated**, or it leaves the debug instance running (it had been, since the earlier session). Noted in DECK.md.
- Builds this round used `-j 8`. `cargo test --release` gives 55 passed. Installed and checked live over CDP (all 28 pages, no errors), then restarted normally; port 9222 is closed.

## 2026-10-12 (night): feature audit, MicDrop removed from Deck
Full report: `docs/audits/2026-10-12-feature-audit.md` (screenshots kept locally).
- **MicDrop removed from Deck** (user request): Home tile, setting, `micdrop.rs` client, `micdrop_send`. MicDrop stays on Game boost's never-close list; Deck still never touches the recording device.
- **Fixed, with tests** (cargo 55 → 69, `regress.mjs` 17 → 26 checks, each new one failing on the old code):
  - Lost clicks: the sidebar was rebuilt every tick, and backend events redrew pages without waiting for the mouse. `render`/`renderSide` now hold while a button is down.
  - "Shut down when…": CPU and network readings froze while Deck was hidden (`sys` only sampled with the page open). Downloads and music conditions fired when nothing had started. Time plans missed during sleep ran on wake.
  - Keep awake "until downloads finish" ended before any download.
  - Crash guard disarmed by a hand quit. CPU plan-switch race. `rtss::stop` race. MAHM double writer when Afterburner starts after Deck.
  - Search: Enter ran the previous query's result; typo tolerance; games sorted and de-duplicated; kill/close/end now ask first.
  - Calculator: `200 + 10%` = 220.
  - Clean up: Recycle Bin no longer pre-ticked as "safe".
  - Network graph starts from backend history (`up_hist` added).
  - Timers and Phone no longer scroll sideways at 900×600.
- **Redesigned:** Keep awake and Shut down when… (two columns, live readings, Windows' own sleep times, plan sentence, status card with countdown).
- **Tools:** `mock.js` keeps power state (`window.__power`); `elev-install.ps1` logs the quit time (measured 2.1 s).
- **Installed and checked live over CDP; restarted normally; port 9222 closed.** Builds at `-j 8`.
- **Next:** toast-stack lost clicks; persist plans across restarts; Clipboard/Workspaces/Disk feature pass; single NVML handle.

- **Later the same night (installed at 19:20 once no game was open; toast hold and the all-users startup switch checked live over CDP; restarted normally, port 9222 closed):**
  - Toast window: toasts under the mouse stay and don't move; held changes run when the mouse leaves; the shutdown warning never waits (`toast-test.mjs`, 6 checks).
  - "Shut down when…" plans survive a restart (`plan.json`).
  - Maximized windows no longer count as games (`hw::game::covers_like_fullscreen`).
  - Paste-back only into the window it came from, never into a fullscreen game.
  - Clipboard: no silently cut text over 1 MB; at most 150 unpinned images (this PC has 90, so nothing deleted).
  - Workspaces: honest "partly ready" toast, no off-screen restore, apps asked to close before being ended.
  - Phone uploads stop before the drive has under 2 GB free.
  - Refresh rate is tested (`CDS_TEST`) before it's applied.
  - Search: "awake 2h" and "cancel shutdown".
  - Game boost crash recovery keeps `boost.json` until the restore is done.
  - Startup apps: all-users entries get an on/off switch when Deck runs as administrator (HKLM StartupApproved, like Task Manager); read-only otherwise.
  - Disk space: Windows' own folders and files (Windows tree, Program Files, Users, profile, drive roots, pagefile…) get a lock instead of a trash button, and the backend refuses them. Windows now warns before anything that can't go to the Recycle Bin is deleted for good (`FOF_WANTNUKEWARNING`).
  - Sound: only a connected playback device can be made default (never a mic, even from the phone); name matching refuses ambiguous names.
  - Screen tools: copy-text on very wide boxes no longer fails on the OCR size limit.
  - Installed again at 19:40 (quit 1.5 s); guards checked live over CDP; restarted normally, port 9222 closed.
  - Profile editors stay open when Deck hides (alt-tab), so a GPU "Try it" can be checked in a game; focus returns to the editor.
  - Dark mode schedule only acts at its switch times, so a manual Dark/Light choice holds until the next one.
  - Ending processes: anything Windows marks critical (`IsProcessCritical`) is never ended, from Processes, search or Game boost.
  - Windows page: setting a window back to 100% removes the see-through layering Deck added.
  - Installed again at 20:00 (no game open); see-through checked live on a Notepad window; restarted normally, port 9222 closed.
  - Small UI: Phone card shows only "off" while the connection is off; media without art shows a music note, not a play triangle (Home and Sound).
  - Final install 20:11 with every fix #1–#43 (no game open); restarted normally, port 9222 closed. One unplanned normal restart at about 20:09 (a restart chained after a build by mistake; no game was open).
  - cargo 75 passed; `regress.mjs` 29/29; `toast-test.mjs` 6/6; `a11y.mjs` 0 problems.
- **Mistake this round:** the second install ran while a game was open (the window list wasn't read first). It re-applied the same profiles and gave the fans to the BIOS for about 2 s. There have been no restarts since. A memory note now requires the game check as its own step.

## 2026-10-12 (night) — sidebar merge, help tips, ping Start/Stop, overlay font list
- **Notes removed** (page, Home tile, nav). `notes` dropped from ALL_TILES; the `notes` field stays in settings.json so saved notes aren't deleted. Settings' tile list skips tiles that no longer exist.
- **Sidebar merged:** Windows = tabs Open windows / Snap zones / Workspaces; Tools (in the PC group) = tabs Screen tools / Locked files / Windows panels. The Misc group is gone. `tabsHead()` in pages1.js; old `go("windows","layouts")` style links still open the right tab.
- **Network ping:** one round when the page opens, then live rounds only between Start and Stop (rounds run back to back, 4 pings each); leaving the page stops it. Strip keeps 60 rounds; row layout fixed so the values no longer overlap the strip.
- **"?" help tips** on every CPU and GPU setting (editors, curve tools, page sections): what it does, why use it, what changing it does, and a warning where there's a catch. `helpTip()` in core.js, texts in new `ui/js/help.js`. 24 px targets; hover/focus shows, click pins.
- **Overlay font:** was a text box with a suggestion list, which only offers entries matching what's typed, so it showed just "Unispace". Now a real dropdown: suggested fonts, then all fonts installed on this PC (new `fonts_list` command, GDI EnumFontFamiliesExW; 280 fonts here).
- Checked: cargo 76 passed (+1 ignored live font test run: 280 fonts); `regress.mjs` 34/34 (5 new checks); `toast-test.mjs` 6/6; `a11y.mjs` 0 problems. Installed at about 21:00 (no game open, checked on its own first; quit 1.6 s); live over CDP: new sidebar, 280-font dropdown, ping Start/Stop. Restarted normally; port 9222 closed.
- Version bumped to 0.2.0; the 0.2.0 setup built (NSIS, current-user install; admin mode is set up from inside Deck with "Set up"). 

## 2026-10-13 — frame stats fixed, power + frametime graphs, Customize, overlay grouping, more "?" tips

- Frame stats (avg, 1% / 0.1% lows, min, max) were never working: the pipe reader called DisconnectNamedPipe every 50 ms while waiting, kicking RivaTuner's hook off, and dropped frames whenever Deck itself was in front. Now read straight from RTSS's per-app frametime ring (see rtss.rs above); follows RTSS's foreground game while it draws, else the last app that drew. A "frame" over 1 s (game minimized / paused) starts the figures over. Checked live: DroidCam 30 fps, osu! ~985 fps.
- Sensors → Graphs: Power (CPU package + GPU board W, `History.cpu_w/gpu_w`), Frametimes (last 600 frames + Now / Average / 1% / 0.1% / Lowest / Highest, from `rtss_state.ft_recent/stats`).
- `layout.js`: Customize on Home and Sensors → Graphs: drag a panel by its bar, drag the corner to resize (twelfths of the width, height in 10 px), buttons for the same, hide / bring back, "Put everything back". Saved in `settings.panels[page]` (`layouts` was taken by snap zones); Home's order/visibility stays in `settings.tiles`. Sidebar width: drag its edge (`settings.nav_width`), double-click resets.
- Overlay "What to show": tick several (Shift-click ranges), move them together (up / down / top / bottom / drag), "Put on one line" with your own label (`osd_style.groups`: item → label; grouped and one-line layouts), remove together. Game RAM now sits on the RAM line.
- "?" tips added for Fans, Lighting, Cooler display, Sensors (alerts, CSV, power, frametimes), the overlay cards, Game boost and three Settings rows (86 entries in help.js).
- In-game frametime graph: RivaTuner's own `EnableFrametimeHistory` graph never showed beside Deck's text. Deck now writes an embedded graph object (RTSS_EMBEDDED_OBJECT_GRAPH 'GR00', FLAG_FRAMETIME, no samples: RTSS fills it) at the start of its OSD slot's object buffer (offset 4608) and ends its text with `<OBJ=00000000>`; the profile key is now always 0 so there is never a second graph. Slot contents checked with `dump_deck_slot`; whether it draws in a game still needs a look in osu!.
- Missing values ("–") were sent to RTSS as "?" (ANSI); now "-".
- Graph v2 (same night): RTSS's FLAG_FRAMETIME graph took a point per frame (unreadable at 1000 fps). Deck now sends its own points: `osd::GraphFeed` makes 120 points over `graph_secs` (5 s – 1 min, default 10), each the average of its frames (`graph_smooth`) or the slowest; `frame_loop` writes the object into Deck's slot each step (`write_graph`), the text loop with each text. `graph_fit` (default on) sizes it to the overlay's longest line (`osd::text_width`). Live: ~12 points/s, slot object 120 points, w −43, autoscaled top.
- Overlay on-delay (was 1–3 s: the loop slept `osd_ms` and took a fresh snapshot first): `rtss::refresh_now()` writes at once from the last snapshot and wakes the loop (`WAKE`); called by the toggle key and by set_setting for osd_on / osd_items / osd_style. Measured with `time_slot_appears`: slot written 4–7 ms after the switch. If RivaTuner itself isn't running, its own start still takes a few seconds.
- Reset key: `osd_reset_hotkey` (hotkey action ResetStats) and `rtss_reset_stats` (buttons on Sensors → Graphs Frametimes and Overlay → Behaviour) clear the figures and the graph.
- Installs: `Start-Process powershell -Verb RunAs -WindowStyle Hidden ... -WindowStyle Hidden -File elev-install.ps1` so no console appears.
- Tests: cargo 83 (+8 ignored, incl. `live_ring_stats`, `dump_rtss_apps`, `dump_deck_slot`); regress 37.
- Version 1.0.0 (2026-10-13): setup built with everything above.

## 2026-10-13 — published on GitHub
- Public repo https://github.com/MohammadFakih02/deck with README (screenshots from the fake-data preview, no real data), MIT `LICENSE`, `THIRD_PARTY_NOTICES.md` (PawnIO GPL-2.0 / modules LGPL-2.1, Fluent icons MIT), `CHANGELOG.md`, CI (`cargo test` + `node --check` on windows-latest), issue templates. Release v1.0.0 with the setup attached.
- Docs cleaned of personal data; personal facts moved to git-ignored `PRIVATE_NOTES.md`, untouched originals in `private/originals/`. Audit reports moved to `docs/audits/` (screenshots stay local in `artifacts/`).
- New rule, written into DECK.md (§0, §9), this file and CONTRIBUTING.md: update the docs before every commit. Enforced by `.githooks/pre-commit` (`git config core.hooksPath .githooks`): code changes need `PROJECT_STATE.md` and `CHANGELOG.md` staged in the same commit.
- CI fix: the UI check step failed on the first run (it also looked for `src-tauri/phone/*.js`, which doesn't exist, so the loop ended on a false test). Now checks `ui/js/*.js` and `ui/guard.js` and stops on the first real error.
