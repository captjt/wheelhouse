//! Images you share with a docket's agents: what something should look
//! like, an inspiration, a screenshot you marked up.
//!
//! An image comes in two steps. The app stages its bytes (`stage`: checked
//! to be a PNG, JPEG, GIF or WebP, never SVG, and put in the blob store by
//! its hash), then sends them with what you typed (`keep`): each one is an
//! artifact on the docket's record, under References, with its caption and
//! the pins you numbered on it. A marked-up image keeps the original beside
//! it.
//!
//! Agents see an image as a file: `materialize` writes it under
//! `<state>/images/<docket>/`, and `block` is the text that goes with a
//! prompt or a message, with each one's path. Claude Code opens it with
//! Read, Codex with view_image, Pi with read. The path is filled in by the
//! hub that hands the text to the agent, so a teammate's agent gets one on
//! its own Mac.

use anyhow::{Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

use sha2::Digest as _;

use crate::hub::{Core, Who};
use crate::record::hex;

/// The largest image: 15 MB. A screenshot at 2x is a few.
pub const MAX: usize = 15 << 20;
/// The most images sent at once.
pub const MAX_PER: usize = 10;
/// The most pixels on a side, and in all: a header claiming more is refused
/// before anything decodes it.
const MAX_SIDE: u32 = 20_000;
const MAX_PIXELS: u64 = 100_000_000;
/// A caption, and a pin's note, are this long at most.
pub const MAX_CAPTION: usize = 500;
/// The most pins on one image.
pub const MAX_PINS: usize = 40;

/// What an image is, from its own bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kind {
    pub media_type: &'static str,
    pub ext: &'static str,
    pub width: u32,
    pub height: u32,
}

/// Read an image's type and size from its header. Anything else (SVG, which
/// can script, HTML, a PDF) is refused, as is a header that claims more
/// pixels than any screenshot has.
pub fn sniff(b: &[u8]) -> Result<Kind> {
    let (media_type, ext, (width, height)) = if b.starts_with(b"\x89PNG\r\n\x1a\n") {
        ("image/png", "png", png(b)?)
    } else if b.starts_with(&[0xFF, 0xD8, 0xFF]) {
        ("image/jpeg", "jpg", jpeg(b)?)
    } else if b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a") {
        ("image/gif", "gif", gif(b)?)
    } else if b.len() >= 12 && &b[..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        ("image/webp", "webp", webp(b)?)
    } else {
        bail!("that isn't a PNG, JPEG, GIF or WebP image");
    };
    if width == 0 || height == 0 {
        bail!("that image has no pixels");
    }
    if width > MAX_SIDE || height > MAX_SIDE || u64::from(width) * u64::from(height) > MAX_PIXELS {
        bail!("that image is {width}×{height}: at most {MAX_SIDE} on a side and {} megapixels", MAX_PIXELS / 1_000_000);
    }
    Ok(Kind { media_type, ext, width, height })
}

fn be16(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from(u16::from_be_bytes(b.get(at..at + 2)?.try_into().ok()?)))
}
fn le16(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?)))
}
fn le24(b: &[u8], at: usize) -> Option<u32> {
    let s = b.get(at..at + 3)?;
    Some(u32::from(s[0]) | u32::from(s[1]) << 8 | u32::from(s[2]) << 16)
}

fn png(b: &[u8]) -> Result<(u32, u32)> {
    // The first chunk is IHDR: its width and height, big-endian.
    if b.get(12..16) != Some(b"IHDR") {
        bail!("that PNG has no header");
    }
    let w = u32::from_be_bytes(b.get(16..20).ok_or_else(|| anyhow!("that PNG is cut short"))?.try_into()?);
    let h = u32::from_be_bytes(b.get(20..24).ok_or_else(|| anyhow!("that PNG is cut short"))?.try_into()?);
    Ok((w, h))
}

fn gif(b: &[u8]) -> Result<(u32, u32)> {
    Ok((le16(b, 6).ok_or_else(|| anyhow!("that GIF is cut short"))?, le16(b, 8).ok_or_else(|| anyhow!("that GIF is cut short"))?))
}

