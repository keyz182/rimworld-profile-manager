//! The Steam app manifest, `<library>/steamapps/appmanifest_294100.acf`.
//!
//! Valve's KeyValues text format. Only three values matter here, so this reads the file
//! with a brace-depth walk rather than pulling in a general VDF parser:
//!
//! - `buildid` — the build identity (`Version.txt` lags the binary);
//! - `StateFlags` — must be exactly 4 to capture;
//! - `BetaKey` — the Steam branch, `""` for the default one.
//!
//! `BetaKey` appears twice: once under `UserConfig` (the branch the player has *selected*)
//! and once under `MountedConfig` (the branch actually *installed*). A capture copies the
//! installed bits, so `MountedConfig` wins; `UserConfig` is the fallback. Those two differ
//! between a branch switch and the download that follows it.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::error::ProfileError;

/// Which block supplied `beta`. Stored so a mismatch can be reported rather than guessed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BetaSource {
    /// `MountedConfig`: the branch whose files are on disk.
    Mounted,
    /// `UserConfig`: the selected branch, used when `MountedConfig` has no `BetaKey`.
    User,
    /// No `BetaKey` anywhere, which is how the default branch usually reads.
    Absent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppManifest {
    pub buildid: String,
    pub state_flags: u64,
    pub beta: String,
    pub beta_source: BetaSource,
}

impl AppManifest {
    /// 4 exactly. 1030 (mid-update) and 36 (pre-launch) both carry bit 4, so a mask
    /// test would let a half-downloaded install through.
    pub fn fully_installed(&self) -> bool {
        self.state_flags == 4
    }
}

pub fn read(path: &Path) -> Result<AppManifest, ProfileError> {
    let text = std::fs::read_to_string(path).map_err(ProfileError::io("read", path))?;
    parse(&text).ok_or_else(|| ProfileError::Parse {
        path: path.to_path_buf(),
        message: "no `buildid` or `StateFlags` in the Steam app manifest".to_owned(),
    })
}

fn parse(text: &str) -> Option<AppManifest> {
    let (mut buildid, mut state_flags) = (None, None);
    let (mut mounted, mut user) = (None, None);
    let mut block: Vec<String> = Vec::new();
    let mut pending: Option<String> = None;
    for line in text.lines() {
        let line = line.trim();
        match line {
            "{" => block.push(pending.take().unwrap_or_default()),
            "}" => {
                block.pop();
            }
            _ => {
                let mut tokens = quoted(line);
                let Some(key) = tokens.next() else { continue };
                match tokens.next() {
                    // A section name sits alone on its line; its `{` follows.
                    None => pending = Some(key.to_ascii_lowercase()),
                    Some(value) => {
                        let here = block.last().map(String::as_str).unwrap_or_default();
                        match key.to_ascii_lowercase().as_str() {
                            "buildid" if buildid.is_none() => buildid = Some(value.to_owned()),
                            "stateflags" if state_flags.is_none() => {
                                state_flags = value.parse().ok();
                            }
                            "betakey" => match here {
                                "mountedconfig" if mounted.is_none() => {
                                    mounted = Some(value.to_owned());
                                }
                                "userconfig" if user.is_none() => user = Some(value.to_owned()),
                                _ => {}
                            },
                            _ => {}
                        }
                    }
                }
            }
        }
    }
    let (beta, beta_source) = match (mounted, user) {
        (Some(b), _) => (b, BetaSource::Mounted),
        (None, Some(b)) => (b, BetaSource::User),
        (None, None) => (String::new(), BetaSource::Absent),
    };
    Some(AppManifest {
        buildid: buildid?,
        state_flags: state_flags?,
        beta,
        beta_source,
    })
}

/// `<library>/steamapps/workshop/appworkshop_294100.acf`: two folders above the Workshop
/// content folder (`<library>/steamapps/workshop/content/294100`).
pub fn workshop_manifest_path(workshop_dir: &Path) -> Option<PathBuf> {
    Some(
        workshop_dir
            .parent()?
            .parent()?
            .join("appworkshop_294100.acf"),
    )
}

/// `timeupdated` (unix seconds) of every installed Workshop item, by item id, from
/// `<library>/steamapps/workshop/appworkshop_294100.acf` (block `WorkshopItemsInstalled`).
/// A missing or unreadable file reads as empty: the time is only a hint for a modpack.
pub fn read_workshop_times(path: &Path) -> BTreeMap<u64, u64> {
    std::fs::read_to_string(path)
        .map(|text| parse_workshop_times(&text))
        .unwrap_or_default()
}

/// One `"key" "value"` line three levels down: `(block, item id, key, value)`, the block
/// and key lower case. Both Workshop readers walk the file once through this.
fn workshop_lines(text: &str) -> Vec<(String, String, String, String)> {
    let mut lines = Vec::new();
    let mut block: Vec<String> = Vec::new();
    let mut pending: Option<String> = None;
    for line in text.lines() {
        match line.trim() {
            "{" => block.push(pending.take().unwrap_or_default()),
            "}" => {
                block.pop();
            }
            line => {
                let mut tokens = quoted(line);
                let Some(key) = tokens.next() else { continue };
                match (tokens.next(), block.as_slice()) {
                    (None, _) => pending = Some(key.to_ascii_lowercase()),
                    (Some(value), [_, group, item]) => {
                        lines.push((group.clone(), item.clone(), key.to_ascii_lowercase(), value));
                    }
                    _ => {}
                }
            }
        }
    }
    lines
}

