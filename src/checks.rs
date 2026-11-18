//! Witnessed checks (milestone 4, Verify). A repo's own checks (`checks` in
//! its table in build.lua) run by the hub itself, at a commit, behind the
//! same wall as the agents. Each writes a `check` entry: the command, the
//! commit it ran at, the patch it covered (the diff from the base, kept by
//! hash), its exit, and its output (kept by hash). The hub watched it run,
//! so it's *witnessed*: unlike an agent saying "tests pass", which is *said*.
//!
//! Checks have teeth: a Build doesn't open its PR, or push a round, on a
//! required check that failed (or didn't run) at that exact commit, unless
//! you say so at the gate, which the record keeps.

use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::build::config::CheckSpec;
use crate::hub::{Shared, Who};
use crate::record::Entry;
use crate::review::rounds::git;

/// The most of a check's output that's kept: its end, where failures are.
const OUTPUT_MAX: usize = 256 * 1024;

/// Dockets with a run going, and its name, so two never overlap.
static RUNNING: Mutex<BTreeMap<String, String>> = Mutex::new(BTreeMap::new());

/// Where a docket's checks run, and which.
#[derive(Clone, Debug)]
pub struct Target {
    pub repo: String,
    pub dir: PathBuf,
    /// The commit the change is measured from, for the patch.
    pub base: Option<String>,
    pub specs: Vec<CheckSpec>,
    /// Folders first on PATH (Build's `path`), as the agents have them.
    pub path: Vec<String>,
}

/// A docket's checks: its Build's worktree, or, for an agent's docket, its
/// folder when that's inside one of Build's repos.
pub fn target(shared: &Shared, docket: &str) -> Result<Target> {
    let cfg = crate::build::config(shared)?;
    let (task, state) = {
        let core = shared.lock();
        (crate::build::get(&core, docket)?, core.store.state(docket)?)
    };
    if let Some(t) = task.filter(|t| t.worktree.as_deref().is_some_and(|w| Path::new(w).join(".git").exists())) {
        let specs = cfg.repos.get(&t.repo).map(|r| r.checks.clone()).unwrap_or_default();
        return Ok(Target { repo: t.repo.clone(), dir: PathBuf::from(t.worktree.clone().unwrap_or_default()), base: t.base_head.clone(), specs, path: cfg.path_dirs.clone() });
    }
    let cwd = state.and_then(|s| s.agent).and_then(|a| a.cwd).map(|c| crate::paths::expand(&c)).ok_or_else(|| anyhow!("{docket} has no worktree or folder to check"))?;
    for (repo, r) in &cfg.repos {
        if cwd.starts_with(&r.checkout) {
            return Ok(Target { repo: repo.clone(), dir: cwd, base: None, specs: r.checks.clone(), path: cfg.path_dirs.clone() });
        }
    }
    bail!("{} isn't in one of Build's repos, so it has no checks", crate::paths::tilde(&cwd.to_string_lossy()))
}

fn hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// Keep bytes in the blob store; their `sha256:` name.
fn keep(shared: &Shared, bytes: &[u8]) -> Result<String> {
    let h = hex(bytes);
    shared.lock().store.put_blob(&h, bytes, &crate::time::now_ms())?;
    Ok(format!("sha256:{h}"))
}

/// The wall, as agents have it: no tokens, no SSH agent, a gh signed out,
/// and every push sent nowhere.
fn walled(cmd: &mut tokio::process::Command, dir: &Path, path: &[String]) {
    let empty = crate::paths::state_dir().join("gh-empty");
    let _ = std::fs::create_dir_all(&empty);
    for v in ["GH_TOKEN", "GITHUB_TOKEN", "LINEAR_API_KEY", "WH_LINEAR_KEY", "SSH_AUTH_SOCK"] {
        cmd.env_remove(v);
    }
    let never = "https://wheelhouse.invalid/no-push";
    let git = [("remote.origin.pushurl", never.to_string()), ("credential.helper", String::new()), (&*format!("url.{never}.pushInsteadOf"), "https://".to_string())];
    cmd.env("GIT_CONFIG_COUNT", git.len().to_string());
    for (i, (k, v)) in git.iter().enumerate() {
        cmd.env(format!("GIT_CONFIG_KEY_{i}"), k).env(format!("GIT_CONFIG_VALUE_{i}"), v);
    }
    cmd.env("GH_CONFIG_DIR", &empty)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_SSH_COMMAND", "false")
        // Your shell's PATH, Build's `path` first: what the builder had.
        .env("PATH", crate::review::agentcli::agent_path(path))
        .env("WH_CHECK", "1")
        .current_dir(dir)
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true);
}

