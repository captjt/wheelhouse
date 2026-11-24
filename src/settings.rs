//! Settings you change in the apps (Settings, on the web and in
//! Wheelhouse.app).
//!
//! - **Values** go in `~/.config/wheelhouse/settings.json`, shaped like the
//!   Lua configs (`{ "build": {…}, "review": {…} }`) and laid over them as
//!   they're read: a value set here wins over `build.lua` or `review.lua`,
//!   and `null` removes one. Every change is checked by the same code that
//!   reads the Lua, so a change that wouldn't load is refused, not written.
//!   Lua stays for what's code: effort functions, agents' programs, PATH.
//! - **Guides and briefs** are Markdown. A built-in one is replaced by a
//!   file of the same name under `~/.config/wheelhouse/` (`briefs/review/`,
//!   `briefs/build/`, `guides/`); resetting moves that file aside. Your own
//!   guides live in `guides/`, and a repo or a slot points at them.
//!
//! Whatever a save replaces is kept in `~/.config/wheelhouse/.history/`,
//! with the time, so nothing you wrote is lost.

use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};

pub fn dir() -> PathBuf {
    crate::review::config::dir()
}

/// `$WH_SETTINGS`, or `~/.config/wheelhouse/settings.json`.
pub fn path() -> PathBuf {
    std::env::var_os("WH_SETTINGS").map(PathBuf::from).unwrap_or_else(|| dir().join("settings.json"))
}

pub fn mtime() -> Option<std::time::SystemTime> {
    std::fs::metadata(path()).and_then(|m| m.modified()).ok()
}

