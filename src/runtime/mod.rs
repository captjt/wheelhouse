//! The runtimes: the terminal multiplexers Wheelhouse's agents run in. Rex
//! and Herdr (herdr-core, `hc`) today. This is the only way the hub reaches
//! one; nothing outside `src/runtime/` names a multiplexer's CLI.
//!
//! Each runtime is an adapter with the same verbs (docs/runtimes.md):
//!
//! - **identify**: which pane a process runs in, from its environment
//!   ([`pane_from_env`]), and which runtime a pane id belongs to ([`of`]).
//! - **observe**: a follower that keeps the hub's [`Mirror`] of its
//!   sessions, panes and their agents' status current ([`follow`]).
//! - **deliver**: type text into a pane and submit it, or press keys.
//! - **lay out**: open, close, rename and show panes.
//!
//! Pane ids carry their runtime: Rex's are its own (`block:…`), and every
//! other runtime's start with its name (`herdr:t2h`). So the record, the
//! store and the apps' JSON keep one `block` field for any runtime, and
//! history written before Herdr reads the same.
//!
//! [`Mirror`]: crate::mirror::Mirror

pub mod herdr;
pub mod rex;

use anyhow::Result;
use serde::Serialize;

use crate::mirror::Record;

/// A runtime Wheelhouse knows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Rex,
    Herdr,
}

pub const ALL: [Kind; 2] = [Kind::Rex, Kind::Herdr];

impl Kind {
    /// Its name in settings, ids and JSON: `rex`, `herdr`.
    pub fn name(self) -> &'static str {
        match self {
            Kind::Rex => "rex",
            Kind::Herdr => "herdr",
        }
    }

    /// Its name for people: `Rex`, `Herdr`.
    pub fn title(self) -> &'static str {
        match self {
            Kind::Rex => "Rex",
            Kind::Herdr => "Herdr",
        }
    }

    pub fn parse(s: &str) -> Option<Kind> {
        ALL.into_iter().find(|k| k.name() == s.trim().to_ascii_lowercase())
    }

    pub fn get(self) -> Runtime {
        match self {
            Kind::Rex => Runtime::Rex(rex::Rex),
            Kind::Herdr => Runtime::Herdr(herdr::Herdr),
        }
    }
}

/// The runtime a pane or session id belongs to. Rex's ids have no prefix
/// of ours, so anything without one is Rex's.
pub fn of(id: &str) -> Kind {
    if id.starts_with(herdr::PREFIX) { Kind::Herdr } else { Kind::Rex }
}

/// The runtime of the pane or session `id`.
pub fn for_id(id: &str) -> Runtime {
    of(id).get()
}

/// The pane this process runs in, as a pane id, from the runtime's own
/// variables: `REX_BLOCK` in Rex, `HC_TERMINAL_ID` in herdr-core.
pub fn pane_from_env() -> Option<String> {
    let var = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
    var("REX_BLOCK").or_else(|| var("HC_TERMINAL_ID").map(|t| herdr::pane_id(&t)))
}

/// Where Wheelhouse opens the agents it starts (`wh start`, Build, Review):
/// `$WH_RUNTIME`, else `runtime` in settings.json, else Rex.
pub fn spawn_kind() -> Kind {
    if let Some(k) = std::env::var("WH_RUNTIME").ok().as_deref().and_then(Kind::parse) {
        return k;
    }
    crate::settings::load().ok().and_then(|v| v["runtime"].as_str().and_then(Kind::parse)).unwrap_or(Kind::Rex)
}

/// One runtime's link, for the apps: `{connected, error, installed}`.
pub fn link_json(m: &crate::mirror::Mirror, k: Kind) -> serde_json::Value {
    let l = m.link(k);
    serde_json::json!({ "connected": l.connected, "error": l.error, "installed": l.installed })
}

/// Every runtime, for the apps and `wh status`: its name, whether it's
/// followed, how many agents run in it, and whether new agents open there.
pub fn status(m: &crate::mirror::Mirror) -> serde_json::Value {
    let spawn = spawn_kind();
    ALL.iter()
        .map(|&k| {
            let mut v = link_json(m, k);
            v["name"] = k.name().into();
            v["title"] = k.title().into();
            v["agents"] = m.panes.values().filter(|p| p.runtime == k && p.is_agent()).count().into();
            v["spawn"] = (k == spawn).into();
            v
        })
        .collect()
}

