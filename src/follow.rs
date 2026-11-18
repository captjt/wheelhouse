//! Following the local Rex server: a snapshot, then its event stream, with
//! the agent dockets reconciled on every change. Ported from rex-inbox.

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tokio::time::{Instant, sleep, sleep_until};

use crate::agents;
use crate::hub::Shared;
use crate::mirror::{Mirror, Record, Session, SessionSnapshot, TERMINAL, Window};
use crate::rex::Rex;

pub async fn run(shared: Shared) {
    let rex = Rex;
    let mut backoff = Duration::from_secs(1);
    loop {
        let started = std::time::Instant::now();
        let err = follow(&shared, &rex).await.err().map(|e| format!("{e:#}"));
        {
            let mut core = shared.lock();
            core.mirror.connected = false;
            core.mirror.error = err.clone();
            core.live_changed();
        }
        say!("lost Rex: {}", err.as_deref().unwrap_or("event stream ended"));
        if started.elapsed() > Duration::from_secs(60) {
            backoff = Duration::from_secs(1);
        }
        sleep(backoff).await;
        backoff = (backoff * 2).min(Duration::from_secs(30));
    }
}

async fn follow(shared: &Shared, rex: &Rex) -> Result<()> {
    // Start the stream before the snapshot, so nothing falls between them.
    let mut child = rex.events()?;
    let stdout = child.stdout.take().context("events stdout")?;
    let mut lines = BufReader::new(stdout).lines();

    let snap = snapshot(rex, None).await?;
    {
        let mut core = shared.lock();
        let mut m = Mirror { connected: true, ..Default::default() };
        apply_snapshot(&mut m, snap);
        core.mirror = m;
        // Every pane now, and every agent the record thinks is open, so
        // dockets whose panes went away while the hub was down are released.
        let mut all: BTreeSet<String> = core.mirror.panes.keys().cloned().collect();
        all.extend(core.agents.by_block.keys().cloned());
        agents::reconcile(&mut core, all);
    }
    say!("following Rex");

    let mut pending: BTreeSet<String> = BTreeSet::new();
    let mut pending_all = false;
    let mut deadline: Option<Instant> = None;
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    loop {
        tokio::select! {
            _ = tick.tick() => {
                agents::tick(&mut shared.lock());
            }
            line = lines.next_line() => {
                let Some(line) = line? else { bail!("event stream ended") };
                let Ok(ev) = serde_json::from_str::<Value>(&line) else { continue };
                let mut core = shared.lock();
                let changes = core.mirror.apply_event(&ev);
                if !changes.panes.is_empty() {
                    agents::reconcile(&mut core, changes.panes);
                }
                if !changes.resync.is_empty() || changes.resync_all {
                    pending.extend(changes.resync);
                    pending_all |= changes.resync_all;
                    deadline.get_or_insert(Instant::now() + Duration::from_millis(300));
                }
            }
            _ = async { sleep_until(deadline.unwrap()).await }, if deadline.is_some() => {
                deadline = None;
                let only = if std::mem::take(&mut pending_all) { None } else { Some(std::mem::take(&mut pending)) };
                pending.clear();
                if let Err(e) = resync(shared, rex, only).await {
                    say!("resync: {e:#}");
                }
            }
        }
    }
}

/// One pane's status in a snapshot: `(session, block, records, process, pwd)`.
type PaneStatus = (String, String, Vec<Record>, Option<Value>, Option<Value>);

struct Snapshot {
    /// `None` when only some sessions were fetched.
    all_sessions: Option<BTreeSet<String>>,
    sessions: Vec<SessionSnapshot>,
    statuses: Vec<PaneStatus>,
}

fn apply_snapshot(m: &mut Mirror, snap: Snapshot) -> BTreeSet<String> {
    let mut changed = BTreeSet::new();
    if let Some(live) = &snap.all_sessions {
        changed.extend(m.retain_sessions(live).panes);
    }
    for s in snap.sessions {
        changed.extend(m.apply_session(s).panes);
    }
    for (sid, bid, records, process, pwd) in snap.statuses {
        m.apply_pane_status(&sid, &bid, records, process.as_ref(), pwd.as_ref());
        changed.insert(bid);
    }
    changed
}

