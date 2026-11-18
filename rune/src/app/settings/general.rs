//! The app's own tabs (MenuBar.swift's `GeneralSettings`,
//! `NotificationSettings` and `HubSettings`): its look, how it opens, its
//! notifications, and the hub it follows. What the Rune app doesn't have yet
//! (hiding its menu bar item, opening at login, another hub) shows, turned
//! off, with why.

use rune::{El, row, text};
use rune_kit::Kit;

use super::Settings;
use super::ui::{self, Look};
use crate::hub::DEFAULT_HUB;
use crate::theme::{K, font, mono};

pub fn general(s: &Settings, k: &K, kit: &Kit) -> El {
    let appearance = ui::segmented(
        "appearance",
        "Appearance",
        &[("System", s.theme.is_none()), ("Light", s.theme == Some(false)), ("Dark", s.theme == Some(true))],
        true,
        k,
        |s: &mut Settings, i| s.set_theme([None, Some(false), Some(true)][i]),
    );
    let name = if s.dev() { "Wheelhouse Dev" } else { "Wheelhouse" };
    let quick = format!("Open {name} from any app with {}", crate::app::settings::CHORD);
    let rows = vec![
        ui::cell(vec![ui::labeled(label("Appearance", k), appearance)]),
        ui::cell(vec![ui::labeled(label("Show Wheelhouse in the menu bar", k), ui::switch(true, "Show Wheelhouse in the menu bar", false, k, kit, |_: &mut Settings| {}))]),
        ui::cell(vec![ui::labeled(label(&quick, k), ui::switch(true, &quick, false, k, kit, |_: &mut Settings| {}))]),
        ui::cell(vec![ui::labeled(label("Open at login", k), ui::switch(false, "Open at login", false, k, kit, |_: &mut Settings| {}))]),
    ];
    let foot = format!(
        "This Rune build always shows its menu bar item, and isn't installed, so it doesn't open at login; its quick panel is always on {}, never the installed app's ⌥⌘K.",
        crate::app::settings::CHORD
    );
    ui::form("general-form", 520.0, vec![ui::section("general", None, rows, Some(&foot), k)], kit)
}

pub fn notifications(_: &Settings, k: &K, kit: &Kit) -> El {
    use crate::app::notify;
    let can = notify::available();
    let (word, color) = match notify::macos() {
        1 => ("Allowed", k.working),
        2 => ("Turned off in System Settings", k.error),
        _ => ("Not asked yet", k.muted),
    };
    let rows = vec![
        ui::cell(vec![
            ui::labeled(
                label("Notify me about what needs me", k),
                ui::switch(notify::on(), "Notify me about what needs me", can, k, kit, |_: &mut Settings| notify::set_on(!notify::on())),
            ),
            ui::words(
                "Prompts to approve, messages and questions, a finished turn from an agent you messaged, and reviews ready to post. Each has its answer on the banner, and goes away once answered anywhere.",
                11.5,
                k.muted,
            ),
        ]),
        ui::cell(vec![ui::labeled(label("macOS", k), text(word).font(font(13.0, 400.0).lh(17.0)).color(color))]),
        ui::cell(vec![row().gap(8.0).child(ui::button("notify-test", "Send a Test", Look::Plain, can, k, |_: &mut Settings, _| notify::test())).child(ui::button(
            "notify-settings",
            "Open System Settings…",
            Look::Plain,
            true,
            k,
            |s: &mut Settings, _| {
                open(s, "x-apple.systempreferences:com.apple.Notifications-Settings.extension?id=dev.wheelhouse.rune");
            },
        ))]),
    ];
    let foot = if can { None } else { Some("Off for this run: WH_RUNE_NOTIFY=off, or the app isn't running from its bundle (macOS notifies only for one).") };
    ui::form("notifications-form", 520.0, vec![ui::section("notifications", None, rows, foot, k)], kit)
}

pub fn hub(s: &Settings, k: &K, kit: &Kit) -> El {
    let address = ui::field_box(text(s.store.hub.base().to_string()).font(mono(12.5).lh(17.0)).color(k.text).selectable(true).label("Address"), k).w(260.0);
    let status = match s.store.state() {
        Some(st) if st.rex_connected() => ("Running · following Rex".to_string(), k.working),
        Some(st) => (format!("Running · Rex: {}", st.rex_error().unwrap_or_else(|| "connecting".into())), k.blocked),
        None => (s.store.error().unwrap_or_else(|| "Connecting…".into()), k.error),
    };
    let running = s.store.state().is_some();
    let rows = vec![
        ui::cell(vec![
            ui::labeled(label("Address", k), address),
            ui::words(
                format!(
                    "The hub on this Mac answers at {DEFAULT_HUB}. Another address is for a second hub you run by hand (WH_HTTP): this build reads it from WH_HUB when it starts."
                ),
                11.5,
                k.muted,
            ),
        ]),
        ui::cell(vec![ui::labeled(label("Status", k), text(status.0).font(font(13.0, 400.0).lh(17.0)).color(status.1).wrap().max_w(300.0))]),
        ui::cell(vec![
            row()
                .gap(8.0)
                .child(ui::button("hub-start", "Start the Hub", Look::Plain, false, k, |_: &mut Settings, _| {}).help(if running {
                    "It's running"
                } else {
                    "This build never starts a hub: start the made-up world's with its script, or Wheelhouse Dev's with `wh-dev up`"
                }))
                .child(ui::button("hub-log", "Show the Log", Look::Plain, true, k, |s: &mut Settings, _| {
                    let log = format!("{}/Library/Logs/wheelhouse.log", std::env::var("HOME").unwrap_or_default());
                    open(s, &log);
                }))
                .child(ui::button("hub-verify", "Verify Every Record", Look::Plain, !s.is_busy("verify"), k, |s: &mut Settings, _| s.verify_all())),
        ]),
    ];
    ui::form("hub-form", 520.0, vec![ui::section("hub", None, rows, None, k)], kit)
}

fn label(t: &str, k: &K) -> El {
    text(t.to_string()).font(font(13.0, 400.0).lh(17.0)).color(k.text).wrap()
}

/// Open a file or a System Settings pane with macOS's `open`, never in a
/// headless run.
fn open(s: &Settings, what: &str) {
    if s.store.headless {
        return;
    }
    let _ = std::process::Command::new("/usr/bin/open").arg(what).spawn();
}
