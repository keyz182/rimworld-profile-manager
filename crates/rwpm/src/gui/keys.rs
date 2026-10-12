//! Keyboard (C: app-level keys). Only keys no widget used reach here, so typing in a
//! text field never triggers a shortcut.

use iced::Task;
use iced::keyboard::key::Named;
use iced::keyboard::{Key, Modifiers};

use super::App;
use super::message::{Dialog, Drill, GuardChoice, Message};
use super::setup::{SetupMsg, Step};

/// The profile search field.
pub const SEARCH: &str = "profile-search";

impl App {
    pub(super) fn key(&mut self, key: Key, modifiers: Modifiers) -> Task<Message> {
        if let Some(setup) = &self.setup {
            // Setup: Enter = its main button.
            let main = match setup.step {
                Step::Game => SetupMsg::Continue,
                Step::Profile => SetupMsg::Create,
            };
            return match key.as_ref() {
                Key::Named(Named::Enter) => self.update(Message::Setup(main)),
                _ => Task::none(),
            };
        }
        match key.as_ref() {
            Key::Named(Named::Escape) => {
                if self.dialog.is_some() {
                    return self.update(Message::Cancel);
                }
                if self.menu_open {
                    self.menu_open = false;
                } else if self.drill.is_some() {
                    self.drill = None;
                } else if !self.search.is_empty() {
                    self.search.clear();
                }
            }
            Key::Named(Named::Enter) => {
                return match &self.dialog {
                    // Enter = the dialog's main button.
                    Some(Dialog::Guard(_)) => self.update(Message::Guard(GuardChoice::Add)),
                    Some(Dialog::Help) => self.update(Message::Cancel),
                    Some(_) => self.update(Message::Confirm),
                    None => self.update(Message::Launch),
                };
            }
            Key::Named(Named::ArrowDown) if self.dialog.is_none() => self.step(1),
            Key::Named(Named::ArrowUp) if self.dialog.is_none() => self.step(-1),
            Key::Character("f") if modifiers.command() && self.dialog.is_none() => {
                let field = if self.drill == Some(Drill::Mods) {
                    super::mods::SEARCH
                } else {
                    SEARCH
                };
                return iced::widget::operation::focus(field);
            }
            Key::Character("n") if modifiers.command() && self.dialog.is_none() => {
                return self.update(Message::New);
            }
            _ => {}
        }
        Task::none()
    }

    /// Moves the selection through the shown profiles.
    fn step(&mut self, by: isize) {
        let ids: Vec<_> = self.shown().map(|p| p.id.clone()).collect();
        let Some(last) = ids.len().checked_sub(1) else {
            return;
        };
        let at = ids.iter().position(|id| *id == self.selected);
        let next = match at {
            Some(i) => i.saturating_add_signed(by).min(last),
            None => 0,
        };
        let id = ids[next].clone();
        let _ = self.update(Message::Select(id));
    }
}
