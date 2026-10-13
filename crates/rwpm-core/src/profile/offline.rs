//! go OFFLINE / Refresh / go LIVE. Two phases: `plan_offline` reads only (sizes, free
//! space, conflicts to ask about); `apply_offline` copies and swaps the profile's `Mods`.
//! Sources are only read (V6); copies keep their source folder name (V12).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::builds::EffectiveGame;
use crate::error::ProfileError;
use crate::fs_copy::copy_tree;
use crate::mods::{ModIndex, ModsError, STEAM_POSTFIX, read_active_mods};
use crate::profile::file::{FILE_NAME, FrozenMod, ModSource, Mode, ProfileFile};
use crate::profile::id::ProfileId;
use crate::profile::running::RunningProbe;
use crate::profile::store::{Location, Profile, ProfileStore, Trash};
use crate::space;

const MODS: &str = "Mods";
const STAGING: &str = "Mods.rwpm-new";
const OLD: &str = "Mods.rwpm-old";
const MODS_CONFIG: &str = "SaveData/Config/ModsConfig.xml";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedCopy {
    /// packageId without `_steam`.
    pub package_id: String,
    pub folder: String,
    pub source_dir: PathBuf,
    pub source: ModSource,
    pub bytes: u64,
}

/// One packageId with more than one candidate folder: the player picks one (V20).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    pub package_id: String,
    pub options: Vec<PlannedCopy>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OfflinePlan {
    pub profile: ProfileId,
    /// Frozen copies exist and no Refresh was asked for: only the mode flips.
    pub reuse: bool,
    pub copies: Vec<PlannedCopy>,
    pub conflicts: Vec<Conflict>,
    /// Active ids installed nowhere and with no frozen copy.
    pub missing: Vec<String>,
    /// Refresh: active mods whose source is gone; the old frozen copy is carried over.
    pub kept: Vec<FrozenMod>,
    pub bytes_needed: u64,
    pub free_bytes: Option<u64>,
}

/// Works out what going OFFLINE (or Refresh, with `refresh`) would copy. Reads only.
pub fn plan_offline<T: Trash>(
    store: &ProfileStore<T>,
    id: &ProfileId,
    game: &EffectiveGame,
    refresh: bool,
) -> Result<OfflinePlan, ProfileError> {
    let dir = stored_dir(&store.get(id)?, "made OFFLINE")?;
    let file = ProfileFile::read(&dir.join(FILE_NAME))?;
    let mut plan = OfflinePlan {
        profile: id.clone(),
        reuse: false,
        copies: Vec::new(),
        conflicts: Vec::new(),
        missing: Vec::new(),
        kept: Vec::new(),
        bytes_needed: 0,
        free_bytes: space::free_space(&dir),
    };
    if !refresh && !file.frozen_mods.is_empty() && dir.join(MODS).is_dir() {
        plan.reuse = true;
        return Ok(plan);
    }
    let config = dir.join(MODS_CONFIG);
    // V36: no mod list yet (a new profile) → nothing to freeze; the launch guard (V5)
    // offers to freeze whatever the player turns on later.
    let active = if config.is_file() {
        read_active_mods(&config).map_err(mods_error)?
    } else {
        Vec::new()
    };
    let workshop = game.steam.as_ref().map(|s| s.workshop_dir.as_path());
    let index = ModIndex::scan(&game.data_dir(), &game.mods_dir(), workshop);
    let candidates = group_candidates(&index, &active, &file, &mut plan);
    for (package_id, mut options) in candidates {
        if options.len() == 1 {
            plan.copies.push(options.remove(0));
        } else {
            plan.conflicts.push(Conflict {
                package_id,
                options,
            });
        }
    }
    plan.bytes_needed = plan.copies.iter().map(|c| c.bytes).sum();
    Ok(plan)
}

