//! Linear, through its GraphQL API with a personal API key (read-only).
//!
//! A docket can start from a Linear issue or have one linked. The issue's
//! description seeds the docket's intent, and the hub keeps following the
//! issue: changes to its title, state, assignee and description, and new
//! comments, come onto the docket as entries read from Linear (checkable).

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::Duration;

use crate::hub::{Shared, Who};

const KEYCHAIN: &str = "wheelhouse-linear";
/// rex-develop's entry, used if Wheelhouse has none of its own.
const KEYCHAIN_REX_DEVELOP: &str = "rex-develop-linear";

fn endpoint() -> String {
    std::env::var("WH_LINEAR_ENDPOINT").unwrap_or_else(|_| "https://api.linear.app/graphql".into())
}

/// The API key: `WH_LINEAR_KEY` or `LINEAR_API_KEY`, else the Keychain
/// (none with `WH_LINEAR_OFF`).
pub fn key() -> Option<String> {
    if std::env::var_os("WH_LINEAR_OFF").is_some() {
        return None;
    }
    for var in ["WH_LINEAR_KEY", "LINEAR_API_KEY"] {
        if let Ok(k) = std::env::var(var)
            && !k.trim().is_empty()
        {
            return Some(k.trim().to_string());
        }
    }
    for service in [KEYCHAIN, KEYCHAIN_REX_DEVELOP] {
        let out = std::process::Command::new("security").args(["find-generic-password", "-s", service, "-w"]).output().ok()?;
        if out.status.success() {
            let k = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !k.is_empty() {
                return Some(k);
            }
        }
    }
    None
}

