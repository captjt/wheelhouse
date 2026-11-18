//! What Recap, Chat, the record, Verify and the docket's panels share:
//! calls to the hub that land back in the window, the Mac app's small marks
//! (Mark, Chip, Panel, Steps, a section's label, a link), a sheet's card, a
//! line of the hub's inline Markdown, and SHA-256 for a kept file's hash.

use rune::{Color, El, Shadow, Span, TextCase, Tx, col, el, rich, row, text};
use serde_json::Value;

use super::Main;
use crate::theme::{self, K, font};

/// The system's monospace at a weight: a check's name.
pub const fn mono_w(size: f32, weight: f32) -> rune::tok::TextStyle {
    rune::tok::TextStyle::new(size, weight).family(rune::tok::Family::SystemMono)
}

// ---- the hub, off the UI thread ----

pub enum Req {
    Get(String),
    Post(String, Value),
}

/// Ask the hub, and hand its answer to `then` on the UI thread: inline in a
/// headless run, so a scene's frames don't wait on the network. A write
/// refreshes the state after (`Store::post`).
pub fn call(s: &mut Main, req: Req, then: impl FnOnce(&mut Main, Result<Value, String>) + Send + 'static) {
    let store = s.store.clone();
    let run = move || match req {
        Req::Get(p) => store.hub.get(&p),
        Req::Post(p, b) => store.post(&p, b),
    };
    if s.store.headless {
        let r = run();
        then(s, r);
        return;
    }
    let me = s.me.clone();
    std::thread::spawn(move || {
        let r = run();
        if let Some(me) = me {
            me.update(move |s: &mut Main, _| then(s, r));
        }
    });
}

/// The same for bytes the hub keeps as they are (a check's output, an
/// export): `Hub::get` reads JSON.
pub fn call_bytes(s: &mut Main, path: String, then: impl FnOnce(&mut Main, Result<Vec<u8>, String>) + Send + 'static) {
    let url = format!("{}{path}", s.store.hub.base());
    let run = move || -> Result<Vec<u8>, String> {
        let mut res = ureq::get(&url).call().map_err(|e| match e {
            ureq::Error::StatusCode(c) => format!("The hub answered {c}"),
            ureq::Error::ConnectionFailed | ureq::Error::HostNotFound => "The hub isn't running".to_string(),
            e => e.to_string(),
        })?;
        res.body_mut().with_config().limit(64 << 20).read_to_vec().map_err(|e| e.to_string())
    };
    if s.store.headless {
        let r = run();
        then(s, r);
        return;
    }
    let me = s.me.clone();
    std::thread::spawn(move || {
        let r = run();
        if let Some(me) = me {
            me.update(move |s: &mut Main, _| then(s, r));
        }
    });
}

// ---- marks ----

/// How an entry is known (Activity.swift's Mark).
pub fn mark(m: &str, k: &K) -> El {
    let (c, why) = match m {
        "witnessed" => (k.working, "Wheelhouse saw it happen and wrote it down."),
        "checkable" => (k.accent, "Read from a source anyone can read again (GitHub, Linear)."),
        _ => (k.faint, "A person or an agent said it: true if you trust them."),
    };
    text(m.to_string()).font(font(10.5, 500.0).lh(14.0)).color(c).help(why.to_string())
}

pub fn mark_words(m: &str) -> &'static str {
    match m {
        "witnessed" => "Wheelhouse saw it happen and wrote it down.",
        "checkable" => "Read from a source anyone can read again (GitHub, Linear).",
        "said" => "A person or an agent said it: true if you trust them.",
        _ => "",
    }
}

/// A section's small caps label, as the web app's are.
pub fn section_label(t: &str, k: &K) -> El {
    text(t.to_string()).font(font(11.0, 600.0).lh(14.0).tracking(0.4)).text_case(TextCase::Uppercase).color(k.muted)
}

/// A card on the content panel (Theme.swift's Panel): its border turns
/// orange when something in it needs you.
pub fn panel(alert: bool, k: &K) -> El {
    col().w_full().padding(14.0).gap(10.0).radius(12.0).continuous_corners().fill(k.elev).border(1.0, if alert { k.blocked.alpha(0.45) } else { k.line })
}

