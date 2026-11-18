//! The lists beside a docket (Screens.swift, Lists.swift): the Inbox, a
//! place's dockets grouped by what their agents are doing, Reviews and Builds
//! by phase; the board; Agents and Recap.

use rune::{Color, El, ScrollSpec, col, el, icon, row, spacer, text};
use rune_kit::{Kit, ScrollBar};

use super::{GroupBy, Main, Place, widgets};
use crate::icons;
use crate::model::{self, BuildTask, Concern, Docket, HubState, InboxItem, InboxKind, LiveState, ReviewPr, Status};
use crate::theme::{self, K, font, mono};

pub struct DocketGroup {
    pub id: String,
    pub label: String,
    pub live: Option<LiveState>,
    pub status: Option<Status>,
    pub session: Option<String>,
    pub items: Vec<Docket>,
}

pub fn group_dockets(list: &[Docket], by: GroupBy) -> Vec<DocketGroup> {
    match by {
        GroupBy::Status => [Status::InProgress, Status::Todo, Status::Done, Status::Dropped]
            .into_iter()
            .map(|s| DocketGroup { id: s.raw().into(), label: s.label().into(), live: None, status: Some(s), session: None, items: list.iter().filter(|d| d.status() == s).cloned().collect() })
            .filter(|g| !g.items.is_empty())
            .collect(),
        GroupBy::Session => {
            let mut m: std::collections::BTreeMap<String, Vec<Docket>> = Default::default();
            for d in list {
                m.entry(d.rex_session().unwrap_or_else(|| "By hand".into())).or_default().push(d.clone());
            }
            m.into_iter().map(|(k, v)| DocketGroup { id: k.clone(), label: k.clone(), live: None, status: None, session: Some(k), items: v }).collect()
        }
        GroupBy::Attention => {
            type Test = fn(&Docket) -> bool;
            let tests: [(&str, &str, Option<LiveState>, Option<Status>, Test); 8] = [
                ("blocked", "Needs you", Some(LiveState::Blocked), None, |d| d.live().is_some_and(|l| l.state() == LiveState::Blocked)),
                ("error", "Failed", Some(LiveState::Error), None, |d| d.live().is_some_and(|l| l.state() == LiveState::Error)),
                ("working", "Working", Some(LiveState::Working), None, |d| d.live().is_some_and(|l| matches!(l.state(), LiveState::Working | LiveState::Starting))),
                ("done", "Finished, your move", Some(LiveState::Done), None, |d| d.live().is_some_and(|l| l.state() == LiveState::Done)),
                ("idle", "Idle", Some(LiveState::Idle), None, |d| d.live().is_some()),
                ("todo", "To do", None, Some(Status::Todo), |d| d.is_open()),
                ("closed", "Done", None, Some(Status::Done), |d| d.status() == Status::Done),
                ("dropped", "Dropped", None, Some(Status::Dropped), |_| true),
            ];
            let mut buckets: Vec<Vec<Docket>> = vec![vec![]; tests.len()];
            for d in list {
                let i = tests.iter().position(|t| (t.4)(d)).unwrap_or(tests.len() - 1);
                buckets[i].push(d.clone());
            }
            tests
                .iter()
                .zip(buckets)
                .filter(|(_, b)| !b.is_empty())
                .map(|(t, b)| DocketGroup { id: t.0.into(), label: t.1.into(), live: t.2, status: t.3, session: None, items: b })
                .collect()
        }
    }
}

pub fn review_groups(st: &HubState) -> Vec<(&'static str, &'static str, LiveState, Vec<ReviewPr>)> {
    let sorted = |f: &dyn Fn(&ReviewPr) -> bool| {
        let mut v: Vec<ReviewPr> = st.prs.iter().filter(|p| f(p)).cloned().collect();
        v.sort_by(|a, b| b.updated_at().cmp(&a.updated_at()));
        v
    };
    vec![
        ("needs", "Needs you", LiveState::Blocked, sorted(&|p| p.attention().is_some())),
        ("reviewing", "Reviewing", LiveState::Working, sorted(&|p| p.attention().is_none() && p.state() == "reviewing")),
        ("queued", "Queued", LiveState::Idle, sorted(&|p| p.attention().is_none() && p.state() == "queued")),
        ("posted", "Waiting on the author", LiveState::Done, sorted(&|p| p.attention().is_none() && p.state() == "posted")),
        ("done", "Done this week", LiveState::Idle, sorted(&|p| p.attention().is_none() && matches!(p.state().as_str(), "merged" | "closed" | "cleaned" | "unrequested"))),
    ]
    .into_iter()
    .filter(|g| !g.3.is_empty())
    .collect()
}

