//! The programs a Claude Code session may need to build and run a project: Rust (toolchains,
//! targets, components, cargo-installed tools), Node and global npm packages, Python and pipx
//! tools, Git. Each PC publishes what it has; the other PCs show what they're missing and, when
//! the user asks, install it in a PowerShell window that runs with the user's normal rights.
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Tool {
    /// "app" (rustup, node, python, git), "rust-toolchain", "rust-target", "rust-component", "cargo", "npm", "pipx"
    pub kind: String,
    pub name: String,
    #[serde(default)]
    pub version: String,
}

impl Tool {
    pub fn key(&self) -> String {
        format!("{}|{}", self.kind, self.name.to_ascii_lowercase())
    }

    /// what the page calls it
    pub fn label(&self) -> String {
        let v = if self.version.is_empty() { String::new() } else { format!(" {}", self.version) };
        match self.kind.as_str() {
            "app" => format!("{}{v}", app_name(&self.name)),
            "rust-toolchain" => format!("Rust toolchain {}", self.name),
            "rust-target" => format!("Rust target {}", self.name),
            "rust-component" => format!("Rust component {}", self.name),
            "cargo" => format!("{}{v} (cargo install)", self.name),
            "npm" => format!("{}{v} (npm, global)", self.name),
            "pipx" => format!("{}{v} (pipx)", self.name),
            _ => self.name.clone(),
        }
    }
}

fn app_name(n: &str) -> &str {
    match n {
        "rustup" => "Rust (rustup)",
        "node" => "Node.js",
        "python" => "Python",
        "git" => "Git",
        "msvc" => "C++ build tools (Visual Studio, needed by Rust)",
        x => x,
    }
}

static CACHE: Mutex<Option<(Instant, Vec<Tool>)>> = Mutex::new(None);
static BUSY: Mutex<bool> = Mutex::new(false);
const MAX_AGE: Duration = Duration::from_secs(15 * 60);

/// What this PC has (None until the first look finished). A look older than 15 minutes is
/// refreshed in the background.
pub fn current() -> Option<Vec<Tool>> {
    let c = CACHE.lock().unwrap_or_else(|e| e.into_inner()).clone();
    if c.as_ref().map(|(t, _)| t.elapsed() > MAX_AGE).unwrap_or(true) {
        refresh();
    }
    c.map(|(_, v)| v)
}

/// Looks again now (in the background).
pub fn refresh() {
    {
        let mut b = BUSY.lock().unwrap_or_else(|e| e.into_inner());
        if *b {
            return;
        }
        *b = true;
    }
    std::thread::spawn(|| {
        let v = detect();
        *CACHE.lock().unwrap_or_else(|e| e.into_inner()) = Some((Instant::now(), v));
        *BUSY.lock().unwrap_or_else(|e| e.into_inner()) = false;
    });
}

/// Windows' PATH as it is now (Deck may have started before something was installed).
fn fresh_path() -> String {
    let get = |root: &str, key: &str| -> String {
        crate::util::run_for(10, "reg.exe", &["query", root, "/v", key])
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
            .and_then(|s| s.lines().find_map(|l| l.split_once("REG_EXPAND_SZ").or_else(|| l.split_once("REG_SZ")).map(|(_, v)| v.trim().to_string())))
            .unwrap_or_default()
    };
    let m = get(r"HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\Environment", "Path");
    let u = get(r"HKCU\Environment", "Path");
    let expand = |s: String| {
        let mut out = s;
        for (k, v) in std::env::vars() {
            out = out.replace(&format!("%{k}%"), &v).replace(&format!("%{}%", k.to_ascii_uppercase()), &v);
        }
        out
    };
    let mut p = format!("{};{}", expand(m), expand(u));
    // rustup's folder, even if the PATH doesn't list it yet
    p.push_str(&format!(";{}", crate::util::home().join(".cargo").join("bin").display()));
    p
}

fn run(path: &str, exe: &str, args: &[&str]) -> Option<String> {
    use std::os::windows::process::CommandExt;
    let o = std::process::Command::new("cmd.exe")
        .arg("/d")
        .arg("/c")
        .arg(exe)
        .args(args)
        .env("PATH", path)
        .creation_flags(0x0800_0000)
        .stdin(std::process::Stdio::null())
        .output()
        .ok()?;
    o.status.success().then(|| String::from_utf8_lossy(&o.stdout).to_string())
}

fn ver_of(s: &str) -> String {
    s.split_whitespace().find(|w| w.trim_start_matches('v').starts_with(|c: char| c.is_ascii_digit())).map(|w| w.trim_start_matches('v').to_string()).unwrap_or_default()
}

