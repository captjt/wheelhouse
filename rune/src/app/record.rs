//! A docket's record, readable (RecordSheet.swift): what the check means,
//! who signed, and every entry with its signature, newest first. Find and a
//! mark narrow it; an entry opens to its facts, its data and, on asking, the
//! envelope exactly as stored and signed.

use std::collections::{HashMap, HashSet};

use rune::{El, FileDialogOptions, ScrollSpec, TextEdit, Tx, col, el, icon, row, spacer, text, text_input};
use rune_kit::{Kit, ScrollBar};
use serde_json::Value;

use super::parts::{self, Req, Rich};
use super::{Main, Sheet, glyphs, widgets};
use crate::icons;
use crate::model::{self, Entry, arr, int, str_or};
use crate::theme::{self, K, font, mono};

/// The open record, and how it's narrowed.
pub struct Record {
    pub id: Option<String>,
    pub rec: Option<Value>,
    pub entries: Vec<Entry>,
    pub error: Option<String>,
    /// `all`, `witnessed`, `checkable`, `said` or `problems`.
    pub mark: &'static str,
    pub find: TextEdit,
    pub explain: bool,
    pub open: HashSet<i64>,
    pub envelopes: HashMap<i64, Value>,
}

impl Default for Record {
    fn default() -> Record {
        Record { id: None, rec: None, entries: vec![], error: None, mark: "all", find: TextEdit::new(""), explain: false, open: HashSet::new(), envelopes: HashMap::new() }
    }
}

/// Before each frame: a record sheet that just opened loads its record.
pub fn poll(s: &mut Main) -> bool {
    let Some(Sheet::Record(id)) = s.sheet.clone() else {
        // Closed: it loads afresh when it opens again, as a sheet does.
        s.record.id = None;
        return false;
    };
    if s.record.id.as_deref() == Some(id.as_str()) {
        return false;
    }
    s.record = Record { id: Some(id.clone()), ..Record::default() };
    parts::call(s, Req::Get(format!("/api/dockets/{id}/record")), move |s, r| {
        if s.record.id.as_deref() != Some(id.as_str()) {
            return;
        }
        match r {
            Ok(v) => {
                s.record.entries = arr(&v["entries"]).iter().map(|e| Entry { j: e.clone() }).collect();
                s.record.rec = Some(v);
            }
            Err(e) => s.record.error = Some(e),
        }
    });
    true
}

/// Whose key: yours, Wheelhouse's, or an agent's app's.
fn key_name(k: Option<&str>) -> String {
    match k {
        Some("owner") => "your key".into(),
        Some("hub") => "Wheelhouse's key".into(),
        Some(k) if k.starts_with("agent-") => format!("{}'s key", model::app_name(k[6..].split('@').next())),
        Some(k) => k.to_string(),
        None => "an unknown key".into(),
    }
}

fn pretty(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_default()
}

/// What Find searches in an entry.
fn haystack(e: &Entry) -> String {
    format!("{} {} {} #{}", e.kind(), e.body().unwrap_or_default(), pretty(e.data()), e.seq()).to_lowercase()
}

/// An entry in a line: its body, or a few of its data's plain fields.
fn summary(e: &Entry) -> String {
    if let Some(b) = e.body().filter(|b| !b.is_empty()) {
        return b.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(140).collect();
    }
    let Some(o) = e.data().as_object() else { return String::new() };
    let mut keys: Vec<&String> = o.keys().collect();
    keys.sort();
    keys.into_iter()
        .filter(|k| !o[*k].is_object() && !o[*k].is_array())
        .take(3)
        .map(|k| {
            let v = &o[k];
            let s = match v {
                Value::String(s) => s.clone(),
                Value::Bool(b) => b.to_string(),
                Value::Null => "null".into(),
                v => v.to_string(),
            };
            format!("{k}: {s}")
        })
        .collect::<Vec<_>>()
        .join(" · ")
}

fn problems(e: &Entry) -> Vec<String> {
    model::strings(&e.j["problems"])
}

