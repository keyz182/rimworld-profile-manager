//! Flatpak Steam. Steam's sandbox sees the host home only below
//! `~/.var/app/com.valvesoftware.Steam`, at the same absolute paths. So with Flatpak Steam
//! this app keeps its data there, Steam launches a copy of rwpm kept there, and the
//! app's own Play goes through `flatpak run` so the game can reach Steam.
//!
//! `sync_launcher` is also reused outside Flatpak: by an AppImage mount and by a
//! macOS `.app` bundle — both need a stable on-disk copy of the binary for the
//! same underlying reason (a transient or translocated run location Steam/RimSort can't
//! keep naming).

use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const STEAM_APP: &str = "com.valvesoftware.Steam";

/// Files rwpm needs beside it: both payloads and Doorstop (`payload_files` in `rwpm run`).
/// `Rwpm.Payload.Hook.dll` is the RimWorld 1.7 startup hook.
/// macOS's loader is `libdoorstop.dylib`, not `.so`.
fn launcher_files() -> [&'static str; 4] {
    [
        "rwpm",
        "payload/Rwpm.Payload.dll",
        "payload/Rwpm.Payload.Hook.dll",
        if cfg!(target_os = "macos") {
            "doorstop/libdoorstop.dylib"
        } else {
            "doorstop/libdoorstop.so"
        },
    ]
}

/// `<home>/.var/app/com.valvesoftware.Steam`: Flatpak Steam's home inside the sandbox.
pub fn steam_home(home: &Path) -> PathBuf {
    home.join(".var").join("app").join(STEAM_APP)
}

/// Is this process inside a Flatpak sandbox (Flatpak Steam's, for a Steam launch)?
pub fn in_sandbox() -> bool {
    Path::new("/.flatpak-info").exists()
}

/// The Steam helper and Valve's library. The app looks for the helper beside its own
/// executable (`fetch::helper_beside`), so a copied launcher needs both beside it.
fn steam_files() -> [&'static str; 2] {
    [
        crate::fetch::HELPER,
        if cfg!(windows) {
            "steam_api64.dll"
        } else if cfg!(target_os = "macos") {
            "libsteam_api.dylib"
        } else {
            "libsteam_api.so"
        },
    ]
}

/// Copies rwpm, the payload and Doorstop from `from` (this install) to `to`, replacing
/// only files that differ, each via a temp file + rename (a running copy keeps its file).
/// The Steam helper and its library go too when `from` has them: a bundle without them
/// still syncs, and the app then offers no Steam route.
/// → `<to>/rwpm`, the program Steam's launch options name.
pub fn sync_launcher(from: &Path, to: &Path) -> io::Result<PathBuf> {
    for rel in launcher_files() {
        copy_if_changed(&from.join(rel), &to.join(rel))?;
    }
    for rel in steam_files() {
        match copy_if_changed(&from.join(rel), &to.join(rel)) {
            Err(e) if e.kind() == io::ErrorKind::NotFound && !from.join(rel).exists() => {}
            other => other?,
        }
    }
    Ok(to.join("rwpm"))
}

fn copy_if_changed(src: &Path, dst: &Path) -> io::Result<()> {
    let new = std::fs::read(src)?;
    if std::fs::read(dst).is_ok_and(|old| old == new) {
        return Ok(());
    }
    if let Some(dir) = dst.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = dst.with_extension("rwpm-tmp");
    std::fs::write(&tmp, &new)?;
    std::fs::set_permissions(&tmp, std::fs::metadata(src)?.permissions())?;
    std::fs::rename(&tmp, dst)
}

