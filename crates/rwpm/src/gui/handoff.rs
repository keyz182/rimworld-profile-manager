//! A later launch of rwpm handing this window a file to open, or asking it to come forward
//! (see `instance`). A window that is busy keeps what it is doing and says so.

use iced::Task;
use iced::window::{self, UserAttention};

use super::message::Message;
use super::{App, Notice, OpenFile, text};
use crate::instance::Request;

impl App {
    pub(super) fn instance_request(&mut self, request: Request) -> Task<Message> {
        // Some desktops (Wayland) only let a window flash for attention, not take focus.
        let forward = window::latest().and_then(|id| {
            Task::batch([
                window::gain_focus(id),
                window::request_user_attention(id, Some(UserAttention::Informational)),
            ])
        });
        match request {
            Request::Focus => forward,
            Request::Open(file) => Task::batch([forward, self.open_from_launch(&file)]),
        }
    }

    /// Setup, a dialog and a running job each hold work the player has not finished:
    /// none of them is replaced by a file that arrives from outside.
    fn open_from_launch(&mut self, file: &OpenFile) -> Task<Message> {
        if self.setup.is_some() || self.dialog.is_some() || self.job.is_some() {
            let (OpenFile::Backup(path) | OpenFile::Pack(path)) = file;
            let name = path.file_name().map_or_else(
                || path.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            );
            self.notice = Some(Notice::Info(text::open_when_busy(&name)));
            return Task::none();
        }
        match file {
            OpenFile::Backup(path) => {
                self.open_backup(path);
                Task::none()
            }
            OpenFile::Pack(path) => self.open_pack(path),
        }
    }
}
