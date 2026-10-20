//! The work. For each id: start it, then wait until Steam says it is done, and report what
//! happens. It talks to Steam only through [`Backend`], so everything here is tested without
//! Steam. A run ends when every id is done, when Steam makes no progress for [`STALL`], or
//! when the stop flag is set (the program that started the helper closed its input).

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::args::Op;
use crate::event::{Event, Fresh};

/// No change in an item for this long fails it.
pub const STALL: Duration = Duration::from_secs(60);
/// Wait this long before the first subscribe or unsubscribe call. To Steam, starting this
/// program starts the game, and Steam then fetches the account's subscription list; a call
/// that lands before that list arrives can be undone by it (SPEC T82, stall trials). The list
/// came within about 1 s in about 10 runs, so this is an ESTIMATE with some margin.
pub const SETTLE: Duration = Duration::from_secs(3);
/// No answer to a call for this long: ask once more. An ESTIMATE: calls that worked took
/// 4-7 s including the download.
pub const RESEND_AFTER: Duration = Duration::from_secs(15);
const STEP: Duration = Duration::from_millis(200);

/// What Steam says about one item right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Item {
    pub subscribed: bool,
    pub installed: bool,
    /// A download is waiting, running, or the installed copy needs an update.
    pub busy: bool,
    /// Bytes done and bytes in all, while a download runs.
    pub progress: Option<(u64, u64)>,
}

impl Item {
    fn ready(self) -> bool {
        self.installed && !self.busy
    }
}

pub trait Backend {
    fn item(&self, id: u64) -> Item;
    /// Starts one id. `Err` = Steam refused at once.
    fn start(&mut self, op: Op, id: u64) -> Result<(), String>;
    /// Lets Steam run its callbacks. Returns the answers to subscribe and unsubscribe calls.
    fn poll(&mut self) -> Vec<(u64, Result<(), String>)>;
}

pub trait Clock {
    /// Waits one step and says how long the run has lasted.
    fn step(&mut self) -> Duration;
}

pub struct RealClock(Instant);

impl RealClock {
    pub fn start() -> Self {
        Self(Instant::now())
    }
}

impl Clock for RealClock {
    fn step(&mut self) -> Duration {
        std::thread::sleep(STEP);
        self.0.elapsed()
    }
}

#[derive(Clone, Copy)]
enum Stage {
    /// Waiting for the answer to a subscribe or unsubscribe call.
    Answer,
    /// Waiting for Steam to finish the download.
    Download,
}

struct Pending {
    id: u64,
    stage: Stage,
    fresh: Fresh,
    last: Item,
    changed_at: Duration,
    /// A subscribe or unsubscribe call was made (not a wait for what Steam does by itself).
    called: bool,
    /// Steam's state has shown the change that was asked for.
    seen: bool,
    /// The call has been made a second time.
    resent: bool,
}

enum Begin {
    Done(Event),
    Wait(Stage, Fresh),
}

fn begin(op: Op, id: u64, before: Item, backend: &mut dyn Backend) -> Begin {
    let done = |fresh| Begin::Done(Event::Ok { id, fresh });
    let start = |backend: &mut dyn Backend, stage, fresh| match backend.start(op, id) {
        Ok(()) => Begin::Wait(stage, fresh),
        Err(reason) => Begin::Done(Event::Fail { id, reason }),
    };
    match op {
        Op::Download if before.ready() => done(Fresh::Already),
        Op::Download => start(backend, Stage::Download, Fresh::New),
        Op::Subscribe if before.subscribed && before.ready() => done(Fresh::Already),
        // Steam downloads a subscribed item by itself: only wait.
        Op::Subscribe if before.subscribed => Begin::Wait(Stage::Download, Fresh::Already),
        Op::Subscribe => start(backend, Stage::Answer, Fresh::New),
        Op::Unsubscribe if !before.subscribed => done(Fresh::Already),
        Op::Unsubscribe => start(backend, Stage::Answer, Fresh::New),
    }
}

/// Steam's own state shows the change is complete.
fn finished(op: Op, item: Item) -> bool {
    match op {
        Op::Download => item.ready(),
        Op::Subscribe => item.subscribed && item.ready(),
        Op::Unsubscribe => !item.subscribed,
    }
}

/// Makes the call a second time. `Err` is Steam refusing it.
fn resend(
    op: Op,
    p: &mut Pending,
    why: &str,
    backend: &mut dyn Backend,
    now: Duration,
) -> Result<(), String> {
    eprintln!("rwpm-steam: {why} for {}, asking again", p.id);
    backend.start(op, p.id)?;
    p.resent = true;
    p.changed_at = now;
    Ok(())
}

