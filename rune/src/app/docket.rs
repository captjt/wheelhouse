//! A docket (DocketView.swift): its header, what it's waiting on you for, its
//! intent, decisions, Verify, kept files, activity and the composer; the
//! inspector beside it; and the sheets it opens.

use rune::{El, ScrollSpec, col, icon, row, spacer, text, text_input};
use rune_kit::{Kit, ScrollBar};

use super::{Main, Place, Sheet, activity, chat, forms, lists, panels, parts, record, shortcuts, verify, widgets};
use crate::icons;
use crate::model::{self, Docket, DocketPage, HubState, LiveState, Status};
use crate::theme::{self, K, font, mono};

/// Markdown as the app writes it.
pub fn markdown(src: &str, k: &K, kit: &Kit) -> El {
    parts::md_with(src, None, k, kit, None)
}

/// The intent, with its task boxes live: ticking one saves the intent as a
/// new version, as the Mac app does.
fn intent_markdown(id: &str, src: &str, k: &K, kit: &Kit) -> El {
    let (id, body) = (id.to_string(), src.to_string());
    let tick: parts::OnTask = std::rc::Rc::new(move |s: &mut Main, at: usize, checked: bool| {
        let mut body = body.clone();
        if body.get(at..at + 1) == Some("[") && body.get(at + 2..at + 3) == Some("]") {
            body.replace_range(at + 1..at + 2, if checked { "x" } else { " " });
            let id = id.clone();
            s.act(&format!("intent:{id}"), format!("/api/dockets/{id}/intent"), serde_json::json!({ "body": body }), move |r| match r {
                Ok(_) => (format!("{id}: intent saved"), false),
                Err(e) => (e, true),
            });
        }
    });
    parts::md_with(src, None, k, kit, Some(tick))
}

pub fn detail(s: &Main, st: &HubState, id: &str, k: &K, kit: &Kit) -> El {
    let Some(d) = st.docket(id) else {
        return widgets::empty(&icons::STACK, "No docket", "It isn't in the hub's state.", k);
    };
    if s.chat && d.talks() && !s.embedded() {
        return chat::view(s, st, d, k, kit);
    }
    let page = s.pages.get(id);
    let mut c = col().key("docket").w_full().max_w(860.0).px(if s.embedded() { 24.0 } else { 36.0 }).pt(22.0).pb(40.0).gap(18.0);
    c = c.child(header(s, d, k));
    if let Some(h) = verify::hold_banner(d, k) {
        c = c.child(h);
    }
    // What it waits on you for: a prompt, a review to post, a Build's call,
    // an agent's ask.
    if let Some(l) = d.live().filter(|l| l.state() == LiveState::Blocked) {
        c = c.child(prompt_callout(s, d, &l, k));
    }
    // It failed: what it said (Callout(tone: .error)).
    if let Some(l) = d.live().filter(|l| l.state() == LiveState::Error) {
        let mut body =
            col().gap(4.0).grow(1.0).min_w(0.0).child(text(format!("{} failed", model::app_name(d.agent_app().as_deref()))).font(font(13.0, 600.0).lh(16.0)).color(k.text));
        if let Some(m) = l.msg() {
            body = body.child(text(m).font(font(12.5, 400.0).lh(17.0)).color(k.text2).wrap().selectable(true));
        }
        c = c.child(row().key("failed").w_full().padding(14.0).radius(10.0).fill(k.danger_tint).border(1.0, k.error.alpha(0.25)).child(body));
    }
    if let Some(p) = panels::review(s, st, d, k, kit) {
        c = c.child(p);
    }
    if let Some(b) = panels::build(st, d, k) {
        c = c.child(b);
    }
    for m in st.messages.iter().filter(|m| m.docket().as_deref() == Some(id)) {
        c = c.child(message_card(s, m, k, kit));
    }
    c = c.child(intent(s, d, page.map(|p| p.as_ref()), k, kit));
    if let Some(p) = page {
        if !p.decisions().is_empty() {
            c = c.child(decisions(p, k));
        }
        if let Some(v) = verify::section(s, d, p, k) {
            c = c.child(v);
        }
        let files: Vec<_> = p.entries.iter().filter(|e| e.kind() == "artifact").collect();
        if !files.is_empty() {
            c = c.child(artifacts(&files, k));
        }
        c = c.child(activity::activity(s, st, d, p, k, kit));
    } else if let Some(e) = s.page_errors.get(id) {
        c = c.child(text(e.clone()).font(font(12.0, 400.0).lh(16.0)).color(k.error));
    } else {
        c = c.child(text("Loading the record…").font(font(12.0, 400.0).lh(16.0)).color(k.muted));
    }
    c = c.child(composer(s, d, false, k));
    let body = col().key("docket-scroll").w_full().h_full().items_center().child(c);
    body.scroll(ScrollSpec { stick_bottom: false }).scroll_bar(kit)
}

