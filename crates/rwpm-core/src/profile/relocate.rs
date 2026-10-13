//! Settings → "Profiles folder" Move (V14, V19, V27): copy every profile, verify the
//! copy byte for byte, switch `settings.toml`, then send the old folders to the recycle
//! bin. Nothing old is removed before the copy is verified.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::ProfileError;
use crate::fs_copy::{copy_tree, same_tree};
use crate::profile::id::{BUILDS_DIR, ProfileId};
use crate::profile::lock::take_run_lock;
use crate::profile::running::RunningProbe;
use crate::profile::file::FILE_NAME;
use crate::profile::store::{ProfileStore, Trash};
use crate::settings::AppSettings;
use crate::space;

/// Left in the target while a move runs, so an interrupted move can be retried (DA1).
const MARKER: &str = ".rwpm-move";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MovePlan {
    pub old_root: PathBuf,
    pub new_root: PathBuf,
    pub profiles: Vec<ProfileId>,
    pub bytes: u64,
    pub free_bytes: Option<u64>,
    /// V56: the game-build store moves with the profiles. Carried by name, never by
    /// `ProfileId`, because `builds` is not a profile.
    pub builds: bool,
    /// Profile copies from an interrupted earlier move to the same target.
    leftovers: Vec<PathBuf>,
}

impl MovePlan {
    /// Every folder this move copies: the profiles, then the build store (V56).
    fn moved_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.profiles.iter().map(ToString::to_string).collect();
        if self.builds {
            names.push(BUILDS_DIR.to_owned());
        }
        names
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveReport {
    pub new_root: PathBuf,
    /// Old profile folders the recycle bin refused; their data is safe in the new root.
    pub left_behind: Vec<PathBuf>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Marker {
    old_root: PathBuf,
    profiles: Vec<String>,
    /// Absent in markers written before v1.4, which never carried a build store.
    #[serde(default)]
    builds: bool,
}

/// Checks `new_root` and measures the move. Reads only.
pub fn plan_move<T: Trash>(
    store: &ProfileStore<T>,
    new_root: &Path,
) -> Result<MovePlan, ProfileError> {
    let old_root = store.root().to_path_buf();
    let bad = |why: &str| ProfileError::BadMoveTarget(why.to_owned());
    if !new_root.is_absolute() {
        return Err(bad("choose a full path"));
    }
    let text = new_root
        .to_str()
        .ok_or_else(|| ProfileError::NonUtf8(new_root.to_path_buf()))?;
    if text.contains('=') {
        return Err(ProfileError::PathHasEquals(new_root.to_path_buf()));
    }
    let (old_real, new_real) = (real(&old_root), real(new_root));
    if new_real == old_real {
        return Err(bad("that is already the profiles folder"));
    }
    if new_real.starts_with(&old_real) || old_real.starts_with(&new_real) {
        return Err(bad(
            "the new folder cannot be inside the old one, or hold it",
        ));
    }
    let leftovers = leftovers(new_root).ok_or_else(|| bad("the new folder is not empty"))?;
    let profiles = profile_ids(&old_root)?;
    let builds = old_root.join(BUILDS_DIR).is_dir() && !is_legacy_profile(&old_root);
    Ok(MovePlan {
        bytes: profiles
            .iter()
            .map(|id| space::dir_size(&old_root.join(id.as_str())))
            .sum::<u64>()
            + if builds {
                space::dir_size(&old_root.join(BUILDS_DIR))
            } else {
                0
            },
        free_bytes: space::free_space(new_root),
        old_root,
        new_root: new_root.to_path_buf(),
        profiles,
        builds,
        leftovers,
    })
}

/// Runs the move. Holds `run.lock` throughout, so no profile can be launched meanwhile.
pub fn apply_move<T: Trash>(
    store: &ProfileStore<T>,
    app_dir: &Path,
    plan: &MovePlan,
    probe: &dyn RunningProbe,
    progress: &mut dyn FnMut(u64),
) -> Result<MoveReport, ProfileError> {
    apply_with_hook(store, app_dir, plan, probe, progress, &mut |_| {})
}

/// `after_copy` gets the new root after copying, before verifying (test hook).
fn apply_with_hook<T: Trash>(
    store: &ProfileStore<T>,
    app_dir: &Path,
    plan: &MovePlan,
    probe: &dyn RunningProbe,
    progress: &mut dyn FnMut(u64),
    after_copy: &mut dyn FnMut(&Path),
) -> Result<MoveReport, ProfileError> {
    let _lock = take_run_lock(app_dir)?;
    if probe.any_game() {
        return Err(ProfileError::GameRunning);
    }
    let new = &plan.new_root;
    for dir in &plan.leftovers {
        std::fs::remove_dir_all(dir).map_err(ProfileError::io("clear", dir))?;
    }
    std::fs::create_dir_all(new).map_err(ProfileError::io("create", new))?;
    write_marker(plan)?;
    let copied = copy_all(plan, progress).and_then(|()| {
        after_copy(new);
        plan.moved_names()
            .iter()
            .try_for_each(|n| same_tree(&plan.old_root.join(n), &new.join(n), &|_| false))
    });
    if let Err(e) = copied {
        undo_copies(plan);
        return Err(e);
    }
    // The switch point: from here on the new folder is the profiles folder.
    let mut settings = AppSettings::load(app_dir)?;
    settings.profiles_root = Some(new.clone());
    if let Err(e) = settings.save(app_dir) {
        undo_copies(plan);
        return Err(e);
    }
    let marker = new.join(MARKER);
    std::fs::remove_file(&marker).map_err(ProfileError::io("remove", &marker))?;
    let left_behind = plan
        .moved_names()
        .iter()
        .map(|n| plan.old_root.join(n))
        .filter(|dir| store.trash_path(dir).is_err())
        .collect();
    // Only an empty old folder goes; anything else in it stays where it was.
    let _ = std::fs::remove_dir(&plan.old_root);
    Ok(MoveReport {
        new_root: new.clone(),
        left_behind,
    })
}

/// RimSort instances exported for these profiles that still point into `old_root` (V19).
/// Reads only; re-exporting (T13) rewrites them with the player's consent.
pub fn stale_rimsort(settings: &Path, old_root: &Path, profiles: &[ProfileId]) -> Vec<String> {
    let Ok(bytes) = std::fs::read(settings) else {
        return Vec::new();
    };
    let Ok(root) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return Vec::new();
    };
    let old = old_root.to_string_lossy();
    let Some(instances) = root.get("instances").and_then(|v| v.as_object()) else {
        return Vec::new();
    };
    instances
        .iter()
        .filter(|(_, inst)| {
            let field = |k: &str| inst.get(k).and_then(|v| v.as_str()).unwrap_or_default();
            let ours = profiles
                .iter()
                .any(|id| crate::rimsort::runs_profile(field("run_args"), id.as_str()));
            let stale = ["config_folder", "local_folder"]
                .iter()
                .any(|k| field(k).starts_with(old.as_ref()));
            ours && stale
        })
        .map(|(name, _)| name.clone())
        .collect()
}

/// `<root>/builds` holding a `profile.toml` is a profile made before v1.4, not the build
/// store. It keeps moving as a profile; the store is carried separately.
fn is_legacy_profile(root: &Path) -> bool {
    root.join(BUILDS_DIR).join(FILE_NAME).is_file()
}

/// Every profile-id folder under the old root, with or without `profile.toml`. The build
/// store is left out on purpose (V56): it is carried by name, not as a `ProfileId`.
fn profile_ids(root: &Path) -> Result<Vec<ProfileId>, ProfileError> {
    let entries = match std::fs::read_dir(root) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(ProfileError::io("list", root)(e)),
    };
    let mut ids: Vec<ProfileId> = entries
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| {
            e.file_name()
                .to_str()
                .and_then(|n| ProfileId::parse(n).ok())
        })
        .filter(|id| !id.is_builds() || is_legacy_profile(root))
        .collect();
    ids.sort();
    Ok(ids)
}

