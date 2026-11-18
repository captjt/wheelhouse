//! The main window (RootView.swift): the sidebar and its places, a list beside
//! the docket it shows, and the docket's inspector, on rune-kit's shell. Every
//! window reads the one Store and is told when it changes.

mod activity;
mod docket;
mod forms;
mod lists;
mod menus;
mod palette;
mod sidebar;
pub mod widgets;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use rune::{AppHandle, Cx, El, Key, KeyEvent, TextEdit, Tx, col, el, row, text};
use rune_kit::shell::{PaneState, SplitView, Toolbar};
use serde_json::{Value, json};

use crate::model::{self, DocketPage, HubState, Status};
use crate::store::Store;
use crate::theme::{self, K, font};

/// The sidebar's places.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Place {
    Inbox,
    Reviews,
    Builds,
    Active,
    All,
    Done,
    Agents,
    Recap,
    Session(String),
}

impl Place {
    pub const ORDER: [Place; 8] = [Place::Inbox, Place::Reviews, Place::Builds, Place::Active, Place::All, Place::Done, Place::Agents, Place::Recap];
    pub fn title(&self) -> String {
        match self {
            Place::Inbox => "Inbox".into(),
            Place::Reviews => "Reviews".into(),
            Place::Builds => "Builds".into(),
            Place::Active => "Active".into(),
            Place::All => "All dockets".into(),
            Place::Done => "Done".into(),
            Place::Agents => "Agents".into(),
            Place::Recap => "Recap".into(),
            Place::Session(s) => s.clone(),
        }
    }
    /// Which dockets a list shows.
    pub fn shows(&self, d: &model::Docket) -> bool {
        match self {
            Place::Active => d.is_open(),
            Place::All => true,
            Place::Done => !d.is_open(),
            Place::Session(s) => d.is_open() && d.rex_session().as_deref() == Some(s),
            _ => false,
        }
    }
    pub fn is_list(&self) -> bool {
        matches!(self, Place::Active | Place::All | Place::Done | Place::Session(_))
    }
}

/// How a list groups its dockets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupBy {
    Attention,
    Status,
    Session,
}

#[derive(Clone)]
pub struct Toast {
    pub text: String,
    pub error: bool,
    pub at: f64,
}

/// What a new docket starts as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NewMode {
    Blank,
    Linear,
    Agent,
}

/// What's open over the window.
#[derive(Clone, Debug, PartialEq)]
pub enum Sheet {
    Record(String),
    Hold(String),
    NewDocket(NewMode),
    StartAgent(String),
    LinkIssue(String),
}

pub struct Main {
    pub store: Arc<Store>,
    pub version: u64,
    pub me: Option<AppHandle<Main>>,
    pub place: Place,
    pub selection: HashMap<Place, String>,
    pub sidebar: PaneState,
    pub inspector: PaneState,
    pub pages: HashMap<String, Arc<DocketPage>>,
    pub page_errors: HashMap<String, String>,
    pub busy: HashSet<String>,
    pub folded: HashSet<String>,
    pub board: bool,
    pub group_by: GroupBy,
    pub chat: bool,
    pub note: TextEdit,
    pub intent_edit: Option<TextEdit>,
    pub hold_why: TextEdit,
    pub sheet: Option<Sheet>,
    pub form: forms::Form,
    pub palette: Option<palette::Palette>,
    pub toasts: Vec<Toast>,
    pub theme: Option<bool>,
    pub clock: f64,
    pub pending_g: bool,
    /// A section to scroll to once it's laid out (a scene's `verify`).
    pub scroll_to: Option<String>,
}

