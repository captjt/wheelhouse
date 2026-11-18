//! Chat (Chat.swift): talking to a docket's agent, as a conversation. What
//! you asked, what it answered, what it's doing now, and a box to send it
//! the next thing; while it works, what you send waits and goes in when its
//! turn ends. And the pane itself, in a sheet (PaneSheet).

use std::collections::{HashMap, HashSet};

use rune::{Color, El, LoopSpec, ScrollSpec, TextEdit, Tx, col, el, icon, row, spacer, text, text_input};
use rune_kit::{Kit, ScrollBar};
use serde_json::json;

use super::parts::{self, Req};
use super::{Main, Sheet, docket, glyphs, lists, widgets};
use crate::icons;
use crate::model::{self, Docket, DocketPage, Entry, HubState, LiveState};
use crate::theme::{self, K, font, mono};

/// What the conversation keeps between frames.
pub struct Talk {
    pub text: TextEdit,
    pub busy: bool,
    /// What you sent, by docket, until it shows in the record or the queue.
    pub sent: Vec<(String, String)>,
    /// The docket whose pane the Now line peeks at, and its last lines.
    pub peek: Option<String>,
    pub tail: String,
    pub peeked_at: f64,
    /// The pane sheet's screen.
    pub screen: String,
    pub screen_error: Option<String>,
    pub screen_at: f64,
    pub pane_text: TextEdit,
    /// Long replies shown in full, and each reply's height.
    pub unclamped: HashSet<i64>,
    pub heights: HashMap<i64, f32>,
    pub second: i64,
}

impl Default for Talk {
    fn default() -> Talk {
        Talk {
            text: TextEdit::new(""),
            busy: false,
            sent: vec![],
            peek: None,
            tail: String::new(),
            peeked_at: f64::NEG_INFINITY,
            screen: String::new(),
            screen_error: None,
            screen_at: f64::NEG_INFINITY,
            pane_text: TextEdit::new(""),
            unclamped: HashSet::new(),
            heights: HashMap::new(),
            second: 0,
        }
    }
}

enum Item {
    You { at: String, text: String, note: bool },
    Agent { seq: i64, at: String, text: String, app: String, ask: Option<Vec<String>>, subject: Option<String> },
    Chose(String),
    Sys(String, Option<LiveState>),
    File(Entry),
    Pause(String),
}

