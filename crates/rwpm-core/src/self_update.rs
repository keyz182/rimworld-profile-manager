//! Downloads a newer release and installs it over the running one.
//!
//! Which files get replaced depends on how rwpm was installed ([`InstallKind`]); a copy a
//! package manager owns never gets here. Every download is checked against the release's
//! `SHA256SUMS` before it is used. A portable folder is replaced all or nothing: each move is
//! journalled and undone if a later one fails, so the folder is never left half old, half new.
//!
//! Nothing here runs by itself. The window calls [`download`], then [`apply`], then
//! [`spawn`] on the result and exits.

use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::http::{self, HttpError};
use crate::install_kind::{InstallKind, Os};
use crate::update_check::{DOWNLOAD_PREFIX, Release, Version};

/// The checksum file the release workflow attaches.
pub const SUMS_NAME: &str = "SHA256SUMS";
/// Work folder inside a portable install. Removed when an update ends and on the next start.
pub const STAGE_DIR: &str = ".rwpm-update";
/// The installer's download folder, under the system temp folder.
const TEMP_DIR: &str = "rwpm-update";

const OLD_SUFFIX: &str = ".old";
const NEW_SUFFIX: &str = ".rwpm-new";
/// The v1.17.0 files are 9.5 to 16 MB; this leaves four times the largest.
const DOWNLOAD_CAP: u64 = 64 * 1024 * 1024;
const SUMS_CAP: u64 = 64 * 1024;
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const SUMS_TIMEOUT: Duration = Duration::from_secs(30);
/// Most an archive may unpack to. The v1.17.0 folders are about 35 MB.
const UNPACK_CAP: u64 = 512 * 1024 * 1024;
const RENAME_TRIES: u32 = 3;
const RENAME_WAIT: Duration = Duration::from_millis(200);

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("this install cannot update itself")]
    Unsupported,
    #[error("the release has no `{0}` file")]
    NoAsset(String),
    #[error("the release has no checksum for `{0}`, so the download cannot be checked")]
    NoChecksum(String),
    #[error("the download address is not one of this project's: {0}")]
    BadUrl(String),
    #[error("could not download the update: {0}")]
    Request(String),
    #[error("the update file is larger than expected")]
    TooBig,
    #[error(
        "the download does not match its checksum (expected {expected}, got {found}); \
         nothing was changed"
    )]
    Mismatch { expected: String, found: String },
    #[error("cannot {what} `{}`: {source}", path.display())]
    Io {
        what: &'static str,
        path: PathBuf,
        source: io::Error,
    },
    #[error("the update file is not a usable archive: {0}")]
    Archive(String),
    #[error("could not replace `{}`: {source}; the old version was put back", path.display())]
    RolledBack { path: PathBuf, source: io::Error },
}

fn io_err(what: &'static str, path: &Path) -> impl FnOnce(io::Error) -> Error {
    let path = path.to_path_buf();
    move |source| Error::Io { what, path, source }
}

/// A download that passed its checksum and is waiting to be installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Staged {
    kind: InstallKind,
    version: Version,
    file: PathBuf,
    /// The running program's file name, taken before any file is replaced (Linux appends
    /// " (deleted)" to the name of a replaced running binary).
    exe_name: OsString,
}

impl Staged {
    pub fn version(&self) -> Version {
        self.version
    }

    /// A download that never happened, for another crate's tests. Only built with the
    /// `test-support` feature, so a release build cannot make one without a checksum.
    #[cfg(feature = "test-support")]
    pub fn for_test(kind: InstallKind, version: Version) -> Self {
        Self {
            kind,
            version,
            file: PathBuf::from("update.bin"),
            exe_name: OsString::from("rwpm"),
        }
    }
}

/// What to start once the update is in place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Handoff {
    pub program: PathBuf,
    pub args: Vec<OsString>,
}

