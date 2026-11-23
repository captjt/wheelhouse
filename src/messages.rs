//! Messages: between agents, and between agents and you (from rex-inbox).
//!
//! - `me` is you; a pane is `pane`, `session/pane`, or its block id.
//! - A message to you is open until you answer it. It can offer up to six
//!   options, shown as buttons; your answer says which you picked.
//! - A message to a pane waits until that pane's agent is between turns,
//!   then it's pasted in and sent. A pane that isn't an agent is never typed
//!   into; it reads its messages with `wh inbox`.
//! - Each message is also an entry on the record: on the sender's docket
//!   (or, for one you send, the recipient's), and your answers on the same
//!   thread. The table here holds the numbers and the delivery state.

use anyhow::{Result, anyhow, bail};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

use crate::agents;
use crate::hub::{Core, Shared, Who};
use crate::mirror::{Pane, app_name};
use crate::store::Message;

pub const ME: &str = "me";
/// Wheelhouse itself, as the sender of a protocol's asks (Build's gates).
pub const HUB: &str = "wheelhouse";
const MAX_OPTIONS: usize = 6;
/// How long an agent must sit between turns before a message goes in, so a
/// message doesn't land on top of someone who just finished typing.
const SETTLE: Duration = Duration::from_secs(2);
/// Longer messages are announced with `wh read N`, not pasted whole.
const MAX_PASTE: usize = 4000;

/// A pane's address: `session/pane`, or the end of its block id unlabeled.
pub fn addr_of(core: &Core, p: &Pane) -> String {
    let session = core.mirror.session_label(&p.session_id);
    let session = if session.is_empty() { short(&p.session_id) } else { session.to_string() };
    let pane = if p.label.is_empty() { short(&p.block_id) } else { p.label.clone() };
    format!("{session}/{pane}")
}

pub(crate) fn short(id: &str) -> String {
    let tail: String = id.chars().rev().take(6).collect::<Vec<_>>().into_iter().rev().collect();
    tail
}

/// The pane an address names. A bare name looks in `near` (the sender's
/// session) first; among several, a lone agent wins.
pub fn resolve<'a>(core: &'a Core, addr: &str, near: Option<&str>) -> Result<&'a Pane> {
    let addr = addr.trim();
    if addr.is_empty() {
        bail!("an address is required");
    }
    let all: Vec<&Pane> = core.mirror.panes.values().collect();
    let matches = |p: &&Pane| -> bool {
        if addr.starts_with("block:") {
            return p.block_id == addr;
        }
        let (session, pane) = match addr.rsplit_once('/') {
            Some((s, n)) => (Some(s), n),
            None => (None, addr),
        };
        let session_ok = session.is_none_or(|s| core.mirror.session_label(&p.session_id) == s || p.session_id.ends_with(s));
        let pane_ok = p.label == pane || (pane.len() >= 4 && p.block_id.ends_with(pane));
        session_ok && pane_ok
    };
    let mut found: Vec<&Pane> = all.into_iter().filter(matches).collect();
    if found.len() > 1
        && !addr.contains('/')
        && let Some(s) = near
    {
        let here: Vec<&Pane> = found.iter().copied().filter(|p| p.session_id == s).collect();
        if !here.is_empty() {
            found = here;
        }
    }
    if found.len() > 1 {
        let agents: Vec<&Pane> = found.iter().copied().filter(|p| p.is_agent()).collect();
        if agents.len() == 1 {
            found = agents;
        }
    }
    match found.len() {
        0 => bail!("no pane is called {addr}. Name one with `wh name NAME`, or list them in the app's Agents view"),
        1 => Ok(found[0]),
        _ => bail!("{addr} could be {}: say which, as session/pane", found.iter().map(|p| addr_of(core, p)).collect::<Vec<_>>().join(", ")),
    }
}

/// Who's calling, from the pane the CLI ran in. A pane that isn't an agent
/// (a shell) is you.
pub struct Caller {
    pub addr: String,
    pub block: Option<String>,
    pub session: Option<String>,
    pub who: Who,
}

