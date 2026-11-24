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
/// When another runtime has a session of the same name (a `wheelhouse` in
/// Rex and in Herdr), the address names its runtime: `herdr:wheelhouse/D-12`.
pub fn addr_of(core: &Core, p: &Pane) -> String {
    let label = core.mirror.session_label(&p.session_id);
    let session = if label.is_empty() { short(&p.session_id) } else { label.to_string() };
    let pane = if p.label.is_empty() { short(&p.block_id) } else { p.label.clone() };
    let shared = !label.is_empty() && core.mirror.sessions.values().any(|s| s.runtime != p.runtime && s.label == label);
    if shared { format!("{}:{session}/{pane}", p.runtime.name()) } else { format!("{session}/{pane}") }
}

/// An address that names its runtime (`herdr:wheelhouse/D-12`, `rex:D-12`):
/// the runtime, and the address in it. A pane id (`herdr:t2h`) isn't one.
fn qualified<'a>(core: &Core, addr: &'a str) -> (Option<crate::runtime::Kind>, &'a str) {
    match addr.split_once(':') {
        Some((k, rest)) if !rest.is_empty() && !core.mirror.panes.contains_key(addr) => match crate::runtime::Kind::parse(k) {
            Some(kind) => (Some(kind), rest),
            None => (None, addr),
        },
        _ => (None, addr),
    }
}

