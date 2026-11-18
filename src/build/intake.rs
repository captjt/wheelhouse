//! Linear intake (`sources.linear` in build.lua). Issues assigned to you, in
//! the teams it maps to repos: one with the label starts a Build (or, with
//! `gates.start.label = "ask"`, is offered one); with `cycle = true`, one in
//! your active cycle is offered one (`gates.start.cycle = "auto"` starts it).
//! An issue with the skip label is left alone.
//!
//! An offer is a docket made from the issue, with an ask on it: Build it, or
//! Not this one. Each issue is handled once (started, offered, or declined),
//! and remembered in the hub's meta, so a restart doesn't ask again.

use anyhow::Result;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

use super::config::{Config, LinearSource};
use super::{decide, steps};
use crate::hub::{Shared, Who};

const HANDLED: &str = "build.linear.handled";
const OFFERS: &str = "build.linear.offers";

#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    Start { key: String, repo: String, why: &'static str },
    Offer { key: String, repo: String, why: &'static str },
}

/// What to do with each issue, by the source's rules and its gates.
pub fn plan(issues: &[Value], src: &LinearSource, label_gate: &str, cycle_gate: &str, known: &BTreeSet<String>) -> Vec<Act> {
    let mut out = Vec::new();
    for i in issues {
        let Some(key) = i["key"].as_str() else { continue };
        if known.contains(key) {
            continue;
        }
        let Some(repo) = i["team"].as_str().and_then(|t| src.teams.get(t)).cloned() else { continue };
        let labels: Vec<String> = i["labels"].as_array().into_iter().flatten().filter_map(|l| l.as_str().map(str::to_lowercase)).collect();
        if src.skip_label.as_ref().is_some_and(|s| labels.contains(&s.to_lowercase())) {
            continue;
        }
        let key = key.to_string();
        if src.label.as_ref().is_some_and(|l| labels.contains(&l.to_lowercase())) {
            out.push(if label_gate == "auto" { Act::Start { key, repo, why: "label" } } else { Act::Offer { key, repo, why: "label" } });
        } else if src.cycle && i["cycle_active"] == true {
            out.push(if cycle_gate == "auto" { Act::Start { key, repo, why: "cycle" } } else { Act::Offer { key, repo, why: "cycle" } });
        }
    }
    out
}

