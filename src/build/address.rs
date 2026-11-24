//! Address rounds: after a Build's PR opens, what reviewers and CI say comes
//! back to the builder as a round of fixes and replies.
//!
//! While the PR is open, each poll reads it as the `pr-reply` contract does:
//! review threads still open whose last word isn't yours, review bodies and
//! conversation comments nothing of yours has answered, the reviewers to ask
//! again, the checks that failed on its head, and whether the base moved on.
//! Something new is an ask: Address it, or Not now. A round reopens the
//! builder (its own session), which fixes, merges the base when it moved,
//! commits, and drafts one reply per thread and an answer per review body or
//! comment. The checker checks it. You approve the push, seeing every reply
//! as it will post; then the hub pushes (fast-forward, or with a lease after
//! a rebase), posts the replies from your account with a footer naming the
//! agent, and re-requests the reviewers. The builder can't push or post.

use anyhow::{Result, anyhow, bail};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use super::config::Config;
use super::decide::Reply;
use super::steps::{self, att, short, wt};
use super::{Attention, Task};
use crate::hub::{Shared, Who};
use crate::review::panes::Place;
use crate::review::rounds::{fetch, git, set_status};
use crate::review::{agentcli, github};

/// Everything a round needs, in one read (the `pr-reply open` query, with
/// the head's checks and the merge state).
const QUERY: &str = "query($owner: String!, $name: String!, $pr: Int!) {
  viewer { login }
  repository(owner: $owner, name: $name) {
    pullRequest(number: $pr) {
      number title state url headRefOid baseRefName baseRefOid mergeable mergeStateStatus author { login }
      reviewRequests(first: 50) { nodes { requestedReviewer { ... on User { login } } } }
      reviews(first: 100) { nodes { databaseId url state body submittedAt author { login __typename } } }
      comments(first: 100) { nodes { databaseId url body createdAt author { login __typename } } }
      reviewThreads(first: 100) { nodes {
        isResolved isOutdated path line originalLine
        comments(first: 50) { nodes { databaseId url body createdAt author { login __typename } } }
      } }
      commits(last: 1) { nodes { commit { oid statusCheckRollup { state contexts(first: 60) { nodes {
        __typename
        ... on CheckRun { name status conclusion detailsUrl }
        ... on StatusContext { context state targetUrl }
      } } } } } }
    }
  }
}";

pub async fn fetch_pr(repo: &str, pr: i64) -> Result<Value> {
    let (owner, name) = repo.split_once('/').ok_or_else(|| anyhow!("{repo} isn't owner/name"))?;
    let out =
        github::gh(&["api", "graphql", "-f", &format!("query={QUERY}"), "-F", &format!("owner={owner}"), "-F", &format!("name={name}"), "-F", &format!("pr={pr}")], None).await?;
    let v: Value = serde_json::from_str(&out)?;
    if v["data"]["repository"]["pullRequest"].is_null() {
        bail!("GitHub didn't return #{pr} on {repo}");
    }
    Ok(v)
}

/// What's open on a PR, read from the query's answer.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Open {
    pub me: String,
    pub state: String,
    pub head: String,
    pub base: String,
    pub base_oid: String,
    pub threads: Vec<Value>,
    pub reviews: Vec<Value>,
    pub comments: Vec<Value>,
    pub rerequest: Vec<String>,
    /// Checks that failed on the head, once they've all finished.
    pub checks: Vec<Value>,
    /// `behind` or `conflicts`, when the base moved on.
    pub base_state: Option<String>,
}

fn who(c: &Value) -> Value {
    json!({ "author": c["author"]["login"].as_str().unwrap_or("ghost"), "bot": c["author"]["__typename"] == "Bot" })
}

fn first(s: &str) -> String {
    s.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("").chars().take(140).collect()
}

/// The query's answer as what needs an answer (as `pr-reply open` says it).
pub fn scan(v: &Value) -> Open {
    let p = &v["data"]["repository"]["pullRequest"];
    let me = v["data"]["viewer"]["login"].as_str().unwrap_or("").to_string();
    let nodes = |x: &Value| x["nodes"].as_array().cloned().unwrap_or_default();
    let mine: Vec<Value> = nodes(&p["comments"]).into_iter().filter(|c| c["author"]["login"] == me.as_str()).collect();
    // Answered: a later comment of yours links to it.
    let answered = |url: &str, at: &str| mine.iter().any(|c| c["createdAt"].as_str().unwrap_or("") > at && c["body"].as_str().unwrap_or("").contains(url));
    let threads = nodes(&p["reviewThreads"])
        .into_iter()
        .filter(|t| t["isResolved"] != true)
        .filter(|t| nodes(&t["comments"]).last().is_some_and(|c| c["author"]["login"] != me.as_str()))
        .map(|t| {
            let cs = nodes(&t["comments"]);
            json!({
                "comment_id": cs[0]["databaseId"], "url": cs[0]["url"], "path": t["path"],
                "line": if t["line"].is_null() { t["originalLine"].clone() } else { t["line"].clone() },
                "outdated": t["isOutdated"], "last": cs.last().map(|c| c["databaseId"].clone()),
                "comments": cs.iter().map(|c| { let mut w = who(c); w["body"] = c["body"].clone(); w["at"] = c["createdAt"].clone(); w["id"] = c["databaseId"].clone(); w }).collect::<Vec<_>>(),
            })
        })
        .collect();
    let reviews: Vec<Value> = nodes(&p["reviews"])
        .into_iter()
        .filter(|r| r["author"]["login"] != me.as_str() && r["state"] != "DISMISSED" && r["state"] != "PENDING" && !r["body"].as_str().unwrap_or("").trim().is_empty())
        .filter(|r| !answered(r["url"].as_str().unwrap_or("?"), r["submittedAt"].as_str().unwrap_or("")))
        .map(|r| {
            let mut w = who(&r);
            for k in ["url", "state", "body"] {
                w[k] = r[k].clone();
            }
            w["at"] = r["submittedAt"].clone();
            w["id"] = r["databaseId"].clone();
            w
        })
        .collect();
    // Bots in the conversation (deploy previews, Linear links) need no answer.
    let comments: Vec<Value> = nodes(&p["comments"])
        .into_iter()
        .filter(|c| c["author"]["login"] != me.as_str() && c["author"]["__typename"] != "Bot")
        .filter(|c| !answered(c["url"].as_str().unwrap_or("?"), c["createdAt"].as_str().unwrap_or("")))
        .map(|c| {
            let mut w = who(&c);
            w["url"] = c["url"].clone();
            w["body"] = c["body"].clone();
            w["at"] = c["createdAt"].clone();
            w["id"] = c["databaseId"].clone();
            w
        })
        .collect();
    let requested: BTreeSet<String> = nodes(&p["reviewRequests"]).iter().filter_map(|r| r["requestedReviewer"]["login"].as_str().map(str::to_string)).collect();
    let mut latest: BTreeMap<String, (String, String)> = BTreeMap::new();
    for r in nodes(&p["reviews"]) {
        let (Some(l), Some(at)) = (r["author"]["login"].as_str(), r["submittedAt"].as_str()) else { continue };
        if l == me || r["author"]["__typename"] != "User" || r["state"] == "PENDING" {
            continue;
        }
        if latest.get(l).is_none_or(|(a, _)| at > a.as_str()) {
            latest.insert(l.to_string(), (at.to_string(), r["state"].as_str().unwrap_or("").to_string()));
        }
    }
    let rerequest = latest.into_iter().filter(|(l, (_, s))| (s == "CHANGES_REQUESTED" || s == "COMMENTED") && !requested.contains(l)).map(|(l, _)| l).collect();
    // Checks: only once every one on the head has finished.
    let commit = &p["commits"]["nodes"][0]["commit"];
    let contexts = nodes(&commit["statusCheckRollup"]["contexts"]);
    let pending = contexts.iter().any(|c| match c["__typename"].as_str() {
        Some("CheckRun") => c["status"] != "COMPLETED",
        _ => matches!(c["state"].as_str(), Some("PENDING" | "EXPECTED")),
    });
    let checks = if pending {
        Vec::new()
    } else {
        contexts
            .iter()
            .filter(|c| match c["__typename"].as_str() {
                Some("CheckRun") => matches!(c["conclusion"].as_str(), Some("FAILURE" | "TIMED_OUT" | "STARTUP_FAILURE" | "ACTION_REQUIRED")),
                _ => matches!(c["state"].as_str(), Some("FAILURE" | "ERROR")),
            })
            .map(|c| json!({ "name": c["name"].as_str().or(c["context"].as_str()).unwrap_or("a check"), "url": c["detailsUrl"].as_str().or(c["targetUrl"].as_str()), "conclusion": c["conclusion"].as_str().or(c["state"].as_str()) }))
            .collect()
    };
    let base_state = match (p["mergeable"].as_str(), p["mergeStateStatus"].as_str()) {
        (Some("CONFLICTING"), _) | (_, Some("DIRTY")) => Some("conflicts".to_string()),
        (_, Some("BEHIND")) => Some("behind".to_string()),
        _ => None,
    };
    Open {
        me,
        state: p["state"].as_str().unwrap_or("").to_string(),
        head: p["headRefOid"].as_str().unwrap_or("").to_string(),
        base: p["baseRefName"].as_str().unwrap_or("").to_string(),
        base_oid: p["baseRefOid"].as_str().unwrap_or("").to_string(),
        threads,
        reviews,
        comments,
        rerequest,
        checks,
        base_state,
    }
}

