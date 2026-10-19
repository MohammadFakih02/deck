# Findings register — 2026-10-17

> Contains unfixed security findings. Keep it out of the public repo until the Must-fix group is released.

Severity: Critical / High / Medium / Low (**security severity** is rated for a typical friend's PC:
UAC on, Deck installed from the release setup). **Priority:** Must fix, High value or Strategic.
**Effort:** S < ½ day, M ½–2 days, L > 2 days. **Conf.** = confidence the finding is real as described.

Shared root causes:
- **RC-ELEV**: the elevated process trusts things the normal user account controls (S1, S2, S3, S4, S7, S8).
- **RC-AUTHZ**: no single authorization point for remote callers (S5, S6, S9, A2).
- **RC-STORE**: every store has its own non-durable save and load (R1, R2, A4).

### Remediation status (updated 2026-10-18; nothing committed yet)

Maintainer decisions that shaped the fixes: S10 hold and ask; S6 installers always ask locally;
protocol compatibility may break (update both PCs, re-pair once); S12 keep custom codes and add a slow KDF.

| ID | Status | What was done | Tests |
|---|---|---|---|
| S1 | **Fixed** | Per-machine NSIS setup (`installMode: perMachine`); `hooks.nsh` removes the old per-user copy. `elevation::setup` refuses a user-changeable exe (`fsguard::user_could_change`: the exe and every parent folder, checked against the limited token). An admin start removes tasks pointing at one and shows a notice. Task XML written to a random file in `Windows\Temp` and held read-only while `schtasks` reads it. | `programs_the_user_could_change`, `the_task_file_cannot_be_swapped`; setup built, **installer not run** |
| S2 | **Fixed** | `SetDllDirectoryW("")`; vendor DLLs (`nvml`, `nvapi64`, `atiadlxx`, `powrprof`) only by full path from System32 or NVSMI (`util::load_trusted_dll`); system folders from known-folder APIs, not env (this also covered the PawnIO driver folder). | `vendor_libraries_come_only_from_trusted_folders`, `system_folders_ignore_the_environment` |
| S3 | **Fixed** | env scrubbed + `--cdp-port`. | 4 in `webview_guard` |
| S4 | **Fixed** | `launch_as_user` retries Explorer for about 3 s, then fails with a message; never an elevated `ShellExecute`. | `a_failing_explorer_is_retried_then_refused`, `explorer_coming_back_is_waited_for` |
| S5 | **Fixed** | `toast_allowed` caller tiers. | 4 in `app::toast_rules_tests` |
| S6 | **Fixed** | Per-PC Ed25519 keys (`beam/device.rs`); id derived from the key; signed hellos; trust and policies keyed to that id (config v2 clears old trust once, keeps "off"). Received installers always wait for a local click and run as the user (`--run-installer`). | `a_pc_cant_be_impersonated`, `a_signed_hello_names_its_pc`, `connections_prove_both_pcs`, `installers_always_ask`, `the_helper_runs_the_job_and_reports_its_code` |
| S7 | **Fixed** | `fsguard.rs`: limited-token `AccessCheck` + handle-relative `NtCreateFile`/`NtSetInformationFile`, folders pinned without `FILE_SHARE_DELETE`, links out of the root refused. Used for phone uploads, the beam inbox, ccsync writes/trash, share zips, Clean up, Recycle and every `write_atomic`. Implemented in-process, not as the medium-integrity helper the register suggested. | 12 in `fsguard`, `a_folder_swapped_after_the_scan_is_not_followed`, `a_link_into_windows_is_still_protected` |
| S8 | **Fixed** | Sharing/serving/sending refuses what the user can't read. Hardware settings are sealed: fingerprints in `HKLM\SOFTWARE\Deck\Seals` written only by admin Deck; on mismatch at an admin start the hardware is held (`HW_HELD`) until **Apply them** (PC only). Non-hardware privileged choices (shared folders, policies) live in user files but every use is checked against the user's rights, so editing them grants nothing. | `reading_for_the_user`, `a_change_made_elsewhere_is_noticed`, `only_the_hardware_part_counts`; **hold not exercised on a real admin start** |
| S9 | **Fixed** | Key never in URLs: session + HMAC-SHA256 per request with single-use counters; Host check (DNS rebinding); hash-based CSP (also U5). | 9 in `phoneauth`; `phone-sign.mjs`; **uploads not exercised end to end** |
| S10 | **Fixed** | `ccsync/guard.rs`: incoming changes to hooks, plugins, statusLine, apiKeyHelper, permissions, env… are held with Apply / Keep this PC's. | 4 in `ccsync::guard` |
| S11 | **Fixed** | Download slot reserved at start, released on failure; home links refused on Funnel requests; Funnel turned off at start unless an internet link is open. | `downloads_reserve_their_place`, `home_links_are_not_served_to_the_internet`, ignored `one_time_link` (passed on 127.0.0.1) |
| S12 | **Fixed** | Custom codes kept; `kdf::stretch` (scrypt ROMix over BLAKE3, 32 MiB, about 0.25 s, cached) for the pairing code and the sync key. | 3 in `kdf` |
| S13 | **Documented** | Accepted risk, written up in `docs/trust-boundaries.md` (off by default, signed requests, paused in games). Per-session local confirmation not built. | — |
| S14 | **Fixed** | `ps_str` escapes every single-quote form; RustDesk password via env or a short-lived file, never PowerShell's command line (RustDesk's own argument remains). | 3 in `rdesk` (one runs PowerShell) |
| S15 | **Fixed** | rejection sampling + panic on RNG failure. | `every_character_is_equally_likely` |
| R1 | **Fixed** | backup + field-merge recovery + atomic save. | 5 in `settings::durability_tests` |
| R2 | **Fixed** | damaged mark → hold; atomic writes. | 3 in `gpu::guard` |
| R3 | **Fixed** | Shared state moved to `parking_lot` (no poisoning) or `unwrap_or_else(PoisonError::into_inner)`; one `lock().unwrap()` left, in a test. | `a_poisoned_lock_still_opens` |
| R4 | **Fixed (partly)** | Watchdog child process (`--watchdog <pid>`) releases fans and the GPU fan when Deck is ended from outside. **Not done:** checking chips for a hardware critical-temperature override. | 3 in `watchdog`; **not exercised on real fans** |
| R5 | **Fixed** | `log.rs`: rotating log in `%LOCALAPPDATA%\Deck\logs`; tail in the support report; hardware engine messages, task changes, refused file actions, failed settings saves and the watchdog are logged (not every `let _ =` in the codebase). | 2 in `log` |
| R6 | **Fixed** | "Shut down when…" plan kept in `plan.json`, restored waiting from the start. | `a_restored_plan_starts_waiting_again` |
| T1 | **Fixed (not run on GitHub)** | CI: clippy `-D warnings`, tests, UI suites, `cargo audit`, NSIS + SHA-256; `release.yml` from `v*` tags; `build.rs` fails with `DECK_REQUIRE_RTSS32=1`. | workflows not yet run (nothing pushed) |
| T2 | **Fixed** | Boundary tests added for every area in the test plan; mapping in the test plan. | see test plan |
| T3 | **Fixed** | regress check 18/19 tab state; 4 pages added to a11y. | `regress.mjs` |
| T4 | **Fixed** | Ignored tests tagged by risk in the test plan. | — |
| U1–U3 | **Fixed** | 24 px hit areas; labels on both send-text boxes. | `a11y.mjs` 0 issues; new regress check |
| U4 | **Fixed** | = T3. | — |
| U5 | **Fixed** | Phone page CSP (with S9). | `csp_*` tests |
| A1 | **Partly done** | Step 1: 56 adapters into 7 `*cmd.rs` modules (`main.rs` 2,122 → 1,780 lines with the new security wiring). The rest (~40 window/dialog-coupled commands) judged optional. | compiler + suite |
| A2 | **Decided against** | The phone's allow-list in `remote::handle` + `toast_allowed` already form the boundary; documented. | — |
| A3 | **Policy** | Move statics into App-owned services as modules are touched. | — |
| A4 | **Partly done** | Every store saves through `util::write_atomic` (now guarded by `fsguard`). Generic recovery/migrations not built (only settings needs recovery today). | — |
| A5 | **Policy** | Converge `windows`/`windows-sys` and mutex types when touching modules (R3 moved most shared state to `parking_lot`). | — |
| A6 | **Fixed** | `never_loop` fixed; clippy warnings 112 → 0; transmutes annotated; four style lints allowed crate-wide with a comment. CI uses `-D warnings`. | clippy |
| A7 | **Fixed** | UI converted to native ES modules (`ui/js/main.js` entry, no build step). | regress 39/39, a11y, toast, phone suites, module-resolution check; **not yet run in the real app** |
| P1 | **Measured, no change** | 1.88 % of one core in total at idle; 5 busy threads, the busiest about 1 %. Nothing worth optimising. | — |
| P2 | **Fixed** | One NVML instance per process (`Nvml::shared()`); `nvml-wrapper` dependency removed. | ignored `one_shared_instance_reads_the_card` (passed live) |
| D1–D3 | **Fixed** | Install location, counts and module list updated; `SECURITY.md` and `docs/trust-boundaries.md` added. | — |

