//! The windows beside the main one (App.swift): dockets in windows of their
//! own (DocketWindow, the "Docket" WindowGroup), the menu bar's panel, and the
//! way the panel, a notification or the Dock reach the main window.
//!
//! Rune makes every window at launch (`run_windows_with`'s list, or
//! `App::window` before `run`); none can be added while the app runs. So the
//! docket windows are a few made at launch, windowless until a docket opens
//! in one: the window already showing it comes forward, else a free one takes
//! it, else the one used longest ago does. A closed one is free again.

use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use rune::{App, AppHandle, AppSpec, Chrome, Cx, El, Key, KeyEvent, Tx, WindowSpec, col, el, row, text};
use rune_kit::shell::WindowTitlebar;

use super::{Main, Place, Window, docket, widgets};
use crate::icons;
use crate::model::HubState;
use crate::store::Store;
use crate::theme::{K, font};

/// How many dockets can show in windows of their own at once.
pub const DOCKET_WINDOWS: usize = 4;
/// SwiftUI's `.defaultSize(width: 1000, height: 840)`.
const DOCKET_SIZE: (f32, f32) = (1000.0, 840.0);
/// The title band's height: a SwiftUI window toolbar's.
const BAND: f32 = 52.0;

struct Slot {
    window: AppHandle<Main>,
    shows: Option<String>,
    used: Option<Instant>,
}

static MAIN: OnceLock<AppHandle<Main>> = OnceLock::new();
static SLOTS: Mutex<Vec<Slot>> = Mutex::new(Vec::new());

/// Every window but the main one and the quick panel: the menu bar's panel
/// and the docket windows. Starts the notifications.
pub fn more(store: &Arc<Store>, main: &AppHandle<Main>) -> Vec<rune::AnyWindowSpec> {
    let _ = MAIN.set(main.clone());
    super::notify::start(store.clone());
    let mut v: Vec<rune::AnyWindowSpec> = vec![super::menubar::spec(store.clone()).into()];
    v.extend((0..DOCKET_WINDOWS).map(|i| docket_window(store, i).into()));
    v
}

pub fn main_window() -> Option<AppHandle<Main>> {
    MAIN.get().cloned()
}

/// The main window, to the front: reopened if it was closed (Windows.showMain).
pub fn show_main() {
    if let Some(m) = MAIN.get() {
        m.open();
    }
}

/// The main window, to the front, showing a docket where it is (Nav.show),
/// or the Inbox for none.
pub fn show_in_main(docket: Option<&str>) {
    let Some(m) = MAIN.get() else { return };
    let id = docket.map(str::to_string);
    m.update(move |s: &mut Main, _| match id {
        Some(id) => s.show(&id),
        None => s.go(Place::Inbox),
    });
    m.open();
}

/// A toast in the main window: how an answer from the menu bar or a
/// notification went.
pub fn toast_in_main(text: String, error: bool) {
    if let Some(m) = MAIN.get() {
        m.update(move |s: &mut Main, _| s.toast(text, error));
    }
}

/// The main window's Dock click: with no window showing, the main one comes
/// back, restored if it was minimized (applicationShouldHandleReopen), never a
/// docket window left free. With one showing, Rune's default.
pub fn reopen(s: &mut Main, has_visible: bool, _: &mut Tx) -> bool {
    if has_visible {
        return false;
    }
    if let Some(me) = &s.me {
        me.open();
    }
    true
}

// ---- docket windows ----

/// Open a docket in a window of its own ("Open Docket", a double-click).
pub fn open_docket(id: &str) {
    let Ok(mut slots) = SLOTS.lock() else { return };
    let showing = |s: &Slot| s.shows.as_deref() == Some(id) && s.window.is_visible();
    let Some(i) = slots
        .iter()
        .position(showing)
        .or_else(|| slots.iter().position(|s| !s.window.is_visible()))
        .or_else(|| slots.iter().enumerate().min_by_key(|(_, s)| s.used).map(|(i, _)| i))
    else {
        return;
    };
    let slot = &mut slots[i];
    if !showing(slot) {
        let id = id.to_string();
        slot.window.update(move |s: &mut Main, tx| show_docket(s, &id, tx));
    }
    slot.shows = Some(id.to_string());
    slot.used = Some(Instant::now());
    slot.window.open();
}

fn show_docket(s: &mut Main, id: &str, tx: &mut Tx) {
    s.window = Window::Docket(Some(id.to_string()));
    s.chat = false;
    s.intent_edit = None;
    s.sheet = None;
    s.note.clear();
    if let Some(st) = s.state() {
        tx.set_title(title(&st, id));
    }
    s.load_page(id);
}

/// The window's title, as DocketWindow's navigationTitle: "D-6 · Its title".
fn title(st: &HubState, id: &str) -> String {
    st.docket(id).map(|d| format!("{id} · {}", d.title())).unwrap_or_else(|| id.to_string())
}