impl Open {
    /// Each thing to address: its id (a new comment in a thread, a new
    /// failing head, a new base, makes a new one) and a line about it.
    pub fn items(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for t in &self.threads {
            let last = t["comments"].as_array().and_then(|c| c.last()).cloned().unwrap_or_default();
            let at = match (t["path"].as_str(), t["line"].as_i64()) {
                (Some(p), Some(l)) => format!(" on {p}:{l}"),
                (Some(p), None) => format!(" on {p}"),
                _ => String::new(),
            };
            out.push((format!("t:{}:{}", t["comment_id"], last["id"]), format!("{}{at}: {}", last["author"].as_str().unwrap_or("?"), first(last["body"].as_str().unwrap_or("")))));
        }
        for r in &self.reviews {
            let state = match r["state"].as_str() {
                Some("CHANGES_REQUESTED") => " (changes requested)",
                Some("APPROVED") => " (approved)",
                _ => "",
            };
            out.push((format!("r:{}", r["id"]), format!("{}'s review{state}: {}", r["author"].as_str().unwrap_or("?"), first(r["body"].as_str().unwrap_or("")))));
        }
        for c in &self.comments {
            out.push((format!("c:{}", c["id"]), format!("{}: {}", c["author"].as_str().unwrap_or("?"), first(c["body"].as_str().unwrap_or("")))));
        }
        if !self.checks.is_empty() {
            let names: Vec<&str> = self.checks.iter().filter_map(|c| c["name"].as_str()).collect();
            out.push((format!("ci:{}", self.head), format!("Failing on the head: {}", names.join(", "))));
        }
        if let Some(s) = &self.base_state {
            out.push((format!("base:{}:{s}", self.base_oid), if s == "conflicts" { format!("Conflicts with {}", self.base) } else { format!("Behind {}", self.base) }));
        }
        out
    }

    pub fn json(&self) -> Value {
        json!({
            "me": self.me, "head": self.head, "base": self.base, "base_oid": self.base_oid,
            "threads": self.threads, "reviews": self.reviews, "comments": self.comments,
            "rerequest": self.rerequest, "checks": self.checks, "base_state": self.base_state,
        })
    }
}

/// Words for a list of things: "2 review threads, a failing check, behind main".
fn sum(o: &Open) -> String {
    let n = |k: usize, one: &str, many: &str| if k == 1 { format!("a {one}") } else { format!("{k} {many}") };
    let mut parts = Vec::new();
    if !o.threads.is_empty() {
        parts.push(n(o.threads.len(), "review thread", "review threads"));
    }
    if !o.reviews.is_empty() {
        parts.push(n(o.reviews.len(), "review", "reviews"));
    }
    if !o.comments.is_empty() {
        parts.push(n(o.comments.len(), "comment", "comments"));
    }
    if !o.checks.is_empty() {
        parts.push(n(o.checks.len(), "failing check", "failing checks"));
    }
    if let Some(s) = &o.base_state {
        parts.push(if s == "conflicts" { format!("conflicts with {}", o.base) } else { format!("behind {}", o.base) });
    }
    parts.join(", ")
}

// ---- Following the PR ----

/// A poll of an open PR: merged or closed ends it; anything new to address
/// is an ask (or, with `gates.address = "auto"`, a round).
/// Everyone's reviews, comments and review-thread comments on the PR but
/// yours and bots': `(id, data, body)`, oldest first.
pub fn feedback(v: &Value) -> Vec<(String, Value, String)> {
    let p = &v["data"]["repository"]["pullRequest"];
    let me = v["data"]["viewer"]["login"].as_str().unwrap_or("");
    let nodes = |x: &Value| x["nodes"].as_array().cloned().unwrap_or_default();
    let person = |c: &Value| c["author"]["login"] != me && c["author"]["__typename"] != "Bot" && c["author"]["login"].is_string();
    let mut out: Vec<(String, Value, String)> = Vec::new();
    for r in nodes(&p["reviews"]).into_iter().filter(|r| person(r) && r["state"] != "PENDING") {
        let d = json!({ "kind": "review", "state": r["state"], "author": r["author"]["login"], "url": r["url"], "at": r["submittedAt"] });
        out.push((format!("review-{}", r["databaseId"]), d, r["body"].as_str().unwrap_or("").to_string()));
    }
    for c in nodes(&p["comments"]).into_iter().filter(|c| person(c)) {
        let d = json!({ "kind": "comment", "author": c["author"]["login"], "url": c["url"], "at": c["createdAt"] });
        out.push((format!("comment-{}", c["databaseId"]), d, c["body"].as_str().unwrap_or("").to_string()));
    }
    for t in nodes(&p["reviewThreads"]) {
        let line = if t["line"].is_null() { t["originalLine"].clone() } else { t["line"].clone() };
        for c in nodes(&t["comments"]).into_iter().filter(|c| person(c)) {
            let d = json!({ "kind": "thread", "author": c["author"]["login"], "url": c["url"], "at": c["createdAt"], "path": t["path"], "line": line });
            out.push((format!("thread-{}", c["databaseId"]), d, c["body"].as_str().unwrap_or("").to_string()));
        }
    }
    out.sort_by(|a, b| a.1["at"].as_str().unwrap_or("").cmp(b.1["at"].as_str().unwrap_or("")));
    out
}

