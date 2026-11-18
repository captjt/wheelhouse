//! A Review or a Build docket's panel (Panels.swift). Review: the PR, where
//! its round is, who reviews it, what needs you, Post, Preview and Dismiss,
//! and how earlier rounds went. Build: the repo and branch, its steps from
//! plan to PR (or an address round's), the builder and checker, the last
//! check, and its decisions.

use std::collections::HashSet;

use rune::{El, ScrollSpec, Tx, col, el, icon, row, spacer, text};
use rune_kit::{Kit, ScrollBar};
use serde_json::{Value, json};

use super::parts::{self, Req, Rich};
use super::{Main, Sheet, glyphs, widgets};
use crate::icons;
use crate::model::{self, BuildTask, Docket, HubState, LiveState, ReviewPr, arr, flag, int, plural, str_or};
use crate::theme::{self, K, font, mono};

#[derive(Default)]
pub struct Panels {
    /// Reviews showing every round, by full key.
    pub all_rounds: HashSet<String>,
    /// The preview a Preview sheet shows: its Markdown and its digest.
    pub preview: Option<(String, Option<String>)>,
}

// ---- reviews ----

/// What a PR's review is doing, in a couple of words.
pub fn phase_text(p: &ReviewPr, paused: bool) -> String {
    if p.attention().is_some_and(|a| a.kind == "ready") {
        return "Ready to post".into();
    }
    match p.state().as_str() {
        "queued" => {
            if model::text(&p.j["start_error"]).is_some() {
                "Didn't start".into()
            } else if paused {
                "Queued · paused".into()
            } else {
                "Queued".into()
            }
        }
        "reviewing" => match p.phase().as_deref() {
            Some("checking") => "Checking".into(),
            Some("awaiting") => "Ready to post".into(),
            _ => format!("Reviewing {}/{}", p.reviewers_done(), p.reviewers_total()),
        },
        "posted" => "Waiting on the author".into(),
        "unrequested" => "No longer asked".into(),
        "merged" => "Merged".into(),
        "closed" => "Closed".into(),
        "cleaned" => "Done".into(),
        s => s.to_string(),
    }
}

/// The checked review, in a line: what it asks and what it found.
pub fn draft_line(p: &ReviewPr) -> Option<String> {
    let d = p.draft()?;
    let event = str_or(&d["event"], "");
    let ask = match event {
        "APPROVE" => "Approve".to_string(),
        "REQUEST_CHANGES" => "Request changes".to_string(),
        e => e.to_string(),
    };
    let risk = model::text(&d["merge_risk"]).map(|r| format!("merge risk {r}"));
    Some([Some(ask).filter(|a| !a.is_empty()), Some(format!("{} blocking, {} non-blocking", int(&d["blocking"]).unwrap_or(0), int(&d["non_blocking"]).unwrap_or(0))), risk].into_iter().flatten().collect::<Vec<_>>().join(" · "))
}

fn tokens(n: i64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1e6)
    } else if n >= 1000 {
        format!("{}k", n / 1000)
    } else {
        n.to_string()
    }
}

fn attention_title(kind: &str, round: i64) -> String {
    match kind {
        "ready" => format!("Round {round} is ready to post"),
        "start-failed" => format!("Round {round} didn't start"),
        "stalled" => "The round stalled".into(),
        "pane-closed" => "A pane closed mid-round".into(),
        "blocked" => "An agent is blocked".into(),
        _ => "Something went wrong".into(),
    }
}

