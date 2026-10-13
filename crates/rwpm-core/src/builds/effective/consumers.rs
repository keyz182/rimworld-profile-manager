//! V55: every consumer of a profile's game follows its pin. One world per test: Steam
//! on one version with `live.local` in its `Mods`, a stored build of another version
//! with `build.local` in its `Mods`, and the workshop item `steam.only` shared by both.

use std::path::{Path, PathBuf};

use super::test_support::Games;
use super::*;
use crate::consent::{ConsentStore, Scope};
use crate::launch::Os;
use crate::profile::file::{FILE_NAME, Mode, ProfileFile};
use crate::profile::guard::{Fix, check_offline};
use crate::profile::health::{self, Problem};
use crate::profile::mod_list::{Source, mod_list};
use crate::profile::offline::plan_offline;
use crate::profile::store::ProfileStore;
use crate::winhttp;

struct NoTrash;
impl crate::profile::store::Trash for NoTrash {
    fn trash(&self, _: &Path) -> std::io::Result<()> {
        Ok(())
    }
}

const ACTIVE: &[&str] = &["ludeon.rimworld", "steam.only", "live.local", "build.local"];

struct World {
    tmp: tempfile::TempDir,
    games: Games,
}

impl World {
    fn new() -> Self {
        let tmp = tempfile::tempdir().expect("tempdir");
        let games = Games::new(tmp.path(), &tmp.path().join("profiles"));
        Self { tmp, games }
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.tmp.path().join(rel)
    }

    fn store(&self) -> ProfileStore<NoTrash> {
        ProfileStore::open(self.path("profiles"), self.path("save"), NoTrash).expect("store")
    }

    /// A profile in `mode` turning on `ACTIVE`, pinned or not. OFFLINE gets an empty
    /// `Mods` folder, as after going OFFLINE with nothing frozen.
    fn profile(&self, mode: Mode, pinned: bool) -> Profile {
        let store = self.store();
        let p = store
            .create(if pinned { "Pinned" } else { "Plain" })
            .expect("create");
        let dir = p.stored_dir().expect("stored").to_path_buf();
        let config = dir.join("SaveData/Config/ModsConfig.xml");
        std::fs::create_dir_all(config.parent().expect("parent")).expect("config dir");
        let items: String = ACTIVE.iter().map(|a| format!("<li>{a}</li>")).collect();
        std::fs::write(
            &config,
            format!("<ModsConfigData><activeMods>{items}</activeMods></ModsConfigData>"),
        )
        .expect("config");
        let mut file = ProfileFile::read(&dir.join(FILE_NAME)).expect("read");
        file.mode = mode;
        file.write_atomic(&dir.join(FILE_NAME)).expect("write");
        if mode == Mode::Offline {
            std::fs::create_dir_all(dir.join("Mods")).expect("mods");
        }
        let p = store.get(&p.id).expect("get");
        if pinned {
            self.games.pin(&store, &p)
        } else {
            p
        }
    }

    fn game(&self, p: &Profile) -> EffectiveGame {
        effective_game(
            p,
            Some(&self.games.live),
            &self.path("profiles"),
            &self.path("app"),
        )
        .expect("resolve")
    }

    fn under_build(&self, path: &Path) -> bool {
        path.starts_with(&self.games.build_dir)
    }
}

#[test]
fn v55_go_offline_copies_local_mods_from_the_pinned_build() {
    let w = World::new();
    let p = w.profile(Mode::Live, true);
    let plan = plan_offline(&w.store(), &p.id, &w.game(&p), false).expect("plan");
    let local: Vec<_> = plan
        .copies
        .iter()
        .filter(|c| c.package_id.ends_with(".local"))
        .collect();
    assert_eq!(local.len(), 1, "{plan:?}");
    assert_eq!(local[0].package_id, "build.local");
    assert!(w.under_build(&local[0].source_dir));
    assert_eq!(plan.missing, ["live.local"]);
    assert!(plan.copies.iter().any(|c| c.package_id == "steam.only"));
}

#[test]
fn v55_go_offline_unpinned_still_uses_the_steam_install() {
    let w = World::new();
    let p = w.profile(Mode::Live, false);
    let plan = plan_offline(&w.store(), &p.id, &w.game(&p), true).expect("plan");
    assert!(plan.copies.iter().any(|c| c.package_id == "live.local"));
    assert_eq!(plan.missing, ["build.local"]);
}

#[test]
fn v55_mod_list_reads_the_pinned_build() {
    let w = World::new();
    let p = w.profile(Mode::Live, true);
    let rows = mod_list(&p, &w.game(&p)).expect("rows");
    let source = |id: &str| rows.iter().find(|r| r.package_id == id).expect(id).source;
    assert_eq!(source("build.local"), Some(Source::Installed));
    assert_eq!(source("live.local"), None);
    assert_eq!(source("steam.only"), Some(Source::Steam));
    assert_eq!(source("ludeon.rimworld"), Some(Source::Game));

    let plain = w.profile(Mode::Live, false);
    let rows = mod_list(&plain, &w.game(&plain)).expect("rows");
    let source = |id: &str| rows.iter().find(|r| r.package_id == id).expect(id).source;
    assert_eq!(source("live.local"), Some(Source::Installed));
    assert_eq!(source("build.local"), None);
}

#[test]
fn v55_guard_offers_copies_from_the_pinned_build() {
    let w = World::new();
    let p = w.profile(Mode::Offline, true);
    let report = check_offline(&w.store(), &p.id, &w.game(&p)).expect("report");
    let copy_of = |id: &str| {
        report.unfrozen.iter().find_map(|u| match &u.fix {
            Fix::Copy(c) if u.active_id == id => Some(c.source_dir.clone()),
            _ => None,
        })
    };
    assert!(w.under_build(&copy_of("build.local").expect("build.local offered")));
    assert_eq!(copy_of("live.local"), None);
    assert!(
        report.missing.contains(&"live.local".to_owned()),
        "{report:?}"
    );
}

#[test]
fn v55_health_checks_the_loader_in_the_pinned_build() {
    let w = World::new();
    let app = w.path("app");
    let p = w.profile(Mode::Offline, true);
    let check =
        || health::check_profile(&w.store(), &p, Some(&w.games.live), &app, None, Os::Windows);
    assert!(check().contains(&Problem::LoaderMissing));
    let dll = w.games.build_dir.join(winhttp::DLL);
    let consent = ConsentStore::open(&app)
        .record(Scope::WinhttpDll, &dll)
        .expect("consent");
    let vendor = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../vendor/unitydoorstop/4.5.0/win_x64/winhttp.dll");
    winhttp::install(&w.games.build_dir, &app, &vendor, &consent).expect("install");
    assert!(!check().contains(&Problem::LoaderMissing), "{:?}", check());
    // The Steam install has no loader; an unpinned profile still reports it.
    let plain = w.profile(Mode::Offline, false);
    let problems = health::check_profile(
        &w.store(),
        &plain,
        Some(&w.games.live),
        &app,
        None,
        Os::Windows,
    );
    assert!(problems.contains(&Problem::LoaderMissing));
}

#[test]
fn v55_missing_build_is_the_one_health_problem() {
    let w = World::new();
    let p = w.profile(Mode::Offline, true);
    w.games.lose_build();
    let problems = health::check_profile(
        &w.store(),
        &p,
        Some(&w.games.live),
        &w.path("app"),
        None,
        Os::Windows,
    );
    assert_eq!(
        problems,
        [Problem::BuildMissing(w.games.build_key.to_string())]
    );
}
