//! The update check: the version in the window, the schedule, the banner, and what
//! Settings and first-run setup save. No test reaches the network: tasks are not run here,
//! so results are fed in as messages.

use rwpm_core::update_check::{FILE_NAME, Frequency, RELEASES_PREFIX, Release, UpdateState};

use super::builds::{clicked, found};
use super::*;
use crate::gui::setup::SetupMsg;

fn release(version: &str) -> Release {
    Release {
        version: version.parse().expect("version"),
        url: format!("{RELEASES_PREFIX}tag/v{version}"),
    }
}

fn announce(version: &str) -> Message {
    Message::UpdateChecked {
        manual: false,
        result: Ok(release(version)),
    }
}

fn banner_text(version: &str) -> String {
    text::update_available(version, rwpm_core::VERSION)
}

fn saved(sb: &Sandbox) -> UpdateState {
    UpdateState::load(&sb.path("app")).expect("updates.toml")
}

fn turn_on(app: &mut App, frequency: Frequency) {
    app.updates.frequency = frequency;
}

#[test]
fn the_window_shows_its_version() {
    let sb = Sandbox::new();
    let app = sb.app();
    assert!(found(&app, &text::version(rwpm_core::VERSION)));
}

#[test]
fn a_fresh_install_checks_for_nothing() {
    let sb = Sandbox::new();
    let mut app = sb.app();
    assert_eq!(app.updates.frequency, Frequency::Off);
    assert_eq!(app.update(Message::UpdateTick).units(), 0, "no request");
    assert!(!app.update_checking);
    assert!(!sb.path("app").join(FILE_NAME).exists(), "nothing written");
}

#[test]
fn a_due_schedule_starts_a_single_check() {
    let sb = Sandbox::new();
    let mut app = sb.app();
    turn_on(&mut app, Frequency::Daily);
    assert_eq!(app.update(Message::UpdateTick).units(), 1, "one request");
    assert!(app.update_checking);
    // While one is on its way, another tick does not queue a second.
    assert_eq!(app.update(Message::UpdateTick).units(), 0);
}

#[test]
fn a_schedule_that_is_not_due_starts_nothing() {
    let sb = Sandbox::new();
    let mut app = sb.app();
    turn_on(&mut app, Frequency::Weekly);
    app.updates
        .record_success(rwpm_core::update_check::now_secs());
    assert_eq!(app.update(Message::UpdateTick).units(), 0, "no request");
    assert!(!app.update_checking);
}

#[test]
fn a_newer_release_shows_a_banner_that_dismiss_hides_for_that_release() {
    let sb = Sandbox::new();
    let mut app = sb.app();
    let _ = app.update(announce("999.0.0"));
    assert!(found(&app, &banner_text("999.0.0")));
    assert!(app.notice.is_none(), "a background find is not a notice");

    let messages = clicked(&app, text::DISMISS);
    assert!(
        matches!(messages.as_slice(), [Message::DismissUpdate]),
        "{messages:?}"
    );
    for m in messages {
        let _ = app.update(m);
    }
    assert!(!found(&app, &banner_text("999.0.0")));
    assert_eq!(saved(&sb).dismissed.as_deref(), Some("999.0.0"));

    // The same release found again stays hidden; a newer one comes back.
    let _ = app.update(announce("999.0.0"));
    assert!(!found(&app, &banner_text("999.0.0")));
    let _ = app.update(announce("1000.0.0"));
    assert!(found(&app, &banner_text("1000.0.0")));
}

#[test]
fn the_banner_has_a_release_page_button() {
    let sb = Sandbox::new();
    let mut app = sb.app();
    let _ = app.update(announce("999.0.0"));
    let messages = clicked(&app, text::RELEASE_PAGE);
    assert!(
        matches!(messages.as_slice(), [Message::OpenRelease]),
        "{messages:?}"
    );
}

#[test]
fn the_release_you_have_or_an_older_one_shows_no_banner() {
    let sb = Sandbox::new();
    let mut app = sb.app();
    for version in [rwpm_core::VERSION, "0.0.1"] {
        let _ = app.update(announce(version));
        assert!(app.announced_update().is_none(), "{version}");
        assert!(app.notice.is_none(), "{version}");
    }
}