/// The end of an id, to tell unnamed panes apart: a Herdr terminal's own
/// address (`herdr:t5` → `t5`), else an id's last six characters.
pub(crate) fn short(id: &str) -> String {
    if let Some(rest) = id.strip_prefix(crate::runtime::herdr::PREFIX) {
        return rest.rsplit(':').next().unwrap_or(rest).to_string();
    }
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
    // A pane id names its pane, in any runtime.
    if let Some(p) = core.mirror.panes.get(addr) {
        return Ok(p);
    }
    if addr.starts_with("block:") {
        bail!("no pane is called {addr}: it closed, or its runtime isn't followed now");
    }
    let (kind, addr) = qualified(core, addr);
    let all: Vec<&Pane> = core.mirror.panes.values().filter(|p| kind.is_none_or(|k| p.runtime == k)).collect();
    let matches = |p: &&Pane| -> bool {
        let (session, pane) = match addr.rsplit_once('/') {
            Some((s, n)) => (Some(s), n),
            None => (None, addr),
        };
        let session_ok = session.is_none_or(|s| core.mirror.session_label(&p.session_id) == s || p.session_id.ends_with(s));
        let pane_ok = p.label == pane || (pane.len() >= 4 && p.block_id.ends_with(pane)) || (p.runtime == crate::runtime::Kind::Herdr && short(&p.block_id) == pane);
        session_ok && pane_ok
    };
    let mut found: Vec<&Pane> = all.into_iter().filter(matches).collect();
    // Among several, the sender's own session first: for a bare name, and
    // for a session both runtimes have. Across runtimes it's never a guess:
    // otherwise the sender is asked to say which.
    if found.len() > 1
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
        _ => {
            // Two panes of one name in one session: only their ids differ.
            let mut say: Vec<String> = found.iter().map(|p| addr_of(core, p)).collect();
            if say.iter().collect::<std::collections::BTreeSet<_>>().len() < say.len() {
                say = found.iter().map(|p| p.block_id.clone()).collect();
            }
            bail!("{addr} could be {}: say which", say.join(", "))
        }
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

/// The docket an `@D-12` address names, if it's one.
fn docket_addr(to: &str) -> Option<String> {
    let d = to.trim().strip_prefix('@')?.trim().to_ascii_uppercase();
    Some(if d.chars().all(|c| c.is_ascii_digit()) { format!("D-{d}") } else { d })
}

/// A message to every agent on a docket (`@D-12`) but the sender: one to
/// each, on one entry of the docket's record, so replies stay on its thread.
fn send_docket(core: &mut Core, from: &Caller, docket: &str, subject: Option<String>, body: String, options: Vec<String>) -> Result<Message> {
    let st = core.store.state(docket)?.ok_or_else(|| anyhow!("no docket {docket}"))?;
    let to: Vec<(String, String)> = st
        .agents_on()
        .filter(|a| Some(&a.agent.block) != from.block.as_ref() && agents::docket_of(core, &a.agent.block).as_deref() == Some(docket))
        .filter_map(|a| core.mirror.panes.get(&a.agent.block).map(|p| (addr_of(core, p), p.block_id.clone())))
        .collect();
    if to.is_empty() {
        bail!("no other agent is on {docket} now");
    }
    let subject = subject.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    let mut sent = Vec::new();
    for (addr, block) in &to {
        let mut m = Message {
            from_addr: from.addr.clone(),
            from_block: from.block.clone(),
            to_addr: addr.clone(),
            to_block: Some(block.clone()),
            subject: subject.clone(),
            body: body.clone(),
            options: options.clone(),
            status: "queued".into(),
            created_at: crate::time::now_ms(),
            ..Default::default()
        };
        m.id = core.store.insert_message(&m)?;
        sent.push(m);
    }
    let kind = if options.is_empty() { "note" } else { "question" };
    let ids: Vec<i64> = sent.iter().map(|m| m.id).collect();
    let addrs: Vec<&str> = to.iter().map(|(a, _)| a.as_str()).collect();
    let data = json!({ "message": ids[0], "messages": ids, "from": from.addr, "to": format!("@{docket}"), "agents": addrs, "subject": subject, "options": options });
    let s = core.append(docket, kind, &from.who, Some(body), data)?;
    for m in &mut sent {
        m.docket = Some(docket.to_string());
        m.seq = Some(s.seq);
        core.store.set_message_place(m.id, docket, s.seq)?;
        let _ = core.events.send(json!({ "type": "message", "id": m.id, "to_block": m.to_block, "to": m.to_addr }));
    }
    let mut first = sent.swap_remove(0);
    first.to_addr = format!("@{docket} ({})", addrs.join(", "));
    Ok(first)
}

/// Send a message. Returns its number. `@D-12` sends it to every agent on
/// that docket.
pub fn send(core: &mut Core, from: &Caller, to: &str, subject: Option<String>, body: &str, options: Vec<String>) -> Result<Message> {
    let body = body.trim_end().to_string();
    if body.trim().is_empty() {
        bail!("an empty message");
    }
    let options: Vec<String> = options.into_iter().map(|o| o.trim().to_string()).filter(|o| !o.is_empty()).collect();
    if options.len() > MAX_OPTIONS {
        bail!("a message offers at most {MAX_OPTIONS} options");
    }
    if let Some(d) = docket_addr(to) {
        return send_docket(core, from, &d, subject, body, options);
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
            let ok = crate::runtime::for_id(&block).submit(&session, &block, &text).await;
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
    // A mirror that lost the pane's runtime may be stale: the agent may have
    // exited and left a shell, which would run the message's lines as commands.
    if !core.mirror.connected_for(block) {
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
        let ok = crate::runtime::for_id(&block).submit(&session, &block, &body).await;
        let mut core = shared.lock();
        last.insert(block.clone(), (root, Instant::now()));
        match ok {
            Ok(()) => {
                tries.remove(&format!("p{id}"));
                let _ = core.store.set_prompt_status(id, &docket, "sent");
                let app = core.mirror.panes.get(&block).and_then(|p| p.app()).unwrap_or("agent").to_string();
                let session_id = core.agents.by_block.get(&block).and_then(|t| t.session.clone());
                if !crate::hooks::installed_for(&app) {
                    let _ = agents::note_prompt(&mut core, &docket, &body, session_id.as_deref(), Some(&block), Some("wheelhouse"));
                }
                core.agents.remote.insert(block.clone());
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

    /// An agent in `session` (a label) of runtime `kind`, settled long enough to take a message.
    fn agent_in(core: &mut Core, kind: crate::runtime::Kind, session: &str, block: &str, label: &str) {
        let sid = match kind {
            crate::runtime::Kind::Rex => format!("session:{session}"),
            crate::runtime::Kind::Herdr => format!("herdr:space:{session}"),
        };
        core.mirror.sessions.insert(sid.clone(), crate::mirror::Session { runtime: kind, session_id: sid.clone(), label: session.into(), windows: vec![] });
        let fg = json!({ "foreground": { "name": "claude", "cwd": "/w" } });
        let rec = Record { state: "idle".into(), app: Some("claude-code".into()), updated_at: Some("2026-01-01T00:00:00Z".into()), ..Default::default() };
        core.mirror.apply_pane_status(&sid, block, vec![rec], Some(&fg), None);
        core.mirror.panes.get_mut(block).unwrap().label = label.into();
        core.mirror.set_link(kind, true, None, true);
        agents::reconcile(core, [block.to_string()]);
    }

    #[test]
    fn agents_in_rex_and_herdr_message_each_other() {
        use crate::runtime::Kind::{Herdr, Rex};
        let mut core = test_core();
        agent_in(&mut core, Rex, "wheelhouse", "block:r1", "D-1");
        agent_in(&mut core, Rex, "api", "block:r2", "builder");
        agent_in(&mut core, Herdr, "wheelhouse", "herdr:t5", "D-1");
        agent_in(&mut core, Herdr, "web", "herdr:t6", "reviewer");

        // A session name both runtimes have: its addresses name the runtime.
        assert_eq!(addr_of(&core, &core.mirror.panes["block:r1"]), "rex:wheelhouse/D-1");
        assert_eq!(addr_of(&core, &core.mirror.panes["herdr:t5"]), "herdr:wheelhouse/D-1");
        // A name only one has doesn't.
        assert_eq!(addr_of(&core, &core.mirror.panes["herdr:t6"]), "web/reviewer");
        assert_eq!(addr_of(&core, &core.mirror.panes["block:r2"]), "api/builder");

        let to = |addr: &str, near: Option<&str>| resolve(&core, addr, near).map(|p| p.block_id.clone()).map_err(|e| e.to_string());
        assert_eq!(to("herdr:wheelhouse/D-1", None).unwrap(), "herdr:t5");
        assert_eq!(to("rex:wheelhouse/D-1", None).unwrap(), "block:r1");
        assert_eq!(to("herdr:D-1", None).unwrap(), "herdr:t5");
        assert_eq!(to("herdr:t5", None).unwrap(), "herdr:t5", "a pane id");
        assert_eq!(to("block:r1", None).unwrap(), "block:r1");
        assert_eq!(to("reviewer", None).unwrap(), "herdr:t6", "a name only Herdr has, from Rex");
        assert_eq!(to("builder", Some("herdr:space:web")).unwrap(), "block:r2", "a name only Rex has, from Herdr");
        // The same name in both: the sender's own session wins, else say which.
        assert_eq!(to("wheelhouse/D-1", Some("herdr:space:wheelhouse")).unwrap(), "herdr:t5");
        assert_eq!(to("D-1", Some("session:wheelhouse")).unwrap(), "block:r1");
        let e = to("wheelhouse/D-1", None).unwrap_err();
        assert!(e.contains("rex:wheelhouse/D-1") && e.contains("herdr:wheelhouse/D-1"), "{e}");
        // From a third session, D-1 is in both runtimes: say which.
        assert!(to("D-1", Some("session:api")).unwrap_err().contains("herdr:wheelhouse/D-1"));
        assert!(to("rex:reviewer", None).is_err(), "reviewer isn't in Rex");
        assert!(to("herdr:t99", None).is_err());
        assert_eq!(to("web/t6", None).unwrap(), "herdr:t6", "an unnamed terminal by its address");
        assert_eq!(short("herdr:t6"), "t6");
        assert_eq!(short("block:01a20af7-41b2-7806"), "2-7806");

        // Rex to Herdr and back, each delivered to its pane in its own runtime.
        let from_rex = caller(&core, Some("block:r2"));
        let m = send(&mut core, &from_rex, "reviewer", None, "Can you look at #12?", vec![]).unwrap();
        assert_eq!((m.to_addr.as_str(), m.to_block.as_deref()), ("web/reviewer", Some("herdr:t6")));
        assert_eq!(m.from_addr, "api/builder");
        let from_herdr = caller(&core, Some("herdr:t6"));
        let r = send(&mut core, &from_herdr, &m.from_addr, None, "On it.", vec![]).unwrap();
        assert_eq!(r.to_block.as_deref(), Some("block:r2"));
        let due: Vec<String> = plan(&core, &Default::default()).into_iter().map(|d| d.2).collect();
        assert!(due.contains(&"herdr:t6".to_string()) && due.contains(&"block:r2".to_string()), "{due:?}");

        // A runtime that's lost: nothing goes into its panes, the other's still do.
        core.mirror.set_link(Herdr, false, Some("gone".into()), true);
        let due: Vec<String> = plan(&core, &Default::default()).into_iter().map(|d| d.2).collect();
        assert_eq!(due, ["block:r2"]);
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
    fn a_message_to_a_docket_reaches_every_agent_on_it_on_one_thread() {
        let mut core = test_core();
        let d = core.open(&Who::Owner, "Shared", json!({ "protocol": "none" })).unwrap();
        for (b, l) in [("b1", "planner"), ("b2", "builder"), ("b3", "reviewer")] {
            agent(&mut core, b, l, "idle");
            agents::join(&mut core, b, &d, None).unwrap();
        }
        agent(&mut core, "b4", "elsewhere", "idle");
        let from = caller(&core, Some("b1"));
        let num = d.trim_start_matches("D-");
        let m = send(&mut core, &from, &format!("@{num}"), None, "I'm taking the fold; who has the apps?", vec![]).unwrap();
        assert!(m.to_addr.starts_with(&format!("@{d}")), "{}", m.to_addr);
        let to: Vec<Option<String>> = core.store.messages_where("status = 'queued'", []).unwrap().into_iter().map(|m| m.to_block).collect();
        assert_eq!(to, [Some("b2".to_string()), Some("b3".to_string())], "everyone on it but the sender");
        let e = core.store.entries(&d).unwrap().last().unwrap().entry().unwrap();
        assert_eq!(e.data["to"], format!("@{d}"));
        assert_eq!(e.data["messages"].as_array().unwrap().len(), 2);
        // A reply stays on the docket's thread.
        let b3 = caller(&core, Some("b3"));
        let r = reply(&mut core, &b3, m.id + 1, "Me, the apps.", None).unwrap();
        assert_eq!(r.docket.as_deref(), Some(d.as_str()));
        assert_eq!(r.to_block.as_deref(), Some("b1"));
        // From outside it, too; a docket with nobody else on it says so.
        let me = caller(&core, None);
        assert!(send(&mut core, &me, &format!("@{d}"), None, "Status?", vec![]).is_ok());
        let lone = core.open(&Who::Owner, "Lone", json!({ "protocol": "none" })).unwrap();
        assert!(send(&mut core, &from, &format!("@{lone}"), None, "hi", vec![]).unwrap_err().to_string().contains("no other agent"));
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
