//! The Review protocol's loop: check GitHub every `poll_seconds` (or when
//! nudged), and move each PR's round along every few seconds. One PR at a
//! time, under its lock, as rex-reviewer's watcher did.

use anyhow::Result;
use serde_json::{Value, json};
use std::path::Path;
use std::time::{Duration, Instant};

use super::config::Config;
use super::pr::Pr;
use super::{Attention, github, rounds, rules};
use crate::hub::{Shared, Who};

const STEP: Duration = Duration::from_secs(5);
const START_BACKOFF: i64 = 120;
/// No round starts this soon after the PR's last post or dismiss: a real
/// round takes longer, so this only brakes a loop nobody foresaw.
const COOLDOWN: i64 = 180;

pub async fn run(shared: Shared) {
    let loaded = super::load(&mut shared.lock());
    if let Err(e) = loaded {
        say!("review: {e:#}");
    }
    tokio::time::sleep(Duration::from_secs(3)).await;
    let mut last_scan: Option<Instant> = None;
    loop {
        let on = super::enabled(&shared.lock());
        if on {
            match super::config(&shared) {
                Ok(cfg) => {
                    let due = last_scan.is_none_or(|t| t.elapsed() >= Duration::from_secs(cfg.poll_seconds));
                    if due {
                        last_scan = Some(Instant::now());
                        scan(&shared, &cfg).await;
                    }
                    advance(&shared, &cfg).await;
                    integrate_next(&shared).await;
                }
                Err(e) => say!("review: config: {e:#}"),
            }
        }
        let nudge = shared.lock().review.check_now.clone();
        tokio::select! {
            _ = tokio::time::sleep(STEP) => {}
            _ = nudge.notified() => { last_scan = None; }
        }
    }
}

/// Read what one more PR is for, if any hasn't been: a PR just requested
/// before its round starts, and every PR from before Wheelhouse did this.
/// One per step, so a backlog goes by slowly; one that fails waits half an
/// hour.
async fn integrate_next(shared: &Shared) {
    let next = {
        let core = shared.lock();
        let tried = &core.review.integrate_tried;
        super::all(&core)
            .unwrap_or_default()
            .into_iter()
            .find(|p| p.integrated_at.is_none() && super::docket_of(&core, &p.key()).is_some() && tried.get(&p.key()).is_none_or(|t| t.elapsed() > Duration::from_secs(1800)))
    };
    let Some(p) = next else { return };
    let key = p.key();
    shared.lock().review.integrate_tried.insert(key.clone(), Instant::now());
    let lock = super::lock(shared, &key);
    let _g = lock.lock().await;
    let done = async {
        let f = super::links::Facts::fetch(&p.repo, p.pr).await?;
        super::links::integrate(shared, &key, &f, None).await
    }
    .await;
    match done {
        Ok(()) => {
            let mut core = shared.lock();
            if let Ok(Some(mut e)) = super::get(&core, &key) {
                e.integrated_at = Some(crate::time::now());
                if let Err(err) = super::save(&mut core, &mut e) {
                    say!("review: {key}: {err:#}");
                }
            }
        }
        Err(err) => say!("review: {key}: linking: {err:#}"),
    }
}

/// What the loop is doing, for the status line: the app hears only changes.
fn set_doing(shared: &Shared, what: Option<String>) {
    let mut core = shared.lock();
    if core.review.doing != what {
        core.review.doing = what;
        core.live_changed();
    }
}

/// Check GitHub, and apply what it says to every PR.
pub async fn scan(shared: &Shared, cfg: &Config) {
    let tracked: Vec<(String, i64)> = {
        let core = shared.lock();
        super::all(&core).unwrap_or_default().into_iter().filter(|p| !p.finished() && cfg.repos.contains_key(&p.repo)).map(|p| (p.repo, p.pr)).collect()
    };
    set_doing(shared, Some("Checking GitHub".into()));
    let res = github::scan(&cfg.repo_list(), &tracked).await;
    set_doing(shared, None);
    let obs = match res {
        Ok((_, obs)) => {
            let mut core = shared.lock();
            core.review.last_scan_at = Some(crate::time::now());
            core.review.scan_error = None;
            obs
        }
        Err(e) => {
            say!("review: checking GitHub: {e:#}");
            shared.lock().review.scan_error = Some(format!("{e:#}"));
            return;
        }
    };
    for o in obs {
        let key = format!("{}#{}", o.repo, o.pr);
        let lock = super::lock(shared, &key);
        let _g = lock.lock().await;
        if let Err(e) = apply(shared, &o) {
            say!("review: {key}: {e:#}");
        }
    }
}

