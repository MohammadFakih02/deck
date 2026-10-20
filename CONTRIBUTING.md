# Contributing to Deck

## The one rule: update the docs in every commit

Every commit that changes code must also update the docs, in the same commit:

| File | What to add |
|---|---|
| `PROJECT_STATE.md` | A dated entry: what changed, why, and what was tested (and what wasn't). |
| `CHANGELOG.md` | User-visible changes under the version being worked on, or `## Unreleased`. These become the release notes. |
| `DECK.md` | Any fact that changed: pages, settings, commands, file layout, test counts, gotchas. |
| `README.md` | The feature list, screenshots or build steps, if they changed. |

Turn on the hook once after cloning, and it will refuse code commits that skip `PROJECT_STATE.md` and `CHANGELOG.md`:

```sh
git config core.hooksPath .githooks
```

AI assistants (Claude and others) follow the same rule; it is also written in `CLAUDE.md` and `DECK.md` §0 and §9. Don't bypass the hook with `--no-verify`.

## Keep it public-safe

The repo is public. Don't commit local paths, PC names, hardware specs, IP addresses, game lists or screenshots of your real desktop. Put machine-specific notes in `PRIVATE_NOTES.md` and screenshots in `artifacts/`; both are git-ignored. README screenshots come from the fake-data preview (`tools/testing/ui-preview/shot.mjs`).

## Building and testing

See the README's "Building from source". Before a pull request:

```powershell
cd src-tauri; cargo test
cd ..\tools\testing\ui-preview; node regress.mjs   # every line should say PASS
```

## Safety

- No input hooks, no injection into games (anti-cheat). Use `RegisterHotKey`, cursor polling or WinEvent hooks.
- Hardware writes (fans, clocks, power limits) need a way back: Deck returns fans to the BIOS on quit and starts the GPU at stock after a crash. Keep it that way.
- It must work on any PC: no hard-coded names, paths or device ids.
- Windows changes (Tweaks) keep their exact before-value so they can be undone, and only touch what the tweak's recipe lists. Test registry changes on the scratch key (`tweaks::steps::use_scratch`, under `HKCU\Software\Deck-test`), never on the real registry.
- Every program Deck runs gets a time limit (`util::run_for`), and downloads end on a stalled connection, not after a fixed time (slow lines).