/// Reviews come home: each person's review, comment and thread comment on
/// the PR goes on the docket once, as it reads on GitHub (checkable there).
fn record_feedback(shared: &Shared, t: &mut Task, n: i64, v: &Value) -> Result<()> {
    let mut done: Vec<String> = t.extra.get("recorded").and_then(|x| serde_json::from_value(x.clone()).ok()).unwrap_or_default();
    let fresh: Vec<(String, Value, String)> = feedback(v).into_iter().filter(|(id, ..)| !done.contains(id)).collect();
    if fresh.is_empty() {
        return Ok(());
    }
    let mut core = shared.lock();
    for (id, mut d, body) in fresh {
        let who = d["author"].as_str().unwrap_or("someone").to_string();
        let words = match (d["kind"].as_str(), d["state"].as_str()) {
            (Some("review"), Some("APPROVED")) => format!("{who} approved #{n}"),
            (Some("review"), Some("CHANGES_REQUESTED")) => format!("{who} requested changes on #{n}"),
            (Some("review"), _) => format!("{who} reviewed #{n}"),
            (Some("thread"), _) => format!("{who} on {}{}", d["path"].as_str().unwrap_or("the diff"), d["line"].as_i64().map(|l| format!(":{l}")).unwrap_or_default()),
            _ => format!("{who} commented on #{n}"),
        };
        d["what"] = json!("github-feedback");
        d["source"] = json!("github");
        d["pr"] = json!(n);
        d["id"] = json!(id);
        d["summary"] = json!(words);
        let text = if body.trim().is_empty() { words } else { body };
        core.append(&t.docket, "observed", &Who::Hub, Some(text), d)?;
        done.push(id);
    }
    t.extra.insert("recorded".into(), json!(done));
    Ok(())
}

pub async fn follow(shared: &Shared, cfg: &Config, t: &mut Task) -> Result<Option<Attention>> {
    let Some(n) = t.pr_number else { return Ok(None) };
    let raw = fetch_pr(&t.repo, n).await?;
    record_feedback(shared, t, n, &raw)?;
    let o = scan(&raw);
    match o.state.as_str() {
        "MERGED" | "CLOSED" => {
            steps::ended(shared, t, o.state == "MERGED").await?;
            return Ok(None);
        }
        _ => {}
    }
    let items = o.items();
    let fresh: Vec<&(String, String)> = items.iter().filter(|(id, _)| !t.seen.contains(id)).collect();
    if fresh.is_empty() {
        // Answered elsewhere since the ask went out: it's moot.
        if t.asks.contains_key("address") {
            steps::close_ask(shared, t, "address", "nothing's left to address");
            t.extra.remove("pending");
        }
        return Ok(None);
    }
    let ids: Vec<String> = items.iter().map(|(id, _)| id.clone()).collect();
    let was: Vec<String> = t.extra.get("pending").and_then(|p| serde_json::from_value(p["ids"].clone()).ok()).unwrap_or_default();
    if was == ids && t.asks.contains_key("address") {
        return Ok(None);
    }
    t.extra.insert("pending".into(), json!({ "ids": ids, "open": o.json() }));
    if cfg.gate("address") == "auto" {
        return start(shared, cfg, t, o).await.map(|_| None);
    }
    let mut body = format!("#{n}, {}, has {}:\n\n", t.title, sum(&o));
    for (_, line) in &items {
        body.push_str(&format!("- {line}\n"));
    }
    body.push_str(&format!(
        "\nAddress it reopens the builder: it fixes what's valid, {}drafts a reply to each, and the checker checks it. You see every reply, and approve the push, before anything reaches GitHub. Not now leaves these for later; anything new asks again.",
        if o.base_state.is_some() { format!("merges {} in, ", o.base) } else { String::new() }
    ));
    steps::send_ask(shared, t, "address", &format!("{} · PR #{n} · {}", t.key, sum(&o)), &body, &["Address it", "Not now"])?;
    Ok(None)
}

/// Your answer to "address it?".
pub async fn answer(shared: &Shared, cfg: &Config, t: &mut Task, r: Reply) -> Result<()> {
    let pending = t.extra.remove("pending").unwrap_or_default();
    let ids: Vec<String> = serde_json::from_value(pending["ids"].clone()).unwrap_or_default();
    if r.choice == Some(1) {
        t.seen.extend(ids.iter().cloned());
        let mut core = shared.lock();
        super::event(
            &mut core,
            &t.docket,
            "set-aside",
            format!("You set aside what's open on the PR ({} item{}); anything new asks again.", ids.len(), if ids.len() == 1 { "" } else { "s" }),
            json!({ "items": ids }),
        )?;
        return Ok(());
    }
    // Read it again: it may have moved since you were asked.
    let Some(n) = t.pr_number else { return Ok(()) };
    let mut o = scan(&fetch_pr(&t.repo, n).await?);
    if !r.text.trim().is_empty() {
        o.comments.push(json!({ "author": o.me, "bot": false, "url": null, "body": r.text.trim(), "note": "from the PR's owner, with the go-ahead: not on GitHub, so no answer is posted for it" }));
    }
    start(shared, cfg, t, o).await
}

// ---- A round ----

/// The base, brought up to date how `update` says.
fn update_words(cfg: &Config, o: &Open) -> String {
    match (&o.base_state, cfg.update.as_str()) {
        (None, _) => format!("{} hasn't moved on in a way that matters: leave the base alone.", o.base),
        (Some(_), "rebase") => format!(
            "{base} moved on (the PR is {state}). Rebase the branch onto `origin/{base}` (`git rebase origin/{base}`), resolving each conflict so both sides' intent survives, then run the tests. Wheelhouse pushes the rebased branch with a lease, after approval.",
            base = o.base,
            state = o.base_state.as_deref().unwrap_or("behind")
        ),
        (Some(_), _) => format!(
            "{base} moved on (the PR is {state}). Merge it in: `git merge origin/{base}`, resolving each conflict so both sides' intent survives, then run the tests. Don't rebase: the branch is public.",
            base = o.base,
            state = o.base_state.as_deref().unwrap_or("behind")
        ),
    }
}

