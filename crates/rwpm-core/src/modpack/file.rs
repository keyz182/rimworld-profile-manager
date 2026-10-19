//! `rimpack.toml`, the first entry of a `.rimpack` (§I file `.rimpack`, C.58b).
//!
//! A pack holds no mod files and no game files. `[[mods]]` is the preview and fetch list;
//! the load order itself is the archived `ModsConfig.xml`. Every string in here is checked
//! on read, because later steps build folder names, command lines and links from them.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::ProfileError;

/// Highest schema this build reads and the one it writes.
pub const SCHEMA: u32 = 1;
pub const FILE_NAME: &str = "rimpack.toml";

const MAX_MODS: usize = 5_000;
const MAX_TEXT: usize = 512;
const MAX_ID: usize = 128;

/// What the pack creator chose to put in. `ModsConfig.xml` is always in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Includes {
    pub mod_settings: bool,
    pub creations: bool,
    pub game_settings: bool,
    /// File names below `Saves/`, for example `Colony.rws`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub saves: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModEntry {
    pub package_id: String,
    pub name: String,
    /// A TOML integer, so a text value such as `1/../..` fails to read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workshop_id: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Steam's `timeupdated` (unix seconds) of the Workshop copy the pack was made with.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_updated: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// Field order matters to the TOML writer: plain values, then the tables.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema: u32,
    pub created_at: toml::value::Datetime,
    pub rwpm_version: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub game_version: Option<String>,
    /// Package ids of the expansions the pack needs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dlc: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_save: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collection_url: Option<String>,
    /// Total bytes of every file after `rimpack.toml`, and how many there are: the limits
    /// restore enforces while it streams.
    pub uncompressed_bytes: u64,
    pub entry_count: u64,
    pub includes: Includes,
    #[serde(default)]
    pub mods: Vec<ModEntry>,
}

impl Manifest {
    /// Parses and validates the text of `rimpack.toml`. `path` names the pack, for errors.
    /// A newer schema is refused before field checks run.
    pub fn parse(text: &str, path: &Path) -> Result<Self, ProfileError> {
        let bad = |reason: String| ProfileError::BadModpack {
            path: path.to_path_buf(),
            reason,
        };
        let table: toml::Table = toml::from_str(text).map_err(|e| bad(e.to_string()))?;
        let found = table.get("schema").and_then(toml::Value::as_integer);
        match found.map(u32::try_from) {
            Some(Ok(found)) if found > SCHEMA => {
                return Err(ProfileError::SchemaTooNew {
                    path: path.to_path_buf(),
                    found,
                    supported: SCHEMA,
                });
            }
            Some(Ok(_)) => {}
            _ => return Err(bad("missing or invalid `schema`".into())),
        }
        let manifest: Self = table
            .try_into()
            .map_err(|e: toml::de::Error| bad(e.to_string()))?;
        manifest.check().map_err(bad)?;
        Ok(manifest)
    }

    pub fn to_toml(&self) -> Result<String, ProfileError> {
        toml::to_string_pretty(self).map_err(|e| ProfileError::BadModpack {
            path: FILE_NAME.into(),
            reason: e.to_string(),
        })
    }

    pub(crate) fn check(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("the pack has no name".into());
        }
        if self.mods.len() > MAX_MODS {
            return Err(format!("it lists more than {MAX_MODS} mods"));
        }
        for entry in &self.mods {
            check_mod(entry)?;
        }
        for id in &self.dlc {
            check_package_id(id)?;
        }
        for url in self.collection_url.iter() {
            check_link(url)?;
        }
        let mut seen = std::collections::BTreeSet::new();
        for save in &self.includes.saves {
            check_save_name(save)?;
            if !seen.insert(save) {
                return Err(format!("the save `{save}` is listed twice"));
            }
        }
        if let Some(start) = &self.start_save
            && !self.includes.saves.contains(start)
        {
            return Err(format!("the start save `{start}` is not one of its saves"));
        }
        Ok(())
    }
}

fn check_mod(entry: &ModEntry) -> Result<(), String> {
    check_package_id(&entry.package_id)?;
    if entry.name.is_empty()
        || entry.name.len() > MAX_TEXT
        || entry.name.chars().any(char::is_control)
    {
        return Err(format!("`{}` has no usable name", entry.package_id));
    }
    if entry.workshop_id == Some(0) {
        return Err(format!("`{}` has a Workshop id of 0", entry.package_id));
    }
    if let Some(url) = &entry.url {
        check_link(url)?;
    }
    if let Some(version) = &entry.version
        && (version.len() > MAX_ID || version.chars().any(char::is_control))
    {
        return Err(format!("`{}` has an unusable version", entry.package_id));
    }
    Ok(())
}

/// A package id as About.xml writes it: letters, digits, `.`, `_` and `-` only, so it is
/// safe in a file name, a command line and a link.
pub fn check_package_id(id: &str) -> Result<(), String> {
    let plain = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-');
    if id.is_empty() || id.len() > MAX_ID || !id.chars().all(plain) {
        return Err(format!("`{}` is not a package id", id.escape_debug()));
    }
    Ok(())
}

/// Links are only ever shown or opened in a browser: web addresses, nothing else.
pub fn check_link(url: &str) -> Result<(), String> {
    let web = url.starts_with("https://") || url.starts_with("http://");
    let clean = !url.chars().any(|c| c.is_control() || c.is_whitespace());
    if !web || !clean || url.len() > MAX_TEXT {
        return Err(format!("`{}` is not a web address", url.escape_debug()));
    }
    Ok(())
}

/// A save is one plain file name below `Saves/`.
pub fn check_save_name(name: &str) -> Result<(), String> {
    let stem = name.strip_suffix(".rws").unwrap_or("");
    let plain = !stem.is_empty()
        && !name.starts_with('.')
        && name.len() <= 200
        && !name.contains(['/', '\\', ':', '\0'])
        && !name.chars().any(char::is_control);
    if plain {
        Ok(())
    } else {
        Err(format!("`{}` is not a save file name", name.escape_debug()))
    }
}
