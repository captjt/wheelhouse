//! Settings (MenuBar.swift's `SettingsView`, SettingsTabs.swift,
//! SettingsModel.swift), as a window of its own beside the main one: the
//! app's own (General, Notifications, Hub), and what Wheelhouse does on its
//! own (Standing Orders, Agents, Repos, Guides & Briefs), kept by the hub
//! (docs/settings.md): `/api/settings`, its guides and briefs, and Review's and
//! Build's own switches. ⌘, or the sidebar's Settings opens it; closing it
//! hides it, and it keeps its tab and drafts.
//!
//! ```sh
//! target/release/wheelhouse-rune --window settings --scene settings-orders --size 900x640 --snap orders.png
//! ```

mod agents;
mod general;
mod glyphs;
mod guides;
mod orders;
pub mod patch;
mod repos;
mod ui;

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use rune::{App, AppHandle, AppSpec, Chrome, Cx, El, Icon, Key, KeyEvent, TextEdit, Tx, WindowOptions, WindowSpec, col, icon, row, text};
use serde_json::{Value, json};

use super::{Main, Toast, widgets};
use crate::model::{self, flag, text as jtext};
use crate::store::Store;
use crate::theme::{K, font};

/// The window's tabs, in the SwiftUI TabView's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    General,
    Orders,
    Agents,
    Repos,
    Guides,
    Notifications,
    Hub,
}

/// The title bar and the tabs under it.
const HEADER: f32 = 88.0;

/// The quick panel's chord, as the menus write it (`panel::CHORD`).
pub const CHORD: &str = "⌃⌥⌘K";

impl Tab {
    pub const ALL: [Tab; 7] = [Tab::General, Tab::Orders, Tab::Agents, Tab::Repos, Tab::Guides, Tab::Notifications, Tab::Hub];

    pub fn title(self) -> &'static str {
        match self {
            Tab::General => "General",
            Tab::Orders => "Standing Orders",
            Tab::Agents => "Agents",
            Tab::Repos => "Repos",
            Tab::Guides => "Guides & Briefs",
            Tab::Notifications => "Notifications",
            Tab::Hub => "Hub",
        }
    }

    /// The SwiftUI tab's tag (`settingsTab`).
    pub fn id(self) -> &'static str {
        match self {
            Tab::General => "general",
            Tab::Orders => "orders",
            Tab::Agents => "agents",
            Tab::Repos => "repos",
            Tab::Guides => "guides",
            Tab::Notifications => "notifications",
            Tab::Hub => "hub",
        }
    }

    fn icon(self) -> &'static Icon {
        match self {
            Tab::General => &crate::icons::GEAR,
            Tab::Orders => &glyphs::CLIPBOARD,
            Tab::Agents => &glyphs::PEOPLE,
            Tab::Repos => &glyphs::FOLDER,
            Tab::Guides => &glyphs::BOOK,
            Tab::Notifications => &glyphs::BELL,
            Tab::Hub => &glyphs::BOLT,
        }
    }

    /// The window's size on this tab: each SwiftUI tab sizes the window for
    /// itself (its frame), under the title bar and tabs.
    pub fn size(self) -> (f32, f32) {
        let (w, h) = match self {
            Tab::General => (560.0, 300.0),
            Tab::Orders => (640.0, 680.0),
            Tab::Agents => (600.0, 680.0),
            Tab::Repos => (860.0, 680.0),
            Tab::Guides => (1040.0, 700.0),
            Tab::Notifications => (560.0, 330.0),
            Tab::Hub => (560.0, 330.0),
        };
        (w, h + HEADER)
    }
}

/// One guide or brief, as Settings lists it (`SettingsDoc`).
#[derive(Clone, Debug, PartialEq)]
pub struct Doc {
    pub j: Value,
}

