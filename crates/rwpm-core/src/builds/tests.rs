use std::cell::RefCell;
use std::fs::File;
use std::path::{Path, PathBuf};

use super::capture::{CaptureKind, UpdateReason, apply_for_test, plan_capture};
use super::*;
use crate::discovery::{GAME_EXE, GameInstall, RIMWORLD_APP_ID, discover};
use crate::profile::lock::RUN_LOCK;
use crate::profile::{ProfileFile, ProfileId, RunningProbe};

#[derive(Default)]
struct RecordingTrash(RefCell<Vec<PathBuf>>);

impl Trash for &RecordingTrash {
    fn trash(&self, path: &Path) -> std::io::Result<()> {
        self.0.borrow_mut().push(path.to_path_buf());
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

struct Fixture {
    tmp: tempfile::TempDir,
    trash: RecordingTrash,
}

/// A Steam library holding a RimWorld install, plus an empty profiles root.
impl Fixture {
    fn new(buildid: &str, state: &str, dlc: &[&str]) -> Self {
        let f = Self {
            tmp: tempfile::tempdir().expect("tempdir"),
            trash: RecordingTrash::default(),
        };
        let steamapps = f.steam().join("steamapps");
        std::fs::create_dir_all(&steamapps).expect("steamapps");
        std::fs::write(
            steamapps.join("libraryfolders.vdf"),
            format!(
                "\"libraryfolders\"\n{{\n\t\"0\"\n\t{{\n\t\t\"path\"\t\t\"{}\"\n\t\t\"apps\"\n\t\t{{\n\t\t}}\n\t}}\n}}\n",
                f.steam().display().to_string().replace('\\', "\\\\")
            ),
        )
        .expect("vdf");
        f.write_manifest(buildid, state, "");
        let game = f.game_dir();
        std::fs::create_dir_all(&game).expect("game");
        // macOS: GAME_EXE is the bundle DIRECTORY (R29), and it only counts as the game
        // when it holds its executable — a bare dir is the husk Steam leaves behind.
        if cfg!(target_os = "macos") {
            let bundle = game.join(GAME_EXE);
            std::fs::create_dir_all(bundle.join("Contents/MacOS")).expect("bundle");
            std::fs::write(
                bundle.join("Contents/Info.plist"),
                "<plist><dict><key>CFBundleExecutable</key><string>RimWorld</string></dict></plist>",
            )
            .expect("plist");
            std::fs::write(bundle.join("Contents/MacOS/RimWorld"), b"game").expect("binary");
        } else {
            std::fs::write(game.join(GAME_EXE), b"game").expect("exe");
        }
        f.write_version("1.6.4871 rev598");
        f.write_dlc(dlc);
        f.write_install_file(&f.install().mods_dir().join("Place mods here.txt"), "x");
        f
    }

    fn steam(&self) -> PathBuf {
        self.tmp.path().join("steam")
    }

    fn app(&self) -> PathBuf {
        self.tmp.path().join("app")
    }

    fn profiles_root(&self) -> PathBuf {
        self.tmp.path().join("profiles")
    }

    fn game_dir(&self) -> PathBuf {
        self.steam().join("steamapps/common/RimWorld")
    }

    fn manifest(&self) -> PathBuf {
        self.steam()
            .join("steamapps")
            .join(format!("appmanifest_{RIMWORLD_APP_ID}.acf"))
    }

    fn write_manifest(&self, buildid: &str, state: &str, beta: &str) {
        std::fs::write(
            self.manifest(),
            format!(
                "\"AppState\"\n{{\n\t\"appid\"\t\t\"{RIMWORLD_APP_ID}\"\n\t\"name\"\t\t\"RimWorld\"\n\
                 \t\"installdir\"\t\t\"RimWorld\"\n\t\"StateFlags\"\t\t\"{state}\"\n\
                 \t\"buildid\"\t\t\"{buildid}\"\n\t\"MountedConfig\"\n\t{{\n\
                 \t\t\"BetaKey\"\t\t\"{beta}\"\n\t}}\n}}\n"
            ),
        )
        .expect("manifest");
    }

    fn write_version(&self, version: &str) {
        let path = self.install_unchecked().version_file();
        self.write_install_file(&path, &format!("{version}\r\n"));
    }

    fn write_dlc(&self, dlc: &[&str]) {
        let data = self.install_unchecked().data_dir();
        let _ = std::fs::remove_dir_all(&data);
        for name in dlc {
            std::fs::create_dir_all(data.join(name)).expect("dlc");
            std::fs::write(data.join(name).join("About.xml"), vec![1u8; 2048]).expect("about");
        }
        // T52: macOS keeps a plain file in `Data/` that is not an expansion.
        std::fs::write(data.join("Place official expansions here.txt"), "x").expect("note");
    }

    fn write_install_file(&self, path: &Path, body: &str) {
        std::fs::create_dir_all(path.parent().expect("parent")).expect("dirs");
        std::fs::write(path, body).expect("write");
    }

    /// Before `Version.txt` exists `discover` still resolves the layout.
    fn install_unchecked(&self) -> GameInstall {
        discover(Some(&self.steam())).expect("discover")
    }

    fn install(&self) -> GameInstall {
        self.install_unchecked()
    }

    fn builds(&self) -> BuildStore {
        BuildStore::open(&self.profiles_root(), &self.app()).expect("store")
    }

    fn profiles(&self) -> ProfileStore<&RecordingTrash> {
        ProfileStore::open(
            self.profiles_root(),
            self.tmp.path().join("game-saves"),
            &self.trash,
        )
        .expect("profiles")
    }

    fn plan(&self) -> Result<crate::builds::capture::CapturePlan, ProfileError> {
        plan_capture(&self.builds(), &self.install())
    }

    fn capture(&self) -> Result<Option<Build>, ProfileError> {
        let plan = self.plan()?;
        super::capture::apply_capture(&self.builds(), &plan, &FakeProbe(false), &mut |_| {})
    }

    /// Captures, failing the run just after the copy (stands in for a crash).
    fn capture_failing(&self) -> Result<Option<Build>, ProfileError> {
        let plan = self.plan()?;
        apply_for_test(
            &self.builds(),
            &plan,
            &FakeProbe(false),
            &mut |_| {},
            &mut |_| Err(ProfileError::EmptyName),
        )
    }

    fn pin(&self, name: &str, key: &str) -> ProfileId {
        let p = self.profiles().create(name).expect("create");
        self.profiles().set_pin(&p.id, Some(key)).expect("pin");
        p.id
    }
}

fn key(s: &str) -> BuildKey {
    BuildKey::parse(s).expect("key")
}

const KEY: &str = "1.6.4871-rev598";

#[test]
fn key_is_a_slug_of_the_version_text() {
    assert_eq!(
        BuildKey::from_version("1.6.4871 rev598")
            .expect("key")
            .as_str(),
        KEY
    );
    // CRLF from the real file, trimmed by the caller or here.
    assert_eq!(
        BuildKey::from_version(" 1.7.4976 rev1043\r\n")
            .expect("key")
            .as_str(),
        "1.7.4976-rev1043"
    );
    assert!(BuildKey::from_version("   ").is_err());
}

#[test]
fn capture_stores_the_build_with_its_metadata() {
    let f = Fixture::new("19234567", "4", &["Core", "Royalty"]);
    let build = f.capture().expect("capture").expect("stored");
    assert_eq!(build.key.as_str(), KEY);
    assert_eq!(build.file.version, "1.6.4871 rev598");
    assert_eq!(build.file.buildid, "19234567");
    assert_eq!(build.file.beta, "");
    // §I.file: folder names only, sorted. The stray text file is not an expansion.
    assert_eq!(build.file.dlc, ["Core", "Royalty"]);
    assert_eq!(build.file.source_dir, f.game_dir());
    assert!(build.file.size > 0);
    assert!(build.dir.join(GAME_EXE).exists(), "no game in the build");
}

/// V58 / R36: 1030 (mid-update) and 36 (pre-launch) both carry bit 4.
#[test]
fn capture_refused_unless_state_flags_are_exactly_four() {
    for state in ["0", "6", "36", "1030", "516"] {
        let f = Fixture::new("19234567", state, &["Core"]);
        assert!(
            matches!(f.plan(), Err(ProfileError::SourceChanging(_))),
            "StateFlags={state} was accepted"
        );
        assert!(!f.builds().root().exists(), "StateFlags={state} wrote");
    }
    let f = Fixture::new("19234567", "4", &["Core"]);
    assert!(f.plan().is_ok());
}

/// V58: Steam can start an update between the plan and the copy; the lock does not stop it.
#[test]
fn capture_refused_when_steam_starts_updating_after_the_plan() {
    let f = Fixture::new("19234567", "4", &["Core"]);
    let plan = f.plan().expect("plan");
    f.write_manifest("19234567", "1030", "");
    let out = super::capture::apply_capture(&f.builds(), &plan, &FakeProbe(false), &mut |_| {});
    assert!(
        matches!(out, Err(ProfileError::SourceChanging(_))),
        "{out:?}"
    );
    assert!(!f.builds().dir_of(&plan.key).exists());
}

/// V59: `run.lock` held → `GameRunning`, for both capture and delete.
#[test]
fn capture_refused_while_the_run_lock_is_held() {
    let f = Fixture::new("19234567", "4", &["Core"]);
    let plan = f.plan().expect("plan");
    std::fs::create_dir_all(f.app()).expect("app");
    let held = File::create(f.app().join(RUN_LOCK)).expect("lock file");
    held.lock().expect("lock");
    let out = super::capture::apply_capture(&f.builds(), &plan, &FakeProbe(false), &mut |_| {});
    assert!(matches!(out, Err(ProfileError::GameRunning)), "{out:?}");
    assert!(!f.builds().dir_of(&plan.key).exists());
}

#[test]
fn capture_refused_while_a_game_is_running() {
    let f = Fixture::new("19234567", "4", &["Core"]);
    let plan = f.plan().expect("plan");
    let out = super::capture::apply_capture(&f.builds(), &plan, &FakeProbe(true), &mut |_| {});
    assert!(matches!(out, Err(ProfileError::GameRunning)), "{out:?}");
}

#[test]
fn delete_refused_while_the_run_lock_is_held() {
    let f = Fixture::new("19234567", "4", &["Core"]);
    f.capture().expect("capture");
    std::fs::create_dir_all(f.app()).expect("app");
    let held = File::create(f.app().join(RUN_LOCK)).expect("lock file");
    held.lock().expect("lock");
    let out = f
        .builds()
        .delete(&f.profiles(), &key(KEY), &FakeProbe(false));
    assert!(matches!(out, Err(ProfileError::GameRunning)), "{out:?}");
    assert!(f.builds().dir_of(&key(KEY)).exists(), "deleted anyway");
}

/// V54: Delete is refused while a profile pins the build, and says which.
#[test]
fn delete_refused_while_a_profile_pins_the_build() {
    let f = Fixture::new("19234567", "4", &["Core"]);
    f.capture().expect("capture");
    f.pin("My Modded Run", KEY);
    f.pin("Other Run", KEY);
    f.profiles().create("Unpinned").expect("create");

    let out = f
        .builds()
        .delete(&f.profiles(), &key(KEY), &FakeProbe(false));
    let Err(ProfileError::BuildInUse { key: k, profiles }) = out else {
        panic!("delete was allowed: {out:?}");
    };
    assert_eq!(k, KEY);
    assert_eq!(profiles, ["My Modded Run", "Other Run"]);
    assert!(f.builds().dir_of(&key(KEY)).exists());
    assert!(
        f.builds()
            .delete(&f.profiles(), &key("9.9"), &FakeProbe(false))
            .is_err()
    );
}

#[test]
fn delete_removes_an_unpinned_build() {
    let f = Fixture::new("19234567", "4", &["Core"]);
    f.capture().expect("capture");
    f.builds()
        .delete(&f.profiles(), &key(KEY), &FakeProbe(false))
        .expect("delete");
    assert!(!f.builds().dir_of(&key(KEY)).exists());
    assert!(f.builds().list().expect("list").builds.is_empty());
}

/// V54: Default has no `profile.toml` and shares the game's save folder.
#[test]
fn the_default_profile_cannot_be_pinned() {
    let f = Fixture::new("19234567", "4", &["Core"]);
    let out = f.profiles().set_pin(&ProfileId::default_id(), Some(KEY));
    assert!(
        matches!(out, Err(ProfileError::DefaultImmutable(_))),
        "{out:?}"
    );
    assert!(
        f.profiles()
            .list()
            .expect("list")
            .profiles
            .iter()
            .all(|p| p.game_build.is_none())
    );
}

/// V63 / B8: `Version.txt` lags the binary, so the same text with a new `buildid` is an
/// update offer, never a silent no-op.
#[test]
fn the_same_version_with_a_new_buildid_is_an_update() {
    let f = Fixture::new("19234567", "4", &["Core"]);
    f.capture().expect("capture");
    f.write_manifest("19300000", "4", "");

    let plan = f.plan().expect("plan");
    assert_eq!(plan.key.as_str(), KEY, "key changed");
    let CaptureKind::Update { reason, old } = &plan.kind else {
        panic!("not offered as an update: {:?}", plan.kind);
    };
    assert_eq!(*reason, UpdateReason::NewBuildId);
    assert_eq!(old.buildid, "19234567");

    let build = f.capture().expect("capture").expect("stored");
    assert_eq!(build.file.buildid, "19300000");
    assert_eq!(f.builds().list().expect("list").builds.len(), 1);
}

#[test]
fn the_same_version_with_different_dlc_is_an_update() {
    let f = Fixture::new("19234567", "4", &["Core"]);
    f.capture().expect("capture");
    f.write_dlc(&["Core", "Odyssey"]);

    let plan = f.plan().expect("plan");
    assert!(
        matches!(
            plan.kind,
            CaptureKind::Update {
                reason: UpdateReason::DlcChanged,
                ..
            }
        ),
        "{:?}",
        plan.kind
    );
    let build = f.capture().expect("capture").expect("stored");
    assert_eq!(build.file.dlc, ["Core", "Odyssey"]);
}

/// The key is the version only, so a branch switch that keeps the version text lands on
/// the same key. It is offered as an update, with the reason named.
#[test]
fn the_same_version_on_another_branch_is_an_update() {
    let f = Fixture::new("19234567", "4", &["Core"]);
    f.capture().expect("capture");
    f.write_manifest("19234567", "4", "unstable");

    let plan = f.plan().expect("plan");
    assert_eq!(plan.beta, "unstable");
    assert!(
        matches!(
            plan.kind,
            CaptureKind::Update {
                reason: UpdateReason::BetaChanged,
                ..
            }
        ),
        "{:?}",
        plan.kind
    );
}

/// V53: capture of a build already stored does nothing, even under another key.
#[test]
fn an_already_stored_buildid_is_a_no_op() {
    let f = Fixture::new("19234567", "4", &["Core"]);
    f.capture().expect("capture");
    let before = crate::space::dir_size(f.builds().root());

    assert!(f.plan().expect("plan").is_no_op());
    assert!(f.capture().expect("capture").is_none(), "copied again");

    // Same build, new version text: a different key, still nothing to do.
    f.write_version("1.6.4872 rev599");
    let plan = f.plan().expect("plan");
    assert_eq!(plan.key.as_str(), "1.6.4872-rev599");
    let CaptureKind::NoOp { stored } = &plan.kind else {
        panic!("re-copied an identical build: {:?}", plan.kind);
    };
    assert_eq!(stored.as_str(), KEY);
    assert!(f.capture().expect("capture").is_none());
    assert_eq!(crate::space::dir_size(f.builds().root()), before);
    assert_eq!(f.builds().list().expect("list").builds.len(), 1);
}

/// V53: a second profile pinning a stored build costs no disk.
#[test]
fn a_second_profile_pinning_a_build_copies_nothing() {
    let f = Fixture::new("19234567", "4", &["Core"]);
    f.capture().expect("capture");
    let before = crate::space::dir_size(f.builds().root());
    f.pin("First", KEY);
    f.pin("Second", KEY);
    assert_eq!(crate::space::dir_size(f.builds().root()), before);
    assert_eq!(f.builds().list().expect("list").builds.len(), 1);
}

/// V60: `<build>/Mods/` comes from the OLD build, not the Steam install.
#[test]
fn an_update_carries_mods_from_the_old_build() {
    let f = Fixture::new("19234567", "4", &["Core"]);
    let build = f.capture().expect("capture").expect("stored");
    let mods_rel = f
        .install()
        .mods_dir()
        .strip_prefix(f.game_dir())
        .expect("rel")
        .to_path_buf();
    // A local mod the player put in the build; V54 allows writes to `<build>/Mods/`.
    let mine = build.dir.join(&mods_rel).join("MyLocal/About/About.xml");
    std::fs::create_dir_all(mine.parent().expect("parent")).expect("dirs");
    std::fs::write(&mine, "mine").expect("mod");
    // A mod only the Steam install has; it must not come across.
    f.write_install_file(
        &f.install().mods_dir().join("FromSteam/About/About.xml"),
        "steam",
    );

    f.write_manifest("19300000", "4", "");
    let updated = f.capture().expect("update").expect("stored");
    let mods = updated.dir.join(&mods_rel);
    assert_eq!(
        std::fs::read_to_string(mods.join("MyLocal/About/About.xml")).expect("carried"),
        "mine"
    );
    assert!(
        !mods.join("FromSteam").exists(),
        "the Steam install's Mods folder was kept"
    );
}

/// The old build may have no `Mods/` at all; so may the install.
#[test]
fn an_update_handles_a_missing_mods_folder_on_either_side() {
    let f = Fixture::new("19234567", "4", &["Core"]);
    let build = f.capture().expect("capture").expect("stored");
    let mods_rel = f
        .install()
        .mods_dir()
        .strip_prefix(f.game_dir())
        .expect("rel")
        .to_path_buf();
    std::fs::remove_dir_all(build.dir.join(&mods_rel)).expect("drop build mods");
    std::fs::remove_dir_all(f.install().mods_dir()).expect("drop install mods");

    f.write_manifest("19300000", "4", "");
    let updated = f.capture().expect("update").expect("stored");
    assert!(!updated.dir.join(&mods_rel).exists());
    assert_eq!(updated.file.buildid, "19300000");
}

/// V60: a failed update leaves the old build whole and launchable.
#[test]
fn a_failed_update_leaves_the_old_build_intact() {
    let f = Fixture::new("19234567", "4", &["Core"]);
    let old = f.capture().expect("capture").expect("stored");
    let before = crate::space::dir_size(&old.dir);
    f.write_manifest("19300000", "4", "");

    assert!(f.capture_failing().is_err(), "the failure did not surface");

    let still = f.builds().get(&key(KEY)).expect("old build gone");
    assert_eq!(still.file, old.file, "build.toml changed");
    assert_eq!(crate::space::dir_size(&still.dir), before);
    assert!(still.dir.join(GAME_EXE).exists(), "not launchable");
    assert!(
        std::fs::read_dir(f.builds().root())
            .expect("list")
            .filter_map(Result::ok)
            .all(|e| e.file_name() == KEY),
        "staging left behind"
    );
}

/// The same guarantee for a first capture: nothing half-copied is ever visible.
#[test]
fn a_failed_capture_leaves_no_build_behind() {
    let f = Fixture::new("19234567", "4", &["Core"]);
    assert!(f.capture_failing().is_err());
    assert!(!f.builds().dir_of(&key(KEY)).exists());
    assert!(f.builds().list().expect("list").builds.is_empty());
}

/// V60: the copy is verified, so a clone that lost a block never becomes a build.
#[test]
fn a_corrupted_copy_fails_the_check() {
    let f = Fixture::new("19234567", "4", &["Core"]);
    let plan = f.plan().expect("plan");
    let out = apply_for_test(
        &f.builds(),
        &plan,
        &FakeProbe(false),
        &mut |_| {},
        &mut |staging| {
            let data = staging.join(
                f.install()
                    .data_dir()
                    .strip_prefix(f.game_dir())
                    .expect("rel"),
            );
            std::fs::write(data.join("Core/About.xml"), b"short").expect("corrupt");
            Ok(())
        },
    );
    assert!(matches!(out, Err(ProfileError::VerifyFailed(_))), "{out:?}");
    assert!(!f.builds().dir_of(&key(KEY)).exists());
}

/// V60: a crash between the two renames leaves `.replaced-<key>`; the old build comes back.
#[test]
fn an_interrupted_swap_restores_the_old_build() {
    let f = Fixture::new("19234567", "4", &["Core"]);
    let old = f.capture().expect("capture").expect("stored");
    let live = f.builds().dir_of(&key(KEY));
    let replaced = f.builds().root().join(format!(".replaced-{KEY}"));
    std::fs::rename(&live, &replaced).expect("half a swap");
    std::fs::create_dir_all(f.builds().root().join(format!(".staging-{KEY}"))).expect("staging");

    let listing = f.builds().list().expect("list");
    assert_eq!(listing.builds.len(), 1);
    assert_eq!(listing.builds[0].file, old.file);
    assert!(
        live.join(GAME_EXE).exists(),
        "not launchable after recovery"
    );
    assert!(!replaced.exists() && !f.builds().root().join(format!(".staging-{KEY}")).exists());
}

/// A finished swap whose clean-up was cut short just drops the leftover.
#[test]
fn a_leftover_from_a_finished_swap_is_dropped() {
    let f = Fixture::new("19234567", "4", &["Core"]);
    f.capture().expect("capture");
    let replaced = f.builds().root().join(format!(".replaced-{KEY}"));
    std::fs::create_dir_all(replaced.join("Data")).expect("leftover");

    assert_eq!(f.builds().list().expect("list").builds.len(), 1);
    assert!(!replaced.exists());
}

/// Housekeeping must never delete a capture that another process is running (its lock).
#[test]
fn recovery_steps_aside_while_the_lock_is_held() {
    let f = Fixture::new("19234567", "4", &["Core"]);
    f.capture().expect("capture");
    let staging = f.builds().root().join(format!(".staging-{KEY}"));
    std::fs::create_dir_all(&staging).expect("staging");
    std::fs::create_dir_all(f.app()).expect("app");
    let held = File::create(f.app().join(RUN_LOCK)).expect("lock file");
    held.lock().expect("lock");

    assert_eq!(f.builds().list().expect("list").builds.len(), 1);
    assert!(
        staging.exists(),
        "a live capture's staging folder was deleted"
    );
}

/// V6 / V54: the Steam install is read-only to rwpm, and the build drops Steam's own files.
#[test]
fn capture_strips_steam_files_and_leaves_the_install_alone() {
    let f = Fixture::new("19234567", "4", &["Core"]);
    let install = f.install();
    // Steam writes this at the install root, beside the bundle, and inside it (T52).
    for dir in [install.game_dir.clone(), install.mods_dir()] {
        f.write_install_file(&dir.join("steam_appid.txt"), "294100");
    }
    f.write_install_file(&install.game_dir.join("mono_crash.1.json"), "{}");
    let before = tree(&install.game_dir);

    let build = f.capture().expect("capture").expect("stored");
    assert_eq!(
        tree(&install.game_dir),
        before,
        "the install was written to"
    );

    let copied = tree(&build.dir);
    assert!(
        !copied.iter().any(|p| p.ends_with("steam_appid.txt")),
        "steam_appid.txt was copied: {copied:?}"
    );
    assert!(
        !copied.iter().any(|p| p.contains("mono_crash.")),
        "a crash dump was copied: {copied:?}"
    );
    assert!(copied.iter().any(|p| p == "build.toml"));
}

/// C1: `builds` only became reserved in v1.4, so a profile may already sit there.
#[test]
fn the_store_refuses_to_open_over_a_profile_folder() {
    let f = Fixture::new("19234567", "4", &["Core"]);
    let legacy = f.profiles_root().join("builds");
    std::fs::create_dir_all(legacy.join("SaveData/Saves")).expect("dirs");
    std::fs::write(legacy.join("SaveData/Saves/colony.rws"), "save").expect("save");
    ProfileFile::new("Builds")
        .write_atomic(&legacy.join("profile.toml"))
        .expect("toml");

    let out = BuildStore::open(&f.profiles_root(), &f.app());
    assert!(
        matches!(out, Err(ProfileError::BuildsFolderTaken(_))),
        "{:?}",
        out.err()
    );
    // The profile is untouched and still listed, so the player can rename it.
    assert!(legacy.join("SaveData/Saves/colony.rws").is_file());
    assert!(
        f.profiles()
            .list()
            .expect("list")
            .profiles
            .iter()
            .any(|p| p.name == "Builds")
    );
}

/// V58 needs the app manifest, so a non-Steam install (GOG, a manual copy) cannot be
/// captured: there is no `buildid` and no way to tell a finished install from a partial one.
#[test]
fn capture_refused_for_a_non_steam_install() {
    let f = Fixture::new("19234567", "4", &["Core"]);
    let gog = GameInstall {
        steam: None,
        ..f.install()
    };
    let out = plan_capture(&f.builds(), &gog);
    assert!(
        matches!(out, Err(ProfileError::NotSteamInstall(_))),
        "{out:?}"
    );
}

#[test]
fn capture_refused_when_the_app_manifest_has_gone() {
    let f = Fixture::new("19234567", "4", &["Core"]);
    let install = f.install();
    std::fs::remove_file(f.manifest()).expect("drop manifest");
    let out = plan_capture(&f.builds(), &install);
    assert!(matches!(out, Err(ProfileError::Io { .. })), "{out:?}");
}

#[test]
fn capture_refused_when_there_is_no_room() {
    let f = Fixture::new("19234567", "4", &["Core"]);
    let mut plan = f.plan().expect("plan");
    plan.free_bytes = Some(1);
    let out = super::capture::apply_capture(&f.builds(), &plan, &FakeProbe(false), &mut |_| {});
    assert!(
        matches!(out, Err(ProfileError::NotEnoughSpace { .. })),
        "{out:?}"
    );
    assert!(!f.builds().dir_of(&plan.key).exists());
}

#[test]
fn an_update_plan_counts_both_copies_in_its_peak() {
    let f = Fixture::new("19234567", "4", &["Core"]);
    f.capture().expect("capture");
    f.write_manifest("19300000", "4", "");
    let plan = f.plan().expect("plan");
    assert!(
        plan.peak_bytes > plan.bytes,
        "peak {} did not count the old build ({} bytes)",
        plan.peak_bytes,
        plan.bytes
    );
}

#[test]
fn an_unreadable_build_toml_is_listed_separately() {
    let f = Fixture::new("19234567", "4", &["Core"]);
    f.capture().expect("capture");
    std::fs::write(
        f.builds().dir_of(&key(KEY)).join("build.toml"),
        "schema = 99\n",
    )
    .expect("write");
    let listing = f.builds().list().expect("list");
    assert!(listing.builds.is_empty());
    assert_eq!(listing.unreadable.len(), 1);
    assert_eq!(listing.unreadable[0].0.as_str(), KEY);
}

/// Every path under `dir`, relative and sorted.
fn tree(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    walk(dir, Path::new(""), &mut out);
    out.sort();
    out
}

fn walk(root: &Path, rel: &Path, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(root.join(rel)) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let rel = rel.join(entry.file_name());
        out.push(rel.to_string_lossy().into_owned());
        if entry.file_type().is_ok_and(|t| t.is_dir()) {
            walk(root, &rel, out);
        }
    }
}
