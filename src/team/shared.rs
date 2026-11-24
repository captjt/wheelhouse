//! Shared dockets: who may write what on one (checked by the workspace hub
//! before it orders an entry, and by every member's hub as it follows),
//! who's on it, and what a member's hub keeps back before an entry leaves.
//! docs/teams.md is the contract.

use anyhow::{Result, anyhow, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::Digest;
use std::collections::BTreeSet;

use super::roster::Roster;
use crate::record::Entry;

/// The kinds a shared docket takes. Anything else is refused: a newer
/// hub's kind waits for the workspace hub to know it.
pub const KINDS: &[&str] = &[
    "share", "people", "trust", "note", "question", "decision", "intent", "set", "claim", "release", "turn", "observed", "check", "verdict", "standing", "artifact", "link",
    "approval",
];

/// How far ahead of the workspace hub's clock an entry's time may be.
pub const SKEW_MS: i64 = 5 * 60 * 1000;

/// Who's on a shared docket.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct People {
    pub owners: BTreeSet<String>,
    pub members: BTreeSet<String>,
    /// Who answers an agent's question: `owners` (its own person and the
    /// owners) or `members` (anyone on it).
    #[serde(default = "owners_default")]
    pub answer: String,
    /// Per person, whose agents' messages go into their panes without
    /// asking: (person, teammate) pairs.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub trust: BTreeSet<(String, String)>,
    /// People who keep their prompts on this docket private.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub private_prompts: BTreeSet<String>,
}

fn owners_default() -> String {
    "owners".into()
}

impl People {
    pub fn on(&self, handle: &str) -> bool {
        self.owners.contains(handle) || self.members.contains(handle)
    }

    /// Apply a `share`, `people` or `trust` entry.
    pub fn apply(&mut self, e: &Entry) {
        let d = &e.data;
        match e.kind.as_str() {
            "share" => {
                self.answer = owners_default();
                for o in d["owners"].as_array().into_iter().flatten().filter_map(Value::as_str) {
                    self.owners.insert(o.to_string());
                }
            }
            "people" => {
                let Some(h) = d["handle"].as_str().map(str::to_string) else {
                    if let Some(a) = d["answer"].as_str() {
                        self.answer = a.to_string();
                    }
                    return;
                };
                match (d["action"].as_str(), d["role"].as_str()) {
                    (Some("remove"), _) => {
                        self.owners.remove(&h);
                        self.members.remove(&h);
                    }
                    (_, Some("owner")) => {
                        self.members.remove(&h);
                        self.owners.insert(h);
                    }
                    _ => {
                        self.owners.remove(&h);
                        self.members.insert(h);
                    }
                }
                if let Some(a) = d["answer"].as_str() {
                    self.answer = a.to_string();
                }
            }
            "trust" => {
                let (Some(t), who) = (d["teammate"].as_str(), e.actor.id.clone()) else { return };
                let pair = (who.clone(), t.to_string());
                if d["on"] == false {
                    self.trust.remove(&pair);
                } else {
                    self.trust.insert(pair);
                }
                if let Some(p) = d["private_prompts"].as_bool() {
                    if p {
                        self.private_prompts.insert(who);
                    } else {
                        self.private_prompts.remove(&who);
                    }
                }
            }
            _ => {}
        }
    }
}

/// The member a key speaks for, and whether the actor is theirs: a person
/// is their own member key, `hub@sam` is a key Sam issued for their hub,
/// and `codex@sam` one Sam issued for an agent (or their hub's).
pub fn speaker<'a>(roster: &'a Roster, e: &Entry, keyid: &str) -> Result<&'a str> {
    let (m, k) = roster.active_key(keyid).ok_or_else(|| anyhow!("signed by a key that isn't an active member's"))?;
    let h = m.handle.as_str();
    match (e.actor.typ.as_str(), k.use_.as_str()) {
        ("person", "member") if e.actor.id == h => {}
        ("hub", "hub") if e.actor.id == format!("hub@{h}") => {}
        ("agent", "agent" | "hub") if e.actor.id.strip_suffix(&format!("@{h}")).is_some_and(|app| !app.is_empty() && !app.contains('@')) => {}
        (t, u) => bail!("{t} {} can't sign with {h}'s {u} key", e.actor.id),
    }
    Ok(h)
}

