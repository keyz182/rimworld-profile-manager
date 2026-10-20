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
use std::time::Duration;

use iced::widget::{Column, column, container, scrollable, text};
use iced::{Element, Task};
use rwpm_core::ProfileError;
use rwpm_core::consent::{Consent, ConsentStore, Scope};
use rwpm_core::discovery::GameInstall;
use rwpm_core::fetch::{self, Fresh, Op, Outcome as Fetched, Request, Status, record};
use rwpm_core::modpack;
use rwpm_core::profile::offline;
use rwpm_core::profile::{ProcessProbe, Profile, ProfileId, RunningProbe};
use rwpm_core::steamcmd::{self, run::Layout};

use super::dialogs::{buttons, heading};
use super::icon::UI_BOLD;
use super::message::{Dialog, Message, Outcome};
use super::modpack_open::PackPreview;
use super::{App, Ctx, Notice, text as t, theme};

/// Names drawn in the confirm; the rest is "and N more".
const MAX_NAMES: usize = 200;
/// The part of the confirm that scrolls (smallest window, V33).
const NAMES_HEIGHT: f32 = 240.0;
/// The progress bar counts to this. The restore takes the first fifth, the fetch the rest.
const TOTAL: u64 = 1_000;
const RESTORE_SHARE: u64 = 200;
/// steamcmd may print nothing for this long, for example while it updates itself.
const CMD_QUIET: Duration = Duration::from_secs(300);

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

/// How the app installs and runs steamcmd. Tests swap in fakes, so none of them starts a
/// process or touches the network (T82c).
pub type InstallFn = dyn Fn(&Layout, &Consent, Option<&str>) -> Result<Option<String>, steamcmd::Error>
    + Send
    + Sync;
pub type CmdDownloadFn = dyn Fn(
        &Layout,
        &[(u64, String)],
        &AtomicBool,
        Duration,
        &mut dyn FnMut(usize, usize),
    ) -> Vec<Fetched>
    + Send
    + Sync;

#[derive(Clone)]
pub struct CmdRunner {
    pub install: Arc<InstallFn>,
    pub download: Arc<CmdDownloadFn>,
}

impl CmdRunner {
    pub fn real() -> Self {
        Self {
            install: Arc::new(steamcmd::run::ensure_installed),
            download: Arc::new(steamcmd::run::download),
        }
    }
}

impl fmt::Debug for CmdRunner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CmdRunner")
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
    /// A mod that arrived but was not kept: its name and why, in plain words.
    mismatched: Vec<(String, String)>,
}

/// The mods to get: (Workshop id, the package id the pack names), and their display names.
fn wanted_and_names(preview: &PackPreview) -> (Vec<(u64, String)>, BTreeMap<u64, String>) {
    let wanted = preview
        .missing
        .iter()
        .filter_map(|m| Some((m.workshop_id?, m.package_id.clone())))
        .collect();
    let names = preview
        .missing
        .iter()
        .filter_map(|m| Some((m.workshop_id?, m.name.clone())))
        .collect();
    (wanted, names)
}

fn summarise(outcomes: &[Fetched], names: &BTreeMap<u64, String>) -> Summary {
    let name = |id: u64| names.get(&id).cloned().unwrap_or_else(|| id.to_string());
    let mut summary = Summary::default();
    for outcome in outcomes {
        match &outcome.status {
            Status::Done(Fresh::New) => summary.subscribed += 1,
            Status::Done(Fresh::Already) => summary.already += 1,
            Status::Failed(why) => summary.failed.push((name(outcome.id), why.clone())),
            Status::Mismatch { expected, found } => summary
                .mismatched
                .push((name(outcome.id), t::pack_mismatch_reason(expected, found))),
        }
    }
    summary
}

/// Makes the new profile OFFLINE, but only when every mod arrived and passed the identity
/// check; otherwise it stays as restored (LIVE), and the line says why and what failed (M3).
fn offline_line(
    ctx: &Ctx,
    game: Option<&GameInstall>,
    profile: &Profile,
    s: &Summary,
    still_to_find: usize,
    extra_workshop: Option<&Path>,
) -> String {
    let attempt = (|| -> Result<(), String> {
        if !s.failed.is_empty() || !s.mismatched.is_empty() {
            return Err(t::OFFLINE_NOT_ALL.to_owned());
        }
        let fail = |e: ProfileError| e.to_string();
        let store = ctx.store().map_err(fail)?;
        let effective = ctx
            .effective_game(profile, game)
            .map_err(|e| e.to_string())?;
        let plan =
            offline::plan_offline_with(&store, &profile.id, &effective, false, extra_workshop)
                .map_err(fail)?;
        if !plan.conflicts.is_empty() {
            return Err(t::OFFLINE_CONFLICTS.to_owned());
        }
        if !plan.missing.is_empty() {
            return Err(t::offline_missing(plan.missing.len()));
        }
        offline::apply_offline(&store, &plan, &[], &ProcessProbe, &mut |_| {})
            .map(|_| ())
            .map_err(fail)
    })();
    match attempt {
        Ok(()) => t::pack_downloaded(&profile.name, s.subscribed + s.already, still_to_find),
        Err(why) => t::pack_kept_live(&profile.name, &s.failed, &s.mismatched, &why),
    }
}

