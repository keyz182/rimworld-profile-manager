//! Messages and dialogs of the GUI state machine.

use std::path::PathBuf;

use rwpm_core::backup::{Includes, RestorePlan};
use rwpm_core::builds::capture::CapturePlan;
use rwpm_core::profile::ProfileId;
use rwpm_core::profile::guard::GuardReport;
use rwpm_core::profile::offline::OfflinePlan;
use rwpm_core::profile::relocate::MovePlan;

use super::modpack::{ShareCover, ShareDraft, ShareTick};
use super::modpack_open::PackPreview;

#[derive(Debug, Clone)]
pub enum Message {
    Select(ProfileId),
    New,
    Clone,
    Rename,
    Delete,
    GoOffline,
    Refresh,
    GoLive,
    Launch,
    Export,
    RemoveLoader,
    OpenSettings,
    OpenLog,
    CopyLog,
    CopyLaunchOptions,
    Help,
    OpenSaves,
    /// Text typed into the profile search.
    Search(String),
    /// Enter in the search: select the first match.
    SearchSubmit,
    /// Open or close the ⋯ More menu.
    ToggleMenu,
    /// A key no widget used (shortcuts, list moves, dialog Enter/Escape).
    Key(iced::keyboard::Key, iced::keyboard::Modifiers),
    /// Whether any RimWorld is running (polled while the window is open).
    GameRunning(bool),
    /// Open a drill-in over the detail pane, or close it (`None`).
    Drill(Option<Drill>),
    /// A profile's mod list, read off the window's thread.
    ModsRead(super::mods::ModList),
    /// Text typed into the mod search.
    ModSearch(String),
    /// Copy settings: the source chosen (`None` = start empty).
    CopyFrom(Option<ProfileId>),
    /// Copy settings: a group ticked or unticked.
    ToggleGroup(rwpm_core::profile::copy_settings::Group, bool),
    /// More → "Copy settings from…".
    OpenCopySettings,
    /// A profile's health check.
    HealthRead(ProfileId, Vec<rwpm_core::profile::health::Problem>),
    /// Health fix: freeze the mods that would load from Steam.
    FreezeUnfrozen,
    /// Health fix: install the Windows loader (asks first).
    InstallLoader,
    /// Open the other layout's profiles folder.
    OpenOtherProfiles,
    /// First-run setup.
    Setup(super::setup::SetupMsg),
    /// Open the system folder dialog for a Settings field.
    Browse(Field),
    /// The folder chosen for a field; `None` when the dialog was closed.
    Picked(Field, Option<PathBuf>),
    /// Text typed into the open dialog's field (`field` picks which one).
    Input(Field, String),
    /// A conflict choice in the OFFLINE dialog: (packageId, chosen folder).
    Choose(String, PathBuf),
    /// The guard dialog's answer before a launch.
    Guard(GuardChoice),
    Confirm,
    Cancel,
    Progress(u64),
    Done(Result<Outcome, String>),
    /// The OS light/dark setting, at start and when it changes.
    Theme(iced::theme::Mode),
    /// The stored game versions, read off the window's thread.
    BuildsRead(Result<super::builds::BuildsView, String>),
    /// Game versions: measure the Steam install, then ask before storing a copy.
    CaptureBuild,
    /// Game versions: ask before moving this stored version to the recycle bin.
    DeleteBuild(String),
    /// Profile row: open the game version picker.
    ChangeGameVersion,
    /// The stored versions, read for the picker.
    PickerRead(Result<super::builds::BuildsView, String>),
    /// Picker: choose a stored version (`Some(key)`) or the Steam install (`None`).
    PickBuild(Option<String>),
    /// More → "Back up…".
    Backup,
    /// Backup dialog: a box ticked or unticked.
    BackupTick(super::backup::Tick, bool),
    /// The file chosen to save the backup in; `None` when the dialog was closed.
    BackupPicked(ProfileId, Includes, Option<PathBuf>),
    /// "Restore backup…": open the file dialog.
    RestoreBackup,
    /// The backup file chosen to restore; `None` when the dialog was closed.
    RestorePicked(Option<PathBuf>),
    /// More → "Share as modpack…".
    Share,
    /// Share dialog: a settings box ticked or unticked.
    ShareTick(ShareTick, bool),
    /// Share dialog: save number `.0` (in the dialog's list) ticked or unticked.
    ShareSave(usize, bool),
    /// Share dialog: the save to start from, by its number; `None` = no start save.
    ShareStart(Option<usize>),
    /// The file chosen to save the pack in; `None` when the dialog was closed.
    SharePicked(Box<ShareDraft>, Option<PathBuf>),
    /// Share dialog: an edit in the description box.
    ShareEdit(iced::widget::text_editor::Action),
    /// Share dialog: show the description as it will be drawn, or edit it.
    SharePreview(bool),
    /// Share dialog: open the file dialog for a cover picture.
    ShareCover,
    /// The picture chosen (or why it was refused); `None` when the dialog was closed.
    ShareCoverPicked(Option<Result<Box<ShareCover>, String>>),
    /// Share dialog: drop the cover picture.
    ShareCoverClear,
    /// A pack profile's "Reset to pack…".
    ResetPack,
    /// Reset dialog: the box for the pack's saves.
    ResetSaves(bool),
    /// "Open modpack…": open the file dialog.
    OpenPack,
    /// The pack chosen to open; `None` when the dialog was closed.
    OpenPackPicked(Option<PathBuf>),
    /// A pack's contents, checked against what is installed.
    PackOpened(Box<PackPreview>),
    /// A Steam page link in the Open dialog (Steam pages only).
    OpenUrl(String),
    /// Open dialog: "Restore and subscribe…".
    PackSubscribe,
    /// Open dialog: the box "Make it an OFFLINE profile" ticked or unticked.
    PackOffline(bool),
    /// Open dialog: "Restore and download…" through Valve's steamcmd.
    PackSteamcmd,
    /// The Cancel button on the progress bar of a job that can be stopped.
    CancelJob,
    /// More → "Remove subscriptions from the pack".
    RemovePackSubs,
    /// The OK button beside a notice in the footer: the player has read it.
    DismissNotice,
}

