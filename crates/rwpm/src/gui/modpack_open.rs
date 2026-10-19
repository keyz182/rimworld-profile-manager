//! Modpacks (T81b): "Open modpack…". The core reads the manifest and restores the profile
//! (every entry is checked); this file checks the mod list against what is installed, asks,
//! shows progress, reports. A pack's own links are shown as text. Only a Steam page built
//! from a Workshop id becomes a button (H1).

use std::path::{Path, PathBuf};

use iced::widget::{Column, button, column, container, row, scrollable, text, text_input};
use iced::{Alignment, Element, Length, Task};
use rwpm_core::ProfileError;
use rwpm_core::builds::acf;
use rwpm_core::discovery::GameInstall;
use rwpm_core::modpack::{self, ModEntry, UnpackPlan};
use rwpm_core::mods::ModIndex;

use super::dialogs::{buttons, heading};
use super::icon::UI_BOLD;
use super::message::{Dialog, Field, Message, Outcome};
use super::{App, Notice, support, text as t, theme};

/// Rows drawn per list; the rest is "and N more", so a huge pack cannot slow the window.
const MAX_ROWS: usize = 200;
const STEAM_PAGE: &str = "https://steamcommunity.com/sharedfiles/filedetails/?id=";
/// The part of the Open dialog that scrolls. In the smallest window (600 px high, V33) the
/// title, the name field, the notice and the buttons need about 340 px of it.
const DETAILS_HEIGHT: f32 = 240.0;

/// One mod of the pack, for the preview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackMod {
    pub name: String,
    pub package_id: String,
    pub workshop_id: Option<u64>,
    /// The pack's own link: shown as text, never opened.
    pub url: Option<String>,
}

impl From<&ModEntry> for PackMod {
    fn from(entry: &ModEntry) -> Self {
        Self {
            name: entry.name.clone(),
            package_id: entry.package_id.clone(),
            workshop_id: entry.workshop_id,
            url: entry.url.clone(),
        }
    }
}

/// A pack's contents set against what is installed here.
#[derive(Debug, Clone)]
pub struct PackPreview {
    /// The core's plan; `plan.name` is the new profile's name, which the player may change.
    pub plan: UnpackPlan,
    pub present: Vec<PackMod>,
    /// Not installed, with a Workshop id: a Steam page can be opened for each.
    pub missing: Vec<PackMod>,
    /// Not installed, no Workshop id: the player has to find them.
    pub manual: Vec<PackMod>,
    pub missing_dlc: Vec<String>,
    /// Installed mods that changed on Steam since the pack was made.
    pub newer: Vec<String>,
    pub game_note: Option<String>,
    /// False when no game was found: nothing could be checked.
    pub checked: bool,
}

impl PackPreview {
    pub fn ready(&self) -> bool {
        !self.plan.name.trim().is_empty()
    }

    fn still_needed(&self) -> usize {
        self.missing.len() + self.manual.len()
    }
}

/// Sorts the pack's mods into present, to-get and to-find, and notes what differs.
pub fn build_preview(plan: UnpackPlan, game: Option<&GameInstall>) -> PackPreview {
    let steam = game.and_then(|g| g.steam.as_ref());
    let index = game.map(|g| {
        let workshop = steam.map(|s| s.workshop_dir.as_path());
        ModIndex::scan(&g.data_dir(), &g.mods_dir(), workshop)
    });
    let times = steam
        .and_then(|s| acf::workshop_manifest_path(&s.workshop_dir))
        .map(|path| acf::read_workshop_times(&path))
        .unwrap_or_default();
    let manifest = &plan.manifest;
    let installed = |id: &str| index.as_ref().is_some_and(|i| i.find(id).is_some());
    let (mut present, mut missing, mut manual, mut newer) = (vec![], vec![], vec![], vec![]);
    for entry in &manifest.mods {
        let line = PackMod::from(entry);
        if installed(&entry.package_id) {
            let now = entry.workshop_id.and_then(|id| times.get(&id));
            if matches!((now, entry.time_updated), (Some(now), Some(then)) if *now > then) {
                newer.push(line.name.clone());
            }
            present.push(line);
        } else if entry.workshop_id.is_some() {
            missing.push(line);
        } else {
            manual.push(line);
        }
    }
    let missing_dlc = match &index {
        Some(index) => manifest
            .dlc
            .iter()
            .filter(|id| index.find(id).is_none())
            .cloned()
            .collect(),
        None => Vec::new(),
    };
    let here = game.and_then(|g| g.version.as_deref());
    let game_note = version_note(manifest.game_version.as_deref(), here);
    PackPreview {
        checked: index.is_some(),
        plan,
        present,
        missing,
        manual,
        missing_dlc,
        newer,
        game_note,
    }
}

/// `1.6.4871 rev598` → (1, 6). A change of the first two numbers is worth a word.
fn minor(version: &str) -> Option<(u32, u32)> {
    let mut parts = version.split_whitespace().next()?.split('.');
    Some((parts.next()?.parse().ok()?, parts.next()?.parse().ok()?))
}

fn version_note(pack: Option<&str>, here: Option<&str>) -> Option<String> {
    let (pack, here) = (pack?, here?);
    (minor(pack)? != minor(here)?).then(|| t::pack_version_differs(pack, here))
}

