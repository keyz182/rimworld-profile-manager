//! "Share as modpack…". The core's own tests cover the format, the checks on every
//! string and every refusal; these cover what the window adds: who is offered what, the
//! defaults and notices, the form's limits, and one real share through the job thread.

use rwpm_core::ProfileError;
use rwpm_core::modpack;

use super::backups::{selected, wait_for};
use super::builds::{clicked, found};
use super::*;
use crate::gui::Job;
use crate::gui::modpack::ShareTick;

/// A LIVE profile listing `active` mods.
pub(super) fn live_profile(sb: &Sandbox, name: &str, active: &[&str]) -> ProfileId {
    let id = sb
        .ctx()
        .store()
        .expect("store")
        .create(name)
        .expect("create")
        .id;
    let items: String = active.iter().map(|a| format!("<li>{a}</li>")).collect();
    put(
        sb,
        &id,
        "Config/ModsConfig.xml",
        &format!("<ModsConfigData><activeMods>{items}</activeMods></ModsConfigData>"),
    );
    id
}

/// A file below the profile's save data.
pub(super) fn put(sb: &Sandbox, id: &ProfileId, rel: &str, body: &str) {
    let path = sb
        .ctx()
        .store()
        .expect("store")
        .root()
        .join(id.as_str())
        .join("SaveData")
        .join(rel);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("dirs");
    std::fs::write(path, body).expect("write");
}

/// What typing `words` into the description box sends.
fn type_description(words: String) -> Message {
    use iced::widget::text_editor::{Action, Edit};
    Message::ShareEdit(Action::Edit(Edit::Paste(std::sync::Arc::new(words))))
}

fn draft(app: &App) -> Box<crate::gui::modpack::ShareDraft> {
    let Some(Dialog::Share(draft)) = app.dialog.clone() else {
        panic!("{:?}", app.dialog);
    };
    draft
}

#[test]
fn share_is_in_the_menu_of_every_profile() {
    let sb = Sandbox::new();
    let live = live_profile(&sb, "Loose", &["ludeon.rimworld"]);
    let offline = sb.offline_profile("Frozen", &["ludeon.rimworld"]);
    for id in [live, offline, ProfileId::default_id()] {
        let mut app = selected(&sb, &id);
        let _ = app.update(Message::ToggleMenu);
        assert!(found(&app, text::SHARE_MENU), "{id}");
    }
}

/// Where `label` lies in a window of the smallest size the layout is designed for:
/// 960 × 600. When a dialog is too tall the layout squeezes the content to no height instead
/// of pushing it off screen, so a test must check the height, not only the position.
pub(super) fn smallest_window_bounds(app: &App, label: &str) -> iced::Rectangle {
    let settings = iced::Settings {
        fonts: crate::gui::icon::fonts(),
        default_font: crate::gui::icon::UI,
        ..iced::Settings::default()
    };
    let mut ui = iced_test::Simulator::with_size(settings, (960.0, 600.0), app.view());
    ui.find(label).expect("laid out").bounds()
}

/// A line of text is about 21 px high; squeezed content is 0.
const TEXT_LINE: f32 = 18.0;

/// `label` has its full height: it was not squeezed away.
pub(super) fn assert_not_squeezed(app: &App, label: &str) {
    let b = smallest_window_bounds(app, label);
    assert!(
        b.height >= TEXT_LINE,
        "{label:?} is squeezed to {} px",
        b.height
    );
}

/// `label` has its full height and lies inside the window: it is on screen without scrolling.
pub(super) fn assert_on_screen(app: &App, label: &str) {
    assert_not_squeezed(app, label);
    let b = smallest_window_bounds(app, label);
    assert!(
        b.y >= 0.0 && b.y + b.height <= 600.0,
        "{label:?} spans {} to {}",
        b.y,
        b.y + b.height
    );
}

/// The dialog's title and its Share button stay on screen at the smallest window, even with
/// many saves ticked and every warning showing; the form between them scrolls.
#[test]
fn the_share_dialog_keeps_its_title_and_button_on_screen_at_the_smallest_window() {
    let sb = Sandbox::new();
    let id = live_profile(&sb, "Ironman", &["ludeon.rimworld"]);
    for i in 0..40 {
        put(&sb, &id, &format!("Saves/colony{i:02}.rws"), "save");
    }
    let mut app = selected(&sb, &id);
    let _ = app.update(Message::Share);
    for i in 0..40 {
        let _ = app.update(Message::ShareSave(i, true));
    }
    let _ = app.update(Message::ShareStart(Some(3)));
    let _ = app.update(Message::Input(Field::Link, "nope".into()));
    let long = "x".repeat(modpack::file::MAX_DESCRIPTION_CHARS + 1);
    let _ = app.update(type_description(long));
    // The title and the button are pinned on screen. The parts of the form are all laid out
    // with their full height (the long ones scroll); none is squeezed away.
    assert_on_screen(&app, text::SHARE_TITLE);
    assert_on_screen(&app, text::SHARE_IT);
    for label in [
        text::SHARE_SAVES,
        text::SHARE_START,
        text::SHARE_DESCRIPTION,
        text::SHARE_LINK,
    ] {
        assert_not_squeezed(&app, label);
    }
}

