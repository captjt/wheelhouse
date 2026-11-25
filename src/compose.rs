//! The composer on a docket's Activity. What you type there is a note,
//! unless it starts with a command:
//!
//! - `@builder fix the flaky test` instructs that agent on the docket (by
//!   its pane's name, its role or its app), as a prompt; `@all` instructs
//!   every agent on it. It's queued while an agent works.
//! - `/ask Which store? | Postgres | SQLite` asks the docket's agents, with
//!   the options as buttons. Their answer stands under Decisions.
//! - `/decide Retries: 5xx and timeouts only` records a decision of yours
//!   (the newest for a subject stands) and tells the agents on the docket.
//!   `/decide #12 Postgres` (or a question's own words before the colon)
//!   answers a question that waits on you.
//! - `/task write the migration's tests` starts another agent on the
//!   docket with that prompt: the app opens Add an agent, filled in.
//! - `/note` keeps text that starts with `@` or `/` as a note.
//!
//! Parsing is here, and pure; `server::compose` carries it out through the
//! paths the apps already use (a prompt, a message, a reply, an entry).

use anyhow::{Result, bail};

/// A subject (or a question's subject line) is at most this long.
pub const MAX_SUBJECT: usize = 200;
/// Anything typed is at most this long: what a pane takes in one paste.
pub const MAX_TEXT: usize = 16_000;

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Note(String),
    /// `to` is the name after `@`, lowercased.
    Instruct {
        to: String,
        text: String,
    },
    Ask {
        question: String,
        options: Vec<String>,
    },
    /// `key`: the first word, when it could name a question (`#12`, `D2`,
    /// `#ab12cd34`); it's only a key if one has it.
    Decide {
        key: Option<String>,
        subject: Option<String>,
        choice: String,
        text: String,
    },
    Task {
        prompt: String,
    },
}

const COMMANDS: &[&str] = &["ask", "decide", "task", "note"];

pub fn parse(text: &str) -> Result<Command> {
    let text = text.trim();
    if text.is_empty() {
        bail!("nothing to send");
    }
    if text.len() > MAX_TEXT {
        bail!("that's more than {} KB: keep the rest in a file and attach it", MAX_TEXT / 1000);
    }
    if let Some(rest) = text.strip_prefix('/') {
        let (word, rest) = split_word(rest);
        let rest = rest.trim();
        // `/tmp/x is full` is a note; `/decied …` is a slip, said as one.
        if !word.chars().all(|c| c.is_ascii_alphabetic()) || word.is_empty() {
            return Ok(Command::Note(text.to_string()));
        }
        let need = |what: &str| -> Result<String> {
            if rest.is_empty() {
                bail!("/{word} needs {what}");
            }
            Ok(rest.to_string())
        };
        return match word.to_ascii_lowercase().as_str() {
            "note" => Ok(Command::Note(need("the note")?)),
            "task" => Ok(Command::Task { prompt: need("what the agent should do")? }),
            "ask" => ask(&need("a question")?),
            "decide" => decide(&need("what you decided")?),
            _ => bail!(
                "there's no /{word}: try {}. To leave a note that starts with /, begin it with /note",
                COMMANDS.iter().map(|c| format!("/{c}")).collect::<Vec<_>>().join(", ")
            ),
        };
    }
    if let Some(rest) = text.strip_prefix('@') {
        let (name, rest) = split_word(rest);
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')) {
            return Ok(Command::Note(text.to_string()));
        }
        let rest = rest.trim();
        if rest.is_empty() {
            bail!("what should @{name} do? Write it after the name");
        }
        return Ok(Command::Instruct { to: name.to_ascii_lowercase(), text: rest.to_string() });
    }
    Ok(Command::Note(text.to_string()))
}

/// The first word, and what follows it.
fn split_word(s: &str) -> (&str, &str) {
    match s.find(char::is_whitespace) {
        Some(i) => (&s[..i], &s[i..]),
        None => (s, ""),
    }
}

fn ask(rest: &str) -> Result<Command> {
    let mut parts = rest.split('|').map(str::trim);
    let question = parts.next().unwrap_or("").to_string();
    if question.is_empty() {
        bail!("/ask needs a question before its options");
    }
    let options: Vec<String> = parts.filter(|o| !o.is_empty()).map(str::to_string).collect();
    if options.len() == 1 {
        bail!("one option isn't a choice: give two or more, or none");
    }
    Ok(Command::Ask { question, options })
}

fn decide(rest: &str) -> Result<Command> {
    let (first, after) = split_word(rest);
    let key = is_key(first).then(|| first.to_string()).filter(|_| !after.trim().is_empty());
    // `Subject: what stands`, when the subject is one short line.
    let (subject, choice) = match rest.split_once(": ") {
        Some((s, c)) if !s.contains('\n') && !s.trim().is_empty() && s.len() <= MAX_SUBJECT && !c.trim().is_empty() => (Some(s.trim().to_string()), c.trim().to_string()),
        _ => (None, rest.trim().to_string()),
    };
    Ok(Command::Decide { key, subject, choice, text: rest.trim().to_string() })
}

