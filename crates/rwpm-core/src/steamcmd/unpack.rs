//! Unpacking Valve's bootstrap archive into the app's own steamcmd folder.
//!
//! The archive came from the network, so every entry is checked before anything is written:
//! the whole archive is walked once to check names, kinds, counts and sizes, and only then
//! walked again to write. Names may not climb out (`..`), start at a root, or hold odd
//! characters. Only files, folders and symbolic links are made. A link is allowed only when
//! its target goes down from its own folder: such links cannot lead out of the folder, even
//! when chained. Valve's macOS archive needs them for its framework layout.

use std::io::Read;
use std::path::{Component, Path, PathBuf};

use super::{ArchiveKind, Error, MAX_ENTRIES, MAX_UNPACKED_BYTES};

/// Unpacks `bytes` into `dest`, which is created if needed. Nothing is written unless the
/// whole archive passes the checks.
pub fn install(kind: ArchiveKind, bytes: &[u8], dest: &Path) -> Result<(), Error> {
    walk(kind, bytes, dest, false)?;
    std::fs::create_dir_all(dest).map_err(Error::io("create", dest))?;
    walk(kind, bytes, dest, true)
}

fn walk(kind: ArchiveKind, bytes: &[u8], dest: &Path, write: bool) -> Result<(), Error> {
    let mut sink = Sink {
        dest,
        write,
        entries: 0,
        total: 0,
    };
    match kind {
        ArchiveKind::Zip => walk_zip(bytes, &mut sink),
        ArchiveKind::TarGz => walk_tar(bytes, &mut sink),
    }
}

/// Checks each entry and, in the writing pass, makes it.
struct Sink<'a> {
    dest: &'a Path,
    write: bool,
    entries: usize,
    total: u64,
}

fn unsafe_entry(name: &str) -> Error {
    Error::Unsafe(name.chars().take(80).collect())
}

/// `./a/b` and `a/b` are the same entry. Anything but plain names is refused.
fn clean_name(raw: &str) -> Result<PathBuf, Error> {
    let bad_char = |c: char| c.is_control() || c == '\\' || c == ':';
    if raw.is_empty() || raw.chars().any(bad_char) {
        return Err(unsafe_entry(raw));
    }
    let mut clean = PathBuf::new();
    for part in Path::new(raw).components() {
        match part {
            Component::Normal(name) => clean.push(name),
            Component::CurDir => {}
            _ => return Err(unsafe_entry(raw)),
        }
    }
    Ok(clean)
}

/// A link target that only goes down: plain names, no `..`, no root.
fn check_target(name: &str, target: &str) -> Result<(), Error> {
    let down = !target.is_empty()
        && !target
            .chars()
            .any(|c| c.is_control() || c == '\\' || c == ':')
        && Path::new(target)
            .components()
            .all(|c| matches!(c, Component::Normal(_)));
    if down {
        Ok(())
    } else {
        Err(unsafe_entry(name))
    }
}

impl Sink<'_> {
    fn count(&mut self, name: &str, size: u64) -> Result<(), Error> {
        self.entries += 1;
        self.total = self.total.saturating_add(size);
        if self.entries > MAX_ENTRIES || self.total > MAX_UNPACKED_BYTES {
            return Err(Error::Archive(format!(
                "too much to unpack (stopped at `{}`)",
                name.chars().take(80).collect::<String>()
            )));
        }
        Ok(())
    }

    fn dir(&mut self, raw: &str) -> Result<(), Error> {
        let clean = clean_name(raw)?;
        self.count(raw, 0)?;
        if self.write {
            let path = self.dest.join(clean);
            std::fs::create_dir_all(&path).map_err(Error::io("create", &path))?;
        }
        Ok(())
    }

    fn file(
        &mut self,
        raw: &str,
        size: u64,
        executable: bool,
        data: &mut dyn Read,
    ) -> Result<(), Error> {
        let clean = clean_name(raw)?;
        if clean.as_os_str().is_empty() {
            return Err(unsafe_entry(raw));
        }
        self.count(raw, size)?;
        if !self.write {
            return Ok(());
        }
        let path = self.dest.join(clean);
        make_parent(&path)?;
        remove_existing(&path)?;
        let mut out = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(Error::io("create", &path))?;
        // Never more than the header promised, which the checking pass already counted.
        let copied =
            std::io::copy(&mut data.take(size), &mut out).map_err(Error::io("write", &path))?;
        if copied != size {
            return Err(Error::Archive(format!("`{raw}` is shorter than it says")));
        }
        set_executable(&path, executable)
    }

    fn link(&mut self, raw: &str, target: &str) -> Result<(), Error> {
        let clean = clean_name(raw)?;
        check_target(raw, target)?;
        self.count(raw, 0)?;
        if !self.write {
            return Ok(());
        }
        let path = self.dest.join(clean);
        make_parent(&path)?;
        remove_existing(&path)?;
        make_link(target, &path)
    }
}

