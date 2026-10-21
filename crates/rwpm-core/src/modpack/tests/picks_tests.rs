use super::*;
use crate::modpack::pick_candidates;

fn names(list: &[crate::modpack::Candidate]) -> Vec<&str> {
    list.iter().map(|c| c.name.as_str()).collect()
}

fn more_files(fx: &Fx) -> PathBuf {
    let sd = fx.source.get(&fx.id).expect("profile").save_data_dir();
    write(&sd.join("Config/Mod_2_B.xml"), "<settings>b</settings>");
    write(&sd.join("Scenarios/Other.rsc"), "<other/>");
    write(&sd.join("Xenotypes/X.xtp"), "<x/>");
    sd
}

fn picks(settings: Option<&[&str]>, creations: Option<&[&str]>) -> PackSpec {
    let own = |l: &[&str]| l.iter().map(|s| (*s).to_owned()).collect();
    let mut s = spec(&[]);
    s.start_save = None;
    s.includes.mod_settings_only = settings.map(own);
    s.includes.creations_only = creations.map(own);
    s
}

fn pack_with(fx: &Fx, spec: PackSpec) -> Result<Manifest, ProfileError> {
    let plan = plan_pack(&fx.source, &fx.id, spec)?;
    let dest = fx.tmp.path().join("picks.rimpack");
    Ok(apply_pack(&plan, &dest, &Probe(false), &mut |_| {})?.manifest)
}

#[test]
fn candidates_are_what_a_pack_could_hold() {
    let fx = fixture();
    let sd = more_files(&fx);
    let found = pick_candidates(&sd).expect("candidates");
    // Not the mod list or the game settings, and not `notes.bin`, which a pack refuses.
    assert_eq!(names(&found.mod_settings), ["Mod_1_A.xml", "Mod_2_B.xml"]);
    assert_eq!(
        names(&found.creations),
        ["Scenarios/My.rsc", "Scenarios/Other.rsc", "Xenotypes/X.xtp"]
    );
    assert!(found.mod_settings.iter().all(|c| c.bytes > 0));
}

#[test]
fn a_pack_with_picks_holds_only_the_picks_and_is_schema_3() {
    let fx = fixture();
    more_files(&fx);
    let manifest = pack_with(
        &fx,
        picks(Some(&["Mod_2_B.xml"]), Some(&["Scenarios/Other.rsc"])),
    )
    .expect("pack");
    assert_eq!(manifest.schema, file::SCHEMA_PICKS);
    assert_eq!(manifest.entry_count, 3, "the list and the two picks");

    let restored = fx
        .unpack(&fx.tmp.path().join("picks.rimpack"))
        .expect("unpack");
    let sd = restored.profile.save_data_dir();
    assert!(sd.join("Config/Mod_2_B.xml").is_file());
    assert!(sd.join("Scenarios/Other.rsc").is_file());
    assert!(!sd.join("Config/Mod_1_A.xml").exists());
    assert!(!sd.join("Scenarios/My.rsc").exists());
    assert!(!sd.join("Xenotypes/X.xtp").exists());
}

#[test]
fn picking_everything_writes_no_list_and_an_old_schema() {
    let fx = fixture();
    more_files(&fx);
    let all_settings = ["Mod_1_A.xml", "Mod_2_B.xml"];
    let all_creations = ["Scenarios/My.rsc", "Scenarios/Other.rsc", "Xenotypes/X.xtp"];
    let manifest = pack_with(&fx, picks(Some(&all_settings), Some(&all_creations))).expect("pack");
    assert_eq!(manifest.schema, file::SCHEMA_BASE);
    assert_eq!(manifest.includes.mod_settings_only, None);
    assert_eq!(manifest.includes.creations_only, None);
    assert!(!manifest.to_toml().expect("toml").contains("_only"));
}

