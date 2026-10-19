//! Modpacks (T83c): getting the missing mods through the Steam helper. "Restore and subscribe…"
//! makes the profile and subscribes to the mods; "Remove subscriptions from the pack" undoes
//! exactly those. The core starts the helper and checks what it delivers (T83b); this file
//! asks, shows progress with a Cancel button, and reports. A pack is written by a stranger, so
//! the confirm lists the mod names with a warning, and a mod that is not the one the pack named
//! is never kept.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use iced::widget::{Column, column, container, scrollable, text};
use iced::{Element, Task};
use rwpm_core::ProfileError;
use rwpm_core::discovery::GameInstall;
use rwpm_core::fetch::{self, Fresh, Op, Outcome as Fetched, Request, Status, record};
use rwpm_core::modpack;
use rwpm_core::profile::{ProcessProbe, ProfileId, RunningProbe};

use super::dialogs::{buttons, heading};
use super::icon::UI_BOLD;
use super::message::{Dialog, Message, Outcome};
use super::modpack_open::PackPreview;
use super::{App, Notice, text as t, theme};

/// Names drawn in the confirm; the rest is "and N more".
const MAX_NAMES: usize = 200;
/// The part of the confirm that scrolls (smallest window, V33).
const NAMES_HEIGHT: f32 = 240.0;
/// The progress bar counts to this. The restore takes the first fifth, the fetch the rest.
const TOTAL: u64 = 1_000;
const RESTORE_SHARE: u64 = 200;

/// What the Steam route needs, when it can be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SteamRoute {
    pub helper: PathBuf,
    /// `<library>/steamapps/workshop/content/294100`: where Steam puts what it downloads.
    pub workshop_dir: PathBuf,
}

/// Is there a helper, and can it reach Steam? `(route, why not)`. No helper at all is not
/// worth a word: the buttons that need it simply are not there.
pub fn steam_route(exe: &Path, game: Option<&GameInstall>) -> (Option<SteamRoute>, Option<String>) {
    let Some(helper) = fetch::helper_beside(exe) else {
        return (None, None);
    };
    let Some(workshop_dir) = game
        .and_then(|g| g.steam.as_ref())
        .map(|s| s.workshop_dir.clone())
    else {
        return (None, Some(t::STEAM_NO_LIBRARY.to_owned()));
    };
    match fetch::check(&helper) {
        Ok(()) => (
            Some(SteamRoute {
                helper,
                workshop_dir,
            }),
            None,
        ),
        Err(why) => (None, Some(t::steam_unreachable(&why))),
    }
}

/// How the app runs the helper. Tests swap in a fake, so none of them starts a process.
pub type FetchFn = dyn Fn(&Path, &Request<'_>, &AtomicBool, &mut dyn FnMut(u64, u64)) -> Vec<Fetched>
    + Send
    + Sync;

#[derive(Clone)]
pub struct Fetcher(pub Arc<FetchFn>);

impl Fetcher {
    pub fn real() -> Self {
        Self(Arc::new(fetch::fetch))
    }
}

impl fmt::Debug for Fetcher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Fetcher")
    }
}

/// `done` of `total`, placed in the part of the bar from `from` to `to`.
fn scaled(done: u64, total: u64, from: u64, to: u64) -> u64 {
    from + (done.min(total.max(1)) * (to - from)) / total.max(1)
}

/// What a fetch did, in words the player reads.
#[derive(Debug, Default, PartialEq, Eq)]
struct Summary {
    subscribed: usize,
    already: usize,
    failed: Vec<(String, String)>,
    mismatched: Vec<String>,
}

fn summarise(outcomes: &[Fetched], names: &BTreeMap<u64, String>) -> Summary {
    let name = |id: u64| names.get(&id).cloned().unwrap_or_else(|| id.to_string());
    let mut summary = Summary::default();
    for outcome in outcomes {
        match &outcome.status {
            Status::Done(Fresh::New) => summary.subscribed += 1,
            Status::Done(Fresh::Already) => summary.already += 1,
            Status::Failed(why) => summary.failed.push((name(outcome.id), why.clone())),
            Status::Mismatch { .. } => summary.mismatched.push(name(outcome.id)),
        }
    }
    summary
}

