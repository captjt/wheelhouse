//! The Review protocol: what rex-reviewer did, native. A PR that requests
//! you is one Review docket for its whole life. Each round preps a worktree
//! and context, starts blind reviewers in Rex panes, then a checker that
//! merges their drafts and assesses the merge risk; the post goes through
//! the same refusals `post-review` had. Its working state per PR lives in
//! `review_prs` (rex-reviewer's ledger format, so that ledger imports as it
//! is); what happened goes on the docket's record.

pub mod agentcli;
pub mod briefs;
pub mod config;
pub mod github;
pub mod links;
pub mod lua;
pub mod panes;
pub mod post;
pub mod posting;
pub mod pr;
pub mod rounds;
pub mod rules;
pub mod watch;

use anyhow::{Context, Result};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::hub::{Core, Shared, Who};
use config::Config;
use pr::Pr;

/// The Rex session review panes open in, one tab per PR
/// (`$WH_REVIEW_SESSION` for tests).
pub fn session_name() -> String {
    std::env::var("WH_REVIEW_SESSION").unwrap_or_else(|_| "wheelhouse-reviews".into())
}

/// Worktrees, bundled guides, the draft format: `$WH_REVIEW_DATA`, or
/// `~/.local/share/wheelhouse/review`.
pub fn data_dir() -> PathBuf {
    std::env::var_os("WH_REVIEW_DATA").map(PathBuf::from).unwrap_or_else(|| crate::paths::home().join(".local/share/wheelhouse/review"))
}

/// Shared caches the agents write to (Cargo's, so far).
pub fn cache_dir() -> PathBuf {
    std::env::var_os("WH_REVIEW_CACHE").map(PathBuf::from).unwrap_or_else(|| crate::paths::home().join(".cache/wheelhouse"))
}

/// The first part of `path`, from its `.tmp` folder down, that's a symbolic
/// link. Agents can write to their worktree's `.tmp`, and the hub writes,
/// reads and deletes there: a link would point the hub somewhere else.
pub fn through_link(path: &Path) -> Option<PathBuf> {
    let parts: Vec<_> = path.components().collect();
    let from = parts.iter().position(|c| c.as_os_str() == ".tmp")?;
    let mut cur = PathBuf::new();
    for (i, c) in parts.iter().enumerate() {
        cur.push(c);
        if i >= from && std::fs::symlink_metadata(&cur).is_ok_and(|m| m.file_type().is_symlink()) {
            return Some(cur);
        }
    }
    None
}

/// Refuse a path through a link in a `.tmp` folder (see `through_link`).
pub fn no_link(path: &Path) -> Result<()> {
    match through_link(path) {
        Some(l) => anyhow::bail!("{} is a link; Wheelhouse won't follow it out of the worktree", l.display()),
        None => Ok(()),
    }
}

pub fn write_atomic(path: &Path, text: &str) -> Result<()> {
    no_link(path)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension(format!("{}.tmp", path.extension().and_then(|e| e.to_str()).unwrap_or("")));
    std::fs::write(&tmp, text).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

fn written(name: &str, text: &str) -> PathBuf {
    let p = data_dir().join(name);
    if std::fs::read_to_string(&p).ok().as_deref() != Some(text) {
        let _ = write_atomic(&p, text);
    }
    p
}

/// A bundled guide, written where agents can read it.
pub fn bundled_guide(slot: &str) -> Option<PathBuf> {
    match slot {
        "style" => Some(written("guides/review-style.md", &crate::settings::guide_text("review-style.md"))),
        "merge_risk" => Some(written("guides/merge-risk.md", &crate::settings::guide_text("merge-risk.md"))),
        _ => None,
    }
}

/// The review draft's format, for `{{DRAFT_SCHEMA}}`.
pub fn draft_schema() -> PathBuf {
    written("review-draft.md", &briefs::brief("review-draft.md"))
}

/// Something about a PR that needs you.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Attention {
    /// `ready`, `blocked`, `start-failed`, `pane-closed`, `stalled`, `error`.
    pub kind: String,
    pub text: String,
    pub round: i64,
}

