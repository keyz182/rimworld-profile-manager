use crate::discovery::Engine;
use std::cell::RefCell;

use super::*;
use crate::discovery::SteamInfo;

#[derive(Default)]
struct RecordingTrash(RefCell<Vec<PathBuf>>);

impl Trash for &RecordingTrash {
    fn trash(&self, path: &Path) -> std::io::Result<()> {
        self.0.borrow_mut().push(path.to_path_buf());
        // Stand in for the recycle bin: the path must disappear.
        std::fs::remove_dir_all(path)
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

const IDLE: FakeProbe = FakeProbe(false);

/// A fake install (Data, Mods), workshop dir and store with one LIVE profile.
struct Fixture {
    tmp: tempfile::TempDir,
    trash: RecordingTrash,
}

impl Fixture {
    fn new() -> Self {
        let f = Self {
            tmp: tempfile::tempdir().expect("tempdir"),
            trash: RecordingTrash::default(),
        };
        f.add_mod("install/Data/Core", "ludeon.rimworld");
        f
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.tmp.path().join(rel)
    }

    fn store(&self) -> ProfileStore<&RecordingTrash> {
        ProfileStore::open(self.path("profiles"), self.path("game-save"), &self.trash)
            .expect("store")
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

    /// `<rel>/About/About.xml` with the id, plus one payload file.
    fn add_mod(&self, rel: &str, id: &str) -> PathBuf {
        let dir = self.path(rel);
        std::fs::create_dir_all(dir.join("About")).expect("about");
        std::fs::write(
            dir.join("About/About.xml"),
            format!("<ModMetaData><packageId>{id}</packageId></ModMetaData>"),
        )
        .expect("about");
        std::fs::write(dir.join("payload.txt"), format!("v1 {id}")).expect("payload");
        dir
    }

    /// A profile whose ModsConfig lists `active`.
    fn profile(&self, active: &[&str]) -> ProfileId {
        let p = self.store().create("Run").expect("create");
        self.set_active(&p.id, active);
        p.id
    }

    fn set_active(&self, id: &ProfileId, active: &[&str]) {
        let config = self.profile_dir(id).join(MODS_CONFIG);
        std::fs::create_dir_all(config.parent().expect("parent")).expect("config dir");
        let items: String = active.iter().map(|a| format!("<li>{a}</li>")).collect();
        std::fs::write(
            config,
            format!("<ModsConfigData><activeMods>{items}</activeMods></ModsConfigData>"),
        )
        .expect("config");
    }

    fn profile_dir(&self, id: &ProfileId) -> PathBuf {
        self.path("profiles").join(id.as_str())
    }

    fn plan(&self, id: &ProfileId, refresh: bool) -> OfflinePlan {
        plan_offline(&self.store(), id, &self.game(), refresh).expect("plan")
    }

    fn apply(
        &self,
        plan: &OfflinePlan,
        choices: &[(String, PathBuf)],
    ) -> Result<Profile, ProfileError> {
        apply_offline(&self.store(), plan, choices, &IDLE, &mut |_| {})
    }

    fn file(&self, id: &ProfileId) -> ProfileFile {
        ProfileFile::read(&self.profile_dir(id).join(FILE_NAME)).expect("toml")
    }

    fn mods(&self, id: &ProfileId) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(self.profile_dir(id).join(MODS))
            .map(|r| {
                r.filter_map(Result::ok)
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }
}

/// Every file under `dir` with its bytes, for before/after comparisons.
#[cfg(unix)]
fn snapshot(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).expect("read").filter_map(Result::ok) {
            if e.file_type().expect("type").is_dir() {
                stack.push(e.path());
            } else {
                out.push((e.path(), std::fs::read(e.path()).expect("bytes")));
            }
        }
    }
    out.sort();
    out
}

#[test]
fn offline_copies_active_mods_keeping_folder_names() {
    let f = Fixture::new();
    f.add_mod("install/Mods/MyMod", "me.mymod");
    let ws = f.add_mod("ws/2009463077", "brrainz.harmony");
    f.add_mod("ws/111", "me.inactive");
    let id = f.profile(&["brrainz.harmony", "ludeon.rimworld", "me.mymod"]);
    let plan = f.plan(&id, false);
    assert!(plan.conflicts.is_empty() && plan.missing.is_empty() && !plan.reuse);
    let p = f.apply(&plan, &[]).expect("apply");
    assert_eq!(p.mode, Mode::Offline);
    // V12: source folder names; official Core and inactive mods not copied.
    assert_eq!(f.mods(&id), ["2009463077", "MyMod"]);
    let copied = f.profile_dir(&id).join("Mods/2009463077/payload.txt");
    assert_eq!(
        std::fs::read_to_string(copied).expect("copy"),
        "v1 brrainz.harmony"
    );
    let frozen = f.file(&id).frozen_mods;
    assert_eq!(frozen.len(), 2);
    assert_eq!(frozen[0].package_id, "brrainz.harmony");
    assert_eq!(frozen[0].source, ModSource::Workshop);
    assert_eq!(frozen[0].source_dir, ws);
    assert_eq!(frozen[1].source, ModSource::Local);
}

#[cfg(unix)]
#[test]
fn offline_never_writes_sources() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    f.add_mod("install/Mods/MyMod", "me.mymod");
    f.add_mod("ws/2009463077", "brrainz.harmony");
    let id = f.profile(&["brrainz.harmony", "me.mymod"]);
    let (install, ws) = (snapshot(&f.path("install")), snapshot(&f.path("ws")));
    let ro = |mode| {
        for d in ["install", "install/Mods", "ws"] {
            std::fs::set_permissions(f.path(d), std::fs::Permissions::from_mode(mode))
                .expect("chmod");
        }
    };
    ro(0o555);
    let result = f.apply(&f.plan(&id, false), &[]);
    ro(0o755);
    result.expect("apply with read-only sources");
    // V6
    assert_eq!(snapshot(&f.path("install")), install);
    assert_eq!(snapshot(&f.path("ws")), ws);
}

#[test]
fn steam_suffix_resolves_workshop() {
    let f = Fixture::new();
    f.add_mod("install/Mods/Harmony", "brrainz.harmony");
    f.add_mod("ws/2009463077", "brrainz.harmony");
    let id = f.profile(&["brrainz.harmony_steam"]);
    let plan = f.plan(&id, false);
    assert_eq!(plan.copies.len(), 1);
    assert_eq!(plan.copies[0].folder, "2009463077");
    assert_eq!(plan.copies[0].package_id, "brrainz.harmony");
}

#[test]
fn both_ids_active_is_conflict_until_chosen() {
    let f = Fixture::new();
    let local = f.add_mod("install/Mods/Harmony", "brrainz.harmony");
    f.add_mod("ws/2009463077", "brrainz.harmony");
    let id = f.profile(&["brrainz.harmony", "brrainz.harmony_steam"]);
    let plan = f.plan(&id, false);
    assert!(plan.copies.is_empty());
    assert_eq!(plan.conflicts.len(), 1);
    assert_eq!(plan.conflicts[0].options.len(), 2);
    assert!(
        matches!(f.apply(&plan, &[]), Err(ProfileError::Unresolved(ids)) if ids == ["brrainz.harmony"])
    );
    assert_eq!(f.file(&id).mode, Mode::Live, "nothing changed");
    // V20: never both.
    f.apply(&plan, &[("brrainz.harmony".into(), local)])
        .expect("apply");
    assert_eq!(f.mods(&id), ["Harmony"]);
}

#[test]
fn ambiguous_same_source_is_conflict() {
    let f = Fixture::new();
    f.add_mod("install/Mods/Mod", "me.mod");
    let old = f.add_mod("install/Mods/Mod - old", "me.mod");
    let id = f.profile(&["me.mod"]);
    let plan = f.plan(&id, false);
    assert_eq!(plan.conflicts.len(), 1);
    f.apply(&plan, &[("me.mod".into(), old)]).expect("apply");
    assert_eq!(f.mods(&id), ["Mod - old"]);
}

#[test]
fn missing_active_mod_reported_not_fatal() {
    let f = Fixture::new();
    f.add_mod("ws/2009463077", "brrainz.harmony");
    let id = f.profile(&["brrainz.harmony", "me.gone"]);
    let plan = f.plan(&id, false);
    assert_eq!(plan.missing, ["me.gone"]);
    f.apply(&plan, &[]).expect("apply");
    assert_eq!(f.mods(&id), ["2009463077"]);
}

/// V36: a new profile with no mod list goes OFFLINE with nothing frozen.
#[test]
fn no_modsconfig_plans_nothing_and_goes_offline() {
    let f = Fixture::new();
    let id = f.store().create("Fresh").expect("create").id;
    let plan = plan_offline(&f.store(), &id, &f.game(), false).expect("plan");
    assert!(plan.copies.is_empty() && plan.conflicts.is_empty() && plan.missing.is_empty());
    f.apply(&plan, &[]).expect("apply");
    assert_eq!(f.store().get(&id).expect("get").mode, Mode::Offline);
}

#[test]
fn default_cannot_go_offline() {
    let f = Fixture::new();
    assert!(matches!(
        plan_offline(&f.store(), &ProfileId::default_id(), &f.game(), false),
        Err(ProfileError::DefaultImmutable(_))
    ));
}

#[test]
fn folder_clash_is_error() {
    let f = Fixture::new();
    f.add_mod("install/Mods/2009463077", "me.sneaky");
    f.add_mod("ws/2009463077", "brrainz.harmony");
    let id = f.profile(&["me.sneaky", "brrainz.harmony"]);
    let plan = f.plan(&id, false);
    assert!(matches!(
        f.apply(&plan, &[]),
        Err(ProfileError::FolderClash(_))
    ));
    assert!(!f.profile_dir(&id).join(STAGING).exists());
}

#[test]
fn plan_reports_bytes_and_free_space() {
    let f = Fixture::new();
    let ws = f.add_mod("ws/2009463077", "brrainz.harmony");
    let id = f.profile(&["brrainz.harmony"]);
    let plan = f.plan(&id, false);
    assert_eq!(plan.bytes_needed, space::dir_size(&ws));
    assert!(plan.bytes_needed > 0);
    assert!(plan.free_bytes.is_some_and(|b| b > 0));
}

#[test]
fn refresh_recopies_and_keeps_gone_sources() {
    let f = Fixture::new();
    let harmony = f.add_mod("ws/2009463077", "brrainz.harmony");
    let gone = f.add_mod("ws/222", "me.unsubscribed");
    let id = f.profile(&["brrainz.harmony", "me.unsubscribed"]);
    f.apply(&f.plan(&id, false), &[]).expect("offline");
    std::fs::write(harmony.join("payload.txt"), "v2").expect("update");
    std::fs::remove_dir_all(&gone).expect("unsubscribe");
    let plan = f.plan(&id, true);
    assert_eq!(plan.kept.len(), 1);
    assert!(plan.missing.is_empty());
    f.apply(&plan, &[]).expect("refresh");
    let mods = f.profile_dir(&id).join(MODS);
    assert_eq!(
        std::fs::read_to_string(mods.join("2009463077/payload.txt")).expect("r"),
        "v2"
    );
    assert_eq!(
        std::fs::read_to_string(mods.join("222/payload.txt")).expect("r"),
        "v1 me.unsubscribed"
    );
    assert_eq!(f.file(&id).frozen_mods.len(), 2);
    assert_eq!(*f.trash.0.borrow(), [f.profile_dir(&id).join(OLD)]);
}

#[test]
fn refresh_keeps_hand_added_folders() {
    let f = Fixture::new();
    f.add_mod("ws/2009463077", "brrainz.harmony");
    let id = f.profile(&["brrainz.harmony"]);
    f.apply(&f.plan(&id, false), &[]).expect("offline");
    let manual = f.profile_dir(&id).join("Mods/MyDevMod");
    std::fs::create_dir_all(&manual).expect("manual");
    std::fs::write(manual.join("x"), "mine").expect("w");
    f.apply(&f.plan(&id, true), &[]).expect("refresh");
    assert_eq!(f.mods(&id), ["2009463077", "MyDevMod"]);
    assert_eq!(
        std::fs::read_to_string(manual.join("x")).expect("r"),
        "mine"
    );
}

#[cfg(unix)]
#[test]
fn failure_leaves_old_mods_and_toml_untouched() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    let harmony = f.add_mod("ws/2009463077", "brrainz.harmony");
    let id = f.profile(&["brrainz.harmony"]);
    f.apply(&f.plan(&id, false), &[]).expect("offline");
    let (mods, toml) = (snapshot(&f.profile_dir(&id).join(MODS)), f.file(&id));
    // An unreadable source file: the scan still sees the mod, the copy fails.
    let locked = harmony.join("payload.txt");
    let plan = f.plan(&id, true);
    assert_eq!(plan.copies.len(), 1, "mod still planned for copy");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).expect("chmod");
    if std::fs::read(&locked).is_ok() {
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o644)).expect("chmod");
        return; // running as root: permissions not enforced, nothing to prove
    }
    let result = f.apply(&plan, &[]);
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o644)).expect("chmod");
    assert!(matches!(result, Err(ProfileError::Io { .. })), "{result:?}");
    assert_eq!(snapshot(&f.profile_dir(&id).join(MODS)), mods);
    assert_eq!(f.file(&id), toml);
    assert!(!f.profile_dir(&id).join(STAGING).exists());
}

