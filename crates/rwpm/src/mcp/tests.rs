use std::path::Path;

use rwpm_core::profile::{OsTrash, ProfileStore, RunningProbe};
use rwpm_core::settings::AppSettings;
use serde_json::Value;

use super::ops::{Ops, clean};

struct Probe(bool);

impl RunningProbe for Probe {
    fn any_game(&self) -> bool {
        self.0
    }

    fn is_running(&self, _: &Path) -> bool {
        self.0
    }
}

fn plenty(_: &Path) -> Option<u64> {
    Some(u64::MAX)
}

fn none(_: &Path) -> Option<u64> {
    Some(0)
}

fn unknown(_: &Path) -> Option<u64> {
    None
}

/// Ops over a store in a temp dir. Keeps the dir alive for the test.
fn sandbox() -> (tempfile::TempDir, Ops) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let save = tmp.path().join("save");
    std::fs::create_dir(&save).expect("save dir");
    let store = ProfileStore::open(tmp.path().join("profiles"), save, OsTrash).expect("store");
    let ops =
        Ops::new(store, tmp.path().join("app"), AppSettings::default()).with_free_space(plenty);
    (tmp, ops)
}

fn id_of(v: &Value) -> String {
    v["id"].as_str().expect("id").to_owned()
}

#[test]
fn clean_flattens_control_characters_and_cuts_long_text() {
    assert_eq!(clean("  a\nb\u{0}c  "), "a b c");
    let long = "x".repeat(500);
    let out = clean(&long);
    assert_eq!(out.chars().count(), 201);
    assert!(out.ends_with('…'));
    assert_eq!(clean("short"), "short");
}

#[test]
fn a_new_profile_is_listed_and_can_be_renamed() {
    let (_tmp, ops) = sandbox();
    let made = ops.create_profile("  Vanilla Plus ").expect("create");
    assert_eq!(made["name"], "Vanilla Plus");
    let id = id_of(&made);

    let listed = ops.list_profiles().expect("list");
    let ids: Vec<&str> = listed["profiles"]
        .as_array()
        .expect("array")
        .iter()
        .filter_map(|p| p["id"].as_str())
        .collect();
    assert!(ids.contains(&id.as_str()), "{ids:?}");
    assert!(ids.contains(&"default"), "{ids:?}");

    let renamed = ops.rename_profile(&id, "Renamed").expect("rename");
    assert_eq!(renamed["name"], "Renamed");
    assert_eq!(id_of(&renamed), id);
    assert_eq!(ops.get_profile(&id).expect("get")["name"], "Renamed");
}

#[test]
fn bad_input_gives_plain_errors() {
    let (_tmp, ops) = sandbox();
    assert!(ops.get_profile("nope").is_err());
    assert!(ops.get_profile("../escape").is_err());
    assert!(ops.create_profile("   ").is_err());
    let id = id_of(&ops.create_profile("A").expect("create"));
    assert!(ops.rename_profile(&id, "").is_err());
    assert!(ops.rename_profile("default", "X").is_err());
}

#[test]
fn a_fresh_profile_has_no_last_run_and_no_mods_problem() {
    let (_tmp, ops) = sandbox();
    let id = id_of(&ops.create_profile("A").expect("create"));
    let last = ops.last_run(&id).expect("last_run");
    assert_eq!(last["last_run"], Value::Null);
    assert!(last["log_file"].is_string());
}

#[test]
fn clone_copies_into_a_new_profile() {
    let (_tmp, ops) = sandbox();
    let src = id_of(&ops.create_profile("Source").expect("create"));
    let copy = ops
        .clone_profile(&src, "Copy", &Probe(false))
        .expect("clone");
    assert_eq!(copy["name"], "Copy");
    assert_ne!(id_of(&copy), src);
}

#[test]
fn clone_is_refused_while_the_game_runs() {
    let (_tmp, ops) = sandbox();
    let src = id_of(&ops.create_profile("Source").expect("create"));
    assert!(ops.clone_profile(&src, "Copy", &Probe(true)).is_err());
    let listed = ops.list_profiles().expect("list");
    assert_eq!(listed["profiles"].as_array().expect("array").len(), 2);
}

#[test]
fn clone_is_refused_without_room_and_leaves_nothing_behind() {
    let (tmp, ops) = sandbox();
    let src = id_of(&ops.create_profile("Source").expect("create"));
    std::fs::write(
        tmp.path()
            .join("profiles")
            .join(&src)
            .join("SaveData")
            .join("big.rws"),
        vec![0u8; 4096],
    )
    .expect("write");
    let tight = ops.with_free_space(none);
    let err = tight
        .clone_profile(&src, "Copy", &Probe(false))
        .expect_err("no room");
    assert!(err.contains("not enough disk space"), "{err}");
    let listed = tight.list_profiles().expect("list");
    assert_eq!(listed["profiles"].as_array().expect("array").len(), 2);
}

#[test]
fn clone_is_refused_when_free_space_cannot_be_read() {
    let (_tmp, ops) = sandbox();
    let src = id_of(&ops.create_profile("Source").expect("create"));
    let blind = ops.with_free_space(unknown);
    let err = blind
        .clone_profile(&src, "Copy", &Probe(false))
        .expect_err("unknown space");
    assert!(err.contains("free disk space"), "{err}");
}
