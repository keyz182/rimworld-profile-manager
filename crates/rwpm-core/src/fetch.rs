//! Fetching the missing mods of a modpack through the `rwpm-steam` helper.
//!
//! The helper is a separate program that asks the running Steam client to download, subscribe
//! to or unsubscribe from Workshop mods and prints one line per event. This module starts it,
//! reads the lines, can stop it at once, and then checks what Steam delivered. A pack is made
//! by a stranger, so its mod names mean nothing: each fetched folder must hold the package id
//! the pack promised, or the mod is not kept.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use crate::mods::about_field;

pub mod record;

/// The helper's file name. It sits beside `rwpm`.
pub const HELPER: &str = if cfg!(windows) {
    "rwpm-steam.exe"
} else {
    "rwpm-steam"
};

/// Ids per helper run. The helper refuses more than 500; this is well under that. Not
/// measured: an estimate that keeps each run short.
pub const BATCH: usize = 200;

/// No line from the helper for this long means it is stuck.
pub const QUIET: Duration = Duration::from_secs(120);
/// Workshop copies of a package id that also exists locally carry this suffix.
const STEAM_POSTFIX: &str = "_steam";
const POLL: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Download,
    Subscribe,
    Unsubscribe,
}

impl Op {
    fn word(self) -> &'static str {
        match self {
            Self::Download => "download",
            Self::Subscribe => "subscribe",
            Self::Unsubscribe => "unsubscribe",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fresh {
    /// This run made the change.
    New,
    /// It was so before this run.
    Already,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Done(Fresh),
    /// In plain words, for the player.
    Failed(String),
    /// What Steam delivered is not the mod the pack named. It is not kept.
    Mismatch {
        expected: String,
        found: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub id: u64,
    pub status: Status,
}

/// What to fetch: each mod's Workshop id with the package id the pack names for it.
#[derive(Debug, Clone)]
pub struct Request<'a> {
    pub op: Op,
    pub wanted: &'a [(u64, String)],
    /// Where Steam keeps downloaded mods, `<library>/steamapps/workshop/content/294100`. Needed
    /// to check what was delivered.
    pub workshop_dir: Option<&'a Path>,
    /// How long the helper may stay silent.
    pub quiet: Duration,
}

/// One line of the helper's output, as far as this program understands it.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Line {
    Ready,
    Progress { id: u64, done: u64, total: u64 },
    Ok { id: u64, fresh: Fresh },
    Fail { id: u64, reason: String },
    Error(String),
}

fn number(word: Option<&str>) -> Option<u64> {
    word?.parse().ok()
}

/// A line the helper printed. Anything that does not fit is ignored.
fn parse_line(line: &str) -> Option<Line> {
    let line = line.trim();
    let (word, rest) = line.split_once(' ').unwrap_or((line, ""));
    match word {
        "ok" if rest.is_empty() => Some(Line::Ready),
        "ok" => {
            let mut parts = rest.split(' ');
            let id = number(parts.next())?;
            let fresh = match parts.next()? {
                "new" => Fresh::New,
                "already" => Fresh::Already,
                _ => return None,
            };
            Some(Line::Ok { id, fresh })
        }
        "progress" => {
            let mut parts = rest.split(' ');
            let (id, done, total) = (
                number(parts.next())?,
                number(parts.next())?,
                number(parts.next())?,
            );
            Some(Line::Progress { id, done, total })
        }
        "fail" => {
            let (id, reason) = rest.split_once(' ').unwrap_or((rest, ""));
            Some(Line::Fail {
                id: id.parse().ok()?,
                reason: reason.trim().to_owned(),
            })
        }
        "error" => Some(Line::Error(rest.trim().to_owned())),
        _ => None,
    }
}

/// The helper beside `exe`, when it is there.
pub fn helper_beside(exe: &Path) -> Option<PathBuf> {
    let path = exe.with_file_name(HELPER);
    path.is_file().then_some(path)
}

/// Starts the helper with its input open. Closing that input is how it is told to stop.
fn spawn(helper: &Path, op: &str, args: &[String]) -> Result<Child, String> {
    let mut tries = 0;
    loop {
        let mut command = Command::new(helper);
        command
            .arg(op)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        no_window(&mut command);
        let started = command.spawn();
        match started {
            Ok(child) => return Ok(child),
            // A program written a moment ago is "busy" while another thread's fork still
            // holds the file open. It clears in a few milliseconds.
            Err(e) if is_busy(&e) && tries < BUSY_TRIES => {
                tries += 1;
                std::thread::sleep(BUSY_WAIT);
            }
            Err(e) => return Err(format!("The Steam helper could not be started: {e}")),
        }
    }
}

/// Starts a console program without a console window. The app has no console of its own, so
/// without this Windows opens a window for every helper and steamcmd run, and Windows Terminal
/// keeps it open afterwards when the exit code is not 0 (found in the real Windows window).
/// A no-op elsewhere.
#[cfg(windows)]
pub(crate) fn no_window(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
pub(crate) fn no_window(_command: &mut Command) {}

pub(crate) const BUSY_TRIES: u32 = 10;
pub(crate) const BUSY_WAIT: Duration = Duration::from_millis(30);

pub(crate) fn is_busy(error: &std::io::Error) -> bool {
    error.kind() == std::io::ErrorKind::ExecutableFileBusy
}

/// Is Steam running and does it know the game? `Err` says why not, in plain words.
pub fn check(helper: &Path) -> Result<(), String> {
    let mut child = spawn(helper, "check", &[])?;
    let stdin = child.stdin.take();
    let out = child
        .stdout
        .take()
        .ok_or("The Steam helper gave no answer.")?;
    let first = BufReader::new(out).lines().map_while(Result::ok).next();
    drop(stdin);
    let _ = child.wait();
    match first.as_deref().and_then(parse_line) {
        Some(Line::Ready) => Ok(()),
        Some(Line::Error(reason)) if !reason.is_empty() => Err(reason),
        _ => Err("The Steam helper gave no answer.".into()),
    }
}

/// Runs the helper and hands every line it prints to `on_line`. Returns early, after ending the
/// helper, when `cancel` is set or the helper has been silent for `quiet`.
fn run_helper(
    helper: &Path,
    op: Op,
    ids: &[u64],
    cancel: &AtomicBool,
    quiet: Duration,
    mut on_line: impl FnMut(Line),
) -> Result<(), String> {
    let args: Vec<String> = ids.iter().map(u64::to_string).collect();
    let mut child = spawn(helper, op.word(), &args)?;
    let stdin = child.stdin.take();
    let out = child
        .stdout
        .take()
        .ok_or("The Steam helper gave no answer.")?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    let mut last_line = Instant::now();
    let mut ended = Ok(());
    loop {
        if cancel.load(Ordering::Relaxed) {
            ended = Err("cancelled".to_owned());
            break;
        }
        match rx.recv_timeout(POLL) {
            Ok(text) => {
                last_line = Instant::now();
                if let Some(line) = parse_line(&text) {
                    on_line(line);
                }
            }
            Err(RecvTimeoutError::Timeout) if last_line.elapsed() >= quiet => {
                ended = Err("The Steam helper stopped answering.".to_owned());
                break;
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    // Closing the input asks it to stop; ending it makes sure it has.
    drop(stdin);
    if ended.is_err() {
        let _ = child.kill();
    }
    let _ = child.wait();
    ended
}

/// What `Status::Mismatch::found` says when the delivered folder has no readable About.xml.
pub const NO_ABOUT: &str = "no About.xml";
/// What `Status::Mismatch::found` says when no folder was looked at, because none is known.
pub const NOT_FOUND: &str = "the downloaded mod was not found";

/// The package id the pack names, as the mod's own About.xml would write it.
pub(crate) fn plain_id(id: &str) -> String {
    let lower = id.to_lowercase();
    lower
        .strip_suffix(STEAM_POSTFIX)
        .map_or(lower.clone(), str::to_owned)
}

/// `Ok` when the folder Steam delivered holds the mod the pack named; `Err` says what is there.
pub(crate) fn check_identity(workshop_dir: &Path, id: u64, expected: &str) -> Result<(), String> {
    let folder = workshop_dir.join(id.to_string());
    let found = about_field(&folder, "packageId").map(|f| f.to_lowercase());
    match found {
        Some(found) if found == plain_id(expected) => Ok(()),
        Some(found) => Err(found),
        None => Err(NO_ABOUT.to_owned()),
    }
}

/// Fetches `request.wanted`. One outcome per mod, in the order asked. `progress` gets the bytes
/// done and the bytes in all, for every mod together.
pub fn fetch(
    helper: &Path,
    request: &Request<'_>,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(u64, u64),
) -> Vec<Outcome> {
    let ids: Vec<u64> = request.wanted.iter().map(|(id, _)| *id).collect();
    let mut results: BTreeMap<u64, Status> = BTreeMap::new();
    let mut sizes: BTreeMap<u64, (u64, u64)> = BTreeMap::new();
    let mut fatal: Option<String> = None;
    // A pack can name any number of mods, and the helper takes a limited number of ids per
    // run, so they go in batches. A batch that was cancelled or went silent
    // ends the run: the mods after it are reported with that reason.
    let mut ended = Ok(());
    for batch in ids.chunks(BATCH) {
        if ended.is_err() {
            break;
        }
        ended = run_helper(
            helper,
            request.op,
            batch,
            cancel,
            request.quiet,
            |line| match line {
                Line::Progress { id, done, total } if ids.contains(&id) => {
                    sizes.insert(id, (done, total));
                    let (done, total) = sizes
                        .values()
                        .fold((0, 0), |(d, t), (sd, st)| (d + sd, t + st));
                    progress(done, total);
                }
                Line::Ok { id, fresh } if ids.contains(&id) => {
                    results.insert(id, Status::Done(fresh));
                }
                Line::Fail { id, reason } if ids.contains(&id) => {
                    results.insert(id, Status::Failed(reason));
                }
                Line::Error(reason) => fatal = Some(reason),
                Line::Ready | Line::Progress { .. } | Line::Ok { .. } | Line::Fail { .. } => {}
            },
        );
    }
    let unanswered = match (&ended, fatal) {
        (Err(why), _) => why.clone(),
        (Ok(()), Some(reason)) => reason,
        (Ok(()), None) => "Steam gave no answer for this mod.".to_owned(),
    };
    let mut outcomes: Vec<Outcome> = request
        .wanted
        .iter()
        .map(|(id, _)| Outcome {
            id: *id,
            status: results
                .remove(id)
                .unwrap_or_else(|| Status::Failed(unanswered.clone())),
        })
        .collect();
    if request.op != Op::Unsubscribe {
        verify(helper, request, &mut outcomes);
    }
    outcomes
}

/// Turns a delivered mod that is not the promised one into a mismatch. One that this run
/// subscribed to is unsubscribed again at once, as well as can be done.
fn verify(helper: &Path, request: &Request<'_>, outcomes: &mut [Outcome]) {
    let mut undo = Vec::new();
    for (outcome, (id, expected)) in outcomes.iter_mut().zip(request.wanted) {
        let Status::Done(fresh) = outcome.status else {
            continue;
        };
        let checked = match request.workshop_dir {
            Some(dir) => check_identity(dir, *id, expected),
            None => Err(NOT_FOUND.to_owned()),
        };
        if let Err(found) = checked {
            if request.op == Op::Subscribe && fresh == Fresh::New {
                undo.push(*id);
            }
            outcome.status = Status::Mismatch {
                expected: plain_id(expected),
                found,
            };
        }
    }
    if !undo.is_empty() {
        let never = AtomicBool::new(false);
        let _ = run_helper(
            helper,
            Op::Unsubscribe,
            &undo,
            &never,
            request.quiet,
            |_| {},
        );
    }
}

#[cfg(test)]
mod tests;
