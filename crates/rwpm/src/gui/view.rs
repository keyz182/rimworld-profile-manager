//! `App::view`: searchable profile list on the left; on the right the
//! chosen profile with Play pinned under its name, then its mods and how to start it from
//! Steam or RimSort. Rare actions sit in the ⋯ More menu. Dialogs go on top.

use iced::widget::text::Wrapping;
use iced::widget::{
    Button, Column, button, center, column, container, mouse_area, opaque, progress_bar,
    responsive, row, rule, scrollable, space, stack, text, text_input, tooltip,
};
use iced::{Alignment, Element, Length};
use rwpm_core::launch::Os;
use rwpm_core::profile::{LastRun, Mode, Profile, RunOutcome, StartedBy};

use super::icon::{Icon, UI_BOLD, icon};
use super::message::{Dialog, Drill, Message};
use super::{App, Notice, dialogs, keys, setup, text as t, theme};

const GAP: u32 = 10;
/// List rows show this many characters of a name; the tooltip shows all of it.
const LIST_NAME_CHARS: usize = 24;

impl App {
    pub fn view(&self) -> Element<'_, Message> {
        if let Some(setup) = &self.setup {
            return setup::view(setup);
        }
        let main = container(
            column![
                self.header(),
                self.banner(),
                row![self.list(), self.details()]
                    .spacing(16)
                    .height(Length::Fill),
                self.footer()
            ]
            .spacing(GAP)
            .padding(16),
        )
        .width(Length::Fill)
        .height(Length::Fill);
        match &self.dialog {
            // Help sizes its list to the window, so its OK button stays on screen.
            Some(Dialog::Help) => stack![
                main,
                opaque(responsive(|size| dialogs::help_layer(size.height)))
            ]
            .into(),
            Some(dialog) => stack![
                main,
                opaque(
                    center(
                        container(dialogs::dialog_view(self, dialog))
                            .padding(24)
                            .max_width(560)
                            .style(theme::raised)
                    )
                    .style(theme::scrim)
                )
            ]
            .into(),
            None => main.into(),
        }
    }

    fn header(&self) -> Element<'_, Message> {
        row![
            text(t::TITLE).size(20).font(UI_BOLD),
            space::horizontal(),
            tip(
                action(
                    Icon::Folder,
                    t::GAME_VERSIONS,
                    Some(Message::Drill(Some(Drill::GameVersions))),
                    theme::ghost
                ),
                t::TIP_GAME_VERSIONS
            ),
            action(Icon::Help, t::HELP, Some(Message::Help), theme::ghost),
            action(
                Icon::Settings,
                t::SETTINGS,
                Some(Message::OpenSettings),
                theme::ghost
            )
        ]
        .spacing(4)
        .align_y(Alignment::Center)
        .into()
    }

    fn banner(&self) -> Element<'_, Message> {
        let mut lines = Column::new().spacing(4);
        if let Err(e) = &self.game {
            lines = lines.push(
                row![
                    problem(t::no_game(e)),
                    action(
                        Icon::Folder,
                        t::FIND_GAME,
                        Some(Message::OpenSettings),
                        theme::secondary
                    )
                ]
                .spacing(GAP)
                .align_y(Alignment::Center),
            );
        }
        for (id, e) in &self.unreadable {
            lines = lines.push(problem(t::unreadable(id, e)));
        }
        if let Some(other) = &self.other_profiles {
            lines = lines.push(
                row![
                    icon(Icon::Help).style(theme::muted),
                    text(t::other_profiles(other, &self.ctx.app_dir))
                        .style(theme::muted)
                        .width(Length::Fill),
                    action(
                        Icon::Folder,
                        t::OPEN_THAT_FOLDER,
                        Some(Message::OpenOtherProfiles),
                        theme::ghost
                    )
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            );
        }
        lines.into()
    }

    fn list(&self) -> Element<'_, Message> {
        let rows: Vec<Element<'_, Message>> = self.shown().map(|p| self.list_row(p)).collect();
        let body: Element<'_, Message> = if rows.is_empty() {
            text(t::NO_MATCH).style(theme::muted).into()
        } else {
            scrollable(Column::with_children(rows).spacing(2))
                .spacing(6)
                .height(Length::Fill)
                .into()
        };
        column![
            text_input(t::SEARCH_HINT, &self.search)
                .id(keys::SEARCH)
                .on_input(Message::Search)
                .on_submit(Message::SearchSubmit)
                .padding(8),
            container(body).height(Length::Fill),
            action(Icon::Add, t::NEW, Some(Message::New), theme::secondary),
            action(
                Icon::Folder,
                t::RESTORE_MENU,
                self.blocked().is_none().then_some(Message::RestoreBackup),
                theme::ghost
            ),
            action(
                Icon::Folder,
                t::OPEN_PACK_MENU,
                self.blocked().is_none().then_some(Message::OpenPack),
                theme::ghost
            )
        ]
        .spacing(GAP)
        .width(280)
        .into()
    }

    fn list_row<'a>(&'a self, p: &'a Profile) -> Element<'a, Message> {
        let name = t::short(&p.name, LIST_NAME_CHARS);
        let long = name.len() < p.name.len();
        let label = column![
            row![
                icon(mode_icon(p)).width(18),
                text(name).wrapping(Wrapping::None),
                // A known health problem: shape and colour, not colour alone.
                self.health
                    .get(&p.id)
                    .is_some_and(|probs| !probs.is_empty())
                    .then(|| icon(Icon::Warning).size(12).style(theme::warning))
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            text(self.facts(p))
                .size(12)
                .style(theme::muted)
                .wrapping(Wrapping::None)
        ]
        .spacing(2);
        let row = button(label)
            .width(Length::Fill)
            .padding([6, 10])
            .style(theme::list_row(p.id == self.selected))
            .on_press(Message::Select(p.id.clone()));
        if long { tip(row, &p.name) } else { row.into() }
    }

    /// "OFFLINE · 142 mods" under a profile's name.
    fn facts(&self, p: &Profile) -> String {
        let mode = if p.is_default() {
            t::BADGE_DEFAULT
        } else if p.mode == Mode::Offline {
            t::BADGE_OFFLINE
        } else {
            t::BADGE_LIVE
        };
        let played = self
            .last_runs
            .get(&p.id)
            .filter(|run| run.outcome == RunOutcome::Ok)
            .and_then(LastRun::age_secs);
        t::facts(mode, self.mod_counts.get(&p.id).copied(), played)
    }

    fn details(&self) -> Element<'_, Message> {
        let Some(p) = self.current() else {
            return space::horizontal().into();
        };
        let heading = row![
            text(&p.name).size(22).font(UI_BOLD).width(Length::Fill),
            action(Icon::More, t::MORE, Some(Message::ToggleMenu), theme::ghost)
        ]
        .spacing(GAP)
        .align_y(Alignment::Start);
        let facts = row![
            icon(mode_icon(p)).size(13).style(theme::muted),
            text(self.facts(p)).size(13).style(theme::muted)
        ]
        .spacing(6)
        .align_y(Alignment::Center);
        // Room on the right so text never runs under the scrollbar.
        let drill = match self.drill {
            Some(Drill::SteamGuide) => Some(self.steam_guide(p)),
            Some(Drill::Mods) => Some(self.mod_list_view(p)),
            Some(Drill::GameVersions) => Some(self.game_versions_view()),
            None => None,
        };
        if let Some(drill) = drill {
            return container(drill)
                .padding(20)
                .width(Length::Fill)
                .height(Length::Fill)
                .style(theme::surface)
                .into();
        }
        let sections = scrollable(
            Column::new()
                .push(self.pack_section(p))
                .push(self.game_version_section(p))
                .push(self.health_section(p))
                .push(self.mods_section(p))
                .push(self.start_section())
                .spacing(24)
                .width(Length::Fill)
                .padding(iced::Padding::ZERO.right(16)),
        )
        .height(Length::Fill);
        let pane = container(
            column![heading, facts, self.play_row()]
                .push(self.sync_banner(p))
                .push(rule::horizontal(1))
                .push(sections)
                .spacing(14),
        )
        .padding(20)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(theme::surface);
        if !self.menu_open {
            return pane.into();
        }
        // The menu floats under the ⋯ button; a click anywhere else closes it.
        let menu = container(self.menu(p))
            .align_right(Length::Fill)
            .align_top(Length::Fill)
            .padding([60, 20]);
        stack![pane, mouse_area(menu).on_press(Message::ToggleMenu)].into()
    }

    fn play_row(&self) -> Element<'_, Message> {
        // A pinned profile plays its build, so a missing Steam install does not stop it.
        let why = self
            .current()
            .and_then(|p| self.game_problem(p))
            .or_else(|| self.blocked());
        let status = why.clone().or_else(|| {
            let run = self.last_runs.get(&self.selected)?;
            Some(t::last_run(
                run.outcome,
                run.started_by,
                run.age_secs(),
                &run.reason,
            ))
        });
        let play = button(
            row![icon(Icon::Play), text(t::LAUNCH).size(18).font(UI_BOLD)]
                .spacing(10)
                .align_y(Alignment::Center),
        )
        .padding([10, 28])
        .style(theme::primary)
        .on_press_maybe(why.is_none().then_some(Message::Launch));
        row![
            tip(play, t::TIP_PLAY),
            text(status.unwrap_or_default()).style(theme::muted)
        ]
        .spacing(16)
        .align_y(Alignment::Center)
        .into()
    }

    fn mods_section<'a>(&'a self, p: &'a Profile) -> Element<'a, Message> {
        let view_list = action(
            Icon::Forward,
            t::VIEW_MODS,
            Some(Message::Drill(Some(Drill::Mods))),
            theme::secondary,
        );
        let mut body = column![section_title(t::MODS_TITLE)].spacing(8);
        if p.is_default() {
            return body.push(text(t::DEFAULT_MODS)).push(view_list).into();
        }
        // These need RimWorld; without it they wait, with the reason on Play.
        let wait = self.blocked().is_some() || self.game_problem(p).is_some();
        let (about, buttons) = if p.mode == Mode::Offline {
            (
                t::OFFLINE_ABOUT,
                row![
                    tipped(
                        Icon::Refresh,
                        t::REFRESH,
                        Message::Refresh,
                        t::TIP_REFRESH,
                        wait
                    ),
                    tipped(
                        Icon::Live,
                        t::GO_LIVE,
                        Message::GoLive,
                        t::TIP_GO_LIVE,
                        wait
                    )
                ],
            )
        } else {
            (
                t::LIVE_ABOUT,
                row![tipped(
                    Icon::Offline,
                    t::GO_OFFLINE,
                    Message::GoOffline,
                    t::TIP_GO_OFFLINE,
                    wait
                )],
            )
        };
        // Own line: three buttons in a row do not fit at 960 px.
        body = body
            .push(text(about))
            .push(buttons.spacing(GAP))
            .push(view_list);
        body.into()
    }

    fn start_section(&self) -> Element<'_, Message> {
        let mut body = column![section_title(t::START_TITLE)].spacing(12);
        if self.launch_options().is_some() {
            let steam_ok = self
                .last_runs
                .get(&self.selected)
                .is_some_and(|r| r.started_by == StartedBy::Steam && r.outcome == RunOutcome::Ok);
            let mut steam = column![
                text(t::STEAM).font(UI_BOLD),
                text(t::STEAM_CAPTION).size(13).style(theme::muted),
                action(
                    Icon::Forward,
                    t::SET_UP_STEAM,
                    Some(Message::Drill(Some(Drill::SteamGuide))),
                    theme::secondary
                )
            ]
            .spacing(6);
            if steam_ok {
                steam = steam.push(
                    row![
                        icon(Icon::Ok).size(13).style(theme::success),
                        text(t::STEAM_WORKS).size(13).style(theme::success)
                    ]
                    .spacing(6),
                );
            }
            body = body.push(steam);
        }
        let wait =
            self.blocked().is_some() || self.current().and_then(|p| self.game_problem(p)).is_some();
        body.push(
            column![
                text(t::RIMSORT).font(UI_BOLD),
                text(t::RIMSORT_CAPTION).size(13).style(theme::muted),
                tipped(
                    Icon::Forward,
                    t::EXPORT,
                    Message::Export,
                    t::TIP_EXPORT,
                    wait
                )
            ]
            .spacing(6),
        )
        .into()
    }

    /// The ⋯ More menu: actions players need now and then.
    fn menu(&self, p: &Profile) -> Element<'_, Message> {
        let wait = self.blocked().is_some();
        let item = |glyph, label, msg: Message, enabled: bool| -> Element<'_, Message> {
            action(glyph, label, enabled.then_some(msg), theme::ghost)
                .width(Length::Fill)
                .into()
        };
        let mut items = vec![
            item(Icon::Copy, t::CLONE, Message::Clone, !wait),
            item(Icon::Folder, t::OPEN_SAVES, Message::OpenSaves, true),
            item(
                Icon::Copy,
                t::COPY_SETTINGS_MENU,
                Message::OpenCopySettings,
                !wait,
            ),
            item(Icon::Log, t::OPEN_LOG, Message::OpenLog, true),
            item(Icon::Clipboard, t::COPY_LOG, Message::CopyLog, true),
        ];
        if !p.is_default() {
            items.insert(1, item(Icon::Rename, t::RENAME, Message::Rename, !wait));
        }
        if !p.is_default() && p.mode == Mode::Offline {
            items.push(item(Icon::Copy, t::BACKUP_MENU, Message::Backup, !wait));
        }
        items.push(item(Icon::Copy, t::SHARE_MENU, Message::Share, !wait));
        // Only where a modpack subscribed this player to something that can be undone.
        if p.stored_dir()
            .is_some_and(|dir| !rwpm_core::fetch::record::read(dir).is_empty())
        {
            items.push(item(
                Icon::Close,
                t::REMOVE_PACK_SUBS,
                Message::RemovePackSubs,
                !wait,
            ));
        }
        if Os::current() == Os::Windows {
            items.push(item(
                Icon::Close,
                t::REMOVE_LOADER,
                Message::RemoveLoader,
                !wait,
            ));
        }
        if !p.is_default() {
            items.push(rule::horizontal(1).into());
            items.push(
                action(
                    Icon::Delete,
                    t::DELETE,
                    (!wait).then_some(Message::Delete),
                    theme::danger_ghost,
                )
                .width(Length::Fill)
                .into(),
            );
        }
        container(Column::with_children(items).spacing(2))
            .padding(6)
            .width(260)
            .style(theme::raised)
            .into()
    }

    fn footer(&self) -> Element<'_, Message> {
        if let Some(job) = &self.job {
            let total = job.total.max(1) as f32;
            let done = if job.total == 0 {
                0.0
            } else {
                job.done.min(job.total) as f32
            };
            let mut head = row![
                icon(Icon::Busy),
                text(if job.percent {
                    t::progress_percent(&job.label, job.done, job.total)
                } else {
                    t::progress(&job.label, job.done, job.total)
                }),
                text(t::OTHERS_WAIT).style(theme::muted)
            ]
            .spacing(8)
            .align_y(Alignment::Center);
            // A job that can be stopped says so with a button.
            if self.cancel.is_some() {
                head = head.push(space::horizontal()).push(
                    button(t::CANCEL)
                        .on_press(Message::CancelJob)
                        .style(theme::secondary),
                );
            }
            return column![head, progress_bar(0.0..=total, done).girth(6)]
                .spacing(6)
                .into();
        }
        let body: Element<'_, Message> = match &self.notice {
            Some(Notice::Info(line)) => row![icon(Icon::Ok), text(line).width(Length::Fill)]
                .spacing(8)
                .into(),
            Some(Notice::Error(line)) => problem(line.clone()),
            None => return space::vertical().height(0).into(),
        };
        // A long notice scrolls in a box of its own, so it cannot push the lists off the
        // window, and OK lets the player say they have read it.
        row![
            container(
                scrollable(container(body).padding(iced::Padding::ZERO.right(12)))
                    .width(Length::Fill)
            )
            .max_height(NOTICE_MAX_HEIGHT)
            .width(Length::Fill),
            button(t::OK)
                .on_press(Message::DismissNotice)
                .style(theme::secondary)
        ]
        .spacing(GAP)
        .align_y(Alignment::Start)
        .into()
    }
}

