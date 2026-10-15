//! Sync key handling: everything on the wire and in the cloud folder is ChaCha20-Poly1305 encrypted
//! with keys derived from the shared sync key, so only machines that know it can read or join.
use chacha20poly1305::aead::Aead;
use chacha20poly1305::{ChaCha20Poly1305, KeyInit, Nonce};
use std::io::{Read, Write};

#[derive(Clone)]
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
pub fn gen_sync_key() -> String {
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

impl Key {
    pub fn new(sync_key: &str) -> Key {
        // case/space/dash-insensitive so it's easy to type on the other machine
        let norm: String = sync_key.chars().filter(|c| c.is_ascii_alphanumeric()).map(|c| c.to_ascii_uppercase()).collect();
        Key { master: blake3::derive_key("Deck Claude Sync 2026 v1 master", norm.as_bytes()) }
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

    /// short public id of the key, so machines with different keys ignore each other's beacons
    pub fn group(&self) -> String {
        hex(&self.sub("group")[..8])
    }

    /// cloud-folder file name for a chunk hash (doesn't reveal the content hash)
    pub fn blob_name(&self, hash: &str) -> String {
        hex(&self.mac("blobname", &[hash.as_bytes()])[..20])
    }

    pub fn seal(&self, data: &[u8]) -> Vec<u8> {
        let c = ChaCha20Poly1305::new(&self.sub("seal").into());
        let n = random::<12>();
        let mut out = n.to_vec();
        out.extend(c.encrypt(Nonce::from_slice(&n), data).expect("encrypt"));
        out
    }

    pub fn open(&self, data: &[u8]) -> Option<Vec<u8>> {
        if data.len() < 28 {
            return None;
        }
        let c = ChaCha20Poly1305::new(&self.sub("seal").into());
        c.decrypt(Nonce::from_slice(&data[..12]), &data[12..]).ok()
    }
}

pub fn deflate(d: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::DeflateEncoder::new(Vec::with_capacity(d.len() / 4 + 64), flate2::Compression::new(5));
    e.write_all(d).expect("deflate");
    e.finish().expect("deflate")
}

pub fn inflate(d: &[u8], max: usize) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    flate2::read::DeflateDecoder::new(d).take(max as u64 + 1).read_to_end(&mut out).ok()?;
    (out.len() <= max).then_some(out)
}

pub fn hash(d: &[u8]) -> String {
    blake3::hash(d).to_hex().to_string()
}
