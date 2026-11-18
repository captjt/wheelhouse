//! A docket's activity (Activity.swift): each entry of its record, said the
//! way the Mac app says it, with how it's known (witnessed, checkable, said).

use rune::{Color, El, Span, col, el, icon, rich, row, spacer, text};
use rune_kit::Kit;

use super::Main;
use super::docket::markdown;
use crate::icons;
use crate::model::{self, Docket, DocketPage, Entry, HubState, LiveState, Status};
use crate::theme::{self, K, font};

/// Who wrote it, as an avatar.
#[derive(Clone)]
enum Avatar {
    Hub,
    You(String),
    App(String),
    Pr,
    Issue,
    Live(LiveState),
}

fn avatar(a: &Avatar, k: &K) -> El {
    match a {
        Avatar::Hub => theme::hub_avatar(16.0, k),
        Avatar::You(n) => theme::person_avatar(n, 16.0, k),
        Avatar::App(app) => theme::app_avatar(Some(app), 16.0, k),
        Avatar::Pr => row().size(16.0, 16.0).radius(4.0).fill(k.recessed).center().child(icon(&icons::PULL, 11.0).color(k.muted)),
        Avatar::Issue => row().size(16.0, 16.0).radius(4.0).fill(theme::hex("#5e6ad2")).center().child(icon(&icons::LINEAR, 11.0).color(Color::WHITE)),
        Avatar::Live(s) => theme::live_icon(Some(*s), 16.0, k),
    }
}

/// A run of a line: bold, plain, or monospace.
enum Bit {
    B(String),
    T(String),
    M(String),
}

