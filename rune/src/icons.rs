//! The panel's marks, drawn as the SwiftUI one draws them: SF Symbols'
//! outlines (redrawn here as SVG: Rune can't draw SF Symbols), the Logo
//! (Theme.swift) and Linear's status circles (`StatusIcon`).

use rune::Icon;

macro_rules! icon {
    ($id:ident, $name:literal, $view:literal, $body:expr) => {
        pub static $id: Icon = Icon { name: $name, view: $view, body: $body };
    };
}

icon!(
    SEARCH,
    "search",
    16.0,
    r#"<g stroke="currentColor" stroke-width="1.4" stroke-linecap="round" fill="none"><circle cx="6.75" cy="6.75" r="4.75"/><path d="M10.3 10.3L14 14"/></g>"#
);
icon!(
    APPROVE,
    "approve",
    16.0,
    r#"<g stroke="currentColor" stroke-width="1.25" stroke-linecap="round" stroke-linejoin="round" fill="none"><circle cx="8" cy="8" r="6.5"/><path d="M5.2 8.2L7.1 10.1L10.9 5.9"/></g>"#
);
icon!(
    DENY,
    "deny",
    16.0,
    r#"<g stroke="currentColor" stroke-width="1.25" stroke-linecap="round" fill="none"><circle cx="8" cy="8" r="6.5"/><path d="M5.7 5.7L10.3 10.3M10.3 5.7L5.7 10.3"/></g>"#
);
icon!(
    REPLY,
    "reply",
    16.0,
    r#"<path d="M6.6 3.2L2 7.6L6.6 12V9.4H9.2C11.4 9.4 13 10.4 14 12.6C14 8.9 12.1 6 9.2 6H6.6Z" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" fill="none"/>"#
);
icon!(
    POST,
    "post",
    16.0,
    r#"<g stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" stroke-linecap="round" fill="none"><path d="M14.2 1.8L1.8 7L6.9 9.1L9 14.2Z"/><path d="M14.2 1.8L6.9 9.1"/></g>"#
);
icon!(
    NEW,
    "new",
    16.0,
    r#"<g stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" stroke-linecap="round" fill="none"><path d="M8.2 2.8H4.3C3.5 2.8 2.8 3.5 2.8 4.3V11.7C2.8 12.5 3.5 13.2 4.3 13.2H11.7C12.5 13.2 13.2 12.5 13.2 11.7V7.8"/><path d="M12.1 2.1C12.6 1.6 13.4 1.6 13.9 2.1C14.4 2.6 14.4 3.4 13.9 3.9L8.4 9.4L6.3 9.9L6.8 7.8Z"/></g>"#
);
icon!(
    AGENT,
    "agent",
    16.0,
    r#"<g stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" stroke-linecap="round" fill="none"><rect x="1.8" y="2.8" width="12.4" height="10.4" rx="1.8"/><path d="M4.8 6.3L6.8 7.9L4.8 9.5"/><path d="M8.3 9.6H11.2"/></g>"#
);
icon!(
    LINEAR,
    "linear",
    16.0,
    r#"<g stroke="currentColor" stroke-width="1.1" fill="none"><circle cx="8" cy="3.6" r="1.6"/><circle cx="4.2" cy="5.8" r="1.6"/><circle cx="11.8" cy="5.8" r="1.6"/><circle cx="8" cy="8" r="1.6"/><circle cx="4.2" cy="10.2" r="1.6"/><circle cx="11.8" cy="10.2" r="1.6"/><circle cx="8" cy="12.4" r="1.6"/></g>"#
);
icon!(
    SYSTEM,
    "system",
    16.0,
    r#"<circle cx="8" cy="8" r="6.3" stroke="currentColor" stroke-width="1.25" fill="none"/><path d="M8 1.7A6.3 6.3 0 0 0 8 14.3Z" fill="currentColor"/>"#
);
icon!(
    LIGHT,
    "light",
    16.0,
    r#"<g stroke="currentColor" stroke-width="1.2" stroke-linecap="round" fill="none"><circle cx="8" cy="8" r="3"/><path d="M8 1.5V3M8 13V14.5M1.5 8H3M13 8H14.5M3.4 3.4L4.5 4.5M11.5 11.5L12.6 12.6M3.4 12.6L4.5 11.5M11.5 4.5L12.6 3.4"/></g>"#
);
icon!(DARK, "dark", 16.0, r#"<path d="M13.4 10.1A5.9 5.9 0 0 1 5.9 2.6A5.9 5.9 0 1 0 13.4 10.1Z" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" fill="none"/>"#);

// Status circles: a ring for To do, half filled for In progress (both in
// the status colour); Done and Dropped are a filled circle with a white
// mark, drawn as an element under these.
icon!(RING, "ring", 14.0, r#"<circle cx="7" cy="7" r="6" stroke="currentColor" stroke-width="1.5" fill="none"/>"#);
icon!(HALF, "half", 14.0, r#"<circle cx="7" cy="7" r="6" stroke="currentColor" stroke-width="1.5" fill="none"/><path d="M7 3.5A3.5 3.5 0 0 1 7 10.5Z" fill="currentColor"/>"#);
icon!(TICK, "tick", 14.0, r#"<path d="M4.3 7.2L6.2 9L9.8 5.2" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" fill="none"/>"#);
icon!(CROSS, "cross", 14.0, r#"<path d="M5 5L9 9M9 5L5 9" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" fill="none"/>"#);

// Wheelhouse Dev's: the same wheel in blue, as a nightly recolors its mark.
icon!(
    LOGO_DEV,
    "logo-dev",
    32.0,
    r##"<g stroke="#2f7dff" fill="none"><circle cx="16" cy="16" r="11" stroke-width="3.2"/><path d="M16 5V2M25.53 10.5L28.12 9M25.53 21.5L28.12 23M16 27V30M6.47 21.5L3.88 23M6.47 10.5L3.88 9" stroke-width="2.6" stroke-linecap="round"/></g><circle cx="16" cy="16" r="3.4" fill="#2f7dff"/>"##
);

// The Logo: a wheel, in Cloudflare orange.
icon!(
    LOGO,
    "logo",
    32.0,
    r##"<g stroke="#f6821f" fill="none"><circle cx="16" cy="16" r="11" stroke-width="3.2"/><path d="M16 5V2M25.53 10.5L28.12 9M25.53 21.5L28.12 23M16 27V30M6.47 21.5L3.88 23M6.47 10.5L3.88 9" stroke-width="2.6" stroke-linecap="round"/></g><circle cx="16" cy="16" r="3.4" fill="#f6821f"/>"##
);

// The sidebar's places, as SF Symbols draw them.
icon!(
    TRAY,
    "tray",
    16.0,
    r#"<g stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" stroke-linecap="round" fill="none"><path d="M2.5 9.5L4.2 3.5H11.8L13.5 9.5V12.5H2.5Z"/><path d="M2.5 9.5H5.5L6.5 11H9.5L10.5 9.5H13.5"/></g>"#
);
icon!(
    PULL,
    "pull",
    16.0,
    r#"<g stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round" fill="none"><circle cx="4" cy="3.5" r="1.5"/><circle cx="4" cy="12.5" r="1.5"/><circle cx="12" cy="12.5" r="1.5"/><path d="M4 5V11"/><path d="M12 11V6.5C12 5.1 11 4 9.5 4H7.2"/><path d="M8.6 2.6L7.2 4L8.6 5.4"/></g>"#
);
icon!(
    HAMMER,
    "hammer",
    16.0,
    r#"<path d="M8.6 2.6L13.4 7.4L12 8.8L10.6 7.4L4.2 13.8L2.2 11.8L8.6 5.4L7.2 4Z" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" fill="none"/>"#
);
icon!(
    DASHED,
    "dashed",
    16.0,
    r#"<path d="M8.63 2.03A6 6 0 0 1 11.27 2.97M12.66 4.22A6 6 0 0 1 13.87 6.75M13.97 8.63A6 6 0 0 1 13.03 11.27M11.78 12.66A6 6 0 0 1 9.25 13.87M7.37 13.97A6 6 0 0 1 4.73 13.03M3.34 11.78A6 6 0 0 1 2.13 9.25M2.03 7.37A6 6 0 0 1 2.97 4.73M4.22 3.34A6 6 0 0 1 6.75 2.13" stroke="currentColor" stroke-width="1.25" stroke-linecap="round" fill="none"/>"#
);
icon!(
    STACK,
    "stack",
    16.0,
    r#"<g stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" stroke-linecap="round" fill="none"><rect x="3" y="5.5" width="10" height="8" rx="1.6"/><path d="M4.5 3.5H11.5"/></g>"#
);
icon!(
    CHART,
    "chart",
    16.0,
    r#"<path d="M2.5 13.5H13.5M4.5 11.5V8M7 11.5V4.5M9.5 11.5V6.5M12 11.5V9" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" fill="none"/>"#
);
icon!(
    EYE,
    "eye",
    16.0,
    r#"<g stroke="currentColor" stroke-width="1.2" fill="none"><path d="M1.8 8C3.2 5.2 5.4 3.8 8 3.8C10.6 3.8 12.8 5.2 14.2 8C12.8 10.8 10.6 12.2 8 12.2C5.4 12.2 3.2 10.8 1.8 8Z" stroke-linejoin="round"/><circle cx="8" cy="8" r="2.1"/></g>"#
);
icon!(PLAY, "play", 16.0, r#"<path d="M5 3.5V12.5L12.5 8Z" fill="currentColor"/>"#);
icon!(PAUSE, "pause", 16.0, r#"<path d="M5 3.5V12.5M11 3.5V12.5" stroke="currentColor" stroke-width="2" stroke-linecap="round"/>"#);
icon!(
    REFRESH,
    "refresh",
    16.0,
    r#"<g stroke="currentColor" stroke-width="1.25" stroke-linecap="round" stroke-linejoin="round" fill="none"><path d="M12.8 8A4.8 4.8 0 1 1 11.4 4.6"/><path d="M11.6 2V4.8H8.8"/></g>"#
);
icon!(CHEVRON, "chevron", 16.0, r#"<path d="M5 6.5L8 9.5L11 6.5" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" fill="none"/>"#);
icon!(
    CHEVRON_RIGHT,
    "chevron-right",
    16.0,
    r#"<path d="M6.5 5L9.5 8L6.5 11" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" fill="none"/>"#
);
icon!(
    SHIELD,
    "shield",
    16.0,
    r#"<g stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" stroke-linecap="round" fill="none"><path d="M8 2L13 3.8V8C13 11 10.8 12.8 8 14C5.2 12.8 3 11 3 8V3.8Z"/><path d="M5.8 8.2L7.3 9.7L10.3 6.5"/></g>"#
);
icon!(
    DOC,
    "doc",
    16.0,
    r#"<g stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" stroke-linecap="round" fill="none"><path d="M4 2.5H9.5L12 5V13.5H4Z"/><path d="M9.5 2.5V5H12M6 8H10M6 10.5H10"/></g>"#
);
icon!(
    LINK,
    "link",
    16.0,
    r#"<g stroke="currentColor" stroke-width="1.3" stroke-linecap="round" fill="none"><path d="M7 9L9 7"/><path d="M6.2 10.8L5.4 11.6C4.5 12.5 3.2 12.5 2.4 11.6C1.5 10.8 1.5 9.5 2.4 8.6L4.6 6.4C5.5 5.5 6.8 5.5 7.6 6.4"/><path d="M9.8 5.2L10.6 4.4C11.5 3.5 12.8 3.5 13.6 4.4C14.5 5.2 14.5 6.5 13.6 7.4L11.4 9.6C10.5 10.5 9.2 10.5 8.4 9.6"/></g>"#
);
icon!(
    GEAR,
    "gear",
    16.0,
    r#"<g stroke="currentColor" stroke-width="1.2" fill="none" stroke-linecap="round"><circle cx="8" cy="8" r="2.2"/><circle cx="8" cy="8" r="4.6"/><path d="M8 1.6V3.4M8 12.6V14.4M1.6 8H3.4M12.6 8H14.4M3.5 3.5L4.7 4.7M11.3 11.3L12.5 12.5M3.5 12.5L4.7 11.3M11.3 4.7L12.5 3.5"/></g>"#
);
icon!(INSPECTOR, "inspector", 16.0, r#"<g stroke="currentColor" stroke-width="1.2" fill="none"><rect x="2" y="3" width="12" height="10" rx="2"/><path d="M10 3V13"/></g>"#);
icon!(
    LIST,
    "list",
    16.0,
    r#"<path d="M5.5 4.5H13M5.5 8H13M5.5 11.5H13M2.8 4.5H3M2.8 8H3M2.8 11.5H3" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" fill="none"/>"#
);
icon!(
    MOON,
    "moon-zz",
    16.0,
    r#"<path d="M13.4 10.1A5.9 5.9 0 0 1 5.9 2.6A5.9 5.9 0 1 0 13.4 10.1Z" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" fill="none"/>"#
);
icon!(CLOCK, "clock", 16.0, r#"<g stroke="currentColor" stroke-width="1.2" fill="none" stroke-linecap="round"><circle cx="8" cy="8" r="6"/><path d="M8 4.8V8L10.2 9.4"/></g>"#);
icon!(QUOTE, "quote", 16.0, r#"<path d="M3 3.5H13V10.5H8L5 13V10.5H3Z" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" fill="none"/>"#);
icon!(
    PENCIL,
    "pencil",
    16.0,
    r#"<path d="M10.8 2.7C11.4 2.1 12.3 2.1 12.9 2.7L13.3 3.1C13.9 3.7 13.9 4.6 13.3 5.2L6.2 12.3L3 13L3.7 9.8Z" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" fill="none"/>"#
);
icon!(
    BANG,
    "bang",
    14.0,
    r#"<path d="M7 4V7.6" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" fill="none"/><circle cx="7" cy="10" r="0.95" fill="currentColor"/>"#
);
icon!(
    LOGO_WHITE,
    "logo-white",
    32.0,
    r##"<g stroke="#ffffff" fill="none"><circle cx="16" cy="16" r="11" stroke-width="3.2"/><path d="M16 5V2M25.53 10.5L28.12 9M25.53 21.5L28.12 23M16 27V30M6.47 21.5L3.88 23M6.47 10.5L3.88 9" stroke-width="2.6" stroke-linecap="round"/></g><circle cx="16" cy="16" r="3.4" fill="#ffffff"/>"##
);
// square.and.pencil: a new docket.
icon!(
    COMPOSE,
    "compose",
    16.0,
    r#"<g stroke="currentColor" stroke-width="1.2" fill="none" stroke-linecap="round" stroke-linejoin="round"><path d="M8 3H4.2C3.5 3 3 3.5 3 4.2V11.8C3 12.5 3.5 13 4.2 13H11.8C12.5 13 13 12.5 13 11.8V8"/><path d="M11.6 2.4C12.1 1.9 12.9 1.9 13.4 2.4L13.6 2.6C14.1 3.1 14.1 3.9 13.6 4.4L8.6 9.4L6.6 9.9L7.1 7.9Z"/></g>"#
);
// terminal: a pane in Rex.
icon!(
    TERMINAL,
    "terminal",
    16.0,
    r#"<g stroke="currentColor" stroke-width="1.2" fill="none" stroke-linecap="round" stroke-linejoin="round"><rect x="2" y="3" width="12" height="10" rx="2"/><path d="M4.8 6.3L6.8 8L4.8 9.7M8.3 10H11"/></g>"#
);
// ellipsis.circle: a docket's menu.
icon!(
    MORE,
    "more",
    16.0,
    r#"<g fill="none"><circle cx="8" cy="8" r="6" stroke="currentColor" stroke-width="1.2"/><circle cx="5.3" cy="8" r="0.9" fill="currentColor"/><circle cx="8" cy="8" r="0.9" fill="currentColor"/><circle cx="10.7" cy="8" r="0.9" fill="currentColor"/></g>"#
);
