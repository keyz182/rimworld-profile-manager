use std::io::Write;
use std::path::{Path, PathBuf};

use tar::EntryType;

use super::entry::{Place, check_declared, check_entry_kind, classify};
use super::*;
use crate::backup::pack::tests::{NoTrash, Probe, write};
use crate::profile::{Mode, ProfileStore};

const MOD_LIST: &str = "<ModsConfigData><activeMods><li>a.b</li></activeMods></ModsConfigData>";

fn includes(saves: &[&str]) -> Includes {
    Includes {
        mod_settings: true,
        creations: true,
        game_settings: false,
        saves: saves.iter().map(|s| (*s).to_owned()).collect(),
    }
}

fn entry(id: &str) -> ModEntry {
    ModEntry {
        package_id: id.into(),
        name: "A mod".into(),
        workshop_id: Some(3552152340),
        url: Some("https://steamcommunity.com/sharedfiles/filedetails/?id=3552152340".into()),
        time_updated: Some(1_760_000_000),
        version: Some("1.2".into()),
    }
}

fn sample() -> Manifest {
    Manifest {
        schema: file::SCHEMA_BASE,
        created_at: "2026-10-19T13:00:00Z".parse().expect("datetime"),
        rwpm_version: "1.11.0".into(),
        name: "Ironman".into(),
        description: "Hard mode".into(),
        game_version: Some("1.7.4977 rev1248".into()),
        dlc: vec!["ludeon.rimworld.royalty".into()],
        start_save: Some("Start.rws".into()),
        collection_url: Some("https://steamcommunity.com/workshop/filedetails/?id=1".into()),
        cover: None,
        uncompressed_bytes: 4096,
        entry_count: 3,
        includes: includes(&["Start.rws"]),
        mods: vec![entry("brrainz.harmony"), entry("fluffy.worktab")],
    }
}

fn parse(text: &str) -> Result<Manifest, ProfileError> {
    Manifest::parse(text, Path::new("x.rimpack"))
}

fn refused(manifest: &Manifest) -> bool {
    let text = manifest.to_toml().expect("toml");
    matches!(parse(&text), Err(ProfileError::BadModpack { .. }))
}

#[test]
fn manifest_round_trips() {
    let text = sample().to_toml().expect("toml");
    assert_eq!(parse(&text).expect("parse"), sample());
}

#[test]
fn manifest_refuses_unknown_fields_and_newer_schema() {
    let text = sample().to_toml().expect("toml");
    let extra = format!("surprise = 1\n{text}");
    assert!(matches!(
        parse(&extra),
        Err(ProfileError::BadModpack { .. })
    ));
    let newer = text.replacen("schema = 1", "schema = 3", 1);
    assert!(matches!(
        parse(&newer),
        Err(ProfileError::SchemaTooNew { found: 3, .. })
    ));
}

#[test]
fn hostile_workshop_ids_are_refused() {
    let base = sample().to_toml().expect("toml");
    let with = |id: &str| {
        let swapped = base.replace("workshop_id = 3552152340", &format!("workshop_id = {id}"));
        parse(&swapped)
    };
    assert!(with("3552152340").is_ok());
    for hostile in ["\"1/../../..\"", "\"1 +quit\"", "\"abc\"", "-5", "0", "1.5"] {
        assert!(
            matches!(with(hostile), Err(ProfileError::BadModpack { .. })),
            "{hostile} must be refused"
        );
    }
}

#[test]
fn hostile_package_ids_and_links_are_refused() {
    for id in ["a/b", "a b", "", "../x", "a\\b", "a:b", "a\nb", "a+b"] {
        let mut m = sample();
        m.mods[0].package_id = id.into();
        assert!(refused(&m), "package id {id:?} must be refused");
    }
    for url in [
        "javascript:alert(1)",
        "file:///etc/passwd",
        "steam://run/1",
        "https://a b",
        "https://a\nb",
        "",
    ] {
        let mut m = sample();
        m.mods[0].url = Some(url.into());
        assert!(refused(&m), "link {url:?} must be refused");
        let mut m = sample();
        m.collection_url = Some(url.into());
        assert!(refused(&m), "collection link {url:?} must be refused");
    }
}

