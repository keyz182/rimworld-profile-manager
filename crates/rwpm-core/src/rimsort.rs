//! Export a profile as a RimSort instance (I.file, V9, V21).
//!
//! RimSort keeps instances in `<data-local>/RimSort/settings.json` and converts each one
//! with `msgspec.convert(dict, Instance)` (RimSort d2454505 settings.py:582), so every
//! field must have exactly the model's type or RimSort cannot load its settings.
//! Everything else in the file is kept as it was, in its order.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::consent::{Consent, Scope};
use crate::discovery::GameInstall;
use crate::profile::{Mode, Profile};

const DIR: &str = "RimSort";
const SETTINGS: &str = "settings.json";
const INSTANCES: &str = "instances";

/// `<data-local>/RimSort/settings.json` (platformdirs `user_data_dir("RimSort", appauthor=False)`).
pub fn settings_path() -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| d.join(DIR).join(SETTINGS))
}

#[derive(Debug, thiserror::Error)]
pub enum RimSortError {
    #[error("RimSort's settings were not found at `{}`; open RimSort once first", .0.display())]
    NotFound(PathBuf),
    #[error("cannot {op} `{}`: {source}", path.display())]
    Io {
        op: &'static str,
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("cannot read RimSort's settings `{}`: {message}", path.display())]
    Parse { path: PathBuf, message: String },
    #[error("RimSort is running; close it first so it does not overwrite the change")]
    RimSortRunning,
    #[error("RimSort's settings changed since the export was prepared; try again")]
    Changed,
    #[error("RimSort cannot use the path `{}` in a launch command", .0.display())]
    UnsafePath(PathBuf),
    #[error("no recorded permission to change `{}`", .0.display())]
    NoConsent(PathBuf),
}

/// Is RimSort running? A trait so tests never depend on real processes.
pub trait RimSortProbe {
    fn running(&self) -> bool;
}

pub struct ProcessRimSortProbe;

impl RimSortProbe for ProcessRimSortProbe {
    fn running(&self) -> bool {
        use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};
        let mut sys = System::new();
        sys.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing(),
        );
        sys.processes().values().any(|p| {
            let name = p.name().to_string_lossy();
            name.eq_ignore_ascii_case("RimSort") || name.eq_ignore_ascii_case("RimSort.exe")
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExportPlan {
    pub settings: PathBuf,
    /// Key under `instances` (also the instance's `name`).
    pub instance_name: String,
    /// An instance exported earlier for this profile is updated in place.
    pub updates_existing: bool,
    instance: Map<String, Value>,
    original: Vec<u8>,
}

/// Prepares the instance for `profile`. Reads only.
pub fn plan_export(
    profile: &Profile,
    game: &GameInstall,
    rwpm_exe: &Path,
    settings: &Path,
) -> Result<ExportPlan, RimSortError> {
    let original = match std::fs::read(settings) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(RimSortError::NotFound(settings.to_path_buf()));
        }
        Err(source) => return Err(io("read", settings)(source)),
    };
    let root = parse(settings, &original)?;
    let instances = instances(settings, &root)?;
    let run_args = rimsort_run_args(rwpm_exe, profile.id.as_str(), cfg!(windows))?;
    let own = instances.iter().find(|(_, v)| {
        v.get("run_args")
            .and_then(Value::as_str)
            .is_some_and(|a| runs_profile(a, profile.id.as_str()))
    });
    let (instance_name, existing) = match own {
        Some((name, v)) => (name.clone(), v.as_object().cloned()),
        None => (free_name(instances, &profile.name), None),
    };
    let rimsort_dir = settings.parent().unwrap_or(Path::new(""));
    let fields = Fields {
        name: &instance_name,
        profile,
        game,
        run_args,
        instance_dir: rimsort_dir.join(INSTANCES).join(&instance_name),
    };
    Ok(ExportPlan {
        settings: settings.to_path_buf(),
        updates_existing: existing.is_some(),
        instance: fields.build(existing.unwrap_or_default())?,
        instance_name,
        original,
    })
}

