use super::*;
use crate::profile::file::{FILE_NAME as PROFILE_FILE, ProfileFile};

fn names(of: &[&str]) -> Vec<String> {
    of.iter().map(|s| (*s).to_owned()).collect()
}

/// A schema 4 manifest that names `of`.
fn by(of: &[&str]) -> Manifest {
    let mut m = sample();
    m.schema = file::SCHEMA_AUTHORS;
    m.authors = names(of);
    m
}

#[test]
fn creators_have_limits() {
    assert!(accepted(&by(&["Ann", "Bob"])));
    let ten: Vec<String> = (0..10).map(|i| format!("Creator {i}")).collect();
    let mut m = by(&[]);
    m.authors = ten.clone();
    assert!(accepted(&m), "ten creators are allowed");
    m.authors.push("One too many".into());
    assert!(!accepted(&m), "eleven are not");

    assert!(accepted(&by(&[&"n".repeat(100)])));
    assert!(!accepted(&by(&[&"n".repeat(101)])));
    assert!(accepted(&by(&[&"é".repeat(100)])), "characters, not bytes");
}

#[test]
fn a_creator_is_one_plain_name() {
    assert!(!accepted(&by(&[""])), "empty");
    assert!(!accepted(&by(&["   "])), "blank");
    assert!(!accepted(&by(&["Ann", "Ann"])), "twice");
    assert!(!accepted(&by(&["Ann", " Ann "])), "twice, with spaces");
    assert!(
        !accepted(&by(&["Smith, J."])),
        "a comma reads back as two people"
    );
    assert!(!accepted(&by(&["two\nlines"])));
    assert!(!accepted(&by(&["bell\u{7}"])));
    for mark in ["\u{202E}", "\u{2066}"] {
        assert!(!accepted(&by(&[&format!("Safe{mark}Name")])), "{mark:?}");
    }
    assert!(accepted(&by(&["حزمة Ünïcode 日本語"])));
}

#[test]
fn creators_need_schema_4() {
    let mut m = by(&["Ann"]);
    m.schema = file::SCHEMA_PICKS;
    assert!(refused(&m), "schema 3 has no creators");
    m.schema = file::SCHEMA_BASE;
    assert!(refused(&m), "nor does schema 1");
    // A pack without creators still reads at every older schema.
    for schema in [file::SCHEMA_BASE, file::SCHEMA_COVER, file::SCHEMA_PICKS] {
        let mut m = sample();
        m.schema = schema;
        assert!(accepted(&m), "schema {schema}");
    }
}

#[test]
fn a_pack_from_a_newer_build_is_still_too_new() {
    let text = by(&["Ann"]).to_toml().expect("toml");
    let newer = text.replacen("schema = 4", "schema = 5", 1);
    assert!(matches!(
        parse(&newer),
        Err(ProfileError::SchemaTooNew { found: 5, .. })
    ));
}

#[test]
fn creators_make_a_pack_schema_4_and_carry_into_the_profile() {
    let fx = fixture();
    let mut s = spec(&["A.rws"]);
    s.authors = names(&["Ann", "Bob"]);
    let plan = plan_pack(&fx.source, &fx.id, s).expect("plan");
    let dest = fx.tmp.path().join("authors.rimpack");
    let report = apply_pack(&plan, &dest, &Probe(false), &mut |_| {}).expect("pack");
    assert_eq!(report.manifest.schema, file::SCHEMA_AUTHORS);
    assert_eq!(report.manifest.authors, names(&["Ann", "Bob"]));

    let restored = fx.unpack(&dest).expect("unpack");
    let origin = restored.profile.pack.clone().expect("a pack origin");
    assert_eq!(origin.authors, names(&["Ann", "Bob"]));
    let dir = restored.profile.save_data_dir();
    let dir = dir.parent().expect("profile dir");
    let file = ProfileFile::read(&dir.join(PROFILE_FILE)).expect("profile.toml");
    assert_eq!(file.schema, 4, "creators need the newest profile schema");
}

#[test]
fn a_pack_without_creators_stays_readable_by_older_builds() {
    let fx = fixture();
    let plan = plan_pack(&fx.source, &fx.id, spec(&["A.rws"])).expect("plan");
    let dest = fx.tmp.path().join("plain.rimpack");
    let report = apply_pack(&plan, &dest, &Probe(false), &mut |_| {}).expect("pack");
    assert_eq!(report.manifest.schema, file::SCHEMA_BASE);
    assert!(report.manifest.authors.is_empty());
    let text = report.manifest.to_toml().expect("toml");
    assert!(!text.contains("authors"), "{text}");
}