/// A new pane, as its runtime made it.
#[derive(Debug, Clone)]
pub struct Created {
    pub session_id: String,
    pub window_id: Option<String>,
    pub block_id: String,
}

/// Where a new pane goes in its session.
pub enum Place<'a> {
    /// A tab of its own.
    Tab,
    /// Beside another pane, in its tab: `right`, `left`, `up`, `down`.
    /// A runtime that can't choose a side puts it where it puts splits.
    Beside { anchor: &'a str, dir: &'a str },
}

/// A key, by the names Wheelhouse uses everywhere: `enter`, `escape`,
/// `tab`, `up`, `down`, or one character such as `1`. Each runtime spells
/// them its own way.
pub type Key<'a> = &'a str;

/// A runtime. One variant each; the methods dispatch to its adapter.
#[derive(Clone, Debug)]
pub enum Runtime {
    Rex(rex::Rex),
    Herdr(herdr::Herdr),
}

impl Runtime {
    pub fn kind(&self) -> Kind {
        match self {
            Runtime::Rex(_) => Kind::Rex,
            Runtime::Herdr(_) => Kind::Herdr,
        }
    }

    /// Type `text` into the pane the way a person pastes it (no keys in it,
    /// newlines that don't submit early), then press Enter. Only one text
    /// goes into a pane at a time, from anywhere in the hub, so two never
    /// land in one prompt. `check`: read the pane from its runtime first,
    /// and type only if an agent there is between turns now (not a shell
    /// left behind by one that died, whatever the mirror last saw).
    pub async fn submit_checked(&self, session: &str, block: &str, text: &str, check: bool) -> Result<()> {
        let _typing = Typing::wait(block).await?;
        if check {
            between_turns(&self.records(session, block).await?)?;
        }
        let text = plain(text);
        match self {
            Runtime::Rex(r) => r.submit(session, block, &text).await,
            Runtime::Herdr(h) => h.submit(block, &text).await,
        }
    }

    pub async fn submit(&self, session: &str, block: &str, text: &str) -> Result<()> {
        self.submit_checked(session, block, text, true).await
    }

    /// Type `text` into an agent at work, for it to read at its next step:
    /// read from its runtime first, one text at a time, never into a prompt
    /// it's waiting on.
    pub async fn steer(&self, session: &str, block: &str, text: &str) -> Result<()> {
        let _typing = Typing::wait(block).await?;
        can_steer(&self.records(session, block).await?)?;
        let text = plain(text);
        match self {
            Runtime::Rex(r) => r.submit(session, block, &text).await,
            Runtime::Herdr(h) => h.submit(block, &text).await,
        }
    }