#[derive(Default)]
pub struct Hub {
    pub(crate) config: Option<(Arc<Config>, Option<std::time::SystemTime>)>,
    pub config_error: Option<String>,
    pub attention: BTreeMap<String, Attention>,
    pub check_now: Arc<tokio::sync::Notify>,
    pub last_scan_at: Option<String>,
    pub scan_error: Option<String>,
    /// What the loop is doing now, for the status line.
    pub doing: Option<String>,
    locks: HashMap<String, Arc<tokio::sync::Mutex<()>>>,
    /// Review panes by label (the review session's labels are unique):
    /// their docket and role, for the tracker.
    pub panes: BTreeMap<String, (String, String)>,
    /// When the hub last tried to read what each PR is for.
    pub integrate_tried: HashMap<String, std::time::Instant>,
}

// ---- Settings ----

pub fn enabled(core: &Core) -> bool {
    core.store.meta_get("review.enabled").ok().flatten().as_deref() == Some("1")
}

pub fn paused(core: &Core) -> bool {
    core.store.meta_get("review.paused").ok().flatten().as_deref() != Some("0")
}

/// What `wh review post-mode ask|auto` set, over every repo's rule; `None`
/// follows the config.
pub fn post_override(core: &Core) -> Option<posting::Post> {
    match core.store.meta_get("review.post").ok().flatten().as_deref() {
        Some("ask") => Some(posting::Post::Ask),
        Some("auto") => Some(posting::Post::Auto),
        _ => None,
    }
}

/// Who posts a checked review in `repo` (or, without one, by default): the
/// override, the repo's own `post`, or the config's.
pub fn post_for(core: &Core, cfg: &Config, repo: Option<&str>) -> posting::Post {
    post_override(core).unwrap_or_else(|| repo.map_or(&cfg.post, |r| cfg.post_for(r)).clone())
}

/// `ask`, `auto` or `rule`, for `repo` (or by default).
pub fn post_mode(core: &Core, cfg: &Config, repo: Option<&str>) -> &'static str {
    post_for(core, cfg, repo).kind()
}

/// The config, read again when its file changes.
pub fn config(shared: &Shared) -> Result<Arc<Config>> {
    let path = config::path();
    let mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok().max(crate::settings::mtime());
    if let Some((c, m)) = &shared.lock().review.config
        && *m == mtime
        && c.path == path
    {
        return Ok(c.clone());
    }
    match config::load() {
        Ok(c) => {
            let c = Arc::new(c);
            let mut core = shared.lock();
            core.review.config = Some((c.clone(), mtime));
            core.review.config_error = None;
            Ok(c)
        }
        Err(e) => {
            shared.lock().review.config_error = Some(format!("{e:#}"));
            Err(e)
        }
    }
}

/// One PR at a time: the loop and your actions take its lock.
pub fn lock(shared: &Shared, key: &str) -> Arc<tokio::sync::Mutex<()>> {
    shared.lock().review.locks.entry(key.to_string()).or_default().clone()
}

// ---- The PRs ----

pub fn get(core: &Core, key: &str) -> Result<Option<Pr>> {
    match core.store.review_pr(key)? {
        Some((_, body)) => Ok(Some(serde_json::from_str(&body).with_context(|| format!("reading {key}"))?)),
        None => Ok(None),
    }
}

pub fn all(core: &Core) -> Result<Vec<Pr>> {
    core.store.review_prs()?.into_iter().map(|(k, _, body)| serde_json::from_str(&body).with_context(|| format!("reading {k}"))).collect()
}

pub fn docket_of(core: &Core, key: &str) -> Option<String> {
    core.store.review_pr(key).ok().flatten().and_then(|(d, _)| d)
}

