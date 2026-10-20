use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

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
    /// The clock's seconds at each start.
    started_at: Vec<u64>,
    clock: Rc<Cell<u64>>,
    refuse: HashSet<u64>,
    /// Refused only when asked a second time.
    refuse_again: HashSet<u64>,
    /// Given at the first poll after the id was started (`starts_needed` times, default once).
    answers: HashMap<u64, Result<(), String>>,
    starts_needed: HashMap<u64, usize>,
}

impl Fake {
    fn starts(&self, id: u64) -> usize {
        self.started.iter().filter(|(_, i)| *i == id).count()
    }
}

impl Backend for Fake {
    fn item(&self, id: u64) -> Item {
        let steps = &self.script[&id];
        steps[self.at.min(steps.len() - 1)]
    }

    fn start(&mut self, op: Op, id: u64) -> Result<(), String> {
        self.started.push((op, id));
        self.started_at.push(self.clock.get());
        if self.refuse.contains(&id) || (self.refuse_again.contains(&id) && self.starts(id) > 1) {
            Err("Steam said no".into())
        } else {
            Ok(())
        }
    }

    fn poll(&mut self) -> Vec<(u64, Result<(), String>)> {
        self.at += 1;
        let due: Vec<u64> = self
            .answers
            .keys()
            .copied()
            .filter(|id| self.starts(*id) >= self.starts_needed.get(id).copied().unwrap_or(1))
            .collect();
        due.into_iter()
            .filter_map(|id| self.answers.remove(&id).map(|a| (id, a)))
            .collect()
    }
}

/// One second per step, shared with the fake so it can note when calls were made.
struct Seconds(Rc<Cell<u64>>);

impl Clock for Seconds {
    fn step(&mut self) -> Duration {
        self.0.set(self.0.get() + 1);
        Duration::from_secs(self.0.get())
    }
}

fn go(op: Op, ids: &[u64], fake: &mut Fake) -> (usize, Vec<String>) {
    go_stopping(op, ids, fake, &AtomicBool::new(false))
}

