//! The review agents' command lines (Claude Code, Codex and Antigravity), and what Wheelhouse
//! reads back from their own logs: which model ran, at what effort, and how
//! many tokens it spent. Ported from rex-reviewer's `agents.lua`,
//! `bin/token-usage` and its byline.

use serde_json::{Value, json};
use std::io::{BufRead, Read};
use std::path::{Path, PathBuf};

use super::config::Config;

/// A random v4 UUID, lowercase.
pub fn uuid_v4() -> String {
    let mut b = [0u8; 16];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        let _ = f.read_exact(&mut b);
    }
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

/// `args` with any effort setting replaced by `level` (none: unchanged).
pub fn with_effort(agent: &str, args: &[String], level: Option<&str>) -> Vec<String> {
    let Some(level) = level else { return args.to_vec() };
    let mut out = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        let next = args.get(i + 1).map(String::as_str).unwrap_or("");
        let skip = match agent {
            "claude" | "agy" if a == "--effort" => 2,
            "claude" | "agy" if a.starts_with("--effort=") => 1,
            "codex" if (a == "-c" || a == "--config") && next.starts_with("model_reasoning_effort=") => 2,
            "codex" if a.starts_with("--config=model_reasoning_effort=") => 1,
            _ => 0,
        };
        if skip == 0 {
            out.push(a.clone());
            i += 1;
        } else {
            i += skip;
        }
    }
    match agent {
        "claude" | "agy" => out.extend(["--effort".to_string(), level.to_string()]),
        _ => out.extend(["-c".to_string(), format!("model_reasoning_effort=\"{level}\"")]),
    }
    out
}

const CODEX_DEFAULTS: [&str; 6] = ["--approve-for-me", "--no-alt-screen", "--disable", "apps", "-c", "plugins.\"github@openai-curated\".enabled=false"];

pub fn program(cfg: &Config, agent: &str) -> String {
    cfg.agent(agent).program.map(|p| crate::paths::expand(&p).to_string_lossy().into_owned()).unwrap_or_else(|| agent.to_string())
}

fn base_args(cfg: &Config, agent: &str, extra: &[String]) -> Vec<String> {
    let a = cfg.agent(agent);
    let mut args: Vec<String> = match (a.args, agent) {
        (Some(x), _) => x,
        (None, "claude") => vec!["--permission-mode".into(), "auto".into()],
        (None, "agy") => vec!["--mode".into(), "auto".into()],
        (None, _) => CODEX_DEFAULTS.iter().map(|s| s.to_string()).collect(),
    };
    args.extend(a.extra_args);
    args.extend(extra.iter().cloned());
    args
}

static CODEX_MCP: std::sync::Mutex<Option<(std::time::Instant, String, Vec<String>)>> = std::sync::Mutex::new(None);

/// Codex's MCP servers, each switched off (`-c mcp_servers."NAME".enabled=
/// false`): Codex merges `-c mcp_servers={}` with your config and plugins,
/// so an empty list switches nothing off, and your servers (a tracker, a
/// cluster) would route around the wall. The names come from `codex mcp
/// list --json`, or, when that can't run, the `[mcp_servers.NAME]` tables
/// of `~/.codex/config.toml`. Kept ten minutes.
pub fn codex_mcp_off(program: &str) -> Vec<String> {
    if let Ok(g) = CODEX_MCP.lock()
        && let Some((at, p, names)) = g.as_ref()
        && p == program
        && at.elapsed() < std::time::Duration::from_secs(600)
    {
        return off_args(names);
    }
    // Tests never run your Codex or read your config.
    let names = if cfg!(test) { Vec::new() } else { codex_mcp_listed(program).unwrap_or_else(codex_mcp_configured) };
    if let Ok(mut g) = CODEX_MCP.lock() {
        *g = Some((std::time::Instant::now(), program.to_string(), names.clone()));
    }
    off_args(&names)
}

/// Codex splits a `-c` key on `.` and takes no quotes (a quoted name is a
/// new server with no command, and Codex won't start), and its server names
/// are letters, digits, `_` and `-`: only those are named.
pub(crate) fn off_args(names: &[String]) -> Vec<String> {
    names
        .iter()
        .filter(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'))
        .flat_map(|n| ["-c".to_string(), format!("mcp_servers.{n}.enabled=false")])
        .collect()
}

fn codex_mcp_listed(program: &str) -> Option<Vec<String>> {
    let mut cmd = std::process::Command::new(program);
    cmd.args(["mcp", "list", "--json"]).env("PATH", agent_path(&[])).stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(cmd.output());
    });
    let out = rx.recv_timeout(std::time::Duration::from_secs(10)).ok()?.ok()?;
    let v: Value = serde_json::from_slice(&out.stdout).ok()?;
    Some(v.as_array()?.iter().filter_map(|s| s["name"].as_str().map(str::to_string)).collect())
}

fn codex_mcp_configured() -> Vec<String> {
    let home = std::env::var_os("CODEX_HOME").map(PathBuf::from).unwrap_or_else(|| crate::paths::home().join(".codex"));
    let text = std::fs::read_to_string(home.join("config.toml")).unwrap_or_default();
    let mut names: Vec<String> =
        text.lines().filter_map(|l| l.trim().strip_prefix("[mcp_servers.")?.strip_suffix(']').map(|n| n.split('.').next().unwrap_or(n).trim_matches('"').to_string())).collect();
    names.dedup();
    names
}

static SHELL_PATH: std::sync::Mutex<Option<(std::time::Instant, String)>> = std::sync::Mutex::new(None);

