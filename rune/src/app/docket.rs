//! A docket (DocketView.swift): its header, what it's waiting on you for, its
//! intent, decisions, Verify, kept files, activity and the composer; the
//! inspector beside it; and the sheets it opens.

use rune::{Color, El, ScrollSpec, col, el, icon, row, spacer, text, text_input};
use rune_kit::{Kit, Markdown, ScrollBar};

use super::{Main, Place, Sheet, activity, chat, forms, lists, panels, parts, record, shortcuts, verify, widgets};
use crate::icons;
use crate::model::{self, Docket, DocketPage, HubState, LiveState, Status};
use crate::theme::{self, K, font, mono};

/// GitHub's alerts (`> [!NOTE]`) as a quote headed by their kind, since
/// rune-kit's Markdown has no alerts; with where each rewrite moved the text,
/// so an offset in the result maps back to the source.
fn prepared(src: &str) -> (String, Vec<(usize, isize)>) {
    let tags = [("[!NOTE]", "**Note**  "), ("[!TIP]", "**Tip**  "), ("[!IMPORTANT]", "**Important**  "), ("[!WARNING]", "**Warning**  "), ("[!CAUTION]", "**Caution**  ")];
    let (mut out, mut shifts, mut i) = (String::new(), vec![], 0);
    while i < src.len() {
        if let Some((tag, word)) = tags.iter().find(|(tag, _)| src[i..].starts_with(tag)) {
            out.push_str(word);
            shifts.push((out.len(), word.len() as isize - tag.len() as isize));
            i += tag.len();
        } else {
            let c = src[i..].chars().next().unwrap_or(' ');
            out.push(c);
            i += c.len_utf8();
        }
    }
    (out, shifts)
}

fn source_offset(off: usize, shifts: &[(usize, isize)]) -> usize {
    let moved: isize = shifts.iter().filter(|(at, _)| *at <= off).map(|(_, d)| *d).sum();
    (off as isize - moved).max(0) as usize
}

/// Markdown as the app writes it.
pub fn markdown(src: &str, kit: &Kit) -> El {
    Markdown::new(prepared(src).0).selectable(true).build(kit)
}

