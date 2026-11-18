//! Standing Orders (`OrdersSettings`): what Wheelhouse does on its own,
//! Review's and Build's switches, and Build's gates. Also who posts a
//! checked review (`PostingPicker`, for every repo or one) and its rule's
//! editor (`RuleEditor`).

use rune::{El, TextEdit, col, el, icon, row, text};
use rune_kit::Kit;
use serde_json::{Value, json};

use super::ui::{self, Look};
use super::{Settings, Sheet, patch, repos};
use crate::icons;
use crate::model::{self, flag, text as jtext};
use crate::theme::{K, font};

pub fn view(s: &Settings, k: &K, kit: &Kit) -> El {
    let state = s.store.state();
    let rv = state.as_ref().map(|st| st.review().clone());
    let bs = state.as_ref().map(|st| st.build().clone());
    let mut sections = problems(s, k);

    // One card, its orders divided by hairlines.
    let mut rows = Vec::new();
    if s.orders().is_empty() {
        let none = if s.view.is_none() { "Loading…" } else { "None: Review and Build aren't set up yet (review.lua, build.lua)." };
        rows.push(ui::cell(vec![ui::words(none, 13.0, k.muted)]));
    }
    for (i, o) in s.orders().iter().enumerate() {
        rows.push(order_row(i, o, k));
    }
    sections.push(ui::section(
        "orders",
        Some("Standing orders"),
        rows,
        Some("What Wheelhouse does on its own. Each is worked out from the settings in these tabs, so it says what will happen."),
        k,
    ));

    if !s.review().is_null() {
        let mut rows = Vec::new();
        let running = rv.as_ref().is_some_and(|r| flag(&r["rex_reviewer_running"]));
        if rv.as_ref().is_some_and(|r| !flag(&r["enabled"])) {
            let sentence = if running { "rex-reviewer's watcher is still running: stop it first." } else { "They start paused, until you resume them." };
            let on = ui::button("review-enable", "Turn On Reviews", Look::Plain, !running, k, |s: &mut Settings, _| {
                s.toggle("review.enabled", "/api/review/settings", json!({ "enabled": true }));
            });
            rows.push(ui::cell(ui::some(vec![Some(ui::labeled(ui::described("Reviews are off", sentence, k), on)), ui::error_line(s, "review.enabled", k)])));
        } else {
            let paused = rv.as_ref().is_some_and(|r| flag(&r["paused"]));
            let sw = ui::switch(!paused, "Take on review requests", !s.is_busy("review.paused"), k, kit, move |s: &mut Settings| {
                s.toggle("review.paused", "/api/review/settings", json!({ "paused": !paused }));
            });
            let sentence = if paused { "Paused: no new rounds start." } else { "A round starts when a PR requests you." };
            rows.push(ui::cell(ui::some(vec![Some(ui::labeled(ui::described("Take on review requests", sentence, k), sw)), ui::error_line(s, "review.paused", k)])));
        }
        // A rule is config: worth setting before Reviews are on, too.
        rows.push(posting(s, None, k));
        sections.push(ui::section("review", Some("Review"), rows, None, k));
    }

    if !s.build().is_null() {
        let paused = bs.as_ref().is_some_and(|b| flag(&b["paused"]));
        let sw = ui::switch(!paused, "Start Builds", !s.is_busy("build.paused"), k, kit, move |s: &mut Settings| {
            s.toggle("build.paused", "/api/build/settings", json!({ "paused": !paused }));
        });
        let sentence = if paused { "Paused: queued Builds wait, and nothing new starts." } else { "Queued Builds start as there's room." };
        let row = ui::cell(ui::some(vec![Some(ui::labeled(ui::described("Start Builds", sentence, k), sw)), ui::error_line(s, "build.paused", k)]));
        sections.push(ui::section("build", Some("Build"), vec![row], None, k));

        let gates: Vec<El> = model::arr(&s.build()["gates"]).iter().map(|g| gate_row(s, g, k)).collect();
        sections.push(ui::section(
            "gates",
            Some("Gates"),
            gates,
            Some("At a gate set to Ask, a Build stops and asks you in the Inbox. Auto goes ahead, and the record says so."),
            k,
        ));
    }
    sections.push(where_it_lives(s, k));
    ui::form("orders-form", 600.0, sections, kit)
}

