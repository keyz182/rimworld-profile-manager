//! Colours and widget styles (T24). One token set for light and one for dark; the OS
//! setting picks which (C: theme follows OS only). Every text colour meets WCAG AA on the
//! surfaces it is drawn on (tested below).

use std::sync::LazyLock;

use iced::theme::Palette;
use iced::widget::{button, container};
use iced::{Background, Border, Color, Shadow, Theme, Vector, color};

/// The colour tokens of one theme.
#[derive(Debug, Clone, Copy)]
pub struct Tokens {
    /// Window background.
    pub bg: Color,
    /// Panels on the background (detail pane).
    pub surface: Color,
    /// Things above a surface (dialogs, tooltips).
    pub raised: Color,
    /// Decorative separators.
    pub line: Color,
    /// Edges that mark a control (outlined buttons, text fields): ≥ 3:1 on bg.
    pub outline: Color,
    pub text: Color,
    /// Secondary text (captions, paths).
    pub muted: Color,
    /// Main actions and selection.
    pub accent: Color,
    /// Text on `accent`.
    pub on_accent: Color,
    /// Selected row background.
    pub accent_soft: Color,
    pub success: Color,
    pub warning: Color,
    pub danger: Color,
    /// Text on `danger`.
    pub on_danger: Color,
}

pub const LIGHT: Tokens = Tokens {
    bg: color!(0xf3f5f6),
    surface: color!(0xffffff),
    raised: color!(0xffffff),
    line: color!(0xdde3e6),
    outline: color!(0x7d8a91),
    text: color!(0x1b2328),
    muted: color!(0x55626a),
    accent: color!(0x0b6e82),
    on_accent: color!(0xffffff),
    accent_soft: color!(0xdceef2),
    success: color!(0x1f7a3a),
    warning: color!(0x8f5300),
    danger: color!(0xb42318),
    on_danger: color!(0xffffff),
};

pub const DARK: Tokens = Tokens {
    bg: color!(0x12181b),
    surface: color!(0x1a2226),
    raised: color!(0x232d32),
    line: color!(0x2e3a40),
    outline: color!(0x75868f),
    text: color!(0xe4eaed),
    muted: color!(0x9eadb5),
    accent: color!(0x5cc3d6),
    on_accent: color!(0x0a1e23),
    accent_soft: color!(0x163a42),
    success: color!(0x63d18e),
    warning: color!(0xf0b45b),
    danger: color!(0xf28b80),
    on_danger: color!(0x24100d),
};

fn build(name: &'static str, t: &Tokens) -> Theme {
    Theme::custom(
        name,
        Palette {
            background: t.bg,
            text: t.text,
            primary: t.accent,
            success: t.success,
            warning: t.warning,
            danger: t.danger,
        },
    )
}

static LIGHT_THEME: LazyLock<Theme> = LazyLock::new(|| build("RWPM Light", &LIGHT));
static DARK_THEME: LazyLock<Theme> = LazyLock::new(|| build("RWPM Dark", &DARK));

/// The app theme for the OS setting; built once, cheap to clone.
pub fn theme(dark: bool) -> Theme {
    let theme: &Theme = if dark { &DARK_THEME } else { &LIGHT_THEME };
    theme.clone()
}

/// The tokens behind a theme made by [`theme`].
pub fn tokens(theme: &Theme) -> &'static Tokens {
    if theme.extended_palette().is_dark {
        &DARK
    } else {
        &LIGHT
    }
}

const RADIUS: f32 = 6.0;

fn faded(c: Color) -> Color {
    Color { a: c.a * 0.45, ..c }
}

fn button_style(
    bg: Option<Color>,
    fg: Color,
    edge: Option<Color>,
    status: button::Status,
) -> button::Style {
    let (bg, fg, edge) = match status {
        button::Status::Disabled => (bg.map(faded), faded(fg), edge.map(faded)),
        _ => (bg, fg, edge),
    };
    button::Style {
        background: bg.map(Background::Color),
        text_color: fg,
        border: Border {
            color: edge.unwrap_or(Color::TRANSPARENT),
            width: if edge.is_some() { 1.0 } else { 0.0 },
            radius: RADIUS.into(),
        },
        shadow: Shadow::default(),
        snap: true,
    }
}

fn hover(c: Color, status: button::Status) -> Color {
    match status {
        button::Status::Hovered => Color { a: 0.85, ..c },
        button::Status::Pressed => Color { a: 0.7, ..c },
        _ => c,
    }
}

/// The one main action of a view (Play, a dialog's main button).
pub fn primary(theme: &Theme, status: button::Status) -> button::Style {
    let t = tokens(theme);
    button_style(Some(hover(t.accent, status)), t.on_accent, None, status)
}

/// Every other action: outlined.
pub fn secondary(theme: &Theme, status: button::Status) -> button::Style {
    let t = tokens(theme);
    let bg = matches!(status, button::Status::Hovered | button::Status::Pressed)
        .then_some(t.accent_soft);
    button_style(bg, t.text, Some(t.outline), status)
}

