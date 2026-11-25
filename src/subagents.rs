//! The subagents of a docket's agents: what each reviewer, checker, Build
//! agent or other agent on the docket spawned, read live from Claude Code's,
//! Codex's and Antigravity's own files. Nothing here is stored or goes on the
//! record.
//!
//! The docket's record names its agents: each `claim` has the agent's app
//! and folder, and a Claude Code agent's has its session. From there:
//!
//! - Claude Code keeps a session's subagents beside it, in
//!   `~/.claude/projects/<folder>/<session>/subagents/agent-<id>.jsonl`, with
//!   an `agent-<id>.meta.json` (its type, task, model, and whether it's done).
//! - Codex keeps every thread in its state database; a subagent's `source`
//!   names the thread that spawned it, and its rollout ends `task_complete`
//!   when it's done.
//! - Antigravity keeps each conversation, a subagent's too, in
//!   `~/.gemini/antigravity-cli/conversations/<id>.db` (its workspace) and
//!   `brain/<id>/` (its transcript); a subagent's first step is a message
//!   from the conversation that invoked it.
//!
//! An agent Wheelhouse started has no session on its claim (it isn't known
//! yet when the agent starts): its sessions are the ones that began in its
//! folder while it held the pane, each given to the latest claim before it.

use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{BufRead, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

use crate::record::Entry;
use crate::review::agentcli;

/// A session that began this long before its claim is still the claim's
/// (the claim is written once the pane is up).
const SLACK: i64 = 120;
/// A subagent that moved this recently and isn't done is running; one
/// quiet longer is idle (stopped, or waiting on something).
const RUNNING: i64 = 600;
/// How much of a Codex rollout's end is read for its state and last words.
const TAIL: u64 = 512 * 1024;
/// Last words are about this long.
const LAST_CHARS: usize = 600;

/// One agent on the docket, from its claims.
#[derive(Debug, Clone, PartialEq)]
pub struct Claim {
    pub app: String,
    pub label: String,
    pub cwd: PathBuf,
    pub session: Option<String>,
    /// The review round it worked on, if it's a reviewer or checker.
    pub round: Option<i64>,
    pub from: i64,
    pub until: Option<i64>,
}

fn epoch(at: &str) -> Option<i64> {
    crate::time::parse(at).map(|t| t.unix_timestamp())
}

fn rfc3339(secs: i64) -> Option<String> {
    time::OffsetDateTime::from_unix_timestamp(secs).ok()?.format(&time::format_description::well_known::Rfc3339).ok()
}

/// The docket's agents, from its record: each claim, until its pane's
/// release. A docket opened for an agent names it in `open`, as the fold
/// reads it: that's its first claim (a hook can open the docket, session
/// and all, before the pane is seen, and then no `claim` follows). One
/// opened without a session, then claimed with one, is one claim.
pub fn claims(entries: &[Entry]) -> Vec<Claim> {
    // (pane, from an `open` with no session, the claim)
    let mut out: Vec<(Option<String>, bool, Claim)> = Vec::new();
    for e in entries {
        let d = &e.data;
        match e.kind.as_str() {
            "open" | "claim" => {
                let a = &d["agent"];
                if !a.is_object() {
                    continue;
                }
                let app = match a["app"].as_str() {
                    Some("claude-code" | "claude") => "claude",
                    Some("codex") => "codex",
                    Some("agy" | "antigravity") => "agy",
                    Some("pi") => "pi",
                    _ => continue,
                };
                let Some(cwd) = a["cwd"].as_str().filter(|c| !c.is_empty()) else { continue };
                let Some(from) = epoch(&e.at) else { continue };
                let label = d["role"].as_str().or(a["pane"].as_str()).unwrap_or(app).to_string();
                let session = d["session"].as_str().filter(|s| !s.is_empty()).map(str::to_string);
                let block = a["block"].as_str().map(str::to_string);
                let mut from = from;
                // The pane's session, claimed after the docket was opened for
                // it: the same claim, from when it was opened.
                if e.kind == "claim"
                    && session.is_some()
                    && let Some(i) = out.iter().rposition(|(blk, opened, c)| *opened && block.is_some() && *blk == block && c.until.is_none() && c.app == app)
                {
                    from = from.min(out.remove(i).2.from);
                }
                let opened = e.kind == "open" && session.is_none();
                out.push((block, opened, Claim { app: app.into(), label, cwd: crate::paths::expand(cwd), session, round: d["round"].as_i64(), from, until: None }));
            }
            "release" => {
                let (Some(b), Some(at)) = (d["block"].as_str(), epoch(&e.at)) else { continue };
                if let Some((_, _, c)) = out.iter_mut().rev().find(|(blk, _, c)| blk.as_deref() == Some(b) && c.until.is_none()) {
                    c.until = Some(at);
                }
            }
            _ => {}
        }
    }
    out.into_iter().map(|(_, _, c)| c).collect()
}

/// Of the claims in `cwd` for `app`, the one a session that began at `t`
/// belongs to: the latest that started before it (with slack), if it still
/// held the pane then.
fn owner<'a>(claims: &'a [Claim], app: &str, cwd: &Path, t: i64) -> Option<&'a Claim> {
    claims.iter().filter(|c| c.app == app && c.cwd == cwd && c.session.is_none() && c.from - SLACK <= t).max_by_key(|c| c.from).filter(|c| c.until.is_none_or(|u| t <= u + SLACK))
}

