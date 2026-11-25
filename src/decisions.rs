//! Decisions in force: each question an agent put to you on a docket, and
//! the answer that stands, with who gave it and when. Read from the record,
//! so the apps, `wh brief` and an export all agree.
//!
//! A question is a `question` entry (a message with options). Its answer is
//! the latest `decision` (a button, maybe with words) or `note` (your own
//! words) that replies to it, or, for a Build decision the plan's approval
//! settled, a `decision` that says the recommendation stood. A Build asks
//! again with a new message under the same id (`D2`); the newest stands.
//! Build's gates (the plan, the PR, a round) are approvals, not decisions.

use serde_json::{Value, json};
use std::collections::BTreeMap;

use crate::record::Entry;

#[derive(Clone, Debug, PartialEq)]
pub struct Decision {
    /// What it's known by: a Build decision's id (`D2`), or `#` and the message.
    pub key: String,
    pub question: String,
    pub options: Vec<String>,
    pub asked_by: String,
    pub asked_at: String,
    pub message: Option<i64>,
    /// The answer in force, if there is one yet.
    pub answer: Option<String>,
    /// Words given with the answer, beyond the choice.
    pub note: Option<String>,
    /// `you`, an agent's app, or `stood` (the recommendation, with the plan).
    pub by: Option<String>,
    pub at: Option<String>,
    /// The repo it stands for, once you made it standing.
    pub standing: Option<String>,
}

impl Decision {
    pub fn json(&self) -> Value {
        json!({
            "key": self.key, "question": self.question, "options": self.options,
            "asked_by": self.asked_by, "asked_at": self.asked_at, "message": self.message,
            "answer": self.answer, "note": self.note, "by": self.by, "at": self.at, "standing": self.standing,
        })
    }
}

/// A decision as a standing rule for its repo's notes.
pub fn rule(d: &Decision, docket: &str) -> String {
    let day = crate::time::parse(d.at.as_deref().unwrap_or("")).map(|t| format!(", {} {}", t.month(), t.day())).unwrap_or_default();
    let q = d.question.trim().trim_end_matches(['?', '.', ':']).trim();
    let a = d.answer.as_deref().unwrap_or("").trim().trim_end_matches('.');
    let note = d.note.as_deref().map(str::trim).filter(|n| !n.is_empty()).map(|n| format!(": {}.", n.trim_end_matches('.'))).unwrap_or_else(|| ".".into());
    if q.is_empty() {
        return format!("- {a}{note} (Decided on {docket}{day}.)");
    }
    format!("- {q}? {a}{note} (Decided on {docket}{day}.)")
}

/// Notes with the rule added under "Standing decisions", or `None` when
/// it's there already.
pub fn with_rule(notes: &str, rule: &str) -> Option<String> {
    let notes = notes.trim_end();
    if notes.contains(rule) {
        return None;
    }
    Some(if notes.contains("## Standing decisions") {
        format!("{notes}\n{rule}\n")
    } else if notes.is_empty() {
        format!("## Standing decisions\n\n{rule}\n")
    } else {
        format!("{notes}\n\n## Standing decisions\n\n{rule}\n")
    })
}

/// A decision of your own is known by its subject (or, with none, its
/// words): `=` and the first 8 hex of their sha256, the same on every hub.
pub fn own_key(words: &str) -> String {
    use sha2::Digest;
    let norm = words.trim().trim_end_matches(['?', '.', ':', '!']).split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
    let h = sha2::Sha256::digest(norm.as_bytes());
    format!("={}", h.iter().take(4).map(|b| format!("{b:02x}")).collect::<String>())
}

fn first_line(s: &str) -> String {
    s.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("").chars().take(200).collect()
}

fn who(e: &Entry) -> String {
    match e.actor.typ.as_str() {
        "person" => "you".into(),
        "agent" => crate::mirror::app_name(e.actor.id.split('@').next().unwrap_or("agent")).to_string(),
        _ => "Wheelhouse".into(),
    }
}

/// A Build gate's ask (`plan`, `open`, `push`…), not a decision.
fn gate(d: &Value) -> bool {
    d["protocol"] == "build" && !d["ask"].as_str().is_some_and(|a| a.starts_with('D') && a[1..].chars().all(|c| c.is_ascii_digit()) && a.len() > 1)
}

/// Every question asked on a docket, in the order first asked, each with
/// the answer that stands.
#[cfg(test)]
pub fn of(entries: &[Entry]) -> Vec<Decision> {
    fold(entries.iter().map(|e| (None, e)))
}

/// `of`, from stored entries: on a shared docket (docs/teams.md) a question
/// is known by its entry's id, the same on every hub, since each hub numbers
/// its own messages; an answer names it by `answers`.
pub fn of_stored(stored: &[crate::record::Stored]) -> Vec<Decision> {
    let read: Vec<(String, Entry)> = stored.iter().filter_map(|s| Some((s.id.clone(), s.entry().ok()?))).collect();
    fold(read.iter().map(|(id, e)| (Some(id.as_str()), e)))
}

