//! T83c: "Restore and subscribe…", the Cancel button and "Remove subscriptions from the pack".
//! The core's own tests cover the helper runner and the identity check; these cover what the
//! window adds: when the buttons are there, what the confirm says, the record of what a pack
//! subscribed to, and Cancel. The helper is replaced by a fake, so no test starts a process.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use rwpm_core::fetch::{self, Fresh, Op, Outcome as Fetched, Request, Status, record};

use super::backups::{selected, wait_for};
use super::builds::{clicked, found};
use super::modpacks::{assert_not_squeezed, assert_on_screen};
use super::pack_open::{entry, make_pack, preview, shown};
use super::*;
use crate::gui::pack_fetch::{Fetcher, SteamRoute};

/// A preview of a pack with `missing` mods to get (ids 101…), and a Steam route.
fn with_steam(sb: &Sandbox, missing: u64) -> crate::gui::modpack_open::PackPreview {
    let mods = (0..missing)
        .map(|i| entry(&format!("m.mod{i}"), &format!("Mod {i}"), Some(101 + i)))
        .collect();
    let pack = make_pack(sb, mods, None, &[]);
    let mut p = preview(sb, &pack, true);
    p.steam = Some(SteamRoute {
        helper: sb.path("rwpm-steam"),
        workshop_dir: sb.path("workshop"),
    });
    p
}

/// A fetcher that answers every mod with `answer(id, position)`.
fn fake(answer: fn(u64, usize) -> Status) -> Fetcher {
    Fetcher(Arc::new(
        move |_: &Path, req: &Request<'_>, _: &AtomicBool, progress: &mut dyn FnMut(u64, u64)| {
            progress(50, 100);
            req.wanted
                .iter()
                .enumerate()
                .map(|(i, (id, _))| Fetched {
                    id: *id,
                    status: answer(*id, i),
                })
                .collect()
        },
    ))
}

#[test]
fn restore_and_subscribe_is_offered_only_with_a_steam_route_and_mods_to_get() {
    let sb = Sandbox::new();
    let app = shown(&sb, with_steam(&sb, 2));
    let messages = clicked(&app, text::SUBSCRIBE_MENU);
    assert!(
        matches!(messages.as_slice(), [Message::PackSubscribe]),
        "{messages:?}"
    );
    assert!(found(&app, text::OPEN_PACK_IT), "plain Restore stays");

    let mut no_route = with_steam(&sb, 2);
    no_route.steam = None;
    assert!(!found(&shown(&sb, no_route), text::SUBSCRIBE_MENU));

    let nothing_missing = with_steam(&sb, 0);
    assert!(!found(&shown(&sb, nothing_missing), text::SUBSCRIBE_MENU));

    let mut why = with_steam(&sb, 2);
    why.steam = None;
    why.steam_note = Some(text::steam_unreachable("Steam is not running"));
    let app = shown(&sb, why);
    assert!(found(
        &app,
        &text::steam_unreachable("Steam is not running")
    ));
}

#[test]
fn the_confirm_lists_the_names_with_the_warning_before_anything_is_done() {
    let sb = Sandbox::new();
    let mut app = shown(&sb, with_steam(&sb, 3));
    let profiles = |sb: &Sandbox| {
        sb.ctx()
            .store()
            .expect("store")
            .list()
            .expect("list")
            .profiles
            .len()
    };
    let before = profiles(&sb);
    let _ = app.update(Message::PackSubscribe);
    assert!(matches!(app.dialog, Some(Dialog::ConfirmSubscribe(_))));
    for name in ["Mod 0", "Mod 1", "Mod 2"] {
        assert!(found(&app, name), "{name}");
    }
    assert!(found(&app, &text::subscribe_about(3)));
    assert!(found(&app, text::SUBSCRIBE_TRUST));
    assert!(found(&app, text::SUBSCRIBE_KEEPS));
    assert!(app.job.is_none(), "nothing starts before the player agrees");
    assert_eq!(profiles(&sb), before, "no profile yet");
}

#[test]
fn a_subscribe_without_a_route_does_nothing() {
    let sb = Sandbox::new();
    let mut no_route = with_steam(&sb, 2);
    no_route.steam = None;
    let mut app = shown(&sb, no_route);
    let _ = app.update(Message::PackSubscribe);
    assert!(
        matches!(app.dialog, Some(Dialog::OpenPack(_))),
        "back to the question"
    );
}

