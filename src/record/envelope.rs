//! DSSE envelopes: https://github.com/secure-systems-lab/dsse

use anyhow::{Context, Result, anyhow, bail};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use serde::{Deserialize, Serialize};

use super::keys::{Key, public_from_keyid};

pub const PAYLOAD_TYPE: &str = "application/vnd.wheelhouse.entry+json";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Envelope {
    pub payload_type: String,
    pub payload: String,
    pub signatures: Vec<Signature>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Signature {
    pub keyid: String,
    pub sig: String,
}

/// DSSE's pre-authentication encoding: what is actually signed.
pub fn pae(payload_type: &str, payload: &[u8]) -> Vec<u8> {
    let mut out = format!("DSSEv1 {} {} {} ", payload_type.len(), payload_type, payload.len()).into_bytes();
    out.extend_from_slice(payload);
    out
}

impl Envelope {
    /// Sign `payload` (an entry's exact bytes) with `key`.
    pub fn sign(payload: &[u8], key: &Key) -> Envelope {
        let sig = key.sign(&pae(PAYLOAD_TYPE, payload));
        Envelope { payload_type: PAYLOAD_TYPE.to_string(), payload: B64.encode(payload), signatures: vec![Signature { keyid: key.keyid(), sig: B64.encode(sig) }] }
    }

    pub fn payload_bytes(&self) -> Result<Vec<u8>> {
        B64.decode(&self.payload).context("payload isn't base64")
    }

    /// Check the one signature. `known` says whether a key id belongs to
    /// this record; the key itself is in the id.
    pub fn verify(&self, known: impl Fn(&str) -> bool) -> Result<String> {
        if self.payload_type != PAYLOAD_TYPE {
            bail!("payload type is {}, not {PAYLOAD_TYPE}", self.payload_type);
        }
        let [s] = self.signatures.as_slice() else { bail!("{} signatures; v1 has exactly one", self.signatures.len()) };
        if !known(&s.keyid) {
            bail!("signed by {}, which this record doesn't know", s.keyid);
        }
        let pk = public_from_keyid(&s.keyid).ok_or_else(|| anyhow!("key id {} isn't an Ed25519 key", s.keyid))?;
        let sig = B64.decode(&s.sig).context("signature isn't base64")?;
        let sig = ed25519_compact::Signature::from_slice(&sig).map_err(|e| anyhow!("bad signature: {e}"))?;
        pk.verify(pae(&self.payload_type, &self.payload_bytes()?), &sig).map_err(|_| anyhow!("signature doesn't match"))?;
        Ok(s.keyid.clone())
    }

    /// The envelope's bytes, as stored. Made once per entry.
    pub fn to_string_compact(&self) -> String {
        serde_json::to_string(self).expect("an envelope always serializes")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pae_matches_the_spec() {
        // DSSE's own example: PAE("http://example.com/HelloWorld", "hello world")
        let got = pae("http://example.com/HelloWorld", b"hello world");
        assert_eq!(got, b"DSSEv1 29 http://example.com/HelloWorld 11 hello world".to_vec());
    }

    #[test]
    fn sign_and_verify() {
        let key = Key::generate("owner");
        let env = Envelope::sign(br#"{"v":1}"#, &key);
        let id = key.keyid();
        assert_eq!(env.verify(|k| k == id).unwrap(), id);
        assert!(env.verify(|_| false).is_err(), "an unknown key fails");

        let mut bad = env.clone();
        bad.payload = B64.encode(br#"{"v":2}"#);
        assert!(bad.verify(|k| k == id).is_err(), "a changed payload fails");

        let other = Key::generate("other");
        let mut swapped = env.clone();
        swapped.signatures[0].keyid = other.keyid();
        assert!(swapped.verify(|_| true).is_err(), "another key's id fails");
    }
}
