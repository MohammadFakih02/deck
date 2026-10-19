# Thick-client security audit — 2026-10-18

> Contains working escalation paths that are **not fixed** in 2.5.6. Keep this file out of the public repo
> until the fixes ship (same rule as the 2026-10-17 audit files).

**Scope:** `main` at `b6168a5` (Deck 2.5.6): `src-tauri/` (Rust), `ui/` (WebView front end), build and CI
files, and the installed `Deck.exe` and `rtss-osd32.exe` (checked with `dumpbin`).
**Threat model:** Deck runs **elevated** (it starts through a highest-privilege scheduled task at logon). The
main attacker is a program running as the same user **without** admin rights, plus anyone on the LAN.
"User-to-admin" therefore means a real privilege boundary, even on a single-user PC: UAC exists to stop
exactly this.
**References:**
- OWASP Desktop App Security Top 10 (2021): DA1 Injections, DA2 Broken Authentication & Session
  Management, DA3 Sensitive Data Exposure, DA4 Improper Cryptography Usage, DA5 Improper Authorization,
  DA6 Security Misconfiguration, DA7 Insecure Communication.
- OWASP TCASVS 5.0: V1 Architecture & Threat Modeling, V2 Build, Deployment & Environment Hardening,
  V3 Data Storage & Protection, V4 Code Quality & Exploit Mitigation, V5 Cryptography, V6 Network
  Communication. The requested layer mapping (static = V1/V2, runtime = V3/V4, network = V5) doesn't match
  the standard's numbering; the real chapters are cited below. Findings are cited **at chapter level**:
  individual requirement IDs were not checked against the text.

This is not a statement that Deck is secure. It covers the areas below, at the depth stated.

**Status (2026-10-18):** F1–F8 fixed in `a7ea08e` (pushed, not released), each with a test (219 pass, clippy clean).
F8 was fixed differently than proposed below: the beacons already carry a code fingerprint, so instead of reordering the
handshake, easy typed codes are refused (`kdf::too_easy`) and connections are capped (`util::Slot`). Not yet exercised as the
installed app.

## Summary

