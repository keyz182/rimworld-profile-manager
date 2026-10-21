//! Modpacks: "Share as modpack…". The core gathers the mod list and writes the file
//! (limits, the allowlist, the checks on every string); this file asks, shows progress,
//! reports. A pack holds no mod files and no game files.

use std::path::{Path, PathBuf};

use iced::Task;
use iced::widget::{image, markdown, text_editor};
use rwpm_core::ProfileError;
use rwpm_core::modpack::file::{
    MAX_DESCRIPTION_CHARS, MAX_NAME_CHARS, check_link, check_save_name,
};
use rwpm_core::modpack::{self, Includes, ModEntry, PackSpec, cover};
use rwpm_core::profile::copy_settings::{Group, entries};
use rwpm_core::profile::{ProcessProbe, ProfileId};

use super::message::{Dialog, Message, Outcome};
use super::{App, Notice, text as t};

/// A settings box in the Share dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShareTick {
    ModSettings,
    Creations,
    GameSettings,
}

/// What a ticked group would put in: an upper bound, since files that are not plain data
/// are left out when the pack is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Count {
    pub files: u64,
    pub bytes: u64,
}

/// The two groups whose items can be picked one by one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickGroup {
    ModSettings,
    Creations,
}

/// One setting entry or creation file a pack could hold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PickItem {
    /// The name the pack lists it by.
    pub name: String,
    /// What the player reads.
    pub label: String,
    pub bytes: u64,
    pub on: bool,
}

/// The items of one group, and whether their list is shown.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Picks {
    pub items: Vec<PickItem>,
    pub open: bool,
}

impl Picks {
    pub fn count(&self) -> Count {
        let on = self.items.iter().filter(|i| i.on);
        Count {
            files: on.clone().count() as u64,
            bytes: on.map(|i| i.bytes).sum(),
        }
    }

    /// `None` when everything is in, which is how a pack that picks nothing narrower is
    /// written; else the names that are.
    fn only(&self) -> Option<Vec<String>> {
        if self.items.iter().all(|i| i.on) {
            return None;
        }
        Some(
            self.items
                .iter()
                .filter(|i| i.on)
                .map(|i| i.name.clone())
                .collect(),
        )
    }

    fn set_all(&mut self, on: bool) {
        self.items.iter_mut().for_each(|i| i.on = on);
    }
}

/// One save file the profile has.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveChoice {
    pub name: String,
    pub bytes: u64,
    pub on: bool,
}

/// The description box. `Content` cannot be cloned, so a clone is a new box with the same
/// words; only the cursor is lost.
#[derive(Debug, Default)]
pub struct Editor(pub text_editor::Content);

impl Clone for Editor {
    fn clone(&self) -> Self {
        Self(text_editor::Content::with_text(&self.0.text()))
    }
}

/// A cover picture chosen for the pack, already checked and shrunk by the core.
#[derive(Debug, Clone)]
pub struct ShareCover {
    pub png: Vec<u8>,
    pub handle: image::Handle,
    pub file_name: String,
}

/// The Share dialog's state.
#[derive(Debug, Clone)]
pub struct ShareDraft {
    pub id: ProfileId,
    pub name: String,
    pub mod_settings: bool,
    pub creations: bool,
    pub game_settings: bool,
    pub settings_picks: Picks,
    pub creations_picks: Picks,
    pub game_settings_count: Count,
    pub saves: Vec<SaveChoice>,
    /// The save to start from: an index into `saves`, always a ticked one.
    pub start: Option<usize>,
    /// The words in `editor`, kept in step with it.
    pub description: String,
    pub editor: Editor,
    /// `Some` while the description is shown as it will be drawn; parsed when switched on.
    pub preview: Option<Vec<markdown::Item>>,
    pub cover: Option<ShareCover>,
    pub link: String,
}

impl ShareDraft {
    pub fn name_too_long(&self) -> bool {
        self.name.chars().count() > MAX_NAME_CHARS
    }

    pub fn description_too_long(&self) -> bool {
        self.description.chars().count() > MAX_DESCRIPTION_CHARS
    }

    /// Empty is fine: the link is optional.
    pub fn link_ok(&self) -> bool {
        let link = self.link.trim();
        link.is_empty() || check_link(link).is_ok()
    }

    pub fn ready(&self) -> bool {
        !self.name_too_long() && !self.description_too_long() && self.link_ok()
    }

