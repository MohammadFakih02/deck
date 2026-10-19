# Deck engineering audit — 2026-10-17

> **Unfixed security findings below. Do not push this folder to the public repo until the
> "Must fix" items in the findings register are fixed and released.** Publishing first would
> hand out working privilege-escalation recipes for every PC that runs the current release.

Scope: the whole repository at `main` = `64b9e15` (Deck 2.5.5), plus the uncommitted doc edits
already in the working tree (documentation only; no code was changed by this audit).

Companion documents (same date):

| Document | What it holds |
|---|---|
| [`2026-10-17-findings-register.md`](2026-10-17-findings-register.md) | Every finding with ID, severity, confidence, evidence, fix, effort, regression risk, verification |
| [`2026-10-17-architecture-plan.md`](2026-10-17-architecture-plan.md) | Target architecture and migration order |
| [`2026-10-17-test-plan.md`](2026-10-17-test-plan.md) | Tests and checks that verify each fix and guard against regressions |
| [`2026-10-17-open-questions.md`](2026-10-17-open-questions.md) | Unresolved questions and the limits of this audit |

---

## 1. Summary

Deck is in better shape than its size suggests. Its hardware code is careful: fans fall back to
the BIOS when a sensor is lost, there is a 20 % floor and a forced 100 % above 85–90 °C, GPU
values are clamped to the driver's ranges, and there is a crash guard. The UI has a working
regression harness that sends real input, with fake backends, an accessibility sweep and
deliberate focus and redraw handling. Transfer and Claude Sync paths from the other PC are
sanitised, and both channels use authenticated encryption.