#[test]
fn asking_by_hand_says_you_are_up_to_date() {
    let sb = Sandbox::new();
    let mut app = sb.app();
    let _ = app.update(Message::UpdateChecked {
        manual: true,
        result: Ok(release(rwpm_core::VERSION)),
    });
    assert_eq!(app.notice, Some(Notice::Info(text::UP_TO_DATE.into())));
}

#[test]
fn asking_by_hand_brings_back_a_dismissed_banner() {
    let sb = Sandbox::new();
    let mut app = sb.app();
    let _ = app.update(announce("999.0.0"));
    let _ = app.update(Message::DismissUpdate);
    assert!(app.announced_update().is_none());
    let _ = app.update(Message::UpdateChecked {
        manual: true,
        result: Ok(release("999.0.0")),
    });
    assert!(found(&app, &banner_text("999.0.0")));
    assert_eq!(saved(&sb).dismissed, None);
}

#[test]
fn a_success_is_remembered_for_the_schedule() {
    let sb = Sandbox::new();
    let mut app = sb.app();
    turn_on(&mut app, Frequency::Daily);
    let _ = app.update(announce(rwpm_core::VERSION));
    let state = saved(&sb);
    assert!(state.last_checked.is_some() && state.last_attempt.is_some());
    assert!(!state.is_due(rwpm_core::update_check::now_secs()));
}

#[test]
fn a_failed_background_check_is_quiet_and_waits_before_trying_again() {
    let sb = Sandbox::new();
    let mut app = sb.app();
    turn_on(&mut app, Frequency::Daily);
    app.update_checking = true;
    let _ = app.update(Message::UpdateChecked {
        manual: false,
        result: Err("offline".into()),
    });
    assert!(!app.update_checking);
    assert!(app.notice.is_none(), "{:?}", app.notice);
    let state = saved(&sb);
    assert_eq!(state.last_checked, None, "a failure is not a check");
    assert!(state.last_attempt.is_some());
    assert!(!app.updates.is_due(rwpm_core::update_check::now_secs()));
}

#[test]
fn a_failed_manual_check_says_why() {
    let sb = Sandbox::new();
    let mut app = sb.app();
    let _ = app.update(Message::UpdateChecked {
        manual: true,
        result: Err("offline".into()),
    });
    assert_eq!(
        app.notice,
        Some(Notice::Error(text::update_check_failed("offline")))
    );
}

#[test]
fn a_failed_check_keeps_the_release_found_before() {
    let sb = Sandbox::new();
    let mut app = sb.app();
    let _ = app.update(announce("999.0.0"));
    let _ = app.update(Message::UpdateChecked {
        manual: false,
        result: Err("offline".into()),
    });
    assert!(found(&app, &banner_text("999.0.0")));
}

#[test]
fn settings_save_the_frequency_and_a_check_follows() {
    let sb = Sandbox::new();
    let mut app = sb.app();
    let _ = app.update(Message::OpenSettings);
    let _ = app.update(Message::SetUpdateFrequency(Frequency::Weekly));
    assert_eq!(app.updates.frequency, Frequency::Off, "not saved yet");
    let _ = app.update(Message::Confirm);
    assert_eq!(saved(&sb).frequency, Frequency::Weekly);
    assert_eq!(app.updates.frequency, Frequency::Weekly);
    assert!(app.update_checking, "turning it on checks at once");
    assert_eq!(app.notice, Some(Notice::Info(text::SETTINGS_SAVED.into())));
}

#[test]
fn settings_keep_the_game_folder_when_only_the_frequency_changes() {
    let sb = Sandbox::new();
    let mut app = sb.app();
    let _ = app.update(Message::OpenSettings);
    let _ = app.update(Message::SetUpdateFrequency(Frequency::Monthly));
    let _ = app.update(Message::Confirm);
    let settings = AppSettings::load(&sb.path("app")).expect("settings");
    assert_eq!(settings.game_dir, Some(sb.game_dir()));
}

