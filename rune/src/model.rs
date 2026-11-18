//! The hub's answers, as the Mac app reads them (mac/Sources/Wheelhouse/
//! Models.swift): thin accessors over the JSON, so a field the hub adds or
//! drops never breaks the app. And what needs you (Store.swift's `inbox`),
//! and the few ways a time is written.

use std::collections::HashMap;

use chrono::{DateTime, Local, Utc};
use serde_json::Value;

pub fn text(v: &Value) -> Option<String> {
    v.as_str().map(str::trim).filter(|t| !t.is_empty()).map(str::to_string)
}
pub fn str_or<'a>(v: &'a Value, d: &'a str) -> &'a str {
    v.as_str().unwrap_or(d)
}
pub fn int(v: &Value) -> Option<i64> {
    v.as_i64()
}
pub fn flag(v: &Value) -> bool {
    v.as_bool().unwrap_or(false)
}
pub fn arr(v: &Value) -> &[Value] {
    v.as_array().map(Vec::as_slice).unwrap_or(&[])
}
pub fn strings(v: &Value) -> Vec<String> {
    arr(v).iter().filter_map(text).collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Status {
    Todo,
    InProgress,
    Done,
    Dropped,
}

impl Status {
    pub const ALL: [Status; 4] = [Status::Todo, Status::InProgress, Status::Done, Status::Dropped];
    pub fn parse(s: &str) -> Status {
        match s {
            "in_progress" => Status::InProgress,
            "done" => Status::Done,
            "dropped" => Status::Dropped,
            _ => Status::Todo,
        }
    }
    pub fn raw(self) -> &'static str {
        match self {
            Status::Todo => "todo",
            Status::InProgress => "in_progress",
            Status::Done => "done",
            Status::Dropped => "dropped",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Status::Todo => "To do",
            Status::InProgress => "In progress",
            Status::Done => "Done",
            Status::Dropped => "Dropped",
        }
    }
    pub fn is_open(self) -> bool {
        matches!(self, Status::Todo | Status::InProgress)
    }
}

/// What an agent is doing, from its program status in Rex.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LiveState {
    Working,
    Blocked,
    Error,
    Done,
    Idle,
    Starting,
}

impl LiveState {
    pub fn parse(s: &str) -> LiveState {
        match s {
            "working" => LiveState::Working,
            "blocked" => LiveState::Blocked,
            "error" => LiveState::Error,
            "done" => LiveState::Done,
            "starting" => LiveState::Starting,
            _ => LiveState::Idle,
        }
    }
    pub fn word(self) -> &'static str {
        match self {
            LiveState::Working => "Working",
            LiveState::Blocked => "Needs you",
            LiveState::Error => "Failed",
            LiveState::Done => "Finished",
            LiveState::Idle => "Idle",
            LiveState::Starting => "Starting",
        }
    }
}