fn header(s: &Main, d: &Docket, k: &K) -> El {
    let id = d.id();
    // Inside the Inbox, the docket's buttons sit on its id row (DocketBody's
    // `embedded`); elsewhere they're in the toolbar or the window's title bar.
    let embedded = s.embedded();
    let mut top = row().w_full().gap(8.0).center_y().child(theme::id_text(&id, k)).child(spacer());
    if d.live().is_some() {
        let (a, b) = (id.clone(), id.clone());
        top = top
            .child(widgets::button("open-rex", "Open in Rex", false, true, k, move |s: &mut Main, _| s.open_in_rex(&a)))
            .child(widgets::icon_button("peek", &icons::EYE, "Peek at the pane", move |s: &mut Main| chat::open_pane(s, &b), k).size(26.0, 22.0));
    }
    let b = id.clone();
    top = top.child(widgets::button("open-docket", "Open Docket", false, true, k, move |s: &mut Main, _| s.open_window(&b)));
    let mut chips = row().gap(6.0).center_y().child(theme::mark_chip(theme::status_icon(d.status(), 12.0, k), d.status().label(), k.text2, k));
    if let Some(l) = d.live() {
        chips = chips.child(theme::mark_chip(theme::live_icon(Some(l.state()), 12.0, k), l.state().word(), k.live(Some(l.state())), k));
    }
    // A Review's PR, the Linear issues, Building, as Chips does.
    let pressable = |chip: El, key: String, url: Option<String>| -> El {
        match url {
            Some(u) => chip.key(key).role("link").on_press_tx(move |_: &mut Main, tx| {
                let _ = tx.open_url(&u);
            }),
            None => chip.key(key),
        }
    };
    if let Some(key) = model::text(&d.j["review"]["key"]) {
        chips = chips.child(pressable(theme::mark_chip(icon(&icons::PULL, 11.0).color(k.text2), &key, k.text2, k), "chip-pr".into(), model::text(&d.j["review"]["url"])));
    }
    let linear: Vec<&serde_json::Value> = std::iter::once(&d.j["linear"]).filter(|l| l.is_object()).chain(model::arr(&d.j["linear_also"]).iter()).collect();
    for l in linear {
        let key = model::text(&l["key"]).or_else(|| model::text(&l["identifier"])).unwrap_or_default();
        let chip = theme::mark_chip(icon(&icons::LINEAR, 11.0).color(k.text2), &key, k.text2, k).help(model::text(&l["title"]).unwrap_or_else(|| key.clone()));
        chips = chips.child(pressable(chip, format!("chip-{key}"), model::text(&l["url"])));
    }
    if s.state().is_some_and(|st| st.task_for(&d.id()).is_some()) {
        chips = chips.child(theme::mark_chip(icon(&icons::HAMMER, 11.0).color(k.text2), "Building", k.text2, k));
    }
    if let Some(why) = d.hold() {
        chips = chips.child(theme::mark_chip(icon(&icons::PAUSE, 11.0).color(k.blocked), "On hold", k.blocked, k).help(why));
    }
    if let Some(why) = model::text(&d.j["closed_on"]).filter(|_| d.status() == Status::Done) {
        chips = chips.child(theme::mark_chip(icon(&icons::QUOTE, 11.0).color(k.text2), "On your word", k.text2, k).help(format!("Closed on your word: {why}")));
    }
    let mut c = col().w_full().gap(8.0);
    if embedded {
        c = c.child(top);
    }
    c.child(text(d.title()).font(font(22.0, 600.0).lh(27.0)).color(k.text).wrap().role("heading").level(1)).child(chips)
}

