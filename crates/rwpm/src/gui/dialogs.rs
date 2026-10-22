//! The dialogs shown on top of the main window, one at a time.

use iced::widget::{
    Column, button, center, checkbox, column, container, image, markdown, radio, row, scrollable,
    space, text, text_editor, text_input,
};
use iced::{Alignment, Element, Length};
use rwpm_core::backup::RestorePlan;
use rwpm_core::builds::capture::{CaptureKind, CapturePlan};
use rwpm_core::profile::guard::{Fix, GuardReport};
use rwpm_core::profile::offline::OfflinePlan;
use rwpm_core::profile::relocate::MovePlan;
use rwpm_core::update_check::Frequency;

use super::backup::{Draft, GameFiles, Tick};
use super::icon::UI_BOLD;
use super::message::{Ask, Dialog, Field, GuardChoice, Message, NameFor};
use super::modpack::{PickGroup, Picks, ShareDraft, ShareTick, creation_folder};
use super::updates::frequency_pick;
use super::{App, text as t, theme};

const GAP: u32 = 10;

/// The part of the Share dialog that scrolls. In the smallest window (600 px high) the
/// title, the padding and the buttons need about 150 px of it.
const SHARE_FORM_HEIGHT: f32 = 400.0;
/// The description box, as an editor or as a preview.
const DESCRIPTION_HEIGHT: f32 = 110.0;
/// Width of the chosen cover in the Share dialog.
const COVER_PREVIEW_WIDTH: f32 = 160.0;

/// Room a scrolling area needs beside the scrollbar, so the bar never covers text.
const SCROLLBAR_GUTTER: f32 = 16.0;

/// Padding for the content of a scrollable: leaves the right edge to the scrollbar.
pub(super) fn gutter() -> iced::Padding {
    iced::Padding::ZERO.right(SCROLLBAR_GUTTER)
}

/// The smallest window (see `min_size` in `mod.rs`); dialog sizes are set for it.
const MIN_WINDOW: iced::Size = iced::Size::new(960.0, 600.0);
const DIALOG_MIN_WIDTH: f32 = 560.0;
const DIALOG_MAX_WIDTH: f32 = 900.0;

/// How a dialog uses the window: it is never smaller than at the smallest window, and
/// grows with the window above that.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Room {
    /// Widest the dialog may be.
    pub width: f32,
    /// Height a scrolling area gains over its size in the smallest window.
    pub grow: f32,
}

impl Room {
    pub fn of(window: iced::Size) -> Self {
        let widest = (window.width - 2.0 * 48.0).max(DIALOG_MIN_WIDTH);
        Self {
            width: (window.width * 0.6)
                .clamp(DIALOG_MIN_WIDTH, DIALOG_MAX_WIDTH)
                .min(widest),
            grow: (window.height - MIN_WINDOW.height).max(0.0),
        }
    }
}

pub fn dialog_view<'a>(app: &'a App, dialog: &'a Dialog, room: Room) -> Element<'a, Message> {
    match dialog {
        Dialog::Name {
            purpose,
            value,
            copy,
        } => name_dialog(app, *purpose, value, copy.as_ref()),
        Dialog::CopySettings(copy) => {
            let ok = (copy.from.is_some() && !copy.groups.is_empty()).then_some(Message::Confirm);
            column![
                super::copy::copy_dialog(app, copy),
                buttons(t::COPY_SETTINGS, ok, false)
            ]
            .spacing(16)
            .into()
        }
        Dialog::ConfirmDelete => {
            let name = app.current().map(|p| p.name.as_str()).unwrap_or_default();
            confirm(t::confirm_delete(name), t::YES_DELETE, true)
        }
        Dialog::Offline { plan, choices } => offline_dialog(plan, choices),
        Dialog::Guard(report) => guard_dialog(report),
        Dialog::Consent(ask) => confirm(
            if *ask == Ask::RimSort {
                t::ASK_RIMSORT
            } else {
                t::ASK_LOADER
            }
            .to_owned(),
            t::ALLOW,
            false,
        ),
        Dialog::Settings {
            game,
            game_check,
            steam,
            move_to,
            updates,
        } => {
            let root = app.ctx.store().map(|s| s.root().display().to_string());
            settings_dialog(SettingsForm {
                game,
                game_check,
                steam,
                move_to,
                profiles_root: root.unwrap_or_default(),
                updates: *updates,
                checking: app.update_checking,
                update_mode: t::update_mode(&app.installer.kind),
            })
        }
        Dialog::ConfirmMove(plan) => move_dialog(plan),
        Dialog::ConfirmCapture(plan) => capture_dialog(plan),
        Dialog::PickVersion {
            steam,
            builds,
            chosen,
        } => super::picker::picker_view(steam, builds, chosen.as_deref()),
        Dialog::ConfirmDowngrade { from, into, .. } => {
            confirm(t::confirm_downgrade(from, into), t::KEEP_GOING, false)
        }
        Dialog::ConfirmDeleteBuild { key, bytes } => {
            confirm(t::confirm_delete_version(key, *bytes), t::YES_DELETE, true)
        }
        Dialog::Backup(draft) => backup_dialog(draft),
        Dialog::Restore(plan) => restore_dialog(plan),
        Dialog::Share(draft) => share_dialog(draft, app.dark, room),
        Dialog::OpenPack(preview) => super::modpack_open::pack_dialog(preview, app.dark, room),
        Dialog::ResetPack(plan) => super::pack_reset::reset_dialog(plan),
        Dialog::ConfirmSubscribe(preview) => {
            super::pack_fetch::confirm_subscribe_dialog(preview, room)
        }
        Dialog::ConsentSteamcmd(_) => super::pack_fetch::consent_steamcmd_dialog(),
        Dialog::TrustSteamcmd { first, found, .. } => {
            super::pack_fetch::trust_steamcmd_dialog(first, found)
        }
        Dialog::ConfirmUnsubscribe { count, .. } => {
            super::pack_fetch::confirm_unsubscribe_dialog(*count)
        }
        Dialog::Help => help_dialog(help_list_height(600.0)),
    }
}

