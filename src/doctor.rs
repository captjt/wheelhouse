//! `wh doctor`: what's set up, what isn't, and what to do about each. It
//! reads the hub's API and this Mac, and changes nothing.

use anyhow::{Result, bail};
use serde_json::Value;
use std::path::Path;

#[derive(Default)]
struct Report {
    bad: usize,
    warn: usize,
}

impl Report {
    fn line(&mut self, mark: &str, what: &str, text: &str, fix: Option<&str>) {
        println!("  {mark} {what:<10} {text}");
        if let Some(f) = fix {
            println!("    {:<10} → {f}", "");
        }
    }
    fn ok(&mut self, what: &str, text: &str) {
        self.line("✓", what, text, None);
    }
    fn note(&mut self, what: &str, text: &str, fix: &str) {
        self.line("·", what, text, Some(fix));
    }
    fn warn(&mut self, what: &str, text: &str, fix: &str) {
        self.warn += 1;
        self.line("!", what, text, Some(fix));
    }
    fn bad(&mut self, what: &str, text: &str, fix: &str) {
        self.bad += 1;
        self.line("✗", what, text, Some(fix));
    }
}

fn git_checkout(p: &str) -> bool {
    crate::paths::expand(p).join(".git").exists()
}

fn get(path: &str) -> Option<Value> {
    crate::client::json("GET", path, None).ok()
}

pub fn run() -> Result<()> {
    let mut r = Report::default();
    println!("Wheelhouse doctor\n");

    println!("The hub");
    let Some(status) = get("/api/status") else {
        r.bad("hub", "not running, or not answering on its socket", "wh up (it starts at login after that), or wh serve in a terminal");
        offline(&mut r);
        return finish(r);
    };
    let commit = status["commit"].as_str().unwrap_or("unknown");
    let mine = env!("WH_COMMIT");
    r.ok("hub", &format!("running for {}m (pid {}), built from {commit}", status["uptime_secs"].as_u64().unwrap_or(0) / 60, status["pid"]));
    if commit != mine && commit != "unknown" && mine != "unknown" {
        r.warn("version", &format!("the hub runs {commit}, but this wh is {mine}"), "wh up restarts it on this one");
    }
    if crate::paths::home().join(format!("Library/LaunchAgents/{}.plist", crate::service::label())).exists() {
        r.ok("login", "starts at login (a LaunchAgent)");
    } else {
        r.note("login", "runs only while its terminal does", "wh up keeps it running, and starts it at login");
    }
    let state_dir = crate::paths::state_dir();
    {
        use std::os::unix::fs::PermissionsExt;
        match std::fs::metadata(&state_dir).map(|m| m.permissions().mode() & 0o077) {
            Ok(0) => r.ok("state", &format!("{} is yours alone", crate::paths::tilde(&state_dir.display().to_string()))),
            Ok(_) => r.bad("state", &format!("{} can be read by others", crate::paths::tilde(&state_dir.display().to_string())), "chmod 700 it: it holds your keys and the record"),
            Err(_) => r.warn("state", "can't read the state folder's permissions", "check WH_STATE"),
        }
    }

    println!("\nAgents");
    if status["rex"]["connected"] == true {
        r.ok("rex", "following your panes");
    } else {
        r.bad("rex", &format!("not connected: {}", status["rex"]["error"].as_str().unwrap_or("connecting")), "is Rex running? rex whoami says which server it's on");
    }
    if status["hooks"] == true {
        r.ok("hooks", "Claude Code's hooks are on: prompts, turns and /clear reach the record");
    } else {
        r.warn("hooks", "Claude Code's hooks are off: dockets get no prompts, turns or names", "wh hooks install");
    }

    println!("\nTools");
    match tool("gh") {
        Some(gh) => {
            let ok = std::process::Command::new(&gh)
                .args(["auth", "status"])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .is_ok_and(|s| s.success());
            if ok {
                r.ok("gh", "signed in: the hub posts and pushes with it, only what you approve");
            } else {
                r.warn("gh", "not signed in", "gh auth login (Review and Build read and write GitHub with it)");
            }
        }
        None => r.warn("gh", "not found", "brew install gh, then gh auth login"),
    }
    if tool("luajit").is_some() || std::env::var_os("WH_LUAJIT").is_some() || Path::new("/opt/homebrew/bin/luajit").exists() {
        r.ok("luajit", "found: it reads review.lua and build.lua");
    } else {
        r.warn("luajit", "not found, so review.lua and build.lua can't be read", "brew install luajit");
    }
    match status["tailnet"]["url"].as_str() {
        Some(u) => r.ok("tailnet", &format!("{u} (answers only {})", status["tailnet"]["owner"].as_str().unwrap_or("you"))),
        None => r.note("tailnet", "Tailscale not seen: the app is on this Mac only", "install Tailscale for your phone, then wh web --publish"),
    }

    println!("\nProtocols");
    let state = get("/api/state").unwrap_or_default();
    let settings = get("/api/settings").unwrap_or_default();
    if let Some(e) = settings["set_error"].as_str() {
        r.bad("settings", &format!("settings.json doesn't load: {e}"), "fix it, or move it aside; Settings rewrites it");
    }
    let review = get("/api/review").unwrap_or_default();
    match (review["enabled"].as_bool(), review["config_error"].as_str()) {
        (_, Some(e)) if review["enabled"] == true => r.bad("review", &format!("its config doesn't load: {e}"), "fix review.lua, or the setting named in the error"),
        (Some(true), _) => {
            if review["rex_reviewer_running"] == true {
                r.bad("review", "on, while rex-reviewer's watcher also runs: two tools would review the same PRs", "rex-reviewer down");
            } else if review["paused"] == true {
                r.note("review", "on, paused", "wh review resume");
            } else {
                r.ok("review", &format!("on, posting: {}", review["post"].as_str().unwrap_or("ask")));
            }
        }
        _ => r.note("review", "off", "set up review.lua (docs/review.md), then wh review on"),
    }
    let build = get("/api/build").unwrap_or_default();
    if let Some(e) = build["config_error"].as_str() {
        r.bad("build", &format!("its config doesn't load: {e}"), "fix build.lua, or the setting named in the error");
    } else if build["paused"] == true {
        r.note("build", "paused: nothing starts or moves on its own", "wh build resume");
    } else {
        r.ok("build", "ready");
    }
    let repos = settings["build"]["config"]["repos"].as_object().cloned().unwrap_or_default();
    for (slug, repo) in &repos {
        let checkout = repo["checkout"].as_str().unwrap_or("");
        if !git_checkout(checkout) {
            r.warn(slug, &format!("{checkout} isn't a git checkout"), "clone it there, or change its checkout in build.lua or review.lua");
        } else if repo["checks"].as_array().is_none_or(|c| c.is_empty()) {
            r.note(slug, "no witnessed checks: done rests on what agents say, and merges", "add its checks in Settings → Repos (docs/verify.md)");
        } else {
            let names: Vec<&str> = repo["checks"].as_array().into_iter().flatten().filter_map(|c| c["name"].as_str()).collect();
            r.ok(slug, &format!("checks: {}", names.join(", ")));
        }
    }
    if state["linear"] == true {
        r.ok("linear", "a key is set: issues link and come in");
    } else {
        r.note("linear", "the hub has no key: it links issues a PR names, but can't read them", "wh setup-linear, so issues seed intents, comments come in, and intake works");
    }

    println!("\nApps");
    let app = ["/Applications/Wheelhouse.app".into(), crate::paths::home().join("Applications/Wheelhouse.app")].into_iter().find(|p: &std::path::PathBuf| p.exists());
    match app {
        Some(p) => r.ok("mac app", &crate::paths::tilde(&p.display().to_string())),
        None => r.note("mac app", "Wheelhouse.app isn't installed", "just app (needs Xcode 26)"),
    }
    completion(&mut r);
    let attention = state["attention"].as_array().map_or(0, Vec::len);
    if attention > 0 {
        r.note("attention", &format!("{attention} docket{} need{} a look", if attention == 1 { "" } else { "s" }, if attention == 1 { "s" } else { "" }), "wh attention");
    }
    finish(r)
}

