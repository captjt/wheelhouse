//! What a PR is for, read when its review kicks off: the Linear issues it
//! addresses, and the Review docket's intent.
//!
//! Linear's own link on the PR (its linkback comment) is the surest sign an
//! issue is addressed. Then come Linear's closing words ("Closes ENG-12"), its
//! other magic words ("Part of ENG-12", "Refs ENG-12"), the branch name, and
//! the title. A bare mention in the description doesn't count: PRs name
//! the issues around them all the time. Without a Linear key, the linkback
//! still gives the issue's title, link and description; with one, Linear
//! says how the issue stands now, and the hub follows it.

use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use std::path::Path;

use crate::hub::{Core, Shared, Who};

/// What the PR says about itself.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Facts {
    pub title: String,
    pub body: String,
    pub branch: String,
    pub author: Option<String>,
    pub url: Option<String>,
    /// Its conversation's comments: (login, body).
    pub comments: Vec<(String, String)>,
}

impl Facts {
    /// From a round's context: `pr.json` and `issue-comments.json`.
    pub fn from_context(dir: &Path) -> Option<Facts> {
        let read = |f: &str| std::fs::read_to_string(dir.join(f)).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok());
        let p = read("pr.json")?;
        let comments = read("issue-comments.json")
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default()
            .iter()
            .map(|c| (c["user"]["login"].as_str().unwrap_or("").to_string(), c["body"].as_str().unwrap_or("").to_string()))
            .collect();
        Some(Facts::from_view(&p, comments))
    }

    /// `gh pr view --json title,body,headRefName,author,url[,comments]`.
    fn from_view(p: &Value, comments: Vec<(String, String)>) -> Facts {
        let s = |k: &str| p[k].as_str().unwrap_or("").to_string();
        let mut comments = comments;
        if comments.is_empty() {
            comments = p["comments"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|c| (c["author"]["login"].as_str().unwrap_or("").to_string(), c["body"].as_str().unwrap_or("").to_string()))
                .collect();
        }
        Facts {
            title: s("title"),
            body: s("body"),
            branch: s("headRefName"),
            author: p["author"]["login"].as_str().map(str::to_string),
            url: p["url"].as_str().map(str::to_string),
            comments,
        }
    }

    /// From GitHub now, in one call.
    pub async fn fetch(repo: &str, pr: i64) -> Result<Facts> {
        let v = super::github::pr_view(repo, pr, "title,body,headRefName,author,url,comments").await?;
        Ok(Facts::from_view(&v, Vec::new()))
    }
}

/// An issue the PR addresses.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Found {
    /// `ENG-445`.
    pub key: String,
    pub url: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    /// `linkback`, `closes`, `refs`, `branch` or `title`: the surest first.
    pub how: &'static str,
    /// How it stands in Linear, when there's a key to ask with.
    pub state: Option<String>,
}

const CLOSING: [&str; 16] = [
    "close", "closes", "closed", "closing", "fix", "fixes", "fixed", "fixing", "resolve", "resolves", "resolved", "resolving", "complete", "completes", "completed", "completing",
];
const REFERRING: [&str; 8] = ["ref", "refs", "references", "part of", "related to", "contributes to", "toward", "towards"];

fn rank(how: &str) -> usize {
    ["linkback", "closes", "refs", "branch", "title"].iter().position(|h| *h == how).unwrap_or(9)
}

/// `ENG-445` at the start of `s` (any case), and its length: a team key of a
/// letter and up to nine letters or digits, a dash, and up to seven digits.
fn key_at(s: &str) -> Option<(String, usize)> {
    let b = s.as_bytes();
    if !b.first()?.is_ascii_alphabetic() {
        return None;
    }
    let team = b.iter().take_while(|c| c.is_ascii_alphanumeric()).count();
    if team > 10 || b.get(team) != Some(&b'-') {
        return None;
    }
    let digits = b[team + 1..].iter().take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 || digits > 7 {
        return None;
    }
    let end = team + 1 + digits;
    if b.get(end).is_some_and(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    Some((s[..end].to_ascii_uppercase(), end))
}

/// Every key in the text, with where it starts and ends.
fn keys_in(text: &str) -> Vec<(usize, usize, String)> {
    let mut out = Vec::new();
    let b = text.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let boundary = i == 0 || !(b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'-' || b[i - 1] == b'_');
        if boundary
            && b[i].is_ascii_alphabetic()
            && let Some((k, len)) = key_at(&text[i..])
        {
            out.push((i, i + len, k));
            i += len;
            continue;
        }
        i += 1;
    }
    out
}

fn team(key: &str) -> &str {
    key.split('-').next().unwrap_or("")
}