/// Step budget: from the list to a saved pack in ≤ 4 steps (select, More, Share as
/// modpack…, Share). The last step opens the system's file dialog, which is not driven here:
/// the check is that the button is there and asks for it.
#[test]
fn sharing_takes_at_most_four_steps() {
    let sb = Sandbox::new();
    let id = live_profile(&sb, "Ironman", &["ludeon.rimworld"]);
    let mut app = sb.app();
    let mut steps = 0;
    for label in ["Ironman", text::MORE, text::SHARE_MENU] {
        super::click(&mut app, label);
        steps += 1;
    }
    assert_eq!(app.selected, id);
    assert!(matches!(app.dialog, Some(Dialog::Share(_))));
    let last = clicked(&app, text::SHARE_IT);
    assert!(matches!(last.as_slice(), [Message::Confirm]), "{last:?}");
    steps += 1;
    assert!(steps <= 4, "{steps} steps");
}

#[test]
fn share_waits_while_something_runs() {
    let sb = Sandbox::new();
    let id = live_profile(&sb, "Loose", &["ludeon.rimworld"]);
    let mut app = selected(&sb, &id);
    let _ = app.update(Message::ToggleMenu);
    let messages = clicked(&app, text::SHARE_MENU);
    assert!(
        matches!(messages.as_slice(), [Message::Share]),
        "{messages:?}"
    );
    // The gate is in `update`: a click while a job runs does nothing.
    app.job = Some(Job {
        label: "Copying".into(),
        done: 0,
        total: 0,
        percent: false,
    });
    let _ = app.update(Message::Share);
    assert!(app.dialog.is_none());
    app.job = None;
    app.game_running = true;
    let _ = app.update(Message::Share);
    assert!(app.dialog.is_none(), "a running game blocks it too");
}

#[test]
fn the_share_dialog_starts_with_settings_and_creations_and_warns_about_private_details() {
    let sb = Sandbox::new();
    let id = live_profile(&sb, "Ironman", &["ludeon.rimworld"]);
    put(&sb, &id, "Config/Mod_1_A.xml", "<a/>");
    put(&sb, &id, "Config/Mod_2_B.xml", "<bb/>");
    put(&sb, &id, "Config/Prefs.xml", "<prefs/>");
    put(&sb, &id, "Scenarios/s.rsc", "<scenario/>");
    put(&sb, &id, "Saves/b.rws", "bbb");
    put(&sb, &id, "Saves/a.rws", "a");
    let mut app = selected(&sb, &id);
    let _ = app.update(Message::Share);
    let d = draft(&app);
    assert!(d.mod_settings && d.creations && !d.game_settings);
    assert_eq!(
        (d.mod_settings_count.files, d.mod_settings_count.bytes),
        (2, 9)
    );
    assert_eq!(d.creations_count.files, 1);
    assert_eq!(d.game_settings_count.files, 1);
    let names: Vec<_> = d.saves.iter().map(|s| (s.name.as_str(), s.on)).collect();
    assert_eq!(names, [("a.rws", false), ("b.rws", false)]);
    assert_eq!(d.start, None);
    assert!(found(&app, text::SHARE_SETTINGS_NOTICE));
    assert!(found(&app, text::SHARE_NOTICE));
    assert!(d.ready());
}

