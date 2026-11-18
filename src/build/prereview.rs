//! Pre-review: before a Build's PR opens, reviewers you name
//! (`prereview.reviewers` in build.lua) review the branch blind, as they
//! would its PR, posting nothing. Blocking findings send it back to the
//! builder, then through the checker, for up to `prereview.loops` more
//! looks; whatever's left is in front of you at the open gate.

use anyhow::Result;
use serde_json::{Value, json};

use super::config::Config;
use super::steps::{self, att, wt};
use super::{Attention, Task};
use crate::review::panes::Place;
use crate::review::rounds::status_in;

/// Whether to pre-review now: never yet, or the last one sent it back and
/// there are looks left.
pub fn due(cfg: &Config, t: &Task) -> bool {
    if cfg.prereview.is_empty() {
        return false;
    }
    match &t.prereview {
        None => true,
        Some(p) => p["last"]["blocking"].as_i64().unwrap_or(0) > 0 && p["n"].as_i64().unwrap_or(0) <= cfg.prereview_loops && p["head"] != json!(t.checked_head),
    }
}

/// Each reviewer's name: its agent, numbered when two share one.
pub fn names(cfg: &Config) -> Vec<String> {
    cfg.prereview
        .iter()
        .enumerate()
        .map(|(i, r)| if cfg.prereview.iter().filter(|x| x.agent == r.agent).count() > 1 { format!("{}{}", r.agent, i + 1) } else { r.agent.clone() })
        .collect()
}

fn dir(t: &Task, n: i64) -> std::path::PathBuf {
    wt(t).join(format!(".tmp/prereview/{n}"))
}

/// Start a pre-review: each reviewer in a pane of its own, beside the builder.
pub async fn start(shared: &crate::hub::Shared, cfg: &Config, t: &mut Task) -> Result<()> {
    let n = t.prereview.as_ref().and_then(|p| p["n"].as_i64()).unwrap_or(0) + 1;
    let out = dir(t, n);
    std::fs::create_dir_all(&out)?;
    std::fs::create_dir_all(wt(t).join(".tmp/prereview/probes"))?;
    let login = crate::review::github::login().await.unwrap_or_else(|_| "you".into());
    let names = names(cfg);
    {
        let p = t.prereview.get_or_insert_with(|| json!({ "n": 0, "reviewers": {} }));
        p["n"] = json!(n);
        p["head"] = json!(t.checked_head);
    }
    for (i, name) in names.iter().enumerate() {
        let mut v = steps::vars(cfg, t, &login);
        v.insert("PREREVIEW", n.to_string());
        v.insert("REVIEWER", name.clone());
        v.insert("OUT", format!(".tmp/prereview/{n}"));
        crate::review::write_atomic(&wt(t).join(format!(".tmp/tasks/prereview-{name}.md")), &crate::review::briefs::render(&steps::brief("prereview.md"), &v))?;
        crate::review::write_atomic(&out.join(format!("{name}.status")), "REVIEWING\n")?;
        let prompt = format!("Read .tmp/ground-rules.md, then .tmp/tasks/prereview-{name}.md, and do it. This is pre-review {n}.");
        let anchor = t.builder.block.clone().filter(|a| crate::review::panes::state(shared, a).is_some());
        let place = match anchor.as_deref() {
            Some(a) => Place::Beside { anchor: a, dir: "down" },
            None => Place::Tab,
        };
        steps::open_role(shared, cfg, t, &format!("reviewer:{i}:{name}"), &prompt, place).await?;
    }
    t.phase = Some("prereview".into());
    let mut core = shared.lock();
    super::event(
        &mut core,
        &t.docket,
        "prereviewing",
        format!("Pre-review {n}: {} reviewing {} blind, before the PR opens.", names.join(" and "), t.branch.clone().unwrap_or_default()),
        json!({ "n": n, "reviewers": names }),
    )?;
    Ok(())
}

/// The pre-review's step: once every reviewer is done, read what they found.
pub async fn step(shared: &crate::hub::Shared, cfg: &Config, t: &mut Task) -> Result<Option<Attention>> {
    let n = t.prereview.as_ref().and_then(|p| p["n"].as_i64()).unwrap_or(0);
    let out = dir(t, n);
    let mut done = true;
    for name in names(cfg) {
        let st = status_in(&wt(t), &format!("prereview/{n}/{name}.status"));
        if let Some(why) = st.strip_prefix("BLOCKED") {
            return Ok(att("blocked", format!("pre-reviewer {name} stopped: {}", why.trim_start_matches(':').trim())));
        }
        if st != "REVIEWED" {
            done = false;
            let block = t.prereview.as_ref().and_then(|p| p["reviewers"][name.as_str()]["block"].as_str().map(str::to_string));
            if block.as_deref().is_none_or(|b| crate::review::panes::state(shared, b).is_none()) {
                return Ok(att("pane-closed", format!("pre-reviewer {name}'s pane closed before it finished")));
            }
        }
    }
    if !done {
        return Ok(None);
    }
    after(shared, cfg, t, n, &out).await.map(|_| None)
}