/// True when `release` carries what `kind` needs: its file and the checksums.
pub fn can_install(release: &Release, kind: &InstallKind) -> bool {
    kind.can_self_update()
        && release.asset(SUMS_NAME).is_some()
        && kind
            .asset_name(&release.version, Os::current())
            .is_some_and(|name| release.asset(&name).is_some())
}

/// Downloads the file for `kind` and checks it against the release's `SHA256SUMS`.
pub fn download(release: &Release, kind: &InstallKind) -> Result<Staged, Error> {
    if let Some(bad) = release
        .assets
        .iter()
        .find(|a| !a.url.starts_with(DOWNLOAD_PREFIX))
    {
        return Err(Error::BadUrl(bad.url.clone()));
    }
    fetch(release, kind, Os::current(), true)
}

fn fetch(release: &Release, kind: &InstallKind, os: Os, https_only: bool) -> Result<Staged, Error> {
    if !kind.can_self_update() {
        return Err(Error::Unsupported);
    }
    let name = kind
        .asset_name(&release.version, os)
        .ok_or(Error::Unsupported)?;
    let asset = release
        .asset(&name)
        .ok_or_else(|| Error::NoAsset(name.clone()))?;
    let sums = release
        .asset(SUMS_NAME)
        .ok_or_else(|| Error::NoAsset(SUMS_NAME.into()))?;
    let agent = format!("rwpm/{}", crate::VERSION);
    let headers = [("User-Agent", agent.as_str())];

    let text =
        http::get(&sums.url, https_only, SUMS_CAP, SUMS_TIMEOUT, &headers).map_err(request_err)?;
    let expected = checksum_for(&String::from_utf8_lossy(&text), &name)
        .ok_or_else(|| Error::NoChecksum(name.clone()))?;

    let file = staging_path(kind, &name);
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent).map_err(io_err("create", parent))?;
    }
    let mut out = File::create(&file).map_err(io_err("create", &file))?;
    let outcome = http::download_to(
        &asset.url,
        https_only,
        &mut out,
        DOWNLOAD_CAP,
        DOWNLOAD_TIMEOUT,
        &headers,
    )
    .map_err(request_err)
    .and_then(|found| {
        if found == expected {
            out.sync_all().map_err(io_err("write", &file))
        } else {
            Err(Error::Mismatch { expected, found })
        }
    });
    drop(out);
    if let Err(e) = outcome {
        // Never leave an unchecked file where an install could pick it up.
        let _ = fs::remove_file(&file);
        return Err(e);
    }
    Ok(Staged {
        kind: kind.clone(),
        version: release.version,
        file,
        exe_name: current_exe_name(),
    })
}

fn request_err(e: HttpError) -> Error {
    match e {
        HttpError::TooBig => Error::TooBig,
        HttpError::Failed(details) => Error::Request(details),
    }
}

fn current_exe_name() -> OsString {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.file_name().map(OsStr::to_os_string))
        .unwrap_or_else(|| OsString::from("rwpm"))
}

/// Where the download goes: on the same filesystem as what it replaces, so the final step is
/// a rename.
fn staging_path(kind: &InstallKind, name: &str) -> PathBuf {
    match kind {
        InstallKind::AppImage(file) => sibling(file, NEW_SUFFIX),
        InstallKind::Portable(dir) => dir.join(STAGE_DIR).join(name),
        _ => std::env::temp_dir().join(TEMP_DIR).join(name),
    }
}

fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// The hash `sums` lists for `name`. `sums` is `sha256sum` output: `<hex>  <name>`, with an
/// optional `*` before the name. The hash comes back lowercase.
pub fn checksum_for(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let (hash, file) = line.trim_end().split_once(char::is_whitespace)?;
        let file = file.trim_start().trim_start_matches('*');
        let valid = hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit());
        (valid && file == name).then(|| hash.to_ascii_lowercase())
    })
}

