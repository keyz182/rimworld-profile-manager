//! Running steamcmd to download Workshop mods into this app's own folder (T82).
//!
//! One process takes a batch of ids. What it delivered is read from its printed results, never
//! from its exit code (R52), and each delivered folder is then checked: its `About.xml` must
//! name the package id the pack promised, or the mod is thrown away. steamcmd runs with its
//! own `HOME`, so nothing under the player's real Steam folders is touched (R52).

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use super::output::{self, Event, Reader};
use super::{Error, SOURCE, Seen, check_hash, download, unpack};
use crate::consent::{Consent, Scope};
use crate::fetch::plain_id;
use crate::fetch::{BUSY_TRIES, BUSY_WAIT, Fresh, Outcome, Status, check_identity, is_busy};

/// RimWorld's Steam app id.
const APP: &str = "294100";
/// Ids per steamcmd process. A command line has a length limit, and a batch that goes wrong
/// costs less.
pub const BATCH: usize = 100;
const POLL: Duration = Duration::from_millis(100);
/// Free space wanted in steamcmd's folder before each batch. An ESTIMATE, not a measured
/// number: mods are usually a few MB to a few hundred MB, and a batch holds up to 100.
pub const MIN_FREE_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// Where steamcmd lives: `<app data>/steamcmd/`.
#[derive(Debug, Clone)]
pub struct Layout {
    root: PathBuf,
}

