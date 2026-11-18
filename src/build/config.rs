//! Build's config: `~/.config/wheelhouse/build.lua` (docs/build.md), read
//! with LuaJIT like Review's. Without one, Build still runs on the repos,
//! agents and PATH in `review.lua`, with the defaults.

use anyhow::{Result, bail};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::review::config::AgentCfg;
use crate::review::lua;

#[derive(Clone, Debug)]
pub struct Repo {
    pub checkout: PathBuf,
    pub notes: String,
    pub base: Option<String>,
    /// `full`, `summary`, or `off`: how much of the plan goes on the PR.
    pub plan: Option<String>,
    /// The repo's own checks, which the hub runs itself and witnesses.
    pub checks: Vec<CheckSpec>,
}

/// A check the hub runs in a worktree, at a commit (`checks` in a repo's
/// table): `test = "cargo test"`, or `lint = { run = "…", required = false,
/// timeout = 600 }`.
#[derive(Clone, Debug, PartialEq)]
pub struct CheckSpec {
    pub name: String,
    pub run: String,
    /// A required check that fails keeps the PR from opening, and a round
    /// from pushing, unless you say so.
    pub required: bool,
    pub timeout: u64,
}

fn checks(v: &Value) -> Result<Vec<CheckSpec>> {
    let mut out = Vec::new();
    for (name, c) in v.as_object().into_iter().flatten() {
        let (run, required, timeout) = match c {
            // `false`: a check build.lua lists, removed in Settings.
            Value::Bool(false) | Value::Null => continue,
            Value::String(s) => (s.clone(), true, 1200),
            Value::Object(o) => (
                o.get("run").and_then(Value::as_str).map(str::to_string).ok_or_else(|| anyhow::anyhow!("checks.{name} needs `run`, the command"))?,
                o.get("required").and_then(Value::as_bool).unwrap_or(true),
                o.get("timeout").and_then(Value::as_u64).unwrap_or(1200),
            ),
            _ => bail!("checks.{name} is a command, {{ run = …, required = …, timeout = … }}, or false (removed)"),
        };
        if run.trim().is_empty() {
            bail!("checks.{name} has no command");
        }
        out.push(CheckSpec { name: name.clone(), run, required, timeout: timeout.clamp(10, 7200) });
    }
    Ok(out)
}

/// `sources.linear`: Linear issues assigned to you that start a Build (with
/// the label) or are offered one (entering your active cycle).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LinearSource {
    /// Each team's key, and the repo its issues build into.
    pub teams: BTreeMap<String, String>,
    pub label: Option<String>,
    pub cycle: bool,
    pub skip_label: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Role {
    pub agent: String,
    pub extra_args: Vec<String>,
    pub effort: Value,
}

#[derive(Clone, Debug)]
pub struct Config {
    pub path: Option<PathBuf>,
    pub raw: Value,
    pub repos: BTreeMap<String, Repo>,
    pub github_label: Option<String>,
    pub linear: Option<LinearSource>,
    pub builder: Role,
    pub checker: Role,
    /// Reviewers who review the branch blind before the PR opens (none: no
    /// pre-review), and how many times they may send it back.
    pub prereview: Vec<Role>,
    pub prereview_loops: i64,
    /// `ask` or `auto`, by gate: `start`, `plan`, `preview`, `open`, and
    /// after it opens, `address` (start a round) and `push` (push a round
    /// and post its replies).
    pub gates: BTreeMap<String, String>,
    pub fix_loops: i64,
    pub max_decisions: i64,
    pub max_tasks: usize,
    pub poll_seconds: u64,
    pub pr_draft: bool,
    pub pr_reviewers: Vec<String>,
    pub branch_prefix: String,
    pub agents: BTreeMap<String, AgentCfg>,
    pub path_dirs: Vec<String>,
    pub caches: Vec<PathBuf>,
    pub mcp: bool,
    /// How a round brings the branch up to date with its base: `merge` (no
    /// force-push) or `rebase` (pushed with a lease).
    pub update: String,
}

