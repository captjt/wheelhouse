//! Agents (`AgentsSettings`): who builds, checks and pre-reviews a Build,
//! how many rounds and Builds at once, its PRs; Review's reviewers and
//! checker, and how many reviews at once.

use rune::El;
use rune_kit::Kit;
use serde_json::{Value, json};

use super::ui::{self, choice};
use super::{Settings, orders, patch};
use crate::model::{self, flag};
use crate::theme::K;

pub fn view(s: &Settings, k: &K, kit: &Kit) -> El {
    let (b, r) = (s.build().clone(), s.review().clone());
    let mut sections = orders::problems(s, k);
    if !b.is_null() {
        let mut rows = vec![
            agent(s, "Builder", "builder", "Writes the plan, then builds it.", "build", k),
            agent(s, "Checker", "checker", "Judges the branch against the intent and the plan.", "build", k),
            prereview(s, &b, k),
        ];
        if !model::strings(&b["prereview"]).is_empty() {
            rows.push(stepper(
                s,
                "Pre-review rounds",
                "prereview_loops",
                model::int(&b["prereview_loops"]).unwrap_or(1),
                (1, 5),
                &["prereview", "loops"],
                "How many times the blind reviewers look again after fixes.",
                "build",
                k,
            ));
        }
        rows.push(stepper(
            s,
            "Fix rounds",
            "fix_loops",
            model::int(&b["fix_loops"]).unwrap_or(2),
            (0, 6),
            &["fix_loops"],
            "How often the checker, or a failing check, can send the work back before it asks you.",
            "build",
            k,
        ));
        rows.push(stepper(s, "Builds at once", "max_tasks", model::int(&b["max_tasks"]).unwrap_or(2), (1, 8), &["max_tasks"], "More wait in the queue.", "build", k));
        sections.push(ui::section("agents-build", Some("Build"), rows, None, k));

        let draft = flag(&b["pr"]["draft"]);
        let sw = ui::switch(draft, "Open PRs as drafts", !s.is_busy("pr.draft"), k, kit, move |s: &mut Settings| {
            s.change("pr.draft", patch::make("build", &["pr", "draft"], Some(json!(!draft))), |_| {});
        });
        let drafts = ui::cell(ui::some(vec![
            Some(ui::labeled(ui::described("Open PRs as drafts", if draft { "You mark each ready for review on GitHub." } else { "Each opens ready for review." }, k), sw)),
            ui::set_here(s, "build", &["pr", "draft"], "pr.draft", k),
            ui::error_line(s, "pr.draft", k),
        ]));
        let rebase = model::str_or(&b["update"], "merge") == "rebase";
        let pick = ui::segmented("update", "Bring in the base", &[("Merge", !rebase), ("Rebase", rebase)], !s.is_busy("update"), k, |s: &mut Settings, i| {
            s.change("update", patch::make("build", &["update"], Some(json!(if i == 0 { "merge" } else { "rebase" }))), |_| {});
        });
        let sentence = if rebase { "Rebase the branch onto it, and force-push with lease." } else { "Merge it into the branch: no force-push." };
        let update = ui::cell(ui::some(vec![
            Some(ui::labeled(ui::described("Bring in the base", sentence, k), pick)),
            ui::set_here(s, "build", &["update"], "update", k),
            ui::error_line(s, "update", k),
        ]));
        sections.push(ui::section("agents-prs", Some("Pull requests"), vec![drafts, update], None, k));
    }
    if !r.is_null() {
        let names =
            model::arr(&r["reviewers"]).iter().map(|x| format!("{} ({})", model::str_or(&x["name"], ""), model::app_name(x["agent"].as_str()))).collect::<Vec<_>>().join(", ");
        let reviewers = ui::cell(vec![ui::labeled(
            ui::described("Reviewers", &format!("Each reviews blind, in its own pane. Their names, agents and effort are in {}.", s.lua("review")), k),
            ui::words(names, 13.0, k.text).align(rune::Align::End).max_w(260.0),
        )]);
        let rows = vec![
            reviewers,
            agent(s, "Checker", "checker", "Merges the reviewers' drafts into one review, and rates the merge risk.", "review", k),
            stepper(s, "Reviews at once", "max_rounds", model::int(&r["max_rounds"]).unwrap_or(3), (1, 8), &["max_rounds"], "More wait in the queue.", "review", k),
        ];
        sections.push(ui::section("agents-review", Some("Review"), rows, None, k));
    }
    sections.push(orders::where_it_lives(s, k));
    ui::form("agents-form", 560.0, sections, kit)
}

