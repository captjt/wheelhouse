//! The panel, drawn to match the SwiftUI one (Palette.swift, floating):
//! a 600 pt card in a 648×520 window, a 16 pt search field, 13 pt rows
//! grouped under 11 pt headers, the selected row in the accent at 14%, the
//! list 380 pt tall. Colours are Kumo's (Theme.swift, `K`), light and dark.

use rune::tok::{Family, TextStyle};
use rune::{Color, Cx, El, ScrollSpec, Shadow, SystemMaterial, col, el, icon, row, spacer, text, text_input};
use rune_kit::{Kit, PaletteSeed, ScrollBar, ScrollBarStyle};

use crate::commands::{CREATE, Command, DOCKETS, Mark, NEEDS_YOU, Status};
use crate::icons;
use crate::{Mode, Panel};

/// The window: the card and room for its shadow.
pub const SIZE: (f32, f32) = (648.0, 520.0);
const CARD_W: f32 = 600.0;
const LIST_H: f32 = 380.0;

const SYS: Family = Family::Named("System Font");
const FIELD: TextStyle = TextStyle::new(16.0, 400.0).family(SYS).lh(18.0);
const ROW: TextStyle = TextStyle::new(13.0, 400.0).family(SYS).lh(16.0);
const GROUP: TextStyle = TextStyle::new(11.0, 600.0).family(SYS).lh(14.0);
const HINT: TextStyle = TextStyle::new(11.5, 400.0).family(SYS).lh(14.0);
const HINT_MONO: TextStyle = TextStyle::new(11.5, 400.0).family(Family::SystemMono).lh(14.0);
const SMALL: TextStyle = TextStyle::new(11.5, 400.0).family(SYS).lh(15.0);
const BUTTON: TextStyle = TextStyle::new(13.0, 500.0).family(SYS).lh(16.0);
const LABEL: TextStyle = TextStyle::new(13.0, 400.0).family(SYS).lh(16.0);

/// Kumo's tokens (Theme.swift), for one appearance.
pub struct K {
    pub dark: bool,
    pub text: Color,
    pub muted: Color,
    pub faint: Color,
    pub accent: Color,
    pub line2: Color,
    pub separator: Color,
    pub hover: Color,
    pub recessed: Color,
    pub card: Color,
    pub progress: Color,
    pub error: Color,
    pub working: Color,
    /// Scroll bars always show ("Show scroll bars: Always", or a mouse): AppKit's legacy
    /// scroller, in a gutter of its own, rather than an overlay.
    pub legacy_bars: bool,
}

fn hex(h: &str) -> Color {
    rune::color::hex(h).unwrap_or(Color::WHITE)
}

pub fn k(dark: bool) -> K {
    let pick = |light: &str, dark_: &str| hex(if dark { dark_ } else { light });
    K {
        dark,
        text: pick("#18181b", "#f5f5f5"),
        muted: pick("#737373", "#a1a1a1"),
        faint: pick("#a1a1a1", "#737373"),
        accent: pick("#056dff", "#2f7dff"),
        line2: pick("#d9d9d9", "#333333"),
        separator: if dark { Color::WHITE.alpha(0.1) } else { Color::BLACK.alpha(0.1) },
        hover: if dark { Color::WHITE.alpha(0.05) } else { Color::BLACK.alpha(0.04) },
        recessed: if dark { Color::BLACK.alpha(0.25) } else { Color::BLACK.alpha(0.04) },
        // What the material looks like over a plain desktop: the fill a
        // headless render (or an opaque window) draws in its place.
        card: pick("#eceef1", "#252628"),
        progress: pick("#e39f08", "#f2bb3b"),
        error: pick("#e7000b", "#ff6467"),
        working: pick("#009966", "#00d492"),
        legacy_bars: false,
    }
}

/// The wheel: orange, or blue on a second hub.
fn logo(s: &Panel) -> El {
    let dev = s.world.as_ref().is_some_and(|w| w.flavor.is_some());
    icon(if dev { &icons::LOGO_DEV } else { &icons::LOGO }, 16.0).a11y_hidden()
}

