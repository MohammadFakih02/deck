# Updates, one-step setup and crash protection (design and plan)

Written 2026-10-19. Builds on `elevation.rs`, `watchdog.rs`, `hooks.nsh`, `gpu/` and the release workflow.

## Why
- After every install or update Deck starts as a normal user, so the Hardware pages show "Set up" again.
  Cause: the setup's finish page and its `/R` restart both use `RunAsUser`; the admin tasks are fine.
- After a GPU driver update Deck is ended from outside ("ended from outside; watchdog released the
  hardware") and nothing starts it again.
- There is no update path at all: friends reinstall by hand.

## Goals
1. One UAC prompt, in the setup, and nowhere else: fresh install, manual update and auto-update all end
   with Deck running as admin through its tasks, hidden, without "Set up".
2. Deck updates itself from GitHub releases, silently, when no game is in front.
3. Deck comes back by itself after a crash or a driver install, and never loops.
4. Installing, updating and restarting never bring a window to the front.

## Non-goals
- Signed updates (the SHA-256 comes from the same release, so it catches damage, not a hijacked release).
- Moving the GPU code into a helper process (decide after the crash reports show what fails).
- Standard users who give another account's admin password to the setup: the tasks are made for the
  account the setup runs as.

## Design
### A. Setup does the set-up
- `hooks.nsh` `NSIS_HOOK_POSTINSTALL`: run `deck.exe --setup-task` (the setup is elevated: per-machine),
  then, for silent, passive and update installs, start Deck through the new `\Deck\Background` task.
  A GUI install's "Run Deck" box still starts it as the user; see B.
- New trigger-less task `\Deck\Background` runs `deck.exe --minimized`: used by the setup, the
  watchdog and the updater, so a start is always hidden. Made and checked by `elevation::setup` /
  `ensure_quit_task` like the others.
- B. Handover: an unelevated `deck.exe` whose exe is the one `\Deck\Open` runs starts that task and exits
  before any window exists (`elevation::handover`). Covers the finish page, the Start menu, a desktop
  shortcut. A 20 s marker file stops a loop if the task does not run elevated. Without the tasks
  (portable copy, declined UAC) nothing changes: the banner stays.

### C. Watchdog restarts Deck
- `watchdog::wait_for` returns how Deck ended (clean, ended with exit code N, nothing to watch).
- After an unclean end: release the hardware (as now), record it in `crashes.json`, log the exit code
  and uptime, wait until no driver installer runs (at least 15 s), then run `\Deck\Background`.
- `restart::decide(history, now)` (pure): 3 unclean ends within 10 minutes means give up, write
  `safe.json`. A clean quit never restarts. Setting `restart_after_crash` (default on).
- Safe mode: at start `safe.json` holds the hardware back exactly like the existing `HW_HELD` state (fans
  on the BIOS, no overclock) with a toast "Deck stopped 3 times in a row" and an "Apply" button that
  clears it.
- The watchdog starts for every elevated Deck with tasks, not only when the hardware engine starts.
- The crash filter writes the exception code and module to the log before Windows reports it.

### D. Driver installs
- `driver::installing()` (pure on a process list): a process running from a known installer folder
  (NVIDIA `Installer2` / `NVIDIA\DisplayDriver`, AMD `AMD\...\Setup`, Intel graphics installer) or named
  like a display driver setup. Best effort; the restart backoff covers the rest.
- A watcher thread (5 s) sets `gpu::PAUSED` while one runs. `Nvml.f` becomes a wrapper whose `Deref`
  gives an empty function table while paused, and `NvApi::func` returns None: every GPU call fails
  cleanly instead of touching a driver that is being replaced. No call site changes.
- When the install ends and 15 s have passed, Deck restarts itself cleanly (`\Deck\Background`) with a
  toast, so NVML starts fresh and the profile is applied again.

### E. Updater (`update.rs`)
- Check at start + 2 min, then every 24 h: `GET api.github.com/repos/MohammadFakih02/deck/releases/latest`
  (ETag, 10/30 s timeouts, retry in an hour on failure). `update.json` keeps last check, etag, known
  version and the version last run.
- Newer = numeric `X.Y.Z` greater than this build. Assets: `*-setup.exe` and its `.sha256`; both URLs must
  start with `https://github.com/MohammadFakih02/deck/releases/download/`.
- Download (at most 150 MB) to `Program Files\Deck\updates` when admin (only admins can write there), or the
  user's Temp when not. Check SHA-256, hold the file open without write sharing while starting it.
- Install when `settings.auto_update`, no game in front (`boost::game_in_front`), no fullscreen app: run
  `setup.exe /S /UPDATE` (admin Deck: no prompt; otherwise one UAC prompt, only on the user's click).
  The setup quits Deck cleanly, installs, re-creates the tasks and starts Deck hidden.
- Next start with a new version: toast "Deck updated to X". Settings has a switch, "Check now" and the
  status line; the tray is unchanged.

## Tests
Pure: version compare, asset pick, checksum file parse, URL allowlist, restart decision, installer-folder
match, handover decision. Live (manual): end Deck with `taskkill /f` and watch it return hidden; run the
setup silently over an install; point the updater at a test release.

## Plan (order)
1. elevation: Background task, handover, setup creates it; hooks.nsh POSTINSTALL; main.rs handover call.
2. watchdog: outcome with exit code, `restart.rs` (history, decide, safe.json), restart, always-on watchdog,
   safe-mode toast, crash filter detail.
3. driver.rs + PAUSED gate in nvml/nvapi + watcher + clean restart.
4. update.rs + settings + commands + UI + hook flags.
5. Docs (DECK.md, README, CHANGELOG, PROJECT_STATE), clippy, tests, UI checks. No commit, no push.
