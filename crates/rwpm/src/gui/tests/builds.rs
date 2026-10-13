//! T56 PR B: the Game versions screen. Jobs are run by feeding their messages by hand,
//! except where a test needs the real copy (the job thread starts when the task is made).

use rwpm_core::builds::capture::{CaptureKind, plan_capture};
use rwpm_core::builds::{BuildKey, BuildStore};

use super::super::message::Drill;
use super::*;
use crate::gui::Job;

pub(super) const VERSION: &str = "1.6.4871 rev598";
pub(super) const KEY: &str = "1.6.4871-rev598";

impl Sandbox {
    /// Makes the sandbox game a Steam install Capture accepts (V58: `StateFlags` 4).
    pub(super) fn steam_install(&self, buildid: &str) {
        let acf = self.path("lib/steamapps/appmanifest_294100.acf");
        std::fs::write(
            acf,
            format!(
                "\"AppState\"\n{{\n\t\"appid\"\t\t\"294100\"\n\t\"StateFlags\"\t\t\"4\"\n\
                 \t\"installdir\"\t\t\"RimWorld\"\n\t\"buildid\"\t\t\"{buildid}\"\n\
                 \t\"MountedConfig\"\n\t{{\n\t\t\"BetaKey\"\t\t\"\"\n\t}}\n}}\n"
            ),
        )
        .expect("manifest");
        std::fs::write(
            self.game_dir().join("Version.txt"),
            format!("{VERSION}\r\n"),
        )
        .expect("version");
    }

    pub(super) fn builds(&self) -> BuildStore {
        self.ctx().build_store().expect("builds")
    }

    /// Stores the install as a build, through core (what the job thread does).
    pub(super) fn stored_build(&self) {
        let app = self.app();
        let plan = plan_capture(&self.builds(), app.game.as_ref().expect("game")).expect("plan");
        rwpm_core::builds::capture::apply_capture(
            &self.builds(),
            &plan,
            &rwpm_core::profile::ProcessProbe,
            &mut |_| {},
        )
        .expect("capture");
    }

    pub(super) fn pin(&self, name: &str) -> ProfileId {
        let store = self.ctx().store().expect("store");
        let p = store.create(name).expect("create");
        store.set_pin(&p.id, Some(KEY)).expect("pin");
        p.id
    }
}

pub(super) fn open_versions(sb: &Sandbox) -> App {
    let mut app = sb.app();
    let _ = app.update(Message::Drill(Some(Drill::GameVersions)));
    let _ = app.update(Message::BuildsRead(sb.ctx().read_builds()));
    app
}

pub(super) fn found(app: &App, label: &str) -> bool {
    iced_test::simulator(app.view()).find(label).is_ok()
}

pub(super) fn clicked(app: &App, label: &str) -> Vec<Message> {
    let mut ui = iced_test::simulator(app.view());
    ui.click(label).unwrap_or_else(|e| panic!("{label}: {e:?}"));
    ui.into_messages().collect()
}

#[test]
fn header_button_opens_the_list_and_back_closes_it() {
    let sb = Sandbox::new();
    let mut app = sb.app();
    for m in clicked(&app, text::GAME_VERSIONS) {
        let _ = app.update(m);
    }
    assert_eq!(app.drill, Some(Drill::GameVersions));
    assert!(found(&app, text::GV_READING), "reads before it shows");
    let _ = app.update(Message::BuildsRead(sb.ctx().read_builds()));
    assert!(found(&app, text::GV_EMPTY));
    for m in clicked(&app, text::BACK) {
        let _ = app.update(m);
    }
    assert_eq!(app.drill, None);
}

