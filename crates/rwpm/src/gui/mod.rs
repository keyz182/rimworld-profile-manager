//! The desktop app: profile list, actions, progress. iced, Elm-style: all state
//! changes go through `update`, so they are tested without a window.

mod backup;
mod builds;
mod copy;
mod dialogs;
mod guide;
mod health;
mod icon;
mod jobs;
mod keys;
mod message;
mod modpack;
mod modpack_open;
mod mods;
mod pack_fetch;
mod pack_reset;
mod pack_sync;
mod picker;
mod setup;
mod support;
pub(crate) mod text;
mod theme;
mod update;
mod updates;
mod view;
mod watch;

use std::collections::HashMap;
use std::path::PathBuf;

use rwpm_core::builds::{EffectiveGame, effective_game};
use rwpm_core::discovery::GameInstall;
use rwpm_core::paths::Layout;
use rwpm_core::profile::{LastRun, OsTrash, Profile, ProfileId, ProfileStore};
use rwpm_core::settings::AppSettings;
use rwpm_core::update_check::{Release, UpdateState};
use rwpm_core::{ProfileError, appimage, flatpak, paths};

pub use message::Dialog;

/// Paths every action needs; cheap to clone into a background job.
#[derive(Debug, Clone)]
pub struct Ctx {
    pub app_dir: PathBuf,
    pub game_save_dir: PathBuf,
    /// `rwpm` itself, for launching (`rwpm run`) and RimSort's run_args. With Flatpak
    /// Steam: the copy in `<app data>/bin`, which Steam's sandbox can see. From an
    /// AppImage: the same copy, since the AppImage's own folder goes when it exits.
    pub exe: PathBuf,
    /// With Flatpak Steam: the `flatpak` program; Play then runs rwpm in Steam's sandbox.
    pub flatpak: Option<PathBuf>,
    /// The launcher copy failed: `exe` is a path Steam cannot use (Flatpak) or that goes
    /// when this app exits (AppImage), so nothing may save it.
    pub launcher_failed: bool,
}

impl Ctx {
    /// The real locations on this machine, and a notice for the player if any.
    pub fn detect() -> Result<(Self, Option<String>), String> {
        let mut ctx = Self {
            app_dir: paths::app_data_dir().ok_or("no user data folder")?,
            game_save_dir: paths::default_game_save_dir().ok_or("no game save folder")?,
            exe: std::env::current_exe().map_err(|e| e.to_string())?,
            flatpak: None,
            launcher_failed: false,
        };
        let mut notes = Vec::new();
        if paths::layout() == Layout::FlatpakSteam {
            let install = ctx.exe.parent().ok_or("cannot find this app's folder")?;
            match flatpak::sync_launcher(install, &ctx.app_dir.join("bin")) {
                Ok(copy) => {
                    ctx.exe = copy;
                    ctx.flatpak = Some("flatpak".into());
                }
                Err(e) => {
                    ctx.launcher_failed = true;
                    notes.push(text::launcher_failed(&e.to_string()));
                }
            }
        } else if let Some(mount) = appimage::current() {
            match flatpak::sync_launcher(&mount, &ctx.app_dir.join("bin")) {
                Ok(copy) => ctx.exe = copy,
                Err(e) => {
                    ctx.launcher_failed = true;
                    notes.push(text::appimage_launcher_failed(&e.to_string()));
                }
            }
        } else if cfg!(target_os = "macos") {
            // Gatekeeper app translocation makes a bundle's own run location
            // unstable for an un-notarised .app, same class of problem as the AppImage
            // mount path above — always sync a copy to a location that stays put.
            let install = ctx.exe.parent().ok_or("cannot find this app's folder")?;
            match flatpak::sync_launcher(install, &ctx.app_dir.join("bin")) {
                Ok(copy) => ctx.exe = copy,
                Err(e) => {
                    ctx.launcher_failed = true;
                    notes.push(text::macos_launcher_failed(&e.to_string()));
                }
            }
        }
        Ok((ctx, (!notes.is_empty()).then(|| notes.join("\n"))))
    }

    pub fn settings(&self) -> Result<AppSettings, ProfileError> {
        AppSettings::load(&self.app_dir)
    }

    pub fn store(&self) -> Result<ProfileStore<OsTrash>, ProfileError> {
        let root = self.settings()?.profiles_root(&self.app_dir);
        ProfileStore::open(root, self.game_save_dir.clone(), OsTrash)
    }