The weak spot is the **privilege boundary**. Deck runs elevated without a UAC prompt, so on a PC
where UAC is on (most friends' PCs) **the elevated Deck becomes a bridge from "any program running
as the user" to "administrator"**. Four confirmed routes cross that bridge (S1–S4), and several
further capabilities from the user's side reach elevated code (S7, S8). Separately, **the phone
remote can run any program on the PC** through an unchecked toast action (S5). Both have small,
local fixes.

The second theme is **data durability on a PC that hard-resets**. `settings.json` silently becomes
defaults when it can't be parsed and is then overwritten (R1). The GPU crash guard fails open when
its own file is damaged (R2).

The third theme is **release confidence**. CI runs only `node --check` and `cargo test`.
Release setups are built by hand on one PC. Clippy, dependency audit, packaging, UI regression
and installer behaviour are not checked anywhere (T1). The documentation's test and command
counts have drifted.

### Highest-risk areas
1. **Elevation model**: tasks pointing at a per-user exe (S1), the DLL search path (S2), the WebView2
   debug flags taken from the user's environment (S3), elevated file work in user folders (S7).
2. **Phone remote authorization**: toast actions (S5), plaintext token on the LAN (S9).
3. **PC to PC trust**: trust and "always allow" are keyed to an id the other side claims (S6);
   received installers run elevated.
4. **Settings and guard files on crash** (R1, R2).
5. **CI and release pipeline** (T1).

---

## 2. Baseline (2026-10-17)

| Check | Result | Notes |
|---|---|---|
| Repo state | `main` = `origin/main` (`64b9e15`), working tree has uncommitted edits to `CHANGELOG.md`, `DECK.md`, `PROJECT_STATE.md`, `README.md`, `docs/screenshots/overlay.png` (pre-existing, left untouched) | |
| Toolchain | stable MSVC, targets `x86_64-pc-windows-msvc` + `i686-pc-windows-msvc`; Node present; Edge present | `cargo-audit` and `cargo-deny` **not installed** |
| `node --check` on all 20 UI/phone scripts | **20/20 OK** | |
| `cargo test --release` (6 jobs, BelowNormal) | **122 passed, 0 failed, 29 ignored**, 2 m 37 s build | Docs say 118; the docs are stale |
| Build warnings | 4 dead-code warnings (`kbps`, `vendor`, `nvmlDeviceGetTargetFanSpeed`, a struct's fields) + `STATIC_VCRUNTIME` deprecation | No rtss32 warning this run (cached build script) |
| 32-bit RTSS helper | `rtss-osd32.exe` built (96 KB) and present in `OUT_DIR` (98 304 bytes) | `build.rs` still only *warns* when it's missing |
| `cargo clippy --release --all-targets` | **1 error** (`never_loop`, `beam/net.rs:903`), ~100 warnings | Error is deny-by-default; blocks any future clippy gate |
| `ui-preview/regress.mjs` | **36/37 PASS**, 1 FAIL | Failure is a stale test: the overlay font list moved to the *Look* tab in 2.5.5; on that tab it has 16 options. Docs say 34 checks |
| `ui-preview/toast-test.mjs` | **6/6 PASS** | |
| `ui-preview/a11y.mjs` 1260×900 and 900×600 | 0 unnamed controls, 0 unlabelled fields, 0 overflow, 0 script errors on the 32 views it visits; small targets: Home `.tile-set` 22×26, Disk space tree toggles 18×18 (×8) | Its page list (`all.sh`) misses **Overlay, Drives, PC to PC, Claude Code sync** |
| Extra check of the 4 missed pages | No script errors, no overflow. PC to PC: 1 textarea named only by its placeholder. Overlay: item checkboxes 16×16 outside a label | Scratch script, not committed |
| Idle cost of the installed Deck (hidden, in-game, overlay on), 30 s | Deck process **0.92 s CPU / 30 s ≈ 3 % of one core**, 21 MB working set, 63 threads, 786 handles; WebView2 processes ≈ 0 | Per-thread breakdown not possible from a non-elevated shell |

Not run, on purpose: live hardware tests (the 29 ignored tests), installers, `elev-install.ps1`,
anything binding 0.0.0.0, Deck's real window, and dependency vulnerability scanning (tool not
installed; installing it means compiling it, and that wasn't needed to finish the audit).

---

## 3. How Deck is built (as verified in code)

```
            ┌────────────── elevated Deck.exe (scheduled task, HighestAvailable) ───────────────┐
 tray ──────┤ main.rs: window control, tray, 151 #[tauri::command]s, CLI modes, panic hooks    │
 hot corner │      │                                                                           │
 hotkeys    │      ▼                                                                           │
            │ App (app.rs): settings Mutex + debounced saver, toasts, change broadcast,        │
 WebView2 ──┤   services: sys, media, apps, files, clip, power, disk, cleanup, net, boost      │
 (ui/, IPC) │   engines:  hw (fans/RGB/LCD/sensors, PawnIO driver), gpu (NVML/NvAPI), rtss     │
            │ + ~79 global statics (PINNED, CONNS, share::ST, beam/ccsync engines, …)          │
            │                                                                                   │
 phone ─────┤ remote.rs  axum on 0.0.0.0:47830  (HTTP, token in ?k=)  ── handle() actions      │
 browser ───┤ share.rs   axum on 0.0.0.0:47837  (token in path; Tailscale Funnel for internet)  │
 other PCs ─┤ beam/      UDP 47833 beacon + TCP 47834 (pairing code → ChaCha20-Poly1305)        │
 other PCs ─┤ ccsync/    UDP 47835 + TCP 47836 (sync key) + cloud "hub" folder (sealed blobs)   │
            └───────────────────────────────────────────────────────────────────────────────────┘
   launch_as_user(): asks Explorer (medium integrity) to start programs; falls back to elevated ShellExecute
```

* **Frontend.** `ui/index.html` loads 18 plain scripts that share globals (`S`, `Pages`, `h()`,
  `invoke`). A page is `{ load, render, tick? }`, and `render()` rebuilds `#content` with
  `replaceChildren`. Redraws are held while the mouse is down, and focus, scroll and `<details>`
  state are restored. The only `innerHTML` uses are backend-built QR SVGs and static icons.
  CSP: `script-src 'self'`.
* **IPC.** One capability file grants `core:default` and a few window permissions to all four windows.
  All 151 custom commands are callable from every window (Tauri 2 allows app commands unless an
  app manifest restricts them).
* **Settings.** One 127-field `Settings` struct with `#[serde(default)]`. `set_setting(key, value)`
  round-trips the whole struct through JSON and runs side effects in a `match`. There are other
  stores too: `transfer/config.json`, `claude-sync/config.json`, `hardware/settings.ini` (MoboCtl
  keys), `shares/shares.json`, `gpu-applied.json`, `cpu-oc-applied.json`, `boost.json`,
  `gpu-defaults.json`. Each has its own load and save code, and only `settings.json` and the hub
  write atomically.
* **Workers.** Started from `App::start` and `main::setup`: settings saver, power loop (1 s),
  hot corner (30 ms poll), clipboard listener, hotkeys (message loop + 1 s / 15 ms poll), CPU plan
  watch (2 s), GPU loop (1 s), hw workers (fans 1 s, lights 33–120 ms only when effects need it,
  sensors), RTSS (5 s), SMART (10 min), housekeeping (60 s), Transfer and Claude Sync threads,
  share expiry (60 s), WebView watchdog. Most have no stop signal and live for the whole process.
  Only hw has an orderly shutdown (fans back to the BIOS).
* **Tests.** 122 unit tests, concentrated in `osd`, `screencast`, `h264`, `afterburner`, `power`,
  `curve`, `disk`, `boost`. **None** in `beam/net.rs`, `beam/crypto.rs`, `ccsync/engine.rs`,
  `settings.rs`, or the phone server's auth paths.

---

## 4. Security assessment

Severity here is *security* severity for a typical friend's PC (UAC on, Deck installed from the
release setup). On a PC where UAC is off, every program already runs as admin, so S1–S3, S7
and S8 add nothing there.

| ID | Sev. | Conf. | Finding |
|---|---|---|---|
| S1 | High | High | Elevated scheduled tasks run whatever `Deck.exe` path set them up; the release setup installs per-user (`installMode: currentUser`), so the exe is user-writable. |
| S2 | High | Med-High | `nvml.dll` (3 places), `nvapi64.dll`, `atiadlxx.dll` loaded by bare name in the elevated process; no `SetDefaultDllDirectories`. On every PC at least one of them is missing from System32, and the search falls through to PATH, which the user can extend. |
| S3 | High | High | `browser_args()` appends `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS` from the environment, so a user-level `--remote-debugging-port` gives any local process an unauthenticated DevTools session into the elevated page and every command. |
| S4 | Medium | High | `launch_as_user` falls back to an **elevated** `ShellExecute` when Explorer automation fails (Explorer restarting, which Deck itself causes in setup-snapshot apply). |
| S5 | High | High | Phone toast actions reach `App::toast_action`, which never checks that the toast was shown or offers that action. Some kinds open a path taken from the id, accept pending transfers, or apply a held GPU profile. |
| S6 | Med-High | High | PC to PC "trust" and "always allow requests" are keyed to the `id` the other side sends in its hello (also broadcast in clear beacons). Anyone with the pairing code can claim a trusted id: auto-accepted installers run **elevated** (silent `runas`, or a plain ShellExecute from the elevated process). |
| S7 | Medium | Medium | The elevated process deletes and writes in user-writable folders by path: Clean up (walk, then `remove_file` later), phone uploads, Transfer inbox, Claude Sync, share zips. A junction swapped in between gives an arbitrary delete or write as admin. |
| S8 | Medium | Medium | Elevated behaviour is driven by user-writable config (`%APPDATA%\Deck\…`): for example a shared folder plus an "always" policy plus the pairing code (all user-readable or writable) makes admin-only files readable over localhost; `share_create` serves any path. |
| S9 | Medium | High | Phone remote is plaintext HTTP on the LAN with the token in every URL (`?k=`), including WebSocket and downloads; one sniffed request = full remote. No CSP, no Host check on the phone page. |
| S10 | Medium | High | Claude Sync syncs `~/.claude/settings.json` (hooks run commands), memory and CLAUDE.md: the sync key (or hub write access + key) = code execution on every synced PC. Inherent, but undocumented and unconfirmed in the UI. |
| S11 | Medium | Med-High | Share links: the download limit is counted only when a download reaches the last byte (parallel or range downloads bypass `max`); LAN-only links are reachable through the Funnel path; a Funnel left on by a crash is not turned off at the next start. |
| S12 | Low | High | Pairing-code fingerprint in clear UDP beacons gives an offline oracle; custom codes are allowed and the KDF is a single BLAKE3 derive (fine for generated ~99-bit codes, weak for chosen ones). |
| S13 | Low | High | Screen *control* mode sends input from an elevated process (UIPI doesn't apply): phone control = admin-level desktop. Off by default. |
| S14 | Low | High | `ps_str` escapes only ASCII `'`; PowerShell also treats `‘ ’ ‚ ‛` as quotes (RustDesk password, local input only); the password also appears in process command lines. |
| S15 | Low | High | `random_token` ignores a `getrandom` failure (would produce `aaaa…`) and has modulo bias. |

Details, evidence and fixes: findings register.

**What holds up well:** the Tauri CSP (`script-src 'self'`) and DOM-built UI; phone page escaping
(`esc()` everywhere); `open_url` scheme allowlist; Transfer `clean_rel` / `clean_component`
(rejects `..`, drive and ADS colons, reserved names) and `resolve_shared` (canonicalise, then
containment check, `..` refused, symlinks not followed in walks); Claude Sync `safe_rel` +
`classify` before every write; mutual challenge–response before any frame is parsed, frame size
limits, per-direction keys; phone upload size and free-space checks; firewall rules limited to
private/domain profiles; test modes bound to 127.0.0.1.

---

## 5. Reliability and hardware safety

| ID | Sev. | Finding |
|---|---|---|
| R1 | High | `Settings::load` returns defaults on any parse error and the next save overwrites the file: profiles, workspaces, hotkeys and the phone token are lost. No `.bak`, no fsync before rename, and two writers share one temp name (`quit_deck` saves on the main thread while the saver thread may be writing). |
| R2 | Medium | `gpu::guard::hold_back`/`cpu_hold` return "don't hold" when the mark file can't be read or parsed, and the marks are written non-atomically. A hard reset mid-write re-applies the overclock: the exact case the guard exists for. |
| R3 | Medium | 171 `std::sync::Mutex::lock().unwrap()` calls; a panic in an hw worker that holds `status`/`hw`/`dp` poisons it, and `tray_menu` (main thread) then panics. One worker bug ends Deck. |
| R4 | Medium | No out-of-process fail-safe: if Deck is killed (End task, fast-fail abort, stack overflow), fixed fan duties and NVML manual GPU fan stay until reboot; the in-process heat override is gone. |
| R5 | Medium | No diagnostic log for the main app: release builds have no console, so the 7 `eprintln!`s are lost and 481 `let _ =` discard results (task creation, firewall rule, settings save, hardware writes). |
| R6 | Low | "Shut down when…" plans don't survive a restart (known); `boost.json` restore exists. |

Verified good: fans release on quit, panic (main or hw thread), unhandled native exception, session
end, `--release-fans` and uninstall. In curve mode a lost sensor hands the header back to the BIOS.
There's a 20 % floor and 4 %/s ramp-down, and 100 % above CPU 85 °C / source 90 °C. GPU power,
offsets, curve and fan are clamped to driver ranges. The crash guard logic itself is unit-tested.
The GPU startup profile waits for the guard.

---

## 6. Performance

No confirmed bottleneck. Measured idle cost is about 3 % of one core for the Deck process (in a game
with the overlay on), with WebView2 near zero while hidden: `guard.js` asks for one frame per 4 s
instead of a rAF loop, and `tick()` stops when hidden. Page switches in the headless preview take
6–93 ms. Suspected, unmeasured: the always-on 33 Hz hot-corner poll (clones a `String` per poll),
three separate NVML handles polling the same counters (known), and `phone_state` rebuilt per phone
every 1.5 s, which makes COM audio calls. None justify work before the security items. The first
step is a per-thread CPU breakdown from an elevated shell (see open questions).

---

## 7. UI, UX and accessibility

The previous UI audit's fixes hold. The regression suite covers lost clicks, focus kept across
redraws, dialog focus traps, Space on buttons and named controls, and passes except for one stale
check. Remaining items, all Low:

* **U1** Disk space tree toggles are 18×18 and Home's tile-settings link is 22×26 (under WCAG
  2.5.8's 24 px).
* **U2** Overlay item checkboxes are 16×16 and not inside a label (the row is a plain `div`).
* **U3** PC to PC "Text or a link to send…" textarea has only a placeholder for a name.
* **U4** The a11y sweep and `all.sh` skip four pages, so new pages escape the gate.
* **U5** Phone page: no CSP (defence in depth for S9).

No full-desktop or real-window screenshots were taken. Visual review of all 8 themes and low-spec
mode was not repeated beyond the previous audits (see open questions).

---

## 8. Architecture and maintainability (short form)

* `main.rs` (2,119 lines) holds window control, tray, 151 commands, CLI modes and debug dumps.
  DECK.md says "about 80 commands".
* The phone protocol (`remote::handle`) re-implements the same actions as the Tauri commands
  (awake, plan, timers, power, profiles, toasts) with different validation. S5 exists because
  the authorization rule lives in neither place.
* About 79 global statics alongside `App`, so services can't be built in isolation for tests.
* Settings: one large struct, a stringly-typed `set_setting`, no migrations although a `version`
  field exists, and eight stores with eight save routines.
* Two Windows binding crates (`windows` 0.62 and `windows-sys` 0.61) plus both std and parking_lot
  mutexes.
* Clippy isn't clean, and FFI function pointers are `transmute`d without type annotations.

The full proposal is in the architecture plan. Its main point is one **actions layer** with a
caller tier (local UI / phone / CLI / peer), one **store** helper, and a **privileged boundary**
that shrinks over time. No framework, no new build system.

---

## 9. Testing, CI and release

* CI (`.github/workflows/ci.yml`) runs `node --check` and `cargo test --release --locked` only.
  It does not run clippy, dependency audit, `cargo tauri build`, `regress.mjs`/`toast-test.mjs`,
  or an installer smoke test, and doesn't fail on the rtss32 warning.
* Release setups are built locally and attached by hand, with no checksums, no signature and no
  provenance. A local build on an unstable PC is also a reproducibility risk.
* Security boundaries without tests: phone auth and actions, toast actions, Transfer path
  sanitisers, trust, the handshake, Claude Sync merge, deletion and tombstones, share counting,
  settings recovery, crash-guard failure modes.
* Stale counts: 118 → 122 tests, 34 → 37 UI checks, "about 80" → 151 commands.

---

## 10. Documentation and developer experience

* **Install location contradiction.** DECK.md §1 and the README describe Program Files plus
  tasks, but the release setup installs per-user. That is S1 from the user's side.
* No threat-model or trust-boundary page (ports, what each authenticates, what runs elevated),
  and no `SECURITY.md` for reporting vulnerabilities in a public repo.
* DECK.md §5 module table omits `osd.rs`, `h264.rs`, `share.rs` and several UI files; the counts
  above are stale.
* `PROJECT_STATE.md` (72 KB) is the de-facto design history. It's good for continuity but not an
  onboarding path. A one-page architecture overview would let a contributor start without it.
* The mandatory "docs before every commit" workflow and pre-commit hook work as described and
  should stay.

---

## 11. Recommended sequence (from the register)

1. **Patch release A (security, small and local):** S5 (toast authorization), S3 (ignore the env
   flags when elevated), S2 (`SetDefaultDllDirectories` + System32-only loads), S15, the R2
   fail-closed guard, and the R1 settings backup and recovery. Each is independent and unit-testable.
2. **Patch release B (install model):** S1, a per-machine setup or a refusal to create elevated
   tasks for a user-writable exe, together with the D1 doc fix and an installer smoke test in CI.
3. **Trust and transport:** S6 (per-device keys or a pinned first-use secret; never run received
   installers elevated without a local prompt), S9 (per-request MAC or header token, CSP), S11.
4. **CI gate (T1):** clippy (after the `never_loop` fix), `regress.mjs` + `toast-test.mjs` on
   `windows-latest`, `cargo audit`, `cargo tauri build`, checksums on releases.
5. **Strategic:** the actions layer and caller tiers, the store helper, a diagnostic log, and a
   de-elevated helper for file work in user folders (S7/S8). Then thin `main.rs`.

Each step leaves Deck usable, and the steps are kept as separate patches so a regression can be
bisected to one change.