fn jpeg(b: &[u8]) -> Result<(u32, u32)> {
    // Walk the segments to the first start-of-frame, which has the size.
    let mut i = 2;
    while i + 4 <= b.len() {
        if b[i] != 0xFF {
            bail!("that JPEG is damaged");
        }
        let marker = b[i + 1];
        // Fill bytes, and markers with no length.
        if marker == 0xFF {
            i += 1;
            continue;
        }
        if marker == 0x01 || (0xD0..=0xD7).contains(&marker) {
            i += 2;
            continue;
        }
        let len = be16(b, i + 2).ok_or_else(|| anyhow!("that JPEG is cut short"))? as usize;
        if len < 2 {
            bail!("that JPEG is damaged");
        }
        let sof = matches!(marker, 0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF);
        if sof {
            let h = be16(b, i + 5).ok_or_else(|| anyhow!("that JPEG is cut short"))?;
            let w = be16(b, i + 7).ok_or_else(|| anyhow!("that JPEG is cut short"))?;
            return Ok((w, h));
        }
        if marker == 0xDA || marker == 0xD9 {
            break;
        }
        i += 2 + len;
    }
    bail!("that JPEG has no size in it")
}

fn webp(b: &[u8]) -> Result<(u32, u32)> {
    let short = || anyhow!("that WebP is cut short");
    match b.get(12..16) {
        Some(b"VP8 ") => {
            // A key frame's start code, then 14-bit sizes.
            if b.get(23..26) != Some(&[0x9D, 0x01, 0x2A]) {
                bail!("that WebP is damaged");
            }
            Ok((le16(b, 26).ok_or_else(short)? & 0x3FFF, le16(b, 28).ok_or_else(short)? & 0x3FFF))
        }
        Some(b"VP8L") => {
            if b.get(20) != Some(&0x2F) {
                bail!("that WebP is damaged");
            }
            let s = b.get(21..25).ok_or_else(short)?;
            let bits = u32::from_le_bytes(s.try_into()?);
            Ok(((bits & 0x3FFF) + 1, ((bits >> 14) & 0x3FFF) + 1))
        }
        Some(b"VP8X") => Ok((le24(b, 24).ok_or_else(short)? + 1, le24(b, 27).ok_or_else(short)? + 1)),
        _ => bail!("that WebP is damaged"),
    }
}

/// An image's name, as kept: cleaned, and ending the way its bytes say.
pub fn name_for(name: &str, k: &Kind) -> String {
    let base = crate::artifacts::clean_name(name);
    let stem = match base.rsplit_once('.') {
        Some((s, _)) if !s.is_empty() => s.to_string(),
        _ => base,
    };
    let stem: String = stem.chars().map(|c| if c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | ' ') { c } else { '-' }).take(80).collect();
    let stem = stem.trim().replace(' ', "-");
    format!("{}.{}", if stem.is_empty() || stem == "file" { "image" } else { &stem }, k.ext)
}

/// A staged image: in the blob store, on no record yet.
#[derive(Debug, Clone, Serialize)]
pub struct Staged {
    pub digest: String,
    pub name: String,
    pub media_type: &'static str,
    pub width: u32,
    pub height: u32,
    pub size: usize,
}

/// Put an image's bytes in the blob store, checked, for `keep` to name.
pub fn stage(core: &mut Core, name: &str, bytes: &[u8]) -> Result<Staged> {
    if bytes.len() > MAX {
        bail!("that image is {} MB; an image is at most {} MB", bytes.len() >> 20, MAX >> 20);
    }
    let k = sniff(bytes)?;
    let hex = hex::encode(&sha2::Sha256::digest(bytes));
    core.store.put_blob(&hex, bytes, &crate::time::now_ms())?;
    Ok(Staged { digest: format!("sha256:{hex}"), name: name_for(name, &k), media_type: k.media_type, width: k.width, height: k.height, size: bytes.len() })
}

