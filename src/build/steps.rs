//! A Build's steps: start, prep, the builder and checker, the gates, opening
//! the PR, following it, and discarding. The caller holds the docket's lock.

use anyhow::{Result, anyhow, bail};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::time::Duration;

use super::agents::{self as wall, Spec};
use super::config::Config;
use super::decide::{self, Reply};
use super::{Attention, Dec, Task, plan};
use crate::hub::{Shared, Who};
use crate::review::panes::{self, Place};
use crate::review::rounds::{fetch, git, set_status, status_in};
use crate::review::{agentcli, github};

const PLAN_PROMPT: &str = "Read .tmp/ground-rules.md, then .tmp/tasks/plan.md, and do the task.";
/// An agent idle this long with its work unfinished has stalled.
const STALLED: i64 = 600;

fn now() -> String {
    crate::time::now()
}

fn short(s: &str) -> String {
    s.chars().take(7).collect()
}

fn epoch() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn att(kind: &str, text: impl Into<String>) -> Option<Attention> {
    Some(Attention { kind: kind.into(), text: text.into() })
}

fn wt(t: &Task) -> PathBuf {
    PathBuf::from(t.worktree.clone().unwrap_or_default())
}

fn slug(s: &str, max: usize) -> String {
    let mut out = String::new();
    for w in s.to_lowercase().split(|c: char| !c.is_ascii_alphanumeric()).filter(|w| !w.is_empty()) {
        if out.len() + w.len() + 1 > max {
            break;
        }
        if !out.is_empty() {
            out.push('-');
        }
        out.push_str(w);
    }
    out
}

// ---- Start ----

/// Start a Build: from a docket with an intent (`D-12`), a GitHub issue
/// (`owner/name#45` or its URL), or a Linear issue (`ENG-142`).
pub async fn start_from(shared: &Shared, spec: &str, repo: Option<&str>) -> Result<Value> {
    let cfg = super::config(shared)?;
    let spec = spec.trim();
    let pick_repo = |want: Option<&str>| -> Result<String> {
        match want {
            Some(r) if cfg.repos.contains_key(r) => Ok(r.to_string()),
            Some(r) => bail!("{r} isn't one of Build's repos ({})", cfg.repos.keys().cloned().collect::<Vec<_>>().join(", ")),
            None if cfg.repos.len() == 1 => Ok(cfg.repos.keys().next().cloned().unwrap_or_default()),
            None => bail!("which repo? --repo {}", cfg.repos.keys().cloned().collect::<Vec<_>>().join(" | ")),
        }
    };
    let gh_issue = spec.strip_prefix("https://github.com/").and_then(|r| {
        let p: Vec<&str> = r.split('/').collect();
        (p.len() >= 4 && p[2] == "issues").then(|| (format!("{}/{}", p[0], p[1]), p[3].parse::<i64>().ok()))
    });
    let gh_issue = gh_issue.or_else(|| crate::review::pr::parse_key(spec).map(|(r, n)| (r, Some(n))));
    let mut t = if let Some(d) = spec.strip_prefix("D-").map(|_| spec.to_string()) {
        let core = shared.lock();
        let st = core.store.state(&d)?.ok_or_else(|| anyhow!("no docket {d}"))?;
        if st.intent.as_ref().is_none_or(|i| i.body.trim().is_empty()) {
            bail!("{d} has no intent yet: write what it's for first (wh intent {d} - < intent.md)");
        }
        Task { docket: d.clone(), key: d.clone(), title: st.title.clone(), source: "docket".into(), trigger: "manual".into(), repo: pick_repo(repo)?, ..Default::default() }
    } else if let Some((r, Some(n))) = gh_issue {
        let v: Value = serde_json::from_str(&github::gh(&["issue", "view", &n.to_string(), "--repo", &r, "--json", "number,title,body,url,state"], None).await?)?;
        if v["state"] != "OPEN" {
            bail!("{r}#{n} is {}", v["state"].as_str().unwrap_or("?"));
        }
        let repo = pick_repo(Some(repo.unwrap_or(&r)))?;
        let key = format!("{r}#{n}");
        let title = v["title"].as_str().unwrap_or("").to_string();
        let mut core = shared.lock();
        let d = core.open(&Who::Owner, &format!("Build {}: {title}", key.rsplit('/').next().unwrap_or(&key)), json!({ "protocol": "build", "title_source": "build" }))?;
        core.append(&d, "link", &Who::Owner, None, json!({ "kind": "github-issue", "url": v["url"], "key": key }))?;
        let body = v["body"].as_str().unwrap_or("").trim();
        core.append(&d, "intent", &Who::Hub, Some(if body.is_empty() { title.clone() } else { body.to_string() }), json!({ "source": "github", "key": key }))?;
        Task { docket: d, key, title, url: v["url"].as_str().map(str::to_string), source: "github".into(), trigger: "manual".into(), repo, ..Default::default() }
    } else if crate::linear::is_key(spec) {
        let i = crate::linear::issue(spec).await?;
        let repo = pick_repo(repo)?;
        let mut core = shared.lock();
        let d = core.open(&Who::Owner, &format!("Build {}: {}", i.identifier, i.title), json!({ "protocol": "build", "title_source": "build" }))?;
        crate::linear::link(&mut core, &d, &i, &Who::Owner)?;
        crate::linear::apply(&mut core, &d, &i)?;
        let mut t = Task { docket: d, key: i.identifier.clone(), title: i.title.clone(), url: Some(i.url.clone()), source: "linear".into(), trigger: "manual".into(), repo, ..Default::default() };
        if let Some(b) = i.branch.clone().filter(|b| !b.is_empty()) {
            t.branch = Some(b);
        }
        t
    } else {
        bail!("start a Build from a docket (D-12), a GitHub issue (owner/name#45), or a Linear issue (ENG-142)");
    };
    let mut core = shared.lock();
    if let Some(old) = super::get(&core, &t.docket)?
        && !old.finished()
    {
        bail!("{} already has a Build ({})", t.docket, old.state);
    }
    t.state = "queued".into();
    t.created_at = Some(now());
    super::save(&mut core, &mut t)?;
    super::event(&mut core, &t.docket, "queued", format!("Build queued: {} into {}.", t.key, t.repo), json!({ "repo": t.repo }))?;
    core.build.check_now.notify_one();
    Ok(json!({ "docket": t.docket, "key": t.key, "repo": t.repo, "state": t.state }))
}

// ---- Prep ----