/// Writes the plan into RimSort's settings (V9): refuses while RimSort runs or if the file
/// changed since `plan_export`, backs the file up into `backup_dir` first, then replaces
/// it atomically. Returns the backup's path.
pub fn apply_export(
    plan: &ExportPlan,
    consent: &Consent,
    backup_dir: &Path,
    probe: &dyn RimSortProbe,
) -> Result<PathBuf, RimSortError> {
    if !consent.covers(Scope::RimsortSettings, &plan.settings) {
        return Err(RimSortError::NoConsent(plan.settings.clone()));
    }
    if probe.running() {
        return Err(RimSortError::RimSortRunning);
    }
    let current = std::fs::read(&plan.settings).map_err(io("read", &plan.settings))?;
    if current != plan.original {
        return Err(RimSortError::Changed);
    }
    let backup = write_backup(backup_dir, &current)?;
    let mut root = parse(&plan.settings, &current)?;
    if let Some(list) = root.get_mut(INSTANCES).and_then(Value::as_object_mut) {
        list.insert(
            plan.instance_name.clone(),
            Value::Object(plan.instance.clone()),
        );
    }
    write_atomic(&plan.settings, &to_json(&root, &plan.settings)?)?;
    if let Some(dir) = plan
        .instance
        .get("steamcmd_install_path")
        .and_then(Value::as_str)
    {
        std::fs::create_dir_all(dir).map_err(io("create", dir))?;
    }
    Ok(backup)
}

/// Inputs for one instance's fields.
struct Fields<'a> {
    name: &'a str,
    profile: &'a Profile,
    game: &'a GameInstall,
    run_args: String,
    instance_dir: PathBuf,
}

impl Fields<'_> {
    /// All 13 `Instance` fields with RimSort's types (V21). On update, fields the player
    /// may have changed in RimSort (steamcmd_*, instance_folder_override, initial_setup)
    /// are kept; new instances get RimSort's own defaults for them.
    fn build(&self, mut map: Map<String, Value>) -> Result<Map<String, Value>, RimSortError> {
        let local = match (self.profile.mode, self.profile.mods_dir()) {
            (Mode::Offline, Some(dir)) => dir,
            _ => self.game.mods_dir(),
        };
        let (workshop, integration) = match &self.game.steam {
            Some(s) => (text(&s.workshop_dir)?, true),
            None => (String::new(), false),
        };
        let mut set = |k: &str, v: Value| {
            map.insert(k.to_owned(), v);
        };
        set("name", self.name.into());
        set("game_folder", text(self.game.rimsort_game_folder())?.into());
        set(
            "config_folder",
            text(&self.profile.save_data_dir().join("Config"))?.into(),
        );
        set("local_folder", text(&local)?.into());
        set("workshop_folder", workshop.into());
        set("run_args", self.run_args.clone().into());
        let mut keep = |k: &str, v: Value| {
            map.entry(k.to_owned()).or_insert(v);
        };
        keep("steamcmd_auto_clear_depot_cache", true.into());
        keep("steamcmd_install_path", text(&self.instance_dir)?.into());
        keep("steamcmd_ignore", false.into());
        map.insert("steam_client_integration".into(), integration.into());
        // RimSort ignores run_args when launching through the Steam protocol (R17).
        map.insert("launch_via_steam_protocol".into(), false.into());
        map.entry("instance_folder_override").or_insert("".into());
        map.entry("initial_setup").or_insert(false.into());
        Ok(map)
    }
}

/// `<rwpm> run <id> -- %command%`, quoted for RimSort's shlex parser
/// (launch_command_parser.py: POSIX shlex; on Windows backslashes are doubled first).
/// A token with `=` before `%command%` would be read as an env var, so refuse it.
/// Also what players paste into Steam's launch options for the game.
pub fn launch_options(rwpm: &Path, id: &str, windows: bool) -> Result<String, RimSortError> {
    command_line(rwpm, &format!("run {id} -- %command%"), windows)
}

/// RimSort's `run_args`: the launch line plus `--from rimsort`, so the run is recorded as
/// started by RimSort (I.file last-run).
pub fn rimsort_run_args(rwpm: &Path, id: &str, windows: bool) -> Result<String, RimSortError> {
    let from = crate::profile::last_run::FROM_RIMSORT;
    command_line(
        rwpm,
        &format!("run --from {from} {id} -- %command%"),
        windows,
    )
}