/// A row's key: ids can hold `/`, which a key path would split on.
pub fn row_key(id: &str) -> String {
    format!("row:{}", id.replace('/', "∕"))
}

pub fn view(s: &Panel, cx: &mut Cx<Panel>) -> El {
    let mut k = k(s.theme.unwrap_or(cx.system_dark()));
    k.legacy_bars = !cx.overlay_scroll_bars();
    let (header, body) = match &s.mode {
        Mode::Palette => palette(s, &k),
        Mode::NewDocket { .. } => new_docket(s, &k),
        Mode::StartAgent { .. } => start_agent(s, &k),
        Mode::Linear { .. } => linear(s, &k),
    };
    // Over the material, a tint: Rune's materials are Glass and Popover, and Popover is
    // lighter than SwiftUI's .regularMaterial, most of all in dark. A headless render draws
    // the card's fill instead of either.
    let tint = if s.headless {
        Color::TRANSPARENT
    } else if k.dark {
        hex("#18191b").alpha(0.75)
    } else {
        Color::WHITE.alpha(0.35)
    };
    let mut content =
        col().key("content").w_full().radius(14.0).continuous_corners().fill(tint).child(header).child(el().h(1.0).w_full().fill(k.separator).a11y_hidden()).child(body);
    if let Some(n) = &s.note {
        content = content.child(note(n, &k));
    }
    let card = col()
        .key("card")
        .w(CARD_W)
        .radius(14.0)
        .continuous_corners()
        .system_material(SystemMaterial::Popover)
        .fill(k.card)
        .border(1.0, k.line2)
        .box_shadow(Shadow::core_animation(0.0, 12.0, 30.0, Color::BLACK.alpha(0.25)))
        // No role of its own: the panel is already a window ("Wheelhouse"), and a dialog
        // inside it reaches VoiceOver as a second, nested AXWindow.
        .child(content);
    col().key("root").w_full().h_full().padding(24.0).child(card)
}

fn palette(s: &Panel, k: &K) -> (El, El) {
    let list = s.shown();
    let selected = list.get(s.sel).map(|c| row_key(&c.id)).unwrap_or_default();
    let field = text_input(&s.query, FIELD)
        .key("query")
        .single_line()
        .grow(1.0)
        .color(k.text)
        .placeholder("Type a command or search dockets…", k.muted)
        .label("Search Wheelhouse")
        .active_descendant(selected)
        .on_edit(|s: &mut Panel| &mut s.query)
        .on_input_key(|s: &mut Panel, ev, tx| s.palette_key(ev, tx))
        .on_input_change(|s: &mut Panel, _| s.sel = 0)
        .on_submit(|s: &mut Panel, tx| s.run_selected(tx));
    let header = row().key("header").padding(14.0).gap(10.0).center_y().child(icon(&icons::SEARCH, 14.0).color(k.muted).a11y_hidden()).child(logo(s));
    // On a second hub, its badge: DEV in white on blue, as Wheelhouse Dev wears it.
    let header = match s.world.as_ref().and_then(|w| w.flavor.as_deref()) {
        Some(f) => header.child(
            row()
                .key("flavor")
                .h(15.0)
                .px(6.0)
                .radius(7.5)
                .center()
                .fill(hex("#2f7dff"))
                .label(format!("Wheelhouse {}", if f == "dev" { "Dev" } else { f }))
                .child(text(f.to_uppercase()).font(TextStyle::new(9.5, 800.0).family(SYS).lh(12.0).tracking(0.6)).color(Color::WHITE)),
        ),
        None => header,
    };
    let header = header.child(field);

    // The rows are the scroll view's own children, as Rune's gallery has them:
    // a content column inside it would shrink to the viewport.
    let mut rows = col()
        .key("rows")
        .w_full()
        .h(LIST_H)
        .pl(6.0)
        .pr(if k.legacy_bars { GUTTER + 6.0 } else { 6.0 })
        .pb(8.0)
        .scroll(ScrollSpec { stick_bottom: false })
        .role("listbox")
        .label("Commands and dockets");
    if let Some(e) = &s.error {
        rows = rows.child(text(format!("{e} ({})", s.hub.base())).font(ROW).color(k.error).px(8.0).pt(12.0).pb(4.0));
    }
    if list.is_empty() {
        rows = rows.child(text("No results").font(ROW).color(k.muted).padding(10.0));
    }
    let n = list.len();
    for (i, c) in list.iter().enumerate() {
        if i == 0 || list[i - 1].group != c.group {
            rows = rows.child(text(c.group).font(GROUP).color(k.muted).px(8.0).pt(10.0).pb(4.0).role("heading").level(2));
        }
        rows = rows.child(command_row(s, c, i, n, k));
    }
    (header, bar(rows, k))
}