| # | Finding | Severity | Layer |
|---|---|---|---|
| F1 | Elevated Deck runs `winget.exe` from a user-writable folder | **High** | Runtime / OS |
| F2 | Elevated PowerShell loads modules (and RustDesk's exe) from user-controlled locations | **High** | Runtime / OS |
| F3 | RustDesk installer is checked, then run, from the user's Temp folder; any valid signer passes | Medium | Runtime / Network |
| F4 | `DECK_PROFILE` from the user's environment redirects settings and the HKLM seal (seal bypass) | Medium | Runtime / Storage |
| F5 | 32-bit helper can be verified then run from `%LOCALAPPDATA%` | Low | Runtime / OS |
| F6 | `Deck.exe` built without Control Flow Guard or CET shadow-stack compatibility | Low | Binary hardening |
| F7 | Device private key, phone token, sync key and pairing code stored in plaintext | Low | Storage |
| F8 | PC to PC handshake gives an offline-guessing oracle for user-chosen codes; no connection cap | Low | Network |
| I1 | Phone remote and LAN share links use plain HTTP on the home network | Info (accepted design) | Network |
| I2 | Any local process can pass `--switch` / `--quit` to the running elevated Deck | Info (intended) | IPC |

F1, F2, F3 and F5 share one root cause with the earlier S1 to S4 findings (RC-ELEV): **the elevated process
trusts a location or a setting the normal user controls.** A lasting fix is a rule enforced in code: an
elevated Deck starts a process only from a full path that `fsguard::user_could_change` reports as
admin-only, and runs helper scripts with a fixed environment.

---

## 1. Static analysis and reverse engineering

### 1.1 Hardcoded secrets: ✅ none found
- Searched `src-tauri/src`, `ui/`, `tools/` and `.github/` for key, secret, password, token and bearer literals,
  and for private-key material. No embedded credentials or API keys.
- The outbound URLs are public endpoints: `open.er-api.com`, `ipwho.is`, `api.ipify.org`,
  `speed.cloudflare.com`, `api.steampowered.com`, `api.github.com`, `pkgs.tailscale.com`. None carries a key.
- `main.rs:1238` prints the phone token, but only in `selftest_remote`, which exists only under
  `#[cfg(debug_assertions)]` and is not in release builds.
- Secrets are generated at runtime from the OS RNG:
  - `util.rs:342` and `beam/crypto.rs:9` use `getrandom`;
  - `beam/device.rs:42` uses `ring::rand::SystemRandom`;
  - generated pairing codes are 20 characters from a 31-character alphabet, about 99 bits (`beam/crypto.rs:18`).

### 1.2 Insecure cryptography: ✅ no weak primitives (one protocol note, see F8)
- **Primitives:**
  - ChaCha20-Poly1305 (AEAD) for the PC to PC and Claude Sync channels;
  - Ed25519 (`ring`) for device identity;
  - HMAC-SHA256 for phone requests;
  - BLAKE3 for hashing;
  - an scrypt-ROMix-style stretch for codes (`kdf.rs`, 32 MiB).
- **Not found:** MD5, SHA-1, DES, RC4 or ECB.
- **Nonces:** counters per direction, under keys derived per connection from both sides' random nonces
  (`beam/net.rs:164-179`), so they are never reused under one key.
- **MAC comparison** is constant-time (`ct_eq`).

### 1.3 Code injection: ✅ mostly safe; the exceptions are F1 and F2
- **No `eval`, `new Function` or `document.write`** in the UI. The only `innerHTML` uses are:
  - fixed icon markup (`core.js:49`, `toast.js:73`);
  - backend-made QR SVGs (`pages3.js:97,104,237`);
  - Deck's own graph SVG (`overlay.js:166`).
- **Outside text stays inert:** text from peers, the clipboard or the phone goes in through `textContent`
  (e.g. `toast.js:77-78`).
- **CSP** (`tauri.conf.json`): `script-src 'self'`, with no `unsafe-eval` and no inline scripts. Even an HTML
  injection could not run script.
- **PowerShell:** scripts are passed `-EncodedCommand`, and `ps_str` escapes every single-quote form
  (`rdesk.rs:103-115`). Secrets go through the environment or a one-time file, never the command line.
- **Process launches:** all use argument vectors (`Command::args`), with no shell string concatenation.
  Rust's `Command` searches the exe folder and System32 before `PATH`, which protects every System32 tool
  Deck runs (`schtasks`, `netsh`, `reg`, `sc`, `taskkill`, `tar`, `cmd`, `powershell`), but **not `winget.exe`
  (F1).**
- **Deserialization:** `serde_json` into typed structs, with no polymorphic or type-tagged deserialization.
  Network JSON is only read after authentication (see 3.2).

---

## 2. Local runtime and Windows OS integration

### F1 — Elevated Deck runs `winget.exe` from a user-writable folder · **High**
- **Where:**
  - `src-tauri/src/beam/apps.rs:41` (`Command::new("winget.exe")`);
  - called from `beam/mod.rs:433` (apps list), `beam/mod.rs:481` and `beam/net.rs:1279` (installing a
    received app list), and from `winget_available()`.
- **Reference:** OWASP Desktop Top 10 **DA5** (least privilege) and **DA1** (binary planting);
  TCASVS **V2** (environment hardening) and **V1** (trust boundaries).
- **Evidence:** `winget.exe` is not in System32. Rust's search therefore falls through to `PATH` and finds
  `%LOCALAPPDATA%\Microsoft\WindowsApps\winget.exe`, an app-execution alias in a folder where the user has
  **Full Control** (checked with `Get-Acl` on this PC).
- **Exploit scenario:**
  1. A program running as the user, with no admin rights, replaces that alias with its own `winget.exe`
     (or adds an earlier folder to the user `Path` in `HKCU\Environment`).
  2. The next time the user opens PC to PC → Apps, or a trusted PC sends an app list, the elevated Deck
     runs it.
  3. The attacker gets silent admin code execution with no UAC prompt.
- **Fix:**
  - resolve winget from App Installer's own package folder (`C:\Program Files\WindowsApps\…`, owned by
    TrustedInstaller);
  - refuse any path the user could change;
  - better still, run winget de-elevated: it doesn't need admin to list apps, and its installers raise
    their own UAC prompt.

