//! Who posts a checked review: `post` in review.lua (or Settings), for
//! every repo or one (`repos["owner/name"].post`).
//!
//! - `"ask"`: it waits for you to Post, Preview or Dismiss it.
//! - `"auto"`: the checker posts it.
//! - `{ auto = { … } }`: a rule. The checker previews and stops, as with
//!   ask; then the hub checks the rule once against the checked review and
//!   posts it when every condition holds, or leaves it waiting for you with
//!   the reason.
//! - A Lua function of the PR and its checked review, returning `"ask"` or
//!   `"auto"`, checked the same way.

use anyhow::{Result, anyhow, bail};
use serde_json::{Value, json};
use std::path::Path;

/// The conditions a rule may set, in the order they read.
pub const CONDITIONS: [&str; 6] = ["verdict", "merge_risk", "max_lines", "authors", "labels", "not_touching"];

/// Merge risk, lowest first.
const RISK: [&str; 3] = ["low", "medium", "high"];

#[derive(Clone, Debug, PartialEq)]
pub enum Post {
    Ask,
    Auto,
    When(When),
    /// A Lua function, by its path in the config (`post`, `repos.o/a.post`).
    Lua(String),
}

/// A rule's conditions: every one set must hold for a review to post itself.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct When {
    /// Only an approval posts itself.
    pub approve: bool,
    /// The highest merge risk that posts itself: `low` or `medium`.
    pub merge_risk: Option<String>,
    /// The most lines the PR may change (additions plus deletions).
    pub max_lines: Option<i64>,
    /// Only PRs by one of these.
    pub authors: Vec<String>,
    /// Only PRs with one of these labels.
    pub labels: Vec<String>,
    /// No changed path may contain any of these.
    pub not_touching: Vec<String>,
}

/// What the hub decided for one checked review.
#[derive(Clone, Debug, PartialEq)]
pub struct Decision {
    pub auto: bool,
    pub why: String,
}

fn list(v: &Value, at: &str) -> Result<Vec<String>> {
    let items: Vec<&Value> = match v {
        Value::String(_) => vec![v],
        Value::Array(a) => a.iter().collect(),
        _ => bail!("{at} must be a name or a list of them"),
    };
    let mut out = Vec::new();
    for x in items {
        match x.as_str().map(str::trim) {
            Some(s) if !s.is_empty() => out.push(s.to_string()),
            _ => bail!("{at} must be a name or a list of them, none empty"),
        }
    }
    if out.is_empty() {
        bail!("{at} is empty; leave it out instead");
    }
    Ok(out)
}

/// `post` as the config has it (`at` names where, for errors). `None`: not set.
pub fn parse(v: &Value, at: &str) -> Result<Option<Post>> {
    match v {
        Value::Null => Ok(None),
        Value::String(s) if s == "ask" => Ok(Some(Post::Ask)),
        Value::String(s) if s == "auto" => Ok(Some(Post::Auto)),
        Value::Object(o) if o.contains_key("$fn") => Ok(Some(Post::Lua(v["$fn"].as_str().unwrap_or_default().to_string()))),
        Value::Object(o) => {
            if let Some(k) = o.keys().find(|k| *k != "auto") {
                bail!("{at}.{k}: a rule is {{ auto = {{ … }} }}, with the conditions under auto");
            }
            let c = o.get("auto").and_then(Value::as_object).ok_or_else(|| anyhow!("{at}.auto must be a table of conditions"))?;
            if c.is_empty() {
                bail!("{at}.auto has no conditions; for a review that always posts itself, use \"auto\"");
            }
            let mut w = When::default();
            for (k, x) in c {
                let here = format!("{at}.auto.{k}");
                match k.as_str() {
                    "verdict" => match x.as_str() {
                        Some("approve") => w.approve = true,
                        _ => bail!("{here} can only be \"approve\", not {x}"),
                    },
                    "merge_risk" => match x.as_str() {
                        Some(r @ ("low" | "medium")) => w.merge_risk = Some(r.to_string()),
                        _ => bail!("{here} must be low or medium, not {x}"),
                    },
                    "max_lines" => match x.as_i64() {
                        Some(n) if n >= 1 => w.max_lines = Some(n),
                        _ => bail!("{here} must be a whole number of lines, 1 or more, not {x}"),
                    },
                    "authors" => w.authors = list(x, &here)?,
                    "labels" => w.labels = list(x, &here)?,
                    "not_touching" => w.not_touching = list(x, &here)?,
                    _ => bail!("{here}: the conditions are {}", CONDITIONS.join(", ")),
                }
            }
            Ok(Some(Post::When(w)))
        }
        other => bail!("{at} must be \"ask\", \"auto\", {{ auto = {{ … }} }}, or a function, not {other}"),
    }
}