/// The record, read as a conversation (ChatView.items).
fn items(d: &Docket, p: &DocketPage) -> Vec<Item> {
    let mut out = vec![];
    let mut session: Option<String> = None;
    let fallback = d.agent_app().unwrap_or_else(|| "agent".into());
    for e in &p.entries {
        let x = e.data();
        let app = if e.actor_type() == "agent" { e.actor_app() } else { fallback.clone() };
        let body = e.body().unwrap_or_default();
        match e.kind().as_str() {
            "claim" => {
                let s = model::str_or(&x["session"], "").to_string();
                if !s.is_empty() && Some(&s) != session.as_ref() {
                    if session.is_some() {
                        out.push(Item::Pause(match x["source"].as_str() {
                            Some("resume") => "Resumed an earlier session".into(),
                            Some("clear") => "A new session (/clear)".into(),
                            _ => "A new session".into(),
                        }));
                    }
                    session = Some(s);
                }
            }
            "note" => {
                let message = !x["message"].is_null();
                if x["via"].as_str() == Some("prompt") {
                    out.push(Item::You { at: e.at(), text: body, note: false });
                } else if message && e.actor_type() == "agent" {
                    let text = format!("{}{body}", model::text(&x["subject"]).map(|s| format!("**{s}**\n\n")).unwrap_or_default());
                    out.push(Item::Agent { seq: e.seq(), at: e.at(), text, app, ask: None, subject: None });
                } else if e.actor_type() == "person" && !message {
                    out.push(Item::You { at: e.at(), text: body, note: true });
                }
            }
            "question" => out.push(Item::Agent { seq: e.seq(), at: e.at(), text: body, app, ask: Some(model::strings(&x["options"])), subject: model::text(&x["subject"]) }),
            "decision" => {
                let c = model::str_or(&x["choice"], "").to_string();
                out.push(Item::Chose(match e.body() {
                    Some(b) if b != c => format!("{c}: {b}"),
                    _ => c,
                }));
            }
            "turn" => {
                if !body.trim().is_empty() {
                    out.push(Item::Agent { seq: e.seq(), at: e.at(), text: body, app: fallback.clone(), ask: None, subject: None });
                }
            }
            "observed" => {
                let colon = e.body().map(|b| format!(": {b}")).unwrap_or_default();
                match x["what"].as_str() {
                    Some("blocked") => {
                        out.push(Item::Sys(format!("Waited on you{}{colon}", model::text(&x["kind"]).map(|k| format!(" ({k})")).unwrap_or_default()), Some(LiveState::Blocked)))
                    }
                    Some("error") => out.push(Item::Sys(format!("Failed{colon}"), Some(LiveState::Error))),
                    _ => {}
                }
            }
            "approval" => {
                if (!x["what"].is_null() || !x["kind"].is_null()) && x["kind"].as_str() != Some("review") {
                    out.push(Item::Sys(
                        format!(
                            "You {} {}{}",
                            if x["decision"].as_str() == Some("approve") { "approved" } else { "denied" },
                            model::str_or(&x["kind"], "a prompt"),
                            e.body().map(|b| format!(": {b}")).unwrap_or_default()
                        ),
                        None,
                    ));
                }
            }
            "artifact" => out.push(Item::File(e.clone())),
            "release" => out.push(Item::Pause(format!("The session ended: {}", model::str_or(&x["reason"], "released")))),
            _ => {}
        }
    }
    out
}

/// The chat for a docket whose agent talks.
pub fn view(s: &Main, st: &HubState, d: &Docket, k: &K, kit: &Kit) -> El {
    let id = d.id();
    let Some(page) = s.pages.get(&id) else {
        let why = s.page_errors.get(&id).cloned().unwrap_or_else(|| "Loading the conversation…".into());
        return col().key("chat").w_full().h_full().center().child(text(why).font(font(12.5, 400.0).lh(16.0)).color(k.muted));
    };
    let items = items(d, page);
    let queued = page.queued();
    let said = |t: &str| items.iter().any(|i| matches!(i, Item::You { text, .. } if text.trim() == t)) || queued.iter().any(|(_, b)| b.trim() == t);
    let pending: Vec<&String> = s.talk.sent.iter().filter(|(on, t)| *on == id && !said(t)).map(|(_, t)| t).collect();
    let state = d.live().map(|l| l.state());
    let can_send = d.live().is_some() && state != Some(LiveState::Error);
    let will_queue = matches!(state, Some(LiveState::Working | LiveState::Starting | LiveState::Blocked));
    let app = model::app_name(d.agent_app().as_deref());

    let mut c = col().key("chat-items").w_full().max_w(820.0).px(28.0).py(20.0).gap(14.0);
    if items.is_empty() {
        c = c.child(
            row().w_full().pt(40.0).justify_center().child(text(format!("Nothing said yet. Send {app} something to start.")).font(font(13.0, 400.0).lh(17.0)).color(k.muted)),
        );
    }
    for it in &items {
        c = c.child(bubble(s, d, it, k, kit));
    }
    for m in st.messages.iter().filter(|m| m.docket().as_deref() == Some(id.as_str())) {
        c = c.child(docket::message_card(s, m, k, kit));
    }
    for (q, body) in &queued {
        let (a, q) = (id.clone(), *q);
        c = c.child(col().w_full().items_end().gap(4.0).child(you_bubble(body, false, k, kit).opacity(0.6)).child(
            row().gap(4.0).center_y().child(text("Queued · goes in when the turn ends ·").font(font(11.5, 400.0).lh(15.0)).color(k.muted)).child(parts::link(
                format!("unqueue-{q}"),
                "Take back",
                11.5,
                k,
                move |s, _| unqueue(s, &a, q),
            )),
        ));
    }
    for t in pending {
        c = c.child(row().w_full().justify_end().child(you_bubble(t, false, k, kit).opacity(0.6)));
    }
    c = c.child(now_line(s, d, k));

    let list = col()
        .key("chat-scroll")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .items_center()
        // Short, it sits at the bottom, by the box (`.defaultScrollAnchor(.bottom)`).
        .child(spacer())
        .child(c)
        .scroll(ScrollSpec { stick_bottom: true })
        .scroll_bar(kit);
    col().key("chat").w_full().h_full().min_h(0.0).child(list).child(parts::divider(k)).child(composer(s, d, can_send, will_queue, &app, k))
}

