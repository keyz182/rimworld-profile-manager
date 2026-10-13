//! V61: changing a profile's game version warns before a downgrade that may break saves.

use std::path::Path;

/// `major.minor.build` of a `Version.txt` text (`1.6.4871 rev598` → `(1, 6, 4871)`).
/// The `rev` part is ignored. `None` when the text is not in that shape.
fn numeric(version: &str) -> Option<(u32, u32, u32)> {
    let mut parts = version.split_whitespace().next()?.split('.');
    let mut next = || parts.next()?.parse::<u32>().ok();
    let found = (next()?, next()?, next()?);
    parts.next().is_none().then_some(found)
}

/// Does moving a profile from `old` to `new` (both `Version.txt` texts; `None` = not
/// known) need the player's OK first? Yes iff the profile has saves and the new version is
/// older, or either version cannot be read (V61, §C pin downgrade).
pub fn pin_warns(old: Option<&str>, new: Option<&str>, saves: &Path) -> bool {
    let has_saves = std::fs::read_dir(saves).is_ok_and(|mut d| d.next().is_some());
    if !has_saves {
        return false;
    }
    match (old.and_then(numeric), new.and_then(numeric)) {
        (Some(old), Some(new)) => new < old,
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_saves() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::write(tmp.path().join("Colony.rws"), "x").expect("save");
        tmp
    }

    #[test]
    fn reads_major_minor_build_and_ignores_rev() {
        assert_eq!(numeric("1.6.4871 rev598"), Some((1, 6, 4871)));
        assert_eq!(numeric(" 1.7.4976 rev1043\r\n"), Some((1, 7, 4976)));
        assert_eq!(numeric("1.6"), None);
        assert_eq!(numeric("1.6.4871.2 rev1"), None);
        assert_eq!(numeric("beta"), None);
        assert_eq!(numeric(""), None);
    }

    #[test]
    fn v61_warns_only_on_a_downgrade_with_saves() {
        let saves = with_saves();
        let s = saves.path();
        assert!(pin_warns(
            Some("1.7.4976 rev1043"),
            Some("1.6.4871 rev598"),
            s
        ));
        assert!(!pin_warns(
            Some("1.6.4871 rev598"),
            Some("1.7.4976 rev1043"),
            s
        ));
        assert!(!pin_warns(
            Some("1.6.4871 rev598"),
            Some("1.6.4871 rev600"),
            s
        ));
        assert!(!pin_warns(
            Some("1.6.4871 rev600"),
            Some("1.6.4871 rev598"),
            s
        ));
        assert!(pin_warns(Some("1.6.4872 rev1"), Some("1.6.4871 rev9"), s));
    }

    #[test]
    fn v61_unreadable_version_warns() {
        let saves = with_saves();
        assert!(pin_warns(None, Some("1.6.4871 rev598"), saves.path()));
        assert!(pin_warns(
            Some("1.6.4871 rev598"),
            Some("odd"),
            saves.path()
        ));
    }

    #[test]
    fn v61_no_saves_never_warns() {
        let tmp = tempfile::tempdir().expect("tempdir");
        assert!(!pin_warns(
            Some("1.7.4976 rev1"),
            Some("1.6.4871 rev1"),
            tmp.path()
        ));
        let missing = tmp.path().join("Saves");
        assert!(!pin_warns(None, None, &missing));
    }
}