/// Start a round: bring the worktree to the PR's head, write what's open,
/// and reopen the builder on it.
pub async fn start(shared: &Shared, cfg: &Config, t: &mut Task, o: Open) -> Result<()> {
    let w = wt(t);
    let checkout = cfg.repos.get(&t.repo).map(|r| r.checkout.clone()).ok_or_else(|| anyhow!("{} isn't one of Build's repos", t.repo))?;
    let (branch, base) = (t.branch.clone().unwrap_or_default(), t.base.clone().unwrap_or_else(|| o.base.clone()));
    if !w.join(".git").exists() {
        bail!("the Build's worktree is gone; nothing to address in");
    }
    fetch(&checkout, &t.repo, &[format!("+refs/heads/{branch}:refs/remotes/origin/{branch}"), format!("+refs/heads/{base}:refs/remotes/origin/{base}")]).await?;
    // Someone may have pushed (a suggestion applied on GitHub): start from there.
    let remote = git(&w, &["rev-parse", &format!("origin/{branch}")]).await?.trim().to_string();
    let head = git(&w, &["rev-parse", "HEAD"]).await?.trim().to_string();
    if remote != head {
        if git(&w, &["merge-base", "--is-ancestor", &head, &remote]).await.is_ok() {
            git(&w, &["merge", "--ff-only", "--quiet", &remote]).await?;
        } else if git(&w, &["merge-base", "--is-ancestor", &remote, &head]).await.is_err() {
            bail!("the worktree's {branch} and GitHub's have gone different ways; look at it by hand");
        }
    }
    let from = git(&w, &["rev-parse", "HEAD"]).await?.trim().to_string();
    t.pushed_head = Some(remote);
    t.rounds += 1;
    let n = t.rounds;
    let dir = w.join(format!(".tmp/address/round-{n}"));
    std::fs::create_dir_all(&dir)?;
    let _ = std::fs::remove_file(dir.join("replies.json"));
    crate::review::write_atomic(&dir.join("open.json"), &serde_json::to_string_pretty(&o.json())?)?;
    // The failing part of each failing GitHub Actions job's log.
    for (i, c) in o.checks.iter().enumerate() {
        let Some(job) = c["url"].as_str().and_then(|u| u.split("/job/").nth(1)).and_then(|j| j.split(['/', '?']).next()) else { continue };
        if let Ok(log) = github::gh(&["run", "view", "--repo", &t.repo, "--job", job, "--log-failed"], None).await {
            let lines: Vec<&str> = log.lines().collect();
            let tail = lines[lines.len().saturating_sub(300)..].join("\n");
            crate::review::write_atomic(&dir.join(format!("ci-{}.log", i + 1)), &tail)?;
        }
    }
    let login = github::login().await.unwrap_or_else(|_| "you".into());
    let mut v = steps::vars(cfg, t, &login);
    v.insert("ROUND", n.to_string());
    v.insert("PR", t.pr_number.unwrap_or(0).to_string());
    v.insert("PR_URL", t.pr_url.clone().unwrap_or_default());
    v.insert("UPDATE", update_words(cfg, &o));
    crate::review::write_atomic(&w.join(".tmp/tasks/address.md"), &crate::review::briefs::render(&steps::brief("address.md"), &v))?;
    let ids: Vec<String> = o.items().into_iter().map(|(id, _)| id).collect();
    t.round = Some(json!({ "n": n, "items": ids, "update": o.base_state.as_ref().map(|_| cfg.update.clone()), "from": from, "summary": sum(&o) }));
    t.fix_used = 0;
    t.built_head = None;
    t.checked_head = None;
    t.approved_head = None;
    set_status(&w, "WORKING addressing")?;
    t.state = "active".into();
    t.phase = Some("addressing".into());
    let prompt = format!("Read .tmp/ground-rules.md, then .tmp/tasks/address.md, and do it. This is address round {n} on PR #{}.", t.pr_number.unwrap_or(0));
    steps::open_role(shared, cfg, t, "builder", &prompt, Place::Tab).await?;
    let mut core = shared.lock();
    super::event(
        &mut core,
        &t.docket,
        "addressing",
        format!("Round {n}: the builder is addressing {} on #{}.", sum(&o), t.pr_number.unwrap_or(0)),
        json!({ "round": n, "items": t.round.as_ref().map(|r| r["items"].clone()) }),
    )?;
    Ok(())
}

/// The round's open things, as it started with them.
fn open_of(t: &Task) -> Value {
    let n = t.round.as_ref().and_then(|r| r["n"].as_i64()).unwrap_or(0);
    std::fs::read_to_string(wt(t).join(format!(".tmp/address/round-{n}/open.json"))).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn replies_path(t: &Task) -> std::path::PathBuf {
    let n = t.round.as_ref().and_then(|r| r["n"].as_i64()).unwrap_or(0);
    wt(t).join(format!(".tmp/address/round-{n}/replies.json"))
}

/// Short commit ids a text names: hex with a digit and a letter, 7 to 40.
fn shas(text: &str) -> Vec<String> {
    let prose = strip_fences(text);
    prose
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| (7..=40).contains(&w.len()) && w.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()))
        .filter(|w| w.chars().any(|c| c.is_ascii_digit()) && w.chars().any(|c| c.is_ascii_alphabetic()))
        .map(str::to_string)
        .collect()
}

fn strip_fences(s: &str) -> String {
    let mut out = String::new();
    let mut fenced = false;
    for l in s.lines() {
        if l.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if !fenced {
            out.push_str(l);
            out.push('\n');
        }
    }
    out
}

/// What's wrong with a round's replies, against what it set out to answer:
/// the first problem, in words the builder can act on.
pub fn check_replies(open: &Value, d: &Value) -> Option<String> {
    let replies = d["replies"].as_array().cloned().unwrap_or_default();
    let answers = d["answers"].as_array().cloned().unwrap_or_default();
    // A thread takes its reply on its first comment; any comment in it maps there.
    let mut root: BTreeMap<String, String> = BTreeMap::new();
    for th in open["threads"].as_array().into_iter().flatten() {
        let r = th["comment_id"].to_string();
        for c in th["comments"].as_array().into_iter().flatten() {
            root.insert(c["id"].to_string(), r.clone());
        }
        root.insert(r.clone(), r);
    }
    let mut answered: BTreeSet<String> = BTreeSet::new();
    for r in &replies {
        let id = r["comment_id"].to_string();
        let Some(rt) = root.get(&id) else { return Some(format!("replies.json answers comment {id}, which isn't in one of the round's open threads (open.json lists them)")) };
        answered.insert(rt.clone());
        if r["body"].as_str().is_none_or(|b| b.trim().is_empty()) {
            return Some(format!("the reply to comment {id} has no body"));
        }
    }
    for th in open["threads"].as_array().into_iter().flatten() {
        if !answered.contains(&th["comment_id"].to_string()) {
            return Some(format!(
                "the thread on {}:{} (comment {}) has no reply: every thread gets one, even when nothing changed",
                th["path"].as_str().unwrap_or("?"),
                th["line"],
                th["comment_id"]
            ));
        }
    }
    let urls: BTreeSet<&str> = answers.iter().filter_map(|a| a["url"].as_str()).collect();
    for a in &answers {
        if a["url"].as_str().is_none_or(|u| !u.starts_with("https://github.com/")) {
            return Some("every answer needs the url of the review or comment it answers".into());
        }
        if a["body"].as_str().is_none_or(|b| b.trim().is_empty()) {
            return Some(format!("the answer to {} has no body", a["url"].as_str().unwrap_or("?")));
        }
    }
    for x in open["reviews"].as_array().into_iter().flatten().chain(open["comments"].as_array().into_iter().flatten()) {
        let Some(u) = x["url"].as_str() else { continue };
        if !urls.contains(u) {
            return Some(format!(
                "{}'s {} ({u}) has no answer in replies.json's answers",
                x["author"].as_str().unwrap_or("?"),
                if x["state"].is_null() { "comment" } else { "review" }
            ));
        }
    }
    let me = open["me"].as_str().unwrap_or("");
    if d["rerequest"].as_array().into_iter().flatten().any(|r| r.as_str() == Some(me)) {
        return Some("rerequest names the PR's own author".into());
    }
    let all: Vec<Value> = replies.iter().chain(answers.iter()).cloned().collect();
    let fp = crate::review::post::first_person(&json!({ "replies": all }));
    if !fp.is_empty() {
        return Some(format!("the replies speak in the first person; the footer already says who wrote them. Rewrite these lines: {}", fp.join(" · ")));
    }
    if all.iter().any(|r| r["body"].as_str().is_some_and(|b| b.contains(".tmp/"))) {
        return Some("a reply mentions .tmp/, which reviewers can't see".into());
    }
    None
}