/// A worktree on the docket's branch from `origin/<base>`, and the context.
pub async fn prep(shared: &Shared, cfg: &Config, t: &mut Task) -> Result<()> {
    let repo = cfg.repos.get(&t.repo).ok_or_else(|| anyhow!("{} isn't one of Build's repos", t.repo))?.clone();
    let checkout = repo.checkout.clone();
    let base = t.base.clone().or(repo.base.clone()).unwrap_or_else(|| "main".into());
    fetch(&checkout, &t.repo, &[format!("+refs/heads/{base}:refs/remotes/origin/{base}")]).await?;
    let branch = t.branch.clone().unwrap_or_else(|| {
        let k = if t.source == "github" { t.key.rsplit('#').next().unwrap_or(&t.key).to_string() } else { t.key.to_lowercase() };
        format!("{}{}-{}", cfg.branch_prefix, k, slug(&t.title, 40)).trim_end_matches('-').to_string()
    });
    let url = format!("{}/{}.git", std::env::var("WH_GIT_BASE").unwrap_or_else(|_| "https://github.com".into()), t.repo);
    let remote = git(&checkout, &["-c", "credential.helper=", "-c", &format!("credential.helper=!'{}' auth git-credential", github::gh_bin().display()), "ls-remote", "--heads", &url, &branch]).await.unwrap_or_default();
    if !remote.trim().is_empty() && t.pushed_head.is_none() {
        bail!("the branch {branch} is already on GitHub; Build won't build over someone's branch. Delete it, or pick another branch");
    }
    let w = super::data_dir().join("worktrees").join(&t.docket);
    if !w.join(".git").exists() {
        if let Some(p) = w.parent() {
            std::fs::create_dir_all(p)?;
        }
        let local = git(&checkout, &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{branch}")]).await.is_ok();
        let ws = w.to_string_lossy().to_string();
        if local {
            git(&checkout, &["worktree", "add", "--quiet", &ws, &branch]).await?;
        } else {
            git(&checkout, &["worktree", "add", "--quiet", "--no-track", "-b", &branch, &ws, &format!("origin/{base}")]).await?;
        }
    }
    let base_head = git(&checkout, &["rev-parse", &format!("origin/{base}")]).await?.trim().to_string();
    for d in ["context", "decisions", "check", "tasks", "gh-empty"] {
        std::fs::create_dir_all(w.join(".tmp").join(d))?;
    }
    let (intent, issue) = {
        let core = shared.lock();
        let st = core.store.state(&t.docket)?;
        let intent = st.as_ref().and_then(|s| s.intent.as_ref()).map(|i| i.body.clone()).unwrap_or_default();
        let issue = st.as_ref().and_then(|s| s.linear.as_ref()).map(|l| format!("# {}: {}\n\n{}\n\n{}", l.key, l.title.clone().unwrap_or_default(), l.url.clone().unwrap_or_default(), l.description.clone().unwrap_or_default()));
        (intent, issue)
    };
    crate::review::write_atomic(&w.join(".tmp/context/intent.md"), &format!("# {}: {}\n\n{intent}\n", t.key, t.title))?;
    if let Some(i) = issue {
        crate::review::write_atomic(&w.join(".tmp/context/issue.md"), &i)?;
    } else if t.source == "github" {
        let n = t.key.rsplit('#').next().unwrap_or("");
        let repo_of = t.key.rsplit_once('#').map(|x| x.0).unwrap_or(&t.repo);
        if let Ok(v) = github::gh(&["issue", "view", n, "--repo", repo_of, "--json", "number,title,body,url,labels,comments,author"], None).await
            && let Ok(j) = serde_json::from_str::<Value>(&v)
        {
            let mut md = format!("# {}: {}\n\n{}\n\n{}\n", t.key, j["title"].as_str().unwrap_or(""), j["url"].as_str().unwrap_or(""), j["body"].as_str().unwrap_or(""));
            for c in j["comments"].as_array().into_iter().flatten() {
                md.push_str(&format!("\n### {}, {}\n\n{}\n", c["author"]["login"].as_str().unwrap_or("?"), c["createdAt"].as_str().unwrap_or(""), c["body"].as_str().unwrap_or("")));
            }
            crate::review::write_atomic(&w.join(".tmp/context/issue.md"), &md)?;
            crate::review::write_atomic(&w.join(".tmp/context/issue.json"), &v)?;
        }
    }
    if !repo.notes.trim().is_empty() {
        crate::review::write_atomic(&w.join(".tmp/context/repo-notes.md"), &repo.notes)?;
    }
    t.base = Some(base);
    t.branch = Some(branch);
    t.base_head = Some(base_head);
    t.worktree = Some(w.display().to_string());
    Ok(())
}

fn brief(name: &str) -> String {
    let own = crate::review::config::dir().join("briefs/build").join(name);
    if let Ok(s) = std::fs::read_to_string(own) {
        return s;
    }
    match name {
        "ground-rules.md" => include_str!("../../briefs/build/ground-rules.md"),
        "plan.md" => include_str!("../../briefs/build/plan.md"),
        "build.md" => include_str!("../../briefs/build/build.md"),
        _ => include_str!("../../briefs/build/check.md"),
    }
    .to_string()
}

fn vars(cfg: &Config, t: &Task, login: &str) -> crate::review::briefs::Vars {
    let mut v = crate::review::briefs::Vars::new();
    let repo = cfg.repos.get(&t.repo);
    v.insert("KEY", t.key.clone());
    v.insert("TITLE", t.title.clone());
    v.insert("DOCKET", t.docket.clone());
    v.insert("SOURCE_LINE", t.url.as_deref().map(|u| format!(", from {u}")).unwrap_or_default());
    v.insert("LOGIN", login.to_string());
    v.insert("REPO", t.repo.clone());
    v.insert("BRANCH", t.branch.clone().unwrap_or_default());
    v.insert("BASE", t.base.clone().unwrap_or_else(|| "main".into()));
    v.insert("PORT", t.port.unwrap_or(4310).to_string());
    v.insert("MAX_DECISIONS", cfg.max_decisions.to_string());
    v.insert("CLOSES", closes(t).unwrap_or_else(|| "the issue link".into()));
    v.insert("PLAN_FORMAT", cfg.guide("plan_format").unwrap_or_else(|| include_str!("../../guides/plan-format.md").to_string()));
    v.insert("PR_STYLE", cfg.guide("pr_style").unwrap_or_else(|| include_str!("../../guides/pr-style.md").to_string()));
    let mode = repo.and_then(|r| r.plan.clone()).unwrap_or_else(|| "full".into());
    v.insert(
        "PLAN_MODE",
        match mode.as_str() {
            "summary" => "This repo is public and the issue may not be, so the plan goes on the PR in summary: write Intent, Acceptance and Out of scope in your own words, without quoting the issue, and leave Notes for things that are safe to publish.",
            "off" => "This repo doesn't attach plans to PRs, but write the plan all the same: you and the checker work from it.",
            _ => "The Intent, Acceptance, Out of scope and Notes sections go on the PR as they are, for its reviewers.",
        }
        .into(),
    );
    for (slot, var, title) in [("plan", "PLAN_GUIDE", "planning"), ("build", "BUILD_GUIDE", "building"), ("check", "CHECK_GUIDE", "checking")] {
        v.insert(var, cfg.guide(slot).map(|g| format!("## More on {title}\n\n{g}\n")).unwrap_or_default());
    }
    v.insert("REPO_NOTES", repo.map(|r| r.notes.trim()).filter(|n| !n.is_empty()).map(|n| format!("## Notes on {}\n\n{n}\n", t.repo)).unwrap_or_default());
    v
}

fn closes(t: &Task) -> Option<String> {
    match t.source.as_str() {
        "linear" => Some(format!("Closes {}", t.key)),
        "github" => Some(if t.key.rsplit_once('#').map(|x| x.0) == Some(t.repo.as_str()) { format!("Closes #{}", t.key.rsplit('#').next().unwrap_or("")) } else { format!("Closes {}", t.key) }),
        _ => None,
    }
}

fn app_of(agent: &str) -> &'static str {
    if agent == "claude" { "claude-code" } else { "codex" }
}

