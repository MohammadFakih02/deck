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

The rule is also written in `DECK.md` §0 and §9. Don't bypass the hook with `--no-verify`.

## Keep it public-safe

The repo is public. Don't commit local paths, PC names, hardware specs, IP addresses, game lists or screenshots of your real desktop. Put machine-specific notes in `PRIVATE_NOTES.md` and screenshots in `artifacts/`; both are git-ignored. README screenshots come from the fake-data preview (`tools/testing/ui-preview/shot.mjs`).

## Building and testing

See the README's "Building from source". Before a pull request:

```powershell
cd src-tauri; cargo test
cd ..\tools\testing\ui-preview; node regress.mjs   # every line should say PASS
```

## Logging

Deck's log (`src-tauri/src/log.rs`) is how a problem on someone else's PC gets fixed, so log at the place where the failure is understood, once:

- `log::warn` when something failed and Deck carried on; `log::error` only for crashes and things left broken (it writes at once); `log::info` for changes Deck made to the PC and the outcome of operations; `log::debug` / `debug_with` for detail (off by default).
- An operation with an outcome: `let op = log::Op::start("area", "what")`, then `op.ok(..)`, `op.fail(e)` or `op.skipped(why)`. One that is dropped without an outcome is logged as such.
- Web requests: build agents with `log::http(ureq::AgentBuilder::new())`, or use `log::http_agent()` for a one-off request. Programs: `util::run_for` / `run_hidden` already log failures.
- Something that fails every second: wrap the line in `log::every(key, interval)`. Identical lines within a minute are counted automatically.
- Never log clipboard text, file contents, what is on screen, request bodies, tokens or keys, device ids (MAC addresses, device paths) or full command lines. Name the thing (a program, a setting key, a share link by when it was made) instead. The log masks common secret patterns, the user's name and the PC's name as a last line of defence, not as permission.
- Never let logging sit in front of hardware hand-back: release first, log after (it only queues a line).

## Safety

- No input hooks, no injection into games (anti-cheat). Use `RegisterHotKey`, cursor polling or WinEvent hooks.
- Hardware writes (fans, clocks, power limits) need a way back: Deck returns fans to the BIOS on quit and starts the GPU at stock after a crash. Keep it that way.
- It must work on any PC: no hard-coded names, paths or device ids.
- Windows changes (Tweaks) keep their exact before-value so they can be undone, and only touch what the tweak's recipe lists. Test registry changes on the scratch key (`tweaks::steps::use_scratch`, under `HKCU\Software\Deck-test`), never on the real registry.
- Every program Deck runs gets a time limit (`util::run_for`), and downloads end on a stalled connection, not after a fixed time (slow lines).