/// What Settings set: `{}` when nothing has been.
pub fn load() -> Result<Value> {
    match std::fs::read_to_string(path()) {
        Ok(s) => {
            let v: Value = serde_json::from_str(&s).with_context(|| format!("{} isn't JSON", crate::paths::tilde(&path().to_string_lossy())))?;
            if !v.is_object() {
                bail!("{} must be an object", crate::paths::tilde(&path().to_string_lossy()));
            }
            Ok(v)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(json!({})),
        Err(e) => Err(e.into()),
    }
}

/// `over` laid on `base`: objects merge key by key, `null` removes a key,
/// anything else replaces. A Lua function (`{"$fn": …}`) is replaced whole.
pub fn merge(base: &mut Value, over: &Value) {
    match (base, over) {
        (Value::Object(b), Value::Object(o)) if !b.contains_key("$fn") => {
            for (k, v) in o {
                if v.is_null() {
                    b.remove(k);
                } else {
                    merge(b.entry(k.clone()).or_insert(Value::Null), v);
                }
            }
        }
        (b, o) => *b = strip_nulls(o),
    }
}

fn strip_nulls(v: &Value) -> Value {
    match v {
        Value::Object(o) => Value::Object(o.iter().filter(|(_, v)| !v.is_null()).map(|(k, v)| (k.clone(), strip_nulls(v))).collect()),
        other => other.clone(),
    }
}

/// Empty objects left by removals, gone.
fn prune(v: &mut Value) {
    if let Value::Object(o) = v {
        for x in o.values_mut() {
            prune(x);
        }
        o.retain(|_, x| !(x.is_object() && x.as_object().is_some_and(Map::is_empty)));
    }
}

/// A config's raw table with Settings laid over it (`part` is `build` or
/// `review`). A broken settings.json is skipped, and its error shown in
/// Settings, so it can't stop the hub.
pub fn overlaid(mut raw: Value, part: &str) -> Value {
    if let Ok(s) = load() {
        lay(&mut raw, &s[part]);
    }
    raw
}

/// One part of settings.json over a config's raw table. Nothing set leaves
/// the table as it is.
fn lay(raw: &mut Value, part: &Value) {
    let Some(o) = part.as_object().filter(|o| !o.is_empty()) else { return };
    if !raw.is_object() {
        *raw = json!({});
    }
    whole(raw, part);
    merge(raw, &Value::Object(o.clone()));
}

/// Review's posting rule (`post`, and each repo's) is replaced whole, not
/// key by key: a rule's conditions only make sense together.
fn whole(base: &mut Value, over: &Value) {
    if over.get("post").is_some()
        && let Some(b) = base.as_object_mut()
    {
        b.remove("post");
    }
    for (slug, r) in over["repos"].as_object().into_iter().flatten() {
        if r.get("post").is_some()
            && let Some(b) = base.get_mut("repos").and_then(|x| x.get_mut(slug)).and_then(Value::as_object_mut)
        {
            b.remove("post");
        }
    }
}

/// What Settings may change. Lua keeps the rest: agents' programs and
/// arguments, PATH, caches, the wall, effort functions.
const ALLOWED: [(&str, &[&str]); 2] = [
    (
        "build",
        &[
            "gates", "builder", "checker", "fix_loops", "max_tasks", "update", "pr.draft", "pr.reviewers", "prereview", "repos.*.notes", "repos.*.base", "repos.*.plan",
            "repos.*.checks", "repos.*.checkout", "guides.plan_format", "guides.pr_style", "sources.github.label", "sources.linear",
        ],
    ),
    ("review", &["reviewers", "checker", "max_rounds", "post", "repos.*.notes", "repos.*.checkout", "repos.*.guides", "repos.*.post", "guides"]),
];

fn allowed(part: &str, at: &[String]) -> bool {
    let Some((_, pats)) = ALLOWED.iter().find(|(p, _)| *p == part) else { return false };
    pats.iter().any(|pat| {
        let segs: Vec<&str> = pat.split('.').collect();
        segs.len() <= at.len() && segs.iter().zip(at).all(|(s, a)| *s == "*" || s == a)
    })
}

/// Every path a patch sets or removes that Settings may not change.
fn refused(part: &str, v: &Value, at: &mut Vec<String>, out: &mut Vec<String>) {
    if !at.is_empty() && allowed(part, at) {
        return;
    }
    match v {
        Value::Object(o) if !o.is_empty() => {
            for (k, x) in o {
                at.push(k.clone());
                refused(part, x, at, out);
                at.pop();
            }
        }
        _ => out.push(format!("{part}.{}", at.join("."))),
    }
}

/// The Lua configs' raw tables, as read now (`null` without the file).
fn lua_raw() -> Result<(Value, Value)> {
    let rp = crate::review::config::path();
    let bp = crate::build::config::path();
    let r = if rp.exists() { crate::review::lua::dump(&rp)? } else { Value::Null };
    let b = if bp.exists() { crate::review::lua::dump(&bp)? } else { json!({}) };
    Ok((r, b))
}

/// Whether `s` (all of settings.json) loads with the Lua under it.
pub fn check(s: &Value) -> Result<()> {
    check_runtime(&s["runtime"])?;
    let (rraw, braw) = lua_raw()?;
    let review = if rraw.is_null() {
        if s["review"].as_object().is_some_and(|o| !o.is_empty()) {
            bail!("Review isn't set up (there's no review.lua), so its settings can't change yet");
        }
        None
    } else {
        let mut r = rraw;
        lay(&mut r, &s["review"]);
        Some(crate::review::config::from_value(crate::review::config::path(), r).context("Review's settings")?)
    };
    let mut b = braw;
    lay(&mut b, &s["build"]);
    let bp = crate::build::config::path();
    crate::build::config::from_value(bp.exists().then_some(bp), b, review.as_ref()).context("Build's settings")?;
    Ok(())
}

/// `runtime`: where new agents open, `rex` or `herdr` (docs/runtimes.md).
fn check_runtime(v: &Value) -> Result<()> {
    match v {
        Value::Null => Ok(()),
        Value::String(s) if crate::runtime::Kind::parse(s).is_some() => Ok(()),
        other => {
            let names: Vec<&str> = crate::runtime::ALL.iter().map(|k| k.name()).collect();
            bail!("runtime is one of {}, not {other}", names.join(", "))
        }
    }
}

/// Review's posting rules in a patch, checked as sent: pruning empty tables
/// later would otherwise turn `{ auto = {} }` into nothing set.
fn check_posts(patch: &Value) -> Result<()> {
    let r = &patch["review"];
    let posts =
        std::iter::once(("post".to_string(), &r["post"])).chain(r["repos"].as_object().into_iter().flatten().map(|(slug, x)| (format!("repos[\"{slug}\"].post"), &x["post"])));
    for (at, v) in posts {
        crate::review::posting::parse(v, &at).context("Review's settings")?;
    }
    Ok(())
}

/// Lay `patch` (`{ "build": {…}, "review": {…} }`) over settings.json, check
/// it loads, and write it. Refuses what Settings may not change.
pub fn change(patch: &Value) -> Result<Value> {
    let Some(p) = patch.as_object() else { bail!("a change is an object: {{ \"build\": {{…}}, \"review\": {{…}} }}") };
    let mut bad = Vec::new();
    for (part, v) in p {
        if part == "runtime" {
            check_runtime(v)?;
            continue;
        }
        if !matches!(part.as_str(), "build" | "review") {
            bail!("settings are build, review or runtime, not {part}");
        }
        refused(part, v, &mut Vec::new(), &mut bad);
    }
    if !bad.is_empty() {
        bail!("Settings doesn't change {}: that stays in the Lua config", bad.join(", "));
    }
    // A checkout you set must be one.
    for part in ["build", "review"] {
        for (slug, r) in patch[part]["repos"].as_object().into_iter().flatten() {
            if let Some(c) = r["checkout"].as_str() {
                let p = crate::paths::expand(c);
                if !p.join(".git").exists() {
                    bail!("{slug}'s checkout, {c}, isn't a git checkout");
                }
            }
        }
    }
    check_posts(patch)?;
    let mut s = load().unwrap_or_else(|_| json!({}));
    if let (Some(r), Some(over)) = (s.get_mut("review"), patch.get("review")) {
        whole(r, over);
    }
    merge(&mut s, patch);
    prune(&mut s);
    check(&s)?;
    keep(&path())?;
    write(&path(), &(serde_json::to_string_pretty(&s)? + "\n"))?;
    Ok(s)
}

/// Keep a file's current text in `.history/` before it's replaced.
fn keep(p: &Path) -> Result<()> {
    if !p.exists() {
        return Ok(());
    }
    let rel = p.strip_prefix(dir()).unwrap_or(p.file_name().map(Path::new).unwrap_or(p));
    let stamp = crate::time::now_ms().replace([':', '.'], "-");
    let to = dir().join(".history").join(format!("{}.{stamp}", rel.display()));
    if let Some(d) = to.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::copy(p, &to).with_context(|| format!("keeping {}", p.display()))?;
    Ok(())
}

fn write(p: &Path, text: &str) -> Result<()> {
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d)?;
    }
    let tmp = p.with_extension("wh-tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, p)?;
    Ok(())
}

