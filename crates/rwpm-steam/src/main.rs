//! `rwpm-steam`: asks the running Steam client to download, subscribe to or unsubscribe from
//! Workshop mods of RimWorld, and prints one line per event (see `event.rs`).
//!
//! This is a separate program from `rwpm` on purpose. Valve's library is loaded when a program
//! that uses it starts, so a program that links it cannot start without the library beside it.
//! `rwpm` never links it. The program that starts this one must keep its standard input open:
//! closing it is how it asks this one to stop. Exit code: 0 all done, 1 some ids failed,
//! 2 bad command line, 3 Steam not reachable.

mod args;
mod drive;
mod event;
mod steam;

use std::io::{Read, Write};
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use args::Command;
use event::Event;

/// A flag that turns true when standard input closes.
fn watch_stdin() -> Arc<AtomicBool> {
    let stop = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&stop);
    std::thread::spawn(move || {
        let mut sink = [0u8; 256];
        let mut stdin = std::io::stdin();
        while matches!(stdin.read(&mut sink), Ok(n) if n > 0) {}
        flag.store(true, Ordering::Relaxed);
    });
    stop
}

fn main() -> ExitCode {
    let words: Vec<String> = std::env::args().skip(1).collect();
    let command = match args::parse(&words) {
        Ok(command) => command,
        Err(reason) => {
            eprintln!("rwpm-steam: {reason}");
            return ExitCode::from(2);
        }
    };
    let mut out = std::io::stdout();
    let mut emit = |event: Event| {
        // A reader that has gone away is the stop signal's job, not an error here.
        let _ = writeln!(out, "{event}");
        let _ = out.flush();
    };
    let mut backend = match steam::Real::connect() {
        Ok(backend) => backend,
        Err(reason) => {
            emit(Event::Error(reason));
            return ExitCode::from(3);
        }
    };
    match command {
        Command::Check => {
            emit(Event::Ready);
            ExitCode::SUCCESS
        }
        Command::Run { op, ids } => {
            let stop = watch_stdin();
            let failed = drive::run(
                op,
                &ids,
                &mut backend,
                &mut drive::RealClock::start(),
                &stop,
                &mut emit,
            );
            if failed == 0 {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }
    }
}