/// `https://linear.app/<workspace>/issue/<KEY>[/slug]` in the text: where
/// each starts and ends, its workspace, key and link.
fn linear_urls(text: &str) -> Vec<(usize, usize, String, String, String)> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(at) = text[from..].find("https://linear.app/") {
        let start = from + at;
        let end = text[start..].find(|c: char| c.is_whitespace() || matches!(c, ')' | '>' | '"' | '\'' | ']')).map_or(text.len(), |n| start + n);
        let url = &text[start..end];
        let parts: Vec<&str> = url["https://linear.app/".len()..].split('/').collect();
        if parts.len() >= 3
            && parts[1] == "issue"
            && let Some((k, len)) = key_at(parts[2])
            && len == parts[2].len()
        {
            out.push((start, end, parts[0].to_string(), k, url.trim_end_matches(['.', ',']).to_string()));
        }
        from = end.max(start + 1);
    }
    out
}

/// Whether the text just before a key ends in one of Linear's magic words:
/// `closes`, `refs`, or neither.
fn magic_before(before: &str) -> Option<&'static str> {
    let t = before.trim_end_matches(|c: char| c.is_whitespace() || matches!(c, ':' | '[' | '(' | '*' | '_' | '`' | '#'));
    let lower = t.to_lowercase();
    let ends = |w: &str| lower.ends_with(w) && lower[..lower.len() - w.len()].chars().next_back().is_none_or(|c| !c.is_alphanumeric());
    if CLOSING.iter().any(|w| ends(w)) {
        return Some("closes");
    }
    if REFERRING.iter().any(|w| ends(w)) {
        return Some("refs");
    }
    None
}

/// Only separators between two keys: "ENG-1, ENG-2 and ENG-3".
fn just_separators(between: &str) -> bool {
    let t = between.replace(" and ", " ").replace('&', " ");
    let t = t.trim_matches(|c: char| c.is_whitespace() || matches!(c, ',' | '[' | ']' | '(' | ')' | '*' | '_' | '`' | '#'));
    t.is_empty()
}

