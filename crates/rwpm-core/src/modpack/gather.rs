//! The `[[mods]]` list of a pack, gathered from a real profile (T80b, C.58b).
//!
//! Active ids come from the profile's `ModsConfig.xml` in load order and are resolved the
//! way the game resolves them. Everything read from About.xml is checked against the
//! manifest rules, and an entry that fails is left out and named in `skipped`, so one odd
//! mod cannot make the whole pack invalid.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use super::file::{ModEntry, check_link, check_package_id};
use crate::builds::{EffectiveGame, acf};
use crate::error::ProfileError;
use crate::mods::{
    InstalledMod, ModIndex, Origin, about_field, manifest_version, read_active_mods,
};
use crate::profile::file::FILE_NAME as PROFILE_FILE;
use crate::profile::{ModSource, Mode, Profile, ProfileFile};

/// Core is always on, so it is not a choice a pack records.
const CORE: &str = "ludeon.rimworld";
const WORKSHOP_PAGE: &str = "https://steamcommunity.com/sharedfiles/filedetails/?id=";
const WORKSHOP_ACF: &str = "appworkshop_294100.acf";
const MAX_NAME: usize = 200;
const MAX_VERSION: usize = 128;

#[derive(Debug, Default)]
pub struct Gathered {
    /// Turned-on mods in load order. A mod that is turned on but not installed is listed by
    /// id only, so the person who opens the pack can see what was missing.
    pub mods: Vec<ModEntry>,
    /// Package ids of the expansions that are turned on.
    pub dlc: Vec<String>,
    /// Entries left out, with the reason in plain words.
    pub skipped: Vec<String>,
}

pub fn gather_mods(profile: &Profile, game: &EffectiveGame) -> Result<Gathered, ProfileError> {
    let config = profile
        .save_data_dir()
        .join("Config")
        .join("ModsConfig.xml");
    if !config.is_file() {
        return Err(ProfileError::NoModList(profile.name.clone()));
    }
    let active = read_active_mods(&config).map_err(|e| ProfileError::Parse {
        path: config.clone(),
        message: e.to_string(),
    })?;
    let offline = profile.mode == Mode::Offline && !profile.is_default();
    let local = match (offline, profile.mods_dir()) {
        (true, Some(dir)) => dir,
        _ => game.mods_dir(),
    };
    let workshop = game.steam.as_ref().map(|s| s.workshop_dir.as_path());
    let index = ModIndex::scan(&game.data_dir(), &local, workshop);
    let times = workshop
        .and_then(workshop_acf)
        .map(|path| acf::read_workshop_times(&path))
        .unwrap_or_default();
    let frozen = if offline {
        frozen_sources(profile)
    } else {
        HashMap::new()
    };
    let mut out = Gathered::default();
    for id in active {
        match index.find(&id) {
            Some(found) if found.origin == Origin::Official => {
                if id != CORE {
                    out.add_dlc(id);
                }
            }
            Some(found) => out.add_mod(entry_for(&id, found, offline, &frozen, &times)),
            None => out.add_mod(ModEntry {
                name: id.clone(),
                package_id: id,
                workshop_id: None,
                url: None,
                time_updated: None,
                version: None,
            }),
        }
    }
    Ok(out)
}

impl Gathered {
    fn add_mod(&mut self, entry: ModEntry) {
        match check_package_id(&entry.package_id) {
            Ok(()) => self.mods.push(entry),
            Err(why) => self.skipped.push(why),
        }
    }

    fn add_dlc(&mut self, id: String) {
        match check_package_id(&id) {
            Ok(()) => self.dlc.push(id),
            Err(why) => self.skipped.push(why),
        }
    }
}

fn entry_for(
    id: &str,
    found: &InstalledMod,
    offline: bool,
    frozen: &HashMap<String, ModSource>,
    times: &BTreeMap<u64, u64>,
) -> ModEntry {
    // A frozen copy keeps the Workshop id as its folder name (C.18).
    let from_workshop = match found.origin {
        Origin::Workshop => true,
        Origin::Local => offline && frozen.get(&found.folder) == Some(&ModSource::Workshop),
        Origin::Official => false,
    };
    let workshop_id = from_workshop
        .then(|| found.folder.parse::<u64>().ok())
        .flatten()
        .filter(|n| *n > 0);
    let url = match workshop_id {
        Some(n) => Some(format!("{WORKSHOP_PAGE}{n}")),
        None => about_field(&found.dir, "url").filter(|u| check_link(u).is_ok()),
    };
    ModEntry {
        package_id: id.to_owned(),
        name: clean_name(&found.name, id),
        workshop_id,
        url,
        // For a frozen copy this is the Steam copy's time: a hint, not a record.
        time_updated: workshop_id.and_then(|n| times.get(&n).copied()),
        version: manifest_version(&found.dir)
            .filter(|v| v.len() <= MAX_VERSION && !v.chars().any(char::is_control)),
    }
}

/// The name players know, without control characters and cut to a sensible length.
fn clean_name(name: &str, id: &str) -> String {
    let clean: String = name
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_NAME)
        .collect();
    let clean = clean.trim();
    if clean.is_empty() {
        id.to_owned()
    } else {
        clean.to_owned()
    }
}

/// `<library>/steamapps/workshop/appworkshop_294100.acf`, two folders above the content dir.
fn workshop_acf(workshop_dir: &Path) -> Option<PathBuf> {
    Some(workshop_dir.parent()?.parent()?.join(WORKSHOP_ACF))
}

/// Source of each frozen copy in an OFFLINE profile, by folder name. An unreadable
/// `profile.toml` reads as empty: the pack then lists those copies as local mods.
fn frozen_sources(profile: &Profile) -> HashMap<String, ModSource> {
    let Some(dir) = profile.stored_dir() else {
        return HashMap::new();
    };
    ProfileFile::read(&dir.join(PROFILE_FILE))
        .map(|file| {
            file.frozen_mods
                .into_iter()
                .map(|m| (m.folder, m.source))
                .collect()
        })
        .unwrap_or_default()
}
