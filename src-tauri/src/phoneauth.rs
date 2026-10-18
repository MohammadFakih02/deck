//! Phone requests carry a signature instead of the phone key. The remote runs over plain HTTP on
//! the home network, where anyone on the same Wi-Fi can read the traffic, so the key itself never
//! travels: the page opens a session (a random id, not a secret), then signs every request with
//! HMAC-SHA256(key, "session|counter|path"). Each counter is used once, so a recorded request
//! can't be sent again; only plain reads of what the phone already fetched (album art, a shared
//! file) may repeat one.

use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::time::{Duration, Instant};

pub fn hmac_sha256(key: &[u8], msg: &[u8]) -> [u8; 32] {
    let mut k = [0u8; 64];
    if key.len() > 64 {
        k[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let pad = |b: u8| k.map(|x| x ^ b);
    let inner = Sha256::new().chain_update(pad(0x36)).chain_update(msg).finalize();
    Sha256::new().chain_update(pad(0x5c)).chain_update(inner).finalize().into()
}

/// Equal without telling, by how long it takes, how much of the start was right.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// The signature the phone page sends (lowercase hex), see the module comment.
pub fn sign(token: &str, session: &str, counter: u64, path: &str) -> String {
    hex(&hmac_sha256(token.as_bytes(), format!("{session}|{counter}|{path}").as_bytes()))
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

#[derive(Debug, PartialEq, Eq)]
pub enum Verdict {
    Ok,
    /// signed with another key (the phone has an old code), or not signed, or sent again
    Refused,
    /// the session isn't known (Deck restarted, or it was dropped): the page opens a new one
    NoSession,
}

struct Session {
    id: String,
    used: BTreeSet<u64>,
    top: u64,
    last: Instant,
}

/// The open sessions (one per phone page; a reload opens a new one).
pub struct Sessions {
    list: Vec<Session>,
}

impl Sessions {
    pub const fn new() -> Self {
        Self { list: Vec::new() }
    }

    /// A new session's id. The oldest unused sessions go first when there are too many.
    pub fn open(&mut self) -> String {
        self.list.retain(|x| x.last.elapsed() < IDLE);
        if self.list.len() >= MAX_SESSIONS {
            if let Some(i) = self.list.iter().enumerate().min_by_key(|(_, x)| x.last).map(|(i, _)| i) {
                self.list.remove(i);
            }
        }
        let id = crate::util::random_token(32);
        self.list.push(Session { id: id.clone(), used: BTreeSet::new(), top: 0, last: Instant::now() });
        id
    }

    /// Whether a request signed `mac` for `path` with counter `counter` in `session` comes from a
    /// page that knows `token`. `repeatable`: a plain read that may be fetched again with the same
    /// signature (the browser asks again for a picture it already has).
    pub fn check(&mut self, token: &str, session: &str, counter: u64, mac: &str, path: &str, repeatable: bool) -> Verdict {
        let Some(x) = self.list.iter_mut().find(|x| x.id == session) else { return Verdict::NoSession };
        if token.is_empty() || counter == 0 || !same(mac, &sign(token, session, counter, path)) {
            return Verdict::Refused;
        }
        x.last = Instant::now();
        let fresh = counter > x.top.saturating_sub(WINDOW) && !x.used.contains(&counter);
        if !fresh {
            // a read the browser repeats (however old its signature): it only gets what it had
            return if repeatable { Verdict::Ok } else { Verdict::Refused };
        }
        x.used.insert(counter);
        x.top = x.top.max(counter);
        let floor = x.top.saturating_sub(WINDOW);
        x.used = x.used.split_off(&floor);
        Verdict::Ok
    }
}

/// How far behind the newest counter a request may still arrive (requests run side by side).
const WINDOW: u64 = 4096;
/// Sessions kept; opening more lets the longest unused one go.
const MAX_SESSIONS: usize = 64;
/// A session nobody used for this long is let go.
const IDLE: Duration = Duration::from_secs(24 * 3600);

/// Whether `host` (a request's Host header) names this PC: an IP address, a one-word computer
/// name, or a home-network or Tailscale name. A web page that points one of its own internet names
/// at this PC (DNS rebinding) sends its own name and is refused.
pub fn host_ok(host: &str) -> bool {
    let h = host.trim();
    if let Some(rest) = h.strip_prefix('[') {
        return rest.split(']').next().is_some_and(|ip| ip.parse::<std::net::Ipv6Addr>().is_ok());
    }
    let name = match h.rsplit_once(':') {
        Some((n, port)) if !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()) => n,
        _ => h,
    };
    let name = name.trim_end_matches('.').to_ascii_lowercase();
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c)) {
        return false;
    }
    if name.parse::<std::net::IpAddr>().is_ok() || !name.contains('.') {
        return true;
    }
    [".local", ".lan", ".home", ".home.arpa", ".internal", ".ts.net"].iter().any(|s| name.ends_with(s))
}

