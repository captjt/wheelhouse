//! The Rex runtime: a thin wrapper over the `rex` CLI, and the follower
//! that mirrors the local Rex server. Wheelhouse talks to Rex only through
//! the documented CLI, so it keeps working as Rex's wire protocol changes.
//! (Ported from rex-inbox.)

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::process::Stdio;
use tokio::io::AsyncWriteExt;
use tokio::process::{Child, Command};

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncBufReadExt;
use tokio::io::BufReader;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tokio::time::{Instant, sleep, sleep_until};

use super::{Created, Kind};
use crate::agents;
use crate::hub::Shared;
use crate::mirror::{Changes, Mirror, Pane, Record, Session, SessionSnapshot, TERMINAL, Window};

/// Where the Rex Server app installs the CLI on macOS, for when `rex` is
/// not on PATH (a LaunchAgent's PATH is short).
const APP_REX: &str = "Library/Application Support/com.superlogical.rex/RexServer/Versions/Current/Rex Server.app/Contents/MacOS/rex";

pub fn rex_bin() -> PathBuf {
    if let Some(p) = std::env::var_os("WH_REX") {
        return PathBuf::from(p);
    }
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            let candidate = dir.join("rex");
            if candidate.is_file() {
                return candidate;
            }
        }
    }
    crate::paths::home().join(APP_REX)
}

/// The local Rex server.
#[derive(Clone, Debug, Default)]
pub struct Rex;

impl Rex {
    fn command(&self) -> Command {
        let mut c = Command::new(rex_bin());
        // Never let a pane's own location leak into the hub's calls.
        c.env_remove("REX_SESSION").env_remove("REX_BLOCK").env_remove("REX_CLIENT");
        c.arg("--autostart=false");
        c.stdin(Stdio::null()).kill_on_drop(true);
        c
    }

    async fn run(&self, args: &[&str], stdin: Option<String>) -> Result<String> {
        let mut c = self.command();
        c.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());
        if stdin.is_some() {
            c.stdin(Stdio::piped());
        }
        let mut child = c.spawn().with_context(|| format!("running {}", rex_bin().display()))?;
        if let Some(input) = stdin {
            let mut si = child.stdin.take().expect("piped stdin");
            si.write_all(input.as_bytes()).await?;
            drop(si);
        }
        // A Rex call that hangs mustn't hang the mirror, delivery or a
        // review: it's killed (kill_on_drop) after this long.
        let out = tokio::time::timeout(std::time::Duration::from_secs(30), child.wait_with_output())
            .await
            .map_err(|_| anyhow::anyhow!("rex {} took over 30s", args.first().unwrap_or(&"")))??;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            bail!("rex {}: {}", args.join(" "), err.trim());
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    async fn run_json(&self, args: &[&str], stdin: Option<String>) -> Result<Value> {
        let out = self.run(args, stdin).await?;
        let out = out.trim();
        if out.is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_str(out).with_context(|| format!("rex {}: bad JSON", args.join(" ")))
    }

    /// `rex api call [-s SESSION] METHOD -`, with `params` on stdin.
    pub async fn api(&self, session: Option<&str>, method: &str, params: &Value) -> Result<Value> {
        let mut args = vec!["api", "call"];
        if let Some(s) = session {
            args.extend(["-s", s]);
        }
        args.extend([method, "-"]);
        self.run_json(&args, Some(params.to_string())).await
    }

    /// `rex block call -s SESSION -b BLOCK METHOD ARGS`.
    pub async fn block_call(&self, session: &str, block: &str, method: &str, params: &Value) -> Result<Value> {
        let p = params.to_string();
        self.run_json(&["block", "call", "-s", session, "-b", block, method, &p], None).await
    }

    /// `rex focus` plus the app's `session.select`, so the pane is in front.
    pub async fn focus(&self, session: &str, block: &str, window: Option<&str>) -> Result<()> {
        self.run(&["focus", "-s", session, block], None).await?;
        let sel = format!("session_id={session}");
        let mut args = vec!["do", "session.select", &sel];
        let win;
        if let Some(w) = window {
            win = format!("window_id={w}");
            args.push(&win);
        }
        // No app connected is fine: the pane is still focused in Rex.
        let _ = self.run(&args, None).await;
        Ok(())
    }