enum Shape {
    Sys(Avatar, Vec<Bit>, Option<&'static str>),
    Comment(Avatar, String, String, String, Option<&'static str>),
    File(Avatar, String, String),
    Skip,
}

fn shape(e: &Entry, d: &Docket, owner: &str) -> Shape {
    use Bit::{B, M, T};
    let data = e.data();
    let s = |v: &serde_json::Value| model::text(v);
    let agent = model::app_name(d.agent_app().as_deref());
    let you = Avatar::You(owner.to_string());
    let app = Avatar::App(d.agent_app().unwrap_or_default());
    let (writer, wa) = match e.actor_type().as_str() {
        "agent" => (model::app_name(Some(&e.actor_app())), Avatar::App(e.actor_app())),
        "person" => ("You".to_string(), you.clone()),
        _ => ("Wheelhouse".to_string(), Avatar::Hub),
    };
    let person = e.actor_type() == "person";
    match e.kind().as_str() {
        "open" => match s(&data["protocol"]).as_deref() {
            Some("review") => Shape::Sys(
                Avatar::Pr,
                vec![B("Wheelhouse".into()), T(format!(" opened this Review docket{}", s(&data["review"]["key"]).map(|k| format!(" for {k}")).unwrap_or_default()))],
                None,
            ),
            Some("agent") => Shape::Sys(
                Avatar::Hub,
                vec![B("Wheelhouse".into()), T(format!(" picked up {agent}{}", d.agent_cwd().map(|c| format!(" in {}", short_path(&c))).unwrap_or_default()))],
                None,
            ),
            _ if person => Shape::Sys(you, vec![B("You".into()), T(" opened this docket".into())], None),
            _ => Shape::Sys(Avatar::Hub, vec![B("Wheelhouse".into()), T(" opened this docket".into())], None),
        },
        "claim" => {
            let src = s(&data["source"]);
            match src.as_deref() {
                Some("rex-reviewer") => Shape::Sys(
                    Avatar::Hub,
                    vec![
                        B("rex-reviewer".into()),
                        T(format!(" started its {}{}", model::str_or(&data["role"], ""), model::int(&data["round"]).map(|r| format!(" for round {r}")).unwrap_or_default())),
                    ],
                    None,
                ),
                Some("wheelhouse") if s(&data["role"]).is_some() => Shape::Sys(
                    Avatar::Hub,
                    vec![
                        B("Wheelhouse".into()),
                        T(format!(
                            " {} the {}{}",
                            if data["round"].is_number() { "started" } else { "found" },
                            model::str_or(&data["role"], ""),
                            model::int(&data["round"]).map(|r| format!(" for round {r}")).unwrap_or_default()
                        )),
                    ],
                    None,
                ),
                Some("wheelhouse") => Shape::Sys(Avatar::Hub, vec![B("Wheelhouse".into()), T(format!(" started {agent} in Rex's wheelhouse session"))], None),
                _ => {
                    let sess: String = model::str_or(&data["session"], "").chars().take(8).collect();
                    let verb = match src.as_deref() {
                        Some("resume") => " resumed session ",
                        Some("clear") => " cleared into session ",
                        _ => " started session ",
                    };
                    Shape::Sys(Avatar::Hub, vec![B(agent), T(verb.into()), M(sess)], None)
                }
            }
        }
        "question" => Shape::Comment(
            wa,
            writer,
            format!(
                "asked {}{} (#{})",
                if model::str_or(&data["to"], "") == "me" { "you".into() } else { model::str_or(&data["to"], "").to_string() },
                s(&data["subject"]).map(|x| format!(" · {x}")).unwrap_or_default(),
                model::int(&data["message"]).unwrap_or(0)
            ),
            format!("{}\n\nOptions: {}", e.body().unwrap_or_default(), model::strings(&data["options"]).join(" · ")),
            Some("ask"),
        ),
        "decision" => {
            let choice = model::str_or(&data["choice"], "").to_string();
            let body = e.body().filter(|b| *b != choice).unwrap_or_else(|| format!("**{choice}**"));
            Shape::Comment(wa, writer, format!("chose “{choice}” (#{})", model::int(&data["replies_to"]).unwrap_or(0)), body, person.then_some("you"))
        }
        "note" => {
            let body = e.body().unwrap_or_default();
            if data["message"].is_number() {
                let verb = model::int(&data["replies_to"]).map(|r| format!("replied to #{r}")).unwrap_or_else(|| {
                    if model::str_or(&data["to"], "") == "me" {
                        format!("wrote to you{}", s(&data["subject"]).map(|x| format!(" · {x}")).unwrap_or_default())
                    } else {
                        format!("wrote to {}", model::str_or(&data["to"], ""))
                    }
                });
                return Shape::Comment(wa, writer, verb, body, person.then_some("you"));
            }
            match model::str_or(&data["via"], "") {
                "prompt" => Shape::Comment(you, "You".into(), format!("asked {agent}"), body, Some("you")),
                "message" => Shape::Comment(Avatar::Hub, "rex-inbox".into(), "delivered a message".into(), body, None),
                _ if e.actor_type() == "agent" => Shape::Comment(app, agent, "noted".into(), body, None),
                _ => Shape::Comment(you, "You".into(), "noted".into(), body, Some("you")),
            }
        }
        "turn" => match e.body().filter(|b| !b.trim().is_empty()) {
            Some(b) => Shape::Comment(app, agent, "finished a turn".into(), b, Some("agent")),
            None => {
                let st = s(&data["state"]).filter(|x| x != "done");
                Shape::Sys(app, vec![B(agent), T(format!(" finished a turn{}", st.map(|x| format!(" ({x})")).unwrap_or_default()))], None)
            }
        },
        "intent" => match s(&data["source"]).as_deref() {
            Some("linear") => {
                Shape::Sys(Avatar::Issue, vec![B("Linear".into()), T(format!(" set the intent from {}'s description", model::str_or(&data["key"], "the issue")))], Some("intent"))
            }
            Some("review") => Shape::Sys(Avatar::Hub, vec![B("Wheelhouse".into()), T(" wrote the intent from the PR".into())], Some("intent")),
            _ => Shape::Sys(wa, vec![B(writer), T(if person { " updated the intent".into() } else { " proposed the intent".into() })], Some("intent")),
        },
        "link" => {
            let key = model::str_or(&data["key"], "").to_string();
            match model::str_or(&data["kind"], "") {
                "linear" => Shape::Sys(
                    if person { you } else { Avatar::Issue },
                    vec![
                        B(if person { "You".into() } else { "Wheelhouse".into() }),
                        T(" linked ".into()),
                        B(key),
                        T(s(&data["title"]).map(|t| format!(" · {t}")).unwrap_or_default()),
                    ],
                    None,
                ),
                "github-pr" => Shape::Sys(Avatar::Pr, vec![T("Following ".into()), B(key), T(" on GitHub".into())], None),
                "github-review" => Shape::Sys(Avatar::Pr, vec![T(format!("Round {}'s review is on GitHub", model::int(&data["round"]).unwrap_or(0)))], Some("approved")),
                _ => Shape::Sys(Avatar::Hub, vec![T(format!("Linked {}", e.body().unwrap_or_default()))], None),
            }
        }
        "observed" => match model::str_or(&data["what"], "") {
            "review" if model::str_or(&data["source"], "") == "wheelhouse" => Shape::Skip,
            "review-event" => {
                let ev = model::str_or(&data["event"], "");
                Shape::Sys(
                    Avatar::Pr,
                    vec![T(e.body().unwrap_or_default())],
                    if ev == "posted" || ev == "merged" {
                        Some("approved")
                    } else if ev == "dismissed" || ev == "post-refused" {
                        Some("denied")
                    } else {
                        None
                    },
                )
            }
            "review-attention" => {
                let blocked = matches!(model::str_or(&data["kind"], ""), "ready" | "blocked");
                Shape::Sys(
                    Avatar::Live(if blocked { LiveState::Blocked } else { LiveState::Error }),
                    vec![T(e.body().unwrap_or_default())],
                    Some(if blocked { "blocked" } else { "error" }),
                )
            }
            "linear-comment" => {
                Shape::Comment(Avatar::Issue, model::str_or(&data["author"], "Someone").to_string(), "commented in Linear".into(), e.body().unwrap_or_default(), Some("linear"))
            }
            "error" => Shape::Sys(Avatar::Live(LiveState::Error), vec![B(agent), T(format!(" failed{}", e.body().map(|b| format!(": {b}")).unwrap_or_default()))], Some("error")),
            _ => Shape::Sys(
                Avatar::Live(LiveState::Blocked),
                vec![
                    B(agent),
                    T(format!(" waited on you{}{}", s(&data["kind"]).map(|x| format!(" ({x})")).unwrap_or_default(), e.body().map(|b| format!(": {b}")).unwrap_or_default())),
                ],
                Some("blocked"),
            ),
        },
        "set" => {
            let mut parts = vec![];
            if let Some(t) = s(&data["title"]) {
                parts.push(if model::str_or(&data["title_source"], "") == "prompt" { "named it from your prompt".to_string() } else { format!("renamed it to “{t}”") });
            }
            if let Some(st) = s(&data["status"]) {
                parts.push(if st == "done" && model::str_or(&data["on"], "") == "word" {
                    format!("closed it on your word: “{}”", s(&data["reason"]).or(e.body()).unwrap_or_default())
                } else {
                    format!("moved it to {}", Status::parse(&st).label())
                });
            }
            if data.as_object().is_some_and(|o| o.contains_key("hold")) {
                parts.push(s(&data["hold"]).map(|h| format!("put it on hold: “{h}”")).unwrap_or_else(|| "resumed it".into()));
            }
            let tone = (s(&data["hold"]).is_some() || model::str_or(&data["on"], "") == "word").then_some("blocked");
            Shape::Sys(if person { you } else { Avatar::Hub }, vec![B(if person { "You".into() } else { "Wheelhouse".into() }), T(format!(" {}", parts.join(" and ")))], tone)
        }
        "check" => {
            let ok = model::flag(&data["ok"]);
            let optional = data["required"].as_bool() == Some(false);
            let how = if ok {
                "passed".to_string()
            } else if data["exit"].is_null() {
                "timed out".into()
            } else {
                format!("failed, exit {}", model::int(&data["exit"]).unwrap_or(1))
            };
            let head: String = model::str_or(&data["head"], "").chars().take(7).collect();
            Shape::Sys(
                Avatar::Hub,
                vec![
                    B("Wheelhouse".into()),
                    T(" ran ".into()),
                    M(model::str_or(&data["name"], "a check").to_string()),
                    T(format!("{} · {how} at ", if optional { " (optional)" } else { "" })),
                    M(head),
                ],
                if ok {
                    Some("approved")
                } else if optional {
                    None
                } else {
                    Some("error")
                },
            )
        }
        "verdict" => {
            let passed = model::str_or(&data["verdict"], "") == "pass";
            let n = model::int(&data["findings"]).unwrap_or(model::arr(&data["findings"]).len() as i64);
            Shape::Comment(
                wa,
                writer,
                format!(
                    "{}: {}{}",
                    model::str_or(&data["role"], "checker"),
                    if passed { "passed" } else { "found problems" },
                    if n > 0 { format!(" · {}", model::plural(n as usize, "finding")) } else { String::new() }
                ),
                e.body().unwrap_or_default(),
                None,
            )
        }
        "approval" => {
            if model::str_or(&data["kind"], "") == "review" {
                let posted = model::str_or(&data["decision"], "") == "post";
                return Shape::Sys(
                    you,
                    vec![B("You".into()), T(if posted { " posted the review".into() } else { " dismissed the review".into() })],
                    Some(if posted { "approved" } else { "denied" }),
                );
            }
            let ok = model::str_or(&data["decision"], "") == "approve";
            let mut bits = vec![B("You".into()), T(format!(" {} {agent}'s {}", if ok { "approved" } else { "denied" }, model::str_or(&data["kind"], "prompt")))];
            if let Some(b) = e.body() {
                bits.push(M(format!(": {b}")));
            }
            Shape::Sys(you, bits, Some(if ok { "approved" } else { "denied" }))
        }
        "artifact" => Shape::File(wa, writer, e.body().or_else(|| s(&data["name"])).unwrap_or_else(|| "a file".into())),
        "release" => Shape::Sys(Avatar::Hub, vec![B("Wheelhouse".into()), T(format!(" saw the session end: {}", model::str_or(&data["reason"], "released")))], None),
        k => Shape::Sys(Avatar::Hub, vec![T(format!("{k}{}", e.body().map(|b| format!(": {b}")).unwrap_or_default()))], None),
    }
}

fn short_path(p: &str) -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    if !home.is_empty() && p.starts_with(&home) { format!("~{}", &p[home.len()..]) } else { p.to_string() }
}

fn tone_color(t: Option<&str>, k: &K) -> Option<Color> {
    match t {
        Some("blocked") => Some(k.blocked),
        Some("error" | "denied") => Some(k.error),
        Some("approved") => Some(k.working),
        _ => None,
    }
}

fn mark(m: &str, k: &K) -> El {
    let (c, why) = match m {
        "witnessed" => (k.working, "Wheelhouse saw it happen and wrote it down."),
        "checkable" => (k.accent, "Read from a source anyone can read again (GitHub, Linear)."),
        _ => (k.faint, "A person or an agent said it: true if you trust them."),
    };
    text(m.to_string()).font(font(10.5, 500.0).lh(14.0)).color(c).help(why.to_string())
}

fn line(bits: &[Bit], when: &str, tone: Option<Color>, k: &K) -> El {
    let mut s = String::new();
    let mut spans = vec![];
    for b in bits {
        let start = s.len();
        let (t, bold, mono) = match b {
            Bit::B(t) => (t, true, false),
            Bit::T(t) => (t, false, false),
            Bit::M(t) => (t, false, true),
        };
        s.push_str(t);
        let mut span = Span::new(start, s.len());
        if bold {
            span = span.weight(600.0).color(k.text);
        } else if let Some(c) = tone {
            span = span.color(c);
        }
        if mono {
            span.mono = true;
        }
        spans.push(span);
    }
    let start = s.len();
    s.push_str(&format!(" · {when}"));
    spans.push(Span::new(start, s.len()).color(k.faint));
    rich(s, spans).font(font(13.0, 400.0).lh(18.0)).color(k.text2).wrap()
}

pub fn activity(s: &Main, st: &HubState, d: &Docket, p: &DocketPage, k: &K, kit: &Kit) -> El {
    let ok = p.verify_ok();
    let owner = st.owner();
    let _ = s;
    let mut c = col().w_full().gap(10.0).child(
        row()
            .w_full()
            .gap(8.0)
            .center_y()
            .child(theme::section_title("Activity", k))
            .child(spacer())
            .child(icon(&icons::SHIELD, 12.0).color(if ok { k.working } else { k.error }))
            .child(text(format!("{} · {} signed", if ok { "Verified" } else { "Doesn't check" }, p.verify_entries())).font(font(12.0, 500.0).lh(15.0)).color(if ok {
                k.working
            } else {
                k.error
            })),
    );
    for e in &p.entries {
        let when = model::ago_long(Some(&e.at()));
        let entry = match shape(e, d, &owner) {
            Shape::Skip => continue,
            Shape::Sys(a, bits, tone) => row()
                .key(format!("entry-{}", e.seq()))
                .w_full()
                .gap(8.0)
                .child(el().pt(1.0).child(avatar(&a, k)))
                .child(line(&bits, &when, tone_color(tone, k), k).grow(1.0).min_w(0.0))
                .child(mark(&e.mark(), k)),
            Shape::File(a, writer, name) => row()
                .key(format!("entry-{}", e.seq()))
                .w_full()
                .gap(8.0)
                .child(el().pt(1.0).child(avatar(&a, k)))
                .child(line(&[Bit::B(writer), Bit::T(" kept ".into()), Bit::B(name)], &when, None, k).grow(1.0).min_w(0.0))
                .child(mark(&e.mark(), k)),
            Shape::Comment(a, writer, verb, body, style) => {
                let (fill, edge) = match style {
                    Some("you") => (k.accent_soft, k.accent.alpha(0.25)),
                    Some("ask") => (k.warning_tint, k.blocked.alpha(0.25)),
                    _ => (k.elev, k.line2),
                };
                col()
                    .key(format!("entry-{}", e.seq()))
                    .w_full()
                    .padding(12.0)
                    .gap(8.0)
                    .radius(10.0)
                    .continuous_corners()
                    .fill(fill)
                    .border(1.0, edge)
                    .child(
                        row()
                            .w_full()
                            .gap(7.0)
                            .center_y()
                            .child(avatar(&a, k))
                            .child(text(writer).font(font(13.0, 600.0).lh(16.0)).color(k.text))
                            .child(text(verb).font(font(13.0, 400.0).lh(16.0)).color(k.muted).truncate().shrink())
                            .child(text(format!("· {when}")).font(font(12.0, 400.0).lh(16.0)).color(k.faint))
                            .child(spacer())
                            .child(mark(&e.mark(), k)),
                    )
                    .child(markdown(&body, k, kit))
            }
        };
        c = c.child(entry);
    }
    c
}
