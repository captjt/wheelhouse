//! The hub's core: the store, the keys, the mirror of Rex, and the agents it
//! tracks, behind one lock. Every entry goes through `Core::append`, which
//! signs it as its actor, chains it, and folds it into the docket's state.

use anyhow::Result;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, MutexGuard};
use tokio::sync::broadcast;

use crate::agents::Tracker;
use crate::mirror::Mirror;
use crate::record::entry::Actor;
use crate::record::{DocketState, Entry, Envelope, Keyring, Stored, verify};
use crate::store::Store;

pub struct Core {
    pub store: Store,
    pub keys: Keyring,
    pub mirror: Mirror,
    pub agents: Tracker,
    pub events: broadcast::Sender<Value>,
    pub owner: String,
}

#[derive(Clone)]
pub struct Shared(Arc<Mutex<Core>>);

impl Shared {
    pub fn new(core: Core) -> Self {
        Shared(Arc::new(Mutex::new(core)))
    }

    pub fn lock(&self) -> MutexGuard<'_, Core> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// Who an entry is from. The hub works out the actor and the key.
#[derive(Clone, Debug)]
pub enum Who {
    /// What the hub saw.
    Hub,
    /// You.
    Owner,
    /// An agent, speaking through `wh` from its pane.
    Agent { app: String, block: String, session: Option<String> },
}

impl Core {
    pub fn new(store: Store, keys: Keyring, owner: &str) -> Result<Self> {
        let (events, _) = broadcast::channel(256);
        let mut core =
            Core { store, keys, mirror: Mirror::default(), agents: Tracker::default(), events, owner: owner.to_string() };
        for name in ["owner", "hub"] {
            core.keys.get_or_create(name)?;
        }
        let at = crate::time::now_ms();
        let known: Vec<(String, String)> = core.keys.iter().map(|k| (k.keyid(), k.name.clone())).collect();
        for (id, name) in known {
            core.store.put_key(&id, &name, &at)?;
        }
        Ok(core)
    }

    fn actor(&self, who: &Who) -> (Actor, String) {
        match who {
            Who::Hub => (Actor::new("hub", "hub@local"), "hub".into()),
            Who::Owner => (Actor::new("person", &self.owner), "owner".into()),
            Who::Agent { app, block, session } => (
                Actor::new("agent", &format!("{app}@local")).with("block", block.as_str()).with("session", session.clone()),
                format!("agent-{app}@local"),
            ),
        }
    }

    /// Sign, chain, store and fold one entry.
    pub fn append(&mut self, docket: &str, kind: &str, who: &Who, body: Option<String>, data: Value) -> Result<Stored> {
        let (actor, key_name) = self.actor(who);
        let at = crate::time::now_ms();
        let mut e = Entry::new(docket, kind, &at, actor);
        e.body = body;
        e.data = data;
        let (envelope, keyid, new_key) = {
            let (key, new) = self.keys.get_or_create(&key_name)?;
            (Envelope::sign(&e.to_bytes(), key).to_string_compact(), key.keyid(), new)
        };
        if new_key {
            self.store.put_key(&keyid, &key_name, &at)?;
        }
        let stored = self.store.append(docket, &envelope, kind, &at, &keyid)?;
        let mut st = self.store.state(docket)?.unwrap_or_else(|| DocketState::new(docket));
        st.apply(stored.seq, &stored.chain, &e);
        self.store.save_state(&st)?;
        let _ = self.events.send(json!({ "type": "docket", "docket": docket }));
        Ok(stored)
    }

    /// Open a new docket with its first entry.
    pub fn open(&mut self, who: &Who, title: &str, data: Value) -> Result<String> {
        let (id, _) = self.store.next_docket()?;
        self.append(&id, "open", who, Some(title.to_string()), data)?;
        Ok(id)
    }

    /// Something live changed (an agent's state), not the record.
    pub fn live_changed(&self) {
        let _ = self.events.send(json!({ "type": "live" }));
    }

    pub fn verify(&self, docket: &str) -> Result<verify::Report> {
        let known: BTreeSet<String> = self.store.keys()?.into_iter().map(|(id, _)| id).collect();
        let entries = self.store.entries(docket)?;
        Ok(verify::verify(docket, &entries, |k| known.contains(k)))
    }

    /// Fold every docket again from its entries, and say which saved states
    /// didn't match (none should).
    #[cfg(test)]
    pub fn rebuild_check(&self) -> Result<Vec<String>> {
        let mut differ = Vec::new();
        for id in self.store.docket_ids()? {
            let entries = self.store.entries(&id)?;
            let parsed: Vec<Entry> = entries.iter().map(|s| s.entry()).collect::<Result<_>>()?;
            let st = crate::record::fold::fold(&id, entries.iter().zip(&parsed).map(|(s, e)| (s.seq, s.chain.as_str(), e)));
            if Some(st) != self.store.state(&id)? {
                differ.push(id);
            }
        }
        Ok(differ)
    }
}

#[cfg(test)]
pub fn test_core() -> Core {
    Core::new(Store::in_memory().unwrap(), Keyring::in_memory(), "you").unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appended_entries_verify_and_fold() {
        let mut core = test_core();
        let id = core.open(&Who::Owner, "Retry failed webhooks", json!({})).unwrap();
        core.append(&id, "note", &Who::Owner, Some("cap it at 3".into()), Value::Null).unwrap();
        core.append(&id, "set", &Who::Owner, None, json!({ "status": "done" })).unwrap();
        let r = core.verify(&id).unwrap();
        assert!(r.ok, "{:?}", r.problems);
        let st = core.store.state(&id).unwrap().unwrap();
        assert_eq!(st.title, "Retry failed webhooks");
        assert_eq!(st.status, "done");
        assert_eq!(Some(st.head.clone()), r.head);
        assert!(core.rebuild_check().unwrap().is_empty());
    }

    #[test]
    fn each_actor_signs_with_its_own_key() {
        let mut core = test_core();
        let id = core.open(&Who::Hub, "Claude Code in ~/git/api", json!({ "protocol": "agent" })).unwrap();
        core.append(&id, "note", &Who::Owner, Some("fix the test".into()), json!({ "via": "prompt" })).unwrap();
        let agent = Who::Agent { app: "claude-code".into(), block: "block:1".into(), session: Some("s1".into()) };
        core.append(&id, "note", &agent, Some("done".into()), Value::Null).unwrap();
        let keys: Vec<Option<String>> = core.store.entries(&id).unwrap().iter().map(|s| s.keyid()).collect();
        assert_eq!(keys.len(), 3);
        assert!(keys[0] != keys[1] && keys[1] != keys[2] && keys[0] != keys[2]);
        let named: Vec<String> = core.store.keys().unwrap().into_iter().map(|(_, n)| n).collect();
        assert!(named.contains(&"agent-claude-code@local".to_string()), "{named:?}");
        assert!(core.verify(&id).unwrap().ok);
    }
}
