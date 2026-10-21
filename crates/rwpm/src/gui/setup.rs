//! First-run setup: find RimWorld, then make the first profile, LIVE or OFFLINE
//! explained in plain words. Shown while `setup_done` is false on a fresh install;
//! installs that already have profiles or a chosen game folder skip it.

use std::path::{Path, PathBuf};

use iced::widget::{button, center, column, container, row, space, text, text_input};
use iced::{Alignment, Element, Length, Task};
use rwpm_core::discovery::{self, GameInstall};
use rwpm_core::profile::Mode;

use super::icon::{Icon, UI_BOLD, icon};
use super::message::Message;
use super::{App, Notice, text as t, theme};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Game,
    Profile,
}

#[derive(Debug)]
pub struct Setup {
    pub step: Step,
    /// The RimWorld folder typed or picked; empty means "as found through Steam".
    pub folder: String,
    /// What that folder (or Steam) gives.
    pub found: Result<GameInstall, String>,
    pub mode: Mode,
    pub name: String,
}

#[derive(Debug, Clone)]
pub enum SetupMsg {
    Folder(String),
    Browse,
    Picked(Option<PathBuf>),
    Continue,
    Mode(Mode),
    Name(String),
    Create,
    Skip,
}

impl Setup {
    pub fn new(found: &Result<GameInstall, String>) -> Self {
        Self {
            step: Step::Game,
            folder: String::new(),
            found: found.clone(),
            mode: Mode::Live,
            name: t::FIRST_PROFILE_NAME.to_owned(),
        }
    }
}

/// The system folder dialog; `None` when the player closes it.
pub async fn pick_folder(title: &'static str) -> Option<PathBuf> {
    rfd::AsyncFileDialog::new()
        .set_title(title)
        .pick_folder()
        .await
        .map(|f| f.path().to_path_buf())
}

impl App {
    /// Starts setup on a fresh install; marks it done on one that is already set up.
    pub(super) fn decide_setup(&mut self) {
        let Ok(mut settings) = self.ctx.settings() else {
            return;
        };
        if settings.setup_done {
            return;
        }
        let established = self.profiles.len() > 1 || settings.game_dir.is_some();
        if !established {
            self.setup = Some(Setup::new(&self.game));
            return;
        }
        settings.setup_done = true;
        if let Err(e) = settings.save(&self.ctx.app_dir) {
            self.notice = Some(Notice::Error(e.to_string()));
        }
    }

    pub(super) fn setup_update(&mut self, msg: SetupMsg) -> Task<Message> {
        let auto = self.game.clone();
        let Some(setup) = &mut self.setup else {
            return Task::none();
        };
        match msg {
            SetupMsg::Folder(folder) => {
                setup.found = check(&folder, &auto);
                setup.folder = folder;
            }
            SetupMsg::Browse => {
                return Task::perform(pick_folder(t::PICK_GAME_TITLE), |p| {
                    Message::Setup(SetupMsg::Picked(p))
                });
            }
            SetupMsg::Picked(Some(dir)) => {
                setup.folder = dir.display().to_string();
                setup.found = check(&setup.folder, &auto);
            }
            SetupMsg::Picked(None) => {}
            SetupMsg::Continue if setup.found.is_ok() => return self.setup_found_game(),
            SetupMsg::Continue => {}
            SetupMsg::Mode(mode) => setup.mode = mode,
            SetupMsg::Name(name) => setup.name = name,
            SetupMsg::Create if !setup.name.trim().is_empty() => return self.setup_create(),
            SetupMsg::Create => {}
            SetupMsg::Skip => self.finish_setup(),
        }
        Task::none()
    }

    /// Saves a typed or picked folder, then moves to the profile step.
    fn setup_found_game(&mut self) -> Task<Message> {
        let Some(setup) = &mut self.setup else {
            return Task::none();
        };
        let folder = setup.folder.trim().to_owned();
        setup.step = Step::Profile;
        if folder.is_empty() {
            return Task::none();
        }
        let saved = self.ctx.settings().and_then(|mut s| {
            s.game_dir = Some(PathBuf::from(&folder));
            s.save(&self.ctx.app_dir)
        });
        if let Err(e) = saved {
            self.notice = Some(Notice::Error(e.to_string()));
        }
        self.reload();
        Task::none()
    }

    fn setup_create(&mut self) -> Task<Message> {
        let Some(setup) = self.setup.as_ref() else {
            return Task::none();
        };
        let (name, mode) = (setup.name.trim().to_owned(), setup.mode);
        let created = self.ctx.store().and_then(|s| s.create(&name));
        self.finish_setup();
        match created {
            Ok(p) => {
                self.reload();
                // The first profile starts from the game's own settings and mod list.
                self.seed_from_default(&p);
                self.reload();
                self.selected = p.id;
                // Keep a copy error (e.g. RimWorld running) in view over "Created".
                if !matches!(self.notice, Some(Notice::Error(_))) {
                    self.notice = Some(Notice::Info(t::created(&p.name)));
                }
                if mode == Mode::Offline {
                    return self.update(Message::GoOffline);
                }
            }
            Err(e) => self.notice = Some(Notice::Error(e.to_string())),
        }
        Task::none()
    }