/// The intent, with its task boxes live: ticking one saves the intent as a
/// new version, as the Mac app does.
fn intent_markdown(id: &str, src: &str, kit: &Kit) -> El {
    let (shown, shifts) = prepared(src);
    let (id, src) = (id.to_string(), src.to_string());
    Markdown::new(shown)
        .selectable(true)
        .on_task(move |s: &mut Main, _: &str, at: usize, checked: bool| {
            let at = source_offset(at, &shifts);
            let mut body = src.clone();
            if body.get(at..at + 1) == Some("[") && body.get(at + 2..at + 3) == Some("]") {
                body.replace_range(at + 1..at + 2, if checked { "x" } else { " " });
                let id = id.clone();
                s.act(&format!("intent:{id}"), format!("/api/dockets/{id}/intent"), serde_json::json!({ "body": body }), move |r| match r {
                    Ok(_) => (format!("{id}: intent saved"), false),
                    Err(e) => (e, true),
                });
            }
        })
        .build(kit)
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
    if let Some(why) = d.hold() {
        chips = chips.child(theme::chip(&format!("On hold: {why}"), k.blocked, false));
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
        .child(markdown(&m.body(), kit));
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
        Some(b) => intent_markdown(&id, &b, kit),
        None => row().w_full().padding(14.0).radius(8.0).border(1.0, k.line2).child(
            text("Write the intent: what this docket is for, how you'll know it's done, and what's out of scope. Agents read it with `wh brief`.")
                .font(font(13.0, 400.0).lh(18.0))
                .color(k.muted)
                .wrap(),
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
            .child(text(if to_agent { "Sent through Rex, and kept on the record." } else { "Signed as you, on the record." }).font(font(11.5, 400.0).lh(15.0)).color(k.faint))
            .child(spacer())
            .child(widgets::button("composer-send", if to_agent { "Send" } else { "Note" }, true, ready, k, move |s: &mut Main, _| if to_agent { s.send(&b) } else { s.note(&b) })),
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
    if let Some(t) = st.task_for(id) {
        c = c.child(widgets::field("Build", widgets::value_text(lists::build_phase(t), k), k));
    } else if let Some(p) = st.pr_for(id) {
        c = c.child(widgets::field("Review", widgets::value_text(format!("{} · round {}", p.key(), p.round()), k), k));
    }
    c = c.child(widgets::field(
        "Linear",
        match d.linear_key() {
            Some(key) => widgets::value_text(format!("{key}{}", d.linear_state().map(|s| format!(" · {s}")).unwrap_or_default()), k),
            None => text("Link issue…").font(font(12.5, 400.0).lh(16.0)).color(k.link),
        },
        k,
    ));
    if d.has_agent() {
        c = c.child(widgets::field(
            "Agent",
            row().gap(6.0).center_y().child(theme::app_avatar(d.agent_app().as_deref(), 14.0, k)).child(widgets::value_text(model::app_name(d.agent_app().as_deref()), k)),
            k,
        ));
    }
    if let Some(l) = d.live() {
        c = c.child(widgets::field(
            "Now",
            row()
                .gap(6.0)
                .center_y()
                .child(theme::live_icon(Some(l.state()), 12.0, k))
                .child(widgets::value_text(format!("{} · {}", l.state().word(), model::ago(l.since().as_deref())), k)),
            k,
        ));
        c = c.child(widgets::field(
            "Rex",
            row().gap(6.0).center_y().child(theme::session_icon(&l.rex_session(), 14.0, k)).child(widgets::value_text(format!("{} / {}", l.rex_session(), l.pane()), k)),
            k,
        ));
    }
    if let Some(cwd) = d.agent_cwd() {
        c = c.child(widgets::field("Folder", widgets::value_text(cwd.replace(&std::env::var("HOME").unwrap_or_default(), "~"), k), k));
    }
    if let Some(sess) = d.session() {
        c = c.child(widgets::field("Session", text(sess.chars().take(8).collect::<String>()).font(mono(12.0).lh(16.0)).color(k.text), k));
    }
    c = c.child(widgets::field(
        "Protocol",
        widgets::value_text(
            match d.protocol().as_str() {
                "agent" => "Agent",
                "review" => "Review",
                "build" => "Build",
                _ => "None",
            },
            k,
        ),
        k,
    ));
    let a = id.to_string();
    c = c.child(widgets::field(
        "Hold",
        match d.hold() {
            Some(why) => row().gap(6.0).child(widgets::value_text(why, k)).child(widgets::button("resume", "Resume", false, true, k, move |s: &mut Main, _| s.resume(&a))),
            None => row()
                .key("hold")
                .role("button")
                .label("Hold…")
                .on_press(move |s: &mut Main| s.sheet = Some(Sheet::Hold(a.clone())))
                .child(text("Hold…").font(font(12.5, 400.0).lh(16.0)).color(k.link)),
        },
        k,
    ));
    if d.prompts() + d.turns() > 0 {
        c = c.child(widgets::field(
            "Work",
            widgets::value_text(format!("{} · {}", model::plural(d.prompts() as usize, "prompt"), model::plural(d.turns() as usize, "turn")), k),
            k,
        ));
    }
    c = c.child(widgets::field("Created", widgets::value_text(model::stamp(Some(&d.created_at())), k), k));
    c = c.child(widgets::field("Updated", widgets::value_text(model::ago_long(Some(&d.updated_at())), k), k));
    c = c.child(el().h(12.0)).child(theme::hairline(k)).child(el().h(10.0));
    c = c.child(text("RECORD").font(font(10.5, 600.0).lh(14.0).tracking(0.4)).color(k.muted)).child(el().h(4.0));
    if let Some(p) = page {
        let ok = p.verify_ok();
        c = c.child(widgets::field(
            "Check",
            row().gap(5.0).center_y().child(icon(&icons::SHIELD, 13.0).color(if ok { k.working } else { k.error })).child(
                text(format!("{} · {} signed", if ok { "Intact" } else { "Broken" }, p.verify_entries())).font(font(12.5, 400.0).lh(16.0)).color(if ok {
                    k.working
                } else {
                    k.error
                }),
            ),
            k,
        ));
    }
    c = c.child(widgets::field("Head", text(d.short_head()).font(mono(12.0).lh(16.0)).color(k.text), k));
    let a = id.to_string();
    c = c.child(widgets::field(
        "Record",
        row()
            .key("open-record")
            .role("button")
            .label("Open the record")
            .on_press(move |s: &mut Main| s.sheet = Some(Sheet::Record(a.clone())))
            .child(text("Open").font(font(12.5, 400.0).lh(16.0)).color(k.link)),
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
    col()
        .key("sheet-scrim")
        .raise()
        .absolute(0.0, 0.0)
        .w_full()
        .h_full()
        .fill(Color::BLACK.alpha(if k.dark { 0.45 } else { 0.18 }))
        .items_center()
        .pt(60.0)
        .modal(true, |s: &mut Main| s.sheet = None)
        .child(card)
}

fn sheet_card(title: &str, body: El, k: &K) -> El {
    col()
        .key("sheet")
        .w(560.0)
        .max_h(640.0)
        .radius(12.0)
        .continuous_corners()
        .fill(if k.dark { theme::hex("#1f1f21") } else { Color::WHITE })
        .border(1.0, k.line2)
        .box_shadow(rune::Shadow::core_animation(0.0, 12.0, 28.0, Color::BLACK.alpha(0.3)))
        .role("dialog")
        .label(title.to_string())
        .child(row().w_full().px(18.0).py(14.0).child(text(title.to_string()).font(font(14.0, 600.0).lh(18.0)).color(k.text)))
        .child(theme::hairline(k))
        .child(body)
}

fn hold_sheet(s: &Main, id: &str, k: &K) -> El {
    let a = id.to_string();
    let field = text_input(&s.hold_why, font(13.0, 400.0).lh(18.0))
        .key("hold-why")
        .single_line()
        .w_full()
        .color(k.text)
        .placeholder("Why? (optional: Build and Review read it)", k.muted)
        .label("Why it's on hold")
        .on_edit(|s: &mut Main| &mut s.hold_why)
        .on_submit({
            let a = a.clone();
            move |s: &mut Main, _| s.hold(&a)
        });
    let body = col()
        .w_full()
        .padding(18.0)
        .gap(12.0)
        .child(
            text("On hold, Build and Review take no steps on it, and it shows in the Inbox's Attention until you resume it.")
                .font(font(12.5, 400.0).lh(17.0))
                .color(k.muted)
                .wrap(),
        )
        .child(row().w_full().px(10.0).py(7.0).radius(7.0).fill(k.recessed).border(1.0, k.line2).child(field))
        .child(row().w_full().gap(8.0).child(spacer()).child(widgets::button("hold-cancel", "Cancel", false, true, k, |s: &mut Main, _| s.sheet = None)).child(widgets::button(
            "hold-go",
            "Hold",
            true,
            true,
            k,
            move |s: &mut Main, _| s.hold(&a),
        )));
    sheet_card(&format!("Hold {id}"), body, k)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_task_box_maps_back_past_an_alert() {
        let src = "> [!NOTE]\n> x\n\n- [ ] a";
        let (shown, shifts) = prepared(src);
        let at = shown.find("[ ]").unwrap();
        assert_eq!(&src[source_offset(at, &shifts)..][..3], "[ ]");
    }
}
