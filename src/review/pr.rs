//! One PR's review state: rex-reviewer's ledger entry, kept field for field
//! so its ledger imports as it is, plus what Wheelhouse adds (pane blocks).

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

/// States a PR is never looked at again in (until it's reopened).
pub const FINISHED: [&str; 3] = ["merged", "closed", "cleaned"];

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Pr {
    pub repo: String,
    pub pr: i64,
    #[serde(default)]
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    #[serde(default)]
    pub round: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// The head when last seen on GitHub.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head: Option<String>,
    /// The head the round in progress reviews.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub round_head: Option<String>,
    /// The head of the last review posted or dismissed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewed_head: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verdict: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merge_risk: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub implementation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_id: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub posted_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cleaned_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub additions: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deletions: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub changed_files: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prepared_round: Option<i64>,
    /// The primary reviewer's worktree; the checker works in it too.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checker_pane: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checker_block: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewer_session: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checker_session: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checker_agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checker_effort: Option<String>,
    #[serde(default, deserialize_with = "list", skip_serializing_if = "Vec::is_empty")]
    pub checker_threads: Vec<String>,
    #[serde(default, deserialize_with = "map")]
    pub reviewers: BTreeMap<String, Reviewer>,
    #[serde(default, deserialize_with = "list")]
    pub round_reviewers: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewers_done: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewers_total: Option<i64>,
    #[serde(default, deserialize_with = "list", skip_serializing_if = "Vec::is_empty")]
    pub extra_worktrees: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_failed_epoch: Option<i64>,
    /// When the hub last read what the PR is for (its Linear issues and
    /// the docket's intent).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub integrated_at: Option<String>,
    #[serde(default, deserialize_with = "list")]
    pub history: Vec<Value>,
    /// Fields this version doesn't know, kept as they were.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Reviewer {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub agent: String,
    #[serde(default)]
    pub index: i64,
    #[serde(default)]
    pub primary: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane: Option<String>,
    /// The Rex block of its pane, while it's open.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_round: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens_seen: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// A list, also from Lua's `{}` for an empty one.
fn list<'de, D: Deserializer<'de>, T: serde::de::DeserializeOwned>(d: D) -> Result<Vec<T>, D::Error> {
    match Value::deserialize(d)? {
        Value::Array(a) => a.into_iter().map(|v| serde_json::from_value(v).map_err(serde::de::Error::custom)).collect(),
        _ => Ok(Vec::new()),
    }
}

/// A map, also from Lua's `[]`-looking empty table.
fn map<'de, D: Deserializer<'de>, T: serde::de::DeserializeOwned>(d: D) -> Result<BTreeMap<String, T>, D::Error> {
    match Value::deserialize(d)? {
        Value::Object(o) => o.into_iter().map(|(k, v)| serde_json::from_value(v).map(|v| (k, v)).map_err(serde::de::Error::custom)).collect(),
        _ => Ok(BTreeMap::new()),
    }
}

impl Pr {
    pub fn new(repo: &str, pr: i64) -> Self {
        Pr { repo: repo.to_string(), pr, state: "queued".into(), ..Default::default() }
    }

    /// `owner/name#n`: the PR's key everywhere in Wheelhouse.
    pub fn key(&self) -> String {
        format!("{}#{}", self.repo, self.pr)
    }

    /// `name#n`, as titles and items show it.
    pub fn short(&self) -> String {
        format!("{}#{}", self.repo.rsplit('/').next().unwrap_or(&self.repo), self.pr)
    }

    pub fn finished(&self) -> bool {
        FINISHED.contains(&self.state.as_str())
    }

    pub fn phase(&self) -> &str {
        self.phase.as_deref().unwrap_or("reviewer")
    }

    /// The round's reviewers, primary first.
    pub fn round_reviewer_list(&self) -> Vec<Reviewer> {
        self.round_reviewers.iter().filter_map(|n| self.reviewers.get(n).cloned()).collect()
    }

    /// Every worktree this PR has used.
    pub fn worktrees(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut add = |w: &str| {
            if !w.is_empty() && !out.iter().any(|x| x == w) {
                out.push(w.to_string());
            }
        };
        if let Some(w) = &self.worktree {
            add(w);
        }
        for w in &self.extra_worktrees {
            add(w);
        }
        for r in self.reviewers.values() {
            if let Some(w) = &r.worktree {
                add(w);
            }
        }
        out
    }
}

/// `owner/name#n` → (`owner/name`, n).
pub fn parse_key(s: &str) -> Option<(String, i64)> {
    let (repo, n) = s.trim().rsplit_once('#')?;
    let n: i64 = n.parse().ok()?;
    (repo.contains('/') && n > 0).then(|| (repo.to_string(), n))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_ledger_entry_round_trips_with_lua_empties_and_unknown_fields() {
        let v = json!({
            "repo": "acme/api", "pr": 632, "state": "posted", "round": 9,
            "history": {}, "round_reviewers": {}, "reviewers": [], "extra_worktrees": {},
            "review_id": 5808497544u64, "someday_field": { "x": 1 }
        });
        let pr: Pr = serde_json::from_value(v).unwrap();
        assert_eq!(pr.key(), "acme/api#632");
        assert_eq!(pr.short(), "api#632");
        assert!(pr.history.is_empty() && pr.reviewers.is_empty() && pr.round_reviewers.is_empty());
        let back = serde_json::to_value(&pr).unwrap();
        assert_eq!(back["someday_field"]["x"], 1);
        assert_eq!(back["review_id"], 5808497544u64);
        assert_eq!(parse_key("acme/api#632"), Some(("acme/api".into(), 632)));
        assert_eq!(parse_key("api#632"), None);
    }
}