/// The job thread is real; the helper is the fake above. Fails if a RimWorld is open.
#[test]
fn subscribing_restores_the_profile_records_only_new_ids_and_says_what_happened() {
    let sb = Sandbox::new();
    let mut app = shown(&sb, with_steam(&sb, 4));
    app.fetcher = fake(|_, i| match i {
        0 => Status::Done(Fresh::New),
        1 => Status::Done(Fresh::Already),
        2 => Status::Failed("Steam said no".into()),
        _ => Status::Mismatch {
            expected: "m.mod3".into(),
            found: "evil.mod".into(),
        },
    });
    // The name is typed in the Open dialog; the confirm comes after it.
    let _ = app.update(Message::Input(Field::Name, "Subscribed Pack".into()));
    let _ = app.update(Message::PackSubscribe);
    let _ = app.update(Message::Confirm);
    assert!(app.job.is_some());
    assert!(app.cancel.is_some(), "a fetch can be cancelled");
    let store = sb.ctx().store().expect("store");
    let made = || {
        store
            .list()
            .ok()
            .and_then(|l| l.profiles.into_iter().find(|p| p.name == "Subscribed Pack"))
    };
    wait_for("the restored profile", || made().is_some());
    let profile = made().expect("profile");
    let dir = store.root().join(profile.id.as_str());
    wait_for("the record", || !record::read(&dir).is_empty());
    assert_eq!(
        record::read(&dir),
        [101],
        "only the subscription this run made"
    );

    let line = text::pack_subscribed(
        "Subscribed Pack",
        1,
        1,
        &[("Mod 2".to_owned(), "Steam said no".to_owned())],
        &["Mod 3".to_owned()],
        0,
    );
    let _ = app.update(Message::Done(Ok(Outcome::Restored {
        id: profile.id.clone(),
        line: line.clone(),
    })));
    assert!(app.cancel.is_none(), "cleared when the job ends");
    assert_eq!(app.notice, Some(Notice::Info(line)));
}

#[test]
fn cancel_stops_the_fetch_and_nothing_is_recorded() {
    let sb = Sandbox::new();
    let mut app = shown(&sb, with_steam(&sb, 2));
    app.fetcher = Fetcher(Arc::new(
        |_: &Path, req: &Request<'_>, cancel: &AtomicBool, _: &mut dyn FnMut(u64, u64)| {
            let started = Instant::now();
            while !cancel.load(Ordering::Relaxed) && started.elapsed() < Duration::from_secs(20) {
                std::thread::sleep(Duration::from_millis(10));
            }
            req.wanted
                .iter()
                .map(|(id, _)| Fetched {
                    id: *id,
                    status: Status::Failed("cancelled".into()),
                })
                .collect()
        },
    ));
    let _ = app.update(Message::Input(Field::Name, "Cancelled Pack".into()));
    let _ = app.update(Message::PackSubscribe);
    let _ = app.update(Message::Confirm);
    let flag = app.cancel.clone().expect("cancellable");
    assert!(!flag.load(Ordering::Relaxed));
    // The footer shows the button, and it sends the message.
    let messages = clicked(&app, text::CANCEL);
    assert!(
        matches!(messages.as_slice(), [Message::CancelJob]),
        "{messages:?}"
    );
    let _ = app.update(Message::CancelJob);
    assert!(flag.load(Ordering::Relaxed));

    let store = sb.ctx().store().expect("store");
    let made = || {
        store
            .list()
            .ok()
            .and_then(|l| l.profiles.into_iter().find(|p| p.name == "Cancelled Pack"))
    };
    wait_for("the profile", || made().is_some());
    let dir = store.root().join(made().expect("profile").id.as_str());
    std::thread::sleep(Duration::from_millis(300));
    assert!(record::read(&dir).is_empty());
}

#[test]
fn a_job_without_cancel_shows_no_cancel_button() {
    let sb = Sandbox::new();
    let mut app = sb.app();
    app.job = Some(crate::gui::Job {
        label: "Copying".into(),
        done: 0,
        total: 10,
        percent: false,
    });
    assert!(!found(&app, text::CANCEL));
}

/// A profile that a pack subscribed this player to two mods for, and a helper file beside the
/// app so the Steam route counts as there.
fn subscribed_profile(sb: &Sandbox) -> (ProfileId, std::path::PathBuf) {
    let id = super::modpacks::live_profile(sb, "From A Pack", &["ludeon.rimworld"]);
    let dir = sb.ctx().store().expect("store").root().join(id.as_str());
    record::add(&dir, &[11, 22]).expect("record");
    std::fs::write(sb.path(fetch::HELPER), "").expect("helper file");
    (id, dir)
}