/// The tallest the footer notice grows before it scrolls: about six lines of text.
const NOTICE_MAX_HEIGHT: f32 = 132.0;

fn mode_icon(p: &Profile) -> Icon {
    if p.is_default() {
        Icon::Default
    } else if p.mode == Mode::Offline {
        Icon::Offline
    } else {
        Icon::Live
    }
}

pub(super) fn section_title(words: &str) -> iced::widget::Text<'_> {
    text(words).size(16).font(UI_BOLD)
}

/// A line that needs the player's attention.
fn problem<'a>(line: String) -> Element<'a, Message> {
    // Fill: long text wraps instead of pushing a fix button beside it out of view.
    row![
        icon(Icon::Warning).style(theme::error),
        text(line).style(theme::error).width(Length::Fill)
    ]
    .spacing(8)
    .into()
}

type Style = fn(&iced::Theme, button::Status) -> button::Style;

/// A button with an icon and a label; disabled when `msg` is `None`.
pub(super) fn action<'a>(
    glyph: Icon,
    label: &'a str,
    msg: Option<Message>,
    style: Style,
) -> Button<'a, Message> {
    button(
        row![icon(glyph).size(14), text(label)]
            .spacing(8)
            .align_y(Alignment::Center),
    )
    .padding([6, 12])
    .style(style)
    .on_press_maybe(msg)
}

/// An outlined action with a plain-words tooltip; disabled while something must wait.
fn tipped<'a>(
    glyph: Icon,
    label: &'a str,
    msg: Message,
    words: &'a str,
    wait: bool,
) -> Element<'a, Message> {
    tip(
        action(glyph, label, (!wait).then_some(msg), theme::secondary),
        words,
    )
}

/// A button with a plain-words explanation shown on hover.
fn tip<'a>(b: Button<'a, Message>, words: &'a str) -> Element<'a, Message> {
    tooltip(
        b,
        container(text(words).size(14))
            .padding(10)
            .max_width(360)
            .style(theme::raised),
        tooltip::Position::Bottom,
    )
    .gap(4)
    .into()
}