impl Doc {
    pub fn id(&self) -> String {
        model::str_or(&self.j["id"], "").to_string()
    }
    /// guide or brief.
    pub fn kind(&self) -> String {
        model::str_or(&self.j["kind"], "guide").to_string()
    }
    /// review, build, or "" for a guide.
    pub fn proto(&self) -> String {
        model::str_or(&self.j["protocol"], "").to_string()
    }
    pub fn title(&self) -> String {
        jtext(&self.j["title"]).unwrap_or_else(|| self.id())
    }
    pub fn about(&self) -> Option<String> {
        jtext(&self.j["about"])
    }
    pub fn builtin(&self) -> bool {
        flag(&self.j["builtin"])
    }
    pub fn overridden(&self) -> bool {
        flag(&self.j["overridden"])
    }
    pub fn path(&self) -> String {
        model::str_or(&self.j["path"], "").to_string()
    }
    pub fn used_by(&self) -> Vec<String> {
        model::strings(&self.j["used_by"])
    }
    pub fn yours(&self) -> bool {
        !self.builtin()
    }
    /// Built in, edited, or yours.
    pub fn origin(&self) -> &'static str {
        if !self.builtin() {
            "Yours"
        } else if self.overridden() {
            "Edited"
        } else {
            "Built in"
        }
    }
}

/// What's open over the window.
#[allow(clippy::large_enum_variant)]
pub enum Sheet {
    /// Add a check to a repo, or edit one (`CheckEditor`).
    Check { slug: String, existing: Option<String>, name: TextEdit, run: TextEdit, required: bool, minutes: i64 },
    /// A posting rule's conditions, for every repo or one (`RuleEditor`).
    Rule { slug: Option<String>, approve: bool, risk: String, max_lines: TextEdit, authors: TextEdit, labels: TextEdit, not_touching: TextEdit, problem: Option<String> },
    /// A guide of your own (`NewGuideSheet`).
    NewGuide { title: TextEdit, error: Option<String> },
    /// Reset a guide or brief to the built-in, or set yours aside.
    Reset(String),
}

/// A repo's form: the fields you type in, kept while you're on it.
pub struct RepoForm {
    pub slug: String,
    pub notes: TextEdit,
    pub base: TextEdit,
    /// The notes and base as the hub last said, to tell edits from news.
    pub saved_notes: String,
    pub saved_base: String,
    /// The base field had the keys last frame: letting go of them saves it.
    pub base_focus: bool,
}

/// A guide's or brief's text, as the hub gave it.
#[derive(Clone)]
pub struct DocText {
    pub saved: String,
    pub original: Option<String>,
}

pub struct Settings {
    pub store: Arc<Store>,
    me: Option<AppHandle<Settings>>,
    main: Option<AppHandle<Main>>,
    pub version: u64,
    pub tab: Tab,
    /// `GET /api/settings`.
    pub view: Option<Arc<Value>>,
    pub load_error: Option<String>,
    /// A change's error, by what caused it: shown beside that control.
    pub errors: HashMap<String, String>,
    /// What a change did that's worth a word, by key.
    pub notes: HashMap<String, String>,
    pub busy: HashSet<String>,
    /// Light or dark; None follows the system. The main window's.
    pub theme: Option<bool>,
    pub toasts: Vec<Toast>,
    pub clock: f64,
    pub sheet: Option<Sheet>,
    /// An element to bring to the top of its scroll, next frame.
    pub reveal: Option<(String, f32)>,
    // Repos
    pub repo: Option<String>,
    pub repo_form: Option<RepoForm>,
    pub check: Option<String>,
    // Guides & Briefs
    pub guide: Option<String>,
    pub texts: HashMap<String, DocText>,
    pub doc_errors: HashMap<String, String>,
    /// Each doc's editor: its text is the draft, so switching keeps edits.
    pub editors: HashMap<String, TextEdit>,
    pub compare: bool,
    /// What a field's late edits go to when what it edits has gone (a sheet
    /// closed under it).
    pub scratch: TextEdit,
}

thread_local! {
    /// The Settings window, for the main window to open (one per app, on the UI thread).
    static WINDOW: RefCell<Option<AppHandle<Settings>>> = const { RefCell::new(None) };
}

/// The Settings window, as the main window's App made it.
pub fn link(h: AppHandle<Settings>) {
    WINDOW.with(|w| *w.borrow_mut() = Some(h));
}

