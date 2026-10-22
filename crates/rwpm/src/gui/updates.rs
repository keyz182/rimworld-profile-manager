//! The optional update check: on the player's schedule the window asks GitHub for the
//! newest release and shows a banner when it is newer than this build. The banner links to
//! the release page. For an install that can replace itself (see `InstallKind`) it also has
//! Install update, which downloads and checks the file, and Restart now, which puts it in
//! place and starts it. A copy a package manager owns only gets the link.

use std::fmt;
use std::sync::Arc;

use iced::widget::pick_list;
use iced::{Element, Task};
use rwpm_core::install_kind::InstallKind;
use rwpm_core::self_update::{self, Staged};
use rwpm_core::update_check::{self, Frequency, Release, UpdateState};

use super::message::{Dialog, Message};
use super::{App, Notice, support, text as t};

type CheckFn = dyn Fn() -> Result<Release, String> + Send + Sync;

/// How the window asks GitHub. Tests swap in a fake, so none of them uses the network.
#[derive(Clone)]
pub struct UpdateCheck(pub Arc<CheckFn>);

impl UpdateCheck {
    pub fn real() -> Self {
        Self(Arc::new(|| {
            update_check::latest(update_check::LATEST_URL, true).map_err(|e| e.to_string())
        }))
    }
}

impl fmt::Debug for UpdateCheck {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("UpdateCheck")
    }
}

type DownloadFn = dyn Fn(&Release, &InstallKind) -> Result<Staged, String> + Send + Sync;
type RestartFn = dyn Fn(&Staged) -> Result<(), String> + Send + Sync;

/// How this copy was installed and how it replaces itself. Tests swap in fakes, so none of
/// them downloads anything or touches a file outside its sandbox.
#[derive(Clone)]
pub struct Installer {
    pub kind: InstallKind,
    pub download: Arc<DownloadFn>,
    /// Puts the checked download in place and starts it. The window exits after it returns.
    pub restart: Arc<RestartFn>,
}

impl Installer {
    pub fn real() -> Self {
        Self {
            kind: InstallKind::detect(),
            download: Arc::new(|release, kind| {
                self_update::download(release, kind).map_err(|e| e.to_string())
            }),
            restart: Arc::new(|staged| {
                // The game may have started since the button was enabled.
                if rwpm_core::profile::any_game_running() {
                    return Err(t::GAME_RUNNING.to_owned());
                }
                let handoff = self_update::apply(staged).map_err(|e| e.to_string())?;
                self_update::spawn(&handoff).map_err(|e| t::update_not_started(&e.to_string()))
            }),
        }
    }
}

impl fmt::Debug for Installer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Installer")
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
}

/// Where Install update has got to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum InstallState {
    #[default]
    Idle,
    Downloading,
    /// Downloaded and checked; Restart now puts it in place.
    Ready(Staged),
}

/// A frequency as the pick list shows it.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Choice(Frequency);

impl fmt::Display for Choice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(t::update_frequency(self.0))
    }
}

/// The "how often" pick list, shared by Settings and first-run setup.
pub(super) fn frequency_pick<'a>(
    selected: Frequency,
    on_select: impl Fn(Frequency) -> Message + 'a,
) -> Element<'a, Message> {
    pick_list(
        Frequency::ALL.map(Choice),
        Some(Choice(selected)),
        move |c: Choice| on_select(c.0),
    )
    .into()
}