/// A protocol's steps, with where it is now.
pub fn steps(steps: &[String], at: i32, k: &K) -> El {
    let mut r = row().w_full().center_y();
    for (i, s) in steps.iter().enumerate() {
        let i = i as i32;
        let dot = el()
            .size(7.0, 7.0)
            .radius(3.5)
            .fill(if i < at {
                k.accent
            } else if i == at {
                k.blocked
            } else {
                k.line2
            })
            .box_shadow(Shadow { x: 0.0, y: 0.0, blur: 0.0, spread: 2.0, color: k.blocked.alpha(if i == at { 0.35 } else { 0.0 }), inset: false });
        r = r.child(row().gap(6.0).center_y().child(dot).child(text(s.clone()).font(font(12.0, if i == at { 600.0 } else { 400.0 }).lh(15.0)).color(if i <= at {
            k.text
        } else {
            k.muted
        })));
        if (i as usize) < steps.len() - 1 {
            r = r.child(el().h(1.0).grow(1.0).min_w(12.0).max_w(40.0).mx(8.0).fill(if i < at { k.accent.alpha(0.5) } else { k.line }).a11y_hidden());
        }
    }
    r
}

/// A borderless button that reads as a link (`.buttonStyle(.link)`).
pub fn link(key: impl Into<String>, label: &str, size: f32, k: &K, on: impl Fn(&mut Main, &mut Tx) + 'static) -> El {
    row()
        .key(key.into())
        .role("button")
        .label(label.to_string())
        .focusable(true)
        .cursor(rune::Cursor::Pointer)
        .on_press_tx(on)
        .child(text(label.to_string()).font(font(size, 400.0).lh(size + 4.0)).color(k.accent))
}

/// "Approved", "Changes requested", from GitHub's word (Panels.swift).
pub fn verdict_word(v: &str) -> String {
    match v {
        "APPROVED" => "Approved".into(),
        "CHANGES_REQUESTED" => "Changes requested".into(),
        "DISMISSED" => "Dismissed".into(),
        "COMMENTED" => "Commented".into(),
        v => v.replace('_', " ").to_lowercase(),
    }
}

pub fn verdict_chip(v: Option<&str>, k: &K) -> El {
    match v {
        Some(v) => theme::chip(
            &verdict_word(v),
            if v == "APPROVED" {
                k.working
            } else if v == "CHANGES_REQUESTED" {
                k.error
            } else {
                k.muted
            },
            false,
        ),
        None => el(),
    }
}

pub fn risk_chip(level: Option<&str>, k: &K) -> El {
    match level {
        Some(l) => theme::chip(&format!("Risk {l}"), k.risk(Some(l)), false),
        None => el(),
    }
}

// ---- text ----

/// Markdown as the Mac app's MarkdownView draws it at `size` (13 by
/// default), with GitHub's alerts as its callouts.
pub fn md(src: &str, size: f32, k: &K, kit: &rune_kit::Kit) -> El {
    md_with(src, Some(size), k, kit, None)
}

/// A task box pressed: its `[`'s byte offset in the source, and whether it's
/// ticked now.
pub type OnTask = std::rc::Rc<dyn Fn(&mut Main, usize, bool)>;

/// A piece of Markdown: plain from byte `at`, or one of GitHub's alerts
/// (`> [!NOTE]` and the quote under it), which rune-kit's Markdown lacks.
#[derive(Debug, PartialEq)]
pub enum Piece<'a> {
    Md { at: usize, text: &'a str },
    Alert { kind: &'static str, body: String },
}

pub fn pieces(src: &str) -> Vec<Piece<'_>> {
    const KINDS: [&str; 5] = ["NOTE", "TIP", "IMPORTANT", "WARNING", "CAUTION"];
    let mut lines = vec![];
    let mut off = 0;
    for l in src.split_inclusive('\n') {
        lines.push((off, l));
        off += l.len();
    }
    let (mut out, mut start, mut fence, mut i) = (vec![], 0, false, 0);
    while i < lines.len() {
        let (o, l) = lines[i];
        let q = l.trim_end_matches(['\n', '\r']).trim_start();
        if q.starts_with("```") || q.starts_with("~~~") {
            fence = !fence;
        }
        let alert = (!fence)
            .then(|| q.strip_prefix('>').map(str::trim_start))
            .flatten()
            .and_then(|r| KINDS.iter().find(|k| r.starts_with(&format!("[!{k}]"))).map(|k| (*k, r[k.len() + 3..].trim().to_string())));
        let Some((kind, first)) = alert else {
            i += 1;
            continue;
        };
        if o > start {
            out.push(Piece::Md { at: start, text: &src[start..o] });
        }
        let mut body = first;
        if !body.is_empty() {
            body.push('\n');
        }
        i += 1;
        while let Some((_, l)) = lines.get(i) {
            let Some(r) = l.trim_end_matches(['\n', '\r']).trim_start().strip_prefix('>') else { break };
            body.push_str(r.strip_prefix(' ').unwrap_or(r));
            body.push('\n');
            i += 1;
        }
        start = lines.get(i).map_or(src.len(), |l| l.0);
        out.push(Piece::Alert { kind, body });
    }
    if start < src.len() || out.is_empty() {
        out.push(Piece::Md { at: start, text: &src[start..] });
    }
    out
}

