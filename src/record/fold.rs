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
                self.status = if self.protocol == "agent" { "in_progress".into() } else { "todo".into() };
                self.created_at = e.at.clone();
                if let Ok(a) = serde_json::from_value::<AgentRef>(d["agent"].clone()) {
                    self.agent = Some(a);
                }
                if let Some(sess) = s(&d["session"]) {
                    self.session = Some(sess);
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