#[test]
fn cancelling_settings_leaves_the_frequency_alone() {
    let sb = Sandbox::new();
    let mut app = sb.app();
    let _ = app.update(Message::OpenSettings);
    let _ = app.update(Message::SetUpdateFrequency(Frequency::Weekly));
    let _ = app.update(Message::Cancel);
    assert_eq!(app.updates.frequency, Frequency::Off);
    assert!(!sb.path("app").join(FILE_NAME).exists());
}

#[test]
fn settings_open_on_the_saved_frequency() {
    let sb = Sandbox::new();
    let mut app = sb.app();
    turn_on(&mut app, Frequency::Monthly);
    let _ = app.update(Message::OpenSettings);
    let Some(Dialog::Settings { updates, .. }) = &app.dialog else {
        panic!("no settings dialog")
    };
    assert_eq!(*updates, Frequency::Monthly);
}

#[test]
fn settings_check_now_asks_whatever_the_frequency_is() {
    let sb = Sandbox::new();
    let mut app = sb.app();
    let _ = app.update(Message::OpenSettings);
    let messages = clicked(&app, text::CHECK_NOW);
    assert!(
        matches!(messages.as_slice(), [Message::CheckUpdatesNow]),
        "{messages:?}"
    );
    for m in messages {
        let _ = app.update(m);
    }
    assert!(app.update_checking);
    assert_eq!(app.updates.frequency, Frequency::Off, "still off");
    assert!(
        clicked(&app, text::CHECK_NOW).is_empty(),
        "no second check while one is on its way"
    );
}

#[test]
fn setup_saves_the_choice_when_it_finishes() {
    let sb = Sandbox::new();
    let mut ctx = sb.ctx();
    ctx.app_dir = sb.path("fresh-app");
    let mut app = App::new(ctx);
    assert!(app.setup.is_some(), "a fresh install runs setup");
    assert_eq!(app.setup.as_ref().map(|s| s.updates), Some(Frequency::Off));
    let _ = app.update(Message::Setup(SetupMsg::Updates(Frequency::Daily)));
    let _ = app.update(Message::Setup(SetupMsg::Skip));
    assert!(app.setup.is_none());
    let state = UpdateState::load(&sb.path("fresh-app")).expect("updates.toml");
    assert_eq!(state.frequency, Frequency::Daily);
    assert!(app.update_checking);
}

#[test]
fn setup_left_on_off_writes_no_update_file() {
    let sb = Sandbox::new();
    let mut ctx = sb.ctx();
    ctx.app_dir = sb.path("fresh-app");
    let mut app = App::new(ctx);
    let _ = app.update(Message::Setup(SetupMsg::Skip));
    assert!(!sb.path("fresh-app").join(FILE_NAME).exists());
    assert!(!app.update_checking);
}

#[test]
fn an_unreadable_state_file_turns_checking_off_and_says_so() {
    let sb = Sandbox::new();
    std::fs::write(sb.path("app").join(FILE_NAME), "frequency = [").expect("write");
    let app = sb.app();
    assert_eq!(app.updates.frequency, Frequency::Off);
    assert!(
        matches!(&app.notice, Some(Notice::Error(e)) if e.contains("update settings")),
        "{:?}",
        app.notice
    );
}

#[test]
fn only_this_projects_release_pages_are_opened() {
    use crate::gui::support::{open_release, open_url};
    let refused =
        |r: std::io::Result<()>| r.is_err_and(|e| e.kind() == std::io::ErrorKind::InvalidInput);
    assert!(refused(open_release("https://example.com/")));
    assert!(refused(open_release(
        "https://steamcommunity.com/sharedfiles/filedetails/?id=1"
    )));
    assert!(refused(open_release(&format!(
        "{RELEASES_PREFIX}tag/v1 .0.0"
    ))));
    // A release page is not a Steam page either: links from a pack stay Steam-only.
    assert!(refused(open_url(&format!("{RELEASES_PREFIX}tag/v1.0.0"))));
}
