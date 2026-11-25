//! Recap: what happened over a period, for a standup or a week's note.
//! Reviews posted and their verdicts, the PRs that merged, still wait or
//! closed; the agents' work; what you decided; and the tokens all of it
//! took, by provider and model, and what they cost at each model's price.
//! The app sends the period's bounds in its own time zone.

use anyhow::Result;
use serde_json::{Value, json};
use std::collections::HashMap;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::hub::Core;

fn at(s: Option<&str>) -> Option<OffsetDateTime> {
    OffsetDateTime::parse(s?, &Rfc3339).ok()
}

struct Window {
    since: OffsetDateTime,
    until: OffsetDateTime,
}

impl Window {
    fn has(&self, t: Option<OffsetDateTime>) -> bool {
        t.is_some_and(|t| t >= self.since && t < self.until)
    }

    fn before(&self) -> Window {
        let len = self.until - self.since;
        Window { since: self.since.checked_sub(len).unwrap_or(self.since), until: self.since }
    }

    fn days(&self) -> usize {
        ((self.until - self.since).whole_hours() as f64 / 24.0).ceil().max(1.0) as usize
    }

    fn day(&self, t: OffsetDateTime) -> usize {
        ((t - self.since).whole_hours() / 24).max(0) as usize
    }

    /// Unix seconds, for the usage.
    fn has_secs(&self, t: i64) -> bool {
        t >= self.since.unix_timestamp() && t < self.until.unix_timestamp()
    }
}

/// A docket whose agents' tokens count: the period's and the one before.
pub struct Work {
    id: String,
    claims: Vec<crate::subagents::Claim>,
}

fn median(mut v: Vec<i64>) -> Option<i64> {
    if v.is_empty() {
        return None;
    }
    v.sort();
    let n = v.len();
    Some(if n % 2 == 1 { v[n / 2] } else { (v[n / 2 - 1] + v[n / 2]) / 2 })
}

/// Each docket's title and kind of work, for the usage kept in the store
/// (a docket whose calls were kept may no longer be in the period's work).
pub type Names = HashMap<String, (String, &'static str)>;

/// The recap, all at once (the usage reads files: see `gather`).
#[cfg(test)]
pub fn recap(core: &mut Core, since: &str, until: &str, label: &str) -> Result<Value> {
    let (out, work, names) = gather(core, since, until)?;
    let (from, read_from, to) = bounds(core, since, until, OffsetDateTime::now_utc().unix_timestamp())?;
    let live = read(&work, read_from).rows;
    let kept = core.store.usage_between(from, to)?;
    Ok(finish(out, &names, &merge(kept, live), &crate::pricing::Prices::built_in(), label))
}

/// What a docket's agents' tokens count as.
fn kind_of(core: &Core, st: &crate::record::DocketState) -> Result<&'static str> {
    Ok(match st.protocol.as_str() {
        "review" => "reviews",
        "build" => "builds",
        _ if core.store.build_task(&st.id)?.is_some() => "builds",
        _ => "agents",
    })
}

/// A docket's agents, from its record, if it has any.
fn work_of(core: &Core, st: &crate::record::DocketState) -> Result<Option<Work>> {
    let stored = core.store.entries(&st.id)?;
    let claims = crate::subagents::claims(&stored.iter().filter_map(|s| s.entry().ok()).filter(|e| matches!(e.kind.as_str(), "open" | "claim" | "release")).collect::<Vec<_>>());
    Ok((!claims.is_empty()).then(|| Work { id: st.id.clone(), claims }))
}

/// Every docket with agents, for a snapshot: an old docket's agent may
/// still write (a claim never released), so none is left out by age. In
/// Recap's order (newest first), so a transcript two dockets reach counts
/// for the same one in both. The hub's lock is taken for one docket at a
/// time.
fn all_work(shared: &crate::hub::Shared) -> Result<Vec<Work>> {
    let ids: Vec<String> = shared.lock().store.states()?.into_iter().map(|s| s.id).collect();
    let mut work = Vec::new();
    for id in ids {
        let core = shared.lock();
        if let Some(st) = core.store.state(&id)? {
            work.extend(work_of(&core, &st)?);
        }
    }
    Ok(work)
}

/// How far the snapshots have kept everything (unix seconds): every
/// transcript last written before this, less `SNAPSHOT_SLACK`, is in the
/// store whole.
const SNAPSHOT: &str = "usage.snapshot";
/// When the last snapshot ran (unix seconds), for when the next one's due.
const SNAPSHOT_RAN: &str = "usage.snapshot_ran";
/// Transcripts written this long before the mark are read again anyway.
const SNAPSHOT_SLACK: i64 = 2 * 86400;
/// A mark further ahead than this is the clock's mistake, and isn't used.
const CLOCK_SLACK: i64 = 3600;

fn meta_secs(core: &Core, k: &str, now: i64) -> Result<Option<i64>> {
    Ok(core.store.meta_get(k)?.and_then(|v| v.parse::<i64>().ok()).filter(|t| *t <= now + CLOCK_SLACK))
}

/// A recap's usage bounds (unix seconds): from the start of the period
/// before, the time its transcripts are read from (later, when a snapshot
/// already kept what came before), and the period's end.
pub fn bounds(core: &Core, since: &str, until: &str, now: i64) -> Result<(i64, i64, i64)> {
    let w = window(since, until)?;
    let from = w.before().since.unix_timestamp();
    Ok((from, read_from(from, meta_secs(core, SNAPSHOT, now)?), w.until.unix_timestamp()))
}

fn read_from(from: i64, snapshot: Option<i64>) -> i64 {
    snapshot.map_or(from, |s| from.max(s.saturating_sub(SNAPSHOT_SLACK)))
}

fn window(since: &str, until: &str) -> Result<Window> {
    Ok(Window { since: at(Some(since)).ok_or_else(|| anyhow::anyhow!("since isn't a time"))?, until: at(Some(until)).ok_or_else(|| anyhow::anyhow!("until isn't a time"))? })
}

