//! Verify (milestone 4): what a docket's work rests on, and what needs your
//! attention beyond your asks.
//!
//! - **Evidence.** A docket with acceptance items in its intent can't be
//!   closed on what was said (DESIGN principle 4). Its done needs something
//!   witnessed (its required checks passed at its latest commit, or its PR
//!   merged) or checkable (its Linear issue completed, its review posted on
//!   GitHub). Without it, you can still close it on your word, with a
//!   reason, and the record says so.
//! - **Verdicts.** A checker's judgment is *said*, by the checker, and the
//!   record shows whether it came from a session other than the builder's.
//! - **Attention.** What's stuck, failing, held, or quiet, across dockets.

use serde_json::{Value, json};

use crate::hub::Core;
use crate::record::{DocketState, Entry};

/// Whether a docket's done needs evidence: its intent has acceptance items
/// (task boxes) to meet.
pub fn needs(st: &DocketState) -> bool {
    st.intent.as_ref().is_some_and(|i| {
        i.body.lines().any(|l| {
            let t = l.trim_start().trim_start_matches(['-', '*', '+']).trim_start();
            t.starts_with("[ ]") || t.starts_with("[x]") || t.starts_with("[X]")
        })
    })
}

/// What a docket's done can rest on, newest first: `{how, what, seq, at}`.
pub fn of(st: &DocketState, entries: &[(i64, Entry)]) -> Vec<Value> {
    let mut out = Vec::new();
    // Its required checks, all passing at one commit.
    let req: Vec<(&String, &crate::record::fold::CheckBrief)> = st.checks.iter().filter(|(_, c)| c.required).collect();
    if !req.is_empty() && req.iter().all(|(_, c)| c.ok) && req.windows(2).all(|w| w[0].1.head == w[1].1.head) {
        let head: String = req[0].1.head.chars().take(7).collect();
        let names: Vec<String> = req.iter().map(|(n, _)| format!("`{n}`")).collect();
        let at = req.iter().map(|(_, c)| c.at.clone()).max().unwrap_or_default();
        let seq = entries.iter().rev().find(|(_, e)| e.kind == "check").map(|(s, _)| *s);
        out.push(json!({ "how": "witnessed", "what": format!("{} passed at {head}", names.join(", ")), "seq": seq, "at": at }));
    }
    for (seq, e) in entries.iter().rev() {
        let d = &e.data;
        let line = match (e.kind.as_str(), d["what"].as_str(), d["event"].as_str()) {
            ("observed", Some("build-event"), Some("merged")) => Some(("witnessed", format!("Its PR{} merged", d["pr"].as_i64().map(|n| format!(" #{n}")).unwrap_or_default()))),
            ("observed", Some("review-event"), Some("merged")) => Some(("witnessed", "The PR merged".to_string())),
            ("observed", Some("review-event"), Some("posted")) => Some(("witnessed", "The review posted".to_string())),
            ("link", _, _) if d["kind"] == "github-review" => Some(("checkable", "The review is on GitHub".to_string())),
            ("observed", Some("linear"), _) if d["issue"]["state_type"] == "completed" => {
                Some(("checkable", format!("{} is {} in Linear", d["issue"]["key"].as_str().unwrap_or("The issue"), d["issue"]["state"].as_str().unwrap_or("done"))))
            }
            _ => None,
        };
        if let Some((how, what)) = line
            && !out.iter().any(|o| o["what"] == what)
        {
            out.push(json!({ "how": how, "what": what, "seq": seq, "at": e.at }));
        }
    }
    out
}

/// The docket page's Verify: its checks, its verdicts, and what its done
/// rests on.
pub fn view(core: &Core, st: &DocketState, entries: &[(i64, Entry)]) -> Value {
    let checks: Vec<Value> = {
        let mut last: std::collections::BTreeMap<String, Value> = Default::default();
        for (seq, e) in entries.iter().filter(|(_, e)| e.kind == "check") {
            let mut d = e.data.clone();
            d["seq"] = json!(seq);
            d["at"] = json!(e.at);
            if let Some(n) = d["name"].as_str() {
                last.insert(n.to_string(), d);
            }
        }
        last.into_values().collect()
    };
    let verdicts: Vec<Value> = entries
        .iter()
        .filter(|(_, e)| e.kind == "verdict")
        .map(|(seq, e)| {
            json!({
                "seq": seq, "at": e.at, "n": e.data["n"], "verdict": e.data["verdict"], "findings": e.data["findings"],
                "summary": e.body, "by": e.actor.id, "role": e.data["role"], "independent": e.data["independent"],
                "head": e.data["head"], "mark": crate::record::mark(e),
            })
        })
        .collect();
    let task = crate::build::get(core, &st.id).ok().flatten();
    let (repo, specs) = match &task {
        Some(t) => (Some(t.repo.clone()), core.build.specs(&t.repo).unwrap_or_default()),
        None => st.home_cwd().and_then(|c| core.build.repo_of(&crate::paths::expand(c))).map(|(r, s)| (Some(r), s)).unwrap_or_default(),
    };
    json!({
        "needs": needs(st),
        "evidence": of(st, entries),
        "checks": checks,
        "repo": repo,
        "configured": specs.iter().map(|s| json!({ "name": s.name, "run": s.run, "required": s.required })).collect::<Vec<_>>(),
        "running": crate::checks::running(&st.id),
        "verdicts": verdicts,
        "hold": st.hold,
        "closed_on": st.closed_on,
    })
}

