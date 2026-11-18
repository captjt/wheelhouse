//! The SF Symbols Recap, Chat, the record, Verify and the panels draw that
//! the app's own set (`crate::icons`) doesn't have, redrawn as SVG in the
//! same 16 pt box. Filled marks keep their white details, so `.color(..)`
//! tints only the fill.

use rune::Icon;

macro_rules! glyph {
    ($id:ident, $name:literal, $body:expr) => {
        pub static $id: Icon = Icon { name: $name, view: 16.0, body: $body };
    };
}

// doc.on.clipboard
glyph!(
    CLIPBOARD,
    "wh-clipboard",
    r#"<g stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" stroke-linecap="round" fill="none"><path d="M5.5 3H4.2C3.5 3 3 3.5 3 4.2V13.3C3 14 3.5 14.5 4.2 14.5H11.8C12.5 14.5 13 14 13 13.3V4.2C13 3.5 12.5 3 11.8 3H10.5"/><rect x="5.5" y="1.8" width="5" height="2.6" rx="0.9"/><path d="M5.6 8H10.4M5.6 10.6H9"/></g>"#
);
// checkmark.seal.fill
glyph!(
    SEAL,
    "wh-seal",
    r##"<path d="M8 1.2L9.5 2.4L11.4 2.2L12.2 3.9L13.9 4.7L13.7 6.6L14.8 8L13.7 9.4L13.9 11.3L12.2 12.1L11.4 13.8L9.5 13.6L8 14.8L6.5 13.6L4.6 13.8L3.8 12.1L2.1 11.3L2.3 9.4L1.2 8L2.3 6.6L2.1 4.7L3.8 3.9L4.6 2.2L6.5 2.4Z" fill="currentColor"/><path d="M5.4 8.2L7.2 10L10.7 6.2" stroke="#ffffff" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" fill="none"/>"##
);
// checkmark.circle.fill
glyph!(
    CHECK_FILL,
    "wh-check-fill",
    r##"<circle cx="8" cy="8" r="6.8" fill="currentColor"/><path d="M5.2 8.2L7.1 10.1L10.9 5.9" stroke="#ffffff" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" fill="none"/>"##
);
// xmark.circle.fill
glyph!(
    CROSS_FILL,
    "wh-cross-fill",
    r##"<circle cx="8" cy="8" r="6.8" fill="currentColor"/><path d="M5.8 5.8L10.2 10.2M10.2 5.8L5.8 10.2" stroke="#ffffff" stroke-width="1.5" stroke-linecap="round" fill="none"/>"##
);
// clock.badge.exclamationmark
glyph!(
    CLOCK_BANG,
    "wh-clock-bang",
    r#"<g stroke="currentColor" stroke-width="1.2" stroke-linecap="round" fill="none"><path d="M11.6 9.2A5.2 5.2 0 1 1 9.2 3"/><path d="M6.6 5V8L8.6 9.2"/><path d="M13 2.4V6"/></g><circle cx="13" cy="8.2" r="0.9" fill="currentColor"/>"#
);
// text.bubble
glyph!(
    TEXT_BUBBLE,
    "wh-text-bubble",
    r#"<g stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" stroke-linecap="round" fill="none"><path d="M3.8 2.8H12.2C13 2.8 13.6 3.4 13.6 4.2V9.6C13.6 10.4 13 11 12.2 11H7.4L4.6 13.4V11H3.8C3 11 2.4 10.4 2.4 9.6V4.2C2.4 3.4 3 2.8 3.8 2.8Z"/><path d="M5.2 5.8H10.8M5.2 8.2H9"/></g>"#
);
// quote.bubble
glyph!(
    QUOTE_BUBBLE,
    "wh-quote-bubble",
    r#"<path d="M3.8 2.8H12.2C13 2.8 13.6 3.4 13.6 4.2V9.6C13.6 10.4 13 11 12.2 11H7.4L4.6 13.4V11H3.8C3 11 2.4 10.4 2.4 9.6V4.2C2.4 3.4 3 2.8 3.8 2.8Z" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" fill="none"/><path d="M5.6 5.4V7.4M7.4 5.4V7.4M8.8 5.4V7.4M10.6 5.4V7.4" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/>"#
);
// hand.thumbsup
glyph!(
    THUMBSUP,
    "wh-thumbsup",
    r#"<g stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" stroke-linecap="round" fill="none"><path d="M5 7.2V13.4H2.6V7.2Z"/><path d="M5 7.2L7.6 2.6C8.5 2.6 9.2 3.3 9.1 4.2L8.8 6.2H12.4C13.2 6.2 13.8 7 13.6 7.8L12.6 12.3C12.4 13 11.8 13.4 11.1 13.4H5"/></g>"#
);
// exclamationmark.bubble
glyph!(
    BUBBLE_BANG,
    "wh-bubble-bang",
    r#"<path d="M3.8 2.8H12.2C13 2.8 13.6 3.4 13.6 4.2V9.6C13.6 10.4 13 11 12.2 11H7.4L4.6 13.4V11H3.8C3 11 2.4 10.4 2.4 9.6V4.2C2.4 3.4 3 2.8 3.8 2.8Z" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" fill="none"/><path d="M8 4.8V7.4" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/><circle cx="8" cy="9.2" r="0.8" fill="currentColor"/>"#
);
// person.2
glyph!(
    PEOPLE,
    "wh-people",
    r#"<g stroke="currentColor" stroke-width="1.2" stroke-linecap="round" fill="none"><circle cx="6" cy="5.6" r="2.2"/><path d="M1.8 13C2.3 10.8 4 9.6 6 9.6C8 9.6 9.7 10.8 10.2 13"/><path d="M10.2 3.6A2.2 2.2 0 1 1 10.6 7.8"/><path d="M11.4 9.7C12.9 10 14 11.2 14.3 13"/></g>"#
);
// person.fill.questionmark
glyph!(
    PERSON_ASK,
    "wh-person-ask",
    r#"<circle cx="6.4" cy="5.4" r="2.6" fill="currentColor"/><path d="M1.6 13.6C2.1 11 4 9.6 6.4 9.6C8.8 9.6 10.7 11 11.2 13.6Z" fill="currentColor"/><path d="M11.6 3.6C11.8 2.8 12.4 2.4 13.1 2.4C13.9 2.4 14.5 3 14.5 3.7C14.5 4.8 13.1 4.9 13.1 6" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" fill="none"/><circle cx="13.1" cy="7.7" r="0.7" fill="currentColor"/>"#
);
// checkmark.shield.fill
glyph!(
    SHIELD_OK,
    "wh-shield-ok",
    r##"<path d="M8 1.4L13.6 3.4V8C13.6 11.2 11.2 13.3 8 14.6C4.8 13.3 2.4 11.2 2.4 8V3.4Z" fill="currentColor"/><path d="M5.5 8.1L7.2 9.8L10.5 6.3" stroke="#ffffff" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" fill="none"/>"##
);
// exclamationmark.shield.fill
glyph!(
    SHIELD_BAD,
    "wh-shield-bad",
    r##"<path d="M8 1.4L13.6 3.4V8C13.6 11.2 11.2 13.3 8 14.6C4.8 13.3 2.4 11.2 2.4 8V3.4Z" fill="currentColor"/><path d="M8 4.8V8.4" stroke="#ffffff" stroke-width="1.6" stroke-linecap="round"/><circle cx="8" cy="10.8" r="0.95" fill="#ffffff"/>"##
);
// pause.circle.fill
glyph!(
    PAUSE_FILL,
    "wh-pause-fill",
    r##"<circle cx="8" cy="8" r="6.8" fill="currentColor"/><path d="M6.4 5.4V10.6M9.6 5.4V10.6" stroke="#ffffff" stroke-width="1.6" stroke-linecap="round"/>"##
);
// clock.arrow.circlepath
glyph!(
    CLOCK_ARROW,
    "wh-clock-arrow",
    r#"<g stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" fill="none"><path d="M3.2 8A4.8 4.8 0 1 0 4.6 4.6"/><path d="M4.4 2V4.8H7.2"/><path d="M8 5.4V8.2L9.8 9.4"/></g>"#
);
// checklist
glyph!(
    CHECKLIST,
    "wh-checklist",
    r#"<g stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round" fill="none"><path d="M2 4.2L3.2 5.4L5.2 3.2M2 9.6L3.2 10.8L5.2 8.6"/><path d="M7.4 4.4H14M7.4 9.8H14"/></g>"#
);
// info.circle
glyph!(
    INFO,
    "wh-info",
    r#"<circle cx="8" cy="8" r="6.4" stroke="currentColor" stroke-width="1.2" fill="none"/><path d="M8 7.2V11.4" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/><circle cx="8" cy="5" r="0.85" fill="currentColor"/>"#
);
// exclamationmark.triangle
glyph!(
    WARN,
    "wh-warn",
    r#"<path d="M8 2.2L14.4 13.4H1.6Z" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" fill="none"/><path d="M8 6.2V9.4" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/><circle cx="8" cy="11.3" r="0.8" fill="currentColor"/>"#
);
// arrow.up, bold: the Chat's send
glyph!(
    ARROW_UP,
    "wh-arrow-up",
    r#"<path d="M8 13.2V3.2M3.6 7.4L8 3L12.4 7.4" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" fill="none"/>"#
);
// square.and.arrow.down
glyph!(
    EXPORT,
    "wh-export",
    r#"<g stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round" fill="none"><path d="M5.4 6.2H4C3.4 6.2 3 6.6 3 7.2V13C3 13.6 3.4 14 4 14H12C12.6 14 13 13.6 13 13V7.2C13 6.6 12.6 6.2 12 6.2H10.6"/><path d="M8 1.8V9.8M5.4 7.4L8 10L10.6 7.4"/></g>"#
);
// arrow.up.right.square
glyph!(
    OPEN_SQUARE,
    "wh-open-square",
    r#"<g stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round" fill="none"><rect x="2.4" y="2.4" width="11.2" height="11.2" rx="2.2"/><path d="M6.2 9.8L10 6M6.8 6H10V9.2"/></g>"#
);
