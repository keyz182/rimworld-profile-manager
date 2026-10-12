//! Slow work (copies, scans) runs on its own thread; progress and the result come back
//! as messages, so the window never freezes.

use iced::Task;
use iced::futures::SinkExt;
use iced::futures::channel::mpsc;

use super::message::{Message, Outcome};

/// Runs `work` on a thread. `work` gets a progress callback (bytes done so far).
pub fn spawn<F>(work: F) -> Task<Message>
where
    F: FnOnce(&mut dyn FnMut(u64)) -> Result<Outcome, String> + Send + 'static,
{
    let (tx, rx) = mpsc::unbounded();
    std::thread::spawn(move || {
        let progress_tx = tx.clone();
        let mut last = std::time::Instant::now();
        let mut progress = |done: u64| {
            // At most ~20 updates a second; the window only needs a smooth bar.
            if last.elapsed().as_millis() >= 50 {
                last = std::time::Instant::now();
                let _ = progress_tx.unbounded_send(Message::Progress(done));
            }
        };
        let result = work(&mut progress);
        let mut tx = tx;
        let _ = iced::futures::executor::block_on(tx.send(Message::Done(result)));
    });
    Task::run(rx, |m| m)
}
