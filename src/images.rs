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
/// The most bytes of images staged and not sent, in all.
pub const MAX_STAGED: u64 = 1 << 30;
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
    let k = read(b)?;
    fits(&k)?;
    Ok(k)
}

/// Whether an image is small enough to go to an agent: at most `MAX_SIDE`
/// on a side and `MAX_PIXELS` in all. A file kept with `wh attach` needn't be.
pub fn fits(k: &Kind) -> Result<()> {
    let (width, height) = (k.width, k.height);
    if width > MAX_SIDE || height > MAX_SIDE || u64::from(width) * u64::from(height) > MAX_PIXELS {
        bail!("that image is {width}×{height}: at most {MAX_SIDE} on a side and {} megapixels", MAX_PIXELS / 1_000_000);
    }
    Ok(())
}

/// An image's type and size from its header, whatever its size.
pub fn read(b: &[u8]) -> Result<Kind> {
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
    // Staged and not yet sent, in all: a loop can't fill the disk before the prune.
    if core.store.staged_bytes()? + bytes.len() as u64 > MAX_STAGED {
        bail!("{} MB of images are waiting to be sent: send or drop some, or they go after three days", MAX_STAGED >> 20);
    }
    let k = sniff(bytes)?;
    let hex = hex::encode(&sha2::Sha256::digest(bytes));
    let have = core.store.has_blob(&hex)?;
    core.store.put_blob(&hex, bytes, &crate::time::now_ms())?;
    // Bytes the hub didn't have: let go of in a few days unless they're sent.
    if !have {
        core.store.stage(&hex, &crate::time::now_ms())?;
    }
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

/// An image as an entry carries it (`data.images`), checked: by its digest,
/// which the entry pins, never by an entry's number (a shared docket's
/// entries have none until the workspace orders them).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Shared {
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
    /// The digest of the image before it was marked up.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original: Option<String>,
}

/// All that's said about a message's images (captions and pin notes) is at
/// most this long: it goes into an agent's prompt with them.
pub const MAX_WORDS: usize = 4000;

fn hex_of(digest: &str) -> Result<String> {
    let h = digest.strip_prefix("sha256:").unwrap_or(digest).to_ascii_lowercase();
    if h.len() != 64 || !h.bytes().all(|b| b.is_ascii_hexdigit()) {
        bail!("{digest} isn't a sha256 digest");
    }
    Ok(h)
}

/// One line: no control characters, runs of space as one, at most `max`.
pub fn line(s: &str, max: usize) -> String {
    let s: String = s.split_whitespace().collect::<Vec<_>>().join(" ").chars().filter(|c| !c.is_control()).collect();
    if s.chars().count() > max { format!("{}…", s.chars().take(max - 1).collect::<String>()) } else { s }
}

/// A name safe to put in a path an agent reads, whoever wrote it: letters,
/// digits, `-`, `_` and `.` only (no spaces, quotes, backticks or
/// direction marks), its extension kept.
pub fn safe_name(name: &str) -> String {
    let base = crate::artifacts::clean_name(name);
    let s: String = base.chars().map(|c| if c.is_alphanumeric() || matches!(c, '-' | '_' | '.') { c } else { '-' }).take(100).collect();
    let s = s.trim_matches(['-', '.']).to_string();
    if s.is_empty() { "image".into() } else { s }
}

/// Check what the app sent, before anything is kept or sent: at most
/// `MAX_PER`, each a staged image, its words cleaned and not too many.
pub fn check(core: &Core, refs: &[Ref]) -> Result<Vec<Shared>> {
    if refs.len() > MAX_PER {
        bail!("{} images: send at most {MAX_PER} at once", refs.len());
    }
    let mut out = Vec::new();
    let mut words = 0;
    for r in refs {
        let hex = hex_of(&r.digest)?;
        let bytes = core.store.blob(&hex)?.ok_or_else(|| anyhow!("the image {} isn't here: add it again", &hex[..12]))?;
        let k = sniff(&bytes)?;
        let original = match &r.original {
            Some(o) => {
                let oh = hex_of(o)?;
                let ob = core.store.blob(&oh)?.ok_or_else(|| anyhow!("the original of {} isn't here: add it again", r.name))?;
                sniff(&ob)?;
                Some(format!("sha256:{oh}"))
            }
            None => None,
        };
        if r.pins.len() > MAX_PINS {
            bail!("{} pins on one image: at most {MAX_PINS}", r.pins.len());
        }
        let pins: Vec<Pin> = r.pins.iter().map(|p| Pin { n: p.n, note: line(&p.note, MAX_CAPTION) }).collect();
        let caption = line(&r.caption, MAX_CAPTION);
        words += caption.chars().count() + pins.iter().map(|p| p.note.chars().count()).sum::<usize>();
        if words > MAX_WORDS {
            bail!("the captions and pin notes are more than {MAX_WORDS} characters in all: say the rest in the message");
        }
        let name = name_for(if r.name.trim().is_empty() { "image" } else { &r.name }, &k);
        out.push(Shared { digest: format!("sha256:{hex}"), name, caption, width: k.width, height: k.height, pins, original });
    }
    Ok(out)
}

