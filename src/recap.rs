//! Recap: what happened over a period, for a standup or a week's note.
//! Reviews posted and their verdicts, the PRs that merged, still wait or
//! closed; the agents' work; and what you decided. The app sends the
//! period's bounds in its own time zone.

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
        Window { since: self.since - len, until: self.since }
    }

    fn days(&self) -> usize {
        ((self.until - self.since).whole_hours() as f64 / 24.0).ceil().max(1.0) as usize
    }

    fn day(&self, t: OffsetDateTime) -> usize {
        ((t - self.since).whole_hours() / 24).max(0) as usize
    }
}

fn median(mut v: Vec<i64>) -> Option<i64> {
    if v.is_empty() {
        return None;
    }
    v.sort();
    let n = v.len();
    Some(if n % 2 == 1 { v[n / 2] } else { (v[n / 2 - 1] + v[n / 2]) / 2 })
}

pub fn recap(core: &Core, since: &str, until: &str, label: &str) -> Result<Value> {
    let w = Window { since: at(Some(since)).ok_or_else(|| anyhow::anyhow!("since isn't a time"))?, until: at(Some(until)).ok_or_else(|| anyhow::anyhow!("until isn't a time"))? };
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
    for st in core.store.states()? {
        if at(Some(&st.updated_at)).is_some_and(|u| u < w.since) {
            continue;
        }
        let mut d_turns = 0;
        let mut finished = None;
        for s in core.store.entries(&st.id)? {
            let Ok(e) = s.entry() else { continue };
            let t = at(Some(&e.at));
            if !w.has(t) {
                continue;
            }
            let you = e.actor.typ == "person";
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
                    decisions.push(json!({ "docket": st.id, "title": st.title, "choice": e.data["choice"], "subject": e.data["subject"], "message": e.data["replies_to"], "at": e.at }));
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
        if let Some(when) = finished
            && st.status == "done"
            && st.protocol != "review"
        {
            done.push(json!({ "id": st.id, "title": st.title, "protocol": st.protocol, "app": st.agent.as_ref().map(|a| a.app.clone()), "turns": d_turns, "at": when, "summary": st.last_turn.as_deref().and_then(crate::agents::summarize) }));
        }
    }
    done.sort_by(|a, b| b["at"].as_str().cmp(&a["at"].as_str()));
    decisions.sort_by(|a, b| b["at"].as_str().cmp(&a["at"].as_str()));

    let mut out = json!({
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
        "you": { "decisions": decisions, "approved": ok, "denied": denied, "asked": asked, "answered": answered, "intents": intents },
    });
    out["markdown"] = json!(markdown(&out, label));
    Ok(out)
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
    let mut line = vec![format!(
        "{} review{} posted ({}{diff} vs the period before)",
        n(&rv["posted"]),
        if n(&rv["posted"]) == 1 { "" } else { "s" },
        if diff >= 0 { "+" } else { "" }
    )];
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
    let you = &r["you"];
    let ds = you["decisions"].as_array().cloned().unwrap_or_default();
    if !ds.is_empty() || n(&you["approved"]) + n(&you["denied"]) > 0 {
        md.push_str(&format!("\n**You:** {} decision{}, {} approval{}, {} denied\n", ds.len(), if ds.len() == 1 { "" } else { "s" }, n(&you["approved"]), if n(&you["approved"]) == 1 { "" } else { "s" }, n(&you["denied"])));
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
        assert!(md.starts_with("## Recap: Nov 10 to 16\n\n**Reviews:** 2 reviews posted (+1 vs the period before) · 1 approved · 1 changes requested · 1 merged · median 20m"), "{md}");
        assert!(md.contains("\n**Merged**\n- api#7: ✕ → ✓, 2 rounds"), "{md}");
    }
}
