//! One window per user. A second `rwpm` hands what it was asked to do (come to the front,
//! or open a `.rimrim` or `.rimpack`) to the window that is already open, over a local
//! socket (a Unix socket, or a Windows named pipe), then exits.
//!
//! Nothing here may stop the app from starting: any failure means "open a window", as
//! before.
//!
//! Wire format: one line per request, `focus` or `open<TAB><absolute path>`; the window
//! answers `ok` once it has queued the request.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(unix)]
use interprocess::local_socket::GenericFilePath;
#[cfg(windows)]
use interprocess::local_socket::GenericNamespaced;
use interprocess::local_socket::prelude::*;
use interprocess::local_socket::{Listener, ListenerOptions, Name, Stream};

pub use rwpm_core::self_update::AFTER_UPDATE_ENV;

use crate::gui::OpenFile;

/// What a second launch asks the open window to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// Come to the front.
    Focus,
    /// Open this file, then come to the front.
    Open(OpenFile),
}

/// What `claim` found out.
#[derive(Debug, PartialEq, Eq)]
pub enum Claim {
    /// This process is the one window: it serves requests from later launches.
    Primary,
    /// Another window took the request. This process has nothing left to do.
    HandedOver,
    /// No socket to be had. Open a window as if there were no other.
    Alone,
}

const ACK: &str = "ok\n";
/// A request is one short line; anything longer is not ours.
const MAX_LINE: u64 = 8192;
/// How long a launch waits for the open window to answer before it opens its own.
const ACK_WAIT: Duration = Duration::from_secs(2);
/// A socket file nobody answers on for this long is a crash's leftover.
const STALE_AFTER: Duration = Duration::from_millis(300);
const POLL: Duration = Duration::from_millis(30);
/// How often a window without the socket tries again (the old window may be closing).
const RETRY: Duration = Duration::from_secs(1);

type Sink = Arc<dyn Fn(Request) -> bool + Send + Sync>;

/// The listener `claim` won, until the window's subscription takes it.
static SLOT: Mutex<Option<Listener>> = Mutex::new(None);
static OWNED: AtomicBool = AtomicBool::new(false);

fn encode(request: &Request) -> Option<String> {
    match request {
        Request::Focus => Some("focus\n".to_owned()),
        Request::Open(OpenFile::Backup(path) | OpenFile::Pack(path)) => {
            // The open window has its own working directory.
            let path = std::path::absolute(path).ok()?;
            let path = path.to_str()?;
            // A name with a line break cannot travel in a line.
            (!path.contains(['\n', '\r'])).then(|| format!("open\t{path}\n"))
        }
    }
}

/// The same checks as for a file given on the command line: an existing file of ours.
fn decode(line: &str) -> Option<Request> {
    let line = line.trim_end_matches(['\r', '\n']);
    if line == "focus" {
        return Some(Request::Focus);
    }
    let path = Path::new(line.strip_prefix("open\t")?);
    if !path.is_absolute() {
        return None;
    }
    OpenFile::from_path(path).map(Request::Open)
}

#[cfg(unix)]
fn socket_path() -> Option<std::path::PathBuf> {
    // The runtime folder belongs to this user alone; without one, the app's own folder does.
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(std::path::PathBuf::from)
        .filter(|d| d.is_dir())
        .or_else(rwpm_core::paths::app_data_dir)?;
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join("rwpm.sock"))
}

fn default_name() -> Option<Name<'static>> {
    #[cfg(unix)]
    {
        socket_path()?.to_fs_name::<GenericFilePath>().ok()
    }
    #[cfg(windows)]
    {
        let user = std::env::var("USERNAME").ok()?;
        format!("rwpm-{user}")
            .to_ns_name::<GenericNamespaced>()
            .ok()
    }
}

/// Sends `request` to the open window. True once it has accepted it.
fn hand_over(name: &Name<'_>, request: &Request) -> bool {
    let Some(line) = encode(request) else {
        return false;
    };
    let Ok(stream) = Stream::connect(name.borrow()) else {
        return false;
    };
    // The stream has no read timeout, so a window that never answers is waited on here.
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(exchange(&stream, &line));
    });
    rx.recv_timeout(ACK_WAIT).unwrap_or(false)
}

