//! Each PC's own key (Ed25519), so PCs that share a pairing code are still told apart: a PC's id
//! is derived from its key, and every connection is signed with it. "Trusted" and "always allow"
//! name an id, so they stay with that one PC; another device with the same pairing code can't take
//! its id without its key.

use crate::beam::crypto::hex;
use ring::signature::{Ed25519KeyPair, KeyPair, UnparsedPublicKey, ED25519};
use serde_json::{json, Value};
use std::path::Path;

pub struct Device {
    pair: Ed25519KeyPair,
}

/// A PC's id from its public key.
pub fn id_of(pk: &[u8]) -> String {
    hex(&blake3::derive_key("Deck PC to PC device id v1", pk)[..16])
}

/// What a hello signs: this connection (both sides' random values) and which side it is, so it
/// can't be used on another connection or sent back as the other side's.
fn transcript(nc: &[u8], ns: &[u8], client: bool) -> Vec<u8> {
    let mut t = b"Deck PC to PC hello v1".to_vec();
    t.extend_from_slice(nc);
    t.extend_from_slice(ns);
    t.push(if client { b'c' } else { b's' });
    t
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    s.len().is_multiple_of(2).then(|| (0..s.len()).step_by(2).map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok()).collect::<Option<Vec<u8>>>()).flatten()
}

impl Device {
    /// This PC's key from `dir` (made the first time; a damaged file is replaced, which gives this
    /// PC a new id that the other PCs have to trust again).
    pub fn load_or_create(dir: &Path) -> Device {
        let file = dir.join("device.key");
        if let Some(pair) = std::fs::read(&file).ok().and_then(|b| Ed25519KeyPair::from_pkcs8(&b).ok()) {
            return Device { pair };
        }
        let rng = ring::rand::SystemRandom::new();
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&rng).expect("the system's random number generator failed");
        let _ = std::fs::create_dir_all(dir);
        let _ = crate::util::write_atomic(&file, pkcs8.as_ref());
        Device { pair: Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).expect("a key just made") }
    }

    /// The id other PCs know this one by (a hash of its public key).
    pub fn id(&self) -> String {
        id_of(self.pair.public_key().as_ref())
    }

    /// This PC's part of the hello: its id, name, public key and its signature over this connection.
    pub fn hello(&self, name: &str, nc: &[u8], ns: &[u8], client: bool) -> Value {
        let sig = self.pair.sign(&transcript(nc, ns, client));
        json!({ "id": self.id(), "name": name, "v": 3, "pk": hex(self.pair.public_key().as_ref()), "sig": hex(sig.as_ref()) })
    }
}

/// The other PC's hello checked: its key signed this connection (from its side), and its id is
/// that key's. Returns its (id, name).
pub fn check_hello(v: &Value, nc: &[u8], ns: &[u8], from_client: bool) -> Result<(String, String), &'static str> {
    const OLD: &str = "the other PC runs an older Deck: update Deck there";
    let pk = v["pk"].as_str().and_then(unhex).ok_or(OLD)?;
    let sig = v["sig"].as_str().and_then(unhex).ok_or(OLD)?;
    UnparsedPublicKey::new(&ED25519, &pk).verify(&transcript(nc, ns, from_client), &sig).map_err(|_| "the other PC couldn't prove which PC it is")?;
    let id = v["id"].as_str().unwrap_or("");
    if id != id_of(&pk) {
        return Err("the other PC gave an id that isn't its own");
    }
    Ok((id.to_string(), v["name"].as_str().unwrap_or("?").to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("deck-device-{name}-{}", crate::util::now_ms()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_pc_keeps_its_key_and_id() {
        let d = tmp("keep");
        let a = Device::load_or_create(&d);
        assert_eq!(a.id().len(), 32);
        assert_eq!(Device::load_or_create(&d).id(), a.id(), "the same id after a restart");
        let other = tmp("other");
        assert_ne!(Device::load_or_create(&other).id(), a.id());
        let _ = (std::fs::remove_dir_all(&d), std::fs::remove_dir_all(&other));
    }

    #[test]
    fn a_signed_hello_names_its_pc() {
        let d = tmp("hello");
        let a = Device::load_or_create(&d);
        let (nc, ns) = ([1u8; 16], [2u8; 16]);
        let h = a.hello("DESKTOP", &nc, &ns, true);
        assert_eq!(check_hello(&h, &nc, &ns, true), Ok((a.id(), "DESKTOP".to_string())));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Another device with the same pairing code can't pass for this PC: claiming its id without
    /// its key, replaying its hello on another connection, or turning a client's hello around.
    #[test]
    fn a_pc_cant_be_impersonated() {
        let (da, dm) = (tmp("a"), tmp("m"));
        let (a, mallory) = (Device::load_or_create(&da), Device::load_or_create(&dm));
        let (nc, ns) = ([1u8; 16], [2u8; 16]);
        let mut claim = mallory.hello("DESKTOP", &nc, &ns, true);
        claim["id"] = Value::String(a.id());
        assert!(check_hello(&claim, &nc, &ns, true).is_err(), "someone else's id");
        let recorded = a.hello("DESKTOP", &nc, &ns, true);
        assert!(check_hello(&recorded, &[9u8; 16], &ns, true).is_err(), "replayed on another connection");
        assert!(check_hello(&recorded, &nc, &ns, false).is_err(), "a client's hello sent back as the server's");
        let mut unsigned = recorded.clone();
        unsigned.as_object_mut().unwrap().remove("sig");
        assert!(check_hello(&unsigned, &nc, &ns, true).is_err(), "no signature (an older Deck)");
        let _ = (std::fs::remove_dir_all(&da), std::fs::remove_dir_all(&dm));
    }
}
