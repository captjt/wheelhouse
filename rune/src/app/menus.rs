//! A docket's menu (Lists.swift `DocketMenu`): a native NSMenu on a right-click
//! or the header's ⋯, with what you can do to it from anywhere.

use rune::{PopupAnchor, PopupItem, PopupMenu, Tx};

use super::{Main, NewMode, Sheet};
use crate::model::{Docket, Status};

pub fn docket_menu(d: &Docket) -> PopupMenu {
    let mut m = PopupMenu::new().item(PopupItem::new("window", "Open in New Window").symbol("macwindow"));
    if let Some(l) = d.live() {
        m = m.separator().item(PopupItem::new("rex", "Show Pane in Rex").symbol("terminal"));
        if l.answerable() {
            let what: String = l.msg().unwrap_or_default().chars().take(40).collect();
            m = m
                .item(PopupItem::new("approve", if what.is_empty() { "Approve".to_string() } else { format!("Approve: {what}") }).symbol("checkmark.circle"))
                .item(PopupItem::new("deny", "Deny").symbol("xmark.circle"));
        }
    }
    let mut status = PopupMenu::new();
    for s in Status::ALL {
        status = status.item(PopupItem::new(format!("status:{}", s.raw()), s.label()).checked(s == d.status()));
    }
    m = m.separator().submenu("Status", status);
    m = if d.hold().is_some() { m.item(PopupItem::new("resume", "Resume").symbol("play.circle")) } else { m.item(PopupItem::new("hold", "Hold…").symbol("pause.circle")) };
    m = m.item(PopupItem::new("checks", "Run Checks").symbol("checklist"));
    if d.is_open() && d.protocol() != "review" && d.live().is_none() {
        m = m.item(PopupItem::new("start", "Start an Agent…").symbol("terminal"));
    }
    m = m.item(PopupItem::new("link", "Link a Linear Issue…").symbol("circle.hexagongrid"));
    m.separator()
        .item(PopupItem::new("copy-id", "Copy ID"))
        .item(PopupItem::new("copy-link", "Copy Link"))
        .item(PopupItem::new("copy-head", "Copy Record Head").enabled(!d.short_head().is_empty()))
}

/// Open it at `anchor`, and do what's picked.
pub fn open(d: &Docket, anchor: impl Into<PopupAnchor>, tx: &mut Tx) {
    let d2 = d.clone();
    tx.popup_menu(docket_menu(d), anchor, move |s: &mut Main, picked, tx| {
        if let Some(p) = picked {
            pick(s, &d2, p, tx);
        }
    });
}

fn pick(s: &mut Main, d: &Docket, id: &str, tx: &mut Tx) {
    let did = d.id();
    match id {
        "window" => s.open_window(&did),
        "rex" => s.open_in_rex(&did),
        "approve" => s.answer(d, true),
        "deny" => s.answer(d, false),
        "resume" => s.resume(&did),
        "hold" => s.sheet = Some(Sheet::Hold(did)),
        "checks" => s.run_checks(&did),
        "start" => s.open_form(Sheet::StartAgent(did)),
        "link" => s.open_form(Sheet::LinkIssue(did)),
        "copy-id" => tx.copy(did.clone()),
        "copy-link" => tx.copy(format!("{}://docket/{did}", if s.dev() { "wheelhouse-dev" } else { "wheelhouse" })),
        "copy-head" => tx.copy(format!("Docket: {did} @ {}", d.short_head())),
        p if p.starts_with("status:") => {
            let st = Status::parse(&p["status:".len()..]);
            if st != d.status() {
                s.set_status(&did, st);
            }
        }
        _ => {}
    }
    let _ = NewMode::Blank;
}
