//! Verify (Verify.swift): what a docket's done rests on. Witnessed checks
//! the hub ran, with their output; the checker's verdicts; a close on your
//! word; and hold. Hidden when there's nothing to show.

use rune::{Color, El, LoopSpec, ScrollSpec, TextEdit, Tx, col, el, icon, row, spacer, text, text_input};
use rune_kit::{Kit, ScrollBar};
use serde_json::{Value, json};

use super::parts::{self, Req, Rich};
use super::{Main, Sheet, glyphs, widgets};
use crate::icons;
use crate::model::{self, Docket, DocketPage, Status, arr, flag, int, str_or};
use crate::theme::{K, font, mono};

/// What Verify keeps between frames.
pub struct Verify {
    /// A docket whose checks are starting: Running… for a second.
    pub starting: Option<(String, f64)>,
    /// A check's output, for its sheet: (docket, check), the text, and
    /// whether its bytes match the record's hash.
    pub output_for: Option<(String, String)>,
    pub output: Option<String>,
    pub output_error: Option<String>,
    pub matches: bool,
    /// Close on your word: why, and the hub's refusal.
    pub reason: TextEdit,
    pub closing: bool,
    pub close_error: Option<String>,
}

impl Default for Verify {
    fn default() -> Verify {
        Verify { starting: None, output_for: None, output: None, output_error: None, matches: false, reason: TextEdit::new(""), closing: false, close_error: None }
    }
}

/// One check's latest run (CheckRun).
struct Check<'a>(&'a Value);

impl Check<'_> {
    fn name(&self) -> &str {
        str_or(&self.0["name"], "")
    }
    fn required(&self) -> bool {
        self.0["required"].as_bool().unwrap_or(true)
    }
    fn ok(&self) -> bool {
        flag(&self.0["ok"])
    }
    fn timed_out(&self) -> bool {
        self.0["exit"].is_null()
    }
    fn head(&self) -> &str {
        str_or(&self.0["head"], "")
    }
    fn short_head(&self) -> String {
        self.head().chars().take(7).collect()
    }
    fn dirty(&self) -> bool {
        flag(&self.0["dirty"])
    }
    fn output(&self) -> Option<String> {
        model::text(&self.0["output"]).map(|o| o.replace("sha256:", ""))
    }
    fn outcome(&self) -> String {
        if self.ok() {
            return if self.dirty() { "passed, with uncommitted changes".into() } else { "passed".into() };
        }
        if self.timed_out() {
            return "timed out".into();
        }
        format!("failed{}", int(&self.0["exit"]).map(|e| format!(", exit {e}")).unwrap_or_default())
    }
}

/// Why Run Checks can't run now, or None when it can.
fn cant_run(ev: &Value) -> Option<String> {
    if flag(&ev["running"]) {
        return Some("They're running".into());
    }
    if arr(&ev["configured"]).is_empty() {
        return Some(match model::text(&ev["repo"]) {
            Some(r) => format!("{r} has no checks: add them in Settings › Repos"),
            None => "Its folder isn't in one of Build's repos, so it has no checks".into(),
        });
    }
    None
}

fn is_empty(ev: &Value) -> bool {
    !flag(&ev["needs"]) && arr(&ev["evidence"]).is_empty() && arr(&ev["checks"]).is_empty() && arr(&ev["verdicts"]).is_empty() && model::text(&ev["closed_on"]).is_none()
}

