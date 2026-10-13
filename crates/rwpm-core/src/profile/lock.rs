//! `<app dir>/run.lock`: the one guard that stops a profile launch racing a bulk file
//! operation. Shared by the Profiles-folder Move (V14) and the build store (V59).

use std::fs::{File, TryLockError};
use std::path::Path;

use crate::error::ProfileError;

pub(crate) const RUN_LOCK: &str = "run.lock";

/// Takes the lock, or `GameRunning` when something else holds it (V59).
pub(crate) fn take_run_lock(app_dir: &Path) -> Result<File, ProfileError> {
    try_run_lock(app_dir)?.ok_or(ProfileError::GameRunning)
}

/// `None` when another process holds the lock. For housekeeping that must never fight a
/// capture in flight: it steps aside instead of failing.
pub(crate) fn try_run_lock(app_dir: &Path) -> Result<Option<File>, ProfileError> {
    let path = app_dir.join(RUN_LOCK);
    std::fs::create_dir_all(app_dir).map_err(ProfileError::io("create", app_dir))?;
    let file = File::options()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&path)
        .map_err(ProfileError::io("open", &path))?;
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(e)) => Err(ProfileError::io("lock", &path)(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_take_reports_the_game_running() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let _held = take_run_lock(tmp.path()).expect("first");
        assert!(matches!(
            take_run_lock(tmp.path()),
            Err(ProfileError::GameRunning)
        ));
    }

    #[test]
    fn try_lock_steps_aside_while_held() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let _held = take_run_lock(tmp.path()).expect("first");
        assert!(try_run_lock(tmp.path()).expect("try").is_none());
    }

    #[test]
    fn lock_is_free_again_after_the_holder_drops() {
        let tmp = tempfile::tempdir().expect("tempdir");
        drop(take_run_lock(tmp.path()).expect("first"));
        assert!(take_run_lock(tmp.path()).is_ok());
    }
}
