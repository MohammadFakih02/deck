# Deck

Read `DECK.md` before doing anything; section 0 is written for you. `PROJECT_STATE.md` has the dated history. If `PRIVATE_NOTES.md` exists (git-ignored, the maintainer's machine only), read it too: it has this PC's paths, hardware and profiles.

## RULE 1 — update the docs before every commit (always, without being asked)

Before **every** `git commit`:
1. Add or extend today's dated entry in `PROJECT_STATE.md`.
2. Add user-visible changes to `CHANGELOG.md` (they become the release notes).
3. Update `DECK.md` and `README.md` wherever a fact changed.
4. Stage those files **in the same commit** as the code.

`.githooks/pre-commit` refuses a commit that changes code without `PROJECT_STATE.md` and `CHANGELOG.md`. If it blocks you, update the docs — never use `--no-verify`. The full checklist is `DECK.md` §9.

## Other rules
- Never capture the real screen. Screenshots of Deck's page over CDP are fine.
- Don't change hardware (fans, clocks, power plans, audio, display) while the user may be gaming. Keep each test under 2 minutes, and put everything back afterwards.
- No input hooks and no injection into games. It must work on friends' PCs: no names, paths or ids specific to one PC. The only Raw Input user is the controller overlay's keyboard & mouse mode (receive-only, only while that mode is on); see `DECK.md` §0 rule 3.
- The repo is **public** (https://github.com/MohammadFakih02/deck). Never commit personal data: local paths, hostnames, PC specs, game lists, IPs, screenshots of the real desktop. Those go in `PRIVATE_NOTES.md` or `artifacts/` (both git-ignored).
- The UI is embedded in the exe: rebuild (`cargo build --release`) and reinstall (`tools\testing\elev-install.ps1`, elevated) to see UI changes. Finish with `tools\testing\restart-normal.ps1`.
- Commit only when asked. Never add Claude attribution (Co-Authored-By, "Generated with Claude Code") to commits or pull requests.

**Reminder: docs first, then commit (RULE 1).**