fn exchange(stream: &Stream, line: &str) -> bool {
    let mut out = stream;
    if out.write_all(line.as_bytes()).is_err() {
        return false;
    }
    let mut answer = String::new();
    BufReader::new(stream).read_line(&mut answer).is_ok() && answer == ACK
}

fn create(name: &Name<'_>, overwrite: bool) -> io::Result<Listener> {
    ListenerOptions::new()
        .name(name.borrow())
        .try_overwrite(overwrite)
        .create_sync()
}

/// Takes the name. `AddrInUse` means a live window has it.
fn bind(name: &Name<'_>) -> io::Result<Listener> {
    match create(name, false) {
        Err(e) if e.kind() == io::ErrorKind::AddrInUse => {}
        other => return other,
    }
    // Held: by a window, by a launch that has bound but not yet listened, or by the
    // leftover of a crash. Only a name nobody answers on for a while is the last.
    let until = Instant::now() + STALE_AFTER;
    while Instant::now() < until {
        if Stream::connect(name.borrow()).is_ok() {
            return Err(io::ErrorKind::AddrInUse.into());
        }
        thread::sleep(POLL);
    }
    create(name, true)
}

enum Won {
    HandedOver,
    Primary(Listener),
}

/// Hands the launch to the open window, or becomes it. `after_update`: the old window is
/// still closing, so there is nobody to hand over to.
fn claim_at(name: &Name<'_>, request: &Request, after_update: bool) -> Option<Won> {
    // Twice: a launch that lost the race for the name finds the winner on the second pass.
    for _ in 0..2 {
        if !after_update && hand_over(name, request) {
            return Some(Won::HandedOver);
        }
        if let Ok(listener) = bind(name) {
            return Some(Won::Primary(listener));
        }
    }
    None
}

/// What this launch is: the window, a hand-over, or on its own.
pub fn claim(request: &Request, after_update: bool) -> Claim {
    let won = default_name().and_then(|name| claim_at(&name, request, after_update));
    match won {
        Some(Won::HandedOver) => Claim::HandedOver,
        Some(Won::Primary(listener)) => {
            if let Ok(mut slot) = SLOT.lock() {
                *slot = Some(listener);
                OWNED.store(true, Ordering::SeqCst);
            }
            Claim::Primary
        }
        None => Claim::Alone,
    }
}

/// Serves one connection: reads its line and queues the request.
fn handle(stream: &Stream, sink: &Sink) {
    let mut line = String::new();
    // Not a line of ours, or a probe that connected and left: nothing to answer.
    if BufReader::new(stream.take(MAX_LINE))
        .read_line(&mut line)
        .unwrap_or(0)
        == 0
    {
        return;
    }
    if let Some(request) = decode(&line)
        && sink(request)
    {
        let mut out = stream;
        let _ = out.write_all(ACK.as_bytes());
    }
}

/// Accepts connections for good; each on its own thread, so one that never writes holds
/// nobody up.
fn serve(listener: &Listener, sink: &Sink) {
    for connection in listener.incoming() {
        let Ok(stream) = connection else {
            thread::sleep(POLL);
            continue;
        };
        let sink = Arc::clone(sink);
        thread::spawn(move || handle(&stream, &sink));
    }
}

/// A window that did not get the name keeps asking for it.
fn wait_for_name() -> Option<Listener> {
    let name = default_name()?;
    loop {
        thread::sleep(RETRY);
        if let Ok(listener) = bind(&name) {
            OWNED.store(true, Ordering::SeqCst);
            return Some(listener);
        }
    }
}

/// Runs for the life of the window. `on_request` returns false once nobody is listening.
pub fn serve_window(on_request: impl Fn(Request) -> bool + Send + Sync + 'static) {
    let won = SLOT.lock().ok().and_then(|mut slot| slot.take());
    let Some(listener) = won.or_else(wait_for_name) else {
        return;
    };
    let sink: Sink = Arc::new(on_request);
    serve(&listener, &sink);
}

/// Called when the window has closed: a socket file left behind would cost the next
/// launch the wait for a stale one.
pub fn release() {
    #[cfg(unix)]
    if OWNED.load(Ordering::SeqCst)
        && let Some(path) = socket_path()
    {
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(all(test, unix))]
mod tests;