pub fn path() -> PathBuf {
    std::env::var_os("WH_BUILD_CONFIG").map(PathBuf::from).unwrap_or_else(|| crate::review::config::dir().join("build.lua"))
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

fn role(v: &Value, field: &str, default: &str) -> Result<Role> {
    let r = match v {
        Value::String(s) => Role { agent: s.clone(), extra_args: Vec::new(), effort: Value::Null },
        Value::Object(_) => Role { agent: v["agent"].as_str().unwrap_or(default).to_string(), extra_args: strings(&v["extra_args"]), effort: v["effort"].clone() },
        _ => Role { agent: default.to_string(), extra_args: Vec::new(), effort: Value::Null },
    };
    if !matches!(r.agent.as_str(), "claude" | "codex") {
        bail!("{field} is claude or codex, not {}", r.agent);
    }
    Ok(r)
}

pub fn load() -> Result<Config> {
    let p = path();
    let raw = crate::settings::overlaid(if p.exists() { lua::dump(&p)? } else { Value::Object(Default::default()) }, "build");
    let review = crate::review::config::load().ok();
    from_value(p.exists().then_some(p), raw, review.as_ref())
}

pub fn from_value(path: Option<PathBuf>, raw: Value, review: Option<&crate::review::config::Config>) -> Result<Config> {
    let mut repos = BTreeMap::new();
    if let Some(rc) = review {
        for (slug, r) in &rc.repos {
            repos.insert(slug.clone(), Repo { checkout: r.checkout.clone(), notes: r.notes.clone(), base: None, plan: None, checks: Vec::new() });
        }
    }
    for (slug, r) in raw["repos"].as_object().into_iter().flatten() {
        if slug.split('/').count() != 2 {
            bail!("repos: {slug} isn't owner/name");
        }
        let prev = repos.get(slug).cloned();
        let checkout = r["checkout"].as_str().map(crate::paths::expand).or(prev.as_ref().map(|p| p.checkout.clone()));
        let Some(checkout) = checkout else { bail!("repos[\"{slug}\"].checkout is required") };
        let plan = match &r["plan"] {
            Value::String(s) if s == "full" || s == "summary" => Some(s.clone()),
            Value::Bool(false) => Some("off".into()),
            Value::Null => None,
            other => bail!("repos[\"{slug}\"].plan is full, summary or false, not {other}"),
        };
        repos.insert(
            slug.clone(),
            Repo {
                checkout,
                notes: r["notes"].as_str().map(str::to_string).or(prev.as_ref().map(|p| p.notes.clone())).unwrap_or_default(),
                base: r["base"].as_str().map(str::to_string),
                plan,
                checks: checks(&r["checks"]).map_err(|e| anyhow::anyhow!("repos[\"{slug}\"].{e}"))?,
            },
        );
    }
    let mut gates: BTreeMap<String, String> =
        [("start", "auto"), ("start_cycle", "ask"), ("plan", "ask"), ("preview", "ask"), ("open", "ask"), ("address", "ask"), ("push", "ask")]
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
    for (k, v) in raw["gates"].as_object().into_iter().flatten() {
        let mut set = |k: String, v: String| -> Result<()> {
            if v != "ask" && v != "auto" {
                bail!("gates.{k} is ask or auto, not {v}");
            }
            gates.insert(k, v);
            Ok(())
        };
        match v {
            Value::String(s) => set(k.clone(), s.clone())?,
            // gates.start = { label = …, cycle = … }: how a Linear issue starts.
            Value::Object(o) => {
                set(k.clone(), o.get("label").and_then(|x| x.as_str()).unwrap_or("auto").to_string())?;
                if let Some(c) = o.get("cycle").and_then(|x| x.as_str()) {
                    set(format!("{k}_cycle"), c.to_string())?;
                }
            }
            _ => continue,
        }
    }
    let linear = match &raw["sources"]["linear"] {
        Value::Object(l) => {
            let teams: BTreeMap<String, String> =
                l.get("teams").and_then(Value::as_object).map(|o| o.iter().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string()))).collect()).unwrap_or_default();
            if teams.is_empty() {
                bail!("sources.linear.teams maps each Linear team to the repo its issues build into, like {{ ENG = \"acme/api\" }}");
            }
            for (team, repo) in &teams {
                if !repos.contains_key(repo) {
                    bail!("sources.linear.teams.{team} is {repo}, which isn't one of Build's repos");
                }
            }
            let label = l.get("label").and_then(Value::as_str).map(str::to_string);
            let cycle = l.get("cycle").and_then(Value::as_bool).unwrap_or(false);
            if label.is_none() && !cycle {
                bail!("sources.linear needs a label, cycle = true, or both");
            }
            Some(LinearSource { teams, label, cycle, skip_label: l.get("skip_label").and_then(Value::as_str).map(str::to_string) })
        }
        _ => None,
    };
    let mut agents = review.map(|r| r.agents.clone()).unwrap_or_default();
    for name in ["codex", "claude"] {
        let a = &raw["agents"][name];
        if a.is_object() {
            agents.insert(
                name.to_string(),
                AgentCfg { program: a["program"].as_str().map(str::to_string), args: a["args"].is_array().then(|| strings(&a["args"])), extra_args: strings(&a["extra_args"]) },
            );
        }
    }
    let path_dirs = if raw["path"].is_array() {
        strings(&raw["path"]).iter().map(|d| crate::paths::expand(d).display().to_string()).collect()
    } else {
        review.map(|r| r.path_dirs.clone()).unwrap_or_default()
    };
    let caches =
        if raw["caches"].is_array() { strings(&raw["caches"]).iter().map(|c| crate::paths::expand(c)).collect() } else { review.map(|r| r.caches.clone()).unwrap_or_default() };
    Ok(Config {
        path,
        repos,
        github_label: raw["sources"]["github"]["label"].as_str().map(str::to_string),
        linear,
        builder: role(&raw["builder"], "builder", "claude")?,
        checker: role(&raw["checker"], "checker", "claude")?,
        prereview: match &raw["prereview"] {
            Value::Object(o) => o
                .get("reviewers")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .enumerate()
                .map(|(i, r)| role(r, &format!("prereview.reviewers[{}]", i + 1), "claude"))
                .collect::<Result<Vec<_>>>()?,
            _ => Vec::new(),
        },
        prereview_loops: raw["prereview"]["loops"].as_i64().unwrap_or(1).max(0),
        gates,
        fix_loops: raw["fix_loops"].as_i64().unwrap_or(2).max(0),
        max_decisions: raw["max_decisions"].as_i64().unwrap_or(3).max(1),
        max_tasks: raw["max_tasks"].as_u64().unwrap_or(2).max(1) as usize,
        // At least 30 seconds, for GitHub's sake; tests ask for less with
        // WH_BUILD_POLL_SECONDS.
        poll_seconds: std::env::var("WH_BUILD_POLL_SECONDS").ok().and_then(|s| s.parse().ok()).unwrap_or_else(|| raw["poll_seconds"].as_u64().unwrap_or(180).max(30)),
        pr_draft: raw["pr"]["draft"].as_bool().unwrap_or(true),
        pr_reviewers: strings(&raw["pr"]["reviewers"]),
        branch_prefix: raw["branch_prefix"].as_str().unwrap_or("wh/").to_string(),
        agents,
        path_dirs,
        caches,
        mcp: raw["wall"]["mcp"].as_bool().unwrap_or(false),
        update: match raw["update"].as_str() {
            None | Some("merge") => "merge".into(),
            Some("rebase") => "rebase".into(),
            Some(o) => bail!("update is merge or rebase, not {o}"),
        },
        raw,
    })
}