pub fn caller(core: &Core, block: Option<&str>) -> Caller {
    match block.and_then(|b| core.mirror.panes.get(b)) {
        Some(p) if p.is_agent() => Caller {
            addr: addr_of(core, p),
            block: Some(p.block_id.clone()),
            session: Some(p.session_id.clone()),
            who: Who::Agent {
                app: p.app().unwrap_or("agent").to_string(),
                block: p.block_id.clone(),
                session: core.agents.by_block.get(&p.block_id).and_then(|t| t.session.clone()),
            },
        },
        Some(p) => Caller { addr: ME.into(), block: None, session: Some(p.session_id.clone()), who: Who::Owner },
        None => Caller { addr: ME.into(), block: None, session: None, who: Who::Owner },
    }
}

/// Send a message. Returns its number.
pub fn send(core: &mut Core, from: &Caller, to: &str, subject: Option<String>, body: &str, options: Vec<String>) -> Result<Message> {
    let body = body.trim_end().to_string();
    if body.trim().is_empty() {
        bail!("an empty message");
    }
    let options: Vec<String> = options.into_iter().map(|o| o.trim().to_string()).filter(|o| !o.is_empty()).collect();
    if options.len() > MAX_OPTIONS {
        bail!("a message offers at most {MAX_OPTIONS} options");
    }
    let (to_addr, to_block) = if to.trim() == ME {
        if from.addr == ME {
            bail!("that's you: send to an agent's pane, or leave a note on a docket");
        }
        (ME.to_string(), None)
    } else {
        let p = resolve(core, to, from.session.as_deref())?;
        if Some(&p.block_id) == from.block.as_ref() {
            bail!("that's your own pane");
        }
        (addr_of(core, p), Some(p.block_id.clone()))
    };
    // The thread lives on the sender's docket, or else the recipient's.
    let docket = from.block.as_deref().and_then(|b| agents::docket_of(core, b)).or_else(|| to_block.as_deref().and_then(|b| agents::docket_of(core, b)));
    let mut m = Message {
        from_addr: from.addr.clone(),
        from_block: from.block.clone(),
        to_addr,
        to_block,
        subject: subject.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
        body,
        options,
        status: if to.trim() == ME { "open".into() } else { "queued".into() },
        created_at: crate::time::now_ms(),
        ..Default::default()
    };
    m.id = core.store.insert_message(&m)?;
    if let Some(d) = &docket {
        let kind = if m.options.is_empty() { "note" } else { "question" };
        let data = json!({ "message": m.id, "from": m.from_addr, "to": m.to_addr, "subject": m.subject, "options": m.options });
        let s = core.append(d, kind, &from.who, Some(m.body.clone()), data)?;
        m.docket = Some(d.clone());
        m.seq = Some(s.seq);
        core.store.set_message_place(m.id, d, s.seq)?;
    }
    let _ = core.events.send(json!({ "type": "message", "id": m.id, "to_block": m.to_block, "to": m.to_addr }));
    Ok(m)
}

/// An ask from Wheelhouse itself, on a docket: a question to you with its
/// options as buttons. Your reply comes back to the hub, which reads it off
/// the record (`replies_to`).
pub fn ask(core: &mut Core, docket: &str, subject: &str, body: &str, options: Vec<String>, data: Value) -> Result<Message> {
    let mut m = Message {
        from_addr: HUB.into(),
        to_addr: ME.into(),
        subject: Some(subject.chars().take(140).collect()),
        body: body.trim_end().to_string(),
        options,
        status: "open".into(),
        created_at: crate::time::now_ms(),
        ..Default::default()
    };
    m.id = core.store.insert_message(&m)?;
    let mut d = json!({ "message": m.id, "from": HUB, "to": ME, "subject": m.subject, "options": m.options });
    if let (Some(o), Some(extra)) = (d.as_object_mut(), data.as_object()) {
        for (k, v) in extra {
            o.insert(k.clone(), v.clone());
        }
    }
    let kind = if m.options.is_empty() { "note" } else { "question" };
    let s = core.append(docket, kind, &Who::Hub, Some(m.body.clone()), d)?;
    m.docket = Some(docket.to_string());
    m.seq = Some(s.seq);
    core.store.set_message_place(m.id, docket, s.seq)?;
    let _ = core.events.send(json!({ "type": "message", "id": m.id, "to": ME }));
    Ok(m)
}

