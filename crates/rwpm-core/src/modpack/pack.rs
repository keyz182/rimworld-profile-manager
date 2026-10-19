//! Pack: one profile → one `.rimpack` (C.58b).
//!
//! `plan_pack` shows the creator what a pack would hold. `apply_pack` walks the profile
//! again (so the totals in `rimpack.toml` describe the bytes it is about to write) and
//! writes to a temp file beside the target that is renamed on success.

use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use tar::Header;

use super::entry::{MOD_LIST, SAVE_DATA, check_declared};
use super::file::{self, Includes, Manifest, ModEntry, check_save_name};
use super::{MAX_BYTES, MAX_ENTRIES, bad};
use crate::backup::pack::{check_space, header_for, temp_beside};
use crate::error::ProfileError;
use crate::profile::copy_settings::{Group, entries};
use crate::profile::offline::now_datetime;
use crate::profile::running::RunningProbe;
use crate::profile::{ProfileId, ProfileStore, Trash};

/// Level 3 is zstd's own default speed/size balance.
const ZSTD_LEVEL: i32 = 3;
const SAVES: &str = "Saves";

/// What the creator chose. The mod list is gathered by the caller (see `mods`).
#[derive(Debug, Clone)]
pub struct PackSpec {
    pub description: String,
    pub game_version: Option<String>,
    pub dlc: Vec<String>,
    pub includes: Includes,
    pub start_save: Option<String>,
    pub collection_url: Option<String>,
    pub mods: Vec<ModEntry>,
}

/// What a pack would hold, for the Share dialog. Holds no handle on the profile.
#[derive(Debug, Clone)]
pub struct PackPlan {
    pub id: ProfileId,
    pub name: String,
    /// Bytes of every file that would go in (an upper bound on the archive size).
    pub bytes: u64,
    pub entries: u64,
    /// Files in the chosen groups that a pack may not hold (not plain data, or links).
    pub skipped: Vec<PathBuf>,
    save_data: PathBuf,
    manifest: Manifest,
}

#[derive(Debug)]
pub struct PackReport {
    pub manifest: Manifest,
    pub archive_bytes: u64,
}

pub fn plan_pack<T: Trash>(
    store: &ProfileStore<T>,
    id: &ProfileId,
    spec: PackSpec,
) -> Result<PackPlan, ProfileError> {
    let profile = store.get(id)?;
    let save_data = profile.save_data_dir();
    let inventory = Inventory::of(&save_data, &spec.includes, &profile.name)?;
    let manifest = Manifest {
        schema: file::SCHEMA,
        created_at: now_datetime(),
        rwpm_version: crate::VERSION.to_owned(),
        name: profile.name.clone(),
        description: spec.description,
        game_version: spec.game_version,
        dlc: spec.dlc,
        start_save: spec.start_save,
        collection_url: spec.collection_url,
        uncompressed_bytes: inventory.bytes,
        entry_count: inventory.items.len() as u64,
        includes: spec.includes,
        mods: spec.mods,
    };
    check_manifest(&manifest)?;
    Ok(PackPlan {
        id: profile.id,
        name: profile.name,
        bytes: inventory.bytes,
        entries: manifest.entry_count,
        skipped: inventory.skipped,
        save_data,
        manifest,
    })
}

fn check_manifest(manifest: &Manifest) -> Result<(), ProfileError> {
    let here = Path::new(file::FILE_NAME);
    manifest.check().map_err(|reason| bad(here, reason))?;
    if manifest.uncompressed_bytes > MAX_BYTES || manifest.entry_count > MAX_ENTRIES {
        return Err(bad(here, "it would be larger than a modpack may be"));
    }
    Ok(())
}

/// Writes the pack to `dest`. Refused while a game runs. Nothing is left at `dest` or
/// beside it on failure; an existing `dest` is replaced only on success.
pub fn apply_pack(
    plan: &PackPlan,
    dest: &Path,
    probe: &dyn RunningProbe,
    progress: &mut dyn FnMut(u64),
) -> Result<PackReport, ProfileError> {
    if probe.any_game() {
        return Err(ProfileError::GameRunning);
    }
    let inventory = Inventory::of(&plan.save_data, &plan.manifest.includes, &plan.name)?;
    check_space(dest, inventory.bytes)?;
    let mut manifest = plan.manifest.clone();
    manifest.created_at = now_datetime();
    manifest.uncompressed_bytes = inventory.bytes;
    manifest.entry_count = inventory.items.len() as u64;
    check_manifest(&manifest)?;
    let tmp = temp_beside(dest)?;
    let written = write_archive(&tmp, &manifest, &inventory, progress)
        .and_then(|()| std::fs::rename(&tmp, dest).map_err(ProfileError::io("save", dest)));
    if let Err(e) = written {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(PackReport {
        archive_bytes: std::fs::metadata(dest).map_or(0, |m| m.len()),
        manifest,
    })
}

fn write_archive(
    tmp: &Path,
    manifest: &Manifest,
    inventory: &Inventory,
    progress: &mut dyn FnMut(u64),
) -> Result<(), ProfileError> {
    let file = File::create(tmp).map_err(ProfileError::io("create", tmp))?;
    let encoder = zstd::Encoder::new(file, ZSTD_LEVEL).map_err(ProfileError::io("write", tmp))?;
    let mut tarball = tar::Builder::new(encoder);
    let text = manifest.to_toml()?;
    let mut header = Header::new_gnu();
    header.set_size(text.len() as u64);
    header.set_mode(0o644);
    tarball
        .append_data(&mut header, file::FILE_NAME, text.as_bytes())
        .map_err(ProfileError::io("write", tmp))?;
    let mut done = 0;
    for item in &inventory.items {
        done += append_file(&mut tarball, item).map_err(ProfileError::io("pack", &item.source))?;
        progress(done);
    }
    if done != inventory.bytes {
        let changed = io::Error::new(
            io::ErrorKind::InvalidData,
            "a file changed while it was packed",
        );
        return Err(ProfileError::io("pack", tmp)(changed));
    }
    let encoder = tarball
        .into_inner()
        .map_err(ProfileError::io("write", tmp))?;
    let file = encoder.finish().map_err(ProfileError::io("write", tmp))?;
    file.sync_all().map_err(ProfileError::io("write", tmp))
}

/// Counts what is read, so a file that shrank mid-read cannot leave a corrupt archive.
struct Counted<R> {
    inner: R,
    read: u64,
}

impl<R: Read> Read for Counted<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.read += n as u64;
        Ok(n)
    }
}

