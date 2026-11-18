//! The Mac's notifications (Notifier.swift), posted with rune-notify.
//!
//! The hub decides what to notify and words it (src/notify.rs): a prompt or
//! a failure, a message to you, a finished turn from an agent you wrote to,
//! a review or a build that needs you. It waits two seconds and drops what
//! resolved meanwhile, holds back bursts (five in thirty seconds), and names
//! each by its group (`wh-D-6`, `wh-msg-3`), so a newer one about the same
//! thing replaces the older. While the app follows it as Wheelhouse.app
//! (`/api/events?client=mac`, which it does only while macOS lets it notify
//! and they're on), the hub posts none itself and sends each as a `notify`
//! event; the app posts it with the answers it allows: Approve or Deny a
//! prompt, an agent's options, a reply, Post a checked review. An answer acts
//! on what the banner showed (the prompt's record and time, the draft's
//! digest), so the hub refuses it if that changed. A click on the banner
//! shows the docket in the main window. One that no longer needs you,
//! answered here, on your phone or in Rex, is withdrawn.
//!
//! `WH_RUNE_NOTIFY=off` turns them off (Settings' "Notify me", for now).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};

use super::windows;
use crate::model::{self, HubState, LiveState};
use crate::store::Store;

/// A button on the banner, or a box to type a reply in.
#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    Button(String, String),
    Reply { placeholder: String },
}

/// A notification, as posted: its id (the hub's group), words, what a click
/// hands back, and its answers.
#[derive(Clone, Debug, PartialEq)]
pub struct Note {
    pub id: String,
    pub title: String,
    pub body: String,
    pub info: Vec<(String, String)>,
    pub acts: Vec<Act>,
}

/// What a posted one is about, to withdraw it once it isn't due.
#[derive(Clone, Debug, PartialEq)]
pub struct About {
    pub kind: String,
    pub docket: Option<String>,
    pub message: Option<i64>,
    pub key: Option<String>,
}

impl About {
    pub fn of(ev: &Value) -> About {
        About { kind: model::str_or(&ev["kind"], "").to_string(), docket: model::text(&ev["docket"]), message: model::int(&ev["message"]), key: model::text(&ev["key"]) }
    }
}