/// Which agent does a part: Claude Code or Codex. Set as `{agent}`, so
/// whatever else the Lua gives it (effort, args) stays.
fn agent(s: &Settings, title: &str, field: &'static str, sentence: &str, part: &'static str, k: &K) -> El {
    let cfg = if part == "build" { s.build() } else { s.review() };
    let now = cfg[field].as_str().or_else(|| cfg[field]["agent"].as_str()).unwrap_or("claude").to_string();
    let key = format!("{part}.{field}");
    let k2 = key.clone();
    let pick = ui::popup(
        &format!("{part}-{field}"),
        title,
        &model::app_name(Some(&now)),
        vec![choice("claude", "Claude Code"), choice("codex", "Codex")],
        &now,
        !s.is_busy(&key),
        k,
        move |s: &mut Settings, v| s.change(&k2, patch::make(part, &[field], Some(json!({ "agent": v }))), |_| {}),
    );
    ui::cell(ui::some(vec![Some(ui::labeled(ui::described(title, sentence, k), pick)), ui::set_here(s, part, &[field], &key, k), ui::error_line(s, &key, k)]))
}

/// Who reads the branch blind before its PR opens.
fn prereview(s: &Settings, b: &Value, k: &K) -> El {
    let now = model::strings(&b["prereview"]);
    let mut sorted = now.clone();
    sorted.sort();
    let tag = match sorted.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        [] => "none",
        ["claude"] => "claude",
        ["codex"] => "codex",
        ["claude", "codex"] => "both",
        _ => "other",
    };
    let mut choices = vec![choice("none", "None"), choice("claude", "Claude Code"), choice("codex", "Codex"), choice("both", "Claude Code and Codex")];
    let others = now.iter().map(|a| model::app_name(Some(a))).collect::<Vec<_>>().join(", ");
    if tag == "other" {
        choices.push(choice("other", &others));
    }
    let value = match tag {
        "none" => "None".to_string(),
        "claude" => "Claude Code".into(),
        "codex" => "Codex".into(),
        "both" => "Claude Code and Codex".into(),
        _ => others,
    };
    let pick = ui::popup("prereview", "Pre-review", &value, choices, tag, !s.is_busy("prereview"), k, |s: &mut Settings, v| {
        let list: Vec<&str> = match v {
            "claude" => vec!["claude"],
            "codex" => vec!["codex"],
            "both" => vec!["claude", "codex"],
            "other" => return,
            _ => vec![],
        };
        s.change("prereview", patch::make("build", &["prereview", "reviewers"], Some(json!(list))), |_| {});
    });
    ui::cell(ui::some(vec![
        Some(ui::labeled(ui::described("Pre-review", "Blind reviewers read the branch before the PR opens, and the builder fixes what they find.", k), pick)),
        ui::set_here(s, "build", &["prereview"], "prereview", k),
        ui::error_line(s, "prereview", k),
    ]))
}

/// A number, stepped within its range.
#[allow(clippy::too_many_arguments)]
fn stepper(s: &Settings, title: &str, field: &str, value: i64, range: (i64, i64), path: &[&str], sentence: &str, part: &'static str, k: &K) -> El {
    let key = format!("{part}.{field}");
    let (k2, p2): (String, Vec<String>) = (key.clone(), path.iter().map(|p| p.to_string()).collect());
    let control = ui::stepper(&format!("{part}-{field}"), title, value, range.0, range.1, !s.is_busy(&key), k, move |s: &mut Settings, v| {
        let p: Vec<&str> = p2.iter().map(String::as_str).collect();
        s.change(&k2, patch::make(part, &p, Some(json!(v))), |_| {});
    });
    ui::cell(ui::some(vec![Some(ui::labeled(ui::described(title, sentence, k), control)), ui::set_here(s, part, path, &key, k), ui::error_line(s, &key, k)]))
}
