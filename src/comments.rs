//! Comments on a diff's lines, in a docket's Changes. Each is a `note` on
//! the record (`via: "diff"`), signed by you, with what it's on in
//! `data.diff`: the file, the side (`additions`, the new lines, or
//! `deletions`, the old), the line or lines, the commits compared (`to` is
//! none for the working tree, with the `head` it was on), and the code it
//! was on, so it reads on its own after the lines move. The agents on the
//! docket are told, as messages placed at that entry.

use anyhow::{Result, bail};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::record::Entry;

/// A comment is at most this long.
pub const MAX_BODY: usize = 8_000;
/// The code a comment is on: at most this many lines, and bytes.
pub const MAX_EXCERPT_LINES: usize = 40;
pub const MAX_EXCERPT: usize = 4_000;
/// A comment spans at most this many lines.
pub const MAX_SPAN: i64 = 500;

/// A new comment, as the app sends it (`POST /api/dockets/{id}/comments`).
#[derive(Deserialize, Debug, Clone, Default)]
pub struct New {
    pub path: String,
    #[serde(default)]
    pub old_path: Option<String>,
    pub side: String,
    pub line: i64,
    #[serde(default)]
    pub end_line: Option<i64>,
    /// The commits compared: `to` is none for the working tree.
    pub from: String,
    #[serde(default)]
    pub to: Option<String>,
    /// HEAD when it was written, for one on the working tree.
    #[serde(default)]
    pub head: Option<String>,
    #[serde(default)]
    pub excerpt: Option<String>,
    pub body: String,
    /// Tell the agents on the docket (the default).
    #[serde(default = "yes")]
    pub tell: bool,
}

fn yes() -> bool {
    true
}

fn sha(s: &str, what: &str) -> Result<String> {
    let s = s.trim();
    if !(7..=64).contains(&s.len()) || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        bail!("{what} isn't a commit");
    }
    Ok(s.to_ascii_lowercase())
}

impl New {
    /// What goes in `data.diff`, once it's checked: a path that stays in the
    /// repo, a side, lines in range, commits that are commits, and an
    /// excerpt cut to size. The comment's words, trimmed.
    pub fn checked(&self) -> Result<(Value, String)> {
        let body = self.body.trim();
        if body.is_empty() {
            bail!("an empty comment");
        }
        if body.len() > MAX_BODY {
            bail!("a comment is at most {} KB: keep the rest in a note or a file", MAX_BODY / 1000);
        }
        let path = crate::diffs::safe_path(&self.path)?.to_string();
        let old_path = self.old_path.as_deref().filter(|p| !p.is_empty() && *p != path).map(crate::diffs::safe_path).transpose()?.map(str::to_string);
        if !matches!(self.side.as_str(), "additions" | "deletions") {
            bail!("a comment is on the new lines (additions) or the old (deletions)");
        }
        if !(1..=10_000_000).contains(&self.line) {
            bail!("no line {}", self.line);
        }
        let end = self.end_line.filter(|e| *e != self.line);
        if let Some(e) = end
            && !(self.line..=self.line + MAX_SPAN).contains(&e)
        {
            bail!("a comment spans at most {MAX_SPAN} lines, from the first to the last");
        }
        let from = sha(&self.from, "from")?;
        let to = self.to.as_deref().filter(|t| !t.is_empty()).map(|t| sha(t, "to")).transpose()?;
        let head = self.head.as_deref().filter(|h| !h.is_empty()).map(|h| sha(h, "head")).transpose()?;
        if to.is_none() && head.is_none() {
            bail!("a comment on the uncommitted changes needs the HEAD they were on");
        }
        let excerpt = self.excerpt.as_deref().map(cut).filter(|e| !e.trim().is_empty());
        let diff = json!({
            "path": path, "old_path": old_path, "side": self.side, "line": self.line, "end_line": end,
            "from": from, "to": to, "head": head, "excerpt": excerpt,
        });
        Ok((diff, body.to_string()))
    }
}

/// The code a comment is on, cut to `MAX_EXCERPT_LINES` lines and
/// `MAX_EXCERPT` bytes (on a character's edge).
fn cut(e: &str) -> String {
    let mut out: String = e.lines().take(MAX_EXCERPT_LINES).collect::<Vec<_>>().join("\n");
    if out.len() > MAX_EXCERPT {
        let mut at = MAX_EXCERPT;
        while !out.is_char_boundary(at) {
            at -= 1;
        }
        out.truncate(at);
    }
    out
}

/// Where a comment is, for people: `src/a.rs:12` or `src/a.rs:12–14`, with
/// `(old)` when it's on the removed lines.
pub fn place(d: &Value) -> String {
    let path = d["path"].as_str().unwrap_or("");
    let line = d["line"].as_i64().unwrap_or(0);
    let span = d["end_line"].as_i64().map(|e| format!("{line}–{e}")).unwrap_or_else(|| line.to_string());
    format!("{path}:{span}{}", if d["side"] == "deletions" { " (old)" } else { "" })
}