fn prompt_callout(s: &Main, d: &Docket, l: &model::Live, k: &K) -> El {
    let who = model::app_name(d.agent_app().as_deref());
    let line = format!("{who} is waiting on you{}", l.kind().map(|k| format!(" · {k}")).unwrap_or_default());
    let mut body =
        col().gap(4.0).child(text(line).font(font(13.0, 600.0).lh(16.0)).color(k.text)).child(text(l.msg().unwrap_or_default()).font(mono(12.5).lh(16.0)).color(k.text2).wrap());
    if !l.answerable() {
        body = body.child(text("Answer it in its pane: Wheelhouse can't read this prompt's choices.").font(font(11.5, 400.0).lh(15.0)).color(k.muted));
    }
    let mut r = row().w_full().gap(10.0).child(body.grow(1.0).min_w(0.0));
    if l.answerable() {
        r = r.child(lists::answer_buttons(s, d, k));
    }
    theme::callout(LiveState::Blocked, r, k)
}

pub(super) fn message_card(s: &Main, m: &model::Msg, k: &K, kit: &Kit) -> El {
    let mut c = col()
        .w_full()
        .padding(14.0)
        .gap(8.0)
        .radius(10.0)
        .continuous_corners()
        .fill(k.warning_tint)
        .border(1.0, k.blocked.alpha(0.25))
        .child(
            row()
                .gap(8.0)
                .center_y()
                .child(theme::live_icon(Some(LiveState::Blocked), 16.0, k))
                .child(text(format!("{} asks you · {}", m.who(), m.title())).font(font(13.0, 600.0).lh(16.0)).color(k.text)),
        )
        .child(markdown(&m.body(), k, kit));
    if !m.options().is_empty() {
        c = c.child(lists::option_buttons(s, m, k));
    }
    c
}

fn intent(s: &Main, d: &Docket, page: Option<&DocketPage>, k: &K, kit: &Kit) -> El {
    let id = d.id();
    let who = page
        .and_then(|p| p.entries.iter().rev().find(|e| e.kind() == "intent"))
        .map(|e| {
            let by = match model::text(&e.data()["source"]).as_deref() {
                Some("linear") => "Linear".to_string(),
                Some("review") => "Wheelhouse, from the PR".to_string(),
                _ if e.actor_type() == "person" => "You".to_string(),
                _ => model::app_name(Some(&e.actor_app())),
            };
            format!("v{} · {by} · {}", d.intent_versions().max(1), model::ago_long(Some(&e.at())))
        })
        .unwrap_or_default();
    let mut head = row().w_full().gap(8.0).center_y().child(theme::section_title("Intent", k)).child(text(who).font(font(12.0, 400.0).lh(16.0)).color(k.muted)).child(spacer());
    if let Some(edit) = &s.intent_edit {
        let (a, b) = (id.clone(), id.clone());
        let field = text_input(edit, font(13.0, 400.0).lh(19.0))
            .key("intent-editor")
            .w_full()
            .min_h(160.0)
            .color(k.text)
            .placeholder("What this docket is for, how you'll know it's done, and what's out of scope.", k.muted)
            .label("Intent")
            .on_edit(|s: &mut Main| s.intent_edit.get_or_insert_with(|| rune::TextEdit::new("")));
        head = head.child(widgets::button("intent-cancel", "Cancel", false, true, k, |s: &mut Main, _| s.intent_edit = None)).child(widgets::button(
            "intent-save",
            "Save",
            true,
            true,
            k,
            move |s: &mut Main, _| s.save_intent(&a),
        ));
        let _ = b;
        return col().w_full().gap(8.0).child(head).child(row().w_full().padding(10.0).radius(8.0).fill(k.recessed).border(1.0, k.line2).child(field));
    }
    // The state carries only an intent's start; its page has the whole of it.
    let body = page.map(|p| p.docket.intent_body()).unwrap_or_else(|| d.intent_body());
    let start = body.clone().unwrap_or_default();
    head = head.child(
        row()
            .key("intent-edit")
            .gap(4.0)
            .center_y()
            .role("button")
            .label("Edit the intent")
            .hover(k.hover)
            .px(6.0)
            .py(2.0)
            .radius(5.0)
            .on_press(move |s: &mut Main| {
                let mut e = rune::TextEdit::new(start.as_str());
                e.focused = true;
                s.intent_edit = Some(e);
            })
            .child(icon(&icons::PENCIL, 11.0).color(k.muted))
            .child(text("Edit").font(font(12.0, 400.0).lh(15.0)).color(k.muted)),
    );
    let content = match body {
        Some(b) => intent_markdown(&id, &b, k, kit),
        // Intent.swift's empty state: a dashed box that opens the editor.
        None => col()
            .key("intent-empty")
            .w_full()
            .padding(14.0)
            .radius(10.0)
            .border(1.0, k.line2)
            .border_dash(4.0, 3.0)
            .role("button")
            .label("Write the intent")
            .on_press(|s: &mut Main| {
                let mut e = rune::TextEdit::new("");
                e.focused = true;
                s.intent_edit = Some(e);
            })
            .child(
                parts::Rich::new()
                    .push("Write the intent: ", |sp| sp.color(k.muted).weight(700.0))
                    .md("what this docket is for, how you'll know it's done, and what's out of scope. Agents read it with `wh brief`.", k.muted)
                    .el()
                    .font(font(12.5, 400.0).lh(17.0))
                    .wrap()
                    .w_full(),
            ),
    };
    col().w_full().gap(8.0).child(head).child(content)
}

