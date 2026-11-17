use sha2::{Digest, Sha256};

use super::hex;

/// An entry's id: sha256 of its envelope's exact bytes.
pub fn entry_id(envelope: &[u8]) -> [u8; 32] {
    Sha256::digest(envelope).into()
}

pub fn id_string(id: &[u8; 32]) -> String {
    format!("sha256:{}", hex::encode(id))
}

#[allow(dead_code)]
pub fn parse_id(s: &str) -> Option<[u8; 32]> {
    hex::decode(s.strip_prefix("sha256:")?)?.try_into().ok()
}

/// The chain value before the first entry.
pub const GENESIS: [u8; 32] = [0; 32];

/// `chain(n) = sha256(chain(n-1) ‖ id(n))`.
pub fn next(prev: &[u8; 32], id: &[u8; 32]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(prev);
    h.update(id);
    h.finalize().into()
}

pub fn parse_chain(s: &str) -> Option<[u8; 32]> {
    hex::decode(s)?.try_into().ok()
}