/// `wh setup-linear`: store a key in the Keychain. `security` asks for it,
/// so the key never appears on a command line.
pub fn setup() -> Result<()> {
    println!("Paste a Linear personal API key (Linear › Settings › Security & access › Personal API keys).");
    println!("Read access is enough. The Keychain asks for it twice.");
    let st = std::process::Command::new("security").args(["add-generic-password", "-U", "-s", KEYCHAIN, "-a", "linear", "-w"]).status()?;
    if !st.success() {
        bail!("the Keychain didn't store the key");
    }
    println!("Stored as {KEYCHAIN} in your login Keychain. Restart the hub to pick it up: wh up (or wh serve).");
    Ok(())
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Issue {
    pub id: String,
    pub identifier: String,
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    pub url: String,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub state_type: Option<String>,
    #[serde(default)]
    pub priority: Option<String>,
    #[serde(default)]
    pub assignee: Option<String>,
    #[serde(default)]
    pub team: Option<String>,
    #[serde(default)]
    pub branch: Option<String>,
    pub updated_at: String,
    #[serde(default)]
    pub comments: Vec<Comment>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Comment {
    pub id: String,
    pub author: String,
    pub body: String,
    pub created_at: String,
}

const FIELDS: &str = "id identifier title description url priorityLabel branchName updatedAt
  state { name type } assignee { name } team { key }
  comments(first: 50) { nodes { id body createdAt user { name } } }";

const LITE: &str = "id identifier title url priorityLabel updatedAt state { name type } assignee { name } team { key }";

fn parse(v: &Value) -> Option<Issue> {
    let s = |p: &str| v.pointer(p).and_then(Value::as_str).map(str::to_string);
    let mut comments: Vec<Comment> = v["comments"]["nodes"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|c| {
                    Some(Comment {
                        id: c["id"].as_str()?.to_string(),
                        author: c["user"]["name"].as_str().unwrap_or("someone").to_string(),
                        body: c["body"].as_str().unwrap_or_default().to_string(),
                        created_at: c["createdAt"].as_str()?.to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    comments.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    Some(Issue {
        id: s("/id")?,
        identifier: s("/identifier")?,
        title: s("/title").unwrap_or_default(),
        description: s("/description").filter(|d| !d.trim().is_empty()),
        url: s("/url").unwrap_or_default(),
        state: s("/state/name"),
        state_type: s("/state/type"),
        priority: s("/priorityLabel"),
        assignee: s("/assignee/name"),
        team: s("/team/key"),
        branch: s("/branchName"),
        updated_at: s("/updatedAt").unwrap_or_default(),
        comments,
    })
}

async fn graphql(query: &str, variables: Value) -> Result<Value> {
    let key = key().ok_or_else(|| anyhow!("no Linear key: run `wh setup-linear`"))?;
    let resp = reqwest::Client::new()
        .post(endpoint())
        .header("Authorization", key)
        .header("Content-Type", "application/json")
        .timeout(Duration::from_secs(20))
        .json(&json!({ "query": query, "variables": variables }))
        .send()
        .await
        .context("reaching Linear")?;
    let status = resp.status();
    let v: Value = resp.json().await.context("Linear's answer isn't JSON")?;
    if let Some(e) = v["errors"].as_array().and_then(|a| a.first()) {
        bail!("Linear: {}", e["message"].as_str().unwrap_or("an error"));
    }
    if !status.is_success() {
        bail!("Linear answered {status}");
    }
    Ok(v["data"].clone())
}

/// One issue, by its key (ENG-142) or id.
pub async fn issue(key: &str) -> Result<Issue> {
    let q = format!("query($id: String!) {{ issue(id: $id) {{ {FIELDS} }} }}");
    let data = graphql(&q, json!({ "id": key })).await?;
    parse(&data["issue"]).ok_or_else(|| anyhow!("Linear has no issue {key}"))
}

/// Issues to start from: a key looks it up; otherwise your open assigned
/// issues, and issues whose title has the words.
pub async fn search(q: &str) -> Result<Vec<Issue>> {
    let q = q.trim();
    if is_key(q) {
        return Ok(vec![issue(q).await?]);
    }
    let query = format!(
        "query($q: String!, $any: Boolean!) {{
           viewer {{ assignedIssues(first: 40, orderBy: updatedAt, filter: {{ state: {{ type: {{ nin: [\"completed\", \"canceled\"] }} }} }}) {{ nodes {{ {LITE} }} }} }}
           issues(first: 20, orderBy: updatedAt, filter: {{ title: {{ containsIgnoreCase: $q }} }}) @include(if: $any) {{ nodes {{ {LITE} }} }}
         }}"
    );
    let data = graphql(&query, json!({ "q": q, "any": !q.is_empty() })).await?;
    let words: Vec<String> = q.to_lowercase().split_whitespace().map(str::to_string).collect();
    let mut out: Vec<Issue> = Vec::new();
    for v in data["viewer"]["assignedIssues"]["nodes"].as_array().into_iter().flatten().chain(data["issues"]["nodes"].as_array().into_iter().flatten()) {
        let Some(i) = parse(v) else { continue };
        let hay = format!("{} {}", i.identifier, i.title).to_lowercase();
        if words.iter().all(|w| hay.contains(w)) && !out.iter().any(|o| o.id == i.id) {
            out.push(i);
        }
    }
    Ok(out)
}

pub fn is_key(s: &str) -> bool {
    let Some((team, n)) = s.split_once('-') else { return false };
    !team.is_empty() && team.chars().all(|c| c.is_ascii_alphanumeric()) && !n.is_empty() && n.chars().all(|c| c.is_ascii_digit())
}

fn snapshot(i: &Issue) -> Value {
    json!({ "key": i.identifier, "id": i.id, "url": i.url, "title": i.title, "state": i.state, "state_type": i.state_type,
            "priority": i.priority, "assignee": i.assignee, "team": i.team, "branch": i.branch, "updated_at": i.updated_at })
}

/// Link an issue to a docket: the link, what Linear says now, its comments,
/// and the intent if the docket has none yet.
pub fn link(core: &mut crate::hub::Core, docket: &str, i: &Issue, by: &Who) -> Result<()> {
    core.append(docket, "link", by, Some(format!("{}: {}", i.identifier, i.title)), json!({ "kind": "linear", "source": "linear", "key": i.identifier, "id": i.id, "url": i.url, "title": i.title }))?;
    core.append(docket, "observed", &Who::Hub, None, json!({ "what": "linear", "source": "linear", "issue": snapshot(i), "description": i.description }))?;
    let has_intent = core.store.state(docket)?.is_some_and(|s| s.intent.is_some());
    if !has_intent && let Some(d) = &i.description {
        core.append(docket, "intent", &Who::Hub, Some(d.clone()), json!({ "source": "linear", "key": i.identifier, "updated_at": i.updated_at }))?;
    }
    for c in &i.comments {
        comment(core, docket, i, c)?;
    }
    Ok(())
}

/// What Linear says of an issue the hub linked for a Review: its state and
/// description, without its comments so far (only new ones come in).
pub fn observe(core: &mut crate::hub::Core, docket: &str, i: &Issue) -> Result<()> {
    let seen = i.comments.iter().map(|c| c.created_at.as_str()).max();
    core.append(docket, "observed", &Who::Hub, None, json!({ "what": "linear", "source": "linear", "issue": snapshot(i), "description": i.description, "comments_seen_at": seen }))?;
    Ok(())
}

fn comment(core: &mut crate::hub::Core, docket: &str, i: &Issue, c: &Comment) -> Result<()> {
    core.append(
        docket,
        "observed",
        &Who::Hub,
        Some(c.body.clone()),
        json!({ "what": "linear-comment", "source": "linear", "key": i.identifier, "comment": c.id, "author": c.author, "created_at": c.created_at }),
    )?;
    Ok(())
}

/// What changed on Linear since the docket last looked, as entries.
pub fn apply(core: &mut crate::hub::Core, docket: &str, i: &Issue) -> Result<usize> {
    let Some(st) = core.store.state(docket)? else { return Ok(0) };
    let Some(seen) = st.linear.clone() else { return Ok(0) };
    let mut n = 0;
    if seen.updated_at.as_deref() != Some(i.updated_at.as_str()) {
        let mut changed = Vec::new();
        if seen.title.as_deref() != Some(i.title.as_str()) {
            changed.push("title");
        }
        if seen.state != i.state {
            changed.push("state");
        }
        if seen.assignee != i.assignee {
            changed.push("assignee");
        }
        if seen.priority != i.priority {
            changed.push("priority");
        }
        let described = seen.description != i.description;
        if described {
            changed.push("description");
        }
        let from = json!({ "state": seen.state, "title": seen.title, "assignee": seen.assignee, "priority": seen.priority });
        let mut data = json!({ "what": "linear", "source": "linear", "issue": snapshot(i), "changed": changed, "from": from });
        if described {
            data["description"] = json!(i.description);
        }
        core.append(docket, "observed", &Who::Hub, None, data)?;
        n += 1;
        // The title follows Linear while it came from Linear.
        if changed.contains(&"title") && st.title_source == "linear" {
            core.append(docket, "set", &Who::Hub, None, json!({ "title": i.title, "title_source": "linear" }))?;
            n += 1;
        }
        // The intent follows Linear's description while the last word on it
        // was Linear's. Once someone edits it here, Linear's change is only
        // shown, to take or leave.
        if described
            && st.intent.as_ref().is_none_or(|t| t.source == "linear")
            && let Some(d) = &i.description
        {
            core.append(docket, "intent", &Who::Hub, Some(d.clone()), json!({ "source": "linear", "key": i.identifier, "updated_at": i.updated_at }))?;
            n += 1;
        }
    }
    for c in i.comments.iter().filter(|c| seen.comments_seen_at.as_deref().is_none_or(|t| c.created_at.as_str() > t)) {
        comment(core, docket, i, c)?;
        n += 1;
    }
    Ok(n)
}

/// Every two minutes, read each open docket's issue again.
pub async fn follow(shared: Shared) {
    let ready = tokio::task::spawn_blocking(|| key().is_some()).await.unwrap_or(false);
    shared.lock().linear_ready = ready;
    loop {
        tokio::time::sleep(Duration::from_secs(if std::env::var("WH_LINEAR_FAST").is_ok() { 2 } else { 120 })).await;
        let ready = tokio::task::spawn_blocking(|| key().is_some()).await.unwrap_or(false);
        shared.lock().linear_ready = ready;
        if !ready {
            continue;
        }
        let linked: Vec<(String, String)> = {
            let core = shared.lock();
            core.store.states().unwrap_or_default().into_iter().filter(|s| s.is_open()).filter_map(|s| Some((s.id.clone(), s.linear?.key))).collect()
        };
        for (docket, key) in linked {
            match issue(&key).await {
                Ok(i) => {
                    let mut core = shared.lock();
                    if let Err(e) = apply(&mut core, &docket, &i) {
                        say!("linear {key}: {e:#}");
                    }
                }
                Err(e) => say!("linear {key}: {e:#}"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hub::test_core;

    fn issue_v(desc: &str, state: &str, updated: &str, comments: &[(&str, &str)]) -> Issue {
        Issue {
            id: "uuid-142".into(),
            identifier: "ENG-142".into(),
            title: "Retry failed webhooks".into(),
            description: Some(desc.into()),
            url: "https://linear.app/acme/issue/ENG-142".into(),
            state: Some(state.into()),
            state_type: Some("unstarted".into()),
            priority: Some("High".into()),
            assignee: Some("You".into()),
            team: Some("ENG".into()),
            branch: Some("eng-142-retry".into()),
            updated_at: updated.into(),
            comments: comments.iter().enumerate().map(|(n, (at, body))| Comment { id: format!("c{n}"), author: "Sam".into(), body: body.to_string(), created_at: at.to_string() }).collect(),
        }
    }

    fn kinds(core: &crate::hub::Core, d: &str) -> Vec<String> {
        core.store.entries(d).unwrap().iter().map(|s| s.entry().unwrap().kind).collect()
    }

    #[test]
    fn a_linked_issue_seeds_the_intent_and_follows_linear() {
        let mut core = test_core();
        let d = core.open(&Who::Owner, "Retry failed webhooks", json!({ "title_source": "linear" })).unwrap();
        let i1 = issue_v("Retries with backoff.", "Todo", "t1", &[("2026-11-17T10:00:00Z", "Cap at 3?")]);
        link(&mut core, &d, &i1, &Who::Owner).unwrap();
        let st = core.store.state(&d).unwrap().unwrap();
        assert_eq!(st.intent.as_ref().unwrap().body, "Retries with backoff.");
        assert_eq!(st.intent.as_ref().unwrap().source, "linear");
        assert_eq!(st.linear.as_ref().unwrap().key, "ENG-142");
        assert_eq!(kinds(&core, &d), ["open", "link", "observed", "intent", "observed"]);

        // Nothing changed: nothing written.
        assert_eq!(apply(&mut core, &d, &i1).unwrap(), 0);

        // State and description change, and a new comment: the intent follows.
        let i2 = issue_v("Retries with backoff, up to 3.", "In Progress", "t2", &[("2026-11-17T10:00:00Z", "Cap at 3?"), ("2026-11-17T11:00:00Z", "Yes, 3.")]);
        assert_eq!(apply(&mut core, &d, &i2).unwrap(), 3);
        let st = core.store.state(&d).unwrap().unwrap();
        assert_eq!(st.intent.as_ref().unwrap().body, "Retries with backoff, up to 3.");
        assert_eq!(st.linear.as_ref().unwrap().state.as_deref(), Some("In Progress"));

        // Once you edit the intent here, Linear's next change is only shown.
        core.append(&d, "intent", &Who::Owner, Some("Ours now.".into()), json!({ "source": "person" })).unwrap();
        let i3 = issue_v("Something else.", "In Progress", "t3", &[]);
        apply(&mut core, &d, &i3).unwrap();
        let st = core.store.state(&d).unwrap().unwrap();
        assert_eq!(st.intent.as_ref().unwrap().body, "Ours now.");
        let last = core.store.entries(&d).unwrap().last().unwrap().entry().unwrap();
        assert_eq!(last.data["description"], "Something else.");
        assert!(core.verify(&d).unwrap().ok);
        assert!(core.rebuild_check().unwrap().is_empty());
        // Read from Linear: checkable.
        let link_entry = core.store.entries(&d).unwrap()[1].entry().unwrap();
        assert_eq!(crate::record::mark(&link_entry), "said", "you linked it");
        let observed = core.store.entries(&d).unwrap()[2].entry().unwrap();
        assert_eq!(crate::record::mark(&observed), "checkable");
    }

    #[test]
    fn keys() {
        assert!(is_key("ENG-142"));
        assert!(is_key("api2-7"));
        assert!(!is_key("retry webhooks"));
        assert!(!is_key("ENG-"));
    }
}
