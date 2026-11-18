//! The ⌥⌘K quick panel (the trial, docs/rune.md), as a window of the whole
//! app: ⌃⌥⌘K from any app shows it over whatever you're in. Approve or deny a
//! prompt, pick an agent's option, start a docket, or find one.

use std::collections::HashSet;
use std::sync::mpsc;
use std::time::Duration;

use rune::{Align, App, AppHandle, AppSpec, DisplayTarget, Key, KeyEvent, Latest, OverlayPlacement, ScreenEdge, TextEdit, Tx, Waker, WindowOptions, WindowSpec};
use serde_json::{Value, json};

use crate::commands::{self, Action, Command, Issue, World};
use crate::hub::{self, Hub};
use crate::view;

/// The panel's chord: ⌃⌥⌘K, so it never takes the installed app's ⌥⌘K.
pub const CHORD: &str = "ctrl+alt+cmd+k";

/// What the panel is showing. One lives in the state, so the forms' fields stay unboxed.
#[allow(clippy::large_enum_variant)]
pub enum Mode {
    Palette,
    NewDocket {
        title: TextEdit,
        note: TextEdit,
    },
    StartAgent {
        prompt: TextEdit,
        folder: TextEdit,
        codex: bool,
        title: TextEdit,
    },
    /// Search Linear: your open assigned issues, or by words, or a key.
    Linear {
        query: TextEdit,
        issues: Vec<Issue>,
        error: Option<String>,
        sel: usize,
        loading: bool,
    },
}

/// The outcome of the last thing done, at the panel's foot.
#[derive(Clone)]
pub struct Note {
    pub text: String,
    pub error: bool,
}

pub struct Panel {
    pub hub: Hub,
    pub world: Option<World>,
    /// Why the hub can't be read, when it can't.
    pub error: Option<String>,
    /// Following the hub's events.
    pub live: bool,
    pub query: TextEdit,
    pub sel: usize,
    pub mode: Mode,
    pub note: Option<Note>,
    /// Commands sent and not yet answered.
    pub pending: HashSet<String>,
    /// The theme Theme: Light or Dark chose; None follows the system.
    pub theme: Option<bool>,
    pub me: Option<AppHandle<Panel>>,
    /// Linear's answers, one search a keystroke, the newest kept.
    linear: Latest<Result<Vec<Issue>, String>>,
    /// A headless run (--snap, --tree, --script): calls are inline, on the
    /// run's virtual clock, so its frames don't depend on the network.
    pub headless: bool,
    /// `--demo`: step the selection down the list on a timer, for timing frames in a window.
    demo: bool,
    refresh: Option<mpsc::Sender<()>>,
}

impl Panel {
    fn new(hub: Hub, me: AppHandle<Panel>) -> Panel {
        let headless = rune::tooling::headless_run().is_some();
        let mut query = TextEdit::new("");
        query.focused = true;
        let mut p = Panel {
            hub,
            world: None,
            error: None,
            live: false,
            query,
            sel: 0,
            mode: Mode::Palette,
            note: None,
            pending: HashSet::new(),
            // WH_RUNE_THEME=light|dark, else the system's.
            theme: match std::env::var("WH_RUNE_THEME").as_deref() {
                Ok("light") => Some(false),
                Ok("dark") => Some(true),
                _ => None,
            },
            me: Some(me),
            linear: Latest::default().debounce(Duration::from_millis(220)),
            headless,
            demo: !headless && rune::tooling::extra_args().iter().any(|a| a == "--demo"),
            refresh: None,
        };
        if headless {
            let r = p.hub.get("/api/state");
            p.landed(r);
        } else {
            appearance(p.theme);
        }
        p
    }