fn parse_workshop_times(text: &str) -> BTreeMap<u64, u64> {
    let mut times = BTreeMap::new();
    for (group, item, key, value) in workshop_lines(text) {
        if group == "workshopitemsinstalled"
            && key == "timeupdated"
            && let (Ok(item), Ok(value)) = (item.parse(), value.parse())
        {
            times.insert(item, value);
        }
    }
    times
}

/// The Workshop items Steam has finished with: installed, and not behind the version it
/// knows is latest. An item that is being downloaded or updated is not in this set.
/// A missing or unreadable file reads as empty.
pub fn read_workshop_synced(path: &Path) -> BTreeSet<u64> {
    std::fs::read_to_string(path)
        .map(|text| parse_workshop_synced(&text))
        .unwrap_or_default()
}

fn parse_workshop_synced(text: &str) -> BTreeSet<u64> {
    let mut installed = BTreeSet::new();
    let mut manifest: BTreeMap<u64, String> = BTreeMap::new();
    let mut latest: BTreeMap<u64, String> = BTreeMap::new();
    for (group, item, key, value) in workshop_lines(text) {
        let Ok(id) = item.parse::<u64>() else { continue };
        match (group.as_str(), key.as_str()) {
            ("workshopitemsinstalled", "timeupdated" | "manifest") => {
                installed.insert(id);
            }
            ("workshopitemdetails", "manifest") => {
                manifest.insert(id, value);
            }
            ("workshopitemdetails", "latest_manifest") => {
                latest.insert(id, value);
            }
            _ => {}
        }
    }
    installed
        .into_iter()
        .filter(|id| match (manifest.get(id), latest.get(id)) {
            (Some(now), Some(newest)) => now == newest,
            _ => true,
        })
        .collect()
}

