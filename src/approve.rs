//! Guarded approve and deny for an agent's permission prompt (from rex-inbox).
//!
//! Keys go to a pane only if, read from Rex right now, the prompt is the one
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
use crate::rex::Rex;

/// The prompt as the app showed it.
#[derive(Debug, Deserialize)]
pub struct Seen {
    pub record: String,
    pub updated_at: Option<String>,
    pub msg: Option<String>,
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
    let v = Rex.block_call(session, block, "program_status", &json!({})).await?;
    Ok(v["records"].as_array().map(|a| a.iter().filter_map(|r| serde_json::from_value(r.clone()).ok()).collect()).unwrap_or_default())
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

/// Whether the prompt in Rex now is the one that was seen.
pub fn same_prompt(now: Option<&Record>, seen: &Seen) -> bool {
    now.is_some_and(|r| r.state == "blocked" && r.kind.as_deref() == Some("permission") && r.msg == seen.msg && r.updated_at == seen.updated_at)
}

/// Approve (`true`) or deny (`false`) the prompt on a docket's agent.
pub async fn answer(shared: &Shared, docket: &str, approve: bool, seen: &Seen) -> Result<Value> {
    let verb = if approve { "approve" } else { "deny" };
    let (session, block, app) = {
        let core = shared.lock();
        let st = core.store.state(docket)?.ok_or_else(|| anyhow!("no docket {docket}"))?;
        let a = st.agent.ok_or_else(|| anyhow!("{docket} has no agent"))?;
        let p = core.mirror.panes.get(&a.block).ok_or_else(|| anyhow!("{docket}'s pane isn't open in Rex"))?;
        (p.session_id.clone(), p.block_id.clone(), p.app().unwrap_or_default().to_string())
    };
    let Some((yes, no)) = prompt_keys(&app) else {
        bail!("Wheelhouse doesn't answer {}'s prompts yet. Answer it in Rex.", app_name(&app));
    };

    // The guard: read the prompt from Rex now, not from the mirror.
    let records = live_records(&session, &block).await?;
    let now = records.iter().find(|r| r.id == seen.record);
    if !same_prompt(now, seen) {
        bail!("The prompt changed since you saw it: it's now {}. Nothing was sent.", describe(now));
    }
    let waiting = records.iter().filter(|r| r.state == "blocked").count();
    if waiting > 1 {
        bail!("{waiting} prompts are waiting in that pane, so a key could answer the wrong one. Answer it in Rex.");
    }

    let keys = if approve { yes } else { no };
    Rex.send_keys(&session, &block, keys).await?;
    {
        let mut core = shared.lock();
        let owner = core.owner.clone();
        let data = json!({ "decision": verb, "by": owner, "kind": "permission", "record": seen.record, "keys": keys });
        core.append(docket, "approval", &Who::Hub, seen.msg.clone(), data)?;
    }

    // Say whether the prompt went away.
    for _ in 0..20 {
        tokio::time::sleep(Duration::from_millis(250)).await;
        let records = live_records(&session, &block).await?;
        let r = records.into_iter().find(|r| r.id == seen.record);
        if r.as_ref().is_none_or(|r| r.state != "blocked" || r.updated_at != seen.updated_at) {
            return Ok(json!({ "answer": verb, "cleared": true, "now": describe(r.as_ref()) }));
        }
    }
    Ok(json!({ "answer": verb, "cleared": false, "now": "still waiting after 5s" }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(state: &str, kind: Option<&str>, msg: &str, at: &str) -> Record {
        Record {
            id: String::new(),
            state: state.into(),
            kind: kind.map(str::to_string),
            msg: Some(msg.into()),
            updated_at: Some(at.into()),
            ..Default::default()
        }
    }

    #[test]
    fn only_the_prompt_you_saw_is_answered() {
        let seen = Seen { record: String::new(), updated_at: Some("t1".into()), msg: Some("Bash: ls".into()) };
        assert!(same_prompt(Some(&rec("blocked", Some("permission"), "Bash: ls", "t1")), &seen));
        assert!(!same_prompt(Some(&rec("blocked", Some("permission"), "Bash: rm -rf /", "t1")), &seen), "another command");
        assert!(!same_prompt(Some(&rec("blocked", Some("permission"), "Bash: ls", "t2")), &seen), "asked again since");
        assert!(!same_prompt(Some(&rec("blocked", Some("question"), "Bash: ls", "t1")), &seen), "not a permission");
        assert!(!same_prompt(Some(&rec("working", Some("permission"), "Bash: ls", "t1")), &seen), "already answered");
        assert!(!same_prompt(None, &seen), "gone");
    }

    #[test]
    fn keys_only_for_checked_agents() {
        assert_eq!(prompt_keys("claude-code"), Some((&["1"][..], &["escape"][..])));
        assert!(prompt_keys("codex").is_none());
    }
}
