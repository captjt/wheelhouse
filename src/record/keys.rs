//! Ed25519 keys. In v1 the hub holds every key on its machine, each as a
//! 32-byte seed in `keys/<name>.seed` (hex), readable only by you.

use anyhow::{Context, Result, anyhow};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::hex;

pub struct Key {
    pub name: String,
    pair: ed25519_compact::KeyPair,
}

impl Key {
    #[cfg(test)]
    pub fn generate(name: &str) -> Self {
        Key { name: name.to_string(), pair: ed25519_compact::KeyPair::from_seed(ed25519_compact::Seed::generate()) }
    }

    /// The key a 32-byte seed makes: an invite's key is made this way, from
    /// the secret in its code.
    pub fn from_seed(name: &str, seed: &[u8]) -> Result<Self> {
        let seed = ed25519_compact::Seed::from_slice(seed).map_err(|e| anyhow!("bad seed for {name}: {e}"))?;
        Ok(Key { name: name.to_string(), pair: ed25519_compact::KeyPair::from_seed(seed) })
    }

    pub fn keyid(&self) -> String {
        format!("ed25519:{}", hex::encode(self.pair.pk.as_ref()))
    }

    /// A plain Ed25519 signature: deterministic, no added noise.
    pub fn sign(&self, msg: &[u8]) -> Vec<u8> {
        self.pair.sk.sign(msg, None).to_vec()
    }
}

/// 32 random bytes, from the system's source.
pub fn random32() -> [u8; 32] {
    let seed = ed25519_compact::Seed::generate();
    let mut out = [0u8; 32];
    out.copy_from_slice(seed.as_ref());
    out
}

/// Check a plain Ed25519 signature by the key a key id names.
pub fn verify_by(keyid: &str, msg: &[u8], sig: &[u8]) -> bool {
    let Some(pk) = public_from_keyid(keyid) else { return false };
    let Ok(sig) = ed25519_compact::Signature::from_slice(sig) else { return false };
    pk.verify(msg, &sig).is_ok()
}

/// The public key a key id names.
pub fn public_from_keyid(keyid: &str) -> Option<ed25519_compact::PublicKey> {
    let bytes = hex::decode(keyid.strip_prefix("ed25519:")?)?;
    ed25519_compact::PublicKey::from_slice(&bytes).ok()
}

/// The hub's keys, by name, loaded from (and saved to) a folder.
pub struct Keyring {
    dir: Option<PathBuf>,
    keys: BTreeMap<String, Key>,
}

impl Keyring {
    pub fn open(dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        set_mode(dir, 0o700);
        let mut keys = BTreeMap::new();
        for e in std::fs::read_dir(dir)? {
            let path = e?.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()).and_then(|n| n.strip_suffix(".seed")) else { continue };
            let text = std::fs::read_to_string(&path)?;
            let seed = hex::decode(text.trim()).ok_or_else(|| anyhow!("{} isn't hex", path.display()))?;
            keys.insert(name.to_string(), Key::from_seed(name, &seed)?);
        }
        Ok(Keyring { dir: Some(dir.to_path_buf()), keys })
    }

    /// Keys that live only in memory, for tests.
    #[cfg(test)]
    pub fn in_memory() -> Self {
        Keyring { dir: None, keys: BTreeMap::new() }
    }

    /// The key named `name`, made (and saved) the first time it's asked for.
    /// The bool says whether it's new.
    pub fn get_or_create(&mut self, name: &str) -> Result<(&Key, bool)> {
        let mut new = false;
        if !self.keys.contains_key(name) {
            let seed = ed25519_compact::Seed::generate();
            if let Some(dir) = &self.dir {
                let path = dir.join(format!("{name}.seed"));
                std::fs::write(&path, format!("{}\n", hex::encode(seed.as_ref()))).with_context(|| format!("writing {}", path.display()))?;
                set_mode(&path, 0o600);
            }
            self.keys.insert(name.to_string(), Key::from_seed(name, seed.as_ref())?);
            new = true;
        }
        Ok((&self.keys[name], new))
    }

    pub fn iter(&self) -> impl Iterator<Item = &Key> {
        self.keys.values()
    }
}

fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode));
}
