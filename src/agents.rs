//! The Agent protocol: every agent in Rex is on a docket.
//!
//! - An agent pane that no docket holds gets a new one, named after the pane
//!   until you ask the agent something.
//! - Its hooks (`wh hook`) say which session it's in, what you asked it, and
//!   what it said when a turn ended. `/clear` starts a new session, so a new
//!   docket; `--resume` goes back to the docket that session was on, and
//!   reopens it, in the same pane or a new one.
//! - Rex's program status says when it's blocked or failed, and when a turn
//!   ended, for agents without hooks.
//! - When the pane closes or the agent exits, the docket is released and done.

use anyhow::Result;
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use crate::hub::{Core, Who};
use crate::mirror::{Pane, app_name};
use crate::record::fold::AgentRef;

/// How long an agent may be gone before its docket is released: long enough
/// for a restart in the same pane.
const GRACE: Duration = Duration::from_secs(6);
/// How long a turn's end waits for the other half (the hook's message, or
/// the status change) before it's written alone.
const TURN_WAIT: Duration = Duration::from_secs(3);
/// Titles are one line, about this long.
const TITLE_CHARS: usize = 80;

#[derive(Debug, Clone)]
pub struct Tracked {
    pub docket: String,
    #[allow(dead_code)]
    pub app: String,
    pub session: Option<String>,
    /// The pane's most urgent state when last seen.
    state: Option<String>,
    /// The root record's state when last seen, for turns.
    root: Option<String>,
    /// The blocked record already written, by its report time.
    blocked_seen: Option<String>,
    missing_since: Option<Instant>,
    /// A prompt Wheelhouse already recorded (one it started the agent with):
    /// the agent's own hook reports it again, once.
    pending_prompt: Option<String>,
}

impl Tracked {
    fn on(docket: &str, app: &str, session: Option<String>) -> Self {
        Tracked { docket: docket.to_string(), app: app.to_string(), session, state: None, root: None, blocked_seen: None, missing_since: None, pending_prompt: None }
    }
}

struct PendingTurn {
    since: Instant,
    status: Option<String>,
    message: Option<String>,
}

#[derive(Default)]
pub struct Tracker {
    pub by_block: BTreeMap<String, Tracked>,
    turns: BTreeMap<String, PendingTurn>,
    /// Panes answering a prompt you sent from the app: the reply is pushed
    /// to you, since you're away from the terminal.
    pub remote: std::collections::HashSet<String>,
}

/// Pick up the dockets agents held when the hub last stopped: open, with
/// agents on the record that haven't left. That's the hub's own Agent
/// dockets, and dockets you opened and put agents on (wh start --docket,
/// wh join, or a resumed session). Review's and Build's panes come back
/// through their own maps. When two dockets claim one pane, the one with a
/// session wins over one without: a docket opened for the pane before it
/// was found.
pub fn load(core: &mut Core) -> Result<()> {
    for st in core.store.states()? {
        if !st.is_open() || matches!(st.protocol.as_str(), "review" | "build") || core.store.build_task(&st.id)?.is_some() {
            continue;
        }
        for a in st.agents_on() {
            if core.agents.by_block.get(&a.agent.block).is_some_and(|cur| cur.session.is_some() || a.session.is_none()) {
                continue;
            }
            core.agents.by_block.insert(a.agent.block.clone(), Tracked::on(&st.id, &a.agent.app, a.session.clone()));
        }
    }
    Ok(())
}

/// Bring the dockets in line with these panes, as Rex has them now.
pub fn reconcile(core: &mut Core, blocks: impl IntoIterator<Item = String>) {
    let mut agents = false;
    for b in blocks {
        // A shell running commands in some pane isn't news to the app.
        agents |= core.agents.by_block.contains_key(&b) || core.mirror.panes.get(&b).is_some_and(|p| p.is_agent());
        if let Err(e) = reconcile_one(core, &b) {
            say!("agents: {b}: {e:#}");
        }
    }
    if agents {
        core.live_changed();
    }
}

fn reconcile_one(core: &mut Core, block: &str) -> Result<()> {
    let pane = core.mirror.panes.get(block).filter(|p| p.is_agent()).cloned();
    let tracked = core.agents.by_block.contains_key(block);
    match (pane, tracked) {
        (Some(p), false) => {
            let a = agent_ref(core, &p);
            if let Some((docket, role)) = crate::review::pane_docket(core, &p).or_else(|| crate::build::pane_docket(core, &p).map(|(d, r)| (d, format!("Build {r}")))) {
                adopt(core, &docket, &a, None);
                core.append(&docket, "claim", &Who::Hub, None, json!({ "source": "wheelhouse", "role": role, "agent": a }))?;
                return status(core, &p);
            }
            open_for(core, a, None, None)?;
            status(core, &p)
        }
        (Some(p), true) => {
            if let Some(t) = core.agents.by_block.get_mut(block) {
                t.missing_since = None;
            }
            status(core, &p)
        }
        (None, true) => {
            if let Some(t) = core.agents.by_block.get_mut(block) {
                t.missing_since.get_or_insert_with(Instant::now);
            }
            Ok(())
        }
        (None, false) => Ok(()),
    }
}

/// Every second: write turns that waited long enough, and release agents
/// that have been gone past the grace period.
/// Every second: turns that ended are written, and agents gone past the
/// grace are released, whichever runtime they were in.
pub async fn tick_loop(shared: crate::hub::Shared) {
    let mut t = tokio::time::interval(std::time::Duration::from_secs(1));
    loop {
        t.tick().await;
        tick(&mut shared.lock());
    }
}

pub fn tick(core: &mut Core) {
    let due: Vec<String> = core.agents.turns.iter().filter(|(_, t)| t.since.elapsed() >= TURN_WAIT).map(|(b, _)| b.clone()).collect();
    for b in due {
        if let Err(e) = flush_turn(core, &b) {
            say!("agents: turn {b}: {e:#}");
        }
    }
    let gone: Vec<String> = core.agents.by_block.iter().filter(|(_, t)| t.missing_since.is_some_and(|m| m.elapsed() >= GRACE)).map(|(b, _)| b.clone()).collect();
    for b in gone {
        let reason = if core.mirror.panes.contains_key(&b) { "agent exited" } else { "pane closed" };
        if let Err(e) = release(core, &b, reason) {
            say!("agents: release {b}: {e:#}");
        }
    }
}

fn agent_ref(core: &Core, p: &Pane) -> AgentRef {
    AgentRef {
        app: p.app().unwrap_or("agent").to_string(),
        host: "local".into(),
        block: p.block_id.clone(),
        rex_session: core.mirror.session_label(&p.session_id).to_string(),
        pane: p.label.clone(),
        cwd: p.cwd.as_deref().map(crate::paths::tilde),
    }
}

fn default_title(a: &AgentRef) -> String {
    let place = match a.cwd.as_deref() {
        Some(cwd) => {
            let parts: Vec<&str> = cwd.trim_end_matches('/').rsplit('/').take(2).collect();
            parts.into_iter().rev().collect::<Vec<_>>().join("/")
        }
        None => a.rex_session.clone(),
    };
    format!("{} in {place}", app_name(&a.app))
}

fn open_for(core: &mut Core, a: AgentRef, session: Option<&str>, source: Option<&str>) -> Result<String> {
    let data = json!({
        "protocol": "agent",
        "title_source": "pane",
        "agent": a,
        "session": session,
        "source": source,
    });
    let id = core.open(&Who::Hub, &default_title(&a), data)?;
    core.agents.by_block.insert(a.block.clone(), Tracked::on(&id, &a.app, session.map(str::to_string)));
    Ok(id)
}

/// Track a pane Wheelhouse started on a docket it already opened.
pub fn adopt(core: &mut Core, docket: &str, a: &AgentRef, prompt: Option<&str>) {
    core.agents.by_block.insert(a.block.clone(), Tracked { pending_prompt: prompt.map(str::to_string), ..Tracked::on(docket, &a.app, None) });
}

/// Write what changed in a tracked pane's program status.
fn status(core: &mut Core, p: &Pane) -> Result<()> {
    let Some(t) = core.agents.by_block.get(&p.block_id).cloned() else { return Ok(()) };
    let head = p.headline().cloned();
    let state = head.as_ref().map(|r| r.state.clone());
    let root = p.root().map(|r| r.state.clone());

    if let Some(h) = head.as_ref().filter(|h| matches!(h.state.as_str(), "blocked" | "error")) {
        let seen = h.updated_at.clone().or_else(|| h.msg.clone());
        if state != t.state || seen != t.blocked_seen {
            let data = json!({ "what": h.state, "kind": h.kind, "record": h.id, "title": h.title, "block": p.block_id });
            core.append(&t.docket, "observed", &Who::Hub, h.msg.clone(), data)?;
            let _ = core.events.send(json!({ "type": "needs", "docket": t.docket, "block": p.block_id, "what": h.state, "updated_at": h.updated_at }));
            if let Some(tt) = core.agents.by_block.get_mut(&p.block_id) {
                tt.blocked_seen = seen;
            }
        }
    }
    if t.root.as_deref() == Some("working") && matches!(root.as_deref(), Some("done" | "idle")) {
        turn_status(core, &p.block_id, root.as_deref().unwrap_or("done"))?;
    }
    if let Some(tt) = core.agents.by_block.get_mut(&p.block_id) {
        tt.state = state;
        tt.root = root;
    }
    Ok(())
}

