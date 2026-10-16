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
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// Exit code for a subcommand that is not built yet.
const EXIT_UNIMPLEMENTED: u8 = 2;

/// `rwpm <file>.rimrim`: how the OS hands over a backup that was opened (V83, T75). Only
/// an existing file counts; any other word is still an unknown command.
fn backup_arg(args: &[OsString]) -> Option<PathBuf> {
    let path = Path::new(args.first()?);
    let named = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("rimrim"));
    (named && path.is_file()).then(|| path.to_path_buf())
}

fn open_window(backup: Option<PathBuf>) -> ExitCode {
    match gui::run_with(backup) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("rwpm: {e}");
            ExitCode::FAILURE
        }
    }
}

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    if restart::is_restart(&args) {
        std::process::exit(restart::main(&args));
    }
    if let Some(backup) = backup_arg(&args) {
        return open_window(Some(backup));
    }
    match args.first().and_then(|a| a.to_str()) {
        None => open_window(None),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn arg(path: &Path) -> Vec<OsString> {
        vec![path.as_os_str().to_owned()]
    }

    #[test]
    fn only_an_existing_rimrim_file_is_a_backup_argument() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let (lower, upper) = (tmp.path().join("a.rimrim"), tmp.path().join("B.RIMRIM"));
        let (text, folder) = (tmp.path().join("notes.txt"), tmp.path().join("dir.rimrim"));
        for file in [&lower, &upper, &text] {
            std::fs::write(file, "x").expect("write");
        }
        std::fs::create_dir(&folder).expect("dir");
        assert_eq!(backup_arg(&arg(&lower)), Some(lower.clone()));
        assert_eq!(backup_arg(&arg(&upper)), Some(upper.clone()));
        for no in [&text, &folder, &tmp.path().join("missing.rimrim")] {
            assert_eq!(backup_arg(&arg(no)), None, "{}", no.display());
        }
        assert_eq!(backup_arg(&[]), None);
    }
}
