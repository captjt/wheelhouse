//! Guides & Briefs (`GuidesSettings`, `DocRow`, `DocEditor`, `DiffView`,
//! `NewGuideSheet`): the guides agents read and the briefs they're told, to
//! edit, compare with the built-in, save, reset or set aside.

use rune::{Color, El, ScrollSpec, TextEdit, col, edit_access, el, icon, row, spacer, text, text_input};
use rune_kit::{Kit, ScrollBar};
use serde_json::json;

use super::patch::{self, Op};
use super::ui::{self, Look};
use super::{Doc, Settings, Sheet, glyphs};
use crate::app::widgets;
use crate::model;
use crate::theme::{K, font, mono};

pub fn view(s: &Settings, k: &K, kit: &Kit) -> El {
    let docs = s.docs();
    let groups: Vec<(&str, Vec<&Doc>)> = vec![
        ("Your guides", docs.iter().filter(|d| d.kind() == "guide" && d.yours()).collect()),
        ("Built-in guides", docs.iter().filter(|d| d.kind() == "guide" && !d.yours()).collect()),
        ("Build's briefs", docs.iter().filter(|d| d.kind() == "brief" && d.proto() == "build").collect()),
        ("Review's briefs", docs.iter().filter(|d| d.kind() == "brief" && d.proto() == "review").collect()),
    ];
    let current = s.current_doc();
    let mut list = col().key("guides-list-rows").w_full().px(8.0).pt(6.0).pb(10.0).gap(1.0).role("listbox").label("Guides and briefs").focus_group();
    for (title, items) in groups.into_iter().filter(|g| !g.1.is_empty()) {
        list = list.child(row().px(8.0).pt(10.0).pb(4.0).child(text(title).font(font(11.0, 600.0).lh(14.0)).color(k.muted).role("heading").level(3)));
        for d in items {
            list = list.child(doc_row(s, d, current.as_ref().is_some_and(|c| c.id() == d.id()), k));
        }
    }
    let side = col()
        .key("guides-side")
        .w(300.0)
        .h_full()
        .child(col().key("guides-list").w_full().grow(1.0).min_h(0.0).child(list).scroll(ScrollSpec { stick_bottom: false }).scroll_bar(kit))
        .child(el().w_full().h(1.0).fill(k.line).a11y_hidden())
        .child(row().w_full().px(10.0).py(7.0).child(ui::tool_button(
            "new-guide",
            &glyphs::PLUS,
            "New Guide…",
            "Write a guide of your own: for a repo's reviews, say",
            k,
            |s: &mut Settings, _| {
                let mut title = TextEdit::new("");
                title.focused = true;
                s.sheet = Some(Sheet::NewGuide { title, error: None });
            },
        )));
    let detail = match &current {
        Some(d) => editor(s, d, k, kit),
        None if s.view.is_some() => widgets::empty(&glyphs::BOOK, "No guides", "", k),
        None => ui::loading(k),
    };
    row()
        .key("guides")
        .w_full()
        .h_full()
        .child(side)
        .child(el().w(1.0).h_full().fill(k.line).a11y_hidden())
        .child(col().key("guide-detail").grow(1.0).min_w(0.0).h_full().fill(k.main).instant_changes().child(detail))
}

/// Unsaved changes in a doc's editor.
fn dirty(s: &Settings, id: &str) -> bool {
    match (s.texts.get(id), s.editors.get(id)) {
        (Some(t), Some(e)) => e.text != t.saved,
        _ => false,
    }
}

fn select(s: &mut Settings, id: String) {
    if s.guide.as_deref() != Some(id.as_str()) {
        s.compare = false;
    }
    s.guide = Some(id.clone());
    s.load_doc(&id);
}