fn turn_status(core: &mut Core, block: &str, state: &str) -> Result<()> {
    match core.agents.turns.get_mut(block) {
        Some(pt) if pt.message.is_some() && pt.status.is_none() => {
            pt.status = Some(state.to_string());
            flush_turn(core, block)
        }
        _ => {
            flush_turn(core, block)?;
            core.agents.turns.insert(block.to_string(), PendingTurn { since: Instant::now(), status: Some(state.into()), message: None });
            Ok(())
        }
    }
}

fn turn_message(core: &mut Core, block: &str, message: &str) -> Result<()> {
    match core.agents.turns.get_mut(block) {
        Some(pt) if pt.status.is_some() && pt.message.is_none() => {
            pt.message = Some(message.to_string());
            flush_turn(core, block)
        }
        _ => {
            flush_turn(core, block)?;
            core.agents.turns.insert(block.to_string(), PendingTurn { since: Instant::now(), status: None, message: Some(message.into()) });
            Ok(())
        }
    }
}

fn flush_turn(core: &mut Core, block: &str) -> Result<()> {
    let Some(pt) = core.agents.turns.remove(block) else { return Ok(()) };
    let Some(t) = core.agents.by_block.get(block) else { return Ok(()) };
    let docket = t.docket.clone();
    let summary = pt.message.as_deref().and_then(summarize);
    let links = pt.message.clone();
    let reply = core
        .agents
        .remote
        .remove(block)
        .then(|| summary.clone().or_else(|| pt.message.as_deref().map(|m| m.lines().find(|l| !l.trim().is_empty()).unwrap_or("").chars().take(200).collect())));
    core.append(&docket, "turn", &Who::Hub, pt.message, json!({ "state": pt.status, "summary": summary, "block": block }))?;
    // You asked from the app: the reply comes to your phone.
    if let Some(text) = reply {
        let _ = core.events.send(json!({ "type": "turn", "docket": docket, "block": block, "text": text }));
    }
    // A Claude artifact the turn names is kept with the docket, as a link.
    if let Some(text) = links
        && let Err(e) = crate::artifacts::keep_links(core, &docket, &text)
    {
        say!("{docket}: keeping the turn's links: {e:#}");
    }
    Ok(())
}

fn release(core: &mut Core, block: &str, reason: &str) -> Result<()> {
    release_as(core, block, reason, "done")
}

/// The pane leaves its docket. When it was the last agent on it, the
/// docket is released and set to `status`.
fn release_as(core: &mut Core, block: &str, reason: &str, status: &str) -> Result<()> {
    release_only(core, block, reason)?.map_or(Ok(()), |d| {
        // A Review's or a Build's docket status is its protocol's to set: a
        // Review docket outlives any one of its panes, and ends when the PR's
        // review is over and no pane of it is left.
        let st = core.store.state(&d)?;
        let ours = st.as_ref().is_none_or(|s| s.protocol != "review") && core.store.build_task(&d)?.is_none();
        let last = st.as_ref().is_none_or(|s| s.agents_on().next().is_none());
        if ours && last {
            core.append(&d, "set", &Who::Hub, None, json!({ "status": status }))?;
        }
        Ok(())
    })
}

/// The pane leaves its docket, which keeps its status. Returns the docket.
fn release_only(core: &mut Core, block: &str, reason: &str) -> Result<Option<String>> {
    flush_turn(core, block)?;
    let Some(t) = core.agents.by_block.remove(block) else { return Ok(None) };
    core.append(&t.docket, "release", &Who::Hub, None, json!({ "reason": reason, "block": block }))?;
    Ok(Some(t.docket))
}

/// Whether a docket's panes are a protocol's (Review's, Build's), which
/// starts and ends them itself.
fn protocols(core: &Core, docket: &str) -> Result<Option<&'static str>> {
    if core.store.build_task(docket)?.is_some() {
        return Ok(Some("Build"));
    }
    Ok(match core.store.state(docket)?.map(|s| s.protocol) {
        Some(p) if p == "review" => Some("Review"),
        Some(p) if p == "build" => Some("Build"),
        _ => None,
    })
}

/// An agent already running joins a docket, beside the agents on it. Its
/// pane leaves the docket it was on: one with nothing on it yet is dropped.
/// A role, as it goes into other agents' briefs: one line, at most
/// `ROLE_CHARS`, letters, digits, spaces and `-_./`, nothing that reads as
/// the brief's own words.
const ROLE_CHARS: usize = 40;

pub fn valid_role(role: &str) -> std::result::Result<String, String> {
    let r = role.trim();
    if r.is_empty() || r.chars().count() > ROLE_CHARS || !r.chars().all(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.' | '/')) {
        return Err(format!("a role is up to {ROLE_CHARS} letters, digits, spaces, '-', '_', '.' or '/', on one line"));
    }
    Ok(r.split_whitespace().collect::<Vec<_>>().join(" "))
}

pub fn join(core: &mut Core, block: &str, docket: &str, role: Option<&str>) -> Result<Value> {
    let role = role.map(valid_role).transpose().map_err(|e| anyhow::anyhow!(e))?;
    let role = role.as_deref();
    let p = core.mirror.panes.get(block).filter(|p| p.is_agent()).cloned().ok_or_else(|| anyhow::anyhow!("wh join runs in an agent's pane"))?;
    let st = core.store.state(docket)?.ok_or_else(|| anyhow::anyhow!("no docket {docket}"))?;
    if !st.is_open() {
        anyhow::bail!("{docket} is {}: reopen it first", st.status.replace('_', " "));
    }
    if let Some(h) = &st.hold {
        anyhow::bail!("{docket} is on hold ({h}): resume it first");
    }
    if let Some(p) = protocols(core, docket)? {
        anyhow::bail!("{docket} is {p}'s: {p} puts its own agents on it");
    }
    let t = core.agents.by_block.get(block).cloned();
    if let Some(t) = &t {
        if t.docket == docket {
            return Ok(json!({ "docket": docket, "already": true, "others": st.agents_on().filter(|a| a.agent.block != block).count() }));
        }
        if let Some(p) = protocols(core, &t.docket)? {
            anyhow::bail!("this pane is {p}'s, on {}", t.docket);
        }
        let fresh = core.store.state(&t.docket)?.is_some_and(|s| s.prompts == 0 && s.turns == 0);
        release_as(core, block, &format!("joined {docket}"), if fresh { "dropped" } else { "done" })?;
    }
    let app = p.app().unwrap_or("agent").to_string();
    let session = t.and_then(|t| t.session);
    core.agents.by_block.insert(block.to_string(), Tracked::on(docket, &app, session.clone()));
    let a = agent_ref(core, &p);
    core.append(docket, "claim", &Who::Hub, None, json!({ "session": session, "source": "join", "role": role, "agent": a }))?;
    if st.status == "todo" {
        core.append(docket, "set", &Who::Hub, None, json!({ "status": "in_progress" }))?;
    }
    core.live_changed();
    Ok(json!({ "docket": docket, "others": st.agents_on().filter(|a| a.agent.block != block).count() }))
}

/// The pane leaves its docket, which stays as it is: held by the agents
/// still on it. The pane gets a docket of its own again.
pub fn leave(core: &mut Core, block: &str, reason: Option<&str>) -> Result<Value> {
    let t = core.agents.by_block.get(block).cloned().ok_or_else(|| anyhow::anyhow!("this pane isn't on a docket"))?;
    if let Some(p) = protocols(core, &t.docket)? {
        anyhow::bail!("this pane is {p}'s, on {}: {p} ends it", t.docket);
    }
    let reason = reason.map(str::trim).filter(|r| !r.is_empty()).unwrap_or("left");
    release_only(core, block, reason)?;
    // Its last agent left a docket nothing was done on: it was never work.
    // One with work keeps its status: leaving isn't finishing.
    if let Some(st) = core.store.state(&t.docket)?
        && st.agents_on().next().is_none()
        && st.prompts == 0
        && st.turns == 0
        && st.is_open()
    {
        core.append(&t.docket, "set", &Who::Hub, None, json!({ "status": "dropped" }))?;
    }
    let p = core.mirror.panes.get(block).cloned();
    let own = match p {
        Some(p) => Some(open_for(core, agent_ref(core, &p), t.session.as_deref(), Some("leave"))?),
        None => None,
    };
    core.live_changed();
    Ok(json!({ "left": t.docket, "docket": own }))
}

/// The docket a resumed session goes back to. A session is its first
/// docket, so a resume goes back there (and not to a docket of its own an
/// older hub gave it by mistake), unless the session moved on purpose since:
/// `wh join` or `wh leave` took it to another docket, and it goes back to
/// the last of those. Never a dropped docket, and not one where another live
/// pane is in that same session (resumed twice at once): that pane keeps it.
fn resumed_docket(core: &Core, block: &str, session: &str) -> Result<Option<crate::record::fold::DocketState>> {
    let n = |id: &str| id.strip_prefix("D-").and_then(|n| n.parse::<u64>().ok()).unwrap_or(0);
    // States come newest first.
    let states: Vec<_> = core.store.states()?.into_iter().filter(|st| st.status != "dropped").collect();
    let ours = |st: &crate::record::fold::DocketState| st.agents.iter().filter(|a| a.session.as_deref() == Some(session)).cloned().collect::<Vec<_>>();
    let moved = states
        .iter()
        .filter_map(|st| ours(st).into_iter().filter(|a| matches!(a.via.as_deref(), Some("join" | "leave"))).map(|a| a.joined_at).max().map(|t| (t, n(&st.id), st)))
        .max_by(|a, b| (&a.0, a.1).cmp(&(&b.0, b.1)))
        .map(|(_, _, st)| st.clone());
    let d = moved.or_else(|| states.iter().rfind(|st| !ours(st).is_empty()).cloned());
    Ok(d.filter(|d| !core.agents.by_block.iter().any(|(b, t)| b != block && t.docket == d.id && t.session.as_deref() == Some(session))))
}