fn bubble(s: &Main, d: &Docket, it: &Item, k: &K, kit: &Kit) -> El {
    match it {
        Item::You { at, text: t, note } => {
            let mut c = col().w_full().items_end().gap(4.0);
            if *note {
                c = c.child(text("Your note on the record (not sent)").font(font(11.0, 400.0).lh(14.0)).color(k.muted));
            }
            c.child(you_bubble(t, *note, k, kit).help(model::stamp(Some(at))))
        }
        Item::Agent { seq, at, text: t, app, ask, subject } => {
            let mut head =
                row().gap(7.0).center_y().child(theme::app_avatar(Some(app), 18.0, k)).child(text(model::app_name(Some(app))).font(font(12.5, 600.0).lh(16.0)).color(k.text));
            if ask.is_some() {
                head = head.child(text(format!("asked you{}", subject.as_ref().map(|s| format!(" · {s}")).unwrap_or_default())).font(font(12.5, 400.0).lh(16.0)).color(k.muted));
            }
            head = head.child(text(model::ago_long(Some(at))).font(font(12.5, 400.0).lh(16.0)).color(k.faint).help(model::stamp(Some(at))));
            let src = match ask {
                Some(o) if !o.is_empty() => format!("{t}\n\nOptions: {}", o.join(" · ")),
                _ => t.clone(),
            };
            col().w_full().gap(8.0).child(head).child(clamp(s, *seq, parts::md(&src, 13.0, k, kit), k))
        }
        Item::Chose(t) => row()
            .w_full()
            .justify_end()
            .child(row().px(12.0).py(7.0).radius(999.0).fill(k.accent_soft).child(text(format!("Chose “{t}”")).font(font(12.5, 400.0).lh(16.0)).color(k.text))),
        Item::Sys(t, tone) => row().w_full().justify_center().child(
            text(t.clone())
                .font(font(12.0, 400.0).lh(16.0))
                .color(match tone {
                    Some(LiveState::Error) => k.error,
                    Some(LiveState::Blocked) => k.blocked,
                    _ => k.muted,
                })
                .wrap()
                .align(rune::Align::Center),
        ),
        Item::File(e) => {
            let link = model::text(&e.data()["url"]);
            let name = e.body().or_else(|| model::text(&e.data()["name"])).unwrap_or_else(|| "File".into());
            let what = if link.is_some() { "· a link".to_string() } else { format!("· {}", model::str_or(&e.data()["name"], "")) };
            let e2 = e.clone();
            let _ = d;
            row().w_full().child(
                row()
                    .key(format!("chat-file-{}", e.seq()))
                    .gap(8.0)
                    .px(12.0)
                    .py(8.0)
                    .radius(9.0)
                    .fill(k.elev)
                    .border(1.0, k.line)
                    .hover(k.hover)
                    .center_y()
                    .role("button")
                    .label(name.clone())
                    .on_press_tx(move |s: &mut Main, tx: &mut Tx| open_file(s, &e2, tx))
                    .child(icon(if link.is_some() { &icons::LINK } else { &icons::DOC }, 14.0).color(k.text))
                    .child(text(name).font(font(12.5, 500.0).lh(16.0)).color(k.text))
                    .child(text(what).font(font(12.5, 400.0).lh(16.0)).color(k.muted)),
            )
        }
        Item::Pause(t) => row()
            .w_full()
            .gap(10.0)
            .center_y()
            .child(el().h(1.0).grow(1.0).fill(k.line))
            .child(text(t.clone()).font(font(11.5, 400.0).lh(15.0)).color(k.muted))
            .child(el().h(1.0).grow(1.0).fill(k.line)),
    }
}