/// Keys Linear's magic words point at in the text, with which kind. Keys
/// in prose are written in capitals ("Fixes ENG-12"), so "fix: utf-8
/// names" isn't one; a team Linear has shown counts in any case.
fn magic(text: &str, teams: &[String]) -> Vec<(String, &'static str)> {
    let urls = linear_urls(text);
    let mut out: Vec<(String, &'static str)> = Vec::new();
    let mut last: Option<(usize, &'static str)> = None;
    for (start, end, k) in keys_in(text) {
        let written = team(&text[start..end]);
        if !(written.chars().all(|c| !c.is_ascii_lowercase()) || teams.iter().any(|t| t.eq_ignore_ascii_case(written))) {
            last = None;
            continue;
        }
        // A key inside a Linear link counts from where the link starts:
        // "Closes https://linear.app/acme/issue/ENG-1/x" or "[ENG-1](url)".
        let at = urls.iter().find(|u| u.0 <= start && start < u.1).map_or(start, |u| u.0);
        let at = if at != start { text[..at].trim_end_matches(['(', ']']).rfind('[').filter(|b| at - b < 200).unwrap_or(at) } else { at };
        let window_start = text[..at].char_indices().rev().nth(39).map_or(0, |(i, _)| i);
        let kind = match last {
            Some((prev_end, kind)) if prev_end <= at && just_separators(&text[prev_end..at]) => Some(kind),
            _ => magic_before(&text[window_start..at]),
        };
        if let Some(kind) = kind {
            out.push((k, kind));
            last = Some((end, kind));
        } else {
            last = None;
        }
    }
    out
}

/// The linkback Linear leaves on a PR it's linked to: the issue's key,
/// link, title, and description.
fn linkback(body: &str) -> Option<Found> {
    if !body.contains("<!-- linear-linkback -->") {
        return None;
    }
    let sum = &body[body.find("<summary>")? + "<summary>".len()..];
    let href = &sum[sum.find("href=\"")? + 6..];
    let url = href[..href.find('"')?].to_string();
    let text = &href[href.find('>')? + 1..];
    let text = unescape(text[..text.find("</a>")?].trim());
    let (key, title) = match text.split_once(' ') {
        Some((k, t)) => (k.to_string(), Some(t.trim().to_string())),
        None => (text.clone(), None),
    };
    let key = key_at(&key).filter(|(_, len)| *len == key.len())?.0;
    let after = &sum[sum.find("</summary>")? + "</summary>".len()..];
    let description = after.find("<p>").and_then(|p| {
        let rest = &after[p + 3..];
        rest.rfind("</details>").map(|end| rest[..end].trim_end().trim_end_matches("</p>").trim().to_string())
    });
    Some(Found { key, url: Some(url), title, description: description.filter(|d| !d.is_empty()), how: "linkback", state: None })
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&#39;", "'").replace("&#x27;", "'").replace("&amp;", "&")
}

/// Whether a comment is Linear's: only Linear's app leaves linkbacks.
fn from_linear(login: &str) -> bool {
    let l = login.trim_end_matches("[bot]");
    l == "linear" || l.starts_with("linear-")
}

/// The issues the PR addresses, the surest first. Keys found only in the
/// branch or title count when their team is one Linear has shown before
/// (`teams`), so `utf-8` or `sha-256` in a branch isn't an issue.
pub fn detect(f: &Facts, teams: &[String]) -> Vec<Found> {
    let mut out: Vec<Found> = Vec::new();
    let mut add = |fd: Found| match out.iter_mut().find(|o| o.key == fd.key) {
        Some(o) => {
            if rank(fd.how) < rank(o.how) {
                o.how = fd.how;
            }
            o.url = o.url.take().or(fd.url);
            o.title = o.title.take().or(fd.title);
            o.description = o.description.take().or(fd.description);
        }
        None => out.push(fd),
    };
    for (login, body) in &f.comments {
        if from_linear(login)
            && let Some(fd) = linkback(body)
        {
            add(fd);
        }
    }
    let mut known: Vec<String> = teams.iter().map(|t| t.to_ascii_uppercase()).collect();
    let text = format!("{}\n\n{}", f.title, f.body);
    for (k, kind) in magic(&text, &known.clone()) {
        known.push(team(&k).to_string());
        add(Found { key: k, how: if kind == "closes" { "closes" } else { "refs" }, ..Default::default() });
    }
    for (k, how) in keys_in(&f.branch).into_iter().map(|(_, _, k)| (k, "branch")).chain(keys_in(&f.title).into_iter().map(|(_, _, k)| (k, "title"))) {
        if known.iter().any(|t| t == team(&k)) {
            add(Found { key: k, how, ..Default::default() });
        }
    }
    // Links the description gives for issues found some other way.
    for (_, _, _, k, url) in linear_urls(&f.body) {
        if let Some(o) = out.iter_mut().find(|o| o.key == k && o.url.is_none()) {
            o.url = Some(url);
        }
    }
    out.sort_by_key(|o| rank(o.how));
    out
}

/// Linear workspaces named in the PR's links: `acme`.
pub fn workspaces(f: &Facts) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for text in std::iter::once(f.body.as_str()).chain(f.comments.iter().filter(|(l, _)| from_linear(l)).map(|(_, b)| b.as_str())) {
        for (_, _, ws, _, _) in linear_urls(text) {
            if !out.contains(&ws) {
                out.push(ws);
            }
        }
    }
    out
}

// ---- The intent ----

/// The description with what isn't the author's account taken out: HTML
/// comments, a plan block, the footer, and a leading Linear line.
fn plain_body(body: &str) -> String {
    let mut s = crate::build::plan::strip(body);
    while let (Some(a), Some(b)) = (s.find("<!--"), s.find("-->")) {
        if b < a {
            break;
        }
        s = format!("{}{}", &s[..a], &s[b + 3..]);
    }
    let mut lines: Vec<&str> = s.lines().collect();
    if let Some(i) = lines.iter().rposition(|l| l.trim() == "---" || l.contains("Generated with [") || l.trim_start().starts_with("🤖")) {
        lines.truncate(i);
    }
    lines.join("\n").trim().to_string()
}

/// The author's own summary: the first section's opening paragraphs, up to
/// a code block, table, rule or the next heading, at most ~700 characters.
/// A paragraph that ends with a colon keeps the list it introduces.
pub fn summary(body: &str) -> Option<String> {
    let body = plain_body(body);
    let lines: Vec<&str> = body.lines().collect();
    let mut paras: Vec<String> = Vec::new();
    let mut cur: Vec<&str> = Vec::new();
    let mut started = false;
    let mut i = 0;
    while i < lines.len() {
        let t = lines[i].trim();
        let heading = t.starts_with('#');
        let rule = t.len() >= 3 && ['-', '*', '_'].iter().any(|r| t.chars().all(|c| c == *r || c == ' '));
        let list = list_item(t).is_some();
        let block = rule || list || t.starts_with("```") || t.starts_with("~~~") || t.starts_with('|') || t.starts_with('<') || t.starts_with('>') || t.starts_with("![");
        if list && started && cur.last().or(paras.last().map(|p| p.as_str()).filter(|_| cur.is_empty()).as_ref()).is_some_and(|l| l.ends_with(':')) {
            if !cur.is_empty() {
                paras.push(cur.join(" "));
                cur.clear();
            }
            let mut items: Vec<String> = Vec::new();
            while i < lines.len() && !lines[i].trim().is_empty() {
                let l = lines[i];
                match list_item(l.trim()) {
                    Some(item) if !l.starts_with([' ', '\t']) => items.push(item.to_string()),
                    _ if l.starts_with([' ', '\t']) && !items.is_empty() && list_item(l.trim()).is_none() => {
                        let last = items.last_mut().expect("checked");
                        last.push(' ');
                        last.push_str(l.trim());
                    }
                    Some(_) => {}
                    None => break,
                }
                i += 1;
            }
            let n = items.len();
            let mut list: Vec<String> = items.into_iter().take(6).map(|x| format!("- {}", clip(&x, 200))).collect();
            if n > 6 {
                list.push(format!("- and {} more", n - 6));
            }
            paras.push(list.join("\n"));
            break;
        }
        i += 1;
        if heading && !started {
            continue;
        }
        if heading || block {
            if started || !cur.is_empty() {
                break;
            }
            continue;
        }
        if t.is_empty() {
            if !cur.is_empty() {
                paras.push(cur.join(" "));
                cur.clear();
            }
            continue;
        }
        // "Closes ENG-12." alone, or "Part of ENG-12." leading a paragraph:
        // the issue is named above it.
        if !started && cur.is_empty() && magic(t, &[]).first().is_some_and(|_| magic_before_line(t)) {
            continue;
        }
        started = true;
        cur.push(t);
    }
    if !cur.is_empty() {
        paras.push(cur.join(" "));
    }
    let mut out = String::new();
    let mut prev_colon = false;
    for p in paras {
        let introduced = prev_colon && p.starts_with("- ");
        if !out.is_empty() && !introduced && out.len() + p.len() > 700 {
            break;
        }
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        prev_colon = p.ends_with(':');
        out.push_str(&p);
    }
    // What the colon introduced (code, a table) isn't here: say more follows.
    if out.ends_with(':') {
        out.pop();
        out.push('…');
    }
    if out.len() > 2000 {
        let cut = out.char_indices().take_while(|(i, _)| *i < 1980).last().map_or(0, |(i, c)| i + c.len_utf8());
        out = format!("{}…", out[..cut].trim_end());
    }
    (!out.trim().is_empty()).then_some(out)
}

/// A list item's text: `- x`, `* x`, `+ x`, `1. x`.
fn list_item(t: &str) -> Option<&str> {
    t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")).or_else(|| t.strip_prefix("+ ")).or_else(|| {
        let d = t.find(|c: char| !c.is_ascii_digit())?;
        (d > 0 && (t[d..].starts_with(". ") || t[d..].starts_with(") "))).then(|| &t[d + 2..])
    })
}