/// A legacy scroller's gutter.
const GUTTER: f32 = 15.0;

/// The list's scroll bar: rune-kit's, which follows "Show scroll bars". Shown always, it
/// takes a gutter and a darker, wider thumb, as AppKit's legacy scroller does; overlaid, the
/// kit's own.
fn bar(viewport: El, k: &K) -> El {
    let kit = Kit::from_palette(PaletteSeed { bg: k.card, fg: k.text, accent: k.accent });
    if !k.legacy_bars {
        return viewport.scroll_bar(&kit);
    }
    let (thumb, hover) = if k.dark { (Color::WHITE.alpha(0.33), Color::WHITE.alpha(0.45)) } else { (Color::BLACK.alpha(0.36), Color::BLACK.alpha(0.5)) };
    viewport.scroll_bar_styled(ScrollBarStyle::from_kit(&kit).thumb(thumb).thumb_hover(hover).width(7.0).hover_width(9.0))
}

fn command_row(s: &Panel, c: &Command, i: usize, n: usize, k: &K) -> El {
    let on = i == s.sel;
    let busy = s.pending.contains(&c.id);
    let id = c.id.clone();
    let hint = if busy {
        Some(text("Sending…").font(HINT).color(k.muted))
    } else {
        c.hint.as_ref().map(|h| {
            let t = text(h.clone()).font(if c.group == DOCKETS { HINT_MONO } else { HINT }).color(k.muted);
            // A shortcut is the row's `shortcut_hint`; read once, not twice.
            if c.group == DOCKETS { t } else { t.a11y_hidden() }
        })
    };
    let mut r = row()
        .key(row_key(&c.id))
        .w_full()
        .px(14.0)
        .py(7.0)
        .gap(10.0)
        .center_y()
        .radius(7.0)
        .fill(if on { k.accent.alpha(0.14) } else { Color::TRANSPARENT })
        .hover(k.hover)
        .opacity(if busy { 0.55 } else { 1.0 })
        .role("option")
        .label(c.label.clone())
        .focus_selected(on)
        .position_in_set(i + 1, n)
        .on_press_tx(move |s: &mut Panel, tx| s.run_id(&id, tx))
        .child(mark(c.mark, k))
        .child(text(c.label.clone()).font(ROW).color(k.text).truncate().shrink());
    if let Mark::Status(st) = c.mark {
        r = r.description(st.label());
    }
    if c.group == NEEDS_YOU || c.group == CREATE {
        r = r.description(c.group);
    }
    r = r.child(spacer());
    if let Some(h) = hint {
        r = r.child(h);
    }
    if let Some(h) = &c.hint
        && c.group != DOCKETS
    {
        r = r.shortcut_hint(h.clone());
    }
    r
}

/// The 18 pt mark at a row's start.
fn mark(m: Mark, k: &K) -> El {
    let glyph = |i: &'static rune::Icon| icon(i, 15.0).color(k.text);
    let inner = match m {
        Mark::Approve => glyph(&icons::APPROVE),
        Mark::Deny => glyph(&icons::DENY),
        Mark::Reply => glyph(&icons::REPLY),
        Mark::Post => glyph(&icons::POST),
        Mark::New => glyph(&icons::NEW),
        Mark::Agent => glyph(&icons::AGENT),
        Mark::Linear => glyph(&icons::LINEAR),
        Mark::System => glyph(&icons::SYSTEM),
        Mark::Light => glyph(&icons::LIGHT),
        Mark::Dark => glyph(&icons::DARK),
        Mark::Status(st) => status(st, k),
    };
    row().w(18.0).h(16.0).center().a11y_hidden().child(inner)
}

