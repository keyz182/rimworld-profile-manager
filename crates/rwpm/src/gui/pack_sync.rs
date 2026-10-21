//! A pack opened through the Steam client leaves the profile LIVE, and Steam fetches the
//! mods in its own time. When they have all arrived, this offers to take the profile
//! offline. The core says whether they have (`record::all_synced`); the offline step is the
//! one "Go offline" already uses. "Not now" is saved, so the question is not asked again.

use iced::widget::{button, row, space, text};
use iced::{Alignment, Element, Length, Task};
use rwpm_core::fetch::record;
use rwpm_core::profile::{Mode, Profile};

use super::message::Message;
use super::{App, Notice, text as t, theme};

impl App {
    /// Where the answer comes from, when the question applies to `p` at all: a LIVE profile
    /// made from a pack that has not been told "Not now", and a Steam install to look in.
    fn sync_source(&self, p: &Profile) -> Option<(std::path::PathBuf, std::path::PathBuf)> {
        if p.mode != Mode::Live || p.pack.as_ref().is_none_or(|pack| pack.declined_offline) {
            return None;
        }
        let workshop = self
            .game
            .as_ref()
            .ok()?
            .steam
            .as_ref()?
            .workshop_dir
            .clone();
        Some((p.stored_dir()?.to_path_buf(), workshop))
    }

    /// Looks at Steam's files off the UI thread; the answer arrives as `SyncChecked`.
    pub(super) fn check_synced(&self) -> Task<Message> {
        let Some((dir, workshop)) = self.current().and_then(|p| self.sync_source(p)) else {
            return Task::none();
        };
        let id = self.selected.clone();
        Task::perform(
            async move { record::all_synced(&dir, &workshop) == Some(true) },
            move |done| Message::SyncChecked(id.clone(), done),
        )
    }

    /// The actions of this file; `update` hands them over together.
    pub(super) fn sync_update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::SyncTick => return self.check_synced(),
            Message::SyncChecked(id, done) => {
                if done {
                    self.pack_synced.insert(id);
                } else {
                    self.pack_synced.remove(&id);
                }
            }
            Message::DeclineOffline => self.decline_offline(),
            _ => {}
        }
        Task::none()
    }

    fn decline_offline(&mut self) {
        let id = self.selected.clone();
        match self.ctx.store().and_then(|s| s.decline_offline_prompt(&id)) {
            Ok(_) => {
                self.pack_synced.remove(&id);
                self.reload();
            }
            Err(e) => self.notice = Some(Notice::Error(e.to_string())),
        }
    }

    /// "The pack's mods have arrived. Take this profile offline?"
    pub(super) fn sync_banner(&self, p: &Profile) -> Option<Element<'_, Message>> {
        if !self.pack_synced.contains(&p.id) || self.sync_source(p).is_none() {
            return None;
        }
        let wait = self.blocked().is_some();
        Some(
            row![
                text(t::PACK_SYNCED).width(Length::Fill),
                space::horizontal(),
                button(t::PACK_SYNCED_LATER)
                    .on_press(Message::DeclineOffline)
                    .style(theme::secondary),
                button(t::PACK_SYNCED_GO)
                    .on_press_maybe((!wait).then_some(Message::GoOffline))
                    .style(theme::primary),
            ]
            .spacing(10)
            .align_y(Alignment::Center)
            .into(),
        )
    }
}
