//! A Steam library on one game version and a stored build of another, for the tests of
//! every effective-game consumer.

use std::path::{Path, PathBuf};

use crate::builds::{BuildFile, BuildKey, file};
use crate::discovery::{self, GAME_EXE, GameInstall, is_bundle_path};
use crate::profile::store::{Profile, ProfileStore, Trash};

pub const STEAM_VERSION: &str = "1.6.4871 rev598";
pub const BUILD_VERSION: &str = "1.5.4409 rev1118";

/// Writes a game folder: the platform's Unity exe, `Version.txt` and `Data/Core`.
pub fn write_game(dir: &Path, version: &str) -> GameInstall {
    let exe = dir.join(GAME_EXE);
    if is_bundle_path(&exe) {
        std::fs::create_dir_all(exe.join("Contents/MacOS")).expect("bundle");
        std::fs::write(exe.join("Contents/Info.plist"), "<plist/>").expect("plist");
        std::fs::write(exe.join("Contents/MacOS/RimWorldMac"), b"").expect("bin");
    } else {
        std::fs::create_dir_all(dir).expect("game dir");
        std::fs::write(&exe, b"").expect("exe");
    }
    std::fs::write(dir.join("Version.txt"), format!("{version}\r\n")).expect("version");
    let install = discovery::from_game_dir(dir).expect("game dir");
    add_mod(&install.data_dir().join("Core"), "ludeon.rimworld", "Core");
    install
}

/// `<dir>/About/About.xml` with the id and name, plus one payload file.
pub fn add_mod(dir: &Path, id: &str, name: &str) {
    std::fs::create_dir_all(dir.join("About")).expect("about");
    std::fs::write(
        dir.join("About/About.xml"),
        format!("<ModMetaData><name>{name}</name><packageId>{id}</packageId></ModMetaData>"),
    )
    .expect("about");
    std::fs::write(dir.join("payload.txt"), id).expect("payload");
}

/// A Steam library at `<root>/steam` holding the game on `STEAM_VERSION`, a workshop
/// item `steam.only` (111) and a local mod `live.local` in the install's `Mods`; and a
/// stored build of `BUILD_VERSION` under `<profiles_root>/builds`, captured from that
/// install, with its own local mod `build.local`.
pub struct Games {
    pub live: GameInstall,
    pub build_key: BuildKey,
    pub build_dir: PathBuf,
}

impl Games {
    pub fn new(root: &Path, profiles_root: &Path) -> Self {
        let live = write_game(&root.join("steam/steamapps/common/RimWorld"), STEAM_VERSION);
        let workshop = &live.steam.as_ref().expect("steam library").workshop_dir;
        add_mod(&workshop.join("111"), "steam.only", "Steam Only");
        add_mod(
            &live.mods_dir().join("LiveLocal"),
            "live.local",
            "Live Local",
        );
        let build_key = BuildKey::from_version(BUILD_VERSION).expect("key");
        let build_dir = profiles_root.join("builds").join(build_key.as_str());
        let build = write_game(&build_dir, BUILD_VERSION);
        add_mod(
            &build.mods_dir().join("BuildLocal"),
            "build.local",
            "Build Local",
        );
        BuildFile {
            schema: file::SCHEMA,
            version: BUILD_VERSION.into(),
            buildid: "1".into(),
            beta: String::new(),
            dlc: Vec::new(),
            captured_at: "2026-10-13T00:00:00Z".parse().expect("datetime"),
            source_dir: live.game_dir.clone(),
            size: 0,
        }
        .write_atomic(&build_dir.join(file::FILE_NAME))
        .expect("build.toml");
        Self {
            live,
            build_key,
            build_dir,
        }
    }

    /// Pins `profile` to the stored build.
    pub fn pin<T: Trash>(&self, store: &ProfileStore<T>, profile: &Profile) -> Profile {
        store
            .set_pin(&profile.id, Some(self.build_key.as_str()))
            .expect("pin")
    }

    /// Removes the stored build, as a player deleting the folder by hand would.
    pub fn lose_build(&self) {
        std::fs::remove_dir_all(&self.build_dir).expect("remove build");
    }
}