// ---- Guides and briefs ----

/// A guide or brief you can read and change.
#[derive(Clone, Debug)]
pub struct Doc {
    /// `review/brief/round-1.md`, `build/brief/plan.md`, `guide/pr-style.md`.
    pub id: String,
    /// `brief` or `guide`.
    pub kind: &'static str,
    /// `review`, `build`, or `` for your own guides.
    pub protocol: &'static str,
    pub title: String,
    pub about: String,
    pub builtin: Option<&'static str>,
    /// Where yours is (or would be).
    pub path: PathBuf,
}

const BUILT_IN_GUIDES: [(&str, &str, &str, &str); 4] = [
    (
        "review-style.md",
        "Review style",
        "How a review reads: its verdict, its points, its tone. Review's `style` slot, unless review.lua points it elsewhere.",
        include_str!("../guides/review-style.md"),
    ),
    ("merge-risk.md", "Merge-risk rubric", "How the risk of merging a PR is judged. Review's `merge_risk` slot.", include_str!("../guides/merge-risk.md")),
    ("plan-format.md", "Plan format", "The shape of a Build plan: what it says before any code is written.", include_str!("../guides/plan-format.md")),
    ("pr-style.md", "PR style", "How a Build's PR title and description are written.", include_str!("../guides/pr-style.md")),
];

fn about(protocol: &str, name: &str) -> (String, String) {
    let (t, a) = match (protocol, name) {
        ("review", "ground-rules.md") => ("Ground rules", "What every reviewer is told first: who it works for, the wall, and what it never does."),
        ("review", "round-1.md") => ("First round", "A reviewer's first look at a PR."),
        ("review", "round-n.md") => ("Later rounds", "A reviewer's next round, after the author pushed."),
        ("review", "delta.md") => ("The head moved", "When the author pushes in the middle of a round."),
        ("review", "check.md") => ("The checker", "The checker that merges the reviewers' drafts into one review."),
        ("review", "merge-risk.md") => ("Merge risk", "The merge-risk assessment, run beside the review."),
        ("review", "review-draft.md") => ("The draft's format", "The shape of a reviewer's draft (`review.json`)."),
        ("review", "parts/merge-drafts.md") => ("Part: merging drafts", "How the checker merges several reviewers' drafts."),
        ("review", "parts/merge-risk-add.md") => ("Part: adding merge risk", "How the merge-risk result joins the review."),
        ("review", "parts/merge-risk-start.md") => ("Part: starting merge risk", "How the merge-risk subagent is started."),
        ("review", "parts/parallel.md") => ("Part: several reviewers", "What a reviewer is told when others review the same PR."),
        ("review", "parts/post-ask.md") => ("Part: posting, when you're asked", "The last step when you approve each post."),
        ("review", "parts/post-auto.md") => ("Part: posting on its own", "The last step when reviews post themselves."),
        ("review", "parts/review-default.md") => ("Part: what to look for", "What a reviewer looks for, without a review guide."),
        ("build", "ground-rules.md") => ("Ground rules", "What every Build agent is told first."),
        ("build", "plan.md") => ("The plan", "The builder's brief for writing its plan."),
        ("build", "build.md") => ("Building", "The builder's brief once you approve the plan."),
        ("build", "check.md") => ("The checker", "The checker's brief: judge the branch against the intent and the plan."),
        ("build", "address.md") => ("Addressing review", "The builder's brief for a round of PR comments."),
        ("build", "prereview.md") => ("Pre-review", "The blind reviewers' brief, before the PR opens."),
        _ => ("", ""),
    };
    (if t.is_empty() { name.trim_end_matches(".md").to_string() } else { t.to_string() }, a.to_string())
}

