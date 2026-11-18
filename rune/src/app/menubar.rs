//! The menu bar's wheel and its panel (MenuBar.swift: MenuBarLabel and
//! MenuBarPanel), and the Dock icon's badge (Badge, in Store.swift).
//!
//! The wheel is an NSStatusItem (Rune has none: the app owns its tray, Rune
//! the window): a template image that follows the bar, orange with the count
//! beside it while something needs you, and on a flavored hub a blue wheel
//! and a DEV pill. A click shows the panel under it, a borderless panel of
//! Rune's that fits its content: what needs you, answerable in place, then
//! "Open Wheelhouse" and a menu with Quit. A click anywhere else puts it away.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use rune::{App, AppHandle, AppSpec, Color, Cx, DisplayTarget, El, Icon, Key, KeyEvent, OverlayPlacement, PopupItem, PopupMenu, ScrollSpec, SystemMaterial, Tx, WindowOptions, WindowSpec, col, el, icon, row, spacer, text};
use rune_kit::ScrollBar;

use super::{Main, NewMode, Sheet, Window, sidebar, windows};
use crate::icons;
use crate::model::{self, HubState, InboxItem, InboxKind, LiveState};
use crate::store::Store;
use crate::theme::{self, K, font};

/// The panel's window, for `--window menubar`.
pub const KEY: &str = "menubar";
const WIDTH: f32 = 380.0;
/// More than five scroll, in a list 460 pt tall; twelve at most.
const FIT: usize = 5;
const LIST_H: f32 = 460.0;
const MOST: usize = 12;

/// SF Symbols' `ellipsis.circle`.
static MORE: Icon = Icon {
    name: "more",
    view: 16.0,
    body: r#"<circle cx="8" cy="8" r="6.4" fill="none" stroke="currentColor" stroke-width="1.15"/><g fill="currentColor"><circle cx="5.1" cy="8" r="0.95"/><circle cx="8" cy="8" r="0.95"/><circle cx="10.9" cy="8" r="0.95"/></g>"#,
};

/// A scene's stand-in for what the hub says: nothing waiting, the hub
/// down, a flavored hub, or more than fit (headless shots only).
static FORCE: AtomicU8 = AtomicU8::new(0);
const EMPTY: u8 = 1;
const DOWN: u8 = 2;
const DEV: u8 = 3;
const MANY: u8 = 4;

/// The "…" menu is open: its tracking takes the panel's focus, and the
/// panel stays for it.
static MENU_OPEN: AtomicBool = AtomicBool::new(false);
/// When the panel last went away (ms since the epoch): a click on the wheel
/// that took its focus put it away already, and mustn't show it again.
static CLOSED_AT: AtomicU64 = AtomicU64::new(0);
/// The panel's height when it last showed, to place it at that size.
static HEIGHT: AtomicU32 = AtomicU32::new(0);

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// The panel's window: hidden until the wheel's clicked. In a window it also
/// puts the wheel in the menu bar and keeps it, and the Dock's badge, current.
pub fn spec(store: Arc<Store>) -> WindowSpec<Main> {
    let spec = AppSpec { scenes: SCENES, poll: Some(super::poll), on_key: Some(on_key), ..AppSpec::new("Wheelhouse", (WIDTH, 320.0), || unreachable!(), view) };
    let app = App::new(spec)
        .windowless()
        .fit_content_height()
        // As MenuBarExtra's window: click anywhere else and it goes.
        .on_focus(|_, focused, tx| {
            if !focused && !MENU_OPEN.load(Ordering::SeqCst) {
                tx.close_window();
            }
        })
        .on_close(|_, _| {
            CLOSED_AT.store(now_ms(), Ordering::SeqCst);
            item::highlight(false);
        });
    let me = app.handle();
    let init_me = me.clone();
    let init_store = store.clone();
    let mut app = app.init(move || {
        let mut m = Main::new(init_store, init_me);
        m.window = Window::MenuBar;
        m
    });
    if !store.headless {
        // The wheel goes in once the loop runs, on the main thread: the first
        // update, or the launcher's prepared window, whichever comes first.
        let (first, ready_me, sync_me) = (me.clone(), me.clone(), me.clone());
        me.update(move |s: &mut Main, _| {
            install(&first);
            sync(s);
        });
        app = app.on_ready(move |w| {
            remember(w);
            install(&ready_me);
            ready_me.update(|s: &mut Main, _| sync(s));
        });
        store.subscribe(move || sync_me.update(|s: &mut Main, _| sync(s)));
    }
    let options = WindowOptions::launcher_panel().macos(|native| native.corner_radius(10.0));
    WindowSpec::from_app(KEY, app).configured_options(options)
}

/// The wheel in the menu bar, once; a click on it toggles `panel`.
fn install(panel: &AppHandle<Main>) {
    let panel = panel.clone();
    item::install(move |at| toggle(&panel, at));
}

