//! Settings' pieces: a grouped Form's sections and rows, and the controls
//! AppKit gives the SwiftUI window (switches, segmented and pop-up pickers,
//! steppers, link buttons), drawn in Kumo's colours as `app/widgets.rs` draws
//! the main window's.

use std::rc::Rc;

use rune::tok::TextStyle;
use rune::{Color, Cursor, El, Icon, PopupItem, PopupMenu, ScrollSpec, Shadow, Span, Tx, col, el, icon, rich, row, spacer, text};
use rune_kit::{Kit, ScrollBar};

use super::{Settings, glyphs};
use crate::theme::{self, K, font, hex, mono};

/// The window's own grey: the title bar, the tabs and a form's ground, as
/// the main window's frame.
pub fn chrome(k: &K) -> Color {
    if k.dark { hex("#1b1b1d") } else { hex("#f1f1f3") }
}

/// A form's card, on that grey.
pub fn card(k: &K) -> Color {
    k.main
}

/// An element key from an id or a slug: a `/` in a key reads as a path of keys.
pub fn key_of(prefix: &str, id: &str) -> String {
    format!("{prefix}-{}", id.replace('/', ":"))
}

// ---- a grouped Form ----

/// A form's sections, in a column that scrolls, centred to `width`.
pub fn form(key: &str, width: f32, sections: Vec<El>, kit: &Kit) -> El {
    col()
        .key(key.to_string())
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .child(col().w_full().items_center().px(24.0).pt(20.0).pb(28.0).child(col().w_full().max_w(width).gap(22.0).children(sections)))
        .scroll(ScrollSpec { stick_bottom: false })
        .scroll_bar(kit)
}

/// A Form's section: its header, a card of rows divided by hairlines, and a
/// footer that says what it's for.
pub fn section(key: &str, header: Option<&str>, rows: Vec<El>, footer: Option<&str>, k: &K) -> El {
    let mut c = col().key(key.to_string()).w_full().gap(7.0);
    if let Some(h) = header {
        c = c.child(row().px(2.0).child(text(h.to_string()).font(font(13.0, 600.0).lh(16.0)).color(k.text).role("heading").level(2)));
    }
    let mut card = col().w_full().radius(10.0).continuous_corners().fill(card(k)).border(1.0, k.line);
    for (i, r) in rows.into_iter().enumerate() {
        if i > 0 {
            card = card.child(row().w_full().pl(12.0).child(el().h(1.0).grow(1.0).fill(k.line).a11y_hidden()));
        }
        card = card.child(r);
    }
    c = c.child(card);
    if let Some(f) = footer {
        c = c.child(col().w_full().px(2.0).child(inline(f, font(11.5, 400.0).lh(15.0), k.muted, k)));
    }
    c
}

/// One of a section's rows: a control's line, and what it says under it.
pub fn cell(lines: Vec<El>) -> El {
    col().w_full().px(12.0).py(9.0).gap(5.0).children(lines)
}

/// A control's label on the left, the control on the right (LabeledContent).
pub fn labeled(label: El, control: El) -> El {
    row().w_full().gap(16.0).center_y().child(label.grow(1.0).min_w(0.0)).child(control)
}

/// A control's label with the sentence in force under it (`Described`).
pub fn described(title: &str, sentence: &str, k: &K) -> El {
    let mut c = col().gap(2.0).child(text(title.to_string()).font(font(13.0, 400.0).lh(17.0)).color(k.text).wrap());
    if !sentence.is_empty() {
        c = c.child(inline(sentence, font(11.5, 400.0).lh(15.0), k.muted, k));
    }
    c
}

/// Plain text, a size and a colour, wrapping.
pub fn words(t: impl Into<String>, size: f32, color: Color) -> El {
    text(t.into()).font(font(size, 400.0).lh((size * 1.32).round())).color(color).wrap()
}

/// Inline Markdown as SwiftUI's `inlineMarkdown` reads it: `code` and
/// **bold**; the rest is as written.
pub fn inline(src: &str, style: TextStyle, color: Color, k: &K) -> El {
    let (plain, spans) = inline_spans(src, k);
    rich(plain, spans).font(style).color(color)
}

