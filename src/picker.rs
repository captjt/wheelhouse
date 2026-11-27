//! Claude Code's question picker (its `AskUserQuestion` tool), answered
//! from Wheelhouse.
//!
//! Claude Code reports the picker as `blocked` on `question`, but its
//! status says only the first question. The picker itself (each question's
//! tab, the question, its numbered options, the review before Submit) is
//! on the pane's screen, so the hub reads it from there ([`parse`]) and
//! answers it with the keys a person would press at the Mac:
//!
//! - a digit picks an option (and moves to the next question), or toggles
//!   it on a question that takes several;
//! - on "Type something", the digit opens its text box: Ctrl+E goes to the
//!   end of any draft there, Backspace clears it, and the words are pasted
//!   and taken with Enter. (On a question that takes several, the digit
//!   only ticks the box, so typed answers there are the pane's.)
//! - while that box is open, a digit is typed into it, so Up leaves it
//!   first (it keeps its draft);
//! - Right moves on from a question that takes several, Left goes back;
//! - on the review, "1. Submit answers" sends them all.
//!
//! The guard is approve's: keys go in only if, read from the pane's runtime
//! right now, it's still blocked on that one question (the same asking of
//! it: its report time), and the screen shows the question and the option,
//! ticked or not, that you were shown. Each answer is an `approval` entry
//! on the docket, witnessed by the hub, even one that only partly went in.

use anyhow::{Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::Duration;

use crate::approve::{Answering, plain};
use crate::hub::{Shared, Who};
use crate::mirror::{Record, app_name};
use crate::runtime;

/// One question's tab, along the top of a picker with several.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Tab {
    pub label: String,
    pub answered: bool,
}

/// What an option does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// One of the agent's answers.
    Choice,
    /// "Type something": your own words.
    Other,
    /// "Chat about this": no answer, talk to the agent instead.
    Chat,
    /// On the review: send the answers.
    Submit,
    /// On the review: answer none of them.
    Cancel,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Opt {
    pub n: u8,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// On a question that takes several: whether it's ticked.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checked: Option<bool>,
    pub kind: Kind,
}

/// A question and its answer so far, on the review.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Answer {
    pub question: String,
    pub answer: String,
}

/// The picker as the pane shows it now.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Picker {
    /// Each question's tab; one, on a picker with a single question.
    pub tabs: Vec<Tab>,
    /// Whether there's a Submit tab: several questions, or one that takes
    /// several answers.
    pub submit_tab: bool,
    /// The review before Submit, rather than a question.
    pub review: bool,
    /// The question shown, on one line. Empty on the review.
    pub question: String,
    /// Whether the question takes several answers (its options tick).
    pub multi: bool,
    pub options: Vec<Opt>,
    /// On a question that takes several: the move on, "Next" or "Submit".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next: Option<String>,
    /// On the review: each question and its answer.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub answers: Vec<Answer>,
    /// On the review: anything else it says (an unanswered question).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
    /// The "Type something" box is open: a digit would be typed into it.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub typing: bool,
    /// What's typed in that box already, open or not.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub draft: Option<String>,
}

impl Picker {
    /// Whether Wheelhouse takes your own words for this question.
    pub fn typed(&self) -> bool {
        !self.multi && self.options.iter().any(|o| o.kind == Kind::Other)
    }

    /// Everything it shows, for the hidden-character check.
    fn text(&self) -> String {
        let mut s = self.question.clone();
        for t in &self.tabs {
            s.push_str(&t.label);
        }
        for o in &self.options {
            s.push_str(&o.label);
            s.push_str(o.description.as_deref().unwrap_or(""));
        }
        for a in &self.answers {
            s.push_str(&a.question);
            s.push_str(&a.answer);
        }
        s.extend(self.notes.iter().map(String::as_str));
        s
    }
}

/// The agents whose picker Wheelhouse reads and answers.
pub fn answers(app: &str) -> bool {
    app == "claude-code"
}

/// The longest picker read off a screen, in lines: a question with four
/// long-described options is well under it.
const MAX_LINES: usize = 80;
/// How much of the pane is read for it.
const CAPTURE: usize = 120;
/// The longest answer typed into "Type something".
pub const MAX_TYPED: usize = 2000;

fn is_rule(l: &str) -> bool {
    let t = l.trim();
    t.chars().count() >= 8 && t.chars().all(|c| c == '─')
}

/// The tabs line: `←  ☐ Color  ☒ Pets  ✔ Submit  →`, or ` ☐ Go` alone.
fn tabs(l: &str) -> Option<(Vec<Tab>, bool)> {
    let t = l.trim();
    let t = t.strip_prefix('←').unwrap_or(t);
    let t = t.strip_suffix('→').unwrap_or(t).trim();
    if t.is_empty() {
        return None;
    }
    let mut out = Vec::new();
    let mut submit = false;
    for tok in t.split("  ").map(str::trim).filter(|s| !s.is_empty()) {
        let mut cs = tok.chars();
        let mark = cs.next()?;
        let label = cs.as_str().trim();
        match mark {
            '☐' | '☒' if !label.is_empty() => out.push(Tab { label: label.to_string(), answered: mark == '☒' }),
            '✔' if label == "Submit" => submit = true,
            _ => return None,
        }
    }
    (!out.is_empty()).then_some((out, submit))
}

