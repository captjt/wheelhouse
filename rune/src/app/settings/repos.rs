//! Repos (`ReposSettings`, `RepoForm`): each repo's checkout, its notes for
//! agents, its PRs' base and plan, its witnessed checks (`ChecksTable`,
//! `CheckEditor`), and its Review: who posts, and its review guide
//! (`ReviewGuidePicker`).

use rune::{Align, Color, Cursor, El, PopupItem, PopupMenu, TextEdit, Tx, auto_grow, col, edit_access, el, row, spacer, text, text_input};
use rune_kit::Kit;
use rune_kit::shell::{Sidebar, SidebarRow, SidebarSection};
use serde_json::{Value, json};

use super::ui::{self, Look, choice};
use super::{RepoForm, Settings, Sheet, Tab, glyphs, orders, patch};
use crate::app::widgets;
use crate::model::{self, flag, text as jtext};
use crate::theme::{K, font, mono};

/// `/Users/you/x` as `~/x`.
pub fn short_path(p: &str) -> String {
    match std::env::var("HOME") {
        Ok(home) if !home.is_empty() && p.starts_with(&home) => format!("~{}", &p[home.len()..]),
        _ => p.to_string(),
    }
}

/// Every repo Build or Review knows, by slug.
pub fn slugs(s: &Settings) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for part in [s.build(), s.review()] {
        if let Some(m) = part["repos"].as_object() {
            out.extend(m.keys().cloned());
        }
    }
    out.sort();
    out.dedup();
    out
}

/// The repo the tab shows: the one picked, or the first.
pub fn current(s: &Settings) -> Option<String> {
    let all = slugs(s);
    s.repo.clone().filter(|r| all.contains(r)).or_else(|| all.into_iter().next())
}

pub fn saved_notes(s: &Settings, slug: &str) -> String {
    s.build()["repos"][slug]["notes"].as_str().or_else(|| s.review()["repos"][slug]["notes"].as_str()).unwrap_or("").to_string()
}

pub fn saved_base(s: &Settings, slug: &str) -> String {
    s.build()["repos"][slug]["base"].as_str().unwrap_or("").to_string()
}

/// The shown repo's fields, made fresh when it changes (`.id(current)`).
pub fn ensure_form(s: &mut Settings) {
    let Some(slug) = current(s) else {
        s.repo_form = None;
        return;
    };
    if s.repo_form.as_ref().is_some_and(|f| f.slug == slug) {
        return;
    }
    let (notes, base) = (saved_notes(s, &slug), saved_base(s, &slug));
    s.repo_form = Some(RepoForm { slug, notes: TextEdit::new(notes.clone()), base: TextEdit::new(base.clone()), saved_notes: notes, saved_base: base, base_focus: false });
    s.check = None;
}

fn select(s: &mut Settings, slug: String) {
    s.repo = Some(slug);
    ensure_form(s);
}

/// The repo's notes field, for its text input.
fn notes_mut(s: &mut Settings) -> &mut TextEdit {
    match s.repo_form.as_mut() {
        Some(f) => &mut f.notes,
        None => &mut s.scratch,
    }
}

/// The repo's base field, for its text input.
fn base_mut(s: &mut Settings) -> &mut TextEdit {
    match s.repo_form.as_mut() {
        Some(f) => &mut f.base,
        None => &mut s.scratch,
    }
}

pub fn view(s: &Settings, k: &K, kit: &Kit) -> El {
    let all = slugs(s);
    let shown = current(s);
    let rows = all.iter().map(|slug| {
        let sl = slug.clone();
        SidebarRow::new(slug.clone())
            .icon(&glyphs::FOLDER)
            .height(28.0)
            .selected(shown.as_deref() == Some(slug))
            .on_select(move |s: &mut Settings, _| select(s, sl.clone()))
            .build(kit)
            .key(ui::key_of("repo", slug))
    });
    let list = Sidebar::new().label("Repos").inset(8.0).row_gap(1.0).section(SidebarSection::new("Repos").key("repos-section").rows(rows)).build(kit);
    let detail = match (&shown, &s.repo_form) {
        (Some(slug), Some(f)) if &f.slug == slug => form(s, f, k, kit),
        (None, _) if s.view.is_some() => {
            widgets::empty(&glyphs::FOLDER, "No repos yet", "Add each repo to build.lua or review.lua with its checkout. Then its notes, checks and review guide are set here.", k)
        }
        _ => ui::loading(k),
    };
    row()
        .key("repos")
        .w_full()
        .h_full()
        .child(col().key("repos-list").w(210.0).h_full().pt(8.0).child(list))
        .child(el().w(1.0).h_full().fill(k.line).a11y_hidden())
        .child(col().key("repo-detail").grow(1.0).min_w(0.0).h_full().instant_changes().child(detail))
}

