//! Project files are copied without `.git` (each PC keeps its own). A PC whose copy has no git yet
//! can get the history from the same place the other PC does: its `origin` remote and branch.
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct GitInfo {
    pub url: String,
    pub branch: String,
}

/// The `origin` remote and current branch of the git repository in `dir` (None: no git, no
/// origin, a detached HEAD, or something that can't be used safely).
pub fn read(dir: &Path) -> Option<GitInfo> {
    let git = dir.join(".git");
    if !git.is_dir() {
        return None;
    }
    let head = std::fs::read_to_string(git.join("HEAD")).ok()?;
    let branch = head.trim().strip_prefix("ref: refs/heads/")?.to_string();
    let cfg = std::fs::read_to_string(git.join("config")).ok()?;
    let mut in_origin = false;
    let mut url = None;
    for l in cfg.lines().map(str::trim) {
        if l.starts_with('[') {
            in_origin = l.replace(' ', "").eq_ignore_ascii_case("[remote\"origin\"]");
        } else if in_origin {
            if let Some((k, v)) = l.split_once('=') {
                if k.trim() == "url" {
                    url = Some(v.trim().to_string());
                }
            }
        }
    }
    let g = GitInfo { url: url?, branch };
    valid(&g).then_some(g)
}

/// Comes from another PC and goes into a PowerShell script: a network address (not a folder on
/// that PC) and a plain branch name only.
pub fn valid(g: &GitInfo) -> bool {
    let plain = |s: &str, extra: &str| s.chars().all(|c| c.is_ascii_alphanumeric() || extra.contains(c));
    let url_ok = (g.url.starts_with("https://") || g.url.starts_with("ssh://") || g.url.starts_with("git@")) && g.url.len() < 300 && plain(&g.url, "-._~:/@+%");
    let branch_ok = !g.branch.is_empty() && g.branch.len() < 120 && plain(&g.branch, "-._/") && !g.branch.contains("..") && !g.branch.starts_with(['-', '/', '.']);
    url_ok && branch_ok
}

/// Connects the folder `dir` (files already there) to the repository: its history is fetched and
/// the branch points at it, without touching the files, so `git status` shows what differs.
pub fn setup_script(dir: &str, g: &GitInfo) -> Option<String> {
    if !valid(g) {
        return None;
    }
    let d = dir.replace('\'', "''");
    let (u, b) = (&g.url, &g.branch);
    Some(format!(
        "$ErrorActionPreference = 'Stop'\r\n\
         Set-Location -LiteralPath '{d}'\r\n\
         Write-Host 'Deck: getting the git history for this folder' -ForegroundColor Cyan\r\n\
         git init\r\n\
         git remote add origin '{u}'\r\n\
         git fetch origin '{b}'\r\n\
         git symbolic-ref HEAD 'refs/heads/{b}'\r\n\
         git reset FETCH_HEAD\r\n\
         git branch --set-upstream-to='origin/{b}'\r\n\
         git status --short | Select-Object -First 30\r\n\
         Write-Host ''; Write-Host 'Done. Above: files that differ from the last commit (if any). You can close this window.' -ForegroundColor Green\r\n"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_origin_and_branch() {
        let d = std::env::temp_dir().join(format!("deck-git-{}", crate::util::now_ms()));
        std::fs::create_dir_all(d.join(".git")).unwrap();
        std::fs::write(d.join(".git").join("HEAD"), "ref: refs/heads/main\n").unwrap();
        std::fs::write(d.join(".git").join("config"), "[core]\n\tbare = false\n[remote \"upstream\"]\n\turl = https://x/y.git\n[remote \"origin\"]\n\turl = https://github.com/MohammadFakih02/deck.git\n\tfetch = +refs/heads/*:refs/remotes/origin/*\n").unwrap();
        assert_eq!(read(&d), Some(GitInfo { url: "https://github.com/MohammadFakih02/deck.git".into(), branch: "main".into() }));
        std::fs::write(d.join(".git").join("HEAD"), "0123456789abcdef\n").unwrap();
        assert_eq!(read(&d), None, "detached");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn nothing_odd_reaches_the_script() {
        let g = |u: &str, b: &str| GitInfo { url: u.into(), branch: b.into() };
        assert!(valid(&g("git@github.com:me/x.git", "feature/a-b")));
        for (u, b) in [("C:\\repo", "main"), ("https://x/'; calc; '", "main"), ("https://x/y", "main'; calc"), ("https://x/y", "-x"), ("file:///c/x", "main"), ("https://x y", "main"), ("https://x/$(evil)", "main")] {
            assert!(!valid(&g(u, b)), "{u} {b}");
            assert!(setup_script("C:\\x", &g(u, b)).is_none());
        }
        let s = setup_script("C:\\it's here", &g("https://github.com/a/b.git", "main")).unwrap();
        assert!(s.contains("'C:\\it''s here'"));
    }
}