/// Views that replace the detail pane, with ‹ Back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Drill {
    SteamGuide,
    Mods,
    GameVersions,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Name,
    GameDir,
    SteamDir,
    MoveTo,
    /// Share dialog: the link to a Steam collection.
    Link,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuardChoice {
    Add,
    TurnOff,
    PlayAnyway,
}

/// What a finished background job hands back.
#[derive(Debug, Clone)]
pub enum Outcome {
    /// Reload the list and show this line.
    Changed(String),
    OfflinePlanned(OfflinePlan),
    MovePlanned(MovePlan),
    /// Fixes are done (guard or loader); continue starting the game.
    ReadyToPlay(ProfileId),
    /// A capture was measured: ask before copying.
    CapturePlanned(Box<CapturePlan>),
    /// A game version was stored, updated or deleted: reload and show this line.
    BuildsChanged(String),
    /// A backup became this new profile: reload, select it and show this line.
    Restored {
        id: ProfileId,
        line: String,
    },
    /// steamcmd's file changed since it was first seen: nothing was restored; ask first.
    SteamcmdChanged {
        preview: Box<PackPreview>,
        first: String,
        found: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameFor {
    New,
    Clone,
    Rename,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ask {
    RimSort,
    /// The Windows loader; `then_play` when asked on the way to Play.
    Loader {
        then_play: bool,
    },
}

#[derive(Debug, Clone)]
pub enum Dialog {
    Name {
        purpose: NameFor,
        value: String,
        /// New profile only: where its settings start from.
        copy: Option<super::copy::SettingsCopy>,
    },
    /// More → "Copy settings from…" into the selected profile.
    CopySettings(super::copy::SettingsCopy),
    ConfirmDelete,
    Offline {
        plan: OfflinePlan,
        choices: Vec<(String, PathBuf)>,
    },
    Guard(GuardReport),
    Consent(Ask),
    Settings {
        game: String,
        /// The typed game folder checked as it changes; `None` while empty (Steam finds it).
        game_check: Option<Result<(), String>>,
        steam: String,
        move_to: String,
    },
    ConfirmMove(MovePlan),
    /// Store (or update) a copy of the Steam install, after showing size and free space.
    ConfirmCapture(Box<CapturePlan>),
    ConfirmDeleteBuild {
        key: String,
        bytes: u64,
    },
    /// The game version picker: (key, label) per stored version, and the current choice.
    PickVersion {
        steam: String,
        builds: Vec<(String, String)>,
        chosen: Option<String>,
    },
    /// The new version is older and the profile has saves.
    ConfirmDowngrade {
        to: Option<String>,
        from: String,
        into: String,
    },
    /// What goes in a backup.
    Backup(Box<super::backup::Draft>),
    /// A backup file's contents and the name of the profile it will become.
    Restore(Box<RestorePlan>),
    /// What goes in a modpack.
    Share(Box<ShareDraft>),
    /// A modpack's contents and the name of the profile it will become.
    OpenPack(Box<PackPreview>),
    /// What opening the pack would subscribe the player to.
    ConfirmSubscribe(Box<PackPreview>),
    /// First use of steamcmd: what will be downloaded and run, asked before anything is.
    ConsentSteamcmd(Box<PackPreview>),
    /// Valve's steamcmd file differs from the one first seen.
    TrustSteamcmd {
        preview: Box<PackPreview>,
        first: String,
        found: String,
    },
    /// Unsubscribe from the mods that a pack subscribed this profile's player to.
    ConfirmUnsubscribe {
        id: ProfileId,
        count: usize,
    },
    /// What a reset to the pack would change.
    ResetPack(Box<rwpm_core::modpack::ResetPlan>),
    Help,
}