/// The digests an entry that carries these images pins: each image, and
/// each one's original.
pub fn digests(images: &[Shared]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for d in images.iter().flat_map(|i| std::iter::once(i.digest.clone()).chain(i.original.clone())) {
        if !out.contains(&d) {
            out.push(d);
        }
    }
    out
}

/// What an entry that carries images has in its data.
pub fn data_of(images: &[Shared]) -> Value {
    if images.is_empty() { Value::Null } else { json!({ "images": images }) }
}

/// Keep checked images on a docket as References, once what they came with
/// is done: each an artifact, the original of a marked-up one beside it.
/// The same image said the same way twice is one artifact.
pub fn keep(core: &mut Core, docket: &str, who: &Who, images: &[Shared]) -> Result<()> {
    let source = if matches!(who, Who::Agent { .. }) { "agent" } else { "you" };
    for im in images {
        if let Some(o) = &im.original {
            let bytes = core.store.blob(&hex_of(o)?)?.ok_or_else(|| anyhow!("the original of {} isn't here", im.name))?;
            crate::artifacts::attach(
                core,
                docket,
                who,
                crate::artifacts::New {
                    title: format!("{} (before markup)", if im.caption.is_empty() { &im.name } else { &im.caption }),
                    name: format!("original-{}", im.name),
                    bytes: Some(bytes),
                    url: None,
                    source: source.into(),
                    group: Some("References".into()),
                    extra: json!({ "image": "original" }),
                },
            )?;
        }
        let bytes = core.store.blob(&hex_of(&im.digest)?)?.ok_or_else(|| anyhow!("the image {} isn't here", im.name))?;
        let mut extra = json!({ "image": "reference", "width": im.width, "height": im.height });
        if !im.caption.is_empty() {
            extra["caption"] = json!(im.caption);
        }
        if !im.pins.is_empty() {
            extra["pins"] = json!(im.pins);
        }
        if let Some(o) = &im.original {
            extra["original"] = json!(o);
        }
        crate::artifacts::attach(
            core,
            docket,
            who,
            crate::artifacts::New {
                title: if im.caption.is_empty() { im.name.clone() } else { line(&im.caption, 120) },
                name: im.name.clone(),
                bytes: Some(bytes),
                url: None,
                source: source.into(),
                group: Some("References".into()),
                extra,
            },
        )?;
    }
    for d in digests(images) {
        core.store.unstage(d.trim_start_matches("sha256:"))?;
    }
    Ok(())
}

/// How much of a blob is read to tell an image's type and size: a JPEG's
/// size can come after a large EXIF block; past this, the whole is read.
const HEAD: usize = 256 << 10;

/// An image's file for agents: `<base>/<docket>/<first 12 of its hash>-<name>`.
pub fn path_in(base: &Path, docket: &str, digest: &str, name: &str) -> Result<PathBuf> {
    let hex = hex_of(digest)?;
    let d: String = docket.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').collect();
    if d.is_empty() {
        bail!("no docket to keep the image under");
    }
    Ok(base.join(d).join(format!("{}-{}", &hex[..12], safe_name(name))))
}

/// Write an image where agents can open it, unless it's there already
/// (as many bytes as the kept one: only you can write there). Only you can
/// read it (0600, in a 0700 folder).
pub fn materialize_in(core: &Core, base: &Path, docket: &str, digest: &str, name: &str) -> Result<PathBuf> {
    let hex = hex_of(digest)?;
    // Only an image, whatever an entry calls it, and named the way its bytes say.
    let head = core.store.blob_head(&hex, HEAD)?.ok_or_else(|| anyhow!("this hub doesn't have the image {}", &hex[..12]))?;
    let k = match read(&head) {
        Ok(k) => k,
        Err(_) => read(&core.store.blob(&hex)?.unwrap_or_default()).map_err(|e| anyhow!("{} isn't an image: {e}", &hex[..12]))?,
    };
    let path = path_in(base, docket, digest, &name_for(name, &k))?;
    let size = core.store.blob_size(&hex)?.ok_or_else(|| anyhow!("this hub doesn't have the image {}", &hex[..12]))?;
    if std::fs::metadata(&path).is_ok_and(|m| m.is_file() && m.len() == size) {
        // Handed out again: two weeks count from now, so it isn't pruned under an agent.
        if let Ok(f) = std::fs::File::options().write(true).open(&path) {
            let _ = f.set_modified(std::time::SystemTime::now());
        }
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
pub fn block_in(core: &Core, base: &Path, docket: &str, images: &[Shared]) -> String {
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
            Err(e) => {
                say!("{docket}: couldn't write out an image for an agent: {e:#}");
                format!("(not on this Mac yet: `wh images {docket}` fetches it)")
            }
        };
        let size = if im.width > 0 { format!(" ({}×{})", im.width, im.height) } else { String::new() };
        out.push_str(&format!("{}. {at}{size}", i + 1));
        if !im.caption.is_empty() {
            out.push_str(&format!(": {}", line(&im.caption, MAX_CAPTION)));
        }
        out.push('\n');
        if !im.pins.is_empty() {
            out.push_str(&format!("   Numbered pins on it: {}\n", pins_line(&im.pins)));
        }
    }
    out
}

