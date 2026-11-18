//! Pieces the screens share: buttons and segments as macOS draws them,
//! headers that fold, an empty place, and toasts.

use rune::{Color, El, Icon, Tx, col, el, icon, row, spacer, text};

use super::{Main, Toast};
use crate::icons;
use crate::theme::{K, font};

/// A button: `primary` is the accent's, else a bordered neutral one.
pub fn button(key: &str, label: &str, primary: bool, enabled: bool, k: &K, on: impl Fn(&mut Main, &mut Tx) + 'static) -> El {
    let (fill, ink) = if primary { (k.accent, Color::WHITE) } else if k.dark { (Color::WHITE.alpha(0.1), k.text) } else { (Color::WHITE, k.text) };
    let mut b = row()
        .key(key.to_string())
        .h(22.0)
        .px(10.0)
        .radius(6.0)
        .center()
        .fill(fill)
        .border(if primary { 0.0 } else { 0.5 }, k.line2)
        .box_shadow(rune::Shadow { x: 0.0, y: 0.5, blur: 1.0, spread: 0.0, color: Color::BLACK.alpha(if primary { 0.15 } else { 0.06 }), inset: false })
        .hover(if primary { Color::WHITE.alpha(0.1) } else { k.hover })
        .role("button")
        .label(label.to_string())
        .focusable(true)
        .opacity(if enabled { 1.0 } else { 0.45 })
        .child(text(label.to_string()).font(font(12.0, 500.0).lh(15.0)).color(ink));
    b = if enabled { b.on_press_tx(on).cursor(rune::Cursor::Pointer) } else { b.disabled(true) };
    b
}

/// A borderless icon button, as a toolbar's.
pub fn icon_button(key: &str, i: &'static Icon, help: &str, on: impl Fn(&mut Main) + 'static, k: &K) -> El {
    row()
        .key(key.to_string())
        .size(28.0, 26.0)
        .radius(6.0)
        .center()
        .hover(k.hover)
        .role("button")
        .label(help.to_string())
        .help(help.to_string())
        .focusable(true)
        .on_press(on)
        .child(icon(i, 16.0).color(k.muted))
}

/// A segmented control: one of a few, pressed.
pub fn segmented(key: &str, segs: &[(&str, bool)], pick: fn(&mut Main, usize), k: &K) -> El {
    let mut r = row().key(key.to_string()).padding(2.0).gap(2.0).radius(7.0).fill(if k.dark { Color::WHITE.alpha(0.07) } else { Color::BLACK.alpha(0.05) }).role("radiogroup");
    for (i, (label, on)) in segs.iter().enumerate() {
        r = r.child(
            row()
                .key(format!("{key}-{i}"))
                .h(20.0)
                .px(10.0)
                .radius(5.0)
                .center()
                .fill(if *on { if k.dark { Color::WHITE.alpha(0.16) } else { Color::WHITE } } else { Color::TRANSPARENT })
                .box_shadow(rune::Shadow { x: 0.0, y: 0.5, blur: 1.5, spread: 0.0, color: Color::BLACK.alpha(if *on { 0.12 } else { 0.0 }), inset: false })
                .role("radio")
                .label(label.to_string())
                .checked(*on)
                .on_press(move |s: &mut Main| pick(s, i))
                .child(text(label.to_string()).font(font(12.0, if *on { 500.0 } else { 400.0 }).lh(15.0)).color(if *on { k.text } else { k.muted })),
        );
    }
    r
}