fn go_stopping(op: Op, ids: &[u64], fake: &mut Fake, stop: &AtomicBool) -> (usize, Vec<String>) {
    let mut lines = Vec::new();
    let mut clock = Seconds(Rc::clone(&fake.clock));
    let failed = run(op, ids, fake, &mut clock, stop, &mut |e| {
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

#[test]
fn no_subscribe_or_unsubscribe_call_is_made_before_the_settle_time() {
    let settle = SETTLE.as_secs();
    assert!(settle >= 1, "there is a settle time at all");
    for op in [Op::Subscribe, Op::Unsubscribe] {
        let before = if op == Op::Subscribe {
            ABSENT
        } else {
            SUBSCRIBED
        };
        let mut f = fake(&[(7, vec![before])]);
        let _ = go(op, &[7], &mut f);
        assert_eq!(f.started_at.first(), Some(&settle), "{op:?}");
    }
    let mut f = fake(&[(5, vec![ABSENT, INSTALLED])]);
    let _ = go(Op::Download, &[5], &mut f);
    assert_eq!(f.started_at, [0], "a download does not wait");
}

#[test]
fn the_stop_flag_during_the_settle_time_cancels_everything_without_a_call() {
    let mut f = fake(&[(1, vec![ABSENT]), (2, vec![ABSENT])]);
    let stop = AtomicBool::new(true);
    let (failed, lines) = go_stopping(Op::Subscribe, &[1, 2], &mut f, &stop);
    assert_eq!(failed, 2);
    assert_eq!(lines, ["fail 1 cancelled", "fail 2 cancelled"]);
    assert!(f.started.is_empty());
}

/// What Steam's log showed: the subscription arrives, then the list Steam fetched at start
/// removes it, and no answer ever comes. The call is made once more.
#[test]
fn a_subscription_that_steam_undoes_is_asked_for_once_more() {
    let arriving = item(true, false, true, None);
    let mut f = fake(&[(
        7,
        vec![
            ABSENT,
            arriving,
            ABSENT,
            ABSENT,
            item(true, false, true, Some((1, 2))),
            SUBSCRIBED,
        ],
    )]);
    f.answers.insert(7, Ok(()));
    f.starts_needed.insert(7, 2);
    let (failed, lines) = go(Op::Subscribe, &[7], &mut f);
    assert_eq!(
        (failed, lines),
        (0, vec!["progress 7 1 2".into(), "ok 7 new".into()])
    );
    assert_eq!(f.started, [(Op::Subscribe, 7), (Op::Subscribe, 7)]);
    assert!(
        f.started_at[1] < SETTLE.as_secs() + RESEND_AFTER.as_secs(),
        "asked again at once, not after the resend time"
    );
}

#[test]
fn an_unsubscribe_that_steam_undoes_is_asked_for_once_more() {
    let mut f = fake(&[(
        4,
        vec![SUBSCRIBED, INSTALLED, SUBSCRIBED, SUBSCRIBED, INSTALLED],
    )]);
    f.answers.insert(4, Ok(()));
    f.starts_needed.insert(4, 2);
    let (failed, lines) = go(Op::Unsubscribe, &[4], &mut f);
    assert_eq!((failed, lines), (0, vec!["ok 4 new".to_owned()]));
    assert_eq!(f.started, [(Op::Unsubscribe, 4), (Op::Unsubscribe, 4)]);
    assert!(f.started_at[1] < SETTLE.as_secs() + RESEND_AFTER.as_secs());
}

#[test]
fn no_answer_for_the_resend_time_asks_once_more() {
    let mut steps = vec![ABSENT; 25];
    steps.push(SUBSCRIBED);
    let mut f = fake(&[(7, steps)]);
    let (failed, lines) = go(Op::Subscribe, &[7], &mut f);
    assert_eq!((failed, lines), (0, vec!["ok 7 new".to_owned()]));
    let resend = SETTLE.as_secs() + RESEND_AFTER.as_secs();
    assert_eq!(f.started_at, [SETTLE.as_secs(), resend]);
}

/// Review M4, M5: after the second call an error may belong to the lost first one, so Steam's
/// own state decides.
#[test]
fn after_a_resend_an_error_answer_does_not_fail_what_steam_shows_done() {
    let arriving = item(true, false, true, None);
    let mut f = fake(&[(
        7,
        vec![ABSENT, arriving, ABSENT, ABSENT, arriving, SUBSCRIBED],
    )]);
    f.answers.insert(7, Err("timeout".into()));
    f.starts_needed.insert(7, 2);
    let (failed, lines) = go(Op::Subscribe, &[7], &mut f);
    assert_eq!((failed, lines), (0, vec!["ok 7 new".to_owned()]));
}

#[test]
fn the_call_is_repeated_at_most_once_then_the_stall_limit_fails_it() {
    let mut f = fake(&[(7, vec![ABSENT])]);
    let (failed, lines) = go(Op::Subscribe, &[7], &mut f);
    assert_eq!(failed, 1);
    assert_eq!(lines, ["fail 7 Steam made no progress for 60 seconds"]);
    assert_eq!(f.started.len(), 2);
}

#[test]
fn a_refused_second_call_fails_with_steams_reason() {
    let mut f = fake(&[(7, vec![ABSENT])]);
    f.refuse_again.insert(7);
    let (failed, lines) = go(Op::Subscribe, &[7], &mut f);
    assert_eq!(
        (failed, lines),
        (1, vec!["fail 7 Steam said no".to_owned()])
    );
    assert_eq!(f.started.len(), 2);
}

/// What the stress test on Linux showed: Steam answers the call, the state shows the change,
/// then the subscription list fetched at start undoes it. The answer is not lost, so only the
/// state tells; the call is made once more at once.
#[test]
fn a_change_undone_after_the_answer_is_asked_for_once_more() {
    let arriving = item(true, false, true, None);
    let mut f = fake(&[(
        7,
        vec![
            ABSENT,
            arriving,
            ABSENT,
            item(true, false, true, Some((1, 2))),
            SUBSCRIBED,
        ],
    )]);
    f.answers.insert(7, Ok(()));
    let (failed, lines) = go(Op::Subscribe, &[7], &mut f);
    assert_eq!(
        (failed, lines),
        (0, vec!["progress 7 1 2".into(), "ok 7 new".into()])
    );
    assert_eq!(f.started, [(Op::Subscribe, 7), (Op::Subscribe, 7)]);
    assert!(
        f.started_at[1] < SETTLE.as_secs() + RESEND_AFTER.as_secs(),
        "asked again at once, not after the resend time"
    );
}

/// The same for an unsubscribe that is answered and then not seen in the state for the resend
/// time (the state can flip back between two looks).
#[test]
fn a_change_that_never_shows_after_the_answer_is_asked_for_after_the_resend_time() {
    let mut steps = vec![SUBSCRIBED; 25];
    steps.push(INSTALLED);
    let mut f = fake(&[(4, steps)]);
    f.answers.insert(4, Ok(()));
    let (failed, lines) = go(Op::Unsubscribe, &[4], &mut f);
    assert_eq!((failed, lines), (0, vec!["ok 4 new".to_owned()]));
    assert_eq!(f.started, [(Op::Unsubscribe, 4), (Op::Unsubscribe, 4)]);
    let first_look = SETTLE.as_secs() + 1;
    assert_eq!(f.started_at[1], first_look + RESEND_AFTER.as_secs());
}

#[test]
fn the_second_call_is_the_last_even_when_it_is_undone_again() {
    let arriving = item(true, false, true, None);
    let mut f = fake(&[(7, vec![ABSENT, arriving, ABSENT, arriving, ABSENT])]);
    f.answers.insert(7, Ok(()));
    let (failed, lines) = go(Op::Subscribe, &[7], &mut f);
    assert_eq!(failed, 1);
    assert_eq!(
        lines.last().map(String::as_str),
        Some("fail 7 Steam made no progress for 60 seconds")
    );
    assert_eq!(f.started.len(), 2, "no third call");
}

#[test]
fn a_refused_second_call_after_the_answer_fails_with_steams_reason() {
    let arriving = item(true, false, true, None);
    let mut f = fake(&[(7, vec![ABSENT, arriving, ABSENT])]);
    f.answers.insert(7, Ok(()));
    f.refuse_again.insert(7);
    let (failed, lines) = go(Op::Subscribe, &[7], &mut f);
    assert_eq!(
        (failed, lines),
        (1, vec!["fail 7 Steam said no".to_owned()])
    );
    assert_eq!(f.started.len(), 2);
}

/// A subscription the player already had that Steam is still downloading is Steam's own work:
/// no call was made, so none is repeated.
#[test]
fn what_steam_downloads_by_itself_is_never_asked_for_again() {
    let mut f = fake(&[(7, vec![item(true, false, true, None)])]);
    let (failed, lines) = go(Op::Subscribe, &[7], &mut f);
    assert_eq!(failed, 1);
    assert_eq!(
        lines,
        ["fail 7 Steam made no progress for 60 seconds".to_owned()]
    );
    assert!(f.started.is_empty());
}