/// Every guide and brief, built in or yours.
pub fn docs() -> Vec<Doc> {
    let d = dir();
    let mut out = Vec::new();
    for (name, title, about, text) in BUILT_IN_GUIDES {
        out.push(Doc {
            id: format!("guide/{name}"),
            kind: "guide",
            protocol: "",
            title: title.into(),
            about: about.into(),
            builtin: Some(text),
            path: d.join("guides").join(name),
        });
    }
    let mut own: Vec<PathBuf> =
        std::fs::read_dir(d.join("guides")).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "md")).collect();
    own.sort();
    for p in own {
        let name = p.file_name().unwrap_or_default().to_string_lossy().to_string();
        if BUILT_IN_GUIDES.iter().any(|(n, ..)| *n == name) {
            continue;
        }
        let title = std::fs::read_to_string(&p)
            .ok()
            .and_then(|t| t.lines().find_map(|l| l.strip_prefix("# ").map(str::to_string)))
            .unwrap_or_else(|| name.trim_end_matches(".md").to_string());
        out.push(Doc { id: format!("guide/{name}"), kind: "guide", protocol: "", title, about: String::new(), builtin: None, path: p });
    }
    for (name, text) in crate::build::steps::BRIEFS {
        let (title, about) = about("build", name);
        out.push(Doc { id: format!("build/brief/{name}"), kind: "brief", protocol: "build", title, about, builtin: Some(text), path: d.join("briefs/build").join(name) });
    }
    for (name, text) in crate::review::briefs::BUILT_IN {
        let (title, about) = about("review", name);
        out.push(Doc { id: format!("review/brief/{name}"), kind: "brief", protocol: "review", title, about, builtin: Some(text), path: d.join("briefs/review").join(name) });
    }
    out
}

pub fn doc(id: &str) -> Result<Doc> {
    docs().into_iter().find(|d| d.id == id).ok_or_else(|| anyhow!("there's no guide or brief {id}"))
}

/// A built-in guide's text, or yours when you've replaced it.
pub fn guide_text(name: &str) -> String {
    let (_, _, _, builtin) = BUILT_IN_GUIDES.iter().find(|(n, ..)| *n == name).copied().unwrap_or(("", "", "", ""));
    std::fs::read_to_string(dir().join("guides").join(name)).unwrap_or_else(|_| builtin.to_string())
}

/// Where a guide is used: the config keys that name its file.
pub fn used_by(p: &Path) -> Vec<String> {
    match lua_raw() {
        Ok((r, b)) => used_in(p, &overlaid(r, "review"), &overlaid(b, "build")),
        Err(_) => Vec::new(),
    }
}

/// `used_by`, in tables already read.
pub fn used_in(p: &Path, review: &Value, build: &Value) -> Vec<String> {
    let full = p.display().to_string();
    let short = crate::paths::tilde(&full);
    let mut out = Vec::new();
    fn walk(v: &Value, at: &mut Vec<String>, full: &str, short: &str, part: &str, out: &mut Vec<String>) {
        match v {
            Value::String(s) if s == full || s == short => out.push(format!("{part}: {}", at.join("."))),
            Value::Object(o) => {
                for (k, x) in o {
                    at.push(k.clone());
                    walk(x, at, full, short, part, out);
                    at.pop();
                }
            }
            Value::Array(a) => {
                for (i, x) in a.iter().enumerate() {
                    at.push(format!("{}", i + 1));
                    walk(x, at, full, short, part, out);
                    at.pop();
                }
            }
            _ => {}
        }
    }
    walk(review, &mut Vec::new(), &full, &short, "review", &mut out);
    walk(build, &mut Vec::new(), &full, &short, "build", &mut out);
    out
}

impl Doc {
    pub fn overridden(&self) -> bool {
        self.builtin.is_some() && self.path.exists()
    }