/// The pane `block` as a docket records it.
fn pane_ref(core: &Core, block: &str, app: &str, cwd: Option<&str>) -> AgentRef {
    match core.mirror.panes.get(block).cloned() {
        Some(pane) => AgentRef { app: app.to_string(), ..agent_ref(core, &pane) },
        None => AgentRef { app: app.to_string(), host: "local".into(), block: block.to_string(), cwd: cwd.map(crate::paths::tilde), ..Default::default() },
    }
}

fn claim(core: &mut Core, block: &str, session: &str, source: &str) -> Result<()> {
    claim_in(core, block, session, source, None)
}

/// Claim the session for the pane's docket. `here`: the pane it's in now,
/// when that may not be the pane the docket last had.
fn claim_in(core: &mut Core, block: &str, session: &str, source: &str, here: Option<AgentRef>) -> Result<()> {
    let Some(t) = core.agents.by_block.get(block).cloned() else { return Ok(()) };
    // This pane, as the docket has it, never the docket's other agents.
    let agent = match here {
        Some(a) => a,
        None => core
            .store
            .state(&t.docket)?
            .and_then(|s| s.agents.into_iter().rev().find(|x| x.agent.block == block).map(|x| x.agent))
            .unwrap_or_else(|| pane_ref(core, block, &t.app, None)),
    };
    core.append(&t.docket, "claim", &Who::Hub, None, json!({ "session": session, "source": source, "agent": agent }))?;
    if let Some(tt) = core.agents.by_block.get_mut(block) {
        tt.session = Some(session.to_string());
    }
    Ok(())
}

/// One call from an agent's hook: `event` is the agent's name for it.
pub fn hook(core: &mut Core, event: &str, app: &str, block: &str, p: &Value) -> Result<Value> {
    let session = p["session_id"].as_str().or_else(|| p["conversationId"].as_str()).filter(|s| !s.is_empty());
    let cwd = p["cwd"].as_str();
    // Antigravity doesn't say it resumed (`agy --conversation`, `-c`): a
    // conversation another docket has, seen in this pane for the first
    // time, is one resumed here. Its first hook may be the prompt's.
    let agy_resumed = app == "agy"
        && session.is_some_and(|s| core.agents.by_block.get(block).is_none_or(|t| t.session.as_deref() != Some(s)))
        && resumed_docket(core, block, session.unwrap_or_default())?.is_some();
    if agy_resumed && event != "SessionStart" {
        hook(core, "SessionStart", app, block, p)?;
    }
    let source = p["source"].as_str().unwrap_or(if agy_resumed { "resume" } else { "startup" });
    // A resumed session goes back to the docket it was on, in whichever
    // pane it's resumed (claude --resume in a new one, after the old closed).
    let resumed = match (event, source, session) {
        ("SessionStart", "resume", Some(s)) => resumed_docket(core, block, s)?,
        _ => None,
    };

    if !core.agents.by_block.contains_key(block) {
        let a = pane_ref(core, block, app, cwd);
        match &resumed {
            Some(d) => adopt(core, &d.id, &a, None),
            None => {
                open_for(core, a, session, Some(source))?;
            }
        }
    }

    match event {
        "SessionStart" => {
            let Some(s) = session else { return Ok(json!({})) };
            let t = core.agents.by_block[block].clone();
            if let Some(d) = resumed {
                if d.id != t.docket {
                    // A docket opened for the pane, with nothing on it yet,
                    // was never work: it's dropped, not done.
                    let fresh = core.store.state(&t.docket)?.is_some_and(|st| st.prompts == 0 && st.turns == 0);
                    release_as(core, block, "resumed another session", if fresh { "dropped" } else { "done" })?;
                    core.agents.by_block.insert(block.to_string(), Tracked { docket: d.id.clone(), session: None, ..t.clone() });
                }
                if !d.is_open() {
                    core.append(&d.id, "set", &Who::Hub, None, json!({ "status": "in_progress" }))?;
                }
                if d.id != t.docket || t.session.as_deref() != Some(s) {
                    claim_in(core, block, s, source, Some(pane_ref(core, block, app, cwd)))?;
                }
                core.live_changed();
                return Ok(json!({ "docket": d.id }));
            }
            match t.session.as_deref() {
                None => claim(core, block, s, source)?,
                Some(cur) if cur == s => {}
                Some(_) => {
                    let fresh = core.store.state(&t.docket)?.is_some_and(|st| st.prompts == 0 && st.turns == 0);
                    if fresh {
                        claim(core, block, s, source)?;
                    } else {
                        release(core, block, &format!("new session ({source})"))?;
                        let a = pane_ref(core, block, app, cwd);
                        open_for(core, a, Some(s), Some(source))?;
                    }
                }
            }
        }
        "UserPromptSubmit" | "PreInvocation" => {
            // Antigravity calls PreInvocation before every model call in a
            // turn, repeating the prompt: only the first (0) is a new one.
            if event == "PreInvocation" && p["invocationNum"].as_i64().is_some_and(|n| n > 0) {
                return Ok(json!({ "docket": core.agents.by_block.get(block).map(|t| t.docket.clone()) }));
            }
            let prompt = p["prompt"].as_str().or_else(|| p["lastUserInput"].as_str()).unwrap_or_default().trim();
            if prompt.is_empty() || from_the_harness(prompt) {
                return Ok(json!({}));
            }
            if let Some(s) = session
                && core.agents.by_block[block].session.is_none()
            {
                claim(core, block, s, "startup")?;
            }
            let docket = core.agents.by_block[block].docket.clone();
            if let Some(t) = core.agents.by_block.get_mut(block)
                && t.pending_prompt.as_deref() == Some(prompt)
            {
                t.pending_prompt = None;
                return Ok(json!({ "docket": docket }));
            }
            if prompt.starts_with("[rex-inbox]") || prompt.starts_with("[wheelhouse]") {
                core.append(&docket, "note", &Who::Hub, Some(prompt.to_string()), json!({ "via": "message", "block": block }))?;
            } else {
                note_prompt(core, &docket, prompt, session, Some(block), None)?;
            }
        }
        "Stop" => {
            if p["stop_hook_active"].as_bool() == Some(true) || p["continuationActive"].as_bool() == Some(true) {
                return Ok(json!({}));
            }
            // Claude Code's Stop is a finished turn; Antigravity's also
            // fires when you cancel or it fails, with no new words.
            if matches!(p["terminationReason"].as_str(), Some("USER_CANCELED" | "ERROR")) {
                return Ok(json!({}));
            }
            let msg = p["last_assistant_message"].as_str().map(str::to_string).filter(|s| !s.trim().is_empty()).or_else(|| p["transcriptPath"].as_str().and_then(agy_last_message));
            if let Some(msg) = msg {
                turn_message(core, block, &msg)?;
            }
        }
        _ => {}
    }
    core.live_changed();
    Ok(json!({ "docket": core.agents.by_block.get(block).map(|t| t.docket.clone()) }))
}