pub(super) fn heading(words: &str) -> iced::widget::Text<'_> {
    text(words).size(18).font(UI_BOLD)
}

/// Room the Help text list may take in a window `window_height` tall: what is left after
/// the dialog's padding, its title and the OK row.
fn help_list_height(window_height: f32) -> f32 {
    const PADDING_AND_CHROME: f32 = 48.0 + 120.0;
    (window_height - PADDING_AND_CHROME).max(120.0)
}

/// The Help sections scroll; the title and OK stay put, so OK is always on screen even in
/// the smallest window. The list takes only as much height as its text needs, up to
/// what the window leaves. The sections once filled 600 px on their own.
fn help_dialog(list_height: f32) -> Element<'static, Message> {
    let mut sections = Column::new()
        .spacing(12)
        .width(Length::Fill)
        .padding(gutter());
    for (title, words) in t::HELP_SECTIONS {
        sections =
            sections.push(column![text(title).font(UI_BOLD), text(words).size(14)].spacing(2));
    }
    let sections = sections.push(text(t::ICON_CREDIT).size(12).style(theme::muted));
    column![
        heading(t::HELP_TITLE),
        container(scrollable(sections).width(Length::Fill)).max_height(list_height),
        row![
            space::horizontal(),
            button(t::OK)
                .on_press(Message::Cancel)
                .style(theme::primary)
        ]
    ]
    .spacing(12)
    .into()
}

/// The Help dialog on its backdrop, sized to the window it is shown in.
pub fn help_layer(window: iced::Size) -> Element<'static, Message> {
    let room = Room::of(window);
    center(
        container(help_dialog(help_list_height(window.height)))
            .padding(24)
            .max_width(room.width)
            .style(theme::raised),
    )
    .style(theme::scrim)
    .into()
}

/// Cancel and the main button; the main button is disabled until `ok` is set.
pub(super) fn buttons(label: &str, ok: Option<Message>, danger: bool) -> Element<'_, Message> {
    row![
        space::horizontal(),
        button(t::CANCEL)
            .on_press(Message::Cancel)
            .style(theme::secondary),
        button(label).on_press_maybe(ok).style(if danger {
            theme::danger
        } else {
            theme::primary
        })
    ]
    .spacing(GAP)
    .into()
}

fn confirm<'a>(question: String, ok: &'a str, danger: bool) -> Element<'a, Message> {
    column![text(question), buttons(ok, Some(Message::Confirm), danger)]
        .spacing(20)
        .into()
}

