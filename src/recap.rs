//! Recap: what happened over a period, for a standup or a week's note.
//! Reviews posted and their verdicts, the PRs that merged, still wait or
//! closed; the agents' work; what you decided; and the tokens all of it
//! took, by provider and model. The app sends the period's bounds in its
//! own time zone.

use anyhow::Result;
use serde_json::{Value, json};
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
    title: String,
    /// `agents`, `builds` or `reviews`.
    kind: &'static str,
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

/// The recap, all at once (the usage reads files: see `gather`).
#[cfg(test)]
pub fn recap(core: &Core, since: &str, until: &str, label: &str) -> Result<Value> {
    let (out, work) = gather(core, since, until)?;
    Ok(finish(out, &work, label))
}

fn window(since: &str, until: &str) -> Result<Window> {
    Ok(Window { since: at(Some(since)).ok_or_else(|| anyhow::anyhow!("since isn't a time"))?, until: at(Some(until)).ok_or_else(|| anyhow::anyhow!("until isn't a time"))? })
}

/// Everything in the recap but its usage and Markdown, from the hub (under
/// its lock), and the dockets whose agents' tokens count. `finish` reads
/// their transcripts, without the lock.
pub fn gather(core: &Core, since: &str, until: &str) -> Result<(Value, Vec<Work>)> {
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
    for st in core.store.states()? {
        let updated = at(Some(&st.updated_at));
        if updated.is_some_and(|u| u < prev.since) {
            continue;
        }
        let stored = core.store.entries(&st.id)?;
        let kind = match st.protocol.as_str() {
            "review" => "reviews",
            "build" => "builds",
            _ if core.store.build_task(&st.id)?.is_some() => "builds",
            _ => "agents",
        };
        let claims = crate::subagents::claims(&stored.iter().filter_map(|s| s.entry().ok()).filter(|e| e.kind == "claim" || e.kind == "release").collect::<Vec<_>>());
        if !claims.is_empty() {
            work.push(Work { id: st.id.clone(), title: st.title.clone(), kind, claims });
        }
        if updated.is_some_and(|u| u < w.since) {
            continue;
        }
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
    Ok((out, work))
}

/// The recap with its usage and Markdown: reads the agents' transcripts.
pub fn finish(mut out: Value, work: &[Work], label: &str) -> Value {
    if let (Some(s), Some(u)) = (out["since"].as_str(), out["until"].as_str())
        && let Ok(w) = window(s, u)
    {
        let mut finder = crate::subagents::Finder::new();
        let mut files: Vec<(usize, crate::subagents::Transcript)> =
            work.iter().enumerate().flat_map(|(i, d)| finder.transcripts(&d.claims).into_iter().map(move |t| (i, t))).collect();
        // A session a claim names is that docket's before one found by its
        // folder (a claim left open can't take another docket's).
        files.sort_by_key(|(_, t)| !t.named);
        // Each file is read once, however many dockets reach it.
        let mut paths: Vec<(String, std::path::PathBuf)> = Vec::new();
        let mut index: std::collections::HashMap<std::path::PathBuf, usize> = std::collections::HashMap::new();
        let spans: Vec<Span> = files
            .into_iter()
            .map(|(i, t)| {
                let k = *index.entry(t.path.clone()).or_insert_with(|| {
                    paths.push((t.app.clone(), t.path.clone()));
                    paths.len() - 1
                });
                Span { docket: i, file: k, from: t.from, until: t.until }
            })
            .collect();
        let from = w.before().since.unix_timestamp();
        // The first read of a period's transcripts can be gigabytes: a few
        // threads share it.
        let next = std::sync::atomic::AtomicUsize::new(0);
        let read: Vec<Vec<crate::usage::Call>> = std::thread::scope(|sc| {
            let jobs: Vec<_> = (0..8)
                .map(|_| {
                    sc.spawn(|| {
                        let mut mine = Vec::new();
                        loop {
                            let k = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            let Some((app, p)) = paths.get(k) else { break };
                            // A file that can't be read is left out, not the rest.
                            let calls = std::panic::catch_unwind(|| crate::usage::calls(app, p, from)).unwrap_or_default();
                            mine.push((k, calls));
                        }
                        mine
                    })
                })
                .collect();
            let mut all = vec![Vec::new(); paths.len()];
            for (k, c) in jobs.into_iter().flat_map(|j| j.join().unwrap_or_default()) {
                all[k] = c;
            }
            all
        });
        crate::usage::forget_stale();
        out["usage"] = usage(&w, work, &spans, &read);
    }
    out["markdown"] = json!(markdown(&out, label));
    out
}

/// A docket's hold on a file (an index into what was read): its calls
/// from `from` to `until` are that docket's.
struct Span {
    docket: usize,
    file: usize,
    from: i64,
    until: Option<i64>,
}

/// Tokens over the window, by provider, model, kind of work, day and
/// docket; the window before's total for comparison. Each call counts once,
/// for the first span that holds it.
fn usage(w: &Window, work: &[Work], spans: &[Span], read: &[Vec<crate::usage::Call>]) -> Value {
    use std::collections::{BTreeMap, HashSet};
    let prev = w.before();
    let mut seen: HashSet<(usize, usize)> = HashSet::new();
    let (mut total, mut prev_total) = (0i64, 0i64);
    let mut split = [0i64; 4];
    let mut per_day = vec![0i64; w.days()];
    // (provider, model) → (fields, dockets)
    let mut models: BTreeMap<(String, String), ([i64; 4], HashSet<usize>)> = BTreeMap::new();
    let mut kinds: BTreeMap<&str, i64> = BTreeMap::new();
    let mut dockets: BTreeMap<usize, (i64, BTreeMap<String, i64>)> = BTreeMap::new();
    for sp in spans {
        let i = sp.docket;
        for (j, c) in read.get(sp.file).into_iter().flatten().enumerate() {
            if c.at < sp.from || sp.until.is_some_and(|u| c.at > u) || !seen.insert((sp.file, j)) {
                continue;
            }
            let c = c.clone();
            let n = c.total();
            if prev.has_secs(c.at) {
                prev_total += n;
            }
            if !w.has_secs(c.at) {
                continue;
            }
            total += n;
            let m = models.entry((c.provider.clone(), c.model.clone())).or_default();
            for (k, t) in c.tokens.iter().enumerate() {
                split[k] += t;
                m.0[k] += t;
            }
            m.1.insert(i);
            if let Some(d) = OffsetDateTime::from_unix_timestamp(c.at).ok().and_then(|t| per_day.get_mut(w.day(t))) {
                *d += n;
            }
            *kinds.entry(work[i].kind).or_default() += n;
            let d = dockets.entry(i).or_default();
            d.0 += n;
            *d.1.entry(c.model).or_default() += n;
        }
    }
    let fields = |t: [i64; 4]| json!({ "input": t[0], "cache_read": t[1], "cache_write": t[2], "output": t[3] });
    let mut by_provider: BTreeMap<String, i64> = BTreeMap::new();
    let mut model_list: Vec<Value> = models
        .into_iter()
        .map(|((provider, model), (t, ds))| {
            let n: i64 = t.iter().sum();
            *by_provider.entry(provider.clone()).or_default() += n;
            json!({ "model": model, "provider": provider, "provider_name": crate::usage::provider_name(&provider), "tokens": n, "split": fields(t), "dockets": ds.len() })
        })
        .collect();
    model_list.sort_by_key(|m| std::cmp::Reverse(m["tokens"].as_i64().unwrap_or(0)));
    let mut providers: Vec<Value> = by_provider.into_iter().map(|(p, n)| json!({ "provider": p, "name": crate::usage::provider_name(&p), "tokens": n })).collect();
    providers.sort_by_key(|p| std::cmp::Reverse(p["tokens"].as_i64().unwrap_or(0)));
    let mut top: Vec<(i64, usize, BTreeMap<String, i64>)> = dockets.into_iter().map(|(i, (n, m))| (n, i, m)).collect();
    top.sort_by_key(|(n, i, _)| (std::cmp::Reverse(*n), *i));
    let top: Vec<Value> = top
        .into_iter()
        .take(10)
        .map(|(n, i, m)| {
            let mut m: Vec<(String, i64)> = m.into_iter().collect();
            m.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
            json!({ "id": work[i].id, "title": work[i].title, "kind": work[i].kind, "tokens": n, "models": m.into_iter().map(|(name, _)| name).collect::<Vec<_>>() })
        })
        .collect();
    let input_side = split[0] + split[1] + split[2];
    json!({
        "tokens": total, "prev_tokens": prev_total, "split": fields(split),
        "cached": if input_side > 0 { Some(split[1] as f64 / input_side as f64) } else { None },
        "per_day": per_day, "providers": providers, "models": model_list,
        "work": { "agents": kinds.get("agents").copied().unwrap_or(0), "builds": kinds.get("builds").copied().unwrap_or(0), "reviews": kinds.get("reviews").copied().unwrap_or(0) },
        "dockets": top,
    })
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
        for d in us["dockets"].as_array().into_iter().flatten().take(5) {
            md.push_str(&format!("- {} {}: {}\n", d["id"].as_str().unwrap_or(""), d["title"].as_str().unwrap_or(""), compact(n(&d["tokens"]))));
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
        let r = recap(&core, &since, &until, "this week").unwrap();
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

        let r = recap(&core, "2026-11-10T00:00:00Z", "2026-11-17T00:00:00Z", "Nov 10 to 16").unwrap();
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
        crate::usage::Call { at: OffsetDateTime::parse(at, &Rfc3339).unwrap().unix_timestamp(), model: model.into(), provider: provider.into(), tokens }
    }

    #[test]
    fn tokens_by_provider_model_and_work_over_the_window_only() {
        let work = |id: &str, kind| Work { id: id.into(), title: format!("{id} title"), kind, claims: Vec::new() };
        let work = vec![work("D-1", "agents"), work("D-2", "reviews"), work("D-3", "builds")];
        let w = window("2026-11-10T00:00:00-05:00", "2026-11-17T00:00:00-05:00").unwrap();
        let read = vec![
            vec![call("2026-11-10T06:00:00Z", "Opus 5.5", "anthropic", [10, 80, 10, 100]), call("2026-11-12T06:00:00Z", "Sonnet 5.5", "anthropic", [5, 0, 0, 5])],
            vec![call("2026-11-11T06:00:00Z", "GPT-6.2 Astra", "openai", [50, 30, 0, 20]), call("2026-11-04T06:00:00Z", "GPT-6.2 Astra", "openai", [1000, 0, 0, 0])],
            // Before the window and after it: only the comparison.
            vec![call("2026-11-09T06:00:00Z", "Opus 5.5", "anthropic", [7, 0, 0, 0]), call("2026-11-17T06:00:00Z", "Opus 5.5", "anthropic", [9, 0, 0, 0])],
        ];
        let all = |docket, file| Span { docket, file, from: i64::MIN / 2, until: None };
        // The same file reached from another docket counts once.
        let spans = vec![all(0, 0), all(1, 0), all(1, 1), all(2, 2)];
        let u = usage(&w, &work, &spans, &read);
        assert_eq!(u["tokens"], 310);
        assert_eq!(u["prev_tokens"], 1007);
        assert_eq!(u["split"], json!({ "input": 65, "cache_read": 110, "cache_write": 10, "output": 125 }));
        assert_eq!(u["providers"], json!([{ "provider": "anthropic", "name": "Anthropic", "tokens": 210 }, { "provider": "openai", "name": "OpenAI", "tokens": 100 }]));
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
        let spans = vec![Span { docket: 0, file: 0, from: 0, until: Some(at("2026-11-11T00:00:00Z")) }, Span { docket: 1, file: 0, from: at("2026-11-11T00:00:00Z"), until: None }];
        let v = usage(&w, &work, &spans, &read);
        assert_eq!(
            v["dockets"],
            json!([
                { "id": "D-1", "title": "D-1 title", "kind": "agents", "tokens": 200, "models": ["Opus 5.5"] },
                { "id": "D-2", "title": "D-2 title", "kind": "reviews", "tokens": 10, "models": ["Sonnet 5.5"] },
            ])
        );
        assert_eq!(v["tokens"], 210);

        let mut r = json!({ "reviews": {}, "agents": {}, "builds": {}, "you": {}, "usage": u });
        r["markdown"] = json!(markdown(&r, "Nov 10 to 16"));
        let md = r["markdown"].as_str().unwrap();
        assert!(
            md.contains("\n**Tokens:** 310 (−697 vs the period before, 59% cached) · Anthropic 210 (Opus 5.5 200, Sonnet 5.5 10) · OpenAI 100 (GPT-6.2 Astra 100) · agents 210, reviews 100\n- D-1 D-1 title: 210\n"),
            "{md}"
        );
    }
}
