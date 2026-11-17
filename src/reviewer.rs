//! rex-reviewer, while it runs alongside (until Wheelhouse's own Review
//! protocol replaces it). It works only through what rex-reviewer publishes:
//!
//! - Its ledger (`~/.local/state/rex-reviewer`, docs/ledger.md there) says
//!   which panes review which PR. Those panes go on one Review docket per
//!   PR, not a docket each, and the docket follows the PR's round, verdict
//!   and merge risk.
//! - Its watcher reports a record per PR that needs you: app
//!   `rex-reviewer`, id `pr/<owner>/<name>/<pr>/<round>`; `blocked` on
//!   `permission` is a review ready to post.
//! - Its Rex actions take that id: `reviewer_post`, `reviewer_dismiss`,
//!   `reviewer_preview`. They refuse once the round has moved on.

use anyhow::{Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::hub::{Core, Shared, Who};
use crate::mirror::Pane;
use crate::record::fold::AgentRef;

pub const APP: &str = "rex-reviewer";

pub fn state_dir() -> PathBuf {
    std::env::var_os("REX_REVIEWER_STATE_DIR").map(PathBuf::from).unwrap_or_else(|| crate::paths::home().join(".local/state/rex-reviewer"))
}

/// One PR, as rex-reviewer's ledger has it.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct Pr {
    pub repo: String,
    pub pr: i64,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub phase: Option<String>,
    #[serde(default)]
    pub round: Option<i64>,
    #[serde(default)]
    pub verdict: Option<String>,
    #[serde(default)]
    pub merge_risk: Option<String>,
    #[serde(default)]
    pub review_url: Option<String>,
    #[serde(default)]
    pub implementation: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub pane: Option<String>,
    #[serde(default)]
    pub checker_pane: Option<String>,
    #[serde(default)]
    pub reviewers: Value,
}

impl Pr {
    /// `name#pr`, as dockets and items show it.
    pub fn key(&self) -> String {
        format!("{}#{}", self.repo.rsplit('/').next().unwrap_or(&self.repo), self.pr)
    }

    pub fn ended(&self) -> bool {
        matches!(self.state.as_deref(), Some("merged" | "closed" | "cleaned" | "unrequested"))
    }

    /// What a pane does for this PR: `codex reviewer`, `checker`.
    fn role_of(&self, session: &str, label: &str) -> Option<String> {
        if self.session_id.as_deref() != Some(session) || label.is_empty() {
            return None;
        }
        if self.checker_pane.as_deref() == Some(label) {
            return Some("checker".into());
        }
        if let Some(rs) = self.reviewers.as_object() {
            for (name, r) in rs {
                if r["pane"].as_str() == Some(label) {
                    return Some(format!("{name} reviewer"));
                }
            }
        }
        (self.pane.as_deref() == Some(label)).then(|| "reviewer".into())
    }
}

/// The ledger, read again when it's more than two seconds old.
#[derive(Default)]
pub struct Ledger {
    prs: Vec<Pr>,
    read_at: Option<Instant>,
    synced_at: Option<Instant>,
}

impl Ledger {
    fn refresh(&mut self) {
        if self.read_at.is_some_and(|t| t.elapsed() < Duration::from_secs(2)) {
            return;
        }
        self.read_at = Some(Instant::now());
        let mut prs = Vec::new();
        let repos = state_dir().join("repos");
        for owner in std::fs::read_dir(&repos).into_iter().flatten().flatten() {
            for name in std::fs::read_dir(owner.path()).into_iter().flatten().flatten() {
                for f in std::fs::read_dir(name.path()).into_iter().flatten().flatten() {
                    let p = f.path();
                    if p.extension().is_some_and(|e| e == "json")
                        && let Ok(text) = std::fs::read_to_string(&p)
                        && let Ok(pr) = serde_json::from_str::<Pr>(&text)
                    {
                        prs.push(pr);
                    }
                }
            }
        }
        self.prs = prs;
    }

    pub fn prs(&mut self) -> &[Pr] {
        self.refresh();
        &self.prs
    }
}

/// The PR a pane works on for rex-reviewer, and its role there.
pub fn pane_role(core: &mut Core, p: &Pane) -> Option<(Pr, String)> {
    let label = p.label.clone();
    let session = p.session_id.clone();
    core.reviewer.prs().iter().find_map(|pr| pr.role_of(&session, &label).map(|r| (pr.clone(), r)))
}