/// The wheel's count and look, and the Dock's badge, as the state is now.
fn sync(s: &mut Main) {
    let st = s.state();
    let n = st.as_deref().map(model::urgent).unwrap_or(0);
    let dev = st.as_deref().is_some_and(|st| st.flavor().is_some());
    item::set(n, dev);
    // The Dock's says nothing while the hub can't be read (Store.refresh).
    dock_badge(if s.store.error().is_some() { 0 } else { n });
}

/// The panel's native window, to read its height before it shows again.
fn remember(w: &rune::WindowHandle) {
    let _ = WINDOW.set(w.clone());
}
static WINDOW: std::sync::OnceLock<rune::WindowHandle> = std::sync::OnceLock::new();

/// A click on the wheel: the panel, under it; or away, if it shows.
fn toggle(panel: &AppHandle<Main>, at: item::Place) {
    if panel.is_visible() || now_ms().saturating_sub(CLOSED_AT.load(Ordering::SeqCst)) < 300 {
        panel.close();
        item::highlight(false);
        return;
    }
    // Under the wheel, its left edge with the wheel's, and on its screen.
    let [sx, _, sw, _] = at.screen;
    let x = at.item[0].min(sx + sw - WIDTH as f64 - 6.0).max(sx + 6.0);
    let y = at.item[1] + at.item[3] + 1.0;
    let h = WINDOW.get().and_then(item::height).map(|h| h as f32).unwrap_or(match HEIGHT.load(Ordering::Relaxed) {
        0 => 320.0,
        h => h as f32,
    });
    HEIGHT.store(h as u32, Ordering::Relaxed);
    panel.place(DisplayTarget::Primary, OverlayPlacement::Rect { x: x as f32, y: y as f32, width: WIDTH, height: h });
    panel.show();
    item::highlight(true);
}

/// Escape puts it away.
fn on_key(_: &mut Main, ev: &KeyEvent, tx: &mut Tx) -> bool {
    if matches!(ev.key, Key::Escape) {
        tx.close_window();
        return true;
    }
    false
}

// ---- the panel ----

fn view(s: &Main, cx: &mut Cx<Main>) -> El {
    let force = FORCE.load(Ordering::Relaxed);
    let dark = s.theme.unwrap_or(cx.system_dark());
    let state = s.state();
    let dev = force == DEV || state.as_ref().is_some_and(|st| st.flavor().is_some());
    let k = K::new(dark, dev);
    let down = force == DOWN || s.store.hub_down();
    let mut items = match (&state, force) {
        (_, EMPTY) => vec![],
        (Some(st), _) if !down => model::inbox(st),
        _ => vec![],
    };
    if force == MANY {
        let base = std::mem::take(&mut items);
        for i in 0..3 {
            items.extend(base.iter().cloned().map(|mut it| {
                it.id = format!("{}-{i}", it.id);
                it
            }));
        }
    }
    let name = if dev { "Wheelhouse Dev" } else { "Wheelhouse" };

    let mut header = row()
        .key("menubar-header")
        .w_full()
        .px(14.0)
        .py(11.0)
        .gap(8.0)
        .center_y()
        .child(theme::hub_avatar(20.0, &k))
        .child(text(name).font(font(13.0, 600.0).lh(16.0)).color(k.text).role("heading").level(1));
    if dev {
        header = header.child(sidebar::dev_badge(&k));
    }
    let count = if items.is_empty() { "All caught up".to_string() } else { format!("{} for you", items.len()) };
    header = header.child(spacer()).child(text(count).font(font(12.0, 400.0).lh(15.0)).color(k.muted));

    // No "Start the hub" here: the trial never starts one (docs/rune.md).
    let body = if down {
        let why = match s.store.error() {
            Some(e) if e != "The hub isn't running" => format!("{e}: {}", s.store.hub.base()),
            _ => format!("Nothing answers at {}", s.store.hub.base()),
        };
        col()
            .key("menubar-down")
            .w_full()
            .padding(20.0)
            .gap(6.0)
            .items_center()
            .child(text("The hub isn't running.").font(font(12.5, 400.0).lh(16.0)).color(k.text))
            .child(text(why).font(font(11.5, 400.0).lh(15.0)).color(k.muted).wrap().align(rune::Align::Center))
    } else if items.is_empty() {
        col()
            .key("menubar-empty")
            .w_full()
            .padding(22.0)
            .gap(6.0)
            .items_center()
            .child(icon(&icons::APPROVE, 22.0).color(k.working).a11y_hidden())
            .child(text("Nothing is waiting on you.").font(font(12.5, 400.0).lh(16.0)).color(k.muted))
    } else {
        let mut list = col().key("menubar-list").w_full().role("list").label("Needs you");
        for (i, it) in items.iter().take(MOST).enumerate() {
            if i > 0 {
                list = list.child(divider(&k, 0.5));
            }
            list = list.child(item_row(s, state.as_deref(), it, &k));
        }
        if items.len() > FIT {
            let kit = k.kit();
            col().w_full().h(LIST_H).child(list.h_full().scroll(ScrollSpec { stick_bottom: false }).scroll_bar(&kit))
        } else {
            list
        }
    };

    let tint = if s.store.headless {
        Color::TRANSPARENT
    } else if dark {
        theme::hex("#1d1e20").alpha(0.7)
    } else {
        Color::WHITE.alpha(0.3)
    };
    col()
        .key("root")
        .w_full()
        .radius(10.0)
        .continuous_corners()
        .system_material(SystemMaterial::Popover)
        .fill(if dark { theme::hex("#262729") } else { theme::hex("#ececee") })
        .border(1.0, if dark { Color::WHITE.alpha(0.13) } else { Color::BLACK.alpha(0.12) })
        .child(
            col()
                .w_full()
                .radius(10.0)
                .continuous_corners()
                .fill(tint)
                .child(header)
                .child(divider(&k, 1.0))
                .child(body)
                .child(divider(&k, 1.0))
                .child(foot(s, state.as_deref(), name, down, &k)),
        )
}

