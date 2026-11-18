//! ⌘K in the main window (Palette.swift, over the window): the quick panel's
//! commands, plus the docket you're on (its pane, prompt, hold, checks,
//! status, record), the places to go, and the theme.

use rune::{Color, El, Icon, Key, KeyEvent, ScrollSpec, TextEdit, Tx, col, el, icon, row, spacer, text, text_input};

use super::{Main, NewMode, Place, Sheet};
use crate::commands::{self, Action, Command, Mark, World};
use crate::icons;
use crate::model::{HubState, Status};
use crate::theme::{self, K, font, mono};

#[derive(Default)]
pub struct Palette {
    pub query: TextEdit,
    pub sel: usize,
}

fn cmd(id: String, group: &'static str, label: String, mark: Mark, hint: Option<&str>, keywords: &str, key: String) -> Command {
    Command { id, group, label, mark, hint: hint.map(str::to_string), keywords: keywords.into(), action: Action::Main(key) }
}

/// Every command, the window's own first.
pub fn all(s: &Main, st: &HubState) -> Vec<Command> {
    let mut out = vec![];
    if let Some(d) = s.current().and_then(|id| st.docket(&id).cloned()) {
        let id = d.id();
        if d.live().is_some() {
            out.push(cmd("rex".into(), "This docket", "Show pane in Rex".into(), Mark::Glyph("terminal"), Some("O"), "screen terminal", format!("rex:{id}")));
        }
        if let Some(l) = d.live().filter(|l| l.answerable()) {
            out.push(cmd("approve".into(), "This docket", format!("Approve: {}", l.msg().unwrap_or_else(|| "the prompt".into())), Mark::Approve, None, "permission yes allow", format!("approve:{id}")));
            out.push(cmd("deny".into(), "This docket", "Deny the prompt".into(), Mark::Deny, None, "permission no reject", format!("deny:{id}")));
        }
        if d.hold().is_some() {
            out.push(cmd("resume".into(), "This docket", "Resume: take it off hold".into(), Mark::Glyph("play"), Some("⇧⌘H"), "hold unpause continue", format!("resume:{id}")));
        } else {
            out.push(cmd("hold".into(), "This docket", "Hold…".into(), Mark::Glyph("pause"), Some("⇧⌘H"), "pause stop wait build review", format!("hold:{id}")));
        }
        out.push(cmd("checks".into(), "This docket", "Run checks".into(), Mark::Glyph("checks"), Some("⇧⌘U"), "verify test lint witnessed", format!("checks:{id}")));
        for st_ in Status::ALL.into_iter().filter(|x| *x != d.status()) {
            out.push(cmd(format!("st-{}", st_.raw()), "This docket", format!("Move to {}", st_.label()), Mark::Status(st_), None, "status", format!("status:{id}:{}", st_.raw())));
        }
        out.push(cmd("rec".into(), "This docket", "Read the record".into(), Mark::Glyph("shield"), None, "verify signed chain", format!("record:{id}")));
        out.push(cmd("link".into(), "This docket", "Link a Linear issue…".into(), Mark::Linear, None, "linear issue", format!("link:{id}")));
    }
    let world = World::parse(&st.j);
    let shared: Vec<Command> = commands::commands(&world)
        .into_iter()
        .map(|mut c| {
            // The window's Create opens its own sheets, not the panel's forms.
            c.action = match c.action {
                Action::NewDocket => Action::Main("new:blank".into()),
                Action::StartAgent => Action::Main("new:agent".into()),
                Action::FromLinear => Action::Main("new:linear".into()),
                a => a,
            };
            c
        })
        .collect();
    let (dockets, rest): (Vec<_>, Vec<_>) = shared.into_iter().partition(|c| c.group == commands::DOCKETS);
    out.extend(rest);
    for (i, p) in Place::ORDER.iter().enumerate() {
        out.push(cmd(format!("go-{i}"), "Go to", p.title(), Mark::Glyph("go"), Some(["⌘1", "⌘2", "⌘3", "⌘4", "⌘5", "⌘6", "⌘7", "⌘8"][i]), "go place", format!("go:{i}")));
    }
    out.extend(dockets);
    out
}

pub fn shown(s: &Main, st: &HubState) -> Vec<Command> {
    let all = all(s, st);
    if s.palette.as_ref().is_none_or(|p| p.query.text.trim().is_empty()) {
        // Over the window, all but the dockets, then the first six of those.
        let (dockets, rest): (Vec<_>, Vec<_>) = all.into_iter().partition(|c| c.group == commands::DOCKETS);
        return rest.into_iter().chain(dockets.into_iter().take(6)).collect();
    }
    commands::shown(all, &s.palette.as_ref().map(|p| p.query.text.clone()).unwrap_or_default())
}

