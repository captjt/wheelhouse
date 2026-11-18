//! Keyboard Shortcuts (RootView.swift's ShortcutsSheet), opened with ⌘/ or
//! ?: what each key does, in two columns of groups.

use rune::{El, ScrollSpec, col, row, text};
use rune_kit::{Kit, ScrollBar};

use super::{Main, parts, widgets};
use crate::theme::{K, font};

/// The quick panel's chord: this app's (⌃⌥⌘K), never the installed one's.
const CHORD: &str = "⌃⌥⌘K";

fn groups(name: &str) -> Vec<(&'static str, Vec<(String, String)>)> {
    let g = |pairs: &[(&str, &str)]| pairs.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect::<Vec<_>>();
    vec![
        ("Anywhere", vec![(CHORD.to_string(), format!("{name} over any app: approve, answer, find a docket, start one"))]),
        (
            "Go",
            g(&[
                ("⌘K  or  /", "Search dockets and commands"),
                ("⌘1 … ⌘8", "Inbox, Reviews, Builds, Active, All, Done, Agents, Recap"),
                ("G, then I R B A D X N P", "The same, as in the web app"),
                ("⌘R", "Refresh"),
                ("⌘,", "Settings: standing orders, agents, repos, guides"),
            ]),
        ),
        ("Create", g(&[("⌘N  or  C", "New docket"), ("⇧⌘N", "Start an agent on a new docket")])),
        (
            "A list",
            g(&[
                ("J K  or  ↑ ↓", "Move"),
                ("↩  or  double-click", "Open the docket in its own window"),
                ("O", "Show the agent's pane in Rex"),
                ("S, then 1–4", "Change status"),
                ("E", "In the Inbox: done"),
                ("Swipe", "Done, Drop, Status; Approve, Deny, File in the Inbox"),
            ]),
        ),
        (
            "A docket",
            g(&[
                ("⇧⌘O", "Open in Rex"),
                ("⇧⌘P", "Peek at the pane"),
                ("⇧⌘R", "Read the record"),
                ("⌥⌘1 … ⌥⌘4", "To do, In progress, Done, Dropped"),
                ("⇧⌘U", "Run its checks"),
                ("⇧⌘H", "Hold, or resume"),
                ("⇧⌘A", "Start an agent on it"),
                ("Double-click the title", "Rename"),
                ("Drop a file", "Keep it with the docket, pinned by its hash"),
            ]),
        ),
        ("Writing", g(&[("⌘↩", "Send, save, or note"), ("↩  /  ⇧↩", "In Chat: send / a new line"), ("Esc", "Cancel"), ("⌘B  ⌘I", "Bold, italic (Markdown)")])),
    ]
}

fn group(title: &str, keys: &[(String, String)], k: &K) -> El {
    let mut c = col().grow(1.0).min_w(0.0).gap(7.0).child(parts::section_label(title, k));
    for (key, what) in keys {
        c = c.child(
            row()
                .w_full()
                .gap(10.0)
                .items_start()
                .child(
                    text(key.clone())
                        .font(font(12.0, 500.0).lh(15.0))
                        .color(k.text)
                        .px(6.0)
                        .py(2.0)
                        .radius(5.0)
                        .fill(k.recessed)
                        .border(1.0, k.line),
                )
                .child(text(what.clone()).font(font(12.5, 400.0).lh(16.0)).color(k.text2).wrap().shrink().grow(1.0).min_w(0.0).pt(1.5)),
        );
    }
    c
}

pub fn sheet(s: &Main, k: &K, kit: &Kit) -> El {
    let name = if s.dev() { "Wheelhouse Dev" } else { "Wheelhouse" };
    let gs = groups(name);
    let mut grid = col().key("shortcuts-grid").w_full().padding(20.0).gap(22.0);
    for pair in gs.chunks(2) {
        let mut r = row().w_full().gap(24.0);
        for (title, keys) in pair {
            r = r.child(group(title, keys, k));
        }
        grid = grid.child(r);
    }
    let head = parts::sheet_head(
        text("Keyboard Shortcuts").font(font(15.0, 600.0).lh(19.0)).color(k.text),
        widgets::button("shortcuts-done", "Done", false, true, k, |s: &mut Main, _| s.sheet = None),
    );
    let body = col().key("shortcuts-scroll").w_full().grow(1.0).min_h(0.0).child(grid).scroll(ScrollSpec { stick_bottom: false }).scroll_bar(kit);
    parts::sheet_card("Keyboard Shortcuts", 720.0, Some(520.0), k).child(head).child(parts::divider(k)).child(body)
}
