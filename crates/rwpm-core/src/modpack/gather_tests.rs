use std::path::{Path, PathBuf};

use super::*;
use crate::backup::pack::tests::{NoTrash, Probe, write};
use crate::builds::EffectiveGame;
use crate::builds::effective::test_support::{add_mod, write_game};
use crate::builds::effective_game;
use crate::profile::file::FILE_NAME as PROFILE_FILE;
use crate::profile::{
    FrozenMod, Location, ModSource, Mode, Profile, ProfileFile, ProfileId, ProfileStore,
};

const ACF: &str = "\"AppWorkshop\"\n{\n\t\"WorkshopItemsInstalled\"\n\t{\n\
    \t\t\"111\"\n\t\t{\n\t\t\t\"timeupdated\"\t\t\"1700000000\"\n\t\t}\n\t}\n}\n";

struct World {
    tmp: tempfile::TempDir,
    store: ProfileStore<NoTrash>,
    game: EffectiveGame,
}

fn about(dir: &Path, body: &str) {
    write(
        &dir.join("About/About.xml"),
        &format!("<ModMetaData>{body}</ModMetaData>"),
    );
}

fn world() -> World {
    let tmp = tempfile::tempdir().expect("tempdir");
    let install = write_game(
        &tmp.path().join("steam/steamapps/common/RimWorld"),
        "1.6.4871 rev598",
    );
    let workshop = install.steam.as_ref().expect("steam").workshop_dir.clone();
    add_mod(
        &install.data_dir().join("Royalty"),
        "ludeon.rimworld.royalty",
        "Royalty",
    );
    add_mod(&workshop.join("111"), "steam.only", "Steam Only");
    write(
        &workshop.join("111/About/Manifest.xml"),
        "<Manifest><version>1.2.3</version></Manifest>",
    );
    let local = install.mods_dir();
    add_mod(&local.join("LiveLocal"), "live.local", "Live Local");
    about(
        &local.join("LiveLocal"),
        "<name>Live Local</name><packageId>live.local</packageId>\
         <url>https://example.com/mod</url>",
    );
    add_mod(&local.join("Hostile"), "hostile.url.mod", "Hostile");
    about(
        &local.join("Hostile"),
        "<name>Hostile</name><packageId>hostile.url.mod</packageId>\
         <url>javascript:alert(1)</url>",
    );
    add_mod(&local.join("Odd"), "bad id", "Odd");
    write(
        &tmp.path()
            .join("steam/steamapps/workshop/appworkshop_294100.acf"),
        ACF,
    );
    let store = ProfileStore::open(
        tmp.path().join("profiles"),
        tmp.path().join("saves"),
        NoTrash,
    )
    .expect("store");
    let profile = store.create("Source").expect("create");
    let game = effective_game(
        &profile,
        Some(&install),
        store.root(),
        &tmp.path().join("app"),
    )
    .expect("game");
    World { tmp, store, game }
}

fn mod_list(profile: &Profile, ids: &[&str]) {
    let items: String = ids.iter().map(|i| format!("<li>{i}</li>")).collect();
    write(
        &profile.save_data_dir().join("Config/ModsConfig.xml"),
        &format!("<ModsConfigData><activeMods>{items}</activeMods></ModsConfigData>"),
    );
}

const ACTIVE: [&str; 8] = [
    "ludeon.rimworld",
    "ludeon.rimworld.royalty",
    "steam.only",
    "live.local",
    "gone.missing",
    "hostile.url.mod",
    "bad id",
    "ludeon.rimworld.ideology",
];

#[test]
fn gathers_the_active_mods_in_load_order() {
    let w = world();
    let profile = w.store.create("Live").expect("create");
    mod_list(&profile, &ACTIVE);
    let got = gather_mods(&profile, &w.game).expect("gather");

    let ids: Vec<&str> = got.mods.iter().map(|m| m.package_id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "steam.only",
            "live.local",
            "gone.missing",
            "hostile.url.mod",
            "ludeon.rimworld.ideology"
        ]
    );
    // Core is always on, so it is not recorded; an expansion that is turned on but not
    // installed has no folder to resolve, so it is listed as a missing mod, not as DLC.
    assert_eq!(got.dlc, ["ludeon.rimworld.royalty"]);

    let steam = &got.mods[0];
    assert_eq!(steam.name, "Steam Only");
    assert_eq!(steam.workshop_id, Some(111));
    assert_eq!(
        steam.url.as_deref(),
        Some("https://steamcommunity.com/sharedfiles/filedetails/?id=111")
    );
    assert_eq!(steam.time_updated, Some(1_700_000_000));
    assert_eq!(steam.version.as_deref(), Some("1.2.3"));

    let local = &got.mods[1];
    assert_eq!((local.workshop_id, local.time_updated), (None, None));
    assert_eq!(local.url.as_deref(), Some("https://example.com/mod"));

    let missing = &got.mods[2];
    assert_eq!(missing.name, "gone.missing");
    assert_eq!(
        (&missing.workshop_id, &missing.url, &missing.version),
        (&None, &None, &None)
    );

    assert_eq!(
        got.mods[3].url, None,
        "a link that is not a web address is dropped"
    );
    assert_eq!(got.skipped.len(), 1, "{:?}", got.skipped);
    assert!(got.skipped[0].contains("bad id"));
}