/// A docket's status, as Linear draws it (StatusIcon).
pub fn status(st: Status, k: &K) -> El {
    match st {
        Status::Todo => icon(&icons::RING, 14.0).color(k.faint),
        Status::InProgress => icon(&icons::HALF, 14.0).color(k.progress),
        Status::Done => el().size(13.0, 13.0).radius(6.5).fill(k.accent).center().child(icon(&icons::TICK, 13.0).color(Color::WHITE)),
        Status::Dropped => el().size(13.0, 13.0).radius(6.5).fill(k.faint).center().child(icon(&icons::CROSS, 13.0).color(Color::WHITE)),
    }
}

/// What the last thing done came to.
fn note(n: &crate::Note, k: &K) -> El {
    row()
        .key("note")
        .w_full()
        .px(14.0)
        .py(9.0)
        .gap(8.0)
        .center_y()
        .role("status")
        .live_region(rune::LiveRegion::Polite)
        .child(el().size(6.0, 6.0).radius(3.0).fill(if n.error { k.error } else { k.working }).a11y_hidden())
        .child(text(n.text.clone()).font(SMALL).color(if n.error { k.error } else { k.muted }).truncate().shrink())
}

fn button(key: &str, label: &str, primary: bool, enabled: bool, k: &K, on: impl Fn(&mut Panel, &mut rune::Tx) + 'static) -> El {
    let mut b = row()
        .key(key.to_string())
        .h(24.0)
        .px(11.0)
        .radius(6.0)
        .center()
        .fill(if primary {
            k.accent
        } else if k.dark {
            Color::WHITE.alpha(0.1)
        } else {
            Color::WHITE
        })
        .border(if primary { 0.0 } else { 0.5 }, k.line2)
        .hover(if primary { Color::WHITE.alpha(0.08) } else { k.hover })
        .role("button")
        .label(label.to_string())
        .focusable(true)
        .opacity(if enabled { 1.0 } else { 0.45 })
        .child(text(label.to_string()).font(BUTTON).color(if primary { Color::WHITE } else { k.text }));
    if enabled {
        b = b.on_press_tx(on);
    } else {
        b = b.disabled(true);
    }
    b
}

fn form_field(edit: &rune::TextEdit, key: &str, placeholder: &str, k: &K) -> El {
    text_input(edit, LABEL)
        .key(key.to_string())
        .single_line()
        .grow(1.0)
        .color(k.text)
        .placeholder(placeholder.to_string(), k.muted)
        .label(placeholder.to_string())
        .on_input_key(|s: &mut Panel, ev, tx| s.form_key(ev, tx))
        .on_submit(|s: &mut Panel, tx| s.submit(tx))
}

fn boxed(field: El, k: &K) -> El {
    row().w_full().px(8.0).py(5.0).radius(6.0).fill(k.recessed).border(0.5, k.line2).child(field)
}

fn new_docket(s: &Panel, k: &K) -> (El, El) {
    let Mode::NewDocket { title, note } = &s.mode else { unreachable!() };
    let busy = s.pending.contains("create");
    let field = text_input(title, FIELD.family(SYS))
        .key("title")
        .single_line()
        .grow(1.0)
        .color(k.text)
        .placeholder("Docket title", k.muted)
        .label("Docket title")
        .on_edit(|s: &mut Panel| match &mut s.mode {
            Mode::NewDocket { title, .. } => title,
            _ => &mut s.query,
        })
        .on_input_key(|s: &mut Panel, ev, tx| s.form_key(ev, tx))
        .on_submit(|s: &mut Panel, tx| s.submit(tx));
    let header = row().key("header").padding(14.0).gap(10.0).center_y().child(icon(&icons::NEW, 15.0).color(k.muted).a11y_hidden()).child(field);
    let note_field = text_input(note, LABEL)
        .key("note")
        .w_full()
        .min_h(72.0)
        .color(k.text)
        .placeholder("Add a note… (Markdown, optional)", k.muted)
        .label("Note")
        .on_edit(|s: &mut Panel| match &mut s.mode {
            Mode::NewDocket { note, .. } => note,
            _ => &mut s.query,
        })
        .on_input_key(|s: &mut Panel, ev, tx| s.form_key(ev, tx));
    let ready = !title.text.trim().is_empty() && !busy;
    let body = col().key("form").w_full().padding(14.0).gap(12.0).child(row().w_full().px(8.0).py(6.0).radius(6.0).fill(k.recessed).border(0.5, k.line2).child(note_field)).child(
        row()
            .w_full()
            .gap(8.0)
            .center_y()
            .child(text("Signed as you, on the record.").font(SMALL).color(k.muted))
            .child(spacer())
            .child(button("cancel", "Cancel", false, true, k, |s, _| s.back()))
            .child(button("create", if busy { "Creating…" } else { "Create docket" }, true, ready, k, |s, tx| s.submit(tx))),
    );
    (header, body)
}