pub fn build_groups(st: &HubState) -> Vec<(&'static str, &'static str, LiveState, Vec<BuildTask>)> {
    let sorted = |f: &dyn Fn(&BuildTask) -> bool| {
        let mut v: Vec<BuildTask> = st.tasks.iter().filter(|t| f(t)).cloned().collect();
        v.sort_by(|a, b| b.updated_at().cmp(&a.updated_at()));
        v
    };
    vec![
        ("needs", "Needs you", LiveState::Blocked, sorted(&|t| t.needs_you())),
        ("working", "Working", LiveState::Working, sorted(&|t| !t.needs_you() && t.state() == "active")),
        ("queued", "Queued", LiveState::Idle, sorted(&|t| !t.needs_you() && t.state() == "queued")),
        ("open", "PRs open", LiveState::Done, sorted(&|t| t.state() == "open")),
        ("done", "Done this week", LiveState::Idle, sorted(&|t| t.finished())),
    ]
    .into_iter()
    .filter(|g| !g.3.is_empty())
    .collect()
}

/// The ids the current list shows, in order: for J and K.
fn ids(s: &Main, st: &HubState) -> Vec<String> {
    match &s.place {
        Place::Inbox => {
            let items = model::inbox(st);
            let concerns = model::concerns(st, &items);
            items.iter().map(|i| i.id.clone()).chain(concerns.iter().map(|c| c.id())).collect()
        }
        Place::Reviews => review_groups(st).into_iter().flat_map(|g| g.3).map(|p| p.docket().unwrap_or_else(|| p.full_key())).collect(),
        Place::Builds => build_groups(st).into_iter().flat_map(|g| g.3).map(|t| t.docket()).collect(),
        p => {
            let shown: Vec<Docket> = st.dockets.iter().filter(|d| p.shows(d)).cloned().collect();
            group_dockets(&shown, s.group_by).into_iter().flat_map(|g| g.items).map(|d| d.id()).collect()
        }
    }
}

/// J and K: the next or the previous row.
pub fn step(s: &mut Main, by: i32) {
    let Some(st) = s.state() else { return };
    let ids = ids(s, &st);
    if ids.is_empty() {
        return;
    }
    let cur = s.selection.get(&s.place).and_then(|id| ids.iter().position(|x| x == id)).unwrap_or(0) as i32;
    let next = (cur + by).clamp(0, ids.len() as i32 - 1) as usize;
    s.select(ids[next].clone());
}

fn selected_id(s: &Main, st: &HubState) -> Option<String> {
    s.selection.get(&s.place).cloned().or_else(|| ids(s, st).into_iter().next())
}

fn list_row(key: String, on: bool, k: &K, content: El, select: String) -> El {
    let menu_id = select.clone();
    row()
        .on_secondary_press(move |s: &mut Main, press, tx| {
            let d = s.state().and_then(|st| docket_for(&st, &menu_id));
            if let Some(d) = d {
                super::menus::open(&d, press, tx);
            }
        })
        .key(key)
        .w_full()
        .px(10.0)
        .py(7.0)
        .radius(8.0)
        .fill(if on { k.sel } else { Color::TRANSPARENT })
        .hover(k.hover)
        .role("option")
        .focus_selected(on)
        .on_press(move |s: &mut Main| s.select(select.clone()))
        .child(content.grow(1.0).min_w(0.0))
}

/// The docket a row is about: its id, an Inbox item's, or a concern's.
fn docket_for(st: &HubState, id: &str) -> Option<Docket> {
    if let Some(d) = st.docket(id) {
        return Some(d.clone());
    }
    let items = model::inbox(st);
    if let Some(it) = items.iter().find(|i| i.id == id) {
        return st.docket(&it.docket_id).cloned();
    }
    model::concerns(st, &items).into_iter().find(|c| c.id() == id).and_then(|c| c.docket).and_then(|d| st.docket(&d).cloned())
}

fn scroll(key: &str, rows: El, k: &K) -> El {
    let _ = k;
    let kit = k.kit();
    rows.key(key.to_string()).w_full().grow(1.0).min_h(0.0).px(6.0).pb(12.0).scroll(ScrollSpec { stick_bottom: false }).scroll_bar(&kit)
}

pub fn list(s: &Main, st: &HubState, k: &K, _kit: &Kit) -> El {
    match &s.place {
        Place::Inbox => inbox(s, st, k),
        Place::Reviews => reviews(s, st, k),
        Place::Builds => builds(s, st, k),
        _ => dockets(s, st, k),
    }
}

// ---- the Inbox ----