fn order_row(i: usize, o: &Value, k: &K) -> El {
    let proto = model::str_or(&o["protocol"], "");
    let state = if !flag(&o["on"]) {
        "Off"
    } else if flag(&o["paused"]) {
        "Paused"
    } else {
        "On"
    };
    let mark = match proto {
        "review" => &icons::PULL,
        "build" => &icons::HAMMER,
        _ => &icons::SHIELD,
    };
    let color = match state {
        "On" => k.working,
        "Paused" => k.blocked,
        _ => k.faint,
    };
    let help = match state {
        "Paused" => "Paused: resume it below",
        "Off" => "Turned off",
        _ => "In force",
    };
    let words = ui::inline(model::str_or(&o["text"], ""), font(13.0, 400.0).lh(18.0), k.text, k).selectable(true);
    row()
        .key(format!("order-{i}"))
        .w_full()
        .px(12.0)
        .py(10.0)
        .gap(10.0)
        .items_start()
        .child(row().w(18.0).pt(1.0).child(icon(mark, 15.0).color(k.muted)))
        .child(words.grow(1.0).min_w(0.0))
        .child(el().pt(1.0).child(ui::chip(state, color).help(help)))
}

fn gate_row(s: &Settings, g: &Value, k: &K) -> El {
    let key = model::str_or(&g["key"], "").to_string();
    let value = model::str_or(&g["value"], "ask").to_string();
    let label = jtext(&g["label"]).unwrap_or_else(|| key.clone());
    let sentence = model::str_or(&g[if value == "auto" { "auto" } else { "ask" }], "").to_string();
    let busy = format!("gates.{key}");
    let (k2, v2) = (key.clone(), value.clone());
    let pick = ui::segmented(&format!("gate-{key}"), &label, &[("Ask", value != "auto"), ("Auto", value == "auto")], !s.is_busy(&busy), k, move |s: &mut Settings, i| {
        let v = if i == 0 { "ask" } else { "auto" };
        if v != v2 {
            s.change(&format!("gates.{k2}"), patch::make("build", &["gates", &k2], Some(json!(v))), |_| {});
        }
    });
    ui::cell(ui::some(vec![Some(ui::labeled(ui::described(&label, &sentence, k), pick)), ui::set_here(s, "build", &["gates", &key], &busy, k), ui::error_line(s, &busy, k)]))
}

// ---- who posts a checked review ----

/// Who posts a checked review, for every repo or (with a slug) one: Ask,
/// Auto, or When… (a rule: it posts itself when every condition holds, and
/// otherwise waits for you, saying why). docs/review.md#who-posts.
pub fn posting(s: &Settings, slug: Option<&str>, k: &K) -> El {
    let rule = match slug {
        Some(sl) => &s.review()["repos"][sl]["post"],
        None => &s.review()["post_rule"],
    };
    let path: Vec<String> = match slug {
        Some(sl) => vec!["repos".into(), sl.into(), "post".into()],
        None => vec!["post".into()],
    };
    let key = slug.map(|sl| format!("repo.{sl}.post")).unwrap_or_else(|| "review.post".into());
    let mode = if rule.is_null() { "same".to_string() } else { model::str_or(&rule["mode"], "ask").to_string() };
    let sentence = match jtext(&rule["fn"]) {
        Some(f) => format!("`{f}` in review.lua decides, for each checked review. Pick Ask, Auto or When… to set it here instead."),
        None if !rule.is_null() => format!("A checked review {}.", model::str_or(&rule["text"], "")),
        None => format!("As every repo: a checked review {}.", model::str_or(&s.review()["post_rule"]["text"], "waits for you")),
    };
    let mut segs: Vec<(&str, &str)> = Vec::new();
    if slug.is_some() {
        segs.push(("same", "Same as All"));
    }
    segs.extend([("ask", "Ask"), ("auto", "Auto"), ("rule", "When…")]);
    let shown: Vec<(&str, bool)> = segs.iter().map(|(id, l)| (*l, *id == mode)).collect();
    let ids: Vec<String> = segs.iter().map(|(id, _)| id.to_string()).collect();
    let (slug_s, path_s, key_s) = (slug.map(str::to_string), path.clone(), key.clone());
    let picker = ui::segmented(&ui::key_of("posting", slug.unwrap_or("all")), "Posting", &shown, !s.is_busy(&key), k, move |s: &mut Settings, i| {
        let p: Vec<&str> = path_s.iter().map(String::as_str).collect();
        match ids[i].as_str() {
            "rule" => edit_rule(s, slug_s.as_deref()),
            "same" => s.change(&key_s, patch::make("review", &p, None), |_| {}),
            v => s.change(&key_s, patch::make("review", &p, Some(json!(v))), |_| {}),
        }
    });
    let mut lines = vec![Some(ui::labeled(ui::described("Posting", &sentence, k), picker))];
    if mode == "rule" && !rule["auto"].is_null() {
        let sl = slug.map(str::to_string);
        lines.push(Some(ui::link(&format!("{key}-edit"), "Edit the Rule…", true, 11.5, k, move |s: &mut Settings, _| edit_rule(s, sl.as_deref()))));
    }
    if slug.is_none()
        && let Some(o) = jtext(&s.review()["post_override"])
    {
        let k2 = key.clone();
        let follow = ui::link("post-follow", "Follow These Settings", !s.is_busy(&key), 11.5, k, move |s: &mut Settings, _| {
            s.toggle(&k2, "/api/review/settings", json!({ "post": "config" }));
        });
        let what = if o == "auto" { "posts itself" } else { "waits for you" };
        lines.push(Some(
            row()
                .w_full()
                .gap(6.0)
                .items_start()
                .child(ui::inline(&format!("`wh review post-mode {o}` is on: every repo {what} until you change posting here."), font(11.5, 400.0).lh(15.0), k.muted, k).shrink())
                .child(follow),
        ));
    }
    let p: Vec<&str> = path.iter().map(String::as_str).collect();
    lines.push(ui::set_here(s, "review", &p, &key, k));
    lines.push(ui::error_line(s, &key, k));
    ui::cell(ui::some(lines))
}