/// The Content-Security-Policy for a page of Deck's: its own inline scripts by hash and nothing
/// else runs, it connects only back to Deck, and nothing loads from anywhere else.
pub fn csp_for(html: &str) -> String {
    let mut hashes = String::new();
    let mut rest = html;
    while let Some(i) = rest.find("<script") {
        let after = &rest[i..];
        let Some(close) = after.find('>') else { break };
        let tag = &after[..close];
        let body_start = close + 1;
        let Some(end) = after[body_start..].find("</script>") else { break };
        if !tag.contains("src=") {
            // the browser's text: line ends read as LF (the file has CRLF on a Windows checkout)
            let text = after[body_start..body_start + end].replace("\r\n", "\n").replace('\r', "\n");
            let h = crate::util::base64(&Sha256::digest(text.as_bytes()));
            hashes += &format!(" 'sha256-{h}'");
        }
        rest = &after[body_start + end..];
    }
    format!(
        "default-src 'none'; script-src 'self'{hashes}; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; media-src 'self' blob:; connect-src 'self' ws: wss:; worker-src 'self' blob:; manifest-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hmac_matches_rfc_4231() {
        assert_eq!(hex(&hmac_sha256(b"Jefe", b"what do ya want for nothing?")), "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843");
        // a key longer than the block is hashed first (RFC 4231 test case 6)
        let key = [0xaau8; 131];
        assert_eq!(
            hex(&hmac_sha256(&key, b"Test Using Larger Than Block-Size Key - Hash Key First")),
            "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54"
        );
    }

    /// The same signature the phone page computes (tools/testing/phone-sign.mjs checks the page
    /// against these values).
    #[test]
    fn signatures_match_the_phone_page() {
        assert_eq!(sign("phone-key", "SESSIONID", 7, "/ws"), "816f170724a06f745eac07cdded9c07d9913abb49be6987187e3d0216b3276f6");
        assert_eq!(sign(&"k".repeat(100), "x", 1, "/file/3"), "76bd60e4cbc61b20cf0db821cfe995debe0e51707c70c3c92ab9a4e7a3bdcdc3");
    }

    #[test]
    fn a_signed_request_is_accepted_once() {
        let mut s = Sessions::new();
        let id = s.open();
        assert_eq!(id.len(), 32);
        let m = sign("phone-key", &id, 1, "/ws");
        assert_eq!(s.check("phone-key", &id, 1, &m, "/ws", false), Verdict::Ok);
        // the same request recorded and sent again
        assert_eq!(s.check("phone-key", &id, 1, &m, "/ws", false), Verdict::Refused);
        // counters may arrive out of order (requests run side by side)
        let (m3, m2) = (sign("phone-key", &id, 3, "/ws"), sign("phone-key", &id, 2, "/ws"));
        assert_eq!(s.check("phone-key", &id, 3, &m3, "/ws", false), Verdict::Ok);
        assert_eq!(s.check("phone-key", &id, 2, &m2, "/ws", false), Verdict::Ok);
    }

    #[test]
    fn wrong_key_path_or_session_is_refused() {
        let mut s = Sessions::new();
        let id = s.open();
        assert_eq!(s.check("phone-key", &id, 1, &sign("old-key", &id, 1, "/ws"), "/ws", false), Verdict::Refused);
        // a signature for one place doesn't open another
        assert_eq!(s.check("phone-key", &id, 2, &sign("phone-key", &id, 2, "/art"), "/ws", false), Verdict::Refused);
        assert_eq!(s.check("phone-key", &id, 3, "", "/ws", false), Verdict::Refused);
        assert_eq!(s.check("phone-key", "nosuchsession", 1, &sign("phone-key", "nosuchsession", 1, "/ws"), "/ws", false), Verdict::NoSession);
        // no key set: nothing is accepted
        assert_eq!(s.check("", &id, 4, &sign("", &id, 4, "/ws"), "/ws", false), Verdict::Refused);
        assert_eq!(s.check("phone-key", &id, 0, &sign("phone-key", &id, 0, "/ws"), "/ws", false), Verdict::Refused);
    }

    #[test]
    fn plain_reads_may_repeat() {
        let mut s = Sessions::new();
        let id = s.open();
        let m = sign("k", &id, 5, "/art");
        assert_eq!(s.check("k", &id, 5, &m, "/art", true), Verdict::Ok);
        assert_eq!(s.check("k", &id, 5, &m, "/art", true), Verdict::Ok);
        // but its counter is still spent for anything else
        assert_eq!(s.check("k", &id, 5, &sign("k", &id, 5, "/ws"), "/ws", false), Verdict::Refused);
    }

    #[test]
    fn old_counters_and_sessions_are_let_go() {
        let mut s = Sessions::new();
        let id = s.open();
        let far = WINDOW + 10;
        assert_eq!(s.check("k", &id, far, &sign("k", &id, far, "/ws"), "/ws", false), Verdict::Ok);
        // far behind the newest: refused even though it was never used
        assert_eq!(s.check("k", &id, 1, &sign("k", &id, 1, "/ws"), "/ws", false), Verdict::Refused);
        let first = id.clone();
        for _ in 0..MAX_SESSIONS {
            s.open();
        }
        assert_eq!(s.list.len(), MAX_SESSIONS);
        assert_eq!(s.check("k", &first, far + 1, &sign("k", &first, far + 1, "/ws"), "/ws", false), Verdict::NoSession);
    }
}