/// A numbered line at an option's indent: `❯ 1. Red`, `  2. [ ] Dog`.
/// Descriptions sit further in, so a description's own "1. …" isn't one.
/// `(n, rest, whether the cursor is on it)`.
fn numbered(l: &str) -> Option<(u8, &str, bool)> {
    let t = l.trim_start_matches(' ');
    if l.len() - t.len() > 2 {
        return None;
    }
    let cursor = t.starts_with('❯');
    let t = t.strip_prefix('❯').map(str::trim_start).unwrap_or(t);
    let digits = t.len() - t.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    if digits == 0 || digits > 2 {
        return None;
    }
    let n: u8 = t[..digits].parse().ok()?;
    let rest = t[digits..].strip_prefix(". ")?;
    Some((n, rest.trim(), cursor))
}

/// A line of text in the picker: without the cursor or the bar a long
/// question is drawn with.
fn words(l: &str) -> &str {
    let t = l.trim();
    let t = t.strip_prefix('│').map(str::trim).unwrap_or(t);
    t.strip_prefix('❯').map(str::trim).unwrap_or(t)
}

fn join(lines: &[&str]) -> String {
    lines.iter().map(|l| words(l)).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(" ")
}

/// The same words, however a screen wrapped them.
pub fn same_words(a: &str, b: &str) -> bool {
    a.split_whitespace().eq(b.split_whitespace())
}

/// A checkbox on an option's label: `[ ] Cat`, `[✔] Fish`.
fn checkbox(label: &str) -> (Option<bool>, &str) {
    if let Some(r) = label.strip_prefix("[ ]") {
        (Some(false), r.trim())
    } else if let Some(r) = label.strip_prefix("[✔]").or_else(|| label.strip_prefix("[x]")).or_else(|| label.strip_prefix("[X]")) {
        (Some(true), r.trim())
    } else {
        (None, label)
    }
}

/// An option as read: its number, its label, the lines under it, and
/// whether the cursor is on it.
type Raw = (u8, String, Vec<String>, bool);

/// Numbered options in `lines`, each with the lines under it as its
/// description, from the last run that counts 1, 2, 3… to the end. A
/// question with a numbered list in it is text above the run. `(index of
/// option 1, options)`.
fn options(lines: &[&str]) -> Option<(usize, Vec<Raw>)> {
    let nums: Vec<(usize, u8)> = lines.iter().enumerate().filter_map(|(i, l)| numbered(l).map(|(n, ..)| (i, n))).collect();
    // From the last option up: each one before it is one less, down to 1.
    let mut k = nums.len().checked_sub(1)?;
    while nums[k].1 != 1 {
        if k == 0 || nums[k - 1].1 + 1 != nums[k].1 {
            return None;
        }
        k -= 1;
    }
    let first = nums[k].0;
    let mut out: Vec<Raw> = Vec::new();
    for l in &lines[first..] {
        match numbered(l) {
            Some((n, rest, cursor)) => out.push((n, rest.to_string(), Vec::new(), cursor)),
            None => {
                let w = words(l);
                if !w.is_empty() {
                    out.last_mut()?.2.push(w.to_string());
                }
            }
        }
    }
    Some((first, out))
}

