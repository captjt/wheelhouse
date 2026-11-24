//! Mac notifications for what needs you: an agent waiting on a prompt or
//! failed, and a message to you. `WH_NOTIFY=off` turns them off.
//!
//! Each waits two seconds and is dropped if it resolved meanwhile. While
//! rex-inbox runs, it already notifies for prompts and failures, so
//! Wheelhouse notifies only for its own messages.
//!
//! While Wheelhouse.app listens (`/api/events?client=mac`), it gets each as a
//! `notify` event and posts it, with actions (Approve, a reply, Post); the
//! hub posts nothing itself. Otherwise, with terminal-notifier on PATH, a
//! click opens the docket; without it, osascript posts it.

use serde_json::Value;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crate::hub::Shared;

const GRACE: Duration = Duration::from_secs(2);
const BURST: usize = 5;
const WINDOW: Duration = Duration::from_secs(30);

pub async fn run(shared: Shared) {
    if std::env::var("WH_NOTIFY").is_ok_and(|v| v == "off" || v == "0") {
        return;
    }
    let mut rx = shared.lock().events.subscribe();
    let mut recent: VecDeque<Instant> = VecDeque::new();
    loop {
        let ev = match rx.recv().await {
            Ok(ev) => ev,
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
            Err(_) => return,
        };
        let kind = ev["type"].as_str().unwrap_or_default();
        if kind != "needs" && kind != "review" && kind != "build" && kind != "turn" && !(kind == "message" && ev["to"] == "me") {
            continue;
        }
        if kind == "needs" && rex_inbox_running() {
            continue;
        }
        let shared = shared.clone();
        while recent.front().is_some_and(|t| t.elapsed() > WINDOW) {
            recent.pop_front();
        }
        if recent.len() >= BURST {
            continue;
        }
        recent.push_back(Instant::now());
        tokio::spawn(async move {
            tokio::time::sleep(GRACE).await;
            if let Some(n) = still_due(&shared, &ev) {
                let app = {
                    let core = shared.lock();
                    let _ = core.events.send(n.event());
                    core.mac_apps.load(std::sync::atomic::Ordering::SeqCst) > 0
                };
                if !app {
                    let url = format!("http://{}{}", crate::server::http_addr().map(|a| a.to_string()).unwrap_or_else(|| "127.0.0.1:7500".into()), n.path);
                    post(&n.title, &n.subtitle, &n.body, &url, &n.group).await;
                }
            }
        });
    }
}

/// A notification: what, where, and the app's path to open on a click.
pub struct Note {
    pub title: String,
    pub subtitle: String,
    pub body: String,
    pub path: String,
    pub group: String,
    /// What it's about, for the actions Wheelhouse.app offers on it: the
    /// event's type, its docket, and a message's id or a review's PR.
    pub kind: &'static str,
    pub docket: Option<String>,
    pub message: Option<i64>,
    pub key: Option<String>,
    /// Its sound, as a sound pack's category (CESP: openpeon.com/spec):
    /// `input.required` when it waits on you, `task.complete` when work
    /// finished, `task.error` when it failed or stalled. Wheelhouse.app
    /// plays one from the pack you picked, if you picked one.
    pub sound: &'static str,
    /// What an answer from the banner must match: a prompt as it was shown
    /// (record, time, words), or the checked review's digest.
    pub seen: Value,
}

impl Note {
    /// The `notify` event Wheelhouse.app posts.
    pub fn event(&self) -> Value {
        serde_json::json!({
            "type": "notify",
            "kind": self.kind,
            "title": self.title,
            "subtitle": self.subtitle,
            "body": self.body,
            "group": self.group,
            "docket": self.docket,
            "message": self.message,
            "key": self.key,
            "sound": self.sound,
            "seen": self.seen,
        })
    }
}

