//! What the Share dialog remembers between packs: the creator(s) last typed.
//!
//! Kept in `<app data>/share.toml`, apart from `settings.toml`, so an older build that cannot
//! read it never loses the folder settings. A missing or unreadable file is not an error:
//! the box just starts empty.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::ProfileError;

pub const FILE_NAME: &str = "share.toml";
/// Highest schema this build reads and the one it writes.
pub const SCHEMA: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShareDefaults {
    pub schema: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub authors: Vec<String>,
}

impl Default for ShareDefaults {
    fn default() -> Self {
        Self {
            schema: SCHEMA,
            authors: Vec::new(),
        }
    }
}

impl ShareDefaults {
    /// Reads `<app_dir>/share.toml`. Remembering is a convenience, so a missing, newer or
    /// damaged file gives the empty default and never blocks sharing.
    pub fn load(app_dir: &Path) -> Self {
        std::fs::read_to_string(app_dir.join(FILE_NAME))
            .ok()
            .and_then(|text| toml::from_str::<Self>(&text).ok())
            .filter(|found| found.schema <= SCHEMA)
            .unwrap_or_default()
    }

    /// Writes via a temp file + rename.
    pub fn save(&self, app_dir: &Path) -> Result<(), ProfileError> {
        let path = app_dir.join(FILE_NAME);
        let text = toml::to_string_pretty(&Self {
            schema: SCHEMA,
            ..self.clone()
        })
        .map_err(|e| ProfileError::Parse {
            path: path.clone(),
            message: e.to_string(),
        })?;
        std::fs::create_dir_all(app_dir).map_err(ProfileError::io("create", app_dir))?;
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, text).map_err(ProfileError::io("write", &tmp))?;
        std::fs::rename(&tmp, &path).map_err(ProfileError::io("replace", &path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(of: &[&str]) -> Vec<String> {
        of.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn a_missing_file_gives_no_creators() {
        let tmp = tempfile::tempdir().expect("tempdir");
        assert_eq!(ShareDefaults::load(tmp.path()), ShareDefaults::default());
    }

    #[test]
    fn creators_survive_a_save_and_load() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let saved = ShareDefaults {
            schema: SCHEMA,
            authors: names(&["Ann", "Bob"]),
        };
        saved.save(tmp.path()).expect("save");
        assert_eq!(ShareDefaults::load(tmp.path()), saved);
    }

    #[test]
    fn a_damaged_or_newer_file_gives_no_creators() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join(FILE_NAME);
        std::fs::write(&path, "not [ toml").expect("write");
        assert_eq!(ShareDefaults::load(tmp.path()), ShareDefaults::default());
        std::fs::write(&path, "schema = 2\nauthors = [\"Ann\"]\n").expect("write");
        assert_eq!(ShareDefaults::load(tmp.path()), ShareDefaults::default());
    }

    #[test]
    fn an_empty_list_is_not_written() {
        let tmp = tempfile::tempdir().expect("tempdir");
        ShareDefaults::default().save(tmp.path()).expect("save");
        let text = std::fs::read_to_string(tmp.path().join(FILE_NAME)).expect("read");
        assert!(!text.contains("authors"), "{text}");
    }
}