/// Candidate folders per packageId (without `_steam`), in load order. Fills
/// `missing` / `kept` on the plan.
fn group_candidates(
    index: &ModIndex,
    active: &[String],
    file: &ProfileFile,
    plan: &mut OfflinePlan,
) -> Vec<(String, Vec<PlannedCopy>)> {
    let mut groups: Vec<(String, Vec<PlannedCopy>)> = Vec::new();
    for id in active {
        let base = id.strip_suffix(STEAM_POSTFIX).unwrap_or(id).to_owned();
        let Some(found) = index.find(id) else {
            match file.frozen_mods.iter().find(|f| f.package_id == base) {
                Some(f) => plan.kept.push(f.clone()),
                None => plan.missing.push(id.clone()),
            }
            continue;
        };
        let Some(source) = found.origin.mod_source() else {
            continue; // official content is never copied
        };
        let ambiguous = index.resolve(std::slice::from_ref(id)).ambiguous;
        let dirs = match ambiguous.into_iter().next() {
            Some((_, dirs)) => dirs,
            None => vec![found.dir.clone()],
        };
        let slot = match groups.iter().position(|(p, _)| *p == base) {
            Some(i) => i,
            None => {
                groups.push((base.clone(), Vec::new()));
                groups.len() - 1
            }
        };
        for dir in dirs {
            if groups[slot].1.iter().all(|c| c.source_dir != dir) {
                groups[slot].1.push(planned(&base, dir, source));
            }
        }
    }
    groups
}

pub(crate) fn planned(package_id: &str, dir: PathBuf, source: ModSource) -> PlannedCopy {
    PlannedCopy {
        package_id: package_id.to_owned(),
        folder: dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        bytes: space::dir_size(&dir),
        source_dir: dir,
        source,
    }
}

/// Copies the plan into the profile and makes it OFFLINE. `choices` = (packageId, chosen
/// source dir) for every conflict. On any failure the old `Mods` and `profile.toml` stay.
pub fn apply_offline<T: Trash>(
    store: &ProfileStore<T>,
    plan: &OfflinePlan,
    choices: &[(String, PathBuf)],
    probe: &dyn RunningProbe,
    progress: &mut dyn FnMut(u64),
) -> Result<Profile, ProfileError> {
    let hooks = Hooks {
        progress,
        after_copy: &mut |_| {},
    };
    apply_with_hook(store, plan, choices, probe, hooks)
}

/// Callbacks during the copy. `after_copy` runs after each mod copy, before its source
/// is re-checked (a test hook for the torn-copy guard).
pub(crate) struct Hooks<'a> {
    pub(crate) progress: &'a mut dyn FnMut(u64),
    pub(crate) after_copy: &'a mut dyn FnMut(&Path),
}

/// What the new `Mods` holds: fresh copies, kept frozen copies, hand-added folders.
struct NewMods<'a> {
    copies: &'a [PlannedCopy],
    kept: &'a [FrozenMod],
    carried: &'a [String],
}

fn apply_with_hook<T: Trash>(
    store: &ProfileStore<T>,
    plan: &OfflinePlan,
    choices: &[(String, PathBuf)],
    probe: &dyn RunningProbe,
    mut hooks: Hooks,
) -> Result<Profile, ProfileError> {
    if probe.any_game() {
        return Err(ProfileError::GameRunning);
    }
    let dir = stored_dir(&store.get(&plan.profile)?, "made OFFLINE")?;
    let toml = dir.join(FILE_NAME);
    let mut file = ProfileFile::read(&toml)?;
    if plan.reuse {
        file.mode = Mode::Offline;
        file.write_atomic(&toml)?;
        return store.get(&plan.profile);
    }
    let copies = chosen_copies(plan, choices)?;
    let carried = hand_added(&dir.join(MODS), &file);
    let new_mods = NewMods {
        copies: &copies,
        kept: &plan.kept,
        carried: &carried,
    };
    check_folders(&new_mods)?;
    let staging = dir.join(STAGING);
    if staging.exists() {
        // Left by a crashed earlier run; only this module writes there.
        std::fs::remove_dir_all(&staging).map_err(ProfileError::io("clear", &staging))?;
    }
    let filled = fill_staging(&dir, &staging, &new_mods, &mut hooks);
    let frozen = match filled {
        Ok(frozen) => frozen,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(e);
        }
    };
    file.mode = Mode::Offline;
    file.frozen_mods = frozen;
    swap_in(store, &dir, &staging, &file)?;
    store.get(&plan.profile)
}

