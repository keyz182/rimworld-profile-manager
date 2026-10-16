use std::path::Path;
use std::time::{Duration, UNIX_EPOCH};

use super::*;
use crate::builds::BuildFile;
use crate::profile::lock::take_run_lock;
use crate::profile::{FrozenMod, ModSource, ProfileFile};

pub(crate) const KEY: &str = "1.6.4871-rev598";

pub(crate) struct NoTrash;

impl Trash for NoTrash {
    fn trash(&self, _: &Path) -> io::Result<()> {
        Ok(())
    }
}

pub(crate) struct Probe(pub(crate) bool);

impl RunningProbe for Probe {
    fn is_running(&self, _: &Path) -> bool {
        self.0
    }

    fn any_game(&self) -> bool {
        self.0
    }
}

pub(crate) struct Fx {
    pub(crate) tmp: tempfile::TempDir,
    pub(crate) app: PathBuf,
    pub(crate) store: ProfileStore<NoTrash>,
    pub(crate) builds: BuildStore,
    pub(crate) id: ProfileId,
}

impl Fx {
    pub(crate) fn dir(&self) -> PathBuf {
        self.store.root().join(self.id.as_str())
    }

    pub(crate) fn dest(&self) -> PathBuf {
        self.tmp.path().join("out.rimrim")
    }

    pub(crate) fn plan(&self, includes: Includes) -> Result<BackupPlan, ProfileError> {
        plan_backup(&self.store, &self.builds, &self.id, includes)
    }

    pub(crate) fn run(&self, includes: Includes) -> Result<BackupReport, ProfileError> {
        let plan = self.plan(includes)?;
        apply_backup(&plan, &self.dest(), &self.app, &Probe(false), &mut |_| {})
    }
}

pub(crate) fn write(path: &Path, body: &str) {
    std::fs::create_dir_all(path.parent().expect("parent")).expect("dirs");
    std::fs::write(path, body).expect("write");
}

pub(crate) fn inc(saves: bool, logs: bool, build: bool) -> Includes {
    Includes { saves, logs, build }
}

/// An OFFLINE profile with a save, a mod, logs and a stored build it is pinned to.
pub(crate) fn fixture(pinned: bool) -> Fx {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().join("profiles");
    let app = tmp.path().join("app");
    let store =
        ProfileStore::open(root.clone(), tmp.path().join("game-saves"), NoTrash).expect("open");
    let profile = store.create("Source Run").expect("create");
    let dir = root.join(profile.id.as_str());
    write(&dir.join("SaveData/Saves/colony.rws"), "save data");
    write(&dir.join("SaveData/Config/ModsConfig.xml"), "<config/>");
    write(&dir.join("Mods/2009463077/About/About.xml"), "about");
    write(&dir.join("Player.log"), "log");
    write(&dir.join("payload.log"), "payload");
    write(&dir.join("Logs/Player.log"), "waylith");
    let mut file = ProfileFile::read(&dir.join(crate::profile::file::FILE_NAME)).expect("read");
    file.mode = Mode::Offline;
    file.game_build = pinned.then(|| KEY.to_owned());
    file.frozen_mods = vec![FrozenMod {
        folder: "2009463077".into(),
        package_id: "brrainz.harmony".into(),
        source: ModSource::Workshop,
        source_dir: "/ws/2009463077".into(),
        copied_at: "2026-10-11T19:00:00Z".parse().expect("datetime"),
    }];
    file.write_atomic(&dir.join(crate::profile::file::FILE_NAME))
        .expect("write");
    let builds = BuildStore::open(&root, &app).expect("builds");
    let build = builds.dir_of(&BuildKey::parse(KEY).expect("key"));
    write(&build.join("RimWorldLinux"), "exe");
    write(&build.join("RimWorldLinux_Data/data.assets"), "assets");
    BuildFile {
        schema: crate::builds::file::SCHEMA,
        version: "1.6.4871 rev598".into(),
        buildid: "123".into(),
        beta: String::new(),
        dlc: vec![],
        captured_at: "2026-10-13T17:00:00Z".parse().expect("datetime"),
        source_dir: "/steam/RimWorld".into(),
        size: 9,
    }
    .write_atomic(&build.join(crate::builds::file::FILE_NAME))
    .expect("build.toml");
    Fx {
        tmp,
        app,
        store,
        builds,
        id: profile.id,
    }
}

#[cfg(unix)]
pub(crate) fn link(target: &str, at: &Path) {
    std::os::unix::fs::symlink(target, at).expect("symlink");
}

pub(crate) struct Entry {
    pub(crate) name: String,
    pub(crate) kind: EntryType,
    pub(crate) link: Option<String>,
    pub(crate) size: u64,
}