fn start_agent(s: &Panel, k: &K) -> (El, El) {
    let Mode::StartAgent { prompt, folder, codex, title } = &s.mode else { unreachable!() };
    let busy = s.pending.contains("start");
    let field = text_input(prompt, FIELD)
        .key("prompt")
        .grow(1.0)
        .color(k.text)
        .placeholder("What should the agent do?", k.muted)
        .label("The agent's first prompt")
        .on_edit(|s: &mut Panel| match &mut s.mode {
            Mode::StartAgent { prompt, .. } => prompt,
            _ => &mut s.query,
        })
        .on_input_key(|s: &mut Panel, ev, tx| s.form_key(ev, tx))
        .on_submit(|s: &mut Panel, tx| s.submit(tx));
    let header = row().key("header").padding(14.0).gap(10.0).child(el().pt(2.0).child(icon(&icons::AGENT, 15.0).color(k.muted)).a11y_hidden()).child(field);
    let label = |t: &str| text(t.to_string()).font(LABEL).color(k.muted).w(52.0);
    let folder_field = form_field(folder, "folder", "~/git/project", k).on_edit(|s: &mut Panel| match &mut s.mode {
        Mode::StartAgent { folder, .. } => folder,
        _ => &mut s.query,
    });
    let title_field = form_field(title, "agent-title", "Optional: named from the prompt otherwise", k).on_edit(|s: &mut Panel| match &mut s.mode {
        Mode::StartAgent { title, .. } => title,
        _ => &mut s.query,
    });
    let seg = |key: &str, name: &str, on: bool, pick: bool| {
        row()
            .key(key.to_string())
            .h(22.0)
            .px(10.0)
            .radius(5.0)
            .center()
            .fill(if on { if k.dark { Color::WHITE.alpha(0.16) } else { Color::WHITE } } else { Color::TRANSPARENT })
            .role("radio")
            .label(name.to_string())
            .checked(on)
            .focusable(true)
            .on_press(move |s: &mut Panel| {
                if let Mode::StartAgent { codex, .. } = &mut s.mode {
                    *codex = pick;
                }
            })
            .child(text(name.to_string()).font(LABEL).color(k.text))
    };
    let agents = row()
        .key("agents")
        .padding(2.0)
        .gap(2.0)
        .radius(7.0)
        .fill(k.recessed)
        .role("radiogroup")
        .label("Agent")
        .focus_group()
        .child(seg("claude", "Claude Code", !codex, false))
        .child(seg("codex", "Codex", *codex, true));
    let mut folders = row().key("folders").gap(6.0).pl(62.0);
    for (i, f) in s.world.as_ref().map(|w| w.folders.clone()).unwrap_or_default().into_iter().take(3).enumerate() {
        let short = f.rsplit('/').next().unwrap_or(&f).to_string();
        folders = folders.child(
            row()
                .key(format!("folder-{i}"))
                .h(20.0)
                .px(8.0)
                .radius(10.0)
                .center()
                .border(0.5, k.line2)
                .hover(k.hover)
                .role("button")
                .label(format!("Use {f}"))
                .on_press(move |s: &mut Panel| {
                    if let Mode::StartAgent { folder, .. } = &mut s.mode {
                        folder.replace_text(&f);
                    }
                })
                .child(text(short).font(SMALL).color(k.muted)),
        );
    }
    let ready = !folder.text.trim().is_empty() && !busy;
    let body = col()
        .key("form")
        .w_full()
        .padding(14.0)
        .gap(10.0)
        .child(row().w_full().gap(10.0).center_y().child(label("Folder")).child(boxed(folder_field, k)))
        .child(folders)
        .child(row().w_full().gap(10.0).center_y().child(label("Agent")).child(agents))
        .child(row().w_full().gap(10.0).center_y().child(label("Title")).child(boxed(title_field, k)))
        .child(
            row()
                .w_full()
                .gap(8.0)
                .pt(4.0)
                .center_y()
                .child(text("A new tab in Rex's wheelhouse session, named after the docket.").font(SMALL).color(k.muted).truncate().shrink())
                .child(spacer())
                .child(button("cancel", "Cancel", false, true, k, |s, _| s.back()))
                .child(button("start", if busy { "Starting…" } else { "Start agent" }, true, ready, k, |s, tx| s.submit(tx))),
        );
    (header, body)
}

