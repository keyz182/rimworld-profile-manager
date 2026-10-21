//! What the MCP tools do: profile reads and the create / clone / rename writes.
//!
//! Every function returns JSON built from an allow-list of fields, or a plain-words error.
//! Text that other people wrote (mod and pack names, a refusal reason) is cleaned and capped
//! first: the agent reads it, and it must stay data.

use std::path::{Path, PathBuf};

use rwpm_core::builds::effective_game;
use rwpm_core::profile::health::{self, Problem};
use rwpm_core::profile::mod_list::{self, Flag, Source};
use rwpm_core::profile::{
    LastRun, Location, Mode, OsTrash, Profile, ProfileId, ProfileStore, RunningProbe,
};
use rwpm_core::settings::AppSettings;
use rwpm_core::{launch::Os, rimsort, space};
use serde_json::{Value, json};

use crate::run;

/// Longest text field handed to the agent, in characters.
const MAX_TEXT: usize = 200;
/// Most names listed inside one problem.
const MAX_NAMES: usize = 50;
/// Extra room a clone must have on top of the source size, as a divisor (10 → 10 %).
const CLONE_MARGIN_DIVISOR: u64 = 10;

/// Control characters become spaces and long text is cut short.
pub(super) fn clean(text: &str) -> String {
    let flat: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let flat = flat.trim();
    if flat.chars().count() <= MAX_TEXT {
        return flat.to_owned();
    }
    let cut: String = flat.chars().take(MAX_TEXT).collect();
    format!("{cut}…")
}

fn names(list: &[String]) -> Vec<String> {
    list.iter().take(MAX_NAMES).map(|n| clean(n)).collect()
}

fn path_text(path: &Path) -> String {
    path.display().to_string()
}

pub(super) struct Ops {
    store: ProfileStore<OsTrash>,
    app_dir: PathBuf,
    settings: AppSettings,
    /// Free bytes on the disk holding a path; swapped in tests.
    free_space: fn(&Path) -> Option<u64>,
}

impl Ops {
    /// Reads the settings and opens the store afresh, so changes made in the window show up.
    pub(super) fn open() -> Result<Self, String> {
        let (store, settings) = run::open().map_err(|e| e.message().to_owned())?;
        let app_dir = rwpm_core::paths::app_data_dir().ok_or("no user data folder")?;
        Ok(Self::new(store, app_dir, settings))
    }

    pub(super) fn new(
        store: ProfileStore<OsTrash>,
        app_dir: PathBuf,
        settings: AppSettings,
    ) -> Self {
        Self {
            store,
            app_dir,
            settings,
            free_space: space::free_space,
        }
    }

    #[cfg(test)]
    pub(super) fn with_free_space(mut self, free_space: fn(&Path) -> Option<u64>) -> Self {
        self.free_space = free_space;
        self
    }

    fn profile(&self, id: &str) -> Result<Profile, String> {
        let id = ProfileId::parse(id).map_err(|e| e.to_string())?;
        self.store.get(&id).map_err(|e| e.to_string())
    }

    pub(super) fn list_profiles(&self) -> Result<Value, String> {
        let listing = self.store.list().map_err(|e| e.to_string())?;
        let unreadable: Vec<Value> = listing
            .unreadable
            .iter()
            .map(|(id, e)| json!({ "id": id.as_str(), "problem": clean(&e.to_string()) }))
            .collect();
        Ok(json!({
            "profiles": listing.profiles.iter().map(profile_json).collect::<Vec<_>>(),
            "unreadable": unreadable,
        }))
    }

    pub(super) fn get_profile(&self, id: &str) -> Result<Value, String> {
        self.profile(id).map(|p| profile_json(&p))
    }

    pub(super) fn profile_mods(&self, id: &str) -> Result<Value, String> {
        let profile = self.profile(id)?;
        let game = effective_game(
            &profile,
            self.settings.game().ok().as_ref(),
            self.store.root(),
            &self.app_dir,
        )
        .map_err(|e| e.to_string())?;
        let rows = mod_list::mod_list(&profile, &game).map_err(|e| clean(&e))?;
        let mods: Vec<Value> = rows
            .iter()
            .map(|r| {
                json!({
                    "package_id": clean(&r.package_id),
                    "name": clean(&r.name),
                    "source": r.source.map(source_text),
                    "flag": r.flag.map(flag_text),
                })
            })
            .collect();
        Ok(json!({ "profile": profile.id.as_str(), "mods": mods }))
    }

