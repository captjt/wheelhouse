//! Recap (Recap.swift): what happened today, this week, or in 30 days, in
//! your time zone. Reviews and their verdicts, PRs merged, agents' turns,
//! Builds and your decisions; and the same as Markdown, for a standup.

use chrono::{Duration, Local, TimeZone, Utc};
use rune::{El, ScrollSpec, Shape, Tx, canvas, col, el, icon, row, spacer, text};
use rune_kit::ScrollBar;
use serde_json::Value;

use super::parts::{self, Req};
use super::{Main, Place, glyphs, widgets};
use crate::icons;
use crate::model::{self, arr, int, plural, str_or};
use crate::theme::{self, K, font, mono};

/// The period Recap covers, and what it has.
pub struct Recap {
    /// `today`, `week` or `30d`.
    pub period: &'static str,
    pub data: Option<Value>,
    pub error: Option<String>,
    pub loading: bool,
    pub loaded_at: f64,
    pub loaded_version: u64,
}

impl Default for Recap {
    fn default() -> Recap {
        Recap { period: "week", data: None, error: None, loading: false, loaded_at: f64::NEG_INFINITY, loaded_version: 0 }
    }
}

const PERIODS: [(&str, &str); 3] = [("today", "Today"), ("week", "This week"), ("30d", "30 days")];

/// The period in words, and the one before it.
fn words(period: &str) -> (&'static str, &'static str) {
    match period {
        "today" => ("today", "yesterday"),
        "30d" => ("the last 30 days", "the 30 days before"),
        _ => ("this week", "the week before"),
    }
}

/// Fetch the recap for the period: from the start of the first day, in
/// your time zone, to the end of today.
pub fn load(s: &mut Main) {
    let r = &mut s.recap;
    if r.loading {
        return;
    }
    r.loading = true;
    let today = Local::now().date_naive();
    let back = match r.period {
        "week" => 6,
        "30d" => 29,
        _ => 0,
    };
    let start = |d: chrono::NaiveDate| Local.from_local_datetime(&d.and_hms_opt(0, 0, 0).unwrap_or_default()).earliest().map(|t| t.with_timezone(&Utc));
    let (Some(since), Some(until)) = (start(today - Duration::days(back)), start(today + Duration::days(1))) else {
        r.loading = false;
        return;
    };
    let iso = |t: chrono::DateTime<Utc>| t.format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let path = format!("/api/recap?since={}&until={}&label={}", crate::hub::encode(&iso(since)), crate::hub::encode(&iso(until)), crate::hub::encode(words(r.period).0));
    let period = r.period;
    parts::call(s, Req::Get(path), move |s, res| {
        let r = &mut s.recap;
        r.loading = false;
        if r.period != period {
            return;
        }
        match res {
            Ok(v) => {
                r.data = Some(v);
                r.error = None;
                r.loaded_at = s.clock;
                r.loaded_version = s.version;
            }
            Err(e) => r.error = Some(e),
        }
    });
}

/// Before each frame: the first load, and a fresh one when the hub has
/// moved on and it's been 15 seconds.
pub fn poll(s: &mut Main) -> bool {
    if s.place != Place::Recap || s.recap.loading {
        return false;
    }
    let r = &s.recap;
    if (r.data.is_none() && r.error.is_none()) || (s.version != r.loaded_version && s.clock - r.loaded_at > 15.0) {
        load(s);
        return true;
    }
    false
}

fn pick(s: &mut Main, i: usize) {
    let p = PERIODS[i.min(2)].0;
    if s.recap.period != p {
        s.recap.period = p;
        s.recap.loading = false;
        load(s);
    }
}

