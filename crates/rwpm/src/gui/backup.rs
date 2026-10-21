//! Backups: "Back up…" and "Restore backup…". The core does the work (lock, size
//! limits, staging, safety checks on every entry); this file asks, shows progress, reports.

use std::path::{Path, PathBuf};

use iced::Task;
use rwpm_core::backup::{self, Includes, RestorePlan};
use rwpm_core::builds::BuildKey;
use rwpm_core::profile::{Mode, ProcessProbe, Profile, ProfileId};
use rwpm_core::space;

use super::message::{Dialog, Message, Outcome};
use super::{App, Notice, text as t};

/// A box in the Backup dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tick {
    Saves,
    Logs,
    Build,
}

/// What the profile offers as game files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GameFiles {
    /// The profile plays the Steam install: there are no game files to include.
    NotPinned,
    /// Pinned to a version that is not stored on this computer.
    Missing(String),
    Stored {
        key: String,
        bytes: u64,
    },
}

/// The Backup dialog's state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Draft {
    pub id: ProfileId,
    pub name: String,
    pub includes: Includes,
    pub saves_bytes: u64,
    pub game: GameFiles,
}

impl Draft {
    fn can_include_build(&self) -> bool {
        matches!(self.game, GameFiles::Stored { .. })
    }
}

/// Only a name is needed: the core already refused a backup that cannot fit.
pub fn restore_ok(plan: &RestorePlan) -> bool {
    !plan.name.trim().is_empty()
}

