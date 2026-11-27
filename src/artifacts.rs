//! Artifacts: what a docket's work produced, kept with the docket.
//!
//! A file goes in the blob store under the sha256 of its bytes, and an
//! `artifact` entry names that digest in `blobs`, so the signed record pins
//! the file's exact bytes: changing it, or losing it, fails the check. A
//! link (a Claude artifact, a page somewhere else) is an `artifact` entry
//! with its URL; the record pins that it was linked, not what's there.
//!
//! The hub keeps what it knows the work made: each review round's reports,
//! checked review, merge risk and posted review; each Build's plan and
//! checks. An agent keeps anything else with `wh attach`, and a Claude
//! artifact named in an agent's turn is kept as a link.

use anyhow::{Result, bail};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::hub::{Core, Who};
use crate::record::hex;

/// The largest file kept: 25 MB.
pub const MAX: usize = 25 << 20;

/// A new artifact: a file's bytes, or a link.
pub struct New {
    /// What it is, for people: "Round 3 · the checked review".
    pub title: String,
    /// The file's name (`report.md`), or the link's.
    pub name: String,
    pub bytes: Option<Vec<u8>>,
    pub url: Option<String>,
    /// `review`, `build`, `agent`, `turn`, `you`.
    pub source: String,
    /// The heading it's listed under: "Round 3", "Plan".
    pub group: Option<String>,
    /// More about it: `round`, `role`, `reviewer`.
    pub extra: Value,
}

/// A file name, as kept: its last path part, no control characters, at
/// most 120 characters.
pub fn clean_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let s: String = base.chars().filter(|c| !c.is_control()).take(120).collect();
    let s = s.trim().trim_start_matches('.').to_string();
    if s.is_empty() { "file".into() } else { s }
}

/// The media type a file is served as, from its name. Anything unknown is
/// bytes to download.
pub fn media_type(name: &str) -> &'static str {
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "md" | "markdown" => "text/markdown",
        "txt" | "log" | "diff" | "patch" | "csv" | "tsv" | "rs" | "ts" | "tsx" | "js" | "py" | "go" | "sh" | "toml" | "yaml" | "yml" | "lua" | "sql" => "text/plain",
        "json" | "jsonl" | "ndjson" => "application/json",
        "html" | "htm" => "text/html",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "pdf" => "application/pdf",
        _ => "application/octet-stream",
    }
}

/// The links an agent's text names that are artifacts: Claude's
/// (`claude.ai/artifact/…`, `claude.ai/code/artifact/…`, public ones).
pub fn links_in(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut from = 0;
    while let Some(at) = text[from..].find("https://claude.ai/") {
        let start = from + at;
        let end = text[start..].find(|c: char| c.is_whitespace() || matches!(c, ')' | '>' | '"' | '\'' | ']' | '`' | '<')).map_or(text.len(), |n| start + n);
        let url = text[start..end].trim_end_matches(['.', ',', ';', ':']);
        let path = &url["https://claude.ai/".len()..];
        let artifact = path.starts_with("artifact/") || path.starts_with("code/artifact/") || path.starts_with("public/artifacts/");
        if artifact && path.split('/').nth(if path.starts_with("artifact/") { 1 } else { 2 }).is_some_and(|id| !id.is_empty()) && !out.iter().any(|u| u == url) {
            out.push(url.to_string());
        }
        from = end.max(start + 1);
    }
    out
}

/// `extra`'s image fields (`crate::images`): an image kept again with
/// another role, caption, pins or reference is its own artifact, so what
/// was said about it each time is kept.
fn existing(core: &Core, docket: &str, digest: Option<&str>, url: Option<&str>, name: &str, extra: &Value) -> Result<Option<i64>> {
    const IMAGE: [&str; 4] = ["image", "caption", "pins", "against"];
    for s in core.store.entries(docket)? {
        let Ok(e) = s.entry() else { continue };
        if e.kind != "artifact" {
            continue;
        }
        let same = match (digest, url) {
            (Some(d), _) => e.data["digest"].as_str() == Some(d) && e.data["name"].as_str() == Some(name) && IMAGE.iter().all(|k| e.data[*k] == extra[*k]),
            (None, Some(u)) => e.data["url"].as_str() == Some(u),
            _ => false,
        };
        if same {
            return Ok(Some(s.seq));
        }
    }
    Ok(None)
}