    /// Paste text into a terminal, the way a person pastes: bracketed, so
    /// newlines don't submit early.
    pub async fn paste(&self, session: &str, block: &str, text: &str) -> Result<()> {
        let text = super::plain(text);
        let params = serde_json::json!({ "block_id": block, "event": { "type": "paste", "payload": { "text": text } } });
        self.api(Some(session), "block.event", &params).await.map(|_| ())
    }

    /// Start `argv` in a new tab of a session, named `label`, in `cwd`,
    /// without taking focus. With no session id, make the session named
    /// `name` first. (From rex-inbox.)
    pub async fn start(&self, session: Option<&str>, name: &str, label: &str, cwd: &str, argv: &[&str]) -> Result<Created> {
        let created = match session {
            Some(sid) => {
                let mut args = vec!["run", "-s", sid, "--label", label, "-c", cwd, "--focus=false", "--json", "--"];
                args.extend_from_slice(argv);
                let v = self.run_json(&args, None).await?;
                Created {
                    session_id: sid.to_string(),
                    window_id: v["window_id"].as_str().map(str::to_string),
                    block_id: v["block_ids"][0].as_str().context("rex run: no block")?.to_string(),
                }
            }
            None => {
                let mut args = vec!["new", name, "-c", cwd, "--window", label, "--json", "--"];
                args.extend_from_slice(argv);
                let v = self.run_json(&args, None).await?;
                let w = &v["initial_windows"][0];
                let c = Created {
                    session_id: v["session_id"].as_str().context("rex new: no session")?.to_string(),
                    window_id: w["window_id"].as_str().map(str::to_string),
                    block_id: w["block_ids"][0].as_str().context("rex new: no block")?.to_string(),
                };
                self.run(&["block", "rename", "-s", &c.session_id, &c.block_id, label], None).await?;
                c
            }
        };
        if let Some(w) = &created.window_id {
            let _ = self.run(&["window", "rename", "-s", &created.session_id, w, label], None).await;
        }
        Ok(created)
    }

    /// Start `argv` in an existing session: in a new tab named `label`, or
    /// split beside `anchor` (`right`, `left`, `above`, `below`).
    pub async fn run_in(&self, session: &str, label: &str, cwd: &str, argv: &[String], split: Option<(&str, &str)>) -> Result<Created> {
        let mut args: Vec<&str> = vec!["run", "-s", session, "--label", label, "-c", cwd, "--focus=false", "--json"];
        if let Some((dir, anchor)) = split {
            args.extend(["--split", dir, "-b", anchor]);
        }
        args.push("--");
        args.extend(argv.iter().map(String::as_str));
        let v = self.run_json(&args, None).await?;
        let c = Created {
            session_id: session.to_string(),
            window_id: v["window_id"].as_str().map(str::to_string),
            block_id: v["block_ids"][0].as_str().context("rex run: no block")?.to_string(),
        };
        if split.is_none()
            && let Some(w) = &c.window_id
        {
            let _ = self.run(&["window", "rename", "-s", session, w, label], None).await;
        }
        Ok(c)
    }

    /// `rex block close -s SESSION BLOCK`.
    pub async fn close_block(&self, session: &str, block: &str) -> Result<()> {
        self.run(&["block", "close", "-s", session, block], None).await.map(|_| ())
    }

    /// `rex block rename -s SESSION BLOCK NAME`.
    pub async fn rename_block(&self, session: &str, block: &str, name: &str) -> Result<()> {
        self.run(&["block", "rename", "-s", session, block, name], None).await.map(|_| ())
    }

    /// `rex send-key -s SESSION -b BLOCK KEY...`.
    pub async fn send_keys(&self, session: &str, block: &str, keys: &[&str]) -> Result<()> {
        let mut args = vec!["send-key", "-s", session, "-b", block];
        args.extend_from_slice(keys);
        self.run(&args, None).await.map(|_| ())
    }

    /// The last `lines` lines of a terminal, as plain text.
    pub async fn capture(&self, session: &str, block: &str, lines: usize) -> Result<String> {
        let out = self.run(&["capture", "-s", session, "-b", block, "--trim"], None).await?;
        let all: Vec<&str> = out.lines().collect();
        Ok(all[all.len().saturating_sub(lines)..].join("\n"))
    }

    /// Start `rex events --all --json`. Read lines from its stdout.
    pub fn events(&self) -> Result<Child> {
        let mut c = self.command();
        c.args(["events", "--all", "--json"]).stdout(Stdio::piped()).stderr(Stdio::null());
        c.spawn().with_context(|| format!("running {}", rex_bin().display()))
    }