fn linear(s: &Panel, k: &K) -> (El, El) {
    let Mode::Linear { query, issues, error, sel, loading } = &s.mode else { unreachable!() };
    let busy = s.pending.contains("from-linear");
    let selected = if issues.is_empty() { String::new() } else { format!("issue-{sel}") };
    let field = text_input(query, FIELD)
        .key("linear-query")
        .single_line()
        .grow(1.0)
        .color(k.text)
        .placeholder("Search Linear, or paste a key (ENG-142)…", k.muted)
        .label("Search Linear")
        .active_descendant(selected)
        .on_edit(|s: &mut Panel| match &mut s.mode {
            Mode::Linear { query, .. } => query,
            _ => &mut s.query,
        })
        .on_input_key(|s: &mut Panel, ev, tx| s.linear_key(ev, tx))
        .on_input_change(|s: &mut Panel, _| s.search_linear())
        .on_submit(|s: &mut Panel, _| {
            if let Mode::Linear { sel, .. } = &s.mode {
                let i = *sel;
                s.pick_issue(i);
            }
        });
    let header = row().key("header").padding(14.0).gap(10.0).center_y().child(icon(&icons::LINEAR, 15.0).color(k.muted).a11y_hidden()).child(field);
    let mut rows = col()
        .key("issues")
        .w_full()
        .h(320.0)
        .pl(6.0)
        .pr(if k.legacy_bars { GUTTER + 6.0 } else { 6.0 })
        .py(6.0)
        .scroll(ScrollSpec { stick_bottom: false })
        .role("listbox")
        .label("Linear issues");
    if let Some(e) = error {
        rows = rows.child(text(e.clone()).font(ROW).color(k.error).padding(8.0).w_full());
    } else if !loading && issues.is_empty() {
        let none = if query.text.is_empty() { "No open issues assigned to you." } else { "No issues match." };
        rows = rows.child(text(none).font(ROW).color(k.muted).padding(8.0));
    }
    let n = issues.len();
    for (i, issue) in issues.iter().enumerate() {
        let on = i == *sel;
        let mut r = row()
            .key(format!("issue-{i}"))
            .w_full()
            .px(8.0)
            .py(8.0)
            .gap(10.0)
            .center_y()
            .radius(7.0)
            .fill(if on { k.accent.alpha(0.14) } else { Color::TRANSPARENT })
            .hover(k.hover)
            .opacity(if busy { 0.55 } else { 1.0 })
            .role("option")
            .label(format!("{}: {}", issue.key, issue.title))
            .focus_selected(on)
            .position_in_set(i + 1, n)
            .on_press(move |s: &mut Panel| s.pick_issue(i))
            .child(row().w(18.0).center().a11y_hidden().child(status(issue.status, k)))
            .child(text(issue.key.clone()).font(HINT_MONO.lh(16.0)).color(k.muted).w(74.0))
            .child(text(issue.title.clone()).font(ROW).color(k.text).truncate().shrink())
            .child(spacer());
        if let Some(st) = &issue.state {
            r = r.description(st.clone()).child(text(st.clone()).font(HINT).color(k.muted));
        }
        rows = rows.child(r);
    }
    (header, bar(rows, k))
}