```rust
// beam/apps.rs
use windows::core::{h, HSTRING};
use windows::Management::Deployment::PackageManager; // Cargo feature "Management_Deployment"

/// winget from App Installer's package folder, never through PATH (the WindowsApps alias in
/// %LOCALAPPDATA% is user-writable).
fn winget_exe() -> Option<std::path::PathBuf> {
    let pm = PackageManager::new().ok()?;
    let pkgs = pm.FindPackagesByUserSecurityIdPackageFamilyName(&HSTRING::new(), h!("Microsoft.DesktopAppInstaller_8wekyb3d8bbwe")).ok()?;
    let dir = pkgs.into_iter().next()?.InstalledLocation().ok()?.Path().ok()?;
    let exe = std::path::PathBuf::from(dir.to_string_lossy()).join("winget.exe");
    // the elevated process only starts what the user can't change
    (exe.is_file() && !crate::fsguard::user_could_change(&exe).unwrap_or(true)).then_some(exe)
}

fn winget(args: &[&str]) -> Option<(i32, String)> {
    let o = Command::new(winget_exe()?).args(args).creation_flags(NO_WINDOW).output().ok()?;
    Some((o.status.code().unwrap_or(-1), String::from_utf8_lossy(&o.stdout).to_string()))
}

#[test]
fn winget_never_comes_from_a_user_folder() {
    if let Some(p) = winget_exe() {
        assert!(!crate::fsguard::user_could_change(&p).unwrap());
        assert!(!p.starts_with(crate::util::env_path("LOCALAPPDATA").unwrap()));
    }
}
```

### F2 — Elevated PowerShell trusts user-controlled module paths and `$env:ProgramFiles` · **High**
- **Where:**
  - `src-tauri/src/rdesk.rs:43-67` (`admin_ps_secret`) and `rdesk.rs:86` (the script prologue). Every
    RustDesk action uses them: install, on/off, password, direct access, tray hiding.
  - `rdesk.rs:240` uses `$env:ProgramFiles`.
- **Reference:** OWASP Desktop Top 10 **DA6** (misconfiguration: inherited environment) and **DA5**;
  TCASVS **V2**.
- **Evidence (reproduced here, harmlessly, without admin):**
  - A module placed in a folder earlier on `PSModulePath` that exports `Get-AuthenticodeSignature` replaced
    the built-in cmdlet: `(Get-Command Get-AuthenticodeSignature).Source` returned the planted module.
  - Windows PowerShell puts the user's `Documents\WindowsPowerShell\Modules` first on that path by default,
    and the elevated Deck passes the user's environment to its child. `-NoProfile` doesn't stop module
    auto-loading.
  - The same applies to every cmdlet the scripts use (`Start-Process`, `Set-Service`, `Get-FileHash`…).
  - The user can also override `ProgramFiles` in `HKCU\Environment`. The script then runs that
    folder's `RustDesk\rustdesk.exe` elevated.
- **Exploit scenario:**
  1. Malware running as the user writes `Documents\WindowsPowerShell\Modules\X\X.psm1`, defining
     `Start-Process` or `Get-AuthenticodeSignature`.
  2. The next time the user touches any Remote desktop setting, that code runs as admin.
  3. If the planted cmdlet is `Get-AuthenticodeSignature`, the installer's signature check is also forged.
- **Fix:**
  - pin `PSModulePath` to System32's module folder, both in the child environment and on the script's
    first line (the UAC path starts a fresh environment);
  - start PowerShell by its full System32 path;
  - pass paths from Rust's known-folder helpers, never `$env:`;
  - long term, replace these scripts with direct API calls (service control manager, `WinVerifyTrust`,
    `CreateProcess`).

```rust
// rdesk.rs
fn ps_home() -> std::path::PathBuf {
    crate::util::windows_dir().join(r"System32\WindowsPowerShell\v1.0") // known-folder API, not %SystemRoot%
}

// in ps_args: the script's first line resets the module path, so a UAC-started copy is covered too
let modules = ps_str(&ps_home().join("Modules").to_string_lossy());
let script = format!("$env:PSModulePath = {modules}\n$ErrorActionPreference = 'Stop'\ntry {{\n{take}{script}\n}} catch {{ $_.Exception.Message; exit 1 }}");

// in admin_ps_secret
let mut cmd = std::process::Command::new(ps_home().join("powershell.exe"));
cmd.env("PSModulePath", ps_home().join("Modules")).args(&args).creation_flags(crate::util::CREATE_NO_WINDOW);

// install(): RustDesk's path from Rust, not $env:ProgramFiles
let rd = ps_str(&crate::util::program_files().join(r"RustDesk\rustdesk.exe").to_string_lossy());
// … "& {rd} --option direct-server Y | Out-Null"

#[test]
fn a_planted_module_is_not_loaded() {
    // a folder with a module that defines Get-AuthenticodeSignature, put first on PSModulePath
    // in this process; the script must still get the built-in cmdlet
    let out = admin_ps("(Get-Command Get-AuthenticodeSignature).Source").unwrap();
    assert_eq!(out, "Microsoft.PowerShell.Security");
}
```

