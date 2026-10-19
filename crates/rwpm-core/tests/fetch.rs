//! The fetch runner against fake helpers: small shell scripts that print what the real one
//! would. Unix only, since the fakes are shell scripts.
//!
//! These tests start many processes, so they live in their own test program. A process that
//! was just forked can hold a copy of any lock file another test has open until it runs its
//! program; in the unit tests that made unrelated lock tests fail at random (about half the
//! runs). Cargo runs test programs one after the other, so here it cannot happen.

#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use rwpm_core::fetch::{Fresh, HELPER, Op, Outcome, Request, Status, check, fetch, helper_beside};

/// A helper that says it is ready to `check` (which `fetch` runs first) and runs `body` (shell)
/// for anything else.
fn helper(dir: &Path, body: &str) -> PathBuf {
    helper_raw(
        dir,
        &format!("[ \"$1\" = check ] && {{ echo ok; exit 0; }}\n{body}"),
    )
}

/// A helper that runs `body` (shell) whatever it is asked, `check` included.
fn helper_raw(dir: &Path, body: &str) -> PathBuf {
    let path = dir.join(HELPER);
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("script");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    path
}

/// `<workshop>/<id>/About/About.xml` naming `package_id`.
fn delivered(workshop: &Path, id: u64, package_id: &str) {
    let about = workshop.join(id.to_string()).join("About");
    std::fs::create_dir_all(&about).expect("about dir");
    std::fs::write(
        about.join("About.xml"),
        format!("<ModMetaData><name>x</name><packageId>{package_id}</packageId></ModMetaData>"),
    )
    .expect("about");
}

fn wanted(pairs: &[(u64, &str)]) -> Vec<(u64, String)> {
    pairs.iter().map(|(id, p)| (*id, (*p).to_owned())).collect()
}

fn request<'a>(op: Op, wanted: &'a [(u64, String)], dir: Option<&'a Path>) -> Request<'a> {
    Request {
        op,
        wanted,
        workshop_dir: dir,
        quiet: Duration::from_secs(5),
    }
}

fn go(helper: &Path, req: &Request<'_>) -> (Vec<Outcome>, Vec<(u64, u64)>) {
    let mut seen = Vec::new();
    let out = fetch(helper, req, &AtomicBool::new(false), &mut |d, t| {
        seen.push((d, t));
    });
    (out, seen)
}

fn statuses(outcomes: &[Outcome]) -> Vec<Status> {
    outcomes.iter().map(|o| o.status.clone()).collect()
}

#[test]
fn the_helper_is_found_beside_the_program_only_when_it_is_there() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let exe = tmp.path().join("rwpm");
    assert_eq!(helper_beside(&exe), None);
    let path = helper(tmp.path(), "exit 0");
    assert_eq!(helper_beside(&exe), Some(path));
}

#[test]
fn a_normal_run_reports_each_mod_and_the_progress_of_all_together() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let workshop = tmp.path().join("workshop");
    delivered(&workshop, 5, "good.one");
    delivered(&workshop, 6, "good.two");
    let h = helper(
        tmp.path(),
        "printf 'progress 5 1 10\\nprogress 6 5 10\\nprogress 5 10 10\\nok 5 new\\nok 6 already\\n'",
    );
    let pairs = wanted(&[(5, "good.one"), (6, "good.two")]);
    let (out, progress) = go(&h, &request(Op::Download, &pairs, Some(&workshop)));
    assert_eq!(
        statuses(&out),
        [Status::Done(Fresh::New), Status::Done(Fresh::Already)]
    );
    assert_eq!(progress, [(1, 10), (6, 20), (15, 20)]);
}

#[test]
fn a_mod_that_is_not_the_one_promised_is_not_kept_and_is_unsubscribed() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let workshop = tmp.path().join("workshop");
    let log = tmp.path().join("calls.txt");
    delivered(&workshop, 5, "evil.mod");
    delivered(&workshop, 6, "good.two");
    let h = helper(
        tmp.path(),
        &format!(
            "echo \"$@\" >> '{}'\ncase \"$1\" in subscribe) printf 'ok 5 new\\nok 6 new\\n';; esac",
            log.display()
        ),
    );
    let pairs = wanted(&[(5, "good.one"), (6, "good.two")]);
    let (out, _) = go(&h, &request(Op::Subscribe, &pairs, Some(&workshop)));
    assert_eq!(
        statuses(&out),
        [
            Status::Mismatch {
                expected: "good.one".into(),
                found: "evil.mod".into()
            },
            Status::Done(Fresh::New)
        ]
    );
    let calls = std::fs::read_to_string(&log).expect("log");
    assert_eq!(
        calls.lines().collect::<Vec<_>>(),
        ["subscribe 5 6", "unsubscribe 5"],
        "only the mod that did not match is undone"
    );
}

