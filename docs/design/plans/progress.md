# Progress ledger — plan: docs/design/plans/2026-10-11-hardware-control.md

Ruling: no git repo — no worktree/commits; isolation = backup D:\Backups\deck-before-hw-20261011-1817 — cost if wrong: manual restore from backup.
Pre-flight: T4 produces hw::Hw used by T5/T6 (RAPL, min lock, OC, game detect); T3 produces gpu::nvml/nvapi consumed by T8; T6/T8 profiles consumed by T10 switch. No conflicts found.
Task 1: complete (elevation.rs, launch_as_user, install/uninstall, --setup-task, --launch-test; tests: cargo test elevation → 2/2; --launch-test unelevated → child Medium). Elevated Explorer path verified at install (Task 11).
Task 1: Ruling: old Deck install is NSIS at %LOCALAPPDATA%\Deck (not Programs\Deck) — install.ps1 removes both + Run key + HKCU uninstall entry — cost if wrong: leftover old exe.
Task 2: complete (NAV Hardware+Misc, hw page filter, icons). Render check folded into Task 5 renders.
Scope change (user): Afterburner fully replaced + uninstalled, RTSS kept → spec §6.7b/§6.10, plan Task 10b.
Task 3: complete (read-only probe). Results (test card, driver 596.36): NVML power 100–270 W (def 240), core offset ±1000, mem −2000..+6000, 2 fans 30–100%, temp limit acoustic 65 (65–90), all writes reachable (NO_PERMISSION unelevated). NvAPI: VF mask 0x182C v1 (132 pts), table get needs mask at +4, curve 0x1C28 v1 entries {freq kHz, volt uV,...} stride 28 @0x48, table stride 36 @0x44 type@+0 delta@+20. GPU pts 0..126, mem 127..131 (editable = type 0). Live state = Afterburner flat ~990 MHz + mem −500 on pts 130/131.
Task 3: Ruling: clocks use Afterburner's mechanism (VF table deltas: uniform delta = core offset, per-point = curve, mem offset on editable mem points); NVML offsets kept at 0 (reset if non-zero) — parity with AB imports, one mechanism, no stacking — cost if wrong: mem offset semantics differ from NVML's.
Task 3: Ruling: elevated write tests run from install (Deck.exe --gpu-probe --write --out file) since this shell isn't admin — cost if wrong: first real write happens at install.

## 2026-10-11 (continued)
Task 4: complete (backend compiles; 37/37 tests).
Task 6: complete (profiles/cpu.rs: plan write/adopt/create, extras, Game Mode, watcher, clock check, --cpu-probe).
Task 8: complete (afterburner.rs import + stop tuning/undo; fixture tests on this PC's real cfg).
Task 10: complete (hotkeys.rs, shortcuts.rs, icons.rs + 25 icons, tray submenus, phone chips, Game boost profiles).
Task 10b: complete (rtss.rs OSD slot + MAHM v2; layout tests).
Tasks 5/7/9 (UI): complete (curve.js, hw.js, profiles.js; offscreen renders checked).
Ruling: Afterburner slots 2-5 hold shaped curves (not plain offsets): imported as curves — matches what AB applied — cost if wrong: user re-picks offset mode.
Ruling: crash guard treats raised curve points only at/above 850 mV as overclock, so flattened undervolts (the user's daily profiles) still auto-apply — cost if wrong: an unstable undervolt re-applied after a crash.
Ruling: GPU temp limit "card's own" = never touched (was re-applying the 65 °C seen at start) — cost if wrong: none.
Ruling: stock VF curve cached when seen at stock, else reconstructed monotonic; editor draws the driver-clamped (suffix-min) curve.
Ruling: proc VRAM overlay item dropped (RTSS shows it itself).
Ruling: Afterburner bare Page Up overlay key not imported (would steal Page Up everywhere).
Removed: hw/uv.rs, MoboCtl plan-preset code, unused plan helpers.
Final review 1: 0 critical, 8 important — all fixed:
Final: fixed profile id collisions (new_id counter) — ids_unique_in_a_tight_loop RED→GREEN.
Final: fixed "Try it" escaping crash guard (records guard; reverted on every modal dismissal).
Final: fixed guard gaps (mark_clean first, interruptible worker sleeps, rearm() at start).
Final: fixed endless driver-reset re-apply (once per apply; never for risky profiles).
Final: fixed 40 s startup freeze on boost recovery (own thread).
Final: fixed min-lock restored into wrong plan (CpuMinPlan GUID).
Final: fixed DC (battery) values overwritten (AC always, DC only for edited fields).
Final: fixed uninstall leaving GPU fan pinned (--quit, fuller --release-fans, narrower shortcut match).
Minors fixed: watcher race (SWITCHING), reactivate w/o High plan, OSD ANSI degree sign, stale RTSS slot, stop race, async heavy commands, AB re-import dedupe + canonical hotkeys, redraw leak, 4500 MHz/PT- hardcodes, Delete hotkey, power label, AB scan cache, native crash filter.
Final: minor (deferred): AMD GPU page monitoring-only view; launch_as_user fallback toast; over-the-shoulder UAC on standard accounts; panic toast for stopped engine; three GPU pollers.
Final review 2: 10 findings, all confirmed and fixed: OC shifts each turbo bin (shift_bins test), held-back OC stays held across a clean quit, --quit w/o Deck doesn't mark crash clean, profile power modes leave EPP to the plan + custom undoes saver window/overlay, card's temp limit restored, save() under SWITCHING, apply() undoes old lock before writing, partial GPU apply still active, startup/boost ordering via gpu_done, state commands async. Suite 39/39.
