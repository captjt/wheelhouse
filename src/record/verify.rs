//! Checking a docket's record: ids, order, chain, signatures, and payloads.

use serde::Serialize;

use super::{Envelope, Stored, chain, hex};

#[derive(Clone, Debug, Serialize)]
pub struct Report {
    pub docket: String,
    pub ok: bool,
    pub entries: usize,
    /// The head the entries chain to, when they do.
    pub head: Option<String>,
    pub problems: Vec<String>,
}

/// Check `entries` (in stored order) against the keys `known` accepts.
pub fn verify(docket: &str, entries: &[Stored], known: impl Fn(&str) -> bool) -> Report {
    let mut problems = Vec::new();
    let mut prev = chain::GENESIS;
    let mut chained = true;
    for (i, s) in entries.iter().enumerate() {
        let want_seq = i as i64 + 1;
        let at = format!("#{}", s.seq);
        if s.seq != want_seq {
            problems.push(format!("{at}: expected #{want_seq} here; an entry is missing or out of order"));
        }
        let id = chain::entry_id(s.envelope.as_bytes());
        if chain::id_string(&id) != s.id {
            problems.push(format!("{at}: its bytes don't match its id: the entry was changed"));
        }
        if chained {
            let c = chain::next(&prev, &id);
            if hex::encode(&c) != s.chain {
                problems.push(format!("{at}: the chain breaks here"));
                chained = false;
            }
            prev = c;
        }
        match serde_json::from_str::<Envelope>(&s.envelope) {
            Err(e) => problems.push(format!("{at}: not an envelope: {e}")),
            Ok(env) => {
                if let Err(e) = env.verify(&known) {
                    problems.push(format!("{at}: {e}"));
                }
                match env.payload_bytes().ok().and_then(|b| serde_json::from_slice::<super::Entry>(&b).ok()) {
                    None => problems.push(format!("{at}: the payload isn't an entry")),
                    Some(e) if e.docket != docket => problems.push(format!("{at}: an entry for {}, not {docket}", e.docket)),
                    Some(e) if e.v != 1 => problems.push(format!("{at}: entry version {}", e.v)),
                    Some(_) => {}
                }
            }
        }
    }
    Report { docket: docket.to_string(), ok: problems.is_empty(), entries: entries.len(), head: (chained && !entries.is_empty()).then(|| hex::encode(&prev)), problems }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::entry::Actor;
    use crate::record::{Entry, Key, fold};

    /// A small docket: three entries, chained.
    fn docket(key: &Key) -> Vec<Stored> {
        let mut prev = chain::GENESIS;
        let mut out = Vec::new();
        let entries = [
            Entry::new("D-1", "open", "2026-11-17T10:00:00.000Z", Actor::new("person", "you")).body("Retry failed webhooks"),
            Entry::new("D-1", "note", "2026-11-17T10:01:00.000Z", Actor::new("person", "you")).body("cap it at 3"),
            Entry::new("D-1", "set", "2026-11-17T10:02:00.000Z", Actor::new("person", "you")).data(serde_json::json!({ "status": "done" })),
        ];
        for (i, e) in entries.iter().enumerate() {
            let env = Envelope::sign(&e.to_bytes(), key).to_string_compact();
            let id = chain::entry_id(env.as_bytes());
            prev = chain::next(&prev, &id);
            out.push(Stored { docket: "D-1".into(), seq: i as i64 + 1, id: chain::id_string(&id), chain: hex::encode(&prev), envelope: env });
        }
        out
    }

    #[test]
    fn a_whole_record_checks() {
        let key = Key::generate("owner");
        let id = key.keyid();
        let d = docket(&key);
        let r = verify("D-1", &d, |k| k == id);
        assert!(r.ok, "{:?}", r.problems);
        assert_eq!(r.head.as_deref(), Some(d[2].chain.as_str()));
    }

    #[test]
    fn a_changed_byte_fails() {
        let key = Key::generate("owner");
        let id = key.keyid();
        let mut d = docket(&key);
        d[1].envelope = d[1].envelope.replacen("\"sig\":\"", "\"sig\":\"A", 1);
        let r = verify("D-1", &d, |k| k == id);
        assert!(!r.ok);
        assert!(r.problems.iter().any(|p| p.contains("#2") && p.contains("changed")), "{:?}", r.problems);
    }

    #[test]
    fn a_rewritten_entry_with_fresh_ids_still_fails_its_signature() {
        let key = Key::generate("owner");
        let id = key.keyid();
        let mut d = docket(&key);
        // Change the payload, then recompute every id and chain value, as
        // someone rewriting history without the key would.
        let mut env: Envelope = serde_json::from_str(&d[1].envelope).unwrap();
        let mut e: Entry = serde_json::from_slice(&env.payload_bytes().unwrap()).unwrap();
        e.body = Some("cap it at 30".into());
        use base64::Engine;
        env.payload = base64::engine::general_purpose::STANDARD.encode(e.to_bytes());
        d[1].envelope = env.to_string_compact();
        let mut prev = chain::GENESIS;
        for s in d.iter_mut() {
            let i = chain::entry_id(s.envelope.as_bytes());
            prev = chain::next(&prev, &i);
            s.id = chain::id_string(&i);
            s.chain = hex::encode(&prev);
        }
        let r = verify("D-1", &d, |k| k == id);
        assert!(!r.ok);
        assert!(r.problems.iter().any(|p| p.contains("#2") && p.contains("signature")), "{:?}", r.problems);
    }

    #[test]
    fn a_removed_entry_fails() {
        let key = Key::generate("owner");
        let id = key.keyid();
        let mut d = docket(&key);
        d.remove(1);
        let r = verify("D-1", &d, |k| k == id);
        assert!(!r.ok);
        assert!(r.problems.iter().any(|p| p.contains("missing or out of order")), "{:?}", r.problems);
        assert!(r.problems.iter().any(|p| p.contains("chain breaks")), "{:?}", r.problems);
    }

    #[test]
    fn a_reordered_pair_fails() {
        let key = Key::generate("owner");
        let id = key.keyid();
        let mut d = docket(&key);
        d.swap(1, 2);
        let r = verify("D-1", &d, |k| k == id);
        assert!(!r.ok);
        assert!(r.problems.iter().any(|p| p.contains("chain breaks")), "{:?}", r.problems);
    }

    #[test]
    fn an_unknown_key_fails() {
        let key = Key::generate("owner");
        let d = docket(&key);
        let r = verify("D-1", &d, |_| false);
        assert!(!r.ok);
    }

    #[test]
    fn the_fold_reads_the_record() {
        let key = Key::generate("owner");
        let d = docket(&key);
        let entries: Vec<Entry> = d.iter().map(|s| s.entry().unwrap()).collect();
        let st = fold::fold("D-1", d.iter().zip(&entries).map(|(s, e)| (s.seq, s.chain.as_str(), e)));
        assert_eq!(st.title, "Retry failed webhooks");
        assert_eq!(st.status, "done");
        assert_eq!(st.notes, 1);
        assert_eq!(st.entries, 3);
        assert_eq!(st.head, d[2].chain);
        assert_eq!(st.ended_at.as_deref(), Some("2026-11-17T10:02:00.000Z"));
    }
}