#[test]
fn manifest_holds_the_start_save_to_its_saves() {
    let mut m = sample();
    m.start_save = Some("Other.rws".into());
    assert!(refused(&m));
    let mut m = sample();
    m.includes.saves = vec!["a/b.rws".into()];
    m.start_save = None;
    assert!(refused(&m));
    let mut m = sample();
    m.includes.saves = vec!["Start.rws".into(), "Start.rws".into()];
    assert!(refused(&m));
    let mut m = sample();
    m.name = "  ".into();
    assert!(refused(&m));
}

fn accepted(manifest: &Manifest) -> bool {
    let text = manifest.to_toml().expect("toml");
    parse(&text).is_ok()
}

#[test]
fn text_a_person_reads_has_limits() {
    let ok = |edit: fn(&mut Manifest)| {
        let mut m = sample();
        edit(&mut m);
        accepted(&m)
    };
    // The edges are allowed, one past them is refused.
    assert!(ok(|m| m.name = "n".repeat(100)));
    assert!(!ok(|m| m.name = "n".repeat(101)));
    assert!(ok(|m| m.description = "d".repeat(2_000)));
    assert!(!ok(|m| m.description = "d".repeat(2_001)));
    assert!(ok(|m| m.game_version = Some("v".repeat(128))));
    assert!(!ok(|m| m.game_version = Some("v".repeat(129))));
    // Counted in characters, not bytes: 100 multi-byte letters are fine.
    assert!(ok(|m| m.name = "é".repeat(100)));
    // A description may have line breaks and tabs; nothing else that is a control character.
    assert!(ok(|m| m.description = "line one\n\tline two".into()));
    assert!(!ok(|m| m.description = "bell\u{7}".into()));
    assert!(!ok(|m| m.description = "nul\u{0}".into()));
    // A name and a version are one line.
    assert!(!ok(|m| m.name = "two\nlines".into()));
    assert!(!ok(|m| m.game_version = Some("1.0\r\n".into())));
}

#[test]
fn text_that_reorders_other_text_is_refused() {
    let hostile = [
        "\u{202E}", "\u{202D}", "\u{202A}", "\u{202B}", "\u{202C}", "\u{2066}", "\u{2069}",
    ];
    for mark in hostile {
        let mut m = sample();
        m.name = format!("Safe{mark}Pack");
        assert!(!accepted(&m), "name with {mark:?}");
        let mut m = sample();
        m.description = format!("Plain{mark}text");
        assert!(!accepted(&m), "description with {mark:?}");
        let mut m = sample();
        m.mods[0].name = format!("Harmony{mark}");
        assert!(!accepted(&m), "mod name with {mark:?}");
        let mut m = sample();
        m.mods[0].version = Some(format!("1.0{mark}"));
        assert!(!accepted(&m), "mod version with {mark:?}");
    }
    // Ordinary right-to-left and accented text is not touched.
    let mut m = sample();
    m.name = "حزمة Ünïcode 日本語".into();
    assert!(accepted(&m));
}

#[test]
fn a_pack_with_a_huge_description_is_refused_before_anything_is_shown() {
    let fx = fixture();
    let mut m = sample();
    m.description = "x".repeat(3 * 1024 * 1024);
    let entries = [list()];
    let mut m_for = manifest_for(&entries, &[]);
    m_for.description = m.description.clone();
    let path = archive(fx.tmp.path(), &m_for, &entries);
    assert!(matches!(
        read_manifest(&path),
        Err(ProfileError::BadModpack { .. })
    ));
    assert!(matches!(
        plan_unpack(&path, &fx.target),
        Err(ProfileError::BadModpack { .. })
    ));
    assert!(fx.leftovers().is_empty());
}

