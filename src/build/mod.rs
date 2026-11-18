//! Build: a docket to a pull request (docs/build.md). The docket's intent is
//! the spec. A builder plans and builds in a worktree, a checker checks it
//! against the plan, and every ask (decisions, the plan, the preview, the
//! PR) is a message to you with buttons. Only the hub writes outward, with
//! your sign-ins, and only what you approved. Each docket's working state is
//! in `build_tasks`; what happened is on the docket's record.

pub mod address;
pub mod agents;
pub mod config;
pub mod decide;
pub mod intake;
pub mod plan;
pub mod prereview;
pub mod steps;
pub mod watch;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Arc;

use crate::hub::{Core, Shared, Who};

/// The Rex session Build's panes open in, a tab per docket.
pub fn session_name() -> String {
    std::env::var("WH_BUILD_SESSION").unwrap_or_else(|_| "wheelhouse-builds".into())
}

pub fn data_dir() -> PathBuf {
    std::env::var_os("WH_BUILD_DATA").map(PathBuf::from).unwrap_or_else(|| crate::paths::home().join(".local/share/wheelhouse/build"))
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Agent {
    #[serde(default)]
    pub agent: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
}

/// An ask out to you: its message, and its version (a re-ask is newer).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Ask {
    pub message: i64,
    pub version: i64,
    pub options: Vec<String>,
}

/// A decision the builder raised, and its answer once there is one.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Dec {
    pub question: String,
    pub options: Vec<String>,
    pub recommend: usize,
    #[serde(default)]
    pub blocking: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<Value>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Task {
    pub docket: String,
    /// What the work is called: the issue's key, or the docket's id.
    pub key: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// `docket`, `github` or `linear`.
    pub source: String,
    pub trigger: String,
    pub repo: String,
    /// `queued`, `active`, `open`, `merged`, `closed`, `discarded`.
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_head: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<i64>,
    #[serde(default)]
    pub builder: Agent,
    #[serde(default)]
    pub checker: Agent,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default)]
    pub plan_version: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,
    #[serde(default)]
    pub decisions: BTreeMap<String, Dec>,
    #[serde(default)]
    pub asks: BTreeMap<String, Ask>,
    /// Prompts for the builder, sent when it's free.
    #[serde(default)]
    pub outbox: Vec<String>,
    /// Problems already told to the builder, so each is told once.
    #[serde(default)]
    pub told: Vec<String>,
    #[serde(default)]
    pub check_n: i64,
    #[serde(default)]
    pub fix_used: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub built_head: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checked_head: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_check: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previewed_head: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approved_head: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pushed_head: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr_number: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_failed_epoch: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    /// Address rounds since the PR opened, and the one in progress:
    /// `{n, items, update, from, summary}`.
    #[serde(default)]
    pub rounds: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub round: Option<Value>,
    /// What's been addressed on the PR, or set aside, by id: a thread's
    /// latest comment, a review, a comment, a failing head, a base.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub seen: Vec<String>,
    /// Pre-review: `{n, reviewers: {name: Agent}, last: {blocking,
    /// non_blocking, findings}}`, once it's run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prereview: Option<Value>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Task {
    pub fn finished(&self) -> bool {
        matches!(self.state.as_str(), "merged" | "closed" | "discarded")
    }

    pub fn phase(&self) -> &str {
        self.phase.as_deref().unwrap_or("")
    }

    /// A pane label for a role: `d-41`, `d-41-check`, `d-41-codex` for a
    /// pre-reviewer.
    pub fn label(&self, role: &str) -> String {
        let id = self.docket.to_lowercase();
        match role {
            "builder" => id,
            "checker" => format!("{id}-check"),
            r => format!("{id}-{}", r.rsplit(':').next().unwrap_or("review")),
        }
    }
}

/// Something about a Build that needs you, beyond its asks.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Attention {
    pub kind: String,
    pub text: String,
}

impl Hub {
    /// A repo's witnessed checks, as Build's config (last read) has them.
    pub fn specs(&self, repo: &str) -> Option<Vec<config::CheckSpec>> {
        self.config.as_ref().and_then(|(c, _, _)| c.repos.get(repo).map(|r| r.checks.clone()))
    }

    /// The repo whose checkout holds `dir`, and its checks.
    pub fn repo_of(&self, dir: &std::path::Path) -> Option<(String, Vec<config::CheckSpec>)> {
        self.config.as_ref().and_then(|(c, _, _)| c.repos.iter().find(|(_, r)| dir.starts_with(&r.checkout)).map(|(k, r)| (k.clone(), r.checks.clone())))
    }
}

#[derive(Default)]
pub struct Hub {
    pub(crate) config: Option<(Arc<config::Config>, Option<std::time::SystemTime>, Option<std::time::SystemTime>)>,
    pub config_error: Option<String>,
    pub attention: BTreeMap<String, Attention>,
    pub check_now: Arc<tokio::sync::Notify>,
    pub doing: Option<String>,
    locks: HashMap<String, Arc<tokio::sync::Mutex<()>>>,
    /// Build panes by label: their docket and role.
    pub panes: BTreeMap<String, (String, String)>,
}

pub fn paused(core: &Core) -> bool {
    core.store.meta_get("build.paused").ok().flatten().as_deref() == Some("1")
}