    pub async fn keys(&self, session: &str, block: &str, keys: &[Key<'_>]) -> Result<()> {
        match self {
            Runtime::Rex(r) => r.send_keys(session, block, keys).await,
            Runtime::Herdr(h) => h.keys(block, keys).await,
        }
    }

    /// The last `lines` lines of the pane, as plain text.
    pub async fn capture(&self, session: &str, block: &str, lines: usize) -> Result<String> {
        match self {
            Runtime::Rex(r) => r.capture(session, block, lines).await,
            Runtime::Herdr(h) => h.capture(block, lines).await,
        }
    }

    /// The pane's status records, read from the runtime now rather than
    /// from the mirror (the approval guard).
    pub async fn records(&self, session: &str, block: &str) -> Result<Vec<Record>> {
        match self {
            Runtime::Rex(r) => r.records(session, block).await,
            Runtime::Herdr(h) => h.records(block).await,
        }
    }

    /// Open `argv` in a pane labelled `label`, in `cwd`, without taking
    /// focus: in the session `session` (an id), or a new session named
    /// `name` when it's `None`.
    pub async fn open(&self, session: Option<&str>, name: &str, label: &str, cwd: &str, argv: &[String], place: Place<'_>) -> Result<Created> {
        // An anchor in another runtime (the setting changed mid-task) can't
        // be split: the pane gets a tab of its own.
        let place = match place {
            Place::Beside { anchor, .. } if of(anchor) != self.kind() => Place::Tab,
            p => p,
        };
        match self {
            Runtime::Rex(r) => match session {
                Some(sid) => {
                    let split = match place {
                        Place::Tab => None,
                        Place::Beside { anchor, dir } => Some((dir, anchor)),
                    };
                    r.run_in(sid, label, cwd, argv, split).await
                }
                None => {
                    let a: Vec<&str> = argv.iter().map(String::as_str).collect();
                    r.start(None, name, label, cwd, &a).await
                }
            },
            Runtime::Herdr(h) => h.open(name, label, cwd, argv, place).await,
        }
    }

    pub async fn close(&self, session: &str, block: &str) -> Result<()> {
        match self {
            Runtime::Rex(r) => r.close_block(session, block).await,
            Runtime::Herdr(h) => h.close(block).await,
        }
    }

    /// Name a pane, so others reach it by that name.
    pub async fn rename(&self, session: &str, block: &str, name: &str) -> Result<()> {
        match self {
            Runtime::Rex(r) => r.rename_block(session, block, name).await,
            Runtime::Herdr(h) => h.rename(block, name).await,
        }
    }

    /// Bring the pane to the front in the runtime's app.
    pub async fn focus(&self, session: &str, block: &str, window: Option<&str>) -> Result<()> {
        match self {
            Runtime::Rex(r) => r.focus(session, block, window).await,
            Runtime::Herdr(h) => h.focus(block).await,
        }
    }
}

/// Text that's only text when typed into a terminal: no escape sequences
/// (which could end a bracketed paste and type keys), no other control
/// characters but newlines and tabs, and carriage returns as newlines.
pub fn plain(text: &str) -> String {
    text.replace("\r\n", "\n")
        .chars()
        .map(|c| if c == '\r' { '\n' } else { c })
        .filter(|&c| c == '\n' || c == '\t' || !(c.is_control() || ('\u{80}'..='\u{9f}').contains(&c)))
        .collect()
}

/// Whether a pane's records, read now, are an agent between turns: its root
/// record idle or done, from an agent (by the same names the mirror knows:
/// Rex calls Antigravity `antigravity`).
pub fn between_turns(records: &[Record]) -> Result<()> {
    let root = records.iter().find(|r| r.is_root());
    let agent = |r: &Record| r.effective_app.as_deref().or(r.app.as_deref()).map(crate::mirror::canonical_app).is_none_or(crate::mirror::is_agent_app);
    if root.is_some_and(|r| matches!(r.state.as_str(), "idle" | "done") && agent(r)) {
        return Ok(());
    }
    anyhow::bail!("the pane isn't an agent between turns now ({})", root.map_or("no status", |r| r.state.as_str()))
}

/// Agents that take what's typed while they work, and read it at their
/// next step: steering. Others get it when the turn ends.
pub fn steers(app: &str) -> bool {
    matches!(crate::mirror::canonical_app(app), "claude-code" | "codex")
}

/// Whether a pane's records, read now, are an agent that can be steered:
/// at work (or between turns), never waiting on a prompt of its own.
pub fn can_steer(records: &[Record]) -> Result<()> {
    let root = records.iter().find(|r| r.is_root());
    let app = root.and_then(|r| r.effective_app.as_deref().or(r.app.as_deref())).unwrap_or("");
    let waiting = records.iter().any(|r| r.state == "blocked");
    match root {
        Some(r) if !waiting && steers(app) && matches!(r.state.as_str(), "working" | "starting" | "idle" | "done") => Ok(()),
        _ => anyhow::bail!("the agent can't take a message now ({})", root.map_or("no status", |r| r.state.as_str())),
    }
}

/// The panes being typed into now: one text at a time per pane.
static TYPING: std::sync::Mutex<std::collections::BTreeSet<String>> = std::sync::Mutex::new(std::collections::BTreeSet::new());

pub struct Typing(String);

impl Typing {
    pub fn take(block: &str) -> Option<Self> {
        TYPING.lock().unwrap_or_else(|e| e.into_inner()).insert(block.to_string()).then(|| Typing(block.to_string()))
    }