impl Main {
    pub fn new(store: Arc<Store>, me: AppHandle<Main>) -> Main {
        let mut m = Main {
            store: store.clone(),
            version: 0,
            me: Some(me.clone()),
            place: Place::Inbox,
            selection: HashMap::new(),
            sidebar: PaneState::new(228.0).range(200.0, 320.0),
            inspector: PaneState::new(280.0).range(240.0, 360.0),
            pages: HashMap::new(),
            page_errors: HashMap::new(),
            busy: HashSet::new(),
            folded: ["reviews:done".to_string(), "builds:done".to_string()].into_iter().collect(),
            board: false,
            group_by: GroupBy::Attention,
            chat: false,
            note: TextEdit::new(""),
            intent_edit: None,
            hold_why: TextEdit::new(""),
            sheet: None,
            form: forms::Form::default(),
            palette: None,
            toasts: Vec::new(),
            theme: match std::env::var("WH_RUNE_THEME").as_deref() {
                Ok("light") => Some(false),
                Ok("dark") => Some(true),
                _ => None,
            },
            clock: 0.0,
            pending_g: false,
            scroll_to: None,
        };
        // Told whenever the hub's state changes: build again, and refresh the
        // docket on show.
        if !store.headless {
            let h = me.clone();
            store.subscribe(move || {
                h.update(|s: &mut Main, _| {
                    s.version += 1;
                    if let Some(id) = s.current() {
                        s.load_page(&id);
                    }
                })
            });
        }
        if let Some(id) = m.current() {
            m.load_page(&id);
        }
        m
    }

    pub fn state(&self) -> Option<Arc<HubState>> {
        self.store.state()
    }

    pub fn dev(&self) -> bool {
        self.state().is_some_and(|s| s.flavor().is_some())
    }

    /// The docket the window shows now: the selected one, or the first.
    pub fn current(&self) -> Option<String> {
        let state = self.state()?;
        let sel = self.selection.get(&self.place).cloned();
        match &self.place {
            Place::Inbox => {
                let items = model::inbox(&state);
                let concerns = model::concerns(&state, &items);
                if let Some(c) = concerns.iter().find(|c| Some(c.id()) == sel) {
                    return c.docket.clone();
                }
                let it = items.iter().find(|i| Some(&i.id) == sel.as_ref()).or(items.first());
                it.map(|i| i.docket_id.clone()).or_else(|| concerns.first().and_then(|c| c.docket.clone())).filter(|id| state.docket(id).is_some())
            }
            Place::Reviews => sel.or_else(|| lists::review_groups(&state).into_iter().flat_map(|g| g.3).find_map(|p| p.docket())),
            Place::Builds => sel.or_else(|| lists::build_groups(&state).into_iter().flat_map(|g| g.3).map(|t| t.docket()).next()),
            p if p.is_list() => sel.filter(|id| state.docket(id).is_some()).or_else(|| {
                let shown: Vec<_> = state.dockets.iter().filter(|d| p.shows(d)).cloned().collect();
                lists::group_dockets(&shown, self.group_by).into_iter().flat_map(|g| g.items).map(|d| d.id()).next()
            }),
            _ => None,
        }
        .filter(|id| state.docket(id).is_some())
    }

    pub fn go(&mut self, p: Place) {
        self.place = p;
        self.chat = false;
        if let Some(id) = self.current() {
            self.load_page(&id);
        }
    }

    pub fn select(&mut self, id: String) {
        self.selection.insert(self.place.clone(), id);
        self.intent_edit = None;
        if let Some(d) = self.current() {
            self.load_page(&d);
        }
    }

    /// Show a docket where it is: the list you're on, the Inbox if it needs
    /// you, else All dockets (Nav.show).
    pub fn show(&mut self, id: &str) {
        let Some(state) = self.state() else { return };
        if self.place == Place::Inbox {
            if let Some(it) = model::inbox(&state).into_iter().find(|i| i.docket_id == id) {
                return self.select(it.id);
            }
        }
        if let Some(d) = state.docket(id) {
            if self.place.shows(d) {
                return self.select(id.to_string());
            }
        }
        if let Some(it) = model::inbox(&state).into_iter().find(|i| i.docket_id == id) {
            self.place = Place::Inbox;
            return self.select(it.id);
        }
        self.place = Place::All;
        self.select(id.to_string());
    }

