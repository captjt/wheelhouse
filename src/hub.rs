//! The hub's core: the store, the keys, the mirror of Rex, and the agents it
//! tracks, behind one lock. Every entry goes through `Core::append`, which
//! signs it as its actor, chains it, and folds it into the docket's state.

use anyhow::Result;
use serde_json::{Value, json};
use sha2::Digest;
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
    /// The Mac's Tailscale user: the only one the tailnet listener answers.
    pub tailnet_owner: Option<String>,
    pub tailnet_host: Option<String>,
    pub started: std::time::Instant,
    /// Whether a Linear key is set up (checked at start and every few minutes).
    pub linear_ready: bool,
    /// Previews up now, by their tailnet port.
    pub previews: std::collections::BTreeMap<u16, crate::preview::Live>,
    /// The Review protocol's live state.
    pub review: crate::review::Hub,
    /// Build's live state.
    pub build: crate::build::Hub,
    /// Something live changed since the last `live` event went out: events
    /// go out at most a few times a second (`run_live`), not once per change.
    pub live_dirty: Arc<std::sync::atomic::AtomicBool>,
    /// Wheelhouse.app windows following the events: while one is, the app
    /// posts the Mac's notifications (with their actions), not the hub.
    pub mac_apps: Arc<std::sync::atomic::AtomicUsize>,
    /// Each docket's last quick check, by its head: a record that hasn't
    /// moved isn't checked again.
    verified: std::collections::HashMap<String, (String, verify::Report)>,
    /// The workspace this hub is a member of, if any (docs/teams.md).
    pub team: Option<crate::team::member::Team>,
    /// Where images are written out for agents to open (`crate::images`).
    pub images_dir: std::path::PathBuf,
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
        let mut core = Core {
            store,
            keys,
            mirror: Mirror::default(),
            agents: Tracker::default(),
            events,
            owner: owner.to_string(),
            tailnet_owner: None,
            tailnet_host: None,
            started: std::time::Instant::now(),
            linear_ready: false,
            previews: Default::default(),
            review: Default::default(),
            build: Default::default(),
            live_dirty: Default::default(),
            mac_apps: Default::default(),
            verified: Default::default(),
            team: None,
            images_dir: crate::paths::state_dir().join("images"),
        };
        for name in ["owner", "hub"] {
            core.keys.get_or_create(name)?;
        }
        let at = crate::time::now_ms();
        let known: Vec<(String, String)> = core.keys.iter().map(|k| (k.keyid(), k.name.clone())).collect();
        for (id, name) in known {
            core.store.put_key(&id, &name, &at)?;
        }
        core.refold()?;
        crate::team::member::load(&mut core)?;
        Ok(core)
    }

    /// Fold every docket again when the fold has changed since its states
    /// were saved: they're a copy of what the entries say, kept for speed.
    ///
    /// Each state says which fold made it, so one saved by an older build (a
    /// branch run against the same store) is folded again too. A docket
    /// whose entries don't read keeps its saved state and is named in the
    /// log: it doesn't stop the hub (`wh verify` shows what's wrong).
    fn refold(&mut self) -> Result<()> {
        use crate::record::fold::{VERSION, fold};
        // Stale states, and dockets with entries but no state at all.
        let fresh: std::collections::HashSet<String> = self.store.states()?.into_iter().filter(|s| s.fold == VERSION).map(|s| s.id).collect();
        let stale: Vec<String> = self.store.docket_ids()?.into_iter().filter(|id| !fresh.contains(id)).collect();
        let (mut done, mut bad) = (0, Vec::new());
        for id in &stale {
            let folded = self.store.entries(id).and_then(|entries| {
                let parsed: Vec<Entry> = entries.iter().map(|s| s.entry()).collect::<Result<_>>()?;
                Ok(fold(id, entries.iter().zip(&parsed).map(|(s, e)| (s.seq, s.chain.as_str(), e))))
            });
            match folded {
                Ok(st) => {
                    self.store.put_state(&st)?;
                    done += 1;
                }
                Err(e) => {
                    say!("{id}: not folded again: {e:#}");
                    bad.push(id.clone());
                }
            }
        }
        if done > 0 {
            say!("folded {done} dockets again for fold v{VERSION}");
        }
        if !bad.is_empty() {
            say!("{} dockets kept their saved state ({}): run wh verify", bad.len(), bad.join(", "));
        }
        Ok(())
    }

    fn actor(&self, who: &Who) -> (Actor, String) {
        match who {
            Who::Hub => (Actor::new("hub", "hub@local"), "hub".into()),
            Who::Owner => (Actor::new("person", &self.owner), "owner".into()),
            Who::Agent { app, block, session } => {
                (Actor::new("agent", &format!("{app}@local")).with("block", block.as_str()).with("session", session.clone()), format!("agent-{app}@local"))
            }
        }
    }

    /// Sign, chain, store and fold one entry.
    pub fn append(&mut self, docket: &str, kind: &str, who: &Who, body: Option<String>, data: Value) -> Result<Stored> {
        self.append_with(docket, kind, who, body, data, Vec::new())
    }

    /// `append`, naming files (`sha256:<hex>`, already in the blob store).
    pub fn append_with(&mut self, docket: &str, kind: &str, who: &Who, body: Option<String>, data: Value, blobs: Vec<String>) -> Result<Stored> {
        // A shared docket's entries are ordered by its workspace: signed
        // here, sent from the outbox. Nothing else but this hub's own `D-`
        // dockets is ever written here.
        // A local docket that was shared: what's written on it now goes on
        // the shared one.
        let docket = match self.team.as_ref().and_then(|t| t.aliases.get(docket)) {
            Some(to) => &to.clone(),
            None => docket,
        };
        if self.team.as_ref().is_some_and(|t| t.shares(docket)) {
            return crate::team::member::append(self, docket, kind, who, body, data, blobs);
        }
        anyhow::ensure!(docket.starts_with("D-"), "{docket} isn't one of this hub's dockets");
        let (actor, key_name) = self.actor(who);
        let at = crate::time::now_ms();
        let mut e = Entry::new(docket, kind, &at, actor);
        e.body = body;
        e.data = data;
        e.blobs = blobs;
        let (envelope, keyid, new_key) = {
            let (key, new) = self.keys.get_or_create(&key_name)?;
            (Envelope::sign(&e.to_bytes(), key).to_string_compact(), key.keyid(), new)
        };
        if new_key {
            self.store.put_key(&keyid, &key_name, &at)?;
        }
        let stored = self.store.append_folded(docket, &envelope, kind, &at, &keyid, |s, was| {
            let mut st = was.unwrap_or_else(|| DocketState::new(docket));
            st.apply(s.seq, &s.chain, &e);
            Some(st)
        })?;
        let _ = self.events.send(json!({ "type": "docket", "docket": docket }));
        Ok(stored)
    }

    /// Open a new docket with its first entry.
    pub fn open(&mut self, who: &Who, title: &str, data: Value) -> Result<String> {
        let (id, _) = self.store.next_docket()?;
        self.append(&id, "open", who, Some(title.to_string()), data)?;
        Ok(id)
    }

    /// Something live changed (an agent's state), not the record. The
    /// `live` event goes out with the next beat of `run_live`.
    pub fn live_changed(&self) {
        self.live_dirty.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    /// Check a docket's record, and that every file it names is kept. A
    /// record whose head hasn't moved since its last check isn't read again.
    pub fn verify(&mut self, docket: &str) -> Result<verify::Report> {
        let head = self.store.state(docket)?.map(|s| s.head);
        if let (Some(h), Some((seen, r))) = (&head, self.verified.get(docket))
            && h == seen
        {
            return Ok(r.clone());
        }
        let r = self.check(docket, false)?;
        if let Some(h) = head {
            self.verified.insert(docket.to_string(), (h, r.clone()));
        }
        Ok(r)
    }

    /// `verify`, and every file's bytes against the hash the record names.
    pub fn verify_full(&self, docket: &str) -> Result<verify::Report> {
        self.check(docket, true)
    }

    fn check(&self, docket: &str, hash: bool) -> Result<verify::Report> {
        let entries = self.store.entries(docket)?;
        let mut r = match &self.team {
            // A shared docket checks with the keys its workspace's member
            // log names, and every entry against the rules, again.
            Some(t) if t.owns_id(docket) => crate::team::member::verify(t, docket, &entries),
            _ => {
                let known: BTreeSet<String> = self.store.keys()?.into_iter().map(|(id, _)| id).collect();
                verify::verify(docket, &entries, |k| known.contains(k))
            }
        };
        for s in &entries {
            let Ok(e) = s.entry() else { continue };
            for b in &e.blobs {
                let hex = b.strip_prefix("sha256:").unwrap_or(b);
                let problem = if hash {
                    match self.store.blob(hex)? {
                        None => Some("is missing"),
                        Some(bytes) if crate::record::hex::encode(&sha2::Sha256::digest(&bytes)) != hex => Some("doesn't match its hash: it was changed"),
                        Some(_) => None,
                    }
                } else {
                    (!self.store.has_blob(hex)?).then_some("is missing")
                };
                if let Some(p) = problem {
                    r.problems.push(format!("#{}: its file {} {p}", s.seq, &b[..b.len().min(19)]));
                }
            }
        }
        r.ok = r.problems.is_empty();
        Ok(r)
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

/// Send the `live` event when something live changed: at most every 300 ms,
/// however many changes come in between, so a busy Rex doesn't make every
/// app refetch on every keystroke.
pub async fn run_live(shared: Shared) {
    let (dirty, events) = {
        let core = shared.lock();
        (core.live_dirty.clone(), core.events.clone())
    };
    let mut beat = tokio::time::interval(std::time::Duration::from_millis(300));
    loop {
        beat.tick().await;
        if dirty.swap(false, std::sync::atomic::Ordering::Relaxed) {
            let _ = events.send(json!({ "type": "live" }));
        }
    }
}

#[cfg(test)]
pub fn test_core() -> Core {
    let mut core = Core::new(Store::in_memory().unwrap(), Keyring::in_memory(), "you").unwrap();
    // Never the real state's: each test core its own.
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    core.images_dir = std::env::temp_dir().join(format!("wh-test-images-{}-{n}", std::process::id()));
    core
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_state_an_older_build_saved_is_folded_again_and_a_bad_docket_doesnt_stop_the_hub() {
        let mut core = test_core();
        let a = core.open(&Who::Hub, "An agent's", json!({ "protocol": "agent", "agent": { "app": "claude-code", "host": "local", "block": "b1" } })).unwrap();
        let b = core.open(&Who::Owner, "Broken", json!({})).unwrap();
        // An older build saved a's state without what this fold adds.
        let mut old = core.store.state(&a).unwrap().unwrap();
        old.agents.clear();
        old.fold = 0;
        core.store.put_state(&old).unwrap();
        // b's entry no longer reads (and its state is stale too).
        let mut stale = core.store.state(&b).unwrap().unwrap();
        stale.fold = 0;
        core.store.put_state(&stale).unwrap();
        core.store.conn_for_tests().execute("UPDATE entries SET envelope = 'not an envelope' WHERE docket = ?1", [&b]).unwrap();

        core.refold().expect("a docket that doesn't read doesn't stop the hub");
        let st = core.store.state(&a).unwrap().unwrap();
        assert_eq!(st.agents.len(), 1, "folded again");
        assert_eq!(st.fold, crate::record::fold::VERSION);
        assert_eq!(core.store.state(&b).unwrap().unwrap().fold, 0, "kept its saved state");
        // Nothing stale left but the broken one: a second start folds only it.
        core.refold().unwrap();
        assert_eq!(core.store.state(&a).unwrap().unwrap(), st);

        // A stale state that gets another entry stays stale, so it's folded
        // at the next start; a docket with entries but no state gets one.
        let mut old = core.store.state(&a).unwrap().unwrap();
        old.agents.clear();
        old.fold = 0;
        core.store.put_state(&old).unwrap();
        core.append(&a, "note", &Who::Owner, Some("later".into()), json!({})).unwrap();
        assert_eq!(core.store.state(&a).unwrap().unwrap().fold, 0);
        core.store.conn_for_tests().execute("DELETE FROM dockets WHERE id = ?1", [&a]).unwrap();
        core.refold().unwrap();
        let st = core.store.state(&a).unwrap().expect("a state again");
        assert_eq!((st.fold, st.agents.len(), st.notes), (crate::record::fold::VERSION, 1, 1));
    }

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
