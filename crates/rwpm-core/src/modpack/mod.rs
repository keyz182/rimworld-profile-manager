//! `.rimpack` modpacks (§I file `.rimpack`, C.58b): a zstd-compressed tar whose first entry
//! is `rimpack.toml`. A pack holds no mod files and no game files, only the load order
//! (`ModsConfig.xml`), the chosen settings, creations and saves, and the list of mods.
//!
//! This module holds the format and the two steps that use it. `pack` writes a pack from a
//! profile; `unpack` makes a NEW profile from one. Fetching the listed mods is separate.

pub mod entry;
pub mod file;
pub mod gather;
pub mod pack;
pub mod unpack;

use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

pub use file::{Includes, Manifest, ModEntry};
pub use gather::{Gathered, gather_mods};
pub use pack::{PackPlan, PackReport, PackSpec, apply_pack, plan_pack};
pub use unpack::{UnpackPlan, UnpackReport, apply_unpack, plan_unpack};

use crate::error::ProfileError;

/// A manifest listing thousands of mods is still far below this; a bigger first entry is
/// not a real manifest.
const MAX_MANIFEST_BYTES: u64 = 4 * 1024 * 1024;
/// Refused outright, whatever a manifest claims. Saves are the only large part of a pack.
pub(crate) const MAX_BYTES: u64 = 8 << 30;
pub(crate) const MAX_ENTRIES: u64 = 200_000;

type Reader = tar::Archive<zstd::Decoder<'static, BufReader<File>>>;

pub(crate) fn bad(path: &Path, reason: impl Into<String>) -> ProfileError {
    ProfileError::BadModpack {
        path: path.to_path_buf(),
        reason: reason.into(),
    }
}

/// Opens a pack as a tar stream. A file that is not zstd data fails on the first read.
pub(crate) fn open(path: &Path) -> Result<Reader, ProfileError> {
    let file = File::open(path).map_err(ProfileError::io("open", path))?;
    let decoder = zstd::Decoder::new(file).map_err(ProfileError::io("open", path))?;
    Ok(tar::Archive::new(decoder))
}

/// Reads the text of the first entry, which must be `rimpack.toml`.
pub(crate) fn read_first<R: Read>(
    first: &mut tar::Entry<'_, R>,
    path: &Path,
) -> Result<String, ProfileError> {
    let decode = |e: std::io::Error| bad(path, format!("it cannot be read as a modpack ({e})"));
    let name = first.path().map_err(decode)?.into_owned();
    if entry::classify(&name) != Ok(entry::Place::Manifest) {
        let want = file::FILE_NAME;
        return Err(bad(
            path,
            format!("its first entry is `{}`, not `{want}`", name.display()),
        ));
    }
    entry::check_entry_kind(first.header().entry_type(), &name).map_err(|r| bad(path, r))?;
    if first.size() > MAX_MANIFEST_BYTES {
        return Err(bad(path, format!("`{}` is too big", file::FILE_NAME)));
    }
    let mut text = String::new();
    first.read_to_string(&mut text).map_err(decode)?;
    Ok(text)
}

/// Reads only the first entry, so a preview never unpacks anything.
pub fn read_manifest(path: &Path) -> Result<Manifest, ProfileError> {
    let decode = |e: std::io::Error| bad(path, format!("it cannot be read as a modpack ({e})"));
    let mut archive = open(path)?;
    let mut entries = archive.entries().map_err(decode)?;
    let mut first = entries
        .next()
        .ok_or_else(|| bad(path, "it is empty"))?
        .map_err(decode)?;
    let text = read_first(&mut first, path)?;
    Manifest::parse(&text, path)
}

#[cfg(test)]
mod gather_tests;
#[cfg(test)]
mod tests;
