# Architecture migration report — 2026-10-17 (Phase 3)

Tracks the incremental architecture refactor approved in `2026-10-17-architecture-plan.md`. Each step
preserves features, performance, safety properties and user-facing behaviour. **No code is committed.**

## Principles applied

- Thin transport/UI adapters around testable application logic (plan principle 5: thin Tauri commands).
- Clear module ownership; reads like the surrounding code (follow the existing convention, don't invent one).
- Smallest coherent change, buildable and green after each step.
- No new dependency, no framework, no build step, stack unchanged (Tauri 2 + Rust + vanilla JS).

## Step 1 — Thin `main.rs`: extract command adapters (plan item 4 / finding A1)

### Problem
`main.rs` was 2,122 lines. It held 98 inline `#[tauri::command]` functions for a dozen unrelated
domains (audio, display, net, disk, system, boost, phone, …) alongside the genuine `main.rs`
responsibilities (setup, tray, window control, `icon://` protocol, CLI modes, `generate_handler!`).
A contributor changing one domain had to scroll past all the others, and the command adapters
(pure JSON translation) were not visually separated from the stateful window/tray code.

### Intended improvement
Move each domain's thin command adapters into their own `<domain>cmd.rs` module, leaving `main.rs`
with setup/tray/window wiring and only the commands that genuinely depend on it.

### Design decision — follow the existing convention, not the plan's sketch
The plan sketched a `cmd/` subdirectory. Code inspection showed the repo already uses a **flat
`<domain>cmd.rs`** convention (`hwcmd.rs`, `profcmd.rs`), each declaring its own `type A`/`type R`
aliases and registered as `module::command` in `generate_handler!`. Following the existing convention
means zero new structure to learn and matches the two modules already there, so Step 1 uses flat
`*cmd.rs` files. (Recorded here per the plan's "explain the change before proceeding".)

### What moved (56 commands)
| New module | Commands | Delegates to |
|---|---|---|
| `audiocmd.rs` | audio_state, set_output, set_master, set_app_volume, media_control (5) | `audio`, `app.media` |
| `displaycmd.rs` | display_state, set_refresh, set_brightness, set_hdr, set_dark, locate (6) | `display`, `app.settings` |
| `netcmd.rs` | net_info, net_public_ip, net_ping, net_speed, net_speed_cancel, game_ping_games, game_ping (7) | `app.net`, `net`, `gameping` |
| `diskcmd.rs` | disk_scan/status/cancel/treemap/types/path/children/big, recycle, cleanup_scan/state/run (13) | `app.disk`, `app.cleanup`, `disk`, `cleanup` |
| `syscmd.rs` | processes, kill, kill_pid, startup_list, startup_set, who_uses, support_report (7) | `app.sys`, `winmgr`, `startup`, `locksmith`, `support` |
| `boostcmd.rs` | boost_state, boost_apps, boost_run, boost_pick (4) | `app.boost`, `boost` |
| `phonecmd.rs` | remote_info, remote_reset_token, rdesk_status, rdesk_do, share_state/create/stop, transfer_state/do/apps/setup_preview, claude_sync_state/do, shared_remove (14) | `remote`, `rdesk`, `share`, `beam`, `ccsync` |

`main.rs`: **2,122 → 1,734 lines.**

### What was deliberately left in `main.rs`
Commands coupled to `main.rs`'s statics, tray, window helpers or file dialogs — moving them needs the
shared helpers lifted first, which is a separate, more careful step:

- Window/show-hide: get_state, live, hide, set_pinned, quit, set_setting, search, open_* , games/fonts (use `PREV_FG`, `PINNED`, `PIN_RECT`, `monitor_*`, `hide_deck`, `step_aside`).
- Clipboard: clip_* (clip_copy → `paste_back`, which owns `PREV_FG`/foreground logic).
- Power/timer: set_awake, set_plan, cancel_plan, run_power, timer_*, stopwatch (tray + handle).
- Toast: toast_action, toast_resize (`toast_action_as`, window sizing).
- Workspaces/windows: windows_list, window_action, snap_window, ws_* (monitor math, `step_aside`).
- Screen tools + dialogs: screen_tool, pick, pick_many, share_to_phone (`DIALOG`, `file_dialog`).
- index_rebuild, index_info (tiny; belong with `search`, which stays).
- elevate_setup (setup/CLI wiring).

### Behaviour that must stay unchanged — and the guard
Command set, each signature, and registration order are identical; only the function's home module
changed. Because these are pure relocations (no logic edited), the guard is the compiler plus the
existing suite, not new tests. Writing "command exists" tests would assert nothing real.

### Verification
- `cargo check` (throttled `-j 6`, BelowNormal, shared target dir) after **every** domain: no errors,
  **no new warnings** — only the 7 pre-existing dead-code warnings (`nvml`, `hw::sensors`, `h264`,
  `screencast`, `osd`).
- Full `cargo test --release -j 6`: **140 passed, 0 failed, 29 ignored**, identical to the Phase 2
  baseline, including `asset_tests::pages_are_embedded` (exercises `generate_context!`).
- No performance impact: same code, same call paths; the adapters compile to the same thing.

## Step 2 — One atomic-write path for JSON stores (plan item 3, findings R1/R2/A4)

### Problem
Phase 2 added `util::write_atomic` (unique temp + `sync_all` + rename-with-retry) and used it for
`settings.json` and the GPU crash-guard marks. But other JSON stores still either hand-rolled their own
temp+rename (`clipboard` history) or wrote non-atomically with plain `fs::write` (`beam` config and
received-items inbox, `boost` active-state). A crash mid-write truncated those files, and there were
three slightly different atomic-write implementations in the tree.

### Change
Pointed the stores that hold costly-to-lose data at the single `util::write_atomic`:
- `clipboard.rs::save` — was tmp+rename; now one call (gains `sync_all` + rename retry).
- `beam/config.rs::save_cfg` and `save_inbox` — were plain `fs::write`; now atomic (received-items list
  no longer truncates on a crash).
- `boost.rs::save` — was plain `fs::write`; now atomic (the restore-state survives an interrupted save).

### Deliberately left as-is (evidence-based deviation from the plan)
- **`ccsync` chunk store (`hub.rs`, `engine.rs`, `config.rs`).** `hub.rs` has its own `write_atomic`
  that also creates parent directories and whose temp files are filtered by a fixed `.ccsync-tmp`
  suffix inside a content-addressed store. Folding it into `util::write_atomic` would need extra
  abstraction (parent creation, temp-suffix contract) that the ~6-line dedup does not justify, and the
  sync engine is concurrency-sensitive. Left untouched; documented here.
- **Currency-rate cache (`calc.rs`) and app-list cache (`beam/apps.rs`).** Regenerated data; a
  truncated cache is simply rebuilt, so hardening adds churn without protecting anything. Left as-is.

### Verification
Full `cargo test --release -j 6`: **140 passed, 0 failed**. No behaviour change on success (same file
contents); the only difference is crash-time atomicity. `clipboard::tests` round-trips the history file.

## Step 3 — Fix the `never_loop` clippy error (finding A6; unblocks a clippy gate)

### Problem
`beam/net.rs::request_job` wrapped its single request/answer exchange in a `loop` whose every match arm
`return`s, so the body ran exactly once — a misleading no-op that clippy rejects with a deny-by-default
`never_loop` **error**, which stopped `cargo clippy` from completing (plan stage 4's CI clippy gate was
therefore impossible).

### Change
Removed the `loop` wrapper; the cancel-check-then-dispatch body is unchanged and still runs once, so
behaviour is identical. The `_` arm was **not** changed to `continue` (that would alter behaviour and
risk a real infinite loop; the original intent is unknowable, so the safe fix matches what the code
does today). Added a one-line comment explaining the one-request/one-answer shape.

### Verification
`cargo clippy` now completes with **no errors** (100 pre-existing style/dead-code warnings remain, none
blocking). Full test suite green. This path is a networked transfer request; like the other `beam`
network paths its live behaviour is exercised only by the `#[ignore]` network tests, so the fix is
verified by compile + clippy + the loop being a proven no-op, not by a live transfer.

## Decision — no unified `actions::` layer (deviation from plan item 1)

Plan item 1 proposed routing every command and `remote::handle` arm through a typed `actions::` module
carrying a `Caller` tier. Code inspection shows the trust boundary is **already consolidated per tier**:
`remote::handle` is a single explicit allow-list of exactly what an authenticated phone may do, and the
Tauri commands are the Local tier. The one dangerous primitive shared across tiers (toast →
`shell_open`, S5) was already fixed in Phase 2 with the `Caller`/`toast_allowed` check. Building a
unified actions layer would be a large mechanical rewrite of ~20 phone arms plus ~90 commands whose
only security benefit is already captured — the "rigid layered architecture / abstraction without
meaningful benefit" the Phase 3 brief warns against. Instead the phone's allow-list in `remote::handle`
now carries an explicit trust-boundary comment (the security context the brief asks to preserve). If a
future change needs a third caller tier (CLI, peer) to share a capability with the phone, revisit then.

## Decision — frontend ES-module conversion deferred (plan item 5) — *superseded by Step 5 below*

The UI is 8,044 lines across 18 classic scripts loaded in a fixed order and sharing a global `S` state.
Converting to ES modules means resolving that global sharing plus CSP/load-order — the plan ranked it
**last**, and the audit rated the current render model good. It cannot be validated here: the headless
regress/a11y/toast suites cover rendering and named-control accessibility but not focus lifecycle,
listener cleanup or real pointer/keyboard interaction, so an autonomous conversion would risk
regressions nothing here can catch. Deferred as the remaining major step, with a recommended approach
below. The backend command extraction kept every command **name** unchanged, so no UI `invoke` contract
moved; the regression suite (37/37) and a11y remain green.

## Status of the plan's items after this session

- **Item 4 (thin `main.rs`):** Step 1 done — 56 delegation commands extracted. The ~40 commands coupled
  to main's window/tray/dialog statics are intentionally left (see below).
- **Item 3 (atomic JSON persistence):** Step 2 done for the costly-to-lose stores; a full generic
  `store::save_json`/`load_json_or` + `settings.json` migrations is still open (see below).
- **Item 1 (actions layer):** decided against as over-abstraction; trust boundary documented instead.
- **Item 5 (frontend ES modules):** deferred, with the approach below.
- **A6 (`never_loop`):** fixed.

## Remaining debt / recommended next steps (as written after Step 3)

> Since then: items 3 and 4 are done (Steps 5 and 4 in the continuation below); item 2's write path is
> unified (Step 7); item 1 stays optional.

Each shippable on its own:

1. **Finish the thin-`main.rs` move.** Extract the coupled command groups *after* lifting their shared
   helpers: give `paste_back`/`hide_deck`/`step_aside`/`file_dialog` and the `PREV_FG`/`PINNED`/`DIALOG`
   statics a small owner (e.g. a `window` module for the focus/paste helpers, a `dialog` helper for the
   pickers). Higher risk than Step 1 because it touches focus/paste and dialog ownership — do one helper
   cluster at a time with the UI regression suite watching. Judgement call: window control legitimately
   belongs with main's tray/lifecycle, so this is optional polish, not a clear win like Step 1 was.
2. **Generic `store::` + settings migrations (rest of item 3).** If a third store needs corrupt-recovery
   (not just atomic write), generalise `settings.rs`'s load/merge/backup into `store::load_json_or` and
   add a `migrate(version)` step keyed off `settings.json`'s `version` field. Only worth it when a real
   schema migration is needed; today the duplication is small.
3. **Frontend ES modules (plan item 5, finding A7).** Recommended approach when tackled: convert
   bottom-up (leaf helpers like `icons.js`, `core.js` first) to `export`/`import`, switch `index.html`
   to a single `<script type="module">` entry that imports the rest, keep `S` as one exported module
   object rather than a global, and after **each** file run `regress.mjs`, `a11y.mjs`, `toast-test.mjs`
   plus a manual focus/alt-tab pass (which the headless suite can't do). No build step; CSP already
   allows module scripts. Do last.
4. **Clippy gate (plan stage 4).** The `never_loop` error is gone, so `cargo clippy` now completes;
   before turning on `-D warnings` in CI, clear the 100 remaining warnings (mostly the pre-existing
   dead-code items in `h264`/`osd`/`screencast`/`nvml`/`sensors` and `io::Error::new(Other, _)` →
   `io::Error::other(_)` style lints). Those dead-code items sit in hardware/media modules and should be
   removed only with the ability to test the affected paths, so they were left untouched here.

---

# Continuation — 2026-10-17/18 (after "finish everything")

The steps below came after the security work in the findings register (S1–S15, R3–R6). Each one
kept the build green and the suites passing. **Nothing is committed.**

## Step 4 — Clippy gate (plan stage 4, finding A6)

- **Problem.** 112 clippy warnings, so CI couldn't gate on them.
- **Change.** Fixed them one by one: real fixes, plus dead code removed from `h264`, `osd`,
  `screencast`, `nvml` and `sensors`. Code that only tests use became `#[cfg(test)]`. Every FFI
  `transmute` is annotated with its function type.
- **Allowed lints.** Four lints are allowed crate-wide as deliberate house style, with a comment in
  `main.rs`: `too_many_arguments`, `type_complexity`, `needless_range_loop` and
  `field_reassign_with_default`. Rewriting those would make the hardware code harder to read for no
  safety gain.
- **Gate.** CI runs `cargo clippy --all-targets -- -D warnings`.
- **Checking `--fix`.** Two `match` guards that `--fix` rewrote were checked by hand: both fall
  through only to `_ => {}`, so behaviour is unchanged.

## Step 5 — Frontend ES modules (plan item 5, finding A7)

- **Problem.** 18 classic scripts shared globals and depended on their load order.
- **Change.** Every UI script is a native ES module.
  - Every top-level declaration that another file uses is `export`ed and `import`ed.
  - `index.html` loads `guard.js` (still classic) and then one entry, `js/main.js`, which imports the
    modules in the old order.
  - `toast.html` and `zones.html` load `js/toast.js` and `js/zones.js` as modules.
  - No build step. The CSP already allowed `'self'` module scripts.
- **Mechanics.** A throwaway analyzer (tokenizer, top-level declarations, cross-file references,
  cross-file writes) produced the import lists, and a resolution check confirmed all 19 modules resolve.
  - It found no duplicate globals and no cross-file assignments. Shared state was already the single `S` object.
  - Its false positives were fixed in the tool, not by hand: default parameter values, `if`/`for` read
    as methods, object keys, a second destructured name, and `from`.
- **Load order.** Two hazards came from import cycles.
  - `Pages` was read while modules loaded, before its declaration ran. It moved from `pages1.js` into
    `core.js`.
  - `boot()` moved from `app.js` to `main.js`, so it runs after every module has loaded.
  - `app.js` gained `resetSide()` for `pages3.js`, instead of `pages3.js` writing app's variables.
- **Tests.** The live-testing scripts and the headless suites call page functions by name. `main.js`
  puts exactly those names on `window`; `DECK.md` §0 has the list.
- **Verification.** Run headless against the fake backend:
  - `regress.mjs` 39/39, including "no script errors";
  - `a11y.mjs` clean;
  - `toast-test`, `phone-shot` and `phone-sign` pass.
- **Not yet checked:** the real app (Tauri WebView), including focus and alt-tab, which the headless
  suite can't see.
- **Backup.** The pre-conversion UI was kept outside the repo for comparison.

## Step 6 — One NVML instance (finding P2)

- **Problem.** NVML was loaded in three places: the GPU engine through Deck's own loader, the Home
  tiles through the `nvml-wrapper` crate, and the Sensors page through a third loader.
- **Change.** `gpu::nvml::Nvml::shared()` is a `OnceLock` instance loaded through the trusted DLL path
  (S2). Read accessors were added: `temp`, `util_rates`, `memory`, `clock`, `power_mw`.
  - `sys.rs` (Home), `hw/sensors.rs` (Sensors), the GPU engine, probe and release all use it.
  - The `NvmlUtil`, `NvmlMem` and `NvmlExtra` structs and the `nvml-wrapper` dependency were removed.
    That is one dependency fewer, and allowed by the "removing one is fine" rule.
- **Verification.** The suite passes. The ignored test `one_shared_instance_reads_the_card` passed on
  an NVIDIA card. The JSON shape of the Home tile is unchanged.

## Step 7 — One guarded write path (plan item 3, findings A4/S7)

- **Problem.** Step 2 left `ccsync` with its own atomic writer. S7 then needed every write the elevated
  process makes in user folders to respect the user's rights.
- **Change.**
  - `util::write_atomic` now goes through `fsguard::write_for_user`: a temp file, then a rename by
    handle, retried, and only where the signed-in user could write.
  - `ccsync` (`hub`, `config`, `locks`, `scan`, the engine state) and the hardware `.ini` files now
    use it, so the "own writer" exception from Step 2 is gone.
  - Conflict copies, trash moves and project deletes go through `fsguard::move_for_user`, with a copy
    fallback across drives.
- **Rename semantics.** The rename uses `FileRenameInformationEx` with POSIX semantics, so it can
  replace a file another process has open (a reader can't block a save). It falls back to the classic
  rename on older systems.
- **Verification.** The `fsguard` tests (12), `a_reader_never_sees_a_half_written_file` and the
  settings durability tests all pass.

## Decisions on the remaining plan items

- **A1 (rest of thin `main.rs`).** Left as optional polish. The ~40 remaining commands own window
  focus, paste-back and dialog state. `main.rs` is 1,780 lines; it grew again from 1,734 because of the
  watchdog, the seal, `--run-installer` and the install-location wiring, which belong in startup.
- **A2 (actions layer).** Still decided against; see above. S6, S9 and S10 were each fixed at their
  own boundary.
- **A3 (global statics).** Policy: move them into App-owned services when a module is touched. No big
  move.
- **A4 (store helper).** The write path is unified (Step 7). A generic `load_json_or` with recovery and
  migrations waits for a store that needs it; today only `settings.json` does, and it has its own.
- **A5 (two Windows crates, two mutex types).** Policy, as for A3. R3 already moved most shared
  state to `parking_lot`.
- **P1 (idle CPU).** Measured on 2026-10-17 instead of guessed: 1.88 % of one core in total, with
  5 busy threads and the busiest about 1 %. Nothing is worth changing. Thread names weren't readable
  without elevation, so the threads were recorded by id.

## Status of the plan's items now

| Item | Status |
|---|---|
| 1 Actions layer | Decided against (documented) |
| 2 Privileged boundary | Done in place: `fsguard`, `seal`, the install-location check, trusted DLLs, the env scrub (see `docs/trust-boundaries.md`) |
| 3 Durable stores | Write path unified; generic recovery and migrations deferred until needed |
| 4 Thin `main.rs` | Step 1 done; the rest is optional |
| 5 ES modules | Done; still to try in the real app |
| Stage 4 CI | Done: clippy gate, audit, UI suites, setup build, tag releases (not yet run on GitHub) |

## Compatibility and safety

The Steps 1–3 notes below still hold for those steps. The continuation **deliberately** changed some
things:
- **PC to PC and Claude Sync wire protocols**: per-PC keys and the slow KDF. Both PCs must be updated
  and re-paired once; the maintainer approved this.
- **Phone request format**: signed requests. The page reloads once.
- **The install location** (per-machine).
- **What the elevated process may do in user folders.**

No settings were renamed or reset. Trust lists were cleared once, on purpose, and "off" choices kept.
No user data is deleted.

Steps 1–3: no settings format, wire protocol, hardware path, privilege boundary or UI contract was changed in any
step. Command **names** (the UI/phone `invoke` contract) are unchanged. The hw/gpu engines, crash guard,
anti-cheat constraints, local-only test binding and the elevated/normal process split are untouched.
Atomic-write changes alter only crash-time durability, not success-path behaviour or file contents.