    /// A docket in a window of its own; until that window exists, here.
    pub fn open_window(&mut self, id: &str) {
        self.show(id);
    }

    /// Fetch a docket's page: inline in a headless run, else off the UI thread.
    pub fn load_page(&mut self, id: &str) {
        if self.store.headless {
            match self.store.page(id) {
                Ok(p) => {
                    self.pages.insert(id.to_string(), Arc::new(p));
                }
                Err(e) => {
                    self.page_errors.insert(id.to_string(), e);
                }
            }
            return;
        }
        let (store, me, id) = (self.store.clone(), self.me.clone(), id.to_string());
        std::thread::spawn(move || {
            let r = store.page(&id);
            if let Some(me) = me {
                me.update(move |s: &mut Main, _| match r {
                    Ok(p) => {
                        s.page_errors.remove(&id);
                        let p = Arc::new(p);
                        if s.pages.get(&id).map(|o| o.j != p.j).unwrap_or(true) {
                            s.pages.insert(id, p);
                        }
                    }
                    Err(e) => {
                        s.page_errors.insert(id, e);
                    }
                });
            }
        });
    }

    pub fn toast(&mut self, text: impl Into<String>, error: bool) {
        self.toasts.push(Toast { text: text.into(), error, at: self.clock });
        if self.toasts.len() > 3 {
            self.toasts.remove(0);
        }
    }

    /// A write to the hub; `done` words its answer as a toast.
    pub fn act(&mut self, busy: &str, path: String, body: Value, done: impl FnOnce(Result<Value, String>) -> (String, bool) + Send + 'static) {
        if !self.busy.insert(busy.to_string()) {
            return;
        }
        let key = busy.to_string();
        if self.store.headless {
            let r = self.store.post(&path, body);
            let (t, e) = done(r);
            self.busy.remove(&key);
            self.toast(t, e);
            if let Some(id) = self.current() {
                self.load_page(&id);
            }
            return;
        }
        let (store, me) = (self.store.clone(), self.me.clone());
        std::thread::spawn(move || {
            let (t, e) = done(store.post(&path, body));
            if let Some(me) = me {
                me.update(move |s: &mut Main, _| {
                    s.busy.remove(&key);
                    s.toast(t, e);
                    if let Some(id) = s.current() {
                        s.load_page(&id);
                    }
                });
            }
        });
    }

    // ---- what you do to a docket (Store.swift) ----

    pub fn answer(&mut self, d: &model::Docket, approve: bool) {
        let Some(l) = d.live() else { return };
        let (id, msg) = (d.id(), l.msg());
        let body = json!({ "record": l.record().unwrap_or_default(), "updated_at": l.since(), "msg": msg });
        let verb = if approve { "approve" } else { "deny" };
        self.act(&format!("{verb}:{id}"), format!("/api/dockets/{id}/{verb}"), body, move |r| match r {
            Ok(v) if model::flag(&v["cleared"]) => (format!("{}{}", if approve { "Approved" } else { "Denied" }, msg.map(|m| format!(": {m}")).unwrap_or_default()), false),
            Ok(_) => ("Sent, but the prompt is still showing. Look at the pane.".into(), true),
            Err(e) => (e, true),
        });
    }

    pub fn reply(&mut self, m: &model::Msg, choice: &str) {
        let (id, choice, who) = (m.id(), choice.to_string(), m.who());
        self.act(&format!("reply:{id}"), format!("/api/messages/{id}/reply"), json!({ "body": "", "choice": choice }), move |r| match r {
            Ok(v) => (format!("Chose {choice} · it goes to {} between turns", model::text(&v["to"]).unwrap_or(who)), false),
            Err(e) => (e, true),
        });
    }

