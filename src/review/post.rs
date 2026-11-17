//! Posting a checked review: rex-reviewer's `bin/post-review`, ported. It
//! renders the draft (docs: briefs/review/review-draft.md), refuses one that
//! breaks a rule, refuses when the PR moved past the head it covers, then
//! posts the replies and the review with your `gh` login.
//!
//! Two fixes over the original: the diff reader takes paths with spaces,
//! and doesn't mistake an added line starting `++` for a file header.

use serde_json::{Value, json};
use std::collections::HashSet;

use super::github;

/// Why nothing was posted.
#[derive(Debug, PartialEq)]
pub enum Refusal {
    /// The draft breaks a rule (post-review's exit 4).
    Rule(String),
    /// The PR's head moved past the one the review covers (exit 3).
    HeadMoved(String),
    /// GitHub, or reading the PR (exit 1).
    Other(String),
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refusal::Rule(s) | Refusal::HeadMoved(s) | Refusal::Other(s) => f.write_str(s),
        }
    }
}

/// jq's `//`: the value unless it's null or false.
fn truthy(v: &Value) -> Option<&Value> {
    (!v.is_null() && v != &Value::Bool(false)).then_some(v)
}

fn or<'a>(a: &'a Value, b: &'a Value) -> &'a Value {
    truthy(a).unwrap_or(b)
}