/// The latest reply to a message, if any.
pub fn answer_to(core: &Core, id: i64) -> Option<Message> {
    core.store.messages_where("reply_to = ?1", rusqlite::params![id]).ok()?.into_iter().last()
}

/// Answer a message: with words, an option, or both. Your answer to an
/// agent goes to its pane; an agent's answer to you is open in your inbox.
pub fn reply(core: &mut Core, from: &Caller, id: i64, body: &str, choice: Option<&str>) -> Result<Message> {
    let orig = core.store.message(id)?.ok_or_else(|| anyhow!("no message #{id}"))?;
    let mine = |addr: &str| addr == from.addr || (from.addr == ME && addr == ME);
    if !mine(&orig.to_addr) && !mine(&orig.from_addr) {
        bail!("#{id} isn't to or from you");
    }
    // An option, by its button or its exact words.
    let pick = |c: &str| orig.options.iter().find(|o| o.eq_ignore_ascii_case(c.trim())).cloned();
    let choice = match choice {
        Some(c) => Some(pick(c).ok_or_else(|| anyhow!("#{id} offers {}, not {c}", orig.options.join(", ")))?),
        None => pick(body),
    };
    let body = body.trim_end();
    if body.trim().is_empty() && choice.is_none() {
        bail!("an empty reply");
    }
    // The other end of the thread.
    let to = if mine(&orig.to_addr) { (orig.from_addr.clone(), orig.from_block.clone()) } else { (orig.to_addr.clone(), orig.to_block.clone()) };
    let mut m = Message {
        from_addr: from.addr.clone(),
        from_block: from.block.clone(),
        to_addr: to.0.clone(),
        to_block: to.1.clone(),
        subject: orig.subject.as_ref().map(|s| if s.starts_with("Re: ") { s.clone() } else { format!("Re: {s}") }),
        body: if body.trim().is_empty() { choice.clone().unwrap_or_default() } else { body.to_string() },
        options: Vec::new(),
        reply_to: Some(id),
        choice: choice.clone(),
        // The hub reads a reply to its ask off the record; nothing to deliver.
        status: if to.0 == ME {
            "open".into()
        } else if to.0 == HUB {
            "done".into()
        } else {
            "queued".into()
        },
        created_at: crate::time::now_ms(),
        ..Default::default()
    };
    m.id = core.store.insert_message(&m)?;
    if mine(&orig.to_addr) && orig.status == "open" {
        core.store.set_message_status(id, "answered", None)?;
    }
    if let Some(d) = orig.docket.clone() {
        let kind = if choice.is_some() { "decision" } else { "note" };
        let data = json!({ "message": m.id, "replies_to": id, "from": m.from_addr, "to": m.to_addr, "choice": choice });
        let s = core.append(&d, kind, &from.who, Some(m.body.clone()), data)?;
        m.docket = Some(d.clone());
        m.seq = Some(s.seq);
        core.store.set_message_place(m.id, &d, s.seq)?;
    }
    let _ = core.events.send(json!({ "type": "message", "id": m.id, "to_block": m.to_block, "to": m.to_addr, "reply_to": id }));
    Ok(m)
}

/// The text pasted into an agent's pane.
pub fn render(m: &Message, docket: Option<&str>) -> String {
    let from = if m.from_addr == ME { "the user (through Wheelhouse)".to_string() } else { m.from_addr.clone() };
    let mut head = format!("[wheelhouse] Message #{} from {from}", m.id);
    if let Some(r) = m.reply_to {
        head.push_str(&format!(", replying to #{r}"));
    }
    if let Some(d) = docket {
        head.push_str(&format!(" ({d})"));
    }
    if m.body.len() > MAX_PASTE {
        return format!("{head}. It's long: read it with `wh read {}`.", m.id);
    }
    let mut out = head;
    if let Some(s) = &m.subject {
        out.push_str(&format!("\nSubject: {s}"));
    }
    if let Some(c) = m.choice.as_ref().filter(|c| c.as_str() != m.body.trim()) {
        out.push_str(&format!("\nChose: {c}"));
    }
    out.push_str("\n\n");
    out.push_str(m.body.trim_end());
    if let Some(first) = m.options.first() {
        out.push_str(&format!("\n\nOptions: {}\nPick one with `wh reply {} --choice \"{first}\"`, and add words if you like.", m.options.join(" · "), m.id));
    }
    out.push_str(&format!("\n\nReply with `wh reply {} \"...\"` (or `wh reply {} -` with the text on stdin).", m.id, m.id));
    out
}