impl App {
    /// The actions of this file; `update` hands them over together.
    pub(super) fn pack_fetch_update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::PackSubscribe => self.ask_subscribe(),
            Message::CancelJob => {
                if let Some(flag) = &self.cancel {
                    flag.store(true, Ordering::Relaxed);
                }
            }
            Message::RemovePackSubs => self.ask_unsubscribe(),
            _ => {}
        }
        Task::none()
    }

    /// "Restore and subscribe…": show what will be subscribed to before anything is done.
    fn ask_subscribe(&mut self) {
        let Some(Dialog::OpenPack(preview)) = self.dialog.take() else {
            return;
        };
        let can = preview.steam.is_some() && !preview.missing.is_empty();
        self.dialog = Some(if can {
            Dialog::ConfirmSubscribe(preview)
        } else {
            Dialog::OpenPack(preview)
        });
    }

    fn ask_unsubscribe(&mut self) {
        let Some(profile) = self.current() else {
            return;
        };
        let count = profile
            .stored_dir()
            .map_or(0, |dir| record::read(dir).len());
        let id = profile.id.clone();
        if count == 0 {
            self.notice = Some(Notice::Info(t::NO_PACK_SUBS.to_owned()));
        } else {
            self.dialog = Some(Dialog::ConfirmUnsubscribe { id, count });
        }
    }

    /// Restore the pack, then subscribe to its missing mods. The mods this run subscribed to
    /// are recorded in the new profile so they can be removed again.
    pub(super) fn subscribe_job(&mut self, preview: PackPreview) -> Task<Message> {
        let Some(route) = preview.steam.clone() else {
            return Task::none();
        };
        let (ctx, fetcher) = (self.ctx.clone(), self.fetcher.clone());
        let wanted: Vec<(u64, String)> = preview
            .missing
            .iter()
            .filter_map(|m| Some((m.workshop_id?, m.package_id.clone())))
            .collect();
        let names: BTreeMap<u64, String> = preview
            .missing
            .iter()
            .filter_map(|m| Some((m.workshop_id?, m.name.clone())))
            .collect();
        let still_to_find = preview.manual.len();
        let plan = preview.plan;
        let label = t::opening_pack(&plan.name);
        self.start_cancellable(label, TOTAL, move |progress, cancel| {
            let fail = |e: ProfileError| e.to_string();
            if ProcessProbe.any_game() {
                return Err(ProfileError::GameRunning.to_string());
            }
            let store = ctx.store().map_err(fail)?;
            let bytes = plan.manifest.uncompressed_bytes;
            let report = modpack::apply_unpack(&plan, &store, &mut |done| {
                progress(scaled(done, bytes, 0, RESTORE_SHARE));
            })
            .map_err(fail)?;
            let request = Request {
                op: Op::Subscribe,
                wanted: &wanted,
                workshop_dir: Some(&route.workshop_dir),
                quiet: fetch::QUIET,
            };
            let outcomes = (fetcher.0)(&route.helper, &request, cancel, &mut |done, total| {
                progress(scaled(done, total, RESTORE_SHARE, TOTAL));
            });
            let new: Vec<u64> = outcomes
                .iter()
                .filter(|o| o.status == Status::Done(Fresh::New))
                .map(|o| o.id)
                .collect();
            if let Some(dir) = report.profile.stored_dir() {
                record::add(dir, &new).map_err(fail)?;
            }
            let s = summarise(&outcomes, &names);
            Ok(Outcome::Restored {
                line: t::pack_subscribed(
                    &report.profile.name,
                    s.subscribed,
                    s.already,
                    &s.failed,
                    &s.mismatched,
                    still_to_find,
                ),
                id: report.profile.id,
            })
        })
    }

    /// Unsubscribes from exactly the mods this profile's pack subscribed to.
    pub(super) fn unsubscribe_job(&mut self, id: ProfileId) -> Task<Message> {
        let Some(dir) = self
            .profiles
            .iter()
            .find(|p| p.id == id)
            .and_then(|p| p.stored_dir().map(Path::to_path_buf))
        else {
            return Task::none();
        };
        let Some(helper) = fetch::helper_beside(&self.ctx.exe) else {
            self.notice = Some(Notice::Error(t::STEAM_HELPER_MISSING.to_owned()));
            return Task::none();
        };
        let fetcher = self.fetcher.clone();
        let wanted: Vec<(u64, String)> = record::read(&dir)
            .into_iter()
            .map(|id| (id, String::new()))
            .collect();
        self.start_cancellable(
            t::UNSUBSCRIBING.to_owned(),
            TOTAL,
            move |progress, cancel| {
                if ProcessProbe.any_game() {
                    return Err(ProfileError::GameRunning.to_string());
                }
                let request = Request {
                    op: Op::Unsubscribe,
                    wanted: &wanted,
                    workshop_dir: None,
                    quiet: fetch::QUIET,
                };
                let outcomes = (fetcher.0)(&helper, &request, cancel, &mut |done, total| {
                    progress(scaled(done, total, 0, TOTAL));
                });
                let done: Vec<u64> = outcomes
                    .iter()
                    .filter(|o| matches!(o.status, Status::Done(_)))
                    .map(|o| o.id)
                    .collect();
                record::remove(&dir, &done).map_err(|e| e.to_string())?;
                Ok(Outcome::Changed(t::pack_unsubscribed(
                    done.len(),
                    outcomes.len() - done.len(),
                )))
            },
        )
    }
}