/// A value as jq's `tostring`/raw output would print it.
fn text(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn findings(d: &Value) -> Vec<&Value> {
    d["findings"].as_array().map(|a| a.iter().collect()).unwrap_or_default()
}

pub fn blocking_count(d: &Value) -> usize {
    findings(d).iter().filter(|f| f["severity"] == "blocking").count()
}

const RISK_WORDS: [(&str, &str, &[&str], f64); 5] = [
    ("alignment", "Alignment", &["poor", "decent", "good"], 1.0),
    ("impact", "Impact", &["large", "medium", "small"], 2.0),
    ("verification", "Verification", &["poor", "decent", "good"], 2.0),
    ("rollback_cost", "Rollback cost", &["high", "medium", "low"], 1.5),
    ("error_detection", "Error detection", &["poor", "decent", "good", "n/a"], 1.0),
];

/// The merge risk's level from its five verdicts: low, medium or high.
pub fn risk_level(m: &Value) -> Result<String, String> {
    // jq walked the categories in key order; the first bad one is reported.
    let mut keys: Vec<_> = RISK_WORDS.iter().collect();
    keys.sort_by_key(|k| k.0);
    let (mut sum, mut weights) = (0.0, 0.0);
    for (k, _, words, w) in keys {
        let v = text(or(&m[k]["verdict"], &Value::Null)).to_ascii_lowercase();
        let Some(i) = words.iter().position(|x| *x == v) else {
            return Err(format!("merge_risk.{k}.verdict must be one of {}, not \"{v}\"", words.join(", ")));
        };
        if !matches!(or(&m[k]["gaps"], &json!([])), Value::Array(_)) {
            return Err(format!("merge_risk.{k}.gaps must be a list"));
        }
        sum += [3.0, 2.0, 1.0, 1.0][i] * w;
        weights += w;
    }
    let score = sum / weights;
    Ok(if score < 1.5 { "low" } else if score < 2.25 { "medium" } else { "high" }.to_string())
}

const FIRST_PERSON: [&str; 15] = ["I", "I'm", "I've", "I'd", "I'll", "my", "My", "me", "we", "We", "we're", "We're", "our", "Our", "us"];

/// Lines of the draft's prose that speak in the first person, outside code.
pub fn first_person(d: &Value) -> Vec<String> {
    let mut parts: Vec<String> = vec![text(or(or(&d["verdict"], &d["summary"]), &json!(""))), text(or(&d["checked"], &json!("")))];
    for f in findings(d) {
        for k in ["bullet", "title", "body"] {
            parts.push(text(or(&f[k], &json!(""))));
        }
    }
    for r in d["replies"].as_array().into_iter().flatten() {
        parts.push(text(or(&r["body"], &json!(""))));
    }
    if let Some(m) = d["merge_risk"].as_object() {
        for c in m.values() {
            for g in or(&c["gaps"], &json!([])).as_array().into_iter().flatten() {
                parts.push(text(g));
            }
        }
    }
    let mut all = String::new();
    for p in parts {
        all.push_str(&p);
        all.push('\n');
    }
    let prose = strip_inline_code(&strip_fences(&all));
    let in_word = |c: char| c.is_alphanumeric() || matches!(c, '_' | '/' | '\'' | '-');
    prose
        .lines()
        .enumerate()
        .filter(|(_, l)| l.split(|c: char| !in_word(c)).any(|w| FIRST_PERSON.contains(&w)))
        .map(|(n, l)| format!("{}:{l}", n + 1).chars().take(200).collect())
        .collect()
}

/// perl's `s/```.*?```//gs`.
fn strip_fences(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find("```") {
        match rest[i + 3..].find("```") {
            Some(j) => {
                out.push_str(&rest[..i]);
                rest = &rest[i + 3 + j + 3..];
            }
            None => break,
        }
    }
    out.push_str(rest);
    out
}

/// perl's ``s/`[^`\n]*`//g``.
fn strip_inline_code(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find('`') {
        let after = &rest[i + 1..];
        let end = after.find(['`', '\n']);
        match end {
            Some(j) if after.as_bytes()[j] == b'`' => {
                out.push_str(&rest[..i]);
                rest = &after[j + 1..];
            }
            _ => {
                out.push_str(&rest[..=i]);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// The draft's rules, checked before anything is read from GitHub. Returns
/// the merge risk's level, when the draft has one.
pub fn check(d: &Value, require_risk: bool) -> Result<Option<String>, String> {
    let event = text(or(&d["event"], &json!("")));
    if event != "APPROVE" && event != "REQUEST_CHANGES" {
        return Err(format!("event must be APPROVE or REQUEST_CHANGES, not '{event}'"));
    }
    let blocking = blocking_count(d);
    if event == "APPROVE" && blocking > 0 {
        return Err(format!("event is APPROVE but {blocking} finding(s) are blocking"));
    }
    if event == "REQUEST_CHANGES" && blocking == 0 {
        return Err("event is REQUEST_CHANGES but no finding is blocking".into());
    }
    if text(or(or(&d["verdict"], &d["summary"]), &json!(""))).is_empty() {
        return Err("the draft has no verdict".into());
    }
    if findings(d).iter().any(|f| text(or(or(&f["bullet"], &f["title"]), &json!(""))).is_empty()) {
        return Err("every finding needs a bullet or a title".into());
    }
    let risk = match truthy(&d["merge_risk"]) {
        Some(m) => Some(risk_level(m)?),
        None if require_risk => return Err("the draft has no merge_risk; add the merge-risk assessment".into()),
        None => None,
    };
    let hits = first_person(d);
    if !hits.is_empty() {
        return Err(format!(
            "the draft speaks in the first person; the footer already says who reviewed. Rewrite these lines:\n{}",
            hits.join("\n")
        ));
    }
    Ok(risk)
}

/// The lines a review comment can go on: the new side's added and context
/// lines, as (path, line).
pub fn right_lines(diff: &str) -> HashSet<(String, i64)> {
    let mut out = HashSet::new();
    let mut path: Option<String> = None;
    let (mut old_left, mut new_left, mut n) = (0i64, 0i64, 0i64);
    for line in diff.lines() {
        if old_left > 0 || new_left > 0 {
            match line.as_bytes().first() {
                Some(b'+') => {
                    if let Some(p) = &path {
                        out.insert((p.clone(), n));
                    }
                    n += 1;
                    new_left -= 1;
                }
                Some(b'-') => old_left -= 1,
                Some(b'\\') => {}
                _ => {
                    if let Some(p) = &path {
                        out.insert((p.clone(), n));
                    }
                    n += 1;
                    old_left -= 1;
                    new_left -= 1;
                }
            }
            continue;
        }
        if let Some(p) = line.strip_prefix("+++ ") {
            let p = p.trim_end_matches('\t');
            path = if p == "/dev/null" { None } else { Some(p.strip_prefix("b/").unwrap_or(p).to_string()) };
        } else if line.starts_with("@@") {
            // @@ -a,b +c,d @@
            let mut parts = line.split_whitespace().skip(1);
            let count = |s: Option<&str>| -> (i64, i64) {
                let s = s.unwrap_or("").trim_start_matches(['-', '+']);
                let mut it = s.split(',');
                let start = it.next().and_then(|x| x.parse().ok()).unwrap_or(0);
                let len = it.next().and_then(|x| x.parse().ok()).unwrap_or(1);
                (start, len)
            };
            let (_, ol) = count(parts.next());
            let (ns, nl) = count(parts.next());
            old_left = ol;
            new_left = nl;
            n = ns;
        }
    }
    out
}

fn footer(who: &str, me: &str, extra: &str) -> String {
    format!("---\n\n<div align=\"right\"><sub><i>{who} on behalf of @{me}{extra}</i></sub></div>")
}

fn indent(s: &str) -> String {
    format!("  {}", s.replace('\n', "\n  "))
}

/// jq's `sub("[.;:]\\s*$"; "")` on a one-line bullet.
fn nodot(s: &str) -> String {
    let t = s.trim_end();
    match t.chars().last() {
        Some('.' | ';' | ':') => t[..t.len() - 1].to_string(),
        _ => s.to_string(),
    }
}

fn line_text(v: &Value) -> String {
    text(v)
}

fn risk_block(m: &Value, level: &str) -> String {
    let mut out = String::new();
    let mut sep = "";
    for (k, name, words, _) in RISK_WORDS {
        let v = text(&m[k]["verdict"]).to_ascii_lowercase();
        let i = words.iter().position(|x| *x == v).unwrap_or(0);
        let gaps: Vec<String> = or(&m[k]["gaps"], &json!([])).as_array().into_iter().flatten().map(text).collect();
        let shown = if v == "n/a" { "N/A".to_string() } else { v.clone() };
        let mut t = format!("**{name}:** {shown} {}", ["⚠️", "👀", "✅", "✅"][i]);
        for g in &gaps {
            t.push_str("\n- ");
            t.push_str(g);
        }
        out.push_str(sep);
        out.push_str(&t);
        sep = if gaps.is_empty() { "\n" } else { "\n\n" };
    }
    format!("**Merge risk: {level}**\n\n{out}")
}

/// Who wrote it and what it covers, for the body's footer.
pub struct Byline<'a> {
    pub by: &'a str,
    pub checked_by: Option<&'a str>,
    pub marker: Option<&'a str>,
    pub me: &'a str,
}

/// The review GitHub gets, and the replies that go before it.
#[derive(Debug, Clone)]
pub struct Rendered {
    pub payload: Value,
    pub replies: Vec<(Value, String)>,
}

pub fn render(d: &Value, b: &Byline, head: &str, lines: &HashSet<(String, i64)>, risk: Option<&str>) -> Rendered {
    let fs = findings(d);
    let inline: Vec<bool> = fs
        .iter()
        .map(|f| {
            let path = text(or(&f["path"], &json!("")));
            let line = f["line"].as_i64();
            match (path.is_empty(), line) {
                (false, Some(l)) => lines.contains(&(path.clone(), l)) && f["start_line"].as_i64().is_none_or(|s| lines.contains(&(path.clone(), s))),
                _ => false,
            }
        })
        .collect();
    let bullet = |f: &Value, inl: bool| -> String {
        let plus_one = text(or(&f["plus_one"], &json!("")));
        let t = if !plus_one.is_empty() {
            let label = text(or(&f["plus_one_label"], &json!("the existing comment")));
            let extra = text(or(&f["bullet"], &json!("")));
            format!("+1 to [{label}]({plus_one}){}", if extra.is_empty() { String::new() } else { format!(": {extra}") })
        } else {
            text(or(&f["bullet"], &f["title"]))
        };
        let body = text(or(&f["body"], &json!("")));
        if inl {
            let path = text(&f["path"]);
            let base = path.rsplit('/').next().unwrap_or(&path).to_string();
            format!("{}; details in the inline comment on `{base}:{}`.", nodot(&t), line_text(&f["line"]))
        } else if !body.is_empty() && plus_one.is_empty() {
            let path = text(or(&f["path"], &json!("")));
            let head = if path.is_empty() {
                t
            } else {
                let at = truthy(&f["line"]).map(|l| format!(":{}", line_text(l))).unwrap_or_default();
                format!("{} (`{path}{at}`).", nodot(&t))
            };
            format!("{head}\n\n{}", indent(&body))
        } else {
            t
        }
    };
    let section = |name: &str, sev: &str| -> Option<String> {
        let items: Vec<String> = fs.iter().zip(&inline).filter(|(f, _)| f["severity"] == sev).map(|(f, i)| format!("- {}", bullet(f, *i))).collect();
        (!items.is_empty()).then(|| format!("**{name}**\n{}", items.join("\n")))
    };
    let mut parts: Vec<String> = vec![text(or(&d["verdict"], &d["summary"]))];
    parts.extend(section("Blocking", "blocking"));
    parts.extend(section("Non-blocking", "non-blocking"));
    let checked = text(or(&d["checked"], &json!("")));
    if !checked.is_empty() {
        parts.push(checked);
    }
    if let Some(level) = risk {
        parts.push(risk_block(&d["merge_risk"], level));
    }
    let extra = b.checked_by.filter(|c| !c.is_empty()).map(|c| format!(" · checked by {c}")).unwrap_or_default();
    let marker = b.marker.filter(|m| !m.is_empty()).map(|m| format!("\n<!-- pr-review {m} -->")).unwrap_or_default();
    parts.push(format!("{}{marker}", footer(b.by, b.me, &extra)));

    let comments: Vec<Value> = fs
        .iter()
        .zip(&inline)
        .filter(|(_, i)| **i)
        .map(|(f, _)| {
            let side = text(or(&f["side"], &json!("RIGHT")));
            let mut body = text(&f["body"]);
            if !f["suggestion"].is_null() {
                body.push_str(&format!("\n\n```suggestion\n{}\n```", text(&f["suggestion"])));
            }
            body.push_str(&format!("\n\n{}", footer(b.by, b.me, "")));
            let mut c = json!({ "path": f["path"], "line": f["line"], "side": side, "body": body });
            if truthy(&f["start_line"]).is_some() {
                c["start_line"] = f["start_line"].clone();
                c["start_side"] = json!(side);
            }
            c
        })
        .collect();
    let mut payload = json!({ "commit_id": head, "event": text(&d["event"]), "body": parts.join("\n\n") });
    if !comments.is_empty() {
        payload["comments"] = json!(comments);
    }
    let replies = d["replies"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|r| (r["comment_id"].clone(), format!("{}\n\n{}", text(&r["body"]), footer(b.by, b.me, ""))))
        .collect();
    Rendered { payload, replies }
}

/// The dry run's text: what would post.
pub fn preview_text(r: &Rendered) -> String {
    let p = &r.payload;
    let head: String = text(&p["commit_id"]).chars().take(7).collect();
    let mut out = format!("{} on {head}\n\n{}", text(&p["event"]), text(&p["body"]));
    for c in p["comments"].as_array().into_iter().flatten() {
        out.push_str(&format!("\n\n── inline on {}:{} ──\n{}", text(&c["path"]), line_text(&c["line"]), text(&c["body"])));
    }
    for (id, body) in &r.replies {
        out.push_str(&format!("\n\n── reply to comment {} ──\n{body}", text(id)));
    }
    out
}

pub struct Args<'a> {
    pub repo: &'a str,
    pub pr: i64,
    pub head: Option<&'a str>,
    pub by: &'a str,
    pub checked_by: Option<&'a str>,
    pub marker: Option<&'a str>,
    pub require_risk: bool,
    pub dry: bool,
}

/// What posted: `{review_id, state, url, inline, replies, merge_risk}`; or
/// with `dry`, `{dry_run: true, preview}`.
pub async fn run(a: &Args<'_>, draft: &Value) -> Result<Value, Refusal> {
    let risk = check(draft, a.require_risk).map_err(Refusal::Rule)?;
    let pr = a.pr.to_string();
    let live = github::gh(&["pr", "view", &pr, "-R", a.repo, "--json", "headRefOid,state"], None)
        .await
        .map_err(|e| Refusal::Other(format!("can't read #{pr}: {e:#}")))?;
    let live: Value = serde_json::from_str(&live).map_err(|e| Refusal::Other(format!("can't read #{pr}: {e}")))?;
    let state = text(&live["state"]);
    let live_head = text(&live["headRefOid"]);
    let mut warning = None;
    if state != "OPEN" {
        if !a.dry {
            return Err(Refusal::Other(format!("#{pr} is {state}")));
        }
        warning = Some(format!("#{pr} is {state}; rendering anyway for the preview"));
    }
    if let Some(h) = a.head
        && h != live_head
    {
        let short = |s: &str| s.chars().take(7).collect::<String>();
        return Err(Refusal::HeadMoved(format!("#{pr} moved to {}; this review covers {}. Review the new commits first.", short(&live_head), short(h))));
    }
    let head = a.head.map(str::to_string).unwrap_or(live_head);
    let me = github::login().await.map_err(|e| Refusal::Other(format!("can't read the signed-in GitHub account: {e:#}")))?;
    let diff = github::gh(&["pr", "diff", &pr, "-R", a.repo], None).await.map_err(|e| Refusal::Other(format!("can't read #{pr}'s diff: {e:#}")))?;
    let lines = right_lines(&diff);
    let byline = Byline { by: a.by, checked_by: a.checked_by, marker: a.marker, me: &me };
    let r = render(draft, &byline, &head, &lines, risk.as_deref());
    if a.dry {
        return Ok(json!({ "dry_run": true, "preview": preview_text(&r), "warning": warning }));
    }
    let mut replied = 0;
    for (id, body) in &r.replies {
        let path = format!("repos/{}/pulls/{pr}/comments/{}/replies", a.repo, text(id));
        github::gh(&["api", "-X", "POST", &path, "--input", "-"], Some(json!({ "body": body }).to_string()))
            .await
            .map_err(|e| Refusal::Other(format!("GitHub refused the reply to comment {}: {e:#}", text(id))))?;
        replied += 1;
    }
    let path = format!("repos/{}/pulls/{pr}/reviews", a.repo);
    let out = github::gh(&["api", "-X", "POST", &path, "--input", "-"], Some(r.payload.to_string()))
        .await
        .map_err(|e| Refusal::Other(format!("GitHub refused the review: {e:#}")))?;
    let out: Value = serde_json::from_str(&out).unwrap_or_default();
    let inline = r.payload["comments"].as_array().map_or(0, Vec::len);
    Ok(json!({ "review_id": out["id"], "state": out["state"], "url": out["html_url"], "inline": inline, "replies": replied, "merge_risk": risk }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft() -> Value {
        json!({
            "event": "REQUEST_CHANGES",
            "verdict": "Close, but retries can double-charge.",
            "implementation": "Adds retries.",
            "findings": [
                { "id": "B1", "severity": "blocking", "title": "Retries aren't idempotent.",
                  "path": "src/pay.rs", "line": 12, "body": "A retry after a timeout charges twice.",
                  "suggestion": "retry_with_key(key)", "evidence": "probe", "found_by": ["claude"] },
                { "id": "N1", "severity": "non-blocking", "title": "The doc comment is stale.",
                  "path": "src/pay.rs", "line": 99, "body": "It still says three tries." },
                { "id": "N2", "severity": "non-blocking", "plus_one": "https://x/1#r2", "plus_one_label": "Sam's point", "bullet": "and add a test" }
            ],
            "checked": "Ran the suite.",
            "merge_risk": {
                "alignment": { "verdict": "good", "gaps": [] },
                "impact": { "verdict": "medium", "gaps": ["Payments."] },
                "verification": { "verdict": "decent", "gaps": [] },
                "rollback_cost": { "verdict": "low", "gaps": [] },
                "error_detection": { "verdict": "n/a", "gaps": [] }
            },
            "replies": [{ "comment_id": 41, "body": "Fixed in abc1234." }]
        })
    }

    const DIFF: &str = "diff --git a/src/pay.rs b/src/pay.rs\n--- a/src/pay.rs\n+++ b/src/pay.rs\n@@ -10,3 +10,4 @@ fn x\n ctx\n+++counter;\n-old\n+new\n ctx2\ndiff --git a/my file.txt b/my file.txt\n--- /dev/null\n+++ b/my file.txt\n@@ -0,0 +1 @@\n+hello\n";

    #[test]
    fn the_diff_reader_finds_the_right_side() {
        let l = right_lines(DIFF);
        // 10 ctx, 11 "++counter" (an added line, not a header), 12 new, 13 ctx2.
        for n in 10..=13 {
            assert!(l.contains(&("src/pay.rs".into(), n)), "{n}");
        }
        assert!(!l.contains(&("src/pay.rs".into(), 14)));
        assert!(l.contains(&("my file.txt".into(), 1)), "{l:?}");
    }

    #[test]
    fn a_good_draft_renders_as_post_review_did() {
        let d = draft();
        // good 1·1, medium 2·2, decent 2·2, low 1·1.5, n/a 1·1 → 11.5/7.5 ≈ 1.53
        assert_eq!(check(&d, true).unwrap().as_deref(), Some("medium"));
        let lines = right_lines(DIFF);
        let b = Byline { by: "Claude Code (Opus 5.5)", checked_by: Some("Claude Code (Opus 5.5)"), marker: Some("round=2 head=abc"), me: "you" };
        let r = render(&d, &b, "abcdef1234", &lines, Some("medium"));
        let body = r.payload["body"].as_str().unwrap();
        assert!(body.starts_with("Close, but retries can double-charge.\n\n**Blocking**\n- Retries aren't idempotent; details in the inline comment on `pay.rs:12`."), "{body}");
        assert!(body.contains("**Non-blocking**\n- The doc comment is stale (`src/pay.rs:99`).\n\n  It still says three tries.\n- +1 to [Sam's point](https://x/1#r2): and add a test"), "{body}");
        assert!(body.contains("Ran the suite.\n\n**Merge risk: medium**\n\n**Alignment:** good ✅\n**Impact:** medium 👀\n- Payments.\n\n**Verification:** decent 👀\n**Rollback cost:** low ✅\n**Error detection:** N/A ✅"), "{body}");
        assert!(body.ends_with("<div align=\"right\"><sub><i>Claude Code (Opus 5.5) on behalf of @you · checked by Claude Code (Opus 5.5)</i></sub></div>\n<!-- pr-review round=2 head=abc -->"), "{body}");
        let c = r.payload["comments"].as_array().unwrap();
        assert_eq!(c.len(), 1, "line 99 isn't in the diff, so N1 goes in the body");
        assert_eq!(c[0]["side"], "RIGHT");
        assert!(c[0]["body"].as_str().unwrap().starts_with("A retry after a timeout charges twice.\n\n```suggestion\nretry_with_key(key)\n```\n\n---"));
        assert!(!r.payload.to_string().contains("evidence") && !r.payload.to_string().contains("found_by") && !body.contains("Adds retries"));
        assert_eq!(r.replies.len(), 1);
        assert!(r.replies[0].1.starts_with("Fixed in abc1234.\n\n---"));
        assert!(preview_text(&r).starts_with("REQUEST_CHANGES on abcdef1\n\n"));
    }

    #[test]
    fn broken_rules_are_refused() {
        let mut d = draft();
        d["event"] = json!("COMMENT");
        assert!(check(&d, false).unwrap_err().contains("not 'COMMENT'"));
        d["event"] = json!("APPROVE");
        assert!(check(&d, false).unwrap_err().contains("1 finding(s) are blocking"));
        let mut d = draft();
        d["findings"] = json!([]);
        assert!(check(&d, false).unwrap_err().contains("no finding is blocking"));
        let mut d = draft();
        d["verdict"] = json!("");
        assert_eq!(check(&d, false).unwrap_err(), "the draft has no verdict");
        let mut d = draft();
        d["findings"][1]["title"] = Value::Null;
        assert_eq!(check(&d, false).unwrap_err(), "every finding needs a bullet or a title");
        let mut d = draft();
        d["merge_risk"]["impact"]["verdict"] = json!("huge");
        assert_eq!(check(&d, false).unwrap_err(), "merge_risk.impact.verdict must be one of large, medium, small, not \"huge\"");
        let mut d = draft();
        d["merge_risk"]["impact"]["gaps"] = json!("none");
        assert_eq!(check(&d, false).unwrap_err(), "merge_risk.impact.gaps must be a list");
        let mut d = draft();
        d.as_object_mut().unwrap().remove("merge_risk");
        assert!(check(&d, true).unwrap_err().contains("no merge_risk"));
        assert_eq!(check(&d, false).unwrap(), None);
    }

    #[test]
    fn first_person_is_caught_outside_code_only() {
        let mut d = draft();
        d["findings"][0]["body"] = json!("I think this charges twice.\n```\nlet me = 1; // we\n```\nSee `our_fn` and the user's own/my-path and I-beam.");
        let hits = first_person(&d);
        assert_eq!(hits.len(), 1, "{hits:?}");
        assert!(hits[0].contains("I think"));
        assert!(check(&d, false).unwrap_err().contains("first person"));
        d["findings"][0]["body"] = json!("We’re close; it's fine. Me and Us are fine too.");
        // A curly apostrophe ends the word, as grep saw it: "We" alone.
        assert_eq!(first_person(&d).len(), 1);
        assert_eq!(first_person(&draft()).len(), 0);
    }

    #[test]
    fn risk_levels_follow_the_weights() {
        let all = |a: &str, i: &str, v: &str, r: &str, e: &str| {
            json!({ "alignment": {"verdict": a}, "impact": {"verdict": i}, "verification": {"verdict": v},
                    "rollback_cost": {"verdict": r}, "error_detection": {"verdict": e} })
        };
        assert_eq!(risk_level(&all("poor", "large", "poor", "high", "poor")).unwrap(), "high");
        assert_eq!(risk_level(&all("good", "small", "good", "low", "n/a")).unwrap(), "low");
        // 2,2,2,2,2 → 2.0: medium
        assert_eq!(risk_level(&all("decent", "medium", "decent", "medium", "decent")).unwrap(), "medium");
        assert_eq!(risk_level(&all("GOOD", "Small", "good", "low", "good")).unwrap(), "low");
    }
}