impl App {
    /// The actions of this file; `update` hands them over together.
    pub(super) fn pack_fetch_update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::PackOffline(on) => {
                if let Some(Dialog::OpenPack(preview)) = &mut self.dialog {
                    preview.offline = on && preview.steam.is_some();
                }
            }
            Message::PackSubscribe => self.ask_subscribe(),
            Message::PackSteamcmd => self.ask_steamcmd(),
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

    /// "Restore and download…" with steamcmd: the first time, explain what will be downloaded
    /// and run; after that, go straight to the list of mods (T82c).
    fn ask_steamcmd(&mut self) {
        let Some(Dialog::OpenPack(mut preview)) = self.dialog.take() else {
            return;
        };
        if !preview.cmd || preview.missing.is_empty() {
            self.dialog = Some(Dialog::OpenPack(preview));
            return;
        }
        preview.via_cmd = true;
        preview.offline = true;
        let layout = Layout::new(&self.ctx.app_dir);
        let agreed = ConsentStore::open(&self.ctx.app_dir)
            .find(Scope::Steamcmd, layout.root())
            .is_ok_and(|found| found.is_some());
        self.dialog = Some(if agreed {
            Dialog::ConfirmSubscribe(preview)
        } else {
            Dialog::ConsentSteamcmd(preview)
        });
    }

    /// The player agreed to download steamcmd: remember it, then show the mods.
    pub(super) fn grant_steamcmd(&mut self, preview: PackPreview) {
        let layout = Layout::new(&self.ctx.app_dir);
        match ConsentStore::open(&self.ctx.app_dir).record(Scope::Steamcmd, layout.root()) {
            Ok(_) => self.dialog = Some(Dialog::ConfirmSubscribe(Box::new(preview))),
            Err(e) => self.notice = Some(Notice::Error(e.to_string())),
        }
    }

    /// The player trusts Valve's changed file: forget the old fingerprint and go on.
    pub(super) fn trust_steamcmd(&mut self, preview: PackPreview) -> Task<Message> {
        let saved = self.ctx.settings().and_then(|mut settings| {
            settings.steamcmd_sha256 = None;
            settings.save(&self.ctx.app_dir)
        });
        match saved {
            Ok(()) => self.steamcmd_job(preview),
            Err(e) => {
                self.notice = Some(Notice::Error(e.to_string()));
                Task::none()
            }
        }
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
        if preview.via_cmd {
            return self.steamcmd_job(preview);
        }
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
        // OFFLINE downloads private copies and never subscribes; LIVE needs the subscription
        // because the game only loads Workshop mods that Steam says are subscribed (R56).
        let offline = preview.offline;
        let op = if offline { Op::Download } else { Op::Subscribe };
        let game = self.game.as_ref().ok().cloned();
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
                op,
                wanted: &wanted,
                workshop_dir: Some(&route.workshop_dir),
                quiet: fetch::QUIET,
            };
            let outcomes = (fetcher.0)(&route.helper, &request, cancel, &mut |done, total| {
                progress(scaled(done, total, RESTORE_SHARE, TOTAL));
            });
            if offline {
                let s = summarise(&outcomes, &names);
                let line = offline_line(
                    &ctx,
                    game.as_ref(),
                    &report.profile,
                    &s,
                    still_to_find,
                    None,
                );
                return Ok(Outcome::Restored {
                    line,
                    id: report.profile.id,
                });
            }
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

