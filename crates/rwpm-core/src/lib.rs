//! Core logic for the RimWorld profile manager.
//!
//! All behaviour lives here; the `rwpm` binary (GUI and `rwpm run`) is a thin front end.

pub mod appimage;
pub mod backup;
pub mod builds;
pub mod consent;
pub mod discovery;
pub mod error;
pub mod flatpak;
mod fs_copy;
pub mod launch;
pub mod mods;
pub mod paths;
pub mod profile;
pub mod rimsort;
pub mod settings;
pub mod space;
pub mod winhttp;

pub use error::ProfileError;

/// Crate version, shared by every front end.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::VERSION;

    #[test]
    fn version_is_semver_triple() {
        let parts: Vec<&str> = VERSION.split('.').collect();
        assert_eq!(parts.len(), 3, "version {VERSION} is not x.y.z");
        assert!(parts.iter().all(|p| p.parse::<u32>().is_ok()));
    }
}