async fn after(shared: &crate::hub::Shared, cfg: &Config, t: &mut Task, n: i64, out: &std::path::Path) -> Result<()> {
    let mut findings: Vec<Value> = Vec::new();
    let mut said: Vec<String> = Vec::new();
    {
        let mut core = shared.lock();
        for name in names(cfg) {
            let path = out.join(format!("{name}.json"));
            if crate::review::through_link(&path).is_some() {
                continue;
            }
            let Ok(bytes) = std::fs::read(&path) else { continue };
            let v: Value = serde_json::from_slice(&bytes).unwrap_or_default();
            for f in v["findings"].as_array().into_iter().flatten() {
                let mut f = f.clone();
                f["by"] = json!(name);
                findings.push(f);
            }
            if let Some(s) = v["summary"].as_str() {
                said.push(format!("{name}: {s}"));
            }
            steps::keep(
                &mut core,
                t,
                format!("Pre-review {n}: {name}"),
                format!("{name}.json"),
                bytes,
                &format!("Pre-review {n}"),
                json!({ "role": "prereview", "n": n, "by": name }),
            );
        }
    }
    // Their panes go; their sessions stay, for the next look.
    if let Some(sid) = t.session_id.clone() {
        let blocks: Vec<String> =
            t.prereview.as_ref().and_then(|p| p["reviewers"].as_object()).map(|o| o.values().filter_map(|a| a["block"].as_str().map(str::to_string)).collect()).unwrap_or_default();
        for b in blocks {
            if crate::review::panes::state(shared, &b).is_some() {
                crate::review::panes::close(&sid, &b).await;
            }
        }
    }
    let blocking = findings.iter().filter(|f| f["severity"] == "blocking").count() as i64;
    let others = findings.len() as i64 - blocking;
    if let Some(p) = t.prereview.as_mut() {
        p["last"] = json!({ "blocking": blocking, "non_blocking": others, "findings": findings, "said": said });
    }
    {
        let mut core = shared.lock();
        super::event(
            &mut core,
            &t.docket,
            "prereviewed",
            format!("Pre-review {n}: {blocking} blocking, {others} non-blocking."),
            json!({ "n": n, "blocking": blocking, "non_blocking": others }),
        )?;
    }
    if blocking > 0 && n <= cfg.prereview_loops {
        t.phase = Some("building".into());
        let text = format!(
            "Pre-review {n} sent it back: .tmp/prereview/{n}/*.json, from {}. Fix each blocking finding, or say in .tmp/report.md why it stands; weigh the non-blocking ones. Commit, update .tmp/pr.json if the change is different now, and write BUILT. The checker checks it again, and the reviewers look again.",
            names(cfg).join(" and ")
        );
        return steps::tell_builder(shared, t, &text, "WORKING building").await;
    }
    steps::gates(shared, cfg, t).await
}

/// What the open gate says of the last pre-review.
pub fn summary(t: &Task) -> Option<String> {
    let last = &t.prereview.as_ref()?["last"];
    let fs = last["findings"].as_array()?;
    let mut s =
        format!("\nPre-review {}: {} blocking, {} non-blocking.\n", t.prereview.as_ref()?["n"], last["blocking"].as_i64().unwrap_or(0), last["non_blocking"].as_i64().unwrap_or(0));
    for f in fs.iter().take(12) {
        s.push_str(&format!(
            "- {} {} ({}, {}): {}\n",
            f["by"].as_str().unwrap_or("?"),
            f["id"].as_str().unwrap_or("?"),
            f["severity"].as_str().unwrap_or("?"),
            f["path"].as_str().map(|p| format!("{p}{}", f["line"].as_i64().map(|l| format!(":{l}")).unwrap_or_default())).unwrap_or_else(|| "the change".into()),
            f["what"].as_str().unwrap_or("")
        ));
    }
    Some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(reviewers: &[&str], loops: i64) -> Config {
        let lua = json!({ "repos": { "acme/api": { "checkout": "/tmp/x" } }, "prereview": { "reviewers": reviewers, "loops": loops } });
        crate::build::config::from_value(None, lua, None).unwrap()
    }

    #[test]
    fn it_runs_once_then_again_only_after_a_fix_it_asked_for() {
        let c = cfg(&["codex"], 1);
        let mut t = Task { checked_head: Some("a".into()), ..Default::default() };
        assert!(due(&c, &t), "never run");
        t.prereview = Some(json!({ "n": 1, "head": "a", "last": { "blocking": 1 } }));
        assert!(!due(&c, &t), "same head: the builder hasn't fixed it yet");
        t.checked_head = Some("b".into());
        assert!(due(&c, &t), "fixed and checked: look again");
        t.prereview = Some(json!({ "n": 2, "head": "b", "last": { "blocking": 1 } }));
        t.checked_head = Some("c".into());
        assert!(!due(&c, &t), "out of looks: the open gate shows what's left");
        t.prereview = Some(json!({ "n": 1, "head": "a", "last": { "blocking": 0 } }));
        assert!(!due(&c, &t), "it passed");
        assert!(!due(&cfg(&[], 1), &Task::default()), "no reviewers, no pre-review");
    }

    #[test]
    fn reviewers_sharing_an_agent_are_numbered() {
        assert_eq!(names(&cfg(&["codex", "claude"], 1)), vec!["codex", "claude"]);
        assert_eq!(names(&cfg(&["claude", "claude"], 1)), vec!["claude1", "claude2"]);
    }
}