fn chosen_copies(
    plan: &OfflinePlan,
    choices: &[(String, PathBuf)],
) -> Result<Vec<PlannedCopy>, ProfileError> {
    let mut copies = plan.copies.clone();
    let mut unresolved = Vec::new();
    for conflict in &plan.conflicts {
        let pick = choices
            .iter()
            .filter(|(id, _)| *id == conflict.package_id)
            .find_map(|(_, dir)| conflict.options.iter().find(|o| o.source_dir == *dir));
        match pick {
            Some(option) => copies.push(option.clone()),
            None => unresolved.push(conflict.package_id.clone()),
        }
    }
    if unresolved.is_empty() {
        Ok(copies)
    } else {
        Err(ProfileError::Unresolved(unresolved))
    }
}

/// Folders in the current `Mods` that no frozen record covers (put there by hand).
fn hand_added(mods: &Path, file: &ProfileFile) -> Vec<String> {
    let frozen: HashSet<&str> = file.frozen_mods.iter().map(|f| f.folder.as_str()).collect();
    let Ok(entries) = std::fs::read_dir(mods) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| !frozen.contains(n.as_str()))
        .collect();
    names.sort();
    names
}

/// V12: folder names are never changed, so two mods may not share one.
fn check_folders(new_mods: &NewMods) -> Result<(), ProfileError> {
    let mut seen = HashSet::new();
    let names = new_mods
        .copies
        .iter()
        .map(|c| c.folder.as_str())
        .chain(new_mods.kept.iter().map(|k| k.folder.as_str()))
        .chain(new_mods.carried.iter().map(String::as_str));
    for name in names {
        if !seen.insert(name.to_lowercase()) {
            return Err(ProfileError::FolderClash(name.to_owned()));
        }
    }
    Ok(())
}

/// Builds the new `Mods` in `staging`; returns the frozen records for it.
fn fill_staging(
    dir: &Path,
    staging: &Path,
    new_mods: &NewMods,
    hooks: &mut Hooks,
) -> Result<Vec<FrozenMod>, ProfileError> {
    std::fs::create_dir(staging).map_err(ProfileError::io("create", staging))?;
    let mut done = 0;
    let mut frozen = Vec::new();
    let copied_at = now_datetime();
    for copy in new_mods.copies {
        let base = done;
        let progress = &mut *hooks.progress;
        let mut step = Hooks {
            progress: &mut |n| {
                done = base + n;
                progress(done);
            },
            after_copy: &mut *hooks.after_copy,
        };
        copy_mod(copy, &staging.join(&copy.folder), &mut step)?;
        frozen.push(FrozenMod {
            folder: copy.folder.clone(),
            package_id: copy.package_id.clone(),
            source: copy.source,
            source_dir: copy.source_dir.clone(),
            copied_at,
        });
    }
    let old = dir.join(MODS);
    let kept_folders = new_mods.kept.iter().map(|k| &k.folder);
    for folder in kept_folders.chain(new_mods.carried.iter()) {
        let (from, to) = (old.join(folder), staging.join(folder));
        copy_tree(&from, &to, &|_| false, &mut |_| {}).map_err(ProfileError::io("copy", &from))?;
    }
    frozen.extend(new_mods.kept.iter().cloned());
    Ok(frozen)
}

/// Copies one mod; if its source changed meanwhile (Steam updating it), tries once more.
pub(crate) fn copy_mod(
    copy: &PlannedCopy,
    dst: &Path,
    hooks: &mut Hooks,
) -> Result<(), ProfileError> {
    let src = &copy.source_dir;
    for _ in 0..2 {
        let before = fingerprint(src);
        copy_tree(src, dst, &|_| false, hooks.progress).map_err(ProfileError::io("copy", src))?;
        (hooks.after_copy)(src);
        if fingerprint(src) == before {
            return Ok(());
        }
        std::fs::remove_dir_all(dst).map_err(ProfileError::io("clear", dst))?;
    }
    Err(ProfileError::SourceChanging(copy.folder.clone()))
}