    pub fn set_status(&mut self, id: &str, s: Status) {
        let id = id.to_string();
        self.act(&format!("status:{id}"), format!("/api/dockets/{id}/set"), json!({ "status": s.raw() }), move |r| match r {
            Ok(_) => (format!("{id} moved to {}", s.label()), false),
            Err(e) => (e, true),
        });
    }

    pub fn note(&mut self, id: &str) {
        let body = self.note.text.trim().to_string();
        if body.is_empty() {
            return;
        }
        self.note.clear();
        let id = id.to_string();
        self.act(&format!("note:{id}"), format!("/api/dockets/{id}/note"), json!({ "body": body }), move |r| match r {
            Ok(_) => ("Noted, on the record".into(), false),
            Err(e) => (e, true),
        });
    }

    pub fn send(&mut self, id: &str) {
        let body = self.note.text.trim().to_string();
        if body.is_empty() {
            return;
        }
        self.note.clear();
        let id = id.to_string();
        self.act(&format!("send:{id}"), format!("/api/dockets/{id}/send"), json!({ "text": body, "queue": true }), move |r| match r {
            Ok(v) if v["queued"].is_number() => ("Queued: it goes in when the turn ends".into(), false),
            Ok(_) => ("Sent to the pane".into(), false),
            Err(e) => (e, true),
        });
    }

    pub fn save_intent(&mut self, id: &str) {
        let Some(edit) = self.intent_edit.take() else { return };
        let id = id.to_string();
        self.act(&format!("intent:{id}"), format!("/api/dockets/{id}/intent"), json!({ "body": edit.text }), move |r| match r {
            Ok(_) => (format!("{id}: intent saved"), false),
            Err(e) => (e, true),
        });
    }

    pub fn hold(&mut self, id: &str) {
        let why = self.hold_why.text.trim().to_string();
        self.hold_why.clear();
        self.sheet = None;
        let id = id.to_string();
        let body = if why.is_empty() { json!({}) } else { json!({ "reason": why }) };
        self.act(&format!("hold:{id}"), format!("/api/dockets/{id}/hold"), body, move |r| match r {
            Ok(_) => (format!("{id} is on hold"), false),
            Err(e) => (e, true),
        });
    }

    pub fn resume(&mut self, id: &str) {
        let id = id.to_string();
        self.act(&format!("hold:{id}"), format!("/api/dockets/{id}/hold"), json!({ "off": true }), move |r| match r {
            Ok(_) => (format!("{id} resumed"), false),
            Err(e) => (e, true),
        });
    }

    pub fn run_checks(&mut self, id: &str) {
        let id = id.to_string();
        self.act(&format!("check:{id}"), format!("/api/dockets/{id}/check"), json!({}), move |r| match r {
            Ok(v) => (format!("Running {}", model::strings(&v["checks"]).join(", ")), false),
            Err(e) => (e, true),
        });
    }

    pub fn open_in_rex(&mut self, id: &str) {
        let id = id.to_string();
        self.act(&format!("rex:{id}"), format!("/api/dockets/{id}/open"), json!({}), move |r| match r {
            Ok(_) => (format!("Showing {id}'s pane in Rex"), false),
            Err(e) => (e, true),
        });
    }

    pub fn post_review(&mut self, p: &model::ReviewPr) {
        let (key, full) = (p.key(), p.full_key());
        let mut body = json!({ "key": full });
        if let Some(d) = p.digest() {
            body["digest"] = json!(d);
        }
        self.act(&format!("post:{full}"), "/api/review/post".into(), body, move |r| match r {
            Ok(_) => (format!("Posted the review of {key}"), false),
            Err(e) => (e, true),
        });
    }

    pub fn dismiss_review(&mut self, p: &model::ReviewPr) {
        let (key, full) = (p.key(), p.full_key());
        self.act(&format!("dismiss:{full}"), "/api/review/dismiss".into(), json!({ "key": full }), move |r| match r {
            Ok(_) => (format!("Dismissed the review of {key}"), false),
            Err(e) => (e, true),
        });
    }
}