/// One check's result: its exit (none when it timed out) and its output.
async fn run_one(spec: &CheckSpec, dir: &Path, path: &[String]) -> (Option<i32>, Vec<u8>, Duration) {
    let shell = std::env::var("SHELL").ok().filter(|s| s.starts_with('/')).unwrap_or_else(|| "/bin/zsh".into());
    let mut cmd = tokio::process::Command::new(shell);
    cmd.arg("-lc").arg(&spec.run);
    walled(&mut cmd, dir, path);
    cmd.stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
    let started = Instant::now();
    let child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return (Some(127), format!("couldn't start the check: {e}").into_bytes(), started.elapsed()),
    };
    match tokio::time::timeout(Duration::from_secs(spec.timeout), child.wait_with_output()).await {
        Ok(Ok(out)) => {
            let mut all = out.stdout;
            all.extend_from_slice(&out.stderr);
            let tail = if all.len() > OUTPUT_MAX { all[all.len() - OUTPUT_MAX..].to_vec() } else { all };
            (Some(out.status.code().unwrap_or(-1)), tail, started.elapsed())
        }
        Ok(Err(e)) => (Some(-1), format!("the check didn't finish: {e}").into_bytes(), started.elapsed()),
        Err(_) => (None, format!("timed out after {}s", spec.timeout).into_bytes(), started.elapsed()),
    }
}

/// Run a docket's checks now, at its worktree's HEAD, and write what the
/// hub saw. `run` names this run, so its entries can be found together.
async fn run(shared: &Shared, docket: &str, t: &Target, run: &str, why: &str) -> Result<Vec<Value>> {
    if t.specs.is_empty() {
        bail!("{} has no checks yet: add them in Settings, under Repos, or in build.lua", t.repo);
    }
    run_inner(shared, docket, t, run, why).await
}

async fn run_inner(shared: &Shared, docket: &str, t: &Target, run: &str, why: &str) -> Result<Vec<Value>> {
    let head = git(&t.dir, &["rev-parse", "HEAD"]).await.context("reading HEAD")?.trim().to_string();
    // Uncommitted changes to tracked files: then the check covers more than
    // the commit, and says so.
    let dirty = !git(&t.dir, &["status", "--porcelain", "--untracked-files=no"]).await.unwrap_or_default().trim().is_empty();
    let patch = match &t.base {
        Some(b) => match git(&t.dir, &["diff", &format!("{b}...{head}")]).await {
            Ok(d) if !d.is_empty() => Some(keep(shared, d.as_bytes())?),
            _ => None,
        },
        None => None,
    };
    let mut results = Vec::new();
    for spec in &t.specs {
        let (exit, output, took) = run_one(spec, &t.dir, &t.path).await;
        let ok = exit == Some(0);
        let out = keep(shared, &output)?;
        let mut blobs = vec![out.clone()];
        if let Some(p) = &patch {
            blobs.push(p.clone());
        }
        let short: String = head.chars().take(7).collect();
        let body = format!(
            "{}: `{}` · {} at {short}{}",
            spec.name,
            spec.run,
            if ok { "passed".to_string() } else { exit.map(|c| format!("failed, exit {c}")).unwrap_or_else(|| "timed out".into()) },
            if dirty { " (with uncommitted changes)" } else { "" }
        );
        let data = json!({
            "name": spec.name, "command": spec.run, "required": spec.required,
            "repo": t.repo, "head": head, "base": t.base, "dirty": dirty, "patch": patch,
            "exit": exit, "ok": ok, "ms": took.as_millis() as u64, "output": out, "run": run, "why": why,
        });
        shared.lock().append_with(docket, "check", &Who::Hub, Some(body), data.clone(), blobs)?;
        results.push(data);
    }
    shared.lock().live_changed();
    Ok(results)
}

/// Start a run in the background; its entries say how it went. It's
/// marked running before this returns, so a step that looks right after
/// sees it going. When one is going already, that one's name comes back.
pub fn spawn(shared: &Shared, docket: &str, t: Target, why: &str) -> String {
    let run = format!("{}-{}", docket.to_lowercase(), crate::time::now_ms().replace([':', '.', '-'], ""));
    {
        let mut going = RUNNING.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(r) = going.get(docket) {
            return r.clone();
        }
        going.insert(docket.to_string(), run.clone());
    }
    let (s, d, r, w) = (shared.clone(), docket.to_string(), run.clone(), why.to_string());
    tokio::spawn(async move {
        let out = self::run(&s, &d, &t, &r, &w).await;
        RUNNING.lock().unwrap_or_else(|e| e.into_inner()).remove(&d);
        if let Err(e) = out {
            say!("checks: {d}: {e:#}");
            let mut core = s.lock();
            let _ = core.append(&d, "observed", &Who::Hub, Some(format!("The checks didn't run: {e:#}")), json!({ "what": "check-error", "run": r }));
            core.build.check_now.notify_one();
        } else {
            s.lock().build.check_now.notify_one();
        }
    });
    run
}