fn form(s: &Settings, f: &RepoForm, k: &K, kit: &Kit) -> El {
    let slug = f.slug.as_str();
    let b = &s.build()["repos"][slug];
    let r = &s.review()["repos"][slug];
    let (in_build, in_review) = (!b.is_null(), !r.is_null());
    let mut sections = orders::problems(s, k);

    let checkout = b["checkout"].as_str().or_else(|| r["checkout"].as_str()).unwrap_or("");
    let checkout = ui::cell(vec![ui::labeled(
        text("Checkout").font(font(13.0, 400.0).lh(17.0)).color(k.text),
        text(checkout.to_string()).font(mono(12.0).lh(16.0)).color(k.text).selectable(true),
    )]);
    sections.push(ui::section("repo-checkout", Some(slug), vec![checkout], Some("Where Builds make their worktrees and reviewers read the code. It's set in the Lua."), k));

    sections.push(ui::section(
        "repo-notes",
        Some("Notes for agents"),
        vec![notes(s, f, k)],
        Some("This repo's rules. Builders, checkers and reviewers here read them before they start."),
        k,
    ));

    if in_build {
        sections.push(ui::section("repo-prs", Some("Pull requests"), vec![base(s, f, k), plan(s, slug, b, k)], None, k));
        sections.push(ui::section(
            "repo-checks",
            Some("Witnessed checks"),
            vec![checks(s, slug, k)],
            Some("Wheelhouse runs these itself, at a Build's commit, before its PR opens or a round pushes, and when you press Run Checks on a docket in this repo. A required one that fails holds the PR; an optional one only informs."),
            k,
        ));
    }
    if in_review {
        sections.push(ui::section("posting", Some("Review"), vec![orders::posting(s, Some(slug), k), guide(s, slug, k)], None, k));
    }
    sections.push(orders::where_it_lives(s, k));
    ui::form("repo-form", 600.0, sections, kit)
}

fn notes(s: &Settings, f: &RepoForm, k: &K) -> El {
    let field = text_input(&f.notes, font(13.0, 400.0).lh(18.0))
        .key("repo-notes-field")
        .w_full()
        .color(k.text)
        .placeholder("Rules agents follow in this repo. For example: Run `just check` before you commit. Don't touch the generated client.", k.faint)
        .label("Notes for agents")
        .on_edit(edit_access(notes_mut))
        // Return breaks the line: ⌘S saves.
        .on_submit(|s: &mut Settings, _| notes_mut(s).insert("\n"));
    let editor = row().w_full().padding(7.0).radius(7.0).fill(ui::card(k)).border(1.0, k.line2).child(auto_grow(field, 4, 11));
    let dirty = f.notes.text != f.saved_notes;
    let slug = f.slug.as_str();
    let mut foot = row().w_full().gap(8.0).center_y();
    if s.is_set("build", &["repos", slug, "notes"]) || s.is_set("review", &["repos", slug, "notes"]) {
        let label = if s.lua_exists("build") || s.lua_exists("review") { "Use the Lua's" } else { "Use the default" };
        foot = foot.child(ui::set_here_line(ui::link("notes-revert-setting", label, !s.is_busy("notes"), 11.5, k, |s: &mut Settings, _| revert_notes(s)), k));
    }
    foot = foot.child(spacer());
    if dirty {
        foot = foot.child(ui::button("notes-revert", "Revert", Look::Plain, true, k, |s: &mut Settings, _| {
            if let Some(f) = &mut s.repo_form {
                let saved = f.saved_notes.clone();
                f.notes.replace_text(saved);
            }
        }));
    }
    foot = foot.child(ui::button("notes-save", "Save Notes", Look::Plain, dirty && !s.is_busy("notes"), k, |s: &mut Settings, _| save_notes(s)).help("Save the notes (⌘S)"));
    ui::cell(ui::some(vec![Some(editor), Some(foot), ui::error_line(s, "notes", k)]))
}

