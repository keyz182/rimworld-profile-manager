use std::io::Write;
use std::time::{Duration, UNIX_EPOCH};

use tar::{EntryType, Header};

use super::*;
use crate::backup::file::{Includes, SCHEMA};
#[cfg(unix)]
use crate::backup::pack::tests::link;
use crate::backup::pack::tests::{KEY, NoTrash, Probe, fixture, inc, write};
use crate::builds::effective::test_support::write_game;
use crate::profile::{FrozenMod, ModSource};

/// The machine a backup is restored on: its own profiles root and game-build store.
struct Machine {
    tmp: tempfile::TempDir,
    store: ProfileStore<NoTrash>,
    builds: BuildStore,
}

fn machine() -> Machine {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().join("profiles");
    let app = tmp.path().join("app");
    let store =
        ProfileStore::open(root.clone(), tmp.path().join("game-saves"), NoTrash).expect("store");
    let builds = BuildStore::open(&root, &app).expect("builds");
    Machine { tmp, store, builds }
}

impl Machine {
    fn plan(&self, archive: &Path) -> Result<RestorePlan, ProfileError> {
        plan_restore(archive, &self.store, &self.builds)
    }

    fn restore(&self, archive: &Path) -> Result<RestoreReport, ProfileError> {
        let plan = self.plan(archive)?;
        apply_restore(&plan, &self.store, &self.builds, &Probe(false), &mut |_| {})
    }

    fn profile_dir(&self, report: &RestoreReport) -> PathBuf {
        self.store.root().join(report.profile.id.as_str())
    }

    /// Profile folders and staging folders: what a failed restore must not leave.
    fn leftovers(&self) -> Vec<String> {
        let list = |dir: &Path| -> Vec<String> {
            std::fs::read_dir(dir)
                .map(|d| {
                    d.filter_map(Result::ok)
                        .filter_map(|e| e.file_name().into_string().ok())
                        .collect()
                })
                .unwrap_or_default()
        };
        let mut left: Vec<String> = list(self.store.root())
            .into_iter()
            .filter(|n| n != "builds")
            .collect();
        left.extend(
            list(self.builds.root())
                .into_iter()
                .filter(|n| n.starts_with(".staging-")),
        );
        left
    }
}

/// A hand-made archive entry, so tests can write names `tar::Builder` would refuse.
struct Raw {
    name: String,
    kind: EntryType,
    body: Vec<u8>,
    link: Option<String>,
}

fn file(name: &str, body: &str) -> Raw {
    Raw {
        name: name.into(),
        kind: EntryType::Regular,
        body: body.as_bytes().to_vec(),
        link: None,
    }
}

fn folder(name: &str) -> Raw {
    Raw {
        kind: EntryType::Directory,
        ..file(name, "")
    }
}

fn symlink(name: &str, target: &str) -> Raw {
    Raw {
        kind: EntryType::Symlink,
        link: Some(target.into()),
        ..file(name, "")
    }
}

fn hardlink(name: &str, target: &str) -> Raw {
    Raw {
        kind: EntryType::Link,
        ..symlink(name, target)
    }
}

fn profile_toml(folders: &[&str], game_build: Option<&str>) -> Raw {
    let mut file_ = ProfileFile::new("Crafted");
    file_.schema = if game_build.is_some() { 2 } else { 1 };
    file_.mode = Mode::Offline;
    file_.game_build = game_build.map(str::to_owned);
    file_.frozen_mods = folders
        .iter()
        .map(|f| FrozenMod {
            folder: (*f).into(),
            package_id: "some.mod".into(),
            source: ModSource::Local,
            source_dir: "/elsewhere".into(),
            copied_at: "2026-10-11T19:00:00Z".parse().expect("datetime"),
        })
        .collect();
    file(
        "profile/profile.toml",
        &toml::to_string_pretty(&file_).expect("toml"),
    )
}

