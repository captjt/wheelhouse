//! A round: prep, the reviewers, the checker, a moved head, the post, and
//! cleanup. Ported from rex-reviewer's `rounds.lua`; the caller holds the
//! PR's lock (`super::lock`).

use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::agentcli::{self, ReviewerLaunch};
use super::briefs::{Vars, brief, render};
use super::config::Config;
use super::panes::{self, Place};
use super::pr::{Pr, Reviewer};
use super::{github, rules};
use crate::hub::{Shared, Who};
use crate::record::fold::AgentRef;

pub const STALE_WORKING: i64 = 300;
/// An agent idle this long with its work unfinished has stalled.
pub const STALLED: i64 = 600;

const REVIEWER_PROMPT: &str = "Read .tmp/ground-rules.md, then .tmp/rounds/{n}/task.md, and do the task.";
const CHECKER_PROMPT: &str = "Read .tmp/rounds/{n}/check.md and do it.";
const DELTA_PROMPT: &str = "The PR's head moved while you reviewed. Read .tmp/rounds/{n}/delta-{h}.md and do it.";

fn now() -> String {
    crate::time::now()
}

fn epoch() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn short(s: &str) -> String {
    s.chars().take(7).collect()
}

// ---- git ----

/// What the hub's git never does, in repos and worktrees agents can write
/// to, while it holds your credentials: run hooks or an fsmonitor program
/// from the repo's config, skip TLS checks, or stall forever on a dead
/// connection.
pub(crate) const SAFE_GIT: [&str; 12] = [
    "-c",
    "core.hooksPath=/dev/null",
    "-c",
    "core.fsmonitor=false",
    "-c",
    "http.sslVerify=true",
    "-c",
    "protocol.ext.allow=never",
    "-c",
    "http.lowSpeedLimit=1000",
    "-c",
    "http.lowSpeedTime=60",
];

/// `-c core.sshCommand=…`: yours, from your global git config, or plain
/// `ssh`; never a repo's own (agents can write its .git/config, and it
/// would run with the hub's SSH agent). A second account's key belongs in
/// a `Host` alias in ~/.ssh/config, which still applies.
pub(crate) fn ssh_command() -> [String; 2] {
    static GLOBAL: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    let cmd = GLOBAL.get_or_init(|| {
        std::process::Command::new("git")
            .args(["config", "--global", "--get", "core.sshCommand"])
            .stdin(std::process::Stdio::null())
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "ssh".into())
    });
    ["-c".into(), format!("core.sshCommand={cmd}")]
}

/// How long one git command may take: a fetch of a big repo, at most.
const GIT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15 * 60);

pub(crate) async fn git(dir: &Path, args: &[&str]) -> Result<String> {
    let child = tokio::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(SAFE_GIT)
        .args(ssh_command())
        .args(args)
        .env("PATH", github::command_path())
        .env("GIT_TERMINAL_PROMPT", "0")
        .env_remove("CDPATH")
        .current_dir(crate::paths::home())
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true)
        .output();
    let out = tokio::time::timeout(GIT_TIMEOUT, child)
        .await
        .map_err(|_| anyhow::anyhow!("git {} took over {} minutes", args.iter().find(|a| !a.starts_with('-') && !a.contains('=')).unwrap_or(&""), GIT_TIMEOUT.as_secs() / 60))?
        .context("running git")?;
    if !out.status.success() {
        bail!("git {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Fetch from GitHub over HTTPS with your `gh` login, so it works without
/// an SSH agent (the hub runs as a LaunchAgent).
pub(crate) async fn fetch(checkout: &Path, repo: &str, refspecs: &[String]) -> Result<()> {
    let gh = github::gh_bin();
    let helper = format!("credential.helper=!'{}' auth git-credential", gh.display());
    // $WH_GIT_BASE points tests at a local origin.
    let url = format!("{}/{repo}.git", std::env::var("WH_GIT_BASE").unwrap_or_else(|_| "https://github.com".into()));
    let mut args: Vec<&str> = vec!["-c", "credential.helper=", "-c", &helper, "fetch", "--no-tags", "--quiet", &url];
    args.extend(refspecs.iter().map(String::as_str));
    git(checkout, &args).await.map(|_| ())
}

fn ref_for(pr: i64, round: i64) -> String {
    format!("refs/wheelhouse/pr-{pr}/round-{round}")
}

/// A round's ref: Wheelhouse's, or rex-reviewer's for an imported PR.
async fn existing_ref(checkout: &Path, pr: i64, round: i64) -> Option<String> {
    for r in [ref_for(pr, round), format!("refs/rex-reviewer/pr-{pr}/round-{round}")] {
        if git(checkout, &["rev-parse", "--verify", "--quiet", &r]).await.is_ok() {
            return Some(r);
        }
    }
    None
}

// ---- Files in a worktree ----

pub fn status(wt: &Path) -> String {
    status_in(wt, "status.txt")
}

/// The first line of `.tmp/<file>`, trimmed.
pub fn status_in(wt: &Path, file: &str) -> String {
    std::fs::read_to_string(wt.join(".tmp").join(file)).ok().and_then(|s| s.lines().next().map(|l| l.trim().to_string())).unwrap_or_default()
}

pub(crate) fn set_status(wt: &Path, s: &str) -> Result<()> {
    super::write_atomic(&wt.join(".tmp/status.txt"), &format!("{s}\n"))
}

fn status_mtime(wt: &Path) -> Option<i64> {
    std::fs::metadata(wt.join(".tmp/status.txt")).ok().map(|m| m.mtime())
}

fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    super::no_link(from)?;
    super::no_link(to)?;
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)? {
        let e = e?;
        let target = to.join(e.file_name());
        let ty = e.file_type()?;
        if ty.is_symlink() {
            continue;
        } else if ty.is_dir() {
            copy_dir(&e.path(), &target)?;
        } else {
            std::fs::copy(e.path(), target)?;
        }
    }
    Ok(())
}

fn read_json(p: &Path) -> Option<Value> {
    std::fs::read_to_string(p).ok().and_then(|s| serde_json::from_str(&s).ok())
}

// ---- Who reviews, where ----

/// Where a PR's worktrees go: beside its primary's, if it has one (an
/// imported PR keeps rex-reviewer's, so its sessions resume), else
/// Wheelhouse's own.
fn worktree_path(e: &Pr, name: Option<&str>) -> PathBuf {
    let base = e
        .worktree
        .as_deref()
        .map(PathBuf::from)
        .filter(|w| w.file_name().and_then(|n| n.to_str()) == Some(&format!("pr-{}", e.pr)))
        .and_then(|w| w.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| super::data_dir().join("worktrees").join(&e.repo));
    base.join(match name {
        None => format!("pr-{}", e.pr),
        Some(n) => format!("pr-{}-{n}", e.pr),
    })
}

/// Pane labels: `<prefix>-<pr>`, `-<name>`, `-check`, at most 32 long.
fn prefix(repo: &str) -> String {
    let name = repo.rsplit('/').next().unwrap_or(repo).to_lowercase();
    let p: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '-' }).collect();
    if p.starts_with(|c: char| c.is_ascii_lowercase()) { p } else { format!("pr-{p}") }
}

fn label(e: &Pr, cfg: &Config, suffix: Option<&str>) -> String {
    let longest = cfg.reviewers.iter().map(|r| r.name.len()).chain(std::iter::once("check".len())).max().unwrap_or(5);
    let room = 32usize.saturating_sub(format!("-{}-", e.pr).len() + longest).max(1);
    let mut p = prefix(&e.repo);
    if p.len() > room {
        p = p[..room].trim_end_matches(['-', '_']).to_string();
    }
    match suffix {
        None => format!("{p}-{}", e.pr),
        Some(s) => format!("{p}-{}-{s}", e.pr),
    }
}

fn port(index: i64, pr: i64) -> i64 {
    20000 + 10000 * (index.max(1) - 1) + pr % 10000
}

/// The round's reviewers: the ones it started with while it runs, else the
/// config's, keeping each one's session when it can resume.
fn round_reviewers(e: &Pr, cfg: &Config) -> Vec<Reviewer> {
    if e.state == "reviewing" && !e.round_reviewers.is_empty() {
        let rs = e.round_reviewer_list();
        if !rs.is_empty() {
            return rs;
        }
    }
    let list: Vec<_> = if e.state == "reviewing" { cfg.reviewers.iter().take(1).collect() } else { cfg.reviewers.iter().collect() };
    list.into_iter()
        .map(|cr| {
            let mut r = match e.reviewers.get(&cr.name) {
                Some(r) if r.agent == cr.agent => r.clone(),
                _ => Reviewer { agent: cr.agent.clone(), ..Default::default() },
            };
            r.name = cr.name.clone();
            r.index = cr.index as i64;
            r.primary = cr.index == 1;
            let wt = worktree_path(e, if r.primary { None } else { Some(&cr.name) }).display().to_string();
            // A session resumes only in the folder it started in.
            if r.worktree.as_deref().is_some_and(|w| w != wt) {
                r.session = None;
            }
            r.worktree = Some(wt);
            r.pane = Some(label(e, cfg, if r.primary { None } else { Some(&cr.name) }));
            r.block = None;
            r
        })
        .collect()
}

// ---- What the policy and briefs see ----

fn delta_lines(diff: &str) -> i64 {
    let mut n = 0;
    let mut in_hunk = false;
    for l in diff.lines() {
        if l.starts_with("diff ") {
            in_hunk = false;
        } else if l.starts_with("@@") {
            in_hunk = true;
        } else if in_hunk && (l.starts_with('+') || l.starts_with('-')) {
            n += 1;
        }
    }
    n
}

/// The PR as the effort policy sees it (rex-reviewer's view, field for field).
pub fn effort_view(e: &Pr, round: i64, drafts: Option<&[Reviewer]>) -> Value {
    let ctx = e.worktree.as_deref().map(|w| Path::new(w).join(format!(".tmp/context/round-{round}")));
    let prj = ctx.as_ref().and_then(|c| read_json(&c.join("pr.json"))).unwrap_or_default();
    let files: Vec<Value> = prj["files"].as_array().into_iter().flatten().filter_map(|f| f["path"].as_str()).map(|p| json!(p)).collect();
    let labels: Vec<Value> = prj["labels"].as_array().into_iter().flatten().filter_map(|l| l["name"].as_str()).map(|p| json!(p)).collect();
    let (a, d) = (e.additions.unwrap_or(0), e.deletions.unwrap_or(0));
    let mut v = json!({
        "repo": e.repo, "pr": e.pr, "title": e.title, "author": e.author, "round": round,
        "additions": a, "deletions": d, "lines": a + d,
        "changed_files": e.changed_files.unwrap_or(files.len() as i64), "files": files, "labels": labels,
    });
    if round > 1
        && let Some(diff) = ctx.as_ref().and_then(|c| std::fs::read_to_string(c.join("delta.diff")).ok())
    {
        v["delta_lines"] = json!(delta_lines(&diff));
    }
    if let Some(h) = e.history.iter().rev().find(|h| h["by"] != "you") {
        v["last_verdict"] = h["verdict"].clone();
        v["last_blocking"] = h["blocking"].clone();
        v["last_merge_risk"] = h["merge_risk"].clone();
    }
    if let Some(rs) = drafts {
        let (mut n, mut b) = (0, 0);
        for r in rs {
            if let Some(d) = r.worktree.as_deref().and_then(|w| read_json(&Path::new(w).join(format!(".tmp/rounds/{round}/review.json")))) {
                n += d["findings"].as_array().map_or(0, Vec::len);
                b += super::post::blocking_count(&d);
            }
        }
        v["drafted"] = json!(n);
        v["drafted_blocking"] = json!(b);
    }
    if let Some(o) = v.as_object_mut() {
        o.retain(|_, x| !x.is_null());
    }
    v
}