#[test]
fn a_subscription_the_player_already_had_is_never_undone() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let workshop = tmp.path().join("workshop");
    let log = tmp.path().join("calls.txt");
    delivered(&workshop, 5, "evil.mod");
    let h = helper(
        tmp.path(),
        &format!(
            "echo \"$@\" >> '{}'\ncase \"$1\" in subscribe) printf 'ok 5 already\\n';; esac",
            log.display()
        ),
    );
    let pairs = wanted(&[(5, "good.one")]);
    let (out, _) = go(&h, &request(Op::Subscribe, &pairs, Some(&workshop)));
    assert!(matches!(out[0].status, Status::Mismatch { .. }));
    let calls = std::fs::read_to_string(&log).expect("log");
    assert_eq!(calls.lines().collect::<Vec<_>>(), ["subscribe 5"]);
}

#[test]
fn the_identity_check_ignores_case_and_the_steam_suffix_and_notices_a_missing_file() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let workshop = tmp.path().join("workshop");
    delivered(&workshop, 5, "Good.Mod");
    std::fs::create_dir_all(workshop.join("6")).expect("folder without about");
    let h = helper(tmp.path(), "printf 'ok 5 new\\nok 6 new\\n'");
    let pairs = wanted(&[(5, "good.mod_steam"), (6, "any.mod")]);
    let (out, _) = go(&h, &request(Op::Download, &pairs, Some(&workshop)));
    assert_eq!(out[0].status, Status::Done(Fresh::New));
    assert_eq!(
        out[1].status,
        Status::Mismatch {
            expected: "any.mod".into(),
            found: "no About.xml".into()
        }
    );
    let (out, _) = go(&h, &request(Op::Download, &pairs, None));
    assert!(
        out.iter()
            .all(|o| matches!(o.status, Status::Mismatch { .. }))
    );
}

#[test]
fn unsubscribing_needs_no_identity_check() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let h = helper(tmp.path(), "printf 'ok 5 new\\n'");
    let pairs = wanted(&[(5, "any.mod")]);
    let (out, _) = go(&h, &request(Op::Unsubscribe, &pairs, None));
    assert_eq!(statuses(&out), [Status::Done(Fresh::New)]);
}

#[test]
fn junk_lines_and_ids_nobody_asked_for_are_ignored_and_a_missing_answer_fails_that_mod() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let workshop = tmp.path().join("workshop");
    delivered(&workshop, 6, "good.two");
    let h = helper(
        tmp.path(),
        "printf 'hello\\nweird 9\\nok 99 new\\nfail 98 x\\nok 6 already\\n'",
    );
    let pairs = wanted(&[(5, "good.one"), (6, "good.two")]);
    let (out, _) = go(&h, &request(Op::Download, &pairs, Some(&workshop)));
    assert_eq!(
        statuses(&out),
        [
            Status::Failed("Steam gave no answer for this mod.".into()),
            Status::Done(Fresh::Already)
        ]
    );
}

#[test]
fn an_error_line_fails_every_mod_with_its_reason() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let h = helper(tmp.path(), "printf 'error Steam is not running\\n'");
    let pairs = wanted(&[(5, "a.b"), (6, "c.d")]);
    let (out, _) = go(&h, &request(Op::Subscribe, &pairs, None));
    let said = Status::Failed("Steam is not running".into());
    assert_eq!(statuses(&out), [said.clone(), said]);
}

#[test]
fn a_helper_that_cannot_start_fails_every_mod_in_plain_words() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let pairs = wanted(&[(5, "a.b")]);
    let missing = tmp.path().join("nowhere");
    let (out, _) = go(&missing, &request(Op::Download, &pairs, None));
    let Status::Failed(why) = &out[0].status else {
        panic!("{out:?}");
    };
    assert!(
        why.starts_with("The Steam helper could not be started"),
        "{why}"
    );
}

#[test]
fn cancelling_ends_the_helper_at_once() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let h = helper(tmp.path(), "exec sleep 30");
    let pairs = wanted(&[(5, "a.b"), (6, "c.d")]);
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&cancel);
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        flag.store(true, Ordering::Relaxed);
    });
    let started = Instant::now();
    let out = fetch(
        &h,
        &request(Op::Download, &pairs, None),
        &cancel,
        &mut |_, _| {},
    );
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "{:?}",
        started.elapsed()
    );
    let cancelled = Status::Failed("cancelled".into());
    assert_eq!(statuses(&out), [cancelled.clone(), cancelled]);
}

#[test]
fn a_helper_that_goes_quiet_is_ended_and_its_mods_fail() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let h = helper(tmp.path(), "exec sleep 30");
    let pairs = wanted(&[(5, "a.b")]);
    let mut req = request(Op::Download, &pairs, None);
    req.quiet = Duration::from_millis(400);
    let started = Instant::now();
    let (out, _) = go(&h, &req);
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(
        statuses(&out),
        [Status::Failed("The Steam helper stopped answering.".into())]
    );
}

