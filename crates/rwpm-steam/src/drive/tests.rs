use std::collections::{HashMap, HashSet};

use super::*;

fn item(subscribed: bool, installed: bool, busy: bool, progress: Option<(u64, u64)>) -> Item {
    Item {
        subscribed,
        installed,
        busy,
        progress,
    }
}

const ABSENT: Item = Item {
    subscribed: false,
    installed: false,
    busy: false,
    progress: None,
};
const INSTALLED: Item = Item {
    subscribed: false,
    installed: true,
    busy: false,
    progress: None,
};
const SUBSCRIBED: Item = Item {
    subscribed: true,
    installed: true,
    busy: false,
    progress: None,
};

/// A Steam that follows a script: what `item` says after each `poll`.
#[derive(Default)]
struct Fake {
    script: HashMap<u64, Vec<Item>>,
    at: usize,
    started: Vec<(Op, u64)>,
    refuse: HashSet<u64>,
    answers: HashMap<u64, Result<(), String>>,
}

impl Backend for Fake {
    fn item(&self, id: u64) -> Item {
        let steps = &self.script[&id];
        steps[self.at.min(steps.len() - 1)]
    }

    fn start(&mut self, op: Op, id: u64) -> Result<(), String> {
        self.started.push((op, id));
        if self.refuse.contains(&id) {
            Err("Steam said no".into())
        } else {
            Ok(())
        }
    }

    fn poll(&mut self) -> Vec<(u64, Result<(), String>)> {
        self.at += 1;
        self.answers.drain().collect()
    }
}

/// One second per step.
struct Seconds(u64);

impl Clock for Seconds {
    fn step(&mut self) -> Duration {
        self.0 += 1;
        Duration::from_secs(self.0)
    }
}

fn go(op: Op, ids: &[u64], fake: &mut Fake) -> (usize, Vec<String>) {
    go_stopping(op, ids, fake, &AtomicBool::new(false))
}

fn go_stopping(op: Op, ids: &[u64], fake: &mut Fake, stop: &AtomicBool) -> (usize, Vec<String>) {
    let mut lines = Vec::new();
    let failed = run(op, ids, fake, &mut Seconds(0), stop, &mut |e| {
        lines.push(e.to_string());
    });
    (failed, lines)
}

fn fake(script: &[(u64, Vec<Item>)]) -> Fake {
    Fake {
        script: script.iter().cloned().collect(),
        ..Fake::default()
    }
}

#[test]
fn a_download_reports_progress_then_that_it_is_new() {
    let mut f = fake(&[(
        5,
        vec![
            ABSENT,
            item(false, false, true, Some((0, 100))),
            item(false, false, true, Some((60, 100))),
            item(false, false, true, Some((100, 100))),
            INSTALLED,
        ],
    )]);
    let (failed, lines) = go(Op::Download, &[5], &mut f);
    assert_eq!(failed, 0);
    assert_eq!(
        lines,
        [
            "progress 5 0 100",
            "progress 5 60 100",
            "progress 5 100 100",
            "ok 5 new"
        ]
    );
    assert_eq!(f.started, [(Op::Download, 5)]);
}

#[test]
fn a_mod_that_is_already_there_is_not_downloaded_again() {
    let mut f = fake(&[(5, vec![INSTALLED])]);
    let (failed, lines) = go(Op::Download, &[5], &mut f);
    assert_eq!((failed, lines), (0, vec!["ok 5 already".to_owned()]));
    assert!(f.started.is_empty());
}

#[test]
fn a_new_subscription_waits_for_the_answer_and_the_download() {
    let mut f = fake(&[(
        7,
        vec![
            ABSENT,
            ABSENT,
            item(true, false, true, Some((1, 2))),
            SUBSCRIBED,
        ],
    )]);
    f.answers.insert(7, Ok(()));
    let (failed, lines) = go(Op::Subscribe, &[7], &mut f);
    assert_eq!(failed, 0);
    assert_eq!(lines.last().map(String::as_str), Some("ok 7 new"));
    assert_eq!(f.started, [(Op::Subscribe, 7)]);
}

/// The bug the first real run found: Steam acknowledged the call but its own state did not
/// show the change yet; the helper said `ok` and exited, and the change was lost.
#[test]
fn a_change_is_not_reported_until_steam_shows_it() {
    let mut f = fake(&[(
        7,
        vec![INSTALLED, INSTALLED, INSTALLED, INSTALLED, SUBSCRIBED],
    )]);
    f.answers.insert(7, Ok(()));
    let (failed, lines) = go(Op::Subscribe, &[7], &mut f);
    assert_eq!((failed, lines), (0, vec!["ok 7 new".to_owned()]));
    assert!(f.at >= 4, "stopped after only {} polls", f.at);

    let mut f = fake(&[(7, vec![SUBSCRIBED, SUBSCRIBED, SUBSCRIBED, INSTALLED])]);
    f.answers.insert(7, Ok(()));
    let (failed, lines) = go(Op::Unsubscribe, &[7], &mut f);
    assert_eq!((failed, lines), (0, vec!["ok 7 new".to_owned()]));
    assert!(f.at >= 3, "stopped after only {} polls", f.at);
}

