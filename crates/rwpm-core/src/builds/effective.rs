//! The effective game of a profile: the stored build it is pinned to, else the
//! Steam install. Every per-profile game lookup goes through `effective_game`.
//!
//! The consumers (go OFFLINE / Refresh, mod list, guard, health, RimSort export) take an
//! `EffectiveGame`, which only this module builds, so none of them can be handed the
//! Steam install for a pinned profile by mistake.

use std::ops::Deref;
use std::path::Path;

use super::{BuildKey, BuildStore};
use crate::discovery::{self, GameInstall};
use crate::error::ProfileError;
use crate::profile::store::Profile;

/// Where a profile's game comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GameSource {
    /// The discovered Steam install (or the game folder set in Settings).
    Steam,
    /// A stored build under `<profiles root>/builds/<key>`.
    Build(BuildKey),
}

/// The game a profile plays. Reads as a `GameInstall`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveGame {
    install: GameInstall,
    source: GameSource,
}

impl EffectiveGame {
    /// The Steam install, as the game of an unpinned profile. Crate-only: callers
    /// outside the crate go through `effective_game`.
    pub(crate) fn steam(install: GameInstall) -> Self {
        Self {
            install,
            source: GameSource::Steam,
        }
    }

    pub fn install(&self) -> &GameInstall {
        &self.install
    }

    pub fn source(&self) -> &GameSource {
        &self.source
    }
}

impl Deref for EffectiveGame {
    type Target = GameInstall;

    fn deref(&self) -> &GameInstall {
        &self.install
    }
}

/// The game `profile` plays.
///
/// - Unpinned (Default is never pinned): `live`, the Steam install; none → `NoGame`.
/// - Pinned, build stored: the build's own folder (exe, engine, version, layout), with
///   the workshop of `live` — or, without one, of the install the build was captured
///   from. Workshop items stay in Steam's library; the copy sits outside it.
/// - Pinned, build gone or not a game folder: `BuildNotFound`. Never the Steam install:
///   that would freeze, list or export mods for the wrong game version.
///
/// The build store is opened only for a pinned profile, so a legacy profile sitting at
/// `<profiles root>/builds` cannot break unpinned profiles.
pub fn effective_game(
    profile: &Profile,
    live: Option<&GameInstall>,
    profiles_root: &Path,
    app_dir: &Path,
) -> Result<EffectiveGame, ProfileError> {
    let Some(key) = profile.game_build.as_deref() else {
        return live
            .cloned()
            .map(EffectiveGame::steam)
            .ok_or(ProfileError::NoGame);
    };
    let key = BuildKey::parse(key)?;
    let build = BuildStore::open(profiles_root, app_dir)?.get(&key)?;
    let mut install = discovery::from_build_dir(&build.dir, &build.file.source_dir)
        .map_err(|_| ProfileError::BuildNotFound(key.to_string()))?;
    if let Some(steam) = live.and_then(|l| l.steam.clone()) {
        install.steam = Some(steam);
    }
    Ok(EffectiveGame {
        install,
        source: GameSource::Build(key),
    })
}

#[cfg(test)]
pub(crate) mod test_support;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod consumers;
