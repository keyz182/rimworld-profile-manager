//! The profile's mod list (T29): read-only, in load order, searchable. Read off the
//! window's thread, since a big list means reading a thousand About.xml files.

use iced::widget::text::Wrapping;
use iced::widget::{Column, button, column, container, row, scrollable, text, text_input};
use iced::{Alignment, Element, Length, Task};
use rwpm_core::profile::mod_list::{self, Flag, ModRow, Source};
use rwpm_core::profile::{Profile, ProfileId};

use super::icon::{Icon, UI_BOLD, icon};
use super::message::Message;
use super::{App, text as t, theme};

/// The mod search field.
pub const SEARCH: &str = "mod-search";

/// A read mod list, for one profile.
#[derive(Debug, Clone)]
pub struct ModList {
    pub profile: ProfileId,
    pub rows: Result<Vec<ModRow>, String>,
}

impl App {
    /// Starts reading the selected profile's mods.
    pub(super) fn read_mods(&mut self) -> Task<Message> {
        self.mods = None;
        self.mod_search.clear();
        let Some(profile) = self.current().cloned() else {
            return Task::none();
        };
        let id = profile.id.clone();
        let game = match self.effective_game() {
            Ok(g) => g,
            Err(e) => {
                self.mods = Some(ModList {
                    profile: id,
                    rows: Err(e.to_string()),
                });
                return Task::none();
            }
        };
        Task::perform(
            async move { mod_list::mod_list(&profile, &game) },
            move |rows| {
                Message::ModsRead(ModList {
                    profile: id.clone(),
                    rows,
                })
            },
        )
    }

    pub(super) fn mod_list_view<'a>(&'a self, p: &'a Profile) -> Element<'a, Message> {
        let back = button(
            row![icon(Icon::Back).size(14), text(t::BACK)]
                .spacing(8)
                .align_y(Alignment::Center),
        )
        .padding([6, 12])
        .style(theme::ghost)
        .on_press(Message::Drill(None));
        let list = self.mods.as_ref().filter(|m| m.profile == p.id);
        let body: Element<'_, Message> = match list.map(|m| &m.rows) {
            None => text(t::READING_MODS).style(theme::muted).into(),
            Some(Err(e)) => text(t::mods_unreadable(e)).style(theme::error).into(),
            Some(Ok(rows)) if rows.is_empty() => text(t::NO_MODS).style(theme::muted).into(),
            Some(Ok(rows)) => self.mod_rows(rows),
        };
        column![
            back,
            text(t::mods_title(&p.name)).size(22).font(UI_BOLD),
            body
        ]
        .spacing(14)
        .into()
    }

    fn mod_rows<'a>(&'a self, rows: &'a [ModRow]) -> Element<'a, Message> {
        let flagged = rows.iter().filter(|r| r.flag.is_some()).count();
        let shown: Vec<Element<'_, Message>> = rows
            .iter()
            .enumerate()
            .filter(|(_, r)| mod_list::matches(r, &self.mod_search))
            .map(|(i, r)| mod_row(i + 1, r))
            .collect();
        let list: Element<'_, Message> = if shown.is_empty() {
            text(t::NO_MOD_MATCH).style(theme::muted).into()
        } else {
            scrollable(
                Column::with_children(shown)
                    .spacing(2)
                    .width(Length::Fill)
                    .padding(iced::Padding::ZERO.right(16)),
            )
            .height(Length::Fill)
            .into()
        };
        column![
            text(t::mods_summary(rows.len(), flagged)).style(theme::muted),
            text_input(t::MOD_SEARCH_HINT, &self.mod_search)
                .id(SEARCH)
                .on_input(Message::ModSearch)
                .padding(8),
            list
        ]
        .spacing(10)
        .height(Length::Fill)
        .into()
    }
}

/// One mod: load position, name, where it loads from, and any problem.
fn mod_row(n: usize, r: &ModRow) -> Element<'_, Message> {
    let source = match r.source {
        Some(Source::Game) => t::SOURCE_GAME,
        Some(Source::Frozen) => t::SOURCE_FROZEN,
        Some(Source::Steam) => t::SOURCE_STEAM,
        Some(Source::Installed) => t::SOURCE_INSTALLED,
        None => "",
    };
    let mut line = row![
        text(n.to_string()).size(12).style(theme::muted).width(36),
        text(&r.name).wrapping(Wrapping::None).width(Length::Fill),
        text(source).size(13).style(theme::muted).width(150)
    ]
    .spacing(10)
    .align_y(Alignment::Center);
    if let Some(flag) = r.flag {
        let words = match flag {
            Flag::Missing => t::FLAG_MISSING,
            Flag::NotFrozen => t::FLAG_NOT_FROZEN,
            Flag::Twice => t::FLAG_TWICE,
        };
        line = line.push(
            row![
                icon(Icon::Warning).size(12).style(theme::warning),
                text(words).size(13).style(theme::warning)
            ]
            .spacing(6)
            .width(170),
        );
    } else {
        line = line.push(container(text("")).width(170));
    }
    container(line).padding([4, 6]).into()
}
