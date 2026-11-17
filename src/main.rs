//! Wheelhouse: every piece of agent work as a docket, on a record you can
//! check. `wh serve` runs the hub; everything else talks to it.

macro_rules! say {
    ($($t:tt)*) => { eprintln!("{} {}", crate::time::now(), format!($($t)*)) };
}

mod agents;
mod approve;
mod client;
mod follow;
mod hooks;
mod hub;
mod mirror;
mod paths;
mod record;
mod rex;
mod server;
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
    /// Open a docket by hand.
    New { title: String },
    /// Add a note to a docket. From an agent's pane, the note is the agent's.
    Note { docket: String, text: String },
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
        Some(Cmd::Ls { all }) => ls(all),
        None => ls(false),
        Some(Cmd::Show { docket }) => show(&normalize(&docket)),
        Some(Cmd::New { title }) => client::json("POST", "/api/dockets", Some(&json!({ "title": title }))).map(|v| {
            println!("{}", v["docket"].as_str().unwrap_or_default());
        }),
        Some(Cmd::Note { docket, text }) => {
            let block = std::env::var("REX_BLOCK").ok();
            let d = normalize(&docket);
            client::json("POST", &format!("/api/dockets/{d}/note"), Some(&json!({ "body": text, "block": block }))).map(|v| {
                println!("{d} #{}", v["seq"]);
            })
        }
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
        server::serve(shared).await
    })
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
            Some("observed") => format!("{}: {body}", e["data"]["what"].as_str().unwrap_or_default()),
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
        Some(d) => vec![client::json("GET", &format!("/api/dockets/{d}"), None)?["verify"].clone()],
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