/// The review round under way in `cwd` at `t`: the latest claim's there.
fn round_at(claims: &[Claim], app: &str, cwd: &Path, t: i64) -> Option<i64> {
    claims.iter().filter(|c| c.app == app && c.cwd == cwd && c.round.is_some() && c.from - SLACK <= t).max_by_key(|c| c.from).and_then(|c| c.round)
}

// ---- Claude Code ----

#[derive(Clone, Default)]
struct ClaudeRead {
    tokens: i64,
    first: Option<String>,
    last: Option<String>,
    words: Option<String>,
    model: Option<String>,
}

type Cache = HashMap<PathBuf, (u64, Option<SystemTime>, ClaudeRead)>;

fn cache() -> &'static Mutex<Cache> {
    static C: OnceLock<Mutex<Cache>> = OnceLock::new();
    C.get_or_init(Mutex::default)
}

/// A Claude Code transcript's tokens, first and last times, model and last
/// words; read again only when the file changed.
fn claude_read(path: &Path) -> ClaudeRead {
    let meta = std::fs::metadata(path).ok();
    let (len, mtime) = (meta.as_ref().map_or(0, |m| m.len()), meta.and_then(|m| m.modified().ok()));
    if let Some((l, m, r)) = cache().lock().unwrap().get(path)
        && *l == len
        && *m == mtime
    {
        return r.clone();
    }
    let mut r = ClaudeRead::default();
    let mut seen = HashSet::new();
    if let Ok(f) = std::fs::File::open(path) {
        for line in std::io::BufReader::new(f).lines().map_while(Result::ok) {
            let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
            if let Some(t) = v["timestamp"].as_str() {
                r.first.get_or_insert_with(|| t.to_string());
                r.last = Some(t.to_string());
            }
            if v["type"] != "assistant" {
                continue;
            }
            let m = &v["message"];
            if let Some(model) = m["model"].as_str().filter(|x| x.starts_with("claude-")) {
                r.model = Some(model.to_string());
            }
            let text: String = m["content"].as_array().into_iter().flatten().filter(|c| c["type"] == "text").filter_map(|c| c["text"].as_str()).collect::<Vec<_>>().join("\n");
            if !text.trim().is_empty() {
                r.words = Some(text);
            }
            let u = &m["usage"];
            if !u.is_null() && m["id"].as_str().is_none_or(|id| seen.insert(id.to_string())) {
                let n = |k: &str| u[k].as_i64().unwrap_or(0);
                r.tokens += n("input_tokens") + n("cache_read_input_tokens") + n("cache_creation_input_tokens") + n("output_tokens");
            }
        }
    }
    cache().lock().unwrap().insert(path.to_path_buf(), (len, mtime, r.clone()));
    r
}

/// Where a Claude Code session's subagents are: beside it in its folder's
/// project, or (a session resumed elsewhere) in whichever project has it.
fn claude_subagent_dir(cwd: &Path, session: &str) -> Option<PathBuf> {
    let d = agentcli::claude_project(cwd).join(session).join("subagents");
    if d.is_dir() {
        return Some(d);
    }
    let projects = crate::paths::home().join(".claude/projects");
    std::fs::read_dir(projects).ok()?.filter_map(Result::ok).map(|e| e.path().join(session).join("subagents")).find(|p| p.is_dir())
}

/// Claude Code sessions that began in `cwd` from `from` (seconds) on; a
/// folder's listing is read once into `listings`.
fn claude_sessions(listings: &mut HashMap<PathBuf, Vec<(String, i64)>>, cwd: &Path, from: i64) -> Vec<(String, i64)> {
    let all = listings.entry(cwd.to_path_buf()).or_insert_with(|| claude_listing(cwd));
    all.iter().filter(|(_, t)| *t >= from).cloned().collect()
}

/// Every Claude Code session in `cwd`'s project, with when it began.
fn claude_listing(cwd: &Path) -> Vec<(String, i64)> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(agentcli::claude_project(cwd)).into_iter().flatten().flatten() {
        let p = e.path();
        if p.extension().is_none_or(|x| x != "jsonl") {
            continue;
        }
        let Some(id) = p.file_stem().and_then(|s| s.to_str()).map(str::to_string) else { continue };
        let born = e.metadata().ok().and_then(|m| m.created().or_else(|_| m.modified()).ok());
        let Some(t) = born.and_then(|b| b.duration_since(SystemTime::UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64) else { continue };
        out.push((id, t));
    }
    out
}

fn claude_subagents(dir: &Path, now: i64) -> Vec<Value> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        let Some(name) = p.file_name().and_then(|n| n.to_str()) else { continue };
        let Some(id) = name.strip_prefix("agent-").and_then(|n| n.strip_suffix(".jsonl")) else { continue };
        let meta: Value = std::fs::read_to_string(dir.join(format!("agent-{id}.meta.json"))).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        let r = claude_read(&p);
        let last = r.last.as_deref().and_then(epoch);
        let state = if meta["finished"] == true {
            "done"
        } else if last.is_some_and(|l| now - l < RUNNING) {
            "running"
        } else {
            "idle"
        };
        let model = meta["model"].as_str().map(str::to_string).or(r.model.clone());
        out.push(json!({
            "id": id,
            "name": meta["agentType"].as_str().unwrap_or("subagent"),
            "task": meta["description"],
            "model": model.as_deref().and_then(agentcli::claude_model_name).or(model),
            "effort": Value::Null,
            "state": state,
            "tokens": r.tokens,
            "started_at": r.first,
            "last_at": r.last,
            "words": r.words.as_deref().map(clip),
            "depth": meta["spawnDepth"].as_i64().unwrap_or(1),
            "background": meta["requestShape"] == "background",
        }));
    }
    out
}