/// A Review docket's PR: where its round is, who reviews it, what needs
/// you, and how earlier rounds went. None when Review is off or the docket
/// has no PR.
pub fn review(s: &Main, st: &HubState, d: &Docket, k: &K, kit: &Kit) -> Option<El> {
    let rv = st.review();
    if !flag(&rv["enabled"]) {
        return None;
    }
    let p = st.pr_for(&d.id())?;
    let a = p.attention();
    let full = p.full_key();
    let url = p.url();
    let name = format!("{}#{}", str_or(&p.j["repo"], ""), int(&p.j["pr"]).unwrap_or(0));
    let link = {
        let u = url.clone();
        let mut t = row().key("review-pr").min_w(0.0).shrink().child(text(name.clone()).font(font(13.0, 600.0).lh(16.0)).color(if url.is_some() { k.accent } else { k.text }).truncate_middle());
        if url.is_some() {
            t = t.role("link").label(name.clone()).cursor(rune::Cursor::Pointer).on_press_tx(move |_: &mut Main, tx: &mut Tx| {
                if let Some(u) = &u {
                    let _ = tx.open_url(u);
                }
            });
        }
        t
    };
    let verdict = if p.state() == "reviewing" { None } else { p.verdict() };
    let risk = p.merge_risk();
    let mut c = parts::panel(a.is_some(), k).key("review-panel").child(
        row()
            .w_full()
            .gap(7.0)
            .center_y()
            .child(icon(&icons::PULL, 14.0).color(k.muted))
            .child(link)
            .child(text(format!("{}round {}", p.author().map(|a| format!("by {a} · ")).unwrap_or_default(), p.round().max(1))).font(font(12.5, 400.0).lh(16.0)).color(k.muted))
            .child(spacer())
            .child(parts::verdict_chip(verdict.as_deref(), k))
            .child(parts::risk_chip(risk.as_deref(), k)),
    );
    let state = p.state();
    if state == "reviewing" || state == "queued" {
        let at: i32 = if state != "reviewing" {
            -1
        } else {
            match p.phase().as_deref() {
                Some("checking") => 1,
                Some("awaiting") => 2,
                _ => 0,
            }
        };
        let auto = p.post().or_else(|| model::text(&rv["post"])).as_deref() == Some("auto");
        let first = format!("Reviewers {}/{}", p.reviewers_done(), p.reviewers_total());
        let steps: Vec<String> = if auto { vec![first, "Checker".into(), "Posted".into()] } else { vec![first, "Checker".into(), "Your call".into(), "Posted".into()] };
        c = c.child(parts::steps(&steps, if auto && at >= 2 { at - 1 } else { at }, k));
    } else {
        c = c.child(
            text(format!("{}{}", phase_text(p, flag(&rv["paused"])), p.posted_at().map(|t| format!(" · last posted {}", model::ago_long(Some(&t)))).unwrap_or_default()))
                .font(font(12.0, 400.0).lh(16.0))
                .color(k.muted),
        );
    }
    if state == "reviewing" && !p.reviewers().is_empty() {
        let mut r = row().gap(6.0).flex_wrap();
        for (name, _, effort) in p.reviewers() {
            r = r.child(theme::chip(&format!("{name}{}", effort.map(|e| format!(" · {e}")).unwrap_or_default()), k.text2, false));
        }
        if let Some(e) = model::text(&p.j["checker_effort"]) {
            r = r.child(theme::chip(&format!("checker · {e}"), k.text2, false));
        }
        c = c.child(r);
    }
    if let Some(a) = &a {
        let tone = if a.kind == "ready" || a.kind == "blocked" { LiveState::Blocked } else { LiveState::Error };
        let mut words = col().gap(4.0).grow(1.0).min_w(0.0).child(text(attention_title(&a.kind, a.round)).font(font(13.0, 600.0).lh(16.0)).color(k.text));
        if a.kind == "ready" && p.draft().is_some() {
            words = words.child(text(draft_line(p).unwrap_or_default()).font(font(12.5, 400.0).lh(16.0)).color(k.text2));
            if let Some(v) = p.draft().and_then(|d| model::text(&d["verdict"])) {
                words = words.child(text(v).font(font(12.5, 400.0).lh(16.0)).color(k.text2).wrap().max_lines(3));
            }
            if let Some(w) = p.waits_why() {
                words = words.child(text(format!("Waits for you: {w}")).font(font(12.5, 400.0).lh(16.0)).color(k.muted).wrap());
            }
        } else {
            words = words.child(text(a.text.clone()).font(font(12.5, 400.0).lh(16.0)).color(k.text2).wrap().selectable(true));
        }
        c = c.child(theme::callout(tone, row().w_full().gap(8.0).items_start().child(words).child(actions(s, p, k)), k));
    } else if state == "reviewing" {
        c = c.child(actions(s, p, k));
    }
    if let Some(i) = model::text(&p.j["implementation"]) {
        c = c.child(parts::md(&i, 12.5, k, kit));
    }
    let history = p.history();
    if !history.is_empty() {
        let all = s.panels.all_rounds.contains(&full);
        let mut h = col().w_full();
        for (i, r) in history.iter().enumerate() {
            if !all && i >= 2 {
                break;
            }
            h = h.child(round_row(r, k)).child(el().h(1.0).w_full().fill(k.line.alpha(0.5)).a11y_hidden());
        }
        if history.len() > 2 {
            let f = full.clone();
            h = h.child(el().pt(4.0).child(parts::link("review-rounds", &if all { "Fewer rounds".to_string() } else { format!("All {} rounds", history.len()) }, 12.0, k, move |s, _| {
                if !s.panels.all_rounds.remove(&f) {
                    s.panels.all_rounds.insert(f.clone());
                }
            })));
        }
        c = c.child(h);
    }
    Some(c)
}