fn clip(s: &str, n: usize) -> String {
    if s.chars().count() > n { format!("{}…", s.chars().take(n - 1).collect::<String>().trim_end()) } else { s.to_string() }
}

/// A line that's only the issue line: "Closes ENG-12.", "Part of ENG-12."
/// or "Closes https://linear.app/acme/issue/ENG-12/slug".
fn magic_before_line(t: &str) -> bool {
    let keys = keys_in(t);
    let Some(last) = keys.last() else { return false };
    let end = linear_urls(t).iter().find(|u| u.0 <= last.0 && last.0 < u.1).map_or(last.1, |u| u.1);
    let rest = t[end..].trim_matches(|c: char| c.is_whitespace() || matches!(c, '.' | ')' | ']' | '>'));
    rest.is_empty() || rest.starts_with("](")
}

/// An issue's acceptance criteria: the list under its "Acceptance criteria"
/// (or "Acceptance", "Done when", "Definition of done") heading.
pub fn acceptance(description: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut inside = false;
    for line in description.lines() {
        let t = line.trim();
        if t.starts_with('#') {
            let h = t.trim_start_matches('#').trim().trim_end_matches(':').to_lowercase();
            inside = ["acceptance criteria", "acceptance", "done when", "definition of done"].contains(&h.as_str());
            continue;
        }
        if !inside {
            continue;
        }
        let top = !line.starts_with([' ', '\t']);
        let item = t.strip_prefix("* ").or_else(|| t.strip_prefix("- ")).or_else(|| t.strip_prefix("+ ")).or_else(|| {
            let d = t.find(|c: char| !c.is_ascii_digit())?;
            (d > 0 && t[d..].starts_with(". ")).then(|| &t[d + 2..])
        });
        match item {
            Some(i) if top => out.push(i.trim().to_string()),
            _ if !t.is_empty() && !out.is_empty() && !top => {
                let last = out.last_mut().expect("checked");
                last.push(' ');
                last.push_str(t.trim_start_matches(['*', '-', '+']).trim());
            }
            _ => {}
        }
    }
    out.into_iter()
        .take(10)
        .map(|i| if i.chars().count() > 320 { format!("{}…", i.chars().take(300).collect::<String>().trim_end()) } else { i })
        .collect()
}

fn issue_link(f: &Found) -> String {
    match &f.url {
        Some(u) => format!("[{}]({u})", f.key),
        None => f.key.clone(),
    }
}

