//! `App::update`: every user action and job result, in one place.

use std::path::PathBuf;

use iced::Task;
use rwpm_core::consent::{ConsentStore, Scope};
use rwpm_core::launch::Os;
use rwpm_core::profile::guard::{self, GuardReport};
use rwpm_core::profile::offline::{self, OfflinePlan};
use rwpm_core::profile::relocate::{self, MovePlan};
use rwpm_core::profile::{Mode, ProcessProbe, ProfileId};
use rwpm_core::{rimsort, space, winhttp};

use super::message::{Ask, Dialog, Field, GuardChoice, Message, NameFor, Outcome};
use super::{App, Job, Notice, jobs, text as t};

impl App {
    pub fn update(&mut self, message: Message) -> Task<Message> {
        // T30: re-check health when the selection changes, a job ends or a game starts or stops.
        let before = self.selected.clone();
        let recheck = matches!(message, Message::Done(_) | Message::GameRunning(_));
        let task = self.handle(message);
        if recheck || self.selected != before {
            return Task::batch([task, self.check_health()]);
        }
        task
    }

    fn handle(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Theme(mode) => {
                self.dark = mode == iced::theme::Mode::Dark;
                return Task::none();
            }
            Message::GameRunning(running) => {
                // A game started or ended: its run record changed.
                self.game_running = running;
                self.reload_last_runs();
                return Task::none();
            }
            Message::Key(key, modifiers) => return self.key(key, modifiers),
            Message::Setup(msg) => return self.setup_update(msg),
            Message::Progress(done) => {
                if let Some(job) = &mut self.job {
                    job.done = done;
                }
                return Task::none();
            }
            Message::Done(result) => return self.finished(result),
            _ => {}
        }
        if !matches!(message, Message::ToggleMenu) {
            self.menu_open = false;
        }
        // V33: while a job runs or the game is open, actions that clash with it wait.
        if self.clashes(&message)
            && let Some(reason) = self.blocked()
        {
            self.notice = Some(Notice::Info(reason));
            return Task::none();
        }
        self.act(message)
    }

    /// Must `message` wait for the running job or game (V33)?
    fn clashes(&self, message: &Message) -> bool {
        use Message as M;
        let confirm_in = |acting: fn(&Dialog) -> bool| {
            matches!(message, M::Confirm) && self.dialog.as_ref().is_some_and(acting)
        };
        if self.job.is_some() {
            // One job at a time: anything that changes profiles or starts the game waits.
            return matches!(
                message,
                M::New
                    | M::Clone
                    | M::Rename
                    | M::Delete
                    | M::GoOffline
                    | M::Refresh
                    | M::GoLive
                    | M::Launch
                    | M::Export
                    | M::RemoveLoader
                    | M::Guard(_)
                    | M::FreezeUnfrozen
                    | M::InstallLoader
            ) || confirm_in(|d| !matches!(d, Dialog::Help));
        }
        // A running game: starting another, or changing mods and game files, waits.
        self.game_running
            && (matches!(
                message,
                M::Launch
                    | M::GoOffline
                    | M::Refresh
                    | M::GoLive
                    | M::Delete
                    | M::RemoveLoader
                    | M::Guard(_)
                    | M::FreezeUnfrozen
                    | M::InstallLoader
            ) || confirm_in(|d| {
                matches!(
                    d,
                    Dialog::ConfirmDelete
                        | Dialog::CopySettings(_)
                        | Dialog::Offline { .. }
                        | Dialog::Settings { .. }
                        | Dialog::ConfirmMove(_)
                )
            }))
    }

    fn act(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Select(id) => {
                self.selected = id;
                self.drill = None;
                if !matches!(self.notice, Some(Notice::Error(_))) {
                    self.notice = None;
                }
            }
            Message::New => self.ask_name(NameFor::New, String::new()),
            Message::Clone => {
                let name = self
                    .current()
                    .map(|p| t::copy_name(&p.name))
                    .unwrap_or_default();
                self.ask_name(NameFor::Clone, name);
            }
            Message::Rename => {
                if let Some(p) = self.current().filter(|p| !p.is_default()) {
                    let name = p.name.clone();
                    self.ask_name(NameFor::Rename, name);
                }
            }
            Message::Delete => {
                if self.current().is_some_and(|p| !p.is_default()) {
                    self.dialog = Some(Dialog::ConfirmDelete);
                }
            }
            Message::GoOffline => return self.plan_offline(false),
            Message::Refresh => return self.plan_offline(true),
            Message::GoLive => self.go_live(),
            Message::Launch => return self.launch(),
            Message::Export => return self.export(),
            Message::RemoveLoader => self.remove_loader(),
            Message::OpenSettings => self.open_settings(),
            Message::OpenLog => self.open_log(),
            Message::OpenSaves => self.open_saves(),
            Message::CopyLog => return self.copy_log(),
            Message::CopyLaunchOptions => return self.copy_launch_options(),
            Message::Help => self.dialog = Some(Dialog::Help),
            Message::Search(value) => self.search = value,
            Message::SearchSubmit => {
                let first = self.shown().next().map(|p| p.id.clone());
                if let Some(id) = first {
                    self.selected = id;
                }
            }
            Message::ToggleMenu => self.menu_open = !self.menu_open,
            Message::CopyFrom(from) => self.edit_copy(|c| c.from = from),
            Message::ToggleGroup(group, on) => self.edit_copy(|c| {
                c.groups.retain(|g| *g != group);
                if on {
                    c.groups.push(group);
                }
            }),
            Message::OpenCopySettings => self.open_copy_settings(),
            Message::HealthRead(id, problems) => {
                self.health.insert(id, problems);
            }
            Message::FreezeUnfrozen => return self.freeze_unfrozen(),
            Message::InstallLoader => self.ask_loader(),
            Message::OpenOtherProfiles => {
                if let Some(dir) = self.other_profiles.clone()
                    && let Err(e) = super::support::open_path(&dir)
                {
                    self.notice = Some(Notice::Error(t::open_folder_failed(&e.to_string())));
                }
            }
            Message::Drill(drill) => {
                self.drill = drill;
                if drill == Some(super::message::Drill::Mods) {
                    return self.read_mods();
                }
            }
            Message::ModsRead(list) => self.mods = Some(list),
            Message::ModSearch(value) => self.mod_search = value,
            Message::Browse(field) => {
                return Task::perform(super::setup::pick_folder(t::PICK_FOLDER_TITLE), move |p| {
                    Message::Picked(field, p)
                });
            }
            Message::Picked(field, Some(dir)) => self.input(field, dir.display().to_string()),
            Message::Picked(_, None) => {}
            Message::Input(field, value) => self.input(field, value),
            Message::Choose(id, dir) => {
                if let Some(Dialog::Offline { choices, .. }) = &mut self.dialog {
                    choices.retain(|(c, _)| *c != id);
                    choices.push((id, dir));
                }
            }
            Message::Guard(choice) => return self.guard_answer(choice),
            Message::Confirm => return self.confirm(),
            Message::Cancel => self.dialog = None,
            Message::Theme(_)
            | Message::GameRunning(_)
            | Message::Setup(_)
            | Message::Key(..)
            | Message::Progress(_)
            | Message::Done(_) => {}
        }
        Task::none()
    }

    fn ask_name(&mut self, purpose: NameFor, value: String) {
        // T33: a new profile starts from Default's settings unless the player changes it.
        let copy = (purpose == NameFor::New)
            .then(|| self.copy_choice(Some(rwpm_core::profile::ProfileId::default_id())));
        self.dialog = Some(Dialog::Name {
            purpose,
            value,
            copy,
        });
    }

    fn input(&mut self, field: Field, value: String) {
        match (&mut self.dialog, field) {
            (Some(Dialog::Name { value: v, .. }), Field::Name) => *v = value,
            (
                Some(Dialog::Settings {
                    game, game_check, ..
                }),
                Field::GameDir,
            ) => {
                *game_check = check_game_folder(&value);
                *game = value;
            }
            (Some(Dialog::Settings { steam, .. }), Field::SteamDir) => *steam = value,
            (Some(Dialog::Settings { move_to, .. }), Field::MoveTo) => *move_to = value,
            _ => {}
        }
    }

    /// Starts a background job with a progress bar.
    pub(super) fn start<F>(&mut self, label: String, total: u64, work: F) -> Task<Message>
    where
        F: FnOnce(&mut dyn FnMut(u64)) -> Result<Outcome, String> + Send + 'static,
    {
        self.dialog = None;
        self.notice = None;
        self.job = Some(Job {
            label,
            done: 0,
            total,
        });
        jobs::spawn(work)
    }

    fn finished(&mut self, result: Result<Outcome, String>) -> Task<Message> {
        self.job = None;
        match result {
            Ok(Outcome::Changed(line)) => {
                self.reload();
                self.notice = Some(Notice::Info(line));
            }
            // Nothing to ask about: earlier copies are reused, or there is nothing to copy (V36).
            Ok(Outcome::OfflinePlanned(plan))
                if plan.reuse
                    || (plan.copies.is_empty()
                        && plan.conflicts.is_empty()
                        && plan.missing.is_empty()) =>
            {
                return self.apply_offline(plan, Vec::new());
            }
            Ok(Outcome::OfflinePlanned(plan)) => {
                self.dialog = Some(Dialog::Offline {
                    plan,
                    choices: Vec::new(),
                });
            }
            Ok(Outcome::MovePlanned(plan)) => self.dialog = Some(Dialog::ConfirmMove(plan)),
            Ok(Outcome::ReadyToPlay(id)) => {
                self.reload();
                self.selected = id;
                return self.loader_then_play();
            }
            Err(e) => self.notice = Some(Notice::Error(e)),
        }
        Task::none()
    }

    fn confirm(&mut self) -> Task<Message> {
        match self.dialog.take() {
            Some(Dialog::Name {
                purpose,
                value,
                copy,
            }) => return self.named(purpose, value, copy),
            Some(Dialog::CopySettings(copy)) => {
                let Some(to) = self.current().filter(|p| !p.is_default()).cloned() else {
                    return Task::none();
                };
                let done = t::settings_copied(&to.name);
                return self.copy_job(to, copy, done, Some(t::NOTHING_TO_COPY));
            }
            Some(Dialog::ConfirmDelete) => self.delete(),
            Some(Dialog::Offline { plan, choices }) => return self.apply_offline(plan, choices),
            Some(Dialog::Consent(ask)) => return self.consented(ask),
            Some(Dialog::Settings {
                game,
                steam,
                move_to,
                ..
            }) => return self.save_settings(game, steam, move_to),
            Some(Dialog::ConfirmMove(plan)) => return self.apply_move(plan),
            Some(dialog @ Dialog::Guard(_)) => self.dialog = Some(dialog),
            Some(Dialog::Help) => {}
            None => {}
        }
        Task::none()
    }

    fn named(
        &mut self,
        purpose: NameFor,
        name: String,
        copy: Option<super::copy::SettingsCopy>,
    ) -> Task<Message> {
        let store = match self.ctx.store() {
            Ok(s) => s,
            Err(e) => return self.fail(e),
        };
        match purpose {
            NameFor::New => match store.create(&name) {
                Ok(p) => {
                    self.changed(Some(p.id.clone()), t::created(&p.name));
                    if let Some(copy) = copy.filter(|c| c.from.is_some() && !c.groups.is_empty()) {
                        let done = t::created_with_settings(&p.name);
                        return self.copy_job(p, copy, done, None);
                    }
                }
                Err(e) => return self.fail(e),
            },
            NameFor::Rename => match store.rename(&self.selected, &name) {
                Ok(p) => self.changed(None, t::renamed(&p.name)),
                Err(e) => return self.fail(e),
            },
            NameFor::Clone => {
                let Some(source) = self.current().cloned() else {
                    return Task::none();
                };
                let total = space::dir_size(&source.save_data_dir())
                    + source.mods_dir().map_or(0, |m| space::dir_size(&m));
                let ctx = self.ctx.clone();
                return self.start(t::copying(&source.name), total, move |progress| {
                    let p = ctx
                        .store()
                        .and_then(|s| s.clone_profile(&source.id, &name, &ProcessProbe, progress))
                        .map_err(|e| e.to_string())?;
                    Ok(Outcome::Changed(t::copied(&p.name)))
                });
            }
        }
        Task::none()
    }

    fn delete(&mut self) {
        let id = self.selected.clone();
        match self.ctx.store().and_then(|s| s.delete(&id, &ProcessProbe)) {
            Ok(()) => self.changed(
                Some(rwpm_core::profile::ProfileId::default_id()),
                t::DELETED.into(),
            ),
            Err(e) => self.notice = Some(Notice::Error(e.to_string())),
        }
    }

    fn plan_offline(&mut self, refresh: bool) -> Task<Message> {
        let Ok(game) = self.game.clone() else {
            return self.no_game();
        };
        let (ctx, id) = (self.ctx.clone(), self.selected.clone());
        self.start(t::JOB_CHECKING.into(), 0, move |_| {
            let store = ctx.store().map_err(|e| e.to_string())?;
            offline::plan_offline(&store, &id, &game, refresh)
                .map(Outcome::OfflinePlanned)
                .map_err(|e| e.to_string())
        })
    }

    fn apply_offline(
        &mut self,
        plan: OfflinePlan,
        choices: Vec<(String, PathBuf)>,
    ) -> Task<Message> {
        let ctx = self.ctx.clone();
        let total = plan.bytes_needed;
        self.start(t::JOB_FREEZING.into(), total, move |progress| {
            let store = ctx.store().map_err(|e| e.to_string())?;
            let p = offline::apply_offline(&store, &plan, &choices, &ProcessProbe, progress)
                .map_err(|e| e.to_string())?;
            Ok(Outcome::Changed(t::now_offline(&p.name)))
        })
    }

    fn go_live(&mut self) {
        let id = self.selected.clone();
        match self
            .ctx
            .store()
            .and_then(|s| offline::go_live(&s, &id, &ProcessProbe))
        {
            Ok(p) => self.changed(None, t::now_live(&p.name)),
            Err(e) => self.notice = Some(Notice::Error(e.to_string())),
        }
    }

    /// Play: the OFFLINE guard (V5) first, then the Windows loader, then start.
    fn launch(&mut self) -> Task<Message> {
        let Ok(game) = self.game.clone() else {
            return self.no_game();
        };
        if self.current().is_some_and(|p| p.mode == Mode::Offline) {
            let report = self
                .ctx
                .store()
                .and_then(|s| guard::check_offline(&s, &self.selected, &game));
            match report {
                Ok(r) if !r.is_clear() => {
                    self.dialog = Some(Dialog::Guard(r));
                    return Task::none();
                }
                Ok(_) => {}
                Err(e) => return self.fail(e),
            }
        }
        self.loader_then_play()
    }

    fn guard_answer(&mut self, choice: GuardChoice) -> Task<Message> {
        let Some(Dialog::Guard(report)) = self.dialog.take() else {
            return Task::none();
        };
        let (ctx, id) = (self.ctx.clone(), self.selected.clone());
        match choice {
            GuardChoice::PlayAnyway => self.loader_then_play(),
            GuardChoice::TurnOff => {
                let ids: Vec<String> = report
                    .unfrozen
                    .iter()
                    .map(|u| u.active_id.clone())
                    .collect();
                match ctx.store().and_then(|s| guard::turn_off(&s, &id, &ids)) {
                    Ok(()) => self.loader_then_play(),
                    Err(e) => self.fail(e),
                }
            }
            GuardChoice::Add => self.start(
                t::JOB_FREEZING_NEW.into(),
                total_of(&report),
                move |progress| {
                    let store = ctx.store().map_err(|e| e.to_string())?;
                    guard::add_to_profile(&store, &id, &report.unfrozen, &ProcessProbe, progress)
                        .map_err(|e| e.to_string())?;
                    Ok(Outcome::ReadyToPlay(id))
                },
            ),
        }
    }

    /// Windows OFFLINE needs the loader beside the game (T12); ask before installing it.
    fn loader_then_play(&mut self) -> Task<Message> {
        let Ok(game) = self.game.clone() else {
            return self.no_game();
        };
        let offline = self.current().is_some_and(|p| p.mode == Mode::Offline);
        if offline && Os::current() == Os::Windows {
            match winhttp::launch_check(&game.game_dir, &self.ctx.app_dir) {
                Ok(_) => {}
                Err(winhttp::WinhttpError::NotInstalled) => {
                    self.dialog = Some(Dialog::Consent(Ask::Loader { then_play: true }));
                    return Task::none();
                }
                Err(e) => {
                    self.notice = Some(Notice::Error(e.to_string()));
                    return Task::none();
                }
            }
        }
        self.play(&game.exe);
        Task::none()
    }

    /// Starts `rwpm run <id> -- <game>`; the guard already ran here, so it is told so.
    /// With Flatpak Steam it runs inside Steam's sandbox, or the game cannot reach Steam (V31).
    fn play(&mut self, game_exe: &std::path::Path) {
        // The guard already ran here; and the run is recorded as started by the app (V32).
        let env = [
            (crate::guard::CHOICE_ENV, "launch"),
            (rwpm_core::profile::last_run::STARTED_BY_ENV, "app"),
        ];
        let mut cmd = match &self.ctx.flatpak {
            Some(flatpak) => rwpm_core::flatpak::play_command(
                flatpak,
                &self.ctx.exe,
                self.selected.as_str(),
                game_exe,
                &env,
            ),
            None => {
                let mut cmd = std::process::Command::new(&self.ctx.exe);
                cmd.args(["run", self.selected.as_str(), "--"])
                    .arg(game_exe)
                    .envs(env);
                cmd
            }
        };
        let spawned = cmd.spawn();
        self.notice = Some(match spawned {
            Ok(mut child) => {
                // Reap it when it ends so it never lingers as a zombie.
                std::thread::spawn(move || child.wait());
                Notice::Info(t::STARTING.into())
            }
            Err(e) => Notice::Error(t::start_failed(&e.to_string())),
        });
    }

    fn export(&mut self) -> Task<Message> {
        let Some(settings) = rimsort::settings_path() else {
            return self.fail(t::NO_RIMSORT);
        };
        match ConsentStore::open(&self.ctx.app_dir).find(Scope::RimsortSettings, &settings) {
            Ok(Some(_)) => self.export_job(settings),
            Ok(None) => {
                self.dialog = Some(Dialog::Consent(Ask::RimSort));
                Task::none()
            }
            Err(e) => self.fail(e),
        }
    }

    pub(super) fn export_job(&mut self, settings: PathBuf) -> Task<Message> {
        if self.ctx.launcher_failed {
            return self.fail(t::NO_LAUNCHER);
        }
        let Ok(game) = self.game.clone() else {
            return self.no_game();
        };
        let Some(profile) = self.current().cloned() else {
            return Task::none();
        };
        let ctx = self.ctx.clone();
        self.start(t::JOB_RIMSORT.into(), 0, move |_| {
            let plan = rimsort::plan_export(&profile, &game, &ctx.exe, &settings)
                .map_err(|e| e.to_string())?;
            let consent = ConsentStore::open(&ctx.app_dir)
                .find(Scope::RimsortSettings, &settings)
                .map_err(|e| e.to_string())?
                .ok_or(t::NO_RIMSORT_CONSENT)?;
            let backups = ctx.app_dir.join("backups").join("rimsort");
            rimsort::apply_export(&plan, &consent, &backups, &rimsort::ProcessRimSortProbe)
                .map_err(|e| e.to_string())?;
            Ok(Outcome::Changed(t::added_to_rimsort(&plan.instance_name)))
        })
    }

    fn consented(&mut self, ask: Ask) -> Task<Message> {
        let store = ConsentStore::open(&self.ctx.app_dir);
        match ask {
            Ask::RimSort => {
                let Some(settings) = rimsort::settings_path() else {
                    return self.fail(t::NO_RIMSORT);
                };
                match store.record(Scope::RimsortSettings, &settings) {
                    Ok(_) => self.export_job(settings),
                    Err(e) => self.fail(e),
                }
            }
            Ask::Loader { then_play } => {
                let Ok(game) = self.game.clone() else {
                    return self.no_game();
                };
                let dll = game.game_dir.join(winhttp::DLL);
                let consent = match store.record(Scope::WinhttpDll, &dll) {
                    Ok(c) => c,
                    Err(e) => return self.fail(e),
                };
                let (ctx, id) = (self.ctx.clone(), self.selected.clone());
                self.start(t::JOB_LOADER.into(), 0, move |_| {
                    let vendor = ctx
                        .exe
                        .parent()
                        .unwrap_or(&ctx.app_dir)
                        .join("doorstop")
                        .join(winhttp::DLL);
                    winhttp::install(&game.game_dir, &ctx.app_dir, &vendor, &consent)
                        .map_err(|e| e.to_string())?;
                    Ok(if then_play {
                        Outcome::ReadyToPlay(id)
                    } else {
                        Outcome::Changed(t::LOADER_INSTALLED.to_owned())
                    })
                })
            }
        }
    }

    fn remove_loader(&mut self) {
        let Ok(game) = self.game.clone() else {
            let _ = self.no_game();
            return;
        };
        self.notice = Some(match winhttp::remove(&game.game_dir, &self.ctx.app_dir) {
            Ok(()) => Notice::Info(t::LOADER_REMOVED.into()),
            Err(e) => Notice::Error(e.to_string()),
        });
    }

    fn open_log(&mut self) {
        let engine = super::support::engine(self.game.as_ref().ok());
        let Some(log) = self.current().map(|p| p.log_path(engine)) else {
            return;
        };
        self.notice = Some(if !log.is_file() {
            Notice::Info(t::NO_LOG.into())
        } else {
            match super::support::open_path(&log) {
                Ok(()) => Notice::Info(t::opened(&log)),
                Err(e) => Notice::Error(t::open_failed(&e.to_string())),
            }
        });
    }

    fn open_saves(&mut self) {
        let Some(dir) = self.current().map(|p| p.save_data_dir()) else {
            return;
        };
        self.notice = Some(match super::support::open_path(&dir) {
            Ok(()) => Notice::Info(t::opened(&dir)),
            Err(e) => Notice::Error(t::open_folder_failed(&e.to_string())),
        });
    }

    fn copy_log(&mut self) -> Task<Message> {
        let Some(profile) = self.current() else {
            return Task::none();
        };
        let report = super::support::support_text(profile, self.game.as_ref().ok());
        self.notice = Some(Notice::Info(t::LOG_COPIED.into()));
        iced::clipboard::write(report)
    }

    /// The line players paste into Steam → RimWorld → Properties → Launch options.
    pub fn launch_options(&self) -> Option<String> {
        rimsort::launch_options(&self.ctx.exe, self.selected.as_str(), cfg!(windows)).ok()
    }

    fn copy_launch_options(&mut self) -> Task<Message> {
        if self.ctx.launcher_failed {
            return self.fail(t::NO_LAUNCHER);
        }
        let Some(line) = self.launch_options() else {
            return self.fail(t::BAD_LAUNCH_FOLDER);
        };
        self.notice = Some(Notice::Info(t::STEAM_COPIED.into()));
        iced::clipboard::write(line)
    }

    fn open_settings(&mut self) {
        let settings = self.ctx.settings().unwrap_or_default();
        let show = |p: Option<PathBuf>| p.map(|p| p.display().to_string()).unwrap_or_default();
        let game = show(settings.game_dir);
        self.dialog = Some(Dialog::Settings {
            game_check: check_game_folder(&game),
            game,
            steam: show(settings.steam_dir),
            move_to: String::new(),
        });
    }

    fn save_settings(&mut self, game: String, steam: String, move_to: String) -> Task<Message> {
        let mut settings = match self.ctx.settings() {
            Ok(s) => s,
            Err(e) => return self.fail(e),
        };
        let path = |s: &str| (!s.trim().is_empty()).then(|| PathBuf::from(s.trim()));
        settings.game_dir = path(&game);
        settings.steam_dir = path(&steam);
        if let Err(e) = settings.save(&self.ctx.app_dir) {
            return self.fail(e);
        }
        self.reload();
        let Some(target) = path(&move_to) else {
            self.notice = Some(Notice::Info(t::SETTINGS_SAVED.into()));
            return Task::none();
        };
        let ctx = self.ctx.clone();
        self.start(t::JOB_MEASURING.into(), 0, move |_| {
            let store = ctx.store().map_err(|e| e.to_string())?;
            relocate::plan_move(&store, &target)
                .map(Outcome::MovePlanned)
                .map_err(|e| e.to_string())
        })
    }

    fn apply_move(&mut self, plan: MovePlan) -> Task<Message> {
        let ctx = self.ctx.clone();
        let total = plan.bytes;
        self.start(t::JOB_MOVING.into(), total, move |progress| {
            let store = ctx.store().map_err(|e| e.to_string())?;
            let report = relocate::apply_move(&store, &ctx.app_dir, &plan, &ProcessProbe, progress)
                .map_err(|e| e.to_string())?;
            let stale = rimsort::settings_path()
                .map(|s| relocate::stale_rimsort(&s, &plan.old_root, &plan.profiles))
                .unwrap_or_default();
            Ok(Outcome::Changed(t::moved(&report.new_root, &stale)))
        })
    }

    fn changed(&mut self, select: Option<ProfileId>, line: String) {
        self.reload();
        if let Some(id) = select {
            self.selected = id;
        }
        self.notice = Some(Notice::Info(line));
    }

    pub(super) fn fail(&mut self, e: impl std::fmt::Display) -> Task<Message> {
        self.notice = Some(Notice::Error(e.to_string()));
        Task::none()
    }

    fn no_game(&mut self) -> Task<Message> {
        self.fail(t::NO_GAME)
    }
}

/// M8: a typed RimWorld folder, checked as it changes; `None` while empty.
fn check_game_folder(folder: &str) -> Option<Result<(), String>> {
    let folder = folder.trim();
    (!folder.is_empty()).then(|| {
        rwpm_core::discovery::from_game_dir(std::path::Path::new(folder))
            .map(|_| ())
            .map_err(|e| e.to_string())
    })
}

pub(super) fn total_of(report: &GuardReport) -> u64 {
    report
        .unfrozen
        .iter()
        .map(|u| match &u.fix {
            guard::Fix::Copy(c) => c.bytes,
            guard::Fix::UseFrozen(_) => 0,
        })
        .sum()
}
