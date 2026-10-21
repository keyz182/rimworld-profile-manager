//! Copy game and mod settings from one profile to another, by group.
//! Never `Saves/` (Copy profile does that) and never into Default (the game's own
//! folder). Everything is staged first, so a failure leaves the target as it was; only
//! then are same-name entries sent to the recycle bin and the staged ones moved in.

use std::path::{Path, PathBuf};

use crate::error::ProfileError;
use crate::fs_copy::copy_tree;

use super::RunningProbe;
use super::store::{Profile, ProfileStore, Trash};

const CONFIG: &str = "Config";
const SAVES: &str = "Saves";
const MOD_LIST: &str = "ModsConfig.xml";
const GAME_SETTINGS: [&str; 2] = ["Prefs.xml", "KeyPrefs.xml"];
const CREATIONS: [&str; 4] = ["Scenarios", "Ideos", "Xenotypes", "ExportedPawns"];
/// Staging folder inside the target's save-data folder (same filesystem → quick renames).
const STAGING: &str = ".rwpm-copy-tmp";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Group {
    /// `Config/ModsConfig.xml`.
    ModList,
    /// `Config/Prefs.xml`, `Config/KeyPrefs.xml`.
    GameSettings,
    /// The rest of `Config/`: every mod's settings.
    ModSettings,
    /// Scenarios, ideoligions, xenotypes and exported pawns the player made.
    Creations,
    /// Other top-level folders mods keep (HugsLib, RimSaves, caches…).
    ModData,
}

impl Group {
    pub const ALL: [Group; 5] = [
        Group::ModList,
        Group::GameSettings,
        Group::ModSettings,
        Group::Creations,
        Group::ModData,
    ];

    /// Ticked unless the player changes it: all but mod data (mostly caches and logs).
    pub fn on_by_default(self) -> bool {
        self != Group::ModData
    }
}

/// Which group a path (relative to the save-data folder) belongs to; `None` = never copied.
pub(crate) fn group_of(rel: &Path, is_dir: bool) -> Option<Group> {
    let mut parts = rel.components().map(|c| c.as_os_str().to_string_lossy());
    let first = parts.next()?;
    let second = parts.next();
    if first == CONFIG {
        return Some(match second.as_deref() {
            Some(MOD_LIST) => Group::ModList,
            Some(f) if GAME_SETTINGS.contains(&f) => Group::GameSettings,
            _ => Group::ModSettings,
        });
    }
    if first == SAVES {
        return None;
    }
    if CREATIONS.contains(&first.as_ref()) {
        return Some(Group::Creations);
    }
    // Top-level files (logs, Steam Cloud's steam_autocloud.vdf) are never copied.
    is_dir.then_some(Group::ModData)
}

/// The entries (relative paths) in `src` that the chosen groups copy: files and folders
/// directly in `Config/`, and top-level folders.
pub fn entries(src: &Path, groups: &[Group]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let list = |dir: &Path| -> Vec<(std::ffi::OsString, bool)> {
        let mut v: Vec<_> = std::fs::read_dir(dir)
            .map(|rd| {
                rd.filter_map(Result::ok)
                    .map(|e| (e.file_name(), e.file_type().is_ok_and(|t| t.is_dir())))
                    .collect()
            })
            .unwrap_or_default();
        v.sort();
        v
    };
    for (name, is_dir) in list(src) {
        if name == STAGING {
            continue;
        }
        if name == CONFIG && is_dir {
            for (inner, inner_dir) in list(&src.join(CONFIG)) {
                let rel = Path::new(CONFIG).join(&inner);
                if group_of(&rel, inner_dir).is_some_and(|g| groups.contains(&g)) {
                    out.push(rel);
                }
            }
        } else if group_of(Path::new(&name), is_dir).is_some_and(|g| groups.contains(&g)) {
            out.push(PathBuf::from(name));
        }
    }
    out
}

/// Bytes each group would copy from `src`, for the dialog.
pub fn group_sizes(src: &Path) -> Vec<(Group, u64)> {
    Group::ALL
        .iter()
        .map(|&g| {
            let bytes = entries(src, &[g])
                .iter()
                .map(|rel| crate::space::dir_size(&src.join(rel)).max(file_size(&src.join(rel))))
                .sum();
            (g, bytes)
        })
        .collect()
}

fn file_size(path: &Path) -> u64 {
    std::fs::metadata(path).map_or(0, |m| if m.is_file() { m.len() } else { 0 })
}

impl<T: Trash> ProfileStore<T> {
    /// Copies the chosen groups from `from` into `to`. → how many entries were copied.
    pub fn copy_settings(
        &self,
        from: &Profile,
        to: &Profile,
        groups: &[Group],
        probe: &dyn RunningProbe,
        progress: &mut dyn FnMut(u64),
    ) -> Result<usize, ProfileError> {
        if to.is_default() {
            return Err(ProfileError::DefaultImmutable("changed"));
        }
        if probe.any_game() {
            return Err(ProfileError::GameRunning);
        }
        let (src, dst) = (from.save_data_dir(), to.save_data_dir());
        if src == dst {
            return Ok(0);
        }
        let staging = dst.join(STAGING);
        if staging.exists() {
            self.trash_path(&staging)?; // left by a crash
        }
        let chosen = entries(&src, groups);
        if let Err(e) = stage(&src, &staging, &chosen, progress) {
            let _ = self.trash_path(&staging);
            return Err(e);
        }
        for rel in &chosen {
            let target = dst.join(rel);
            if target.symlink_metadata().is_ok() {
                self.trash_path(&target)?;
            }
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(ProfileError::io("create", parent))?;
            }
            std::fs::rename(staging.join(rel), &target)
                .map_err(ProfileError::io("move into place", &target))?;
        }
        remove_empty_dirs(&staging);
        Ok(chosen.len())
    }
}

/// Copies every chosen entry under `staging`, keeping relative paths.
fn stage(
    src: &Path,
    staging: &Path,
    chosen: &[PathBuf],
    progress: &mut dyn FnMut(u64),
) -> Result<(), ProfileError> {
    std::fs::create_dir_all(staging.join(CONFIG)).map_err(ProfileError::io("create", staging))?;
    let mut done = 0;
    for rel in chosen {
        let (from, to) = (src.join(rel), staging.join(rel));
        if from.is_dir() {
            let base = done;
            copy_tree(&from, &to, &|_| false, &mut |n| {
                done = base + n;
                progress(done);
            })
            .map_err(ProfileError::io("copy", &from))?;
        } else {
            reflink_copy::reflink_or_copy(&from, &to).map_err(ProfileError::io("copy", &from))?;
            done += file_size(&to);
            progress(done);
        }
    }
    Ok(())
}

/// Removes the staging folder once every entry has been moved out of it.
fn remove_empty_dirs(dir: &Path) {
    let _ = std::fs::remove_dir(dir.join(CONFIG));
    let _ = std::fs::remove_dir(dir);
}

#[cfg(test)]
mod tests;
