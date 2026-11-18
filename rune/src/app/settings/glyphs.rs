//! Settings' own marks, redrawn as SVG after the SF Symbols the SwiftUI
//! window uses (Rune can't draw SF Symbols): its tabs, and its controls'.

use rune::Icon;

macro_rules! glyph {
    ($id:ident, $name:literal, $body:expr) => {
        pub static $id: Icon = Icon { name: $name, view: 16.0, body: $body };
    };
}

// The tabs: gearshape comes from crate::icons::GEAR.
glyph!(
    CLIPBOARD,
    "settings-clipboard",
    r#"<g stroke="currentColor" stroke-width="1.15" stroke-linecap="round" stroke-linejoin="round" fill="none"><path d="M5.4 2.9H4.4C3.6 2.9 3 3.5 3 4.3V13.1C3 13.9 3.6 14.5 4.4 14.5H11.6C12.4 14.5 13 13.9 13 13.1V4.3C13 3.5 12.4 2.9 11.6 2.9H10.6"/><rect x="5.4" y="1.6" width="5.2" height="2.6" rx="1"/><path d="M7.2 7.3H10.8M7.2 9.7H10.8M7.2 12.1H10.8"/><path d="M5.2 7.3H5.3M5.2 9.7H5.3M5.2 12.1H5.3"/></g>"#
);
glyph!(
    PEOPLE,
    "settings-people",
    r#"<g stroke="currentColor" stroke-width="1.15" stroke-linecap="round" fill="none"><circle cx="5.9" cy="5.5" r="2.3"/><path d="M1.7 13.3C2 10.9 3.7 9.4 5.9 9.4C8.1 9.4 9.8 10.9 10.1 13.3"/><circle cx="11.2" cy="5.3" r="1.9"/><path d="M10.7 8.9C12.6 8.8 14.1 10.1 14.4 12.3"/></g>"#
);
glyph!(
    FOLDER,
    "settings-folder",
    r#"<path d="M1.8 4.4C1.8 3.6 2.4 3 3.2 3H6.1L7.6 4.6H12.8C13.6 4.6 14.2 5.2 14.2 6V12C14.2 12.8 13.6 13.4 12.8 13.4H3.2C2.4 13.4 1.8 12.8 1.8 12ZM1.8 6.5H14.2" stroke="currentColor" stroke-width="1.15" stroke-linejoin="round" fill="none"/>"#
);
glyph!(
    BOOK,
    "settings-book",
    r#"<g stroke="currentColor" stroke-width="1.15" stroke-linecap="round" stroke-linejoin="round" fill="none"><path d="M3.2 12.8V3.3C3.2 2.5 3.8 1.9 4.6 1.9H12.8V11.9H4.6C3.8 11.9 3.2 12.5 3.2 13.3C3.2 14.1 3.8 14.6 4.6 14.6H12.8"/><path d="M6 4.8H10M6 7H10"/></g>"#
);
glyph!(
    BELL,
    "settings-bell",
    r#"<g stroke="currentColor" stroke-width="1.15" stroke-linecap="round" stroke-linejoin="round" fill="none"><path d="M8.6 2.3C8.4 2.2 8.2 2.2 8 2.2C5.8 2.2 4.3 3.9 4.3 6V8.8L3 11.2H13L11.7 8.8V7.2"/><path d="M6.5 12.8C6.8 13.6 7.3 14 8 14C8.7 14 9.2 13.6 9.5 12.8"/></g><circle cx="11.8" cy="3.8" r="2" fill="currentColor"/>"#
);
glyph!(
    BOLT,
    "settings-bolt",
    r#"<g stroke="currentColor" stroke-width="1.15" stroke-linejoin="round" fill="none"><circle cx="8" cy="8" r="6.3"/><path d="M8.9 3.9L5.3 8.7H8L7.1 12.1L10.7 7.3H8Z"/></g>"#
);

// The controls.
glyph!(PLUS, "settings-plus", r#"<path d="M8 3.5V12.5M3.5 8H12.5" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" fill="none"/>"#);
glyph!(MINUS, "settings-minus", r#"<path d="M3.5 8H12.5" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" fill="none"/>"#);
glyph!(UP, "settings-up", r#"<path d="M4.5 10L8 6.5L11.5 10" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" fill="none"/>"#);
glyph!(DOWN, "settings-down", r#"<path d="M4.5 6L8 9.5L11.5 6" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" fill="none"/>"#);
glyph!(
    UPDOWN,
    "settings-updown",
    r#"<path d="M5.2 6.2L8 3.4L10.8 6.2M5.2 9.8L8 12.6L10.8 9.8" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" fill="none"/>"#
);
glyph!(
    WARN,
    "settings-warn",
    r#"<path fill-rule="evenodd" fill="currentColor" d="M7.1 2.6C7.5 1.9 8.5 1.9 8.9 2.6L14.8 12.9C15.2 13.6 14.7 14.4 13.9 14.4H2.1C1.3 14.4 0.8 13.6 1.2 12.9ZM7.3 6H8.7L8.5 10.1H7.5ZM8 11.1A0.85 0.85 0 1 0 8 12.8A0.85 0.85 0 1 0 8 11.1Z"/>"#
);
glyph!(
    CURLY,
    "settings-curly",
    r#"<path d="M5.6 2.5C4.4 2.5 4 3.1 4 4.2V6.4C4 7.3 3.5 8 2.6 8C3.5 8 4 8.7 4 9.6V11.8C4 12.9 4.4 13.5 5.6 13.5M10.4 2.5C11.6 2.5 12 3.1 12 4.2V6.4C12 7.3 12.5 8 13.4 8C12.5 8 12 8.7 12 9.6V11.8C12 12.9 11.6 13.5 10.4 13.5" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round" fill="none"/>"#
);