/// The Review docket's intent: what the PR is, what it says it does, what
/// it addresses, and what the review holds it to.
pub fn intent(repo: &str, pr: i64, f: &Facts, found: &[Found], mode: &str) -> String {
    let pr_ref = match &f.url {
        Some(u) => format!("[{repo}#{pr}]({u})"),
        None => format!("{repo}#{pr}"),
    };
    let by = f.author.as_deref().map(|a| format!(" by @{a}")).unwrap_or_default();
    let mut s = format!("Review {pr_ref}{by}: {}.", f.title.trim().trim_end_matches('.'));
    if !found.is_empty() {
        let names: Vec<String> = found.iter().map(|i| format!("{}{}", issue_link(i), i.title.as_deref().map(|t| format!(" ({t})")).unwrap_or_default())).collect();
        let verb = if found.iter().all(|i| i.how == "closes") { "It closes" } else { "It addresses" };
        s.push_str(&format!(" {verb} {}.", join(&names)));
    }
    s.push('\n');
    if let Some(sum) = summary(&f.body) {
        s.push_str("\nWhat it says it does:\n\n");
        for (n, p) in sum.split("\n\n").enumerate() {
            if n > 0 {
                s.push_str(">\n");
            }
            for l in p.lines() {
                s.push_str(&format!("> {l}\n"));
            }
        }
    }
    s.push_str("\n## Acceptance\n\n");
    s.push_str(if mode == "auto" {
        "- Each request for your review gets a round: blind reviews, then a check. The checked review posts by itself.\n"
    } else {
        "- Each request for your review gets a round: blind reviews, then a check. The checked review posts once you approve it.\n"
    });
    let with_criteria: Vec<(&Found, Vec<String>)> = found.iter().filter_map(|i| Some((i, acceptance(i.description.as_deref()?)))).filter(|(_, a)| !a.is_empty()).collect();
    if with_criteria.is_empty() {
        let against = if found.is_empty() { String::new() } else { format!(", and against {}", join(&found.iter().map(|i| i.key.clone()).collect::<Vec<_>>())) };
        s.push_str(&format!("- The review holds the PR to what its description says it does{against}.\n"));
    } else {
        for (i, items) in with_criteria {
            s.push_str(&format!("- The review holds the PR to its part of {}'s acceptance criteria:\n", i.key));
            for a in items {
                s.push_str(&format!("  - {a}\n"));
            }
        }
    }
    s.push_str("- Done when the PR merges or closes, or no longer asks for your review.\n");
    s
}

fn join(xs: &[String]) -> String {
    match xs {
        [] => String::new(),
        [a] => a.clone(),
        [a, b] => format!("{a} and {b}"),
        _ => format!("{}, and {}", xs[..xs.len() - 1].join(", "), xs[xs.len() - 1]),
    }
}

/// `linear.md` for the reviewers: each issue, how it's linked, and what it
/// asks for.
pub fn linear_md(found: &[Found]) -> String {
    let mut s = String::from("# The Linear issues this PR addresses\n\nHold the PR to what they ask for. An issue's description may cover more than this PR; its own description says which part is this PR's.\n");
    for i in found {
        let how = match i.how {
            "linkback" => "Linear links it to the PR",
            "closes" => "the PR says it closes it",
            "refs" => "the PR says it's part of it, or refers to it",
            "branch" => "named in the branch",
            _ => "named in the title",
        };
        s.push_str(&format!("\n## {}{}\n\n", i.key, i.title.as_deref().map(|t| format!(": {t}")).unwrap_or_default()));
        s.push_str(&format!("- How it's linked: {how}\n"));
        if let Some(u) = &i.url {
            s.push_str(&format!("- Link: {u}\n"));
        }
        if let Some(st) = &i.state {
            s.push_str(&format!("- State in Linear: {st}\n"));
        }
        match &i.description {
            Some(d) => s.push_str(&format!("\n{}\n", d.trim())),
            None => s.push_str("\nIts description isn't available here.\n"),
        }
    }
    s
}

// ---- Applying it ----

/// Teams Linear has shown: from linkbacks and magic words seen before, and
/// every docket's linked issue.
fn known_teams(core: &Core) -> Vec<String> {
    let mut t: Vec<String> = core.store.meta_get("linear.teams").ok().flatten().unwrap_or_default().split(',').filter(|x| !x.is_empty()).map(str::to_string).collect();
    for s in core.store.states().unwrap_or_default() {
        if let Some(l) = s.linear {
            let k = team(&l.key).to_string();
            if !k.is_empty() && !t.contains(&k) {
                t.push(k);
            }
        }
    }
    t
}

fn learn(core: &mut Core, found: &[Found], ws: &[String]) -> Result<()> {
    let mut t: Vec<String> = core.store.meta_get("linear.teams")?.unwrap_or_default().split(',').filter(|x| !x.is_empty()).map(str::to_string).collect();
    let before = t.len();
    for f in found.iter().filter(|f| matches!(f.how, "linkback" | "closes" | "refs")) {
        let k = team(&f.key).to_string();
        if !t.contains(&k) {
            t.push(k);
        }
    }
    if t.len() != before {
        core.store.meta_set("linear.teams", &t.join(","))?;
    }
    if let Some(w) = ws.first()
        && core.store.meta_get("linear.workspace")?.is_none()
    {
        core.store.meta_set("linear.workspace", w)?;
    }
    Ok(())
}