/// The toolbar's Period picker and Copy as Markdown.
pub fn toolbar(s: &Main, k: &K) -> El {
    let segs: Vec<(&str, bool)> = PERIODS.iter().map(|(p, l)| (*l, s.recap.period == *p)).collect();
    let ready = s.recap.data.as_ref().is_some_and(|d| model::text(&d["markdown"]).is_some());
    row()
        .gap(6.0)
        .center_y()
        .child(widgets::segmented("recap-period", &segs, pick, k))
        .child(
            row()
                .key("recap-copy")
                .size(28.0, 26.0)
                .radius(6.0)
                .center()
                .hover(k.hover)
                .role("button")
                .label("Copy as Markdown")
                .help("Copy the recap as a Markdown note, for a standup")
                .focusable(ready)
                .opacity(if ready { 1.0 } else { 0.4 })
                .on_press_tx(move |s: &mut Main, tx: &mut Tx| copy(s, tx))
                .child(icon(&glyphs::CLIPBOARD, 16.0).color(k.muted)),
        )
}

fn copy(s: &mut Main, tx: &mut Tx) {
    let Some(md) = s.recap.data.as_ref().and_then(|d| model::text(&d["markdown"])) else { return };
    tx.copy(md);
    let now = words(s.recap.period).0;
    s.toast(format!("Copied the recap for {now} as Markdown"), false);
}

pub fn view(s: &Main, k: &K) -> El {
    let r = &s.recap;
    let body = match &r.data {
        Some(d) => content(s, d, k).w_full().max_w(940.0).padding(28.0),
        None => text(r.error.as_ref().map(|e| format!("Couldn't make the recap: {e}")).unwrap_or_default()).font(font(13.0, 400.0).lh(17.0)).color(k.muted).padding(40.0),
    };
    col().key("recap").w_full().h_full().items_center().child(body).scroll(ScrollSpec { stick_bottom: false }).scroll_bar(&k.kit())
}

fn pct(a: i64, b: i64) -> String {
    if b == 0 { "–".into() } else { format!("{}%", ((a as f64 / b as f64) * 100.0).round() as i64) }
}

/// How long from request to review: 40m, 3h 5m, 2d 4h.
fn dur(s: f64) -> String {
    if s < 3600.0 {
        format!("{}m", ((s / 60.0).round() as i64).max(1))
    } else if s < 86400.0 {
        format!("{}h {}m", (s / 3600.0) as i64, ((s % 3600.0) / 60.0).round() as i64)
    } else {
        format!("{}d {}h", (s / 86400.0) as i64, ((s % 86400.0) / 3600.0).round() as i64)
    }
}

/// A big count, short: 11.4M, 12k.
fn compact(n: i64) -> String {
    let f = n as f64;
    if n >= 1_000_000_000 {
        format!("{:.1}B", f / 1e9)
    } else if n >= 10_000_000 {
        format!("{:.0}M", f / 1e6)
    } else if n >= 1_000_000 {
        format!("{:.1}M", f / 1e6)
    } else if n >= 10000 {
        format!("{}k", n / 1000)
    } else {
        // 1,234, as `formatted()` writes it.
        let s = n.to_string();
        let mut out = String::new();
        for (i, c) in s.chars().enumerate() {
            if i > 0 && (s.len() - i) % 3 == 0 {
                out.push(',');
            }
            out.push(c);
        }
        out
    }
}

fn ints(v: &Value) -> Vec<i64> {
    arr(v).iter().filter_map(Value::as_i64).collect()
}