fn or(items: &[String], quote: bool) -> String {
    let q: Vec<String> = items.iter().map(|s| if quote { format!("`{s}`") } else { s.clone() }).collect();
    match q.len() {
        0 => String::new(),
        1 => q[0].clone(),
        n => format!("{} or {}", q[..n - 1].join(", "), q[n - 1]),
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

impl When {
    /// Each condition as a clause: "it approves", "its merge risk is low".
    pub fn clauses(&self) -> Vec<String> {
        let mut out = Vec::new();
        if self.approve {
            out.push("it approves".to_string());
        }
        match self.merge_risk.as_deref() {
            Some("low") => out.push("its merge risk is low".into()),
            Some(r) => out.push(format!("its merge risk is {r} or lower")),
            None => {}
        }
        if let Some(n) = self.max_lines {
            out.push(format!("it changes at most {n} lines"));
        }
        if !self.authors.is_empty() {
            out.push(format!("it's by {}", or(&self.authors, false)));
        }
        if !self.labels.is_empty() {
            out.push(format!("it's labeled {}", or(&self.labels, true)));
        }
        if !self.not_touching.is_empty() {
            out.push(format!("it doesn't touch {}", or(&self.not_touching, true)));
        }
        out
    }

    pub fn json(&self) -> Value {
        let mut o = serde_json::Map::new();
        if self.approve {
            o.insert("verdict".into(), json!("approve"));
        }
        if let Some(r) = &self.merge_risk {
            o.insert("merge_risk".into(), json!(r));
        }
        if let Some(n) = self.max_lines {
            o.insert("max_lines".into(), json!(n));
        }
        for (k, v) in [("authors", &self.authors), ("labels", &self.labels), ("not_touching", &self.not_touching)] {
            if !v.is_empty() {
                o.insert(k.into(), json!(v));
            }
        }
        Value::Object(o)
    }

    /// Check the rule against a checked review's facts: what held, or what didn't.
    pub fn check(&self, f: &Value) -> std::result::Result<Vec<String>, Vec<String>> {
        let (mut held, mut missed) = (Vec::new(), Vec::new());
        if self.approve {
            if f["verdict"] == "approve" {
                held.push("it approves".to_string());
            } else {
                missed.push("it requests changes (auto needs an approval)".to_string());
            }
        }
        if let Some(max) = &self.merge_risk {
            let need = if max == "low" { "low".to_string() } else { format!("{max} or lower") };
            let rank = |s: &str| RISK.iter().position(|x| *x == s);
            match f["merge_risk"].as_str() {
                Some(r) if rank(r).is_some() && rank(r) <= rank(max) => held.push(format!("merge risk is {r}")),
                Some(r) => missed.push(format!("merge risk is {r} (auto needs {need})")),
                None => missed.push(format!("it has no merge risk (auto needs {need})")),
            }
        }
        if let Some(max) = self.max_lines {
            match f["lines"].as_i64() {
                Some(n) if n <= max => held.push(format!("it changes {n} lines")),
                Some(n) => missed.push(format!("it changes {n} lines (auto needs at most {max})")),
                None => missed.push(format!("its size isn't known (auto needs at most {max} lines)")),
            }
        }
        if !self.authors.is_empty() {
            let by = f["author"].as_str().unwrap_or("");
            if self.authors.iter().any(|a| a.eq_ignore_ascii_case(by)) {
                held.push(format!("it's by {by}"));
            } else {
                missed.push(format!("it's by {} (auto needs {})", if by.is_empty() { "someone unknown" } else { by }, or(&self.authors, false)));
            }
        }
        if !self.labels.is_empty() {
            let has: Vec<&str> = f["labels"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
            match self.labels.iter().find(|l| has.iter().any(|h| h.eq_ignore_ascii_case(l))) {
                Some(l) => held.push(format!("it's labeled `{l}`")),
                None => missed.push(format!("it isn't labeled {} (auto needs one)", or(&self.labels, true))),
            }
        }
        if !self.not_touching.is_empty() {
            let files: Vec<&str> = f["files"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
            let hit = files.iter().find_map(|file| self.not_touching.iter().find(|p| file.contains(p.as_str())).map(|p| (*file, p)));
            match hit {
                Some((file, p)) => missed.push(format!("it touches {file} (auto needs nothing in `{p}`)")),
                None if files.is_empty() => missed.push(format!("its files aren't known (auto needs nothing in {})", or(&self.not_touching, true))),
                None => held.push(format!("it doesn't touch {}", or(&self.not_touching, true))),
            }
        }
        if missed.is_empty() { Ok(held) } else { Err(missed) }
    }
}

impl Post {
    /// `ask`, `auto`, or `rule`.
    pub fn kind(&self) -> &'static str {
        match self {
            Post::Ask => "ask",
            Post::Auto => "auto",
            Post::When(_) | Post::Lua(_) => "rule",
        }
    }

    /// What happens to a checked review, as the end of a sentence about it:
    /// "posts itself if it approves, or else waits for you".
    pub fn outcome(&self) -> String {
        match self {
            Post::Ask => "waits for you to post it".into(),
            Post::Auto => "posts itself".into(),
            Post::When(w) => format!("posts itself if {}, or else waits for you", and(&w.clauses())),
            Post::Lua(f) => format!("posts itself or waits for you, as `{f}` in review.lua decides"),
        }
    }

    /// The standing order's end: "…, check with Claude Code, and ask you before posting."
    pub fn order(&self) -> String {
        match self {
            Post::Ask => "ask you before posting".into(),
            Post::Auto => "post it".into(),
            Post::When(w) => format!("post it if {}, or else ask you", and(&w.clauses())),
            Post::Lua(f) => format!("post it or ask you, as `{f}` in review.lua decides"),
        }
    }

    /// For the apps: `{mode, auto, fn, text}`.
    pub fn json(&self) -> Value {
        json!({
            "mode": self.kind(),
            "auto": match self { Post::When(w) => w.json(), _ => Value::Null },
            "fn": match self { Post::Lua(f) => json!(f), _ => Value::Null },
            "text": self.outcome(),
        })
    }

    /// Decide for one checked review. `facts`: the PR and the checked
    /// review (verdict, merge_risk, lines, author, labels, files, …), which
    /// a Lua function gets as its argument. A function that fails asks.
    pub fn decide(&self, facts: &Value, lua: &Path) -> Decision {
        match self {
            Post::Ask => Decision { auto: false, why: "posting is set to Ask".into() },
            Post::Auto => Decision { auto: true, why: "posting is set to Auto".into() },
            Post::When(w) => match w.check(facts) {
                Ok(held) => Decision { auto: true, why: and(&held) },
                Err(missed) => Decision { auto: false, why: missed.join("; ") },
            },
            Post::Lua(f) => match super::lua::call(lua, f, facts) {
                Ok(Value::String(s)) if s == "auto" => Decision { auto: true, why: format!("`{f}` in review.lua returned auto") },
                Ok(Value::String(s)) if s == "ask" => Decision { auto: false, why: format!("`{f}` in review.lua returned ask") },
                Ok(Value::Null | Value::Bool(false)) => Decision { auto: false, why: format!("`{f}` in review.lua returned nothing") },
                Ok(other) => Decision { auto: false, why: format!("`{f}` in review.lua returned {other}; return \"ask\" or \"auto\"") },
                Err(e) => Decision { auto: false, why: format!("`{f}` in review.lua failed: {}", format!("{e:#}").lines().next().unwrap_or("")) },
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(v: Value) -> Post {
        parse(&v, "post").unwrap().unwrap()
    }

    fn facts() -> Value {
        json!({ "verdict": "approve", "merge_risk": "low", "lines": 120, "author": "Alice", "labels": ["safe"], "files": ["src/api.rs", "docs/api.md"] })
    }

    #[test]
    fn post_reads_as_ask_auto_a_rule_or_a_function() {
        assert_eq!(parse(&Value::Null, "post").unwrap(), None);
        assert_eq!(rule(json!("ask")), Post::Ask);
        assert_eq!(rule(json!("auto")), Post::Auto);
        assert_eq!(rule(json!({ "$fn": "repos.o/a.post" })), Post::Lua("repos.o/a.post".into()));
        let w =
            rule(json!({ "auto": { "verdict": "approve", "merge_risk": "medium", "max_lines": 400, "authors": "alice", "labels": ["safe", "docs"], "not_touching": ["db/"] } }));
        let Post::When(w) = w else { panic!("a rule") };
        assert_eq!(
            w,
            When {
                approve: true,
                merge_risk: Some("medium".into()),
                max_lines: Some(400),
                authors: vec!["alice".into()],
                labels: vec!["safe".into(), "docs".into()],
                not_touching: vec!["db/".into()]
            }
        );
        assert_eq!(rule(w.json().as_object().map(|o| json!({ "auto": o })).unwrap()), Post::When(w.clone()), "its JSON reads back the same");
    }

    #[test]
    fn a_bad_rule_says_whats_wrong() {
        let err = |v: Value| parse(&v, "repos[\"o/a\"].post").unwrap_err().to_string();
        assert!(err(json!("sometimes")).contains("must be \"ask\", \"auto\""));
        assert!(err(json!({ "auto": {} })).contains("has no conditions"));
        assert!(err(json!({ "ask": {} })).contains("a rule is { auto = { … } }"));
        assert!(err(json!({ "auto": true })).contains("must be a table of conditions"));
        assert!(err(json!({ "auto": { "verdict": "request_changes" } })).contains("can only be \"approve\""));
        assert!(err(json!({ "auto": { "merge_risk": "high" } })).contains("must be low or medium"));
        assert!(err(json!({ "auto": { "max_lines": 0 } })).contains("1 or more"));
        assert!(err(json!({ "auto": { "authors": [] } })).contains("is empty"));
        assert!(err(json!({ "auto": { "labels": ["ok", ""] } })).contains("none empty"));
        assert_eq!(err(json!({ "auto": { "size": 3 } })), "repos[\"o/a\"].post.auto.size: the conditions are verdict, merge_risk, max_lines, authors, labels, not_touching");
    }

    #[test]
    fn a_rule_posts_when_every_condition_holds_and_says_why_it_asks() {
        let w = rule(
            json!({ "auto": { "verdict": "approve", "merge_risk": "low", "max_lines": 400, "authors": ["alice", "bob"], "labels": ["safe"], "not_touching": ["db/migrations/"] } }),
        );
        let d = w.decide(&facts(), Path::new("/x"));
        assert!(d.auto);
        assert_eq!(d.why, "it approves, merge risk is low, it changes 120 lines, it's by Alice, it's labeled `safe`, and it doesn't touch `db/migrations/`");

        let mut f = facts();
        f["verdict"] = json!("request_changes");
        f["merge_risk"] = json!("medium");
        f["lines"] = json!(612);
        f["author"] = json!("carol");
        f["labels"] = json!([]);
        f["files"] = json!(["src/api.rs", "db/migrations/007_users.sql"]);
        let d = w.decide(&f, Path::new("/x"));
        assert!(!d.auto);
        assert_eq!(
            d.why,
            "it requests changes (auto needs an approval); merge risk is medium (auto needs low); it changes 612 lines (auto needs at most 400); it's by carol (auto needs alice or bob); it isn't labeled `safe` (auto needs one); it touches db/migrations/007_users.sql (auto needs nothing in `db/migrations/`)"
        );

        // Merge risk off, or files unknown: the rule can't hold.
        let mr = rule(json!({ "auto": { "merge_risk": "medium" } }));
        assert!(mr.decide(&json!({ "merge_risk": "medium" }), Path::new("/x")).auto);
        assert_eq!(mr.decide(&json!({ "merge_risk": "high" }), Path::new("/x")).why, "merge risk is high (auto needs medium or lower)");
        assert_eq!(mr.decide(&json!({}), Path::new("/x")).why, "it has no merge risk (auto needs medium or lower)");
        assert!(!rule(json!({ "auto": { "not_touching": "db/" } })).decide(&json!({ "files": [] }), Path::new("/x")).auto);
    }

    #[test]
    fn a_rule_reads_as_a_sentence() {
        let w = rule(json!({ "auto": { "verdict": "approve", "merge_risk": "low", "max_lines": 400 } }));
        assert_eq!(w.order(), "post it if it approves, its merge risk is low, and it changes at most 400 lines, or else ask you");
        assert_eq!(w.outcome(), "posts itself if it approves, its merge risk is low, and it changes at most 400 lines, or else waits for you");
        assert_eq!(rule(json!({ "auto": { "labels": ["a", "b", "c"] } })).order(), "post it if it's labeled `a`, `b` or `c`, or else ask you");
        assert_eq!(Post::Ask.order(), "ask you before posting");
        assert_eq!(Post::Auto.order(), "post it");
        assert_eq!(Post::Lua("post".into()).order(), "post it or ask you, as `post` in review.lua decides");
        assert_eq!(w.json()["mode"], "rule");
        assert_eq!(w.json()["auto"]["max_lines"], 400);
    }
}