#[test]
fn the_start_save_is_always_a_ticked_save() {
    let sb = Sandbox::new();
    let id = live_profile(&sb, "Ironman", &["ludeon.rimworld"]);
    put(&sb, &id, "Saves/a.rws", "a");
    put(&sb, &id, "Saves/b.rws", "b");
    let mut app = selected(&sb, &id);
    let _ = app.update(Message::Share);
    let _ = app.update(Message::ShareStart(Some(0)));
    assert_eq!(draft(&app).start, None, "an unticked save cannot start");
    let _ = app.update(Message::ShareSave(0, true));
    let _ = app.update(Message::ShareStart(Some(0)));
    assert_eq!(draft(&app).start, Some(0));
    let _ = app.update(Message::ShareStart(Some(7)));
    assert_eq!(draft(&app).start, None, "no such save");
    let _ = app.update(Message::ShareStart(Some(0)));
    let _ = app.update(Message::ShareSave(0, false));
    assert_eq!(
        draft(&app).start,
        None,
        "unticking the start save clears it"
    );
    let _ = app.update(Message::ShareTick(ShareTick::GameSettings, true));
    assert!(draft(&app).game_settings);
}

#[test]
fn a_profile_with_no_mod_list_cannot_be_shared() {
    let sb = Sandbox::new();
    let id = sb
        .ctx()
        .store()
        .expect("store")
        .create("Fresh")
        .expect("create")
        .id;
    let mut app = selected(&sb, &id);
    let _ = app.update(Message::Share);
    assert!(app.dialog.is_none());
    let words = ProfileError::NoModList("Fresh".into()).to_string();
    assert_eq!(app.notice, Some(Notice::Error(words)));
}

#[test]
fn the_form_will_not_share_what_the_core_would_refuse() {
    let sb = Sandbox::new();
    let id = live_profile(&sb, "Ironman", &["ludeon.rimworld"]);
    let mut app = selected(&sb, &id);
    let _ = app.update(Message::Share);

    let _ = app.update(Message::Input(Field::Link, "javascript:alert(1)".into()));
    assert!(found(&app, text::SHARE_LINK_BAD));
    let _ = app.update(Message::Confirm);
    assert!(matches!(app.dialog, Some(Dialog::Share(_))), "stays open");
    assert!(app.job.is_none());

    let _ = app.update(Message::Input(
        Field::Link,
        "https://steamcommunity.com/x".into(),
    ));
    assert!(draft(&app).ready());

    let long = "x".repeat(modpack::file::MAX_DESCRIPTION_CHARS + 1);
    let _ = app.update(type_description(long));
    assert!(found(
        &app,
        &text::share_description_long(modpack::file::MAX_DESCRIPTION_CHARS)
    ));
    assert!(!draft(&app).ready());
    let _ = app.update(Message::Confirm);
    assert!(matches!(app.dialog, Some(Dialog::Share(_))));

    let name = "n".repeat(modpack::file::MAX_NAME_CHARS + 1);
    let long_named = live_profile(&sb, &name, &["ludeon.rimworld"]);
    let mut app = selected(&sb, &long_named);
    let _ = app.update(Message::Share);
    assert!(found(
        &app,
        &text::share_name_long(modpack::file::MAX_NAME_CHARS)
    ));
    assert!(!draft(&app).ready());
}

/// The one test that uses the real job thread and the real process probe: it needs the
/// pack to be written (the thread starts when the task is made). It fails if a RimWorld is
/// open.
#[test]
fn a_pack_shared_in_the_app_holds_the_list_and_the_choices() {
    let sb = Sandbox::new();
    sb.steam_install("1111");
    let id = live_profile(
        &sb,
        "Shared",
        &["ludeon.rimworld", "brrainz.harmony", "gone.missing"],
    );
    put(&sb, &id, "Config/Mod_1_A.xml", "<a/>");
    put(&sb, &id, "Saves/colony.rws", "save");
    std::fs::create_dir_all(sb.path("out")).expect("out");

    let mut app = selected(&sb, &id);
    let _ = app.update(Message::Share);
    let _ = app.update(Message::ShareSave(0, true));
    let _ = app.update(Message::ShareStart(Some(0)));
    let _ = app.update(type_description("  Hard mode  ".into()));
    let chosen = draft(&app);
    // The player typed no extension: the file still becomes `.rimpack`.
    let _ = app.update(Message::SharePicked(chosen, Some(sb.path("out/mine"))));
    assert!(app.job.is_some());
    let pack = sb.path("out/mine.rimpack");
    wait_for("the modpack file", || pack.is_file());

    let manifest = modpack::read_manifest(&pack).expect("manifest");
    assert_eq!(manifest.name, "Shared");
    assert_eq!(manifest.description, "Hard mode");
    assert_eq!(manifest.includes.saves, ["colony.rws"]);
    assert_eq!(manifest.start_save.as_deref(), Some("colony.rws"));
    assert!(manifest.includes.mod_settings && manifest.includes.creations);
    assert!(!manifest.includes.game_settings);
    assert!(manifest.game_version.is_some(), "from the game install");
    let ids: Vec<_> = manifest
        .mods
        .iter()
        .map(|m| m.package_id.as_str())
        .collect();
    assert_eq!(
        ids,
        ["brrainz.harmony", "gone.missing"],
        "Core is not a choice"
    );
    assert_eq!(manifest.mods[0].workshop_id, Some(2009463077));
    assert_eq!(manifest.mods[1].workshop_id, None);
}

