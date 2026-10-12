use std::cell::RefCell;
use std::path::{Path, PathBuf};

use super::*;
use crate::profile::{ProfileId, RunningProbe};

/// A recycle bin that moves things into `<tmp>/bin/` and remembers what it got.
struct Bin {
    dir: PathBuf,
    got: RefCell<Vec<PathBuf>>,
}

impl Trash for Bin {
    fn trash(&self, path: &Path) -> std::io::Result<()> {
        let n = self.got.borrow().len();
        std::fs::create_dir_all(&self.dir)?;
        std::fs::rename(path, self.dir.join(n.to_string()))?;
        self.got.borrow_mut().push(path.to_path_buf());
        Ok(())
    }
}

struct Probe(bool);

impl RunningProbe for Probe {
    fn is_running(&self, _: &Path) -> bool {
        self.0
    }
    fn any_game(&self) -> bool {
        self.0
    }
}

struct World {
    tmp: tempfile::TempDir,
    store: ProfileStore<Bin>,
}

impl World {
    fn new() -> Self {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = ProfileStore::open(
            tmp.path().join("profiles"),
            tmp.path().join("game-save"),
            Bin {
                dir: tmp.path().join("bin"),
                got: RefCell::new(Vec::new()),
            },
        )
        .expect("store");
        Self { tmp, store }
    }

    /// The game's own save folder (Default), filled like a played install.
    fn default_with_everything(&self) -> Profile {
        let d = self.tmp.path().join("game-save");
        for (rel, body) in [
            ("Config/ModsConfig.xml", "mods"),
            ("Config/Prefs.xml", "prefs"),
            ("Config/KeyPrefs.xml", "keys"),
            ("Config/Mod_1_Harmony.xml", "harmony settings"),
            ("Config/TotalControlData/x.xml", "nested mod settings"),
            ("Saves/Colony.rws", "a save"),
            ("Scenarios/Mine.rsc", "scenario"),
            ("Ideos/Mine.rid", "ideo"),
            ("HugsLib/log.txt", "mod data"),
            ("Player.log", "log"),
            ("steam_autocloud.vdf", "cloud"),
        ] {
            let p = d.join(rel);
            std::fs::create_dir_all(p.parent().expect("p")).expect("dir");
            std::fs::write(p, body).expect("w");
        }
        self.store.get(&ProfileId::default_id()).expect("default")
    }

    fn read(&self, p: &Profile, rel: &str) -> Option<String> {
        std::fs::read_to_string(p.save_data_dir().join(rel)).ok()
    }
}

fn defaults() -> Vec<Group> {
    Group::ALL
        .into_iter()
        .filter(|g| g.on_by_default())
        .collect()
}

#[test]
fn default_groups_copy_settings_and_creations_but_never_saves_or_files() {
    let w = World::new();
    let from = w.default_with_everything();
    let to = w.store.create("New").expect("create");
    let n = w
        .store
        .copy_settings(&from, &to, &defaults(), &Probe(false), &mut |_| {})
        .expect("copy");
    assert_eq!(n, 7, "4 Config files, 1 Config folder, Scenarios, Ideos");
    assert_eq!(
        w.read(&to, "Config/ModsConfig.xml").as_deref(),
        Some("mods")
    );
    assert_eq!(w.read(&to, "Config/KeyPrefs.xml").as_deref(), Some("keys"));
    assert_eq!(
        w.read(&to, "Config/TotalControlData/x.xml").as_deref(),
        Some("nested mod settings")
    );
    assert_eq!(w.read(&to, "Ideos/Mine.rid").as_deref(), Some("ideo"));
    for never in [
        "Saves/Colony.rws",
        "Player.log",
        "steam_autocloud.vdf",
        "HugsLib/log.txt",
    ] {
        assert!(w.read(&to, never).is_none(), "{never} copied");
    }
    assert!(
        !to.save_data_dir().join(STAGING).exists(),
        "staging cleaned up"
    );
}

