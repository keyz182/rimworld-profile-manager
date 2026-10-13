//! Capturing a Steam install as a stored build, and updating one that is already stored.
//!
//! The flow follows the proven T50 spike (`spike/linux-builds/capture.sh`): read the
//! version and the Steam app manifest, refuse unless the install is complete, clone into a
//! staging folder, drop the files that are Steam's rather than the game's, then rename the
//! staging folder into place.
//!
//! An update (V60) is the same capture into a staging folder *beside* the build, with the
//! old build's `Mods/` carried over, verified, and only then swapped in. Until the swap the
//! old build is whole and launchable; a failure at any earlier point leaves it untouched.
//!
//! Captured builds are never re-signed on macOS. RimWorld's bundle ships seal-invalid
//! (Ludeon sealed `Contents/PlugIns`, the install has `Contents/Plugins`, which resolves
//! the same on case-insensitive APFS), so `codesign --force --deep` would make the stored
//! build diverge from the install it is meant to reproduce and swap Ludeon's identity for
//! an ad-hoc one. A faithful copy that reproduces the same state is the correct outcome.

use std::path::{Path, PathBuf};

use super::acf::{self, BetaSource};
use super::file::{self, BuildFile, dlc_folders};
use super::{Build, BuildKey, BuildStore, REPLACED_PREFIX, STAGING_PREFIX};
use crate::discovery::{GameInstall, RIMWORLD_APP_ID};
use crate::error::ProfileError;
use crate::fs_copy::{copy_tree, same_tree};
use crate::profile::lock::take_run_lock;
use crate::profile::running::RunningProbe;
use crate::space;

/// Why a stored build is being offered as an update rather than skipped (V63, §C).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateReason {
    /// Same `Version.txt`, different `buildid`: a hotfix (B8).
    NewBuildId,
    /// The player installed or removed an expansion.
    DlcChanged,
    /// The same version text on a different Steam branch.
    BetaChanged,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureKind {
    /// Nothing stored under this key or this `buildid`.
    New,
    /// This exact build is already stored, possibly under another key (V53).
    NoOp { stored: BuildKey },
    /// The key is taken by an older build of the same version (V60).
    Update {
        reason: UpdateReason,
        old: Box<BuildFile>,
    },
}

/// What a capture would do, measured without touching anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturePlan {
    pub kind: CaptureKind,
    pub key: BuildKey,
    pub version: String,
    pub buildid: String,
    pub beta: String,
    /// Which app-manifest block named the branch. `UserConfig` means Steam has recorded a
    /// branch switch whose download has not landed in `MountedConfig` yet.
    pub beta_source: BetaSource,
    pub dlc: Vec<String>,
    pub source_dir: PathBuf,
    /// `Mods/` relative to the game dir: inside the bundle on macOS (R29).
    mods_rel: PathBuf,
    manifest: PathBuf,
    /// Bytes the copy will add. Reflink copies usually cost far less; this is the
    /// worst case, which is what a free-space check has to assume.
    pub bytes: u64,
    /// Bytes the store holds while an update runs: the new copy and the old build, which
    /// is kept until the swap so a failure leaves it launchable (V60).
    pub peak_bytes: u64,
    pub free_bytes: Option<u64>,
}

impl CapturePlan {
    pub fn is_no_op(&self) -> bool {
        matches!(self.kind, CaptureKind::NoOp { .. })
    }
}

/// Files that belong to Steam or to a crash, not to the game.
///
/// Matched on the file name at any depth: Steam writes `steam_appid.txt` at the install
/// root, beside the macOS bundle, and inside `RimWorldMac.app/Contents/MacOS/` (T52). V52
/// needs the capture to run on the `SteamAppId` env var alone.
fn skip(rel: &Path) -> bool {
    let name = rel.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    name == "steam_appid.txt"
        || name.starts_with("mono_crash.")
        || rel == Path::new(file::FILE_NAME)
}