/// A numbered pin on a marked-up image, and what it says.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pin {
    pub n: u32,
    #[serde(default)]
    pub note: String,
}

/// An image to send, as the app names it: a staged digest, and what you
/// said about it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Ref {
    pub digest: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub caption: String,
    /// The image before you marked it up, staged too.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pins: Vec<Pin>,
}

/// An image kept on a docket: what goes in an entry's `images`, and what
/// `block` writes out for an agent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Kept {
    /// Its artifact's entry.
    pub seq: i64,
    pub digest: String,
    pub name: String,
    #[serde(default)]
    pub caption: String,
    #[serde(default)]
    pub width: u32,
    #[serde(default)]
    pub height: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pins: Vec<Pin>,
}

fn hex_of(digest: &str) -> Result<String> {
    let h = digest.strip_prefix("sha256:").unwrap_or(digest).to_ascii_lowercase();
    if h.len() != 64 || !h.bytes().all(|b| b.is_ascii_hexdigit()) {
        bail!("{digest} isn't a sha256 digest");
    }
    Ok(h)
}

/// One line: no control characters, runs of space as one, at most `max`.
fn line(s: &str, max: usize) -> String {
    let s: String = s.split_whitespace().collect::<Vec<_>>().join(" ").chars().filter(|c| !c.is_control()).collect();
    if s.chars().count() > max { format!("{}…", s.chars().take(max - 1).collect::<String>()) } else { s }
}

/// Check what the app sent, before anything is kept: at most `MAX_PER`,
/// each a staged image, its words cleaned.
pub fn check(core: &Core, refs: &[Ref]) -> Result<Vec<(Ref, Kind)>> {
    if refs.len() > MAX_PER {
        bail!("{} images: send at most {MAX_PER} at once", refs.len());
    }
    let mut out = Vec::new();
    for r in refs {
        let hex = hex_of(&r.digest)?;
        let bytes = core.store.blob(&hex)?.ok_or_else(|| anyhow!("the image {} isn't here: add it again", &hex[..12]))?;
        let k = sniff(&bytes)?;
        if let Some(o) = &r.original {
            let oh = hex_of(o)?;
            let ob = core.store.blob(&oh)?.ok_or_else(|| anyhow!("the original of {} isn't here: add it again", r.name))?;
            sniff(&ob)?;
        }
        if r.pins.len() > MAX_PINS {
            bail!("{} pins on one image: at most {MAX_PINS}", r.pins.len());
        }
        let pins = r.pins.iter().map(|p| Pin { n: p.n, note: line(&p.note, MAX_CAPTION) }).collect();
        let name = name_for(if r.name.trim().is_empty() { "image" } else { &r.name }, &k);
        out.push((
            Ref {
                digest: format!("sha256:{hex}"),
                name,
                caption: line(&r.caption, MAX_CAPTION),
                original: r.original.as_deref().map(hex_of).transpose()?.map(|h| format!("sha256:{h}")),
                pins,
            },
            k,
        ));
    }
    Ok(out)
}