pub fn detect() -> Vec<Tool> {
    let path = fresh_path();
    let mut v = Vec::new();
    let mut add = |kind: &str, name: &str, version: &str| {
        if valid_name(name) && valid_version(version) {
            v.push(Tool { kind: kind.into(), name: name.into(), version: version.into() });
        }
    };
    if let Some(out) = run(&path, "rustup", &["--version"]) {
        add("app", "rustup", &ver_of(&out));
        let tcs = run(&path, "rustup", &["toolchain", "list"]).unwrap_or_default();
        let mut host = String::new();
        let mut names = Vec::new();
        for l in tcs.lines() {
            let name = l.split_whitespace().next().unwrap_or("");
            if name.is_empty() || name.starts_with("no ") {
                continue;
            }
            if l.contains("default") {
                host = name.split_once('-').map(|(_, h)| h.to_string()).unwrap_or_default();
            }
            names.push(name.to_string());
        }
        // "stable", not "stable-x86_64-pc-windows-msvc": a PC with another processor installs
        // its own kind (one for another host stays as it is)
        for n in names {
            let short = if host.is_empty() { n.as_str() } else { n.strip_suffix(&format!("-{host}")).unwrap_or(&n) };
            add("rust-toolchain", short, "");
        }
        for t in run(&path, "rustup", &["target", "list", "--installed"]).unwrap_or_default().lines().map(str::trim).filter(|t| !t.is_empty()) {
            if t != host {
                add("rust-target", t, "");
            }
        }
        // the ones a toolchain always has don't need a mention
        const BUILT_IN: &[&str] = &["rustc", "cargo", "rust-std", "rust-docs", "rust-mingw", "rustfmt", "clippy"];
        for c in run(&path, "rustup", &["component", "list", "--installed"]).unwrap_or_default().lines().map(str::trim) {
            let c = if host.is_empty() { c } else { c.strip_suffix(&format!("-{host}")).unwrap_or(c) };
            if !c.is_empty() && !BUILT_IN.contains(&c) && !c.starts_with("rust-std") {
                add("rust-component", c, "");
            }
        }
        // `name v1.2.3:` (tools installed from a local folder or git can't be installed elsewhere)
        for l in run(&path, "cargo", &["install", "--list"]).unwrap_or_default().lines().filter(|l| !l.starts_with(' ')) {
            let l = l.trim_end_matches(':');
            if l.contains('(') {
                continue;
            }
            if let Some((n, ver)) = l.split_once(' ') {
                add("cargo", n, ver.trim_start_matches('v'));
            }
        }
    }
    if let Some(out) = run(&path, "node", &["--version"]) {
        add("app", "node", &ver_of(&out));
        if let Some(j) = run(&path, "npm", &["ls", "-g", "--depth=0", "--json"]).and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok()) {
            for (n, d) in j["dependencies"].as_object().into_iter().flatten() {
                if n != "npm" && n != "corepack" {
                    add("npm", n, d["version"].as_str().unwrap_or(""));
                }
            }
        }
    }
    // (the Microsoft Store's "python" placeholder prints nothing and fails)
    if let Some(out) = run(&path, "python", &["--version"]).filter(|o| o.starts_with("Python ")) {
        add("app", "python", &ver_of(&out));
    }
    if let Some(out) = run(&path, "pipx", &["list", "--short"]) {
        for l in out.lines() {
            if let Some((n, ver)) = l.trim().split_once(' ') {
                add("pipx", n, ver.trim());
            }
        }
    }
    if let Some(out) = run(&path, "git", &["--version"]) {
        add("app", "git", &ver_of(&out));
    }
    // Rust on Windows links with Microsoft's C++ build tools (Visual Studio or its Build Tools)
    let vswhere = crate::util::program_files_x86().join(r"Microsoft Visual Studio\Installer\vswhere.exe");
    if vswhere.is_file() {
        let o = crate::util::run_for(30, &vswhere.to_string_lossy(), &["-products", "*", "-requires", "Microsoft.VisualStudio.Component.VC.Tools.x86.x64", "-latest", "-property", "catalog_productLineVersion"]);
        if let Some(year) = o.ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).filter(|y| !y.is_empty()) {
            add("app", "msvc", &year);
        }
    }
    v.sort();
    v.dedup();
    v
}

/// Names and versions come from another PC and end up in a PowerShell script: only plain ones.
pub fn valid_name(s: &str) -> bool {
    !s.is_empty() && s.len() <= 120 && s.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '@') && s.chars().all(|c| c.is_ascii_alphanumeric() || "._+/@-".contains(c)) && !s.contains("..")
}

pub fn valid_version(s: &str) -> bool {
    s.len() <= 40 && s.chars().all(|c| c.is_ascii_alphanumeric() || ".+_-".contains(c))
}

/// What `theirs` has that `here` doesn't (by name; other versions count as there).
pub fn missing(here: &[Tool], theirs: &[Tool]) -> Vec<Tool> {
    let have: std::collections::HashSet<String> = here.iter().map(Tool::key).collect();
    theirs.iter().filter(|t| !have.contains(&t.key()) && valid_name(&t.name) && valid_version(&t.version)).cloned().collect()
}