fn fold<'a>(entries: impl Iterator<Item = (Option<&'a str>, &'a Entry)>) -> Vec<Decision> {
    let mut order: Vec<String> = Vec::new();
    let mut by_key: BTreeMap<String, Decision> = BTreeMap::new();
    let mut key_of: BTreeMap<i64, String> = BTreeMap::new();
    let mut key_of_entry: BTreeMap<String, String> = BTreeMap::new();
    for (id, e) in entries {
        let d = &e.data;
        let shared = !e.docket.starts_with("D-");
        match e.kind.as_str() {
            "question" => {
                if gate(d) {
                    continue;
                }
                let Some(m) = d["message"].as_i64() else { continue };
                let key = match id.filter(|_| shared) {
                    // Hubs' message numbers clash on a shared docket: its entry's id doesn't.
                    Some(id) => format!("#{}", &id.trim_start_matches("sha256:")[..8.min(id.len())]),
                    None => d["ask"].as_str().filter(|_| d["protocol"] == "build").map(str::to_string).unwrap_or_else(|| format!("#{m}")),
                };
                if let Some(id) = id {
                    key_of_entry.insert(id.to_string(), key.clone());
                }
                let question = d["subject"].as_str().map(str::to_string).filter(|s| !s.trim().is_empty()).unwrap_or_else(|| first_line(e.body.as_deref().unwrap_or("")));
                // A Build's subject leads with its key: "D-12 · D2 · Which store?"
                let question = question.rsplit(" · ").next().unwrap_or(&question).to_string();
                if !shared {
                    key_of.insert(m, key.clone());
                }
                if !by_key.contains_key(&key) {
                    order.push(key.clone());
                }
                let was = by_key.remove(&key);
                by_key.insert(
                    key.clone(),
                    Decision {
                        key,
                        question,
                        options: d["options"].as_array().map(|o| o.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default(),
                        asked_by: if d["from"] == "wheelhouse" { "Wheelhouse".into() } else { who(e) },
                        asked_at: was.as_ref().map(|w| w.asked_at.clone()).unwrap_or_else(|| e.at.clone()),
                        message: Some(m),
                        answer: None,
                        note: None,
                        by: None,
                        at: None,
                        standing: was.as_ref().and_then(|w| w.standing.clone()),
                    },
                );
            }
            "decision" if d["stood"] == true => {
                if let Some(x) = d["id"].as_str().and_then(|k| by_key.get_mut(k))
                    && x.answer.is_none()
                {
                    x.answer = d["label"].as_str().map(str::to_string).or_else(|| e.body.clone());
                    x.by = Some("stood".into());
                    x.at = Some(e.at.clone());
                }
            }
            "standing" => {
                if let Some(x) = d["key"].as_str().and_then(|k| by_key.get_mut(k)) {
                    x.standing = d["repo"].as_str().map(str::to_string);
                }
            }
            // One of yours, from the composer (`/decide`): no question
            // asked it, and the newest for a subject stands.
            "decision" if d["via"] == "composer" && d["replies_to"].is_null() && d["answers"].is_null() => {
                let Some(choice) = d["choice"].as_str().map(str::trim).filter(|c| !c.is_empty()) else { continue };
                let subject = d["subject"].as_str().map(str::trim).filter(|s| !s.is_empty()).unwrap_or("");
                let key = own_key(if subject.is_empty() { choice } else { subject });
                if !by_key.contains_key(&key) {
                    order.push(key.clone());
                }
                let standing = by_key.remove(&key).and_then(|w| w.standing);
                by_key.insert(
                    key.clone(),
                    Decision {
                        key,
                        question: subject.to_string(),
                        options: Vec::new(),
                        asked_by: who(e),
                        asked_at: e.at.clone(),
                        message: None,
                        answer: Some(choice.to_string()),
                        note: None,
                        by: Some(who(e)),
                        at: Some(e.at.clone()),
                        standing,
                    },
                );
            }
            "decision" | "note" => {
                let by_entry = d["answers"].as_str().and_then(|a| key_of_entry.get(a));
                let Some(k) = by_entry.or_else(|| d["replies_to"].as_i64().and_then(|m| key_of.get(&m))) else { continue };
                let Some(x) = by_key.get_mut(k) else { continue };
                let choice = d["choice"].as_str().map(str::to_string);
                let body = e.body.as_deref().map(str::trim).filter(|b| !b.is_empty()).map(str::to_string);
                x.answer = choice.clone().or_else(|| body.as_deref().map(first_line));
                x.note = body.filter(|b| Some(b) != choice.as_ref() && choice.is_some());
                x.by = Some(who(e));
                x.at = Some(e.at.clone());
            }
            _ => {}
        }
    }
    order.into_iter().filter_map(|k| by_key.remove(&k)).collect()
}