/// What a Review docket shows of its PR.
pub fn summary(p: &Pr) -> Value {
    json!({
        "repo": p.repo, "pr": p.pr, "key": p.short(), "url": p.url, "title": p.title, "author": p.author,
        "state": p.state, "phase": p.phase, "round": p.round, "verdict": p.verdict, "merge_risk": p.merge_risk,
        "review_url": p.review_url, "implementation": p.implementation,
    })
}

fn status_for(state: &str) -> &'static str {
    match state {
        "queued" => "todo",
        "reviewing" => "in_progress",
        "unrequested" => "dropped",
        _ => "done",
    }
}

/// The PR's docket, opened the first time it's needed. A Review docket the
/// rex-reviewer bridge opened for the PR is taken over.
fn docket_for(core: &mut Core, p: &Pr) -> Result<String> {
    if let Some(d) = docket_of(core, &p.key()) {
        return Ok(d);
    }
    let short = p.short();
    let existing = core
        .store
        .states()?
        .into_iter()
        .filter(|s| s.protocol == "review" && s.review.as_ref().is_some_and(|r| r.key == short))
        .max_by_key(|s| (s.is_open(), s.created_at.clone()));
    let id = match existing {
        Some(s) => s.id,
        None => {
            let title = format!("Review {short}{}", p.title.as_deref().map(|t| format!(": {t}")).unwrap_or_default());
            let id = core.open(&Who::Hub, &title, json!({ "protocol": "review", "title_source": "review", "review": summary(p), "source": "wheelhouse" }))?;
            if let Some(url) = &p.url {
                core.append(&id, "link", &Who::Hub, None, json!({ "kind": "github-pr", "url": url, "key": p.key() }))?;
            }
            id
        }
    };
    core.store.put_review_pr(&p.key(), &p.repo, p.pr, Some(&id), &serde_json::to_string(p)?)?;
    Ok(id)
}

/// Save a PR's state, and bring its docket along: its summary and status.
pub fn save(core: &mut Core, p: &mut Pr) -> Result<String> {
    p.updated_at = Some(crate::time::now());
    let docket = docket_for(core, p)?;
    core.store.put_review_pr(&p.key(), &p.repo, p.pr, Some(&docket), &serde_json::to_string(p)?)?;
    if let Some(st) = core.store.state(&docket)? {
        let now = summary(p);
        let was = st.review.as_ref().map(|r| serde_json::to_value(r).unwrap_or_default());
        let changed = was.as_ref().is_none_or(|w| ["state", "phase", "round", "verdict", "merge_risk", "review_url", "title"].iter().any(|k| w[k] != now[k]));
        if changed {
            core.append(&docket, "observed", &Who::Hub, None, json!({ "what": "review", "source": "wheelhouse", "review": now }))?;
        }
        let want = status_for(&p.state);
        if st.status != want {
            core.append(&docket, "set", &Who::Hub, None, json!({ "status": want }))?;
        }
    }
    index_panes(core, p, &docket);
    core.live_changed();
    Ok(docket)
}

/// Keep the tracker's map of review panes up to date for this PR.
fn index_panes(core: &mut Core, p: &Pr, docket: &str) {
    core.review.panes.retain(|_, (d, _)| d != docket);
    if p.state != "reviewing" && p.state != "posted" {
        return;
    }
    for r in p.reviewers.values() {
        if let Some(l) = &r.pane {
            core.review.panes.insert(l.clone(), (docket.to_string(), format!("{} reviewer", r.name)));
        }
    }
    if let Some(l) = &p.checker_pane {
        core.review.panes.insert(l.clone(), (docket.to_string(), "checker".into()));
    }
}

/// Rebuild the pane map from every PR (at start).
pub fn load(core: &mut Core) -> Result<()> {
    for (key, docket, body) in core.store.review_prs()? {
        let Some(docket) = docket else { continue };
        if let Ok(p) = serde_json::from_str::<Pr>(&body) {
            index_panes(core, &p, &docket);
        } else {
            say!("review: can't read {key}");
        }
    }
    Ok(())
}

