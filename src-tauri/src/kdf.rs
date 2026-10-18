//! Keys from codes a person may have typed (the PC to PC pairing code, the Claude Code sync key).
//! The PCs broadcast a short fingerprint of the key on the network, so a weak typed code could be
//! guessed offline at the speed of one hash per guess. `stretch` makes every guess cost a
//! deliberately slow, memory-hungry computation instead.

use parking_lot::Mutex;

/// 32-byte blocks held per derivation: 32 MiB, so guessing in parallel needs that much memory per guess.
const MEMORY_BLOCKS: usize = 1 << 20;

/// The key for `secret` under `context`: scrypt's ROMix with BLAKE3 as its hash (fill 32 MiB with
/// a hash chain, then walk it in an order that depends on what was read). About a quarter of a
/// second; remembered per code for the rest of the run, since the PCs derive it often.
pub fn stretch(context: &str, secret: &str) -> [u8; 32] {
    static CACHE: Mutex<Vec<([u8; 32], [u8; 32])>> = parking_lot::const_mutex(Vec::new());
    let seed = blake3::derive_key(context, secret.as_bytes());
    if let Some((_, k)) = CACHE.lock().iter().find(|(s, _)| *s == seed) {
        return *k;
    }
    let key = blake3::derive_key(context, &romix(seed, MEMORY_BLOCKS));
    let mut c = CACHE.lock();
    if c.len() >= 8 {
        c.remove(0);
    }
    c.push((seed, key));
    key
}

fn romix(seed: [u8; 32], n: usize) -> [u8; 32] {
    let mut v = vec![[0u8; 32]; n];
    let mut x = seed;
    for b in v.iter_mut() {
        *b = x;
        x = *blake3::hash(&x).as_bytes();
    }
    for _ in 0..n {
        let j = (u64::from_le_bytes(x[..8].try_into().unwrap()) % n as u64) as usize;
        let mut m = x;
        for (a, b) in m.iter_mut().zip(v[j].iter()) {
            *a ^= b;
        }
        x = *blake3::hash(&m).as_bytes();
    }
    x
}

#[cfg(test)]
mod tests {
    #[test]
    fn stretching_is_deterministic_and_separates_inputs() {
        let a = super::stretch("ctx one", "K7QMXRPA9DF7HWTZC4N2");
        assert_eq!(a, super::stretch("ctx one", "K7QMXRPA9DF7HWTZC4N2"));
        assert_ne!(a, super::stretch("ctx two", "K7QMXRPA9DF7HWTZC4N2"));
        assert_ne!(a, super::stretch("ctx one", "K7QMXRPA9DF7HWTZC4N3"));
    }

    /// Not the plain one-hash derivation a guesser could run billions of times a second.
    #[test]
    fn stretching_is_not_a_single_hash() {
        assert_ne!(super::stretch("ctx", "password"), blake3::derive_key("ctx", b"password"));
        const _: () = assert!(super::MEMORY_BLOCKS * 32 >= 32 << 20, "at least 32 MiB per guess");
    }

    /// The value is part of the protocol between PCs: it must never change by accident.
    #[test]
    fn stretching_is_stable() {
        assert_eq!(crate::beam::crypto::hex(&super::stretch("Deck test", "ABCD")), FROZEN);
    }

    const FROZEN: &str = "4ba0bd3caf0200794cdcb6c9c1d32042704cce7d302cd341e9cd91fc6bd848b1";
}
