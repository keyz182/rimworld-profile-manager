use std::path::{Path, PathBuf};

use super::test_support::{BUILD_VERSION, Games, STEAM_VERSION};
use super::*;
use crate::profile::id::ProfileId;
use crate::profile::store::ProfileStore;

struct NoTrash;
impl crate::profile::store::Trash for NoTrash {
    fn trash(&self, _: &Path) -> std::io::Result<()> {
        Ok(())
    }
}

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

    fn resolve(
        &self,
        p: &Profile,
        live: Option<&GameInstall>,
    ) -> Result<EffectiveGame, ProfileError> {
        effective_game(p, live, &self.path("profiles"), &self.path("app"))
    }

    fn pinned(&self) -> Profile {
        let store = self.store();
        let p = store.create("Pinned").expect("create");
        self.games.pin(&store, &p)
    }
}

#[test]
fn unpinned_profile_plays_the_steam_install() {
    let w = World::new();
    let p = w.store().create("Plain").expect("create");
    let g = w.resolve(&p, Some(&w.games.live)).expect("resolve");
    assert_eq!(g.source(), &GameSource::Steam);
    assert_eq!(g.install(), &w.games.live);
}

#[test]
fn default_profile_plays_the_steam_install() {
    let w = World::new();
    let d = w.store().get(&ProfileId::default_id()).expect("default");
    let g = w.resolve(&d, Some(&w.games.live)).expect("resolve");
    assert_eq!(g.source(), &GameSource::Steam);
}

#[test]
fn unpinned_without_steam_install_is_no_game() {
    let w = World::new();
    let p = w.store().create("Plain").expect("create");
    assert!(matches!(w.resolve(&p, None), Err(ProfileError::NoGame)));
}

#[test]
fn pinned_profile_plays_its_build_with_the_steam_workshop() {
    let w = World::new();
    let g = w
        .resolve(&w.pinned(), Some(&w.games.live))
        .expect("resolve");
    assert_eq!(g.source(), &GameSource::Build(w.games.build_key.clone()));
    assert_eq!(g.game_dir, w.games.build_dir);
    assert!(g.exe.starts_with(&w.games.build_dir));
    assert_eq!(g.version.as_deref(), Some(BUILD_VERSION));
    assert_ne!(g.version.as_deref(), Some(STEAM_VERSION));
    assert!(g.mods_dir().starts_with(&w.games.build_dir));
    assert_eq!(g.steam, w.games.live.steam);
}

#[test]
fn pinned_profile_without_steam_install_takes_the_workshop_it_was_captured_from() {
    let w = World::new();
    let g = w.resolve(&w.pinned(), None).expect("resolve");
    assert_eq!(g.game_dir, w.games.build_dir);
    assert_eq!(g.steam, w.games.live.steam);
}

#[test]
fn missing_build_is_refused_never_the_steam_install() {
    let w = World::new();
    let p = w.pinned();
    w.games.lose_build();
    let err = w.resolve(&p, Some(&w.games.live)).expect_err("refused");
    assert!(matches!(&err, ProfileError::BuildNotFound(k) if k == w.games.build_key.as_str()));
}

#[test]
fn build_without_build_toml_is_missing() {
    let w = World::new();
    let p = w.pinned();
    std::fs::remove_file(w.games.build_dir.join("build.toml")).expect("rm");
    assert!(matches!(
        w.resolve(&p, Some(&w.games.live)),
        Err(ProfileError::BuildNotFound(_))
    ));
}

#[test]
fn build_without_its_game_is_missing() {
    let w = World::new();
    let p = w.pinned();
    let exe = w.resolve(&p, None).expect("resolve").exe.clone();
    if exe.is_dir() {
        std::fs::remove_dir_all(&exe).expect("rm bundle");
    } else {
        std::fs::remove_file(&exe).expect("rm exe");
    }
    assert!(matches!(
        w.resolve(&p, Some(&w.games.live)),
        Err(ProfileError::BuildNotFound(_))
    ));
}

/// A pre-v1.4 profile at `<root>/builds` makes the build store refuse to
/// open. Unpinned profiles must not notice.
#[test]
fn unpinned_profile_ignores_a_legacy_profile_in_the_builds_folder() {
    let w = World::new();
    let p = w.store().create("Plain").expect("create");
    std::fs::write(w.path("profiles/builds/profile.toml"), "schema = 1").expect("legacy");
    let g = w.resolve(&p, Some(&w.games.live)).expect("resolve");
    assert_eq!(g.source(), &GameSource::Steam);
}

/// The build was captured from another library than the one Steam uses now.
/// The workshop follows the Steam install; the capture source is only the fallback.
#[test]
fn pinned_build_takes_the_steam_installs_workshop_over_its_capture_source() {
    let w = World::new();
    let p = w.pinned();
    let toml = w.games.build_dir.join("build.toml");
    let mut file = crate::builds::BuildFile::read(&toml).expect("read");
    file.source_dir = w.path("old-lib/steamapps/common/RimWorld");
    file.write_atomic(&toml).expect("write");
    let workshop = |g: &EffectiveGame| g.steam.as_ref().map(|s| s.workshop_dir.clone());

    let g = w.resolve(&p, Some(&w.games.live)).expect("resolve");
    assert_eq!(g.steam, w.games.live.steam);
    let g = w.resolve(&p, None).expect("resolve");
    assert_eq!(
        workshop(&g),
        Some(w.path("old-lib/steamapps/workshop/content/294100"))
    );
}

/// A key that is not a build key (only reachable by building a `Profile`
/// directly — `profile.toml` refuses it on read) is `BuildNotFound`, never the Steam install.
#[test]
fn invalid_build_key_is_build_not_found() {
    let w = World::new();
    let p = Profile {
        game_build: Some("Not A Key!".into()),
        ..w.store().create("Odd").expect("create")
    };
    assert!(matches!(
        w.resolve(&p, Some(&w.games.live)),
        Err(ProfileError::BuildNotFound(k)) if k == "Not A Key!"
    ));
}
