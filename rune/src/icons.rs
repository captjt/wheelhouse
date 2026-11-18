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