/// Puts the staged update in place and says what to start next. The running program keeps
/// working from its old files until it exits.
pub fn apply(staged: &Staged) -> Result<Handoff, Error> {
    match &staged.kind {
        InstallKind::AppImage(target) => apply_appimage(&staged.file, target),
        InstallKind::Portable(dir) => apply_portable(
            &staged.file,
            dir,
            &staged.exe_name,
            &|a, b| fs::rename(a, b),
            RENAME_WAIT,
        ),
        InstallKind::WindowsInstaller(_) => Ok(Handoff {
            program: staged.file.clone(),
            // The installer starts rwpm again, on success or not (see rwpm.nsi).
            args: vec!["/S".into(), "/RELAUNCH".into()],
        }),
        InstallKind::Managed | InstallKind::MacOs => Err(Error::Unsupported),
    }
}

/// Set on what `spawn` starts. The window that is closing still holds the open-file socket,
/// so the new one must not hand its launch over to it.
pub const AFTER_UPDATE_ENV: &str = "RWPM_AFTER_UPDATE";

/// Starts `handoff` and lets go of it, so it outlives this process.
pub fn spawn(handoff: &Handoff) -> io::Result<()> {
    Command::new(&handoff.program)
        .args(&handoff.args)
        .env(AFTER_UPDATE_ENV, "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(drop)
}

fn apply_appimage(staged: &Path, target: &Path) -> Result<Handoff, Error> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(staged, fs::Permissions::from_mode(0o755))
            .map_err(io_err("make executable", staged))?;
    }
    // The old image stays mounted for this process; the rename only swaps the directory entry.
    fs::rename(staged, target).map_err(io_err("replace", target))?;
    Ok(Handoff {
        program: target.to_path_buf(),
        args: Vec::new(),
    })
}

type Rename<'a> = &'a dyn Fn(&Path, &Path) -> io::Result<()>;

fn apply_portable(
    archive: &Path,
    dir: &Path,
    exe_name: &OsStr,
    rename: Rename<'_>,
    wait: Duration,
) -> Result<Handoff, Error> {
    let stage = dir.join(STAGE_DIR);
    let fresh = stage.join("new");
    let result = (|| {
        if fresh.exists() {
            fs::remove_dir_all(&fresh).map_err(io_err("remove", &fresh))?;
        }
        fs::create_dir_all(&fresh).map_err(io_err("create", &fresh))?;
        extract(archive, &fresh)?;
        if !fresh.join(exe_name).is_file() {
            return Err(Error::Archive(format!(
                "it has no `{}`",
                Path::new(exe_name).display()
            )));
        }
        install_tree(&fresh, dir, rename, wait)
    })();
    // The archive lives in the stage folder too, so this drops it as well.
    let _ = fs::remove_dir_all(&stage);
    result?;
    Ok(Handoff {
        program: dir.join(exe_name),
        args: Vec::new(),
    })
}

/// Removes what an earlier update left behind: its work folder, replaced `*.old` files, an
/// unused download. Best effort; a file that cannot be removed (a locked DLL on Windows) is
/// left for the next start.
pub fn cleanup(kind: &InstallKind) {
    match kind {
        InstallKind::Portable(dir) => {
            let _ = fs::remove_dir_all(dir.join(STAGE_DIR));
            remove_replaced_files(dir);
        }
        InstallKind::AppImage(file) => {
            let _ = fs::remove_file(sibling(file, NEW_SUFFIX));
        }
        InstallKind::WindowsInstaller(_) => {
            let _ = fs::remove_dir_all(std::env::temp_dir().join(TEMP_DIR));
        }
        InstallKind::Managed | InstallKind::MacOs => {}
    }
}

/// Deletes `x.old` when `x` exists beside it. A `.old` file with no partner is not ours.
fn remove_replaced_files(dir: &Path) {
    let mut pending = vec![dir.to_path_buf()];
    while let Some(next) = pending.pop() {
        let Ok(entries) = fs::read_dir(&next) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                pending.push(path);
            } else if let Some(partner) = path
                .to_str()
                .and_then(|p| p.strip_suffix(OLD_SUFFIX))
                .map(PathBuf::from)
                && partner.exists()
            {
                let _ = fs::remove_file(&path);
            }
        }
    }
}

