//! The main window (RootView.swift): the sidebar and its places, a list beside
//! the docket it shows, and the docket's inspector, on rune-kit's shell. Every
//! window reads the one Store and is told when it changes.

mod activity;
mod chat;
mod docket;
mod fixtures;
mod forms;
mod glyphs;
mod lists;
pub mod menubar;
mod menus;
pub mod notify;
mod palette;
mod panels;
mod parts;
mod recap;
mod record;
pub mod settings;
mod shortcuts;
mod sidebar;
mod verify;
pub mod widgets;
pub mod windows;

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
    /// ⌘/ or ?: the keyboard's shortcuts.
    Shortcuts,
    /// A check's output: (docket, check).
    CheckOutput(String, String),
    /// Done needs evidence: close on your word, from the status it was in.
    CloseOnWord(String, Status),
    /// A live look at an agent's pane.
    Pane(String),
    /// A review as it would post, by its full key.
    Preview(String),
}

/// Which window a `Main` draws (windows.rs): every one acts on the hub the same way.
#[derive(Clone, Debug, PartialEq)]
pub enum Window {
    Main,
    /// The menu bar's panel: what needs you, and no docket page.
    MenuBar,
    /// A docket in a window of its own (DocketWindow); None while it's free.
    Docket(Option<String>),
}

pub struct Main {
    pub store: Arc<Store>,
    pub version: u64,
    pub me: Option<AppHandle<Main>>,
    pub window: Window,
    pub place: Place,
    pub selection: HashMap<Place, String>,
    pub sidebar: PaneState,
    pub inspector: PaneState,
    pub pages: HashMap<String, Arc<DocketPage>>,
    pub page_errors: HashMap<String, String>,
    pub busy: HashSet<String>,
    pub folded: HashSet<String>,
    /// Per place, as the Mac app keeps them: the board, and how lists group.
    pub boards: HashSet<Place>,
    pub group_by: HashMap<Place, GroupBy>,
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
    pub recap: recap::Recap,
    pub talk: chat::Talk,
    pub record: record::Record,
    pub verify: verify::Verify,
    pub panels: panels::Panels,
}