/// Everything in the recap but its usage and Markdown, from the hub (under
/// its lock), and the dockets whose agents' tokens count. `finish` reads
/// their transcripts, without the lock.
pub fn gather(core: &Core, since: &str, until: &str) -> Result<(Value, Vec<Work>, Names)> {
    let w = window(since, until)?;
    let prev = w.before();
    let days = w.days();

    // ---- Reviews ----
    let prs = crate::review::all(core)?;
    let (mut posted, mut prev_posted, mut approved, mut changes, mut dismissed) = (0, 0, 0, 0, 0);
    let mut per_day = vec![0; days];
    let mut waits = Vec::new();
    let (mut tok_total, mut tok_cached) = (0i64, 0i64);
    let (mut merged, mut open, mut closed) = (Vec::new(), Vec::new(), Vec::new());
    let mut prev_merged = 0;
    for p in &prs {
        let mut active = false;
        for h in &p.history {
            let t = at(h["at"].as_str());
            let verdict = h["verdict"].as_str().unwrap_or("");
            if prev.has(t) && verdict != "DISMISSED" {
                prev_posted += 1;
            }
            if !w.has(t) {
                continue;
            }
            active = true;
            match verdict {
                "DISMISSED" => {
                    dismissed += 1;
                    continue;
                }
                "APPROVED" => approved += 1,
                "CHANGES_REQUESTED" => changes += 1,
                _ => {}
            }
            posted += 1;
            if let Some(t) = t {
                if let Some(d) = per_day.get_mut(w.day(t)) {
                    *d += 1;
                }
                if let Some(r) = at(h["requested_at"].as_str()) {
                    waits.push((t - r).whole_seconds());
                }
            }
            tok_total += h["tokens"]["total"].as_i64().unwrap_or(0);
            tok_cached += h["tokens"]["cache_read"].as_i64().unwrap_or(0);
        }
        let ended = at(p.ended_at.as_deref());
        if p.outcome.as_deref() == Some("merged") && prev.has(ended) {
            prev_merged += 1;
        }
        let item = || {
            json!({
                "key": p.short(), "full_key": p.key(), "title": p.title, "url": p.url, "author": p.author,
                "docket": crate::review::docket_of(core, &p.key()),
                "rounds": p.history.iter().filter(|h| h["verdict"] != "DISMISSED").count(),
                "verdicts": p.history.iter().filter_map(|h| h["verdict"].as_str()).filter(|v| *v != "DISMISSED").collect::<Vec<_>>(),
                "merge_risk": p.merge_risk, "tokens": p.tokens.as_ref().and_then(|t| t["total"].as_i64()),
                "additions": p.additions, "deletions": p.deletions, "implementation": p.implementation,
                "state": p.state, "outcome": p.outcome, "ended_at": p.ended_at,
                "last_at": p.history.last().and_then(|h| h["at"].as_str()),
            })
        };
        // A PR you never posted a review of isn't one you reviewed.
        let reviewed = p.history.iter().any(|h| h["verdict"] != "DISMISSED");
        match p.outcome.as_deref() {
            _ if !reviewed => {}
            Some("merged") if w.has(ended) => merged.push(item()),
            Some("closed") if w.has(ended) => closed.push(item()),
            _ if matches!(p.state.as_str(), "queued" | "reviewing" | "posted") && (active || w.has(at(p.updated_at.as_deref()))) => open.push(item()),
            _ => {}
        }
    }
    let by_end = |a: &Value, b: &Value| b["ended_at"].as_str().cmp(&a["ended_at"].as_str());
    merged.sort_by(by_end);
    closed.sort_by(by_end);
    open.sort_by(|a, b| b["last_at"].as_str().cmp(&a["last_at"].as_str()));

    // ---- Agents, and you ----
    let (mut started, mut turns, mut prompts, mut intents) = (0, 0, 0, 0);
    let (mut ok, mut denied, mut asked, mut answered) = (0, 0, 0, 0);
    let mut turns_per_day = vec![0; days];
    let mut done = Vec::new();
    let mut decisions = Vec::new();
    // Builds: PRs opened, address rounds pushed (and their replies), and how
    // each ended, from the events Build puts on its docket.
    let mut builds = Vec::new();
    let (mut b_opened, mut b_rounds, mut b_replies, mut b_merged) = (0, 0, 0, 0);
    let mut work = Vec::new();
    let mut names = Names::new();
    for st in core.store.states()? {
        let kind = kind_of(core, &st)?;
        names.insert(st.id.clone(), (st.title.clone(), kind));
        let updated = at(Some(&st.updated_at));
        if updated.is_some_and(|u| u < prev.since) {
            continue;
        }
        work.extend(work_of(core, &st)?);
        if updated.is_some_and(|u| u < w.since) {
            continue;
        }
        let stored = core.store.entries(&st.id)?;
        let mut d_turns = 0;
        let mut finished = None;
        let mut b: Option<Value> = None;
        for s in &stored {
            let Ok(e) = s.entry() else { continue };
            let t = at(Some(&e.at));
            if !w.has(t) {
                continue;
            }
            let you = e.actor.typ == "person";
            if e.kind == "observed" && e.data["what"] == "build-event" {
                let x = b.get_or_insert_with(|| json!({ "id": st.id, "title": st.title, "opened": false, "rounds": 0, "replies": 0, "outcome": null, "at": e.at }));
                x["at"] = json!(e.at);
                match e.data["event"].as_str() {
                    Some("opened") => {
                        b_opened += 1;
                        x["opened"] = json!(true);
                        x["pr"] = e.data["pr"].clone();
                        x["url"] = e.data["url"].clone();
                    }
                    Some("pushed-round") => {
                        b_rounds += 1;
                        let r = e.data["replies"].as_i64().unwrap_or(0);
                        b_replies += r;
                        x["rounds"] = json!(x["rounds"].as_i64().unwrap_or(0) + 1);
                        x["replies"] = json!(x["replies"].as_i64().unwrap_or(0) + r);
                    }
                    Some(o @ ("merged" | "closed" | "discarded")) => {
                        if o == "merged" {
                            b_merged += 1;
                        }
                        x["outcome"] = json!(o);
                    }
                    _ => {}
                }
                continue;
            }
            match e.kind.as_str() {
                "open" if e.data["protocol"] == "agent" => started += 1,
                "turn" => {
                    turns += 1;
                    d_turns += 1;
                    if let Some(x) = t.and_then(|t| turns_per_day.get_mut(w.day(t))) {
                        *x += 1;
                    }
                }
                "note" if you && e.data["via"] == "prompt" => prompts += 1,
                "intent" if you => intents += 1,
                "question" => asked += 1,
                "decision" if you => {
                    answered += 1;
                    decisions
                        .push(json!({ "docket": st.id, "title": st.title, "choice": e.data["choice"], "subject": e.data["subject"], "message": e.data["replies_to"], "at": e.at }));
                }
                "approval" if you => {
                    if e.data["decision"] == "deny" || e.data["decision"] == "dismiss" {
                        denied += 1;
                    } else {
                        ok += 1;
                    }
                }
                "set" if e.data["status"] == "done" => finished = Some(e.at.clone()),
                _ => {}
            }
        }
        if let Some(mut x) = b {
            if let Ok(Some(t)) = crate::build::get(core, &st.id) {
                x["key"] = json!(t.key);
                x["repo"] = json!(t.repo);
                if x["url"].is_null() {
                    x["url"] = json!(t.pr_url);
                    x["pr"] = json!(t.pr_number);
                }
            }
            builds.push(x);
        }
        if let Some(when) = finished
            && st.status == "done"
            && st.protocol != "review"
        {
            done.push(json!({ "id": st.id, "title": st.title, "protocol": st.protocol, "app": st.agent.as_ref().map(|a| a.app.clone()), "turns": d_turns, "at": when, "summary": st.last_turn.as_deref().and_then(crate::agents::summarize) }));
        }
    }
    done.sort_by(|a, b| b["at"].as_str().cmp(&a["at"].as_str()));
    builds.sort_by(|a, b| b["at"].as_str().cmp(&a["at"].as_str()));
    decisions.sort_by(|a, b| b["at"].as_str().cmp(&a["at"].as_str()));

    let out = json!({
        "since": since, "until": until, "days": days,
        "reviews": {
            "posted": posted, "prev_posted": prev_posted, "per_day": per_day,
            "approved": approved, "changes": changes, "dismissed": dismissed,
            "merged": merged.len(), "prev_merged": prev_merged,
            "median_to_review": median(waits),
            "tokens": tok_total, "cached": if tok_total > 0 { Some(tok_cached as f64 / tok_total as f64) } else { None },
            "prs": { "merged": merged, "open": open, "closed": closed },
        },
        "agents": { "started": started, "turns": turns, "per_day": turns_per_day, "prompts": prompts, "done": done },
        "builds": { "opened": b_opened, "rounds": b_rounds, "replies": b_replies, "merged": b_merged, "list": builds },
        "you": { "decisions": decisions, "approved": ok, "denied": denied, "asked": asked, "answered": answered, "intents": intents },
    });
    Ok((out, work, names))
}

/// What reading the work's transcripts takes: each file once (by its
/// `usage::file_key`, however many dockets or paths reach it), and each
/// docket's hold on it.
pub struct Plan {
    ids: Vec<String>,
    /// (app, path, key)
    files: Vec<(String, std::path::PathBuf, String)>,
    spans: Vec<Span>,
}

pub fn plan(work: &[Work]) -> Plan {
    let mut finder = crate::subagents::Finder::new();
    let mut found: Vec<(usize, crate::subagents::Transcript)> = work.iter().enumerate().flat_map(|(i, d)| finder.transcripts(&d.claims).into_iter().map(move |t| (i, t))).collect();
    // A session a claim names is that docket's before one found by its
    // folder (a claim left open can't take another docket's).
    found.sort_by_key(|(_, t)| !t.named);
    let mut files = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    let spans = found
        .into_iter()
        .map(|(i, t)| {
            let key = crate::usage::file_key(&t.app, &t.path);
            let k = *index.entry(key.clone()).or_insert_with(|| {
                files.push((t.app.clone(), t.path.clone(), key));
                files.len() - 1
            });
            Span { docket: i, file: k, from: t.from, until: t.until }
        })
        .collect();
    Plan { ids: work.iter().map(|w| w.id.clone()).collect(), files, spans }
}

/// What a read came to: its rows, and the transcripts it couldn't read.
#[derive(Default)]
pub struct Read {
    pub rows: Vec<crate::usage::Row>,
    pub failed: usize,
    /// The oldest last write among those (unix seconds), when known.
    pub failed_written: Option<i64>,
}

/// Every call in the work's transcripts written since `from` (unix
/// seconds), as rows, each counted for the first docket that held it.
/// Reads files: call it off the hub's lock.
pub fn read(work: &[Work], from: i64) -> Read {
    let p = plan(work);
    let r = read_part(&p, 0, p.files.len(), from);
    crate::usage::forget_stale();
    r
}

/// The plan's files `lo..hi`, read by a few threads (a first read can be
/// gigabytes), as rows. A file's rows depend only on its own spans, so
/// reading in parts counts the same as reading at once.
fn read_part(p: &Plan, lo: usize, hi: usize, from: i64) -> Read {
    let hi = hi.min(p.files.len());
    let part = &p.files[lo.min(hi)..hi];
    let next = std::sync::atomic::AtomicUsize::new(0);
    let got: Vec<Option<Vec<crate::usage::Call>>> = std::thread::scope(|sc| {
        let jobs: Vec<_> = (0..8)
            .map(|_| {
                sc.spawn(|| {
                    let mut mine = Vec::new();
                    loop {
                        let k = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some((app, path, _)) = part.get(k) else { break };
                        // A file that can't be read is left out, not the rest.
                        let calls = std::panic::catch_unwind(|| crate::usage::calls(app, path, from)).ok().flatten();
                        mine.push((k, calls));
                    }
                    mine
                })
            })
            .collect();
        let mut all = vec![None; part.len()];
        for (k, c) in jobs.into_iter().flat_map(|j| j.join().unwrap_or_default()) {
            all[k] = c;
        }
        all
    });
    let mut out = Read::default();
    for (k, c) in got.iter().enumerate() {
        if c.is_some() {
            continue;
        }
        out.failed += 1;
        let path = &part[k].1;
        let written = std::fs::metadata(path)
            .or_else(|_| std::fs::metadata(format!("{}.zst", path.display())))
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::SystemTime::UNIX_EPOCH).ok())
            .map_or(from, |d| d.as_secs() as i64);
        out.failed_written = Some(out.failed_written.map_or(written, |w| w.min(written)));
    }
    let spans: Vec<&Span> = p.spans.iter().filter(|s| (lo..hi).contains(&s.file)).collect();
    out.rows = attribute(&p.ids, &spans, |f| got.get(f.wrapping_sub(lo)).and_then(Option::as_deref), |f| p.files.get(f).map(|x| x.2.clone()));
    out
}

/// What Recap counts: what the store keeps, with what it just read over
/// it. A minute read now replaces the kept one unless the kept one holds
/// more (a snapshot reads every docket's transcripts, and may have
/// counted a call the period's work doesn't reach).
pub fn merge(kept: Vec<crate::usage::Row>, live: Vec<crate::usage::Row>) -> Vec<crate::usage::Row> {
    // Whole minutes of a transcript, every model's: a model named
    // differently now is the same calls, not more.
    type Minute = (String, i64);
    let group = |rows: Vec<crate::usage::Row>| {
        let mut by: HashMap<Minute, Vec<crate::usage::Row>> = HashMap::new();
        for r in rows {
            by.entry((r.file.clone(), r.minute)).or_default().push(r);
        }
        by
    };
    let total = |rs: &[crate::usage::Row]| rs.iter().fold(0i64, |a, r| a.saturating_add(r.total()));
    let mut by = group(kept);
    for (k, rs) in group(live) {
        if by.get(&k).is_none_or(|was| total(&rs) >= total(was)) {
            by.insert(k, rs);
        }
    }
    let mut out: Vec<crate::usage::Row> = by.into_values().flatten().collect();
    out.sort_by(|a, b| (a.minute, &a.file, &a.provider, &a.model).cmp(&(b.minute, &b.file, &b.provider, &b.model)));
    out
}

