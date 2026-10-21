//! A profile's health check: problems in plain words, each with the fix the app
//! offers. Read only: fixes go through the existing actions, which keep the consent
//! pattern for anything outside this tool's folders.

use std::path::Path;

use crate::builds::{EffectiveGame, effective_game};
use crate::discovery::GameInstall;
use crate::error::ProfileError;
use crate::launch::{self, Os};
use crate::profile::{Mode, Profile, ProfileStore, Trash, guard};
use crate::{rimsort, winhttp};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    /// Only the game's own content (Core, DLCs) is turned on here, while Default has other
    /// mods: usually a profile made empty. Fix: copy settings from Default.
    OnlyGameContent,
    /// OFFLINE, but these turned-on mods would load from Steam. Fix: freeze them.
    NotFrozen(Vec<String>),
    /// Turned on but installed nowhere; the game turns them off. Fix: none (look at the list).
    NotInstalled(Vec<String>),
    /// Windows OFFLINE needs the loader in the game folder. Fix: install it (consent).
    LoaderMissing,
    /// Another tool's `winhttp.dll` is there and cannot be used. No fix here.
    LoaderForeign,
    /// RimSort's entry points at this profile's old folder (after a Move). Fix: add again.
    RimSortStale(String),
    /// Pinned to a stored game version that is gone or cannot be read. Every game
    /// action refuses until the profile is pinned again. No fix here.
    BuildMissing(String),
}

/// Does `profile` turn on anything beyond the game's own content (`ludeon.rimworld*`)?
/// No mod list counts as no.
fn has_own_mods(profile: &Profile) -> bool {
    let config = profile
        .save_data_dir()
        .join("Config")
        .join("ModsConfig.xml");
    crate::mods::read_active_mods(&config)
        .is_ok_and(|ids| ids.iter().any(|id| !id.starts_with("ludeon.rimworld")))
}

/// Checks `profile` against its effective game. A pinned build that cannot be used
/// (gone, unreadable, store not readable) is the one problem reported; an unpinned profile
/// with no game at all has nothing to check.
pub fn check_profile<T: Trash>(
    store: &ProfileStore<T>,
    profile: &Profile,
    live: Option<&GameInstall>,
    app_dir: &Path,
    rimsort_settings: Option<&Path>,
    os: Os,
) -> Vec<Problem> {
    match effective_game(profile, live, store.root(), app_dir) {
        Ok(game) => check(store, profile, &game, app_dir, rimsort_settings, os),
        Err(ProfileError::NoGame) => Vec::new(),
        Err(_) => profile
            .game_build
            .clone()
            .map(|key| vec![Problem::BuildMissing(key)])
            .unwrap_or_default(),
    }
}

/// Does RimSort's entry point somewhere other than where `profile` plays now: its config
/// folder (after a Move) or its game and mod folders (after a game version change)?
/// A field the entry does not have is not compared.
fn rimsort_entry_is_stale(entry: &rimsort::Entry, profile: &Profile, game: &EffectiveGame) -> bool {
    let differs = |found: &str, ours: &Path| Path::new(found) != ours;
    differs(
        &entry.config_folder,
        &profile.save_data_dir().join("Config"),
    ) || entry
        .game_folder
        .as_deref()
        .is_some_and(|f| differs(f, game.rimsort_game_folder()))
        || entry
            .local_folder
            .as_deref()
            .is_some_and(|f| differs(f, &rimsort::local_folder(profile, game)))
}

