//! `<profile>/profile.toml`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::builds::BuildKey;
use crate::error::ProfileError;

/// Highest schema this build reads. A file is written as the lowest schema that holds
/// what it has (see `ProfileFile::to_write`), so an older rwpm can still read it.
pub const SCHEMA: u32 = 3;
/// Schema 2 added `game_build`.
pub const SCHEMA_PINNED: u32 = 2;
/// The schema a profile with no pinned game build and no pack origin is written as.
pub const SCHEMA_UNPINNED: u32 = 1;
pub const FILE_NAME: &str = "profile.toml";
/// The `.rimpack` a profile was made from, kept beside `profile.toml`.
pub const PACK_FILE: &str = "pack.rimpack";

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

/// What a profile keeps of the pack that made it. The pack file (`PACK_FILE`) and the cover
/// thumbnail (`modpack::cover::PROFILE_COVER`) sit beside `profile.toml` under fixed names,
/// so nothing here is a path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackOrigin {
    pub name: String,
    /// Markdown source, as the pack creator wrote it.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub has_cover: bool,
    pub imported_at: toml::value::Datetime,
    /// The player answered "Not now" to "take this offline?", so it is not asked again.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub declined_offline: bool,
}

impl PackOrigin {
    /// The same limits a manifest holds its text to; a backup can bring in a foreign file.
    fn check(&self) -> Result<(), String> {
        use crate::modpack::file::{MAX_DESCRIPTION_CHARS, MAX_NAME_CHARS, check_text};
        if self.name.trim().is_empty() {
            return Err("`pack.name` is empty".into());
        }
        check_text(&self.name, "`pack.name`", MAX_NAME_CHARS, false)?;
        check_text(
            &self.description,
            "`pack.description`",
            MAX_DESCRIPTION_CHARS,
            true,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileFile {
    pub schema: u32,
    pub name: String,
    pub mode: Mode,
    /// Key of the stored game build this profile is pinned to; absent = the Steam install.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub game_build: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub frozen_mods: Vec<FrozenMod>,
    /// Set when a `.rimpack` made this profile. Needs schema 3.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pack: Option<PackOrigin>,
}

impl ProfileFile {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            schema: SCHEMA_UNPINNED,
            name: name.into(),
            mode: Mode::Live,
            game_build: None,
            frozen_mods: Vec::new(),
            pack: None,
        }
    }

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
        let file: Self = table
            .try_into()
            .map_err(|e: toml::de::Error| parse_err(e.to_string()))?;
        if let Some(key) = &file.game_build {
            BuildKey::parse(key).map_err(|_| {
                parse_err(format!("`game_build` {key:?} is not a game version key"))
            })?;
        }
        if let Some(origin) = &file.pack {
            origin.check().map_err(parse_err)?;
        }
        Ok(file)
    }

    /// The lowest schema that holds what the file has: 3 with a pack origin, 2 with a
    /// pinned game build, else 1. Recomputed on every write, so dropping the pin or the
    /// origin takes the file back down instead of stranding it.
    fn to_write(&self) -> Self {
        let mut out = self.clone();
        out.schema = if out.pack.is_some() {
            SCHEMA
        } else if out.game_build.is_some() {
            SCHEMA_PINNED
        } else {
            SCHEMA_UNPINNED
        };
        out
    }