#[cfg(test)]
mod page_tests {
    /// Only this PC's own names reach the phone server: an IP address, a single-word computer name,
    /// or a home/Tailscale name. A web page that points its own internet name at this PC (DNS
    /// rebinding) is refused.
    #[test]
    fn hosts_are_this_pcs_names_only() {
        for ok in ["192.168.1.20:47830", "100.101.102.103:47830", "127.0.0.1", "[fe80::1]:47830", "localhost:47830", "GAMINGPC:47830", "gamingpc.local", "pc.lan:1", "pc.home.arpa", "pc.tail1234.ts.net:47830"] {
            assert!(super::host_ok(ok), "{ok}");
        }
        for bad in ["evil.example.com:47830", "attacker.com", "192.168.1.20.nip.io", "", "x.ts.net.evil.com", "a b"] {
            assert!(!super::host_ok(bad), "{bad}");
        }
    }

    /// The page may run only its own inline scripts (by their hash), connect only back to Deck, and
    /// load nothing from elsewhere.
    #[test]
    fn csp_allows_exactly_the_pages_own_scripts() {
        let html = "<p>x</p><script>alert(1)</script><script src=\"/a.js\"></script>\n<script>\nlet a = 2;\n</script>";
        let csp = super::csp_for(html);
        use sha2::Digest;
        for body in ["alert(1)", "\nlet a = 2;\n"] {
            let h = crate::util::base64(&sha2::Sha256::digest(body.as_bytes()));
            assert!(csp.contains(&format!("'sha256-{h}'")), "{csp}");
        }
        assert_eq!(csp.matches("'sha256-").count(), 2);
        for part in ["default-src 'none'", "script-src 'self' ", "connect-src 'self' ws: wss:", "frame-ancestors 'none'", "base-uri 'none'", "form-action 'none'"] {
            assert!(csp.contains(part), "{part} missing from {csp}");
        }
        assert!(!csp.contains("unsafe-eval") && !csp.contains("script-src 'self' 'unsafe-inline'"));
    }

    /// Browsers hash a script's text after turning CRLF (and a lone CR) into LF, and the page file
    /// has CRLF on a Windows checkout: the hash must be of the browser's text, or the page won't run.
    #[test]
    fn csp_hashes_scripts_as_the_browser_reads_them() {
        use sha2::Digest;
        let lf = crate::util::base64(&sha2::Sha256::digest(b"\nlet a = 1;\nlet b = 2;\n"));
        assert!(super::csp_for("<script>\r\nlet a = 1;\rlet b = 2;\r\n</script>").contains(&lf));
    }
}
