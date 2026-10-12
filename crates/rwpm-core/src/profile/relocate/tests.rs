use std::cell::RefCell;

use super::*;
use crate::profile::{Mode, ProfileFile};

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

impl Fixture {
    /// App dir with two profiles (one OFFLINE with a mod and a symlink).
    fn new() -> Self {
        let f = Self {
            tmp: tempfile::tempdir().expect("tempdir"),
            trash: RecordingTrash::default(),
        };
        let s = f.store();
        let a = s.create("Alpha").expect("a");
        let b = s.create("Beta").expect("b");
        let dir = s.root().join(b.id.as_str());
        std::fs::create_dir_all(dir.join("SaveData/Saves")).expect("saves");
        std::fs::write(dir.join("SaveData/Saves/colony.rws"), vec![7u8; 200_000]).expect("save");
        std::fs::create_dir_all(dir.join("Mods/2009463077/About")).expect("mod");
        std::fs::write(dir.join("Mods/2009463077/About/About.xml"), "x").expect("about");
        #[cfg(unix)]
        std::os::unix::fs::symlink("/somewhere", dir.join("Mods/link")).expect("link");
        let mut file = ProfileFile::read(&dir.join("profile.toml")).expect("toml");
        file.mode = Mode::Offline;
        file.write_atomic(&dir.join("profile.toml")).expect("toml");
        let _ = a;
        f
    }

    fn app(&self) -> PathBuf {
        self.tmp.path().join("app")
    }

    fn store(&self) -> ProfileStore<&RecordingTrash> {
        let root = AppSettings::load(&self.app())
            .expect("settings")
            .profiles_root(&self.app());
        ProfileStore::open(root, self.tmp.path().join("game"), &self.trash).expect("store")
    }

    fn target(&self) -> PathBuf {
        self.tmp.path().join("big disk/Profiles")
    }

    fn apply(&self, plan: &MovePlan) -> Result<MoveReport, ProfileError> {
        apply_move(
            &self.store(),
            &self.app(),
            plan,
            &FakeProbe(false),
            &mut |_| {},
        )
    }
}

fn names(store: &ProfileStore<&RecordingTrash>) -> Vec<String> {
    store
        .list()
        .expect("list")
        .profiles
        .into_iter()
        .map(|p| p.name)
        .collect()
}

#[test]
fn move_copies_verifies_then_switches_and_trashes_old() {
    let f = Fixture::new();
    let old = f.store().root().to_path_buf();
    let plan = plan_move(&f.store(), &f.target()).expect("plan");
    assert_eq!(plan.profiles.len(), 2);
    assert!(plan.bytes >= 200_000);
    let report = f.apply(&plan).expect("move");
    assert!(report.left_behind.is_empty());
    let moved = f.store();
    assert_eq!(moved.root(), f.target());
    assert_eq!(names(&moved), ["Default", "Alpha", "Beta"]);
    let beta = f.target().join("beta");
    assert_eq!(
        std::fs::read(beta.join("SaveData/Saves/colony.rws"))
            .expect("save")
            .len(),
        200_000
    );
    #[cfg(unix)]
    assert_eq!(
        std::fs::read_link(beta.join("Mods/link")).expect("link"),
        PathBuf::from("/somewhere")
    );
    assert_eq!(
        f.trash.0.borrow().len(),
        2,
        "old profiles to the recycle bin"
    );
    assert!(!old.exists(), "empty old root removed");
    assert!(!f.target().join(MARKER).exists());
}

#[test]
fn verify_mismatch_keeps_old_and_settings() {
    let f = Fixture::new();
    let plan = plan_move(&f.store(), &f.target()).expect("plan");
    let r = apply_with_hook(
        &f.store(),
        &f.app(),
        &plan,
        &FakeProbe(false),
        &mut |_| {},
        &mut |new| {
            std::fs::write(new.join("beta/SaveData/Saves/colony.rws"), b"torn").expect("corrupt");
        },
    );
    assert!(matches!(r, Err(ProfileError::VerifyFailed(_))), "{r:?}");
    assert_eq!(f.store().root(), plan.old_root, "settings not switched");
    assert_eq!(names(&f.store()), ["Default", "Alpha", "Beta"]);
    assert!(f.trash.0.borrow().is_empty());
    assert!(!f.target().exists(), "new copies removed");
}

