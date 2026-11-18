//! A docket's state, worked out from its entries in order. Nothing here is
//! stored as truth: the store keeps a copy for speed, and `rebuild` proves
//! the copy matches.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::Entry;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct DocketState {
    pub id: String,
    pub title: String,
    /// Where the title came from: `pane` (made up from the pane), `prompt`
    /// (the first thing you asked), or `person` (you named it).
    pub title_source: String,
    pub status: String,
    /// `agent` or `none`, for now.
    pub protocol: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<AgentRef>,
    /// The agent session that holds the docket, from its hooks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    pub prompts: u32,
    pub turns: u32,
    pub notes: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_turn: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub released: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<String>,
    pub entries: i64,
    /// Hex of the last entry's chain value.
    pub head: String,
    /// The docket's living spec: its newest `intent`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent: Option<IntentRef>,
    #[serde(default)]
    pub intent_versions: u32,
    /// The Linear issue linked to the docket, as Linear last said.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub linear: Option<LinearRef>,
    /// Other issues linked beside it: a PR can address more than one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub linear_also: Vec<IssueLink>,
    /// The PR a Review docket is about, as rex-reviewer last said.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review: Option<ReviewRef>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct ReviewRef {
    pub key: String,
    #[serde(default)]
    pub repo: String,
    #[serde(default)]
    pub pr: i64,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub phase: Option<String>,
    #[serde(default)]
    pub round: Option<i64>,
    #[serde(default)]
    pub verdict: Option<String>,
    #[serde(default)]
    pub merge_risk: Option<String>,
    #[serde(default)]
    pub review_url: Option<String>,
    #[serde(default)]
    pub implementation: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct IntentRef {
    pub body: String,
    /// `person`, `agent`, or `linear`.
    pub source: String,
    /// The actor's id.
    pub by: String,
    pub at: String,
    pub seq: i64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct IssueLink {
    pub key: String,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct LinearRef {
    pub key: String,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub state_type: Option<String>,
    #[serde(default)]
    pub priority: Option<String>,
    #[serde(default)]
    pub assignee: Option<String>,
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
    /// Linear's description as last read, to see when it changes.
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub comments_seen_at: Option<String>,
}

/// The agent a docket is about, as Rex placed it.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct AgentRef {
    pub app: String,
    pub host: String,
    pub block: String,
    #[serde(default)]
    pub rex_session: String,
    #[serde(default)]
    pub pane: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
}

fn s(v: &Value) -> Option<String> {
    v.as_str().map(str::to_string)
}

impl DocketState {
    pub fn new(id: &str) -> Self {
        DocketState { id: id.to_string(), status: "todo".into(), protocol: "none".into(), ..Default::default() }
    }

    /// Apply the next entry.
    pub fn apply(&mut self, seq: i64, chain: &str, e: &Entry) {
        let d = &e.data;
        match e.kind.as_str() {
            "open" => {
                self.title = e.body.clone().unwrap_or_default();
                self.title_source = s(&d["title_source"]).unwrap_or_else(|| "person".into());
                self.protocol = s(&d["protocol"]).unwrap_or_else(|| "none".into());
                // Work that starts with an agent on it is under way from the start.
                self.status = if matches!(self.protocol.as_str(), "agent" | "review") { "in_progress".into() } else { "todo".into() };
                self.created_at = e.at.clone();
                if let Ok(a) = serde_json::from_value::<AgentRef>(d["agent"].clone()) {
                    self.agent = Some(a);
                }
                if let Some(sess) = s(&d["session"]) {
                    self.session = Some(sess);
                }
                if let Ok(r) = serde_json::from_value::<ReviewRef>(d["review"].clone()) {
                    self.review = Some(r);
                }
            }
            "observed" if d["what"].as_str() == Some("review") => {
                if let Ok(r) = serde_json::from_value::<ReviewRef>(d["review"].clone()) {
                    self.review = Some(r);
                }
            }
            "set" => {
                if let Some(t) = s(&d["title"]) {
                    self.title = t;
                    self.title_source = s(&d["title_source"]).unwrap_or_else(|| "person".into());
                }
                if let Some(st) = s(&d["status"]) {
                    self.ended_at = matches!(st.as_str(), "done" | "dropped").then(|| e.at.clone());
                    self.status = st;
                }
            }
            "claim" => {
                self.session = s(&d["session"]);
                self.released = None;
                if let Ok(a) = serde_json::from_value::<AgentRef>(d["agent"].clone()) {
                    self.agent = Some(a);
                }
            }
            "release" => {
                self.released = Some(s(&d["reason"]).unwrap_or_else(|| "released".into()));
            }
            "note" => {
                if d["via"].as_str() == Some("prompt") {
                    self.prompts += 1;
                    self.last_prompt = e.body.clone();
                } else {
                    self.notes += 1;
                }
            }
            "intent" => {
                let source = s(&d["source"]).unwrap_or_else(|| e.actor.typ.clone());
                self.intent = Some(IntentRef { body: e.body.clone().unwrap_or_default(), source, by: e.actor.id.clone(), at: e.at.clone(), seq });
                self.intent_versions += 1;
            }
            "link"
                if d["kind"].as_str() == Some("linear") && d["also"].as_bool() == Some(true) && self.linear.as_ref().is_some_and(|l| Some(l.key.as_str()) != d["key"].as_str()) =>
            {
                let key = s(&d["key"]).unwrap_or_default();
                if !self.linear_also.iter().any(|l| l.key == key) {
                    self.linear_also.push(IssueLink { key, url: s(&d["url"]), title: s(&d["title"]) });
                }
            }
            "link" if d["kind"].as_str() == Some("linear") => {
                let mut l = self.linear.take().unwrap_or_default();
                l.key = s(&d["key"]).unwrap_or_default();
                l.id = s(&d["id"]).or(l.id);
                l.url = s(&d["url"]).or(l.url);
                l.title = s(&d["title"]).or(l.title);
                self.linear = Some(l);
            }
            "observed" if d["what"].as_str() == Some("linear") => {
                let i = &d["issue"];
                let mut l = self.linear.take().unwrap_or_default();
                if let Some(k) = s(&i["key"]) {
                    l.key = k;
                }
                l.id = s(&i["id"]).or(l.id);
                l.url = s(&i["url"]).or(l.url);
                l.title = s(&i["title"]).or(l.title);
                l.state = s(&i["state"]);
                l.state_type = s(&i["state_type"]);
                l.priority = s(&i["priority"]);
                l.assignee = s(&i["assignee"]);
                l.branch = s(&i["branch"]).or(l.branch);
                l.updated_at = s(&i["updated_at"]);
                if d.get("description").is_some() {
                    l.description = s(&d["description"]);
                }
                // Comments already there when the hub linked it, left unread.
                let seen = s(&d["comments_seen_at"]);
                if seen > l.comments_seen_at {
                    l.comments_seen_at = seen;
                }
                self.linear = Some(l);
            }
            "observed" if d["what"].as_str() == Some("linear-comment") => {
                if let Some(l) = self.linear.as_mut() {
                    let at = s(&d["created_at"]);
                    if at > l.comments_seen_at {
                        l.comments_seen_at = at;
                    }
                }
            }
            "turn" => {
                self.turns += 1;
                if e.body.as_deref().is_some_and(|b| !b.trim().is_empty()) {
                    self.last_turn = e.body.clone();
                }
            }
            _ => {}
        }
        self.updated_at = e.at.clone();
        self.entries = seq;
        self.head = chain.to_string();
    }

    pub fn short_head(&self) -> &str {
        &self.head[..self.head.len().min(8)]
    }

    pub fn is_open(&self) -> bool {
        !matches!(self.status.as_str(), "done" | "dropped")
    }
}

/// Fold a whole docket from its entries.
#[cfg(test)]
pub fn fold<'a>(id: &str, entries: impl IntoIterator<Item = (i64, &'a str, &'a Entry)>) -> DocketState {
    let mut st = DocketState::new(id);
    for (seq, chain, e) in entries {
        st.apply(seq, chain, e);
    }
    st
}