fn content(s: &Main, d: &Value, k: &K) -> El {
    let (now, before) = words(s.recap.period);
    let (rv, ag, you) = (&d["reviews"], &d["agents"], &d["you"]);
    let n = |v: &Value| int(v).unwrap_or(0);
    let posted = n(&rv["posted"]);
    let done = arr(&ag["done"]);

    // The numbers: a tile each, as many to a row as fit.
    let tiles = row()
        .key("recap-tiles")
        .w_full()
        .flex_wrap()
        .gap(12.0)
        .child(tile("Reviews posted", posted, None, Some((posted, n(&rv["prev_posted"]), before)), ints(&rv["per_day"]), k))
        .child(tile("Approved", n(&rv["approved"]), Some(format!("{} of reviews", pct(n(&rv["approved"]), posted))), None, vec![], k))
        .child(tile("Changes requested", n(&rv["changes"]), Some(format!("{} of reviews", pct(n(&rv["changes"]), posted))), None, vec![], k))
        .child(tile("PRs merged", n(&rv["merged"]), None, Some((n(&rv["merged"]), n(&rv["prev_merged"]), before)), vec![], k))
        .child(tile(
            "Agent turns",
            n(&ag["turns"]),
            Some(format!("{} done · {}", plural(done.len(), "docket"), plural(n(&ag["prompts"]) as usize, "prompt"))),
            None,
            ints(&ag["per_day"]),
            k,
        ));
    let mut extras = vec![];
    if let Some(m) = rv["median_to_review"].as_f64() {
        extras.push(format!("Median time from request to review: {}", dur(m)));
    }
    if n(&rv["tokens"]) > 0 {
        extras.push(format!("{} review tokens{}", compact(n(&rv["tokens"])), rv["cached"].as_f64().map(|c| format!(", {}% cached", (c * 100.0) as i64)).unwrap_or_default()));
    }
    if n(&rv["dismissed"]) > 0 {
        extras.push(format!("{} dismissed", n(&rv["dismissed"])));
    }

    let mut c = col().w_full().gap(28.0).child(tiles);
    if !extras.is_empty() {
        // Under the tiles, as SwiftUI's VStack spacing puts it.
        c = c.child(text(extras.join(" · ")).font(font(12.5, 400.0).lh(16.0)).color(k.muted).wrap());
    }

    // Reviews: merged, waiting on their authors, closed.
    let prs = &rv["prs"];
    let (merged, open, closed) = (arr(&prs["merged"]), arr(&prs["open"]), arr(&prs["closed"]));
    let mut reviews = section("Reviews", k);
    if merged.is_empty() && open.is_empty() && closed.is_empty() {
        reviews = reviews.child(text(format!("No reviews {now}.")).font(font(13.0, 400.0).lh(17.0)).color(k.muted));
    } else {
        reviews = pr_group(reviews, "Merged", merged, "merged", k);
        reviews = pr_group(reviews, "Waiting on their authors", open, "open", k);
        reviews = pr_group(reviews, "Closed without merging", closed, "closed", k);
    }
    c = c.child(reviews);

    // Agents: what they started, finished and did.
    let mut agents = section("Agents", k).child(
        text(format!(
            "{} started · {} done · {} turns · {} from you",
            plural(n(&ag["started"]) as usize, "docket"),
            done.len(),
            n(&ag["turns"]),
            plural(n(&ag["prompts"]) as usize, "prompt")
        ))
        .font(font(12.5, 400.0).lh(16.0))
        .color(k.muted),
    );
    for (i, x) in done.iter().enumerate() {
        let id = model::text(&x["id"]);
        agents = agents.child(recap_row(
            format!("recap-agent-{i}"),
            theme::app_avatar(x["app"].as_str(), 16.0, k),
            str_or(&x["id"], ""),
            str_or(&x["title"], ""),
            format!("{} {now}", plural(n(&x["turns"]) as usize, "turn")),
            model::text(&x["summary"]),
            x["at"].as_str(),
            Open::Docket(id),
            k,
        ));
    }
    c = c.child(agents);

    // Builds, when there were any.
    let bs = &d["builds"];
    let list = arr(&bs["list"]);
    if !list.is_empty() {
        let replies = n(&bs["replies"]);
        let mut builds = section("Builds", k).child(
            text(format!(
                "{} opened · {} ({replies} {}) · {} merged",
                plural(n(&bs["opened"]) as usize, "PR"),
                plural(n(&bs["rounds"]) as usize, "address round"),
                if replies == 1 { "reply" } else { "replies" },
                n(&bs["merged"])
            ))
            .font(font(12.5, 400.0).lh(16.0))
            .color(k.muted),
        );
        for (i, x) in list.iter().enumerate() {
            let mut meta = vec![];
            if let Some(p) = int(&x["pr"]) {
                meta.push(format!("#{p}"));
            }
            if model::flag(&x["opened"]) {
                meta.push("opened".into());
            }
            if n(&x["rounds"]) > 0 {
                meta.push(plural(n(&x["rounds"]) as usize, "round"));
            }
            if let Some(o) = x["outcome"].as_str() {
                meta.push(o.to_string());
            }
            let merged = x["outcome"].as_str() == Some("merged");
            builds = builds.child(recap_row(
                format!("recap-build-{i}"),
                icon(&icons::HAMMER, 15.0).color(if merged { k.working } else { k.accent }),
                str_or(&x["id"], ""),
                str_or(&x["title"], ""),
                meta.join(" · "),
                None,
                x["at"].as_str(),
                Open::Docket(model::text(&x["id"])),
                k,
            ));
        }
        c = c.child(builds);
    }

    // You: your decisions, approvals and intents.
    let decisions = arr(&you["decisions"]);
    let asked = n(&you["asked"]);
    let mut yours = section("You", k).child(
        text(format!(
            "{} · {} · {} denied · {}{}",
            plural(decisions.len(), "decision"),
            plural(n(&you["approved"]) as usize, "approval"),
            n(&you["denied"]),
            plural(n(&you["intents"]) as usize, "intent edit"),
            if asked > 0 { format!(" · agents asked you {}", plural(asked as usize, "time")) } else { String::new() }
        ))
        .font(font(12.5, 400.0).lh(16.0))
        .color(k.muted),
    );
    for (i, x) in decisions.iter().enumerate() {
        yours = yours.child(recap_row(
            format!("recap-decision-{i}"),
            icon(&icons::APPROVE, 15.0).color(k.accent),
            str_or(&x["docket"], ""),
            str_or(&x["title"], ""),
            format!("“{}”", str_or(&x["choice"], "")),
            None,
            x["at"].as_str(),
            Open::Docket(model::text(&x["docket"])),
            k,
        ));
    }
    c.child(yours)
}