#[test]
fn classify_accepts_only_the_manifest_and_save_data() {
    assert_eq!(classify(Path::new("rimpack.toml")), Ok(Place::Manifest));
    assert_eq!(
        classify(Path::new("SaveData/Config/ModsConfig.xml")),
        Ok(Place::SaveData(PathBuf::from("Config/ModsConfig.xml")))
    );
    for name in [
        "",
        "SaveData",
        "Mods/x/About/About.xml",
        "profile/Mods/x",
        "../SaveData/x",
        "/etc/passwd",
        "SaveData/../x",
        "SaveData/a\\b",
        "SaveData/C:evil",
        "rimpack.toml/x",
        "other",
    ] {
        assert!(
            classify(Path::new(name)).is_err(),
            "{name:?} must be refused"
        );
    }
}

#[test]
fn only_plain_files_are_entries() {
    let name = Path::new("SaveData/Config/a.xml");
    assert!(check_entry_kind(EntryType::Regular, name).is_ok());
    for kind in [
        EntryType::Directory,
        EntryType::Symlink,
        EntryType::Link,
        EntryType::Fifo,
    ] {
        assert!(check_entry_kind(kind, name).is_err());
    }
}

#[test]
fn declared_files_follow_the_ticks_and_the_save_list() {
    let all = Includes {
        mod_settings: true,
        creations: true,
        game_settings: true,
        saves: vec!["A.rws".into()],
    };
    let none = Includes {
        mod_settings: false,
        creations: false,
        game_settings: false,
        saves: vec![],
    };
    let ok = |rel: &str, inc: &Includes| check_declared(Path::new(rel), inc).is_ok();
    assert!(ok("Config/ModsConfig.xml", &none));
    for rel in [
        "Config/Mod_1_A.xml",
        "Config/Prefs.xml",
        "Scenarios/My.rsc",
        "Saves/A.rws",
    ] {
        assert!(ok(rel, &all), "{rel} is declared");
        assert!(!ok(rel, &none), "{rel} is not declared when unticked");
    }
    for rel in [
        "Config/evil.dll",
        "Config/lib.so",
        "Config/run.sh",
        "Config/Sub/evil.exe",
        "Config/ModsConfig.xml/evil.xml",
        "Saves/B.rws",
        "Saves/sub/A.rws",
        "Logs/x.txt",
        "Mods/x/a.xml",
        "Config",
        "Prefs.xml",
    ] {
        assert!(!ok(rel, &all), "{rel} must be refused even with every tick");
    }
}

/// A source profile with a realistic save-data folder, and a second, empty machine.
struct Fx {
    tmp: tempfile::TempDir,
    source: ProfileStore<NoTrash>,
    target: ProfileStore<NoTrash>,
    id: crate::profile::ProfileId,
}

fn fixture() -> Fx {
    let tmp = tempfile::tempdir().expect("tempdir");
    let source = ProfileStore::open(tmp.path().join("a"), tmp.path().join("a-saves"), NoTrash)
        .expect("source store");
    let target = ProfileStore::open(tmp.path().join("b"), tmp.path().join("b-saves"), NoTrash)
        .expect("target store");
    let profile = source.create("Source").expect("create");
    let sd = profile.save_data_dir();
    write(&sd.join("Config/ModsConfig.xml"), MOD_LIST);
    write(&sd.join("Config/Mod_1_A.xml"), "<settings>a</settings>");
    write(&sd.join("Config/Prefs.xml"), "<prefs/>");
    write(&sd.join("Config/notes.bin"), "not data");
    write(&sd.join("Scenarios/My.rsc"), "<scenario/>");
    write(&sd.join("Saves/A.rws"), "save a");
    write(&sd.join("Saves/B.rws"), "save b");
    write(&sd.join("Logs/x.txt"), "log");
    Fx {
        tmp,
        source,
        target,
        id: profile.id,
    }
}

fn spec(saves: &[&str]) -> PackSpec {
    PackSpec {
        description: "d".into(),
        game_version: None,
        dlc: vec![],
        includes: includes(saves),
        start_save: saves.first().map(|s| (*s).to_owned()),
        collection_url: None,
        cover: None,
        mods: vec![entry("brrainz.harmony")],
    }
}

impl Fx {
    fn pack(&self, saves: &[&str]) -> Result<PathBuf, ProfileError> {
        let plan = plan_pack(&self.source, &self.id, spec(saves))?;
        let dest = self.tmp.path().join("out.rimpack");
        apply_pack(&plan, &dest, &Probe(false), &mut |_| {})?;
        Ok(dest)
    }

