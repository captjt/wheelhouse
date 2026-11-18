//! Starting an agent on a docket: Claude Code or Codex, in a new tab of the
//! `wheelhouse` Rex session, in a folder, with your prompt. The docket comes
//! first, and the pane is named after it (`wheelhouse/D-12`), so others can
//! message it.

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

use crate::agents;
use crate::hub::{Shared, Who};
use crate::record::fold::AgentRef;

pub const SESSION: &str = "wheelhouse";

pub fn program(app: &str) -> Result<(&'static str, &'static str)> {
    match app {
        "" | "claude" | "claude-code" => Ok(("claude", "claude-code")),
        "codex" => Ok(("codex", "codex")),
        other => bail!("Wheelhouse starts Claude Code or Codex, not {other}"),
    }
}

fn expand(cwd: &str) -> String {
    let home = crate::paths::home().to_string_lossy().into_owned();
    match cwd.trim() {
        "" | "~" => home,
        s if s.starts_with("~/") => format!("{home}{}", &s[1..]),
        s => s.to_string(),
    }
}

/// `docket`: an existing docket to work on, instead of a new one. It must be
/// open, and not already held by an agent in a live pane.
pub async fn start(shared: &Shared, app: &str, cwd: &str, prompt: Option<&str>, title: Option<&str>, docket: Option<&str>) -> Result<Value> {
    let (bin, app) = program(app)?;
    let cwd = expand(cwd);
    if !std::path::Path::new(&cwd).is_dir() {
        bail!("{cwd} isn't a folder on this Mac");
    }
    let prompt = prompt.map(str::trim).filter(|p| !p.is_empty());
    let shown = crate::paths::tilde(&cwd);
    let (docket, session) = {
        let mut core = shared.lock();
        let (t, src) = match (title.map(str::trim).filter(|t| !t.is_empty()), prompt.and_then(agents::title_from)) {
            (Some(t), _) => (t.to_string(), "person"),
            (None, Some(t)) => (t, "prompt"),
            (None, None) => (format!("{} in {}", crate::mirror::app_name(app), shown), "pane"),
        };
        let id = match docket {
            Some(d) => {
                let st = core.store.state(d)?.ok_or_else(|| anyhow::anyhow!("no docket {d}"))?;
                if !st.is_open() {
                    bail!("{d} is {}: reopen it first", st.status.replace('_', " "));
                }
                if st.agent.as_ref().is_some_and(|a| agents::live(&core, &a.block).is_some()) {
                    bail!("{d} already has an agent in a live pane: open it there, or end that session first");
                }
                d.to_string()
            }
            None => core.open(&Who::Owner, &t, json!({ "protocol": "agent", "title_source": src, "started": true }))?,
        };
        let session = core.mirror.sessions.values().find(|s| s.label == SESSION).map(|s| s.session_id.clone());
        (id, session)
    };
    // `rex run` starts the agent straight from Rex Server, with its short
    // login PATH: no nvm, pyenv or the rest `.zshrc` adds, so hooks that run
    // `node` fail. It gets your shell's PATH, as Build's and Review's do.
    let path = tokio::task::spawn_blocking(|| crate::review::agentcli::agent_path(&[])).await.context("reading your shell's PATH")?;
    let mut argv = vec!["env".to_string(), format!("PATH={path}"), bin.to_string()];
    if let Some(p) = prompt {
        // After `--`, a prompt that starts with `-` is a prompt, never a flag.
        argv.push("--".into());
        argv.push(p.to_string());
    }
    let argv: Vec<&str> = argv.iter().map(String::as_str).collect();
    let created = crate::rex::Rex.start(session.as_deref(), SESSION, &docket, &cwd, &argv).await.with_context(|| format!("starting {bin} in Rex"))?;
    let mut core = shared.lock();
    let agent = AgentRef { app: app.into(), host: "local".into(), block: created.block_id.clone(), rex_session: SESSION.into(), pane: docket.clone(), cwd: Some(shown) };
    // Rex may have shown the new pane to the mirror first, and it got a
    // docket of its own: that one goes, and the pane is this docket's.
    if let Some(other) = core.agents.by_block.get(&created.block_id).map(|t| t.docket.clone()).filter(|d| *d != docket) {
        core.append(&other, "set", &Who::Hub, None, json!({ "status": "dropped" }))?;
        core.append(&other, "release", &Who::Hub, None, json!({ "reason": format!("it's {docket}, started from Wheelhouse") }))?;
    }
    agents::adopt(&mut core, &docket, &agent, prompt);
    core.append(&docket, "claim", &Who::Hub, None, json!({ "source": "wheelhouse", "agent": agent }))?;
    if core.store.state(&docket)?.is_some_and(|s| s.status == "todo") {
        core.append(&docket, "set", &Who::Hub, None, json!({ "status": "in_progress" }))?;
    }
    if let Some(p) = prompt {
        core.append(&docket, "note", &Who::Owner, Some(p.to_string()), json!({ "via": "prompt", "from": "wheelhouse" }))?;
    }
    Ok(json!({ "docket": docket, "block": created.block_id }))
}