pub fn sheet(s: &Main, id: &str, k: &K, kit: &Kit) -> El {
    let r = &s.record;
    let rec = r.rec.as_ref().filter(|_| r.id.as_deref() == Some(id));
    let v = rec.map(|r| &r["verify"]);
    let ok = v.is_some_and(|v| model::flag(&v["ok"]));
    let short = rec.map(|r| str_or(&r["short_head"], "").to_string()).unwrap_or_default();

    let mut title = row().gap(10.0).center_y().child(text(format!("{id}'s record")).font(font(15.0, 600.0).lh(19.0)).color(k.text));
    if rec.is_some() {
        title = title.child(text(format!("{} entries · head {short}", r.entries.len())).font(font(12.5, 400.0).lh(16.0)).color(k.muted));
    }
    let a = id.to_string();
    let head = parts::sheet_head(
        title,
        row()
            .gap(8.0)
            .child(
                row()
                    .key("record-export")
                    .h(22.0)
                    .px(10.0)
                    .gap(5.0)
                    .radius(6.0)
                    .center()
                    .fill(if k.dark { rune::Color::WHITE.alpha(0.1) } else { rune::Color::WHITE })
                    .border(0.5, k.line2)
                    .hover(k.hover)
                    .role("button")
                    .label("Export…")
                    .on_press_tx(move |s: &mut Main, tx: &mut Tx| export(s, &a, tx))
                    .child(icon(&glyphs::EXPORT, 13.0).color(k.text))
                    .child(text("Export…").font(font(12.0, 500.0).lh(15.0)).color(k.text)),
            )
            .child(widgets::button("record-done", "Done", false, true, k, |s: &mut Main, _| s.sheet = None)),
    );

    let mut body = col().key("record-body").w_full().padding(18.0).gap(14.0);
    if let Some(e) = &r.error {
        body = body.child(text(e.clone()).font(font(13.0, 400.0).lh(17.0)).color(k.error));
    }
    if let (Some(rec), Some(v)) = (rec, v) {
        // The check, in words.
        let mut words = col().gap(4.0).grow(1.0).min_w(0.0).child(
            text(if ok { "Verified: the record is intact." } else { "The record doesn't check." }).font(font(13.5, 600.0).lh(17.0)).color(k.text),
        );
        if ok {
            words = words.child(
                text(format!(
                    "All {} entries are as they were written: none changed, removed, added in between or reordered, each signed by the key it names, and every file they keep matches its hash.",
                    int(&v["entries"]).unwrap_or(0)
                ))
                .font(font(12.5, 400.0).lh(17.0))
                .color(k.text2)
                .wrap(),
            );
        } else {
            for p in model::strings(&v["problems"]).into_iter().take(6) {
                words = words.child(text(format!("• {p}")).font(font(12.5, 400.0).lh(17.0)).color(k.text).wrap());
            }
        }
        body = body.child(
            row()
                .w_full()
                .padding(14.0)
                .gap(10.0)
                .radius(10.0)
                .fill(if ok { k.success_tint } else { k.danger_tint })
                .child(icon(if ok { &glyphs::SHIELD_OK } else { &glyphs::SHIELD_BAD }, 20.0).color(if ok { k.working } else { k.error }))
                .child(words),
        );

        // What this means, folded.
        let mut means = col().w_full().gap(8.0).child(
            row()
                .key("record-explain")
                .gap(5.0)
                .center_y()
                .role("button")
                .label("What this means")
                .expanded(r.explain)
                .on_press(|s: &mut Main| s.record.explain = !s.record.explain)
                .child(icon(if r.explain { &icons::CHEVRON } else { &icons::CHEVRON_RIGHT }, 11.0).color(k.muted))
                .child(text("What this means").font(font(12.5, 400.0).lh(16.0)).color(k.text)),
        );
        if r.explain {
            let sh = if short.is_empty() { "…".to_string() } else { short.clone() };
            let bullet = |head: &str, rest: String| Rich::new().push(head, |s| s.weight(600.0).color(k.text2)).push(&format!(" {rest}"), |s| s.color(k.text2)).el().font(font(12.5, 400.0).lh(17.0)).wrap();
            means = means.child(
                col()
                    .w_full()
                    .gap(8.0)
                    .pl(16.0)
                    .child(bullet("Each entry is signed", "by whoever wrote it, with their own key on this Mac: you, Wheelhouse, or an agent (each app has its own). Nobody can sign as someone else through Wheelhouse.".into()))
                    .child(bullet(
                        "Each entry is chained",
                        format!("to the one before by hash. Its id is the hash of its exact bytes. The head ({sh}) names this exact record: a PR footer that says {id} @ {sh} points here."),
                    ))
                    .child(bullet("Verified", "means nothing was changed, removed, slipped in or reordered since it was written, and every kept file is byte for byte what the record pinned.".into()))
                    .child(bullet(
                        "It doesn't mean an entry is true.",
                        format!("Each one's mark says how it's known. Witnessed: {} Checkable: {} Said: {}", parts::mark_words("witnessed"), parts::mark_words("checkable"), parts::mark_words("said")),
                    ))
                    .child(bullet("The export", format!("({id}.jsonl) checks without the hub: wh verify --file {id}.jsonl."))),
            );
        }
        body = body.child(means);

        // Who signed it.
        let mut keys = row().w_full().gap(8.0).center_y().flex_wrap().child(text("Signed by").font(font(12.0, 400.0).lh(16.0)).color(k.muted));
        for key in arr(&rec["keys"]) {
            let keyid = str_or(&key["keyid"], "");
            let short_id: String = keyid.replace("ed25519:", "").chars().take(8).collect();
            keys = keys.child(
                Rich::new()
                    .push(&key_name(key["name"].as_str()), |s| s.color(k.text))
                    .push(&format!(" · {}", int(&key["entries"]).unwrap_or(0)), |s| s.color(k.muted))
                    .push(&format!(" {short_id}"), |s| s.color(k.faint).mono().size(11.0))
                    .el()
                    .font(font(12.0, 400.0).lh(16.0))
                    .px(8.0)
                    .py(3.0)
                    .radius(999.0)
                    .fill(k.recessed)
                    .help(keyid.to_string()),
            );
        }
        body = body.child(keys);

        // Narrow it: by mark, and by what it says.
        let mut segs = vec![("All", r.mark == "all"), ("Witnessed", r.mark == "witnessed"), ("Checkable", r.mark == "checkable"), ("Said", r.mark == "said")];
        if !ok {
            segs.push(("Problems", r.mark == "problems"));
        }
        let find = text_input(&r.find, font(12.5, 400.0).lh(16.0))
            .key("record-find")
            .single_line()
            .w_full()
            .color(k.text)
            .placeholder("Find in the record…", k.muted)
            .label("Find in the record")
            .on_edit(|s: &mut Main| &mut s.record.find);
        body = body.child(
            row()
                .w_full()
                .center_y()
                .child(widgets::segmented("record-mark", &segs, |s: &mut Main, i| s.record.mark = ["all", "witnessed", "checkable", "said", "problems"][i.min(4)], k))
                .child(spacer())
                .child(row().w(240.0).px(7.0).py(3.0).radius(6.0).fill(k.elev).border(1.0, k.line2).child(find)),
        );

        // Every entry, newest first.
        let q = r.find.text.trim().to_lowercase();
        let owner = s.state().map(|st| st.owner()).unwrap_or_else(|| "you".into());
        let mut rows = col().key("record-rows").w_full().radius(10.0).continuous_corners().clip_corners([10.0; 4]).fill(k.elev).border(1.0, k.line);
        let mut n = 0;
        for e in r.entries.iter().rev() {
            let mark_ok = r.mark == "all" || e.mark() == r.mark || (r.mark == "problems" && !problems(e).is_empty());
            if !mark_ok || (!q.is_empty() && !haystack(e).contains(&q)) {
                continue;
            }
            n += 1;
            rows = rows.child(record_row(s, id, e, &owner, k, kit));
        }
        if n == 0 {
            rows = rows.child(text("Nothing in the record matches.").font(font(12.5, 400.0).lh(16.0)).color(k.muted).padding(14.0));
        }
        body = body.child(rows);
    } else if r.error.is_none() {
        body = body.child(row().w_full().justify_center().py(40.0).child(text("Loading the record…").font(font(12.5, 400.0).lh(16.0)).color(k.muted)));
    }

    let scroll = col().key("record-scroll").w_full().grow(1.0).min_h(0.0).child(body).scroll(ScrollSpec { stick_bottom: false }).scroll_bar(kit);
    parts::sheet_card(&format!("{id}'s record"), 920.0, Some(760.0), k).child(head).child(parts::divider(k)).child(scroll)
}