/// Keep checked images on a docket as References: each an artifact, the
/// original of a marked-up one beside it. The same image kept twice is one
/// artifact. Returns them in order.
pub fn keep(core: &mut Core, docket: &str, who: &Who, checked: Vec<(Ref, Kind)>) -> Result<Vec<Kept>> {
    let source = if matches!(who, Who::Agent { .. }) { "agent" } else { "you" };
    let mut out = Vec::new();
    for (r, k) in checked {
        let mut original_seq = None;
        if let Some(o) = &r.original {
            let bytes = core.store.blob(&hex_of(o)?)?.ok_or_else(|| anyhow!("the original of {} isn't here", r.name))?;
            let (seq, _) = crate::artifacts::attach(
                core,
                docket,
                who,
                crate::artifacts::New {
                    title: format!("{} (before markup)", if r.caption.is_empty() { &r.name } else { &r.caption }),
                    name: format!("original-{}", r.name),
                    bytes: Some(bytes),
                    url: None,
                    source: source.into(),
                    group: Some("References".into()),
                    extra: json!({ "image": "original" }),
                },
            )?;
            original_seq = Some(seq);
        }
        let bytes = core.store.blob(&hex_of(&r.digest)?)?.ok_or_else(|| anyhow!("the image {} isn't here", r.name))?;
        let mut extra = json!({ "image": "reference", "width": k.width, "height": k.height });
        if !r.caption.is_empty() {
            extra["caption"] = json!(r.caption);
        }
        if !r.pins.is_empty() {
            extra["pins"] = json!(r.pins);
        }
        if let Some(s) = original_seq {
            extra["original"] = json!(s);
        }
        let (seq, _) = crate::artifacts::attach(
            core,
            docket,
            who,
            crate::artifacts::New {
                title: if r.caption.is_empty() { r.name.clone() } else { line(&r.caption, 120) },
                name: r.name.clone(),
                bytes: Some(bytes),
                url: None,
                source: source.into(),
                group: Some("References".into()),
                extra,
            },
        )?;
        out.push(Kept { seq, digest: r.digest, name: r.name, caption: r.caption, width: k.width, height: k.height, pins: r.pins });
    }
    Ok(out)
}

/// An image's file for agents: `<base>/<docket>/<first 12 of its hash>-<name>`.
pub fn path_in(base: &Path, docket: &str, digest: &str, name: &str) -> Result<PathBuf> {
    let hex = hex_of(digest)?;
    let d: String = docket.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').collect();
    if d.is_empty() {
        bail!("no docket to keep the image under");
    }
    let name = crate::artifacts::clean_name(name);
    Ok(base.join(d).join(format!("{}-{}", &hex[..12], name)))
}

/// Write an image where agents can open it, unless it's there already with
/// the same bytes. Only you can read it (0600, in a 0700 folder).
pub fn materialize_in(core: &Core, base: &Path, docket: &str, digest: &str, name: &str) -> Result<PathBuf> {
    let path = path_in(base, docket, digest, name)?;
    let hex = hex_of(digest)?;
    if let Ok(have) = std::fs::read(&path)
        && hex::encode(&sha2::Sha256::digest(&have)) == hex
    {
        return Ok(path);
    }
    let bytes = core.store.blob(&hex)?.ok_or_else(|| anyhow!("this hub doesn't have the image {}", &hex[..12]))?;
    let parent = path.parent().ok_or_else(|| anyhow!("no folder for {}", path.display()))?;
    std::fs::create_dir_all(parent)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for d in [base, parent] {
            let _ = std::fs::set_permissions(d, std::fs::Permissions::from_mode(0o700));
        }
    }
    // Written beside, then renamed: an agent never opens half an image.
    let tmp = parent.join(format!(".{}.{}.tmp", &hex[..12], std::process::id()));
    {
        use std::io::Write;
        let mut o = std::fs::OpenOptions::new();
        o.write(true).create(true).truncate(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
        let mut f = o.open(&tmp)?;
        f.write_all(&bytes)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, &path)?;
    Ok(path)
}

pub fn materialize(core: &Core, docket: &str, digest: &str, name: &str) -> Result<PathBuf> {
    materialize_in(core, &core.images_dir, docket, digest, name)
}

/// The text that goes with a prompt or a message: each image's path, so the
/// agent opens it, its caption and its pins. An image this hub can't write
/// out is named, with `wh images` to fetch it.
pub fn block_in(core: &Core, base: &Path, docket: &str, images: &[Kept]) -> String {
    if images.is_empty() {
        return String::new();
    }
    let n = images.len();
    let mut out = format!(
        "{} (open {} to see {}: Read in Claude Code, view_image in Codex):\n",
        if n == 1 { "An image comes with this" } else { "Images come with this" },
        if n == 1 { "it" } else { "each one" },
        if n == 1 { "it" } else { "them" },
    );
    for (i, im) in images.iter().enumerate() {
        let at = match materialize_in(core, base, docket, &im.digest, &im.name) {
            Ok(p) => p.display().to_string(),
            Err(_) => format!("(not on this Mac yet: `wh images {docket}` fetches it)"),
        };
        let size = if im.width > 0 { format!(" ({}×{})", im.width, im.height) } else { String::new() };
        out.push_str(&format!("{}. {at}{size}", i + 1));
        if !im.caption.is_empty() {
            out.push_str(&format!(": {}", line(&im.caption, MAX_CAPTION)));
        }
        out.push('\n');
        if !im.pins.is_empty() {
            let pins: Vec<String> = im.pins.iter().map(|p| if p.note.is_empty() { format!("{}", p.n) } else { format!("{}. {}", p.n, line(&p.note, MAX_CAPTION)) }).collect();
            out.push_str(&format!("   Numbered pins on it: {}\n", pins.join("; ")));
        }
    }
    out
}

