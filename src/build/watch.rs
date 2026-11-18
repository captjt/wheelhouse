//! Build's loop: start what's queued (up to `max_tasks` at once), step each
//! active Build every few seconds, follow open PRs on GitHub, and take in
//! GitHub and Linear issues (`sources`).

use serde_json::{Value, json};
use std::time::{Duration, Instant};

use super::{Attention, Task, steps};
use crate::hub::{Shared, Who};

const STEP: Duration = Duration::from_secs(5);
const START_BACKOFF: i64 = 300;

pub async fn run(shared: Shared) {
    let loaded = super::load(&mut shared.lock());
    if let Err(e) = loaded {
        say!("build: {e:#}");
    }
    tokio::time::sleep(Duration::from_secs(4)).await;
    let mut last_poll: Option<Instant> = None;
    loop {
        if let Ok(cfg) = super::config(&shared) {
            let poll = last_poll.is_none_or(|t| t.elapsed() >= Duration::from_secs(cfg.poll_seconds));
            if poll {
                last_poll = Some(Instant::now());
                if !super::paused(&shared.lock()) {
                    if cfg.github_label.is_some() {
                        intake(&shared, &cfg).await;
                    }
                    super::intake::linear(&shared, &cfg).await;
                }
            }
            // Your answers to Linear offers, every step.
            super::intake::answers(&shared).await;
            let has = !super::all(&shared.lock()).unwrap_or_default().iter().all(Task::finished);
            if has {
                advance(&shared, &cfg, poll).await;
            }
        }
        let nudge = shared.lock().build.check_now.clone();
        tokio::select! {
            _ = tokio::time::sleep(STEP) => {}
            _ = nudge.notified() => {}
        }
    }
}

/// Issues assigned to you with the label, in Build's repos, start a Build.
async fn intake(shared: &Shared, cfg: &super::config::Config) {
    let Some(label) = cfg.github_label.as_deref() else { return };
    let mut args =
        vec!["search".to_string(), "issues".into(), "--assignee=@me".into(), format!("--label={label}"), "--state=open".into(), "--json".into(), "number,repository".into()];
    for r in cfg.repos.keys() {
        args.push(format!("--repo={r}"));
    }
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    let Ok(out) = crate::review::github::gh(&a, None).await else { return };
    let found: Vec<Value> = serde_json::from_str(&out).unwrap_or_default();
    let known: Vec<String> = super::all(&shared.lock()).unwrap_or_default().into_iter().map(|t| t.key).collect();
    for i in found {
        let (Some(repo), Some(n)) = (i["repository"]["nameWithOwner"].as_str(), i["number"].as_i64()) else { continue };
        let key = format!("{repo}#{n}");
        if known.contains(&key) {
            continue;
        }
        match steps::start_from(shared, &key, Some(repo)).await {
            Ok(v) => {
                if let Some(d) = v["docket"].as_str() {
                    super::set_trigger(shared, d, "label");
                }
            }
            Err(e) => say!("build: {key}: {e:#}"),
        }
    }
}

fn set_doing(shared: &Shared, what: Option<String>) {
    let mut core = shared.lock();
    if core.build.doing != what {
        core.build.doing = what;
        core.live_changed();
    }
}

fn epoch() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

async fn advance(shared: &Shared, cfg: &super::config::Config, poll: bool) {
    let (tasks, paused) = {
        let core = shared.lock();
        (super::all(&core).unwrap_or_default(), super::paused(&core))
    };
    let mut active = tasks.iter().filter(|t| t.state == "active" && !t.phase().ends_with("_gate") && t.phase() != "not_ready").count();
    for t in tasks.into_iter().filter(|t| !t.finished()) {
        let lock = super::lock(shared, &t.docket);
        let _g = lock.lock().await;
        let Ok(Some(mut t)) = super::get(&shared.lock(), &t.docket) else { continue };
        // On hold: no steps, no polls, until you resume it.
        let held = shared.lock().store.state(&t.docket).ok().flatten().is_some_and(|s| s.hold.is_some());
        if held {
            continue;
        }
        let before = serde_json::to_value(&t).ok();
        let res: anyhow::Result<Option<Attention>> = match t.state.as_str() {
            "queued" => {
                if t.start_failed_epoch.is_some_and(|e| epoch() - e < START_BACKOFF) {
                    Ok(Some(Attention { kind: "start-failed".into(), text: t.start_error.clone().unwrap_or_default() }))
                } else if paused || active >= cfg.max_tasks {
                    Ok(None)
                } else {
                    active += 1;
                    set_doing(shared, Some(format!("Starting the Build of {}", t.key)));
                    match steps::start(shared, cfg, &mut t).await {
                        Ok(()) => Ok(None),
                        Err(e) => {
                            t.start_error = Some(format!("{e:#}"));
                            t.start_failed_epoch = Some(epoch());
                            Ok(Some(Attention { kind: "start-failed".into(), text: format!("{e:#}") }))
                        }
                    }
                }
            }
            "active" => steps::step(shared, cfg, &mut t).await,
            "open" => steps::step_open(shared, cfg, &mut t, poll).await,
            _ => Ok(None),
        };
        set_doing(shared, None);
        let att = match res {
            Ok(a) => a,
            Err(e) => {
                say!("build: {}: {e:#}", t.docket);
                Some(Attention { kind: "error".into(), text: format!("{e:#}").chars().take(400).collect() })
            }
        };
        let mut core = shared.lock();
        if serde_json::to_value(&t).ok() != before
            && let Err(e) = super::save(&mut core, &mut t)
        {
            say!("build: {}: {e:#}", t.docket);
        }
        let was = core.build.attention.get(&t.docket).cloned();
        match att {
            Some(a) if Some(&a) != was.as_ref() => {
                core.build.attention.insert(t.docket.clone(), a.clone());
                let _ = core.append(&t.docket, "observed", &Who::Hub, Some(a.text.clone()), json!({ "what": "build-attention", "kind": a.kind }));
                let _ = core.events.send(json!({ "type": "build", "docket": t.docket, "kind": a.kind }));
                core.live_changed();
            }
            None if was.is_some() => {
                core.build.attention.remove(&t.docket);
                core.live_changed();
            }
            _ => {}
        }
    }
}

/// Builds at a glance, for the app and `wh build`.
pub fn status(shared: &Shared) -> Value {
    let core = shared.lock();
    let tasks = super::all(&core).unwrap_or_default();
    let list: Vec<Value> = tasks
        .iter()
        .filter(|t| !t.finished() || t.ended_at.as_deref().and_then(crate::time::secs_since).is_some_and(|s| s < 7 * 86400))
        .map(|t| {
            let mut v = super::summary(t);
            v["attention"] = json!(core.build.attention.get(&t.docket));
            v
        })
        .collect();
    let cfg = core.build.config.as_ref().map(|(c, _, _)| c.clone());
    json!({
        "paused": super::paused(&core),
        "doing": core.build.doing,
        "config": cfg.as_ref().map(|c| json!({
            "path": c.path.as_ref().map(|p| crate::paths::tilde(&p.to_string_lossy())),
            "repos": c.repos.keys().collect::<Vec<_>>(),
            "builder": c.builder.agent, "checker": c.checker.agent, "gates": c.gates, "max_tasks": c.max_tasks,
        })),
        "config_error": core.build.config_error,
        "tasks": list,
    })
}
