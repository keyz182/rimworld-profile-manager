//! The Health section: the selected profile's problems in plain words, each with
//! one fix. Checked off the window's thread when the selection or the profiles change.

use iced::widget::{Column, button, column, row, text};
use iced::{Alignment, Element, Length, Task};
use rwpm_core::launch::Os;
use rwpm_core::profile::guard;
use rwpm_core::profile::health::{self, Problem};
use rwpm_core::profile::{ProcessProbe, Profile};
use rwpm_core::rimsort;

use super::icon::{Icon, UI_BOLD, icon};
use super::message::{Ask, Dialog, Drill, Message, Outcome};
use super::{App, text as t, theme};

impl App {
    /// Starts checking the selected profile; the answer comes back as `HealthRead`.
    pub(super) fn check_health(&self) -> Task<Message> {
        let Some(profile) = self.current().cloned() else {
            return Task::none();
        };
        let live = self.game.clone().ok();
        let ctx = self.ctx.clone();
        let id = profile.id.clone();
        Task::perform(
            async move {
                let settings = rimsort::settings_path().filter(|s| s.is_file());
                match ctx.store() {
                    Ok(store) => health::check_profile(
                        &store,
                        &profile,
                        live.as_ref(),
                        &ctx.app_dir,
                        settings.as_deref(),
                        Os::current(),
                    ),
                    Err(_) => Vec::new(),
                }
            },
            move |problems| Message::HealthRead(id.clone(), problems),
        )
    }

    /// "Freeze them": copies the mods that would load from Steam into the profile.
    pub(super) fn freeze_unfrozen(&mut self) -> Task<Message> {
        let game = match self.effective_game() {
            Ok(g) => g,
            Err(e) => return self.fail(e),
        };
        let report = self
            .ctx
            .store()
            .and_then(|s| guard::check_offline(&s, &self.selected, &game));
        let report = match report {
            Ok(r) if !r.unfrozen.is_empty() => r,
            Ok(_) => return Task::none(),
            Err(e) => return self.fail(e),
        };
        let (ctx, id, n) = (
            self.ctx.clone(),
            self.selected.clone(),
            report.unfrozen.len(),
        );
        let total = super::update::total_of(&report);
        self.start(t::JOB_FREEZING_NEW.into(), total, move |progress| {
            let store = ctx.store().map_err(|e| e.to_string())?;
            guard::add_to_profile(&store, &id, &report.unfrozen, &ProcessProbe, progress)
                .map_err(|e| e.to_string())?;
            Ok(Outcome::Changed(t::frozen_now(n)))
        })
    }

    pub(super) fn health_section<'a>(&'a self, p: &'a Profile) -> Option<Element<'a, Message>> {
        let problems = self.health.get(&p.id)?;
        let mut body = Column::new()
            .push(text(t::HEALTH_TITLE).size(16).font(UI_BOLD))
            .spacing(10);
        if problems.is_empty() {
            return Some(
                body.push(line(
                    Icon::Ok,
                    t::HEALTH_OK.to_owned(),
                    theme::success,
                    None,
                ))
                .into(),
            );
        }
        let wait = self.blocked().is_some();
        for problem in problems {
            let (words, fix) = match problem {
                Problem::NotFrozen(names) => (
                    t::not_frozen(names),
                    Some((t::FIX_FREEZE, Message::FreezeUnfrozen)),
                ),
                Problem::NotInstalled(ids) => (
                    t::not_installed_mods(ids),
                    Some((t::VIEW_MODS, Message::Drill(Some(Drill::Mods)))),
                ),
                Problem::LoaderMissing => (
                    t::LOADER_MISSING.to_owned(),
                    Some((t::FIX_LOADER, Message::InstallLoader)),
                ),
                Problem::LoaderForeign => (t::LOADER_FOREIGN.to_owned(), None),
                Problem::OnlyGameContent => (
                    t::ONLY_GAME_CONTENT.to_owned(),
                    Some((t::COPY_SETTINGS_MENU, Message::OpenCopySettings)),
                ),
                Problem::BuildMissing(key) => (t::build_missing(key), None),
                Problem::RimSortStale(name) => (
                    t::rimsort_stale(name),
                    Some((t::FIX_RIMSORT, Message::Export)),
                ),
            };
            // Fixes that change things wait for a running job or game, like other actions.
            let fix = fix.map(|(label, m)| {
                let enabled = !wait || matches!(m, Message::Drill(_));
                (label, enabled.then_some(m))
            });
            body = body.push(line(Icon::Warning, words, theme::warning, fix));
        }
        Some(body.into())
    }

    /// "Install loader…" from the Health section: ask first (consent pattern), then
    /// install without starting the game.
    pub(super) fn ask_loader(&mut self) {
        self.dialog = Some(Dialog::Consent(Ask::Loader { then_play: false }));
    }
}

type TextStyle = fn(&iced::Theme) -> text::Style;

fn line<'a>(
    glyph: Icon,
    words: String,
    style: TextStyle,
    fix: Option<(&'a str, Option<Message>)>,
) -> Element<'a, Message> {
    let mut r = row![
        icon(glyph).size(14).style(style),
        text(words).width(Length::Fill)
    ]
    .spacing(10)
    .align_y(Alignment::Center);
    if let Some((label, msg)) = fix {
        r = r.push(
            button(text(label))
                .padding([6, 12])
                .style(theme::secondary)
                .on_press_maybe(msg),
        );
    }
    column![r].into()
}