/// The picker at the bottom of a pane's screen, if one is there.
pub fn parse(screen: &str) -> Option<Picker> {
    let lines: Vec<&str> = screen.lines().map(str::trim_end).collect();
    let end = lines.iter().rposition(|l| !l.trim().is_empty())?;
    let floor = end.saturating_sub(MAX_LINES);
    // The tabs, right under a rule: the picker's top.
    let top = (floor.max(1)..=end).rev().find(|&i| is_rule(lines[i - 1]) && tabs(lines[i]).is_some())?;
    let (tabs, submit_tab) = tabs(lines[top])?;
    let body = &lines[top + 1..=end];

    if body.iter().any(|l| l.trim() == "Review your answers") {
        return review(tabs, submit_tab, body);
    }

    // A question: its footer is the last line.
    let foot = body.last()?.trim();
    if !(foot.starts_with("Enter to select") && foot.contains("Esc to cancel")) {
        return None;
    }
    let body = &body[..body.len() - 1];
    // A rule between your answers and "Chat about this".
    let rule = body.iter().rposition(|l| is_rule(l));
    let (above, below) = match rule {
        Some(r) => (&body[..r], &body[r + 1..]),
        None => (body, &body[..0]),
    };
    let (first, mut opts) = options(above)?;
    let question = join(&above[..first]);
    if question.is_empty() {
        return None;
    }
    // On a question that takes several, the line after "Type something"
    // moves on: "Next", or "Submit" on the last.
    let mut next = None;
    if let Some(last) = opts.last_mut()
        && let Some(w) = last.2.last()
        && matches!(w.as_str(), "Next" | "Submit")
        && checkbox(&last.1).0.is_some()
    {
        next = last.2.pop();
    }
    let others = opts.len();
    let on_other = opts.last().is_some_and(|o| o.3);
    // Words in the box already: anything but its placeholder, wrapped or not.
    let draft = opts.last().map(|o| checkbox(&o.1).1).filter(|l| !l.starts_with("Type something")).map(|l| {
        let last = opts.last().expect("an option");
        std::iter::once(l).chain(last.2.iter().map(String::as_str)).collect::<Vec<_>>().join(" ")
    });
    let mut out: Vec<Opt> = Vec::new();
    for (i, (n, label, desc, _)) in opts.into_iter().enumerate() {
        let (checked, label) = checkbox(&label);
        let kind = if i + 1 == others { Kind::Other } else { Kind::Choice };
        out.push(Opt { n, label: label.to_string(), description: (!desc.is_empty()).then(|| desc.join(" ")), checked, kind });
    }
    // Below the rule: "Chat about this", numbered on from the rest.
    for l in below {
        if let Some((n, rest, _)) = numbered(l) {
            if n as usize != out.len() + 1 {
                return None;
            }
            out.push(Opt { n, label: rest.to_string(), description: None, checked: None, kind: Kind::Chat });
        } else if !words(l).is_empty() {
            return None;
        }
    }
    let multi = out.iter().any(|o| o.checked.is_some());
    // The box for your own words, open: the cursor on it, or the footer's
    // hint for editing it elsewhere.
    let typing = on_other || foot.contains("ctrl+g");
    Some(Picker { tabs, submit_tab, review: false, question, multi, options: out, next, answers: Vec::new(), notes: Vec::new(), typing, draft })
}

fn review(tabs: Vec<Tab>, submit_tab: bool, body: &[&str]) -> Option<Picker> {
    // Its options are the last lines on the screen, with nothing under them.
    numbered(body.last()?)?;
    let (first, opts) = options(body)?;
    if opts.iter().any(|o| !o.2.is_empty()) {
        return None;
    }
    let mut answers: Vec<Answer> = Vec::new();
    let mut notes = Vec::new();
    for l in &body[..first] {
        let w = words(l);
        if w.is_empty() || w == "Review your answers" {
            continue;
        }
        if let Some(q) = w.strip_prefix("● ") {
            answers.push(Answer { question: q.trim().to_string(), answer: String::new() });
        } else if let Some(a) = w.strip_prefix("→ ")
            && let Some(last) = answers.last_mut()
        {
            last.answer = a.trim().to_string();
        } else if let Some(last) = answers.last_mut().filter(|a| !a.answer.is_empty()) {
            // An answer that wrapped.
            last.answer.push(' ');
            last.answer.push_str(w);
        } else {
            notes.push(w.to_string());
        }
    }
    let options: Vec<Opt> = opts
        .into_iter()
        .map(|(n, label, desc, _)| {
            let kind = match label.as_str() {
                "Submit answers" => Kind::Submit,
                "Cancel" => Kind::Cancel,
                _ => Kind::Choice,
            };
            Opt { n, label, description: (!desc.is_empty()).then(|| desc.join(" ")), checked: None, kind }
        })
        .collect();
    if !options.iter().any(|o| o.kind == Kind::Submit) {
        return None;
    }
    Some(Picker { tabs, submit_tab, review: true, question: String::new(), multi: false, options, next: None, answers, notes, typing: false, draft: None })
}

// ---- answering ----

/// What you did, on the picker as the app showed it.
#[derive(Debug, Deserialize)]
pub struct Act {
    /// The agent's pane, on a docket with several: its lead when not given.
    #[serde(default)]
    pub block: Option<String>,
    /// When the question you saw was asked: its record's report time, as
    /// the hub gave it with the picker. Asked again since, it's refused.
    #[serde(default)]
    pub since: Option<String>,
    /// The question you were shown; empty on the review.
    #[serde(default)]
    pub question: String,
    /// Whether you were shown the review.
    #[serde(default)]
    pub review: bool,
    /// `pick` (an option, by `n` and `label`), `next`, or `back`.
    pub action: String,
    #[serde(default)]
    pub n: Option<u8>,
    #[serde(default)]
    pub label: Option<String>,
    /// On a question that takes several: whether the option was ticked
    /// when you saw it.
    #[serde(default)]
    pub checked: Option<bool>,
    /// For "Type something": your words.
    #[serde(default)]
    pub text: Option<String>,
}

/// One step of an answer: keys pressed, or words pasted and taken with Enter.
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    Keys(Vec<String>),
    Text(String),
}

impl Step {
    fn keys(ks: &[&str]) -> Step {
        Step::Keys(ks.iter().map(|k| k.to_string()).collect())
    }
}