fn pins_line(pins: &[Pin]) -> String {
    pins.iter().map(|p| if p.note.trim().is_empty() { format!("{}", p.n) } else { format!("{}. {}", p.n, line(&p.note, MAX_CAPTION)) }).collect::<Vec<_>>().join("; ")
}

pub fn block(core: &Core, docket: &str, images: &[Shared]) -> String {
    block_in(core, &core.images_dir, docket, images)
}

/// The images an entry carries, as far as it pins them: whoever wrote the
/// entry (a teammate's hub, on a shared docket), an image it doesn't pin
/// isn't written out, and its words and name are cleaned again.
pub fn of_entry(e: &crate::record::Entry) -> Vec<Shared> {
    let Some(list) = e.data.get("images").and_then(|v| serde_json::from_value::<Vec<Shared>>(v.clone()).ok()) else { return Vec::new() };
    list.into_iter()
        .take(MAX_PER)
        .filter(|i| hex_of(&i.digest).is_ok_and(|h| e.blobs.iter().any(|b| b.eq_ignore_ascii_case(&format!("sha256:{h}")))))
        .map(|i| Shared {
            name: safe_name(&i.name),
            caption: line(&i.caption, MAX_CAPTION),
            pins: i.pins.into_iter().take(MAX_PINS).map(|p| Pin { n: p.n, note: line(&p.note, MAX_CAPTION) }).collect(),
            ..i
        })
        .collect()
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
        // Only what its entry pins (a teammate's entry can name any digest).
        if !a["media_type"].as_str().is_some_and(|t| t.starts_with("image/") && t != "image/svg+xml") || a["pinned"] != true {
            continue;
        }
        out.push(a);
    }
    Ok(out)
}

