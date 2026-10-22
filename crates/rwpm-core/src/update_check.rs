//! Optional "is there a newer release?" check against this project's GitHub releases.
//!
//! Nothing here runs by itself. The window asks [`UpdateState::is_due`] and, when the
//! player turned checking on, calls [`latest`]. The state lives in `<app data>/updates.toml`,
//! apart from `settings.toml`, so an older build that cannot read it never loses the
//! folder settings.

use std::path::Path;
use std::str::FromStr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::error::ProfileError;
use crate::http::{self, HttpError};

pub const FILE_NAME: &str = "updates.toml";
/// Highest schema this build reads and the one it writes.
pub const SCHEMA: u32 = 2;
/// The newest published release. GitHub leaves out drafts and pre-releases here.
pub const LATEST_URL: &str =
    "https://api.github.com/repos/keyz182/rimworld-profile-manager/releases/latest";
/// The only release pages the window opens.
pub const RELEASES_PREFIX: &str = "https://github.com/keyz182/rimworld-profile-manager/releases/";
/// The only place an update file is fetched from.
pub const DOWNLOAD_PREFIX: &str =
    "https://github.com/keyz182/rimworld-profile-manager/releases/download/";

const TIMEOUT: Duration = Duration::from_secs(15);
const MAX_BODY_BYTES: u64 = 256 * 1024;
const DAY_SECS: u64 = 24 * 60 * 60;
/// After a failed request, background checks wait this long before trying again.
const RETRY_SECS: u64 = 6 * 60 * 60;

/// How often the window checks on its own.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Frequency {
    #[default]
    Off,
    Daily,
    Weekly,
    Monthly,
}

impl Frequency {
    pub const ALL: [Frequency; 4] = [Self::Off, Self::Daily, Self::Weekly, Self::Monthly];

    /// Seconds between checks; `None` when checking is off.
    pub fn interval_secs(self) -> Option<u64> {
        match self {
            Self::Off => None,
            Self::Daily => Some(DAY_SECS),
            Self::Weekly => Some(7 * DAY_SECS),
            Self::Monthly => Some(30 * DAY_SECS),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateState {
    pub schema: u32,
    #[serde(default)]
    pub frequency: Frequency,
    /// Unix seconds of the last check that got an answer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_checked: Option<u64>,
    /// Unix seconds of the last try, answered or not. Drives the retry wait.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_attempt: Option<u64>,
    /// The release the player hid the banner for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dismissed: Option<String>,
    /// The version an install was handed over to. Cleared on the next start; if this build is
    /// still older than it, the install did not happen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending: Option<String>,
}

impl Default for UpdateState {
    fn default() -> Self {
        Self {
            schema: SCHEMA,
            frequency: Frequency::Off,
            last_checked: None,
            last_attempt: None,
            dismissed: None,
            pending: None,
        }
    }
}

impl UpdateState {
    /// Reads `<app_dir>/updates.toml`; a missing file means checking is off.
    pub fn load(app_dir: &Path) -> Result<Self, ProfileError> {
        let path = app_dir.join(FILE_NAME);
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(ProfileError::io("read", &path)(e)),
        };
        let parse_err = |message: String| ProfileError::Parse {
            path: path.clone(),
            message,
        };
        let table: toml::Table = toml::from_str(&text).map_err(|e| parse_err(e.to_string()))?;
        match table
            .get("schema")
            .and_then(toml::Value::as_integer)
            .map(u32::try_from)
        {
            Some(Ok(found)) if found > SCHEMA => Err(ProfileError::SchemaTooNew {
                path: path.clone(),
                found,
                supported: SCHEMA,
            }),
            Some(Ok(_)) => table
                .try_into()
                .map_err(|e: toml::de::Error| parse_err(e.to_string())),
            _ => Err(parse_err("missing or invalid `schema`".into())),
        }
    }

    /// Writes via a temp file + rename, always at this build's schema.
    pub fn save(&self, app_dir: &Path) -> Result<(), ProfileError> {
        let path = app_dir.join(FILE_NAME);
        let current = Self {
            schema: SCHEMA,
            ..self.clone()
        };
        let text = toml::to_string_pretty(&current).map_err(|e| ProfileError::Parse {
            path: path.clone(),
            message: e.to_string(),
        })?;
        std::fs::create_dir_all(app_dir).map_err(ProfileError::io("create", app_dir))?;
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, text).map_err(ProfileError::io("write", &tmp))?;
        std::fs::rename(&tmp, &path).map_err(ProfileError::io("replace", &path))
    }

    /// True when a background check should run now. A clock that moved back counts as due
    /// for `last_checked` (the stamp is unusable) but a recent failed try still waits.
    pub fn is_due(&self, now: u64) -> bool {
        let Some(interval) = self.frequency.interval_secs() else {
            return false;
        };
        if self
            .last_attempt
            .is_some_and(|at| at <= now && now - at < RETRY_SECS)
        {
            return false;
        }
        match self.last_checked {
            Some(at) if at <= now => now - at >= interval,
            _ => true,
        }
    }

    pub fn record_attempt(&mut self, now: u64) {
        self.last_attempt = Some(now);
    }

    pub fn record_success(&mut self, now: u64) {
        self.last_attempt = Some(now);
        self.last_checked = Some(now);
    }

    /// Whether the banner for `release` should show.
    pub fn shows(&self, release: &Release, current: &str) -> bool {
        release.version.is_newer_than(current)
            && self.dismissed.as_deref() != Some(release.version.to_string().as_str())
    }

    /// Settles a hand-over recorded by an earlier run. Returns the version that was meant to
    /// be installed when this build is still older than it; clears `pending` either way.
    pub fn take_failed_install(&mut self, current: &str) -> Option<String> {
        let wanted = self.pending.take()?;
        wanted
            .parse::<Version>()
            .is_ok_and(|v| v.is_newer_than(current))
            .then_some(wanted)
    }
}

/// Current time as Unix seconds.
pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// A plain `major.minor.patch` version, as in this project's `vX.Y.Z` tags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(u64, u64, u64);

impl Version {
    /// True when `self` is newer than `current`. An unreadable `current` is never older.
    pub fn is_newer_than(&self, current: &str) -> bool {
        current.parse::<Version>().is_ok_and(|c| *self > c)
    }
}

impl FromStr for Version {
    type Err = Error;