fn record_row(s: &Main, id: &str, e: &Entry, owner: &str, k: &K, kit: &Kit) -> El {
    let seq = e.seq();
    let open = s.record.open.contains(&seq);
    let probs = problems(e);
    let avatar = match e.actor_type().as_str() {
        "person" => theme::person_avatar(owner, 16.0, k),
        "agent" => theme::app_avatar(Some(&e.actor_app()), 16.0, k),
        _ => theme::hub_avatar(16.0, k),
    };
    let line = row()
        .key(format!("record-row-{seq}"))
        .w_full()
        .px(12.0)
        .py(8.0)
        .gap(10.0)
        .center_y()
        .fill(if probs.is_empty() { rune::Color::TRANSPARENT } else { k.danger_tint })
        .hover(k.hover)
        .role("button")
        .label(format!("#{seq} {}", e.kind()))
        .expanded(open)
        .on_press(move |s: &mut Main| {
            if !s.record.open.remove(&seq) {
                s.record.open.insert(seq);
            }
        })
        .child(text(format!("#{seq}")).font(mono(11.5).lh(15.0)).color(k.faint).w(40.0))
        .child(avatar)
        .child(text(e.kind()).font(font(12.5, 500.0).lh(16.0)).color(k.text).w(74.0).truncate())
        .child(text(summary(e)).font(font(12.5, 400.0).lh(16.0)).color(k.text2).truncate().shrink().grow(1.0).min_w(0.0))
        .child(parts::mark(&e.mark(), k))
        .child(row().w(70.0).justify_end().child(text(model::ago_long(Some(&e.at()))).font(font(11.5, 400.0).lh(15.0)).color(k.faint).help(model::stamp(Some(&e.at())))))
        .child(icon(if open { &icons::CHEVRON } else { &icons::CHEVRON_RIGHT }, 10.0).color(k.faint));
    let mut c = col().w_full().child(line);
    if open {
        c = c.child(details(s, id, e, k, kit));
    }
    c.child(el().h(1.0).w_full().fill(k.line.alpha(0.6)).a11y_hidden())
}

