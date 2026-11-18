//! What the panel lists: the hub's state read the way the Mac app's Store
//! reads it (mac/Sources/Wheelhouse/Store.swift, `inbox`), turned into
//! commands the way its palette does (Palette.swift, floating), with the
//! same scoring.

use serde_json::Value;

/// A docket's status: the model's, shared with the main window.
pub use crate::model::Status;

#[derive(Clone, Debug, PartialEq)]
pub struct Docket {
    pub id: String,
    pub title: String,
    pub status: Status,
    pub keywords: String,
}

/// Something that needs you and can be answered from the panel.
#[derive(Clone, Debug, PartialEq)]
pub enum Need {
    /// An agent's permission prompt, answerable in its pane.
    Prompt { docket: String, title: String, msg: Option<String>, record: String, since: Option<String> },
    /// A review drafted and ready to post.
    Review { key: String, full_key: String, title: String, digest: String },
    /// A message with options: an agent's ask.
    Ask { id: i64, docket: String, title: String, subject: String, who: String, options: Vec<String> },
}

/// What `/api/state` says, as far as the panel needs it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct World {
    pub dockets: Vec<Docket>,
    pub needs: Vec<Need>,
    /// Folders agents work in, for Start an agent.
    pub folders: Vec<String>,
    /// A second hub's flavor (Wheelhouse Dev's `dev`), which the panel wears.
    pub flavor: Option<String>,
}

fn s(v: &Value) -> Option<String> {
    v.as_str().map(str::trim).filter(|t| !t.is_empty()).map(str::to_string)
}

fn app_name(app: Option<&str>) -> String {
    match app {
        Some("claude-code" | "claude") => "Claude Code".into(),
        Some("codex") => "Codex".into(),
        Some(a) if !a.is_empty() => a.into(),
        _ => "Agent".into(),
    }
}

impl World {
    pub fn parse(j: &Value) -> World {
        let empty = Vec::new();
        let arr = |v: &Value| -> Vec<Value> { v.as_array().unwrap_or(&empty).clone() };
        let dockets_j = arr(&j["dockets"]);
        let title_of = |id: &str| -> Option<String> { dockets_j.iter().find(|d| d["id"].as_str() == Some(id)).and_then(|d| s(&d["title"])) };

        let dockets = dockets_j
            .iter()
            .map(|d| {
                let id = s(&d["id"]).unwrap_or_default();
                let agent = &d["agent"];
                Docket {
                    title: s(&d["title"]).unwrap_or_else(|| id.clone()),
                    status: Status::parse(d["status"].as_str().unwrap_or("")),
                    keywords: format!("{id} {} {}", s(&agent["cwd"]).unwrap_or_default(), s(&agent["rex_session"]).unwrap_or_default()),
                    id,
                }
            })
            .collect();

        // The Inbox's order: prompts, then reviews, then messages; newest first in each.
        let mut needs: Vec<(u8, String, Need)> = Vec::new();
        for d in &dockets_j {
            let live = &d["live"];
            if live["state"].as_str() == Some("blocked") && live["answerable"].as_bool() == Some(true) {
                let id = s(&d["id"]).unwrap_or_default();
                needs.push((
                    0,
                    s(&live["since"]).unwrap_or_default(),
                    Need::Prompt {
                        title: s(&d["title"]).unwrap_or_else(|| id.clone()),
                        docket: id,
                        msg: s(&live["msg"]),
                        record: live["record"].as_str().unwrap_or("").to_string(),
                        since: s(&live["since"]),
                    },
                ));
            }
        }
        for p in arr(&j["review"]["prs"]) {
            let ready = p["state"].as_str() == Some("reviewing") && p["phase"].as_str() == Some("awaiting");
            if p["attention"].is_object()
                && ready
                && let Some(digest) = s(&p["draft"]["digest"])
            {
                let key = s(&p["key"]).unwrap_or_default();
                needs.push((
                    1,
                    s(&p["updated_at"]).unwrap_or_default(),
                    Need::Review { full_key: s(&p["full_key"]).unwrap_or_else(|| key.clone()), title: s(&p["title"]).unwrap_or_default(), key, digest },
                ));
            }
        }
        for m in arr(&j["messages"]) {
            let options: Vec<String> = arr(&m["options"]).iter().filter_map(s).collect();
            if options.is_empty() {
                continue;
            }
            let id = m["id"].as_i64().unwrap_or(0);
            let subject = s(&m["subject"])
                .or_else(|| m["body"].as_str().unwrap_or("").lines().map(str::trim).find(|l| !l.is_empty()).map(str::to_string))
                .unwrap_or_else(|| format!("Message #{id}"));
            let from = m["from"].as_str().unwrap_or("");
            let who = if from == "wheelhouse" { "Wheelhouse".into() } else { s(&m["from_app"]).unwrap_or_else(|| app_name(Some(from))) };
            let docket = s(&m["docket"]);
            needs.push((
                2,
                s(&m["created_at"]).unwrap_or_default(),
                Need::Ask {
                    id,
                    title: docket.as_deref().and_then(title_of).unwrap_or_else(|| subject.clone()),
                    docket: docket.unwrap_or_else(|| format!("#{id}")),
                    subject,
                    who,
                    options,
                },
            ));
        }
        needs.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));

        let mut folders: Vec<String> = dockets_j.iter().filter_map(|d| s(&d["agent"]["cwd"])).filter(|c| !c.contains("/.local/share/")).collect();
        folders.sort();
        folders.dedup();

        World { dockets, needs: needs.into_iter().map(|n| n.2).collect(), folders, flavor: s(&j["flavor"]["name"]) }
    }
}