/// What you said: on the right, in the accent's tint; a note on the
/// record (not sent) dashed and plain.
fn you_bubble(t: &str, note: bool, k: &K, kit: &Kit) -> El {
    let mut b =
        col().max_w(560.0).min_w(0.0).px(13.0).py(9.0).radius(14.0).continuous_corners().fill(if note { k.recessed } else { k.accent_soft }).child(parts::md(t, 13.0, k, kit));
    if note {
        b = b.border(1.0, k.line).border_dash(4.0, 3.0);
    }
    b
}

/// Long text folds, with Show more (Clamp): past 560 pt, shown to 520.
fn clamp(s: &Main, seq: i64, content: El, k: &K) -> El {
    const LIMIT: f32 = 520.0;
    let full = s.talk.heights.get(&seq).copied().unwrap_or(0.0);
    let long = full > LIMIT + 40.0;
    let open = s.talk.unclamped.contains(&seq);
    let inner = col().w_full().child(content).on_layout_tx(move |s: &mut Main, r, tx: &mut Tx| {
        let h = r[3];
        if (s.talk.heights.get(&seq).copied().unwrap_or(-10.0) - h).abs() < 1.0 {
            tx.live_only();
        } else {
            s.talk.heights.insert(seq, h);
        }
    });
    let mut frame = col().w_full().child(inner);
    if long && !open {
        frame = frame.max_h(LIMIT).clip_fade(0.0, 0.0, 0.0, LIMIT * 0.2);
    }
    let mut c = col().w_full().gap(6.0).child(frame);
    if long {
        c = c.child(parts::link(format!("clamp-{seq}"), if open { "Show less" } else { "Show more" }, 12.0, k, move |s, _| {
            if !s.talk.unclamped.remove(&seq) {
                s.talk.unclamped.insert(seq);
            }
        }));
    }
    c
}

/// What the agent is doing now, at the end of the conversation (NowLine).
fn now_line(s: &Main, d: &Docket, k: &K) -> El {
    let app = model::app_name(d.agent_app().as_deref());
    let id = d.id();
    let Some(l) = d.live() else {
        let why = match model::text(&d.j["released"]) {
            Some(r) => format!("The session ended ({r})."),
            None => format!("{app} isn't running in Rex."),
        };
        return text(format!("{why} Start it again on the Mac to keep going.")).font(font(12.5, 400.0).lh(16.0)).color(k.muted).wrap();
    };
    match l.state() {
        LiveState::Blocked => {
            let mut body = col()
                .gap(4.0)
                .grow(1.0)
                .min_w(0.0)
                .child(text(format!("{app} is asking{}", l.kind().map(|k| format!(" ({k})")).unwrap_or_default())).font(font(13.0, 600.0).lh(16.0)).color(k.text));
            if let Some(m) = l.msg() {
                body = body.child(text(m).font(mono(12.5).lh(16.0)).color(k.text2).wrap());
            }
            let a = id.clone();
            let r = row().w_full().gap(8.0).child(body).child(lists::answer_buttons(s, d, k)).child(el().pt(3.0).child(widgets::button(
                "now-terminal",
                "Terminal",
                false,
                true,
                k,
                move |s: &mut Main, _| open_pane(s, &a),
            )));
            theme::callout(LiveState::Blocked, r, k)
        }
        LiveState::Error => {
            let a = id.clone();
            row()
                .w_full()
                .gap(8.0)
                .center_y()
                .child(theme::live_icon(Some(LiveState::Error), 15.0, k))
                .child(
                    text(format!("{app} stopped with an error{}", l.msg().map(|m| format!(": {m}")).unwrap_or_else(|| ".".into())))
                        .font(font(12.5, 400.0).lh(16.0))
                        .color(k.error)
                        .wrap()
                        .shrink(),
                )
                .child(parts::link("now-look", "Look at the terminal", 12.5, k, move |s, _| open_pane(s, &a)))
        }
        LiveState::Working | LiveState::Starting => {
            let peeking = s.talk.peek.as_deref() == Some(id.as_str());
            let a = id.clone();
            let mut c = col().w_full().gap(8.0).child(
                row()
                    .w_full()
                    .gap(8.0)
                    .center_y()
                    .child(dots(k))
                    .child(
                        text(format!("{app} is {}{}", if l.state() == LiveState::Starting { "starting" } else { "working" }, elapsed(l.since().as_deref())))
                            .font(font(12.5, 400.0).lh(16.0))
                            .color(k.muted),
                    )
                    .child(spacer())
                    .child(parts::link("now-peek", if peeking { "Hide" } else { "Peek" }, 12.5, k, move |s, _| toggle_peek(s, &a))),
            );
            if peeking {
                c = c.child(
                    text(if s.talk.tail.is_empty() { "…".to_string() } else { s.talk.tail.clone() })
                        .font(mono(11.0).lh(14.0))
                        .color(k.text2)
                        .selectable(true)
                        .w_full()
                        .padding(10.0)
                        .radius(8.0)
                        .fill(k.recessed),
                );
            }
            c
        }
        _ => el(),
    }
}