/// The config, read again when build.lua or review.lua changes.
pub fn config(shared: &Shared) -> Result<Arc<config::Config>> {
    let m = |p: PathBuf| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    // Settings lie over both files: a change to any of them reloads.
    let (bm, rm) = (m(config::path()).max(crate::settings::mtime()), m(crate::review::config::path()));
    if let Some((c, b, r)) = &shared.lock().build.config
        && *b == bm
        && *r == rm
    {
        return Ok(c.clone());
    }
    match config::load() {
        Ok(c) => {
            let c = Arc::new(c);
            let mut core = shared.lock();
            core.build.config = Some((c.clone(), bm, rm));
            core.build.config_error = None;
            Ok(c)
        }
        Err(e) => {
            shared.lock().build.config_error = Some(format!("{e:#}"));
            Err(e)
        }
    }
}

pub fn lock(shared: &Shared, docket: &str) -> Arc<tokio::sync::Mutex<()>> {
    shared.lock().build.locks.entry(docket.to_string()).or_default().clone()
}

pub fn get(core: &Core, docket: &str) -> Result<Option<Task>> {
    match core.store.build_task(docket)? {
        Some(b) => Ok(Some(serde_json::from_str(&b).with_context(|| format!("reading {docket}'s build"))?)),
        None => Ok(None),
    }
}

pub fn all(core: &Core) -> Result<Vec<Task>> {
    core.store.build_tasks()?.iter().map(|b| serde_json::from_str(b).context("reading a build")).collect()
}

/// What a docket shows of its Build.
pub fn summary(t: &Task) -> Value {
    json!({
        "docket": t.docket, "key": t.key, "title": t.title, "url": t.url, "source": t.source, "repo": t.repo,
        "state": t.state, "phase": t.phase, "branch": t.branch, "base": t.base,
        "plan_version": t.plan_version, "check_n": t.check_n, "fix_used": t.fix_used,
        "last_check": t.last_check, "checked_head": t.checked_head, "pushed_head": t.pushed_head,
        "pr_number": t.pr_number, "pr_url": t.pr_url, "builder": t.builder, "checker": t.checker,
        "decisions": t.decisions.iter().map(|(id, d)| json!({ "id": id, "question": d.question, "answered": d.answer.is_some(), "label": d.answer.as_ref().and_then(|a| a["label"].as_str()) })).collect::<Vec<_>>(),
        "asks": t.asks.keys().collect::<Vec<_>>(),
        "why": t.why, "start_error": t.start_error, "updated_at": t.updated_at,
        "rounds": t.rounds, "round": t.round.as_ref().map(|r| json!({ "n": r["n"], "summary": r["summary"], "update": r["update"] })),
        "prereview": t.prereview.as_ref().map(|p| json!({ "n": p["n"], "reviewers": p["reviewers"].as_object().map(|o| o.keys().cloned().collect::<Vec<_>>()), "blocking": p["last"]["blocking"], "non_blocking": p["last"]["non_blocking"] })),
        "pending": t.extra.get("pending").map(|p| p["ids"].as_array().map_or(0, Vec::len)),
        "tokens": t.tokens.as_ref().and_then(|x| x["total"].as_i64()),
    })
}

/// Say what started a Build (`label`, `linear-cycle`…). One lock, held once:
/// never inside an `if let` that locks again, which would hang the hub.
pub fn set_trigger(shared: &crate::hub::Shared, docket: &str, trigger: &str) {
    let mut core = shared.lock();
    if let Ok(Some(mut t)) = get(&core, docket) {
        t.trigger = trigger.to_string();
        let _ = save(&mut core, &mut t);
    }
}

/// Save a Build's state; note its phase on the docket when it changes.
pub fn save(core: &mut Core, t: &mut Task) -> Result<()> {
    t.updated_at = Some(crate::time::now());
    let was: Option<Task> = get(core, &t.docket)?;
    core.store.put_build_task(&t.docket, &serde_json::to_string(t)?)?;
    let moved = was.as_ref().is_none_or(|w| w.state != t.state || w.phase != t.phase);
    if moved {
        core.append(&t.docket, "observed", &Who::Hub, None, json!({ "what": "build", "build": summary(t) }))?;
        let status = match t.state.as_str() {
            "queued" => Some("todo"),
            "active" | "open" => Some("in_progress"),
            "merged" => Some("done"),
            "closed" | "discarded" => Some("dropped"),
            _ => None,
        };
        if let (Some(s), Some(st)) = (status, core.store.state(&t.docket)?)
            && st.status != s
        {
            core.append(&t.docket, "set", &Who::Hub, None, json!({ "status": s }))?;
        }
    }
    index_panes(core, t);
    core.live_changed();
    Ok(())
}

fn index_panes(core: &mut Core, t: &Task) {
    core.build.panes.retain(|_, (d, _)| *d != t.docket);
    if t.finished() {
        return;
    }
    core.build.panes.insert(t.label("builder"), (t.docket.clone(), "builder".into()));
    core.build.panes.insert(t.label("checker"), (t.docket.clone(), "checker".into()));
}

/// Rebuild the pane map from every Build (at start).
pub fn load(core: &mut Core) -> Result<()> {
    for t in all(core)? {
        index_panes(core, &t);
    }
    Ok(())
}

/// The Build docket and role of a pane in the build session, if it's one.
pub fn pane_docket(core: &Core, pane: &crate::mirror::Pane) -> Option<(String, String)> {
    if core.mirror.session_label(&pane.session_id) != session_name() {
        return None;
    }
    core.build.panes.get(&pane.label).cloned()
}

/// What happened, in words, on the docket.
pub fn event(core: &mut Core, docket: &str, what: &str, body: String, data: Value) -> Result<()> {
    let mut data = data;
    data["what"] = json!("build-event");
    data["event"] = json!(what);
    core.append(docket, "observed", &Who::Hub, Some(body), data)?;
    Ok(())
}