fn apply(shared: &Shared, o: &rules::Obs) -> Result<()> {
    let key = format!("{}#{}", o.repo, o.pr);
    let mut core = shared.lock();
    let before = super::get(&core, &key)?;
    let (after, events) = rules::decide(before.clone(), o, &crate::time::now());
    let Some(mut after) = after else { return Ok(()) };
    let changed = before.as_ref().is_none_or(|b| serde_json::to_value(b).ok() != serde_json::to_value(&after).ok());
    if changed {
        super::save(&mut core, &mut after)?;
    }
    for ev in events {
        super::event(&mut core, &after, &ev.kind, ev.data, &Who::Hub)?;
    }
    Ok(())
}

/// Every PR one step along.
pub async fn advance(shared: &Shared, cfg: &Config) {
    let (prs, paused) = {
        let core = shared.lock();
        (super::all(&core).unwrap_or_default(), super::paused(&core))
    };
    let mut active = prs.iter().filter(|p| p.state == "reviewing" && p.phase() != "awaiting").count();
    for p in prs.into_iter().filter(|p| p.state != "cleaned" && cfg.repos.contains_key(&p.repo)) {
        let key = p.key();
        // On hold: no rounds, no polls, until you resume it.
        let held = {
            let core = shared.lock();
            super::docket_of(&core, &key).and_then(|d| core.store.state(&d).ok().flatten()).is_some_and(|s| s.hold.is_some())
        };
        if held {
            continue;
        }
        let lock = super::lock(shared, &key);
        let _g = lock.lock().await;
        let Ok(Some(e)) = super::get(&shared.lock(), &key) else { continue };
        let att = match step(shared, cfg, e, paused, &mut active).await {
            Ok(a) => a,
            Err(err) => {
                say!("review: {key}: {err:#}");
                let round = super::get(&shared.lock(), &key).ok().flatten().map_or(0, |e| e.round);
                Some(Attention { kind: "error".into(), text: format!("{err:#}").chars().take(400).collect(), round })
            }
        };
        set_doing(shared, None);
        attend(shared, &key, att);
    }
}

/// Keep what needs you current: new ones go on the docket and notify you.
fn attend(shared: &Shared, key: &str, att: Option<Attention>) {
    let mut core = shared.lock();
    let was = core.review.attention.get(key).cloned();
    match att {
        None => {
            if was.is_some() {
                core.review.attention.remove(key);
                core.live_changed();
            }
        }
        Some(a) if Some(&a) != was.as_ref() => {
            core.review.attention.insert(key.to_string(), a.clone());
            if let Some(d) = super::docket_of(&core, key) {
                let _ = core.append(&d, "observed", &Who::Hub, Some(a.text.clone()), json!({ "what": "review-attention", "kind": a.kind, "round": a.round }));
                let _ = core.events.send(json!({ "type": "review", "key": key, "kind": a.kind, "docket": d }));
            }
            core.live_changed();
        }
        Some(_) => {}
    }
}

fn epoch() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn att(kind: &str, text: impl Into<String>, round: i64) -> Option<Attention> {
    Some(Attention { kind: kind.into(), text: text.into(), round })
}

