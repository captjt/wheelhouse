//! Wheelhouse: every piece of agent work as a docket, on a record you can
//! check. `wh serve` runs the hub; everything else talks to it.

// A lock taken in an `if let` or `match` scrutinee is held to the end of
// the block; locking again inside hangs the hub. Bind the result first.
#![warn(clippy::significant_drop_in_scrutinee)]

macro_rules! say {
    ($($t:tt)*) => { eprintln!("{} {}", crate::time::now(), format!($($t)*)) };
}

mod agents;
mod approve;
mod artifacts;
mod brief;
mod build;
mod checks;
mod client;
mod complete;
mod decisions;
mod diffs;
mod doctor;
mod evidence;

mod compose;
mod hooks;
mod hub;
mod linear;
mod messages;
mod mirror;
mod notify;
mod paths;
mod preview;
mod pricing;
mod push;
mod recap;
mod record;
mod review;
mod runtime;
mod server;
mod service;
mod settings;
mod start;
mod store;
mod subagents;
mod tailnet;
mod team;
mod time;
mod usage;

use anyhow::{Context, Result, bail};
use clap::{CommandFactory, Parser, Subcommand};
use serde_json::{Value, json};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "wh", version, about = "Wheelhouse: every piece of agent work as a docket, on a record you can check")]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run the hub: follow Rex, keep the record, serve the app.
    /// --workspace runs a workspace hub instead (docs/teams.md).
    Serve {
        #[arg(long)]
        workspace: bool,
    },
    /// Open dockets, newest first (all of them with --all).
    Ls {
        #[arg(long)]
        all: bool,
    },
    /// One docket's record.
    Show {
        #[arg(add = complete::dockets())]
        docket: String,
    },
    /// Open a docket by hand, or from a Linear issue (--linear ENG-142);
    /// --team opens it on your workspace, shared.
    New {
        title: Option<String>,
        #[arg(long)]
        linear: Option<String>,
        #[arg(long, conflicts_with = "linear")]
        team: bool,
    },
    /// Teams: your workspace, its members and shared dockets (docs/teams.md).
    /// Alone, how it is: connected, members, shared dockets, the outbox.
    Team {
        #[command(subcommand)]
        cmd: Option<TeamCmd>,
    },
    /// Link a Linear issue to a docket.
    Link {
        #[arg(add = complete::dockets())]
        docket: String,
        issue: String,
    },
    /// A docket's intent: print it, or set it (TEXT, or - for stdin).
    Intent {
        #[arg(add = complete::dockets())]
        docket: String,
        text: Option<String>,
    },
    /// What an agent reads to work on a docket (its own, from its pane).
    Brief {
        #[arg(add = complete::dockets())]
        docket: Option<String>,
    },
    /// Message an agent's pane, or you (`--to me`), with options as buttons.
    Send {
        #[arg(long, add = complete::addresses())]
        to: String,
        #[arg(long)]
        subject: Option<String>,
        #[arg(long = "option")]
        options: Vec<String>,
        /// The message, or - for stdin.
        text: Option<String>,
    },
    /// Answer message N: words, an option (--choice), or both.
    Reply {
        #[arg(add = complete::messages())]
        n: i64,
        #[arg(long, add = complete::choices())]
        choice: Option<String>,
        text: Option<String>,
    },
    /// Messages to this pane (or to you, from a shell) not read yet.
    Inbox {
        #[arg(long)]
        json: bool,
    },
    /// Read message N.
    Read {
        #[arg(add = complete::messages())]
        n: i64,
        #[arg(long)]
        json: bool,
    },
    /// Wait for a message to this pane (or a reply in --thread N).
    Wait {
        #[arg(long)]
        thread: Option<i64>,
        #[arg(long, default_value_t = 300)]
        timeout: u64,
        #[arg(long)]
        json: bool,
    },
    /// Name this pane, so others can message it by that name.
    Name { name: String },
    /// This pane's address and docket.
    Whoami,
    /// Put the agent in this pane on a docket, beside the agents already on
    /// it. It leaves the docket it was on.
    Join {
        #[arg(add = complete::dockets())]
        docket: String,
        /// What it's there for, shown beside it: reviewer, tests, docs.
        #[arg(long)]
        role: Option<String>,
    },
    /// Take this pane off its docket. The agents still on it keep it; this
    /// pane gets a docket of its own.
    Leave {
        #[arg(long)]
        reason: Option<String>,
    },
    /// Start Claude Code, Codex, Antigravity or Pi on a new docket, in a
    /// folder, with a prompt; --docket D-12 works on that docket instead.
    Start {
        #[arg(long)]
        codex: bool,
        #[arg(long, conflicts_with = "codex")]
        agy: bool,
        #[arg(long, conflicts_with_all = ["codex", "agy"])]
        pi: bool,
        #[arg(long)]
        docket: Option<String>,
        #[arg(short = 'C', long, default_value = ".", value_hint = clap::ValueHint::DirPath)]
        dir: String,
        #[arg(long)]
        title: Option<String>,
        prompt: Option<String>,
    },
    /// Show the user an app this pane serves on PORT, on the tailnet, with a
    /// question and answers as buttons. --stop takes it (or all yours) down.
    Share {
        port: Option<u16>,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        ask: Option<String>,
        #[arg(long = "option")]
        options: Vec<String>,
        #[arg(long)]
        stop: bool,
    },
    /// Reviews: status, on, off, pause, resume, check,
    /// post-mode ask|auto|config, pickup, preview, post, dismiss.
    /// A PR is OWNER/NAME N (or OWNER/NAME#N).
    Review {
        #[arg(add = complete::words(complete::REVIEW))]
        action: Option<String>,
        #[arg(add = complete::review_args())]
        args: Vec<String>,
        /// With `on`: even though rex-reviewer still runs.
        #[arg(long)]
        force: bool,
        #[arg(long)]
        json: bool,
    },
    /// Move over from another tool, once: `wh import rex-reviewer` brings its
    /// config, guides and ledger into Wheelhouse.
    Import {
        #[arg(add = complete::words(complete::IMPORT))]
        from: String,
        #[arg(long)]
        force: bool,
    },
    /// Build: `start D-12|owner/name#45|ENG-142 [--repo owner/name]`,
    /// `discard D-12`, `pause`, `resume`, or the list.
    Build {
        #[arg(add = complete::words(complete::BUILD))]
        action: Option<String>,
        #[arg(add = complete::build_args())]
        args: Vec<String>,
        #[arg(long)]
        repo: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// What happened: today, this week (the default), or 30 days; as Markdown.
    Recap {
        #[arg(add = complete::words(complete::RECAP))]
        period: Option<String>,
    },
    /// The agents' skill for Wheelhouse: show, install, or remove.
    Skill {
        #[arg(add = complete::words(complete::SKILL))]
        action: Option<String>,
    },
    /// Store a Linear API key in the Keychain.
    SetupLinear,
    /// Add a note to a docket. From an agent's pane, the note is the agent's.
    Note {
        #[arg(add = complete::dockets())]
        docket: String,
        text: String,
    },
    /// Keep a file (a report, a page, an image) or a link (a Claude artifact)
    /// with a docket: the record pins a file by its hash. From an agent's
    /// pane, its own docket.
    Attach {
        /// A file, or an https:// link.
        #[arg(value_hint = clap::ValueHint::FilePath)]
        what: String,
        /// What it is, for people. Defaults to the file's name.
        #[arg(long)]
        title: Option<String>,
        #[arg(long, add = complete::dockets())]
        docket: Option<String>,
    },
    /// Rename a docket, or set its status (todo, in_progress, done, dropped).
    /// A docket with acceptance items closes on evidence (a passing check,
    /// a merged PR), or on your word: --anyway --reason "…".
    Set {
        #[arg(add = complete::dockets())]
        docket: String,
        #[arg(long)]
        title: Option<String>,
        #[arg(long, add = complete::words(complete::STATUS))]
        status: Option<String>,
        /// Close it without evidence, on your word (the record says so).
        #[arg(long)]
        anyway: bool,
        #[arg(long)]
        reason: Option<String>,
    },
    /// Run a docket's witnessed checks now (its repo's `checks` in
    /// build.lua or Settings), in its worktree; --wait for the results.
    Check {
        #[arg(add = complete::dockets())]
        docket: String,
        #[arg(long)]
        wait: bool,
    },
    /// Hold a docket: its protocol takes no steps until you resume it
    /// (--off). The reason goes on the record.
    Hold {
        #[arg(add = complete::dockets())]
        docket: String,
        reason: Option<String>,
        #[arg(long)]
        off: bool,
    },
    /// What needs you beyond your asks: failing checks, stalled Builds,
    /// held dockets, and dockets closed on your word.
    Attention,
    /// Check a docket's record (or every docket's), or an exported file.
    Verify {
        #[arg(add = complete::dockets())]
        docket: Option<String>,
        #[arg(long, value_hint = clap::ValueHint::FilePath)]
        file: Option<PathBuf>,
    },
    /// A docket's record as JSON lines, checkable without the hub.
    Export {
        #[arg(add = complete::dockets())]
        docket: String,
    },
    /// The agents' hooks: status, install, or remove.
    Hooks {
        #[arg(add = complete::words(complete::HOOKS))]
        action: Option<String>,
    },
    /// Run the hub in the background, at login (a LaunchAgent);
    /// --workspace runs a workspace hub beside it instead (docs/teams.md).
    Up {
        #[arg(long)]
        workspace: bool,
    },
    /// Stop the background hub (--workspace: the workspace hub).
    Down {
        #[arg(long)]
        workspace: bool,
    },
    /// The background hub's log.
    Logs {
        #[arg(short = 'n', default_value_t = 60)]
        lines: usize,
    },
    /// Whether the hub runs, and what it sees.
    Status,
    /// What's set up, what isn't, and what to do about each.
    Doctor,
    /// Open a docket (or the Inbox) in Wheelhouse.app, or in the web app
    /// without it
    Open {
        #[arg(add = complete::dockets())]
        docket: Option<String>,
    },
    /// The app's address on your tailnet; --publish serves it there.
    Web {
        #[arg(long)]
        publish: bool,
    },
    /// Tab completion for zsh or bash: `eval "$(wh completions zsh)"` in
    /// ~/.zshrc turns it on (bash: ~/.bashrc). Alone, it says how.
    Completions { shell: Option<complete::Shell> },
    /// Run by an agent's hooks.
    #[command(hide = true)]
    Hook {
        #[arg(long, default_value = "claude-code")]
        agent: String,
        event: String,
    },
}