fn inbox(s: &Main, st: &HubState, k: &K) -> El {
    let items = model::inbox(st);
    let concerns = model::concerns(st, &items);
    if items.is_empty() && concerns.is_empty() {
        return widgets::empty(&icons::TRAY, "You're all caught up", "Nothing is waiting on you. Agents that block, fail, or finish a turn show up here.", k);
    }
    let sel = selected_id(s, st);
    let mut rows = col().role("listbox").label("Inbox").gap(1.0);
    if !items.is_empty() {
        if !concerns.is_empty() {
            rows = rows.child(header(theme::live_icon(Some(LiveState::Blocked), 13.0, k), "Needs you", items.len(), k));
        }
        for it in &items {
            rows = rows.child(list_row(format!("inbox-{}", it.id), sel.as_deref() == Some(&it.id), k, inbox_row(s, it, k), it.id.clone()));
        }
    }
    if !concerns.is_empty() {
        rows = rows.child(header(icon(&icons::EYE, 13.0).color(k.muted), "Attention", concerns.len(), k));
        for c in &concerns {
            rows = rows.child(list_row(format!("concern-{}", c.id()), sel.as_deref() == Some(&c.id()), k, concern_row(c, k), c.id()));
        }
    }
    scroll("inbox-list", rows, k)
}

fn header(mark: El, label: &str, count: usize, k: &K) -> El {
    row()
        .w_full()
        .h(30.0)
        .px(10.0)
        .gap(7.0)
        .center_y()
        .role("heading")
        .level(2)
        .child(mark)
        .child(text(label.to_string()).font(font(12.0, 600.0).lh(15.0)).color(k.text))
        .child(text(count.to_string()).font(font(11.5, 400.0).lh(15.0)).color(k.faint))
}

fn title_line(title: &str, at: Option<&str>, weight: f32, k: &K) -> El {
    row()
        .w_full()
        .gap(6.0)
        .child(text(title.to_string()).font(font(13.0, weight).lh(17.0)).color(k.text).truncate().shrink())
        .child(spacer())
        .child(text(model::ago(at)).font(font(11.5, 400.0).lh(17.0)).color(k.faint))
}

fn inbox_row(s: &Main, it: &InboxItem, k: &K) -> El {
    let avatar = if it.pr.is_some() {
        row().size(24.0, 24.0).radius(6.0).fill(k.recessed).center().child(icon(&icons::PULL, 13.0).color(k.muted))
    } else if it.msg.as_ref().is_some_and(|m| m.from() == "wheelhouse") {
        theme::hub_avatar(24.0, k)
    } else {
        let app = it.docket.as_ref().and_then(|d| d.agent_app()).or_else(|| it.msg.as_ref().map(|m| if m.from_app().as_deref() == Some("Codex") { "codex".to_string() } else { "claude-code".to_string() }));
        theme::app_avatar(app.as_deref(), 24.0, k)
    };
    let badge = theme::live_icon(Some(match it.kind {
        InboxKind::Error => LiveState::Error,
        InboxKind::Done => LiveState::Done,
        _ => LiveState::Blocked,
    }), 12.0, k);
    let mark = el().size(28.0, 28.0).child(avatar).child(el().absolute(15.0, 15.0).size(14.0, 14.0).radius(7.0).fill(k.main).center().child(badge));
    let prefix = match it.kind {
        InboxKind::Blocked => it.docket.as_ref().and_then(|d| d.live()).and_then(|l| l.kind()).map(|k| format!("{k}: ")).unwrap_or_else(|| "Needs you: ".into()),
        InboxKind::Error => "Failed: ".into(),
        _ => String::new(),
    };
    let mut body = col()
        .gap(3.0)
        .child(title_line(&it.title, it.at.as_deref(), 600.0, k))
        .child(
            row()
                .w_full()
                .gap(6.0)
                .child(theme::id_text(&it.docket_id, k))
                .child(text(format!("{prefix}{}", it.line)).font(font(12.0, 400.0).lh(15.0)).color(k.muted).wrap().max_lines(2).shrink()),
        );
    if let Some(p) = it.pr.as_ref().filter(|p| p.attention().is_some_and(|a| a.kind == "ready")) {
        body = body.child(review_actions(s, p, k));
    } else if let Some(m) = it.msg.as_ref().filter(|m| !m.options().is_empty()) {
        body = body.child(option_buttons(s, m, k));
    } else if it.kind == InboxKind::Blocked {
        if let Some(d) = it.docket.as_ref().filter(|d| d.live().is_some_and(|l| l.answerable())) {
            body = body.child(answer_buttons(s, d, k));
        }
    }
    row().w_full().gap(10.0).child(el().pt(1.0).child(mark)).child(body.grow(1.0).min_w(0.0))
}

