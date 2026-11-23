//! The round rules: what an observation of a PR on GitHub does to its
//! review state. Pure, ported from rex-reviewer's `rules.lua` in its order,
//! with one change: a PR reopened after its cleanup is reviewed again.

use serde_json::{Value, json};

use super::pr::Pr;

/// A PR as GitHub showed it on one check.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Obs {
    pub repo: String,
    pub pr: i64,
    pub title: Option<String>,
    pub url: Option<String>,
    pub author: Option<String>,
    /// `OPEN`, `MERGED` or `CLOSED`.
    pub state: String,
    pub head: Option<String>,
    /// It asks you, by name, for a review.
    pub requested: bool,
    /// When it last asked you.
    pub requested_at: Option<String>,
}

/// Something that happened to a PR, for its docket.
#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    pub kind: String,
    pub data: Value,
}

fn ev(out: &mut Vec<Event>, kind: &str, data: Value) {
    out.push(Event { kind: kind.to_string(), data });
}

fn newer(a: Option<&str>, b: Option<&str>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => a > b,
        _ => true,
    }
}

/// What this observation does to the PR's state (`None`: a PR we don't
/// track that doesn't ask for you), and what happened.
pub fn decide(e: Option<Pr>, o: &Obs, now: &str) -> (Option<Pr>, Vec<Event>) {
    let mut out = Vec::new();
    let mut e = match e {
        Some(e) => e,
        None => {
            if !o.requested {
                return (None, out);
            }
            let mut e = Pr::new(&o.repo, o.pr);
            e.trigger = Some("request".into());
            e.created_at = Some(now.to_string());
            e.requested_at = Some(o.requested_at.clone().unwrap_or_else(|| now.to_string()));
            ev(&mut out, "requested", json!({ "round": 1, "head": o.head, "author": o.author, "title": o.title }));
            e
        }
    };
    if o.title.is_some() {
        e.title = o.title.clone();
    }
    if o.url.is_some() {
        e.url = o.url.clone();
    }
    if o.author.is_some() {
        e.author = o.author.clone();
    }

    if o.state == "MERGED" || o.state == "CLOSED" {
        let s = o.state.to_lowercase();
        if e.state != s && e.state != "cleaned" {
            e.state = s.clone();
            e.outcome = Some(s.clone());
            e.ended_at = Some(now.to_string());
            ev(&mut out, &s, json!({ "head": o.head }));
        }
    } else if e.state == "posted" {
        // A new request: newer than the post, and not the request that
        // started the last round (whatever the clocks say).
        let same_request = o.requested_at.is_some() && o.requested_at == e.requested_at;
        if o.requested && !same_request && newer(o.requested_at.as_deref(), e.posted_at.as_deref()) {
            e.state = "queued".into();
            e.trigger = Some("re-request".into());
            e.requested_at = Some(o.requested_at.clone().unwrap_or_else(|| now.to_string()));
            ev(&mut out, "re-requested", json!({ "round": e.round + 1, "head": o.head, "reviewed_head": e.reviewed_head }));
        } else if let (Some(h), Some(was)) = (&o.head, &e.head)
            && h != was
        {
            ev(&mut out, "new-head", json!({ "head": h, "previous_head": was, "note": "waiting for a re-request" }));
        }
    } else if e.state == "queued" && e.trigger.as_deref() != Some("manual") && !o.requested {
        e.state = "unrequested".into();
        ev(&mut out, "unrequested", json!({ "head": o.head }));
    } else if e.state == "unrequested" && o.requested {
        e.state = "queued".into();
        e.requested_at = Some(o.requested_at.clone().unwrap_or_else(|| now.to_string()));
        ev(&mut out, "requested", json!({ "round": e.round + 1, "head": o.head }));
    } else if e.state == "reviewing" {
        if let (Some(h), Some(rh)) = (&o.head, &e.round_head)
            && h != rh
            && Some(h) != e.head.as_ref()
        {
            ev(&mut out, "head-moved", json!({ "round": e.round, "head": h, "round_head": rh }));
        }
    } else if e.state == "cleaned" && o.state == "OPEN" && o.requested && newer(o.requested_at.as_deref(), e.cleaned_at.as_deref()) {
        // Reopened, and asking again: rex-reviewer never looked again.
        e.state = "queued".into();
        e.trigger = Some("request".into());
        e.outcome = None;
        e.ended_at = None;
        e.requested_at = Some(o.requested_at.clone().unwrap_or_else(|| now.to_string()));
        ev(&mut out, "requested", json!({ "round": e.round + 1, "head": o.head, "note": "reopened" }));
    }
    if o.head.is_some() {
        e.head = o.head.clone();
    }
    (Some(e), out)
}