#[derive(Subcommand)]
enum TeamCmd {
    /// Make a workspace on a workspace hub (wh serve --workspace printed
    /// its setup code), with you as its first admin.
    Create {
        name: String,
        /// What its shared dockets are numbered with: ACME for ACME-12.
        #[arg(long)]
        prefix: String,
        /// The workspace hub's address: https://ws.tail1234.ts.net
        #[arg(long)]
        url: String,
        #[arg(long)]
        setup: String,
        /// Your handle in it (lowercase). Defaults to your user name.
        #[arg(long)]
        handle: Option<String>,
        /// Your Tailscale login, when this Mac's Tailscale isn't up to say.
        #[arg(long)]
        login: Option<String>,
    },
    /// Invite someone: prints a code to hand them for `wh team join`.
    Invite {
        handle: String,
        #[arg(long)]
        admin: bool,
        /// How long the code works (default 24).
        #[arg(long)]
        hours: Option<i64>,
    },
    /// Join a workspace with an invite code.
    Join {
        code: String,
        #[arg(long)]
        login: Option<String>,
        /// Your name, as teammates see it.
        #[arg(long)]
        name: Option<String>,
    },
    /// Who's in the workspace.
    Members,
    /// Share a docket of yours with the workspace: it goes on as ACME-n,
    /// naming its history (which stays on this Mac), and you as its first owner.
    Share {
        #[arg(add = complete::dockets())]
        docket: String,
    },
    /// Add someone to a shared docket (or yourself), make them an owner,
    /// take them off it, or say who answers its agents' questions.
    People {
        docket: String,
        handle: Option<String>,
        #[arg(long)]
        owner: bool,
        #[arg(long)]
        remove: bool,
        /// owners (the agent's own person and the owners) or members.
        #[arg(long)]
        answer: Option<String>,
    },
    /// Let a teammate's agents' messages into your panes on a shared docket
    /// without asking you (--off to ask again); --private-prompts on|off
    /// keeps your prompts on it to yourself.
    Trust {
        docket: String,
        teammate: Option<String>,
        #[arg(long)]
        off: bool,
        #[arg(long)]
        private_prompts: Option<String>,
    },
    /// Make a member an admin, or an admin a member.
    Role { handle: String, role: String },
    /// Take a member out of the workspace (admins), from now on.
    Remove {
        handle: String,
        #[arg(long)]
        reason: Option<String>,
    },
    /// Leave the workspace yourself.
    Leave,
    /// Send an entry the outbox held back (it looked like a secret).
    Send { n: i64 },
    /// Drop an entry from the outbox, held or refused.
    Drop { n: i64 },
    /// Send the outbox and take in what's new, now.
    Sync,
    /// Follow a shared docket again that this hub stopped following (it
    /// refused something the workspace served): from where it stopped.
    Follow { docket: String },
}