impl Layout {
    pub fn new(app_dir: &Path) -> Self {
        Self {
            root: app_dir.join("steamcmd"),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn program(&self) -> PathBuf {
        self.root.join(if cfg!(windows) {
            "steamcmd.exe"
        } else {
            "steamcmd.sh"
        })
    }

    /// steamcmd's own `HOME` on Linux and macOS, so it leaves the real one alone.
    pub fn home(&self) -> PathBuf {
        self.root.join("home")
    }

    /// steamcmd's `+force_install_dir`. It must NOT be steamcmd's own folder: steamcmd then
    /// refuses it ("set the game install path to something other than the Steam install
    /// folder") and writes under `$HOME/Steam` instead (found by the live run, T82b).
    pub fn install_dir(&self) -> PathBuf {
        self.root.join("install")
    }

    /// Where steamcmd puts a downloaded mod: `<this>/<id>/`.
    pub fn content_dir(&self) -> PathBuf {
        self.install_dir()
            .join("steamapps")
            .join("workshop")
            .join("content")
            .join(APP)
    }

    /// Written last, once everything is unpacked. Without it the folder may hold half an
    /// install (the program file comes first), so it is installed again (T82 review H1).
    fn marker(&self) -> PathBuf {
        self.root.join(".rwpm-installed")
    }

    /// Empties the folder the mods are downloaded into. It is steamcmd's own copy: nothing
    /// else lives there, and what a cancelled or earlier run left must not be mistaken for
    /// this run's delivery (T82 review M2).
    pub fn clear_content(&self) -> std::io::Result<()> {
        match std::fs::remove_dir_all(self.content_dir()) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }

    pub fn installed(&self) -> bool {
        self.program().is_file() && self.marker().is_file()
    }
}

/// Downloads and unpacks steamcmd if it is not there yet. Needs the player's recorded consent.
/// `Ok(Some(hash))` is the first-seen checksum, to be kept in the settings.
pub fn ensure_installed(
    layout: &Layout,
    consent: &Consent,
    first_hash: Option<&str>,
) -> Result<Option<String>, Error> {
    install_with(layout, consent, first_hash, download::bootstrap)
}

pub(super) fn install_with(
    layout: &Layout,
    consent: &Consent,
    first_hash: Option<&str>,
    fetch: impl FnOnce() -> Result<Vec<u8>, Error>,
) -> Result<Option<String>, Error> {
    if layout.installed() {
        return Ok(None);
    }
    if !consent.covers(Scope::Steamcmd, layout.root()) {
        return Err(Error::NoConsent);
    }
    let bytes = fetch()?;
    let seen = check_hash(first_hash, &bytes)?;
    unpack::install(SOURCE.kind, &bytes, layout.root())?;
    std::fs::write(layout.marker(), b"unpacked completely\n")
        .map_err(Error::io("write", &layout.marker()))?;
    Ok(match seen {
        Seen::First(hash) => Some(hash),
        Seen::Same => None,
    })
}

/// Downloads `wanted` (Workshop id, the package id the pack names). One outcome per mod, in
/// the order asked. `progress` gets the mods finished and the mods asked for.
pub fn download(
    layout: &Layout,
    wanted: &[(u64, String)],
    cancel: &AtomicBool,
    quiet: Duration,
    progress: &mut dyn FnMut(usize, usize),
) -> Vec<Outcome> {
    download_with(layout, wanted, cancel, quiet, MIN_FREE_BYTES, progress)
}

/// `download` with the free space wanted before each batch given (tests set it).
pub fn download_with(
    layout: &Layout,
    wanted: &[(u64, String)],
    cancel: &AtomicBool,
    quiet: Duration,
    min_free: u64,
    progress: &mut dyn FnMut(usize, usize),
) -> Vec<Outcome> {
    let total = wanted.len();
    let mut outcomes: Vec<Outcome> = Vec::with_capacity(total);
    if let Err(e) = layout.clear_content() {
        let why = format!("steamcmd's folder could not be emptied first: {e}");
        return failed_all(wanted, &why);
    }
    for batch in wanted.chunks(BATCH) {
        if let Some(free) = crate::space::free_space(layout.root()).filter(|f| *f < min_free) {
            let why = low_space(free);
            outcomes.extend(failed_all(batch, &why));
            continue;
        }
        let before = outcomes.len();
        let mut part = run_batch(layout, batch, cancel, quiet, &mut |n| {
            progress(before + n, total);
        });
        outcomes.append(&mut part);
    }
    outcomes
}

fn failed_all(wanted: &[(u64, String)], why: &str) -> Vec<Outcome> {
    wanted
        .iter()
        .map(|(id, _)| Outcome {
            id: *id,
            status: Status::Failed(why.to_owned()),
        })
        .collect()
}

fn low_space(free: u64) -> String {
    format!(
        "There is not enough free disk space for steamcmd ({} MB left). Free some space and try again.",
        free / 1024 / 1024
    )
}

fn run_batch(
    layout: &Layout,
    batch: &[(u64, String)],
    cancel: &AtomicBool,
    quiet: Duration,
    on_done: &mut dyn FnMut(usize),
) -> Vec<Outcome> {
    let ids: Vec<u64> = batch.iter().map(|(id, _)| *id).collect();
    let mut results: BTreeMap<u64, Status> = BTreeMap::new();
    let mut reader = Reader::default();
    let ended = run_process(layout, &ids, cancel, quiet, &mut |text| {
        for event in reader.feed(text) {
            match event {
                Event::Done(id) if ids.contains(&id) => {
                    results.insert(id, Status::Done(Fresh::New));
                    on_done(results.len());
                }
                Event::Failed { id, reason } if ids.contains(&id) => {
                    results.insert(id, Status::Failed(output::plain_reason(&reason)));
                }
                Event::Done(_) | Event::Failed { .. } => {}
            }
        }
    });
    let unanswered = match ended {
        Err(why) => why,
        Ok(()) => output::explain(&reader),
    };
    let mut outcomes: Vec<Outcome> = batch
        .iter()
        .map(|(id, _)| Outcome {
            id: *id,
            status: results
                .remove(id)
                .unwrap_or_else(|| Status::Failed(unanswered.clone())),
        })
        .collect();
    verify(layout, batch, &mut outcomes);
    outcomes
}

/// A delivered folder that is not the promised mod is thrown away and reported.
fn verify(layout: &Layout, batch: &[(u64, String)], outcomes: &mut [Outcome]) {
    let content = layout.content_dir();
    for (outcome, (id, expected)) in outcomes.iter_mut().zip(batch) {
        if !matches!(outcome.status, Status::Done(_)) {
            continue;
        }
        if let Err(found) = check_identity(&content, *id, expected) {
            // The folder is steamcmd's own copy, named by a number, so removing it is safe.
            let _ = std::fs::remove_dir_all(content.join(id.to_string()));
            outcome.status = Status::Mismatch {
                expected: plain_id(expected),
                found,
            };
        }
    }
}

fn command(layout: &Layout, ids: &[u64]) -> Command {
    let mut cmd = Command::new(layout.program());
    cmd.current_dir(layout.root())
        .arg("+force_install_dir")
        .arg(layout.install_dir())
        .args(["+login", "anonymous"]);
    for id in ids {
        cmd.arg("+workshop_download_item")
            .arg(APP)
            .arg(id.to_string());
    }
    cmd.arg("+quit")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if !cfg!(windows) {
        // Its own HOME, and no XDG folder that could lead it back to the real Steam.
        cmd.env("HOME", layout.home())
            .env_remove("XDG_DATA_HOME")
            .env_remove("XDG_CONFIG_HOME");
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // steamcmd.sh starts the real program as a child and waits, so stopping means
        // stopping the whole group.
        cmd.process_group(0);
    }
    cmd
}

fn spawn(layout: &Layout, ids: &[u64]) -> Result<Child, String> {
    std::fs::create_dir_all(layout.home())
        .map_err(|e| format!("Could not make steamcmd's folder: {e}"))?;
    let mut tries = 0;
    loop {
        match command(layout, ids).spawn() {
            Ok(child) => return Ok(child),
            Err(e) if is_busy(&e) && tries < BUSY_TRIES => {
                tries += 1;
                std::thread::sleep(BUSY_WAIT);
            }
            Err(e) => return Err(format!("steamcmd could not be started: {e}")),
        }
    }
}

/// Ends steamcmd and what it started. `false` when that could not be done.
#[cfg(unix)]
fn stop(child: &mut Child) -> bool {
    // The shell's own `kill`, so it works where no `kill` program is installed (T82 review
    // M3). A negative process id names the group the child leads; the id is an argument,
    // never part of the script text.
    let signalled = Command::new("sh")
        .args([
            "-c",
            "kill -s KILL -- \"-$1\"",
            "sh",
            &child.id().to_string(),
        ])
        .status()
        .is_ok_and(|status| status.success());
    let _ = child.kill();
    signalled
}

#[cfg(not(unix))]
fn stop(child: &mut Child) -> bool {
    child.kill().is_ok()
}

/// What the player is told when the run was ended early.
pub(super) fn ended_reason(why: &str, stopped: bool) -> String {
    if stopped {
        why.to_owned()
    } else {
        format!("{why}, but steamcmd may still be running")
    }
}

fn pump(mut from: impl Read + Send + 'static, to: mpsc::Sender<String>) {
    std::thread::spawn(move || {
        let mut chunk = [0_u8; 4096];
        while let Ok(n) = from.read(&mut chunk) {
            if n == 0
                || to
                    .send(String::from_utf8_lossy(&chunk[..n]).into_owned())
                    .is_err()
            {
                break;
            }
        }
    });
}

/// Runs steamcmd and hands everything it prints to `on_text`. Returns early, after stopping it,
/// when `cancel` is set or it has printed nothing for `quiet`.
fn run_process(
    layout: &Layout,
    ids: &[u64],
    cancel: &AtomicBool,
    quiet: Duration,
    on_text: &mut dyn FnMut(&str),
) -> Result<(), String> {
    let mut child = spawn(layout, ids)?;
    let (tx, rx) = mpsc::channel();
    if let Some(out) = child.stdout.take() {
        pump(out, tx.clone());
    }
    if let Some(err) = child.stderr.take() {
        pump(err, tx.clone());
    }
    drop(tx);
    let mut last = Instant::now();
    let mut ended = Ok(());
    loop {
        if cancel.load(Ordering::Relaxed) {
            ended = Err("cancelled".to_owned());
            break;
        }
        match rx.recv_timeout(POLL) {
            Ok(text) => {
                last = Instant::now();
                on_text(&text);
            }
            Err(RecvTimeoutError::Timeout) if last.elapsed() >= quiet => {
                ended = Err("steamcmd stopped answering.".to_owned());
                break;
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    if let Err(why) = &ended {
        let stopped = stop(&mut child);
        ended = Err(ended_reason(why, stopped));
    }
    let _ = child.wait();
    ended
}
