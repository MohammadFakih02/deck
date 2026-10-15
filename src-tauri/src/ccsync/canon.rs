//! Machine-independent ("canonical") form of synced files.
//!
//! Every project has an alias (e.g. `GAME_APP`) that maps to a local path on each machine. Inside file
//! contents, each spelling of a local project path is replaced by a token `<<ccs:ALIAS:VC>>`, where V is
//! the spelling and C the drive-letter case:
//!   r = `C:\a\b`   j = `C:\\a\\b` (JSON)   J = `C:\\\\a\\\\b` (JSON in JSON)   f = `C:/a/b`
//!   e = `C--a-b` (Claude's encoded project folder name)
//! The receiving machine turns tokens back into its own path for that alias. The home folder is the
//! built-in alias `~`.
use aho_corasick::{AhoCorasick, AhoCorasickBuilder, MatchKind};
use std::borrow::Cow;
use std::io::Write;

const SEPS: [(u8, &str); 4] = [(b'r', "\\"), (b'j', "\\\\"), (b'J', "\\\\\\\\"), (b'f', "/")];
const OPEN: &[u8] = b"<<ccs:";

/// `C:/x/y\` -> `C:\x\y`
pub fn norm_path(p: &str) -> String {
    let mut s = p.trim().replace('/', "\\");
    while s.len() > 3 && s.ends_with('\\') {
        s.pop();
    }
    s
}

/// Claude Code's project folder name: every char outside [A-Za-z0-9] becomes '-' (per UTF-16 unit).
pub fn encode_dir(p: &str) -> String {
    let mut s = String::with_capacity(p.len());
    for c in p.chars() {
        if c.is_ascii_alphanumeric() {
            s.push(c);
        } else {
            for _ in 0..c.len_utf16() {
                s.push('-');
            }
        }
    }
    s
}

pub struct Canon {
    ac: Option<AhoCorasick>,
    pats: Vec<(usize, u8)>,
    aliases: Vec<(String, String)>,
    pub fp: String,
}

impl Canon {
    pub fn new(aliases: &[(String, String)]) -> Canon {
        let mut strs: Vec<String> = Vec::new();
        let mut pats = Vec::new();
        let mut al: Vec<(String, String)> = Vec::new();
        let mut seen: Vec<String> = Vec::new();
        let mut h = blake3::Hasher::new();
        for (a, p) in aliases {
            let p = norm_path(p);
            h.update(a.as_bytes());
            h.update(b"\0");
            h.update(p.as_bytes());
            h.update(b"\n");
            let idx = al.len();
            al.push((a.clone(), p.clone()));
            // two aliases on one path: only the first one tokenizes (decode still renders both)
            let low = p.to_ascii_lowercase();
            if p.len() < 4 || !p.contains('\\') || seen.contains(&low) {
                continue;
            }
            seen.push(low);
            let segs: Vec<&str> = p.split('\\').collect();
            for (v, sep) in SEPS {
                strs.push(segs.join(sep));
                pats.push((idx, v));
            }
            strs.push(encode_dir(&p));
            pats.push((idx, b'e'));
            if let Some(m) = msys(&p) {
                strs.push(m);
                pats.push((idx, b'm'));
            }
        }
        let ac = if strs.is_empty() {
            None
        } else {
            AhoCorasickBuilder::new().match_kind(MatchKind::LeftmostLongest).ascii_case_insensitive(true).build(&strs).ok()
        };
        Canon { ac, pats, aliases: al, fp: h.finalize().to_hex()[..16].to_string() }
    }

    fn path_of(&self, alias: &str) -> Option<&str> {
        self.aliases.iter().find(|(a, _)| a == alias).map(|(_, p)| p.as_str())
    }

    /// local bytes -> canonical bytes. Binary files (NUL in the first 8 KB) pass through unchanged.
    pub fn encode<'a>(&self, d: &'a [u8]) -> Cow<'a, [u8]> {
        let Some(ac) = &self.ac else { return Cow::Borrowed(d) };
        if d[..d.len().min(8192)].contains(&0) {
            return Cow::Borrowed(d);
        }
        let mut out: Option<Vec<u8>> = None;
        let mut last = 0;
        for m in ac.find_iter(d) {
            let (s, e) = (m.start(), m.end());
            let (ai, v) = self.pats[m.pattern().as_usize()];
            if !boundary_ok(d, s, e, v) {
                continue;
            }
            let o = out.get_or_insert_with(|| Vec::with_capacity(d.len()));
            o.extend_from_slice(&d[last..s]);
            let c = if v == b'm' { d[s + 1] } else { d[s] };
            let case = if c.is_ascii_uppercase() {
                'u'
            } else if c.is_ascii_lowercase() {
                'l'
            } else {
                'x'
            };
            let _ = write!(o, "<<ccs:{}:{}{}>>", self.aliases[ai].0, v as char, case);
            last = e;
        }
        match out {
            None => Cow::Borrowed(d),
            Some(mut o) => {
                o.extend_from_slice(&d[last..]);
                Cow::Owned(o)
            }
        }
    }

    /// canonical bytes -> local bytes. Unknown aliases are left as tokens.
    pub fn decode<'a>(&self, d: &'a [u8]) -> Cow<'a, [u8]> {
        let finder = memchr::memmem::Finder::new(OPEN);
        if finder.find(d).is_none() {
            return Cow::Borrowed(d);
        }
        let mut o = Vec::with_capacity(d.len() + 4096);
        let mut last = 0;
        for s in finder.find_iter(d) {
            if s < last {
                continue;
            }
            let rest = &d[s + OPEN.len()..];
            let Some(colon) = rest.iter().take(80).position(|&b| b == b':') else { continue };
            if rest.len() < colon + 5 || &rest[colon + 3..colon + 5] != b">>" {
                continue;
            }
            let Ok(alias) = std::str::from_utf8(&rest[..colon]) else { continue };
            let Some(path) = self.path_of(alias) else { continue };
            o.extend_from_slice(&d[last..s]);
            o.extend_from_slice(render(path, rest[colon + 1], rest[colon + 2]).as_bytes());
            last = s + OPEN.len() + colon + 5;
        }
        o.extend_from_slice(&d[last..]);
        Cow::Owned(o)
    }
}