/// Open a role's pane behind the wall and hand it `prompt`.
async fn open_role(shared: &Shared, cfg: &Config, t: &mut Task, role: &str, prompt: &str, place: Place<'_>) -> Result<()> {
    let w = wt(t);
    let r = if role == "builder" { &cfg.builder } else { &cfg.checker };
    let view = json!({ "key": t.key, "title": t.title, "repo": t.repo, "source": t.source });
    let effort = cfg.effort(if role == "builder" { "build" } else { "check" }, &view)?;
    let resume = if role == "builder" { t.builder.session.clone() } else { t.checker.session.clone() };
    let checkout = cfg.repos.get(&t.repo).map(|x| x.checkout.clone()).unwrap_or_default();
    let title = format!("wheelhouse build {} {role}", t.docket);
    let l = wall::argv(cfg, &Spec { agent: &r.agent, extra_args: &r.extra_args, resume: resume.as_deref(), worktree: &w, checkout: &checkout, effort: effort.as_deref(), title: &title });
    let label = t.label(role);
    shared.lock().build.panes.insert(label.clone(), (t.docket.clone(), role.to_string()));
    if let (Some(b), Some(s)) = (panes::find_in(shared, None, &super::session_name(), &label), panes::session_named(shared, &super::session_name())) {
        panes::close(&s, &b).await;
    }
    let since = epoch();
    let created = panes::open_in(shared, &super::session_name(), &label, &w.to_string_lossy(), &l.argv, place).await?;
    {
        let a = crate::record::fold::AgentRef { app: app_of(&r.agent).into(), host: "local".into(), block: created.block_id.clone(), rex_session: super::session_name(), pane: label.clone(), cwd: Some(crate::paths::tilde(&w.to_string_lossy())) };
        let mut core = shared.lock();
        crate::agents::adopt(&mut core, &t.docket, &a, Some(prompt));
        let _ = core.append(&t.docket, "claim", &Who::Hub, None, json!({ "source": "wheelhouse", "role": format!("Build {role}"), "agent": a }));
    }
    let settled = panes::settled(shared, &created.block_id, Duration::from_secs(180)).await;
    let why = match settled {
        Ok(s) if s == "blocked" => Some("it's waiting on a prompt (a trust or permission question?)".to_string()),
        Ok(_) => None,
        Err(e) => Some(format!("{e:#}")),
    };
    if let Some(why) = why {
        let tail = panes::tail(&created.session_id, &created.block_id).await;
        panes::close(&created.session_id, &created.block_id).await;
        bail!("the {role} ({}) didn't start: {why}\n{}", r.agent, tail.lines().rev().take(6).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n"));
    }
    let session = if r.agent == "codex" { agentcli::codex_thread(&w, since).or(l.session) } else { l.session };
    let a = super::Agent { agent: r.agent.clone(), session, pane: Some(label), block: Some(created.block_id.clone()), effort };
    if role == "builder" {
        t.builder = a;
    } else {
        t.checker = a;
    }
    t.session_id = Some(created.session_id.clone());
    panes::deliver(shared, &created.session_id, &created.block_id, prompt).await?;
    Ok(())
}

/// Prep, write the briefs, and start the builder on the plan.
pub async fn start(shared: &Shared, cfg: &Config, t: &mut Task) -> Result<()> {
    prep(shared, cfg, t).await?;
    let taken: Vec<i64> = super::all(&shared.lock())?.iter().filter(|x| !x.finished() && x.docket != t.docket).filter_map(|x| x.port).collect();
    t.port = (4310..4400).find(|p| !taken.contains(p));
    let login = github::login().await.unwrap_or_else(|_| "you".into());
    let w = wt(t);
    let v = vars(cfg, t, &login);
    let render = |n: &str| crate::review::briefs::render(&brief(n), &v);
    crate::review::write_atomic(&w.join(".tmp/ground-rules.md"), &render("ground-rules.md"))?;
    for n in ["plan.md", "build.md", "check.md"] {
        crate::review::write_atomic(&w.join(".tmp/tasks").join(n), &render(n))?;
    }
    set_status(&w, "WORKING planning")?;
    open_role(shared, cfg, t, "builder", PLAN_PROMPT, Place::Tab).await?;
    t.state = "active".into();
    t.phase = Some("planning".into());
    t.start_error = None;
    t.start_failed_epoch = None;
    let mut core = shared.lock();
    super::save(&mut core, t)?;
    super::event(&mut core, &t.docket, "started", format!("The builder is planning {} on {}, from {}.", t.key, t.branch.clone().unwrap_or_default(), t.base.clone().unwrap_or_default()), json!({ "branch": t.branch, "base": t.base }))?;
    Ok(())
}

// ---- Asks ----

/// Ask you something on the docket; a re-ask replaces the last one.
fn send_ask(shared: &Shared, t: &mut Task, what: &str, subject: &str, body: &str, options: &[&str]) -> Result<()> {
    let mut core = shared.lock();
    let version = t.asks.get(what).map_or(0, |a| a.version) + 1;
    if let Some(old) = t.asks.get(what) {
        let _ = core.store.set_message_status(old.message, "done", Some("asked again"));
    }
    let opts: Vec<String> = options.iter().map(|s| s.to_string()).collect();
    let m = crate::messages::ask(&mut core, &t.docket, subject, body, opts.clone(), json!({ "ask": what, "version": version, "protocol": "build" }))?;
    t.asks.insert(what.to_string(), super::Ask { message: m.id, version, options: opts });
    Ok(())
}

fn close_asks(shared: &Shared, t: &mut Task) {
    let mut core = shared.lock();
    for a in t.asks.values() {
        let _ = core.store.set_message_status(a.message, "done", Some("the Build moved on"));
    }
    drop(core);
    t.asks.clear();
}

/// Send the builder a prompt now if it's free, else keep it for later.
async fn tell_builder(shared: &Shared, t: &mut Task, text: &str, status: &str) -> Result<()> {
    let w = wt(t);
    if builder_free(shared, t) {
        set_status(&w, status)?;
        let (s, b) = (t.session_id.clone().unwrap_or_default(), t.builder.block.clone().unwrap_or_default());
        panes::deliver(shared, &s, &b, text).await
    } else {
        t.outbox.push(text.to_string());
        Ok(())
    }
}

fn pane(shared: &Shared, a: &super::Agent) -> Option<(String, Option<String>)> {
    a.block.as_deref().and_then(|b| panes::state(shared, b))
}

fn builder_free(shared: &Shared, t: &Task) -> bool {
    pane(shared, &t.builder).is_some_and(|(s, _)| s != "working" && s != "blocked")
}

fn idle_for(at: Option<&str>) -> Option<i64> {
    let t = time::OffsetDateTime::parse(at?, &time::format_description::well_known::Rfc3339).ok()?;
    Some((time::OffsetDateTime::now_utc() - t).whole_seconds())
}

// ---- The step ----

/// One step of an active Build. Returns what needs you beyond its asks.
pub async fn step(shared: &Shared, cfg: &Config, t: &mut Task) -> Result<Option<Attention>> {
    // Your answers first.
    let answers: Vec<(String, Reply)> = {
        let core = shared.lock();
        t.asks.iter().filter_map(|(what, a)| crate::messages::answer_to(&core, a.message).map(|m| (what.clone(), decide::read(&a.options, m.choice.as_deref(), &m.body)))).collect()
    };
    for (what, r) in answers {
        t.asks.remove(&what);
        answer(shared, cfg, t, &what, r).await?;
        if t.state != "active" {
            return Ok(None);
        }
    }
    let w = wt(t);
    decisions(shared, t)?;
    let bstat = status_in(&w, "status.txt");
    let bpane = pane(shared, &t.builder);
    let free = bpane.as_ref().is_some_and(|(s, _)| s != "working" && s != "blocked");
    let phase = t.phase().to_string();
    if matches!(phase.as_str(), "planning" | "building") {
        if bpane.is_none() && bstat.starts_with("WORKING") {
            return Ok(att("pane-closed", "the builder's pane closed mid-task"));
        }
        if let Some(why) = bstat.strip_prefix("BLOCKED") {
            return Ok(att("blocked", format!("the builder stopped: {}", why.trim_start_matches(':').trim())));
        }
        if bpane.as_ref().is_some_and(|(s, _)| s == "blocked") {
            return Ok(att("blocked", "the builder is waiting on a prompt in its pane"));
        }
    }
    match phase.as_str() {
        "planning" if free => {
            if !t.outbox.is_empty() && (bstat == "PLAN_READY" || bstat.starts_with("WORKING")) {
                let text = format!("Before the plan goes out:\n\n{}\n\nUpdate .tmp/plan.md if these change it, then write PLAN_READY again.", t.outbox.join("\n\n"));
                t.outbox.clear();
                tell_builder(shared, t, &text, "WORKING planning").await?;
            } else if bstat == "PLAN_READY" {
                plan_ready(shared, cfg, t).await?;
            } else if let Some(why) = bstat.strip_prefix("NOT_READY") {
                let why = why.trim_start_matches(':').trim().to_string();
                let more = std::fs::read_to_string(w.join(".tmp/not-ready.md")).unwrap_or_else(|_| why.clone());
                let body = format!("{more}\n\nProceed has the builder plan on its stated assumptions (add anything it should know). Drop stops the Build.");
                send_ask(shared, t, "not_ready", &format!("{} · not ready: {why}", t.key), &body, &["Proceed", "Drop"])?;
                t.phase = Some("not_ready".into());
            } else if bstat.starts_with("WORKING") && idle_for(bpane.as_ref().and_then(|p| p.1.as_deref())).is_some_and(|s| s >= STALLED) {
                return Ok(att("stalled", "the builder went idle without finishing its plan; look at its pane"));
            }
        }
        "building" if free => {
            if !t.outbox.is_empty() {
                let text = format!("{}\n\nThen carry on, and write BUILT when it's done.", t.outbox.join("\n\n"));
                t.outbox.clear();
                tell_builder(shared, t, &text, "WORKING building").await?;
            } else if bstat == "BUILT" {
                built(shared, cfg, t).await?;
            } else if bstat.starts_with("WORKING") && idle_for(bpane.as_ref().and_then(|p| p.1.as_deref())).is_some_and(|s| s >= STALLED) {
                return Ok(att("stalled", "the builder went idle without finishing; look at its pane"));
            }
        }
        "checking" => {
            let cstat = status_in(&w, "checker-status.txt");
            let cpane = pane(shared, &t.checker);
            if let Some(why) = cstat.strip_prefix("BLOCKED") {
                return Ok(att("blocked", format!("the checker stopped: {}", why.trim_start_matches(':').trim())));
            }
            if cstat == "CHECKED" && cpane.as_ref().is_none_or(|(s, _)| s != "working") {
                after_check(shared, cfg, t).await?;
            } else if cpane.is_none() && cstat.starts_with("CHECKING") {
                return Ok(att("pane-closed", "the checker's pane closed before it finished"));
            }
        }
        "opening" => {
            if let Err(e) = open_pr(shared, cfg, t).await {
                let e = format!("{e:#}");
                if t.phase() == "opening" {
                    t.phase = Some("open_gate".into());
                    ask_open(shared, t, Some(&e))?;
                }
                return Ok(att("error", format!("opening the PR failed: {e}")));
            }
        }
        _ => {}
    }
    Ok(None)
}

/// New decision files: each valid one becomes an ask; a bad one is told to
/// the builder once.
fn decisions(shared: &Shared, t: &mut Task) -> Result<()> {
    let dir = wt(t).join(".tmp/decisions");
    let mut files: Vec<(u32, String)> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let n = e.file_name().to_string_lossy().to_string();
            let id = n.strip_suffix(".json").filter(|x| !x.contains('.'))?.to_string();
            id.strip_prefix('D')?.parse::<u32>().ok().map(|k| (k, id))
        })
        .collect();
    files.sort();
    for (_, id) in files {
        if t.decisions.contains_key(&id) {
            continue;
        }
        let Ok(v) = std::fs::read_to_string(dir.join(format!("{id}.json"))).map(|s| serde_json::from_str::<Value>(&s)) else { continue };
        let parsed = v.map_err(|e| e.to_string()).and_then(|v| decide::parse(&id, &v));
        match parsed {
            Ok(d) => {
                let (subject, body, options) = decide::ask(&t.key, &d);
                let opts: Vec<&str> = options.iter().map(String::as_str).collect();
                send_ask(shared, t, &id, &subject, &body, &opts)?;
                t.decisions.insert(id.clone(), Dec { question: d.question, options, recommend: d.recommend, blocking: d.blocking, answer: None });
            }
            Err(why) => {
                let note = format!(".tmp/decisions/{id}.json isn't valid: {why}. Fix the file; Wheelhouse picks it up as soon as it is.");
                if !t.told.contains(&note) {
                    t.told.push(note.clone());
                    t.outbox.push(note);
                }
            }
        }
    }
    Ok(())
}