    fn unpack(&self, archive: &Path) -> Result<UnpackReport, ProfileError> {
        let plan = plan_unpack(archive, &self.target)?;
        apply_unpack(&plan, &self.target, &mut |_| {})
    }

    fn leftovers(&self) -> Vec<String> {
        std::fs::read_dir(self.target.root())
            .map(|d| {
                d.filter_map(Result::ok)
                    .filter_map(|e| e.file_name().into_string().ok())
                    .collect()
            })
            .unwrap_or_default()
    }
}

#[test]
fn pack_and_unpack_round_trip() {
    let fx = fixture();
    let plan = plan_pack(&fx.source, &fx.id, spec(&["A.rws"])).expect("plan");
    assert_eq!(plan.entries, 4, "list, one setting, one creation, one save");
    assert_eq!(plan.skipped, vec![PathBuf::from("Config/notes.bin")]);
    let dest = fx.tmp.path().join("out.rimpack");
    let report = apply_pack(&plan, &dest, &Probe(false), &mut |_| {}).expect("pack");
    assert_eq!(report.manifest.entry_count, 4);

    let preview = read_manifest(&dest).expect("manifest");
    assert_eq!(preview, report.manifest);
    assert_eq!(preview.mods.len(), 1);

    let restored = fx.unpack(&dest).expect("unpack");
    assert_eq!(restored.profile.mode, Mode::Live);
    assert_eq!(restored.profile.name, "Source");
    let sd = restored.profile.save_data_dir();
    let read = |rel: &str| std::fs::read_to_string(sd.join(rel)).expect(rel);
    assert_eq!(read("Config/ModsConfig.xml"), MOD_LIST);
    assert_eq!(read("Config/Mod_1_A.xml"), "<settings>a</settings>");
    assert_eq!(read("Scenarios/My.rsc"), "<scenario/>");
    assert_eq!(read("Saves/A.rws"), "save a");
    for left_out in [
        "Config/Prefs.xml",
        "Config/notes.bin",
        "Saves/B.rws",
        "Logs/x.txt",
    ] {
        assert!(
            !sd.join(left_out).exists(),
            "{left_out} must not be restored"
        );
    }
}

#[test]
fn a_pack_without_saves_or_ticks_holds_only_the_mod_list() {
    let fx = fixture();
    let mut s = spec(&[]);
    s.includes.mod_settings = false;
    s.includes.creations = false;
    let plan = plan_pack(&fx.source, &fx.id, s).expect("plan");
    assert_eq!(plan.entries, 1);
    let dest = fx.tmp.path().join("small.rimpack");
    apply_pack(&plan, &dest, &Probe(false), &mut |_| {}).expect("pack");
    let restored = fx.unpack(&dest).expect("unpack");
    let sd = restored.profile.save_data_dir();
    assert!(sd.join("Config/ModsConfig.xml").is_file());
    assert!(!sd.join("Saves").exists());
}

#[test]
fn restoring_twice_makes_two_profiles() {
    let fx = fixture();
    let dest = fx.pack(&["A.rws"]).expect("pack");
    let first = fx.unpack(&dest).expect("first");
    let second = fx.unpack(&dest).expect("second");
    assert_ne!(first.profile.id, second.profile.id);
}

#[test]
fn pack_refuses_what_it_cannot_hold() {
    let fx = fixture();
    assert!(matches!(
        fx.pack(&["Missing.rws"]),
        Err(ProfileError::BadModpack { .. })
    ));
    assert!(matches!(
        fx.pack(&["../escape.rws"]),
        Err(ProfileError::BadModpack { .. })
    ));
    let plan = plan_pack(&fx.source, &fx.id, spec(&[])).expect("plan");
    let dest = fx.tmp.path().join("busy.rimpack");
    assert!(matches!(
        apply_pack(&plan, &dest, &Probe(true), &mut |_| {}),
        Err(ProfileError::GameRunning)
    ));
    assert!(!dest.exists());
    let empty = fx.source.create("Empty").expect("create");
    assert!(matches!(
        plan_pack(&fx.source, &empty.id, spec(&[])),
        Err(ProfileError::NoModList(_))
    ));
}