impl Config {
    pub fn gate(&self, name: &str) -> &str {
        self.gates.get(name).map(String::as_str).unwrap_or("ask")
    }

    pub fn agent(&self, name: &str) -> AgentCfg {
        self.agents.get(name).cloned().unwrap_or_default()
    }

    /// A role's effort for this task: a level, or the config's function of it.
    pub fn effort(&self, role: &str, view: &Value) -> Result<Option<String>> {
        let r = if role == "build" { &self.builder } else { &self.checker };
        let v = if r.effort.is_null() { self.raw["effort"][role].clone() } else { r.effort.clone() };
        let out = match (v["$fn"].as_str(), &self.path) {
            (Some(f), Some(p)) => lua::call(p, f, view)?,
            _ => v,
        };
        match out {
            Value::Null | Value::Bool(false) => Ok(None),
            Value::String(s) if crate::review::config::LEVELS.contains(&s.as_str()) => Ok(Some(s)),
            other => bail!("effort.{role} returned {other}; return low, medium, high, xhigh, or max"),
        }
    }

    /// A guide for the briefs: inline text, a file to read, or nothing.
    pub fn guide(&self, slot: &str) -> Option<String> {
        let v = self.raw["guides"][slot].as_str()?;
        let p = crate::paths::expand(v);
        if p.is_absolute() && !v.contains('\n') && p.exists() {
            return Some(format!("Read `{}` and follow it.", p.display()));
        }
        Some(v.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn build_shares_reviews_repos_and_has_its_defaults() {
        let rc = crate::review::config::from_value("/x".into(), json!({ "repos": { "o/api": { "checkout": "/src/api", "notes": "Use just." } }, "path": ["/nvm/bin"] })).unwrap();
        let c = from_value(None, json!({}), Some(&rc)).unwrap();
        assert_eq!(c.repos["o/api"].notes, "Use just.");
        assert_eq!((c.builder.agent.as_str(), c.checker.agent.as_str()), ("claude", "claude"));
        assert_eq!((c.gate("plan"), c.gate("open"), c.gate("start")), ("ask", "ask", "auto"));
        assert_eq!((c.fix_loops, c.max_decisions, c.max_tasks, c.branch_prefix.as_str()), (2, 3, 2, "wh/"));
        assert_eq!(c.path_dirs, ["/nvm/bin"]);
        assert!(!c.mcp && c.pr_draft);
        let c = from_value(None, json!({ "repos": { "o/api": { "base": "develop", "plan": false } }, "gates": { "plan": "auto" }, "checker": "codex" }), Some(&rc)).unwrap();
        assert_eq!(c.repos["o/api"].base.as_deref(), Some("develop"));
        assert_eq!(c.repos["o/api"].plan.as_deref(), Some("off"));
        assert_eq!(c.repos["o/api"].checkout, PathBuf::from("/src/api"));
        assert_eq!((c.gate("plan"), c.checker.agent.as_str()), ("auto", "codex"));
        assert!(from_value(None, json!({ "gates": { "plan": "maybe" } }), Some(&rc)).is_err());
    }
}