/// Show Settings in the main window's theme: ⌘, or the sidebar's Settings.
pub fn open(theme: Option<bool>) {
    WINDOW.with(|w| {
        if let Some(h) = w.borrow().as_ref() {
            h.update(move |s: &mut Settings, tx| {
                s.theme = theme;
                s.opened(tx);
            });
            h.open();
        }
    });
}

/// The main window's theme changed: Settings follows it.
pub fn follow_theme(theme: Option<bool>) {
    WINDOW.with(|w| {
        if let Some(h) = w.borrow().as_ref() {
            h.update(move |s: &mut Settings, _| s.theme = theme);
        }
    });
}

static NULL: Value = Value::Null;

impl Settings {
    fn new(store: Arc<Store>, me: AppHandle<Settings>, main: AppHandle<Main>) -> Settings {
        let mut s = Settings {
            store: store.clone(),
            me: Some(me.clone()),
            main: Some(main),
            version: 0,
            tab: Tab::General,
            view: None,
            load_error: None,
            errors: HashMap::new(),
            notes: HashMap::new(),
            busy: HashSet::new(),
            theme: match std::env::var("WH_RUNE_THEME").as_deref() {
                Ok("light") => Some(false),
                Ok("dark") => Some(true),
                _ => None,
            },
            toasts: Vec::new(),
            clock: 0.0,
            sheet: None,
            reveal: None,
            repo: None,
            repo_form: None,
            check: None,
            guide: None,
            texts: HashMap::new(),
            doc_errors: HashMap::new(),
            editors: HashMap::new(),
            compare: false,
            scratch: TextEdit::new(""),
        };
        // Told whenever the hub's state changes (Review's and Build's pauses
        // come from it): build again.
        if !store.headless {
            store.subscribe(move || me.update(|s: &mut Settings, _| s.version += 1));
        }
        s.load();
        s
    }

    /// The window was asked for: read the settings again, as each SwiftUI tab does on show.
    fn opened(&mut self, tx: &mut Tx) {
        tx.set_title(self.tab.title());
        self.load();
        self.shown();
    }

    /// Go to a tab: the window takes its size, and its settings are read again.
    pub fn go(&mut self, tab: Tab, tx: &mut Tx) {
        if self.tab != tab {
            self.tab = tab;
            tx.set_title(tab.title());
            tx.resize_window(tab.size());
        }
        self.load();
        self.shown();
    }

    /// What the tab shows needs its data: the repo's fields, the guide's text.
    fn shown(&mut self) {
        repos::ensure_form(self);
        if self.tab == Tab::Guides
            && let Some(id) = self.current_doc().map(|d| d.id())
            && !self.texts.contains_key(&id)
        {
            self.load_doc(&id);
        }
    }

    pub fn dev(&self) -> bool {
        self.store.state().is_some_and(|s| s.flavor().is_some())
    }

    // ---- the hub's view of the settings ----

    pub fn v(&self) -> &Value {
        self.view.as_deref().unwrap_or(&NULL)
    }
    pub fn set(&self) -> &Value {
        &self.v()["set"]
    }
    pub fn build(&self) -> &Value {
        &self.v()["build"]["config"]
    }
    pub fn review(&self) -> &Value {
        &self.v()["review"]["config"]
    }
    pub fn orders(&self) -> &[Value] {
        model::arr(&self.v()["orders"])
    }
    pub fn docs(&self) -> Vec<Doc> {
        model::arr(&self.v()["docs"]).iter().map(|j| Doc { j: j.clone() }).collect()
    }
    pub fn doc(&self, id: &str) -> Option<Doc> {
        self.docs().into_iter().find(|d| d.id() == id)
    }
    /// The guide or brief the Guides tab shows: the one picked, or the first.
    pub fn current_doc(&self) -> Option<Doc> {
        let docs = self.docs();
        self.guide.as_deref().and_then(|g| docs.iter().find(|d| d.id() == g).cloned()).or_else(|| docs.into_iter().next())
    }
    /// settings.json, by name.
    pub fn file(&self) -> String {
        model::str_or(&self.v()["file"], "settings.json").to_string()
    }
    /// Where whatever a save replaces is kept.
    pub fn history(&self) -> String {
        model::str_or(&self.v()["history"], "the history").to_string()
    }
    /// build.lua or review.lua, by name.
    pub fn lua(&self, part: &str) -> String {
        jtext(&self.v()[part]["lua"]).and_then(|p| p.rsplit('/').next().map(str::to_string)).unwrap_or_else(|| format!("{part}.lua"))
    }
    pub fn lua_exists(&self, part: &str) -> bool {
        flag(&self.v()[part]["lua_exists"])
    }
    /// Going back: to the Lua's value, or the default without one.
    pub fn revert_label(&self, part: &str) -> String {
        if self.lua_exists(part) { format!("Use {}'s", self.lua(part)) } else { "Use the default".into() }
    }
    pub fn is_set(&self, part: &str, path: &[&str]) -> bool {
        patch::is_set(self.set(), part, path)
    }
    pub fn is_busy(&self, key: &str) -> bool {
        self.busy.contains(key)
    }