pub fn running(docket: &str) -> bool {
    RUNNING.lock().unwrap_or_else(|e| e.into_inner()).contains_key(docket)
}

/// Each check's latest result on a docket, by name.
pub fn latest(entries: &[Entry]) -> BTreeMap<String, Value> {
    let mut out = BTreeMap::new();
    for e in entries.iter().filter(|e| e.kind == "check") {
        if let Some(n) = e.data["name"].as_str() {
            out.insert(n.to_string(), e.data.clone());
        }
    }
    out
}

/// Whether the required checks passed at exactly `head`; if not, why, in
/// words. `Ok` when there are no required checks.
pub fn passed_at(specs: &[CheckSpec], entries: &[Entry], head: &str) -> std::result::Result<(), String> {
    let last = latest(entries);
    let mut why = Vec::new();
    for s in specs.iter().filter(|s| s.required) {
        match last.get(&s.name) {
            Some(c) if c["head"] == head && c["ok"] == true && c["dirty"] != true => {}
            Some(c) if c["head"] == head => why.push(format!("`{}` failed", s.name)),
            _ => why.push(format!("`{}` hasn't run at {}", s.name, head.chars().take(7).collect::<String>())),
        }
    }
    if why.is_empty() { Ok(()) } else { Err(why.join(", ")) }
}

/// The entries of one run.
pub fn of_run<'a>(entries: &'a [Entry], run: &str) -> Vec<&'a Entry> {
    entries.iter().filter(|e| e.kind == "check" && e.data["run"] == run).collect()
}

/// The last lines of a check's output, for a prompt or a gate.
pub fn tail(shared: &Shared, output: &Value, lines: usize) -> String {
    let Some(hex) = output.as_str().and_then(|d| d.strip_prefix("sha256:")) else { return String::new() };
    let bytes = shared.lock().store.blob(hex).ok().flatten().unwrap_or_default();
    let text = String::from_utf8_lossy(&bytes);
    let all: Vec<&str> = text.lines().collect();
    all[all.len().saturating_sub(lines)..].join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::entry::Actor;

    fn check(name: &str, head: &str, ok: bool) -> Entry {
        Entry {
            v: 1,
            docket: "D-1".into(),
            kind: "check".into(),
            at: "t".into(),
            actor: Actor { typ: "hub".into(), id: "hub@local".into(), extra: Default::default() },
            body: None,
            data: json!({ "name": name, "head": head, "ok": ok, "dirty": false }),
            blobs: vec![],
            replaces: None,
        }
    }

    fn spec(name: &str, required: bool) -> CheckSpec {
        CheckSpec { name: name.into(), run: "true".into(), required, timeout: 60 }
    }

    #[test]
    fn required_checks_must_pass_at_that_exact_commit() {
        let specs = [spec("test", true), spec("lint", false)];
        let entries = vec![check("test", "aaa", false), check("test", "bbb", true), check("lint", "bbb", false)];
        assert_eq!(passed_at(&specs, &entries, "bbb"), Ok(()), "a failing optional check doesn't block");
        assert_eq!(passed_at(&specs, &entries[..1], "aaa"), Err("`test` failed".into()));
        assert_eq!(passed_at(&specs, &entries, "ccc"), Err("`test` hasn't run at ccc".into()));
        assert_eq!(passed_at(&[spec("lint", false)], &[], "x"), Ok(()), "nothing required, nothing to pass");
    }

    #[tokio::test]
    async fn a_check_runs_behind_the_wall() {
        let dir = std::env::temp_dir().join(format!("wh-check-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let s = CheckSpec { name: "env".into(), run: "echo token=${GH_TOKEN:-none} ssh=${SSH_AUTH_SOCK:-none} check=$WH_CHECK; exit 3".into(), required: true, timeout: 30 };
        // SAFETY: this test alone reads these, in its own child.
        unsafe { std::env::set_var("GH_TOKEN", "secret") };
        let (exit, out, _) = run_one(&s, &dir, &[]).await;
        assert_eq!(exit, Some(3));
        assert!(String::from_utf8_lossy(&out).contains("token=none ssh=none check=1"), "{}", String::from_utf8_lossy(&out));
        let quick = CheckSpec { name: "slow".into(), run: "sleep 5".into(), required: true, timeout: 1 };
        let (exit, out, _) = run_one(&quick, &dir, &[]).await;
        assert_eq!(exit, None);
        assert!(String::from_utf8_lossy(&out).contains("timed out"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
