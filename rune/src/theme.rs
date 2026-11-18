//! Wheelhouse's look (Theme.swift): Cloudflare's Kumo tokens, as the web app
//! uses them, light and dark; SF Pro, as the system draws it; and the small
//! marks every screen shares: a docket's status, an agent's state, avatars,
//! chips and callouts.

use rune::tok::{Family, TextStyle};
use rune::{Color, El, Icon, el, icon, row, text};
use rune_kit::{Kit, PaletteSeed};

use crate::icons;
use crate::model::{LiveState, Status};

pub const SYS: Family = Family::Named("System Font");

/// A system-font style: `font(13.0, 400.0)`.
pub const fn font(size: f32, weight: f32) -> TextStyle {
    TextStyle::new(size, weight).family(SYS)
}
pub const fn mono(size: f32) -> TextStyle {
    TextStyle::new(size, 400.0).family(Family::SystemMono)
}

pub fn hex(h: &str) -> Color {
    rune::color::hex(h).unwrap_or(Color::WHITE)
}

/// Kumo's tokens for one appearance.
#[derive(Clone, Copy)]
pub struct K {
    pub dark: bool,
    pub main: Color,
    pub elev: Color,
    pub recessed: Color,
    pub hover: Color,
    pub sel: Color,
    pub line: Color,
    pub line2: Color,
    pub text: Color,
    pub text2: Color,
    pub muted: Color,
    pub faint: Color,
    pub brand: Color,
    pub brand2: Color,
    pub accent: Color,
    pub accent_soft: Color,
    pub link: Color,
    pub working: Color,
    pub blocked: Color,
    pub error: Color,
    pub progress: Color,
    pub you: Color,
    pub success_tint: Color,
    pub warning_tint: Color,
    pub danger_tint: Color,
    pub claude: Color,
    pub codex: Color,
}

impl K {
    pub fn new(dark: bool, dev: bool) -> K {
        let p = |l: &str, d: &str| hex(if dark { d } else { l });
        let pa = |l: &str, la: f32, d: &str, da: f32| if dark { hex(d).alpha(da) } else { hex(l).alpha(la) };
        K {
            dark,
            main: p("#ffffff", "#111111"),
            elev: p("#ffffff", "#171717"),
            recessed: p("#f5f5f5", "#0b0b0b"),
            hover: p("#f3f3f3", "#1a1a1a"),
            sel: p("#ededed", "#1e1e1e"),
            line: p("#e9e9e9", "#232323"),
            line2: p("#d9d9d9", "#333333"),
            text: p("#18181b", "#f5f5f5"),
            text2: p("#3d3d3d", "#d1d1d1"),
            muted: p("#737373", "#a1a1a1"),
            faint: p("#a1a1a1", "#737373"),
            // Wheelhouse Dev's brand is blue, as a nightly recolors its mark.
            brand: hex(if dev { "#2f7dff" } else { "#f6821f" }),
            brand2: hex(if dev { "#8fb8ff" } else { "#fbad41" }),
            accent: p("#056dff", "#2f7dff"),
            accent_soft: pa("#dbeafe", 0.65, "#1c398e", 0.28),
            link: p("#193cb8", "#51a2ff"),
            working: p("#009966", "#00d492"),
            blocked: p("#db6809", "#ff8904"),
            error: p("#e7000b", "#ff6467"),
            progress: p("#e39f08", "#f2bb3b"),
            you: hex("#155dfc"),
            success_tint: pa("#dcfce7", 0.8, "#0c542b", 0.22),
            warning_tint: pa("#ffebd4", 0.85, "#573100", 0.37),
            danger_tint: pa("#ffe2e2", 0.7, "#990003", 0.2),
            claude: hex("#d97757"),
            codex: hex("#3b3d44"),
        }
    }

    pub fn live(&self, s: Option<LiveState>) -> Color {
        match s {
            Some(LiveState::Working) => self.working,
            Some(LiveState::Blocked) => self.blocked,
            Some(LiveState::Error) => self.error,
            Some(LiveState::Done) => self.accent,
            _ => self.faint,
        }
    }

