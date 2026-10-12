//! The string catalogue: every word the GUI shows, in British English (C: ∀ UI text keyed).
//! Plain words for an average player, no jargon. Values go in through the small functions
//! below, which also handle plurals. Errors raised by rwpm-core keep their own wording.

use std::path::Path;

// Window and list.
pub const TITLE: &str = "RimWorld Profile Manager";
pub const NEW: &str = "New profile";
pub const HELP: &str = "Help";
pub const SETTINGS: &str = "Settings";
pub const BADGE_DEFAULT: &str = "Game's own saves";
pub const SEARCH_HINT: &str = "Search profiles  (Ctrl+F)";
pub const NO_MATCH: &str = "No profile matches the search.";
pub const FIND_GAME: &str = "Find RimWorld…";
pub const MORE: &str = "More";
pub const MODS_TITLE: &str = "Mods";
pub const START_TITLE: &str = "Start from elsewhere";
pub const STEAM: &str = "Steam";
pub const STEAM_CAPTION: &str =
    "Put this line in RimWorld's launch options in Steam. Steam then starts this profile.";
pub const SET_UP_STEAM: &str = "Set up Steam";
pub const STEAM_WORKS: &str = "Steam starts this profile.";
pub const BACK: &str = "Back";
pub const GUIDE_COPY: &str = "Copy this line:";
pub const GUIDE_OPEN: &str =
    "In Steam, open your Library, right-click RimWorld and choose Properties.";
pub const GUIDE_PASTE: &str =
    "On the General tab, paste the line into Launch options. Replace anything already there.";
pub const GUIDE_START: &str = "Start RimWorld from Steam as usual. This page shows when it worked.";
pub const GUIDE_ONE_PROFILE: &str = "Steam starts only the profile on this line. To play another profile from Steam, paste that profile's line instead.";
pub const GUIDE_WAITING: &str = "Waiting for a start from Steam. This updates by itself.";
pub const RIMSORT: &str = "RimSort";
pub const START_FROM: &str = "Start from";
pub const START_EMPTY: &str = "Empty (no settings)";
pub const COPY_NOTE: &str = "Saves are never copied. Files with the same name are replaced; the old ones go to the recycle bin.";
pub const COPY_SETTINGS: &str = "Copy settings";
pub const COPY_SETTINGS_MENU: &str = "Copy settings from…";
pub const JOB_COPYING_SETTINGS: &str = "Copying settings";
pub const ONLY_GAME_CONTENT: &str = "Only the game's own content is turned on here, while Default has more mods. Copy the mod list and settings from Default to start with yours.";
pub const NOTHING_TO_COPY: &str =
    "That profile has none of the chosen settings yet, so nothing was copied.";
pub const HEALTH_TITLE: &str = "Health";
pub const HEALTH_OK: &str = "No problems found.";
pub const FIX_FREEZE: &str = "Freeze them";
pub const FIX_LOADER: &str = "Install loader…";
pub const FIX_RIMSORT: &str = "Add to RimSort again";
pub const LOADER_MISSING: &str =
    "OFFLINE needs a small loader file in the RimWorld folder before it can start.";
pub const LOADER_FOREIGN: &str = "Another tool's loader file (winhttp.dll) is in the RimWorld folder, so OFFLINE cannot start. Remove that tool's file to fix it.";
pub const LOADER_INSTALLED: &str =
    "The loader is in the RimWorld folder. OFFLINE profiles can start.";
pub const OPEN_THAT_FOLDER: &str = "Open that folder";
pub const VIEW_MODS: &str = "View mod list";
pub const READING_MODS: &str = "Reading the mod list…";
pub const NO_MODS: &str =
    "This profile has no mods turned on yet. Start it once and pick mods in the game.";
