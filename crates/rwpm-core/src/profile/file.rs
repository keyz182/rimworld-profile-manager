//! `<profile>/profile.toml` (§I.file).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::ProfileError;

/// Highest schema this build reads and the one it writes.
pub const SCHEMA: u32 = 1;
pub const FILE_NAME: &str = "profile.toml";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Live,
    Offline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModSource {
    Workshop,
    Local,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrozenMod {
    pub folder: String,
    pub package_id: String,
    pub source: ModSource,
    pub source_dir: PathBuf,
    pub copied_at: toml::value::Datetime,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileFile {
    pub schema: u32,
    pub name: String,
    pub mode: Mode,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub frozen_mods: Vec<FrozenMod>,
}

impl ProfileFile {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            schema: SCHEMA,
            name: name.into(),
            mode: Mode::Live,
            frozen_mods: Vec::new(),
        }
    }

    /// Reads and validates; a newer schema is refused before field checks run.
    pub fn read(path: &Path) -> Result<Self, ProfileError> {
        let text = std::fs::read_to_string(path).map_err(ProfileError::io("read", path))?;
        let parse_err = |message: String| ProfileError::Parse {
            path: path.to_path_buf(),
            message,
        };
        let table: toml::Table = toml::from_str(&text).map_err(|e| parse_err(e.to_string()))?;
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
            _ => return Err(parse_err("missing or invalid `schema`".into())),
        }
        table
            .try_into()
            .map_err(|e: toml::de::Error| parse_err(e.to_string()))
    }

    /// Writes via a sibling temp file + rename, so readers never see a partial file.
    pub fn write_atomic(&self, path: &Path) -> Result<(), ProfileError> {
        let text = toml::to_string_pretty(self).map_err(|e| ProfileError::Parse {
            path: path.to_path_buf(),
            message: e.to_string(),
        })?;
        let tmp = path.with_extension("toml.tmp");
        let write = || -> std::io::Result<()> {
            let mut f = std::fs::File::create(&tmp)?;
            std::io::Write::write_all(&mut f, text.as_bytes())?;
            f.sync_all()
        };
        write().map_err(ProfileError::io("write", &tmp))?;
        std::fs::rename(&tmp, path).map_err(ProfileError::io("replace", path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> ProfileFile {
        ProfileFile {
            schema: SCHEMA,
            name: "My Modded Run".into(),
            mode: Mode::Offline,
            frozen_mods: vec![FrozenMod {
                folder: "2009463077".into(),
                package_id: "brrainz.harmony".into(),
                source: ModSource::Workshop,
                source_dir: PathBuf::from("/steam/workshop/content/294100/2009463077"),
                copied_at: "2026-10-11T17:00:00Z".parse().expect("datetime"),
            }],
        }
    }

    #[test]
    fn toml_roundtrip_with_frozen_mods() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(FILE_NAME);
        sample().write_atomic(&path).expect("write");
        assert_eq!(ProfileFile::read(&path).expect("read"), sample());
    }

    #[test]
    fn rejects_newer_schema() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(FILE_NAME);
        std::fs::write(
            &path,
            "schema = 2\nname = \"x\"\nmode = \"live\"\nfuture = true\n",
        )
        .expect("write");
        assert!(matches!(
            ProfileFile::read(&path),
            Err(ProfileError::SchemaTooNew { found: 2, .. })
        ));
    }

    #[test]
    fn rejects_unknown_fields() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(FILE_NAME);
        std::fs::write(
            &path,
            "schema = 1\nname = \"x\"\nmode = \"live\"\nnmae = \"typo\"\n",
        )
        .expect("write");
        assert!(matches!(
            ProfileFile::read(&path),
            Err(ProfileError::Parse { .. })
        ));
    }

    #[test]
    fn atomic_write_leaves_no_tmp() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(FILE_NAME);
        sample().write_atomic(&path).expect("write");
        sample().write_atomic(&path).expect("overwrite");
        let names: Vec<_> = std::fs::read_dir(dir.path())
            .expect("read_dir")
            .map(|e| e.expect("entry").file_name())
            .collect();
        assert_eq!(names, vec![std::ffi::OsString::from(FILE_NAME)]);
    }
}