fn main() {
    complete::answer(Cli::command);
    let cli = Cli::parse();
    let result = match cli.cmd {
        Some(Cmd::Serve { workspace: false }) => serve(),
        Some(Cmd::Serve { workspace: true }) => team::workspace::serve(),
        Some(Cmd::Hook { agent, event }) => hooks::run(&agent, &event),
        Some(Cmd::Hooks { action }) => hooks::cli(action.as_deref()),
        Some(Cmd::Up { workspace: false }) => service::up(),
        Some(Cmd::Up { workspace: true }) => service::up_workspace(),
        Some(Cmd::Down { workspace: false }) => service::down(),
        Some(Cmd::Down { workspace: true }) => service::down_workspace(),
        Some(Cmd::Logs { lines }) => service::logs(lines),
        Some(Cmd::Status) => status(),
        Some(Cmd::Doctor) => doctor::run(),
        Some(Cmd::Completions { shell }) => complete::cli(shell),
        Some(Cmd::Web { publish }) => tailnet::cli(publish),
        Some(Cmd::Open { docket }) => open(docket.as_deref().map(normalize)),
        Some(Cmd::Ls { all }) => ls(all),
        None => ls(false),
        Some(Cmd::Show { docket }) => show(&normalize(&docket)),
        Some(Cmd::New { title, linear, team: false }) => new_docket(title, linear),
        Some(Cmd::New { title, team: true, .. }) => {
            let t = title.ok_or_else(|| anyhow::anyhow!("a title: wh new --team \"Rate limits\""));
            t.and_then(|t| client::json("POST", "/api/team/new", Some(&json!({ "title": t })))).map(|v| println!("{}", v["docket"].as_str().unwrap_or("?")))
        }
        Some(Cmd::Team { cmd }) => team_cmd(cmd),
        Some(Cmd::Link { docket, issue }) => {
            let d = normalize(&docket);
            client::json("POST", &format!("/api/dockets/{d}/linear"), Some(&json!({ "key": issue }))).map(|_| println!("{d} linked to {issue}"))
        }
        Some(Cmd::Intent { docket, text }) => intent(&normalize(&docket), text),
        Some(Cmd::Brief { docket }) => brief(docket.as_deref().map(normalize)),
        Some(Cmd::SetupLinear) => linear::setup(),
        Some(Cmd::Start { codex, agy, pi, docket, dir, title, prompt }) => {
            let cwd = std::fs::canonicalize(&dir).map(|p| p.to_string_lossy().into_owned()).unwrap_or(dir);
            let prompt = match prompt.as_deref() {
                Some("-") => text_or_stdin(prompt.clone()).ok(),
                _ => prompt,
            };
            let (app, label) = if codex {
                ("codex", "Codex")
            } else if agy {
                ("agy", "Antigravity")
            } else if pi {
                ("pi", "Pi")
            } else {
                ("claude-code", "Claude Code")
            };
            client::json("POST", "/api/agents/start", Some(&json!({ "app": app, "cwd": cwd, "prompt": prompt, "title": title, "docket": docket.as_deref().map(normalize) }))).map(
                |v| {
                    let beside = match v["others"].as_u64().unwrap_or(0) {
                        0 => String::new(),
                        1 => ", beside 1 other agent".into(),
                        n => format!(", beside {n} other agents"),
                    };
                    println!("{label} started in {}: {}{beside}", v["docket"].as_str().unwrap_or("?"), v["pane"].as_str().unwrap_or("?"))
                },
            )
        }
        Some(Cmd::Skill { action }) => skill(action.as_deref()),
        Some(Cmd::Import { from, force }) => match from.as_str() {
            "rex-reviewer" => client::json("POST", "/api/import/rex-reviewer", Some(&json!({ "force": force }))).map(|v| {
                for p in v["config"].as_array().into_iter().flatten() {
                    println!("wrote {}", p.as_str().unwrap_or("?"));
                }
                println!("imported {} PR(s) from its ledger (PRs Wheelhouse already had are left alone)", v["prs"]);
            }),
            other => Err(anyhow::anyhow!("wh import takes rex-reviewer, not {other}")),
        },
        Some(Cmd::Build { action, args, repo, json: as_json }) => build_cmd(action.as_deref().unwrap_or("status"), &args, repo, as_json),
        Some(Cmd::Recap { period }) => {
            let p = period.as_deref().unwrap_or("week");
            client::json("GET", &format!("/api/recap?period={}", enc(p)), None).map(|v| print!("{}", v["markdown"].as_str().unwrap_or("")))
        }
        Some(Cmd::Review { action, args, force, json }) => review_cmd(action.as_deref().unwrap_or("status"), &args, force, json),
        Some(Cmd::Share { port, title, ask, options, stop }) => {
            let block = crate::runtime::pane_from_env();
            if stop {
                client::json("POST", "/api/share/stop", Some(&json!({ "port": port, "block": block }))).map(|v| println!("stopped {} preview(s)", v["stopped"]))
            } else {
                let port = port.ok_or_else(|| anyhow::anyhow!("which port? wh share 5173"));
                port.and_then(|p| client::json("POST", "/api/share", Some(&json!({ "port": p, "title": title, "ask": ask, "options": options, "block": block })))).map(|v| {
                    println!("#{} in the user's inbox: {}", v["message"], v["url"].as_str().unwrap_or("?"));
                    println!("their answer comes back as a reply: wh wait --thread {}", v["message"]);
                })
            }
        }
        Some(Cmd::Send { to, subject, options, text }) => msg_send(&to, subject, options, text),
        Some(Cmd::Reply { n, choice, text }) => msg_reply(n, choice, text),
        Some(Cmd::Inbox { json }) => msg_inbox(json),
        Some(Cmd::Read { n, json }) => msg_read(n, json),
        Some(Cmd::Wait { thread, timeout, json }) => msg_wait(thread, timeout, json),
        Some(Cmd::Name { name }) => {
            let block = crate::runtime::pane_from_env().ok_or_else(|| anyhow::anyhow!("run this in a Rex pane or a Herdr terminal"));
            block.and_then(|b| client::json("POST", "/api/panes/name", Some(&json!({ "block": b, "name": name })))).map(|v| {
                println!("this pane is {}", v["addr"].as_str().unwrap_or("?"));
            })
        }
        Some(Cmd::Join { docket, role }) => {
            let d = normalize(&docket);
            let block = crate::runtime::pane_from_env().ok_or_else(|| anyhow::anyhow!("run this in an agent's Rex pane or Herdr terminal"));
            block.and_then(|b| client::json("POST", "/api/join", Some(&json!({ "block": b, "docket": d, "role": role })))).map(|v| {
                let n = v["others"].as_u64().unwrap_or(0);
                let beside = match n {
                    0 => String::new(),
                    1 => " beside 1 other agent".into(),
                    n => format!(" beside {n} other agents"),
                };
                if v["already"] == true {
                    println!("already on {d}{beside}");
                } else {
                    println!("on {d}{beside}. `wh brief` says who they are");
                }
            })
        }
        Some(Cmd::Leave { reason }) => {
            let block = crate::runtime::pane_from_env().ok_or_else(|| anyhow::anyhow!("run this in an agent's Rex pane or Herdr terminal"));
            block.and_then(|b| client::json("POST", "/api/leave", Some(&json!({ "block": b, "reason": reason })))).map(|v| {
                println!("left {}{}", v["left"].as_str().unwrap_or("?"), v["docket"].as_str().map(|d| format!("; this pane is on {d} now")).unwrap_or_default());
            })
        }
        Some(Cmd::Whoami) => client::json("GET", &format!("/api/whoami{}", block_q("?")), None).map(|v| {
            println!("{}{}", v["addr"].as_str().unwrap_or("?"), v["docket"].as_str().map(|d| format!(" on {d}")).unwrap_or_default());
        }),
        Some(Cmd::Note { docket, text }) => {
            let block = crate::runtime::pane_from_env();
            let d = normalize(&docket);
            client::json("POST", &format!("/api/dockets/{d}/note"), Some(&json!({ "body": text, "block": block }))).map(|v| println!("{}", placed(&d, &v)))
        }
        Some(Cmd::Attach { what, title, docket }) => attach(&what, title, docket),
        Some(Cmd::Set { docket, title, status, anyway, reason }) => {
            let d = normalize(&docket);
            client::json("POST", &format!("/api/dockets/{d}/set"), Some(&json!({ "title": title, "status": status, "anyway": anyway, "reason": reason })))
                .map(|v| println!("{}", placed(&d, &v)))
        }
        Some(Cmd::Check { docket, wait }) => check(&normalize(&docket), wait),
        Some(Cmd::Hold { docket, reason, off }) => {
            let d = normalize(&docket);
            client::json("POST", &format!("/api/dockets/{d}/hold"), Some(&json!({ "reason": reason, "off": off }))).map(|v| {
                println!("{d} {} #{}", if off { "resumed" } else { "held" }, v["seq"]);
            })
        }
        Some(Cmd::Attention) => client::json("GET", "/api/state", None).map(|v| {
            let items = v["attention"].as_array().cloned().unwrap_or_default();
            if items.is_empty() {
                println!("Nothing needs your attention beyond your asks.");
            }
            for a in items {
                println!("{:<6} {:<9} {}", a["docket"].as_str().unwrap_or("-"), a["kind"].as_str().unwrap_or(""), a["text"].as_str().unwrap_or(""));
            }
        }),
        Some(Cmd::Verify { docket, file }) => verify(docket.as_deref().map(normalize), file),
        Some(Cmd::Export { docket }) => client::text(&format!("/api/dockets/{}/export", normalize(&docket))).map(|t| print!("{t}")),
    };
    if let Err(e) = result {
        eprintln!("wh: {e:#}");
        std::process::exit(1);
    }
}

/// Where an entry went: its place, or the outbox for a shared docket.
fn placed(d: &str, v: &Value) -> String {
    match v["seq"].as_i64() {
        Some(0) => format!("{d}: sent to the workspace"),
        Some(n) => format!("{d} #{n}"),
        None => d.to_string(),
    }
}