/// Save the repo's notes: both protocols' agents read them.
pub fn save_notes(s: &mut Settings) {
    let Some(f) = &s.repo_form else { return };
    let (slug, t) = (f.slug.clone(), f.notes.text.trim().to_string());
    if t == f.saved_notes.trim() {
        return;
    }
    let in_build = !s.build()["repos"][&slug].is_null();
    let in_review = !s.review()["repos"][&slug].is_null();
    let mut p = json!({});
    if in_build || s.lua_exists("build") {
        p = patch::make("build", &["repos", &slug, "notes"], Some(json!(t)));
    }
    if in_review && s.lua_exists("review") {
        p = patch::merge(p, patch::make("review", &["repos", &slug, "notes"], Some(json!(t))));
    }
    s.change("notes", p, move |s| {
        if let Some(f) = s.repo_form.as_mut().filter(|f| f.slug == slug) {
            f.notes.replace_text(t);
        }
    });
}

fn revert_notes(s: &mut Settings) {
    let Some(slug) = s.repo_form.as_ref().map(|f| f.slug.clone()) else { return };
    let mut p = json!({});
    for part in ["build", "review"] {
        if s.is_set(part, &["repos", &slug, "notes"]) {
            p = patch::merge(p, patch::make(part, &["repos", &slug, "notes"], None));
        }
    }
    s.change("notes", p, |_| {});
}

fn base(s: &Settings, f: &RepoForm, k: &K) -> El {
    let field = text_input(&f.base, ui::field_font(12.5, true))
        .key("repo-base")
        .single_line()
        .w_full()
        .color(k.text)
        .placeholder("the default branch", k.faint)
        .label("Base branch")
        .on_edit(edit_access(base_mut))
        .on_submit(|s: &mut Settings, _| save_base(s));
    let slug = f.slug.as_str();
    ui::cell(ui::some(vec![
        Some(ui::labeled(ui::described("Base branch", "Builds branch from it, and their PRs go into it.", k), ui::field_box(field, k).w(200.0))),
        ui::set_here(s, "build", &["repos", slug, "base"], "base", k),
        ui::error_line(s, "base", k),
    ]))
}

/// The base, as typed: on Return, and when the field lets go of the keys.
pub fn save_base(s: &mut Settings) {
    let Some(f) = &s.repo_form else { return };
    let (slug, t) = (f.slug.clone(), f.base.text.trim().to_string());
    if t == f.saved_base {
        return;
    }
    s.change("base", patch::make("build", &["repos", &slug, "base"], if t.is_empty() { None } else { Some(json!(t)) }), |_| {});
}

/// The base field let go of the keys: what's in it is saved.
pub fn follow_focus(s: &mut Settings) {
    let Some(f) = &mut s.repo_form else { return };
    let (was, is) = (f.base_focus, f.base.focused);
    f.base_focus = is;
    if was && !is {
        save_base(s);
    }
}

fn plan(s: &Settings, slug: &str, b: &Value, k: &K) -> El {
    let plan = match &b["plan"] {
        Value::Bool(false) => "off".to_string(),
        v => v.as_str().unwrap_or("full").to_string(),
    };
    let sentence = match plan.as_str() {
        "full" => "Its intent, acceptance, out of scope and notes go on the PR as they are.",
        "summary" => "In the builder's own words, without quoting the issue: for a public repo with a private tracker.",
        "off" => "No plan on the PR. The builder still writes one, for you and the checker.",
        _ => "",
    };
    let sl = slug.to_string();
    let ids = ["full", "summary", "off"];
    let pick = ui::segmented(
        "repo-plan",
        "The plan on the PR",
        &[("Full", plan == "full"), ("Summary", plan == "summary"), ("Off", plan == "off")],
        !s.is_busy("plan"),
        k,
        move |s: &mut Settings, i| {
            let v = if ids[i] == "off" { json!(false) } else { json!(ids[i]) };
            s.change("plan", patch::make("build", &["repos", &sl, "plan"], Some(v)), |_| {});
        },
    );
    ui::cell(ui::some(vec![
        Some(ui::labeled(ui::described("The plan on the PR", sentence, k), pick)),
        ui::set_here(s, "build", &["repos", slug, "plan"], "plan", k),
        ui::error_line(s, "plan", k),
    ]))
}