/// A `notify` event, as the banner shows it, with the answers it allows now.
pub fn note(ev: &Value, state: Option<&HubState>) -> Note {
    let about = About::of(ev);
    let group = model::text(&ev["group"]);
    let mut info = vec![("kind".to_string(), about.kind.clone()), ("group".to_string(), group.clone().unwrap_or_default())];
    if let Some(d) = &about.docket {
        info.push(("docket".into(), d.clone()));
    }
    if let Some(m) = about.message {
        info.push(("message".into(), m.to_string()));
    }
    if let Some(k) = &about.key {
        info.push(("key".into(), k.clone()));
    }
    // What the banner shows, so an answer from it is to exactly that.
    if let Some(seen) = ev["seen"].as_object() {
        for (k, v) in seen {
            if let Some(s) = v.as_str() {
                info.push((format!("seen_{k}"), s.to_string()));
            }
        }
    }
    let seen = |k: &str| ev["seen"][k].is_string();
    let mut acts = vec![];
    match about.kind.as_str() {
        "needs" => {
            let answerable = state.zip(about.docket.as_deref()).and_then(|(st, d)| st.docket(d)).and_then(|d| d.live()).is_some_and(|l| l.answerable());
            if answerable && seen("record") {
                acts.push(Act::Button("approve".into(), "Approve".into()));
                acts.push(Act::Button("deny".into(), "Deny".into()));
            }
        }
        "turn" => acts.push(Act::Reply { placeholder: "Message the agent…".into() }),
        "message" => {
            let m = state.zip(about.message).and_then(|(st, id)| st.messages.iter().find(|m| m.id() == id).cloned());
            // An agent's options, as the banner's buttons (macOS shows up to four).
            for (i, o) in m.map(|m| m.options()).unwrap_or_default().into_iter().take(4).enumerate() {
                acts.push(Act::Button(format!("opt-{i}"), o));
            }
            acts.push(Act::Reply { placeholder: "Your answer…".into() });
        }
        "review" => {
            let ready = state.zip(about.key.as_deref()).and_then(|(st, k)| st.prs.iter().find(|p| p.full_key() == k || p.key() == k)).is_some_and(|p| p.ready());
            if ready && seen("digest") {
                acts.push(Act::Button("post".into(), "Post Review".into()));
                acts.push(Act::Button("open".into(), "Preview".into()));
            }
        }
        _ => {}
    }
    // No subtitle in rune-notify: it leads the body.
    let body = [model::text(&ev["subtitle"]), model::text(&ev["body"])].into_iter().flatten().filter(|s| !s.trim().is_empty()).collect::<Vec<_>>().join("\n");
    Note {
        id: group.unwrap_or_else(|| format!("wh-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0))),
        title: model::text(&ev["title"]).unwrap_or_else(|| "Wheelhouse".into()),
        body,
        info,
        acts,
    }
}

/// Whether what it's about still needs you (Notifier.reconcile).
pub fn due(a: &About, state: &HubState) -> bool {
    let live = a.docket.as_deref().and_then(|d| state.docket(d)).and_then(|d| d.live()).map(|l| l.state());
    match a.kind.as_str() {
        "needs" => matches!(live, Some(LiveState::Blocked | LiveState::Error)),
        // A finished turn, until the agent's at work again or waits on you.
        "turn" => !matches!(live, Some(LiveState::Working | LiveState::Starting | LiveState::Blocked)),
        "message" => a.message.is_some_and(|id| state.messages.iter().any(|m| m.id() == id)),
        "review" => a.key.as_deref().is_some_and(|k| state.prs.iter().any(|p| (p.full_key() == k || p.key() == k) && p.attention().is_some())),
        "build" => a.docket.as_deref().and_then(|d| state.task_for(d)).is_some_and(|t| t.attention().is_some()),
        _ => true,
    }
}

/// What this run posted and hasn't withdrawn, by id.
static POSTED: Mutex<Option<HashMap<String, About>>> = Mutex::new(None);

fn enabled() -> bool {
    !std::env::var("WH_RUNE_NOTIFY").is_ok_and(|v| v == "off" || v == "0")
}

/// Post the hub's notifications from now on: in a window, from a bundled
/// app (macOS notifies only for one), while they're on. Clicks and answers
/// go to the hub and the main window.
pub fn start(store: Arc<Store>) {
    if store.headless || !enabled() || !rune_notify::available() {
        return;
    }
    {
        let store = store.clone();
        rune_notify::on_click(move |c| {
            let store = store.clone();
            std::thread::spawn(move || respond(&c, &store));
        });
    }
    {
        let s = store.clone();
        store.on_event(move |ev| {
            if ev["type"] == "notify" {
                post(ev, &s);
            }
        });
    }
    {
        let s = store.clone();
        store.subscribe(move || {
            if let Some(st) = s.state() {
                reconcile(&st);
            }
        });
    }
    // Ask at launch (Notifier.setUp), then look again every 30 s: System
    // Settings may turn them on or off (Store.checkNotifying).
    let s = store.clone();
    auth::request(move |ok| s.set_notifies(ok));
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_secs(30));
            let s = store.clone();
            auth::allowed(move |ok| s.set_notifies(ok));
        }
    });
}

/// A `notify` event: post it, while the hub knows the app posts them.
fn post(ev: &Value, store: &Store) {
    if !store.notifying() {
        return;
    }
    let n = note(ev, store.state().as_deref());
    let mut x = rune_notify::Notification::new(&n.id, &n.title).body(&n.body);
    for (k, v) in &n.info {
        x = x.info(k, v);
    }
    for a in &n.acts {
        x = match a {
            Act::Button(id, label) => x.action(id, label),
            Act::Reply { placeholder } => x.reply("reply", placeholder, "Reply"),
        };
    }
    if rune_notify::post(&x)
        && let Ok(mut p) = POSTED.lock()
    {
        p.get_or_insert_with(HashMap::new).insert(n.id, About::of(ev));
    }
}

/// Withdraw what no longer needs you.
pub fn reconcile(state: &HubState) {
    let stale: Vec<String> = match POSTED.lock() {
        Ok(mut p) => {
            let p = p.get_or_insert_with(HashMap::new);
            let stale: Vec<String> = p.iter().filter(|(_, a)| !due(a, state)).map(|(id, _)| id.clone()).collect();
            for id in &stale {
                p.remove(id);
            }
            stale
        }
        Err(_) => return,
    };
    if !stale.is_empty() {
        rune_notify::remove(&stale.iter().map(String::as_str).collect::<Vec<_>>());
    }
}

