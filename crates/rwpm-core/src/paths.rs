//! Default on-disk locations (§I).

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::flatpak;

/// App data folder name (§I `<app>`).
pub const APP_DIR: &str = "RimWorld Profile Manager";

/// Where this app and the game keep their files (V31).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// The user's own data and config folders (inside a sandbox: the sandbox's).
    Standard,
    /// Below `~/.var/app/com.valvesoftware.Steam`, where Flatpak Steam's game and
    /// launches see the same folders as this app on the host.
    FlatpakSteam,
}

/// Which Steam installs have RimWorld.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GameIn {
    pub native: bool,
    pub flatpak: bool,
}

/// V31: Flatpak when RimWorld is only in Flatpak Steam, or in both and this app
/// already keeps its data there. Inside a sandbox the standard folders already are
/// Flatpak Steam's.
pub fn decide_layout(in_sandbox: bool, game: GameIn, flatpak_data_exists: bool) -> Layout {
    if in_sandbox || cfg!(windows) {
        return Layout::Standard;
    }
    match (game.native, game.flatpak) {
        (false, true) => Layout::FlatpakSteam,
        (true, true) if flatpak_data_exists => Layout::FlatpakSteam,
        _ => Layout::Standard,
    }
}

/// This machine's layout, decided once per process.
pub fn layout() -> Layout {
    static LAYOUT: OnceLock<Layout> = OnceLock::new();
    *LAYOUT.get_or_init(|| {
        if flatpak::in_sandbox() || cfg!(windows) {
            return Layout::Standard;
        }
        let flatpak_data = dirs::home_dir().map(|h| flatpak_app_dir(&h));
        decide_layout(
            false,
            crate::discovery::steams_with_game(),
            flatpak_data.is_some_and(|d| d.is_dir()),
        )
    })
}

fn flatpak_app_dir(home: &Path) -> PathBuf {
    flatpak::steam_home(home).join("data").join(APP_DIR)
}

fn flatpak_game_save_dir(home: &Path) -> PathBuf {
    ludeon(&flatpak::steam_home(home).join("config").join("unity3d"))
}

fn ludeon(base: &Path) -> PathBuf {
    base.join("Ludeon Studios")
        .join("RimWorld by Ludeon Studios")
}

/// `<data-local>/<app>`: Linux `~/.local/share/…`, Windows `%LOCALAPPDATA%\…`;
/// Flatpak Steam: `~/.var/app/com.valvesoftware.Steam/data/…`.
pub fn app_data_dir() -> Option<PathBuf> {
    app_data_dir_for(layout())
}

fn app_data_dir_for(layout: Layout) -> Option<PathBuf> {
    match layout {
        Layout::Standard => dirs::data_local_dir().map(|d| d.join(APP_DIR)),
        Layout::FlatpakSteam => dirs::home_dir().map(|h| flatpak_app_dir(&h)),
    }
}

/// `<app data>/profiles`.
pub fn default_profiles_root() -> Option<PathBuf> {
    app_data_dir().map(|d| d.join("profiles"))
}

/// `<app data>/run.lock`: held by `rwpm run` while its game runs (V11).
pub fn run_lock_path() -> Option<PathBuf> {
    app_data_dir().map(|d| d.join("run.lock"))
}

/// The game's own save-data dir, used by the Default profile (§I).
/// Unity `persistentDataPath`: Windows `AppData\LocalLow\…`, Linux `$XDG_CONFIG_HOME/unity3d/…`;
/// Flatpak Steam: its sandbox's `$XDG_CONFIG_HOME`.
pub fn default_game_save_dir() -> Option<PathBuf> {
    if layout() == Layout::FlatpakSteam {
        return dirs::home_dir().map(|h| flatpak_game_save_dir(&h));
    }
    #[cfg(windows)]
    let base = dirs::home_dir().map(|h| h.join("AppData").join("LocalLow"));
    #[cfg(not(windows))]
    let base = dirs::config_dir().map(|c| c.join("unity3d"));
    base.map(|b| ludeon(&b))
}

/// The other layout's profiles folder, when it holds profiles this app now ignores.
pub fn ignored_profiles() -> Option<PathBuf> {
    if flatpak::in_sandbox() || cfg!(windows) {
        return None;
    }
    let other = match layout() {
        Layout::Standard => Layout::FlatpakSteam,
        Layout::FlatpakSteam => Layout::Standard,
    };
    let dir = app_data_dir_for(other)?.join("profiles");
    let has_profiles = std::fs::read_dir(&dir)
        .is_ok_and(|mut entries| entries.any(|e| e.is_ok_and(|e| e.path().is_dir())));
    has_profiles.then_some(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles_root_ends_with_app_dir() {
        let root = default_profiles_root().expect("data-local dir on test host");
        assert!(root.ends_with(PathBuf::from(APP_DIR).join("profiles")));
    }

    #[test]
    fn game_save_dir_ends_with_ludeon_path() {
        let dir = default_game_save_dir().expect("home dir on test host");
        assert!(dir.ends_with(PathBuf::from("Ludeon Studios").join("RimWorld by Ludeon Studios")));
    }

    #[cfg(not(windows))]
    #[test]
    fn layout_follows_where_the_game_is() {
        let only = |native, flatpak| GameIn { native, flatpak };
        let cases = [
            // (sandbox, native, flatpak, flatpak data exists) → layout
            (false, false, true, false, Layout::FlatpakSteam),
            (false, false, true, true, Layout::FlatpakSteam),
            (false, true, false, true, Layout::Standard),
            (false, true, true, false, Layout::Standard),
            (false, true, true, true, Layout::FlatpakSteam),
            (false, false, false, true, Layout::Standard),
            (true, false, true, true, Layout::Standard),
        ];
        for (sandbox, native, flatpak, exists, want) in cases {
            assert_eq!(
                decide_layout(sandbox, only(native, flatpak), exists),
                want,
                "{sandbox} {native} {flatpak} {exists}"
            );
        }
    }

    /// The same folders Flatpak Steam's sandbox gets as XDG data and config (R23).
    #[test]
    fn flatpak_folders_match_the_sandbox() {
        let home = Path::new("/home/u");
        assert_eq!(
            flatpak_app_dir(home),
            Path::new("/home/u/.var/app/com.valvesoftware.Steam/data/RimWorld Profile Manager")
        );
        assert_eq!(
            flatpak_game_save_dir(home),
            Path::new(
                "/home/u/.var/app/com.valvesoftware.Steam/config/unity3d/Ludeon Studios/RimWorld by Ludeon Studios"
            )
        );
    }
}
