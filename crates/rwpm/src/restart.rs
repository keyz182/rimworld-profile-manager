//! Restart trampoline. On Linux OFFLINE the game runs with argv[0] = rwpm, so the
//! in-game Restart (GenCommandLine.cs:45-58) runs `rwpm <game args…>` with the game's
//! environment. That environment no longer enables Doorstop (the payload turned it off
//! for helpers), so rebuild the launch and exec the real game in this process.
//!
//! The same path runs the game behind wrappers: `rwpm run` puts
//! `rwpm --rwpm-inner <game args…>` where the start script was, with the same env.
//!
//! No lock and no guard: the original `rwpm run` still holds the lock and is waiting.

use std::ffi::OsString;

use rwpm_core::launch::{self, ARG_INNER, ENV_GAME_EXE, ENV_RESTART, Os};
use rwpm_core::profile::{Profile, ProfileId};

use crate::{guard, run};

const SUBCOMMANDS: [&str; 3] = ["run", "--version", "-V"];

/// Restart mode: `RWPM_RESTART` is set and the args are the game's, not a subcommand.
pub fn is_restart(args: &[OsString]) -> bool {
    std::env::var_os(ENV_RESTART).is_some()
        && args
            .first()
            .is_some_and(|a| !SUBCOMMANDS.iter().any(|s| a == *s))
}

/// Only returns on failure (exec replaces this process).
pub fn main(args: &[OsString]) -> i32 {
    let mut profile = None;
    let err = exec_game(args, &mut profile);
    let msg = format!("restart failed: {err}");
    match &profile {
        Some(p) => guard::append_log(p, &msg),
        None => eprintln!("rwpm: {msg}"),
    }
    1
}

fn exec_game(args: &[OsString], found: &mut Option<Profile>) -> String {
    let id = std::env::var(ENV_RESTART).unwrap_or_default();
    let id = match ProfileId::parse(&id) {
        Ok(id) => id,
        Err(e) => return e.to_string(),
    };
    let Some(exe) = std::env::var_os(ENV_GAME_EXE) else {
        return format!("{ENV_GAME_EXE} is not set");
    };
    let profile = match run::open_store()
        .and_then(|s| s.get(&id).map_err(|e| run::RunError::Failed(e.to_string())))
    {
        Ok(p) => found.insert(p),
        Err(e) => return e.message().to_owned(),
    };
    let payload = match run::payload_files() {
        Ok(p) => p,
        Err(e) => return e,
    };
    let args = args.strip_prefix(&[ARG_INNER.into()]).unwrap_or(args);
    let mut command = vec![exe];
    command.extend(args.iter().cloned());
    let plan = match launch::build(profile, &command, &payload, Os::current(), &|k| {
        std::env::var_os(k)
    }) {
        Ok(plan) => plan,
        Err(e) => return e.to_string(),
    };
    exec(run::command_for(&plan))
}

#[cfg(unix)]
fn exec(mut cmd: std::process::Command) -> String {
    let err = std::os::unix::process::CommandExt::exec(&mut cmd);
    format!("cannot start the game: {err}")
}

#[cfg(not(unix))]
fn exec(_: std::process::Command) -> String {
    "restart trampoline is Linux-only".to_owned()
}
