//! The dialogs shown on top of the main window, one at a time.

use iced::Element;
use iced::widget::{button, column, row, space, text, text_input};
use rwpm_core::builds::capture::{CaptureKind, CapturePlan};
use rwpm_core::profile::guard::{Fix, GuardReport};
use rwpm_core::profile::offline::OfflinePlan;
use rwpm_core::profile::relocate::MovePlan;

use super::icon::UI_BOLD;
use super::message::{Ask, Dialog, Field, GuardChoice, Message, NameFor};
use super::{App, text as t, theme};

const GAP: u32 = 10;

pub fn dialog_view<'a>(app: &'a App, dialog: &'a Dialog) -> Element<'a, Message> {
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
        } => {
            let root = app.ctx.store().map(|s| s.root().display().to_string());
            settings_dialog(game, game_check, steam, move_to, root.unwrap_or_default())
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
        Dialog::Help => help_dialog(),
    }
}

fn heading(words: &str) -> iced::widget::Text<'_> {
    text(words).size(18).font(UI_BOLD)
}

fn help_dialog() -> Element<'static, Message> {
    let mut body = column![heading(t::HELP_TITLE)].spacing(12);
    for (title, words) in t::HELP_SECTIONS {
        body = body.push(column![text(title).font(UI_BOLD), text(words).size(14)].spacing(2));
    }
    body.push(text(t::ICON_CREDIT).size(12).style(theme::muted))
        .push(row![
            space::horizontal(),
            button(t::OK)
                .on_press(Message::Cancel)
                .style(theme::primary)
        ])
        .into()
}

/// Cancel and the main button; the main button is disabled until `ok` is set.
fn buttons(label: &str, ok: Option<Message>, danger: bool) -> Element<'_, Message> {
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

fn settings_dialog<'a>(
    game: &'a str,
    game_check: &'a Option<Result<(), String>>,
    steam: &'a str,
    move_to: &'a str,
    profiles_root: String,
) -> Element<'a, Message> {
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
    // M8: say at once whether the typed folder has RimWorld in it.
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

/// V58/§C: size and free space before a copy starts; Store is off when it cannot fit.
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
    let fits = plan.free_bytes.is_none_or(|free| free >= plan.bytes);
    let mut body = column![text(words)].spacing(12);
    if !fits {
        body = body.push(text(t::NOT_ENOUGH_ROOM).style(theme::error));
    }
    body.push(buttons(label, fits.then_some(Message::Confirm), false))
        .spacing(16)
        .into()
}