fn review_data(pr: &Pr) -> Value {
    json!({ "repo": pr.repo, "pr": pr.pr, "key": pr.key(), "url": pr.url, "title": pr.title, "author": pr.author,
            "state": pr.state, "phase": pr.phase, "round": pr.round, "verdict": pr.verdict, "merge_risk": pr.merge_risk,
            "review_url": pr.review_url, "implementation": pr.implementation })
}

/// The open Review docket for a PR, opened the first time one of its panes appears.
pub fn docket_for(core: &mut Core, pr: &Pr) -> Result<String> {
    let key = pr.key();
    if let Some(st) = core.store.states()?.into_iter().find(|s| s.is_open() && s.review.as_ref().is_some_and(|r| r.key == key)) {
        return Ok(st.id);
    }
    let title = format!("Review {key}{}", pr.title.as_deref().map(|t| format!(": {t}")).unwrap_or_default());
    core.open(&Who::Hub, &title, json!({ "protocol": "review", "title_source": "review", "review": review_data(pr), "source": "rex-reviewer" }))
}

/// Put a reviewer's or checker's pane on its PR's docket.
pub fn adopt_pane(core: &mut Core, pr: &Pr, role: &str, agent: AgentRef) -> Result<String> {
    let docket = docket_for(core, pr)?;
    crate::agents::adopt(core, &docket, &agent, None);
    core.append(&docket, "claim", &Who::Hub, None, json!({ "source": "rex-reviewer", "role": role, "round": pr.round, "agent": agent }))?;
    Ok(docket)
}

/// Every ten seconds: open Review dockets follow their PR in the ledger,
/// and close once it's over and none of its panes are left.
pub fn sync(core: &mut Core) {
    if core.reviewer.synced_at.is_some_and(|t| t.elapsed() < Duration::from_secs(10)) {
        return;
    }
    core.reviewer.synced_at = Some(Instant::now());
    let prs = core.reviewer.prs().to_vec();
    let Ok(states) = core.store.states() else { return };
    for st in states.into_iter().filter(|s| s.is_open()) {
        let Some(r) = st.review.clone() else { continue };
        let Some(pr) = prs.iter().find(|p| p.key() == r.key) else { continue };
        let now = (pr.state.clone(), pr.phase.clone(), pr.round, pr.verdict.clone(), pr.merge_risk.clone(), pr.review_url.clone());
        let was = (r.state.clone(), r.phase.clone(), r.round, r.verdict.clone(), r.merge_risk.clone(), r.review_url.clone());
        if now != was {
            let _ = core.append(&st.id, "observed", &Who::Hub, None, json!({ "what": "review", "source": APP, "review": review_data(pr) }));
        }
        let panes_left = core.agents.by_block.values().any(|t| t.docket == st.id);
        if pr.ended() && !panes_left {
            let _ = core.append(&st.id, "set", &Who::Hub, None, json!({ "status": "done" }));
        }
    }
}

/// Whether a Review docket's PR is over, so its last pane closing ends it.
pub fn ended(core: &mut Core, key: &str) -> bool {
    core.reviewer.prs().iter().find(|p| p.key() == key).is_none_or(Pr::ended)
}

/// A review ready to post, from the watcher's record.
#[derive(Clone, Debug, Serialize)]
pub struct Ready {
    pub item: String,
    pub key: String,
    pub round: Option<i64>,
    pub msg: Option<String>,
    pub updated_at: Option<String>,
    pub docket: Option<String>,
    pub title: Option<String>,
    pub url: Option<String>,
}

fn key_of_item(item: &str) -> Option<(String, Option<i64>)> {
    let mut parts = item.strip_prefix("pr/")?.split('/');
    let (_owner, name, pr, round) = (parts.next()?, parts.next()?, parts.next()?, parts.next());
    Some((format!("{name}#{pr}"), round.and_then(|r| r.parse().ok())))
}

pub fn ready(core: &mut Core) -> Vec<Ready> {
    let mut out = Vec::new();
    let found: Vec<(String, Option<String>, Option<String>)> = core
        .mirror
        .panes
        .values()
        .flat_map(|p| p.records.values())
        .filter(|r| r.app.as_deref() == Some(APP) && r.id.starts_with("pr/") && r.state == "blocked" && r.kind.as_deref() == Some("permission"))
        .map(|r| (r.id.clone(), r.msg.clone(), r.updated_at.clone()))
        .collect();
    let states = core.store.states().unwrap_or_default();
    let prs = core.reviewer.prs().to_vec();
    for (item, msg, updated_at) in found {
        let Some((key, round)) = key_of_item(&item) else { continue };
        let docket = states.iter().find(|s| s.is_open() && s.review.as_ref().is_some_and(|r| r.key == key)).map(|s| s.id.clone());
        let pr = prs.iter().find(|p| p.key() == key);
        out.push(Ready { item, key, round, msg, updated_at, docket, title: pr.and_then(|p| p.title.clone()), url: pr.and_then(|p| p.url.clone()) });
    }
    out
}

