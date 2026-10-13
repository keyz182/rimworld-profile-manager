//! T59: the docs and the Help section quote the labels the app really shows, so a renamed
//! button cannot leave them describing a screen that is gone (D39).

use super::*;

const GUIDE: &str = include_str!("../../../../../docs/game-versions.md");
const README: &str = include_str!("../../../../../README.md");

#[test]
fn the_guide_quotes_the_shipped_labels() {
    let downgrade = text::confirm_downgrade("1.7.4976 rev1043", "1.6.4871 rev598");
    for label in [
        text::GAME_VERSIONS,
        text::GAME_VERSION,
        text::CAPTURE,
        text::STORE_IT,
        text::UPDATE_IT,
        text::CHANGE,
        text::USE_VERSION,
        text::KEEP_GOING,
        text::DELETE_VERSION,
        text::YES_DELETE,
        text::CANCEL,
        "Steam install",
        downgrade.as_str(),
    ] {
        assert!(
            GUIDE.contains(label),
            "docs/game-versions.md does not quote {label:?}"
        );
    }
    // The "Plays:" line of the profile section.
    let plays = text::game_version_is("");
    assert!(
        GUIDE.contains(plays.trim()),
        "docs/game-versions.md does not quote {plays:?}"
    );
}

#[test]
fn the_readme_quotes_the_labels_and_links_the_guide() {
    for label in [
        text::GAME_VERSIONS,
        text::GAME_VERSION,
        text::CAPTURE,
        text::STORE_IT,
        text::CHANGE,
    ] {
        assert!(README.contains(label), "README.md does not quote {label:?}");
    }
    assert!(README.contains("docs/game-versions.md"));
}

#[test]
fn help_explains_game_versions() {
    let (title, words) = text::HELP_SECTIONS[6];
    assert_eq!(title, "Game versions");
    for part in [text::GAME_VERSIONS, "overlay", "launch options"] {
        assert!(words.contains(part), "{part:?} missing from {words}");
    }
    // The sections scroll, so the last one may be below the fold; the fit test in
    // `tests.rs` covers the dialog itself.
    let sb = Sandbox::new();
    let mut app = sb.app();
    let _ = app.update(Message::Help);
    let mut ui = iced_test::simulator(app.view());
    ui.find(text::HELP_SECTIONS[0].1)
        .expect("the dialog shows its sections");
}