Verification (latest full run, before the last three boundary tests were added): `cargo test --release`
194 passed / 0 failed / 29 ignored; clippy `-D warnings` clean; `regress.mjs` 39/39; `a11y.mjs` clean;
`toast-test`, `phone-shot`, `phone-sign` pass; NSIS setup builds (per-machine confirmed in the
generated script). **Never exercised live:** the installer run, the watchdog on real fans, phone
uploads, CI on GitHub, the two-engine beam/ccsync network tests (they bind 0.0.0.0).

---

## Must fix

### S5 — Phone toast actions are not checked against shown toasts
- **Category:** Security (remote authorization) · **Sev.:** High · **Conf.:** High · **Effort:** S · **Regression risk:** Low
- **Where:** `remote.rs` `handle()` (`"toast"` arm) → `app.rs` `App::toast_action`
- **Evidence:** `toast_action` acts on the id and action it is given without checking that a toast
  with that id exists or offers that action. Some toast kinds open a file path embedded in the id;
  others accept or decline pending transfers or apply a held GPU profile.
- **Impact:** a phone client (any holder of the phone token) can trigger actions well beyond the
  phone's intended scope, including opening arbitrary paths on the PC.
- **Fix:** act only when the toast is currently listed in `app.toasts` and the action is one of its
  declared actions. Resolve received files by `Shared` id, never from a path inside the id string.
  Give the phone an allowlist of toast kinds it may answer (plans, timers, breaks, phone text);
  transfer and GPU decisions stay on the PC.