/// Reads what a capture of `install` would do. Touches nothing.
pub fn plan_capture(
    store: &BuildStore,
    install: &GameInstall,
) -> Result<CapturePlan, ProfileError> {
    let version = install.version.clone().ok_or_else(|| ProfileError::Parse {
        path: install.version_file(),
        message: "the game folder has no readable version".to_owned(),
    })?;
    let key = BuildKey::from_version(&version)?;
    let manifest = manifest_path(install)?;
    let app = acf::read(&manifest)?;
    // V58: 4 exactly. Anything else means Steam has not finished with the folder.
    if !app.fully_installed() {
        return Err(ProfileError::SourceChanging("RimWorld".to_owned()));
    }
    let dlc = dlc_folders(&install.data_dir());
    let stored = store.list()?;
    let kind = classify(&stored.builds, &key, &app.buildid, &app.beta, &dlc);
    let bytes = space::dir_size(&install.game_dir);
    let old_bytes = match &kind {
        CaptureKind::Update { .. } => space::dir_size(&store.dir_of(&key)),
        _ => 0,
    };
    let mods_rel = install
        .mods_dir()
        .strip_prefix(&install.game_dir)
        .unwrap_or(Path::new("Mods"))
        .to_path_buf();
    Ok(CapturePlan {
        kind,
        key,
        version,
        buildid: app.buildid,
        beta: app.beta,
        beta_source: app.beta_source,
        dlc,
        source_dir: install.game_dir.clone(),
        mods_rel,
        manifest,
        bytes,
        peak_bytes: bytes + old_bytes,
        free_bytes: space::free_space(store.root()),
    })
}

/// V53: an identical build already stored is a no-op, even under another key.
/// V63/B8: the same `Version.txt` with a new `buildid` is an update, never a no-op.
fn classify(
    stored: &[Build],
    key: &BuildKey,
    buildid: &str,
    beta: &str,
    dlc: &[String],
) -> CaptureKind {
    if let Some(same) = stored
        .iter()
        .find(|b| b.file.buildid == buildid && b.file.dlc == dlc && b.file.beta == beta)
    {
        return CaptureKind::NoOp {
            stored: same.key.clone(),
        };
    }
    let Some(old) = stored.iter().find(|b| b.key == *key) else {
        return CaptureKind::New;
    };
    let reason = if old.file.buildid != buildid {
        UpdateReason::NewBuildId
    } else if old.file.beta != beta {
        UpdateReason::BetaChanged
    } else {
        UpdateReason::DlcChanged
    };
    CaptureKind::Update {
        reason,
        old: Box::new(old.file.clone()),
    }
}

fn manifest_path(install: &GameInstall) -> Result<PathBuf, ProfileError> {
    install
        .steam
        .as_ref()
        .map(|s| {
            s.library_dir
                .join("steamapps")
                .join(format!("appmanifest_{RIMWORLD_APP_ID}.acf"))
        })
        .ok_or_else(|| ProfileError::NotSteamInstall(install.game_dir.clone()))
}

/// Runs the capture. `Ok(None)` when there was nothing to do (V53).
pub fn apply_capture(
    store: &BuildStore,
    plan: &CapturePlan,
    probe: &dyn RunningProbe,
    progress: &mut dyn FnMut(u64),
) -> Result<Option<Build>, ProfileError> {
    apply_with_hook(store, plan, probe, progress, &mut |_| Ok(()))
}

/// `after_copy` gets the staging dir once the copy is done, before it is verified (test
/// hook: it stands in for a crash or a corrupted clone).
fn apply_with_hook(
    store: &BuildStore,
    plan: &CapturePlan,
    probe: &dyn RunningProbe,
    progress: &mut dyn FnMut(u64),
    after_copy: &mut dyn FnMut(&Path) -> Result<(), ProfileError>,
) -> Result<Option<Build>, ProfileError> {
    // V59: both capture and update are refused while the game runs.
    let _lock = take_run_lock(store.app_dir())?;
    if probe.any_game() {
        return Err(ProfileError::GameRunning);
    }
    if plan.is_no_op() {
        return Ok(None);
    }
    // Steam does not respect our lock, so re-read the manifest inside it: the install may
    // have started updating since the plan was made (V58).
    let app = acf::read(&plan.manifest)?;
    if !app.fully_installed() || app.buildid != plan.buildid {
        return Err(ProfileError::SourceChanging("RimWorld".to_owned()));
    }
    if let Some(free) = plan.free_bytes
        && free < plan.bytes
    {
        return Err(ProfileError::NotEnoughSpace {
            path: store.root().to_path_buf(),
            need: plan.bytes,
            free,
        });
    }
    std::fs::create_dir_all(store.root()).map_err(ProfileError::io("create", store.root()))?;
    store.recover_locked()?;

    let staging = store.root().join(format!("{STAGING_PREFIX}{}", plan.key));
    let _ = std::fs::remove_dir_all(&staging);
    let built = stage(plan, &staging, store, progress, after_copy);
    if built.is_err() {
        // Nothing outside the staging folder has been touched yet, so the old build (if
        // any) is still whole and launchable (V60).
        let _ = std::fs::remove_dir_all(&staging);
    }
    built?;
    swap_in(store, plan, &staging)?;
    store.get(&plan.key).map(Some)
}