fn one_line(s: &str) -> String {
    s.lines().map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(" ")
}

/// The briefs' placeholders for a round, for one reviewer (or the checker).
fn vars(cfg: &Config, e: &Pr, round: i64, r: Option<&Reviewer>, rs: &[Reviewer], login: &str, mode: &str) -> Result<Vars> {
    let entry = serde_json::to_value(e)?;
    let mut v = Vars::new();
    v.insert("REPO", e.repo.clone());
    v.insert("PR", e.pr.to_string());
    if let Some(t) = &e.title {
        v.insert("TITLE", t.clone());
    }
    if let Some(a) = &e.author {
        v.insert("AUTHOR", a.clone());
    }
    v.insert("LOGIN", login.to_string());
    v.insert("BASE", e.base.clone().unwrap_or_else(|| "main".into()));
    v.insert("ROUND", round.to_string());
    let head = e.round_head.clone().unwrap_or_default();
    v.insert("HEAD_SHORT", short(&head));
    v.insert("HEAD", head);
    v.insert("PREV_ROUND", (round - 1).to_string());
    v.insert("PREV_HEAD", e.reviewed_head.clone().unwrap_or_else(|| "?".into()));
    v.insert("PREV_HEAD_SHORT", e.reviewed_head.as_deref().map(short).unwrap_or_else(|| "?".into()));
    v.insert("CONTEXT", format!(".tmp/context/round-{round}"));
    v.insert("OUT", format!(".tmp/rounds/{round}"));
    v.insert("PORT", port(r.map_or(1, |r| r.index), e.pr).to_string());
    v.insert("DRAFT_SCHEMA", super::draft_schema().display().to_string());
    let notes = cfg.repos.get(&e.repo).map(|r| r.notes.trim().to_string()).unwrap_or_default();
    v.insert(
        "REPO_NOTES",
        if notes.is_empty() { String::new() } else { format!("## Notes for this repo\n\n{}\n", notes.replace("## Standing decisions", "### Standing decisions")) },
    );
    v.insert("N", rs.len().to_string());
    v.insert("PRIMARY", rs.first().map(|r| r.name.clone()).unwrap_or_default());
    let review = match cfg.guide(&e.repo, "review", r.map(|r| r.name.as_str()), &entry)? {
        Some(g) => g,
        None => one_line(&render(&brief("parts/review-default.md"), &v)),
    };
    v.insert("REVIEW_GUIDE", review);
    v.insert("STYLE_GUIDE", cfg.guide(&e.repo, "style", None, &entry)?.unwrap_or_else(|| "Write for the PR author, plainly and briefly.".into()));
    let risk = cfg.guide(&e.repo, "merge_risk", None, &entry)?;
    v.insert("MERGE_RISK_GUIDE", risk.clone().unwrap_or_default());
    v.insert("CHECK_GUIDE", cfg.guide(&e.repo, "check", None, &entry)?.map(|g| format!("## More instructions\n\n{g}\n")).unwrap_or_default());
    v.insert("PREVIEW_CMD", format!("wh review preview {} {}", e.repo, e.pr));
    v.insert("POST_CMD", format!("wh review post {} {}", e.repo, e.pr));
    let names: Vec<String> = rs.iter().map(|r| format!("\"{}\"", r.name)).collect();
    v.insert("FOUND_BY_EXAMPLE", format!("`[{}]`", if names.is_empty() { "\"claude\"".to_string() } else { names.join(", ") }));
    v.insert("THE_REVIEWERS", if rs.len() > 1 { "the reviewers" } else { "the reviewer" }.into());
    let start = if risk.is_some() { render(&brief("parts/merge-risk-start.md"), &v) } else { String::new() };
    let add = if risk.is_some() { render(&brief("parts/merge-risk-add.md"), &v) } else { String::new() };
    let post = render(&brief(if mode == "auto" { "parts/post-auto.md" } else { "parts/post-ask.md" }), &v);
    let parallel = if rs.len() > 1 { render(&brief("parts/parallel.md"), &v).trim().to_string() } else { String::new() };
    v.insert("MERGE_RISK_START", start);
    v.insert("MERGE_RISK_ADD", add);
    v.insert("POST_STEP", post);
    v.insert("PARALLEL", parallel);
    Ok(v)
}

// ---- Prep ----

/// Fetch the PR's head into the round's ref, put every reviewer's worktree
/// at it, and write the round's context. Also re-preps the round in
/// progress when its head moved.
pub async fn prep(shared: &Shared, key: &str) -> Result<()> {
    let cfg = super::config(shared)?;
    let mut e = super::get(&shared.lock(), key)?.ok_or_else(|| anyhow!("{key} isn't tracked"))?;
    if e.finished() {
        bail!("{key} is {}", e.state);
    }
    let round = if e.state == "reviewing" { e.round } else { e.round + 1 };
    let checkout = cfg.repos.get(&e.repo).map(|r| r.checkout.clone()).ok_or_else(|| anyhow!("{} isn't in the review config", e.repo))?;
    let rf = ref_for(e.pr, round);
    fetch(&checkout, &e.repo, &[format!("+refs/pull/{}/head:{rf}", e.pr)]).await?;
    let base = e.base.clone().unwrap_or_else(|| "main".into());
    if let Err(err) = fetch(&checkout, &e.repo, &[format!("+refs/heads/{base}:refs/remotes/origin/{base}")]).await {
        say!("review: {key}: fetching {base}: {err:#}");
    }
    let head = git(&checkout, &["rev-parse", &rf]).await?.trim().to_string();
    let rs = round_reviewers(&e, &cfg);
    for r in &rs {
        let wt = PathBuf::from(r.worktree.as_deref().unwrap_or_default());
        if wt.join(".git").exists() {
            git(&wt, &["checkout", "--detach", "--quiet", &head]).await?;
        } else {
            if let Some(p) = wt.parent() {
                std::fs::create_dir_all(p)?;
            }
            git(&checkout, &["worktree", "add", "--detach", "--quiet", &wt.to_string_lossy(), &head]).await?;
        }
        if !r.primary && !e.extra_worktrees.iter().any(|x| Some(x.as_str()) == r.worktree.as_deref()) {
            e.extra_worktrees.push(r.worktree.clone().unwrap_or_default());
        }
    }
    let primary = PathBuf::from(rs.first().and_then(|r| r.worktree.clone()).ok_or_else(|| anyhow!("no reviewers"))?);
    let ctx = primary.join(format!(".tmp/context/round-{round}"));
    github::fetch_context(&e.repo, e.pr, &ctx).await?;
    // What the PR is for: its Linear issues (and `linear.md` for the
    // reviewers) and the docket's intent. A failure here doesn't stop the
    // round.
    if let Some(f) = super::links::Facts::from_context(&ctx) {
        match super::links::integrate(shared, key, &f, Some(&ctx)).await {
            Ok(()) => e.integrated_at = Some(crate::time::now()),
            Err(err) => say!("review: {key}: linking: {err:#}"),
        }
    }
    if let Some(p) = read_json(&ctx.join("pr.json")) {
        if let Some(b) = p["baseRefName"].as_str() {
            if b != base {
                let _ = fetch(&checkout, &e.repo, &[format!("+refs/heads/{b}:refs/remotes/origin/{b}")]).await;
            }
            e.base = Some(b.to_string());
        }
        e.additions = p["additions"].as_i64();
        e.deletions = p["deletions"].as_i64();
        e.changed_files = p["changedFiles"].as_i64();
    }
    if round > 1
        && let Some(prev) = existing_ref(&checkout, e.pr, round - 1).await
    {
        let range = format!("{prev}..{rf}");
        super::write_atomic(&ctx.join("delta.diff"), &git(&checkout, &["diff", "--no-ext-diff", "--no-textconv", &prev, &rf]).await.unwrap_or_default())?;
        super::write_atomic(&ctx.join("delta-commits.txt"), &git(&checkout, &["log", "--oneline", &range]).await.unwrap_or_default())?;
    }
    if e.state == "reviewing"
        && let Some(from) = e.round_head.clone().filter(|h| *h != head)
    {
        let range = format!("{from}..{head}");
        super::write_atomic(&ctx.join("delta-head.diff"), &git(&checkout, &["diff", "--no-ext-diff", "--no-textconv", &from, &head]).await.unwrap_or_default())?;
        super::write_atomic(&ctx.join("delta-head-commits.txt"), &git(&checkout, &["log", "--oneline", &range]).await.unwrap_or_default())?;
    }
    for r in rs.iter().skip(1) {
        let other = PathBuf::from(r.worktree.as_deref().unwrap_or_default()).join(format!(".tmp/context/round-{round}"));
        super::no_link(&other)?;
        let _ = std::fs::remove_dir_all(&other);
        copy_dir(&ctx, &other)?;
    }
    e.worktree = Some(primary.display().to_string());
    e.round_head = Some(head);
    e.prepared_round = Some(round);
    let mut core = shared.lock();
    super::save(&mut core, &mut e)?;
    Ok(())
}

// ---- Panes ----

fn last_lines(s: &str, n: usize) -> String {
    let l: Vec<&str> = s.lines().collect();
    l[l.len().saturating_sub(n)..].join("\n")
}

fn app_of(agent: &str) -> &'static str {
    match agent {
        "claude" => "claude-code",
        "agy" => "agy",
        "pi" => "pi",
        _ => "codex",
    }
}

/// Tell the tracker a pane is this PR's, before it can be seen as a stray.
fn expect(shared: &Shared, docket: &str, label: &str, role: &str) {
    shared.lock().review.panes.insert(label.to_string(), (docket.to_string(), role.to_string()));
}

#[allow(clippy::too_many_arguments)]
fn adopt(shared: &Shared, docket: &str, agent: &str, block: &str, label: &str, cwd: &Path, prompt: &str, role: &str, round: i64) {
    let a = AgentRef {
        app: app_of(agent).into(),
        host: "local".into(),
        block: block.into(),
        rex_session: super::session_name(),
        pane: label.into(),
        cwd: Some(crate::paths::tilde(&cwd.to_string_lossy())),
    };
    let mut core = shared.lock();
    crate::agents::adopt(&mut core, docket, &a, Some(prompt));
    let _ = core.append(docket, "claim", &Who::Hub, None, json!({ "source": "wheelhouse", "role": role, "round": round, "agent": a }));
}

/// A PR's pane: by its block, or by label in the session it opened in.
fn pane_block(shared: &Shared, e: &Pr, block: Option<&str>, label: Option<&str>) -> Option<String> {
    if let Some(b) = block
        && panes::state(shared, b).is_some()
    {
        return Some(b.to_string());
    }
    panes::find(shared, e.session_id.as_deref(), label?)
}

