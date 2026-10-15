---
name: deck-pc-to-pc
description: Send files, folders or whole projects (dependencies included) to the user's other PCs, or fetch from them, with Deck's PC to PC. Use when the user asks to send, copy, move or sync something to or from another of their PCs (their laptop or another desktop).
---

# Deck PC to PC

Deck (installed on this PC) sends files between the user's own paired PCs over the home network or Tailscale, encrypted. Run its command line; it prints progress and ends with one result line.

PowerShell: `& "$env:LOCALAPPDATA\Deck\deck.exe" <args>`
Bash: `"$LOCALAPPDATA/Deck/deck.exe" <args>`

| What | Arguments |
|---|---|
| Which PCs are reachable | `--pc-list` |
| Send files or a folder | `--pc-send <path> [--pc-send <path> …] [--to NAME]` |
| Send a project with its dependencies (node_modules, .venv, build output…) | add `--all` |
| Only what is new or changed since the last copy | add `--sync` |
| Also delete files on the other PC that are gone here | add `--delete-extra` (the other PC's user is always asked) |
| Leave more out | `--exclude "*.log,tmp"` |
| Run an installer on arrival | add `--install` |
| Fetch from the other PC's shared folders | `--pc-get "SharedFolder/sub/path" --from NAME` |
| Machine-readable result | add `--json` |
| Look longer for the other PC | `--wait 60` (default 15 s) |

`--to` is needed only when more than one PC is paired.

Exit codes: 0 done, 1 failed or declined, 2 the PC wasn't found (Deck must be open on it, PC to PC on, same pairing code), 3 PC to PC is off here or a path is wrong.

Things to know:
- The other PC's user is asked to accept unless they trust this PC, so a send can wait up to 2.5 minutes for them. Tell the user to watch for the prompt on the other PC. If nobody answers, the result says so.
- Without `--all`, a folder leaves out build output (target, node_modules, .git, dist and the like). Use `--all` when the other PC needs to run the project without reinstalling dependencies, and say so to the user, since it can be large.
- Received files land in the other PC's PC to PC folder (Downloads\From my PCs by default). A folder sent again merges into the earlier copy (newer files win).
- Never send secrets (.env files, keys, credentials) unless the user asked for that file explicitly.