pub(crate) fn entries(path: &Path) -> Vec<Entry> {
    let mut archive = crate::backup::open(path).expect("open");
    archive
        .entries()
        .expect("entries")
        .map(|e| {
            let e = e.expect("entry");
            Entry {
                name: e.path().expect("path").to_string_lossy().into_owned(),
                kind: e.header().entry_type(),
                link: e
                    .link_name()
                    .expect("link")
                    .map(|l| l.to_string_lossy().into_owned()),
                size: e.size(),
            }
        })
        .collect()
}

pub(crate) fn names(path: &Path) -> Vec<String> {
    entries(path).into_iter().map(|e| e.name).collect()
}

#[test]
fn backup_writes_the_manifest_first_and_profile_toml_last() {
    let fx = fixture(false);
    let mut last = 0;
    let plan = fx.plan(inc(true, false, false)).expect("plan");
    let report =
        apply_backup(&plan, &fx.dest(), &fx.app, &Probe(false), &mut |n| last = n).expect("backup");
    let all = entries(&fx.dest());
    assert_eq!(all.first().map(|e| e.name.as_str()), Some("rimrim.toml"));
    assert_eq!(
        all.last().map(|e| e.name.as_str()),
        Some("profile/profile.toml")
    );
    let names: Vec<_> = all.iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains(&"profile/Mods/2009463077/About/About.xml"));
    assert!(names.contains(&"profile/SaveData/Saves/colony.rws"));
    assert!(!names.iter().any(|n| n.starts_with("profile/logs")));
    assert!(
        all.iter().all(|e| e.link.is_none()),
        "no links outside a build"
    );
    let after_manifest = &all[1..];
    let file_bytes: u64 = after_manifest
        .iter()
        .filter(|e| e.kind == EntryType::Regular)
        .map(|e| e.size)
        .sum();
    assert_eq!(report.manifest.uncompressed_bytes, file_bytes);
    assert_eq!(report.manifest.entry_count, after_manifest.len() as u64);
    assert_eq!(report.manifest.uncompressed_bytes, plan.bytes);
    assert_eq!(last, file_bytes, "progress ends at the total");
    assert_eq!(
        crate::backup::read_manifest(&fx.dest()).expect("manifest"),
        report.manifest
    );
    assert!(!fx.tmp.path().join(".out.rimrim.tmp").exists());
}

#[test]
fn backup_keeps_modified_times_and_drops_owner_names() {
    let fx = fixture(false);
    let save = fx.dir().join("SaveData/Saves/colony.rws");
    let when = UNIX_EPOCH + Duration::from_secs(1_000_000_000);
    File::options()
        .write(true)
        .open(&save)
        .and_then(|f| f.set_modified(when))
        .expect("set mtime");
    fx.run(inc(true, false, false)).expect("backup");
    let mut archive = crate::backup::open(&fx.dest()).expect("open");
    let entry = archive
        .entries()
        .expect("entries")
        .map(|e| e.expect("entry"))
        .find(|e| e.path().is_ok_and(|p| p.ends_with("colony.rws")))
        .expect("save entry");
    assert_eq!(entry.header().mtime().expect("mtime"), 1_000_000_000);
    assert_eq!(entry.header().uid().expect("uid"), 0);
    assert_eq!(entry.header().username().expect("user"), Some(""));
}

#[test]
fn saves_off_keeps_only_the_config_folder() {
    let fx = fixture(false);
    fx.run(inc(false, false, false)).expect("backup");
    let names = names(&fx.dest());
    assert!(names.contains(&"profile/SaveData/Config/ModsConfig.xml".to_owned()));
    assert!(!names.iter().any(|n| n.contains("Saves")));
}

#[test]
fn logs_go_under_one_archive_folder() {
    let fx = fixture(false);
    fx.run(inc(true, true, false)).expect("backup");
    let names = names(&fx.dest());
    for want in [
        "profile/logs/Player.log",
        "profile/logs/payload.log",
        "profile/logs/Logs/Player.log",
    ] {
        assert!(names.contains(&want.to_owned()), "{want} missing");
    }
    assert!(!names.contains(&"profile/Player.log".to_owned()));
}