/// The recap with its usage, from the rows `merge` gave, at `prices`, and
/// its Markdown.
pub fn finish(mut out: Value, names: &Names, rows: &[crate::usage::Row], prices: &crate::pricing::Prices, label: &str) -> Value {
    if let (Some(s), Some(u)) = (out["since"].as_str(), out["until"].as_str())
        && let Ok(w) = window(s, u)
    {
        out["usage"] = usage(&w, names, rows, prices);
    }
    out["markdown"] = json!(markdown(&out, label));
    out
}

/// A docket's hold on a file (an index into the plan's files): its calls
/// from `from` to `until` are that docket's.
struct Span {
    docket: usize,
    file: usize,
    from: i64,
    until: Option<i64>,
}

/// Each file's calls as rows, a minute and a model at a time: each call
/// counts once, for the first span that holds it, and a minute's row goes
/// to the docket its first call counted for. Calls no span holds are some
/// other work's, and aren't kept. `calls` and `key` look a file up by its
/// index; one that wasn't read has no calls.
fn attribute<'a>(ids: &[String], spans: &[&Span], calls: impl Fn(usize) -> Option<&'a [crate::usage::Call]>, key: impl Fn(usize) -> Option<String>) -> Vec<crate::usage::Row> {
    use std::collections::{BTreeMap, HashSet};
    let mut seen: HashSet<(usize, usize)> = HashSet::new();
    // (file, minute, provider, model) → (docket, tokens, 1-hour writes)
    type Minute = (usize, i64, String, String);
    let mut rows: BTreeMap<Minute, (usize, [i64; 4], i64)> = BTreeMap::new();
    for sp in spans {
        for (j, c) in calls(sp.file).into_iter().flatten().enumerate() {
            if c.at < sp.from || sp.until.is_some_and(|u| c.at > u) || !seen.insert((sp.file, j)) {
                continue;
            }
            let r = rows.entry((sp.file, c.at.div_euclid(60) * 60, c.provider.clone(), c.model.clone())).or_insert((sp.docket, [0; 4], 0));
            for (t, n) in r.1.iter_mut().zip(c.tokens) {
                *t = t.saturating_add(n);
            }
            r.2 = r.2.saturating_add(c.write_1h);
        }
    }
    rows.into_iter()
        .filter_map(|((file, minute, provider, model), (d, tokens, write_1h))| {
            Some(crate::usage::Row { file: key(file)?, minute, provider, model, docket: ids.get(d)?.clone(), tokens, write_1h })
        })
        .collect()
}

/// Tokens over the window, by provider, model, kind of work, day and
/// docket; the window before's total for comparison. From the rows the
/// store keeps, each a minute of one transcript's calls. Each count's cost
/// beside it, at the model's price when the minute was (`pricing`), and
/// the tokens of models with no price, so a cost never quietly leaves
/// them out.
fn usage(w: &Window, names: &Names, kept: &[crate::usage::Row], prices: &crate::pricing::Prices) -> Value {
    use crate::pricing::{Price, Spend};
    use std::collections::{BTreeMap, HashSet};
    let prev = w.before();
    let (mut total, mut prev_total) = (0i64, 0i64);
    let (mut spend, mut prev_spend) = (Spend::default(), Spend::default());
    // The four fields, then the writes that went to the 1-hour cache.
    let mut split = [0i64; 5];
    let mut per_day = vec![0i64; w.days()];
    let mut cost_per_day = vec![Spend::default(); w.days()];
    // (provider, model) → (fields, dockets, cost, the price last used)
    type Model<'a> = ([i64; 5], HashSet<&'a str>, Spend, Option<Price>);
    let mut models: BTreeMap<(String, String), Model> = BTreeMap::new();
    let mut kinds: BTreeMap<&str, (i64, Spend)> = BTreeMap::new();
    let mut dockets: BTreeMap<&str, (i64, Spend, BTreeMap<String, i64>)> = BTreeMap::new();
    let kind = |id: &str| names.get(id).map_or("agents", |n| n.1);
    for r in kept {
        let n = r.total();
        let price = prices.at(&r.provider, &r.model, r.minute);
        let cost = price.map(|p| p.cost(r.tokens, r.write_1h));
        if prev.has_secs(r.minute) {
            prev_total = prev_total.saturating_add(n);
            prev_spend.add(cost, n);
        }
        if !w.has_secs(r.minute) {
            continue;
        }
        total = total.saturating_add(n);
        spend.add(cost, n);
        let m = models.entry((r.provider.clone(), r.model.clone())).or_default();
        let hour = r.write_1h.clamp(0, r.tokens[2].max(0));
        for (k, t) in r.tokens.iter().chain([&hour]).enumerate() {
            split[k] = split[k].saturating_add(*t);
            m.0[k] = m.0[k].saturating_add(*t);
        }
        m.1.insert(&r.docket);
        m.2.add(cost, n);
        m.3 = price.or(m.3);
        if let Ok(t) = OffsetDateTime::from_unix_timestamp(r.minute) {
            let day = w.day(t);
            if let Some(d) = per_day.get_mut(day) {
                *d = d.saturating_add(n);
            }
            if let Some(d) = cost_per_day.get_mut(day) {
                d.add(cost, n);
            }
        }
        let k = kinds.entry(kind(&r.docket)).or_default();
        k.0 = k.0.saturating_add(n);
        k.1.add(cost, n);
        let d = dockets.entry(&r.docket).or_default();
        d.0 = d.0.saturating_add(n);
        d.1.add(cost, n);
        let dm = d.2.entry(r.model.clone()).or_default();
        *dm = dm.saturating_add(n);
    }
    let fields = |t: [i64; 5]| json!({ "input": t[0], "cache_read": t[1], "cache_write": t[2], "output": t[3], "cache_write_1h": t[4] });
    let mut by_provider: BTreeMap<String, (i64, Spend)> = BTreeMap::new();
    let mut unpriced: Vec<Value> = Vec::new();
    let mut model_list: Vec<Value> = models
        .into_iter()
        .map(|((provider, model), (t, ds, c, price))| {
            let n: i64 = t[..4].iter().fold(0i64, |a, x| a.saturating_add(*x));
            let p = by_provider.entry(provider.clone()).or_default();
            p.0 = p.0.saturating_add(n);
            p.1.merge(c);
            let provider_name = crate::usage::provider_name(&provider);
            if c.unpriced > 0 {
                unpriced.push(json!({ "model": model, "provider": provider, "provider_name": provider_name, "tokens": c.unpriced }));
            }
            json!({
                "model": model, "provider": provider, "provider_name": provider_name, "tokens": n, "split": fields(t), "dockets": ds.len(),
                // No price: no cost, rather than a cost of nothing.
                "cost": price.map(|_| c.usd()), "unpriced": c.unpriced,
                "price": price.map(|p| p.json(prices.source(&provider, &model))),
            })
        })
        .collect();
    model_list.sort_by_key(|m| std::cmp::Reverse(m["tokens"].as_i64().unwrap_or(0)));
    unpriced.sort_by_key(|m| std::cmp::Reverse(m["tokens"].as_i64().unwrap_or(0)));
    let mut providers: Vec<Value> = by_provider
        .into_iter()
        .map(|(p, (n, c))| json!({ "provider": p, "name": crate::usage::provider_name(&p), "tokens": n, "cost": c.usd(), "unpriced": c.unpriced }))
        .collect();
    providers.sort_by_key(|p| std::cmp::Reverse(p["tokens"].as_i64().unwrap_or(0)));
    let mut top: Vec<(i64, &str, Spend, BTreeMap<String, i64>)> = dockets.into_iter().map(|(id, (n, c, m))| (n, id, c, m)).collect();
    top.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| docket_order(a.1).cmp(&docket_order(b.1))));
    let top: Vec<Value> = top
        .into_iter()
        .take(10)
        .map(|(n, id, c, m)| {
            let mut m: Vec<(String, i64)> = m.into_iter().collect();
            m.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
            let title = names.get(id).map_or(id, |n| n.0.as_str());
            json!({ "id": id, "title": title, "kind": kind(id), "tokens": n, "cost": c.usd(), "unpriced": c.unpriced, "models": m.into_iter().map(|(name, _)| name).collect::<Vec<_>>() })
        })
        .collect();
    let input_side = split[0].saturating_add(split[1]).saturating_add(split[2]);
    let work = |k: &str| kinds.get(k).copied().unwrap_or_default();
    json!({
        "tokens": total, "prev_tokens": prev_total, "split": fields(split),
        "cached": if input_side > 0 { Some(split[1] as f64 / input_side as f64) } else { None },
        "per_day": per_day, "providers": providers, "models": model_list,
        "work": { "agents": work("agents").0, "builds": work("builds").0, "reviews": work("reviews").0 },
        "dockets": top,
        // US dollars at API prices; `unpriced` is what they leave out.
        "cost": spend.usd(), "prev_cost": prev_spend.usd(),
        "cost_per_day": cost_per_day.iter().map(Spend::usd).collect::<Vec<_>>(),
        "work_cost": { "agents": work("agents").1.usd(), "builds": work("builds").1.usd(), "reviews": work("reviews").1.usd() },
        "unpriced": { "tokens": spend.unpriced, "prev_tokens": prev_spend.unpriced, "models": unpriced },
        "price_notes": prices.notes,
    })
}