/// An entry, opened: its problems, facts, body, data and envelope.
fn details(s: &Main, id: &str, e: &Entry, k: &K, kit: &Kit) -> El {
    let seq = e.seq();
    let mut c = col().w_full().px(14.0).pt(4.0).pb(14.0).gap(10.0);
    for p in problems(e) {
        c = c.child(text(format!("• {p}")).font(font(12.0, 400.0).lh(16.0)).color(k.error).wrap());
    }
    let who = match e.actor_type().as_str() {
        "person" => "You".to_string(),
        "agent" => model::app_name(Some(&e.actor_app())),
        _ => "Wheelhouse".to_string(),
    };
    let fact = |label: &str, v: String, is_mono: bool| {
        row()
            .w_full()
            .gap(14.0)
            .child(text(label.to_string()).font(font(12.0, 400.0).lh(16.0)).color(k.muted).w(104.0))
            .child(text(v).font(if is_mono { mono(11.5).lh(16.0) } else { font(12.0, 400.0).lh(16.0) }).color(k.text).selectable(true).wrap_anywhere().shrink().grow(1.0).min_w(0.0))
    };
    let mut facts = col()
        .w_full()
        .gap(5.0)
        .child(fact("Written by", format!("{who} ({}: {})", e.actor_type(), str_or(&e.j["actor"]["id"], "")), false))
        .child(fact("Signed with", format!("{} {}", key_name(e.j["key"].as_str()), str_or(&e.j["keyid"], "")), false))
        .child(fact("When", model::stamp(Some(&e.at())), false))
        .child(fact("How it's known", format!("{}: {}", e.mark(), parts::mark_words(&e.mark())), false))
        .child(fact("Id", str_or(&e.j["id"], "").to_string(), true))
        .child(fact("Chain", str_or(&e.j["chain"], "").to_string(), true));
    let blobs = model::strings(&e.j["blobs"]);
    if !blobs.is_empty() {
        facts = facts.child(fact("Files", blobs.join("\n"), true));
    }
    c = c.child(facts);
    if let Some(b) = e.body().filter(|b| !b.is_empty()) {
        c = c.child(col().w_full().padding(10.0).radius(8.0).fill(k.recessed).child(parts::md(&b, 12.5, k, kit)));
    }
    if e.data().as_object().is_some_and(|o| !o.is_empty()) {
        c = c.child(code(pretty(e.data()), k));
    }
    match s.record.envelopes.get(&seq) {
        Some(env) => {
            let raw = str_or(&env["envelope"], "");
            let shown = serde_json::from_str::<Value>(raw).map(|v| pretty(&v)).unwrap_or_else(|_| raw.to_string());
            c = c
                .child(text("The envelope, exactly as stored and signed (DSSE, Ed25519):").font(font(11.5, 400.0).lh(15.0)).color(k.muted))
                .child(code(shown, k))
                .child(text("Its payload, the bytes the signature covers:").font(font(11.5, 400.0).lh(15.0)).color(k.muted))
                .child(code(str_or(&env["payload"], "").to_string(), k));
        }
        None => {
            let a = id.to_string();
            c = c.child(parts::link(format!("record-envelope-{seq}"), "Show the signed envelope", 12.0, k, move |s, _| envelope(s, &a, seq)));
        }
    }
    c
}

