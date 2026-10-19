# Third-party notices

Deck's own code is under the [MIT licence](LICENSE). It ships or uses the following, each under its own licence.

| Component | Where | Licence | Source |
|---|---|---|---|
| PawnIO driver (`PawnIO.sys`, `.inf`, `.cat`), Microsoft-signed | `src-tauri/src/hw/mods/`, installed as a separate kernel service | GPL-2.0 | https://github.com/namazso/PawnIO |
| PawnIO modules (`LpcIO`, `IntelMSR`, `SmbusI801`, `AMDFamily17`) | `src-tauri/src/hw/mods/` | LGPL-2.1 | https://github.com/namazso/PawnIO.Modules |
| Fluent UI System Icons (profile icons) | `tools/icons/svg/`, `src-tauri/icons/profiles/` | MIT | https://github.com/microsoft/fluentui-system-icons |
| SDL 3.4.16 (`SDL3.dll`, official x64 build, gzipped) | `src-tauri/vendor/sdl3/`, embedded in `Deck.exe` and written next to it on first use | zlib | https://github.com/libsdl-org/SDL |
| resvg / usvg / tiny-skia (the controller overlay's art) | linked into `Deck.exe` | MIT / Apache-2.0 | https://github.com/linebender/resvg |
| Tauri and the Rust crates in `src-tauri/Cargo.lock` | linked into `Deck.exe` | MIT / Apache-2.0 and others (see each crate) | https://crates.io |

The PawnIO driver and modules are unmodified copies of the upstream signed binaries; their source code is available at the links above. Deck talks to the driver only through its documented IOCTL interface.

RivaTuner Statistics Server, MSI Afterburner and NVIDIA's drivers are not included; Deck only talks to them when they are installed.