#[test]
fn refuses_while_any_game_runs() {
    let f = Fixture::new();
    f.add_mod("ws/2009463077", "brrainz.harmony");
    let id = f.profile(&["brrainz.harmony"]);
    let plan = f.plan(&id, false);
    let busy = FakeProbe(true);
    assert!(matches!(
        apply_offline(&f.store(), &plan, &[], &busy, &mut |_| {}),
        Err(ProfileError::GameRunning)
    ));
    assert!(matches!(
        go_live(&f.store(), &id, &busy),
        Err(ProfileError::GameRunning)
    ));
    assert!(f.mods(&id).is_empty());
}

#[test]
fn go_live_keeps_frozen_copies() {
    let f = Fixture::new();
    f.add_mod("ws/2009463077", "brrainz.harmony");
    let id = f.profile(&["brrainz.harmony"]);
    f.apply(&f.plan(&id, false), &[]).expect("offline");
    let p = go_live(&f.store(), &id, &IDLE).expect("live");
    assert_eq!(p.mode, Mode::Live);
    assert_eq!(f.mods(&id), ["2009463077"]);
    assert_eq!(f.file(&id).frozen_mods.len(), 1);
}

#[test]
fn offline_again_reuses_copies() {
    let f = Fixture::new();
    let harmony = f.add_mod("ws/2009463077", "brrainz.harmony");
    let id = f.profile(&["brrainz.harmony"]);
    f.apply(&f.plan(&id, false), &[]).expect("offline");
    go_live(&f.store(), &id, &IDLE).expect("live");
    std::fs::write(harmony.join("payload.txt"), "v2").expect("update");
    let plan = f.plan(&id, false);
    assert!(plan.reuse && plan.copies.is_empty());
    assert_eq!(f.apply(&plan, &[]).expect("offline").mode, Mode::Offline);
    let copy = f.profile_dir(&id).join("Mods/2009463077/payload.txt");
    assert_eq!(
        std::fs::read_to_string(copy).expect("r"),
        "v1 brrainz.harmony",
        "frozen copy kept"
    );
}