/// Checks `profile`. `rimsort_settings`: RimSort's settings file, if RimSort is installed.
pub fn check<T: Trash>(
    store: &ProfileStore<T>,
    profile: &Profile,
    game: &EffectiveGame,
    app_dir: &Path,
    rimsort_settings: Option<&Path>,
    os: Os,
) -> Vec<Problem> {
    let mut problems = Vec::new();
    if !profile.is_default()
        && !has_own_mods(profile)
        && store
            .get(&crate::profile::ProfileId::default_id())
            .is_ok_and(|d| has_own_mods(&d))
    {
        problems.push(Problem::OnlyGameContent);
    }
    let offline = profile.mode == Mode::Offline && !profile.is_default();
    if offline && let Ok(report) = guard::check_offline(store, &profile.id, game) {
        if !report.unfrozen.is_empty() {
            let names = report.unfrozen.into_iter().map(|u| u.name).collect();
            problems.push(Problem::NotFrozen(names));
        }
        if !report.missing.is_empty() {
            problems.push(Problem::NotInstalled(report.missing));
        }
    }
    if !profile.is_default() && launch::needs_game_folder_loader(profile.mode, os, game.engine()) {
        match winhttp::launch_check(&game.game_dir, app_dir) {
            Ok(_) => {}
            Err(winhttp::WinhttpError::Foreign) => problems.push(Problem::LoaderForeign),
            Err(_) => problems.push(Problem::LoaderMissing),
        }
    }
    if let Some(settings) = rimsort_settings
        && let Some(entry) = rimsort::entry_for(settings, profile.id.as_str())
        && rimsort_entry_is_stale(&entry, profile, game)
    {
        problems.push(Problem::RimSortStale(entry.name));
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::Engine;
    use crate::profile::{ProfileFile, ProfileStore};

    struct NoTrash;
    impl Trash for NoTrash {
        fn trash(&self, _: &Path) -> std::io::Result<()> {
            Ok(())
        }
    }

    struct World {
        tmp: tempfile::TempDir,
    }

    impl World {
        fn new() -> Self {
            Self {
                tmp: tempfile::tempdir().expect("tempdir"),
            }
        }

        fn path(&self, rel: &str) -> std::path::PathBuf {
            self.tmp.path().join(rel)
        }

        fn store(&self) -> ProfileStore<NoTrash> {
            ProfileStore::open(self.path("profiles"), self.path("save"), NoTrash).expect("store")
        }

        fn add_mod(&self, rel: &str, id: &str, name: &str) {
            let dir = self.path(rel).join("About");
            std::fs::create_dir_all(&dir).expect("about");
            std::fs::write(
                dir.join("About.xml"),
                format!(
                    "<ModMetaData><name>{name}</name><packageId>{id}</packageId></ModMetaData>"
                ),
            )
            .expect("about");
        }

        fn game(&self) -> EffectiveGame {
            EffectiveGame::steam(GameInstall {
                game_dir: self.path("game"),
                exe: self.path("game/RimWorldWin64.exe"),
                version: None,
                engine: Engine::Unity,
                steam: Some(crate::discovery::SteamInfo {
                    library_dir: self.path("lib"),
                    workshop_dir: self.path("workshop"),
                }),
            })
        }

        fn offline(&self, active: &[&str]) -> Profile {
            let store = self.store();
            let p = store.create("Sick").expect("create");
            let dir = store.root().join(p.id.as_str());
            let mut file = ProfileFile::read(&dir.join("profile.toml")).expect("toml");
            file.mode = Mode::Offline;
            file.write_atomic(&dir.join("profile.toml")).expect("toml");
            let config = dir.join("SaveData/Config/ModsConfig.xml");
            std::fs::create_dir_all(config.parent().expect("p")).expect("dir");
            let items: String = active.iter().map(|a| format!("<li>{a}</li>")).collect();
            std::fs::write(
                config,
                format!("<ModsConfigData><activeMods>{items}</activeMods></ModsConfigData>"),
            )
            .expect("config");
            store.get(&p.id).expect("get")
        }
    }

    #[test]
    fn offline_profile_lists_unfrozen_and_missing_mods_by_name() {
        let w = World::new();
        w.add_mod("game/Data/Core", "ludeon.rimworld", "Core");
        w.add_mod("workshop/111", "brrainz.harmony", "Harmony");
        let p = w.offline(&["ludeon.rimworld", "brrainz.harmony", "me.gone"]);
        let problems = check(&w.store(), &p, &w.game(), &w.path("app"), None, Os::Linux);
        assert_eq!(
            problems,
            [
                Problem::NotFrozen(vec!["Harmony".into()]),
                Problem::NotInstalled(vec!["me.gone".into()])
            ]
        );
    }

    #[test]
    fn profile_with_only_game_content_is_flagged_when_default_has_mods() {
        let w = World::new();
        let config = |dir: &std::path::Path, ids: &[&str]| {
            let f = dir.join("Config/ModsConfig.xml");
            std::fs::create_dir_all(f.parent().expect("p")).expect("dir");
            let items: String = ids.iter().map(|a| format!("<li>{a}</li>")).collect();
            std::fs::write(
                f,
                format!("<ModsConfigData><activeMods>{items}</activeMods></ModsConfigData>"),
            )
            .expect("w");
        };
        let p = w.offline(&["ludeon.rimworld", "ludeon.rimworld.royalty"]);
        let check_now = || check(&w.store(), &p, &w.game(), &w.path("app"), None, Os::Linux);
        assert!(
            !check_now().contains(&Problem::OnlyGameContent),
            "Default has no mods either"
        );
        config(&w.path("save"), &["ludeon.rimworld", "brrainz.harmony"]);
        assert!(check_now().contains(&Problem::OnlyGameContent));
        config(&p.save_data_dir(), &["ludeon.rimworld", "brrainz.harmony"]);
        assert!(
            !check_now().contains(&Problem::OnlyGameContent),
            "it has mods now"
        );
    }

    #[test]
    fn windows_offline_needs_the_loader() {
        let w = World::new();
        let p = w.offline(&[]);
        std::fs::create_dir_all(w.path("game")).expect("game");
        let problems = check(&w.store(), &p, &w.game(), &w.path("app"), None, Os::Windows);
        assert_eq!(problems, [Problem::LoaderMissing]);
        let linux = check(&w.store(), &p, &w.game(), &w.path("app"), None, Os::Linux);
        assert!(linux.is_empty(), "Linux needs no loader");
    }

    #[test]
    fn rimsort_entry_pointing_elsewhere_is_stale() {
        let w = World::new();
        let p = w.offline(&[]);
        let settings = w.path("RimSort/settings.json");
        std::fs::create_dir_all(settings.parent().expect("p")).expect("dir");
        let here = p.save_data_dir().join("Config");
        let write = |folder: &str| {
            let json = serde_json::json!({"instances": {"Sick": {
                "run_args": format!("'/r' run --from rimsort {} -- %command%", p.id),
                "config_folder": folder,
            }}});
            std::fs::write(&settings, serde_json::to_vec(&json).expect("j")).expect("w");
        };
        write("/old/root/sick/SaveData/Config");
        let problems = check(
            &w.store(),
            &p,
            &w.game(),
            &w.path("app"),
            Some(&settings),
            Os::Linux,
        );
        assert_eq!(problems, [Problem::RimSortStale("Sick".into())]);
        write(&here.display().to_string());
        let problems = check(
            &w.store(),
            &p,
            &w.game(),
            &w.path("app"),
            Some(&settings),
            Os::Linux,
        );
        assert!(problems.is_empty());
    }

    /// The player changes a profile's game version after exporting it. RimSort's
    /// `game_folder` / `local_folder` still name the old game, so health says so; a
    /// current entry, or one without those fields, is fine.
    #[test]
    fn rimsort_entry_naming_another_game_is_stale() {
        let w = World::new();
        let p = w.offline(&[]);
        let game = w.game();
        let settings = w.path("RimSort/settings.json");
        std::fs::create_dir_all(settings.parent().expect("p")).expect("dir");
        let config = p.save_data_dir().join("Config").display().to_string();
        let ours_game = game.rimsort_game_folder().display().to_string();
        let ours_local = rimsort::local_folder(&p, &game).display().to_string();
        let problems_with = |entry: serde_json::Value| {
            let mut entry = entry;
            entry["run_args"] = format!("'/r' run --from rimsort {} -- %command%", p.id).into();
            let json = serde_json::json!({"instances": {"Sick": entry}});
            std::fs::write(&settings, serde_json::to_vec(&json).expect("j")).expect("w");
            check(
                &w.store(),
                &p,
                &game,
                &w.path("app"),
                Some(&settings),
                Os::Linux,
            )
        };
        let stale = vec![Problem::RimSortStale("Sick".into())];

        let current = serde_json::json!({
            "config_folder": config, "game_folder": ours_game, "local_folder": ours_local
        });
        assert!(problems_with(current).is_empty());
        let old_game = serde_json::json!({
            "config_folder": config, "game_folder": "/old/build", "local_folder": ours_local
        });
        assert_eq!(problems_with(old_game), stale);
        let old_local = serde_json::json!({
            "config_folder": config, "game_folder": ours_game, "local_folder": "/old/build/Mods"
        });
        assert_eq!(problems_with(old_local), stale);
        let no_fields = serde_json::json!({"config_folder": config});
        assert!(
            problems_with(no_fields).is_empty(),
            "absent fields are not compared"
        );
    }

    #[test]
    fn waylith_offline_windows_no_loader_problem() {
        let w = World::new();
        let p = w.offline(&[]);
        // The engine is resolved when the install is read, so a 1.7 install is a
        // `GameInstall` that says Waylith, not a marker file found later.
        let game = EffectiveGame::steam(GameInstall {
            engine: Engine::Waylith,
            ..w.game().install().clone()
        });
        let problems = check(&w.store(), &p, &game, &w.path("app"), None, Os::Windows);
        assert!(problems.is_empty(), "{problems:?}");
    }
}
