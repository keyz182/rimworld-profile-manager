//! Game versions (T56): the list of stored builds, Capture and Delete. A drill-in, like the
//! mod list, so the job bar stays visible while a copy runs. The core does the work (lock,
//! free-space check, staging, recovery); this file only asks, shows progress and reports.

use iced::widget::{Column, button, column, container, row, scrollable, space, text};
use iced::{Alignment, Element, Length, Task};
use rwpm_core::builds::capture::{CaptureKind, apply_capture, plan_capture};
use rwpm_core::builds::{BuildKey, BuildStore, pinned_by};
use rwpm_core::discovery::{Engine, engine_of};
use rwpm_core::profile::ProcessProbe;

use super::icon::{Icon, UI_BOLD, icon};
use super::message::{Dialog, Message, Outcome};
use super::{App, Ctx, Notice, text as t, theme};

/// One stored build, as the list shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildRow {
    pub key: String,
    /// `None` when `build.toml` cannot be read; then `problem` says why.
    pub waylith: bool,
    pub size: u64,
    pub stored: Option<String>,
    pub problem: Option<String>,
    /// Display names of profiles pinned to it (V54).
    pub used_by: Vec<String>,
    /// Profiles whose `profile.toml` could not be read, so their pin is unknown (D27).
    pub unchecked: Vec<String>,
}

impl BuildRow {
    /// Delete is open only when nobody uses the build and nobody can be unsure (V54).
    pub fn deletable(&self) -> bool {
        self.used_by.is_empty() && self.unchecked.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BuildsView {
    pub rows: Vec<BuildRow>,
}

impl Ctx {
    pub fn build_store(&self) -> Result<BuildStore, rwpm_core::ProfileError> {
        let root = self.settings()?.profiles_root(&self.app_dir);
        BuildStore::open(&root, &self.app_dir)
    }

    /// Reads the stored builds and who uses each. Listing also tidies up after an
    /// interrupted capture, so it runs off the window's thread.
    pub fn read_builds(&self) -> Result<BuildsView, String> {
        let store = self.build_store().map_err(|e| e.to_string())?;
        let profiles = self.store().map_err(|e| e.to_string())?;
        let listing = store.list().map_err(|e| e.to_string())?;
        let mut rows = Vec::new();
        let mut push = |key: &BuildKey,
                        found: Result<(bool, u64, Option<String>), String>|
         -> Result<(), String> {
            let pins = pinned_by(&profiles, key).map_err(|e| e.to_string())?;
            let (waylith, size, stored, problem) = match found {
                Ok((w, size, stored)) => (w, size, stored, None),
                Err(e) => {
                    let dir = store.dir_of(key);
                    let w = engine_of(&dir) == Engine::Waylith;
                    (w, rwpm_core::space::dir_size(&dir), None, Some(e))
                }
            };
            rows.push(BuildRow {
                key: key.to_string(),
                waylith,
                size,
                stored,
                problem,
                used_by: pins.profiles,
                unchecked: pins.unchecked,
            });
            Ok(())
        };
        for build in &listing.builds {
            let waylith = engine_of(&build.dir) == Engine::Waylith;
            let stored = build.file.captured_at.date.map(|d| d.to_string());
            push(&build.key, Ok((waylith, build.file.size, stored)))?;
        }
        for (key, e) in &listing.unreadable {
            push(key, Err(e.to_string()))?;
        }
        rows.sort_by(|a, b| a.key.cmp(&b.key));
        Ok(BuildsView { rows })
    }
}

impl App {
    /// Starts reading the list; the answer comes back as `BuildsRead`.
    pub(super) fn read_builds_task(&self) -> Task<Message> {
        let ctx = self.ctx.clone();
        Task::perform(async move { ctx.read_builds() }, Message::BuildsRead)
    }

    /// Measures the Steam install, then asks (`CapturePlanned`).
    pub(super) fn capture_build(&mut self) -> Task<Message> {
        let install = match &self.game {
            Ok(game) => game.clone(),
            Err(_) => return self.fail(t::GV_NO_GAME),
        };
        let ctx = self.ctx.clone();
        self.start(t::JOB_MEASURING_GAME.into(), 0, move |_| {
            let store = ctx.build_store().map_err(|e| e.to_string())?;
            plan_capture(&store, &install)
                .map(|plan| Outcome::CapturePlanned(Box::new(plan)))
                .map_err(|e| e.to_string())
        })
    }

    /// A measured capture: nothing to do, or the question with size and free space.
    pub(super) fn capture_planned(&mut self, plan: Box<rwpm_core::builds::capture::CapturePlan>) {
        if let CaptureKind::NoOp { stored } = &plan.kind {
            self.notice = Some(Notice::Info(t::already_stored(stored.as_str())));
        } else {
            self.dialog = Some(Dialog::ConfirmCapture(plan));
        }
    }

    pub(super) fn apply_capture(
        &mut self,
        plan: Box<rwpm_core::builds::capture::CapturePlan>,
    ) -> Task<Message> {
        let ctx = self.ctx.clone();
        let total = plan.bytes;
        self.start(t::JOB_STORING.into(), total, move |progress| {
            let store = ctx.build_store().map_err(|e| e.to_string())?;
            let stored =
                apply_capture(&store, &plan, &ProcessProbe, progress).map_err(|e| e.to_string())?;
            Ok(Outcome::BuildsChanged(match stored {
                Some(build) => t::version_stored(build.key.as_str()),
                None => t::already_stored(plan.key.as_str()),
            }))
        })
    }

    /// Delete build: asks first; the core re-checks the pin guard and the lock.
    pub(super) fn ask_delete_build(&mut self, key: &str) {
        let Some(row) = self
            .builds
            .as_ref()
            .and_then(|v| v.rows.iter().find(|r| r.key == key))
        else {
            return;
        };
        if row.deletable() {
            self.dialog = Some(Dialog::ConfirmDeleteBuild {
                key: row.key.clone(),
                bytes: row.size,
            });
        }
    }

    pub(super) fn delete_build(&mut self, key: String) -> Task<Message> {
        let ctx = self.ctx.clone();
        self.start(t::JOB_DELETING_VERSION.into(), 0, move |_| {
            let build_key = BuildKey::parse(&key).map_err(|e| e.to_string())?;
            let builds = ctx.build_store().map_err(|e| e.to_string())?;
            let profiles = ctx.store().map_err(|e| e.to_string())?;
            builds
                .delete(&profiles, &build_key, &ProcessProbe)
                .map_err(|e| e.to_string())?;
            Ok(Outcome::BuildsChanged(t::version_deleted(&key)))
        })
    }

    pub(super) fn game_versions_view(&self) -> Element<'_, Message> {
        let back = button(
            row![icon(Icon::Back).size(14), text(t::BACK)]
                .spacing(8)
                .align_y(Alignment::Center),
        )
        .padding([6, 12])
        .style(theme::ghost)
        .on_press(Message::Drill(None));
        let list: Element<'_, Message> = match &self.builds {
            None => text(t::GV_READING).style(theme::muted).into(),
            Some(view) if view.rows.is_empty() => text(t::GV_EMPTY).style(theme::muted).into(),
            Some(view) => scrollable(
                Column::with_children(view.rows.iter().map(|r| self.build_row(r)))
                    .spacing(12)
                    .width(Length::Fill)
                    .padding(iced::Padding::ZERO.right(16)),
            )
            .height(Length::Fill)
            .into(),
        };
        let count = self.builds.as_ref().map_or(0, |v| v.rows.len());
        column![
            back,
            text(t::gv_title(count)).size(22).font(UI_BOLD),
            text(t::GV_ABOUT).size(14).style(theme::muted),
            self.capture_row(),
            list
        ]
        .spacing(14)
        .height(Length::Fill)
        .into()
    }