/// An earlier round: its verdict, risk, cost, and what each reviewer found.
fn round_row(h: &Value, k: &K) -> El {
    let found = h["reviewers"]
        .as_object()
        .map(|o| {
            let mut names: Vec<&String> = o.keys().collect();
            names.sort();
            names
                .into_iter()
                .map(|n| {
                    let r = &o[n];
                    let kept = int(&r["kept"]).or_else(|| int(&r["drafted"])).unwrap_or(0);
                    let only = int(&r["only"]).unwrap_or(0);
                    format!("{n}: {kept}{}", if only > 0 { format!(" ({only} alone)") } else { String::new() })
                })
                .collect::<Vec<_>>()
                .join(" · ")
        })
        .unwrap_or_default();
    let first = match int(&h["blocking"]) {
        Some(b) => Some(format!("{b} blocking · {} non-blocking", int(&h["non_blocking"]).unwrap_or(0))),
        None => (h["by"].as_str() == Some("you")).then(|| "by you".to_string()),
    };
    let line = [first, (!found.is_empty()).then_some(found)].into_iter().flatten().collect::<Vec<_>>().join("  ·  ");
    let at = h["at"].as_str();
    let mut top = row()
        .w_full()
        .gap(8.0)
        .center_y()
        .child(text(format!("R{}", int(&h["round"]).unwrap_or(0))).font(mono(11.5).lh(15.0)).color(k.muted))
        .child(parts::verdict_chip(h["verdict"].as_str(), k))
        .child(parts::risk_chip(h["merge_risk"].as_str(), k))
        .child(spacer());
    if let Some(t) = int(&h["tokens"]["total"]) {
        top = top.child(text(format!("{} tok", tokens(t))).font(font(12.0, 400.0).lh(15.0)).color(k.faint));
    }
    top = top.child(text(model::ago(at)).font(font(12.0, 400.0).lh(15.0)).color(k.faint).help(model::stamp(at)));
    if let Some(u) = model::text(&h["url"]) {
        top = top.child(
            row()
                .role("link")
                .label("The review on GitHub")
                .help("The review on GitHub")
                .cursor(rune::Cursor::Pointer)
                .on_press_tx(move |_: &mut Main, tx: &mut Tx| {
                    let _ = tx.open_url(&u);
                })
                .child(icon(&glyphs::OPEN_SQUARE, 13.0).color(k.accent)),
        );
    }
    col().w_full().gap(3.0).py(6.0).child(top).child(text(line).font(font(12.0, 400.0).lh(15.0)).color(k.muted).truncate().pl(28.0).help("Findings kept from each reviewer"))
}

/// Post, Preview and Dismiss for a round. Post only when it waits for you:
/// the hub posts the draft you saw (its digest), or refuses.
pub fn actions(s: &Main, p: &ReviewPr, k: &K) -> El {
    if p.state() != "reviewing" {
        return el();
    }
    let full = p.full_key();
    let busy = |w: &str| s.busy.contains(&format!("{w}:{full}"));
    let any = busy("post") || busy("preview") || busy("dismiss");
    let mut r = row().gap(6.0).center_y();
    if p.ready() {
        let a = p.clone();
        r = r.child(widgets::button(&format!("post-{full}"), if busy("post") { "Posting…" } else { "Post" }, true, !any, k, move |s: &mut Main, _| post(s, &a, None)).help("Post the checked review to GitHub as you"));
    }
    if matches!(p.phase().as_deref(), Some("awaiting" | "checking")) {
        let a = p.clone();
        r = r.child(widgets::button(&format!("preview-{full}"), if busy("preview") { "Loading…" } else { "Preview" }, false, !any, k, move |s: &mut Main, _| preview(s, &a)));
    }
    let a = p.clone();
    r.child(widgets::button(&format!("dismiss-{full}"), if busy("dismiss") { "Dismissing…" } else { "Dismiss" }, false, !any, k, move |s: &mut Main, _| dismiss(s, &a)).help("End this round without posting anything"))
}

