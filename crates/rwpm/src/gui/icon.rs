//! Embedded fonts (Inter, Font Awesome Free 7.3.1 Solid) and the icons the GUI uses.
//! Code points come from the font's own cmap (checked in the tests below).

use iced::widget::{Text, text};
use iced::{Font, font};

pub const INTER_REGULAR: &[u8] = include_bytes!("../../assets/fonts/Inter-Regular.ttf");
pub const INTER_SEMIBOLD: &[u8] = include_bytes!("../../assets/fonts/Inter-SemiBold.ttf");
pub const FA_SOLID: &[u8] = include_bytes!("../../assets/fonts/FontAwesome7Free-Solid-900.otf");

/// Body text.
pub const UI: Font = Font::with_name("Inter");
/// Headings.
pub const UI_BOLD: Font = Font {
    weight: font::Weight::Semibold,
    ..UI
};
const ICONS: Font = Font {
    weight: font::Weight::Black,
    ..Font::with_name("Font Awesome 7 Free")
};

/// The app's own icon, 64 px raw RGBA made by `scripts/icon.sh` from `rwpm.svg`.
const APP_ICON_64: &[u8] = include_bytes!("../../assets/icon/rwpm-64.rgba");
/// Window class / Wayland app id; the Linux desktop entry is `rwpm.desktop`.
#[cfg(target_os = "linux")]
pub const APP_ID: &str = "rwpm";

/// The window and taskbar icon.
pub fn window_icon() -> Option<iced::window::Icon> {
    iced::window::icon::from_rgba(APP_ICON_64.to_vec(), 64, 64).ok()
}

/// Every font to load at start, in the app and in headless renders.
pub fn fonts() -> Vec<std::borrow::Cow<'static, [u8]>> {
    [INTER_REGULAR, INTER_SEMIBOLD, FA_SOLID]
        .into_iter()
        .map(Into::into)
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(
    dead_code,
    reason = "glyphs for the T25-T30 screens; the font test covers every one"
)]
pub enum Icon {
    Play,
    Offline,
    Live,
    Default,
    Add,
    Search,
    Settings,
    Help,
    More,
    Warning,
    Ok,
    Failed,
    Copy,
    Folder,
    Back,
    Forward,
    Close,
    Refresh,
    Rename,
    Delete,
    Log,
    Clipboard,
    Busy,
}

impl Icon {
    #[cfg(test)]
    pub const ALL: [Icon; 23] = [
        Icon::Play,
        Icon::Offline,
        Icon::Live,
        Icon::Default,
        Icon::Add,
        Icon::Search,
        Icon::Settings,
        Icon::Help,
        Icon::More,
        Icon::Warning,
        Icon::Ok,
        Icon::Failed,
        Icon::Copy,
        Icon::Folder,
        Icon::Back,
        Icon::Forward,
        Icon::Close,
        Icon::Refresh,
        Icon::Rename,
        Icon::Delete,
        Icon::Log,
        Icon::Clipboard,
        Icon::Busy,
    ];

    /// The Font Awesome private-use code point (glyph name in the comment).
    pub fn code(self) -> char {
        match self {
            Icon::Play => '\u{f04b}',      // play
            Icon::Offline => '\u{f2dc}',   // snowflake
            Icon::Live => '\u{f0c2}',      // cloud
            Icon::Default => '\u{f015}',   // house
            Icon::Add => '\u{f067}',       // plus
            Icon::Search => '\u{f002}',    // magnifying-glass
            Icon::Settings => '\u{f013}',  // gear
            Icon::Help => '\u{f059}',      // circle-question
            Icon::More => '\u{f141}',      // ellipsis
            Icon::Warning => '\u{f071}',   // triangle-exclamation
            Icon::Ok => '\u{f058}',        // circle-check
            Icon::Failed => '\u{f057}',    // circle-xmark
            Icon::Copy => '\u{f0c5}',      // copy
            Icon::Folder => '\u{f07c}',    // folder-open
            Icon::Back => '\u{f060}',      // arrow-left
            Icon::Forward => '\u{f054}',   // chevron-right
            Icon::Close => '\u{f00d}',     // xmark
            Icon::Refresh => '\u{f2f1}',   // rotate
            Icon::Rename => '\u{f304}',    // pen
            Icon::Delete => '\u{f2ed}',    // trash-can
            Icon::Log => '\u{f15c}',       // file-lines
            Icon::Clipboard => '\u{f328}', // clipboard
            Icon::Busy => '\u{f110}',      // spinner
        }
    }
}

/// An icon glyph, sized like the text beside it.
pub fn icon<'a>(i: Icon) -> Text<'a> {
    text(i.code()).font(ICONS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_icon_is_a_valid_64px_image() {
        assert_eq!(APP_ICON_64.len(), 64 * 64 * 4);
        assert!(window_icon().is_some());
    }

    #[test]
    fn every_icon_is_in_the_embedded_font() {
        let face = ttf_parser::Face::parse(FA_SOLID, 0).expect("Font Awesome parses");
        for i in Icon::ALL {
            assert!(face.glyph_index(i.code()).is_some(), "{i:?} missing");
        }
    }

    #[test]
    fn embedded_fonts_match_their_checksums() {
        use sha2::{Digest, Sha256};
        let sums = include_str!("../../assets/fonts/SHA256SUMS");
        for (file, bytes) in [
            ("Inter-Regular.ttf", INTER_REGULAR),
            ("Inter-SemiBold.ttf", INTER_SEMIBOLD),
            ("FontAwesome7Free-Solid-900.otf", FA_SOLID),
        ] {
            let hex: String = Sha256::digest(bytes)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            assert!(sums.contains(&format!("{hex}  {file}")), "{file} changed");
        }
    }
}