    pub fn status(&self, s: Status) -> Color {
        match s {
            Status::InProgress => self.progress,
            Status::Done => self.accent,
            _ => self.faint,
        }
    }

    pub fn risk(&self, level: Option<&str>) -> Color {
        match level.map(str::to_lowercase).as_deref() {
            Some("low") => self.working,
            Some("medium") => self.progress,
            Some("high" | "critical") => self.error,
            _ => self.muted,
        }
    }

    /// rune-kit's components, in these colours.
    pub fn kit(&self) -> Kit {
        let mut k = Kit::from_palette(PaletteSeed { bg: self.main, fg: self.text, accent: self.accent });
        k.c.text_muted = self.muted;
        k.c.text_faint = self.faint;
        k
    }
}

// ---- marks ----

/// A docket's status, as Linear draws it.
pub fn status_icon(s: Status, size: f32, k: &K) -> El {
    let c = k.status(s);
    match s {
        Status::Todo => icon(&icons::RING, size).color(c),
        Status::InProgress => icon(&icons::HALF, size).color(c),
        Status::Done => el().size(size - 1.0, size - 1.0).radius((size - 1.0) / 2.0).fill(c).center().child(icon(&icons::TICK, size - 1.0).color(Color::WHITE)),
        Status::Dropped => el().size(size - 1.0, size - 1.0).radius((size - 1.0) / 2.0).fill(c).center().child(icon(&icons::CROSS, size - 1.0).color(Color::WHITE)),
    }
    .label(s.label())
}

/// What an agent is doing: a pulse while it works, a mark when it needs you.
pub fn live_icon(s: Option<LiveState>, size: f32, k: &K) -> El {
    let c = k.live(s);
    let mark =
        |i: &'static Icon| el().size(size * 12.0 / 14.0, size * 12.0 / 14.0).radius(size * 3.5 / 14.0).fill(c).center().child(icon(i, size * 12.0 / 14.0).color(Color::WHITE));
    let inner = match s {
        None => el().size(size, size),
        Some(LiveState::Blocked) => mark(&icons::BANG),
        Some(LiveState::Error) => mark(&icons::CROSS),
        Some(LiveState::Working) => {
            el().size(size, size).center().child(el().size(size * 0.5, size * 0.5).radius(size * 0.25).fill(c).looping(rune::LoopSpec::opacity(1.4, 0.0, 0.45, 1.0)))
        }
        _ => el().size(size * 0.44, size * 0.44).radius(size * 0.22).fill(c),
    };
    let e = row().size(size, size).center().child(inner);
    match s {
        Some(st) => e.label(st.word()),
        None => e.a11y_hidden(),
    }
}

fn letter_box(letter: &str, fill: Color, size: f32, round: bool) -> El {
    row()
        .size(size, size)
        .radius(if round { size / 2.0 } else { size * 0.28 })
        .continuous_corners()
        .fill(fill)
        .center()
        .child(text(letter.to_string()).font(font(size * 0.55, 600.0).lh(size)).color(Color::WHITE))
}

/// An agent's app: Claude Code's coral C, Codex's slate X.
pub fn app_avatar(app: Option<&str>, size: f32, k: &K) -> El {
    let (l, f) = match app {
        Some("codex") => ("X", k.codex),
        Some("claude-code" | "claude") => ("C", k.claude),
        _ => ("A", k.faint),
    };
    letter_box(l, f, size, false).label(crate::model::app_name(app))
}

pub fn person_avatar(name: &str, size: f32, k: &K) -> El {
    letter_box(&name.chars().next().unwrap_or('?').to_uppercase().to_string(), k.you, size, true).label(name.to_string())
}

/// Wheelhouse itself: the wheel on the brand's gradient.
pub fn hub_avatar(size: f32, k: &K) -> El {
    row().size(size, size).radius(size * 0.28).continuous_corners().gradient(k.brand2, k.brand).center().child(icon(&icons::LOGO_WHITE, size * 0.62)).label("Wheelhouse")
}