// ---- Codex ----

/// The end of a Codex rollout: whether its last turn completed, and its
/// last words.
fn codex_tail(rollout: &str) -> (Option<&'static str>, Option<String>) {
    let Ok(mut f) = std::fs::File::open(rollout) else { return (None, None) };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let _ = f.seek(SeekFrom::Start(len.saturating_sub(TAIL)));
    let mut buf = Vec::new();
    let _ = f.read_to_end(&mut buf);
    let text = String::from_utf8_lossy(&buf);
    let (mut state, mut words) = (None, None);
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        let p = &v["payload"];
        match (v["type"].as_str(), p["type"].as_str()) {
            (Some("event_msg"), Some("task_complete")) => {
                state = Some("done");
                if let Some(m) = p["last_agent_message"].as_str() {
                    words = Some(m.to_string());
                }
            }
            (Some("event_msg"), Some("turn_aborted")) => state = Some("stopped"),
            (Some("event_msg"), Some("task_started")) => state = None,
            (Some("event_msg"), Some("agent_message")) => {
                if let Some(m) = p["message"].as_str() {
                    words = Some(m.to_string());
                }
            }
            _ => {}
        }
    }
    (state, words)
}

/// Codex threads that began in `cwd` from `from` on, not themselves subagents.
fn codex_threads(conn: &rusqlite::Connection, cwd: &Path, from: i64) -> Vec<(String, i64)> {
    conn.prepare("select id, created_at from threads where cwd = ?1 and created_at >= ?2 and source not like '{%'")
        .and_then(|mut st| st.query_map(rusqlite::params![cwd.to_string_lossy(), from], |r| Ok((r.get(0)?, r.get(1)?)))?.collect())
        .unwrap_or_default()
}

/// Every subagent under `thread`, nested ones too (`depth` counts down).
fn codex_subagents(conn: &rusqlite::Connection, thread: &str, now: i64, depth: i64, out: &mut Vec<Value>) {
    if depth > 4 {
        return;
    }
    let sql = "select id, agent_nickname, json_extract(source, '$.subagent.thread_spawn.agent_path'), agent_role, model, reasoning_effort, tokens_used, created_at, updated_at, rollout_path, title
               from threads where source like '{\"subagent\"%' and json_valid(source) and json_extract(source, '$.subagent.thread_spawn.parent_thread_id') = ?1 order by created_at";
    type Row = (String, Option<String>, Option<String>, Option<String>, Option<String>, Option<String>, i64, i64, i64, String, String);
    let rows: Vec<Row> = conn
        .prepare(sql)
        .and_then(|mut st| {
            st.query_map([thread], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?, r.get(10)?)))?.collect()
        })
        .unwrap_or_default();
    for (id, nick, path, role, model, effort, tokens, created, updated, rollout, title) in rows {
        let (done, words) = codex_tail(&rollout);
        let state = done.unwrap_or(if now - updated < RUNNING { "running" } else { "idle" });
        let role = path.as_deref().map(|p| p.trim_start_matches("/root/").trim_start_matches('/').to_string()).or(role);
        out.push(json!({
            "id": id,
            "name": nick.unwrap_or_else(|| "subagent".into()),
            "task": role,
            "title": (!title.is_empty()).then_some(title),
            "model": model,
            "effort": effort,
            "state": state,
            "tokens": tokens,
            "started_at": rfc3339(created),
            "last_at": rfc3339(updated),
            "words": words.as_deref().map(clip),
            "depth": depth,
            "parent": thread,
        }));
        codex_subagents(conn, &id, now, depth + 1, out);
    }
}

// ---- Antigravity ----

/// Every subagent under `conversation` (nested ones too), from the
/// conversations in its folder: each names its caller.
fn agy_subagents(root: &Path, all: &[agentcli::AgyConversation], conversation: &str, now: i64, depth: i64, out: &mut Vec<Value>) {
    if depth > 4 {
        return;
    }
    let caller = agentcli::agy_read(root, conversation);
    for c in all.iter().filter(|c| c.parent.as_deref() == Some(conversation)) {
        let r = agentcli::agy_read(root, &c.id);
        // Its task is the prompt it was invoked with: the call that sent it
        // says its role and type.
        let task = agy_task(root, &c.id);
        let call = task.as_deref().and_then(|t| caller.invoked.iter().find(|(p, _, _)| p.trim() == t.trim()));
        let last = r.last.as_deref().and_then(epoch).unwrap_or(c.updated);
        let state = if r.finished {
            "done"
        } else if now - last < RUNNING {
            "running"
        } else {
            "idle"
        };
        out.push(json!({
            "id": c.id,
            "name": call.and_then(|(_, _, ty)| ty.clone()).unwrap_or_else(|| "subagent".into()),
            "task": call.and_then(|(_, role, _)| role.clone()).or_else(|| task.as_deref().map(clip)),
            "model": r.model,
            "effort": r.effort,
            "state": state,
            "tokens": r.tokens.iter().sum::<i64>(),
            "started_at": r.first.or_else(|| rfc3339(c.created)),
            "last_at": r.last.or_else(|| rfc3339(c.updated)),
            "words": r.words.as_deref().map(clip),
            "depth": depth,
            "parent": conversation,
        }));
        agy_subagents(root, all, &c.id, now, depth + 1, out);
    }
}