/// `flatpak run --command=<rwpm> [--env=K=V…] com.valvesoftware.Steam run <id> -- <game>`.
pub fn play_command(
    flatpak: &Path,
    rwpm: &Path,
    id: &str,
    game_exe: &Path,
    env: &[(&str, &str)],
) -> Command {
    let mut cmd = Command::new(flatpak);
    cmd.arg("run");
    cmd.arg(format!("--command={}", rwpm.display()));
    for (k, v) in env {
        cmd.arg(format!("--env={k}={v}"));
    }
    cmd.args([STEAM_APP, "run", id, "--"]).arg(game_exe);
    cmd
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bundle(dir: &Path, tag: &str) {
        for rel in launcher_files() {
            let p = dir.join(rel);
            std::fs::create_dir_all(p.parent().expect("parent")).expect("dir");
            std::fs::write(&p, format!("{rel}:{tag}")).expect("file");
        }
    }

    #[test]
    fn sync_copies_then_replaces_changed_files_only() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let (app, copy) = (tmp.path().join("app"), tmp.path().join("copy"));
        bundle(&app, "v1");
        assert_eq!(sync_launcher(&app, &copy).expect("sync"), copy.join("rwpm"));
        for rel in launcher_files() {
            assert_eq!(
                std::fs::read_to_string(copy.join(rel)).expect("copied"),
                format!("{rel}:v1")
            );
        }
        let payload = copy.join("payload/Rwpm.Payload.dll");
        let before = std::fs::metadata(&payload)
            .expect("meta")
            .modified()
            .expect("mtime");
        std::fs::write(app.join("rwpm"), "rwpm:v2").expect("update");
        std::thread::sleep(std::time::Duration::from_millis(20));
        sync_launcher(&app, &copy).expect("sync");
        assert_eq!(
            std::fs::read_to_string(copy.join("rwpm")).expect("rwpm"),
            "rwpm:v2"
        );
        let after = std::fs::metadata(&payload)
            .expect("meta")
            .modified()
            .expect("mtime");
        assert_eq!(before, after, "unchanged file rewritten");
    }

    #[test]
    fn sync_copies_the_steam_helper_so_the_copy_finds_it() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let (app, copy) = (tmp.path().join("app"), tmp.path().join("copy"));
        bundle(&app, "v1");
        for rel in steam_files() {
            std::fs::write(app.join(rel), format!("{rel}:v1")).expect("file");
        }
        let rwpm = sync_launcher(&app, &copy).expect("sync");
        for rel in steam_files() {
            assert_eq!(
                std::fs::read_to_string(copy.join(rel)).expect("copied"),
                format!("{rel}:v1")
            );
        }
        assert_eq!(
            crate::fetch::helper_beside(&rwpm),
            Some(copy.join(crate::fetch::HELPER))
        );
        std::fs::write(app.join(crate::fetch::HELPER), "newer").expect("update");
        sync_launcher(&app, &copy).expect("sync again");
        assert_eq!(
            std::fs::read_to_string(copy.join(crate::fetch::HELPER)).expect("helper"),
            "newer"
        );
    }

    #[test]
    fn sync_still_works_for_a_bundle_without_the_steam_helper() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let (app, copy) = (tmp.path().join("app"), tmp.path().join("copy"));
        bundle(&app, "v1");
        let rwpm = sync_launcher(&app, &copy).expect("sync");
        assert!(rwpm.is_file());
        assert_eq!(crate::fetch::helper_beside(&rwpm), None);
    }

    #[test]
    fn sync_copies_the_startup_hook() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let (app, copy) = (tmp.path().join("app"), tmp.path().join("copy"));
        bundle(&app, "v1");
        sync_launcher(&app, &copy).expect("sync");
        assert!(copy.join("payload/Rwpm.Payload.Hook.dll").is_file());
    }

    #[cfg(unix)]
    #[test]
    fn sync_keeps_rwpm_executable() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().expect("tempdir");
        let (app, copy) = (tmp.path().join("app"), tmp.path().join("copy"));
        bundle(&app, "v1");
        std::fs::set_permissions(app.join("rwpm"), std::fs::Permissions::from_mode(0o755))
            .expect("chmod");
        sync_launcher(&app, &copy).expect("sync");
        let mode = std::fs::metadata(copy.join("rwpm"))
            .expect("meta")
            .permissions()
            .mode();
        assert_eq!(mode & 0o111, 0o111);
    }

    #[test]
    fn sync_fails_when_bundle_incomplete() {
        let tmp = tempfile::tempdir().expect("tempdir");
        assert!(sync_launcher(tmp.path(), &tmp.path().join("copy")).is_err());
    }

    #[test]
    fn play_command_runs_rwpm_in_steam_sandbox() {
        let cmd = play_command(
            Path::new("flatpak"),
            Path::new("/h/.var/app/com.valvesoftware.Steam/data/x/bin/rwpm"),
            "run-1",
            Path::new("/h/.var/app/com.valvesoftware.Steam/g/RimWorldLinux"),
            &[("RWPM_GUARD_CHOICE", "launch")],
        );
        let args: Vec<_> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(cmd.get_program(), "flatpak");
        assert_eq!(
            args,
            [
                "run",
                "--command=/h/.var/app/com.valvesoftware.Steam/data/x/bin/rwpm",
                "--env=RWPM_GUARD_CHOICE=launch",
                STEAM_APP,
                "run",
                "run-1",
                "--",
                "/h/.var/app/com.valvesoftware.Steam/g/RimWorldLinux",
            ]
        );
    }
}