// ---- witnessed checks ----

#[derive(Clone)]
pub struct Check {
    pub name: String,
    pub run: String,
    pub required: bool,
    pub timeout: i64,
}

fn repo_checks(s: &Settings, slug: &str) -> Vec<Check> {
    model::arr(&s.build()["repos"][slug]["checks"])
        .iter()
        .map(|c| Check {
            name: model::str_or(&c["name"], "").to_string(),
            run: model::str_or(&c["run"], "").to_string(),
            required: if c["required"].is_null() { true } else { flag(&c["required"]) },
            timeout: model::int(&c["timeout"]).unwrap_or(1200),
        })
        .collect()
}

/// A repo's checks: name, command, required, timeout; add, edit, remove.
fn checks(s: &Settings, slug: &str, k: &K) -> El {
    let list = repo_checks(s, slug);
    let sel = list.iter().find(|c| Some(&c.name) == s.check.as_ref()).cloned();
    let set_here = |name: &str| s.is_set("build", &["repos", slug, "checks", name]);
    let head = |t: &str, w: Option<f32>, end: bool| {
        let e = text(t.to_string()).font(font(11.0, 500.0).lh(14.0)).color(k.muted);
        let e = if end { e.align(Align::End) } else { e };
        match w {
            Some(w) => e.w(w),
            None => e.grow(1.0).min_w(0.0),
        }
    };
    let mut table = col()
        .key("checks-table")
        .w_full()
        .radius(7.0)
        .fill(ui::card(k))
        .border(1.0, k.line2)
        .clip_rounded(7.0)
        .role("grid")
        .label("Witnessed checks")
        .focus_group()
        .child(
            row()
                .w_full()
                .px(10.0)
                .py(6.0)
                .gap(10.0)
                .child(head("Name", Some(90.0), false))
                .child(head("Command", None, false))
                .child(head("Required", Some(62.0), false))
                .child(head("Timeout", Some(56.0), true)),
        )
        .child(el().w_full().h(1.0).fill(k.line).a11y_hidden());
    if list.is_empty() {
        table = table.child(row().w_full().padding(10.0).child(ui::words(
            "No checks yet. Add the commands that show this repo's work is sound: its tests, its linter.",
            12.0,
            k.muted,
        )));
    }
    for (i, c) in list.iter().enumerate() {
        let on = sel.as_ref().is_some_and(|x| x.name == c.name);
        let timeout = if c.timeout % 60 == 0 { format!("{} min", c.timeout / 60) } else { format!("{} s", c.timeout) };
        let mut command =
            row().grow(1.0).min_w(0.0).gap(6.0).center_y().child(text(c.run.clone()).font(mono(12.0).lh(16.0)).color(k.text).truncate_middle().shrink().help(c.run.clone()));
        if set_here(&c.name) {
            command = command.child(text("set here").font(font(10.5, 400.0).lh(14.0)).color(k.muted));
        }
        let (a, b, d, slug_d) = (c.name.clone(), c.name.clone(), c.name.clone(), slug.to_string());
        let here = set_here(&c.name);
        table = table.child(
            row()
                .key(format!("check-{}", c.name))
                .w_full()
                .px(10.0)
                .py(6.0)
                .gap(10.0)
                .center_y()
                .fill(if on { k.accent.alpha(0.16) } else { Color::TRANSPARENT })
                .role("row")
                .label(format!("{}: {}", c.name, c.run))
                .focus_selected(on)
                .focusable(true)
                .on_press(move |s: &mut Settings| s.check = Some(a.clone()))
                .on_double_press(move |s: &mut Settings| edit_check(s, &b))
                .on_secondary_press(move |s: &mut Settings, press, tx: &mut Tx| {
                    s.check = Some(d.clone());
                    let menu = PopupMenu::new().item(PopupItem::new("edit", "Edit…")).item(PopupItem::new("remove", if here { "Remove from Settings" } else { "Remove" }));
                    let (name, slug) = (d.clone(), slug_d.clone());
                    tx.popup_menu(menu, press, move |s: &mut Settings, picked, _| match picked {
                        Some("edit") => edit_check(s, &name),
                        Some("remove") => remove_check(s, &slug, &name),
                        _ => {}
                    });
                })
                .child(text(c.name.clone()).font(rune::tok::TextStyle::new(12.0, 500.0).family(rune::tok::Family::SystemMono).lh(16.0)).color(k.text).truncate().w(90.0))
                .child(command)
                .child(text(if c.required { "Required" } else { "Optional" }).font(font(12.0, 400.0).lh(16.0)).color(if c.required { k.text } else { k.muted }).w(62.0))
                .child(text(timeout).font(font(12.0, 400.0).lh(16.0).features("tnum")).color(k.text2).align(Align::End).w(56.0)),
        );
        if i + 1 < list.len() {
            table = table.child(el().w_full().h(1.0).fill(k.line.alpha(0.5)).a11y_hidden());
        }
    }

    // The control group under it: add, remove, and edit what's selected.
    let remove_help = match &sel {
        Some(c) if set_here(&c.name) => format!("Remove {} from Settings", c.name),
        Some(c) => format!("Turn {} off: {} lists it, and Settings keeps it off", c.name, s.lua("build")),
        None => "Select a check".into(),
    };
    let sl = slug.to_string();
    let seg = |key: &str, i: &'static rune::Icon, help: String, enabled: bool| {
        row()
            .key(key.to_string())
            .size(26.0, 20.0)
            .center()
            .role("button")
            .label(help.clone())
            .help(help)
            .opacity(if enabled { 1.0 } else { 0.4 })
            .child(rune::icon(i, 11.0).color(k.text))
    };
    let add = seg("check-add", &glyphs::PLUS, "Add a check".into(), true).hover(k.hover).cursor(Cursor::Pointer).on_press(move |s: &mut Settings| add_check(s, &sl));
    let minus = seg("check-remove", &glyphs::MINUS, remove_help, sel.is_some());
    let minus = match &sel {
        Some(c) => {
            let (name, slug) = (c.name.clone(), slug.to_string());
            minus.hover(k.hover).cursor(Cursor::Pointer).on_press(move |s: &mut Settings| remove_check(s, &slug, &name))
        }
        None => minus.disabled(true),
    };
    let group = row()
        .radius(6.0)
        .fill(if k.dark { Color::WHITE.alpha(0.1) } else { Color::WHITE })
        .border(0.5, k.line2)
        .clip_rounded(6.0)
        .child(add)
        .child(el().w(0.5).h(20.0).fill(k.line2).a11y_hidden())
        .child(minus);
    let edit_name = sel.as_ref().map(|c| c.name.clone());
    let edit = ui::button("check-edit", "Edit…", Look::Plain, sel.is_some(), k, move |s: &mut Settings, _| {
        if let Some(n) = &edit_name {
            edit_check(s, n);
        }
    });
    let mut bar = row().w_full().gap(6.0).center_y().child(group).child(edit).child(spacer());
    match &sel {
        Some(c) if !set_here(&c.name) => bar = bar.child(text(format!("From {}: editing it sets it here.", s.lua("build"))).font(font(11.5, 400.0).lh(15.0)).color(k.muted)),
        _ => {
            if let Some(n) = s.notes.get("checks") {
                bar = bar.child(text(n.clone()).font(font(11.5, 400.0).lh(15.0)).color(k.muted).shrink());
            }
        }
    }
    ui::cell(ui::some(vec![Some(table), Some(bar), ui::error_line(s, "checks", k)]))
}