    // ---- calls to the hub: inline in a headless run, else off the UI thread ----

    fn call(&mut self, busy: &str, work: impl FnOnce(&Store) -> Result<Value, String> + Send + 'static, done: impl FnOnce(&mut Settings, Result<Value, String>) + Send + 'static) {
        let key = busy.to_string();
        if !key.is_empty() && !self.busy.insert(key.clone()) {
            return;
        }
        if self.store.headless {
            let r = work(&self.store);
            self.busy.remove(&key);
            done(self, r);
            return;
        }
        let (store, me) = (self.store.clone(), self.me.clone());
        std::thread::spawn(move || {
            let r = work(&store);
            if let Some(me) = me {
                me.update(move |s: &mut Settings, _| {
                    s.busy.remove(&key);
                    done(s, r);
                });
            }
        });
    }

    pub fn load(&mut self) {
        self.call(
            "",
            |st| st.hub.get("/api/settings"),
            |s, r| match r {
                Ok(v) => s.landed(v),
                Err(e) => s.load_error = Some(e),
            },
        );
    }

    /// The settings arrived: a repo's fields follow them unless you've typed in them.
    fn landed(&mut self, v: Value) {
        self.view = Some(Arc::new(v));
        self.load_error = None;
        repos::ensure_form(self);
        let Some(f) = &self.repo_form else { return };
        let slug = f.slug.clone();
        let (notes, base) = (repos::saved_notes(self, &slug), repos::saved_base(self, &slug));
        if let Some(f) = &mut self.repo_form {
            if f.notes.text == f.saved_notes && notes != f.saved_notes {
                f.notes.replace_text(notes.clone());
            }
            if !f.base.focused && f.base.text == f.saved_base && base != f.saved_base {
                f.base.replace_text(base.clone());
            }
            f.saved_notes = notes;
            f.saved_base = base;
        }
    }

    /// Change settings; on refusal, keep the hub's sentence by `key`. `then`
    /// runs once it's taken.
    pub fn change(&mut self, key: &str, patch: Value, then: impl FnOnce(&mut Settings) + Send + 'static) {
        let k = key.to_string();
        self.call(
            key,
            move |st| st.post("/api/settings", patch),
            move |s, r| match r {
                Ok(v) => {
                    if v.get("orders").is_some() {
                        s.landed(v);
                    } else {
                        s.load();
                    }
                    s.errors.remove(&k);
                    then(s);
                }
                Err(e) => {
                    s.errors.insert(k, e);
                }
            },
        );
    }

    /// Review's and Build's own switches (posting, pausing), as before.
    pub fn toggle(&mut self, key: &str, path: &str, body: Value) {
        let (k, path) = (key.to_string(), path.to_string());
        self.call(
            key,
            move |st| st.post(&path, body),
            move |s, r| {
                match r {
                    Ok(_) => s.errors.remove(&k),
                    Err(e) => s.errors.insert(k, e),
                };
                s.store.reload();
                s.load();
            },
        );
    }

    pub fn toast(&mut self, text: impl Into<String>, error: bool) {
        self.toasts.push(Toast { text: text.into(), error, at: self.clock });
        if self.toasts.len() > 3 {
            self.toasts.remove(0);
        }
    }