/// Your interactive shell's PATH (what a terminal of yours has: nvm, Nix,
/// pyenv and the rest that `.zshrc` adds), read with `$SHELL -ilc` and kept
/// for ten minutes. A pane only gets the login shell's, which lacks them.
pub fn shell_path() -> Option<String> {
    if let Some((at, p)) = SHELL_PATH.lock().ok()?.as_ref()
        && at.elapsed() < std::time::Duration::from_secs(600)
    {
        return Some(p.clone());
    }
    let shell = std::env::var("SHELL").ok().filter(|s| s.starts_with('/')).unwrap_or_else(|| "/bin/zsh".into());
    let home = crate::paths::home();
    let mut child = std::process::Command::new(&shell)
        .args(["-ilc", "printf 'WH_PATH=%s\\n' \"$PATH\""])
        .env_clear()
        .env("HOME", &home)
        .env("USER", std::env::var("USER").unwrap_or_default())
        .env("TERM", "dumb")
        .current_dir(&home)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    let started = std::time::Instant::now();
    while child.try_wait().ok()?.is_none() {
        if started.elapsed() > std::time::Duration::from_secs(15) {
            let _ = child.kill();
            return None;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let out = child.wait_with_output().ok()?;
    let p = String::from_utf8_lossy(&out.stdout).lines().filter_map(|l| l.strip_prefix("WH_PATH=")).next_back()?.trim().to_string();
    if p.is_empty() {
        return None;
    }
    if let Ok(mut c) = SHELL_PATH.lock() {
        *c = Some((std::time::Instant::now(), p.clone()));
    }
    Some(p)
}

/// The agents' PATH: the config's folders first, then your shell's (or the
/// usual places, if it can't be read), each once.
pub fn agent_path(first: &[String]) -> String {
    let rest = shell_path().unwrap_or_else(super::github::command_path);
    let mut out: Vec<String> = Vec::new();
    for d in first.iter().cloned().chain(rest.split(':').map(str::to_string)) {
        if !d.is_empty() && !out.contains(&d) {
            out.push(d);
        }
    }
    out.join(":")
}

/// The pane's program: `env` with gh signed out, your shell's PATH (the
/// config's `path` first), and Cargo's caches shared; then the agent.
/// Where every push from an agent's pane goes: nowhere.
pub const NEVER: &str = "wheelhouse-agents-never-push:///";

/// A git ignore file with yours and `.tmp/`, for the agents' git.
fn agent_gitignore() -> std::path::PathBuf {
    let p = super::cache_dir().join("agent-gitignore");
    let mine = ["~/.config/git/ignore", "~/.gitignore_global", "~/.gitignore"]
        .iter()
        .map(|f| crate::paths::expand(f))
        .find(|f| f.exists())
        .and_then(|f| std::fs::read_to_string(f).ok())
        .unwrap_or_default();
    let text = format!("{mine}\n# Wheelhouse's, never committed\n.tmp/\n");
    if std::fs::read_to_string(&p).ok().as_deref() != Some(&text) {
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::write(&p, &text);
    }
    p
}

/// What the wall sets `GH_TOKEN` to: not a token, so `gh` and GitHub refuse
/// it, and `gh` never looks for yours.
pub const NO_TOKEN: &str = "wheelhouse-wall-no-token";

/// The pane's program for any agent Wheelhouse starts: `env` with the wall
/// up. No GitHub or tracker tokens, no SSH agent, `gh` signed out, and every
/// git push rewritten to fail, so an agent's mistake can't reach GitHub;
/// only the hub writes there, with what you approved. This stops mistakes,
/// not a determined agent running as you: see docs/security.md.
pub fn env_wrapper(wt: &Path, path: &[String]) -> Vec<String> {
    let mut v: Vec<String> = ["env", "-u", "LINEAR_API_KEY", "-u", "WH_LINEAR_KEY", "-u", "SSH_AUTH_SOCK"].iter().map(|s| s.to_string()).collect();
    // A token that works nowhere, rather than none: with none, `gh auth
    // token` reads yours from the Keychain and hands it out.
    v.push(format!("GH_TOKEN={NO_TOKEN}"));
    v.push(format!("GITHUB_TOKEN={NO_TOKEN}"));
    let instead = format!("url.{NEVER}.pushInsteadOf");
    let git: Vec<(String, String)> = vec![
        ("remote.origin.pushurl".into(), NEVER.into()),
        ("credential.helper".into(), String::new()),
        (instead.clone(), "https://".into()),
        (instead.clone(), "git@".into()),
        (instead, "ssh://".into()),
        ("core.excludesFile".into(), agent_gitignore().display().to_string()),
    ];
    v.push(format!("PATH={}", agent_path(path)));
    v.push(format!("GH_CONFIG_DIR={}", wt.join(".tmp/gh-empty").display()));
    v.push("GIT_TERMINAL_PROMPT=0".into());
    v.push("GIT_SSH_COMMAND=false".into());
    v.push(format!("GIT_CONFIG_COUNT={}", git.len()));
    for (i, (k, val)) in git.iter().enumerate() {
        v.push(format!("GIT_CONFIG_KEY_{i}={k}"));
        v.push(format!("GIT_CONFIG_VALUE_{i}={val}"));
    }
    v.push(format!("CARGO_HOME={}", super::cache_dir().join("cargo").display()));
    v
}

/// What starts an agent, and the session it will have (Claude Code's is
/// chosen up front; Codex's is found after it starts).
pub struct Launch {
    pub argv: Vec<String>,
    pub session: Option<String>,
}

pub struct ReviewerLaunch<'a> {
    pub agent: &'a str,
    pub extra_args: &'a [String],
    pub resume: Option<&'a str>,
    pub worktree: &'a Path,
    /// Writable folders: git's worktree metadata.
    pub dirs: &'a [PathBuf],
    pub caches: &'a [PathBuf],
    pub effort: Option<&'a str>,
    pub title: &'a str,
}

pub fn reviewer(cfg: &Config, l: &ReviewerLaunch) -> Launch {
    let args = with_effort(l.agent, &base_args(cfg, l.agent, l.extra_args), l.effort);
    let mut argv = env_wrapper(l.worktree, &cfg.path_dirs);
    argv.push(program(cfg, l.agent));
    if l.agent == "claude" {
        let session = l.resume.map(str::to_string).unwrap_or_else(uuid_v4);
        argv.extend([if l.resume.is_some() { "--resume" } else { "--session-id" }.to_string(), session.clone()]);
        argv.extend(args);
        for d in l.dirs {
            argv.extend(["--add-dir".to_string(), d.display().to_string()]);
        }
        // Caches go through settings, not --add-dir: Claude Code asks to
        // trust every untrusted --add-dir folder, and that prompt would hold
        // the pane.
        let caches: Vec<String> = l.caches.iter().map(|c| c.display().to_string()).collect();
        if !caches.is_empty() {
            argv.extend(["--settings".to_string(), json!({ "permissions": { "additionalDirectories": caches } }).to_string()]);
        }
        // No MCP servers: a reviewer reads the PR, and a tracker's or a
        // cluster's tools would route around the wall.
        argv.push("--strict-mcp-config".into());
        argv.extend(["-n".to_string(), l.title.to_string()]);
        Launch { argv, session: Some(session) }
    } else if l.agent == "agy" {
        if let Some(id) = l.resume {
            argv.extend(["--conversation".to_string(), id.to_string()]);
        }
        argv.extend(args);
        for d in l.dirs.iter().chain(l.caches) {
            argv.extend(["--add-dir".to_string(), d.display().to_string()]);
        }
        argv.extend(["--settings".to_string(), json!({ "permissions": { "deny": ["mcp(*)"] } }).to_string()]);
        Launch { argv, session: l.resume.map(str::to_string) }
    } else {
        if let Some(t) = l.resume {
            argv.extend(["resume".to_string(), t.to_string()]);
        }
        argv.extend(args);
        argv.extend(codex_mcp_off(&program(cfg, l.agent)));
        for d in l.dirs.iter().chain(l.caches) {
            argv.extend(["--add-dir".to_string(), d.display().to_string()]);
        }
        argv.extend(["-C".to_string(), l.worktree.display().to_string()]);
        Launch { argv, session: l.resume.map(str::to_string) }
    }
}

/// The checker: always a fresh session, allowed to preview (and in auto
/// mode, post) through `wh`, never to dismiss.
pub fn checker(cfg: &Config, worktree: &Path, others: &[PathBuf], effort: Option<&str>, auto: bool, title: &str) -> Launch {
    let agent = cfg.checker.as_str();
    let mut extra = Vec::new();
    if agent == "codex" {
        // The sandbox needs the network to reach the hub's socket.
        extra.extend(["-c".to_string(), "sandbox_workspace_write.network_access=true".to_string()]);
    }
    extra.extend(cfg.checker_extra_args.iter().cloned());
    let args = with_effort(agent, &base_args(cfg, agent, &extra), effort);
    let mut argv = env_wrapper(worktree, &cfg.path_dirs);
    argv.push(program(cfg, agent));
    if agent == "claude" {
        let session = uuid_v4();
        argv.extend(["--session-id".to_string(), session.clone()]);
        argv.extend(args);
        for d in others {
            argv.extend(["--add-dir".to_string(), d.display().to_string()]);
        }
        argv.push("--allowedTools".into());
        argv.push("Bash(wh review preview:*)".into());
        if auto {
            argv.push("Bash(wh review post:*)".into());
        }
        argv.push("--disallowedTools".into());
        argv.push("Bash(wh review dismiss:*)".into());
        if !auto {
            argv.push("Bash(wh review post:*)".into());
        }
        argv.push("--strict-mcp-config".into());
        argv.extend(["-n".to_string(), title.to_string()]);
        Launch { argv, session: Some(session) }
    } else if agent == "agy" {
        argv.extend(args);
        for d in others {
            argv.extend(["--add-dir".to_string(), d.display().to_string()]);
        }
        // Whole-word prefixes: agy takes no glob in `command(…)`.
        let mut allow = vec!["command(wh review preview)"];
        let mut deny = vec!["command(wh review dismiss)", "mcp(*)"];
        if auto {
            allow.push("command(wh review post)");
        } else {
            deny.push("command(wh review post)");
        }
        argv.extend(["--settings".to_string(), json!({ "permissions": { "allow": allow, "deny": deny } }).to_string()]);
        Launch { argv, session: None }
    } else {
        argv.extend(args);
        argv.extend(codex_mcp_off(&program(cfg, agent)));
        for d in others {
            argv.extend(["--add-dir".to_string(), d.display().to_string()]);
        }
        argv.extend(["-C".to_string(), worktree.display().to_string()]);
        Launch { argv, session: None }
    }
}

// ---- What the agents' logs say ----

/// Claude Code's folder for a working directory's sessions.
pub fn claude_project(cwd: &Path) -> PathBuf {
    let s: String = cwd.to_string_lossy().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    crate::paths::home().join(".claude/projects").join(s)
}

/// `claude-opus-5-5` → `Opus 5.5`; `claude-sonnet-4-5-20250929` → `Sonnet 4.5`.
pub fn claude_model_name(id: &str) -> Option<String> {
    let mut parts = id.strip_prefix("claude-")?.split('-');
    let family = parts.next()?;
    let mut f = family.chars();
    let family = f.next()?.to_uppercase().chain(f).collect::<String>();
    let nums: Vec<&str> = parts.take_while(|p| (1..=2).contains(&p.len()) && p.chars().all(|c| c.is_ascii_digit())).collect();
    Some(if nums.is_empty() { family } else { format!("{family} {}", nums.join(".")) })
}

/// The last model and effort a Claude Code session ran with.
fn claude_last(cwd: &Path, session: &str) -> (Option<String>, Option<String>) {
    let Ok(f) = std::fs::File::open(claude_project(cwd).join(format!("{session}.jsonl"))) else { return (None, None) };
    let (mut model, mut effort) = (None, None);
    for line in std::io::BufReader::new(f).lines().map_while(Result::ok) {
        if !line.contains("\"assistant\"") {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
        if v["type"] != "assistant" {
            continue;
        }
        if let Some(m) = v["message"]["model"].as_str().filter(|m| m.starts_with("claude-")) {
            model = Some(m.to_string());
        }
        if let Some(e) = v["effort"].as_str() {
            effort = Some(e.to_string());
        }
    }
    (model, effort)
}

fn codex_db() -> Option<PathBuf> {
    let dir = crate::paths::home().join(".codex");
    let mut dbs: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(&dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("state_") && n.ends_with(".sqlite")))
        .filter_map(|p| Some((p.metadata().ok()?.modified().ok()?, p)))
        .collect();
    dbs.sort();
    dbs.pop().map(|(_, p)| p)
}

pub fn codex_conn() -> Option<rusqlite::Connection> {
    let db = codex_db()?;
    let ro = rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI;
    rusqlite::Connection::open_with_flags(&db, ro).ok().filter(|c| c.query_row("select 1 from threads limit 1", [], |_| Ok(())).is_ok()).or_else(|| {
        // No -shm when Codex isn't running: read it as an immutable file.
        rusqlite::Connection::open_with_flags(format!("file:{}?immutable=1", db.display()), ro).ok()
    })
}

/// The Codex thread started in `cwd` since `since` (seconds), skipping subagents.
pub fn codex_thread(cwd: &Path, since: i64) -> Option<String> {
    let c = codex_conn()?;
    c.query_row(
        "select id from threads where cwd = ?1 and created_at >= ?2 and archived = 0 and source not like '{%' order by created_at desc limit 1",
        rusqlite::params![cwd.to_string_lossy(), since],
        |r| r.get::<_, String>(0),
    )
    .ok()
}

fn codex_model(thread: &str) -> Option<String> {
    let c = codex_conn()?;
    c.query_row("select model from threads where id = ?1", [thread], |r| r.get::<_, Option<String>>(0)).ok().flatten()
}

/// Antigravity CLI's own folder: `conversations/<id>.db` and `brain/<id>/`.
pub fn agy_home() -> PathBuf {
    crate::paths::home().join(".gemini/antigravity-cli")
}

/// Antigravity CLI's transcript for a conversation, under `root`.
pub fn agy_transcript(root: &Path, session: &str) -> PathBuf {
    root.join("brain").join(session).join(".system_generated/logs/transcript.jsonl")
}

/// One Antigravity conversation, from its database and transcript.
#[derive(Debug, Clone, PartialEq)]
pub struct AgyConversation {
    pub id: String,
    pub created: i64,
    pub updated: i64,
    /// The conversation that spawned it, for a subagent: its first step is
    /// a message from its caller, `sender=<id>`.
    pub parent: Option<String>,
}

fn epoch_of(t: std::io::Result<std::time::SystemTime>) -> Option<i64> {
    Some(t.ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs() as i64)
}

/// `file://` plus the path, percent-encoded as Antigravity writes it.
fn file_uri(cwd: &Path) -> Vec<u8> {
    let mut out = b"file://".to_vec();
    for &b in cwd.to_string_lossy().as_bytes() {
        if b.is_ascii_alphanumeric() || b"/-._~".contains(&b) {
            out.push(b);
        } else {
            out.extend(format!("%{b:02X}").bytes());
        }
    }
    out
}

/// Whether a conversation's metadata (protobuf) holds `uri` as a whole
/// string field: its length goes just before it, so `/w/pr-7` isn't
/// `/w/pr-70`.
fn holds_string(blob: &[u8], uri: &[u8]) -> bool {
    let mut len = Vec::new();
    let mut n = uri.len();
    loop {
        let b = (n & 0x7f) as u8;
        n >>= 7;
        if n == 0 {
            len.push(b);
            break;
        }
        len.push(b | 0x80);
    }
    let mut want = len;
    want.extend_from_slice(uri);
    blob.windows(want.len()).any(|w| w == want.as_slice())
}

fn agy_workspace_blob(db: &Path) -> Option<Vec<u8>> {
    let ro = rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI;
    let q = |c: rusqlite::Connection| c.query_row("select data from trajectory_metadata_blob limit 1", [], |r| r.get::<_, Vec<u8>>(0)).ok();
    rusqlite::Connection::open_with_flags(db, ro)
        .ok()
        .and_then(q)
        .or_else(|| rusqlite::Connection::open_with_flags(format!("file:{}?immutable=1", db.display()), ro).ok().and_then(q))
}

/// The caller of a subagent conversation, from its first transcript step.
fn agy_parent(root: &Path, id: &str) -> Option<String> {
    let f = std::fs::File::open(agy_transcript(root, id)).ok()?;
    let first = std::io::BufReader::new(f).lines().next()?.ok()?;
    let v: Value = serde_json::from_str(&first).ok()?;
    if v["type"] != "SYSTEM_MESSAGE" {
        return None;
    }
    let c = v["content"].as_str()?;
    let at = c.find("[Message] ")?;
    let sender = c[at..].split_whitespace().find_map(|w| w.strip_prefix("sender="))?;
    Some(sender.to_string())
}

/// Antigravity conversations in `cwd` that moved from `since` (seconds)
/// on, subagents too, oldest first.
pub fn agy_conversations(root: &Path, cwd: &Path, since: i64) -> Vec<AgyConversation> {
    let uri = file_uri(cwd);
    let mut out = Vec::new();
    for e in std::fs::read_dir(root.join("conversations")).into_iter().flatten().flatten() {
        let p = e.path();
        if p.extension().is_none_or(|x| x != "db") {
            continue;
        }
        let Some(id) = p.file_stem().and_then(|s| s.to_str()).map(str::to_string) else { continue };
        let meta = e.metadata().ok();
        let wal = std::fs::metadata(p.with_extension("db-wal")).ok().and_then(|m| epoch_of(m.modified()));
        let updated = meta.as_ref().and_then(|m| epoch_of(m.modified())).max(wal).unwrap_or(0);
        if updated < since {
            continue;
        }
        if !agy_workspace_blob(&p).is_some_and(|b| holds_string(&b, &uri)) {
            continue;
        }
        let created = meta.as_ref().and_then(|m| epoch_of(m.created())).unwrap_or(updated);
        out.push(AgyConversation { parent: agy_parent(root, &id), id, created, updated });
    }
    out.sort_by_key(|c| c.created);
    out
}

/// The Antigravity conversation started in `cwd` since `since` (seconds),
/// skipping subagents. An interactive agy starts its conversation with its
/// first prompt, so look after the prompt goes in.
pub fn agy_conversation(cwd: &Path, since: i64) -> Option<String> {
    agy_conversations(&agy_home(), cwd, since).into_iter().rev().find(|c| c.parent.is_none() && c.created >= since).map(|c| c.id)
}

/// What an Antigravity transcript says: model and effort last run with,
/// tokens (`[input, cache_read, cache_write, output]`), last words, when it
/// began and last moved, whether its last step finished its turn, and the
/// subagents it invoked (`Prompt` → (`Role`, `TypeName`)).
#[derive(Debug, Default)]
pub struct AgyRead {
    pub model: Option<String>,
    pub effort: Option<String>,
    pub tokens: [i64; 4],
    pub words: Option<String>,
    pub first: Option<String>,
    pub last: Option<String>,
    pub finished: bool,
    pub invoked: Vec<(String, Option<String>, Option<String>)>,
}

pub fn agy_read(root: &Path, session: &str) -> AgyRead {
    let mut r = AgyRead::default();
    let Ok(f) = std::fs::File::open(agy_transcript(root, session)) else { return r };
    let n = |v: &Value| v.as_i64().or_else(|| v.as_str().and_then(|s| s.parse().ok())).unwrap_or(0);
    // Tool arguments are JSON-encoded strings: `"\"Reply with pong.\""`.
    let arg = |v: &Value| v.as_str().map(|s| serde_json::from_str::<String>(s).unwrap_or_else(|_| s.to_string()));
    for line in std::io::BufReader::new(f).lines().map_while(Result::ok) {
        let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
        if let Some(at) = v["created_at"].as_str() {
            r.first.get_or_insert_with(|| at.to_string());
            r.last = Some(at.to_string());
        }
        if v["type"] != "PLANNER_RESPONSE" {
            r.finished = false;
            continue;
        }
        if let Some(m) = v["model"].as_str().filter(|m| !m.is_empty()) {
            let (pretty, eff) = agy_model_name(m);
            r.model = Some(pretty);
            if eff.is_some() {
                r.effort = eff;
            }
        }
        for (i, k) in ["input_tokens", "cache_read_tokens", "cache_write_tokens", "output_tokens"].iter().enumerate() {
            r.tokens[i] += n(&v[k]);
        }
        if let Some(t) = v["content"].as_str().or_else(|| v["response"].as_str()).filter(|t| !t.trim().is_empty()) {
            r.words = Some(t.to_string());
        }
        let calls = v["tool_calls"].as_array().map(Vec::as_slice).unwrap_or_default();
        for c in calls.iter().filter(|c| c["name"] == "invoke_subagent") {
            if let Some(p) = arg(&c["args"]["Prompt"]) {
                r.invoked.push((p, arg(&c["args"]["Role"]), arg(&c["args"]["TypeName"])));
            }
        }
        r.finished = calls.is_empty() && v["status"] == "DONE";
    }
    r
}

/// `gemini-4-neon-medium` → `("Gemini 4 Neon", Some("medium"))`.
pub fn agy_model_name(id: &str) -> (String, Option<String>) {
    let mut parts: Vec<&str> = id.split('-').filter(|p| !p.is_empty()).collect();
    let effort = parts.last().copied().filter(|e| super::config::LEVELS.contains(e)).map(str::to_string);
    if effort.is_some() {
        parts.pop();
    }
    let name = parts
        .into_iter()
        .map(|p| {
            let mut c = p.chars();
            match c.next() {
                Some(first) => first.to_uppercase().chain(c).collect::<String>(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    (if name.is_empty() { id.to_string() } else { name }, effort)
}

/// A fake Antigravity conversation under `root` for tests: its database
/// names `cwd` as Antigravity's does (a length-prefixed `file://` string).
#[cfg(test)]
pub fn fake_agy(root: &Path, id: &str, cwd: &Path, lines: &[Value]) {
    std::fs::create_dir_all(root.join("conversations")).unwrap();
    let uri = file_uri(cwd);
    let mut blob = vec![0x0a, 0x24];
    blob.extend_from_slice(b"f2347147-0e29-4db4-89b5-d0e6e9816683");
    blob.push(0x32);
    blob.push(uri.len() as u8);
    blob.extend_from_slice(&uri);
    blob.push(0x7a);
    let c = rusqlite::Connection::open(root.join("conversations").join(format!("{id}.db"))).unwrap();
    c.execute_batch("create table trajectory_metadata_blob (id text primary key, data blob)").unwrap();
    c.execute("insert into trajectory_metadata_blob values ('main', ?1)", [blob]).unwrap();
    let t = agy_transcript(root, id);
    std::fs::create_dir_all(t.parent().unwrap()).unwrap();
    std::fs::write(t, lines.iter().map(Value::to_string).collect::<Vec<_>>().join("\n")).unwrap();
}

/// The last model and effort an Antigravity CLI conversation ran with.
fn agy_last(session: &str) -> (Option<String>, Option<String>) {
    let r = agy_read(&agy_home(), session);
    (r.model, r.effort)
}

/// How a harness and model sign a review: `Claude Code (Opus 5.5)`.
pub fn display_name(agent: &str, cwd: &Path, session: Option<&str>) -> String {
    match agent {
        "claude" => match session.and_then(|s| claude_last(cwd, s).0).and_then(|m| claude_model_name(&m)) {
            Some(m) => format!("Claude Code ({m})"),
            None => "Claude Code".into(),
        },
        "agy" => match session.and_then(|s| agy_last(s).0) {
            Some(m) => format!("Antigravity ({m})"),
            None => "Antigravity".into(),
        },
        _ => {
            let model = session.and_then(codex_model);
            let shown = model.as_ref().map(|m| {
                let cache = std::fs::read_to_string(crate::paths::home().join(".codex/models_cache.json")).ok().and_then(|s| serde_json::from_str::<Value>(&s).ok());
                cache
                    .and_then(|c| c["models"].as_array().and_then(|a| a.iter().find(|x| x["slug"] == m.as_str()).and_then(|x| x["display_name"].as_str().map(str::to_string))))
                    .unwrap_or_else(|| m.clone())
            });
            match shown {
                Some(m) => format!("Codex ({m})"),
                None => "Codex".into(),
            }
        }
    }
}

/// The effort a session last ran at, from its log.
pub fn session_effort(agent: &str, cwd: &Path, session: Option<&str>) -> Option<String> {
    let s = session?;
    match agent {
        "claude" => claude_last(cwd, s).1,
        "agy" => agy_last(s).1,
        _ => {
            let c = codex_conn()?;
            c.query_row("select reasoning_effort from threads where id = ?1", [s], |r| r.get::<_, Option<String>>(0)).ok().flatten()
        }
    }
}

/// "A, B and C".
pub fn byline(names: &[String]) -> String {
    let mut seen: Vec<&String> = Vec::new();
    for n in names {
        if !seen.contains(&n) {
            seen.push(n);
        }
    }
    match seen.len() {
        0 => "Wheelhouse".into(),
        1 => seen[0].clone(),
        n => format!("{} and {}", seen[..n - 1].iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "), seen[n - 1]),
    }
}

fn walk_jsonl(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            walk_jsonl(&p, out);
        } else if p.extension().is_some_and(|x| x == "jsonl") {
            out.push(p);
        }
    }
}

fn usage(input: i64, cache_read: i64, cache_write: i64, output: i64) -> Value {
    json!({ "input": input, "cache_read": cache_read, "cache_write": cache_write, "output": output, "total": input + cache_read + cache_write + output })
}

fn add(sum: &mut [i64; 4], u: [i64; 4]) {
    for i in 0..4 {
        sum[i] += u[i];
    }
}

/// Tokens spent in a worktree, by agent: `{claude: {input, cache_read,
/// cache_write, output, total}, codex: {…}, agy: {…}, total}`. Every session with this
/// cwd counts: reviewers, checkers, subagents.
pub fn tokens(wt: &Path) -> Value {
    let mut out = serde_json::Map::new();
    let mut total = 0;

    let mut files = Vec::new();
    walk_jsonl(&claude_project(wt), &mut files);
    let mut seen = std::collections::HashSet::new();
    let mut c = [0i64; 4];
    for f in files {
        let Ok(file) = std::fs::File::open(&f) else { continue };
        for line in std::io::BufReader::new(file).lines().map_while(Result::ok) {
            if !line.contains("\"usage\"") {
                continue;
            }
            let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
            let u = &v["message"]["usage"];
            if v["type"] != "assistant" || u.is_null() {
                continue;
            }
            // A response in several blocks repeats its usage on each line.
            if let Some(id) = v["message"]["id"].as_str()
                && !seen.insert(id.to_string())
            {
                continue;
            }
            let n = |k: &str| u[k].as_i64().unwrap_or(0);
            add(&mut c, [n("input_tokens"), n("cache_read_input_tokens"), n("cache_creation_input_tokens"), n("output_tokens")]);
        }
    }
    if c.iter().any(|x| *x > 0) {
        let u = usage(c[0], c[1], c[2], c[3]);
        total += u["total"].as_i64().unwrap_or(0);
        out.insert("claude".into(), u);
    }

    if let Some(conn) = codex_conn() {
        let mut x = [0i64; 4];
        let rows: Vec<(Option<i64>, Option<String>)> = conn
            .prepare("select tokens_used, rollout_path from threads where cwd = ?1")
            .and_then(|mut st| st.query_map([wt.to_string_lossy()], |r| Ok((r.get(0)?, r.get(1)?)))?.collect())
            .unwrap_or_default();
        for (used, rollout) in rows {
            match rollout.as_deref().and_then(codex_usage) {
                Some(u) => add(&mut x, u),
                None => add(&mut x, [used.unwrap_or(0), 0, 0, 0]),
            }
        }
        if x.iter().any(|v| *v > 0) {
            let u = usage(x[0], x[1], x[2], x[3]);
            total += u["total"].as_i64().unwrap_or(0);
            out.insert("codex".into(), u);
        }
    }
    let mut g = [0i64; 4];
    let root = agy_home();
    for c in agy_conversations(&root, wt, 0) {
        add(&mut g, agy_read(&root, &c.id).tokens);
    }
    if g.iter().any(|v| *v > 0) {
        let u = usage(g[0], g[1], g[2], g[3]);
        total += u["total"].as_i64().unwrap_or(0);
        out.insert("agy".into(), u);
    }
    out.insert("total".into(), json!(total));
    Value::Object(out)
}

/// A Codex rollout's last token count (also from an archived `.zst`).
fn codex_usage(rollout: &str) -> Option<[i64; 4]> {
    let text = match std::fs::read_to_string(rollout) {
        Ok(t) => t,
        Err(_) => {
            let z = format!("{rollout}.zst");
            if !Path::new(&z).exists() {
                return None;
            }
            let out = std::process::Command::new("zstd").args(["-dcq", &z]).env("PATH", super::github::command_path()).output().ok()?;
            String::from_utf8_lossy(&out.stdout).into_owned()
        }
    };
    let line = text.lines().rev().filter(|l| l.contains("\"token_count\"")).find_map(|l| {
        let v: Value = serde_json::from_str(l).ok()?;
        (!v["payload"]["info"].is_null()).then_some(v)
    })?;
    let t = &line["payload"]["info"]["total_token_usage"];
    let n = |k: &str| t[k].as_i64().unwrap_or(0);
    let cached = n("cached_input_tokens");
    Some([n("input_tokens") - cached, cached, n("cache_write_input_tokens"), n("output_tokens")])
}

/// Two token counts' sum, by agent.
pub fn add_tokens(a: &Value, b: &Value) -> Value {
    let mut out = serde_json::Map::new();
    let mut total = 0;
    for agent in ["codex", "claude", "agy"] {
        if a[agent].is_null() && b[agent].is_null() {
            continue;
        }
        let mut u = serde_json::Map::new();
        for f in super::rules::TOKEN_FIELDS {
            u.insert(f.into(), json!(a[agent][f].as_i64().unwrap_or(0) + b[agent][f].as_i64().unwrap_or(0)));
        }
        total += u["total"].as_i64().unwrap_or(0);
        out.insert(agent.into(), Value::Object(u));
    }
    out.insert("total".into(), json!(total));
    Value::Object(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn effort_replaces_whatever_was_there() {
        assert_eq!(with_effort("claude", &s(&["--model", "m", "--effort", "high"]), Some("xhigh")), s(&["--model", "m", "--effort", "xhigh"]));
        assert_eq!(with_effort("claude", &s(&["--effort=low"]), Some("max")), s(&["--effort", "max"]));
        assert_eq!(with_effort("claude", &s(&["--effort", "high"]), None), s(&["--effort", "high"]));
        assert_eq!(
            with_effort("codex", &s(&["-c", "model=\"x\"", "-c", "model_reasoning_effort=high", "--config=model_reasoning_effort=low"]), Some("medium")),
            s(&["-c", "model=\"x\"", "-c", "model_reasoning_effort=\"medium\""])
        );
    }

    #[test]
    fn model_names_and_bylines() {
        assert_eq!(claude_model_name("claude-opus-5-5").as_deref(), Some("Opus 5.5"));
        assert_eq!(claude_model_name("claude-sonnet-4-5-20250929").as_deref(), Some("Sonnet 4.5"));
        assert_eq!(claude_model_name("gpt-6"), None);
        assert_eq!(byline(&s(&["A", "B", "A", "C"])), "A, B and C");
        assert_eq!(byline(&s(&["A", "A"])), "A");
        let u = uuid_v4();
        assert_eq!((u.len(), &u[14..15]), (36, "4"));
    }

    #[test]
    fn claude_reviewers_and_checkers_start_as_rex_reviewer_started_them() {
        let raw = json!({
            "repos": { "o/api": { "checkout": "/r" } }, "reviewers": ["claude"], "checker": "claude",
            "agents": { "claude": { "program": "/bin/claude", "extra_args": ["--model", "claude-opus-5-5", "--effort", "high"] } }
        });
        let cfg = super::super::config::from_value("/x".into(), raw).unwrap();
        let wt = PathBuf::from("/w/pr-7");
        let l = reviewer(
            &cfg,
            &ReviewerLaunch {
                agent: "claude",
                extra_args: &[],
                resume: None,
                worktree: &wt,
                dirs: &[PathBuf::from("/r/.git/worktrees")],
                caches: &[PathBuf::from("/c/npm")],
                effort: Some("xhigh"),
                title: "wheelhouse review o/api#7",
            },
        );
        let a = l.argv.join(" ");
        assert!(a.starts_with("env -u LINEAR_API_KEY -u WH_LINEAR_KEY -u SSH_AUTH_SOCK GH_TOKEN=wheelhouse-wall-no-token GITHUB_TOKEN=wheelhouse-wall-no-token PATH="), "{a}");
        assert!(a.contains("GIT_CONFIG_KEY_2=url.wheelhouse-agents-never-push:///.pushInsteadOf GIT_CONFIG_VALUE_2=https://"), "{a}");
        assert!(a.contains(" GH_CONFIG_DIR=/w/pr-7/.tmp/gh-empty GIT_TERMINAL_PROMPT=0 GIT_SSH_COMMAND=false"), "{a}");
        assert!(a.contains(&format!("/bin/claude --session-id {} --permission-mode auto --model claude-opus-5-5 --effort xhigh --add-dir /r/.git/worktrees --settings {{\"permissions\":{{\"additionalDirectories\":[\"/c/npm\"]}}}} --strict-mcp-config -n", l.session.as_deref().unwrap())), "{a}");
        let c = checker(&cfg, &wt, &[], Some("high"), false, "t");
        let a = c.argv.join(" ");
        assert!(a.contains("--allowedTools Bash(wh review preview:*) --disallowedTools Bash(wh review dismiss:*) Bash(wh review post:*) --strict-mcp-config -n t"), "{a}");
        let c = checker(&cfg, &wt, &[], None, true, "t");
        assert!(c.argv.join(" ").contains("--allowedTools Bash(wh review preview:*) Bash(wh review post:*) --disallowedTools Bash(wh review dismiss:*) --strict-mcp-config -n"));
    }

    #[test]
    fn agy_reviewers_and_checkers_start_with_mode_effort_and_permissions() {
        assert_eq!(with_effort("agy", &s(&["--model", "m", "--effort", "high"]), Some("xhigh")), s(&["--model", "m", "--effort", "xhigh"]));
        assert_eq!(agy_model_name("gemini-4-neon-medium"), ("Gemini 4 Neon".into(), Some("medium".into())));
        let raw = json!({
            "repos": { "o/api": { "checkout": "/r" } }, "reviewers": ["agy"], "checker": "agy",
            "agents": { "agy": { "program": "/bin/agy", "extra_args": ["--model", "gemini-4-neon"] } }
        });
        let cfg = super::super::config::from_value("/x".into(), raw).unwrap();
        let wt = PathBuf::from("/w/pr-7");
        let l = reviewer(
            &cfg,
            &ReviewerLaunch {
                agent: "agy",
                extra_args: &[],
                resume: Some("conv-123"),
                worktree: &wt,
                dirs: &[PathBuf::from("/r/.git/worktrees")],
                caches: &[PathBuf::from("/c/npm")],
                effort: Some("high"),
                title: "wheelhouse review o/api#7",
            },
        );
        let a = l.argv.join(" ");
        assert!(a.contains("/bin/agy --conversation conv-123 --mode auto --model gemini-4-neon --effort high --add-dir /r/.git/worktrees --add-dir /c/npm"), "{a}");
        let c = checker(&cfg, &wt, &[], Some("high"), false, "t");
        let a = c.argv.join(" ");
        assert!(a.contains("--mode auto --model gemini-4-neon --effort high --settings"), "{a}");
        assert!(a.contains("command(wh review preview)") && a.contains("command(wh review dismiss)") && a.contains("command(wh review post)"), "{a}");
        // agy takes no glob in a command: none has one. MCP is denied whole.
        assert!(!a.split("command(").skip(1).any(|c| c.split(')').next().unwrap_or("").contains('*')), "{a}");
        assert!(a.contains("\"mcp(*)\""), "{a}");
    }
    #[test]
    fn codex_gets_each_mcp_server_switched_off_by_name() {
        let a = off_args(&["linear".into(), "k8s-prod".into(), "odd\"name".into(), "a.b".into()]);
        assert_eq!(a, ["-c", "mcp_servers.linear.enabled=false", "-c", "mcp_servers.k8s-prod.enabled=false"], "unquoted; nothing Codex couldn't name");
    }

    #[test]
    fn agy_conversations_are_found_by_folder_and_read_from_their_transcripts() {
        assert_eq!(file_uri(Path::new("/w/My Code")), b"file:///w/My%20Code".to_vec());
        let root = std::env::temp_dir().join(format!("wh-agy-read-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (wt, other) = (PathBuf::from("/w/pr-7"), PathBuf::from("/w/pr-70"));
        let planner = |content: &str, calls: Value, tokens: Value| {
            let mut v = json!({ "type": "PLANNER_RESPONSE", "status": "DONE", "created_at": "2026-11-23T23:57:36Z", "model": "gemini-4-neon-high", "input_tokens": tokens[0], "cache_read_tokens": tokens[1], "output_tokens": tokens[2], "content": content });
            if !calls.is_null() {
                v["tool_calls"] = calls;
            }
            v
        };
        let call = json!([{ "name": "invoke_subagent", "args": { "Prompt": "\"Find the flaky test.\"", "Role": "\"Test Hunter\"", "TypeName": "\"research\"" } }]);
        fake_agy(
            &root,
            "main",
            &wt,
            &[
                json!({ "type": "USER_INPUT", "created_at": "2026-11-23T23:57:35Z", "content": "<USER_REQUEST>\nReview it\n</USER_REQUEST>" }),
                planner("", call, json!([100, 0, 10])),
                // Older transcripts write counts as strings.
                planner("Looks good.", Value::Null, json!(["20", "300", "5"])),
            ],
        );
        fake_agy(
            &root,
            "sub",
            &wt,
            &[
                json!({ "type": "SYSTEM_MESSAGE", "created_at": "2026-11-23T23:57:39Z", "content": "<SYSTEM_MESSAGE>\n[Message] timestamp=2026-11-23T23:57:39Z sender=main priority=MESSAGE_PRIORITY_HIGH content=Find the flaky test.\n</SYSTEM_MESSAGE>" }),
                planner("", json!([{ "name": "send_message", "args": {} }]), json!([1, 2, 3])),
            ],
        );
        fake_agy(&root, "elsewhere", &other, &[planner("x", Value::Null, json!([1, 0, 1]))]);

        let cs = agy_conversations(&root, &wt, 0);
        let ids: Vec<(&str, Option<&str>)> = cs.iter().map(|c| (c.id.as_str(), c.parent.as_deref())).collect();
        assert!(ids.contains(&("main", None)) && ids.contains(&("sub", Some("main"))) && ids.len() == 2, "/w/pr-7 isn't /w/pr-70: {ids:?}");
        assert!(agy_conversations(&root, &wt, i64::MAX).is_empty(), "nothing moved since then");

        let r = agy_read(&root, "main");
        assert_eq!((r.model.as_deref(), r.effort.as_deref()), (Some("Gemini 4 Neon"), Some("high")));
        assert_eq!(r.tokens, [120, 300, 0, 15]);
        assert_eq!((r.words.as_deref(), r.finished), (Some("Looks good."), true));
        assert_eq!(r.invoked, vec![("Find the flaky test.".to_string(), Some("Test Hunter".to_string()), Some("research".to_string()))]);
        assert!(!agy_read(&root, "sub").finished, "its last step called a tool");
        let _ = std::fs::remove_dir_all(&root);
    }
}

#[cfg(test)]
mod shell {
    /// `cargo test print_agent_path -- --ignored --nocapture` shows what the
    /// agents get on this Mac.
    #[test]
    #[ignore]
    fn print_agent_path() {
        println!("WH_AGENT_PATH={}", super::agent_path(&["/first".into()]));
    }
}

#[cfg(test)]
mod agy_live {
    /// `WH_AGY_CWD=/path cargo test agy_live -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn print_agy_folder() {
        let cwd = std::path::PathBuf::from(std::env::var("WH_AGY_CWD").unwrap());
        let root = super::agy_home();
        for c in super::agy_conversations(&root, &cwd, 0) {
            let r = super::agy_read(&root, &c.id);
            println!("{} parent={:?} model={:?} tokens={:?} finished={} invoked={}", c.id, c.parent, r.model, r.tokens, r.finished, r.invoked.len());
        }
        println!("newest top-level: {:?}", super::agy_conversation(&cwd, 0));
        println!("tokens: {}", super::tokens(&cwd));
    }
}