/// What a question is known by: `#12`, `#ab12cd34` (a shared docket's),
/// or a Build decision's `D2`.
fn is_key(w: &str) -> bool {
    let digits = |s: &str| !s.is_empty() && s.len() <= 12 && s.chars().all(|c| c.is_ascii_digit());
    match (w.strip_prefix('#'), w.strip_prefix('D')) {
        (Some(n), _) => digits(n) || (n.len() == 8 && n.chars().all(|c| c.is_ascii_hexdigit())),
        (_, Some(n)) => digits(n),
        _ => false,
    }
}

/// A question's words, for matching: case, spacing and the ending
/// punctuation don't count.
pub fn same_words(a: &str, b: &str) -> bool {
    let norm = |s: &str| s.trim().trim_end_matches(['?', '.', ':', '!']).split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
    let (a, b) = (norm(a), norm(b));
    !a.is_empty() && a == b
}

/// The question a `/decide` answers, if it answers one: by its key, or by
/// its words before the colon. The newest asking wins.
pub fn question_for<'a>(ds: &'a [crate::decisions::Decision], key: Option<&str>, subject: Option<&str>) -> Option<&'a crate::decisions::Decision> {
    if let Some(k) = key
        && let Some(d) = ds.iter().rev().find(|d| d.key == k && d.message.is_some())
    {
        return Some(d);
    }
    let s = subject?;
    ds.iter().rev().find(|d| d.message.is_some() && same_words(&d.question, s))
}

/// The names an agent on a docket answers to after `@`: its pane's name,
/// its role and each word of it, and its app's short names.
pub fn names(pane: &str, role: Option<&str>, app: &str) -> Vec<String> {
    let mut out = vec![pane.to_lowercase()];
    if let Some(r) = role.map(str::trim).filter(|r| !r.is_empty()) {
        out.push(r.to_lowercase().replace(' ', "-"));
        out.extend(r.split_whitespace().map(str::to_lowercase));
    }
    out.extend(
        match app {
            "claude-code" | "claude" => &["claude", "claude-code"][..],
            "codex" => &["codex"],
            "agy" | "antigravity" => &["agy", "antigravity", "gemini"],
            "pi" => &["pi"],
            _ => &[],
        }
        .iter()
        .map(|s| s.to_string()),
    );
    out.retain(|n| !n.is_empty());
    out.dedup();
    out
}

