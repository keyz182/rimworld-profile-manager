//! `rwpm run <profile-id> -- <game exe> [args…]` (I.cmd).
//!
//! Holds `run.lock` from before the "is a game running?" check until the last game
//! process exits (V11), waits through the in-game Restart child (V17), and exits
//! with the first game process's code. OFFLINE profiles pass the V5 guard first; a
//! player who cancels there exits 0 without starting the game. Every attempt is recorded
//! in the profile's `last-run.toml` (V32): who started it and how it went.

use std::ffi::OsString;
use std::fs::{File, TryLockError};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use rwpm_core::ProfileError;
use rwpm_core::launch::{self, LaunchPlan, Os, PayloadFiles};
use rwpm_core::paths;
use rwpm_core::profile::last_run::{self, LastRun, RunOutcome, StartedBy};
use rwpm_core::profile::{Mode, OsTrash, Profile, ProfileId, ProfileStore, any_game_running};
use rwpm_core::settings::AppSettings;
use rwpm_core::winhttp;

use crate::guard;

const EX_USAGE: i32 = 64;
const EX_TEMPFAIL: i32 = 75;
const EX_FAILED: i32 = 1;
const POLL: Duration = Duration::from_secs(1);
const USAGE: &str = "usage: rwpm run [--from rimsort] <profile-id> -- <game command…>";

pub(crate) enum RunError {
    Usage(String),
    Busy(String),
    /// A check said no (V8 loader); the game was not started.
    Refused(String),
    Failed(String),
}

impl RunError {
    fn exit_code(&self) -> i32 {
        match self {
            Self::Usage(_) => EX_USAGE,
            Self::Busy(_) => EX_TEMPFAIL,
            Self::Refused(_) | Self::Failed(_) => EX_FAILED,
        }
    }

    pub(crate) fn message(&self) -> &str {
        match self {
            Self::Usage(m) | Self::Busy(m) | Self::Refused(m) | Self::Failed(m) => m,
        }
    }
}

/// How a launch that got past the checks ended.
enum Ended {
    Played(i32),
    /// The player chose Cancel in the OFFLINE guard (V5).
    Cancelled,
}

/// Records attempts in `last-run.toml`; a failed write is logged, never fatal (V32).
struct Record<'a> {
    profile: &'a Profile,
    path: PathBuf,
    by: StartedBy,
}

impl Record<'_> {
    fn write(&self, outcome: RunOutcome, reason: &str, exit: Option<i32>) {
        let run = LastRun::now(self.by, outcome, reason.to_owned(), exit);
        if let Err(e) = run.write(&self.path) {
            guard::append_log(
                self.profile,
                &format!("cannot write `{}`: {e}", self.path.display()),
            );
        }
    }
}

/// Entry point: returns the process exit code (the game's, or an `EX_*` code).
pub fn main(args: &[OsString]) -> i32 {
    match run(args) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("rwpm run: {}", e.message());
            e.exit_code()
        }
    }
}

fn run(args: &[OsString]) -> Result<i32, RunError> {
    let (from, id, command) = parse(args)?;
    // Before this process changes its environment (`SteamAppId` for the game).
    let by = last_run::started_by(&|k| std::env::var_os(k), from.as_deref());
    let store = open_store()?;
    let profile = store.get(&id).map_err(|e| match e {
        ProfileError::NotFound(_) => RunError::Usage(e.to_string()),
        other => RunError::Failed(other.to_string()),
    })?;
    let record = Record {
        profile: &profile,
        path: LastRun::path(store.root(), &profile),
        by,
    };
    match launch(&store, &profile, &command, &record) {
        Ok(Ended::Played(code)) => {
            record.write(RunOutcome::Ok, "", Some(code));
            Ok(code)
        }
        Ok(Ended::Cancelled) => {
            record.write(RunOutcome::Refused, GUARD_CANCELLED, None);
            Ok(0)
        }
        Err(e) => {
            let outcome = match e {
                RunError::Busy(_) | RunError::Refused(_) => RunOutcome::Refused,
                RunError::Usage(_) | RunError::Failed(_) => RunOutcome::Failed,
            };
            record.write(outcome, e.message(), None);
            Err(e)
        }
    }
}

const GUARD_CANCELLED: &str = "the player chose Cancel when asked about mods that are not frozen";

fn launch(
    store: &ProfileStore<OsTrash>,
    profile: &Profile,
    command: &[OsString],
    record: &Record<'_>,
) -> Result<Ended, RunError> {
    let _lock = acquire_lock()?;
    if any_game_running() {
        return Err(RunError::Busy(
            "RimWorld is already running; close it first".into(),
        ));
    }
    if profile.mode == Mode::Offline
        && guard::run(store, profile, command) == guard::Outcome::Cancel
    {
        return Ok(Ended::Cancelled);
    }
    let mut payload = payload_files().map_err(RunError::Failed)?;
    if profile.mode == Mode::Offline && Os::current() == Os::Windows {
        payload.chain_target = windows_loader(profile, command)?;
    }
    let plan = launch::build(profile, command, &payload, Os::current(), &|k| {
        std::env::var_os(k)
    })
    .map_err(|e| RunError::Failed(e.to_string()))?;
    if !plan.replaced.is_empty() {
        eprintln!(
            "rwpm run: replaced launch args {:?} with this profile's",
            plan.replaced
        );
    }
    let mut cmd = command_for(&plan);
    let failed = |e: std::io::Error| {
        RunError::Failed(format!("cannot start `{}`: {e}", plan.program.display()))
    };
    let mut child = cmd.spawn().map_err(failed)?;
    // Started: say so now, so the app can show it worked while the game runs.
    record.write(RunOutcome::Ok, "", None);
    let code = child.wait().map_err(failed)?.code().unwrap_or(EX_FAILED);
    // V17: an in-game Restart spawns the child before the parent quits, so there is no gap.
    while any_game_running() {
        std::thread::sleep(POLL);
    }
    Ok(Ended::Played(code))
}