    /// Paste, a moment for the program to take it, then Enter.
    pub async fn submit(&self, session: &str, block: &str, text: &str) -> Result<()> {
        self.paste(session, block, text).await?;
        // Pasted: from here, a failure is Enter's, retried alone. Pasting
        // again would put the text in the agent's box twice.
        let mut last = None;
        for wait in [150, 300, 600] {
            tokio::time::sleep(std::time::Duration::from_millis(wait)).await;
            match self.send_keys(session, block, &["enter"]).await {
                Ok(()) => return Ok(()),
                Err(e) => last = Some(e),
            }
        }
        Err(super::PastedNoEnter(last.map(|e| format!("{e:#}")).unwrap_or_default()).into())
    }

    /// The pane's program status records (OSC 7501), read now.
    pub async fn records(&self, session: &str, block: &str) -> Result<Vec<Record>> {
        let v = self.block_call(session, block, "program_status", &json!({})).await?;
        Ok(records_of(&v))
    }

    pub async fn session_list(&self) -> Result<Value> {
        self.api(None, "session.list", &json!({})).await
    }
}

/// The records in a `program_status` answer.
fn records_of(v: &Value) -> Vec<Record> {
    v["records"].as_array().map(|a| a.iter().filter_map(|r| serde_json::from_value(r.clone()).ok()).collect()).unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Following Rex

/// Follow the local Rex server for as long as the hub runs: a snapshot,
/// then its event stream, with the agent dockets reconciled on every
/// change. Lost, it's followed again, backing off to 30s.
pub async fn follow(shared: Shared) {
    let rex = Rex;
    let mut backoff = Duration::from_secs(1);
    loop {
        let started = std::time::Instant::now();
        let err = follow_once(&shared, &rex).await.err().map(|e| format!("{e:#}"));
        {
            let mut core = shared.lock();
            core.mirror.set_link(Kind::Rex, false, err.clone(), rex_bin().is_file());
            core.live_changed();
        }
        say!("lost Rex: {}", err.as_deref().unwrap_or("event stream ended"));
        if started.elapsed() > Duration::from_secs(60) {
            backoff = Duration::from_secs(1);
        }
        sleep(backoff).await;
        backoff = (backoff * 2).min(Duration::from_secs(30));
    }
}

async fn follow_once(shared: &Shared, rex: &Rex) -> Result<()> {
    // Start the stream before the snapshot, so nothing falls between them.
    let mut child = rex.events()?;
    let stdout = child.stdout.take().context("events stdout")?;
    let mut lines = BufReader::new(stdout).lines();

    let snap = snapshot(rex, None).await?;
    {
        let mut core = shared.lock();
        let mut m = Mirror::default();
        apply_snapshot(&mut m, snap);
        core.mirror.replace(Kind::Rex, m);
        core.mirror.set_link(Kind::Rex, true, None, true);
        // Every Rex pane now, and every agent in Rex the record thinks is
        // open, so dockets whose panes went away while the hub was down are
        // released. Other runtimes' agents are their followers'.
        let mut all: BTreeSet<String> = core.mirror.panes.values().filter(|p| p.runtime == Kind::Rex).map(|p| p.block_id.clone()).collect();
        all.extend(core.agents.by_block.keys().filter(|b| super::of(b) == Kind::Rex).cloned());
        agents::reconcile(&mut core, all);
    }
    say!("following Rex");

    let mut pending: BTreeSet<String> = BTreeSet::new();
    let mut pending_all = false;
    let mut deadline: Option<Instant> = None;
    loop {
        tokio::select! {
            line = lines.next_line() => {
                let Some(line) = line? else { bail!("event stream ended") };
                let Ok(ev) = serde_json::from_str::<Value>(&line) else { continue };
                let mut core = shared.lock();
                let changes = apply_event(&mut core.mirror, &ev);
                if !changes.panes.is_empty() {
                    agents::reconcile(&mut core, changes.panes);
                }
                if !changes.resync.is_empty() || changes.resync_all {
                    pending.extend(changes.resync);
                    pending_all |= changes.resync_all;
                    deadline.get_or_insert(Instant::now() + Duration::from_millis(300));
                }
            }
            _ = async { sleep_until(deadline.unwrap()).await }, if deadline.is_some() => {
                deadline = None;
                let only = if std::mem::take(&mut pending_all) { None } else { Some(std::mem::take(&mut pending)) };
                pending.clear();
                if let Err(e) = resync(shared, rex, only).await {
                    say!("resync: {e:#}");
                }
            }
        }
    }
}

/// One pane's status in a snapshot: `(session, block, records, process, pwd)`.
type PaneStatus = (String, String, Vec<Record>, Option<Value>, Option<Value>);

struct Snapshot {
    /// `None` when only some sessions were fetched.
    all_sessions: Option<BTreeSet<String>>,
    sessions: Vec<SessionSnapshot>,
    statuses: Vec<PaneStatus>,
}

fn apply_snapshot(m: &mut Mirror, snap: Snapshot) -> BTreeSet<String> {
    let mut changed = BTreeSet::new();
    if let Some(live) = &snap.all_sessions {
        changed.extend(m.retain_sessions(Kind::Rex, live).panes);
    }
    for s in snap.sessions {
        changed.extend(m.apply_session(s).panes);
    }
    for (sid, bid, records, process, pwd) in snap.statuses {
        m.apply_pane_status(&sid, &bid, records, process.as_ref(), pwd.as_ref());
        changed.insert(bid);
    }
    changed
}

/// Fetch sessions (all, or `only`), their layouts, and the status of their
/// terminals. With `known`, status is fetched just for panes new to the mirror.
async fn snapshot(rex: &Rex, only: Option<(BTreeSet<String>, BTreeSet<String>)>) -> Result<Snapshot> {
    let (wanted, all_sessions, known) = match only {
        Some((sessions, known)) => (sessions.into_iter().collect::<Vec<_>>(), None, Some(known)),
        None => {
            let list = rex.session_list().await?;
            let ids: Vec<String> = list["sessions"].as_array().map(|a| a.iter().filter_map(|s| s["session_id"].as_str().map(str::to_string)).collect()).unwrap_or_default();
            let all = ids.iter().cloned().collect();
            (ids, Some(all), None)
        }
    };

    let limit = Arc::new(Semaphore::new(8));
    let mut set = JoinSet::new();
    for sid in wanted {
        let limit = limit.clone();
        let rex = rex.clone();
        set.spawn(async move {
            let _permit = limit.acquire_owned().await;
            fetch_session(&rex, &sid).await
        });
    }
    let mut sessions = Vec::new();
    while let Some(r) = set.join_next().await {
        match r? {
            Ok(s) => sessions.push(s),
            // A session can vanish between list and view.
            Err(e) => say!("{e:#}"),
        }
    }

    let mut set = JoinSet::new();
    for s in &sessions {
        for (bid, _, creator, _) in &s.blocks {
            if creator != TERMINAL || known.as_ref().is_some_and(|k| k.contains(bid)) {
                continue;
            }
            let (rex, sid, bid, limit) = (rex.clone(), s.session.session_id.clone(), bid.clone(), limit.clone());
            set.spawn(async move {
                let _permit = limit.acquire_owned().await;
                let status = rex.records(&sid, &bid).await;
                let process = rex.block_call(&sid, &bid, "process", &json!({})).await.ok();
                let pwd = rex.block_call(&sid, &bid, "pwd", &json!({})).await.ok();
                (sid, bid, status, process, pwd)
            });
        }
    }
    let mut statuses = Vec::new();
    while let Some(r) = set.join_next().await {
        let (sid, bid, status, process, pwd) = r?;
        let Ok(records) = status else { continue };
        statuses.push((sid, bid, records, process, pwd));
    }
    Ok(Snapshot { all_sessions, sessions, statuses })
}

async fn fetch_session(rex: &Rex, sid: &str) -> Result<SessionSnapshot> {
    let view = rex.api(Some(sid), "session.view", &json!({})).await?;
    let blocks = rex.api(Some(sid), "session.list_blocks", &json!({})).await?;
    let windows = view["windows"]
        .as_array()
        .map(|a| a.iter().filter_map(|w| Some(Window { window_id: w["window_id"].as_str()?.to_string(), label: w["label"].as_str().unwrap_or_default().to_string() })).collect())
        .unwrap_or_default();
    let blocks = blocks["blocks"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|b| {
                    Some((
                        b["block_id"].as_str()?.to_string(),
                        b["label"].as_str().unwrap_or_default().to_string(),
                        b["creator_name"].as_str().unwrap_or_default().to_string(),
                        b["window_id"].as_str().map(str::to_string),
                    ))
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(SessionSnapshot { session: Session { runtime: Kind::Rex, session_id: sid.to_string(), label: view["label"].as_str().unwrap_or_default().to_string(), windows }, blocks })
}

/// Fetch some sessions again (or the whole list, with `None`). Status is
/// fetched only for panes new to the mirror; events keep the rest current.
async fn resync(shared: &Shared, rex: &Rex, only: Option<BTreeSet<String>>) -> Result<()> {
    let known: BTreeSet<String> = shared.lock().mirror.panes.values().filter(|p| p.runtime == Kind::Rex).map(|p| p.block_id.clone()).collect();
    let sessions = match only {
        Some(s) => s,
        None => {
            let list = rex.session_list().await?;
            let ids: BTreeSet<String> = list["sessions"].as_array().map(|a| a.iter().filter_map(|s| s["session_id"].as_str().map(str::to_string)).collect()).unwrap_or_default();
            let mut core = shared.lock();
            let changed = core.mirror.retain_sessions(Kind::Rex, &ids).panes;
            agents::reconcile(&mut core, changed);
            ids
        }
    };
    let snap = snapshot(rex, Some((sessions, known))).await?;
    let mut core = shared.lock();
    let changed = apply_snapshot(&mut core.mirror, snap);
    agents::reconcile(&mut core, changed);
    Ok(())
}

/// Apply one line of `rex events --all --json`.
pub fn apply_event(m: &mut Mirror, ev: &Value) -> Changes {
    let mut changes = Changes::default();
    let typ = ev["type"].as_str().unwrap_or_default();
    let payload = &ev["payload"];
    match typ {
        "session_event" => {
            let Some(sid) = s(&payload["session_id"]) else { return changes };
            let inner = &payload["event"];
            let p = &inner["payload"];
            match inner["type"].as_str().unwrap_or_default() {
                "block_event" => apply_block_event(m, &sid, p, &mut changes),
                "block_closed" => {
                    if let Some(b) = s(&p["block_id"]) {
                        m.panes.remove(&b);
                        changes.panes.insert(b);
                    }
                }
                "block_created" | "session_view_changed" | "session_label_changed" | "window_created" | "window_closed" | "window_label_changed" | "active_window_changed" => {
                    changes.resync.insert(sid);
                }
                "session_destroyed" => changes.resync_all = true,
                _ => {}
            }
        }
        t if t.starts_with("session_") => changes.resync_all = true,
        _ => {}
    }
    changes
}

fn apply_block_event(m: &mut Mirror, sid: &str, p: &Value, changes: &mut Changes) {
    let Some(block) = s(&p["block_id"]) else { return };
    if p["creator_name"].as_str() != Some(TERMINAL) {
        return;
    }
    let name = p["name"].as_str().unwrap_or_default();
    let body = &p["payload"];
    let known = m.panes.contains_key(&block);
    let pane = m.panes.entry(block.clone()).or_insert_with(|| Pane::new(sid, &block));
    if !known {
        changes.resync.insert(sid.to_string());
    }
    match name {
        "program_status_changed" => {
            let Ok(mut rec) = serde_json::from_value::<Record>(body["record"].clone()) else { return };
            rec.id = body["id"].as_str().unwrap_or_default().to_string();
            // Events queued while a snapshot was taken can be older than it.
            if let Some(old) = pane.records.get(&rec.id) {
                let t = |r: &Record| r.updated_at.as_deref().and_then(crate::time::parse);
                if matches!((t(old), t(&rec)), (Some(a), Some(b)) if b < a) {
                    return;
                }
            }
            if pane.records.get(&rec.id) != Some(&rec) {
                pane.records.insert(rec.id.clone(), rec);
                changes.panes.insert(block);
            }
        }
        "program_status_removed" => {
            match body["ids"].as_array() {
                Some(ids) => {
                    for id in ids.iter().filter_map(Value::as_str) {
                        pane.records.remove(id);
                    }
                }
                None => pane.records.clear(),
            }
            changes.panes.insert(block);
        }
        "child_exited" => {
            pane.set_reported_cwd(None);
            changes.panes.insert(block);
        }
        "process_changed" => {
            if let Some(fg) = body.get("foreground") {
                pane.set_foreground(fg);
            }
            changes.panes.insert(block);
        }
        "pwd_changed" => {
            if let Some(pwd) = body["pwd"].as_str() {
                pane.set_reported_cwd(Some(pwd));
            }
        }
        _ => {}
    }
}

fn s(v: &Value) -> Option<String> {
    v.as_str().map(str::to_string)
}