fn name_dialog<'a>(
    app: &'a App,
    purpose: NameFor,
    value: &'a str,
    copy: Option<&'a super::copy::SettingsCopy>,
) -> Element<'a, Message> {
    let title = match purpose {
        NameFor::New => t::NAME_NEW,
        NameFor::Clone => t::NAME_COPY,
        NameFor::Rename => t::NAME_RENAME,
    };
    let ok = (!value.trim().is_empty()).then_some(Message::Confirm);
    column![
        heading(title),
        text_input(t::NAME_HINT, value)
            .padding(8)
            .on_input(|v| Message::Input(Field::Name, v))
            .on_submit(Message::Confirm),
    ]
    .spacing(16)
    .push(copy.map(|c| app.copy_form(c, true, None)))
    .push(buttons(t::OK, ok, false))
    .into()
}

fn offline_dialog<'a>(
    plan: &'a OfflinePlan,
    choices: &'a [(String, std::path::PathBuf)],
) -> Element<'a, Message> {
    let mut body = column![
        heading(t::FREEZE_TITLE),
        text(t::freeze_summary(
            plan.copies.len(),
            plan.bytes_needed,
            plan.free_bytes
        )),
    ]
    .spacing(10);
    if !plan.missing.is_empty() {
        body = body.push(text(t::not_installed(&plan.missing)));
    }
    for c in &plan.conflicts {
        body = body.push(text(t::installed_twice(&c.package_id)));
        for o in &c.options {
            let picked = choices
                .iter()
                .any(|(id, d)| *id == c.package_id && *d == o.source_dir);
            let label = format!(
                "{} {}",
                if picked { "●" } else { "○" },
                o.source_dir.display()
            );
            body = body.push(
                button(text(label).size(13))
                    .style(theme::ghost)
                    .on_press(Message::Choose(c.package_id.clone(), o.source_dir.clone())),
            );
        }
    }
    let ready = plan
        .conflicts
        .iter()
        .all(|c| choices.iter().any(|(id, _)| *id == c.package_id));
    body.push(buttons(t::FREEZE, ready.then_some(Message::Confirm), false))
        .into()
}

fn guard_dialog(report: &GuardReport) -> Element<'_, Message> {
    let mut body = column![heading(t::GUARD_TITLE), text(t::GUARD_BODY)].spacing(8);
    for u in &report.unfrozen {
        let what = match &u.fix {
            Fix::Copy(_) => t::unfrozen_copy(&u.name),
            Fix::UseFrozen(_) => t::unfrozen_steam_copy(&u.name),
        };
        body = body.push(text(what));
    }
    body.push(
        row![
            space::horizontal(),
            button(t::CANCEL)
                .on_press(Message::Cancel)
                .style(theme::secondary),
            button(t::PLAY_ANYWAY)
                .on_press(Message::Guard(GuardChoice::PlayAnyway))
                .style(theme::secondary),
            button(t::TURN_OFF)
                .on_press(Message::Guard(GuardChoice::TurnOff))
                .style(theme::secondary),
            button(t::ADD_TO_PROFILE)
                .on_press(Message::Guard(GuardChoice::Add))
                .style(theme::primary),
        ]
        .spacing(GAP),
    )
    .into()
}

/// What the Settings dialog shows.
struct SettingsForm<'a> {
    game: &'a str,
    game_check: &'a Option<Result<(), String>>,
    steam: &'a str,
    move_to: &'a str,
    profiles_root: String,
    updates: Frequency,
    /// An update check is on its way.
    checking: bool,
    /// What Install update does for this install.
    update_mode: &'static str,
}

fn settings_dialog<'a>(form: SettingsForm<'a>) -> Element<'a, Message> {
    let SettingsForm {
        game,
        game_check,
        steam,
        move_to,
        profiles_root,
        updates,
        checking,
        update_mode,
    } = form;
    let field = |label: &'a str, hint: &'a str, value: &'a str, f: Field| {
        column![
            text(label),
            row![
                text_input(hint, value)
                    .padding(8)
                    .on_input(move |v| Message::Input(f, v)),
                button(text(t::BROWSE))
                    .padding([6, 12])
                    .style(theme::secondary)
                    .on_press(Message::Browse(f))
            ]
            .spacing(GAP)
            .align_y(iced::Alignment::Center)
        ]
        .spacing(4)
    };
    // Say at once whether the typed folder has RimWorld in it.
    let check: Element<'a, Message> = match game_check {
        None => text(t::FOUND_THROUGH_STEAM)
            .size(13)
            .style(theme::muted)
            .into(),
        Some(Ok(())) => text(t::GAME_FOLDER_OK)
            .size(13)
            .style(theme::success)
            .into(),
        Some(Err(e)) => text(t::setup_not_found(e))
            .size(13)
            .style(theme::error)
            .into(),
    };
    let ok = !matches!(game_check, Some(Err(_)));
    column![
        heading(t::SETTINGS),
        column![
            field(t::GAME_FOLDER, t::FOUND_AUTO, game, Field::GameDir),
            check
        ]
        .spacing(4),
        field(t::STEAM_FOLDER, t::FOUND_AUTO, steam, Field::SteamDir),
        text(t::PROFILES_FOLDER).font(UI_BOLD),
        text(t::profiles_now(&profiles_root))
            .size(13)
            .style(theme::muted),
        field(t::MOVE_TO, t::MOVE_TO_HINT, move_to, Field::MoveTo),
        column![
            text(t::UPDATES_LABEL).font(UI_BOLD),
            row![
                frequency_pick(updates, Message::SetUpdateFrequency),
                button(text(t::CHECK_NOW))
                    .padding([6, 12])
                    .style(theme::secondary)
                    .on_press_maybe((!checking).then_some(Message::CheckUpdatesNow))
            ]
            .spacing(GAP)
            .align_y(Alignment::Center),
            text(t::UPDATES_ABOUT).size(13).style(theme::muted),
            text(update_mode).size(13).style(theme::muted)
        ]
        .spacing(4),
        buttons(t::SAVE, ok.then_some(Message::Confirm), false)
    ]
    .spacing(16)
    .into()
}