impl App {
    /// The actions of this file; `update` hands them over together.
    pub(super) fn backup_update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Backup => self.ask_backup(),
            Message::BackupTick(tick, on) => self.tick(tick, on),
            Message::BackupPicked(id, includes, Some(path)) => {
                return self.run_backup(&id, includes, path);
            }
            Message::RestoreBackup => {
                return Task::perform(
                    pick_backup_file(t::PICK_BACKUP_OPEN_TITLE),
                    Message::RestorePicked,
                );
            }
            Message::RestorePicked(Some(path)) => self.open_backup(&path),
            _ => {}
        }
        Task::none()
    }

    fn ask_backup(&mut self) {
        let Some(p) = self
            .current()
            .filter(|p| !p.is_default() && p.mode == Mode::Offline)
        else {
            return;
        };
        let game = match p.game_build.as_deref() {
            None => GameFiles::NotPinned,
            Some(key) => match self.stored_size(key) {
                Some(bytes) => GameFiles::Stored {
                    key: key.to_owned(),
                    bytes,
                },
                None => GameFiles::Missing(key.to_owned()),
            },
        };
        self.dialog = Some(Dialog::Backup(Box::new(Draft {
            id: p.id.clone(),
            name: p.name.clone(),
            includes: Includes {
                saves: true,
                logs: false,
                build: false,
            },
            saves_bytes: space::dir_size(&p.save_data_dir()),
            game,
        })));
    }

    fn tick(&mut self, tick: Tick, on: bool) {
        let Some(Dialog::Backup(draft)) = &mut self.dialog else {
            return;
        };
        match tick {
            Tick::Saves => draft.includes.saves = on,
            Tick::Logs => draft.includes.logs = on,
            Tick::Build => draft.includes.build = on && draft.can_include_build(),
        }
    }

    /// Size of a stored game version, `None` when it is not stored.
    fn stored_size(&self, key: &str) -> Option<u64> {
        let key = BuildKey::parse(key).ok()?;
        let store = self.ctx.build_store().ok()?;
        store.get(&key).ok().map(|b| b.file.size)
    }

    /// The Backup dialog's button: ask where to save the file.
    pub(super) fn pick_backup_save(&self, draft: Draft) -> Task<Message> {
        let name = format!(
            "{}.{}",
            ProfileId::slug_base(&draft.name),
            t::BACKUP_EXTENSION
        );
        let (id, includes) = (draft.id, draft.includes);
        Task::perform(
            pick_backup_save(t::PICK_BACKUP_SAVE_TITLE, name),
            move |path| Message::BackupPicked(id.clone(), includes, path),
        )
    }

    fn run_backup(&mut self, id: &ProfileId, includes: Includes, path: PathBuf) -> Task<Message> {
        let Some(p) = self.profiles.iter().find(|p| p.id == *id).cloned() else {
            return Task::none();
        };
        let path = with_extension(path, t::BACKUP_EXTENSION);
        let total = self.backup_total(&p, includes);
        let (ctx, id) = (self.ctx.clone(), p.id.clone());
        self.start(t::backing_up(&p.name), total, move |progress| {
            let fail = |e: rwpm_core::ProfileError| e.to_string();
            let store = ctx.store().map_err(fail)?;
            let builds = ctx.build_store().map_err(fail)?;
            let plan = backup::plan_backup(&store, &builds, &id, includes).map_err(fail)?;
            let report = backup::apply_backup(&plan, &path, &ctx.app_dir, &ProcessProbe, progress)
                .map_err(fail)?;
            Ok(Outcome::Changed(t::backed_up(
                &plan.name,
                &path.display().to_string(),
                report.archive_bytes,
                report.skipped_links.len(),
            )))
        })
    }

    /// What the progress bar counts to: an estimate; the core counts the real bytes.
    fn backup_total(&self, p: &Profile, includes: Includes) -> u64 {
        let dir = |d: Option<PathBuf>| d.map_or(0, |d| space::dir_size(&d));
        let saves = if includes.saves {
            space::dir_size(&p.save_data_dir())
        } else {
            0
        };
        let build = match (includes.build, p.game_build.as_deref()) {
            (true, Some(key)) => self.stored_size(key).unwrap_or(0),
            _ => 0,
        };
        dir(p.mods_dir()) + saves + build
    }

    /// Reads a backup's manifest and asks before restoring it. Also how `rwpm <file>.rimrim`
    /// starts the window.
    pub(super) fn open_backup(&mut self, path: &Path) {
        let opened = self.ctx.store().and_then(|store| {
            let builds = self.ctx.build_store()?;
            backup::plan_restore(path, &store, &builds)
        });
        match opened {
            Ok(plan) => self.dialog = Some(Dialog::Restore(Box::new(plan))),
            Err(e) => self.notice = Some(Notice::Error(e.to_string())),
        }
    }

    pub(super) fn restore_job(&mut self, plan: RestorePlan) -> Task<Message> {
        let ctx = self.ctx.clone();
        let total = plan.manifest.uncompressed_bytes;
        self.start(t::restoring(&plan.name), total, move |progress| {
            let fail = |e: rwpm_core::ProfileError| e.to_string();
            let store = ctx.store().map_err(fail)?;
            let builds = ctx.build_store().map_err(fail)?;
            let report = backup::apply_restore(&plan, &store, &builds, &ProcessProbe, progress)
                .map_err(fail)?;
            Ok(Outcome::Restored {
                line: t::restored(&report.profile.name, &report.notes),
                id: report.profile.id,
            })
        })
    }
}

/// The player may type a name with no extension; the file always gets it.
pub(super) fn with_extension(path: PathBuf, extension: &str) -> PathBuf {
    let has = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case(extension));
    if has {
        return path;
    }
    let mut name = path.into_os_string();
    name.push(".");
    name.push(extension);
    PathBuf::from(name)
}

fn backup_dialog_builder(title: &'static str) -> rfd::AsyncFileDialog {
    rfd::AsyncFileDialog::new()
        .set_title(title)
        .add_filter(t::BACKUP_FILTER, &[t::BACKUP_EXTENSION])
}

async fn pick_backup_save(title: &'static str, name: String) -> Option<PathBuf> {
    backup_dialog_builder(title)
        .set_file_name(name)
        .save_file()
        .await
        .map(|f| f.path().to_path_buf())
}

async fn pick_backup_file(title: &'static str) -> Option<PathBuf> {
    backup_dialog_builder(title)
        .pick_file()
        .await
        .map(|f| f.path().to_path_buf())
}
