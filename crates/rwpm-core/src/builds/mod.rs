//! Stored game builds: `<profiles root>/builds/<key>/` (§C, §I.file).
//!
//! A build is a copy of a RimWorld install that a profile can pin, so a player can keep
//! 1.6 and 1.7 side by side without making Steam re-download either. The folder IS the
//! game directory, with `build.toml` beside the game files.
//!
//! `<key>` is a slug of the trimmed `Version.txt` text, but the build's *identity* is the
//! Steam app manifest `buildid` (V63): `Version.txt` lags the binary (R35), so a hotfix
//! can keep the same version text and still be a different build (B8).

pub mod acf;
pub mod capture;
pub mod file;

use std::fmt;
use std::path::{Path, PathBuf};

use crate::error::ProfileError;
use crate::profile::file::FILE_NAME as PROFILE_FILE;
use crate::profile::id::BUILDS_DIR;
use crate::profile::lock::{take_run_lock, try_run_lock};
use crate::profile::running::RunningProbe;
use crate::profile::store::{ProfileStore, Trash};

pub use file::{BuildFile, dlc_folders};

/// A capture in flight. Never a build: it is swapped into place under its real key.
const STAGING_PREFIX: &str = ".staging-";
/// The build a finished update has replaced, kept until the swap is complete.
const REPLACED_PREFIX: &str = ".replaced-";

/// Folder name of a stored build: a slug of the trimmed `Version.txt` text.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BuildKey(String);

impl BuildKey {
    /// `1.6.4871 rev598` → `1.6.4871-rev598`. Lower case, `[a-z0-9.-]` only.
    pub fn from_version(version: &str) -> Result<Self, ProfileError> {
        let mut key = String::with_capacity(version.len());
        for c in version.trim().chars() {
            if c.is_ascii_alphanumeric() || c == '.' {
                key.push(c.to_ascii_lowercase());
            } else if !key.ends_with('-') {
                key.push('-');
            }
        }
        let key = key.trim_matches(|c| c == '-' || c == '.').to_owned();
        if key.is_empty() {
            return Err(ProfileError::Parse {
                path: PathBuf::from("Version.txt"),
                message: "the game folder has no readable version".to_owned(),
            });
        }
        Ok(Self(key))
    }