/// The docket's Verify section, or None when there's nothing to show.
pub fn section(s: &Main, d: &Docket, p: &DocketPage, k: &K) -> Option<El> {
    let ev = p.evidence();
    if is_empty(ev) {
        return None;
    }
    let id = d.id();
    let lines = arr(&ev["evidence"]);
    let checks = arr(&ev["checks"]);
    let configured = arr(&ev["configured"]);
    let needs = flag(&ev["needs"]);
    let repo = model::text(&ev["repo"]);
    let mut sub = vec![];
    if let Some(r) = &repo {
        sub.push(r.clone());
    }
    if needs {
        sub.push(if lines.is_empty() { "done needs evidence".into() } else { format!("done rests on {}", model::plural(lines.len(), "thing")) });
    }
    let starting = s.verify.starting.as_ref().is_some_and(|(d, at)| *d == id && s.clock - at < 1.0) || s.busy.contains(&format!("check:{id}"));
    let mut head = row()
        .w_full()
        .gap(8.0)
        .center_y()
        .child(text("Verify").font(font(13.0, 600.0).lh(16.0)).color(k.text))
        .child(text(sub.join(" · ")).font(font(12.0, 400.0).lh(16.0)).color(k.muted).truncate().shrink())
        .child(spacer());
    if flag(&ev["running"]) || starting {
        head = head.child(
            row()
                .gap(6.0)
                .center_y()
                .child(icon(&icons::DASHED, 11.0).color(k.muted).looping(LoopSpec::rotation(1.0)))
                .child(text("Running…").font(font(12.0, 400.0).lh(15.0)).color(k.muted)),
        );
    } else if !configured.is_empty() || repo.is_some() {
        let why = cant_run(ev);
        let can = why.is_none();
        let names: Vec<&str> = configured.iter().filter_map(|c| c["name"].as_str()).collect();
        let help = why.clone().unwrap_or_else(|| format!("Run {} at the folder's commit, as the hub, and keep the results on the record", names.join(", ")));
        let a = id.clone();
        let mut b = row()
            .key("run-checks")
            .gap(5.0)
            .px(6.0)
            .py(2.0)
            .radius(5.0)
            .center_y()
            .role("button")
            .label("Run Checks")
            .help(help)
            .opacity(if can { 1.0 } else { 0.45 })
            .child(icon(&icons::PLAY, 10.0).color(k.muted))
            .child(text("Run Checks").font(font(12.0, 400.0).lh(15.0)).color(k.muted));
        b = if can { b.hover(k.hover).on_press(move |s: &mut Main| s.run_checks(&a)) } else { b.disabled(true) };
        head = head.child(b);
    }

    // The rows, with a rule between each.
    let mut rows: Vec<El> = vec![];
    for l in lines {
        let how = str_or(&l["how"], "said");
        let witnessed = how == "witnessed";
        let mut r = row()
            .w_full()
            .px(12.0)
            .py(9.0)
            .gap(9.0)
            .child(row().w(16.0).pt(1.0).justify_center().child(icon(if witnessed { &glyphs::SEAL } else { &icons::APPROVE }, 13.0).color(if witnessed {
                k.working
            } else {
                k.accent
            })))
            .child(Rich::new().push("Done rests on ", |s| s.color(k.muted)).md(str_or(&l["what"], ""), k.text).el().font(font(12.5, 400.0).lh(17.0)).wrap().grow(1.0).min_w(0.0))
            .child(parts::mark(how, k));
        if let Some(at) = l["at"].as_str() {
            r = r.child(text(model::ago(Some(at))).font(font(11.5, 400.0).lh(17.0)).color(k.faint).help(model::stamp(Some(at))));
        }
        rows.push(r);
    }
    if needs && lines.is_empty() {
        let mut r = row().w_full().px(12.0).py(9.0).gap(9.0).child(row().w(16.0).pt(2.0).justify_center().child(icon(&glyphs::TEXT_BUBBLE, 13.0).color(k.faint))).child(
            col()
                .gap(3.0)
                .grow(1.0)
                .min_w(0.0)
                .child(
                    text(if d.status() == Status::Done { "Done rests on what was said." } else { "Nothing yet but what was said." }).font(font(12.5, 400.0).lh(17.0)).color(k.text),
                )
                .child(
                    text("Its intent has acceptance items, so done needs a passing check, a merged PR, or a completed issue. Or your word, with a reason.")
                        .font(font(12.0, 400.0).lh(16.0))
                        .color(k.muted)
                        .wrap(),
                ),
        );
        if d.is_open() {
            let (a, from) = (id.clone(), d.status());
            r = r.child(parts::link("close-on-word", "Close on Your Word…", 12.0, k, move |s, _| close_on_word(s, &a, from)));
        }
        rows.push(r);
    }
    for c in checks {
        rows.push(check_row(&id, &Check(c), k));
    }
    let ran: Vec<&str> = checks.iter().filter_map(|c| c["name"].as_str()).collect();
    let unrun: Vec<&str> = configured.iter().filter_map(|c| c["name"].as_str()).filter(|n| !ran.contains(n)).collect();
    if !unrun.is_empty() {
        rows.push(
            row().w_full().px(12.0).py(9.0).gap(9.0).child(row().w(16.0).pt(2.0).justify_center().child(icon(&icons::DASHED, 13.0).color(k.faint))).child(
                Rich::new()
                    .push(&unrun.join(", "), |s| s.mono().weight(500.0).color(k.text2))
                    .push(if checks.is_empty() { "  not run on this docket yet" } else { "  not run yet" }, |s| s.color(k.muted))
                    .el()
                    .font(font(12.5, 400.0).lh(17.0))
                    .wrap()
                    .grow(1.0)
                    .min_w(0.0),
            ),
        );
    }
    for v in arr(&ev["verdicts"]) {
        rows.push(verdict_row(v, k));
    }
    if let Some(why) = model::text(&ev["closed_on"]).or_else(|| d.closed_on()) {
        rows.push(
            row()
                .w_full()
                .px(12.0)
                .py(9.0)
                .gap(9.0)
                .help("Nothing witnessed or checkable said its acceptance items were met. The record keeps your reason.")
                .child(row().w(16.0).pt(2.0).justify_center().child(icon(&glyphs::QUOTE_BUBBLE, 13.0).color(k.blocked)))
                .child(
                    Rich::new()
                        .push("Closed on your word: ", |s| s.weight(500.0).color(k.text))
                        .push(&why, |s| s.color(k.text2))
                        .el()
                        .font(font(12.5, 400.0).lh(17.0))
                        .wrap()
                        .selectable(true)
                        .grow(1.0)
                        .min_w(0.0),
                )
                .child(parts::mark("said", k)),
        );
    }
    let mut card = col().key("verify-card").w_full().radius(10.0).continuous_corners().clip_corners([10.0; 4]).fill(k.elev).border(1.0, k.line);
    for (i, r) in rows.into_iter().enumerate() {
        if i > 0 {
            card = card.child(el().h(1.0).w_full().fill(k.line.alpha(0.6)).a11y_hidden());
        }
        card = card.child(r);
    }
    let mut c = col().key("verify").w_full().gap(10.0).child(head).child(card);
    if let Some(why) = cant_run(ev).filter(|_| configured.is_empty() && needs) {
        c = c.child(text(why).font(font(11.5, 400.0).lh(15.0)).color(k.faint));
    }
    Some(c)
}

