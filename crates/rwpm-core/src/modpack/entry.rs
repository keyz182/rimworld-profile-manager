//! Rules for archive entry names. A pack is a short allowlist: `rimpack.toml`, an optional
//! `cover.png`, then plain files below `SaveData/`. Restore never joins an unchecked name onto a path, and
//! the same rules decide what a pack may hold when it is made.

use std::path::{Component, Path, PathBuf};

use super::cover::COVER_ENTRY;
use super::file::{FILE_NAME, Includes};
use crate::profile::copy_settings::{Group, group_of};

pub const SAVE_DATA: &str = "SaveData";
pub const MOD_LIST: &str = "Config/ModsConfig.xml";
const SAVES: &str = "Saves";
/// Settings and creations are plain data. A file of any other kind (a library, a script, a
/// binary) is left out of a pack and refused from one.
const EXTENSIONS: [&str; 7] = ["xml", "rsc", "rid", "xtp", "json", "txt", "ini"];

/// Where an entry belongs, and its path below that place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Place {
    /// `rimpack.toml`.
    Manifest,
    /// `cover.png`, the cover picture.
    Cover,
    /// `SaveData/<rel>`; `rel` is never empty.
    SaveData(PathBuf),
}

/// Sorts an entry name into its place, or says why it is refused: only plain relative names
/// that are `rimpack.toml` or sit inside `SaveData/`.
pub fn classify(name: &Path) -> Result<Place, String> {
    let mut parts = Vec::new();
    for component in name.components() {
        let Component::Normal(part) = component else {
            return Err(format!("`{}` is not a plain relative path", name.display()));
        };
        let part = part
            .to_str()
            .ok_or_else(|| format!("`{}` is not valid UTF-8", name.display()))?;
        if part.contains(['\\', ':', '\0']) {
            return Err(format!(
                "`{}` has a character not allowed in a path",
                name.display()
            ));
        }
        parts.push(part);
    }
    match parts.as_slice() {
        [] => Err("an entry has an empty name".into()),
        [first] if *first == FILE_NAME => Ok(Place::Manifest),
        [first] if *first == COVER_ENTRY => Ok(Place::Cover),
        [first, rest @ ..] if *first == SAVE_DATA && !rest.is_empty() => {
            Ok(Place::SaveData(rest.iter().collect()))
        }
        _ => Err(format!(
            "`{}` is outside the places a modpack may write",
            name.display()
        )),
    }
}

/// Only regular files. A pack holds no folders (restore makes them), no links and no
/// special files: a link written early could redirect a later entry out of the profile.
pub fn check_entry_kind(kind: tar::EntryType, name: &Path) -> Result<(), String> {
    if kind == tar::EntryType::Regular {
        Ok(())
    } else {
        Err(format!(
            "`{}` is a folder, a link or a special file, which a modpack may not hold",
            name.display()
        ))
    }
}

/// Whether the pack's own manifest declares this file (`rel` is below `SaveData/`): the mod
/// list always, then settings and creations by the ticks, then exactly the listed saves.
pub fn check_declared(rel: &Path, includes: &Includes) -> Result<(), String> {
    let parts = crate::backup::unpack::parts_of(rel);
    let outside = || {
        format!(
            "`{SAVE_DATA}/{}` is not something this pack declares",
            rel.display()
        )
    };
    if parts.len() < 2 {
        return Err(outside());
    }
    if parts[0] == SAVES {
        let listed = parts.len() == 2 && includes.saves.iter().any(|s| s == parts[1]);
        return if listed { Ok(()) } else { Err(outside()) };
    }
    let ticked = match group_of(rel, false) {
        // The list is one file, never a folder of that name.
        Some(Group::ModList) => parts.len() == 2,
        Some(Group::ModSettings) => includes.mod_settings,
        Some(Group::GameSettings) => includes.game_settings,
        Some(Group::Creations) => includes.creations,
        Some(Group::ModData) | None => false,
    };
    if !ticked {
        return Err(outside());
    }
    let extension = rel.extension().and_then(|e| e.to_str()).unwrap_or("");
    if EXTENSIONS.iter().any(|e| e.eq_ignore_ascii_case(extension)) {
        Ok(())
    } else {
        Err(format!(
            "`{SAVE_DATA}/{}` is not a settings or creation file",
            rel.display()
        ))
    }
}