fn manifest(entries: &[Raw], includes: Includes, game_build: Option<&str>) -> Manifest {
    Manifest {
        schema: SCHEMA,
        created_at: "2026-10-16T13:00:00Z".parse().expect("datetime"),
        rwpm_version: "1.10.0".into(),
        name: "Crafted".into(),
        mode: Mode::Offline,
        game_build: game_build.map(str::to_owned),
        game_version: None,
        os: std::env::consts::OS.into(),
        arch: std::env::consts::ARCH.into(),
        uncompressed_bytes: entries
            .iter()
            .filter(|e| e.kind == EntryType::Regular)
            .map(|e| e.body.len() as u64)
            .sum(),
        entry_count: entries.len() as u64,
        includes,
        notice_mods: "m".into(),
        notice_game: "g".into(),
    }
}

/// Writes `entries` after `manifest`, in order, as a `.rimrim`.
fn craft(dir: &Path, manifest: &Manifest, entries: &[Raw]) -> PathBuf {
    let mut tarball = tar::Builder::new(Vec::new());
    let text = manifest.to_toml().expect("toml");
    let mut head = Header::new_gnu();
    head.set_size(text.len() as u64);
    head.set_mode(0o644);
    tarball
        .append_data(&mut head, "rimrim.toml", text.as_bytes())
        .expect("manifest");
    for entry in entries {
        let mut header = Header::new_gnu();
        let old = header.as_old_mut();
        old.name[..entry.name.len()].copy_from_slice(entry.name.as_bytes());
        if let Some(target) = &entry.link {
            old.linkname[..target.len()].copy_from_slice(target.as_bytes());
        }
        header.set_entry_type(entry.kind);
        header.set_size(entry.body.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        tarball
            .append(&header, entry.body.as_slice())
            .expect("append");
    }
    let raw = tarball.into_inner().expect("tar");
    let path = dir.join("crafted.rimrim");
    let mut out = std::fs::File::create(&path).expect("create");
    out.write_all(&zstd::encode_all(raw.as_slice(), 3).expect("zstd"))
        .expect("write");
    path
}

fn good() -> Vec<Raw> {
    vec![
        folder("profile/Mods"),
        file("profile/Mods/Core/About.xml", "about"),
        profile_toml(&["Core"], None),
    ]
}

fn crafted_restore(
    m: &Machine,
    entries: &[Raw],
    includes: Includes,
) -> Result<RestoreReport, ProfileError> {
    let archive = craft(m.tmp.path(), &manifest(entries, includes, None), entries);
    m.restore(&archive)
}

fn assert_refused(entries: Vec<Raw>, includes: Includes) {
    let m = machine();
    let result = crafted_restore(&m, &entries, includes);
    assert!(
        matches!(result, Err(ProfileError::BadBackup { .. })),
        "{result:?}"
    );
    assert_eq!(
        m.leftovers(),
        Vec::<String>::new(),
        "a failed restore left files"
    );
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).expect("read")
}

#[test]
fn round_trip_restores_a_new_profile_without_a_build() {
    let a = fixture(false);
    let saved = a.dir().join("SaveData/Saves/colony.rws");
    let when = UNIX_EPOCH + Duration::from_secs(1_000_000_000);
    std::fs::File::options()
        .write(true)
        .open(&saved)
        .and_then(|f| f.set_modified(when))
        .expect("mtime");
    a.run(inc(true, true, false)).expect("backup");
    let b = machine();
    let report = b.restore(&a.dest()).expect("restore");
    let dir = b.profile_dir(&report);
    assert_eq!(report.profile.id.as_str(), "source-run");
    assert_eq!(report.profile.name, "Source Run");
    assert_eq!(report.profile.mode, Mode::Offline);
    assert!(report.notes.is_empty(), "{:?}", report.notes);
    assert_eq!(read(&dir.join("Mods/2009463077/About/About.xml")), "about");
    assert_eq!(read(&dir.join("SaveData/Saves/colony.rws")), "save data");
    assert_eq!(
        read(&dir.join("SaveData/Config/ModsConfig.xml")),
        "<config/>"
    );
    assert_eq!(read(&dir.join("Player.log")), "log");
    assert_eq!(read(&dir.join("Logs/Player.log")), "waylith");
    let modified = dir.join("SaveData/Saves/colony.rws");
    let modified = std::fs::metadata(modified)
        .and_then(|m| m.modified())
        .expect("mtime");
    assert_eq!(modified, when, "saves keep their modified time");
    let file = ProfileFile::read(&dir.join("profile.toml")).expect("profile.toml");
    assert_eq!(file.frozen_mods.len(), 1);
    assert!(!dir.join("profile.toml.tmp").exists());
}