// ---- unpacking ----

fn extract(archive: &Path, dest: &Path) -> Result<(), Error> {
    let name = archive.to_string_lossy();
    if name.ends_with(".zip") {
        extract_zip(archive, dest)
    } else if name.ends_with(".tar.gz") {
        extract_tar(archive, dest)
    } else {
        Err(Error::Archive(format!("`{name}` is not a .zip or .tar.gz")))
    }
}

/// The path inside the archive without its top folder (`rwpm-1.18.0-linux-x86_64/`). `None`
/// for the top folder itself. Anything that could leave the target folder is an error.
fn inner_path(raw: &Path) -> Result<Option<PathBuf>, Error> {
    let mut parts = Vec::new();
    for part in raw.components() {
        match part {
            Component::Normal(p) => parts.push(p),
            Component::CurDir => {}
            _ => return Err(Error::Archive(format!("unsafe path `{}`", raw.display()))),
        }
    }
    let rest: PathBuf = parts.into_iter().skip(1).collect();
    Ok((!rest.as_os_str().is_empty()).then_some(rest))
}

/// Copies `from` to `to`, drawing down `budget`; an archive that unpacks past it is refused.
fn copy_within(from: &mut impl Read, to: &mut impl Write, budget: &mut u64) -> Result<(), Error> {
    let copied = io::copy(&mut from.take(budget.saturating_add(1)), to)
        .map_err(|e| Error::Archive(e.to_string()))?;
    if copied > *budget {
        return Err(Error::Archive("it unpacks to more than expected".into()));
    }
    *budget -= copied;
    Ok(())
}

fn make_parent(path: &Path) -> Result<(), Error> {
    match path.parent() {
        Some(parent) => fs::create_dir_all(parent).map_err(io_err("create", parent)),
        None => Ok(()),
    }
}

fn extract_tar(archive: &Path, dest: &Path) -> Result<(), Error> {
    let file = File::open(archive).map_err(io_err("open", archive))?;
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(file));
    let bad = |e: io::Error| Error::Archive(e.to_string());
    let mut budget = UNPACK_CAP;
    for entry in tar.entries().map_err(bad)? {
        let mut entry = entry.map_err(bad)?;
        let raw = entry.path().map_err(bad)?.into_owned();
        let Some(inner) = inner_path(&raw)? else {
            continue;
        };
        let target = dest.join(inner);
        let kind = entry.header().entry_type();
        if kind.is_dir() {
            fs::create_dir_all(&target).map_err(io_err("create", &target))?;
            continue;
        }
        if !kind.is_file() {
            return Err(Error::Archive(format!(
                "unexpected entry `{}`",
                raw.display()
            )));
        }
        make_parent(&target)?;
        let mut out = File::create(&target).map_err(io_err("create", &target))?;
        copy_within(&mut entry, &mut out, &mut budget)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = entry.header().mode().map_err(bad)? & 0o777;
            out.set_permissions(fs::Permissions::from_mode(mode))
                .map_err(io_err("set permissions", &target))?;
        }
    }
    Ok(())
}

fn extract_zip(archive: &Path, dest: &Path) -> Result<(), Error> {
    let file = File::open(archive).map_err(io_err("open", archive))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| Error::Archive(e.to_string()))?;
    let mut budget = UNPACK_CAP;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| Error::Archive(e.to_string()))?;
        let raw = entry
            .enclosed_name()
            .ok_or_else(|| Error::Archive("an entry has an unsafe path".into()))?;
        let Some(inner) = inner_path(&raw)? else {
            continue;
        };
        let target = dest.join(inner);
        if entry.is_dir() {
            fs::create_dir_all(&target).map_err(io_err("create", &target))?;
            continue;
        }
        if entry.is_symlink() {
            return Err(Error::Archive(format!(
                "unexpected entry `{}`",
                raw.display()
            )));
        }
        make_parent(&target)?;
        let mut out = File::create(&target).map_err(io_err("create", &target))?;
        copy_within(&mut entry, &mut out, &mut budget)?;
    }
    Ok(())
}