- **Verify:** unit tests that unknown ids and undeclared actions are ignored; a phone test that a real
  "File from your phone" toast still opens.

### S3 — Elevated WebView accepts browser flags from the user environment
- **Category:** Security (privilege boundary) · **Sev.:** High · **Conf.:** High · **Effort:** S · **Regression risk:** Low
- **Where:** `webview_guard.rs` `browser_args()`
- **Evidence:** the function appends `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS`, which a user can set
  without admin rights. The debug scripts rely on this to open a DevTools port.
- **Impact:** a user-level program can make the elevated page debuggable, and from there reach the
  whole command surface.
- **Fix:** ignore the variable when elevated (except in the `test` profile). Never accept
  remote-debugging, extension or profile-directory switches from it. Give the debug scripts an
  explicit command-line switch instead.
- **Verify:** unit test of the filter; after a normal restart with the variable set, no debug port listens.

### S2 — GPU vendor libraries loaded through the default DLL search path
- **Category:** Security (DLL search order) · **Sev.:** High · **Conf.:** Medium-High · **Effort:** S · **Regression risk:** Medium
- **Where:** `gpu/nvml.rs` (`nvml.dll`), `gpu/nvapi.rs` (`nvapi64.dll`), `hw/sensors.rs` `load_lib`
  (`nvml.dll`, `atiadlxx.dll`), `sys.rs` `Nvml::init()`; no `SetDefaultDllDirectories` call
- **Evidence:** bare-name loads in an elevated process. On any PC at least one of these libraries is
  absent from System32, so the search continues into user-influenced locations.
- **Impact:** the elevated process may load a library that is not the vendor's.
- **Fix:** `SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_SYSTEM32)` at the top of `main()`; load vendor
  DLLs with `LoadLibraryExW(..., LOAD_LIBRARY_SEARCH_SYSTEM32)` or an absolute path (plus the NVSMI
  folder for old NVIDIA drivers); pass an explicit `lib_path` to `nvml-wrapper`.
- **Verify:** Process Monitor on a test VM shows no probes outside System32/NVSMI for these names;
  GPU and sensor pages still work on NVIDIA and non-NVIDIA PCs.

