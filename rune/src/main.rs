//! wheelhouse-rune: Wheelhouse.app rewritten in Rune (docs/rune.md). The main
//! window (the sidebar, a list beside its docket, the inspector) and the ⌃⌥⌘K
//! quick panel, both on one Store that follows the hub's events. It reads
//! `WH_HUB` (the made-up world by default) and never talks to the daily hub.
//!
//! ```sh
//! cargo run --release                                          # the app
//! target/release/wheelhouse-rune --window main --scene docket --snap docket.png --size 1440x900
//! target/release/wheelhouse-rune --window panel --snap panel.png
//! ```

mod app;
mod commands;
mod hub;
mod icons;
mod model;
mod panel;
mod store;
mod theme;
mod view;

use rune::{App, AppSpec, Chrome, RunOptions, WindowSpec};

use hub::Hub;
use store::Store;

/// A headless run: Rune's tooling flags, read before Rune starts.
fn headless() -> bool {
    std::env::args().any(|a| matches!(a.as_str(), "--snap" | "--tree" | "--script" | "--video" | "--out" | "--trace" | "--list-scenes" | "--list-windows" | "--strip") && !std::env::args().any(|a| a == "--native"))
}

fn main() {
    let hub = match Hub::from_env() {
        Ok(h) => h,
        Err(e) => {
            eprintln!("wheelhouse-rune: {e}");
            std::process::exit(2);
        }
    };
    // `--show`, or WH_RUNE_SHOW=1 (the bundle's, so opening the app shows the panel).
    let show_panel = rune::tooling::extra_args().iter().any(|a| a == "--show") || std::env::var_os("WH_RUNE_SHOW").is_some_and(|v| v == "1");
    let store = Store::new(hub.clone(), headless());

    rune::run_windows_with(RunOptions { accessory: false }, move |_| {
        let spec = AppSpec {
            scenes: app::SCENES,
            poll: Some(app::poll),
            on_key: Some(app::on_key),
            ..AppSpec::new("Wheelhouse", (1440.0, 900.0), || unreachable!(), app::view)
        };
        // The traffic lights centred on the toolbar, in the framed pane.
        let chrome = Chrome { full_size_content: true, traffic_lights: Some((16.0, 25.0)), drag_regions: true };
        let mut main = App::new(spec).chrome(chrome).keep_running();
        let me = main.handle();
        // Settings (⌘,): a window of its own, hidden until the main window opens it.
        let (settings, _) = main.window(app::settings::spec(store.clone(), me.clone()));
        app::settings::link(settings);
        let follow = store.clone();
        let main = main.init(move || app::Main::new(store, me)).on_ready(move |_| follow.follow());
        let windows: Vec<rune::AnyWindowSpec> = vec![WindowSpec::from_app("main", main).into(), panel::spec(hub, show_panel).into()];
        windows
    });
}
