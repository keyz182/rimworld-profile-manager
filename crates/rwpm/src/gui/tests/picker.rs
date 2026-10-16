//! T56 PR C: the profile's Game version row and picker, V61 and V54.

use super::super::message::Drill;
use iced::Task;
use rwpm_core::builds::BuildKey;

use super::builds::{VERSION, clicked, found};
use super::*;

pub(super) const OLD: &str = "1.5.4409 rev1118";
const NEW: &str = "1.7.4976 rev1043";

fn profile_toml(sb: &Sandbox, id: &ProfileId) -> Vec<u8> {
    std::fs::read(sb.path(&format!("app/profiles/{}/profile.toml", id.as_str()))).expect("read")
}

pub(super) fn saves(sb: &Sandbox, id: &ProfileId) {
    let dir = sb.path(&format!("app/profiles/{}/SaveData/Saves", id.as_str()));
    std::fs::create_dir_all(&dir).expect("saves");
    std::fs::write(dir.join("Colony.rws"), "x").expect("save");
}

/// A profile, the Steam install on `VERSION`, and a stored build of `stored`.
pub(super) fn world(stored: &str) -> (Sandbox, ProfileId, String) {
    let sb = Sandbox::new();
    sb.steam_install_as("1111", stored);
    sb.stored_build();
    sb.steam_install("2222");
    let id = sb
        .ctx()
        .store()
        .expect("store")
        .create("Run")
        .expect("create")
        .id;
    let key = BuildKey::from_version(stored).expect("key").to_string();
    (sb, id, key)
}

/// Opens the picker the way the Change… button does, feeding the read by hand.
pub(super) fn open_picker(sb: &Sandbox, id: &ProfileId) -> App {
    let mut app = sb.app();
    let _ = app.update(Message::Select(id.clone()));
    let _ = app.update(Message::PickerRead(sb.ctx().read_builds()));
    app
}

pub(super) fn pick(app: &mut App, key: Option<&str>) -> Task<Message> {
    let _ = app.update(Message::PickBuild(key.map(ToOwned::to_owned)));
    app.update(Message::Confirm)
}

#[test]
fn default_has_a_caption_and_no_picker() {
    let sb = Sandbox::new();
    let app = sb.app();
    assert!(found(&app, text::DEFAULT_GAME_VERSION));
    assert!(!found(&app, text::CHANGE), "V54: Default cannot be pinned");
}

#[test]
fn row_shows_the_game_and_change_opens_the_picker() {
    let (sb, id, key) = world(OLD);
    let mut app = sb.app();
    let _ = app.update(Message::Select(id.clone()));
    assert!(found(
        &app,
        &text::game_version_is(&text::steam_install_label(Some(VERSION)))
    ));
    assert_eq!(clicked(&app, text::CHANGE).len(), 1);
    assert_eq!(clicked(&app, text::MANAGE).len(), 1);
    for m in clicked(&app, text::MANAGE) {
        let _ = app.update(m);
    }
    assert_eq!(app.drill, Some(Drill::GameVersions));

    let _ = app.update(Message::Select(id.clone()));
    let _ = app.update(Message::PickerRead(sb.ctx().read_builds()));
    let Some(Dialog::PickVersion {
        steam,
        builds,
        chosen,
    }) = &app.dialog
    else {
        panic!("no picker: {:?}", app.dialog);
    };
    assert_eq!(steam, &text::steam_install_label(Some(VERSION)));
    assert_eq!(builds.len(), 1);
    assert_eq!(builds[0].0, key);
    assert_eq!(chosen, &None);
    assert!(found(&app, steam));
    assert!(found(&app, &builds[0].1));
}

#[test]
fn picking_a_stored_version_pins_and_picking_steam_unpins() {
    let (sb, id, key) = world(OLD);
    let mut app = open_picker(&sb, &id);
    let _ = pick(&mut app, Some(&key));
    assert!(app.dialog.is_none());
    let pinned = app.profiles.iter().find(|p| p.id == id).expect("profile");
    assert_eq!(pinned.game_build.as_deref(), Some(key.as_str()));
    assert_eq!(
        app.notice,
        Some(Notice::Info(text::game_version_changed("Run", &key)))
    );
    assert!(found(&app, &text::game_version_is(&key)));

    let mut app = open_picker(&sb, &id);
    let _ = pick(&mut app, None);
    let back = app.profiles.iter().find(|p| p.id == id).expect("profile");
    assert_eq!(back.game_build, None);
}