fn section(title: &str, k: &K) -> El {
    col().w_full().gap(10.0).child(text(title.to_string()).font(font(17.0, 600.0).lh(21.0)).color(k.text).role("heading").level(2))
}

fn pr_group(mut c: El, title: &str, prs: &[Value], kind: &str, k: &K) -> El {
    if prs.is_empty() {
        return c;
    }
    c = c.child(el().pt(4.0).child(parts::section_label(&format!("{title} · {}", prs.len()), k)));
    for (i, p) in prs.iter().enumerate() {
        let verdicts = model::strings(&p["verdicts"]);
        let last = verdicts.last().map(String::as_str);
        let trail = verdicts.iter().map(|v| if v == "APPROVED" { "✓" } else if v == "CHANGES_REQUESTED" { "✕" } else { "·" }).collect::<Vec<_>>().join(" → ");
        let state = if kind == "open" {
            match last {
                Some("APPROVED") => "approved, not merged yet".to_string(),
                Some("CHANGES_REQUESTED") => "changes requested".to_string(),
                _ if p["state"].as_str() == Some("reviewing") => "in review".to_string(),
                _ => "queued".to_string(),
            }
        } else {
            trail
        };
        let lines = [int(&p["additions"]).map(|a| format!("+{a}")), int(&p["deletions"]).map(|d| format!("−{d}"))].into_iter().flatten().collect::<Vec<_>>().join(" ");
        let meta = [
            Some(state),
            Some(plural(int(&p["rounds"]).unwrap_or(0) as usize, "round")),
            model::text(&p["author"]),
            (!lines.is_empty()).then_some(lines),
            model::text(&p["merge_risk"]).map(|r| format!("risk {r}")),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ");
        let tint = match kind {
            "merged" => k.working,
            "closed" => k.faint,
            _ => k.accent,
        };
        let open = match model::text(&p["docket"]) {
            Some(id) => Open::Docket(Some(id)),
            None => Open::Url(model::text(&p["url"])),
        };
        c = c.child(recap_row(
            format!("recap-pr-{kind}-{i}"),
            icon(&icons::PULL, 15.0).color(tint),
            str_or(&p["key"], ""),
            str_or(&p["title"], ""),
            meta,
            model::text(&p["implementation"]),
            p["ended_at"].as_str().or(p["last_at"].as_str()),
            open,
            k,
        ));
    }
    c
}

#[derive(Clone)]
enum Open {
    Docket(Option<String>),
    Url(Option<String>),
}

/// A row of the recap: its mark, key and title, what it was, and when. A
/// click shows its docket, or opens the PR.
#[allow(clippy::too_many_arguments)]
fn recap_row(key: String, mark: El, id: &str, title: &str, meta: String, detail: Option<String>, at: Option<&str>, open: Open, k: &K) -> El {
    let mut body = col()
        .gap(3.0)
        .grow(1.0)
        .min_w(0.0)
        .child(
            row()
                .w_full()
                .gap(8.0)
                .child(text(id.to_string()).font(mono(11.5).lh(17.0)).color(k.muted))
                .child(text(title.to_string()).font(font(13.0, 500.0).lh(17.0)).color(k.text).truncate().shrink())
                .child(spacer())
                .child(text(model::ago(at)).font(font(11.5, 400.0).lh(17.0)).color(k.faint)),
        )
        .child(text(meta).font(font(12.0, 400.0).lh(16.0)).color(k.muted).wrap());
    if let Some(dt) = detail {
        body = body.child(text(dt).font(font(12.0, 400.0).lh(16.0)).color(k.text2).wrap().max_lines(2));
    }
    row()
        .key(key)
        .w_full()
        .padding(10.0)
        .gap(10.0)
        .radius(8.0)
        .hover(k.hover)
        .role("button")
        .label(format!("{id} {title}"))
        .on_press_tx(move |s: &mut Main, tx: &mut Tx| match &open {
            Open::Docket(Some(id)) => s.show(id),
            Open::Url(Some(u)) => {
                let _ = tx.open_url(u);
            }
            _ => {}
        })
        .child(row().w(18.0).pt(1.0).justify_center().child(mark))
        .child(body)
}

/// A number, what it's of, and how it moved.
fn tile(label: &str, value: i64, sub: Option<String>, delta: Option<(i64, i64, &str)>, spark: Vec<i64>, k: &K) -> El {
    let big = font(28.0, 600.0).lh(34.0).features("'tnum' 1");
    let mut c = col()
        .grow(1.0)
        .min_w(160.0)
        .min_h(150.0)
        .padding(14.0)
        .gap(6.0)
        .radius(12.0)
        .continuous_corners()
        .fill(k.elev)
        .border(1.0, k.line)
        .child(text(label.to_string()).font(font(12.0, 400.0).lh(15.0)).color(k.muted))
        .child(text(value.to_string()).font(big).color(k.text));
    if let Some((now, before, words)) = delta {
        let d = now - before;
        let sign = if d > 0 { "+" } else if d < 0 { "−" } else { "±" };
        c = c.child(text(format!("{sign}{} vs {words}", d.abs())).font(font(11.5, 400.0).lh(14.0)).color(if d > 0 { k.working } else if d < 0 { k.error } else { k.faint }));
    }
    if let Some(sub) = sub {
        c = c.child(text(sub).font(font(11.5, 400.0).lh(14.0)).color(k.muted));
    }
    c = c.child(spacer());
    if spark.len() > 1 {
        c = c.child(sparkline(&spark, k));
    }
    c
}

/// Each day's count, as Swift Charts draws it here: a monotone line over a
/// faint area, the last day marked.
fn sparkline(v: &[i64], k: &K) -> El {
    const H: f32 = 30.0;
    let step = 10.0;
    let w = step * (v.len() - 1) as f32;
    let top = (*v.iter().max().unwrap_or(&1)).max(1) as f32;
    let pts: Vec<[f32; 2]> = v.iter().enumerate().map(|(i, &y)| [i as f32 * step, H - y as f32 / top * H]).collect();
    let line = monotone(&pts);
    let mut area = line.clone();
    area.push([w, H]);
    area.push([0.0, H]);
    let shape = Shape::new().fill(area, k.accent.alpha(0.08)).stroke(line, 1.5, k.faint).view_box(w, H);
    let last_y = pts.last().map(|p| p[1]).unwrap_or(H);
    el().w_full()
        .h(H)
        .a11y_hidden()
        .child(canvas(shape).absolute(0.0, 0.0).w_full().h(H))
        .child(row().absolute(0.0, 0.0).w_full().h(H).justify_end().child(el().size(6.0, 6.0).radius(3.0).fill(k.accent).mt(last_y - 3.0).offset(3.0, 0.0)))
}

/// Fritsch–Carlson monotone cubic through `p`, flattened: it never
/// overshoots a day's count, as `.interpolationMethod(.monotone)` doesn't.
fn monotone(p: &[[f32; 2]]) -> Vec<[f32; 2]> {
    let n = p.len();
    if n < 3 {
        return p.to_vec();
    }
    let d: Vec<f32> = (0..n - 1).map(|i| (p[i + 1][1] - p[i][1]) / (p[i + 1][0] - p[i][0])).collect();
    let mut m = vec![0.0f32; n];
    m[0] = d[0];
    m[n - 1] = d[n - 2];
    for i in 1..n - 1 {
        m[i] = if d[i - 1] * d[i] <= 0.0 { 0.0 } else { (d[i - 1] + d[i]) / 2.0 };
    }
    for i in 0..n - 1 {
        if d[i] == 0.0 {
            m[i] = 0.0;
            m[i + 1] = 0.0;
        } else {
            let (a, b) = (m[i] / d[i], m[i + 1] / d[i]);
            let s = a * a + b * b;
            if s > 9.0 {
                let t = 3.0 / s.sqrt();
                m[i] = t * a * d[i];
                m[i + 1] = t * b * d[i];
            }
        }
    }
    // Hermite segments, sampled: the area under the line follows the same
    // points as the line.
    let mut out = vec![p[0]];
    for i in 0..n - 1 {
        let h = p[i + 1][0] - p[i][0];
        for s in 1..=12 {
            let t = s as f32 / 12.0;
            let (t2, t3) = (t * t, t * t * t);
            let (h00, h10, h01, h11) = (2.0 * t3 - 3.0 * t2 + 1.0, t3 - 2.0 * t2 + t, -2.0 * t3 + 3.0 * t2, t3 - t2);
            out.push([p[i][0] + t * h, h00 * p[i][1] + h10 * h * m[i] + h01 * p[i + 1][1] + h11 * h * m[i + 1]]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_read_as_the_mac_app_writes_them() {
        assert_eq!(pct(1, 3), "33%");
        assert_eq!(pct(0, 0), "–");
        assert_eq!(dur(2400.0), "40m");
        assert_eq!(dur(11100.0), "3h 5m");
        assert_eq!(compact(11_400_000), "11M");
        assert_eq!(compact(1_400_000), "1.4M");
        assert_eq!(compact(1234), "1,234");
    }

    #[test]
    fn a_monotone_line_never_overshoots() {
        let pts = [[0.0, 30.0], [10.0, 30.0], [20.0, 0.0], [30.0, 30.0]];
        assert!(monotone(&pts).iter().all(|p| p[1] >= -0.01 && p[1] <= 30.01));
    }
}