fn inline_spans(src: &str, k: &K) -> (String, Vec<Span>) {
    let code_bg = if k.dark { Color::WHITE.alpha(0.08) } else { Color::BLACK.alpha(0.05) };
    let (mut out, mut spans, mut rest) = (String::new(), Vec::new(), src);
    loop {
        let tick = rest.find('`');
        let bold = rest.find("**");
        let (at, code) = match (tick, bold) {
            (Some(t), Some(b)) if b < t => (b, false),
            (Some(t), _) => (t, true),
            (None, Some(b)) => (b, false),
            (None, None) => break,
        };
        let open = if code { 1 } else { 2 };
        let close = rest[at + open..].find(if code { "`" } else { "**" });
        let Some(close) = close.filter(|c| *c > 0) else { break };
        out.push_str(&rest[..at]);
        let inner = &rest[at + open..at + open + close];
        let start = out.len();
        out.push_str(inner);
        spans.push(if code { Span { bg: Some(code_bg), bg_pad: 2.0, ..Span::new(start, out.len()).mono() } } else { Span::new(start, out.len()).bold() });
        rest = &rest[at + open + close + open..];
    }
    out.push_str(rest);
    (out, spans)
}

/// "Set here", with the way back to the Lua's value. Nothing when the value
/// comes from the Lua or the default (`SetHere`).
pub fn set_here(s: &Settings, part: &'static str, path: &[&str], key: &str, k: &K) -> Option<El> {
    if !s.is_set(part, path) {
        return None;
    }
    let path: Vec<String> = path.iter().map(|p| p.to_string()).collect();
    let key_s = key.to_string();
    let undo = link(&format!("{key}-revert"), &s.revert_label(part), !s.is_busy(key), 11.5, k, move |s: &mut Settings, _| {
        let p: Vec<&str> = path.iter().map(String::as_str).collect();
        s.change(&key_s, super::patch::make(part, &p, None), |_| {});
    })
    .help(format!("Take this out of settings.json: {} applies again", if s.lua_exists(part) { format!("{}'s value", s.lua(part)) } else { "the default".into() }));
    Some(set_here_line(undo, k))
}

/// "Set here · <the way back>", with any link.
pub fn set_here_line(undo: El, k: &K) -> El {
    row()
        .gap(6.0)
        .center_y()
        .child(text("Set here").font(font(11.5, 400.0).lh(15.0)).color(k.muted))
        .child(text("·").font(font(11.5, 400.0).lh(15.0)).color(k.faint).a11y_hidden())
        .child(undo)
}

/// The hub's refusal, beside what caused it (`ErrorLine`).
pub fn error_line(s: &Settings, key: &str, k: &K) -> Option<El> {
    s.errors.get(key).map(|e| alert(e, k.error, k))
}

/// A warning triangle and its words, in `color`.
pub fn alert(t: &str, color: Color, _k: &K) -> El {
    row()
        .w_full()
        .gap(5.0)
        .items_start()
        .role("alert")
        .child(el().pt(1.5).child(icon(&glyphs::WARN, 12.0).color(color)))
        .child(text(t.to_string()).font(font(11.5, 400.0).lh(15.0)).color(color).wrap().selectable(true).grow(1.0).min_w(0.0))
}

/// Lines that only exist sometimes, without the gaps.
pub fn some(lines: Vec<Option<El>>) -> Vec<El> {
    lines.into_iter().flatten().collect()
}

// ---- controls ----

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Look {
    /// The accent's (`.borderedProminent`).
    Primary,
    /// Bordered and neutral.
    Plain,
    /// Neutral, its words in the error colour (a destructive confirmation).
    Danger,
}

/// A push button, as `widgets::button` draws one.
pub fn button(key: &str, label: &str, look: Look, enabled: bool, k: &K, on: impl Fn(&mut Settings, &mut Tx) + 'static) -> El {
    let primary = look == Look::Primary;
    let (fill, ink) = match look {
        Look::Primary => (k.accent, Color::WHITE),
        _ if k.dark => (Color::WHITE.alpha(0.1), if look == Look::Danger { k.error } else { k.text }),
        _ => (Color::WHITE, if look == Look::Danger { k.error } else { k.text }),
    };
    let b = row()
        .key(key.to_string())
        .h(22.0)
        .px(10.0)
        .radius(6.0)
        .center()
        .fill(fill)
        .border(if primary { 0.0 } else { 0.5 }, k.line2)
        .box_shadow(Shadow { x: 0.0, y: 0.5, blur: 1.0, spread: 0.0, color: Color::BLACK.alpha(if primary { 0.15 } else { 0.06 }), inset: false })
        .hover(if primary { Color::WHITE.alpha(0.1) } else { k.hover })
        .role("button")
        .label(label.to_string())
        .focusable(true)
        .opacity(if enabled { 1.0 } else { 0.45 })
        .child(text(label.to_string()).font(font(12.0, 500.0).lh(15.0)).color(ink));
    if enabled { b.on_press_tx(on).cursor(Cursor::Pointer) } else { b.disabled(true) }
}