fn remove_check(s: &mut Settings, slug: &str, name: &str) {
    let (sl, n) = (slug.to_string(), name.to_string());
    if !s.is_set("build", &["repos", slug, "checks", name]) {
        // build.lua lists it: `false` here keeps it off (settings.json wins).
        s.change("checks", patch::make("build", &["repos", slug, "checks", name], Some(json!(false))), move |s| {
            s.notes.insert("checks".into(), format!("Removed {n}. {} still lists it; Settings keeps it off.", s.lua("build")));
            s.check = None;
        });
        return;
    }
    s.change("checks", patch::make("build", &["repos", slug, "checks", name], None), move |s| {
        // Removing what's set here brings back the Lua's check of that name, if it has one.
        let back = model::arr(&s.build()["repos"][&sl]["checks"]).iter().any(|c| c["name"].as_str() == Some(n.as_str()));
        let note = if back { format!("{}'s {n} applies again.", s.lua("build")) } else { format!("Removed {n}.") };
        s.notes.insert("checks".into(), note);
        if !back {
            s.check = None;
        }
    });
}

/// Open the check editor on a check of the shown repo.
pub fn edit_check(s: &mut Settings, name: &str) {
    let Some(slug) = current(s) else { return };
    let Some(c) = repo_checks(s, &slug).into_iter().find(|c| c.name == name) else { return };
    s.errors.remove("check.edit");
    s.check = Some(c.name.clone());
    let minutes = ((c.timeout as f64) / 60.0).ceil().max(1.0) as i64;
    s.sheet = Some(Sheet::Check { slug, existing: Some(c.name.clone()), name: TextEdit::new(c.name), run: TextEdit::new(c.run), required: c.required, minutes });
}