    /// Accepts `1.2.3` and `v1.2.3`. Anything else (pre-release suffix, missing part) is an error.
    fn from_str(text: &str) -> Result<Self, Error> {
        let bad = || Error::BadTag(text.to_string());
        let mut parts = text.strip_prefix('v').unwrap_or(text).split('.');
        let mut next = || -> Result<u64, Error> {
            let part = parts.next().ok_or_else(bad)?;
            if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
                return Err(bad());
            }
            part.parse().map_err(|_| bad())
        };
        let version = Self(next()?, next()?, next()?);
        if parts.next().is_some() {
            return Err(bad());
        }
        Ok(version)
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

/// The newest published release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub version: Version,
    /// The release page. Always starts with [`RELEASES_PREFIX`].
    pub url: String,
    /// The files attached to the release. Every `url` starts with [`DOWNLOAD_PREFIX`].
    pub assets: Vec<Asset>,
}

impl Release {
    pub fn asset(&self, name: &str) -> Option<&Asset> {
        self.assets.iter().find(|a| a.name == name)
    }
}

/// One file attached to a release.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Asset {
    pub name: String,
    #[serde(rename = "browser_download_url")]
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("could not reach GitHub: {0}")]
    Request(String),
    #[error("GitHub's answer was too large")]
    TooBig,
    #[error("GitHub's answer was not understood: {0}")]
    Parse(String),
    #[error("the newest release has an unexpected tag: {0}")]
    BadTag(String),
    #[error("the newest release points somewhere unexpected: {0}")]
    BadUrl(String),
}

#[derive(Deserialize)]
struct Payload {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    assets: Vec<Asset>,
}

fn is_plain_url(url: &str, prefix: &str) -> bool {
    url.starts_with(prefix) && !url.chars().any(|c| c.is_control() || c.is_whitespace())
}

/// Asks `url` (normally [`LATEST_URL`]) for the newest release. `https_only` is off only in
/// tests, which talk to a local server.
pub fn latest(url: &str, https_only: bool) -> Result<Release, Error> {
    let agent = format!("rwpm/{}", crate::VERSION);
    let body = http::get(
        url,
        https_only,
        MAX_BODY_BYTES,
        TIMEOUT,
        &[
            ("User-Agent", &agent),
            ("Accept", "application/vnd.github+json"),
        ],
    )
    .map_err(|e| match e {
        HttpError::TooBig => Error::TooBig,
        HttpError::Failed(details) => Error::Request(details),
    })?;
    parse(&body)
}

fn parse(body: &[u8]) -> Result<Release, Error> {
    let payload: Payload = serde_json::from_slice(body).map_err(|e| Error::Parse(e.to_string()))?;
    let version = payload.tag_name.parse::<Version>()?;
    if !is_plain_url(&payload.html_url, RELEASES_PREFIX) {
        return Err(Error::BadUrl(payload.html_url));
    }
    if let Some(bad) = payload
        .assets
        .iter()
        .find(|a| !is_plain_url(&a.url, DOWNLOAD_PREFIX))
    {
        return Err(Error::BadUrl(bad.url.clone()));
    }
    Ok(Release {
        version,
        url: payload.html_url,
        assets: payload.assets,
    })
}

#[cfg(test)]
mod tests;