    pub(super) fn includes(&self) -> Includes {
        Includes {
            mod_settings: self.mod_settings,
            creations: self.creations,
            game_settings: self.game_settings,
            mod_settings_only: self
                .mod_settings
                .then(|| self.settings_picks.only())
                .flatten(),
            creations_only: self
                .creations
                .then(|| self.creations_picks.only())
                .flatten(),
            saves: self
                .saves
                .iter()
                .filter(|s| s.on)
                .map(|s| s.name.clone())
                .collect(),
        }
    }

    fn picks_mut(&mut self, group: PickGroup) -> &mut Picks {
        match group {
            PickGroup::ModSettings => &mut self.settings_picks,
            PickGroup::Creations => &mut self.creations_picks,
        }
    }

    fn start_name(&self) -> Option<String> {
        let save = self.saves.get(self.start?)?;
        save.on.then(|| save.name.clone())
    }

    /// What the progress bar counts to: an estimate; the core counts the real bytes.
    fn total(&self) -> u64 {
        let groups = [
            (self.mod_settings, self.settings_picks.count()),
            (self.creations, self.creations_picks.count()),
            (self.game_settings, self.game_settings_count),
        ];
        let settings: u64 = groups.iter().filter(|g| g.0).map(|g| g.1.bytes).sum();
        let saves: u64 = self.saves.iter().filter(|s| s.on).map(|s| s.bytes).sum();
        settings + saves
    }
}