fn divider(k: &K, opacity: f32) -> El {
    el().h(1.0).w_full().fill(if k.dark { Color::WHITE.alpha(0.1 * opacity) } else { Color::BLACK.alpha(0.1 * opacity) }).a11y_hidden()
}

/// What needs you, and its answer (PanelItem): a click shows it in the main window.
fn item_row(s: &Main, st: Option<&HubState>, it: &InboxItem, k: &K) -> El {
    let mark = match it.kind {
        InboxKind::Error => LiveState::Error,
        InboxKind::Done => LiveState::Done,
        _ => LiveState::Blocked,
    };
    let top = row()
        .w_full()
        .gap(9.0)
        .child(el().pt(2.0).child(theme::live_icon(Some(mark), 13.0, k)))
        .child(
            col()
                .gap(2.0)
                .grow(1.0)
                .min_w(0.0)
                .child(
                    row()
                        .w_full()
                        .gap(4.0)
                        .child(text(it.title.clone()).font(font(12.5, 600.0).lh(15.0)).color(k.text).truncate().shrink())
                        .child(spacer())
                        .child(text(model::ago(it.at.as_deref())).font(font(11.0, 400.0).lh(15.0)).color(k.faint)),
                )
                .child(text(format!("{}  {}", it.docket_id, it.line)).font(font(11.5, 400.0).lh(14.0)).color(k.muted).truncate()),
        );
    let mut c = col().w_full().gap(6.0).child(top);
    if let Some(a) = actions(s, it, k) {
        c = c.child(a.pl(22.0));
    }
    let docket = it.docket.as_ref().map(|d| d.id()).filter(|id| st.is_some_and(|st| st.docket(id).is_some()));
    col()
        .key(format!("mb-{}", it.id.replace('/', "∕")))
        .w_full()
        .px(14.0)
        .py(9.0)
        .hover(if k.dark { Color::WHITE.alpha(0.06) } else { Color::BLACK.alpha(0.045) })
        .role("button")
        .label(format!("{}: {}", it.title, it.line))
        .on_press_tx(move |_: &mut Main, tx| {
            windows::show_in_main(docket.as_deref());
            tx.close_window();
        })
        .child(c)
}

/// The answer it takes here: Approve or Deny a prompt, an agent's options, a
/// checked review's Post (AnswerButtons, OptionButtons, ReviewActions, small).
fn actions(s: &Main, it: &InboxItem, k: &K) -> Option<El> {
    if let Some(p) = it.pr.as_ref().filter(|p| p.attention().is_some_and(|a| a.kind == "ready") && p.state() == "reviewing") {
        let full = p.full_key();
        let busy = |what: &str| s.busy.contains(&format!("{what}:{full}"));
        let (a, b) = (p.clone(), p.clone());
        let mut r = row().gap(6.0);
        if p.ready() {
            r = r.child(small_button(&format!("mb-post-{full}"), if busy("post") { "Posting…" } else { "Post" }, true, !busy("post") && !busy("dismiss"), k, move |s: &mut Main, _| s.post_review(&a)));
        }
        return Some(r.child(small_button(&format!("mb-dismiss-{full}"), if busy("dismiss") { "Dismissing…" } else { "Dismiss" }, false, !busy("post") && !busy("dismiss"), k, move |s: &mut Main, _| s.dismiss_review(&b))));
    }
    if let Some(m) = it.msg.as_ref().filter(|m| !m.options().is_empty()) {
        let busy = s.busy.contains(&format!("reply:{}", m.id()));
        let mut r = row().gap(6.0);
        for (i, o) in m.options().into_iter().enumerate() {
            let (m2, o2) = (m.clone(), o.clone());
            r = r.child(small_button(&format!("mb-opt-{}-{i}", m.id()), &o, i == 0, !busy, k, move |s: &mut Main, _| s.reply(&m2, &o2)));
        }
        return Some(r);
    }
    let d = it.docket.as_ref().filter(|d| it.kind == InboxKind::Blocked && d.live().is_some_and(|l| l.answerable()))?;
    let id = d.id();
    let (approving, denying) = (s.busy.contains(&format!("approve:{id}")), s.busy.contains(&format!("deny:{id}")));
    let (a, n) = (d.clone(), d.clone());
    Some(
        row()
            .gap(6.0)
            .child(small_button(&format!("mb-approve-{id}"), if approving { "Approving…" } else { "Approve" }, true, !approving && !denying, k, move |s: &mut Main, _| s.answer(&a, true)))
            .child(small_button(&format!("mb-deny-{id}"), if denying { "Denying…" } else { "Deny" }, false, !approving && !denying, k, move |s: &mut Main, _| s.answer(&n, false))),
    )
}

