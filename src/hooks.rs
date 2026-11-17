//! The agents' hooks. Claude Code runs `wh hook --agent claude-code EVENT`
//! at `SessionStart`, `UserPromptSubmit` and `Stop`, with the hook's input on
//! stdin. The hook hands it to the hub and returns: it prints nothing (Claude
//! Code adds a SessionStart or UserPromptSubmit hook's output to the agent's
//! context) and always exits 0, so it never holds up or changes the agent.
//!
//! `wh hooks install` adds the three to `~/.claude/settings.json`, beside
//! whatever hooks are there. Codex comes once its hook events are checked
//! against a live Codex.

use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

const EVENTS: [&str; 3] = ["SessionStart", "UserPromptSubmit", "Stop"];
/// The hook's own timeout in the agent's config, in seconds.
const HOOK_TIMEOUT: u64 = 5;

/// `wh hook --agent APP EVENT`.
pub fn run(agent: &str, event: &str) -> Result<()> {
    let mut input = String::new();
    let _ = std::io::stdin().take(4 << 20).read_to_string(&mut input);
    // Outside Rex there's no pane, so nothing to put it on.
    let Some(block) = std::env::var_os("REX_BLOCK").and_then(|b| b.into_string().ok()) else { return Ok(()) };
    let payload: Value = serde_json::from_str(&input).unwrap_or(Value::Null);
    let body = json!({ "event": event, "agent": agent, "block": block, "payload": payload });
    if let Err(e) = crate::client::call("POST", "/api/hook", Some(&body), Duration::from_secs(2)) {
        log(&format!("{event}: {e:#}"));
    }
    Ok(())
}

fn log(line: &str) {
    let dir = crate::paths::state_dir();
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("hook.log")) {
        let _ = writeln!(f, "{} {line}", crate::time::now());
    }
}

// ---- installing ----

fn claude_settings() -> PathBuf {
    let dir = std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from).unwrap_or_else(|| crate::paths::home().join(".claude"));
    dir.join("settings.json")
}

fn command(event: &str) -> Result<String> {
    let exe = std::env::current_exe()?;
    let exe = exe.canonicalize().unwrap_or(exe);
    let exe = exe.to_string_lossy();
    let exe = if exe.contains([' ', '\'', '"']) { format!("'{}'", exe.replace('\'', r"'\''")) } else { exe.into_owned() };
    Ok(format!("{exe} hook --agent claude-code {event}"))
}

fn is_ours(hook: &Value) -> bool {
    hook["command"].as_str().is_some_and(|c| c.contains(" hook --agent ") && EVENTS.iter().any(|ev| c.trim_end().ends_with(ev)))
}