/// The notification, if what it's about still needs you.
pub fn still_due(shared: &Shared, ev: &Value) -> Option<Note> {
    let core = shared.lock();
    let base = "";
    match ev["type"].as_str()? {
        "needs" => {
            let block = ev["block"].as_str()?;
            let p = core.mirror.panes.get(block)?;
            let h = p.headline()?;
            if h.state != ev["what"].as_str()? || h.updated_at.as_deref() != ev["updated_at"].as_str() {
                return None;
            }
            let docket = ev["docket"].as_str()?.to_string();
            let title = core.store.state(&docket).ok()??.title;
            let app = crate::mirror::app_name(p.app().unwrap_or("agent")).to_string();
            let what = if h.state == "error" { "failed".to_string() } else { h.kind.clone().map(|k| format!("waits on you ({k})")).unwrap_or("waits on you".into()) };
            Some(Note {
                title: format!("{app} {what}"),
                subtitle: format!("{docket} · {title}"),
                body: h.msg.clone().unwrap_or_default(),
                path: format!("{base}/#/d/{docket}"),
                group: format!("wh-{docket}"),
                kind: "needs",
                docket: Some(docket),
                message: None,
                key: None,
                sound: if h.state == "error" { "task.error" } else { "input.required" },
                seen: serde_json::json!({ "record": h.id, "updated_at": h.updated_at, "msg": h.msg.clone().or_else(|| h.title.clone()), "block": p.block_id }),
            })
        }
        // An agent answered what you sent it from the app.
        "turn" => {
            let docket = ev["docket"].as_str()?.to_string();
            let st = core.store.state(&docket).ok()??;
            // The agent that answered: the docket may have several.
            let block = ev["block"].as_str();
            let a = st.agents.iter().rev().find(|a| Some(a.agent.block.as_str()) == block).map(|a| &a.agent).or(st.agent.as_ref());
            let app = a.map(|a| crate::mirror::app_name(&a.app).to_string()).unwrap_or_else(|| "The agent".into());
            Some(Note {
                title: format!("{app} replied"),
                subtitle: format!("{docket} · {}", st.title),
                body: ev["text"].as_str().unwrap_or_default().to_string(),
                path: format!("{base}/#/d/{docket}"),
                group: format!("wh-{docket}"),
                kind: "turn",
                docket: Some(docket),
                message: None,
                key: None,
                sound: "task.complete",
                seen: Value::Null,
            })
        }
        "message" => {
            let m = core.store.message(ev["id"].as_i64()?).ok()??;
            if m.status != "open" {
                return None;
            }
            let from = crate::messages::view(&core, &m)["from_app"].as_str().map(str::to_string).unwrap_or(m.from_addr.clone());
            let title = if m.options.is_empty() { format!("{from} wrote to you") } else { format!("{from} asks you") };
            let sub = [m.docket.clone(), m.subject.clone()].into_iter().flatten().collect::<Vec<_>>().join(" · ");
            let url = m.docket.as_ref().map(|d| format!("{base}/#/d/{d}")).unwrap_or(format!("{base}/#/inbox"));
            Some(Note {
                title,
                subtitle: sub,
                body: m.body.lines().next().unwrap_or_default().to_string(),
                path: url,
                group: format!("wh-msg-{}", m.id),
                kind: "message",
                docket: m.docket.clone(),
                message: Some(m.id),
                key: None,
                sound: "input.required",
                seen: Value::Null,
            })
        }
        "build" => {
            let docket = ev["docket"].as_str()?.to_string();
            let a = core.build.attention.get(&docket)?;
            if a.kind != ev["kind"].as_str()? {
                return None;
            }
            let title = core.store.state(&docket).ok()??.title;
            Some(Note {
                title: format!(
                    "Build {}",
                    match a.kind.as_str() {
                        "blocked" => "is blocked",
                        "stalled" => "stalled",
                        "start-failed" => "didn't start",
                        "pane-closed" => "lost a pane",
                        _ => "needs you",
                    }
                ),
                subtitle: format!("{docket} · {title}"),
                body: a.text.clone(),
                path: format!("{base}/#/d/{docket}"),
                group: format!("wh-build-{docket}"),
                kind: "build",
                docket: Some(docket),
                message: None,
                key: None,
                sound: sound(&a.kind),
                seen: Value::Null,
            })
        }
        "review" => {
            let key = ev["key"].as_str()?;
            let a = core.review.attention.get(key)?;
            if a.kind != ev["kind"].as_str()? {
                return None;
            }
            let docket = ev["docket"].as_str()?.to_string();
            let title = core.store.state(&docket).ok()??.title;
            let short = key.rsplit('/').next().unwrap_or(key);
            let what = match a.kind.as_str() {
                "ready" => "is ready to post".to_string(),
                "blocked" => "is blocked".to_string(),
                "start-failed" => "didn't start".to_string(),
                "stalled" => "stalled".to_string(),
                "pane-closed" => "lost a pane".to_string(),
                _ => "needs you".to_string(),
            };
            Some(Note {
                title: format!("Review of {short} {what}"),
                subtitle: format!("{docket} · {title}"),
                body: a.text.clone(),
                path: format!("{base}/#/d/{docket}"),
                group: format!("wh-review-{key}"),
                kind: "review",
                docket: Some(docket),
                message: None,
                key: Some(key.to_string()),
                sound: sound(&a.kind),
                seen: crate::review::all(&core)
                    .ok()
                    .and_then(|prs| prs.into_iter().find(|p| p.key() == key))
                    .map(|p| serde_json::json!({ "digest": crate::review::watch::draft_summary(&p)["digest"] }))
                    .unwrap_or(Value::Null),
            })
        }
        _ => None,
    }
}