fn shared_caches(cfg: &Config) -> Vec<PathBuf> {
    let cargo = super::cache_dir().join("cargo");
    let _ = std::fs::create_dir_all(&cargo);
    for d in ["registry", "git", "advisory-dbs"] {
        let src = crate::paths::home().join(".cargo").join(d);
        let link = cargo.join(d);
        if src.exists() && link.symlink_metadata().is_err() {
            let _ = std::os::unix::fs::symlink(&src, &link);
        }
    }
    let mut out = vec![super::cache_dir()];
    out.extend(cfg.caches.iter().filter(|c| c.exists()).cloned());
    out
}

async fn wait_codex_thread(wt: &Path, since: i64) -> Option<String> {
    for _ in 0..15 {
        if let Some(t) = agentcli::codex_thread(wt, since) {
            return Some(t);
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    None
}

/// Open one reviewer's pane and hand it `prompt`. Returns its block.
#[allow(clippy::too_many_arguments)]
async fn open_reviewer(
    shared: &Shared,
    cfg: &Config,
    e: &Pr,
    docket: &str,
    r: &mut Reviewer,
    resume: bool,
    place: Place<'_>,
    prompt: &str,
    round: i64,
) -> Result<(String, String)> {
    let wt = PathBuf::from(r.worktree.as_deref().unwrap_or_default());
    let checkout = &cfg.repos[&e.repo].checkout;
    let dirs: Vec<PathBuf> = [checkout.join(".git/worktrees")].into_iter().filter(|d| d.exists()).collect();
    let caches = shared_caches(cfg);
    let extra = cfg.reviewer(&r.name).map(|c| c.extra_args.clone()).unwrap_or_default();
    let title = format!("wheelhouse review {}{}", e.key(), if r.primary { String::new() } else { format!(" {}", r.name) });
    let l = agentcli::reviewer(
        cfg,
        &ReviewerLaunch {
            agent: &r.agent,
            extra_args: &extra,
            resume: if resume { r.session.as_deref() } else { None },
            worktree: &wt,
            dirs: &dirs,
            caches: &caches,
            effort: r.effort.as_deref(),
            title: &title,
        },
    );
    let label = r.pane.clone().unwrap_or_else(|| label(e, cfg, None));
    let role = format!("{} reviewer", r.name);
    expect(shared, docket, &label, &role);
    let since = epoch();
    let created = panes::open(shared, &label, &wt.to_string_lossy(), &l.argv, place).await?;
    adopt(shared, docket, &r.agent, &created.block_id, &label, &wt, prompt, &role, round);
    let program = agentcli::program(cfg, &r.agent);
    let block = created.block_id.clone();
    let session = created.session_id.clone();
    let why = match panes::settled(shared, &block, Duration::from_secs(180)).await {
        Ok(s) if s == "blocked" => Some("it's waiting on a prompt (a trust or permission question?)".to_string()),
        Ok(_) => None,
        Err(why) => Some(format!("{why:#}")),
    };
    if let Some(why) = why {
        let tail = panes::tail(&session, &block).await;
        panes::close(&session, &block).await;
        if tail.contains("Hooks need review") {
            bail!("Codex is waiting for its hooks to be trusted. Run codex once, open /hooks, and trust them; the round retries on its own.");
        }
        bail!("the {} reviewer ({program}) didn't start: {why}\n{}", r.name, last_lines(&tail, 6));
    }
    r.session = if r.agent == "codex" {
        wait_codex_thread(&wt, since).await.or(l.session)
    } else {
        l.session.or_else(|| shared.lock().agents.by_block.get(&block).and_then(|t| t.session.clone()))
    };
    r.block = Some(block.clone());
    r.pane = Some(label.clone());
    if let Err(why) = panes::deliver(shared, &session, &block, prompt).await {
        panes::close(&session, &block).await;
        bail!("{label} didn't take its task: {why:#}");
    }
    if r.agent == "agy" && r.session.is_none() {
        r.session = panes::agy_session(shared, &block, &wt, since).await;
    }
    Ok((session, block))
}

// ---- Start ----

/// Start the round prep made: every reviewer in its own pane, blind to the
/// others. All or nothing: if one fails, the panes opened are closed and
/// nothing is saved, so the next try starts clean.
pub async fn start(shared: &Shared, key: &str) -> Result<()> {
    let cfg = super::config(shared)?;
    let (mut e, docket, mode) = {
        let core = shared.lock();
        let e = super::get(&core, key)?.ok_or_else(|| anyhow!("{key} isn't tracked"))?;
        let d = super::docket_of(&core, key).ok_or_else(|| anyhow!("{key} has no docket"))?;
        let mode = super::post_mode(&core, &cfg, Some(&e.repo));
        (e, d, mode)
    };
    if e.state != "queued" {
        bail!("{key} is {}, not queued", e.state);
    }
    let round = e.round + 1;
    if e.prepared_round != Some(round) || e.worktree.is_none() {
        bail!("{key}: prep round {round} first");
    }
    let login = github::login().await?;
    let mut rs = round_reviewers(&e, &cfg);
    for r in &rs {
        if !Path::new(r.worktree.as_deref().unwrap_or_default()).join(".git").exists() {
            bail!("{key}: {}'s worktree isn't there; prep again", r.name);
        }
    }
    let view = effort_view(&e, round, None);
    for r in rs.iter_mut() {
        r.effort = cfg.effort(&e.repo, "review", &view, Some(&r.name))?;
    }
    e.checker_effort = None;
    e.checker_agent = None;
    // Panes left from an earlier round with these labels go first.
    for l in rs.iter().filter_map(|r| r.pane.clone()).chain(std::iter::once(label(&e, &cfg, Some("check")))) {
        if let (Some(b), Some(s)) = (panes::find(shared, None, &l), panes::session(shared)) {
            panes::close(&s, &b).await;
        }
    }
    let prompt = REVIEWER_PROMPT.replace("{n}", &round.to_string());
    let snapshot = rs.clone();
    let mut opened: Vec<(String, String)> = Vec::new();
    let mut anchor: Option<String> = None;
    let mut failure = None;
    for (i, r) in rs.iter_mut().enumerate() {
        let wt = PathBuf::from(r.worktree.clone().unwrap_or_default());
        let run = async {
            let v = vars(&cfg, &e, round, Some(r), &snapshot, &login, mode)?;
            for d in [format!(".tmp/rounds/{round}"), ".tmp/probes".into(), ".tmp/gh-empty".into()] {
                std::fs::create_dir_all(wt.join(d))?;
            }
            super::write_atomic(&wt.join(".tmp/ground-rules.md"), &render(&brief("ground-rules.md"), &v))?;
            let fresh = round == 1 || r.session.is_none();
            super::write_atomic(&wt.join(format!(".tmp/rounds/{round}/task.md")), &render(&brief(if fresh { "round-1.md" } else { "round-n.md" }), &v))?;
            set_status(&wt, &format!("WORKING round {round}"))?;
            if r.session.is_none() {
                r.first_round = Some(round);
                r.tokens_seen = None;
            }
            let place = match &anchor {
                None => Place::Tab,
                Some(a) => Place::Beside { anchor: a, dir: if i == 1 { "right" } else { "below" } },
            };
            open_reviewer(shared, &cfg, &e, &docket, r, !fresh, place, &prompt, round).await
        };
        match run.await {
            Ok((s, b)) => {
                anchor.get_or_insert(b.clone());
                opened.push((s, b));
            }
            Err(err) => {
                failure = Some(err);
                break;
            }
        }
    }
    if let Some(err) = failure {
        for (s, b) in &opened {
            panes::close(s, b).await;
        }
        let mut core = shared.lock();
        let _ = super::load(&mut core);
        return Err(err);
    }
    let session = opened.first().map(|(s, _)| s.clone());
    let head = e.round_head.clone().unwrap_or_default();
    for r in &rs {
        e.reviewers.insert(r.name.clone(), r.clone());
    }
    rules::mark_started(&mut e, &head, &now());
    e.round_reviewers = rs.iter().map(|r| r.name.clone()).collect();
    e.reviewers_total = Some(rs.len() as i64);
    e.reviewers_done = Some(0);
    e.reviewer = rs.first().map(|r| r.agent.clone());
    e.reviewer_session = rs.first().and_then(|r| r.session.clone());
    e.pane = rs.first().and_then(|r| r.pane.clone());
    e.phase = Some("reviewer".into());
    e.session_id = session;
    e.checker_pane = None;
    e.checker_block = None;
    e.start_error = None;
    e.start_failed_epoch = None;
    let effort: serde_json::Map<String, Value> = rs.iter().map(|r| (r.name.clone(), json!(r.effort))).collect();
    let note = format!(
        "{} reviewer{}: {}",
        rs.len(),
        if rs.len() == 1 { "" } else { "s" },
        rs.iter().map(|r| format!("{}{}", r.name, r.effort.as_deref().map(|x| format!(" ({x})")).unwrap_or_default())).collect::<Vec<_>>().join(", ")
    );
    let mut core = shared.lock();
    super::save(&mut core, &mut e)?;
    super::event(&mut core, &e, "started", json!({ "round": round, "head": head, "note": note, "effort": effort }), &Who::Hub)?;
    Ok(())
}

// ---- Progress ----

#[derive(Debug, Default)]
pub struct Progress {
    pub done: i64,
    pub total: i64,
    pub ready: bool,
    pub blocked: Option<(String, String)>,
    pub closed: Option<String>,
    pub stalled: Option<String>,
    pub stale: Vec<String>,
}

fn secs_since_rex(at: Option<&str>) -> Option<i64> {
    let t = time::OffsetDateTime::parse(at?, &time::format_description::well_known::Rfc3339).ok()?;
    Some((time::OffsetDateTime::now_utc() - t).whole_seconds())
}

/// Where the round's reviewers are.
pub fn progress(shared: &Shared, e: &Pr) -> Progress {
    let rs = e.round_reviewer_list();
    let mut p = Progress { total: rs.len() as i64, ..Default::default() };
    for r in &rs {
        let wt = PathBuf::from(r.worktree.as_deref().unwrap_or_default());
        let st = status(&wt);
        let block = pane_block(shared, e, r.block.as_deref(), r.pane.as_deref());
        let pane = block.as_deref().and_then(|b| panes::state(shared, b));
        let pstate = pane.as_ref().map(|(s, _)| s.as_str());
        let since = pane.as_ref().and_then(|(_, at)| secs_since_rex(at.as_deref()));
        if let Some(why) = st.strip_prefix("BLOCKED") {
            p.blocked.get_or_insert((r.name.clone(), why.trim_start_matches(':').trim().to_string()));
        } else if st == "READY_TO_POST" && pstate != Some("working") {
            p.done += 1;
        } else if st == "READY_TO_POST" {
            // Codex can stay "working" after its turn (a background
            // terminal): done if the status was written after it began
            // working, a while ago.
            let mtime = status_mtime(&wt).unwrap_or(0);
            let began = since.map(|s| epoch() - s).unwrap_or(i64::MAX);
            if mtime > began && epoch() - mtime >= STALE_WORKING {
                p.done += 1;
                p.stale.push(r.name.clone());
            }
        } else if st.starts_with("WORKING") && pane.is_none() {
            p.closed.get_or_insert(r.name.clone());
        } else if st.starts_with("WORKING") && matches!(pstate, Some("idle" | "done")) && since.is_some_and(|s| s >= STALLED) {
            p.stalled.get_or_insert(r.name.clone());
        }
    }
    p.ready = p.total > 0 && p.done == p.total;
    p
}

// ---- The checker ----

/// Every reviewer is done and the head hasn't moved: start the checker in
/// the primary's worktree, beside its pane.
pub async fn start_checker(shared: &Shared, key: &str) -> Result<()> {
    let cfg = super::config(shared)?;
    let (mut e, docket, mode) = {
        let core = shared.lock();
        let e = super::get(&core, key)?.ok_or_else(|| anyhow!("{key} isn't tracked"))?;
        let d = super::docket_of(&core, key).ok_or_else(|| anyhow!("{key} has no docket"))?;
        let mode = super::post_mode(&core, &cfg, Some(&e.repo));
        (e, d, mode)
    };
    let round = e.round;
    let rs = e.round_reviewer_list();
    let primary = PathBuf::from(e.worktree.clone().ok_or_else(|| anyhow!("{key} has no worktree"))?);
    let out = format!(".tmp/rounds/{round}");
    let mut others = Vec::new();
    for r in rs.iter().skip(1) {
        let wt = PathBuf::from(r.worktree.as_deref().unwrap_or_default());
        let dest = primary.join(format!("{out}/reviewers/{}", r.name));
        std::fs::create_dir_all(&dest)?;
        for f in ["review.json", "report.md"] {
            let src = wt.join(format!("{out}/{f}"));
            if src.exists() {
                std::fs::copy(&src, dest.join(f))?;
            }
        }
        others.push(wt);
    }
    let view = effort_view(&e, round, Some(&rs));
    e.checker_effort = cfg.effort(&e.repo, "check", &view, None)?;
    let login = github::login().await?;
    let mut v = vars(&cfg, &e, round, None, &rs, &login, mode)?;
    let head = e.round_head.clone().unwrap_or_default();
    let program = |a: &str| agentcli::program(&cfg, a);
    if rs.len() <= 1 {
        v.insert("DRAFTS", format!("A reviewer reviewed head {head} in this worktree and drafted `{out}/review.json` and `{out}/report.md`."));
        v.insert("DRAFT_FILES", format!("`{out}/review.json`, `{out}/report.md`"));
        v.insert("MERGE_DRAFTS", String::new());
    } else {
        let mut d = format!("{} reviewers reviewed head {head}, each on its own in its own worktree, and each drafted a review:\n\n", rs.len());
        for r in &rs {
            if r.primary || Some(&r.name) == rs.first().map(|x| &x.name) {
                d.push_str(&format!("- `{}` ({}), in this worktree: `{out}/review.json` and `{out}/report.md`.\n", r.name, program(&r.agent)));
            } else {
                d.push_str(&format!(
                    "- `{}` ({}), in `{}`: copied to `{out}/reviewers/{}/review.json` and `report.md`. Its probes and builds are in its worktree.\n",
                    r.name,
                    program(&r.agent),
                    r.worktree.as_deref().unwrap_or_default(),
                    r.name
                ));
            }
        }
        v.insert("DRAFTS", d.trim_end().to_string());
        v.insert("DRAFT_FILES", "each draft and report above".into());
        let md = render(&brief("parts/merge-drafts.md"), &v);
        v.insert("MERGE_DRAFTS", md);
    }
    super::write_atomic(&primary.join(format!("{out}/check.md")), &render(&brief("check.md"), &v))?;
    if cfg.merge_risk_enabled(&e.repo, &serde_json::to_value(&e)?) {
        super::write_atomic(&primary.join(format!("{out}/merge-risk.md")), &render(&brief("merge-risk.md"), &v))?;
    }
    set_status(&primary, &format!("VERIFYING round {round}"))?;
    let check_label = label(&e, &cfg, Some("check"));
    if let (Some(b), Some(s)) = (pane_block(shared, &e, e.checker_block.as_deref(), Some(&check_label)), e.session_id.clone().or_else(|| panes::session(shared))) {
        panes::close(&s, &b).await;
    }
    let title = format!("wheelhouse review {} round {round} check", e.key());
    let l = agentcli::checker(&cfg, &primary, &others, e.checker_effort.as_deref(), mode == "auto", &title);
    let primary_block = rs.first().and_then(|r| pane_block(shared, &e, r.block.as_deref(), r.pane.as_deref()));
    let place = match &primary_block {
        Some(b) => Place::Beside { anchor: b, dir: "left" },
        None => Place::Tab,
    };
    expect(shared, &docket, &check_label, "checker");
    let since = epoch();
    let created = panes::open(shared, &check_label, &primary.to_string_lossy(), &l.argv, place).await?;
    let prompt = CHECKER_PROMPT.replace("{n}", &round.to_string());
    adopt(shared, &docket, &cfg.checker, &created.block_id, &check_label, &primary, &prompt, "checker", round);
    let ok = match panes::settled(shared, &created.block_id, Duration::from_secs(120)).await {
        Ok(s) if s == "blocked" => Err(anyhow!("it's waiting on a prompt")),
        Ok(_) => panes::deliver(shared, &created.session_id, &created.block_id, &prompt).await,
        Err(e) => Err(e),
    };
    if let Err(why) = ok {
        let tail = panes::tail(&created.session_id, &created.block_id).await;
        panes::close(&created.session_id, &created.block_id).await;
        bail!("the checker ({}) didn't start: {why:#}\n{}", program(&cfg.checker), last_lines(&tail, 6));
    }
    let session = if cfg.checker == "codex" {
        wait_codex_thread(&primary, since).await
    } else if cfg.checker == "agy" {
        panes::agy_session(shared, &created.block_id, &primary, since).await
    } else {
        l.session.or_else(|| shared.lock().agents.by_block.get(&created.block_id).and_then(|t| t.session.clone()))
    };
    if cfg.checker == "codex"
        && let Some(t) = &session
    {
        e.checker_threads.push(t.clone());
    }
    e.checker_session = session;
    e.checker_agent = Some(cfg.checker.clone());
    e.checker_post = Some(mode.to_string());
    e.post_decision = None;
    e.phase = Some("checking".into());
    e.checker_pane = Some(check_label);
    e.checker_block = Some(created.block_id);
    e.reviewers_done = Some(rs.len() as i64);
    let mut core = shared.lock();
    super::save(&mut core, &mut e)?;
    super::event(&mut core, &e, "checking", json!({ "round": round, "head": head, "effort": e.checker_effort }), &Who::Hub)?;
    Ok(())
}

// ---- A head that moved mid-round ----

/// The PR is at `live` on GitHub now (read live, ahead of the next scan):
/// keep it, put it on the record if it's news, and wake the watcher so it
/// follows at once.
pub fn saw_head(shared: &Shared, key: &str, live: &str) -> Result<()> {
    let mut core = shared.lock();
    let Some(mut e) = super::get(&core, key)? else { return Ok(()) };
    if e.head.as_deref() != Some(live) {
        e.head = Some(live.to_string());
        super::save(&mut core, &mut e)?;
        if e.state == "reviewing" && e.round_head.as_deref() != Some(live) {
            super::event(&mut core, &e, "head-moved", json!({ "round": e.round, "head": live, "round_head": e.round_head }), &Who::Hub)?;
        }
    }
    if e.state == "reviewing" && e.round_head.as_deref() != Some(live) {
        core.review.check_now.notify_one();
    }
    Ok(())
}

/// Where the round's PR is now, when it isn't where the round is: `None`
/// while the round still covers its head. The last scan's head is only a
/// hint, read again live before acting on it (a scan from before the round
/// started is behind it, not ahead); with `confirm`, it's read live even
/// when the scan agrees.
pub async fn moved_head(shared: &Shared, e: &Pr, confirm: bool) -> Result<Option<String>> {
    let Some(rh) = e.round_head.as_deref() else { return Ok(None) };
    if !confirm && e.head.as_deref().is_none_or(|h| h == rh) {
        return Ok(None);
    }
    let live = github::live_head(&e.repo, e.pr).await?;
    if live.is_empty() {
        return Ok(None);
    }
    saw_head(shared, &e.key(), &live)?;
    Ok((live != rh).then_some(live))
}

/// The PR's own diff (against where it branched from the base), without
/// index lines, so a merge of the base into it compares equal.
async fn pr_diff(checkout: &Path, base: &str, sha: &str) -> Result<String> {
    let mb = git(checkout, &["merge-base", &format!("origin/{base}"), sha]).await?;
    let d = git(checkout, &["diff", "--no-ext-diff", "--no-textconv", mb.trim(), sha]).await?;
    Ok(d.lines().filter(|l| !l.starts_with("index ")).collect::<Vec<_>>().join("\n"))
}

/// The head moved during the round: if the PR's own changes are the same
/// (the base was merged in), the checker checks again at the new head;
/// otherwise the reviewers review the new commits in the same round.
pub async fn follow_head(shared: &Shared, key: &str, new: &str) -> Result<&'static str> {
    let cfg = super::config(shared)?;
    let e = super::get(&shared.lock(), key)?.ok_or_else(|| anyhow!("{key} isn't tracked"))?;
    let old = e.round_head.clone().unwrap_or_default();
    let checkout = cfg.repos.get(&e.repo).map(|r| r.checkout.clone()).ok_or_else(|| anyhow!("{} isn't in the review config", e.repo))?;
    let base = e.base.clone().unwrap_or_else(|| "main".into());
    let _ = fetch(&checkout, &e.repo, &[format!("+refs/heads/{base}:refs/remotes/origin/{base}"), new.to_string()]).await;
    let same = match (pr_diff(&checkout, &base, &old).await, pr_diff(&checkout, &base, new).await) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    };
    prep(shared, key).await?;
    let mut e = super::get(&shared.lock(), key)?.ok_or_else(|| anyhow!("{key} isn't tracked"))?;
    let head = e.round_head.clone().unwrap_or_default();
    if same {
        {
            let mut core = shared.lock();
            super::event(&mut core, &e, "repointed", json!({ "round": e.round, "head": head, "from": old }), &Who::Hub)?;
        }
        start_checker(shared, key).await?;
        return Ok("repointed");
    }
    if let (Some(b), Some(s)) = (pane_block(shared, &e, e.checker_block.as_deref(), e.checker_pane.as_deref()), e.session_id.clone()) {
        panes::close(&s, &b).await;
    }
    e.checker_pane = None;
    e.checker_block = None;
    let (docket, mode) = {
        let core = shared.lock();
        (super::docket_of(&core, key).unwrap_or_default(), super::post_mode(&core, &cfg, Some(&e.repo)))
    };
    let login = github::login().await?;
    let round = e.round;
    let h7 = short(&head);
    let prompt = DELTA_PROMPT.replace("{n}", &round.to_string()).replace("{h}", &h7);
    let rs = e.round_reviewer_list();
    let mut anchor: Option<String> = None;
    for mut r in rs.clone() {
        let wt = PathBuf::from(r.worktree.clone().unwrap_or_default());
        let mut v = vars(&cfg, &e, round, Some(&r), &rs, &login, mode)?;
        v.insert("MOVED_FROM", old.clone());
        v.insert("MOVED_FROM_SHORT", short(&old));
        super::write_atomic(&wt.join(format!(".tmp/rounds/{round}/delta-{h7}.md")), &render(&brief("delta.md"), &v))?;
        set_status(&wt, &format!("WORKING round {round} delta"))?;
        match pane_block(shared, &e, r.block.as_deref(), r.pane.as_deref()) {
            Some(b) => {
                let s = e.session_id.clone().unwrap_or_default();
                panes::deliver(shared, &s, &b, &prompt).await?;
                anchor.get_or_insert(b);
            }
            None => {
                let place = match &anchor {
                    None => Place::Tab,
                    Some(a) => Place::Beside { anchor: a, dir: "right" },
                };
                let resume = r.session.is_some();
                let (_, b) = open_reviewer(shared, &cfg, &e, &docket, &mut r, resume, place, &prompt, round).await?;
                anchor.get_or_insert(b);
                e.reviewers.insert(r.name.clone(), r);
            }
        }
    }
    e.reviewers_done = Some(0);
    e.phase = Some("reviewer".into());
    let mut core = shared.lock();
    super::save(&mut core, &mut e)?;
    super::event(&mut core, &e, "delta", json!({ "round": round, "head": head, "from": old }), &Who::Hub)?;
    Ok("delta")
}

