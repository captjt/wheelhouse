//! The sheets that make or change something (Palette.swift `NewDocketSheet`,
//! `LinearPicker`, `LinkIssueSheet`, `StartAgentSheet`): a docket blank, an
//! agent on a new docket, a docket from a Linear issue, a Linear issue
//! linked, an agent started on a docket.

use rune::{Color, El, Key, ScrollSpec, TextEdit, Tx, col, el, icon, row, spacer, text, text_input};
use serde_json::{Value, json};

use super::{Main, NewMode, Sheet, widgets};
use crate::commands::Issue;
use crate::icons;
use crate::model;
use crate::theme::{self, K, font, mono};

#[derive(Default)]
pub struct Form {
    pub title: TextEdit,
    pub note: TextEdit,
    pub folder: TextEdit,
    pub codex: bool,
    pub query: TextEdit,
    pub issues: Vec<Issue>,
    pub sel: usize,
    pub error: Option<String>,
    pub loading: bool,
    pub folders: Vec<String>,
    pub busy: bool,
    pub asked: u64,
}

impl Main {
    /// Open a sheet with a fresh form, focused where you'd type first.
    pub fn open_form(&mut self, sheet: Sheet) {
        self.form = Form::default();
        match &sheet {
            Sheet::NewDocket(NewMode::Blank) => self.form.title.focused = true,
            Sheet::NewDocket(NewMode::Agent) => self.form.note.focused = true,
            Sheet::NewDocket(NewMode::Linear) | Sheet::LinkIssue(_) => self.form.query.focused = true,
            Sheet::StartAgent(id) => {
                self.form.note = TextEdit::new(format!("Read wh brief {id}, then work on its intent.").as_str());
                self.form.note.focused = true;
                if let Some(d) = self.state().and_then(|s| s.docket(id).cloned()) {
                    self.form.codex = d.agent_app().as_deref() == Some("codex");
                }
            }
            _ => {}
        }
        let docket = match &sheet {
            Sheet::StartAgent(id) => Some(id.clone()),
            _ => None,
        };
        let linear = matches!(sheet, Sheet::NewDocket(NewMode::Linear) | Sheet::LinkIssue(_));
        self.sheet = Some(sheet);
        if linear {
            self.search_linear();
        }
        self.load_folders(docket);
    }

    fn load_folders(&mut self, docket: Option<String>) {
        let path = format!("/api/folders{}", docket.map(|d| format!("?docket={d}")).unwrap_or_default());
        let land = |s: &mut Main, r: Result<Value, String>| {
            if let Ok(v) = r {
                s.form.folders = model::strings(&v["folders"]);
                if s.form.folder.text.is_empty()
                    && let Some(f) = model::text(&v["suggested"]).or_else(|| s.form.folders.first().cloned())
                {
                    s.form.folder = TextEdit::new(short(&f).as_str());
                }
            }
        };
        if self.store.headless {
            let r = self.store.hub.get(&path);
            land(self, r);
            return;
        }
        let (store, me) = (self.store.clone(), self.me.clone());
        std::thread::spawn(move || {
            let r = store.hub.get(&path);
            if let Some(me) = me {
                me.update(move |s: &mut Main, _| land(s, r));
            }
        });
    }