fn decisions(p: &DocketPage, k: &K) -> El {
    let n = p.decisions().iter().filter(|d| model::text(&d["answer"]).is_some()).count();
    let mut c = col()
        .w_full()
        .gap(6.0)
        .child(row().gap(8.0).center_y().child(theme::section_title("Decisions", k)).child(text(format!("{n} in force")).font(font(12.0, 400.0).lh(16.0)).color(k.muted)));
    for d in p.decisions() {
        let answered = model::text(&d["answer"]);
        let mut line = row()
            .gap(8.0)
            .center_y()
            .child(icon(if answered.is_some() { &icons::TICK } else { &icons::QUOTE }, 13.0).color(if answered.is_some() { k.working } else { k.blocked }))
            .child(text(model::str_or(&d["question"], "").to_string()).font(font(13.0, 500.0).lh(17.0)).color(k.text));
        if let Some(a) = &answered {
            line = line.child(text("→").font(font(12.0, 400.0).lh(17.0)).color(k.faint)).child(text(a.clone()).font(font(13.0, 600.0).lh(17.0)).color(k.text));
        }
        line = line.child(text(format!("{} · {}", model::str_or(&d["by"], ""), model::ago_long(d["at"].as_str()))).font(font(11.5, 400.0).lh(17.0)).color(k.faint));
        let mut item = col().gap(2.0).child(line);
        if let Some(note) = model::text(&d["note"]) {
            item = item.child(text(format!("“{note}”")).font(font(12.0, 400.0).lh(16.0)).color(k.muted).pl(21.0));
        }
        c = c.child(item);
    }
    c
}

fn artifacts(files: &[&model::Entry], k: &K) -> El {
    let mut grid = row().w_full().gap(10.0).wrap();
    for e in files {
        let d = e.data();
        let link = model::text(&d["url"]);
        let sub = match &link {
            Some(_) => {
                if model::str_or(&d["provider"], "") == "claude" {
                    "Claude artifact".to_string()
                } else {
                    "Link".to_string()
                }
            }
            None => format!("{} · {} B", model::str_or(&d["name"], ""), model::int(&d["size"]).unwrap_or(0)),
        };
        let title = e.body().unwrap_or_else(|| model::str_or(&d["name"], "").to_string());
        grid = grid.child(
            row()
                .w(250.0)
                .px(12.0)
                .py(10.0)
                .gap(10.0)
                .radius(10.0)
                .continuous_corners()
                .fill(k.elev)
                .border(1.0, k.line2)
                .hover(k.hover)
                .center_y()
                .role("button")
                .label(title.clone())
                .child(row().size(28.0, 28.0).radius(6.0).fill(k.recessed).center().child(icon(if link.is_some() { &icons::LINK } else { &icons::DOC }, 14.0).color(k.muted)))
                .child(
                    col()
                        .gap(1.0)
                        .shrink()
                        .min_w(0.0)
                        .child(text(title).font(font(12.5, 500.0).lh(16.0)).color(k.text).truncate())
                        .child(text(sub).font(font(11.5, 400.0).lh(14.0)).color(k.muted).truncate()),
                ),
        );
    }
    col()
        .w_full()
        .gap(8.0)
        .child(
            row()
                .gap(8.0)
                .center_y()
                .child(theme::section_title("Artifacts", k))
                .child(text(format!("{} kept · each pinned in the record by its hash", files.len())).font(font(12.0, 400.0).lh(16.0)).color(k.muted)),
        )
        .child(grid)
}