/// A borderless button with an icon and words ("+ New Guide…").
pub fn tool_button(key: &str, i: &'static Icon, label: &str, help: &str, k: &K, on: impl Fn(&mut Settings, &mut Tx) + 'static) -> El {
    row()
        .key(key.to_string())
        .h(22.0)
        .px(6.0)
        .gap(5.0)
        .radius(5.0)
        .center_y()
        .hover(k.hover)
        .role("button")
        .label(label.to_string())
        .help(help.to_string())
        .focusable(true)
        .cursor(Cursor::Pointer)
        .on_press_tx(on)
        .child(icon(i, 12.0).color(k.muted))
        .child(text(label.to_string()).font(font(12.0, 400.0).lh(15.0)).color(k.text))
}

/// A link-style button (`.buttonStyle(.link)`).
pub fn link(key: &str, label: &str, enabled: bool, size: f32, k: &K, on: impl Fn(&mut Settings, &mut Tx) + 'static) -> El {
    let l = row()
        .key(key.to_string())
        .role("button")
        .label(label.to_string())
        .focusable(true)
        .child(text(label.to_string()).font(font(size, 400.0).lh((size * 1.3).round())).color(k.link));
    if enabled { l.on_press_tx(on).cursor(Cursor::Pointer) } else { l.opacity(0.45).disabled(true) }
}

/// A segmented picker: one of a few, pressed (`.pickerStyle(.segmented)`).
pub fn segmented(key: &str, label: &str, segs: &[(&str, bool)], enabled: bool, k: &K, pick: impl Fn(&mut Settings, usize) + 'static) -> El {
    let pick = Rc::new(pick);
    let mut r = row()
        .key(key.to_string())
        .padding(2.0)
        .gap(2.0)
        .radius(7.0)
        .fill(if k.dark { Color::WHITE.alpha(0.07) } else { Color::BLACK.alpha(0.05) })
        .role("radiogroup")
        .label(label.to_string())
        .opacity(if enabled { 1.0 } else { 0.5 });
    for (i, (seg, on)) in segs.iter().enumerate() {
        let mut b = row()
            .key(format!("{key}-{i}"))
            .h(20.0)
            .px(10.0)
            .radius(5.0)
            .center()
            .fill(if *on { if k.dark { Color::WHITE.alpha(0.16) } else { Color::WHITE } } else { Color::TRANSPARENT })
            .box_shadow(Shadow { x: 0.0, y: 0.5, blur: 1.5, spread: 0.0, color: Color::BLACK.alpha(if *on { 0.12 } else { 0.0 }), inset: false })
            .role("radio")
            .label(seg.to_string())
            .checked(*on)
            .child(text(seg.to_string()).font(font(12.0, if *on { 500.0 } else { 400.0 }).lh(15.0)).color(if *on { k.text } else { k.muted }));
        if enabled {
            let p = pick.clone();
            b = b.on_press(move |s: &mut Settings| p(s, i));
        } else {
            b = b.disabled(true);
        }
        r = r.child(b);
    }
    r
}

/// One of a pop-up button's choices; `None` is a separator.
pub type Choice = Option<(String, String)>;

pub fn choice(id: &str, title: &str) -> Choice {
    Some((id.to_string(), title.to_string()))
}