// ---- Post, dismiss ----

/// Who asks to post.
#[derive(Debug, Clone, PartialEq)]
pub enum Asker {
    /// You: the app, or `wh` outside an agent's pane.
    Owner,
    /// This PR's checker, from its pane.
    Checker,
    /// Any other agent.
    Agent(String),
    /// The hub, when your posting rule passed for the checked review.
    Hub,
}

fn worktree_tokens(e: &Pr) -> (Vec<(String, Value)>, Value) {
    let mut by = Vec::new();
    let mut sum = json!({ "total": 0 });
    for w in e.worktrees() {
        let t = agentcli::tokens(Path::new(&w));
        sum = agentcli::add_tokens(&sum, &t);
        by.push((w, t));
    }
    (by, sum)
}

/// Each reviewer's part in the round: findings drafted, kept, found only
/// by it, effort, tokens.
fn scoreboard(e: &mut Pr, verified: Option<&Value>, by_wt: &[(String, Value)]) -> Value {
    let round = e.round;
    let mut out = serde_json::Map::new();
    for name in e.round_reviewers.clone() {
        let Some(r) = e.reviewers.get_mut(&name) else { continue };
        let wt = PathBuf::from(r.worktree.as_deref().unwrap_or_default());
        let mut s = serde_json::Map::new();
        if let Some(d) = read_json(&wt.join(format!(".tmp/rounds/{round}/review.json"))) {
            s.insert("drafted".into(), json!(d["findings"].as_array().map_or(0, Vec::len)));
        }
        let effort = agentcli::session_effort(&r.agent, &wt, r.session.as_deref()).or_else(|| r.effort.clone());
        s.insert("effort".into(), json!(effort));
        if let Some(v) = verified {
            let fs = v["findings"].as_array().cloned().unwrap_or_default();
            let has = |f: &Value| f["found_by"].as_array().is_some_and(|a| a.iter().any(|x| x == name.as_str()));
            s.insert("kept".into(), json!(fs.iter().filter(|f| has(f)).count()));
            s.insert("only".into(), json!(fs.iter().filter(|f| f["found_by"].as_array().is_some_and(|a| a.len() == 1 && a[0] == name.as_str())).count()));
        }
        let agent_key = r.agent.clone();
        if let Some((_, t)) = by_wt.iter().find(|(w, _)| Some(w.as_str()) == r.worktree.as_deref()) {
            let total = t[&agent_key]["total"].as_i64().unwrap_or(0);
            let spent = if r.first_round == Some(round) { total } else { total - r.tokens_seen.unwrap_or(0) };
            s.insert("tokens".into(), json!(spent.max(0)));
            r.tokens_seen = Some(total);
        }
        out.insert(name, Value::Object(s));
    }
    Value::Object(out)
}