/// Three dots, breathing: an agent at work.
fn dots(k: &K) -> El {
    let mut r = row().gap(3.0).center_y().a11y_hidden();
    for i in 0..3 {
        r = r.child(el().size(5.0, 5.0).radius(2.5).fill(k.working).looping(LoopSpec::opacity(1.4, i as f64 * 0.2, 0.25, 1.0)));
    }
    r
}

/// How long it's been at it: " · 42s", " · 3m", " · 1h 5m".
fn elapsed(since: Option<&str>) -> String {
    let Some(d) = model::date(since) else { return String::new() };
    let s = (chrono::Utc::now() - d).num_seconds().max(0);
    format!(
        " · {}",
        if s < 60 {
            format!("{s}s")
        } else if s < 3600 {
            format!("{}m", s / 60)
        } else {
            format!("{}h {}m", s / 3600, (s % 3600) / 60)
        }
    )
}

/// The box to the agent: Return sends, Shift-Return makes a new line.
fn composer(s: &Main, d: &Docket, can_send: bool, will_queue: bool, app: &str, k: &K) -> El {
    let id = d.id();
    let placeholder = if !can_send {
        if d.live().is_some() { format!("{app} stopped with an error") } else { format!("{app} isn't running") }
    } else if will_queue {
        format!("Queue a message for {app}…")
    } else {
        format!("Message {app}…")
    };
    let a = id.clone();
    let field = text_input(&s.talk.text, font(13.0, 400.0).lh(18.0))
        .key("chat-composer")
        .w_full()
        .min_h(20.0)
        .max_h(180.0)
        .color(k.text)
        .placeholder(placeholder, k.muted)
        .label(format!("Message {app}"))
        .disabled(!can_send)
        .on_edit(|s: &mut Main| &mut s.talk.text)
        .on_submit(move |s: &mut Main, _| send(s, &a));
    let ready = can_send && !s.talk.busy && !s.talk.text.text.trim().is_empty();
    let b = id.clone();
    let help = if will_queue { format!("Queue it: it goes in when {app}'s turn ends") } else { format!("Send to {app} (↩; ⇧↩ for a new line)") };
    let button = row()
        .key("chat-send")
        .size(30.0, 30.0)
        .radius(15.0)
        .center()
        .fill(if ready { k.accent } else { k.line2 })
        .role("button")
        .label(if will_queue { "Queue" } else { "Send" })
        .help(help)
        .disabled(!ready)
        .on_press(move |s: &mut Main| send(s, &b))
        .child(icon(if will_queue { &glyphs::CLOCK_ARROW } else { &glyphs::ARROW_UP }, 14.0).color(Color::WHITE));
    let bar = row()
        .w_full()
        .max_w(860.0)
        .px(20.0)
        .py(12.0)
        .gap(10.0)
        .items_end()
        .child(row().grow(1.0).min_w(0.0).px(10.0).py(8.0).radius(10.0).fill(k.elev).border(1.0, k.line).opacity(if can_send { 1.0 } else { 0.6 }).child(field))
        .child(button);
    row().key("chat-bar").w_full().justify_center().fill(k.main).child(bar)
}

