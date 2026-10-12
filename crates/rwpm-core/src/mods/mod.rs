//! Installed mods: the active list in `ModsConfig.xml` and where each packageId lives (R1).
//! Read-only: scans the game's `Data` and `Mods` folders and the workshop content dir.

mod config;
mod index;

use std::path::PathBuf;

pub use config::{read_active_mods, rewrite_active_mods};
pub use index::{InstalledMod, ModIndex, Origin, Resolution, Skipped};

/// Workshop copies of a packageId that also exists locally get this suffix (R1).
pub const STEAM_POSTFIX: &str = "_steam";

#[derive(Debug, thiserror::Error)]
pub enum ModsError {
    #[error("cannot read `{}`: {source}", path.display())]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("cannot read `{}`: {message}", path.display())]
    Xml { path: PathBuf, message: String },
}

/// Reads an XML file as UTF-8 text without a byte-order mark.
fn read_xml_text(path: &std::path::Path) -> Result<String, ModsError> {
    let bytes = std::fs::read(path).map_err(|source| ModsError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes);
    String::from_utf8(bytes.to_vec()).map_err(|_| ModsError::Xml {
        path: path.to_path_buf(),
        message: "not UTF-8 text".into(),
    })
}
