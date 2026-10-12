//! Is a profile's game running? (V17 detection, used by V27 and later the T10 lock.)
//!
//! A profile is running when any process carries that profile's exact
//! `-savedatafolder=<SaveData>` argument. This also catches the in-game Restart
//! child, which re-uses the parent's arguments (GenCommandLine.cs:41-71).

use std::path::Path;

pub trait RunningProbe {
    fn is_running(&self, save_data_dir: &Path) -> bool;
    /// Any RimWorld at all, including Default (which has no `-savedatafolder`).
    fn any_game(&self) -> bool;
}

/// Scans live processes' argument lists.
pub struct ProcessProbe;

impl RunningProbe for ProcessProbe {
    fn is_running(&self, save_data_dir: &Path) -> bool {
        use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

        let needle = crate::profile::savedatafolder_arg(save_data_dir);
        let mut sys = System::new();
        sys.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing().with_cmd(UpdateKind::Always),
        );
        sys.processes().values().any(|p| p.cmd().contains(&needle))
    }

    fn any_game(&self) -> bool {
        any_game_running()
    }
}

/// Executable names of the game on Linux and Windows.
pub const GAME_PROCESS_NAMES: [&str; 2] = ["RimWorldLinux", "RimWorldWin64.exe"];

/// Is any RimWorld process alive (whatever its profile, launched by us or not)?
/// V11: profiles share `temporaryCachePath`, so only one game may run at once.
pub fn any_game_running() -> bool {
    use sysinfo::{ProcessRefreshKind, ProcessStatus, ProcessesToUpdate, System};

    let mut sys = System::new();
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing());
    sys.processes().values().any(|p| {
        p.status() != ProcessStatus::Zombie && GAME_PROCESS_NAMES.iter().any(|n| p.name() == *n)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_probe_sees_child_with_matching_arg() {
        let dir = tempfile::tempdir().expect("tempdir");
        let save = dir.path().join("SaveData");
        let arg = crate::profile::savedatafolder_arg(&save);
        let mut child = std::process::Command::new(std::env::current_exe().expect("exe"))
            // libtest stops option parsing at `--`; the extra arg becomes an unmatched filter.
            .args([
                "--ignored",
                "--exact",
                "--",
                "profile::running::tests::never_runs_just_sleeps",
            ])
            .arg(&arg)
            .spawn()
            .expect("spawn");
        let seen = (0..50).any(|_| {
            std::thread::sleep(std::time::Duration::from_millis(20));
            ProcessProbe.is_running(&save)
        });
        let _ = child.kill();
        let _ = child.wait();
        assert!(seen, "probe did not see {arg:?}");
        assert!(!ProcessProbe.is_running(&dir.path().join("Other")));
    }

    /// Helper process for the probe test: idles until killed.
    #[test]
    #[ignore = "spawned by process_probe_sees_child_with_matching_arg"]
    fn never_runs_just_sleeps() {
        std::thread::sleep(std::time::Duration::from_secs(10));
    }
}