fn concern_row(c: &Concern, k: &K) -> El {
    let (i, tint) = match c.kind.as_str() {
        "build" => (&icons::HAMMER, k.blocked),
        "review" => (&icons::PULL, k.blocked),
        "failing" => (&icons::DENY, k.error),
        "held" => (&icons::PAUSE, k.blocked),
        "long" => (&icons::CLOCK, k.progress),
        "quiet" => (&icons::MOON, k.faint),
        "word" => (&icons::QUOTE, k.accent),
        _ => (&icons::EYE, k.faint),
    };
    row()
        .w_full()
        .gap(10.0)
        .help(c.label())
        .child(row().size(24.0, 24.0).radius(6.0).fill(k.recessed).center().child(icon(i, 13.0).color(tint)))
        .child(
            col()
                .gap(3.0)
                .grow(1.0)
                .min_w(0.0)
                .child(title_line(&c.title, c.at.as_deref(), 600.0, k))
                .child(
                    row()
                        .w_full()
                        .gap(6.0)
                        .child(theme::id_text(c.docket.as_deref().unwrap_or(""), k))
                        .child(text(c.text.replace("**", "").replace('`', "")).font(font(12.0, 400.0).lh(15.0)).color(k.muted).wrap().max_lines(2).shrink()),
                ),
        )
}

pub fn answer_buttons(s: &Main, d: &Docket, k: &K) -> El {
    let id = d.id();
    let (a, n) = (d.clone(), d.clone());
    let busy = s.busy.contains(&format!("approve:{id}")) || s.busy.contains(&format!("deny:{id}"));
    row()
        .gap(6.0)
        .pt(3.0)
        .child(widgets::button(&format!("approve-{id}"), "Approve", false, !busy, k, move |s: &mut Main, _| s.answer(&a, true)))
        .child(widgets::button(&format!("deny-{id}"), "Deny", false, !busy, k, move |s: &mut Main, _| s.answer(&n, false)))
}

pub fn option_buttons(s: &Main, m: &model::Msg, k: &K) -> El {
    let busy = s.busy.contains(&format!("reply:{}", m.id()));
    let mut r = row().gap(6.0).pt(3.0);
    for (i, o) in m.options().into_iter().enumerate() {
        let (m2, o2) = (m.clone(), o.clone());
        r = r.child(widgets::button(&format!("opt-{}-{i}", m.id()), &o, i == 0, !busy, k, move |s: &mut Main, _| s.reply(&m2, &o2)));
    }
    r
}

pub fn review_actions(s: &Main, p: &ReviewPr, k: &K) -> El {
    let busy = s.busy.contains(&format!("post:{}", p.full_key())) || s.busy.contains(&format!("dismiss:{}", p.full_key()));
    let (a, b) = (p.clone(), p.clone());
    row()
        .gap(6.0)
        .pt(3.0)
        .child(widgets::button(&format!("post-{}", p.full_key()), "Post review", true, !busy, k, move |s: &mut Main, _| s.post_review(&a)))
        .child(widgets::button(&format!("dismiss-{}", p.full_key()), "Dismiss", false, !busy, k, move |s: &mut Main, _| s.dismiss_review(&b)))
}

// ---- a place's dockets ----

fn dockets(s: &Main, st: &HubState, k: &K) -> El {
    let shown: Vec<Docket> = st.dockets.iter().filter(|d| s.place.shows(d)).cloned().collect();
    if shown.is_empty() {
        return widgets::empty(&icons::STACK, "No dockets here", "Dockets open when an agent starts in Rex, or when you make one with C.", k);
    }
    let sel = selected_id(s, st);
    let mut rows = col().role("listbox").label(s.place.title()).gap(1.0);
    for g in group_dockets(&shown, s.group_by) {
        let key = format!("{}:{}", s.place.title(), g.id);
        let folded = s.folded.contains(&key);
        let mark = group_mark(&g, k);
        rows = rows.child(widgets::fold_header(&key, mark, &g.label, g.items.len(), folded, k));
        if !folded {
            for d in &g.items {
                rows = rows.child(list_row(format!("docket-{}", d.id()), sel.as_deref() == Some(&d.id()), k, docket_row(d, k), d.id()));
            }
        }
    }
    scroll("docket-list", rows, k)
}

fn group_mark(g: &DocketGroup, k: &K) -> El {
    if let Some(l) = g.live {
        theme::live_icon(Some(l), 13.0, k)
    } else if let Some(s) = g.status {
        theme::status_icon(s, 13.0, k)
    } else if let Some(s) = &g.session {
        theme::session_icon(s, 14.0, k)
    } else {
        el()
    }
}