/// The Review docket and role of a pane in the review session, if it's one.
pub fn pane_docket(core: &Core, pane: &crate::mirror::Pane) -> Option<(String, String)> {
    if core.mirror.session_label(&pane.session_id) != session_name() {
        return None;
    }
    core.review.panes.get(&pane.label).cloned()
}

fn short(h: Option<&Value>) -> String {
    h.and_then(Value::as_str).map(|s| s.chars().take(7).collect()).unwrap_or_else(|| "?".into())
}

/// What happened to a PR, in words, on its docket.
pub fn event(core: &mut Core, p: &Pr, kind: &str, data: Value, who: &Who) -> Result<()> {
    let Some(docket) = docket_of(core, &p.key()) else { return Ok(()) };
    let d = &data;
    let head = short(d.get("head"));
    let body = match kind {
        "requested" if d["note"] == "reopened" => format!("Reopened and asking again: round {} on {head}.", d["round"]),
        "requested" => format!("{} asked you to review it: round {} on {head}.", d["author"].as_str().or(p.author.as_deref()).unwrap_or("The author"), d["round"]),
        "re-requested" => format!("Asked again: round {} on {head}.", d["round"]),
        "new-head" => format!("New commits ({head}); waiting for a re-request."),
        "unrequested" => "No longer asks for your review.".into(),
        "head-moved" => format!("The head moved to {head} mid-round."),
        "merged" => "Merged.".into(),
        "closed" => "Closed without merging.".into(),
        "pickup" => format!("Picked up by you: round {} on {head}.", d["round"]),
        "started" => format!("Round {} started on {head}{}.", d["round"], d["note"].as_str().map(|n| format!(": {n}")).unwrap_or_default()),
        "checking" => format!("The checker is checking round {}{}.", d["round"], d["effort"].as_str().map(|e| format!(" ({e})")).unwrap_or_default()),
        "awaiting" => format!("Round {}'s checked review is ready to post.", d["round"]),
        "delta" => format!("The head moved from {} to {head}: the reviewers review the new commits.", short(d.get("from"))),
        "repointed" => format!("The head moved from {} to {head}, but the PR's own diff didn't change: the checker checks again.", short(d.get("from"))),
        "decided" if d["auto"] == json!(true) => format!("Round {}'s checked review posts itself, by your rule: {}.", d["round"], d["why"].as_str().unwrap_or("?")),
        "decided" => format!("Round {}'s checked review waits for you: {}.", d["round"], d["why"].as_str().unwrap_or("?")),
        "post-refused" => format!("Your rule would post round {}, but the post was refused: {}. It waits for you.", d["round"], d["error"].as_str().unwrap_or("?")),
        "posted" => format!(
            "Posted round {}{}: {}{}.",
            d["round"],
            if d["by"] == "rule" { " by your rule" } else { "" },
            d["verdict"].as_str().unwrap_or("?").replace('_', " ").to_lowercase(),
            d["note"].as_str().map(|n| format!(", {n}")).unwrap_or_default()
        ),
        "dismissed" => format!("Round {} dismissed; nothing was posted.", d["round"]),
        "cleaned" => "Cleaned up: the panes, worktrees and refs.".into(),
        "imported" => format!("Imported from rex-reviewer: {} round(s) so far, {}.", p.round, p.state),
        other => other.to_string(),
    };
    let mut data = data.clone();
    data["what"] = json!("review-event");
    data["event"] = json!(kind);
    core.append(&docket, "observed", who, Some(body), data)?;
    Ok(())
}

// ---- Moving over from rex-reviewer ----

pub fn rex_reviewer_state_dir() -> PathBuf {
    std::env::var_os("REX_REVIEWER_STATE_DIR").map(PathBuf::from).unwrap_or_else(|| crate::paths::home().join(".local/state/rex-reviewer"))
}