### F3 — RustDesk installer: check-then-use in the user's Temp, any valid signer accepted · **Medium**
- **Where:** `src-tauri/src/rdesk.rs:216-244`, specifically:
  - `rdesk.rs:227` (a fixed folder `%TEMP%\Deck-rustdesk`);
  - `rdesk.rs:235` (the signature check);
  - `rdesk.rs:236` (`msiexec /i` on the same path).
- **Reference:** OWASP Desktop Top 10 **DA5** and **DA4** ("poor integrity check design"); TCASVS **V2**
  and **V5**.
- **Evidence:**
  - The elevated Deck writes the MSI to a folder the user owns. The admin PowerShell then hashes it, checks
    its signature and starts `msiexec` on the path, which is three separate opens.
  - The signature check accepts **any** valid Authenticode signature, not RustDesk's publisher.
  - The hash check is skipped when GitHub returns no `digest` (`unwrap_or("")`).
  - A fixed folder name lets the user pre-create it as a junction.
- **Exploit scenario:** a user-level program watches `%TEMP%\Deck-rustdesk`. After the checks pass, it swaps
  in its own MSI (signed with any cheap code-signing certificate, or not at all if F2 is used too).
  `msiexec` then installs it as admin.
- **Fix:**
  - download into an admin-only folder with a random name: `elevation.rs:84` already does this for task XML
    under `Windows\Temp`;
  - keep the file open without write or delete sharing until `msiexec` has finished;
  - require the digest;
  - check the signer, not just validity.

```rust
// rdesk.rs install()
if sha.is_empty() { return Err("GitHub gave no checksum for RustDesk's setup".into()); }
let dir = crate::util::windows_dir().join("Temp").join(format!("deck-rd-{}", crate::util::random_token(16)));
std::fs::create_dir(&dir).map_err(|e| e.to_string())?; // Windows\Temp: users can't open others' new folders
let file = dir.join("rustdesk.msi");                    // our name, never the server's
std::fs::write(&file, &body).map_err(|e| e.to_string())?;
// hold it read-only (no write/delete sharing) while it is checked and installed
let _hold = std::fs::OpenOptions::new().read(true).share_mode(FILE_SHARE_READ).open(&file).map_err(|e| e.to_string())?;
// in the script: pin the signer as well as the status
// $s = Get-AuthenticodeSignature $f
// if ($s.Status -ne 'Valid' -or $s.SignerCertificate.Subject -notmatch '<RustDesk publisher O= value>') { exit 3 }
```
Read the publisher's `O=` value from a real RustDesk MSI before pinning it.

### F4 — `DECK_PROFILE` from the user environment bypasses the hardware seal · **Medium**
- **Where:**
  - `src-tauri/src/settings.rs:434` (the settings folder becomes `%APPDATA%\Deck-<profile>`);
  - `src-tauri/src/seal.rs:36` (the seal key becomes `HKLM\SOFTWARE\Deck-<profile>\Seals`);
  - `seal.rs:27-33` (`None` → `Seal::First` → trusted and recorded);
  - `hw/kv.rs:10` (the hardware `.ini` files follow the settings folder).
- **Reference:** OWASP Desktop Top 10 **DA5** and **DA6**; TCASVS **V3** (integrity of stored data) and **V1**.
- **Evidence:**
  - `DECK_PROFILE` is a test switch. The elevated Deck still honours it in release builds, and the user can
    set it in `HKCU\Environment` without admin rights. The logon task gives it to the elevated process.
  - A fresh profile name points at a seal key that doesn't exist, which counts as a first start, so
    whatever settings the attacker prepared are trusted.
- **Exploit scenario:**
  1. A user-level program writes `%APPDATA%\Deck-x\settings.json` and `hardware\curves.ini`, with fan
     curves pinned low and an aggressive GPU/CPU profile applied at start.
  2. It sets `DECK_PROFILE=x`.
  3. At the next logon the elevated Deck applies them without the "changed outside Deck" hold (the S8
     protection).