/// A small button, as macOS draws one over a menu's material: the accent's
/// when `primary`, else a quiet fill.
fn small_button(key: &str, label: &str, primary: bool, enabled: bool, k: &K, on: impl Fn(&mut Main, &mut Tx) + 'static) -> El {
    let (fill, ink) = match (primary, k.dark) {
        (true, _) => (k.accent, Color::WHITE),
        (false, true) => (Color::WHITE.alpha(0.12), k.text),
        (false, false) => (Color::BLACK.alpha(0.075), k.text),
    };
    let mut b = row()
        .key(key.to_string())
        .h(19.0)
        .px(9.0)
        .radius(5.0)
        .continuous_corners()
        .center()
        .fill(fill)
        .hover(if primary { Color::WHITE.alpha(0.12) } else if k.dark { Color::WHITE.alpha(0.06) } else { Color::BLACK.alpha(0.05) })
        .role("button")
        .label(label.to_string())
        .focusable(true)
        .opacity(if enabled { 1.0 } else { 0.5 })
        .child(text(label.to_string()).font(font(12.0, 400.0).lh(15.0)).color(ink));
    b = if enabled { b.on_press_tx(on).cursor(rune::Cursor::Pointer) } else { b.disabled(true) };
    b
}

/// Whether it follows Rex, then Open Wheelhouse and the "…" menu; or how the
/// last answer went, for a few seconds.
fn foot(s: &Main, st: Option<&HubState>, name: &str, down: bool, k: &K) -> El {
    let (dot, line) = match (s.toasts.last(), st) {
        (Some(t), _) => (if t.error { k.error } else { k.working }, t.text.clone()),
        (None, _) if down => (k.error, "Hub unreachable".to_string()),
        (None, Some(st)) if st.rex_connected() && (s.store.live() || s.store.headless) => (k.working, "Following Rex".to_string()),
        (None, Some(st)) if st.rex_connected() => (k.blocked, "Reconnecting to the hub…".to_string()),
        (None, Some(_)) => (k.blocked, "Rex: connecting".to_string()),
        (None, None) => (k.error, "Hub unreachable".to_string()),
    };
    let open = row()
        .key("menubar-open")
        .h(18.0)
        .px(6.0)
        .radius(5.0)
        .center()
        .hover(if k.dark { Color::WHITE.alpha(0.08) } else { Color::BLACK.alpha(0.06) })
        .role("button")
        .label(format!("Open {name}"))
        .focusable(true)
        .on_press_tx(|_: &mut Main, tx| {
            windows::show_main();
            tx.close_window();
        })
        .child(text(format!("Open {name}")).font(font(12.0, 400.0).lh(15.0)).color(k.text2));
    let more = row()
        .key("menubar-more")
        .size(20.0, 18.0)
        .radius(5.0)
        .center()
        .hover(if k.dark { Color::WHITE.alpha(0.08) } else { Color::BLACK.alpha(0.06) })
        .role("button")
        .label("More")
        .help("New Docket, Quit")
        .focusable(true)
        .on_press_tx(move |_: &mut Main, tx| menu(tx))
        .child(icon(&MORE, 15.0).color(k.text2));
    row()
        .key("menubar-foot")
        .w_full()
        .px(12.0)
        .py(6.5)
        .gap(4.0)
        .center_y()
        .child(el().size(6.0, 6.0).radius(3.0).fill(dot).a11y_hidden())
        .child(text(line).font(font(11.0, 400.0).lh(14.0)).color(k.muted).truncate().shrink())
        .child(spacer())
        .child(open)
        .child(more)
}