    /// Ask Linear (through the hub) for what the search says; the newest answer wins.
    pub fn search_linear(&mut self) {
        self.form.asked += 1;
        self.form.loading = true;
        let (n, q) = (self.form.asked, self.form.query.text.clone());
        let land = move |s: &mut Main, r: Result<Value, String>| {
            if s.form.asked != n {
                return;
            }
            s.form.loading = false;
            s.form.sel = 0;
            match r {
                Ok(v) if v["error"].is_string() => {
                    s.form.error = model::text(&v["error"]);
                    s.form.issues.clear();
                }
                Ok(v) => {
                    s.form.error = None;
                    s.form.issues = model::arr(&v["issues"]).iter().map(Issue::parse).collect();
                }
                Err(e) => {
                    s.form.error = Some(e);
                    s.form.issues.clear();
                }
            }
        };
        let path = format!("/api/linear/search?q={}", crate::hub::encode(&q));
        if self.store.headless {
            let r = self.store.hub.get(&path);
            land(self, r);
            return;
        }
        let (store, me) = (self.store.clone(), self.me.clone());
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(220));
            let r = store.hub.get(&path);
            if let Some(me) = me {
                me.update(move |s: &mut Main, _| land(s, r));
            }
        });
    }

    fn done_form(&mut self, made: Option<String>) {
        self.sheet = None;
        self.form.busy = false;
        if let Some(id) = made {
            self.store.reload();
            self.show(&id);
        }
    }

    /// Run a form's write; on success the sheet closes and its docket shows.
    fn submit_form(&mut self, path: String, body: Value, word: impl FnOnce(&Value) -> (Option<String>, String) + Send + 'static) {
        if self.form.busy {
            return;
        }
        self.form.busy = true;
        let finish = |s: &mut Main, r: Result<Value, String>, word: Box<dyn FnOnce(&Value) -> (Option<String>, String) + Send>| match r {
            Ok(v) => {
                let (id, t) = word(&v);
                s.toast(t, false);
                s.done_form(id);
            }
            Err(e) => {
                s.form.busy = false;
                s.form.error = Some(e);
            }
        };
        let word: Box<dyn FnOnce(&Value) -> (Option<String>, String) + Send> = Box::new(word);
        if self.store.headless {
            let r = self.store.post(&path, body);
            finish(self, r, word);
            return;
        }
        let (store, me) = (self.store.clone(), self.me.clone());
        std::thread::spawn(move || {
            let r = store.post(&path, body);
            if let Some(me) = me {
                me.update(move |s: &mut Main, _| finish(s, r, word));
            }
        });
    }

    pub fn create_blank(&mut self) {
        let (title, note) = (self.form.title.text.trim().to_string(), self.form.note.text.trim().to_string());
        if title.is_empty() {
            return;
        }
        let store = self.store.clone();
        self.submit_form("/api/dockets".into(), json!({ "title": title }), move |v| {
            let id = model::text(&v["docket"]);
            if let (Some(id), false) = (&id, note.is_empty()) {
                let _ = store.post(&format!("/api/dockets/{id}/note"), json!({ "body": note }));
            }
            (id.clone(), format!("{} created", id.unwrap_or_default()))
        });
    }

    pub fn start_agent(&mut self, docket: Option<String>) {
        let cwd = self.form.folder.text.trim().to_string();
        if cwd.is_empty() {
            return;
        }
        let app = if self.form.codex { "codex" } else { "claude-code" };
        let mut body = json!({ "app": app, "cwd": cwd, "prompt": self.form.note.text.trim() });
        if !self.form.title.text.trim().is_empty() {
            body["title"] = json!(self.form.title.text.trim());
        }
        if let Some(d) = &docket {
            body["docket"] = json!(d);
        }
        let name = model::app_name(Some(app));
        self.submit_form("/api/agents/start".into(), body, move |v| {
            let id = model::text(&v["docket"]);
            let t = match &docket {
                Some(d) => format!("{name} is on {d}, in wheelhouse/{d}"),
                None => format!("{}: {name} started in Rex's wheelhouse session", id.clone().unwrap_or_default()),
            };
            (id, t)
        });
    }

    pub fn pick_issue(&mut self, i: usize) {
        let Some(key) = self.form.issues.get(i).map(|x| x.key.clone()) else { return };
        match self.sheet.clone() {
            Some(Sheet::LinkIssue(id)) => {
                self.submit_form(format!("/api/dockets/{id}/linear"), json!({ "key": key }), move |_| (Some(id.clone()), format!("{id} linked to {key}")))
            }
            _ => self.submit_form("/api/dockets/from-linear".into(), json!({ "key": key }), move |v| {
                let id = model::text(&v["docket"]);
                let t = if model::flag(&v["existing"]) {
                    format!("{key} is already {}", id.clone().unwrap_or_default())
                } else {
                    format!("{} created from {key}", id.clone().unwrap_or_default())
                };
                (id, t)
            }),
        }
    }
}

