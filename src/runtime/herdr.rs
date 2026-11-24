//! The Herdr runtime: herdr-core's `hc` runs the terminals, the Herdr app
//! shows them in spaces and tabs, and the phone app lists them.
//!
//! herdr-core keeps the same OSC 7501 program status that Claude Code,
//! Codex and Antigravity report in Rex, so a Herdr terminal's agent is
//! mirrored, tracked, messaged and approved like a Rex pane's. In the
//! mirror (docs/runtimes.md):
//!
//! | Wheelhouse | Herdr                                     | id                    |
//! | ---------- | ----------------------------------------- | --------------------- |
//! | session    | a space (the `herdr.space` label)         | `herdr:space:<label>` |
//! | window     | a tab (the `herdr.tab` label)             | `herdr:tab:<label>`   |
//! | pane       | a terminal                                | `herdr:<address>`     |
//!
//! A terminal Wheelhouse opens carries `wh.session`, `wh.pane` and `wh.tab`
//! labels with its names, so it's found by name whatever the app calls its
//! space and tab. The app places a labelled terminal in its space and tab
//! whenever it runs; without the app the terminals still run.

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::time::sleep;

use super::{Created, Kind, Place};
use crate::agents;
use crate::hub::Shared;
use crate::mirror::{Mirror, Pane, Record, Session, Window};

/// What starts every Herdr id: panes, sessions and windows.
pub const PREFIX: &str = "herdr:";
const SPACE: &str = "herdr:space:";
const TAB: &str = "herdr:tab:";
/// The Herdr app's bundle id, to bring it to the front.
const APP_BUNDLE: &str = "dev.herdr.app";

/// A terminal's pane id: `t2h` → `herdr:t2h`.
pub fn pane_id(address: &str) -> String {
    format!("{PREFIX}{address}")
}

/// A pane id's terminal address: `herdr:t2h` → `t2h`.
fn address(block: &str) -> &str {
    block.strip_prefix(PREFIX).unwrap_or(block)
}

fn find_bin(env: &str, name: &str, fallback: &str) -> PathBuf {
    if let Some(p) = std::env::var_os(env) {
        return PathBuf::from(p);
    }
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return candidate;
            }
        }
    }
    crate::paths::home().join(fallback)
}

/// `$WH_HC`, else `hc` on PATH, else where herdr-core installs it (a
/// LaunchAgent's PATH is short).
pub fn hc_bin() -> PathBuf {
    find_bin("WH_HC", "hc", ".local/bin/hc")
}

/// `$WH_HERDR_APP`, else `herdr-app` on PATH, else beside `hc`.
fn app_bin() -> PathBuf {
    find_bin("WH_HERDR_APP", "herdr-app", ".local/bin/herdr-app")
}

/// Whether herdr-core is on this Mac.
pub fn installed() -> bool {
    hc_bin().is_file()
}

/// The local herdr-core.
#[derive(Clone, Debug, Default)]
pub struct Herdr;

impl Herdr {
    fn command(&self, bin: PathBuf) -> Command {
        let mut c = Command::new(bin);
        // Never let a terminal's own place leak into the hub's calls: hc
        // takes the caller's terminal as a new one's parent.
        c.env_remove("HC_TERMINAL_ID").env_remove("HC_SCOPE").env_remove("HC_TERMINAL_SOCKET");
        c.stdin(Stdio::null()).kill_on_drop(true);
        c
    }

    async fn run_bin(&self, bin: PathBuf, args: &[&str]) -> Result<String> {
        let name = bin.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let mut c = self.command(bin.clone());
        c.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());
        let child = c.spawn().with_context(|| format!("running {}", bin.display()))?;
        // As with Rex: a call that hangs is killed after this long.
        let out = tokio::time::timeout(Duration::from_secs(30), child.wait_with_output())
            .await
            .map_err(|_| anyhow::anyhow!("{name} {} took over 30s", args.first().unwrap_or(&"")))??;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            let err = if err.trim().is_empty() { String::from_utf8_lossy(&out.stdout) } else { err };
            bail!("{name} {}: {}", args.first().unwrap_or(&""), err.trim());
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    async fn hc(&self, args: &[&str]) -> Result<String> {
        self.run_bin(hc_bin(), args).await
    }