fn write_answer(t: &Task, id: &str, answer: &Value) -> Result<()> {
    crate::review::write_atomic(&wt(t).join(format!(".tmp/decisions/{id}.answer.json")), &serde_json::to_string_pretty(answer)?)
}

fn decision_of(t: &Task, id: &str) -> Option<decide::Decision> {
    let d = t.decisions.get(id)?;
    Some(decide::Decision {
        id: id.to_string(),
        question: d.question.clone(),
        why: None,
        options: d.options.iter().map(|l| decide::Opt { label: l.clone(), detail: None }).collect(),
        recommend: d.recommend,
        blocking: d.blocking,
        meanwhile: None,
    })
}

/// Apply your answer to an ask.
async fn answer(shared: &Shared, cfg: &Config, t: &mut Task, what: &str, r: Reply) -> Result<()> {
    let words = |r: &Reply| if r.text.trim().is_empty() { String::new() } else { format!("\n\nFrom {}: \"{}\"", "you", r.text.trim()) };
    if what.starts_with('D') {
        let Some(d) = decision_of(t, what) else { return Ok(()) };
        let a = decide::answer_file(&d, &r, false, &now());
        write_answer(t, what, &a)?;
        let label = a["label"].as_str().unwrap_or("").to_string();
        if let Some(x) = t.decisions.get_mut(what) {
            x.answer = Some(a);
        }
        if !matches!(t.phase(), "plan_gate" | "not_ready") {
            let note = if r.text.trim().is_empty() { String::new() } else { format!(", with this note: \"{}\"", r.text.trim()) };
            let msg = format!("{what} ({}): {label}{note}. Its answer is in .tmp/decisions/{what}.answer.json.", d.question);
            if status_in(&wt(t), "status.txt") == format!("DECISION {what}") {
                let phase = t.phase().to_string();
                tell_builder(shared, t, &format!("Your decision has an answer:\n\n{msg}\n\nCarry on, and write {} when it's done.", if phase == "planning" { "PLAN_READY" } else { "BUILT" }), &format!("WORKING {phase}")).await?;
            } else {
                t.outbox.push(msg);
            }
        }
        return Ok(());
    }
    match (what, r.choice) {
        ("not_ready", Some(1)) => discard(shared, t, "you dropped it: not ready").await?,
        ("not_ready", _) => {
            t.phase = Some("planning".into());
            tell_builder(shared, t, &format!("Proceed: plan on the assumptions you stated, and mark each one in the plan's Notes.{}\n\nWrite the plan as .tmp/tasks/plan.md says, then write PLAN_READY.", words(&r)), "WORKING planning").await?;
        }
        ("plan", Some(0)) => approve_plan(shared, t, &r.text, false).await?,
        ("plan", Some(2)) => discard(shared, t, "you discarded the plan").await?,
        ("plan", _) => {
            t.phase = Some("planning".into());
            let said = if r.text.trim().is_empty() { "No more was said; look again at what's weakest.".to_string() } else { format!("The answer was: \"{}\"", r.text.trim()) };
            tell_builder(shared, t, &format!("Revise the plan. {said}\n\nUpdate .tmp/plan.md (and add decisions if it raises any), then write PLAN_READY again."), "WORKING planning").await?;
        }
        ("preview", Some(0)) => {
            t.previewed_head = t.checked_head.clone();
            ask_open(shared, t, None)?;
            t.phase = Some("open_gate".into());
            gate_open(cfg, t);
        }
        ("preview", _) => {
            t.phase = Some("building".into());
            tell_builder(shared, t, &format!("The preview needs work: {}\n\nFix it, commit, and write BUILT. The checker checks it again.", r.text.trim()), "WORKING building").await?;
        }
        ("open", Some(0)) => {
            let mut core = shared.lock();
            core.append(&t.docket, "approval", &Who::Owner, None, json!({ "what": "open-pr", "gate": "open", "head": t.checked_head, "branch": t.branch }))?;
            drop(core);
            t.approved_head = t.checked_head.clone();
            t.phase = Some("opening".into());
        }
        ("open", Some(2)) => discard(shared, t, "you discarded it before it opened").await?,
        ("open", _) => {
            t.phase = Some("building".into());
            tell_builder(shared, t, &format!("Before the PR opens, this needs changes: {}\n\nMake them, commit, update .tmp/pr.json if the description changes, and write BUILT. The checker checks it again.", r.text.trim()), "WORKING building").await?;
        }
        _ => {}
    }
    Ok(())
}

