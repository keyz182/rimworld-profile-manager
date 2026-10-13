//! T56: with no Steam install, a pinned profile with a real stored build still plays, and
//! its mod-list and RimSort buttons work; an unpinned one, or a pinned one whose build is
//! gone, is held back with a reason (T55/T60 deferred this).

use rwpm_core::profile::health::Problem;

use super::builds::{KEY, clicked};
use super::*;

/// A real stored build, a profile pinned to it, and an app that cannot find Steam's game.
fn pinned_without_steam() -> (Sandbox, App, ProfileId) {
    let sb = Sandbox::new();
    sb.steam_install("1111");
    sb.stored_build();
    let id = sb.pin("Pinned");
    let mut app = sb.app();
    let _ = app.update(Message::Select(id.clone()));
    app.game = Err("RimWorld was not found".to_owned());
    (sb, app, id)
}

#[test]
fn play_runs_the_build_with_no_steam_install() {
    let (sb, mut app, id) = pinned_without_steam();
    let messages = clicked(&app, text::LAUNCH);
    assert!(
        matches!(messages.as_slice(), [Message::Launch]),
        "{messages:?}"
    );
    for m in messages {
        let _ = app.update(m);
    }
    assert!(
        !matches!(app.notice, Some(Notice::Error(_))),
        "{:?}",
        app.notice
    );
    let args = sb.launched().expect("fake rwpm ran");
    let build = sb
        .builds()
        .dir_of(&rwpm_core::builds::BuildKey::parse(KEY).expect("key"));
    assert_eq!(args[..3], ["run", id.as_str(), "--"]);
    assert_eq!(args[3], build.join("RimWorldLinux").display().to_string());
}

#[test]
fn freeze_and_rimsort_buttons_work_with_no_steam_install() {
    let (_sb, app, _id) = pinned_without_steam();
    let go = clicked(&app, text::GO_OFFLINE);
    assert!(matches!(go.as_slice(), [Message::GoOffline]), "{go:?}");
    let export = clicked(&app, text::EXPORT);
    assert!(matches!(export.as_slice(), [Message::Export]), "{export:?}");
}

#[test]
fn unpinned_without_steam_is_held_back_on_every_button() {
    let sb = Sandbox::new();
    let id = sb
        .ctx()
        .store()
        .expect("store")
        .create("Loose")
        .expect("create")
        .id;
    let mut app = sb.app();
    let _ = app.update(Message::Select(id));
    app.game = Err("RimWorld was not found".to_owned());
    for label in [text::LAUNCH, text::GO_OFFLINE, text::EXPORT] {
        assert!(clicked(&app, label).is_empty(), "{label} should wait");
    }
}

#[test]
fn pinned_with_a_missing_build_is_held_back_and_says_why() {
    let (_sb, mut app, id) = pinned_without_steam();
    app.health
        .insert(id, vec![Problem::BuildMissing(KEY.to_owned())]);
    for label in [text::LAUNCH, text::GO_OFFLINE] {
        assert!(clicked(&app, label).is_empty(), "{label} should wait");
    }
    // The RimSort button sits below the simulator's window once the problem line shows;
    // it is gated by the same `game_problem` the other two use.
    let p = app.current().expect("profile");
    assert_eq!(app.game_problem(p), Some(text::build_missing(KEY)));
}

/// L1: a build whose `build.toml` is broken reads as "build missing" in the log actions,
/// the same words health and `rwpm run` use, not a raw parse error.
#[test]
fn a_broken_build_reads_as_build_missing_for_the_logs() {
    let (sb, app, _id) = pinned_without_steam();
    let dir = sb
        .builds()
        .dir_of(&rwpm_core::builds::BuildKey::parse(KEY).expect("key"));
    std::fs::remove_file(dir.join("RimWorldLinux")).expect("break the build");
    std::fs::write(dir.join("build.toml"), "schema = 99\n").expect("break build.toml");
    assert_eq!(
        app.log_game().err(),
        Some(GameError::BuildMissing(KEY.to_owned()))
    );
    assert_eq!(
        app.effective_game().err(),
        Some(GameError::BuildMissing(KEY.to_owned()))
    );
}