/// One check's latest run: how it went, where, how long, and its output.
fn check_row(docket: &str, c: &Check, k: &K) -> El {
    let tint = if c.ok() {
        k.working
    } else if c.timed_out() {
        k.blocked
    } else if c.required() {
        k.error
    } else {
        k.muted
    };
    let mark = if c.ok() {
        &glyphs::CHECK_FILL
    } else if c.timed_out() {
        &glyphs::CLOCK_BANG
    } else {
        &glyphs::CROSS_FILL
    };
    let mut name = row().w_full().gap(7.0).center_y().child(text(c.name().to_string()).font(parts::mono_w(12.5, 600.0).lh(16.0)).color(k.text));
    if !c.required() {
        name = name.child(crate::theme::chip("optional", k.muted, false).help("An optional check only informs: it doesn't hold a PR or a push"));
    }
    name = name.child(text(c.outcome()).font(font(12.5, 400.0).lh(16.0)).color(if c.ok() { k.text2 } else { tint })).child(spacer()).child(parts::mark("witnessed", k));
    if let Some(at) = c.0["at"].as_str() {
        name = name.child(text(model::ago(Some(at))).font(font(11.5, 400.0).lh(16.0)).color(k.faint).help(model::stamp(Some(at))));
    }
    let mut how = Rich::new().push(str_or(&c.0["command"], ""), |s| s.mono().size(11.5));
    if !c.short_head().is_empty() {
        how = how.push(&format!("  at {}", c.short_head()), |s| s);
    }
    if c.dirty() {
        how = how.push(" with uncommitted changes", |s| s);
    }
    if let Some(ms) = int(&c.0["ms"]) {
        how = how.push(&format!(" · {}", model::duration(Some(ms))), |s| s);
    }
    let why = match c.0["why"].as_str() {
        Some("asked") => "run when asked".to_string(),
        Some(w) => format!("run {w}"),
        None => "run".to_string(),
    };
    let mut r = row()
        .key(format!("check-{}", c.name()))
        .w_full()
        .px(12.0)
        .py(9.0)
        .gap(9.0)
        .hover(k.hover)
        .help(format!("#{} · {why} · {}", int(&c.0["seq"]).unwrap_or(0), c.head()))
        .child(row().w(16.0).pt(1.0).justify_center().child(icon(mark, 13.0).color(tint)))
        .child(col().gap(3.0).grow(1.0).min_w(0.0).child(name).child(how.el().font(font(12.0, 400.0).lh(16.0)).color(k.muted).wrap().selectable(true)));
    if c.output().is_some() {
        let (d, n) = (docket.to_string(), c.name().to_string());
        r = r.child(
            row()
                .key(format!("output-{}", c.name()))
                .px(6.0)
                .py(1.0)
                .radius(5.0)
                .hover(k.hover)
                .role("button")
                .label("Output")
                .help("Its output's last 256 KB, kept by its hash")
                .on_press(move |s: &mut Main| open_output(s, &d, &n))
                .child(text("Output").font(font(11.5, 400.0).lh(15.0)).color(k.muted)),
        );
    }
    r
}