/// The plan is approved: the decisions stand, the intent takes the plan's
/// spec, and the builder builds.
async fn approve_plan(shared: &Shared, t: &mut Task, note: &str, auto: bool) -> Result<()> {
    let ids: Vec<String> = t.decisions.iter().filter(|(_, d)| d.answer.is_none()).map(|(k, _)| k.clone()).collect();
    for id in ids {
        if let Some(d) = decision_of(t, &id) {
            let a = decide::answer_file(&d, &Reply { choice: Some(d.recommend), text: String::new() }, true, &now());
            write_answer(t, &id, &a)?;
            let mut core = shared.lock();
            core.append(&t.docket, "decision", &Who::Hub, Some(d.options[d.recommend].label.clone()), json!({ "id": id, "stood": true, "label": d.options[d.recommend].label }))?;
            if let Some(ask) = t.asks.remove(&id) {
                let _ = core.store.set_message_status(ask.message, "done", Some("the recommendation stood"));
            }
            drop(core);
            if let Some(x) = t.decisions.get_mut(&id) {
                x.answer = Some(a);
            }
        }
    }
    let p = plan::parse(t.plan.as_deref().unwrap_or(""));
    {
        let mut core = shared.lock();
        let who = if auto { Who::Hub } else { Who::Owner };
        let s = core.append(&t.docket, "intent", &who, Some(plan::intent(&p)), json!({ "source": "plan", "version": t.plan_version }))?;
        core.append(&t.docket, "approval", &who, if note.trim().is_empty() { None } else { Some(note.trim().to_string()) }, json!({ "what": "plan", "gate": "plan", "version": t.plan_version, "intent_seq": s.seq, "auto": auto }))?;
    }
    let mut msg = String::from("The plan is approved.\n\nThe decisions, as they stand (each answer is also in .tmp/decisions/):\n");
    for (id, d) in &t.decisions {
        let a = d.answer.as_ref();
        let label = a.and_then(|a| a["label"].as_str()).unwrap_or("?");
        let text = a.and_then(|a| a["text"].as_str()).filter(|x| !x.is_empty()).map(|x| format!(", with this note: \"{x}\"")).unwrap_or_default();
        msg.push_str(&format!("- {id} ({}): {label}{text}\n", d.question));
    }
    if t.decisions.is_empty() {
        msg = "The plan is approved.\n".into();
    }
    if !note.trim().is_empty() {
        msg.push_str(&format!("\nFrom you: \"{}\"\n", note.trim()));
    }
    for o in t.outbox.drain(..) {
        msg.push_str(&format!("\n{o}\n"));
    }
    msg.push_str("\nNow read .tmp/tasks/build.md and do it.");
    t.phase = Some("building".into());
    tell_builder(shared, t, &msg, "WORKING building").await
}