pub fn post(s: &mut Main, p: &ReviewPr, digest: Option<String>) {
    let (full, key) = (p.full_key(), p.key());
    let busy = format!("post:{full}");
    if !s.busy.insert(busy.clone()) {
        return;
    }
    s.sheet = None;
    let mut body = json!({ "key": full });
    if let Some(d) = digest.or_else(|| p.digest()) {
        body["digest"] = json!(d);
    }
    parts::call(s, Req::Post("/api/review/post".into(), body), move |s, r| {
        s.busy.remove(&busy);
        match r {
            Ok(_) => s.toast(format!("Posted the review of {key}"), false),
            Err(e) => s.toast(e, true),
        }
    });
}

pub fn dismiss(s: &mut Main, p: &ReviewPr) {
    let (full, key, round) = (p.full_key(), p.key(), p.round());
    let busy = format!("dismiss:{full}");
    if !s.busy.insert(busy.clone()) {
        return;
    }
    s.sheet = None;
    parts::call(s, Req::Post("/api/review/dismiss".into(), json!({ "key": full })), move |s, r| {
        s.busy.remove(&busy);
        match r {
            Ok(_) => s.toast(format!("Dismissed round {round} of {key}; nothing was posted"), false),
            Err(e) => s.toast(e, true),
        }
    });
}

fn preview(s: &mut Main, p: &ReviewPr) {
    let full = p.full_key();
    let busy = format!("preview:{full}");
    if !s.busy.insert(busy.clone()) {
        return;
    }
    parts::call(s, Req::Post("/api/review/preview".into(), json!({ "key": full })), move |s, r| {
        s.busy.remove(&busy);
        match r {
            Ok(v) => {
                s.panels.preview = Some((str_or(&v["preview"], "").to_string(), model::text(&v["digest"])));
                s.sheet = Some(Sheet::Preview(full));
            }
            Err(e) => s.toast(e, true),
        }
    });
}

/// The review as it would post.
pub fn preview_sheet(s: &Main, full: &str, k: &K, kit: &Kit) -> El {
    let p = s.state().and_then(|st| st.prs.iter().find(|p| p.full_key() == full).cloned());
    let (md, digest) = s.panels.preview.clone().unwrap_or_default();
    let ready = p.as_ref().is_some_and(|p| p.ready());
    let mut trailing = row();
    if let Some(u) = p.as_ref().and_then(|p| p.url()) {
        trailing = trailing.child(parts::link("preview-open", "Open the PR", 13.0, k, move |_, tx| {
            let _ = tx.open_url(&u);
        }));
    }
    let head = parts::sheet_head(
        text(match &p {
            Some(p) => format!("Review of {} · round {}, as it would post", p.key(), p.round()),
            None => "Review, as it would post".into(),
        })
        .font(font(13.0, 600.0).lh(16.0))
        .color(k.text),
        trailing,
    );
    let body = col().key("preview-scroll").w_full().grow(1.0).min_h(0.0).child(col().w_full().padding(20.0).child(parts::md(&md, 13.0, k, kit))).scroll(ScrollSpec { stick_bottom: false }).scroll_bar(kit);
    let mut buttons = row().gap(8.0).child(widgets::button("preview-close", "Close", false, true, k, |s: &mut Main, _| s.sheet = None));
    if let Some(p) = p.clone() {
        let d = p.clone();
        buttons = buttons.child(widgets::button("preview-dismiss", "Dismiss", false, true, k, move |s: &mut Main, _| dismiss(s, &d)));
        if ready {
            buttons = buttons.child(widgets::button("preview-post", "Post Review", true, true, k, move |s: &mut Main, _| post(s, &p, digest.clone())));
        }
    }
    let foot = row()
        .w_full()
        .padding(16.0)
        .gap(10.0)
        .center_y()
        .child(text(if ready { "Posts from your GitHub account, with the footer above." } else { "The checker hasn't finished; this is its draft so far." }).font(font(12.0, 400.0).lh(16.0)).color(k.muted).shrink())
        .child(spacer())
        .child(buttons);
    parts::sheet_card("Review preview", 760.0, Some(680.0), k).child(head).child(parts::divider(k)).child(body).child(parts::divider(k)).child(foot)
}

// ---- builds ----