/// Quiet actions inside text (links, icon buttons).
pub fn ghost(theme: &Theme, status: button::Status) -> button::Style {
    let t = tokens(theme);
    let bg = matches!(status, button::Status::Hovered | button::Status::Pressed)
        .then_some(t.accent_soft);
    button_style(bg, t.accent, None, status)
}

/// A destructive confirm (Delete it).
pub fn danger(theme: &Theme, status: button::Status) -> button::Style {
    let t = tokens(theme);
    button_style(Some(hover(t.danger, status)), t.on_danger, None, status)
}

/// A destructive item in a menu (Delete…).
pub fn danger_ghost(theme: &Theme, status: button::Status) -> button::Style {
    let t = tokens(theme);
    let bg = matches!(status, button::Status::Hovered | button::Status::Pressed)
        .then_some(t.accent_soft);
    button_style(bg, t.danger, None, status)
}

/// A profile list row.
pub fn list_row(selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let t = tokens(theme);
        let bg = if selected {
            Some(t.accent_soft)
        } else if matches!(status, button::Status::Hovered | button::Status::Pressed) {
            Some(t.surface)
        } else {
            None
        };
        button_style(bg, t.text, None, status)
    }
}

/// A panel on the window background.
pub fn surface(theme: &Theme) -> container::Style {
    let t = tokens(theme);
    container::Style {
        text_color: Some(t.text),
        background: Some(t.surface.into()),
        border: Border {
            color: t.line,
            width: 1.0,
            radius: RADIUS.into(),
        },
        ..container::Style::default()
    }
}

/// A dialog or tooltip, above everything else.
pub fn raised(theme: &Theme) -> container::Style {
    let t = tokens(theme);
    container::Style {
        text_color: Some(t.text),
        background: Some(t.raised.into()),
        border: Border {
            color: t.line,
            width: 1.0,
            radius: (RADIUS * 1.5).into(),
        },
        shadow: Shadow {
            color: Color::from_rgba8(0, 0, 0, 0.35),
            offset: Vector::new(0.0, 6.0),
            blur_radius: 24.0,
        },
        snap: true,
    }
}

/// The dimmed layer behind a dialog.
pub fn scrim(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Color::from_rgba8(0, 0, 0, 0.45).into()),
        ..container::Style::default()
    }
}

/// Secondary text.
pub fn muted(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(tokens(theme).muted),
    }
}

/// Good news text (found, worked).
pub fn success(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(tokens(theme).success),
    }
}

/// A large option the player picks one of (setup's LIVE / OFFLINE).
pub fn choice(selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let t = tokens(theme);
        let (bg, edge) = if selected {
            (Some(t.accent_soft), t.accent)
        } else if matches!(status, button::Status::Hovered | button::Status::Pressed) {
            (Some(t.surface), t.outline)
        } else {
            (None, t.outline)
        };
        let mut style = button_style(bg, t.text, Some(edge), status);
        style.border.width = if selected { 2.0 } else { 1.0 };
        style
    }
}

/// Warning text (something to look at, not broken).
pub fn warning(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(tokens(theme).warning),
    }
}

/// Error text.
pub fn error(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(tokens(theme).danger),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WCAG 2.x contrast ratio of two opaque colours.
    fn contrast(a: Color, b: Color) -> f32 {
        fn lum(c: Color) -> f32 {
            let ch = |v: f32| {
                if v <= 0.03928 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * ch(c.r) + 0.7152 * ch(c.g) + 0.0722 * ch(c.b)
        }
        let (x, y) = (lum(a), lum(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    #[test]
    fn colours_meet_wcag_aa_in_both_themes() {
        for (name, t) in [("light", LIGHT), ("dark", DARK)] {
            let mut pairs = vec![
                ("on_accent/accent".to_owned(), t.on_accent, t.accent, 4.5),
                ("on_danger/danger".to_owned(), t.on_danger, t.danger, 4.5),
                ("text/accent_soft".to_owned(), t.text, t.accent_soft, 4.5),
                (
                    "accent/accent_soft".to_owned(),
                    t.accent,
                    t.accent_soft,
                    4.5,
                ),
            ];
            for (ground, g) in [("bg", t.bg), ("surface", t.surface), ("raised", t.raised)] {
                for (fg, c, min) in [
                    ("text", t.text, 4.5),
                    ("muted", t.muted, 4.5),
                    ("accent", t.accent, 4.5),
                    ("success", t.success, 4.5),
                    ("warning", t.warning, 4.5),
                    ("danger", t.danger, 4.5),
                    ("outline", t.outline, 3.0),
                ] {
                    pairs.push((format!("{fg}/{ground}"), c, g, min));
                }
            }
            for (pair, fg, ground, min) in pairs {
                let ratio = contrast(fg, ground);
                assert!(ratio >= min, "{name} {pair}: {ratio:.2} < {min}");
            }
        }
    }

    #[test]
    fn os_setting_picks_the_theme() {
        assert!(tokens(&theme(true)).bg == DARK.bg);
        assert!(tokens(&theme(false)).bg == LIGHT.bg);
    }
}
