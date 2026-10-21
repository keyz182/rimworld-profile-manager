//! A profile made from a modpack: the panel that shows the pack (name, cover, description)
//! and "Reset to pack…". The core does the reset (staging, the bin, the checks); this file
//! asks, shows progress, reports.

use iced::widget::{checkbox, column, container, image, markdown, text};
use iced::{Element, Length, Task};
use rwpm_core::modpack::{self, ResetPlan};
use rwpm_core::profile::{ProcessProbe, Profile};

use super::dialogs::{buttons, heading};
use super::icon::Icon;
use super::message::{Dialog, Message, Outcome};
use super::view::{action, section_title};
use super::{App, Notice, text as t, theme};

/// Width the cover is drawn at in the profile view.
const COVER_WIDTH: f32 = 240.0;

/// What a pack profile shows of its pack. Read once when the list loads, not every frame.
#[derive(Debug, Clone)]
pub struct PackCard {
    pub about: Vec<markdown::Item>,
    pub cover: Option<image::Handle>,
}

impl PackCard {
    /// `None` for a profile that was not made from a pack.
    pub fn of(profile: &Profile) -> Option<Self> {
        let origin = profile.pack.as_ref()?;
        let cover = origin
            .has_cover
            .then(|| profile.stored_dir())
            .flatten()
            .and_then(|dir| std::fs::read(dir.join(modpack::cover::PROFILE_COVER)).ok())
            .map(image::Handle::from_bytes);
        Some(Self {
            about: markdown::parse(&origin.description).collect(),
            cover,
        })
    }
}

impl App {
    /// "From a modpack": the pack's name, cover and description, and the way back to it.
    pub(super) fn pack_section<'a>(&'a self, p: &'a Profile) -> Option<Element<'a, Message>> {
        let origin = p.pack.as_ref()?;
        let card = self.pack_cards.get(&p.id);
        let mut body = column![
            section_title(t::PACK_SECTION),
            text(t::pack_origin(&origin.name))
        ]
        .spacing(8);
        if let Some(cover) = card.and_then(|c| c.cover.clone()) {
            body = body.push(image(cover).width(Length::Fixed(COVER_WIDTH)));
        }
        if let Some(card) = card.filter(|c| !c.about.is_empty()) {
            body = body.push(
                container(
                    markdown::view(&card.about, theme::markdown_settings(self.dark))
                        .map(|url| Message::OpenUrl(url.to_string())),
                )
                .width(Length::Fill),
            );
        }
        let wait = self.blocked().is_some();
        Some(
            body.push(action(
                Icon::Refresh,
                t::PACK_RESET,
                (!wait).then_some(Message::ResetPack),
                theme::secondary,
            ))
            .into(),
        )
    }

    /// The actions of this file; `update` hands them over together.
    pub(super) fn reset_update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::ResetPack => self.ask_reset(),
            Message::ResetSaves(on) => self.reset_saves(on),
            _ => {}
        }
        Task::none()
    }

    fn ask_reset(&mut self) {
        let Some(id) = self
            .current()
            .filter(|p| p.pack.is_some())
            .map(|p| p.id.clone())
        else {
            return;
        };
        match self.plan_reset(&id, false) {
            Ok(plan) => self.dialog = Some(Dialog::ResetPack(Box::new(plan))),
            Err(words) => self.notice = Some(Notice::Error(words)),
        }
    }

    /// Plans again, so the space check follows the box.
    fn reset_saves(&mut self, on: bool) {
        let Some(Dialog::ResetPack(plan)) = &self.dialog else {
            return;
        };
        let id = plan.profile.id.clone();
        match self.plan_reset(&id, on) {
            Ok(plan) => self.dialog = Some(Dialog::ResetPack(Box::new(plan))),
            Err(words) => self.notice = Some(Notice::Error(words)),
        }
    }

    fn plan_reset(
        &self,
        id: &rwpm_core::profile::ProfileId,
        saves: bool,
    ) -> Result<ResetPlan, String> {
        let store = self.ctx.store().map_err(|e| e.to_string())?;
        modpack::plan_reset(&store, id, saves).map_err(|e| e.to_string())
    }

    /// The dialog's button. Enter must not start what the disabled button refuses.
    pub(super) fn reset_job(&mut self, plan: ResetPlan) -> Task<Message> {
        if !plan.missing.is_empty() {
            self.dialog = Some(Dialog::ResetPack(Box::new(plan)));
            return Task::none();
        }
        let ctx = self.ctx.clone();
        let (label, total) = (t::resetting(&plan.profile.name), plan.unpack.bytes_needed);
        self.start(label, total, move |progress| {
            let store = ctx.store().map_err(|e| e.to_string())?;
            let report = modpack::apply_reset(&plan, &store, &ProcessProbe, progress)
                .map_err(|e| e.to_string())?;
            Ok(Outcome::Changed(t::reset_done(
                &plan.profile.name,
                report.replaced,
                report.added,
            )))
        })
    }
}

/// What a reset does, the opt-in for saves, and, for an offline profile, what it lacks.
pub fn reset_dialog(plan: &ResetPlan) -> Element<'_, Message> {
    let mut body = column![
        heading(t::RESET_TITLE),
        text(t::reset_about(&plan.profile.name)),
        text(t::RESET_KEEPS).size(13),
        checkbox(plan.restore_saves)
            .label(t::RESET_SAVES)
            .on_toggle(Message::ResetSaves),
    ]
    .spacing(10);
    if plan.restore_saves {
        body = body.push(text(t::RESET_SAVES_NOTE).size(13));
    }
    if !plan.missing.is_empty() {
        let names: Vec<&str> = plan.missing.iter().map(|m| m.name.as_str()).collect();
        body = body.push(
            text(t::reset_missing(&names))
                .size(13)
                .style(theme::warning),
        );
    }
    let ok = plan.missing.is_empty().then_some(Message::Confirm);
    body.push(buttons(t::RESET_IT, ok, true)).into()
}