- **Impact:** hardware safety and stability, not code execution. Chip thermal protection still applies.
- **Fix:** honour `DECK_PROFILE` only in debug builds or when Deck isn't elevated, and have every reader go
  through one function.

```rust
// settings.rs
/// The test profile name. An elevated release build ignores it: the user can set it without admin
/// rights, and it would point Deck (and the hardware seal) at files of their choosing.
pub fn profile() -> Option<String> {
    if !cfg!(debug_assertions) && crate::elevation::is_admin() {
        return None;
    }
    std::env::var("DECK_PROFILE").ok().filter(|p| !p.is_empty())
}
// Settings::dir():  profile().map(|p| format!("Deck-{p}")).unwrap_or_else(|| "Deck".into())
// seal::key_path(): profile().map(|p| format!("-{p}")).unwrap_or_default()
// log.rs:83, remote.rs:123/151/159, share.rs:150/365: use profile() as well
```

### F5 — The 32-bit helper can be verified, then run, from a user folder · **Low**
- **Where:** `src-tauri/src/rtss.rs:666-680` (`rtss32_helper`, with the `%LOCALAPPDATA%\Deck` fallback at
  line 670).
- **Reference:** OWASP Desktop Top 10 **DA5**; TCASVS **V2**.
- **Evidence:** when writing next to `Deck.exe` fails (for example the old copy is in use), the elevated Deck
  falls back to `%LOCALAPPDATA%\Deck\rtss-osd32.exe`. It compares the file's bytes and then starts it by
  path, so it can be swapped between the read and the start.
- **Exploit scenario:** a race window that only opens when the Program Files write fails. That makes it
  unlikely, but it gives admin code execution when it hits.
- **Fix:** when elevated, use only the exe folder.

```rust
let mut dirs = vec![std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.to_path_buf()))];
if !crate::elevation::is_admin() {
    dirs.push(crate::util::env_path("LOCALAPPDATA").map(|p| p.join("Deck")));
}
```

### F6 — Binary hardening: no CFG or CET shadow-stack flag · **Low**
- **Where:** `src-tauri/Cargo.toml` (`[profile.release]`); there is no `.cargo/config.toml`.
- **Reference:** OWASP Desktop Top 10 **DA6**; TCASVS **V4** (exploit mitigation).
- **Evidence (`dumpbin`):**

| Binary | ASLR | High-entropy ASLR | DEP | GS cookie | SafeSEH | CFG | CETCOMPAT |
|---|---|---|---|---|---|---|---|
| `C:\Program Files\Deck\Deck.exe` (x64) | ✅ | ✅ | ✅ | ✅ | n/a on x64 | ❌ | ❌ |
| `rtss-osd32.exe` (x86) | ✅ | n/a | ✅ | ✅ | ✅ (29 handlers) | ❌ | ❌ |

- **Imports:** only Windows DLLs. `rtss-osd32.exe` imports `KERNEL32`, `USER32`, `ntdll` and an API set, and
  no VC runtime.
- **Risk:** Rust code is memory-safe outside `unsafe` blocks, so this matters most for the FFI code (`rtss.rs`,
  `hw/`, `screencast.rs`) and for C dependencies. CFG and CET make a memory bug harder to turn into code
  execution.
- **Fix:**

```toml
# src-tauri/.cargo/config.toml
[target.x86_64-pc-windows-msvc]
rustflags = ["-Ccontrol-flow-guard", "-Clink-arg=/CETCOMPAT"]
[target.i686-pc-windows-msvc]
rustflags = ["-Ccontrol-flow-guard", "-Ctarget-feature=+crt-static"]
```
- **Caveats:**
  - Rust's precompiled standard library isn't CFG-instrumented unless built with `-Zbuild-std`, so coverage
    is partial.
  - Test that WebView2 and the vendor GPU DLLs still load: `/CETCOMPAT` applies to the process.
- **Already in place:**
  - `SetDllDirectoryW("")` at the top of `main()` (`main.rs:1318`);
  - vendor DLLs loaded only by full path from System32 or NVSMI (`util::load_trusted_dll`);
  - per-machine install to Program Files;
  - task targets checked with `fsguard::user_could_change`.
- **Possible addition:** `SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_SYSTEM32 | LOAD_LIBRARY_SEARCH_APPLICATION_DIR)`
  would also cover loads made by third-party code.