fn doc_row(s: &Settings, d: &Doc, selected: bool, k: &K) -> El {
    let id = d.id();
    let mut top = row().w_full().gap(6.0).center_y().child(text(d.title()).font(font(13.0, 500.0).lh(17.0)).color(k.text).truncate().shrink());
    if dirty(s, &id) {
        top = top.child(el().size(6.0, 6.0).radius(3.0).fill(k.accent).help("Unsaved changes").label("Unsaved changes"));
    }
    top = top.child(spacer());
    if d.kind() == "brief" || !d.yours() {
        top = top.child(text(d.origin()).font(font(10.5, 500.0).lh(13.0)).color(if d.overridden() { k.blocked } else { k.muted }));
    }
    let mut c = col()
        .key(ui::key_of("doc", &id))
        .w_full()
        .px(8.0)
        .py(6.0)
        .gap(3.0)
        .radius(6.0)
        .fill(if selected { if k.dark { Color::WHITE.alpha(0.08) } else { Color::BLACK.alpha(0.06) } } else { Color::TRANSPARENT })
        .hover(if selected { Color::TRANSPARENT } else { k.hover })
        .role("option")
        .label(d.title())
        .focus_selected(selected)
        .focusable(true)
        .on_press(move |s: &mut Settings| select(s, id.clone()))
        .child(top);
    if let Some(a) = d.about() {
        c = c.child(ui::inline(&a, font(11.5, 400.0).lh(15.0), k.muted, k).max_lines(2));
    }
    let used = d.used_by();
    if !used.is_empty() {
        c = c.child(text(format!("Used by {}", used.iter().map(|u| patch::used_by(u)).collect::<Vec<_>>().join(", "))).font(font(11.0, 400.0).lh(14.0)).color(k.accent).truncate());
    }
    c
}