impl App {
    /// The actions of this file; `update` hands them over together.
    pub(super) fn modpack_update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Share => self.ask_share(),
            Message::ShareTick(tick, on) => self.share_tick(tick, on),
            Message::ShareSave(index, on) => self.share_save(index, on),
            Message::SharePickOpen(group, on) => self.share_pick_open(group, on),
            Message::SharePick(group, index, on) => self.share_pick(group, index, on),
            Message::ShareStart(index) => self.share_start(index),
            Message::SharePicked(draft, Some(path)) => return self.run_share(*draft, path),
            Message::ShareEdit(action) => self.share_edit(action),
            Message::SharePreview(on) => self.share_preview(on),
            Message::ShareCover => {
                return Task::perform(pick_cover(), Message::ShareCoverPicked);
            }
            Message::ShareCoverPicked(Some(picked)) => self.share_cover(picked),
            Message::ShareCoverClear => self.set_cover(None),
            _ => {}
        }
        Task::none()
    }

    fn ask_share(&mut self) {
        let Some(p) = self.current() else { return };
        let save_data = p.save_data_dir();
        if !save_data.join("Config").join("ModsConfig.xml").is_file() {
            let words = ProfileError::NoModList(p.name.clone()).to_string();
            self.notice = Some(Notice::Error(words));
            return;
        }
        let mods = self.known_mods(p);
        let found = match modpack::pick_candidates(&save_data) {
            Ok(found) => found,
            Err(e) => {
                self.notice = Some(Notice::Error(e.to_string()));
                return;
            }
        };
        let draft = ShareDraft {
            id: p.id.clone(),
            name: p.name.clone(),
            mod_settings: true,
            creations: true,
            game_settings: false,
            settings_picks: picks_of(&found.mod_settings, |n| setting_label(n, &mods)),
            creations_picks: picks_of(&found.creations, creation_label),
            game_settings_count: count_group(&save_data, Group::GameSettings),
            saves: list_saves(&save_data),
            start: None,
            description: String::new(),
            editor: Editor::default(),
            preview: None,
            cover: None,
            link: String::new(),
        };
        self.dialog = Some(Dialog::Share(Box::new(draft)));
    }

    /// The mods the profile has on, for naming settings files. Best effort: with no game or
    /// no mod list the files are shown by their own names.
    fn known_mods(&self, p: &rwpm_core::profile::Profile) -> Vec<ModEntry> {
        let Ok(game) = self.ctx.effective_game(p, self.game.as_ref().ok()) else {
            return Vec::new();
        };
        modpack::gather_mods(p, &game).map_or_else(|_| Vec::new(), |g| g.mods)
    }

    fn share_tick(&mut self, tick: ShareTick, on: bool) {
        let Some(Dialog::Share(draft)) = &mut self.dialog else {
            return;
        };
        match tick {
            // Ticking a group takes everything in it again.
            ShareTick::ModSettings => {
                draft.mod_settings = on;
                if on {
                    draft.settings_picks.set_all(true);
                }
            }
            ShareTick::Creations => {
                draft.creations = on;
                if on {
                    draft.creations_picks.set_all(true);
                }
            }
            ShareTick::GameSettings => draft.game_settings = on,
        }
    }

    fn share_pick_open(&mut self, group: PickGroup, on: bool) {
        if let Some(Dialog::Share(draft)) = &mut self.dialog {
            draft.picks_mut(group).open = on;
        }
    }

    /// Unticking the last item unticks the group: a group with nothing in it is no group.
    fn share_pick(&mut self, group: PickGroup, index: usize, on: bool) {
        let Some(Dialog::Share(draft)) = &mut self.dialog else {
            return;
        };
        let picks = draft.picks_mut(group);
        let Some(item) = picks.items.get_mut(index) else {
            return;
        };
        item.on = on;
        let empty = picks.items.iter().all(|i| !i.on);
        if empty {
            picks.open = false;
            match group {
                PickGroup::ModSettings => draft.mod_settings = false,
                PickGroup::Creations => draft.creations = false,
            }
        }
    }

    fn share_save(&mut self, index: usize, on: bool) {
        let Some(Dialog::Share(draft)) = &mut self.dialog else {
            return;
        };
        let Some(save) = draft.saves.get_mut(index) else {
            return;
        };
        save.on = on;
        if !on && draft.start == Some(index) {
            draft.start = None;
        }
    }

    fn share_edit(&mut self, action: text_editor::Action) {
        let Some(Dialog::Share(draft)) = &mut self.dialog else {
            return;
        };
        draft.editor.0.perform(action);
        draft.description = draft.editor.0.text().trim_end_matches('\n').to_owned();
    }

    fn share_preview(&mut self, on: bool) {
        let Some(Dialog::Share(draft)) = &mut self.dialog else {
            return;
        };
        draft.preview = on.then(|| markdown::parse(&draft.description).collect());
    }

    fn share_cover(&mut self, picked: Result<Box<ShareCover>, String>) {
        match picked {
            Ok(cover) => self.set_cover(Some(*cover)),
            Err(words) => self.notice = Some(Notice::Error(words)),
        }
    }

    fn set_cover(&mut self, cover: Option<ShareCover>) {
        if let Some(Dialog::Share(draft)) = &mut self.dialog {
            draft.cover = cover;
        }
    }

    fn share_start(&mut self, index: Option<usize>) {
        let Some(Dialog::Share(draft)) = &mut self.dialog else {
            return;
        };
        draft.start = index.filter(|i| draft.saves.get(*i).is_some_and(|s| s.on));
    }

    /// The Share dialog's button: ask where to save the file.
    pub(super) fn pick_share_save(&self, draft: ShareDraft) -> Task<Message> {
        let name = format!(
            "{}.{}",
            ProfileId::slug_base(&draft.name),
            t::PACK_EXTENSION
        );
        Task::perform(
            pick_pack_save(t::PICK_SHARE_SAVE_TITLE, name),
            move |path| Message::SharePicked(Box::new(draft.clone()), path),
        )
    }

    fn run_share(&mut self, draft: ShareDraft, path: PathBuf) -> Task<Message> {
        let Some(p) = self.profiles.iter().find(|p| p.id == draft.id).cloned() else {
            return Task::none();
        };
        let game = match self.ctx.effective_game(&p, self.game.as_ref().ok()) {
            Ok(game) => game,
            Err(e) => {
                self.notice = Some(Notice::Error(e.to_string()));
                return Task::none();
            }
        };
        let path = super::backup::with_extension(path, t::PACK_EXTENSION);
        let (ctx, total) = (self.ctx.clone(), draft.total());
        self.start(t::sharing(&p.name), total, move |progress| {
            let fail = |e: ProfileError| e.to_string();
            let store = ctx.store().map_err(fail)?;
            let gathered = modpack::gather_mods(&p, &game).map_err(fail)?;
            let link = draft.link.trim();
            let spec = PackSpec {
                description: draft.description.trim().to_owned(),
                game_version: game.version.clone(),
                dlc: gathered.dlc,
                includes: draft.includes(),
                start_save: draft.start_name(),
                collection_url: (!link.is_empty()).then(|| link.to_owned()),
                cover: draft.cover.as_ref().map(|c| c.png.clone()),
                mods: gathered.mods,
            };
            let plan = modpack::plan_pack(&store, &p.id, spec).map_err(fail)?;
            let report =
                modpack::apply_pack(&plan, &path, &ProcessProbe, progress).map_err(fail)?;
            Ok(Outcome::Changed(t::shared(
                &plan.name,
                &path.display().to_string(),
                report.archive_bytes,
                gathered.skipped.len(),
                plan.skipped.len(),
            )))
        })
    }
}