pub fn block(core: &Core, docket: &str, images: &[Kept]) -> String {
    block_in(core, &core.images_dir, docket, images)
}

/// The images an entry carries (`data.images`), as kept.
pub fn of_entry(data: &Value) -> Vec<Kept> {
    data.get("images").and_then(|v| serde_json::from_value::<Vec<Kept>>(v.clone()).ok()).unwrap_or_default()
}

/// Text, then the block for its images.
pub fn with_block(text: &str, block: &str) -> String {
    if block.is_empty() { text.to_string() } else { format!("{}\n\n{}", text.trim_end(), block.trim_end()) }
}

/// A docket's images: the References you shared, the screenshots agents
/// attached, and the intent's, newest first, each with whatever it was
/// kept against.
pub fn list(core: &Core, docket: &str) -> Result<Vec<Value>> {
    let mut out = Vec::new();
    for a in crate::artifacts::list(core, docket)? {
        if !a["media_type"].as_str().is_some_and(|t| t.starts_with("image/") && t != "image/svg+xml") {
            continue;
        }
        out.push(a);
    }
    Ok(out)
}

/// The image links an intent's Markdown names (`/api/blobs/sha256:…`), with
/// their alt text: the app puts them there when you paste an image into it.
pub fn in_markdown(md: &str) -> Vec<(String, String, String)> {
    let mut out: Vec<(String, String, String)> = Vec::new();
    let mut from = 0;
    while let Some(at) = md[from..].find("![") {
        let start = from + at;
        from = start + 2;
        let Some(close) = md[start + 2..].find("](") else { continue };
        let alt = &md[start + 2..start + 2 + close];
        if alt.contains('\n') {
            continue;
        }
        let url_at = start + 2 + close + 2;
        let Some(end) = md[url_at..].find(')') else { continue };
        let url = md[url_at..url_at + end].trim();
        let url = url.split_whitespace().next().unwrap_or("");
        let Some(rest) = url.strip_prefix("/api/blobs/") else { continue };
        let (digest, query) = rest.split_once('?').unwrap_or((rest, ""));
        let Ok(hex) = hex_of(digest) else { continue };
        let name = query.split('&').find_map(|kv| kv.strip_prefix("name=")).map(pct_decode).unwrap_or_else(|| "image".into());
        if !out.iter().any(|(d, ..)| d.ends_with(&hex)) {
            out.push((format!("sha256:{hex}"), crate::artifacts::clean_name(&name), line(alt, MAX_CAPTION)));
        }
        from = url_at + end;
    }
    out
}

