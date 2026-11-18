//! The Review protocol's config: `~/.config/wheelhouse/review.lua`, a Lua
//! file that returns a table (docs/review.md has the keys). LuaJIT reads
//! it: settings come out as JSON, and functions (the effort policy, guides)
//! are called per PR. `wh import rex-reviewer` writes it from rex-reviewer's.

use anyhow::{Result, anyhow, bail};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;

use super::lua;
use super::posting::{self, Post};

pub const LEVELS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];
pub const MAX_REVIEWERS: usize = 4;

#[derive(Clone, Debug)]
pub struct Repo {
    pub checkout: PathBuf,
    pub notes: String,
    /// This repo's own `post`, over the config's.
    pub post: Option<Post>,
}

#[derive(Clone, Debug)]
pub struct ReviewerCfg {
    pub name: String,
    pub agent: String,
    pub extra_args: Vec<String>,
    /// 1-based, in the config's order; the first is the primary.
    pub index: usize,
}

#[derive(Clone, Debug, Default)]
pub struct AgentCfg {
    pub program: Option<String>,
    pub args: Option<Vec<String>>,
    pub extra_args: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct Config {
    pub path: PathBuf,
    pub raw: Value,
    pub repos: BTreeMap<String, Repo>,
    pub reviewers: Vec<ReviewerCfg>,
    pub checker: String,
    pub checker_extra_args: Vec<String>,
    /// Who posts a checked review, unless a repo says otherwise.
    pub post: Post,
    pub max_rounds: usize,
    pub poll_seconds: u64,
    pub caches: Vec<PathBuf>,
    pub agents: BTreeMap<String, AgentCfg>,
    /// Folders put first on the agents' PATH (node for a hook, say).
    pub path_dirs: Vec<String>,
}

/// Wheelhouse's config folder: `~/.config/wheelhouse`.
/// Wheelhouse's config (settings, review.lua, build.lua, briefs): `$WH_CONFIG`
/// for a second hub beside the daily one, or `~/.config/wheelhouse`.
pub fn dir() -> PathBuf {
    std::env::var_os("WH_CONFIG").map(PathBuf::from).unwrap_or_else(|| crate::paths::home().join(".config/wheelhouse"))
}

/// Where the config is: `$WH_REVIEW_CONFIG`, or `~/.config/wheelhouse/review.lua`.
pub fn path() -> PathBuf {
    std::env::var_os("WH_REVIEW_CONFIG").map(PathBuf::from).unwrap_or_else(|| dir().join("review.lua"))
}

/// Bring rex-reviewer's config over: its `config.lua` becomes
/// `review.lua`, and its guides move under `~/.config/wheelhouse/guides`,
/// with the config's paths to them rewritten. Its own files stay.
pub fn import_rex_reviewer() -> Result<Vec<PathBuf>> {
    let to = path();
    if to.exists() {
        bail!("{} already exists; Wheelhouse's review config is its own now", crate::paths::tilde(&to.to_string_lossy()));
    }
    let from_dir = std::env::var_os("REX_REVIEWER_CONFIG_DIR").map(PathBuf::from).unwrap_or_else(|| crate::paths::home().join(".config/rex-reviewer"));
    let from = from_dir.join("config.lua");
    let text = std::fs::read_to_string(&from).map_err(|e| anyhow!("reading {}: {e}", from.display()))?;
    let mut written = Vec::new();
    let guides_from = from_dir.join("guides");
    let guides_to = to.parent().unwrap_or(&dir()).join("guides");
    if guides_from.is_dir() {
        std::fs::create_dir_all(&guides_to)?;
        for e in std::fs::read_dir(&guides_from)?.flatten() {
            let dest = guides_to.join(e.file_name());
            if e.path().is_file() && !dest.exists() {
                std::fs::copy(e.path(), &dest)?;
                written.push(dest);
            }
        }
    }
    let home = crate::paths::home().to_string_lossy().into_owned();
    let mut text = text
        .replace("~/.config/rex-reviewer/guides/", "~/.config/wheelhouse/guides/")
        .replace(&format!("{home}/.config/rex-reviewer/guides/"), &format!("{home}/.config/wheelhouse/guides/"));
    text = format!(
        "-- Wheelhouse's review config (docs/review.md), brought over from rex-reviewer's\n-- on {}. Edit it here; rex-reviewer's copy is no longer read.\n{text}",
        crate::time::now()
    );
    if let Some(d) = to.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::write(&to, text)?;
    written.insert(0, to);
    Ok(written)
}

const DEFAULT_CACHES: [&str; 12] = [
    "~/Library/Caches/go-build",
    "~/.cache/go-build",
    "~/go/pkg/mod",
    "~/.npm",
    "~/Library/pnpm/store",
    "~/.local/share/pnpm/store",
    "~/.cache/uv",
    "~/Library/Caches/ms-playwright",
    "~/.cache/ms-playwright",
    "~/.cargo/registry",
    "~/.cargo/git",
    "~/.cargo/advisory-dbs",
];

fn strings(v: &Value) -> Vec<String> {
    v.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

fn is_fn(v: &Value) -> Option<&str> {
    v["$fn"].as_str()
}

pub fn load() -> Result<Config> {
    let path = path();
    if !path.exists() {
        bail!("no review config at {} (docs/review.md; wh import rex-reviewer brings rex-reviewer's over)", crate::paths::tilde(&path.to_string_lossy()));
    }
    let raw = crate::settings::overlaid(lua::dump(&path)?, "review");
    from_value(path, raw)
}

pub fn from_value(path: PathBuf, raw: Value) -> Result<Config> {
    let mut repos = BTreeMap::new();
    for (slug, r) in raw["repos"].as_object().into_iter().flatten() {
        if slug.split('/').count() != 2 {
            bail!("repos: {slug} isn't owner/name");
        }
        let checkout = r["checkout"].as_str().ok_or_else(|| anyhow!("repos[\"{slug}\"].checkout is required"))?;
        let post = posting::parse(&r["post"], &format!("repos[\"{slug}\"].post"))?;
        repos.insert(slug.clone(), Repo { checkout: crate::paths::expand(checkout), notes: r["notes"].as_str().unwrap_or("").to_string(), post });
    }
    if repos.is_empty() {
        bail!("the config lists no repos");
    }

    let list: Vec<Value> = match (&raw["reviewers"], &raw["reviewer"]) {
        (Value::Array(a), _) => a.clone(),
        (_, Value::String(s)) => vec![Value::String(s.clone())],
        // Claude Code by default; Codex is one line away in review.lua.
        _ => vec![Value::String("claude".into())],
    };
    if list.is_empty() || list.len() > MAX_REVIEWERS {
        bail!("reviewers: one to {MAX_REVIEWERS}");
    }
    let mut reviewers: Vec<ReviewerCfg> = Vec::new();
    for (i, r) in list.iter().enumerate() {
        let (name, agent, extra) = match r {
            Value::String(s) => (s.clone(), s.clone(), Vec::new()),
            Value::Object(_) => {
                let agent = r["agent"].as_str().or(r["name"].as_str()).unwrap_or("codex").to_string();
                let name = r["name"].as_str().map(str::to_string).unwrap_or_else(|| agent.clone());
                (name, agent, strings(&r["extra_args"]))
            }
            _ => bail!("reviewers[{}] must be an agent name or a table", i + 1),
        };
        if !matches!(agent.as_str(), "codex" | "claude") {
            bail!("reviewers[{}]: agent must be codex or claude, not {agent}", i + 1);
        }
        let ok_name = name.len() <= 12 && name.starts_with(|c: char| c.is_ascii_lowercase()) && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        if !ok_name || name == "check" {
            bail!("reviewers[{}]: name {name:?} must be a-z, 0-9 and -, start with a letter, be at most 12 long, and not be \"check\"", i + 1);
        }
        if reviewers.iter().any(|x| x.name == name) {
            bail!("reviewers: {name} is listed twice");
        }
        reviewers.push(ReviewerCfg { name, agent, extra_args: extra, index: i + 1 });
    }

    let (checker, checker_extra_args) = match &raw["checker"] {
        Value::String(s) => (s.clone(), Vec::new()),
        Value::Object(_) => (raw["checker"]["agent"].as_str().unwrap_or("claude").to_string(), strings(&raw["checker"]["extra_args"])),
        _ => ("claude".to_string(), Vec::new()),
    };
    if !matches!(checker.as_str(), "codex" | "claude") {
        bail!("checker must be codex or claude, not {checker}");
    }
    let post = posting::parse(&raw["post"], "post")?.unwrap_or(Post::Ask);
    let mut caches: Vec<String> = if raw["caches"].is_array() { strings(&raw["caches"]) } else { DEFAULT_CACHES.iter().map(|s| s.to_string()).collect() };
    caches.extend(strings(&raw["extra_caches"]));
    let mut agents = BTreeMap::new();
    for name in ["codex", "claude"] {
        let a = &raw["agents"][name];
        agents.insert(
            name.to_string(),
            AgentCfg { program: a["program"].as_str().map(str::to_string), args: a["args"].is_array().then(|| strings(&a["args"])), extra_args: strings(&a["extra_args"]) },
        );
    }
    for slot in raw["guides"].as_object().into_iter().flatten().map(|(k, _)| k) {
        if !matches!(slot.as_str(), "review" | "style" | "merge_risk" | "check") {
            bail!("guides.{slot}: the slots are review, style, merge_risk and check");
        }
    }
    Ok(Config {
        path,
        repos,
        reviewers,
        checker,
        checker_extra_args,
        post,
        max_rounds: raw["max_rounds"].as_u64().filter(|n| *n >= 1).unwrap_or(3) as usize,
        poll_seconds: raw["poll_seconds"].as_u64().unwrap_or(180).max(30),
        caches: caches.iter().map(|c| crate::paths::expand(c)).collect(),
        agents,
        path_dirs: strings(&raw["path"]).iter().map(|d| crate::paths::expand(d).display().to_string()).collect(),
        raw,
    })
}

impl Config {
    pub fn repo_list(&self) -> Vec<String> {
        self.repos.keys().cloned().collect()
    }

    pub fn reviewer(&self, name: &str) -> Option<&ReviewerCfg> {
        self.reviewers.iter().find(|r| r.name == name)
    }

    /// Who posts a checked review in `repo`: its own `post`, or the config's.
    pub fn post_for(&self, repo: &str) -> &Post {
        self.repos.get(repo).and_then(|r| r.post.as_ref()).unwrap_or(&self.post)
    }

    pub fn agent(&self, name: &str) -> AgentCfg {
        self.agents.get(name).cloned().unwrap_or_default()
    }

    /// A value from the config, calling it with `arg` if it's a function.
    fn value(&self, v: &Value, arg: &Value) -> Result<Value> {
        match is_fn(v) {
            Some(p) => lua::call(&self.path, p, arg),
            None => Ok(v.clone()),
        }
    }

    /// The effort for `slot` (`review` or `check`) on this PR: the reviewer's
    /// own, the repo's, then the global one. `None`: the agent's own setting.
    pub fn effort(&self, repo: &str, slot: &str, view: &Value, reviewer: Option<&str>) -> Result<Option<String>> {
        let rv = reviewer.and_then(|n| self.raw["reviewers"].as_array().and_then(|a| a.iter().find(|r| r["name"].as_str().or(r["agent"].as_str()) == Some(n))));
        let candidates = [
            if slot == "review" { rv.map(|r| r["effort"].clone()) } else { Some(self.raw["checker"]["effort"].clone()) },
            Some(self.raw["repos"][repo]["effort"][slot].clone()),
            Some(self.raw["effort"][slot].clone()),
        ];
        let Some(v) = candidates.into_iter().flatten().find(|v| !v.is_null()) else { return Ok(None) };
        let out = self.value(&v, view)?;
        match out {
            Value::Null | Value::Bool(false) => Ok(None),
            Value::String(s) if LEVELS.contains(&s.as_str()) => Ok(Some(s)),
            other => bail!("effort.{slot} returned {other} for {repo}#{}; return low, medium, high, xhigh, or max, or nil for the agent's own setting", view["pr"]),
        }
    }

    /// The guide for `slot`, as instructions for the brief, or `None`.
    pub fn guide(&self, repo: &str, slot: &str, reviewer: Option<&str>, entry: &Value) -> Result<Option<String>> {
        let rv = reviewer.and_then(|n| self.raw["reviewers"].as_array().and_then(|a| a.iter().find(|r| r["name"].as_str().or(r["agent"].as_str()) == Some(n))));
        let default = match slot {
            "style" | "merge_risk" => Value::String("bundled".into()),
            _ => Value::Bool(false),
        };
        let candidates = [
            if slot == "review" { rv.map(|r| r["guides"][slot].clone()) } else { None },
            Some(self.raw["repos"][repo]["guides"][slot].clone()),
            Some(self.raw["guides"][slot].clone()),
        ];
        let v = candidates.into_iter().flatten().find(|v| !v.is_null()).unwrap_or(default);
        let v = self.value(&v, entry)?;
        let s = match v {
            Value::Null | Value::Bool(false) => return Ok(None),
            Value::String(s) if s.is_empty() => return Ok(None),
            Value::String(s) => s,
            _ => bail!("guides.{slot} for {repo} must be a path, instructions, a function, or false"),
        };
        if s == "bundled" {
            let path = super::bundled_guide(slot).ok_or_else(|| anyhow!("guides.{slot}: there's no bundled guide for {slot}"))?;
            return Ok(Some(format!("Read `{}` and follow it.", path.display())));
        }
        let expanded = crate::paths::expand(&s);
        if expanded.is_absolute() && !s.contains('\n') {
            if !expanded.exists() {
                bail!("guides.{slot} for {repo}: {} doesn't exist", expanded.display());
            }
            return Ok(Some(format!("Read `{}` and follow it.", expanded.display())));
        }
        Ok(Some(s))
    }

    pub fn merge_risk_enabled(&self, repo: &str, entry: &Value) -> bool {
        self.guide(repo, "merge_risk", None, entry).ok().flatten().is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    pub fn sample() -> Value {
        json!({
            "repos": { "o/api": { "checkout": "~/git/api", "notes": "Run `just test`." } },
            "reviewers": [{ "name": "claude", "agent": "claude" }, { "name": "claude-2", "agent": "claude" }],
            "checker": "claude", "post": "auto", "max_rounds": 2,
            "guides": { "review": "Look hard at retries.", "check": false },
            "effort": { "review": "high" },
        })
    }

    #[test]
    fn a_config_reads_with_rex_reviewers_defaults() {
        let c = from_value("/x/config.lua".into(), sample()).unwrap();
        assert_eq!(c.repo_list(), ["o/api"]);
        assert!(c.repos["o/api"].checkout.ends_with("git/api"));
        assert_eq!(c.reviewers.iter().map(|r| (r.name.as_str(), r.index)).collect::<Vec<_>>(), [("claude", 1), ("claude-2", 2)]);
        assert_eq!((c.checker.as_str(), c.post.kind(), c.max_rounds, c.poll_seconds), ("claude", "auto", 2, 180));
        assert_eq!(c.caches.len(), 12);
        let view = json!({ "repo": "o/api", "pr": 1 });
        assert_eq!(c.effort("o/api", "review", &view, Some("claude")).unwrap().as_deref(), Some("high"));
        assert_eq!(c.effort("o/api", "check", &view, None).unwrap(), None);
        assert_eq!(c.guide("o/api", "review", Some("claude"), &json!({})).unwrap().as_deref(), Some("Look hard at retries."));
        assert_eq!(c.guide("o/api", "check", None, &json!({})).unwrap(), None);
        assert!(c.guide("o/api", "style", None, &json!({})).unwrap().unwrap().starts_with("Read `"));
        assert!(c.merge_risk_enabled("o/api", &json!({})));
    }

    #[test]
    fn a_repo_can_post_its_own_way() {
        let mut v = sample();
        v["post"] = json!("ask");
        v["repos"]["o/web"] = json!({ "checkout": "~/git/web", "post": { "auto": { "verdict": "approve", "max_lines": 200 } } });
        let c = from_value("/x".into(), v).unwrap();
        assert_eq!(c.post_for("o/api"), &Post::Ask);
        assert_eq!(c.post_for("o/web").kind(), "rule");
        assert_eq!(c.post_for("o/elsewhere"), &Post::Ask);
        let mut v = sample();
        v.as_object_mut().unwrap().remove("post");
        assert_eq!(from_value("/x".into(), v).unwrap().post, Post::Ask, "ask by default");
    }

    #[test]
    fn bad_configs_say_whats_wrong() {
        let mut v = sample();
        v["reviewers"] = json!([{ "name": "check", "agent": "claude" }]);
        assert!(from_value("/x".into(), v).unwrap_err().to_string().contains("not be \"check\""));
        let mut v = sample();
        v["reviewers"] = json!(["claude", "claude"]);
        assert!(from_value("/x".into(), v).unwrap_err().to_string().contains("twice"));
        let mut v = sample();
        v["post"] = json!("sometimes");
        assert!(from_value("/x".into(), v).is_err());
        let mut v = sample();
        v["repos"]["o/api"]["post"] = json!({ "auto": { "merge_risk": "huge" } });
        assert_eq!(from_value("/x".into(), v).unwrap_err().to_string(), "repos[\"o/api\"].post.auto.merge_risk must be low or medium, not \"huge\"");
        let mut v = sample();
        v["effort"]["review"] = json!("huge");
        let c = from_value("/x".into(), v).unwrap();
        assert!(c.effort("o/api", "review", &json!({ "pr": 3 }), None).unwrap_err().to_string().contains("returned \"huge\""));
    }
}
