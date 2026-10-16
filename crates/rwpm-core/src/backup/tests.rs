use std::io::Write;
use std::path::Path;

use super::*;
use crate::builds::BuildKey;

fn sample() -> Manifest {
    Manifest {
        schema: file::SCHEMA,
        created_at: "2026-10-16T13:00:00Z".parse().expect("datetime"),
        rwpm_version: "1.10.0".into(),
        name: "Ironman".into(),
        mode: crate::profile::Mode::Offline,
        game_build: Some("1.6.4871-rev598".into()),
        game_version: Some("1.6.4871 rev598".into()),
        os: "linux".into(),
        arch: "x86_64".into(),
        uncompressed_bytes: 4096,
        entry_count: 3,
        includes: Includes {
            saves: true,
            logs: false,
            build: false,
        },
        notice_mods: "mods notice".into(),
        notice_game: "game notice".into(),
    }
}

fn parse(text: &str) -> Result<Manifest, ProfileError> {
    Manifest::parse(text, Path::new("x.rimrim"))
}

/// A `.rimrim` made of `entries` (name, kind, body), in the order given.
fn archive(dir: &Path, entries: &[(&str, tar::EntryType, &[u8])]) -> std::path::PathBuf {
    let mut tarball = tar::Builder::new(Vec::new());
    for (name, kind, body) in entries {
        let mut header = tar::Header::new_gnu();
        header.set_path(name).expect("path");
        header.set_entry_type(*kind);
        header.set_size(body.len() as u64);
        header.set_mode(0o644);
        if matches!(kind, tar::EntryType::Symlink) {
            header.set_link_name("elsewhere").expect("link");
        }
        header.set_cksum();
        tarball.append(&header, *body).expect("append");
    }
    let raw = tarball.into_inner().expect("tar");
    let path = dir.join("test.rimrim");
    let mut f = std::fs::File::create(&path).expect("create");
    f.write_all(&zstd::encode_all(raw.as_slice(), 3).expect("zstd"))
        .expect("write");
    path
}

const FILE: tar::EntryType = tar::EntryType::Regular;

#[test]
fn manifest_round_trips() {
    let text = sample().to_toml().expect("toml");
    assert_eq!(parse(&text).expect("parse"), sample());
}

#[test]
fn manifest_refuses_unknown_fields_and_newer_schema() {
    let text = sample().to_toml().expect("toml");
    let extra = format!("{text}surprise = 1\n");
    assert!(matches!(parse(&extra), Err(ProfileError::BadBackup { .. })));
    let newer = text.replacen("schema = 1", "schema = 2", 1);
    assert!(matches!(
        parse(&newer),
        Err(ProfileError::SchemaTooNew { found: 2, .. })
    ));
}

#[test]
fn manifest_refuses_inconsistent_content() {
    let mut live = sample();
    live.mode = crate::profile::Mode::Live;
    let mut traversal = sample();
    traversal.game_build = Some("../x".into());
    let mut build_without_key = sample();
    build_without_key.game_build = None;
    build_without_key.includes.build = true;
    let mut nameless = sample();
    nameless.name = "  ".into();
    for manifest in [live, traversal, build_without_key, nameless] {
        let text = manifest.to_toml().expect("toml");
        assert!(
            matches!(parse(&text), Err(ProfileError::BadBackup { .. })),
            "{manifest:?}"
        );
    }
}

#[test]
fn classify_accepts_the_three_places() {
    assert_eq!(classify(Path::new("rimrim.toml")), Ok(Place::Manifest));
    assert_eq!(
        classify(Path::new("profile")),
        Ok(Place::Profile("".into()))
    );
    assert_eq!(
        classify(Path::new("profile/Mods/Core 1.6/About/About.xml")),
        Ok(Place::Profile("Mods/Core 1.6/About/About.xml".into()))
    );
    assert_eq!(
        classify(Path::new("build/1.6.4871-rev598/Data/x.dll")),
        Ok(Place::Build {
            key: BuildKey::parse("1.6.4871-rev598").expect("key"),
            rel: "Data/x.dll".into(),
        })
    );
}

#[test]
fn classify_refuses_names_outside_the_places() {
    for name in [
        "",
        "/abs/path",
        "../evil",
        "profile/../../evil",
        "profile/Mods/../../x",
        "./profile/x",
        "profile\\Mods\\x",
        "profile/C:/x",
        "profile/a:b",
        "other/x",
        "rimrim.toml/x",
        "build",
        "build/../x",
        "build/Not A Key/x",
        "build/-bad-/x",
    ] {
        assert!(classify(Path::new(name)).is_err(), "{name:?} was accepted");
    }
}

fn place(name: &str) -> Place {
    classify(Path::new(name)).expect("classify")
}

#[test]
fn check_kind_allows_files_and_folders_everywhere() {
    for name in ["rimrim.toml", "profile/x", "build/1.6/x"] {
        let (name, place) = (Path::new(name), place(name));
        assert!(check_kind(tar::EntryType::Regular, name, &place).is_ok());
        assert!(check_kind(tar::EntryType::Directory, name, &place).is_ok());
    }
}