impl App {
    /// The actions of this file; `update` hands them over together.
    pub(super) fn pack_update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::OpenPack => {
                return Task::perform(
                    pick_pack_file(t::PICK_PACK_OPEN_TITLE),
                    Message::OpenPackPicked,
                );
            }
            Message::OpenPackPicked(Some(path)) => return self.open_pack(&path),
            Message::PackOpened(preview) => self.dialog = Some(Dialog::OpenPack(preview)),
            Message::OpenUrl(url) => {
                if let Err(e) = support::open_url(&url) {
                    self.notice = Some(Notice::Error(t::open_page_failed(&e.to_string())));
                }
            }
            _ => {}
        }
        Task::none()
    }

    /// Reads a pack's manifest, then checks its mods off the UI thread. Also how
    /// `rwpm <file>.rimpack` starts the window.
    pub(super) fn open_pack(&mut self, path: &Path) -> Task<Message> {
        let opened = self
            .ctx
            .store()
            .and_then(|store| modpack::plan_unpack(path, &store));
        match opened {
            Ok(plan) => {
                let game = self.game.as_ref().ok().cloned();
                Task::perform(
                    async move { Box::new(build_preview(plan, game.as_ref())) },
                    Message::PackOpened,
                )
            }
            Err(e) => {
                self.notice = Some(Notice::Error(e.to_string()));
                Task::none()
            }
        }
    }

    pub(super) fn pack_restore_job(&mut self, preview: PackPreview) -> Task<Message> {
        let ctx = self.ctx.clone();
        let still_needed = preview.still_needed();
        let plan = preview.plan;
        let total = plan.manifest.uncompressed_bytes;
        self.start(t::opening_pack(&plan.name), total, move |progress| {
            let fail = |e: ProfileError| e.to_string();
            let store = ctx.store().map_err(fail)?;
            let report = modpack::apply_unpack(&plan, &store, progress).map_err(fail)?;
            Ok(Outcome::Restored {
                line: t::pack_restored(&report.profile.name, still_needed),
                id: report.profile.id,
            })
        })
    }
}

/// How one row is drawn.
#[derive(Debug, Clone, Copy)]
enum Row {
    /// A button to the Steam page, built from the Workshop id.
    Page,
    /// The pack's own link, as text.
    Text,
    Plain,
}

/// The pack, its notes and its lists, before the player agrees.
pub fn pack_dialog(preview: &PackPreview) -> Element<'_, Message> {
    let m = &preview.plan.manifest;
    let mut details = Column::new().spacing(10);
    let mut notes = Vec::new();
    if !preview.checked {
        notes.push(t::PACK_NO_GAME.to_owned());
    }
    notes.extend(preview.game_note.clone());
    if !preview.missing_dlc.is_empty() {
        notes.push(t::pack_dlc_missing(&preview.missing_dlc));
    }
    if !preview.newer.is_empty() {
        notes.push(t::pack_newer(&preview.newer));
    }
    for note in notes {
        details = details.push(text(note).size(13).style(theme::warning));
    }
    details = details.push(lists(preview));
    if preview.still_needed() > 0 {
        details = details.push(text(t::PACK_HINT).size(13));
    }
    let ok = preview.ready().then_some(Message::Confirm);
    // The title, the name and the buttons stay on screen; the notes and lists scroll (V33).
    column![
        heading(t::OPEN_PACK_TITLE),
        text(t::pack_about(&m.name, m.mods.len(), &m.description)),
        text(t::RESTORE_NAME),
        text_input(t::NAME_HINT, &preview.plan.name)
            .padding(8)
            .on_input(|v| Message::Input(Field::Name, v))
            .on_submit(Message::Confirm),
        container(scrollable(details)).max_height(DETAILS_HEIGHT),
        text(t::SHARE_NOTICE).size(13),
        buttons(t::OPEN_PACK_IT, ok, false),
    ]
    .spacing(10)
    .into()
}

fn lists(preview: &PackPreview) -> Element<'_, Message> {
    column![
        section(t::PACK_MISSING, &preview.missing, Row::Page),
        section(t::PACK_MANUAL, &preview.manual, Row::Text),
        section(t::PACK_PRESENT, &preview.present, Row::Plain),
    ]
    .spacing(10)
    .into()
}

fn section<'a>(title: &str, rows: &'a [PackMod], kind: Row) -> Element<'a, Message> {
    if rows.is_empty() {
        return Column::new().into();
    }
    let mut list = Column::new()
        .spacing(4)
        .push(text(t::pack_section(title, rows.len())).font(UI_BOLD));
    for m in rows.iter().take(MAX_ROWS) {
        list = list.push(mod_row(m, kind));
    }
    if rows.len() > MAX_ROWS {
        let more = t::and_more(rows.len() - MAX_ROWS);
        list = list.push(text(more).size(13).style(theme::muted));
    }
    list.into()
}

fn mod_row(m: &PackMod, kind: Row) -> Element<'_, Message> {
    let mut label = column![
        text(&m.name),
        text(&m.package_id).size(12).style(theme::muted)
    ];
    if let (Row::Text, Some(url)) = (kind, &m.url) {
        label = label.push(text(url).size(12).style(theme::muted));
    }
    let Row::Page = kind else {
        return label.into();
    };
    let page = m
        .workshop_id
        .map(|id| Message::OpenUrl(format!("{STEAM_PAGE}{id}")));
    row![
        label.width(Length::Fill),
        button(text(t::OPEN_STEAM_PAGE))
            .padding([4, 10])
            .style(theme::ghost)
            .on_press_maybe(page)
    ]
    .align_y(Alignment::Center)
    .into()
}

async fn pick_pack_file(title: &'static str) -> Option<PathBuf> {
    rfd::AsyncFileDialog::new()
        .set_title(title)
        .add_filter(t::PACK_FILTER, &[t::PACK_EXTENSION])
        .pick_file()
        .await
        .map(|f| f.path().to_path_buf())
}