/// The Markdown style: the kit's (or `size`), with task boxes as the Mac's
/// checkboxes, the accent's when ticked.
fn md_style(size: Option<f32>, k: &K, kit: &rune_kit::Kit) -> rune_kit::MarkdownStyle {
    let mut s = rune_kit::MarkdownStyle::from_kit(kit).text(k.text);
    if let Some(size) = size {
        s = s.body_font(font(size, 400.0).lh((size * 1.45).round()));
    }
    let off = if k.dark { theme::hex("#5e5e5e") } else { theme::hex("#b8b8b8") };
    s.task_checkbox(14.0, 3.5, 8.0, None, k.accent, Color::WHITE, k.elev, off)
}

/// Markdown in pieces: plain runs, and alerts as a coloured bar, an icon and
/// a title over what they say. `on_task` makes task boxes live.
pub fn md_with(src: &str, size: Option<f32>, k: &K, kit: &rune_kit::Kit, on_task: Option<OnTask>) -> El {
    let style = md_style(size, k, kit);
    let one = |text: &str, at: usize| {
        let mut m = rune_kit::Markdown::new(text.to_string()).selectable(true);
        if let Some(f) = on_task.clone() {
            m = m.on_task(move |s: &mut Main, _: &str, off: usize, checked: bool| f(s, at + off, checked));
        }
        m.render(&style)
    };
    let ps = pieces(src);
    if let [Piece::Md { at, text }] = ps.as_slice() {
        return one(text, *at);
    }
    let mut c = col().w_full().gap(12.0);
    for p in ps {
        c = c.child(match p {
            Piece::Md { at, text } => one(text, at),
            Piece::Alert { kind, body } => {
                let (color, glyph, title) = match kind {
                    "WARNING" => (k.blocked, &crate::icons::BANG, "Warning"),
                    "CAUTION" => (k.error, &crate::icons::BANG, "Caution"),
                    "TIP" => (k.working, &crate::icons::BULB, "Tip"),
                    "IMPORTANT" => (theme::hex("#a855f7"), &crate::icons::INFO, "Important"),
                    _ => (k.accent, &crate::icons::INFO, "Note"),
                };
                let fs = size.unwrap_or(kit.ty.body.size);
                row()
                    .w_full()
                    .gap(10.0)
                    .child(el().w(3.0).radius(1.0).fill(color).self_stretch())
                    .child(
                        col()
                            .grow(1.0)
                            .min_w(0.0)
                            .gap(6.0)
                            .child(
                                row()
                                    .gap(6.0)
                                    .center_y()
                                    .child(rune::icon(glyph, fs).color(color))
                                    .child(text(title).font(font(fs, 600.0).lh((fs * 1.3).round())).color(color)),
                            )
                            .child(rune_kit::Markdown::new(body).selectable(true).render(&style)),
                    )
            }
        });
    }
    c
}

/// Runs of rich text, built up in order.
#[derive(Default)]
pub struct Rich {
    pub s: String,
    pub spans: Vec<Span>,
}