    pub fn text(&self) -> String {
        std::fs::read_to_string(&self.path).ok().or(self.builtin.map(str::to_string)).unwrap_or_default()
    }

    pub fn json(&self, with_text: bool) -> Value {
        self.json_in(with_text, None)
    }

    /// With the configs' tables already read, for a list of them.
    pub fn json_in(&self, with_text: bool, raws: Option<(&Value, &Value)>) -> Value {
        let used = match (self.kind, raws) {
            ("guide", Some((r, b))) => used_in(&self.path, r, b),
            ("guide", None) => used_by(&self.path),
            _ => Vec::new(),
        };
        let mut v = json!({
            "id": self.id, "kind": self.kind, "protocol": self.protocol, "title": self.title, "about": self.about,
            "builtin": self.builtin.is_some(), "overridden": self.overridden(), "path": crate::paths::tilde(&self.path.display().to_string()),
            "used_by": used,
        });
        if with_text {
            v["text"] = json!(self.text());
            v["original"] = json!(self.builtin);
        }
        v
    }
}

/// Write a guide or brief: yours replaces the built-in one from now on.
pub fn save_doc(id: &str, text: &str) -> Result<Doc> {
    let d = match doc(id) {
        Ok(d) => d,
        Err(_) => {
            // A new guide of your own.
            let name = id.strip_prefix("guide/").ok_or_else(|| anyhow!("there's no guide or brief {id}"))?;
            let ok = name.ends_with(".md")
                && name.len() <= 64
                && name.trim_end_matches(".md").chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
                && !name.starts_with('-');
            if !ok {
                bail!("a guide's name is lowercase letters, digits and dashes, ending in .md, like `api-review.md`");
            }
            Doc { id: id.into(), kind: "guide", protocol: "", title: name.into(), about: String::new(), builtin: None, path: dir().join("guides").join(name) }
        }
    };
    if text.trim().is_empty() {
        bail!("it's empty: to go back to the built-in one, reset it");
    }
    if text.len() > 256 * 1024 {
        bail!("that's over 256 KB");
    }
    keep(&d.path)?;
    write(&d.path, text)?;
    doc(id)
}

/// Back to the built-in text (yours is kept in `.history/`), or, for a
/// guide of your own, set aside, unless a config still names it.
pub fn reset_doc(id: &str) -> Result<()> {
    let d = doc(id)?;
    if !d.path.exists() {
        bail!("{} is already the built-in one", d.title);
    }
    if d.builtin.is_none() {
        let used = used_by(&d.path);
        if !used.is_empty() {
            bail!("{} is still used by {}: point those elsewhere first", d.title, used.join(", "));
        }
    }
    keep(&d.path)?;
    std::fs::remove_file(&d.path)?;
    Ok(())
}

// ---- What Settings shows ----

fn app(agent: &str) -> &'static str {
    match agent {
        "codex" => "Codex",
        "agy" => "Antigravity",
        _ => "Claude Code",
    }
}

fn and(items: &[String]) -> String {
    match items.len() {
        0 => String::new(),
        1 => items[0].clone(),
        2 => format!("{} and {}", items[0], items[1]),
        n => format!("{}, and {}", items[..n - 1].join(", "), items[n - 1]),
    }
}

fn or(items: &[String]) -> String {
    match items.len() {
        0 => String::new(),
        1 => items[0].clone(),
        n => format!("{} or {}", items[..n - 1].join(", "), items[n - 1]),
    }
}

/// What each gate means, asked and not.
pub const GATES: [(&str, &str, &str, &str); 7] = [
    ("start", "Start", "Ask before a labeled issue starts a Build", "Start a Build when an issue gets the label"),
    ("start_cycle", "Active cycle", "Ask whether to Build an issue that enters your active cycle", "Build issues that enter your active cycle"),
    ("plan", "Plan", "Ask you to approve the plan before building", "Build as soon as the plan is written"),
    ("preview", "Preview", "Ask you to look at a running preview before the PR opens", "Skip the preview"),
    ("open", "Open PR", "Ask before opening the PR", "Open the PR when the checks pass"),
    ("address", "Address", "Ask before addressing new review comments", "Address new review comments as they come"),
    ("push", "Push", "Ask before pushing a round and posting its replies", "Push rounds and post replies when they're checked"),
];