async fn step(shared: &Shared, cfg: &Config, mut e: Pr, paused: bool, active: &mut usize) -> Result<Option<Attention>> {
    let key = e.key();
    match e.state.as_str() {
        "queued" => {
            if let (Some(t), Some(why)) = (e.start_failed_epoch, &e.start_error)
                && epoch() - t < START_BACKOFF
            {
                return Ok(att("start-failed", why.clone(), e.round + 1));
            }
            if paused || *active >= cfg.max_rounds {
                return Ok(None);
            }
            let cooldown = std::env::var("WH_REVIEW_COOLDOWN").ok().and_then(|v| v.parse().ok()).unwrap_or(COOLDOWN);
            if e.posted_at.as_deref().and_then(crate::time::secs_since).is_some_and(|s| s < cooldown) {
                return Ok(None);
            }
            *active += 1;
            set_doing(shared, Some(format!("Starting round {} of {}", e.round + 1, e.short())));
            let started = async {
                rounds::prep(shared, &key).await?;
                rounds::start(shared, &key).await
            }
            .await;
            if let Err(err) = started {
                let mut core = shared.lock();
                if let Ok(Some(mut e)) = super::get(&core, &key) {
                    e.start_error = Some(format!("{err:#}"));
                    e.start_failed_epoch = Some(epoch());
                    super::save(&mut core, &mut e)?;
                }
                return Ok(att("start-failed", format!("{err:#}"), e.round + 1));
            }
            Ok(None)
        }
        "reviewing" => {
            let round = e.round;
            match e.phase() {
                "reviewer" => {
                    let p = rounds::progress(shared, &e);
                    if e.reviewers_done != Some(p.done) || e.reviewers_total != Some(p.total) {
                        e.reviewers_done = Some(p.done);
                        e.reviewers_total = Some(p.total);
                        super::save(&mut shared.lock(), &mut e)?;
                    }
                    if let Some((name, why)) = p.blocked {
                        let text = if p.total > 1 { format!("{name}: {why}") } else { why };
                        return Ok(att("blocked", text, round));
                    }
                    if p.ready {
                        if !p.stale.is_empty() {
                            say!("review: {key}: counting {} as done (still working, but finished a while ago)", p.stale.join(", "));
                        }
                        set_doing(shared, Some(format!("Checking {}", e.short())));
                        let live = github::live_head(&e.repo, e.pr).await?;
                        if Some(&live) == e.round_head.as_ref() {
                            rounds::start_checker(shared, &key).await?;
                        } else {
                            rounds::follow_head(shared, &key, &live).await?;
                        }
                        return Ok(None);
                    }
                    if let Some(name) = p.closed {
                        let text = if p.total > 1 { format!("the {name} reviewer's pane closed mid-round") } else { "the reviewer's pane closed mid-round".into() };
                        return Ok(att("pane-closed", text, round));
                    }
                    if let Some(name) = p.stalled {
                        return Ok(att("stalled", format!("the {name} reviewer went idle without finishing; look at its pane, or dismiss the round"), round));
                    }
                    Ok(None)
                }
                phase => {
                    let primary = e.worktree.clone().unwrap_or_default();
                    let st = rounds::status(Path::new(&primary));
                    if let Some(why) = st.strip_prefix("BLOCKED") {
                        return Ok(att("blocked", why.trim_start_matches(':').trim().to_string(), round));
                    }
                    if st == "AWAITING_APPROVAL" {
                        if phase != "awaiting" {
                            e.phase = Some("awaiting".into());
                            let mut core = shared.lock();
                            super::save(&mut core, &mut e)?;
                            super::event(&mut core, &e, "awaiting", json!({ "round": round, "head": e.round_head }), &Who::Hub)?;
                        }
                        if e.head.is_some() && e.head != e.round_head {
                            let head = e.head.clone().unwrap_or_default();
                            rounds::follow_head(shared, &key, &head).await?;
                            return Ok(None);
                        }
                        return match rounds::decide(shared, &key).await? {
                            rounds::Decided::Posted => Ok(None),
                            rounds::Decided::Waits(why) => Ok(att("ready", format!("the checked review is ready to post; it waits for you because {why}"), round)),
                            rounds::Decided::NotRule => Ok(att("ready", "the checked review is ready to post", round)),
                        };
                    }
                    if phase == "checking" && st == "HEAD_MOVED" {
                        let live = github::live_head(&e.repo, e.pr).await?;
                        rounds::follow_head(shared, &key, &live).await?;
                        return Ok(None);
                    }
                    if phase == "checking" && st.starts_with("VERIFYING") {
                        let block = e
                            .checker_block
                            .clone()
                            .filter(|b| super::panes::state(shared, b).is_some())
                            .or_else(|| e.checker_pane.as_deref().and_then(|l| super::panes::find(shared, e.session_id.as_deref(), l)));
                        match block.and_then(|b| super::panes::state(shared, &b)) {
                            None => return Ok(att("pane-closed", "the checker's pane closed before it finished", round)),
                            Some((s, at)) if matches!(s.as_str(), "idle" | "done") => {
                                let since = at
                                    .and_then(|a| time::OffsetDateTime::parse(&a, &time::format_description::well_known::Rfc3339).ok())
                                    .map(|t| (time::OffsetDateTime::now_utc() - t).whole_seconds());
                                if since.is_some_and(|s| s >= rounds::STALLED) {
                                    return Ok(att("stalled", "the checker went idle without finishing; look at its pane, or dismiss the round", round));
                                }
                            }
                            _ => {}
                        }
                    }
                    Ok(None)
                }
            }
        }
        "posted" => {
            if e.pane.is_some() || e.checker_pane.is_some() {
                let checker = e.checker_block.clone().and_then(|b| super::panes::state(shared, &b)).map(|s| s.0);
                if checker.as_deref() != Some("working") {
                    rounds::close_panes(shared, &key).await?;
                }
            }
            Ok(None)
        }
        "merged" | "closed" => {
            set_doing(shared, Some(format!("Cleaning up {}", e.short())));
            rounds::cleanup(shared, &key).await?;
            Ok(None)
        }
        _ => Ok(None),
    }
}