fn short(p: &str) -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    if !home.is_empty() && p.starts_with(&home) { format!("~{}", &p[home.len()..]) } else { p.to_string() }
}

fn input(edit: &TextEdit, key: &str, placeholder: &str, single: bool, min_h: f32, k: &K, access: fn(&mut Main) -> &mut TextEdit) -> El {
    let mut t = text_input(edit, font(13.0, 400.0).lh(18.0))
        .key(key.to_string())
        .w_full()
        .color(k.text)
        .placeholder(placeholder.to_string(), k.muted)
        .label(placeholder.to_string())
        .on_edit(access);
    if single {
        t = t.single_line();
    } else {
        t = t.min_h(min_h);
    }
    row().w_full().px(10.0).py(7.0).radius(7.0).fill(k.recessed).border(1.0, k.line2).child(t)
}

fn card(title: El, body: El, k: &K) -> El {
    col()
        .key("sheet")
        .w(620.0)
        .radius(12.0)
        .continuous_corners()
        .fill(if k.dark { theme::hex("#1f1f21") } else { Color::WHITE })
        .border(1.0, k.line2)
        .box_shadow(rune::Shadow::core_animation(0.0, 12.0, 28.0, Color::BLACK.alpha(0.3)))
        .role("dialog")
        .child(row().w_full().px(16.0).py(14.0).gap(10.0).center_y().child(title))
        .child(theme::hairline(k))
        .child(body)
}

fn footer(hint: &str, cancel: El, go: El, k: &K) -> El {
    row().w_full().gap(8.0).center_y().child(text(hint.to_string()).font(font(11.5, 400.0).lh(15.0)).color(k.muted).wrap().shrink()).child(spacer()).child(cancel).child(go)
}

fn agent_picker(s: &Main, k: &K) -> El {
    widgets::segmented("agent-app", &[("Claude Code", !s.form.codex), ("Codex", s.form.codex)], |s: &mut Main, i| s.form.codex = i == 1, k)
}

fn folder_row(s: &Main, k: &K) -> El {
    let mut chips = row().gap(6.0);
    for f in s.form.folders.iter().take(4) {
        let f2 = short(f);
        chips = chips.child(
            row()
                .key(format!("folder-{}", f.rsplit('/').next().unwrap_or(f)))
                .h(20.0)
                .px(8.0)
                .radius(10.0)
                .center()
                .border(1.0, k.line2)
                .hover(k.hover)
                .role("button")
                .label(format!("Use {f2}"))
                .on_press(move |s: &mut Main| s.form.folder = TextEdit::new(f2.as_str()))
                .child(text(f.rsplit('/').next().unwrap_or(f).to_string()).font(font(11.5, 400.0).lh(14.0)).color(k.muted)),
        );
    }
    col().w_full().gap(6.0).child(input(&s.form.folder, "folder", "~/git/project", true, 0.0, k, |s| &mut s.form.folder)).child(chips)
}

fn grid_row(label: &str, field: El, k: &K) -> El {
    row().w_full().gap(12.0).child(text(label.to_string()).font(font(13.0, 400.0).lh(30.0)).color(k.muted).w(52.0)).child(field.grow(1.0).min_w(0.0))
}

fn error_line(s: &Main, k: &K) -> El {
    match &s.form.error {
        Some(e) => text(e.clone()).font(font(12.0, 400.0).lh(16.0)).color(k.error).wrap(),
        None => el(),
    }
}

