//! Finds the RimWorld install, its Steam library and the workshop content dir.
//! Read-only: never writes inside any Steam folder (V6).

use std::path::{Path, PathBuf};

pub const RIMWORLD_APP_ID: u32 = 294100;

#[cfg(windows)]
pub const GAME_EXE: &str = "RimWorldWin64.exe";
#[cfg(not(windows))]
pub const GAME_EXE: &str = "RimWorldLinux";

const VERSION_FILE: &str = "Version.txt";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameInstall {
    pub game_dir: PathBuf,
    pub exe: PathBuf,
    /// `Version.txt`, trimmed (e.g. `1.6.4871 rev598`); ModsConfig needs it verbatim.
    pub version: Option<String>,
    /// `None` for a non-Steam install (e.g. GOG): no workshop.
    pub steam: Option<SteamInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SteamInfo {
    pub library_dir: PathBuf,
    /// `<library>/steamapps/workshop/content/294100`; may not exist yet.
    pub workshop_dir: PathBuf,
}

#[derive(Debug, thiserror::Error)]
pub enum DiscoveryError {
    #[error("Steam was not found: {0}")]
    SteamNotFound(String),
    #[error("RimWorld is not installed in any Steam library")]
    GameNotInstalled,
    #[error("`{}` is not a RimWorld folder (missing `{}`)", dir.display(), exe.display())]
    NotAGameDir { dir: PathBuf, exe: PathBuf },
    #[error("could not read Steam's library data: {0}")]
    Steam(String),
}

/// Finds RimWorld via Steam. With no override, tries every Steam install found
/// (native, Flatpak, Snap) and takes the first that has the game.
pub fn discover(steam_override: Option<&Path>) -> Result<GameInstall, DiscoveryError> {
    let steam_dirs = match steam_override {
        Some(path) => vec![
            steamlocate::SteamDir::from_dir(path)
                .map_err(|e| DiscoveryError::SteamNotFound(e.to_string()))?,
        ],
        None => {
            steamlocate::locate_all().map_err(|e| DiscoveryError::SteamNotFound(e.to_string()))?
        }
    };
    if steam_dirs.is_empty() {
        return Err(DiscoveryError::SteamNotFound(
            "no Steam install found".into(),
        ));
    }
    for steam in steam_dirs {
        let found = steam
            .find_app(RIMWORLD_APP_ID)
            .map_err(|e| DiscoveryError::Steam(e.to_string()))?;
        if let Some((app, library)) = found {
            let mut install = read_game_dir(&library.resolve_app_dir(&app))?;
            install.steam = Some(steam_info(library.path()));
            return Ok(install);
        }
    }
    Err(DiscoveryError::GameNotInstalled)
}

/// Which Steam installs (native or Flatpak) have RimWorld (V31). Errors count as "no".
pub fn steams_with_game() -> crate::paths::GameIn {
    let mut found = crate::paths::GameIn::default();
    let flatpak_home = dirs::home_dir().map(|h| crate::flatpak::steam_home(&h));
    for steam in steamlocate::locate_all().unwrap_or_default() {
        if !steam.find_app(RIMWORLD_APP_ID).is_ok_and(|f| f.is_some()) {
            continue;
        }
        if flatpak_home
            .as_ref()
            .is_some_and(|f| steam.path().starts_with(f))
        {
            found.flatpak = true;
        } else {
            found.native = true;
        }
    }
    found
}

/// Uses a game folder chosen in Settings. Attaches Steam info when the folder sits
/// in `<library>/steamapps/common/`.
pub fn from_game_dir(dir: &Path) -> Result<GameInstall, DiscoveryError> {
    let mut install = read_game_dir(dir)?;
    install.steam = library_of(dir).map(steam_info);
    Ok(install)
}

fn read_game_dir(dir: &Path) -> Result<GameInstall, DiscoveryError> {
    let exe = dir.join(GAME_EXE);
    if !exe.is_file() {
        return Err(DiscoveryError::NotAGameDir {
            dir: dir.to_path_buf(),
            exe,
        });
    }
    let version = std::fs::read_to_string(dir.join(VERSION_FILE))
        .ok()
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty());
    Ok(GameInstall {
        game_dir: dir.to_path_buf(),
        exe,
        version,
        steam: None,
    })
}