/// The prompt a subagent was invoked with: its first step,
/// `[Message] … sender=<id> … content=<prompt>`.
fn agy_task(root: &Path, id: &str) -> Option<String> {
    let f = std::fs::File::open(agentcli::agy_transcript(root, id)).ok()?;
    let first = std::io::BufReader::new(f).lines().next()?.ok()?;
    let v: Value = serde_json::from_str(&first).ok()?;
    let c = v["content"].as_str()?;
    let body = &c[c.find(" content=")? + " content=".len()..];
    Some(body.split("</SYSTEM_MESSAGE>").next().unwrap_or(body).trim().to_string())
}

fn clip(s: &str) -> String {
    let s = s.trim();
    if s.chars().count() <= LAST_CHARS {
        return s.to_string();
    }
    let mut t: String = s.chars().take(LAST_CHARS).collect();
    t.push('…');
    t
}

// ---- A docket's ----

/// (label, round, cwd, began) of an (app, session).
type Session = (String, Option<i64>, PathBuf, i64);

/// Antigravity's conversations, by folder.
type AgyByFolder = HashMap<PathBuf, Vec<agentcli::AgyConversation>>;

/// The docket's agents' sessions, each (app, session) once with the claim
/// it's under, and Antigravity's conversations by folder (for subagents).
fn collect(
    claims: &[Claim],
    conn: Option<&rusqlite::Connection>,
    agy_root: &Path,
    pi_root: &Path,
    listings: &mut HashMap<PathBuf, Vec<(String, i64)>>,
) -> (BTreeMap<(String, String), Session>, AgyByFolder) {
    let mut sessions: BTreeMap<(String, String), Session> = BTreeMap::new();
    let mut put = |app: &str, s: String, c: &Claim, t: i64| {
        sessions.entry((app.to_string(), s)).or_insert_with(|| (c.label.clone(), c.round, c.cwd.clone(), t));
    };
    for c in claims.iter().filter(|c| c.app == "claude") {
        match &c.session {
            Some(s) => put("claude", s.clone(), c, c.from),
            None => {
                for (s, t) in claude_sessions(listings, &c.cwd, c.from - SLACK) {
                    if owner(claims, "claude", &c.cwd, t).is_some_and(|o| std::ptr::eq(o, c)) {
                        put("claude", s, c, t);
                    }
                }
            }
        }
    }
    if let Some(conn) = conn {
        for c in claims.iter().filter(|c| c.app == "codex") {
            for (s, t) in codex_threads(conn, &c.cwd, c.from - SLACK) {
                if owner(claims, "codex", &c.cwd, t).is_some_and(|o| std::ptr::eq(o, c)) {
                    put("codex", s, c, t);
                }
            }
        }
    }
    let mut agy: AgyByFolder = HashMap::new();
    for c in claims.iter().filter(|c| c.app == "agy") {
        let first = claims.iter().filter(|x| x.app == "agy" && x.cwd == c.cwd).map(|x| x.from).min().unwrap_or(c.from);
        let all = agy.entry(c.cwd.clone()).or_insert_with(|| agentcli::agy_conversations(agy_root, &c.cwd, first - SLACK)).clone();
        match &c.session {
            Some(s) => put("agy", s.clone(), c, c.from),
            None => {
                for x in all.iter().filter(|x| x.parent.is_none() && x.created >= c.from - SLACK) {
                    if owner(claims, "agy", &c.cwd, x.created).is_some_and(|o| std::ptr::eq(o, c)) {
                        put("agy", x.id.clone(), c, x.created);
                    }
                }
            }
        }
    }
    // Pi's: the session its extension named, else the ones begun in its
    // folder while this claim held it.
    for c in claims.iter().filter(|c| c.app == "pi") {
        match &c.session {
            Some(s) => put("pi", s.clone(), c, c.from),
            None => {
                for (s, t) in pi_listing(&agentcli::pi_project(pi_root, &c.cwd)) {
                    if t >= c.from - SLACK && owner(claims, "pi", &c.cwd, t).is_some_and(|o| std::ptr::eq(o, c)) {
                        put("pi", s, c, t);
                    }
                }
            }
        }
    }
    (sessions, agy)
}

/// Every Pi session in one of its folders, with when it began.
fn pi_listing(dir: &Path) -> Vec<(String, i64)> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        let Some(id) = agentcli::pi_session_id(&p).filter(|id| plain_id(id)).map(str::to_string) else { continue };
        let born = e.metadata().ok().and_then(|m| m.created().or_else(|_| m.modified()).ok());
        let Some(t) = born.and_then(|b| b.duration_since(SystemTime::UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64) else { continue };
        out.push((id, t));
    }
    out
}

/// A transcript a docket's agent wrote, and the span (unix seconds) the
/// docket's claims held its session over: its calls outside that are some
/// other work's. `named`: a claim names the session (not found by folder).
#[derive(Debug, Clone, PartialEq)]
pub struct Transcript {
    pub app: String,
    pub path: PathBuf,
    pub from: i64,
    pub until: Option<i64>,
    pub named: bool,
}