pub fn new_docket(s: &Main, mode: NewMode, k: &K) -> El {
    let title = row().w_full().gap(10.0).center_y().child(text("New docket").font(font(14.0, 600.0).lh(18.0)).color(k.text)).child(spacer()).child(widgets::segmented(
        "new-mode",
        &[("Blank", mode == NewMode::Blank), ("From Linear", mode == NewMode::Linear), ("Agent", mode == NewMode::Agent)],
        |s: &mut Main, i| s.open_form(Sheet::NewDocket([NewMode::Blank, NewMode::Linear, NewMode::Agent][i])),
        k,
    ));
    let cancel = widgets::button("cancel", "Cancel", false, true, k, |s: &mut Main, _| s.sheet = None);
    let body = match mode {
        NewMode::Blank => {
            let ready = !s.form.title.text.trim().is_empty() && !s.form.busy;
            let t = text_input(&s.form.title, font(18.0, 600.0).lh(22.0))
                .key("new-title")
                .single_line()
                .w_full()
                .color(k.text)
                .placeholder("Docket title", k.muted)
                .label("Docket title")
                .on_edit(|s: &mut Main| &mut s.form.title)
                .on_submit(|s: &mut Main, _| s.create_blank());
            col()
                .w_full()
                .padding(16.0)
                .gap(12.0)
                .child(t)
                .child(input(&s.form.note, "new-note", "Add a note… (Markdown, optional)", false, 90.0, k, |s| &mut s.form.note))
                .child(error_line(s, k))
                .child(footer(
                    "Signed as you, on the record.",
                    cancel,
                    widgets::button("create", if s.form.busy { "Creating…" } else { "Create docket" }, true, ready, k, |s: &mut Main, _| s.create_blank()),
                    k,
                ))
        }
        NewMode::Agent => {
            let ready = !s.form.folder.text.trim().is_empty() && !s.form.busy;
            col()
                .w_full()
                .padding(16.0)
                .gap(12.0)
                .child(input(&s.form.note, "agent-prompt", "What should the agent do? (its first prompt; the docket is named from it)", false, 110.0, k, |s| &mut s.form.note))
                .child(grid_row("Folder", folder_row(s, k), k))
                .child(grid_row("Agent", row().child(agent_picker(s, k)), k))
                .child(grid_row("Title", input(&s.form.title, "agent-title", "Optional: named from the prompt otherwise", true, 0.0, k, |s| &mut s.form.title), k))
                .child(error_line(s, k))
                .child(footer(
                    "A new tab in Rex's wheelhouse session, named after the docket.",
                    cancel,
                    widgets::button("start", if s.form.busy { "Starting…" } else { "Start agent" }, true, ready, k, |s: &mut Main, _| s.start_agent(None)),
                    k,
                ))
        }
        NewMode::Linear => linear_picker(s, k),
    };
    card(title, body, k)
}

pub fn start_agent(s: &Main, id: &str, k: &K) -> El {
    let title = row()
        .gap(8.0)
        .center_y()
        .child(text(format!("Start an agent on {id}")).font(font(14.0, 600.0).lh(18.0)).color(k.text))
        .child(text(s.state().and_then(|st| st.docket(id).map(|d| d.title())).unwrap_or_default()).font(font(13.0, 400.0).lh(18.0)).color(k.muted).truncate().shrink());
    let ready = !s.form.folder.text.trim().is_empty() && !s.form.busy;
    let a = id.to_string();
    let body = col()
        .w_full()
        .padding(16.0)
        .gap(12.0)
        .child(input(&s.form.note, "start-prompt", "What should the agent do first? Leave it empty to start it idle.", false, 80.0, k, |s| &mut s.form.note))
        .child(grid_row("Folder", folder_row(s, k), k))
        .child(grid_row("Agent", row().child(agent_picker(s, k)), k))
        .child(error_line(s, k))
        .child(footer(
            &format!("A new tab in Rex's wheelhouse session, named {id}. The agent works on this docket and reads its intent."),
            widgets::button("cancel", "Cancel", false, true, k, |s: &mut Main, _| s.sheet = None),
            widgets::button("start", if s.form.busy { "Starting…" } else { "Start Agent" }, true, ready, k, move |s: &mut Main, _| s.start_agent(Some(a.clone()))),
            k,
        ));
    card(title, body, k)
}