/// `<library>/steamapps/common/<game>` → `<library>`.
fn library_of(game_dir: &Path) -> Option<&Path> {
    let common = game_dir.parent()?;
    let steamapps = common.parent()?;
    let is = |p: &Path, name: &str| p.file_name().is_some_and(|n| n.eq_ignore_ascii_case(name));
    (is(common, "common") && is(steamapps, "steamapps"))
        .then(|| steamapps.parent())
        .flatten()
}

fn steam_info(library: &Path) -> SteamInfo {
    SteamInfo {
        library_dir: library.to_path_buf(),
        workshop_dir: library
            .join("steamapps")
            .join("workshop")
            .join("content")
            .join(RIMWORLD_APP_ID.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const INSTALL_DIR: &str = "RimWorld";

    fn vdf_path(p: &Path) -> String {
        p.display().to_string().replace('\\', "\\\\")
    }

    /// Builds `<root>/steam` with the given extra libraries. `game_lib` indexes
    /// `[steam, extra…]`; that library gets RimWorld installed.
    fn steam_fixture(
        root: &Path,
        extra_libs: usize,
        game_lib: Option<usize>,
    ) -> (PathBuf, Vec<PathBuf>) {
        let steam = root.join("steam");
        let mut libs = vec![steam.clone()];
        libs.extend((0..extra_libs).map(|i| root.join(format!("lib{i}"))));
        let mut vdf = String::from("\"libraryfolders\"\n{\n");
        for (i, lib) in libs.iter().enumerate() {
            std::fs::create_dir_all(lib.join("steamapps")).expect("steamapps");
            vdf += &format!(
                "\t\"{i}\"\n\t{{\n\t\t\"path\"\t\t\"{}\"\n\t\t\"apps\"\n\t\t{{\n\t\t}}\n\t}}\n",
                vdf_path(lib)
            );
        }
        vdf += "}\n";
        std::fs::write(steam.join("steamapps/libraryfolders.vdf"), vdf).expect("vdf");
        if let Some(i) = game_lib {
            let lib = &libs[i];
            std::fs::write(
                lib.join(format!("steamapps/appmanifest_{RIMWORLD_APP_ID}.acf")),
                format!("\"AppState\"\n{{\n\t\"appid\"\t\t\"{RIMWORLD_APP_ID}\"\n\t\"name\"\t\t\"RimWorld\"\n\t\"installdir\"\t\t\"{INSTALL_DIR}\"\n}}\n"),
            )
            .expect("manifest");
            let game = lib.join("steamapps/common").join(INSTALL_DIR);
            std::fs::create_dir_all(&game).expect("game dir");
            std::fs::write(game.join(GAME_EXE), b"").expect("exe");
            std::fs::write(game.join(VERSION_FILE), "1.6.4871 rev598\r\n").expect("version");
        }
        (steam, libs)
    }

    #[test]
    fn discover_finds_game_in_secondary_library() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let (steam, libs) = steam_fixture(tmp.path(), 2, Some(2));
        let gi = discover(Some(&steam)).expect("discover");
        let game = libs[2].join("steamapps/common/RimWorld");
        assert_eq!(gi.game_dir, game);
        assert_eq!(gi.exe, game.join(GAME_EXE));
        let s = gi.steam.expect("steam info");
        assert_eq!(s.library_dir, libs[2]);
        assert_eq!(
            s.workshop_dir,
            libs[2].join("steamapps/workshop/content/294100")
        );
    }

    #[test]
    fn discover_with_override_uses_given_steam_dir() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let (steam, libs) = steam_fixture(tmp.path(), 0, Some(0));
        let gi = discover(Some(&steam)).expect("discover");
        assert_eq!(gi.steam.expect("steam").library_dir, libs[0]);
    }

    #[test]
    fn discover_reports_game_not_installed() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let (steam, _) = steam_fixture(tmp.path(), 1, None);
        assert!(matches!(
            discover(Some(&steam)),
            Err(DiscoveryError::GameNotInstalled)
        ));
    }

    #[test]
    fn reads_version_txt_trimmed() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let (steam, _) = steam_fixture(tmp.path(), 0, Some(0));
        assert_eq!(
            discover(Some(&steam)).expect("discover").version.as_deref(),
            Some("1.6.4871 rev598")
        );
    }

    #[test]
    fn from_game_dir_attaches_steam_library_when_inside_steamapps() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let (_, libs) = steam_fixture(tmp.path(), 0, Some(0));
        let gi = from_game_dir(&libs[0].join("steamapps/common/RimWorld")).expect("manual");
        assert_eq!(gi.steam.expect("steam").library_dir, libs[0]);
    }

    #[test]
    fn from_game_dir_without_steam_has_no_workshop() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let game = tmp.path().join("GOG Games/RimWorld");
        std::fs::create_dir_all(&game).expect("dir");
        std::fs::write(game.join(GAME_EXE), b"").expect("exe");
        let gi = from_game_dir(&game).expect("manual");
        assert_eq!(gi.steam, None);
        assert_eq!(gi.version, None);
    }

    #[test]
    fn from_game_dir_rejects_dir_without_exe() {
        let tmp = tempfile::tempdir().expect("tempdir");
        assert!(matches!(
            from_game_dir(tmp.path()),
            Err(DiscoveryError::NotAGameDir { .. })
        ));
    }

    /// V6: discovery must succeed on a read-only Steam tree and leave it untouched.
    #[cfg(unix)]
    #[test]
    fn discovery_works_on_read_only_steam_tree() {
        use std::os::unix::fs::PermissionsExt;

        fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
            for e in std::fs::read_dir(dir).expect("read_dir") {
                let p = e.expect("entry").path();
                if p.is_dir() {
                    walk(&p, out);
                }
                out.push(p);
            }
        }
        fn snapshot(root: &Path) -> Vec<(PathBuf, u64, std::time::SystemTime)> {
            let mut paths = Vec::new();
            walk(root, &mut paths);
            paths.sort();
            paths
                .into_iter()
                .map(|p| {
                    let m = std::fs::metadata(&p).expect("meta");
                    (p, m.len(), m.modified().expect("mtime"))
                })
                .collect()
        }
        fn set_mode(root: &Path, dir_mode: u32, file_mode: u32) {
            let mut paths = vec![root.to_path_buf()];
            walk(root, &mut paths);
            for p in paths {
                let mode = if p.is_dir() { dir_mode } else { file_mode };
                std::fs::set_permissions(&p, std::fs::Permissions::from_mode(mode)).expect("chmod");
            }
        }

        let tmp = tempfile::tempdir().expect("tempdir");
        let (steam, _) = steam_fixture(tmp.path(), 1, Some(1));
        let before = snapshot(tmp.path());
        set_mode(tmp.path(), 0o555, 0o444);
        let result = discover(Some(&steam));
        let after = snapshot(tmp.path());
        set_mode(tmp.path(), 0o755, 0o644);
        result.expect("discover on read-only tree");
        assert_eq!(before, after, "discovery changed the Steam tree");
    }

    /// Manual: `cargo test -p rwpm-core live_discovers_rimworld -- --ignored`
    #[test]
    #[ignore = "needs a real Steam + RimWorld install"]
    fn live_discovers_rimworld() {
        let gi = discover(None).expect("discover");
        assert!(gi.exe.is_file(), "{gi:?}");
        assert!(gi.version.is_some(), "{gi:?}");
        eprintln!("{gi:#?}");
        let s = gi.steam.expect("steam");
        assert!(s.workshop_dir.is_dir(), "{s:?}");
    }
}