#[test]
fn check_says_ok_or_why_not() {
    let tmp = tempfile::tempdir().expect("tempdir");
    assert_eq!(check(&helper_raw(tmp.path(), "printf 'ok\\n'")), Ok(()));
    assert_eq!(
        check(&helper_raw(
            tmp.path(),
            "printf 'error Steam is not running\\n'"
        )),
        Err("Steam is not running".to_owned())
    );
    assert_eq!(
        check(&helper_raw(tmp.path(), "exit 0")),
        Err("The Steam helper gave no answer.".to_owned())
    );
    let nowhere = tmp.path().join("nowhere");
    assert!(check(&nowhere).is_err());
}

/// Logs the word it was started with, answers `check` as `check_says`, and `ok <id> new` for
/// every id of anything else.
fn logging(dir: &Path, check_says: &str) -> PathBuf {
    helper_raw(
        dir,
        &format!(
            "echo \"$1\" >> \"$(dirname \"$0\")/words.txt\"\n\
             if [ \"$1\" = check ]; then echo '{check_says}'; exit 0; fi\n\
             shift\nfor id in \"$@\"; do echo \"ok $id new\"; done"
        ),
    )
}

fn words(dir: &Path) -> Vec<String> {
    std::fs::read_to_string(dir.join("words.txt"))
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

#[test]
fn a_check_runs_just_before_the_real_run() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let h = logging(tmp.path(), "ok");
    let pairs = wanted(&[(5, "a.b")]);
    let (out, _) = go(&h, &request(Op::Unsubscribe, &pairs, None));
    assert_eq!(statuses(&out), [Status::Done(Fresh::New)]);
    assert_eq!(words(tmp.path()), ["check", "unsubscribe"]);
}

#[test]
fn a_failed_check_fails_every_mod_with_its_reason_and_asks_steam_nothing() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let h = logging(tmp.path(), "error Steam is not running");
    let pairs = wanted(&[(5, "a.b"), (6, "c.d")]);
    let (out, _) = go(&h, &request(Op::Subscribe, &pairs, None));
    let said = Status::Failed("Steam is not running".into());
    assert_eq!(statuses(&out), [said.clone(), said]);
    assert_eq!(words(tmp.path()), ["check"], "no subscribe was started");
}

#[test]
fn nothing_wanted_starts_no_helper() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let h = logging(tmp.path(), "ok");
    let (out, _) = go(&h, &request(Op::Download, &[], None));
    assert!(out.is_empty());
    assert!(words(tmp.path()).is_empty());
}

/// A helper that answers `ok <id> new` for every id it is given and logs how many it got.
const COUNTING: &str = r#"echo $(( $# - 1 )) >> "$(dirname "$0")/calls.txt"
shift
for id in "$@"; do echo "ok $id new"; done"#;

#[test]
fn a_pack_with_more_than_five_hundred_mods_goes_in_batches() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let h = helper(tmp.path(), COUNTING);
    let pairs: Vec<(u64, String)> = (1..=501).map(|id| (id, "x.y".to_owned())).collect();
    let req = request(Op::Unsubscribe, &pairs, None);
    let (out, _) = go(&h, &req);
    assert_eq!(out.len(), 501);
    assert!(
        out.iter().all(|o| o.status == Status::Done(Fresh::New)),
        "every mod has its own outcome"
    );
    assert_eq!(
        out.iter().map(|o| o.id).collect::<Vec<_>>(),
        (1..=501).collect::<Vec<u64>>(),
        "in the order asked"
    );
    let calls = std::fs::read_to_string(tmp.path().join("calls.txt")).expect("calls");
    let sizes: Vec<usize> = calls.lines().map(|l| l.parse().expect("number")).collect();
    assert_eq!(sizes, [200, 200, 101]);
    assert!(
        sizes.iter().all(|n| *n <= rwpm_core::fetch::BATCH),
        "the helper never sees more than a batch"
    );
}

#[test]
fn cancelling_a_big_pack_cancels_every_mod_and_starts_no_later_batch() {
    let tmp = tempfile::tempdir().expect("tempdir");
    // The first run sets the flag's file; the test thread watches it and cancels.
    let h = helper(
        tmp.path(),
        &format!("touch \"$(dirname \"$0\")/started\"\nsleep 1\n{COUNTING}"),
    );
    let pairs: Vec<(u64, String)> = (1..=401).map(|id| (id, "x.y".to_owned())).collect();
    let req = request(Op::Unsubscribe, &pairs, None);
    let cancel = AtomicBool::new(false);
    let started = tmp.path().join("started");
    let out = std::thread::scope(|scope| {
        scope.spawn(|| {
            while !started.exists() {
                std::thread::sleep(Duration::from_millis(20));
            }
            cancel.store(true, Ordering::Relaxed);
        });
        fetch(&h, &req, &cancel, &mut |_, _| {})
    });
    assert_eq!(out.len(), 401);
    assert!(
        out.iter()
            .all(|o| matches!(&o.status, Status::Failed(why) if why == "cancelled")),
        "{:?}",
        out.first()
    );
    let calls = std::fs::read_to_string(tmp.path().join("calls.txt")).unwrap_or_default();
    assert!(
        calls.lines().count() <= 1,
        "no batch ran after the cancel: {calls}"
    );
}
