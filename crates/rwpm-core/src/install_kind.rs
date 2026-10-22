//! How this copy of rwpm was installed, which decides whether it may replace itself.
//!
//! Nothing in the build records the install method, so it is read from where the program
//! runs. A copy owned by a package manager (AUR, deb, rpm) only tells the player about an
//! update; the other kinds can swap their own files.

use std::path::{Path, PathBuf};

use crate::update_check::Version;

/// Folders a package manager owns. The AUR, deb and rpm packages all install to `/opt/rwpm`.
const MANAGED_PREFIXES: [&str; 2] = ["/opt/rwpm", "/usr"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Linux,
    Windows,
    MacOs,
}

impl Os {
    pub fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::MacOs
        } else {
            Self::Linux
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallKind {
    /// Someone else owns the files (a package manager, Flatpak), or this folder is read-only.
    /// The window shows the release page and nothing more.
    Managed,
    /// The `.AppImage` file the player started.
    AppImage(PathBuf),
    /// The per-user Windows installer; `uninstall.exe` sits in this folder.
    WindowsInstaller(PathBuf),
    /// A plain folder from the tar.gz or zip.
    Portable(PathBuf),
    /// macOS: the app is ad-hoc signed, so the player downloads updates by hand.
    MacOs,
}

/// What the install looks like from outside. A struct so tests can build any case.
pub struct Probe<'a> {
    pub exe: &'a Path,
    /// `$APPIMAGE`, only when this process runs from that AppImage's mount.
    pub appimage: Option<&'a Path>,
    pub os: Os,
    pub in_flatpak: bool,
    pub exists: &'a dyn Fn(&Path) -> bool,
    pub writable: &'a dyn Fn(&Path) -> bool,
}

impl InstallKind {
    /// The kind for the running process.
    pub fn detect() -> Self {
        let Ok(exe) = std::env::current_exe() else {
            return Self::Managed;
        };
        let exe = std::fs::canonicalize(&exe).unwrap_or(exe);
        // `APPIMAGE` is inherited by every child, like `APPDIR`; only trust it inside the mount.
        let appimage = crate::appimage::current()
            .and_then(|_| std::env::var_os("APPIMAGE"))
            .map(PathBuf::from);
        Self::detect_from(&Probe {
            exe: &exe,
            appimage: appimage.as_deref(),
            os: Os::current(),
            in_flatpak: crate::flatpak::in_sandbox(),
            exists: &|p| p.exists(),
            writable: &dir_is_writable,
        })
    }

    pub fn detect_from(probe: &Probe<'_>) -> Self {
        if probe.os == Os::MacOs {
            return Self::MacOs;
        }
        if probe.in_flatpak {
            return Self::Managed;
        }
        if let Some(file) = probe.appimage.filter(|f| (probe.exists)(f)) {
            return match file.parent() {
                Some(dir) if (probe.writable)(dir) => Self::AppImage(file.to_path_buf()),
                _ => Self::Managed,
            };
        }
        let Some(dir) = probe.exe.parent() else {
            return Self::Managed;
        };
        if probe.os == Os::Windows && (probe.exists)(&dir.join("uninstall.exe")) {
            return Self::WindowsInstaller(dir.to_path_buf());
        }
        if probe.os == Os::Linux && MANAGED_PREFIXES.iter().any(|p| probe.exe.starts_with(p)) {
            return Self::Managed;
        }
        if (probe.writable)(dir) {
            Self::Portable(dir.to_path_buf())
        } else {
            Self::Managed
        }
    }

    /// True when the window may offer to install an update.
    pub fn can_self_update(&self) -> bool {
        matches!(
            self,
            Self::AppImage(_) | Self::WindowsInstaller(_) | Self::Portable(_)
        )
    }

    /// The release file this kind installs from, named as the release workflow names it.
    /// `None` for kinds that never self-update, and on CPUs the releases do not cover.
    pub fn asset_name(&self, version: &Version, os: Os) -> Option<String> {
        if std::env::consts::ARCH != "x86_64" {
            return None;
        }
        match (self, os) {
            (Self::AppImage(_), _) => Some(format!("rwpm-{version}-x86_64.AppImage")),
            (Self::WindowsInstaller(_), _) => {
                Some(format!("rwpm-{version}-windows-x86_64-setup.exe"))
            }
            (Self::Portable(_), Os::Windows) => Some(format!("rwpm-{version}-windows-x86_64.zip")),
            (Self::Portable(_), Os::Linux) => Some(format!("rwpm-{version}-linux-x86_64.tar.gz")),
            _ => None,
        }
    }
}