    fn finish_setup(&mut self) {
        self.setup = None;
        let saved = self.ctx.settings().and_then(|mut s| {
            s.setup_done = true;
            s.save(&self.ctx.app_dir)
        });
        if let Err(e) = saved {
            self.notice = Some(Notice::Error(e.to_string()));
        }
    }
}

/// What a typed folder gives; an empty one means "found through Steam".
fn check(folder: &str, auto: &Result<GameInstall, String>) -> Result<GameInstall, String> {
    let folder = folder.trim();
    if folder.is_empty() {
        return auto.clone();
    }
    discovery::from_game_dir(Path::new(folder)).map_err(|e| e.to_string())
}

pub fn view(setup: &Setup) -> Element<'_, Message> {
    let body = match setup.step {
        Step::Game => game_step(setup),
        Step::Profile => profile_step(setup),
    };
    center(
        container(body)
            .padding(32)
            .max_width(640)
            .style(theme::raised),
    )
    .padding(16)
    .into()
}

fn game_step(setup: &Setup) -> Element<'_, Message> {
    let status: Element<'_, Message> = match &setup.found {
        Ok(game) => row![
            icon(Icon::Ok).style(theme::success),
            text(t::found_game(&game.game_dir.display().to_string()))
        ]
        .spacing(8)
        .into(),
        Err(e) => row![
            icon(Icon::Warning).style(theme::error),
            // Steam's own error says little to a player; a typed folder's reason helps.
            text(if setup.folder.trim().is_empty() {
                t::SETUP_NOT_FOUND_AUTO.to_owned()
            } else {
                t::setup_not_found(e)
            })
            .style(theme::error)
        ]
        .spacing(8)
        .into(),
    };
    column![
        text(t::SETUP_STEP_1).size(13).style(theme::muted),
        text(t::SETUP_GAME_TITLE).size(24).font(UI_BOLD),
        text(t::SETUP_GAME_BODY),
        status,
        row![
            text_input(t::GAME_FOLDER_HINT, &setup.folder)
                .on_input(|v| Message::Setup(SetupMsg::Folder(v)))
                .padding(8),
            button(
                row![icon(Icon::Folder).size(14), text(t::CHOOSE_FOLDER)]
                    .spacing(8)
                    .align_y(Alignment::Center)
            )
            .padding([6, 12])
            .style(theme::secondary)
            .on_press(Message::Setup(SetupMsg::Browse))
        ]
        .spacing(10)
        .align_y(Alignment::Center),
        row![
            space::horizontal(),
            button(text(t::CONTINUE))
                .padding([8, 20])
                .style(theme::primary)
                .on_press_maybe(
                    setup
                        .found
                        .is_ok()
                        .then_some(Message::Setup(SetupMsg::Continue))
                )
        ]
    ]
    .spacing(16)
    .into()
}

fn profile_step(setup: &Setup) -> Element<'_, Message> {
    let choice = |mode: Mode, glyph: Icon, title: &'static str, about: &'static str| {
        button(
            row![
                icon(glyph).size(22).width(32),
                column![
                    text(title).font(UI_BOLD),
                    text(about).size(13).style(theme::muted)
                ]
                .spacing(2)
            ]
            .spacing(12)
            .align_y(Alignment::Center),
        )
        .width(Length::Fill)
        .padding(14)
        .style(theme::choice(setup.mode == mode))
        .on_press(Message::Setup(SetupMsg::Mode(mode)))
    };
    column![
        text(t::SETUP_STEP_2).size(13).style(theme::muted),
        text(t::SETUP_PROFILE_TITLE).size(24).font(UI_BOLD),
        text(t::SETUP_PROFILE_BODY),
        choice(Mode::Live, Icon::Live, t::CHOOSE_LIVE, t::CHOOSE_LIVE_ABOUT),
        choice(
            Mode::Offline,
            Icon::Offline,
            t::CHOOSE_OFFLINE,
            t::CHOOSE_OFFLINE_ABOUT
        ),
        column![
            text(t::NAME_HINT),
            text_input(t::NAME_HINT, &setup.name)
                .on_input(|v| Message::Setup(SetupMsg::Name(v)))
                .on_submit(Message::Setup(SetupMsg::Create))
                .padding(8)
        ]
        .spacing(4),
        row![
            button(text(t::SKIP_SETUP))
                .padding([8, 16])
                .style(theme::ghost)
                .on_press(Message::Setup(SetupMsg::Skip)),
            space::horizontal(),
            button(text(t::CREATE_PROFILE))
                .padding([8, 20])
                .style(theme::primary)
                .on_press_maybe(
                    (!setup.name.trim().is_empty()).then_some(Message::Setup(SetupMsg::Create))
                )
        ]
        .align_y(Alignment::Center)
    ]
    .spacing(16)
    .into()
}