/// The note box: a note on the record, or (in Chat) a message to the agent.
fn composer(s: &Main, d: &Docket, to_agent: bool, k: &K) -> El {
    let id = d.id();
    let (a, b) = (id.clone(), id.clone());
    let field = text_input(&s.note, font(13.0, 400.0).lh(18.0))
        .key("composer")
        .w_full()
        .min_h(54.0)
        .color(k.text)
        .placeholder(
            if to_agent {
                format!("Message {}… (Return sends; it waits for the turn to end)", model::app_name(d.agent_app().as_deref()))
            } else {
                "Leave a note… (Markdown)".to_string()
            },
            k.muted,
        )
        .label(if to_agent { "Message the agent" } else { "Leave a note" })
        .on_edit(|s: &mut Main| &mut s.note)
        .on_submit(move |s: &mut Main, _| if to_agent { s.send(&a) } else { s.note(&a) });
    let ready = !s.note.text.trim().is_empty();
    col().w_full().gap(8.0).padding(12.0).radius(10.0).continuous_corners().border(1.0, k.line2).fill(k.elev).child(field).child(
        row()
            .w_full()
            .center_y()
            .child(text(if to_agent { "Sent through Rex, and kept on the record." } else { "Signed as you" }).font(font(11.5, 400.0).lh(15.0)).color(k.faint))
            .child(spacer())
            .child(widgets::button(
                "composer-send",
                if to_agent { "Send  ⌘↩" } else { "Note  ⌘↩" },
                true,
                ready,
                k,
                move |s: &mut Main, _| if to_agent { s.send(&b) } else { s.note(&b) },
            )),
    )
}

// ---- the inspector ----