/// Fetch sessions (all, or `only`), their layouts, and the status of their
/// terminals. With `known`, status is fetched just for panes new to the mirror.
async fn snapshot(rex: &Rex, only: Option<(BTreeSet<String>, BTreeSet<String>)>) -> Result<Snapshot> {
    let (wanted, all_sessions, known) = match only {
        Some((sessions, known)) => (sessions.into_iter().collect::<Vec<_>>(), None, Some(known)),
        None => {
            let list = rex.session_list().await?;
            let ids: Vec<String> = list["sessions"].as_array().map(|a| a.iter().filter_map(|s| s["session_id"].as_str().map(str::to_string)).collect()).unwrap_or_default();
            let all = ids.iter().cloned().collect();
            (ids, Some(all), None)
        }
    };

    let limit = Arc::new(Semaphore::new(8));
    let mut set = JoinSet::new();
    for sid in wanted {
        let limit = limit.clone();
        let rex = rex.clone();
        set.spawn(async move {
            let _permit = limit.acquire_owned().await;
            fetch_session(&rex, &sid).await
        });
    }
    let mut sessions = Vec::new();
    while let Some(r) = set.join_next().await {
        match r? {
            Ok(s) => sessions.push(s),
            // A session can vanish between list and view.
            Err(e) => say!("{e:#}"),
        }
    }

    let mut set = JoinSet::new();
    for s in &sessions {
        for (bid, _, creator, _) in &s.blocks {
            if creator != TERMINAL || known.as_ref().is_some_and(|k| k.contains(bid)) {
                continue;
            }
            let (rex, sid, bid, limit) = (rex.clone(), s.session.session_id.clone(), bid.clone(), limit.clone());
            set.spawn(async move {
                let _permit = limit.acquire_owned().await;
                let status = rex.block_call(&sid, &bid, "program_status", &json!({})).await;
                let process = rex.block_call(&sid, &bid, "process", &json!({})).await.ok();
                let pwd = rex.block_call(&sid, &bid, "pwd", &json!({})).await.ok();
                (sid, bid, status, process, pwd)
            });
        }
    }
    let mut statuses = Vec::new();
    while let Some(r) = set.join_next().await {
        let (sid, bid, status, process, pwd) = r?;
        let Ok(status) = status else { continue };
        let records: Vec<Record> = status["records"].as_array().map(|a| a.iter().filter_map(|v| serde_json::from_value(v.clone()).ok()).collect()).unwrap_or_default();
        statuses.push((sid, bid, records, process, pwd));
    }
    Ok(Snapshot { all_sessions, sessions, statuses })
}

async fn fetch_session(rex: &Rex, sid: &str) -> Result<SessionSnapshot> {
    let view = rex.api(Some(sid), "session.view", &json!({})).await?;
    let blocks = rex.api(Some(sid), "session.list_blocks", &json!({})).await?;
    let windows = view["windows"]
        .as_array()
        .map(|a| a.iter().filter_map(|w| Some(Window { window_id: w["window_id"].as_str()?.to_string(), label: w["label"].as_str().unwrap_or_default().to_string() })).collect())
        .unwrap_or_default();
    let blocks = blocks["blocks"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|b| {
                    Some((
                        b["block_id"].as_str()?.to_string(),
                        b["label"].as_str().unwrap_or_default().to_string(),
                        b["creator_name"].as_str().unwrap_or_default().to_string(),
                        b["window_id"].as_str().map(str::to_string),
                    ))
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(SessionSnapshot { session: Session { session_id: sid.to_string(), label: view["label"].as_str().unwrap_or_default().to_string(), windows }, blocks })
}

/// Fetch some sessions again (or the whole list, with `None`). Status is
/// fetched only for panes new to the mirror; events keep the rest current.
async fn resync(shared: &Shared, rex: &Rex, only: Option<BTreeSet<String>>) -> Result<()> {
    let known: BTreeSet<String> = shared.lock().mirror.panes.keys().cloned().collect();
    let sessions = match only {
        Some(s) => s,
        None => {
            let list = rex.session_list().await?;
            let ids: BTreeSet<String> = list["sessions"].as_array().map(|a| a.iter().filter_map(|s| s["session_id"].as_str().map(str::to_string)).collect()).unwrap_or_default();
            let mut core = shared.lock();
            let changed = core.mirror.retain_sessions(&ids).panes;
            agents::reconcile(&mut core, changed);
            ids
        }
    };
    let snap = snapshot(rex, Some((sessions, known))).await?;
    let mut core = shared.lock();
    let changed = apply_snapshot(&mut core.mirror, snap);
    agents::reconcile(&mut core, changed);
    Ok(())
}