fn append_file<W: io::Write>(tarball: &mut tar::Builder<W>, item: &Item) -> io::Result<u64> {
    let file = File::open(&item.source)?;
    let meta = file.metadata()?;
    let mut header = header_for(&meta);
    let mut reader = Counted {
        inner: file.take(meta.len()),
        read: 0,
    };
    tarball.append_data(&mut header, &item.name, &mut reader)?;
    if reader.read == meta.len() {
        return Ok(meta.len());
    }
    Err(io::Error::new(
        io::ErrorKind::UnexpectedEof,
        "the file changed while it was being packed",
    ))
}

struct Item {
    name: String,
    source: PathBuf,
}

#[derive(Default)]
struct Inventory {
    items: Vec<Item>,
    bytes: u64,
    skipped: Vec<PathBuf>,
}

impl Inventory {
    /// The mod list first (a pack without one is no pack), then the ticked groups, then the
    /// listed saves. `rel` paths are below the save-data folder.
    fn of(save_data: &Path, includes: &Includes, profile: &str) -> Result<Self, ProfileError> {
        let mut inventory = Self::default();
        let list = save_data.join(MOD_LIST);
        let meta = std::fs::symlink_metadata(&list)
            .ok()
            .filter(|m| m.is_file());
        let Some(meta) = meta else {
            return Err(ProfileError::NoModList(profile.to_owned()));
        };
        inventory.push(Path::new(MOD_LIST), list, meta.len());
        let ticks = [
            (includes.mod_settings, Group::ModSettings),
            (includes.game_settings, Group::GameSettings),
            (includes.creations, Group::Creations),
        ];
        let groups: Vec<Group> = ticks.iter().filter(|t| t.0).map(|t| t.1).collect();
        for rel in entries(save_data, &groups) {
            inventory.walk(save_data, &rel, includes)?;
        }
        for save in &includes.saves {
            let here = Path::new(file::FILE_NAME);
            check_save_name(save).map_err(|r| bad(here, r))?;
            let rel = Path::new(SAVES).join(save);
            let source = save_data.join(&rel);
            let found = std::fs::symlink_metadata(&source)
                .ok()
                .filter(|m| m.is_file());
            let Some(meta) = found else {
                return Err(bad(
                    here,
                    format!("the save `{save}` is not in this profile"),
                ));
            };
            inventory.push(&rel, source, meta.len());
        }
        Ok(inventory)
    }

    fn walk(&mut self, root: &Path, rel: &Path, includes: &Includes) -> Result<(), ProfileError> {
        let path = root.join(rel);
        let meta = std::fs::symlink_metadata(&path).map_err(ProfileError::io("read", &path))?;
        if meta.is_dir() {
            let read = std::fs::read_dir(&path).map_err(ProfileError::io("read", &path))?;
            let mut names: Vec<_> = read.filter_map(Result::ok).map(|e| e.file_name()).collect();
            names.sort();
            for name in names {
                self.walk(root, &rel.join(name), includes)?;
            }
        } else if meta.is_file() && check_declared(rel, includes).is_ok() {
            self.push(rel, path, meta.len());
        } else {
            self.skipped.push(rel.to_path_buf());
        }
        Ok(())
    }

    /// A name that is not plain text (or fails the entry rules) is left out, not an error.
    fn push(&mut self, rel: &Path, source: PathBuf, len: u64) {
        let parts: Option<Vec<&str>> = rel.components().map(|c| c.as_os_str().to_str()).collect();
        let name = parts.map(|p| format!("{SAVE_DATA}/{}", p.join("/")));
        match name.filter(|n| super::entry::classify(Path::new(n)).is_ok()) {
            Some(name) => {
                self.bytes += len;
                self.items.push(Item { name, source });
            }
            None => self.skipped.push(rel.to_path_buf()),
        }
    }
}
