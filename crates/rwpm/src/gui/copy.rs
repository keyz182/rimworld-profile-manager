//! Copy settings between profiles (T33): "Start from" in the New profile dialog, the
//! More → "Copy settings from…" dialog, and first-run setup's seeding from Default.

use iced::widget::{Column, checkbox, column, pick_list, row, text};
use iced::{Alignment, Element, Task};
use rwpm_core::profile::copy_settings::{self, Group};
use rwpm_core::profile::{ProcessProbe, Profile, ProfileId};

use super::icon::UI_BOLD;
use super::message::{Dialog, Message, Outcome};
use super::{App, Notice, text as t, theme};

/// Where settings come from and which groups, as chosen in a dialog.
#[derive(Debug, Clone, PartialEq)]
pub struct SettingsCopy {
    /// `None`: start empty (New profile only).
    pub from: Option<ProfileId>,
    pub groups: Vec<Group>,
    /// Each group's size in the chosen source (review M1).
    pub sizes: Vec<(Group, u64)>,
}

/// One entry of the "Start from" list.
#[derive(Debug, Clone, PartialEq)]
pub struct Source {
    pub id: Option<ProfileId>,
    pub label: String,
}

impl std::fmt::Display for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

impl App {
    /// The default choice: settings from Default, the usual groups.
    pub(super) fn copy_choice(&self, from: Option<ProfileId>) -> SettingsCopy {
        let sizes = from
            .as_ref()
            .and_then(|id| self.profiles.iter().find(|p| p.id == *id))
            .map(|p| copy_settings::group_sizes(&p.save_data_dir()))
            .unwrap_or_default();
        SettingsCopy {
            from,
            groups: Group::ALL
                .into_iter()
                .filter(|g| g.on_by_default())
                .collect(),
            sizes,
        }
    }

    pub(super) fn open_copy_settings(&mut self) {
        if self.current().is_some_and(|p| !p.is_default()) {
            self.dialog = Some(Dialog::CopySettings(
                self.copy_choice(Some(ProfileId::default_id())),
            ));
        }
    }

    /// The source or a tick box changed in the open dialog.
    pub(super) fn edit_copy(&mut self, f: impl FnOnce(&mut SettingsCopy)) {
        let Some(Dialog::Name { copy: Some(c), .. } | Dialog::CopySettings(c)) = &self.dialog
        else {
            return;
        };
        let mut edited = c.clone();
        f(&mut edited);
        if edited.from != c.from {
            edited.sizes = self.copy_choice(edited.from.clone()).sizes;
        }
        if let Some(Dialog::Name { copy: Some(c), .. } | Dialog::CopySettings(c)) = &mut self.dialog
        {
            *c = edited;
        }
    }

    /// Copies `choice` into profile `to` as a job; `done` is the line shown after.
    /// With nothing to copy, `nothing` is shown instead (`None` keeps the current line).
    pub(super) fn copy_job(
        &mut self,
        to: Profile,
        choice: SettingsCopy,
        done: String,
        nothing: Option<&str>,
    ) -> Task<Message> {
        // Nothing to copy (an empty source or no group ticked): no job.
        let Some(from) = choice
            .from
            .and_then(|id| self.profiles.iter().find(|p| p.id == id).cloned())
            .filter(|p| !copy_settings::entries(&p.save_data_dir(), &choice.groups).is_empty())
        else {
            if let Some(line) = nothing {
                self.notice = Some(Notice::Info(line.to_owned()));
            }
            return Task::none();
        };
        let total = choice
            .sizes
            .iter()
            .filter(|(g, _)| choice.groups.contains(g))
            .map(|(_, b)| b)
            .sum();
        let ctx = self.ctx.clone();
        self.start(t::JOB_COPYING_SETTINGS.into(), total, move |progress| {
            let store = ctx.store().map_err(|e| e.to_string())?;
            store
                .copy_settings(&from, &to, &choice.groups, &ProcessProbe, progress)
                .map_err(|e| e.to_string())?;
            Ok(Outcome::Changed(done))
        })
    }

    /// First-run setup: the new profile starts from Default with the usual groups, at once.
    pub(super) fn seed_from_default(&mut self, to: &Profile) {
        let Some(default) = self.profiles.iter().find(|p| p.is_default()).cloned() else {
            return;
        };
        let groups: Vec<Group> = Group::ALL
            .into_iter()
            .filter(|g| g.on_by_default())
            .collect();
        let copied = self
            .ctx
            .store()
            .and_then(|s| s.copy_settings(&default, to, &groups, &ProcessProbe, &mut |_| {}));
        if let Err(e) = copied {
            self.notice = Some(Notice::Error(e.to_string()));
        }
    }

    /// The "Start from" list: Empty (New only), then every profile but `except`.
    fn sources(&self, with_empty: bool, except: Option<&ProfileId>) -> Vec<Source> {
        let empty = with_empty.then(|| Source {
            id: None,
            label: t::START_EMPTY.to_owned(),
        });
        empty
            .into_iter()
            .chain(
                self.profiles
                    .iter()
                    .filter(|p| Some(&p.id) != except)
                    .map(|p| Source {
                        id: Some(p.id.clone()),
                        label: p.name.clone(),
                    }),
            )
            .collect()
    }

    /// "Start from" + the five tick boxes, shared by both dialogs.
    pub(super) fn copy_form<'a>(
        &'a self,
        copy: &'a SettingsCopy,
        with_empty: bool,
        except: Option<&ProfileId>,
    ) -> Element<'a, Message> {
        let sources = self.sources(with_empty, except);
        let selected = sources.iter().find(|s| s.id == copy.from).cloned();
        let mut form = Column::new().spacing(8).push(
            row![
                text(t::START_FROM).font(UI_BOLD),
                pick_list(sources, selected, |s: Source| Message::CopyFrom(s.id))
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        );
        if copy.from.is_none() {
            return form.into();
        }
        for group in Group::ALL {
            let size = copy
                .sizes
                .iter()
                .find(|(g, _)| *g == group)
                .map_or(0, |(_, b)| *b);
            form = form.push(
                checkbox(copy.groups.contains(&group))
                    .label(t::copy_group(group, size))
                    .on_toggle(move |on| Message::ToggleGroup(group, on)),
            );
        }
        form.push(text(t::COPY_NOTE).size(13).style(theme::muted))
            .into()
    }
}

/// The More → "Copy settings from…" dialog body (buttons added by the caller).
pub fn copy_dialog<'a>(app: &'a App, copy: &'a SettingsCopy) -> Element<'a, Message> {
    let name = app.current().map(|p| p.name.as_str()).unwrap_or_default();
    column![
        text(t::copy_title(name)).size(18).font(UI_BOLD),
        app.copy_form(copy, false, Some(&app.selected))
    ]
    .spacing(14)
    .into()
}