pub fn send(s: &mut Main, id: &str) {
    let t = s.talk.text.text.trim().to_string();
    let live = s.state().and_then(|st| st.docket(id).and_then(|d| d.live()));
    if t.is_empty() || s.talk.busy || live.is_none() {
        return;
    }
    s.talk.busy = true;
    let id = id.to_string();
    parts::call(s, Req::Post(format!("/api/dockets/{id}/send"), json!({ "text": t, "queue": true })), move |s, r| {
        s.talk.busy = false;
        match r {
            Ok(v) => {
                s.talk.text.clear();
                s.talk.sent.push((id.clone(), t));
                if v["queued"].is_number() {
                    s.toast("Queued: it goes in when the turn ends", false);
                }
            }
            Err(e) => s.toast(e, true),
        }
        s.load_page(&id);
    });
}

fn unqueue(s: &mut Main, id: &str, q: i64) {
    let id = id.to_string();
    parts::call(s, Req::Post(format!("/api/dockets/{id}/queue/{q}/cancel"), json!({})), move |s, r| {
        match r {
            Ok(_) => s.toast("Taken back", false),
            Err(e) => s.toast(e, true),
        }
        s.load_page(&id);
    });
}

fn toggle_peek(s: &mut Main, id: &str) {
    if s.talk.peek.as_deref() == Some(id) {
        s.talk.peek = None;
        return;
    }
    s.talk.peek = Some(id.to_string());
    s.talk.tail.clear();
    s.talk.peeked_at = f64::NEG_INFINITY;
    peek(s);
}

/// The pane's last 14 lines, for the Now line.
pub(super) fn peek(s: &mut Main) {
    let Some(id) = s.talk.peek.clone() else { return };
    s.talk.peeked_at = s.clock;
    parts::call(s, Req::Get(format!("/api/dockets/{id}/screen")), move |s, r| {
        if let (Ok(v), true) = (r, s.talk.peek.as_deref() == Some(id.as_str())) {
            let all = model::str_or(&v["text"], "").trim_end().to_string();
            let lines: Vec<&str> = all.split('\n').collect();
            s.talk.tail = lines[lines.len().saturating_sub(14)..].join("\n");
        }
    });
}

pub fn open_pane(s: &mut Main, id: &str) {
    s.talk.screen.clear();
    s.talk.screen_error = None;
    s.talk.pane_text.clear();
    s.sheet = Some(Sheet::Pane(id.to_string()));
    load_screen(s);
}

fn load_screen(s: &mut Main) {
    let Some(Sheet::Pane(id)) = s.sheet.clone() else { return };
    s.talk.screen_at = s.clock;
    parts::call(s, Req::Get(format!("/api/dockets/{id}/screen")), move |s, r| {
        if s.sheet != Some(Sheet::Pane(id.clone())) {
            return;
        }
        match r {
            Ok(v) => {
                s.talk.screen = model::str_or(&v["text"], "").to_string();
                s.talk.screen_error = None;
            }
            Err(e) => s.talk.screen_error = Some(e),
        }
    });
}