fn handled(shared: &Shared) -> BTreeSet<String> {
    shared.lock().store.meta_get(HANDLED).ok().flatten().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn mark(shared: &Shared, key: &str) {
    let mut core = shared.lock();
    let mut h: BTreeSet<String> = core.store.meta_get(HANDLED).ok().flatten().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    h.insert(key.to_string());
    let _ = core.store.meta_set(HANDLED, &serde_json::to_string(&h).unwrap_or_default());
}

fn offers(shared: &Shared) -> BTreeMap<String, Value> {
    shared.lock().store.meta_get(OFFERS).ok().flatten().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn save_offers(shared: &Shared, o: &BTreeMap<String, Value>) {
    let _ = shared.lock().store.meta_set(OFFERS, &serde_json::to_string(o).unwrap_or_default());
}

/// A poll: read your issues from Linear, then start or offer what's new.
pub async fn linear(shared: &Shared, cfg: &Config) {
    let Some(src) = &cfg.linear else { return };
    if crate::linear::key().is_none() {
        return;
    }
    let teams: Vec<String> = src.teams.keys().cloned().collect();
    let issues = match crate::linear::intake(&teams).await {
        Ok(i) => i,
        Err(e) => {
            say!("build: Linear intake: {e:#}");
            return;
        }
    };
    let mut known = handled(shared);
    known.extend(super::all(&shared.lock()).unwrap_or_default().into_iter().map(|t| t.key));
    for act in plan(&issues, src, cfg.gate("start"), cfg.gate("start_cycle"), &known) {
        match act {
            Act::Start { key, repo, why } => {
                mark(shared, &key);
                match steps::start_from(shared, &key, Some(&repo)).await {
                    Ok(v) => {
                        if let Some(d) = v["docket"].as_str() {
                            super::set_trigger(shared, d, &format!("linear-{why}"));
                        }
                    }
                    Err(e) => say!("build: {key}: {e:#}"),
                }
            }
            Act::Offer { key, repo, why } => {
                mark(shared, &key);
                if let Err(e) = offer(shared, &key, &repo, why).await {
                    say!("build: offering {key}: {e:#}");
                }
            }
        }
    }
}

/// Make a docket from the issue, and ask on it whether to build it.
async fn offer(shared: &Shared, key: &str, repo: &str, why: &str) -> Result<()> {
    let i = crate::linear::issue(key).await?;
    let mut core = shared.lock();
    let d = core.open(&Who::Hub, &format!("{}: {}", i.identifier, i.title), json!({ "protocol": "build", "title_source": "build" }))?;
    crate::linear::link(&mut core, &d, &i, &Who::Hub)?;
    crate::linear::apply(&mut core, &d, &i)?;
    let what = if why == "cycle" { "is in your active cycle" } else { "has the Build label" };
    let body = format!(
        "{} {what}, assigned to you.\n\n{}\n\nBuild it gives its description (the docket's intent now) to a builder and a checker, into {repo}: you approve the plan and the PR before anything reaches GitHub. Not this one leaves it; it isn't offered again.",
        i.identifier, i.url
    );
    let m = crate::messages::ask(
        &mut core,
        &d,
        &format!("{} · {} · build it into {repo}?", i.identifier, i.title),
        &body,
        vec!["Build it".into(), "Not this one".into()],
        json!({ "ask": "offer", "protocol": "build" }),
    )?;
    drop(core);
    let mut o = offers(shared);
    o.insert(key.to_string(), json!({ "docket": d, "message": m.id, "repo": repo }));
    save_offers(shared, &o);
    Ok(())
}

/// Your answers to offers, every step: Build it starts the Build on the
/// offer's docket; Not this one drops the docket.
pub async fn answers(shared: &Shared) {
    let o = offers(shared);
    if o.is_empty() {
        return;
    }
    let mut left = o.clone();
    for (key, v) in o {
        let (Some(d), Some(m), Some(repo)) = (v["docket"].as_str(), v["message"].as_i64(), v["repo"].as_str()) else {
            left.remove(&key);
            continue;
        };
        let reply = {
            let core = shared.lock();
            crate::messages::answer_to(&core, m).map(|r| decide::read(&["Build it".to_string(), "Not this one".to_string()], r.choice.as_deref(), &r.body))
        };
        let Some(r) = reply else { continue };
        left.remove(&key);
        if r.choice == Some(0) {
            if let Err(e) = steps::start_from(shared, d, Some(repo)).await {
                say!("build: starting {key}: {e:#}");
                let mut core = shared.lock();
                let _ = super::event(&mut core, d, "start-failed", format!("Couldn't start the Build: {e:#}"), json!({}));
            } else {
                super::set_trigger(shared, d, "linear-offer");
            }
        } else {
            let mut core = shared.lock();
            let _ = core.append(d, "set", &Who::Owner, None, json!({ "status": "dropped" }));
            let _ = super::event(&mut core, d, "declined", format!("You passed on building {key}."), json!({ "key": key }));
        }
    }
    save_offers(shared, &left);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn src() -> LinearSource {
        LinearSource { teams: [("ENG".to_string(), "acme/api".to_string())].into(), label: Some("agent".into()), cycle: true, skip_label: Some("no-agent".into()) }
    }

    fn issue(key: &str, team: &str, labels: &[&str], cycle: bool) -> Value {
        json!({ "key": key, "team": team, "labels": labels, "cycle_active": cycle })
    }

    #[test]
    fn each_issue_once_by_its_label_or_its_cycle() {
        let issues = vec![
            issue("ENG-1", "ENG", &["Agent"], false),
            issue("ENG-2", "ENG", &[], true),
            issue("ENG-3", "ENG", &["agent", "no-agent"], true),
            issue("OPS-4", "OPS", &["agent"], true),
            issue("ENG-5", "ENG", &[], false),
            issue("ENG-6", "ENG", &["agent"], false),
        ];
        let known: BTreeSet<String> = ["ENG-6".to_string()].into();
        let acts = plan(&issues, &src(), "auto", "ask", &known);
        assert_eq!(
            acts,
            vec![Act::Start { key: "ENG-1".into(), repo: "acme/api".into(), why: "label" }, Act::Offer { key: "ENG-2".into(), repo: "acme/api".into(), why: "cycle" },]
        );
        // Asked first, by its gates.
        let acts = plan(&issues, &src(), "ask", "auto", &known);
        assert_eq!(acts[0], Act::Offer { key: "ENG-1".into(), repo: "acme/api".into(), why: "label" });
        assert_eq!(acts[1], Act::Start { key: "ENG-2".into(), repo: "acme/api".into(), why: "cycle" });
        // Without cycle, a cycle's issue waits for its label.
        let mut s = src();
        s.cycle = false;
        assert_eq!(plan(&issues, &s, "auto", "ask", &known).len(), 1);
    }
}