fn move_dialog(plan: &MovePlan) -> Element<'_, Message> {
    confirm(
        t::confirm_move(
            plan.profiles.len(),
            plan.bytes,
            &plan.new_root.display().to_string(),
            plan.free_bytes,
        ),
        t::MOVE,
        false,
    )
}

/// Size and free space before a copy starts; Store is off when it cannot fit.
fn capture_dialog(plan: &CapturePlan) -> Element<'_, Message> {
    let (words, label) = match &plan.kind {
        CaptureKind::Update { reason, .. } => (
            t::confirm_update(
                plan.key.as_str(),
                t::update_reason(*reason),
                plan.bytes,
                plan.free_bytes,
            ),
            t::UPDATE_IT,
        ),
        _ => (
            t::confirm_capture(&plan.version, plan.bytes, plan.free_bytes),
            t::STORE_IT,
        ),
    };
    let fits = super::builds::fits(plan);
    let mut body = column![text(words)].spacing(12);
    if !fits {
        body = body.push(text(t::NOT_ENOUGH_ROOM).style(theme::error));
    }
    body.push(buttons(label, fits.then_some(Message::Confirm), false))
        .spacing(16)
        .into()
}

/// The mods notice always; the game notice when the game files are ticked.
fn backup_dialog(draft: &Draft) -> Element<'_, Message> {
    let tick = |label: String, on: bool, which: Tick| {
        checkbox(on)
            .label(label)
            .on_toggle(move |v| Message::BackupTick(which, v))
    };
    let game: Element<'_, Message> = match &draft.game {
        GameFiles::Stored { key, bytes } => tick(
            t::backup_build(key, *bytes),
            draft.includes.build,
            Tick::Build,
        )
        .into(),
        GameFiles::NotPinned => text(t::BACKUP_BUILD_NONE)
            .size(13)
            .style(theme::muted)
            .into(),
        GameFiles::Missing(key) => text(t::backup_build_missing(key))
            .size(13)
            .style(theme::muted)
            .into(),
    };
    let mut body = column![
        heading(t::BACKUP_TITLE),
        text(t::backup_about(&draft.name)),
        tick(
            t::backup_saves(draft.saves_bytes),
            draft.includes.saves,
            Tick::Saves
        ),
        tick(t::BACKUP_LOGS.to_owned(), draft.includes.logs, Tick::Logs),
        game,
        text(t::NOTICE_MODS).size(13),
    ]
    .spacing(10);
    if draft.includes.build {
        body = body.push(text(t::NOTICE_GAME).size(13).style(theme::warning));
    }
    body.push(buttons(t::BACKUP_IT, Some(Message::Confirm), false))
        .into()
}