/// Before each frame: toasts age out.
pub fn poll(s: &mut Main, t: f64, tx: &mut Tx) -> bool {
    s.clock = t;
    // After the first frame, so the section has a place to scroll to.
    if t > 0.05 {
        if let Some(key) = s.scroll_to.take() {
            tx.scroll_into_view(key);
            return true;
        }
    } else if s.scroll_to.is_some() {
        tx.wake_after(std::time::Duration::from_millis(60));
    }
    let before = s.toasts.len();
    s.toasts.retain(|x| t - x.at < 4.0);
    if !s.toasts.is_empty() {
        tx.wake_after(std::time::Duration::from_millis(500));
    }
    before != s.toasts.len()
}

/// The window's keys: G then a letter goes somewhere, / searches, C creates,
/// ⌘1…⌘8 go to the places, ⌥⌘I shows the inspector.
pub fn on_key(s: &mut Main, ev: &KeyEvent, _: &mut Tx) -> bool {
    if rune_kit::shell::is_sidebar_toggle(ev) {
        s.sidebar.toggle();
        return true;
    }
    if ev.is_command('i') && ev.mods.alt {
        s.inspector.toggle();
        return true;
    }
    if ev.mods.cmd {
        if let Key::Command(c) = &ev.key {
            if let Some(n) = c.to_digit(10).filter(|n| (1..=8).contains(n)) {
                s.go(Place::ORDER[n as usize - 1].clone());
                return true;
            }
            let cur = s.current();
            match (c, ev.mods.shift) {
                ('k', false) => s.open_palette(),
                ('n', false) => s.open_form(Sheet::NewDocket(NewMode::Blank)),
                ('n', true) => s.open_form(Sheet::NewDocket(NewMode::Agent)),
                ('h', true) => {
                    if let Some(id) = cur {
                        let held = s.state().and_then(|st| st.docket(&id).map(|d| d.hold().is_some())).unwrap_or(false);
                        if held { s.resume(&id) } else { s.sheet = Some(Sheet::Hold(id)) }
                    }
                }
                ('u', true) => {
                    if let Some(id) = cur {
                        s.run_checks(&id);
                    }
                }
                _ => return false,
            }
            return true;
        }
        return false;
    }
    let Key::Text(t) = &ev.key else {
        return match ev.key {
            Key::Escape if s.sheet.is_some() => {
                s.sheet = None;
                true
            }
            _ => false,
        };
    };
    if s.pending_g {
        s.pending_g = false;
        let place = match t.as_str() {
            "i" => Place::Inbox,
            "r" => Place::Reviews,
            "b" => Place::Builds,
            "a" => Place::Active,
            "d" => Place::All,
            "x" => Place::Done,
            "n" => Place::Agents,
            "p" => Place::Recap,
            _ => return false,
        };
        s.go(place);
        return true;
    }
    match t.as_str() {
        "g" => s.pending_g = true,
        "j" | "k" => lists::step(s, if t == "j" { 1 } else { -1 }),
        "c" => s.open_form(Sheet::NewDocket(NewMode::Blank)),
        "/" => s.open_palette(),
        _ => return false,
    }
    true
}

