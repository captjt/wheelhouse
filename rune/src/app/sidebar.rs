//! The sidebar (RootView.swift `Sidebar`): the places with what each holds,
//! the Rex sessions open dockets run in, and its foot: which hub, and the theme.

use rune::{El, Icon, col, el, row, spacer, text};
use rune_kit::Kit;
use rune_kit::shell::{Sidebar, SidebarRow, SidebarSection};

use super::{Main, Place, widgets};
use crate::icons;
use crate::model::{self, HubState};
use crate::theme::{self, K, font};

fn place_icon(p: &Place) -> &'static Icon {
    match p {
        Place::Inbox => &icons::TRAY,
        Place::Reviews => &icons::PULL,
        Place::Builds => &icons::HAMMER,
        Place::Active => &icons::DASHED,
        Place::All => &icons::STACK,
        Place::Done => &icons::APPROVE,
        Place::Agents => &icons::AGENT,
        Place::Recap => &icons::CHART,
        Place::Session(_) => &icons::LIST,
    }
}

pub fn sidebar(s: &Main, st: Option<&HubState>, k: &K, kit: &Kit) -> El {
    let count = |p: &Place| -> (usize, bool) {
        let Some(st) = st else { return (0, false) };
        match p {
            Place::Inbox => (model::inbox(st).len(), model::urgent(st) > 0),
            Place::Reviews => {
                let alerts = st.prs.iter().filter(|p| p.attention().is_some()).count();
                (if alerts > 0 { alerts } else { st.prs.iter().filter(|p| matches!(p.state().as_str(), "queued" | "reviewing")).count() }, alerts > 0)
            }
            Place::Builds => {
                let alerts = st.tasks.iter().filter(|t| t.attention().is_some()).count();
                (if alerts > 0 { alerts } else { st.tasks.iter().filter(|t| matches!(t.state().as_str(), "queued" | "active")).count() }, alerts > 0)
            }
            Place::Active => (st.dockets.iter().filter(|d| d.is_open()).count(), false),
            Place::Agents => (st.agents().len(), false),
            _ => (0, false),
        }
    };
    let mut sb = Sidebar::new().label("Places").inset(0.0).row_gap(1.0).header_gap(0.0).header_padding(0.0, 6.0);
    for p in Place::ORDER.iter() {
        let (n, alert) = count(p);
        let target = p.clone();
        let mut r = SidebarRow::new(p.title()).icon(place_icon(p)).height(28.0).selected(&s.place == p).on_select(move |s: &mut Main, _| s.go(target.clone()));
        if n > 0 {
            r = r.trailing(text(n.to_string()).font(font(11.5, if alert { 600.0 } else { 400.0 }).lh(14.0)).color(if alert { k.blocked } else { k.muted }));
        }
        sb = sb.child(r.build(kit).key(format!("place-{}", p.title().to_lowercase().replace(' ', "-"))));
    }
    if let Some(st) = st {
        let sessions = st.sessions();
        if !sessions.is_empty() {
            let rows = sessions.into_iter().map(|(name, n)| {
                let target = Place::Session(name.clone());
                let selected = s.place == target;
                SidebarRow::new(name.clone())
                    .avatar(theme::session_icon(&name, 16.0, k))
                    .height(28.0)
                    .selected(selected)
                    .trailing(text(n.to_string()).font(font(11.5, 400.0).lh(14.0)).color(k.muted))
                    .on_select(move |s: &mut Main, _| s.go(target.clone()))
                    .build(kit)
                    .key(format!("session-{name}"))
            });
            sb = sb.section(SidebarSection::new("Rex sessions").rows(rows));
        }
    }
    // Settings (⌘,), over the hub's status and the theme.
    let settings = SidebarRow::new("Settings").icon(&icons::GEAR).height(28.0).on_select(|s: &mut Main, _| super::settings::open(s.theme)).build(kit).key("open-settings");
    sb.footer(settings).footer(foot(s, st, k)).build(kit)
}