#[cfg(unix)]
#[test]
fn pack_never_follows_a_link() {
    let fx = fixture();
    let sd = fx.source.get(&fx.id).expect("profile").save_data_dir();
    std::os::unix::fs::symlink("/etc/hostname", sd.join("Config/Mod_link.xml")).expect("link");
    let plan = plan_pack(&fx.source, &fx.id, spec(&[])).expect("plan");
    assert!(plan.skipped.contains(&PathBuf::from("Config/Mod_link.xml")));
    assert_eq!(plan.entries, 3, "list, one setting, one creation");
}

#[test]
fn a_pack_whose_first_entry_is_not_the_manifest_is_refused() {
    let fx = fixture();
    let mut tarball = tar::Builder::new(Vec::new());
    let mut header = tar::Header::new_gnu();
    header.set_size(0);
    header.set_mode(0o644);
    tarball
        .append_data(
            &mut header,
            "SaveData/Config/ModsConfig.xml",
            std::io::empty(),
        )
        .expect("append");
    let raw = tarball.into_inner().expect("tar");
    let path = fx.tmp.path().join("order.rimpack");
    std::fs::write(&path, zstd::encode_all(raw.as_slice(), 3).expect("zstd")).expect("write");
    assert!(matches!(
        read_manifest(&path),
        Err(ProfileError::BadModpack { .. })
    ));
}

/// A `.rimpack` made of `entries` (name, kind, body), in the order given, behind a manifest
/// whose totals match them.
fn archive(dir: &Path, manifest: &Manifest, entries: &[(&str, EntryType, &str)]) -> PathBuf {
    let mut tarball = tar::Builder::new(Vec::new());
    let text = manifest.to_toml().expect("toml");
    let mut all = vec![("rimpack.toml", EntryType::Regular, text.as_str())];
    all.extend_from_slice(entries);
    for (name, kind, body) in all {
        let mut header = tar::Header::new_gnu();
        // Raw bytes: the builder itself refuses the hostile names these tests need.
        let raw = name.as_bytes();
        assert!(raw.len() <= 100, "test names fit the header");
        header.as_old_mut().name[..raw.len()].copy_from_slice(raw);
        header.set_entry_type(kind);
        header.set_size(body.len() as u64);
        header.set_mode(0o644);
        if kind == EntryType::Symlink {
            header.set_link_name("elsewhere").expect("link");
        }
        header.set_cksum();
        tarball.append(&header, body.as_bytes()).expect("append");
    }
    let raw = tarball.into_inner().expect("tar");
    let path = dir.join("hand.rimpack");
    let mut f = std::fs::File::create(&path).expect("create");
    f.write_all(&zstd::encode_all(raw.as_slice(), 3).expect("zstd"))
        .expect("write");
    path
}

/// Manifest totals for `entries`, with the given ticks.
fn manifest_for(entries: &[(&str, EntryType, &str)], saves: &[&str]) -> Manifest {
    let mut m = sample();
    m.start_save = None;
    m.includes = includes(saves);
    m.entry_count = entries.len() as u64;
    m.uncompressed_bytes = entries.iter().map(|e| e.2.len() as u64).sum();
    m
}

const FILE: EntryType = EntryType::Regular;

/// One archive entry: name, kind, body.
type Raw = (&'static str, EntryType, &'static str);
/// A change to a manifest that no longer matches its archive.
type Tweak = fn(&mut Manifest);

fn list() -> (&'static str, EntryType, &'static str) {
    ("SaveData/Config/ModsConfig.xml", FILE, MOD_LIST)
}

#[test]
fn a_hand_made_pack_restores() {
    let fx = fixture();
    let entries = [list(), ("SaveData/Config/Mod_2_B.xml", FILE, "<b/>")];
    let path = archive(fx.tmp.path(), &manifest_for(&entries, &[]), &entries);
    let restored = fx.unpack(&path).expect("unpack");
    assert!(
        restored
            .profile
            .save_data_dir()
            .join("Config/Mod_2_B.xml")
            .is_file()
    );
}

