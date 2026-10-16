//! `.rimrim` backups of OFFLINE profiles (§I file `.rimrim`, V73-V84): a zstd-compressed
//! tar whose first entry is `rimrim.toml`.
//!
//! This module holds the format: the manifest, the rules for entry names and manifest
//! strings, and the manifest reader. `pack` writes a backup; restore builds on the rest.

pub mod entry;
pub mod file;
pub mod pack;
pub mod unpack;

use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

pub use entry::{Place, check_folder, check_kind, check_link_target, classify};
pub use file::{Includes, Manifest};
pub use pack::{BackupPlan, BackupReport, apply_backup, plan_backup};
pub use unpack::{RestoreNote, RestorePlan, RestoreReport, apply_restore, plan_restore};

use crate::error::ProfileError;

/// V80, mods part. Shown before every backup and restore, and stored in `rimrim.toml`.
pub const NOTICE_MODS: &str =
    "Check each mod's licence or ask its author before you share this backup.";
/// V80, game part. Only a backup that holds the game build carries it as a warning to act on.
pub const NOTICE_GAME: &str = "This backup holds RimWorld's game files. Do not share it.";

/// `rimrim.toml` is a few hundred bytes; a bigger first entry is not a real manifest.
const MAX_MANIFEST_BYTES: u64 = 64 * 1024;

type Reader = tar::Archive<zstd::Decoder<'static, BufReader<File>>>;

pub(crate) fn bad(path: &Path, reason: impl Into<String>) -> ProfileError {
    ProfileError::BadBackup {
        path: path.to_path_buf(),
        reason: reason.into(),
    }
}

/// Opens a backup as a tar stream. A file that is not zstd data fails on the first read.
pub(crate) fn open(path: &Path) -> Result<Reader, ProfileError> {
    let file = File::open(path).map_err(ProfileError::io("open", path))?;
    let decoder = zstd::Decoder::new(file).map_err(ProfileError::io("open", path))?;
    Ok(tar::Archive::new(decoder))
}

/// Reads only the first entry (V82), so a restore preview never unpacks anything.
pub fn read_manifest(path: &Path) -> Result<Manifest, ProfileError> {
    let decode = |e: std::io::Error| bad(path, format!("it cannot be read as a backup ({e})"));
    let mut archive = open(path)?;
    let mut entries = archive.entries().map_err(decode)?;
    let mut first = entries
        .next()
        .ok_or_else(|| bad(path, "it is empty"))?
        .map_err(decode)?;
    let name = first.path().map_err(decode)?.into_owned();
    if classify(&name) != Ok(Place::Manifest) {
        return Err(bad(
            path,
            format!(
                "its first entry is `{}`, not `{}`",
                name.display(),
                file::FILE_NAME
            ),
        ));
    }
    check_kind(first.header().entry_type(), &name, &Place::Manifest)
        .map_err(|reason| bad(path, reason))?;
    if first.size() > MAX_MANIFEST_BYTES {
        return Err(bad(path, format!("`{}` is too big", file::FILE_NAME)));
    }
    let mut text = String::new();
    first.read_to_string(&mut text).map_err(decode)?;
    Manifest::parse(&text, path)
}

#[cfg(test)]
mod tests;