/// The builder wrote BUILT on a round: check its replies and commits, then
/// to the checker (or, with no new commits, straight to your call).
pub async fn built(shared: &Shared, cfg: &Config, t: &mut Task) -> Result<()> {
    let w = wt(t);
    let from = t.round.as_ref().and_then(|r| r["from"].as_str()).unwrap_or("").to_string();
    let range = format!("{from}..HEAD");
    let count = git(&w, &["rev-list", "--count", &range]).await.unwrap_or_default().trim().parse::<i64>().unwrap_or(0);
    let tmp: Vec<String> = git(&w, &["diff", "--name-only", &range]).await.unwrap_or_default().lines().filter(|l| l.starts_with(".tmp/")).map(str::to_string).collect();
    let open = open_of(t);
    let d: Option<Value> =
        (crate::review::through_link(&replies_path(t)).is_none()).then(|| std::fs::read_to_string(replies_path(t)).ok()).flatten().and_then(|s| serde_json::from_str(&s).ok());
    let needs_commit = !open["checks"].as_array().is_none_or(Vec::is_empty) || !open["base_state"].is_null();
    let mut problem = if !tmp.is_empty() {
        Some(format!("the commits include files under .tmp/, which never ship: {}. Take them out of the commits", tmp.join(", ")))
    } else if needs_commit && count == 0 {
        Some("the round has failing checks or a base to merge, but no new commits".to_string())
    } else if let Some(d) = &d {
        check_replies(&open, d)
    } else {
        Some(format!("{} is missing or isn't JSON", replies_path(t).strip_prefix(&w).unwrap_or(Path::new("replies.json")).display()))
    };
    // Every commit a reply names is on the branch.
    if problem.is_none()
        && let Some(d) = &d
    {
        let text: String =
            d["replies"].as_array().into_iter().flatten().chain(d["answers"].as_array().into_iter().flatten()).filter_map(|r| r["body"].as_str()).collect::<Vec<_>>().join("\n");
        for s in shas(&text) {
            if git(&w, &["merge-base", "--is-ancestor", &s, "HEAD"]).await.is_err() {
                problem = Some(format!("a reply names {s}, which isn't a commit on {}", t.branch.clone().unwrap_or_default()));
                break;
            }
        }
    }
    if let Some(p) = problem {
        let p = format!("Round {}: {p}. Fix it, then write BUILT again.", t.rounds);
        if t.told.contains(&p) {
            return Ok(());
        }
        t.told.push(p.clone());
        return steps::tell_builder(shared, t, &p, "WORKING addressing").await;
    }
    if count > 0 {
        return steps::start_check(shared, cfg, t).await;
    }
    // Only words: nothing for the checker to run.
    let head = git(&w, &["rev-parse", "HEAD"]).await?.trim().to_string();
    t.built_head = Some(head.clone());
    t.checked_head = Some(head);
    ask_push(shared, t)
}

/// The footer, as the PR's and the review's: who wrote it, for whom.
fn footer(by: &str, me: &str) -> String {
    format!("---\n\n<div align=\"right\"><sub><i>{by} on behalf of @{me}</i></sub></div>")
}

/// The replies as they'll post: each thread's reply (on its first comment),
/// the one conversation comment with the answers, and who's asked again.
pub fn render(open: &Value, d: &Value, by: &str) -> (Vec<(String, String)>, Option<String>, Vec<String>) {
    let me = open["me"].as_str().unwrap_or("you");
    let mut root: BTreeMap<String, String> = BTreeMap::new();
    for th in open["threads"].as_array().into_iter().flatten() {
        for c in th["comments"].as_array().into_iter().flatten() {
            root.insert(c["id"].to_string(), th["comment_id"].to_string());
        }
        root.insert(th["comment_id"].to_string(), th["comment_id"].to_string());
    }
    let replies = d["replies"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| root.get(&r["comment_id"].to_string()).map(|id| (id.clone(), format!("{}\n\n{}", r["body"].as_str().unwrap_or("").trim(), footer(by, me)))))
        .collect();
    let answers: Vec<String> = d["answers"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|a| {
            let quote = a["quote"].as_str().filter(|q| !q.trim().is_empty()).map(|q| format!("\n> {}", q.trim().replace('\n', "\n> "))).unwrap_or_default();
            format!("> **[{}]({})**{quote}\n\n{}", a["label"].as_str().unwrap_or("This comment"), a["url"].as_str().unwrap_or(""), a["body"].as_str().unwrap_or("").trim())
        })
        .collect();
    let comment = (!answers.is_empty()).then(|| format!("{}\n\n{}", answers.join("\n\n"), footer(by, me)));
    let rerequest = d["rerequest"].as_array().into_iter().flatten().filter_map(|r| r.as_str().map(str::to_string)).filter(|r| r != me).collect();
    (replies, comment, rerequest)
}

/// The round's draft, and a hash of it: what you approve is what posts.
fn said(t: &Task) -> (Value, String) {
    let path = replies_path(t);
    let text = if crate::review::through_link(&path).is_none() { std::fs::read_to_string(&path).unwrap_or_default() } else { String::new() };
    (serde_json::from_str(&text).unwrap_or_default(), crate::review::rounds::draft_digest(&text))
}

fn by(t: &Task) -> String {
    agentcli::display_name(&t.builder.agent, &wt(t), t.builder.session.as_deref())
}

