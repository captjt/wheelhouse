//! Wheelhouse: every piece of agent work as a docket, on a record you can
//! check. `wh serve` runs the hub; everything else talks to it.

macro_rules! say {
    ($($t:tt)*) => { eprintln!("{} {}", crate::time::now(), format!($($t)*)) };
}

mod agents;
mod artifacts;
mod build;
mod brief;
mod linear;
mod messages;
mod notify;
mod approve;
mod client;
mod follow;
mod hooks;
mod hub;
mod mirror;
mod paths;
mod preview;
mod recap;
mod review;
mod push;
mod record;
mod rex;
mod server;
mod service;
mod start;
mod tailnet;
mod store;
mod time;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
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
    Serve,
    /// Open dockets, newest first (all of them with --all).
    Ls {
        #[arg(long)]
        all: bool,
    },
    /// One docket's record.
    Show { docket: String },
    /// Open a docket by hand, or from a Linear issue (--linear ENG-142).
    New {
        title: Option<String>,
        #[arg(long)]
        linear: Option<String>,
    },
    /// Link a Linear issue to a docket.
    Link { docket: String, issue: String },
    /// A docket's intent: print it, or set it (TEXT, or - for stdin).
    Intent { docket: String, text: Option<String> },
    /// What an agent reads to work on a docket (its own, from its pane).
    Brief { docket: Option<String> },
    /// Message an agent's pane, or you (`--to me`), with options as buttons.
    Send {
        #[arg(long)]
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
        n: i64,
        #[arg(long)]
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
    /// Start Claude Code (or Codex) on a new docket, in a folder, with a prompt.
    Start {
        #[arg(long)]
        codex: bool,
        #[arg(short = 'C', long, default_value = ".")]
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
        action: Option<String>,
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
        from: String,
        #[arg(long)]
        force: bool,
    },
    /// Build: `start D-12|owner/name#45|ENG-142 [--repo owner/name]`,
    /// `discard D-12`, `pause`, `resume`, or the list.
    Build {
        action: Option<String>,
        args: Vec<String>,
        #[arg(long)]
        repo: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// What happened: today, this week (the default), or 30 days; as Markdown.
    Recap { period: Option<String> },
    /// The agents' skill for Wheelhouse: show, install, or remove.
    Skill { action: Option<String> },
    /// Store a Linear API key in the Keychain.
    SetupLinear,
    /// Add a note to a docket. From an agent's pane, the note is the agent's.
    Note { docket: String, text: String },
    /// Keep a file (a report, a page, an image) or a link (a Claude artifact)
    /// with a docket: the record pins a file by its hash. From an agent's
    /// pane, its own docket.
    Attach {
        /// A file, or an https:// link.
        what: String,
        /// What it is, for people. Defaults to the file's name.
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        docket: Option<String>,
    },
    /// Rename a docket, or set its status (todo, in_progress, done, dropped).
    Set {
        docket: String,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        status: Option<String>,
    },
    /// Check a docket's record (or every docket's), or an exported file.
    Verify {
        docket: Option<String>,
        #[arg(long)]
        file: Option<PathBuf>,
    },
    /// A docket's record as JSON lines, checkable without the hub.
    Export { docket: String },
    /// The agents' hooks: status, install, or remove.
    Hooks { action: Option<String> },
    /// Run the hub in the background, at login (a LaunchAgent).
    Up,
    /// Stop the background hub.
    Down,
    /// The background hub's log.
    Logs {
        #[arg(short = 'n', default_value_t = 60)]
        lines: usize,
    },
    /// Whether the hub runs, and what it sees.
    Status,
    /// The app's address on your tailnet; --publish serves it there.
    Web {
        #[arg(long)]
        publish: bool,
    },
    /// Run by an agent's hooks.
    #[command(hide = true)]
    Hook {
        #[arg(long, default_value = "claude-code")]
        agent: String,
        event: String,
    },
}

fn main() {
    let cli = Cli::parse();
    let result = match cli.cmd {
        Some(Cmd::Serve) => serve(),
        Some(Cmd::Hook { agent, event }) => hooks::run(&agent, &event),
        Some(Cmd::Hooks { action }) => hooks::cli(action.as_deref()),
        Some(Cmd::Up) => service::up(),
        Some(Cmd::Down) => service::down(),
        Some(Cmd::Logs { lines }) => service::logs(lines),
        Some(Cmd::Status) => status(),
        Some(Cmd::Web { publish }) => tailnet::cli(publish),
        Some(Cmd::Ls { all }) => ls(all),
        None => ls(false),
        Some(Cmd::Show { docket }) => show(&normalize(&docket)),
        Some(Cmd::New { title, linear }) => new_docket(title, linear),
        Some(Cmd::Link { docket, issue }) => {
            let d = normalize(&docket);
            client::json("POST", &format!("/api/dockets/{d}/linear"), Some(&json!({ "key": issue }))).map(|_| println!("{d} linked to {issue}"))
        }
        Some(Cmd::Intent { docket, text }) => intent(&normalize(&docket), text),
        Some(Cmd::Brief { docket }) => brief(docket.as_deref().map(normalize)),
        Some(Cmd::SetupLinear) => linear::setup(),
        Some(Cmd::Start { codex, dir, title, prompt }) => {
            let cwd = std::fs::canonicalize(&dir).map(|p| p.to_string_lossy().into_owned()).unwrap_or(dir);
            let prompt = match prompt.as_deref() {
                Some("-") => text_or_stdin(prompt.clone()).ok(),
                _ => prompt,
            };
            client::json("POST", "/api/agents/start", Some(&json!({ "app": if codex { "codex" } else { "claude-code" }, "cwd": cwd, "prompt": prompt, "title": title })))
                .map(|v| println!("{} started in {}: wheelhouse/{}", if codex { "Codex" } else { "Claude Code" }, v["docket"].as_str().unwrap_or("?"), v["docket"].as_str().unwrap_or("?")))
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
            client::json("GET", &format!("/api/recap?period={p}"), None).map(|v| print!("{}", v["markdown"].as_str().unwrap_or("")))
        }
        Some(Cmd::Review { action, args, force, json }) => review_cmd(action.as_deref().unwrap_or("status"), &args, force, json),
        Some(Cmd::Share { port, title, ask, options, stop }) => {
            let block = std::env::var("REX_BLOCK").ok();
            if stop {
                client::json("POST", "/api/share/stop", Some(&json!({ "port": port, "block": block }))).map(|v| println!("stopped {} preview(s)", v["stopped"]))
            } else {
                let port = port.ok_or_else(|| anyhow::anyhow!("which port? wh share 5173"));
                port.and_then(|p| client::json("POST", "/api/share", Some(&json!({ "port": p, "title": title, "ask": ask, "options": options, "block": block }))))
                    .map(|v| {
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
            let block = std::env::var("REX_BLOCK").map_err(|_| anyhow::anyhow!("run this in a Rex pane"));
            block.and_then(|b| client::json("POST", "/api/panes/name", Some(&json!({ "block": b, "name": name })))).map(|v| {
                println!("this pane is {}", v["addr"].as_str().unwrap_or("?"));
            })
        }
        Some(Cmd::Whoami) => client::json("GET", &format!("/api/whoami{}", block_q("?")), None).map(|v| {
            println!("{}{}", v["addr"].as_str().unwrap_or("?"), v["docket"].as_str().map(|d| format!(" on {d}")).unwrap_or_default());
        }),
        Some(Cmd::Note { docket, text }) => {
            let block = std::env::var("REX_BLOCK").ok();
            let d = normalize(&docket);
            client::json("POST", &format!("/api/dockets/{d}/note"), Some(&json!({ "body": text, "block": block }))).map(|v| {
                println!("{d} #{}", v["seq"]);
            })
        }
        Some(Cmd::Attach { what, title, docket }) => attach(&what, title, docket),
        Some(Cmd::Set { docket, title, status }) => {
            let d = normalize(&docket);
            client::json("POST", &format!("/api/dockets/{d}/set"), Some(&json!({ "title": title, "status": status }))).map(|v| {
                println!("{d} #{}", v["seq"]);
            })
        }
        Some(Cmd::Verify { docket, file }) => verify(docket.as_deref().map(normalize), file),
        Some(Cmd::Export { docket }) => client::text(&format!("/api/dockets/{}/export", normalize(&docket))).map(|t| print!("{t}")),
    };
    if let Err(e) = result {
        eprintln!("wh: {e:#}");
        std::process::exit(1);
    }
}

/// `12` and `d-12` both mean `D-12`.
fn normalize(d: &str) -> String {
    let n = d.trim().trim_start_matches(['D', 'd']).trim_start_matches('-');
    if n.chars().all(|c| c.is_ascii_digit()) && !n.is_empty() { format!("D-{n}") } else { d.to_string() }
}

fn serve() -> Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let dir = paths::state_dir();
        std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
        let store = store::Store::open(&paths::db())?;
        let keys = record::Keyring::open(&paths::keys())?;
        let owner = std::env::var("USER").unwrap_or_else(|_| "you".into());
        let mut core = hub::Core::new(store, keys, &owner)?;
        agents::load(&mut core)?;
        let tracked = core.agents.by_block.len();
        let shared = hub::Shared::new(core);
        say!("state in {} ({tracked} agent dockets open)", paths::tilde(&dir.to_string_lossy()));
        tokio::spawn(follow::run(shared.clone()));
        tokio::spawn(tailnet::watch(shared.clone()));
        tokio::spawn(linear::follow(shared.clone()));
        tokio::spawn(messages::deliver_loop(shared.clone()));
        tokio::spawn(notify::run(shared.clone()));
        tokio::spawn(push::run(shared.clone()));
        tokio::spawn(preview::sweep(shared.clone()));
        tokio::spawn(review::watch::run(shared.clone()));
        tokio::spawn(build::watch::run(shared.clone()));
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
                println!("{}: Build of {} into {} is {}", v["docket"].as_str().unwrap_or("?"), v["key"].as_str().unwrap_or("?"), v["repo"].as_str().unwrap_or("?"), v["state"].as_str().unwrap_or("?"));
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
        println!(
            "  {:<6} {:<22} {:<10} {}{}{}",
            t["docket"].as_str().unwrap_or(""),
            t["key"].as_str().unwrap_or(""),
            t["state"].as_str().unwrap_or(""),
            t["phase"].as_str().unwrap_or(""),
            t["pr_url"].as_str().map(|u| format!(" · {u}")).unwrap_or_default(),
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
    review::pr::parse_key(&key).map(|(r, n)| format!("{r}#{n}")).ok_or_else(|| anyhow::anyhow!("give the PR as OWNER/NAME N, such as: acme/api 632"))
}

fn review_cmd(action: &str, args: &[String], force: bool, as_json: bool) -> Result<()> {
    let block = std::env::var("REX_BLOCK").ok();
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
            let v = client::json("POST", &format!("/api/review/{action}"), Some(&json!({ "key": key, "block": block })))?;
            if !as_json {
                match action {
                    "preview" => {
                        if let Some(w) = v["warning"].as_str() {
                            eprintln!("{w}");
                        }
                        println!("{}", v["preview"].as_str().unwrap_or_default());
                    }
                    "post" => println!("posted {} ({} inline, {} replies): {}", v["state"].as_str().unwrap_or("?"), v["inline"], v["replies"], v["url"].as_str().unwrap_or("?")),
                    _ => println!("dismissed round {}; nothing was posted", v["round"]),
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
        println!("config   {} · {} · reviewers {} · checker {} · post {}", c["path"].as_str().unwrap_or("?"), c["repos"].as_array().map_or(0, Vec::len).to_string() + " repos", rs.join(", "), c["checker"].as_str().unwrap_or("?"), v["post"].as_str().unwrap_or("?"));
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
/// `~/.agents/skills`, it goes there and is linked into Claude Code's and
/// Codex's skills; otherwise it goes in `~/.claude/skills`.
fn skill(action: Option<&str>) -> Result<()> {
    let home = paths::home();
    let shared = home.join(".agents/skills");
    let homes: Vec<std::path::PathBuf> = [".claude/skills", ".codex/skills"].iter().map(|d| home.join(d)).filter(|d| d.is_dir()).collect();
    let source = if shared.is_dir() { shared.join("wheelhouse") } else { home.join(".claude/skills/wheelhouse") };
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
            }
        }
        "remove" => {
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
    std::env::var("REX_BLOCK").map(|b| format!("{sep}block={b}")).unwrap_or_default()
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
    let block = std::env::var("REX_BLOCK").ok();
    let v = client::json("POST", "/api/messages", Some(&json!({ "to": to, "subject": subject, "options": options, "body": body, "block": block })))?;
    let state = if v["status"] == "open" { "in your inbox" } else { "queued: it goes in when that agent is between turns" };
    println!("#{} to {}: {state}", v["id"], v["to"].as_str().unwrap_or(to));
    Ok(())
}

fn msg_reply(n: i64, choice: Option<String>, text: Option<String>) -> Result<()> {
    let body = text_or_stdin(text)?;
    let block = std::env::var("REX_BLOCK").ok();
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
    println!("#{} from {} to {}{}", v["id"], v["from"].as_str().unwrap_or("?"), v["to"].as_str().unwrap_or("?"), v["docket"].as_str().map(|d| format!(" ({d})")).unwrap_or_default());
    if let Some(s) = v["subject"].as_str() {
        println!("Subject: {s}");
    }
    if let Some(c) = v["choice"].as_str() {
        println!("Chose: {c}");
    }
    println!("
{}", v["body"].as_str().unwrap_or_default());
    let opts: Vec<&str> = v["options"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
    if !opts.is_empty() {
        println!("
Options: {}", opts.join(" · "));
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
            let block = std::env::var("REX_BLOCK").ok();
            let v = client::json("POST", &format!("/api/dockets/{d}/intent"), Some(&json!({ "body": body, "block": block })))?;
            if v["unchanged"].as_bool() == Some(true) { println!("{d}: the intent is already that") } else { println!("{d} #{}", v["seq"]) }
            Ok(())
        }
    }
}

fn attach(what: &str, title: Option<String>, docket: Option<String>) -> Result<()> {
    use base64::Engine;
    let block = std::env::var("REX_BLOCK").ok();
    let d = match docket {
        Some(d) => normalize(&d),
        None => {
            let b = block.clone().ok_or_else(|| anyhow::anyhow!("name a docket (--docket D-12), or run this from an agent's pane"))?;
            client::json("GET", &format!("/api/whoami?block={b}"), None)?["docket"]
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
        Some(d) => format!("/api/dockets/{d}/brief"),
        None => {
            let block = std::env::var("REX_BLOCK").map_err(|_| anyhow::anyhow!("name a docket (wh brief D-12), or run this from an agent's pane"))?;
            format!("/api/brief?block={block}")
        }
    };
    print!("{}", client::text(&path)?);
    Ok(())
}

fn status() -> Result<()> {
    let v = client::json("GET", "/api/status", None)?;
    let up = v["uptime_secs"].as_u64().unwrap_or(0);
    println!("Wheelhouse {} running for {}m (pid {})", v["version"].as_str().unwrap_or("?"), up / 60, v["pid"]);
    println!(
        "  Rex      {}",
        if v["rex"]["connected"].as_bool() == Some(true) { "following".to_string() } else { format!("not connected: {}", v["rex"]["error"].as_str().unwrap_or("connecting")) }
    );
    println!("  agents   {} in Rex · dockets {} open, {} in all", v["agents"], v["dockets"]["open"], v["dockets"]["all"]);
    println!("  hooks    {}", if v["hooks"].as_bool() == Some(true) { "Claude Code: on" } else { "off: wh hooks install" });
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
    if !v["rex"]["connected"].as_bool().unwrap_or(false) {
        println!("\nNot following Rex: {}", v["rex"]["error"].as_str().unwrap_or("connecting"));
    }
    Ok(())
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
            println!("✓ {:<6} {} entries, head {}", r["docket"].as_str().unwrap_or_default(), r["entries"], &r["head"].as_str().unwrap_or("")[..8.min(r["head"].as_str().unwrap_or("").len())]);
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
    let keys: std::collections::BTreeSet<String> =
        header["keys"].as_array().into_iter().flatten().filter_map(|k| k["keyid"].as_str().map(str::to_string)).collect();
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
