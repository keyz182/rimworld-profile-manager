//! Restore: a `.rimrim` → a NEW profile (T73; V73-V79, V82).
//!
//! The archive is untrusted. Every entry name, kind, size and link target is checked
//! before anything is written, nothing is ever written through a link (links are made
//! last), and the profile is built the way `clone_profile` builds one: into a freshly
//! claimed folder, `profile.toml` last, the folder removed on any error.

use std::collections::BTreeSet;
use std::fs::{File, OpenOptions};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use tar::EntryType;

use super::entry::{Place, check_folder, check_kind, check_link_target, classify};
use super::file::Manifest;
use super::{bad, open, read_manifest};
use crate::builds::{BuildFile, BuildKey, BuildStore, STAGING_PREFIX};
use crate::discovery;
use crate::error::ProfileError;
use crate::profile::file::FILE_NAME as PROFILE_FILE;
use crate::profile::lock::take_run_lock;
use crate::profile::running::RunningProbe;
use crate::profile::{Mode, Profile, ProfileFile, ProfileId, ProfileStore, Trash};
use crate::space;

/// Refused outright, whatever a manifest claims (V75).
const MAX_BYTES: u64 = 128 << 30;
const MAX_ENTRIES: u64 = 2_000_000;
/// Free space wanted on top of the declared size.
const MARGIN_PERCENT: u64 = 10;
/// `profile.toml` and `build.toml` are read into memory; neither is ever this big.
const MAX_SMALL_FILE: u64 = 64 * 1024;
const LOG_FILES: [&str; 4] = ["Player.log", "Player-prev.log", "payload.log", "rwpm.log"];
const LOG_DIR: &str = "Logs";

/// Things the player should hear about after (or before) a restore. Plain words.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RestoreNote {
    #[error(
        "The backup holds game files for {os} ({arch}), and this computer runs {here_os} ({here_arch}). The game files were not restored."
    )]
    BuildOtherSystem {
        os: String,
        arch: String,
        here_os: &'static str,
        here_arch: &'static str,
    },
    #[error(
        "The profile uses game version `{0}`, which is not stored on this computer. Capture it under Game versions, or choose another version, before you play."
    )]
    BuildMissing(String),
    #[error(
        "A different copy of game version `{0}` is already stored here, so the one in the backup was not restored."
    )]
    BuildDiffers(String),
    #[error(
        "The game files in the backup could not be used ({reason}), so they were not restored. The profile still uses game version `{key}`."
    )]
    BuildBroken { key: String, reason: String },
}

/// What restore does about the game build.
#[derive(Debug, Clone, PartialEq, Eq)]
enum BuildAction {
    /// The backup holds no build.
    None,
    /// Extract it into the store.
    Extract(BuildKey),
    /// A build with this key is stored already: keep it, and only compare.
    Compare(BuildKey),
    /// Wrong operating system: leave the entries unread.
    Skip(BuildKey),
}

/// What a restore would do, for the Restore dialog.
#[derive(Debug, Clone)]
pub struct RestorePlan {
    pub archive: PathBuf,
    pub manifest: Manifest,
    /// Display name of the new profile; the player may change it before `apply_restore`.
    pub name: String,
    /// Disk the restore needs, margin included.
    pub bytes_needed: u64,
    pub free_bytes: Option<u64>,
    pub notes: Vec<RestoreNote>,
}

#[derive(Debug)]
pub struct RestoreReport {
    pub profile: Profile,
    pub notes: Vec<RestoreNote>,
}

pub fn plan_restore<T: Trash>(
    archive: &Path,
    store: &ProfileStore<T>,
    builds: &BuildStore,
) -> Result<RestorePlan, ProfileError> {
    let manifest = read_manifest(archive)?;
    if manifest.uncompressed_bytes > MAX_BYTES || manifest.entry_count > MAX_ENTRIES {
        return Err(bad(archive, "it is larger than a backup may be"));
    }
    let need = manifest.uncompressed_bytes + manifest.uncompressed_bytes / 100 * MARGIN_PERCENT;
    let free = space::free_space(store.root());
    if let Some(free) = free.filter(|&f| f < need) {
        return Err(ProfileError::NotEnoughSpace {
            path: store.root().to_path_buf(),
            need,
            free,
        });
    }
    let (_, notes) = build_action(&manifest, builds)?;
    Ok(RestorePlan {
        archive: archive.to_path_buf(),
        name: manifest.name.clone(),
        manifest,
        bytes_needed: need,
        free_bytes: free,
        notes,
    })
}