async fn plan_ready(shared: &Shared, cfg: &Config, t: &mut Task) -> Result<()> {
    let md = std::fs::read_to_string(wt(t).join(".tmp/plan.md")).unwrap_or_default();
    let p = plan::parse(&md);
    let problems = plan::problems(&p);
    if !problems.is_empty() {
        let text = format!(".tmp/plan.md isn't ready: {}. Fix it as .tmp/tasks/plan.md says, then write PLAN_READY.", problems.join("; "));
        return tell_builder(shared, t, &text, "WORKING planning").await;
    }
    t.plan_version += 1;
    t.plan = Some(md.clone());
    {
        let mut core = shared.lock();
        core.append(&t.docket, "artifact", &Who::Hub, Some(md.clone()), json!({ "name": "plan", "version": t.plan_version }))?;
    }
    if cfg.gate("plan") == "auto" {
        return approve_plan(shared, t, "", true).await;
    }
    let open: Vec<String> = t.decisions.iter().filter(|(_, d)| d.answer.is_none()).map(|(id, d)| format!("- {id}: {} (recommended: {})", d.question, d.options.get(d.recommend).cloned().unwrap_or_default())).collect();
    let mut body = String::new();
    for (name, text) in &p.sections {
        if name.eq_ignore_ascii_case("Decisions") {
            continue;
        }
        body.push_str(&format!("{name}:\n{}\n\n", text.trim()));
    }
    if !open.is_empty() {
        body.push_str(&format!("Open decisions (each is its own message). Approve accepts the recommendation for any you haven't answered:\n{}\n\n", open.join("\n")));
    }
    body.push_str("Approve, Revise (say what to change), or Discard. Approving makes this the docket's intent.");
    let subject = format!(
        "{} · plan · {} acceptance item{}, {} decision{} open{}",
        t.key,
        p.acceptance().len(),
        if p.acceptance().len() == 1 { "" } else { "s" },
        open.len(),
        if open.len() == 1 { "" } else { "s" },
        if t.plan_version > 1 { format!(" · revision {}", t.plan_version) } else { String::new() }
    );
    send_ask(shared, t, "plan", &subject, &body, &["Approve", "Revise", "Discard"])?;
    t.phase = Some("plan_gate".into());
    Ok(())
}

/// The builder says it's built: check what's checkable, then the checker.
async fn built(shared: &Shared, cfg: &Config, t: &mut Task) -> Result<()> {
    let w = wt(t);
    let base_head = t.base_head.clone().unwrap_or_default();
    let range = format!("{base_head}..HEAD");
    let count = git(&w, &["rev-list", "--count", &range]).await.unwrap_or_default().trim().parse::<i64>().unwrap_or(0);
    let tmp: Vec<String> = git(&w, &["diff", "--name-only", &range]).await.unwrap_or_default().lines().filter(|l| l.starts_with(".tmp/")).map(str::to_string).collect();
    let prj = std::fs::read_to_string(w.join(".tmp/pr.json")).ok().and_then(|s| serde_json::from_str::<Value>(&s).ok());
    let pr_ok = prj.as_ref().is_some_and(|p| p["title"].as_str().is_some_and(|x| !x.trim().is_empty()) && p["body"].as_str().is_some_and(|x| !x.trim().is_empty()));
    let problem = if count == 0 {
        Some(format!("there are no commits on {} past {}. Commit the work (Conventional Commits), then write BUILT.", t.branch.clone().unwrap_or_default(), t.base.clone().unwrap_or_default()))
    } else if !tmp.is_empty() {
        Some(format!("the commits include files under .tmp/, which is Wheelhouse's and never ships: {}. Take them out of the commits, then write BUILT.", tmp.join(", ")))
    } else if !pr_ok {
        Some(".tmp/pr.json is missing or has no title and body. Write it as .tmp/tasks/build.md says, then write BUILT.".to_string())
    } else {
        None
    };
    if let Some(p) = problem {
        if t.told.contains(&p) {
            return Ok(());
        }
        t.told.push(p.clone());
        return tell_builder(shared, t, &p, "WORKING building").await;
    }
    start_check(shared, cfg, t).await
}

async fn start_check(shared: &Shared, cfg: &Config, t: &mut Task) -> Result<()> {
    let w = wt(t);
    let head = git(&w, &["rev-parse", "HEAD"]).await?.trim().to_string();
    let old = t.built_head.clone();
    t.check_n += 1;
    t.built_head = Some(head.clone());
    set_status(&w, "WORKING building").ok();
    crate::review::write_atomic(&w.join(".tmp/checker-status.txt"), &format!("CHECKING {}\n", t.check_n))?;
    let n = t.check_n;
    let checker_open = pane(shared, &t.checker).is_some();
    if checker_open && t.checker.session.is_some() {
        let text = format!(
            "The builder changed the branch since your last check ({}..{}). Check it again as .tmp/tasks/check.md says. This is check {n}: write .tmp/check/check-{n}.json.",
            old.as_deref().map(short).unwrap_or_default(),
            short(&head)
        );
        let (s, b) = (t.session_id.clone().unwrap_or_default(), t.checker.block.clone().unwrap_or_default());
        panes::deliver(shared, &s, &b, &text).await?;
    } else {
        let prompt = format!("Read .tmp/ground-rules.md, then .tmp/tasks/check.md, and do it. This is check {n}.");
        let anchor = t.builder.block.clone();
        match anchor {
            Some(a) if panes::state(shared, &a).is_some() => open_role(shared, cfg, t, "checker", &prompt, Place::Beside { anchor: &a, dir: "right" }).await?,
            _ => open_role(shared, cfg, t, "checker", &prompt, Place::Tab).await?,
        }
    }
    t.phase = Some("checking".into());
    let mut core = shared.lock();
    super::event(&mut core, &t.docket, "checking", format!("Check {n} of {} at {}.", t.branch.clone().unwrap_or_default(), short(&head)), json!({ "n": n, "head": head }))?;
    Ok(())
}

async fn after_check(shared: &Shared, cfg: &Config, t: &mut Task) -> Result<()> {
    let w = wt(t);
    let n = t.check_n;
    let Some(c) = std::fs::read_to_string(w.join(format!(".tmp/check/check-{n}.json"))).ok().and_then(|s| serde_json::from_str::<Value>(&s).ok()) else {
        bail!("the checker wrote CHECKED but no .tmp/check/check-{n}.json");
    };
    let head = git(&w, &["rev-parse", "HEAD"]).await?.trim().to_string();
    if Some(&head) != t.built_head.as_ref() {
        return start_check(shared, cfg, t).await;
    }
    let verdict = c["verdict"].as_str().unwrap_or("pass").to_string();
    let findings = c["findings"].as_array().cloned().unwrap_or_default();
    t.checked_head = t.built_head.clone();
    t.last_check = Some(json!({ "n": n, "verdict": verdict, "summary": c["summary"], "findings": findings }));
    t.tokens = Some(agentcli::tokens(&w));
    {
        let mut core = shared.lock();
        core.append(&t.docket, "verdict", &Who::Hub, c["summary"].as_str().map(str::to_string), json!({ "role": "checker", "n": n, "verdict": verdict, "findings": findings.len(), "head": head }))?;
    }
    if verdict == "fix" && !findings.is_empty() && t.fix_used < cfg.fix_loops {
        t.fix_used += 1;
        t.phase = Some("building".into());
        let text = format!(
            "The checker found problems: .tmp/check/check-{n}.json (and its report, .tmp/check/report-{n}.md). Fix each one, or say in the report why it stands. Commit, update .tmp/pr.json if it changes, and write BUILT. This is fix round {} of {}.",
            t.fix_used, cfg.fix_loops
        );
        return tell_builder(shared, t, &text, "WORKING building").await;
    }
    // Something to see?
    let preview = std::fs::read_to_string(w.join(".tmp/preview.json")).ok().and_then(|s| serde_json::from_str::<Value>(&s).ok());
    if let Some(p) = preview.filter(|_| cfg.gate("preview") == "ask" && t.previewed_head != t.checked_head) {
        let title = p["title"].as_str().unwrap_or("the change").to_string();
        let body = format!("{}\n\n{}", p["ask"].as_str().unwrap_or("Does it look right?"), p["what"].as_str().unwrap_or(""));
        let mut asked = false;
        if let (Some(port), Some(b)) = (p["port"].as_u64(), t.builder.block.clone()) {
            let from = crate::messages::caller(&shared.lock(), Some(&b));
            if let Ok(v) = crate::preview::share(shared, &from, port as u16, Some(format!("{} · preview · {title}", t.key)), Some(body.clone()), vec!["Looks right".into(), "Needs work".into()]).await {
                let id = v["message"].as_i64().unwrap_or(0);
                let version = t.asks.get("preview").map_or(0, |a| a.version) + 1;
                t.asks.insert("preview".into(), super::Ask { message: id, version, options: vec!["Looks right".into(), "Needs work".into()] });
                asked = true;
            }
        }
        if !asked {
            send_ask(shared, t, "preview", &format!("{} · preview · {title}", t.key), &body, &["Looks right", "Needs work"])?;
        }
        t.phase = Some("preview_gate".into());
        return Ok(());
    }
    ask_open(shared, t, None)?;
    t.phase = Some("open_gate".into());
    gate_open(cfg, t);
    Ok(())
}

