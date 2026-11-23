//! Tab completion in zsh and bash. `wh completions zsh` prints a hook for
//! the shell's startup file. On each Tab the hook runs `wh` again with the
//! line so far: clap's engine completes the commands and flags, and the hub
//! fills in what's live (dockets, messages, agents, PRs). A Tab only reads,
//! gives up on a slow hub quickly, and never writes to the terminal.

use clap_complete::engine::{ArgValueCandidates, ArgValueCompleter, CompletionCandidate};
use clap_complete::env::{Bash, EnvCompleter, Zsh};
use serde_json::Value;
use std::ffi::OsStr;
use std::time::Duration;

/// What the hook sets to ask `wh` for completions. Not clap's usual
/// `COMPLETE`: one left in someone's environment would turn every `wh`,
/// the agents' hooks included, into a completion.
const VAR: &str = "WH_COMPLETE";

/// What the hook exports, for `wh doctor`.
pub const MARK: &str = "WH_TAB";

/// How long a Tab waits for the hub. It answers in milliseconds; one that
/// doesn't mustn't hang the prompt.
const PATIENCE: Duration = Duration::from_millis(800);

#[derive(Clone, Copy, clap::ValueEnum)]
pub enum Shell {
    Zsh,
    Bash,
}

/// If the shell's hook ran `wh` for a Tab, answer it and exit. The first
/// thing `main` does.
pub fn answer(cli: fn() -> clap::Command) {
    let args: Vec<std::ffi::OsString> = std::env::args_os().collect();
    let tab = args.iter().any(|a| a == "--");
    let cwd = std::env::current_dir().ok();
    match clap_complete::CompleteEnv::with_factory(cli).var(VAR).try_complete(args, cwd.as_deref()) {
        Ok(false) => {}
        Ok(true) => std::process::exit(0),
        // Nothing to offer. Bash shows a completer's errors in the prompt.
        Err(_) if tab => std::process::exit(1),
        Err(e) => e.exit(),
    }
}

/// `wh completions zsh|bash`: the hook. Without a shell, how to add it.
pub fn cli(shell: Option<Shell>) -> anyhow::Result<()> {
    let Some(shell) = shell else {
        let login = std::env::var("SHELL").unwrap_or_default();
        let rows = [("zsh", "~/.zshrc"), ("bash", "~/.bashrc")];
        let yours: Vec<_> = rows.iter().filter(|(name, _)| login.rsplit('/').next() == Some(name)).collect();
        println!("Tab completion for wh: add this line to your shell's startup file, then open a new terminal.\n");
        for (name, rc) in if yours.is_empty() { rows.iter().collect() } else { yours } {
            println!("  {rc:<10} eval \"$(wh completions {name})\"");
        }
        println!("\nwh doctor says whether the shell you run it in has it.");
        return Ok(());
    };
    std::io::Write::write_all(&mut std::io::stdout(), &script(shell)?)?;
    Ok(())
}

/// zsh: clap's hook registers itself where it's sourced, which a `compinit`
/// later in ~/.zshrc undoes (oh-my-zsh runs one). This one registers at the
/// first prompt, once ~/.zshrc is done, and starts zsh's completion system
/// if nothing has.
const ZSH_REGISTER: &str = "compdef _clap_dynamic_completer_wh wh";
const ZSH_DEFERRED: &str = r#"_wh_compdef() {
    add-zsh-hook -d precmd _wh_compdef
    (( $+functions[compdef] )) || { autoload -Uz compinit && compinit -i; }
    compdef _clap_dynamic_completer_wh wh
}
autoload -Uz add-zsh-hook && add-zsh-hook precmd _wh_compdef"#;

