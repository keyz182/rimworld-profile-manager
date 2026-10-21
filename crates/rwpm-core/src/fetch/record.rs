//! `<profile>/pack-subscriptions.toml`: the Workshop ids that opening a modpack subscribed this
//! account to, and only those. "Remove subscriptions from the pack"
//! unsubscribes exactly these, so a mod the player already had is never touched.

use std::collections::BTreeSet;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::ProfileError;

pub const FILE_NAME: &str = "pack-subscriptions.toml";
const SCHEMA: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    schema: u32,
    ids: Vec<u64>,
}

/// The recorded ids, sorted. A missing, unreadable or newer file reads as empty: the record
/// only ever adds a safety net, so a bad one must not stop anything.
pub fn read(profile_dir: &Path) -> Vec<u64> {
    let Ok(text) = std::fs::read_to_string(profile_dir.join(FILE_NAME)) else {
        return Vec::new();
    };
    match toml::from_str::<File>(&text) {
        Ok(file) if file.schema <= SCHEMA => {
            let set: BTreeSet<u64> = file.ids.into_iter().collect();
            set.into_iter().collect()
        }
        _ => Vec::new(),
    }
}

fn write(profile_dir: &Path, ids: &BTreeSet<u64>) -> Result<(), ProfileError> {
    let path = profile_dir.join(FILE_NAME);
    if ids.is_empty() {
        return match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(ProfileError::io("remove", &path)(e)),
        };
    }
    let file = File {
        schema: SCHEMA,
        ids: ids.iter().copied().collect(),
    };
    let text = toml::to_string_pretty(&file).map_err(|e| ProfileError::Parse {
        path: path.clone(),
        message: e.to_string(),
    })?;
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, text).map_err(ProfileError::io("write", &tmp))?;
    std::fs::rename(&tmp, &path).map_err(ProfileError::io("save", &path))
}

/// Adds `ids` to the record.
pub fn add(profile_dir: &Path, ids: &[u64]) -> Result<(), ProfileError> {
    let mut all: BTreeSet<u64> = read(profile_dir).into_iter().collect();
    all.extend(ids.iter().copied());
    write(profile_dir, &all)
}

/// Takes `ids` out of the record; the file goes when nothing is left.
pub fn remove(profile_dir: &Path, ids: &[u64]) -> Result<(), ProfileError> {
    let mut all: BTreeSet<u64> = read(profile_dir).into_iter().collect();
    for id in ids {
        all.remove(id);
    }
    write(profile_dir, &all)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_added_once_sorted_and_removed_again() {
        let tmp = tempfile::tempdir().expect("tempdir");
        assert!(read(tmp.path()).is_empty());
        add(tmp.path(), &[30, 10]).expect("add");
        add(tmp.path(), &[20, 10]).expect("add again");
        assert_eq!(read(tmp.path()), [10, 20, 30]);
        remove(tmp.path(), &[20, 99]).expect("remove");
        assert_eq!(read(tmp.path()), [10, 30]);
        remove(tmp.path(), &[10, 30]).expect("remove all");
        assert!(read(tmp.path()).is_empty());
        assert!(
            !tmp.path().join(FILE_NAME).exists(),
            "an empty record is no file"
        );
        remove(tmp.path(), &[1]).expect("nothing to remove is fine");
    }

    #[test]
    fn a_damaged_or_newer_record_reads_as_empty() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join(FILE_NAME);
        for text in [
            "not toml at all",
            "schema = 9\nids = [1]\n",
            "schema = 1\nids = [\"x\"]\n",
            "schema = 1\nids = [1]\nsurprise = 1\n",
        ] {
            std::fs::write(&path, text).expect("write");
            assert!(read(tmp.path()).is_empty(), "{text:?}");
        }
    }
}