/// The standing orders, as sentences: what starts a protocol, and how it runs.
pub fn orders(core: &crate::hub::Core, b: Option<&crate::build::config::Config>, r: Option<&crate::review::config::Config>) -> Vec<Value> {
    let mut out = Vec::new();
    if let Some(r) = r {
        let repos: Vec<String> = r.repos.keys().cloned().collect();
        let reviewers: Vec<String> = {
            let mut v: Vec<String> = Vec::new();
            for x in &r.reviewers {
                let a = app(&x.agent).to_string();
                if !v.contains(&a) {
                    v.push(a);
                }
            }
            v
        };
        let post = crate::review::post_for(core, r, None);
        let on = crate::review::enabled(core);
        out.push(json!({
            "protocol": "review", "on": on, "paused": on && crate::review::paused(core),
            "text": format!(
                "When a PR in {} requests you, Review it with {}{}, check with {}, and {}.",
                or(&repos),
                and(&reviewers),
                if r.reviewers.len() > reviewers.len() { format!(" ({} reviewers)", r.reviewers.len()) } else { String::new() },
                app(&r.checker),
                post.order()
            ),
        }));
        // A repo that posts its own way, unless `wh review post-mode` set one way for all.
        if crate::review::post_override(core).is_none() {
            for (slug, p) in r.repos.iter().filter_map(|(slug, x)| x.post.as_ref().filter(|p| **p != r.post).map(|p| (slug, p))) {
                out.push(json!({ "protocol": "review", "on": on, "paused": on && crate::review::paused(core), "text": format!("In {slug}, a checked review {}.", p.outcome()) }));
            }
        }
    }
    if let Some(b) = b {
        let asked: Vec<String> = ["plan", "preview", "open", "address", "push"].iter().filter(|g| b.gate(g) == "ask").map(|g| g.to_string()).collect();
        let how = format!(
            "Build it with {}, check with {}{}, and {}",
            app(&b.builder.agent),
            app(&b.checker.agent),
            if b.prereview.is_empty() {
                String::new()
            } else {
                format!(", have {} review it blind first", and(&b.prereview.iter().map(|p| app(&p.agent).to_string()).collect::<Vec<_>>()))
            },
            if asked.is_empty() { "never stop to ask you".to_string() } else { format!("ask you at {}", and(&asked)) }
        );
        let paused = core.store.meta_get("build.paused").ok().flatten().as_deref() == Some("1");
        if let Some(l) = &b.github_label {
            let repos: Vec<String> = b.repos.keys().cloned().collect();
            out.push(json!({ "protocol": "build", "on": true, "paused": paused, "text": format!("When an issue in {} assigned to you is labeled `{l}`, {}.", or(&repos), how) }));
        }
        if let Some(l) = &b.linear {
            let teams: Vec<String> = l.teams.iter().map(|(t, r)| format!("{t} (into {r})")).collect();
            if let Some(label) = &l.label {
                let start = if b.gate("start") == "ask" { "ask whether to start it, then " } else { "" };
                out.push(json!({ "protocol": "build", "on": true, "paused": paused, "text": format!("When a {} issue assigned to you is labeled `{label}`, {start}{}.", or(&teams), how.replacen("Build", "build", 1)) }));
            }
            if l.cycle {
                let start = if b.gate("start_cycle") == "ask" { "ask you whether to start it" } else { "Build it" };
                out.push(
                    json!({ "protocol": "build", "on": true, "paused": paused, "text": format!("When a {} issue assigned to you enters your active cycle, {start}.", or(&teams)) }),
                );
            }
        }
        out.push(json!({ "protocol": "build", "on": true, "paused": paused, "text": format!("When you start a Build, {}.", how.replacen("Build", "build", 1)) }));
        for (slug, repo) in &b.repos {
            if repo.checks.is_empty() {
                continue;
            }
            let req: Vec<String> = repo.checks.iter().filter(|c| c.required).map(|c| format!("`{}`", c.name)).collect();
            let opt: Vec<String> = repo.checks.iter().filter(|c| !c.required).map(|c| format!("`{}`", c.name)).collect();
            let mut text = format!(
                "Before a PR in {slug} opens or a round pushes, run {} at that commit myself",
                and(&repo.checks.iter().map(|c| format!("`{}`", c.name)).collect::<Vec<_>>())
            );
            if !req.is_empty() {
                text.push_str(&format!(", and hold it until {} pass{}", and(&req), if req.len() == 1 { "es" } else { "" }));
            }
            if !opt.is_empty() {
                text.push_str(&format!(" ({} only inform)", and(&opt)));
            }
            text.push('.');
            out.push(json!({ "protocol": "verify", "on": true, "paused": false, "text": text }));
        }
    }
    out
}