pub fn docket_row(d: &Docket, k: &K) -> El {
    let mark = match d.live() {
        Some(l) => theme::live_icon(Some(l.state()), 14.0, k),
        None => theme::status_icon(d.status(), 14.0, k),
    };
    let mut meta = row().w_full().gap(6.0).center_y().child(theme::id_text(&d.id(), k));
    if d.live().is_some() {
        meta = meta.child(theme::status_icon(d.status(), 11.0, k));
    }
    if let Some(s) = d.rex_session() {
        meta = meta.child(row().gap(4.0).center_y().child(theme::session_icon(&s, 12.0, k)).child(text(s).font(font(11.5, 400.0).lh(14.0)).color(k.muted)));
    }
    if d.turns() > 0 {
        meta = meta.child(text(model::plural(d.turns() as usize, "turn")).font(font(11.5, 400.0).lh(14.0)).color(k.muted));
    }
    if let Some(l) = d.linear_key() {
        meta = meta.child(text(l).font(mono(11.0).lh(14.0)).color(k.muted));
    }
    if let Some(r) = d.review_key() {
        meta = meta.child(text(r).font(font(11.5, 400.0).lh(14.0)).color(k.muted).truncate().shrink());
    }
    meta = meta.child(spacer());
    if d.has_agent() {
        meta = meta.child(theme::app_avatar(d.agent_app().as_deref(), 14.0, k));
    }
    let mut body = col().gap(3.0).grow(1.0).min_w(0.0).child(title_line(&d.title(), Some(&d.updated_at()), 500.0, k)).child(meta);
    if let Some(m) = d.live().filter(|l| l.state() == LiveState::Blocked).and_then(|l| l.msg()) {
        body = body.child(text(m).font(font(11.5, 400.0).lh(14.0)).color(k.blocked).truncate());
    }
    if let Some(why) = d.hold() {
        body = body.child(
            row()
                .gap(5.0)
                .center_y()
                .child(icon(&icons::PAUSE, 11.0).color(k.blocked))
                .child(text(format!("On hold: {why}")).font(font(11.5, 400.0).lh(14.0)).color(k.blocked).truncate().shrink()),
        );
    }
    row().w_full().gap(9.0).child(el().pt(1.5).child(mark)).child(body)
}

// ---- Reviews and Builds ----

fn bar(items: Vec<El>, k: &K) -> El {
    let mut r = row().key("bar").w_full().px(14.0).py(8.0).gap(8.0).center_y();
    for i in items {
        r = r.child(i);
    }
    col().w_full().child(r).child(theme::hairline(k))
}

fn reviews(s: &Main, st: &HubState, k: &K) -> El {
    let rv = st.review();
    if rv.is_object() && !model::flag(&rv["enabled"]) {
        return widgets::empty(&icons::PULL, "Reviews run in Wheelhouse", "Each PR that asks you for a review becomes a Review docket: blind reviewers in Rex, a checker that merges their drafts and rates the merge risk, and a post you approve.", k);
    }
    let groups = review_groups(st);
    if groups.is_empty() {
        return widgets::empty(&icons::PULL, "No reviews asked of you", "Each PR that requests your review, in the repos review.lua lists, shows up here.", k);
    }
    let paused = model::flag(&rv["paused"]);
    let c = &rv["config"];
    let mut bits = vec![theme::chip(if paused { "Paused" } else { "On" }, if paused { k.blocked } else { k.working }, false)];
    let scan = model::text(&rv["last_scan_at"]);
    bits.push(text(match scan.as_deref() {
        None => "Not checked GitHub yet".to_string(),
        Some(t) if model::ago(Some(t)) == "now" => "Checked GitHub just now".to_string(),
        Some(t) => format!("Checked GitHub {} ago", model::ago(Some(t))),
    })
    .font(font(12.0, 400.0).lh(15.0))
    .color(k.muted));
    if c.is_object() {
        let reviewers: Vec<String> = model::arr(&c["reviewers"]).iter().filter_map(|r| model::text(&r["name"])).collect();
        bits.push(text(format!("{} → checker {}", reviewers.join(", "), model::str_or(&c["checker"], ""))).font(font(12.0, 400.0).lh(15.0)).color(k.muted).truncate().shrink());
    }
    let sel = selected_id(s, st);
    let mut rows = col().role("listbox").label("Reviews").gap(1.0);
    for (key, label, live, items) in groups {
        let fk = format!("reviews:{key}");
        let folded = s.folded.contains(&fk);
        rows = rows.child(widgets::fold_header(&fk, theme::live_icon(Some(live), 13.0, k), label, items.len(), folded, k));
        if !folded {
            for p in &items {
                let id = p.docket().unwrap_or_else(|| p.full_key());
                rows = rows.child(list_row(format!("review-{id}"), sel.as_deref() == Some(&id), k, review_row(s, p, paused, k), id));
            }
        }
    }
    col().w_full().h_full().child(bar(bits, k)).child(scroll("review-list", rows, k))
}