/// Before each frame: the peek every 2 s, the pane sheet every 1.5 s, and
/// the Now line's clock each second. True when the view should run again.
pub fn poll(s: &mut Main, t: f64, tx: &mut Tx) -> bool {
    let mut again = false;
    if s.talk.peek.is_some() {
        let current = s.current();
        let working = current.as_deref() == s.talk.peek.as_deref()
            && s.state()
                .and_then(|st| s.talk.peek.as_deref().and_then(|p| st.docket(p).and_then(|d| d.live())))
                .is_some_and(|l| matches!(l.state(), LiveState::Working | LiveState::Starting));
        if !working || !s.chat {
            s.talk.peek = None;
        } else {
            if t - s.talk.peeked_at >= 2.0 {
                peek(s);
            }
            tx.wake_after(std::time::Duration::from_millis(500));
        }
    }
    if matches!(s.sheet, Some(Sheet::Pane(_))) && !s.store.headless {
        if t - s.talk.screen_at >= 1.5 {
            load_screen(s);
        }
        tx.wake_after(std::time::Duration::from_millis(500));
    }
    // A working agent's clock, in the chat.
    if s.chat && !s.store.headless {
        let live = s.current().and_then(|id| s.state().and_then(|st| st.docket(&id).and_then(|d| d.live())));
        if live.is_some_and(|l| matches!(l.state(), LiveState::Working | LiveState::Starting)) {
            let sec = t.floor() as i64;
            if sec != s.talk.second {
                s.talk.second = sec;
                again = true;
            }
            tx.wake_after(std::time::Duration::from_millis(1000));
        }
    }
    again
}

/// Open a kept file: a link in the browser; a file fetched by its hash,
/// checked against it, and opened with its app.
fn open_file(s: &mut Main, e: &Entry, tx: &mut Tx) {
    let d = e.data();
    if let Some(u) = model::text(&d["url"]) {
        let _ = tx.open_url(&u);
        return;
    }
    let Some(digest) = model::text(&d["digest"]) else { return };
    let hex = digest.trim_start_matches("sha256:").to_string();
    let name = model::text(&d["name"]).unwrap_or_else(|| "file".into());
    let seq = e.seq();
    parts::call_bytes(s, format!("/api/blobs/{hex}"), move |s, r| match r {
        Ok(bytes) if parts::sha256_hex(&bytes) == hex => {
            let dir = std::env::temp_dir().join("wheelhouse-rune");
            let path = dir.join(&name);
            if std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&path, &bytes)).is_ok() {
                if let Some(me) = s.me.clone() {
                    let url = format!("file://{}", path.display());
                    me.update(move |_: &mut Main, tx| {
                        let _ = tx.open_url(&url);
                    });
                }
                s.toast(format!("{name} matches its hash in the record (#{seq})"), false);
            }
        }
        Ok(_) => s.toast(format!("{name} doesn't match its hash in the record"), true),
        Err(e) => s.toast(e, true),
    });
}

// ---- the pane, in a sheet ----