### F7 — Secrets stored in plaintext in the user profile · **Low**
- **Where:**
  - `beam/device.rs:38-46` (`%APPDATA%\Deck\transfer\device.key`, an Ed25519 PKCS#8 key);
  - `settings.rs:208` (`remote_token`);
  - `ccsync/config.rs:57` (`sync_key`);
  - `beam/config.rs:20` (pairing `code`).
- **Reference:** OWASP Desktop Top 10 **DA3**; TCASVS **V3**.
- **Evidence:** written through `util::write_atomic` with the default ACL (the user and admins). They are not
  in logs: no secret reaches `log.rs` or `eprintln!`.
- **Risk:**
  - Malware running as the same user can read them whatever Deck does: DPAPI doesn't stop same-user code.
    So the real exposure is copies of the profile: backups, roaming profiles, cloud-synced AppData, or a
    disk read offline.
  - A stolen `device.key` lets another machine pass as this "trusted PC". Received installers still need a
    local click (S6), which limits the damage.
- **Fix:** wrap the values in DPAPI (CurrentUser), which ties them to this Windows account and this machine.

```rust
use windows_sys::Win32::Security::Cryptography::{CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN};
pub fn protect(plain: &[u8]) -> Option<Vec<u8>> {
    let mut inp = CRYPT_INTEGER_BLOB { cbData: plain.len() as u32, pbData: plain.as_ptr() as *mut u8 };
    let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
    let ok = unsafe { CryptProtectData(&mut inp, std::ptr::null(), std::ptr::null(), std::ptr::null(), std::ptr::null(), CRYPTPROTECT_UI_FORBIDDEN, &mut out) };
    if ok == 0 { return None; }
    let v = unsafe { std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec() };
    unsafe { windows_sys::Win32::Foundation::LocalFree(out.pbData as _) };
    Some(v)
}
// device.key: write protect(pkcs8); on load try CryptUnprotectData first, fall back to the old
// plaintext file once and rewrite it protected (migration)
```

### Local IPC: ✅ with notes
- **Tauri IPC:**
  - Only Deck's own bundled pages can call commands: windows `main`, `toast`, `zones` and `loupe`, with no
    remote URLs (`capabilities/default.json`).
  - The CSP limits `connect-src` to `ipc:`.
  - Toast actions are authorised per caller (S5, fixed).
- **I2 · Single-instance channel** (`main.rs:1489`, `handle_args` at `main.rs:1269`). Any local process can
  start `Deck.exe --switch cpu:<id>` or `--quit`, and the elevated instance acts on it. **Intended** (desktop
  shortcuts). It can only choose profiles the user already saved, which are sealed. Keep that list closed:
  never add an argument that takes a path or a command.
- **`icon://` protocol** (`main.rs:1495-1519`): reads only Deck's own clipboard images (no `/` or `\` in the
  name), app icons by id and file icons. It sends `Access-Control-Allow-Origin: *`, but only Deck's own
  origins load it.
- **Network listeners:** PC to PC, Claude Sync, the phone remote and Share bind `0.0.0.0`. All are
  authenticated (see 3.2). There are no named pipes or `WM_COPYDATA` handlers of Deck's own.

### Data leakage in logs: ✅
`%LOCALAPPDATA%\Deck\logs` (`log.rs`) records events and errors. No tokens, codes or keys are written there
(checked every `crate::log::` and `eprintln!` call).

---

## 3. Network and API layer

### 3.1 Transport security
- **✅ Outbound HTTPS:** `ureq` 2 with `tls` uses rustls and the webpki roots, so certificates are validated.
  No `danger`, custom `ServerCertVerifier` or `accept_invalid` exists anywhere. Every outbound URL in 1.1
  is `https://`.
- **✅ Dependency advisories:** `cargo audit` runs in CI. rustls was updated to 0.23.46 for
  RUSTSEC-2026-0346 to 0349.
- **I1 · Plain HTTP on the LAN** (Info, accepted design):
  - The phone remote (`remote.rs:69-91`) and LAN share links (`share.rs:321`) are `http://`.
  - Phone requests are signed (HMAC-SHA256 with single-use counters, S9), and the key never travels.
  - But the **content** can be read on the network: the Screen view's JPEG frames, clipboard text sent to
    the phone, and uploaded files.
  - Recommendation: say so on the Phone page, and steer the Screen view to Tailscale (encrypted), e.g. allow
    it only over Tailscale unless the user overrides.
  - The Funnel share path is HTTPS through Tailscale.

