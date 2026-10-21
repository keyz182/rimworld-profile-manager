//! Backup: one OFFLINE profile → one `.rimrim`.
//!
//! `plan_backup` shows the player what a backup would hold. `apply_backup` takes the run
//! lock, walks the profile again (so the totals in `rimrim.toml` describe the bytes it is
//! about to write), and writes to a temp file beside the target that is renamed on success.

use std::ffi::OsStr;
use std::fs::{File, Metadata};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use tar::{EntryType, Header, HeaderMode};

use super::entry::{BUILD_ROOT, PROFILE_ROOT, check_link_target};
use super::file::{self, Includes, Manifest};
use super::{NOTICE_GAME, NOTICE_MODS};
use crate::builds::{BuildKey, BuildStore};
use crate::error::ProfileError;
use crate::profile::file::FILE_NAME as PROFILE_FILE;
use crate::profile::lock::take_run_lock;
use crate::profile::offline::now_datetime;
use crate::profile::running::RunningProbe;
use crate::profile::{Mode, ProfileId, ProfileStore, Trash};
use crate::space;

const SAVE_DATA: &str = "SaveData";
const MODS: &str = "Mods";
const CONFIG: &str = "Config";
/// Archive folder for the profile's log files; restore puts each back at the profile root.
const ARCHIVE_LOGS: &str = "logs";
/// Logs at the profile root, and the Waylith log folder.
const LOG_FILES: [&str; 4] = ["Player.log", "Player-prev.log", "payload.log", "rwpm.log"];
const LOG_DIR: &str = "Logs";
/// Builds are mostly textures and DLLs; level 3 is zstd's own default speed/size balance.
const ZSTD_LEVEL: i32 = 3;
/// How many offending paths an error names before it says "and N more".
const NAMED_PATHS: usize = 5;

/// What a backup would hold, for the Backup dialog. Holds no handle on the profile.
#[derive(Debug, Clone)]
pub struct BackupPlan {
    pub id: ProfileId,
    pub name: String,
    pub includes: Includes,
    /// Bytes of every regular file that would go in (an upper bound on the archive size).
    pub bytes: u64,
    /// Entries after `rimrim.toml`.
    pub entries: u64,
    /// Links in the game build that are left out, with their path below the build folder.
    pub skipped_links: Vec<PathBuf>,
    dir: PathBuf,
    game_build: Option<String>,
    game_version: Option<String>,
    /// Set only when the build is included.
    build: Option<(BuildKey, PathBuf)>,
}

#[derive(Debug)]
pub struct BackupReport {
    pub manifest: Manifest,
    pub skipped_links: Vec<PathBuf>,
    pub archive_bytes: u64,
}

pub fn plan_backup<T: Trash>(
    store: &ProfileStore<T>,
    builds: &BuildStore,
    id: &ProfileId,
    includes: Includes,
) -> Result<BackupPlan, ProfileError> {
    let profile = store.get(id)?;
    let Some(dir) = profile.stored_dir().map(Path::to_path_buf) else {
        return Err(ProfileError::DefaultImmutable("backed up"));
    };
    if profile.mode != Mode::Offline {
        return Err(ProfileError::NotOffline(profile.name));
    }
    let build = match profile.game_build.as_deref() {
        Some(key) => match builds.get(&BuildKey::parse(key)?) {
            Ok(build) => Some(build),
            Err(ProfileError::BuildNotFound(_)) if !includes.build => None,
            Err(e) => return Err(e),
        },
        None if includes.build => return Err(ProfileError::NoBuildToBackUp(profile.name)),
        None => None,
    };
    let mut plan = BackupPlan {
        id: profile.id,
        name: profile.name,
        includes,
        bytes: 0,
        entries: 0,
        skipped_links: Vec::new(),
        dir,
        game_build: profile.game_build,
        game_version: build.as_ref().map(|b| b.file.version.clone()),
        build: build.filter(|_| includes.build).map(|b| (b.key, b.dir)),
    };
    let inventory = Inventory::of(&plan)?;
    plan.bytes = inventory.bytes;
    plan.entries = inventory.items.len() as u64;
    plan.skipped_links = inventory.skipped_links;
    Ok(plan)
}