#[test]
fn check_kind_refuses_links_and_special_files_outside_a_build() {
    for name in ["rimrim.toml", "profile/x", "profile", "build/1.6"] {
        for kind in [tar::EntryType::Symlink, tar::EntryType::Link] {
            assert!(
                check_kind(kind, Path::new(name), &place(name)).is_err(),
                "{kind:?} at {name:?} was accepted"
            );
        }
    }
    let name = "build/1.6/x";
    for kind in [
        tar::EntryType::Link,
        tar::EntryType::Char,
        tar::EntryType::Block,
        tar::EntryType::Fifo,
    ] {
        assert!(
            check_kind(kind, Path::new(name), &place(name)).is_err(),
            "{kind:?} was accepted in a build"
        );
    }
}

#[test]
fn check_kind_allows_a_symlink_below_a_build() {
    let name = "build/1.6/RimWorldWin64_Data";
    assert!(check_kind(tar::EntryType::Symlink, Path::new(name), &place(name)).is_ok());
}

#[test]
fn check_link_target_accepts_what_real_builds_hold() {
    let ok = |link: &str, target: &str| check_link_target(Path::new(link), Path::new(target));
    assert_eq!(ok("RimWorldWin64_Data", "RimWorldLinux_Data"), Ok(()));
    assert_eq!(ok("a/b/l", "../x/y"), Ok(()));
    assert_eq!(ok("a/b/l", "../../x"), Ok(()));
    assert_eq!(ok("l", "a/b"), Ok(()));
}

#[test]
fn check_link_target_refuses_anything_that_could_leave_the_build() {
    let bad = |link: &str, target: &str| check_link_target(Path::new(link), Path::new(target));
    for (link, target) in [
        ("libvulkan.so", "/run/host/usr/lib/libvulkan.so.1.4.363"),
        ("l", "../x"),
        ("a/l", "../../x"),
        ("a/l", "b/../../x"),
        ("l", "a/../b"),
        ("l", "./a"),
        ("l", ".."),
        ("a/l", "../.."),
        ("l", ""),
        ("l", "a\\b"),
        ("l", "C:/x"),
    ] {
        assert!(
            bad(link, target).is_err(),
            "{link:?} -> {target:?} accepted"
        );
    }
}

#[test]
fn check_folder_accepts_real_mod_folders() {
    for name in ["Core", "Mod Name (1.6)", "2009463077", "a.b-c_d"] {
        assert_eq!(check_folder(name), Ok(()), "{name:?}");
    }
}

#[test]
fn check_folder_refuses_anything_that_can_leave_the_profile() {
    for name in [
        "", ".", "..", "a/b", "a\\b", "../x", "..\\x", "C:", "C:\\x", "x.", "x ", "a\0b",
    ] {
        assert!(check_folder(name).is_err(), "{name:?} was accepted");
    }
}

#[test]
fn read_manifest_reads_the_first_entry() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let text = sample().to_toml().expect("toml");
    let path = archive(
        tmp.path(),
        &[
            ("rimrim.toml", FILE, text.as_bytes()),
            ("profile/Mods/Core/x", FILE, b"data"),
        ],
    );
    assert_eq!(read_manifest(&path).expect("manifest"), sample());
}

#[test]
fn read_manifest_refuses_a_manifest_that_is_not_first() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let text = sample().to_toml().expect("toml");
    let path = archive(
        tmp.path(),
        &[
            ("profile/Mods/Core/x", FILE, b"data"),
            ("rimrim.toml", FILE, text.as_bytes()),
        ],
    );
    assert!(matches!(
        read_manifest(&path),
        Err(ProfileError::BadBackup { .. })
    ));
}

#[test]
fn read_manifest_refuses_a_link_in_the_manifest_slot() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = archive(tmp.path(), &[("rimrim.toml", tar::EntryType::Symlink, b"")]);
    assert!(matches!(
        read_manifest(&path),
        Err(ProfileError::BadBackup { .. })
    ));
}

#[test]
fn read_manifest_refuses_an_oversize_manifest() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let big = vec![b'#'; (MAX_MANIFEST_BYTES + 1) as usize];
    let path = archive(tmp.path(), &[("rimrim.toml", FILE, &big)]);
    assert!(matches!(
        read_manifest(&path),
        Err(ProfileError::BadBackup { .. })
    ));
}

#[test]
fn read_manifest_refuses_files_that_are_not_backups() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let text = tmp.path().join("notes.rimrim");
    std::fs::write(&text, b"just some text, not a backup").expect("write");
    assert!(matches!(
        read_manifest(&text),
        Err(ProfileError::BadBackup { .. })
    ));
    let empty = archive(tmp.path(), &[]);
    assert!(matches!(
        read_manifest(&empty),
        Err(ProfileError::BadBackup { .. })
    ));
    let missing = tmp.path().join("missing.rimrim");
    assert!(matches!(
        read_manifest(&missing),
        Err(ProfileError::Io { .. })
    ));
}