fn team_cmd(cmd: Option<TeamCmd>) -> Result<()> {
    let call = |path: &str, body: Value| client::json_within("POST", path, Some(&body), std::time::Duration::from_secs(60));
    match cmd {
        None => {
            let v = client::json("GET", "/api/team", None)?;
            print_team(&v);
        }
        Some(TeamCmd::Create { name, prefix, url, setup, handle, login }) => {
            let handle = handle.unwrap_or_else(|| std::env::var("USER").unwrap_or_default().to_ascii_lowercase());
            let v = call("/api/team/create", json!({ "name": name, "prefix": prefix, "url": url, "setup": setup, "handle": handle, "login": login }))?;
            println!("Made {name} ({}), with you as {handle}, its first admin. Invite someone: wh team invite HANDLE", v["prefix"].as_str().unwrap_or(&prefix));
        }
        Some(TeamCmd::Invite { handle, admin, hours }) => {
            let v = call("/api/team/invite", json!({ "handle": handle, "admin": admin, "hours": hours }))?;
            println!("An invite for {handle}, good for {} hours. Hand them this, and they run wh team join with it:\n", v["hours"]);
            println!("{}", v["code"].as_str().unwrap_or("?"));
            println!("\nIt's a secret: whoever has it can join as {handle} until it's used.");
        }
        Some(TeamCmd::Join { code, login, name }) => {
            let v = call("/api/team/join", json!({ "code": code, "login": login, "name": name }))?;
            println!(
                "You're in {} as {}. Its shared dockets are ACME-style: {}-n. wh team shows them",
                v["workspace"].as_str().unwrap_or("?"),
                v["handle"].as_str().unwrap_or("?"),
                v["prefix"].as_str().unwrap_or("?")
            );
        }
        Some(TeamCmd::Members) => {
            let v = client::json("GET", "/api/team", None)?;
            for m in v["members"].as_array().into_iter().flatten() {
                println!(
                    "  {:<12} {:<7} {}{}{}",
                    m["handle"].as_str().unwrap_or(""),
                    m["role"].as_str().unwrap_or(""),
                    m["login"].as_str().unwrap_or(""),
                    if m["active"] == false { "  (removed)" } else { "" },
                    if m["you"] == true { "  (you)" } else { "" }
                );
            }
        }
        Some(TeamCmd::Share { docket }) => {
            let v = call("/api/team/share", json!({ "docket": normalize(&docket) }))?;
            println!(
                "{} is shared as {}; its history stays on this Mac. You're its owner: wh team people {} HANDLE adds someone",
                v["from"].as_str().unwrap_or("?"),
                v["docket"].as_str().unwrap_or("?"),
                v["docket"].as_str().unwrap_or("?")
            );
        }
        Some(TeamCmd::People { docket, handle, owner, remove, answer }) => {
            let d = enc(docket.trim());
            call(&format!("/api/team/dockets/{d}/people"), json!({ "handle": handle, "role": if owner { "owner" } else { "member" }, "remove": remove, "answer": answer }))?;
            println!("{d}: sent to the workspace");
        }
        Some(TeamCmd::Trust { docket, teammate, off, private_prompts }) => {
            let d = enc(docket.trim());
            let pp = match private_prompts.as_deref() {
                None => None,
                Some("on") => Some(true),
                Some("off") => Some(false),
                Some(o) => bail!("--private-prompts on or off, not {o}"),
            };
            call(&format!("/api/team/dockets/{d}/trust"), json!({ "teammate": teammate, "on": !off, "private_prompts": pp }))?;
            println!("{d}: sent to the workspace");
        }
        Some(TeamCmd::Role { handle, role }) => {
            call("/api/team/role", json!({ "handle": handle, "role": role }))?;
            println!("{handle} is {role} now");
        }
        Some(TeamCmd::Remove { handle, reason }) => {
            call("/api/team/remove", json!({ "handle": handle, "reason": reason }))?;
            println!("{handle} is out of the workspace from now on; what they wrote before still checks");
        }
        Some(TeamCmd::Leave) => {
            let v = client::json("GET", "/api/team", None)?;
            let me = v["workspace"]["you"].as_str().ok_or_else(|| anyhow::anyhow!("this hub isn't in a workspace"))?.to_string();
            call("/api/team/remove", json!({ "handle": me, "reason": "left" }))?;
            println!("You left {}", v["workspace"]["name"].as_str().unwrap_or("the workspace"));
        }
        Some(TeamCmd::Send { n }) => {
            call(&format!("/api/team/outbox/{n}"), json!({ "action": "send" }))?;
            println!("#{n} goes to the workspace");
        }
        Some(TeamCmd::Drop { n }) => {
            call(&format!("/api/team/outbox/{n}"), json!({ "action": "drop" }))?;
            println!("#{n} dropped");
        }
        Some(TeamCmd::Follow { docket }) => {
            let d = enc(docket.trim());
            call(&format!("/api/team/dockets/{d}/follow"), json!({}))?;
            println!("{d}: following again; wh team sync to try now");
        }
        Some(TeamCmd::Sync) => {
            let v = call("/api/team/sync", json!({}))?;
            print_team(&v);
        }
    }
    Ok(())
}

fn print_team(v: &Value) {
    let w = &v["workspace"];
    if w.is_null() {
        println!("This hub isn't in a workspace. Join one with an invite: wh team join CODE (docs/teams.md)");
        return;
    }
    println!(
        "{} ({}) at {} · you're {} · {}",
        w["name"].as_str().unwrap_or("?"),
        w["prefix"].as_str().unwrap_or("?"),
        w["url"].as_str().unwrap_or("?"),
        w["you"].as_str().unwrap_or("?"),
        if v["online"] == true { "connected".to_string() } else { format!("not connected: {}", v["error"].as_str().unwrap_or("not yet")) }
    );
    let members: Vec<&str> = v["members"].as_array().into_iter().flatten().filter(|m| m["active"] != false).filter_map(|m| m["handle"].as_str()).collect();
    println!("members  {}", members.join(", "));
    for d in v["dockets"].as_array().into_iter().flatten() {
        let owners: Vec<&str> = d["owners"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
        let agents = d["agents"].as_array().map_or(0, Vec::len);
        println!(
            "  {:<9} {:<11} {}  · owners {}{}{}",
            d["id"].as_str().unwrap_or(""),
            d["status"].as_str().unwrap_or("").replace('_', " "),
            truncate(d["title"].as_str().unwrap_or(""), 44),
            owners.join(", "),
            if agents > 0 { format!(" · {agents} agent{}", if agents == 1 { "" } else { "s" }) } else { String::new() },
            d["halted"].as_str().map(|h| format!("  ✗ not followed: {h}")).unwrap_or_default()
        );
    }
    for o in v["outbox"].as_array().into_iter().flatten() {
        println!(
            "  outbox #{} {} {}{}",
            o["n"],
            o["docket"].as_str().unwrap_or(""),
            o["status"].as_str().unwrap_or(""),
            o["why"].as_str().map(|w| format!(": {w}")).unwrap_or_default()
        );
    }
}

/// `wh check D-12`: start its checks; with --wait, print how they went.
fn check(d: &str, wait: bool) -> Result<()> {
    let v = client::json("POST", &format!("/api/dockets/{d}/check"), Some(&json!({})))?;
    let run = v["run"].as_str().unwrap_or("").to_string();
    let names: Vec<String> = v["checks"].as_array().into_iter().flatten().filter_map(|x| x.as_str().map(str::to_string)).collect();
    println!("{d}: running {}", names.join(", "));
    if !wait {
        return Ok(());
    }
    let mut failed = false;
    loop {
        std::thread::sleep(std::time::Duration::from_secs(2));
        let doc = client::json("GET", &format!("/api/dockets/{d}"), None)?;
        let ev = &doc["evidence"];
        let mine: Vec<&Value> = ev["checks"].as_array().into_iter().flatten().filter(|c| c["run"] == run.as_str()).collect();
        if ev["running"] == true || mine.len() < names.len() {
            if ev["running"] != true && mine.len() < names.len() {
                anyhow::bail!("the checks stopped before they all ran; see the docket");
            }
            continue;
        }
        for c in mine {
            let ok = c["ok"] == true;
            failed |= !ok && c["required"] != false;
            println!(
                "  {} {:<10} {}  ({} ms){}",
                if ok { "✓" } else { "✗" },
                c["name"].as_str().unwrap_or(""),
                c["command"].as_str().unwrap_or(""),
                c["ms"],
                if c["required"] == false { " · optional" } else { "" }
            );
        }
        break;
    }
    if failed {
        anyhow::bail!("a required check failed");
    }
    Ok(())
}

/// `12` and `d-12` both mean `D-12`.
/// `wh open D-12`: the docket in Wheelhouse.app if it's installed, else the
/// web app on this Mac.
fn open(docket: Option<String>) -> Result<()> {
    let app = ["/Applications/Wheelhouse.app".into(), crate::paths::home().join("Applications/Wheelhouse.app")].into_iter().any(|p: std::path::PathBuf| p.exists());
    let url = match (&docket, app) {
        (Some(d), true) => format!("wheelhouse://docket/{d}"),
        (None, true) => "wheelhouse://inbox".to_string(),
        (Some(d), false) => format!("http://{}/#/d/{d}", server::http_addr().map(|a| a.to_string()).unwrap_or_else(|| "127.0.0.1:7500".into())),
        (None, false) => format!("http://{}/#/inbox", server::http_addr().map(|a| a.to_string()).unwrap_or_else(|| "127.0.0.1:7500".into())),
    };
    let ok = std::process::Command::new("/usr/bin/open").arg(&url).status().is_ok_and(|s| s.success());
    if !ok {
        anyhow::bail!("couldn't open {url}");
    }
    Ok(())
}

/// A docket as you typed it (`12`, `d-12`, `D-12`), as the hub names it.
/// It goes into a URL's path, so anything else is encoded: a name with `/`
/// or `..` in it is one segment the hub doesn't know, never another route.
fn normalize(d: &str) -> String {
    let n = d.trim().trim_start_matches(['D', 'd']).trim_start_matches('-');
    if n.chars().all(|c| c.is_ascii_digit()) && !n.is_empty() { format!("D-{n}") } else { enc(d.trim()) }
}

/// Percent-encode a value for a URL's path segment or query: everything
/// but RFC 3986's unreserved characters, and `:` (pane ids are `block:…`,
/// `herdr:…`).
fn enc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~' | b':') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Run one of the hub's loops, and start it again if it panics: a bug in
/// one loop is logged and survives, instead of silently stopping, say, the
/// Rex mirror while the rest of the hub carries on.
fn supervise<F, Fut>(name: &'static str, shared: hub::Shared, run: F)
where
    F: Fn(hub::Shared) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    tokio::spawn(async move {
        loop {
            match tokio::spawn(run(shared.clone())).await {
                Err(e) if e.is_panic() => {
                    say!("{name} stopped on a bug (a panic); it starts again in 5s");
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                }
                _ => return,
            }
        }
    });
}

/// Refuse to start beside a running hub: a second one would take over the
/// first one's socket (and every hook and `wh` command with it), run every
/// loop twice for a moment, and then fail on the port anyway.
fn alone() -> Result<()> {
    let sock = paths::socket();
    if std::os::unix::net::UnixStream::connect(&sock).is_ok() {
        anyhow::bail!("a hub is already running on {} (wh status). Stop it first: wh down, or quit its terminal", paths::tilde(&sock.to_string_lossy()));
    }
    if let Some(addr) = server::http_addr()
        && std::net::TcpListener::bind(addr).is_err()
    {
        anyhow::bail!("{addr} is in use: another hub, or another app. Stop it, or set WH_HTTP to another address");
    }
    Ok(())
}

fn serve() -> Result<()> {
    alone()?;
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let dir = paths::state_dir();
        std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
        // The record, messages and kept files are yours alone on this Mac.
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
        }
        let store = store::Store::open(&paths::db())?;
        {
            use std::os::unix::fs::PermissionsExt;
            for f in ["wheelhouse.db", "wheelhouse.db-wal", "wheelhouse.db-shm"] {
                let _ = std::fs::set_permissions(dir.join(f), std::fs::Permissions::from_mode(0o600));
            }
        }
        let keys = record::Keyring::open(&paths::keys())?;
        let owner = std::env::var("USER").unwrap_or_else(|_| "you".into());
        let mut core = hub::Core::new(store, keys, &owner)?;
        agents::load(&mut core)?;
        let tracked = core.agents.by_block.len();
        let shared = hub::Shared::new(core);
        say!("state in {} ({tracked} agent dockets open)", paths::tilde(&dir.to_string_lossy()));
        supervise("live events", shared.clone(), hub::run_live);
        supervise("the Rex mirror", shared.clone(), runtime::rex::follow);
        supervise("the Herdr mirror", shared.clone(), runtime::herdr::follow);
        supervise("agents", shared.clone(), agents::tick_loop);
        supervise("the tailnet", shared.clone(), tailnet::watch);
        supervise("Linear", shared.clone(), linear::follow);
        supervise("message delivery", shared.clone(), messages::deliver_loop);
        supervise("notifications", shared.clone(), notify::run);
        supervise("push", shared.clone(), push::run);
        supervise("previews", shared.clone(), preview::sweep);
        supervise("Review", shared.clone(), review::watch::run);
        supervise("Build", shared.clone(), build::watch::run);
        supervise("logs", shared.clone(), service::trim_logs);
        supervise("token snapshots", shared.clone(), recap::snapshot_loop);
        supervise("the workspace", shared.clone(), team::member::follow);
        server::serve(shared).await
    })
}