/// Writes the backup to `dest`. Refused while a game runs. Nothing is left at `dest`
/// or beside it on failure; an existing `dest` is replaced only on success.
pub fn apply_backup(
    plan: &BackupPlan,
    dest: &Path,
    app_dir: &Path,
    probe: &dyn RunningProbe,
    progress: &mut dyn FnMut(u64),
) -> Result<BackupReport, ProfileError> {
    let _lock = take_run_lock(app_dir)?;
    if probe.any_game() {
        return Err(ProfileError::GameRunning);
    }
    let inventory = Inventory::of(plan)?;
    check_space(dest, inventory.bytes)?;
    let manifest = manifest_for(plan, &inventory);
    let tmp = temp_beside(dest)?;
    let written = write_archive(&tmp, &manifest, &inventory.items, progress)
        .and_then(|()| std::fs::rename(&tmp, dest).map_err(ProfileError::io("save", dest)));
    if let Err(e) = written {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(BackupReport {
        archive_bytes: std::fs::metadata(dest).map_or(0, |m| m.len()),
        manifest,
        skipped_links: inventory.skipped_links,
    })
}

fn manifest_for(plan: &BackupPlan, inventory: &Inventory) -> Manifest {
    Manifest {
        schema: file::SCHEMA,
        created_at: now_datetime(),
        rwpm_version: crate::VERSION.to_owned(),
        name: plan.name.clone(),
        mode: Mode::Offline,
        game_build: plan.game_build.clone(),
        game_version: plan.game_version.clone(),
        os: std::env::consts::OS.to_owned(),
        arch: std::env::consts::ARCH.to_owned(),
        uncompressed_bytes: inventory.bytes,
        entry_count: inventory.items.len() as u64,
        includes: plan.includes,
        notice_mods: NOTICE_MODS.to_owned(),
        notice_game: NOTICE_GAME.to_owned(),
    }
}

pub(crate) fn check_space(dest: &Path, need: u64) -> Result<(), ProfileError> {
    let dir = dest.parent().filter(|p| !p.as_os_str().is_empty());
    let dir = dir.unwrap_or_else(|| Path::new("."));
    match space::free_space(dir) {
        Some(free) if free < need => Err(ProfileError::NotEnoughSpace {
            path: dir.to_path_buf(),
            need,
            free,
        }),
        _ => Ok(()),
    }
}

pub(crate) fn temp_beside(dest: &Path) -> Result<PathBuf, ProfileError> {
    let Some(name) = dest.file_name().and_then(OsStr::to_str) else {
        let source = io::Error::new(io::ErrorKind::InvalidInput, "no file name");
        return Err(ProfileError::io("save", dest)(source));
    };
    Ok(dest.with_file_name(format!(".{name}.tmp")))
}

fn write_archive(
    tmp: &Path,
    manifest: &Manifest,
    items: &[Item],
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
    for item in items {
        append_item(&mut tarball, item, &mut done, progress)
            .map_err(ProfileError::io("pack", &item.source))?;
    }
    let encoder = tarball
        .into_inner()
        .map_err(ProfileError::io("write", tmp))?;
    let file = encoder.finish().map_err(ProfileError::io("write", tmp))?;
    file.sync_all().map_err(ProfileError::io("write", tmp))
}

/// A header with the owner, group and user name left blank (the file may be shared) but
/// the real modified time kept: RimWorld lists saves by it.
pub(crate) fn header_for(meta: &Metadata) -> Header {
    let mut header = Header::new_gnu();
    header.set_metadata_in_mode(meta, HeaderMode::Deterministic);
    if let Some(secs) = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
    {
        header.set_mtime(secs.as_secs());
    }
    header
}

fn append_item<W: Write>(
    tarball: &mut tar::Builder<W>,
    item: &Item,
    done: &mut u64,
    progress: &mut dyn FnMut(u64),
) -> io::Result<()> {
    match &item.kind {
        Kind::Dir => {
            let mut header = header_for(&std::fs::metadata(&item.source)?);
            tarball.append_data(&mut header, &item.name, io::empty())
        }
        Kind::Link(target) => {
            let mut header = Header::new_gnu();
            header.set_entry_type(EntryType::Symlink);
            header.set_size(0);
            header.set_mode(0o777);
            header.set_link_name(target)?;
            tarball.append_data(&mut header, &item.name, io::empty())
        }
        Kind::File => {
            let file = File::open(&item.source)?;
            let meta = file.metadata()?;
            let mut header = header_for(&meta);
            let before = *done;
            let reader = Counting {
                inner: file.take(meta.len()),
                done,
                progress,
            };
            tarball.append_data(&mut header, &item.name, reader)?;
            if *done - before == meta.len() {
                return Ok(());
            }
            // The file shrank while it was read; the archive would be corrupt.
            Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "the file changed while it was being backed up",
            ))
        }
    }
}

struct Counting<'a, R> {
    inner: R,
    done: &'a mut u64,
    progress: &'a mut dyn FnMut(u64),
}

impl<R: Read> Read for Counting<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        *self.done += n as u64;
        (self.progress)(*self.done);
        Ok(n)
    }
}

enum Kind {
    Dir,
    File,
    Link(PathBuf),
}

struct Item {
    /// Name inside the archive.
    name: PathBuf,
    source: PathBuf,
    kind: Kind,
}

/// Everything a backup would write after `rimrim.toml`, in archive order.
#[derive(Default)]
struct Inventory {
    items: Vec<Item>,
    bytes: u64,
    /// Links and special files in the profile, which a backup cannot hold.
    refused: Vec<String>,
    skipped_links: Vec<PathBuf>,
    build_links: Vec<Item>,
}