#[allow(clippy::too_many_arguments)]
/// A pop-up button (a menu-style Picker): the current choice and the
/// chevrons; pressing it opens the native menu with that choice over it.
pub fn popup(key: &str, label: &str, value: &str, choices: Vec<Choice>, selected: &str, enabled: bool, k: &K, pick: impl Fn(&mut Settings, &str) + 'static) -> El {
    let pick = Rc::new(pick);
    let (key_s, selected) = (key.to_string(), selected.to_string());
    let b = row()
        .key(key.to_string())
        .h(22.0)
        .pl(9.0)
        .pr(5.0)
        .gap(6.0)
        .radius(6.0)
        .center_y()
        .fill(if k.dark { Color::WHITE.alpha(0.1) } else { Color::WHITE })
        .border(0.5, k.line2)
        .box_shadow(Shadow { x: 0.0, y: 0.5, blur: 1.0, spread: 0.0, color: Color::BLACK.alpha(0.06), inset: false })
        .hover(k.hover)
        .role("combobox")
        .label(format!("{label}: {value}"))
        .focusable(true)
        .opacity(if enabled { 1.0 } else { 0.45 })
        .child(text(value.to_string()).font(font(12.5, 400.0).lh(15.0)).color(k.text))
        .child(icon(&glyphs::UPDOWN, 11.0).color(k.muted));
    if !enabled {
        return b.disabled(true);
    }
    b.cursor(Cursor::Pointer).on_press_tx(move |_: &mut Settings, tx: &mut Tx| {
        let mut menu = PopupMenu::new();
        for c in &choices {
            menu = match c {
                Some((id, title)) => menu.item(PopupItem::new(id.clone(), title.clone()).checked(*id == selected)),
                None => menu.separator(),
            };
        }
        let p = pick.clone();
        tx.popup_menu(menu.selected(selected.clone()), key_s.clone(), move |s: &mut Settings, picked, _| {
            if let Some(id) = picked {
                p(s, id);
            }
        });
    })
}

#[allow(clippy::too_many_arguments)]
/// A stepper beside its value, as SwiftUI's Stepper draws one: the number,
/// then a pair of arrows. Arrow keys step it too.
pub fn stepper(key: &str, label: &str, value: i64, lo: i64, hi: i64, enabled: bool, k: &K, set: impl Fn(&mut Settings, i64) + 'static) -> El {
    let set = Rc::new(set);
    let arrow = |dir: i64, i: &'static Icon| {
        let can = enabled && if dir > 0 { value < hi } else { value > lo };
        let mut a = row()
            .key(format!("{key}-{}", if dir > 0 { "up" } else { "down" }))
            .w_full()
            .h(10.5)
            .center()
            .hover(k.hover)
            .role("button")
            .label(if dir > 0 { "Increment" } else { "Decrement" })
            .child(icon(i, 10.0).color(if can { k.text2 } else { k.faint }));
        if can {
            let f = set.clone();
            a = a.on_press(move |s: &mut Settings| f(s, value + dir));
        } else {
            a = a.disabled(true);
        }
        a
    };
    let step = set.clone();
    let arrows = col()
        .key(format!("{key}-arrows"))
        .w(15.0)
        .h(22.0)
        .radius(5.0)
        .fill(if k.dark { Color::WHITE.alpha(0.1) } else { Color::WHITE })
        .border(0.5, k.line2)
        .box_shadow(Shadow { x: 0.0, y: 0.5, blur: 1.0, spread: 0.0, color: Color::BLACK.alpha(0.06), inset: false })
        .clip_rounded(5.0)
        .child(arrow(1, &glyphs::UP))
        .child(el().w_full().h(0.5).fill(k.line2).a11y_hidden())
        .child(arrow(-1, &glyphs::DOWN));
    let arrows = if enabled { arrows.on_step(move |s: &mut Settings, d, _| step(s, (value + d as i64).clamp(lo, hi))) } else { arrows };
    row()
        .key(key.to_string())
        .gap(6.0)
        .center_y()
        .label(format!("{label}: {value}"))
        .opacity(if enabled { 1.0 } else { 0.5 })
        .child(text(value.to_string()).font(font(13.0, 400.0).lh(16.0).features("tnum")).color(k.text))
        .child(arrows.label(label.to_string()).range_value(value as f64, lo as f64, hi as f64))
}