#[test]
fn list_shows_each_build_with_engine_size_date_and_who_uses_it() {
    let sb = Sandbox::new();
    sb.steam_install("1111");
    sb.stored_build();
    sb.pin("Pinned Run");
    let app = open_versions(&sb);
    let row = &app.builds.as_ref().expect("read").rows[0];
    assert_eq!(row.key, KEY);
    assert!(!row.waylith);
    assert_eq!(row.used_by, ["Pinned Run"]);
    assert!(
        row.stored.as_deref().is_some_and(|d| d.len() == 10),
        "{:?}",
        row.stored
    );
    assert!(found(&app, KEY));
    assert!(found(&app, &text::used_by(&["Pinned Run".to_owned()])));
    assert!(found(&app, &text::gv_title(1)));
    assert!(found(&app, &text::steam_now(VERSION)));
}

#[test]
fn delete_is_off_while_a_profile_uses_the_build() {
    let sb = Sandbox::new();
    sb.steam_install("1111");
    sb.stored_build();
    sb.pin("Pinned Run");
    let mut app = open_versions(&sb);
    assert!(
        clicked(&app, text::DELETE_VERSION).is_empty(),
        "button is disabled"
    );
    let _ = app.update(Message::DeleteBuild(KEY.into()));
    assert!(
        app.dialog.is_none(),
        "a pinned build cannot be queued for delete"
    );
}

#[test]
fn delete_asks_first_when_nobody_uses_it() {
    let sb = Sandbox::new();
    sb.steam_install("1111");
    sb.stored_build();
    let mut app = open_versions(&sb);
    for m in clicked(&app, text::DELETE_VERSION) {
        let _ = app.update(m);
    }
    let Some(Dialog::ConfirmDeleteBuild { key, .. }) = &app.dialog else {
        panic!("no question: {:?}", app.dialog);
    };
    assert_eq!(key, KEY);
    let _ = app.update(Message::Cancel);
    assert!(
        sb.builds()
            .dir_of(&BuildKey::parse(KEY).expect("key"))
            .exists()
    );
}

#[cfg(unix)]
#[test]
fn a_profile_that_cannot_be_read_blocks_delete_and_is_named() {
    use std::os::unix::fs::PermissionsExt;
    let sb = Sandbox::new();
    sb.steam_install("1111");
    sb.stored_build();
    let id = sb.pin("Locked");
    let file = sb.path(&format!("app/profiles/{}/profile.toml", id.as_str()));
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o000)).expect("chmod");
    let readable = std::fs::read(&file).is_ok();
    let mut app = open_versions(&sb);
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).expect("chmod");
    if readable {
        return; // running as root: permissions do not stop reads
    }
    assert_eq!(
        app.builds.as_ref().expect("read").rows[0].unchecked,
        [id.to_string()]
    );
    let _ = app.update(Message::DeleteBuild(KEY.into()));
    assert!(app.dialog.is_none());
}

