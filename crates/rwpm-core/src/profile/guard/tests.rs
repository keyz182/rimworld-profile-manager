use crate::discovery::Engine;
use std::path::PathBuf;

use super::*;
use crate::discovery::SteamInfo;

struct NoTrash;

impl Trash for NoTrash {
    fn trash(&self, _: &Path) -> std::io::Result<()> {
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

struct Fixture {
    tmp: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let f = Self {
            tmp: tempfile::tempdir().expect("tempdir"),
        };
        f.add_mod("install/Data/Core", "ludeon.rimworld");
        f
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.tmp.path().join(rel)
    }

    fn store(&self) -> ProfileStore<NoTrash> {
        ProfileStore::open(self.path("profiles"), self.path("game-save"), NoTrash).expect("store")
    }

    fn game(&self) -> GameInstall {
        GameInstall {
            game_dir: self.path("install"),
            exe: self.path("install/RimWorldLinux"),
            version: None,
            engine: Engine::Unity,
            steam: Some(SteamInfo {
                library_dir: self.path("lib"),
                workshop_dir: self.path("ws"),
            }),
        }
    }

    fn add_mod(&self, rel: &str, id: &str) -> PathBuf {
        let dir = self.path(rel);
        std::fs::create_dir_all(dir.join("About")).expect("about");
        std::fs::write(
            dir.join("About/About.xml"),
            format!("<ModMetaData><packageId>{id}</packageId></ModMetaData>"),
        )
        .expect("about");
        std::fs::write(dir.join("payload.txt"), id).expect("payload");
        dir
    }

    /// An OFFLINE profile with `frozen` (folder, id) copies in its Mods and `active` listed.
    fn offline(&self, frozen: &[(&str, &str)], active: &[&str]) -> ProfileId {
        let id = self.store().create("Run").expect("create").id;
        let dir = self.dir(&id);
        let mut file = ProfileFile::read(&dir.join(FILE_NAME)).expect("toml");
        file.mode = Mode::Offline;
        file.write_atomic(&dir.join(FILE_NAME)).expect("toml");
        for (folder, pkg) in frozen {
            self.add_mod(&format!("profiles/{id}/Mods/{folder}"), pkg);
        }
        self.set_active(&id, active);
        id
    }

    fn set_active(&self, id: &ProfileId, active: &[&str]) {
        let config = self.dir(id).join(MODS_CONFIG);
        std::fs::create_dir_all(config.parent().expect("parent")).expect("config dir");
        let items: String = active
            .iter()
            .map(|a| format!("\n    <li>{a}</li>"))
            .collect();
        std::fs::write(
            config,
            format!(
                "<ModsConfigData>\n  <activeMods>{items}\n  </activeMods>\n</ModsConfigData>\n"
            ),
        )
        .expect("config");
    }

    fn dir(&self, id: &ProfileId) -> PathBuf {
        self.path("profiles").join(id.as_str())
    }

    fn check(&self, id: &ProfileId) -> GuardReport {
        check_offline(&self.store(), id, &self.game()).expect("check")
    }

    fn active(&self, id: &ProfileId) -> Vec<String> {
        read_active_mods(&self.dir(id).join(MODS_CONFIG)).expect("active")
    }
}

fn copy_folder(u: &Unfrozen) -> &str {
    match &u.fix {
        Fix::Copy(c) => &c.folder,
        Fix::UseFrozen(_) => panic!("expected a copy, got {u:?}"),
    }
}

#[test]
fn clear_when_all_active_frozen_or_official() {
    let f = Fixture::new();
    f.add_mod("ws/2009463077", "brrainz.harmony");
    let id = f.offline(
        &[("2009463077", "brrainz.harmony")],
        &["brrainz.harmony", "ludeon.rimworld"],
    );
    let report = f.check(&id);
    assert!(report.is_clear(), "{report:?}");
    assert!(report.missing.is_empty());
}

#[test]
fn workshop_only_mod_is_unfrozen() {
    let f = Fixture::new();
    f.add_mod("ws/2009463077", "brrainz.harmony");
    let id = f.offline(&[], &["ludeon.rimworld", "brrainz.harmony"]);
    let report = f.check(&id);
    assert_eq!(report.unfrozen.len(), 1);
    assert_eq!(report.unfrozen[0].active_id, "brrainz.harmony");
    assert_eq!(copy_folder(&report.unfrozen[0]), "2009463077");
}

#[test]
fn steam_suffixed_active_with_frozen_copy_uses_frozen() {
    let f = Fixture::new();
    f.add_mod("ws/2009463077", "brrainz.harmony");
    let id = f.offline(
        &[("2009463077", "brrainz.harmony")],
        &["brrainz.harmony_steam"],
    );
    let report = f.check(&id);
    assert_eq!(
        report.unfrozen,
        [Unfrozen {
            active_id: "brrainz.harmony_steam".into(),
            // No <name> in this About.xml: the package id stands in.
            name: "brrainz.harmony".into(),
            fix: Fix::UseFrozen("brrainz.harmony".into())
        }]
    );
}

#[test]
fn install_mods_only_is_unfrozen_local() {
    let f = Fixture::new();
    f.add_mod("install/Mods/MyMod", "me.mymod");
    let id = f.offline(&[], &["me.mymod"]);
    let report = f.check(&id);
    assert_eq!(copy_folder(&report.unfrozen[0]), "MyMod");
    assert!(matches!(&report.unfrozen[0].fix, Fix::Copy(c) if c.source == ModSource::Local));
}

#[test]
fn missing_is_info() {
    let f = Fixture::new();
    let id = f.offline(&[], &["me.gone"]);
    let report = f.check(&id);
    assert!(report.is_clear());
    assert_eq!(report.missing, ["me.gone"]);
}

#[test]
fn live_default_and_unlaunched_are_clear() {
    let f = Fixture::new();
    f.add_mod("ws/2009463077", "brrainz.harmony");
    let live = f.store().create("Live").expect("create").id;
    f.set_active(&live, &["brrainz.harmony"]);
    assert_eq!(f.check(&live), GuardReport::default());
    assert_eq!(f.check(&ProfileId::default_id()), GuardReport::default());
    let fresh = f.offline(&[], &[]);
    std::fs::remove_file(f.dir(&fresh).join(MODS_CONFIG)).expect("rm config");
    assert_eq!(f.check(&fresh), GuardReport::default());
}

#[test]
fn add_to_profile_copies_and_records() {
    let f = Fixture::new();
    f.add_mod("ws/2009463077", "brrainz.harmony");
    let id = f.offline(&[], &["brrainz.harmony"]);
    let report = f.check(&id);
    add_to_profile(
        &f.store(),
        &id,
        &report.unfrozen,
        &FakeProbe(false),
        &mut |_| {},
    )
    .expect("add");
    let copied = f.dir(&id).join("Mods/2009463077/payload.txt");
    assert_eq!(
        std::fs::read_to_string(copied).expect("copy"),
        "brrainz.harmony"
    );
    let file = ProfileFile::read(&f.dir(&id).join(FILE_NAME)).expect("toml");
    assert_eq!(file.frozen_mods.len(), 1);
    assert_eq!(file.frozen_mods[0].source, ModSource::Workshop);
    assert!(f.check(&id).is_clear(), "guard clear after the fix");
    assert!(!f.dir(&id).join("Mods/.rwpm-add-2009463077").exists());
}

#[test]
fn add_to_profile_switches_steam_id_to_frozen_copy() {
    let f = Fixture::new();
    f.add_mod("ws/2009463077", "brrainz.harmony");
    let id = f.offline(
        &[("2009463077", "brrainz.harmony")],
        &["brrainz.harmony_steam", "ludeon.rimworld"],
    );
    let report = f.check(&id);
    add_to_profile(
        &f.store(),
        &id,
        &report.unfrozen,
        &FakeProbe(false),
        &mut |_| {},
    )
    .expect("add");
    assert_eq!(f.active(&id), ["brrainz.harmony", "ludeon.rimworld"]);
    assert!(f.check(&id).is_clear());
}

#[test]
fn add_to_profile_folder_clash() {
    let f = Fixture::new();
    f.add_mod("ws/2009463077", "brrainz.harmony");
    // A different mod already sits in a folder with the same name.
    let id = f.offline(&[("2009463077", "me.other")], &["brrainz.harmony"]);
    let report = f.check(&id);
    assert!(matches!(
        add_to_profile(
            &f.store(),
            &id,
            &report.unfrozen,
            &FakeProbe(false),
            &mut |_| {}
        ),
        Err(ProfileError::FolderClash(_))
    ));
}

#[test]
fn add_to_profile_refuses_while_any_game_runs() {
    let f = Fixture::new();
    f.add_mod("ws/2009463077", "brrainz.harmony");
    let id = f.offline(&[], &["brrainz.harmony"]);
    let report = f.check(&id);
    assert!(matches!(
        add_to_profile(
            &f.store(),
            &id,
            &report.unfrozen,
            &FakeProbe(true),
            &mut |_| {}
        ),
        Err(ProfileError::GameRunning)
    ));
    assert!(!f.dir(&id).join("Mods/2009463077").exists());
}

#[test]
fn turn_off_removes_only_those_entries() {
    let f = Fixture::new();
    f.add_mod("ws/2009463077", "brrainz.harmony");
    let id = f.offline(&[], &["ludeon.rimworld", "brrainz.harmony", "me.gone"]);
    turn_off(&f.store(), &id, &["brrainz.harmony".into()]).expect("turn off");
    assert_eq!(f.active(&id), ["ludeon.rimworld", "me.gone"]);
    assert!(f.check(&id).is_clear());
}
