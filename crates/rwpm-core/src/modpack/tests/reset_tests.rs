use std::cell::RefCell;

use super::*;
use crate::profile::file::{FILE_NAME as PROFILE_FILE, PACK_FILE};
use crate::profile::{FrozenMod, ModSource, ProfileFile, Trash};

/// Records what was sent to the recycle bin, and removes it like the real one.
#[derive(Default)]
struct Bin(RefCell<Vec<PathBuf>>);

impl Trash for Bin {
    fn trash(&self, path: &Path) -> std::io::Result<()> {
        self.0.borrow_mut().push(path.to_path_buf());
        std::fs::remove_file(path)
    }
}

struct World {
    /// Keeps the temporary folders alive.
    _fx: Fx,
    store: ProfileStore<Bin>,
    profile: crate::profile::Profile,
}

/// A profile restored from a pack with saves A and B, into a store that has a bin.
fn world() -> World {
    let fx = fixture();
    let archive = fx.pack(&["A.rws", "B.rws"]).expect("pack");
    let store = ProfileStore::open(
        fx.tmp.path().join("c"),
        fx.tmp.path().join("c-saves"),
        Bin::default(),
    )
    .expect("store");
    let plan = plan_unpack(&archive, &store).expect("plan");
    let profile = apply_unpack(&plan, &store, &mut |_| {})
        .expect("unpack")
        .profile;
    World {
        _fx: fx,
        store,
        profile,
    }
}

impl World {
    fn sd(&self, rel: &str) -> PathBuf {
        self.profile.save_data_dir().join(rel)
    }

    fn dir(&self) -> PathBuf {
        self.sd("").parent().expect("dir").to_path_buf()
    }

    fn reset(&self, saves: bool) -> Result<ResetReport, ProfileError> {
        let plan = plan_reset(&self.store, &self.profile.id, saves)?;
        apply_reset(&plan, &self.store, &Probe(false), &mut |_| {})
    }

    fn trashed(&self) -> Vec<PathBuf> {
        self.store.trash_ref().0.borrow().clone()
    }

    fn go_offline(&self, held: &[&str]) {
        let path = self.dir().join(PROFILE_FILE);
        let mut file = ProfileFile::read(&path).expect("read");
        file.mode = Mode::Offline;
        file.frozen_mods = held
            .iter()
            .map(|id| FrozenMod {
                folder: (*id).into(),
                package_id: (*id).into(),
                source: ModSource::Workshop,
                source_dir: PathBuf::from("/elsewhere"),
                copied_at: "2026-10-19T00:00:00Z".parse().expect("datetime"),
            })
            .collect();
        file.write_atomic(&path).expect("write");
    }
}

#[test]
fn a_reset_puts_the_pack_files_back_and_bins_what_it_replaces() {
    let w = world();
    write(&w.sd("Config/ModsConfig.xml"), "<changed/>");
    write(&w.sd("Config/mine.xml"), "<mine/>");
    let report = w.reset(false).expect("reset");
    assert_eq!(
        std::fs::read_to_string(w.sd("Config/ModsConfig.xml")).expect("read"),
        MOD_LIST
    );
    assert_eq!(report.replaced, 1);
    assert_eq!(w.trashed(), [w.sd("Config/ModsConfig.xml")]);
    assert!(
        w.sd("Config/mine.xml").is_file(),
        "files the pack lacks stay"
    );
    assert!(!w.dir().join(reset::STAGING).exists(), "staging is removed");
}

#[test]
fn files_that_already_match_are_left_alone() {
    let w = world();
    let report = w.reset(false).expect("reset");
    assert_eq!((report.replaced, report.added), (0, 0));
    assert!(report.unchanged > 0);
    assert!(w.trashed().is_empty());
}

#[test]
fn saves_come_back_only_when_asked() {
    let w = world();
    write(&w.sd("Saves/A.rws"), "played on");
    w.reset(false).expect("reset");
    assert_eq!(
        std::fs::read_to_string(w.sd("Saves/A.rws")).expect("read"),
        "played on"
    );
    w.reset(true).expect("reset with saves");
    assert_eq!(
        std::fs::read_to_string(w.sd("Saves/A.rws")).expect("read"),
        "save a"
    );
}

#[test]
fn a_reset_waits_for_the_game_to_close() {
    let w = world();
    let plan = plan_reset(&w.store, &w.profile.id, false).expect("plan");
    let err = apply_reset(&plan, &w.store, &Probe(true), &mut |_| {}).unwrap_err();
    assert!(matches!(err, ProfileError::GameRunning));
}

#[test]
fn only_a_pack_profile_can_be_reset() {
    let w = world();
    let plain = w.store.create("Plain").expect("create");
    assert!(matches!(
        plan_reset(&w.store, &plain.id, false),
        Err(ProfileError::NoPackOrigin(_))
    ));
    let default = crate::profile::ProfileId::parse("default").expect("id");
    assert!(matches!(
        plan_reset(&w.store, &default, false),
        Err(ProfileError::DefaultImmutable(_))
    ));
}

#[test]
fn a_missing_pack_file_is_named() {
    let w = world();
    let kept = w.dir().join(PACK_FILE);
    std::fs::remove_file(&kept).expect("remove");
    match plan_reset(&w.store, &w.profile.id, false) {
        Err(ProfileError::PackFileMissing(path)) => assert_eq!(path, kept),
        other => panic!("expected PackFileMissing, got {other:?}"),
    }
}

#[test]
fn an_offline_profile_must_hold_the_mods_the_pack_lists() {
    let w = world();
    w.go_offline(&[]);
    let plan = plan_reset(&w.store, &w.profile.id, false).expect("plan");
    assert_eq!(plan.missing.len(), 1);
    write(&w.sd("Config/ModsConfig.xml"), "<changed/>");
    let err = apply_reset(&plan, &w.store, &Probe(false), &mut |_| {}).unwrap_err();
    assert!(
        matches!(&err, ProfileError::ResetNeedsMods(n) if n == &["A mod"]),
        "{err:?}"
    );
    assert_eq!(
        std::fs::read_to_string(w.sd("Config/ModsConfig.xml")).expect("read"),
        "<changed/>",
        "a refused reset changes nothing"
    );

    w.go_offline(&["Brrainz.Harmony"]);
    let plan = plan_reset(&w.store, &w.profile.id, false).expect("plan");
    assert!(plan.missing.is_empty(), "ids match without regard to case");
    w.reset(false).expect("reset");
}

#[test]
fn a_live_profile_never_lists_missing_mods() {
    let w = world();
    let plan = plan_reset(&w.store, &w.profile.id, false).expect("plan");
    assert!(plan.missing.is_empty());
}