/// Open the rule's editor, with the rule in force when there is one.
pub fn edit_rule(s: &mut Settings, slug: Option<&str>) {
    let rule = match slug {
        Some(sl) => s.review()["repos"][sl]["post"].clone(),
        None => s.review()["post_rule"].clone(),
    };
    let auto = if model::str_or(&rule["mode"], "") == "rule" { rule["auto"].clone() } else { Value::Null };
    let list = |k: &str| model::strings(&auto[k]).join(", ");
    s.errors.remove("rule.edit");
    s.sheet = Some(Sheet::Rule {
        slug: slug.map(str::to_string),
        approve: auto.is_null() || model::str_or(&auto["verdict"], "") == "approve",
        risk: if auto.is_null() { "low".into() } else { model::str_or(&auto["merge_risk"], "").to_string() },
        max_lines: TextEdit::new(model::int(&auto["max_lines"]).map(|n| n.to_string()).unwrap_or_default()),
        authors: TextEdit::new(list("authors")),
        labels: TextEdit::new(list("labels")),
        not_touching: TextEdit::new(list("not_touching")),
        problem: None,
    });
}

/// A posting rule's conditions: every one set must hold for a checked
/// review to post itself (`RuleEditor`).
pub fn rule_editor(s: &Settings, sheet: &Sheet, k: &K) -> El {
    let Sheet::Rule { slug, approve, risk, max_lines, authors, labels, not_touching, problem } = sheet else { return col() };
    let field = |edit: &TextEdit, key: &str, prompt: &str, monospace: bool, access: fn(&mut Settings) -> &mut TextEdit| {
        let input = rune::text_input(edit, ui::field_font(13.0, monospace))
            .key(key.to_string())
            .single_line()
            .w_full()
            .color(k.text)
            .placeholder(prompt.to_string(), k.faint)
            .label(prompt.to_string())
            .on_edit(rune::edit_access(access))
            .on_submit(|s: &mut Settings, _| save_rule(s));
        ui::field_box(input, k)
    };
    fn rule_field(s: &mut Settings, which: u8) -> &mut TextEdit {
        match &mut s.sheet {
            Some(Sheet::Rule { max_lines, authors, labels, not_touching, .. }) => match which {
                0 => max_lines,
                1 => authors,
                2 => labels,
                _ => not_touching,
            },
            _ => &mut s.scratch,
        }
    }
    let verdict = ui::segmented("rule-verdict", "Verdict", &[("Approves", *approve), ("Any", !*approve)], true, k, |s: &mut Settings, i| {
        if let Some(Sheet::Rule { approve, .. }) = &mut s.sheet {
            *approve = i == 0;
        }
    });
    let risks = ["low", "medium", ""];
    let risk_pick = ui::segmented(
        "rule-risk",
        "Merge risk",
        &[("Low", risk == "low"), ("Medium or Lower", risk == "medium"), ("Any", risk.is_empty())],
        true,
        k,
        move |s: &mut Settings, i| {
            if let Some(Sheet::Rule { risk, .. }) = &mut s.sheet {
                *risk = risks[i].to_string();
            }
        },
    );
    let mut rows: Vec<El> = [
        ui::form_line("Verdict", row().child(verdict), k),
        ui::form_line("Merge risk", row().child(risk_pick), k),
        ui::form_line("At most, lines", field(max_lines, "rule-lines", "any", false, |s| rule_field(s, 0)), k),
        ui::form_line("By one of", field(authors, "rule-authors", "anyone", false, |s| rule_field(s, 1)), k),
        ui::form_line("Labeled one of", field(labels, "rule-labels", "any labels", false, |s| rule_field(s, 2)), k),
        ui::form_line("Not touching", field(not_touching, "rule-paths", "db/migrations/, auth/", true, |s| rule_field(s, 3)), k),
    ]
    .into_iter()
    .map(|l| ui::cell(vec![l]))
    .collect();
    if let Some(p) = problem {
        rows.push(ui::cell(vec![text(p.clone()).font(font(11.5, 400.0).lh(15.0)).color(k.error).wrap()]));
    }
    if let Some(e) = ui::error_line(s, "rule.edit", k) {
        rows.push(ui::cell(vec![e]));
    }
    let body = vec![ui::section("rule-form", None, rows, None, k)];
    let title = slug.as_ref().map(|sl| format!("When a review in {sl} posts itself")).unwrap_or_else(|| "When a review posts itself".into());
    let foot = ui::words("It posts itself only when every condition holds. Otherwise it waits for you, and the Inbox says why. Lists are comma-separated.", 11.5, k.muted);
    let buttons = vec![
        ui::button("rule-cancel", "Cancel", Look::Plain, true, k, |s: &mut Settings, _| s.sheet = None),
        ui::button("rule-save", "Save", Look::Primary, !s.is_busy("rule.edit"), k, |s: &mut Settings, _| save_rule(s)).default_button(),
    ];
    ui::sheet("rule-editor", &title, 480.0, body, Some(foot), buttons, k)
}

