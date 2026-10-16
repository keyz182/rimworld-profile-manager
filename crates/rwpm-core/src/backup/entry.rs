//! Rules for archive entry names and manifest strings (V73, V74). Restore never joins an
//! unchecked name onto a path: every name goes through here first.

use std::path::{Component, Path, PathBuf};

use crate::builds::BuildKey;

pub const PROFILE_ROOT: &str = "profile";
pub const BUILD_ROOT: &str = "build";

/// Where an entry belongs, and its path below that place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Place {
    /// `rimrim.toml`.
    Manifest,
    /// `profile/<rel>`; `rel` is empty for the `profile` folder itself.
    Profile(PathBuf),
    /// `build/<key>/<rel>`; `rel` is empty for the build folder itself.
    Build { key: BuildKey, rel: PathBuf },
}

/// Sorts an entry name into its place, or says why it is refused (V73): only plain
/// relative names that start in `rimrim.toml`, `profile/` or `build/<key>/`.
pub fn classify(name: &Path) -> Result<Place, String> {
    let mut parts = Vec::new();
    for component in name.components() {
        let Component::Normal(part) = component else {
            return Err(format!("`{}` is not a plain relative path", name.display()));
        };
        let part = part
            .to_str()
            .ok_or_else(|| format!("`{}` is not valid UTF-8", name.display()))?;
        if part.contains(['\\', ':', '\0']) {
            return Err(format!(
                "`{}` has a character not allowed in a path",
                name.display()
            ));
        }
        parts.push(part);
    }
    let outside = || {
        format!(
            "`{}` is outside the places a backup may write",
            name.display()
        )
    };
    match parts.as_slice() {
        [] => Err("an entry has an empty name".into()),
        [first] if *first == super::file::FILE_NAME => Ok(Place::Manifest),
        [first, rest @ ..] if *first == PROFILE_ROOT => Ok(Place::Profile(rest.iter().collect())),
        [first, key, rest @ ..] if *first == BUILD_ROOT => {
            let key = BuildKey::parse(key).map_err(|_| outside())?;
            Ok(Place::Build {
                key,
                rel: rest.iter().collect(),
            })
        }
        _ => Err(outside()),
    }
}

/// Regular files and folders may go anywhere a backup writes. A symlink may sit only below
/// `build/<key>/` (V73: real game builds hold them), and its target goes through
/// [`check_link_target`]. Hardlinks and special files are refused: a link written early
/// could redirect a later entry out of the profile.
pub fn check_kind(kind: tar::EntryType, name: &Path, place: &Place) -> Result<(), String> {
    match (kind, place) {
        (tar::EntryType::Regular | tar::EntryType::Directory, _) => Ok(()),
        (tar::EntryType::Symlink, Place::Build { rel, .. }) if !rel.as_os_str().is_empty() => {
            Ok(())
        }
        _ => Err(format!(
            "`{}` is a link or special file, which a backup may not hold there",
            name.display()
        )),
    }
}

/// A build symlink's target (V73): `../` repeated at most as deep as the link's own folder,
/// then plain names, with at least one name. `link_rel` is the link's path below the build
/// folder. Nothing may step up after a name, so a later link that passes through this one
/// cannot climb out of the build; absolute targets (host paths) never pass.
pub fn check_link_target(link_rel: &Path, target: &Path) -> Result<(), String> {
    let bad = |why: &str| {
        Err(format!(
            "the link `{}` points to `{}`, which {why}",
            link_rel.display(),
            target.display()
        ))
    };
    let depth = link_rel.components().count().saturating_sub(1);
    let (mut ups, mut named) = (0, false);
    for component in target.components() {
        match component {
            Component::ParentDir if named => return bad("steps up after a name"),
            Component::ParentDir => {
                ups += 1;
                if ups > depth {
                    return bad("leaves the game folder");
                }
            }
            Component::Normal(part) => {
                let Some(part) = part.to_str() else {
                    return bad("is not valid UTF-8");
                };
                if part.contains(['\\', ':', '\0']) {
                    return bad("has a character not allowed in a path");
                }
                named = true;
            }
            _ => return bad("is not a plain relative path"),
        }
    }
    if named { Ok(()) } else { bad("names nothing") }
}

/// A mod folder name from a restored `profile.toml` (V74): one plain path component.
/// Refresh joins it onto paths, so a separator, `..` or drive prefix would leave the profile.
pub fn check_folder(folder: &str) -> Result<(), String> {
    let bad = |why: &str| Err(format!("the mod folder name `{folder}` {why}"));
    if folder.is_empty() {
        return bad("is empty");
    }
    if folder == "." || folder == ".." {
        return bad("is not a folder name");
    }
    if folder.contains(['/', '\\', ':', '\0']) {
        return bad("has a path separator or a character not allowed in a name");
    }
    if folder.ends_with(['.', ' ']) {
        return bad("ends with a dot or a space");
    }
    Ok(())
}
