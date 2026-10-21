//! "Back up…" and "Restore backup…". The core's own tests cover the file format and
//! every refusal; these cover what the window adds: who is offered what, the notices, the
//! blocking rules, and one real backup and restore through the job thread.

use rwpm_core::backup::{self, Includes};

use super::builds::{KEY, clicked, found};
use super::*;
use crate::gui::Job;
use crate::gui::backup::{GameFiles, Tick};

/// No RimWorld running, whatever is on this machine.
pub(super) struct Idle;

impl rwpm_core::profile::RunningProbe for Idle {
    fn is_running(&self, _: &Path) -> bool {
        false
    }

    fn any_game(&self) -> bool {
        false
    }
}

pub(super) fn inc(saves: bool, logs: bool, build: bool) -> Includes {
    Includes { saves, logs, build }
}

/// A profile pinned to the stored build and made OFFLINE.
pub(super) fn offline_pinned(sb: &Sandbox) -> ProfileId {
    sb.steam_install("1111");
    sb.stored_build();
    let id = sb.pin("Pinned Run");
    let dir = sb.ctx().store().expect("store").root().join(id.as_str());
    let mut file = ProfileFile::read(&dir.join("profile.toml")).expect("read");
    file.mode = Mode::Offline;
    file.write_atomic(&dir.join("profile.toml")).expect("write");
    id
}

pub(super) fn selected(sb: &Sandbox, id: &ProfileId) -> App {
    let mut app = sb.app();
    let _ = app.update(Message::Select(id.clone()));
    app
}

/// A backup made through core, as the job thread would.
pub(super) fn make_backup(sb: &Sandbox, id: &ProfileId, includes: Includes) -> PathBuf {
    let (store, builds) = (sb.ctx().store().expect("store"), sb.builds());
    let plan = backup::plan_backup(&store, &builds, id, includes).expect("plan");
    let dest = sb.path("made.rimrim");
    backup::apply_backup(&plan, &dest, &sb.path("app"), &Idle, &mut |_| {}).expect("backup");
    dest
}