impl Main {
    pub fn new(store: Arc<Store>, me: AppHandle<Main>) -> Main {
        let mut m = Main {
            store: store.clone(),
            version: 0,
            me: Some(me.clone()),
            window: Window::Main,
            place: Place::Inbox,
            selection: HashMap::new(),
            sidebar: PaneState::new(228.0).range(200.0, 320.0),
            inspector: PaneState::new(280.0).range(240.0, 360.0),
            pages: HashMap::new(),
            page_errors: HashMap::new(),
            busy: HashSet::new(),
            folded: ["reviews:done".to_string(), "builds:done".to_string()].into_iter().collect(),
            boards: HashSet::new(),
            group_by: HashMap::new(),
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
            recap: Default::default(),
            talk: Default::default(),
            record: Default::default(),
            verify: Default::default(),
            panels: Default::default(),
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
        match &self.window {
            Window::Main => {}
            Window::MenuBar => return None,
            Window::Docket(id) => return id.clone().filter(|id| state.docket(id).is_some()),
        }
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
                lists::group_dockets(&shown, self.group()).into_iter().flat_map(|g| g.items).map(|d| d.id()).next()
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
        if self.place == Place::Inbox
            && let Some(it) = model::inbox(&state).into_iter().find(|i| i.docket_id == id)
        {
            return self.select(it.id);
        }
        if let Some(d) = state.docket(id)
            && self.place.shows(d)
        {
            return self.select(id.to_string());
        }
        if let Some(it) = model::inbox(&state).into_iter().find(|i| i.docket_id == id) {
            self.place = Place::Inbox;
            return self.select(it.id);
        }
        self.place = Place::All;
        self.select(id.to_string());
    }

    /// The docket the window shows beside its list, if any: none on Recap,
    /// Agents or a board.
    pub fn shown_docket(&self) -> Option<String> {
        if self.embedded() || matches!(self.place, Place::Agents | Place::Recap) || (self.place.is_list() && self.board()) {
            return None;
        }
        let id = self.current()?;
        self.state()?.docket(&id).map(|_| id)
    }

    /// How this place's list groups: All and Done by status, the rest by
    /// what needs you, until you pick (Lists.swift's defaults).
    pub fn group(&self) -> GroupBy {
        self.group_by.get(&self.place).copied().unwrap_or(if matches!(self.place, Place::All | Place::Done) { GroupBy::Status } else { GroupBy::Attention })
    }
    pub fn board(&self) -> bool {
        self.boards.contains(&self.place)
    }
    pub fn set_board(&mut self, on: bool) {
        if on {
            self.boards.insert(self.place.clone());
        } else {
            self.boards.remove(&self.place);
        }
    }

    /// The Inbox shows a docket inside it, as the Mac app's `embedded`
    /// DocketView: its id row has the docket's buttons, and there's no
    /// inspector, chat or toolbar of its own.
    pub fn embedded(&self) -> bool {
        self.place == Place::Inbox && self.window == Window::Main
    }

    /// A docket in a window of its own (windows.rs).
    pub fn open_window(&mut self, id: &str) {
        windows::open_docket(id);
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

    /// Who posts a checked review. Back to the config's own mode clears the
    /// override instead of pinning it (ReviewsScreen's Picker).
    pub fn review_posting(&mut self, v: &str) {
        let Some(st) = self.state() else { return };
        let rv = st.review();
        let c = &rv["config"];
        let ruled = model::str_or(&c["post"], "") == "rule" || c["repo_posts"].as_object().is_some_and(|o| !o.is_empty());
        let post = if v == "rule" || (rv["post_override"].is_string() && !ruled && model::str_or(&c["post"], "") == v) { "config" } else { v };
        let done = match v {
            "rule" => "Each repo posts as your rules say",
            "auto" => "The checker posts each review",
            _ => "You post each review",
        };
        self.act("review:settings", "/api/review/settings".into(), json!({ "post": post }), move |r| match r {
            Ok(_) => (done.into(), false),
            Err(e) => (e, true),
        });
    }

    pub fn review_paused(&mut self, paused: bool) {
        self.act("review:settings", "/api/review/settings".into(), json!({ "paused": paused }), move |r| match r {
            Ok(_) => (if paused { "Reviews paused: no new rounds start" } else { "Reviews resumed" }.into(), false),
            Err(e) => (e, true),
        });
    }

    pub fn check_github(&mut self) {
        self.act("review:check", "/api/review/check".into(), json!({}), |r| match r {
            Ok(_) => ("Checking GitHub".into(), false),
            Err(e) => (e, true),
        });
    }

    /// Build it: the docket's intent to a builder and a checker, into `repo`.
    pub fn build(&mut self, id: &str, repo: &str) {
        let (id, repo) = (id.to_string(), repo.to_string());
        self.act(&format!("build:{id}"), "/api/build/start".into(), json!({ "from": id, "repo": repo }), move |r| match r {
            Ok(_) => (format!("{id} is queued to build into {repo}"), false),
            Err(e) => (e, true),
        });
    }

    /// Run a docket's checks (Verify's Run Checks).
    pub fn run_checks(&mut self, id: &str) {
        verify::run(self, id, false);
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
            tx.scroll_center(key);
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
    // Recap, a record or a check's output that just opened, and Chat's peek.
    let loaded = recap::poll(s) | record::poll(s) | verify::poll(s);
    let ticked = chat::poll(s, t, tx);
    before != s.toasts.len() || loaded || ticked
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
    // ⌘/ (and ? below): the keyboard's shortcuts.
    if ev.is_command('/') || (ev.mods.cmd && ev.is_command('?')) {
        s.sheet = Some(Sheet::Shortcuts);
        return true;
    }
    if ev.is_command(',') && !ev.mods.alt && !ev.mods.shift {
        settings::open(s.theme);
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
                ('a', true) => {
                    if let Some(id) = cur {
                        s.open_form(Sheet::StartAgent(id));
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
    // The Inbox's keys, on the item it shows: approve, deny, an option, done, the pane.
    if s.place == Place::Inbox && inbox_key(s, t) {
        return true;
    }
    match t.as_str() {
        "g" => s.pending_g = true,
        "j" | "k" => lists::step(s, if t == "j" { 1 } else { -1 }),
        "c" => s.open_form(Sheet::NewDocket(NewMode::Blank)),
        "/" => s.open_palette(),
        "?" => s.sheet = Some(Sheet::Shortcuts),
        _ => return false,
    }
    true
}

fn inbox_key(s: &mut Main, t: &str) -> bool {
    let Some(st) = s.state() else { return false };
    let items = model::inbox(&st);
    let sel = s.selection.get(&Place::Inbox).cloned();
    let Some(it) = items.iter().find(|i| Some(&i.id) == sel.as_ref()).or(items.first()).cloned() else { return false };
    match t {
        "a" | "d" => match it.docket.filter(|d| it.kind == model::InboxKind::Blocked && d.live().is_some_and(|l| l.answerable())) {
            Some(d) => s.answer(&d, t == "a"),
            None => return false,
        },
        "e" => match it.docket.filter(|d| d.is_open()) {
            Some(d) => s.set_status(&d.id(), Status::Done),
            None => return false,
        },
        "o" => match it.docket.filter(|d| d.live().is_some()) {
            Some(d) => s.open_in_rex(&d.id()),
            None => return false,
        },
        n => {
            let Some(i) = n.parse::<usize>().ok().filter(|i| (1..=6).contains(i)) else { return false };
            match it.msg.filter(|m| i <= m.options().len()) {
                Some(m) => {
                    let o = m.options()[i - 1].clone();
                    s.reply(&m, &o);
                }
                None => return false,
            }
        }
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
    let toolbar = toolbar(s, state.as_deref(), &k, &kit);
    let mut split = SplitView::new(main).sidebar(&s.sidebar, |s: &mut Main| &mut s.sidebar, sidebar::sidebar(s, state.as_deref(), &k, &kit));
    // The inspector is the docket's, as in the Mac app: none where no docket shows.
    if let (Some(st), Some(id)) = (&state, s.shown_docket()) {
        split = split.inspector(&s.inspector, |s: &mut Main| &mut s.inspector, docket::inspector(s, st, &id, &k));
    }
    let split = split
        .toolbar(toolbar)
        // The toggle at the sidebar's end, Search and New docket past the
        // traffic lights (the sidebar's toolbar in RootView).
        .sidebar_toggle(rune_kit::shell::SidebarToggle::new().in_header())
        .titlebar_inset(cx.titlebar_inset())
        .frame_inset(8.0)
        .toolbar_in_panel()
        .sidebar_padding(10.0, 10.0)
        .reduced_motion(cx.is_reduced_motion())
        .build(&kit);
    let mut root = col().key("root").w_full().h_full().fill(if dark { theme::hex("#1b1b1d") } else { theme::hex("#f1f1f3") }).child(split);
    if s.sidebar.is_open() {
        let x = if cx.titlebar_inset() > 0.0 { cx.titlebar_inset() + 8.0 } else { 12.0 };
        root = root.child(
            row()
                .key("sidebar-tools")
                .raise()
                .absolute(x, 8.0 + (48.0 - 26.0) / 2.0)
                .gap(2.0)
                .child(widgets::icon_button("search", &crate::icons::SEARCH, "Search dockets and commands (⌘K)", |s: &mut Main| s.open_palette(), &k))
                .child(widgets::icon_button("compose", &crate::icons::COMPOSE, "New docket (⌘N)", |s: &mut Main| s.open_form(Sheet::NewDocket(NewMode::Blank)), &k)),
        );
    }
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
    let title = col().gap(1.0).child(text(s.place.title()).font(font(13.0, 600.0).lh(16.0)).color(k.text)).child(text(subtitle).font(font(11.0, 400.0).lh(13.0)).color(k.muted));
    let mut trailing = row().gap(6.0).center_y();
    if s.place == Place::Recap {
        trailing = trailing.child(recap::toolbar(s, k));
    }
    if s.place.is_list() {
        // Group by: a native menu, as the SwiftUI toolbar's.
        let cur = match s.group() {
            GroupBy::Attention => "attention",
            GroupBy::Status => "status",
            GroupBy::Session => "session",
        };
        trailing = trailing.child(
            row()
                .key("group-by")
                .h(24.0)
                .px(8.0)
                .gap(4.0)
                .radius(6.0)
                .center_y()
                .hover(k.hover)
                .role("button")
                .label("Group by")
                .help("Group by")
                .on_press_tx(move |_: &mut Main, tx| {
                    let menu = rune::PopupMenu::picker([("attention", "Attention"), ("status", "Status"), ("session", "Rex session")], cur);
                    tx.popup_menu(menu, "group-by", |s: &mut Main, picked, _| {
                        let g = match picked {
                            Some("status") => GroupBy::Status,
                            Some("session") => GroupBy::Session,
                            Some(_) => GroupBy::Attention,
                            None => return,
                        };
                        s.group_by.insert(s.place.clone(), g);
                    });
                })
                .child(rune::icon(&crate::icons::LIST, 14.0).color(k.muted))
                .child(rune::icon(&crate::icons::CHEVRON, 10.0).color(k.muted)),
        );
        trailing = trailing.child(widgets::segmented("layout", &[("List", !s.board()), ("Board", s.board())], |s: &mut Main, i| s.set_board(i == 1), k));
    }
    // Reviews' own (ReviewsScreen.toolbar): who posts, pause, check GitHub now.
    if let Some(rv) = state.map(|st| st.review()).filter(|rv| s.place == Place::Reviews && model::flag(&rv["enabled"])) {
        let c = &rv["config"];
        let ruled = model::str_or(&c["post"], "") == "rule" || c["repo_posts"].as_object().is_some_and(|o| !o.is_empty());
        let posting = model::text(&rv["post_override"]).unwrap_or_else(|| {
            if ruled {
                "rule".into()
            } else if model::str_or(&c["post"], "") == "auto" {
                "auto".into()
            } else {
                "ask".into()
            }
        });
        let mut segs = vec![("Ask", posting == "ask"), ("Auto", posting == "auto")];
        if ruled {
            segs.push(("Rules", posting == "rule"));
        }
        trailing = trailing.child(widgets::segmented("posting", &segs, |s: &mut Main, i| s.review_posting(["ask", "auto", "rule"][i]), k).help(if ruled {
            "Who posts a checked review: you (Ask), the checker (Auto), or your rules, repo by repo (Rules, in Settings)"
        } else {
            "Who posts a checked review: you (Ask), or the checker (Auto)"
        }));
        let paused = model::flag(&rv["paused"]);
        trailing = trailing
            .child(widgets::icon_button(
                "review-pause",
                if paused { &crate::icons::PLAY } else { &crate::icons::PAUSE },
                if paused { "Resume: new rounds start" } else { "Pause: no new rounds start" },
                move |s: &mut Main| s.review_paused(!paused),
                k,
            ))
            .child(widgets::icon_button("review-check", &crate::icons::REFRESH, "Check GitHub for review requests now", |s: &mut Main| s.check_github(), k));
    }
    let Some(d) = s.shown_docket().and_then(|id| state.and_then(|st| st.docket(&id).cloned())) else {
        let _ = kit;
        return Toolbar::new().leading(title).trailing(trailing);
    };
    // The docket's own toolbar (DocketView.toolbar): Chat or Docket, its pane, its menu.
    if d.talks() {
        trailing = trailing.child(widgets::segmented("tab", &[("Chat", s.chat), ("Docket", !s.chat)], |s: &mut Main, i| s.chat = i == 0, k));
    }
    if d.live().is_some() {
        let (a, b) = (d.id(), d.id());
        trailing = trailing
            .child(widgets::icon_button("open-rex", &crate::icons::TERMINAL, "Bring the agent's pane to the front in Rex (O)", move |s: &mut Main| s.open_in_rex(&a), k))
            .child(widgets::icon_button("peek", &crate::icons::EYE, "Peek at the agent's pane here", move |s: &mut Main| chat::open_pane(s, &b), k));
    }
    let d2 = d.clone();
    trailing = trailing.child(
        widgets::icon_button("docket-menu", &crate::icons::MORE, "Everything you can do to this docket", |_: &mut Main| {}, k)
            .on_press_tx(move |_: &mut Main, tx| menus::open(&d2, "docket-menu", tx)),
    );
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
        Place::Recap => recap::view(s, k),
        p if p.is_list() && s.board() => lists::board(s, st, k),
        _ => {
            let list = lists::list(s, st, k, kit);
            let detail = match s.current() {
                Some(id) => docket::detail(s, st, &id, k, kit),
                None => widgets::empty(&crate::icons::TRAY, "Nothing selected", "Pick something from the list.", k),
            };
            let columns = row()
                .key("columns")
                .w_full()
                .grow(1.0)
                .min_h(0.0)
                .child(col().key("list-column").w(360.0).h_full().min_h(0.0).child(list))
                .child(el().w(1.0).h_full().fill(k.line).a11y_hidden())
                .child(col().key("detail-column").grow(1.0).min_w(0.0).h_full().min_h(0.0).child(detail));
            // Reviews' status runs across both, as ReviewsBar does.
            match lists::reviews_bar(st, k).filter(|_| s.place == Place::Reviews) {
                Some(bar) => col().w_full().h_full().child(bar).child(columns),
                None => columns.h_full(),
            }
        }
    }
}

fn hub_down(s: &Main, k: &K) -> El {
    let err = s.store.error().unwrap_or_else(|| "Connecting…".into());
    widgets::empty(
        &crate::icons::REFRESH,
        "The hub isn't running",
        &format!("{err} ({}). Start it with `wh-dev up` for Wheelhouse Dev, or the made-up world's script.", s.store.hub.base()),
        k,
    )
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
        s.set_board(true);
    }),
    ("all", |s| scene(s, Place::All, None)),
    ("done", |s| scene(s, Place::Done, None)),
    ("agents", |s| scene(s, Place::Agents, None)),
    ("recap", fixtures::recap),
    ("record", |s| {
        scene(s, Place::Active, Some("D-6"));
        s.sheet = Some(Sheet::Record("D-6".into()));
    }),
    ("record-open", fixtures::record_open),
    ("envelope", fixtures::envelope),
    ("record-find", fixtures::record_find),
    ("chat-peek", fixtures::chat_peek),
    ("chat-blocked", fixtures::chat_blocked),
    ("pane", fixtures::pane),
    ("output", fixtures::output),
    ("close", fixtures::close),
    ("verdicts", fixtures::verdicts),
    ("shortcuts", fixtures::shortcuts),
    ("review-ready", fixtures::review_ready),
    ("review-live", fixtures::review_live),
    ("preview", fixtures::preview),
    ("build-plan", fixtures::build_plan),
    ("build-check", fixtures::build_check),
    ("build-round", fixtures::build_round),
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