pub fn pane_sheet(s: &Main, id: &str, k: &K, kit: &Kit) -> El {
    let st = s.state();
    let d = st.as_ref().and_then(|st| st.docket(id).cloned());
    let state = d.as_ref().and_then(|d| d.live()).map(|l| l.state());
    let app = model::app_name(d.as_ref().and_then(|d| d.agent_app()).as_deref());
    let why = match state {
        Some(LiveState::Error) => Some(format!("{app} stopped with an error.")),
        None => Some("The pane isn't open in Rex.".to_string()),
        _ => None,
    };
    let queues = matches!(state, Some(LiveState::Working | LiveState::Starting | LiveState::Blocked));

    let mut meta = row().gap(6.0).center_y().child(theme::id_text(id, k));
    if let Some(st) = state {
        meta = meta.child(theme::live_icon(Some(st), 11.0, k)).child(text(st.word()).font(font(11.5, 400.0).lh(14.0)).color(k.text2));
    }
    if let Some(d) = &d
        && d.has_agent()
    {
        let pane = d.agent_pane().unwrap_or_else(|| "pane".into());
        meta = meta.child(text(format!("{} / {pane}", d.rex_session().unwrap_or_default())).font(font(11.5, 400.0).lh(14.0)).color(k.muted));
    }
    let (a, b) = (id.to_string(), id.to_string());
    let head = parts::sheet_head(
        row().gap(10.0).center_y().child(theme::app_avatar(d.as_ref().and_then(|d| d.agent_app()).as_deref(), 22.0, k)).child(
            col().gap(2.0).child(text(d.as_ref().map(|d| d.title()).unwrap_or_else(|| id.to_string())).font(font(13.0, 600.0).lh(16.0)).color(k.text).truncate()).child(meta),
        ),
        row().gap(8.0).child(widgets::button("pane-rex", "Open in Rex", false, true, k, move |s: &mut Main, _| s.open_in_rex(&a))).child(widgets::button(
            "pane-done",
            "Done",
            false,
            true,
            k,
            |s: &mut Main, _| s.sheet = None,
        )),
    );
    let screen = if s.talk.screen.is_empty() { s.talk.screen_error.clone().unwrap_or_else(|| "Loading the pane…".into()) } else { s.talk.screen.clone() };
    let term = col()
        .key("pane-screen")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .fill(theme::hex("#121212"))
        .child(text(screen).font(mono(11.5).lh(15.0)).color(theme::hex("#e0e0e0")).selectable(true).padding(14.0))
        .scroll_xy()
        .scroll_bar(kit);
    let foot = match &d {
        Some(d) if d.live().is_some_and(|l| l.answerable()) && state == Some(LiveState::Blocked) => {
            let l = d.live();
            let mut words = col().gap(2.0).grow(1.0).min_w(0.0).child(
                text(format!("{app} is asking{}", l.as_ref().and_then(|l| l.kind()).map(|k| format!(" ({k})")).unwrap_or_default())).font(font(12.5, 600.0).lh(16.0)).color(k.text),
            );
            if let Some(m) = l.and_then(|l| l.msg()) {
                words = words.child(text(m).font(mono(12.0).lh(15.0)).color(k.muted).max_lines(2));
            }
            row().w_full().padding(14.0).gap(10.0).center_y().child(theme::live_icon(Some(LiveState::Blocked), 15.0, k)).child(words).child(lists::answer_buttons(s, d, k))
        }
        _ => {
            let placeholder = why.clone().unwrap_or_else(|| if queues { format!("Queue a message for {app}…") } else { format!("Message {app}…") });
            let c = id.to_string();
            let field = text_input(&s.talk.pane_text, font(13.0, 400.0).lh(18.0))
                .key("pane-composer")
                .w_full()
                .min_h(20.0)
                .max_h(120.0)
                .color(k.text)
                .placeholder(placeholder, k.muted)
                .label(format!("Message {app}"))
                .disabled(why.is_some())
                .on_edit(|s: &mut Main| &mut s.talk.pane_text)
                .on_input_key(move |s: &mut Main, ev: &rune::KeyEvent, _| {
                    if ev.mods.cmd && matches!(ev.key, rune::Key::Enter) {
                        pane_send(s, &c);
                        return true;
                    }
                    false
                });
            let ready = why.is_none() && !s.talk.busy && !s.talk.pane_text.text.trim().is_empty();
            row().w_full().padding(14.0).gap(8.0).items_end().child(row().grow(1.0).min_w(0.0).child(field)).child(widgets::button(
                "pane-send",
                if queues { "Queue  ⌘↩" } else { "Send  ⌘↩" },
                true,
                ready,
                k,
                move |s: &mut Main, _| pane_send(s, &b),
            ))
        }
    };
    parts::sheet_card("Pane", 900.0, Some(640.0), k).child(head).child(parts::divider(k)).child(term).child(parts::divider(k)).child(foot)
}

fn pane_send(s: &mut Main, id: &str) {
    let t = s.talk.pane_text.text.trim().to_string();
    if t.is_empty() || s.talk.busy {
        return;
    }
    s.talk.busy = true;
    let id = id.to_string();
    parts::call(s, Req::Post(format!("/api/dockets/{id}/send"), json!({ "text": t, "queue": true })), move |s, r| {
        s.talk.busy = false;
        match r {
            Ok(v) => {
                s.talk.pane_text.clear();
                if v["queued"].is_number() {
                    s.toast("Queued: it goes in when the turn ends", false);
                }
            }
            Err(e) => s.toast(e, true),
        }
    });
}