    /// Restore the pack as an OFFLINE profile with its missing mods downloaded by steamcmd
    /// (T82c). steamcmd is installed first, so a changed Valve file stops everything before a
    /// profile is made; its folder is emptied by the download and again afterwards.
    pub(super) fn steamcmd_job(&mut self, preview: PackPreview) -> Task<Message> {
        let (ctx, runner) = (self.ctx.clone(), self.cmd.clone());
        let (wanted, names) = wanted_and_names(&preview);
        let still_to_find = preview.manual.len();
        let game = self.game.as_ref().ok().cloned();
        let again = preview.clone();
        let plan = preview.plan;
        let label = t::opening_pack(&plan.name);
        self.start_cancellable(label, TOTAL, move |progress, cancel| {
            let fail = |e: ProfileError| e.to_string();
            if ProcessProbe.any_game() {
                return Err(ProfileError::GameRunning.to_string());
            }
            let layout = Layout::new(&ctx.app_dir);
            let consent = ConsentStore::open(&ctx.app_dir)
                .find(Scope::Steamcmd, layout.root())
                .map_err(|e| e.to_string())?
                .ok_or_else(|| t::CMD_NO_CONSENT.to_owned())?;
            let mut settings = ctx.settings().map_err(fail)?;
            match (runner.install)(&layout, &consent, settings.steamcmd_sha256.as_deref()) {
                Ok(Some(hash)) => {
                    settings.steamcmd_sha256 = Some(hash);
                    settings.save(&ctx.app_dir).map_err(fail)?;
                }
                Ok(None) => {}
                Err(steamcmd::Error::Changed { first, found }) => {
                    return Ok(Outcome::SteamcmdChanged {
                        preview: Box::new(again),
                        first,
                        found,
                    });
                }
                Err(e) => return Err(t::cmd_failed(&e.to_string(), e.details())),
            }
            let store = ctx.store().map_err(fail)?;
            let bytes = plan.manifest.uncompressed_bytes;
            let report = modpack::apply_unpack(&plan, &store, &mut |done| {
                progress(scaled(done, bytes, 0, RESTORE_SHARE));
            })
            .map_err(fail)?;
            let outcomes = (runner.download)(&layout, &wanted, cancel, CMD_QUIET, &mut |d, n| {
                progress(scaled(d as u64, n as u64, RESTORE_SHARE, TOTAL));
            });
            let s = summarise(&outcomes, &names);
            let content = layout.content_dir();
            let line = offline_line(
                &ctx,
                game.as_ref(),
                &report.profile,
                &s,
                still_to_find,
                Some(&content),
            );
            // Whatever the result, the profile holds its own copies now, or the download
            // failed: either way steamcmd's copies are not kept (T82 review M2).
            let _ = layout.clear_content();
            Ok(Outcome::Restored {
                line,
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
                let failed: Vec<String> = outcomes
                    .iter()
                    .filter_map(|o| match &o.status {
                        Status::Done(_) => None,
                        Status::Failed(why) => Some(why.clone()),
                        // Unsubscribing has no identity check, so this does not happen.
                        Status::Mismatch { .. } => Some("not the mod the pack named".to_owned()),
                    })
                    .collect();
                record::remove(&dir, &done).map_err(|e| e.to_string())?;
                Ok(Outcome::Changed(t::pack_unsubscribed(done.len(), &failed)))
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
    let count = preview.missing.len();
    // OFFLINE downloads private copies and does not subscribe; the wording says which.
    let (title, about, keeps, go) = if preview.via_cmd {
        (
            t::CMD_DOWNLOAD_TITLE,
            t::cmd_download_about(count),
            t::CMD_DOWNLOAD_KEEPS,
            t::DOWNLOAD_IT,
        )
    } else if preview.offline {
        (
            t::DOWNLOAD_TITLE,
            t::download_about(count),
            t::DOWNLOAD_KEEPS,
            t::DOWNLOAD_IT,
        )
    } else {
        (
            t::SUBSCRIBE_TITLE,
            t::subscribe_about(count),
            t::SUBSCRIBE_KEEPS,
            t::SUBSCRIBE_IT,
        )
    };
    column![
        heading(title),
        text(about),
        container(scrollable(names)).max_height(NAMES_HEIGHT),
        text(t::SUBSCRIBE_TRUST)
            .size(13)
            .font(UI_BOLD)
            .style(theme::warning),
        text(keeps).size(13),
        buttons(go, Some(Message::Confirm), false),
    ]
    .spacing(10)
    .into()
}

/// First use of steamcmd: exactly what will be downloaded and run, before anything is.
pub fn consent_steamcmd_dialog() -> Element<'static, Message> {
    let mut points = Column::new().spacing(6);
    for point in t::CMD_CONSENT_POINTS {
        points = points.push(text(point).size(14));
    }
    if let Some(note) = t::cmd_platform_note(cfg!(target_os = "macos")) {
        points = points.push(text(note).size(14).font(UI_BOLD).style(theme::warning));
    }
    column![
        heading(t::CMD_CONSENT_TITLE),
        text(t::CMD_CONSENT_ABOUT),
        points,
        buttons(t::CMD_CONSENT_GO, Some(Message::Confirm), false),
    ]
    .spacing(10)
    .into()
}

/// Valve's file is not the one first seen: both fingerprints, and the choice.
pub fn trust_steamcmd_dialog<'a>(first: &str, found: &str) -> Element<'a, Message> {
    column![
        heading(t::CMD_CHANGED_TITLE),
        text(t::cmd_changed_about(first, found)).size(14),
        buttons(t::CMD_TRUST_IT, Some(Message::Confirm), true),
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
            [("4".to_owned(), t::pack_mismatch_reason("a", "b"))],
            "an id with no name falls back to the number"
        );
    }
}