pub const NO_MOD_MATCH: &str = "No mod matches the search.";
pub const MOD_SEARCH_HINT: &str = "Search mods  (Ctrl+F)";
pub const SOURCE_GAME: &str = "Game";
pub const SOURCE_FROZEN: &str = "Frozen copy";
pub const SOURCE_STEAM: &str = "Steam Workshop";
pub const SOURCE_INSTALLED: &str = "Mods folder";
pub const FLAG_MISSING: &str = "Not installed";
pub const FLAG_NOT_FROZEN: &str = "Not frozen";
pub const FLAG_TWICE: &str = "Installed twice";
pub const RIMSORT_CAPTION: &str =
    "Show this profile in RimSort, which sorts its mods and starts it through this tool.";
pub const OPEN_SAVES: &str = "Open saves folder";
pub const OTHERS_WAIT: &str = "Other changes wait until this is done.";
pub const GAME_RUNNING: &str = "RimWorld is running. Close it first.";
pub const BADGE_LIVE: &str = "LIVE";
pub const BADGE_OFFLINE: &str = "OFFLINE";

// Profile actions.
pub const LAUNCH: &str = "Play";
pub const CLONE: &str = "Copy profile…";
pub const RENAME: &str = "Rename…";
pub const DELETE: &str = "Delete…";
pub const GO_OFFLINE: &str = "Freeze mods (OFFLINE)";
pub const GO_LIVE: &str = "Use Steam mods (LIVE)";
pub const REFRESH: &str = "Update frozen mods";
pub const EXPORT: &str = "Add to RimSort";
pub const REMOVE_LOADER: &str = "Remove from game folder";
pub const OPEN_LOG: &str = "Open log";
pub const COPY_LOG: &str = "Copy log for support";
pub const COPY_STEAM: &str = "Copy line";

// Dialog buttons.
pub const OK: &str = "OK";
pub const CANCEL: &str = "Cancel";
pub const SAVE: &str = "Save";
pub const YES_DELETE: &str = "Delete it";
pub const ALLOW: &str = "Allow";
pub const MOVE: &str = "Move profiles";
pub const FREEZE: &str = "Freeze mods";
pub const ADD_TO_PROFILE: &str = "Add to profile";
pub const TURN_OFF: &str = "Turn off";
pub const PLAY_ANYWAY: &str = "Play anyway";

// Tooltips.
pub const TIP_PLAY: &str = "Start RimWorld with this profile's saves, settings and mods.";
pub const TIP_PLAY_NO_GAME: &str = "RimWorld was not found. Choose its folder in Settings first.";
pub const TIP_EXPORT: &str = "Show this profile in RimSort, so RimSort sorts its mods and starts the game through this tool.";
pub const TIP_GO_OFFLINE: &str = "Copy the mods this profile uses into the profile, so Steam Workshop updates cannot change them.";
pub const TIP_REFRESH: &str = "Copy the newest Steam versions of this profile's mods. Mods that left the Workshop keep their old copy.";
pub const TIP_GO_LIVE: &str =
    "Play with the Steam Workshop mods again. The frozen copies are kept, so you can switch back.";

// Help.
pub const HELP_TITLE: &str = "How profiles work";
pub const HELP_SECTIONS: [(&str, &str); 6] = [
    (
        "Profiles",
        "Each profile has its own saves, settings and mod list. Playing one never touches another.",
    ),
    (
        "LIVE",
        "The profile uses your Steam Workshop mods. When a mod updates on Steam, this profile gets the update too.",
    ),
    (
        "OFFLINE",
        "The profile keeps private copies of its mods. Workshop updates, removed mods and new subscriptions do not change it. Use \"Update frozen mods\" when you want the new versions.",
    ),
    (
        "Default",
        "The game as it is without this tool: its usual saves and settings. Steam Cloud keeps syncing it.",
    ),
    (
        "Steam and RimSort",
        "To start a profile from Steam, set the game's launch options to the line this tool shows. RimSort can start profiles too: use \"Add to RimSort\".",
    ),
    (
        "RimWorld 1.7",
        "Profiles work on 1.7 (Steam's unstable branch) as on 1.6. The first time a profile starts on 1.7, the game clears its mod list, because a 1.6 list does not carry over. Keep separate profiles for 1.6 and 1.7. Pressing Play in Steam without this tool's launch options starts Default, so Default's list is cleared too.",
    ),
];
pub const ICON_CREDIT: &str = "Icons: Font Awesome Free (CC BY 4.0). Text: Inter (SIL OFL 1.1).";