fn boundary_ok(d: &[u8], s: usize, e: usize, v: u8) -> bool {
    if s > 0 {
        let p = d[s - 1];
        // `\n`, `\r`, `\t` escapes in JSON text are fine before a path
        let escape = s >= 2 && d[s - 2] == b'\\' && matches!(p, b'n' | b'r' | b't');
        if !escape && (p.is_ascii_alphanumeric() || p == b'_' || (v == b'e' && p == b'-')) {
            return false;
        }
    }
    if e < d.len() {
        let n = d[e];
        if n.is_ascii_alphanumeric() || n == b'_' || n == b'-' {
            return false;
        }
        // "...\GAME APP." ends a sentence, "...\GAME APP.old" is another folder
        if n == b'.' && e + 1 < d.len() && d[e + 1].is_ascii_alphanumeric() {
            return false;
        }
    }
    true
}

fn render(path: &str, v: u8, case: u8) -> String {
    let mut s = if v == b'e' {
        encode_dir(path)
    } else if v == b'm' {
        msys(path).unwrap_or_else(|| path.replace('\\', "/"))
    } else {
        let sep = SEPS.iter().find(|(k, _)| *k == v).map(|(_, s)| *s).unwrap_or("\\");
        path.split('\\').collect::<Vec<_>>().join(sep)
    };
    let at = if v == b'm' { 1 } else { 0 };
    if let Some(c) = s.chars().nth(at) {
        if c.is_ascii_alphabetic() {
            let n = if case == b'l' { c.to_ascii_lowercase() } else if case == b'u' { c.to_ascii_uppercase() } else { c };
            s.replace_range(at..at + 1, &n.to_string());
        }
    }
    s
}

/// Git Bash / MSYS spelling: `C:\a\b` -> `/c/a/b`
fn msys(p: &str) -> Option<String> {
    let b = p.as_bytes();
    if b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'\\' {
        Some(format!("/{}/{}", (b[0] as char).to_ascii_lowercase(), p[3..].replace('\\', "/")))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_between_machines() {
        let pc = Canon::new(&[("~".into(), r"C:\Users\ALEXANDER".into()), ("GAME_APP".into(), r"C:\PROJECTS\GAME APP".into())]);
        let lap = Canon::new(&[("~".into(), r"C:\Users\sam".into()), ("GAME_APP".into(), r"D:\Work\phone".into())]);
        let src = br#"{"cwd":"c:\\PROJECTS\\GAME APP","p":"C:\\Users\\ALEXANDER\\.claude\\projects\\c--PROJECTS-GAME-APP\\memory","t":"see C:\PROJECTS\GAME APP. and C:/PROJECTS/GAME APP/x","n":"C:\\PROJECTS\\GAME APP2","m":"C:\\Users\\ALEXANDERX","g":"/c/Users/ALEXANDER","o":"dir\r\nC:\\Users\\ALEXANDER\\x","h":"/c/PROJECTS/GAME APP/a"}"#;
        let c = pc.encode(src).into_owned();
        let s = String::from_utf8_lossy(&c);
        assert!(s.contains("<<ccs:GAME_APP:jl>>"), "{s}");
        assert!(s.contains("<<ccs:GAME_APP:el>>"), "{s}");
        assert!(s.contains("<<ccs:~:ju>>"), "{s}");
        assert!(s.contains("GAME APP2"), "{s}");
        assert!(s.contains("ALEXANDERX"), "{s}");
        let l = lap.decode(&c).into_owned();
        let ls = String::from_utf8_lossy(&l);
        assert!(ls.contains(r#""cwd":"d:\\Work\\phone""#), "{ls}");
        assert!(ls.contains(r#"C:\\Users\\sam\\.claude\\projects\\d--Work-phone\\memory"#), "{ls}");
        assert!(ls.contains(r#"see D:\Work\phone. and D:/Work/phone/x"#), "{ls}");
        assert!(ls.contains(r#""g":"/c/Users/sam""#), "{ls}");
        assert!(ls.contains(r#"dir\r\nC:\\Users\\sam\\x"#), "{ls}");
        assert!(ls.contains(r#""h":"/d/Work/phone/a""#), "{ls}");
        // and back again gives the same canonical bytes
        assert_eq!(lap.encode(&l).into_owned(), c);
        assert_eq!(pc.decode(&c).into_owned(), src.to_vec());
    }

    #[test]
    fn encode_dir_matches_claude() {
        assert_eq!(encode_dir(r"C:\PROJECTS\GAME APP"), "C--PROJECTS-GAME-APP");
        assert_eq!(encode_dir(r"e:\My PS5\Games_PKGs"), "e--My-PS5-Games-PKGs");
    }
}