fn draft_path(e: &Pr, file: &str) -> Option<PathBuf> {
    e.worktree.as_deref().map(|w| Path::new(w).join(format!(".tmp/rounds/{}/{file}", e.round)))
}

/// Preview (`dry`) or post the round's checked review.
/// A short hash of a checked review's text: what you saw, so what posts is
/// what you approved.
pub fn draft_digest(text: &str) -> String {
    use sha2::Digest;
    crate::record::hex::encode(&sha2::Sha256::digest(text.as_bytes()))[..16].to_string()
}

/// Post (or with `dry`, preview) the round's checked review. With `seen`,
/// the digest of the draft you looked at: if the file changed since, it
/// doesn't post.
pub async fn post(shared: &Shared, key: &str, dry: bool, asker: Asker, seen: Option<&str>) -> Result<Value> {
    post_as(shared, key, dry, asker, seen, true).await
}

/// The round's review, as `post` would put it on GitHub: `round=R head=SHA`.
fn post_marker(e: &Pr) -> String {
    format!("round={} head={}", e.round, e.round_head.as_deref().unwrap_or(""))
}

/// `post`; with `approve` false, your approval is already on the record
/// (the review is on GitHub, and only its record is missing).
async fn post_as(shared: &Shared, key: &str, dry: bool, asker: Asker, seen: Option<&str>, approve: bool) -> Result<Value> {
    let cfg = super::config(shared)?;
    let (mut e, mode) = {
        let core = shared.lock();
        let e = super::get(&core, key)?.ok_or_else(|| anyhow!("{key} isn't tracked"))?;
        let mode = super::post_mode(&core, &cfg, Some(&e.repo));
        (e, mode)
    };
    if e.state != "reviewing" {
        bail!("{} has no round in progress", e.short());
    }
    if !dry {
        match &asker {
            Asker::Agent(_) => bail!("only {}'s checker, or you, can post its review", e.short()),
            Asker::Checker if mode == "rule" => bail!("posting in {} is a rule the hub checks once you've previewed the review; preview it and stop", e.repo),
            Asker::Checker if mode != "auto" => {
                bail!("post = \"ask\": {} posts this review, not the checker; preview it and stop", github::login().await.unwrap_or_else(|_| "the user".into()))
            }
            _ => {}
        }
        if mode != "auto" && e.phase() != "awaiting" {
            bail!("{} round {} isn't waiting for you (posting is {mode} and its phase is {}); nothing was posted", e.short(), e.round, e.phase());
        }
    }
    let path = draft_path(&e, "review.verified.json").ok_or_else(|| anyhow!("{key} has no worktree"))?;
    super::no_link(&path)?;
    let text = std::fs::read_to_string(&path).map_err(|_| anyhow!("write {} first: the checked review, in the format of {}", path.display(), super::draft_schema().display()))?;
    let draft: Value = serde_json::from_str(&text).map_err(|err| anyhow!("{} isn't valid JSON: {err}", path.display()))?;
    let digest = draft_digest(&text);
    // A preview is what a post without its own digest is held to.
    // Kept in the store, so a post after the hub restarts still knows it.
    let previewed = {
        let mut core = shared.lock();
        let k = format!("review.previewed.{key}");
        if dry {
            core.store.meta_set(&k, &digest)?;
        }
        core.store.meta_get(&k)?
    };
    let seen = match seen.filter(|s| !s.is_empty()) {
        Some(s) => Some(s.to_string()),
        // Only recording what's on GitHub already: nothing posts.
        None if dry || !approve => None,
        None => Some(previewed.ok_or_else(|| anyhow!("preview the checked review first (wh review preview), then post it; nothing was posted"))?),
    };
    if !dry && let Some(s) = seen.as_deref().filter(|s| **s != digest) {
        bail!("the checked review changed since you looked at it ({s} then, {digest} now); look again before posting");
    }
    let primary = PathBuf::from(e.worktree.clone().unwrap_or_default());
    let names: Vec<String> =
        e.round_reviewer_list().iter().map(|r| agentcli::display_name(&r.agent, Path::new(r.worktree.as_deref().unwrap_or_default()), r.session.as_deref())).collect();
    let by = agentcli::byline(&names);
    let checked_by = e.checker_agent.as_deref().map(|a| agentcli::display_name(a, &primary, e.checker_session.as_deref()));
    let head = e.round_head.clone();
    let marker = post_marker(&e);
    let require_risk = cfg.merge_risk_enabled(&e.repo, &serde_json::to_value(&e)?);
    if !dry && approve && asker == Asker::Owner {
        let mut core = shared.lock();
        if let Some(d) = super::docket_of(&core, key) {
            core.append(&d, "approval", &Who::Owner, None, json!({ "what": "post-review", "round": e.round, "head": head, "draft": digest }))?;
        }
    }
    let args = super::post::Args { repo: &e.repo, pr: e.pr, head: head.as_deref(), by: &by, checked_by: checked_by.as_deref(), marker: Some(&marker), require_risk, dry };
    let res = match super::post::run(&args, &draft).await {
        Ok(res) => res,
        Err(super::post::Refusal::HeadMoved { live, .. }) => {
            saw_head(shared, key, &live)?;
            bail!(
                "{} moved to {} after this review was checked at {}, so nothing was posted. Wheelhouse is following it now: the reviewers look at the new commits (or the checker checks again, if only the base was merged in), then it's back for you.",
                e.short(),
                short(&live),
                short(head.as_deref().unwrap_or(""))
            );
        }
        Err(other) => bail!("{other}"),
    };
    if dry {
        let p = primary.join(format!(".tmp/rounds/{}/preview.md", e.round));
        super::write_atomic(&p, res["preview"].as_str().unwrap_or(""))?;
        return Ok(json!({ "dry_run": true, "preview": res["preview"], "preview_path": p, "warning": res["warning"], "digest": digest }));
    }
    let blocking = super::post::blocking_count(&draft) as i64;
    let total = draft["findings"].as_array().map_or(0, Vec::len) as i64;
    let (by_wt, tokens) = worktree_tokens(&e);
    let reviewers = scoreboard(&mut e, Some(&draft), &by_wt);
    let checker_effort = e.checker_agent.as_deref().and_then(|a| agentcli::session_effort(a, &primary, e.checker_session.as_deref())).or(e.checker_effort.clone());
    let implementation = draft["implementation"].as_str().map(one_line);
    let verdict = res["state"].as_str().unwrap_or("COMMENTED").to_string();
    let risk = res["merge_risk"].as_str().map(str::to_string);
    rules::mark_posted(
        &mut e,
        rules::Ended {
            head: head.clone(),
            verdict: verdict.clone(),
            review_id: Some(res["review_id"].clone()),
            url: res["url"].as_str().map(str::to_string),
            by: None,
            blocking: Some(blocking),
            non_blocking: Some(total - blocking),
            merge_risk: risk.clone(),
            implementation,
            tokens: Some(tokens),
            reviewers: Some(reviewers),
            checker_effort,
        },
        &now(),
    );
    e.merge_risk = risk.clone();
    let _ = set_status(&primary, &format!("POSTED {}", res["url"].as_str().unwrap_or("")));
    let mut core = shared.lock();
    super::save(&mut core, &mut e)?;
    let note = format!("{blocking} blocking, {} non-blocking{}", total - blocking, risk.as_deref().map(|r| format!(", merge risk {r}")).unwrap_or_default());
    super::event(
        &mut core,
        &e,
        "posted",
        json!({ "round": e.round, "head": head, "verdict": verdict, "review_id": res["review_id"], "note": note, "by": match asker { Asker::Owner => "you", Asker::Hub => "rule", _ => "checker" } }),
        &Who::Hub,
    )?;
    if let (Some(d), Some(url)) = (super::docket_of(&core, key), res["url"].as_str()) {
        core.append(&d, "link", &Who::Hub, None, json!({ "kind": "github-review", "url": url, "round": e.round, "review_id": res["review_id"] }))?;
    }
    keep_round(&mut core, &e, res["preview"].as_str());
    core.review.attention.remove(key);
    core.review.check_now.notify_one();
    Ok(res)
}