    /// The game `profile` plays: its stored build when pinned, else `live`, the
    /// Steam install. A pinned profile whose build is gone or cannot be used (broken
    /// `build.toml`, store folder taken) reads as "build missing" in every action, like
    /// health and `rwpm run`; only a bad settings file keeps its own reason (#15 N1).
    pub fn effective_game(
        &self,
        profile: &Profile,
        live: Option<&GameInstall>,
    ) -> Result<EffectiveGame, GameError> {
        let root = self
            .settings()
            .map_err(|e| GameError::Other(e.to_string()))?
            .profiles_root(&self.app_dir);
        effective_game(profile, live, &root, &self.app_dir).map_err(|e| {
            match (e, profile.game_build.as_deref()) {
                (ProfileError::NoGame, _) => GameError::NoGame,
                (_, Some(key)) => GameError::BuildMissing(key.to_owned()),
                (e, None) => GameError::Other(e.to_string()),
            }
        })
    }
}

/// Why a profile has no game to play (#15 N1: typed, so callers never compare text).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GameError {
    /// Unpinned, and RimWorld was not found.
    NoGame,
    /// Pinned to a stored game version that is gone or cannot be read.
    BuildMissing(String),
    /// Anything else (settings or build store unreadable), in core's plain words.
    Other(String),
}

impl std::fmt::Display for GameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoGame => f.write_str(text::NO_GAME),
            Self::BuildMissing(key) => f.write_str(&text::build_missing(key)),
            Self::Other(reason) => f.write_str(reason),
        }
    }
}

#[derive(Debug)]
pub struct Job {
    pub label: String,
    pub done: u64,
    pub total: u64,
    /// `done` and `total` are steps, not bytes: shown as a percentage.
    pub percent: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    Info(String),
    Error(String),
}

#[derive(Debug)]
pub struct App {
    pub ctx: Ctx,
    pub game: Result<GameInstall, String>,
    pub profiles: Vec<Profile>,
    /// Profile folders whose profile.toml could not be read: (id, reason).
    pub unreadable: Vec<(String, String)>,
    pub selected: ProfileId,
    pub dialog: Option<Dialog>,
    pub job: Option<Job>,
    /// Set by the Cancel button of a job that can be stopped. Cleared when it ends.
    pub cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    /// How the Steam helper is run; tests replace it so none of them starts a process.
    pub fetcher: pack_fetch::Fetcher,
    /// How steamcmd is installed and run; tests replace it too.
    pub cmd: pack_fetch::CmdRunner,
    pub notice: Option<Notice>,
    /// The OS asks for dark colours.
    pub dark: bool,
    /// Profile search text.
    pub search: String,
    pub menu_open: bool,
    /// Some RimWorld is running: actions that clash with it wait.
    pub game_running: bool,
    /// Turned-on mods per profile; absent when the mod list cannot be read.
    pub mod_counts: HashMap<ProfileId, usize>,
    /// Pack profiles whose mods Steam has all fetched, as of the last look.
    pub pack_synced: std::collections::HashSet<ProfileId>,
    /// What each pack profile shows of its pack, read when the list loads.
    pub pack_cards: HashMap<ProfileId, pack_reset::PackCard>,
    /// Each profile's last `rwpm run` attempt, if any.
    pub last_runs: HashMap<ProfileId, LastRun>,
    /// First-run setup, while it is shown.
    pub setup: Option<setup::Setup>,
    /// The drill-in shown instead of the detail sections, if any.
    pub drill: Option<message::Drill>,
    /// The last mod list read and its search text.
    pub mods: Option<mods::ModList>,
    pub mod_search: String,
    /// Health problems per profile, for the profiles checked so far.
    pub health: HashMap<ProfileId, Vec<rwpm_core::profile::health::Problem>>,
    /// The other layout's profiles folder, when it has profiles this app ignores.
    pub other_profiles: Option<PathBuf>,
    /// The stored game versions, once read.
    pub builds: Option<builds::BuildsView>,
    /// Why the stored game versions could not be read, if they could not.
    pub builds_error: Option<String>,
    /// How often to check for a newer release, and when that last happened.
    pub updates: UpdateState,
    /// How the window asks GitHub for the newest release; tests replace it.
    pub update_check: updates::UpdateCheck,
    /// The newest release the last check found.
    pub update_found: Option<Release>,
    /// A check is on its way; no second one starts.
    pub update_checking: bool,
    /// How this copy was installed, and how it replaces itself; tests replace it.
    pub installer: updates::Installer,
    /// Where Install update has got to.
    pub install: updates::InstallState,
}