#[cfg(unix)]
#[test]
fn build_part_is_ordered_and_carries_only_safe_links() {
    let fx = fixture(true);
    let build = fx.builds.dir_of(&BuildKey::parse(KEY).expect("key"));
    link("RimWorldLinux_Data", &build.join("RimWorldWin64_Data"));
    link(
        "/run/host/usr/lib/libvulkan.so.1",
        &build.join("libvulkan.so"),
    );
    let report = fx.run(inc(true, false, true)).expect("backup");
    let all = entries(&fx.dest());
    let at = |name: &str| {
        all.iter()
            .position(|e| e.name == name)
            .unwrap_or_else(|| panic!("{name} missing"))
    };
    let (data, safe) = (
        at(&format!("build/{KEY}/RimWorldLinux_Data/data.assets")),
        at(&format!("build/{KEY}/RimWorldWin64_Data")),
    );
    let (toml, profile_toml) = (
        at(&format!("build/{KEY}/build.toml")),
        at("profile/profile.toml"),
    );
    assert!(
        data < safe && safe < toml && toml < profile_toml,
        "{data} {safe} {toml}"
    );
    assert_eq!(all[safe].kind, EntryType::Symlink);
    assert_eq!(all[safe].link.as_deref(), Some("RimWorldLinux_Data"));
    assert!(all.iter().all(|e| !e.name.ends_with("libvulkan.so")));
    assert_eq!(report.skipped_links, [PathBuf::from("libvulkan.so")]);
    assert!(report.manifest.includes.build);
    assert_eq!(report.manifest.game_build.as_deref(), Some(KEY));
    assert_eq!(
        report.manifest.game_version.as_deref(),
        Some("1.6.4871 rev598")
    );
}

#[test]
fn only_offline_profiles_stored_builds_and_real_pins_can_be_backed_up() {
    let fx = fixture(false);
    let live = fx.store.create("Live One").expect("create");
    assert!(matches!(
        plan_backup(&fx.store, &fx.builds, &live.id, inc(true, false, false)),
        Err(ProfileError::NotOffline(_))
    ));
    assert!(matches!(
        plan_backup(
            &fx.store,
            &fx.builds,
            &ProfileId::default_id(),
            inc(true, false, false)
        ),
        Err(ProfileError::DefaultImmutable(_))
    ));
    assert!(matches!(
        fx.plan(inc(true, false, true)),
        Err(ProfileError::NoBuildToBackUp(_))
    ));
}

#[test]
fn a_missing_pinned_build_only_matters_when_it_is_included() {
    let fx = fixture(true);
    let build = fx.builds.dir_of(&BuildKey::parse(KEY).expect("key"));
    std::fs::remove_dir_all(build).expect("remove build");
    assert!(matches!(
        fx.plan(inc(true, false, true)),
        Err(ProfileError::BuildNotFound(_))
    ));
    let report = fx.run(inc(true, false, false)).expect("backup");
    assert_eq!(report.manifest.game_build.as_deref(), Some(KEY));
    assert_eq!(report.manifest.game_version, None);
}

#[cfg(unix)]
#[test]
fn links_in_mods_stop_the_backup_and_are_named() {
    let fx = fixture(false);
    link("About", &fx.dir().join("Mods/2009463077/shortcut"));
    match fx.plan(inc(true, false, false)) {
        Err(ProfileError::NotBackupable(named)) => {
            assert!(named.iter().any(|n| n.ends_with("shortcut")), "{named:?}");
        }
        other => panic!("expected NotBackupable, got {other:?}"),
    }
}

#[cfg(unix)]
#[test]
fn names_restore_would_refuse_stop_the_backup() {
    let fx = fixture(false);
    write(&fx.dir().join("Mods/2009463077/a:b.txt"), "x");
    assert!(matches!(
        fx.plan(inc(true, false, false)),
        Err(ProfileError::NotBackupable(_))
    ));
}

#[test]
fn a_running_game_or_a_held_lock_refuses_and_leaves_nothing() {
    let fx = fixture(false);
    let plan = fx.plan(inc(true, false, false)).expect("plan");
    let tmp_name = fx.tmp.path().join(".out.rimrim.tmp");
    let running = apply_backup(&plan, &fx.dest(), &fx.app, &Probe(true), &mut |_| {});
    assert!(matches!(running, Err(ProfileError::GameRunning)));
    let _held = take_run_lock(&fx.app).expect("lock");
    let locked = apply_backup(&plan, &fx.dest(), &fx.app, &Probe(false), &mut |_| {});
    assert!(matches!(locked, Err(ProfileError::GameRunning)));
    assert!(!fx.dest().exists() && !tmp_name.exists());
}

#[test]
fn a_failed_backup_leaves_no_file_and_keeps_an_existing_one() {
    let fx = fixture(false);
    let plan = fx.plan(inc(true, false, false)).expect("plan");
    let nowhere = fx.tmp.path().join("nope/out.rimrim");
    let failed = apply_backup(&plan, &nowhere, &fx.app, &Probe(false), &mut |_| {});
    assert!(matches!(failed, Err(ProfileError::Io { .. })));
    assert!(!fx.tmp.path().join("nope").exists());
    std::fs::write(fx.dest(), "older backup").expect("write");
    fx.run(inc(true, false, false)).expect("backup");
    assert!(
        crate::backup::read_manifest(&fx.dest()).is_ok(),
        "replaced on success"
    );
}
