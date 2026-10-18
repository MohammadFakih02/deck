//! Pairing code handling: every connection proves knowledge of the code and is then encrypted
//! with ChaCha20-Poly1305 using per-connection keys derived from it.
pub struct Key {
    master: [u8; 32],
}

pub fn random<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    getrandom::fill(&mut b).expect("os rng");
    b
}

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// e.g. `K7QM-2XRP-A9DF-7HWT-ZC4N` (100 bits, no look-alike characters)
pub fn gen_code() -> String {
    const A: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";
    let r = random::<20>();
    let mut s = String::new();
    for (i, b) in r.iter().enumerate() {
        if i > 0 && i % 4 == 0 {
            s.push('-');
        }
        s.push(A[*b as usize % A.len()] as char);
    }
    s
}

/// case/space/dash-insensitive, so it's easy to type on the other PC
pub fn normalize(code: &str) -> String {
    code.chars().filter(|c| c.is_ascii_alphanumeric()).map(|c| c.to_ascii_uppercase()).collect()
}

impl Key {
    pub fn new(code: &str) -> Key {
        // slow on purpose: a typed code could otherwise be guessed from the beacons' fingerprint (kdf.rs)
        Key { master: crate::kdf::stretch("AppBeam 2026 v2 master", &normalize(code)) }
    }

    pub fn sub(&self, ctx: &str) -> [u8; 32] {
        *blake3::keyed_hash(&self.master, ctx.as_bytes()).as_bytes()
    }

    pub fn mac(&self, ctx: &str, parts: &[&[u8]]) -> [u8; 32] {
        let mut h = blake3::Hasher::new_keyed(&self.sub(ctx));
        for p in parts {
            h.update(p);
        }
        *h.finalize().as_bytes()
    }

    /// short public id of the code, so PCs with different codes ignore each other's beacons
    pub fn group(&self) -> String {
        hex(&self.sub("group")[..8])
    }
}
