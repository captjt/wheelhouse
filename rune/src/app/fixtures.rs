//! Scenes for headless shots of what the made-up world doesn't have: a
//! Build at each of its steps, a review round waiting on your call, a
//! checker's verdicts. Each lays made-up data over the hub's state (or a
//! docket's page) for that run only (`Store::amend`); nothing is written.

use std::sync::Arc;

use chrono::{Duration, Utc};
use serde_json::{Value, json};

use super::{Main, Place, Sheet, chat, recap, record};
use crate::model::DocketPage;

fn ago(min: i64) -> String {
    (Utc::now() - Duration::minutes(min)).format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn show(s: &mut Main, p: Place, id: &str) {
    s.place = p;
    s.selection.insert(s.place.clone(), id.to_string());
    s.load_page(id);
}

/// Change a docket's page (its evidence, say) for this run.
fn amend_page(s: &mut Main, id: &str, f: impl FnOnce(&mut Value)) {
    s.load_page(id);
    let Some(p) = s.pages.get(id) else { return };
    let mut j = p.j.clone();
    f(&mut j);
    s.pages.insert(id.to_string(), Arc::new(DocketPage::parse(j)));
}

// ---- Recap, Chat, the record, Verify, Shortcuts: the world as it is ----

pub fn recap(s: &mut Main) {
    s.place = Place::Recap;
    recap::load(s);
}

pub fn chat_peek(s: &mut Main) {
    show(s, Place::Active, "D-6");
    s.chat = true;
    s.talk.peek = Some("D-6".into());
    chat::peek(s);
}

pub fn chat_blocked(s: &mut Main) {
    show(s, Place::Active, "D-1");
    s.chat = true;
}

pub fn pane(s: &mut Main) {
    show(s, Place::Active, "D-6");
    s.chat = true;
    chat::open_pane(s, "D-6");
}

pub fn record_open(s: &mut Main) {
    show(s, Place::Active, "D-6");
    s.sheet = Some(Sheet::Record("D-6".into()));
    record::poll(s);
    s.record.explain = true;
    s.record.open.insert(14);
    record::envelope(s, "D-6", 14);
}

pub fn record_find(s: &mut Main) {
    show(s, Place::Active, "D-6");
    s.sheet = Some(Sheet::Record("D-6".into()));
    record::poll(s);
    s.record.mark = "witnessed";
    s.record.find = rune::TextEdit::new("check");
}

pub fn output(s: &mut Main) {
    show(s, Place::Active, "D-6");
    s.sheet = Some(Sheet::CheckOutput("D-6".into(), "lint".into()));
}

pub fn close(s: &mut Main) {
    show(s, Place::Active, "D-1");
    super::verify::close_on_word(s, "D-1", crate::model::Status::InProgress);
}

pub fn shortcuts(s: &mut Main) {
    s.place = Place::Inbox;
    if let Some(id) = s.current() {
        s.load_page(&id);
    }
    s.sheet = Some(Sheet::Shortcuts);
}

/// D-6's Verify, with a checker's verdicts and a close on your word.
pub fn verdicts(s: &mut Main) {
    show(s, Place::Active, "D-6");
    amend_page(s, "D-6", |j| {
        j["evidence"]["verdicts"] = json!([
            {"seq": 15, "at": ago(3), "n": 1, "verdict": "pass", "findings": 0, "by": "codex@local", "role": "checker", "independent": true, "head": "d905eb8988d1", "mark": "said",
             "summary": "Retries stop at 3 with jitter, and `deliver.test.ts` covers a 5xx, a timeout and a 4xx."},
            {"seq": 16, "at": ago(2), "n": 2, "verdict": "fail", "findings": [{"text": "a"}, {"text": "b"}], "by": "claude-code@local", "role": "checker", "independent": false, "mark": "said",
             "summary": "The retry log line has no attempt number yet: the **third acceptance item** isn't met."}
        ]);
        j["evidence"]["closed_on"] = json!("The retries shipped in #212; logging is its own docket.");
    });
}

// ---- Review: a round waiting on your call ----

fn review_round(j: &mut Value, phase: &str, ready: bool) {
    let Some(prs) = j["review"]["prs"].as_array_mut() else { return };
    let Some(p) = prs.iter_mut().find(|p| p["docket"] == "D-8") else { return };
    p["state"] = json!("reviewing");
    p["phase"] = json!(phase);
    p["round"] = json!(1);
    p["reviewers"] = json!([{"name": "claude", "agent": "claude", "effort": "high"}, {"name": "codex", "agent": "codex", "effort": "high"}]);
    p["reviewers_total"] = json!(2);
    p["reviewers_done"] = json!(if phase == "reviewers" { 1 } else { 2 });
    p["checker_effort"] = json!("high");
    p["updated_at"] = json!(ago(4));
    if ready {
        p["attention"] = json!({"kind": "ready", "text": "Round 1 is checked and waits for you to post it.", "round": 1});
        p["draft"] = json!({"event": "REQUEST_CHANGES", "verdict": "A bundle can stack with an item discount when the item is already on sale: the larger of the two should win.",
            "blocking": 1, "non_blocking": 2, "merge_risk": "medium", "digest": "sha256:5e1b0c0de"});
        p["post_decision"] = json!({"auto": false, "why": "acme/storefront asks before posting"});
    }
}

pub fn review_ready(s: &mut Main) {
    s.store.amend(|j| review_round(j, "awaiting", true));
    show(s, Place::Reviews, "D-8");
}

pub fn review_live(s: &mut Main) {
    s.store.amend(|j| review_round(j, "reviewers", false));
    show(s, Place::Reviews, "D-8");
}

pub fn preview(s: &mut Main) {
    review_ready(s);
    s.panels.preview = Some((
        "**Request changes** · 1 blocking, 2 non-blocking · merge risk medium\n\n### Blocking\n\n1. `applyBundle` stacks the bundle price with an item discount when the item is already on sale. The intent says the larger of the two wins, never both.\n\n### Non-blocking\n\n- `bundle.test.ts`: add a case with a sale item in the bundle.\n- `pricing.ts:42`: name the rounding step.\n\n---\n\n<sub>Reviewed by claude and codex, checked by claude · Docket: D-8 @ 5f66825d</sub>".into(),
        Some("sha256:5e1b0c0de".into()),
    ));
    s.sheet = Some(Sheet::Preview("acme/storefront#214".into()));
}

// ---- Build: its steps, from plan to PR, and an address round ----

fn build_task(phase: &str) -> Value {
    json!({
        "docket": "D-4", "key": "acme/storefront@D-4", "title": "Bundle discounts", "repo": "acme/storefront",
        "state": "active", "phase": phase, "branch": "wh/d-4-bundle-discounts", "base": "main",
        "plan_version": 2, "check_n": 2, "fix_used": 1, "rounds": 0, "round": null, "asks": [],
        "builder": {"agent": "claude", "effort": "high"}, "checker": {"agent": "codex", "effort": "medium"},
        "last_check": {"n": 1, "verdict": "fail", "summary": "The cart test doesn't cover a bundle with an item already on sale.", "findings": [{"text": "a"}, {"text": "b"}]},
        "decisions": [
            {"id": "q1", "question": "Round the bundle price to cents before comparing?", "answered": true, "label": "Yes, to cents"},
            {"id": "q2", "question": "Show which discount won on the receipt?", "answered": false}
        ],
        "updated_at": ago(2), "attention": null
    })
}

fn with_build(s: &mut Main, task: Value, message: Option<Value>) {
    s.store.amend(move |j| {
        j["build"]["tasks"] = json!([task]);
        if let (Some(m), Some(list)) = (message, j["messages"].as_array_mut()) {
            list.push(m);
        }
    });
    show(s, Place::Active, "D-4");
}

/// The plan, waiting on your call: the Build asks with a message.
pub fn build_plan(s: &mut Main) {
    let mut t = build_task("plan_gate");
    t["asks"] = json!(["plan"]);
    t["last_check"] = Value::Null;
    t["check_n"] = json!(0);
    t["fix_used"] = json!(0);
    let m = json!({"id": 90, "docket": "D-4", "from": "wheelhouse", "subject": "Plan v2: your call", "created_at": ago(1),
        "body": "The builder's plan for **D-4**, version 2:\n\n1. `applyBundle` compares the bundle price with the item discounts, in cents, and takes the larger.\n2. The cart test gains a bundle with an item on sale.\n\nApprove it and the builder starts; ask for changes and it plans again.",
        "options": ["Approve the plan", "Ask for changes"]});
    with_build(s, t, Some(m));
}

pub fn build_check(s: &mut Main) {
    with_build(s, build_task("checking"), None);
}

/// An address round on the open PR: feedback in, your call to push.
pub fn build_round(s: &mut Main) {
    let mut t = build_task("push_gate");
    t["state"] = json!("open");
    t["rounds"] = json!(1);
    t["pr_number"] = json!(231);
    t["pr_url"] = json!("https://github.com/acme/storefront/pull/231");
    t["round"] = json!({"n": 1, "summary": "2 review comments addressed, 1 replied to", "update": "rebase"});
    t["attention"] = json!({"kind": "stalled", "text": "The checker hasn't written for 20 minutes.", "round": 1});
    with_build(s, t, None);
}