/// → (`--from` value, profile id, game command).
fn parse(args: &[OsString]) -> Result<(Option<String>, ProfileId, Vec<OsString>), RunError> {
    let usage = |why: &str| RunError::Usage(format!("{why}\n{USAGE}"));
    let (from, args) = match args {
        [flag, who, rest @ ..] if flag == "--from" => {
            if who != last_run::FROM_RIMSORT {
                return Err(usage("`--from` takes `rimsort`"));
            }
            (Some(last_run::FROM_RIMSORT.to_owned()), rest)
        }
        _ => (None, args),
    };
    let (id, rest) = args
        .split_first()
        .ok_or_else(|| usage("missing profile id"))?;
    let (sep, command) = rest
        .split_first()
        .ok_or_else(|| usage("missing `--` and game command"))?;
    if sep != "--" {
        return Err(usage("expected `--` after the profile id"));
    }
    if command.is_empty() {
        return Err(usage("missing game command after `--`"));
    }
    let id = id
        .to_str()
        .ok_or_else(|| usage("profile id is not valid text"))
        .and_then(|s| ProfileId::parse(s).map_err(|e| usage(&e.to_string())))?;
    Ok((from, id, command.to_vec()))
}

pub(crate) fn open_store() -> Result<ProfileStore<OsTrash>, RunError> {
    let app_dir =
        paths::app_data_dir().ok_or_else(|| RunError::Failed("no user data folder".into()))?;
    let root = AppSettings::load(&app_dir)
        .map_err(|e| RunError::Failed(e.to_string()))?
        .profiles_root(&app_dir);
    let save = paths::default_game_save_dir()
        .ok_or_else(|| RunError::Failed("no game save folder".into()))?;
    ProfileStore::open(root, save, OsTrash).map_err(|e| RunError::Failed(e.to_string()))
}

/// V11: exclusive OS lock, released on drop or if rwpm dies.
fn acquire_lock() -> Result<File, RunError> {
    let path =
        paths::run_lock_path().ok_or_else(|| RunError::Failed("no user data folder".into()))?;
    let failed =
        |e: std::io::Error| RunError::Failed(format!("cannot use lock `{}`: {e}", path.display()));
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(failed)?;
    }
    let file = File::options()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&path)
        .map_err(failed)?;
    match file.try_lock() {
        Ok(()) => Ok(file),
        Err(TryLockError::WouldBlock) => Err(RunError::Busy(
            "another profile is already being launched or played; close it first".into(),
        )),
        Err(TryLockError::Error(e)) => Err(failed(e)),
    }
}

/// Windows OFFLINE needs `winhttp.dll` beside the game: ours, or a compatible foreign
/// Doorstop whose own target the payload then chain-loads (T12, V8).
fn windows_loader(profile: &Profile, command: &[OsString]) -> Result<Option<PathBuf>, RunError> {
    let game_dir = command
        .first()
        .and_then(|p| Path::new(p).parent())
        .ok_or_else(|| RunError::Failed("cannot find the game folder".into()))?;
    let app_dir =
        paths::app_data_dir().ok_or_else(|| RunError::Failed("no user data folder".into()))?;
    winhttp::launch_check(game_dir, &app_dir).map_err(|e| {
        guard::append_log(profile, &format!("launch refused: {e}"));
        RunError::Refused(e.to_string())
    })
}

/// Payload + Doorstop beside this binary; this binary is the restart trampoline (V28).
pub(crate) fn payload_files() -> Result<PayloadFiles, String> {
    let exe = std::env::current_exe().map_err(|e| format!("cannot find rwpm itself: {e}"))?;
    let app_dir = exe
        .parent()
        .ok_or_else(|| "cannot find the rwpm install folder".to_owned())?;
    let mut payload = PayloadFiles::beside(app_dir);
    payload.trampoline = Some(exe.clone());
    Ok(payload)
}

/// The game command for a plan: program, argv[0], args, env and working dir.
pub(crate) fn command_for(plan: &LaunchPlan) -> Command {
    let mut cmd = Command::new(&plan.program);
    #[cfg(unix)]
    if let Some(arg0) = &plan.arg0 {
        std::os::unix::process::CommandExt::arg0(&mut cmd, arg0);
    }
    cmd.args(&plan.args);
    for key in &plan.env_remove {
        cmd.env_remove(key);
    }
    for (key, value) in &plan.env_set {
        cmd.env(key, value);
    }
    if let Some(dir) = &plan.cwd {
        cmd.current_dir(dir);
    }
    cmd
}