fn add_check(s: &mut Settings, slug: &str) {
    s.errors.remove("check.edit");
    let mut name = TextEdit::new("");
    name.focused = true;
    s.sheet = Some(Sheet::Check { slug: slug.to_string(), existing: None, name, run: TextEdit::new(""), required: true, minutes: 20 });
}

fn check_field(s: &mut Settings, which: u8) -> &mut TextEdit {
    match &mut s.sheet {
        Some(Sheet::Check { name, .. }) if which == 0 => name,
        Some(Sheet::Check { run, .. }) => run,
        _ => &mut s.scratch,
    }
}

/// What's wrong with a check's name, if anything.
fn name_problem(s: &Settings, slug: &str, existing: &Option<String>, name: &str) -> Option<&'static str> {
    let n = name.trim();
    if n.is_empty() {
        None
    } else if !patch::valid_check_name(n) {
        Some("Letters, digits, dashes and underscores")
    } else if existing.is_none() && repo_checks(s, slug).iter().any(|c| c.name == n) {
        Some("There's a check by that name already")
    } else {
        None
    }
}

/// Add or edit a check: its command, whether it's required, and how long it may run (`CheckEditor`).
pub fn check_editor(s: &Settings, sheet: &Sheet, k: &K) -> El {
    let Sheet::Check { slug, existing, name, run, required, minutes } = sheet else { return col() };
    let problem = name_problem(s, slug, existing, &name.text);
    let name_field = text_input(name, ui::field_font(13.0, true))
        .key("check-name")
        .single_line()
        .w_full()
        .color(if existing.is_some() { k.muted } else { k.text })
        .placeholder("test", k.faint)
        .label("Name")
        .on_edit(edit_access(|s: &mut Settings| check_field(s, 0)))
        .on_submit(|s: &mut Settings, _| save_check(s));
    let name_field = if existing.is_some() { name_field.disabled(true) } else { name_field };
    let run_field = text_input(run, ui::field_font(13.0, true))
        .key("check-run")
        .single_line()
        .w_full()
        .color(k.text)
        .placeholder("pnpm test", k.faint)
        .label("Command")
        .on_edit(edit_access(|s: &mut Settings| check_field(s, 1)))
        .on_submit(|s: &mut Settings, _| save_check(s));
    let req = *required;
    let required_switch = ui::switch(req, "Required", true, k, &k.kit(), |s: &mut Settings| {
        if let Some(Sheet::Check { required, .. }) = &mut s.sheet {
            *required = !*required;
        }
    });
    let timeout = ui::stepper("check-timeout", "Timeout", *minutes, 1, 120, true, k, |s: &mut Settings, v| {
        if let Some(Sheet::Check { minutes, .. }) = &mut s.sheet {
            *minutes = v;
        }
    });
    let mut rows = vec![ui::cell(vec![ui::form_line("Name", ui::field_box(name_field, k), k)])];
    if let Some(p) = problem {
        rows.push(ui::cell(vec![text(p).font(font(11.5, 400.0).lh(15.0)).color(k.error)]));
    }
    rows.push(ui::cell(vec![ui::form_line("Command", ui::field_box(run_field, k), k)]));
    let sentence = if req { "If it fails, the PR doesn't open and a round doesn't push." } else { "It only informs: a failure holds nothing." };
    rows.push(ui::cell(vec![ui::labeled(ui::described("Required", sentence, k), required_switch)]));
    rows.push(ui::cell(vec![ui::labeled(
        text("Timeout").font(font(13.0, 400.0).lh(17.0)).color(k.text),
        row().gap(4.0).center_y().child(timeout).child(text("min").font(font(13.0, 400.0).lh(16.0)).color(k.text)),
    )]));
    if let Some(e) = ui::error_line(s, "check.edit", k) {
        rows.push(ui::cell(vec![e]));
    }
    let ok = !name.text.trim().is_empty() && problem.is_none() && !run.text.trim().is_empty() && !s.is_busy("check.edit");
    let title = match existing {
        Some(n) => format!("Edit {n}"),
        None => format!("Add a check to {slug}"),
    };
    let foot = ui::words("It runs in the docket's worktree or folder, with your login shell, behind the same wall as the agents.", 11.5, k.muted);
    let buttons = vec![
        ui::button("check-cancel", "Cancel", Look::Plain, true, k, |s: &mut Settings, _| s.sheet = None),
        ui::button("check-save", if existing.is_none() { "Add Check" } else { "Save" }, Look::Primary, ok, k, |s: &mut Settings, _| save_check(s)).default_button(),
    ];
    ui::sheet("check-editor", &title, 480.0, vec![ui::section("check-form", None, rows, None, k)], Some(foot), buttons, k)
}