/// The "…" menu: a new docket or an agent in the main window, or Quit.
fn menu(tx: &mut Tx) {
    let quit = "Quit Wheelhouse";
    let m = PopupMenu::new()
        .item(PopupItem::new("new", "New Docket…"))
        .item(PopupItem::new("agent", "Start an Agent…"))
        .separator()
        .item(PopupItem::new("quit", quit).key_equivalent("cmd+q".parse().expect("a shortcut")));
    MENU_OPEN.store(true, Ordering::SeqCst);
    tx.popup_menu(m, "menubar-more", |_: &mut Main, picked, tx| {
        MENU_OPEN.store(false, Ordering::SeqCst);
        match picked {
            Some(which @ ("new" | "agent")) => {
                let mode = if which == "agent" { NewMode::Agent } else { NewMode::Blank };
                if let Some(m) = windows::main_window() {
                    m.update(move |s: &mut Main, _| s.open_form(Sheet::NewDocket(mode)));
                }
                windows::show_main();
                tx.close_window();
            }
            Some("quit") => tx.quit(),
            _ => {}
        }
    });
}

// ---- scenes, for headless shots: `--window menubar --scene empty` ----

type Scene = (&'static str, fn(&mut Main));

const SCENES: &[Scene] = &[
    ("inbox", |_| FORCE.store(0, Ordering::Relaxed)),
    ("empty", |_| FORCE.store(EMPTY, Ordering::Relaxed)),
    ("down", |_| FORCE.store(DOWN, Ordering::Relaxed)),
    ("dev", |_| FORCE.store(DEV, Ordering::Relaxed)),
    ("many", |_| FORCE.store(MANY, Ordering::Relaxed)),
];

// ---- the Dock's badge ----

/// What needs you, on the Dock icon; none at 0.
pub fn dock_badge(n: usize) {
    static SHOWN: AtomicU64 = AtomicU64::new(u64::MAX);
    if SHOWN.swap(n as u64, Ordering::SeqCst) == n as u64 {
        return;
    }
    #[cfg(target_os = "macos")]
    {
        use objc2_app_kit::NSApplication;
        use objc2_foundation::{MainThreadMarker, NSString};
        let Some(mtm) = MainThreadMarker::new() else { return };
        let label = (n > 0).then(|| NSString::from_str(&n.to_string()));
        // SAFETY: the app's own Dock tile, on the main thread.
        unsafe { NSApplication::sharedApplication(mtm).dockTile().setBadgeLabel(label.as_deref()) };
    }
}

// ---- the status item ----

#[cfg(target_os = "macos")]
mod item {
    //! The wheel in the menu bar: an NSStatusItem whose button draws
    //! MenuBarLabel's image, with the count as its title.

    use std::cell::{Cell, RefCell};

    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject, Bool, ClassBuilder, Sel};
    use objc2::{class, msg_send, msg_send_id, sel};
    use objc2_app_kit::{NSCellImagePosition, NSFont, NSImage, NSStatusBar, NSStatusItem};
    use objc2_foundation::{CGFloat, MainThreadMarker, NSPoint, NSRect, NSSize, NSString};

    /// Where the wheel is, and the visible part of its screen: top-down
    /// global points, `[x, y, width, height]`.
    pub struct Place {
        pub item: [f64; 4],
        pub screen: [f64; 4],
    }

    type Click = Box<dyn Fn(Place)>;

    thread_local! {
        static ITEM: RefCell<Option<Retained<NSStatusItem>>> = const { RefCell::new(None) };
        static TARGET: RefCell<Option<Retained<AnyObject>>> = const { RefCell::new(None) };
        static CLICK: RefCell<Option<Click>> = const { RefCell::new(None) };
        static SHOWN: Cell<Option<(usize, bool)>> = const { Cell::new(None) };
    }

    /// Orange, as the brand: something needs you.
    const ALERT: (f64, f64, f64) = (0.965, 0.51, 0.122);
    /// Wheelhouse Dev's blue.
    const DEV_BLUE: (f64, f64, f64) = (0.184, 0.49, 1.0);

    /// Put the wheel in the menu bar, once; `click` runs on each click.
    pub fn install(click: impl Fn(Place) + 'static) {
        let Some(mtm) = MainThreadMarker::new() else { return };
        if ITEM.with_borrow(|i| i.is_some()) {
            return;
        }
        // SAFETY: AppKit's status bar, on the main thread; the target and the
        // item live as long as the process, in this thread's locals.
        unsafe {
            let item = NSStatusBar::systemStatusBar().statusItemWithLength(-1.0);
            item.setAutosaveName(Some(&NSString::from_str("dev.wheelhouse.rune.wheel")));
            let target: Retained<AnyObject> = msg_send_id![target_class(), new];
            if let Some(b) = item.button(mtm) {
                b.setTarget(Some(&target));
                b.setAction(Some(sel!(clicked:)));
                // On the press, as a menu opens: left or right.
                // NSEventMaskLeftMouseDown | NSEventMaskRightMouseDown (an unsigned long long).
                let _: isize = msg_send![&b, sendActionOn: (1u64 << 1) | (1u64 << 3)];
            }
            ITEM.set(Some(item));
            TARGET.set(Some(target));
        }
        CLICK.set(Some(Box::new(click)));
    }

    /// The wheel for `n` needing you, on a flavored hub or not.
    pub fn set(n: usize, dev: bool) {
        let Some(mtm) = MainThreadMarker::new() else { return };
        if SHOWN.get() == Some((n, dev)) {
            return;
        }
        ITEM.with_borrow(|item| {
            let Some(b) = item.as_ref().and_then(|i| unsafe { i.button(mtm) }) else { return };
            SHOWN.set(Some((n, dev)));
            let name = if dev { "Wheelhouse Dev" } else { "Wheelhouse" };
            // SAFETY: the item's own button, on the main thread.
            unsafe {
                b.setImage(Some(&label(n > 0, dev)));
                let size = NSFont::menuBarFontOfSize(0.0).pointSize();
                b.setFont(Some(&NSFont::monospacedDigitSystemFontOfSize_weight(size, 0.0)));
                b.setTitle(&NSString::from_str(&if n > 0 { n.to_string() } else { String::new() }));
                b.setImagePosition(if n > 0 { NSCellImagePosition::NSImageLeft } else { NSCellImagePosition::NSImageOnly });
                let words = if n > 0 { format!("{name}: {n} need{} you", if n == 1 { "s" } else { "" }) } else { name.to_string() };
                let words = NSString::from_str(&words);
                b.setToolTip(Some(&words));
                let _: () = msg_send![&b, setAccessibilityLabel: &*words];
            }
        });
    }

    /// The wheel pressed while its panel shows, as a menu's title is.
    pub fn highlight(on: bool) {
        let Some(mtm) = MainThreadMarker::new() else { return };
        ITEM.with_borrow(|item| {
            if let Some(b) = item.as_ref().and_then(|i| unsafe { i.button(mtm) }) {
                // SAFETY: the item's own button, on the main thread.
                unsafe { b.highlight(on) };
            }
        });
    }

    /// The panel's height now, from its native window (once it has one).
    pub fn height(w: &rune::WindowHandle) -> Option<f64> {
        use rune::raw_window_handle::RawWindowHandle;
        w.with_raw_handle(|h| match h.as_raw() {
            RawWindowHandle::AppKit(a) => {
                let view = a.ns_view.as_ptr().cast::<AnyObject>();
                // SAFETY: the view Rune lends for the call, and its window.
                unsafe {
                    let window: *mut AnyObject = msg_send![view, window];
                    (!window.is_null()).then(|| {
                        let f: NSRect = msg_send![window, frame];
                        f.size.height
                    })
                }
            }
            _ => None,
        })
        .ok()
        .flatten()
        .filter(|h| *h > 1.0)
    }

    fn target_class() -> &'static AnyClass {
        const NAME: &str = "WheelhouseRuneWheelTarget";
        if let Some(c) = AnyClass::get(NAME) {
            return c;
        }
        let mut b = ClassBuilder::new(NAME, class!(NSObject)).expect("the wheel's target class");
        // SAFETY: the method's signature is the action's: (self, _cmd, sender).
        unsafe { b.add_method(sel!(clicked:), clicked as extern "C" fn(_, _, _)) };
        b.register()
    }

    extern "C" fn clicked(_this: *mut AnyObject, _cmd: Sel, sender: *mut AnyObject) {
        // SAFETY: the status bar button, on the main thread.
        let Some(at) = (unsafe { place(sender) }) else { return };
        CLICK.with_borrow(|f| {
            if let Some(f) = f {
                f(at)
            }
        });
    }

    /// The button's window and its screen, flipped to top-down from the
    /// primary screen's top-left (AppKit's are bottom-up).
    unsafe fn place(button: *mut AnyObject) -> Option<Place> {
        unsafe {
            let window: *mut AnyObject = msg_send![button, window];
            if window.is_null() {
                return None;
            }
            let frame: NSRect = msg_send![window, frame];
            let screen: *mut AnyObject = msg_send![window, screen];
            let visible: NSRect = if screen.is_null() { frame } else { msg_send![screen, visibleFrame] };
            let screens: *mut AnyObject = msg_send![class!(NSScreen), screens];
            let primary: *mut AnyObject = msg_send![screens, firstObject];
            let top = if primary.is_null() {
                0.0
            } else {
                let f: NSRect = msg_send![primary, frame];
                f.size.height
            };
            let flip = |r: NSRect| [r.origin.x, top - r.origin.y - r.size.height, r.size.width, r.size.height];
            Some(Place { item: flip(frame), screen: flip(visible) })
        }
    }

    /// MenuBarLabel's image: the wheel, 16 pt, and on a flavored hub DEV in
    /// a pill beside it, drawn by AppKit at whatever scale the bar has.
    pub(super) fn label(alert: bool, dev: bool) -> Retained<NSImage> {
        // SAFETY: AppKit drawing, on the main thread or in the image's handler.
        unsafe {
            let attrs = if dev { Some(pill_attributes()) } else { None };
            let text: Option<(Retained<NSString>, NSSize)> = attrs.as_ref().map(|a| {
                let s = NSString::from_str("DEV");
                let size: NSSize = msg_send![&s, sizeWithAttributes: &**a];
                (s, size)
            });
            let pill = text.as_ref().map(|(_, t)| NSSize::new(t.width.ceil() + 9.0, 13.0));
            let w = 16.0 + pill.map(|p| 3.0 + p.width).unwrap_or(0.0);
            let color = if alert { Some(ALERT) } else if dev { Some(DEV_BLUE) } else { None };
            let draw = RcBlock::new(move |r: NSRect| -> Bool {
                wheel(NSRect::new(NSPoint::new(0.0, (r.size.height - 16.0) / 2.0), NSSize::new(16.0, 16.0)), color);
                if let (Some(p), Some((s, t)), Some(a)) = (pill, &text, &attrs) {
                    let at = NSRect::new(NSPoint::new(19.0, (r.size.height - p.height) / 2.0), p);
                    ink(DEV_BLUE);
                    let path: *mut AnyObject = msg_send![class!(NSBezierPath), bezierPathWithRoundedRect: at, xRadius: p.height / 2.0, yRadius: p.height / 2.0];
                    let _: () = msg_send![path, fill];
                    let point = NSPoint::new(at.origin.x + (p.width - t.width) / 2.0, at.origin.y + (p.height - t.height) / 2.0 + 0.5);
                    let _: () = msg_send![&**s, drawAtPoint: point, withAttributes: &**a];
                }
                Bool::YES
            });
            let img: Retained<NSImage> = msg_send_id![class!(NSImage), imageWithSize: NSSize::new(w, 16.0), flipped: false, drawingHandler: &*draw];
            // Dev's stays blue in a light or dark bar; the daily one follows the bar.
            img.setTemplate(!alert && !dev);
            img
        }
    }

    unsafe fn ink((r, g, b): (f64, f64, f64)) {
        unsafe {
            let c: *mut AnyObject = msg_send![class!(NSColor), colorWithSRGBRed: r as CGFloat, green: g as CGFloat, blue: b as CGFloat, alpha: 1.0 as CGFloat];
            let _: () = msg_send![c, set];
        }
    }

    /// The Logo: a ring, a hub, six handles (MenuBarLabel.wheel).
    unsafe fn wheel(r: NSRect, color: Option<(f64, f64, f64)>) {
        unsafe {
            ink(color.unwrap_or((0.0, 0.0, 0.0)));
            let u = r.size.width / 32.0;
            let (cx, cy) = (r.origin.x + r.size.width / 2.0, r.origin.y + r.size.height / 2.0);
            let oval = |rad: f64| NSRect::new(NSPoint::new(cx - rad * u, cy - rad * u), NSSize::new(2.0 * rad * u, 2.0 * rad * u));
            let ring: *mut AnyObject = msg_send![class!(NSBezierPath), bezierPathWithOvalInRect: oval(10.5)];
            let _: () = msg_send![ring, setLineWidth: 3.4 * u];
            let _: () = msg_send![ring, stroke];
            let hub: *mut AnyObject = msg_send![class!(NSBezierPath), bezierPathWithOvalInRect: oval(3.6)];
            let _: () = msg_send![hub, fill];
            for i in 0..6 {
                let a = i as f64 * std::f64::consts::PI / 3.0 + std::f64::consts::PI / 2.0;
                let p: *mut AnyObject = msg_send![class!(NSBezierPath), bezierPath];
                let _: () = msg_send![p, moveToPoint: NSPoint::new(cx + a.cos() * 11.0 * u, cy + a.sin() * 11.0 * u)];
                let _: () = msg_send![p, lineToPoint: NSPoint::new(cx + a.cos() * 14.5 * u, cy + a.sin() * 14.5 * u)];
                let _: () = msg_send![p, setLineWidth: 3.0 * u];
                // NSLineCapStyleRound
                let _: () = msg_send![p, setLineCapStyle: 1usize];
                let _: () = msg_send![p, stroke];
            }
        }
    }

    /// DEV's look: 9 pt heavy, white, a little apart.
    unsafe fn pill_attributes() -> Retained<AnyObject> {
        unsafe {
            // NSFontWeightHeavy
            let font: *mut AnyObject = msg_send![class!(NSFont), systemFontOfSize: 9.0 as CGFloat, weight: 0.56 as CGFloat];
            let white: *mut AnyObject = msg_send![class!(NSColor), whiteColor];
            let kern: *mut AnyObject = msg_send![class!(NSNumber), numberWithDouble: 0.5f64];
            // NSFontAttributeName, NSForegroundColorAttributeName, NSKernAttributeName.
            let names = [NSString::from_str("NSFont"), NSString::from_str("NSColor"), NSString::from_str("NSKern")];
            let keys: Vec<*const AnyObject> = names.iter().map(|k| Retained::as_ptr(k).cast::<AnyObject>()).collect();
            let values: [*const AnyObject; 3] = [font, white, kern];
            msg_send_id![class!(NSDictionary), dictionaryWithObjects: values.as_ptr(), forKeys: keys.as_ptr(), count: 3usize]
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod item {
    pub struct Place {
        pub item: [f64; 4],
        pub screen: [f64; 4],
    }
    pub fn install(_: impl Fn(Place) + 'static) {}
    pub fn set(_: usize, _: bool) {}
    pub fn highlight(_: bool) {}
    pub fn height(_: &rune::WindowHandle) -> Option<f64> {
        None
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

    /// Each look of the wheel draws, offscreen at 4×, over a light bar or a
    /// dark one (nothing goes in the menu bar). `WH_RUNE_WHEEL_OUT=dir` keeps
    /// the PNGs.
    #[test]
    fn the_wheel_draws_in_each_look() {
        let out = std::env::var_os("WH_RUNE_WHEEL_OUT").map(std::path::PathBuf::from);
        for (alert, dev, name) in [(false, false, "wheel"), (true, false, "alert"), (false, true, "dev"), (true, true, "dev-alert")] {
            for (bar, bar_name) in [((0.92, 0.92, 0.92), "light"), ((0.16, 0.16, 0.17), "dark")] {
                // A template is the bar's to tint, so it's drawn (black) on the light one only.
                if !alert && !dev && bar_name == "dark" {
                    continue;
                }
                let img = super::item::label(alert, dev);
                // SAFETY: AppKit drawing into a bitmap of our own.
                unsafe {
                    let size: NSSize = msg_send![&img, size];
                    assert_eq!(size.height, 16.0);
                    assert!(if dev { size.width > 30.0 } else { size.width == 16.0 }, "{name}: {size:?}");
                    let (w, h) = (size.width + 8.0, 22.0);
                    let rep: *mut AnyObject = msg_send![class!(NSBitmapImageRep), alloc];
                    let rep: *mut AnyObject = msg_send![rep,
                        initWithBitmapDataPlanes: std::ptr::null_mut::<*mut u8>(),
                        pixelsWide: (w * 4.0) as isize,
                        pixelsHigh: (h * 4.0) as isize,
                        bitsPerSample: 8isize,
                        samplesPerPixel: 4isize,
                        hasAlpha: true,
                        isPlanar: false,
                        colorSpaceName: &*NSString::from_str("NSCalibratedRGBColorSpace"),
                        bytesPerRow: 0isize,
                        bitsPerPixel: 0isize];
                    assert!(!rep.is_null());
                    let _: () = msg_send![rep, setSize: NSSize::new(w, h)];
                    let ctx: *mut AnyObject = msg_send![class!(NSGraphicsContext), graphicsContextWithBitmapImageRep: rep];
                    let _: () = msg_send![class!(NSGraphicsContext), saveGraphicsState];
                    let _: () = msg_send![class!(NSGraphicsContext), setCurrentContext: ctx];
                    let c: *mut AnyObject = msg_send![class!(NSColor), colorWithSRGBRed: bar.0, green: bar.1, blue: bar.2, alpha: 1.0f64];
                    let _: () = msg_send![c, set];
                    let all: *mut AnyObject = msg_send![class!(NSBezierPath), bezierPathWithRect: NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(w, h))];
                    let _: () = msg_send![all, fill];
                    let at = NSRect::new(NSPoint::new(4.0, 3.0), size);
                    let _: () = msg_send![&img, drawInRect: at];
                    let _: () = msg_send![class!(NSGraphicsContext), restoreGraphicsState];
                    // Something drew: the ring's top, (12, 16.25) pt up from the bottom, isn't the bar.
                    let ring: *mut AnyObject = msg_send![rep, colorAtX: ((4.0 + 8.0) * 4.0) as isize, y: ((h - 3.0 - 8.0 - 5.25) * 4.0) as isize];
                    let (r, g, b): (f64, f64, f64) = (msg_send![ring, redComponent], msg_send![ring, greenComponent], msg_send![ring, blueComponent]);
                    assert!((r - bar.0).abs() + (g - bar.1).abs() + (b - bar.2).abs() > 0.3, "{name} on {bar_name}: the ring didn't draw ({r}, {g}, {b})");
                    if let Some(dir) = &out {
                        let none: *mut AnyObject = msg_send![class!(NSDictionary), dictionary];
                        let png: *mut AnyObject = msg_send![rep, representationUsingType: 4usize, properties: none];
                        let path = NSString::from_str(&dir.join(format!("wheel-{name}-{bar_name}.png")).to_string_lossy());
                        let ok: bool = msg_send![png, writeToFile: &*path, atomically: true];
                        assert!(ok);
                    }
                    let _: () = msg_send![rep, release];
                }
            }
        }
    }
}