/// `docket`, then `docket-2`…: windowless until a docket opens in one.
fn docket_window(store: &Arc<Store>, i: usize) -> WindowSpec<Main> {
    let key = if i == 0 { "docket".to_string() } else { format!("docket-{}", i + 1) };
    let spec =
        AppSpec { scenes: SCENES, poll: Some(super::poll), on_key: Some(on_key), min_size: Some((620.0, 480.0)), ..AppSpec::new("Docket", DOCKET_SIZE, || unreachable!(), view) };
    // The traffic lights centred on the title band, where SwiftUI's sit.
    let chrome = Chrome { full_size_content: true, traffic_lights: Some((20.0, 19.0)), drag_regions: true };
    // Closed, it's free: it stops following its docket's page.
    let app = App::new(spec).chrome(chrome).windowless().keep_running().on_close(|s, _| s.window = Window::Docket(None));
    let me = app.handle();
    if let Ok(mut slots) = SLOTS.lock() {
        slots.push(Slot { window: me.clone(), shows: None, used: None });
    }
    // Its title follows the docket's.
    if !store.headless {
        let h = me.clone();
        store.subscribe(move || {
            h.update(|s: &mut Main, tx| {
                if let (Window::Docket(Some(id)), Some(st)) = (&s.window, s.state()) {
                    tx.set_title(title(&st, id));
                }
            })
        });
    }
    let store = store.clone();
    let app = app.init(move || {
        let mut m = Main::new(store, me);
        m.window = Window::Docket(None);
        m
    });
    WindowSpec::from_app(key, app)
}

/// ⌥⌘I shows the inspector; Escape closes a sheet.
fn on_key(s: &mut Main, ev: &KeyEvent, _: &mut Tx) -> bool {
    if ev.is_command('i') && ev.mods.alt {
        s.inspector.toggle();
        return true;
    }
    if matches!(ev.key, Key::Escape) && s.sheet.is_some() {
        s.sheet = None;
        return true;
    }
    false
}

/// The docket, as in the main window's detail, under a title band of its own
/// and beside its inspector: a plain window, as SwiftUI's DocketWindow.
fn view(s: &Main, cx: &mut Cx<Main>) -> El {
    let dark = s.theme.unwrap_or(cx.system_dark());
    let k = K::new(dark, s.dev());
    let kit = k.kit();
    let state = s.state();
    let id = s.current();
    let (main, inspector) = match (&state, &id) {
        (None, _) => (widgets::empty(&icons::REFRESH, "The hub isn't running", &s.store.error().unwrap_or_else(|| "Connecting…".into()), &k), None),
        (Some(st), Some(id)) => (docket::detail(s, st, id, &k, &kit), Some(docket::inspector(s, st, id, &k))),
        (Some(_), None) => (widgets::empty(&icons::STACK, "No docket", "It isn't in the hub's state.", &k), None),
    };
    let mut body = row().key("docket-window").w_full().grow(1.0).min_h(0.0).child(col().key("detail-column").grow(1.0).min_w(0.0).h_full().child(main));
    if let Some(i) = inspector.filter(|_| s.inspector.is_open()) {
        body = body.child(el().w(1.0).h_full().fill(k.line).a11y_hidden()).child(col().key("inspector").w(s.inspector.width()).h_full().min_h(0.0).px(10.0).child(i));
    }
    let band = titlebar(s, state.as_deref(), id.as_deref(), cx.chrome_insets(), &k, &kit);
    let mut root = col().key("root").w_full().h_full().fill(k.main).child(band).child(body);
    if let Some(sheet) = &s.sheet {
        root = root.child(docket::sheet(s, sheet, &k, &kit));
    }
    if !s.toasts.is_empty() {
        root = root.child(widgets::toasts(&s.toasts, &k));
    }
    root
}

/// The title band, 52 pt as a SwiftUI window's toolbar: "D-6 · Its title"
/// and Chat or Docket, then Open in Rex, Peek and the inspector (DocketWindow's
/// navigationTitle, DocketView's toolbar). It moves the window.
fn titlebar(s: &Main, st: Option<&HubState>, id: Option<&str>, insets: rune::ChromeInsets, k: &K, kit: &rune_kit::Kit) -> El {
    let d = st.zip(id).and_then(|(st, id)| st.docket(id));
    let mut band = WindowTitlebar::from_insets(insets).height(BAND).gap(14.0);
    if let Some(d) = d {
        band = band.leading(text(format!("{} · {}", d.id(), d.title())).font(font(13.0, 600.0).lh(16.0)).color(k.text2).truncate().max_w(420.0).role("heading").level(1));
        if d.talks() {
            band = band.leading(widgets::segmented("tab", &[("Chat", s.chat), ("Docket", !s.chat)], |s: &mut Main, i| s.chat = i == 0, k));
        }
    }
    if let Some(d) = d.filter(|d| d.live().is_some()) {
        let (id, peek) = (d.id(), d.id());
        band = band
            .trailing(widgets::icon_button("open-rex", &icons::TERMINAL, "Bring the agent's pane to the front in Rex (O)", move |s: &mut Main| s.open_in_rex(&id), k))
            .trailing(widgets::icon_button("peek", &icons::EYE, "Peek at the agent's pane here", move |s: &mut Main| super::chat::open_pane(s, &peek), k));
    }
    band.trailing(widgets::icon_button(
        "inspector-toggle",
        &icons::INSPECTOR,
        if s.inspector.is_open() { "Hide the inspector (⌥⌘I)" } else { "Show the inspector (⌥⌘I)" },
        |s: &mut Main| s.inspector.toggle(),
        k,
    ))
    .build(kit)
}

// ---- scenes, for headless shots: `--window docket --scene docket` ----

fn scene(s: &mut Main, id: &str) {
    s.window = Window::Docket(Some(id.to_string()));
    s.load_page(id);
}

type Scene = (&'static str, fn(&mut Main));

const SCENES: &[Scene] = &[
    ("docket", |s| scene(s, "D-6")),
    ("chat", |s| {
        scene(s, "D-6");
        s.chat = true;
    }),
    ("blocked", |s| scene(s, "D-1")),
    ("review", |s| scene(s, "D-7")),
    ("record", |s| {
        scene(s, "D-6");
        s.sheet = Some(super::Sheet::Record("D-6".into()));
    }),
];