const SKILL: &str = include_str!("../skills/wheelhouse/SKILL.md");

fn build_cmd(action: &str, args: &[String], repo: Option<String>, as_json: bool) -> Result<()> {
    let v = match action {
        "status" => client::json("GET", "/api/build", None)?,
        "start" => {
            let from = args.first().ok_or_else(|| anyhow::anyhow!("start what? wh build start D-12 (or owner/name#45, or ENG-142)"))?;
            let v = client::json("POST", "/api/build/start", Some(&json!({ "from": from, "repo": repo })))?;
            if !as_json {
                println!(
                    "{}: Build of {} into {} is {}",
                    v["docket"].as_str().unwrap_or("?"),
                    v["key"].as_str().unwrap_or("?"),
                    v["repo"].as_str().unwrap_or("?"),
                    v["state"].as_str().unwrap_or("?")
                );
                return Ok(());
            }
            v
        }
        "discard" => client::json("POST", "/api/build/discard", Some(&json!({ "docket": args.first().ok_or_else(|| anyhow::anyhow!("discard which? wh build discard D-12"))? })))?,
        "pause" => client::json("POST", "/api/build/settings", Some(&json!({ "paused": true })))?,
        "resume" => client::json("POST", "/api/build/settings", Some(&json!({ "paused": false })))?,
        other => bail!("wh build takes start, discard, pause or resume, not {other}"),
    };
    if as_json || action != "status" {
        println!("{}", serde_json::to_string_pretty(&v)?);
        return Ok(());
    }
    if let Some(e) = v["config_error"].as_str() {
        println!("config   {e}");
    }
    println!("Build    {}{}", if v["paused"] == true { "paused" } else { "on" }, v["doing"].as_str().map(|d| format!(" · {d}")).unwrap_or_default());
    for t in v["tasks"].as_array().into_iter().flatten() {
        let mut bits: Vec<String> = Vec::new();
        if let Some(p) = t["phase"].as_str() {
            bits.push(p.replace('_', " "));
        }
        if let Some(n) = t["round"]["n"].as_i64() {
            bits.push(format!("round {n}: {}", t["round"]["summary"].as_str().unwrap_or("")));
        } else if t["rounds"].as_i64().unwrap_or(0) > 0 {
            bits.push(format!("{} round{} pushed", t["rounds"], if t["rounds"] == 1 { "" } else { "s" }));
        }
        if let Some(n) = t["prereview"]["n"].as_i64() {
            bits.push(format!("pre-review {n}: {} blocking", t["prereview"]["blocking"].as_i64().unwrap_or(0)));
        }
        if let Some(u) = t["pr_url"].as_str() {
            bits.push(u.to_string());
        }
        let asks: Vec<&str> = t["asks"].as_array().into_iter().flatten().filter_map(|a| a.as_str()).collect();
        println!(
            "  {:<6} {:<22} {:<8} {}{}{}",
            t["docket"].as_str().unwrap_or(""),
            t["key"].as_str().unwrap_or(""),
            t["state"].as_str().unwrap_or(""),
            bits.join(" · "),
            if asks.is_empty() { String::new() } else { format!("  ⚑ waiting on you: {}", asks.join(", ")) },
            t["attention"]["text"].as_str().map(|a| format!("  ⚑ {a}")).unwrap_or_default()
        );
    }
    Ok(())
}

/// `OWNER/NAME N` or `OWNER/NAME#N` → `OWNER/NAME#N`.
fn pr_key(args: &[String]) -> Result<String> {
    let joined = args.join(" ");
    let key = match args {
        [one] => one.clone(),
        [repo, n] => format!("{repo}#{}", n.trim_start_matches('#')),
        _ => joined,
    };
    review::pr::parse_key(&key).map(|(r, n)| format!("{r}#{n}")).ok_or_else(|| anyhow::anyhow!("give the PR as OWNER/NAME N, such as: acme/api 12"))
}

fn review_cmd(action: &str, args: &[String], force: bool, as_json: bool) -> Result<()> {
    let block = crate::runtime::pane_from_env();
    let settings = |body: Value| client::json("POST", "/api/review/settings", Some(&body));
    let out = match action {
        "status" => client::json("GET", "/api/review", None)?,
        "on" => settings(json!({ "enabled": true, "force": force }))?,
        "off" => settings(json!({ "enabled": false }))?,
        "pause" => settings(json!({ "paused": true }))?,
        "resume" => settings(json!({ "paused": false }))?,
        "post-mode" => settings(json!({ "post": args.first().map(String::as_str).unwrap_or("config") }))?,
        "check" => client::json("POST", "/api/review/check", Some(&json!({})))?,
        "pickup" => {
            let key = pr_key(args)?;
            let (repo, pr) = review::pr::parse_key(&key).expect("checked");
            client::json("POST", "/api/review/pickup", Some(&json!({ "repo": repo, "pr": pr })))?
        }
        "preview" | "post" | "dismiss" => {
            let key = pr_key(args)?;
            let v = client::json_within("POST", &format!("/api/review/{action}"), Some(&json!({ "key": key, "block": block })), std::time::Duration::from_secs(300))?;
            if !as_json {
                match action {
                    "preview" => {
                        if let Some(w) = v["warning"].as_str() {
                            eprintln!("{w}");
                        }
                        println!("{}", v["preview"].as_str().unwrap_or_default());
                    }
                    "post" => println!("posted {} ({} inline, {} replies): {}", v["state"].as_str().unwrap_or("?"), v["inline"], v["replies"], v["url"].as_str().unwrap_or("?")),
                    _ if v["dismissed"] == true => println!("dismissed round {}; nothing was posted", v["round"]),
                    _ => println!("this round was on GitHub already, so it's recorded as posted, not dismissed: {}", v["url"].as_str().unwrap_or("?")),
                }
                return Ok(());
            }
            v
        }
        other => bail!("wh review takes status, on, off, pause, resume, check, post-mode, pickup, preview, post or dismiss, not {other}"),
    };
    if as_json || !matches!(action, "status" | "on" | "off" | "pause" | "resume" | "post-mode") {
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }
    print_review_status(&out);
    Ok(())
}

