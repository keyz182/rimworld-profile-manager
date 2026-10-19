//! T82c: "Restore and download…" through Valve's steamcmd, for a machine where Steam cannot be
//! reached. The core's tests cover downloading, unpacking and running steamcmd; these cover
//! what the window adds: when the button is there, the first-use consent, the changed-file
//! question, and that the job makes an OFFLINE profile. steamcmd is replaced by a fake, so no
//! test starts a process or touches the network.

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rwpm_core::consent::{ConsentStore, Scope};
use rwpm_core::fetch::{Fresh, Outcome as Fetched, Status};
use rwpm_core::steamcmd::{self, run::Layout};

use super::backups::wait_for;
use super::builds::{clicked, found};
use super::modpacks::assert_on_screen;
use super::pack_fetch::mode_of;
use super::pack_open::{entry, make_pack, preview, shown};
use super::*;
use crate::gui::modpack_open::PackPreview;
use crate::gui::pack_fetch::CmdRunner;

/// A pack with `missing` mods to get (ids 101…), no Steam route, and steamcmd available.
fn with_cmd(sb: &Sandbox, missing: u64) -> PackPreview {
    let mods = (0..missing)
        .map(|i| entry(&format!("m.mod{i}"), &format!("Mod {i}"), Some(101 + i)))
        .collect();
    let pack = make_pack(sb, mods, None, &[]);
    let mut p = preview(sb, &pack, true);
    p.steam = None;
    p.cmd = true;
    p
}

/// What the fake steamcmd was asked.
#[derive(Default)]
struct Calls {
    /// The recorded fingerprint each install call was given.
    installs: Mutex<Vec<Option<String>>>,
    /// How many mods each download call was asked for.
    downloads: Mutex<Vec<usize>>,
}

type InstallResult = Result<Option<String>, steamcmd::Error>;

fn runner(
    calls: &Arc<Calls>,
    install: fn(Option<&str>) -> InstallResult,
    answer: fn(usize) -> Status,
) -> CmdRunner {
    let (a, b) = (Arc::clone(calls), Arc::clone(calls));
    CmdRunner {
        install: Arc::new(
            move |_: &Layout, _: &rwpm_core::consent::Consent, hash: Option<&str>| {
                a.installs
                    .lock()
                    .expect("installs")
                    .push(hash.map(str::to_owned));
                install(hash)
            },
        ),
        download: Arc::new(
            move |_: &Layout,
                  wanted: &[(u64, String)],
                  _: &AtomicBool,
                  _: Duration,
                  progress: &mut dyn FnMut(usize, usize)| {
                b.downloads.lock().expect("downloads").push(wanted.len());
                progress(1, wanted.len());
                wanted
                    .iter()
                    .enumerate()
                    .map(|(i, (id, _))| Fetched {
                        id: *id,
                        status: answer(i),
                    })
                    .collect()
            },
        ),
    }
}

fn first_use(hash: Option<&str>) -> InstallResult {
    Ok(hash.is_none().then(|| "h1".to_owned()))
}

fn all_done(_: usize) -> Status {
    Status::Done(Fresh::New)
}

fn consented(sb: &Sandbox) -> bool {
    let ctx = sb.ctx();
    ConsentStore::open(&ctx.app_dir)
        .find(Scope::Steamcmd, Layout::new(&ctx.app_dir).root())
        .expect("consent file")
        .is_some()
}

fn recorded_hash(sb: &Sandbox) -> Option<String> {
    sb.ctx().settings().expect("settings").steamcmd_sha256
}

fn profile_count(sb: &Sandbox) -> usize {
    sb.ctx()
        .store()
        .expect("store")
        .list()
        .expect("list")
        .profiles
        .len()
}

#[test]
fn the_button_is_offered_only_when_steam_is_not_usable_and_mods_are_missing() {
    let sb = Sandbox::new();
    let app = shown(&sb, with_cmd(&sb, 2));
    let messages = clicked(&app, text::DOWNLOAD_MENU);
    assert!(
        matches!(messages.as_slice(), [Message::PackSteamcmd]),
        "{messages:?}"
    );
    assert!(found(&app, text::CMD_NOTE));
    assert!(found(&app, text::OPEN_PACK_IT), "plain Restore stays");

    let mut with_steam = with_cmd(&sb, 2);
    with_steam.steam = Some(crate::gui::pack_fetch::SteamRoute {
        helper: sb.path("rwpm-steam"),
        workshop_dir: sb.path("workshop"),
    });
    let app = shown(&sb, with_steam);
    assert!(
        !found(&app, text::CMD_NOTE),
        "Steam is usable, so no steamcmd"
    );
    assert!(found(&app, text::SUBSCRIBE_MENU));

    assert!(!found(&shown(&sb, with_cmd(&sb, 0)), text::DOWNLOAD_MENU));
    let mut off = with_cmd(&sb, 2);
    off.cmd = false;
    assert!(!found(&shown(&sb, off), text::DOWNLOAD_MENU));
}