#[test]
fn capture_asks_with_size_and_free_space_then_stores_the_build() {
    let sb = Sandbox::new();
    sb.steam_install("1111");
    let mut app = sb.app();
    let _ = app.update(Message::CaptureBuild);
    assert_eq!(
        app.job.as_ref().map(|j| j.label.as_str()),
        Some(text::JOB_MEASURING_GAME)
    );
    app.job = None;

    let plan = plan_capture(&sb.builds(), app.game.as_ref().expect("game")).expect("plan");
    assert_eq!(plan.kind, CaptureKind::New);
    let _ = app.update(Message::Done(Ok(Outcome::CapturePlanned(Box::new(
        plan.clone(),
    )))));
    assert!(matches!(app.dialog, Some(Dialog::ConfirmCapture(_))));
    assert!(found(
        &app,
        &text::confirm_capture(VERSION, plan.bytes, plan.free_bytes)
    ));

    let _ = app.update(Message::Confirm);
    assert_eq!(
        app.job.as_ref().map(|j| j.label.as_str()),
        Some(text::JOB_STORING)
    );
    assert_eq!(app.job.as_ref().map(|j| j.total), Some(plan.bytes));
    let stored = sb
        .builds()
        .dir_of(&BuildKey::parse(KEY).expect("key"))
        .join("build.toml");
    for _ in 0..100 {
        if stored.is_file() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(stored.is_file(), "the job thread stored the build");
    let _ = app.update(Message::Done(Ok(Outcome::BuildsChanged(
        text::version_stored(KEY),
    ))));
    assert!(app.job.is_none());
    assert_eq!(app.notice, Some(Notice::Info(text::version_stored(KEY))));
}

#[test]
fn capturing_a_stored_version_again_is_a_notice_not_a_question() {
    let sb = Sandbox::new();
    sb.steam_install("1111");
    sb.stored_build();
    let mut app = sb.app();
    let plan = plan_capture(&sb.builds(), app.game.as_ref().expect("game")).expect("plan");
    assert!(plan.is_no_op());
    let _ = app.update(Message::Done(Ok(Outcome::CapturePlanned(Box::new(plan)))));
    assert!(app.dialog.is_none());
    assert_eq!(app.notice, Some(Notice::Info(text::already_stored(KEY))));
}

#[test]
fn a_changed_build_is_offered_as_an_update() {
    let sb = Sandbox::new();
    sb.steam_install("1111");
    sb.stored_build();
    sb.steam_install("2222");
    let mut app = sb.app();
    let plan = plan_capture(&sb.builds(), app.game.as_ref().expect("game")).expect("plan");
    let CaptureKind::Update { reason, .. } = &plan.kind else {
        panic!("not an update: {:?}", plan.kind);
    };
    let words = text::confirm_update(
        KEY,
        text::update_reason(*reason),
        plan.bytes,
        plan.free_bytes,
    );
    let _ = app.update(Message::Done(Ok(Outcome::CapturePlanned(Box::new(plan)))));
    assert!(found(&app, &words));
    assert!(found(&app, text::UPDATE_IT));
}

#[test]
fn store_is_off_when_the_copy_will_not_fit() {
    let sb = Sandbox::new();
    sb.steam_install("1111");
    let mut app = sb.app();
    let mut plan = plan_capture(&sb.builds(), app.game.as_ref().expect("game")).expect("plan");
    plan.bytes = 1000;
    plan.free_bytes = Some(10);
    let _ = app.update(Message::Done(Ok(Outcome::CapturePlanned(Box::new(plan)))));
    assert!(found(&app, text::NOT_ENOUGH_ROOM));
    assert!(
        clicked(&app, text::STORE_IT).is_empty(),
        "Store it is disabled"
    );
}

#[test]
fn capture_and_delete_wait_for_a_job_or_a_running_game() {
    let sb = Sandbox::new();
    sb.steam_install("1111");
    sb.stored_build();
    let mut app = open_versions(&sb);
    app.game_running = true;
    let _ = app.update(Message::CaptureBuild);
    assert!(app.job.is_none(), "a running game blocks Capture");
    assert_eq!(
        app.notice,
        Some(Notice::Info(text::GAME_RUNNING.to_owned()))
    );
    assert!(clicked_disabled(&app, text::CAPTURE));
    let _ = app.update(Message::DeleteBuild(KEY.into()));
    assert!(app.dialog.is_none(), "a running game blocks Delete");

    app.game_running = false;
    app.job = Some(Job {
        label: "Storing".into(),
        done: 0,
        total: 1,
    });
    let _ = app.update(Message::CaptureBuild);
    let _ = app.update(Message::DeleteBuild(KEY.into()));
    assert!(app.dialog.is_none());
    assert_eq!(app.job.as_ref().map(|j| j.label.as_str()), Some("Storing"));
}

fn clicked_disabled(app: &App, label: &str) -> bool {
    clicked(app, label).is_empty()
}

#[test]
fn capture_without_a_game_says_why() {
    let sb = Sandbox::new();
    let mut app = sb.app();
    app.game = Err("gone".to_owned());
    let _ = app.update(Message::CaptureBuild);
    assert_eq!(app.notice, Some(Notice::Error(text::GV_NO_GAME.to_owned())));
    assert!(app.job.is_none());
}