/// Restores into a new profile. Refused while a game runs (V79). On any error the new
/// profile folder and the build staging folder are removed (V76, V77).
pub fn apply_restore<T: Trash>(
    plan: &RestorePlan,
    store: &ProfileStore<T>,
    builds: &BuildStore,
    probe: &dyn RunningProbe,
    progress: &mut dyn FnMut(u64),
) -> Result<RestoreReport, ProfileError> {
    let _lock = take_run_lock(builds.app_dir())?;
    if probe.any_game() {
        return Err(ProfileError::GameRunning);
    }
    let name = plan.name.trim();
    if name.is_empty() {
        return Err(ProfileError::EmptyName);
    }
    let (action, notes) = build_action(&plan.manifest, builds)?;
    if matches!(action, BuildAction::Extract(_)) {
        builds.recover_locked()?;
    }
    let (id, dir) = store.claim(&ProfileId::slug_base(name))?;
    let staging = builds
        .root()
        .join(format!("{STAGING_PREFIX}restore-{}", id.as_str()));
    let mut job = Job {
        plan,
        builds,
        action,
        dir: &dir,
        staging: &staging,
        progress,
        bytes: 0,
        entries: 0,
        mod_dirs: BTreeSet::new(),
        profile_toml: None,
        build_toml: None,
        links: Vec::new(),
        notes,
    };
    let done = job
        .run()
        .and_then(|()| job.finish(name))
        .and_then(|()| store.get(&id));
    let notes = job.notes;
    match done {
        Ok(profile) => Ok(RestoreReport { profile, notes }),
        Err(e) => {
            let _ = std::fs::remove_dir_all(&dir);
            let _ = std::fs::remove_dir_all(&staging);
            Err(e)
        }
    }
}

fn build_action(
    manifest: &Manifest,
    builds: &BuildStore,
) -> Result<(BuildAction, Vec<RestoreNote>), ProfileError> {
    let Some(key) = manifest.game_build.as_deref() else {
        return Ok((BuildAction::None, Vec::new()));
    };
    let key = BuildKey::parse(key)?;
    let stored = builds.dir_of(&key).exists();
    let mut notes = Vec::new();
    let action = if !manifest.includes.build {
        BuildAction::None
    } else if manifest.os != std::env::consts::OS || manifest.arch != std::env::consts::ARCH {
        notes.push(RestoreNote::BuildOtherSystem {
            os: manifest.os.clone(),
            arch: manifest.arch.clone(),
            here_os: std::env::consts::OS,
            here_arch: std::env::consts::ARCH,
        });
        BuildAction::Skip(key.clone())
    } else if stored {
        BuildAction::Compare(key.clone())
    } else {
        BuildAction::Extract(key.clone())
    };
    // The pin is kept either way; say so when nothing will provide the build (V78).
    if !stored && !matches!(action, BuildAction::Extract(_)) {
        notes.push(RestoreNote::BuildMissing(key.to_string()));
    }
    Ok((action, notes))
}