/// One guide or brief: edit it, compare it with the built-in, save, reset (`DocEditor`).
fn editor(s: &Settings, d: &Doc, k: &K, kit: &Kit) -> El {
    let id = d.id();
    let loaded = s.texts.get(&id);
    let original = loaded.and_then(|t| t.original.clone());
    let draft = s.editors.get(&id).map(|e| e.text.clone());
    let saved = loaded.map(|t| t.saved.clone()).unwrap_or_default();
    let now = draft.clone().unwrap_or_else(|| saved.clone());
    let is_dirty = dirty(s, &id);
    let busy = s.is_busy(&format!("doc:{id}"));

    // Its title, what it's for, where it lives, and who uses it.
    let mut title = row()
        .w_full()
        .gap(8.0)
        .center_y()
        .child(text(d.title()).font(font(15.0, 600.0).lh(19.0)).color(k.text).role("heading").level(2))
        .child(ui::chip(
            d.origin(),
            if d.overridden() {
                k.blocked
            } else if d.yours() {
                k.accent
            } else {
                k.muted
            },
        ))
        .child(spacer());
    if original.is_some() {
        title = title.child(ui::segmented("doc-mode", "Edit or compare", &[("Edit", !s.compare), ("Compare with Built-in", s.compare)], true, k, |s: &mut Settings, i| {
            s.compare = i == 1;
            if s.compare {
                reveal_first_change(s);
            }
        }));
    }
    let mut head = col().w_full().px(18.0).pt(16.0).pb(12.0).gap(6.0).child(title);
    if let Some(a) = d.about() {
        head = head.child(ui::inline(&a, font(12.5, 400.0).lh(17.0), k.text2, k));
    }
    head = head.child(text(d.path()).font(mono(11.0).lh(15.0)).color(k.muted).selectable(true));
    let used = d.used_by();
    if !used.is_empty() {
        head = head.child(ui::words(format!("Used by {}.", used.iter().map(|u| patch::used_by(u)).collect::<Vec<_>>().join(", ")), 11.5, k.muted));
    }
    if d.kind() == "brief" {
        head = head.child(
            row()
                .w_full()
                .gap(8.0)
                .padding(10.0)
                .radius(8.0)
                .continuous_corners()
                .fill(k.warning_tint)
                .items_start()
                .child(el().pt(1.0).child(icon(&glyphs::CURLY, 13.0).color(k.blocked)))
                .child(
                    ui::inline(
                        "A brief is what agents are told. Its `{{VARIABLES}}` are filled in by Wheelhouse when an agent starts, so keep them. A broken brief changes what every agent is told: compare with the built-in before you save.",
                        font(12.0, 400.0).lh(16.0),
                        k.text2,
                        k,
                    )
                    .grow(1.0)
                    .min_w(0.0),
                ),
        );
    }

    // The text, or how it differs from the built-in.
    let body = if loaded.is_none() && !s.doc_errors.contains_key(&id) {
        ui::loading(k)
    } else if let (true, Some(orig)) = (s.compare, &original) {
        diff(orig, &now, k, kit)
    } else {
        let eid = id.clone();
        let field = text_input(s.editors.get(&id).unwrap_or(&TextEdit::new("")), mono(12.5).lh(18.0))
            .key("doc-editor")
            .w_full()
            .min_h(240.0)
            .color(k.text)
            .label(format!("{}'s text", d.title()))
            .on_edit(edit_access(move |s: &mut Settings| s.editor_mut(&eid)))
            // Return breaks the line: ⌘S saves.
            .on_submit({
                let id = id.clone();
                move |s: &mut Settings, _| s.editor_mut(&id).insert("\n")
            });
        col().key("doc-editor-scroll").w_full().h_full().child(col().w_full().px(14.0).py(10.0).child(field)).scroll(ScrollSpec { stick_bottom: false }).scroll_bar(kit)
    };

    // What's wrong, where it stands, and what you can do.
    let mut foot = col().w_full().px(18.0).py(12.0).gap(6.0);
    if let (Some(orig), "brief") = (&original, d.kind().as_str()) {
        let gone = patch::dropped(orig, &now);
        if !gone.is_empty() {
            foot = foot.child(ui::alert(&format!("This leaves out {}, which the built-in uses: agents won't be told what it fills in.", gone.join(", ")), k.blocked, k));
        }
    }
    if let Some(e) = s.doc_errors.get(&id) {
        foot = foot.child(ui::alert(e, k.error, k));
    }
    let status = if is_dirty {
        "Unsaved changes".to_string()
    } else if d.yours() {
        "Your guide.".into()
    } else if d.overridden() {
        "Edited: yours replaces the built-in.".into()
    } else {
        format!("Built in. Saving makes it yours, at {}.", d.path())
    };
    let mut bar = row().w_full().gap(8.0).center_y().child(text(status).font(font(11.5, 400.0).lh(15.0)).color(k.muted).truncate().shrink()).child(spacer());
    let rid = id.clone();
    if d.builtin() && d.overridden() {
        bar = bar.child(ui::button("doc-reset", "Reset to Built-in…", Look::Plain, !busy, k, move |s: &mut Settings, _| s.sheet = Some(Sheet::Reset(rid.clone()))));
    } else if d.yours() {
        let help = if used.is_empty() {
            "Move it out of your guides; it's kept in the history".to_string()
        } else {
            format!("It's still used by {}: point those elsewhere first", used.iter().map(|u| patch::used_by(u)).collect::<Vec<_>>().join(", "))
        };
        bar = bar.child(
            ui::button("doc-aside", "Set Aside…", Look::Plain, !busy && used.is_empty(), k, move |s: &mut Settings, _| s.sheet = Some(Sheet::Reset(rid.clone()))).help(help),
        );
    }
    if is_dirty {
        let (vid, back) = (id.clone(), saved.clone());
        bar = bar.child(ui::button("doc-revert", "Revert", Look::Plain, true, k, move |s: &mut Settings, _| s.editor_mut(&vid).replace_text(back.clone())));
    }
    let sid = id.clone();
    bar = bar
        .child(ui::button("doc-save", if busy { "Saving…" } else { "Save" }, Look::Primary, is_dirty && !busy, k, move |s: &mut Settings, _| s.save_doc(&sid)).help("Save (⌘S)"));
    foot = foot.child(bar);

    col()
        .key(ui::key_of("doc-editor", &id))
        .w_full()
        .h_full()
        .child(head)
        .child(el().w_full().h(1.0).fill(k.line).a11y_hidden())
        .child(col().w_full().grow(1.0).min_h(0.0).child(body))
        .child(el().w_full().h(1.0).fill(k.line).a11y_hidden())
        .child(foot)
}