/// A Linear issue, as `/api/linear/search` lists it.
#[derive(Clone, Debug, PartialEq)]
pub struct Issue {
    pub key: String,
    pub title: String,
    pub state: Option<String>,
    pub status: Status,
}

impl Issue {
    pub fn parse(j: &Value) -> Issue {
        Issue {
            key: s(&j["key"]).or_else(|| s(&j["identifier"])).unwrap_or_default(),
            title: s(&j["title"]).unwrap_or_default(),
            state: s(&j["state"]),
            status: match j["state_type"].as_str() {
                Some("started") => Status::InProgress,
                Some("completed") => Status::Done,
                Some("canceled") => Status::Dropped,
                _ => Status::Todo,
            },
        }
    }
}

/// What a command does.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Answer { docket: String, approve: bool, record: String, since: Option<String>, msg: Option<String> },
    Choose { id: i64, choice: String },
    Post { full_key: String, key: String, digest: String },
    NewDocket,
    StartAgent,
    FromLinear,
    Open(String),
    Theme(Option<bool>),
    /// A command only the main window's palette has: `go:inbox`, `rex:D-6`…
    Main(String),
}

/// The mark at a row's start.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mark {
    Approve,
    Deny,
    Reply,
    Post,
    New,
    Agent,
    Linear,
    Status(Status),
    System,
    Light,
    Dark,
    /// A mark by name, for the main window's own commands.
    Glyph(&'static str),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Command {
    pub id: String,
    pub group: &'static str,
    pub label: String,
    pub mark: Mark,
    pub hint: Option<String>,
    pub keywords: String,
    pub action: Action,
}

pub const NEEDS_YOU: &str = "Needs you";
pub const CREATE: &str = "Create";
pub const DOCKETS: &str = "Dockets";
pub const THEME: &str = "Theme";

fn cmd(id: impl Into<String>, group: &'static str, label: impl Into<String>, mark: Mark, hint: Option<&str>, keywords: impl Into<String>, action: Action) -> Command {
    Command { id: id.into(), group, label: label.into(), mark, hint: hint.map(str::to_string), keywords: keywords.into(), action }
}

/// Every command, in the palette's order.
pub fn commands(w: &World) -> Vec<Command> {
    let mut out = Vec::new();
    for n in &w.needs {
        match n {
            Need::Prompt { docket, title, msg, record, since } => {
                let what = msg.clone().unwrap_or_else(|| "the prompt".into());
                for approve in [true, false] {
                    let (verb, mark, words) = if approve { ("Approve", Mark::Approve, "approve permission yes") } else { ("Deny", Mark::Deny, "deny permission no") };
                    out.push(cmd(
                        format!("{}-{docket}", if approve { "ap" } else { "dn" }),
                        NEEDS_YOU,
                        format!("{verb} {docket}: {what}"),
                        mark,
                        None,
                        format!("{words} {title}"),
                        Action::Answer { docket: docket.clone(), approve, record: record.clone(), since: since.clone(), msg: msg.clone() },
                    ));
                }
            }
            Need::Review { key, full_key, title, digest } => out.push(cmd(
                format!("post-{full_key}"),
                NEEDS_YOU,
                format!("Post the review of {key}"),
                Mark::Post,
                None,
                format!("review post github {title}"),
                Action::Post { full_key: full_key.clone(), key: key.clone(), digest: digest.clone() },
            )),
            Need::Ask { id, docket, title, subject, who, options } => {
                for o in options {
                    out.push(cmd(
                        format!("op-{id}-{o}"),
                        NEEDS_YOU,
                        format!("{docket} · {subject}: {o}"),
                        Mark::Reply,
                        None,
                        format!("answer reply {who} {title}"),
                        Action::Choose { id: *id, choice: o.clone() },
                    ));
                }
            }
        }
    }
    out.push(cmd("new", CREATE, "New docket", Mark::New, Some("⌘N"), "", Action::NewDocket));
    out.push(cmd("new-agent", CREATE, "Start an agent on a new docket", Mark::Agent, Some("⇧⌘N"), "claude codex start run", Action::StartAgent));
    out.push(cmd("new-linear", CREATE, "New docket from a Linear issue", Mark::Linear, None, "linear issue import", Action::FromLinear));
    out.push(cmd("th-system", THEME, "Theme: System", Mark::System, None, "appearance mode auto", Action::Theme(None)));
    out.push(cmd("th-light", THEME, "Theme: Light", Mark::Light, None, "appearance mode", Action::Theme(Some(false))));
    out.push(cmd("th-dark", THEME, "Theme: Dark", Mark::Dark, None, "appearance mode", Action::Theme(Some(true))));
    for d in &w.dockets {
        out.push(cmd(format!("d-{}", d.id), DOCKETS, d.title.clone(), Mark::Status(d.status), Some(&d.id), d.keywords.clone(), Action::Open(d.id.clone())));
    }
    out
}

