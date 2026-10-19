# Open questions and environmental limits — 2026-10-17

> **Resolved 2026-10-17/18.**
> - **Q1:** the release setup is now per-machine (S1 fixed).
> - **Q3:** received installers always ask on this PC and run as the user.
> - **Q4:** incoming hooks and permissions are held for Apply on each PC.
> - **Q6:** custom codes kept, with a slow KDF.
> - **Q5:** fixed regardless of where the phone is used (signed requests, S9).
> - **Q2:** the fixes assume the stricter answer (code already running as the user).
>
> Compatibility breaks were approved: update both PCs and re-pair once.
>
> In "Things to verify" below:
> - **Answered:** P1 was measured (1.88 % of one core; see the migration report); the dependency audit
>   runs in CI; `nvml-wrapper` was removed.
> - **Still open:** the live hardware paths, now including the watchdog, and the installer run.

## Questions for the maintainer (change severity or fix)

1. **Release install location (S1, D1).** *Partly answered 2026-10-17:* the maintainer's own
   `deck.exe` is in **Program Files** (installed via `install.ps1`), so S1 does **not** apply to the
   maintainer's PC. Still open: the public **GitHub release setup** is built with
   `installMode: "currentUser"` ([tauri.conf.json](../../src-tauri/tauri.conf.json)), so a friend who
   runs it gets a per-user install under `%LOCALAPPDATA%`, where the elevated tasks point at a
   user-writable exe. S1 should be confirmed with a clean-VM install of the **release setup** (not
   `install.ps1`), and the fix is to make the release setup per-machine.
2. **Who are "friends" in the threat model?** Is the worry an attacker already running code as the
   user, or only trusted people on the same LAN? This changes whether S2/S3/S4/S7 are "fix now" or
   "defence in depth". The report assumes the former because the repo is public and Deck runs elevated.
3. **Is unattended auto-install of received apps/setups wanted at all (S6)?** If "trusted PC → silent
   elevated install" is a deliberate convenience, the fix is to bind trust to a device key and keep a
   local prompt for elevation; if not, received installers should never run without a click.
4. **Claude Sync and hooks (S10).** Do you intend `~/.claude/settings.json` hooks to propagate between
   your PCs? If yes, document it as "the sync key can run code on all synced PCs"; if no, exclude
   `hooks`/`permissions` by default.
5. **Phone over plain HTTP (S9).** Is the phone remote meant to be used on untrusted Wi-Fi, or only at
   home + Tailscale? If home-only, the token-in-URL fix is still worth doing but the severity is lower.
6. **Custom pairing codes (S12).** Is letting users type their own 20-char code a feature to keep? If
   generated-only is acceptable, S12 closes cheaply.

## Things to verify that this audit could not

- **Per-thread idle CPU (P1).** `GetThreadDescription`-based attribution needs an elevated shell;
  the non-elevated run returned nothing. Re-run from an elevated PowerShell to confirm whether the
  33 Hz hot-corner poll or the 1 s loops dominate the ~3 % figure before optimising anything.
- **Dependency vulnerabilities (T1).** `cargo-audit`/`cargo-deny` aren't installed; no CVE scan was
  run against `Cargo.lock`. Add one in CI. (`axum 0.8`, `tokio 1.53`, `chacha20poly1305 0.10`,
  `nvml-wrapper 0.13` were not checked against the advisory DB.)
- **Live hardware paths.** Fan release, GPU apply/restore, the heat override and the kill-case
  fail-safe (R4) were read, not exercised. The 29 ignored live tests were not run.
- **Installer/updater/uninstaller behaviour.** Not executed (would change the system and could prompt
  the firewall). S1, the per-user vs per-machine migration, and the quit/release sequence need a
  clean-VM pass.
- **Real-window and full-theme visual review.** Per the hard rule, no real desktop capture. The
  offscreen preview covers structure, naming, overflow and script errors, not pixel-level polish of
  all 8 themes and low-spec mode at every size; that still wants a human look via `shot.mjs`.
- **AMD/Intel-only and no-GPU PCs.** Code paths were read (pages hide, ADL load exists) but not run on
  such hardware; S2's non-NVIDIA assumption and the page-hiding logic want a real test there.
- **WebSocket flow control and screencast under a real slow link.** The ack/pad logic was read, not
  load-tested from an actual phone over Tailscale.

## Assumptions made
- "A typical friend's PC" means UAC on and the release setup used; the maintainer's own PC has UAC off
  (per PRIVATE_NOTES), where S1–S3/S7/S8 add no privilege because everything already runs as admin.
- The uncommitted working-tree doc edits are intentional and were left untouched.
- Severity ratings weigh "runs elevated + public repo" heavily; if the real audience is narrower
  (question 2), several Medium items drop to Low.