/// Keep an artifact on a docket. The same file (by name and bytes) or link
/// kept twice is one artifact: its first entry's number comes back.
pub fn attach(core: &mut Core, docket: &str, who: &Who, a: New) -> Result<(i64, bool)> {
    let name = clean_name(&a.name);
    let mut data = json!({ "name": name, "source": a.source });
    if let Some(g) = &a.group {
        data["group"] = json!(g);
    }
    if let Some(m) = a.extra.as_object() {
        for (k, v) in m {
            data[k] = v.clone();
        }
    }
    let mut blobs = Vec::new();
    match (a.bytes, a.url) {
        (Some(bytes), _) => {
            if bytes.len() > MAX {
                bail!("{name} is {} MB; artifacts are at most {} MB", bytes.len() >> 20, MAX >> 20);
            }
            let hex = hex::encode(&Sha256::digest(&bytes));
            let digest = format!("sha256:{hex}");
            if let Some(seq) = existing(core, docket, Some(&digest), None, &name, &a.extra)? {
                return Ok((seq, false));
            }
            core.store.put_blob(&hex, &bytes, &crate::time::now_ms())?;
            data["digest"] = json!(digest);
            data["size"] = json!(bytes.len());
            data["media_type"] = json!(media_type(&name));
            blobs.push(digest);
        }
        (None, Some(url)) => {
            if !url.starts_with("https://") {
                bail!("a linked artifact is an https:// link");
            }
            if let Some(seq) = existing(core, docket, None, Some(&url), &name, &a.extra)? {
                return Ok((seq, false));
            }
            if url.starts_with("https://claude.ai/") {
                data["provider"] = json!("claude");
            }
            data["url"] = json!(url);
        }
        (None, None) => bail!("an artifact is a file or a link"),
    }
    let s = core.append_with(docket, "artifact", who, Some(a.title), data, blobs)?;
    Ok((s.seq, true))
}

/// A docket's artifacts, newest first, as the app lists them.
pub fn list(core: &Core, docket: &str) -> Result<Vec<Value>> {
    let mut out = Vec::new();
    for s in core.store.entries(docket)? {
        let e = s.entry()?;
        if e.kind != "artifact" {
            continue;
        }
        out.push(json!({
            "seq": s.seq, "id": s.id, "at": e.at, "actor": e.actor, "mark": crate::record::mark(&e),
            "title": e.body, "name": e.data["name"], "media_type": e.data["media_type"], "size": e.data["size"],
            "digest": e.data["digest"], "url": e.data["url"], "provider": e.data["provider"], "source": e.data["source"],
            "group": e.data["group"], "round": e.data["round"], "role": e.data["role"], "reviewer": e.data["reviewer"],
            "image": e.data["image"], "caption": e.data["caption"], "width": e.data["width"], "height": e.data["height"],
            "pins": e.data["pins"], "original": e.data["original"], "against": e.data["against"],
        }));
    }
    out.reverse();
    Ok(out)
}