// ---- Posting by your rule ----

/// The facts a posting rule is checked against: the PR, as an effort
/// function sees it, and its checked review.
pub fn posting_facts(e: &Pr, draft: &Value) -> Value {
    let mut v = effort_view(e, e.round, None);
    let blocking = super::post::blocking_count(draft);
    let total = draft["findings"].as_array().map_or(0, Vec::len);
    v["verdict"] = json!(match draft["event"].as_str() {
        Some("APPROVE") => "approve",
        Some("REQUEST_CHANGES") => "request_changes",
        _ => "none",
    });
    v["blocking"] = json!(blocking);
    v["non_blocking"] = json!(total - blocking);
    v["findings"] = json!(total);
    if draft["merge_risk"].is_object()
        && let Ok(r) = super::post::risk_level(&draft["merge_risk"])
    {
        v["merge_risk"] = json!(r);
    }
    v
}

/// What the hub did with a checked review.
#[derive(Debug, PartialEq)]
pub enum Decided {
    /// Posting wasn't a rule when the round's checker started: you decide.
    NotRule,
    Posted,
    /// It waits for you, and why.
    Waits(String),
}

/// Under a posting rule, the hub decides once for each checked review:
/// post it (through the same refusals, pinned to the digest it checked),
/// or leave it waiting for you with the reason. Both go on the record.
pub async fn decide(shared: &Shared, key: &str) -> Result<Decided> {
    let cfg = super::config(shared)?;
    let (mut e, rule) = {
        let core = shared.lock();
        let e = super::get(&core, key)?.ok_or_else(|| anyhow!("{key} isn't tracked"))?;
        let rule = super::post_for(&core, &cfg, Some(&e.repo));
        (e, rule)
    };
    if e.checker_post.as_deref() != Some("rule") || e.state != "reviewing" || e.phase() != "awaiting" {
        return Ok(Decided::NotRule);
    }
    let Some(path) = draft_path(&e, "review.verified.json") else { return Ok(Decided::NotRule) };
    if super::through_link(&path).is_some() {
        return Ok(Decided::Waits("the checked review is a link out of the worktree; Wheelhouse won't follow it".into()));
    }
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Ok(Decided::Waits("there's no checked review for your rule to look at".into()));
    };
    let digest = draft_digest(&text);
    let (round, head) = (e.round, e.round_head.clone());
    let decided = e.post_decision.clone().filter(|d| d["round"] == json!(round) && d["head"] == json!(head) && d["digest"] == json!(digest));
    let decision = match decided {
        Some(d) => d,
        None => {
            let d = match serde_json::from_str::<Value>(&text) {
                Ok(draft) => rule.decide(&posting_facts(&e, &draft), &cfg.path),
                Err(err) => super::posting::Decision { auto: false, why: format!("the checked review isn't valid JSON ({err})") },
            };
            let v = json!({ "round": round, "head": head, "digest": digest, "auto": d.auto, "why": d.why });
            e.post_decision = Some(v.clone());
            let mut core = shared.lock();
            super::save(&mut core, &mut e)?;
            super::event(&mut core, &e, "decided", json!({ "round": round, "head": head, "auto": d.auto, "why": d.why, "rule": rule.json() }), &Who::Hub)?;
            v
        }
    };
    if decision["auto"] != json!(true) {
        return Ok(Decided::Waits(decision["why"].as_str().unwrap_or_default().to_string()));
    }
    if let Some(r) = decision["refused"].as_str() {
        return Ok(Decided::Waits(format!("your rule would post it, but the post was refused: {r}")));
    }
    match post(shared, key, false, Asker::Hub, Some(&digest)).await {
        Ok(_) => Ok(Decided::Posted),
        Err(err) => {
            let err = format!("{err:#}");
            let mut core = shared.lock();
            if let Some(mut e) = super::get(&core, key)? {
                if let Some(d) = e.post_decision.as_mut() {
                    d["refused"] = json!(err);
                }
                super::save(&mut core, &mut e)?;
                super::event(&mut core, &e, "post-refused", json!({ "round": round, "head": head, "error": err }), &Who::Hub)?;
            }
            Ok(Decided::Waits(format!("your rule would post it, but the post was refused: {err}")))
        }
    }
}

/// Keep the round's work on its docket before cleanup removes the
/// worktrees: each reviewer's report and draft, the checked review, the
/// checker's notes, the merge risk, and the review as it was posted. A
/// file that isn't there is skipped; a failure is logged, never the post's.
fn keep_round(core: &mut crate::hub::Core, e: &Pr, posted: Option<&str>) {
    let Some(docket) = super::docket_of(core, &e.key()) else { return };
    let Some(primary) = e.worktree.as_deref().map(PathBuf::from) else { return };
    let round = e.round;
    let dir = primary.join(format!(".tmp/rounds/{round}"));
    let mut files: Vec<(PathBuf, String, String, &str, Option<String>)> = Vec::new();
    for (i, r) in e.round_reviewer_list().iter().enumerate() {
        let copied = dir.join("reviewers").join(&r.name);
        let own = r.worktree.as_deref().map(|w| PathBuf::from(w).join(format!(".tmp/rounds/{round}")));
        let base = if i == 0 {
            dir.clone()
        } else if copied.exists() {
            copied
        } else {
            own.unwrap_or(copied)
        };
        files.push((base.join("report.md"), format!("{}'s report", r.name), format!("{}-report.md", r.name), "report", Some(r.name.clone())));
        files.push((base.join("review.json"), format!("{}'s draft review", r.name), format!("{}-review.json", r.name), "draft", Some(r.name.clone())));
    }
    files.push((dir.join("review.verified.json"), "The checked review".into(), "review.verified.json".into(), "checked", None));
    files.push((dir.join("checker.md"), "The checker's notes".into(), "checker.md".into(), "checker", None));
    files.push((dir.join("merge-risk.json"), "Merge risk".into(), "merge-risk.json".into(), "merge-risk", None));
    let group = Some(format!("Round {round}"));
    let mut keep = |title: String, name: String, bytes: Vec<u8>, role: &str, reviewer: Option<String>| {
        let a = crate::artifacts::New {
            title,
            name,
            bytes: Some(bytes),
            url: None,
            source: "review".into(),
            group: group.clone(),
            extra: json!({ "round": round, "role": role, "reviewer": reviewer }),
        };
        if let Err(err) = crate::artifacts::attach(core, &docket, &Who::Hub, a) {
            say!("review: {}: keeping round {round}'s files: {err:#}", e.short());
        }
    };
    if let Some(text) = posted.filter(|t| !t.trim().is_empty()) {
        keep("The review as posted".into(), format!("round-{round}-posted.md"), text.as_bytes().to_vec(), "posted", None);
    }
    for (path, title, name, role, reviewer) in files {
        if super::through_link(&path).is_none()
            && let Ok(bytes) = std::fs::read(&path)
            && !bytes.is_empty()
        {
            keep(title, name, bytes, role, reviewer);
        }
    }
}

/// Drop the round's review: nothing goes to GitHub.
pub async fn dismiss(shared: &Shared, key: &str) -> Result<Value> {
    let mut e = super::get(&shared.lock(), key)?.ok_or_else(|| anyhow!("{key} isn't tracked"))?;
    if e.state != "reviewing" {
        bail!("{} has no round in progress", e.short());
    }
    // Posted, but the record never heard (a post cut off before it was
    // recorded, a crash): it's on GitHub, so it's recorded as posted, not
    // as dismissed with nothing posted.
    if matches!(e.phase(), "checking" | "awaiting")
        && let Some(v) = super::post::find_posted(&e.repo, e.pr, &post_marker(&e)).await
    {
        say!("review: {key}: round {} is on GitHub already ({}); recording it as posted", e.round, v["html_url"].as_str().unwrap_or(""));
        let url = v["html_url"].as_str().unwrap_or_default().to_string();
        return post_as(shared, key, false, Asker::Owner, None, false)
            .await
            .with_context(|| format!("round {} is on GitHub already ({url}), so it isn't dismissed; recording it failed", e.round));
    }
    let d = draft_path(&e, "review.verified.json").and_then(|p| read_json(&p)).or_else(|| draft_path(&e, "review.json").and_then(|p| read_json(&p)));
    let checking = matches!(e.phase(), "checking" | "awaiting");
    let (by_wt, tokens) = worktree_tokens(&e);
    let verified = draft_path(&e, "review.verified.json").and_then(|p| read_json(&p));
    let reviewers = scoreboard(&mut e, verified.as_ref(), &by_wt);
    let primary = PathBuf::from(e.worktree.clone().unwrap_or_default());
    let checker_effort =
        if checking { e.checker_agent.as_deref().and_then(|a| agentcli::session_effort(a, &primary, e.checker_session.as_deref())).or(e.checker_effort.clone()) } else { None };
    let head = e.round_head.clone();
    rules::mark_posted(
        &mut e,
        rules::Ended {
            head: head.clone(),
            verdict: "DISMISSED".into(),
            by: Some("you".into()),
            implementation: d.as_ref().and_then(|d| d["implementation"].as_str()).map(one_line),
            tokens: Some(tokens),
            reviewers: Some(reviewers),
            checker_effort,
            ..Default::default()
        },
        &now(),
    );
    let _ = set_status(&primary, "DISMISSED");
    let mut core = shared.lock();
    super::save(&mut core, &mut e)?;
    super::event(&mut core, &e, "dismissed", json!({ "round": e.round, "head": head }), &Who::Owner)?;
    keep_round(&mut core, &e, None);
    core.review.attention.remove(key);
    core.review.check_now.notify_one();
    Ok(json!({ "dismissed": true, "round": e.round }))
}