    fn capture_row(&self) -> Element<'_, Message> {
        let why = match &self.game {
            Err(_) => Some(t::GV_NO_GAME.to_owned()),
            Ok(_) => self.blocked(),
        };
        let steam = match &self.game {
            Ok(game) => game.version.as_deref().map(t::steam_now),
            Err(_) => None,
        };
        let capture = super::view::action(
            Icon::Add,
            t::CAPTURE,
            why.is_none().then_some(Message::CaptureBuild),
            theme::primary,
        );
        row![
            capture,
            text(why.or(steam).unwrap_or_default()).style(theme::muted)
        ]
        .spacing(16)
        .align_y(Alignment::Center)
        .into()
    }

    fn build_row<'a>(&'a self, r: &'a BuildRow) -> Element<'a, Message> {
        let mut info = column![
            text(&r.key).font(UI_BOLD),
            text(t::gv_facts(r.waylith, r.size, r.stored.as_deref()))
                .size(13)
                .style(theme::muted)
        ]
        .spacing(2)
        .width(Length::Fill);
        if let Some(e) = &r.problem {
            info = info.push(
                text(t::version_unreadable(&r.key, e))
                    .size(13)
                    .style(theme::error),
            );
        }
        if !r.used_by.is_empty() {
            info = info.push(text(t::used_by(&r.used_by)).size(13));
        }
        if !r.unchecked.is_empty() {
            info = info.push(
                text(t::cannot_check(&r.unchecked))
                    .size(13)
                    .style(theme::error),
            );
        }
        let delete = super::view::action(
            Icon::Delete,
            t::DELETE_VERSION,
            (r.deletable() && self.blocked().is_none())
                .then(|| Message::DeleteBuild(r.key.clone())),
            theme::secondary,
        );
        container(
            row![info, space::horizontal(), delete]
                .spacing(12)
                .align_y(Alignment::Center),
        )
        .padding(10)
        .width(Length::Fill)
        .style(theme::surface)
        .into()
    }
}
