//! Recorded consent for writes outside the tool's own folders.
//!
//! A `Consent` can only come from this store, so core code that writes elsewhere takes
//! one as proof the player agreed to that exact change. Kept in `<app data>/consent.toml`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const FILE: &str = "consent.toml";

/// What the player agreed to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Scope {
    /// Add or update instances in RimSort's `settings.json`.
    RimsortSettings,
    /// Put our `winhttp.dll` beside `RimWorldWin64.exe` (Windows OFFLINE).
    WinhttpDll,
    /// Download steamcmd from Valve into this app's own `steamcmd` folder and run it.
    Steamcmd,
}

/// Proof of recorded consent for one scope and target path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Consent {
    scope: Scope,
    target: PathBuf,
}

impl Consent {
    /// True when this consent covers `scope` on `target`.
    pub fn covers(&self, scope: Scope, target: &Path) -> bool {
        self.scope == scope && self.target == target
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConsentError {
    #[error("cannot {op} `{}`: {source}", path.display())]
    Io {
        op: &'static str,
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("cannot read `{}`: {message}", path.display())]
    Parse { path: PathBuf, message: String },
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConsentFile {
    #[serde(default, rename = "consent")]
    entries: Vec<Entry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    scope: Scope,
    target: PathBuf,
    granted_at: toml::value::Datetime,
}

pub struct ConsentStore {
    path: PathBuf,
}

impl ConsentStore {
    /// The store in `app_dir` (normally `paths::app_data_dir()`).
    pub fn open(app_dir: &Path) -> Self {
        Self {
            path: app_dir.join(FILE),
        }
    }

    /// Consent already given for `scope` on `target`, if any.
    pub fn find(&self, scope: Scope, target: &Path) -> Result<Option<Consent>, ConsentError> {
        Ok(self
            .read()?
            .entries
            .iter()
            .any(|e| e.scope == scope && e.target == target)
            .then(|| Consent {
                scope,
                target: target.to_path_buf(),
            }))
    }

    /// Records that the player agreed; call only right after they did.
    pub fn record(&self, scope: Scope, target: &Path) -> Result<Consent, ConsentError> {
        if let Some(c) = self.find(scope, target)? {
            return Ok(c);
        }
        let mut file = self.read()?;
        file.entries.push(Entry {
            scope,
            target: target.to_path_buf(),
            granted_at: crate::profile::offline::now_datetime(),
        });
        self.write(&file)?;
        Ok(Consent {
            scope,
            target: target.to_path_buf(),
        })
    }

    fn read(&self) -> Result<ConsentFile, ConsentError> {
        match std::fs::read_to_string(&self.path) {
            Ok(text) => toml::from_str(&text).map_err(|e| ConsentError::Parse {
                path: self.path.clone(),
                message: e.to_string(),
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(ConsentFile::default()),
            Err(source) => Err(ConsentError::Io {
                op: "read",
                path: self.path.clone(),
                source,
            }),
        }
    }

    fn write(&self, file: &ConsentFile) -> Result<(), ConsentError> {
        let io = |op, source| ConsentError::Io {
            op,
            path: self.path.clone(),
            source,
        };
        let text = toml::to_string_pretty(file).map_err(|e| ConsentError::Parse {
            path: self.path.clone(),
            message: e.to_string(),
        })?;
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| io("create", e))?;
        }
        let tmp = self.path.with_extension("toml.tmp");
        std::fs::write(&tmp, text).map_err(|e| io("write", e))?;
        std::fs::rename(&tmp, &self.path).map_err(|e| io("replace", e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consent_record_and_find_roundtrip() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = ConsentStore::open(&tmp.path().join("app"));
        let target = tmp.path().join("RimSort/settings.json");
        assert!(
            store
                .find(Scope::RimsortSettings, &target)
                .expect("find")
                .is_none()
        );
        let c = store
            .record(Scope::RimsortSettings, &target)
            .expect("record");
        assert!(c.covers(Scope::RimsortSettings, &target));
        assert!(!c.covers(Scope::RimsortSettings, &tmp.path().join("other.json")));
        // Persisted: a fresh store sees it, and recording again adds no duplicate.
        let again = ConsentStore::open(&tmp.path().join("app"));
        assert_eq!(
            again.find(Scope::RimsortSettings, &target).expect("find"),
            Some(c)
        );
        again
            .record(Scope::RimsortSettings, &target)
            .expect("record");
        let text = std::fs::read_to_string(tmp.path().join("app").join(FILE)).expect("file");
        assert_eq!(text.matches("[[consent]]").count(), 1, "{text}");
        assert!(text.contains("scope = \"rimsort-settings\""), "{text}");
    }

    #[test]
    fn unreadable_store_is_an_error_not_consent() {
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::write(tmp.path().join(FILE), "not = [toml").expect("w");
        let store = ConsentStore::open(tmp.path());
        assert!(matches!(
            store.find(Scope::RimsortSettings, Path::new("/x")),
            Err(ConsentError::Parse { .. })
        ));
    }
}