### S1 — Elevated tasks can point at a user-writable executable
- **Category:** Security (install model) · **Sev.:** High · **Conf.:** High (code) / Medium (install folder) · **Effort:** M · **Regression risk:** Medium
- **Where:** `tauri.conf.json` (`installMode: "currentUser"`); `elevation.rs` `setup`, `ensure_task`,
  `ensure_quit_task` (task command = `current_exe()`)
- **Evidence:** the release setup installs per-user; "Set up" then registers highest-privilege tasks for
  that path. `install.ps1` (Program Files) does not have this problem.
- **Impact:** on release installs the elevated tasks run a file the normal user can change.
- **Fix:** ship a per-machine setup (Program Files). Before registering tasks, check that the exe's
  folder is writable only by administrators and SYSTEM, and refuse with a clear message otherwise.
  On start, detect tasks that point at a user-writable path, remove them and ask for a reinstall.
- **Verify:** installer smoke test (task XML points to Program Files; folder ACL check); manual test on a
  clean VM with UAC on.

### R1 — Damaged `settings.json` silently resets to defaults
- **Category:** Reliability (data loss) · **Sev.:** High · **Conf.:** High (path) / Medium (frequency) · **Effort:** S–M · **Regression risk:** Low
- **Where:** `settings.rs` `load`, `save`; `app.rs` saver thread; `main.rs` `quit_deck`
- **Evidence:** any parse error yields defaults and the next save overwrites the file; no fsync before
  rename; the saver thread and `quit_deck` can write the same temp file.
- **Impact:** profiles, workspaces, hotkeys and the phone token lost after one bad shutdown, silently.
- **Fix:** unique temp name + `sync_all` + rename; keep `settings.json.bak`; on a parse error keep the bad
  file as `settings.json.corrupt-<time>`, load `.bak`, otherwise merge the fields that still parse; toast
  the user. Serialise all saves through one path.
- **Verify:** unit tests for truncated file, wrong-typed field, concurrent saves.

### R2 — Crash guard fails open on a damaged mark file
- **Category:** Reliability / hardware safety · **Sev.:** Medium · **Conf.:** High · **Effort:** S · **Regression risk:** Low
- **Where:** `gpu/guard.rs` `hold_back`, `cpu_hold`, `record`, `write`, `rearm`
- **Evidence:** read/parse errors return "don't hold"; marks are written non-atomically.
- **Impact:** after a hard reset during a write, an overclock can be re-applied at boot.
- **Fix:** atomic writes; a file that exists but can't be parsed means "hold".
- **Verify:** unit tests: empty → hold, garbage → hold, missing → no hold.

### T1 — CI misses release blockers; releases are built by hand
- **Category:** Testing / release · **Sev.:** High (engineering) · **Conf.:** High · **Effort:** M · **Regression risk:** Low
- **Where:** `.github/workflows/ci.yml`, `build.rs` `rtss32()`, DECK.md §6
- **Evidence:** CI runs only `node --check` and `cargo test`. No clippy (currently 1 error), no dependency
  audit, no packaging, no UI regression, no installer check; `build.rs` only warns when the 32-bit
  helper is missing; setups are attached by hand without checksums.
- **Fix:** add clippy (correctness lints as errors), `cargo audit`/`cargo deny`, `regress.mjs`,
  `toast-test.mjs`, `a11y.mjs`, `cargo tauri build --bundles nsis` with `DECK_REQUIRE_RTSS32=1` (make
  `build.rs` fail when set), upload the setup with its SHA-256, build releases from tags in CI.
- **Verify:** a branch without the i686 target must fail CI.

---

## High value

### S6 — PC to PC trust keyed to a self-reported device id
- **Category:** Security (peer authentication) · **Sev.:** Medium-High · **Conf.:** High · **Effort:** M–L · **Regression risk:** Medium
- **Where:** `beam/net.rs` `serve_conn`, `receive_offer`, `handle_request`; `beam/config.rs` `Trust`,
  `DevicePolicy`; `beam/setup.rs` `install_async`, `shell_run`
- **Evidence:** the peer's id comes from its own hello message and is also visible in beacons. Every
  device with the pairing code shares one key, so "trusted PC" and "always allow requests" are not
  bound to a device. Installers from trusted peers start from the elevated process.
- **Impact:** trust decisions don't separate devices that share a code; received installers can run with
  admin rights without a local prompt.