/// Whether `e`, signed by `keyid`, may be entry `seq` of shared docket
/// `docket`, given who's on it so far. `now_ms` is the checker's clock, for
/// entries from the future; `None` skips that (a member's hub reading what
/// was ordered long ago).
pub fn check(roster: &Roster, people: &People, docket: &str, seq: i64, e: &Entry, keyid: &str, now_ms: Option<i64>) -> Result<()> {
    ensure!(e.docket == docket, "an entry for {}, not {docket}", e.docket);
    ensure!(KINDS.contains(&e.kind.as_str()), "`{}` isn't a kind a shared docket takes", e.kind);
    ensure!((seq == 1) == (e.kind == "share"), "a shared docket starts with `share`, and only its first entry is one");
    let at = crate::time::parse(&e.at).ok_or_else(|| anyhow!("its time {} doesn't read", e.at))?;
    if let Some(now) = now_ms {
        let ms = (at.unix_timestamp_nanos() / 1_000_000) as i64;
        ensure!(ms <= now + SKEW_MS, "its time {} is ahead of the workspace's clock", e.at);
    }
    let h = speaker(roster, e, keyid)?;
    let owner = people.owners.contains(h);
    let d = &e.data;
    match e.kind.as_str() {
        "share" => {
            ensure!(e.actor.typ == "person", "a person shares a docket");
            let owners: Vec<&str> = d["owners"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
            ensure!(owners.contains(&h), "the sharer is its first owner");
            for o in owners {
                ensure!(roster.members.get(o).is_some_and(|m| m.active()), "{o} isn't an active member");
            }
        }
        "people" => {
            ensure!(e.actor.typ == "person", "a person changes who's on a docket");
            match d["handle"].as_str() {
                None => ensure!(owner && d["answer"].is_string(), "an owner sets who answers"),
                Some(t) => {
                    ensure!(roster.members.get(t).is_some_and(|m| m.active()) || d["action"] == "remove", "{t} isn't an active member");
                    let self_join = t == h && d["action"] != "remove" && d["role"] != "owner";
                    let self_leave = t == h && d["action"] == "remove";
                    ensure!(owner || self_join || self_leave, "only an owner adds or removes someone else, or makes an owner");
                    if d["action"] == "remove" && people.owners.contains(t) {
                        ensure!(people.owners.len() > 1, "a shared docket keeps at least one owner");
                    }
                }
            }
            if let Some(a) = d["answer"].as_str() {
                ensure!(matches!(a, "owners" | "members"), "who answers is owners or members");
            }
        }
        "trust" => {
            ensure!(e.actor.typ == "person", "a person trusts a teammate's agents");
            ensure!(people.on(h), "{h} isn't on this docket");
            if let Some(t) = d["teammate"].as_str() {
                ensure!(roster.members.contains_key(t), "{t} isn't a member");
            }
        }
        "intent" | "set" => ensure!(owner, "only an owner (or their agents) changes a shared docket's {}", if e.kind == "intent" { "intent" } else { "title or status" }),
        "approval" => {
            // Redacted at writing: who answered, not what.
            ensure!(e.body.is_none(), "an approval on a shared docket carries no text");
            for k in ["record", "keys", "prompt", "msg", "text"] {
                ensure!(d.get(k).is_none(), "an approval on a shared docket doesn't carry `{k}`");
            }
        }
        _ => {}
    }
    Ok(())
}

/// What a member's hub writes on a shared docket instead of `(body, data)`,
/// and the full text it keeps for itself (`data.private` names its
/// digest). Approvals and blocked prompts always; prompts when their person
/// keeps them private.
pub fn redact(kind: &str, body: Option<String>, data: Value, private_prompts: bool) -> (Option<String>, Value, Option<Vec<u8>>) {
    let prompt = kind == "note" && data["via"] == "prompt";
    let blocked = kind == "observed" && matches!(data["what"].as_str(), Some("blocked"));
    if !(kind == "approval" || blocked || (prompt && private_prompts)) {
        return (body, data, None);
    }
    let full = serde_json::to_vec(&json!({ "body": body, "data": data })).expect("json");
    let digest = format!("sha256:{}", crate::record::hex::encode(&sha2::Sha256::digest(&full)));
    let mut kept = serde_json::Map::new();
    for k in ["decision", "by", "kind", "block", "via", "what", "to", "from", "agent", "session"] {
        if let Some(v) = data.get(k) {
            kept.insert(k.into(), v.clone());
        }
    }
    kept.insert("private".into(), Value::String(digest));
    (None, Value::Object(kept), Some(full))
}

/// Why an entry's text looks like it holds a secret, if it does: checked
/// before an entry leaves a member's hub.
pub fn secret_in(body: Option<&str>, data: &Value) -> Option<String> {
    let mut text = body.unwrap_or("").to_string();
    text.push('\n');
    text.push_str(&data.to_string());
    looks_secret(&text)
}

fn looks_secret(t: &str) -> Option<String> {
    if t.contains("-----BEGIN") && t.contains("PRIVATE KEY") {
        return Some("a private key".into());
    }
    const PREFIXES: &[(&str, usize, &str)] = &[
        ("ghp_", 30, "a GitHub token"),
        ("gho_", 30, "a GitHub token"),
        ("ghs_", 30, "a GitHub token"),
        ("github_pat_", 30, "a GitHub token"),
        ("sk-ant-", 30, "an Anthropic key"),
        ("sk-proj-", 30, "an OpenAI key"),
        ("xoxb-", 20, "a Slack token"),
        ("xoxp-", 20, "a Slack token"),
        ("AKIA", 16, "an AWS access key"),
        ("lin_api_", 30, "a Linear key"),
        ("glpat-", 20, "a GitLab token"),
        ("tskey-", 20, "a Tailscale key"),
    ];
    for (p, min, what) in PREFIXES {
        let mut rest = t;
        while let Some(i) = rest.find(p) {
            let tail = &rest[i + p.len()..];
            let n = tail.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-').count();
            let start_ok = i == 0 || !rest[..i].chars().next_back().is_some_and(|c| c.is_ascii_alphanumeric());
            if start_ok && n + p.len() >= *min {
                return Some((*what).into());
            }
            rest = &rest[i + p.len()..];
        }
    }
    // `password = …`, `token: …`, `secret=…`, `api_key …` followed by a
    // long random-looking value.
    let lower = t.to_ascii_lowercase();
    for word in ["password", "passwd", "secret", "token", "api_key", "apikey", "api-key", "private_key"] {
        let mut from = 0;
        while let Some(i) = lower[from..].find(word) {
            let after = &t[from + i + word.len()..];
            let after = after.trim_start_matches(['"', '\'', ' ', '\\']);
            if let Some(v) = after.strip_prefix(['=', ':']) {
                let v = v.trim_start_matches(['"', '\'', ' ', '\\']);
                let val: String = v.chars().take_while(|c| !c.is_whitespace() && !matches!(c, '"' | '\'' | ',' | '}' | '\\')).collect();
                if val.len() >= 16 && random_looking(&val) {
                    return Some(format!("a {word} value"));
                }
            }
            from += i + word.len();
        }
    }
    None
}

/// Letters and digits mixed, the way a generated value is and a word isn't.
fn random_looking(v: &str) -> bool {
    let digits = v.chars().filter(char::is_ascii_digit).count();
    let upper = v.chars().filter(char::is_ascii_uppercase).count();
    let lower = v.chars().filter(char::is_ascii_lowercase).count();
    digits >= 2 && (upper + lower) >= 4 && !v.contains('<') && !v.starts_with("${") && !v.starts_with('$')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::Key;
    use crate::record::entry::Actor;
    use crate::team::roster::tests::{T0, create, entry as log_entry, invite_and_join};

    struct World {
        roster: Roster,
        jordan: Key,
        sam: Key,
        sam_hub: Key,
        sam_agent: Key,
    }

    fn world() -> World {
        let jordan = Key::generate("owner");
        let sam = Key::generate("owner");
        let sam_hub = Key::generate("hub");
        let sam_agent = Key::generate("agent-codex@local");
        let mut l = create(&jordan);
        invite_and_join(&mut l, ("jordan", &jordan), "sam", "member", &sam).unwrap();
        l.push(log_entry("ACME-0", "key", "sam", json!({ "keyid": sam_hub.keyid(), "use": "hub", "name": "hub" })), &sam).unwrap();
        l.push(log_entry("ACME-0", "key", "sam", json!({ "keyid": sam_agent.keyid(), "use": "agent", "name": "agent-codex@local" })), &sam).unwrap();
        World { roster: l.roster, jordan, sam, sam_hub, sam_agent }
    }

    fn e(kind: &str, typ: &str, id: &str, data: Value) -> Entry {
        let mut e = Entry::new("ACME-1", kind, T0, Actor::new(typ, id));
        e.data = data;
        e
    }

    fn shared(w: &World) -> People {
        let mut p = People::default();
        p.apply(&e("share", "person", "jordan", json!({ "owners": ["jordan"] })));
        let _ = w;
        p
    }

    #[test]
    fn a_share_starts_a_docket_once() {
        let w = world();
        let p = People::default();
        let share = e("share", "person", "jordan", json!({ "owners": ["jordan"] }));
        check(&w.roster, &p, "ACME-1", 1, &share, &w.jordan.keyid(), None).unwrap();
        assert!(check(&w.roster, &p, "ACME-1", 2, &share, &w.jordan.keyid(), None).is_err());
        let note = e("note", "person", "jordan", json!({}));
        assert!(check(&w.roster, &p, "ACME-1", 1, &note, &w.jordan.keyid(), None).is_err());
        // Sam can't share in Jordan's name, nor without being an owner.
        let other = e("share", "person", "sam", json!({ "owners": ["jordan"] }));
        assert!(check(&w.roster, &p, "ACME-1", 1, &other, &w.sam.keyid(), None).is_err());
    }

    #[test]
    fn actors_must_be_their_keys() {
        let w = world();
        let p = shared(&w);
        let ok = |kind: &str, typ: &str, id: &str, k: &Key| check(&w.roster, &p, "ACME-1", 2, &e(kind, typ, id, json!({})), &k.keyid(), None);
        ok("note", "person", "sam", &w.sam).unwrap();
        ok("turn", "hub", "hub@sam", &w.sam_hub).unwrap();
        ok("note", "agent", "codex@sam", &w.sam_agent).unwrap();
        assert!(ok("note", "person", "jordan", &w.sam).is_err(), "Sam writes as Jordan");
        assert!(ok("turn", "hub", "hub@jordan", &w.sam_hub).is_err(), "Sam's hub writes as Jordan's");
        assert!(ok("note", "agent", "codex@jordan", &w.sam_agent).is_err(), "Sam's agent writes as Jordan's");
        assert!(ok("note", "person", "sam", &w.sam_agent).is_err(), "an agent key writes as its person");
        assert!(ok("note", "person", "sam", &Key::generate("x")).is_err(), "an unknown key");
    }

    #[test]
    fn only_owners_change_intent_status_and_people() {
        let w = world();
        let mut p = shared(&w);
        let c = |p: &People, entry: Entry, k: &Key| check(&w.roster, p, "ACME-1", 2, &entry, &k.keyid(), None);
        assert!(c(&p, e("intent", "person", "sam", json!({})), &w.sam).is_err());
        assert!(c(&p, e("set", "agent", "codex@sam", json!({ "status": "done" })), &w.sam_agent).is_err());
        c(&p, e("intent", "person", "jordan", json!({})), &w.jordan).unwrap();
        // Sam may join as a member, not as an owner.
        c(&p, e("people", "person", "sam", json!({ "handle": "sam", "role": "member" })), &w.sam).unwrap();
        assert!(c(&p, e("people", "person", "sam", json!({ "handle": "sam", "role": "owner" })), &w.sam).is_err());
        // Jordan makes Sam an owner; now Sam's agents may change the intent.
        let up = e("people", "person", "jordan", json!({ "handle": "sam", "role": "owner" }));
        c(&p, up.clone(), &w.jordan).unwrap();
        p.apply(&up);
        c(&p, e("intent", "agent", "codex@sam", json!({})), &w.sam_agent).unwrap();
        // The last owner can't be removed.
        let mut solo = shared(&w);
        solo.apply(&e("people", "person", "jordan", json!({ "handle": "sam", "role": "member" })));
        assert!(c(&solo, e("people", "person", "jordan", json!({ "handle": "jordan", "action": "remove" })), &w.jordan).is_err());
    }

    #[test]
    fn approvals_arrive_redacted_or_not_at_all() {
        let w = world();
        let p = shared(&w);
        let full = e("approval", "hub", "hub@sam", json!({ "decision": "approve", "record": "r1", "keys": "1" }));
        assert!(check(&w.roster, &p, "ACME-1", 2, &full, &w.sam_hub.keyid(), None).is_err());
        let (body, data, private) = redact(
            "approval",
            Some("Bash: rm -rf build".into()),
            json!({ "decision": "approve", "by": "sam", "kind": "permission", "record": "r1", "keys": "1", "block": "b1" }),
            false,
        );
        assert!(body.is_none());
        assert!(data.get("record").is_none() && data.get("keys").is_none());
        assert_eq!(data["decision"], "approve");
        let private = private.unwrap();
        assert!(String::from_utf8_lossy(&private).contains("rm -rf build"));
        assert_eq!(data["private"].as_str().unwrap(), format!("sha256:{}", crate::record::hex::encode(&sha2::Sha256::digest(&private))));
        let mut red = e("approval", "hub", "hub@sam", data);
        red.body = body;
        check(&w.roster, &p, "ACME-1", 2, &red, &w.sam_hub.keyid(), None).unwrap();
    }

    #[test]
    fn prompts_are_kept_back_only_when_their_person_asks() {
        let (b, d, p) = redact("note", Some("try the cache".into()), json!({ "via": "prompt" }), false);
        assert_eq!(b.as_deref(), Some("try the cache"));
        assert!(p.is_none() && d.get("private").is_none());
        let (b, d, p) = redact("note", Some("try the cache".into()), json!({ "via": "prompt" }), true);
        assert!(b.is_none() && p.is_some() && d["via"] == "prompt");
        let (b, _, p) = redact("observed", Some("Allow Bash(curl …)?".into()), json!({ "what": "blocked" }), false);
        assert!(b.is_none() && p.is_some(), "a permission prompt's text never leaves");
    }

    #[test]
    fn the_future_is_refused() {
        let w = world();
        let p = shared(&w);
        let now = (crate::time::parse(T0).unwrap().unix_timestamp_nanos() / 1_000_000) as i64;
        let mut late = e("note", "person", "jordan", json!({}));
        check(&w.roster, &p, "ACME-1", 2, &late, &w.jordan.keyid(), Some(now)).unwrap();
        late.at = "2026-11-24T11:00:00.000Z".into();
        assert!(check(&w.roster, &p, "ACME-1", 2, &late, &w.jordan.keyid(), Some(now)).is_err());
    }

    #[test]
    fn secrets_are_spotted() {
        assert!(secret_in(Some("use ghp_abcdefghijklmnopqrstuvwxyz0123456789 for it"), &Value::Null).is_some());
        assert!(secret_in(None, &json!({ "x": "AKIAABCDEFGHIJKLMNOP" })).is_some());
        assert!(secret_in(Some("-----BEGIN OPENSSH PRIVATE KEY-----\nb3Bl"), &Value::Null).is_some());
        assert!(secret_in(Some("password=Xk92mPq7Lr4tZw8vNb3s"), &Value::Null).is_some());
        assert!(secret_in(Some("API_KEY: \"a8F3kL92mQx7Rt5vWz1p\""), &Value::Null).is_some());
        // Words, placeholders and talk about tokens are fine.
        assert!(secret_in(Some("the token bucket refills every second"), &Value::Null).is_none());
        assert!(secret_in(Some("password = <your password here>"), &Value::Null).is_none());
        assert!(secret_in(Some("token: ${GITHUB_TOKEN}"), &Value::Null).is_none());
        assert!(secret_in(Some("set password=correcthorsebatterystaple"), &Value::Null).is_none());
        assert!(secret_in(Some("we use ghp_ tokens"), &Value::Null).is_none());
    }
}
