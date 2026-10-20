//! `<app data>/settings.toml` (§I): app-wide settings: folders and first-run state.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::ProfileError;

pub const FILE_NAME: &str = "settings.toml";
/// Highest schema this build reads and the one it writes.
pub const SCHEMA: u32 = 1;
const DEFAULT_PROFILES: &str = "profiles";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppSettings {
    pub schema: u32,
    /// Absent: `<app data>/profiles`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profiles_root: Option<PathBuf>,
    /// The RimWorld folder, when the player picked one (else found through Steam).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub game_dir: Option<PathBuf>,
    /// The Steam folder, when automatic detection picks the wrong one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub steam_dir: Option<PathBuf>,
    /// First-run setup is finished (or was not needed).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub setup_done: bool,
    /// SHA-256 (lower-case hex) of the steamcmd archive the first time it was downloaded
    /// (T82). Later downloads must match it. Kept here, not beside the unpacked files, so
    /// that deleting those does not forget it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub steamcmd_sha256: Option<String>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            schema: SCHEMA,
            profiles_root: None,
            game_dir: None,
            steam_dir: None,
            setup_done: false,
            steamcmd_sha256: None,
        }
    }
}

impl AppSettings {
    /// Reads `<app_dir>/settings.toml`; a missing file means defaults.
    pub fn load(app_dir: &Path) -> Result<Self, ProfileError> {
        let path = app_dir.join(FILE_NAME);
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(ProfileError::io("read", &path)(e)),
        };
        let parse_err = |message: String| ProfileError::Parse {
            path: path.clone(),
            message,
        };
        let table: toml::Table = toml::from_str(&text).map_err(|e| parse_err(e.to_string()))?;
        match table
            .get("schema")
            .and_then(toml::Value::as_integer)
            .map(u32::try_from)
        {
            Some(Ok(found)) if found > SCHEMA => Err(ProfileError::SchemaTooNew {
                path: path.clone(),
                found,
                supported: SCHEMA,
            }),
            Some(Ok(_)) => table
                .try_into()
                .map_err(|e: toml::de::Error| parse_err(e.to_string())),
            _ => Err(parse_err("missing or invalid `schema`".into())),
        }
    }

    /// Writes via a temp file + rename.
    pub fn save(&self, app_dir: &Path) -> Result<(), ProfileError> {
        let path = app_dir.join(FILE_NAME);
        let text = toml::to_string_pretty(self).map_err(|e| ProfileError::Parse {
            path: path.clone(),
            message: e.to_string(),
        })?;
        std::fs::create_dir_all(app_dir).map_err(ProfileError::io("create", app_dir))?;
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, text).map_err(ProfileError::io("write", &tmp))?;
        std::fs::rename(&tmp, &path).map_err(ProfileError::io("replace", &path))
    }

    /// The game install: the picked folder if set, else discovered through Steam.
    pub fn game(&self) -> Result<crate::discovery::GameInstall, crate::discovery::DiscoveryError> {
        match &self.game_dir {
            Some(dir) => crate::discovery::from_game_dir(dir),
            None => crate::discovery::discover(self.steam_dir.as_deref()),
        }
    }

    /// The profiles folder in use.
    pub fn profiles_root(&self, app_dir: &Path) -> PathBuf {
        self.profiles_root
            .clone()
            .unwrap_or_else(|| app_dir.join(DEFAULT_PROFILES))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_uses_default() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let s = AppSettings::load(tmp.path()).expect("load");
        assert_eq!(s, AppSettings::default());
        assert_eq!(s.profiles_root(tmp.path()), tmp.path().join("profiles"));
    }

    #[test]
    fn roundtrip_and_newer_schema_refused() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let s = AppSettings {
            schema: SCHEMA,
            profiles_root: Some(PathBuf::from("/games/profiles")),
            game_dir: Some(PathBuf::from("/games/RimWorld")),
            steam_dir: None,
            setup_done: true,
            steamcmd_sha256: Some("ab".repeat(32)),
        };
        s.save(tmp.path()).expect("save");
        let text = std::fs::read_to_string(tmp.path().join(FILE_NAME)).expect("read");
        assert!(text.contains("setup_done = true"), "{text}");
        let back = AppSettings::load(tmp.path()).expect("load");
        assert_eq!(back, s);
        assert_eq!(
            back.profiles_root(tmp.path()),
            PathBuf::from("/games/profiles")
        );
        std::fs::write(tmp.path().join(FILE_NAME), "schema = 2\nfuture = 1\n").expect("w");
        assert!(matches!(
            AppSettings::load(tmp.path()),
            Err(ProfileError::SchemaTooNew { found: 2, .. })
        ));
        std::fs::write(tmp.path().join(FILE_NAME), "schema = 1\nextra = 1\n").expect("w");
        assert!(matches!(
            AppSettings::load(tmp.path()),
            Err(ProfileError::Parse { .. })
        ));
    }
}