/// `D-12` before `D-112`, and a shared docket's by its own prefix.
fn docket_order(id: &str) -> (&str, i64) {
    match id.rsplit_once('-') {
        Some((p, n)) => (p, n.parse().unwrap_or(i64::MAX)),
        None => (id, i64::MAX),
    }
}

/// Keep what the agents' transcripts came to, about daily, so Recap's
/// tokens outlast the tools pruning them: the first time, everything on
/// disk; then whatever was written since a little before the mark. A
/// snapshot that fails (or the hub stopping in one) is taken again.
pub async fn snapshot_loop(shared: crate::hub::Shared) {
    const DAY: i64 = 86400;
    // Let the hub settle before the first one, which can read a lot.
    tokio::time::sleep(std::time::Duration::from_secs(90)).await;
    loop {
        let now = OffsetDateTime::now_utc().unix_timestamp();
        let due = {
            let core = shared.lock();
            let ran = meta_secs(&core, SNAPSHOT_RAN, now);
            let mark = meta_secs(&core, SNAPSHOT, now);
            match (ran, mark) {
                (Ok(ran), Ok(mark)) => ran.is_none_or(|r| now - r >= DAY - 3600).then_some(mark),
                (Err(e), _) | (_, Err(e)) => {
                    say!("couldn't read when the tokens were last kept: {e:#}");
                    None
                }
            }
        };
        if let Some(mark) = due {
            let first = mark.is_none();
            let sh = shared.clone();
            let took = tokio::task::spawn_blocking(move || snapshot(&sh, mark.map_or(0, |m| m.saturating_sub(SNAPSHOT_SLACK)), now)).await;
            match took {
                Ok(Ok((rows, 0))) => say!("kept the tokens of {rows} transcript-minutes{}", if first { " (everything on disk, the first time)" } else { "" }),
                Ok(Ok((rows, failed))) => say!("kept the tokens of {rows} transcript-minutes; {failed} transcripts couldn't be read, and are tried again next time"),
                Ok(Err(e)) => say!("couldn't keep the tokens: {e:#}; trying again within the hour"),
                Err(e) => say!("couldn't keep the tokens: {e}; trying again within the hour"),
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
    }
}

/// Files read and kept at a time: what's held in memory at once.
const SNAPSHOT_PART: usize = 64;

/// One snapshot: every docket's transcripts written since `from`, read a
/// part at a time and kept (the lock held only to write a batch). Then the
/// mark: `now`, or early enough that a transcript that couldn't be read is
/// read again. Returns the rows kept and the transcripts that failed.
fn snapshot(shared: &crate::hub::Shared, from: i64, now: i64) -> Result<(usize, usize)> {
    let p = plan(&all_work(shared)?);
    let (mut rows, mut failed, mut oldest) = (0, 0, None::<i64>);
    for lo in (0..p.files.len()).step_by(SNAPSHOT_PART) {
        let r = read_part(&p, lo, lo + SNAPSHOT_PART, from);
        keep(shared, &r.rows)?;
        crate::usage::forget_stale();
        rows += r.rows.len();
        failed += r.failed;
        oldest = match (oldest, r.failed_written) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
    }
    let mark = mark_after(now, from, oldest);
    let mut core = shared.lock();
    core.store.meta_set(SNAPSHOT, &mark.to_string())?;
    core.store.meta_set(SNAPSHOT_RAN, &now.to_string())?;
    Ok((rows, failed))
}

/// How far a snapshot that read from `from` kept everything: `now`, or,
/// when a transcript last written at `failed` couldn't be read, early
/// enough that Recap and the next snapshot read it again. Never before
/// where the last snapshot's mark stood (`from` plus the slack).
fn mark_after(now: i64, from: i64, failed: Option<i64>) -> i64 {
    let floor = from.saturating_add(SNAPSHOT_SLACK).min(now);
    // One that's failed for a week (no zstd, a file it can't open) stops
    // holding every later read back: it's said in the log each time.
    let failed = failed.filter(|w| now.saturating_sub(*w) < GIVE_UP);
    failed.map_or(now, |w| now.min(w.saturating_add(SNAPSHOT_SLACK)).max(floor))
}

/// How long a transcript that can't be read holds the mark back.
const GIVE_UP: i64 = 7 * 86400;

/// Keep rows in the store a batch at a time, so a long snapshot never
/// holds the hub's lock for long. A batch ends only between minutes: each
/// replaces its minutes whole.
fn keep(shared: &crate::hub::Shared, rows: &[crate::usage::Row]) -> Result<()> {
    let mut start = 0;
    while start < rows.len() {
        let mut end = (start + 2000).min(rows.len());
        while end < rows.len() && (rows[end].file.as_str(), rows[end].minute) == (rows[end - 1].file.as_str(), rows[end - 1].minute) {
            end += 1;
        }
        shared.lock().store.put_usage(&rows[start..end])?;
        start = end;
    }
    Ok(())
}

fn compact(n: i64) -> String {
    match n {
        n if n >= 1_000_000_000 => format!("{:.1}B", n as f64 / 1e9).replace(".0B", "B"),
        n if n >= 1_000_000 => format!("{:.1}M", n as f64 / 1e6).replace(".0M", "M"),
        n if n >= 10_000 => format!("{}k", n / 1000),
        n => n.to_string(),
    }
}

fn duration(secs: i64) -> String {
    match secs {
        s if s < 3600 => format!("{}m", (s / 60).max(1)),
        s if s < 86400 => format!("{}h {}m", s / 3600, (s % 3600) / 60),
        s => format!("{}d {}h", s / 86400, (s % 86400) / 3600),
    }
}

fn mark(v: &str) -> &'static str {
    match v {
        "APPROVED" => "✓",
        "CHANGES_REQUESTED" => "✕",
        _ => "·",
    }
}

/// Dollars in the JSON, as `pricing::money` writes them.
fn usd(v: &Value) -> String {
    crate::pricing::money(crate::pricing::usd_micros(v.as_f64().unwrap_or(0.0)))
}

/// A count's cost after its tokens: `, $1.23`, `, $1.23 (5k tokens
/// unpriced)`, or `, unpriced` when none of it has a price.
fn cost_of(x: &Value) -> String {
    let (tokens, unpriced) = (x["tokens"].as_i64().unwrap_or(0), x["unpriced"].as_i64().unwrap_or(0));
    if tokens > 0 && unpriced >= tokens {
        ", unpriced".into()
    } else if unpriced > 0 {
        format!(", {} ({} tokens unpriced)", usd(&x["cost"]), compact(unpriced))
    } else {
        format!(", {}", usd(&x["cost"]))
    }
}

/// What the period's tokens cost, by provider and model and kind of work,
/// and the models with no price, whose tokens the cost leaves out. When
/// none has a price, it says so, never "$0".
fn cost_line(us: &Value) -> String {
    let micros = |v: &Value| crate::pricing::usd_micros(v.as_f64().unwrap_or(0.0));
    let off = us["unpriced"]["tokens"].as_i64().unwrap_or(0);
    let which: Vec<String> = us["unpriced"]["models"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|m| format!("{} ({})", m["model"].as_str().unwrap_or(""), m["provider_name"].as_str().unwrap_or("")))
        .collect();
    if off > 0 && off >= us["tokens"].as_i64().unwrap_or(0) {
        return format!("**Cost:** unpriced, {} tokens with no price: {}\n", compact(off), which.join(", "));
    }
    let d = micros(&us["cost"]).saturating_sub(micros(&us["prev_cost"]));
    let sign = match d {
        d if d > 0 => "+",
        d if d < 0 => "−",
        _ => "±",
    };
    let mut line = vec![format!("{} at API prices ({sign}{} vs the period before)", usd(&us["cost"]), crate::pricing::money(d.saturating_abs()))];
    let models = us["models"].as_array().cloned().unwrap_or_default();
    for p in us["providers"].as_array().into_iter().flatten() {
        let mine: Vec<String> = models
            .iter()
            .filter(|m| m["provider"] == p["provider"] && !m["cost"].is_null())
            .map(|m| format!("{} {}", m["model"].as_str().unwrap_or(""), usd(&m["cost"])))
            .collect();
        if !mine.is_empty() {
            line.push(format!("{} {} ({})", p["name"].as_str().unwrap_or(""), usd(&p["cost"]), mine.join(", ")));
        }
    }
    let kinds: Vec<String> = [("agents", "agents"), ("builds", "Builds"), ("reviews", "reviews")]
        .iter()
        .filter(|(k, _)| micros(&us["work_cost"][k]) > 0)
        .map(|(k, w)| format!("{w} {}", usd(&us["work_cost"][k])))
        .collect();
    if !kinds.is_empty() {
        line.push(kinds.join(", "));
    }
    if off > 0 {
        line.push(format!("{} tokens unpriced: {}", compact(off), which.join(", ")));
    }
    format!("**Cost:** {}\n", line.join(" · "))
}

/// The recap as a standup or weekly note.
pub fn markdown(r: &Value, label: &str) -> String {
    let rv = &r["reviews"];
    let n = |v: &Value| v.as_i64().unwrap_or(0);
    let mut md = format!("## Recap: {label}\n\n");
    let diff = n(&rv["posted"]) - n(&rv["prev_posted"]);
    let mut line =
        vec![format!("{} review{} posted ({}{diff} vs the period before)", n(&rv["posted"]), if n(&rv["posted"]) == 1 { "" } else { "s" }, if diff >= 0 { "+" } else { "" })];
    line.push(format!("{} approved", n(&rv["approved"])));
    line.push(format!("{} changes requested", n(&rv["changes"])));
    line.push(format!("{} merged", n(&rv["merged"])));
    if let Some(m) = rv["median_to_review"].as_i64() {
        line.push(format!("median {} from request to review", duration(m)));
    }
    if n(&rv["tokens"]) > 0 {
        line.push(format!("{} tokens{}", compact(n(&rv["tokens"])), rv["cached"].as_f64().map(|c| format!(", {:.0}% cached", c * 100.0)).unwrap_or_default()));
    }
    md.push_str(&format!("**Reviews:** {}\n", line.join(" · ")));
    for (title, key) in [("Merged", "merged"), ("Waiting on their authors", "open"), ("Closed without merging", "closed")] {
        let list = rv["prs"][key].as_array().cloned().unwrap_or_default();
        if list.is_empty() {
            continue;
        }
        md.push_str(&format!("\n**{title}**\n"));
        let more = list.len().saturating_sub(20);
        for p in list.into_iter().take(20) {
            let trail: Vec<&str> = p["verdicts"].as_array().into_iter().flatten().filter_map(|v| v.as_str()).map(mark).collect();
            let mut bits = vec![];
            if !trail.is_empty() {
                bits.push(trail.join(" → "));
            }
            bits.push(format!("{} round{}", n(&p["rounds"]), if n(&p["rounds"]) == 1 { "" } else { "s" }));
            if let Some(risk) = p["merge_risk"].as_str() {
                bits.push(format!("{risk} risk"));
            }
            if let Some(a) = p["author"].as_str() {
                bits.push(format!("@{a}"));
            }
            let link = match p["url"].as_str() {
                Some(u) => format!("[{}]({u})", p["key"].as_str().unwrap_or("")),
                None => p["key"].as_str().unwrap_or("").to_string(),
            };
            let title = p["title"].as_str().filter(|t| !t.is_empty()).map(|t| format!(" {t}")).unwrap_or_default();
            md.push_str(&format!("- {link}{title}: {}", bits.join(", ")));
            if let Some(i) = p["implementation"].as_str() {
                md.push_str(&format!(". {i}"));
            }
            md.push('\n');
        }
        if more > 0 {
            md.push_str(&format!("- … and {more} more\n"));
        }
    }
    let ag = &r["agents"];
    let done = ag["done"].as_array().cloned().unwrap_or_default();
    md.push_str(&format!(
        "\n**Agents:** {} docket{} started, {} done, {} turns, {} prompt{} from you\n",
        n(&ag["started"]),
        if n(&ag["started"]) == 1 { "" } else { "s" },
        done.len(),
        n(&ag["turns"]),
        n(&ag["prompts"]),
        if n(&ag["prompts"]) == 1 { "" } else { "s" }
    ));
    for d in done.iter().take(15) {
        md.push_str(&format!("- {} {}", d["id"].as_str().unwrap_or(""), d["title"].as_str().unwrap_or("")));
        if let Some(s) = d["summary"].as_str() {
            md.push_str(&format!(": {s}"));
        }
        md.push('\n');
    }
    let bs = &r["builds"];
    let list = bs["list"].as_array().cloned().unwrap_or_default();
    if !list.is_empty() {
        let s = |k: i64, w: &str| format!("{k} {w}{}", if k == 1 { "" } else { "s" });
        md.push_str(&format!(
            "\n**Builds:** {} opened, {} ({}), {} merged\n",
            s(n(&bs["opened"]), "PR"),
            s(n(&bs["rounds"]), "address round"),
            s(n(&bs["replies"]), "reply").replace("replys", "replies"),
            n(&bs["merged"])
        ));
        for x in list.iter().take(15) {
            let pr = match (x["pr"].as_i64(), x["url"].as_str()) {
                (Some(p), Some(u)) => format!(" [#{p}]({u})"),
                (Some(p), None) => format!(" #{p}"),
                _ => String::new(),
            };
            let mut bits = Vec::new();
            if x["opened"] == true {
                bits.push("opened".to_string());
            }
            if n(&x["rounds"]) > 0 {
                bits.push(s(n(&x["rounds"]), "round"));
            }
            if let Some(o) = x["outcome"].as_str() {
                bits.push(o.to_string());
            }
            md.push_str(&format!(
                "- {} {}{pr}{}\n",
                x["id"].as_str().unwrap_or(""),
                x["title"].as_str().unwrap_or(""),
                if bits.is_empty() { String::new() } else { format!(": {}", bits.join(", ")) }
            ));
        }
    }
    let us = &r["usage"];
    if n(&us["tokens"]) > 0 {
        let d = n(&us["tokens"]) - n(&us["prev_tokens"]);
        let mut line = vec![format!(
            "{} ({}{} vs the period before{})",
            compact(n(&us["tokens"])),
            if d >= 0 { "+" } else { "−" },
            compact(d.abs()),
            us["cached"].as_f64().map(|c| format!(", {:.0}% cached", c * 100.0)).unwrap_or_default()
        )];
        let models = us["models"].as_array().cloned().unwrap_or_default();
        for p in us["providers"].as_array().into_iter().flatten() {
            let mine: Vec<String> =
                models.iter().filter(|m| m["provider"] == p["provider"]).map(|m| format!("{} {}", m["model"].as_str().unwrap_or(""), compact(n(&m["tokens"])))).collect();
            line.push(format!("{} {} ({})", p["name"].as_str().unwrap_or(""), compact(n(&p["tokens"])), mine.join(", ")));
        }
        let kinds: Vec<String> = [("agents", "agents"), ("builds", "Builds"), ("reviews", "reviews")]
            .iter()
            .filter(|(k, _)| n(&us["work"][k]) > 0)
            .map(|(k, w)| format!("{w} {}", compact(n(&us["work"][k]))))
            .collect();
        if !kinds.is_empty() {
            line.push(kinds.join(", "));
        }
        md.push_str(&format!("\n**Tokens:** {}\n", line.join(" · ")));
        md.push_str(&cost_line(us));
        for d in us["dockets"].as_array().into_iter().flatten().take(5) {
            md.push_str(&format!("- {} {}: {}{}\n", d["id"].as_str().unwrap_or(""), d["title"].as_str().unwrap_or(""), compact(n(&d["tokens"])), cost_of(d)));
        }
    }
    let you = &r["you"];
    let ds = you["decisions"].as_array().cloned().unwrap_or_default();
    if !ds.is_empty() || n(&you["approved"]) + n(&you["denied"]) > 0 {
        md.push_str(&format!(
            "\n**You:** {} decision{}, {} approval{}, {} denied\n",
            ds.len(),
            if ds.len() == 1 { "" } else { "s" },
            n(&you["approved"]),
            if n(&you["approved"]) == 1 { "" } else { "s" },
            n(&you["denied"])
        ));
        for d in ds.iter().take(15) {
            md.push_str(&format!("- {} {}: chose “{}”\n", d["docket"].as_str().unwrap_or(""), d["title"].as_str().unwrap_or(""), d["choice"].as_str().unwrap_or("")));
        }
    }
    md
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hub::{Who, test_core};
    use crate::review::pr::Pr;

    #[test]
    fn a_week_of_reviews_and_agent_work() {
        let mut core = test_core();
        let mut p = Pr::new("o/api", 7);
        p.state = "cleaned".into();
        p.outcome = Some("merged".into());
        p.ended_at = Some("2026-11-12T12:00:00Z".into());
        p.history = vec![
            json!({ "round": 1, "verdict": "CHANGES_REQUESTED", "at": "2026-11-10T10:30:00Z", "requested_at": "2026-11-10T10:00:00Z", "tokens": { "total": 100, "cache_read": 90 } }),
            json!({ "round": 2, "verdict": "APPROVED", "at": "2026-11-11T10:10:00Z", "requested_at": "2026-11-11T10:00:00Z", "tokens": { "total": 50, "cache_read": 40 } }),
            json!({ "round": 3, "verdict": "DISMISSED", "by": "you", "at": "2026-11-11T11:00:00Z" }),
        ];
        crate::review::save(&mut core, &mut p).unwrap();
        // Last week's review counts for the comparison only.
        let mut q = Pr::new("o/api", 5);
        q.state = "posted".into();
        q.history = vec![json!({ "round": 1, "verdict": "APPROVED", "at": "2026-11-03T10:00:00Z" })];
        crate::review::save(&mut core, &mut q).unwrap();

        let id = core.open(&Who::Hub, "Fix the flaky test", json!({ "protocol": "agent" })).unwrap();
        core.append(&id, "note", &Who::Owner, Some("fix it".into()), json!({ "via": "prompt" })).unwrap();
        core.append(&id, "turn", &Who::Hub, Some("Fixed.".into()), json!({})).unwrap();
        core.append(&id, "decision", &Who::Owner, None, json!({ "choice": "Merge", "replies_to": 3 })).unwrap();
        core.append(&id, "set", &Who::Hub, None, json!({ "status": "done" })).unwrap();
        // A Build: its PR opened, a round pushed with two replies, merged.
        let b = core.open(&Who::Hub, "Retry the outbox", json!({ "protocol": "build" })).unwrap();
        for (what, data) in
            [("opened", json!({ "pr": 45, "url": "https://github.com/acme/api/pull/45" })), ("pushed-round", json!({ "round": 1, "replies": 2 })), ("merged", json!({ "pr": 45 }))]
        {
            crate::build::event(&mut core, &b, what, String::new(), data).unwrap();
        }

        let now = OffsetDateTime::now_utc();
        let since = (now - time::Duration::days(7)).format(&Rfc3339).unwrap();
        let until = (now + time::Duration::days(1)).format(&Rfc3339).unwrap();
        let r = recap(&mut core, &since, &until, "this week").unwrap();
        // The agent's work is "now"; the fixed-date reviews may fall outside.
        assert_eq!(r["agents"]["started"], 1);
        assert_eq!(r["agents"]["turns"], 1);
        assert_eq!(r["agents"]["prompts"], 1);
        assert_eq!(r["agents"]["done"][0]["id"], id.as_str());
        assert_eq!(r["you"]["decisions"][0]["choice"], "Merge");
        let bs = &r["builds"];
        assert_eq!((bs["opened"].as_i64(), bs["rounds"].as_i64(), bs["replies"].as_i64(), bs["merged"].as_i64()), (Some(1), Some(1), Some(2), Some(1)));
        assert_eq!(bs["list"][0]["outcome"], "merged");
        let md = r["markdown"].as_str().unwrap();
        assert!(
            md.contains(&format!(
                "**Builds:** 1 PR opened, 1 address round (2 replies), 1 merged\n- {b} Retry the outbox [#45](https://github.com/acme/api/pull/45): opened, 1 round, merged\n"
            )),
            "{md}"
        );

        let r = recap(&mut core, "2026-11-10T00:00:00Z", "2026-11-17T00:00:00Z", "Nov 10 to 16").unwrap();
        let rv = &r["reviews"];
        assert_eq!((rv["posted"].as_i64(), rv["approved"].as_i64(), rv["changes"].as_i64(), rv["dismissed"].as_i64()), (Some(2), Some(1), Some(1), Some(1)));
        assert_eq!(rv["prev_posted"], 1);
        assert_eq!(rv["per_day"][0], 1);
        assert_eq!(rv["per_day"][1], 1);
        assert_eq!(rv["median_to_review"], 1200, "30m and 10m");
        assert_eq!(rv["tokens"], 150);
        assert_eq!(rv["prs"]["merged"][0]["verdicts"], json!(["CHANGES_REQUESTED", "APPROVED"]));
        assert_eq!(rv["prs"]["merged"][0]["rounds"], 2);
        let md = r["markdown"].as_str().unwrap();
        assert!(
            md.starts_with("## Recap: Nov 10 to 16\n\n**Reviews:** 2 reviews posted (+1 vs the period before) · 1 approved · 1 changes requested · 1 merged · median 20m"),
            "{md}"
        );
        assert!(md.contains("\n**Merged**\n- api#7: ✕ → ✓, 2 rounds"), "{md}");
    }

    fn call(at: &str, model: &str, provider: &str, tokens: [i64; 4]) -> crate::usage::Call {
        crate::usage::Call { at: OffsetDateTime::parse(at, &Rfc3339).unwrap().unix_timestamp(), model: model.into(), provider: provider.into(), tokens, write_1h: 0 }
    }

    fn names() -> Names {
        [("D-1", "agents"), ("D-2", "reviews"), ("D-3", "builds")].into_iter().map(|(id, k)| (id.to_string(), (format!("{id} title"), k))).collect()
    }

    fn ids() -> Vec<String> {
        vec!["D-1".into(), "D-2".into(), "D-3".into()]
    }

    fn keys() -> Vec<String> {
        vec!["claude:a.jsonl".into(), "codex:b.jsonl".into(), "claude:c.jsonl".into()]
    }

    /// `usage` at the built-in prices.
    fn usage_of(w: &Window, names: &Names, rows: &[crate::usage::Row]) -> Value {
        usage(w, names, rows, &crate::pricing::Prices::built_in())
    }

    /// `attribute` over calls already read, one list per file.
    fn rows_of(spans: &[Span], read: &[Vec<crate::usage::Call>]) -> Vec<crate::usage::Row> {
        let spans: Vec<&Span> = spans.iter().collect();
        attribute(&ids(), &spans, |f| read.get(f).map(Vec::as_slice), |f| keys().get(f).cloned())
    }

    #[test]
    fn tokens_by_provider_model_and_work_over_the_window_only() {
        let w = window("2026-11-10T00:00:00-05:00", "2026-11-17T00:00:00-05:00").unwrap();
        let read = vec![
            vec![call("2026-11-10T06:00:00Z", "Opus 5.5", "anthropic", [10, 80, 10, 100]), call("2026-11-12T06:00:00Z", "Sonnet 5.5", "anthropic", [5, 0, 0, 5])],
            vec![call("2026-11-11T06:00:00Z", "GPT-6.2 Astra", "openai", [50, 30, 0, 20]), call("2026-11-04T06:00:00Z", "GPT-6.2 Astra", "openai", [1000, 0, 0, 0])],
            // Before the window and after it: only the comparison.
            vec![call("2026-11-09T06:00:00Z", "Opus 5.5", "anthropic", [7, 0, 0, 0]), call("2026-11-17T06:00:00Z", "Opus 5.5", "anthropic", [9, 0, 0, 0])],
        ];
        let all = |docket, file| Span { docket, file, from: i64::MIN / 2, until: None };
        // The same file reached from another docket counts once.
        let rows = rows_of(&[all(0, 0), all(1, 0), all(1, 1), all(2, 2)], &read);
        assert_eq!(rows.len(), 6);
        let u = usage_of(&w, &names(), &rows);
        assert_eq!(u["tokens"], 310);
        assert_eq!(u["prev_tokens"], 1007);
        assert_eq!(u["split"], json!({ "input": 65, "cache_read": 110, "cache_write": 10, "output": 125, "cache_write_1h": 0 }));
        assert_eq!(
            u["providers"],
            json!([
                { "provider": "anthropic", "name": "Anthropic", "tokens": 210, "cost": 0.002166, "unpriced": 0 },
                { "provider": "openai", "name": "OpenAI", "tokens": 100, "cost": 0.001212, "unpriced": 0 },
            ])
        );
        // Opus 5.5: 10 in at $4, 80 cached at $0.20, 10 written at $5, 100 out at $20 a million.
        assert_eq!(u["models"][0]["cost"], 0.002106);
        assert_eq!(u["models"][0]["price"], json!({ "input": 4.0, "cache_read": 0.2, "cache_write": 5.0, "output": 20.0, "cache_write_1h": 8.0, "source": "built-in" }));
        assert_eq!((u["cost"].as_f64(), u["prev_cost"].as_f64()), (Some(0.003378), Some(0.008028)));
        assert_eq!(u["cost_per_day"], json!([0.002106, 0.001212, 0.00006, 0.0, 0.0, 0.0, 0.0]));
        assert_eq!(u["work_cost"], json!({ "agents": 0.002166, "builds": 0.0, "reviews": 0.001212 }));
        assert_eq!(u["unpriced"], json!({ "tokens": 0, "prev_tokens": 0, "models": [] }));
        assert_eq!(u["models"][0]["model"], "Opus 5.5");
        assert_eq!(u["models"][0]["tokens"], 200);
        assert_eq!(u["models"][2]["model"], "Sonnet 5.5");
        assert_eq!(u["work"], json!({ "agents": 210, "builds": 0, "reviews": 100 }));
        assert_eq!(u["per_day"], json!([200, 100, 10, 0, 0, 0, 0]), "days in the window's own time zone");
        assert_eq!(u["dockets"][0]["id"], "D-1");
        assert_eq!(u["dockets"][0]["models"], json!(["Opus 5.5", "Sonnet 5.5"]));
        assert_eq!(u["dockets"].as_array().unwrap().len(), 2);

        // A session held by D-1 until Nov 11, then by D-2: each gets its own calls.
        let at = |s: &str| OffsetDateTime::parse(s, &Rfc3339).unwrap().unix_timestamp();
        let spans = [Span { docket: 0, file: 0, from: 0, until: Some(at("2026-11-11T00:00:00Z")) }, Span { docket: 1, file: 0, from: at("2026-11-11T00:00:00Z"), until: None }];
        let v = usage_of(&w, &names(), &rows_of(&spans, &read));
        assert_eq!(
            v["dockets"],
            json!([
                { "id": "D-1", "title": "D-1 title", "kind": "agents", "tokens": 200, "cost": 0.002106, "unpriced": 0, "models": ["Opus 5.5"] },
                { "id": "D-2", "title": "D-2 title", "kind": "reviews", "tokens": 10, "cost": 0.00006, "unpriced": 0, "models": ["Sonnet 5.5"] },
            ])
        );
        assert_eq!(v["tokens"], 210);

        let mut r = json!({ "reviews": {}, "agents": {}, "builds": {}, "you": {}, "usage": u });
        r["markdown"] = json!(markdown(&r, "Nov 10 to 16"));
        let md = r["markdown"].as_str().unwrap();
        assert!(
            md.contains("\n**Tokens:** 310 (−697 vs the period before, 59% cached) · Anthropic 210 (Opus 5.5 200, Sonnet 5.5 10) · OpenAI 100 (GPT-6.2 Astra 100) · agents 210, reviews 100\n**Cost:** <$0.01 at API prices (−<$0.01 vs the period before) · Anthropic <$0.01 (Opus 5.5 <$0.01, Sonnet 5.5 <$0.01) · OpenAI <$0.01 (GPT-6.2 Astra <$0.01) · agents <$0.01, reviews <$0.01\n- D-1 D-1 title: 210, <$0.01\n"),
            "{md}"
        );
    }

    #[test]
    fn cost_at_each_models_price_and_the_tokens_no_price_covers() {
        let w = window("2026-11-24T00:00:00Z", "2026-12-03T00:00:00Z").unwrap();
        let row = |at: &str, docket: &str, provider: &str, model: &str, tokens| crate::usage::Row {
            file: format!("x:{docket}"),
            minute: OffsetDateTime::parse(at, &Rfc3339).unwrap().unix_timestamp(),
            provider: provider.into(),
            model: model.into(),
            docket: docket.into(),
            tokens,
            write_1h: 0,
        };
        let m = 1_000_000;
        let rows = vec![
            // $4 + $0.20 + $5 + $20.
            row("2026-11-24T10:00:00Z", "D-1", "anthropic", "Opus 5.5", [m, m, m, m]),
            // Gemini 4 Carbon doubles on December 1: $10 a million out, then $20.
            row("2026-11-30T23:59:00Z", "D-3", "google", "Gemini 4 Carbon", [0, 0, 0, m]),
            row("2026-12-01T00:00:00Z", "D-3", "google", "Gemini 4 Carbon", [0, 0, 0, m]),
            // No price: its tokens are said, never costed at nothing.
            row("2026-11-25T10:00:00Z", "D-1", "ollama", "Llama 4", [3 * m, 0, 0, m]),
            row("2026-11-25T10:00:00Z", "D-2", "openrouter", "Sonnet 5.5", [m, 0, 0, 0]),
            // The week before: $2 a million in for Sonnet 5.5.
            row("2026-11-20T10:00:00Z", "D-2", "anthropic", "Sonnet 5.5", [m, 0, 0, 0]),
            row("2026-11-20T10:00:00Z", "D-2", "ollama", "Llama 4", [5, 0, 0, 0]),
        ];
        let u = usage_of(&w, &names(), &rows);
        assert_eq!(u["tokens"], 4 * m + 2 * m + 4 * m + m);
        assert_eq!(u["cost"], 59.2);
        assert_eq!((u["prev_cost"].as_f64(), u["unpriced"]["prev_tokens"].as_i64()), (Some(2.0), Some(5)));
        assert_eq!(u["unpriced"]["tokens"], 5 * m);
        assert_eq!(
            u["unpriced"]["models"],
            json!([
                { "model": "Llama 4", "provider": "ollama", "provider_name": "Ollama", "tokens": 4 * m },
                { "model": "Sonnet 5.5", "provider": "openrouter", "provider_name": "OpenRouter", "tokens": m },
            ])
        );
        let llama = u["models"].as_array().unwrap().iter().find(|x| x["model"] == "Llama 4").unwrap();
        assert_eq!((llama["cost"].clone(), llama["price"].clone(), llama["unpriced"].as_i64()), (Value::Null, Value::Null, Some(4 * m)));
        let carbon = u["models"].as_array().unwrap().iter().find(|x| x["model"] == "Gemini 4 Carbon").unwrap();
        assert_eq!(carbon["cost"], 30.0);
        assert_eq!(carbon["price"]["output"], 20.0, "the price last used");
        assert_eq!(u["work_cost"], json!({ "agents": 29.2, "builds": 30.0, "reviews": 0.0 }));
        assert_eq!(u["cost_per_day"][6], 10.0);
        assert_eq!(u["cost_per_day"][7], 20.0);
        let d1 = u["dockets"].as_array().unwrap().iter().find(|x| x["id"] == "D-1").unwrap();
        assert_eq!((d1["cost"].as_f64(), d1["unpriced"].as_i64()), (Some(29.2), Some(4 * m)));
        let d2 = u["dockets"].as_array().unwrap().iter().find(|x| x["id"] == "D-2").unwrap();
        assert_eq!((d2["cost"].as_f64(), d2["unpriced"].as_i64()), (Some(0.0), Some(m)));
        assert_eq!(u["price_notes"], json!([]));

        let mut r = json!({ "reviews": {}, "agents": {}, "builds": {}, "you": {}, "usage": u });
        r["markdown"] = json!(markdown(&r, "Nov 24 to Dec 2"));
        let md = r["markdown"].as_str().unwrap();
        assert!(
            md.contains(
                "\n**Cost:** $59.20 at API prices (+$57.20 vs the period before) · Anthropic $29.20 (Opus 5.5 $29.20) · Google $30.00 (Gemini 4 Carbon $30.00) · agents $29.20, Builds $30.00 · 5M tokens unpriced: Llama 4 (Ollama), Sonnet 5.5 (OpenRouter)\n"
            ),
            "{md}"
        );
        assert!(md.contains("\n- D-1 D-1 title: 8M, $29.20 (4M tokens unpriced)\n"), "{md}");
        assert!(md.contains("\n- D-2 D-2 title: 1M, unpriced\n"), "{md}");

        // A price set in settings.json is used, and says so; one that can't be is said.
        let set = crate::pricing::Prices::with(
            &json!({ "ollama": { "Llama 4": { "input": 0.5, "cache_read": 0, "cache_write": 0, "output": 1 } }, "openrouter": { "Sonnet 5.5": { "input": -2 } } }),
        );
        let v = usage(&w, &names(), &rows, &set);
        assert_eq!(v["cost"], 61.7);
        assert_eq!(v["unpriced"]["tokens"], m);
        let llama = v["models"].as_array().unwrap().iter().find(|x| x["model"] == "Llama 4").unwrap();
        assert_eq!(llama["price"]["source"], "settings");
        assert_eq!(v["price_notes"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn many_small_costs_add_up_exactly() {
        let w = window("2026-11-10T00:00:00Z", "2026-11-11T00:00:00Z").unwrap();
        // 1,440 minutes of one Haiku 5.5 input token each: a tenth of a
        // micro-dollar a minute, 144 micro-dollars in all, none lost to
        // rounding each minute.
        let rows: Vec<crate::usage::Row> = (0..1440)
            .map(|i| crate::usage::Row {
                file: "claude:a.jsonl".into(),
                minute: w.since.unix_timestamp() + i * 60,
                provider: "anthropic".into(),
                model: "Haiku 5.5".into(),
                docket: "D-1".into(),
                tokens: [1, 0, 0, 0],
                write_1h: 0,
            })
            .collect();
        let u = usage_of(&w, &names(), &rows);
        assert_eq!(u["cost"], 0.000144);
        assert_eq!(u["models"][0]["cost"], 0.000144);
        assert_eq!(u["dockets"][0]["cost"], 0.000144);
    }

    #[test]
    fn writes_to_the_1_hour_cache_cost_their_own_price() {
        let at = |s: &str| OffsetDateTime::parse(s, &Rfc3339).unwrap().unix_timestamp();
        let m = 1_000_000;
        // Two calls in a minute: their 1-hour writes add up in the row.
        let mut a = call("2026-11-10T06:00:01Z", "Opus 5.5", "anthropic", [0, 0, 600_000, 0]);
        a.write_1h = 500_000;
        let mut b = call("2026-11-10T06:00:30Z", "Opus 5.5", "anthropic", [0, 0, 400_000, 0]);
        b.write_1h = 400_000;
        let rows = rows_of(&[Span { docket: 0, file: 0, from: 0, until: None }], &[vec![a, b]]);
        assert_eq!((rows.len(), rows[0].tokens[2], rows[0].write_1h, rows[0].minute), (1, m, 900_000, at("2026-11-10T06:00:00Z")));
        let w = window("2026-11-10T00:00:00Z", "2026-11-11T00:00:00Z").unwrap();
        let u = usage_of(&w, &names(), &rows);
        // 100,000 at $5 a million (5 minutes) and 900,000 at $8 (an hour).
        assert_eq!(u["cost"], 7.7);
        assert_eq!(u["split"]["cache_write"], m);
        assert_eq!(u["split"]["cache_write_1h"], 900_000);
        assert_eq!(u["models"][0]["split"]["cache_write_1h"], 900_000);
        assert_eq!(u["models"][0]["price"]["cache_write_1h"], 8.0);
        // Kept and read back from the store: the same.
        let mut core = test_core();
        core.store.put_usage(&rows).unwrap();
        assert_eq!(usage_of(&w, &names(), &core.store.usage_between(0, i64::MAX).unwrap())["cost"], 7.7);
    }

    #[test]
    fn nothing_priced_says_unpriced_not_nothing() {
        let w = window("2026-11-10T00:00:00Z", "2026-11-11T00:00:00Z").unwrap();
        let row = |at: &str, provider: &str, model: &str, n: i64| crate::usage::Row {
            file: "pi:a.jsonl".into(),
            minute: OffsetDateTime::parse(at, &Rfc3339).unwrap().unix_timestamp(),
            provider: provider.into(),
            model: model.into(),
            docket: "D-1".into(),
            tokens: [n, 0, 0, 0],
            write_1h: 0,
        };
        let md = |u: Value| markdown(&json!({ "reviews": {}, "agents": {}, "builds": {}, "you": {}, "usage": u }), "today");
        let u = usage_of(&w, &names(), &[row("2026-11-10T06:00:00Z", "ollama", "Llama 4", 5_000_000)]);
        assert_eq!((u["cost"].as_f64(), u["unpriced"]["tokens"].as_i64()), (Some(0.0), Some(5_000_000)));
        let text = md(u);
        assert!(text.contains("\n**Cost:** unpriced, 5M tokens with no price: Llama 4 (Ollama)\n"), "{text}");
        assert!(!text.contains("$0"), "{text}");
        // The same cost as the period before: ±$0, as the apps say it.
        let u = usage_of(&w, &names(), &[row("2026-11-09T06:00:00Z", "anthropic", "Opus 5.5", 1_000_000), row("2026-11-10T06:00:00Z", "anthropic", "Opus 5.5", 1_000_000)]);
        assert!(md(u).contains("\n**Cost:** $4.00 at API prices (±$0 vs the period before)"));
    }

    #[test]
    fn a_minutes_calls_are_one_row_and_calls_no_span_holds_arent_kept() {
        let at = |s: &str| OffsetDateTime::parse(s, &Rfc3339).unwrap().unix_timestamp();
        // Two models taking turns in one minute (the reader keeps them as
        // separate calls), and a call after the docket let the session go.
        let read = vec![vec![
            call("2026-11-10T06:00:01Z", "Opus 5.5", "anthropic", [1, 0, 0, 1]),
            call("2026-11-10T06:00:20Z", "Haiku 5.5", "anthropic", [2, 0, 0, 2]),
            call("2026-11-10T06:00:40Z", "Opus 5.5", "anthropic", [3, 0, 0, 3]),
            call("2026-11-10T09:00:00Z", "Opus 5.5", "anthropic", [50, 0, 0, 50]),
        ]];
        let rows = rows_of(&[Span { docket: 0, file: 0, from: 0, until: Some(at("2026-11-10T08:00:00Z")) }], &read);
        let minute = at("2026-11-10T06:00:00Z");
        let row = |model: &str, tokens| crate::usage::Row {
            file: "claude:a.jsonl".into(),
            minute,
            provider: "anthropic".into(),
            model: model.into(),
            docket: "D-1".into(),
            tokens,
            write_1h: 0,
        };
        assert_eq!(rows, vec![row("Haiku 5.5", [2, 0, 0, 2]), row("Opus 5.5", [4, 0, 0, 4])]);
    }

    #[test]
    fn kept_tokens_outlast_the_transcript_and_count_once_however_often_kept() {
        let mut core = test_core();
        let w = window("2026-11-10T00:00:00Z", "2026-11-17T00:00:00Z").unwrap();
        let (from, to) = (w.before().since.unix_timestamp(), w.until.unix_timestamp());
        let all = |docket, file| Span { docket, file, from: 0, until: None };
        let spans = [all(0, 0), all(2, 2)];
        let first =
            vec![vec![call("2026-11-10T06:00:00Z", "Opus 5.5", "anthropic", [10, 0, 0, 10])], vec![], vec![call("2026-11-11T06:00:00Z", "Opus 5.5", "anthropic", [1, 0, 0, 1])]];
        let rows = rows_of(&spans, &first);
        // Kept twice: still counted once.
        core.store.put_usage(&rows).unwrap();
        core.store.put_usage(&rows).unwrap();
        assert_eq!(usage_of(&w, &names(), &core.store.usage_between(from, to).unwrap())["tokens"], 22);

        // a.jsonl grew in the same minute, and later: its minutes are replaced, not added to.
        let grown = vec![vec![call("2026-11-10T06:00:00Z", "Opus 5.5", "anthropic", [15, 0, 0, 15]), call("2026-11-12T06:00:00Z", "Opus 5.5", "anthropic", [5, 0, 0, 5])]];
        core.store.put_usage(&rows_of(&spans[..1], &grown)).unwrap();
        assert_eq!(usage_of(&w, &names(), &core.store.usage_between(from, to).unwrap())["tokens"], 42);

        // A model named differently now: the minute is replaced whole, not added to.
        let mut renamed = rows_of(&spans[..1], &grown);
        renamed.iter_mut().for_each(|r| r.model = "Opus 5.5 (1M)".into());
        core.store.put_usage(&renamed).unwrap();
        assert_eq!(usage_of(&w, &names(), &core.store.usage_between(from, to).unwrap())["tokens"], 42);

        // Then c.jsonl is pruned: nothing reads it, and its tokens still count.
        let kept = core.store.usage_between(from, to).unwrap();
        let u = usage_of(&w, &names(), &merge(kept, rows_of(&spans[..1], &grown)));
        assert_eq!(u["tokens"], 42);
        assert_eq!(u["work"], json!({ "agents": 40, "builds": 2, "reviews": 0 }));

        // A docket the period's work doesn't name keeps its own title.
        let mut few = names();
        few.remove("D-3");
        assert_eq!(
            usage_of(&w, &few, &core.store.usage_between(from, to).unwrap())["dockets"][1],
            json!({ "id": "D-3", "title": "D-3", "kind": "agents", "tokens": 2, "cost": 0.000024, "unpriced": 0, "models": ["Opus 5.5"] })
        );
    }

    #[test]
    fn recap_reads_over_what_is_kept_but_never_below_it() {
        let row = |file: &str, docket: &str, n: i64| crate::usage::Row {
            file: file.into(),
            minute: 60,
            provider: "anthropic".into(),
            model: "Opus 5.5".into(),
            docket: docket.into(),
            tokens: [n, 0, 0, 0],
            write_1h: 0,
        };
        // The file grew since the snapshot: what's read now wins.
        assert_eq!(merge(vec![row("a", "D-1", 5)], vec![row("a", "D-1", 9)]), vec![row("a", "D-1", 9)]);
        // The snapshot counted a call this period's work doesn't reach: it stays.
        assert_eq!(merge(vec![row("a", "D-2", 9)], vec![row("a", "D-1", 5)]), vec![row("a", "D-2", 9)]);
        // Only kept (a pruned transcript), or only read: each counts once.
        assert_eq!(merge(vec![row("a", "D-1", 5)], vec![row("b", "D-1", 7)]), vec![row("a", "D-1", 5), row("b", "D-1", 7)]);
        // A model named differently since: the same minute, counted once.
        let mut renamed = row("a", "D-1", 5);
        renamed.model = "Opus 5.5 (1M)".into();
        assert_eq!(merge(vec![row("a", "D-1", 5)], vec![renamed.clone()]), vec![renamed]);
    }

    #[test]
    fn reading_in_parts_counts_what_reading_at_once_does() {
        let dir = std::env::temp_dir().join(format!("wh-recap-parts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let line = |id: &str, at: &str, n: i64| {
            json!({ "type": "assistant", "timestamp": at, "message": { "id": id, "model": "claude-opus-5-5", "usage": { "input_tokens": n, "output_tokens": 1 } } }).to_string()
                + "\n"
        };
        let mut files = Vec::new();
        for i in 0..5 {
            let p = dir.join(format!("s{i}.jsonl"));
            std::fs::write(&p, line(&format!("m{i}"), "2026-11-10T06:00:00Z", i + 1) + &line(&format!("n{i}"), "2026-11-10T07:00:00Z", 10)).unwrap();
            files.push(("claude".to_string(), p.clone(), crate::usage::file_key("claude", &p)));
        }
        // A sixth that can't be read: a folder where the transcript should be.
        let bad = dir.join("s5.jsonl");
        std::fs::create_dir_all(&bad).unwrap();
        files.push(("claude".to_string(), bad, "claude:s5.jsonl".into()));
        // Two dockets reach file 2: the first holds it.
        let mut spans: Vec<Span> = (0..6).map(|f| Span { docket: f % 2, file: f, from: 0, until: None }).collect();
        spans.push(Span { docket: 2, file: 2, from: 0, until: None });
        let p = Plan { ids: ids(), files, spans };
        let whole = read_part(&p, 0, 6, 0);
        let mut parts = Vec::new();
        let mut failed = 0;
        for lo in (0..6).step_by(4) {
            let r = read_part(&p, lo, lo + 4, 0);
            failed += r.failed;
            parts.extend(r.rows);
        }
        assert_eq!(whole.rows, parts);
        assert_eq!((whole.failed, failed), (1, 1));
        assert!(whole.failed_written.is_some());
        assert_eq!(whole.rows.len(), 10);
        assert_eq!(whole.rows.iter().filter(|r| r.docket == "D-3").count(), 0);
        assert_eq!(read_part(&p, 7, 9, 0).rows, vec![], "past the end: nothing");
    }

    #[test]
    fn a_transcript_that_couldnt_be_read_holds_the_mark_back_until_it_is() {
        let day = 86400;
        assert_eq!(mark_after(100 * day, 97 * day, None), 100 * day);
        // Written on day 98 and not read: the next read starts by day 98.
        let mark = mark_after(100 * day, 97 * day, Some(98 * day));
        assert_eq!(mark, 100 * day);
        assert!(read_from(0, Some(mark)) <= 98 * day);
        let mark = mark_after(104 * day, 97 * day, Some(98 * day));
        assert_eq!(read_from(0, Some(mark)), 98 * day);
        // Never behind the last mark (day 99), even for an older file.
        assert_eq!(mark_after(102 * day, 97 * day, Some(96 * day)), 99 * day);
        // The first snapshot (from 0) that fails holds at the file's day.
        assert_eq!(read_from(0, Some(mark_after(105 * day, 0, Some(100 * day)))), 100 * day);
        // One that's failed for a week no longer holds anything back.
        assert_eq!(mark_after(110 * day, 97 * day, Some(98 * day)), 110 * day);
    }

    #[test]
    fn a_batch_never_splits_a_minute_it_replaces() {
        let shared = crate::hub::Shared::new(test_core());
        let row = |minute: i64, model: &str| crate::usage::Row {
            file: "claude:a.jsonl".into(),
            minute,
            provider: "anthropic".into(),
            model: model.into(),
            docket: "D-1".into(),
            tokens: [1, 0, 0, 0],
            write_1h: 0,
        };
        // 1,999 minutes, then one with four models across where a batch would end.
        let mut rows: Vec<_> = (0..1999).map(|m| row(m * 60, "Opus 5.5")).collect();
        rows.extend(["Opus 5.5", "Haiku 5.5", "Sonnet 5.5", "Fable 5.2"].map(|m| row(1999 * 60, m)));
        keep(&shared, &rows).unwrap();
        assert_eq!(shared.lock().store.usage_between(1999 * 60, 2000 * 60).unwrap().len(), 4);
        assert_eq!(shared.lock().store.usage_between(0, i64::MAX).unwrap().len(), 2003);
    }

    #[test]
    fn transcripts_a_snapshot_kept_arent_read_again() {
        let day = 86400;
        assert_eq!(read_from(100 * day, None), 100 * day, "no snapshot yet: the whole period");
        assert_eq!(read_from(100 * day, Some(130 * day)), 128 * day, "kept up to two days before the snapshot");
        assert_eq!(read_from(100 * day, Some(101 * day)), 100 * day, "never before the period");
    }

    #[test]
    fn a_snapshot_marks_how_far_it_kept_and_a_mark_from_the_future_isnt_used() {
        let shared = crate::hub::Shared::new(test_core());
        // No dockets with agents: nothing to read, and still marked.
        assert_eq!(snapshot(&shared, 0, 1_800_000_000).unwrap(), (0, 0));
        let core = shared.lock();
        assert_eq!(core.store.meta_get(SNAPSHOT).unwrap().as_deref(), Some("1800000000"));
        assert_eq!(core.store.meta_get(SNAPSHOT_RAN).unwrap().as_deref(), Some("1800000000"));
        let (_, from, _) = bounds(&core, "2027-01-10T00:00:00Z", "2027-01-17T00:00:00Z", 1_800_000_100).unwrap();
        assert_eq!(from, 1_800_000_000 - SNAPSHOT_SLACK);
        // The clock went back a day: the mark is ignored, and the whole period read.
        let (prev, from, _) = bounds(&core, "2027-01-10T00:00:00Z", "2027-01-17T00:00:00Z", 1_800_000_000 - 86400).unwrap();
        assert_eq!(from, prev);
    }
}