#[test]
fn saves_and_logs_are_left_out_when_not_chosen() {
    let a = fixture(false);
    a.run(inc(false, false, false)).expect("backup");
    let b = machine();
    let dir = b.profile_dir(&b.restore(&a.dest()).expect("restore"));
    assert!(dir.join("SaveData/Config/ModsConfig.xml").is_file());
    assert!(!dir.join("SaveData/Saves").exists());
    assert!(!dir.join("Player.log").exists() && !dir.join("Logs").exists());
}

#[test]
fn restoring_twice_makes_two_profiles_and_never_overwrites() {
    let a = fixture(false);
    a.run(inc(true, false, false)).expect("backup");
    let b = machine();
    let first = b.restore(&a.dest()).expect("first");
    write(&b.profile_dir(&first).join("Mods/own-change.txt"), "mine");
    let second = b.restore(&a.dest()).expect("second");
    assert_eq!(first.profile.id.as_str(), "source-run");
    assert_eq!(second.profile.id.as_str(), "source-run-2");
    assert_eq!(
        read(&b.profile_dir(&first).join("Mods/own-change.txt")),
        "mine"
    );
}

#[cfg(unix)]
#[test]
fn executable_files_stay_executable_and_others_do_not() {
    use std::os::unix::fs::PermissionsExt;
    let a = fixture(false);
    let tool = a.dir().join("Mods/2009463077/run.sh");
    write(&tool, "#!/bin/sh");
    std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o4755)).expect("chmod");
    a.run(inc(true, false, false)).expect("backup");
    let b = machine();
    let dir = b.profile_dir(&b.restore(&a.dest()).expect("restore"));
    let mode = |p: &str| {
        std::fs::metadata(dir.join(p))
            .expect("meta")
            .permissions()
            .mode()
            & 0o7777
    };
    assert_eq!(
        mode("Mods/2009463077/run.sh"),
        0o755,
        "setuid is never restored"
    );
    assert_eq!(mode("Mods/2009463077/About/About.xml"), 0o644);
}

#[cfg(unix)]
#[test]
fn a_build_is_restored_with_its_safe_links_and_the_pin_kept() {
    let a = fixture(true);
    let key = BuildKey::parse(KEY).expect("key");
    let build = a.builds.dir_of(&key);
    write_game(&build, "1.6.4871 rev598");
    link("RimWorldLinux_Data", &build.join("RimWorldWin64_Data"));
    link(
        "/run/host/usr/lib/libvulkan.so.1",
        &build.join("libvulkan.so"),
    );
    a.run(inc(true, false, true)).expect("backup");
    let b = machine();
    let report = b.restore(&a.dest()).expect("restore");
    let stored = b.builds.dir_of(&key);
    assert!(stored.join("build.toml").is_file());
    assert!(stored.join("Version.txt").is_file());
    assert_eq!(
        std::fs::read_link(stored.join("RimWorldWin64_Data")).expect("link"),
        PathBuf::from("RimWorldLinux_Data")
    );
    assert!(std::fs::symlink_metadata(stored.join("libvulkan.so")).is_err());
    assert_eq!(report.profile.game_build.as_deref(), Some(KEY));
    assert!(report.notes.is_empty(), "{:?}", report.notes);
    // The restored profile stays; no staging folder is left behind.
    assert_eq!(b.leftovers(), ["source-run"]);
    assert!(b.builds.get(&key).is_ok(), "the stored build is whole");
}

