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

/// The letters and digits codes are made of (no 0/O, 1/I/L: easy to read out and type).
const CODE_CHARS: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";

/// A new pairing code or sync key: 20 characters (about 99 bits) in groups of four, each one
/// equally likely, and never one that `too_easy` would turn down on the other PC.
pub fn gen_code() -> String {
    loop {
        let mut picked = Vec::with_capacity(20);
        while picked.len() < 20 {
            let mut b = [0u8; 32];
            getrandom::fill(&mut b).expect("the system's random number generator failed");
            // 248 = 8 x 31: bytes above it would make the first letters more likely
            picked.extend(b.iter().filter(|&&x| x < 248).map(|&x| CODE_CHARS[x as usize % CODE_CHARS.len()] as char));
        }
        let code = picked[..20].chunks(4).map(|c| c.iter().collect::<String>()).collect::<Vec<_>>().join("-");
        if too_easy(&code).is_none() {
            return code;
        }
    }
}

/// Why a typed code is too easy to guess, if it is. The PCs' beacons carry a fingerprint of the
/// code, so anyone on the network can try guesses offline (each costs a `stretch`); a code made of
/// repeats, runs like 1234 or ABCD, or few different characters falls to a short list of guesses.
pub fn too_easy(code: &str) -> Option<&'static str> {
    let c: Vec<u8> = code.bytes().filter(u8::is_ascii_alphanumeric).map(|b| b.to_ascii_uppercase()).collect();
    if c.len() < 16 {
        return Some("Use at least 16 letters and digits");
    }
    const EASY: &str = "That code is too easy to guess: use the code from your other PC, or make a new one";
    let distinct = c.iter().collect::<std::collections::BTreeSet<_>>().len();
    let run = c.windows(4).any(|w| w.windows(2).all(|p| p[1] as i16 - p[0] as i16 == 1) || w.windows(2).all(|p| p[0] as i16 - p[1] as i16 == 1));
    let repeat = c.windows(4).enumerate().any(|(i, w)| c[i + 1..].windows(4).any(|x| x == w));
    (distinct < 10 || run || repeat).then_some(EASY)
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

    /// Typed codes a short guess list would find are turned down; made ones never are.
    #[test]
    fn easy_codes_are_turned_down() {
        use super::too_easy;
        for easy in ["AAAA-AAAA-AAAA-AAAA-AAAA", "PASS-WORD-PASS-WORD-1234", "ABCD-EFGH-JKMN-PQRS-TUVW", "1234-5678-9ABC-DEFG-HJKM", "my-key-12", "QWER-TYQW-ERTY-QWER-TYXX"] {
            assert!(too_easy(easy).is_some(), "{easy}");
        }
        assert!(too_easy("K7QD-M2XH-9RFA-WN4T-3BJP").is_none());
        for _ in 0..500 {
            let c = super::gen_code();
            assert!(too_easy(&c).is_none(), "{c}");
            assert_eq!(c.len(), 24);
        }
    }
}