    // ---- guides and briefs ----

    pub fn load_doc(&mut self, id: &str) {
        let (path, id) = (format!("/api/settings/doc?id={}", crate::hub::encode(id)), id.to_string());
        self.call(
            "",
            move |st| st.hub.get(&path),
            move |s, r| match r {
                Ok(v) => {
                    s.doc_errors.remove(&id);
                    s.doc_landed(&id, &v);
                }
                Err(e) => {
                    s.doc_errors.insert(id, e);
                }
            },
        );
    }

    /// A doc's text arrived: its editor takes it, unless you've changed it.
    fn doc_landed(&mut self, id: &str, v: &Value) {
        let next = DocText { saved: model::str_or(&v["text"], "").to_string(), original: v["original"].as_str().map(str::to_string) };
        let before = self.texts.get(id).map(|t| t.saved.clone());
        match (self.editors.get_mut(id), before) {
            // Your edits stay.
            (Some(e), Some(b)) if e.text != b => {}
            (Some(e), _) => e.replace_text(next.saved.clone()),
            (None, _) => {
                self.editors.insert(id.to_string(), TextEdit::new(next.saved.clone()));
            }
        }
        self.texts.insert(id.to_string(), next);
    }

    /// The editor of a doc whose text is in.
    pub fn editor_mut(&mut self, id: &str) -> &mut TextEdit {
        self.editors.entry(id.to_string()).or_insert_with(|| TextEdit::new(""))
    }

    /// Save what's in a doc's editor.
    pub fn save_doc(&mut self, id: &str) {
        let Some(saved) = self.texts.get(id).map(|t| t.saved.clone()) else { return };
        let Some(text) = self.editors.get(id).map(|e| e.text.clone()) else { return };
        if text == saved {
            return;
        }
        let (id, title) = (id.to_string(), self.doc(id).map(|d| d.title()).unwrap_or_default());
        let body = json!({ "id": id, "text": text });
        self.call(
            &format!("doc:{id}"),
            move |st| st.post("/api/settings/doc", body),
            move |s, r| match r {
                Ok(v) => {
                    s.doc_errors.remove(&id);
                    s.texts.insert(id.clone(), DocText { saved: model::str_or(&v["text"], &text).to_string(), original: v["original"].as_str().map(str::to_string) });
                    s.load();
                    s.toast(format!("Saved {title}: agents get it from their next brief"), false);
                }
                Err(e) => {
                    s.doc_errors.insert(id, e);
                }
            },
        );
    }

    /// Reset a doc to the built-in, or set a guide of yours aside.
    pub fn reset_doc(&mut self, id: &str) {
        self.sheet = None;
        let Some(doc) = self.doc(id) else { return };
        let (id, body) = (id.to_string(), json!({ "id": id }));
        self.call(
            &format!("doc:{id}"),
            move |st| st.post("/api/settings/doc/reset", body),
            move |s, r| match r {
                Ok(v) => {
                    // What you had is kept in the history: the editor starts again
                    // from the built-in, or goes with a guide set aside.
                    s.doc_errors.remove(&id);
                    s.texts.remove(&id);
                    s.editors.remove(&id);
                    if !flag(&v["removed"]) {
                        s.doc_landed(&id, &v);
                    }
                    s.load();
                    s.toast(if doc.yours() { format!("Set aside {}", doc.title()) } else { format!("{} is the built-in again", doc.title()) }, false);
                }
                Err(e) => {
                    s.doc_errors.insert(id, e);
                }
            },
        );
    }

    /// ⌘S: save what the tab is editing.
    fn save_current(&mut self) {
        match self.tab {
            Tab::Guides => {
                if let Some(id) = self.current_doc().map(|d| d.id()) {
                    self.save_doc(&id);
                }
            }
            Tab::Repos => repos::save_notes(self),
            _ => {}
        }
    }

    /// The theme, here and in the main window (General's Appearance).
    pub fn set_theme(&mut self, theme: Option<bool>) {
        self.theme = theme;
        if let Some(m) = &self.main {
            m.update(move |m: &mut Main, _| m.theme = theme);
        }
    }