fn print_review_status(v: &Value) {
    let on = v["enabled"].as_bool() == Some(true);
    let paused = v["paused"].as_bool() == Some(true);
    println!(
        "Review   {}{}",
        if on { "on" } else { "off (wh review on turns it on; docs/review.md)" },
        if on && paused { " · paused: no new rounds start (wh review resume)" } else { "" }
    );
    if let Some(e) = v["config_error"].as_str() {
        println!("config   {e}");
    } else if let Some(c) = v["config"].as_object() {
        let rs: Vec<String> = c["reviewers"].as_array().into_iter().flatten().map(|r| r["name"].as_str().unwrap_or("?").to_string()).collect();
        println!(
            "config   {} · {} · reviewers {} · checker {} · post {}",
            c["path"].as_str().unwrap_or("?"),
            c["repos"].as_array().map_or(0, Vec::len).to_string() + " repos",
            rs.join(", "),
            c["checker"].as_str().unwrap_or("?"),
            v["post"].as_str().unwrap_or("?")
        );
        let how = v["post_rule"]["text"].as_str().unwrap_or("?");
        match v["post_override"].as_str() {
            Some(o) => println!("posting  a checked review {how} (wh review post-mode {o}; config follows review.lua and Settings)"),
            None => {
                println!("posting  a checked review {how}");
                for (slug, p) in c["repo_posts"].as_object().into_iter().flatten() {
                    println!("         in {slug}, it {}", p["text"].as_str().unwrap_or("?"));
                }
            }
        }
    }
    if v["rex_reviewer_running"].as_bool() == Some(true) {
        println!("         rex-reviewer's watcher is running too (rex-reviewer down stops it)");
    }
    if let Some(e) = v["scan_error"].as_str() {
        println!("GitHub   {e}");
    } else if let Some(at) = v["last_scan_at"].as_str() {
        println!("GitHub   checked {at}");
    }
    if let Some(d) = v["doing"].as_str() {
        println!("now      {d}");
    }
    let prs: Vec<&Value> = v["prs"].as_array().into_iter().flatten().collect();
    let done = prs.iter().filter(|p| matches!(p["state"].as_str(), Some("merged" | "closed" | "cleaned" | "unrequested")) && p["attention"].is_null()).count();
    for p in prs.iter().filter(|p| !matches!(p["state"].as_str(), Some("merged" | "closed" | "cleaned" | "unrequested")) || !p["attention"].is_null()) {
        let phase = p["phase"].as_str().map(|x| format!("/{x}")).unwrap_or_default();
        let att = p["attention"]["text"].as_str().map(|t| format!("  ⚑ {t}")).unwrap_or_default();
        println!(
            "  {:<24} {:<10} r{:<2} {}{}{}",
            p["key"].as_str().unwrap_or("?"),
            format!("{}{phase}", p["state"].as_str().unwrap_or("?")),
            p["round"],
            p["docket"].as_str().unwrap_or(""),
            p["verdict"].as_str().map(|x| format!(" · {x}")).unwrap_or_default(),
            att
        );
    }
    if done > 0 {
        println!("  … and {done} done this week (wh review --json lists them)");
    }
}

/// `wh skill [show|install|remove]`: the agents' skill. With a shared
/// `~/.agents/skills`, it goes there and is linked into Claude Code's,
/// Codex's and Antigravity's skills; otherwise it goes in `~/.claude/skills`.
fn skill(action: Option<&str>) -> Result<()> {
    let home = paths::home();
    let shared = home.join(".agents/skills");
    let homes: Vec<std::path::PathBuf> = [".claude/skills", ".codex/skills", ".gemini/config/skills"].iter().map(|d| home.join(d)).filter(|d| d.is_dir()).collect();
    let source = if shared.is_dir() { shared.join("wheelhouse") } else { home.join(".claude/skills/wheelhouse") };
    // Pi reads ~/.agents/skills itself: a link in its own folder too would
    // be the skill twice. Without that folder, it gets a link.
    let pi = hooks::pi_agent_dir();
    let pi_link = pi.join("skills/wheelhouse");
    let pi_linked = || {
        pi_link.symlink_metadata().is_ok_and(|m| m.file_type().is_symlink())
            && std::fs::read_link(&pi_link).is_ok_and(|t| t == source || t == home.join(".claude/skills/wheelhouse"))
    };
    match action.unwrap_or("show") {
        "show" => print!("{SKILL}"),
        "install" => {
            std::fs::create_dir_all(&source)?;
            std::fs::write(source.join("SKILL.md"), SKILL)?;
            println!("wrote {}", paths::tilde(&source.join("SKILL.md").to_string_lossy()));
            if shared.is_dir() {
                for h in &homes {
                    let link = h.join("wheelhouse");
                    if link.symlink_metadata().is_err() {
                        std::os::unix::fs::symlink(&source, &link)?;
                        println!("linked {}", paths::tilde(&link.to_string_lossy()));
                    }
                }
                if pi_linked() {
                    std::fs::remove_file(&pi_link)?;
                    println!("unlinked {}: Pi reads {}", paths::tilde(&pi_link.to_string_lossy()), paths::tilde(&shared.to_string_lossy()));
                }
            } else if pi.is_dir() && pi_link.symlink_metadata().is_err() {
                std::fs::create_dir_all(pi.join("skills"))?;
                std::os::unix::fs::symlink(&source, &pi_link)?;
                println!("linked {}", paths::tilde(&pi_link.to_string_lossy()));
            }
        }
        "remove" => {
            if pi_linked() {
                std::fs::remove_file(&pi_link)?;
                println!("unlinked {}", paths::tilde(&pi_link.to_string_lossy()));
            }
            for h in &homes {
                let link = h.join("wheelhouse");
                if link.symlink_metadata().is_ok_and(|m| m.file_type().is_symlink()) {
                    std::fs::remove_file(&link)?;
                    println!("unlinked {}", paths::tilde(&link.to_string_lossy()));
                }
            }
            let _ = std::fs::remove_file(source.join("SKILL.md"));
            let _ = std::fs::remove_dir(&source);
            println!("removed {}", paths::tilde(&source.to_string_lossy()));
        }
        other => bail!("wh skill takes show, install or remove, not {other}"),
    }
    Ok(())
}

fn block_q(sep: &str) -> String {
    crate::runtime::pane_from_env().map(|b| format!("{sep}block={}", enc(&b))).unwrap_or_default()
}

fn text_or_stdin(t: Option<String>) -> Result<String> {
    match t.as_deref() {
        Some("-") | None if !std::io::IsTerminal::is_terminal(&std::io::stdin()) || t.is_some() => {
            let mut s = String::new();
            std::io::Read::read_to_string(&mut std::io::stdin(), &mut s)?;
            Ok(s)
        }
        Some(t) => Ok(t.to_string()),
        None => Ok(String::new()),
    }
}

