//! OFFLINE pre-launch guard (V5): every active mod must be a frozen copy in the profile.
//!
//! In OFFLINE the game lists `<install>/Data`, the profile's `Mods` (redirected) and the
//! subscribed workshop items; `<install>/Mods` is not seen. A workshop item turned on
//! in-game therefore loads live from the workshop, which the guard reports before launch.

use std::path::Path;

use crate::builds::EffectiveGame;
use crate::error::ProfileError;
use crate::mods::{
    ModIndex, ModsError, Origin, STEAM_POSTFIX, read_active_mods, rewrite_active_mods,
};
use crate::profile::file::{FILE_NAME, FrozenMod, ModSource, Mode, ProfileFile};
use crate::profile::id::ProfileId;
use crate::profile::offline::{Hooks, PlannedCopy, copy_mod, now_datetime, planned};
use crate::profile::running::RunningProbe;
use crate::profile::store::{Location, Profile, ProfileStore, Trash};

const MODS: &str = "Mods";
const MODS_CONFIG: &str = "SaveData/Config/ModsConfig.xml";

/// How "Add to profile" fixes one unfrozen active mod.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fix {
    /// Copy this source into the profile.
    Copy(PlannedCopy),
    /// The active id is `x_steam` but `x` is already frozen: point the entry at `x`.
    UseFrozen(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unfrozen {
    /// The id as listed in `activeMods`.
    pub active_id: String,
    /// The mod's name, as players know it.
    pub name: String,
    pub fix: Fix,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GuardReport {
    pub unfrozen: Vec<Unfrozen>,
    /// Active ids installed nowhere; the game turns these off itself (shown as info).
    pub missing: Vec<String>,
}

impl GuardReport {
    /// Nothing would load from outside the profile.
    pub fn is_clear(&self) -> bool {
        self.unfrozen.is_empty()
    }
}

/// Checks an OFFLINE profile's active mods. LIVE, Default and never-launched profiles
/// are always clear.
pub fn check_offline<T: Trash>(
    store: &ProfileStore<T>,
    id: &ProfileId,
    game: &EffectiveGame,
) -> Result<GuardReport, ProfileError> {
    let profile = store.get(id)?;
    let Location::Stored { dir } = &profile.location else {
        return Ok(GuardReport::default());
    };
    let config = dir.join(MODS_CONFIG);
    if profile.mode != Mode::Offline || !config.is_file() {
        return Ok(GuardReport::default());
    }
    let active = read_active_mods(&config).map_err(mods_error)?;
    let workshop = game.steam.as_ref().map(|s| s.workshop_dir.as_path());
    let view = ModIndex::scan(&game.data_dir(), &dir.join(MODS), workshop);
    // `<install>/Mods` only, to offer a copy of mods the OFFLINE game cannot see.
    let install = ModIndex::scan(Path::new(""), &game.mods_dir(), None);
    let mut report = GuardReport::default();
    for active_id in active {
        let base = active_id
            .strip_suffix(STEAM_POSTFIX)
            .unwrap_or(&active_id)
            .to_owned();
        let name = view
            .find(&active_id)
            .or_else(|| install.find(&active_id))
            .map_or_else(|| active_id.clone(), |m| m.name.clone());
        let fix = match view.find(&active_id) {
            Some(m) if m.origin != Origin::Workshop => continue,
            Some(m) => match view.find(&base) {
                Some(frozen) if frozen.origin == Origin::Local && base != active_id => {
                    Fix::UseFrozen(frozen.package_id.clone())
                }
                _ => Fix::Copy(planned(&base, m.dir.clone(), ModSource::Workshop)),
            },
            None => match install.find(&active_id) {
                Some(m) => Fix::Copy(planned(&base, m.dir.clone(), ModSource::Local)),
                None => {
                    report.missing.push(active_id);
                    continue;
                }
            },
        };
        report.unfrozen.push(Unfrozen {
            active_id,
            name,
            fix,
        });
    }
    Ok(report)
}

/// "Add to profile": copies each unfrozen mod in (with the torn-copy guard) and records
/// it, or re-points `x_steam` at the frozen `x`. Refused while any RimWorld runs.
pub fn add_to_profile<T: Trash>(
    store: &ProfileStore<T>,
    id: &ProfileId,
    unfrozen: &[Unfrozen],
    probe: &dyn RunningProbe,
    progress: &mut dyn FnMut(u64),
) -> Result<Profile, ProfileError> {
    if probe.any_game() {
        return Err(ProfileError::GameRunning);
    }
    let dir = stored_dir(&store.get(id)?)?;
    let mods = dir.join(MODS);
    std::fs::create_dir_all(&mods).map_err(ProfileError::io("create", &mods))?;
    let toml = dir.join(FILE_NAME);
    let mut renames = Vec::new();
    for item in unfrozen {
        match &item.fix {
            Fix::UseFrozen(frozen) => renames.push((item.active_id.clone(), Some(frozen.clone()))),
            Fix::Copy(copy) => {
                copy_in(&mods, copy, progress)?;
                // Record each copy as it lands, so a later failure loses nothing.
                let mut file = ProfileFile::read(&toml)?;
                file.frozen_mods.push(FrozenMod {
                    folder: copy.folder.clone(),
                    package_id: copy.package_id.clone(),
                    source: copy.source,
                    source_dir: copy.source_dir.clone(),
                    copied_at: now_datetime(),
                });
                file.write_atomic(&toml)?;
            }
        }
    }
    if !renames.is_empty() {
        rewrite_active_mods(&dir.join(MODS_CONFIG), &renames).map_err(mods_error)?;
    }
    store.get(id)
}

/// Copies to a hidden temp folder, then renames it into place (V12: same folder name).
fn copy_in(
    mods: &Path,
    copy: &PlannedCopy,
    progress: &mut dyn FnMut(u64),
) -> Result<(), ProfileError> {
    let dst = mods.join(&copy.folder);
    if dst.exists() {
        return Err(ProfileError::FolderClash(copy.folder.clone()));
    }
    let tmp = mods.join(format!(".rwpm-add-{}", copy.folder));
    if tmp.exists() {
        // Left by a crashed earlier run; only this module writes there.
        std::fs::remove_dir_all(&tmp).map_err(ProfileError::io("clear", &tmp))?;
    }
    let mut hooks = Hooks {
        progress,
        after_copy: &mut |_| {},
    };
    let copied = copy_mod(copy, &tmp, &mut hooks)
        .and_then(|()| std::fs::rename(&tmp, &dst).map_err(ProfileError::io("move", &tmp)));
    if copied.is_err() {
        let _ = std::fs::remove_dir_all(&tmp);
    }
    copied
}

/// "Turn off": removes these ids from the profile's `activeMods`.
pub fn turn_off<T: Trash>(
    store: &ProfileStore<T>,
    id: &ProfileId,
    ids: &[String],
) -> Result<(), ProfileError> {
    let dir = stored_dir(&store.get(id)?)?;
    let changes: Vec<_> = ids.iter().map(|i| (i.clone(), None)).collect();
    rewrite_active_mods(&dir.join(MODS_CONFIG), &changes).map_err(mods_error)
}

fn stored_dir(profile: &Profile) -> Result<std::path::PathBuf, ProfileError> {
    match &profile.location {
        Location::Stored { dir } => Ok(dir.clone()),
        Location::Default { .. } => Err(ProfileError::DefaultImmutable(
            "changed by the OFFLINE guard",
        )),
    }
}

fn mods_error(e: ModsError) -> ProfileError {
    match e {
        ModsError::Io { path, source } => ProfileError::Io {
            op: "update",
            path,
            source,
        },
        ModsError::Xml { path, message } => ProfileError::Parse { path, message },
    }
}

#[cfg(test)]
mod tests;
