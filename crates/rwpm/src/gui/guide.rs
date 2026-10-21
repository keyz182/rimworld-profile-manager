//! The Steam launch guide: a drill-in that shows the launch line, where to paste it,
//! and whether Steam has started this profile yet, read from `last-run.toml`.

use iced::widget::{button, column, container, row, scrollable, text};
use iced::{Alignment, Element, Length};
use rwpm_core::profile::{LastRun, Profile, RunOutcome, StartedBy};

use super::icon::{Icon, UI_BOLD, icon};
use super::message::Message;
use super::{App, text as t, theme};

impl App {
    pub(super) fn steam_guide<'a>(&'a self, p: &'a Profile) -> Element<'a, Message> {
        let back = button(
            row![icon(Icon::Back).size(14), text(t::BACK)]
                .spacing(8)
                .align_y(Alignment::Center),
        )
        .padding([6, 12])
        .style(theme::ghost)
        .on_press(Message::Drill(None));
        let line = self.launch_options().unwrap_or_default();
        let copy = button(
            row![icon(Icon::Copy).size(14), text(t::COPY_STEAM)]
                .spacing(8)
                .align_y(Alignment::Center),
        )
        .padding([6, 12])
        .style(theme::primary)
        .on_press(Message::CopyLaunchOptions);
        // No working launcher → no line to show or copy, only why.
        let first: Element<'a, Message> = if self.ctx.launcher_failed {
            text(t::NO_LAUNCHER).style(theme::error).into()
        } else {
            column![
                text(t::GUIDE_COPY),
                container(text(line).size(13))
                    .padding(10)
                    .width(Length::Fill)
                    .style(theme::surface),
                copy
            ]
            .spacing(8)
            .into()
        };
        let steps = column![
            step(1, first),
            step(2, text(t::GUIDE_OPEN).into()),
            step(3, text(t::GUIDE_PASTE).into()),
            step(4, text(t::GUIDE_START).into()),
        ]
        .spacing(16);
        let body = column![
            back,
            text(t::guide_title(&p.name)).size(22).font(UI_BOLD),
            steps,
            text(t::GUIDE_ONE_PROFILE).size(13).style(theme::muted),
            self.steam_check(p)
        ]
        .spacing(18)
        .padding(iced::Padding::ZERO.right(16));
        scrollable(body).height(Length::Fill).into()
    }

    /// Has Steam started this profile yet? Updates when a game starts or ends.
    fn steam_check(&self, p: &Profile) -> Element<'_, Message> {
        let run = self
            .last_runs
            .get(&p.id)
            .filter(|r| r.started_by == StartedBy::Steam);
        let (glyph, words, style): (Icon, String, fn(&iced::Theme) -> text::Style) = match run {
            Some(
                r @ LastRun {
                    outcome: RunOutcome::Ok,
                    ..
                },
            ) => (Icon::Ok, t::steam_worked(r.age_secs()), theme::success),
            Some(r) => (
                Icon::Failed,
                t::steam_refused(r.age_secs(), &r.reason),
                theme::error,
            ),
            None => (Icon::Busy, t::GUIDE_WAITING.to_owned(), theme::muted),
        };
        container(
            row![icon(glyph).style(style), text(words).style(style)]
                .spacing(10)
                .align_y(Alignment::Center),
        )
        .padding(14)
        .width(Length::Fill)
        .style(theme::surface)
        .into()
    }
}

/// A numbered step.
fn step(n: u8, body: Element<'_, Message>) -> Element<'_, Message> {
    row![
        container(text(n.to_string()).font(UI_BOLD))
            .width(28)
            .center_x(28),
        container(body).width(Length::Fill)
    ]
    .spacing(10)
    .into()
}
