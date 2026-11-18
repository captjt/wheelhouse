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

struct PendingTurn {
    since: Instant,
    status: Option<String>,
    message: Option<String>,
}

#[derive(Default)]
pub struct Tracker {
    pub by_block: BTreeMap<String, Tracked>,
    turns: BTreeMap<String, PendingTurn>,
    /// Dockets whose agent is answering a prompt you sent from the app:
    /// its reply is pushed to you, since you're away from the terminal.
    pub remote: std::collections::HashSet<String>,
}

/// Pick up the agent dockets that were open when the hub last stopped.
pub fn load(core: &mut Core) -> Result<()> {
    for st in core.store.states()? {
        let (true, Some(a), None) = (st.protocol == "agent" && st.is_open(), &st.agent, &st.released) else { continue };
        core.agents.by_block.entry(a.block.clone()).or_insert(Tracked {
            docket: st.id.clone(),
            app: a.app.clone(),
            session: st.session.clone(),
            state: None,
            root: None,
            blocked_seen: None,
            missing_since: None,
            pending_prompt: None,
        });
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
    core.agents.by_block.insert(
        a.block.clone(),
        Tracked {
            docket: id.clone(),
            app: a.app.clone(),
            session: session.map(str::to_string),
            state: None,
            root: None,
            blocked_seen: None,
            missing_since: None,
            pending_prompt: None,
        },
    );
    Ok(id)
}

/// Track a pane Wheelhouse started on a docket it already opened.
pub fn adopt(core: &mut Core, docket: &str, a: &AgentRef, prompt: Option<&str>) {
    core.agents.by_block.insert(
        a.block.clone(),
        Tracked {
            docket: docket.to_string(),
            app: a.app.clone(),
            session: None,
            state: None,
            root: None,
            blocked_seen: None,
            missing_since: None,
            pending_prompt: prompt.map(str::to_string),
        },
    );
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
            let data = json!({ "what": h.state, "kind": h.kind, "record": h.id, "title": h.title });
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
        .remove(&docket)
        .then(|| summary.clone().or_else(|| pt.message.as_deref().map(|m| m.lines().find(|l| !l.trim().is_empty()).unwrap_or("").chars().take(200).collect())));
    core.append(&docket, "turn", &Who::Hub, pt.message, json!({ "state": pt.status, "summary": summary }))?;
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

/// Release the pane's docket and set it to `status`.
fn release_as(core: &mut Core, block: &str, reason: &str, status: &str) -> Result<()> {
    flush_turn(core, block)?;
    let Some(t) = core.agents.by_block.remove(block) else { return Ok(()) };
    core.append(&t.docket, "release", &Who::Hub, None, json!({ "reason": reason, "block": block }))?;
    // A Review docket outlives any one of its panes: it ends when the PR's
    // review is over and no pane of it is left.
    // A Review's or a Build's docket status is its protocol's to set.
    let ours = core.store.state(&t.docket)?.is_none_or(|s| s.protocol != "review") && core.store.build_task(&t.docket)?.is_none();
    if ours {
        core.append(&t.docket, "set", &Who::Hub, None, json!({ "status": status }))?;
    }
    Ok(())
}

/// The docket a resumed session was on: the first one it was claimed on
/// that wasn't dropped. Not one another live pane still holds (the session
/// resumed twice at once): that pane keeps it.
fn resumed_docket(core: &Core, block: &str, session: &str) -> Result<Option<crate::record::fold::DocketState>> {
    // States come newest first.
    let d = core.store.states()?.into_iter().rfind(|st| st.session.as_deref() == Some(session) && st.status != "dropped");
    Ok(d.filter(|d| !core.agents.by_block.iter().any(|(b, t)| b != block && t.docket == d.id)))
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
    let agent = match here {
        Some(a) => Some(a),
        None => core.store.state(&t.docket)?.and_then(|s| s.agent),
    };
    core.append(&t.docket, "claim", &Who::Hub, None, json!({ "session": session, "source": source, "agent": agent }))?;
    if let Some(tt) = core.agents.by_block.get_mut(block) {
        tt.session = Some(session.to_string());
    }
    Ok(())
}

/// One call from an agent's hook: `event` is Claude Code's name for it.
pub fn hook(core: &mut Core, event: &str, app: &str, block: &str, p: &Value) -> Result<Value> {
    let session = p["session_id"].as_str().filter(|s| !s.is_empty());
    let source = p["source"].as_str().unwrap_or("startup");
    let cwd = p["cwd"].as_str();
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
        "UserPromptSubmit" => {
            let prompt = p["prompt"].as_str().unwrap_or_default().trim();
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
                core.append(&docket, "note", &Who::Hub, Some(prompt.to_string()), json!({ "via": "message" }))?;
            } else {
                note_prompt(core, &docket, prompt, session, None)?;
            }
        }
        "Stop" => {
            if p["stop_hook_active"].as_bool() == Some(true) {
                return Ok(json!({}));
            }
            let msg = p["last_assistant_message"].as_str().unwrap_or_default();
            if !msg.trim().is_empty() {
                turn_message(core, block, msg)?;
            }
        }
        _ => {}
    }
    core.live_changed();
    Ok(json!({ "docket": core.agents.by_block.get(block).map(|t| t.docket.clone()) }))
}

/// Text Claude Code hands its agent as a prompt that nobody typed: a
/// background subagent's result, or a task's notification.
fn from_the_harness(prompt: &str) -> bool {
    ["<agent-message", "<task-notification", "<system-reminder"].iter().any(|p| prompt.starts_with(p))
}

/// A prompt you gave an agent: a note on its docket, and the docket's title
/// if it has none of its own yet. `from` says where you typed it, when that
/// wasn't the pane (`wheelhouse`).
pub fn note_prompt(core: &mut Core, docket: &str, prompt: &str, session: Option<&str>, from: Option<&str>) -> Result<()> {
    core.append(docket, "note", &Who::Owner, Some(prompt.to_string()), json!({ "via": "prompt", "session": session, "from": from }))?;
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
    if !p.is_agent() {
        return Err("That pane isn't running an agent, so Wheelhouse won't type into it.".into());
    }
    match p.root().map(|r| r.state.as_str()) {
        Some("idle" | "done") => Ok(()),
        Some("working") => Err(format!("{app} is working. Send it when the turn ends.")),
        Some("blocked") => Err(format!("{app} is waiting on a prompt in its pane. Answer that in Rex first.")),
        Some("error") => Err(format!("{app} stopped with an error. Look at its pane in Rex.")),
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

/// What an agent is doing now, from Rex. Not part of the record.
#[derive(Debug, Clone, Serialize)]
pub struct Live {
    pub block: String,
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
        core.agents.remote.insert(d.clone());
        agent_pane(&mut core, "b1", Some("working"));
        reconcile(&mut core, ["b1".to_string()]);
        hook(&mut core, "Stop", "claude-code", "b1", &json!({ "last_assistant_message": "**Fixed.** The retry test passes now." })).unwrap();
        agent_pane(&mut core, "b1", Some("done"));
        reconcile(&mut core, ["b1".to_string()]);
        let t = turns(&mut rx);
        assert_eq!(t.len(), 1, "{t:?}");
        assert_eq!(t[0]["docket"], d.as_str());
        assert_eq!(t[0]["text"], "Fixed. The retry test passes now.");
        assert!(!core.agents.remote.contains(&d));
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
        core.mirror.sessions.insert("s1".into(), crate::mirror::Session { session_id: "s1".into(), label: crate::review::session_name(), windows: vec![] });
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
}
