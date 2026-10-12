//! `rwpm` front end. No arguments → GUI (T15); `rwpm run <profile-id> -- <game exe> [args…]` → launch wrapper.
//!
//! On Windows this is a GUI program, so double-clicking it opens no console window (one
//! would also close the app when closed). `rwpm run` is started by Steam or RimSort and
//! needs no console; its problems go to `<profile>/rwpm.log`.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod guard;
mod gui;
mod restart;
mod run;

use std::ffi::OsString;
use std::process::ExitCode;

/// Exit code for a subcommand that is not built yet.
const EXIT_UNIMPLEMENTED: u8 = 2;

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    if restart::is_restart(&args) {
        std::process::exit(restart::main(&args));
    }
    match args.first().and_then(|a| a.to_str()) {
        None => match gui::run() {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("rwpm: {e}");
                ExitCode::FAILURE
            }
        },
        Some("--version" | "-V") => {
            println!("rwpm {}", rwpm_core::VERSION);
            ExitCode::SUCCESS
        }
        // Full i32 exit code (Windows game codes do not fit ExitCode's u8).
        Some("run") => std::process::exit(run::main(&args[1..])),
        Some(other) => {
            eprintln!("rwpm: unknown command `{other}`");
            ExitCode::from(EXIT_UNIMPLEMENTED)
        }
    }
}