/// With the open gate on auto, it opens at once.
fn gate_open(cfg: &Config, t: &mut Task) {
    if cfg.gate("open") == "auto" {
        t.approved_head = t.checked_head.clone();
        t.phase = Some("opening".into());
    }
}

fn ask_open(shared: &Shared, t: &mut Task, failed: Option<&str>) -> Result<()> {
    let w = wt(t);
    let prj: Value = std::fs::read_to_string(w.join(".tmp/pr.json")).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    let commits = std::process::Command::new("git")
        .arg("-C")
        .arg(&w)
        .args(["log", "--oneline", &format!("{}..HEAD", t.base_head.clone().unwrap_or_default())])
        .env("PATH", github::command_path())
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    let n = commits.lines().count();
    let c = t.last_check.clone().unwrap_or_default();
    let left: Vec<String> = c["findings"].as_array().into_iter().flatten().map(|f| format!("- {} ({}): {}", f["id"].as_str().unwrap_or("?"), f["severity"].as_str().unwrap_or("?"), f["what"].as_str().unwrap_or(""))).collect();
    let stood = t.decisions.values().filter(|d| d.answer.as_ref().is_some_and(|a| a["stood"] == true)).count();
    let mut subject = format!("{} · ready to open · {n} commit{}, checked", t.key, if n == 1 { "" } else { "s" });
    if !left.is_empty() && c["verdict"] == "fix" {
        subject.push_str(&format!(", {} finding{} left", left.len(), if left.len() == 1 { "" } else { "s" }));
    } else if stood > 0 {
        subject.push_str(&format!(", {stood} recommendation{} stood", if stood == 1 { "" } else { "s" }));
    }
    let mut body = String::new();
    if let Some(e) = failed {
        body.push_str(&format!("Opening it failed: {e}\n\n"));
    }
    body.push_str(&format!("{}\n\n{}\n\nCommits:\n", prj["title"].as_str().unwrap_or("(no title)"), plan::strip(prj["body"].as_str().unwrap_or(""))));
    for l in commits.lines() {
        body.push_str(&format!("- {l}\n"));
    }
    if let Some(s) = c["summary"].as_str() {
        body.push_str(&format!("\nThe checker: {s}\n"));
    }
    if !left.is_empty() {
        body.push_str(&format!("\nFindings:\n{}\n", left.join("\n")));
    }
    body.push_str(&format!("\nOpen PR pushes {} and opens it {}on {} with the plan attached. Needs changes sends your words to the builder.", t.branch.clone().unwrap_or_default(), "as a draft ", t.repo));
    send_ask(shared, t, "open", &subject, &body, &["Open PR", "Needs changes", "Discard"])
}

// ---- Opening the PR ----