/// Messages to a pane (or to you) not yet delivered or read.
pub fn inbox(core: &Core, who: &Caller) -> Result<Vec<Message>> {
    match &who.block {
        // Delivered means pasted into the agent's prompt: it has seen it.
        Some(b) => core.store.messages_where("to_block = ?1 AND status = 'queued'", rusqlite::params![b]),
        None => core.store.messages_where("to_addr = 'me' AND status = 'open'", []),
    }
}

/// Read one message, which marks a message to you read.
pub fn read(core: &mut Core, who: &Caller, id: i64) -> Result<Message> {
    let m = core.store.message(id)?.ok_or_else(|| anyhow!("no message #{id}"))?;
    let to_me = who.block.is_some() && m.to_block == who.block;
    if to_me && matches!(m.status.as_str(), "queued" | "delivered") {
        core.store.set_message_status(id, "read", None)?;
    }
    Ok(m)
}

/// Deliver what's due: every second, into agents between turns.
pub async fn deliver_loop(shared: Shared) {
    // The root record each pane had when it last got a message: nothing more
    // goes in until it changes, or a minute passes.
    let mut last: std::collections::HashMap<String, (Option<String>, Instant)> = Default::default();
    // Tries per message (and queued prompt), so one that keeps failing stops.
    let mut tries: std::collections::HashMap<String, u32> = Default::default();
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    let mut swept = Instant::now();
    loop {
        tick.tick().await;
        if swept.elapsed() > Duration::from_secs(600) {
            swept = Instant::now();
            expire(&shared);
        }
        let due = {
            let core = shared.lock();
            plan(&core, &last)
        };
        for (m, session, block, root, docket) in due {
            let text = render(&m, docket.as_deref());
            let rex = crate::rex::Rex;
            let ok = async {
                rex.paste(&session, &block, &text).await?;
                tokio::time::sleep(Duration::from_millis(150)).await;
                rex.send_keys(&session, &block, &["enter"]).await
            }
            .await;
            let mut core = shared.lock();
            // Tried this turn, whatever happened: a paste that went in but
            // whose Enter failed isn't pasted again a second later.
            last.insert(block.clone(), (root, Instant::now()));
            match ok {
                Ok(()) => {
                    tries.remove(&format!("m{}", m.id));
                    let _ = core.store.set_message_status(m.id, "delivered", None);
                    core.live_changed();
                }
                Err(e) => {
                    let n = tries.entry(format!("m{}", m.id)).or_default();
                    *n += 1;
                    say!("deliver #{} (try {n}): {e:#}", m.id);
                    if *n >= MAX_TRIES {
                        let _ = core.store.set_message_status(m.id, "failed", Some(&format!("couldn't type it into the pane: {e:#}")));
                        core.live_changed();
                    }
                }
            }
        }
        deliver_prompts(&shared, &mut last, &mut tries).await;
    }
}

type Due = (Message, String, String, Option<String>, Option<String>);

fn plan(core: &Core, last: &std::collections::HashMap<String, (Option<String>, Instant)>) -> Vec<Due> {
    let Ok(queued) = core.store.messages_where("status = 'queued' AND to_block IS NOT NULL", []) else { return Vec::new() };
    let mut seen = std::collections::HashSet::new();
    let mut due = Vec::new();
    for m in queued {
        let Some(block) = m.to_block.clone() else { continue };
        // Oldest first, one per pane per pass.
        if !seen.insert(block.clone()) {
            continue;
        }
        // A pane that closed keeps its messages; `wh inbox` there finds none.
        let Some((session, root)) = ready(core, &block, last) else { continue };
        let docket = agents::docket_of(core, &block);
        due.push((m, session, block, root, docket));
    }
    due
}