fn msg_line(v: &Value) -> String {
    let opts: Vec<&str> = v["options"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
    format!(
        "#{:<4} from {}{}{}: {}{}",
        v["id"],
        v["from"].as_str().unwrap_or("?"),
        v["docket"].as_str().map(|d| format!(" ({d})")).unwrap_or_default(),
        v["subject"].as_str().map(|s| format!(" · {s}")).unwrap_or_default(),
        v["body"].as_str().unwrap_or_default().lines().next().unwrap_or_default(),
        if opts.is_empty() { String::new() } else { format!("  [{}]", opts.join(" · ")) }
    )
}

fn msg_send(to: &str, subject: Option<String>, options: Vec<String>, text: Option<String>) -> Result<()> {
    let body = text_or_stdin(text)?;
    let block = crate::runtime::pane_from_env();
    let v = client::json("POST", "/api/messages", Some(&json!({ "to": to, "subject": subject, "options": options, "body": body, "block": block })))?;
    let state = if v["status"] == "open" { "in your inbox" } else { "queued: it goes in when that agent is between turns" };
    println!("#{} to {}: {state}", v["id"], v["to"].as_str().unwrap_or(to));
    Ok(())
}

fn msg_reply(n: i64, choice: Option<String>, text: Option<String>) -> Result<()> {
    let body = text_or_stdin(text)?;
    let block = crate::runtime::pane_from_env();
    let v = client::json("POST", &format!("/api/messages/{n}/reply"), Some(&json!({ "body": body, "choice": choice, "block": block })))?;
    println!("#{} to {}{}", v["id"], v["to"].as_str().unwrap_or("?"), v["choice"].as_str().map(|c| format!(" (chose {c})")).unwrap_or_default());
    Ok(())
}

fn msg_inbox(as_json: bool) -> Result<()> {
    let v = client::json("GET", &format!("/api/messages{}", block_q("?")), None)?;
    if as_json {
        println!("{}", serde_json::to_string_pretty(&v)?);
        return Ok(());
    }
    let list = v.as_array().cloned().unwrap_or_default();
    if list.is_empty() {
        println!("No messages.");
    }
    for m in &list {
        println!("{}", msg_line(m));
    }
    Ok(())
}

fn print_msg(v: &Value, as_json: bool) -> Result<()> {
    if as_json {
        println!("{}", serde_json::to_string_pretty(v)?);
        return Ok(());
    }
    println!(
        "#{} from {} to {}{}",
        v["id"],
        v["from"].as_str().unwrap_or("?"),
        v["to"].as_str().unwrap_or("?"),
        v["docket"].as_str().map(|d| format!(" ({d})")).unwrap_or_default()
    );
    if let Some(s) = v["subject"].as_str() {
        println!("Subject: {s}");
    }
    if let Some(c) = v["choice"].as_str() {
        println!("Chose: {c}");
    }
    println!(
        "
{}",
        v["body"].as_str().unwrap_or_default()
    );
    let opts: Vec<&str> = v["options"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
    if !opts.is_empty() {
        println!(
            "
Options: {}",
            opts.join(" · ")
        );
    }
    Ok(())
}

fn msg_read(n: i64, as_json: bool) -> Result<()> {
    let v = client::json("GET", &format!("/api/messages/{n}{}", block_q("?")), None)?;
    print_msg(&v, as_json)
}

fn msg_wait(thread: Option<i64>, timeout: u64, as_json: bool) -> Result<()> {
    let mut q = format!("/api/messages/wait?timeout={timeout}");
    if let Some(t) = thread {
        q.push_str(&format!("&thread={t}"));
    }
    q.push_str(&block_q("&"));
    let (status, bytes) = client::call("GET", &q, None, std::time::Duration::from_secs(timeout + 15))?;
    let v: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    if status == 408 {
        bail!("nothing came within {timeout}s");
    }
    if !(200..300).contains(&status) {
        bail!("{}", v["error"].as_str().unwrap_or("wait failed"));
    }
    print_msg(&v, as_json)
}

fn new_docket(title: Option<String>, linear: Option<String>) -> Result<()> {
    let v = match (linear, title) {
        (Some(key), _) => client::json("POST", "/api/dockets/from-linear", Some(&json!({ "key": key })))?,
        (None, Some(title)) => client::json("POST", "/api/dockets", Some(&json!({ "title": title })))?,
        (None, None) => bail!("give a title, or --linear ENG-142"),
    };
    let id = v["docket"].as_str().unwrap_or_default();
    println!("{id}{}", if v["existing"].as_bool() == Some(true) { " (already open for that issue)" } else { "" });
    Ok(())
}

fn intent(d: &str, text: Option<String>) -> Result<()> {
    match text {
        None => {
            let v = client::json("GET", &format!("/api/dockets/{d}"), None)?;
            match v["docket"]["intent"]["body"].as_str() {
                Some(b) => println!("{b}"),
                None => println!("{d} has no intent yet. Set one: wh intent {d} \"...\" (or - to read it from stdin)"),
            }
            Ok(())
        }
        Some(t) => {
            let body = if t == "-" {
                let mut s = String::new();
                std::io::Read::read_to_string(&mut std::io::stdin(), &mut s)?;
                s
            } else {
                t
            };
            let block = crate::runtime::pane_from_env();
            let v = client::json("POST", &format!("/api/dockets/{d}/intent"), Some(&json!({ "body": body, "block": block })))?;
            if v["unchanged"].as_bool() == Some(true) {
                println!("{d}: the intent is already that")
            } else {
                println!("{}", placed(d, &v))
            }
            Ok(())
        }
    }
}

fn attach(what: &str, title: Option<String>, docket: Option<String>) -> Result<()> {
    use base64::Engine;
    let block = crate::runtime::pane_from_env();
    let d = match docket {
        Some(d) => normalize(&d),
        None => {
            let b = block.clone().ok_or_else(|| anyhow::anyhow!("name a docket (--docket D-12), or run this from an agent's pane"))?;
            client::json("GET", &format!("/api/whoami?block={}", enc(&b)), None)?["docket"]
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| anyhow::anyhow!("this pane has no docket; name one with --docket"))?
        }
    };
    let body = if what.starts_with("https://") {
        json!({ "url": what, "title": title, "block": block })
    } else {
        let path = std::path::Path::new(what);
        let bytes = std::fs::read(path).with_context(|| format!("reading {what}"))?;
        anyhow::ensure!(bytes.len() <= artifacts::MAX, "{what} is {} MB; artifacts are at most {} MB", bytes.len() >> 20, artifacts::MAX >> 20);
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "file".into());
        json!({ "name": name, "content": base64::engine::general_purpose::STANDARD.encode(&bytes), "title": title, "block": block })
    };
    let (status, out) = client::call("POST", &format!("/api/dockets/{d}/artifacts"), Some(&body), std::time::Duration::from_secs(120))?;
    let v: Value = serde_json::from_slice(&out).unwrap_or_default();
    anyhow::ensure!((200..300).contains(&status), "{}", v["error"].as_str().unwrap_or("the hub refused it"));
    if v["new"].as_bool() == Some(true) {
        println!("{d} #{}: kept", v["seq"]);
    } else {
        println!("{d}: already kept, as #{}", v["seq"]);
    }
    Ok(())
}

fn brief(d: Option<String>) -> Result<()> {
    let path = match d {
        Some(d) => format!("/api/dockets/{d}/brief{}", block_q("?")),
        None => {
            let block = crate::runtime::pane_from_env().ok_or_else(|| anyhow::anyhow!("name a docket (wh brief D-12), or run this from an agent's pane"))?;
            format!("/api/brief?block={}", enc(&block))
        }
    };
    print!("{}", client::text(&path)?);
    Ok(())
}

fn status() -> Result<()> {
    let v = client::json("GET", "/api/status", None)?;
    let up = v["uptime_secs"].as_u64().unwrap_or(0);
    println!("Wheelhouse {} running for {}m (pid {})", v["version"].as_str().unwrap_or("?"), up / 60, v["pid"]);
    for r in v["runtimes"].as_array().into_iter().flatten() {
        println!("  {:<8} {}", r["title"].as_str().unwrap_or("?"), runtime_line(r));
    }
    println!("  agents   {} · dockets {} open, {} in all", v["agents"], v["dockets"]["open"], v["dockets"]["all"]);
    let on: Vec<&str> =
        [("claude-code", "Claude Code"), ("agy", "Antigravity"), ("pi", "Pi")].into_iter().filter(|(app, _)| hooks::installed_for(app)).map(|(_, name)| name).collect();
    println!("  hooks    {}", if on.is_empty() { "off: wh hooks install".to_string() } else { format!("{} on", on.join(", ")) });
    match v["tailnet"]["url"].as_str() {
        Some(u) => println!("  tailnet  {u} (answers {}) · publish with wh web --publish", v["tailnet"]["owner"].as_str().unwrap_or("?")),
        None => println!("  tailnet  Tailscale not seen"),
    }
    println!("  state    {}", v["state_dir"].as_str().unwrap_or("?"));
    Ok(())
}