    async fn hc_json(&self, args: &[&str]) -> Result<Value> {
        let out = self.hc(args).await?;
        let out = out.trim();
        if out.is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_str(out).with_context(|| format!("hc {}: bad JSON", args.first().unwrap_or(&"")))
    }

    /// One terminal's record, as `hc get` gives it.
    async fn get(&self, block: &str) -> Result<Value> {
        self.hc_json(&["get", address(block), "--json"]).await
    }

    /// Paste and press Enter: `hc send`, which waits out a person's
    /// unfinished draft and won't type into a program that's blocked.
    pub async fn submit(&self, block: &str, text: &str) -> Result<()> {
        // After `--`, text that starts with `-` is text, never a flag.
        self.hc(&["send", address(block), "--json", "--", text]).await.map(|_| ())
    }

    pub async fn keys(&self, block: &str, keys: &[&str]) -> Result<()> {
        let mut args = vec!["keys", address(block)];
        args.extend(keys.iter().map(|k| hc_key(k)));
        args.push("--json");
        self.hc(&args).await.map(|_| ())
    }

    pub async fn capture(&self, block: &str, lines: usize) -> Result<String> {
        let n = lines.to_string();
        let out = self.hc(&["read", address(block), "--lines", &n]).await?;
        let all: Vec<&str> = out.trim_end().lines().collect();
        Ok(all[all.len().saturating_sub(lines)..].join("\n"))
    }

    /// The terminal's root status, read now. herdr-core's own record keeps
    /// only the root; a subagent's records come with its events.
    pub async fn records(&self, block: &str) -> Result<Vec<Record>> {
        let t = self.get(block).await?;
        Ok(root_record(&t).into_iter().collect())
    }

    /// Start `argv` in a new terminal labelled for the space `name` and
    /// either a tab of its own or the anchor's.
    pub async fn open(&self, name: &str, label: &str, cwd: &str, argv: &[String], place: Place<'_>) -> Result<Created> {
        let space = slug(name);
        let (tab, tab_name, new_tab) = match place {
            Place::Tab => (format!("{space}.{}", slug(label)), label.to_string(), true),
            Place::Beside { anchor, .. } => {
                let a = self.get(anchor).await.context("the pane to open beside")?;
                let labels = &a["labels"];
                match labels["herdr.tab"].as_str() {
                    Some(t) => (t.to_string(), labels["wh.tab"].as_str().unwrap_or(t).to_string(), false),
                    None => (format!("{space}.{}", slug(label)), label.to_string(), true),
                }
            }
        };
        let labels = [
            format!("herdr.space={space}"),
            format!("herdr.tab={tab}"),
            "wh=1".to_string(),
            format!("wh.session={name}"),
            format!("wh.tab={tab_name}"),
            format!("wh.pane={label}"),
        ];
        let mut args: Vec<&str> = vec!["run", "--bg", "--json", "--no-parent", "--no-notify", "--cwd", cwd];
        for l in &labels {
            args.extend(["--label", l]);
        }
        args.push("--");
        args.extend(argv.iter().map(String::as_str));
        let v = self.hc_json(&args).await?;
        let addr = v["address"].as_str().context("hc run: no address")?.to_string();
        if new_tab {
            // The app names a space it made after its first terminal's folder
            // and a tab by its number; name them once it has placed this one.
            let (space, tab, name, tab_name) = (space.clone(), tab.clone(), name.to_string(), tab_name);
            tokio::spawn(async move {
                sleep(Duration::from_secs(3)).await;
                let h = Herdr;
                let _ = h.app(&["do", "space.rename", &json!({ "id": space, "name": name }).to_string()]).await;
                let _ = h.app(&["do", "tab.rename", &json!({ "id": tab, "name": tab_name }).to_string()]).await;
            });
        }
        Ok(Created { session_id: format!("{SPACE}{space}"), window_id: Some(format!("{TAB}{tab}")), block_id: pane_id(&addr) })
    }