/// A pane that can take something typed now: an agent between turns, settled
/// for a moment, that hasn't had something typed this turn. Its session, and
/// its root record's time.
fn ready(core: &Core, block: &str, last: &std::collections::HashMap<String, (Option<String>, Instant)>) -> Option<(String, Option<String>)> {
    // A mirror that lost Rex may be stale: the agent may have exited and
    // left a shell, which would run the message's lines as commands.
    if !core.mirror.connected {
        return None;
    }
    let p = core.mirror.panes.get(block)?;
    if !p.is_agent() {
        return None;
    }
    let root = p.root()?;
    if root.state != "idle" && root.state != "done" {
        return None;
    }
    let settled = root.updated_at.as_deref().and_then(crate::time::parse).is_some_and(|t| (time::OffsetDateTime::now_utc() - t) >= SETTLE);
    if !settled {
        return None;
    }
    if let Some((upd, at)) = last.get(block)
        && *upd == root.updated_at
        && at.elapsed() < Duration::from_secs(60)
    {
        return None;
    }
    Some((p.session_id.clone(), root.updated_at.clone()))
}

/// Type the prompts you queued for agents that were working, as each one's
/// turn ends: oldest first, one per pane per turn, after any message.
/// Tries before a message or a queued prompt is given up on.
const MAX_TRIES: u32 = 5;

/// A day on: messages and queued prompts for panes that never came back are
/// given up, so they don't wait (and get checked) forever.
fn expire(shared: &Shared) {
    let mut core = shared.lock();
    let cutoff = 24 * 3600;
    let old = |at: &str| crate::time::secs_since(at).is_some_and(|s| s > cutoff);
    for m in core.store.messages_where("status = 'queued' AND to_block IS NOT NULL", []).unwrap_or_default() {
        if old(&m.created_at) && m.to_block.as_ref().is_none_or(|b| !core.mirror.panes.contains_key(b)) {
            let _ = core.store.set_message_status(m.id, "failed", Some("its pane closed, and didn't come back in a day"));
        }
    }
    for (id, docket, block, _, at) in core.store.queued_prompts(None).unwrap_or_default() {
        if old(&at) && !core.mirror.panes.contains_key(&block) {
            let _ = core.store.set_prompt_status(id, &docket, "cancelled");
        }
    }
}

async fn deliver_prompts(shared: &Shared, last: &mut std::collections::HashMap<String, (Option<String>, Instant)>, tries: &mut std::collections::HashMap<String, u32>) {
    let due: Vec<(i64, String, String, String, String, Option<String>)> = {
        let core = shared.lock();
        let mut seen = std::collections::HashSet::new();
        core.store
            .queued_prompts(None)
            .unwrap_or_default()
            .into_iter()
            .filter(|(_, _, block, _, _)| seen.insert(block.clone()))
            .filter_map(|(id, docket, block, body, _)| ready(&core, &block, last).map(|(session, root)| (id, docket, block, body, session, root)))
            .collect()
    };
    for (id, docket, block, body, session, root) in due {
        let rex = crate::rex::Rex;
        let ok = async {
            rex.paste(&session, &block, &body).await?;
            tokio::time::sleep(Duration::from_millis(150)).await;
            rex.send_keys(&session, &block, &["enter"]).await
        }
        .await;
        let mut core = shared.lock();
        last.insert(block.clone(), (root, Instant::now()));
        match ok {
            Ok(()) => {
                tries.remove(&format!("p{id}"));
                let _ = core.store.set_prompt_status(id, &docket, "sent");
                let app = core.mirror.panes.get(&block).and_then(|p| p.app()).unwrap_or("agent").to_string();
                let session_id = core.agents.by_block.get(&block).and_then(|t| t.session.clone());
                if !crate::hooks::installed_for(&app) {
                    let _ = agents::note_prompt(&mut core, &docket, &body, session_id.as_deref(), Some("wheelhouse"));
                }
                core.agents.remote.insert(docket.clone());
            }
            Err(e) => {
                let n = tries.entry(format!("p{id}")).or_default();
                *n += 1;
                say!("{docket}: typing a queued prompt (try {n}): {e:#}");
                if *n >= MAX_TRIES {
                    let _ = core.store.set_prompt_status(id, &docket, "cancelled");
                }
            }
        }
        core.live_changed();
    }
}