pub fn review_live(p: &ReviewPr) -> LiveState {
    if p.attention().is_some() {
        return LiveState::Blocked;
    }
    match p.state().as_str() {
        "reviewing" => LiveState::Working,
        "posted" => LiveState::Done,
        _ => LiveState::Idle,
    }
}

pub fn phase_text(p: &ReviewPr, paused: bool) -> String {
    match (p.state().as_str(), p.phase().as_deref()) {
        ("queued", _) if paused => "Queued: reviews are paused".into(),
        ("queued", _) => "Queued: starts when a slot frees".into(),
        ("reviewing", Some("reviewers")) => format!("Reviewers {}/{}", p.reviewers_done(), p.reviewers_total()),
        ("reviewing", Some("checker")) => "The checker is merging the drafts".into(),
        ("reviewing", Some("awaiting")) => "Checked: ready to post".into(),
        ("reviewing", Some(ph)) => ph.replace('_', " "),
        (s, _) => s.replace('_', " "),
    }
}

fn verdict_chip(v: &str, k: &K) -> El {
    let l = v.to_lowercase();
    let c = if l.contains("approve") { k.working } else if l.contains("request") || l.contains("changes") { k.error } else { k.muted };
    let label = l.replace('_', " ");
    let mut cs = label.chars();
    theme::chip(&cs.next().map(|f| f.to_uppercase().collect::<String>() + cs.as_str()).unwrap_or_default(), c, false)
}

fn review_row(s: &Main, p: &ReviewPr, paused: bool, k: &K) -> El {
    let mut meta = row().w_full().gap(6.0).center_y();
    if let Some(d) = p.docket() {
        meta = meta.child(theme::id_text(&d, k));
    }
    meta = meta.child(text(p.key()).font(font(11.5, 400.0).lh(14.0)).color(k.muted).truncate().shrink());
    if let Some(a) = p.author() {
        meta = meta.child(text(format!("· {a}")).font(font(11.5, 400.0).lh(14.0)).color(k.muted));
    }
    meta = meta.child(spacer());
    if p.round() > 0 {
        meta = meta.child(text(format!("R{}", p.round())).font(mono(11.0).lh(14.0)).color(k.muted));
    }
    let mut body = col().gap(3.0).grow(1.0).min_w(0.0).child(title_line(&p.title().unwrap_or_else(|| p.key()), p.updated_at().as_deref(), 500.0, k)).child(meta);
    let v = if p.state() == "reviewing" { None } else { p.verdict().or_else(|| p.history().first().and_then(|h| model::text(&h["verdict"]))) };
    if v.is_some() || p.merge_risk().is_some() {
        let mut chips = row().gap(5.0);
        if let Some(v) = v {
            chips = chips.child(verdict_chip(&v, k));
        }
        if let Some(r) = p.merge_risk() {
            chips = chips.child(theme::chip(&format!("Risk {r}"), k.risk(Some(&r)), false));
        }
        body = body.child(chips);
    }
    if let Some(a) = p.attention() {
        let line = if a.kind == "ready" {
            [model::draft_line(p).or(Some(a.text.clone())), p.waits_why().map(|w| format!("waits for you: {w}"))].into_iter().flatten().collect::<Vec<_>>().join(" · ")
        } else {
            a.text.clone()
        };
        body = body.child(text(line).font(font(11.5, 400.0).lh(14.0)).color(k.blocked).wrap().max_lines(2));
        if a.kind == "ready" {
            body = body.child(review_actions(s, p, k));
        }
    } else if matches!(p.state().as_str(), "reviewing" | "queued") {
        body = body.child(text(phase_text(p, paused)).font(font(11.5, 400.0).lh(14.0)).color(k.text2));
    }
    row().w_full().gap(9.0).child(el().pt(1.5).child(theme::live_icon(Some(review_live(p)), 14.0, k))).child(body)
}

pub fn build_live(t: &BuildTask) -> LiveState {
    if t.needs_you() {
        return LiveState::Blocked;
    }
    match t.state().as_str() {
        "active" => LiveState::Working,
        "open" => LiveState::Done,
        "failed" => LiveState::Error,
        _ => LiveState::Idle,
    }
}