fn ls(all: bool) -> Result<()> {
    let v = client::json("GET", "/api/state", None)?;
    let asks = v["asks"].as_array().cloned().unwrap_or_default();
    if !asks.is_empty() {
        println!("Needs you");
        for a in &asks {
            println!(
                "  {:<6} {:<9} {}  {}",
                a["docket"].as_str().unwrap_or_default(),
                a["what"].as_str().unwrap_or_default(),
                a["title"].as_str().unwrap_or_default(),
                a["msg"].as_str().map(|m| format!("· {m}")).unwrap_or_default(),
            );
        }
        println!();
    }
    let dockets = v["dockets"].as_array().cloned().unwrap_or_default();
    let shown: Vec<&Value> = dockets.iter().filter(|d| all || d["status"] != "done" && d["status"] != "dropped").collect();
    println!("{}", if all { "Dockets" } else { "In flight" });
    for d in shown {
        let live = d["live"]["state"].as_str().map(|s| format!("● {s}")).unwrap_or_default();
        println!(
            "  {:<6} {:<11} {:<9} {:<50} {}",
            d["id"].as_str().unwrap_or_default(),
            d["status"].as_str().unwrap_or_default(),
            d["short_head"].as_str().unwrap_or_default(),
            truncate(d["title"].as_str().unwrap_or_default(), 50),
            live
        );
    }
    for r in v["runtimes"].as_array().into_iter().flatten() {
        if r["installed"] == true && r["connected"] != true {
            println!("\nNot following {}: {}", r["title"].as_str().unwrap_or("?"), r["error"].as_str().unwrap_or("connecting"));
        }
    }
    Ok(())
}

/// A runtime as `wh status` shows it: `following · 3 agents · new agents open here`.
fn runtime_line(r: &Value) -> String {
    let mut s = if r["connected"] == true {
        format!("following · {} agent{}", r["agents"], if r["agents"] == 1 { "" } else { "s" })
    } else if r["installed"] != true {
        "not installed".to_string()
    } else {
        format!("not connected: {}", r["error"].as_str().unwrap_or("connecting"))
    };
    if r["spawn"] == true {
        s.push_str(" · new agents open here");
    }
    s
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n { s.to_string() } else { format!("{}…", s.chars().take(n - 1).collect::<String>()) }
}

fn show(d: &str) -> Result<()> {
    let v = client::json("GET", &format!("/api/dockets/{d}"), None)?;
    let dk = &v["docket"];
    println!("{d}  {}", dk["title"].as_str().unwrap_or_default());
    println!(
        "    {} · {} · head {} · {} entries",
        dk["status"].as_str().unwrap_or_default(),
        dk["protocol"].as_str().unwrap_or_default(),
        dk["short_head"].as_str().unwrap_or_default(),
        dk["entries"]
    );
    if let Some(a) = dk["agent"].as_object() {
        println!(
            "    {} in {} ({} / {})",
            mirror::app_name(a["app"].as_str().unwrap_or_default()),
            a["cwd"].as_str().unwrap_or("?"),
            a["rex_session"].as_str().unwrap_or_default(),
            a["pane"].as_str().unwrap_or_default()
        );
    }
    println!();
    for e in v["entries"].as_array().into_iter().flatten() {
        let at = e["at"].as_str().unwrap_or_default();
        let hm = at.get(11..16).unwrap_or(at);
        let body = e["body"].as_str().or_else(|| e["data"]["summary"].as_str()).unwrap_or_default();
        let detail = match e["kind"].as_str() {
            Some("set") => e["data"].to_string(),
            Some("claim") => format!("session {} ({})", e["data"]["session"].as_str().unwrap_or("?"), e["data"]["source"].as_str().unwrap_or("?")),
            Some("release") => e["data"]["reason"].as_str().unwrap_or_default().to_string(),
            Some("observed") => match e["data"]["what"].as_str() {
                Some("linear") => {
                    let changed: Vec<&str> = e["data"]["changed"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
                    let i = &e["data"]["issue"];
                    if changed.is_empty() {
                        format!("Linear {}: {}", i["key"].as_str().unwrap_or("?"), i["state"].as_str().unwrap_or("?"))
                    } else {
                        format!("Linear {}: {} changed (now {})", i["key"].as_str().unwrap_or("?"), changed.join(", "), i["state"].as_str().unwrap_or("?"))
                    }
                }
                Some("linear-comment") => format!("{} in Linear: {body}", e["data"]["author"].as_str().unwrap_or("someone")),
                w => format!("{}: {body}", w.unwrap_or_default()),
            },
            Some("intent") => format!("({}) {}", e["data"]["source"].as_str().unwrap_or("?"), body.lines().next().unwrap_or_default()),
            Some("approval") => format!("{} {}", e["data"]["decision"].as_str().unwrap_or("?"), body),
            _ => body.lines().next().unwrap_or_default().to_string(),
        };
        println!(
            "  #{:<3} {hm}  {:<8} {:<10} {:<6} {}",
            e["seq"],
            e["kind"].as_str().unwrap_or_default(),
            e["mark"].as_str().unwrap_or_default(),
            e["key"].as_str().unwrap_or("?").trim_start_matches("agent-"),
            truncate(&detail, 70)
        );
    }
    let r = &v["verify"];
    println!();
    if r["ok"].as_bool() == Some(true) {
        println!("  ✓ chain intact, {} signatures good", r["entries"]);
    } else {
        println!("  ✕ {}", r["problems"].as_array().map(|p| p.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("; ")).unwrap_or_default());
    }
    Ok(())
}

fn verify(docket: Option<String>, file: Option<PathBuf>) -> Result<()> {
    if let Some(file) = file {
        return verify_file(&file);
    }
    let reports: Vec<Value> = match docket {
        Some(d) => vec![client::json("GET", &format!("/api/dockets/{d}/verify"), None)?],
        None => client::json("GET", "/api/verify", None)?["dockets"].as_array().cloned().unwrap_or_default(),
    };
    let mut bad = 0;
    for r in &reports {
        if r["ok"].as_bool() == Some(true) {
            println!(
                "✓ {:<6} {} entries, head {}",
                r["docket"].as_str().unwrap_or_default(),
                r["entries"],
                &r["head"].as_str().unwrap_or("")[..8.min(r["head"].as_str().unwrap_or("").len())]
            );
        } else {
            bad += 1;
            println!("✕ {:<6} {}", r["docket"].as_str().unwrap_or_default(), r["problems"]);
        }
    }
    if bad > 0 {
        bail!("{bad} of {} dockets don't check", reports.len());
    }
    Ok(())
}

/// Check an export with nothing but the keys it names.
fn verify_file(file: &std::path::Path) -> Result<()> {
    let text = std::fs::read_to_string(file).with_context(|| format!("reading {}", file.display()))?;
    let mut lines = text.lines();
    let header: Value = serde_json::from_str(lines.next().unwrap_or("")).context("the first line isn't the export's header")?;
    if header["wheelhouse"] != "record/v1" {
        bail!("not a Wheelhouse record/v1 export");
    }
    let docket = header["docket"].as_str().unwrap_or_default().to_string();
    let keys: std::collections::BTreeSet<String> = header["keys"].as_array().into_iter().flatten().filter_map(|k| k["keyid"].as_str().map(str::to_string)).collect();
    let mut entries = Vec::new();
    for l in lines.filter(|l| !l.trim().is_empty()) {
        let v: Value = serde_json::from_str(l)?;
        entries.push(record::Stored {
            docket: docket.clone(),
            seq: v["seq"].as_i64().unwrap_or(0),
            id: v["id"].as_str().unwrap_or_default().into(),
            chain: v["chain"].as_str().unwrap_or_default().into(),
            envelope: v["envelope"].as_str().unwrap_or_default().into(),
        });
    }
    let r = record::verify::verify(&docket, &entries, |k| keys.contains(k));
    if r.ok {
        println!("✓ {docket} {} entries, head {}", r.entries, r.head.as_deref().map(|h| &h[..8]).unwrap_or(""));
        Ok(())
    } else {
        for p in &r.problems {
            println!("✕ {p}");
        }
        bail!("{docket} doesn't check");
    }
}

#[cfg(test)]
mod cli_tests {
    use super::*;

    #[test]
    fn a_docket_or_a_pane_never_leaves_its_place_in_a_url() {
        assert_eq!(normalize("12"), "D-12");
        assert_eq!(normalize(" d-7 "), "D-7");
        assert_eq!(normalize("D-41"), "D-41");
        // Not a docket id: one encoded segment, never another route.
        assert_eq!(normalize("../hub/stop"), "..%2Fhub%2Fstop");
        assert_eq!(normalize("D-1?x=1#y"), "D-1%3Fx%3D1%23y");
        assert_eq!(enc("block:01a2-7806"), "block:01a2-7806");
        assert_eq!(enc("herdr:t2h"), "herdr:t2h");
        assert_eq!(enc("b&docket=D-1"), "b%26docket%3DD-1");
        assert_eq!(enc("é"), "%C3%A9");
    }
}