// ---- all-or-nothing replace ----

/// One change to the install folder, kept so it can be undone.
enum Step {
    /// `from` was renamed to `to` to make room.
    Moved { from: PathBuf, to: PathBuf },
    /// A new file was put at this path.
    Placed(PathBuf),
    /// A folder was created.
    MadeDir(PathBuf),
}

/// Moves every file under `src` to the same relative path under `dest`. A file that is already
/// there is first renamed to `<name>.old` (Windows lets a running program be renamed but not
/// overwritten). If any step fails, the ones done so far are undone, in reverse.
fn install_tree(src: &Path, dest: &Path, rename: Rename<'_>, wait: Duration) -> Result<(), Error> {
    let mut journal = Vec::new();
    for rel in list_files(src)? {
        if let Err((path, source)) = place(
            &src.join(&rel),
            &dest.join(&rel),
            rename,
            wait,
            &mut journal,
        ) {
            undo(&journal);
            return Err(Error::RolledBack { path, source });
        }
    }
    for step in &journal {
        if let Step::Moved { to, .. } = step {
            let _ = fs::remove_file(to);
        }
    }
    Ok(())
}

fn place(
    from: &Path,
    to: &Path,
    rename: Rename<'_>,
    wait: Duration,
    journal: &mut Vec<Step>,
) -> Result<(), (PathBuf, io::Error)> {
    let fail = |path: &Path| {
        let path = path.to_path_buf();
        move |e| (path, e)
    };
    if let Some(parent) = to.parent() {
        let mut missing = Vec::new();
        let mut at = parent;
        while !at.exists() {
            missing.push(at.to_path_buf());
            match at.parent() {
                Some(up) => at = up,
                None => break,
            }
        }
        for dir in missing.into_iter().rev() {
            fs::create_dir(&dir).map_err(fail(&dir))?;
            journal.push(Step::MadeDir(dir));
        }
    }
    if to.symlink_metadata().is_ok() {
        let old = sibling(to, OLD_SUFFIX);
        let _ = fs::remove_file(&old);
        retry(|| rename(to, &old), wait).map_err(fail(to))?;
        journal.push(Step::Moved {
            from: to.to_path_buf(),
            to: old,
        });
    }
    retry(|| rename(from, to), wait).map_err(fail(to))?;
    journal.push(Step::Placed(to.to_path_buf()));
    Ok(())
}

fn undo(journal: &[Step]) {
    for step in journal.iter().rev() {
        let _ = match step {
            Step::Placed(path) => fs::remove_file(path),
            Step::Moved { from, to } => fs::rename(to, from),
            Step::MadeDir(dir) => fs::remove_dir(dir),
        };
    }
}

/// Runs `f` up to [`RENAME_TRIES`] times. Antivirus or an indexer can hold a new file for a
/// moment right after it is written.
fn retry(mut f: impl FnMut() -> io::Result<()>, wait: Duration) -> io::Result<()> {
    let mut attempt = 1;
    loop {
        match f() {
            Ok(()) => return Ok(()),
            Err(e) if attempt >= RENAME_TRIES => return Err(e),
            Err(_) => {
                attempt += 1;
                std::thread::sleep(wait);
            }
        }
    }
}

/// Every file under `root`, as paths relative to it, in a stable order.
fn list_files(root: &Path) -> Result<Vec<PathBuf>, Error> {
    let mut files = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(&dir).map_err(io_err("read", &dir))? {
            let entry = entry.map_err(io_err("read", &dir))?;
            let path = entry.path();
            if entry.file_type().map_err(io_err("read", &path))?.is_dir() {
                pending.push(path);
            } else if let Ok(rel) = path.strip_prefix(root) {
                files.push(rel.to_path_buf());
            }
        }
    }
    files.sort();
    Ok(files)
}

#[cfg(test)]
mod tests;