impl App {
    pub(super) fn updates_update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::UpdateTick => return self.check_updates_if_due(),
            Message::CheckUpdatesNow if !self.update_checking => return self.start_check(true),
            Message::UpdateChecked { manual, result } => self.update_checked(manual, result),
            Message::SetUpdateFrequency(frequency) => {
                if let Some(Dialog::Settings { updates, .. }) = &mut self.dialog {
                    *updates = frequency;
                }
            }
            Message::OpenRelease => self.open_release(),
            Message::DismissUpdate => self.dismiss_update(),
            Message::InstallUpdate => return self.start_install(),
            Message::UpdateDownloaded(result) => self.update_downloaded(result),
            Message::RestartForUpdate => return self.restart_for_update(),
            Message::UpdateInstalled(result) => return self.update_installed(result),
            _ => {}
        }
        Task::none()
    }

    /// True when the banner should offer Install update for the release it announces.
    pub(super) fn can_install_update(&self) -> bool {
        self.announced_update()
            .is_some_and(|r| self_update::can_install(r, &self.installer.kind))
    }

    fn start_install(&mut self) -> Task<Message> {
        if self.install != InstallState::Idle || !self.can_install_update() {
            return Task::none();
        }
        let Some(release) = self.update_found.clone() else {
            return Task::none();
        };
        self.install = InstallState::Downloading;
        let installer = self.installer.clone();
        Task::perform(
            async move { (installer.download)(&release, &installer.kind) },
            Message::UpdateDownloaded,
        )
    }

    fn update_downloaded(&mut self, result: Result<Staged, String>) {
        match result {
            Ok(staged) => self.install = InstallState::Ready(staged),
            Err(e) => {
                self.install = InstallState::Idle;
                self.notice = Some(Notice::Error(t::update_install_failed(&e)));
            }
        }
    }

    fn restart_for_update(&mut self) -> Task<Message> {
        let InstallState::Ready(staged) = &self.install else {
            return Task::none();
        };
        if let Some(why) = self.blocked() {
            self.notice = Some(Notice::Info(why));
            return Task::none();
        }
        let staged = staged.clone();
        // Written before the new version starts, so it can tell whether the install happened.
        self.updates.pending = Some(staged.version().to_string());
        self.save_updates();
        let installer = self.installer.clone();
        Task::perform(
            async move { (installer.restart)(&staged) },
            Message::UpdateInstalled,
        )
    }

    fn update_installed(&mut self, result: Result<(), String>) -> Task<Message> {
        match result {
            Ok(()) => iced::exit(),
            Err(e) => {
                self.updates.pending = None;
                self.save_updates();
                self.install = InstallState::Idle;
                self.notice = Some(Notice::Error(t::update_install_failed(&e)));
                Task::none()
            }
        }
    }

    /// Reads the saved schedule. A file that cannot be read means checking stays off, and
    /// the player is told, since saving would replace it.
    pub(super) fn load_updates(&mut self) {
        match UpdateState::load(&self.ctx.app_dir) {
            Ok(state) => {
                self.updates = state;
                // An earlier run handed over to an install. If this is still the old version,
                // it did not happen, and the player has to know why the app looks unchanged.
                // The marker is cleared either way, and only then is the file written.
                let had_pending = self.updates.pending.is_some();
                if let Some(wanted) = self.updates.take_failed_install(rwpm_core::VERSION) {
                    self.notice = Some(Notice::Error(t::update_not_installed(
                        &wanted,
                        rwpm_core::VERSION,
                    )));
                }
                if had_pending {
                    self.save_updates();
                }
            }
            Err(e) => {
                self.updates = UpdateState::default();
                self.notice = Some(Notice::Error(t::updates_unreadable(&e.to_string())));
            }
        }
    }

    /// The release to announce, if there is one the player has not dismissed.
    pub(super) fn announced_update(&self) -> Option<&Release> {
        self.update_found
            .as_ref()
            .filter(|r| self.updates.shows(r, rwpm_core::VERSION))
    }

    /// Starts a background check when the player's schedule says one is due.
    pub(super) fn check_updates_if_due(&mut self) -> Task<Message> {
        if self.update_checking || !self.updates.is_due(update_check::now_secs()) {
            return Task::none();
        }
        self.start_check(false)
    }

    /// Saves how often to check, then checks at once if that makes one due.
    pub(super) fn set_update_frequency(&mut self, frequency: Frequency) -> Task<Message> {
        if self.updates.frequency == frequency {
            return Task::none();
        }
        self.updates.frequency = frequency;
        self.save_updates();
        self.check_updates_if_due()
    }

    fn start_check(&mut self, manual: bool) -> Task<Message> {
        self.update_checking = true;
        let check = Arc::clone(&self.update_check.0);
        Task::perform(async move { check() }, move |result| {
            Message::UpdateChecked { manual, result }
        })
    }

    fn update_checked(&mut self, manual: bool, result: Result<Release, String>) {
        self.update_checking = false;
        let now = update_check::now_secs();
        match result {
            Ok(release) => {
                let newer = release.version.is_newer_than(rwpm_core::VERSION);
                // Asking again is asking to see it: an earlier "Dismiss" no longer applies.
                if manual && newer {
                    self.updates.dismissed = None;
                }
                self.updates.record_success(now);
                self.save_updates();
                if manual && !newer {
                    self.notice = Some(Notice::Info(t::UP_TO_DATE.into()));
                }
                // A download of an older release is no use once a newer one is out.
                if matches!(&self.install, InstallState::Ready(s) if s.version() != release.version)
                {
                    self.install = InstallState::Idle;
                }
                self.update_found = Some(release);
            }
            Err(e) => {
                self.updates.record_attempt(now);
                self.save_updates();
                // A background check that fails stays quiet; the player did not ask.
                if manual {
                    self.notice = Some(Notice::Error(t::update_check_failed(&e)));
                }
            }
        }
    }

    fn dismiss_update(&mut self) {
        if let Some(release) = &self.update_found {
            self.updates.dismissed = Some(release.version.to_string());
            self.save_updates();
        }
    }

    fn open_release(&mut self) {
        let Some(release) = &self.update_found else {
            return;
        };
        if let Err(e) = support::open_release(&release.url) {
            self.notice = Some(Notice::Error(t::release_open_failed(&e.to_string())));
        }
    }

    fn save_updates(&mut self) {
        if let Err(e) = self.updates.save(&self.ctx.app_dir) {
            self.notice = Some(Notice::Error(e.to_string()));
        }
    }
}