/// The last assistant response text in an Antigravity CLI `transcript.jsonl`.
fn agy_last_message(path: &str) -> Option<String> {
    use std::io::BufRead;
    let f = std::fs::File::open(path).ok()?;
    let mut last = None;
    for line in std::io::BufReader::new(f).lines().map_while(Result::ok) {
        if !line.contains("\"PLANNER_RESPONSE\"") {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
        if v["type"] != "PLANNER_RESPONSE" {
            continue;
        }
        if let Some(text) = v["response"].as_str().or_else(|| v["content"].as_str()).filter(|s| !s.trim().is_empty()) {
            last = Some(text.to_string());
        }
    }
    last
}

/// Text Claude Code hands its agent as a prompt that nobody typed: a
/// background subagent's result, or a task's notification.
fn from_the_harness(prompt: &str) -> bool {
    ["<agent-message", "<task-notification", "<system-reminder"].iter().any(|p| prompt.starts_with(p))
}

/// A prompt you gave an agent: a note on its docket, and the docket's title
/// if it has none of its own yet. `from` says where you typed it, when that
/// wasn't the pane (`wheelhouse`). `block`: the agent's pane, on a docket
/// with several.
pub fn note_prompt(core: &mut Core, docket: &str, prompt: &str, session: Option<&str>, block: Option<&str>, from: Option<&str>) -> Result<()> {
    core.append(docket, "note", &Who::Owner, Some(prompt.to_string()), json!({ "via": "prompt", "session": session, "block": block, "from": from }))?;
    let untitled = core.store.state(docket)?.is_some_and(|st| st.title_source == "pane");
    if untitled && let Some(title) = title_from(prompt) {
        core.append(docket, "set", &Who::Hub, None, json!({ "title": title, "title_source": "prompt" }))?;
    }
    Ok(())
}

/// Whether Wheelhouse may type a message into this pane now, and if not, why.
/// Only an agent, and only between turns: never a shell (the text would run
/// as a command), never mid-turn, and never over a prompt it's waiting on.
pub fn can_send(p: &Pane) -> std::result::Result<(), String> {
    let app = app_name(p.app().unwrap_or("agent")).to_string();
    let rt = p.runtime.title();
    if !p.is_agent() {
        return Err("That pane isn't running an agent, so Wheelhouse won't type into it.".into());
    }
    match p.root().map(|r| r.state.as_str()) {
        Some("idle" | "done") => Ok(()),
        Some("working") => Err(format!("{app} is working. Send it when the turn ends.")),
        Some("blocked") => Err(format!("{app} is waiting on a prompt in its pane. Answer that in {rt} first.")),
        Some("error") => Err(format!("{app} stopped with an error. Look at its pane in {rt}.")),
        _ => Err(format!("{app} hasn't said it's ready yet.")),
    }
}

/// A docket title from a prompt: its first real line, cut at a word.
pub fn title_from(prompt: &str) -> Option<String> {
    if prompt.starts_with('/') {
        return None;
    }
    let line = prompt.lines().map(str::trim).find(|l| !l.is_empty())?;
    let line = line.trim_start_matches('#').trim();
    if line.chars().count() <= TITLE_CHARS {
        return Some(line.to_string());
    }
    let cut: String = line.chars().take(TITLE_CHARS).collect();
    let cut = match cut.rfind(' ') {
        Some(i) if i > TITLE_CHARS / 2 => &cut[..i],
        _ => cut.as_str(),
    };
    Some(format!("{}…", cut.trim_end_matches([' ', ',', ';', ':', '.', '-'])))
}

/// A one-paragraph form of an agent's last message (from rex-inbox).
pub fn summarize(text: &str) -> Option<String> {
    const CHARS: usize = 300;
    let mut parts: Vec<String> = Vec::new();
    let mut fenced = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with("```") || line.starts_with("~~~") {
            fenced = !fenced;
            continue;
        }
        if fenced || line.is_empty() || line.starts_with('|') || line.starts_with("---") {
            continue;
        }
        let line = line.trim_start_matches('#').trim_start_matches('>').trim_start();
        let line = ["- [ ] ", "- [x] ", "- ", "* ", "+ ", "• "].iter().find_map(|b| line.strip_prefix(b)).unwrap_or(line);
        let line: String = line.replace("**", "").replace("__", "").replace('`', "");
        let line = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if line.is_empty() {
            continue;
        }
        parts.push(line);
    }
    let mut out = String::new();
    for p in parts {
        if !out.is_empty() {
            out.push_str(if out.ends_with(['.', '!', '?', ':', ';', ',']) { " " } else { ". " });
        }
        out.push_str(&p);
        if out.chars().count() > CHARS {
            break;
        }
    }
    if out.is_empty() {
        return None;
    }
    if out.chars().count() > CHARS {
        let cut: String = out.chars().take(CHARS).collect();
        let cut = match cut.rfind(' ') {
            Some(i) if i > CHARS / 2 => &cut[..i],
            _ => cut.as_str(),
        };
        out = format!("{}…", cut.trim_end_matches([' ', ',', ';', ':', '.', '-']));
    }
    Some(out)
}

// ---- views ----

/// What an agent is doing now, from its runtime. Not part of the record.
#[derive(Debug, Clone, Serialize)]
pub struct Live {
    pub block: String,
    /// The runtime the pane is in: `rex`, `herdr`.
    pub runtime: crate::runtime::Kind,
    /// The record that the state comes from, for answering its prompt.
    pub record: Option<String>,
    /// Whether Wheelhouse can answer this prompt with a key.
    pub answerable: bool,
    pub app: String,
    pub state: String,
    pub kind: Option<String>,
    pub msg: Option<String>,
    pub since: Option<String>,
    pub rex_session: String,
    pub pane: String,
    pub cwd: Option<String>,
}

pub fn live(core: &Core, block: &str) -> Option<Live> {
    let p = core.mirror.panes.get(block)?;
    let h = p.headline();
    let answerable = h.is_some_and(|r| r.state == "blocked" && r.kind.as_deref() == Some("permission")) && p.app().and_then(crate::approve::prompt_keys).is_some();
    Some(Live {
        block: block.to_string(),
        runtime: p.runtime,
        record: h.map(|r| r.id.clone()),
        answerable,
        app: p.app().unwrap_or("agent").to_string(),
        state: h.map_or("starting".into(), |r| r.state.clone()),
        kind: h.and_then(|r| r.kind.clone()),
        msg: h.and_then(|r| r.msg.clone().or_else(|| r.title.clone())),
        since: h.and_then(|r| r.updated_at.clone()),
        rex_session: core.mirror.session_label(&p.session_id).to_string(),
        pane: p.label.clone(),
        cwd: p.cwd.as_deref().map(crate::paths::tilde),
    })
}

/// Of a docket's agents live now, the one the apps put first: the one that
/// needs you (blocked, then failed), then one at work, then the most
/// recently active. None when no agent of it is live.
pub fn lead(core: &Core, st: &crate::record::fold::DocketState) -> Option<String> {
    let rank = |l: &Live| match l.state.as_str() {
        "blocked" => 0,
        "error" => 1,
        "working" => 2,
        _ => 3,
    };
    st.agents_on()
        .filter(|a| docket_of(core, &a.agent.block).as_deref() == Some(st.id.as_str()))
        .filter_map(|a| Some((a, live(core, &a.agent.block)?)))
        // The lowest rank; among equals, the most recently active.
        .min_by(|(a, la), (b, lb)| rank(la).cmp(&rank(lb)).then(b.active_at.cmp(&a.active_at)))
        .map(|(a, _)| a.agent.block.clone())
}

/// The pane to act on for a docket: `want`, when it's one of the docket's
/// own, or else its lead.
///
/// A pane of the docket's that no docket tracks any more (its agent exited
/// and left a shell, or the hub hasn't found it yet) still counts, while
/// it's open: you can still read what a crashed agent last said. One that
/// moved to another docket doesn't.
pub fn pane_on(core: &Core, st: &crate::record::fold::DocketState, want: Option<&str>) -> std::result::Result<String, String> {
    let ours = |b: &str| match docket_of(core, b) {
        Some(d) => d == st.id,
        None => core.mirror.panes.contains_key(b) && st.agents.iter().any(|a| a.agent.block == b),
    };
    match want.filter(|w| !w.is_empty()) {
        Some(w) if ours(w) => Ok(w.to_string()),
        Some(w) => Err(format!("{w} isn't an agent on {}", st.id)),
        None => lead(core, st).or_else(|| st.agent.as_ref().map(|a| a.block.clone()).filter(|b| ours(b))).ok_or_else(|| format!("{} has no pane open", st.id)),
    }
}