/// Where a Build is, in a few words (Panels.swift's buildPhase).
pub fn build_phase(t: &BuildTask) -> String {
    let asks = model::strings(&t.j["asks"]);
    let rounds = int(&t.j["rounds"]).unwrap_or(0);
    match t.state().as_str() {
        "queued" => return if model::text(&t.j["start_error"]).is_some() { "Didn't start".into() } else { "Queued".into() },
        "open" => {
            return if asks.iter().any(|a| a == "address") {
                "Feedback: your call".into()
            } else if rounds > 0 {
                format!("PR open · {}", plural(rounds as usize, "round"))
            } else {
                "PR open".into()
            };
        }
        "merged" => return "Merged".into(),
        "closed" => return "Closed".into(),
        "discarded" => return "Discarded".into(),
        _ => {}
    }
    let n = int(&t.j["round"]["n"]).unwrap_or(rounds);
    match t.phase().as_deref().unwrap_or("") {
        "planning" => "Planning".into(),
        "not_ready" => "Not ready: your call".into(),
        "plan_gate" => "Plan: your call".into(),
        "building" => "Building".into(),
        "checking" => format!("Checking (check {})", int(&t.j["check_n"]).unwrap_or(0)),
        "prereview" => format!("Pre-review {}", int(&t.j["prereview"]["n"]).unwrap_or(1)),
        "preview_gate" => "Preview: your call".into(),
        "open_gate" => "Ready to open".into(),
        "opening" => "Opening".into(),
        "addressing" => format!("Round {n}: addressing"),
        "push_gate" => format!("Round {n}: your call"),
        "pushing" => "Pushing".into(),
        _ => "Working".into(),
    }
}

