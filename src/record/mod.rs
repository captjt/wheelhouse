//! The record: signed, chained entries, and the docket state they fold to.
//! `docs/record.md` is the contract; this module is one implementation.

pub mod chain;
pub mod entry;
pub mod envelope;
pub mod fold;
pub mod hex;
pub mod keys;
pub mod verify;

pub use entry::Entry;
pub use envelope::Envelope;
pub use fold::DocketState;
#[cfg(test)]
pub use keys::Key;
pub use keys::Keyring;

/// One entry as the store keeps it: its place in the docket, its id and
/// chain value, and the envelope's exact bytes.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Stored {
    pub docket: String,
    pub seq: i64,
    /// `sha256:<hex>` of `envelope`.
    pub id: String,
    /// Hex of the chain value after this entry.
    pub chain: String,
    pub envelope: String,
}

impl Stored {
    pub fn entry(&self) -> anyhow::Result<Entry> {
        let env: Envelope = serde_json::from_str(&self.envelope)?;
        Ok(serde_json::from_slice(&env.payload_bytes()?)?)
    }

    pub fn keyid(&self) -> Option<String> {
        let env: Envelope = serde_json::from_str(&self.envelope).ok()?;
        env.signatures.first().map(|s| s.keyid.clone())
    }
}

/// How an entry is known.
pub fn mark(entry: &Entry) -> &'static str {
    match entry.actor.typ.as_str() {
        "hub" if matches!(entry.kind.as_str(), "link") => "checkable",
        "hub" => "witnessed",
        _ => "said",
    }
}