/// Folders an agent could start in: where agents have worked, Build's and
/// Review's checkouts, and each one's git worktrees (not Wheelhouse's own,
/// under `~/.local/share`). With a docket, `suggested` is where it should
/// start: its last agent's folder, its Build's worktree, or a folder its
/// intent or notes name.
pub async fn folders(shared: &Shared, docket: Option<&str>) -> Result<Value> {
    let (mut seen, text, last, worktree) = {
        let core = shared.lock();
        let mut states = core.store.states()?;
        states.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        let mut seen: Vec<String> = states.iter().filter_map(|s| s.agent.as_ref()?.cwd.clone()).collect();
        if let Some((c, _, _)) = &core.build.config {
            seen.extend(c.repos.values().map(|r| crate::paths::tilde(&r.checkout.to_string_lossy())));
        }
        let (mut text, mut last, mut worktree) = (String::new(), None, None);
        if let Some(d) = docket
            && let Some(st) = core.store.state(d)?
        {
            last = st.agent.as_ref().and_then(|a| a.cwd.clone());
            worktree = crate::build::get(&core, d).ok().flatten().and_then(|t| t.worktree).map(|w| crate::paths::tilde(&w));
            text = st.intent.as_ref().map(|i| i.body.clone()).unwrap_or_default();
            for s in core.store.entries(d)? {
                if let Ok(e) = s.entry()
                    && e.kind == "note"
                {
                    text.push('\n');
                    text.push_str(e.body.as_deref().unwrap_or(""));
                }
            }
        }
        (seen, text, last, worktree)
    };
    // Each repo's worktrees, once per repo.
    let mut repos: Vec<String> = Vec::new();
    for dir in seen.clone() {
        let full = crate::paths::expand(&dir);
        if !full.is_dir() {
            continue;
        }
        let Ok(Ok(out)) =
            tokio::time::timeout(std::time::Duration::from_secs(2), tokio::process::Command::new("git").arg("-C").arg(&full).args(["worktree", "list", "--porcelain"]).output())
                .await
        else {
            continue;
        };
        let list: Vec<String> = String::from_utf8_lossy(&out.stdout).lines().filter_map(|l| l.strip_prefix("worktree ")).map(crate::paths::tilde).collect();
        if list.first().is_some_and(|main| repos.contains(main)) {
            continue;
        }
        if let Some(main) = list.first() {
            repos.push(main.clone());
        }
        seen.extend(list);
        if repos.len() >= 24 {
            break;
        }
    }
    // One entry per real folder (git names /private/var, a HOME may say /var),
    // in the form first seen.
    let mut out: Vec<String> = Vec::new();
    let mut real: Vec<std::path::PathBuf> = Vec::new();
    for f in seen {
        let f = f.trim_end_matches('/').to_string();
        if f.is_empty() || f.contains("/.local/share/") {
            continue;
        }
        let Ok(r) = std::fs::canonicalize(crate::paths::expand(&f)) else { continue };
        if r.is_dir() && !real.contains(&r) {
            real.push(r);
            out.push(f);
        }
    }
    // The longest folder the docket's own words name, so a worktree beats its repo.
    let named = out
        .iter()
        .filter(|f| {
            let full = crate::paths::expand(f).to_string_lossy().into_owned();
            text.contains(f.as_str()) || text.contains(&full)
        })
        .max_by_key(|f| f.len())
        .cloned();
    let suggested = last.filter(|l| crate::paths::expand(l).is_dir()).or(worktree).or(named);
    Ok(json!({ "folders": out, "suggested": suggested }))
}