/// `Some(dirs to clear)` when the target is usable: missing, empty, or holding only an
/// interrupted move's marker plus the profile folders it lists.
fn leftovers(target: &Path) -> Option<Vec<PathBuf>> {
    let Ok(entries) = std::fs::read_dir(target) else {
        return (!target.exists()).then(Vec::new);
    };
    let names: Vec<String> = entries
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    if names.is_empty() {
        return Some(Vec::new());
    }
    let text = std::fs::read_to_string(target.join(MARKER)).ok()?;
    let marker: Marker = toml::from_str(&text).ok()?;
    let dirs = names.iter().filter(|n| *n != MARKER);
    let mut out = Vec::new();
    for name in dirs {
        let known = marker.profiles.contains(name) || (marker.builds && name == BUILDS_DIR);
        if !known || !target.join(name).is_dir() {
            return None;
        }
        out.push(target.join(name));
    }
    Some(out)
}

fn write_marker(plan: &MovePlan) -> Result<(), ProfileError> {
    let marker = Marker {
        old_root: plan.old_root.clone(),
        profiles: plan.profiles.iter().map(ToString::to_string).collect(),
        builds: plan.builds,
    };
    let path = plan.new_root.join(MARKER);
    let text = toml::to_string(&marker).map_err(|e| ProfileError::Parse {
        path: path.clone(),
        message: e.to_string(),
    })?;
    std::fs::write(&path, text).map_err(ProfileError::io("write", &path))
}

fn copy_all(plan: &MovePlan, progress: &mut dyn FnMut(u64)) -> Result<(), ProfileError> {
    let mut done = 0;
    for name in plan.moved_names() {
        let (from, to) = (plan.old_root.join(&name), plan.new_root.join(&name));
        let base = done;
        copy_tree(&from, &to, &|_| false, &mut |n| {
            done = base + n;
            progress(done);
        })
        .map_err(ProfileError::io("copy", &from))?;
    }
    Ok(())
}

/// Removes only what this move created; the old profiles are untouched.
fn undo_copies(plan: &MovePlan) {
    for name in plan.moved_names() {
        let _ = std::fs::remove_dir_all(plan.new_root.join(name));
    }
    let _ = std::fs::remove_file(plan.new_root.join(MARKER));
    let _ = std::fs::remove_dir(&plan.new_root);
}

/// Canonical form of `p`, or of its nearest existing parent joined with the rest.
fn real(p: &Path) -> PathBuf {
    let mut tail = Vec::new();
    let mut cur = p;
    loop {
        if let Ok(c) = cur.canonicalize() {
            return tail
                .iter()
                .rev()
                .fold(c, |acc: PathBuf, part| acc.join(part));
        }
        match (cur.parent(), cur.file_name()) {
            (Some(parent), Some(name)) => {
                tail.push(name.to_owned());
                cur = parent;
            }
            _ => return p.to_path_buf(),
        }
    }
}

#[cfg(test)]
mod tests;