#[test]
fn removing_subscriptions_asks_then_unsubscribes_exactly_the_recorded_ones() {
    let sb = Sandbox::new();
    let (id, dir) = subscribed_profile(&sb);
    let mut app = selected(&sb, &id);
    let _ = app.update(Message::ToggleMenu);
    assert!(found(&app, text::REMOVE_PACK_SUBS));
    let _ = app.update(Message::RemovePackSubs);
    assert!(matches!(
        app.dialog,
        Some(Dialog::ConfirmUnsubscribe { count: 2, .. })
    ));
    assert!(found(&app, &text::unsubscribe_about(2)));
    app.fetcher = fake(|_, i| {
        if i == 0 {
            Status::Done(Fresh::New)
        } else {
            Status::Failed("no".into())
        }
    });
    let _ = app.update(Message::Confirm);
    assert!(app.job.is_some());
    wait_for("the record to shrink", || record::read(&dir) == [22]);
    assert_eq!(
        record::read(&dir),
        [22],
        "the one that failed stays recorded"
    );
}

#[test]
fn nothing_to_remove_means_no_menu_item_and_a_plain_answer() {
    let sb = Sandbox::new();
    let id = super::modpacks::live_profile(&sb, "Loose", &["ludeon.rimworld"]);
    let mut app = selected(&sb, &id);
    let _ = app.update(Message::ToggleMenu);
    assert!(!found(&app, text::REMOVE_PACK_SUBS));
    let _ = app.update(Message::RemovePackSubs);
    assert!(app.dialog.is_none());
    assert_eq!(
        app.notice,
        Some(Notice::Info(text::NO_PACK_SUBS.to_owned()))
    );
}

#[test]
fn without_the_helper_removing_says_so() {
    let sb = Sandbox::new();
    let (id, _) = subscribed_profile(&sb);
    std::fs::remove_file(sb.path(fetch::HELPER)).expect("remove helper");
    let mut app = selected(&sb, &id);
    let _ = app.update(Message::RemovePackSubs);
    let _ = app.update(Message::Confirm);
    assert!(app.job.is_none());
    assert_eq!(
        app.notice,
        Some(Notice::Error(text::STEAM_HELPER_MISSING.to_owned()))
    );
}

#[test]
fn the_confirm_keeps_its_title_and_buttons_on_screen_in_the_smallest_window() {
    let sb = Sandbox::new();
    let mut app = shown(&sb, with_steam(&sb, 250));
    let _ = app.update(Message::PackSubscribe);
    assert_on_screen(&app, text::SUBSCRIBE_TITLE);
    assert_on_screen(&app, text::SUBSCRIBE_IT);
    assert_not_squeezed(&app, text::SUBSCRIBE_TRUST);
    // The "and N more" line is small text: any real height will do.
    let more = super::modpacks::smallest_window_bounds(&app, &text::and_more(50));
    assert!(more.height > 10.0, "squeezed to {} px", more.height);

    let big = shown(&sb, with_steam(&sb, 250));
    assert_on_screen(&big, text::SUBSCRIBE_MENU);
    let _ = Op::Download;
}

/// A fetcher that records the operation it was asked for, then answers with `answer`.
fn recording(ops: &Arc<std::sync::Mutex<Vec<Op>>>, answer: fn(u64, usize) -> Status) -> Fetcher {
    let ops = Arc::clone(ops);
    Fetcher(Arc::new(
        move |_: &Path, req: &Request<'_>, _: &AtomicBool, _: &mut dyn FnMut(u64, u64)| {
            ops.lock().expect("ops").push(req.op);
            req.wanted
                .iter()
                .enumerate()
                .map(|(i, (id, _))| Fetched {
                    id: *id,
                    status: answer(*id, i),
                })
                .collect()
        },
    ))
}

pub(super) fn mode_of(sb: &Sandbox, name: &str) -> Option<(Mode, std::path::PathBuf)> {
    let store = sb.ctx().store().ok()?;
    let profile = store
        .list()
        .ok()?
        .profiles
        .into_iter()
        .find(|p| p.name == name)?;
    let dir = store.root().join(profile.id.as_str());
    let file = ProfileFile::read(&dir.join("profile.toml")).ok()?;
    Some((file.mode, dir))
}

#[test]
fn the_offline_box_is_offered_with_the_steam_route_and_changes_the_main_button() {
    let sb = Sandbox::new();
    let mut app = shown(&sb, with_steam(&sb, 2));
    assert!(found(&app, text::PACK_OFFLINE_TICK));
    assert!(found(&app, text::SUBSCRIBE_MENU));
    assert!(!found(&app, text::DOWNLOAD_MENU));
    let _ = app.update(Message::PackOffline(true));
    assert!(found(&app, text::DOWNLOAD_MENU));
    assert!(!found(&app, text::SUBSCRIBE_MENU));
    let _ = app.update(Message::PackOffline(false));
    assert!(found(&app, text::SUBSCRIBE_MENU));

    let mut no_route = with_steam(&sb, 2);
    no_route.steam = None;
    let mut app = shown(&sb, no_route);
    assert!(!found(&app, text::PACK_OFFLINE_TICK));
    let _ = app.update(Message::PackOffline(true));
    let Some(Dialog::OpenPack(p)) = &app.dialog else {
        panic!("{:?}", app.dialog);
    };
    assert!(!p.offline, "no Steam route, so no OFFLINE download");
}