// Mode explanations.
pub const LIVE_ABOUT: &str =
    "LIVE: uses your Steam Workshop mods as they are now. Saves and settings stay in this profile.";
pub const OFFLINE_ABOUT: &str =
    "OFFLINE: plays with private copies of its mods, so Workshop updates cannot break this game.";
pub const DEFAULT_MODS: &str = "Uses your Steam Workshop mods, as the game does without this tool.";

// Consent.
pub const ASK_RIMSORT: &str = "To add profiles to RimSort, this tool needs to change RimSort's settings file. \
     It makes a backup first and only changes when RimSort is closed. Allow this?";
pub const ASK_LOADER: &str = "OFFLINE profiles on Windows need a small loader file (winhttp.dll) in the RimWorld folder. \
     It does nothing unless this tool starts the game. You can remove it any time. Allow this?";

// Name dialog.
pub const NAME_NEW: &str = "Name the new profile";
pub const NAME_COPY: &str = "Name the copy";
pub const NAME_RENAME: &str = "New name";
pub const NAME_HINT: &str = "Profile name";

// Freeze and guard dialogs.
pub const FREEZE_TITLE: &str = "Freeze this profile's mods";
pub const GUARD_TITLE: &str = "Some turned-on mods are not frozen in this profile";
pub const GUARD_BODY: &str = "They would load from Steam and can change:";

// Settings dialog.
pub const GAME_FOLDER: &str = "RimWorld folder (leave empty to find it through Steam)";
pub const STEAM_FOLDER: &str = "Steam folder (only if the wrong one is found)";
pub const FOUND_AUTO: &str = "Found automatically";
pub const MOVE_TO: &str = "Move all profiles to this folder";
pub const PROFILES_FOLDER: &str = "Profiles folder";
pub const FOUND_THROUGH_STEAM: &str = "Empty: RimWorld is found through Steam.";
pub const GAME_FOLDER_OK: &str = "RimWorld is in this folder.";
pub const MOVE_TO_HINT: &str = "Leave empty to keep them where they are";

// First-run setup.
pub const SETUP_STEP_1: &str = "STEP 1 OF 2";
pub const SETUP_STEP_2: &str = "STEP 2 OF 2";
pub const SETUP_GAME_TITLE: &str = "Find RimWorld";
pub const SETUP_GAME_BODY: &str =
    "This tool starts RimWorld for you, so it needs to know where the game is.";
pub const GAME_FOLDER_HINT: &str = "RimWorld folder";
pub const SETUP_NOT_FOUND_AUTO: &str =
    "RimWorld was not found automatically. Choose its folder below.";
pub const CHOOSE_FOLDER: &str = "Choose folder…";
pub const CONTINUE: &str = "Continue";
pub const SETUP_PROFILE_TITLE: &str = "Your first profile";
pub const SETUP_PROFILE_BODY: &str =
    "A profile has its own saves, settings and mod list. How should this one treat its mods?";
pub const CHOOSE_LIVE: &str = "Keep my mods up to date (LIVE)";
pub const CHOOSE_LIVE_ABOUT: &str =
    "Uses your Steam Workshop mods. When a mod updates, this profile gets the update.";
pub const CHOOSE_OFFLINE: &str = "Freeze my mods so updates can't break my colony (OFFLINE)";
pub const CHOOSE_OFFLINE_ABOUT: &str =
    "Keeps private copies of the mods. You choose when to update them.";
pub const FIRST_PROFILE_NAME: &str = "My colony";
pub const CREATE_PROFILE: &str = "Create profile";
pub const SKIP_SETUP: &str = "Skip, use Default";
pub const BROWSE: &str = "Browse…";
pub const PICK_GAME_TITLE: &str = "Choose the RimWorld folder";
pub const PICK_FOLDER_TITLE: &str = "Choose a folder";