/// The PowerShell line that installs `t` (None: not something Deck installs).
pub fn command(t: &Tool) -> Option<String> {
    if !valid_name(&t.name) || !valid_version(&t.version) {
        return None;
    }
    let winget = |id: &str| format!("winget install --id '{id}' -e --accept-package-agreements --accept-source-agreements; Refresh-Path");
    let (n, v) = (&t.name, &t.version);
    Some(match t.kind.as_str() {
        "app" => match n.as_str() {
            "rustup" => winget("Rustlang.Rustup"),
            "node" => winget("OpenJS.NodeJS.LTS"),
            "git" => winget("Git.Git"),
            "msvc" => "winget install --id 'Microsoft.VisualStudio.2022.BuildTools' -e --accept-package-agreements --accept-source-agreements --override '--wait --passive --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended'; Refresh-Path".into(),
            "python" => {
                let minor: Vec<&str> = v.split('.').take(2).collect();
                winget(&if minor.len() == 2 { format!("Python.Python.{}.{}", minor[0], minor[1]) } else { "Python.Python.3.12".into() })
            }
            _ => return None,
        },
        "rust-toolchain" => format!("rustup toolchain install '{n}'"),
        "rust-target" => format!("rustup target add '{n}'"),
        "rust-component" => format!("rustup component add '{n}'"),
        "cargo" if v.is_empty() => format!("cargo install '{n}' --locked"),
        "cargo" => format!("cargo install '{n}' --version '{v}' --locked"),
        "npm" if v.is_empty() => format!("npm install -g '{n}'"),
        "npm" => format!("npm install -g '{n}@{v}'"),
        "pipx" if v.is_empty() => format!("pipx install '{n}'"),
        "pipx" => format!("pipx install '{n}=={v}'"),
        _ => return None,
    })
}

/// A PowerShell script installing `items` (programs first, so the commands after them exist).
pub fn script(from: &str, items: &[Tool]) -> String {
    let mut items = items.to_vec();
    // programs first (the C++ build tools before Rust, which needs them to build), then the rest
    items.sort_by_key(|t| (t.kind != "app", t.name != "msvc", t.kind != "rust-toolchain", t.kind.clone(), t.name.clone()));
    let mut s = String::from(
        "$ErrorActionPreference = 'Continue'\r\n\
         function Refresh-Path { $env:Path = [Environment]::GetEnvironmentVariable('Path','Machine') + ';' + [Environment]::GetEnvironmentVariable('Path','User') + ';' + \"$env:USERPROFILE\\.cargo\\bin\" }\r\n\
         Refresh-Path\r\n",
    );
    let from: String = from.chars().filter(|c| c.is_alphanumeric() || " -_".contains(*c)).collect();
    s.push_str(&format!("Write-Host 'Deck: installing what {from} has and this PC is missing' -ForegroundColor Cyan\r\n"));
    for t in &items {
        if let Some(c) = command(t) {
            let label: String = t.label().chars().filter(|c| *c != '\'').collect();
            s.push_str(&format!("Write-Host ''; Write-Host '> {label}' -ForegroundColor Yellow\r\n{c}\r\n"));
        }
    }
    s.push_str("Write-Host ''; Write-Host 'Done. You can close this window.' -ForegroundColor Green\r\n");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(kind: &str, name: &str, version: &str) -> Tool {
        Tool { kind: kind.into(), name: name.into(), version: version.into() }
    }

    #[test]
    fn what_is_missing() {
        let here = vec![t("app", "rustup", "1.28"), t("rust-target", "x86_64-pc-windows-msvc", "")];
        let there = vec![t("app", "rustup", "1.27"), t("rust-target", "aarch64-linux-android", ""), t("cargo", "tauri-cli", "2.12.1"), t("npm", "@github/copilot", "1.0.80")];
        let m = missing(&here, &there);
        assert_eq!(m.len(), 3, "another version counts as there");
        assert_eq!(command(&m[0]).unwrap(), "rustup target add 'aarch64-linux-android'");
        assert_eq!(command(&m[1]).unwrap(), "cargo install 'tauri-cli' --version '2.12.1' --locked");
        assert_eq!(command(&m[2]).unwrap(), "npm install -g '@github/copilot@1.0.80'");
        assert!(command(&t("app", "python", "3.11.3")).unwrap().contains("Python.Python.3.11"));
    }

    #[test]
    fn nothing_from_another_pc_reaches_the_script_unchecked() {
        for bad in ["x'; Remove-Item C:\\ -Recurse; '", "a b", "$(evil)", "../x", "", "x`y"] {
            assert!(!valid_name(bad), "{bad}");
            assert!(command(&t("cargo", bad, "1")).is_none());
        }
        assert!(command(&t("cargo", "ok", "1'; calc")).is_none());
        assert!(missing(&[], &[t("npm", "x;y", "")]).is_empty());
        let s = script("LAPTOP'; calc; '", &[t("rust-target", "aarch64-linux-android", ""), t("app", "rustup", "1")]);
        assert!(!s.contains("calc;"));
        assert!(s.find("Rustlang.Rustup").unwrap() < s.find("rustup target add").unwrap(), "rustup first");
    }

    /// What this PC has, as the other PCs would see it.
    #[test]
    #[ignore = "runs this PC's rustup, cargo, npm..."]
    fn detect_here() {
        for t in detect() {
            println!("{:<16} {}", t.kind, t.label());
        }
    }
}
