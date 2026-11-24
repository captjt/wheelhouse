//! Review panes in Rex: opened in their own session (a tab per PR), handed
//! their prompt once the agent is ready, and closed when the round is done.
//! (What rex-agent did for rex-reviewer.)

use anyhow::{Result, anyhow, bail};
use std::time::{Duration, Instant};

use super::session_name;
use crate::hub::Shared;
use crate::rex::{Created, Rex};

/// The review session's id, if it's open.
pub fn session(shared: &Shared) -> Option<String> {
    session_named(shared, &session_name())
}

/// A Rex session's id by its label, if it's open.
pub fn session_named(shared: &Shared, name: &str) -> Option<String> {
    let core = shared.lock();
    core.mirror.sessions.values().find(|s| s.label == name).map(|s| s.session_id.clone())
}

/// A pane's root state and when it last changed, or `None` if it's gone.
pub fn state(shared: &Shared, block: &str) -> Option<(String, Option<String>)> {
    let core = shared.lock();
    let p = core.mirror.panes.get(block)?;
    Some(match p.root() {
        Some(r) => (r.state.clone(), r.updated_at.clone()),
        None => ("unknown".into(), None),
    })
}

/// The block of the pane labelled `label` in `session` (or the review session).
pub fn find(shared: &Shared, session: Option<&str>, label: &str) -> Option<String> {
    find_in(shared, session, &session_name(), label)
}

/// The block labelled `label` in `session`, or else in the session named `name`.
pub fn find_in(shared: &Shared, session: Option<&str>, name: &str, label: &str) -> Option<String> {
    let core = shared.lock();
    let sid = match session {
        Some(s) => s.to_string(),
        None => core.mirror.sessions.values().find(|s| s.label == name)?.session_id.clone(),
    };
    core.mirror.panes.values().find(|p| p.session_id == sid && p.label == label).map(|p| p.block_id.clone())
}

pub enum Place<'a> {
    Tab,
    Beside { anchor: &'a str, dir: &'a str },
}

/// Open `argv` in a pane labelled `label`, in the review session (made
/// with this pane if it isn't open).
pub async fn open(shared: &Shared, label: &str, cwd: &str, argv: &[String], place: Place<'_>) -> Result<Created> {
    open_in(shared, &session_name(), label, cwd, argv, place).await
}

/// Open a pane in the session named `name` (made with it if it isn't open).
pub async fn open_in(shared: &Shared, name: &str, label: &str, cwd: &str, argv: &[String], place: Place<'_>) -> Result<Created> {
    let rex = Rex;
    match session_named(shared, name) {
        Some(sid) => {
            let split = match place {
                Place::Tab => None,
                Place::Beside { anchor, dir } => Some((dir, anchor)),
            };
            rex.run_in(&sid, label, cwd, argv, split).await
        }
        None => {
            let a: Vec<&str> = argv.iter().map(String::as_str).collect();
            let c = rex.start(None, name, label, cwd, &a).await?;
            // Wait for the mirror to see the new session, so the next pane
            // goes in it.
            let t = Instant::now();
            while session_named(shared, name).is_none() && t.elapsed() < Duration::from_secs(10) {
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
            Ok(c)
        }
    }
}

/// Wait until the agent in `block` settles (idle, done, blocked or error).
pub async fn settled(shared: &Shared, block: &str, timeout: Duration) -> Result<String> {
    let t = Instant::now();
    let mut seen = false;
    loop {
        match state(shared, block) {
            Some((s, _)) => {
                seen = true;
                if matches!(s.as_str(), "idle" | "done" | "blocked" | "error") {
                    return Ok(s);
                }
            }
            None if seen => bail!("the pane closed"),
            None => {}
        }
        if t.elapsed() > timeout {
            bail!("it didn't settle within {}s", timeout.as_secs());
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

/// Hand the agent its prompt: wait until it isn't busy, paste, then Enter.
/// Agents report idle at start and then go busy while their hooks run, so
/// a busy agent is tried again, up to eight times.
pub async fn deliver(shared: &Shared, session: &str, block: &str, prompt: &str) -> Result<()> {
    let rex = Rex;
    for _ in 0..8 {
        let t = Instant::now();
        while t.elapsed() < Duration::from_secs(60) {
            match state(shared, block) {
                None => bail!("the pane closed"),
                Some((s, _)) if s == "working" => tokio::time::sleep(Duration::from_millis(500)).await,
                Some(_) => break,
            }
        }
        match state(shared, block).map(|s| s.0) {
            None => bail!("the pane closed"),
            Some(s) if s == "blocked" => bail!("it's waiting on a prompt of its own"),
            Some(s) if s == "working" => {
                tokio::time::sleep(Duration::from_secs(3)).await;
                continue;
            }
            Some(_) => {}
        }
        rex.paste(session, block, prompt).await?;
        tokio::time::sleep(Duration::from_millis(200)).await;
        rex.send_keys(session, block, &["enter"]).await?;
        return Ok(());
    }
    Err(anyhow!("it stayed busy and never took its task"))
}

/// An Antigravity agent's conversation, once it has its first prompt: its
/// hook names it, or its own files do (with the hooks off). An interactive
/// agy starts the conversation only then, so this runs after `deliver`.
pub async fn agy_session(shared: &Shared, block: &str, cwd: &std::path::Path, since: i64) -> Option<String> {
    for _ in 0..30 {
        let hooked = shared.lock().agents.by_block.get(block).and_then(|t| t.session.clone());
        if let Some(s) = hooked.or_else(|| super::agentcli::agy_conversation(cwd, since)) {
            return Some(s);
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    None
}

pub async fn close(session: &str, block: &str) {
    if let Err(e) = Rex.close_block(session, block).await {
        say!("review: closing {block}: {e:#}");
    }
}

/// The last lines of a pane, to say why an agent didn't start.
pub async fn tail(session: &str, block: &str) -> String {
    Rex.capture(session, block, 15).await.unwrap_or_default()
}