#[test]
fn a_build_stored_here_is_kept_and_a_different_copy_is_reported() {
    let a = fixture(true);
    let key = BuildKey::parse(KEY).expect("key");
    write_game(&a.builds.dir_of(&key), "1.6.4871 rev598");
    a.run(inc(true, false, true)).expect("backup");

    let same = machine();
    let theirs = same.builds.dir_of(&key);
    write_game(&theirs, "1.6.4871 rev598");
    std::fs::copy(
        a.builds.dir_of(&key).join("build.toml"),
        theirs.join("build.toml"),
    )
    .expect("toml");
    write(&theirs.join("marker.txt"), "mine");
    let report = same.restore(&a.dest()).expect("restore");
    assert!(report.notes.is_empty(), "{:?}", report.notes);
    assert_eq!(read(&theirs.join("marker.txt")), "mine");

    let other = machine();
    let theirs = other.builds.dir_of(&key);
    write_game(&theirs, "1.6.4871 rev598");
    let mut file = BuildFile::read(&a.builds.dir_of(&key).join("build.toml")).expect("read");
    file.buildid = "999".into();
    file.write_atomic(&theirs.join("build.toml"))
        .expect("write");
    let report = other.restore(&a.dest()).expect("restore");
    assert_eq!(report.notes, [RestoreNote::BuildDiffers(KEY.into())]);
}

#[test]
fn a_build_for_another_system_is_skipped_and_the_pin_kept() {
    let build_entries = vec![
        file("build/1.6.4871-rev598/RimWorld.exe", "exe"),
        file("build/1.6.4871-rev598/build.toml", "ignored"),
        profile_toml(&[], Some(KEY)),
    ];
    let mut m = manifest(&build_entries, inc(false, false, true), Some(KEY));
    m.os = "plan9".into();
    let machine = machine();
    let archive = craft(machine.tmp.path(), &m, &build_entries);
    let plan = machine.plan(&archive).expect("plan");
    assert!(
        plan.notes
            .iter()
            .any(|n| matches!(n, RestoreNote::BuildOtherSystem { .. }))
    );
    let report = machine.restore(&archive).expect("restore");
    assert_eq!(report.profile.game_build.as_deref(), Some(KEY));
    assert!(
        !machine
            .builds
            .dir_of(&BuildKey::parse(KEY).expect("key"))
            .exists()
    );
    assert!(
        report
            .notes
            .contains(&RestoreNote::BuildMissing(KEY.into()))
    );
    // The profile stays; no staging folder is left behind.
    assert_eq!(machine.leftovers(), ["crafted"]);
}

#[test]
fn a_pin_with_no_stored_build_and_none_in_the_backup_is_reported() {
    let a = fixture(true);
    a.run(inc(true, false, false)).expect("backup");
    let b = machine();
    let report = b.restore(&a.dest()).expect("restore");
    assert_eq!(report.profile.game_build.as_deref(), Some(KEY));
    assert_eq!(report.notes, [RestoreNote::BuildMissing(KEY.into())]);
}

#[test]
fn a_build_that_is_not_a_game_folder_is_left_out_with_a_note() {
    let a = fixture(true);
    let key = BuildKey::parse(KEY).expect("key");
    // No game executable on any platform: build.toml is fine, the folder is not a game.
    let _ = std::fs::remove_file(a.builds.dir_of(&key).join("RimWorldLinux"));
    a.run(inc(true, false, true)).expect("backup");
    let b = machine();
    let report = b.restore(&a.dest()).expect("restore");
    assert!(
        matches!(report.notes.as_slice(), [RestoreNote::BuildBroken { .. }]),
        "{:?}",
        report.notes
    );
    assert!(
        !b.builds.dir_of(&key).exists(),
        "no half build in the store"
    );
    assert_eq!(
        b.leftovers(),
        ["source-run"],
        "only the profile, no staging"
    );
    assert_eq!(report.profile.game_build.as_deref(), Some(KEY));
}

/// Control for every refusal test below: the same helpers, with nothing wrong, restore.
/// Without it a broken `craft` would make each "is refused" test pass for the wrong reason.
#[test]
fn a_plain_crafted_archive_restores() {
    let m = machine();
    let report = crafted_restore(&m, &good(), inc(true, false, false)).expect("restore");
    assert_eq!(report.profile.name, "Crafted");
    let dir = m.profile_dir(&report);
    assert_eq!(read(&dir.join("Mods/Core/About.xml")), "about");
    assert!(report.notes.is_empty(), "{:?}", report.notes);
}