#[test]
fn an_offline_copy_of_a_workshop_mod_keeps_its_workshop_id() {
    let w = world();
    let dir = w.tmp.path().join("offline");
    let frozen = |folder: &str, id: &str, source| FrozenMod {
        folder: folder.into(),
        package_id: id.into(),
        source,
        source_dir: PathBuf::from("/elsewhere"),
        copied_at: "2026-10-19T00:00:00Z".parse().expect("datetime"),
    };
    let mut file = ProfileFile::new("Offline");
    file.mode = Mode::Offline;
    file.frozen_mods = vec![
        frozen("111", "steam.only", ModSource::Workshop),
        frozen("LiveLocal", "live.local", ModSource::Local),
    ];
    std::fs::create_dir_all(&dir).expect("dir");
    file.write_atomic(&dir.join(PROFILE_FILE))
        .expect("profile.toml");
    add_mod(&dir.join("Mods/111"), "steam.only", "Steam Only");
    add_mod(&dir.join("Mods/LiveLocal"), "live.local", "Live Local");
    let profile = Profile {
        id: ProfileId::parse("offline").expect("id"),
        name: "Offline".into(),
        mode: Mode::Offline,
        game_build: None,
        location: Location::Stored { dir },
    };
    mod_list(&profile, &["steam.only", "live.local"]);
    let got = gather_mods(&profile, &w.game).expect("gather");
    assert_eq!(got.mods[0].workshop_id, Some(111));
    assert_eq!(got.mods[0].time_updated, Some(1_700_000_000));
    assert_eq!(got.mods[1].workshop_id, None, "a local mod stays local");
    assert!(got.skipped.is_empty());
}

#[test]
fn a_numeric_folder_is_not_a_workshop_id_unless_the_profile_says_so() {
    let w = world();
    let profile = w.store.create("Numeric").expect("create");
    let local = w.game.mods_dir().join("424242");
    add_mod(&local, "numeric.local", "Numeric");
    mod_list(&profile, &["numeric.local"]);
    let got = gather_mods(&profile, &w.game).expect("gather");
    assert_eq!(got.mods[0].workshop_id, None);
}

#[test]
fn gather_needs_a_readable_mod_list() {
    let w = world();
    let empty = w.store.create("Empty").expect("create");
    assert!(matches!(
        gather_mods(&empty, &w.game),
        Err(ProfileError::NoModList(_))
    ));
    let broken = w.store.create("Broken").expect("create");
    write(
        &broken.save_data_dir().join("Config/ModsConfig.xml"),
        "<not closed",
    );
    assert!(matches!(
        gather_mods(&broken, &w.game),
        Err(ProfileError::Parse { .. })
    ));
}

#[test]
fn a_gathered_list_goes_straight_into_a_pack() {
    let w = world();
    let profile = w.store.create("Share").expect("create");
    mod_list(&profile, &ACTIVE);
    let got = gather_mods(&profile, &w.game).expect("gather");
    let spec = PackSpec {
        description: String::new(),
        game_version: None,
        dlc: got.dlc.clone(),
        includes: Includes {
            mod_settings: false,
            creations: false,
            game_settings: false,
            saves: vec![],
        },
        start_save: None,
        collection_url: None,
        mods: got.mods.clone(),
    };
    let plan = plan_pack(&w.store, &profile.id, spec).expect("plan");
    let dest = w.tmp.path().join("share.rimpack");
    apply_pack(&plan, &dest, &Probe(false), &mut |_| {}).expect("pack");
    let manifest = read_manifest(&dest).expect("manifest");
    assert_eq!(manifest.mods, got.mods);
    assert_eq!(manifest.dlc, got.dlc);
}