/// Both notices that apply, and the plain-words notes (missing game version and so on)
/// before the player agrees.
fn restore_dialog(plan: &RestorePlan) -> Element<'_, Message> {
    let m = &plan.manifest;
    let mut body = column![
        heading(t::RESTORE_TITLE),
        text(t::restore_summary(
            &m.name,
            &m.rwpm_version,
            m.game_version.as_deref(),
            plan.bytes_needed,
            plan.free_bytes
        )),
        text(t::RESTORE_NAME),
        text_input(t::NAME_HINT, &plan.name)
            .padding(8)
            .on_input(|v| Message::Input(Field::Name, v))
            .on_submit(Message::Confirm),
    ]
    .spacing(10);
    for note in &plan.notes {
        body = body.push(text(note.to_string()).size(13).style(theme::warning));
    }
    body = body.push(text(t::NOTICE_MODS).size(13));
    if m.includes.build {
        body = body.push(text(t::NOTICE_GAME).size(13).style(theme::warning));
    }
    let ok = super::backup::restore_ok(plan).then_some(Message::Confirm);
    body.push(buttons(t::RESTORE_IT, ok, false)).into()
}

/// The pack holds no mods, so it has its own notice; mod settings carry a privacy warning.
fn share_dialog(draft: &ShareDraft, dark: bool, room: Room) -> Element<'_, Message> {
    let tick = |label: String, on: bool, which: ShareTick| {
        checkbox(on)
            .label(label)
            .on_toggle(move |v| Message::ShareTick(which, v))
    };
    let mut body = column![
        text(t::share_about(&draft.name)),
        group_picks(
            t::SHARE_MOD_SETTINGS,
            &draft.settings_picks,
            draft.mod_settings,
            ShareTick::ModSettings,
            PickGroup::ModSettings,
        ),
        text(t::SHARE_SETTINGS_NOTICE)
            .size(13)
            .style(theme::warning),
        group_picks(
            t::SHARE_CREATIONS,
            &draft.creations_picks,
            draft.creations,
            ShareTick::Creations,
            PickGroup::Creations,
        ),
        tick(
            t::share_group(
                t::SHARE_GAME_SETTINGS,
                draft.game_settings_count.files,
                draft.game_settings_count.bytes
            ),
            draft.game_settings,
            ShareTick::GameSettings
        ),
    ]
    .spacing(GAP);
    body = body.push(saves_section(draft));
    body = body
        .push(text(t::SHARE_DESCRIPTION))
        .push(description_box(draft, dark))
        .push(cover_section(draft))
        .push(text(t::SHARE_LINK))
        .push(
            text_input("https://", &draft.link)
                .padding(8)
                .on_input(|v| Message::Input(Field::Link, v))
                .on_submit(Message::Confirm),
        );
    let warnings = [
        (!draft.link_ok(), t::SHARE_LINK_BAD.to_owned()),
        (
            draft.description_too_long(),
            t::share_description_long(rwpm_core::modpack::file::MAX_DESCRIPTION_CHARS),
        ),
        (
            draft.name_too_long(),
            t::share_name_long(rwpm_core::modpack::file::MAX_NAME_CHARS),
        ),
    ];
    for (shown, words) in warnings {
        if shown {
            body = body.push(text(words).size(13).style(theme::error));
        }
    }
    // The title and the buttons stay on screen; the form scrolls.
    column![
        heading(t::SHARE_TITLE),
        container(
            scrollable(
                body.push(text(t::SHARE_NOTICE).size(13))
                    .width(Length::Fill)
                    .padding(gutter())
            )
            .width(Length::Fill)
        )
        .max_height(SHARE_FORM_HEIGHT + room.grow),
        buttons(
            t::SHARE_IT,
            draft.ready().then_some(Message::Confirm),
            false
        ),
    ]
    .spacing(GAP)
    .into()
}

/// The saves to include, and which of them the pack starts from.
/// The description: a box to type Markdown in, or the same words as they will be drawn.
fn description_box(draft: &ShareDraft, dark: bool) -> Element<'_, Message> {
    let shown: Element<'_, Message> = match &draft.preview {
        Some(items) => container(
            markdown::view(items, theme::markdown_settings(dark))
                .map(|url| Message::OpenUrl(url.to_string())),
        )
        .padding(8)
        .width(Length::Fill)
        .height(Length::Fixed(DESCRIPTION_HEIGHT))
        .into(),
        None => text_editor(&draft.editor.0)
            .on_action(Message::ShareEdit)
            .height(Length::Fixed(DESCRIPTION_HEIGHT))
            .into(),
    };
    column![
        shown,
        row![
            text(t::SHARE_MARKDOWN_HINT).size(13),
            space::horizontal(),
            checkbox(draft.preview.is_some())
                .label(t::SHARE_PREVIEW)
                .on_toggle(Message::SharePreview),
        ]
        .align_y(Alignment::Center),
    ]
    .spacing(4)
    .into()
}

