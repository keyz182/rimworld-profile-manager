//! The steamcmd route (T82): getting a modpack's missing mods with Valve's own command-line
//! downloader, for a machine that has no Steam client to ask.
//!
//! steamcmd is not bundled. It is downloaded from Valve once, into this app's own folder
//! (`<app data>/steamcmd/`), after the player agrees. The archive is checked against the
//! SHA-256 seen the first time (a later change is refused, not trusted), and unpacked by code
//! that refuses every entry that could land outside that folder.

use std::path::PathBuf;

pub mod download;
pub mod output;
pub mod run;
pub mod unpack;

#[cfg(test)]
mod tests;

/// How Valve packs the bootstrap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveKind {
    Zip,
    TarGz,
}

/// Valve's bootstrap for this operating system.
#[derive(Debug, Clone, Copy)]
pub struct Source {
    pub url: &'static str,
    pub kind: ArchiveKind,
}

/// The three files measured in R52, 2026-10-19. The URL is fixed in the program: nothing a
/// pack or a setting says can change where steamcmd comes from.
pub const SOURCE: Source = if cfg!(windows) {
    Source {
        url: "https://steamcdn-a.akamaihd.net/client/installer/steamcmd.zip",
        kind: ArchiveKind::Zip,
    }
} else if cfg!(target_os = "macos") {
    Source {
        url: "https://steamcdn-a.akamaihd.net/client/installer/steamcmd_osx.tar.gz",
        kind: ArchiveKind::TarGz,
    }
} else {
    Source {
        url: "https://steamcdn-a.akamaihd.net/client/installer/steamcmd_linux.tar.gz",
        kind: ArchiveKind::TarGz,
    }
};

/// The biggest archive that is read. Valve's are 0.8-2.5 MB.
pub const MAX_ARCHIVE_BYTES: u64 = 16 * 1024 * 1024;
/// The most that may be written when unpacking. Valve's hold 6.5 MB.
pub const MAX_UNPACKED_BYTES: u64 = 128 * 1024 * 1024;
/// The most entries an archive may hold. Valve's hold 1 to 69.
pub const MAX_ENTRIES: usize = 1000;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Could not download steamcmd from Valve: {0}")]
    Download(String),
    #[error("steamcmd is not downloaded until you agree to it.")]
    NoConsent,
    #[error(
        "Valve's steamcmd file is bigger than {} MB, so it was thrown away.",
        MAX_ARCHIVE_BYTES / 1024 / 1024
    )]
    TooBig,
    #[error(
        "Valve's steamcmd file is not the one downloaded the first time (its checksum is \
         {found}, first seen {first}). It was not unpacked. Valve may have updated it, or \
         something changed it on the way."
    )]
    Changed { first: String, found: String },
    #[error(
        "The steamcmd file has an entry that is not safe to unpack (`{0}`), so nothing was unpacked."
    )]
    Unsafe(String),
    #[error("The steamcmd file is damaged: {0}")]
    Archive(String),
    #[error("cannot {op} `{}`: {source}", path.display())]
    Io {
        op: &'static str,
        path: PathBuf,
        source: std::io::Error,
    },
}

impl Error {
    fn io(op: &'static str, path: &std::path::Path) -> impl FnOnce(std::io::Error) -> Self {
        let path = path.to_path_buf();
        move |source| Self::Io { op, path, source }
    }
}

/// What the checksum of a downloaded archive says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Seen {
    /// Nothing was recorded yet: record this one.
    First(String),
    /// The same as the one recorded.
    Same,
}

/// Compares the archive's SHA-256 with the one recorded the first time, if any.
pub fn check_hash(first: Option<&str>, bytes: &[u8]) -> Result<Seen, Error> {
    use sha2::{Digest, Sha256};
    let found: String = Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    match first {
        None => Ok(Seen::First(found)),
        Some(first) if first == found => Ok(Seen::Same),
        Some(first) => Err(Error::Changed {
            first: first.to_owned(),
            found,
        }),
    }
}
