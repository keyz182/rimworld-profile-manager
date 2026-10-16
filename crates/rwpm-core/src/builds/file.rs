//! `<builds>/<key>/build.toml` (§I.file).
//!
//! The file sits beside the game files, inside the build folder itself: §I.file puts it at
//! `<builds>/<key>/build.toml`, V60 names `<build>/Mods/` as the build's local-mods folder
//! and V54 allows writes to `<build>/Mods/`, which only agree if `<builds>/<key>/` *is* the
//! copied game directory. RimWorld ignores the extra file.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::ProfileError;

/// Highest schema this build reads and the one it writes.
pub const SCHEMA: u32 = 1;
pub const FILE_NAME: &str = "build.toml";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuildFile {
    pub schema: u32,
    /// Raw `Version.txt`, trimmed (e.g. `1.6.4871 rev598`).
    pub version: String,
    /// Steam app manifest `buildid`. The build's identity (V63).
    pub buildid: String,
    /// Steam branch; `""` is the default branch.
    pub beta: String,
    /// Sorted folder names under `Data/`.
    pub dlc: Vec<String>,
    pub captured_at: toml::value::Datetime,
    /// Where the copy came from, for the player's own reference.
    pub source_dir: PathBuf,
    pub size: u64,
}

impl BuildFile {
    /// Reads and validates; a newer schema is refused before field checks run.
    pub fn read(path: &Path) -> Result<Self, ProfileError> {
        let text = std::fs::read_to_string(path).map_err(ProfileError::io("read", path))?;
        Self::parse(&text, path)
    }

    /// The checks of [`Self::read`] on text already in hand; `path` names it in errors.
    pub fn parse(text: &str, path: &Path) -> Result<Self, ProfileError> {
        let parse_err = |message: String| ProfileError::Parse {
            path: path.to_path_buf(),
            message,
        };
        let table: toml::Table = toml::from_str(text).map_err(|e| parse_err(e.to_string()))?;
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

/// Sorted names of the folders directly in the game's `Data` dir (§I.file). Takes the
/// `Data` dir itself, because it sits inside the bundle on macOS (`GameInstall::data_dir`).
///
/// Folders only: a macOS install also keeps a plain `Place official expansions here.txt`
/// there, which is not an expansion.
pub fn dlc_folders(data_dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(data_dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| e.file_name().to_str().map(ToOwned::to_owned))
        .collect();
    names.sort();
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> BuildFile {
        BuildFile {
            schema: SCHEMA,
            version: "1.6.4871 rev598".into(),
            buildid: "19234567".into(),
            beta: String::new(),
            dlc: vec!["Core".into(), "Royalty".into()],
            captured_at: "2026-10-13T12:00:00Z".parse().expect("datetime"),
            source_dir: PathBuf::from("/steam/steamapps/common/RimWorld"),
            size: 1_773_456_789,
        }
    }

    #[test]
    fn toml_roundtrip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(FILE_NAME);
        sample().write_atomic(&path).expect("write");
        assert_eq!(BuildFile::read(&path).expect("read"), sample());
    }

    #[test]
    fn rejects_newer_schema() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(FILE_NAME);
        std::fs::write(&path, "schema = 2\nversion = \"x\"\nlater = true\n").expect("write");
        assert!(matches!(
            BuildFile::read(&path),
            Err(ProfileError::SchemaTooNew { found: 2, .. })
        ));
    }

    #[test]
    fn rejects_unknown_fields() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(FILE_NAME);
        let mut text = toml::to_string_pretty(&sample()).expect("toml");
        text.push_str("\nbiuldid = \"typo\"\n");
        std::fs::write(&path, text).expect("write");
        assert!(matches!(
            BuildFile::read(&path),
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

    /// §I.file: folder names. A macOS install keeps a stray text file in `Data/`.
    #[test]
    fn dlc_lists_sorted_folders_only() {
        let dir = tempfile::tempdir().expect("tempdir");
        for name in ["Royalty", "Core", "Anomaly", "Odyssey"] {
            std::fs::create_dir_all(dir.path().join("Data").join(name)).expect("dir");
        }
        std::fs::write(
            dir.path().join("Data/Place official expansions here.txt"),
            "x",
        )
        .expect("file");
        assert_eq!(
            dlc_folders(&dir.path().join("Data")),
            ["Anomaly", "Core", "Odyssey", "Royalty"]
        );
        assert!(dlc_folders(&dir.path().join("missing")).is_empty());
    }
}