/// Code, wrapped: never a sideways scroll.
fn code(s: String, k: &K) -> El {
    text(s).font(mono(11.5).lh(15.0)).color(k.text2).selectable(true).wrap_anywhere().w_full().padding(10.0).radius(8.0).fill(k.recessed)
}

pub fn envelope(s: &mut Main, id: &str, seq: i64) {
    let id = id.to_string();
    parts::call(s, Req::Get(format!("/api/dockets/{id}/record/{seq}")), move |s, r| match r {
        Ok(v) => {
            s.record.envelopes.insert(seq, v);
        }
        Err(e) => s.toast(e, true),
    });
}

/// Save the record as JSON lines: it checks without the hub.
fn export(s: &mut Main, id: &str, tx: &mut Tx) {
    let options = FileDialogOptions { title: format!("Export {id}'s record"), file_name: Some(format!("{id}.jsonl")), ..Default::default() }.filter("JSON Lines", &["jsonl"]);
    let id = id.to_string();
    let _ = s;
    tx.save_file(options, move |s: &mut Main, picked, _| {
        let path = match picked {
            Ok(Some(p)) => p,
            Ok(None) => return,
            // No Save panel in this build: the Downloads folder, as a browser saves.
            Err(_) => std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default()).join("Downloads").join(format!("{id}.jsonl")),
        };
        parts::call_bytes(s, format!("/api/dockets/{id}/export"), move |s, r| match r.and_then(|b| std::fs::write(&path, b).map_err(|e| e.to_string())) {
            Ok(()) => {
                let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                let place = path.parent().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                s.toast(format!("Saved {name} to {place}"), false);
            }
            Err(e) => s.toast(e, true),
        });
    });
}