/// Under the sidebar: which hub, how it's connected, and the theme.
fn foot(s: &Main, st: Option<&HubState>, k: &K) -> El {
    let mut c = col().key("sidebar-foot").w_full().gap(6.0).px(6.0).pt(6.0).pb(4.0);
    if let Some(build) = st.and_then(|st| st.flavor_build()) {
        c = c.child(row().gap(6.0).center_y().child(dev_badge(k)).child(text(build).font(theme::mono(11.0).lh(14.0)).color(k.muted).truncate().shrink()));
    }
    // What's off, with the command that turns it on (Dev has them off on purpose).
    if let Some(st) = st.filter(|st| st.flavor().is_none()) {
        if !st.hooks() {
            c = c.child(hint(
                "hint-hooks",
                &icons::LINK,
                "Hooks off",
                "wh hooks install",
                "Claude Code's hooks are off, so dockets are named after their panes and turns carry no message.",
                k,
            ));
        }
        if !st.linear() {
            c = c.child(hint("hint-linear", &icons::LINEAR, "Linear off", "wh setup-linear", "Connect Linear to start dockets from issues and link issues to dockets.", k));
        }
    }
    let (dot, line) = match st {
        Some(st) if st.flavor().is_some() => (k.brand, "Nothing live".to_string()),
        Some(st) if st.rex_connected() && (s.store.live() || s.store.headless) => (k.working, "Following Rex".to_string()),
        Some(st) if st.rex_connected() => (k.blocked, if s.store.was_live() { "Reconnecting to the hub…".into() } else { "Connecting…".into() }),
        Some(st) => (k.blocked, format!("Rex: {}", st.rex_error().unwrap_or_else(|| "connecting".into()))),
        None => (k.error, "Hub unreachable".to_string()),
    };
    c.child(
        row()
            .w_full()
            .gap(6.0)
            .center_y()
            .child(el().size(7.0, 7.0).radius(3.5).fill(dot))
            .child(text(line).font(font(11.5, 400.0).lh(14.0)).color(k.muted).truncate().shrink())
            .child(spacer())
            .child(widgets::icon_segments(
                "theme",
                &[(&icons::SYSTEM, "System", s.theme.is_none()), (&icons::LIGHT, "Light", s.theme == Some(false)), (&icons::DARK, "Dark", s.theme == Some(true))],
                |s: &mut Main, i| {
                    s.theme = [None, Some(false), Some(true)][i];
                    super::settings::follow_theme(s.theme);
                },
                k,
            )),
    )
}

/// Something off, as a line that copies the command to turn it on.
fn hint(key: &str, i: &'static rune::Icon, title: &str, command: &'static str, why: &str, k: &K) -> El {
    row()
        .key(key.to_string())
        .w_full()
        .gap(6.0)
        .center_y()
        .role("button")
        .label(format!("{title}: turn on"))
        .help(format!("{why} Click to copy the command: {command}"))
        .focusable(true)
        .on_press_tx(move |s: &mut Main, tx| {
            tx.copy(command);
            s.toast(format!("Copied “{command}”. Run it in a terminal."), false);
        })
        .child(row().w(14.0).center().child(rune::icon(i, 12.0).color(k.muted)))
        .child(text(title.to_string()).font(font(11.5, 400.0).lh(14.0)).color(k.muted))
        .child(spacer())
        .child(text("Turn on").font(font(11.5, 400.0).lh(14.0)).color(k.accent))
}

/// DEV, as a nightly build marks itself.
pub fn dev_badge(k: &K) -> El {
    row()
        .px(5.0)
        .h(15.0)
        .radius(7.5)
        .center()
        .gradient(k.brand2, k.brand)
        .label("Wheelhouse Dev")
        .child(text("DEV").font(font(9.5, 800.0).lh(12.0).tracking(0.6)).color(rune::Color::WHITE))
}
