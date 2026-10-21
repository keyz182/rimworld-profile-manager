//! Restore: a `.rimpack` → a NEW profile.
//!
//! The archive is untrusted. Every entry name, kind and size is checked against the
//! manifest before anything is written, files are created new (a repeated name fails), and
//! the profile is built like `backup::unpack` builds one: into a freshly claimed folder,
//! `profile.toml` last, the folder removed on any error. The archived `ModsConfig.xml` is
//! extracted as it is; restore never writes a load order of its own.

use std::collections::BTreeSet;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use tar::EntryType;

use super::cover;
use super::entry::{Place, SAVE_DATA, check_declared, check_entry_kind, classify};
use super::file::Manifest;
use super::{MAX_BYTES, MAX_ENTRIES, bad, open, read_first, read_preview};
use crate::backup::unpack::{parts_of, write_entry};
use crate::error::ProfileError;
use crate::profile::file::{FILE_NAME as PROFILE_FILE, PACK_FILE};
use crate::profile::offline::now_datetime;
use crate::profile::{PackOrigin, Profile, ProfileFile, ProfileId, ProfileStore, Trash};
use crate::space;

/// Free space wanted on top of the declared size.
const MARGIN_PERCENT: u64 = 10;

/// What a restore would do, for the Open dialog.
#[derive(Debug, Clone)]
pub struct UnpackPlan {
    pub archive: PathBuf,
    pub manifest: Manifest,
    /// The cover as a thumbnail PNG, when the pack has one.
    pub cover: Option<Vec<u8>>,
    /// Display name of the new profile; the player may change it before `apply_unpack`.
    pub name: String,
    /// Disk the restore needs, margin included.
    pub bytes_needed: u64,
    pub free_bytes: Option<u64>,
}

#[derive(Debug)]
pub struct UnpackReport {
    pub profile: Profile,
}

pub fn plan_unpack<T: Trash>(
    archive: &Path,
    store: &ProfileStore<T>,
) -> Result<UnpackPlan, ProfileError> {
    let (manifest, cover) = read_preview(archive, true)?;
    if manifest.uncompressed_bytes > MAX_BYTES || manifest.entry_count > MAX_ENTRIES {
        return Err(bad(archive, "it is larger than a modpack may be"));
    }
    // The pack itself is kept too (a copy, though a reflink may cost nothing).
    let kept = std::fs::metadata(archive).map_or(0, |m| m.len());
    let need =
        manifest.uncompressed_bytes + manifest.uncompressed_bytes / 100 * MARGIN_PERCENT + kept;
    let free = space::free_space(store.root());
    if let Some(free) = free.filter(|&f| f < need) {
        return Err(ProfileError::NotEnoughSpace {
            path: store.root().to_path_buf(),
            need,
            free,
        });
    }
    Ok(UnpackPlan {
        archive: archive.to_path_buf(),
        name: manifest.name.clone(),
        manifest,
        cover,
        bytes_needed: need,
        free_bytes: free,
    })
}

/// Restores into a new LIVE profile. On any error the new profile folder is removed.
pub fn apply_unpack<T: Trash>(
    plan: &UnpackPlan,
    store: &ProfileStore<T>,
    progress: &mut dyn FnMut(u64),
) -> Result<UnpackReport, ProfileError> {
    let name = plan.name.trim();
    if name.is_empty() {
        return Err(ProfileError::EmptyName);
    }
    let (id, dir) = store.claim(&ProfileId::slug_base(name))?;
    let mut job = Job {
        plan,
        dir: &dir,
        progress,
        bytes: 0,
        entries: 0,
        mod_list: false,
        cover: false,
        write_saves: true,
        saves: BTreeSet::new(),
    };
    let done = job
        .run()
        .and_then(|()| job.finish(name))
        .and_then(|()| store.get(&id));
    match done {
        Ok(profile) => Ok(UnpackReport { profile }),
        Err(e) => {
            let _ = std::fs::remove_dir_all(&dir);
            Err(e)
        }
    }
}

struct Job<'a> {
    plan: &'a UnpackPlan,
    dir: &'a Path,
    progress: &'a mut dyn FnMut(u64),
    bytes: u64,
    entries: u64,
    mod_list: bool,
    cover: bool,
    /// Off for a reset that leaves the player's saves alone: saves are still counted and
    /// checked, but not written.
    write_saves: bool,
    /// Save file names seen, to hold the manifest to what it lists.
    saves: BTreeSet<String>,
}