/// What the player agrees to: the names (which come from the pack), the warning, the count.
pub fn confirm_subscribe_dialog(preview: &PackPreview) -> Element<'_, Message> {
    let mut names = Column::new().spacing(4);
    for m in preview.missing.iter().take(MAX_NAMES) {
        names = names.push(text(&m.name));
    }
    if preview.missing.len() > MAX_NAMES {
        let more = t::and_more(preview.missing.len() - MAX_NAMES);
        names = names.push(text(more).size(13).style(theme::muted));
    }
    column![
        heading(t::SUBSCRIBE_TITLE),
        text(t::subscribe_about(preview.missing.len())),
        container(scrollable(names)).max_height(NAMES_HEIGHT),
        text(t::SUBSCRIBE_TRUST)
            .size(13)
            .font(UI_BOLD)
            .style(theme::warning),
        text(t::SUBSCRIBE_KEEPS).size(13),
        buttons(t::SUBSCRIBE_IT, Some(Message::Confirm), false),
    ]
    .spacing(10)
    .into()
}

pub fn confirm_unsubscribe_dialog(count: usize) -> Element<'static, Message> {
    column![
        heading(t::UNSUBSCRIBE_TITLE),
        text(t::unsubscribe_about(count)),
        buttons(t::UNSUBSCRIBE_IT, Some(Message::Confirm), true),
    ]
    .spacing(10)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_is_placed_in_its_part_of_the_bar() {
        assert_eq!(scaled(0, 100, 0, 200), 0);
        assert_eq!(scaled(50, 100, 0, 200), 100);
        assert_eq!(scaled(100, 100, 200, 1000), 1000);
        assert_eq!(
            scaled(5, 0, 200, 1000),
            1000,
            "a total of 0 never divides by 0"
        );
        assert_eq!(scaled(500, 100, 200, 1000), 1000, "never past the end");
    }

    #[test]
    fn a_fetch_is_summed_up_by_name() {
        let names: BTreeMap<u64, String> = [(1, "One".to_owned()), (2, "Two".to_owned())].into();
        let outcome = |id, status| Fetched { id, status };
        let s = summarise(
            &[
                outcome(1, Status::Done(Fresh::New)),
                outcome(2, Status::Failed("no".into())),
                outcome(3, Status::Done(Fresh::Already)),
                outcome(
                    4,
                    Status::Mismatch {
                        expected: "a".into(),
                        found: "b".into(),
                    },
                ),
            ],
            &names,
        );
        assert_eq!(s.subscribed, 1);
        assert_eq!(s.already, 1);
        assert_eq!(s.failed, [("Two".to_owned(), "no".to_owned())]);
        assert_eq!(
            s.mismatched,
            ["4"],
            "an id with no name falls back to the number"
        );
    }
}