/// Ask to push the round, showing every reply as it will post.
pub fn ask_push(shared: &Shared, t: &mut Task) -> Result<()> {
    let w = wt(t);
    let (d, digest) = said(t);
    t.extra.insert("asked_replies".into(), json!(digest));
    let open = open_of(t);
    let from = t.round.as_ref().and_then(|r| r["from"].as_str()).unwrap_or("").to_string();
    let commits = std::process::Command::new("git")
        .arg("-C")
        .arg(&w)
        .args(crate::review::rounds::SAFE_GIT)
        .args(["log", "--oneline", &format!("{from}..HEAD")])
        .env("PATH", github::command_path())
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    let (replies, comment, rerequest) = render(&open, &d, &by(t));
    let n = t.rounds;
    let k = commits.lines().count();
    let update = t.round.as_ref().and_then(|r| r["update"].as_str()).map(str::to_string);
    let mut subject = format!("{} · round {n} ready · {k} commit{}, {} repl{}", t.key, if k == 1 { "" } else { "s" }, replies.len(), if replies.len() == 1 { "y" } else { "ies" });
    if let Some(u) = &update {
        subject.push_str(if u == "rebase" { ", rebased" } else { ", base merged" });
    }
    let mut body = String::new();
    if k > 0 {
        body.push_str("Commits:\n");
        for l in commits.lines() {
            body.push_str(&format!("- {l}\n"));
        }
        body.push('\n');
    }
    if let Some(c) = t.last_check.as_ref().filter(|_| k > 0)
        && let Some(s) = c["summary"].as_str()
    {
        body.push_str(&format!("The checker: {s}\n\n"));
    }
    for (i, (id, text)) in replies.iter().enumerate() {
        let th = open["threads"].as_array().into_iter().flatten().find(|x| x["comment_id"].to_string().as_str() == id.as_str()).cloned().unwrap_or_default();
        body.push_str(&format!("**Reply {} of {}**, on {}:{}:\n\n{}\n\n", i + 1, replies.len(), th["path"].as_str().unwrap_or("?"), th["line"], text));
    }
    if let Some(c) = &comment {
        body.push_str(&format!("**A comment on the PR**, answering its reviews and comments:\n\n{c}\n\n"));
    }
    if !rerequest.is_empty() {
        body.push_str(&format!("Then a review is requested again from {}.\n\n", rerequest.join(", ")));
    }
    body.push_str(&format!(
        "Push and reply {} {}, then posts these from your account. Needs changes sends your words to the builder. Drop the round throws its commits away.",
        if update.as_deref() == Some("rebase") { "pushes the rebased" } else { "pushes" },
        t.branch.clone().unwrap_or_default()
    ));
    t.phase = Some("push_gate".into());
    steps::send_ask(shared, t, "push", &subject, &body, &["Push and reply", "Needs changes", "Drop the round"])
}

/// Your answer to the push gate.
pub async fn answer_push(shared: &Shared, cfg: &Config, t: &mut Task, r: Reply) -> Result<()> {
    match r.choice {
        Some(0) => {
            // Approving over failing checks is your word against them: kept.
            let over = steps::checks_ok(shared, cfg, t, t.checked_head.as_deref().unwrap_or("")).err();
            if over.is_some() {
                t.extra.insert("checks_override".into(), json!(t.checked_head));
            }
            let mut core = shared.lock();
            core.append(
                &t.docket,
                "approval",
                &Who::Owner,
                None,
                json!({ "what": "push-round", "gate": "push", "round": t.rounds, "head": t.checked_head, "replies": t.extra.get("asked_replies"), "over_checks": over }),
            )?;
            drop(core);
            t.approved_head = t.checked_head.clone();
            t.phase = Some("pushing".into());
        }
        Some(2) => drop_round(shared, t, "you dropped it").await?,
        _ => {
            t.phase = Some("addressing".into());
            steps::tell_builder(
                shared,
                t,
                &format!(
                    "Before round {} is pushed, this needs changes: {}\n\nMake them, update the replies, commit, and write BUILT. The checker checks it again.",
                    t.rounds,
                    r.text.trim()
                ),
                "WORKING addressing",
            )
            .await?;
        }
    }
    Ok(())
}

/// Throw a round away: its commits go, and the PR is followed as before.
pub async fn drop_round(shared: &Shared, t: &mut Task, why: &str) -> Result<()> {
    let w = wt(t);
    if let Some(from) = t.round.as_ref().and_then(|r| r["from"].as_str()).map(str::to_string) {
        let _ = git(&w, &["reset", "--hard", "--quiet", &from]).await;
    }
    let ids: Vec<String> = t.round.as_ref().and_then(|r| serde_json::from_value(r["items"].clone()).ok()).unwrap_or_default();
    t.seen.extend(ids);
    end_round(shared, t).await;
    let mut core = shared.lock();
    super::event(&mut core, &t.docket, "round-dropped", format!("Round {} dropped: {why}. Nothing was pushed or posted.", t.rounds), json!({ "round": t.rounds }))?;
    Ok(())
}

async fn end_round(shared: &Shared, t: &mut Task) {
    steps::close_panes(shared, t).await;
    t.round = None;
    t.state = "open".into();
    t.phase = None;
    t.extra.remove("asked_replies");
}