    /// `GET /api/verify`, in a toast (Hub's Verify Every Record).
    pub fn verify_all(&mut self) {
        self.call(
            "verify",
            |st| st.hub.get("/api/verify"),
            |s, r| match r {
                Ok(v) => {
                    let dockets = model::arr(&v["dockets"]);
                    if flag(&v["ok"]) {
                        s.toast(format!("All {} records check: chains intact, signatures good", dockets.len()), false);
                    } else {
                        s.toast(format!("{} records don't check", dockets.iter().filter(|d| !flag(&d["ok"])).count()), true);
                    }
                }
                Err(e) => s.toast(e, true),
            },
        );
    }
}

// ---- the window ----

/// The Settings window, hidden until ⌘, opens it.
pub fn spec(store: Arc<Store>, main: AppHandle<Main>) -> WindowSpec<Settings> {
    let spec = AppSpec {
        scenes: SCENES,
        poll: Some(poll),
        on_key: Some(on_key),
        min_size: Some((520.0, 320.0)),
        // Mono spans (inline `code`) in SF Mono, as the app's mono text.
        families: ("Inter", "ui-monospace, Inter"),
        ..AppSpec::new("Settings", Tab::General.size(), || unreachable!(), view)
    };
    // A titled window drawn edge to edge: its title and tabs are the app's,
    // the traffic lights centred on the title.
    let chrome = Chrome { full_size_content: true, traffic_lights: Some((12.0, 7.0)), drag_regions: true };
    let app = App::new(spec).chrome(chrome);
    let me = app.handle();
    let app = app.init(move || Settings::new(store, me, main));
    WindowSpec::from_app("settings", app).options(WindowOptions { visible: false, min_size: Some((520.0, 320.0)), ..Default::default() })
}

/// Before each frame: toasts age out, and a scroll asked for lands.
pub fn poll(s: &mut Settings, t: f64, tx: &mut Tx) -> bool {
    s.clock = t;
    repos::follow_focus(s);
    if let Some((key, at)) = s.reveal.take() {
        tx.scroll_key_to(key, at);
    }
    let before = s.toasts.len();
    s.toasts.retain(|x| t - x.at < 4.0);
    if !s.toasts.is_empty() {
        tx.wake_after(std::time::Duration::from_millis(500));
    }
    before != s.toasts.len()
}

/// ⌘W closes the window, ⌘S saves what you're editing, Escape closes a sheet.
pub fn on_key(s: &mut Settings, ev: &KeyEvent, tx: &mut Tx) -> bool {
    if ev.is_command('w') && !ev.mods.shift && !ev.mods.alt {
        tx.close_window();
        return true;
    }
    if ev.is_command('s') && !ev.mods.shift && !ev.mods.alt {
        s.save_current();
        return true;
    }
    if matches!(ev.key, Key::Escape) && s.sheet.is_some() {
        s.sheet = None;
        return true;
    }
    false
}

pub fn view(s: &Settings, cx: &mut Cx<Settings>) -> El {
    let dark = s.theme.unwrap_or(cx.system_dark());
    let k = K::new(dark, s.dev());
    let kit = k.kit();
    let body = match s.tab {
        Tab::General => general::general(s, &k, &kit),
        Tab::Orders => orders::view(s, &k, &kit),
        Tab::Agents => agents::view(s, &k, &kit),
        Tab::Repos => repos::view(s, &k, &kit),
        Tab::Guides => guides::view(s, &k, &kit),
        Tab::Notifications => general::notifications(s, &k, &kit),
        Tab::Hub => general::hub(s, &k, &kit),
    };
    let mut root = col()
        .key("settings-root")
        .w_full()
        .h_full()
        .fill(ui::chrome(&k))
        .child(header(s, &k))
        .child(rune::el().h(1.0).w_full().fill(k.line2).a11y_hidden())
        .child(col().key(format!("tab-{}", s.tab.id())).w_full().grow(1.0).min_h(0.0).child(body));
    if let Some(sheet) = &s.sheet {
        root = root.child(ui::scrim(sheets(s, sheet, &k), &k));
    }
    if !s.toasts.is_empty() {
        root = root.child(widgets::toasts(&s.toasts, &k));
    }
    root
}