impl Job<'_> {
    fn archive(&self) -> &Path {
        &self.plan.archive
    }

    fn fault(&self, reason: impl Into<String>) -> ProfileError {
        bad(self.archive(), reason)
    }

    fn run(&mut self) -> Result<(), ProfileError> {
        let archive = self.archive().to_path_buf();
        let decode = |e: io::Error| bad(&archive, format!("it cannot be read as a modpack ({e})"));
        let mut reader = open(&archive)?;
        let mut entries = reader.entries().map_err(decode)?;
        let mut first = entries
            .next()
            .ok_or_else(|| self.fault("it is empty"))?
            .map_err(decode)?;
        let text = read_first(&mut first, &archive)?;
        if Manifest::parse(&text, &archive)? != self.plan.manifest {
            return Err(self.fault("it changed after it was opened"));
        }
        for entry in entries {
            self.entry(entry.map_err(decode)?)?;
            (self.progress)(self.bytes);
        }
        Ok(())
    }

    fn entry<R: Read>(&mut self, mut entry: tar::Entry<'_, R>) -> Result<(), ProfileError> {
        self.entries += 1;
        if self.entries > self.plan.manifest.entry_count {
            return Err(self.fault("it holds more entries than it says"));
        }
        let name = entry
            .path()
            .map_err(ProfileError::io("read", self.archive()))?
            .into_owned();
        let place = classify(&name).map_err(|r| self.fault(r))?;
        let kind = entry.header().entry_type();
        check_entry_kind(kind, &name).map_err(|r| self.fault(r))?;
        self.bytes += entry.size();
        if self.bytes > self.plan.manifest.uncompressed_bytes {
            return Err(self.fault("it holds more data than it says"));
        }
        let rel = match place {
            Place::Manifest => return Err(self.fault("it has a second `rimpack.toml`")),
            Place::Cover => return self.cover(&mut entry),
            Place::SaveData(rel) => rel,
        };
        check_declared(&rel, &self.plan.manifest.includes).map_err(|r| self.fault(r))?;
        if self.note(&rel) && !self.write_saves {
            return Ok(());
        }
        let dest = self.dir.join(SAVE_DATA).join(&rel);
        // Files are created new, so a repeated name fails instead of overwriting.
        write_entry(&dest, EntryType::Regular, &mut entry).map_err(|e| match &e {
            ProfileError::Io { source, .. } if source.kind() == io::ErrorKind::AlreadyExists => {
                self.fault(format!("it holds `{}` twice", name.display()))
            }
            _ => e,
        })
    }

    /// The cover is decoded under the cover limits and only its thumbnail is kept.
    fn cover<R: Read>(&mut self, entry: &mut tar::Entry<'_, R>) -> Result<(), ProfileError> {
        if self.plan.manifest.cover.is_none() || self.cover {
            return Err(self.fault("it holds a cover it does not declare"));
        }
        self.cover = true;
        let mut bytes = Vec::new();
        entry
            .take(cover::MAX_COVER_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(ProfileError::io("read", self.archive()))?;
        let thumb = cover::thumbnail(&bytes)
            .map_err(|r| self.fault(format!("its cover is refused ({r})")))?;
        let dest = self.dir.join(cover::PROFILE_COVER);
        std::fs::write(&dest, thumb).map_err(ProfileError::io("write", &dest))
    }

    /// Records what `rel` is; true when it is a save file.
    fn note(&mut self, rel: &Path) -> bool {
        match parts_of(rel).as_slice() {
            ["Config", "ModsConfig.xml"] => self.mod_list = true,
            ["Saves", save] => {
                self.saves.insert((*save).to_owned());
                return true;
            }
            _ => {}
        }
        false
    }

    /// After the stream: the totals and the listed saves must match.
    fn verify(&self) -> Result<(), ProfileError> {
        let manifest = &self.plan.manifest;
        if (self.entries, self.bytes) != (manifest.entry_count, manifest.uncompressed_bytes) {
            return Err(self.fault("it holds less than it says"));
        }
        if !self.mod_list {
            return Err(self.fault("it has no mod list"));
        }
        if manifest.cover.is_some() && !self.cover {
            return Err(self.fault("its cover is missing"));
        }
        let listed: BTreeSet<String> = manifest.includes.saves.iter().cloned().collect();
        if self.saves != listed {
            return Err(self.fault("its saves are not the ones it lists"));
        }
        Ok(())
    }

    /// `verify`, then the pack is kept and `profile.toml` is written.
    fn finish(&mut self, name: &str) -> Result<(), ProfileError> {
        self.verify()?;
        let manifest = &self.plan.manifest;
        // The pack stays with the profile, for "reset to pack". Copied here, while the
        // archive is still where the player opened it.
        let kept = self.dir.join(PACK_FILE);
        reflink_copy::reflink_or_copy(self.archive(), &kept)
            .map_err(ProfileError::io("copy", self.archive()))?;
        let mut file = ProfileFile::new(name);
        file.pack = Some(PackOrigin {
            name: manifest.name.clone(),
            description: manifest.description.clone(),
            has_cover: self.cover,
            imported_at: now_datetime(),
            declined_offline: false,
        });
        file.write_atomic(&self.dir.join(PROFILE_FILE))
    }
}

/// Reads `plan.archive` into `dir` (`SaveData/...`, and `cover.png` when it has one) without
/// making a profile of it. Saves are written only when `saves` is set.
pub(crate) fn extract(
    plan: &UnpackPlan,
    dir: &Path,
    saves: bool,
    progress: &mut dyn FnMut(u64),
) -> Result<(), ProfileError> {
    let mut job = Job {
        plan,
        dir,
        progress,
        bytes: 0,
        entries: 0,
        mod_list: false,
        cover: false,
        write_saves: saves,
        saves: BTreeSet::new(),
    };
    job.run()?;
    job.verify()
}
