# Proposed architecture and migration plan — 2026-10-17

Grounded in the current code. Deck works; this is incremental refactoring to make the privilege
boundary safe and the code easier to change, not a rewrite. **No frontend framework, no new build
system, no large new dependency.** The UI stays vanilla JS embedded in the exe.

## Principles

1. **One authorization point per trust boundary.** Local UI, phone, CLI and peer PCs are different
   tiers with different rights. Today the Tauri commands and `remote::handle` each decide for
   themselves, and some decide nothing (S5).
2. **The elevated process trusts as little as the user account can change.** Executable path, DLL
   search path, environment flags, config files and file paths are all under the user's control and
   must be validated or dropped before elevated code acts on them (S1–S4, S7, S8).
3. **Every persistent store is durable and recoverable.** Atomic write, fsync, a backup, and a
   defined behaviour when the file is corrupt (R1, R2).
4. **Hardware state always has a way back**, including when Deck is killed (R4).
5. **Thin Tauri commands.** A command validates input, names the caller tier, and delegates to a
   testable action. No business logic in `main.rs`.

## Target shape

```
ui/ (vanilla JS, ES modules)        phone/ (vanilla JS)        CLI / peers
        │ invoke                         │ ws + http                 │
        ▼                                ▼                           ▼
  cmd/ modules (thin #[tauri::command])  remote.rs (thin)      beam/, ccsync/ (thin)
        └──────────────┬─────────────────┴────────────┬────────────┘
                       ▼                               ▼
                 actions::*  (typed actions; each takes a Caller tier)
                       │  authorization decided here, once, per tier
                       ▼
     services (App-owned, buildable in tests):
       sys · media · audio · apps · files · clip · power · disk · cleanup · net · boost
       hw-engine (fans/RGB/LCD/sensors)   gpu-engine (NVML/NvAPI)   rtss
                       │
                       ▼
           store (atomic + fsync + backup + migrations)
           privileged boundary (a medium-integrity helper for user-folder file work)
```

`Caller` is an enum: `LocalUi`, `Phone`, `Cli`, `Peer { id }`. Each action states which tiers may
call it and what each may do (e.g. a phone may set a sleep plan but not open an arbitrary path).

## What stays

- The hw and gpu engines and their safety behaviour (fan release, clamps, heat override, crash guard
  logic). These are the most careful code in the repo. Only their file writes change (atomic).
- The `beam` and `ccsync` wire protocols (authenticated, size-limited, sanitised). Only the trust
  model (S6) and installer elevation change.
- The UI's render model (DOM diffing via `replaceChildren`, held redraws, focus/scroll restore) and
  the preview/regression tooling. Both are good.
- The mandatory "docs before every commit" workflow and the pre-commit hook.
- The 127-field settings *content*; only how it's stored and mutated changes.

## What moves or changes

### 1. Authorization layer (fixes RC-AUTHZ: S5, and the home for S6/S9 checks)
Add `actions/` with typed actions and a `Caller`. `remote::handle` and the Tauri commands both call
it. S5's toast rule lives in `actions::toast_action`, which checks the live toast list and the
caller tier. Benefit: one place to read and test the rules. Cost: a mechanical rewrite of ~20
actions. Compatibility: none broken if the actions match current behaviour. Migrate action by action.

### 2. Privileged boundary (fixes RC-ELEV: S1–S4, S7, S8)
Immediate, no new process: `SetDefaultDllDirectories` + explicit DLL paths (S2); drop env browser
flags when elevated (S3); per-machine install + task-path ACL check (S1); retry-then-fail in
`launch_as_user` (S4). Later: a small medium-integrity helper for file work in user folders (Clean
up, phone/Transfer inbox, Claude Sync, share zips) so arbitrary delete/write as admin via junctions
(S7) becomes impossible, and privileged config moves to an admin-only ProgramData store (S8).
Benefit: closes the main escalation routes. Cost: the helper is real work (L); the four immediate
fixes are each S. Do the four first.

### 3. Store helper (fixes RC-STORE: R1, R2, A4)
One `store::save_json(path, value)` / `load_json_or(path, default, on_corrupt)` with unique temp
name, `sync_all`, a `.bak`, and a corrupt-file policy (keep aside, try `.bak`, then field-merge).
Add a `migrate(version)` step for `settings.json` using the existing `version` field. Point
`settings.rs`, `gpu/guard.rs`, `boost.json`, `shares.json`, `transfer`, `ccsync` at it. Benefit:
R1/R2 fixed everywhere at once; migrations become possible. Cost: M. Compatibility: readers must
accept files without a `.bak`.

### 4. Thin `main.rs` (A1, A3)
Split the 151 commands into `cmd/` modules by domain (window, media, power, disk, hardware, phone,
transfer, …). `main.rs` keeps setup, tray and wiring. Move statics into the services that own them
as each is touched (don't do it all at once). Benefit: a contributor can find and change one domain
without reading 2,100 lines. Cost: M, low risk (moves, not rewrites).

### 5. Frontend ES modules (A7)
Convert the 18 globals-sharing scripts to native ES modules with explicit imports. `script-src
'self'` already allows module scripts; still no build step. Benefit: load order and dependencies
become explicit. Cost: M. Do it last.

## Migration order (each stage leaves Deck shippable)

1. **Security patch A** — S5, S3, S2, S4, S15; R2 and R1 via a first cut of the store helper. All
   small, independent, unit-testable. Ship.
2. **Install model** — S1 + D1 + installer smoke test (T1 part 1). Ship.
3. **Store helper everywhere** — finish R1/R2/A4, add migrations. Ship.
4. **CI gate** — T1 rest (clippy after A6's `never_loop` fix, audit, packaging, UI regression). Ship.
5. **Actions layer** — A2 + fold S5's rule in properly; add the phone transport fix (S9) and trust
   fix (S6) behind it. Ship between sub-steps.
6. **Privileged helper** — S7/S8. Ship.
7. **Strategic cleanup** — A1/A3 split, ES modules (A7), dependency convergence (A5), perf (P1/P2).

## Dependencies between tasks
- A6's clippy error must be fixed before T1 can add a clippy gate.
- The store helper (3) should land before or with R1/R2 so there's one implementation.
- The actions layer (5) is where S5/S6/S9 are cleanest, but S5 and S3/S2/S1 are urgent, so they ship
  first as local patches and are folded into the actions layer later.
- The privileged helper (6) depends on nothing but is the largest item; keep it last of the security work.

## Explicitly not recommended
- A frontend framework or bundler: the UI is maintainable with modules; a build step would fight the
  "embedded in the exe, rebuild to see changes" model and add supply-chain surface.
- Dropping `windows-sys` for `windows` wholesale: convergence only as modules are touched (A5).
- Re-propose NetLimiter-style limits or internet remote desktop: the user declined these.
