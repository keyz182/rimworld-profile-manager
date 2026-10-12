//! A profile's turned-on mods in load order, each with where it loads from and anything
//! wrong with it (T29, read-only). OFFLINE profiles load from their own frozen copies,
//! so anything else is flagged the way the launch guard (V5) would see it.

use crate::discovery::GameInstall;
use crate::mods::{ModIndex, Origin, read_active_mods};

use super::{Mode, Profile};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// Core and the DLCs, from the game's `Data` folder.
    Game,
    /// The profile's own frozen copy (OFFLINE).
    Frozen,
    /// The Steam Workshop.
    Steam,
    /// The game's `Mods` folder.
    Installed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flag {
    /// Turned on, but not installed anywhere the game looks.
    Missing,
    /// OFFLINE, but this mod would load from Steam and can change (V5).
    NotFrozen,
    /// Installed more than once in the same place; the game picks one.
    Twice,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModRow {
    pub package_id: String,
    /// The name players know; the package id when the mod is not installed.
    pub name: String,
    pub source: Option<Source>,
    pub flag: Option<Flag>,
}

/// The mods turned on in `profile`, in load order. No mod list yet → empty.
pub fn mod_list(profile: &Profile, game: &GameInstall) -> Result<Vec<ModRow>, String> {
    let config = profile
        .save_data_dir()
        .join("Config")
        .join("ModsConfig.xml");
    if !config.is_file() {
        return Ok(Vec::new());
    }
    let active = read_active_mods(&config).map_err(|e| e.to_string())?;
    let offline = profile.mode == Mode::Offline && !profile.is_default();
    let local = match (offline, profile.mods_dir()) {
        (true, Some(dir)) => dir,
        _ => game.mods_dir(),
    };
    let workshop = game.steam.as_ref().map(|s| s.workshop_dir.as_path());
    let index = ModIndex::scan(&game.data_dir(), &local, workshop);
    let twice: Vec<String> = index
        .resolve(&active)
        .ambiguous
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    Ok(active
        .into_iter()
        .map(|id| row(&index, id, offline, &twice))
        .collect())
}

fn row(index: &ModIndex, package_id: String, offline: bool, twice: &[String]) -> ModRow {
    let Some(found) = index.find(&package_id) else {
        return ModRow {
            name: package_id.clone(),
            package_id,
            source: None,
            flag: Some(Flag::Missing),
        };
    };
    let source = match (found.origin, offline) {
        (Origin::Official, _) => Source::Game,
        (Origin::Local, true) => Source::Frozen,
        (Origin::Local, false) => Source::Installed,
        (Origin::Workshop, _) => Source::Steam,
    };
    let flag = if twice.contains(&package_id) {
        Some(Flag::Twice)
    } else if offline && source == Source::Steam {
        Some(Flag::NotFrozen)
    } else {
        None
    };
    ModRow {
        name: found.name.clone(),
        package_id,
        source: Some(source),
        flag,
    }
}

/// Does `row` match a search (name or package id, any case)?
pub fn matches(row: &ModRow, needle: &str) -> bool {
    let needle = needle.trim().to_lowercase();
    needle.is_empty()
        || row.name.to_lowercase().contains(&needle)
        || row.package_id.contains(&needle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::SteamInfo;
    use crate::profile::{Location, ProfileId};

    struct World {
        tmp: tempfile::TempDir,
    }

    impl World {
        fn new() -> Self {
            Self {
                tmp: tempfile::tempdir().expect("tempdir"),
            }
        }

        fn path(&self, rel: &str) -> std::path::PathBuf {
            self.tmp.path().join(rel)
        }

        fn add_mod(&self, rel: &str, id: &str, name: &str) {
            let dir = self.path(rel).join("About");
            std::fs::create_dir_all(&dir).expect("about");
            std::fs::write(
                dir.join("About.xml"),
                format!(
                    "<ModMetaData><name>{name}</name><packageId>{id}</packageId></ModMetaData>"
                ),
            )
            .expect("about");
        }

        fn game(&self) -> GameInstall {
            GameInstall {
                game_dir: self.path("game"),
                exe: self.path("game/RimWorldLinux"),
                version: None,
                steam: Some(SteamInfo {
                    library_dir: self.path("library"),
                    workshop_dir: self.path("workshop"),
                }),
            }
        }

        fn profile(&self, mode: Mode, active: &[&str]) -> Profile {
            let dir = self.path("profile");
            let config = dir.join("SaveData/Config/ModsConfig.xml");
            std::fs::create_dir_all(config.parent().expect("p")).expect("dir");
            let items: String = active.iter().map(|a| format!("<li>{a}</li>")).collect();
            std::fs::write(
                config,
                format!("<ModsConfigData><activeMods>{items}</activeMods></ModsConfigData>"),
            )
            .expect("config");
            Profile {
                id: ProfileId::parse("p").expect("id"),
                name: "P".into(),
                mode,
                location: Location::Stored { dir },
            }
        }
    }

    #[test]
    fn offline_rows_show_frozen_steam_and_missing_in_load_order() {
        let w = World::new();
        w.add_mod("game/Data/Core", "ludeon.rimworld", "Core");
        w.add_mod("profile/Mods/Harmony", "brrainz.harmony", "Harmony");
        w.add_mod("workshop/111", "me.newer", "Newer Mod");
        let p = w.profile(
            Mode::Offline,
            &["brrainz.harmony", "ludeon.rimworld", "me.newer", "me.gone"],
        );
        let rows = mod_list(&p, &w.game()).expect("list");
        let got: Vec<(&str, Option<Source>, Option<Flag>)> = rows
            .iter()
            .map(|r| (r.name.as_str(), r.source, r.flag))
            .collect();
        assert_eq!(
            got,
            [
                ("Harmony", Some(Source::Frozen), None),
                ("Core", Some(Source::Game), None),
                ("Newer Mod", Some(Source::Steam), Some(Flag::NotFrozen)),
                ("me.gone", None, Some(Flag::Missing)),
            ]
        );
    }

    #[test]
    fn live_rows_load_from_install_and_steam_and_flag_twice() {
        let w = World::new();
        w.add_mod("game/Mods/Local", "me.local", "Local Mod");
        w.add_mod("workshop/1", "me.dup", "Dup A");
        w.add_mod("workshop/2", "me.dup", "Dup B");
        let p = w.profile(Mode::Live, &["me.local", "me.dup"]);
        let rows = mod_list(&p, &w.game()).expect("list");
        assert_eq!(rows[0].source, Some(Source::Installed));
        assert_eq!(
            (rows[1].source, rows[1].flag),
            (Some(Source::Steam), Some(Flag::Twice))
        );
        assert!(
            matches(&rows[0], "LOCAL") && matches(&rows[0], "me.loc") && !matches(&rows[0], "dup")
        );
    }

    #[test]
    fn no_mod_list_yet_is_empty() {
        let w = World::new();
        let p = Profile {
            id: ProfileId::parse("p").expect("id"),
            name: "P".into(),
            mode: Mode::Live,
            location: Location::Stored {
                dir: w.path("nothing"),
            },
        };
        assert_eq!(mod_list(&p, &w.game()), Ok(Vec::new()));
    }
}
