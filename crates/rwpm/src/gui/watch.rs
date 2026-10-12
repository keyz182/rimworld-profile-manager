//! Watches for a running RimWorld, so actions that clash with it are disabled (V33).
//! A thread checks every 2 s and reports changes only.

use iced::futures::Stream;
use iced::futures::channel::mpsc;

use super::message::Message;

const EVERY: std::time::Duration = std::time::Duration::from_secs(2);

/// `Message::GameRunning` at start and whenever the answer changes.
pub fn game_running() -> impl Stream<Item = Message> {
    let (tx, rx) = mpsc::unbounded();
    std::thread::spawn(move || {
        let mut last = None;
        loop {
            let now = rwpm_core::profile::any_game_running();
            if last != Some(now) {
                last = Some(now);
                if tx.unbounded_send(Message::GameRunning(now)).is_err() {
                    return; // The window closed.
                }
            }
            std::thread::sleep(EVERY);
        }
    });
    rx
}