- **Fix:** per-device static keys (e.g. X25519) pinned on first pairing, with trust tied to the key.
  Received installers always need a local confirmation and start through the normal UAC path, never
  inherit Deck's token.
- **Verify:** unit tests: a second device with the same code but another key is not trusted; installs
  from trusted peers still prompt.

### S9 — Phone remote token travels in plaintext URLs on the LAN
- **Category:** Security (transport) · **Sev.:** Medium · **Conf.:** High · **Effort:** M · **Regression risk:** Medium
- **Where:** `remote.rs` (all handlers read `?k=`), `phone/index.html`
- **Evidence:** HTTP without TLS at home; the token is in the query string of page, WebSocket, upload,
  file and art requests; no CSP or Host check.
- **Impact:** whoever can observe home-network traffic can obtain full phone-remote access.
- **Fix:** keep the token out of URLs (send it in the first WebSocket message or a header) and
  authenticate requests with a MAC derived from it; add a strict CSP and a Host/Origin check; suggest
  Tailscale (already encrypted) in the UI; offer token rotation on the Phone page.
- **Verify:** unit tests for auth helpers; `phone.mjs` end-to-end with the new handshake.

### S4 — Launching "as the user" can silently fall back to admin
- **Category:** Security (privilege boundary) · **Sev.:** Medium · **Conf.:** High · **Effort:** S · **Regression risk:** Low
- **Where:** `util.rs` `launch_as_user`
- **Evidence:** when the Explorer automation call fails (Explorer not running or restarting), it calls
  `ShellExecute` from the elevated process. Deck itself restarts Explorer when applying a setup snapshot.
- **Fix:** retry briefly, then fail with a message (or start through a de-elevated helper); never run
  user-chosen targets elevated.
- **Verify:** unit-level test with the Explorer path stubbed to fail.

### S7 — Elevated file operations in user-writable folders
- **Category:** Security (filesystem) · **Sev.:** Medium · **Conf.:** Medium · **Effort:** L · **Regression risk:** Medium
- **Where:** `cleanup.rs` (walk, then delete by path), phone uploads (`remote.rs`), Transfer inbox
  (`beam/net.rs`), Claude Sync writes (`ccsync/engine.rs`), share zips (`share.rs`)
- **Evidence:** these paths are resolved and then used later by name; reparse points created in between
  are followed by the elevated process.
- **Fix:** move user-folder file work into a medium-integrity helper process; until then, open handles
  with `FILE_FLAG_OPEN_REPARSE_POINT`, verify the final path is still under the intended root, and
  delete or rename by handle.
- **Verify:** tests that create a junction mid-operation in a temp folder and assert nothing outside
  the root changes.

### S8 — Elevated behaviour configured from user-writable files
- **Category:** Security (configuration trust) · **Sev.:** Medium · **Conf.:** Medium · **Effort:** M · **Regression risk:** Medium
- **Where:** `%APPDATA%\Deck\settings.json`, `transfer\config.json`, `hardware\*.ini`; `share_create`, shared folders
- **Fix:** keep privileged choices (shared folders, request policies, share paths, hardware limits) in an
  admin-only store under ProgramData, or confirm them in the UI when they change outside Deck; refuse
  to share or serve paths the signed-in user can't read themselves.
- **Verify:** tests that a path the user can't read is refused for sharing.

### S10 — Claude Sync gives the sync key code-execution reach
- **Category:** Security (trust model) · **Sev.:** Medium · **Conf.:** High · **Effort:** S–M · **Regression risk:** Low
- **Where:** `ccsync/` (settings.json, memory, CLAUDE.md are synced)
- **Evidence:** synced Claude Code settings can contain hook commands that run on every synced PC.
- **Fix:** don't sync `hooks` (or `permissions`) by default; when an incoming change touches them, hold
  it and ask on that PC. Explain the trust model on the Claude Code sync page.
- **Verify:** unit test that a remote settings change with new hooks is held.

### S11 — Share link limits and Funnel scope
- **Category:** Security / correctness · **Sev.:** Medium · **Conf.:** Medium-High · **Effort:** S–M · **Regression risk:** Low
- **Where:** `share.rs` `download`, `counted`, `tidy`, `funnel`, `init`
- **Evidence:** downloads count only when the last byte is sent, so parallel or partial downloads
  are not limited; LAN-only links are served on the same paths the Funnel exposes; Funnel state isn't
  reconciled on start.