#[test]
fn a_pick_that_cannot_be_packed_is_refused_when_planning() {
    let fx = fixture();
    more_files(&fx);
    // `notes.bin` is there but a pack may not hold it; `Nope.xml` is not there at all.
    for bad in ["notes.bin", "Nope.xml"] {
        let got = plan_pack(&fx.source, &fx.id, picks(Some(&[bad]), None));
        assert!(
            matches!(got, Err(ProfileError::BadModpack { .. })),
            "{bad}: {got:?}"
        );
    }
    let got = plan_pack(
        &fx.source,
        &fx.id,
        picks(None, Some(&["Scenarios/Gone.rsc"])),
    );
    assert!(matches!(got, Err(ProfileError::BadModpack { .. })));
}

fn with_lists(settings: Option<Vec<String>>, creations: Option<Vec<String>>) -> Manifest {
    let mut m = sample();
    m.schema = file::SCHEMA_PICKS;
    m.includes.mod_settings_only = settings;
    m.includes.creations_only = creations;
    m
}

fn one(name: &str) -> Option<Vec<String>> {
    Some(vec![name.to_owned()])
}

#[test]
fn a_manifest_with_picks_round_trips() {
    let m = with_lists(one("Mod_1_A.xml"), one("Scenarios/My.rsc"));
    let text = m.to_toml().expect("toml");
    assert_eq!(parse(&text).expect("parse"), m);
}

#[test]
fn a_manifest_refuses_a_bad_pick() {
    let long = "a".repeat(600);
    let many: Vec<String> = (0..5_001).map(|i| format!("Mod_{i}.xml")).collect();
    let cases = [
        with_lists(Some(vec![]), None),
        with_lists(Some(many), None),
        with_lists(one(&long), None),
        with_lists(Some(vec!["A.xml".into(), "A.xml".into()]), None),
        with_lists(one("Prefs.xml"), None),
        with_lists(one("ModsConfig.xml"), None),
        with_lists(one("Config/A.xml"), None),
        with_lists(one("../A.xml"), None),
        with_lists(one(""), None),
        with_lists(None, one("A.xml")),
        with_lists(None, one("Scenarios")),
        with_lists(None, one("Saves/A.rws")),
        with_lists(None, one("Scenarios/../../x")),
        with_lists(None, one("/etc/passwd")),
    ];
    for m in &cases {
        assert!(
            refused(m),
            "{:?} {:?}",
            m.includes.mod_settings_only,
            m.includes.creations_only
        );
    }
    let mut early = with_lists(one("A.xml"), None);
    early.schema = file::SCHEMA_COVER;
    assert!(refused(&early), "a list needs schema 3");
    let mut unticked = with_lists(one("A.xml"), None);
    unticked.includes.mod_settings = false;
    assert!(refused(&unticked), "a list needs its group ticked");
}

#[test]
fn a_pack_whose_lists_name_files_it_lacks_or_adds_is_refused_when_opened() {
    let fx = fixture();
    let a = ("SaveData/Config/Mod_1_A.xml", FILE, "<a/>");
    let b = ("SaveData/Config/Mod_2_B.xml", FILE, "<b/>");
    let make = |entries: &[Raw], settings: &str| {
        let mut m = manifest_for(entries, &[]);
        m.schema = file::SCHEMA_PICKS;
        m.includes.mod_settings_only = one(settings);
        m.includes.creations = false;
        archive(fx.tmp.path(), &m, entries)
    };
    // Lists A but holds B too: B is not declared.
    let extra = make(&[list(), a, b], "Mod_1_A.xml");
    assert!(fx.unpack(&extra).is_err());
    // Lists B but holds only A: A is not declared.
    let wrong = make(&[list(), a], "Mod_2_B.xml");
    assert!(fx.unpack(&wrong).is_err());
    // Lists A and holds only the list: A is promised and missing.
    let short = make(&[list()], "Mod_1_A.xml");
    assert!(fx.unpack(&short).is_err());
    // Lists A and holds A: restores.
    let fine = make(&[list(), a], "Mod_1_A.xml");
    assert!(fx.unpack(&fine).is_ok());
    assert!(
        fx.leftovers().len() <= 1,
        "failed restores leave nothing behind"
    );
}