/// An acknowledgement that Steam's state never confirms is a failure, never an `ok`.
#[test]
fn an_acknowledgement_that_steam_never_confirms_fails() {
    let mut f = fake(&[(7, vec![INSTALLED])]);
    f.answers.insert(7, Ok(()));
    let (failed, lines) = go(Op::Subscribe, &[7], &mut f);
    assert_eq!(failed, 1);
    assert!(lines.iter().all(|l| !l.starts_with("ok ")), "{lines:?}");
}

#[test]
fn a_subscription_the_player_already_had_is_reported_as_already() {
    let mut f = fake(&[(7, vec![SUBSCRIBED])]);
    let (failed, lines) = go(Op::Subscribe, &[7], &mut f);
    assert_eq!((failed, lines), (0, vec!["ok 7 already".to_owned()]));
    assert!(
        f.started.is_empty(),
        "no call is made for what is already so"
    );
}

#[test]
fn a_subscribed_item_still_downloading_is_waited_for_not_resubscribed() {
    let mut f = fake(&[(7, vec![item(true, false, true, None), SUBSCRIBED])]);
    let (failed, lines) = go(Op::Subscribe, &[7], &mut f);
    assert_eq!((failed, lines), (0, vec!["ok 7 already".to_owned()]));
    assert!(f.started.is_empty());
}

#[test]
fn a_refused_start_and_a_refused_subscription_each_fail_that_id_only() {
    let mut f = fake(&[(1, vec![ABSENT]), (2, vec![ABSENT]), (3, vec![INSTALLED])]);
    f.refuse.insert(1);
    let (failed, lines) = go(Op::Download, &[1, 3], &mut f);
    assert_eq!(failed, 1);
    assert_eq!(lines, ["fail 1 Steam said no", "ok 3 already"]);

    let mut f = fake(&[(2, vec![ABSENT, ABSENT])]);
    f.answers.insert(2, Err("not allowed".into()));
    let (failed, lines) = go(Op::Subscribe, &[2], &mut f);
    assert_eq!((failed, lines), (1, vec!["fail 2 not allowed".to_owned()]));
}

#[test]
fn unsubscribing_reports_new_or_already() {
    let mut f = fake(&[(4, vec![SUBSCRIBED, INSTALLED]), (9, vec![INSTALLED])]);
    f.answers.insert(4, Ok(()));
    let (failed, lines) = go(Op::Unsubscribe, &[4, 9], &mut f);
    assert_eq!(failed, 0);
    assert_eq!(lines, ["ok 9 already", "ok 4 new"]);
    assert_eq!(f.started, [(Op::Unsubscribe, 4)]);
}

#[test]
fn an_item_that_makes_no_progress_fails_after_the_stall_time() {
    let mut f = fake(&[(5, vec![item(false, false, true, Some((10, 100)))])]);
    let (failed, lines) = go(Op::Download, &[5], &mut f);
    assert_eq!(failed, 1);
    assert_eq!(lines.first().map(String::as_str), Some("progress 5 10 100"));
    assert_eq!(
        lines.last().map(String::as_str),
        Some("fail 5 Steam made no progress for 60 seconds")
    );
}

#[test]
fn one_stuck_item_does_not_hold_back_the_others() {
    let mut f = fake(&[
        (1, vec![item(false, false, true, None)]),
        (2, vec![ABSENT, INSTALLED]),
    ]);
    let (failed, lines) = go(Op::Download, &[1, 2], &mut f);
    assert_eq!(failed, 1);
    assert!(lines.contains(&"ok 2 new".to_owned()), "{lines:?}");
    assert!(lines.iter().any(|l| l.starts_with("fail 1 ")), "{lines:?}");
}

#[test]
fn the_stop_flag_ends_the_run_and_fails_what_is_left() {
    let mut f = fake(&[
        (1, vec![item(false, false, true, None)]),
        (2, vec![INSTALLED]),
    ]);
    let stop = AtomicBool::new(true);
    let (failed, lines) = go_stopping(Op::Download, &[1, 2], &mut f, &stop);
    assert_eq!(failed, 1);
    assert_eq!(lines, ["ok 2 already", "fail 1 cancelled"]);
}