// Job labels (shown with progress).
pub const JOB_CHECKING: &str = "Checking mods";
pub const JOB_FREEZING: &str = "Freezing mods";
pub const JOB_FREEZING_NEW: &str = "Freezing the new mods";
pub const JOB_RIMSORT: &str = "Adding to RimSort";
pub const JOB_LOADER: &str = "Installing the loader";
pub const JOB_MEASURING: &str = "Measuring profiles";
pub const JOB_MOVING: &str = "Moving profiles";

// Notices.
pub const DELETED: &str = "Moved to the recycle bin.";
pub const STARTING: &str = "RimWorld is starting…";
pub const LOADER_REMOVED: &str = "The loader was removed from the RimWorld folder.";
pub const NO_LOG: &str = "There is no log yet. Play this profile once first.";
pub const LOG_COPIED: &str = "Log copied. Paste it into your message when you ask for help.";
pub const STEAM_COPIED: &str =
    "Copied. In Steam: RimWorld → Properties → Launch options, then paste.";
pub const SETTINGS_SAVED: &str = "Settings saved.";
pub const NO_GAME: &str = "RimWorld was not found. Choose its folder in Settings.";
pub const NO_RIMSORT: &str = "RimSort's folder was not found.";
pub const NO_RIMSORT_CONSENT: &str = "Permission to change RimSort's settings is missing.";
pub const BAD_LAUNCH_FOLDER: &str = "This tool's folder cannot be used in a launch command.";
pub const NO_LAUNCHER: &str = "Steam and RimSort need this app's launcher, which could not be set up. Close this app and open it again to retry.";

/// "1 mod", "3 mods".
pub fn count(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}

/// "1.2 GB" style sizes; under 1 MB reads "less than 1 MB".
pub fn size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["MB", "GB", "TB", "PB"];
    let mut value = bytes as f64 / (1024.0 * 1024.0);
    if value < 1.0 {
        return "less than 1 MB".to_owned();
    }
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

fn free(bytes: Option<u64>) -> String {
    bytes.map_or_else(|| "unknown".to_owned(), size)
}

/// The first `max` characters of `name`, with "…" when cut.
pub fn short(name: &str, max: usize) -> String {
    if name.chars().count() <= max {
        name.to_owned()
    } else {
        let cut: String = name.chars().take(max.saturating_sub(1)).collect();
        format!("{}…", cut.trim_end())
    }
}

/// "OFFLINE · 142 mods · 2 h ago"; parts that are not known are left out.
pub fn facts(mode: &str, mods: Option<usize>, played_secs_ago: Option<u64>) -> String {
    let mut line = mode.to_owned();
    if let Some(n) = mods {
        line.push_str(&format!(" · {}", count(n, "mod", "mods")));
    }
    if let Some(secs) = played_secs_ago {
        line.push_str(&format!(" · {}", ago(secs)));
    }
    line
}

/// "just now", "5 min ago", "3 h ago", "yesterday", "4 days ago".
pub fn ago(secs: u64) -> String {
    match secs {
        0..60 => "just now".to_owned(),
        60..3600 => format!("{} min ago", secs / 60),
        3600..86_400 => format!("{} h ago", secs / 3600),
        86_400..172_800 => "yesterday".to_owned(),
        _ => format!("{} days ago", secs / 86_400),
    }
}

/// The Play row's line about the last attempt.
pub fn last_run(
    outcome: rwpm_core::profile::RunOutcome,
    by: rwpm_core::profile::StartedBy,
    secs_ago: Option<u64>,
    reason: &str,
) -> String {
    use rwpm_core::profile::{RunOutcome, StartedBy};
    let when = secs_ago.map_or_else(|| "Last run".to_owned(), |s| format!("Last run {}", ago(s)));
    let from = match by {
        StartedBy::Steam => " (started from Steam)",
        StartedBy::App => "",
        StartedBy::Rimsort => " (started from RimSort)",
        StartedBy::Other => " (started from elsewhere)",
    };
    match outcome {
        RunOutcome::Ok => format!("{when}: worked{from}."),
        RunOutcome::Refused => format!("{when}: not started{from}. {}", sentence(reason)),
        RunOutcome::Failed => format!("{when}: could not start{from}. {}", sentence(reason)),
    }
}