/// How well `q` matches `text`: higher is better, None is no match. As
/// Palette.swift: a substring scores by how early it starts, else the
/// letters in order score their count.
fn score(q: &str, text: &str) -> Option<i64> {
    if q.is_empty() {
        return Some(0);
    }
    let t = text.to_lowercase();
    if let Some(at) = t.find(q) {
        return Some(100 - t[..at].chars().count() as i64);
    }
    let want: Vec<char> = q.chars().collect();
    let mut j = 0;
    for c in t.chars() {
        if c == want[j] {
            j += 1;
            if j == want.len() {
                return Some(j as i64);
            }
        }
    }
    None
}

/// What the list shows for `query`. Empty, floating: what needs you,
/// Create, and the first eight dockets.
pub fn shown(all: Vec<Command>, query: &str) -> Vec<Command> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        let (dockets, rest): (Vec<_>, Vec<_>) = all.into_iter().partition(|c| c.group == DOCKETS);
        let mut keep: Vec<Command> = rest.into_iter().filter(|c| c.group == NEEDS_YOU || c.group == CREATE).collect();
        keep.extend(dockets.into_iter().take(8));
        return keep;
    }
    let mut scored: Vec<(Command, i64)> = all
        .into_iter()
        .filter_map(|c| {
            let s = score(&q, &c.label).unwrap_or(-1).max(score(&q, &c.keywords).unwrap_or(-100) - 20);
            (s >= 0).then_some((c, s))
        })
        .collect();
    // Stable, as the best first: ties keep the palette's order.
    scored.sort_by_key(|c| std::cmp::Reverse(c.1));
    scored.into_iter().take(30).map(|(c, _)| c).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn world() -> World {
        World::parse(&json!({
            "dockets": [
                {"id": "D-6", "title": "Retry failed webhook deliveries", "status": "in_progress",
                 "agent": {"cwd": "/code/acme-api", "rex_session": "session:acme-api"},
                 "live": {"state": "working", "answerable": false}},
                {"id": "D-1", "title": "Fix the checkout tests", "status": "in_progress",
                 "agent": {"cwd": "/code/acme-api"},
                 "live": {"state": "blocked", "answerable": true, "msg": "Bash: pnpm test", "record": "r1", "since": "2026-11-18T14:22:49Z"}}
            ],
            "messages": [
                {"id": 3, "docket": "D-6", "subject": "Reindex now or tonight?", "options": ["Tonight at 2am", "Now"], "from": "acme-api/search"},
                {"id": 4, "docket": "D-6", "subject": "FYI", "options": []}
            ]
        }))
    }

    #[test]
    fn needs_you_comes_first_in_the_inboxs_order() {
        let shown = shown(commands(&world()), "");
        let labels: Vec<&str> = shown.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(
            &labels[..4],
            ["Approve D-1: Bash: pnpm test", "Deny D-1: Bash: pnpm test", "D-6 · Reindex now or tonight?: Tonight at 2am", "D-6 · Reindex now or tonight?: Now",]
        );
        assert_eq!(labels[4], "New docket");
        assert!(!labels.iter().any(|l| l.starts_with("Theme")), "themes only when searched");
        assert_eq!(shown.last().map(|c| c.group), Some(DOCKETS));
    }

    #[test]
    fn search_scores_as_the_swift_palette_does() {
        assert_eq!(score("web", "retry failed webhook deliveries"), Some(100 - 13));
        assert_eq!(score("rfw", "retry failed webhook"), Some(3));
        assert_eq!(score("zz", "retry"), None);
        let found = shown(commands(&world()), "webhook");
        assert_eq!(found[0].label, "Retry failed webhook deliveries");
        // By id, through the keywords: below a label that has it, as in Swift.
        let found = shown(commands(&world()), "d-6");
        assert_eq!(found[0].id, "op-3-Tonight at 2am");
        assert!(found.iter().any(|c| c.id == "d-D-6"));
        // By the session an agent runs in.
        assert!(shown(commands(&world()), "session:acme").iter().any(|c| c.id == "d-D-6"));
    }

    #[test]
    fn folders_are_where_agents_work() {
        assert_eq!(world().folders, vec!["/code/acme-api".to_string()]);
    }
}