pub fn build_phase(t: &BuildTask) -> String {
    match (t.state().as_str(), t.phase().as_deref()) {
        ("active", Some(p)) => p.replace('_', " "),
        ("open", _) => t.pr_number().map(|n| format!("PR #{n} open")).unwrap_or_else(|| "PR open".into()),
        (s, _) => s.replace('_', " "),
    }
}

fn builds(s: &Main, st: &HubState, k: &K) -> El {
    let groups = build_groups(st);
    if groups.is_empty() {
        return widgets::empty(
            &icons::HAMMER,
            "Nothing's building",
            "Open a docket, write what it's for in its intent, and press Build it: a builder plans it, you approve the plan, a checker checks the work, and you approve the PR before it opens.",
            k,
        );
    }
    let bs = st.build();
    let paused = model::flag(&bs["paused"]);
    let c = &bs["config"];
    let mut bits = vec![theme::chip(if paused { "Paused" } else { "On" }, if paused { k.blocked } else { k.working }, false)];
    if c.is_object() {
        bits.push(text(format!("builder {} → checker {}", model::str_or(&c["builder"], ""), model::str_or(&c["checker"], ""))).font(font(12.0, 400.0).lh(15.0)).color(k.muted));
    }
    let sel = selected_id(s, st);
    let mut rows = col().role("listbox").label("Builds").gap(1.0);
    for (key, label, live, items) in groups {
        let fk = format!("builds:{key}");
        let folded = s.folded.contains(&fk);
        rows = rows.child(widgets::fold_header(&fk, theme::live_icon(Some(live), 13.0, k), label, items.len(), folded, k));
        if !folded {
            for t in &items {
                rows = rows.child(list_row(format!("build-{}", t.docket()), sel.as_deref() == Some(&t.docket()), k, build_row(t, k), t.docket()));
            }
        }
    }
    col().w_full().h_full().child(bar(bits, k)).child(scroll("build-list", rows, k))
}

fn build_row(t: &BuildTask, k: &K) -> El {
    let mut meta = row().w_full().gap(6.0).center_y().child(theme::id_text(&t.docket(), k));
    if t.key() != t.docket() {
        meta = meta.child(text(t.key()).font(font(11.5, 400.0).lh(14.0)).color(k.muted));
    }
    meta = meta.child(text(format!("· {}", build_phase(t))).font(font(11.5, 400.0).lh(14.0)).color(k.muted)).child(spacer());
    if let Some(b) = t.branch() {
        meta = meta.child(text(b).font(mono(11.0).lh(14.0)).color(k.muted).truncate().shrink());
    }
    let mut body = col().gap(3.0).grow(1.0).min_w(0.0).child(title_line(&t.title(), t.updated_at().as_deref(), 500.0, k)).child(meta);
    if let Some(a) = t.attention() {
        body = body.child(text(a.text).font(font(11.5, 400.0).lh(14.0)).color(k.blocked).wrap().max_lines(2));
    }
    row().w_full().gap(9.0).child(el().pt(1.5).child(theme::live_icon(Some(build_live(t)), 14.0, k))).child(body)
}

// ---- the board ----

pub fn board(s: &Main, st: &HubState, k: &K) -> El {
    let shown: Vec<Docket> = st.dockets.iter().filter(|d| s.place.shows(d)).cloned().collect();
    let mut cols = row().key("board").gap(12.0).padding(14.0).h_full();
    for g in group_dockets(&shown, s.group_by) {
        let mut cards = col().gap(8.0).w_full();
        for d in &g.items {
            let id = d.id();
            cards = cards.child(
                col()
                    .key(format!("card-{id}"))
                    .w_full()
                    .padding(10.0)
                    .gap(6.0)
                    .radius(10.0)
                    .continuous_corners()
                    .fill(k.elev)
                    .border(1.0, k.line)
                    .box_shadow(rune::Shadow { x: 0.0, y: 1.0, blur: 2.0, spread: 0.0, color: Color::BLACK.alpha(0.05), inset: false })
                    .hover(k.hover)
                    .role("button")
                    .label(d.title())
                    .on_press(move |s: &mut Main| {
                        s.board = false;
                        s.select(id.clone());
                    })
                    .child(text(d.title()).font(font(13.0, 500.0).lh(17.0)).color(k.text).wrap().max_lines(3))
                    .child(
                        row()
                            .gap(6.0)
                            .center_y()
                            .child(theme::status_icon(d.status(), 12.0, k))
                            .child(theme::id_text(&d.id(), k))
                            .child(spacer())
                            .child(text(model::ago(Some(&d.updated_at()))).font(font(11.0, 400.0).lh(14.0)).color(k.faint)),
                    ),
            );
        }
        cols = cols.child(
            col()
                .key(format!("lane-{}", g.id))
                .w(270.0)
                .h_full()
                .min_h(0.0)
                .gap(8.0)
                .child(row().gap(7.0).px(4.0).center_y().child(group_mark(&g, k)).child(text(g.label.clone()).font(font(12.0, 600.0).lh(15.0)).color(k.text)).child(text(g.items.len().to_string()).font(font(11.5, 400.0).lh(15.0)).color(k.faint)))
                .child(cards.grow(1.0).min_h(0.0).scroll(ScrollSpec { stick_bottom: false })),
        );
    }
    el().key("board-scroll").w_full().h_full().scroll_x().child(cols)
}

