# Third-party notices

Deck's own code is under the [MIT licence](LICENSE). It ships or uses the following, each under its own licence.

| Component | Where | Licence | Source |
|---|---|---|---|
| PawnIO driver (`PawnIO.sys`, `.inf`, `.cat`), Microsoft-signed | `src-tauri/src/hw/mods/`, installed as a separate kernel service | GPL-2.0 | https://github.com/namazso/PawnIO |
| PawnIO modules (`LpcIO`, `IntelMSR`, `SmbusI801`, `AMDFamily17`) | `src-tauri/src/hw/mods/` | LGPL-2.1 | https://github.com/namazso/PawnIO.Modules |
| Fluent UI System Icons (profile icons) | `tools/icons/svg/`, `src-tauri/icons/profiles/` | MIT | https://github.com/microsoft/fluentui-system-icons |
| SDL 3.4.16 (`SDL3.dll`, official x64 build, gzipped) | `src-tauri/vendor/sdl3/`, embedded in `Deck.exe` and written next to it on first use | zlib | https://github.com/libsdl-org/SDL |
| resvg / usvg / tiny-skia (the controller overlay's art) | linked into `Deck.exe` | MIT / Apache-2.0 | https://github.com/linebender/resvg |
| Win11Debloat by Raphire: tweak recipes (registry values, Windows build ranges), the built-in app list and its ratings, the empty Start layout (`start2.bin`) | `src-tauri/tweaks/` (made by `tools/tweaks/build.py` at a pinned commit) | MIT | https://github.com/Raphire/Win11Debloat |
| WinUtil by Chris Titus Tech: tweak recipes (registry values with their original values) | `src-tauri/tweaks/tweaks.json` (made by `tools/tweaks/build.py` at a pinned commit) | MIT | https://github.com/ChrisTitusTech/winutil |
| pull-microsoft-store by sht2017: the request flow for Microsoft's update service (FE3) used by Get Store apps | `src-tauri/src/tweaks/storedl.rs` (Deck's own Rust, same protocol) | MIT | https://github.com/sht2017/pull-microsoft-store |
| Microsoft Root Certificate Authority 2011 and Microsoft ECC Product Root CA 2018 (public root certificates, the same Windows trusts) | `src-tauri/tweaks/certs/`, used only for Microsoft's update service | Public certificates | https://www.microsoft.com/pkiops/docs/repository.htm |
| Tauri and the Rust crates in `src-tauri/Cargo.lock` | linked into `Deck.exe` | MIT / Apache-2.0 and others (see each crate) | https://crates.io |

The PawnIO driver and modules are unmodified copies of the upstream signed binaries; their source code is available at the links above. Deck talks to the driver only through its documented IOCTL interface.

Nothing from WinScript (GPL-3.0) is included; it was used as a list of ideas only. VB-CABLE is never bundled: Deck downloads VB-Audio's own package unchanged and opens its setup.

RivaTuner Statistics Server, MSI Afterburner and NVIDIA's drivers are not included; Deck only talks to them when they are installed.