/// One step of waiting for the answer to a subscribe or unsubscribe call. `Err` fails the id.
///
/// The call is made once more when it looks lost: Steam's state showed what was asked and then
/// went back (the subscription list Steam fetched at start undid it), or no answer came for
/// [`RESEND_AFTER`].
fn wait_answer(
    op: Op,
    p: &mut Pending,
    answer: Option<&Result<(), String>>,
    backend: &mut dyn Backend,
    now: Duration,
) -> Result<(), String> {
    match answer {
        // After a second call, an error may belong to the lost first one, and a second call to
        // what Steam already holds may answer with one (T82 review M4, M5): Steam's own state
        // and the stall limit decide instead.
        Some(Err(_)) if p.resent => {}
        Some(Err(reason)) => return Err(reason.clone()),
        // Steam has acknowledged the call. It is not done until Steam's own state shows it
        // (stage `Download`): a program that exits on the acknowledgement alone can leave the
        // change unsaved.
        Some(Ok(())) => {
            p.stage = Stage::Download;
            p.changed_at = now;
            p.seen |= backend.item(p.id).subscribed == matches!(op, Op::Subscribe);
            return Ok(());
        }
        None => {}
    }
    let asked = backend.item(p.id).subscribed == matches!(op, Op::Subscribe);
    if p.resent {
        if asked {
            p.stage = Stage::Download;
        }
    } else if asked {
        p.seen = true;
    } else if p.seen || now.saturating_sub(p.changed_at) >= RESEND_AFTER {
        resend(op, p, "no answer", backend, now)?;
    }
    Ok(())
}

/// One step of waiting for Steam's state after the call was answered. Steam can answer and then
/// undo the change: the subscription list it fetches when this program starts can arrive after
/// the call and put the old state back (SPEC, stall trials). The call is made once more when the
/// state showed the change and then went back, or when nothing changed for [`RESEND_AFTER`];
/// the id then waits for an answer again. `Err` fails the id.
fn watch_change(
    op: Op,
    p: &mut Pending,
    item: Item,
    backend: &mut dyn Backend,
    now: Duration,
) -> Result<(), String> {
    if !p.called || p.resent || matches!(op, Op::Download) {
        return Ok(());
    }
    if item.subscribed == matches!(op, Op::Subscribe) {
        p.seen = true;
        return Ok(());
    }
    if p.seen || now.saturating_sub(p.changed_at) >= RESEND_AFTER {
        resend(op, p, "change undone or missing", backend, now)?;
        p.stage = Stage::Answer;
    }
    Ok(())
}

/// Runs `op` on `ids`. Returns how many ids failed.
pub fn run(
    op: Op,
    ids: &[u64],
    backend: &mut dyn Backend,
    clock: &mut dyn Clock,
    stop: &AtomicBool,
    emit: &mut dyn FnMut(Event),
) -> usize {
    let mut now = Duration::ZERO;
    if matches!(op, Op::Subscribe | Op::Unsubscribe) {
        while now < SETTLE {
            if stop.load(Ordering::Relaxed) {
                for &id in ids {
                    emit(Event::Fail {
                        id,
                        reason: "cancelled".into(),
                    });
                }
                return ids.len();
            }
            now = clock.step();
        }
    }
    let mut failed = 0;
    let mut pending = Vec::new();
    for &id in ids {
        let before = backend.item(id);
        match begin(op, id, before, backend) {
            Begin::Done(event) => {
                failed += usize::from(matches!(event, Event::Fail { .. }));
                emit(event);
            }
            // `last` starts empty so the first thing seen is reported, even a download that
            // is already running.
            Begin::Wait(stage, fresh) => pending.push(Pending {
                id,
                stage,
                fresh,
                last: Item::default(),
                changed_at: now,
                called: matches!(stage, Stage::Answer),
                seen: false,
                resent: false,
            }),
        }
    }
    while !pending.is_empty() {
        if stop.load(Ordering::Relaxed) {
            for p in pending.drain(..) {
                failed += 1;
                emit(Event::Fail {
                    id: p.id,
                    reason: "cancelled".into(),
                });
            }
            break;
        }
        let now = clock.step();
        let answers = backend.poll();
        let mut still = Vec::new();
        for mut p in pending {
            if let Stage::Answer = p.stage {
                let answer = answers.iter().find(|(id, _)| *id == p.id).map(|(_, a)| a);
                if let Err(reason) = wait_answer(op, &mut p, answer, backend, now) {
                    failed += 1;
                    emit(Event::Fail { id: p.id, reason });
                    continue;
                }
            }
            if let Stage::Download = p.stage {
                let item = backend.item(p.id);
                if let Err(reason) = watch_change(op, &mut p, item, backend, now) {
                    failed += 1;
                    emit(Event::Fail { id: p.id, reason });
                    continue;
                }
                if let Stage::Answer = p.stage {
                    still.push(p);
                    continue;
                }
                if item != p.last {
                    if let (true, Some((done, total))) =
                        (item.progress != p.last.progress, item.progress)
                    {
                        emit(Event::Progress {
                            id: p.id,
                            done,
                            total,
                        });
                    }
                    p.last = item;
                    p.changed_at = now;
                }
                if finished(op, item) {
                    emit(Event::Ok {
                        id: p.id,
                        fresh: p.fresh,
                    });
                    continue;
                }
            }
            if now.saturating_sub(p.changed_at) >= STALL {
                failed += 1;
                emit(Event::Fail {
                    id: p.id,
                    reason: "Steam made no progress for 60 seconds".into(),
                });
                continue;
            }
            still.push(p);
        }
        pending = still;
    }
    failed
}

#[cfg(test)]
mod tests;