/// The steps for `act` on `p`, the picker on the screen now, and what they
/// do in words for the record. Refused unless `p` is the picker you saw.
pub fn plan(p: &Picker, act: &Act) -> Result<(Vec<Step>, String)> {
    if p.review != act.review || (!p.review && !same_words(&p.question, &act.question)) {
        bail!("The question changed since you saw it: it's now {}. Nothing was sent.", if p.review { "the review before Submit".to_string() } else { format!("“{}”", p.question) });
    }
    let about = if p.review { "the answers".to_string() } else { p.question.clone() };
    // The box for your own words is open: a digit (or an arrow) would go
    // into it. Up leaves it, keeping what's typed there.
    let leave: Vec<Step> = if p.typing { vec![Step::keys(&["up"])] } else { Vec::new() };
    let then = |s: Step| leave.iter().cloned().chain(std::iter::once(s)).collect::<Vec<_>>();
    match act.action.as_str() {
        "pick" => {
            let n = act.n.ok_or_else(|| anyhow!("Which option? Nothing was sent."))?;
            let o = p.options.iter().find(|o| o.n == n).ok_or_else(|| anyhow!("There's no option {n} now. Nothing was sent."))?;
            if n > 9 {
                bail!("Option {n} can't be picked with one key. Answer it in the pane.");
            }
            let key = n.to_string();
            if o.kind == Kind::Other {
                if p.multi {
                    bail!("On a question that takes several answers, type your own in the pane.");
                }
                let text = act.text.as_deref().map(str::trim).unwrap_or("");
                if text.is_empty() {
                    bail!("Type your answer first. Nothing was sent.");
                }
                if text.contains(['\n', '\r']) || !plain(Some(text)) {
                    bail!("Your answer has to be one line of plain text. Nothing was sent.");
                }
                if text.chars().count() > MAX_TYPED {
                    bail!("Your answer is longer than {MAX_TYPED} characters. Nothing was sent.");
                }
                // Open the box (unless it is), go to the end of what's in it,
                // clear that, then the words and Enter.
                let mut steps = Vec::new();
                if !p.typing {
                    steps.push(Step::Keys(vec![key]));
                }
                let mut clear = vec!["ctrl+e".to_string()];
                // A wrapped draft lost a space at each break: a few more
                // Backspaces than it shows, which an empty box ignores.
                let draft = p.draft.as_deref().map_or(0, |d| d.chars().count() + d.split(' ').count());
                clear.extend(std::iter::repeat_n("backspace".to_string(), draft.min(MAX_TYPED + 200)));
                steps.push(Step::Keys(clear));
                steps.push(Step::Text(text.to_string()));
                return Ok((steps, format!("{about} → “{text}”")));
            }
            let seen = act.label.as_deref().unwrap_or("");
            if !same_words(seen, &o.label) {
                bail!("Option {n} is now “{}”, not “{seen}”. Nothing was sent.", o.label);
            }
            if let (Some(now), Some(was)) = (o.checked, act.checked)
                && now != was
            {
                bail!("“{}” is {} now. Nothing was sent.", o.label, if now { "ticked" } else { "unticked" });
            }
            let said = match (o.kind, o.checked) {
                (Kind::Submit, _) => "Submitted the answers".to_string(),
                (Kind::Cancel, _) => "Cancelled the questions".to_string(),
                (Kind::Chat, _) => format!("{about} → chat about it instead"),
                (_, Some(was)) => format!("{about}: {} “{}”", if was { "unticked" } else { "ticked" }, o.label),
                _ => format!("{about} → {}", o.label),
            };
            Ok((then(Step::Keys(vec![key])), said))
        }
        "next" => {
            let Some(next) = p.next.as_deref().filter(|_| p.multi) else { bail!("This question moves on when you pick an answer. Nothing was sent.") };
            Ok((then(Step::keys(&["right"])), format!("{about}: {}", next.to_lowercase())))
        }
        "back" => {
            if !p.submit_tab && p.tabs.len() < 2 {
                bail!("There's no question before this one. Nothing was sent.");
            }
            Ok((then(Step::keys(&["left"])), format!("{about}: back a question")))
        }
        other => bail!("Wheelhouse doesn't know “{other}” on a question. Nothing was sent."),
    }
}

/// The picker now, read from the pane's runtime.
async fn read(session: &str, block: &str) -> Result<Option<Picker>> {
    let screen = runtime::for_id(block).capture(session, block, CAPTURE).await?;
    Ok(parse(&screen))
}

/// The picker on a docket's agent, if it's asking one, with when it was
/// asked (`since`, for the answer to send back).
pub async fn current(shared: &Shared, docket: &str, block: Option<&str>) -> Result<Value> {
    let (session, block, live) = {
        let core = shared.lock();
        let st = core.store.state(docket)?.ok_or_else(|| anyhow!("no docket {docket}"))?;
        let b = crate::agents::pane_on(&core, &st, block).map_err(|e| anyhow!(e))?;
        let p = core.mirror.panes.get(&b).ok_or_else(|| anyhow!("{docket}'s pane isn't open in {}", runtime::of(&b).title()))?;
        (p.session_id.clone(), p.block_id.clone(), crate::agents::live(&core, &b))
    };
    let (mut picker, mut since) = (None, None);
    if live.as_ref().is_some_and(|l| l.pickable) {
        let records = runtime::for_id(&block).records(&session, &block).await?;
        if let Ok(r) = asking(&records) {
            since = r.updated_at.clone();
            picker = read(&session, &block).await?.filter(|p| plain(Some(&p.text())));
        }
    }
    let typed = picker.as_ref().is_some_and(Picker::typed);
    Ok(json!({ "picker": picker, "since": since, "typed": typed, "live": live }))
}