/// A line diff, wrapped: lines only, nothing sideways (`DiffView`).
fn diff(original: &str, now: &str, k: &K, kit: &Kit) -> El {
    let c = col().key("diff").w_full().h_full();
    if original == now {
        return c.child(row().w_full().padding(18.0).child(ui::words("Same as the built-in.", 12.5, k.muted))).scroll(ScrollSpec { stick_bottom: false }).scroll_bar(kit);
    }
    let mut lines = col().w_full().padding(14.0).role("list").label("Changes from the built-in");
    for (i, (op, line)) in patch::line_diff(original, now).into_iter().enumerate() {
        let (mark, ink, fill) = match op {
            Op::Add => ("+", k.working, k.success_tint),
            Op::Del => ("−", k.error, k.danger_tint),
            Op::Same => (" ", k.faint, Color::TRANSPARENT),
        };
        lines = lines.child(
            row()
                .key(format!("diff-{i}"))
                .w_full()
                .px(10.0)
                .py(1.5)
                .gap(8.0)
                .items_start()
                .fill(fill)
                .label(match op {
                    Op::Add => format!("Added: {line}"),
                    Op::Del => format!("Removed: {line}"),
                    Op::Same => line.clone(),
                })
                .child(text(mark).font(mono(12.0).lh(16.0)).color(ink).a11y_hidden())
                .child(
                    text(if line.is_empty() { " ".to_string() } else { line })
                        .font(mono(12.0).lh(16.0))
                        .color(if op == Op::Same { k.muted } else { k.text })
                        .wrap_anywhere()
                        .selectable(true)
                        .grow(1.0)
                        .min_w(0.0),
                ),
        );
    }
    c.child(lines).scroll(ScrollSpec { stick_bottom: false }).scroll_bar(kit)
}

/// To the first change, with a few lines before it.
fn reveal_first_change(s: &mut Settings) {
    let Some(d) = s.current_doc() else { return };
    let id = d.id();
    let (Some(t), Some(e)) = (s.texts.get(&id), s.editors.get(&id)) else { return };
    let Some(orig) = &t.original else { return };
    let first = patch::line_diff(orig, &e.text).iter().position(|(op, _)| *op != Op::Same);
    if let Some(first) = first.filter(|f| *f > 6) {
        s.reveal = Some((format!("diff-{}", first - 4), 8.0));
    }
}

/// A made-up edit, to show the comparison: `remove` taken out of the doc's text.
pub fn demo_edit(s: &mut Settings, id: &str, remove: &str) {
    let Some(saved) = s.texts.get(id).map(|t| t.saved.clone()) else { return };
    s.editor_mut(id).replace_text(saved.replace(remove, ""));
    s.compare = true;
    reveal_first_change(s);
}

// ---- sheets ----

/// Reset a doc to the built-in, or set a guide of yours aside: a confirmation.
pub fn confirm_reset(s: &Settings, id: &str, k: &K) -> El {
    let Some(d) = s.doc(id) else { return col() };
    let yours = d.yours();
    let title = if yours { format!("Set aside “{}”?", d.title()) } else { format!("Reset “{}” to the built-in?", d.title()) };
    let message = format!("Your text is kept in {}.{}", s.history(), if yours { "" } else { " Agents get the built-in from now on." });
    let rid = id.to_string();
    let buttons = vec![
        ui::button("reset-cancel", "Cancel", Look::Plain, true, k, |s: &mut Settings, _| s.sheet = None),
        ui::button("reset-go", if yours { "Set Aside" } else { "Reset" }, Look::Danger, true, k, move |s: &mut Settings, _| s.reset_doc(&rid)).default_button(),
    ];
    ui::sheet("reset-confirm", &title, 400.0, vec![ui::words(message, 12.5, k.text2)], None, buttons, k)
}