impl Main {
    pub fn open_palette(&mut self) {
        let mut p = Palette::default();
        p.query.focused = true;
        self.palette = Some(p);
    }

    fn run_command(&mut self, c: &Command, tx: &mut Tx) {
        self.palette = None;
        let st = self.state();
        match &c.action {
            Action::Answer { docket, approve, .. } => {
                if let Some(d) = st.as_ref().and_then(|s| s.docket(docket).cloned()) {
                    self.answer(&d, *approve);
                }
            }
            Action::Choose { id, choice } => {
                if let Some(m) = st.as_ref().and_then(|s| s.messages.iter().find(|m| m.id() == *id).cloned()) {
                    self.reply(&m, choice);
                }
            }
            Action::Post { full_key, .. } => {
                if let Some(p) = st.as_ref().and_then(|s| s.prs.iter().find(|p| &p.full_key() == full_key).cloned()) {
                    self.post_review(&p);
                }
            }
            Action::Open(id) => self.show(id),
            Action::Theme(t) => {
                self.theme = *t;
                tx.color_motion(rune::tok::motion::CALM);
            }
            Action::Main(key) => {
                let parts: Vec<&str> = key.split(':').collect();
                let d = parts.get(1).and_then(|id| st.as_ref().and_then(|s| s.docket(id).cloned()));
                match (parts[0], d) {
                    ("go", _) => {
                        if let Some(p) = parts.get(1).and_then(|i| i.parse::<usize>().ok()).and_then(|i| Place::ORDER.get(i)) {
                            self.go(p.clone());
                        }
                    }
                    ("new", _) => self.open_form(Sheet::NewDocket(match parts.get(1).copied() {
                        Some("agent") => NewMode::Agent,
                        Some("linear") => NewMode::Linear,
                        _ => NewMode::Blank,
                    })),
                    ("rex", Some(d)) => self.open_in_rex(&d.id()),
                    ("approve", Some(d)) => self.answer(&d, true),
                    ("deny", Some(d)) => self.answer(&d, false),
                    ("hold", Some(d)) => self.sheet = Some(Sheet::Hold(d.id())),
                    ("resume", Some(d)) => self.resume(&d.id()),
                    ("checks", Some(d)) => self.run_checks(&d.id()),
                    ("record", Some(d)) => self.sheet = Some(Sheet::Record(d.id())),
                    ("link", Some(d)) => self.open_form(Sheet::LinkIssue(d.id())),
                    ("status", Some(d)) => self.set_status(&d.id(), Status::parse(parts.get(2).unwrap_or(&""))),
                    _ => {}
                }
            }
            _ => {}
        }
    }
}

fn glyph(name: &str) -> &'static Icon {
    match name {
        "terminal" => &icons::AGENT,
        "play" => &icons::PLAY,
        "pause" => &icons::PAUSE,
        "checks" => &icons::APPROVE,
        "shield" => &icons::SHIELD,
        "go" => &icons::CHEVRON_RIGHT,
        _ => &icons::SEARCH,
    }
}

fn mark(m: Mark, k: &K) -> El {
    let i = |i: &'static Icon| icon(i, 15.0).color(k.text);
    let inner = match m {
        Mark::Approve => i(&icons::APPROVE),
        Mark::Deny => i(&icons::DENY),
        Mark::Reply => i(&icons::REPLY),
        Mark::Post => i(&icons::POST),
        Mark::New => i(&icons::NEW),
        Mark::Agent => i(&icons::AGENT),
        Mark::Linear => i(&icons::LINEAR),
        Mark::System => i(&icons::SYSTEM),
        Mark::Light => i(&icons::LIGHT),
        Mark::Dark => i(&icons::DARK),
        Mark::Glyph(n) => i(glyph(n)),
        Mark::Status(st) => theme::status_icon(st, 14.0, k),
    };
    row().w(18.0).h(16.0).center().a11y_hidden().child(inner)
}

fn palette_key(s: &mut Main, ev: &KeyEvent, tx: &mut Tx) -> bool {
    let Some(st) = s.state() else { return false };
    let n = shown(s, &st).len();
    let Some(p) = s.palette.as_mut() else { return false };
    match ev.key {
        Key::Down if n > 0 => p.sel = (p.sel + 1).min(n - 1),
        Key::Up if n > 0 => p.sel = p.sel.saturating_sub(1),
        Key::Escape => {
            s.palette = None;
            return true;
        }
        _ => return false,
    }
    let sel = p.sel;
    tx.scroll_into_view(format!("pal-{sel}"));
    true
}