    /// Writes via a sibling temp file + rename, so readers never see a partial file.
    pub fn write_atomic(&self, path: &Path) -> Result<(), ProfileError> {
        let text = toml::to_string_pretty(&self.to_write()).map_err(|e| ProfileError::Parse {
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
            schema: SCHEMA_UNPINNED,
            name: "My Modded Run".into(),
            mode: Mode::Offline,
            game_build: None,
            frozen_mods: vec![FrozenMod {
                folder: "2009463077".into(),
                package_id: "brrainz.harmony".into(),
                source: ModSource::Workshop,
                source_dir: PathBuf::from("/steam/workshop/content/294100/2009463077"),
                copied_at: "2026-10-11T17:00:00Z".parse().expect("datetime"),
            }],
            pack: None,
        }
    }

    fn origin() -> PackOrigin {
        PackOrigin {
            name: "Ironman".into(),
            description: "# Hard mode\n\n- no saves\n- no mercy".into(),
            has_cover: true,
            imported_at: "2026-10-21T12:00:00Z".parse().expect("datetime"),
            declined_offline: false,
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
            "schema = 4\nname = \"x\"\nmode = \"live\"\nfuture = true\n",
        )
        .expect("write");
        assert!(matches!(
            ProfileFile::read(&path),
            Err(ProfileError::SchemaTooNew { found: 4, .. })
        ));
    }

    /// 3 while a pack origin is set; without one the file stays readable by older builds.
    #[test]
    fn schema_follows_the_pack_origin_in_both_directions() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(FILE_NAME);
        let mut file = ProfileFile::new("Run");
        file.pack = Some(origin());
        file.write_atomic(&path).expect("write");
        let read = ProfileFile::read(&path).expect("read");
        assert_eq!(read.schema, 3);
        assert_eq!(read.pack, Some(origin()));

        file.game_build = Some("1.6.4871-rev598".into());
        file.write_atomic(&path).expect("write");
        assert_eq!(ProfileFile::read(&path).expect("read").schema, 3);

        file.pack = None;
        file.write_atomic(&path).expect("write");
        assert_eq!(ProfileFile::read(&path).expect("read").schema, 2);
        file.game_build = None;
        file.write_atomic(&path).expect("write");
        let text = std::fs::read_to_string(&path).expect("read");
        assert!(text.contains("schema = 1") && !text.contains("pack"), "{text}");
    }

    /// An origin read from a backup is as untrusted as a pack manifest.
    #[test]
    fn rejects_hostile_pack_text() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(FILE_NAME);
        let long = "x".repeat(2_001);
        let cases = [
            ("name = \"\"", ""),
            ("name = \"a\\u0007b\"", ""),
            ("name = \"Ok\"", &format!("description = {long:?}")),
            ("name = \"Ok\"", "description = \"a\\u202Eb\""),
        ];
        for (name, extra) in cases {
            let text = format!(
                "schema = 3\nname = \"x\"\nmode = \"live\"\n[pack]\n{name}\n{extra}\n\
                 imported_at = 2026-10-21T12:00:00Z\n"
            );
            std::fs::write(&path, text).expect("write");
            let out = ProfileFile::read(&path);
            assert!(
                matches!(out, Err(ProfileError::Parse { .. })),
                "{name} {extra:.20}: {out:?}"
            );
        }
    }

    /// 2 while pinned, back to 1 when the pin goes.
    #[test]
    fn schema_follows_the_pin_in_both_directions() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(FILE_NAME);
        let mut file = ProfileFile::new("Run");
        file.write_atomic(&path).expect("write");
        assert_eq!(ProfileFile::read(&path).expect("read").schema, 1);

        file.game_build = Some("1.6.4871-rev598".into());
        file.write_atomic(&path).expect("write");
        let read = ProfileFile::read(&path).expect("read");
        assert_eq!(read.schema, 2);
        assert_eq!(read.game_build.as_deref(), Some("1.6.4871-rev598"));

        file.game_build = None;
        file.write_atomic(&path).expect("write");
        assert_eq!(ProfileFile::read(&path).expect("read").schema, 1);
        let text = std::fs::read_to_string(&path).expect("read");
        assert!(!text.contains("game_build"), "{text}");
    }

    /// A schema-1 file written before pins existed still reads.
    #[test]
    fn reads_a_schema_one_file_without_a_pin() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(FILE_NAME);
        std::fs::write(&path, "schema = 1\nname = \"Old\"\nmode = \"live\"\n").expect("w");
        let read = ProfileFile::read(&path).expect("read");
        assert_eq!(read.game_build, None);
        assert_eq!(read.schema, 1);
    }

    /// `game_build` names a stored build, so it must be a build key. A path
    /// such as `../x` would otherwise point the game lookup outside the build store.
    #[test]
    fn rejects_a_game_build_that_is_not_a_build_key() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(FILE_NAME);
        for bad in ["../x", "", "1.6 rev598", "UPPER", "-1.6"] {
            let text = format!("schema = 2\nname = \"x\"\nmode = \"live\"\ngame_build = {bad:?}\n");
            std::fs::write(&path, text).expect("write");
            let out = ProfileFile::read(&path);
            assert!(
                matches!(out, Err(ProfileError::Parse { .. })),
                "{bad:?}: {out:?}"
            );
        }
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