/// A message as the apps show it.
pub fn view(core: &Core, m: &Message) -> Value {
    let from_app = m.from_block.as_deref().and_then(|b| core.mirror.panes.get(b)).and_then(|p| p.app()).map(app_name);
    json!({
        "id": m.id, "docket": m.docket, "from": m.from_addr, "from_app": from_app, "to": m.to_addr,
        "subject": m.subject, "body": m.body, "options": m.options, "reply_to": m.reply_to, "choice": m.choice,
        "status": m.status, "created_at": m.created_at, "preview": m.preview,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hub::test_core;
    use crate::mirror::Record;

    fn agent(core: &mut Core, block: &str, label: &str, state: &str) {
        let fg = json!({ "foreground": { "name": "claude", "cwd": "/w" } });
        core.mirror.apply_pane_status("s1", block, vec![Record { state: state.into(), app: Some("claude-code".into()), ..Default::default() }], Some(&fg), None);
        core.mirror.panes.get_mut(block).unwrap().label = label.into();
        agents::reconcile(core, [block.to_string()]);
    }

    #[test]
    fn an_agent_asks_you_and_your_answer_goes_back_on_the_thread() {
        let mut core = test_core();
        agent(&mut core, "b1", "builder", "idle");
        let from = caller(&core, Some("b1"));
        let m = send(&mut core, &from, "me", Some("PR #12".into()), "Tests pass. Merge?", vec!["Merge".into(), "Wait".into()]).unwrap();
        assert_eq!(m.status, "open");
        let d = m.docket.clone().expect("on the builder's docket");
        assert_eq!(inbox(&core, &caller(&core, None)).unwrap().len(), 1);

        // Picking an option by its words.
        let me = caller(&core, None);
        assert!(reply(&mut core, &me, m.id, "", Some("Later")).is_err(), "not an option");
        let r = reply(&mut core, &me, m.id, "merge", None).unwrap();
        assert_eq!(r.choice.as_deref(), Some("Merge"));
        assert_eq!(r.to_block.as_deref(), Some("b1"));
        assert_eq!(r.status, "queued");
        assert_eq!(core.store.message(m.id).unwrap().unwrap().status, "answered");
        let kinds: Vec<String> = core.store.entries(&d).unwrap().iter().map(|s| s.entry().unwrap().kind).collect();
        assert!(kinds.ends_with(&["question".to_string(), "decision".to_string()]), "{kinds:?}");
        assert!(core.verify(&d).unwrap().ok);

        let text = render(&r, Some(&d));
        assert!(text.starts_with(&format!("[wheelhouse] Message #{} from the user", r.id)), "{text}");
        assert!(text.contains(&format!("wh reply {}", r.id)));
    }

    #[test]
    fn agents_message_each_other_by_name() {
        let mut core = test_core();
        agent(&mut core, "b1", "planner", "idle");
        agent(&mut core, "b2", "reviewer", "working");
        let from = caller(&core, Some("b1"));
        let m = send(&mut core, &from, "reviewer", None, "Does /v2/items need pagination?", vec![]).unwrap();
        assert_eq!(m.to_block.as_deref(), Some("b2"));
        assert_eq!(m.status, "queued");
        assert!(send(&mut core, &from, "nobody", None, "hi", vec![]).is_err());
        assert!(send(&mut core, &from, "planner", None, "hi", vec![]).is_err(), "your own pane");
        // The reviewer is working: nothing is due yet.
        assert!(plan(&core, &Default::default()).is_empty());
        let mine = inbox(&core, &caller(&core, Some("b2"))).unwrap();
        assert_eq!(mine.len(), 1);
        let reviewer = caller(&core, Some("b2"));
        let read_it = read(&mut core, &reviewer, m.id).unwrap();
        assert_eq!(read_it.body, "Does /v2/items need pagination?");
        assert_eq!(core.store.message(m.id).unwrap().unwrap().status, "read");
    }
}