pub(super) fn wait_for(what: &str, ready: impl Fn() -> bool) {
    for _ in 0..500 {
        if ready() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    panic!("timed out waiting for {what}");
}

#[test]
fn backup_is_in_the_menu_of_offline_profiles_only() {
    let sb = Sandbox::new();
    let offline = sb.offline_profile("Frozen", &[]);
    let live = sb
        .ctx()
        .store()
        .expect("store")
        .create("Loose")
        .expect("create")
        .id;
    for (id, shown) in [
        (offline, true),
        (live, false),
        (ProfileId::default_id(), false),
    ] {
        let mut app = selected(&sb, &id);
        let _ = app.update(Message::ToggleMenu);
        assert_eq!(found(&app, text::BACKUP_MENU), shown, "{id}");
    }
}

#[test]
fn restore_is_on_the_list_and_waits_while_something_runs() {
    let sb = Sandbox::new();
    let mut app = sb.app();
    let messages = clicked(&app, text::RESTORE_MENU);
    assert!(
        matches!(messages.as_slice(), [Message::RestoreBackup]),
        "{messages:?}"
    );
    app.job = Some(Job {
        label: "Copying".into(),
        done: 0,
        total: 0,
        percent: false,
    });
    assert!(clicked(&app, text::RESTORE_MENU).is_empty());
}

#[test]
fn the_backup_dialog_starts_with_saves_only_and_ticks_build_only_when_stored() {
    let sb = Sandbox::new();
    let loose = sb.offline_profile("Frozen", &[]);
    let mut app = selected(&sb, &loose);
    let _ = app.update(Message::Backup);
    let Some(Dialog::Backup(draft)) = app.dialog.clone() else {
        panic!("{:?}", app.dialog);
    };
    assert_eq!(draft.includes, inc(true, false, false));
    assert_eq!(draft.game, GameFiles::NotPinned);
    assert!(found(&app, text::NOTICE_MODS));
    assert!(!found(&app, text::NOTICE_GAME));
    let _ = app.update(Message::BackupTick(Tick::Build, true));
    let Some(Dialog::Backup(draft)) = &app.dialog else {
        panic!("{:?}", app.dialog);
    };
    assert!(!draft.includes.build, "no game files to include");

    let sb = Sandbox::new();
    let pinned = offline_pinned(&sb);
    let mut app = selected(&sb, &pinned);
    let _ = app.update(Message::Backup);
    let _ = app.update(Message::BackupTick(Tick::Build, true));
    let _ = app.update(Message::BackupTick(Tick::Logs, true));
    let Some(Dialog::Backup(draft)) = &app.dialog else {
        panic!("{:?}", app.dialog);
    };
    assert!(matches!(&draft.game, GameFiles::Stored { key, .. } if key == KEY));
    assert_eq!(draft.includes, inc(true, true, true));
    assert!(found(&app, text::NOTICE_GAME), "ticked: the game notice");
    let _ = app.update(Message::BackupTick(Tick::Build, false));
    assert!(!found(&app, text::NOTICE_GAME), "unticked: no game notice");
}

#[test]
fn a_pin_to_a_version_that_is_not_stored_offers_no_game_files() {
    let sb = Sandbox::new();
    let pinned = offline_pinned(&sb);
    let key = rwpm_core::builds::BuildKey::parse(KEY).expect("key");
    std::fs::remove_dir_all(sb.builds().dir_of(&key)).expect("remove build");
    let mut app = selected(&sb, &pinned);
    let _ = app.update(Message::Backup);
    let Some(Dialog::Backup(draft)) = &app.dialog else {
        panic!("{:?}", app.dialog);
    };
    assert_eq!(draft.game, GameFiles::Missing(KEY.to_owned()));
    let _ = app.update(Message::BackupTick(Tick::Build, true));
    let Some(Dialog::Backup(draft)) = &app.dialog else {
        panic!("{:?}", app.dialog);
    };
    assert!(!draft.includes.build);
}

/// The one test that uses the real job thread and the real process probe: it needs the
/// copy to happen (the thread starts when the task is made). It fails if a RimWorld is open.
#[test]
fn a_backup_made_in_the_app_restores_into_a_new_profile() {
    let sb = Sandbox::new();
    let id = sb.offline_profile("Ironman", &[]);
    let saves = sb.ctx().store().expect("store").root().join(id.as_str());
    let saves = saves.join("SaveData/Saves");
    std::fs::create_dir_all(&saves).expect("saves");
    std::fs::write(saves.join("colony.rws"), "save").expect("save");
    std::fs::create_dir_all(sb.path("out")).expect("out");

    let mut app = selected(&sb, &id);
    let _ = app.update(Message::Backup);
    let Some(Dialog::Backup(draft)) = app.dialog.clone() else {
        panic!("{:?}", app.dialog);
    };
    // The player typed no extension: the file still becomes `.rimrim`.
    let typed = sb.path("out/ironman");
    let _ = app.update(Message::BackupPicked(id, draft.includes, Some(typed)));
    assert!(app.job.is_some());
    let backup = sb.path("out/ironman.rimrim");
    wait_for("the backup file", || backup.is_file());

    let mut app = sb.app();
    let _ = app.update(Message::RestorePicked(Some(backup)));
    assert!(found(&app, text::NOTICE_MODS));
    assert!(!found(&app, text::NOTICE_GAME));
    let _ = app.update(Message::Input(Field::Name, "Ironman Two".into()));
    let _ = app.update(Message::Confirm);
    assert!(app.job.is_some());
    let store = sb.ctx().store().expect("store");
    let restored = || {
        store
            .list()
            .ok()
            .and_then(|l| l.profiles.into_iter().find(|p| p.name == "Ironman Two"))
    };
    wait_for("the restored profile", || restored().is_some());
    let profile = restored().expect("restored");
    let dir = store.root().join(profile.id.as_str());
    assert_eq!(
        std::fs::read_to_string(dir.join("SaveData/Saves/colony.rws")).expect("save"),
        "save"
    );
    // The job's own result, fed by hand as the other tests do.
    let line = text::restored("Ironman Two", &[]);
    let _ = app.update(Message::Done(Ok(Outcome::Restored {
        id: profile.id.clone(),
        line: line.clone(),
    })));
    assert_eq!(app.selected, profile.id);
    assert_eq!(app.notice, Some(Notice::Info(line)));
}

#[test]
fn the_restore_dialog_shows_both_notices_and_will_not_start_without_a_name() {
    let sb = Sandbox::new();
    let id = offline_pinned(&sb);
    let backup = make_backup(&sb, &id, inc(true, false, true));
    let mut app = sb.app();
    let _ = app.update(Message::RestorePicked(Some(backup)));
    assert!(found(&app, text::NOTICE_MODS));
    assert!(found(&app, text::NOTICE_GAME), "the backup holds the game");
    let _ = app.update(Message::Input(Field::Name, "   ".into()));
    let _ = app.update(Message::Confirm);
    assert!(
        matches!(app.dialog, Some(Dialog::Restore(_))),
        "Enter must not start what the disabled button refuses"
    );
    assert!(app.job.is_none());
}

#[test]
fn a_file_that_is_not_a_backup_says_so() {
    let sb = Sandbox::new();
    let notes = sb.path("notes.rimrim");
    std::fs::write(&notes, "just words").expect("write");
    let mut app = sb.app();
    let _ = app.update(Message::RestorePicked(Some(notes)));
    assert!(app.dialog.is_none());
    assert!(
        matches!(&app.notice, Some(Notice::Error(e)) if e.contains("not a backup")),
        "{:?}",
        app.notice
    );
}

#[test]
fn closing_a_file_dialog_does_nothing() {
    let sb = Sandbox::new();
    let id = sb.offline_profile("Frozen", &[]);
    let mut app = selected(&sb, &id);
    let _ = app.update(Message::RestorePicked(None));
    let _ = app.update(Message::BackupPicked(id, inc(true, false, false), None));
    assert!(app.dialog.is_none() && app.job.is_none() && app.notice.is_none());
}

#[test]
fn backup_and_restore_wait_for_a_job_or_a_running_game() {
    let sb = Sandbox::new();
    let id = sb.offline_profile("Frozen", &[]);
    let backup = sb.path("later.rimrim");
    let messages = |id: &ProfileId| {
        vec![
            Message::Backup,
            Message::RestoreBackup,
            Message::RestorePicked(Some(sb.path("later.rimrim"))),
            Message::BackupPicked(id.clone(), inc(true, false, false), Some(backup.clone())),
        ]
    };
    let mut app = selected(&sb, &id);
    app.job = Some(Job {
        label: "Copying".into(),
        done: 0,
        total: 0,
        percent: false,
    });
    for message in messages(&id) {
        let why = app.blocked().expect("busy");
        let _ = app.update(message);
        assert!(app.dialog.is_none());
        assert_eq!(app.notice, Some(Notice::Info(why)));
        app.notice = None;
    }
    app.job = None;
    app.game_running = true;
    for message in messages(&id) {
        let _ = app.update(message);
        assert!(app.dialog.is_none() && app.job.is_none());
        assert_eq!(app.notice, Some(Notice::Info(text::GAME_RUNNING.into())));
        app.notice = None;
    }
    assert!(!backup.exists());
}