/// What needs your attention beyond your asks, worst first.
pub fn attention(core: &Core, states: &[DocketState]) -> Vec<Value> {
    let now = time::OffsetDateTime::now_utc();
    let age = |at: &str| crate::time::parse(at).map(|t| (now - t).whole_seconds()).unwrap_or(0);
    let mut out: Vec<(u8, Value)> = Vec::new();
    for st in states {
        let mut push = |rank: u8, kind: &str, text: String| out.push((rank, json!({ "docket": st.id, "title": st.title, "kind": kind, "text": text, "at": st.updated_at })));
        if let Some(a) = core.build.attention.get(&st.id) {
            push(0, "build", format!("Build: {}", a.text));
        }
        if let Some(why) = &st.hold {
            push(2, "held", format!("On hold: {why}"));
        }
        if st.is_open() {
            // Required ones: an optional check only informs.
            let failing: Vec<&String> = st.checks.iter().filter(|(_, c)| !c.ok && c.required).map(|(n, _)| n).collect();
            if !failing.is_empty() {
                push(1, "failing", format!("Failing: {}", failing.iter().map(|n| format!("`{n}`")).collect::<Vec<_>>().join(", ")));
            }
            let live = crate::agents::lead(core, st).and_then(|b| crate::agents::live(core, &b));
            if let Some(l) = &live
                && l.state == "working"
                && l.since.as_deref().is_some_and(|s| age(s) > 3600)
            {
                push(3, "long", format!("Working for {}h without finishing a turn", age(l.since.as_deref().unwrap_or("")) / 3600));
            }
            if live.is_none() && st.hold.is_none() && age(&st.updated_at) > 7 * 86400 {
                push(4, "quiet", format!("Nothing for {} days, and no agent on it", age(&st.updated_at) / 86400));
            }
        }
        if st.status == "done"
            && let Some(why) = &st.closed_on
            && st.ended_at.as_deref().is_some_and(|e| age(e) < 7 * 86400)
        {
            push(5, "word", format!("Closed on your word: {why}"));
        }
    }
    for (key, a) in &core.review.attention {
        let d = crate::review::docket_of(core, key);
        out.push((0, json!({ "docket": d, "title": key, "kind": "review", "text": format!("Review of {key}: {}", a.text), "at": null })));
    }
    out.sort_by_key(|(r, _)| *r);
    out.into_iter().map(|(_, v)| v).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::entry::Actor;
    use crate::record::fold::CheckBrief;

    fn e(kind: &str, data: Value) -> Entry {
        Entry {
            v: 1,
            docket: "D-1".into(),
            kind: kind.into(),
            at: "2026-11-18T01:00:00Z".into(),
            actor: Actor { typ: "hub".into(), id: "hub@local".into(), extra: Default::default() },
            body: None,
            data,
            blobs: vec![],
            replaces: None,
        }
    }

    #[test]
    fn done_rests_on_what_was_witnessed_or_can_be_checked() {
        let mut st = DocketState::default();
        assert!(!needs(&st));
        st.intent = Some(crate::record::fold::IntentRef { body: "Retries.\n\n## Acceptance\n- [ ] A 5xx retries".into(), ..Default::default() });
        assert!(needs(&st));
        assert!(of(&st, &[]).is_empty(), "said isn't evidence");
        st.checks.insert("test".into(), CheckBrief { ok: true, head: "abc1234ff".into(), at: "t".into(), required: true });
        st.checks.insert("lint".into(), CheckBrief { ok: false, head: "abc1234ff".into(), at: "t".into(), required: false });
        let ev = of(&st, &[]);
        assert_eq!(ev[0]["what"], "`test` passed at abc1234", "an optional check's failure doesn't count against it");
        st.checks.insert("test".into(), CheckBrief { ok: false, head: "abc1234ff".into(), at: "t".into(), required: true });
        let entries = vec![
            (5, e("observed", json!({ "what": "build-event", "event": "merged", "pr": 45 }))),
            (6, e("observed", json!({ "what": "linear", "issue": { "key": "ENG-7", "state": "Done", "state_type": "completed" } }))),
        ];
        let ev = of(&st, &entries);
        let whats: Vec<&str> = ev.iter().filter_map(|x| x["what"].as_str()).collect();
        assert_eq!(whats, vec!["ENG-7 is Done in Linear", "Its PR #45 merged"]);
        assert_eq!(ev[0]["how"], "checkable");
    }
}