/// A colour from hue, saturation and brightness, as SwiftUI's `Color(hue:…)`.
pub fn hsb(h: f32, s: f32, b: f32) -> Color {
    let i = (h * 6.0).floor();
    let f = h * 6.0 - i;
    let (p, q, t) = (b * (1.0 - s), b * (1.0 - f * s), b * (1.0 - (1.0 - f) * s));
    let (r, g, bl) = match (i as i32).rem_euclid(6) {
        0 => (b, t, p),
        1 => (q, b, p),
        2 => (p, b, t),
        3 => (p, q, b),
        4 => (t, p, b),
        _ => (b, p, q),
    };
    Color { r, g, b: bl, a: 1.0 }
}

/// A Rex session: its first letter, in a hue of its own (SessionIcon).
pub fn session_icon(name: &str, size: f32, k: &K) -> El {
    let mut h: i64 = 0;
    for c in name.chars() {
        h = (h * 31 + c as i64) % 360;
    }
    let hue = ((h * 7 + 200) % 360) as f32 / 360.0;
    let letter = name.strip_prefix("rex").map(|r| r.trim_start_matches('-')).filter(|r| !r.is_empty()).unwrap_or(name).chars().next().unwrap_or('?').to_uppercase().to_string();
    let (fill, edge, ink) = if k.dark {
        (hsb(hue, 0.45, 0.42).alpha(0.75), hsb(hue, 0.55, 0.7).alpha(0.5), hsb(hue, 0.45, 0.98))
    } else {
        (hsb(hue, 0.75, 0.97), hsb(hue, 0.55, 0.85).alpha(0.5), hsb(hue, 0.75, 0.52))
    };
    row()
        .size(size, size)
        .radius(4.0)
        .continuous_corners()
        .fill(fill)
        .border(1.0, edge)
        .center()
        .label(name.to_string())
        .child(text(letter).font(font(size * 0.62, 600.0).lh(size)).color(ink))
}

/// A docket id, in the list's quiet monospace.
pub fn id_text(id: &str, k: &K) -> El {
    text(id.to_string()).font(mono(11.5).lh(14.0)).color(k.muted)
}

/// A rounded tag: a verdict, a risk, a phase.
pub fn chip(t: &str, c: Color, filled: bool) -> El {
    row().px(6.0).py(1.5).radius(999.0).fill(if filled { c } else { c.alpha(0.12) }).child(text(t.to_string()).font(font(11.0, 500.0).lh(14.0)).color(if filled {
        Color::WHITE
    } else {
        c
    }))
}

/// A chip with a mark: the docket's status, or what its agent does.
pub fn mark_chip(mark: El, t: &str, c: Color, k: &K) -> El {
    row().gap(5.0).pl(5.0).pr(7.0).py(2.0).radius(999.0).border(1.0, k.line2).center_y().child(mark).child(text(t.to_string()).font(font(11.5, 500.0).lh(14.0)).color(c))
}

/// A status line: tinted by what it's about.
pub fn callout(tone: LiveState, content: El, k: &K) -> El {
    let (tint, edge) = if tone == LiveState::Error { (k.danger_tint, k.error) } else { (k.warning_tint, k.blocked) };
    row()
        .w_full()
        .gap(10.0)
        .padding(12.0)
        .radius(10.0)
        .continuous_corners()
        .fill(tint)
        .border(1.0, edge.alpha(0.25))
        .child(el().pt(1.0).child(live_icon(Some(tone), 16.0, k)))
        .child(content.grow(1.0).min_w(0.0))
}

/// A section's heading in a docket: "Intent", "Activity".
pub fn section_title(t: &str, k: &K) -> El {
    text(t.to_string()).font(font(13.0, 600.0).lh(16.0)).color(k.text)
}

pub fn hairline(k: &K) -> El {
    el().h(1.0).w_full().fill(k.line).a11y_hidden()
}
