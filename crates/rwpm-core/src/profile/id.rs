//! Stable profile ids (V19): slug of the first name, `[a-z0-9-]`, never `=` or whitespace.

use std::fmt;

use crate::error::ProfileError;

/// Reserved id of the built-in Default profile.
pub const DEFAULT_ID: &str = "default";
/// Reserved name of the game-build store under the profiles root (SPEC §I.cmd, §C).
/// `parse` still accepts it: a profile created before v1.4 may already sit there, and it
/// has to stay listable so the player can rename it. Nothing new may claim it, and the
/// build store refuses to open over it.
pub const BUILDS_DIR: &str = "builds";
/// Longest slug base; a `-N` suffix may add up to 4 more chars.
pub const MAX_BASE_LEN: usize = 40;
const MAX_ID_LEN: usize = MAX_BASE_LEN + 4;
const FALLBACK: &str = "profile";

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ProfileId(String);

impl ProfileId {
    /// Validates an id typed by a user or read from a launch option.
    pub fn parse(s: &str) -> Result<Self, ProfileError> {
        let charset_ok = s
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        let shape_ok =
            !s.is_empty() && s.len() <= MAX_ID_LEN && !s.starts_with('-') && !s.ends_with('-');
        if charset_ok && shape_ok {
            Ok(Self(s.to_owned()))
        } else {
            Err(ProfileError::InvalidId(s.to_owned()))
        }
    }

    pub fn default_id() -> Self {
        Self(DEFAULT_ID.to_owned())
    }

    pub fn is_default(&self) -> bool {
        self.0 == DEFAULT_ID
    }

    /// Names the build store, so no new profile may take it (V56, §I.cmd).
    pub fn is_builds(&self) -> bool {
        self.0 == BUILDS_DIR
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Slug base for a new profile name. May equal `default`; the store skips that. Also the
    /// suggested file name of a backup.
    pub fn slug_base(name: &str) -> String {
        let mut slug = String::with_capacity(name.len());
        for c in name.chars() {
            if c.is_ascii_alphanumeric() {
                slug.push(c.to_ascii_lowercase());
            } else if !slug.ends_with('-') {
                slug.push('-');
            }
        }
        let mut slug = slug.trim_matches('-').to_owned();
        slug.truncate(MAX_BASE_LEN);
        let slug = slug.trim_end_matches('-');
        if slug.is_empty() {
            FALLBACK.to_owned()
        } else {
            slug.to_owned()
        }
    }

    /// `base` for n = 1, else `base-n`. `base` must come from `slug_base`.
    pub(crate) fn candidate(base: &str, n: u32) -> Self {
        if n <= 1 {
            Self(base.to_owned())
        } else {
            Self(format!("{base}-{n}"))
        }
    }
}

impl fmt::Display for ProfileId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_lowercases_and_hyphenates() {
        assert_eq!(ProfileId::slug_base("My Modded Run!"), "my-modded-run");
        assert_eq!(
            ProfileId::slug_base("  CE + Vanilla  Expanded "),
            "ce-vanilla-expanded"
        );
    }

    #[test]
    fn slug_never_contains_whitespace_or_equals() {
        for name in [
            "a=b c",
            "tab\there",
            "x==y",
            "Ünïcode Ŝave",
            "line\nbreak",
            "--edge--",
        ] {
            let slug = ProfileId::slug_base(name);
            assert!(!slug.is_empty(), "{name:?}");
            assert!(
                slug.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
                "{name:?} -> {slug}"
            );
            assert!(
                !slug.starts_with('-') && !slug.ends_with('-'),
                "{name:?} -> {slug}"
            );
            assert!(ProfileId::parse(&slug).is_ok(), "{name:?} -> {slug}");
        }
    }

    #[test]
    fn slug_empty_falls_back() {
        assert_eq!(ProfileId::slug_base("   "), "profile");
        assert_eq!(ProfileId::slug_base("日本語"), "profile");
    }

    #[test]
    fn slug_truncates_to_max_base_len() {
        let slug = ProfileId::slug_base(&"a b ".repeat(40));
        assert!(slug.len() <= MAX_BASE_LEN);
        assert!(!slug.ends_with('-'));
    }

    #[test]
    fn builds_is_reserved_but_still_parses_for_legacy_dirs() {
        let id = ProfileId::parse(BUILDS_DIR).expect("legacy read path");
        assert!(id.is_builds());
        assert!(!id.is_default());
        assert!(!ProfileId::parse("builds-2").expect("ok").is_builds());
    }

    #[test]
    fn candidate_suffixes_from_two() {
        assert_eq!(ProfileId::candidate("run", 1).as_str(), "run");
        assert_eq!(ProfileId::candidate("run", 2).as_str(), "run-2");
    }

    #[test]
    fn parse_rejects_bad_ids() {
        for bad in [
            "",
            "Upper",
            "has space",
            "a=b",
            "-lead",
            "trail-",
            &"x".repeat(45),
        ] {
            assert!(ProfileId::parse(bad).is_err(), "{bad:?} accepted");
        }
        assert!(ProfileId::parse("my-run-2").is_ok());
    }
}