#[test]
fn names_outside_the_allowed_places_are_refused_and_nothing_is_left() {
    for bad in [
        "profile/../../evil",
        "profile/Mods/../x",
        "/abs/evil",
        "other/evil",
        "profile/random.txt",
        "profile/Mods/a:b/x",
        "profile\\Mods\\x",
    ] {
        let mut entries = vec![
            folder("profile/Mods"),
            file("profile/Mods/Core/About.xml", "a"),
        ];
        entries.push(file(bad, "x"));
        entries.push(profile_toml(&["Core"], None));
        assert_refused(entries, inc(true, false, false));
    }
}

#[test]
fn links_are_refused_outside_a_build_and_hardlinks_everywhere() {
    let mut a = good();
    a.insert(1, symlink("profile/Mods/Core/link", "About.xml"));
    assert_refused(a, inc(true, false, false));
    let mut b = good();
    b.insert(
        1,
        hardlink("profile/Mods/Core/hard", "profile/Mods/Core/About.xml"),
    );
    assert_refused(b, inc(true, false, false));
}

#[test]
fn unsafe_build_links_are_refused() {
    for target in ["/etc/passwd", "../x", "a/../b", "./a", "..", "C:/x"] {
        let entries = vec![
            file("build/1.6.4871-rev598/build.toml", "x"),
            symlink("build/1.6.4871-rev598/l", target),
            profile_toml(&[], Some(KEY)),
        ];
        let m = machine();
        let manifest = manifest(&entries, inc(false, false, true), Some(KEY));
        let archive = craft(m.tmp.path(), &manifest, &entries);
        let result = m.restore(&archive);
        assert!(
            matches!(result, Err(ProfileError::BadBackup { .. })),
            "{target}: {result:?}"
        );
        assert_eq!(m.leftovers(), Vec::<String>::new(), "{target}");
    }
}

#[test]
fn manifest_and_profile_strings_that_could_leave_the_profile_are_refused() {
    for folder_name in ["../x", "a/b", "..", "", "a\\b", "C:"] {
        let entries = vec![
            folder("profile/Mods"),
            file("profile/Mods/Core/About.xml", "a"),
            profile_toml(&["Core", folder_name], None),
        ];
        assert_refused(entries, inc(true, false, false));
    }
    let ghost = vec![
        folder("profile/Mods"),
        file("profile/Mods/Core/About.xml", "a"),
        profile_toml(&["Core", "Ghost"], None),
    ];
    assert_refused(ghost, inc(true, false, false));
}

#[test]
fn entries_the_manifest_does_not_declare_are_refused() {
    let saves = vec![
        file("profile/SaveData/Saves/x.rws", "x"),
        profile_toml(&[], None),
    ];
    assert_refused(saves, inc(false, false, false));
    let logs = vec![
        file("profile/logs/Player.log", "x"),
        profile_toml(&[], None),
    ];
    assert_refused(logs, inc(true, false, false));
    let build = vec![
        file("build/1.6.4871-rev598/build.toml", "x"),
        profile_toml(&[], None),
    ];
    assert_refused(build, inc(true, false, false));
    let sneaky = vec![
        file("profile/logs/profile.toml", "x"),
        profile_toml(&[], None),
    ];
    assert_refused(sneaky, inc(true, true, false));
}

#[test]
fn order_rules_hold() {
    let mut late = good();
    late.push(file("profile/Mods/Core/late.txt", "x"));
    assert_refused(late, inc(true, false, false));
    let mut no_profile = good();
    no_profile.pop();
    assert_refused(no_profile, inc(true, false, false));
    let mut second = good();
    second.insert(0, file("rimrim.toml", "x"));
    assert_refused(second, inc(true, false, false));
}

#[test]
fn a_repeated_name_does_not_overwrite_the_earlier_entry() {
    let mut entries = good();
    entries.insert(2, file("profile/Mods/Core/About.xml", "second"));
    assert_refused(entries, inc(true, false, false));
}