    /// End the terminal's program, and remove it.
    pub async fn close(&self, block: &str) -> Result<()> {
        let t = address(block);
        let _ = self.hc(&["kill", t, "--json"]).await;
        self.hc(&["rm", t, "--json"]).await.map(|_| ())
    }

    pub async fn rename(&self, block: &str, name: &str) -> Result<()> {
        let l = format!("wh.pane={name}");
        self.hc(&["label", address(block), &l, "--json"]).await.map(|_| ())
    }

    async fn app(&self, args: &[&str]) -> Result<String> {
        let out = self.run_bin(app_bin(), args).await?;
        if out.contains("\"error\"") {
            bail!("herdr-app {}: {}", args.get(1).unwrap_or(&""), out.trim());
        }
        Ok(out)
    }

    /// Show the terminal in the Herdr app, in its tab, and bring the app
    /// to the front.
    pub async fn focus(&self, block: &str) -> Result<()> {
        if !app_bin().is_file() {
            bail!("the Herdr app isn't installed: the terminal is {} (`hc attach {}`)", address(block), address(block));
        }
        self.app(&["do", "agent.focus", &json!({ "terminal": address(block) }).to_string()]).await.context("the Herdr app isn't open")?;
        let _ = tokio::process::Command::new("open").args(["-b", APP_BUNDLE]).status().await;
        Ok(())
    }

    /// `hc events --json`: a snapshot of every terminal, then one line per change.
    fn events(&self) -> Result<Child> {
        let mut c = self.command(hc_bin());
        c.args(["events", "--json"]).stdout(Stdio::piped()).stderr(Stdio::null());
        c.spawn().with_context(|| format!("running {}", hc_bin().display()))
    }
}

/// Wheelhouse's key names in hc's spelling.
fn hc_key(k: &str) -> &str {
    match k {
        "escape" => "esc",
        "return" => "enter",
        other => other,
    }
}

/// A label value hc takes: the name, lowercased, with anything but
/// letters, digits, `.`, `_` and `-` made `-`.
fn slug(s: &str) -> String {
    let s: String = s.trim().chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') { c.to_ascii_lowercase() } else { '-' }).collect();
    if s.is_empty() { "wheelhouse".into() } else { s }
}

/// A terminal's root status as a Rex-shaped record, timed by `status_ms`.
fn root_record(t: &Value) -> Option<Record> {
    let s = t.get("status").filter(|s| s.is_object())?;
    let mut r = record_of("", s)?;
    r.updated_at = t["status_ms"].as_i64().and_then(crate::time::from_unix_ms);
    Some(r)
}

fn record_of(id: &str, v: &Value) -> Option<Record> {
    let str_of = |k: &str| v[k].as_str().filter(|s| !s.is_empty()).map(str::to_string);
    Some(Record {
        id: id.to_string(),
        state: str_of("state")?,
        kind: str_of("kind"),
        app: str_of("app"),
        effective_app: None,
        title: str_of("title"),
        msg: str_of("msg"),
        updated_at: str_of("updated_at"),
    })
}

// ---------------------------------------------------------------------------
// Following herdr-core

/// One terminal as the follower knows it: its record, and the subagents'
/// status records its events carried.
#[derive(Default)]
struct Term {
    rec: Value,
    sub: Vec<Record>,
}