#[test]
fn the_first_time_asks_before_anything_is_downloaded_and_cancel_changes_nothing() {
    let sb = Sandbox::new();
    let mut app = shown(&sb, with_cmd(&sb, 2));
    let calls = Arc::new(Calls::default());
    app.cmd = runner(&calls, first_use, all_done);
    let before = profile_count(&sb);
    let _ = app.update(Message::PackSteamcmd);
    assert!(matches!(app.dialog, Some(Dialog::ConsentSteamcmd(_))));
    assert!(found(&app, text::CMD_CONSENT_ABOUT));
    for point in text::CMD_CONSENT_POINTS {
        assert!(found(&app, point), "{point}");
    }
    assert!(!consented(&sb), "nothing is recorded before they agree");
    let _ = app.update(Message::Cancel);
    assert!(app.dialog.is_none());
    assert!(!consented(&sb));
    assert!(app.job.is_none());
    assert_eq!(profile_count(&sb), before, "no profile");
    assert!(calls.installs.lock().expect("installs").is_empty());
}

#[test]
fn agreeing_records_it_and_shows_the_mods_and_the_next_time_skips_the_question() {
    let sb = Sandbox::new();
    let mut app = shown(&sb, with_cmd(&sb, 3));
    let _ = app.update(Message::PackSteamcmd);
    let _ = app.update(Message::Confirm);
    assert!(consented(&sb));
    assert!(matches!(app.dialog, Some(Dialog::ConfirmSubscribe(_))));
    for name in ["Mod 0", "Mod 1", "Mod 2"] {
        assert!(found(&app, name), "{name}");
    }
    assert!(found(&app, &text::download_about(3)));
    assert!(found(&app, text::CMD_VIA));
    assert!(found(&app, text::SUBSCRIBE_TRUST), "the warning stays");
    assert!(!found(&app, text::SUBSCRIBE_KEEPS), "nothing is subscribed");
    assert!(app.job.is_none(), "nothing starts before the player agrees");

    let mut again = shown(&sb, with_cmd(&sb, 3));
    let _ = again.update(Message::PackSteamcmd);
    assert!(
        matches!(again.dialog, Some(Dialog::ConfirmSubscribe(_))),
        "already agreed"
    );
}

/// The job thread is real; steamcmd is the fake. Fails if a RimWorld is open.
fn restore(sb: &Sandbox, name: &str, cmd: CmdRunner, missing: u64) {
    let mut app = shown(sb, with_cmd(sb, missing));
    app.cmd = cmd;
    let _ = app.update(Message::Input(Field::Name, name.into()));
    let _ = app.update(Message::PackSteamcmd);
    if matches!(app.dialog, Some(Dialog::ConsentSteamcmd(_))) {
        let _ = app.update(Message::Confirm);
    }
    let _ = app.update(Message::Confirm);
    assert!(app.job.is_some());
    wait_for("the restored profile", || mode_of(sb, name).is_some());
    std::thread::sleep(Duration::from_millis(300));
}

#[test]
fn a_steamcmd_restore_makes_an_offline_profile_and_keeps_the_first_fingerprint() {
    let sb = Sandbox::new();
    let calls = Arc::new(Calls::default());
    restore(&sb, "Cmd Pack", runner(&calls, first_use, all_done), 2);
    let (mode, _) = mode_of(&sb, "Cmd Pack").expect("profile");
    assert_eq!(mode, Mode::Offline);
    assert_eq!(*calls.installs.lock().expect("installs"), [None]);
    assert_eq!(*calls.downloads.lock().expect("downloads"), [2]);
    assert_eq!(recorded_hash(&sb).as_deref(), Some("h1"));

    // The next restore is given that fingerprint to compare with.
    restore(&sb, "Cmd Pack Two", runner(&calls, first_use, all_done), 2);
    assert_eq!(
        *calls.installs.lock().expect("installs"),
        [None, Some("h1".to_owned())]
    );
}