/// `reason` with a capital first letter and a full stop.
fn sentence(reason: &str) -> String {
    let mut chars = reason.trim().chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    let mut out: String = first.to_uppercase().chain(chars).collect();
    if !out.ends_with('.') {
        out.push('.');
    }
    out
}

pub fn found_game(folder: &str) -> String {
    format!("Found RimWorld in {folder}")
}

pub fn setup_not_found(reason: &str) -> String {
    format!("That folder does not have RimWorld in it ({reason}).")
}

pub fn guide_title(name: &str) -> String {
    format!("Start “{name}” from Steam")
}

pub fn steam_worked(secs_ago: Option<u64>) -> String {
    match secs_ago {
        Some(s) => format!("Started from Steam {}: it worked.", ago(s)),
        None => "Started from Steam: it worked.".to_owned(),
    }
}

pub fn steam_refused(secs_ago: Option<u64>, reason: &str) -> String {
    let when = secs_ago.map_or_else(String::new, |s| format!(" {}", ago(s)));
    format!(
        "Steam tried to start it{when}, but it did not start. {}",
        sentence(reason)
    )
}

pub fn mods_title(name: &str) -> String {
    format!("Mods in “{name}”")
}

pub fn mods_summary(total: usize, flagged: usize) -> String {
    let mods = count(total, "mod turned on", "mods turned on");
    match flagged {
        0 => format!("{mods}, in load order."),
        n => format!(
            "{mods}, in load order. {} to look at.",
            count(n, "mod", "mods")
        ),
    }
}

pub fn mods_unreadable(reason: &str) -> String {
    format!("The mod list could not be read: {reason}")
}

/// "Harmony, HugsLib and 3 more"
fn names_list(names: &[String]) -> String {
    const SHOWN: usize = 3;
    let head = names
        .iter()
        .take(SHOWN)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    match names.len().saturating_sub(SHOWN) {
        0 => head,
        rest => format!("{head} and {rest} more"),
    }
}

pub fn not_frozen(names: &[String]) -> String {
    format!(
        "{} would load from Steam and can change: {}.",
        count(names.len(), "mod", "mods"),
        names_list(names)
    )
}

pub fn not_installed_mods(ids: &[String]) -> String {
    format!(
        "{} turned on but not installed, so the game turns {} off: {}.",
        count(ids.len(), "mod is", "mods are"),
        if ids.len() == 1 { "it" } else { "them" },
        names_list(ids)
    )
}

pub fn rimsort_stale(name: &str) -> String {
    format!("RimSort's “{name}” still points at this profile's old folder.")
}

pub fn frozen_now(n: usize) -> String {
    format!(
        "Froze {}. This profile no longer loads them from Steam.",
        count(n, "mod", "mods")
    )
}

pub fn profiles_now(root: &str) -> String {
    format!("Profiles are now in {root}")
}

pub fn copy_group(group: rwpm_core::profile::copy_settings::Group, bytes: u64) -> String {
    use rwpm_core::profile::copy_settings::Group;
    let what = match group {
        Group::ModList => "Mod list",
        Group::GameSettings => "Game settings and key bindings",
        Group::ModSettings => "Mod settings",
        Group::Creations => "Your scenarios, ideoligions and xenotypes",
        Group::ModData => "Mod data (caches, logs, replays)",
    };
    format!("{what} ({})", size(bytes))
}

pub fn copy_title(name: &str) -> String {
    format!("Copy settings into “{name}”")
}

pub fn settings_copied(name: &str) -> String {
    format!("Settings copied into “{name}”.")
}

pub fn created_with_settings(name: &str) -> String {
    format!("Created “{name}” with the chosen settings.")
}

pub fn busy(job: &str) -> String {
    format!("Busy: {}. Other changes wait.", job.to_lowercase())
}

pub fn open_folder_failed(reason: &str) -> String {
    format!("Could not open the folder: {reason}")
}

pub fn no_game(reason: &str) -> String {
    format!("RimWorld was not found ({reason}).")
}

