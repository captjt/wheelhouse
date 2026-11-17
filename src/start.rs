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

pub async fn start(shared: &Shared, app: &str, cwd: &str, prompt: Option<&str>, title: Option<&str>) -> Result<Value> {
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
        let id = core.open(&Who::Owner, &t, json!({ "protocol": "agent", "title_source": src, "started": true }))?;
        let session = core.mirror.sessions.values().find(|s| s.label == SESSION).map(|s| s.session_id.clone());
        (id, session)
    };
    let mut argv = vec![bin.to_string()];
    if let Some(p) = prompt {
        argv.push(p.to_string());
    }
    let argv: Vec<&str> = argv.iter().map(String::as_str).collect();
    let created = crate::rex::Rex.start(session.as_deref(), SESSION, &docket, &cwd, &argv).await.with_context(|| format!("starting {bin} in Rex"))?;
    let mut core = shared.lock();
    let agent = AgentRef { app: app.into(), host: "local".into(), block: created.block_id.clone(), rex_session: SESSION.into(), pane: docket.clone(), cwd: Some(shown) };
    agents::adopt(&mut core, &docket, &agent, prompt);
    core.append(&docket, "claim", &Who::Hub, None, json!({ "source": "wheelhouse", "agent": agent }))?;
    if let Some(p) = prompt {
        core.append(&docket, "note", &Who::Owner, Some(p.to_string()), json!({ "via": "prompt", "from": "wheelhouse" }))?;
    }
    Ok(json!({ "docket": docket, "block": created.block_id }))
}