#[test]
fn a_failed_mod_leaves_the_profile_live() {
    let sb = Sandbox::new();
    let calls = Arc::new(Calls::default());
    let half = |i| {
        if i == 0 {
            Status::Done(Fresh::New)
        } else {
            Status::Failed("no".into())
        }
    };
    restore(&sb, "Half Cmd", runner(&calls, first_use, half), 2);
    let (mode, _) = mode_of(&sb, "Half Cmd").expect("profile");
    assert_eq!(mode, Mode::Live, "not every mod arrived");
}

#[test]
fn a_pack_of_six_hundred_mods_is_not_refused() {
    let sb = Sandbox::new();
    let calls = Arc::new(Calls::default());
    restore(&sb, "Big Cmd", runner(&calls, first_use, all_done), 600);
    assert_eq!(*calls.downloads.lock().expect("downloads"), [600]);
    let (mode, _) = mode_of(&sb, "Big Cmd").expect("profile");
    assert_eq!(mode, Mode::Offline);
}

fn changed(_: Option<&str>) -> InstallResult {
    Err(steamcmd::Error::Changed {
        first: "aaaa".into(),
        found: "bbbb".into(),
    })
}

#[test]
fn a_changed_file_stops_before_any_profile_and_asks_whether_to_trust_it() {
    let sb = Sandbox::new();
    let calls = Arc::new(Calls::default());
    let mut app = shown(&sb, with_cmd(&sb, 2));
    app.cmd = runner(&calls, changed, all_done);
    let before = profile_count(&sb);
    let preview = {
        let _ = app.update(Message::PackSteamcmd);
        let _ = app.update(Message::Confirm);
        let _ = app.update(Message::Confirm);
        wait_for("the install to be tried", || {
            !calls.installs.lock().expect("installs").is_empty()
        });
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(profile_count(&sb), before, "no profile was made");
        assert!(calls.downloads.lock().expect("downloads").is_empty());
        shown(&sb, with_cmd(&sb, 2))
    };
    // What the job reports is turned into the question.
    let mut app = preview;
    let Some(Dialog::OpenPack(p)) = app.dialog.take() else {
        panic!("{:?}", app.dialog);
    };
    let _ = app.finished(Ok(Outcome::SteamcmdChanged {
        preview: p,
        first: "aaaa".into(),
        found: "bbbb".into(),
    }));
    assert!(matches!(app.dialog, Some(Dialog::TrustSteamcmd { .. })));
    assert!(found(&app, text::CMD_CHANGED_TITLE));
    assert!(found(&app, &text::cmd_changed_about("aaaa", "bbbb")));
    assert_on_screen(&app, text::CMD_TRUST_IT);
}

#[test]
fn trusting_the_new_file_forgets_the_old_fingerprint_and_goes_on() {
    let sb = Sandbox::new();
    let mut settings = sb.ctx().settings().expect("settings");
    settings.steamcmd_sha256 = Some("old".into());
    settings.save(&sb.ctx().app_dir).expect("save");
    let calls = Arc::new(Calls::default());
    let mut app = shown(&sb, with_cmd(&sb, 1));
    app.cmd = runner(&calls, first_use, all_done);
    let Some(Dialog::OpenPack(mut p)) = app.dialog.take() else {
        panic!("{:?}", app.dialog);
    };
    p.via_cmd = true;
    p.offline = true;
    p.plan.name = "Trusted".into();
    let _ = app.finished(Ok(Outcome::SteamcmdChanged {
        preview: p,
        first: "old".into(),
        found: "new".into(),
    }));
    let _ = ConsentStore::open(&sb.ctx().app_dir)
        .record(Scope::Steamcmd, Layout::new(&sb.ctx().app_dir).root());
    let _ = app.update(Message::Confirm);
    wait_for("the restored profile", || mode_of(&sb, "Trusted").is_some());
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(
        *calls.installs.lock().expect("installs"),
        [None],
        "the old fingerprint was forgotten first"
    );
    assert_eq!(recorded_hash(&sb).as_deref(), Some("h1"));
}

#[test]
fn the_consent_and_changed_file_dialogs_keep_their_buttons_on_screen() {
    let sb = Sandbox::new();
    let mut app = shown(&sb, with_cmd(&sb, 2));
    let _ = app.update(Message::PackSteamcmd);
    assert_on_screen(&app, text::CMD_CONSENT_TITLE);
    assert_on_screen(&app, text::CMD_CONSENT_GO);
    let _ = app.update(Message::Cancel);
    let big = shown(&sb, with_cmd(&sb, 250));
    assert_on_screen(&big, text::DOWNLOAD_MENU);
    assert_on_screen(&big, text::OPEN_PACK_IT);
}