/// Close a PR's panes (its round is over). Worktrees and sessions stay for
/// later rounds.
pub async fn close_panes(shared: &Shared, key: &str) -> Result<()> {
    let mut e = super::get(&shared.lock(), key)?.ok_or_else(|| anyhow!("{key} isn't tracked"))?;
    let session = e.session_id.clone().or_else(|| panes::session(shared));
    let mut blocks = Vec::new();
    if let Some(b) = pane_block(shared, &e, e.checker_block.as_deref(), e.checker_pane.as_deref()) {
        blocks.push(b);
    }
    for r in e.reviewers.values() {
        if let Some(b) = pane_block(shared, &e, r.block.as_deref(), r.pane.as_deref())
            && !blocks.contains(&b)
        {
            blocks.push(b);
        }
    }
    if let Some(s) = &session {
        for b in &blocks {
            panes::close(s, b).await;
        }
    }
    e.pane = None;
    e.checker_pane = None;
    e.checker_block = None;
    e.phase = None;
    for r in e.reviewers.values_mut() {
        r.block = None;
    }
    let mut core = shared.lock();
    super::save(&mut core, &mut e)?;
    Ok(())
}

/// Merged or closed: count the tokens one last time, then remove the
/// panes, worktrees and refs.
pub async fn cleanup(shared: &Shared, key: &str) -> Result<()> {
    let cfg = super::config(shared)?;
    {
        let mut e = super::get(&shared.lock(), key)?.ok_or_else(|| anyhow!("{key} isn't tracked"))?;
        let (_, tokens) = worktree_tokens(&e);
        if tokens["total"].as_i64().unwrap_or(0) > 0 {
            e.tokens = Some(tokens);
        }
        super::save(&mut shared.lock(), &mut e)?;
    }
    close_panes(shared, key).await?;
    let mut e = super::get(&shared.lock(), key)?.ok_or_else(|| anyhow!("{key} isn't tracked"))?;
    if let Some(checkout) = cfg.repos.get(&e.repo).map(|r| r.checkout.clone()) {
        let list = git(&checkout, &["worktree", "list", "--porcelain"]).await.unwrap_or_default();
        let all: Vec<String> = list.lines().filter_map(|l| l.strip_prefix("worktree ")).map(str::to_string).collect();
        let uid = std::fs::metadata(crate::paths::home()).map(|m| m.uid()).unwrap_or(501);
        for wt in e.worktrees() {
            if !Path::new(&wt).join(".git").exists() {
                continue;
            }
            for nested in all.iter().filter(|w| w.starts_with(&format!("{wt}/"))) {
                let _ = git(&checkout, &["worktree", "remove", "--force", nested]).await;
            }
            if let Err(err) = git(&checkout, &["worktree", "remove", "--force", &wt]).await {
                say!("review: {key}: {err:#}");
            }
            let scratch: String = wt.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
            let _ = std::fs::remove_dir_all(format!("/private/tmp/claude-{uid}/{scratch}"));
        }
        let _ = git(&checkout, &["worktree", "prune"]).await;
        for ns in ["wheelhouse", "rex-reviewer"] {
            let refs = git(&checkout, &["for-each-ref", "--format=%(refname)", &format!("refs/{ns}/pr-{}/", e.pr)]).await.unwrap_or_default();
            for r in refs.lines().filter(|l| !l.is_empty()) {
                let _ = git(&checkout, &["update-ref", "-d", r]).await;
            }
        }
    }
    let threads: Vec<String> = e.reviewers.values().filter(|r| r.agent == "codex").filter_map(|r| r.session.clone()).chain(e.checker_threads.iter().cloned()).collect();
    for t in threads {
        let _ = tokio::process::Command::new(agentcli::program(&cfg, "codex")).args(["archive", &t]).env("PATH", github::command_path()).output().await;
    }
    e.state = "cleaned".into();
    e.pane = None;
    e.checker_pane = None;
    e.phase = None;
    e.cleaned_at = Some(now());
    let mut core = shared.lock();
    super::save(&mut core, &mut e)?;
    super::event(&mut core, &e, "cleaned", json!({}), &Who::Hub)?;
    core.review.attention.remove(key);
    Ok(())
}

