//! The docs and the Help section quote the labels the app really shows, so a renamed
//! button cannot leave them describing a screen that is gone.

use super::*;

const GUIDE: &str = include_str!("../../../../../docs/game-versions.md");
const README: &str = include_str!("../../../../../README.md");
const BACKUPS: &str = include_str!("../../../../../docs/backups.md");
const MODPACKS: &str = include_str!("../../../../../docs/modpacks.md");

/// A label as the guide writes it: in straight double quotes, so the check does not pass on
/// the same word used in other prose.
fn quoted(label: &str) -> String {
    format!("\"{label}\"")
}

#[test]
fn the_guide_quotes_the_shipped_labels() {
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
    ] {
        assert!(
            GUIDE.contains(&quoted(label)),
            "docs/game-versions.md does not quote {label:?}"
        );
    }
    // The "Plays:" line of the profile section.
    let plays = text::game_version_is("");
    assert!(GUIDE.contains(&quoted(plays.trim())), "no {plays:?}");
    // The question before an older game version: a block quote, word for word.
    let downgrade = text::confirm_downgrade("1.7.4976 rev1043", "1.6.4871 rev598");
    assert!(
        GUIDE.contains(&format!("> {downgrade}")),
        "no {downgrade:?}"
    );
}

/// The messages the Limits section quotes: each one is what the code says today.
#[test]
fn the_guide_quotes_the_refusals_the_code_gives() {
    let running_core = rwpm_core::ProfileError::GameRunning.to_string();
    let updating_core = rwpm_core::ProfileError::SourceChanging("RimWorld".into()).to_string();
    for (message, shown) in [
        (crate::run::LOCK_BUSY, crate::run::LOCK_BUSY),
        (text::GAME_RUNNING, text::GAME_RUNNING),
        (running_core.as_str(), running_core.as_str()),
        (updating_core.as_str(), "Steam is updating"),
    ] {
        assert!(message.contains(shown));
        assert!(
            GUIDE.contains(&quoted(shown)),
            "docs/game-versions.md does not quote {shown:?}"
        );
    }
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
        assert!(
            README.contains(&quoted(label)),
            "README.md does not quote {label:?}"
        );
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

/// The backup guide and the README quote the labels the app shows, and the guide
/// quotes the two sharing notes word for word.
#[test]
fn the_backup_docs_quote_the_shipped_labels_and_notices() {
    for label in [
        text::BACKUP_MENU,
        text::RESTORE_MENU,
        text::BACKUP_IT,
        text::RESTORE_IT,
        text::BACKUP_LOGS,
    ] {
        assert!(
            BACKUPS.contains(&quoted(label)),
            "docs/backups.md does not quote {label:?}"
        );
    }
    for notice in [text::NOTICE_MODS, text::NOTICE_GAME] {
        assert!(
            BACKUPS.contains(&format!("> {notice}")),
            "docs/backups.md does not quote {notice:?}"
        );
    }
    for label in [text::BACKUP_MENU, text::RESTORE_MENU, text::BACKUP_IT] {
        assert!(
            README.contains(&quoted(label)),
            "README.md does not quote {label:?}"
        );
    }
    assert!(README.contains("docs/backups.md"));
}

/// The modpack guide and the README quote the labels the app shows, and the guide
/// quotes the notices word for word.
#[test]
fn the_modpack_docs_quote_the_shipped_labels_and_notices() {
    for label in [
        text::SHARE_MENU,
        text::OPEN_PACK_MENU,
        text::SHARE_IT,
        text::OPEN_PACK_IT,
        text::OPEN_STEAM_PAGE,
        text::PACK_MISSING,
        text::PACK_MANUAL,
        text::PACK_PRESENT,
        text::SHARE_MOD_SETTINGS,
        text::SHARE_PICK,
        text::SHARE_COVER_CHOOSE,
        text::SHARE_PREVIEW,
        text::PACK_SECTION,
        text::PACK_RESET,
        text::RESET_SAVES,
        text::RESET_IT,
        text::PACK_SYNCED_GO,
        text::PACK_SYNCED_LATER,
    ] {
        assert!(
            MODPACKS.contains(&quoted(label)),
            "docs/modpacks.md does not quote {label:?}"
        );
    }
    for notice in [
        text::SHARE_NOTICE,
        text::SHARE_SETTINGS_NOTICE,
        text::PACK_HINT,
        text::PACK_SYNCED,
    ] {
        assert!(
            MODPACKS.contains(&format!("> {notice}")),
            "docs/modpacks.md does not quote {notice:?}"
        );
    }
    for label in [text::SHARE_MENU, text::OPEN_PACK_MENU, text::SHARE_IT] {
        assert!(
            README.contains(&quoted(label)),
            "README.md does not quote {label:?}"
        );
    }
    assert!(README.contains("docs/modpacks.md"));
}

/// The README's "Update check" section quotes the labels the app shows.
#[test]
fn the_readme_quotes_the_update_check_labels() {
    for label in [
        text::SETTINGS,
        text::UPDATES_LABEL,
        text::SAVE,
        text::RELEASE_PAGE,
        text::DISMISS,
        text::CHECK_NOW,
        "Daily",
        "Weekly",
        "Monthly",
        "Off",
    ] {
        assert!(
            README.contains(&quoted(label)),
            "README.md does not quote {label:?}"
        );
    }
    for frequency in rwpm_core::update_check::Frequency::ALL {
        assert!(
            ["Off", "Daily", "Weekly", "Monthly"].contains(&text::update_frequency(frequency)),
            "{frequency:?}"
        );
    }
}

#[test]
fn help_explains_modpacks_and_the_private_details_warning() {
    let (title, words) = text::HELP_SECTIONS[8];
    assert_eq!(title, "Modpacks");
    for part in [
        text::SHARE_MENU,
        text::OPEN_PACK_MENU,
        ".rimpack",
        "no mods",
        "private details",
    ] {
        assert!(words.contains(part), "{part:?} missing from {words}");
    }
}

#[test]
fn help_explains_backups_and_the_sharing_limit() {
    let (title, words) = text::HELP_SECTIONS[7];
    assert_eq!(title, "Backups");
    for part in [
        text::BACKUP_MENU,
        text::RESTORE_MENU,
        ".rimrim",
        "licence",
        "do not share",
    ] {
        assert!(words.contains(part), "{part:?} missing from {words}");
    }
}