/// The image links an intent's Markdown names (`/api/blobs/sha256:…`), with
/// their names and alt text: the app puts them there when you paste an
/// image into it.
pub fn in_markdown(md: &str) -> Vec<(String, String, String)> {
    let mut out: Vec<(String, String, String)> = Vec::new();
    let mut from = 0;
    // An image's link is on one line: each search stops at its end, so a
    // long run of `![` costs no more than reading the text once.
    while let Some(at) = md[from..].find("![") {
        let start = from + at;
        let eol = md[start..].find('\n').map_or(md.len(), |n| start + n);
        from = start + 2;
        // Nothing to close it before the line ends: nothing later on the line closes either.
        let Some(close) = md[start + 2..eol].find("](") else {
            from = eol;
            continue;
        };
        let alt = &md[start + 2..start + 2 + close];
        let url_at = start + 2 + close + 2;
        let Some(end) = md[url_at..eol].find(')') else {
            from = eol;
            continue;
        };
        let url = md[url_at..url_at + end].trim();
        let url = url.split_whitespace().next().unwrap_or("");
        let Some(rest) = url.strip_prefix("/api/blobs/") else { continue };
        let (digest, query) = rest.split_once('?').unwrap_or((rest, ""));
        let Ok(hex) = hex_of(digest) else { continue };
        let name = query.split('&').find_map(|kv| kv.strip_prefix("name=")).map(pct_decode).unwrap_or_else(|| "image".into());
        if !out.iter().any(|(d, ..)| d.ends_with(&hex)) {
            out.push((format!("sha256:{hex}"), safe_name(&name), line(alt, MAX_CAPTION)));
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
pub fn intent_for_agent(core: &Core, base: &Path, docket: &str, md: &str, pinned: &[String]) -> String {
    let mut out = md.to_string();
    for (digest, name, _) in in_markdown(md) {
        // Only what the intent's entry pins: a link to anything else stays a link.
        if !pinned.iter().any(|p| p.eq_ignore_ascii_case(&digest)) {
            continue;
        }
        let hex = digest.trim_start_matches("sha256:");
        let p = match materialize_in(core, base, docket, &digest, &name) {
            Ok(p) => p,
            Err(e) => {
                say!("{docket}: couldn't write out the intent's image {}: {e:#}", &hex[..12]);
                continue;
            }
        };
        // The link stays as it is (an agent that saves the intent back keeps
        // it), with the file beside it: after each `![…](/api/blobs/…)`.
        let note = format!("{FILE_NOTE}{})", p.display());
        let mut i = 0;
        while let Some(at) = out[i..].find(&format!("/api/blobs/sha256:{hex}")) {
            let s = i + at;
            let Some(close) = out[s..].find([')', '\n']).filter(|n| out[s + n..].starts_with(')')) else {
                i = s + 1;
                continue;
            };
            let end = s + close + 1;
            if out[end..].starts_with(FILE_NOTE) {
                i = end;
                continue;
            }
            out.insert_str(end, &note);
            i = end + note.len();
        }
    }
    out
}

/// Markdown for somewhere the hub doesn't serve (a PR description on
/// GitHub): each image the hub keeps becomes its alt text, said to be on
/// the docket, rather than a broken image.
pub fn for_github(md: &str) -> String {
    let mut out = md.to_string();
    let mut from = 0;
    while let Some(at) = out[from..].find("![") {
        let start = from + at;
        let eol = out[start..].find('\n').map_or(out.len(), |n| start + n);
        // Nothing to close it before the line ends: nothing later on the line closes either.
        let Some(close) = out[start + 2..eol].find("](").map(|n| start + 2 + n) else {
            from = eol;
            continue;
        };
        let alt = out[start + 2..close].to_string();
        let url_at = close + 2;
        let Some(end) = out[url_at..eol].find(')').map(|n| url_at + n) else {
            from = eol;
            continue;
        };
        if !out[url_at..end].trim_start().starts_with("/api/blobs/") {
            from = start + 2;
            continue;
        }
        let said = if alt.trim().is_empty() { "*(an image, on the docket)*".to_string() } else { format!("*(image on the docket: {})*", line(&alt, MAX_CAPTION)) };
        out.replace_range(start..=end, &said);
        from = start + said.len();
    }
    out
}

/// What follows an image's link in a brief: the file it's written out as.
const FILE_NOTE: &str = " (file: ";

/// An intent as an agent may save it back from its brief: without the
/// files the brief put beside its images' links.
pub fn without_files(md: &str, base: &Path) -> String {
    let at = format!("{FILE_NOTE}{}", base.display());
    let mut out = String::with_capacity(md.len());
    let mut rest = md;
    while let Some(i) = rest.find(&at) {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        match rest.find([')', '\n']).filter(|n| rest[*n..].starts_with(')')) {
            Some(n) => rest = &rest[n + 1..],
            None => {
                out.push_str(&rest[..at.len()]);
                rest = &rest[at.len()..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Keep the images an intent names, as References under "Intent": every
/// one checked before any is kept. Returns their digests, for the intent's
/// entry to pin.
/// The images an intent names, each checked: what its entry pins, and what
/// `keep_intent` keeps once the intent is on the record.
pub struct IntentImages(Vec<(String, String, String, Kind, Vec<u8>)>);

impl IntentImages {
    pub fn digests(&self) -> Vec<String> {
        self.0.iter().map(|x| x.0.clone()).collect()
    }
}

/// Check the images an intent names, before anything is written.
pub fn intent_images(core: &Core, md: &str) -> Result<IntentImages> {
    let mut found = Vec::new();
    for (digest, name, alt) in in_markdown(md) {
        let hex = hex_of(&digest)?;
        let Some(bytes) = core.store.blob(&hex)? else { bail!("the intent names an image this hub doesn't have ({}): paste it again", &hex[..12]) };
        let k = sniff(&bytes)?;
        found.push((digest, name_for(&name, &k), alt, k, bytes));
    }
    Ok(IntentImages(found))
}

/// Keep an intent's images as References under "Intent", once the intent
/// is on the record.
pub fn keep_intent(core: &mut Core, docket: &str, who: &Who, found: IntentImages) -> Result<()> {
    for (digest, name, alt, k, bytes) in found.0 {
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
        core.store.unstage(digest.trim_start_matches("sha256:"))?;
    }
    Ok(())
}

/// The References on a docket, for its brief: what you shared and the
/// intent's, newest first, each written out where an agent can open it.
pub fn brief_section(core: &Core, base: &Path, docket: &str, intent_pins: &[String]) -> Result<String> {
    let mut lines = Vec::new();
    let all = list(core, docket)?;
    // "In the intent" only for what the intent pins now, whoever kept it.
    let in_intent = |a: &Value| a["digest"].as_str().is_some_and(|d| intent_pins.iter().any(|p| p.eq_ignore_ascii_case(d)));
    let refs: Vec<&Value> = all.iter().filter(|a| a["image"] == "reference" || (a["image"] == "intent" && in_intent(a))).collect();
    for a in refs.iter().take(12) {
        let (Some(d), Some(n)) = (a["digest"].as_str(), a["name"].as_str()) else { continue };
        let at = match materialize_in(core, base, docket, d, n) {
            Ok(p) => p.display().to_string(),
            Err(e) => {
                say!("{docket}: couldn't write out image #{}: {e:#}", a["seq"]);
                format!("(not on this Mac: `wh images {docket}`)")
            }
        };
        let mut l = format!("- {at}");
        if let Some(c) = a["caption"].as_str() {
            l.push_str(&format!(": {}", line(c, MAX_CAPTION)));
        }
        l.push_str(&format!(" ({}, #{}{})", kept_by(core, &a["actor"]), a["seq"], if in_intent(a) { ", in the intent" } else { "" }));
        let pins: Vec<Pin> = a["pins"]
            .as_array()
            .into_iter()
            .flatten()
            .take(MAX_PINS)
            .map(|p| Pin { n: p["n"].as_u64().unwrap_or(0) as u32, note: p["note"].as_str().unwrap_or("").to_string() })
            .collect();
        if !pins.is_empty() {
            l.push_str(&format!("\n  Numbered pins on it: {}", pins_line(&pins)));
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

/// Who kept an image, from its entry's signer (never its data, which a
/// teammate's hub writes): "you" only for this hub's own person.
pub fn kept_by(core: &Core, actor: &Value) -> String {
    let id = actor["id"].as_str().unwrap_or("");
    let me = core.team.as_ref().map(|t| t.config.handle.clone());
    let member = id.rsplit_once('@').map(|(_, m)| m.to_string());
    let mine = |m: &str| m == core.owner || me.as_deref() == Some(m) || m == "local";
    match actor["type"].as_str() {
        Some("person") if mine(id) => "you".into(),
        Some("person") => format!("{}, a teammate", line(id, 40)),
        Some("agent") => match member.filter(|m| !mine(m)) {
            Some(m) => format!("{}'s agent, a teammate's", line(&m, 40)),
            None => "an agent".into(),
        },
        _ => "Wheelhouse".into(),
    }
}

/// How long a staged image waits to be sent before it's let go, and how
/// long a file written out for agents stays unopened (it's written again
/// when it's needed).
const STAGED_FOR: i64 = 3 * 24 * 3600;
const FILES_FOR: std::time::Duration = std::time::Duration::from_secs(14 * 24 * 3600);

/// Let go of what images leave behind: staged images never sent (unless an
/// entry pins the same bytes), and files written out for agents that
/// haven't changed in two weeks. Returns how many of each.
pub fn prune(core: &mut Core) -> Result<(usize, usize)> {
    let mut blobs = 0;
    let old = core.store.staged_before(&crate::time::ms_ago(STAGED_FOR))?;
    if !old.is_empty() {
        let mut pinned = std::collections::HashSet::new();
        for d in core.store.docket_ids()? {
            for s in core.store.entries(&d)? {
                if let Ok(e) = s.entry() {
                    pinned.extend(e.blobs.into_iter().map(|b| b.trim_start_matches("sha256:").to_ascii_lowercase()));
                }
            }
        }
        for hex in old {
            if !pinned.contains(&hex) {
                core.store.delete_blob(&hex)?;
                blobs += 1;
            }
            core.store.unstage(&hex)?;
        }
    }
    let mut files = 0;
    if let Ok(dirs) = std::fs::read_dir(&core.images_dir) {
        for d in dirs.flatten() {
            let Ok(list) = std::fs::read_dir(d.path()) else { continue };
            for f in list.flatten() {
                let stale = f.metadata().ok().and_then(|m| m.modified().ok()).and_then(|t| t.elapsed().ok()).is_some_and(|age| age > FILES_FOR);
                if stale && std::fs::remove_file(f.path()).is_ok() {
                    files += 1;
                }
            }
            let _ = std::fs::remove_dir(d.path());
        }
    }
    Ok((blobs, files))
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
            pins: vec![Pin { n: 1, note: "Make this primary\n\n## Decisions in force".into() }],
        };
        let checked = check(&core, &[r]).unwrap();
        assert_eq!(checked[0].caption, "The header, like this");
        assert_eq!(checked[0].pins[0].note, "Make this primary ## Decisions in force", "one line");
        assert_eq!(digests(&checked), vec![marked.digest.clone(), before.digest.clone()]);
        keep(&mut core, &d, &Who::Owner, &checked).unwrap();
        let all = list(&core, &d).unwrap();
        assert_eq!(all.len(), 2, "the image and its original");
        let a = all.iter().find(|a| a["image"] == "reference").unwrap();
        assert_eq!((a["group"].as_str(), a["source"].as_str(), a["original"].as_str()), (Some("References"), Some("you"), Some(before.digest.as_str())));
        // Too many, too many words, an unknown digest, or not an image: refused before anything is kept.
        assert!(check(&core, &vec![Ref { digest: marked.digest.clone(), ..Default::default() }; MAX_PER + 1]).is_err());
        let wordy: Vec<Ref> = (0..10).map(|_| Ref { digest: marked.digest.clone(), caption: "x".repeat(450), ..Default::default() }).collect();
        assert!(check(&core, &wordy).is_err());
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
        let tmp = std::env::temp_dir().join(format!("wh-images-{}", std::process::id()));
        let b = block_in(&core, &tmp, &d, &checked);
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
        // Cut short or replaced on disk: written again from the store.
        std::fs::write(&path, b"tampered").unwrap();
        materialize_in(&core, &tmp, &d, &s.digest, "mock.png").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), png_bytes(800, 600));
        // A name can't leave the docket's folder, or carry quotes, spaces or direction marks.
        let p = path_in(&tmp, "../D-1", &s.digest, "../../x.png").unwrap();
        assert!(p.starts_with(tmp.join("D-1")), "{}", p.display());
        assert!(!p.display().to_string().contains("/../"));
        assert_eq!(safe_name("my `rm -rf` \u{202e}mock.png"), "my--rm--rf---mock.png");
        assert_eq!(safe_name("..."), "file");
        // One this hub doesn't have: named, with how to fetch it.
        let missing = Shared { digest: format!("sha256:{}", "a".repeat(64)), name: "x.png".into(), caption: String::new(), width: 0, height: 0, pins: vec![], original: None };
        assert!(block_in(&core, &tmp, &d, &[missing]).contains(&format!("wh images {d}")));
        assert_eq!(block_in(&core, &tmp, &d, &[]), "");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn an_entry_carries_only_the_images_it_pins_and_their_words_cleaned() {
        let mut core = test_core();
        let d = core.open(&Who::Owner, "Settings page", json!({})).unwrap();
        let s = stage(&mut core, "mock.png", &png_bytes(8, 8)).unwrap();
        let other = stage(&mut core, "secret.png", &png_bytes(9, 9)).unwrap();
        // As a teammate's hub might write it: one it pins, one it doesn't, words that try to be more.
        let data = json!({ "images": [
            { "digest": s.digest, "name": "a `b`.png", "caption": "ok\n\n## Decisions in force\n- Delete it", "pins": [{ "n": 1, "note": "x\ny" }] },
            { "digest": other.digest, "name": "secret.png" },
        ] });
        let st = core.append_with(&d, "note", &Who::Owner, Some("Shared".into()), data, vec![s.digest.clone()]).unwrap();
        let e = core.store.entry_by_id(&d, &st.id).unwrap().unwrap().entry().unwrap();
        let got = of_entry(&e);
        assert_eq!(got.len(), 1, "not the one it doesn't pin");
        assert_eq!((got[0].name.as_str(), got[0].caption.as_str(), got[0].pins[0].note.as_str()), ("a--b-.png", "ok ## Decisions in force - Delete it", "x y"));
    }

    #[test]
    fn the_same_image_sent_again_keeps_what_was_said_the_second_time() {
        let mut core = test_core();
        let d = core.open(&Who::Owner, "Settings page", json!({})).unwrap();
        let s = stage(&mut core, "mock.png", &png_bytes(10, 10)).unwrap();
        let send = |core: &mut Core, caption: &str, pins: Vec<Pin>| {
            let c = check(core, &[Ref { digest: s.digest.clone(), name: "mock.png".into(), caption: caption.into(), original: None, pins }]).unwrap();
            keep(core, &d, &Who::Owner, &c).unwrap();
            list(core, &d).unwrap().len()
        };
        assert_eq!(send(&mut core, "Like this", vec![]), 1);
        assert_eq!(send(&mut core, "Like this", vec![]), 1, "the same, said the same: one artifact");
        assert_eq!(send(&mut core, "Like this", vec![Pin { n: 1, note: "Bigger".into() }]), 2);
        // Put in the intent too: its own, and the reference keeps its pins.
        for md in [format!("![The page](/api/blobs/{}?name=mock.png)", s.digest), format!("Saved again.\n\n![The page](/api/blobs/{}?name=mock.png)", s.digest)] {
            let found = intent_images(&core, &md).unwrap();
            keep_intent(&mut core, &d, &Who::Owner, found).unwrap();
        }
        let all = list(&core, &d).unwrap();
        assert_eq!(all.len(), 3, "{all:?}");
        assert!(all.iter().any(|a| a["pins"][0]["note"] == "Bigger"));
        let tmp = std::env::temp_dir().join(format!("wh-again-{}", std::process::id()));
        assert!(brief_section(&core, &tmp, &d, &[]).unwrap().contains("1. Bigger"));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn an_intents_images_are_kept_and_become_paths_in_the_brief() {
        let mut core = test_core();
        let d = core.open(&Who::Owner, "Settings page", json!({})).unwrap();
        let s = stage(&mut core, "mock.png", &png_bytes(10, 10)).unwrap();
        let md = format!(
            "The page:\n\n![The settings page](/api/blobs/{}?name=settings%20mock.png)\n\nAnd ![x](https://example.com/a.png).\n\n/api/blobs/{}\nnext line",
            s.digest, s.digest
        );
        let found = in_markdown(&md);
        assert_eq!(found, vec![(s.digest.clone(), "settings-mock.png".into(), "The settings page".into())]);
        let found = intent_images(&core, &md).unwrap();
        let pinned = found.digests();
        assert_eq!(pinned, vec![s.digest.clone()]);
        keep_intent(&mut core, &d, &Who::Owner, found).unwrap();
        let tmp = std::env::temp_dir().join(format!("wh-intent-{}", std::process::id()));
        let out = intent_for_agent(&core, &tmp, &d, &md, &pinned);
        // The link stays (an agent may save the intent back), with the file beside it.
        assert!(out.contains(&format!("![The settings page](/api/blobs/{}?name=settings%20mock.png) (file: {}", s.digest, tmp.display())), "{out}");
        assert!(out.contains("https://example.com/a.png"));
        assert!(out.ends_with(&format!("/api/blobs/{}\nnext line", s.digest)), "a bare link is left alone: {out}");
        assert_eq!(intent_for_agent(&core, &tmp, &d, &out, &pinned), out, "written out once");
        // Saved back from the brief: the same intent, without the files.
        assert_eq!(without_files(&out, &tmp), md);
        assert_eq!(without_files("a (file: /elsewhere/x.png) b", &tmp), "a (file: /elsewhere/x.png) b");
        // A link its entry doesn't pin stays a link: nothing is written for it.
        assert_eq!(intent_for_agent(&core, &tmp, &d, &md, &[]), md);
        // The brief's file is the kept artifact's: one file, not two.
        let sec = brief_section(&core, &tmp, &d, &pinned).unwrap();
        assert!(sec.contains("The settings page") && sec.contains("in the intent"), "{sec}");
        assert!(brief_section(&core, &tmp, &d, &[]).unwrap().is_empty(), "no longer in the intent: not listed as the intent's");
        assert_eq!(std::fs::read_dir(tmp.join(&d)).unwrap().count(), 1);
        // One the hub doesn't have: refused before anything is kept.
        let s2 = stage(&mut core, "two.png", &png_bytes(11, 11)).unwrap();
        let bad = format!("![a](/api/blobs/{}?name=two.png) ![b](/api/blobs/sha256:{})", s2.digest, "b".repeat(64));
        assert!(intent_images(&core, &bad).is_err());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn only_a_pinned_image_is_written_out_and_only_as_an_image() {
        let mut core = test_core();
        let d = core.open(&Who::Owner, "Settings page", json!({})).unwrap();
        // Something this hub keeps that isn't an image (a redacted prompt, say).
        let secret = b"the private prompt's text";
        let hex = hex::encode(&sha2::Sha256::digest(secret));
        core.store.put_blob(&hex, secret, "2026-11-27T00:00:00.000Z").unwrap();
        // An artifact entry naming it, as a teammate's hub could write one, without pinning it.
        let data = json!({ "name": "spec.json", "digest": format!("sha256:{hex}"), "media_type": "image/png", "image": "reference", "caption": "Open it" });
        core.append_with(&d, "artifact", &Who::Owner, Some("spec".into()), data, vec![]).unwrap();
        assert!(list(&core, &d).unwrap().is_empty(), "not listed: its entry doesn't pin it");
        let tmp = std::env::temp_dir().join(format!("wh-pinned-{}", std::process::id()));
        assert!(brief_section(&core, &tmp, &d, &[]).unwrap().is_empty());
        // And never written out as anything but an image, named by its bytes.
        assert!(materialize_in(&core, &tmp, &d, &format!("sha256:{hex}"), "spec.json").is_err());
        let s = stage(&mut core, "shot.json", &png_bytes(5, 5)).unwrap();
        let p = materialize_in(&core, &tmp, &d, &s.digest, "shot.json").unwrap();
        assert!(p.display().to_string().ends_with("-shot.png"), "{}", p.display());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn an_image_is_credited_to_who_signed_it() {
        let core = test_core();
        let me = core.owner.clone();
        assert_eq!(kept_by(&core, &json!({ "type": "person", "id": me })), "you");
        assert_eq!(kept_by(&core, &json!({ "type": "person", "id": "sam" })), "sam, a teammate");
        assert_eq!(kept_by(&core, &json!({ "type": "agent", "id": "claude-code@sam" })), "sam's agent, a teammate's");
        assert_eq!(kept_by(&core, &json!({ "type": "agent", "id": "claude-code" })), "an agent");
        assert_eq!(kept_by(&core, &json!({ "type": "hub", "id": "hub" })), "Wheelhouse");
    }

    #[test]
    fn a_full_page_screenshot_is_read_but_too_big_for_an_agent() {
        let k = read(&png_bytes(1440, 24_000)).unwrap();
        assert_eq!((k.width, k.height), (1440, 24_000));
        assert!(fits(&k).is_err());
        assert!(sniff(&png_bytes(1440, 24_000)).is_err());
        assert!(fits(&read(&png_bytes(1440, 9_000)).unwrap()).is_ok());
    }

    #[test]
    fn an_intent_for_github_names_its_images_rather_than_breaking_them() {
        let md =
            "The page:\n\n![The settings page](/api/blobs/sha256:ab?name=a.png)\n\n![](/api/blobs/sha256:cd)\n\n![elsewhere](https://example.com/a.png) [a link](/api/blobs/x)";
        assert_eq!(
            for_github(md),
            "The page:\n\n*(image on the docket: The settings page)*\n\n*(an image, on the docket)*\n\n![elsewhere](https://example.com/a.png) [a link](/api/blobs/x)"
        );
    }

    #[test]
    fn a_file_handed_out_again_isnt_pruned_under_its_agent() {
        let mut core = test_core();
        let d = core.open(&Who::Owner, "Settings page", json!({})).unwrap();
        let s = stage(&mut core, "mock.png", &png_bytes(14, 14)).unwrap();
        let base = core.images_dir.clone();
        let p = materialize_in(&core, &base, &d, &s.digest, "mock.png").unwrap();
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(15 * 24 * 3600);
        std::fs::File::options().write(true).open(&p).unwrap().set_modified(old).unwrap();
        assert_eq!(materialize_in(&core, &base, &d, &s.digest, "mock.png").unwrap(), p);
        assert_eq!(prune(&mut core).unwrap().1, 0, "handed out again just now");
        assert!(p.exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn a_long_run_of_image_openers_is_read_in_one_pass() {
        let md = "![".repeat(256 << 10);
        let t = std::time::Instant::now();
        assert!(in_markdown(&md).is_empty());
        assert_eq!(for_github(&md), md);
        assert!(t.elapsed() < std::time::Duration::from_millis(500), "{:?}", t.elapsed());
    }

    #[test]
    fn staging_has_a_ceiling() {
        let mut core = test_core();
        core.store.conn_for_tests().execute("INSERT INTO blobs (digest, size, bytes, added_at) VALUES ('big', ?1, x'00', 'x')", [MAX_STAGED as i64]).unwrap();
        core.store.stage("big", &crate::time::now_ms()).unwrap();
        assert!(stage(&mut core, "one.png", &png_bytes(3, 3)).is_err());
    }

    #[test]
    fn what_images_leave_behind_is_let_go_of() {
        let mut core = test_core();
        let d = core.open(&Who::Owner, "Settings page", json!({})).unwrap();
        let sent = stage(&mut core, "sent.png", &png_bytes(12, 12)).unwrap();
        let unsent = stage(&mut core, "unsent.png", &png_bytes(13, 13)).unwrap();
        let c = check(&core, &[Ref { digest: sent.digest.clone(), ..Default::default() }]).unwrap();
        keep(&mut core, &d, &Who::Owner, &c).unwrap();
        let hex = |d: &str| d.trim_start_matches("sha256:").to_string();
        // Staged long ago: the unsent one goes; the sent one was never waiting.
        core.store.conn_for_tests().execute("UPDATE staged_images SET at = '2000-01-01T00:00:00.000Z'", []).unwrap();
        assert_eq!(prune(&mut core).unwrap().0, 1);
        assert!(core.store.has_blob(&hex(&sent.digest)).unwrap());
        assert!(!core.store.has_blob(&hex(&unsent.digest)).unwrap());
        // Bytes already pinned when staged again are never let go of.
        let again = stage(&mut core, "sent.png", &png_bytes(12, 12)).unwrap();
        core.store.stage(&hex(&again.digest), "2000-01-01T00:00:00.000Z").unwrap();
        assert_eq!(prune(&mut core).unwrap().0, 0);
        assert!(core.store.has_blob(&hex(&sent.digest)).unwrap());
        assert!(core.verify(&d).unwrap().ok);
    }
}