/// A round starts on `head`.
pub fn mark_started(e: &mut Pr, head: &str, now: &str) {
    e.round += 1;
    e.state = "reviewing".into();
    e.round_head = Some(head.to_string());
    e.started_at = Some(now.to_string());
}

/// What a round ended with: posted (`verdict` is GitHub's review state) or
/// dismissed (`DISMISSED`, by you).
#[derive(Default)]
pub struct Ended {
    pub head: Option<String>,
    pub verdict: String,
    pub review_id: Option<Value>,
    pub url: Option<String>,
    pub by: Option<String>,
    pub blocking: Option<i64>,
    pub non_blocking: Option<i64>,
    pub merge_risk: Option<String>,
    pub implementation: Option<String>,
    /// Tokens so far, by agent: `{claude: {input, …, total}, total}`.
    pub tokens: Option<Value>,
    pub reviewers: Option<Value>,
    pub checker_effort: Option<String>,
}

pub const TOKEN_FIELDS: [&str; 5] = ["input", "cache_read", "cache_write", "output", "total"];

/// Tokens summed across agents.
pub fn flat_tokens(t: &Value) -> Value {
    let mut sum = serde_json::Map::new();
    for f in TOKEN_FIELDS {
        let n: i64 = ["codex", "claude", "agy"].iter().map(|a| t[a][f].as_i64().unwrap_or(0)).sum();
        sum.insert(f.into(), json!(n));
    }
    Value::Object(sum)
}

fn token_delta(before: &Value, after: &Value) -> Value {
    let mut d = serde_json::Map::new();
    for f in TOKEN_FIELDS {
        d.insert(f.into(), json!((after[f].as_i64().unwrap_or(0) - before[f].as_i64().unwrap_or(0)).max(0)));
    }
    Value::Object(d)
}

