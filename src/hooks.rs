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
const AGY_EVENTS: [&str; 3] = ["SessionStart", "PreInvocation", "Stop"];
/// The hook's own timeout in the agent's config, in seconds.
const HOOK_TIMEOUT: u64 = 5;

/// `wh hook --agent APP EVENT`.
pub fn run(agent: &str, event: &str) -> Result<()> {
    let mut input = String::new();
    let _ = std::io::stdin().take(4 << 20).read_to_string(&mut input);
    // Outside Rex there's no pane, so nothing to put it on.
    if let Some(block) = std::env::var_os("REX_BLOCK").and_then(|b| b.into_string().ok()) {
        let payload: Value = serde_json::from_str(&input).unwrap_or(Value::Null);
        let body = json!({ "event": event, "agent": agent, "block": block, "payload": payload });
        if let Err(e) = crate::client::call("POST", "/api/hook", Some(&body), Duration::from_secs(2)) {
            log(&format!("{event}: {e:#}"));
        }
    }
    // Antigravity hooks expect a valid JSON object on stdout; Claude Code
    // expects nothing (its stdout is added to the agent's context).
    if agent == "agy" {
        println!("{{}}");
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

fn agy_hooks_file() -> PathBuf {
    let dir = std::env::var_os("AGY_CONFIG_DIR").map(PathBuf::from).unwrap_or_else(|| crate::paths::home().join(".gemini/config"));
    dir.join("hooks.json")
}

fn command(agent: &str, event: &str) -> Result<String> {
    let exe = std::env::current_exe()?;
    let exe = exe.canonicalize().unwrap_or(exe);
    let exe = exe.to_string_lossy();
    let exe = if exe.contains([' ', '\'', '"']) { format!("'{}'", exe.replace('\'', r"'\''")) } else { exe.into_owned() };
    Ok(format!("{exe} hook --agent {agent} {event}"))
}

fn is_ours(hook: &Value) -> bool {
    hook["command"].as_str().is_some_and(|c| c.contains(" hook --agent ") && EVENTS.iter().chain(AGY_EVENTS.iter()).any(|ev| c.trim_end().ends_with(ev)))
}

fn read(file: &Path) -> Result<Value> {
    match std::fs::read_to_string(file) {
        Ok(s) if s.trim().is_empty() => Ok(json!({})),
        Ok(s) => serde_json::from_str(&s).with_context(|| format!("{} isn't valid JSON", file.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(json!({})),
        Err(e) => Err(e).with_context(|| format!("reading {}", file.display())),
    }
}

/// Which of our events a Claude Code config has.
fn installed(config: &Value) -> Vec<&'static str> {
    EVENTS
        .into_iter()
        .filter(|ev| config["hooks"][ev].as_array().is_some_and(|groups| groups.iter().any(|g| g["hooks"].as_array().is_some_and(|hs| hs.iter().any(is_ours)))))
        .collect()
}

/// Which of our events an Antigravity `hooks.json` has.
fn agy_installed_events(config: &Value) -> Vec<&'static str> {
    AGY_EVENTS.into_iter().filter(|ev| config["wheelhouse"][ev].as_array().is_some_and(|hs| hs.iter().any(is_ours))).collect()
}

/// Whether Claude Code runs all of our hooks on this machine.
pub fn claude_installed() -> bool {
    read(&claude_settings()).is_ok_and(|c| installed(&c).len() == EVENTS.len())
}

/// Whether Antigravity CLI runs all of our hooks on this machine.
pub fn agy_installed() -> bool {
    read(&agy_hooks_file()).is_ok_and(|c| agy_installed_events(&c).len() == AGY_EVENTS.len())
}

/// Whether hooks are installed for `app` (`claude-code` or `agy`).
pub fn installed_for(app: &str) -> bool {
    match app {
        "claude-code" => claude_installed(),
        "agy" => agy_installed(),
        _ => false,
    }
}

/// Whether any supported agent has our hooks installed.
pub fn any_installed() -> bool {
    claude_installed() || agy_installed()
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
            .push(json!({ "hooks": [{ "type": "command", "command": command("claude-code", ev)?, "timeout": HOOK_TIMEOUT }] }));
    }
    Ok(())
}

/// Add Wheelhouse's named hook block to Antigravity's `hooks.json`, keeping
/// all other named hooks untouched.
fn install_agy_into(config: &mut Value) -> Result<()> {
    let root = config.as_object_mut().ok_or_else(|| anyhow!("hooks.json isn't a JSON object"))?;
    let mut wh = serde_json::Map::new();
    for ev in AGY_EVENTS {
        wh.insert(ev.to_string(), json!([{ "type": "command", "command": command("agy", ev)?, "timeout": HOOK_TIMEOUT }]));
    }
    root.insert("wheelhouse".into(), Value::Object(wh));
    Ok(())
}

fn remove_agy_from(config: &mut Value) {
    if let Some(root) = config.as_object_mut() {
        root.remove("wheelhouse");
    }
}

fn remove_from(config: &mut Value) {
    // `get_mut`, not `config["hooks"]`: indexing a missing key inserts null,
    // and a settings file with no hooks yet then can't be installed into.
    let Some(hooks) = config.get_mut("hooks").and_then(Value::as_object_mut) else { return };
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
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
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
    let agy_file = agy_hooks_file();
    let agy_shown = crate::paths::tilde(&agy_file.to_string_lossy());
    let mut agy_config = read(&agy_file)?;
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
            let agy_have = agy_installed_events(&agy_config);
            if agy_have.len() == AGY_EVENTS.len() {
                println!("Antigravity  tracked: SessionStart, PreInvocation and Stop hooks in {agy_shown}");
            } else if agy_have.is_empty() {
                println!("Antigravity  no hooks: agents are tracked by pane only. Run `wh hooks install`.");
            } else {
                println!("Antigravity  some hooks ({}): run `wh hooks install`", agy_have.join(", "));
            }
            println!("Codex        tracked by pane only, for now");
        }
        "install" => {
            install_into(&mut config)?;
            write(&file, &config)?;
            println!("Claude Code  added SessionStart, UserPromptSubmit and Stop hooks to {shown}");
            install_agy_into(&mut agy_config)?;
            write(&agy_file, &agy_config)?;
            println!("Antigravity  added SessionStart, PreInvocation and Stop hooks to {agy_shown}");
            println!("             Agents already running pick them up when they next start.");
        }
        "remove" => {
            remove_from(&mut config);
            write(&file, &config)?;
            println!("Claude Code  removed Wheelhouse's hooks from {shown}");
            if agy_file.exists() {
                remove_agy_from(&mut agy_config);
                write(&agy_file, &agy_config)?;
                println!("Antigravity  removed Wheelhouse's hooks from {agy_shown}");
            }
        }
        other => anyhow::bail!("wh hooks takes status, install or remove, not {other}"),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_into_settings_with_no_hooks_yet() {
        // A fresh Mac: settings.json is missing, empty, or has other keys only.
        for start in [json!({}), json!({ "model": "opus" })] {
            let mut c = start.clone();
            install_into(&mut c).unwrap();
            assert!(c["hooks"]["Stop"].as_array().is_some_and(|a| a.len() == 1), "{c}");
            remove_from(&mut c);
            assert!(c["hooks"].as_object().is_some_and(|h| h.is_empty()), "{c}");
        }
    }

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

    #[test]
    fn agy_hooks_install_and_remove_keep_other_hooks() {
        let mut config = json!({
            "lint-checker": {
                "PostToolUse": [{ "matcher": "run_command", "hooks": [{ "type": "command", "command": "./lint.sh" }] }]
            }
        });
        install_agy_into(&mut config).unwrap();
        install_agy_into(&mut config).unwrap();
        assert_eq!(agy_installed_events(&config), AGY_EVENTS.to_vec());
        assert!(config.get("lint-checker").is_some());
        assert_eq!(config["wheelhouse"]["Stop"].as_array().unwrap().len(), 1);
        remove_agy_from(&mut config);
        assert!(agy_installed_events(&config).is_empty());
        assert!(config.get("lint-checker").is_some());
    }
}
