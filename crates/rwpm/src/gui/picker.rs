//! The profile's game version: a row in the detail pane and a picker. Default cannot
//! be pinned. A pin that goes to an older version asks first when there are saves.
//! The core takes `run.lock`, so a pin never races Delete build or a capture.

use iced::widget::{Column, button, column, row, space, text};
use iced::{Alignment, Element, Length, Task};
use rwpm_core::builds::{BuildKey, BuildStore, pin, pin_warns};
use rwpm_core::profile::{ProcessProbe, Profile};

use super::icon::{Icon, UI_BOLD, icon};
use super::message::{Dialog, Message};
use super::view::{action, section_title};
use super::{App, Notice, text as t, theme};

impl App {
    /// "Game version: …" with Change… and Manage…; a caption for Default.
    pub(super) fn game_version_section<'a>(&'a self, p: &'a Profile) -> Element<'a, Message> {
        let title = section_title(t::GAME_VERSION);
        if p.is_default() {
            return column![
                title,
                text(t::DEFAULT_GAME_VERSION).size(14).style(theme::muted)
            ]
            .spacing(8)
            .into();
        }
        let label = match p.game_build.as_deref() {
            Some(key) => key.to_owned(),
            None => t::steam_install_label(self.live_version()),
        };
        let wait = self.blocked().is_some();
        let change = action(
            Icon::Rename,
            t::CHANGE,
            (!wait).then_some(Message::ChangeGameVersion),
            theme::secondary,
        );
        let manage = action(
            Icon::Folder,
            t::MANAGE,
            Some(Message::Drill(Some(super::message::Drill::GameVersions))),
            theme::ghost,
        );
        column![
            title,
            row![
                text(t::game_version_is(&label)).font(UI_BOLD),
                space::horizontal(),
                change,
                manage
            ]
            .spacing(10)
            .align_y(Alignment::Center)
        ]
        .spacing(8)
        .into()
    }

    fn live_version(&self) -> Option<&str> {
        self.game.as_ref().ok().and_then(|g| g.version.as_deref())
    }

    /// Reads the stored versions, then opens the picker (`PickerRead`).
    pub(super) fn change_game_version(&mut self) -> Task<Message> {
        if self.current().is_none_or(Profile::is_default) {
            return Task::none();
        }
        let ctx = self.ctx.clone();
        Task::perform(async move { ctx.read_builds() }, Message::PickerRead)
    }

    pub(super) fn picker_read(&mut self, view: super::builds::BuildsView) {
        let Some(p) = self.current().filter(|p| !p.is_default()) else {
            return;
        };
        let builds = view
            .rows
            .iter()
            .filter(|r| r.problem.is_none())
            .map(|r| {
                (
                    r.key.clone(),
                    t::stored_version_label(&r.key, r.waylith, r.size),
                )
            })
            .collect();
        self.dialog = Some(Dialog::PickVersion {
            steam: t::steam_install_label(self.live_version()),
            builds,
            chosen: p.game_build.clone(),
        });
        self.builds = Some(view);
    }

    pub(super) fn pick_build(&mut self, key: Option<String>) {
        if let Some(Dialog::PickVersion { chosen, .. }) = &mut self.dialog {
            *chosen = key;
        }
    }

    /// Picker confirmed: no change, a pin, or the downgrade question first.
    pub(super) fn confirm_pick(&mut self, to: Option<String>) -> Task<Message> {
        let Some(p) = self.current().filter(|p| !p.is_default()).cloned() else {
            return Task::none();
        };
        if p.game_build == to {
            return Task::none();
        }
        let from = self.effective_game().ok().and_then(|g| g.version.clone());
        let into = match &to {
            Some(key) => self.stored_version(key),
            None => self.live_version().map(ToOwned::to_owned),
        };
        let saves = p.save_data_dir().join("Saves");
        if pin_warns(from.as_deref(), into.as_deref(), &saves) {
            self.dialog = Some(Dialog::ConfirmDowngrade {
                to,
                from: from.unwrap_or_default(),
                into: into.unwrap_or_default(),
            });
            return Task::none();
        }
        self.apply_pin(&p, to)
    }

    /// The raw `Version.txt` text of a stored version, if it can be read.
    fn stored_version(&self, key: &str) -> Option<String> {
        let key = BuildKey::parse(key).ok()?;
        let store: BuildStore = self.ctx.build_store().ok()?;
        store.get(&key).ok().map(|b| b.file.version)
    }

    pub(super) fn apply_pin(&mut self, p: &Profile, to: Option<String>) -> Task<Message> {
        let store = match self.ctx.store() {
            Ok(s) => s,
            Err(e) => return self.fail(e),
        };
        match pin(
            &store,
            &self.ctx.app_dir,
            &p.id,
            to.as_deref(),
            &ProcessProbe,
        ) {
            Ok(updated) => {
                let label = match updated.game_build.as_deref() {
                    Some(key) => key.to_owned(),
                    None => t::steam_install_label(self.live_version()),
                };
                self.reload();
                self.health.remove(&p.id);
                self.mods = None;
                self.notice = Some(Notice::Info(t::game_version_changed(&p.name, &label)));
                self.check_health()
            }
            Err(e) => self.fail(e),
        }
    }
}

/// The picker: Steam install, then each stored version, as one choice. A button per
/// choice (the chosen one marked), so each can be found and clicked by its words.
pub fn picker_view<'a>(
    steam: &'a str,
    builds: &'a [(String, String)],
    chosen: Option<&'a str>,
) -> Element<'a, Message> {
    let choice = |label: &'a str, key: Option<&'a str>| {
        let on = chosen == key;
        button(
            row![
                icon(if on { Icon::Ok } else { Icon::Forward }).size(14),
                text(label)
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        )
        .padding([8, 12])
        .width(Length::Fill)
        .style(if on { theme::primary } else { theme::secondary })
        .on_press(Message::PickBuild(key.map(ToOwned::to_owned)))
    };
    let mut list = Column::new().spacing(8).push(choice(steam, None));
    for (key, label) in builds {
        list = list.push(choice(label, Some(key.as_str())));
    }
    column![
        text(t::PICK_TITLE).size(18).font(UI_BOLD),
        text(t::PICK_ABOUT).size(14).style(theme::muted),
        list,
        row![
            space::horizontal(),
            button(t::CANCEL)
                .on_press(Message::Cancel)
                .style(theme::secondary),
            button(t::USE_VERSION)
                .on_press(Message::Confirm)
                .style(theme::primary)
        ]
        .spacing(10)
    ]
    .spacing(16)
    .into()
}