struct Job<'a> {
    plan: &'a RestorePlan,
    builds: &'a BuildStore,
    action: BuildAction,
    dir: &'a Path,
    staging: &'a Path,
    progress: &'a mut dyn FnMut(u64),
    bytes: u64,
    entries: u64,
    /// Mod folders seen in the archive, for the folder check on `profile.toml` (V74).
    mod_dirs: BTreeSet<String>,
    profile_toml: Option<String>,
    build_toml: Option<String>,
    /// Build links, made after everything else: (path below the build, target).
    links: Vec<(PathBuf, PathBuf)>,
    notes: Vec<RestoreNote>,
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
        let decode = |e: io::Error| bad(&archive, format!("it cannot be read as a backup ({e})"));
        let mut reader = open(&archive)?;
        let mut entries = reader.entries().map_err(decode)?;
        let mut first = entries
            .next()
            .ok_or_else(|| self.fault("it is empty"))?
            .map_err(decode)?;
        let text = read_small(&mut first).map_err(|r| self.fault(r))?;
        if Manifest::parse(&text, &archive)? != self.plan.manifest {
            return Err(self.fault("it changed after it was opened"));
        }
        if self.action_extracts() {
            // A machine that has never stored a build has no `builds` folder yet.
            let root = self.builds.root();
            std::fs::create_dir_all(root).map_err(ProfileError::io("create", root))?;
            std::fs::create_dir(self.staging).map_err(ProfileError::io("create", self.staging))?;
        }
        for entry in entries {
            self.entry(entry.map_err(decode)?)?;
            (self.progress)(self.bytes);
        }
        let want = (
            self.plan.manifest.entry_count,
            self.plan.manifest.uncompressed_bytes,
        );
        if (self.entries, self.bytes) != want {
            return Err(self.fault("it holds less than it says"));
        }
        Ok(())
    }

    fn action_extracts(&self) -> bool {
        matches!(self.action, BuildAction::Extract(_))
    }

    fn entry<R: Read>(&mut self, mut entry: tar::Entry<'_, R>) -> Result<(), ProfileError> {
        if self.profile_toml.is_some() {
            return Err(self.fault("it has entries after `profile.toml`"));
        }
        self.entries += 1;
        if self.entries > self.plan.manifest.entry_count {
            return Err(self.fault("it holds more entries than it says"));
        }
        let name = entry
            .path()
            .map_err(ProfileError::io("read", self.archive()))?;
        let name = name.into_owned();
        let place = classify(&name).map_err(|r| self.fault(r))?;
        let kind = entry.header().entry_type();
        check_kind(kind, &name, &place).map_err(|r| self.fault(r))?;
        if kind == EntryType::Regular {
            self.bytes += entry.size();
            if self.bytes > self.plan.manifest.uncompressed_bytes {
                return Err(self.fault("it holds more data than it says"));
            }
        }
        let done = match place {
            Place::Manifest => Err(self.fault("it has a second `rimrim.toml`")),
            Place::Profile(rel) => self.profile_entry(&rel, kind, &mut entry),
            Place::Build { key, rel } => self.build_entry(&key, &rel, kind, &mut entry),
        };
        // Files are created new, so a repeated name fails instead of overwriting (V73).
        done.map_err(|e| match &e {
            ProfileError::Io { source, .. } if source.kind() == io::ErrorKind::AlreadyExists => {
                self.fault(format!("it holds `{}` twice", name.display()))
            }
            _ => e,
        })
    }

    fn profile_entry<R: Read>(
        &mut self,
        rel: &Path,
        kind: EntryType,
        entry: &mut tar::Entry<'_, R>,
    ) -> Result<(), ProfileError> {
        let parts = parts_of(rel);
        let includes = self.plan.manifest.includes;
        let dest = match parts.as_slice() {
            [] if kind == EntryType::Directory => return Ok(()),
            ["profile.toml"] if kind == EntryType::Regular => {
                self.profile_toml = Some(read_small(entry).map_err(|r| self.fault(r))?);
                return Ok(());
            }
            ["Mods" | "SaveData"] if kind == EntryType::Directory => self.dir.join(rel),
            ["Mods", rest @ ..] if !rest.is_empty() => {
                self.note_mod_dir(rest, kind)?;
                self.dir.join(rel)
            }
            ["SaveData", rest @ ..] if includes.saves || rest[0] == "Config" => self.dir.join(rel),
            ["logs"] if includes.logs && kind == EntryType::Directory => return Ok(()),
            ["logs", name, ..] if includes.logs && is_log(name, kind, parts.len()) => {
                self.dir.join(rel.strip_prefix("logs").unwrap_or(rel))
            }
            _ => {
                let why = format!(
                    "`profile/{}` is not something this backup declares",
                    rel.display()
                );
                return Err(self.fault(why));
            }
        };
        write_entry(&dest, kind, entry)
    }

    /// Every name directly below `Mods/` must be a plain folder name (V74).
    fn note_mod_dir(&mut self, rest: &[&str], kind: EntryType) -> Result<(), ProfileError> {
        let Some(folder) = rest.first() else {
            return Ok(());
        };
        check_folder(folder).map_err(|r| self.fault(r))?;
        if rest.len() > 1 || kind == EntryType::Directory {
            self.mod_dirs.insert((*folder).to_owned());
        }
        Ok(())
    }

    fn build_entry<R: Read>(
        &mut self,
        key: &BuildKey,
        rel: &Path,
        kind: EntryType,
        entry: &mut tar::Entry<'_, R>,
    ) -> Result<(), ProfileError> {
        let named = self.plan.manifest.game_build.as_deref() == Some(key.as_str());
        if matches!(self.action, BuildAction::None) || !named {
            return Err(self.fault("it holds game files the manifest does not declare"));
        }
        if matches!(self.action, BuildAction::Skip(_)) {
            return Ok(());
        }
        let is_toml = rel == Path::new(crate::builds::file::FILE_NAME);
        if is_toml && kind == EntryType::Regular {
            let text = read_small(entry).map_err(|r| self.fault(r))?;
            if self.action_extracts() {
                write_bytes(&self.staging.join(rel), text.as_bytes())?;
            }
            self.build_toml = Some(text);
            return Ok(());
        }
        if !self.action_extracts() {
            return Ok(());
        }
        if kind == EntryType::Symlink {
            let target = entry.link_name().ok().flatten().map(|t| t.into_owned());
            let target = target.ok_or_else(|| self.fault("a link has no target"))?;
            check_link_target(rel, &target).map_err(|r| self.fault(r))?;
            self.links.push((rel.to_path_buf(), target));
            return Ok(());
        }
        write_entry(&self.staging.join(rel), kind, entry)
    }

    /// After the stream: check `profile.toml` against what was unpacked (V74), settle the
    /// build, then write `profile.toml` last (V76).
    fn finish(&mut self, name: &str) -> Result<(), ProfileError> {
        let text = self
            .profile_toml
            .take()
            .ok_or_else(|| self.fault("it has no `profile.toml`"))?;
        let mut file = ProfileFile::parse(&text, &self.plan.archive.join(PROFILE_FILE))?;
        self.check_profile(&file)?;
        self.settle_build();
        file.name = name.to_owned();
        file.write_atomic(&self.dir.join(PROFILE_FILE))
    }

    fn check_profile(&self, file: &ProfileFile) -> Result<(), ProfileError> {
        if file.mode != Mode::Offline {
            return Err(self.fault("its profile is not an offline profile"));
        }
        if file.game_build != self.plan.manifest.game_build {
            return Err(self.fault("its profile and its manifest name different game versions"));
        }
        for mod_ in &file.frozen_mods {
            check_folder(&mod_.folder).map_err(|r| self.fault(r))?;
            if !self.mod_dirs.contains(&mod_.folder) {
                let why = format!("`{}` is listed but has no folder in it", mod_.folder);
                return Err(self.fault(why));
            }
        }
        Ok(())
    }

    /// A build that cannot be used is left out with a note; the rest of the restore stands.
    fn settle_build(&mut self) {
        let action = self.action.clone();
        let outcome = match &action {
            BuildAction::Extract(key) => self.install_build(key),
            BuildAction::Compare(key) => self.compare_build(key),
            BuildAction::None | BuildAction::Skip(_) => Ok(()),
        };
        if let (Err(reason), BuildAction::Extract(key) | BuildAction::Compare(key)) =
            (outcome, &action)
        {
            let _ = std::fs::remove_dir_all(self.staging);
            self.notes.push(RestoreNote::BuildBroken {
                key: key.to_string(),
                reason,
            });
        }
    }

    fn compare_build(&mut self, key: &BuildKey) -> Result<(), String> {
        let text = self.build_toml.take().ok_or("it has no `build.toml`")?;
        let theirs = BuildFile::parse(&text, Path::new(crate::builds::file::FILE_NAME))
            .map_err(|e| e.to_string())?;
        let ours = self.builds.get(key).map_err(|e| e.to_string())?;
        if ours.file.buildid != theirs.buildid {
            self.notes.push(RestoreNote::BuildDiffers(key.to_string()));
        }
        Ok(())
    }

    /// Links, then verification, then the move into the store (V77).
    fn install_build(&mut self, key: &BuildKey) -> Result<(), String> {
        make_links(self.staging, &self.links)?;
        let file = BuildFile::read(&self.staging.join(crate::builds::file::FILE_NAME))
            .map_err(|e| e.to_string())?;
        discovery::from_build_dir(self.staging, &file.source_dir)
            .map_err(|_| "it is not a RimWorld game folder".to_owned())?;
        let live = self.builds.dir_of(key);
        if live.exists() {
            // Someone stored this key while the restore ran; theirs stays (V77).
            let _ = std::fs::remove_dir_all(self.staging);
            return Ok(());
        }
        std::fs::rename(self.staging, &live).map_err(|e| e.to_string())
    }
}