fn read(file: &Path) -> Result<Value> {
    match std::fs::read_to_string(file) {
        Ok(s) if s.trim().is_empty() => Ok(json!({})),
        Ok(s) => serde_json::from_str(&s).with_context(|| format!("{} isn't valid JSON", file.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(json!({})),
        Err(e) => Err(e).with_context(|| format!("reading {}", file.display())),
    }
}

/// Which of our events a config has.
fn installed(config: &Value) -> Vec<&'static str> {
    EVENTS
        .into_iter()
        .filter(|ev| {
            config["hooks"][ev].as_array().is_some_and(|groups| {
                groups.iter().any(|g| g["hooks"].as_array().is_some_and(|hs| hs.iter().any(is_ours)))
            })
        })
        .collect()
}

/// Whether Claude Code runs all of our hooks on this machine.
pub fn claude_installed() -> bool {
    read(&claude_settings()).is_ok_and(|c| installed(&c).len() == EVENTS.len())
}

/// Add ours (or point them at this `wh`), leaving every other hook as it
/// was. Our group goes last in each event's list.
fn install_into(config: &mut Value) -> Result<()> {
    remove_from(config);
    let root = config.as_object_mut().ok_or_else(|| anyhow!("settings isn't a JSON object"))?;
    let hooks = root.entry("hooks").or_insert_with(|| json!({})).as_object_mut().ok_or_else(|| anyhow!("hooks isn't an object"))?;
    for ev in EVENTS {
        let list = hooks.entry(ev).or_insert_with(|| json!([]));
        list.as_array_mut()
            .ok_or_else(|| anyhow!("hooks.{ev} isn't a list"))?
            .push(json!({ "hooks": [{ "type": "command", "command": command(ev)?, "timeout": HOOK_TIMEOUT }] }));
    }
    Ok(())
}

fn remove_from(config: &mut Value) {
    let Some(hooks) = config["hooks"].as_object_mut() else { return };
    for ev in EVENTS {
        let Some(groups) = hooks.get_mut(ev).and_then(Value::as_array_mut) else { continue };
        for g in groups.iter_mut() {
            if let Some(hs) = g["hooks"].as_array_mut() {
                hs.retain(|h| !is_ours(h));
            }
        }
        groups.retain(|g| g["hooks"].as_array().is_none_or(|h| !h.is_empty()));
        if groups.is_empty() {
            hooks.remove(ev);
        }
    }
}

/// Write a config in place: through a symlink to the file it points at, by
/// way of a temporary file beside it, so a failure leaves the old one whole.
fn write(file: &Path, config: &Value) -> Result<()> {
    let target = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let dir = target.parent().ok_or_else(|| anyhow!("{} has no folder", target.display()))?;
    let tmp = dir.join(format!(".{}.wh.tmp", target.file_name().map(|n| n.to_string_lossy()).unwrap_or_default()));
    let mut text = serde_json::to_string_pretty(config)?;
    text.push('\n');
    std::fs::write(&tmp, text).with_context(|| format!("writing {}", tmp.display()))?;
    if let Ok(meta) = std::fs::metadata(&target) {
        let _ = std::fs::set_permissions(&tmp, meta.permissions());
    }
    std::fs::rename(&tmp, &target).with_context(|| format!("replacing {}", target.display()))?;
    Ok(())
}

/// `wh hooks [status | install | remove]`.
pub fn cli(action: Option<&str>) -> Result<()> {
    let file = claude_settings();
    let shown = crate::paths::tilde(&file.to_string_lossy());
    let mut config = read(&file)?;
    match action.unwrap_or("status") {
        "status" => {
            let have = installed(&config);
            if have.len() == EVENTS.len() {
                println!("Claude Code  tracked: SessionStart, UserPromptSubmit and Stop hooks in {shown}");
            } else if have.is_empty() {
                println!("Claude Code  no hooks: agents are tracked by pane only. Run `wh hooks install`.");
            } else {
                println!("Claude Code  some hooks ({}): run `wh hooks install`", have.join(", "));
            }
            println!("Codex        tracked by pane only, for now");
        }
        "install" => {
            install_into(&mut config)?;
            write(&file, &config)?;
            println!("Claude Code  added SessionStart, UserPromptSubmit and Stop hooks to {shown}");
            println!("             Agents already running pick them up when they next start.");
        }
        "remove" => {
            remove_from(&mut config);
            write(&file, &config)?;
            println!("Claude Code  removed Wheelhouse's hooks from {shown}");
        }
        other => anyhow::bail!("wh hooks takes status, install or remove, not {other}"),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_keeps_other_hooks_and_is_idempotent() {
        let mut config = json!({
            "model": "opus",
            "hooks": {
                "Stop": [{ "hooks": [{ "type": "command", "command": "/x/rex-inbox hook stop" }] }],
                "SessionStart": [{ "matcher": "^startup$", "hooks": [{ "type": "command", "command": "other" }] }]
            }
        });
        install_into(&mut config).unwrap();
        install_into(&mut config).unwrap();
        assert_eq!(installed(&config), EVENTS.to_vec());
        assert_eq!(config["model"], "opus");
        let stop = config["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(stop.len(), 2, "rex-inbox's hook, then ours, once");
        assert_eq!(stop[0]["hooks"][0]["command"], "/x/rex-inbox hook stop");
        assert_eq!(config["hooks"]["SessionStart"][0]["hooks"][0]["command"], "other");

        remove_from(&mut config);
        assert!(installed(&config).is_empty());
        assert_eq!(config["hooks"]["Stop"].as_array().unwrap().len(), 1);
        assert!(config["hooks"].get("UserPromptSubmit").is_none());
    }
}
