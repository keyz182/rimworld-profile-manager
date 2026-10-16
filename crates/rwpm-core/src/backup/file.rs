//! `rimrim.toml`, the first entry of a `.rimrim` backup (§I file `.rimrim`).

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::builds::BuildKey;
use crate::error::ProfileError;
use crate::profile::Mode;

/// Highest schema this build reads and the one it writes.
pub const SCHEMA: u32 = 1;
pub const FILE_NAME: &str = "rimrim.toml";

/// What the player chose to put in the backup. `Mods/` and profile data are always in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Includes {
    pub saves: bool,
    pub logs: bool,
    pub build: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema: u32,
    pub created_at: toml::value::Datetime,
    pub rwpm_version: String,
    pub name: String,
    pub mode: Mode,
    /// Key of the pinned build the profile used; absent = the Steam install.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub game_build: Option<String>,
    /// Raw `Version.txt` of that build, for the restore preview.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub game_version: Option<String>,
    /// `std::env::consts::OS` / `ARCH` of the machine that made the backup (V78).
    pub os: String,
    pub arch: String,
    /// Total bytes of every regular file after `rimrim.toml`, and the number of entries
    /// after it: the limits restore enforces while it streams (V75).
    pub uncompressed_bytes: u64,
    pub entry_count: u64,
    pub includes: Includes,
    pub notice_mods: String,
    pub notice_game: String,
}

impl Manifest {
    /// Parses and validates the text of `rimrim.toml`. `path` names the backup, for errors.
    /// A newer schema is refused before field checks run.
    pub fn parse(text: &str, path: &Path) -> Result<Self, ProfileError> {
        let bad = |reason: String| ProfileError::BadBackup {
            path: path.to_path_buf(),
            reason,
        };
        let table: toml::Table = toml::from_str(text).map_err(|e| bad(e.to_string()))?;
        let found = table.get("schema").and_then(toml::Value::as_integer);
        match found.map(u32::try_from) {
            Some(Ok(found)) if found > SCHEMA => {
                return Err(ProfileError::SchemaTooNew {
                    path: path.to_path_buf(),
                    found,
                    supported: SCHEMA,
                });
            }
            Some(Ok(_)) => {}
            _ => return Err(bad("missing or invalid `schema`".into())),
        }
        let manifest: Self = table
            .try_into()
            .map_err(|e: toml::de::Error| bad(e.to_string()))?;
        manifest.check().map_err(bad)?;
        Ok(manifest)
    }

    pub fn to_toml(&self) -> Result<String, ProfileError> {
        toml::to_string_pretty(self).map_err(|e| ProfileError::BadBackup {
            path: FILE_NAME.into(),
            reason: e.to_string(),
        })
    }

    fn check(&self) -> Result<(), String> {
        if self.mode != Mode::Offline {
            return Err("only offline profiles can be backed up".into());
        }
        if self.name.trim().is_empty() {
            return Err("the profile has no name".into());
        }
        if let Some(key) = &self.game_build {
            BuildKey::parse(key).map_err(|_| format!("`{key}` is not a game version name"))?;
        }
        if self.includes.build && self.game_build.is_none() {
            return Err("it holds a game build but names none".into());
        }
        Ok(())
    }
}