/// A checker's verdict, as it signed it, and whether it's independent.
fn verdict_row(v: &Value, k: &K) -> El {
    let by = str_or(&v["by"], "");
    let who = model::app_name(by.split('@').next());
    let verdict = str_or(&v["verdict"], "");
    let passed = verdict == "pass";
    let findings = int(&v["findings"]).unwrap_or(arr(&v["findings"]).len() as i64);
    let role = model::text(&v["role"]).map(|r| {
        let mut c = r.chars();
        c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
    });
    let mut top = row()
        .w_full()
        .gap(7.0)
        .center_y()
        .child(
            text(format!("{}{}", role.unwrap_or_else(|| "Checker".into()), int(&v["n"]).map(|n| format!(" · check {n}")).unwrap_or_default()))
                .font(font(12.5, 600.0).lh(16.0))
                .color(k.text),
        )
        .child(text(who).font(font(12.5, 400.0).lh(16.0)).color(k.muted))
        .child(crate::theme::chip(
            if passed {
                "passed"
            } else if verdict.is_empty() {
                "no verdict"
            } else {
                "found problems"
            },
            if passed { k.working } else { k.error },
            false,
        ));
    if findings > 0 {
        top = top.child(text(model::plural(findings as usize, "finding")).font(font(12.0, 400.0).lh(16.0)).color(k.muted));
    }
    top = top.child(spacer()).child(parts::mark(str_or(&v["mark"], "said"), k));
    if let Some(at) = v["at"].as_str() {
        top = top.child(text(model::ago(Some(at))).font(font(11.5, 400.0).lh(16.0)).color(k.faint).help(model::stamp(Some(at))));
    }
    let mut body = col().gap(4.0).grow(1.0).min_w(0.0).child(top);
    if let Some(i) = v["independent"].as_bool() {
        body = body.child(
            row().gap(5.0).center_y().child(icon(if i { &glyphs::PEOPLE } else { &glyphs::PERSON_ASK }, 12.0).color(if i { k.muted } else { k.blocked })).child(
                text(if i { "Independent: its own session, apart from the builder's" } else { "Same session as the builder: not an independent check" })
                    .font(font(11.5, 400.0).lh(15.0))
                    .color(if i { k.muted } else { k.blocked }),
            ),
        );
    }
    if let Some(sum) = model::text(&v["summary"]) {
        body = body.child(Rich::new().md(&sum, k.text2).el().font(font(12.5, 400.0).lh(17.0)).wrap().selectable(true));
    }
    let head: String = str_or(&v["head"], "").chars().take(7).collect();
    row()
        .w_full()
        .px(12.0)
        .py(9.0)
        .gap(9.0)
        .help(format!("#{} · said by {by}{}", int(&v["seq"]).unwrap_or(0), if head.is_empty() { String::new() } else { format!(" · at {head}") }))
        .child(row().w(16.0).pt(2.0).justify_center().child(icon(if passed { &glyphs::THUMBSUP } else { &glyphs::BUBBLE_BANG }, 13.0).color(if passed {
            k.working
        } else {
            k.error
        })))
        .child(body)
}