fn pct_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && let Some(v) = s.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(if b[i] == b'+' { b' ' } else { b[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// An intent's Markdown for an agent: each image link it has becomes the
/// image's path on this Mac, so `wh brief` shows the mockups as files.
pub fn intent_for_agent(core: &Core, base: &Path, docket: &str, md: &str) -> String {
    let mut out = md.to_string();
    for (digest, name, _) in in_markdown(md) {
        let hex = digest.trim_start_matches("sha256:");
        let Ok(p) = materialize_in(core, base, docket, &digest, &name) else { continue };
        // Every spelling of this image's link: with or without a query.
        let mut i = 0;
        while let Some(at) = out[i..].find(&format!("/api/blobs/sha256:{hex}")) {
            let s = i + at;
            let e = out[s..].find([')', ' ']).map_or(out.len(), |n| s + n);
            let path = p.display().to_string();
            out.replace_range(s..e, &path);
            i = s + path.len();
        }
    }
    out
}

/// Keep the images an intent names that aren't on the docket yet, as
/// References under "Intent". Returns their digests, for the intent's
/// entry to pin.
pub fn keep_intent(core: &mut Core, docket: &str, who: &Who, md: &str) -> Result<Vec<String>> {
    let mut digests = Vec::new();
    for (digest, name, alt) in in_markdown(md) {
        let hex = hex_of(&digest)?;
        let Some(bytes) = core.store.blob(&hex)? else { bail!("the intent names an image this hub doesn't have ({}): paste it again", &hex[..12]) };
        let k = sniff(&bytes)?;
        let name = name_for(&name, &k);
        let mut extra = json!({ "image": "intent", "width": k.width, "height": k.height });
        if !alt.is_empty() {
            extra["caption"] = json!(alt);
        }
        crate::artifacts::attach(
            core,
            docket,
            who,
            crate::artifacts::New {
                title: if alt.is_empty() { name.clone() } else { line(&alt, 120) },
                name,
                bytes: Some(bytes),
                url: None,
                source: if matches!(who, Who::Agent { .. }) { "agent".into() } else { "you".into() },
                group: Some("Intent".into()),
                extra,
            },
        )?;
        digests.push(digest);
    }
    Ok(digests)
}

/// The References on a docket, for its brief: what you shared and the
/// intent's, newest first, each written out where an agent can open it.
pub fn brief_section(core: &Core, base: &Path, docket: &str) -> Result<String> {
    let mut lines = Vec::new();
    let all = list(core, docket)?;
    let refs: Vec<&Value> = all.iter().filter(|a| matches!(a["image"].as_str(), Some("reference" | "intent"))).collect();
    for a in refs.iter().take(12) {
        let (Some(d), Some(n)) = (a["digest"].as_str(), a["name"].as_str()) else { continue };
        let at = match materialize_in(core, base, docket, d, n) {
            Ok(p) => p.display().to_string(),
            Err(_) => format!("(not on this Mac: `wh images {docket}`)"),
        };
        let mut l = format!("- {at}");
        if let Some(c) = a["caption"].as_str() {
            l.push_str(&format!(": {}", line(c, MAX_CAPTION)));
        }
        let by = if a["source"] == "agent" { "an agent" } else { "you" };
        l.push_str(&format!(" ({by}, #{}{})", a["seq"], if a["image"] == "intent" { ", in the intent" } else { "" }));
        if let Some(pins) = a["pins"].as_array().filter(|p| !p.is_empty()) {
            let ps: Vec<String> = pins.iter().map(|p| format!("{}. {}", p["n"], p["note"].as_str().unwrap_or(""))).collect();
            l.push_str(&format!("\n  Numbered pins on it: {}", ps.join("; ")));
        }
        lines.push(l);
    }
    if lines.is_empty() {
        return Ok(String::new());
    }
    let more = if refs.len() > lines.len() { format!("\n\n{} more: `wh images {docket}`.", refs.len() - lines.len()) } else { String::new() };
    Ok(format!(
        "\n## Images\n\nWhat the work should look like, and what inspired it (newest first). Open each one to see it. To show yours against one, `wh attach shot.png --against <#>`.\n\n{}{more}\n",
        lines.join("\n")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hub::test_core;

    /// A real 3×2 PNG.
    pub fn png_bytes(w: u32, h: u32) -> Vec<u8> {
        let mut b = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        b.extend(w.to_be_bytes());
        b.extend(h.to_be_bytes());
        b.extend([8, 6, 0, 0, 0, 0, 0, 0, 0]);
        b
    }

    fn jpeg_bytes(w: u16, h: u16) -> Vec<u8> {
        let mut b = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
        b.extend(b"JFIF\0\x01\x01\0\0\x01\0\x01\0\0");
        b.extend([0xFF, 0xC0, 0x00, 0x11, 0x08]);
        b.extend(h.to_be_bytes());
        b.extend(w.to_be_bytes());
        b.extend([0x03, 0x01, 0x22, 0x00, 0x02, 0x11, 0x01, 0x03, 0x11, 0x01]);
        b
    }

    #[test]
    fn images_are_known_by_their_bytes_not_their_names() {
        assert_eq!(sniff(&png_bytes(1440, 900)).unwrap(), Kind { media_type: "image/png", ext: "png", width: 1440, height: 900 });
        assert_eq!(sniff(&jpeg_bytes(640, 480)).unwrap().width, 640);
        assert_eq!(sniff(&jpeg_bytes(640, 480)).unwrap().height, 480);
        let mut gif = b"GIF89a".to_vec();
        gif.extend([0x20, 0x00, 0x10, 0x00]);
        assert_eq!((sniff(&gif).unwrap().width, sniff(&gif).unwrap().height), (32, 16));
        let mut webp = b"RIFF\0\0\0\0WEBPVP8X\x0a\0\0\0\0\0\0\0".to_vec();
        webp.extend([0x7F, 0x02, 0x00, 0xDF, 0x01, 0x00]);
        assert_eq!((sniff(&webp).unwrap().width, sniff(&webp).unwrap().height), (640, 480));
        // SVG can script; HTML named .png is still HTML.
        assert!(sniff(b"<svg xmlns='http://www.w3.org/2000/svg'><script>alert(1)</script></svg>").is_err());
        assert!(sniff(b"<html><body>hi</body></html>").is_err());
        // Cut short, damaged, or a bomb: refused before anything decodes it.
        assert!(sniff(&png_bytes(1, 1)[..18]).is_err());
        assert!(sniff(&png_bytes(0, 10)).is_err());
        assert!(sniff(&png_bytes(30_000, 10)).is_err());
        assert!(sniff(&png_bytes(15_000, 15_000)).is_err());
        assert!(sniff(&[0xFF, 0xD8, 0xFF, 0x00]).is_err());
        assert!(sniff(&[0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x00, 0x00]).is_err());
        assert!(sniff(b"RIFF\0\0\0\0WEBPVP8 ").is_err());
    }

    #[test]
    fn a_name_ends_the_way_the_bytes_say() {
        let k = sniff(&png_bytes(2, 2)).unwrap();
        assert_eq!(name_for("Screenshot 2026-11-27 at 9.41.03.jpg", &k), "Screenshot-2026-11-27-at-9.41.03.png");
        assert_eq!(name_for("../../etc/passwd", &k), "passwd.png");
        assert_eq!(name_for("", &k), "image.png");
        assert_eq!(name_for("a\u{202e}gnp.exe", &k), "a-gnp.png");
    }

    #[test]
    fn a_staged_image_is_kept_with_its_caption_and_pins_and_its_original() {
        let mut core = test_core();
        let d = core.open(&Who::Owner, "Settings page", json!({})).unwrap();
        let marked = stage(&mut core, "shot.png", &png_bytes(1440, 900)).unwrap();
        let before = stage(&mut core, "shot.png", &png_bytes(1440, 901)).unwrap();
        assert!(stage(&mut core, "x.svg", b"<svg/>").is_err());
        let r = Ref {
            digest: marked.digest.clone(),
            name: "shot.png".into(),
            caption: "  The header,\nlike this  ".into(),
            original: Some(before.digest.clone()),
            pins: vec![Pin { n: 1, note: "Make this primary".into() }],
        };
        let checked = check(&core, &[r]).unwrap();
        let kept = keep(&mut core, &d, &Who::Owner, checked).unwrap();
        assert_eq!(kept[0].caption, "The header, like this");
        let all = list(&core, &d).unwrap();
        assert_eq!(all.len(), 2, "the image and its original");
        let a = all.iter().find(|a| a["seq"] == kept[0].seq).unwrap();
        assert_eq!(a["group"], "References");
        assert_eq!(a["source"], "you");
        // Too many, an unknown digest, or not an image: refused before anything is kept.
        assert!(check(&core, &vec![Ref { digest: marked.digest.clone(), ..Default::default() }; MAX_PER + 1]).is_err());
        assert!(check(&core, &[Ref { digest: format!("sha256:{}", "0".repeat(64)), ..Default::default() }]).is_err());
        assert!(check(&core, &[Ref { digest: "sha256:nothex".into(), ..Default::default() }]).is_err());
    }

    #[test]
    fn agents_get_a_file_they_can_open_and_the_words_with_it() {
        let mut core = test_core();
        let d = core.open(&Who::Owner, "Settings page", json!({})).unwrap();
        let s = stage(&mut core, "mock.png", &png_bytes(800, 600)).unwrap();
        let checked =
            check(&core, &[Ref { digest: s.digest.clone(), name: s.name, caption: "Like this".into(), original: None, pins: vec![Pin { n: 1, note: "Bigger".into() }] }]).unwrap();
        let kept = keep(&mut core, &d, &Who::Owner, checked).unwrap();
        let tmp = std::env::temp_dir().join(format!("wh-images-{}", std::process::id()));
        let b = block_in(&core, &tmp, &d, &kept);
        let path = materialize_in(&core, &tmp, &d, &s.digest, "mock.png").unwrap();
        assert!(b.contains(&path.display().to_string()), "{b}");
        assert!(b.contains("(800×600): Like this"), "{b}");
        assert!(b.contains("Numbered pins on it: 1. Bigger"), "{b}");
        assert_eq!(std::fs::read(&path).unwrap(), png_bytes(800, 600));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        }
        // Changed on disk: written again from the store.
        std::fs::write(&path, b"tampered").unwrap();
        materialize_in(&core, &tmp, &d, &s.digest, "mock.png").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), png_bytes(800, 600));
        // A name can't leave the docket's folder.
        let p = path_in(&tmp, "../D-1", &s.digest, "../../x.png").unwrap();
        assert!(p.starts_with(tmp.join("D-1")), "{}", p.display());
        assert!(!p.display().to_string().contains("/../"));
        // One this hub doesn't have: named, with how to fetch it.
        let missing = Kept { seq: 9, digest: format!("sha256:{}", "a".repeat(64)), name: "x.png".into(), caption: String::new(), width: 0, height: 0, pins: vec![] };
        assert!(block_in(&core, &tmp, &d, &[missing]).contains(&format!("wh images {d}")));
        assert_eq!(block_in(&core, &tmp, &d, &[]), "");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn an_intents_images_are_kept_and_become_paths_in_the_brief() {
        let mut core = test_core();
        let d = core.open(&Who::Owner, "Settings page", json!({})).unwrap();
        let s = stage(&mut core, "mock.png", &png_bytes(10, 10)).unwrap();
        let md = format!("The page:\n\n![The settings page](/api/blobs/{}?name=settings%20mock.png)\n\nAnd ![x](https://example.com/a.png).", s.digest);
        let found = in_markdown(&md);
        assert_eq!(found, vec![(s.digest.clone(), "settings mock.png".into(), "The settings page".into())]);
        let pinned = keep_intent(&mut core, &d, &Who::Owner, &md).unwrap();
        assert_eq!(pinned, vec![s.digest.clone()]);
        let tmp = std::env::temp_dir().join(format!("wh-intent-{}", std::process::id()));
        let out = intent_for_agent(&core, &tmp, &d, &md);
        assert!(!out.contains("/api/blobs/"), "{out}");
        assert!(out.contains(&tmp.display().to_string()), "{out}");
        assert!(out.contains("https://example.com/a.png"));
        let sec = brief_section(&core, &tmp, &d).unwrap();
        assert!(sec.contains("The settings page") && sec.contains("in the intent"), "{sec}");
        // One the hub doesn't have can't be kept.
        assert!(keep_intent(&mut core, &d, &Who::Owner, &format!("![a](/api/blobs/sha256:{})", "b".repeat(64))).is_err());
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