/// Review a PR that didn't ask you: you picked it up.
pub async fn pickup(shared: &Shared, repo: &str, n: i64) -> Result<Value> {
    let cfg = super::config(shared)?;
    if !cfg.repos.contains_key(repo) {
        bail!("{repo} isn't in the review config ({})", cfg.repo_list().join(", "));
    }
    let key = format!("{repo}#{n}");
    if let Some(e) = super::get(&shared.lock(), &key)?
        && matches!(e.state.as_str(), "queued" | "reviewing")
    {
        return Ok(json!({ "key": key, "state": e.state, "note": format!("already {}", e.state) }));
    }
    let v = github::pr_view(repo, n, "number,title,url,state,headRefOid,baseRefName,author").await?;
    let state = v["state"].as_str().unwrap_or("");
    if state != "OPEN" {
        bail!("{repo}#{n} is {state}");
    }
    let mut core = shared.lock();
    let mut e = super::get(&core, &key)?.unwrap_or_else(|| {
        let mut p = Pr::new(repo, n);
        p.created_at = Some(now());
        p
    });
    e.state = "queued".into();
    e.trigger = Some("manual".into());
    e.requested_at = Some(now());
    e.head = v["headRefOid"].as_str().map(str::to_string);
    e.title = v["title"].as_str().map(str::to_string);
    e.url = v["url"].as_str().map(str::to_string);
    e.author = v["author"]["login"].as_str().map(str::to_string);
    e.outcome = None;
    e.ended_at = None;
    super::save(&mut core, &mut e)?;
    super::event(&mut core, &e, "pickup", json!({ "round": e.round + 1, "head": e.head }), &Who::Owner)?;
    core.review.check_now.notify_one();
    Ok(json!({ "key": key, "state": "queued" }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_rule_decides_once_per_checked_review_and_says_why() {
        let wt = std::env::temp_dir().join(format!("wh-decide-{}", std::process::id()));
        let out = wt.join(".tmp/rounds/1");
        std::fs::create_dir_all(&out).unwrap();
        std::fs::create_dir_all(wt.join(".tmp/context/round-1")).unwrap();
        std::fs::write(wt.join(".tmp/context/round-1/pr.json"), r#"{"files":[{"path":"src/api.rs"}],"labels":[{"name":"safe"}]}"#).unwrap();
        let shared = Shared::new(crate::hub::test_core());
        // The hub's cached config, so nothing reads your own review.lua.
        let cfg = super::super::config::from_value(
            super::super::config::path(),
            json!({ "repos": { "acme/api": { "checkout": wt, "post": { "auto": { "verdict": "approve", "max_lines": 100, "labels": "safe" } } } } }),
        )
        .unwrap();
        let mtime = std::fs::metadata(super::super::config::path()).and_then(|m| m.modified()).ok().max(crate::settings::mtime());
        shared.lock().review.config = Some((std::sync::Arc::new(cfg), mtime));
        let key = "acme/api#42";
        let mut e = Pr {
            repo: "acme/api".into(),
            pr: 42,
            state: "reviewing".into(),
            phase: Some("awaiting".into()),
            round: 1,
            round_head: Some("a1b2c3d4".into()),
            worktree: Some(wt.display().to_string()),
            author: Some("dana".into()),
            additions: Some(30),
            deletions: Some(4),
            checker_post: Some("rule".into()),
            ..Default::default()
        };
        let d = super::super::save(&mut shared.lock(), &mut e).unwrap();
        let count = |s: &Shared| s.lock().store.state(&d).unwrap().unwrap().entries;

        // It requests changes: it waits for you, and the record says why, once.
        let blocking = r#"{"event":"REQUEST_CHANGES","verdict":"Retries can loop.","findings":[{"severity":"blocking","bullet":"Retries can loop forever."}]}"#;
        std::fs::write(out.join("review.verified.json"), blocking).unwrap();
        assert_eq!(decide(&shared, key).await.unwrap(), Decided::Waits("it requests changes (auto needs an approval)".into()));
        let n = count(&shared);
        assert_eq!(decide(&shared, key).await.unwrap(), Decided::Waits("it requests changes (auto needs an approval)".into()));
        assert_eq!(count(&shared), n, "decided once");
        let e1 = super::super::get(&shared.lock(), key).unwrap().unwrap();
        assert_eq!(e1.post_decision.as_ref().unwrap()["auto"], json!(false));

        // A new checked review that the rule passes: the hub posts it, and
        // a post the refusals stop waits for you with the reason.
        std::fs::write(out.join("review.verified.json"), r#"{"event":"APPROVE","verdict":"Looks right.","findings":[]}"#).unwrap();
        let Decided::Waits(why) = decide(&shared, key).await.unwrap() else { panic!("the post is refused: no merge risk") };
        assert!(why.starts_with("your rule would post it, but the post was refused: the draft has no merge_risk"), "{why}");
        let e2 = super::super::get(&shared.lock(), key).unwrap().unwrap();
        assert_eq!(e2.post_decision.as_ref().unwrap()["auto"], json!(true));
        assert_eq!(e2.post_decision.as_ref().unwrap()["why"], "it approves, it changes 34 lines, and it's labeled `safe`");
        assert!(e2.post_decision.as_ref().unwrap()["refused"].is_string());
        let n = count(&shared);
        assert!(matches!(decide(&shared, key).await.unwrap(), Decided::Waits(_)));
        assert_eq!(count(&shared), n, "a refused post isn't tried again");
        let bodies: Vec<String> = shared.lock().store.entries(&d).unwrap().iter().filter_map(|s| s.entry().unwrap().body).collect();
        assert!(bodies.iter().any(|b| b == "Round 1's checked review waits for you: it requests changes (auto needs an approval)."), "{bodies:?}");
        assert!(bodies.iter().any(|b| b == "Round 1's checked review posts itself, by your rule: it approves, it changes 34 lines, and it's labeled `safe`."), "{bodies:?}");
        assert!(bodies.iter().any(|b| b.starts_with("Your rule would post round 1, but the post was refused: the draft has no merge_risk")), "{bodies:?}");

        // A round whose checker started under ask: yours to decide.
        let mut e3 = super::super::get(&shared.lock(), key).unwrap().unwrap();
        e3.checker_post = Some("ask".into());
        super::super::save(&mut shared.lock(), &mut e3).unwrap();
        assert_eq!(decide(&shared, key).await.unwrap(), Decided::NotRule);
        assert!(shared.lock().verify(&d).unwrap().ok);
        let _ = std::fs::remove_dir_all(&wt);
    }

    #[tokio::test]
    async fn a_round_follows_a_head_the_scan_hasnt_seen_yet() {
        let wt = std::env::temp_dir().join(format!("wh-moved-{}", std::process::id()));
        std::fs::create_dir_all(wt.join(".tmp/rounds/2")).unwrap();
        let risk = r#""merge_risk":{"alignment":{"verdict":"good","gaps":[]},"impact":{"verdict":"small","gaps":[]},"verification":{"verdict":"good","gaps":[]},"rollback_cost":{"verdict":"low","gaps":[]},"error_detection":{"verdict":"good","gaps":[]}}"#;
        std::fs::write(wt.join(".tmp/rounds/2/review.verified.json"), format!(r#"{{"event":"APPROVE","verdict":"Looks right.","findings":[],{risk}}}"#)).unwrap();
        // A stand-in gh: the PR's head on "GitHub" is whatever `head` says,
        // and its reviews are whatever `reviews` holds.
        let gh = wt.join("gh");
        let head_file = wt.join("head");
        let reviews = wt.join("reviews");
        let stub = r#"#!/bin/sh
d=$(dirname "$0")
h=$(cat "$d/head")
case "$*" in
  'api user'*) echo you ;;
  *'/reviews --jq'*) cat "$d/reviews" 2>/dev/null ;;
  *'--jq .headRefOid') echo "$h" ;;
  'pr view'*) printf '{"headRefOid":"%s","state":"OPEN"}' "$h" ;;
  'pr diff'*) ;;
  *) exit 1 ;;
esac
"#;
        std::fs::write(&gh, stub).unwrap();
        std::fs::set_permissions(&gh, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        // SAFETY: only this test's helpers run gh.
        unsafe { std::env::set_var("WH_GH", &gh) };
        let shared = Shared::new(crate::hub::test_core());
        let cfg = super::super::config::from_value(super::super::config::path(), json!({ "repos": { "acme/web": { "checkout": wt, "post": "ask" } } })).unwrap();
        let mtime = std::fs::metadata(super::super::config::path()).and_then(|m| m.modified()).ok().max(crate::settings::mtime());
        shared.lock().review.config = Some((std::sync::Arc::new(cfg), mtime));
        let key = "acme/web#790";
        let mut e = Pr {
            repo: "acme/web".into(),
            pr: 790,
            state: "reviewing".into(),
            phase: Some("awaiting".into()),
            round: 2,
            round_head: Some("532aff7".into()),
            head: Some("532aff7".into()),
            worktree: Some(wt.display().to_string()),
            ..Default::default()
        };
        let d = super::super::save(&mut shared.lock(), &mut e).unwrap();
        let now = |s: &Shared| super::super::get(&s.lock(), key).unwrap().unwrap();

        // The author pushed after the last scan. Trusting the scan, the
        // round still covers its head; read live, it doesn't.
        std::fs::write(&head_file, "fa3f931").unwrap();
        assert_eq!(moved_head(&shared, &e, false).await.unwrap(), None, "the scan's head is only read again when it disagrees");
        assert_eq!(moved_head(&shared, &e, true).await.unwrap(), Some("fa3f931".into()));
        assert_eq!(now(&shared).head.as_deref(), Some("fa3f931"), "kept for the watcher");

        // Your approval of the stale review: nothing posts, the hub knows
        // the new head, and the message says what happens next.
        let mut e = now(&shared);
        e.head = Some("532aff7".into());
        super::super::save(&mut shared.lock(), &mut e).unwrap();
        // A post that names no digest is held to the last preview: none yet.
        let err = format!("{:#}", post(&shared, key, false, Asker::Owner, None).await.unwrap_err());
        assert!(err.contains("preview the checked review first"), "{err}");
        let _ = post(&shared, key, true, Asker::Owner, None).await;
        let err = format!("{:#}", post(&shared, key, false, Asker::Owner, None).await.unwrap_err());
        assert!(err.contains("moved to fa3f931 after this review was checked at 532aff7") && err.contains("following it now"), "{err}");
        assert_eq!(now(&shared).head.as_deref(), Some("fa3f931"));
        let kinds: Vec<String> = shared.lock().store.entries(&d).unwrap().iter().filter_map(|s| s.entry().unwrap().body).collect();
        assert!(kinds.iter().any(|b| b == "The head moved to fa3f931 mid-round."), "{kinds:?}");

        // A scan from before the round started is behind it, not ahead:
        // no delta, and the stale head is corrected.
        let mut e = now(&shared);
        e.round_head = Some("fa3f931".into());
        e.head = Some("ee9a359".into());
        super::super::save(&mut shared.lock(), &mut e).unwrap();
        assert_eq!(moved_head(&shared, &e, false).await.unwrap(), None);
        assert_eq!(now(&shared).head.as_deref(), Some("fa3f931"));

        // Your post reached GitHub, but the request was cut off before the
        // record heard. Dismissing it records the post, not "nothing was
        // posted", and adds no second approval.
        std::fs::write(&head_file, "fa3f931").unwrap();
        std::fs::write(&reviews, r#"{"id":5996470580,"state":"CHANGES_REQUESTED","html_url":"https://github.com/acme/web/pull/790#pullrequestreview-5996470580","body":"Two blocking.\n<!-- pr-review round=2 head=fa3f931 -->","user":"you"}"#).unwrap();
        let approvals = |s: &Shared| s.lock().store.entries(&d).unwrap().iter().filter(|x| x.entry().unwrap().kind == "approval").count();
        let before = approvals(&shared);
        let res = dismiss(&shared, key).await.unwrap();
        assert_eq!(res["review_id"], json!(5996470580_i64), "{res}");
        assert_eq!(res["already"], json!(true));
        let e = now(&shared);
        assert_eq!(e.state, "posted");
        assert_eq!(e.history.last().map(|h| h["verdict"].clone()), Some(json!("CHANGES_REQUESTED")));
        assert_eq!(approvals(&shared), before);
        let bodies: Vec<String> = shared.lock().store.entries(&d).unwrap().iter().filter_map(|x| x.entry().unwrap().body).collect();
        assert!(!bodies.iter().any(|b| b.contains("dismissed")), "{bodies:?}");
        assert!(shared.lock().verify(&d).unwrap().ok);
        let _ = std::fs::remove_dir_all(&wt);
    }

    #[test]
    fn a_rounds_work_is_kept_on_its_docket() {
        let dir = std::env::temp_dir().join(format!("wh-keep-{}", std::process::id()));
        let (main, other) = (dir.join("main"), dir.join("other"));
        let out = main.join(".tmp/rounds/2");
        std::fs::create_dir_all(out.join("reviewers/codex")).unwrap();
        std::fs::create_dir_all(other.join(".tmp/rounds/2")).unwrap();
        std::fs::write(out.join("report.md"), "# claude\nAll good.").unwrap();
        std::fs::write(out.join("review.json"), r#"{"event":"APPROVE","verdict":"Fine.","findings":[]}"#).unwrap();
        std::fs::write(out.join("reviewers/codex/report.md"), "# codex\nOne nit.").unwrap();
        std::fs::write(out.join("review.verified.json"), r#"{"event":"APPROVE","verdict":"Fine, checked.","findings":[]}"#).unwrap();
        std::fs::write(out.join("merge-risk.json"), r#"{"alignment":{"verdict":"good","gaps":[]}}"#).unwrap();
        let mut e = Pr::new("o/api", 7);
        e.round = 2;
        e.state = "reviewing".into();
        e.worktree = Some(main.display().to_string());
        for (name, wt, primary) in [("claude", &main, true), ("codex", &other, false)] {
            e.reviewers
                .insert(name.into(), super::super::pr::Reviewer { name: name.into(), agent: name.into(), primary, worktree: Some(wt.display().to_string()), ..Default::default() });
            e.round_reviewers.push(name.into());
        }
        let mut core = crate::hub::test_core();
        let d = super::super::save(&mut core, &mut e).unwrap();
        keep_round(&mut core, &e, Some("APPROVE on abc\n\nFine, checked."));
        let l = crate::artifacts::list(&core, &d).unwrap();
        let mut got: Vec<(String, String)> = l.iter().map(|a| (a["title"].as_str().unwrap().to_string(), a["name"].as_str().unwrap().to_string())).collect();
        got.sort();
        assert_eq!(
            got,
            [
                ("Merge risk", "merge-risk.json"),
                ("The checked review", "review.verified.json"),
                ("The review as posted", "round-2-posted.md"),
                ("claude's draft review", "claude-review.json"),
                ("claude's report", "claude-report.md"),
                ("codex's report", "codex-report.md"),
            ]
            .map(|(a, b)| (a.to_string(), b.to_string()))
        );
        assert!(l.iter().all(|a| a["group"] == "Round 2" && a["round"] == 2 && a["source"] == "review"));
        // Kept twice (a post after a dry run, say): once.
        keep_round(&mut core, &e, Some("APPROVE on abc\n\nFine, checked."));
        assert_eq!(crate::artifacts::list(&core, &d).unwrap().len(), 6);
        assert!(core.verify_full(&d).unwrap().ok);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn labels_fit_and_ports_follow_the_reviewer() {
        let raw = json!({ "repos": { "acme/a-very-long-repository-name-for-labels": { "checkout": "/r" } }, "reviewers": ["claude", { "name": "claude-2", "agent": "claude" }] });
        let cfg = super::super::config::from_value("/x".into(), raw).unwrap();
        let e = Pr::new("acme/a-very-long-repository-name-for-labels", 1234);
        let l = label(&e, &cfg, Some("claude-2"));
        assert!(l.len() <= 32, "{l}");
        assert!(l.ends_with("-1234-claude-2") && !l.contains("--"), "{l}");
        assert!(label(&e, &cfg, Some("check")).len() <= 32);
        let e = Pr::new("o/9lives", 7);
        assert_eq!(label(&e, &cfg, None), "pr-9lives-7");
        assert_eq!((port(1, 632), port(2, 632), port(1, 10632)), (20632, 30632, 20632));
    }

    #[test]
    fn worktrees_stay_where_an_imported_pr_had_them() {
        let mut e = Pr::new("o/api", 7);
        assert!(worktree_path(&e, None).ends_with("worktrees/o/api/pr-7"));
        e.worktree = Some("/old/rex-reviewer/worktrees/o/api/pr-7".into());
        assert_eq!(worktree_path(&e, Some("claude-2")), PathBuf::from("/old/rex-reviewer/worktrees/o/api/pr-7-claude-2"));
    }

    #[test]
    fn a_reviewer_moved_to_another_folder_starts_fresh() {
        let raw = json!({ "repos": { "o/api": { "checkout": "/r" } }, "reviewers": ["claude", { "name": "claude-2", "agent": "claude" }] });
        let cfg = super::super::config::from_value("/x".into(), raw).unwrap();
        let mut e = Pr::new("o/api", 7);
        e.state = "posted".into();
        e.worktree = Some("/w/o/api/pr-7".into());
        // Was a second reviewer beside Codex; now it's the primary.
        e.reviewers.insert(
            "claude".into(),
            Reviewer { name: "claude".into(), agent: "claude".into(), index: 2, worktree: Some("/w/o/api/pr-7-claude".into()), session: Some("s1".into()), ..Default::default() },
        );
        e.reviewers.insert(
            "claude-2".into(),
            Reviewer {
                name: "claude-2".into(),
                agent: "claude".into(),
                index: 2,
                worktree: Some("/w/o/api/pr-7-claude-2".into()),
                session: Some("s2".into()),
                ..Default::default()
            },
        );
        let rs = round_reviewers(&e, &cfg);
        assert_eq!(rs[0].worktree.as_deref(), Some("/w/o/api/pr-7"));
        assert_eq!(rs[0].session, None, "can't resume in another folder");
        assert_eq!(rs[1].session.as_deref(), Some("s2"));
        assert_eq!(rs[0].pane.as_deref(), Some("api-7"));
        assert_eq!(rs[1].pane.as_deref(), Some("api-7-claude-2"));
    }

    #[test]
    fn delta_lines_count_only_hunk_lines() {
        let d = "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1,2 +1,2 @@\n-a\n+b\n c\n";
        assert_eq!(delta_lines(d), 2);
    }
}