/// Who `@to` means among the agents on a docket, each `(block, names, shown)`:
/// every one for `@all`, else each one that answers to the name. None is
/// an error that lists who's there; two by an app's name is fine
/// (`@claude` instructs both).
pub fn targets<'a>(to: &str, agents: &'a [(String, Vec<String>, String)]) -> Result<Vec<&'a (String, Vec<String>, String)>> {
    if agents.is_empty() {
        bail!("no agent is on this docket now: start one with /task, or Add an agent");
    }
    if matches!(to, "all" | "agents" | "everyone") {
        return Ok(agents.iter().collect());
    }
    let found: Vec<_> = agents.iter().filter(|a| a.1.iter().any(|n| n == to)).collect();
    if found.is_empty() {
        let mut here: Vec<&str> = agents.iter().map(|a| a.2.as_str()).collect();
        here.dedup();
        bail!("no agent here is @{to}. On this docket: {} (or @all)", here.join(", "));
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_message_is_a_note_and_so_is_a_path_or_a_lone_at() {
        assert_eq!(parse("  Looks good.  ").unwrap(), Command::Note("Looks good.".into()));
        assert_eq!(parse("/tmp/wh is full").unwrap(), Command::Note("/tmp/wh is full".into()));
        assert_eq!(parse("@ noon, then").unwrap(), Command::Note("@ noon, then".into()));
        assert_eq!(parse("@sam's idea: retry").unwrap(), Command::Note("@sam's idea: retry".into()));
        assert_eq!(parse("/note @all is not a command here").unwrap(), Command::Note("@all is not a command here".into()));
        assert!(parse("   ").is_err());
        assert!(parse(&"x".repeat(MAX_TEXT + 1)).is_err());
    }

    #[test]
    fn a_slip_or_a_command_without_its_words_says_so() {
        let e = parse("/decied 5xx only").unwrap_err().to_string();
        assert!(e.contains("no /decied") && e.contains("/decide") && e.contains("/note"), "{e}");
        assert!(parse("/ask").unwrap_err().to_string().contains("needs a question"));
        assert!(parse("/task   ").unwrap_err().to_string().contains("needs what the agent should do"));
        assert!(parse("@builder").unwrap_err().to_string().contains("what should @builder do"));
        assert!(parse("/ask | a | b").unwrap_err().to_string().contains("question before"));
        assert!(parse("/ask Which? | only one").unwrap_err().to_string().contains("one option"));
    }

    #[test]
    fn instruct_ask_and_task() {
        assert_eq!(parse("@Builder fix the test\nthen run it").unwrap(), Command::Instruct { to: "builder".into(), text: "fix the test\nthen run it".into() });
        assert_eq!(
            parse("/ask Which store? | Postgres |  SQLite | ").unwrap(),
            Command::Ask { question: "Which store?".into(), options: vec!["Postgres".into(), "SQLite".into()] }
        );
        assert_eq!(parse("/ASK Does /v2/items page?").unwrap(), Command::Ask { question: "Does /v2/items page?".into(), options: vec![] });
        assert_eq!(parse("/task write the tests").unwrap(), Command::Task { prompt: "write the tests".into() });
    }

    #[test]
    fn decide_reads_a_key_and_a_subject() {
        let d = |s: &str| match parse(s).unwrap() {
            Command::Decide { key, subject, choice, .. } => (key, subject, choice),
            c => panic!("{c:?}"),
        };
        assert_eq!(d("/decide Retries: 5xx and timeouts only"), (None, Some("Retries".into()), "5xx and timeouts only".into()));
        assert_eq!(d("/decide #12 Postgres"), (Some("#12".into()), None, "#12 Postgres".into()));
        assert_eq!(d("/decide D2 cap at 3"), (Some("D2".into()), None, "D2 cap at 3".into()));
        assert_eq!(d("/decide #ab12cd34 yes"), (Some("#ab12cd34".into()), None, "#ab12cd34 yes".into()));
        assert_eq!(d("/decide Use Postgres, not SQLite"), (None, None, "Use Postgres, not SQLite".into()));
        // A time isn't a subject, and a key alone isn't an answer.
        assert_eq!(d("/decide ship at 10:30"), (None, None, "ship at 10:30".into()));
        assert_eq!(d("/decide #12"), (None, None, "#12".into()));
        assert_eq!(d("/decide Dashboards"), (None, None, "Dashboards".into()));
    }

    fn q(key: &str, question: &str, message: Option<i64>) -> crate::decisions::Decision {
        crate::decisions::Decision {
            key: key.into(),
            question: question.into(),
            options: vec![],
            asked_by: "Claude Code".into(),
            asked_at: "t".into(),
            message,
            answer: None,
            note: None,
            by: None,
            at: None,
            standing: None,
        }
    }

    #[test]
    fn a_decision_finds_the_question_it_answers() {
        let ds = vec![q("#3", "Retries for 4xx?", Some(3)), q("D2", "Which store?", Some(9)), q("=x", "Mine", None)];
        assert_eq!(question_for(&ds, Some("#3"), None).unwrap().key, "#3");
        assert_eq!(question_for(&ds, None, Some("which  STORE")).unwrap().key, "D2");
        assert_eq!(question_for(&ds, Some("#99"), Some("retries for 4xx")).unwrap().key, "#3", "a key no question has falls back to the words");
        assert!(question_for(&ds, None, Some("Mine")).is_none(), "your own decisions aren't questions");
        assert!(question_for(&ds, None, None).is_none());
        assert!(!same_words("", "?"));
    }

    #[test]
    fn at_names_an_agent_by_pane_role_or_app() {
        let a = vec![
            ("b1".to_string(), names("D-12", Some("Build builder"), "claude-code"), "D-12 (Claude Code)".to_string()),
            ("b2".to_string(), names("D-12-2", Some("tests"), "codex"), "D-12-2 (Codex)".to_string()),
            ("b3".to_string(), names("planner", None, "claude-code"), "planner (Claude Code)".to_string()),
        ];
        let blocks = |to: &str| targets(to, &a).unwrap().into_iter().map(|t| t.0.as_str()).collect::<Vec<_>>();
        assert_eq!(blocks("builder"), ["b1"]);
        assert_eq!(blocks("build-builder"), ["b1"]);
        assert_eq!(blocks("tests"), ["b2"]);
        assert_eq!(blocks("codex"), ["b2"]);
        assert_eq!(blocks("d-12-2"), ["b2"]);
        assert_eq!(blocks("planner"), ["b3"]);
        assert_eq!(blocks("claude"), ["b1", "b3"]);
        assert_eq!(blocks("all"), ["b1", "b2", "b3"]);
        let e = targets("reviewer", &a).unwrap_err().to_string();
        assert!(e.contains("no agent here is @reviewer") && e.contains("D-12-2 (Codex)"), "{e}");
        assert!(targets("all", &[]).unwrap_err().to_string().contains("no agent is on this docket"));
    }
}