fn picks_of(found: &[modpack::Candidate], label: impl Fn(&str) -> String) -> Picks {
    let items = found
        .iter()
        .map(|c| PickItem {
            name: c.name.clone(),
            label: label(&c.name),
            bytes: c.bytes,
            on: true,
        })
        .collect();
    Picks { items, open: false }
}

/// A settings file as the player reads it. The game names a mod's file `Mod_<Workshop
/// id>_<class>.xml`, so the id gives the mod's name; any other file keeps its own name.
pub(crate) fn setting_label(name: &str, mods: &[ModEntry]) -> String {
    let id = name
        .strip_prefix("Mod_")
        .and_then(|rest| rest.split('_').next())
        .and_then(|digits| digits.parse::<u64>().ok());
    match mods.iter().find(|m| id.is_some() && m.workshop_id == id) {
        Some(m) => format!("{} ({name})", m.name),
        None => name.to_owned(),
    }
}

/// A creation by its file name; the folder it is in is shown above it.
fn creation_label(name: &str) -> String {
    name.split_once('/')
        .map_or(name, |(_, file)| file)
        .to_owned()
}

/// The folder a creation is in, such as `Scenarios`.
pub(crate) fn creation_folder(name: &str) -> &str {
    name.split_once('/').map_or("", |(folder, _)| folder)
}

/// Files and bytes a group would put in, counted the way the core walks: links are skipped.
fn count_group(save_data: &Path, group: Group) -> Count {
    let mut count = Count::default();
    for rel in entries(save_data, &[group]) {
        walk(&save_data.join(rel), &mut count);
    }
    count
}

fn walk(path: &Path, count: &mut Count) {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return;
    };
    if meta.is_dir() {
        if let Ok(read) = std::fs::read_dir(path) {
            for entry in read.filter_map(Result::ok) {
                walk(&entry.path(), count);
            }
        }
    } else if meta.is_file() {
        count.files += 1;
        count.bytes += meta.len();
    }
}

/// The save files a pack may hold, by name, none ticked.
fn list_saves(save_data: &Path) -> Vec<SaveChoice> {
    let Ok(read) = std::fs::read_dir(save_data.join("Saves")) else {
        return Vec::new();
    };
    let mut saves: Vec<SaveChoice> = read
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let meta = entry.metadata().ok().filter(|m| m.is_file())?;
            let name = entry.file_name().into_string().ok()?;
            check_save_name(&name).ok()?;
            Some(SaveChoice {
                name,
                bytes: meta.len(),
                on: false,
            })
        })
        .collect();
    saves.sort_by(|a, b| a.name.cmp(&b.name));
    saves
}

/// Asks for a picture, then has the core check and shrink it. The core refuses anything that
/// is not a PNG or JPEG, or is too big to decode safely.
async fn pick_cover() -> Option<Result<Box<ShareCover>, String>> {
    let file = rfd::AsyncFileDialog::new()
        .set_title(t::PICK_COVER_TITLE)
        .add_filter(t::COVER_FILTER, &["png", "jpg", "jpeg"])
        .pick_file()
        .await?;
    Some(load_cover(file.path()).map(Box::new))
}

pub(crate) fn load_cover(path: &Path) -> Result<ShareCover, String> {
    let source = cover::load_source(path).map_err(|e| e.to_string())?;
    let png = cover::prepare(&source)?;
    Ok(ShareCover {
        handle: image::Handle::from_bytes(png.clone()),
        file_name: path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        png,
    })
}

async fn pick_pack_save(title: &'static str, name: String) -> Option<PathBuf> {
    rfd::AsyncFileDialog::new()
        .set_title(title)
        .add_filter(t::PACK_FILTER, &[t::PACK_EXTENSION])
        .set_file_name(name)
        .save_file()
        .await
        .map(|f| f.path().to_path_buf())
}
