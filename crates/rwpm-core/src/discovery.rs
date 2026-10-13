//! Finds the RimWorld install, its Steam library and the workshop content dir.
//! Read-only: never writes inside any Steam folder (V6).

use std::path::{Path, PathBuf};

pub const RIMWORLD_APP_ID: u32 = 294100;

#[cfg(windows)]
pub const GAME_EXE: &str = "RimWorldWin64.exe";
/// R29: the bundle DIRECTORY, not the inner Mach-O — matches R33 (Steam's own
/// `%command%` names the bundle too); `launch::resolve_macos_bundle` resolves it to the
/// actual binary at launch time, not here.
#[cfg(target_os = "macos")]
pub const GAME_EXE: &str = "RimWorldMac.app";
#[cfg(not(any(windows, target_os = "macos")))]
pub const GAME_EXE: &str = "RimWorldLinux";

const VERSION_FILE: &str = "Version.txt";
/// R37: RimWorld 1.7+ runs on Waylith (CoreCLR); its engine assembly sits beside the exe.
const WAYLITH_MARKER: &str = "Waylith.dll";
/// The Waylith exe: Linux `RimWorld` (no `RimWorldLinux`, no start script, R37), Windows
/// `RimWorld.exe` (no `RimWorldWin64.exe`, R44), macOS the bundle DIRECTORY `RimWorld.app`
/// — the same R29 convention `GAME_EXE` uses, resolved to `Contents/MacOS/RimWorld` by
/// `launch::resolve_macos_bundle` at launch time.
#[cfg(windows)]
pub const WAYLITH_EXE: &str = "RimWorld.exe";
#[cfg(target_os = "macos")]
pub const WAYLITH_EXE: &str = "RimWorld.app";
#[cfg(not(any(windows, target_os = "macos")))]
pub const WAYLITH_EXE: &str = "RimWorld";

const BUNDLE_EXT: &str = "app";
const INFO_PLIST: &str = "Contents/Info.plist";
const BUNDLE_BIN_DIR: &str = "Contents/MacOS";

/// The game engine of an install: Unity/Mono (≤ 1.6) or Waylith/CoreCLR (1.7+, R37).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    Unity,
    Waylith,
}

/// V69: `Waylith.dll` beside the game marks a Waylith install, on any OS. `dir` is the
/// folder holding the exe — or, on macOS, the `.app` bundle itself, where the engine
/// assemblies sit in `Contents/MacOS/` (live-confirmed on the 1.7 macOS depot).
pub fn engine_of(dir: &Path) -> Engine {
    let beside = if is_bundle_path(dir) {
        dir.join(BUNDLE_BIN_DIR)
    } else {
        dir.to_path_buf()
    };
    if beside.join(WAYLITH_MARKER).is_file() {
        Engine::Waylith
    } else {
        Engine::Unity
    }
}

/// The engine of the install this exe belongs to. A macOS bundle carries its own
/// assemblies; a flat exe has them as siblings.
pub fn engine_of_exe(exe: &Path) -> Engine {
    if is_bundle_path(exe) {
        engine_of(exe)
    } else {
        exe.parent().map_or(Engine::Unity, engine_of)
    }
}

/// Does this path name a macOS `.app` bundle? Decided by the path's own shape, not by the
/// host running this code, so a Linux-shaped fixture behaves like Linux everywhere.
pub fn is_bundle_path(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()) == Some(BUNDLE_EXT)
}

/// Is `path` the game's exe, as this platform shapes one?
///
/// A macOS bundle counts only when it actually holds its executable. After a 1.6 to 1.7
/// switch Steam removes only the files its depots own, so the old `RimWorldMac.app` is
/// left behind as a husk: `Mods/` and `bak/` survive, `Contents/MacOS/` is empty and the
/// binary is gone. The husk is still a directory, so a bare `is_dir` test accepts it and
/// a launch then resolves a target inside a bundle with nothing to run. Requiring
/// `Info.plist` as well means any bundle discovery accepts is one `resolve_macos_bundle`
/// can read later.
fn is_game_exe(path: &Path) -> bool {
    if !is_bundle_path(path) {
        return path.is_file();
    }
    path.join(INFO_PLIST).is_file() && holds_a_file(&path.join(BUNDLE_BIN_DIR))
}