/// What the agents on the docket read: where, on what, the code, and the
/// comment.
pub fn message(docket: &str, d: &Value, body: &str) -> String {
    let at = match d["to"].as_str() {
        Some(t) => format!("at {}", &t[..7.min(t.len())]),
        None => format!("in the uncommitted changes (on {})", d["head"].as_str().map(|h| &h[..7.min(h.len())]).unwrap_or("HEAD")),
    };
    let mut out = format!("Comment on {} {at}, in {docket}'s Changes:\n", place(d));
    if let Some(e) = d["excerpt"].as_str() {
        let fence = if e.contains("```") { "````" } else { "```" };
        out.push_str(&format!("\n{fence}\n{e}\n{fence}\n"));
    }
    out.push_str(&format!("\n{body}\n"));
    out
}

/// Every comment on a docket's diff, oldest first, for the Changes view.
pub fn list(entries: &[(i64, Entry)]) -> Vec<Value> {
    entries
        .iter()
        .filter(|(_, e)| e.kind == "note" && e.data["via"] == "diff" && e.data["diff"].is_object())
        .map(|(seq, e)| {
            let by = match e.actor.typ.as_str() {
                "person" => "you".to_string(),
                _ => e.actor.id.split('@').next().unwrap_or("").to_string(),
            };
            json!({ "seq": seq, "at": e.at, "by": by, "body": e.body, "diff": e.data["diff"] })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new() -> New {
        New {
            path: "src/retry.rs".into(),
            side: "additions".into(),
            line: 12,
            from: "4e1d9a0".into(),
            to: Some("9F2C41AB".into()),
            excerpt: Some("    let tries = 5;".into()),
            body: "  Cap it at 3, as decided.  ".into(),
            tell: true,
            ..Default::default()
        }
    }

    #[test]
    fn a_comment_is_checked_before_it_goes_on_the_record() {
        let (d, body) = new().checked().unwrap();
        assert_eq!(body, "Cap it at 3, as decided.");
        assert_eq!(d["to"], "9f2c41ab");
        assert_eq!(d["end_line"], Value::Null, "one line");
        assert_eq!(place(&d), "src/retry.rs:12");

        let bad = |f: fn(&mut New)| {
            let mut n = new();
            f(&mut n);
            n.checked().unwrap_err().to_string()
        };
        assert!(bad(|n| n.body = " ".into()).contains("empty"));
        assert!(bad(|n| n.body = "x".repeat(MAX_BODY + 1)).contains("at most"));
        assert!(!bad(|n| n.path = "../etc/passwd".into()).is_empty());
        assert!(!bad(|n| n.path = "/etc/passwd".into()).is_empty());
        assert!(bad(|n| n.side = "both".into()).contains("new lines"));
        assert!(bad(|n| n.line = 0).contains("no line"));
        assert!(bad(|n| n.end_line = Some(11)).contains("spans"));
        assert!(bad(|n| n.end_line = Some(12 + MAX_SPAN + 1)).contains("spans"));
        assert!(bad(|n| n.from = "main; rm -rf".into()).contains("isn't a commit"));
        assert!(bad(|n| n.to = Some("HEAD".into())).contains("isn't a commit"));
        assert!(bad(|n| n.to = None).contains("needs the HEAD"));
    }

    #[test]
    fn a_span_the_working_tree_and_the_old_side_read_right() {
        let mut n = new();
        n.side = "deletions".into();
        n.end_line = Some(14);
        n.to = None;
        n.head = Some("abcdef0123".into());
        let (d, body) = n.checked().unwrap();
        assert_eq!(place(&d), "src/retry.rs:12–14 (old)");
        let m = message("D-41", &d, &body);
        assert!(m.starts_with("Comment on src/retry.rs:12–14 (old) in the uncommitted changes (on abcdef0), in D-41's Changes:"), "{m}");
        assert!(m.contains("```\n    let tries = 5;\n```"), "{m}");
        assert!(m.ends_with("Cap it at 3, as decided.\n"));
    }

    #[test]
    fn an_excerpt_is_cut_to_size_and_keeps_its_fences() {
        let mut n = new();
        n.excerpt = Some((0..100).map(|i| format!("line {i} é")).collect::<Vec<_>>().join("\n"));
        let (d, _) = n.checked().unwrap();
        let e = d["excerpt"].as_str().unwrap();
        assert_eq!(e.lines().count(), MAX_EXCERPT_LINES);
        n.excerpt = Some("é".repeat(MAX_EXCERPT));
        let (d, _) = n.checked().unwrap();
        assert!(d["excerpt"].as_str().unwrap().len() <= MAX_EXCERPT);
        n.excerpt = Some("```rust\nx\n```".into());
        let (d, body) = n.checked().unwrap();
        assert!(message("D-1", &d, &body).contains("````\n```rust"), "a fence inside gets a longer one");
        n.excerpt = Some("  \n ".into());
        assert_eq!(n.checked().unwrap().0["excerpt"], Value::Null);
    }
}