/// True when a file can be created in `dir`. Checks by trying, because permission bits do not
/// cover read-only mounts or ACLs.
pub fn dir_is_writable(dir: &Path) -> bool {
    let probe = dir.join(format!(".rwpm-write-test-{}", std::process::id()));
    let ok = std::fs::File::create(&probe).is_ok();
    // Best effort: a leftover empty file is harmless and the next run overwrites it.
    let _ = std::fs::remove_file(&probe);
    ok
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kind(
        exe: &str,
        appimage: Option<&str>,
        os: Os,
        writable: bool,
        uninstall: bool,
    ) -> InstallKind {
        let appimage = appimage.map(Path::new);
        InstallKind::detect_from(&Probe {
            exe: Path::new(exe),
            appimage,
            os,
            in_flatpak: false,
            exists: &|p| {
                p.file_name().is_some_and(|n| n == "uninstall.exe") && uninstall
                    || appimage.is_some_and(|a| a == p)
            },
            writable: &|_| writable,
        })
    }

    #[test]
    fn package_manager_install_only_notifies() {
        let k = kind("/opt/rwpm/rwpm", None, Os::Linux, true, false);
        assert_eq!(k, InstallKind::Managed);
        assert!(!k.can_self_update());
    }

    #[test]
    fn appimage_updates_the_file_the_player_started() {
        let k = kind(
            "/tmp/.mount_x/rwpm",
            Some("/home/a/rwpm.AppImage"),
            Os::Linux,
            true,
            false,
        );
        assert_eq!(k, InstallKind::AppImage("/home/a/rwpm.AppImage".into()));
        assert!(k.can_self_update());
    }

    #[test]
    fn appimage_in_a_read_only_folder_only_notifies() {
        let k = kind(
            "/tmp/.mount_x/rwpm",
            Some("/srv/rwpm.AppImage"),
            Os::Linux,
            false,
            false,
        );
        assert_eq!(k, InstallKind::Managed);
    }

    #[test]
    fn appimage_that_is_gone_falls_back_to_the_folder_rules() {
        let k = InstallKind::detect_from(&Probe {
            exe: Path::new("/home/a/rwpm/rwpm"),
            appimage: Some(Path::new("/home/a/moved.AppImage")),
            os: Os::Linux,
            in_flatpak: false,
            exists: &|_| false,
            writable: &|_| true,
        });
        assert_eq!(k, InstallKind::Portable("/home/a/rwpm".into()));
    }

    #[test]
    fn uninstaller_beside_the_exe_marks_the_installer() {
        let k = kind(
            "C:/Users/a/AppData/Local/Programs/rwpm/rwpm.exe",
            None,
            Os::Windows,
            true,
            true,
        );
        assert!(matches!(k, InstallKind::WindowsInstaller(_)));
    }

    #[test]
    fn plain_folder_is_portable_on_both_systems() {
        assert_eq!(
            kind("/home/a/rwpm/rwpm", None, Os::Linux, true, false),
            InstallKind::Portable("/home/a/rwpm".into())
        );
        assert!(matches!(
            kind("C:/tools/rwpm/rwpm.exe", None, Os::Windows, true, false),
            InstallKind::Portable(_)
        ));
    }

    #[test]
    fn read_only_portable_folder_only_notifies() {
        assert_eq!(
            kind("/home/a/rwpm/rwpm", None, Os::Linux, false, false),
            InstallKind::Managed
        );
    }

    #[test]
    fn flatpak_and_macos_never_self_update() {
        let flatpak = InstallKind::detect_from(&Probe {
            exe: Path::new("/app/bin/rwpm"),
            appimage: None,
            os: Os::Linux,
            in_flatpak: true,
            exists: &|_| false,
            writable: &|_| true,
        });
        assert_eq!(flatpak, InstallKind::Managed);
        let mac = kind(
            "/Applications/rwpm.app/Contents/MacOS/rwpm",
            None,
            Os::MacOs,
            true,
            false,
        );
        assert_eq!(mac, InstallKind::MacOs);
        assert!(!mac.can_self_update());
    }

    #[test]
    fn asset_names_match_the_release_workflow() {
        if std::env::consts::ARCH != "x86_64" {
            return;
        }
        let v: Version = "1.18.0".parse().unwrap();
        let name = |k: InstallKind, os| k.asset_name(&v, os);
        assert_eq!(
            name(InstallKind::AppImage("/x".into()), Os::Linux).as_deref(),
            Some("rwpm-1.18.0-x86_64.AppImage")
        );
        assert_eq!(
            name(InstallKind::WindowsInstaller("/x".into()), Os::Windows).as_deref(),
            Some("rwpm-1.18.0-windows-x86_64-setup.exe")
        );
        assert_eq!(
            name(InstallKind::Portable("/x".into()), Os::Windows).as_deref(),
            Some("rwpm-1.18.0-windows-x86_64.zip")
        );
        assert_eq!(
            name(InstallKind::Portable("/x".into()), Os::Linux).as_deref(),
            Some("rwpm-1.18.0-linux-x86_64.tar.gz")
        );
        assert_eq!(name(InstallKind::Managed, Os::Linux), None);
    }

    #[test]
    fn write_probe_reports_a_missing_folder_as_not_writable() {
        let dir = tempfile::tempdir().unwrap();
        assert!(dir_is_writable(dir.path()));
        assert!(!dir_is_writable(&dir.path().join("nope")));
    }
}