/// Push the approved round, then post its replies from your account.
pub async fn push(shared: &Shared, cfg: &Config, t: &mut Task) -> Result<Option<Attention>> {
    let w = wt(t);
    let branch = t.branch.clone().unwrap_or_default();
    let head = git(&w, &["rev-parse", "HEAD"]).await?.trim().to_string();
    if Some(&head) != t.checked_head.as_ref() {
        t.phase = Some("addressing".into());
        set_status(&w, "BUILT")?;
        bail!("the branch moved to {} after it was checked; it's checked again before it's pushed", short(&head));
    }
    if t.approved_head.as_ref() != Some(&head) {
        bail!("you approved {}, but the branch is at {}; nothing was pushed", t.approved_head.as_deref().map(short).unwrap_or_else(|| "nothing".into()), short(&head));
    }
    let (d, digest) = said(t);
    if t.extra.get("asked_replies").and_then(Value::as_str) != Some(digest.as_str()) {
        bail!("the replies changed after you were asked; ask again before they post. Nothing was pushed");
    }
    let from = t.round.as_ref().and_then(|r| r["from"].as_str()).unwrap_or("").to_string();
    let rebased = t.round.as_ref().and_then(|r| r["update"].as_str()) == Some("rebase");
    let tmp = git(&w, &["diff", "--name-only", &format!("{}..HEAD", t.base_head.clone().unwrap_or(from.clone()))]).await?;
    if tmp.lines().any(|l| l.starts_with(".tmp/")) {
        bail!("the commits include .tmp/ files; nothing was pushed");
    }
    let open = open_of(t);
    if let Some(p) = check_replies(&open, &d) {
        bail!("the replies don't hold up ({p}); nothing was pushed");
    }
    if let Err(why) = steps::checks_ok(shared, cfg, t, &head)
        && t.extra.get("checks_override").and_then(Value::as_str) != Some(head.as_str())
    {
        bail!("the witnessed checks don't pass at {} ({why}); nothing was pushed", short(&head));
    }
    let checkout = cfg.repos.get(&t.repo).map(|r| r.checkout.clone()).unwrap_or_default();
    let url = format!("{}/{}.git", std::env::var("WH_GIT_BASE").unwrap_or_else(|_| "https://github.com".into()), t.repo);
    let helper = format!("credential.helper=!'{}' auth git-credential", github::gh_bin().display());
    let remote = git(&checkout, &["-c", "credential.helper=", "-c", &helper, "ls-remote", "--heads", &url, &branch]).await?;
    let rsha = remote.split_whitespace().next().unwrap_or("").to_string();
    if head != rsha {
        if rebased {
            // A rebase rewrites the branch: push it only over the head this
            // round started from, never over someone's newer commits.
            let pushed = t.pushed_head.clone().unwrap_or_default();
            if rsha != pushed {
                bail!("GitHub's {branch} moved to {} since the round started; nothing was pushed", short(&rsha));
            }
            git(
                &w,
                &[
                    "-c",
                    "credential.helper=",
                    "-c",
                    &helper,
                    "push",
                    "--quiet",
                    &format!("--force-with-lease=refs/heads/{branch}:{pushed}"),
                    &url,
                    &format!("HEAD:refs/heads/{branch}"),
                ],
            )
            .await?;
        } else {
            if !rsha.is_empty() {
                let _ = git(&checkout, &["-c", "credential.helper=", "-c", &helper, "fetch", "--no-tags", "--quiet", &url, &rsha]).await;
                if git(&w, &["merge-base", "--is-ancestor", &rsha, "HEAD"]).await.is_err() {
                    bail!("GitHub's {branch} has commits this branch doesn't; nothing was pushed");
                }
            }
            git(&w, &["-c", "credential.helper=", "-c", &helper, "push", "--quiet", &url, &format!("HEAD:refs/heads/{branch}")]).await?;
        }
    }
    t.pushed_head = Some(head.clone());
    // Then the words, in the contract's order: thread replies, the one
    // comment with the answers, the re-requests. A failure after the push
    // says what posted; nothing posts twice.
    let n = t.pr_number.unwrap_or(0);
    let (replies, comment, rerequest) = render(&open, &d, &by(t));
    let mut posted = 0;
    let mut problem: Option<String> = None;
    for (id, body) in &replies {
        let path = format!("repos/{}/pulls/{n}/comments/{id}/replies", t.repo);
        match github::gh(&["api", "-X", "POST", &path, "--input", "-"], Some(json!({ "body": body }).to_string())).await {
            Ok(_) => posted += 1,
            Err(e) => {
                problem = Some(format!("GitHub refused the reply to comment {id} after {posted} posted: {e:#}"));
                break;
            }
        }
    }
    let mut comment_url = None;
    if problem.is_none()
        && let Some(c) = &comment
    {
        match github::gh(&["api", "-X", "POST", &format!("repos/{}/issues/{n}/comments", t.repo), "--input", "-", "--jq", ".html_url"], Some(json!({ "body": c }).to_string()))
            .await
        {
            Ok(u) => comment_url = Some(u.trim().to_string()),
            Err(e) => problem = Some(format!("GitHub refused the answers' comment after {posted} replies posted: {e:#}")),
        }
    }
    if problem.is_none()
        && !rerequest.is_empty()
        && let Err(e) =
            github::gh(&["api", "-X", "POST", &format!("repos/{}/pulls/{n}/requested_reviewers", t.repo), "--input", "-"], Some(json!({ "reviewers": rerequest }).to_string()))
                .await
    {
        problem = Some(format!("GitHub refused the re-requests; the replies posted: {e:#}"));
    }
    let ids: Vec<String> = t.round.as_ref().and_then(|r| serde_json::from_value(r["items"].clone()).ok()).unwrap_or_default();
    t.seen.extend(ids);
    let round = t.rounds;
    {
        let mut core = shared.lock();
        if crate::review::through_link(&replies_path(t)).is_none()
            && let Ok(bytes) = std::fs::read(replies_path(t))
        {
            steps::keep(&mut core, t, format!("Round {round}'s replies"), "replies.json".into(), bytes, &format!("Round {round}"), json!({ "role": "replies", "round": round }));
        }
        super::event(
            &mut core,
            &t.docket,
            "pushed-round",
            format!(
                "Round {round}: pushed {} at {}, {posted} repl{} posted{}{}.",
                branch,
                short(&head),
                if posted == 1 { "y" } else { "ies" },
                if comment_url.is_some() { ", with a comment answering the reviews" } else { "" },
                if rerequest.is_empty() || problem.is_some() { String::new() } else { format!(", and {} asked to review again", rerequest.join(", ")) }
            ),
            json!({ "round": round, "head": head, "replies": posted, "comment": comment_url, "rerequest": rerequest }),
        )?;
    }
    end_round(shared, t).await;
    Ok(problem.map(|p| Attention { kind: "error".into(), text: p }))
}

/// A round's step, while the builder works it.
pub async fn step(shared: &Shared, cfg: &Config, t: &mut Task, bstat: &str, free: bool, idle: Option<i64>) -> Result<Option<Attention>> {
    if !free {
        return Ok(None);
    }
    if !t.outbox.is_empty() {
        let text = format!("{}\n\nThen carry on, and write BUILT when the round is done.", t.outbox.join("\n\n"));
        t.outbox.clear();
        steps::tell_builder(shared, t, &text, "WORKING addressing").await?;
    } else if bstat == "BUILT" {
        built(shared, cfg, t).await?;
    } else if bstat.starts_with("WORKING") && idle.is_some_and(|s| s >= steps::STALLED) {
        return Ok(att("stalled", "the builder went idle without finishing the round; look at its pane"));
    }
    Ok(None)
}