pub fn view(s: &Main, cx: &mut Cx<Main>) -> El {
    let dark = s.theme.unwrap_or(cx.system_dark());
    let k = K::new(dark, s.dev());
    let kit = k.kit();
    let state = s.state();
    let main = match &state {
        None => hub_down(s, &k),
        Some(st) => content(s, st, &k, &kit),
    };
    let inspector = match (&state, s.current()) {
        (Some(st), Some(id)) if s.place != Place::Agents && s.place != Place::Recap => docket::inspector(s, st, &id, &k),
        _ => col(),
    };
    let toolbar = toolbar(s, state.as_deref(), &k, &kit);
    let split = SplitView::new(main)
        .sidebar(&s.sidebar, |s: &mut Main| &mut s.sidebar, sidebar::sidebar(s, state.as_deref(), &k, &kit))
        .inspector(&s.inspector, |s: &mut Main| &mut s.inspector, inspector)
        .toolbar(toolbar)
        .titlebar_inset(cx.titlebar_inset())
        .frame_inset(8.0)
        .toolbar_in_panel()
        .sidebar_padding(10.0, 10.0)
        .reduced_motion(cx.is_reduced_motion())
        .build(&kit);
    let mut root = col().key("root").w_full().h_full().fill(if dark { theme::hex("#1b1b1d") } else { theme::hex("#f1f1f3") }).child(split);
    if let Some(sheet) = &s.sheet {
        root = root.child(docket::sheet(s, sheet, &k, &kit));
    }
    if let (Some(_), Some(st)) = (&s.palette, &state) {
        root = root.child(palette::view(s, st, &k));
    }
    if !s.toasts.is_empty() {
        root = root.child(widgets::toasts(&s.toasts, &k));
    }
    root
}

fn toolbar(s: &Main, state: Option<&HubState>, k: &K, kit: &rune_kit::Kit) -> Toolbar {
    let subtitle = state.map(|st| subtitle(s, st)).unwrap_or_default();
    let title = col()
        .gap(1.0)
        .child(text(s.place.title()).font(font(13.0, 600.0).lh(16.0)).color(k.text))
        .child(text(subtitle).font(font(11.0, 400.0).lh(13.0)).color(k.muted));
    let mut trailing = row().gap(6.0).center_y();
    if s.place.is_list() {
        trailing = trailing.child(widgets::segmented(
            "layout",
            &[("List", !s.board), ("Board", s.board)],
            |s: &mut Main, i| s.board = i == 1,
            k,
        ));
    }
    if let (Some(st), Some(id)) = (state, s.current()) {
        if st.docket(&id).is_some_and(|d| d.talks()) && !s.board {
            trailing = trailing.child(widgets::segmented("tab", &[("Chat", s.chat), ("Docket", !s.chat)], |s: &mut Main, i| s.chat = i == 0, k));
        }
    }
    trailing = trailing.child(widgets::icon_button(
        "inspector-toggle",
        &crate::icons::INSPECTOR,
        if s.inspector.is_open() { "Hide the inspector (⌥⌘I)" } else { "Show the inspector (⌥⌘I)" },
        |s: &mut Main| s.inspector.toggle(),
        k,
    ));
    let _ = kit;
    Toolbar::new().leading(title).trailing(trailing)
}

fn subtitle(s: &Main, st: &HubState) -> String {
    match &s.place {
        Place::Inbox => {
            let items = model::inbox(st);
            let concerns = model::concerns(st, &items);
            let blocked = items.iter().filter(|i| i.kind == model::InboxKind::Blocked).count();
            let failed = items.iter().filter(|i| i.kind == model::InboxKind::Error).count();
            let mut parts = vec![];
            if blocked > 0 {
                parts.push(format!("{blocked} waiting on you"));
            }
            if failed > 0 {
                parts.push(format!("{failed} failed"));
            }
            if !items.is_empty() {
                parts.push(format!("{} in all", items.len()));
            }
            if !concerns.is_empty() {
                parts.push(format!("{} worth a look", concerns.len()));
            }
            parts.join(" · ")
        }
        Place::Reviews => {
            let n = st.prs.iter().filter(|p| matches!(p.state().as_str(), "queued" | "reviewing" | "posted")).count();
            if n > 0 { format!("{n} live") } else { String::new() }
        }
        Place::Builds => {
            let n = st.tasks.iter().filter(|t| matches!(t.state().as_str(), "queued" | "active" | "open")).count();
            if n > 0 { format!("{n} live") } else { String::new() }
        }
        Place::Agents => model::plural(st.agents().len(), "agent"),
        Place::Recap => String::new(),
        p => format!("{}", st.dockets.iter().filter(|d| p.shows(d)).count()),
    }
}