    /// The hub's state arrived (or didn't).
    fn landed(&mut self, r: Result<Value, String>) {
        match r {
            Ok(j) => {
                let w = World::parse(&j);
                // WH_RUNE_LOG=1: a line for each state that lands, to watch it follow the hub.
                if std::env::var_os("WH_RUNE_LOG").is_some() {
                    eprintln!("wheelhouse-rune: state landed, {} dockets, {} need you, live {}", w.dockets.len(), w.needs.len(), self.live);
                }
                // An answered prompt or ask leaves the list: it's no longer pending.
                let ids: HashSet<String> = commands::commands(&w).into_iter().map(|c| c.id).collect();
                self.pending.retain(|p| ids.contains(p));
                if self.world.as_ref() != Some(&w) {
                    self.world = Some(w);
                }
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
        let n = self.shown().len();
        if self.sel >= n {
            self.sel = n.saturating_sub(1);
        }
    }

    pub fn shown(&self) -> Vec<Command> {
        commands::shown(commands::commands(self.world.as_ref().unwrap_or(&World::default())), &self.query.text)
    }

    /// Shown again: empty, at the top, and focused.
    fn reset(&mut self) {
        self.query.clear();
        self.query.focused = true;
        self.sel = 0;
        self.mode = Mode::Palette;
        self.note = None;
    }

    /// Ask for the hub's state again now.
    fn reload(&mut self) {
        if self.headless {
            let r = self.hub.get("/api/state");
            self.landed(r);
        } else if let Some(tx) = &self.refresh {
            let _ = tx.send(());
        }
    }

    /// Send a write; `done` words its answer. The row it came from waits,
    /// dimmed, until the hub answers.
    fn act(&mut self, id: String, path: String, body: Value, done: impl FnOnce(Result<Value, String>) -> Note + Send + 'static) {
        if !self.pending.insert(id.clone()) {
            return;
        }
        if self.headless {
            let r = self.hub.post(&path, body);
            self.finish(&id, done(r));
            return;
        }
        let (hub, me) = (self.hub.clone(), self.me.clone());
        std::thread::spawn(move || {
            let note = done(hub.post(&path, body));
            if let Some(me) = me {
                me.update(move |s: &mut Panel, _| s.finish(&id, note));
            }
        });
    }

    fn finish(&mut self, id: &str, note: Note) {
        if note.error {
            self.pending.remove(id);
        }
        self.note = Some(note);
        self.reload();
    }

    /// Run the selected command (Return), or the one pressed.
    pub fn run(&mut self, c: &Command, tx: &mut Tx) {
        let id = c.id.clone();
        match c.action.clone() {
            Action::Answer { docket, approve, record, since, msg } => {
                let body = json!({ "record": record, "updated_at": since, "msg": msg });
                self.act(id, format!("/api/dockets/{docket}/{}", if approve { "approve" } else { "deny" }), body, move |r| match r {
                    Ok(v) if v["cleared"].as_bool() == Some(true) => {
                        ok(format!("{} {docket}: {}", if approve { "Approved" } else { "Denied" }, msg.as_deref().unwrap_or("the prompt")))
                    }
                    Ok(_) => fail("Sent, but the prompt is still showing. Look at the pane."),
                    Err(e) => fail(e),
                });
            }
            Action::Choose { id: m, choice } => {
                self.act(id, format!("/api/messages/{m}/reply"), json!({ "body": "", "choice": choice }), move |r| match r {
                    Ok(v) => ok(format!("Chose {choice} · it goes to {} between turns", v["to"].as_str().unwrap_or("the agent"))),
                    Err(e) => fail(e),
                });
            }
            Action::Post { full_key, key, digest } => {
                self.act(id, "/api/review/post".into(), json!({ "key": full_key, "digest": digest }), move |r| match r {
                    Ok(_) => ok(format!("Posted the review of {key}")),
                    Err(e) => fail(e),
                });
            }
            Action::NewDocket => {
                let mut title = TextEdit::new("");
                title.focused = true;
                self.mode = Mode::NewDocket { title, note: TextEdit::new("") };
                self.note = None;
            }
            Action::StartAgent => {
                let mut prompt = TextEdit::new("");
                prompt.focused = true;
                let folder = self.world.as_ref().and_then(|w| w.folders.first().cloned()).unwrap_or_default();
                self.mode = Mode::StartAgent { prompt, folder: TextEdit::new(&folder), codex: false, title: TextEdit::new("") };
                self.note = None;
            }
            Action::FromLinear => {
                let mut query = TextEdit::new("");
                query.focused = true;
                self.mode = Mode::Linear { query, issues: Vec::new(), error: None, sel: 0, loading: true };
                self.note = None;
                self.search_linear();
            }
            Action::Open(docket) => {
                let url = format!("{}/#/d/{docket}", self.hub.base());
                if self.headless {
                    self.note = Some(ok(format!("Opens {docket} in the web app")));
                } else {
                    let _ = std::process::Command::new("/usr/bin/open").arg(&url).spawn();
                    tx.close_window();
                }
            }
            Action::Theme(t) => {
                self.theme = t;
                tx.color_motion(rune::tok::motion::CALM);
                if !self.headless {
                    appearance(t);
                }
            }
        }
    }

    /// Ask Linear (through the hub) for what the search field says.
    pub fn search_linear(&mut self) {
        let Mode::Linear { query, loading, .. } = &mut self.mode else { return };
        *loading = true;
        let q = query.text.clone();
        let hub = self.hub.clone();
        let search = move |q: String| -> Result<Vec<Issue>, String> {
            let v = hub.get(&format!("/api/linear/search?q={}", hub::encode(&q)))?;
            if let Some(e) = v["error"].as_str() {
                return Err(e.to_string());
            }
            Ok(v["issues"].as_array().map(|a| a.iter().map(Issue::parse).collect()).unwrap_or_default())
        };
        if self.headless {
            let r = search(q);
            self.linear_landed(r);
        } else {
            self.linear.request(q, search);
        }
    }

    fn linear_landed(&mut self, r: Result<Vec<Issue>, String>) {
        if let Mode::Linear { issues, error, sel, loading, .. } = &mut self.mode {
            *loading = false;
            *sel = 0;
            match r {
                Ok(i) => {
                    *issues = i;
                    *error = None;
                }
                Err(e) => {
                    issues.clear();
                    *error = Some(e);
                }
            }
        }
    }

    /// Make a docket from the issue (or find the one it already is).
    pub fn pick_issue(&mut self, i: usize) {
        let Mode::Linear { issues, .. } = &self.mode else { return };
        let Some(key) = issues.get(i).map(|x| x.key.clone()) else { return };
        self.act("from-linear".into(), "/api/dockets/from-linear".into(), json!({ "key": key }), move |r| match r {
            Ok(v) => {
                let id = v["docket"].as_str().unwrap_or("?");
                ok(if v["existing"].as_bool() == Some(true) { format!("{key} is already {id}") } else { format!("{id} created from {key}") })
            }
            Err(e) => fail(e),
        });
        self.after_create();
    }

    /// The Linear list's keys: arrows, Return to pick, Escape back.
    pub fn linear_key(&mut self, ev: &KeyEvent, tx: &mut Tx) -> bool {
        let Mode::Linear { issues, sel, .. } = &mut self.mode else { return false };
        let n = issues.len();
        match ev.key {
            Key::Down if n > 0 => *sel = (*sel + 1).min(n - 1),
            Key::Up if n > 0 => *sel = sel.saturating_sub(1),
            Key::Escape => {
                self.back();
                return true;
            }
            _ => return false,
        }
        let key = format!("issue-{}", *sel);
        tx.scroll_into_view(key);
        true
    }

    pub fn run_selected(&mut self, tx: &mut Tx) {
        if let Some(c) = self.shown().get(self.sel).cloned() {
            self.run(&c, tx);
        }
    }

    pub fn run_id(&mut self, id: &str, tx: &mut Tx) {
        let list = self.shown();
        if let Some(i) = list.iter().position(|c| c.id == id) {
            self.sel = i;
            let c = list[i].clone();
            self.run(&c, tx);
        }
    }

    /// The palette's keys: arrows move the selection, Escape closes.
    pub fn palette_key(&mut self, ev: &KeyEvent, tx: &mut Tx) -> bool {
        let n = self.shown().len();
        match ev.key {
            Key::Down if n > 0 => self.sel = (self.sel + 1).min(n - 1),
            Key::Up if n > 0 => self.sel = self.sel.saturating_sub(1),
            Key::Escape => {
                tx.close_window();
                return true;
            }
            _ => return false,
        }
        tx.scroll_into_view(view::row_key(&self.shown()[self.sel].id));
        true
    }

    /// A form's keys: Escape goes back to the palette, ⌘Return submits.
    pub fn form_key(&mut self, ev: &KeyEvent, tx: &mut Tx) -> bool {
        match ev.key {
            Key::Escape => {
                self.back();
                true
            }
            Key::Enter if ev.mods.cmd => {
                self.submit(tx);
                true
            }
            _ => false,
        }
    }

    pub fn back(&mut self) {
        self.mode = Mode::Palette;
        self.query.focused = true;
    }

    /// Create the docket, or start the agent.
    pub fn submit(&mut self, _tx: &mut Tx) {
        match &self.mode {
            Mode::Palette | Mode::Linear { .. } => {}
            Mode::NewDocket { title, note } => {
                let (t, n) = (title.text.trim().to_string(), note.text.trim().to_string());
                if t.is_empty() {
                    return;
                }
                let hub = self.hub.clone();
                self.act("create".into(), "/api/dockets".into(), json!({ "title": t }), move |r| match r {
                    Ok(v) => {
                        let id = v["docket"].as_str().unwrap_or("?").to_string();
                        if !n.is_empty() {
                            let _ = hub.post(&format!("/api/dockets/{id}/note"), json!({ "body": n }));
                        }
                        ok(format!("{id} created"))
                    }
                    Err(e) => fail(e),
                });
                self.after_create();
            }
            Mode::StartAgent { prompt, folder, codex, title } => {
                let cwd = folder.text.trim().to_string();
                if cwd.is_empty() {
                    return;
                }
                let app = if *codex { "codex" } else { "claude-code" };
                let mut body = json!({ "app": app, "cwd": cwd, "prompt": prompt.text });
                if !title.text.trim().is_empty() {
                    body["title"] = json!(title.text.trim());
                }
                let name = if *codex { "Codex" } else { "Claude Code" };
                self.act("start".into(), "/api/agents/start".into(), body, move |r| match r {
                    Ok(v) => ok(format!("{}: {name} started in Rex's wheelhouse session", v["docket"].as_str().unwrap_or("?"))),
                    Err(e) => fail(e),
                });
                self.after_create();
            }
        }
    }

    fn after_create(&mut self) {
        for id in ["create", "start", "from-linear"] {
            self.pending.remove(id);
        }
        self.back();
        self.query.clear();
        self.sel = 0;
    }
}

/// The app's appearance, light, dark or the system's, so the native material (drawn by
/// AppKit, not Rune) follows the Theme commands as the panel's own colours do.
#[cfg(target_os = "macos")]
fn appearance(dark: Option<bool>) {
    use objc2_app_kit::{NSAppearance, NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSApplication};
    use objc2_foundation::MainThreadMarker;
    let Some(mtm) = MainThreadMarker::new() else { return };
    let app = NSApplication::sharedApplication(mtm);
    // SAFETY: AppKit's own appearance names, read on the main thread.
    let named = dark.and_then(|d| unsafe { NSAppearance::appearanceNamed(if d { NSAppearanceNameDarkAqua } else { NSAppearanceNameAqua }) });
    app.setAppearance(named.as_deref());
}

#[cfg(not(target_os = "macos"))]
fn appearance(_: Option<bool>) {}

fn ok(text: impl Into<String>) -> Note {
    Note { text: text.into(), error: false }
}

fn fail(text: impl Into<String>) -> Note {
    Note { text: text.into(), error: true }
}

/// Follow the hub's events, and refresh the state after each burst of them,
/// as the Mac app's Store does: a refresh 150 ms after the first event, once
/// for any that came meanwhile.
fn follow(hub: Hub, me: AppHandle<Panel>) {
    let (tx, rx) = mpsc::channel::<()>();
    let poke = tx.clone();
    me.update(move |s: &mut Panel, _| s.refresh = Some(poke));
    {
        let (hub, me) = (hub.clone(), me.clone());
        std::thread::spawn(move || {
            while rx.recv().is_ok() {
                std::thread::sleep(Duration::from_millis(150));
                while rx.try_recv().is_ok() {}
                let r = hub.get("/api/state");
                me.update(move |s: &mut Panel, _| s.landed(r));
            }
        });
    }
    std::thread::spawn(move || {
        let mut wait = Duration::from_secs(1);
        loop {
            let _ = tx.send(());
            let (m1, m2) = (me.clone(), me.clone());
            let poke = tx.clone();
            let why = hub.follow(
                move || m1.update(|s: &mut Panel, _| s.live = true),
                move || {
                    let _ = poke.send(());
                },
            );
            m2.update(|s: &mut Panel, _| s.live = false);
            if why.contains("isn't running") {
                let _ = tx.send(());
            }
            std::thread::sleep(wait);
            wait = (wait * 2).min(Duration::from_secs(10));
        }
    });
}

/// The waker that lands Linear's answers.
fn setup(s: &mut Panel, waker: Waker) {
    s.linear.set_waker(waker);
}

/// The demo's step: a keyboard user holding Down, then back to the top.
const DEMO_STEP: Duration = Duration::from_millis(280);

/// Before each frame: true only when something changed.
fn poll(s: &mut Panel, _: f64, tx: &mut Tx) -> bool {
    let mut changed = false;
    if let Some(r) = s.linear.take() {
        s.linear_landed(r);
        changed = true;
    }
    if s.demo && s.me.as_ref().is_some_and(|m| m.is_visible()) && matches!(s.mode, Mode::Palette) {
        let n = s.shown().len();
        if n > 0 {
            s.sel = (s.sel + 1) % n;
            tx.scroll_into_view(view::row_key(&s.shown()[s.sel].id));
            changed = true;
        }
        tx.wake_after(DEMO_STEP);
    }
    changed
}


/// The panel's window: a launcher panel, hidden until ⌃⌥⌘K.
pub fn spec(hub: Hub, show_at_launch: bool) -> WindowSpec<Panel> {
    let spec = AppSpec { setup: Some(setup), poll: Some(poll), ..AppSpec::new("Wheelhouse", view::SIZE, || unreachable!(), view::view) };
    let app = App::new(spec).windowless();
    let me = app.handle();
    let (init_me, ready_me) = (me.clone(), me.clone());
    let follow_hub = hub.clone();
    let app = app
        .init(move || Panel::new(hub, init_me))
        .global_shortcut(CHORD, |s, tx| {
            let Some(me) = s.me.clone() else { return };
            if me.is_visible() {
                tx.close_window();
                return;
            }
            s.reset();
            show(&me);
        })
        // As the SwiftUI panel's resignKey: click anywhere else and it goes.
        .on_focus(|_, focused, tx| {
            if !focused {
                tx.close_window();
            }
        })
        .on_ready(move |_| {
            // Once: on_ready runs again if the native window is ever made again.
            static STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
            if rune::tooling::headless_run().is_none() && !STARTED.swap(true, std::sync::atomic::Ordering::SeqCst) {
                follow(follow_hub.clone(), ready_me.clone());
                if show_at_launch {
                    show(&ready_me);
                    ready_me.update(|_, tx| tx.wake_after(DEMO_STEP));
                }
            }
        });
    let options = WindowOptions::launcher_panel().macos(|native| native.shadow(false));
    WindowSpec::from_app("panel", app).configured_options(options)
}

/// In the top third of the screen the pointer is on, as Spotlight sits.
fn show(me: &AppHandle<Panel>) {
    me.place(DisplayTarget::UnderPointer, OverlayPlacement::Edge { edge: ScreenEdge::Top, margin: 140.0, align: Align::Center, inset: 0.0 });
    me.show();
}