fn make_parent(path: &Path) -> Result<(), Error> {
    match path.parent() {
        Some(parent) => std::fs::create_dir_all(parent).map_err(Error::io("create", parent)),
        None => Ok(()),
    }
}

/// A file or link already at `path` is replaced, never written through.
fn remove_existing(path: &Path) -> Result<(), Error> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => Err(unsafe_entry(&path.display().to_string())),
        Ok(_) => std::fs::remove_file(path).map_err(Error::io("remove", path)),
        Err(_) => Ok(()),
    }
}

#[cfg(unix)]
fn make_link(target: &str, path: &Path) -> Result<(), Error> {
    std::os::unix::fs::symlink(target, path).map_err(Error::io("link", path))
}

#[cfg(not(unix))]
fn make_link(_target: &str, path: &Path) -> Result<(), Error> {
    Err(unsafe_entry(&path.display().to_string()))
}

#[cfg(unix)]
fn set_executable(path: &Path, executable: bool) -> Result<(), Error> {
    use std::os::unix::fs::PermissionsExt;
    let mode = if executable { 0o755 } else { 0o644 };
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .map_err(Error::io("chmod", path))
}

#[cfg(not(unix))]
fn set_executable(_path: &Path, _executable: bool) -> Result<(), Error> {
    Ok(())
}

fn walk_zip(bytes: &[u8], sink: &mut Sink<'_>) -> Result<(), Error> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|e| Error::Archive(e.to_string()))?;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|e| Error::Archive(e.to_string()))?;
        let name = entry
            .name()
            .map_err(|e| Error::Archive(e.to_string()))?
            .into_owned();
        let mode = entry.unix_mode().unwrap_or(0o644);
        // 0o120000 is a link, which Valve's zip does not use.
        if mode & 0o170000 == 0o120000 {
            return Err(unsafe_entry(&name));
        }
        if entry.is_dir() {
            sink.dir(&name)?;
        } else {
            let size = entry.size();
            sink.file(&name, size, mode & 0o111 != 0, &mut entry)?;
        }
    }
    Ok(())
}

fn walk_tar(bytes: &[u8], sink: &mut Sink<'_>) -> Result<(), Error> {
    use tar::EntryType;
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(bytes));
    let damaged = |e: std::io::Error| Error::Archive(e.to_string());
    for entry in archive.entries().map_err(damaged)? {
        let mut entry = entry.map_err(damaged)?;
        let raw = String::from_utf8_lossy(&entry.path_bytes()).into_owned();
        let size = entry.size();
        let executable = entry.header().mode().is_ok_and(|m| m & 0o111 != 0);
        match entry.header().entry_type() {
            EntryType::Directory => sink.dir(&raw)?,
            EntryType::Regular => sink.file(&raw, size, executable, &mut entry)?,
            EntryType::Symlink => {
                let target = entry
                    .link_name_bytes()
                    .map(|t| String::from_utf8_lossy(&t).into_owned())
                    .unwrap_or_default();
                sink.link(&raw, &target)?;
            }
            _ => return Err(unsafe_entry(&raw)),
        }
    }
    Ok(())
}