/// Post (`true`) or dismiss a ready review, only if it's still the one shown.
pub async fn answer(shared: &Shared, item: &str, post: bool, msg: Option<&str>, updated_at: Option<&str>) -> Result<Value> {
    let (session, block, docket) = {
        let mut core = shared.lock();
        let p = core.mirror.panes.values().find(|p| p.records.contains_key(item)).ok_or_else(|| anyhow!("that review isn't waiting any more"))?;
        let (s, b) = (p.session_id.clone(), p.block_id.clone());
        let docket = ready(&mut core).into_iter().find(|r| r.item == item).and_then(|r| r.docket);
        (s, b, docket)
    };
    let v = crate::rex::Rex.block_call(&session, &block, "program_status", &json!({})).await?;
    let now = v["records"].as_array().into_iter().flatten().find(|r| r["id"] == item);
    let same = now.is_some_and(|r| r["state"] == "blocked" && r["kind"] == "permission" && r["msg"].as_str() == msg && r["updated_at"].as_str() == updated_at);
    if !same {
        bail!("that review changed since you saw it; nothing was sent");
    }
    let action = if post { "reviewer_post" } else { "reviewer_dismiss" };
    let result = crate::rex::Rex.action(action, &[("item", item)]).await?;
    if let Some(d) = docket {
        let mut core = shared.lock();
        let owner = core.owner.clone();
        core.append(&d, "approval", &Who::Hub, msg.map(str::to_string), json!({ "decision": if post { "post" } else { "dismiss" }, "kind": "review", "item": item, "by": owner, "url": result["url"] }))?;
    }
    Ok(json!({ "posted": post, "url": result["url"], "result": result }))
}

pub async fn preview(item: &str) -> Result<String> {
    let v = crate::rex::Rex.action("reviewer_preview", &[("item", item)]).await?;
    v["preview"].as_str().map(str::to_string).ok_or_else(|| anyhow!("rex-reviewer's preview came back empty"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panes_and_items_map_to_their_pr() {
        let pr: Pr = serde_json::from_value(json!({
            "repo": "acme/api", "pr": 341, "session_id": "s1", "pane": "api-341", "checker_pane": "api-341-check",
            "reviewers": { "codex": { "pane": "api-341" }, "claude": { "pane": "api-341-claude" } }, "state": "reviewing"
        }))
        .unwrap();
        assert_eq!(pr.key(), "api#341");
        assert_eq!(pr.role_of("s1", "api-341-check").as_deref(), Some("checker"));
        assert_eq!(pr.role_of("s1", "api-341-claude").as_deref(), Some("claude reviewer"));
        assert!(pr.role_of("s2", "api-341").is_none(), "another session");
        assert!(!pr.ended());
        assert_eq!(key_of_item("pr/acme/api/341/2"), Some(("api#341".into(), Some(2))));
    }

    #[test]
    fn the_watchers_record_is_a_ready_review() {
        let mut core = crate::hub::test_core();
        let rec = |id: &str, state: &str, kind: Option<&str>| crate::mirror::Record {
            id: id.into(),
            state: state.into(),
            kind: kind.map(str::to_string),
            app: Some(APP.into()),
            msg: Some("REQUEST_CHANGES · 2 blocking · risk high".into()),
            updated_at: Some("t1".into()),
            ..Default::default()
        };
        core.mirror.apply_pane_status(
            "s1",
            "watcher",
            vec![rec("", "idle", None), rec("pr/acme/api/341/2", "blocked", Some("permission")), rec("pr/acme/api/340/1", "blocked", None)],
            None,
            None,
        );
        let r = ready(&mut core);
        assert_eq!(r.len(), 1, "only a review waiting to post");
        assert_eq!(r[0].key, "api#341");
        assert_eq!(r[0].round, Some(2));
        assert_eq!(r[0].msg.as_deref(), Some("REQUEST_CHANGES · 2 blocking · risk high"));
    }
}