pub fn unreadable(id: &str, reason: &str) -> String {
    format!("Profile folder “{id}” cannot be read: {reason}")
}

pub fn progress(label: &str, done: u64, total: u64) -> String {
    if total == 0 {
        format!("{label}…")
    } else {
        format!("{label}… {} of {}", size(done), size(total))
    }
}

pub fn confirm_delete(name: &str) -> String {
    format!("Delete “{name}”? It goes to the recycle bin, so you can still get it back.")
}

pub fn freeze_summary(mods: usize, bytes: u64, free_bytes: Option<u64>) -> String {
    format!(
        "{} will be copied: up to {} (free space: {}).",
        count(mods, "mod", "mods"),
        size(bytes),
        free(free_bytes)
    )
}

pub fn not_installed(ids: &[String]) -> String {
    format!("Not installed, so not copied: {}", ids.join(", "))
}

pub fn installed_twice(id: &str) -> String {
    format!("“{id}” is installed twice. Which copy should this profile keep?")
}

pub fn unfrozen_copy(name: &str) -> String {
    format!("• {name}")
}

pub fn unfrozen_steam_copy(name: &str) -> String {
    format!("• {name} (the Steam copy of a mod this profile has frozen)")
}

pub fn confirm_move(profiles: usize, bytes: u64, to: &str, free_bytes: Option<u64>) -> String {
    format!(
        "Move {} ({}) to {to}? Free space there: {}. The old copies go to the recycle bin only after the new ones are checked.",
        count(profiles, "profile", "profiles"),
        size(bytes),
        free(free_bytes)
    )
}

pub fn copy_name(name: &str) -> String {
    format!("{name} copy")
}

pub fn created(name: &str) -> String {
    format!("Created “{name}”.")
}

pub fn renamed(name: &str) -> String {
    format!("Renamed to “{name}”.")
}

pub fn copying(name: &str) -> String {
    format!("Copying “{name}”")
}

pub fn copied(name: &str) -> String {
    format!("Copied to “{name}”.")
}

pub fn now_offline(name: &str) -> String {
    format!("“{name}” is OFFLINE: its mods are frozen.")
}

pub fn now_live(name: &str) -> String {
    format!("“{name}” is LIVE. Its frozen copies are kept for later.")
}

pub fn start_failed(reason: &str) -> String {
    format!("Could not start RimWorld: {reason}")
}

pub fn added_to_rimsort(name: &str) -> String {
    format!("Added to RimSort as “{name}”.")
}

pub fn opened(path: &Path) -> String {
    format!("Opened {}", path.display())
}

pub fn open_failed(reason: &str) -> String {
    format!("Could not open the log: {reason}")
}

pub fn moved(to: &Path, readd: &[String]) -> String {
    let mut line = format!("Profiles moved to {}.", to.display());
    if !readd.is_empty() {
        line.push_str(&format!(
            " Add these to RimSort again: {}.",
            readd.join(", ")
        ));
    }
    line
}

pub fn appimage_launcher_failed(reason: &str) -> String {
    format!(
        "This app could not copy its launcher out of the AppImage ({reason}). \
         Launching from Steam will not work."
    )
}

pub fn launcher_failed(reason: &str) -> String {
    format!(
        "RimWorld is in Flatpak Steam, but this app could not put its launcher \
         where Steam can see it ({reason}). Launching from Steam will not work."
    )
}

/// V46/R30: an un-notarised `.app` run from a quarantined location (eg straight out of
/// Downloads) can execute from a Gatekeeper-translocated path that changes on every
/// launch, so Steam/RimSort references to it would break.
pub fn macos_launcher_failed(reason: &str) -> String {
    format!(
        "This app could not copy its launcher to a stable folder ({reason}). \
         Launching from Steam or RimSort may not work until it is moved to Applications."
    )
}