- **Fix:** reserve a download slot when a download starts (released on failure); serve only
  `internet` links to Funnel requests; on start always turn Funnel off unless an internet link is open.
- **Verify:** ignored live test extended: two parallel downloads of a one-time link → one succeeds.

### R3 — Poisoned std mutexes can bring Deck down
- **Category:** Reliability · **Sev.:** Medium · **Conf.:** Medium · **Effort:** M · **Regression risk:** Low
- **Where:** 171 `lock().unwrap()` (mostly `hw/`, `beam/`, `ccsync/`); `main.rs` `tray_menu`
- **Fix:** use `parking_lot` or `unwrap_or_else(PoisonError::into_inner)` on shared engine state; never
  unwrap a worker's lock on the main thread.
- **Verify:** test that panics while holding `status` and then builds the tray menu.

### R4 — No fail-safe when Deck is killed
- **Category:** Hardware safety · **Sev.:** Medium · **Conf.:** Medium · **Effort:** M · **Regression risk:** Medium
- **Where:** `hw/mod.rs` `stop`, `on_panic`, `release_fans_direct`; `main.rs` hooks
- **Evidence:** release runs on quit, panic, native exceptions and session end, but not when the
  process is terminated; fixed fan duties and a manual GPU fan then persist.
- **Fix:** a small watchdog task that releases fans and the GPU fan when Deck's heartbeat stops;
  check whether supported chips offer a hardware critical-temperature override and enable it.
- **Verify:** test session: set a fixed duty, end the process, confirm release within seconds.

### R5 — No diagnostic log for the main app
- **Category:** Reliability / supportability · **Sev.:** Medium · **Conf.:** High · **Effort:** S–M · **Regression risk:** Low
- **Where:** release builds have no console; `eprintln!` ×7; `let _ =` ×481
- **Fix:** a small rotating log in `%LOCALAPPDATA%\Deck\logs`; log failed hardware writes, task/firewall
  changes, settings saves; include the tail in the This PC report.
- **Verify:** unit test of rotation; support report contains the log tail.

### T2 — Security boundaries have no tests
- **Category:** Testing · **Sev.:** Medium · **Conf.:** High · **Effort:** M · **Regression risk:** None
- **Where:** no tests in `beam/net.rs`, `beam/crypto.rs`, `ccsync/engine.rs`, `settings.rs`, phone auth
- **Fix / verify:** see the test plan (sanitisers, handshake, trust, toast authorization, settings
  recovery, guard, share counting).

### T3 — Stale UI check and incomplete page coverage
- **Category:** Testing · **Sev.:** Low · **Conf.:** High · **Effort:** S
- **Evidence:** `regress.mjs` check 18 looks for the font list on the default tab (it moved to Look);
  `all.sh` (used by `a11y.mjs`) lacks Overlay, Drives, PC to PC, Claude Code sync.
- **Fix:** open the Look tab in check 18; add the four pages to `all.sh`.

### A2 — Two implementations of the same remote actions
- **Category:** Architecture · **Sev.:** Medium · **Conf.:** High · **Effort:** M · **Regression risk:** Medium
- **Where:** `main.rs` commands (`set_awake`, `set_plan`, `timer_*`, `run_power`, profile switch) vs `remote.rs` `handle`
- **Fix:** one `actions` module with typed actions and a caller tier (local UI, phone, CLI, peer); both
  front ends call it. This is where S5-style checks belong.

### U1–U3 — Small accessibility gaps
- **U1** Disk space tree toggles 18×18, Home tile-settings link 22×26 → at least 24×24 hit area.
- **U2** Overlay item checkboxes 16×16 outside a label → wrap in a label or enlarge the hit area.
- **U3** PC to PC "Text or a link to send…" textarea → add an `aria-label`.
- **Sev.:** Low · **Effort:** S · **Verify:** `a11y.mjs` with the four pages added reports 0 small targets.

---

## Strategic