/// Everything Settings shows.
pub fn view(
    core: &crate::hub::Core,
    b: std::result::Result<std::sync::Arc<crate::build::config::Config>, String>,
    r: std::result::Result<std::sync::Arc<crate::review::config::Config>, String>,
) -> Value {
    let (set, set_error) = match load() {
        Ok(s) => (s, None),
        Err(e) => (json!({}), Some(format!("{e:#}"))),
    };
    let build = match &b {
        Ok(c) => json!({
            "gates": GATES.iter().map(|(k, label, ask, auto)| json!({ "key": k, "label": label, "value": c.gate(k), "ask": ask, "auto": auto })).collect::<Vec<_>>(),
            "builder": c.builder.agent, "checker": c.checker.agent,
            "prereview": c.prereview.iter().map(|p| p.agent.clone()).collect::<Vec<_>>(), "prereview_loops": c.prereview_loops,
            "fix_loops": c.fix_loops, "max_tasks": c.max_tasks, "update": c.update,
            "pr": { "draft": c.pr_draft, "reviewers": c.pr_reviewers },
            "github_label": c.github_label,
            "linear": c.linear.as_ref().map(|l| json!({ "teams": l.teams, "label": l.label, "cycle": l.cycle, "skip_label": l.skip_label })),
            "guides": { "plan_format": c.raw["guides"]["plan_format"], "pr_style": c.raw["guides"]["pr_style"] },
            "repos": c.repos.iter().map(|(slug, r)| (slug.clone(), json!({
                "checkout": crate::paths::tilde(&r.checkout.display().to_string()), "notes": r.notes, "base": r.base, "plan": r.plan,
                "checks": r.checks.iter().map(|k| json!({ "name": k.name, "run": k.run, "required": k.required, "timeout": k.timeout })).collect::<Vec<_>>(),
            }))).collect::<Map<String, Value>>(),
        }),
        Err(_) => Value::Null,
    };
    let review = match &r {
        Ok(c) => json!({
            "reviewers": c.reviewers.iter().map(|x| json!({ "name": x.name, "agent": x.agent })).collect::<Vec<_>>(),
            "checker": c.checker, "post": crate::review::post_mode(core, c, None), "max_rounds": c.max_rounds,
            "post_rule": c.post.json(), "post_override": crate::review::post_override(core).map(|p| p.kind()),
            "enabled": crate::review::enabled(core),
            "guides": c.raw["guides"].clone(),
            "repos": c.repos.iter().map(|(slug, x)| (slug.clone(), json!({
                "checkout": crate::paths::tilde(&x.checkout.display().to_string()), "notes": x.notes, "guides": c.raw["repos"][slug]["guides"].clone(),
                "post": x.post.as_ref().map(|p| p.json()),
            }))).collect::<Map<String, Value>>(),
        }),
        Err(_) => Value::Null,
    };
    let tilde = |p: PathBuf| crate::paths::tilde(&p.display().to_string());
    // The configs as loaded (Settings laid over the Lua), read once for
    // every guide's "used by".
    let rr = r.as_ref().map(|c| c.raw.clone()).unwrap_or_default();
    let br = b.as_ref().map(|c| c.raw.clone()).unwrap_or_default();
    let doc_list: Vec<Value> = docs().iter().map(|d| d.json_in(false, Some((&rr, &br)))).collect();
    json!({
        "file": tilde(path()),
        "set": set,
        "set_error": set_error,
        "build": { "lua": tilde(crate::build::config::path()), "lua_exists": crate::build::config::path().exists(), "error": b.as_ref().err(), "config": build },
        "review": { "lua": tilde(crate::review::config::path()), "lua_exists": crate::review::config::path().exists(), "error": r.as_ref().err(), "config": review },
        "orders": orders(core, b.as_deref().ok(), r.as_deref().ok()),
        // Where new agents open, and every runtime with whether it's followed.
        "runtime": { "value": crate::runtime::spawn_kind(), "env": std::env::var("WH_RUNTIME").ok(), "runtimes": crate::runtime::status(&core.mirror) },
        "docs": doc_list,
        "history": tilde(dir().join(".history")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_lay_over_lua_and_null_removes() {
        let mut raw = json!({ "gates": { "plan": "ask", "open": "ask" }, "repos": { "o/a": { "notes": "x", "checkout": "/c" } }, "effort": { "$fn": "f" } });
        merge(&mut raw, &json!({ "gates": { "open": "auto" }, "repos": { "o/a": { "notes": null } }, "effort": { "plan": "high" } }));
        assert_eq!(raw["gates"], json!({ "plan": "ask", "open": "auto" }));
        assert_eq!(raw["repos"]["o/a"], json!({ "checkout": "/c" }));
        assert_eq!(raw["effort"], json!({ "plan": "high" }), "a function is replaced whole");
        // Nothing set for a part leaves its config alone.
        let mut lua = json!({ "repos": { "o/a": { "checkout": "/c" } } });
        lay(&mut lua, &Value::Null);
        lay(&mut lua, &json!({}));
        assert_eq!(lua, json!({ "repos": { "o/a": { "checkout": "/c" } } }));
        let mut t = json!({ "build": { "repos": { "o/a": {} } } });
        prune(&mut t);
        assert_eq!(t, json!({}));
    }

    #[test]
    fn the_runtime_is_rex_or_herdr() {
        assert!(check_runtime(&json!("herdr")).is_ok());
        assert!(check_runtime(&json!("rex")).is_ok());
        assert!(check_runtime(&Value::Null).is_ok());
        assert_eq!(check_runtime(&json!("tmux")).unwrap_err().to_string(), "runtime is one of rex, herdr, not \"tmux\"");
        assert!(check_runtime(&json!(1)).is_err());
    }

    #[test]
    fn settings_change_only_what_isnt_code() {
        let mut bad = Vec::new();
        refused("build", &json!({ "gates": { "open": "auto" }, "repos": { "o/a": { "notes": "n", "checks": { "test": "cargo test" } } } }), &mut Vec::new(), &mut bad);
        assert!(bad.is_empty(), "{bad:?}");
        refused("build", &json!({ "agents": { "claude": { "program": "/bin/sh" } }, "path": ["/tmp"], "wall": { "mcp": true } }), &mut Vec::new(), &mut bad);
        assert_eq!(bad, vec!["build.agents.claude.program", "build.path", "build.wall.mcp"]);
        let mut bad = Vec::new();
        refused(
            "review",
            &json!({ "repos": { "o/a": { "guides": { "review": "~/x.md" }, "post": "auto" } }, "post": { "auto": { "verdict": "approve" } }, "effort": { "review": "high" } }),
            &mut Vec::new(),
            &mut bad,
        );
        assert_eq!(bad, vec!["review.effort.review"]);
    }

    #[test]
    fn an_empty_or_bad_posting_rule_is_refused_as_sent() {
        assert!(check_posts(&json!({ "review": { "post": { "auto": { "verdict": "approve" } }, "repos": { "o/a": { "post": null } } } })).is_ok());
        assert!(check_posts(&json!({ "build": { "gates": { "open": "auto" } } })).is_ok());
        let err = |v: Value| format!("{:#}", check_posts(&v).unwrap_err());
        assert!(err(json!({ "review": { "post": { "auto": {} } } })).contains("post.auto has no conditions"));
        assert!(err(json!({ "review": { "repos": { "o/a": { "post": "sometimes" } } } })).contains("repos[\"o/a\"].post must be"));
    }

    #[test]
    fn a_posting_rule_is_replaced_whole() {
        let mut raw =
            json!({ "post": { "auto": { "merge_risk": "low", "max_lines": 400 } }, "repos": { "o/a": { "checkout": "/c", "post": { "auto": { "verdict": "approve" } } } } });
        lay(&mut raw, &json!({ "post": { "auto": { "verdict": "approve" } }, "repos": { "o/a": { "post": { "auto": { "labels": ["safe"] } } } } }));
        assert_eq!(raw["post"], json!({ "auto": { "verdict": "approve" } }));
        assert_eq!(raw["repos"]["o/a"], json!({ "checkout": "/c", "post": { "auto": { "labels": ["safe"] } } }));
        // Setting something else leaves it alone.
        let mut raw = json!({ "post": { "auto": { "verdict": "approve" } } });
        lay(&mut raw, &json!({ "max_rounds": 2 }));
        assert_eq!(raw["post"], json!({ "auto": { "verdict": "approve" } }));
        // In settings.json, as change() stores it: a new rule replaces the
        // one there, and null removes it, so review.lua's comes back.
        let mut s = json!({ "review": { "post": { "auto": { "merge_risk": "low" } }, "max_rounds": 2 } });
        let patch = json!({ "review": { "post": { "auto": { "verdict": "approve" } } } });
        whole(&mut s["review"], &patch["review"]);
        merge(&mut s, &patch);
        assert_eq!(s["review"]["post"], json!({ "auto": { "verdict": "approve" } }));
        let patch = json!({ "review": { "post": null } });
        whole(&mut s["review"], &patch["review"]);
        merge(&mut s, &patch);
        assert_eq!(s, json!({ "review": { "max_rounds": 2 } }));
    }
}
