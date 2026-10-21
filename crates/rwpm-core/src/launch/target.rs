//! The target swap for a pinned profile. `rwpm run` is handed Steam's
//! `%command%`, which names the Steam install's game. For a pinned profile that one entry
//! is replaced with the stored build's own, before anything else reads the command: the
//! guard, the engine check, the Windows loader, the macOS bundle and the restart env then
//! all see the build.
//!
//! Wrappers before the game entry (Steam Linux Runtime, gamemoderun) and arguments after
//! it are kept byte for byte.

use std::ffi::OsString;
use std::path::Path;

use super::{LINUX_ELF, LINUX_START_SCRIPT, LaunchError};
use crate::builds::{EffectiveGame, GameSource};
use crate::discovery::{Engine, GameInstall};

/// Every file name the game's own entry has, on any OS and engine: Linux 1.6 (script, ELF)
/// and 1.7, Windows 1.6 and 1.7, macOS 1.6 and 1.7 bundles.
const GAME_ENTRIES: [&str; 7] = [
    LINUX_START_SCRIPT,
    LINUX_ELF,
    "RimWorld",
    "RimWorldWin64.exe",
    "RimWorld.exe",
    "RimWorldMac.app",
    "RimWorld.app",
];

/// The index of the game's own entry in `command`, found by file name only. The Steam
/// install it names may be another version, a husk or gone, so the disk is not read.
///
/// Flags (`-x`) and `key=value` tokens are never the entry, though they can end in a game
/// name (`-savedatafolder=/x/RimWorld`). More than one match is refused: swapping the wrong
/// one would start Steam's game without a word.
pub fn game_target(command: &[OsString]) -> Result<usize, LaunchError> {
    let mut found = command
        .iter()
        .enumerate()
        .filter(|(_, arg)| is_game_entry(arg))
        .map(|(at, _)| at);
    let at = found.next().ok_or(LaunchError::NoGameInCommand)?;
    if found.next().is_some() {
        return Err(LaunchError::AmbiguousGameInCommand);
    }
    Ok(at)
}

/// `command` with the game entry replaced by `game`'s own. For an unpinned profile the
/// effective game is the Steam install, and `command` comes back unchanged.
pub fn swap_target(
    command: &[OsString],
    game: &EffectiveGame,
) -> Result<Vec<OsString>, LaunchError> {
    match game.source() {
        GameSource::Steam => Ok(command.to_vec()),
        GameSource::Build(_) => swap_for(command, game.install()),
    }
}

/// The swap for one install. A Linux 1.6 build launches the way Steam launches 1.6: its
/// start script, unless the incoming entry was the ELF itself (app Play), which stays an
/// ELF. That keeps the script's cwd and `LC_ALL` on LIVE and the wrapped path on
/// OFFLINE. Every other case takes the build's exe.
pub(crate) fn swap_for(
    command: &[OsString],
    install: &GameInstall,
) -> Result<Vec<OsString>, LaunchError> {
    let at = game_target(command)?;
    let script = install.game_dir.join(LINUX_START_SCRIPT);
    let script = (install.engine == Engine::Unity && script.is_file()).then_some(script);
    Ok(swap_to(command, at, &install.exe, script.as_deref()))
}

/// Pure: replaces `command[at]`. `script` is the build's start script, when it has one.
fn swap_to(command: &[OsString], at: usize, exe: &Path, script: Option<&Path>) -> Vec<OsString> {
    let incoming_elf = file_name(&command[at]).eq_ignore_ascii_case(LINUX_ELF);
    let entry = match script {
        Some(script) if !incoming_elf => script,
        _ => exe,
    };
    let mut out = command.to_vec();
    out[at] = entry.as_os_str().to_os_string();
    out
}

fn is_game_entry(arg: &OsString) -> bool {
    let text = arg.to_string_lossy();
    if text.starts_with('-') || text.contains('=') {
        return false;
    }
    let name = file_name(arg);
    GAME_ENTRIES.iter().any(|g| g.eq_ignore_ascii_case(&name))
}

/// The last path segment, split on `/` and `\` whatever the host, so a Windows command is
/// read the same on Linux. A trailing separator (`RimWorldMac.app/`) is ignored.
fn file_name(arg: &OsString) -> String {
    let text = arg.to_string_lossy();
    text.trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .to_owned()
}

#[cfg(test)]
mod tests;