pub fn other_profiles(other: &Path, used: &Path) -> String {
    format!(
        "Profiles also exist in {}. This app uses {}, the folder that goes with \
         your RimWorld install.",
        other.display(),
        used.display()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_in_plain_units() {
        assert_eq!(size(512), "less than 1 MB");
        assert_eq!(size(1536 * 1024), "1.5 MB");
        assert_eq!(size(3 * 1024 * 1024 * 1024), "3.0 GB");
    }

    #[test]
    fn long_names_are_cut_with_an_ellipsis() {
        assert_eq!(short("Short", 24), "Short");
        let long = "A very long profile name that goes on";
        let cut = short(long, 24);
        assert_eq!(cut.chars().count(), 24);
        assert!(cut.ends_with('…'));
        assert_eq!(short("ÄÖÜÄÖÜ", 4), "ÄÖÜ…", "counts characters, not bytes");
    }

    #[test]
    fn last_run_lines_say_how_it_went_and_who_started_it() {
        use rwpm_core::profile::{RunOutcome, StartedBy};
        assert_eq!(
            last_run(RunOutcome::Ok, StartedBy::Steam, Some(7200), ""),
            "Last run 2 h ago: worked (started from Steam)."
        );
        assert_eq!(
            last_run(
                RunOutcome::Refused,
                StartedBy::App,
                Some(30),
                "RimWorld is already running; close it first"
            ),
            "Last run just now: not started. RimWorld is already running; close it first."
        );
        assert_eq!(ago(86_400 * 3), "3 days ago");
        assert_eq!(
            facts("LIVE", Some(1), Some(120)),
            "LIVE · 1 mod · 2 min ago"
        );
    }

    #[test]
    fn counts_use_singular_for_one() {
        assert_eq!(count(1, "mod", "mods"), "1 mod");
        assert_eq!(count(0, "mod", "mods"), "0 mods");
        assert_eq!(
            freeze_summary(1, 65, Some(0)),
            "1 mod will be copied: up to less than 1 MB (free space: less than 1 MB)."
        );
    }

    /// Inline words in `src`: string literals with letters (outside `{…}` placeholders)
    /// passed to the calls that show text. → (line number, literal).
    fn inline_words(src: &str) -> Vec<(usize, String)> {
        let calls = [
            "text(\"",
            "button(\"",
            "Notice::Info(\"",
            "Notice::Error(\"",
            "format!(\"",
            "fail(\"",
            "push(\"",
        ];
        let mut found = Vec::new();
        for (n, line) in src.lines().enumerate() {
            for call in calls {
                let Some(at) = line.find(call) else { continue };
                let literal = line[at + call.len()..]
                    .split('"')
                    .next()
                    .unwrap_or_default();
                let mut depth = 0;
                let words: String = literal
                    .chars()
                    .filter(|c| {
                        match c {
                            '{' => depth += 1,
                            '}' => depth -= 1,
                            _ => return depth == 0,
                        }
                        false
                    })
                    .collect();
                if words.chars().any(char::is_alphabetic) {
                    found.push((n + 1, literal.to_owned()));
                }
            }
        }
        found
    }

    #[test]
    fn inline_words_found_outside_placeholders_only() {
        let src =
            "text(\"Hello\")\nformat!(\"{name} copy\")\nformat!(\"{} {}\", a, b)\ntext(t::OK)";
        assert_eq!(
            inline_words(src),
            [(1, "Hello".to_owned()), (2, "{name} copy".to_owned())]
        );
    }

    /// New player-facing text goes in this file: the GUI code passes no literal words
    /// to the widgets or notices that show text (DA: rwpm-core errors are the exception).
    #[test]
    fn gui_code_shows_no_inline_words() {
        for (file, src) in [
            ("view.rs", include_str!("view.rs")),
            ("update.rs", include_str!("update.rs")),
            ("mod.rs", include_str!("mod.rs")),
            ("dialogs.rs", include_str!("dialogs.rs")),
            ("setup.rs", include_str!("setup.rs")),
            ("keys.rs", include_str!("keys.rs")),
            ("guide.rs", include_str!("guide.rs")),
            ("mods.rs", include_str!("mods.rs")),
            ("health.rs", include_str!("health.rs")),
            ("copy.rs", include_str!("copy.rs")),
        ] {
            assert_eq!(inline_words(src), [], "{file}");
        }
    }
}