#[test]
fn each_group_copies_only_its_own_entries() {
    let w = World::new();
    let from = w.default_with_everything();
    let src = from.save_data_dir();
    let names = |g| -> Vec<String> {
        entries(&src, &[g])
            .iter()
            .map(|p| p.display().to_string())
            .collect()
    };
    assert_eq!(names(Group::ModList), ["Config/ModsConfig.xml"]);
    assert_eq!(
        names(Group::GameSettings),
        ["Config/KeyPrefs.xml", "Config/Prefs.xml"]
    );
    assert_eq!(
        names(Group::ModSettings),
        ["Config/Mod_1_Harmony.xml", "Config/TotalControlData"]
    );
    assert_eq!(names(Group::Creations), ["Ideos", "Scenarios"]);
    assert_eq!(names(Group::ModData), ["HugsLib"]);
    let sizes = group_sizes(&src);
    assert!(sizes.contains(&(Group::ModList, 4)), "{sizes:?}");
}

#[test]
fn same_name_target_goes_to_the_bin_and_target_only_is_kept() {
    let w = World::new();
    let from = w.default_with_everything();
    let to = w.store.create("Has Settings").expect("create");
    let cfg = to.save_data_dir().join("Config");
    std::fs::create_dir_all(&cfg).expect("dir");
    std::fs::write(cfg.join("Prefs.xml"), "old prefs").expect("w");
    std::fs::write(cfg.join("Mod_9_Only.xml"), "mine").expect("w");
    w.store
        .copy_settings(
            &from,
            &to,
            &[Group::GameSettings],
            &Probe(false),
            &mut |_| {},
        )
        .expect("copy");
    assert_eq!(w.read(&to, "Config/Prefs.xml").as_deref(), Some("prefs"));
    assert_eq!(
        w.read(&to, "Config/Mod_9_Only.xml").as_deref(),
        Some("mine")
    );
    let binned = w.store_bin();
    assert_eq!(binned, [cfg.join("Prefs.xml")], "only the replaced file");
}

#[test]
fn default_is_never_a_target_and_a_running_game_blocks() {
    let w = World::new();
    let from = w.default_with_everything();
    let other = w.store.create("Other").expect("create");
    assert!(matches!(
        w.store
            .copy_settings(&other, &from, &defaults(), &Probe(false), &mut |_| {}),
        Err(ProfileError::DefaultImmutable(_))
    ));
    assert!(matches!(
        w.store
            .copy_settings(&from, &other, &defaults(), &Probe(true), &mut |_| {}),
        Err(ProfileError::GameRunning)
    ));
    assert!(w.read(&other, "Config/ModsConfig.xml").is_none());
}

#[test]
fn leftover_staging_is_binned_not_moved_in() {
    let w = World::new();
    let from = w.default_with_everything();
    let to = w.store.create("Crashed Before").expect("create");
    let stale = to.save_data_dir().join(STAGING).join("Config");
    std::fs::create_dir_all(&stale).expect("dir");
    std::fs::write(stale.join("Stale.xml"), "stale").expect("w");
    w.store
        .copy_settings(&from, &to, &[Group::ModList], &Probe(false), &mut |_| {})
        .expect("copy");
    assert!(w.read(&to, "Config/Stale.xml").is_none());
    assert_eq!(
        w.read(&to, "Config/ModsConfig.xml").as_deref(),
        Some("mods")
    );
}

#[cfg(unix)]
#[test]
fn a_failed_copy_leaves_the_target_untouched() {
    use std::os::unix::fs::PermissionsExt;
    let w = World::new();
    let from = w.default_with_everything();
    let unreadable = from.save_data_dir().join("Config/Mod_1_Harmony.xml");
    std::fs::set_permissions(&unreadable, std::fs::Permissions::from_mode(0o000)).expect("chmod");
    if std::fs::read(&unreadable).is_ok() {
        return; // Running as root: cannot make a file unreadable.
    }
    let to = w.store.create("Untouched").expect("create");
    let cfg = to.save_data_dir().join("Config");
    std::fs::create_dir_all(&cfg).expect("dir");
    std::fs::write(cfg.join("ModsConfig.xml"), "mine").expect("w");
    let result = w
        .store
        .copy_settings(&from, &to, &defaults(), &Probe(false), &mut |_| {});
    assert!(result.is_err());
    assert_eq!(
        w.read(&to, "Config/ModsConfig.xml").as_deref(),
        Some("mine")
    );
    assert!(!to.save_data_dir().join(STAGING).exists());
}

impl World {
    fn store_bin(&self) -> Vec<PathBuf> {
        self.store.trash_ref().got.borrow().clone()
    }
}