pub fn view(s: &Main, st: &HubState, k: &K) -> El {
    let Some(p) = &s.palette else { return el() };
    let list = shown(s, st);
    let sel = p.sel.min(list.len().saturating_sub(1));
    let field = text_input(&p.query, font(16.0, 400.0).lh(18.0))
        .key("palette-query")
        .single_line()
        .grow(1.0)
        .color(k.text)
        .placeholder("Type a command or search dockets…", k.muted)
        .label("Search Wheelhouse")
        .active_descendant(if list.is_empty() { String::new() } else { format!("pal-{sel}") })
        .on_edit(|s: &mut Main| &mut s.palette.get_or_insert_with(Palette::default).query)
        .on_input_key(palette_key)
        .on_input_change(|s: &mut Main, _| {
            if let Some(p) = s.palette.as_mut() {
                p.sel = 0;
            }
        })
        .on_submit(|s: &mut Main, tx| {
            let Some(st) = s.state() else { return };
            let list = shown(s, &st);
            let i = s.palette.as_ref().map(|p| p.sel).unwrap_or(0);
            if let Some(c) = list.get(i).cloned() {
                s.run_command(&c, tx);
            }
        });
    let mut id_chip = el();
    if let Some(id) = s.current() {
        id_chip = row().px(6.0).py(2.0).radius(999.0).fill(k.recessed).child(text(id).font(mono(11.5).lh(14.0)).color(k.text2));
    }
    let header = row().key("palette-header").padding(14.0).gap(10.0).center_y().child(icon(&icons::SEARCH, 14.0).color(k.muted)).child(id_chip).child(field);
    let mut rows = col().key("palette-rows").w_full().px(6.0).pb(8.0).h(380.0).role("listbox").label("Commands and dockets").scroll(ScrollSpec { stick_bottom: false });
    if list.is_empty() {
        rows = rows.child(text("No results").font(font(13.0, 400.0).lh(16.0)).color(k.muted).padding(10.0));
    }
    for (i, c) in list.iter().enumerate() {
        if i == 0 || list[i - 1].group != c.group {
            rows = rows.child(text(c.group).font(font(11.0, 600.0).lh(14.0)).color(k.muted).px(8.0).pt(10.0).pb(4.0).role("heading").level(2));
        }
        let on = i == sel;
        let c2 = c.clone();
        let mut r = row()
            .key(format!("pal-{i}"))
            .w_full()
            .px(14.0)
            .py(7.0)
            .gap(10.0)
            .center_y()
            .radius(7.0)
            .fill(if on { k.accent.alpha(0.14) } else { Color::TRANSPARENT })
            .hover(k.hover)
            .role("option")
            .label(c.label.clone())
            .focus_selected(on)
            .on_press_tx(move |s: &mut Main, tx| s.run_command(&c2, tx))
            .child(mark(c.mark, k))
            .child(text(c.label.clone()).font(font(13.0, 400.0).lh(16.0)).color(k.text).truncate().shrink())
            .child(spacer());
        if let Some(h) = &c.hint {
            r = r.child(text(h.clone()).font(if c.group == commands::DOCKETS { mono(11.5).lh(14.0) } else { font(11.5, 400.0).lh(14.0) }).color(k.muted).a11y_hidden());
        }
        rows = rows.child(r);
    }
    let card = col()
        .key("palette")
        .w(600.0)
        .radius(14.0)
        .continuous_corners()
        .fill(if k.dark { theme::hex("#252628") } else { theme::hex("#f6f6f8") })
        .border(1.0, k.line2)
        .box_shadow(rune::Shadow::core_animation(0.0, 12.0, 30.0, Color::BLACK.alpha(0.25)))
        .child(header)
        .child(el().h(1.0).w_full().fill(if k.dark { Color::WHITE.alpha(0.1) } else { Color::BLACK.alpha(0.1) }))
        .child(rows);
    col()
        .key("palette-scrim")
        .raise()
        .absolute(0.0, 0.0)
        .w_full()
        .h_full()
        .fill(Color::BLACK.alpha(0.18))
        .items_center()
        .pt(90.0)
        .modal(true, |s: &mut Main| s.palette = None)
        .child(card)
}