    /// Wait (a few seconds at most) for the pane to be free.
    pub async fn wait(block: &str) -> Result<Self> {
        for _ in 0..50 {
            if let Some(t) = Self::take(block) {
                return Ok(t);
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        anyhow::bail!("something else is being typed into that pane")
    }
}

impl Drop for Typing {
    fn drop(&mut self) {
        TYPING.lock().unwrap_or_else(|e| e.into_inner()).remove(&self.0);
    }
}

/// The text went in, but Enter didn't: it's waiting in the agent's box.
/// Typing it again would put it there twice, so it isn't tried again.
#[derive(Debug)]
pub struct PastedNoEnter(pub String);

impl std::fmt::Display for PastedNoEnter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "it was pasted, but Enter didn't go in ({}): press Enter in the pane", self.0)
    }
}

impl std::error::Error for PastedNoEnter {}

/// Whether an error from typing means don't type it again.
pub fn pasted(e: &anyhow::Error) -> bool {
    e.downcast_ref::<PastedNoEnter>().is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_agent_between_turns_is_typed_into() {
        let rec = |state: &str, app: Option<&str>| Record { state: state.into(), app: app.map(str::to_string), ..Default::default() };
        for app in ["claude-code", "codex", "agy", "antigravity"] {
            assert!(between_turns(&[rec("idle", Some(app))]).is_ok(), "{app}");
        }
        assert!(between_turns(&[rec("done", None)]).is_ok());
        assert!(between_turns(&[rec("working", Some("claude-code"))]).is_err());
        assert!(between_turns(&[rec("blocked", Some("claude-code"))]).is_err());
        assert!(between_turns(&[rec("idle", Some("zsh"))]).is_err(), "not an agent");
        assert!(between_turns(&[]).is_err(), "no status: a shell");
    }

    #[test]
    fn only_claude_and_codex_at_work_are_steered() {
        let rec = |state: &str, app: &str| Record { state: state.into(), app: Some(app.into()), ..Default::default() };
        assert!(can_steer(&[rec("working", "claude-code")]).is_ok());
        assert!(can_steer(&[rec("working", "codex")]).is_ok());
        assert!(can_steer(&[rec("idle", "claude-code")]).is_ok());
        assert!(can_steer(&[rec("working", "agy")]).is_err(), "Antigravity gets it when the turn ends");
        assert!(can_steer(&[rec("blocked", "claude-code")]).is_err(), "never into a prompt it's waiting on");
        let sub = Record { id: "t1".into(), state: "blocked".into(), app: Some("claude-code".into()), ..Default::default() };
        assert!(can_steer(&[rec("working", "claude-code"), sub]).is_err(), "nor while a subagent waits");
        assert!(can_steer(&[]).is_err());
    }

    #[test]
    fn one_text_at_a_time_per_pane() {
        let a = Typing::take("b1").expect("free");
        assert!(Typing::take("b1").is_none());
        assert!(Typing::take("b2").is_some());
        drop(a);
        assert!(Typing::take("b1").is_some());
    }

    #[test]
    fn a_paste_carries_no_keys() {
        // An escape that would end a bracketed paste, then Enter and Shift-Tab.
        let evil = "fix it\x1b[201~\r\x1b[Zrm -rf ~\x07\u{9b}2J";
        assert_eq!(plain(evil), "fix it[201~\n[Zrm -rf ~2J");
        assert_eq!(plain("a\r\nb\tc\u{7f}"), "a\nb\tc");
        assert_eq!(plain("ünïcode ✓ stays"), "ünïcode ✓ stays");
    }

    #[test]
    fn an_id_names_its_runtime() {
        assert_eq!(of("block:01a20af7-41b2"), Kind::Rex);
        assert_eq!(of("session:01a2"), Kind::Rex);
        assert_eq!(of("a1"), Kind::Rex);
        assert_eq!(of("herdr:t2h"), Kind::Herdr);
        assert_eq!(of("herdr:space:wheelhouse"), Kind::Herdr);
        assert_eq!(Kind::parse(" Herdr"), Some(Kind::Herdr));
        assert_eq!(Kind::parse("tmux"), None);
    }
}