/// Start a docket's checks; `sheet` closes the close-on-word sheet once
/// they're running, or shows the hub's refusal there.
pub fn run(s: &mut Main, id: &str, sheet: bool) {
    s.verify.starting = Some((id.to_string(), s.clock));
    let id = id.to_string();
    parts::call(s, Req::Post(format!("/api/dockets/{id}/check"), json!({})), move |s, r| {
        match r {
            Ok(v) => {
                let names = model::strings(&v["checks"]);
                s.toast(format!("Running {id}'s checks{}", if names.is_empty() { String::new() } else { format!(": {}", names.join(", ")) }), false);
                if sheet {
                    s.sheet = None;
                }
            }
            Err(e) if sheet => s.verify.close_error = Some(e),
            Err(e) => s.toast(e, true),
        }
        s.load_page(&id);
    });
}

// ---- a check's output ----

fn open_output(s: &mut Main, docket: &str, check: &str) {
    s.sheet = Some(Sheet::CheckOutput(docket.to_string(), check.to_string()));
}

/// Before each frame: an output sheet that just opened fetches its bytes.
pub fn poll(s: &mut Main) -> bool {
    let Some(Sheet::CheckOutput(d, c)) = s.sheet.clone() else {
        s.verify.output_for = None;
        return false;
    };
    if s.verify.output_for.as_ref() == Some(&(d.clone(), c.clone())) {
        return false;
    }
    s.verify.output_for = Some((d.clone(), c.clone()));
    s.verify.output = None;
    s.verify.output_error = None;
    s.verify.matches = false;
    let hex = s.pages.get(&d).and_then(|p| arr(&p.evidence()["checks"]).iter().find(|x| x["name"].as_str() == Some(c.as_str())).and_then(|x| Check(x).output()));
    let Some(hex) = hex else {
        s.verify.output_error = Some("It kept no output.".into());
        return true;
    };
    let want = (d, c);
    parts::call_bytes(s, format!("/api/blobs/{hex}"), move |s, r| {
        if s.verify.output_for.as_ref() != Some(&want) {
            return;
        }
        match r {
            Ok(bytes) => {
                s.verify.matches = parts::sha256_hex(&bytes) == hex;
                s.verify.output = Some(String::from_utf8_lossy(&bytes).to_string());
            }
            Err(e) => s.verify.output_error = Some(e),
        }
    });
    true
}