#[test]
fn restore_refuses_entries_a_pack_may_not_hold() {
    let cases: [(&str, (&str, EntryType, &str)); 12] = [
        ("mods folder", ("Mods/x/About/About.xml", FILE, "<a/>")),
        (
            "mods inside save data",
            ("SaveData/Mods/x/a.xml", FILE, "<a/>"),
        ),
        (
            "library in config",
            ("SaveData/Config/evil.dll", FILE, "MZ"),
        ),
        ("script in config", ("SaveData/Config/run.sh", FILE, "#!")),
        (
            "game settings unticked",
            ("SaveData/Config/Prefs.xml", FILE, "<p/>"),
        ),
        ("undeclared save", ("SaveData/Saves/Z.rws", FILE, "z")),
        ("mod data folder", ("SaveData/HugsLib/x.xml", FILE, "<a/>")),
        ("parent folder", ("SaveData/../escape.xml", FILE, "<a/>")),
        ("absolute path", ("/etc/cron.d/x", FILE, "x")),
        ("profile file", ("profile.toml", FILE, "x")),
        (
            "link",
            ("SaveData/Config/Mod_l.xml", EntryType::Symlink, ""),
        ),
        ("folder", ("SaveData/Config", EntryType::Directory, "")),
    ];
    for (why, bad_entry) in cases {
        let fx = fixture();
        let entries = [list(), bad_entry];
        let mut m = manifest_for(&entries, &[]);
        m.includes.game_settings = false;
        let path = archive(fx.tmp.path(), &m, &entries);
        let outcome = fx.unpack(&path);
        assert!(
            matches!(outcome, Err(ProfileError::BadModpack { .. })),
            "{why}: {outcome:?}"
        );
        assert!(
            fx.leftovers().is_empty(),
            "{why}: left {:?}",
            fx.leftovers()
        );
    }
}

#[test]
fn restore_refuses_a_pack_that_lies_about_itself() {
    let extra = ("SaveData/Config/Mod_2_B.xml", FILE, "<b/>");
    let checks: [(&str, Vec<Raw>, Tweak); 6] = [
        ("more than declared", vec![list(), extra], |m| {
            m.entry_count = 1;
        }),
        ("fewer than declared", vec![list()], |m| m.entry_count = 2),
        ("more data than declared", vec![list()], |m| {
            m.uncompressed_bytes = 1;
        }),
        ("no mod list", vec![extra], |_| {}),
        ("repeated name", vec![list(), list()], |_| {}),
        (
            "second manifest",
            vec![list(), ("rimpack.toml", FILE, "x")],
            |_| {},
        ),
    ];
    for (why, entries, tweak) in checks {
        let fx = fixture();
        let mut m = manifest_for(&entries, &[]);
        tweak(&mut m);
        let path = archive(fx.tmp.path(), &m, &entries);
        let outcome = fx.unpack(&path);
        assert!(
            matches!(outcome, Err(ProfileError::BadModpack { .. })),
            "{why}: {outcome:?}"
        );
        assert!(
            fx.leftovers().is_empty(),
            "{why}: left {:?}",
            fx.leftovers()
        );
    }
}

#[test]
fn restore_holds_the_saves_to_the_manifest() {
    let fx = fixture();
    let entries = [list(), ("SaveData/Saves/A.rws", FILE, "a")];
    // Lists A.rws and B.rws but holds only A.rws.
    let path = archive(
        fx.tmp.path(),
        &manifest_for(&entries, &["A.rws", "B.rws"]),
        &entries,
    );
    assert!(matches!(
        fx.unpack(&path),
        Err(ProfileError::BadModpack { .. })
    ));
    assert!(fx.leftovers().is_empty());
}

#[test]
fn a_file_that_is_not_a_modpack_is_refused() {
    let fx = fixture();
    let path = fx.tmp.path().join("junk.rimpack");
    std::fs::write(&path, "plain text").expect("write");
    assert!(read_manifest(&path).is_err());
    assert!(plan_unpack(&path, &fx.target).is_err());
    assert!(fx.leftovers().is_empty());
}

#[cfg(test)]
mod cover_tests;