pub(crate) fn parts_of(rel: &Path) -> Vec<&str> {
    rel.components()
        .filter_map(|c| c.as_os_str().to_str())
        .collect()
}

/// `profile/logs/<name>` entries: the known log files, and the `Logs` folder with its contents.
fn is_log(name: &str, kind: EntryType, depth: usize) -> bool {
    if name == LOG_DIR {
        return true;
    }
    depth == 2 && kind == EntryType::Regular && LOG_FILES.contains(&name)
}

fn read_small<R: Read>(entry: &mut tar::Entry<'_, R>) -> Result<String, String> {
    if entry.size() > MAX_SMALL_FILE {
        return Err("a settings file in it is too big".into());
    }
    let mut text = String::new();
    entry
        .read_to_string(&mut text)
        .map_err(|e| format!("it cannot be read as a backup ({e})"))?;
    Ok(text)
}

fn write_bytes(dest: &Path, bytes: &[u8]) -> Result<(), ProfileError> {
    let io = ProfileError::io("unpack", dest);
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(ProfileError::io("unpack", parent))?;
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dest)
        .map_err(io)?;
    io::Write::write_all(&mut file, bytes).map_err(ProfileError::io("unpack", dest))
}

/// Folders and regular files only (V73). A file is created new, so a repeated name fails
/// instead of overwriting an earlier entry. Modes are reduced to plain or executable.
pub(crate) fn write_entry<R: Read>(
    dest: &Path,
    kind: EntryType,
    entry: &mut tar::Entry<'_, R>,
) -> Result<(), ProfileError> {
    let fail = || ProfileError::io("unpack", dest);
    if kind == EntryType::Directory {
        return std::fs::create_dir_all(dest).map_err(fail());
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(ProfileError::io("unpack", parent))?;
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dest)
        .map_err(fail())?;
    io::copy(entry, &mut file).map_err(fail())?;
    let mode = entry.header().mode().unwrap_or(0o644);
    set_mode(&file, mode).map_err(fail())?;
    if let Some(when) = entry
        .header()
        .mtime()
        .ok()
        .and_then(|s| UNIX_EPOCH.checked_add(Duration::from_secs(s)))
    {
        // A time that cannot be set is only cosmetic; the file itself is whole.
        let _ = file.set_modified(when);
    }
    Ok(())
}

