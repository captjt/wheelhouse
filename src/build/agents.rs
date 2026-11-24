//! Build's agents and their wall: no GitHub or tracker credentials, no SSH,
//! every push rewritten to fail, `.tmp/` ignored by git, and for Claude Code
//! no `git push`, `gh` or `wh send|share|reply`, and no MCP servers.

use std::path::Path;

use super::config::Config;
use crate::review::agentcli::{self, Launch};

/// The pane's program: `env` with the wall up (Review's and Build's are one).
pub fn env_wall(cfg: &Config, wt: &Path) -> Vec<String> {
    agentcli::env_wrapper(wt, &cfg.path_dirs)
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
        (None, "agy") => vec!["--mode".into(), "auto".into()],
        (None, _) => [
            "--approve-for-me",
            "--no-alt-screen",
            "--disable",
            "apps",
            "-c",
            "plugins.\"github@openai-curated\".enabled=false",
            "-c",
            "sandbox_workspace_write.network_access=true",
        ]
        .iter()
        .map(|x| x.to_string())
        .collect(),
    };
    args.extend(a.extra_args.iter().cloned());
    args.extend(s.extra_args.iter().cloned());
    let args = agentcli::with_effort(s.agent, &args, s.effort);
    // Commits land in the checkout's .git; the package caches are shared.
    let gitdir = s.checkout.join(".git");
    let caches: Vec<String> = std::iter::once(crate::review::cache_dir()).chain(cfg.caches.iter().filter(|c| c.exists()).cloned()).map(|c| c.display().to_string()).collect();
    let mut argv = env_wall(cfg, s.worktree);
    argv.push(program.clone());
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
    } else if s.agent == "agy" {
        if let Some(id) = s.resume {
            argv.extend(["--conversation".to_string(), id.to_string()]);
        }
        argv.extend(args);
        for d in std::iter::once(gitdir.display().to_string()).chain(caches) {
            argv.extend(["--add-dir".to_string(), d]);
        }
        // agy matches `command(…)` by whole-word prefix and takes no glob:
        // `command(git push*)` would never match `git push`.
        let mut deny = vec!["command(git push)", "command(gh)", "command(wh send)", "command(wh share)", "command(wh reply)"];
        if !cfg.mcp {
            deny.push("mcp(*)");
        }
        argv.extend(["--settings".to_string(), serde_json::json!({ "permissions": { "deny": deny } }).to_string()]);
        Launch { argv, session: s.resume.map(str::to_string) }
    } else {
        if let Some(t) = s.resume {
            argv.extend(["resume".to_string(), t.to_string()]);
        }
        argv.extend(args);
        if !cfg.mcp {
            argv.extend(agentcli::codex_mcp_off(&program));
        }
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
            &Spec {
                agent: "claude",
                extra_args: &[],
                resume: None,
                worktree: Path::new("/w/D-1"),
                checkout: Path::new("/src/api"),
                effort: Some("high"),
                title: "wheelhouse build D-1 builder",
            },
        );
        let a = l.argv.join(" ");
        assert!(a.starts_with("env -u LINEAR_API_KEY -u WH_LINEAR_KEY -u SSH_AUTH_SOCK GH_TOKEN=wheelhouse-wall-no-token GITHUB_TOKEN=wheelhouse-wall-no-token PATH="), "{a}");
        assert!(a.contains("GIT_SSH_COMMAND=false GIT_CONFIG_COUNT=6 GIT_CONFIG_KEY_0=remote.origin.pushurl GIT_CONFIG_VALUE_0=wheelhouse-agents-never-push:///"), "{a}");
        assert!(a.contains("GIT_CONFIG_KEY_2=url.wheelhouse-agents-never-push:///.pushInsteadOf GIT_CONFIG_VALUE_2=https://"), "{a}");
        assert!(a.contains("--effort high --add-dir /src/api/.git --settings"), "{a}");
        assert!(a.contains("--disallowedTools Bash(git push:*) Bash(gh:*) Bash(wh send:*) Bash(wh share:*) Bash(wh reply:*) --strict-mcp-config -n"), "{a}");

        let agy = argv(
            &cfg,
            &Spec {
                agent: "agy",
                extra_args: &[],
                resume: Some("conv-42"),
                worktree: Path::new("/w/D-1"),
                checkout: Path::new("/src/api"),
                effort: Some("high"),
                title: "wheelhouse build D-1 builder",
            },
        );
        let a = agy.argv.join(" ");
        assert!(a.contains("agy --conversation conv-42 --mode auto --effort high --add-dir /src/api/.git"), "{a}");
        assert!(a.contains("command(git push)") && a.contains("command(gh)") && !a.contains("command(gh *)") && a.contains("mcp(*)"), "{a}");
    }
}