/// The round is over, posted or dismissed: record it in the history.
pub fn mark_posted(e: &mut Pr, f: Ended, now: &str) {
    let before = match &e.tokens {
        Some(t) => Some(flat_tokens(t)),
        None if e.round == 1 => Some(flat_tokens(&json!({}))),
        None => None,
    };
    let round_tokens = match (&f.tokens, before) {
        (Some(t), Some(b)) => Some(token_delta(&b, &flat_tokens(t))),
        _ => None,
    };
    if f.tokens.is_some() {
        e.tokens = f.tokens.clone();
    }
    e.state = "posted".into();
    e.reviewed_head = f.head.clone().or_else(|| e.round_head.clone());
    if e.head.is_none() {
        e.head = e.reviewed_head.clone();
    }
    e.posted_at = Some(now.to_string());
    e.verdict = Some(f.verdict.clone());
    e.review_id = f.review_id.clone();
    e.review_url = f.url.clone();
    if f.implementation.is_some() {
        e.implementation = f.implementation.clone();
    }
    let mut h = json!({
        "round": e.round, "trigger": e.trigger, "head": e.reviewed_head, "verdict": f.verdict,
        "review_id": f.review_id, "url": f.url, "by": f.by.unwrap_or_else(|| "agents".into()),
        "requested_at": e.requested_at, "started_at": e.started_at, "at": now,
        "blocking": f.blocking, "non_blocking": f.non_blocking, "merge_risk": f.merge_risk,
        "tokens": round_tokens, "reviewers": f.reviewers, "checker_effort": f.checker_effort,
    });
    if let Some(o) = h.as_object_mut() {
        o.retain(|_, v| !v.is_null());
    }
    e.history.push(h);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obs(requested: bool, at: Option<&str>, head: &str) -> Obs {
        Obs {
            repo: "o/api".into(),
            pr: 7,
            title: Some("Retry webhooks".into()),
            state: "OPEN".into(),
            head: Some(head.into()),
            requested,
            requested_at: at.map(str::to_string),
            ..Default::default()
        }
    }

    fn kinds(evs: &[Event]) -> Vec<&str> {
        evs.iter().map(|e| e.kind.as_str()).collect()
    }

    #[test]
    fn a_request_queues_and_a_post_waits_for_a_re_request() {
        let (e, evs) = decide(None, &obs(false, None, "a1"), "t0");
        assert!(e.is_none() && evs.is_empty(), "not asking: nothing to track");

        let (e, evs) = decide(None, &obs(true, Some("2026-11-01T10:00:00Z"), "a1"), "2026-11-01T10:00:05Z");
        let mut e = e.unwrap();
        assert_eq!((e.state.as_str(), e.round, e.head.as_deref()), ("queued", 0, Some("a1")));
        assert_eq!(kinds(&evs), ["requested"]);

        mark_started(&mut e, "a1", "2026-11-01T10:01:00Z");
        assert_eq!((e.state.as_str(), e.round), ("reviewing", 1));
        mark_posted(&mut e, Ended { verdict: "CHANGES_REQUESTED".into(), ..Default::default() }, "2026-11-01T11:00:00Z");
        assert_eq!((e.state.as_str(), e.reviewed_head.as_deref()), ("posted", Some("a1")));
        assert_eq!(e.history.len(), 1);

        // A push alone is noted, nothing starts.
        let (e2, evs) = decide(Some(e.clone()), &obs(false, None, "b2"), "2026-11-01T12:00:00Z");
        assert_eq!(kinds(&evs), ["new-head"]);
        let e = e2.unwrap();
        assert_eq!(e.state, "posted");

        // Asked again after the post: the next round.
        let (e, evs) = decide(Some(e), &obs(true, Some("2026-11-01T13:00:00Z"), "b2"), "2026-11-01T13:00:05Z");
        let e = e.unwrap();
        assert_eq!((e.state.as_str(), e.trigger.as_deref()), ("queued", Some("re-request")));
        assert_eq!(evs[0].data["round"], 2);
    }

    #[test]
    fn a_dismissed_round_doesnt_restart_while_the_old_request_stands() {
        let (e, _) = decide(None, &obs(true, Some("2026-11-01T10:00:00Z"), "a1"), "2026-11-01T10:00:00Z");
        let mut e = e.unwrap();
        mark_started(&mut e, "a1", "2026-11-01T10:01:00Z");
        mark_posted(&mut e, Ended { verdict: "DISMISSED".into(), by: Some("you".into()), ..Default::default() }, "2026-11-01T10:30:00Z");
        // Still requested on GitHub (nothing was posted), with the old time.
        let (e, evs) = decide(Some(e), &obs(true, Some("2026-11-01T10:00:00Z"), "a1"), "2026-11-01T10:31:00Z");
        assert_eq!(e.unwrap().state, "posted");
        assert!(evs.is_empty());
    }

    #[test]
    fn the_request_that_started_a_round_never_starts_another() {
        // A request stamped after the post (a clock ahead of ours) must not
        // loop: it's the same request.
        let (e, _) = decide(None, &obs(true, Some("2026-11-01T23:59:00Z"), "a1"), "2026-11-01T10:00:00Z");
        let mut e = e.unwrap();
        mark_started(&mut e, "a1", "2026-11-01T10:01:00Z");
        mark_posted(&mut e, Ended { verdict: "APPROVED".into(), ..Default::default() }, "2026-11-01T10:30:00Z");
        let (e, evs) = decide(Some(e), &obs(true, Some("2026-11-01T23:59:00Z"), "a1"), "2026-11-01T10:31:00Z");
        assert_eq!(e.unwrap().state, "posted");
        assert!(evs.is_empty());
    }

    #[test]
    fn unrequested_requested_head_moved_and_merged() {
        let (e, _) = decide(None, &obs(true, Some("2026-11-01T10:00:00Z"), "a1"), "t");
        let (e, evs) = decide(e, &obs(false, None, "a1"), "t");
        assert_eq!((e.as_ref().unwrap().state.as_str(), kinds(&evs)), ("unrequested", vec!["unrequested"]));
        let (e, evs) = decide(e, &obs(true, Some("2026-11-01T11:00:00Z"), "a1"), "t");
        let mut e = e.unwrap();
        assert_eq!((e.state.as_str(), kinds(&evs)), ("queued", vec!["requested"]));

        // A manual pickup stays queued when nobody asks.
        let mut m = e.clone();
        m.trigger = Some("manual".into());
        let (m, evs) = decide(Some(m), &obs(false, None, "a1"), "t");
        assert_eq!(m.unwrap().state, "queued");
        assert!(evs.is_empty());

        mark_started(&mut e, "a1", "t");
        let (e, evs) = decide(Some(e), &obs(false, None, "c3"), "t");
        assert_eq!(kinds(&evs), ["head-moved"]);
        // Once per new head.
        let (e, evs) = decide(e, &obs(false, None, "c3"), "t");
        assert!(evs.is_empty());

        let mut merged = obs(false, None, "c3");
        merged.state = "MERGED".into();
        let (e, evs) = decide(e, &merged, "t9");
        let e = e.unwrap();
        assert_eq!((e.state.as_str(), e.outcome.as_deref(), kinds(&evs)), ("merged", Some("merged"), vec!["merged"]));
    }

    #[test]
    fn a_reopened_pr_that_asks_again_is_reviewed_again() {
        let mut e = Pr::new("o/api", 7);
        e.state = "cleaned".into();
        e.round = 2;
        e.outcome = Some("closed".into());
        e.cleaned_at = Some("2026-11-02T00:00:00Z".into());
        // The old request is stale.
        let (e2, evs) = decide(Some(e.clone()), &obs(true, Some("2026-11-01T00:00:00Z"), "a1"), "t");
        assert_eq!(e2.unwrap().state, "cleaned");
        assert!(evs.is_empty());
        let (e2, evs) = decide(Some(e), &obs(true, Some("2026-11-03T00:00:00Z"), "a1"), "t");
        let e2 = e2.unwrap();
        assert_eq!((e2.state.as_str(), e2.outcome.as_deref()), ("queued", None));
        assert_eq!(evs[0].data["round"], 3);
    }

    #[test]
    fn a_rounds_tokens_are_what_it_added() {
        let mut e = Pr::new("o/api", 7);
        mark_started(&mut e, "a1", "t");
        let t1 = json!({ "claude": { "input": 10, "cache_read": 100, "cache_write": 5, "output": 20, "total": 135 }, "total": 135 });
        mark_posted(&mut e, Ended { verdict: "APPROVED".into(), tokens: Some(t1), ..Default::default() }, "t1");
        assert_eq!(e.history[0]["tokens"]["total"], 135);
        e.state = "queued".into();
        mark_started(&mut e, "b2", "t2");
        let t2 = json!({ "claude": { "input": 15, "cache_read": 300, "cache_write": 5, "output": 30, "total": 350 }, "total": 350 });
        mark_posted(&mut e, Ended { verdict: "APPROVED".into(), tokens: Some(t2), ..Default::default() }, "t3");
        assert_eq!(e.history[1]["tokens"]["total"], 215);
        assert_eq!(e.history[1]["tokens"]["cache_write"], 0);
    }
}