/// The decisions as the brief lists them: what stands, then what waits.
pub fn brief(ds: &[Decision]) -> String {
    let mut out = String::new();
    for d in ds.iter().filter(|d| d.answer.is_some()) {
        let by = match d.by.as_deref() {
            Some("stood") => "the recommendation, approved with the plan".to_string(),
            Some(b) => b.to_string(),
            None => "?".into(),
        };
        if d.question.trim().is_empty() {
            out.push_str(&format!("- **{}** ({by})\n", d.answer.as_deref().unwrap_or("")));
            continue;
        }
        out.push_str(&format!(
            "- {}: **{}** ({by}){}\n",
            d.question,
            d.answer.as_deref().unwrap_or(""),
            d.note.as_deref().map(|n| format!(". \"{}\"", first_line(n))).unwrap_or_default()
        ));
    }
    for d in ds.iter().filter(|d| d.answer.is_none()) {
        out.push_str(&format!("- {}: waiting on an answer{}\n", d.question, if d.options.is_empty() { String::new() } else { format!(" ({})", d.options.join(" / ")) }));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_standing_decision_reads_as_a_rule_in_the_notes() {
        let d = Decision {
            key: "#12".into(),
            question: "How many retries?".into(),
            options: vec!["3".into(), "5".into()],
            asked_by: "claude-code".into(),
            asked_at: "t".into(),
            message: Some(12),
            answer: Some("3".into()),
            note: Some("To match the API docs.".into()),
            by: Some("you".into()),
            at: Some("2026-11-18T01:00:00Z".into()),
            standing: None,
        };
        let r = rule(&d, "D-6");
        assert_eq!(r, "- How many retries? 3: To match the API docs. (Decided on D-6, November 18.)");
        assert_eq!(with_rule("", &r).unwrap(), format!("## Standing decisions\n\n{r}\n"));
        let once = with_rule("Run just check.", &r).unwrap();
        assert_eq!(once, format!("Run just check.\n\n## Standing decisions\n\n{r}\n"));
        assert_eq!(with_rule(&once, &r), None, "not twice");
        let r2 = "- Base? main. (Decided on D-7.)";
        assert_eq!(with_rule(&once, r2).unwrap(), format!("{once}{r2}\n"));
    }
    use crate::record::entry::Actor;

    fn e(kind: &str, typ: &str, id: &str, body: Option<&str>, data: Value, at: &str) -> Entry {
        Entry {
            v: 1,
            docket: "D-1".into(),
            kind: kind.into(),
            at: at.into(),
            actor: Actor { typ: typ.into(), id: id.into(), extra: Default::default() },
            body: body.map(str::to_string),
            data,
            blobs: vec![],
            replaces: None,
        }
    }

    #[test]
    fn the_answer_that_stands_with_who_gave_it() {
        let entries = vec![
            // An agent asks; you pick with words.
            e("question", "agent", "claude-code@b1", Some("Tests pass. Merge?"), json!({ "message": 3, "from": "s/a", "subject": "PR #12", "options": ["Merge", "Wait"] }), "t1"),
            e("decision", "person", "you", Some("but squash it"), json!({ "choice": "Merge", "replies_to": 3 }), "t2"),
            // A Build decision, asked twice; the second stands, answered in words.
            e(
                "question",
                "hub",
                "wheelhouse",
                Some("…"),
                json!({ "message": 5, "from": "wheelhouse", "subject": "D-1 · D1 · Which store?", "options": ["Postgres", "SQLite"], "ask": "D1", "protocol": "build" }),
                "t3",
            ),
            e(
                "question",
                "hub",
                "wheelhouse",
                Some("…"),
                json!({ "message": 6, "from": "wheelhouse", "subject": "D-1 · D1 · Which store?", "options": ["Postgres", "SQLite"], "ask": "D1", "protocol": "build" }),
                "t4",
            ),
            e("note", "person", "you", Some("SQLite, behind a trait"), json!({ "replies_to": 6 }), "t5"),
            // One the plan's approval settled.
            e(
                "question",
                "hub",
                "wheelhouse",
                Some("…"),
                json!({ "message": 7, "from": "wheelhouse", "subject": "D-1 · D2 · Retry cap?", "options": ["3", "5"], "ask": "D2", "protocol": "build" }),
                "t6",
            ),
            e("decision", "hub", "wheelhouse", Some("3"), json!({ "id": "D2", "stood": true, "label": "3" }), "t7"),
            // A gate is an approval, not a decision; an open question waits.
            e(
                "question",
                "hub",
                "wheelhouse",
                Some("…"),
                json!({ "message": 8, "from": "wheelhouse", "subject": "D-1 · plan", "options": ["Approve", "Revise", "Discard"], "ask": "plan", "protocol": "build" }),
                "t8",
            ),
            e("question", "agent", "codex@b2", Some("Rename the flag?"), json!({ "message": 9, "from": "s/b", "options": ["Yes", "No"] }), "t9"),
        ];
        let ds = of(&entries);
        let got: Vec<(&str, &str, Option<&str>, Option<&str>)> = ds.iter().map(|d| (d.key.as_str(), d.question.as_str(), d.answer.as_deref(), d.by.as_deref())).collect();
        assert_eq!(
            got,
            vec![
                ("#3", "PR #12", Some("Merge"), Some("you")),
                ("D1", "Which store?", Some("SQLite, behind a trait"), Some("you")),
                ("D2", "Retry cap?", Some("3"), Some("stood")),
                ("#9", "Rename the flag?", None, None),
            ]
        );
        assert_eq!(ds[0].note.as_deref(), Some("but squash it"));
        assert_eq!(ds[1].asked_at, "t3", "a re-ask keeps when it was first asked");
        assert_eq!(ds[3].asked_by, "Codex");
        let b = brief(&ds);
        assert!(b.contains("- PR #12: **Merge** (you). \"but squash it\""));
        assert!(b.contains("- Retry cap?: **3** (the recommendation, approved with the plan)"));
        assert!(b.contains("- Rename the flag?: waiting on an answer (Yes / No)"));
    }

    #[test]
    fn your_own_decisions_stand_newest_per_subject() {
        let mine = |subject: Option<&str>, choice: &str, at: &str| {
            let mut data = json!({ "via": "composer", "choice": choice });
            if let Some(s) = subject {
                data["subject"] = json!(s);
            }
            e("decision", "person", "you", Some(choice), data, at)
        };
        let es = vec![
            mine(Some("Retries"), "5xx only", "2026-11-25T10:00:00Z"),
            mine(None, "Use Postgres", "2026-11-25T10:01:00Z"),
            mine(Some("retries?"), "5xx and timeouts", "2026-11-25T10:02:00Z"),
            // Not one of yours from the composer: a reply's, and a malformed one.
            e("decision", "person", "you", Some("x"), json!({ "choice": "x" }), "2026-11-25T10:03:00Z"),
            mine(Some("Empty"), "  ", "2026-11-25T10:04:00Z"),
        ];
        let ds = of(&es);
        assert_eq!(ds.len(), 2, "{ds:?}");
        assert_eq!((ds[0].question.as_str(), ds[0].answer.as_deref(), ds[0].by.as_deref()), ("retries?", Some("5xx and timeouts"), Some("you")));
        assert_eq!(ds[0].key, own_key("Retries"), "the same subject, however it's written");
        assert_eq!((ds[1].question.as_str(), ds[1].answer.as_deref()), ("", Some("Use Postgres")));
        assert_eq!(brief(&ds), "- retries?: **5xx and timeouts** (you)\n- **Use Postgres** (you)\n");
        let r = rule(&ds[1], "D-6");
        assert!(r.starts_with("- Use Postgres. (Decided on D-6"), "{r}");
    }

    #[test]
    fn on_a_shared_docket_answers_find_their_question_by_its_entry() {
        let mk = |kind: &str, typ: &str, id: &str, data: serde_json::Value| {
            let mut x = Entry::new("ACME-1", kind, "2026-11-24T10:00:00.000Z", crate::record::entry::Actor::new(typ, id));
            x.data = data;
            x
        };
        // Two hubs' questions, each numbered #3 on its own hub.
        let q1 = mk("question", "agent", "codex@sam", json!({ "message": 3, "from": "api/agent", "to": "me", "subject": "Empty bucket", "options": ["429", "503"] }));
        let q2 = mk(
            "question",
            "agent",
            "claude-code@jordan",
            json!({ "message": 3, "from": "api/agent", "to": "me", "subject": "Header names", "options": ["X-RateLimit", "RateLimit"] }),
        );
        // Jordan answers Sam's agent's question from his own copy, #7 on his hub.
        let a = mk("decision", "person", "jordan", json!({ "message": 9, "replies_to": 7, "answers": "sha256:aaaa1111", "choice": "429" }));
        let ids = ["sha256:aaaa1111", "sha256:bbbb2222", "sha256:cccc3333"];
        let ds = fold(ids.iter().map(|i| Some(*i)).zip([&q1, &q2, &a]));
        assert_eq!(ds.len(), 2, "two questions, not one");
        assert_eq!(ds[0].question, "Empty bucket");
        assert_eq!(ds[0].answer.as_deref(), Some("429"));
        assert!(ds[1].answer.is_none());
    }
}