pub fn app_name(app: Option<&str>) -> String {
    match app {
        Some("claude-code" | "claude") => "Claude Code".into(),
        Some("codex") => "Codex".into(),
        Some(a) if !a.is_empty() => a.into(),
        _ => "Agent".into(),
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Live {
    pub j: Value,
}

impl Live {
    pub fn record(&self) -> Option<String> {
        self.j["record"].as_str().map(str::to_string)
    }
    pub fn answerable(&self) -> bool {
        flag(&self.j["answerable"])
    }
    pub fn app(&self) -> String {
        str_or(&self.j["app"], "").to_string()
    }
    pub fn state(&self) -> LiveState {
        LiveState::parse(str_or(&self.j["state"], ""))
    }
    pub fn kind(&self) -> Option<String> {
        text(&self.j["kind"])
    }
    pub fn msg(&self) -> Option<String> {
        text(&self.j["msg"])
    }
    pub fn since(&self) -> Option<String> {
        text(&self.j["since"])
    }
    pub fn rex_session(&self) -> String {
        str_or(&self.j["rex_session"], "").to_string()
    }
    pub fn pane(&self) -> String {
        str_or(&self.j["pane"], "").to_string()
    }
    pub fn cwd(&self) -> Option<String> {
        text(&self.j["cwd"])
    }
    pub fn docket(&self) -> Option<String> {
        text(&self.j["docket"])
    }
    pub fn block(&self) -> String {
        str_or(&self.j["block"], "").to_string()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Docket {
    pub j: Value,
}

impl Docket {
    pub fn id(&self) -> String {
        str_or(&self.j["id"], "").to_string()
    }
    pub fn title(&self) -> String {
        text(&self.j["title"]).unwrap_or_else(|| self.id())
    }
    pub fn status(&self) -> Status {
        Status::parse(str_or(&self.j["status"], ""))
    }
    pub fn protocol(&self) -> String {
        str_or(&self.j["protocol"], "none").to_string()
    }
    pub fn agent_app(&self) -> Option<String> {
        text(&self.j["agent"]["app"])
    }
    pub fn agent_cwd(&self) -> Option<String> {
        text(&self.j["agent"]["cwd"])
    }
    pub fn rex_session(&self) -> Option<String> {
        text(&self.j["agent"]["rex_session"])
    }
    pub fn agent_pane(&self) -> Option<String> {
        text(&self.j["agent"]["pane"])
    }
    pub fn has_agent(&self) -> bool {
        self.j["agent"].is_object()
    }
    pub fn session(&self) -> Option<String> {
        text(&self.j["session"])
    }
    pub fn prompts(&self) -> i64 {
        int(&self.j["prompts"]).unwrap_or(0)
    }
    pub fn turns(&self) -> i64 {
        int(&self.j["turns"]).unwrap_or(0)
    }
    pub fn last_turn(&self) -> Option<String> {
        text(&self.j["last_turn"])
    }
    pub fn created_at(&self) -> String {
        str_or(&self.j["created_at"], "").to_string()
    }
    pub fn updated_at(&self) -> String {
        str_or(&self.j["updated_at"], "").to_string()
    }
    pub fn short_head(&self) -> String {
        str_or(&self.j["short_head"], "").to_string()
    }
    pub fn live(&self) -> Option<Live> {
        self.j["live"].is_object().then(|| Live { j: self.j["live"].clone() })
    }
    pub fn intent_body(&self) -> Option<String> {
        text(&self.j["intent"]["body"])
    }
    pub fn intent_versions(&self) -> i64 {
        int(&self.j["intent_versions"]).unwrap_or(0)
    }
    pub fn linear_key(&self) -> Option<String> {
        text(&self.j["linear"]["key"]).or_else(|| text(&self.j["linear"]["identifier"]))
    }
    pub fn linear_state(&self) -> Option<String> {
        text(&self.j["linear"]["state"])
    }
    pub fn review_key(&self) -> Option<String> {
        text(&self.j["review"]["key"])
    }
    pub fn is_open(&self) -> bool {
        self.status().is_open()
    }
    pub fn hold(&self) -> Option<String> {
        text(&self.j["hold"])
    }
    pub fn closed_on(&self) -> Option<String> {
        text(&self.j["closed_on"])
    }
    pub fn talks(&self) -> bool {
        self.protocol() == "agent" && self.has_agent()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Msg {
    pub j: Value,
}

impl Msg {
    pub fn id(&self) -> i64 {
        int(&self.j["id"]).unwrap_or(0)
    }
    pub fn docket(&self) -> Option<String> {
        text(&self.j["docket"])
    }
    pub fn from(&self) -> String {
        str_or(&self.j["from"], "").to_string()
    }
    pub fn from_app(&self) -> Option<String> {
        text(&self.j["from_app"])
    }
    pub fn subject(&self) -> Option<String> {
        text(&self.j["subject"])
    }
    pub fn body(&self) -> String {
        str_or(&self.j["body"], "").to_string()
    }
    pub fn options(&self) -> Vec<String> {
        strings(&self.j["options"])
    }
    pub fn created_at(&self) -> String {
        str_or(&self.j["created_at"], "").to_string()
    }
    pub fn who(&self) -> String {
        if self.from() == "wheelhouse" { "Wheelhouse".into() } else { self.from_app().unwrap_or_else(|| app_name(Some(&self.from()))) }
    }
    pub fn title(&self) -> String {
        self.subject().or_else(|| self.body().lines().map(str::trim).find(|l| !l.is_empty()).map(str::to_string)).unwrap_or_else(|| format!("Message #{}", self.id()))
    }
}

/// A Review's or a Build's call for you.
#[derive(Clone, Debug, PartialEq)]
pub struct Attention {
    pub kind: String,
    pub text: String,
    pub round: i64,
}

impl Attention {
    fn parse(v: &Value) -> Option<Attention> {
        v.is_object().then(|| Attention { kind: str_or(&v["kind"], "").into(), text: str_or(&v["text"], "").into(), round: int(&v["round"]).unwrap_or(0) })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ReviewPr {
    pub j: Value,
}

impl ReviewPr {
    pub fn key(&self) -> String {
        str_or(&self.j["key"], "").to_string()
    }
    pub fn full_key(&self) -> String {
        text(&self.j["full_key"]).unwrap_or_else(|| self.key())
    }
    pub fn url(&self) -> Option<String> {
        text(&self.j["url"])
    }
    pub fn title(&self) -> Option<String> {
        text(&self.j["title"])
    }
    pub fn author(&self) -> Option<String> {
        text(&self.j["author"])
    }
    pub fn state(&self) -> String {
        str_or(&self.j["state"], "").to_string()
    }
    pub fn phase(&self) -> Option<String> {
        text(&self.j["phase"])
    }
    pub fn round(&self) -> i64 {
        int(&self.j["round"]).unwrap_or(0)
    }
    pub fn verdict(&self) -> Option<String> {
        text(&self.j["verdict"])
    }
    pub fn merge_risk(&self) -> Option<String> {
        text(&self.j["merge_risk"])
    }
    pub fn docket(&self) -> Option<String> {
        text(&self.j["docket"])
    }
    pub fn reviewers_done(&self) -> i64 {
        int(&self.j["reviewers_done"]).unwrap_or(0)
    }
    pub fn reviewers(&self) -> Vec<(String, String, Option<String>)> {
        arr(&self.j["reviewers"]).iter().map(|r| (str_or(&r["name"], "").into(), str_or(&r["agent"], "").into(), text(&r["effort"]))).collect()
    }
    pub fn reviewers_total(&self) -> i64 {
        int(&self.j["reviewers_total"]).unwrap_or(self.reviewers().len() as i64)
    }
    pub fn attention(&self) -> Option<Attention> {
        Attention::parse(&self.j["attention"])
    }
    pub fn updated_at(&self) -> Option<String> {
        text(&self.j["updated_at"])
    }
    pub fn posted_at(&self) -> Option<String> {
        text(&self.j["posted_at"])
    }
    pub fn history(&self) -> Vec<Value> {
        arr(&self.j["history"]).to_vec()
    }
    pub fn draft(&self) -> Option<Value> {
        self.j["draft"].is_object().then(|| self.j["draft"].clone())
    }
    pub fn digest(&self) -> Option<String> {
        text(&self.j["draft"]["digest"])
    }
    pub fn ready(&self) -> bool {
        self.state() == "reviewing" && self.phase().as_deref() == Some("awaiting")
    }
    pub fn post(&self) -> Option<String> {
        text(&self.j["post"])
    }
    pub fn waits_why(&self) -> Option<String> {
        let d = &self.j["post_decision"];
        if let Some(r) = text(&d["refused"]) {
            return Some(format!("your rule would post it, but the post was refused: {r}"));
        }
        (d.is_object() && !flag(&d["auto"])).then(|| text(&d["why"])).flatten()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct BuildTask {
    pub j: Value,
}

impl BuildTask {
    pub fn docket(&self) -> String {
        str_or(&self.j["docket"], "").to_string()
    }
    pub fn key(&self) -> String {
        str_or(&self.j["key"], "").to_string()
    }
    pub fn title(&self) -> String {
        str_or(&self.j["title"], "").to_string()
    }
    pub fn repo(&self) -> String {
        str_or(&self.j["repo"], "").to_string()
    }
    pub fn state(&self) -> String {
        str_or(&self.j["state"], "").to_string()
    }
    pub fn phase(&self) -> Option<String> {
        text(&self.j["phase"])
    }
    pub fn branch(&self) -> Option<String> {
        text(&self.j["branch"])
    }
    pub fn attention(&self) -> Option<Attention> {
        Attention::parse(&self.j["attention"])
    }
    pub fn updated_at(&self) -> Option<String> {
        text(&self.j["updated_at"])
    }
    pub fn pr_number(&self) -> Option<i64> {
        int(&self.j["pr_number"])
    }
    pub fn pr_url(&self) -> Option<String> {
        text(&self.j["pr_url"])
    }
    pub fn needs_you(&self) -> bool {
        self.attention().is_some()
    }
    pub fn finished(&self) -> bool {
        matches!(self.state().as_str(), "merged" | "closed" | "discarded" | "failed")
    }
}

/// Something worth a look that isn't an ask (src/evidence.rs `attention`).
#[derive(Clone, Debug, PartialEq)]
pub struct Concern {
    pub docket: Option<String>,
    pub title: String,
    pub kind: String,
    pub text: String,
    pub at: Option<String>,
}

impl Concern {
    pub fn id(&self) -> String {
        format!("at:{}:{}", self.docket.clone().unwrap_or_else(|| self.title.clone()), self.kind)
    }
    pub fn label(&self) -> String {
        match self.kind.as_str() {
            "build" => "Build".into(),
            "review" => "Review".into(),
            "failing" => "Failing".into(),
            "held" => "On hold".into(),
            "long" => "Working long".into(),
            "quiet" => "Quiet".into(),
            "word" => "Your word".into(),
            k => k.to_string(),
        }
    }
}

/// What `/api/state` says: every docket, what needs you, and the protocols.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct HubState {
    pub j: Value,
    pub dockets: Vec<Docket>,
    pub messages: Vec<Msg>,
    pub prs: Vec<ReviewPr>,
    pub tasks: Vec<BuildTask>,
    by_id: HashMap<String, usize>,
}

impl HubState {
    pub fn parse(j: Value) -> HubState {
        let dockets: Vec<Docket> = arr(&j["dockets"]).iter().map(|d| Docket { j: d.clone() }).collect();
        let by_id = dockets.iter().enumerate().map(|(i, d)| (d.id(), i)).collect();
        HubState {
            messages: arr(&j["messages"]).iter().map(|m| Msg { j: m.clone() }).collect(),
            prs: arr(&j["review"]["prs"]).iter().map(|p| ReviewPr { j: p.clone() }).collect(),
            tasks: arr(&j["build"]["tasks"]).iter().map(|t| BuildTask { j: t.clone() }).collect(),
            dockets,
            by_id,
            j,
        }
    }
    pub fn docket(&self, id: &str) -> Option<&Docket> {
        self.by_id.get(id).map(|&i| &self.dockets[i])
    }
    pub fn owner(&self) -> String {
        str_or(&self.j["owner"], "you").to_string()
    }
    pub fn hooks(&self) -> bool {
        flag(&self.j["hooks"])
    }
    pub fn linear(&self) -> bool {
        flag(&self.j["linear"])
    }
    pub fn rex_connected(&self) -> bool {
        flag(&self.j["rex"]["connected"])
    }
    pub fn rex_error(&self) -> Option<String> {
        text(&self.j["rex"]["error"])
    }
    pub fn flavor(&self) -> Option<String> {
        text(&self.j["flavor"]["name"])
    }
    pub fn flavor_build(&self) -> Option<String> {
        let f = &self.j["flavor"];
        f.is_object().then(|| format!("{} {}", str_or(&f["branch"], ""), str_or(&f["commit"], "").chars().take(7).collect::<String>()))
    }
    pub fn agents(&self) -> Vec<Live> {
        arr(&self.j["agents"]).iter().map(|a| Live { j: a.clone() }).collect()
    }
    pub fn review(&self) -> &Value {
        &self.j["review"]
    }
    pub fn build(&self) -> &Value {
        &self.j["build"]
    }
    pub fn task_for(&self, docket: &str) -> Option<&BuildTask> {
        self.tasks.iter().find(|t| t.docket() == docket)
    }
    pub fn pr_for(&self, docket: &str) -> Option<&ReviewPr> {
        self.prs.iter().find(|p| p.docket().as_deref() == Some(docket))
    }
    pub fn attention(&self) -> Vec<Concern> {
        arr(&self.j["attention"])
            .iter()
            .map(|c| Concern {
                docket: text(&c["docket"]),
                title: str_or(&c["title"], "").into(),
                kind: str_or(&c["kind"], "").into(),
                text: str_or(&c["text"], "").into(),
                at: text(&c["at"]),
            })
            .collect()
    }
    /// The Rex sessions open dockets' agents run in, and how many each.
    pub fn sessions(&self) -> Vec<(String, usize)> {
        let mut m: std::collections::BTreeMap<String, usize> = Default::default();
        for d in self.dockets.iter().filter(|d| d.is_open()) {
            if let Some(s) = d.rex_session() {
                *m.entry(s).or_default() += 1;
            }
        }
        m.into_iter().collect()
    }
}

/// One thing in the Inbox: a prompt, a review to post, a message, a failure,
/// or a finished turn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum InboxKind {
    Blocked,
    Review,
    Message,
    Error,
    Done,
}

#[derive(Clone, Debug, PartialEq)]
pub struct InboxItem {
    pub kind: InboxKind,
    pub docket: Option<Docket>,
    pub docket_id: String,
    pub title: String,
    pub line: String,
    pub at: Option<String>,
    pub msg: Option<Msg>,
    pub pr: Option<ReviewPr>,
    pub id: String,
}

fn first_line(s: Option<String>) -> Option<String> {
    s.and_then(|t| t.lines().map(str::trim).find(|l| !l.is_empty()).map(|l| l.trim_start_matches('#').trim().to_string()))
}

/// A ready review's draft, in a line: its verdict and findings.
pub fn draft_line(p: &ReviewPr) -> Option<String> {
    let d = p.draft()?;
    let verdict = text(&d["verdict"]).map(|v| v.replace('_', " "));
    let b = int(&d["blocking"]).unwrap_or(0);
    let nb = int(&d["non_blocking"]).unwrap_or(0);
    let mut parts = vec![];
    if let Some(v) = verdict {
        parts.push(v);
    }
    parts.push(format!("{b} blocking · {nb} non-blocking"));
    if let Some(r) = text(&d["merge_risk"]) {
        parts.push(format!("risk {r}"));
    }
    Some(parts.join(" · "))
}

/// What needs you, in the Inbox's order: prompts, reviews, messages,
/// failures, then finished turns; newest first in each (Store.swift).
pub fn inbox(state: &HubState) -> Vec<InboxItem> {
    let mut items = vec![];
    let label = |k: &str| match k {
        "ready" => "Ready to post",
        "blocked" => "Blocked",
        "start-failed" => "Didn't start",
        "stalled" => "Stalled",
        "pane-closed" => "Pane closed",
        "error" => "Failed",
        _ => "Needs you",
    };
    for p in &state.prs {
        let Some(a) = p.attention() else { continue };
        let d = p.docket().and_then(|id| state.docket(&id).cloned());
        let what = if a.kind == "ready" { draft_line(p).unwrap_or(a.text.clone()) } else { a.text.clone() };
        items.push(InboxItem {
            kind: InboxKind::Review,
            docket_id: p.docket().unwrap_or_else(|| p.key()),
            title: d.as_ref().map(|d| d.title()).unwrap_or_else(|| format!("Review {}{}", p.key(), p.title().map(|t| format!(": {t}")).unwrap_or_default())),
            line: format!("{} (round {}): {what}", label(&a.kind), a.round),
            at: p.updated_at(),
            msg: None,
            pr: Some(p.clone()),
            id: format!("rv:{}", p.full_key()),
            docket: d,
        });
    }
    for m in &state.messages {
        let d = m.docket().and_then(|id| state.docket(&id).cloned());
        items.push(InboxItem {
            kind: InboxKind::Message,
            docket_id: m.docket().unwrap_or_else(|| format!("#{}", m.id())),
            title: d.as_ref().map(|d| d.title()).unwrap_or_else(|| m.title()),
            line: format!("{}: {}", m.who(), m.title()),
            at: Some(m.created_at()),
            msg: Some(m.clone()),
            pr: None,
            id: format!("m{}", m.id()),
            docket: d,
        });
    }
    for d in &state.dockets {
        let Some(l) = d.live() else { continue };
        let (kind, line, at) = match l.state() {
            LiveState::Blocked => (InboxKind::Blocked, l.msg().unwrap_or_else(|| format!("{} needs you", app_name(d.agent_app().as_deref()))), l.since()),
            LiveState::Error if d.is_open() => (InboxKind::Error, l.msg().unwrap_or_else(|| "Failed".into()), l.since()),
            LiveState::Done if d.is_open() => (InboxKind::Done, first_line(d.last_turn()).unwrap_or_else(|| "Finished its turn".into()), l.since().or(Some(d.updated_at()))),
            _ => continue,
        };
        items.push(InboxItem { kind, docket: Some(d.clone()), docket_id: d.id(), title: d.title(), line, at, msg: None, pr: None, id: d.id() });
    }
    items.sort_by(|a, b| a.kind.cmp(&b.kind).then(b.at.cmp(&a.at)));
    items
}

/// The hub's attention list, less what's already an ask.
pub fn concerns(state: &HubState, inbox: &[InboxItem]) -> Vec<Concern> {
    state
        .attention()
        .into_iter()
        .filter(|c| {
            let asked = c.docket.as_ref().is_some_and(|d| inbox.iter().any(|i| &i.docket_id == d && matches!(i.kind, InboxKind::Review | InboxKind::Message)));
            !(asked && matches!(c.kind.as_str(), "review" | "build"))
        })
        .collect()
}

/// How many need you right now: the Dock and menu bar count.
pub fn urgent(state: &HubState) -> usize {
    inbox(state).iter().filter(|i| matches!(i.kind, InboxKind::Blocked | InboxKind::Review | InboxKind::Message | InboxKind::Error)).count()
}

// ---- a docket's page ----

/// One entry of a docket's record.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub j: Value,
}

impl Entry {
    pub fn seq(&self) -> i64 {
        int(&self.j["seq"]).unwrap_or(0)
    }
    pub fn kind(&self) -> String {
        str_or(&self.j["kind"], "").to_string()
    }
    pub fn at(&self) -> String {
        str_or(&self.j["at"], "").to_string()
    }
    pub fn actor_type(&self) -> String {
        str_or(&self.j["actor"]["type"], "").to_string()
    }
    pub fn actor_app(&self) -> String {
        str_or(&self.j["actor"]["id"], "").split('@').next().unwrap_or("").to_string()
    }
    pub fn body(&self) -> Option<String> {
        self.j["body"].as_str().map(str::to_string)
    }
    pub fn data(&self) -> &Value {
        &self.j["data"]
    }
    pub fn mark(&self) -> String {
        str_or(&self.j["mark"], "said").to_string()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct DocketPage {
    pub j: Value,
    pub docket: Docket,
    pub entries: Vec<Entry>,
}

impl DocketPage {
    pub fn parse(j: Value) -> DocketPage {
        DocketPage { docket: Docket { j: j["docket"].clone() }, entries: arr(&j["entries"]).iter().map(|e| Entry { j: e.clone() }).collect(), j }
    }
    pub fn verify_ok(&self) -> bool {
        flag(&self.j["verify"]["ok"])
    }
    pub fn verify_entries(&self) -> i64 {
        int(&self.j["verify"]["entries"]).unwrap_or(0)
    }
    pub fn evidence(&self) -> &Value {
        &self.j["evidence"]
    }
    pub fn decisions(&self) -> &[Value] {
        arr(&self.j["decisions"])
    }
    pub fn queued(&self) -> Vec<(i64, String)> {
        arr(&self.j["queued"]).iter().map(|q| (int(&q["id"]).unwrap_or(0), str_or(&q["body"], "").to_string())).collect()
    }
}

/// How long a check took: 153 ms, 4.2 s, 3m 05s.
pub fn duration(ms: Option<i64>) -> String {
    let Some(ms) = ms else { return String::new() };
    if ms < 1000 {
        format!("{ms} ms")
    } else if ms < 60000 {
        format!("{:.1} s", ms as f64 / 1000.0)
    } else {
        let s = ms / 1000;
        format!("{}m {:02}s", s / 60, s % 60)
    }
}

// ---- times ----

pub fn date(iso: Option<&str>) -> Option<DateTime<Utc>> {
    let iso = iso?.trim();
    if iso.is_empty() {
        return None;
    }
    DateTime::parse_from_rfc3339(iso).ok().map(|d| d.with_timezone(&Utc))
}

/// How long ago, in the list's few characters: now, 5m, 3h, 2d, Mar 4.
pub fn ago(iso: Option<&str>) -> String {
    let Some(d) = date(iso) else { return String::new() };
    let s = (Utc::now() - d).num_seconds().max(0);
    if s < 60 {
        "now".into()
    } else if s < 3600 {
        format!("{}m", s / 60)
    } else if s < 86400 {
        format!("{}h", s / 3600)
    } else if s < 86400 * 7 {
        format!("{}d", s / 86400)
    } else {
        d.with_timezone(&Local).format("%b %-d").to_string()
    }
}

/// The same, in words: just now, 5m ago, Mar 4.
pub fn ago_long(iso: Option<&str>) -> String {
    let a = ago(iso);
    if a.is_empty() {
        return a;
    }
    if a == "now" {
        return "just now".into();
    }
    if a.chars().next().is_some_and(|c| c.is_ascii_digit()) && a.ends_with(['m', 'h', 'd']) { format!("{a} ago") } else { a }
}

/// A timestamp: Nov 18, 14:05.
pub fn stamp(iso: Option<&str>) -> String {
    match date(iso) {
        Some(d) => d.with_timezone(&Local).format("%b %-d, %H:%M").to_string(),
        None => iso.unwrap_or("").to_string(),
    }
}

pub fn plural(n: usize, word: &str) -> String {
    format!("{n} {word}{}", if n == 1 { "" } else { "s" })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn inbox_orders_prompts_then_messages_then_finished() {
        let s = HubState::parse(json!({
            "dockets": [
                {"id": "D-1", "title": "a", "status": "in_progress", "live": {"state": "done", "since": "2026-11-18T10:00:00Z"}},
                {"id": "D-2", "title": "b", "status": "in_progress", "live": {"state": "blocked", "msg": "Bash: ls", "since": "2026-11-18T09:00:00Z"}},
                {"id": "D-3", "title": "c", "status": "done", "live": {"state": "done"}}
            ],
            "messages": [{"id": 4, "docket": "D-1", "subject": "Which?", "options": ["x"], "created_at": "2026-11-18T11:00:00Z"}]
        }));
        let items = inbox(&s);
        let ids: Vec<&str> = items.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids, ["D-2", "m4", "D-1"], "a closed docket's finished turn isn't asking");
        assert_eq!(items[0].line, "Bash: ls");
    }

    #[test]
    fn times_read_as_the_lists_write_them() {
        assert_eq!(duration(Some(153)), "153 ms");
        assert_eq!(duration(Some(4200)), "4.2 s");
        assert_eq!(duration(Some(185_000)), "3m 05s");
        assert_eq!(ago(None), "");
        assert_eq!(plural(1, "repo"), "1 repo");
    }
}