#[test]
fn source_changed_during_copy_retries_then_fails() {
    let f = Fixture::new();
    let harmony = f.add_mod("ws/2009463077", "brrainz.harmony");
    let id = f.profile(&["brrainz.harmony"]);
    let plan = f.plan(&id, false);
    let store = f.store();
    // Changes once: the retry succeeds with the new bytes.
    let mut calls = 0;
    let hooks = Hooks {
        progress: &mut |_| {},
        after_copy: &mut |src| {
            calls += 1;
            if calls == 1 {
                std::fs::write(src.join("payload.txt"), "v2 longer").expect("update");
            }
        },
    };
    apply_with_hook(&store, &plan, &[], &IDLE, hooks).expect("retry succeeds");
    assert_eq!(calls, 2);
    let copy = f.profile_dir(&id).join("Mods/2009463077/payload.txt");
    assert_eq!(std::fs::read_to_string(&copy).expect("r"), "v2 longer");
    // Keeps changing: refuses and leaves the profile as it was.
    let before = f.file(&id);
    let mut n = 0u8;
    let hooks = Hooks {
        progress: &mut |_| {},
        after_copy: &mut |src| {
            n += 1;
            std::fs::write(src.join("payload.txt"), vec![b'x'; usize::from(n) * 100])
                .expect("update");
        },
    };
    let result = apply_with_hook(&store, &f.plan(&id, true), &[], &IDLE, hooks);
    assert!(matches!(result, Err(ProfileError::SourceChanging(ref d)) if d == "2009463077"));
    assert_eq!(f.file(&id), before);
    assert!(!f.profile_dir(&id).join(STAGING).exists());
    assert!(harmony.exists());
}