fn new_title(s: &mut Settings) -> &mut TextEdit {
    match &mut s.sheet {
        Some(Sheet::NewGuide { title, .. }) => title,
        _ => &mut s.scratch,
    }
}

/// A guide of your own: its name, and a first line to start from (`NewGuideSheet`).
pub fn new_guide(s: &Settings, sheet: &Sheet, k: &K) -> El {
    let Sheet::NewGuide { title, error } = sheet else { return col() };
    let file = patch::guide_file(&title.text);
    let taken = file.as_ref().is_some_and(|f| s.docs().iter().any(|d| d.id() == format!("guide/{f}")));
    let busy = s.is_busy("new-guide");
    let field = text_input(title, ui::field_font(13.0, false))
        .key("new-guide-name")
        .single_line()
        .w_full()
        .color(k.text)
        .placeholder("API review", k.faint)
        .label("Name")
        .on_edit(edit_access(new_title))
        .on_submit(|s: &mut Settings, _| create(s));
    let dir = {
        let f = model::str_or(&s.v()["file"], "~/.config/wheelhouse/settings.json").to_string();
        f.rsplit_once('/').map(|(d, _)| d.to_string()).unwrap_or(f)
    };
    let (where_, monospace) = match &file {
        Some(f) if taken => (format!("There's a guide called {f} already."), true),
        Some(f) => (format!("Saved as {dir}/guides/{f}"), true),
        None => ("Letters and digits; the file name is made from them.".to_string(), false),
    };
    let mut body = vec![
        ui::words("A guide is Markdown agents read and follow: how to review a repo, say. Pick it as a repo's review guide in Repos.", 12.5, k.text2),
        ui::field_box(field, k),
        text(where_).font(if monospace { mono(11.5).lh(15.0) } else { font(11.5, 400.0).lh(15.0) }).color(if taken { k.error } else { k.muted }).wrap_anywhere(),
    ];
    if let Some(e) = error {
        body.push(text(e.clone()).font(font(11.5, 400.0).lh(15.0)).color(k.error).wrap());
    }
    let buttons = vec![
        ui::button("new-guide-cancel", "Cancel", Look::Plain, true, k, |s: &mut Settings, _| s.sheet = None),
        ui::button("new-guide-create", if busy { "Creating…" } else { "Create Guide" }, Look::Primary, file.is_some() && !taken && !busy, k, |s: &mut Settings, _| create(s))
            .default_button(),
    ];
    ui::sheet("new-guide", "New guide", 460.0, body, None, buttons, k)
}

fn create(s: &mut Settings) {
    let Some(Sheet::NewGuide { title, .. }) = &s.sheet else { return };
    let name = title.text.trim().to_string();
    let Some(file) = patch::guide_file(&name) else { return };
    if s.docs().iter().any(|d| d.id() == format!("guide/{file}")) {
        return;
    }
    let id = format!("guide/{file}");
    let body = json!({ "id": id, "text": format!("# {name}\n\nWhat agents should look for, and how.\n") });
    s.call(
        "new-guide",
        move |st| st.post("/api/settings/doc", body),
        move |s, r| match r {
            Ok(v) => {
                let made = model::str_or(&v["id"], &id).to_string();
                s.sheet = None;
                s.guide = Some(made.clone());
                s.compare = false;
                s.load();
                s.load_doc(&made);
            }
            Err(e) => {
                if let Some(Sheet::NewGuide { error, .. }) = &mut s.sheet {
                    *error = Some(e);
                }
            }
        },
    );
}