// ---- Agents ----

pub fn agents(_s: &Main, st: &HubState, k: &K) -> El {
    let agents = st.agents();
    if agents.is_empty() {
        return widgets::empty(&icons::AGENT, "No agents running", "Agents in Rex panes show up here: Claude Code and Codex, what each is doing, and the docket it's on.", k);
    }
    let mut rows = col().role("list").label("Agents").gap(1.0).padding(14.0);
    rows = rows.child(
        row()
            .w_full()
            .px(10.0)
            .pb(6.0)
            .gap(10.0)
            .child(text("Agent").font(font(11.5, 600.0).lh(14.0)).color(k.muted).w(220.0))
            .child(text("Doing").font(font(11.5, 600.0).lh(14.0)).color(k.muted).w(150.0))
            .child(text("Docket").font(font(11.5, 600.0).lh(14.0)).color(k.muted).grow(1.0)),
    );
    for a in &agents {
        let doc = a.docket().and_then(|id| st.docket(&id).map(|d| (id, d.title())));
        rows = rows.child(
            row()
                .key(format!("agent-{}", a.block()))
                .w_full()
                .px(10.0)
                .py(8.0)
                .gap(10.0)
                .radius(8.0)
                .hover(k.hover)
                .center_y()
                .child(
                    row()
                        .w(220.0)
                        .gap(8.0)
                        .center_y()
                        .child(theme::app_avatar(Some(&a.app()), 20.0, k))
                        .child(
                            col()
                                .gap(1.0)
                                .shrink()
                                .min_w(0.0)
                                .child(text(format!("{} / {}", a.rex_session(), a.pane())).font(font(12.5, 500.0).lh(16.0)).color(k.text).truncate())
                                .child(text(a.cwd().unwrap_or_default()).font(font(11.0, 400.0).lh(14.0)).color(k.muted).truncate()),
                        ),
                )
                .child(row().w(150.0).gap(6.0).center_y().child(theme::live_icon(Some(a.state()), 13.0, k)).child(text(a.state().word()).font(font(12.0, 400.0).lh(15.0)).color(k.text2)))
                .child(match doc {
                    Some((id, title)) => row().grow(1.0).min_w(0.0).gap(6.0).child(theme::id_text(&id, k)).child(text(title).font(font(12.5, 400.0).lh(16.0)).color(k.text).truncate().shrink()),
                    None => text("No docket").font(font(12.0, 400.0).lh(15.0)).color(k.faint),
                }),
        );
    }
    rows.key("agents").w_full().h_full().scroll(ScrollSpec { stick_bottom: false })
}

// ---- Recap ----

pub fn recap(_s: &Main, st: &HubState, k: &K) -> El {
    let open = st.dockets.iter().filter(|d| d.is_open()).count();
    let done = st.dockets.iter().filter(|d| d.status() == Status::Done).count();
    let turns: i64 = st.dockets.iter().map(|d| d.turns()).sum();
    let tile = |label: &str, n: String| {
        col()
            .w(160.0)
            .padding(14.0)
            .gap(4.0)
            .radius(10.0)
            .fill(k.elev)
            .border(1.0, k.line)
            .child(text(n).font(font(24.0, 600.0).lh(28.0)).color(k.text))
            .child(text(label.to_string()).font(font(12.0, 400.0).lh(15.0)).color(k.muted))
    };
    col()
        .key("recap")
        .w_full()
        .h_full()
        .padding(24.0)
        .gap(16.0)
        .child(text("This week").font(font(20.0, 600.0).lh(24.0)).color(k.text))
        .child(row().gap(12.0).child(tile("Open dockets", open.to_string())).child(tile("Done", done.to_string())).child(tile("Turns", turns.to_string())).child(tile("Reviews", st.prs.len().to_string())))
}