/// A 1 x 1 PNG.
const TINY_PNG: [u8; 70] = [
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0xF8, 0xCF, 0xC0, 0xF0,
    0x1F, 0x00, 0x05, 0x00, 0x01, 0xFF, 0x89, 0x99, 0x3D, 0x1D, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45,
    0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
];

fn picked_cover(sb: &Sandbox) -> crate::gui::modpack::ShareCover {
    let file = sb.path("cover.png");
    std::fs::write(&file, TINY_PNG).expect("write");
    crate::gui::modpack::load_cover(&file).expect("a PNG is a cover")
}

#[test]
fn a_cover_is_chosen_cleared_and_refused_when_it_is_not_a_picture() {
    let sb = Sandbox::new();
    sb.steam_install("1111");
    let id = live_profile(&sb, "Shared", &["ludeon.rimworld"]);
    let mut app = selected(&sb, &id);
    let _ = app.update(Message::Share);

    let cover = picked_cover(&sb);
    assert_eq!(cover.file_name, "cover.png");
    let _ = app.update(Message::ShareCoverPicked(Some(Ok(Box::new(cover)))));
    assert!(draft(&app).cover.is_some());
    let _ = app.update(Message::ShareCoverClear);
    assert!(draft(&app).cover.is_none());

    let notes = sb.path("notes.png");
    std::fs::write(&notes, "not a picture").expect("write");
    let refused = crate::gui::modpack::load_cover(&notes).expect_err("refused");
    let _ = app.update(Message::ShareCoverPicked(Some(Err(refused))));
    assert!(matches!(app.notice, Some(Notice::Error(_))));
    assert!(draft(&app).cover.is_none());
}

#[test]
fn the_preview_parses_the_description_and_editing_goes_back() {
    use iced::widget::markdown::Item;
    let sb = Sandbox::new();
    sb.steam_install("1111");
    let id = live_profile(&sb, "Shared", &["ludeon.rimworld"]);
    let mut app = selected(&sb, &id);
    let _ = app.update(Message::Share);
    let _ = app.update(type_description("# Hard\n\nplay *well*".into()));
    assert!(draft(&app).preview.is_none());
    let _ = app.update(Message::SharePreview(true));
    let items = draft(&app).preview.expect("preview on");
    assert!(matches!(items[0], Item::Heading(..)), "{items:?}");
    let _ = app.update(Message::SharePreview(false));
    assert!(draft(&app).preview.is_none());
}

#[test]
fn a_cover_and_the_preview_keep_the_title_and_button_on_screen() {
    let sb = Sandbox::new();
    sb.steam_install("1111");
    let id = live_profile(&sb, "Shared", &["ludeon.rimworld"]);
    let mut app = selected(&sb, &id);
    let _ = app.update(Message::Share);
    let _ = app.update(type_description("A line.\n\n".repeat(80)));
    let _ = app.update(Message::SharePreview(true));
    let cover = picked_cover(&sb);
    let _ = app.update(Message::ShareCoverPicked(Some(Ok(Box::new(cover)))));
    assert_on_screen(&app, text::SHARE_TITLE);
    assert_on_screen(&app, text::SHARE_IT);
}

#[test]
fn a_chosen_cover_is_written_into_the_pack() {
    let sb = Sandbox::new();
    sb.steam_install("1111");
    let id = live_profile(&sb, "Shared", &["ludeon.rimworld", "brrainz.harmony"]);
    std::fs::create_dir_all(sb.path("out")).expect("out");
    let mut app = selected(&sb, &id);
    let _ = app.update(Message::Share);
    let cover = picked_cover(&sb);
    let _ = app.update(Message::ShareCoverPicked(Some(Ok(Box::new(cover)))));
    let chosen = draft(&app);
    let _ = app.update(Message::SharePicked(chosen, Some(sb.path("out/covered"))));
    let pack = sb.path("out/covered.rimpack");
    wait_for("the modpack file", || pack.is_file());
    let (manifest, cover) = modpack::read_preview(&pack, true).expect("preview");
    assert_eq!(manifest.cover.as_deref(), Some("cover.png"));
    assert!(cover.is_some());
}