/// A build's or a review's attention, as its sound: ready to post is work
/// done, blocked waits on you, and the rest (stalled, didn't start, lost a
/// pane) went wrong.
fn sound(attention: &str) -> &'static str {
    match attention {
        "ready" => "task.complete",
        "blocked" => "input.required",
        _ => "task.error",
    }
}

pub fn rex_inbox_running() -> bool {
    let sock = crate::paths::home().join(".local/state/rex-inbox/hub.sock");
    std::os::unix::net::UnixStream::connect(sock).is_ok()
}

fn on_path(bin: &str) -> Option<std::path::PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?).map(|d| d.join(bin)).find(|p| p.is_file())
}

async fn post(title: &str, subtitle: &str, body: &str, url: &str, group: &str) {
    let r = match on_path("terminal-notifier") {
        Some(tn) => {
            tokio::process::Command::new(tn)
                .args(["-title", title, "-subtitle", subtitle, "-message", if body.is_empty() { " " } else { body }, "-open", url, "-group", group])
                .output()
                .await
        }
        None => {
            let q = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
            let script = format!("display notification \"{}\" with title \"{}\" subtitle \"{}\"", q(body), q(title), q(subtitle));
            tokio::process::Command::new("osascript").args(["-e", &script]).output().await
        }
    };
    if let Err(e) = r {
        say!("notify: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_app_gets_what_its_actions_need() {
        let n = Note {
            title: "Claude Code asks you".into(),
            subtitle: "D-1 · Ship it?".into(),
            body: "Tests pass.".into(),
            path: "/#/d/D-1".into(),
            group: "wh-msg-1".into(),
            kind: "message",
            docket: Some("D-1".into()),
            message: Some(1),
            key: None,
            sound: "input.required",
            seen: Value::Null,
        };
        let ev = n.event();
        assert_eq!(ev["type"], "notify");
        assert_eq!(ev["kind"], "message");
        assert_eq!(ev["group"], "wh-msg-1");
        assert_eq!(ev["docket"], "D-1");
        assert_eq!(ev["message"], 1);
        assert!(ev["key"].is_null());
        assert_eq!(ev["sound"], "input.required");
    }

    #[test]
    fn attention_sounds_as_done_waiting_or_wrong() {
        assert_eq!(sound("ready"), "task.complete");
        assert_eq!(sound("blocked"), "input.required");
        for k in ["stalled", "start-failed", "pane-closed", "anything-new"] {
            assert_eq!(sound(k), "task.error", "{k}");
        }
    }
}