#[test]
fn a_manifest_that_understates_or_overstates_is_caught_while_streaming() {
    for (bytes, count) in [(-1_i64, 0_i64), (0, -1), (0, 1), (1, 0)] {
        let entries = good();
        let mut m = manifest(&entries, inc(true, false, false), None);
        m.uncompressed_bytes = m.uncompressed_bytes.saturating_add_signed(bytes);
        m.entry_count = m.entry_count.saturating_add_signed(count);
        let machine = machine();
        let archive = craft(machine.tmp.path(), &m, &entries);
        let result = machine.restore(&archive);
        assert!(
            matches!(result, Err(ProfileError::BadBackup { .. })),
            "{bytes} {count}: {result:?}"
        );
        assert_eq!(machine.leftovers(), Vec::<String>::new(), "{bytes} {count}");
    }
}

#[test]
fn a_backup_larger_than_any_allowed_is_refused_before_it_is_read() {
    let entries = good();
    let mut m = manifest(&entries, inc(true, false, false), None);
    m.uncompressed_bytes = MAX_BYTES + 1;
    let machine = machine();
    let archive = craft(machine.tmp.path(), &m, &entries);
    assert!(matches!(
        machine.plan(&archive),
        Err(ProfileError::BadBackup { .. })
    ));
    m.uncompressed_bytes = 0;
    m.entry_count = MAX_ENTRIES + 1;
    let archive = craft(machine.tmp.path(), &m, &entries);
    assert!(matches!(
        machine.plan(&archive),
        Err(ProfileError::BadBackup { .. })
    ));
}

#[test]
fn a_running_game_or_a_held_lock_refuses_and_claims_nothing() {
    let a = fixture(false);
    a.run(inc(true, false, false)).expect("backup");
    let b = machine();
    let plan = b.plan(&a.dest()).expect("plan");
    let running = apply_restore(&plan, &b.store, &b.builds, &Probe(true), &mut |_| {});
    assert!(matches!(running, Err(ProfileError::GameRunning)));
    let _held = crate::profile::lock::take_run_lock(b.builds.app_dir()).expect("lock");
    let locked = apply_restore(&plan, &b.store, &b.builds, &Probe(false), &mut |_| {});
    assert!(matches!(locked, Err(ProfileError::GameRunning)));
    assert_eq!(b.leftovers(), Vec::<String>::new());
}

#[test]
fn files_that_are_not_backups_are_refused_and_a_blank_name_is_rejected() {
    let b = machine();
    let text = b.tmp.path().join("notes.rimrim");
    std::fs::write(&text, "not a backup").expect("write");
    assert!(matches!(b.plan(&text), Err(ProfileError::BadBackup { .. })));
    let a = fixture(false);
    a.run(inc(true, false, false)).expect("backup");
    let mut plan = b.plan(&a.dest()).expect("plan");
    plan.name = "   ".into();
    let result = apply_restore(&plan, &b.store, &b.builds, &Probe(false), &mut |_| {});
    assert!(matches!(result, Err(ProfileError::EmptyName)));
    assert_eq!(b.leftovers(), Vec::<String>::new());
}

#[test]
fn the_restore_name_can_be_changed_before_applying() {
    let a = fixture(false);
    a.run(inc(true, false, false)).expect("backup");
    let b = machine();
    let mut plan = b.plan(&a.dest()).expect("plan");
    plan.name = "Fresh Start".into();
    let report =
        apply_restore(&plan, &b.store, &b.builds, &Probe(false), &mut |_| {}).expect("restore");
    assert_eq!(report.profile.name, "Fresh Start");
    assert_eq!(report.profile.id.as_str(), "fresh-start");
}

#[test]
fn progress_ends_at_the_declared_total() {
    let a = fixture(false);
    let report = a.run(inc(true, true, false)).expect("backup");
    let b = machine();
    let plan = b.plan(&a.dest()).expect("plan");
    let mut last = 0;
    apply_restore(&plan, &b.store, &b.builds, &Probe(false), &mut |n| last = n).expect("restore");
    assert_eq!(last, report.manifest.uncompressed_bytes);
}
