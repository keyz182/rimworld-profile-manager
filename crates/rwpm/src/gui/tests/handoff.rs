//! A later launch of rwpm handing this window a file (see `instance`).

use super::backups::{inc, make_backup};
use super::*;
use crate::gui::setup::Setup;
use crate::instance::Request;

fn backup_request(sb: &Sandbox) -> (Request, PathBuf) {
    let id = sb.offline_profile("Source", &[]);
    let backup = make_backup(sb, &id, inc(true, false, false));
    (Request::Open(OpenFile::Backup(backup.clone())), backup)
}

#[test]
fn an_idle_window_opens_the_file_it_is_handed() {
    let sb = Sandbox::new();
    let (request, _) = backup_request(&sb);
    let mut app = sb.app();
    let _ = app.update(Message::Instance(request));
    assert!(matches!(app.dialog, Some(Dialog::Restore(_))));
    assert!(app.notice.is_none());
}

#[test]
fn a_window_with_a_dialog_open_keeps_it_and_says_why() {
    let sb = Sandbox::new();
    let (request, backup) = backup_request(&sb);
    let mut app = sb.app();
    let _ = app.update(Message::RestorePicked(Some(backup.clone())));
    let shown = format!("{:?}", app.dialog);
    let _ = app.update(Message::Instance(request));
    assert_eq!(format!("{:?}", app.dialog), shown, "the open dialog stays");
    let name = backup.file_name().expect("name").to_string_lossy();
    assert_eq!(app.notice, Some(Notice::Info(text::open_when_busy(&name))));
}

#[test]
fn a_window_with_a_job_running_keeps_it_and_says_why() {
    let sb = Sandbox::new();
    let (request, _) = backup_request(&sb);
    let mut app = sb.app();
    app.job = Some(Job {
        label: "Copying".into(),
        done: 0,
        total: 0,
        percent: false,
    });
    let _ = app.update(Message::Instance(request));
    assert!(app.dialog.is_none());
    assert!(app.job.is_some());
    assert!(matches!(app.notice, Some(Notice::Info(_))));
}

#[test]
fn a_window_in_first_run_setup_says_so_and_opens_nothing() {
    let sb = Sandbox::new();
    let (request, _) = backup_request(&sb);
    let mut app = sb.app();
    app.setup = Some(Setup::new(&app.game));
    let _ = app.update(Message::Instance(request));
    assert!(app.dialog.is_none());
    assert!(matches!(app.notice, Some(Notice::Info(_))));
}

#[test]
fn coming_to_the_front_changes_nothing_else() {
    let sb = Sandbox::new();
    let mut app = sb.app();
    let _ = app.update(Message::Instance(Request::Focus));
    assert!(app.dialog.is_none() && app.job.is_none() && app.notice.is_none());
}