/// bash 3.2 (macOS's) can't turn `nospace` off for one word, so clap's hook
/// leaves you at `wh review` with no space after it. Put the space in.
const BASH_3: &str = r#"if [[ ${BASH_VERSINFO[0]} -lt 4 ]]; then
    _wh_complete() {
        _clap_complete_wh "$@"
        if [[ ${#COMPREPLY[@]} -eq 1 && ! ${COMPREPLY[0]} =~ [=/:]$ ]]; then COMPREPLY[0]+=" "; fi
    }
    complete -o nospace -o bashdefault -F _wh_complete wh
fi
"#;

fn script(shell: Shell) -> std::io::Result<Vec<u8>> {
    let me = completer();
    let mut clap = Vec::new();
    let (name, rc) = match shell {
        Shell::Zsh => {
            Zsh.write_registration(VAR, "wh", "wh", &me, &mut clap)?;
            ("zsh", "~/.zshrc")
        }
        Shell::Bash => {
            Bash.write_registration(VAR, "wh", "wh", &me, &mut clap)?;
            ("bash", "~/.bashrc")
        }
    };
    let clap = String::from_utf8_lossy(&clap);
    let body = match shell {
        Shell::Zsh => clap.replacen(ZSH_REGISTER, ZSH_DEFERRED, 1),
        Shell::Bash => format!("{clap}{BASH_3}"),
    };
    // So `wh doctor` can tell this shell has it, wherever the line lives.
    Ok(format!("# Tab completion for wh. In {rc}: eval \"$(wh completions {name})\"\n{body}\nexport {MARK}={name}\n").into_bytes())
}

/// The `wh` the hook calls back: by name when this one was found on PATH,
/// so updating it in place keeps working; by its full path when it was run
/// by one (`./target/release/wh completions zsh`).
fn completer() -> String {
    let arg0 = std::path::PathBuf::from(std::env::args_os().next().unwrap_or_else(|| "wh".into()));
    let path = if arg0.components().count() > 1 { std::env::current_dir().map(|d| d.join(&arg0)).unwrap_or(arg0) } else { arg0 };
    path.to_string_lossy().into_owned()
}

// ---- The words each command takes ----

type Words = &'static [(&'static str, &'static str)];

pub const REVIEW: Words = &[
    ("status", "every PR it follows, and what each needs"),
    ("on", "turn Review on (paused, the first time)"),
    ("off", "turn Review off"),
    ("pause", "stop new rounds"),
    ("resume", "restart new rounds"),
    ("check", "check GitHub now"),
    ("post-mode", "who posts a checked review: ask, auto, or config (your rules)"),
    ("pickup", "review a PR that didn't request you"),
    ("preview", "see a round's checked review"),
    ("post", "post a round's checked review"),
    ("dismiss", "drop a round's checked review"),
];
const POST_MODE: Words = &[("ask", "you post it"), ("auto", "the checker posts it"), ("config", "your rules, from Settings and review.lua")];
pub const BUILD: Words = &[
    ("status", "every Build, and what each needs"),
    ("start", "Build a docket, a PR (owner/name#45) or a Linear issue"),
    ("discard", "drop a Build"),
    ("pause", "stop new Builds"),
    ("resume", "restart new Builds"),
];
pub const SKILL: Words = &[("show", "print the agents' skill"), ("install", "install it for Claude Code, Codex and Antigravity"), ("remove", "remove it")];
pub const HOOKS: Words = &[("status", "whether the agents' hooks are in"), ("install", "install Claude Code's and Antigravity's hooks"), ("remove", "remove them")];
pub const RECAP: Words = &[("today", ""), ("week", "the default"), ("30d", "the last 30 days")];
pub const STATUS: Words = &[("todo", ""), ("in_progress", ""), ("done", "on evidence, or --anyway --reason"), ("dropped", "")];
pub const IMPORT: Words = &[("rex-reviewer", "its config, guides and ledger, once")];

pub fn words(list: Words) -> ArgValueCandidates {
    ArgValueCandidates::new(move || list.iter().map(|(w, h)| cand(*w, *h)).collect())
}

// ---- What the hub knows ----

/// A docket: `wh show D-<Tab>`.
pub fn dockets() -> ArgValueCompleter {
    ArgValueCompleter::new(|typed: &OsStr| typed.to_str().zip(get("/api/state")).map(|(t, state)| docket_candidates(&state, t)).unwrap_or_default())
}

/// A message to you, or to this pane: `wh reply <Tab>`.
pub fn messages() -> ArgValueCandidates {
    ArgValueCandidates::new(|| inbox().map(|v| message_candidates(&v)).unwrap_or_default())
}

/// `wh reply 12 --choice <Tab>`: #12's options.
pub fn choices() -> ArgValueCompleter {
    ArgValueCompleter::new(|typed: &OsStr| {
        let typed = typed.to_str().unwrap_or_default();
        let n = after("reply", &before()).into_iter().next();
        n.zip(inbox()).map(|(n, list)| choice_candidates(&list, &n, typed, bash())).unwrap_or_default()
    })
}

/// Who a message can go to: `wh send --to <Tab>`.
pub fn addresses() -> ArgValueCandidates {
    ArgValueCandidates::new(|| {
        let mut out = vec![cand("me", "you, in the Inbox")];
        if let Some(state) = get("/api/state") {
            out.extend(address_candidates(&state, std::env::var("REX_BLOCK").ok().as_deref()));
        }
        out
    })
}

/// What follows `wh review ACTION`: a mode, or a PR.
pub fn review_args() -> ArgValueCompleter {
    ArgValueCompleter::new(|typed: &OsStr| review_candidates(&after("review", &before()), typed.to_str().unwrap_or_default(), || get("/api/review")))
}

/// What follows `wh build ACTION`: a docket.
pub fn build_args() -> ArgValueCompleter {
    ArgValueCompleter::new(|typed: &OsStr| {
        let typed = typed.to_str().unwrap_or_default();
        match after("build", &before()).as_slice() {
            [a] if a == "start" => get("/api/state").map(|s| docket_candidates(&s, typed)).unwrap_or_default(),
            [a] if a == "discard" => get("/api/build").map(|b| build_candidates(&b, typed)).unwrap_or_default(),
            _ => vec![],
        }
    })
}

fn get(path: &str) -> Option<Value> {
    let (status, body) = crate::client::call("GET", path, None, PATIENCE).ok()?;
    (200..300).contains(&status).then(|| serde_json::from_slice(&body).ok())?
}

/// The inbox `wh inbox` shows. Reading a message would mark it read; a Tab
/// only lists them.
fn inbox() -> Option<Value> {
    get(&format!("/api/messages{}", crate::block_q("?")))
}

/// The words before the one being completed, from the hook's call: for
/// `wh -- wh review post <Tab>`, `[wh, review, post]`.
fn before() -> Vec<String> {
    let args: Vec<String> = std::env::args().collect();
    let Some(start) = args.iter().position(|a| a == "--") else { return vec![] };
    let words = &args[start + 1..];
    let index = std::env::var("_CLAP_COMPLETE_INDEX").ok().and_then(|i| i.parse().ok()).unwrap_or(words.len());
    words[..index.min(words.len())].to_vec()
}

/// The words after `wh SUB`, its flags left out.
fn after(sub: &str, words: &[String]) -> Vec<String> {
    words.iter().skip(1).skip_while(|w| *w != sub).skip(1).filter(|w| !w.starts_with('-')).cloned().collect()
}

/// Whether bash asked: it puts a value in the line as it is, so one with a
/// space has to come quoted (zsh quotes what it puts in itself).
fn bash() -> bool {
    std::env::var_os("_CLAP_COMPLETE_COMP_TYPE").is_some()
}

fn cand(value: impl Into<String>, help: impl Into<String>) -> CompletionCandidate {
    let help: String = help.into();
    CompletionCandidate::new(value.into()).help((!help.is_empty()).then(|| help.into()))
}

fn line(s: &str, n: usize) -> String {
    crate::truncate(s.lines().next().unwrap_or_default().trim(), n)
}

fn starting(typed: &str, list: Vec<CompletionCandidate>) -> Vec<CompletionCandidate> {
    list.into_iter().filter(|c| c.get_value().to_string_lossy().starts_with(typed)).collect()
}

/// In-flight dockets, newest first, with their titles; closed ones too
/// once a number is typed (`D-2<Tab>`), or when none is in flight. As it's
/// typed: `D-12`, `d-12` or `12`.
fn docket_candidates(state: &Value, typed: &str) -> Vec<CompletionCandidate> {
    let (lead, digits) = typed.split_at(typed.find(|c: char| c.is_ascii_digit()).unwrap_or(typed.len()));
    if !digits.chars().all(|c| c.is_ascii_digit()) {
        return vec![];
    }
    let prefix = match lead {
        "" if !digits.is_empty() => "",
        "" | "D" | "D-" => "D-",
        "d" | "d-" => "d-",
        _ => return vec![],
    };
    let closed = |d: &Value| matches!(d["status"].as_str(), Some("done" | "dropped"));
    let matching = state["dockets"].as_array().into_iter().flatten().filter(|d| d["id"].as_str().and_then(|i| i.strip_prefix("D-")).is_some_and(|n| n.starts_with(digits)));
    let (open, shut): (Vec<&Value>, Vec<&Value>) = matching.partition(|d| !closed(d));
    let shown = if digits.is_empty() && !open.is_empty() { open } else { open.into_iter().chain(shut).collect() };
    shown
        .into_iter()
        .map(|d| {
            let n = d["id"].as_str().unwrap_or_default().trim_start_matches("D-");
            let title = line(d["title"].as_str().unwrap_or_default(), 70);
            let help = if closed(d) { format!("{} · {title}", d["status"].as_str().unwrap_or_default()) } else { title };
            cand(format!("{prefix}{n}"), help)
        })
        .collect()
}

fn message_candidates(list: &Value) -> Vec<CompletionCandidate> {
    list.as_array()
        .into_iter()
        .flatten()
        .map(|m| {
            let what = m["subject"].as_str().or(m["body"].as_str()).unwrap_or_default();
            cand(m["id"].to_string(), format!("from {} · {}", m["from"].as_str().unwrap_or("?"), line(what, 60)))
        })
        .collect()
}

fn choice_candidates(list: &Value, n: &str, typed: &str, quote: bool) -> Vec<CompletionCandidate> {
    let m = list.as_array().into_iter().flatten().find(|m| m["id"].as_i64().is_some_and(|id| n.trim_start_matches('#').parse() == Ok(id)));
    let options = m.and_then(|m| m["options"].as_array()).into_iter().flatten().filter_map(Value::as_str).filter(|o| o.starts_with(typed));
    options
        .map(|o| if quote && o.contains(|c: char| !(c.is_alphanumeric() || "-_.,/+@%".contains(c))) { format!("'{}'", o.replace('\'', r"'\''")) } else { o.to_string() })
        .map(|o| cand(o, ""))
        .collect()
}

/// Each agent but this pane, by an address that names only it: its
/// `session/pane`, or the end of its block id where two share a label.
fn address_candidates(state: &Value, own: Option<&str>) -> Vec<CompletionCandidate> {
    fn label(a: &Value) -> (&str, &str) {
        (a["rex_session"].as_str().unwrap_or_default(), a["pane"].as_str().unwrap_or_default())
    }
    let agents: Vec<&Value> = state["agents"].as_array().into_iter().flatten().collect();
    agents
        .iter()
        .filter(|a| a["block"].as_str() != own)
        .map(|a| {
            let (session, pane) = label(a);
            let block = a["block"].as_str().unwrap_or_default();
            let shared = agents.iter().filter(|b| label(b) == (session, pane)).count() > 1;
            let addr = if session.is_empty() {
                block.to_string()
            } else if pane.is_empty() || shared {
                format!("{session}/{}", crate::messages::short(block))
            } else {
                format!("{session}/{pane}")
            };
            let on = a["docket"].as_str().map(|d| format!(" on {d}")).unwrap_or_default();
            cand(addr, format!("{}{on} · {}", crate::mirror::app_name(a["app"].as_str().unwrap_or("agent")), a["state"].as_str().unwrap_or("?")))
        })
        .collect()
}

/// `post-mode` takes a mode; `preview`, `post` and `dismiss` a PR it's
/// following; `pickup` one of the config's repos, then the PR's number.
fn review_candidates(after: &[String], typed: &str, review: impl Fn() -> Option<Value>) -> Vec<CompletionCandidate> {
    let following = |v: &Value| -> Vec<Value> {
        v["prs"].as_array().into_iter().flatten().filter(|p| !matches!(p["state"].as_str(), Some("merged" | "closed" | "cleaned" | "unrequested"))).cloned().collect()
    };
    let about = |p: &Value| format!("{} · {}", p["state"].as_str().unwrap_or("?"), line(p["title"].as_str().unwrap_or_default(), 60));
    let list = match after {
        [a] if a == "post-mode" => POST_MODE.iter().map(|(w, h)| cand(*w, *h)).collect(),
        [a] if matches!(a.as_str(), "preview" | "post" | "dismiss") => {
            review().map(|v| following(&v).iter().map(|p| cand(p["full_key"].as_str().unwrap_or_default(), about(p))).collect()).unwrap_or_default()
        }
        [a] if a == "pickup" => {
            review().map(|v| v["config"]["repos"].as_array().into_iter().flatten().filter_map(Value::as_str).map(|r| cand(r, "then the PR's number")).collect()).unwrap_or_default()
        }
        [a, repo] if matches!(a.as_str(), "preview" | "post" | "dismiss") && !repo.contains('#') => {
            review().map(|v| following(&v).iter().filter(|p| p["repo"].as_str() == Some(repo)).map(|p| cand(p["pr"].to_string(), about(p))).collect()).unwrap_or_default()
        }
        _ => vec![],
    };
    starting(typed, list)
}

fn build_candidates(build: &Value, typed: &str) -> Vec<CompletionCandidate> {
    let tasks = build["tasks"].as_array().into_iter().flatten();
    starting(
        typed,
        tasks.map(|t| cand(t["docket"].as_str().unwrap_or_default(), format!("{} · {}", t["key"].as_str().unwrap_or("?"), t["state"].as_str().unwrap_or("?")))).collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn values(c: &[CompletionCandidate]) -> Vec<String> {
        c.iter().map(|c| c.get_value().to_string_lossy().into_owned()).collect()
    }

    fn helps(c: &[CompletionCandidate]) -> Vec<String> {
        c.iter().map(|c| c.get_help().map(|h| h.to_string()).unwrap_or_default()).collect()
    }

    /// What clap's engine offers for the last word, as the hook would ask.
    /// (With nothing typed it offers the flags too; these leave them out.)
    fn tab(line: &[&str]) -> Vec<String> {
        let mut cmd = <crate::Cli as clap::CommandFactory>::command();
        let args = line.iter().map(Into::into).collect();
        let all = values(&clap_complete::engine::complete(&mut cmd, args, line.len() - 1, None).unwrap());
        all.into_iter().filter(|v| line.last().is_some_and(|l| l.starts_with('-')) || !v.starts_with('-')).collect()
    }

    #[test]
    fn commands_flags_and_fixed_words() {
        let top = tab(&["wh", ""]);
        for c in ["show", "send", "review", "build", "completions", "doctor"] {
            assert!(top.contains(&c.to_string()), "{c} in {top:?}");
        }
        assert!(!top.contains(&"hook".to_string()), "the hooks' own command stays hidden");
        assert_eq!(tab(&["wh", "rev"]), ["review"]);
        assert_eq!(tab(&["wh", "review", "p"]), ["pause", "post-mode", "pickup", "preview", "post"]);
        assert_eq!(tab(&["wh", "build", ""]), ["status", "start", "discard", "pause", "resume"]);
        assert_eq!(tab(&["wh", "set", "D-4", "--status", ""]), ["todo", "in_progress", "done", "dropped"]);
        assert_eq!(tab(&["wh", "recap", ""]), ["today", "week", "30d"]);
        assert_eq!(tab(&["wh", "completions", ""]), ["zsh", "bash"]);
        assert_eq!(tab(&["wh", "hooks", "i"]), ["install"]);
        assert_eq!(tab(&["wh", "import", ""]), ["rex-reviewer"]);
        assert_eq!(tab(&["wh", "send", "--s"]), ["--subject"]);
        assert_eq!(tab(&["wh", "check", "D-4", "--"]), ["--wait", "--help"]);
    }

    fn world() -> Value {
        json!({
            "dockets": [
                { "id": "D-14", "status": "in_progress", "title": "Retry the uploads\nwith backoff" },
                { "id": "D-13", "status": "done", "title": "Fix the login" },
                { "id": "D-12", "status": "todo", "title": "Rate limits" },
                { "id": "D-1", "status": "dropped", "title": "First" },
            ],
            "agents": [
                { "block": "block:aaaa-111111", "rex_session": "api", "pane": "planner", "app": "claude-code", "state": "working", "docket": "D-14" },
                { "block": "block:bbbb-222222", "rex_session": "api", "pane": "shell", "app": "codex", "state": "done", "docket": "D-12" },
                { "block": "block:cccc-333333", "rex_session": "api", "pane": "shell", "app": "claude-code", "state": "blocked", "docket": null },
                { "block": "block:dddd-444444", "rex_session": "", "pane": "", "app": "claude-code", "state": "working", "docket": "D-1" },
            ],
        })
    }

    #[test]
    fn dockets_in_flight_first_and_closed_once_narrowed() {
        let w = world();
        let c = docket_candidates(&w, "");
        assert_eq!(values(&c), ["D-14", "D-12"], "in flight only, newest first");
        assert_eq!(helps(&c), ["Retry the uploads", "Rate limits"], "a title's first line");
        assert_eq!(values(&docket_candidates(&w, "D-")), ["D-14", "D-12"]);
        let c = docket_candidates(&w, "D-1");
        assert_eq!(values(&c), ["D-14", "D-12", "D-13", "D-1"], "closed ones once a number's typed, after the rest");
        assert_eq!(helps(&c)[2], "done · Fix the login");
        assert_eq!(values(&docket_candidates(&w, "d-1")), ["d-14", "d-12", "d-13", "d-1"], "as it's typed");
        assert_eq!(values(&docket_candidates(&w, "13")), ["13"], "a bare number, as `wh show 13` takes");
        assert!(docket_candidates(&w, "x").is_empty() && docket_candidates(&w, "D-1x").is_empty() && docket_candidates(&w, "--").is_empty());
        let none_open = json!({ "dockets": [{ "id": "D-3", "status": "done", "title": "Old" }] });
        assert_eq!(values(&docket_candidates(&none_open, "")), ["D-3"], "nothing in flight: the closed ones");
    }

    #[test]
    fn addresses_name_one_pane_each() {
        let c = address_candidates(&world(), Some("block:aaaa-111111"));
        assert_eq!(values(&c), ["api/222222", "api/333333", "block:dddd-444444"], "not this pane; a shared label by its block");
        assert_eq!(helps(&c)[0], "Codex on D-12 · done");
        assert_eq!(helps(&c)[1], "Claude Code · blocked");
        assert_eq!(values(&address_candidates(&world(), None))[0], "api/planner");
    }

    #[test]
    fn messages_and_their_options() {
        let inbox = json!([
            { "id": 41, "from": "api/planner", "subject": "Migration order", "body": "…", "options": ["Users first", "Together", "Don't"] },
            { "id": 42, "from": "me", "subject": null, "body": "Is /v2 paginated?\nAsking for the client", "options": [] },
        ]);
        let c = message_candidates(&inbox);
        assert_eq!(values(&c), ["41", "42"]);
        assert_eq!(helps(&c), ["from api/planner · Migration order", "from me · Is /v2 paginated?"]);
        assert_eq!(values(&choice_candidates(&inbox, "41", "", false)), ["Users first", "Together", "Don't"], "zsh quotes them itself");
        assert_eq!(values(&choice_candidates(&inbox, "41", "", true)), ["'Users first'", "Together", r"'Don'\''t'"], "bash doesn't");
        assert_eq!(values(&choice_candidates(&inbox, "41", "T", true)), ["Together"]);
        assert!(choice_candidates(&inbox, "42", "", false).is_empty() && choice_candidates(&inbox, "7", "", false).is_empty());
    }

    #[test]
    fn review_takes_a_mode_or_a_pr() {
        let review = || {
            Some(json!({
                "config": { "repos": ["acme/api", "acme/web"] },
                "prs": [
                    { "full_key": "acme/api#12", "repo": "acme/api", "pr": 12, "state": "posted", "title": "Add retries" },
                    { "full_key": "acme/api#9", "repo": "acme/api", "pr": 9, "state": "cleaned", "title": "Old" },
                    { "full_key": "acme/web#3", "repo": "acme/web", "pr": 3, "state": "reviewing", "title": "Dark mode" },
                ],
            }))
        };
        let w = |s: &[&str]| s.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(values(&review_candidates(&w(&["post-mode"]), "", || panic!("no hub needed"))), ["ask", "auto", "config"]);
        let c = review_candidates(&w(&["post"]), "", review);
        assert_eq!(values(&c), ["acme/api#12", "acme/web#3"], "only PRs it still follows");
        assert_eq!(helps(&c)[0], "posted · Add retries");
        assert_eq!(values(&review_candidates(&w(&["preview"]), "acme/w", review)), ["acme/web#3"]);
        assert_eq!(values(&review_candidates(&w(&["dismiss", "acme/api"]), "", review)), ["12"], "OWNER/NAME N");
        assert_eq!(values(&review_candidates(&w(&["pickup"]), "", review)), ["acme/api", "acme/web"]);
        assert!(review_candidates(&w(&["post", "acme/api#12"]), "", review).is_empty() && review_candidates(&w(&[]), "", review).is_empty());
    }

    #[test]
    fn builds_to_discard() {
        let b = json!({ "tasks": [{ "docket": "D-35", "key": "ENG-500", "state": "merged" }, { "docket": "D-40", "key": "acme/api#7", "state": "building" }] });
        let c = build_candidates(&b, "D-4");
        assert_eq!(values(&c), ["D-40"]);
        assert_eq!(helps(&c), ["acme/api#7 · building"]);
    }

    #[test]
    fn the_words_before_the_tab() {
        let words = ["wh", "--json", "review", "--force", "post", "acme/api"].map(String::from);
        assert_eq!(after("review", &words), ["post", "acme/api"]);
        assert!(after("build", &words).is_empty());
    }

    #[test]
    fn the_hooks_call_wh_back() {
        let zsh = String::from_utf8(script(Shell::Zsh).unwrap()).unwrap();
        assert!(zsh.contains("WH_COMPLETE=\"zsh\"") && zsh.contains("function _clap_dynamic_completer_wh()"), "{zsh}");
        // Registered at the first prompt only: if clap's hook changes, this says so.
        assert_eq!(zsh.matches(ZSH_REGISTER).count(), 1, "{zsh}");
        assert!(zsh.contains("add-zsh-hook precmd _wh_compdef\n") && zsh.trim_end().ends_with("export WH_TAB=zsh"), "{zsh}");
        let bash = String::from_utf8(script(Shell::Bash).unwrap()).unwrap();
        assert!(bash.contains("WH_COMPLETE=\"bash\"") && bash.contains("-F _clap_complete_wh wh") && bash.contains("-F _wh_complete wh"), "{bash}");
    }
}