impl App {
    pub fn new(ctx: Ctx) -> Self {
        let mut app = Self {
            ctx,
            game: Err(String::new()),
            profiles: Vec::new(),
            unreadable: Vec::new(),
            selected: ProfileId::default_id(),
            dialog: None,
            job: None,
            cancel: None,
            fetcher: pack_fetch::Fetcher::real(),
            cmd: pack_fetch::CmdRunner::real(),
            notice: None,
            dark: false,
            search: String::new(),
            menu_open: false,
            game_running: false,
            mod_counts: HashMap::new(),
            pack_cards: HashMap::new(),
            pack_synced: std::collections::HashSet::new(),
            last_runs: HashMap::new(),
            setup: None,
            drill: None,
            mods: None,
            mod_search: String::new(),
            health: HashMap::new(),
            other_profiles: None,
            builds: None,
            builds_error: None,
            updates: UpdateState::default(),
            update_check: updates::UpdateCheck::real(),
            update_found: None,
            update_checking: false,
            installer: updates::Installer::real(),
            install: updates::InstallState::Idle,
        };
        app.reload();
        app.load_updates();
        app.decide_setup();
        // The last played profile is ready to Play with one click.
        if let Some(id) = app.last_played() {
            app.selected = id;
        }
        app
    }

    /// The profile whose game ran most recently.
    pub fn last_played(&self) -> Option<ProfileId> {
        self.last_runs
            .iter()
            .filter(|(_, run)| run.outcome == rwpm_core::profile::RunOutcome::Ok)
            .min_by_key(|(_, run)| run.age_secs().unwrap_or(u64::MAX))
            .map(|(id, _)| id.clone())
    }

    /// Re-reads every profile's `last-run.toml`.
    pub fn reload_last_runs(&mut self) {
        let Ok(store) = self.ctx.store() else {
            return;
        };
        self.last_runs = self
            .profiles
            .iter()
            .filter_map(|p| {
                Some((
                    p.id.clone(),
                    LastRun::read(&LastRun::path(store.root(), p))?,
                ))
            })
            .collect();
    }

    /// Re-reads settings, the game install and the profile list.
    pub fn reload(&mut self) {
        self.game = match self.ctx.settings() {
            Ok(s) => s.game().map_err(|e| e.to_string()),
            Err(e) => Err(e.to_string()),
        };
        match self.ctx.store().and_then(|s| s.list()) {
            Ok(listing) => {
                self.profiles = listing.profiles;
                self.unreadable = listing
                    .unreadable
                    .into_iter()
                    .map(|(id, e)| (id.to_string(), e.to_string()))
                    .collect();
            }
            Err(e) => self.notice = Some(Notice::Error(e.to_string())),
        }
        if !self.profiles.iter().any(|p| p.id == self.selected) {
            self.selected = ProfileId::default_id();
        }
        self.mod_counts = self
            .profiles
            .iter()
            .filter_map(|p| {
                let config = p.save_data_dir().join("Config").join("ModsConfig.xml");
                let n = rwpm_core::mods::read_active_mods(&config).ok()?.len();
                Some((p.id.clone(), n))
            })
            .collect();
        self.pack_cards = self
            .profiles
            .iter()
            .filter_map(|p| Some((p.id.clone(), pack_reset::PackCard::of(p)?)))
            .collect();
        self.reload_last_runs();
    }

    /// The profiles the search matches, in list order.
    pub fn shown(&self) -> impl Iterator<Item = &Profile> {
        let needle = self.search.trim().to_lowercase();
        self.profiles
            .iter()
            .filter(move |p| needle.is_empty() || p.name.to_lowercase().contains(&needle))
    }

    /// Why actions that change profiles or start the game must wait, if they must.
    pub fn blocked(&self) -> Option<String> {
        if let Some(job) = &self.job {
            Some(text::busy(&job.label))
        } else if self.game_running {
            Some(text::GAME_RUNNING.to_owned())
        } else {
            None
        }
    }

    /// The selected profile's game. Every per-profile game action goes through it.
    pub fn effective_game(&self) -> Result<EffectiveGame, GameError> {
        let profile = self.current().ok_or(GameError::NoGame)?;
        self.ctx.effective_game(profile, self.game.as_ref().ok())
    }