/// A Build docket's panel. None when it isn't building.
pub fn build(st: &HubState, d: &Docket, k: &K) -> Option<El> {
    let t = st.task_for(&d.id())?;
    let id = d.id();
    let state = t.state();
    let phase = t.phase().unwrap_or_default();
    let base = model::text(&t.j["base"]);
    let check_n = int(&t.j["check_n"]).unwrap_or(0);

    let mut top = row()
        .w_full()
        .gap(7.0)
        .center_y()
        .child(icon(&icons::HAMMER, 14.0).color(k.muted))
        .child(text("Build").font(font(13.0, 600.0).lh(16.0)).color(k.text))
        .child(text(format!("into {}", t.repo())).font(font(12.5, 400.0).lh(16.0)).color(k.muted));
    if let Some(b) = t.branch() {
        top = top.child(text(format!("· {b}")).font(mono(12.0).lh(16.0)).color(k.muted).truncate().shrink().help(format!("from {}", base.clone().unwrap_or_else(|| "main".into()))));
    }
    top = top.child(spacer());
    if let Some(u) = t.pr_url() {
        top = top.child(parts::link("build-pr", &format!("PR #{}", t.pr_number().unwrap_or(0)), 12.5, k, move |_, tx| {
            let _ = tx.open_url(&u);
        }));
    }
    if state == "active" || state == "queued" {
        let a = id.clone();
        top = top.child(widgets::button("build-discard", "Discard", false, true, k, move |s: &mut Main, _| discard(s, &a)).help("Stop the Build: its panes close and its worktree goes; the branch stays"));
    }
    let mut c = parts::panel(t.attention().is_some(), k).key("build-panel").child(top);

    let round = &t.j["round"];
    if let Some(n) = int(&round["n"]) {
        let steps: [(String, &[&str]); 5] = [
            ("Feedback".into(), &[]),
            (format!("Round {n}"), &["addressing"]),
            ("Check".into(), &["checking"]),
            ("Your call".into(), &["push_gate"]),
            ("Pushed".into(), &["pushing"]),
        ];
        let at = steps.iter().position(|(_, ps)| ps.contains(&phase.as_str())).unwrap_or(1) as i32;
        c = c.child(parts::steps(&steps.iter().map(|(l, _)| l.clone()).collect::<Vec<_>>(), at, k));
        if let Some(sum) = model::text(&round["summary"]) {
            let update = match round["update"].as_str() {
                Some("rebase") => format!(" · rebasing onto {}", base.clone().unwrap_or_else(|| "the base".into())),
                Some(_) => format!(" · merging {}", base.clone().unwrap_or_else(|| "the base".into())),
                None => String::new(),
            };
            c = c.child(text(format!("Round {n}: {sum}{update}")).font(font(12.0, 400.0).lh(16.0)).color(k.muted).wrap());
        }
    } else if matches!(state.as_str(), "active" | "queued" | "open") {
        let steps: [(&str, &[&str]); 5] = [
            ("Plan", &["planning", "not_ready"]),
            ("Your call", &["plan_gate"]),
            ("Build", &["building"]),
            ("Check", &["checking", "prereview", "preview_gate"]),
            ("Open", &["open_gate", "opening"]),
        ];
        let at = if state == "open" || state == "merged" {
            steps.len() as i32
        } else if state == "queued" {
            -1
        } else {
            steps.iter().position(|(_, ps)| ps.contains(&phase.as_str())).unwrap_or(0) as i32
        };
        let labels: Vec<String> = steps
            .iter()
            .enumerate()
            .map(|(i, (l, _))| {
                let here = i as i32 == at;
                if *l == "Check" && here && check_n > 0 {
                    format!("Check {check_n}")
                } else if *l == "Your call" && here {
                    "Plan: your call".into()
                } else {
                    l.to_string()
                }
            })
            .collect();
        c = c.child(parts::steps(&labels, at, k));
    } else {
        c = c.child(text(format!("{}{}", build_phase(t), model::text(&t.j["why"]).map(|w| format!(": {w}")).unwrap_or_default())).font(font(12.0, 400.0).lh(16.0)).color(k.muted).wrap());
    }

    if let Some(a) = t.attention() {
        let title = match a.kind.as_str() {
            "stalled" => "The Build stalled",
            "start-failed" => "The Build didn't start",
            "pane-closed" => "An agent's pane closed",
            "blocked" => "An agent is blocked",
            _ => "Something went wrong",
        };
        let words = col()
            .gap(4.0)
            .grow(1.0)
            .min_w(0.0)
            .child(text(title).font(font(13.0, 600.0).lh(16.0)).color(k.text))
            .child(text(a.text.clone()).font(font(12.5, 400.0).lh(16.0)).color(k.text2).wrap().selectable(true));
        c = c.child(theme::callout(if a.kind == "blocked" { LiveState::Blocked } else { LiveState::Error }, words, k));
    }

    let who = |j: &Value, role: &str| format!("{role} · {}{}", str_or(&j["agent"], "claude"), model::text(&j["effort"]).map(|e| format!(" · {e}")).unwrap_or_default());
    let mut chips = row().gap(6.0).flex_wrap().child(theme::chip(&who(&t.j["builder"], "builder"), k.text2, false)).child(theme::chip(&who(&t.j["checker"], "checker"), k.text2, false));
    let plan = int(&t.j["plan_version"]).unwrap_or(0);
    if plan > 0 {
        chips = chips.child(theme::chip(&format!("plan v{plan}"), k.text2, false));
    }
    let fixes = int(&t.j["fix_used"]).unwrap_or(0);
    if fixes > 0 {
        chips = chips.child(theme::chip(&plural(fixes as usize, "fix round"), k.text2, false));
    }
    c = c.child(chips);

    let last = &t.j["last_check"];
    if !last.is_null() {
        let n = arr(&last["findings"]).len();
        c = c.child(
            Rich::new()
                .push(&format!("Check {}: ", int(&last["n"]).unwrap_or(0)), |s| s.weight(600.0).color(k.text))
                .push(if last["verdict"].as_str() == Some("pass") { "passed. " } else { "found problems. " }, |s| s.color(k.text))
                .push(str_or(&last["summary"], ""), |s| s.color(k.text))
                .push(&if n > 0 { format!(" ({})", plural(n, "finding")) } else { String::new() }, |s| s.color(k.muted))
                .el()
                .font(font(12.5, 400.0).lh(17.0))
                .wrap(),
        );
    }

    let decisions = arr(&t.j["decisions"]);
    if !decisions.is_empty() {
        let mut list = col().w_full().gap(4.0);
        for x in decisions {
            let answered = flag(&x["answered"]);
            list = list.child(
                row()
                    .w_full()
                    .gap(8.0)
                    .child(text(str_or(&x["id"], "").to_string()).font(mono(11.5).lh(16.0)).color(k.muted))
                    .child(text(str_or(&x["question"], "").to_string()).font(font(12.5, 400.0).lh(16.0)).color(k.text).wrap().shrink().grow(1.0).min_w(0.0))
                    .child(text(if answered { str_or(&x["label"], "answered").to_string() } else { "open".into() }).font(font(12.5, 400.0).lh(16.0)).color(if answered { k.working } else { k.blocked })),
            );
        }
        c = c.child(list);
    }
    Some(c)
}

fn discard(s: &mut Main, id: &str) {
    let id = id.to_string();
    parts::call(s, Req::Post("/api/build/discard".into(), json!({ "docket": id })), move |s, r| match r {
        Ok(_) => s.toast(format!("Discarded {id}'s Build; its branch stays"), false),
        Err(e) => s.toast(e, true),
    });
}