    pub(super) fn profile_health(&self, id: &str) -> Result<Value, String> {
        let profile = self.profile(id)?;
        let rimsort_settings = rimsort::settings_path().filter(|s| s.is_file());
        let problems = health::check_profile(
            &self.store,
            &profile,
            self.settings.game().ok().as_ref(),
            &self.app_dir,
            rimsort_settings.as_deref(),
            Os::current(),
        );
        Ok(json!({
            "profile": profile.id.as_str(),
            "problems": problems.iter().map(problem_json).collect::<Vec<_>>(),
        }))
    }

    pub(super) fn last_run(&self, id: &str) -> Result<Value, String> {
        let profile = self.profile(id)?;
        let last = LastRun::read(&LastRun::path(self.store.root(), &profile)).map(|r| {
            json!({
                "time": r.time.to_string(),
                "started_by": serde_json::to_value(r.started_by).unwrap_or(Value::Null),
                "outcome": serde_json::to_value(r.outcome).unwrap_or(Value::Null),
                "reason": clean(&r.reason),
                "exit": r.exit,
            })
        });
        Ok(json!({
            "profile": profile.id.as_str(),
            "last_run": last,
            "log_file": profile.log_file().as_deref().map(path_text),
        }))
    }

    pub(super) fn game_install(&self) -> Result<Value, String> {
        let game = self.settings.game().map_err(|e| clean(&e.to_string()))?;
        Ok(json!({
            "game_dir": path_text(&game.game_dir),
            "exe": path_text(&game.exe),
            "version": game.version.as_deref().map(clean),
            "engine": format!("{:?}", game.engine()),
            "steam": game.steam.is_some(),
        }))
    }

    pub(super) fn create_profile(&self, name: &str) -> Result<Value, String> {
        let profile = self.store.create(name).map_err(|e| e.to_string())?;
        Ok(profile_json(&profile))
    }

    pub(super) fn rename_profile(&self, id: &str, name: &str) -> Result<Value, String> {
        let id = ProfileId::parse(id).map_err(|e| e.to_string())?;
        let profile = self.store.rename(&id, name).map_err(|e| e.to_string())?;
        Ok(profile_json(&profile))
    }

    /// Refused up front when the copy would not fit: an agent cannot watch a progress bar,
    /// and a cancelled request does not stop the copy.
    pub(super) fn clone_profile(
        &self,
        source: &str,
        name: &str,
        probe: &dyn RunningProbe,
    ) -> Result<Value, String> {
        let src = self.profile(source)?;
        self.check_room_for_clone(&src)?;
        let profile = self
            .store
            .clone_profile(&src.id, name, probe, &mut |_| {})
            .map_err(|e| e.to_string())?;
        Ok(profile_json(&profile))
    }

    fn check_room_for_clone(&self, src: &Profile) -> Result<(), String> {
        let from = match &src.location {
            Location::Default { save_dir } => save_dir,
            Location::Stored { dir } => dir,
        };
        let size = space::dir_size(from);
        let need = size.saturating_add(size / CLONE_MARGIN_DIVISOR);
        let free = (self.free_space)(self.store.root())
            .ok_or("cannot read the free disk space, so the copy was not started")?;
        if free < need {
            return Err(format!(
                "not enough disk space to copy this profile: needs about {} MiB, {} MiB free",
                need >> 20,
                free >> 20
            ));
        }
        Ok(())
    }
}

fn profile_json(p: &Profile) -> Value {
    json!({
        "id": p.id.as_str(),
        "name": clean(&p.name),
        "default": p.is_default(),
        "mode": match p.mode {
            Mode::Live => "live",
            Mode::Offline => "offline",
        },
        "game_build": p.game_build.as_deref().map(clean),
        "pack": p.pack.as_ref().map(|o| clean(&o.name)),
        "save_data_dir": path_text(&p.save_data_dir()),
    })
}

fn source_text(s: Source) -> &'static str {
    match s {
        Source::Game => "game",
        Source::Frozen => "frozen",
        Source::Steam => "steam",
        Source::Installed => "installed",
    }
}

fn flag_text(f: Flag) -> &'static str {
    match f {
        Flag::Missing => "missing",
        Flag::NotFrozen => "not_frozen",
        Flag::Twice => "installed_twice",
    }
}

fn problem_json(p: &Problem) -> Value {
    match p {
        Problem::OnlyGameContent => json!({ "kind": "only_game_content" }),
        Problem::NotFrozen(mods) => json!({ "kind": "not_frozen", "mods": names(mods) }),
        Problem::NotInstalled(mods) => json!({ "kind": "not_installed", "mods": names(mods) }),
        Problem::LoaderMissing => json!({ "kind": "loader_missing" }),
        Problem::LoaderForeign => json!({ "kind": "loader_foreign" }),
        Problem::RimSortStale(_) => json!({ "kind": "rimsort_stale" }),
        Problem::BuildMissing(key) => json!({ "kind": "build_missing", "build": clean(key) }),
    }
}