/// The checked review of the round in progress, in brief, once the
/// checker has written it.
pub(crate) fn draft_summary(p: &Pr) -> Value {
    if p.state != "reviewing" || p.phase() == "reviewer" {
        return Value::Null;
    }
    let Some(wt) = &p.worktree else { return Value::Null };
    let path = Path::new(wt).join(format!(".tmp/rounds/{}/review.verified.json", p.round));
    let Ok(text) = std::fs::read_to_string(path) else { return Value::Null };
    let Ok(d) = serde_json::from_str::<Value>(&text) else { return Value::Null };
    let total = d["findings"].as_array().map_or(0, Vec::len);
    let blocking = super::post::blocking_count(&d);
    let risk = if d["merge_risk"].is_object() { super::post::risk_level(&d["merge_risk"]).ok() } else { None };
    json!({ "event": d["event"], "verdict": d["verdict"].as_str().or(d["summary"].as_str()), "blocking": blocking, "non_blocking": total - blocking, "merge_risk": risk, "digest": super::rounds::draft_digest(&text) })
}

/// The protocol at a glance, for the app and `wh review`.
pub fn status(shared: &Shared) -> Value {
    let core = shared.lock();
    let prs = super::all(&core).unwrap_or_default();
    let cfg = core.review.config.as_ref().map(|(c, _)| c.clone());
    let mode = cfg.as_ref().map(|c| super::post_mode(&core, c, None));
    let rule = cfg.as_ref().map(|c| super::post_for(&core, c, None).json());
    let list: Vec<Value> = prs
        .iter()
        .filter(|p| !p.finished() || p.cleaned_at.as_deref().and_then(crate::time::secs_since).is_some_and(|s| s < 7 * 86400))
        .filter(|p| p.state != "cleaned" || p.history.last().is_some())
        .map(|p| {
            let mut v = super::summary(p);
            let docket = super::docket_of(&core, &p.key());
            v["full_key"] = json!(p.key());
            v["head"] = json!(p.head);
            v["round_head"] = json!(p.round_head);
            v["reviewers_done"] = json!(p.reviewers_done);
            v["reviewers_total"] = json!(p.reviewers_total);
            v["reviewers"] = json!(p.round_reviewer_list().iter().map(|r| json!({ "name": r.name, "agent": r.agent, "effort": r.effort })).collect::<Vec<_>>());
            v["checker_effort"] = json!(p.checker_effort);
            v["start_error"] = json!(p.start_error);
            v["attention"] = json!(core.review.attention.get(&p.key()));
            v["posted_at"] = json!(p.posted_at);
            v["updated_at"] = json!(p.updated_at);
            // History is for a docket's panel; Recap has its own.
            v["history"] = if docket.is_some() { json!(p.history.iter().rev().collect::<Vec<_>>()) } else { json!([]) };
            v["draft"] = draft_summary(p);
            v["post_decision"] = json!(p.post_decision.as_ref().filter(|d| d["round"] == json!(p.round)));
            v["post"] = json!(cfg.as_ref().map(|c| super::post_mode(&core, c, Some(&p.repo))));
            v["docket"] = json!(docket);
            if p.finished() && docket.is_none() {
                v["implementation"] = Value::Null;
            }
            v
        })
        .collect();
    json!({
        "enabled": super::enabled(&core),
        "paused": super::paused(&core),
        "post": mode,
        "post_rule": rule,
        "post_override": super::post_override(&core).map(|p| p.kind()),
        "config": cfg.as_ref().map(|c| json!({
            "path": crate::paths::tilde(&c.path.to_string_lossy()),
            "repos": c.repo_list(),
            "reviewers": c.reviewers.iter().map(|r| json!({ "name": r.name, "agent": r.agent })).collect::<Vec<_>>(),
            "checker": c.checker, "post": c.post.kind(), "max_rounds": c.max_rounds, "poll_seconds": c.poll_seconds,
            "repo_posts": c.repos.iter().filter_map(|(slug, r)| r.post.as_ref().map(|p| (slug.clone(), p.json()))).collect::<serde_json::Map<String, Value>>(),
        })),
        "config_error": core.review.config_error,
        "last_scan_at": core.review.last_scan_at,
        "scan_error": core.review.scan_error,
        "doing": core.review.doing,
        "rex_reviewer_running": super::rex_reviewer_running(),
        "prs": list,
    })
}