### F8 — PC to PC handshake: server proves first; no connection cap · **Low**
- **Where:**
  - `beam/net.rs:970-978`: the server sends `MAC(key,"auth-s",nc,ns)` before the client has proved anything;
  - the same pattern in `ccsync/lan.rs:202`;
  - unbounded threads in `beam/net.rs:1487-1493`.
- **Reference:** OWASP Desktop Top 10 **DA4** and **DA7**; TCASVS **V5** and **V6**.
- **Evidence:**
  - Any LAN host can send 20 bytes and receive a value it can test pairing-code guesses against offline.
    With a generated code (about 99 bits) that's infeasible.
  - With a **user-chosen** code, guessing costs one KDF run each (about 0.25 s, 32 MiB). A short dictionary
    code falls in hours to days on one GPU-less PC.
  - Separately, each TCP connection gets its own thread and waits up to 60 s, so a LAN host can open
    thousands. That is a DoS on the elevated process.
- **Fix:**
  - the client proves first: it sends `MAC("auth-c", nc, ns)` before the server reveals anything;
  - cap concurrent unauthenticated connections;
  - keep the current minimum length on custom codes, or require generated codes.

```rust
// serve_conn: client proves first (protocol v4; both PCs must update)
let ns = random::<16>();
s.write_all(&ns)?;                                   // nonce only, no MAC yet
let mut m = [0u8; 32];
s.read_exact(&mut m)?;
if !ct_eq(&m, &key.mac("auth-c", &[nc, &ns])) { return Err(bad("a PC with a different pairing code tried to connect")); }
s.write_all(&key.mac("auth-s", &[nc, &ns]))?;         // only now does the server prove itself

// accept loop
static PENDING: AtomicUsize = AtomicUsize::new(0);
if PENDING.fetch_add(1, Relaxed) >= 32 { PENDING.fetch_sub(1, Relaxed); continue; } // drop excess
// … decrement when the handshake finishes or fails
```

### 3.2 Input validation at boundaries: ✅
- **Frame sizes:** frames are length-checked (`17..=MAX_FRAME`: `beam/net.rs:201`, `ccsync/lan.rs:103`) and
  read only **after** the peer has proved the pairing code (`beam/net.rs:974-978`). The 256 MiB / 96 MiB
  buffers can't be forced by a stranger.
- **Peer identity:** the peer's hello is checked against its Ed25519 key, and its id must be the key's hash
  (`beam/device.rs` `check_hello`).
- **Received file names:**
  - cleaned by `clean_name` (`beam/net.rs:280`), `safe_rel` (`ccsync/scan.rs:83`) and `safe_name`
    (`remote.rs:464`);
  - then written through `fsguard` (handle-relative opens, no following links out of the root, checked
    against the user's own rights).
- **RivaTuner shared memory** (a local, untrusted header): bounded by `array()` against the mapping size from
  `VirtualQuery`, and name reads capped at 260 bytes (V9, fixed in 2.5.6, with a test).
- **Downloaded JSON** (GitHub, exchange rates, Steam relays) is parsed by `serde_json` into typed or
  `Value` data, not executed. The RustDesk asset name from GitHub is used as a file name (`rdesk.rs:229`).
  It is TLS-protected, but F3's fix replaces it with a fixed name anyway.

---

## 4. Fix order

1. **F1 and F2** (High, S effort each): two small code changes close two silent user-to-admin paths. Add
   a test for each.
2. **F4** (S): one helper function.
3. **F3 and F5** (S–M).
4. **F6** (S, then a full regression run with WebView2 and the GPU DLLs).
5. **F8** (M; protocol bump, both PCs update) and **F7** (M, with migration).
6. Add to `docs/trust-boundaries.md`: "an elevated Deck starts processes only from admin-only full paths,
   with a fixed environment". Add a test that greps for bare-name `Command::new` targets outside System32.

## What was not covered
- No dynamic testing on a separate VM: no Process Monitor traces, no fuzzing of the network parsers, no
  live MITM attempt.
- F1 and F2 were confirmed by code path plus a harmless reproduction of the module shadowing. The full chain
  was **not** run against the elevated Deck.
- Third-party crates were covered by `cargo audit` only, with no manual review. RivaTuner, RustDesk,
  Tailscale and the PawnIO driver are out of scope.