fn content(s: &Main, st: &HubState, k: &K, kit: &rune_kit::Kit) -> El {
    match &s.place {
        Place::Agents => lists::agents(s, st, k),
        Place::Recap => lists::recap(s, st, k),
        p if p.is_list() && s.board => lists::board(s, st, k),
        _ => {
            let list = lists::list(s, st, k, kit);
            let detail = match s.current() {
                Some(id) => docket::detail(s, st, &id, k, kit),
                None => widgets::empty(&crate::icons::TRAY, "Nothing selected", "Pick something from the list.", k),
            };
            row()
                .key("columns")
                .w_full()
                .h_full()
                .child(col().key("list-column").w(360.0).h_full().min_h(0.0).child(list))
                .child(el().w(1.0).h_full().fill(k.line).a11y_hidden())
                .child(col().key("detail-column").grow(1.0).min_w(0.0).h_full().min_h(0.0).child(detail))
        }
    }
}

fn hub_down(s: &Main, k: &K) -> El {
    let err = s.store.error().unwrap_or_else(|| "Connecting…".into());
    widgets::empty(&crate::icons::REFRESH, "The hub isn't running", &format!("{err} ({}). Start it with `wh-dev up` for Wheelhouse Dev, or the made-up world's script.", s.store.hub.base()), k)
}

// ---- scenes, for headless shots: a place, a selection, the docket's page ----

fn scene(s: &mut Main, p: Place, select: Option<&str>) {
    s.place = p;
    if let Some(id) = select {
        s.selection.insert(s.place.clone(), id.to_string());
    }
    if let Some(id) = s.current() {
        s.load_page(&id);
    }
}

pub const SCENES: &[(&str, fn(&mut Main))] = &[
    ("inbox", |s| scene(s, Place::Inbox, None)),
    ("docket", |s| scene(s, Place::Active, Some("D-6"))),
    ("chat", |s| {
        scene(s, Place::Active, Some("D-6"));
        s.chat = true;
    }),
    ("reviews", |s| scene(s, Place::Reviews, Some("D-7"))),
    ("builds", |s| scene(s, Place::Builds, None)),
    ("board", |s| {
        scene(s, Place::All, None);
        s.board = true;
    }),
    ("all", |s| scene(s, Place::All, None)),
    ("done", |s| scene(s, Place::Done, None)),
    ("agents", |s| scene(s, Place::Agents, None)),
    ("recap", |s| scene(s, Place::Recap, None)),
    ("record", |s| {
        scene(s, Place::Active, Some("D-6"));
        s.sheet = Some(Sheet::Record("D-6".into()));
    }),
    ("new", |s| {
        scene(s, Place::Inbox, None);
        s.open_form(Sheet::NewDocket(NewMode::Blank));
    }),
    ("agent", |s| {
        scene(s, Place::Inbox, None);
        s.open_form(Sheet::NewDocket(NewMode::Agent));
    }),
    ("palette", |s| {
        scene(s, Place::Inbox, None);
        s.open_palette();
    }),
    ("start-agent", |s| {
        scene(s, Place::Active, Some("D-6"));
        s.open_form(Sheet::StartAgent("D-6".into()));
    }),
    ("verify", |s| {
        scene(s, Place::Active, Some("D-6"));
        s.scroll_to = Some("verify".into());
    }),
    ("verify-held", |s| {
        scene(s, Place::Active, Some("D-3"));
        s.scroll_to = Some("verify".into());
    }),
    ("hold", |s| {
        scene(s, Place::Active, Some("D-6"));
        s.sheet = Some(Sheet::Hold("D-6".into()));
    }),
];