/// RimSort's entry for profile `id`, if it has one: (instance name, its `config_folder`).
/// Read only; an unreadable settings file means "none".
pub fn entry_for(settings: &Path, id: &str) -> Option<(String, String)> {
    let root: Value = serde_json::from_slice(&std::fs::read(settings).ok()?).ok()?;
    root.get(INSTANCES)?
        .as_object()?
        .iter()
        .find(|(_, v)| {
            v.get("run_args")
                .and_then(Value::as_str)
                .is_some_and(|a| runs_profile(a, id))
        })
        .map(|(name, v)| {
            let folder = v.get("config_folder").and_then(Value::as_str);
            (name.clone(), folder.unwrap_or_default().to_owned())
        })
}

/// Is `run_args` one of ours for profile `id`? Matches every form this tool has written,
/// with or without `--from rimsort` (B5: one check for export and Move).
pub fn runs_profile(run_args: &str, id: &str) -> bool {
    run_args.ends_with(&format!(" {id} -- %command%"))
        && (run_args.contains(" run --from ") || run_args.contains(&format!(" run {id} -- ")))
}

fn command_line(rwpm: &Path, args: &str, windows: bool) -> Result<String, RimSortError> {
    let path = text(rwpm)?;
    let quote = if windows { '"' } else { '\'' };
    if path.contains('=') || path.contains(quote) {
        return Err(RimSortError::UnsafePath(rwpm.to_path_buf()));
    }
    Ok(format!("{quote}{path}{quote} {args}"))
}

/// `name`, else `name (rwpm)`, `name (rwpm) 2`, … not already used.
fn free_name(instances: &Map<String, Value>, name: &str) -> String {
    let base = format!("{name} (rwpm)");
    std::iter::once(name.to_owned())
        .chain(std::iter::once(base.clone()))
        .chain((2..).map(|n| format!("{base} {n}")))
        .find(|n| !instances.contains_key(n))
        .unwrap_or(base)
}

fn instances<'a>(path: &Path, root: &'a Value) -> Result<&'a Map<String, Value>, RimSortError> {
    root.get(INSTANCES)
        .and_then(Value::as_object)
        .ok_or_else(|| RimSortError::Parse {
            path: path.to_path_buf(),
            message: "no `instances` table".into(),
        })
}

fn parse(path: &Path, bytes: &[u8]) -> Result<Value, RimSortError> {
    let root: Value = serde_json::from_slice(bytes).map_err(|e| RimSortError::Parse {
        path: path.to_path_buf(),
        message: e.to_string(),
    })?;
    if root.is_object() {
        Ok(root)
    } else {
        Err(RimSortError::Parse {
            path: path.to_path_buf(),
            message: "not a JSON object".into(),
        })
    }
}

/// Pretty JSON with 4-space indents, as RimSort writes it.
fn to_json(root: &Value, path: &Path) -> Result<Vec<u8>, RimSortError> {
    let mut out = Vec::new();
    let fmt = serde_json::ser::PrettyFormatter::with_indent(b"    ");
    let mut ser = serde_json::Serializer::with_formatter(&mut out, fmt);
    serde::Serialize::serialize(root, &mut ser).map_err(|e| RimSortError::Parse {
        path: path.to_path_buf(),
        message: e.to_string(),
    })?;
    Ok(out)
}

fn write_backup(dir: &Path, bytes: &[u8]) -> Result<PathBuf, RimSortError> {
    std::fs::create_dir_all(dir).map_err(io("create", dir))?;
    let stamp = crate::profile::offline::now_datetime()
        .to_string()
        .replace(':', "-");
    let mut path = dir.join(format!("settings-{stamp}.json"));
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("settings-{stamp}-{n}.json"));
        n += 1;
    }
    std::fs::write(&path, bytes).map_err(io("write", &path))?;
    Ok(path)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), RimSortError> {
    let tmp = path.with_extension("json.rwpm-tmp");
    std::fs::write(&tmp, bytes).map_err(io("write", &tmp))?;
    std::fs::rename(&tmp, path).map_err(io("replace", path))
}

fn text(path: &Path) -> Result<String, RimSortError> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| RimSortError::UnsafePath(path.to_path_buf()))
}

fn io(op: &'static str, path: impl Into<PathBuf>) -> impl FnOnce(std::io::Error) -> RimSortError {
    let path = path.into();
    move |source| RimSortError::Io { op, path, source }
}

#[cfg(test)]
mod tests;
