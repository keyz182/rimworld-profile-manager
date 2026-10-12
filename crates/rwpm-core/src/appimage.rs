//! AppImage (Linux). An AppImage mounts its files at a new temporary path on each run and
//! unmounts them when it exits, so Steam's launch options and the in-game Restart cannot
//! point inside it. The GUI copies rwpm, the payload and Doorstop to a fixed folder (as for
//! Flatpak Steam, `flatpak::sync_launcher`) and uses that copy.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// The AppImage's mount folder when `exe` runs from inside it. `APPDIR` alone is not
/// enough: every process the AppImage starts (Steam, the game) inherits it.
pub fn mount_dir(exe: &Path, appdir: Option<&OsStr>) -> Option<PathBuf> {
    let dir = PathBuf::from(appdir?);
    exe.starts_with(&dir).then_some(dir)
}

/// [`mount_dir`] for this process. `current_exe` is a resolved path, so `APPDIR` is
/// resolved too (a temp folder behind a symlink); unresolvable → as given.
pub fn current() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let appdir = std::env::var_os("APPDIR")?;
    let resolved = std::fs::canonicalize(&appdir).map_or(appdir, PathBuf::into_os_string);
    mount_dir(&exe, Some(&resolved))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inside_the_mount() {
        let dir = OsStr::new("/tmp/.mount_rwpmAb12");
        assert_eq!(
            mount_dir(Path::new("/tmp/.mount_rwpmAb12/rwpm"), Some(dir)),
            Some(PathBuf::from("/tmp/.mount_rwpmAb12"))
        );
    }

    #[test]
    fn inherited_appdir_is_ignored() {
        let dir = OsStr::new("/tmp/.mount_rwpmAb12");
        assert_eq!(
            mount_dir(Path::new("/home/p/.local/share/x/bin/rwpm"), Some(dir)),
            None
        );
        // A path that only shares a prefix string is not inside the mount.
        assert_eq!(
            mount_dir(Path::new("/tmp/.mount_rwpmAb12x/rwpm"), Some(dir)),
            None
        );
    }

    #[test]
    fn not_an_appimage() {
        assert_eq!(mount_dir(Path::new("/opt/rwpm/rwpm"), None), None);
    }
}