/// The double-quoted tokens of one line, unescaped only for `\"` and `\\`.
fn quoted(line: &str) -> impl Iterator<Item = String> + '_ {
    let mut chars = line.chars().peekable();
    std::iter::from_fn(move || {
        chars.by_ref().find(|c| *c == '"')?;
        let mut out = String::new();
        loop {
            match chars.next()? {
                '"' => return Some(out),
                '\\' => out.push(chars.next()?),
                c => out.push(c),
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(state: &str, user: &str, mounted: &str) -> String {
        format!(
            "\"AppState\"\n{{\n\t\"appid\"\t\t\"294100\"\n\t\"StateFlags\"\t\t\"{state}\"\n\
             \t\"buildid\"\t\t\"19234567\"\n\t\"UserConfig\"\n\t{{\n\t\t\"BetaKey\"\t\t\"{user}\"\n\
             \t\t\"language\"\t\t\"english\"\n\t}}\n\t\"MountedConfig\"\n\t{{\n\
             \t\t\"BetaKey\"\t\t\"{mounted}\"\n\t}}\n}}\n"
        )
    }

    const WORKSHOP: &str = "\"AppWorkshop\"\n{\n\t\"appid\"\t\t\"294100\"\n\
        \t\"TimeLastUpdated\"\t\t\"1792413618\"\n\t\"WorkshopItemsInstalled\"\n\t{\n\
        \t\t\"717575199\"\n\t\t{\n\t\t\t\"size\"\t\t\"1459202\"\n\
        \t\t\t\"timeupdated\"\t\t\"1785962405\"\n\t\t\t\"manifest\"\t\t\"5231497147622646016\"\n\t\t}\n\
        \t\t\"730936602\"\n\t\t{\n\t\t\t\"timeupdated\"\t\t\"1786739756\"\n\t\t}\n\t}\n\
        \t\"WorkshopItemDetails\"\n\t{\n\t\t\"717575199\"\n\t\t{\n\
        \t\t\t\"timeupdated\"\t\t\"1\"\n\t\t}\n\t\t\"999\"\n\t\t{\n\
        \t\t\t\"timeupdated\"\t\t\"2\"\n\t\t}\n\t}\n}\n";

    #[test]
    fn reads_workshop_times_of_installed_items_only() {
        let times = parse_workshop_times(WORKSHOP);
        assert_eq!(times.len(), 2);
        assert_eq!(times[&717_575_199], 1_785_962_405);
        assert_eq!(times[&730_936_602], 1_786_739_756);
        assert!(!times.contains_key(&999), "details block is not installed");
    }

    /// Item 1 is done; 2 is installed but Steam knows a newer manifest; 3 has details only
    /// (still downloading); 4 is installed with no details at all.
    const SYNC: &str = "\"AppWorkshop\"\n{\n\t\"WorkshopItemsInstalled\"\n\t{\n\
        \t\t\"1\"\n\t\t{\n\t\t\t\"manifest\"\t\t\"a\"\n\t\t}\n\
        \t\t\"2\"\n\t\t{\n\t\t\t\"manifest\"\t\t\"b\"\n\t\t}\n\
        \t\t\"4\"\n\t\t{\n\t\t\t\"timeupdated\"\t\t\"5\"\n\t\t}\n\t}\n\
        \t\"WorkshopItemDetails\"\n\t{\n\
        \t\t\"1\"\n\t\t{\n\t\t\t\"manifest\"\t\t\"a\"\n\t\t\t\"latest_manifest\"\t\t\"a\"\n\t\t}\n\
        \t\t\"2\"\n\t\t{\n\t\t\t\"manifest\"\t\t\"b\"\n\t\t\t\"latest_manifest\"\t\t\"c\"\n\t\t}\n\
        \t\t\"3\"\n\t\t{\n\t\t\t\"manifest\"\t\t\"0\"\n\t\t\t\"latest_manifest\"\t\t\"d\"\n\t\t}\n\t}\n}\n";

    #[test]
    fn an_item_is_synced_when_installed_and_not_behind_the_latest() {
        let done = parse_workshop_synced(SYNC);
        assert_eq!(done.into_iter().collect::<Vec<_>>(), [1, 4]);
    }

    #[test]
    fn synced_survives_junk_and_a_missing_file() {
        assert!(parse_workshop_synced("not a manifest").is_empty());
        assert!(read_workshop_synced(Path::new("/nonexistent/appworkshop.acf")).is_empty());
    }

    #[test]
    fn workshop_times_survive_junk_and_a_missing_file() {
        let junk = WORKSHOP.replace("\"1785962405\"", "\"soon\"");
        assert_eq!(parse_workshop_times(&junk).len(), 1);
        assert!(parse_workshop_times("not a manifest").is_empty());
        assert!(read_workshop_times(Path::new("/nonexistent/appworkshop.acf")).is_empty());
    }

    #[test]
    fn reads_buildid_and_state_flags() {
        let m = parse(&manifest("4", "", "")).expect("parse");
        assert_eq!(m.buildid, "19234567");
        assert_eq!(m.state_flags, 4);
        assert!(m.fully_installed());
    }

    /// 1030 and 36 both carry bit 4 but are not a finished install.
    #[test]
    fn only_state_flags_four_counts_as_installed() {
        for state in ["0", "1030", "36", "6", "516"] {
            let m = parse(&manifest(state, "", "")).expect("parse");
            assert!(!m.fully_installed(), "StateFlags={state} passed");
        }
    }

    /// The installed branch wins over the selected one: they differ between a branch
    /// switch and the download that follows it.
    #[test]
    fn mounted_beta_key_wins_over_user_beta_key() {
        let m = parse(&manifest("4", "unstable", "")).expect("parse");
        assert_eq!(m.beta, "");
        assert_eq!(m.beta_source, BetaSource::Mounted);

        let m = parse(&manifest("4", "", "unstable")).expect("parse");
        assert_eq!(m.beta, "unstable");
        assert_eq!(m.beta_source, BetaSource::Mounted);
    }

    #[test]
    fn user_beta_key_is_the_fallback_when_nothing_is_mounted() {
        let text = "\"AppState\"\n{\n\t\"StateFlags\"\t\t\"4\"\n\t\"buildid\"\t\t\"1\"\n\
                    \t\"UserConfig\"\n\t{\n\t\t\"BetaKey\"\t\t\"unstable\"\n\t}\n}\n";
        let m = parse(text).expect("parse");
        assert_eq!(m.beta, "unstable");
        assert_eq!(m.beta_source, BetaSource::User);
    }

    #[test]
    fn no_beta_key_reads_as_the_default_branch() {
        let text = "\"AppState\"\n{\n\t\"StateFlags\"\t\t\"4\"\n\t\"buildid\"\t\t\"1\"\n}\n";
        let m = parse(text).expect("parse");
        assert_eq!(m.beta, "");
        assert_eq!(m.beta_source, BetaSource::Absent);
    }

    #[test]
    fn a_manifest_without_buildid_is_a_parse_error() {
        let text = "\"AppState\"\n{\n\t\"StateFlags\"\t\t\"4\"\n}\n";
        assert!(parse(text).is_none());
        let text = "\"AppState\"\n{\n\t\"buildid\"\t\t\"1\"\n}\n";
        assert!(parse(text).is_none());
    }

    /// A depot block carries its own keys; they must not be mistaken for the app's.
    #[test]
    fn nested_depot_blocks_do_not_shadow_the_app_values() {
        let text = "\"AppState\"\n{\n\t\"buildid\"\t\t\"19234567\"\n\t\"StateFlags\"\t\t\"4\"\n\
                    \t\"InstalledDepots\"\n\t{\n\t\t\"294101\"\n\t\t{\n\
                    \t\t\t\"manifest\"\t\t\"777\"\n\t\t\t\"BetaKey\"\t\t\"depotbeta\"\n\
                    \t\t}\n\t}\n}\n";
        let m = parse(text).expect("parse");
        assert_eq!(m.buildid, "19234567");
        assert_eq!(m.beta, "", "a depot BetaKey was taken for the app's");
    }
}