/// `Storefront / Claude Code` → `("Storefront", "Claude Code")`: where the app
/// shows a terminal, from its `where` label.
fn place_of(labels: &Value) -> (Option<&str>, Option<&str>) {
    match labels["where"].as_str() {
        Some(w) => match w.split_once(" / ") {
            Some((space, tab)) => (Some(space), Some(tab)),
            None => (Some(w), None),
        },
        None => (None, None),
    }
}

/// The Herdr part of the mirror, from every live terminal.
fn build(terms: &BTreeMap<String, Term>) -> Mirror {
    let mut m = Mirror::default();
    for (addr, t) in terms {
        let rec = &t.rec;
        if rec["state"].as_str().is_some_and(|s| s != "live") {
            continue;
        }
        let labels = &rec["labels"];
        let (where_space, where_tab) = place_of(labels);
        let space = labels["herdr.space"].as_str().unwrap_or_default();
        let sid = format!("{SPACE}{space}");
        let session = m.sessions.entry(sid.clone()).or_insert_with(|| Session {
            runtime: Kind::Herdr,
            session_id: sid.clone(),
            label: labels["wh.session"].as_str().or(where_space).map(str::to_string).unwrap_or_else(|| if space.is_empty() { "herdr".into() } else { space.to_string() }),
            windows: vec![],
        });
        let window_id = labels["herdr.tab"].as_str().map(|t| format!("{TAB}{t}"));
        if let Some(w) = &window_id
            && !session.windows.iter().any(|x| &x.window_id == w)
        {
            let label = labels["wh.tab"].as_str().or(where_tab).unwrap_or(w.trim_start_matches(TAB)).to_string();
            session.windows.push(Window { window_id: w.clone(), label });
        }
        let block = pane_id(addr);
        let mut p = Pane::new(&sid, &block);
        p.window_id = window_id;
        p.label = labels["wh.pane"].as_str().or(rec["name"].as_str()).unwrap_or_default().to_string();
        p.records = root_record(rec).into_iter().chain(t.sub.iter().cloned()).map(|r| (r.id.clone(), r)).collect();
        match rec.get("foreground").filter(|f| f.is_object()) {
            Some(fg) => p.set_foreground(fg),
            None => p.set_reported_cwd(rec["cwd"].as_str()),
        }
        m.panes.insert(block, p);
    }
    m
}

/// Follow herdr-core for as long as the hub runs. Not installed, it's
/// looked for again every minute, quietly; lost, it's followed again,
/// backing off to 30s.
pub async fn follow(shared: Shared) {
    let mut backoff = Duration::from_secs(1);
    let mut said_missing = false;
    loop {
        if !installed() {
            {
                let mut core = shared.lock();
                if core.mirror.link(Kind::Herdr).connected || !said_missing {
                    let changed = core.mirror.replace(Kind::Herdr, Mirror::default());
                    core.mirror.set_link(Kind::Herdr, false, None, false);
                    agents::reconcile(&mut core, changed);
                    core.live_changed();
                }
            }
            said_missing = true;
            sleep(Duration::from_secs(60)).await;
            continue;
        }
        said_missing = false;
        let started = std::time::Instant::now();
        let err = follow_once(&shared).await.err().map(|e| format!("{e:#}"));
        {
            let mut core = shared.lock();
            core.mirror.set_link(Kind::Herdr, false, err.clone(), installed());
            core.live_changed();
        }
        say!("lost Herdr: {}", err.as_deref().unwrap_or("event stream ended"));
        if started.elapsed() > Duration::from_secs(60) {
            backoff = Duration::from_secs(1);
        }
        sleep(backoff).await;
        backoff = (backoff * 2).min(Duration::from_secs(30));
    }
}