fn holds_a_file(dir: &Path) -> bool {
    std::fs::read_dir(dir).is_ok_and(|mut entries| {
        entries.any(|e| e.is_ok_and(|e| e.file_type().is_ok_and(|t| t.is_file())))
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameInstall {
    pub game_dir: PathBuf,
    pub exe: PathBuf,
    /// `Version.txt`, trimmed (e.g. `1.6.4871 rev598`); ModsConfig needs it verbatim.
    pub version: Option<String>,
    /// `None` for a non-Steam install (e.g. GOG): no workshop.
    pub steam: Option<SteamInfo>,
    /// Resolved once, when the install is read, so the path accessors below stay pure
    /// path arithmetic instead of each going back to the disk.
    pub engine: Engine,
}

impl GameInstall {
    pub fn engine(&self) -> Engine {
        self.engine
    }

    /// Is `exe` a macOS `.app` bundle (R29), rather than a flat Linux/Windows binary?
    fn is_macos_bundle(&self) -> bool {
        is_bundle_path(&self.exe)
    }

    /// A macOS bundle that holds the game's own folders inside it — the 1.6 layout.
    ///
    /// The layout follows the VERSION, not the platform: 1.6 keeps `Version.txt`, `Data/`
    /// and `Mods/` inside `RimWorldMac.app`, while the 1.7 macOS depot (Waylith) lays the
    /// install out flat, like Linux and Windows, with only the exe in `RimWorld.app`.
    fn is_unity_bundle(&self) -> bool {
        self.is_macos_bundle() && self.engine == Engine::Unity
    }

    /// The game's own read-only content dir (R1 mod discovery, Managed assemblies).
    /// Linux/Windows: `<game_dir>/Data`, a flat sibling of the exe. macOS: `exe` IS the
    /// bundle (`<game_dir>/RimWorldMac.app`, R29) and `Data` lives inside it, at
    /// `Contents/Resources/Data` (GenFilePaths' `UnityData.dataPath`, live-confirmed).
    pub fn data_dir(&self) -> PathBuf {
        if self.is_unity_bundle() {
            self.exe.join("Contents").join("Resources").join("Data")
        } else {
            self.game_dir.join("Data")
        }
    }

    /// The game's own install Mods dir (LIVE source; V12/V20 install-Mods checks).
    /// Linux/Windows: `<game_dir>/Mods`. macOS: `<bundle>/Mods` — GenFilePaths walks up
    /// from `UnityData.dataPath` to the bundle's parent, i.e. the bundle itself (R29).
    pub fn mods_dir(&self) -> PathBuf {
        if self.is_unity_bundle() {
            self.exe.join("Mods")
        } else {
            self.game_dir.join("Mods")
        }
    }

    /// `Version.txt` for this install: the install root when it is there, otherwise
    /// inside a macOS bundle.
    ///
    /// The file moves with the VERSION, not the platform. macOS 1.6 keeps it inside
    /// `RimWorldMac.app`; the 1.7 macOS depot puts it at the install root, where every
    /// other platform already has it. Probing the root first keeps this right for both
    /// without depending on engine detection, and picks the live file over one a husk
    /// bundle left behind.
    pub fn version_file(&self) -> PathBuf {
        let root = self.game_dir.join(VERSION_FILE);
        if root.is_file() || !self.is_macos_bundle() {
            root
        } else {
            self.exe.join(VERSION_FILE)
        }
    }

    /// RimSort's `game_folder` field (T45). Linux/Windows: `game_dir`, the flat install
    /// root. macOS: the `.app` bundle itself — confirmed live against this machine's real
    /// `~/Library/Application Support/RimSort/settings.json`, which names the bundle
    /// path, not its parent (R29 addendum).
    pub fn rimsort_game_folder(&self) -> &Path {
        if self.is_unity_bundle() {
            &self.exe
        } else {
            &self.game_dir
        }
    }
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

/// The game's exe in `dir`, by name. Precedence is fixed: the Unity exe first, the
/// Waylith exe second (V69 / B12 / B13). A folder can hold both — Steam leaves the old
/// bundle behind as a husk when it switches version — so "the first one that is actually
/// usable" decides, never the order the filesystem happens to answer in.
///
/// The names are parameters rather than the platform constants so the rules can be
/// checked against every platform's shapes from any host.
fn pick_exe(dir: &Path, unity: &str, waylith: &str) -> Option<PathBuf> {
    let primary = dir.join(unity);
    if is_game_exe(&primary) {
        return Some(primary);
    }
    let waylith = dir.join(waylith);
    (is_game_exe(&waylith) && engine_of_exe(&waylith) == Engine::Waylith).then_some(waylith)
}

fn read_game_dir(dir: &Path) -> Result<GameInstall, DiscoveryError> {
    let exe = pick_exe(dir, GAME_EXE, WAYLITH_EXE).ok_or_else(|| DiscoveryError::NotAGameDir {
        dir: dir.to_path_buf(),
        exe: dir.join(GAME_EXE),
    })?;
    let mut install = GameInstall {
        game_dir: dir.to_path_buf(),
        engine: engine_of_exe(&exe),
        exe,
        version: None,
        steam: None,
    };
    // `Version.txt` is CRLF-terminated on every platform; `trim` handles that.
    install.version = std::fs::read_to_string(install.version_file())
        .ok()
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty());
    Ok(install)
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

    fn flat_install() -> GameInstall {
        GameInstall {
            game_dir: PathBuf::from("/games/RimWorld"),
            exe: PathBuf::from("/games/RimWorld/RimWorldLinux"),
            version: None,
            engine: Engine::Unity,
            steam: None,
        }
    }

    fn macos_bundle_install() -> GameInstall {
        GameInstall {
            game_dir: PathBuf::from("/games/RimWorld"),
            exe: PathBuf::from("/games/RimWorld/RimWorldMac.app"),
            version: None,
            engine: Engine::Unity,
            steam: None,
        }
    }

    /// Fixture shape decides branch, not the host running the tests (same principle as
    /// `resolve_macos_bundle` in launch.rs — a Linux fixture stays Linux everywhere).
    #[test]
    fn data_dir_and_mods_dir_flat_vs_bundle() {
        let flat = flat_install();
        assert_eq!(flat.data_dir(), PathBuf::from("/games/RimWorld/Data"));
        assert_eq!(flat.mods_dir(), PathBuf::from("/games/RimWorld/Mods"));

        let bundle = macos_bundle_install();
        assert_eq!(
            bundle.data_dir(),
            PathBuf::from("/games/RimWorld/RimWorldMac.app/Contents/Resources/Data")
        );
        assert_eq!(
            bundle.mods_dir(),
            PathBuf::from("/games/RimWorld/RimWorldMac.app/Mods")
        );
    }

    /// T45, live-confirmed against this machine's real RimSort settings.json: macOS
    /// `game_folder` names the bundle, not its parent (R29 addendum).
    #[test]
    fn rimsort_game_folder_flat_vs_bundle() {
        assert_eq!(
            flat_install().rimsort_game_folder(),
            Path::new("/games/RimWorld")
        );
        assert_eq!(
            macos_bundle_install().rimsort_game_folder(),
            Path::new("/games/RimWorld/RimWorldMac.app")
        );
    }

    fn vdf_path(p: &Path) -> String {
        p.display().to_string().replace('\\', "\\\\")
    }

    /// Writes `GAME_EXE` under `dir` as whatever `read_game_dir` expects to find for this
    /// host OS — a plain file on Linux/Windows, a bundle DIRECTORY on macOS (R29).
    fn write_game_exe(dir: &Path) {
        let exe = dir.join(GAME_EXE);
        if cfg!(target_os = "macos") {
            let bin = exe.join(BUNDLE_BIN_DIR);
            std::fs::create_dir_all(&bin).expect("bundle dir");
            std::fs::write(exe.join(INFO_PLIST), plist("RimWorld")).expect("plist");
            std::fs::write(bin.join("RimWorld"), b"").expect("binary");
        } else {
            std::fs::write(&exe, b"").expect("exe");
        }
    }

    fn plist(exe_name: &str) -> String {
        format!(
            "<?xml version=\"1.0\"?>\n<plist version=\"1.0\"><dict>\n<key>CFBundleExecutable</key>\n<string>{exe_name}</string>\n</dict></plist>\n"
        )
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
            write_game_exe(&game);
            // T52: on macOS `Version.txt` lives INSIDE the bundle; there is none at the
            // install root. CRLF on every platform.
            let version_dir = if cfg!(target_os = "macos") {
                game.join(GAME_EXE)
            } else {
                game.clone()
            };
            std::fs::write(version_dir.join(VERSION_FILE), "1.6.4871 rev598\r\n").expect("version");
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

    /// R29 + T52: the bundle holds `Version.txt`; a flat install holds it at the root.
    #[test]
    fn version_file_sits_inside_a_macos_bundle() {
        let flat = GameInstall {
            game_dir: PathBuf::from("/games/RimWorld"),
            exe: PathBuf::from("/games/RimWorld/RimWorldLinux"),
            version: None,
            engine: Engine::Unity,
            steam: None,
        };
        assert_eq!(
            flat.version_file(),
            PathBuf::from("/games/RimWorld/Version.txt")
        );
        let bundle = GameInstall {
            exe: PathBuf::from("/games/RimWorld/RimWorldMac.app"),
            ..flat
        };
        assert_eq!(
            bundle.version_file(),
            PathBuf::from("/games/RimWorld/RimWorldMac.app/Version.txt")
        );
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
        write_game_exe(&game);
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

    #[test]
    fn v69_unity_dir_is_unity() {
        let tmp = tempfile::tempdir().expect("tempdir");
        write_game_exe(tmp.path());
        let gi = from_game_dir(tmp.path()).expect("game dir");
        assert_eq!(gi.engine(), Engine::Unity);
        assert_eq!(gi.exe, tmp.path().join(GAME_EXE));
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn v69_discovery_accepts_waylith_dir() {
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::write(tmp.path().join(WAYLITH_EXE), b"").expect("exe");
        std::fs::write(tmp.path().join(WAYLITH_MARKER), b"").expect("marker");
        std::fs::write(tmp.path().join(VERSION_FILE), "1.7.4976 rev1043\n").expect("version");
        let gi = from_game_dir(tmp.path()).expect("1.7 game dir");
        assert_eq!(gi.engine(), Engine::Waylith);
        assert_eq!(gi.exe, tmp.path().join(WAYLITH_EXE));
        assert_eq!(gi.version.as_deref(), Some("1.7.4976 rev1043"));
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn v69_rimworld_file_without_waylith_is_not_a_game_dir() {
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::write(tmp.path().join(WAYLITH_EXE), b"").expect("exe");
        assert!(from_game_dir(tmp.path()).is_err());
    }

    /// B13: the Windows 1.7 exe is `RimWorld.exe`; checked on every host through the helpers.
    #[test]
    fn v69_windows_waylith_exe_name() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let exe = tmp.path().join("RimWorld.exe");
        std::fs::write(&exe, b"").expect("exe");
        assert!(is_game_exe(&exe));
        assert_eq!(engine_of_exe(&exe), Engine::Unity, "no Waylith.dll");
        std::fs::write(tmp.path().join(WAYLITH_MARKER), b"").expect("marker");
        assert_eq!(engine_of_exe(&exe), Engine::Waylith);
        if cfg!(windows) {
            assert_eq!(WAYLITH_EXE, "RimWorld.exe");
        }
    }

    /// The 1.7 macOS depot (Waylith): flat install with only the exe in `RimWorld.app`.
    #[test]
    fn engine_of_reads_waylith_inside_a_bundle() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let bundle = tmp.path().join("RimWorld.app");
        let bin = bundle.join(BUNDLE_BIN_DIR);
        std::fs::create_dir_all(&bin).expect("bundle");
        std::fs::write(bundle.join(INFO_PLIST), plist("RimWorld")).expect("plist");
        std::fs::write(bin.join("RimWorld"), b"").expect("binary");

        assert_eq!(engine_of(&bundle), Engine::Unity, "no marker yet");
        assert_eq!(engine_of_exe(&bundle), Engine::Unity);
        std::fs::write(bin.join(WAYLITH_MARKER), b"").expect("marker");
        assert_eq!(engine_of(&bundle), Engine::Waylith);
        assert_eq!(engine_of_exe(&bundle), Engine::Waylith);
        // The marker is NOT at the install root on macOS, so the flat rule must not see it.
        assert_eq!(engine_of(tmp.path()), Engine::Unity);
    }

    /// Builds a macOS install under `root`: `bundle` holds the executable, `waylith` puts
    /// the engine assemblies inside it, `root_version` puts `Version.txt` at the install
    /// root rather than in the bundle. Shapes are written by hand, not via `cfg!`, so
    /// these run on every host.
    fn macos_install(root: &Path, bundle: &str, waylith: bool, root_version: bool) {
        let bundle = root.join(bundle);
        let bin = bundle.join(BUNDLE_BIN_DIR);
        std::fs::create_dir_all(&bin).expect("bundle");
        std::fs::write(bundle.join(INFO_PLIST), plist("RimWorld")).expect("plist");
        std::fs::write(bin.join("RimWorld"), b"").expect("binary");
        if waylith {
            std::fs::write(bin.join(WAYLITH_MARKER), b"").expect("marker");
        }
        let version = if root_version { root } else { &bundle };
        std::fs::write(version.join(VERSION_FILE), "1.7.4976 rev1043\r\n").expect("version");
        let data = if root_version {
            root.join("Data")
        } else {
            bundle.join("Contents/Resources/Data")
        };
        std::fs::create_dir_all(data.join("Core")).expect("data");
    }

    /// The husk Steam leaves behind after a version switch: the folders its depots do not
    /// own survive, `Contents/MacOS/` is emptied and the binary and plist are gone.
    fn macos_husk(root: &Path, bundle: &str) {
        let bundle = root.join(bundle);
        std::fs::create_dir_all(bundle.join(BUNDLE_BIN_DIR)).expect("husk");
        std::fs::create_dir_all(bundle.join("Mods")).expect("mods");
        std::fs::create_dir_all(bundle.join("bak")).expect("bak");
    }

    const MAC_UNITY_EXE: &str = "RimWorldMac.app";
    const MAC_WAYLITH_EXE: &str = "RimWorld.app";

    /// A `GameInstall` for a macOS layout, built without going through `read_game_dir`,
    /// whose exe names are chosen at compile time. The accessors under test decide on the
    /// path's own shape, so these run on every host.
    fn mac_install(root: &Path, bundle: &str, engine: Engine) -> GameInstall {
        GameInstall {
            game_dir: root.to_path_buf(),
            exe: root.join(bundle),
            version: None,
            engine,
            steam: None,
        }
    }

    /// The 1.7 macOS depot: Waylith, `RimWorld.app`, flat install around it.
    #[test]
    fn macos_1_7_install_is_found_as_waylith() {
        let tmp = tempfile::tempdir().expect("tempdir");
        macos_install(tmp.path(), MAC_WAYLITH_EXE, true, true);

        let exe = pick_exe(tmp.path(), MAC_UNITY_EXE, MAC_WAYLITH_EXE).expect("1.7 macOS exe");
        assert_eq!(exe, tmp.path().join(MAC_WAYLITH_EXE));
        assert_eq!(engine_of_exe(&exe), Engine::Waylith);

        let gi = mac_install(tmp.path(), MAC_WAYLITH_EXE, Engine::Waylith);
        // Layout follows the version: flat, like Linux and Windows.
        assert_eq!(gi.version_file(), tmp.path().join(VERSION_FILE));
        assert_eq!(gi.data_dir(), tmp.path().join("Data"));
        assert_eq!(gi.mods_dir(), tmp.path().join("Mods"));
        assert_eq!(gi.rimsort_game_folder(), tmp.path());
    }

    /// Regression: 1.6 macOS still resolves everything inside `RimWorldMac.app`.
    #[test]
    fn macos_1_6_install_still_resolves_inside_the_bundle() {
        let tmp = tempfile::tempdir().expect("tempdir");
        macos_install(tmp.path(), MAC_UNITY_EXE, false, false);
        let bundle = tmp.path().join(MAC_UNITY_EXE);

        let exe = pick_exe(tmp.path(), MAC_UNITY_EXE, MAC_WAYLITH_EXE).expect("1.6 macOS exe");
        assert_eq!(exe, bundle);
        assert_eq!(engine_of_exe(&exe), Engine::Unity);

        let gi = mac_install(tmp.path(), MAC_UNITY_EXE, Engine::Unity);
        assert_eq!(gi.version_file(), bundle.join(VERSION_FILE));
        assert_eq!(gi.data_dir(), bundle.join("Contents/Resources/Data"));
        assert_eq!(gi.mods_dir(), bundle.join("Mods"));
        assert_eq!(gi.rimsort_game_folder(), bundle);
    }

    /// A bundle with no executable is not the game, however much else survives in it.
    #[test]
    fn a_husk_bundle_is_not_a_game() {
        let tmp = tempfile::tempdir().expect("tempdir");
        macos_husk(tmp.path(), MAC_UNITY_EXE);
        std::fs::write(tmp.path().join(VERSION_FILE), "1.7.4976 rev1043\r\n").expect("version");
        assert!(!is_game_exe(&tmp.path().join(MAC_UNITY_EXE)));
        assert_eq!(pick_exe(tmp.path(), MAC_UNITY_EXE, MAC_WAYLITH_EXE), None);
    }

    /// 1.6 to 1.7: the old bundle is left behind, the new one is the game.
    #[test]
    fn a_husk_beside_a_real_bundle_does_not_win() {
        let tmp = tempfile::tempdir().expect("tempdir");
        macos_husk(tmp.path(), MAC_UNITY_EXE);
        macos_install(tmp.path(), MAC_WAYLITH_EXE, true, true);

        let exe = pick_exe(tmp.path(), MAC_UNITY_EXE, MAC_WAYLITH_EXE).expect("exe");
        assert_eq!(exe, tmp.path().join(MAC_WAYLITH_EXE));
        assert_eq!(engine_of_exe(&exe), Engine::Waylith);
    }

    /// 1.7 back to 1.6: the 1.6 bundle is restored and wins, husk or no husk.
    #[test]
    fn a_restored_1_6_bundle_beats_a_1_7_husk() {
        let tmp = tempfile::tempdir().expect("tempdir");
        macos_install(tmp.path(), MAC_UNITY_EXE, false, false);
        macos_husk(tmp.path(), MAC_WAYLITH_EXE);

        let exe = pick_exe(tmp.path(), MAC_UNITY_EXE, MAC_WAYLITH_EXE).expect("exe");
        assert_eq!(exe, tmp.path().join(MAC_UNITY_EXE));
        assert_eq!(engine_of_exe(&exe), Engine::Unity);
    }

    /// Both present: the install root wins, so a stale bundle copy never shadows the
    /// live file.
    #[test]
    fn version_file_prefers_the_install_root() {
        let tmp = tempfile::tempdir().expect("tempdir");
        macos_install(tmp.path(), MAC_UNITY_EXE, false, false);
        std::fs::write(tmp.path().join(VERSION_FILE), "1.6.4871 rev598\r\n").expect("version");

        let gi = mac_install(tmp.path(), MAC_UNITY_EXE, Engine::Unity);
        assert_eq!(gi.version_file(), tmp.path().join(VERSION_FILE));
    }

    /// Both the Unity and the Waylith shapes of every other platform, from any host.
    #[test]
    fn pick_exe_honours_precedence_for_flat_layouts() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let (unity, waylith) = ("RimWorldWin64.exe", "RimWorld.exe");
        assert_eq!(pick_exe(tmp.path(), unity, waylith), None, "empty dir");

        std::fs::write(tmp.path().join(waylith), b"").expect("exe");
        assert_eq!(
            pick_exe(tmp.path(), unity, waylith),
            None,
            "a Waylith exe with no marker is not the game"
        );
        std::fs::write(tmp.path().join(WAYLITH_MARKER), b"").expect("marker");
        assert_eq!(
            pick_exe(tmp.path(), unity, waylith),
            Some(tmp.path().join(waylith))
        );

        // Both present: the Unity exe wins.
        std::fs::write(tmp.path().join(unity), b"").expect("exe");
        assert_eq!(
            pick_exe(tmp.path(), unity, waylith),
            Some(tmp.path().join(unity))
        );
    }
}
