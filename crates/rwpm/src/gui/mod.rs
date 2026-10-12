//! The desktop app (I.gui): profile list, actions, progress. iced, Elm-style: all state
//! changes go through `update`, so they are tested without a window.

mod copy;
mod dialogs;
mod guide;
mod health;
mod icon;
mod jobs;
mod keys;
mod message;
mod mods;
mod setup;
mod support;
mod text;
mod theme;
mod update;
mod view;
mod watch;

use std::collections::HashMap;
use std::path::PathBuf;

use rwpm_core::discovery::GameInstall;
use rwpm_core::paths::Layout;
use rwpm_core::profile::{LastRun, OsTrash, Profile, ProfileId, ProfileStore};
use rwpm_core::settings::AppSettings;
use rwpm_core::{ProfileError, appimage, flatpak, paths};

pub use message::Dialog;

/// Paths every action needs; cheap to clone into a background job.
#[derive(Debug, Clone)]
pub struct Ctx {
    pub app_dir: PathBuf,
    pub game_save_dir: PathBuf,
    /// `rwpm` itself, for launching (`rwpm run`) and RimSort's run_args. With Flatpak
    /// Steam: the copy in `<app data>/bin`, which Steam's sandbox can see (V31). From an
    /// AppImage: the same copy, since the AppImage's own folder goes when it exits.
    pub exe: PathBuf,
    /// With Flatpak Steam: the `flatpak` program; Play then runs rwpm in Steam's sandbox.
    pub flatpak: Option<PathBuf>,
    /// The launcher copy failed: `exe` is a path Steam cannot use (Flatpak) or that goes
    /// when this app exits (AppImage), so nothing may save it (V38).
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
}

#[derive(Debug)]
pub struct Job {
    pub label: String,
    pub done: u64,
    pub total: u64,
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
    pub notice: Option<Notice>,
    /// The OS asks for dark colours.
    pub dark: bool,
    /// Profile search text.
    pub search: String,
    pub menu_open: bool,
    /// Some RimWorld is running: actions that clash with it wait (V33).
    pub game_running: bool,
    /// Turned-on mods per profile; absent when the mod list cannot be read.
    pub mod_counts: HashMap<ProfileId, usize>,
    /// Each profile's last `rwpm run` attempt (V32), if any.
    pub last_runs: HashMap<ProfileId, LastRun>,
    /// First-run setup, while it is shown (T27).
    pub setup: Option<setup::Setup>,
    /// The drill-in shown instead of the detail sections, if any.
    pub drill: Option<message::Drill>,
    /// The last mod list read (T29) and its search text.
    pub mods: Option<mods::ModList>,
    pub mod_search: String,
    /// Health problems per profile, for the profiles checked so far (T30).
    pub health: HashMap<ProfileId, Vec<rwpm_core::profile::health::Problem>>,
    /// The other layout's profiles folder, when it has profiles this app ignores (V31).
    pub other_profiles: Option<PathBuf>,
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
            notice: None,
            dark: false,
            search: String::new(),
            menu_open: false,
            game_running: false,
            mod_counts: HashMap::new(),
            last_runs: HashMap::new(),
            setup: None,
            drill: None,
            mods: None,
            mod_search: String::new(),
            health: HashMap::new(),
            other_profiles: None,
        };
        app.reload();
        app.decide_setup();
        // B2: the last played profile is ready to Play with one click.
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
        ])
    }
}

/// Opens the window; returns when it closes.
pub fn run() -> iced::Result {
    let (ctx, note) = match Ctx::detect() {
        Ok(found) => found,
        Err(e) => {
            eprintln!("rwpm: {e}");
            std::process::exit(1);
        }
    };
    let start = move || {
        let mut app = App::new(ctx.clone());
        if let Some(note) = &note {
            app.notice = Some(Notice::Info(note.clone()));
        }
        app.other_profiles = paths::ignored_profiles();
        let health = app.check_health();
        (
            app,
            iced::Task::batch([iced::system::theme().map(message::Message::Theme), health]),
        )
    };
    let mut program = iced::application(start, App::update, App::view)
        .title(App::title)
        .theme(App::theme)
        .subscription(App::subscription)
        .default_font(icon::UI)
        .window(iced::window::Settings {
            size: iced::Size::new(960.0, 640.0),
            // V33: the smallest window the layout is designed and tested for.
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