fn save_rule(s: &mut Settings) {
    let Some(Sheet::Rule { slug, approve, risk, max_lines, authors, labels, not_touching, problem }) = &mut s.sheet else { return };
    match patch::rule(*approve, risk, &max_lines.text, &authors.text, &labels.text, &not_touching.text) {
        Err(e) => *problem = Some(e),
        Ok(v) => {
            *problem = None;
            let path: Vec<String> = match slug {
                Some(sl) => vec!["repos".into(), sl.clone(), "post".into()],
                None => vec!["post".into()],
            };
            let p: Vec<&str> = path.iter().map(String::as_str).collect();
            let patch = patch::make("review", &p, Some(v));
            s.change("rule.edit", patch, |s| s.sheet = None);
        }
    }
}

// ---- what every settings tab shows ----

/// The configs' own errors, at the top of a tab: they stop what's below
/// (`ConfigProblems`).
pub fn problems(s: &Settings, k: &K) -> Vec<El> {
    let mut out = Vec::new();
    if let Some(e) = &s.load_error {
        out.push(ui::section("load-error", None, vec![ui::cell(vec![ui::alert(&format!("Can't read the settings: {e}"), k.error, k)])], None, k));
    }
    let v = s.v();
    let problems: Vec<(String, String)> = [
        jtext(&v["set_error"]).map(|e| ("settings.json".to_string(), e)),
        jtext(&v["build"]["error"]).map(|e| (s.lua("build"), e)),
        jtext(&v["review"]["error"]).map(|e| (s.lua("review"), e)),
    ]
    .into_iter()
    .flatten()
    .collect();
    if !problems.is_empty() {
        let rows = problems.iter().map(|(file, e)| ui::cell(vec![ui::alert(&format!("{file} doesn't load: {e}"), k.error, k)])).collect();
        out.push(ui::section("problems", None, rows, None, k));
    }
    out
}

/// Where these settings live, and what stays in the Lua (`WhereItLives`).
pub fn where_it_lives(s: &Settings, k: &K) -> El {
    let mut lines = vec![
        ui::words(
            format!(
                "Saved in {}, laid over {} and {}. A value set here wins over theirs, and {} takes it back out. Whatever a save replaces is kept in {}.",
                s.file(),
                s.lua("build"),
                s.lua("review"),
                s.revert_label("build"),
                s.history()
            ),
            11.5,
            k.muted,
        )
        .selectable(true),
        ui::words("Agents' programs and arguments, PATH, caches, the wall and effort functions stay in the Lua: edit those there.", 11.5, k.muted),
    ];
    let mut paths = col().w_full().gap(2.0).pt(2.0);
    for p in ["build", "review"] {
        if let Some(path) = jtext(&s.v()[p]["lua"]) {
            let missing = if s.lua_exists(p) { "" } else { " (not there)" };
            paths = paths.child(
                text(format!("{}: {}{missing}", s.lua(p), repos::short_path(&path))).font(crate::theme::mono(11.0).lh(15.0)).color(k.muted).wrap_anywhere().selectable(true),
            );
        }
    }
    lines.push(paths);
    ui::section("where", None, vec![col().w_full().px(12.0).py(10.0).gap(6.0).children(lines)], None, k)
}
