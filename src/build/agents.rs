//! Build's agents and their wall: no GitHub or tracker credentials, no SSH,
//! every push rewritten to fail, `.tmp/` ignored by git, and for Claude Code
//! no `git push`, `gh` or `wh send|share|reply`, and no MCP servers.

use std::path::{Path, PathBuf};

use super::config::Config;
use crate::review::agentcli::{self, Launch};

/// Where every push from an agent's pane goes: nowhere.
const NEVER: &str = "wheelhouse-agents-never-push:///";

/// A git ignore file with yours and `.tmp/`, for the agents' git.
fn agent_gitignore() -> PathBuf {
    let p = crate::review::cache_dir().join("agent-gitignore");
    let home = crate::paths::home();
    let mine = ["~/.config/git/ignore", "~/.gitignore_global", "~/.gitignore"]
        .iter()
        .map(|f| crate::paths::expand(f))
        .find(|f| f.exists())
        .and_then(|f| std::fs::read_to_string(f).ok())
        .unwrap_or_default();
    let text = format!("{mine}\n# Wheelhouse's, never committed\n.tmp/\n");
    if std::fs::read_to_string(&p).ok().as_deref() != Some(&text) {
        let _ = std::fs::create_dir_all(p.parent().unwrap_or(&home));
        let _ = std::fs::write(&p, &text);
    }
    p
}

/// The pane's program: `env` with the wall up, then the agent.
pub fn env_wall(cfg: &Config, wt: &Path) -> Vec<String> {
    let mut v: Vec<String> = ["env", "-u", "GH_TOKEN", "-u", "GITHUB_TOKEN", "-u", "LINEAR_API_KEY", "-u", "SSH_AUTH_SOCK"].iter().map(|s| s.to_string()).collect();
    let instead = format!("url.{NEVER}.pushInsteadOf");
    let git: Vec<(String, String)> = vec![
        ("remote.origin.pushurl".into(), NEVER.into()),
        ("credential.helper".into(), String::new()),
        (instead.clone(), "https://".into()),
        (instead.clone(), "git@".into()),
        (instead, "ssh://".into()),
        ("core.excludesFile".into(), agent_gitignore().display().to_string()),
    ];
    v.push(format!("PATH={}", agentcli::agent_path(&cfg.path_dirs)));
    v.push(format!("GH_CONFIG_DIR={}", wt.join(".tmp/gh-empty").display()));
    v.push("GIT_TERMINAL_PROMPT=0".into());
    v.push("GIT_SSH_COMMAND=false".into());
    v.push(format!("GIT_CONFIG_COUNT={}", git.len()));
    for (i, (k, val)) in git.iter().enumerate() {
        v.push(format!("GIT_CONFIG_KEY_{i}={k}"));
        v.push(format!("GIT_CONFIG_VALUE_{i}={val}"));
    }
    v.push(format!("CARGO_HOME={}", crate::review::cache_dir().join("cargo").display()));
    v
}

pub struct Spec<'a> {
    pub agent: &'a str,
    pub extra_args: &'a [String],
    pub resume: Option<&'a str>,
    pub worktree: &'a Path,
    pub checkout: &'a Path,
    pub effort: Option<&'a str>,
    pub title: &'a str,
}

pub fn argv(cfg: &Config, s: &Spec) -> Launch {
    let a = cfg.agent(s.agent);
    let program = a.program.as_deref().map(|p| crate::paths::expand(p).display().to_string()).unwrap_or_else(|| s.agent.to_string());
    let mut args: Vec<String> = match (&a.args, s.agent) {
        (Some(x), _) => x.clone(),
        (None, "claude") => vec!["--permission-mode".into(), "auto".into()],
        (None, _) => ["--approve-for-me", "--no-alt-screen", "--disable", "apps", "-c", "plugins.\"github@openai-curated\".enabled=false", "-c", "sandbox_workspace_write.network_access=true"].iter().map(|x| x.to_string()).collect(),
    };
    args.extend(a.extra_args.iter().cloned());
    args.extend(s.extra_args.iter().cloned());
    let args = agentcli::with_effort(s.agent, &args, s.effort);
    // Commits land in the checkout's .git; the package caches are shared.
    let gitdir = s.checkout.join(".git");
    let caches: Vec<String> = std::iter::once(crate::review::cache_dir()).chain(cfg.caches.iter().filter(|c| c.exists()).cloned()).map(|c| c.display().to_string()).collect();
    let mut argv = env_wall(cfg, s.worktree);
    argv.push(program);
    if s.agent == "claude" {
        let session = s.resume.map(str::to_string).unwrap_or_else(agentcli::uuid_v4);
        argv.extend([if s.resume.is_some() { "--resume" } else { "--session-id" }.to_string(), session.clone()]);
        argv.extend(args);
        argv.extend(["--add-dir".to_string(), gitdir.display().to_string()]);
        argv.extend(["--settings".to_string(), serde_json::json!({ "permissions": { "additionalDirectories": caches } }).to_string()]);
        argv.push("--disallowedTools".into());
        for d in ["Bash(git push:*)", "Bash(gh:*)", "Bash(wh send:*)", "Bash(wh share:*)", "Bash(wh reply:*)"] {
            argv.push(d.into());
        }
        if !cfg.mcp {
            argv.push("--strict-mcp-config".into());
        }
        argv.extend(["-n".to_string(), s.title.to_string()]);
        Launch { argv, session: Some(session) }
    } else {
        if let Some(t) = s.resume {
            argv.extend(["resume".to_string(), t.to_string()]);
        }
        argv.extend(args);
        for d in std::iter::once(gitdir.display().to_string()).chain(caches) {
            argv.extend(["--add-dir".to_string(), d]);
        }
        argv.extend(["-C".to_string(), s.worktree.display().to_string()]);
        Launch { argv, session: s.resume.map(str::to_string) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_builder_starts_behind_the_wall() {
        let rc = crate::review::config::from_value("/x".into(), json!({ "repos": { "o/api": { "checkout": "/src/api" } } })).unwrap();
        let cfg = super::super::config::from_value(None, json!({}), Some(&rc)).unwrap();
        let l = argv(
            &cfg,
            &Spec { agent: "claude", extra_args: &[], resume: None, worktree: Path::new("/w/D-1"), checkout: Path::new("/src/api"), effort: Some("high"), title: "wheelhouse build D-1 builder" },
        );
        let a = l.argv.join(" ");
        assert!(a.starts_with("env -u GH_TOKEN -u GITHUB_TOKEN -u LINEAR_API_KEY -u SSH_AUTH_SOCK PATH="), "{a}");
        assert!(a.contains("GIT_SSH_COMMAND=false GIT_CONFIG_COUNT=6 GIT_CONFIG_KEY_0=remote.origin.pushurl GIT_CONFIG_VALUE_0=wheelhouse-agents-never-push:///"), "{a}");
        assert!(a.contains("GIT_CONFIG_KEY_2=url.wheelhouse-agents-never-push:///.pushInsteadOf GIT_CONFIG_VALUE_2=https://"), "{a}");
        assert!(a.contains("--effort high --add-dir /src/api/.git --settings"), "{a}");
        assert!(a.contains("--disallowedTools Bash(git push:*) Bash(gh:*) Bash(wh send:*) Bash(wh share:*) Bash(wh reply:*) --strict-mcp-config -n"), "{a}");
    }
}