#[test]
fn copied_at_formats_utc() {
    assert_eq!(utc_datetime(0).to_string(), "1970-01-01T00:00:00Z");
    assert_eq!(
        utc_datetime(951_782_400).to_string(),
        "2000-02-29T00:00:00Z"
    );
    assert_eq!(
        utc_datetime(1_791_745_445).to_string(),
        "2026-10-11T19:04:05Z"
    );
}

/// Manual smoke against this machine's install, workshop and Default mod list, into a
/// temp store: `cargo test -p rwpm-core real_install_offline -- --ignored --nocapture`.
#[test]
#[ignore = "reads the real RimWorld install and copies real mods into a temp dir"]
fn real_install_offline_round_trip() {
    let game = crate::discovery::discover(None).expect("discover");
    let f = Fixture::new();
    let id = f.store().create("Real").expect("create").id;
    let config = f.profile_dir(&id).join(MODS_CONFIG);
    std::fs::create_dir_all(config.parent().expect("parent")).expect("dir");
    let real = crate::paths::default_game_save_dir()
        .expect("save dir")
        .join("Config/ModsConfig.xml");
    std::fs::copy(real, &config).expect("copy ModsConfig");
    let plan = plan_offline(&f.store(), &id, &game, false).expect("plan");
    println!(
        "copies={} conflicts={} missing={:?} bytes={} free={:?}",
        plan.copies.len(),
        plan.conflicts.len(),
        plan.missing,
        plan.bytes_needed,
        plan.free_bytes
    );
    let p = apply_offline(&f.store(), &plan, &[], &IDLE, &mut |_| {}).expect("apply");
    let copied = space::dir_size(&f.profile_dir(&id).join(MODS));
    println!(
        "mode={:?} frozen={} copied_bytes={copied}",
        p.mode,
        f.file(&id).frozen_mods.len()
    );
    assert_eq!(copied, plan.bytes_needed);
}