/// Whether rex-reviewer's watcher has run lately (it steps at least every
/// few minutes, paused or not) and wasn't stopped with `rex-reviewer down`.
pub fn rex_reviewer_running() -> bool {
    let state: Value = std::fs::read_to_string(rex_reviewer_state_dir().join("state.json")).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    if state["stopped"] == json!(true) {
        return false;
    }
    let p = rex_reviewer_state_dir().join("watcher.json");
    let at = std::fs::read_to_string(&p).ok().and_then(|s| serde_json::from_str::<Value>(&s).ok()).and_then(|v| v["at"].as_str().map(str::to_string));
    at.and_then(|a| crate::time::secs_since(&a)).is_some_and(|s| s < 600)
}

/// Bring rex-reviewer's ledger in: each PR's state as it is, and its pause.
/// A PR Wheelhouse already has is left alone. Returns how many came in.
pub fn import(core: &mut Core) -> Result<usize> {
    let dir = rex_reviewer_state_dir();
    let mut n = 0;
    for owner in std::fs::read_dir(dir.join("repos")).into_iter().flatten().flatten() {
        for name in std::fs::read_dir(owner.path()).into_iter().flatten().flatten() {
            for f in std::fs::read_dir(name.path()).into_iter().flatten().flatten() {
                let path = f.path();
                let file = path.file_name().and_then(|x| x.to_str()).unwrap_or("");
                if !(file.starts_with("pr-") && file.ends_with(".json")) {
                    continue;
                }
                let Ok(text) = std::fs::read_to_string(&path) else { continue };
                let mut p: Pr = match serde_json::from_str(&text) {
                    Ok(p) => p,
                    Err(e) => {
                        say!("review: import {}: {e}", path.display());
                        continue;
                    }
                };
                if core.store.review_pr(&p.key())?.is_some() {
                    continue;
                }
                p.extra.insert("imported_from".into(), json!("rex-reviewer"));
                if p.finished() {
                    // History only: no docket until it comes back.
                    core.store.put_review_pr(&p.key(), &p.repo, p.pr, None, &serde_json::to_string(&p)?)?;
                } else {
                    save(core, &mut p)?;
                    event(core, &p, "imported", json!({}), &Who::Hub)?;
                }
                n += 1;
            }
        }
    }
    let state: Value = std::fs::read_to_string(dir.join("state.json")).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    if core.store.meta_get("review.paused")?.is_none() {
        core.store.meta_set("review.paused", if state["paused"] == json!(false) { "0" } else { "1" })?;
    }
    Ok(n)
}

#[cfg(test)]
mod link_tests {
    use super::*;

    #[test]
    fn the_hub_wont_write_through_a_link_in_tmp() {
        let root = std::env::temp_dir().join(format!("wh-no-link-{}", std::process::id()));
        let _gone = crate::hub::TestDir(root.clone());
        let elsewhere = root.join("elsewhere");
        let wt = root.join("wt");
        std::fs::create_dir_all(&elsewhere).unwrap();
        std::fs::create_dir_all(wt.join(".tmp")).unwrap();
        std::os::unix::fs::symlink(&elsewhere, wt.join(".tmp/context")).unwrap();
        let err = write_atomic(&wt.join(".tmp/context/round-1/pr.diff"), "x").unwrap_err();
        assert!(err.to_string().contains("is a link"), "{err}");
        assert!(!elsewhere.join("round-1").exists());
        write_atomic(&wt.join(".tmp/status.txt"), "ok\n").unwrap();
        // A .tmp that's itself a link.
        let wt2 = root.join("wt2");
        std::fs::create_dir_all(&wt2).unwrap();
        std::os::unix::fs::symlink(&elsewhere, wt2.join(".tmp")).unwrap();
        assert!(write_atomic(&wt2.join(".tmp/status.txt"), "x").is_err());
        let _ = std::fs::remove_dir_all(&root);
    }
}