#[test]
fn v61_older_version_with_saves_asks_and_cancel_changes_nothing() {
    let (sb, id, key) = world(OLD);
    saves(&sb, &id);
    let before = profile_toml(&sb, &id);
    let mut app = open_picker(&sb, &id);
    let _ = pick(&mut app, Some(&key));
    let Some(Dialog::ConfirmDowngrade { from, into, .. }) = &app.dialog else {
        panic!("no question: {:?}", app.dialog);
    };
    assert_eq!((from.as_str(), into.as_str()), (VERSION, OLD));
    assert!(found(
        &app,
        "Saves made in 1.6 may not open in 1.5. Change anyway?"
    ));
    let _ = app.update(Message::Cancel);
    assert!(app.dialog.is_none());
    assert_eq!(
        profile_toml(&sb, &id),
        before,
        "Cancel leaves profile.toml byte-identical"
    );

    let _ = app.update(Message::PickerRead(sb.ctx().read_builds()));
    let _ = pick(&mut app, Some(&key));
    assert!(matches!(app.dialog, Some(Dialog::ConfirmDowngrade { .. })));
    let _ = app.update(Message::Confirm);
    assert!(app.dialog.is_none());
    let pinned = app.profiles.iter().find(|p| p.id == id).expect("profile");
    assert_eq!(pinned.game_build.as_deref(), Some(key.as_str()));
}

#[test]
fn v61_no_question_without_saves_or_for_a_newer_version() {
    let (sb, id, key) = world(OLD);
    let mut app = open_picker(&sb, &id);
    let _ = pick(&mut app, Some(&key));
    assert!(app.dialog.is_none(), "no saves, no question");

    let (sb, id, key) = world(NEW);
    saves(&sb, &id);
    let mut app = open_picker(&sb, &id);
    let _ = pick(&mut app, Some(&key));
    assert!(app.dialog.is_none(), "1.6 to 1.7 is not a downgrade");
    let pinned = app.profiles.iter().find(|p| p.id == id).expect("profile");
    assert_eq!(pinned.game_build.as_deref(), Some(key.as_str()));
}

#[test]
fn change_waits_while_a_game_runs_and_a_removed_build_is_refused() {
    let (sb, id, key) = world(OLD);
    let mut app = open_picker(&sb, &id);
    app.game_running = true;
    let _ = app.update(Message::ChangeGameVersion);
    assert_eq!(
        app.notice,
        Some(Notice::Info(text::GAME_RUNNING.to_owned()))
    );
    assert!(
        clicked(&app, text::CHANGE).is_empty(),
        "Change… is off while a game runs"
    );
    app.game_running = false;

    let before = profile_toml(&sb, &id);
    let dir = sb.builds().dir_of(&BuildKey::parse(&key).expect("key"));
    std::fs::remove_dir_all(dir).expect("remove build");
    let _ = pick(&mut app, Some(&key));
    assert!(
        matches!(&app.notice, Some(Notice::Error(e)) if e.contains(&key)),
        "{:?}",
        app.notice
    );
    assert_eq!(profile_toml(&sb, &id), before);
}

/// N3: an unreadable version still gets a question, with words that name no version
/// instead of a blank ("Saves made in  may not open in …").
#[test]
fn v61_question_without_a_readable_version_names_none() {
    assert_eq!(
        text::confirm_downgrade("", "1.5.4409 rev1118"),
        text::CONFIRM_DOWNGRADE_UNKNOWN
    );
    assert_eq!(
        text::confirm_downgrade("1.6.4871 rev598", "  "),
        text::CONFIRM_DOWNGRADE_UNKNOWN
    );
    assert!(!text::CONFIRM_DOWNGRADE_UNKNOWN.contains("  "));

    // Through the dialog: the current game cannot be read (no Steam, unpinned profile).
    let (sb, id, key) = world(OLD);
    saves(&sb, &id);
    let mut app = open_picker(&sb, &id);
    app.game = Err("gone".to_owned());
    let _ = pick(&mut app, Some(&key));
    assert!(matches!(app.dialog, Some(Dialog::ConfirmDowngrade { .. })));
    assert!(found(&app, text::CONFIRM_DOWNGRADE_UNKNOWN));
}