/// A session or conversation id that's only an id: letters, digits, `-`
/// and `_` (never a path).
fn plain_id(s: &str) -> bool {
    !s.is_empty() && s.len() <= 128 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// What finding many dockets' sessions shares: Codex's database, and each
/// Claude Code project's listing.
pub struct Finder {
    conn: Option<rusqlite::Connection>,
    agy_root: PathBuf,
    pi_root: PathBuf,
    claude: HashMap<PathBuf, Vec<(String, i64)>>,
    codex: Option<CodexTree>,
}

/// Every Codex thread's rollout, and which threads each one spawned.
#[derive(Default)]
struct CodexTree {
    rollout: HashMap<String, String>,
    children: HashMap<String, Vec<String>>,
}

fn codex_tree(conn: &rusqlite::Connection) -> CodexTree {
    let mut t = CodexTree::default();
    let sql = "select id, rollout_path, case when source like '{\"subagent\"%' and json_valid(source) then json_extract(source, '$.subagent.thread_spawn.parent_thread_id') end from threads";
    let rows: Vec<(String, String, Option<String>)> =
        conn.prepare(sql).and_then(|mut st| st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect()).unwrap_or_default();
    for (id, rollout, parent) in rows {
        if let Some(p) = parent {
            t.children.entry(p).or_default().push(id.clone());
        }
        t.rollout.insert(id, rollout);
    }
    t
}

impl Finder {
    pub fn new() -> Self {
        Finder { conn: agentcli::codex_conn(), agy_root: agentcli::agy_home(), pi_root: agentcli::pi_sessions_root(), claude: HashMap::new(), codex: None }
    }

    /// Every file a docket's agents wrote their model calls to: each
    /// agent's own transcript and its subagents', with the span its claims
    /// held the session over. For the recap's usage; a path is the same
    /// file however it's reached.
    pub fn transcripts(&mut self, claims: &[Claim]) -> Vec<Transcript> {
        let (sessions, agy) = collect(claims, self.conn.as_ref(), &self.agy_root, &self.pi_root, &mut self.claude);
        let mut found: Vec<(String, PathBuf, String)> = Vec::new();
        let mut all = Vec::new();
        for ((app, session), (_, _, cwd, _)) in sessions {
            // Session ids come from agents' hooks: only a plain id names a file.
            if !plain_id(&session) {
                continue;
            }
            found.clear();
            let out = &mut found;
            match app.as_str() {
                "claude" => {
                    let own = agentcli::claude_project(&cwd).join(format!("{session}.jsonl"));
                    let Some(own) = (if own.is_file() { Some(own) } else { claude_find(&session) }) else { continue };
                    // Its subagents are beside it.
                    let subs = own.with_extension("").join("subagents");
                    out.push(("claude".to_string(), own, session.clone()));
                    for e in std::fs::read_dir(subs).into_iter().flatten().flatten() {
                        let p = e.path();
                        if p.extension().is_some_and(|x| x == "jsonl") {
                            out.push(("claude".into(), p, session.clone()));
                        }
                    }
                }
                "codex" => {
                    let tree = self.codex.get_or_insert_with(|| self.conn.as_ref().map(codex_tree).unwrap_or_default());
                    let mut ids = vec![session.clone()];
                    let mut i = 0;
                    // Its subagents, and theirs.
                    while i < ids.len() && i < 256 {
                        ids.extend(tree.children.get(&ids[i]).into_iter().flatten().cloned());
                        i += 1;
                    }
                    out.extend(ids.iter().filter_map(|id| tree.rollout.get(id)).map(|p| ("codex".to_string(), PathBuf::from(p), session.clone())));
                }
                "agy" => {
                    let mut ids = vec![session.clone()];
                    let all = agy.get(&cwd).map(Vec::as_slice).unwrap_or_default();
                    let mut i = 0;
                    // Its subagents, and theirs, by caller.
                    while i < ids.len() && i < 256 {
                        let id = ids[i].clone();
                        ids.extend(all.iter().filter(|c| c.parent.as_deref() == Some(&id)).map(|c| c.id.clone()));
                        i += 1;
                    }
                    out.extend(ids.iter().filter(|id| plain_id(id)).map(|id| ("agy".to_string(), agentcli::agy_transcript(&self.agy_root, id), session.clone())));
                }
                "pi" => {
                    let Some(p) = agentcli::pi_transcript(&self.pi_root, &cwd, &session) else { continue };
                    out.push(("pi".to_string(), p, session.clone()));
                }
                _ => {}
            }
            // The claims that held it: one naming it, or (a session found by
            // its folder) the session-less ones in that folder.
            let held: Vec<&Claim> = claims.iter().filter(|c| c.app == app && (c.session.as_deref() == Some(&session) || (c.session.is_none() && c.cwd == cwd))).collect();
            let named = held.iter().any(|c| c.session.is_some());
            let from = held.iter().map(|c| c.from).min().unwrap_or(i64::MIN / 2) - SLACK;
            let until = if held.iter().any(|c| c.until.is_none()) { None } else { held.iter().filter_map(|c| c.until).max().map(|u| u + SLACK) };
            all.extend(found.drain(..).map(|(app, path, _)| Transcript { app, path, from, until, named }));
        }
        all
    }
}

/// A Claude Code session's transcript in whichever project has it (one
/// resumed in another folder).
fn claude_find(session: &str) -> Option<PathBuf> {
    let projects = crate::paths::home().join(".claude/projects");
    std::fs::read_dir(projects).ok()?.filter_map(Result::ok).map(|e| e.path().join(format!("{session}.jsonl"))).find(|p| p.is_file())
}

/// Each of the docket's agents (one per session) with its subagents, newest
/// agent first; `running`, `total` and `tokens` sum them.
pub fn of(claims: &[Claim]) -> Value {
    let now = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let conn = agentcli::codex_conn();
    let agy_root = agentcli::agy_home();
    let (sessions, agy) = collect(claims, conn.as_ref(), &agy_root, &agentcli::pi_sessions_root(), &mut HashMap::new());
    let (mut running, mut total, mut tokens) = (0, 0, 0i64);
    let mut agents = Vec::new();
    for ((app, session), (label, round, cwd, began)) in sessions {
        let subs = match app.as_str() {
            "claude" => claude_subagent_dir(&cwd, &session).map(|d| claude_subagents(&d, now)).unwrap_or_default(),
            "codex" => {
                let mut v = Vec::new();
                if let Some(conn) = &conn {
                    codex_subagents(conn, &session, now, 1, &mut v);
                }
                v
            }
            "agy" => {
                let mut v = Vec::new();
                if let Some(all) = agy.get(&cwd) {
                    agy_subagents(&agy_root, all, &session, now, 1, &mut v);
                }
                v
            }
            _ => Vec::new(),
        };
        let mut subs = subs;
        subs.sort_by(|a, b| a["started_at"].as_str().cmp(&b["started_at"].as_str()));
        running += subs.iter().filter(|s| s["state"] == "running").count();
        total += subs.len();
        tokens += subs.iter().filter_map(|s| s["tokens"].as_i64()).sum::<i64>();
        // A reviewer's session goes on from round to round: each subagent
        // is the round's whose claim was latest when it started.
        let mut by_round: BTreeMap<Option<i64>, Vec<Value>> = BTreeMap::new();
        for sub in subs {
            let t = sub["started_at"].as_str().and_then(epoch).unwrap_or(began);
            let r = if round.is_some() { round_at(claims, &app, &cwd, t).or(round) } else { None };
            by_round.entry(r).or_default().push(sub);
        }
        if by_round.is_empty() {
            by_round.insert(round, Vec::new());
        }
        for (r, subs) in by_round {
            let first = subs.first().and_then(|x| x["started_at"].as_str()).and_then(epoch).unwrap_or(began);
            agents.push((
                (r.unwrap_or(0), first),
                json!({ "app": app, "label": label, "round": r, "session": session, "cwd": crate::paths::tilde(&cwd.to_string_lossy()), "began_at": rfc3339(first), "subagents": subs }),
            ));
        }
    }
    agents.sort_by_key(|(k, _)| std::cmp::Reverse(*k));
    json!({ "agents": agents.into_iter().map(|(_, a)| a).collect::<Vec<_>>(), "running": running, "total": total, "tokens": tokens })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(kind: &str, at: &str, data: Value) -> Entry {
        serde_json::from_value(json!({ "v": 1, "docket": "D-1", "kind": kind, "at": at, "actor": { "type": "hub", "id": "hub@local" }, "data": data })).unwrap()
    }

    #[test]
    fn a_records_claims_name_its_agents_until_their_panes_close() {
        let es = [
            entry(
                "claim",
                "2026-11-20T13:31:20Z",
                json!({ "source": "wheelhouse", "role": "codex reviewer", "agent": { "app": "codex", "block": "b1", "pane": "storefront-793", "cwd": "/w/pr-793" } }),
            ),
            entry("claim", "2026-11-20T13:51:23Z", json!({ "source": "wheelhouse", "role": "checker", "agent": { "app": "codex", "block": "b2", "cwd": "/w/pr-793" } })),
            entry("release", "2026-11-20T13:58:22Z", json!({ "reason": "pane closed", "block": "b1" })),
            entry(
                "claim",
                "2026-11-20T16:38:22Z",
                json!({ "session": "s-1", "source": "startup", "agent": { "app": "claude-code", "block": "b3", "pane": "shell", "cwd": "/r" } }),
            ),
            entry("claim", "2026-11-20T16:40:00Z", json!({ "agent": { "app": "rex", "cwd": "/r" } })),
        ];
        let cs = claims(&es);
        assert_eq!(cs.len(), 3, "an app that isn't an agent is left out");
        assert_eq!((cs[0].label.as_str(), cs[0].until.is_some()), ("codex reviewer", true));
        assert_eq!((cs[1].label.as_str(), cs[1].until), ("checker", None));
        assert_eq!((cs[2].app.as_str(), cs[2].session.as_deref()), ("claude", Some("s-1")));

        // A thread that began after the checker's claim is the checker's,
        // though the reviewer's pane was still open; one from before it is
        // the reviewer's.
        let t_checker = cs[1].from + 30;
        assert_eq!(owner(&cs, "codex", Path::new("/w/pr-793"), t_checker).map(|c| c.label.as_str()), Some("checker"));
        assert_eq!(owner(&cs, "codex", Path::new("/w/pr-793"), cs[0].from + 10).map(|c| c.label.as_str()), Some("codex reviewer"));
        assert_eq!(owner(&cs, "codex", Path::new("/w/other"), t_checker), None);
        // A session long after the reviewer's pane closed, with no claim of
        // its own, isn't the docket's.
        // A session that goes on into the next round: what it spawns then is
        // that round's.
        let mut rounds = cs.clone();
        rounds[0].round = Some(1);
        rounds[1].round = Some(1);
        rounds.push(Claim { round: Some(2), from: cs[1].from + 3600, until: None, ..cs[0].clone() });
        assert_eq!(round_at(&rounds, "codex", Path::new("/w/pr-793"), cs[0].from + 60), Some(1));
        assert_eq!(round_at(&rounds, "codex", Path::new("/w/pr-793"), cs[1].from + 3700), Some(2));
        let only_reviewer = &cs[..1];
        assert_eq!(owner(only_reviewer, "codex", Path::new("/w/pr-793"), cs[0].until.unwrap() + 3600), None);
    }

    #[test]
    fn a_docket_opened_for_an_agent_names_it_once() {
        let pane = |app: &str, b: &str| json!({ "app": app, "block": b, "pane": "D-1", "cwd": "/w" });
        // A hook opened it, session and all, before the pane was seen: no claim follows.
        let cs = claims(&[entry("open", "2026-11-25T14:00:00Z", json!({ "protocol": "agent", "agent": pane("pi", "p1"), "session": "s-1", "source": "startup" }))]);
        assert_eq!(cs.len(), 1);
        assert_eq!((cs[0].app.as_str(), cs[0].session.as_deref()), ("pi", Some("s-1")));

        // Opened for the pane, then its session claimed: one claim, from the open.
        let cs = claims(&[
            entry("open", "2026-11-25T14:00:00Z", json!({ "protocol": "agent", "agent": pane("claude-code", "b1") })),
            entry("claim", "2026-11-25T14:00:02Z", json!({ "session": "s-2", "source": "startup", "agent": pane("claude-code", "b1") })),
            entry("release", "2026-11-25T15:00:00Z", json!({ "block": "b1", "reason": "pane closed" })),
        ]);
        assert_eq!(cs.len(), 1, "{cs:?}");
        assert_eq!((cs[0].session.as_deref(), cs[0].from, cs[0].until), (Some("s-2"), epoch("2026-11-25T14:00:00Z").unwrap(), epoch("2026-11-25T15:00:00Z")));

        // Codex has no hooks: the open is all there is, found by its folder.
        let cs = claims(&[entry("open", "2026-11-25T14:00:00Z", json!({ "protocol": "agent", "agent": pane("codex", "b2") }))]);
        assert_eq!((cs.len(), cs[0].session.as_deref()), (1, None));

        // A docket you opened has no agent; another pane's claim isn't this one's.
        let cs = claims(&[
            entry("open", "2026-11-25T14:00:00Z", json!({ "protocol": "none" })),
            entry("open", "2026-11-25T14:00:00Z", json!({ "protocol": "agent", "agent": pane("pi", "p1") })),
            entry("claim", "2026-11-25T14:05:00Z", json!({ "session": "s-3", "source": "join", "agent": pane("pi", "p2") })),
        ]);
        assert_eq!(cs.iter().map(|c| c.session.as_deref()).collect::<Vec<_>>(), [None, Some("s-3")]);
    }

    #[test]
    fn only_a_plain_session_id_names_a_file() {
        assert!(plain_id("01a20b4f-50c1-70e3-ae70-28a8998a2f12"));
        for bad in ["", "../../etc/x", "a/b", "a.jsonl", &"x".repeat(129)] {
            assert!(!plain_id(bad), "{bad}");
        }
    }

    #[test]
    fn a_codex_rollouts_end_says_whether_its_done() {
        let dir = std::env::temp_dir().join(format!("wh-subagents-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let r = dir.join("rollout.jsonl");
        let line = |t: &str, extra: Value| {
            let mut p = json!({ "type": t });
            p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            json!({ "type": "event_msg", "payload": p }).to_string()
        };
        std::fs::write(&r, [line("task_started", json!({})), line("agent_message", json!({ "message": "Reading the migration." }))].join("\n")).unwrap();
        assert_eq!(codex_tail(&r.to_string_lossy()), (None, Some("Reading the migration.".into())));
        std::fs::write(&r, [line("task_started", json!({})), line("task_complete", json!({ "last_agent_message": "Migration 154 leaves links ambiguous." }))].join("\n")).unwrap();
        assert_eq!(codex_tail(&r.to_string_lossy()), (Some("done"), Some("Migration 154 leaves links ambiguous.".into())));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_claude_subagent_reads_from_its_transcript_and_meta() {
        let dir = std::env::temp_dir().join(format!("wh-subagents-claude-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let usage = json!({ "input_tokens": 10, "cache_read_input_tokens": 100, "cache_creation_input_tokens": 5, "output_tokens": 20 });
        let lines = [
            json!({ "type": "user", "timestamp": "2026-11-20T17:00:00Z", "message": { "role": "user", "content": "Research it" } }),
            json!({ "type": "assistant", "timestamp": "2026-11-20T17:00:05Z", "message": { "id": "m1", "model": "claude-opus-5-5", "content": [{ "type": "tool_use" }], "usage": usage } }),
            json!({ "type": "assistant", "timestamp": "2026-11-20T17:00:06Z", "message": { "id": "m1", "model": "claude-opus-5-5", "content": [{ "type": "text", "text": "Found it." }], "usage": usage } }),
        ];
        std::fs::write(dir.join("agent-a1.jsonl"), lines.iter().map(Value::to_string).collect::<Vec<_>>().join("\n")).unwrap();
        std::fs::write(
            dir.join("agent-a1.meta.json"),
            r#"{"agentType":"general-purpose","description":"Research AIUC-1","model":"claude-opus-5-5","finished":true,"spawnDepth":1}"#,
        )
        .unwrap();
        let subs = claude_subagents(&dir, epoch("2026-11-20T17:01:00Z").unwrap());
        assert_eq!(subs.len(), 1);
        let s = &subs[0];
        assert_eq!((s["name"].as_str(), s["task"].as_str(), s["state"].as_str()), (Some("general-purpose"), Some("Research AIUC-1"), Some("done")));
        assert_eq!(s["tokens"], json!(135), "a response's usage counts once");
        assert_eq!((s["model"].as_str(), s["words"].as_str()), (Some("Opus 5.5"), Some("Found it.")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_agy_subagent_is_the_conversation_its_caller_invoked() {
        let root = std::env::temp_dir().join(format!("wh-subagents-agy-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let wt = PathBuf::from("/w/pr-9");
        let call = json!([{ "name": "invoke_subagent", "args": { "Prompt": "\"Reply with pong.\"", "Role": "\"Ping Responder\"", "TypeName": "\"research\"" } }]);
        agentcli::fake_agy(
            &root,
            "main",
            &wt,
            &[
                json!({ "type": "PLANNER_RESPONSE", "status": "DONE", "created_at": "2026-11-23T23:57:36Z", "model": "gemini-4-neon-medium", "input_tokens": 10, "tool_calls": call }),
            ],
        );
        agentcli::fake_agy(
            &root,
            "kid",
            &wt,
            &[
                json!({ "type": "SYSTEM_MESSAGE", "created_at": "2026-11-23T23:57:39Z", "content": "<SYSTEM_MESSAGE>\n[Message] timestamp=2026-11-23T23:57:39Z sender=main priority=MESSAGE_PRIORITY_HIGH content=Reply with pong.\n</SYSTEM_MESSAGE>" }),
                json!({ "type": "PLANNER_RESPONSE", "status": "DONE", "created_at": "2026-11-23T23:57:43Z", "model": "gemini-4-neon-medium", "input_tokens": 7, "output_tokens": 3, "content": "Sent pong to caller." }),
            ],
        );
        let all = agentcli::agy_conversations(&root, &wt, 0);
        let mut subs = Vec::new();
        agy_subagents(&root, &all, "main", epoch("2026-11-23T23:58:00Z").unwrap(), 1, &mut subs);
        assert_eq!(subs.len(), 1);
        let s = &subs[0];
        assert_eq!((s["id"].as_str(), s["name"].as_str(), s["task"].as_str(), s["state"].as_str()), (Some("kid"), Some("research"), Some("Ping Responder"), Some("done")));
        assert_eq!((s["tokens"].as_i64(), s["model"].as_str(), s["words"].as_str()), (Some(10), Some("Gemini 4 Neon"), Some("Sent pong to caller.")));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_pi_agents_session_is_found_by_its_id_or_its_folder() {
        let root = std::env::temp_dir().join(format!("wh-subagents-pi-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let cwd = PathBuf::from("/Users/you/git/api");
        let dir = agentcli::pi_project(&root, &cwd);
        assert_eq!(dir.file_name().unwrap(), "--Users-you-git-api--");
        std::fs::create_dir_all(&dir).unwrap();
        let named = dir.join("2026-11-25T14-01-08-166Z_01a21302-9d46-70b8-8140-c80e75e0a0bb.jsonl");
        std::fs::write(&named, "{}\n").unwrap();
        // A session resumed from another folder stays in its first one.
        let elsewhere = agentcli::pi_project(&root, Path::new("/Users/you/git/web"));
        std::fs::create_dir_all(&elsewhere).unwrap();
        let moved = elsewhere.join("2026-11-25T09-00-00-000Z_old-1.jsonl");
        std::fs::write(&moved, "{}\n").unwrap();
        std::fs::write(dir.join("notes.txt"), "").unwrap();

        let es = [
            entry(
                "claim",
                "2026-11-25T14:00:00Z",
                json!({ "session": "01a21302-9d46-70b8-8140-c80e75e0a0bb", "source": "startup", "agent": { "app": "pi", "block": "b1", "pane": "D-3", "cwd": "/Users/you/git/api" } }),
            ),
            entry(
                "claim",
                "2026-11-25T14:00:00Z",
                json!({ "session": "old-1", "source": "resume", "agent": { "app": "pi", "block": "b2", "pane": "D-4", "cwd": "/Users/you/git/api" } }),
            ),
        ];
        let cs = claims(&es);
        assert_eq!(cs.iter().map(|c| c.app.as_str()).collect::<Vec<_>>(), ["pi", "pi"]);
        let mut f = Finder { conn: None, agy_root: root.join("none"), pi_root: root.clone(), claude: HashMap::new(), codex: None };
        let mut paths: Vec<PathBuf> = f.transcripts(&cs).into_iter().map(|t| t.path).collect();
        paths.sort();
        let mut want = vec![named.clone(), moved.clone()];
        want.sort();
        assert_eq!(paths, want);

        // No extension, so no session on the claim: the folder's, begun while it held the pane.
        let unnamed = [entry("claim", "2026-01-01T00:00:00Z", json!({ "source": "startup", "agent": { "app": "pi", "block": "b3", "pane": "D-5", "cwd": "/Users/you/git/api" } }))];
        let found: Vec<PathBuf> = f.transcripts(&claims(&unnamed)).into_iter().map(|t| t.path).collect();
        assert_eq!(found, vec![named]);
        assert_eq!(agentcli::pi_session_id(Path::new("x/notes.jsonl")), None);
        let _ = std::fs::remove_dir_all(&root);
    }
}
