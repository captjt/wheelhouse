//! The member log (`ACME-0`) and the roster it folds to: who's in a
//! workspace, their keys, and the admins. docs/teams.md is the contract.
//!
//! Every hub folds the log the same way, and checks every entry against the
//! rules before it applies it: the workspace hub before it orders one, and
//! each member's hub as it follows the log. So a workspace hub can't add a
//! member or a key that no admin (or member) signed for.

use anyhow::{Result, anyhow, bail, ensure};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

use crate::record::{Entry, Envelope, Stored, chain, hex, keys};

/// How long an invite lasts unless it says otherwise.
pub const INVITE_HOURS: i64 = 24;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Roster {
    /// The workspace's id: 16 random bytes, hex.
    pub id: String,
    pub name: String,
    /// What its shared dockets are numbered with: `ACME` for `ACME-12`.
    pub prefix: String,
    pub members: BTreeMap<String, Member>,
    /// Every key the log has named, by key id: members' own and the ones
    /// they issued. A removed member's keys stay, marked, so what they
    /// signed before still verifies.
    pub keys: BTreeMap<String, KeyInfo>,
    /// Invites, by the id of their entry.
    pub invites: BTreeMap<String, Invite>,
    /// Entries applied, and the chain value after the last.
    pub seq: i64,
    pub head: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Member {
    pub handle: String,
    #[serde(default)]
    pub name: String,
    /// Their Tailscale login, as they gave it when they joined.
    #[serde(default)]
    pub login: Option<String>,
    /// Their member key: their hub's `owner` key.
    pub key: String,
    /// `admin` or `member`.
    pub role: String,
    pub joined_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub removed_at: Option<String>,
}

impl Member {
    pub fn active(&self) -> bool {
        self.removed_at.is_none()
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct KeyInfo {
    pub handle: String,
    /// `member` (their own key), `hub`, or `agent`.
    #[serde(rename = "use")]
    pub use_: String,
    /// The key's name on its hub: `owner`, `hub`, `agent-codex@local`.
    pub name: String,
    /// The member log entry that named it.
    pub since: i64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Invite {
    pub handle: String,
    pub role: String,
    /// The invite key's id: the key the code's secret makes.
    #[serde(rename = "pub")]
    pub pub_: String,
    pub expires: String,
    pub by: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub used_by: Option<String>,
}

/// A handle: lowercase letters, digits, `-` and `_`, starting with a
/// letter, at most 32. It goes into actor ids (`codex@sam`) and addresses.
pub fn handle_ok(h: &str) -> bool {
    let mut c = h.chars();
    matches!(c.next(), Some('a'..='z')) && h.len() <= 32 && c.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

/// A prefix: 2 to 10 capital letters and digits, starting with a letter.
/// `D` is the local dockets' own.
pub fn prefix_ok(p: &str) -> bool {
    let mut c = p.chars();
    matches!(c.next(), Some('A'..='Z')) && (2..=10).contains(&p.len()) && c.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
}

/// A key id that names an Ed25519 key, in the one spelling ids use.
pub fn keyid_ok(k: &str) -> bool {
    k.strip_prefix("ed25519:").is_some_and(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))) && keys::public_from_keyid(k).is_some()
}

/// What an invite key signs to admit a member key.
pub fn join_message(workspace: &str, invite: &str, key: &str) -> Vec<u8> {
    format!("wh-join/v1\n{workspace}\n{invite}\n{key}").into_bytes()
}

fn s<'a>(v: &'a Value, k: &str) -> Result<&'a str> {
    v[k].as_str().ok_or_else(|| anyhow!("its data has no {k}"))
}

impl Roster {
    /// The member whose own key this is, if they're active.
    pub fn member_by_key(&self, keyid: &str) -> Option<&Member> {
        let k = self.keys.get(keyid)?;
        let m = self.members.get(&k.handle)?;
        (k.use_ == "member" && m.key == keyid && m.active()).then_some(m)
    }

    /// The active member a key belongs to (their own, or one they issued),
    /// with the key's use.
    pub fn active_key(&self, keyid: &str) -> Option<(&Member, &KeyInfo)> {
        let k = self.keys.get(keyid)?;
        let m = self.members.get(&k.handle)?;
        m.active().then_some((m, k))
    }

    /// The member a key belongs to, active or not: for checking what was
    /// signed before a removal.
    pub fn any_key(&self, keyid: &str) -> Option<(&Member, &KeyInfo)> {
        let k = self.keys.get(keyid)?;
        Some((self.members.get(&k.handle)?, k))
    }

    pub fn admins(&self) -> impl Iterator<Item = &Member> {
        self.members.values().filter(|m| m.active() && m.role == "admin")
    }

    fn admin_by_key(&self, keyid: &str) -> Result<&Member> {
        let m = self.member_by_key(keyid).ok_or_else(|| anyhow!("signed by {}, which isn't an active member's own key", short(keyid)))?;
        ensure!(m.role == "admin", "{} isn't an admin", m.handle);
        Ok(m)
    }

    fn key_is_new(&self, keyid: &str) -> Result<()> {
        ensure!(keyid_ok(keyid), "{keyid} isn't an Ed25519 key id");
        ensure!(!self.keys.contains_key(keyid), "the key {} is already in the workspace", short(keyid));
        ensure!(!self.invites.values().any(|i| i.pub_ == keyid), "the key {} is an invite's", short(keyid));
        Ok(())
    }

    /// Whether `e`, signed by `keyid`, may be the next entry. Nothing
    /// changes. `id` is the entry's id (`sha256:…`).
    pub fn check(&self, e: &Entry, keyid: &str, id: &str) -> Result<()> {
        let d = &e.data;
        let at = crate::time::parse(&e.at).ok_or_else(|| anyhow!("its time {} doesn't read", e.at))?;
        ensure!(e.actor.typ == "person", "member log entries are people's, not {}'s", e.actor.typ);
        if self.seq == 0 {
            ensure!(e.kind == "workspace", "a member log starts with `workspace`, not `{}`", e.kind);
        }
        match e.kind.as_str() {
            "workspace" => {
                ensure!(self.seq == 0, "a workspace is created once");
                let wid = s(d, "id")?;
                ensure!(wid.len() == 32 && hex::decode(wid).is_some(), "a workspace id is 16 bytes of hex");
                ensure!(!s(d, "name")?.trim().is_empty() && s(d, "name")?.len() <= 64, "a workspace needs a name of at most 64 characters");
                ensure!(prefix_ok(s(d, "prefix")?), "a prefix is 2 to 10 capital letters and digits, starting with a letter");
                let h = s(d, "handle")?;
                ensure!(handle_ok(h), "{h} isn't a handle: lowercase letters, digits, - and _, starting with a letter");
                ensure!(e.actor.id == h, "the creator writes as {h}");
                ensure!(s(d, "key")? == keyid, "the creator signs with the key they name");
                self.key_is_new(keyid)?;
            }
            "invite" => {
                let admin = self.admin_by_key(keyid)?;
                ensure!(e.actor.id == admin.handle, "an admin writes as themselves");
                let h = s(d, "handle")?;
                ensure!(handle_ok(h), "{h} isn't a handle");
                ensure!(self.members.get(h).is_none_or(|m| !m.active()), "{h} is already a member");
                ensure!(matches!(s(d, "role")?, "member" | "admin"), "an invite's role is member or admin");
                self.key_is_new(s(d, "pub")?)?;
                let exp = crate::time::parse(s(d, "expires")?).ok_or_else(|| anyhow!("its expiry doesn't read"))?;
                ensure!(exp > at, "an invite that's expired already");
            }
            "join" => {
                let iid = s(d, "invite")?;
                let inv = self.invites.get(iid).ok_or_else(|| anyhow!("no invite {}", short(iid)))?;
                ensure!(inv.used_by.is_none(), "that invite was used");
                let exp = crate::time::parse(&inv.expires).ok_or_else(|| anyhow!("the invite's expiry doesn't read"))?;
                ensure!(at < exp, "that invite expired at {}", inv.expires);
                ensure!(s(d, "handle")? == inv.handle && e.actor.id == inv.handle, "the invite is for {}", inv.handle);
                ensure!(self.members.get(&inv.handle).is_none_or(|m| !m.active()), "{} is already a member", inv.handle);
                ensure!(s(d, "key")? == keyid, "a join is signed by the key it names");
                self.key_is_new(keyid)?;
                let proof = B64.decode(s(d, "proof")?).map_err(|_| anyhow!("its proof isn't base64"))?;
                ensure!(keys::verify_by(&inv.pub_, &join_message(&self.id, iid, keyid), &proof), "its proof isn't the invite key's signature: the code wasn't this invite's");
                if let Some(l) = d["login"].as_str() {
                    ensure!(l.len() <= 128 && !l.chars().any(char::is_control), "a login of at most 128 characters");
                }
                ensure!(d["name"].as_str().unwrap_or("").len() <= 64, "a name of at most 64 characters");
            }
            "key" => {
                let m = self.member_by_key(keyid).ok_or_else(|| anyhow!("a key is issued by an active member's own key"))?;
                ensure!(e.actor.id == m.handle, "a member issues keys as themselves");
                self.key_is_new(s(d, "keyid")?)?;
                ensure!(matches!(s(d, "use")?, "hub" | "agent"), "a key's use is hub or agent");
                let n = s(d, "name")?;
                ensure!(!n.is_empty() && n.len() <= 64 && !n.chars().any(char::is_control), "a key's name is 1 to 64 characters");
            }
            "role" => {
                let admin = self.admin_by_key(keyid)?;
                ensure!(e.actor.id == admin.handle, "an admin writes as themselves");
                let h = s(d, "handle")?;
                let m = self.members.get(h).filter(|m| m.active()).ok_or_else(|| anyhow!("{h} isn't an active member"))?;
                let role = s(d, "role")?;
                ensure!(matches!(role, "member" | "admin"), "a role is member or admin");
                ensure!(!(m.role == "admin" && role == "member" && self.admins().count() == 1), "the last admin stays an admin");
            }
            "remove" => {
                let h = s(d, "handle")?;
                let target = self.members.get(h).filter(|m| m.active()).ok_or_else(|| anyhow!("{h} isn't an active member"))?;
                let signer = self.member_by_key(keyid).ok_or_else(|| anyhow!("signed by a key that isn't an active member's own"))?;
                ensure!(e.actor.id == signer.handle, "a member writes as themselves");
                ensure!(signer.handle == h || signer.role == "admin", "only an admin removes someone else");
                ensure!(!(target.role == "admin" && self.admins().count() == 1), "the last admin can't be removed: make someone else an admin first");
            }
            other => bail!("`{other}` isn't a member log kind"),
        }
        let _ = id;
        Ok(())
    }

    /// Apply an entry `check` passed.
    pub fn apply(&mut self, seq: i64, chain: &str, id: &str, e: &Entry, keyid: &str) {
        let d = &e.data;
        let st = |k: &str| d[k].as_str().unwrap_or_default().to_string();
        match e.kind.as_str() {
            "workspace" => {
                self.id = st("id");
                self.name = st("name");
                self.prefix = st("prefix");
                let h = st("handle");
                self.members.insert(
                    h.clone(),
                    Member {
                        handle: h.clone(),
                        name: st("display"),
                        login: d["login"].as_str().map(str::to_string),
                        key: keyid.into(),
                        role: "admin".into(),
                        joined_at: e.at.clone(),
                        removed_at: None,
                    },
                );
                self.keys.insert(keyid.into(), KeyInfo { handle: h, use_: "member".into(), name: "owner".into(), since: seq });
            }
            "invite" => {
                self.invites.insert(id.into(), Invite { handle: st("handle"), role: st("role"), pub_: st("pub"), expires: st("expires"), by: e.actor.id.clone(), used_by: None });
            }
            "join" => {
                let iid = st("invite");
                let role = self.invites.get(&iid).map(|i| i.role.clone()).unwrap_or_else(|| "member".into());
                if let Some(i) = self.invites.get_mut(&iid) {
                    i.used_by = Some(keyid.into());
                }
                let h = st("handle");
                self.members.insert(
                    h.clone(),
                    Member {
                        handle: h.clone(),
                        name: st("name"),
                        login: d["login"].as_str().map(str::to_string),
                        key: keyid.into(),
                        role,
                        joined_at: e.at.clone(),
                        removed_at: None,
                    },
                );
                self.keys.insert(keyid.into(), KeyInfo { handle: h, use_: "member".into(), name: "owner".into(), since: seq });
            }
            "key" => {
                self.keys.insert(st("keyid"), KeyInfo { handle: e.actor.id.clone(), use_: st("use"), name: st("name"), since: seq });
            }
            "role" => {
                if let Some(m) = self.members.get_mut(&st("handle")) {
                    m.role = st("role");
                }
            }
            "remove" => {
                if let Some(m) = self.members.get_mut(&st("handle")) {
                    m.removed_at = Some(e.at.clone());
                }
            }
            _ => {}
        }
        self.seq = seq;
        self.head = chain.to_string();
    }

    /// Check and apply the next stored entry: its place in the chain, its
    /// signature, and the rules.
    pub fn admit(&mut self, s: &Stored) -> Result<()> {
        ensure!(s.seq == self.seq + 1, "#{} doesn't follow #{}", s.seq, self.seq);
        let id = chain::entry_id(s.envelope.as_bytes());
        ensure!(chain::id_string(&id) == s.id, "#{}: its bytes don't match its id", s.seq);
        let prev = if self.seq == 0 { chain::GENESIS } else { chain::parse_chain(&self.head).ok_or_else(|| anyhow!("the head isn't hex"))? };
        ensure!(hex::encode(&chain::next(&prev, &id)) == s.chain, "#{}: the chain breaks here", s.seq);
        let (e, keyid) = open_envelope(&s.envelope)?;
        ensure!(e.docket == s.docket, "#{}: an entry for {}, not {}", s.seq, e.docket, s.docket);
        if self.seq > 0 {
            ensure!(s.docket == format!("{}-0", self.prefix), "#{}: the member log is {}-0", s.seq, self.prefix);
        }
        self.check(&e, &keyid, &s.id).map_err(|err| anyhow!("#{}: {err}", s.seq))?;
        self.apply(s.seq, &s.chain, &s.id, &e, &keyid);
        Ok(())
    }

    /// The roster a whole member log folds to, checking every entry.
    pub fn fold(entries: &[Stored]) -> Result<Roster> {
        let mut r = Roster::default();
        for s in entries {
            r.admit(s)?;
        }
        Ok(r)
    }
}

/// An envelope's entry and its signer, with the signature checked against
/// the key the envelope names. Whether that key may sign is the caller's
/// rule to check.
pub fn open_envelope(envelope: &str) -> Result<(Entry, String)> {
    ensure!(envelope.len() <= MAX_ENVELOPE, "an entry of {} bytes; at most {MAX_ENVELOPE}", envelope.len());
    let env: Envelope = serde_json::from_str(envelope).map_err(|e| anyhow!("not an envelope: {e}"))?;
    let keyid = env.verify(keyid_ok)?;
    let e: Entry = serde_json::from_slice(&env.payload_bytes()?).map_err(|e| anyhow!("the payload isn't an entry: {e}"))?;
    ensure!(e.v == 1, "entry version {}", e.v);
    Ok((e, keyid))
}

/// The largest envelope a workspace takes: 256 KB. Files go as blobs.
pub const MAX_ENVELOPE: usize = 256 * 1024;

pub fn short(keyid: &str) -> String {
    let h = keyid.strip_prefix("ed25519:").unwrap_or(keyid);
    format!("ed25519:{}…", &h[..h.len().min(12)])
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::record::Key;
    use crate::record::entry::Actor;
    use serde_json::json;

    /// A member log being written, as a workspace hub would order it.
    pub struct Log {
        pub entries: Vec<Stored>,
        pub roster: Roster,
        pub prefix: String,
    }

    impl Log {
        pub fn new(prefix: &str) -> Self {
            Log { entries: Vec::new(), roster: Roster::default(), prefix: prefix.into() }
        }

        /// Sign `e` with `key`, order it, and admit it.
        pub fn push(&mut self, e: Entry, key: &Key) -> Result<Stored> {
            let s = self.order(e, key);
            self.roster.admit(&s)?;
            self.entries.push(s.clone());
            Ok(s)
        }

        pub fn order(&self, e: Entry, key: &Key) -> Stored {
            let env = Envelope::sign(&e.to_bytes(), key).to_string_compact();
            let id = chain::entry_id(env.as_bytes());
            let prev = self.entries.last().map(|s| chain::parse_chain(&s.chain).unwrap()).unwrap_or(chain::GENESIS);
            Stored { docket: e.docket.clone(), seq: self.entries.len() as i64 + 1, id: chain::id_string(&id), chain: hex::encode(&chain::next(&prev, &id)), envelope: env }
        }

        pub fn log(&self) -> String {
            format!("{}-0", self.prefix)
        }
    }

    pub const T0: &str = "2026-11-24T10:00:00.000Z";

    pub fn entry(log: &str, kind: &str, who: &str, data: Value) -> Entry {
        let mut e = Entry::new(log, kind, T0, Actor::new("person", who));
        e.data = data;
        e
    }

    pub fn create(jordan: &Key) -> Log {
        let mut l = Log::new("ACME");
        let wid = hex::encode(&[7u8; 16]);
        l.push(entry("ACME-0", "workspace", "jordan", json!({ "id": wid, "name": "Acme", "prefix": "ACME", "handle": "jordan", "key": jordan.keyid() })), jordan).unwrap();
        l
    }

    /// Invite `handle` (by `admin`), and join with `key` from the code.
    pub fn invite_and_join(l: &mut Log, admin: (&str, &Key), handle: &str, role: &str, key: &Key) -> Result<()> {
        let seed = keys::random32();
        let ik = Key::from_seed("invite", &seed).unwrap();
        let log = l.log();
        let inv = l.push(entry(&log, "invite", admin.0, json!({ "handle": handle, "role": role, "pub": ik.keyid(), "expires": "2026-11-25T10:00:00.000Z" })), admin.1)?;
        let proof = B64.encode(ik.sign(&join_message(&l.roster.id, &inv.id, &key.keyid())));
        l.push(
            entry(&log, "join", handle, json!({ "invite": inv.id, "handle": handle, "key": key.keyid(), "login": format!("{handle}@acme.dev"), "name": handle, "proof": proof })),
            key,
        )?;
        Ok(())
    }

    #[test]
    fn create_invite_join_and_issue_keys() {
        let jordan = Key::generate("owner");
        let sam = Key::generate("owner");
        let sam_hub = Key::generate("hub");
        let mut l = create(&jordan);
        invite_and_join(&mut l, ("jordan", &jordan), "sam", "member", &sam).unwrap();
        l.push(entry("ACME-0", "key", "sam", json!({ "keyid": sam_hub.keyid(), "use": "hub", "name": "hub" })), &sam).unwrap();
        let r = &l.roster;
        assert_eq!(r.prefix, "ACME");
        assert_eq!(r.members["jordan"].role, "admin");
        assert_eq!(r.members["sam"].role, "member");
        assert_eq!(r.members["sam"].login.as_deref(), Some("sam@acme.dev"));
        assert_eq!(r.active_key(&sam_hub.keyid()).map(|(m, k)| (m.handle.as_str(), k.use_.as_str())), Some(("sam", "hub")));
        assert!(r.member_by_key(&sam_hub.keyid()).is_none(), "a hub key isn't a member key");
        // Folding the log again gives the same roster.
        assert_eq!(&Roster::fold(&l.entries).unwrap(), r);
    }

    #[test]
    fn a_member_cant_invite_or_issue_keys_for_others() {
        let jordan = Key::generate("owner");
        let sam = Key::generate("owner");
        let mut l = create(&jordan);
        invite_and_join(&mut l, ("jordan", &jordan), "sam", "member", &sam).unwrap();
        let ik = Key::generate("invite");
        let err =
            l.push(entry("ACME-0", "invite", "sam", json!({ "handle": "lee", "role": "member", "pub": ik.keyid(), "expires": "2026-11-25T10:00:00.000Z" })), &sam).unwrap_err();
        assert!(err.to_string().contains("isn't an admin"), "{err}");
        // Sam signs a key entry claiming to be Jordan.
        let k = Key::generate("hub");
        let err = l.push(entry("ACME-0", "key", "jordan", json!({ "keyid": k.keyid(), "use": "hub", "name": "hub" })), &sam).unwrap_err();
        assert!(err.to_string().contains("as themselves"), "{err}");
    }

    #[test]
    fn an_invite_can_only_admit_the_key_its_secret_signed_for() {
        let jordan = Key::generate("owner");
        let mut l = create(&jordan);
        let seed = keys::random32();
        let ik = Key::from_seed("invite", &seed).unwrap();
        let inv =
            l.push(entry("ACME-0", "invite", "jordan", json!({ "handle": "sam", "role": "member", "pub": ik.keyid(), "expires": "2026-11-25T10:00:00.000Z" })), &jordan).unwrap();
        let sam = Key::generate("owner");
        let proof = B64.encode(ik.sign(&join_message(&l.roster.id, &inv.id, &sam.keyid())));
        // A workspace hub that saw Sam's proof swaps in a key of its own.
        let thief = Key::generate("owner");
        let err = l.push(entry("ACME-0", "join", "sam", json!({ "invite": inv.id, "handle": "sam", "key": thief.keyid(), "proof": proof.clone() })), &thief).unwrap_err();
        assert!(err.to_string().contains("proof"), "{err}");
        // Sam's own join goes in, once.
        l.push(entry("ACME-0", "join", "sam", json!({ "invite": inv.id, "handle": "sam", "key": sam.keyid(), "proof": proof })), &sam).unwrap();
        let again = Key::generate("owner");
        let p2 = B64.encode(ik.sign(&join_message(&l.roster.id, &inv.id, &again.keyid())));
        let err = l.push(entry("ACME-0", "join", "sam", json!({ "invite": inv.id, "handle": "sam", "key": again.keyid(), "proof": p2 })), &again).unwrap_err();
        assert!(err.to_string().contains("used"), "{err}");
    }

    #[test]
    fn an_expired_invite_admits_nobody() {
        let jordan = Key::generate("owner");
        let mut l = create(&jordan);
        let ik = Key::generate("invite");
        let inv =
            l.push(entry("ACME-0", "invite", "jordan", json!({ "handle": "sam", "role": "member", "pub": ik.keyid(), "expires": "2026-11-24T11:00:00.000Z" })), &jordan).unwrap();
        let sam = Key::generate("owner");
        let proof = B64.encode(ik.sign(&join_message(&l.roster.id, &inv.id, &sam.keyid())));
        let mut late = entry("ACME-0", "join", "sam", json!({ "invite": inv.id, "handle": "sam", "key": sam.keyid(), "proof": proof }));
        late.at = "2026-11-24T12:00:00.000Z".into();
        let err = l.push(late, &sam).unwrap_err();
        assert!(err.to_string().contains("expired"), "{err}");
    }

    #[test]
    fn removal_stops_a_members_keys_and_the_last_admin_stays() {
        let jordan = Key::generate("owner");
        let sam = Key::generate("owner");
        let sam_hub = Key::generate("hub");
        let mut l = create(&jordan);
        invite_and_join(&mut l, ("jordan", &jordan), "sam", "member", &sam).unwrap();
        l.push(entry("ACME-0", "key", "sam", json!({ "keyid": sam_hub.keyid(), "use": "hub", "name": "hub" })), &sam).unwrap();
        let err = l.push(entry("ACME-0", "remove", "jordan", json!({ "handle": "jordan" })), &jordan).unwrap_err();
        assert!(err.to_string().contains("last admin"), "{err}");
        let err = l.push(entry("ACME-0", "role", "jordan", json!({ "handle": "jordan", "role": "member" })), &jordan).unwrap_err();
        assert!(err.to_string().contains("last admin"), "{err}");
        l.push(entry("ACME-0", "remove", "jordan", json!({ "handle": "sam", "reason": "left the team" })), &jordan).unwrap();
        assert!(l.roster.active_key(&sam_hub.keyid()).is_none());
        assert!(l.roster.any_key(&sam_hub.keyid()).is_some(), "what Sam signed before still has a key to check against");
        let k = Key::generate("agent");
        assert!(l.push(entry("ACME-0", "key", "sam", json!({ "keyid": k.keyid(), "use": "agent", "name": "agent-codex@local" })), &sam).is_err());
    }

    #[test]
    fn a_rewritten_or_reordered_log_doesnt_fold() {
        let jordan = Key::generate("owner");
        let sam = Key::generate("owner");
        let mut l = create(&jordan);
        invite_and_join(&mut l, ("jordan", &jordan), "sam", "member", &sam).unwrap();
        let mut swapped = l.entries.clone();
        swapped.swap(1, 2);
        assert!(Roster::fold(&swapped).is_err());
        let mut dropped = l.entries.clone();
        dropped.remove(1);
        assert!(Roster::fold(&dropped).is_err());
        // A workspace hub that slips in a key entry signed by its own key.
        let mut slipped = l.clone_entries();
        let rogue = Key::generate("owner");
        let e = entry("ACME-0", "key", "sam", json!({ "keyid": rogue.keyid(), "use": "hub", "name": "hub" }));
        slipped.push(Log { entries: slipped.clone(), roster: Roster::default(), prefix: "ACME".into() }.order(e, &rogue));
        let err = Roster::fold(&slipped).unwrap_err();
        assert!(err.to_string().contains("active member"), "{err}");
    }

    impl Log {
        fn clone_entries(&self) -> Vec<Stored> {
            self.entries.clone()
        }
    }

    #[test]
    fn handles_and_prefixes() {
        assert!(handle_ok("sam") && handle_ok("p-r_1"));
        assert!(!handle_ok("Sam") && !handle_ok("1sam") && !handle_ok("") && !handle_ok("a/b") && !handle_ok("a@b"));
        assert!(prefix_ok("ACME") && prefix_ok("ENG2"));
        assert!(!prefix_ok("A") && !prefix_ok("acme") && !prefix_ok("2X") && !prefix_ok("ABCDEFGHIJK"));
    }
}