/// Read what the PR is for and bring its docket along: link the issues it
/// addresses that aren't linked yet, and write the intent while it's
/// Wheelhouse's (once you edit it, it's yours). With a round's context
/// folder, `linear.md` goes there for the reviewers.
pub async fn integrate(shared: &Shared, key: &str, f: &Facts, ctx: Option<&Path>) -> Result<()> {
    let (teams, ws_known) = {
        let core = shared.lock();
        (known_teams(&core), core.store.meta_get("linear.workspace")?)
    };
    let mut found = detect(f, &teams);
    let ws = workspaces(f);
    let api = tokio::task::spawn_blocking(|| crate::linear::key().is_some()).await.unwrap_or(false);
    let mut issues: Vec<crate::linear::Issue> = Vec::new();
    if api {
        let mut keep = Vec::new();
        for fd in found.iter_mut() {
            match crate::linear::issue(&fd.key).await {
                Ok(i) => {
                    fd.url = Some(i.url.clone());
                    fd.title = Some(i.title.clone());
                    fd.description = i.description.clone().or(fd.description.take());
                    fd.state = i.state.clone();
                    issues.push(i);
                    keep.push(true);
                }
                // A key Linear doesn't know isn't an issue (or not one of
                // yours), unless Linear itself linked it.
                Err(e) if e.to_string().contains("has no issue") || e.to_string().to_lowercase().contains("not found") => keep.push(fd.how == "linkback"),
                Err(e) => {
                    say!("review: {key}: Linear {}: {e:#}", fd.key);
                    keep.push(true);
                }
            }
        }
        let mut k = keep.iter();
        found.retain(|_| *k.next().unwrap_or(&true));
    }
    for fd in found.iter_mut().filter(|fd| fd.url.is_none()) {
        if let Some(w) = ws.first().or(ws_known.as_ref()) {
            fd.url = Some(format!("https://linear.app/{w}/issue/{}", fd.key));
        }
    }
    if let Some(dir) = ctx {
        let p = dir.join("linear.md");
        if found.is_empty() {
            let _ = std::fs::remove_file(&p);
        } else {
            super::write_atomic(&p, &linear_md(&found))?;
        }
    }

    let cfg = super::config(shared).ok();
    let mut core = shared.lock();
    learn(&mut core, &found, &ws)?;
    let p = super::get(&core, key)?.ok_or_else(|| anyhow!("{key} isn't tracked"))?;
    let docket = super::docket_of(&core, key).context("it has no docket yet")?;
    let st = core.store.state(&docket)?.context("its docket is missing")?;
    let mut linked: Vec<String> = st.linear.iter().map(|l| l.key.clone()).chain(st.linear_also.iter().map(|l| l.key.clone())).collect();
    for fd in &found {
        if linked.contains(&fd.key) {
            continue;
        }
        let also = !linked.is_empty();
        let data = json!({ "kind": "linear", "source": "review", "key": fd.key, "url": fd.url, "title": fd.title, "how": fd.how, "pr": key, "also": also });
        let body = format!("{}{}", fd.key, fd.title.as_deref().map(|t| format!(": {t}")).unwrap_or_default());
        core.append(&docket, "link", &Who::Hub, Some(body), data)?;
        // The first issue is the one the hub follows in Linear.
        if !also && let Some(i) = issues.iter().find(|i| i.identifier.eq_ignore_ascii_case(&fd.key)) {
            crate::linear::observe(&mut core, &docket, i)?;
        }
        linked.push(fd.key.clone());
    }
    let mode = cfg.map(|c| super::post_mode(&core, &c)).unwrap_or_else(|| "ask".into());
    let body = intent(&p.repo, p.pr, f, &found, &mode);
    let ours = st.intent.as_ref().is_none_or(|i| i.source == "review");
    if ours && st.intent.as_ref().is_none_or(|i| i.body.trim() != body.trim()) {
        let keys: Vec<&str> = found.iter().map(|f| f.key.as_str()).collect();
        core.append(&docket, "intent", &Who::Hub, Some(body), json!({ "source": "review", "pr": key, "linear": keys }))?;
    }
    core.live_changed();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINKBACK: &str = "<!-- linear-linkback -->\n<details>\n<summary><a href=\"https://linear.app/acme/issue/ENG-445/webhook-retries\">ENG-445 Webhook retries &amp; backoff</a></summary>\n<p>\n\n## Outcome\n\nFailed deliveries retry with backoff.\n\n## Acceptance criteria\n\n* ~~The retry design is reviewed.~~ Signed off.\n* The retry PR is merged, and `just ci` passes\n  for `libs/webhooks`.\n* Existing behavior is unchanged.\n\n## Docs\n\n* Not this.\n\n</p>\n</details>\n<!-- linear-review-link -->\n<p><a href=\"https://linear.app/acme/review/x\">Review in Linear</a></p>";

    fn pr742() -> Facts {
        Facts {
            title: "feat(webhooks): retry failed deliveries with backoff".into(),
            body: "## What\n\nThis adds an optional `retries` setting to webhook delivery. It's the second ENG-445 PR, after #33. ENG-448 then turns it on.\n\n```json\n{}\n```\n\n## Not in this PR\n\n- **Turning it on**: ENG-448.\n\nRefs ENG-445\n\n🤖 Generated with [Claude Code](https://claude.com/claude-code)\n".into(),
            branch: "feat/eng-445-retries".into(),
            author: Some("dana".into()),
            url: Some("https://github.com/acme/api/pull/742".into()),
            comments: vec![("ci-bot".into(), "[vc]: #x".into()), ("linear-code".into(), LINKBACK.into()), ("dana".into(), "Fixed in 18f7e91.".into())],
        }
    }

    #[test]
    fn linears_link_on_the_pr_is_the_issue_and_bare_mentions_arent() {
        let found = detect(&pr742(), &[]);
        assert_eq!(found.len(), 1, "{found:?}");
        let f = &found[0];
        assert_eq!((f.key.as_str(), f.how), ("ENG-445", "linkback"));
        assert_eq!(f.title.as_deref(), Some("Webhook retries & backoff"));
        assert_eq!(f.url.as_deref(), Some("https://linear.app/acme/issue/ENG-445/webhook-retries"));
        assert!(f.description.as_deref().unwrap().starts_with("## Outcome"));
        assert!(f.description.as_deref().unwrap().ends_with("* Not this."), "{:?}", f.description);
        assert_eq!(workspaces(&pr742()), ["acme"]);
    }

    #[test]
    fn magic_words_branches_and_titles() {
        let f = |title: &str, body: &str, branch: &str| Facts { title: title.into(), body: body.into(), branch: branch.into(), ..Default::default() };
        let keys = |f: &Facts, teams: &[&str]| detect(f, &teams.iter().map(|t| t.to_string()).collect::<Vec<_>>()).into_iter().map(|x| (x.key, x.how)).collect::<Vec<_>>();
        assert_eq!(keys(&f("x", "Closes ENG-12.", ""), &[]), [("ENG-12".into(), "closes")]);
        assert_eq!(keys(&f("x", "Part of [ENG-438](https://linear.app/acme/issue/ENG-438/x). ENG-9 is next.", ""), &[]), [("ENG-438".into(), "refs")]);
        assert_eq!(keys(&f("x", "Fixes ENG-1, ENG-2 and ENG-3", ""), &[]), [("ENG-1".into(), "closes"), ("ENG-2".into(), "closes"), ("ENG-3".into(), "closes")]);
        assert_eq!(keys(&f("x", "fixed: https://linear.app/acme/issue/ENG-7/slug", ""), &[]), [("ENG-7".into(), "closes")]);
        assert_eq!(keys(&f("x", "This prefixes ENG-4 and resolves nothing; see ENG-5.", ""), &[]), Vec::<(String, &str)>::new());
        // The branch and title count for a team Linear has shown.
        assert_eq!(keys(&f("fix: utf-8 names", "", "fix/utf-8-names"), &["ENG"]), Vec::<(String, &str)>::new());
        assert_eq!(keys(&f("x", "closes eng-3", ""), &["ENG"]), [("ENG-3".into(), "closes")]);
        assert_eq!(keys(&f("x", "", "feat/ENG-386-skeletons"), &["ENG"]), [("ENG-386".into(), "branch")]);
        assert_eq!(keys(&f("feat(ENG-9): y", "", "main"), &["eng"]), [("ENG-9".into(), "title")]);
        assert_eq!(keys(&f("x", "Refs ENG-5", "feat/eng-6-y"), &[]), [("ENG-5".into(), "refs"), ("ENG-6".into(), "branch")]);
        // A linkback from anyone but Linear is only a comment.
        let mut spoof = pr742();
        spoof.comments = vec![("someone".into(), LINKBACK.into())];
        spoof.body = String::new();
        spoof.branch = String::new();
        assert!(detect(&spoof, &[]).is_empty());
    }

    #[test]
    fn the_intent_says_what_the_review_holds_the_pr_to() {
        let found = detect(&pr742(), &[]);
        let i = intent("acme/api", 742, &pr742(), &found, "ask");
        assert!(
            i.starts_with("Review [acme/api#742](https://github.com/acme/api/pull/742) by @dana: feat(webhooks): retry failed deliveries with backoff. It addresses [ENG-445](https://linear.app/acme/issue/ENG-445/webhook-retries) (Webhook retries & backoff).\n"),
            "{i}"
        );
        assert!(i.contains("What it says it does:\n\n> This adds an optional `retries` setting to webhook delivery. It's the second ENG-445 PR, after #33. ENG-448 then turns it on.\n"), "{i}");
        assert!(i.contains("posts once you approve it"), "{i}");
        assert!(i.contains("- The review holds the PR to its part of ENG-445's acceptance criteria:\n  - ~~The retry design is reviewed.~~ Signed off.\n  - The retry PR is merged, and `just ci` passes for `libs/webhooks`.\n  - Existing behavior is unchanged.\n- Done when"), "{i}");
        assert!(!i.contains("Not this") && !i.contains("Generated with"), "{i}");
        let bare = intent("o/api", 3, &Facts { title: "Tidy".into(), ..Default::default() }, &[], "auto");
        assert!(bare.starts_with("Review o/api#3: Tidy.\n\n## Acceptance"), "{bare}");
        assert!(bare.contains("posts by itself") && bare.contains("what its description says it does.\n"), "{bare}");
    }

    #[test]
    fn the_summary_skips_the_issue_line_and_the_footer() {
        let s = summary("Part of ENG-438. This is the API part.\n\n**Draft.** It deploys after X.\n\n## Why\n\nMore.").unwrap();
        assert_eq!(s, "Part of ENG-438. This is the API part.\n\n**Draft.** It deploys after X.");
        let s = summary("Closes ENG-12.\n\nRetries failed webhooks.\n\n---\nfooter").unwrap();
        assert_eq!(s, "Retries failed webhooks.");
        assert_eq!(summary("<!-- x -->\n🤖 Generated with [Claude Code](x)"), None);
        // A Linear link as the issue line; a rule ends the summary.
        let s = summary("Closes https://linear.app/acme/issue/ENG-293/use-pnpm\n\nSwitches to pnpm.\n\n---\n\nScreenshots").unwrap();
        assert_eq!(s, "Switches to pnpm.");
        assert_eq!(summary("It refuses models like these:\n\n```py\nx\n```").unwrap(), "It refuses models like these…");
        // A colon keeps the list it introduces.
        let s = summary("It refuses a model that hides in:\n\n- a tool\n- middleware,\n  wrapped\n  - nested\n\n## Why\n\nx").unwrap();
        assert_eq!(s, "It refuses a model that hides in:\n\n- a tool\n- middleware, wrapped");
        let q = intent("o/a", 1, &Facts { title: "T".into(), body: "It covers:\n- a\n- b".into(), ..Default::default() }, &[], "ask");
        assert!(q.contains("> It covers:\n>\n> - a\n> - b\n"), "{q}");
        let long = format!("{}\n\n{}", "a".repeat(600), "b".repeat(600));
        assert_eq!(summary(&long).unwrap(), "a".repeat(600));
    }

    #[tokio::test]
    async fn a_kicked_off_review_links_its_issue_and_writes_its_intent() {
        // SAFETY: no test reads this but Linear's key lookup.
        unsafe { std::env::set_var("WH_LINEAR_OFF", "1") };
        let shared = Shared::new(crate::hub::test_core());
        let key = "acme/api#742";
        let mut p = crate::review::pr::Pr { repo: "acme/api".into(), pr: 742, state: "queued".into(), title: Some(pr742().title), ..Default::default() };
        let d = crate::review::save(&mut shared.lock(), &mut p).unwrap();
        let ctx = std::env::temp_dir().join(format!("wh-links-{}", std::process::id()));
        std::fs::create_dir_all(&ctx).unwrap();
        integrate(&shared, key, &pr742(), Some(&ctx)).await.unwrap();
        let st = |s: &Shared| s.lock().store.state(&d).unwrap().unwrap();
        let s1 = st(&shared);
        let l = s1.linear.as_ref().unwrap();
        assert_eq!((l.key.as_str(), l.title.as_deref()), ("ENG-445", Some("Webhook retries & backoff")));
        let i = s1.intent.as_ref().unwrap();
        assert_eq!(i.source, "review");
        assert!(i.body.starts_with("Review [acme/api#742]"), "{}", i.body);
        let md = std::fs::read_to_string(ctx.join("linear.md")).unwrap();
        assert!(md.contains("## ENG-445: Webhook retries & backoff\n\n- How it's linked: Linear links it to the PR\n- Link: https://linear.app/acme/issue/ENG-445/webhook-retries\n"), "{md}");
        assert_eq!(shared.lock().store.meta_get("linear.teams").unwrap().as_deref(), Some("GRD"));

        // Nothing new: nothing written.
        integrate(&shared, key, &pr742(), None).await.unwrap();
        assert_eq!(st(&shared).entries, s1.entries);

        // The PR closes a second issue: linked beside the first. The new
        // description makes a new version of the intent.
        let mut f = pr742();
        f.body = format!("Closes ENG-450.\n\n{}", f.body);
        integrate(&shared, key, &f, None).await.unwrap();
        let s2 = st(&shared);
        assert_eq!(s2.linear.as_ref().unwrap().key, "ENG-445");
        assert_eq!(s2.linear_also.iter().map(|l| l.key.as_str()).collect::<Vec<_>>(), ["ENG-450"]);
        assert_eq!(s2.linear_also[0].url.as_deref(), Some("https://linear.app/acme/issue/ENG-450"));
        assert_eq!(s2.intent_versions, 2);
        assert!(s2.intent.as_ref().unwrap().body.contains("It addresses [ENG-445](https://linear.app/acme/issue/ENG-445/webhook-retries) (Webhook retries & backoff) and [ENG-450](https://linear.app/acme/issue/ENG-450)."));

        // Once you edit the intent, it's yours.
        shared.lock().append(&d, "intent", &Who::Owner, Some("Mine.".into()), json!({ "source": "person" })).unwrap();
        f.title = "feat: renamed".into();
        integrate(&shared, key, &f, None).await.unwrap();
        assert_eq!(st(&shared).intent.unwrap().body, "Mine.");
        let core = shared.lock();
        assert!(core.verify(&d).unwrap().ok);
        assert!(core.rebuild_check().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&ctx);
    }

    #[test]
    fn keys_are_whole_words() {
        assert_eq!(keys_in("x ENG-1, sha256-2x eng-42 a-1b").into_iter().map(|k| k.2).collect::<Vec<_>>(), ["ENG-1", "ENG-42"]);
        assert_eq!(keys_in("feat/eng-445-profile").into_iter().map(|k| k.2).collect::<Vec<_>>(), ["ENG-445"]);
    }
}