pub fn inspector(s: &Main, st: &HubState, id: &str, k: &K) -> El {
    let Some(d) = st.docket(id) else { return col() };
    let page = s.pages.get(id);
    let mut c = col().key("inspector").w_full().px(16.0).pt(14.0).pb(16.0).gap(0.0);
    // A menu-style picker, as SwiftUI's: the current status sits over the value.
    let (a, cur) = (id.to_string(), d.status());
    let status = row()
        .key("status-picker")
        .gap(6.0)
        .px(4.0)
        .py(1.0)
        .radius(5.0)
        .center_y()
        .hover(k.hover)
        .role("combobox")
        .label(format!("Status: {}", cur.label()))
        .on_press_tx(move |_: &mut Main, tx| {
            let items: Vec<(&str, &str)> = Status::ALL.iter().map(|s| (s.raw(), s.label())).collect();
            let menu = rune::PopupMenu::picker(items, cur.raw());
            let a = a.clone();
            tx.popup_menu(menu, "status-picker", move |s: &mut Main, picked, _| {
                if let Some(p) = picked.filter(|p| *p != cur.raw()) {
                    s.set_status(&a, Status::parse(p));
                }
            });
        })
        .child(theme::status_icon(cur, 13.0, k))
        .child(widgets::value_text(cur.label(), k))
        .child(icon(&icons::CHEVRON, 10.0).color(k.muted));
    c = c.child(widgets::field("Status", status, k));
    let proto = d.protocol();
    let task = st.task_for(id);
    let link = |key: &str, label: &str, on: Box<dyn Fn(&mut Main, &mut rune::Tx)>| parts::link(key.to_string(), label, 12.5, k, move |s, tx| on(s, tx));
    // A Review docket's PR, its round and verdict.
    let r = &d.j["review"];
    if let Some(key) = model::text(&r["key"]) {
        let mut line = key;
        if let Some(n) = model::int(&r["round"]) {
            line.push_str(&format!(" · round {n}"));
        }
        if let Some(v) = model::text(&r["verdict"]) {
            line.push_str(&format!(" · {}", v.replace('_', " ").to_lowercase()));
        }
        let url = model::text(&r["url"]);
        let review = row()
            .key("inspector-review")
            .role("link")
            .label(line.clone())
            .on_press_tx(move |_: &mut Main, tx| {
                if let Some(u) = &url {
                    let _ = tx.open_url(u);
                }
            })
            .child(text(line).font(font(12.5, 400.0).lh(16.0)).color(k.accent).wrap().grow(1.0).min_w(0.0));
        c = c.child(widgets::field("Review", review, k));
    }
    if proto != "review" {
        let build = if let Some(t) = task {
            widgets::value_text(lists::build_phase(t), k)
        } else if d.intent_body().is_some_and(|b| !b.trim().is_empty()) {
            let repos: Vec<String> = model::arr(&st.j["build"]["config"]["repos"]).iter().filter_map(|r| r.as_str().map(str::to_string)).collect();
            match repos.as_slice() {
                [] => text("No repos in build.lua").font(font(12.5, 400.0).lh(16.0)).color(k.faint),
                [repo] => {
                    let (a, repo) = (id.to_string(), repo.clone());
                    link("build-it", "Build it", Box::new(move |s, _| s.build(&a, &repo))).help(format!("Give this docket's intent to a builder and a checker, into {}", repos[0]))
                }
                _ => {
                    let (a, list) = (id.to_string(), repos.clone());
                    link(
                        "build-it",
                        "Build it…",
                        Box::new(move |_, tx| {
                            let mut m = rune::PopupMenu::new();
                            for r in &list {
                                m = m.item(rune::PopupItem::new(r.as_str(), r.as_str()));
                            }
                            let a = a.clone();
                            tx.popup_menu(m, "build-it", move |s: &mut Main, picked, _| {
                                if let Some(r) = picked {
                                    s.build(&a, r);
                                }
                            });
                        }),
                    )
                }
            }
        } else {
            text("Needs an intent").font(font(12.5, 400.0).lh(16.0)).color(k.faint).help("Write the intent first: what this is for, and how you'll know it's done")
        };
        c = c.child(widgets::field("Build", build, k));
    }
    let a = id.to_string();
    c = c.child(widgets::field(
        "Linear",
        match (d.linear_key(), model::text(&d.j["linear"]["url"])) {
            (Some(key), url) => link(
                "inspector-linear",
                &key,
                Box::new(move |_, tx| {
                    if let Some(u) = &url {
                        let _ = tx.open_url(u);
                    }
                }),
            ),
            (None, _) => link("link-issue", "Link issue…", Box::new(move |s, _| s.open_form(Sheet::LinkIssue(a.clone())))),
        },
        k,
    ));
    let can_start = d.is_open() && proto != "review" && d.live().is_none();
    if !d.has_agent() && task.is_none() && can_start {
        let a = id.to_string();
        c = c.child(widgets::field("Agent", link("start-agent", "Start an agent…", Box::new(move |s, _| s.open_form(Sheet::StartAgent(a.clone())))), k));
    }
    if d.has_agent() && proto != "review" && task.is_none() {
        c = c.child(widgets::field(
            "Agent",
            row().gap(6.0).center_y().child(theme::app_avatar(d.agent_app().as_deref(), 15.0, k)).child(widgets::value_text(model::app_name(d.agent_app().as_deref()), k)),
            k,
        ));
        let now = match d.live() {
            Some(l) => format!("{}{}", l.state().word(), l.since().map(|s| format!(" · {}", model::ago(Some(&s)))).unwrap_or_default()),
            None => model::text(&d.j["released"]).map(|r| format!("Ended · {r}")).unwrap_or_else(|| "Not running".into()),
        };
        let mut now_row =
            row().gap(6.0).center_y().child(theme::live_icon(Some(d.live().map(|l| l.state()).unwrap_or(LiveState::Idle)), 12.0, k)).child(widgets::value_text(now, k));
        if can_start {
            let a = id.to_string();
            now_row = now_row.child(link("start-again", "Start one…", Box::new(move |s, _| s.open_form(Sheet::StartAgent(a.clone())))));
        }
        c = c.child(widgets::field("Now", now_row, k));
        let sess = d.rex_session().unwrap_or_default();
        let pane = model::text(&d.j["agent"]["pane"]).unwrap_or_else(|| "pane".into());
        let a = id.to_string();
        let live = d.live().is_some();
        c = c.child(widgets::field(
            "Rex",
            row()
                .gap(6.0)
                .center_y()
                .role("button")
                .label("Show the pane in Rex")
                .on_press(move |s: &mut Main| {
                    if live {
                        s.open_in_rex(&a)
                    }
                })
                .child(theme::session_icon(&sess, 14.0, k))
                .child(widgets::value_text(format!("{sess} / {pane}"), k)),
            k,
        ));
        if let Some(cwd) = d.agent_cwd() {
            let short = cwd.replace(&std::env::var("HOME").unwrap_or_default(), "~");
            c = c.child(widgets::field(
                "Folder",
                row()
                    .role("button")
                    .label(format!("Open {short} in Finder"))
                    .help(format!("{cwd}\nClick to open it in Finder"))
                    .on_press_tx(move |_: &mut Main, tx| {
                        let _ = tx.open_url(&format!("file://{cwd}"));
                    })
                    .child(widgets::value_text(short, k)),
                k,
            ));
        }
        if let Some(sess) = d.session() {
            let short: String = sess.chars().take(8).collect();
            c = c.child(widgets::field(
                "Session",
                row()
                    .role("button")
                    .label("Copy the agent's session id")
                    .help("Copy the agent's session id")
                    .on_press_tx(move |s: &mut Main, tx| {
                        tx.copy(&sess);
                        s.toast(format!("Copied {sess}"), false);
                    })
                    .child(text(short).font(mono(12.0).lh(16.0)).color(k.text)),
                k,
            ));
        }
    }
    let protocol = if task.is_some() {
        "Build"
    } else {
        match proto.as_str() {
            "agent" => "Agent",
            "review" => "Review",
            _ => "By hand",
        }
    };
    c = c.child(widgets::field("Protocol", widgets::value_text(protocol, k), k));
    let a = id.to_string();
    c = c.child(widgets::field(
        "Hold",
        match d.hold() {
            Some(why) => row().gap(8.0).center_y().child(text("On hold").font(font(12.5, 400.0).lh(16.0)).color(k.blocked).help(why)).child(link(
                "resume",
                "Resume",
                Box::new(move |s, _| s.resume(&a)),
            )),
            None => link("hold", "Hold…", Box::new(move |s, _| s.sheet = Some(Sheet::Hold(a.clone())))).help("Build and Review take no steps on it until you resume it"),
        },
        k,
    ));
    if proto != "review" && task.is_none() {
        c = c.child(widgets::field(
            "Work",
            widgets::value_text(format!("{} · {}", model::plural(d.prompts() as usize, "prompt"), model::plural(d.turns() as usize, "turn")), k),
            k,
        ));
    }
    c = c.child(widgets::field("Created", widgets::value_text(model::stamp(Some(&d.created_at())), k), k));
    c = c.child(widgets::field("Updated", widgets::value_text(model::ago_long(Some(&d.updated_at())), k), k));
    c = c.child(text("RECORD").font(font(10.5, 600.0).lh(14.0).tracking(0.4)).color(k.muted).pt(18.0).pb(6.0));
    if let Some(p) = page {
        let ok = p.verify_ok();
        let a = id.to_string();
        c = c.child(widgets::field(
            "Check",
            row()
                .gap(5.0)
                .center_y()
                .role("button")
                .label("Open the record")
                .help("Open the record: every signed entry, and what the check means")
                .on_press(move |s: &mut Main| s.sheet = Some(Sheet::Record(a.clone())))
                .child(icon(&icons::SHIELD, 13.0).color(if ok { k.working } else { k.error }))
                .child(text(if ok { format!("Intact · {} signed", p.verify_entries()) } else { "Doesn't check".into() }).font(font(12.5, 400.0).lh(16.0)).color(if ok {
                    k.working
                } else {
                    k.error
                })),
            k,
        ));
    }
    let (head, a) = (d.short_head(), id.to_string());
    let copy = format!("Docket: {a} @ {head}");
    c = c.child(widgets::field(
        "Head",
        row()
            .role("button")
            .label(format!("Copy “{copy}”"))
            .help(format!("Copy “{copy}”, for a PR or a note"))
            .on_press_tx(move |s: &mut Main, tx| {
                tx.copy(&copy);
                s.toast(format!("Copied {copy}"), false);
            })
            .child(text(head).font(mono(12.0).lh(16.0)).color(k.text)),
        k,
    ));
    let (a, b) = (id.to_string(), id.to_string());
    c = c.child(widgets::field(
        "Record",
        row().gap(10.0).child(link("open-record", "Open", Box::new(move |s, _| s.sheet = Some(Sheet::Record(a.clone()))))).child(
            link("export-record", "Export…", Box::new(move |s, tx| record::export(s, &b, tx))).help(format!("Save {id}.jsonl: it checks without the hub (wh verify --file)")),
        ),
        k,
    ));
    let _ = Place::Inbox;
    c.scroll(ScrollSpec { stick_bottom: false })
}