async fn follow_once(shared: &Shared) -> Result<()> {
    let h = Herdr;
    let mut child = h.events()?;
    let stdout = child.stdout.take().context("events stdout")?;
    let mut lines = BufReader::new(stdout).lines();
    let mut terms: BTreeMap<String, Term> = BTreeMap::new();
    let mut first = true;
    // Terminals whose status changed: read again with `hc get` for the
    // exact report time the approval guard compares.
    let mut stale: BTreeSet<String> = BTreeSet::new();
    let mut tick = tokio::time::interval(Duration::from_millis(300));
    loop {
        tokio::select! {
            _ = tick.tick(), if !stale.is_empty() => {
                for addr in std::mem::take(&mut stale) {
                    let Ok(fresh) = h.get(&addr).await else { continue };
                    if let Some(t) = terms.get_mut(&addr) {
                        for k in ["status", "status_ms", "foreground", "labels", "title", "state"] {
                            if let Some(v) = fresh.get(k) {
                                t.rec[k] = v.clone();
                            }
                        }
                    }
                }
                publish(shared, &terms, false);
            }
            line = lines.next_line() => {
                let Some(line) = line? else { bail!("event stream ended") };
                let Ok(msg) = serde_json::from_str::<Value>(&line) else { continue };
                if let Some(all) = msg["terminals"].as_array() {
                    terms = all.iter().filter_map(|t| Some((t["address"].as_str()?.to_string(), Term { rec: t.clone(), sub: vec![] }))).collect();
                    publish(shared, &terms, first);
                    if first {
                        say!("following Herdr");
                    }
                    first = false;
                    continue;
                }
                match msg["method"].as_str() {
                    // Events were dropped: start again from a snapshot.
                    Some("missed") => bail!("hc missed events"),
                    Some("event") => {}
                    _ => continue,
                }
                let p = &msg["params"];
                let Some(addr) = p["terminal"].as_str().map(str::to_string) else { continue };
                let data = &p["data"];
                match p["type"].as_str().unwrap_or_default() {
                    "created" => {
                        terms.insert(addr, Term { rec: data.clone(), sub: vec![] });
                    }
                    "removed" | "finished" => {
                        terms.remove(&addr);
                    }
                    "status" => {
                        let t = terms.entry(addr.clone()).or_default();
                        t.rec["status"] = data["status"].clone();
                        t.rec["status_ms"] = json!(time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000);
                        t.sub = data["records"].as_object().map(|o| o.iter().filter_map(|(id, r)| record_of(id, r)).collect()).unwrap_or_default();
                        stale.insert(addr);
                    }
                    "labels" => terms.entry(addr).or_default().rec["labels"] = data["labels"].clone(),
                    "title" => terms.entry(addr).or_default().rec["title"] = data["title"].clone(),
                    "foreground" => terms.entry(addr).or_default().rec["foreground"] = data.clone(),
                    _ => continue,
                }
                publish(shared, &terms, false);
            }
        }
    }
}

