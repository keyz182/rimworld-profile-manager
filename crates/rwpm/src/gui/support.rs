//! "Open log" and "Copy log for support".

use std::path::Path;

use rwpm_core::discovery::{Engine, GameInstall};
use rwpm_core::profile::Profile;

/// Only the end of a long game log is useful; keep the paste a sane size.
const MAX_LOG_BYTES: usize = 256 * 1024;

/// Everything a helper needs, as one paste: versions, the profile, our own logs and
/// the end of the game's log.
pub fn support_text(profile: &Profile, game: Option<&GameInstall>) -> String {
    let mut out = format!(
        "RimWorld Profile Manager {}\nSystem: {} {}\nProfile: {} ({}, {:?})\nRimWorld: {}\n",
        rwpm_core::VERSION,
        std::env::consts::OS,
        std::env::consts::ARCH,
        profile.name,
        profile.id,
        profile.mode,
        game.and_then(|g| g.version.clone())
            .unwrap_or_else(|| "not found".into()),
    );
    if let Some(dir) = profile.stored_dir() {
        for name in ["rwpm.log", "payload.log"] {
            section(&mut out, name, &dir.join(name));
        }
    }
    section(&mut out, "Player.log", &profile.log_path(engine(game)));
    out
}

/// The engine whose log path applies (V67): the found game's, else Unity's.
pub fn engine(game: Option<&GameInstall>) -> Engine {
    game.map_or(Engine::Unity, GameInstall::engine)
}

fn section(out: &mut String, name: &str, path: &Path) {
    out.push_str(&format!("\n===== {name} =====\n"));
    match std::fs::read(path) {
        Ok(bytes) => {
            let start = bytes.len().saturating_sub(MAX_LOG_BYTES);
            if start > 0 {
                out.push_str(&format!("[first {start} bytes left out]\n"));
            }
            out.push_str(&String::from_utf8_lossy(&bytes[start..]));
        }
        Err(_) => out.push_str("(none)\n"),
    }
}

/// Opens a file or folder with the system's default app.
pub fn open_path(path: &Path) -> std::io::Result<()> {
    let opener = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let mut child = std::process::Command::new(opener).arg(path).spawn()?;
    std::thread::spawn(move || child.wait());
    Ok(())
}