impl Inventory {
    fn of(plan: &BackupPlan) -> Result<Self, ProfileError> {
        let mut inventory = Self::default();
        let root = Path::new(PROFILE_ROOT);
        inventory.tree(&plan.dir.join(MODS), &root.join(MODS), None)?;
        let saves = plan.dir.join(SAVE_DATA);
        if plan.includes.saves {
            inventory.tree(&saves, &root.join(SAVE_DATA), None)?;
        } else {
            let (src, name) = (saves.join(CONFIG), root.join(SAVE_DATA).join(CONFIG));
            inventory.tree(&src, &name, None)?;
        }
        if plan.includes.logs {
            let logs = root.join(ARCHIVE_LOGS);
            for file in LOG_FILES {
                inventory.file(&plan.dir.join(file), &logs.join(file))?;
            }
            inventory.tree(&plan.dir.join(LOG_DIR), &logs.join(LOG_DIR), None)?;
        }
        if let Some((key, dir)) = &plan.build {
            inventory.build(key, dir)?;
        }
        inventory.file(&plan.dir.join(PROFILE_FILE), &root.join(PROFILE_FILE))?;
        if inventory.refused.is_empty() {
            return Ok(inventory);
        }
        let more = inventory.refused.len().saturating_sub(NAMED_PATHS);
        let mut named: Vec<String> = inventory.refused.drain(..).take(NAMED_PATHS).collect();
        if more > 0 {
            named.push(format!("and {more} more"));
        }
        Err(ProfileError::NotBackupable(named))
    }

    /// The build folder: files and folders, then its links, then `build.toml`, so an
    /// unfinished extract never looks like a whole build.
    fn build(&mut self, key: &BuildKey, dir: &Path) -> Result<(), ProfileError> {
        let root = Path::new(BUILD_ROOT).join(key.as_str());
        let first = self.items.len();
        self.tree(dir, &root, Some(&root))?;
        let manifest = root.join(crate::builds::file::FILE_NAME);
        let at = self.items[first..]
            .iter()
            .position(|i| i.name == manifest)
            .map(|p| p + first);
        let manifest = at.map(|p| self.items.remove(p));
        self.items.append(&mut self.build_links);
        self.items.extend(manifest);
        Ok(())
    }

    /// `build_root` is `Some` below a game build, where safe links are carried.
    fn tree(
        &mut self,
        src: &Path,
        name: &Path,
        build_root: Option<&Path>,
    ) -> Result<(), ProfileError> {
        let meta = match std::fs::symlink_metadata(src) {
            Ok(meta) => meta,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(ProfileError::io("read", src)(e)),
        };
        if !meta.is_dir() {
            self.refuse(src);
            return Ok(());
        }
        self.items.push(Item {
            name: name.to_path_buf(),
            source: src.to_path_buf(),
            kind: Kind::Dir,
        });
        let read = |e| ProfileError::io("read", src)(e);
        let mut entries = std::fs::read_dir(src)
            .map_err(read)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(read)?;
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for entry in entries {
            let (path, child) = (entry.path(), name.join(entry.file_name()));
            if !name_ok(&entry.file_name()) {
                self.refuse(&path);
                continue;
            }
            let kind = entry.file_type().map_err(ProfileError::io("read", &path))?;
            if kind.is_dir() {
                self.tree(&path, &child, build_root)?;
            } else if kind.is_file() {
                self.file(&path, &child)?;
            } else if kind.is_symlink() {
                self.link(&path, &child, build_root)?;
            } else {
                self.refuse(&path);
            }
        }
        Ok(())
    }

    /// A regular file; a missing one is skipped, anything else is refused.
    fn file(&mut self, src: &Path, name: &Path) -> Result<(), ProfileError> {
        let meta = match std::fs::symlink_metadata(src) {
            Ok(meta) => meta,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(ProfileError::io("read", src)(e)),
        };
        if !meta.is_file() {
            self.refuse(src);
            return Ok(());
        }
        self.bytes += meta.len();
        self.items.push(Item {
            name: name.to_path_buf(),
            source: src.to_path_buf(),
            kind: Kind::File,
        });
        Ok(())
    }

    fn link(
        &mut self,
        src: &Path,
        name: &Path,
        build_root: Option<&Path>,
    ) -> Result<(), ProfileError> {
        let Some(root) = build_root else {
            self.refuse(src);
            return Ok(());
        };
        let target = std::fs::read_link(src).map_err(ProfileError::io("read", src))?;
        let rel = name.strip_prefix(root).unwrap_or(name).to_path_buf();
        if check_link_target(&rel, &target).is_ok() {
            self.build_links.push(Item {
                name: name.to_path_buf(),
                source: src.to_path_buf(),
                kind: Kind::Link(target),
            });
        } else {
            self.skipped_links.push(rel);
        }
        Ok(())
    }

    fn refuse(&mut self, path: &Path) {
        self.refused.push(path.display().to_string());
    }
}

/// Names restore would refuse are refused here, so no backup is unrestorable.
fn name_ok(name: &OsStr) -> bool {
    name.to_str()
        .is_some_and(|s| !s.contains(['\\', ':', '\0']))
}

#[cfg(test)]
pub(crate) mod tests;
