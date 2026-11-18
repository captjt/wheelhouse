//! The review agents' command lines (Claude Code and Codex), and what Wheelhouse
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
            "claude" if a == "--effort" => 2,
            "claude" if a.starts_with("--effort=") => 1,
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
        "claude" => out.extend(["--effort".to_string(), level.to_string()]),
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
        (None, _) => CODEX_DEFAULTS.iter().map(|s| s.to_string()).collect(),
    };
    args.extend(a.extra_args);
    args.extend(extra.iter().cloned());
    args
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

/// The pane's program for any agent Wheelhouse starts: `env` with the wall
/// up. No GitHub or tracker tokens, no SSH agent, `gh` signed out, and every
/// git push rewritten to fail, so an agent's mistake can't reach GitHub;
/// only the hub writes there, with what you approved. This stops mistakes,
/// not a determined agent running as you: see docs/security.md.
pub fn env_wrapper(wt: &Path, path: &[String]) -> Vec<String> {
    let mut v: Vec<String> =
        ["env", "-u", "GH_TOKEN", "-u", "GITHUB_TOKEN", "-u", "LINEAR_API_KEY", "-u", "WH_LINEAR_KEY", "-u", "SSH_AUTH_SOCK"].iter().map(|s| s.to_string()).collect();
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
        argv.extend(["-n".to_string(), l.title.to_string()]);
        Launch { argv, session: Some(session) }
    } else {
        if let Some(t) = l.resume {
            argv.extend(["resume".to_string(), t.to_string()]);
        }
        argv.extend(args);
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
        argv.extend(["-n".to_string(), title.to_string()]);
        Launch { argv, session: Some(session) }
    } else {
        argv.extend(args);
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

fn codex_conn() -> Option<rusqlite::Connection> {
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

/// How a harness and model sign a review: `Claude Code (Opus 5.5)`.
pub fn display_name(agent: &str, cwd: &Path, session: Option<&str>) -> String {
    match agent {
        "claude" => match session.and_then(|s| claude_last(cwd, s).0).and_then(|m| claude_model_name(&m)) {
            Some(m) => format!("Claude Code ({m})"),
            None => "Claude Code".into(),
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
/// cache_write, output, total}, codex: {…}, total}`. Every session with this
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
    for agent in ["codex", "claude"] {
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
        assert!(a.starts_with("env -u GH_TOKEN -u GITHUB_TOKEN -u LINEAR_API_KEY -u WH_LINEAR_KEY -u SSH_AUTH_SOCK PATH="), "{a}");
        assert!(a.contains("GIT_CONFIG_KEY_2=url.wheelhouse-agents-never-push:///.pushInsteadOf GIT_CONFIG_VALUE_2=https://"), "{a}");
        assert!(a.contains(" GH_CONFIG_DIR=/w/pr-7/.tmp/gh-empty GIT_TERMINAL_PROMPT=0 GIT_SSH_COMMAND=false"), "{a}");
        assert!(a.contains(&format!("/bin/claude --session-id {} --permission-mode auto --model claude-opus-5-5 --effort xhigh --add-dir /r/.git/worktrees --settings {{\"permissions\":{{\"additionalDirectories\":[\"/c/npm\"]}}}} -n", l.session.as_deref().unwrap())), "{a}");
        let c = checker(&cfg, &wt, &[], Some("high"), false, "t");
        let a = c.argv.join(" ");
        assert!(a.contains("--allowedTools Bash(wh review preview:*) --disallowedTools Bash(wh review dismiss:*) Bash(wh review post:*) -n t"), "{a}");
        let c = checker(&cfg, &wt, &[], None, true, "t");
        assert!(c.argv.join(" ").contains("--allowedTools Bash(wh review preview:*) Bash(wh review post:*) --disallowedTools Bash(wh review dismiss:*) -n"));
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