pub fn output_sheet(s: &Main, docket: &str, check: &str, k: &K, kit: &Kit) -> El {
    let page = s.pages.get(docket);
    let raw = page.and_then(|p| arr(&p.evidence()["checks"]).iter().find(|x| x["name"].as_str() == Some(check)).cloned()).unwrap_or(Value::Null);
    let c = Check(&raw);
    let v = &s.verify;
    let text_now = v.output.clone();
    let copy = text_now.clone().unwrap_or_default();
    let head = parts::sheet_head(
        row().gap(8.0).center_y().child(text(check.to_string()).font(parts::mono_w(14.0, 600.0).lh(18.0)).color(k.text)).child(
            text(format!("{}{} · {docket}", c.outcome(), if c.short_head().is_empty() { String::new() } else { format!(" at {}", c.short_head()) }))
                .font(font(12.5, 400.0).lh(16.0))
                .color(k.muted),
        ),
        row()
            .gap(8.0)
            .child(widgets::button("output-copy", "Copy", false, text_now.is_some(), k, move |s: &mut Main, tx: &mut Tx| {
                tx.copy(copy.clone());
                s.toast("Copied the output", false);
            }))
            .child(widgets::button("output-done", "Done", false, true, k, |s: &mut Main, _| s.sheet = None)),
    );
    let body = match (&v.output, &v.output_error) {
        (Some(t), _) => col()
            .key("output-text")
            .w_full()
            .grow(1.0)
            .min_h(0.0)
            .child(
                text(if t.is_empty() { "(no output)".to_string() } else { t.clone() })
                    .font(mono(11.5).lh(15.0))
                    .color(k.text)
                    .selectable(true)
                    .wrap_anywhere()
                    .w_full()
                    .px(12.0)
                    .py(10.0),
            )
            .scroll(ScrollSpec { stick_bottom: false })
            .scroll_bar(kit),
        (None, Some(e)) => col()
            .w_full()
            .grow(1.0)
            .center()
            .gap(8.0)
            .child(icon(&glyphs::WARN, 28.0).color(k.faint))
            .child(text("Can't show the output").font(font(17.0, 600.0).lh(21.0)).color(k.text))
            .child(text(e.clone()).font(font(12.5, 400.0).lh(16.0)).color(k.muted)),
        _ => col().w_full().grow(1.0).center().child(icon(&icons::DASHED, 14.0).color(k.muted).looping(LoopSpec::rotation(1.0))),
    };
    let hex = c.output().unwrap_or_default();
    let mut foot = row().w_full().px(14.0).py(9.0).gap(6.0).center_y();
    if v.matches {
        foot = foot.child(icon(&icons::SHIELD, 13.0).color(k.working));
    }
    foot = foot
        .child(
            text(format!("{} · the end of its output, kept by hash{}", str_or(&raw["command"], ""), if v.matches { ": it matches" } else { "" }))
                .font(font(11.5, 400.0).lh(15.0))
                .color(k.muted)
                .truncate_middle()
                .shrink(),
        )
        .child(spacer())
        .child(text(format!("sha256:{}…", hex.chars().take(12).collect::<String>())).font(mono(11.0).lh(14.0)).color(k.faint).help(hex));
    parts::sheet_card(&format!("{check}'s output"), 760.0, Some(520.0), k).child(head).child(parts::divider(k)).child(body).child(parts::divider(k)).child(foot)
}

// ---- close on your word ----

pub fn close_on_word(s: &mut Main, id: &str, from: Status) {
    s.verify.reason.clear();
    s.verify.reason.focused = true;
    s.verify.close_error = None;
    s.verify.closing = false;
    s.sheet = Some(Sheet::CloseOnWord(id.to_string(), from));
}