/// Whether Tab completes `wh` in the shell this runs in (its hook exports
/// a mark), or will in the next one (its startup file has the line).
fn completion(r: &mut Report) {
    if let Ok(shell) = std::env::var(crate::complete::MARK) {
        return r.ok("completion", &format!("Tab completes wh in {shell}"));
    }
    let login = std::env::var("SHELL").unwrap_or_default();
    let home = crate::paths::home();
    let (name, rc) = match login.rsplit('/').next() {
        Some("zsh") => ("zsh", std::env::var_os("ZDOTDIR").map(std::path::PathBuf::from).unwrap_or_else(|| home.clone()).join(".zshrc")),
        Some("bash") => ("bash", home.join(".bashrc")),
        _ => return,
    };
    let shown = crate::paths::tilde(&rc.display().to_string());
    if std::fs::read_to_string(&rc).is_ok_and(|s| s.contains("wh completions")) {
        r.note("completion", &format!("{shown} has it, but this shell started before"), "open a new terminal");
    } else {
        r.note("completion", &format!("Tab doesn't complete wh in this {name}"), &format!("add eval \"$(wh completions {name})\" to {shown}"));
    }
}

/// Without a hub: what can still be checked here.
fn offline(r: &mut Report) {
    if tool("luajit").is_none() && !Path::new("/opt/homebrew/bin/luajit").exists() {
        r.warn("luajit", "not found, so review.lua and build.lua can't be read", "brew install luajit");
    }
    if tool("gh").is_none() {
        r.warn("gh", "not found", "brew install gh, then gh auth login");
    }
    completion(r);
}

fn tool(name: &str) -> Option<std::path::PathBuf> {
    if name == "gh"
        && let Some(p) = std::env::var_os("WH_GH")
    {
        return Some(p.into());
    }
    crate::paths::which(name).or_else(|| ["/opt/homebrew/bin", "/usr/local/bin"].iter().map(|d| Path::new(d).join(name)).find(|p| p.exists()))
}

fn finish(r: Report) -> Result<()> {
    println!();
    match (r.bad, r.warn) {
        (0, 0) => println!("Everything checks out."),
        (0, w) => println!("{w} thing{} to look at.", if w == 1 { "" } else { "s" }),
        (b, _) => bail!("{b} problem{} to fix.", if b == 1 { "" } else { "s" }),
    }
    Ok(())
}