/// (files, bytes, newest mtime) of a tree; symlinks not followed.
fn fingerprint(dir: &Path) -> (u64, u64, Option<SystemTime>) {
    let mut acc = (0, 0, None);
    walk(dir, &mut acc);
    acc
}

fn walk(dir: &Path, acc: &mut (u64, u64, Option<SystemTime>)) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let Ok(meta) = entry.metadata() else { continue };
        if meta.is_dir() {
            walk(&entry.path(), acc);
        } else {
            acc.0 += 1;
            acc.1 += meta.len();
            acc.2 = acc.2.max(meta.modified().ok());
        }
    }
}

/// Old `Mods` → `Mods.rwpm-old`, staging → `Mods`, then `profile.toml`, each step undone
/// on failure. The old copies then go to the recycle bin.
fn swap_in<T: Trash>(
    store: &ProfileStore<T>,
    dir: &Path,
    staging: &Path,
    file: &ProfileFile,
) -> Result<(), ProfileError> {
    let (mods, old) = (dir.join(MODS), dir.join(OLD));
    let had_old = mods.exists();
    let undo_staging = |e: ProfileError| {
        let _ = std::fs::remove_dir_all(staging);
        e
    };
    if old.exists() {
        std::fs::remove_dir_all(&old)
            .map_err(ProfileError::io("clear", &old))
            .map_err(undo_staging)?;
    }
    if had_old {
        std::fs::rename(&mods, &old)
            .map_err(ProfileError::io("move", &mods))
            .map_err(undo_staging)?;
    }
    let restore = |e: ProfileError| {
        if had_old {
            let _ = std::fs::rename(&old, &mods);
        }
        e
    };
    std::fs::rename(staging, &mods)
        .map_err(ProfileError::io("move", staging))
        .map_err(undo_staging)
        .map_err(restore)?;
    if let Err(e) = file.write_atomic(&dir.join(FILE_NAME)) {
        let _ = std::fs::rename(&mods, staging);
        return Err(restore(undo_staging(e)));
    }
    if had_old {
        store.trash_path(&old)?;
    }
    Ok(())
}

/// Makes the profile LIVE. Frozen copies stay, so going OFFLINE again restores them.
pub fn go_live<T: Trash>(
    store: &ProfileStore<T>,
    id: &ProfileId,
    probe: &dyn RunningProbe,
) -> Result<Profile, ProfileError> {
    if probe.any_game() {
        return Err(ProfileError::GameRunning);
    }
    let profile = store.get(id)?;
    let Location::Stored { dir } = &profile.location else {
        return Ok(profile); // Default is always LIVE
    };
    let toml = dir.join(FILE_NAME);
    let mut file = ProfileFile::read(&toml)?;
    file.mode = Mode::Live;
    file.write_atomic(&toml)?;
    store.get(id)
}

fn stored_dir(profile: &Profile, what: &'static str) -> Result<PathBuf, ProfileError> {
    match &profile.location {
        Location::Stored { dir } => Ok(dir.clone()),
        Location::Default { .. } => Err(ProfileError::DefaultImmutable(what)),
    }
}

fn mods_error(e: ModsError) -> ProfileError {
    match e {
        ModsError::Io { path, source } => ProfileError::Io {
            op: "read",
            path,
            source,
        },
        ModsError::Xml { path, message } => ProfileError::Parse { path, message },
    }
}

/// Current UTC time as a TOML datetime (also used for log lines).
pub fn now_datetime() -> toml::value::Datetime {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    utc_datetime(secs)
}

/// Unix seconds → TOML UTC datetime (civil-from-days, H. Hinnant).
fn utc_datetime(secs: u64) -> toml::value::Datetime {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    let text = format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    );
    text.parse().expect("formatted datetime is valid TOML")
}

#[cfg(test)]
mod tests;