pub fn link_issue(s: &Main, id: &str, k: &K) -> El {
    let title = row()
        .w_full()
        .center_y()
        .child(text(format!("Link a Linear issue to {id}")).font(font(14.0, 600.0).lh(18.0)).color(k.text))
        .child(spacer())
        .child(widgets::button("cancel", "Cancel", false, true, k, |s: &mut Main, _| s.sheet = None));
    card(title, linear_picker(s, k), k)
}

/// Search Linear: your open assigned issues, or by words, or a key.
fn linear_picker(s: &Main, k: &K) -> El {
    let field = text_input(&s.form.query, font(15.0, 400.0).lh(19.0))
        .key("linear-query")
        .single_line()
        .w_full()
        .color(k.text)
        .placeholder("Search Linear, or paste a key (ENG-142)…", k.muted)
        .label("Search Linear")
        .active_descendant(if s.form.issues.is_empty() { String::new() } else { format!("issue-{}", s.form.sel) })
        .on_edit(|s: &mut Main| &mut s.form.query)
        .on_input_change(|s: &mut Main, _| s.search_linear())
        .on_input_key(|s: &mut Main, ev, _: &mut Tx| {
            let n = s.form.issues.len();
            match ev.key {
                Key::Down if n > 0 => s.form.sel = (s.form.sel + 1).min(n - 1),
                Key::Up if n > 0 => s.form.sel = s.form.sel.saturating_sub(1),
                Key::Escape => s.sheet = None,
                _ => return false,
            }
            true
        })
        .on_submit(|s: &mut Main, _| {
            let i = s.form.sel;
            s.pick_issue(i);
        });
    let mut rows = col().key("issues").w_full().h(320.0).px(6.0).py(6.0).role("listbox").label("Linear issues").scroll(ScrollSpec { stick_bottom: false });
    if let Some(e) = &s.form.error {
        rows = rows.child(text(e.clone()).font(font(13.0, 400.0).lh(17.0)).color(k.error).padding(10.0).wrap());
    } else if !s.form.loading && s.form.issues.is_empty() {
        rows = rows.child(
            text(if s.form.query.text.is_empty() { "No open issues assigned to you." } else { "No issues match." }).font(font(13.0, 400.0).lh(17.0)).color(k.muted).padding(10.0),
        );
    }
    for (i, issue) in s.form.issues.iter().enumerate() {
        let on = i == s.form.sel;
        let mut r = row()
            .key(format!("issue-{i}"))
            .w_full()
            .px(10.0)
            .py(8.0)
            .gap(10.0)
            .radius(7.0)
            .center_y()
            .fill(if on { k.accent.alpha(0.14) } else { Color::TRANSPARENT })
            .hover(k.hover)
            .role("option")
            .label(format!("{}: {}", issue.key, issue.title))
            .focus_selected(on)
            .on_press(move |s: &mut Main| s.pick_issue(i))
            .child(theme::status_icon(issue.status, 14.0, k))
            .child(text(issue.key.clone()).font(mono(12.0).lh(16.0)).color(k.muted).w(74.0))
            .child(text(issue.title.clone()).font(font(13.0, 400.0).lh(16.0)).color(k.text).truncate().shrink())
            .child(spacer());
        if let Some(st) = &issue.state {
            r = r.child(text(st.clone()).font(font(11.5, 400.0).lh(15.0)).color(k.muted));
        }
        rows = rows.child(r);
    }
    col().w_full().child(row().w_full().px(16.0).py(12.0).gap(10.0).center_y().child(icon(&icons::LINEAR, 15.0).color(k.muted)).child(field)).child(theme::hairline(k)).child(rows)
}