/// Put the terminals in the mirror, and reconcile the dockets of panes that
/// changed. The first time, also every agent the record thinks is open in
/// Herdr, so dockets whose terminals went away while the hub was down are
/// released.
fn publish(shared: &Shared, terms: &BTreeMap<String, Term>, first: bool) {
    let m = build(terms);
    let mut core = shared.lock();
    let mut changed = core.mirror.replace(Kind::Herdr, m);
    if first {
        core.mirror.set_link(Kind::Herdr, true, None, true);
        changed.extend(core.mirror.panes.values().filter(|p| p.runtime == Kind::Herdr).map(|p| p.block_id.clone()));
        changed.extend(core.agents.by_block.keys().filter(|b| super::of(b) == Kind::Herdr).cloned());
        core.live_changed();
    }
    if !changed.is_empty() {
        agents::reconcile(&mut core, changed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn term(addr: &str, labels: Value, status: Value) -> (String, Term) {
        let rec = json!({ "address": addr, "state": "live", "cwd": "/w", "labels": labels, "status": status, "status_ms": 1795481147023i64,
                          "foreground": { "name": "claude", "cwd": "/w/repo" } });
        (addr.to_string(), Term { rec, sub: vec![] })
    }

    #[test]
    fn terminals_become_sessions_tabs_and_panes() {
        let terms: BTreeMap<String, Term> = [
            term(
                "t2h",
                json!({ "herdr.space": "sp36", "herdr.tab": "tb77", "where": "Storefront / Claude Code" }),
                json!({ "state": "blocked", "kind": "permission", "app": "claude-code", "msg": "Run ls?" }),
            ),
            term(
                "t3a",
                json!({ "herdr.space": "wheelhouse", "herdr.tab": "wheelhouse.d-12", "wh.session": "wheelhouse", "wh.tab": "D-12", "wh.pane": "D-12", "where": "rex-hub / 1" }),
                json!({ "state": "idle", "app": "codex" }),
            ),
            term("t3b", json!({}), Value::Null),
        ]
        .into_iter()
        .collect();
        let m = build(&terms);

        let p = &m.panes["herdr:t2h"];
        assert_eq!(p.runtime, Kind::Herdr);
        assert_eq!(p.session_id, "herdr:space:sp36");
        assert_eq!(m.session_label("herdr:space:sp36"), "Storefront");
        assert_eq!(m.sessions["herdr:space:sp36"].windows[0].label, "Claude Code");
        let r = p.root().unwrap();
        assert_eq!((r.state.as_str(), r.kind.as_deref(), r.msg.as_deref()), ("blocked", Some("permission"), Some("Run ls?")));
        assert_eq!(r.updated_at.as_deref(), Some("2026-11-24T00:45:47.023Z"));
        assert_eq!(p.app(), Some("claude-code"));
        assert_eq!(p.cwd.as_deref(), Some("/w/repo"));

        // Wheelhouse's own: found by its names, whatever the app calls them.
        let q = &m.panes["herdr:t3a"];
        assert_eq!((m.session_label(&q.session_id), q.label.as_str()), ("wheelhouse", "D-12"));
        assert_eq!(m.sessions[&q.session_id].windows[0].label, "D-12");

        // A terminal with no space or status is still a pane, not an agent.
        let s = &m.panes["herdr:t3b"];
        assert_eq!(m.session_label(&s.session_id), "herdr");
        assert!(s.root().is_none());
    }

    #[test]
    fn a_finished_terminal_is_no_pane() {
        let (a, mut t) = term("t9", json!({}), Value::Null);
        t.rec["state"] = json!("finished");
        let m = build(&[(a, t)].into_iter().collect());
        assert!(m.panes.is_empty());
    }

    #[test]
    fn keys_and_names() {
        assert_eq!(hc_key("escape"), "esc");
        assert_eq!(hc_key("1"), "1");
        assert_eq!(slug("wheelhouse-reviews"), "wheelhouse-reviews");
        assert_eq!(slug("D-12 · builder"), "d-12---builder");
        assert_eq!(address("herdr:t2h"), "t2h");
        assert_eq!(pane_id("t2h"), "herdr:t2h");
    }

    #[test]
    fn replacing_herdr_leaves_rex_alone() {
        let mut m = Mirror::default();
        m.apply_pane_status("session:1", "block:1", vec![], None, None);
        let terms: BTreeMap<String, Term> = [term("t1", json!({ "herdr.space": "s" }), json!({ "state": "idle", "app": "codex" }))].into_iter().collect();
        let changed = m.replace(Kind::Herdr, build(&terms));
        assert_eq!(changed, ["herdr:t1".to_string()].into_iter().collect());
        assert!(m.panes.contains_key("block:1"));
        // The same again changes nothing; an empty Herdr drops only its own.
        assert!(m.replace(Kind::Herdr, build(&terms)).is_empty());
        let changed = m.replace(Kind::Herdr, Mirror::default());
        assert_eq!(changed, ["herdr:t1".to_string()].into_iter().collect());
        assert_eq!(m.panes.keys().collect::<Vec<_>>(), ["block:1"]);
    }
}