/// Keep the Claude artifacts an agent's text links to.
pub fn keep_links(core: &mut Core, docket: &str, text: &str) -> Result<usize> {
    let mut n = 0;
    for url in links_in(text) {
        let id = url.rsplit('/').find(|p| !p.is_empty()).unwrap_or("artifact");
        let (_, new) = attach(
            core,
            docket,
            &Who::Hub,
            New { title: "A Claude artifact".into(), name: id.to_string(), bytes: None, url: Some(url), source: "turn".into(), group: Some("Linked".into()), extra: json!({}) },
        )?;
        n += usize::from(new);
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hub::test_core;

    fn file(title: &str, name: &str, bytes: &[u8]) -> New {
        New { title: title.into(), name: name.into(), bytes: Some(bytes.to_vec()), url: None, source: "review".into(), group: Some("Round 1".into()), extra: json!({ "round": 1 }) }
    }

    #[test]
    fn a_kept_file_is_pinned_by_the_record() {
        let mut core = test_core();
        let d = core.open(&Who::Owner, "Review o/api#3", json!({})).unwrap();
        let (seq, new) = attach(&mut core, &d, &Who::Hub, file("Round 1 · claude's report", "../.tmp/rounds/1/report.md", b"# Report\nAll good.")).unwrap();
        assert!(new);
        // The same file again: the same artifact.
        assert_eq!(attach(&mut core, &d, &Who::Hub, file("again", "report.md", b"# Report\nAll good.")).unwrap(), (seq, false));
        let l = list(&core, &d).unwrap();
        assert_eq!(l.len(), 1);
        assert_eq!(l[0]["name"], "report.md");
        assert_eq!(l[0]["media_type"], "text/markdown");
        assert_eq!(l[0]["round"], 1);
        let digest = l[0]["digest"].as_str().unwrap().to_string();
        let e = core.store.entries(&d).unwrap().last().unwrap().entry().unwrap();
        assert_eq!(e.blobs, std::slice::from_ref(&digest));
        assert_eq!(core.store.blob(&digest[7..]).unwrap().unwrap(), b"# Report\nAll good.");
        assert!(core.verify_full(&d).unwrap().ok);
        assert!(core.rebuild_check().unwrap().is_empty());

        // A changed file fails the full check; a lost one fails both.
        core.store.conn_for_tests().execute("UPDATE blobs SET bytes = x'00'", []).unwrap();
        let r = core.verify_full(&d).unwrap();
        assert!(!r.ok && r.problems[0].contains("was changed"), "{:?}", r.problems);
        assert!(core.verify(&d).unwrap().ok, "the quick check only looks for the file, and is kept until the record moves");
        core.store.conn_for_tests().execute("DELETE FROM blobs", []).unwrap();
        assert!(core.verify_full(&d).unwrap().problems[0].contains("is missing"));
        // The record moves: the quick check looks again, and sees it gone.
        core.append(&d, "note", &Who::Owner, Some("later".into()), json!({})).unwrap();
        assert!(core.verify(&d).unwrap().problems[0].contains("is missing"));
    }

    #[test]
    fn links_and_limits() {
        let mut core = test_core();
        let d = core.open(&Who::Owner, "Design", json!({})).unwrap();
        let text = "Published: https://claude.ai/artifact/1Pk8XW49eoekg2gp5aGpfc. Also https://claude.ai/code/artifact/abc-123 and (https://claude.ai/public/artifacts/xyz). Not https://claude.ai/chat/1 or https://claude.ai/artifact/.";
        assert_eq!(links_in(text), ["https://claude.ai/artifact/1Pk8XW49eoekg2gp5aGpfc", "https://claude.ai/code/artifact/abc-123", "https://claude.ai/public/artifacts/xyz"]);
        assert_eq!(keep_links(&mut core, &d, text).unwrap(), 3);
        assert_eq!(keep_links(&mut core, &d, text).unwrap(), 0, "kept once");
        let l = list(&core, &d).unwrap();
        assert_eq!(l[2]["provider"], "claude");
        assert_eq!(l[2]["url"], "https://claude.ai/artifact/1Pk8XW49eoekg2gp5aGpfc");
        let big = New { bytes: Some(vec![0; MAX + 1]), ..file("big", "big.bin", b"") };
        assert!(attach(&mut core, &d, &Who::Hub, big).unwrap_err().to_string().contains("at most 25 MB"));
        let http = New { bytes: None, url: Some("http://x".into()), ..file("x", "x", b"") };
        assert!(attach(&mut core, &d, &Who::Hub, http).is_err());
        assert_eq!(clean_name("../../etc/passwd"), "passwd");
        assert_eq!(clean_name(".env"), "env");
        assert_eq!(media_type("Plan.MD"), "text/markdown");
        assert_eq!(media_type("x.exe"), "application/octet-stream");
    }
}
