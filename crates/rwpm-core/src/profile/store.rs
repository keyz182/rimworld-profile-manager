//! Profile store: the Default profile plus one dir per profile under the profiles root.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::discovery::Engine;
use crate::error::ProfileError;
use crate::profile::file::{FILE_NAME, Mode, ProfileFile};
use crate::profile::id::ProfileId;
use crate::profile::running::RunningProbe;

const SAVE_DATA: &str = "SaveData";
const MODS: &str = "Mods";
const PLAYER_LOG: &str = "Player.log";
/// R41: Waylith's `FileLog` writes `<dir>/Logs/Player.log`.
const WAYLITH_LOG_DIR: &str = "Logs";
const DEFAULT_NAME: &str = "Default";
const MAX_SUFFIX: u32 = 999;
/// Top-level files in the game's own save dir that a clone of Default leaves behind:
/// Unity logs, and Steam Cloud's marker (the clone is not cloud-synced).
const DEFAULT_CLONE_SKIP: [&str; 3] = ["Player.log", "Player-prev.log", "steam_autocloud.vdf"];

/// Moves a path to the OS recycle bin. A trait so tests never touch the real one.
pub trait Trash {
    fn trash(&self, path: &Path) -> std::io::Result<()>;
}

pub struct OsTrash;

impl Trash for OsTrash {
    fn trash(&self, path: &Path) -> std::io::Result<()> {
        trash::delete(path).map_err(std::io::Error::other)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Location {
    /// Built-in: the game's own save dir, launched without `-savedatafolder` (V10).
    Default {
        save_dir: PathBuf,
    },
    Stored {
        dir: PathBuf,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Profile {
    pub id: ProfileId,
    pub name: String,
    pub mode: Mode,
    /// Key of the stored game build this profile is pinned to; `None` = the Steam
    /// install. Default is never pinned (V54).
    pub game_build: Option<String>,
    pub location: Location,
}

impl Profile {
    pub fn is_default(&self) -> bool {
        matches!(self.location, Location::Default { .. })
    }

    pub fn save_data_dir(&self) -> PathBuf {
        match &self.location {
            Location::Default { save_dir } => save_dir.clone(),
            Location::Stored { dir } => dir.join(SAVE_DATA),
        }
    }

    /// `None` for Default (V10); otherwise one argv token without a trailing separator (V4).
    pub fn savedatafolder_arg(&self) -> Option<OsString> {
        self.stored_dir()
            .map(|dir| savedatafolder_arg(&dir.join(SAVE_DATA)))
    }

    pub fn mods_dir(&self) -> Option<PathBuf> {
        self.stored_dir().map(|dir| dir.join(MODS))
    }

    /// The `-logFile` launch argument (V15); `None` for Default, which is launched without it.
    pub fn log_file(&self) -> Option<PathBuf> {
        self.stored_dir().map(|dir| dir.join(PLAYER_LOG))
    }

    /// Where the game's log for this profile is, per engine (V67).
    /// Unity: ours (`-logFile`) for stored profiles, Unity's own default
    /// (`<game save dir>/Player.log`) for Default (V15).
    /// Waylith: `<folder>/Logs/Player.log`, the folder being the profile's (startup hook,
    /// `RWPM_LOG_DIR`) or Default's save dir (the engine's own default, R41).
    pub fn log_path(&self, engine: Engine) -> PathBuf {
        let dir = match &self.location {
            Location::Default { save_dir } => save_dir,
            Location::Stored { dir } => dir,
        };
        match engine {
            Engine::Unity => dir.join(PLAYER_LOG),
            Engine::Waylith => dir.join(WAYLITH_LOG_DIR).join(PLAYER_LOG),
        }
    }

    /// The profile's own folder; `None` for Default, which has none.
    pub fn stored_dir(&self) -> Option<&Path> {
        match &self.location {
            Location::Default { .. } => None,
            Location::Stored { dir } => Some(dir),
        }
    }
}

/// `-savedatafolder=<dir>` with any trailing separator removed (GenFilePaths.cs:101 never trims).
pub fn savedatafolder_arg(dir: &Path) -> OsString {
    // Re-collecting components drops a trailing separator without touching the rest.
    let normalised: PathBuf = dir.components().collect();
    let mut arg = OsString::from("-savedatafolder=");
    arg.push(normalised.as_os_str());
    arg
}

/// Result of `list`: readable profiles, plus ones whose `profile.toml` could not be read.
#[derive(Debug)]
pub struct Listing {
    pub profiles: Vec<Profile>,
    pub unreadable: Vec<(ProfileId, ProfileError)>,
}

pub struct ProfileStore<T: Trash> {
    root: PathBuf,
    game_save_dir: PathBuf,
    trash: T,
}

impl<T: Trash> ProfileStore<T> {
    /// Root must be absolute, UTF-8 and free of `=` (V4). Created if missing.
    pub fn open(root: PathBuf, game_save_dir: PathBuf, trash: T) -> Result<Self, ProfileError> {
        if !root.is_absolute() {
            return Err(ProfileError::RootNotAbsolute(root));
        }
        let text = root
            .to_str()
            .ok_or_else(|| ProfileError::NonUtf8(root.clone()))?;
        if text.contains('=') {
            return Err(ProfileError::PathHasEquals(root));
        }
        std::fs::create_dir_all(&root).map_err(ProfileError::io("create", &root))?;
        Ok(Self {
            root,
            game_save_dir,
            trash,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Default first, then stored profiles by name. Dirs without `profile.toml` are not profiles.
    pub fn list(&self) -> Result<Listing, ProfileError> {
        let mut listing = Listing {
            profiles: vec![self.default_profile()],
            unreadable: Vec::new(),
        };
        let entries =
            std::fs::read_dir(&self.root).map_err(ProfileError::io("list", &self.root))?;
        let mut stored = Vec::new();
        for entry in entries {
            let entry = entry.map_err(ProfileError::io("list", &self.root))?;
            let Some(id) = entry
                .file_name()
                .to_str()
                .and_then(|n| ProfileId::parse(n).ok())
            else {
                continue;
            };
            if id.is_default() || !entry.path().join(FILE_NAME).is_file() {
                continue;
            }
            match self.load(&id) {
                Ok(p) => stored.push(p),
                Err(e) => listing.unreadable.push((id, e)),
            }
        }
        stored.sort_by(|a, b| {
            a.name
                .to_lowercase()
                .cmp(&b.name.to_lowercase())
                .then_with(|| a.id.cmp(&b.id))
        });
        listing.profiles.extend(stored);
        Ok(listing)
    }

    pub fn get(&self, id: &ProfileId) -> Result<Profile, ProfileError> {
        if id.is_default() {
            return Ok(self.default_profile());
        }
        if !self.root.join(id.as_str()).join(FILE_NAME).is_file() {
            return Err(ProfileError::NotFound(id.to_string()));
        }
        self.load(id)
    }

    /// Claims a fresh id with an atomic `create_dir`; any existing dir counts as taken.
    pub fn create(&self, name: &str) -> Result<Profile, ProfileError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(ProfileError::EmptyName);
        }
        let (id, dir) = self.claim(&ProfileId::slug_base(name))?;
        let file = ProfileFile::new(name);
        let populate = || -> Result<(), ProfileError> {
            let save = dir.join(SAVE_DATA);
            std::fs::create_dir(&save).map_err(ProfileError::io("create", &save))?;
            file.write_atomic(&dir.join(FILE_NAME))
        };
        if let Err(e) = populate() {
            // Only what this call created lives in `dir`; undo the claim.
            let _ = std::fs::remove_dir_all(&dir);
            return Err(e);
        }
        Ok(Self::profile(id, dir, file))
    }

    /// Copies `source` (SaveData, and Mods with folder names kept, V12) into a new profile
    /// called `name`. Refused while any RimWorld runs: a save mid-write would copy torn.
    /// `profile.toml` is written last, so a partial clone is never listed.
    pub fn clone_profile(
        &self,
        source: &ProfileId,
        name: &str,
        probe: &dyn RunningProbe,
        progress: &mut dyn FnMut(u64),
    ) -> Result<Profile, ProfileError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(ProfileError::EmptyName);
        }
        let src = self.get(source)?;
        if probe.any_game() {
            return Err(ProfileError::GameRunning);
        }
        let (id, dir) = self.claim(&ProfileId::slug_base(name))?;
        let mut populate = || -> Result<ProfileFile, ProfileError> {
            let mut copy = |from: &Path, to: &Path, skip: &dyn Fn(&Path) -> bool| {
                crate::fs_copy::copy_tree(from, to, skip, progress)
                    .map_err(ProfileError::io("copy", from))
            };
            match &src.location {
                Location::Default { save_dir } => {
                    let skip = |rel: &Path| DEFAULT_CLONE_SKIP.iter().any(|f| rel == Path::new(f));
                    copy(save_dir, &dir.join(SAVE_DATA), &skip)?;
                    Ok(ProfileFile::new(name))
                }
                Location::Stored { dir: src_dir } => {
                    copy(&src_dir.join(SAVE_DATA), &dir.join(SAVE_DATA), &|_| false)?;
                    if src_dir.join(MODS).is_dir() {
                        copy(&src_dir.join(MODS), &dir.join(MODS), &|_| false)?;
                    }
                    let mut file = ProfileFile::read(&src_dir.join(FILE_NAME))?;
                    file.name = name.to_owned();
                    Ok(file)
                }
            }
        };
        let file = populate().and_then(|file| {
            file.write_atomic(&dir.join(FILE_NAME))?;
            Ok(file)
        });
        match file {
            Ok(file) => Ok(Self::profile(id, dir, file)),
            Err(e) => {
                // Only what this call created lives in `dir`; undo the claim.
                let _ = std::fs::remove_dir_all(&dir);
                Err(e)
            }
        }
    }

    /// Changes the display name only; id and dir stay (V19).
    pub fn rename(&self, id: &ProfileId, new_name: &str) -> Result<Profile, ProfileError> {
        if id.is_default() {
            return Err(ProfileError::DefaultImmutable("renamed"));
        }
        let new_name = new_name.trim();
        if new_name.is_empty() {
            return Err(ProfileError::EmptyName);
        }
        let dir = self
            .get(id)?
            .stored_dir()
            .map(Path::to_path_buf)
            .ok_or(ProfileError::DefaultImmutable("renamed"))?;
        let path = dir.join(FILE_NAME);
        let mut file = ProfileFile::read(&path)?;
        file.name = new_name.to_owned();
        file.write_atomic(&path)?;
        Ok(Self::profile(id.clone(), dir, file))
    }

    /// Pins the profile to a stored game build, or clears the pin with `None` (§I.file).
    /// V54: Default cannot be pinned — it has no `profile.toml` to hold the key, and it
    /// shares the game's own save folder.
    pub fn set_pin(&self, id: &ProfileId, key: Option<&str>) -> Result<Profile, ProfileError> {
        if id.is_default() {
            return Err(ProfileError::DefaultImmutable("pinned to a game version"));
        }
        let dir = self
            .get(id)?
            .stored_dir()
            .map(Path::to_path_buf)
            .ok_or(ProfileError::DefaultImmutable("pinned to a game version"))?;
        let path = dir.join(FILE_NAME);
        let mut file = ProfileFile::read(&path)?;
        file.game_build = key.map(ToOwned::to_owned);
        file.write_atomic(&path)?;
        Ok(Self::profile(id.clone(), dir, file))
    }

    /// Moves the profile dir to the recycle bin. Refuses Default (V10) and running profiles (V27).
    pub fn delete(&self, id: &ProfileId, probe: &dyn RunningProbe) -> Result<(), ProfileError> {
        if id.is_default() {
            return Err(ProfileError::DefaultImmutable("deleted"));
        }
        let profile = self.get(id)?;
        if probe.is_running(&profile.save_data_dir()) {
            return Err(ProfileError::Running(id.to_string()));
        }
        let dir = self.root.join(id.as_str());
        self.trash
            .trash(&dir)
            .map_err(ProfileError::io("move to recycle bin", &dir))
    }

    #[cfg(test)]
    pub(crate) fn trash_ref(&self) -> &T {
        &self.trash
    }

    /// Moves `path` to the OS recycle bin (crate-internal: OFFLINE swaps old copies out).
    pub(crate) fn trash_path(&self, path: &Path) -> Result<(), ProfileError> {
        self.trash
            .trash(path)
            .map_err(ProfileError::io("move to recycle bin", path))
    }

    fn claim(&self, base: &str) -> Result<(ProfileId, PathBuf), ProfileError> {
        for n in 1..=MAX_SUFFIX {
            let id = ProfileId::candidate(base, n);
            // `default` is the built-in profile; `builds` is the game-build store (V56).
            if id.is_default() || id.is_builds() {
                continue;
            }
            let dir = self.root.join(id.as_str());
            match std::fs::create_dir(&dir) {
                Ok(()) => return Ok((id, dir)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(ProfileError::io("create", &dir)(e)),
            }
        }
        Err(ProfileError::NoFreeId(base.to_owned()))
    }

    fn load(&self, id: &ProfileId) -> Result<Profile, ProfileError> {
        let dir = self.root.join(id.as_str());
        let file = ProfileFile::read(&dir.join(FILE_NAME))?;
        Ok(Self::profile(id.clone(), dir, file))
    }

    fn profile(id: ProfileId, dir: PathBuf, file: ProfileFile) -> Profile {
        Profile {
            id,
            name: file.name,
            mode: file.mode,
            game_build: file.game_build,
            location: Location::Stored { dir },
        }
    }

    fn default_profile(&self) -> Profile {
        Profile {
            id: ProfileId::default_id(),
            name: DEFAULT_NAME.to_owned(),
            mode: Mode::Live,
            // V54: Default shares the game's own save folder and cannot be pinned.
            game_build: None,
            location: Location::Default {
                save_dir: self.game_save_dir.clone(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;
    use crate::profile::file::FILE_NAME;

    #[derive(Default)]
    struct RecordingTrash(RefCell<Vec<PathBuf>>);

    impl Trash for &RecordingTrash {
        fn trash(&self, path: &Path) -> std::io::Result<()> {
            self.0.borrow_mut().push(path.to_path_buf());
            Ok(())
        }
    }

    struct FakeProbe(bool);

    impl RunningProbe for FakeProbe {
        fn is_running(&self, _: &Path) -> bool {
            self.0
        }

        fn any_game(&self) -> bool {
            self.0
        }
    }

    fn no_progress() -> impl FnMut(u64) {
        |_| {}
    }

    fn write(path: &Path, body: &str) {
        std::fs::create_dir_all(path.parent().expect("parent")).expect("dirs");
        std::fs::write(path, body).expect("write");
    }

    fn frozen(folder: &str) -> crate::profile::FrozenMod {
        crate::profile::FrozenMod {
            folder: folder.to_owned(),
            package_id: "brrainz.harmony".to_owned(),
            source: crate::profile::ModSource::Workshop,
            source_dir: PathBuf::from("/ws").join(folder),
            copied_at: "2026-10-11T19:00:00Z".parse().expect("datetime"),
        }
    }

    /// An OFFLINE profile with a save, a frozen mod and its logs.
    fn offline_source(s: &ProfileStore<&RecordingTrash>) -> Profile {
        let p = s.create("Source Run").expect("create");
        let dir = s.root().join(p.id.as_str());
        write(&dir.join("SaveData/Saves/colony.rws"), "save");
        write(&dir.join("Mods/2009463077/About/About.xml"), "about");
        write(&dir.join("Player.log"), "log");
        write(&dir.join("payload.log"), "log");
        let mut file = ProfileFile::read(&dir.join(FILE_NAME)).expect("read");
        file.mode = Mode::Offline;
        file.frozen_mods = vec![frozen("2009463077")];
        file.write_atomic(&dir.join(FILE_NAME)).expect("write");
        p
    }

    /// §I.cmd: `builds` is reserved for the game-build store, so no new profile takes it.
    #[test]
    fn a_profile_named_builds_gets_the_next_free_id() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let trash = RecordingTrash::default();
        let s = store(tmp.path(), &trash);
        let p = s.create("Builds").expect("create");
        assert_eq!(p.id.as_str(), "builds-2");
        assert!(!s.root().join("builds").exists());
    }

    #[test]
    fn clone_copies_savedata_and_mods_keeping_folder_names() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let trash = RecordingTrash::default();
        let s = store(tmp.path(), &trash);
        let src = offline_source(&s);
        let c = s
            .clone_profile(&src.id, "Copy", &FakeProbe(false), &mut no_progress())
            .expect("clone");
        let dir = s.root().join(c.id.as_str());
        assert_eq!(c.id.as_str(), "copy");
        let read = |rel: &str| std::fs::read_to_string(dir.join(rel)).expect(rel);
        assert_eq!(read("SaveData/Saves/colony.rws"), "save");
        // V12: same folder name as the source copy.
        assert_eq!(read("Mods/2009463077/About/About.xml"), "about");
        assert!(!dir.join("Player.log").exists() && !dir.join("payload.log").exists());
    }

    #[test]
    fn clone_keeps_mode_and_frozen_mods_with_new_name() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let trash = RecordingTrash::default();
        let s = store(tmp.path(), &trash);
        let src = offline_source(&s);
        let c = s
            .clone_profile(&src.id, "  Copy  ", &FakeProbe(false), &mut no_progress())
            .expect("clone");
        let file = ProfileFile::read(&s.root().join(c.id.as_str()).join(FILE_NAME)).expect("read");
        assert_eq!((file.name.as_str(), file.mode), ("Copy", Mode::Offline));
        assert_eq!(file.frozen_mods, vec![frozen("2009463077")]);
        assert_eq!(s.get(&src.id).expect("source").name, "Source Run");
    }

    #[test]
    fn clone_default_skips_logs_and_cloud_marker_and_is_live() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let trash = RecordingTrash::default();
        let s = store(tmp.path(), &trash);
        let game = tmp.path().join("game-save");
        for f in [
            "Config/Prefs.xml",
            "Saves/a.rws",
            "Player.log",
            "Player-prev.log",
            "steam_autocloud.vdf",
            "prefs",
        ] {
            write(&game.join(f), f);
        }
        let mut bytes = 0;
        let c = s
            .clone_profile(
                &ProfileId::default_id(),
                "From Vanilla",
                &FakeProbe(false),
                &mut |n| bytes = n,
            )
            .expect("clone");
        let save = s.root().join(c.id.as_str()).join(SAVE_DATA);
        for kept in ["Config/Prefs.xml", "Saves/a.rws", "prefs"] {
            assert!(save.join(kept).is_file(), "{kept}");
        }
        for gone in ["Player.log", "Player-prev.log", "steam_autocloud.vdf"] {
            assert!(!save.join(gone).exists(), "{gone}");
        }
        assert_eq!(c.mode, Mode::Live);
        assert!(!s.root().join(c.id.as_str()).join(MODS).exists());
        assert_eq!(
            bytes,
            ("Config/Prefs.xml".len() + "Saves/a.rws".len() + 5) as u64
        );
    }

    #[test]
    fn clone_refuses_while_any_game_runs() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let trash = RecordingTrash::default();
        let s = store(tmp.path(), &trash);
        for source in [ProfileId::default_id(), offline_source(&s).id] {
            assert!(matches!(
                s.clone_profile(&source, "Copy", &FakeProbe(true), &mut no_progress()),
                Err(ProfileError::GameRunning)
            ));
        }
        assert!(!s.root().join("copy").exists(), "nothing claimed");
    }

    #[cfg(unix)]
    #[test]
    fn clone_failure_removes_partial_dir_and_is_never_listed() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().expect("tempdir");
        let trash = RecordingTrash::default();
        let s = store(tmp.path(), &trash);
        let src = offline_source(&s);
        let locked = s.root().join(src.id.as_str()).join("SaveData/Saves");
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).expect("chmod");
        let result = s.clone_profile(&src.id, "Copy", &FakeProbe(false), &mut no_progress());
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        if nix_is_root() {
            return; // root ignores permissions; nothing to prove.
        }
        assert!(matches!(result, Err(ProfileError::Io { .. })), "{result:?}");
        assert!(!s.root().join("copy").exists(), "partial clone left behind");
        let names: Vec<_> = s
            .list()
            .expect("list")
            .profiles
            .into_iter()
            .map(|p| p.name)
            .collect();
        assert_eq!(names, ["Default", "Source Run"]);
    }

    #[cfg(unix)]
    fn nix_is_root() -> bool {
        std::fs::read_to_string("/proc/self/status")
            .map(|s| {
                s.lines()
                    .any(|l| l.starts_with("Uid:") && l.split_whitespace().nth(1) == Some("0"))
            })
            .unwrap_or(false)
    }

    fn store<'t>(dir: &Path, trash: &'t RecordingTrash) -> ProfileStore<&'t RecordingTrash> {
        ProfileStore::open(dir.join("profiles"), dir.join("game-save"), trash).expect("open")
    }

    #[test]
    fn create_writes_profile_toml_and_savedata() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let trash = RecordingTrash::default();
        let p = store(tmp.path(), &trash)
            .create("My Modded Run")
            .expect("create");
        assert_eq!(p.id.as_str(), "my-modded-run");
        let dir = tmp.path().join("profiles/my-modded-run");
        assert!(dir.join(SAVE_DATA).is_dir());
        let file = ProfileFile::read(&dir.join(FILE_NAME)).expect("read");
        assert_eq!(
            (file.name.as_str(), file.mode),
            ("My Modded Run", Mode::Live)
        );
    }

    #[test]
    fn create_claim_is_atomic() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let trash = RecordingTrash::default();
        let s = store(tmp.path(), &trash);
        assert_eq!(s.create("Run").expect("1").id.as_str(), "run");
        assert_eq!(s.create("run").expect("2").id.as_str(), "run-2");
        assert_eq!(s.create("RUN!").expect("3").id.as_str(), "run-3");
    }

    #[test]
    fn create_skips_existing_dir_without_toml() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let trash = RecordingTrash::default();
        let s = store(tmp.path(), &trash);
        std::fs::create_dir_all(s.root().join("my-run/stray")).expect("stray dir");
        let p = s.create("My Run").expect("create");
        assert_eq!(p.id.as_str(), "my-run-2");
        assert!(
            s.root().join("my-run/stray").is_dir(),
            "stray files untouched"
        );
        assert!(!s.root().join("my-run").join(FILE_NAME).exists());
    }

    #[test]
    fn create_never_uses_reserved_default_id() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let trash = RecordingTrash::default();
        let p = store(tmp.path(), &trash).create("Default").expect("create");
        assert_eq!(p.id.as_str(), "default-2");
    }

    #[test]
    fn create_rejects_blank_name() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let trash = RecordingTrash::default();
        assert!(matches!(
            store(tmp.path(), &trash).create("   "),
            Err(ProfileError::EmptyName)
        ));
    }

    #[test]
    fn rename_changes_name_only() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let trash = RecordingTrash::default();
        let s = store(tmp.path(), &trash);
        let p = s.create("Old Name").expect("create");
        let renamed = s.rename(&p.id, "New Name").expect("rename");
        assert_eq!(renamed.id, p.id);
        assert_eq!(renamed.location, p.location);
        assert_eq!(renamed.name, "New Name");
        assert_eq!(s.get(&p.id).expect("get").name, "New Name");
    }

    #[test]
    fn delete_trashes_dir() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let trash = RecordingTrash::default();
        let s = store(tmp.path(), &trash);
        let p = s.create("Gone").expect("create");
        s.delete(&p.id, &FakeProbe(false)).expect("delete");
        assert_eq!(*trash.0.borrow(), vec![s.root().join("gone")]);
    }

    #[test]
    fn delete_refuses_while_running() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let trash = RecordingTrash::default();
        let s = store(tmp.path(), &trash);
        let p = s.create("Busy").expect("create");
        assert!(matches!(
            s.delete(&p.id, &FakeProbe(true)),
            Err(ProfileError::Running(_))
        ));
        assert!(trash.0.borrow().is_empty());
    }

    #[test]
    fn default_cannot_be_deleted_or_renamed() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let trash = RecordingTrash::default();
        let s = store(tmp.path(), &trash);
        let id = ProfileId::default_id();
        assert!(matches!(
            s.delete(&id, &FakeProbe(false)),
            Err(ProfileError::DefaultImmutable(_))
        ));
        assert!(matches!(
            s.rename(&id, "Vanilla"),
            Err(ProfileError::DefaultImmutable(_))
        ));
        assert!(trash.0.borrow().is_empty());
    }

    #[test]
    fn default_has_no_savedatafolder_arg() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let trash = RecordingTrash::default();
        let d = store(tmp.path(), &trash)
            .get(&ProfileId::default_id())
            .expect("default");
        assert!(d.is_default());
        assert_eq!(d.mode, Mode::Live);
        assert_eq!(d.savedatafolder_arg(), None);
        assert_eq!(d.save_data_dir(), tmp.path().join("game-save"));
        assert_eq!(
            d.log_path(Engine::Unity),
            tmp.path().join("game-save").join(PLAYER_LOG)
        );
    }

    #[test]
    fn savedatafolder_arg_is_single_token_no_trailing_sep() {
        let arg = savedatafolder_arg(Path::new("/p/run/SaveData/"));
        assert_eq!(arg, OsString::from("-savedatafolder=/p/run/SaveData"));
        let tmp = tempfile::tempdir().expect("tempdir");
        let trash = RecordingTrash::default();
        let p = store(tmp.path(), &trash).create("Run").expect("create");
        let expected = format!(
            "-savedatafolder={}",
            tmp.path().join("profiles/run/SaveData").display()
        );
        assert_eq!(p.savedatafolder_arg(), Some(OsString::from(expected)));
    }

    #[test]
    fn root_with_equals_rejected() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let trash = RecordingTrash::default();
        let bad = ProfileStore::open(tmp.path().join("a=b"), tmp.path().join("g"), &trash);
        assert!(matches!(bad, Err(ProfileError::PathHasEquals(_))));
        let rel = ProfileStore::open(PathBuf::from("relative"), tmp.path().join("g"), &trash);
        assert!(matches!(rel, Err(ProfileError::RootNotAbsolute(_))));
    }

    #[test]
    fn list_has_default_first_and_skips_dirs_without_toml() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let trash = RecordingTrash::default();
        let s = store(tmp.path(), &trash);
        s.create("Zeta").expect("z");
        s.create("Alpha").expect("a");
        std::fs::create_dir(s.root().join("not-a-profile")).expect("stray");
        let names: Vec<_> = s
            .list()
            .expect("list")
            .profiles
            .into_iter()
            .map(|p| p.name)
            .collect();
        assert_eq!(names, ["Default", "Alpha", "Zeta"]);
    }

    #[test]
    fn list_reports_unreadable_profile_without_hiding_others() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let trash = RecordingTrash::default();
        let s = store(tmp.path(), &trash);
        s.create("Good").expect("good");
        let bad = s.create("Bad").expect("bad");
        std::fs::write(s.root().join("bad").join(FILE_NAME), "not = [toml").expect("corrupt");
        let listing = s.list().expect("list");
        assert_eq!(listing.profiles.len(), 2, "Default + Good");
        assert_eq!(listing.unreadable.len(), 1);
        assert_eq!(listing.unreadable[0].0, bad.id);
    }

    #[test]
    fn offline_profile_paths() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let trash = RecordingTrash::default();
        let p = store(tmp.path(), &trash).create("Run").expect("create");
        let dir = tmp.path().join("profiles/run");
        assert_eq!(p.mods_dir(), Some(dir.join(MODS)));
        assert_eq!(p.log_file(), Some(dir.join(PLAYER_LOG)));
    }

    #[test]
    fn v67_log_path_per_engine() {
        let dir = PathBuf::from("/profiles/p");
        let save = PathBuf::from("/home/u/.config/unity3d/x");
        let stored = Profile {
            id: ProfileId::parse("p").expect("id"),
            name: "P".into(),
            mode: Mode::Live,
            game_build: None,
            location: Location::Stored { dir: dir.clone() },
        };
        let default = Profile {
            id: ProfileId::default_id(),
            name: "Default".into(),
            mode: Mode::Live,
            game_build: None,
            location: Location::Default {
                save_dir: save.clone(),
            },
        };
        assert_eq!(stored.log_path(Engine::Unity), dir.join("Player.log"));
        assert_eq!(default.log_path(Engine::Unity), save.join("Player.log"));
        assert_eq!(
            stored.log_path(Engine::Waylith),
            dir.join("Logs/Player.log")
        );
        assert_eq!(
            default.log_path(Engine::Waylith),
            save.join("Logs/Player.log")
        );
    }
}