/// A picture for the pack: chosen from a file, shown small, removable.
fn cover_section(draft: &ShareDraft) -> Element<'_, Message> {
    let mut body = column![text(t::SHARE_COVER)].spacing(4);
    match &draft.cover {
        Some(cover) => {
            body = body
                .push(image(cover.handle.clone()).width(Length::Fixed(COVER_PREVIEW_WIDTH)))
                .push(
                    row![
                        text(&cover.file_name).size(13),
                        space::horizontal(),
                        button(t::SHARE_COVER_CLEAR)
                            .on_press(Message::ShareCoverClear)
                            .style(theme::secondary),
                    ]
                    .align_y(Alignment::Center),
                );
        }
        None => {
            body = body
                .push(
                    button(t::SHARE_COVER_CHOOSE)
                        .on_press(Message::ShareCover)
                        .style(theme::secondary),
                )
                .push(text(t::SHARE_COVER_HINT).size(13));
        }
    }
    body.into()
}

/// A group's box, and under it, when it holds more than one item, a way to leave single
/// items out. Everything in is the default, and the list stays shut until asked for.
fn group_picks<'a>(
    label: &str,
    picks: &'a Picks,
    on: bool,
    tick: ShareTick,
    group: PickGroup,
) -> Element<'a, Message> {
    let count = picks.count();
    let words = if count.files as usize == picks.items.len() {
        t::share_group(label, count.files, count.bytes)
    } else {
        t::share_group_some(label, count.files, picks.items.len(), count.bytes)
    };
    let mut section = Column::new().spacing(6).push(
        checkbox(on)
            .label(words)
            .on_toggle(move |v| Message::ShareTick(tick, v)),
    );
    if !on || picks.items.len() < 2 {
        return section.into();
    }
    section = section.push(
        checkbox(picks.open)
            .label(t::SHARE_PICK)
            .on_toggle(move |v| Message::SharePickOpen(group, v)),
    );
    if !picks.open {
        return section.into();
    }
    let mut list = Column::new().spacing(6).padding([0, 20]);
    let mut folder = None;
    for (i, item) in picks.items.iter().enumerate() {
        if group == PickGroup::Creations && folder != Some(creation_folder(&item.name)) {
            folder = Some(creation_folder(&item.name));
            let heading = t::share_folder(creation_folder(&item.name));
            list = list.push(text(heading).size(13).style(theme::muted));
        }
        list = list.push(
            checkbox(item.on)
                .label(t::share_save(&item.label, item.bytes))
                .on_toggle(move |v| Message::SharePick(group, i, v)),
        );
    }
    section.push(list).into()
}

fn saves_section(draft: &ShareDraft) -> Element<'_, Message> {
    if draft.saves.is_empty() {
        return text(t::SHARE_NO_SAVES).size(13).style(theme::muted).into();
    }
    let mut list = Column::new().spacing(6);
    for (i, save) in draft.saves.iter().enumerate() {
        list = list.push(
            checkbox(save.on)
                .label(t::share_save(&save.name, save.bytes))
                .on_toggle(move |v| Message::ShareSave(i, v)),
        );
    }
    let mut section = column![text(t::SHARE_SAVES), list].spacing(6);
    if draft.saves.iter().any(|s| s.on) {
        section = section.push(text(t::SHARE_START));
        section = section.push(radio(
            t::SHARE_START_NONE,
            None,
            Some(draft.start),
            Message::ShareStart,
        ));
        for (i, save) in draft.saves.iter().enumerate().filter(|(_, s)| s.on) {
            section = section.push(radio(
                save.name.as_str(),
                Some(i),
                Some(draft.start),
                Message::ShareStart,
            ));
        }
    }
    section.into()
}

#[cfg(test)]
mod room_tests {
    use super::*;

    #[test]
    fn smallest_window_keeps_the_old_sizes() {
        let r = Room::of(iced::Size::new(960.0, 600.0));
        assert_eq!(r.grow, 0.0);
        assert!((DIALOG_MIN_WIDTH..=DIALOG_MAX_WIDTH).contains(&r.width));
    }

    #[test]
    fn grows_with_the_window_and_stops_at_the_cap() {
        let r = Room::of(iced::Size::new(1600.0, 1000.0));
        assert_eq!(r.grow, 400.0);
        assert_eq!(r.width, 900.0);
    }

    #[test]
    fn never_below_the_minimum() {
        let r = Room::of(iced::Size::new(300.0, 200.0));
        assert_eq!(r.grow, 0.0);
        assert_eq!(r.width, DIALOG_MIN_WIDTH);
    }
}
