//! Reset: put a pack profile's mod list, settings and creations back as its pack had them.
//!
//! The pack is the copy kept beside the profile (`PACK_FILE`). It is read like any other
//! pack, into a staging folder, and only then merged into the profile: a file that already
//! matches stays, a different one goes to the recycle bin first, so a reset can be undone.
//! Saves come back only when the player asks. Files the pack does not hold are left alone.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::entry::SAVE_DATA;
use super::file::ModEntry;
use super::read_preview;
use super::unpack::{UnpackPlan, extract};
use crate::error::ProfileError;
use crate::profile::file::{FILE_NAME as PROFILE_FILE, PACK_FILE};
use crate::profile::running::RunningProbe;
use crate::profile::{Mode, Profile, ProfileFile, ProfileId, ProfileStore, Trash};
use crate::space;

pub(crate) const STAGING: &str = "reset-staging";

/// What a reset would do, for its confirm dialog.
#[derive(Debug, Clone)]
pub struct ResetPlan {
    pub profile: Profile,
    /// The pack's manifest and cover, as read from the kept pack.
    pub unpack: UnpackPlan,
    pub restore_saves: bool,
    /// Offline profile only: pack mods the profile holds no copy of. The pack lists mods,
    /// it does not hold them, so a reset cannot make these appear; fetch them first.
    pub missing: Vec<ModEntry>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ResetReport {
    /// Files that were different; the old ones are in the recycle bin.
    pub replaced: usize,
    pub added: usize,
    /// Files that already matched the pack.
    pub unchanged: usize,
}

pub fn plan_reset<T: Trash>(
    store: &ProfileStore<T>,
    id: &ProfileId,
    restore_saves: bool,
) -> Result<ResetPlan, ProfileError> {
    if id.is_default() {
        return Err(ProfileError::DefaultImmutable("reset"));
    }
    let profile = store.get(id)?;
    if profile.pack.is_none() {
        return Err(ProfileError::NoPackOrigin(profile.name));
    }
    let dir = stored(&profile)?.to_path_buf();
    let archive = dir.join(PACK_FILE);
    if !archive.is_file() {
        return Err(ProfileError::PackFileMissing(archive));
    }
    let (manifest, cover) = read_preview(&archive, true)?;
    let bytes_needed = if restore_saves {
        manifest.uncompressed_bytes
    } else {
        0
    };
    let free_bytes = space::free_space(&dir);
    if let Some(free) = free_bytes.filter(|&f| f < bytes_needed) {
        return Err(ProfileError::NotEnoughSpace {
            path: dir,
            need: bytes_needed,
            free,
        });
    }
    let missing = if profile.mode == Mode::Offline {
        let held = held_ids(&dir)?;
        let want = |m: &&ModEntry| !held.contains(&m.package_id.to_lowercase());
        manifest.mods.iter().filter(want).cloned().collect()
    } else {
        Vec::new()
    };
    Ok(ResetPlan {
        unpack: UnpackPlan {
            archive,
            name: profile.name.clone(),
            manifest,
            cover,
            bytes_needed,
            free_bytes,
        },
        profile,
        restore_saves,
        missing,
    })
}

fn stored(profile: &Profile) -> Result<&Path, ProfileError> {
    profile
        .stored_dir()
        .ok_or(ProfileError::DefaultImmutable("reset"))
}

/// Package ids of the mods an offline profile holds a copy of, lower case.
fn held_ids(dir: &Path) -> Result<BTreeSet<String>, ProfileError> {
    let file = ProfileFile::read(&dir.join(PROFILE_FILE))?;
    Ok(file
        .frozen_mods
        .iter()
        .map(|m| m.package_id.to_lowercase())
        .collect())
}

/// Resets the profile. Refused while a game runs, or while an offline profile lacks mods
/// the pack lists. A failure part-way leaves a mix of old and pack files; whatever was
/// replaced is in the recycle bin.
pub fn apply_reset<T: Trash>(
    plan: &ResetPlan,
    store: &ProfileStore<T>,
    probe: &dyn RunningProbe,
    progress: &mut dyn FnMut(u64),
) -> Result<ResetReport, ProfileError> {
    if probe.any_game() {
        return Err(ProfileError::GameRunning);
    }
    if !plan.missing.is_empty() {
        let names = plan.missing.iter().map(|m| m.name.clone()).collect();
        return Err(ProfileError::ResetNeedsMods(names));
    }
    let dir = stored(&plan.profile)?;
    let staging = dir.join(STAGING);
    remove(&staging)?;
    std::fs::create_dir_all(&staging).map_err(ProfileError::io("create", &staging))?;
    let done = extract(&plan.unpack, &staging, plan.restore_saves, progress).and_then(|()| {
        merge(
            store,
            &staging.join(SAVE_DATA),
            &plan.profile.save_data_dir(),
        )
    });
    let _ = remove(&staging);
    done
}

fn remove(dir: &Path) -> Result<(), ProfileError> {
    match std::fs::remove_dir_all(dir) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            Err(ProfileError::io("remove", dir)(e))
        }
        _ => Ok(()),
    }
}

fn merge<T: Trash>(
    store: &ProfileStore<T>,
    from: &Path,
    to: &Path,
) -> Result<ResetReport, ProfileError> {
    let mut files = Vec::new();
    list(from, Path::new(""), &mut files)?;
    let mut report = ResetReport::default();
    for rel in files {
        let (src, dest) = (from.join(&rel), to.join(&rel));
        if dest.exists() {
            if same(&src, &dest) {
                report.unchanged += 1;
                continue;
            }
            store.trash_path(&dest)?;
            report.replaced += 1;
        } else {
            report.added += 1;
        }
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(ProfileError::io("create", parent))?;
        }
        std::fs::rename(&src, &dest).map_err(ProfileError::io("move", &dest))?;
    }
    Ok(report)
}

/// Every file below `root/rel`, as paths relative to `root`.
fn list(root: &Path, rel: &Path, out: &mut Vec<PathBuf>) -> Result<(), ProfileError> {
    let here = root.join(rel);
    let Ok(read) = std::fs::read_dir(&here) else {
        return Ok(());
    };
    for entry in read {
        let entry = entry.map_err(ProfileError::io("read", &here))?;
        let next = rel.join(entry.file_name());
        let kind = entry.file_type().map_err(ProfileError::io("read", &here))?;
        if kind.is_dir() {
            list(root, &next, out)?;
        } else {
            out.push(next);
        }
    }
    Ok(())
}

fn same(a: &Path, b: &Path) -> bool {
    match (std::fs::read(a), std::fs::read(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}
