//! `rwpm` front end. No arguments → GUI; `rwpm run <profile-id> -- <game exe> [args…]` → launch wrapper;
//! `rwpm mcp` → MCP server on stdio.
//!
//! On Windows this is a GUI program, so double-clicking it opens no console window (one
//! would also close the app when closed). `rwpm run` is started by Steam or RimSort and
//! needs no console; its problems go to `<profile>/rwpm.log`.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod guard;
mod gui;
mod instance;
mod mcp;
mod restart;
mod run;

use std::ffi::OsString;
use std::path::Path;
use std::process::ExitCode;

/// Exit code for a subcommand that is not built yet.
const EXIT_UNIMPLEMENTED: u8 = 2;

/// What `rwpm --help` prints. The `run` line is the one `rwpm run` shows on a usage error.
fn help_text() -> String {
    let run = run::USAGE.trim_start_matches("usage: ");
    format!(
        "rwpm {}\n\
         Separate RimWorld profiles, each with its own saves, settings and mod list.\n\
         \n\
         usage:\n  \
         rwpm                  open the app\n  \
         rwpm <file>           open a .rimrim backup or a .rimpack modpack\n  \
         {run}\n                        start a profile (a Steam launch option)\n  \
         rwpm mcp              run an MCP server on standard input and output\n  \
         rwpm --version        show the version\n  \
         rwpm --help           show this text",
        rwpm_core::VERSION
    )
}

/// `rwpm <file>.rimrim` or `<file>.rimpack`: how the OS hands over a backup or a modpack
/// that was opened. Only an existing file counts; any other word is still
/// an unknown command.
fn file_arg(args: &[OsString]) -> Option<gui::OpenFile> {
    gui::OpenFile::from_path(Path::new(args.first()?))
}

/// One window per user: with a window already open, it gets the file (or is brought to the
/// front) and this process ends.
fn open_window(open: Option<gui::OpenFile>) -> ExitCode {
    let request = open
        .clone()
        .map_or(instance::Request::Focus, instance::Request::Open);
    let after_update = std::env::var_os(instance::AFTER_UPDATE_ENV).is_some();
    if instance::claim(&request, after_update) == instance::Claim::HandedOver {
        return ExitCode::SUCCESS;
    }
    let result = gui::run_with(open);
    instance::release();
    match result {
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
    if let Some(file) = file_arg(&args) {
        return open_window(Some(file));
    }
    match args.first().and_then(|a| a.to_str()) {
        None => open_window(None),
        Some("--version" | "-V") => {
            println!("rwpm {}", rwpm_core::VERSION);
            ExitCode::SUCCESS
        }
        Some("--help" | "-h") => {
            println!("{}", help_text());
            ExitCode::SUCCESS
        }
        // Full i32 exit code (Windows game codes do not fit ExitCode's u8).
        Some("run") => std::process::exit(run::main(&args[1..])),
        Some("mcp") => std::process::exit(mcp::main()),
        Some(other) => {
            eprintln!("rwpm: unknown command `{other}`. Try `rwpm --help`.");
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
    fn help_names_every_command() {
        let help = help_text();
        for word in ["rwpm mcp", "--version", "--help", ".rimrim", ".rimpack"] {
            assert!(help.contains(word), "help lacks {word}");
        }
        assert!(help.contains(run::USAGE.trim_start_matches("usage: ")));
        assert!(help.contains(rwpm_core::VERSION));
    }

    #[test]
    fn only_an_existing_rimrim_or_rimpack_file_is_opened() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let (lower, upper) = (tmp.path().join("a.rimrim"), tmp.path().join("B.RIMRIM"));
        let (pack, shout) = (tmp.path().join("p.rimpack"), tmp.path().join("Q.RimPack"));
        let (text, folder) = (tmp.path().join("notes.txt"), tmp.path().join("dir.rimrim"));
        for file in [&lower, &upper, &pack, &shout, &text] {
            std::fs::write(file, "x").expect("write");
        }
        std::fs::create_dir(&folder).expect("dir");
        assert_eq!(
            file_arg(&arg(&lower)),
            Some(gui::OpenFile::Backup(lower.clone()))
        );
        assert_eq!(
            file_arg(&arg(&upper)),
            Some(gui::OpenFile::Backup(upper.clone()))
        );
        assert_eq!(
            file_arg(&arg(&pack)),
            Some(gui::OpenFile::Pack(pack.clone()))
        );
        assert_eq!(
            file_arg(&arg(&shout)),
            Some(gui::OpenFile::Pack(shout.clone()))
        );
        let missing = [
            tmp.path().join("missing.rimrim"),
            tmp.path().join("m.rimpack"),
        ];
        for no in [&text, &folder, &missing[0], &missing[1]] {
            assert_eq!(file_arg(&arg(no)), None, "{}", no.display());
        }
        assert_eq!(file_arg(&[]), None);
    }
}