/// A click on a banner, or one of its answers (Notifier.respond), off the UI
/// thread: the state as it is now, then the hub.
fn respond(c: &rune_notify::Click, store: &Store) {
    let docket = c.get("docket").map(str::to_string);
    let message = c.get("message").and_then(|m| m.parse::<i64>().ok());
    let seen = |k: &str| c.get(&format!("seen_{k}")).map(str::to_string);
    store.load_now();
    let state = store.state();
    let msg = |id: i64| state.as_ref().and_then(|st| st.messages.iter().find(|m| m.id() == id).cloned());
    let say = |r: Result<(String, bool), String>| match r {
        Ok((t, e)) => windows::toast_in_main(t, e),
        Err(e) => windows::toast_in_main(e, true),
    };
    match c.action() {
        Some(a @ ("approve" | "deny")) => {
            // The prompt the banner showed, not whatever waits now: the hub
            // refuses if they differ.
            let (Some(d), Some(record)) = (docket, seen("record")) else { return };
            let approve = a == "approve";
            let shown = seen("msg");
            let body = json!({ "record": record, "updated_at": seen("updated_at"), "msg": shown });
            say(store.post(&format!("/api/dockets/{d}/{a}"), body).map(|v| {
                if model::flag(&v["cleared"]) {
                    (format!("{} {d}{}", if approve { "Approved" } else { "Denied" }, shown.map(|m| format!(": {m}")).unwrap_or_default()), false)
                } else {
                    ("Sent, but the prompt is still showing. Look at the pane.".into(), true)
                }
            }));
        }
        Some("reply") => {
            let Some(t) = c.input("reply").map(str::trim).filter(|t| !t.is_empty()) else { return };
            if let Some(id) = message {
                let Some(m) = msg(id) else {
                    windows::toast_in_main(format!("Message #{id} was already answered; your reply wasn't sent"), true);
                    windows::show_main();
                    return;
                };
                say(store.post(&format!("/api/messages/{id}/reply"), json!({ "body": t })).map(|v| (format!("Replied · it goes to {} between turns", model::text(&v["to"]).unwrap_or(m.who())), false)));
            } else if let Some(d) = docket {
                say(store.post(&format!("/api/dockets/{d}/send"), json!({ "text": t, "queue": true })).map(|v| {
                    (if v["queued"].is_number() { "Queued: it goes in when the turn ends".to_string() } else { "Sent to the pane".to_string() }, false)
                }));
            }
        }
        Some(a) if a.starts_with("opt-") => {
            let (Some(id), Ok(i)) = (message, a[4..].parse::<usize>()) else { return };
            let Some(choice) = msg(id).and_then(|m| m.options().get(i).cloned()) else { return };
            say(store.post(&format!("/api/messages/{id}/reply"), json!({ "body": "", "choice": choice })).map(|v| {
                (format!("Chose {choice} · it goes to {} between turns", model::text(&v["to"]).unwrap_or_else(|| "the agent".into())), false)
            }));
        }
        Some("post") => {
            // The draft whose summary the banner showed; the hub refuses a changed one.
            let key = c.get("key").unwrap_or_default();
            let p = state.as_ref().and_then(|st| st.prs.iter().find(|p| p.full_key() == key || p.key() == key).cloned());
            let (Some(p), Some(digest)) = (p, seen("digest")) else { return };
            say(store.post("/api/review/post", json!({ "key": p.full_key(), "digest": digest })).map(|_| (format!("Posted the review of {}", p.key()), false)));
        }
        // A click, or Preview: the docket, in the main window.
        _ => windows::show_in_main(docket.as_deref().filter(|d| state.as_ref().is_some_and(|st| st.docket(d).is_some()))),
    }
}

/// Whether macOS lets the app notify (UNUserNotificationCenter's settings),
/// which rune-notify doesn't say.
#[cfg(target_os = "macos")]
mod auth {
    use block2::RcBlock;
    use objc2::runtime::{AnyObject, Bool};
    use objc2::{class, msg_send};

    // UNAuthorizationOptionAlert: banners, as rune-notify asks.
    const ALERT: usize = 1 << 2;

    unsafe fn center() -> *mut AnyObject {
        unsafe { msg_send![class!(UNUserNotificationCenter), currentNotificationCenter] }
    }

    /// Ask, the first time; `done` hears whether they're allowed.
    pub fn request(done: impl Fn(bool) + Send + 'static) {
        let block = RcBlock::new(move |granted: Bool, _: *mut AnyObject| done(granted.as_bool()));
        // SAFETY: a bundled app's notification center (the caller checked).
        unsafe {
            let _: () = msg_send![center(), requestAuthorizationWithOptions: ALERT, completionHandler: &*block];
        }
    }