/// Builds the complete new build inside `staging`, verified and with `build.toml` written.
fn stage(
    plan: &CapturePlan,
    staging: &Path,
    store: &BuildStore,
    progress: &mut dyn FnMut(u64),
    after_copy: &mut dyn FnMut(&Path) -> Result<(), ProfileError>,
) -> Result<(), ProfileError> {
    let mut done = 0;
    copy_tree(&plan.source_dir, staging, &skip, &mut |n| {
        done = n;
        progress(done);
    })
    .map_err(ProfileError::io("copy", &plan.source_dir))?;
    after_copy(staging)?;
    same_tree(&plan.source_dir, staging, &skip)?;

    if matches!(plan.kind, CaptureKind::Update { .. }) {
        carry_mods(plan, staging, &store.dir_of(&plan.key), progress, &mut done)?;
    }
    let file = BuildFile {
        schema: file::SCHEMA,
        version: plan.version.clone(),
        buildid: plan.buildid.clone(),
        beta: plan.beta.clone(),
        dlc: plan.dlc.clone(),
        captured_at: now(),
        source_dir: plan.source_dir.clone(),
        size: space::dir_size(staging),
    };
    file.write_atomic(&staging.join(file::FILE_NAME))
}

/// V60: `<build>/Mods/` comes from the OLD build, not from the Steam install, so local
/// mods kept per build survive an update. The old build is only read here.
fn carry_mods(
    plan: &CapturePlan,
    staging: &Path,
    old: &Path,
    progress: &mut dyn FnMut(u64),
    done: &mut u64,
) -> Result<(), ProfileError> {
    let (from, to) = (old.join(&plan.mods_rel), staging.join(&plan.mods_rel));
    if to.exists() {
        std::fs::remove_dir_all(&to).map_err(ProfileError::io("replace", &to))?;
    }
    if !from.is_dir() {
        // The old build has no Mods folder, so there is nothing to carry.
        return Ok(());
    }
    let base = *done;
    copy_tree(&from, &to, &|_| false, &mut |n| {
        *done = base + n;
        progress(*done);
    })
    .map_err(ProfileError::io("copy", &from))?;
    same_tree(&from, &to, &|_| false)
}

/// Puts the staged build in place. For an update this is two renames: a crash between
/// them leaves `.replaced-<key>` behind, which `recover` puts back (V60).
fn swap_in(store: &BuildStore, plan: &CapturePlan, staging: &Path) -> Result<(), ProfileError> {
    let live = store.dir_of(&plan.key);
    let replaced = store.root().join(format!("{REPLACED_PREFIX}{}", plan.key));
    if live.exists() {
        std::fs::rename(&live, &replaced).map_err(ProfileError::io("replace", &live))?;
    }
    if let Err(e) = std::fs::rename(staging, &live) {
        // Put the old build back before giving up; it must stay launchable (V60).
        if replaced.exists() {
            let _ = std::fs::rename(&replaced, &live);
        }
        let _ = std::fs::remove_dir_all(staging);
        return Err(ProfileError::io("store", &live)(e));
    }
    let _ = std::fs::remove_dir_all(&replaced);
    Ok(())
}

fn now() -> toml::value::Datetime {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let days = secs / 86_400;
    let (y, m, d) = civil_from_days(days as i64);
    let (hh, mm, ss) = (secs % 86_400 / 3600, secs % 3600 / 60, secs % 60);
    format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}Z")
        .parse()
        .expect("datetime")
}

/// Howard Hinnant's days-from-civil, inverted. Avoids a date dependency for one timestamp.
fn civil_from_days(z: i64) -> (i64, u64, u64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u64;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u64;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
pub(super) fn apply_for_test(
    store: &BuildStore,
    plan: &CapturePlan,
    probe: &dyn RunningProbe,
    progress: &mut dyn FnMut(u64),
    after_copy: &mut dyn FnMut(&Path) -> Result<(), ProfileError>,
) -> Result<Option<Build>, ProfileError> {
    apply_with_hook(store, plan, probe, progress, after_copy)
}