/// The title, centred over the tabs, as a Settings window's toolbar.
fn header(s: &Settings, k: &K) -> El {
    let title = row().w_full().h(30.0).center().child(text(s.tab.title()).font(font(13.0, 600.0).lh(16.0)).color(k.text).role("heading").level(1));
    let mut tabs = row().key("tabs").gap(2.0).role("tablist").label("Settings").no_window_drag();
    for t in Tab::ALL {
        let on = s.tab == t;
        let ink = if on { k.text } else { k.muted };
        tabs = tabs.child(
            col()
                .key(format!("tab-button-{}", t.id()))
                .min_w(56.0)
                .px(7.0)
                .pt(5.0)
                .pb(4.0)
                .gap(3.0)
                .items_center()
                .radius(7.0)
                .continuous_corners()
                .fill(if on { if k.dark { rune::Color::WHITE.alpha(0.09) } else { rune::Color::BLACK.alpha(0.06) } } else { rune::Color::TRANSPARENT })
                .hover(if on { rune::Color::TRANSPARENT } else { k.hover })
                .role("tab")
                .label(t.title())
                .checked(on)
                .focusable(true)
                .on_press_tx(move |s: &mut Settings, tx| s.go(t, tx))
                .child(icon(t.icon(), 21.0).color(ink))
                .child(text(t.title()).font(font(11.0, if on { 500.0 } else { 400.0 }).lh(13.0)).color(ink)),
        );
    }
    col().key("settings-titlebar").w_full().h(HEADER - 1.0).window_drag().child(title).child(row().w_full().justify_center().child(tabs))
}

fn sheets(s: &Settings, sheet: &Sheet, k: &K) -> El {
    match sheet {
        Sheet::Check { .. } => repos::check_editor(s, sheet, k),
        Sheet::Rule { .. } => orders::rule_editor(s, sheet, k),
        Sheet::NewGuide { .. } => guides::new_guide(s, sheet, k),
        Sheet::Reset(id) => guides::confirm_reset(s, id, k),
    }
}

// ---- scenes, for headless shots: one per tab, as Snapshot.swift names them ----

fn tab(s: &mut Settings, t: Tab) {
    s.tab = t;
    s.shown();
}

/// A named state for a headless shot.
type Scene = (&'static str, fn(&mut Settings));

pub const SCENES: &[Scene] = &[
    ("settings-general", |s| tab(s, Tab::General)),
    ("settings-orders", |s| tab(s, Tab::Orders)),
    ("settings-agents", |s| tab(s, Tab::Agents)),
    ("settings-repos", |s| tab(s, Tab::Repos)),
    ("settings-guides", |s| {
        s.guide = Some("guide/plan-format.md".into());
        tab(s, Tab::Guides);
    }),
    ("settings-notifications", |s| tab(s, Tab::Notifications)),
    ("settings-hub", |s| tab(s, Tab::Hub)),
    // A check's editor, over its repo.
    ("settings-check", |s| {
        tab(s, Tab::Repos);
        repos::edit_check(s, "lint");
    }),
    // Who posts a checked review: a repo's own.
    ("settings-posting", |s| {
        tab(s, Tab::Repos);
        s.reveal = Some(("posting".into(), 0.0));
    }),
    // The posting rule's editor, for every repo.
    ("settings-rule", |s| {
        tab(s, Tab::Orders);
        orders::edit_rule(s, None);
    }),
    // A brief, edited to leave out a variable, compared with the built-in.
    ("settings-brief", |s| {
        s.guide = Some("build/brief/plan.md".into());
        tab(s, Tab::Guides);
        guides::demo_edit(s, "build/brief/plan.md", "{{PLAN_GUIDE}}");
    }),
    // Refused (nothing is written): the hub's sentence, under the builder.
    ("settings-error", |s| {
        tab(s, Tab::Agents);
        s.change("build.builder", patch::make("build", &["builder"], Some(json!({ "agent": "gpt" }))), |_| {});
    }),
];
