//! Messages and dialogs of the GUI state machine.

use std::path::PathBuf;

use rwpm_core::builds::capture::CapturePlan;
use rwpm_core::profile::ProfileId;
use rwpm_core::profile::guard::GuardReport;
use rwpm_core::profile::offline::OfflinePlan;
use rwpm_core::profile::relocate::MovePlan;

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
    /// A profile's mod list, read off the window's thread (T29).
    ModsRead(super::mods::ModList),
    /// Text typed into the mod search.
    ModSearch(String),
    /// Copy settings: the source chosen (`None` = start empty).
    CopyFrom(Option<ProfileId>),
    /// Copy settings: a group ticked or unticked.
    ToggleGroup(rwpm_core::profile::copy_settings::Group, bool),
    /// More → "Copy settings from…".
    OpenCopySettings,
    /// A profile's health check (T30).
    HealthRead(ProfileId, Vec<rwpm_core::profile::health::Problem>),
    /// Health fix: freeze the mods that would load from Steam.
    FreezeUnfrozen,
    /// Health fix: install the Windows loader (asks first).
    InstallLoader,
    /// Open the other layout's profiles folder (V31 notice).
    OpenOtherProfiles,
    /// First-run setup (T27).
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
    /// The stored game versions, read off the window's thread (T56).
    BuildsRead(Result<super::builds::BuildsView, String>),
    /// Game versions: measure the Steam install, then ask before storing a copy.
    CaptureBuild,
    /// Game versions: ask before moving this stored version to the recycle bin.
    DeleteBuild(String),
    /// Profile row: open the game version picker (T56).
    ChangeGameVersion,
    /// The stored versions, read for the picker.
    PickerRead(Result<super::builds::BuildsView, String>),
    /// Picker: choose a stored version (`Some(key)`) or the Steam install (`None`).
    PickBuild(Option<String>),
}

/// Views that replace the detail pane, with ‹ Back (T23 layout).
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
    /// A capture was measured: ask before copying (T56).
    CapturePlanned(Box<CapturePlan>),
    /// A game version was stored, updated or deleted: reload and show this line.
    BuildsChanged(String),
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
        /// New profile only: where its settings start from (T33).
        copy: Option<super::copy::SettingsCopy>,
    },
    /// More → "Copy settings from…" into the selected profile (T33).
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
    /// V61: the new version is older and the profile has saves.
    ConfirmDowngrade {
        to: Option<String>,
        from: String,
        into: String,
    },
    Help,
}