#[cfg(unix)]
#[test]
fn copy_failure_removes_new_copies() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    let plan = plan_move(&f.store(), &f.target()).expect("plan");
    let locked = plan.old_root.join("beta/SaveData/Saves/colony.rws");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).expect("chmod");
    if std::fs::read(&locked).is_ok() {
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o644)).expect("chmod");
        return; // root: permissions not enforced
    }
    let r = f.apply(&plan);
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o644)).expect("chmod");
    assert!(matches!(r, Err(ProfileError::Io { .. })), "{r:?}");
    assert!(!f.target().exists());
    assert_eq!(f.store().root(), plan.old_root);
}

#[test]
fn bad_targets_refused() {
    let f = Fixture::new();
    let s = f.store();
    let old = s.root().to_path_buf();
    let full = f.tmp.path().join("full");
    std::fs::create_dir_all(&full).expect("dir");
    std::fs::write(full.join("mine.txt"), "x").expect("w");
    for bad in [
        PathBuf::from("relative/dir"),
        f.tmp.path().join("a=b"),
        old.join("inner"),
        old.parent().expect("parent").to_path_buf(),
        old.clone(),
        full,
    ] {
        assert!(plan_move(&s, &bad).is_err(), "{}", bad.display());
    }
}

#[test]
fn interrupted_move_can_be_retried() {
    let f = Fixture::new();
    let plan = plan_move(&f.store(), &f.target()).expect("plan");
    // Simulate a crash mid-copy: marker + a partial copy left behind.
    std::fs::create_dir_all(f.target().join("alpha/SaveData")).expect("partial");
    write_marker(&plan).expect("marker");
    let retry = plan_move(&f.store(), &f.target()).expect("retry allowed");
    f.apply(&retry).expect("move");
    assert_eq!(names(&f.store()), ["Default", "Alpha", "Beta"]);
    // A foreign file next to a marker is still refused.
    let other = f.tmp.path().join("other");
    std::fs::create_dir_all(&other).expect("dir");
    std::fs::write(
        other.join(MARKER),
        "old_root = \"/x\"\nprofiles = [\"alpha\"]\n",
    )
    .expect("marker");
    std::fs::write(other.join("notes.txt"), "mine").expect("w");
    assert!(matches!(
        plan_move(&f.store(), &other),
        Err(ProfileError::BadMoveTarget(_))
    ));
}

#[test]
fn refuses_while_any_game_runs() {
    let f = Fixture::new();
    let plan = plan_move(&f.store(), &f.target()).expect("plan");
    let r = apply_move(&f.store(), &f.app(), &plan, &FakeProbe(true), &mut |_| {});
    assert!(matches!(r, Err(ProfileError::GameRunning)));
    assert!(!f.target().exists());
}

#[test]
fn refuses_while_run_lock_held() {
    let f = Fixture::new();
    let plan = plan_move(&f.store(), &f.target()).expect("plan");
    std::fs::create_dir_all(f.app()).expect("app");
    let lock = File::create(f.app().join(RUN_LOCK)).expect("lock");
    lock.try_lock().expect("take");
    assert!(matches!(f.apply(&plan), Err(ProfileError::GameRunning)));
    drop(lock);
    assert!(!f.target().exists());
}

#[test]
fn stale_rimsort_instances_flagged() {
    let f = Fixture::new();
    let old = f.store().root().to_path_buf();
    let settings = f.tmp.path().join("RimSort/settings.json");
    std::fs::create_dir_all(settings.parent().expect("dir")).expect("dir");
    let inst = |run: &str, cfg: &str| serde_json::json!({"run_args": run, "config_folder": cfg, "local_folder": "/g/Mods"});
    let json = serde_json::json!({"instances": {
        "Default": inst("", "/cfg"),
        "Beta": inst("'/rwpm' run beta -- %command%", &format!("{}/beta/SaveData/Config", old.display())),
        // Exported since v1.1, with the RimSort marker (B5).
        "Alpha": inst("'/rwpm' run --from rimsort alpha -- %command%", &format!("{}/alpha/SaveData/Config", old.display())),
        "Elsewhere": inst("'/rwpm' run alpha -- %command%", "/new/alpha/SaveData/Config"),
        "Foreign": inst("gamemoderun %command%", &format!("{}/beta/SaveData/Config", old.display())),
    }});
    let before = serde_json::to_vec(&json).expect("json");
    std::fs::write(&settings, &before).expect("w");
    let ids: Vec<ProfileId> = ["alpha", "beta"]
        .iter()
        .map(|i| ProfileId::parse(i).expect("id"))
        .collect();
    assert_eq!(
        stale_rimsort(&settings, &old, &ids),
        ["Beta", "Alpha"],
        "file order"
    );
    assert_eq!(std::fs::read(&settings).expect("r"), before, "read only");
}