    /// Allowed now: authorized, provisional or ephemeral.
    pub fn allowed(done: impl Fn(bool) + Send + 'static) {
        let block = RcBlock::new(move |settings: *mut AnyObject| {
            // SAFETY: the UNNotificationSettings the center hands back.
            let status: isize = if settings.is_null() { 0 } else { unsafe { msg_send![settings, authorizationStatus] } };
            done(matches!(status, 2..=4));
        });
        // SAFETY: as above.
        unsafe {
            let _: () = msg_send![center(), getNotificationSettingsWithCompletionHandler: &*block];
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod auth {
    pub fn request(done: impl Fn(bool) + Send + 'static) {
        done(rune_notify::available());
    }
    pub fn allowed(done: impl Fn(bool) + Send + 'static) {
        done(rune_notify::available());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> HubState {
        HubState::parse(json!({
            "dockets": [
                { "id": "D-1", "title": "Fix the checkout tests", "status": "in_progress",
                  "live": { "state": "blocked", "answerable": true, "record": "r1", "msg": "Bash: pnpm test" } },
                { "id": "D-2", "title": "Idle one", "status": "in_progress", "live": { "state": "idle" } },
                { "id": "D-3", "title": "Working one", "status": "in_progress", "live": { "state": "working" } }
            ],
            "messages": [ { "id": 3, "docket": "D-5", "from": "codex", "options": ["Tonight at 2am", "Now"], "body": "Reindex now or tonight?" } ],
            "review": { "prs": [ { "key": "acme/api#9", "full_key": "acme/api#9", "state": "reviewing", "phase": "awaiting", "attention": { "kind": "ready", "text": "", "round": 1 },
                                   "draft": { "digest": "abc" } } ] }
        }))
    }

    #[test]
    fn a_prompt_gets_approve_and_deny_with_what_it_showed() {
        let ev = json!({ "type": "notify", "kind": "needs", "title": "Claude Code waits on you (Bash)", "subtitle": "D-1 · Fix the checkout tests",
                         "body": "Bash: pnpm test", "group": "wh-D-1", "docket": "D-1", "seen": { "record": "r1", "updated_at": "t", "msg": "Bash: pnpm test" } });
        let n = note(&ev, Some(&state()));
        assert_eq!(n.id, "wh-D-1");
        assert_eq!(n.title, "Claude Code waits on you (Bash)");
        assert_eq!(n.body, "D-1 · Fix the checkout tests\nBash: pnpm test");
        assert_eq!(n.acts, vec![Act::Button("approve".into(), "Approve".into()), Act::Button("deny".into(), "Deny".into())]);
        assert!(n.info.contains(&("seen_record".into(), "r1".into())));
        assert!(n.info.contains(&("docket".into(), "D-1".into())));
        // Without the record it showed, no answer from the banner.
        let bare = json!({ "type": "notify", "kind": "needs", "group": "wh-D-1", "docket": "D-1" });
        assert!(note(&bare, Some(&state())).acts.is_empty());
    }

    #[test]
    fn a_message_gets_its_options_and_a_reply() {
        let ev = json!({ "type": "notify", "kind": "message", "title": "Codex asks you", "group": "wh-msg-3", "docket": "D-5", "message": 3 });
        let n = note(&ev, Some(&state()));
        assert_eq!(
            n.acts,
            vec![Act::Button("opt-0".into(), "Tonight at 2am".into()), Act::Button("opt-1".into(), "Now".into()), Act::Reply { placeholder: "Your answer…".into() }]
        );
        assert!(n.info.contains(&("message".into(), "3".into())));
    }

    #[test]
    fn a_ready_review_can_be_posted_from_the_banner() {
        let ev = json!({ "type": "notify", "kind": "review", "group": "wh-review-acme/api#9", "docket": "D-7", "key": "acme/api#9", "seen": { "digest": "abc" } });
        assert_eq!(note(&ev, Some(&state())).acts.len(), 2);
        let turn = json!({ "type": "notify", "kind": "turn", "group": "wh-D-2", "docket": "D-2" });
        assert_eq!(note(&turn, Some(&state())).acts, vec![Act::Reply { placeholder: "Message the agent…".into() }]);
        let build = json!({ "type": "notify", "kind": "build", "group": "wh-build-D-2", "docket": "D-2" });
        assert!(note(&build, Some(&state())).acts.is_empty());
    }

    #[test]
    fn what_was_answered_is_withdrawn() {
        let st = state();
        let about = |kind: &str, docket: Option<&str>, message: Option<i64>, key: Option<&str>| About {
            kind: kind.into(),
            docket: docket.map(Into::into),
            message,
            key: key.map(Into::into),
        };
        assert!(due(&about("needs", Some("D-1"), None, None), &st));
        assert!(!due(&about("needs", Some("D-2"), None, None), &st));
        assert!(due(&about("turn", Some("D-2"), None, None), &st));
        assert!(!due(&about("turn", Some("D-3"), None, None), &st));
        assert!(due(&about("message", None, Some(3), None), &st));
        assert!(!due(&about("message", None, Some(4), None), &st));
        assert!(due(&about("review", Some("D-7"), None, Some("acme/api#9")), &st));
        assert!(!due(&about("review", Some("D-7"), None, Some("acme/api#10")), &st));
        assert!(!due(&about("build", Some("D-2"), None, None), &st));
        assert!(due(&about("other", None, None, None), &st));
    }
}
