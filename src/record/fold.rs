//! A docket's state, worked out from its entries in order. Nothing here is
//! stored as truth: the store keeps a copy for speed, and `rebuild` proves
//! the copy matches.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

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
    /// The agent the apps show when they show one: the most recently active
    /// of `agents` still on the docket, or else the last one. Kept beside
    /// `agents` so a reader of one agent still reads.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<AgentRef>,
    /// Every agent that has been on the docket, by pane, in the order they
    /// joined: those on it now have no `left_at`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub agents: Vec<DocketAgent>,
    /// The session of `agent`, from its hooks.
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
    /// On hold, and why: its protocol takes no steps until you resume it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hold: Option<String>,
    /// Closed with nothing witnessed or checkable behind it: on your word.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed_on: Option<String>,
    /// Each witnessed check's latest result, by name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub checks: BTreeMap<String, CheckBrief>,
    /// A local docket that was shared: the shared docket it goes on as
    /// (`ACME-31`). Its record here ends at the share.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shared_as: Option<String>,
    /// A shared docket: the sharer's local docket it started from, by its
    /// sharer (`jordan/D-12`), when it had one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shared_from: Option<String>,
    /// A shared docket's owners and members, by handle (docs/teams.md).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub owners: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub members: Vec<String>,
    /// The fold that made this state (`VERSION`). An older build that saves
    /// a state leaves it out, so the next start folds that docket again.
    #[serde(default)]
    pub fold: u32,
}

/// A witnessed check's latest result, in brief.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct CheckBrief {
    pub ok: bool,
    pub head: String,
    pub at: String,
    #[serde(default)]
    pub required: bool,
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

/// One agent on a docket: a pane, from its first claim (or the `open` that
/// named it) to its release.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct DocketAgent {
    #[serde(flatten)]
    pub agent: AgentRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    /// What it's for, when a protocol says: `Build builder`, `claude reviewer`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub round: Option<i64>,
    /// How it came on: its first claim's `source` (`join` for `wh join`,
    /// `wheelhouse`, `startup`, …), or the `open`'s (`leave` for the docket
    /// `wh leave` gave it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub via: Option<String>,
    /// Whose it is, on a shared docket: the workspace member whose hub
    /// claimed it (docs/teams.md). None on your own dockets, where every
    /// agent is yours. Panes are told apart by member and block together:
    /// two Macs can each have a `herdr:t1`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub member: Option<String>,
    pub joined_at: String,
    /// When it last did something the record names it for.
    pub active_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub left_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub left: Option<String>,
}

impl DocketAgent {
    pub fn on(&self) -> bool {
        self.left_at.is_none()
    }
}

/// The member an entry's hub or agent belongs to, on a shared docket:
/// `hub@sam` and `codex@sam` are Sam's. None for your own hub's (`@local`).
pub fn member_of(e: &Entry) -> Option<String> {
    // On a shared docket, anything a person wrote is theirs too: never
    // read as this hub's own (the rules refuse a person's claim anyway).
    if e.actor.typ == "person" {
        return (!e.docket.starts_with("D-")).then(|| e.actor.id.clone());
    }
    if !matches!(e.actor.typ.as_str(), "hub" | "agent") {
        return None;
    }
    let (_, m) = e.actor.id.rsplit_once('@')?;
    (m != "local" && !m.is_empty()).then(|| m.to_string())
}

/// The pane an entry names as its agent's: an agent's own entries carry it
/// in the actor, the hub's in `data.block`.
pub fn block_of(e: &Entry) -> Option<&str> {
    e.data["block"].as_str().or_else(|| e.actor.extra.get("block").and_then(Value::as_str)).filter(|b| !b.is_empty())
}

fn s(v: &Value) -> Option<String> {
    v.as_str().map(str::to_string)
}

impl DocketState {
    pub fn new(id: &str) -> Self {
        // Made by this fold, from nothing. A saved state an older fold made
        // keeps its version through later appends, until it's folded again.
        DocketState { id: id.to_string(), status: "todo".into(), protocol: "none".into(), fold: VERSION, ..Default::default() }
    }