    /// Validates a key read from `profile.toml` or a folder name.
    pub fn parse(s: &str) -> Result<Self, ProfileError> {
        let ok = !s.is_empty()
            && s.len() <= 64
            && s.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'.')
            && !s.starts_with(['-', '.'])
            && !s.ends_with(['-', '.']);
        if ok {
            Ok(Self(s.to_owned()))
        } else {
            Err(ProfileError::BuildNotFound(s.to_owned()))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for BuildKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Build {
    pub key: BuildKey,
    /// The build's game directory. Nothing writes in here after capture, except `Mods/`
    /// and a consented `winhttp.dll` (V54).
    pub dir: PathBuf,
    pub file: BuildFile,
}

#[derive(Debug, Default)]
pub struct BuildListing {
    pub builds: Vec<Build>,
    pub unreadable: Vec<(BuildKey, ProfileError)>,
}

pub struct BuildStore {
    root: PathBuf,
    app_dir: PathBuf,
}

impl BuildStore {
    /// The store lives at `<profiles root>/builds`. The folder is made on the first
    /// capture, not here, so an install that never stores a build keeps a clean root.
    ///
    /// Refused when a profile already sits there: `builds` only became reserved in v1.4,
    /// so an older rwpm could have put a profile at that name. Capturing over it would
    /// bury the player's saves under a copy of the game.
    pub fn open(profiles_root: &Path, app_dir: &Path) -> Result<Self, ProfileError> {
        let root = profiles_root.join(BUILDS_DIR);
        if root.join(crate::profile::file::FILE_NAME).is_file() {
            return Err(ProfileError::BuildsFolderTaken(root));
        }
        Ok(Self {
            root,
            app_dir: app_dir.to_path_buf(),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub(crate) fn app_dir(&self) -> &Path {
        &self.app_dir
    }

    pub fn dir_of(&self, key: &BuildKey) -> PathBuf {
        self.root.join(key.as_str())
    }

    /// Every stored build, by key. Tidies up after an interrupted capture first — but
    /// only when nothing else holds `run.lock`, so it never deletes a capture in flight.
    pub fn list(&self) -> Result<BuildListing, ProfileError> {
        self.recover()?;
        let mut listing = BuildListing::default();
        let entries = match std::fs::read_dir(&self.root) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(listing),
            Err(e) => return Err(ProfileError::io("list", &self.root)(e)),
        };
        for entry in entries {
            let entry = entry.map_err(ProfileError::io("list", &self.root))?;
            if !entry.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }
            let Some(key) = entry
                .file_name()
                .to_str()
                .and_then(|n| BuildKey::parse(n).ok())
            else {
                continue;
            };
            let dir = entry.path();
            match BuildFile::read(&dir.join(file::FILE_NAME)) {
                Ok(f) => listing.builds.push(Build { key, dir, file: f }),
                Err(e) => listing.unreadable.push((key, e)),
            }
        }
        listing.builds.sort_by(|a, b| a.key.cmp(&b.key));
        Ok(listing)
    }

    pub fn get(&self, key: &BuildKey) -> Result<Build, ProfileError> {
        let dir = self.dir_of(key);
        if !dir.join(file::FILE_NAME).is_file() {
            return Err(ProfileError::BuildNotFound(key.to_string()));
        }
        Ok(Build {
            key: key.clone(),
            file: BuildFile::read(&dir.join(file::FILE_NAME))?,
            dir,
        })
    }

    /// V54: refused while a profile pins the build. V59: refused while the game runs.
    pub fn delete<T: Trash>(
        &self,
        profiles: &ProfileStore<T>,
        key: &BuildKey,
        probe: &dyn RunningProbe,
    ) -> Result<(), ProfileError> {
        let _lock = take_run_lock(&self.app_dir)?;
        if probe.any_game() {
            return Err(ProfileError::GameRunning);
        }
        let build = self.get(key)?;
        let pinned = pinned_by(profiles, key)?;
        if !pinned.is_empty() {
            return Err(ProfileError::BuildInUse {
                key: key.to_string(),
                profiles: pinned,
            });
        }
        profiles.trash_path(&build.dir)
    }

    /// Clears what an interrupted capture left behind. Steps aside when another process
    /// holds `run.lock`: that process may be mid-capture, and its staging folder is live.
    fn recover(&self) -> Result<(), ProfileError> {
        match try_run_lock(&self.app_dir)? {
            Some(_lock) => self.recover_locked(),
            None => Ok(()),
        }
    }

    /// Caller holds `run.lock`.
    fn recover_locked(&self) -> Result<(), ProfileError> {
        let Ok(entries) = std::fs::read_dir(&self.root) else {
            return Ok(());
        };
        for entry in entries.filter_map(Result::ok) {
            let Some(name) = entry.file_name().to_str().map(ToOwned::to_owned) else {
                continue;
            };
            if name.starts_with(STAGING_PREFIX) {
                // A staging folder is never worth keeping: the capture is re-runnable.
                let _ = std::fs::remove_dir_all(entry.path());
            } else if let Some(key) = name.strip_prefix(REPLACED_PREFIX) {
                let live = self.root.join(key);
                if live.exists() {
                    // The swap finished; only the clean-up did not.
                    let _ = std::fs::remove_dir_all(entry.path());
                } else {
                    // The swap was cut in half. Put the old build back (V60).
                    std::fs::rename(entry.path(), &live)
                        .map_err(ProfileError::io("restore", &live))?;
                }
            }
        }
        Ok(())
    }
}

/// Display names of the profiles pinned to `key` (V54). A `profile.toml` that fails to
/// load (unknown field, newer schema, bad value) is still read as plain TOML: if its
/// `game_build` names this build, it blocks too. A file that is not TOML at all cannot
/// name a build and is passed over.
fn pinned_by<T: Trash>(
    profiles: &ProfileStore<T>,
    key: &BuildKey,
) -> Result<Vec<String>, ProfileError> {
    let listing = profiles.list()?;
    let mut names: Vec<String> = listing
        .profiles
        .into_iter()
        .filter(|p| p.game_build.as_deref() == Some(key.as_str()))
        .map(|p| p.name)
        .collect();
    for (id, _) in &listing.unreadable {
        let path = profiles.root().join(id.as_str()).join(PROFILE_FILE);
        let Some(table) = std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| text.parse::<toml::Table>().ok())
        else {
            continue;
        };
        if table.get("game_build").and_then(toml::Value::as_str) == Some(key.as_str()) {
            let name = table.get("name").and_then(toml::Value::as_str);
            names.push(name.unwrap_or(id.as_str()).to_owned());
        }
    }
    Ok(names)
}

#[cfg(test)]
mod tests;