#[cfg(unix)]
fn set_mode(file: &File, mode: u32) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let plain = if mode & 0o111 == 0 { 0o644 } else { 0o755 };
    file.set_permissions(std::fs::Permissions::from_mode(plain))
}

#[cfg(not(unix))]
fn set_mode(_: &File, _: u32) -> io::Result<()> {
    Ok(())
}

/// Makes the build's links once every file is in place (V73). A link goes only into a real
/// folder, never over an existing name, and its target was checked when it was read.
#[cfg(unix)]
fn make_links(root: &Path, links: &[(PathBuf, PathBuf)]) -> Result<(), String> {
    for (rel, target) in links {
        let at = root.join(rel);
        let parent = at.parent().unwrap_or(root);
        let real_dir = std::fs::symlink_metadata(parent).is_ok_and(|m| m.is_dir());
        if !real_dir || std::fs::symlink_metadata(&at).is_ok() {
            return Err(format!("the link `{}` has no place to go", rel.display()));
        }
        std::os::unix::fs::symlink(target, &at).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn make_links(_: &Path, links: &[(PathBuf, PathBuf)]) -> Result<(), String> {
    if links.is_empty() {
        Ok(())
    } else {
        Err("links in the game files cannot be restored on this system".into())
    }
}

#[cfg(test)]
mod tests;