/// A switch, as AppKit draws a form's: NSSwitch's mini capsule (36 × 16, a
/// 21 × 14 knob), the accent's when on.
pub fn switch(on: bool, label: &str, enabled: bool, k: &K, kit: &Kit, toggle: impl Fn(&mut Settings) + 'static) -> El {
    let (tw, th, kw, kh, inset) = (36.0, 16.0, 21.0, 14.0, 1.0);
    let track = if on {
        k.accent
    } else if k.dark {
        Color::WHITE.alpha(0.16)
    } else {
        Color::BLACK.alpha(0.09)
    };
    let knob = el()
        .key("thumb")
        .size(kw, kh)
        .absolute(if on { tw - inset - kw } else { inset }, inset)
        .radius(kh / 2.0)
        .fill(if k.dark && !on { hex("#d8d8da") } else { Color::WHITE })
        .border(0.5, Color::BLACK.alpha(0.05))
        .box_shadow(Shadow { x: 0.0, y: 0.5, blur: 1.5, spread: 0.0, color: Color::BLACK.alpha(0.28), inset: false })
        .motion(kit.motion.control);
    let control = el()
        .key("track")
        .size(tw, th)
        .radius(th / 2.0)
        .fill(track)
        .box_shadow(Shadow { x: 0.0, y: 0.5, blur: 1.0, spread: 0.0, color: Color::BLACK.alpha(if on { 0.0 } else { 0.06 }), inset: true })
        .motion(kit.motion.control)
        .child(knob);
    let r = row().role("switch").label(label.to_string()).checked(on).opacity(if enabled { 1.0 } else { 0.5 }).child(control);
    if enabled { r.focusable(true).cursor(Cursor::Pointer).on_press(toggle) } else { r.disabled(true) }
}

/// A text field's box: the field inside, a border around it.
pub fn field_box(input: El, k: &K) -> El {
    row().px(8.0).py(4.0).radius(6.0).fill(if k.dark { Color::WHITE.alpha(0.04) } else { Color::WHITE }).border(1.0, k.line2).center_y().child(input.grow(1.0).min_w(0.0))
}

/// A field's text style: the system font, or monospace.
pub fn field_font(size: f32, monospace: bool) -> TextStyle {
    if monospace { mono(size).lh((size * 1.35).round()) } else { font(size, 400.0).lh((size * 1.35).round()) }
}

// ---- sheets and empty places ----

/// A sheet over the window: its title, its body, and the buttons at its foot.
pub fn sheet(key: &str, title: &str, width: f32, body: Vec<El>, foot: Option<El>, buttons: Vec<El>, k: &K) -> El {
    let mut c = col()
        .key(key.to_string())
        .w(width)
        .radius(12.0)
        .continuous_corners()
        .fill(if k.dark { hex("#1f1f21") } else { Color::WHITE })
        .border(1.0, k.line2)
        .box_shadow(Shadow::core_animation(0.0, 12.0, 28.0, Color::BLACK.alpha(0.3)))
        .role("dialog")
        .label(title.to_string())
        .child(row().w_full().px(20.0).pt(18.0).pb(12.0).child(text(title.to_string()).font(font(14.0, 600.0).lh(18.0)).color(k.text).wrap()))
        .child(col().w_full().px(20.0).gap(10.0).children(body));
    if let Some(f) = foot {
        c = c.child(col().w_full().px(20.0).pt(12.0).child(f));
    }
    let mut b = row().w_full().px(20.0).pt(16.0).pb(18.0).gap(8.0).center_y().child(spacer());
    for x in buttons {
        b = b.child(x);
    }
    c.child(b)
}

/// A sheet's scrim: the window dims behind it; a click outside or Escape
/// cancels it.
pub fn scrim(card: El, k: &K) -> El {
    col()
        .key("sheet-scrim")
        .absolute(0.0, 0.0)
        .w_full()
        .h_full()
        .fill(Color::BLACK.alpha(if k.dark { 0.45 } else { 0.18 }))
        .items_center()
        .pt(56.0)
        .modal(true, |s: &mut Settings| s.sheet = None)
        .child(card)
}

/// One labelled line of a sheet's form: the label on the left at a fixed
/// width, the control after it.
pub fn form_line(label: &str, control: El, k: &K) -> El {
    row().w_full().gap(12.0).center_y().child(text(label.to_string()).font(font(13.0, 400.0).lh(17.0)).color(k.text).w(110.0)).child(control.grow(1.0).min_w(0.0))
}

/// Loading, in a quiet line.
pub fn loading(k: &K) -> El {
    col().w_full().h_full().center().child(text("Loading…").font(font(12.5, 400.0).lh(16.0)).color(k.muted))
}

/// A chip: a verdict, a state, an origin.
pub fn chip(t: &str, c: Color) -> El {
    theme::chip(t, c, false)
}