// ---- sheets ----

pub fn sheet(s: &Main, sheet: &Sheet, k: &K, kit: &Kit) -> El {
    let card = match sheet {
        Sheet::Hold(id) => hold_sheet(s, id, k),
        Sheet::NewDocket(mode) => forms::new_docket(s, *mode, k),
        Sheet::StartAgent(id) => forms::start_agent(s, id, k),
        Sheet::LinkIssue(id) => forms::link_issue(s, id, k),
        // Recap's, Chat's, the record's, Verify's and the panels' sheets,
        // centred as the Mac app's are.
        Sheet::Record(id) => return parts::scrim(record::sheet(s, id, k, kit), k),
        Sheet::Shortcuts => return parts::scrim(shortcuts::sheet(s, k, kit), k),
        Sheet::CheckOutput(d, c) => return parts::scrim(verify::output_sheet(s, d, c, k, kit), k),
        Sheet::CloseOnWord(id, from) => return parts::scrim(verify::close_sheet(s, id, *from, k), k),
        Sheet::Pane(id) => return parts::scrim(chat::pane_sheet(s, id, k, kit), k),
        Sheet::Preview(full) => return parts::scrim(panels::preview_sheet(s, full, k, kit), k),
    };
    parts::scrim(card, k)
}

fn hold_sheet(s: &Main, id: &str, k: &K) -> El {
    let a = id.to_string();
    let title = s.state().and_then(|st| st.docket(id).map(|d| d.title())).unwrap_or_default();
    let field = text_input(&s.hold_why, font(13.0, 400.0).lh(18.0))
        .key("hold-why")
        .w_full()
        .min_h(36.0)
        .max_h(72.0)
        .color(k.text)
        .placeholder("Why? It goes on the record, and on the docket.", k.muted)
        .label("Why it's on hold")
        .on_edit(|s: &mut Main| &mut s.hold_why)
        .on_submit({
            let a = a.clone();
            move |s: &mut Main, _| s.hold(&a)
        });
    let busy = s.busy.contains(&format!("hold:{id}"));
    let body = col()
        .w(460.0)
        .padding(20.0)
        .gap(14.0)
        .child(
            row().gap(12.0).child(icon(&icons::PAUSE_CIRCLE, 26.0).color(k.blocked)).child(
                col()
                    .gap(4.0)
                    .min_w(0.0)
                    .child(text(format!("Hold {id}")).font(font(14.0, 600.0).lh(18.0)).color(k.text).role("heading").level(2))
                    .child(text(title).font(font(12.5, 400.0).lh(16.0)).color(k.muted).truncate()),
            ),
        )
        .child(
            text("Build and Review take no steps on it, polls included, until you resume it. Agents in its panes carry on.").font(font(12.5, 400.0).lh(17.0)).color(k.text2).wrap(),
        )
        .child(row().w_full().px(8.0).py(6.0).radius(6.0).fill(k.elev).border(1.0, k.line2).child(field))
        .child(row().w_full().gap(8.0).child(spacer()).child(widgets::button("hold-cancel", "Cancel", false, true, k, |s: &mut Main, _| s.sheet = None)).child(widgets::button(
            "hold-go",
            if busy { "Holding…" } else { "Hold" },
            true,
            !busy,
            k,
            move |s: &mut Main, _| s.hold(&a),
        )));
    parts::sheet_card(&format!("Hold {id}"), 460.0, None, k).child(body)
}

#[cfg(test)]
mod tests {
    use super::parts::{Piece, pieces};

    #[test]
    fn an_alert_is_its_own_piece_and_offsets_stay_the_source_s() {
        let src = "Intro\n\n> [!NOTE]\n> Out of scope.\n\n- [ ] a\n";
        let ps = pieces(src);
        assert_eq!(ps[0], Piece::Md { at: 0, text: "Intro\n\n" });
        assert_eq!(ps[1], Piece::Alert { kind: "NOTE", body: "Out of scope.\n".into() });
        let Piece::Md { at, text } = ps[2] else { panic!("{ps:?}") };
        assert_eq!(&src[at + text.find("[ ]").unwrap()..][..3], "[ ]");
    }

    #[test]
    fn a_fenced_alert_is_code() {
        assert_eq!(pieces("```\n> [!NOTE]\n```\n").len(), 1);
    }
}
