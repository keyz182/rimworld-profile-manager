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

/// Runs `op` on `ids`. Returns how many ids failed.
pub fn run(
    op: Op,
    ids: &[u64],
    backend: &mut dyn Backend,
    clock: &mut dyn Clock,
    stop: &AtomicBool,
    emit: &mut dyn FnMut(Event),
) -> usize {
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
                changed_at: Duration::ZERO,
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
            if let (Stage::Answer, Some((_, answer))) =
                (p.stage, answers.iter().find(|(id, _)| *id == p.id))
            {
                match answer {
                    Err(reason) => {
                        failed += 1;
                        emit(Event::Fail {
                            id: p.id,
                            reason: reason.clone(),
                        });
                        continue;
                    }
                    // Steam has acknowledged the call. It is not done until Steam's own
                    // state shows it (below): a program that exits on the acknowledgement
                    // alone can leave the change unsaved.
                    Ok(()) => {
                        p.stage = Stage::Download;
                        p.changed_at = now;
                    }
                }
            }
            if let Stage::Download = p.stage {
                let item = backend.item(p.id);
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