| ID | Category | Sev. | Finding | Fix | Effort |
|---|---|---|---|---|---|
| A1 | Architecture | Medium | `main.rs` 2,119 lines, 151 commands, window/tray/CLI/business logic mixed | Per-domain `cmd/` modules, `main.rs` only wiring | M |
| A3 | Architecture | Medium | ~79 global statics beside `App` | Move into App-owned services as touched | L |
| A4 | Architecture | Medium | Eight stores, eight save routines; `set_setting` takes any key; `version` unused | One store helper (atomic, fsync, backup, migrations); per-key validation | M |
| A5 | Maintainability | Low | `windows` + `windows-sys`, std + parking_lot mutexes | Converge when touching modules | L |
| A6 | Code quality | Low | Clippy error `never_loop` (`beam/net.rs:903`), ~100 warnings, un-annotated FFI `transmute`s | Fix error first, annotate transmutes, then gate | S–M |
| A7 | Frontend | Low | 18 scripts share globals with load-order coupling | Native ES modules (allowed by `script-src 'self'`), no build step | M |
| P1 | Performance | Low | Idle ≈ 3 % of one core; hot corner polls at 33 Hz; per-thread split unknown | Measure per thread elevated before changing anything | S |
| P2 | Performance | Low | NVML opened in 3 places | One GPU telemetry service | M |
| S12 | Security | Low | Pairing fingerprint in beacons + user-chosen codes + fast KDF | Generated codes only, or a slow KDF | S |
| S13 | Security | Low | Screen control from an elevated process reaches elevated windows | Document; per-session local confirmation | S |
| S14 | Security | Low | `ps_str` misses Unicode quotes; password on command lines | Pass via stdin/env, escape all quote characters | S |
| S15 | Security | Low | `random_token` ignores RNG failure, modulo bias | Fail hard, rejection sampling | S |
| D1 | Docs | Medium | Install location in DECK.md/README contradicts the release setup | Fix with S1 | S |
| D2 | Docs | Low | Stale counts (118→122 tests, 34→37 UI checks, ~80→151 commands); §5 module list incomplete | Update; generate counts in CI | S |
| D3 | Docs | Low | No trust-boundary page, no `SECURITY.md` | Add both | S |
| T4 | Testing | Low | 29 ignored live tests, no documented safe subset | Tag by risk, document | S |
| R6 | Reliability | Low | "Shut down when…" plan lost on restart (known) | Persist with the store helper | S |

## Phase 4 verification (2026-10-18): new findings

Found by the independent re-check (a separate read-only reviewer for the newest Rust, a release-artifact check and live
measurements). All fixed in the working tree, not committed.

| ID | Status | Finding | Test |
| --- | --- | --- | --- |
| V1 | **Fixed** | 32-bit RivaTuner helper imported `VCRUNTIME140.dll`; without the x86 VC++ runtime the overlay key couldn't flip RivaTuner. Built with `+crt-static`. | `rtss::the_32_bit_helper_needs_no_visual_cpp_runtime` |
| V2 | **Fixed** | Per-thread clocks capped at nominal: PDH caps `% Processor Performance` at 100 without `PDH_FMT_NOCAP100`. | live `live_thread_clocks` under load (E-cores 2748 MHz vs 2600 nominal) |
| V3 | **Fixed** | Battery/plugged-in switch lost if the power source changed during Game boost or a hardware hold. | `a_power_change_while_held_waits_until_the_hold_ends` |
| V4 | **Fixed** | Turning "Profiles also apply on battery" on bypassed the seal hold and re-activated a plan the user may have left. | static review; `rewrite_active` returns while held |
| V5 | **Fixed** | Deleting a CPU profile left it chosen for plugged in / on battery (error toast on each plug/unplug). | `a_deleted_profile_is_not_switched_to` |
| V6 | **Fixed** | Clipboard page put a 236,000-character item on the page in full (150-206 ms to open). | regress "a huge clipboard item shows a short preview" |
| V7 | **Fixed** | `Cpu` never closed its two PDH queries (leak per hardware restart). | static review |
| V8 | **Fixed** | Theme-blind colours (Midnight greys hard-coded in buttons, switches, toasts, calendar). | screenshots in 5 themes |
| V9 | **Fixed** | RivaTuner's shared-memory header was trusted: any program creating the mapping first could make the elevated Deck read or write outside it. Every reader/writer now goes through `array()` (bounds from `VirtualQuery`, re-read each time); `szName` read capped at 260. Live: 256 apps + 8 OSD slots in a 5.58 MB mapping, unchanged. | `a_lying_rivatuner_header_stays_inside_the_mapping` |
| V10 | **Open (cosmetic)** | `STATIC_VCRUNTIME is deprecated` warning from tauri-build; the static runtime still applies (`dumpbin /dependents`). | — |
