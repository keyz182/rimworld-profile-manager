//! `<profile>/last-run.toml`: what happened the last time `rwpm run` was
//! asked to start a profile, and who asked. Default has no folder, so its record lives in
//! `<profiles root>/default/`, a folder the profile list skips.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::offline::now_datetime;
use super::store::{Location, Profile};

pub const FILE_NAME: &str = "last-run.toml";
/// Set by the app's own Play, so `rwpm run` knows the app started it.
pub const STARTED_BY_ENV: &str = "RWPM_STARTED_BY";
/// `rwpm run --from rimsort …`: written into RimSort's run_args by the export.
pub const FROM_RIMSORT: &str = "rimsort";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StartedBy {
    Steam,
    App,
    Rimsort,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RunOutcome {
    /// The game started; `exit` is its exit code.
    Ok,
    /// `rwpm run` did not start it: another game, the lock, the player's Cancel, a loader check.
    Refused,
    /// Starting it went wrong.
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LastRun {
    pub time: toml::value::Datetime,
    pub started_by: StartedBy,
    pub outcome: RunOutcome,
    /// Plain words for refused and failed runs; empty when it worked.
    #[serde(default)]
    pub reason: String,
    pub exit: Option<i32>,
}

impl LastRun {
    pub fn now(
        started_by: StartedBy,
        outcome: RunOutcome,
        reason: String,
        exit: Option<i32>,
    ) -> Self {
        Self {
            time: now_datetime(),
            started_by,
            outcome,
            reason,
            exit,
        }
    }

    /// Where `profile`'s record is, under the profiles root `root`.
    pub fn path(root: &Path, profile: &Profile) -> PathBuf {
        match &profile.location {
            Location::Stored { dir } => dir.join(FILE_NAME),
            Location::Default { .. } => root.join(profile.id.as_str()).join(FILE_NAME),
        }
    }

    /// Writes via a temp file and rename, so a reader never sees half a record.
    pub fn write(&self, path: &Path) -> std::io::Result<()> {
        let text = toml::to_string(self).map_err(std::io::Error::other)?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, path)
    }

    /// The record, or `None` when there is none or it cannot be read (it is only a hint).
    pub fn read(path: &Path) -> Option<Self> {
        toml::from_str(&std::fs::read_to_string(path).ok()?).ok()
    }

    /// Seconds since this run, by the clock now; `None` if the time is in the future or bad.
    pub fn age_secs(&self) -> Option<u64> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
        now.checked_sub(unix_secs(&self.time)?)
    }
}

/// Who started `rwpm run`. Decided once, before `rwpm run` changes its own
/// environment (it sets `SteamAppId` for the game). The first match wins.
pub fn started_by(var: &dyn Fn(&str) -> Option<OsString>, from: Option<&str>) -> StartedBy {
    if var(STARTED_BY_ENV).is_some_and(|v| v == "app") {
        StartedBy::App
    } else if from == Some(FROM_RIMSORT) {
        StartedBy::Rimsort
    } else if var("SteamGameId").is_some() || var("SteamAppId").is_some() {
        StartedBy::Steam
    } else {
        StartedBy::Other
    }
}

/// A UTC TOML datetime → Unix seconds (days-from-civil, H. Hinnant).
fn unix_secs(t: &toml::value::Datetime) -> Option<u64> {
    let (date, time) = (t.date?, t.time?);
    let (y, m, d) = (
        i64::from(date.year),
        i64::from(date.month),
        i64::from(date.day),
    );
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let secs = days * 86_400
        + i64::from(time.hour) * 3600
        + i64::from(time.minute) * 60
        + i64::from(time.second.unwrap_or(0));
    let offset_mins = match t.offset {
        Some(toml::value::Offset::Custom { minutes }) => i64::from(minutes),
        _ => 0,
    };
    u64::try_from(secs - offset_mins * 60).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::ProfileId;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let pairs: Vec<(String, String)> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        move |k| pairs.iter().find(|(n, _)| n == k).map(|(_, v)| v.into())
    }

    #[test]
    fn first_matching_rule_decides_who_started_it() {
        let steam = [("SteamGameId", "294100")];
        // (environment, --from, expected)
        type Case<'a> = (&'a [(&'a str, &'a str)], Option<&'a str>, StartedBy);
        let cases: [Case; 6] = [
            (
                &[("RWPM_STARTED_BY", "app"), ("SteamAppId", "294100")],
                None,
                StartedBy::App,
            ),
            (
                &[("RWPM_STARTED_BY", "app")],
                Some("rimsort"),
                StartedBy::App,
            ),
            (&steam, Some("rimsort"), StartedBy::Rimsort),
            (&steam, None, StartedBy::Steam),
            (&[("SteamAppId", "294100")], None, StartedBy::Steam),
            (&[("RWPM_STARTED_BY", "other")], None, StartedBy::Other),
        ];
        for (vars, from, want) in cases {
            assert_eq!(started_by(&env(vars), from), want, "{vars:?} {from:?}");
        }
    }

    #[test]
    fn record_round_trips_and_bad_files_read_as_none() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("p").join(FILE_NAME);
        let run = LastRun::now(
            StartedBy::Steam,
            RunOutcome::Refused,
            "RimWorld is already running".into(),
            None,
        );
        run.write(&path).expect("write");
        assert_eq!(LastRun::read(&path), Some(run));
        assert!(!path.with_extension("toml.tmp").exists());
        std::fs::write(&path, "outcome = \"maybe\"").expect("w");
        assert_eq!(LastRun::read(&path), None);
        assert_eq!(LastRun::read(&tmp.path().join("none.toml")), None);
    }

    #[test]
    fn default_record_lives_in_a_folder_the_list_skips() {
        let root = Path::new("/r/profiles");
        let default = Profile {
            id: ProfileId::default_id(),
            name: "Default".into(),
            mode: crate::profile::Mode::Live,
            game_build: None,
            location: Location::Default {
                save_dir: "/game".into(),
            },
        };
        assert_eq!(
            LastRun::path(root, &default),
            root.join("default").join(FILE_NAME)
        );
    }

    #[test]
    fn age_counts_from_the_recorded_utc_time() {
        let mut run = LastRun::now(StartedBy::App, RunOutcome::Ok, String::new(), Some(0));
        assert!(run.age_secs().is_some_and(|a| a < 5));
        run.time = "1970-01-02T00:00:00Z".parse().expect("t");
        assert_eq!(unix_secs(&run.time), Some(86_400));
        run.time = "2026-10-12T12:00:00+01:00".parse().expect("t");
        assert_eq!(
            unix_secs(&run.time),
            unix_secs(&"2026-10-12T11:00:00Z".parse().expect("t"))
        );
    }
}