/// A small segmented control of icons: the theme's System, Light, Dark.
pub fn icon_segments(key: &str, segs: &[(&'static Icon, &str, bool)], pick: fn(&mut Main, usize), k: &K) -> El {
    let mut r = row().key(key.to_string()).padding(2.0).gap(1.0).radius(6.0).fill(if k.dark { Color::WHITE.alpha(0.07) } else { Color::BLACK.alpha(0.05) }).role("radiogroup").label("Theme");
    for (i, (ic, label, on)) in segs.iter().enumerate() {
        r = r.child(
            row()
                .key(format!("{key}-{i}"))
                .size(20.0, 16.0)
                .radius(4.0)
                .center()
                .fill(if *on { if k.dark { Color::WHITE.alpha(0.16) } else { Color::WHITE } } else { Color::TRANSPARENT })
                .role("radio")
                .label(label.to_string())
                .help(label.to_string())
                .checked(*on)
                .on_press(move |s: &mut Main| pick(s, i))
                .child(icon(ic, 11.0).color(if *on { k.text } else { k.muted })),
        );
    }
    r
}

/// A group's header that folds it: a chevron, a mark, its label and count.
pub fn fold_header(key: &str, mark: El, label: &str, count: usize, folded: bool, k: &K) -> El {
    let key_s = key.to_string();
    row()
        .key(format!("fold-{key}"))
        .w_full()
        .h(28.0)
        .px(12.0)
        .gap(7.0)
        .center_y()
        .role("button")
        .label(format!("{label}, {count}"))
        .expanded(!folded)
        .on_press(move |s: &mut Main| {
            if !s.folded.remove(&key_s) {
                s.folded.insert(key_s.clone());
            }
        })
        .child(icon(if folded { &icons::CHEVRON_RIGHT } else { &icons::CHEVRON }, 11.0).color(k.faint))
        .child(mark)
        .child(text(label.to_string()).font(font(12.0, 600.0).lh(15.0)).color(k.text))
        .child(text(count.to_string()).font(font(11.5, 400.0).lh(15.0)).color(k.faint))
        .child(spacer())
}

/// Nothing to show here, and why.
pub fn empty(i: &'static Icon, title: &str, why: &str, k: &K) -> El {
    col()
        .key("empty")
        .w_full()
        .h_full()
        .center()
        .gap(8.0)
        .padding(40.0)
        .child(icon(i, 30.0).color(k.faint))
        .child(text(title.to_string()).font(font(20.0, 600.0).lh(24.0)).color(k.text))
        .child(text(why.to_string()).font(font(13.0, 400.0).lh(17.0)).color(k.muted).max_w(420.0).wrap().align(rune::Align::Center))
}

/// What just happened, at the window's foot.
pub fn toasts(list: &[Toast], k: &K) -> El {
    let mut c = col().key("toasts").absolute(0.0, 0.0).w_full().h_full().justify_end().items_center().pb(24.0).gap(6.0).inert();
    for (i, t) in list.iter().enumerate() {
        c = c.child(
            row()
                .key(format!("toast-{i}"))
                .px(14.0)
                .py(8.0)
                .gap(8.0)
                .radius(10.0)
                .continuous_corners()
                .fill(if k.dark { k.elev } else { Color::WHITE })
                .border(1.0, k.line2)
                .box_shadow(rune::Shadow::core_animation(0.0, 6.0, 14.0, Color::BLACK.alpha(0.18)))
                .center_y()
                .role("status")
                .live_region(rune::LiveRegion::Polite)
                .child(el().size(7.0, 7.0).radius(3.5).fill(if t.error { k.error } else { k.working }))
                .child(text(t.text.clone()).font(font(12.5, 400.0).lh(16.0)).color(if t.error { k.error } else { k.text })),
        );
    }
    c
}

/// A label beside its value, in the inspector.
pub fn field(label: &str, value: El, k: &K) -> El {
    row()
        .w_full()
        .gap(10.0)
        .py(5.0)
        .child(text(label.to_string()).font(font(12.0, 400.0).lh(16.0)).color(k.muted).w(74.0))
        .child(value.grow(1.0).min_w(0.0))
}

pub fn value_text(t: impl Into<String>, k: &K) -> El {
    text(t.into()).font(font(12.5, 400.0).lh(16.0)).color(k.text).wrap()
}