#[test]
fn the_offline_confirm_says_download_not_subscribe() {
    let sb = Sandbox::new();
    let mut app = shown(&sb, with_steam(&sb, 3));
    let _ = app.update(Message::PackOffline(true));
    let _ = app.update(Message::PackSubscribe);
    assert!(found(&app, &text::download_about(3)));
    assert!(found(&app, text::DOWNLOAD_KEEPS));
    assert!(found(&app, text::SUBSCRIBE_TRUST), "the warning stays");
    assert!(
        !found(&app, text::SUBSCRIBE_KEEPS),
        "no subscription is made"
    );
    assert!(found(&app, "Mod 0"));
}

/// Drives the whole OFFLINE restore with a fake helper and returns what was asked of it.
fn offline_restore(name: &str, answer: fn(u64, usize) -> Status) -> (Sandbox, Vec<Op>) {
    let sb = Sandbox::new();
    let ops = Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut app = shown(&sb, with_steam(&sb, 2));
    app.fetcher = recording(&ops, answer);
    let _ = app.update(Message::PackOffline(true));
    let _ = app.update(Message::Input(Field::Name, name.into()));
    let _ = app.update(Message::PackSubscribe);
    let _ = app.update(Message::Confirm);
    assert!(app.job.is_some());
    wait_for("the restored profile", || mode_of(&sb, name).is_some());
    // The job ends a moment after the profile exists: wait for the fetch to have run too.
    wait_for("the helper to be asked", || {
        !ops.lock().expect("ops").is_empty()
    });
    std::thread::sleep(Duration::from_millis(300));
    let asked = ops.lock().expect("ops").clone();
    (sb, asked)
}

/// The job thread is real, the helper is the fake. Fails if a RimWorld is open.
#[test]
fn an_offline_restore_downloads_never_subscribes_and_makes_the_profile_offline() {
    let (sb, asked) = offline_restore("Offline Pack", |_, _| Status::Done(Fresh::New));
    assert_eq!(asked, [Op::Download], "never a subscribe");
    let (mode, dir) = mode_of(&sb, "Offline Pack").expect("profile");
    assert_eq!(mode, Mode::Offline);
    assert!(
        record::read(&dir).is_empty(),
        "nothing subscribed, nothing to remove"
    );
}

#[test]
fn one_failed_mod_leaves_the_profile_live() {
    let (sb, asked) = offline_restore("Partly Fetched", |_, i| {
        if i == 0 {
            Status::Done(Fresh::New)
        } else {
            Status::Failed("Steam said no".into())
        }
    });
    assert_eq!(asked, [Op::Download]);
    let (mode, dir) = mode_of(&sb, "Partly Fetched").expect("profile");
    assert_eq!(mode, Mode::Live, "not every mod arrived");
    assert!(record::read(&dir).is_empty());
}

#[test]
fn a_mod_that_is_not_the_one_promised_leaves_the_profile_live() {
    let (sb, _) = offline_restore("Wrong Mod", |_, i| {
        if i == 0 {
            Status::Mismatch {
                expected: "m.mod0".into(),
                found: "evil.mod".into(),
            }
        } else {
            Status::Done(Fresh::New)
        }
    });
    let (mode, _) = mode_of(&sb, "Wrong Mod").expect("profile");
    assert_eq!(mode, Mode::Live);
}

#[test]
fn the_open_dialog_with_every_note_and_the_offline_box_keeps_its_buttons_on_screen() {
    let sb = Sandbox::new();
    let mut p = with_steam(&sb, 250);
    p.newer = vec!["Harmony".into()];
    p.game_note = Some(text::pack_version_differs("0.0.1 rev1", "1.6.4871 rev598"));
    p.missing_dlc = vec!["ludeon.rimworld.royalty".into()];
    p.steam_note = Some(text::steam_unreachable("Steam is not running"));
    let app = shown(&sb, p);
    assert_on_screen(&app, text::OPEN_PACK_TITLE);
    assert_on_screen(&app, text::PACK_OFFLINE_TICK);
    assert_on_screen(&app, text::SUBSCRIBE_MENU);
    assert_on_screen(&app, text::OPEN_PACK_IT);
}