/// What the checker is told on a round: the feedback, and the replies.
pub fn check_note(t: &Task) -> String {
    let n = t.rounds;
    format!(
        "\n\nThis check is for address round {n} on PR #{}: what reviewers and CI said is in .tmp/address/round-{n}/open.json, and the builder's replies in .tmp/address/round-{n}/replies.json. Check the fixes against them too, and that each reply is true to the change it names.",
        t.pr_number.unwrap_or(0)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pr() -> Value {
        json!({ "data": {
            "viewer": { "login": "you" },
            "repository": { "pullRequest": {
                "number": 45, "state": "OPEN", "headRefOid": "abc123", "baseRefName": "main", "baseRefOid": "base1",
                "mergeable": "MERGEABLE", "mergeStateStatus": "BEHIND", "author": { "login": "you" },
                "reviewRequests": { "nodes": [] },
                "reviews": { "nodes": [
                    { "databaseId": 7, "url": "https://github.com/acme/api/pull/45#pullrequestreview-7", "state": "CHANGES_REQUESTED", "body": "The retry cap needs a test.", "submittedAt": "2026-11-17T10:00:00Z", "author": { "login": "dana", "__typename": "User" } },
                    { "databaseId": 8, "url": "https://github.com/acme/api/pull/45#pullrequestreview-8", "state": "COMMENTED", "body": "", "submittedAt": "2026-11-17T10:01:00Z", "author": { "login": "sam", "__typename": "User" } }
                ] },
                "comments": { "nodes": [
                    { "databaseId": 90, "url": "https://github.com/acme/api/pull/45#issuecomment-90", "body": "Does this cover 429s?", "createdAt": "2026-11-17T10:02:00Z", "author": { "login": "sam", "__typename": "User" } },
                    { "databaseId": 91, "url": "https://github.com/acme/api/pull/45#issuecomment-91", "body": "Preview deployed", "createdAt": "2026-11-17T10:03:00Z", "author": { "login": "vercel", "__typename": "Bot" } }
                ] },
                "reviewThreads": { "nodes": [
                    { "isResolved": false, "isOutdated": false, "path": "src/retry.ts", "line": 42, "comments": { "nodes": [
                        { "databaseId": 100, "url": "https://github.com/acme/api/pull/45#discussion_r100", "body": "Jitter can go negative here.", "createdAt": "2026-11-17T10:00:00Z", "author": { "login": "dana", "__typename": "User" } }
                    ] } },
                    { "isResolved": false, "isOutdated": false, "path": "src/retry.ts", "line": 9, "comments": { "nodes": [
                        { "databaseId": 110, "url": "u110", "body": "Rename?", "createdAt": "2026-11-17T09:00:00Z", "author": { "login": "dana", "__typename": "User" } },
                        { "databaseId": 111, "url": "u111", "body": "Kept as is: it matches the module.", "createdAt": "2026-11-17T09:30:00Z", "author": { "login": "you", "__typename": "User" } }
                    ] } },
                    { "isResolved": true, "path": "README.md", "line": 1, "comments": { "nodes": [
                        { "databaseId": 120, "url": "u120", "body": "Typo", "createdAt": "2026-11-17T09:00:00Z", "author": { "login": "dana", "__typename": "User" } }
                    ] } }
                ] },
                "commits": { "nodes": [ { "commit": { "oid": "abc123", "statusCheckRollup": { "state": "FAILURE", "contexts": { "nodes": [
                    { "__typename": "CheckRun", "name": "test", "status": "COMPLETED", "conclusion": "FAILURE", "detailsUrl": "https://github.com/acme/api/actions/runs/5/job/6" },
                    { "__typename": "CheckRun", "name": "lint", "status": "COMPLETED", "conclusion": "SUCCESS", "detailsUrl": "x" }
                ] } } } } ] }
            } }
        } })
    }

    #[test]
    fn every_persons_feedback_comes_home_once_in_order() {
        let f = feedback(&pr());
        let ids: Vec<&str> = f.iter().map(|(id, ..)| id.as_str()).collect();
        // Everyone's, answered or resolved too; not yours, not a bot's.
        assert_eq!(ids, vec!["thread-110", "thread-120", "review-7", "thread-100", "review-8", "comment-90"]);
        let (_, d, body) = &f[2];
        assert_eq!((d["kind"].as_str(), d["state"].as_str(), body.as_str()), (Some("review"), Some("CHANGES_REQUESTED"), "The retry cap needs a test."));
        let (_, d, _) = &f[3];
        assert_eq!((d["path"].as_str(), d["line"].as_i64()), (Some("src/retry.ts"), Some(42)));
    }

    #[test]
    fn reads_what_needs_an_answer_as_pr_reply_does() {
        let o = scan(&pr());
        // The thread you answered last, and the resolved one, aren't open.
        assert_eq!(o.threads.len(), 1);
        assert_eq!(o.threads[0]["comment_id"], 100);
        // An empty review body needs no answer; a bot's comment neither.
        assert_eq!(o.reviews.len(), 1);
        assert_eq!(o.comments.len(), 1);
        assert_eq!(o.rerequest, vec!["dana".to_string(), "sam".to_string()]);
        assert_eq!(o.checks.len(), 1);
        assert_eq!(o.checks[0]["name"], "test");
        assert_eq!(o.base_state.as_deref(), Some("behind"));
        let ids: Vec<String> = o.items().into_iter().map(|(id, _)| id).collect();
        assert_eq!(ids, vec!["t:100:100", "r:7", "c:90", "ci:abc123", "base:base1:behind"]);
        assert_eq!(sum(&o), "a review thread, a review, a comment, a failing check, behind main");
    }

    #[test]
    fn checks_wait_until_every_one_finishes() {
        let mut v = pr();
        v["data"]["repository"]["pullRequest"]["commits"]["nodes"][0]["commit"]["statusCheckRollup"]["contexts"]["nodes"][1]["status"] = json!("IN_PROGRESS");
        assert!(scan(&v).checks.is_empty());
    }

    #[test]
    fn a_later_comment_linking_it_answers_a_review() {
        let mut v = pr();
        v["data"]["repository"]["pullRequest"]["comments"]["nodes"].as_array_mut().unwrap().push(json!({
            "databaseId": 95, "url": "u95", "body": "> **[dana's review](https://github.com/acme/api/pull/45#pullrequestreview-7)**\n\nDone.",
            "createdAt": "2026-11-17T11:00:00Z", "author": { "login": "you", "__typename": "User" }
        }));
        assert!(scan(&v).reviews.is_empty());
    }

    fn good() -> Value {
        json!({
            "replies": [{ "comment_id": 100, "body": "Fixed in 1a2b3c4. `jitter()` now clamps at zero; `jitter_never_negative` covers it." }],
            "answers": [
                { "url": "https://github.com/acme/api/pull/45#pullrequestreview-7", "label": "dana's review", "body": "`retry_caps_at_three` covers the cap." },
                { "url": "https://github.com/acme/api/pull/45#issuecomment-90", "label": "sam's comment", "quote": "Does this cover 429s?", "body": "Yes: a 429 is retried like a 5xx." }
            ],
            "rerequest": ["dana"]
        })
    }

    #[test]
    fn replies_answer_everything_open_in_the_right_voice() {
        let open = scan(&pr()).json();
        assert_eq!(check_replies(&open, &good()), None);
        let mut d = good();
        d["replies"] = json!([]);
        assert!(check_replies(&open, &d).unwrap().contains("has no reply"));
        let mut d = good();
        d["answers"].as_array_mut().unwrap().pop();
        assert!(check_replies(&open, &d).unwrap().contains("issuecomment-90"));
        let mut d = good();
        d["replies"][0]["body"] = json!("I clamped it at zero.");
        assert!(check_replies(&open, &d).unwrap().contains("first person"));
        let mut d = good();
        d["replies"][0]["comment_id"] = json!(999);
        assert!(check_replies(&open, &d).unwrap().contains("999"));
        let mut d = good();
        d["rerequest"] = json!(["you"]);
        assert!(check_replies(&open, &d).unwrap().contains("own author"));
    }

    #[test]
    fn renders_as_the_contract_posts() {
        let open = scan(&pr()).json();
        let (replies, comment, rerequest) = render(&open, &good(), "Claude Code (Opus 5.5)");
        assert_eq!(replies.len(), 1);
        assert_eq!(replies[0].0, "100");
        assert!(replies[0].1.starts_with("Fixed in 1a2b3c4."));
        assert!(replies[0].1.ends_with("<div align=\"right\"><sub><i>Claude Code (Opus 5.5) on behalf of @you</i></sub></div>"));
        let c = comment.unwrap();
        assert!(c.starts_with("> **[dana's review](https://github.com/acme/api/pull/45#pullrequestreview-7)**\n\n"));
        assert!(c.contains("> **[sam's comment](https://github.com/acme/api/pull/45#issuecomment-90)**\n> Does this cover 429s?"));
        assert_eq!(rerequest, vec!["dana".to_string()]);
    }

    #[test]
    fn names_only_commit_like_words() {
        assert_eq!(shas("Fixed in 1a2b3c4, see deadbeef and 1234567.\n```\nabc1234 in code\n```"), vec!["1a2b3c4".to_string()]);
    }
}