/// Push the checked head and open the PR, with the plan and the footer.
pub async fn open_pr(shared: &Shared, cfg: &Config, t: &mut Task) -> Result<()> {
    let w = wt(t);
    let branch = t.branch.clone().unwrap_or_default();
    let base = t.base.clone().unwrap_or_default();
    if branch.is_empty() || branch == base {
        bail!("the Build's branch is its base; nothing was pushed");
    }
    let head = git(&w, &["rev-parse", "HEAD"]).await?.trim().to_string();
    if Some(&head) != t.checked_head.as_ref() {
        t.phase = Some("building".into());
        set_status(&w, "BUILT")?;
        bail!("the branch moved to {} after the check passed {}; it's checked again before it opens", short(&head), t.checked_head.as_deref().map(short).unwrap_or_default());
    }
    if t.approved_head.as_ref() != Some(&head) {
        bail!("you approved {}, but the branch is at {}; nothing was pushed", t.approved_head.as_deref().map(short).unwrap_or_else(|| "nothing".into()), short(&head));
    }
    let base_head = t.base_head.clone().unwrap_or_default();
    let tmp = git(&w, &["diff", "--name-only", &format!("{base_head}..HEAD")]).await?;
    if tmp.lines().any(|l| l.starts_with(".tmp/")) {
        bail!("the commits include .tmp/ files; nothing was pushed");
    }
    let prj: Value = std::fs::read_to_string(w.join(".tmp/pr.json")).ok().and_then(|s| serde_json::from_str(&s).ok()).ok_or_else(|| anyhow!(".tmp/pr.json has no title and body; nothing was pushed"))?;
    let (title, body) = (prj["title"].as_str().unwrap_or("").trim().to_string(), plan::strip(prj["body"].as_str().unwrap_or("")));
    if title.is_empty() || body.is_empty() {
        bail!(".tmp/pr.json has no title and body; nothing was pushed");
    }
    let fp = crate::review::post::first_person(&json!({ "verdict": title, "checked": body }));
    if !fp.is_empty() {
        bail!("the PR draft speaks in the first person ({}); nothing was pushed", fp.join(" · "));
    }
    // Push as you, fast-forward only.
    let checkout = cfg.repos.get(&t.repo).map(|r| r.checkout.clone()).unwrap_or_default();
    let url = format!("{}/{}.git", std::env::var("WH_GIT_BASE").unwrap_or_else(|_| "https://github.com".into()), t.repo);
    let helper = format!("credential.helper=!'{}' auth git-credential", github::gh_bin().display());
    let remote = git(&checkout, &["-c", "credential.helper=", "-c", &helper, "ls-remote", "--heads", &url, &branch]).await?;
    if let Some(rsha) = remote.split_whitespace().next() {
        let _ = git(&checkout, &["-c", "credential.helper=", "-c", &helper, "fetch", "--no-tags", "--quiet", &url, rsha]).await;
        if git(&w, &["merge-base", "--is-ancestor", rsha, "HEAD"]).await.is_err() {
            bail!("GitHub's {branch} has commits this branch doesn't; nothing was pushed");
        }
    }
    git(&w, &["-c", "credential.helper=", "-c", &helper, "push", "--quiet", &url, &format!("HEAD:refs/heads/{branch}")]).await?;
    t.pushed_head = Some(head.clone());
    let login = github::login().await.unwrap_or_else(|_| "you".into());
    let names = [(&t.builder, "builder"), (&t.checker, "checker")].map(|(a, _)| agentcli::display_name(&a.agent, &w, a.session.as_deref()));
    let mode = cfg.repos.get(&t.repo).and_then(|r| r.plan.clone()).unwrap_or_else(|| "full".into());
    let mut full = String::new();
    if let Some(c) = closes(t)
        && !body.contains(&c)
    {
        full.push_str(&format!("{c}\n\n"));
    }
    full.push_str(&body);
    if mode != "off" {
        let (intent, record_head) = {
            let core = shared.lock();
            let st = core.store.state(&t.docket)?;
            (st.as_ref().and_then(|s| s.intent.as_ref()).map(|i| i.body.clone()).unwrap_or_default(), st.map(|s| s.head).unwrap_or_default())
        };
        let _ = record_head;
        let p = plan::parse(t.plan.as_deref().unwrap_or(""));
        let decided: Vec<plan::Decided> = t
            .decisions
            .iter()
            .map(|(id, d)| plan::Decided {
                id: id.clone(),
                question: d.question.clone(),
                answer: d.answer.as_ref().and_then(|a| a["label"].as_str().map(str::to_string)).map(|l| if l == "Another answer" { d.answer.as_ref().and_then(|a| a["text"].as_str()).unwrap_or("").to_string() } else { l }),
                by: d.answer.as_ref().map(|a| if a["stood"] == true { "stood".into() } else { "you".into() }),
                note: d.answer.as_ref().and_then(|a| a["text"].as_str().map(str::to_string)),
            })
            .collect();
        full.push_str("\n\n");
        full.push_str(&plan::block(&t.key, &intent, p.get("Notes"), &decided, &format!("@{login}"), &mode));
    }
    let record = { shared.lock().store.state(&t.docket)?.map(|s| s.head.chars().take(8).collect::<String>()).unwrap_or_default() };
    full.push_str(&format!(
        "\n\n---\n\n<div align=\"right\"><sub><i>{} on behalf of @{login} · checked by {} · Wheelhouse {} @ {record}</i></sub></div>",
        names[0], names[1], t.docket
    ));
    let existing = github::gh(&["pr", "list", "--repo", &t.repo, "--head", &branch, "--state", "open", "--json", "number,url"], None).await.unwrap_or_default();
    let existing: Vec<Value> = serde_json::from_str(&existing).unwrap_or_default();
    let (number, url) = if let Some(pr) = existing.first() {
        (pr["number"].as_i64(), pr["url"].as_str().map(str::to_string))
    } else {
        let mut args = vec!["pr", "create", "--repo", &t.repo, "--base", &base, "--head", &branch, "--title", &title, "--body-file", "-"];
        if cfg.pr_draft {
            args.push("--draft");
        }
        let rv: Vec<String> = cfg.pr_reviewers.iter().flat_map(|r| ["--reviewer".to_string(), r.clone()]).collect();
        args.extend(rv.iter().map(String::as_str));
        let out = github::gh(&args, Some(full.clone())).await?;
        let url = out.lines().rev().find(|l| l.contains("/pull/")).map(|l| l.trim().to_string());
        (url.as_deref().and_then(|u| u.rsplit('/').next()).and_then(|n| n.parse().ok()), url)
    };
    crate::review::write_atomic(&w.join(".tmp/pr-body.md"), &full)?;
    t.pr_number = number;
    t.pr_url = url.clone();
    t.state = "open".into();
    t.phase = None;
    t.tokens = Some(agentcli::tokens(&w));
    close_panes(shared, t).await;
    let mut core = shared.lock();
    core.append(&t.docket, "link", &Who::Hub, None, json!({ "kind": "github-pr", "url": url, "key": format!("{}#{}", t.repo, number.unwrap_or(0)) }))?;
    super::event(&mut core, &t.docket, "opened", format!("Opened {}#{} from {} at {}.", t.repo.rsplit('/').next().unwrap_or(&t.repo), number.unwrap_or(0), branch, short(&head)), json!({ "pr": number, "url": url, "head": head }))?;
    Ok(())
}

async fn close_panes(shared: &Shared, t: &mut Task) {
    if let Some(s) = t.session_id.clone() {
        for a in [&t.builder, &t.checker] {
            if let Some(b) = a.block.as_deref().filter(|b| panes::state(shared, b).is_some()) {
                panes::close(&s, b).await;
            }
        }
    }
    t.builder.block = None;
    t.checker.block = None;
}

/// Stop a Build: its panes close and its worktree goes; its branch stays.
pub async fn discard(shared: &Shared, t: &mut Task, why: &str) -> Result<()> {
    close_panes(shared, t).await;
    close_asks(shared, t);
    remove_worktree(shared, t, false).await;
    t.state = "discarded".into();
    t.phase = None;
    t.why = Some(why.to_string());
    t.ended_at = Some(now());
    let mut core = shared.lock();
    super::event(&mut core, &t.docket, "discarded", format!("Discarded: {why}. Its branch stays."), json!({}))?;
    Ok(())
}

async fn remove_worktree(shared: &Shared, t: &Task, branch_too: bool) {
    let Ok(cfg) = super::config(shared) else { return };
    let Some(checkout) = cfg.repos.get(&t.repo).map(|r| r.checkout.clone()) else { return };
    let w = wt(t);
    if w.join(".git").exists() {
        let _ = git(&checkout, &["worktree", "remove", "--force", &w.to_string_lossy()]).await;
    }
    let _ = git(&checkout, &["worktree", "prune"]).await;
    if branch_too
        && let Some(b) = &t.branch
    {
        let _ = git(&checkout, &["branch", "-D", b]).await;
    }
}

/// An open PR merged or closed: clean up.
pub async fn follow_pr(shared: &Shared, t: &mut Task) -> Result<()> {
    let Some(n) = t.pr_number else { return Ok(()) };
    let state = github::gh(&["pr", "view", &n.to_string(), "--repo", &t.repo, "--json", "state", "--jq", ".state"], None).await?.trim().to_string();
    if state != "MERGED" && state != "CLOSED" {
        return Ok(());
    }
    let merged = state == "MERGED";
    remove_worktree(shared, t, merged).await;
    t.state = if merged { "merged" } else { "closed" }.into();
    t.ended_at = Some(now());
    let mut core = shared.lock();
    super::event(&mut core, &t.docket, &t.state.clone(), if merged { "Merged.".to_string() } else { "Closed without merging; the branch stays.".to_string() }, json!({ "pr": n }))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn branch_names_are_short_and_plain() {
        assert_eq!(super::slug("Retry failed webhook deliveries, with backoff!", 40), "retry-failed-webhook-deliveries-with");
        assert_eq!(super::slug("Ünïcode & spaces", 40), "n-code-spaces");
    }
}
