//! Watches for a running RimWorld, so actions that clash with it are disabled.
//! A thread checks every 2 s and reports changes only.

use iced::futures::Stream;
use iced::futures::channel::mpsc;

use super::message::Message;

const EVERY: std::time::Duration = std::time::Duration::from_secs(2);
/// Steam fetches mods over minutes, so a slow look is enough.
const SYNC_EVERY: std::time::Duration = std::time::Duration::from_secs(15);

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

/// An open window re-asks the schedule hourly, so a window left open for days still checks.
const UPDATE_EVERY: std::time::Duration = std::time::Duration::from_secs(60 * 60);

/// `Message::UpdateTick` every `UPDATE_EVERY`. The first one comes an hour after the window
/// opens: start-up checks for itself.
pub fn update_ticks() -> impl Stream<Item = Message> {
    let (tx, rx) = mpsc::unbounded();
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(UPDATE_EVERY);
            if tx.unbounded_send(Message::UpdateTick).is_err() {
                return; // The window closed.
            }
        }
    });
    rx
}

/// `Message::SyncTick` every `SYNC_EVERY`.
pub fn sync_ticks() -> impl Stream<Item = Message> {
    let (tx, rx) = mpsc::unbounded();
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(SYNC_EVERY);
            if tx.unbounded_send(Message::SyncTick).is_err() {
                return; // The window closed.
            }
        }
    });
    rx
}