/// The docket a pane is on, if any.
pub fn docket_of(core: &Core, block: &str) -> Option<String> {
    core.agents.by_block.get(block).map(|t| t.docket.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hub::test_core;
    use crate::mirror::Record;

    fn agent_pane(core: &mut Core, block: &str, state: Option<&str>) {
        let fg = json!({ "foreground": { "name": "claude", "cwd": "/Users/you/git/api" } });
        let recs = state.map(|s| vec![Record { state: s.into(), app: Some("claude-code".into()), ..Default::default() }]).unwrap_or_default();
        core.mirror.apply_pane_status("s1", block, recs, Some(&fg), None);
    }

    fn entries(core: &Core, docket: &str) -> Vec<String> {
        core.store.entries(docket).unwrap().iter().map(|s| s.entry().unwrap().kind).collect()
    }

    #[test]
    fn every_agent_gets_a_docket_named_by_its_first_prompt() {
        let mut core = test_core();
        agent_pane(&mut core, "b1", Some("idle"));
        reconcile(&mut core, ["b1".to_string()]);
        let d = docket_of(&core, "b1").expect("a docket");
        let st = core.store.state(&d).unwrap().unwrap();
        assert_eq!(st.protocol, "agent");
        assert_eq!(st.title_source, "pane");
        assert_eq!(st.title, "Claude Code in git/api");

        hook(&mut core, "SessionStart", "claude-code", "b1", &json!({ "session_id": "s-1", "source": "startup" })).unwrap();
        hook(&mut core, "UserPromptSubmit", "claude-code", "b1", &json!({ "session_id": "s-1", "prompt": "Fix the flaky retry test\nIt fails on CI." })).unwrap();
        hook(&mut core, "UserPromptSubmit", "claude-code", "b1", &json!({ "session_id": "s-1", "prompt": "Also run lint" })).unwrap();
        let st = core.store.state(&d).unwrap().unwrap();
        assert_eq!(st.title, "Fix the flaky retry test");
        assert_eq!(st.session.as_deref(), Some("s-1"));
        assert_eq!(st.prompts, 2);
        assert_eq!(entries(&core, &d), ["open", "claim", "note", "set", "note"]);
        assert!(core.verify(&d).unwrap().ok);
    }

    #[test]
    fn agents_in_rex_and_herdr_are_tracked_side_by_side() {
        let mut core = test_core();
        agent_pane(&mut core, "block:1", Some("idle"));
        let fg = json!({ "foreground": { "name": "claude", "cwd": "/Users/you/git/web" } });
        let rec = vec![Record { state: "blocked".into(), kind: Some("permission".into()), app: Some("claude-code".into()), ..Default::default() }];
        core.mirror.apply_pane_status("herdr:space:wheelhouse", "herdr:t2h", rec, Some(&fg), None);
        reconcile(&mut core, ["block:1".to_string(), "herdr:t2h".to_string()]);
        let rex = docket_of(&core, "block:1").expect("Rex's agent has a docket");
        let herdr = docket_of(&core, "herdr:t2h").expect("Herdr's agent has a docket");
        assert_ne!(rex, herdr);
        let l = live(&core, "herdr:t2h").unwrap();
        assert_eq!(l.runtime, crate::runtime::Kind::Herdr);
        assert!(l.answerable);
        assert_eq!(can_send(&core.mirror.panes["herdr:t2h"]).unwrap_err(), "Claude Code is waiting on a prompt in its pane. Answer that in Herdr first.");

        // Herdr went away: its agent is missing, Rex's isn't.
        let changed = core.mirror.replace(crate::runtime::Kind::Herdr, crate::mirror::Mirror::default());
        reconcile(&mut core, changed);
        assert!(core.agents.by_block["herdr:t2h"].missing_since.is_some());
        assert!(core.agents.by_block["block:1"].missing_since.is_none());
    }

    #[test]
    fn a_turn_takes_the_hooks_message_and_the_status() {
        let mut core = test_core();
        agent_pane(&mut core, "b1", Some("working"));
        reconcile(&mut core, ["b1".to_string()]);
        let d = docket_of(&core, "b1").unwrap();
        hook(&mut core, "Stop", "claude-code", "b1", &json!({ "last_assistant_message": "Fixed it. **Tests pass.**" })).unwrap();
        agent_pane(&mut core, "b1", Some("done"));
        reconcile(&mut core, ["b1".to_string()]);
        let st = core.store.state(&d).unwrap().unwrap();
        assert_eq!(st.turns, 1, "one turn, not two");
        assert_eq!(st.last_turn.as_deref(), Some("Fixed it. **Tests pass.**"));
        let turn = core.store.entries(&d).unwrap().last().unwrap().entry().unwrap();
        assert_eq!(turn.data["state"], "done");
        assert_eq!(turn.data["summary"], "Fixed it. Tests pass.");
    }

    #[test]
    fn a_reply_to_what_you_sent_from_the_app_is_pushed_and_no_other() {
        let mut core = test_core();
        let mut rx = core.events.subscribe();
        agent_pane(&mut core, "b1", Some("working"));
        reconcile(&mut core, ["b1".to_string()]);
        let d = docket_of(&core, "b1").unwrap();
        let turns = |rx: &mut tokio::sync::broadcast::Receiver<Value>| std::iter::from_fn(|| rx.try_recv().ok()).filter(|e| e["type"] == "turn").collect::<Vec<_>>();
        // A turn you started at the terminal: no push.
        hook(&mut core, "Stop", "claude-code", "b1", &json!({ "last_assistant_message": "Done at the desk." })).unwrap();
        agent_pane(&mut core, "b1", Some("done"));
        reconcile(&mut core, ["b1".to_string()]);
        assert!(turns(&mut rx).is_empty());
        // You sent it something from the app: its reply comes to you, once.
        core.agents.remote.insert("b1".into());
        agent_pane(&mut core, "b1", Some("working"));
        reconcile(&mut core, ["b1".to_string()]);
        hook(&mut core, "Stop", "claude-code", "b1", &json!({ "last_assistant_message": "**Fixed.** The retry test passes now." })).unwrap();
        agent_pane(&mut core, "b1", Some("done"));
        reconcile(&mut core, ["b1".to_string()]);
        let t = turns(&mut rx);
        assert_eq!(t.len(), 1, "{t:?}");
        assert_eq!(t[0]["docket"], d.as_str());
        assert_eq!(t[0]["text"], "Fixed. The retry test passes now.");
        assert!(core.agents.remote.is_empty());
    }

    #[test]
    fn a_prompt_queued_while_the_agent_works_waits_its_turn() {
        let mut core = test_core();
        let q1 = core.store.queue_prompt("D-1", "b1", "Also run lint").unwrap();
        let q2 = core.store.queue_prompt("D-1", "b1", "Then push").unwrap();
        core.store.queue_prompt("D-2", "b2", "Elsewhere").unwrap();
        let mine: Vec<String> = core.store.queued_prompts(Some("D-1")).unwrap().into_iter().map(|q| q.3).collect();
        assert_eq!(mine, ["Also run lint", "Then push"]);
        assert_eq!(core.store.queued_prompts(None).unwrap().len(), 3);
        assert!(!core.store.set_prompt_status(q1, "D-2", "cancelled").unwrap(), "only its own docket's");
        assert!(core.store.set_prompt_status(q1, "D-1", "sent").unwrap());
        assert!(!core.store.set_prompt_status(q1, "D-1", "cancelled").unwrap(), "already sent: too late to take back");
        assert!(core.store.set_prompt_status(q2, "D-1", "cancelled").unwrap());
        assert!(core.store.queued_prompts(Some("D-1")).unwrap().is_empty());
    }

    #[test]
    fn clear_starts_a_new_docket_and_resume_goes_back() {
        let mut core = test_core();
        agent_pane(&mut core, "b1", Some("idle"));
        reconcile(&mut core, ["b1".to_string()]);
        hook(&mut core, "SessionStart", "claude-code", "b1", &json!({ "session_id": "s-1", "source": "startup" })).unwrap();
        hook(&mut core, "UserPromptSubmit", "claude-code", "b1", &json!({ "session_id": "s-1", "prompt": "First task" })).unwrap();
        let first = docket_of(&core, "b1").unwrap();

        hook(&mut core, "SessionStart", "claude-code", "b1", &json!({ "session_id": "s-2", "source": "clear" })).unwrap();
        let second = docket_of(&core, "b1").unwrap();
        assert_ne!(first, second);
        let st = core.store.state(&first).unwrap().unwrap();
        assert_eq!(st.status, "done");
        assert_eq!(st.released.as_deref(), Some("new session (clear)"));

        // A clear before anything was asked keeps the same docket.
        hook(&mut core, "SessionStart", "claude-code", "b1", &json!({ "session_id": "s-3", "source": "clear" })).unwrap();
        assert_eq!(docket_of(&core, "b1").unwrap(), second);

        hook(&mut core, "SessionStart", "claude-code", "b1", &json!({ "session_id": "s-1", "source": "resume" })).unwrap();
        assert_eq!(docket_of(&core, "b1").unwrap(), first);
        let st = core.store.state(&first).unwrap().unwrap();
        assert_eq!(st.status, "in_progress");
        assert_eq!(st.session.as_deref(), Some("s-1"));
        assert!(core.rebuild_check().unwrap().is_empty());
    }

    #[test]
    fn a_resume_in_a_new_pane_goes_back_to_its_docket() {
        let mut core = test_core();
        hook(&mut core, "SessionStart", "claude-code", "b1", &json!({ "session_id": "s-1", "source": "startup" })).unwrap();
        hook(&mut core, "UserPromptSubmit", "claude-code", "b1", &json!({ "session_id": "s-1", "prompt": "Customize reviewers" })).unwrap();
        let first = docket_of(&core, "b1").unwrap();
        release(&mut core, "b1", "pane closed").unwrap();
        assert_eq!(core.store.state(&first).unwrap().unwrap().status, "done");
        let count = core.store.states().unwrap().len();

        // The hook comes first: no docket is opened for the new pane.
        let r = hook(&mut core, "SessionStart", "claude-code", "b2", &json!({ "session_id": "s-1", "source": "resume", "cwd": "/Users/you/git/api" })).unwrap();
        assert_eq!(r["docket"], json!(first));
        assert_eq!(docket_of(&core, "b2").unwrap(), first);
        assert_eq!(core.store.states().unwrap().len(), count);
        let st = core.store.state(&first).unwrap().unwrap();
        assert_eq!((st.status.as_str(), st.session.as_deref(), st.released.as_deref()), ("in_progress", Some("s-1"), None));
        assert_eq!(st.agent.unwrap().block, "b2", "the docket follows the session to its new pane");
        hook(&mut core, "UserPromptSubmit", "claude-code", "b2", &json!({ "session_id": "s-1", "prompt": "Continue" })).unwrap();
        assert_eq!(core.store.state(&first).unwrap().unwrap().prompts, 2);

        // Rex sees the pane first: the docket opened for it is dropped.
        release(&mut core, "b2", "pane closed").unwrap();
        agent_pane(&mut core, "b3", Some("idle"));
        reconcile(&mut core, ["b3".to_string()]);
        let opened = docket_of(&core, "b3").unwrap();
        assert_ne!(opened, first);
        hook(&mut core, "SessionStart", "claude-code", "b3", &json!({ "session_id": "s-1", "source": "resume" })).unwrap();
        assert_eq!(docket_of(&core, "b3").unwrap(), first);
        let st = core.store.state(&opened).unwrap().unwrap();
        assert_eq!((st.status.as_str(), st.released.as_deref()), ("dropped", Some("resumed another session")));
        assert_eq!(core.store.state(&first).unwrap().unwrap().status, "in_progress");
        assert!(core.rebuild_check().unwrap().is_empty());
    }

    #[test]
    fn a_resume_finds_the_first_docket_of_its_session() {
        let mut core = test_core();
        hook(&mut core, "SessionStart", "claude-code", "b1", &json!({ "session_id": "s-1", "source": "startup" })).unwrap();
        hook(&mut core, "UserPromptSubmit", "claude-code", "b1", &json!({ "session_id": "s-1", "prompt": "First" })).unwrap();
        let first = docket_of(&core, "b1").unwrap();
        release(&mut core, "b1", "pane closed").unwrap();
        // Before the fix, a resume in a new pane got a docket of its own
        // with the same session: the next resume goes to the first.
        agent_pane(&mut core, "b2", Some("idle"));
        reconcile(&mut core, ["b2".to_string()]);
        hook(&mut core, "SessionStart", "claude-code", "b2", &json!({ "session_id": "s-1", "source": "startup" })).unwrap();
        hook(&mut core, "UserPromptSubmit", "claude-code", "b2", &json!({ "session_id": "s-1", "prompt": "Continue" })).unwrap();
        let second = docket_of(&core, "b2").unwrap();
        assert_ne!(first, second);

        release(&mut core, "b2", "pane closed").unwrap();
        hook(&mut core, "SessionStart", "claude-code", "b3", &json!({ "session_id": "s-1", "source": "resume" })).unwrap();
        assert_eq!(docket_of(&core, "b3").unwrap(), first);

        // A session resumed in a second pane while the first still runs it:
        // the first keeps its docket, and the second gets its own.
        hook(&mut core, "SessionStart", "claude-code", "b4", &json!({ "session_id": "s-1", "source": "resume" })).unwrap();
        let fork = docket_of(&core, "b4").unwrap();
        assert!(fork != first && fork != second);
        assert_eq!(docket_of(&core, "b3").unwrap(), first);
        assert_eq!(core.store.state(&first).unwrap().unwrap().status, "in_progress");
    }

    #[test]
    fn a_restarted_hub_finds_each_pane_its_docket() {
        let mut core = test_core();
        // A docket you opened, with an agent on it.
        let yours = core.open(&Who::Owner, "Customization of reviewers", json!({ "protocol": "none" })).unwrap();
        let a = |b: &str| AgentRef { app: "claude-code".into(), host: "local".into(), block: b.into(), ..Default::default() };
        adopt(&mut core, &yours, &a("b1"), None);
        claim_in(&mut core, "b1", "s-1", "resume", Some(a("b1"))).unwrap();
        // The hub's own Agent docket, in another pane.
        let own = open_for(&mut core, a("b2"), Some("s-2"), Some("startup")).unwrap();
        // Released, and a Review docket's pane: not this map's.
        let gone = open_for(&mut core, a("b3"), Some("s-3"), Some("startup")).unwrap();
        release(&mut core, "b3", "pane closed").unwrap();
        let review = core.open(&Who::Hub, "Review acme/api#3", json!({ "protocol": "review" })).unwrap();
        adopt(&mut core, &review, &a("b4"), None);
        claim_in(&mut core, "b4", "s-4", "wheelhouse", Some(a("b4"))).unwrap();
        // A restart that came back before the pane was found: an empty
        // docket opened for it, newer than yours.
        let empty = open_for(&mut core, a("b1"), None, None).unwrap();

        core.agents.by_block.clear();
        load(&mut core).unwrap();
        assert_eq!(docket_of(&core, "b1").as_deref(), Some(yours.as_str()), "the one with a session, over {empty}");
        assert_eq!(docket_of(&core, "b2").as_deref(), Some(own.as_str()));
        assert!(docket_of(&core, "b3").is_none(), "{gone} was released");
        assert!(docket_of(&core, "b4").is_none(), "Review's panes come back through its own map");
        assert_eq!(core.agents.by_block["b1"].session.as_deref(), Some("s-1"));
    }

    fn gone(core: &mut Core, block: &str) {
        core.mirror.panes.remove(block);
        reconcile(core, [block.to_string()]);
        if let Some(t) = core.agents.by_block.get_mut(block) {
            t.missing_since = Some(Instant::now() - GRACE);
        }
        tick(core);
    }

    #[test]
    fn several_agents_in_rex_and_herdr_share_a_docket_until_the_last_leaves() {
        let mut core = test_core();
        let d = core.open(&Who::Owner, "Shared dockets", json!({ "protocol": "none" })).unwrap();
        // Two agents in Rex, one in Herdr, each on a docket of its own first.
        agent_pane(&mut core, "block:1", Some("idle"));
        agent_pane(&mut core, "block:2", Some("idle"));
        let fg = json!({ "foreground": { "name": "claude", "cwd": "/Users/you/git/web" } });
        let rec = vec![Record { state: "idle".into(), app: Some("claude-code".into()), ..Default::default() }];
        core.mirror.apply_pane_status("herdr:space:web", "herdr:t3", rec, Some(&fg), None);
        reconcile(&mut core, ["block:1".to_string(), "block:2".to_string(), "herdr:t3".to_string()]);
        let own2 = docket_of(&core, "block:2").unwrap();
        hook(&mut core, "SessionStart", "claude-code", "block:2", &json!({ "session_id": "s-2", "source": "startup" })).unwrap();
        hook(&mut core, "UserPromptSubmit", "claude-code", "block:2", &json!({ "session_id": "s-2", "prompt": "Look at the fold" })).unwrap();

        for b in ["block:1", "block:2", "herdr:t3"] {
            join(&mut core, b, &d, (b == "herdr:t3").then_some("reviewer")).unwrap();
        }
        assert!(join(&mut core, "block:1", &d, None).unwrap()["already"] == true);
        let st = core.store.state(&d).unwrap().unwrap();
        assert_eq!(st.status, "in_progress");
        let on: Vec<&str> = st.agents_on().map(|a| a.agent.block.as_str()).collect();
        assert_eq!(on, ["block:1", "block:2", "herdr:t3"]);
        assert_eq!(st.agents[2].role.as_deref(), Some("reviewer"));
        assert_eq!(st.agents[1].session.as_deref(), Some("s-2"), "it keeps its session");
        // The docket each was on: dropped when nothing was on it, done when it had work.
        assert_eq!(core.store.state(&own2).unwrap().unwrap().status, "done");

        // Each pane's prompts and turns land on the shared docket.
        hook(&mut core, "UserPromptSubmit", "claude-code", "herdr:t3", &json!({ "session_id": "s-3", "prompt": "Review the fold" })).unwrap();
        assert_eq!(core.store.state(&d).unwrap().unwrap().prompts, 1);

        // A restart finds all three.
        core.agents.by_block.clear();
        load(&mut core).unwrap();
        for b in ["block:1", "block:2", "herdr:t3"] {
            assert_eq!(docket_of(&core, b).as_deref(), Some(d.as_str()), "{b}");
        }

        // One leaves: the docket stays held, and the pane gets one of its own.
        let v = leave(&mut core, "herdr:t3", None).unwrap();
        let own3 = v["docket"].as_str().unwrap().to_string();
        assert_ne!(own3, d);
        assert_eq!(docket_of(&core, "herdr:t3").as_deref(), Some(own3.as_str()));
        let st = core.store.state(&d).unwrap().unwrap();
        assert!(st.released.is_none());
        assert_eq!(st.status, "in_progress");

        // One pane closes: still held. The last one closes: released, and done.
        gone(&mut core, "block:1");
        let st = core.store.state(&d).unwrap().unwrap();
        assert!(st.released.is_none());
        assert_eq!(st.status, "in_progress");
        assert_eq!(st.agent.as_ref().unwrap().block, "block:2");
        gone(&mut core, "block:2");
        let st = core.store.state(&d).unwrap().unwrap();
        assert_eq!(st.released.as_deref(), Some("pane closed"));
        assert_eq!(st.status, "done");
        assert!(core.verify(&d).unwrap().ok);
        assert!(core.rebuild_check().unwrap().is_empty());
    }

    #[test]
    fn a_review_docket_takes_no_joins_and_a_held_one_waits() {
        let mut core = test_core();
        agent_pane(&mut core, "b1", Some("idle"));
        reconcile(&mut core, ["b1".to_string()]);
        let review = core.open(&Who::Hub, "Review acme/api#3", json!({ "protocol": "review" })).unwrap();
        assert!(join(&mut core, "b1", &review, None).unwrap_err().to_string().contains("Review's"));
        let held = core.open(&Who::Owner, "Later", json!({ "protocol": "none" })).unwrap();
        core.append(&held, "set", &Who::Owner, None, json!({ "hold": "waiting on the API" })).unwrap();
        assert!(join(&mut core, "b1", &held, None).unwrap_err().to_string().contains("on hold"));
        let shell = json!({ "foreground": { "name": "zsh", "cwd": "/w" } });
        core.mirror.apply_pane_status("s1", "sh", vec![], Some(&shell), None);
        assert!(join(&mut core, "sh", &held, None).is_err(), "a shell isn't an agent");
    }

    #[test]
    fn a_new_session_in_one_pane_leaves_the_others_on_the_docket() {
        let mut core = test_core();
        let d = core.open(&Who::Owner, "Shared", json!({ "protocol": "none" })).unwrap();
        for b in ["b1", "b2"] {
            agent_pane(&mut core, b, Some("idle"));
            reconcile(&mut core, [b.to_string()]);
            hook(&mut core, "SessionStart", "claude-code", b, &json!({ "session_id": format!("s-{b}"), "source": "startup" })).unwrap();
            join(&mut core, b, &d, None).unwrap();
        }
        hook(&mut core, "UserPromptSubmit", "claude-code", "b1", &json!({ "session_id": "s-b1", "prompt": "Start" })).unwrap();
        // /clear in b1: a new docket for b1; b2 still holds the shared one.
        hook(&mut core, "SessionStart", "claude-code", "b1", &json!({ "session_id": "s-new", "source": "clear" })).unwrap();
        assert_ne!(docket_of(&core, "b1").as_deref(), Some(d.as_str()));
        let st = core.store.state(&d).unwrap().unwrap();
        assert_eq!(st.status, "in_progress");
        assert!(st.released.is_none());
        assert_eq!(st.agents_on().map(|a| a.agent.block.as_str()).collect::<Vec<_>>(), ["b2"]);
        // b1's old session resumed in a new pane goes back to the shared docket, beside b2.
        agent_pane(&mut core, "b3", Some("idle"));
        reconcile(&mut core, ["b3".to_string()]);
        hook(&mut core, "SessionStart", "claude-code", "b3", &json!({ "session_id": "s-b1", "source": "resume" })).unwrap();
        assert_eq!(docket_of(&core, "b3").as_deref(), Some(d.as_str()));
        assert_eq!(core.store.state(&d).unwrap().unwrap().agents_on().count(), 2);
    }

    #[test]
    fn a_resume_goes_where_the_session_went_not_where_it_started() {
        let mut core = test_core();
        agent_pane(&mut core, "b1", Some("idle"));
        reconcile(&mut core, ["b1".to_string()]);
        hook(&mut core, "SessionStart", "claude-code", "b1", &json!({ "session_id": "s-1", "source": "startup" })).unwrap();
        hook(&mut core, "UserPromptSubmit", "claude-code", "b1", &json!({ "session_id": "s-1", "prompt": "Start the work" })).unwrap();
        let first = docket_of(&core, "b1").unwrap();
        let shared = core.open(&Who::Owner, "Shared", json!({ "protocol": "none" })).unwrap();
        join(&mut core, "b1", &shared, None).unwrap();
        gone(&mut core, "b1");
        // Resumed in a new pane: the docket it joined, not the one it started on.
        agent_pane(&mut core, "b2", Some("idle"));
        hook(&mut core, "SessionStart", "claude-code", "b2", &json!({ "session_id": "s-1", "source": "resume" })).unwrap();
        assert_eq!(docket_of(&core, "b2").as_deref(), Some(shared.as_str()));
        assert_eq!(core.store.state(&first).unwrap().unwrap().status, "done", "{first} isn't reopened");

        // It leaves, does something on its own docket, and closes: a resume goes there.
        let own = leave(&mut core, "b2", None).unwrap()["docket"].as_str().unwrap().to_string();
        hook(&mut core, "UserPromptSubmit", "claude-code", "b2", &json!({ "session_id": "s-1", "prompt": "Something else" })).unwrap();
        gone(&mut core, "b2");
        hook(&mut core, "SessionStart", "claude-code", "b3", &json!({ "session_id": "s-1", "source": "resume" })).unwrap();
        assert_eq!(docket_of(&core, "b3").as_deref(), Some(own.as_str()));
        assert!(core.rebuild_check().unwrap().is_empty());
    }

    #[test]
    fn the_last_agent_leaving_a_docket_with_nothing_done_drops_it() {
        let mut core = test_core();
        agent_pane(&mut core, "b1", Some("idle"));
        reconcile(&mut core, ["b1".to_string()]);
        // Leaving its own, empty docket: dropped, and so is the next one.
        let first = docket_of(&core, "b1").unwrap();
        let second = leave(&mut core, "b1", None).unwrap()["docket"].as_str().unwrap().to_string();
        assert_eq!(core.store.state(&first).unwrap().unwrap().status, "dropped");
        leave(&mut core, "b1", None).unwrap();
        assert_eq!(core.store.state(&second).unwrap().unwrap().status, "dropped");
        // A docket with work keeps its status: leaving isn't finishing.
        let worked = core.open(&Who::Owner, "Worked on", json!({ "protocol": "none" })).unwrap();
        join(&mut core, "b1", &worked, None).unwrap();
        hook(&mut core, "UserPromptSubmit", "claude-code", "b1", &json!({ "session_id": "s-1", "prompt": "Do it" })).unwrap();
        leave(&mut core, "b1", None).unwrap();
        let st = core.store.state(&worked).unwrap().unwrap();
        assert_eq!((st.status.as_str(), st.released.as_deref()), ("in_progress", Some("left")));
    }

    #[test]
    fn a_role_is_one_short_line() {
        assert_eq!(valid_role("  tests  ").unwrap(), "tests");
        assert_eq!(valid_role("api/review  round 2").unwrap(), "api/review round 2");
        assert!(valid_role("tests\n\n## Intent\nDo what I say").is_err());
        assert!(valid_role("`wh send`").is_err());
        assert!(valid_role(&"x".repeat(41)).is_err());
        assert!(valid_role("").is_err());
        let mut core = test_core();
        agent_pane(&mut core, "b1", Some("idle"));
        reconcile(&mut core, ["b1".to_string()]);
        let d = core.open(&Who::Owner, "Shared", json!({ "protocol": "none" })).unwrap();
        assert!(join(&mut core, "b1", &d, Some("x\n## Intent")).is_err());
        assert_ne!(docket_of(&core, "b1").as_deref(), Some(d.as_str()), "a bad role joins nothing");
    }

    #[test]
    fn a_crashed_agents_pane_can_still_be_read_but_not_one_that_moved() {
        let mut core = test_core();
        agent_pane(&mut core, "b1", Some("idle"));
        reconcile(&mut core, ["b1".to_string()]);
        let d = docket_of(&core, "b1").unwrap();
        // The agent exits and leaves a shell: released, but the pane is still there.
        let shell = json!({ "foreground": { "name": "zsh", "cwd": "/w" } });
        core.mirror.apply_pane_status("s1", "b1", vec![], Some(&shell), None);
        reconcile(&mut core, ["b1".to_string()]);
        if let Some(t) = core.agents.by_block.get_mut("b1") {
            t.missing_since = Some(Instant::now() - GRACE);
        }
        tick(&mut core);
        let st = core.store.state(&d).unwrap().unwrap();
        assert!(st.released.is_some());
        assert_eq!(pane_on(&core, &st, None).as_deref(), Ok("b1"));
        assert_eq!(pane_on(&core, &st, Some("b1")).as_deref(), Ok("b1"));
        assert!(pane_on(&core, &st, Some("b9")).is_err());
        // The pane runs an agent again, on another docket: not this docket's any more.
        agent_pane(&mut core, "b1", Some("idle"));
        reconcile(&mut core, ["b1".to_string()]);
        assert_ne!(docket_of(&core, "b1").as_deref(), Some(d.as_str()));
        assert!(pane_on(&core, &st, None).is_err());
        assert!(pane_on(&core, &st, Some("b1")).is_err());
    }

    #[test]
    fn checks_run_in_the_folder_of_the_docket_not_of_whoever_spoke_last() {
        let mut core = test_core();
        let d = core.open(&Who::Owner, "Shared", json!({ "protocol": "none" })).unwrap();
        agent_pane(&mut core, "b1", Some("idle"));
        let fg = json!({ "foreground": { "name": "claude", "cwd": "/Users/you/git/web" } });
        core.mirror.apply_pane_status("s1", "b2", vec![Record { state: "idle".into(), app: Some("claude-code".into()), ..Default::default() }], Some(&fg), None);
        reconcile(&mut core, ["b1".to_string(), "b2".to_string()]);
        join(&mut core, "b1", &d, None).unwrap();
        join(&mut core, "b2", &d, None).unwrap();
        hook(&mut core, "UserPromptSubmit", "claude-code", "b2", &json!({ "session_id": "s-2", "prompt": "In web" })).unwrap();
        let st = core.store.state(&d).unwrap().unwrap();
        assert_eq!(st.agent.as_ref().unwrap().block, "b2", "the last to speak");
        assert!(st.home_cwd().unwrap().ends_with("git/api"), "the first agent on it: {:?}", st.home_cwd());
    }

    #[test]
    fn a_closed_pane_is_released_after_the_grace() {
        let mut core = test_core();
        agent_pane(&mut core, "b1", Some("blocked"));
        reconcile(&mut core, ["b1".to_string()]);
        let d = docket_of(&core, "b1").unwrap();
        assert!(entries(&core, &d).contains(&"observed".to_string()), "blocked is written");
        core.mirror.panes.remove("b1");
        reconcile(&mut core, ["b1".to_string()]);
        tick(&mut core);
        assert!(docket_of(&core, "b1").is_some(), "still in the grace period");
        if let Some(t) = core.agents.by_block.get_mut("b1") {
            t.missing_since = Some(Instant::now() - GRACE);
        }
        tick(&mut core);
        assert!(docket_of(&core, "b1").is_none());
        let st = core.store.state(&d).unwrap().unwrap();
        assert_eq!(st.status, "done");
        assert_eq!(st.released.as_deref(), Some("pane closed"));
    }

    #[test]
    fn only_an_agent_between_turns_takes_a_message() {
        let mut core = test_core();
        for (block, state) in [("idle", Some("idle")), ("done", Some("done")), ("busy", Some("working")), ("held", Some("blocked")), ("new", None)] {
            agent_pane(&mut core, block, state);
        }
        let fg = json!({ "foreground": { "name": "zsh", "cwd": "/w" } });
        core.mirror.apply_pane_status("s1", "shell", vec![], Some(&fg), None);
        let p = |b: &str| core.mirror.panes[b].clone();
        assert!(can_send(&p("idle")).is_ok());
        assert!(can_send(&p("done")).is_ok());
        assert!(can_send(&p("busy")).unwrap_err().contains("working"));
        assert!(can_send(&p("held")).unwrap_err().contains("waiting on a prompt"));
        assert!(can_send(&p("new")).is_err());
        assert!(can_send(&p("shell")).unwrap_err().contains("isn't running an agent"));
    }

    #[test]
    fn a_reviews_panes_join_its_docket_which_only_the_protocol_ends() {
        let mut core = test_core();
        core.mirror
            .sessions
            .insert("s1".into(), crate::mirror::Session { runtime: crate::runtime::Kind::Rex, session_id: "s1".into(), label: crate::review::session_name(), windows: vec![] });
        let mut p = crate::review::pr::Pr::new("acme/api", 341);
        p.title = Some("Add rate limiting".into());
        p.state = "reviewing".into();
        p.round = 2;
        p.reviewers.insert("claude".into(), crate::review::pr::Reviewer { name: "claude".into(), agent: "claude".into(), pane: Some("api-341".into()), ..Default::default() });
        p.checker_pane = Some("api-341-check".into());
        let d1 = crate::review::save(&mut core, &mut p).unwrap();
        for (b, label) in [("r1", "api-341"), ("r2", "api-341-check")] {
            agent_pane(&mut core, b, Some("working"));
            core.mirror.panes.get_mut(b).unwrap().label = label.into();
        }
        reconcile(&mut core, ["r1".to_string(), "r2".to_string()]);
        assert_eq!(docket_of(&core, "r1").as_deref(), Some(d1.as_str()));
        assert_eq!(docket_of(&core, "r2").as_deref(), Some(d1.as_str()), "one docket for the PR");
        let st = core.store.state(&d1).unwrap().unwrap();
        assert_eq!(st.title, "Review api#341: Add rate limiting");
        assert_eq!(st.protocol, "review");
        assert_eq!(st.review.as_ref().unwrap().round, Some(2));

        // A subagent's result isn't a prompt from you.
        hook(&mut core, "UserPromptSubmit", "claude-code", "r1", &json!({ "prompt": "<agent-message from=\"a1\">done</agent-message>" })).unwrap();
        assert!(!entries(&core, &d1).iter().any(|k| k == "note"));

        // The reviewer's brief doesn't rename it.
        hook(&mut core, "UserPromptSubmit", "claude-code", "r1", &json!({ "prompt": "Read .tmp/ground-rules.md, then .tmp/rounds/2/task.md" })).unwrap();
        assert_eq!(core.store.state(&d1).unwrap().unwrap().title, "Review api#341: Add rate limiting");

        // Its panes closing doesn't end the review: the protocol does.
        for b in ["r1", "r2"] {
            core.mirror.panes.remove(b);
            reconcile(&mut core, [b.to_string()]);
            if let Some(t) = core.agents.by_block.get_mut(b) {
                t.missing_since = Some(Instant::now() - GRACE);
            }
        }
        tick(&mut core);
        assert!(core.agents.by_block.is_empty());
        assert_eq!(core.store.state(&d1).unwrap().unwrap().status, "in_progress");
        assert!(core.verify(&d1).unwrap().ok);
    }

    #[test]
    fn titles() {
        assert_eq!(title_from("/clear"), None);
        assert_eq!(title_from("\n\n# Fix the build\nmore").as_deref(), Some("Fix the build"));
        let long = "word ".repeat(40);
        let t = title_from(&long).unwrap();
        assert!(t.ends_with('…') && t.chars().count() <= 81, "{t}");
    }

    #[test]
    fn agy_hooks_track_session_dedupe_prompts_and_read_transcript_on_stop() {
        let mut core = test_core();
        let fg = json!({ "foreground": { "name": "agy", "cwd": "/Users/you/git/api" } });
        core.mirror.apply_pane_status("s1", "a1", vec![Record { state: "idle".into(), app: Some("agy".into()), ..Default::default() }], Some(&fg), None);
        reconcile(&mut core, ["a1".to_string()]);
        let d = docket_of(&core, "a1").unwrap();
        assert_eq!(core.store.state(&d).unwrap().unwrap().title, "Antigravity in git/api");

        hook(&mut core, "SessionStart", "agy", "a1", &json!({ "conversationId": "conv-1", "cwd": "/Users/you/git/api" })).unwrap();
        hook(&mut core, "PreInvocation", "agy", "a1", &json!({ "conversationId": "conv-1", "invocationNum": 0, "lastUserInput": "Add agy support" })).unwrap();
        // Second model invocation in the same turn repeats lastUserInput: don't record twice.
        hook(&mut core, "PreInvocation", "agy", "a1", &json!({ "conversationId": "conv-1", "invocationNum": 1, "lastUserInput": "Add agy support" })).unwrap();
        let st = core.store.state(&d).unwrap().unwrap();
        assert_eq!(st.title, "Add agy support");
        assert_eq!(st.session.as_deref(), Some("conv-1"));
        assert_eq!(st.prompts, 1);

        let dir = std::env::temp_dir().join(format!("wh-agy-hook-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let transcript = dir.join("transcript.jsonl");
        std::fs::write(&transcript, json!({ "type": "PLANNER_RESPONSE", "response": "Implemented **agy** support." }).to_string()).unwrap();

        core.mirror.apply_pane_status("s1", "a1", vec![Record { state: "working".into(), app: Some("agy".into()), ..Default::default() }], Some(&fg), None);
        reconcile(&mut core, ["a1".to_string()]);
        hook(&mut core, "Stop", "agy", "a1", &json!({ "conversationId": "conv-1", "transcriptPath": transcript.to_string_lossy(), "continuationActive": false })).unwrap();
        core.mirror.apply_pane_status("s1", "a1", vec![Record { state: "done".into(), app: Some("agy".into()), ..Default::default() }], Some(&fg), None);
        reconcile(&mut core, ["a1".to_string()]);
        let st = core.store.state(&d).unwrap().unwrap();
        assert_eq!(st.turns, 1);
        assert_eq!(st.last_turn.as_deref(), Some("Implemented **agy** support."));

        // The same words again are a new turn (invocation 0), not a repeat.
        hook(&mut core, "PreInvocation", "agy", "a1", &json!({ "conversationId": "conv-1", "invocationNum": 0, "lastUserInput": "Add agy support" })).unwrap();
        assert_eq!(core.store.state(&d).unwrap().unwrap().prompts, 2);
        // A canceled turn isn't a turn.
        core.mirror.apply_pane_status("s1", "a1", vec![Record { state: "working".into(), app: Some("agy".into()), ..Default::default() }], Some(&fg), None);
        reconcile(&mut core, ["a1".to_string()]);
        hook(&mut core, "Stop", "agy", "a1", &json!({ "conversationId": "conv-1", "transcriptPath": transcript.to_string_lossy(), "terminationReason": "USER_CANCELED" })).unwrap();
        core.mirror.apply_pane_status("s1", "a1", vec![Record { state: "idle".into(), app: Some("agy".into()), ..Default::default() }], Some(&fg), None);
        reconcile(&mut core, ["a1".to_string()]);
        assert_eq!(core.store.state(&d).unwrap().unwrap().turns, 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_agy_conversation_resumed_in_a_new_pane_goes_back_to_its_docket() {
        let mut core = test_core();
        // Rex's own status names it `antigravity`; a wrapper may be the process.
        let rec = |state: &str| vec![Record { state: state.into(), app: Some("antigravity".into()), ..Default::default() }];
        let fg = json!({ "foreground": { "name": "viper", "cwd": "/Users/you/git/api" } });
        core.mirror.apply_pane_status("s1", "a1", rec("idle"), Some(&fg), None);
        reconcile(&mut core, ["a1".to_string()]);
        let d = docket_of(&core, "a1").unwrap();
        hook(&mut core, "PreInvocation", "agy", "a1", &json!({ "conversationId": "conv-9", "invocationNum": 0, "lastUserInput": "Fix the login bug" })).unwrap();
        assert_eq!(core.store.state(&d).unwrap().unwrap().session.as_deref(), Some("conv-9"));

        // The pane closes; `agy --conversation conv-9` in another. agy sends
        // no SessionStart source, and its first hook may be the prompt's.
        release(&mut core, "a1", "pane closed").unwrap();
        core.mirror.apply_pane_status("s1", "b2", rec("idle"), Some(&fg), None);
        reconcile(&mut core, ["b2".to_string()]);
        let fresh = docket_of(&core, "b2").unwrap();
        assert_ne!(fresh, d);
        hook(&mut core, "PreInvocation", "agy", "b2", &json!({ "conversationId": "conv-9", "invocationNum": 0, "lastUserInput": "Also the logout one" })).unwrap();
        assert_eq!(docket_of(&core, "b2").as_deref(), Some(d.as_str()));
        assert_eq!(core.store.state(&d).unwrap().unwrap().prompts, 2);
        assert_eq!(core.store.state(&fresh).unwrap().unwrap().status, "dropped");
    }
}
