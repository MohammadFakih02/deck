# Trust boundaries

What Deck trusts, what it doesn't, and where each check lives. Read this before changing anything that
runs elevated, listens on the network, or acts on a file in the user's folders. Reporting a problem:
see [`SECURITY.md`](../SECURITY.md).

## Why this matters

Deck normally runs **as administrator** (the `\Deck\Deck` and `\Deck\Open` scheduled tasks start it
with the highest privileges, with no UAC prompt) so it can drive fans, power limits and the GPU. Many
of its inputs, though, come from places the ordinary signed-in account controls: files under
`%APPDATA%`, environment variables, the user's folders, and other devices on the home network.

The rule behind every check below:

> **The elevated Deck never does what the signed-in user couldn't do themselves**, and nothing that
> reaches it over the network gets more than the person at the PC gave that caller.

## Processes

| Process | Runs as | Started by | Purpose |
|---|---|---|---|
| `Deck.exe` (main) | Administrator (via the tasks), or the user before **Set up** | logon task, Start menu (`\Deck\Open`), the user | UI, every feature, hardware engines |
| `Deck.exe --watchdog <pid>` | same as the main process | the main process when hardware control starts | gives fans back to the BIOS and the GPU fan back to the driver if the main process is ended from outside |
| `Deck.exe --run-installer <job>` | **the signed-in user** (started through Explorer) | the main process, for an installer received from another PC | so Windows shows its own UAC prompt with the publisher; the installer never inherits Deck's admin token |
| `claude-sync-hook.exe` | the user (run by Claude Code) | Claude Code hooks | a copy of `Deck.exe` in `%LOCALAPPDATA%\Deck`, so updates aren't blocked |
| Programs Deck launches (apps, games, files) | **the signed-in user** | `util::launch_as_user` (Explorer automation) | never elevated; if Explorer can't take the launch within about 3 s, Deck shows an error instead of starting it as admin |

## The install location

- **Setup:** the release setup installs **per machine** to `C:\Program Files\Deck`, a folder only
  administrators can change. It asks for admin (UAC) once, and removes an older per-user copy from
  `%LOCALAPPDATA%\Deck` (settings are kept).
- **Set up** (`elevation::setup`) refuses to create admin tasks for a `Deck.exe` the signed-in user
  could change (`fsguard::user_could_change`). It checks the exe and every folder above it for write,
  delete and permission rights.
- **At every admin start**, `elevation::ensure_quit_task` checks the same thing and removes Deck's
  tasks if they point at a user-changeable copy. The notice "Deck's admin start was turned off" asks
  the user to run Set up again from the installed copy.
- The scheduled-task XML is written to a random file in `C:\Windows\Temp` and held open read-only
  while `schtasks` reads it, so it can't be swapped in between (`elevation::task_file`).

## Inputs the elevated process must not trust