    /// The game whose log Open log / Copy log read. A pinned profile whose
    /// build cannot be used refuses; an unpinned one with no game found gets `None`
    /// (Unity's log path). Other errors keep their own reason (#15 N1).
    pub fn log_game(&self) -> Result<Option<EffectiveGame>, GameError> {
        match self.effective_game() {
            Ok(game) => Ok(Some(game)),
            Err(GameError::NoGame) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Why `p` cannot play right now, without touching the disk (the view calls it each
    /// frame). An unpinned profile needs the Steam install; a pinned one needs only its
    /// build, which health checks off the window thread.
    pub fn game_problem(&self, p: &Profile) -> Option<String> {
        match p.game_build.as_deref() {
            None => self
                .game
                .is_err()
                .then(|| text::TIP_PLAY_NO_GAME.to_owned()),
            Some(key) => self
                .health
                .get(&p.id)
                .is_some_and(|found| {
                    found
                        .iter()
                        .any(|f| matches!(f, rwpm_core::profile::health::Problem::BuildMissing(_)))
                })
                .then(|| text::build_missing(key)),
        }
    }

    pub fn current(&self) -> Option<&Profile> {
        self.profiles.iter().find(|p| p.id == self.selected)
    }

    fn title(&self) -> String {
        text::TITLE.to_owned()
    }

    fn theme(&self) -> iced::Theme {
        theme::theme(self.dark)
    }

    fn subscription(&self) -> iced::Subscription<message::Message> {
        use iced::keyboard::Event;
        let keys = iced::keyboard::listen().filter_map(|event| match event {
            Event::KeyPressed { key, modifiers, .. } => Some(message::Message::Key(key, modifiers)),
            _ => None,
        });
        iced::Subscription::batch([
            iced::system::theme_changes().map(message::Message::Theme),
            keys,
            iced::Subscription::run(watch::game_running),
            iced::Subscription::run(watch::sync_ticks),
            iced::Subscription::run(watch::update_ticks),
        ])
    }
}

/// Opens the window; returns when it closes. `backup` is a `.rimrim` file the OS opened:
/// the Restore dialog starts on it. First-run setup comes first and takes it over,
/// so the player restores from "Restore backup…" once setup is done.
/// A file the OS handed over when it was opened with rwpm: what the window opens at
/// start-up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenFile {
    /// A `.rimrim` backup: the Restore dialog.
    Backup(PathBuf),
    /// A `.rimpack` modpack: the Open modpack dialog.
    Pack(PathBuf),
}

pub fn run_with(open: Option<OpenFile>) -> iced::Result {
    let (ctx, note) = match Ctx::detect() {
        Ok(found) => found,
        Err(e) => {
            eprintln!("rwpm: {e}");
            std::process::exit(1);
        }
    };
    let start = move || {
        let mut app = App::new(ctx.clone());
        // Leftovers of an earlier update. Not in `App::new`, which tests call in a folder
        // this must never clean.
        rwpm_core::self_update::cleanup(&app.installer.kind);
        if let Some(note) = &note {
            app.notice = Some(Notice::Info(note.clone()));
        }
        app.other_profiles = paths::ignored_profiles();
        let mut opened = iced::Task::none();
        if app.setup.is_none() {
            match &open {
                Some(OpenFile::Backup(path)) => app.open_backup(path),
                Some(OpenFile::Pack(path)) => opened = app.open_pack(path),
                None => {}
            }
        }
        let health = app.check_health();
        // First-run setup comes first; a fresh install has checking off anyway.
        let updates = app.check_updates_if_due();
        (
            app,
            iced::Task::batch([
                iced::system::theme().map(message::Message::Theme),
                health,
                opened,
                updates,
            ]),
        )
    };
    let mut program = iced::application(start, App::update, App::view)
        .title(App::title)
        .theme(App::theme)
        .subscription(App::subscription)
        .default_font(icon::UI)
        .window(iced::window::Settings {
            size: iced::Size::new(960.0, 640.0),
            // The smallest window the layout is designed and tested for.
            min_size: Some(iced::Size::new(960.0, 600.0)),
            icon: icon::window_icon(),
            #[cfg(target_os = "linux")]
            platform_specific: iced::window::settings::PlatformSpecific {
                // Wayland matches this to `rwpm.desktop` for the taskbar icon.
                application_id: icon::APP_ID.to_owned(),
                ..Default::default()
            },
            ..iced::window::Settings::default()
        });
    for font in icon::fonts() {
        program = program.font(font);
    }
    program.run()
}

#[cfg(test)]
mod tests;