fn save_check(s: &mut Settings) {
    let Some(Sheet::Check { slug, existing, name, run, required, minutes }) = &s.sheet else { return };
    let n = name.text.trim().to_string();
    if n.is_empty() || run.text.trim().is_empty() || name_problem(s, slug, existing, &n).is_some() {
        return;
    }
    let p = patch::make("build", &["repos", slug, "checks", &n], Some(patch::check(&run.text, *required, minutes * 60)));
    s.change("check.edit", p, |s| {
        s.notes.remove("checks");
        s.sheet = None;
    });
}

// ---- the repo's review guide ----

/// The repo's review guide: one of your guides, or none.
fn guide(s: &Settings, slug: &str, k: &K) -> El {
    let mine: Vec<_> = s.docs().into_iter().filter(|d| d.kind() == "guide" && d.yours()).collect();
    let current = jtext(&s.review()["repos"][slug]["guides"]["review"]);
    let doc = mine.iter().find(|d| Some(d.path()) == current).cloned();
    let mut choices = vec![choice("none", "None")];
    if !mine.is_empty() {
        choices.push(None);
    }
    choices.extend(mine.iter().map(|d| choice(&d.path(), &d.title())));
    if let Some(c) = current.as_ref().filter(|_| doc.is_none()) {
        choices.push(choice(c, &short_path(c)));
    }
    let value = match (&doc, &current) {
        (Some(d), _) => d.title(),
        (None, Some(c)) => short_path(c),
        _ => "None".into(),
    };
    let sl = slug.to_string();
    let pick = ui::popup("repo-guide", "Review guide", &value, choices, current.as_deref().unwrap_or("none"), !s.is_busy("guide"), k, move |s: &mut Settings, v| {
        let value = if v == "none" { None } else { Some(json!(v)) };
        s.change("guide", patch::make("review", &["repos", &sl, "guides", "review"], value), |_| {});
    });
    let mut control = row().gap(8.0).center_y().child(pick);
    if let Some(d) = doc {
        control = control.child(ui::button("repo-guide-edit", "Edit…", Look::Plain, true, k, move |s: &mut Settings, tx| {
            s.guide = Some(d.id());
            s.go(Tab::Guides, tx);
        }));
    }
    let sentence =
        if mine.is_empty() { "Write a guide of your own in Guides & Briefs, then pick it here." } else { "What reviewers in this repo look for, beyond what they always do." };
    ui::cell(ui::some(vec![
        Some(ui::labeled(ui::described("Review guide", sentence, k), control)),
        ui::set_here(s, "review", &["repos", slug, "guides", "review"], "guide", k),
        ui::error_line(s, "guide", k),
    ]))
}