| Input | Risk | What Deck does | Where |
|---|---|---|---|
| Files and folders in user-writable places (phone uploads, the PC to PC inbox, Claude Sync writes, share zips, Clean up, Recycle, settings saves) | a folder swapped for a junction or link between the check and the use makes the admin process write or delete inside Windows | Every action is checked against the user's **limited token** (`AccessCheck` on the linked token of the elevated session). Work is done relative to an open folder handle (`NtCreateFile` with a root directory, `FILE_OPEN_REPARSE_POINT`, folders held without `FILE_SHARE_DELETE`). Renames go by handle. Links out of the intended root are refused. Network paths (`\\server\…`) are exempt from the token check, since the server enforces its own rights. | `fsguard.rs`, used by `util::write_atomic`, `remote.rs`, `beam/net.rs`, `ccsync/engine.rs`, `share.rs`, `cleanup.rs`, `disk.rs` |
| Sharing, sending or serving a file | the admin process could hand out a file the user can't read | refused unless the signed-in user could open it (`open_read_for_user`, `user_may_read_tree`); a folder containing links isn't zipped | `share.rs`, `remote.rs`, `beam/net.rs` |
| Hardware settings (`settings.json`'s CPU/GPU profiles, startup profile and boost choices, `hardware\settings.ini`, `hardware\curves.ini`) | any program of the user's could edit fan curves or an overclock that the admin process then applies | Each time Deck saves them it records a SHA-256 fingerprint under `HKLM\SOFTWARE\Deck\Seals`, where only administrators can write. At an admin start, a file whose fingerprint doesn't match waits: fans stay on the BIOS and no overclock is applied until the user clicks **Apply them** on the "Hardware settings were changed outside Deck" notice (PC only, never the phone). | `seal.rs`, `app.rs` (`HW_HELD`) |
| Environment variables | `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS` could open a DevTools port into the admin page; `%ProgramFiles%` and similar could point Deck's own folders elsewhere | An elevated Deck removes every `WEBVIEW2_*` variable at the top of `main()`; the debug port comes only from `--cdp-port` on Deck's command line. System folders come from Windows (known-folder APIs), not the environment. | `webview_guard.rs`, `util.rs` |
| DLL search order | a vendor library found in a user-writable folder would load into the admin process | `SetDllDirectoryW("")` removes the current folder from the search. Vendor libraries (`nvml.dll`, `nvapi64.dll`, `atiadlxx.dll`, `powrprof.dll`) load only from System32, or NVIDIA's NVSMI folder for `nvml.dll`, by full path. | `main.rs`, `util::trusted_dll_paths`, `util::load_trusted_dll` |
| Secrets on command lines | other processes and audit logs can read command lines | The RustDesk password reaches PowerShell through the environment or a short-lived file, never PowerShell's command line. RustDesk itself only accepts it as an argument, so it is briefly visible on RustDesk's own command line. PowerShell strings escape every kind of single quote, including typographic ones. | `rdesk.rs` |

## Network callers

Each caller gets its own fixed allow-list. Nothing a network caller sends is treated as a path, a
command or a program to run.

| Caller | Port(s) | Authentication | What it may do | Where |
|---|---|---|---|---|
| **Phone remote** | TCP 47830 (LAN, Tailscale) | The phone key never travels. The page opens a session, then signs each request with HMAC-SHA256(key, session \| counter \| path). Each counter works once, so a recorded request can't be replayed. A request must name this PC (Host check). The page carries a strict CSP that allows only its own hashed scripts. | the explicit list in `remote::handle`: media, volume, profiles, timers, power plans, shared files, its own notifications. It may not accept transfers, apply a held overclock, open files on the PC, or answer the hardware-changed notice (`app::toast_allowed`, `Caller::Phone`). | `remote.rs`, `phoneauth.rs`, `app.rs` |
| **Phone screen view / control** | same | same | Screen view is off by default (`remote_screen`). In `control` mode, taps and typing are sent with `SendInput` and paused while a game is in front. **See S13 below.** | `screencast.rs` |
| **PC to PC** (Deck's AppBeam engine) | UDP 47833 (beacon), TCP 47834 | The pairing code is stretched with a slow, memory-hard derivation (`kdf.rs`, about 0.25 s, 32 MiB). Each PC also has its own Ed25519 key; its id is derived from that key and every connection is signed with it. "Trusted" and "always allow" name a key-derived id, so another device that knows the pairing code can't take it over. | Offers wait for Accept unless the sender is trusted. **Installers always wait for a click on this PC** and start as the signed-in user. Requests are served only from folders the user shared, and only files the user could read. Names from the other PC are cleaned (`..`, drive letters, alternate data streams, reserved names). Frames over the size cap are refused before they are read. | `beam/net.rs`, `beam/device.rs`, `beam/setup.rs`, `kdf.rs` |
| **Claude Code sync** | UDP 47835, TCP 47836, plus an optional cloud folder | sync key, stretched with the same `kdf.rs` | Syncs sessions, memory and Claude Code settings. **A change that could make Claude Code run something or do more without asking** (hooks, plugins, status line, helper commands, permissions, environment) is held until the user clicks Apply or "Keep this PC's" on this PC. Files are written with the user's rights only. | `ccsync/`, `ccsync/guard.rs` |
| **Share links** | TCP 47837; over the internet only through Tailscale Funnel | a random link token | Home links are never served to Funnel requests. Funnel publishes only `/deck-share/` and only while an internet link is open; Deck turns it off at start when none is. A one-time link reserves its download when the download starts, so two downloads can't both use it. | `share.rs` |
| **Local UI** | none (embedded pages, `icon://`) | the WebView loads only Deck's own embedded assets (`script-src 'self'`) | every Tauri command | `main.rs`, `*cmd.rs` |

Servers in the `test` profile (`DECK_PROFILE=test`) bind to 127.0.0.1 only.

## Hardware safety

These are not security boundaries, but they protect the PC when something goes wrong.

- **Fans go back to the BIOS**, and the GPU fan back to the driver:
  - when Deck quits, panics or hits a native exception, and at log-off;
  - with `--release-fans` and in the uninstaller;
  - through the **watchdog** process when Deck is ended from outside (Task Manager, `taskkill /f`).
  The watchdog waits on Deck's process and on a named event (`Local\Deck-watchdog-<pid>`) that Deck sets
  when it exits cleanly.
- **Crash guard** (`gpu/guard.rs`): after an unexpected restart with an overclock on, the next boot
  starts at stock and asks. A damaged guard file holds the overclock too.
- **Changed outside Deck** (`seal.rs`): see the table above.

## Known residual risks (accepted, documented)

- **S13: phone screen control reaches admin windows.** Deck is elevated, so input it sends with
  `SendInput` isn't limited by UIPI: a phone in Control mode can click and type into admin windows,
  including UAC-elevated apps on the same desktop. Mitigations:
  - Screen view is off by default, and control is a separate switch.
  - The phone needs the phone key, and every request is signed.
  - Control pauses while a game is in front.

  Treat the phone key as an admin credential while Control is on.
- **The phone link at home is plain HTTP.** The key itself is never sent, and requests can't be
  replayed. But anyone on the same Wi-Fi can read what the page shows (media titles, file names, the
  screen view). Use Tailscale (encrypted) on networks you don't trust.
- **Custom pairing codes and sync keys** are only as strong as what the user types. The slow
  derivation makes each guess expensive. Use the generated codes.
- **Network paths** (`\\server\share`) are exempt from the limited-token check: the file server
  enforces its own permissions.
- **RustDesk's password** is briefly visible on RustDesk's own command line, since it takes no
  other way in.

## Changing something here

- **A new network-reachable action** goes into the caller's allow-list (`remote::handle`, the beam
  request handlers) with a test that a forbidden caller is refused.
- **A new file operation in a user-writable folder from the elevated process** goes through
  `fsguard` (`open_for_user`, `write_for_user`, `move_for_user`, `Dir`), never `std::fs` by path.
- **A new setting that changes hardware behaviour** goes into `seal::hardware_part` (or `seal::parts`).
- **A new vendor DLL** is loaded with `util::load_trusted_dll`.