/// The one question the pane's records, read now, are waiting on.
fn asking(records: &[Record]) -> Result<&Record> {
    let blocked: Vec<&Record> = records.iter().filter(|r| r.state == "blocked").collect();
    match blocked.as_slice() {
        [r] if r.kind.as_deref() == Some("question") => Ok(r),
        [] => bail!("The agent isn't waiting on a question any more. Nothing was sent."),
        [r] => bail!("The agent is waiting on a {} now, not a question. Nothing was sent.", r.kind.as_deref().unwrap_or("prompt")),
        many => bail!("{} prompts are waiting in that pane, so a key could answer the wrong one. Answer it in the pane.", many.len()),
    }
}

/// Whether the question waiting is the asking of it you saw: a question
/// with no report time can't be told from the next, so it never matches.
fn same_asking(r: &Record, since: Option<&str>) -> bool {
    r.updated_at.is_some() && r.updated_at.as_deref() == since
}

/// Answer the picker on a docket's agent: `act`, checked against the pane
/// as it is now.
pub async fn answer(shared: &Shared, docket: &str, act: &Act) -> Result<Value> {
    let (session, block, app, mirrored) = {
        let core = shared.lock();
        let st = core.store.state(docket)?.ok_or_else(|| anyhow!("no docket {docket}"))?;
        let b = crate::agents::pane_on(&core, &st, act.block.as_deref()).map_err(|e| anyhow!(e))?;
        let p = core.mirror.panes.get(&b).ok_or_else(|| anyhow!("{docket}'s pane isn't open in {}", runtime::of(&b).title()))?;
        let mirrored = p.records.values().filter(|r| r.state == "blocked").count();
        (p.session_id.clone(), p.block_id.clone(), p.app().unwrap_or_default().to_string(), mirrored)
    };
    let where_ = runtime::of(&block).title();
    if !answers(&app) {
        bail!("Wheelhouse doesn't answer {}'s questions yet. Answer it in {where_}.", app_name(&app));
    }
    // One answer per pane at a time, a permission's or a question's.
    let Some(_answering) = Answering::take(&block) else {
        bail!("That question is being answered already. Nothing was sent.");
    };

    // The guard: the pane's records and its screen, read now.
    let rt = runtime::for_id(&block);
    let records = rt.records(&session, &block).await?;
    let r = asking(&records)?;
    if !same_asking(r, act.since.as_deref()) {
        bail!("The agent asked again since you saw it. Nothing was sent: look at the question again.");
    }
    if mirrored > 1 {
        bail!("{mirrored} prompts are waiting in that pane, so a key could answer the wrong one. Answer it in {where_}.");
    }
    let Some(before) = read(&session, &block).await? else {
        bail!("Wheelhouse can't read the question on the pane's screen. Answer it in {where_}.");
    };
    if !plain(Some(&before.text())) {
        bail!("That question has characters in it that don't show, so what you'd pick isn't what you read. Answer it in {where_}.");
    }
    let (steps, said) = plan(&before, act)?;

    // From the first key on, something went in: whatever happens, say what,
    // on the record and to you, and never send the rest blind.
    let mut done: Vec<String> = Vec::new();
    let mut failed: Option<anyhow::Error> = None;
    for (i, step) in steps.iter().enumerate() {
        if i > 0 {
            // A moment for the picker to take the last step (the box to open).
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        let r = match step {
            Step::Keys(ks) => {
                let refs: Vec<&str> = ks.iter().map(String::as_str).collect();
                rt.keys(&session, &block, &refs).await
            }
            Step::Text(t) => rt.type_into_dialog(&session, &block, t).await,
        };
        match r {
            Ok(()) => done.push(match step {
                Step::Keys(ks) if ks.len() > 3 => format!("{} and {} more keys", ks[..2].join(" "), ks.len() - 2),
                Step::Keys(ks) => ks.join(" "),
                Step::Text(_) => "your words and Enter".to_string(),
            }),
            Err(e) => {
                failed = Some(e);
                break;
            }
        }
    }
    if done.is_empty()
        && let Some(e) = failed
    {
        return Err(e.context("Nothing was sent"));
    }

    // Something went in: say so even if the record can't be written now.
    let recorded = {
        let mut core = shared.lock();
        let owner = core.owner.clone();
        let typed = steps.iter().find_map(|s| if let Step::Text(t) = s { Some(t.clone()) } else { None });
        let choice = act.n.and_then(|n| before.options.iter().find(|o| o.n == n)).map(|o| if o.kind == Kind::Other { typed.clone().unwrap_or_default() } else { o.label.clone() });
        let keys: Vec<String> = steps.iter().flat_map(|s| if let Step::Keys(ks) = s { ks.clone() } else { Vec::new() }).collect();
        let body = if failed.is_some() { format!("{said} (only partly: {})", done.join(", then ")) } else { said.clone() };
        let data = json!({ "decision": "answer", "by": owner, "kind": "question", "action": act.action, "question": (!before.review).then(|| before.question.clone()), "choice": choice, "keys": keys, "partial": failed.is_some(), "block": block });
        match core.append(docket, "approval", &Who::Hub, Some(body), data) {
            Ok(_) => true,
            Err(e) => {
                say!("{docket}: the answer went in, but wasn't recorded: {e:#}");
                false
            }
        }
    };
    if let Some(e) = failed {
        bail!("Only part of that went in ({}), then: {e:#}. Look at the pane before answering again.", done.join(", then "));
    }

    // The picker as it is now: the next question, the review, or gone. A
    // read mid-redraw that doesn't parse isn't an answer: read again.
    for _ in 0..16 {
        tokio::time::sleep(Duration::from_millis(250)).await;
        let now_records = match rt.records(&session, &block).await {
            Ok(r) => r,
            Err(_) => continue,
        };
        let Ok(r) = asking(&now_records) else {
            return Ok(json!({ "did": said, "cleared": true, "picker": null, "recorded": recorded }));
        };
        let since = r.updated_at.clone();
        if let Ok(Some(now)) = read(&session, &block).await
            && now != before
        {
            let typed = now.typed();
            return Ok(json!({ "did": said, "cleared": false, "picker": now, "since": since, "typed": typed, "recorded": recorded }));
        }
    }
    Ok(json!({ "did": said, "cleared": false, "picker": null, "recorded": recorded, "now": "the pane didn't change in 4s" }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const REX_BOXED: &str = include_str!("../tests/fixtures/picker/rex-boxed.txt");
    const MULTI: &str = include_str!("../tests/fixtures/picker/multi.txt");
    const TABS: &str = include_str!("../tests/fixtures/picker/tabs.txt");
    const REVIEW: &str = include_str!("../tests/fixtures/picker/review.txt");

    const ONE: &str = "\
⏺ Ready to ship.
────────────────────────────────────────────────
 ☐ Go

Ship it now?

❯ 1. Yes, ship
     Push and open the PR
  2. Not yet
     Not yet
  3. Type something.
────────────────────────────────────────────────
  4. Chat about this

Enter to select · ↑/↓ to navigate · Esc to cancel
";

    fn labels(p: &Picker) -> Vec<(u8, &str, Kind)> {
        p.options.iter().map(|o| (o.n, o.label.as_str(), o.kind)).collect()
    }

    #[test]
    fn reads_a_question_rex_drew_with_a_bar() {
        let p = parse(REX_BOXED).expect("a picker");
        assert_eq!(p.tabs, vec![Tab { label: "Annotate".into(), answered: false }, Tab { label: "Scope".into(), answered: false }]);
        assert!(p.submit_tab && !p.review && !p.multi);
        assert!(p.question.starts_with("For annotated images: should Wheelhouse have its own markup tool (boxes, arrows, pen"), "{}", p.question);
        assert!(p.question.ends_with("and just paste the result?"), "{}", p.question);
        assert_eq!(
            labels(&p),
            vec![(1, "Built-in markup (Recommended)", Kind::Choice), (2, "Bring your own", Kind::Choice), (3, "Type something.", Kind::Other), (4, "Chat about this", Kind::Chat)]
        );
        assert!(p.options[0].description.as_deref().unwrap().starts_with("A small editor in the composer: box, arrow, pen, text, numbered pins. The flattened PNG"));
        assert!(p.typed());
    }

    #[test]
    fn reads_a_question_that_takes_several() {
        let p = parse(MULTI).expect("a picker");
        assert!(p.multi);
        assert_eq!(p.question, "Which tools?");
        assert_eq!(p.next.as_deref(), Some("Next"));
        let got: Vec<(&str, Option<bool>)> = p.options.iter().map(|o| (o.label.as_str(), o.checked)).collect();
        assert_eq!(got, vec![("Vim", Some(false)), ("Emacs", Some(false)), ("Type something", Some(true)), ("Chat about this", None)]);
        assert!(!p.typed(), "typed answers there are the pane's");
    }

    #[test]
    fn reads_the_second_question_and_the_review() {
        let p = parse(TABS).expect("a picker");
        assert_eq!(p.question, "Which OS?");
        assert_eq!(p.tabs.iter().map(|t| t.answered).collect::<Vec<_>>(), vec![true, false]);
        let r = parse(REVIEW).expect("the review");
        assert!(r.review && r.question.is_empty());
        assert_eq!(r.answers, vec![Answer { question: "Which tools?".into(), answer: "Vim".into() }, Answer { question: "Which OS?".into(), answer: "Linux".into() }]);
        assert_eq!(labels(&r), vec![(1, "Submit answers", Kind::Submit), (2, "Cancel", Kind::Cancel)]);
    }

    #[test]
    fn reads_a_single_question_without_tabs_to_move_between() {
        let p = parse(ONE).expect("a picker");
        assert_eq!(p.tabs, vec![Tab { label: "Go".into(), answered: false }]);
        assert!(!p.submit_tab);
        assert_eq!(p.options[0].description.as_deref(), Some("Push and open the PR"));
    }

    #[test]
    fn no_picker_is_read_where_there_is_none() {
        assert!(parse("").is_none());
        assert!(parse("⏺ Done.\n\n❯ \n────────\n  ⏵⏵ auto mode on").is_none());
        // The picker answered, scrolled up: something under it now.
        assert!(parse(&format!("{ONE}\n⏺ Answered Claude's questions:\n  ⎿  · Ship it now? → Yes, ship\n")).is_none());
        // A permission prompt isn't one.
        let perm = "────────────────\n Bash command\n\n   rm -rf build\n\n Do you want to proceed?\n ❯ 1. Yes\n   2. No\n\n Esc to cancel · Tab to amend";
        assert!(parse(perm).is_none());
        // Options that don't count from 1.
        assert!(parse(&ONE.replace("1. Yes, ship", "5. Yes, ship")).is_none());
    }

    #[test]
    fn a_numbered_list_in_the_question_stays_in_the_question() {
        let s = ONE.replace("Ship it now?", "Two ways:\n1. ship now\n2. ship later\nWhich?");
        let p = parse(&s).expect("a picker");
        assert_eq!(p.question, "Two ways: 1. ship now 2. ship later Which?");
        assert_eq!(p.options[0].label, "Yes, ship");
    }

    fn act(action: &str, q: &str, n: Option<u8>, label: Option<&str>, text: Option<&str>) -> Act {
        Act {
            block: None,
            since: None,
            question: q.into(),
            review: false,
            action: action.into(),
            n,
            label: label.map(str::to_string),
            checked: None,
            text: text.map(str::to_string),
        }
    }

    fn keys(ks: &[&str]) -> Step {
        Step::keys(ks)
    }

    #[test]
    fn keys_go_in_only_for_the_question_and_option_you_saw() {
        let p = parse(ONE).unwrap();
        let (steps, said) = plan(&p, &act("pick", "Ship it now?", Some(2), Some("Not yet"), None)).unwrap();
        assert_eq!((steps, said.as_str()), (vec![keys(&["2"])], "Ship it now? → Not yet"));
        // However the app wrapped it.
        assert!(plan(&p, &act("pick", "Ship  it\nnow?", Some(2), Some("Not yet"), None)).is_ok());
        assert!(plan(&p, &act("pick", "Deploy to prod?", Some(2), Some("Not yet"), None)).is_err(), "another question");
        assert!(plan(&p, &act("pick", "Ship it now?", Some(2), Some("Yes, ship"), None)).is_err(), "option 2 isn't what you saw");
        assert!(plan(&p, &act("pick", "Ship it now?", Some(7), Some("Not yet"), None)).is_err(), "no option 7");
        let review = Act { review: true, ..act("pick", "", Some(1), Some("Submit answers"), None) };
        assert!(plan(&p, &review).is_err(), "you saw the review, the pane shows a question");
        assert!(plan(&p, &act("next", "Ship it now?", None, None, None)).is_err(), "one answer moves on by itself");
        assert!(plan(&p, &act("back", "Ship it now?", None, None, None)).is_err(), "nothing before it");
        assert!(plan(&p, &act("shell", "Ship it now?", None, None, None)).is_err());
    }

    #[test]
    fn your_own_words_open_the_box_clear_it_and_go_in_on_one_line() {
        let p = parse(ONE).unwrap();
        let (steps, said) = plan(&p, &act("pick", "Ship it now?", Some(3), Some("Type something."), Some("  after the demo "))).unwrap();
        assert_eq!(steps, vec![keys(&["3"]), keys(&["ctrl+e"]), Step::Text("after the demo".into())]);
        assert_eq!(said, "Ship it now? → “after the demo”");
        assert!(plan(&p, &act("pick", "Ship it now?", Some(3), None, Some(" "))).is_err(), "nothing typed");
        assert!(plan(&p, &act("pick", "Ship it now?", Some(3), None, Some("yes\nrm -rf /"))).is_err(), "a newline would submit early");
        assert!(plan(&p, &act("pick", "Ship it now?", Some(3), None, Some("ok\u{1b}[2K"))).is_err(), "an escape");
        assert!(plan(&p, &act("pick", "Ship it now?", Some(3), None, Some(&"x".repeat(MAX_TYPED + 1)))).is_err());
        let m = parse(MULTI).unwrap();
        assert!(plan(&m, &act("pick", "Which tools?", Some(3), None, Some("Helix"))).is_err(), "the pane's, on one that takes several");
    }

    /// The box open, with "Gr" typed in it, as Claude Code draws it.
    const TYPING: &str = "\
────────────────────────────────────────────────
←  ☐ Color  ☐ Size  ✔ Submit  →

Which color?

  1. Red
     Choose red
  2. Blue
     Choose blue
❯ 3. Gr
────────────────────────────────────────────────
  4. Chat about this

Enter to select · Tab/Arrow keys to navigate · ctrl+g to edit in VS Code · Esc to cancel
";

    #[test]
    fn with_the_box_open_a_digit_would_be_typed_so_up_leaves_it_first() {
        let p = parse(TYPING).expect("a picker");
        assert!(p.typing);
        assert_eq!(p.draft.as_deref(), Some("Gr"));
        assert_eq!(p.options[2].kind, Kind::Other);
        let (steps, _) = plan(&p, &act("pick", "Which color?", Some(1), Some("Red"), None)).unwrap();
        assert_eq!(steps, vec![keys(&["up"]), keys(&["1"])]);
        // Your own words: it's open, so no digit; its draft is cleared first.
        let (steps, _) = plan(&p, &act("pick", "Which color?", Some(3), None, Some("Teal"))).unwrap();
        assert_eq!(steps, vec![keys(&["ctrl+e", "backspace", "backspace", "backspace"]), Step::Text("Teal".into())]);
        // A draft left in the box, closed: open it, then clear it.
        let closed = TYPING.replace("❯ 3. Gr", "  3. Gr").replace("  1. Red", "❯ 1. Red").replace(" · ctrl+g to edit in VS Code", "");
        let p = parse(&closed).unwrap();
        assert!(!p.typing);
        let (steps, _) = plan(&p, &act("pick", "Which color?", Some(3), None, Some("Teal"))).unwrap();
        assert_eq!(steps[0], keys(&["3"]));
        assert!(!parse(ONE).unwrap().typing, "the placeholder isn't a draft");
        assert!(parse(ONE).unwrap().draft.is_none());
    }

    #[test]
    fn a_description_that_counts_is_still_a_description() {
        let s = ONE.replace("     Push and open the PR", "     1. run the tests\n     2. push");
        let p = parse(&s).expect("a picker");
        assert_eq!(p.question, "Ship it now?");
        assert_eq!(labels(&p)[0], (1, "Yes, ship", Kind::Choice));
        assert_eq!(p.options[0].description.as_deref(), Some("1. run the tests 2. push"));
    }

    #[test]
    fn several_answers_tick_then_move_on() {
        let m = parse(MULTI).unwrap();
        let (steps, said) = plan(&m, &act("pick", "Which tools?", Some(1), Some("Vim"), None)).unwrap();
        assert_eq!((steps, said.as_str()), (vec![keys(&["1"])], "Which tools?: ticked “Vim”"));
        // Ticked since you saw it: tapping again would untick it.
        let stale = Act { checked: Some(true), ..act("pick", "Which tools?", Some(1), Some("Vim"), None) };
        assert!(plan(&m, &stale).is_err());
        let fresh = Act { checked: Some(false), ..act("pick", "Which tools?", Some(1), Some("Vim"), None) };
        assert!(plan(&m, &fresh).is_ok());
        let (steps, said) = plan(&m, &act("next", "Which tools?", None, None, None)).unwrap();
        assert_eq!((steps, said.as_str()), (vec![keys(&["right"])], "Which tools?: next"));
        let r = parse(REVIEW).unwrap();
        let submit = Act { review: true, ..act("pick", "", Some(1), Some("Submit answers"), None) };
        assert_eq!(plan(&r, &submit).unwrap().0, vec![keys(&["1"])]);
        let back = Act { review: true, ..act("back", "", None, None, None) };
        assert_eq!(plan(&r, &back).unwrap().0, vec![keys(&["left"])]);
        assert!(plan(&r, &act("pick", "Which OS?", Some(1), Some("Submit answers"), None)).is_err(), "the pane shows the review");
    }

    #[test]
    fn the_review_is_only_read_at_the_bottom() {
        assert!(parse(&format!("{REVIEW}\n⏺ Answered.\n")).is_none(), "something under it");
        assert!(parse(&REVIEW.replace("  2. Cancel", "  2. Cancel\n     and more")).is_none());
    }

    #[test]
    fn one_question_waiting_and_the_same_asking_of_it_or_nothing_is_sent() {
        let r = |state: &str, kind: Option<&str>, id: &str| Record {
            id: id.into(),
            state: state.into(),
            kind: kind.map(str::to_string),
            updated_at: Some("t1".into()),
            ..Default::default()
        };
        let q = [r("blocked", Some("question"), "")];
        assert!(asking(&q).is_ok());
        assert!(asking(&[r("working", None, "")]).is_err(), "answered already");
        assert!(asking(&[r("blocked", Some("permission"), "")]).is_err(), "a permission now");
        assert!(asking(&[r("blocked", Some("question"), ""), r("blocked", Some("permission"), "sub")]).is_err(), "two waiting");
        assert!(same_asking(asking(&q).unwrap(), Some("t1")));
        assert!(!same_asking(asking(&q).unwrap(), Some("t0")), "asked again since");
        assert!(!same_asking(asking(&q).unwrap(), None), "an app that didn't say");
        let untimed = Record { updated_at: None, ..r("blocked", Some("question"), "") };
        assert!(!same_asking(&untimed, None), "no time: never the same");
    }
}