impl Rich {
    pub fn new() -> Rich {
        Rich::default()
    }
    pub fn push(mut self, t: &str, style: impl FnOnce(Span) -> Span) -> Rich {
        let start = self.s.len();
        self.s.push_str(t);
        self.spans.push(style(Span::new(start, self.s.len())));
        self
    }
    /// A line of the hub's inline Markdown: `code` and **bold**.
    pub fn md(mut self, src: &str, c: Color) -> Rich {
        let mut rest = src;
        while !rest.is_empty() {
            let tick = rest.find('`');
            let star = rest.find("**");
            let next = match (tick, star) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            };
            let Some(at) = next else {
                self = self.push(rest, |s| s.color(c));
                break;
            };
            if at > 0 {
                self = self.push(&rest[..at], |s| s.color(c));
            }
            let code = rest[at..].starts_with('`');
            let open = if code { 1 } else { 2 };
            let body = &rest[at + open..];
            match body.find(if code { "`" } else { "**" }) {
                Some(end) => {
                    let inner = &body[..end];
                    self = if code { self.push(inner, |s| s.color(c).mono()) } else { self.push(inner, |s| s.color(c).weight(600.0)) };
                    rest = &body[end + open..];
                }
                None => {
                    self = self.push(&rest[at..], |s| s.color(c));
                    break;
                }
            }
        }
        self
    }
    pub fn el(self) -> El {
        rich(self.s, self.spans)
    }
}

// ---- sheets ----

/// What's open over the window, centred on it as the Mac app's sheets are.
pub fn scrim(card: El, k: &K) -> El {
    col()
        .key("sheet-scrim")
        .raise()
        .absolute(0.0, 0.0)
        .w_full()
        .h_full()
        .fill(Color::BLACK.alpha(if k.dark { 0.45 } else { 0.18 }))
        .items_center()
        .justify_center()
        .py(36.0)
        .px(24.0)
        .modal(true, |s: &mut Main| s.sheet = None)
        .child(card)
}

/// A sheet's card: `w` wide, and `h` tall at most when it has a height of
/// its own (a list that scrolls), else as tall as what it holds.
pub fn sheet_card(title: &str, w: f32, h: Option<f32>, k: &K) -> El {
    let c = col()
        .key("sheet")
        .w(w)
        .max_w(f32::MAX)
        .min_h(0.0)
        .radius(16.0)
        .continuous_corners()
        .clip_corners([16.0; 4])
        .fill(if k.dark { theme::hex("#232325") } else { Color::WHITE })
        .border(1.0, if k.dark { Color::WHITE.alpha(0.12) } else { Color::BLACK.alpha(0.08) })
        .box_shadow(Shadow::core_animation(0.0, 16.0, 36.0, Color::BLACK.alpha(if k.dark { 0.5 } else { 0.25 })))
        .role("dialog")
        .label(title.to_string());
    match h {
        Some(h) => c.grow(1.0).max_h(h),
        None => c,
    }
}

/// A sheet's top: its title, what's beside it, and its buttons.
pub fn sheet_head(title: El, trailing: El) -> El {
    row().w_full().padding(16.0).gap(10.0).center_y().child(title).child(rune::spacer()).child(trailing)
}

pub fn divider(k: &K) -> El {
    el().h(1.0).w_full().fill(if k.dark { Color::WHITE.alpha(0.1) } else { Color::BLACK.alpha(0.1) }).a11y_hidden()
}

// ---- SHA-256, for a kept file checked against its hash ----

pub fn sha256_hex(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    let mut h: [u32; 8] = [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    let mut msg = data.to_vec();
    let bits = (data.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bits.to_be_bytes());
    for chunk in msg.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([chunk[i * 4], chunk[i * 4 + 1], chunk[i * 4 + 2], chunk[i * 4 + 3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let mut v = h;
        for i in 0..64 {
            let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
            let ch = (v[4] & v[5]) ^ (!v[4] & v[6]);
            let t1 = v[7].wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
            let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
            let t2 = s0.wrapping_add(maj);
            v = [t1.wrapping_add(t2), v[0], v[1], v[2], v[3].wrapping_add(t1), v[4], v[5], v[6]];
        }
        for i in 0..8 {
            h[i] = h[i].wrapping_add(v[i]);
        }
    }
    h.iter().map(|x| format!("{x:08x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_matches_known_digests() {
        assert_eq!(sha256_hex(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        let long = vec![b'a'; 1000];
        assert_eq!(sha256_hex(&long), "41edece42d63e8d9bf515a9ba6932e1c20cbc9f5a5d134645adb5db1b9737ea3");
    }

    #[test]
    fn inline_markdown_marks_code_and_bold() {
        let r = Rich::new().md("`test` passed at **d905eb8**", Color::BLACK);
        assert_eq!(r.s, "test passed at d905eb8");
        assert!(r.spans[0].mono);
        assert_eq!(r.spans.last().and_then(|s| s.weight), Some(600.0));
    }
}
