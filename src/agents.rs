//! The Agent protocol: every agent in Rex is on a docket.
//!
//! - An agent pane that no docket holds gets a new one, named after the pane
//!   until you ask the agent something.
//! - Its hooks (`wh hook`) say which session it's in, what you asked it, and
//!   what it said when a turn ended. `/clear` starts a new session, so a new
//!   docket; `--resume` goes back to the docket that session was on.
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
    for b in blocks {
        if let Err(e) = reconcile_one(core, &b) {
            say!("agents: {b}: {e:#}");
        }
    }
    core.live_changed();
}

fn reconcile_one(core: &mut Core, block: &str) -> Result<()> {
    let pane = core.mirror.panes.get(block).filter(|p| p.is_agent()).cloned();
    let tracked = core.agents.by_block.contains_key(block);
    match (pane, tracked) {
        (Some(p), false) => {
            let a = agent_ref(core, &p);
            if let Some((docket, role)) = crate::review::pane_docket(core, &p) {
                adopt(core, &docket, &a, None);
                core.append(&docket, "claim", &Who::Hub, None, json!({ "source": "wheelhouse", "role": role, "agent": a }))?;
                return status(core, &p);
            }
            match crate::reviewer::pane_role(core, &p) {
                Some((pr, role)) => {
                    crate::reviewer::adopt_pane(core, &pr, &role, a)?;
                }
                None => {
                    open_for(core, a, None, None)?;
                }
            }
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
    let gone: Vec<String> = core
        .agents
        .by_block
        .iter()
        .filter(|(_, t)| t.missing_since.is_some_and(|m| m.elapsed() >= GRACE))
        .map(|(b, _)| b.clone())
        .collect();
    for b in gone {
        let reason = if core.mirror.panes.contains_key(&b) { "agent exited" } else { "pane closed" };
        if let Err(e) = release(core, &b, reason) {
            say!("agents: release {b}: {e:#}");
        }
    }
    if !crate::review::enabled(core) {
        crate::reviewer::sync(core);
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
    core.append(&docket, "turn", &Who::Hub, pt.message, json!({ "state": pt.status, "summary": summary }))?;
    Ok(())
}

fn release(core: &mut Core, block: &str, reason: &str) -> Result<()> {
    flush_turn(core, block)?;
    let Some(t) = core.agents.by_block.remove(block) else { return Ok(()) };
    core.append(&t.docket, "release", &Who::Hub, None, json!({ "reason": reason, "block": block }))?;
    // A Review docket outlives any one of its panes: it ends when the PR's
    // review is over and no pane of it is left.
    let review = core.store.state(&t.docket)?.and_then(|s| s.review).map(|r| r.key);
    let others = core.agents.by_block.values().any(|o| o.docket == t.docket);
    let native = core.store.review_prs()?.iter().any(|(_, d, _)| d.as_deref() == Some(t.docket.as_str()));
    let done = match review {
        // The Review protocol sets its docket's status itself.
        Some(_) if native => false,
        Some(key) => !others && crate::reviewer::ended(core, &key),
        None => true,
    };
    if done {
        core.append(&t.docket, "set", &Who::Hub, None, json!({ "status": "done" }))?;
    }
    Ok(())
}

fn claim(core: &mut Core, block: &str, session: &str, source: &str) -> Result<()> {
    let Some(t) = core.agents.by_block.get(block).cloned() else { return Ok(()) };
    let agent = core.store.state(&t.docket)?.and_then(|s| s.agent);
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

    if !core.agents.by_block.contains_key(block) {
        let a = match core.mirror.panes.get(block).cloned() {
            Some(pane) => {
                let mut a = agent_ref(core, &pane);
                a.app = app.to_string();
                a
            }
            None => AgentRef {
                app: app.to_string(),
                host: "local".into(),
                block: block.to_string(),
                cwd: p["cwd"].as_str().map(crate::paths::tilde),
                ..Default::default()
            },
        };
        open_for(core, a, session, Some(source))?;
    }

    match event {
        "SessionStart" => {
            let Some(s) = session else { return Ok(json!({})) };
            let t = core.agents.by_block[block].clone();
            match t.session.as_deref() {
                None => claim(core, block, s, source)?,
                Some(cur) if cur == s => {}
                Some(_) => {
                    if source == "resume"
                        && let Some(d) = core.store.states()?.into_iter().find(|st| st.session.as_deref() == Some(s) && st.id != t.docket)
                    {
                        release(core, block, "resumed another session")?;
                        core.agents.by_block.insert(block.to_string(), Tracked { docket: d.id.clone(), session: None, ..t.clone() });
                        if !d.is_open() {
                            core.append(&d.id, "set", &Who::Hub, None, json!({ "status": "in_progress" }))?;
                        }
                        claim(core, block, s, source)?;
                        return Ok(json!({ "docket": d.id }));
                    }
                    let fresh = core.store.state(&t.docket)?.is_some_and(|st| st.prompts == 0 && st.turns == 0);
                    if fresh {
                        claim(core, block, s, source)?;
                    } else {
                        release(core, block, &format!("new session ({source})"))?;
                        let a = core.mirror.panes.get(block).cloned().map(|pane| {
                            let mut a = agent_ref(core, &pane);
                            a.app = app.to_string();
                            a
                        });
                        let a = a.unwrap_or(AgentRef { app: app.into(), host: "local".into(), block: block.into(), ..Default::default() });
                        open_for(core, a, Some(s), Some(source))?;
                    }
                }
            }
        }
        "UserPromptSubmit" => {
            let prompt = p["prompt"].as_str().unwrap_or_default().trim();
            if prompt.is_empty() {
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
    let answerable = h.is_some_and(|r| r.state == "blocked" && r.kind.as_deref() == Some("permission"))
        && p.app().and_then(crate::approve::prompt_keys).is_some();
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
        hook(&mut core, "UserPromptSubmit", "claude-code", "b1", &json!({ "session_id": "s-1", "prompt": "Fix the flaky retry test\nIt fails on CI." }))
            .unwrap();
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
    fn a_reviews_panes_share_one_docket_that_ends_with_the_review() {
        // A made-up rex-reviewer ledger: one PR, a reviewer and a checker.
        let dir = std::env::temp_dir().join(format!("wh-reviewer-{}", std::process::id()));
        let pr_dir = dir.join("repos/acme/api");
        std::fs::create_dir_all(&pr_dir).unwrap();
        let write = |state: &str| {
            let pr = json!({ "repo": "acme/api", "pr": 341, "title": "Add rate limiting", "state": state, "phase": "reviewer", "round": 2,
                             "session_id": "s1", "pane": "api-341", "checker_pane": "api-341-check", "reviewers": { "codex": { "pane": "api-341" } } });
            std::fs::write(pr_dir.join("pr-341.json"), pr.to_string()).unwrap();
        };
        write("reviewing");
        // SAFETY: the only test that reads this variable.
        unsafe { std::env::set_var("REX_REVIEWER_STATE_DIR", &dir) };
        let mut core = test_core();
        for (b, label) in [("r1", "api-341"), ("r2", "api-341-check")] {
            agent_pane(&mut core, b, Some("working"));
            core.mirror.panes.get_mut(b).unwrap().label = label.into();
        }
        reconcile(&mut core, ["r1".to_string(), "r2".to_string()]);
        let d1 = docket_of(&core, "r1").unwrap();
        assert_eq!(docket_of(&core, "r2").as_deref(), Some(d1.as_str()), "one docket for the PR");
        let st = core.store.state(&d1).unwrap().unwrap();
        assert_eq!(st.title, "Review api#341: Add rate limiting");
        assert_eq!(st.protocol, "review");
        assert_eq!(st.review.as_ref().unwrap().round, Some(2));

        // rex-reviewer's prompt doesn't rename it.
        hook(&mut core, "UserPromptSubmit", "claude-code", "r1", &json!({ "prompt": "Read .tmp/ground-rules.md, then .tmp/rounds/2/task.md" })).unwrap();
        assert_eq!(core.store.state(&d1).unwrap().unwrap().title, "Review api#341: Add rate limiting");

        // One pane closing doesn't end the review.
        core.mirror.panes.remove("r1");
        reconcile(&mut core, ["r1".to_string()]);
        if let Some(t) = core.agents.by_block.get_mut("r1") {
            t.missing_since = Some(Instant::now() - GRACE);
        }
        tick(&mut core);
        assert_eq!(core.store.state(&d1).unwrap().unwrap().status, "in_progress");

        // The PR merges and the last pane closes: done.
        write("merged");
        core.reviewer = Default::default();
        core.mirror.panes.remove("r2");
        reconcile(&mut core, ["r2".to_string()]);
        if let Some(t) = core.agents.by_block.get_mut("r2") {
            t.missing_since = Some(Instant::now() - GRACE);
        }
        tick(&mut core);
        assert_eq!(core.store.state(&d1).unwrap().unwrap().status, "done");
        assert!(core.verify(&d1).unwrap().ok);
        let _ = std::fs::remove_dir_all(&dir);
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