    /// Apply the next entry.
    pub fn apply(&mut self, seq: i64, chain: &str, e: &Entry) {
        let d = &e.data;
        let member = member_of(e);
        match e.kind.as_str() {
            "open" => {
                self.title = e.body.clone().unwrap_or_default();
                self.title_source = s(&d["title_source"]).unwrap_or_else(|| "person".into());
                self.protocol = s(&d["protocol"]).unwrap_or_else(|| "none".into());
                // Work that starts with an agent on it is under way from the start.
                self.status = if matches!(self.protocol.as_str(), "agent" | "review") { "in_progress".into() } else { "todo".into() };
                self.created_at = e.at.clone();
                if let Ok(a) = serde_json::from_value::<AgentRef>(d["agent"].clone()) {
                    self.join(a, member.clone(), s(&d["session"]), None, None, s(&d["source"]), &e.at);
                }
                if let Some(sess) = s(&d["session"]) {
                    self.session = Some(sess);
                }
                if let Ok(r) = serde_json::from_value::<ReviewRef>(d["review"].clone()) {
                    self.review = Some(r);
                }
            }
            "share" => {
                self.title = s(&d["title"]).unwrap_or_default();
                self.title_source = "person".into();
                self.protocol = s(&d["protocol"]).filter(|p| matches!(p.as_str(), "agent" | "none")).unwrap_or_else(|| "none".into());
                self.status = s(&d["status"]).filter(|st| matches!(st.as_str(), "todo" | "in_progress")).unwrap_or_else(|| "todo".into());
                self.created_at = e.at.clone();
                self.shared_from = s(&d["local"]).map(|l| format!("{}/{l}", e.actor.id));
                self.owners = d["owners"].as_array().into_iter().flatten().filter_map(|o| o.as_str().map(str::to_string)).collect();
                if let Some(b) = s(&d["intent"]).filter(|b| !b.trim().is_empty()) {
                    self.intent = Some(IntentRef { body: b, source: "person".into(), by: e.actor.id.clone(), at: e.at.clone(), seq });
                    self.intent_versions += 1;
                }
            }
            "people" => {
                let on = |h: &String| self.owners.contains(h) || self.members.contains(h);
                let self_join = |h: &String| e.actor.id == *h && d["action"] != "remove" && d["role"] != "owner";
                if let Some(h) = s(&d["handle"]).filter(|h| !(self_join(h) && on(h))) {
                    self.owners.retain(|o| *o != h);
                    self.members.retain(|m| *m != h);
                    match (d["action"].as_str(), d["role"].as_str()) {
                        (Some("remove"), _) => {}
                        (_, Some("owner")) => self.owners.push(h),
                        _ => self.members.push(h),
                    }
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
                    self.closed_on = (st == "done" && d["on"] == "word").then(|| s(&d["reason"]).unwrap_or_else(|| "your word".into()));
                    self.status = st;
                }
                if d.get("hold").is_some() {
                    self.hold = s(&d["hold"]);
                }
                if let Some(to) = s(&d["shared_as"]) {
                    self.shared_as = Some(to);
                }
            }
            "check" => {
                if let Some(n) = s(&d["name"]) {
                    let c = CheckBrief { ok: d["ok"] == true, head: s(&d["head"]).unwrap_or_default(), at: e.at.clone(), required: d["required"] != false };
                    self.checks.insert(n, c);
                }
            }
            "claim" => {
                self.session = s(&d["session"]);
                self.released = None;
                match serde_json::from_value::<AgentRef>(d["agent"].clone()) {
                    Ok(a) => self.join(a, member.clone(), s(&d["session"]), s(&d["role"]), d["round"].as_i64(), s(&d["source"]), &e.at),
                    // A claim that names no pane is the docket's agent's.
                    Err(_) => {
                        let sess = s(&d["session"]);
                        if let Some(b) = self.agent.as_ref().map(|a| a.block.clone())
                            && let Some(x) = self.agents.iter_mut().rev().find(|x| x.agent.block == b && x.member == member)
                        {
                            x.session = sess;
                            x.active_at = e.at.clone();
                            x.left_at = None;
                            x.left = None;
                        }
                    }
                }
            }
            "release" => {
                let reason = s(&d["reason"]).unwrap_or_else(|| "released".into());
                // A release names its pane; an old one that doesn't is everyone's.
                let block = s(&d["block"]);
                // Another member's hub releases only its own agents.
                for x in self.agents.iter_mut().filter(|x| x.on() && x.member == member && block.as_ref().is_none_or(|b| *b == x.agent.block)) {
                    x.left_at = Some(e.at.clone());
                    x.left = Some(reason.clone());
                }
                // The docket is released when its last agent leaves.
                if !self.agents.iter().any(DocketAgent::on) {
                    self.released = Some(reason);
                }
                self.pick_agent();
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
        // What an agent did moves it to the front.
        if !matches!(e.kind.as_str(), "claim" | "release" | "open")
            && let Some(b) = block_of(e)
            && let Some(x) = self.agents.iter_mut().rev().find(|x| x.agent.block == b && x.member == member && x.on())
        {
            x.active_at = e.at.clone();
            self.pick_agent();
        }
        self.updated_at = e.at.clone();
        self.entries = seq;
        self.head = chain.to_string();
    }

    /// An agent joins, or one already on the docket claims again (a new
    /// session, a resume). A claim that doesn't say its role keeps the one
    /// it had.
    #[allow(clippy::too_many_arguments)]
    fn join(&mut self, a: AgentRef, member: Option<String>, session: Option<String>, role: Option<String>, round: Option<i64>, via: Option<String>, at: &str) {
        match self.agents.iter_mut().rev().find(|x| x.agent.block == a.block && x.member == member) {
            Some(x) => {
                x.agent = a;
                x.session = session;
                x.role = role.or(x.role.take());
                x.round = round.or(x.round);
                x.active_at = at.to_string();
                if x.left_at.is_some() {
                    x.left_at = None;
                    x.left = None;
                    x.joined_at = at.to_string();
                    // A move on purpose (wh join, wh leave) is how it came
                    // on; a resume of it in the same pane isn't a new way in.
                    if matches!(via.as_deref(), Some("join" | "leave")) || x.via.is_none() {
                        x.via = via;
                    }
                }
            }
            None => self.agents.push(DocketAgent { agent: a, session, role, round, via, member, joined_at: at.into(), active_at: at.into(), left_at: None, left: None }),
        }
        self.pick_agent();
    }

    /// `agent` and `session`: the most recently active agent still on the
    /// docket, or, with none on it, the last to be active.
    fn pick_agent(&mut self) {
        let on = self.agents.iter().filter(|x| x.on()).max_by(|a, b| a.active_at.cmp(&b.active_at));
        let pick = on.or_else(|| self.agents.iter().max_by(|a, b| a.active_at.cmp(&b.active_at)));
        if let Some(x) = pick {
            self.agent = Some(x.agent.clone());
            self.session = x.session.clone();
        }
    }

    /// The folder the docket's work is in, for its checks and evidence: the
    /// first agent still on it (who it was opened for or who joined first),
    /// else the first it ever had, else the docket's agent. Not whoever spoke last, which on a docket
    /// with agents in different checkouts would move the checks around.
    pub fn home_cwd(&self) -> Option<&str> {
        self.agents_on().find_map(|a| a.agent.cwd.as_deref()).or_else(|| self.agents.iter().find_map(|a| a.agent.cwd.as_deref())).or_else(|| self.agent.as_ref()?.cwd.as_deref())
    }

    /// The agents on the docket now, every member's.
    pub fn agents_on(&self) -> impl Iterator<Item = &DocketAgent> {
        self.agents.iter().filter(|x| x.on())
    }

    /// The agents on the docket now that are this hub's own: on a shared
    /// docket, those its own hub claimed (`me` is this hub's member handle),
    /// not a teammate's. Only these are this hub's to track, release,
    /// message or approve.
    pub fn own_agents_on<'a>(&'a self, me: Option<&'a str>) -> impl Iterator<Item = &'a DocketAgent> {
        self.agents_on().filter(move |x| x.member.is_none() || x.member.as_deref() == me)
    }

    pub fn short_head(&self) -> &str {
        &self.head[..self.head.len().min(8)]
    }

    pub fn is_open(&self) -> bool {
        !matches!(self.status.as_str(), "done" | "dropped")
    }
}

/// The fold's version. When it changes (a field the state didn't have), the
/// hub folds every docket again once, at start, so the saved copies match.
pub const VERSION: u32 = 3;

/// Fold a whole docket from its entries.
pub fn fold<'a>(id: &str, entries: impl IntoIterator<Item = (i64, &'a str, &'a Entry)>) -> DocketState {
    let mut st = DocketState::new(id);
    for (seq, chain, e) in entries {
        st.apply(seq, chain, e);
    }
    st
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::entry::Actor;
    use serde_json::json;

    fn e(kind: &str, at: &str, data: Value) -> Entry {
        let mut e = Entry::new("D-1", kind, at, Actor::new("hub", "hub@local"));
        e.data = data;
        e
    }

    fn agent(block: &str) -> Value {
        json!({ "app": "claude-code", "host": "local", "block": block, "rex_session": "wheelhouse", "pane": block })
    }

    fn run(entries: &[Entry]) -> DocketState {
        let chain = "00".repeat(32);
        fold("D-1", entries.iter().enumerate().map(|(i, e)| (i as i64 + 1, chain.as_str(), e)))
    }

    fn on(st: &DocketState) -> Vec<&str> {
        st.agents_on().map(|a| a.agent.block.as_str()).collect()
    }

    #[test]
    fn an_old_single_agent_record_folds_as_before() {
        let st = run(&[
            e("open", "t1", json!({ "protocol": "agent", "title_source": "pane", "agent": agent("b1") })),
            e("claim", "t2", json!({ "session": "s-1", "source": "startup", "agent": agent("b1") })),
            e("turn", "t3", json!({ "state": "done" })),
            e("release", "t4", json!({ "reason": "pane closed", "block": "b1" })),
        ]);
        assert_eq!(st.agent.as_ref().unwrap().block, "b1");
        assert_eq!(st.session.as_deref(), Some("s-1"));
        assert_eq!(st.released.as_deref(), Some("pane closed"));
        assert_eq!(st.agents.len(), 1);
        assert_eq!(st.agents[0].left.as_deref(), Some("pane closed"));
        assert!(on(&st).is_empty());

        // An older release named no pane: everyone left.
        let st = run(&[e("claim", "t1", json!({ "session": "s-1", "agent": agent("b1") })), e("release", "t2", json!({ "reason": "agent exited" }))]);
        assert_eq!(st.released.as_deref(), Some("agent exited"));

        // A resume in a new pane: the new pane is the agent, and it holds the docket.
        let st = run(&[
            e("claim", "t1", json!({ "session": "s-1", "agent": agent("b1") })),
            e("release", "t2", json!({ "reason": "pane closed", "block": "b1" })),
            e("claim", "t3", json!({ "session": "s-1", "source": "resume", "agent": agent("b2") })),
        ]);
        assert_eq!(st.agent.as_ref().unwrap().block, "b2");
        assert!(st.released.is_none());
        assert_eq!(on(&st), ["b2"]);
    }

    #[test]
    fn agents_join_and_leave_and_the_last_to_leave_releases() {
        let st = run(&[
            e("open", "t1", json!({ "protocol": "agent", "title_source": "person" })),
            e("claim", "t2", json!({ "source": "wheelhouse", "agent": agent("block:1") })),
            e("claim", "t3", json!({ "source": "join", "agent": agent("herdr:t2") })),
            e("claim", "t4", json!({ "session": "s-1", "source": "startup", "agent": agent("block:1") })),
        ]);
        assert_eq!(on(&st), ["block:1", "herdr:t2"]);
        assert_eq!(st.agent.as_ref().unwrap().block, "block:1", "the most recently active");
        assert_eq!(st.session.as_deref(), Some("s-1"));

        let mut later = vec![
            e("claim", "t2", json!({ "source": "wheelhouse", "agent": agent("block:1") })),
            e("claim", "t3", json!({ "source": "join", "agent": agent("herdr:t2") })),
            e("turn", "t5", json!({ "state": "done", "block": "herdr:t2" })),
        ];
        let st = run(&later);
        assert_eq!(st.agent.as_ref().unwrap().block, "herdr:t2", "a turn moves its agent to the front");

        later.push(e("release", "t6", json!({ "reason": "left", "block": "herdr:t2" })));
        let st = run(&later);
        assert_eq!(on(&st), ["block:1"]);
        assert!(st.released.is_none(), "one agent is still on it");
        assert_eq!(st.agent.as_ref().unwrap().block, "block:1");

        later.push(e("release", "t7", json!({ "reason": "pane closed", "block": "block:1" })));
        let st = run(&later);
        assert!(on(&st).is_empty());
        assert_eq!(st.released.as_deref(), Some("pane closed"));
        assert_eq!(st.agents.len(), 2, "both stay on the record, as having left");

        // One joins again: the docket is held again.
        later.push(e("claim", "t8", json!({ "source": "join", "agent": agent("herdr:t2") })));
        let st = run(&later);
        assert_eq!(on(&st), ["herdr:t2"]);
        assert!(st.released.is_none());
        assert_eq!(st.agents.iter().find(|a| a.agent.block == "herdr:t2").unwrap().joined_at, "t8");
    }

    #[test]
    fn a_role_outlives_a_claim_that_doesnt_say_it() {
        let st = run(&[
            e("claim", "t1", json!({ "source": "wheelhouse", "role": "Build builder", "agent": agent("b1") })),
            e("claim", "t2", json!({ "source": "wheelhouse", "role": "Build checker", "round": 2, "agent": agent("b2") })),
            e("claim", "t3", json!({ "session": "s-1", "source": "startup", "agent": agent("b1") })),
        ]);
        let role = |b: &str| st.agents.iter().find(|a| a.agent.block == b).and_then(|a| a.role.clone());
        assert_eq!(role("b1").as_deref(), Some("Build builder"));
        assert_eq!(role("b2").as_deref(), Some("Build checker"));
        assert_eq!(st.agents[1].round, Some(2));
    }
}