/// Done was refused: say why, and offer its checks, or your word.
pub fn close_sheet(s: &Main, id: &str, from: Status, k: &K) -> El {
    let title = s.state().and_then(|st| st.docket(id).map(|d| d.title())).unwrap_or_default();
    let ev = s.pages.get(id).map(|p| p.evidence().clone());
    let can_run = ev.as_ref().is_some_and(|e| cant_run(e).is_none());
    let v = &s.verify;
    let mut c = col()
        .w_full()
        .padding(20.0)
        .gap(14.0)
        .child(
            row()
                .w_full()
                .gap(12.0)
                .child(icon(&glyphs::SEAL, 26.0).color(k.blocked))
                .child(
                    col()
                        .gap(4.0)
                        .grow(1.0)
                        .min_w(0.0)
                        .child(text(format!("{id} needs evidence to be done")).font(font(14.0, 600.0).lh(18.0)).color(k.text))
                        .child(text(title).font(font(12.5, 400.0).lh(16.0)).color(k.muted).truncate()),
                ),
        )
        .child(
            text("Its intent has acceptance items, and nothing witnessed or checkable says they're met: no passing checks, no merged PR, no completed issue. Run its checks, or close it on your word.")
                .font(font(12.5, 400.0).lh(17.0))
                .color(k.text2)
                .wrap(),
        );
    if let Some(ev) = &ev {
        if let Some(why) = cant_run(ev) {
            c = c.child(row().gap(6.0).center_y().child(icon(&glyphs::INFO, 13.0).color(k.muted)).child(text(why).font(font(12.0, 400.0).lh(16.0)).color(k.muted)));
        } else {
            let names: Vec<String> = arr(&ev["configured"])
                .iter()
                .map(|x| {
                    let n = str_or(&x["name"], "");
                    if x["required"].as_bool().unwrap_or(true) { n.to_string() } else { format!("{n} (optional)") }
                })
                .collect();
            c = c.child(
                row().gap(6.0).center_y().child(icon(&glyphs::CHECKLIST, 13.0).color(k.muted)).child(
                    text(format!("{}'s checks: {}", model::text(&ev["repo"]).unwrap_or_else(|| "Its repo".into()), names.join(", ")))
                        .font(font(12.0, 400.0).lh(16.0))
                        .color(k.muted)
                        .wrap(),
                ),
            );
        }
    }
    let a = id.to_string();
    let field = text_input(&v.reason, font(13.0, 400.0).lh(18.0))
        .key("close-reason")
        .w_full()
        .min_h(36.0)
        .color(k.text)
        .placeholder("Why it's done (required)", k.muted)
        .label("Why it's done")
        .on_edit(|s: &mut Main| &mut s.verify.reason)
        .on_submit(move |s: &mut Main, _| close(s, &a, from));
    let mut word = col()
        .w_full()
        .gap(6.0)
        .child(text("Close on your word").font(font(12.5, 600.0).lh(16.0)).color(k.text))
        .child(row().w_full().px(8.0).py(5.0).radius(6.0).fill(k.elev).border(1.0, k.line2).child(field))
        .child(text("The record keeps your reason, and the docket shows it was closed on your word.").font(font(11.5, 400.0).lh(15.0)).color(k.muted).wrap());
    if let Some(e) = &v.close_error {
        word = word.child(row().gap(6.0).child(icon(&glyphs::WARN, 12.0).color(k.error)).child(text(e.clone()).font(font(12.0, 400.0).lh(16.0)).color(k.error).wrap().shrink()));
    }
    c = c.child(word);
    let (b, r) = (id.to_string(), id.to_string());
    let ready = !v.reason.text.trim().is_empty() && !v.closing;
    c = c.child(
        row()
            .w_full()
            .gap(8.0)
            .child(
                widgets::button("close-run", "Run Checks", false, can_run && !v.closing, k, move |s: &mut Main, _| run(s, &r, true))
                    .help("Run its checks now; a passing run is evidence"),
            )
            .child(spacer())
            .child(widgets::button("close-cancel", "Cancel", false, true, k, |s: &mut Main, _| s.sheet = None))
            .child(widgets::button("close-go", if v.closing { "Closing…" } else { "Close on Your Word" }, true, ready, k, move |s: &mut Main, _| close(s, &b, from))),
    );
    let _ = Color::WHITE;
    parts::sheet_card(&format!("Close {id} on your word"), 500.0, None, k).child(c)
}

fn close(s: &mut Main, id: &str, from: Status) {
    let reason = s.verify.reason.text.trim().to_string();
    if reason.is_empty() || s.verify.closing {
        return;
    }
    s.verify.closing = true;
    let id = id.to_string();
    let _ = from;
    parts::call(s, Req::Post(format!("/api/dockets/{id}/set"), json!({ "status": Status::Done.raw(), "anyway": true, "reason": reason })), move |s, r| {
        s.verify.closing = false;
        match r {
            Ok(_) => {
                s.sheet = None;
                s.toast(format!("{id} closed on your word"), false);
            }
            Err(e) => s.verify.close_error = Some(e),
        }
        s.load_page(&id);
    });
}

// ---- hold ----

/// On hold: why, and Resume (HoldBanner).
pub fn hold_banner(d: &Docket, k: &K) -> Option<El> {
    let why = d.hold()?;
    let id = d.id();
    Some(
        row()
            .key("hold-banner")
            .w_full()
            .padding(12.0)
            .gap(10.0)
            .radius(10.0)
            .continuous_corners()
            .fill(k.warning_tint)
            .border(1.0, k.blocked.alpha(0.25))
            .child(el().pt(1.0).child(icon(&glyphs::PAUSE_FILL, 16.0).color(k.blocked)))
            .child(
                col().gap(4.0).grow(1.0).min_w(0.0).child(text("On hold").font(font(13.0, 600.0).lh(16.0)).color(k.text)).child(
                    Rich::new()
                        .push(&why, |s| s.color(k.text))
                        .push("  Build and Review take no steps on it until you resume it.", |s| s.color(k.muted))
                        .el()
                        .font(font(12.5, 400.0).lh(17.0))
                        .wrap()
                        .selectable(true),
                ),
            )
            .child(widgets::button("hold-resume", "Resume", false, true, k, move |s: &mut Main, _| s.resume(&id)).help("Take it off hold: Build and Review carry on")),
    )
}
