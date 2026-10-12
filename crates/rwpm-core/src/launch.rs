//! Launch builder: profile + `%command%` + OS → exact program, args, cwd and env.
//! Pure: spawning lives in `rwpm run` (T10). Recipes verified live in T2/T3 (R20, R21).

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use crate::profile::{Mode, Profile};

pub const STEAM_APP_ID: &str = "294100";
pub const ENV_MODS_DIR: &str = "RWPM_MODS_DIR";
pub const ENV_PAYLOAD_LOG: &str = "RWPM_PAYLOAD_LOG";
/// V28: set on Linux OFFLINE games; the in-game Restart re-runs rwpm, which reads these.
pub const ENV_RESTART: &str = "RWPM_RESTART";
pub const ENV_GAME_EXE: &str = "RWPM_GAME_EXE";
/// A foreign Doorstop's own target assembly, started by our payload after the redirect.
pub const ENV_CHAIN_TARGET: &str = "RWPM_CHAIN_TARGET";
/// V29: first arg of the inner rwpm that takes the start script's place behind wrappers.
pub const ARG_INNER: &str = "--rwpm-inner";
const LINUX_START_SCRIPT: &str = "start_RimWorld.sh";
const LINUX_ELF: &str = "RimWorldLinux";
const LINUX_DOORSTOP_LIB: &str = "libdoorstop.so";
const PAYLOAD_LOG: &str = "payload.log";
const DOORSTOP_GUARDS: [&str; 2] = ["DOORSTOP_INITIALIZED", "DOORSTOP_DISABLE"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Linux,
    Windows,
}

impl Os {
    pub fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else {
            Self::Linux
        }
    }
}

/// Payload + Doorstop files shipped beside the app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PayloadFiles {
    pub dll: PathBuf,
    /// Linux: dir holding `libdoorstop.so`. Windows: unused (winhttp.dll lives in the game dir, T12).
    pub doorstop_dir: PathBuf,
    /// Linux: the `rwpm` binary, given to the game as argv[0] so an in-game Restart
    /// re-enters rwpm (V28). `None`: no trampoline, Doorstop stays on for helpers.
    pub trampoline: Option<PathBuf>,
    /// Windows: the assembly a reused foreign Doorstop would have loaded (T12).
    pub chain_target: Option<PathBuf>,
}

impl PayloadFiles {
    /// `<app>/payload/Rwpm.Payload.dll`, `<app>/doorstop/`.
    pub fn beside(app_dir: &Path) -> Self {
        Self {
            dll: app_dir.join("payload").join("Rwpm.Payload.dll"),
            doorstop_dir: app_dir.join("doorstop"),
            trampoline: None,
            chain_target: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LaunchPlan {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub cwd: Option<PathBuf>,
    /// argv[0] for the game, when it differs from `program` (V28 trampoline).
    pub arg0: Option<OsString>,
    pub env_set: Vec<(OsString, OsString)>,
    pub env_remove: Vec<OsString>,
    /// Incoming `-savedatafolder` / `-logFile` args dropped in favour of ours.
    pub replaced: Vec<OsString>,
}

#[derive(Debug, thiserror::Error)]
pub enum LaunchError {
    #[error("no game command was given (expected `rwpm run <profile> -- %command%`)")]
    NoCommand,
    #[error("a required launch file is missing: `{}`", .0.display())]
    MissingFile(PathBuf),
    #[error("`{}` contains `:`, which breaks the Linux library path", .0.display())]
    PathHasColon(PathBuf),
    #[error("profile `{0}` is OFFLINE but has no Mods folder")]
    NoModsDir(String),
    #[error(
        "the game is started through another program, and rwpm cannot find itself to go between them"
    )]
    WrappedWithoutRwpm,
}

/// Builds the launch for `profile`. `env` reads the inherited environment (for V18 prepends).
pub fn build(
    profile: &Profile,
    command: &[OsString],
    payload: &PayloadFiles,
    os: Os,
    env: &dyn Fn(&str) -> Option<OsString>,
) -> Result<LaunchPlan, LaunchError> {
    let (program, rest) = command.split_first().ok_or(LaunchError::NoCommand)?;
    if os == Os::Linux
        && profile.mode == Mode::Offline
        && let Some(at) = rest.iter().position(is_start_script)
    {
        return build_wrapped(profile, command, at + 1, payload, env);
    }
    let (game_args, replaced) = strip_profile_args(rest);
    let mut plan = LaunchPlan {
        program: PathBuf::from(program),
        replaced,
        ..LaunchPlan::default()
    };
    // V24: Steam app id via env, independent of steam_appid.txt (R17, R20, R21).
    set(&mut plan, "SteamAppId", STEAM_APP_ID);
    if profile.mode == Mode::Offline {
        add_offline(&mut plan, profile, payload, os, env)?;
    }
    plan.args.extend(game_args);
    // V4 / V10: one token, only for stored profiles.
    plan.args.extend(profile.savedatafolder_arg());
    // V15
    if let Some(log) = profile.log_file() {
        plan.args.push("-logFile".into());
        plan.args.push(log.into_os_string());
    }
    Ok(plan)
}

fn is_start_script(arg: &OsString) -> bool {
    Path::new(arg).file_name() == Some(OsStr::new(LINUX_START_SCRIPT))
}

/// The game's own program in `%command%`: the start script behind any wrappers, else the first.
pub fn game_program(command: &[OsString]) -> Option<&OsString> {
    command
        .iter()
        .find(|a| is_start_script(a))
        .or_else(|| command.first())
}

/// V29 / B3: wrappers before the start script (Steam Linux Runtime, gamemoderun) are
/// `/bin/sh` scripts that Doorstop would break (R22). They get no Doorstop env; rwpm takes
/// the script's place and, inside them, adds Doorstop and execs the game (restart mode).
fn build_wrapped(
    profile: &Profile,
    command: &[OsString],
    at: usize,
    payload: &PayloadFiles,
    env: &dyn Fn(&str) -> Option<OsString>,
) -> Result<LaunchPlan, LaunchError> {
    let rwpm = payload
        .trampoline
        .as_ref()
        .ok_or(LaunchError::WrappedWithoutRwpm)?;
    // The inner launch, built now so missing files fail here, before any wrapper runs.
    let inner = build(profile, &command[at..], payload, Os::Linux, env)?;
    let mut plan = LaunchPlan {
        program: PathBuf::from(&command[0]),
        args: command[1..at].to_vec(),
        env_remove: inner.env_remove,
        replaced: inner.replaced,
        ..LaunchPlan::default()
    };
    plan.args.push(rwpm.clone().into_os_string());
    plan.args.push(ARG_INNER.into());
    plan.args.extend(inner.args);
    set(&mut plan, "SteamAppId", STEAM_APP_ID);
    set(&mut plan, ENV_RESTART, profile.id.as_str());
    set(&mut plan, ENV_GAME_EXE, command[at].clone());
    Ok(plan)
}

/// Drops incoming `savedatafolder=` / `-savedatafolder=` (any case; the game takes the
/// first match, GenCommandLine.cs:22-39) and `-logFile <value>` pairs.
fn strip_profile_args(args: &[OsString]) -> (Vec<OsString>, Vec<OsString>) {
    let (mut kept, mut replaced) = (Vec::new(), Vec::new());
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let lower = arg.to_string_lossy().to_lowercase();
        let key = lower.split('=').next().unwrap_or_default();
        if lower.contains('=') && (key == "savedatafolder" || key == "-savedatafolder") {
            replaced.push(arg.clone());
        } else if lower == "-logfile" {
            replaced.push(arg.clone());
            replaced.extend(iter.next().cloned());
        } else {
            kept.push(arg.clone());
        }
    }
    (kept, replaced)
}

fn add_offline(
    plan: &mut LaunchPlan,
    profile: &Profile,
    payload: &PayloadFiles,
    os: Os,
    env: &dyn Fn(&str) -> Option<OsString>,
) -> Result<(), LaunchError> {
    let mods = profile
        .mods_dir()
        .ok_or_else(|| LaunchError::NoModsDir(profile.id.to_string()))?;
    require(&payload.dll)?;
    // V3: a stale guard from a parent Doorstop process would make ours skip injection.
    plan.env_remove = DOORSTOP_GUARDS.iter().map(OsString::from).collect();
    set(plan, ENV_PAYLOAD_LOG, mods.with_file_name(PAYLOAD_LOG));
    set(plan, ENV_MODS_DIR, mods);
    if let Some(target) = &payload.chain_target {
        set(plan, ENV_CHAIN_TARGET, target.clone());
    }
    match os {
        Os::Linux => {
            add_linux_doorstop(plan, payload, env)?;
            if let Some(rwpm) = &payload.trampoline {
                // V28: GenCommandLine.Restart re-runs argv[0]; rwpm then re-adds Doorstop.
                plan.arg0 = Some(rwpm.clone().into_os_string());
                set(plan, ENV_RESTART, profile.id.as_str());
                let exe = plan.program.clone();
                set(plan, ENV_GAME_EXE, exe);
            }
            Ok(())
        }
        Os::Windows => {
            // C15: winhttp.dll stays inert without these.
            plan.args.extend(
                ["--doorstop-enabled", "true", "--doorstop-target-assembly"].map(OsString::from),
            );
            plan.args.push(payload.dll.clone().into_os_string());
            Ok(())
        }
    }
}

fn add_linux_doorstop(
    plan: &mut LaunchPlan,
    payload: &PayloadFiles,
    env: &dyn Fn(&str) -> Option<OsString>,
) -> Result<(), LaunchError> {
    require(&payload.doorstop_dir.join(LINUX_DOORSTOP_LIB))?;
    if payload.doorstop_dir.to_string_lossy().contains(':') {
        return Err(LaunchError::PathHasColon(payload.doorstop_dir.clone()));
    }
    set(plan, "DOORSTOP_ENABLED", "1");
    set(plan, "DOORSTOP_TARGET_ASSEMBLY", payload.dll.clone());
    // V18: prepend, never replace. Preload by name: LD_PRELOAD splits on spaces too (R20).
    let lib_path = prepend(payload.doorstop_dir.as_os_str(), env("LD_LIBRARY_PATH"));
    set(plan, "LD_LIBRARY_PATH", lib_path);
    set(
        plan,
        "LD_PRELOAD",
        prepend(OsStr::new(LINUX_DOORSTOP_LIB), env("LD_PRELOAD")),
    );
    // V25 / R19: never preload into the bash start script; run the ELF as the script would.
    if plan.program.file_name() == Some(OsStr::new(LINUX_START_SCRIPT)) {
        let dir = plan
            .program
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        plan.program = dir.join(LINUX_ELF);
        plan.cwd = Some(dir);
        set(plan, "LC_ALL", "C");
    }
    Ok(())
}

fn require(path: &Path) -> Result<(), LaunchError> {
    if path.is_file() {
        Ok(())
    } else {
        Err(LaunchError::MissingFile(path.to_path_buf()))
    }
}

fn prepend(head: &OsStr, existing: Option<OsString>) -> OsString {
    let existing = existing.filter(|v| !v.is_empty());
    // A restart child inherits our earlier prepend; never list the head twice.
    if let Some(rest) = &existing
        && rest
            .to_string_lossy()
            .split(':')
            .any(|e| e == head.to_string_lossy())
    {
        return rest.clone();
    }
    let mut value = head.to_os_string();
    if let Some(rest) = existing {
        value.push(":");
        value.push(rest);
    }
    value
}

fn set(plan: &mut LaunchPlan, key: &str, value: impl Into<OsString>) {
    plan.env_set.push((key.into(), value.into()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::{Location, ProfileId};

    const GAME: &str = "/steam/steamapps/common/RimWorld";

    fn stored(mode: Mode) -> Profile {
        Profile {
            id: ProfileId::parse("run").expect("id"),
            name: "Run".into(),
            mode,
            location: Location::Stored {
                dir: PathBuf::from("/profiles/run"),
            },
        }
    }

    fn default_profile() -> Profile {
        Profile {
            id: ProfileId::default_id(),
            name: "Default".into(),
            mode: Mode::Live,
            location: Location::Default {
                save_dir: PathBuf::from("/home/u/.config/unity3d/x"),
            },
        }
    }

    /// Payload files that exist on disk, so OFFLINE builds pass the fail-closed check.
    fn payload(tmp: &Path) -> PayloadFiles {
        let p = PayloadFiles::beside(tmp);
        std::fs::create_dir_all(p.dll.parent().expect("parent")).expect("payload dir");
        std::fs::write(&p.dll, b"").expect("dll");
        std::fs::create_dir_all(&p.doorstop_dir).expect("doorstop dir");
        std::fs::write(p.doorstop_dir.join(LINUX_DOORSTOP_LIB), b"").expect("so");
        p
    }

    fn cmd(parts: &[&str]) -> Vec<OsString> {
        parts.iter().map(OsString::from).collect()
    }

    fn linux_script() -> Vec<OsString> {
        cmd(&[&format!("{GAME}/start_RimWorld.sh"), "-popupwindow"])
    }

    fn no_env(_: &str) -> Option<OsString> {
        None
    }

    fn env_get<'a>(plan: &'a LaunchPlan, key: &str) -> Option<&'a OsString> {
        plan.env_set.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    fn has_arg(plan: &LaunchPlan, arg: &str) -> bool {
        plan.args.iter().any(|a| a == arg)
    }

    #[test]
    fn offline_removes_doorstop_guards() {
        let tmp = tempfile::tempdir().expect("tempdir");
        for os in [Os::Linux, Os::Windows] {
            let plan = build(
                &stored(Mode::Offline),
                &linux_script(),
                &payload(tmp.path()),
                os,
                &no_env,
            )
            .expect("build");
            for g in DOORSTOP_GUARDS {
                assert!(plan.env_remove.iter().any(|k| k == g), "{os:?} keeps {g}");
            }
        }
    }

    #[test]
    fn live_has_no_doorstop_env_or_args() {
        let tmp = tempfile::tempdir().expect("tempdir");
        for os in [Os::Linux, Os::Windows] {
            for profile in [stored(Mode::Live), default_profile()] {
                let plan = build(&profile, &linux_script(), &payload(tmp.path()), os, &no_env)
                    .expect("build");
                assert!(
                    !plan
                        .env_set
                        .iter()
                        .any(|(k, _)| k.to_string_lossy().starts_with("DOORSTOP_")),
                    "{os:?}"
                );
                assert!(env_get(&plan, "LD_PRELOAD").is_none(), "{os:?}");
                assert!(env_get(&plan, ENV_MODS_DIR).is_none(), "{os:?}");
                assert!(
                    !plan
                        .args
                        .iter()
                        .any(|a| a.to_string_lossy().starts_with("--doorstop")),
                    "{os:?}"
                );
                assert!(
                    plan.env_remove.is_empty(),
                    "{os:?} LIVE must not touch Doorstop guards"
                );
            }
        }
    }

    #[test]
    fn stored_profile_gets_single_savedatafolder_token() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let plan = build(
            &stored(Mode::Live),
            &linux_script(),
            &payload(tmp.path()),
            Os::Linux,
            &no_env,
        )
        .expect("build");
        let matching: Vec<_> = plan
            .args
            .iter()
            .filter(|a| a.to_string_lossy().starts_with("-savedatafolder="))
            .collect();
        assert_eq!(
            matching,
            [&OsString::from("-savedatafolder=/profiles/run/SaveData")]
        );
    }

    #[test]
    fn stored_profile_gets_logfile() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let plan = build(
            &stored(Mode::Live),
            &linux_script(),
            &payload(tmp.path()),
            Os::Linux,
            &no_env,
        )
        .expect("build");
        let i = plan
            .args
            .iter()
            .position(|a| a == "-logFile")
            .expect("-logFile");
        assert_eq!(plan.args[i + 1], OsString::from("/profiles/run/Player.log"));
    }

    #[test]
    fn default_profile_has_no_savedatafolder_or_logfile() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let plan = build(
            &default_profile(),
            &linux_script(),
            &payload(tmp.path()),
            Os::Linux,
            &no_env,
        )
        .expect("build");
        assert!(!plan.args.iter().any(|a| {
            a.to_string_lossy()
                .to_lowercase()
                .contains("savedatafolder")
        }));
        assert!(!has_arg(&plan, "-logFile"));
        assert!(has_arg(&plan, "-popupwindow"), "user args kept");
    }

    #[test]
    fn ld_preload_prepends_existing() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let p = payload(tmp.path());
        let env = |k: &str| match k {
            "LD_PRELOAD" => Some(OsString::from("/steam/gameoverlayrenderer.so")),
            "LD_LIBRARY_PATH" => Some(OsString::from("/steam/runtime/lib")),
            _ => None,
        };
        let plan =
            build(&stored(Mode::Offline), &linux_script(), &p, Os::Linux, &env).expect("build");
        assert_eq!(
            env_get(&plan, "LD_PRELOAD"),
            Some(&OsString::from(
                "libdoorstop.so:/steam/gameoverlayrenderer.so"
            ))
        );
        let expected = format!("{}:/steam/runtime/lib", p.doorstop_dir.display());
        assert_eq!(
            env_get(&plan, "LD_LIBRARY_PATH"),
            Some(&OsString::from(expected))
        );
    }

    #[test]
    fn ld_preload_set_when_absent() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let p = payload(tmp.path());
        let plan = build(
            &stored(Mode::Offline),
            &linux_script(),
            &p,
            Os::Linux,
            &no_env,
        )
        .expect("build");
        assert_eq!(
            env_get(&plan, "LD_PRELOAD"),
            Some(&OsString::from("libdoorstop.so"))
        );
        assert_eq!(
            env_get(&plan, "LD_LIBRARY_PATH"),
            Some(&p.doorstop_dir.clone().into_os_string())
        );
    }

    #[test]
    fn doorstop_dir_with_colon_rejected() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let p = payload(&tmp.path().join("a:b"));
        let r = build(
            &stored(Mode::Offline),
            &linux_script(),
            &p,
            Os::Linux,
            &no_env,
        );
        assert!(matches!(r, Err(LaunchError::PathHasColon(_))), "{r:?}");
    }

    #[test]
    fn steam_app_id_set_in_every_mode() {
        let tmp = tempfile::tempdir().expect("tempdir");
        for os in [Os::Linux, Os::Windows] {
            for profile in [stored(Mode::Live), stored(Mode::Offline), default_profile()] {
                let plan = build(&profile, &linux_script(), &payload(tmp.path()), os, &no_env)
                    .expect("build");
                assert_eq!(
                    env_get(&plan, "SteamAppId"),
                    Some(&OsString::from(STEAM_APP_ID)),
                    "{os:?} {:?}",
                    profile.mode
                );
            }
        }
    }

    #[test]
    fn offline_linux_start_script_runs_elf_directly() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let plan = build(
            &stored(Mode::Offline),
            &linux_script(),
            &payload(tmp.path()),
            Os::Linux,
            &no_env,
        )
        .expect("build");
        assert_eq!(plan.program, PathBuf::from(GAME).join(LINUX_ELF));
        assert_eq!(plan.cwd.as_deref(), Some(Path::new(GAME)));
        assert_eq!(env_get(&plan, "LC_ALL"), Some(&OsString::from("C")));
        assert_eq!(
            plan.args.first(),
            Some(&OsString::from("-popupwindow")),
            "script args kept, script dropped"
        );
    }

    #[test]
    fn live_linux_keeps_start_script() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let plan = build(
            &stored(Mode::Live),
            &linux_script(),
            &payload(tmp.path()),
            Os::Linux,
            &no_env,
        )
        .expect("build");
        assert_eq!(plan.program, PathBuf::from(GAME).join(LINUX_START_SCRIPT));
        assert_eq!(plan.cwd, None);
    }

    #[test]
    fn offline_sets_payload_channel_and_log() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let p = payload(tmp.path());
        for os in [Os::Linux, Os::Windows] {
            let plan =
                build(&stored(Mode::Offline), &linux_script(), &p, os, &no_env).expect("build");
            assert_eq!(
                env_get(&plan, ENV_MODS_DIR),
                Some(&OsString::from("/profiles/run/Mods")),
                "{os:?}"
            );
            assert_eq!(
                env_get(&plan, ENV_PAYLOAD_LOG),
                Some(&OsString::from("/profiles/run/payload.log")),
                "{os:?}"
            );
        }
        let linux = build(
            &stored(Mode::Offline),
            &linux_script(),
            &p,
            Os::Linux,
            &no_env,
        )
        .expect("build");
        assert_eq!(
            env_get(&linux, "DOORSTOP_ENABLED"),
            Some(&OsString::from("1"))
        );
        assert_eq!(
            env_get(&linux, "DOORSTOP_TARGET_ASSEMBLY"),
            Some(&p.dll.clone().into_os_string())
        );
    }

    #[test]
    fn linux_offline_sets_trampoline_arg0_and_restart_env() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let mut p = payload(tmp.path());
        p.trampoline = Some(PathBuf::from("/app/rwpm"));
        let plan = build(
            &stored(Mode::Offline),
            &linux_script(),
            &p,
            Os::Linux,
            &no_env,
        )
        .expect("build");
        assert_eq!(plan.arg0, Some(OsString::from("/app/rwpm")));
        assert_eq!(env_get(&plan, ENV_RESTART), Some(&OsString::from("run")));
        assert_eq!(
            env_get(&plan, ENV_GAME_EXE),
            Some(&OsString::from(format!("{GAME}/RimWorldLinux")))
        );
    }

    #[test]
    fn live_windows_and_no_trampoline_have_no_arg0() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let mut p = payload(tmp.path());
        let none = build(
            &stored(Mode::Offline),
            &linux_script(),
            &p,
            Os::Linux,
            &no_env,
        )
        .expect("build");
        assert_eq!(none.arg0, None);
        assert!(env_get(&none, ENV_RESTART).is_none());
        p.trampoline = Some(PathBuf::from("/app/rwpm"));
        for (mode, os) in [
            (Mode::Live, Os::Linux),
            (Mode::Offline, Os::Windows),
            (Mode::Live, Os::Windows),
        ] {
            let plan = build(&stored(mode), &linux_script(), &p, os, &no_env).expect("build");
            assert_eq!(plan.arg0, None, "{mode:?} {os:?}");
            assert!(env_get(&plan, ENV_RESTART).is_none(), "{mode:?} {os:?}");
        }
    }

    #[test]
    fn chain_target_passed_to_payload() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let mut p = payload(tmp.path());
        p.chain_target = Some(PathBuf::from(r"C:\Games\RimWorld\Doorstop.dll"));
        let plan = build(
            &stored(Mode::Offline),
            &linux_script(),
            &p,
            Os::Windows,
            &no_env,
        )
        .expect("build");
        assert_eq!(
            env_get(&plan, ENV_CHAIN_TARGET),
            Some(&OsString::from(r"C:\Games\RimWorld\Doorstop.dll"))
        );
        let live = build(
            &stored(Mode::Live),
            &linux_script(),
            &p,
            Os::Windows,
            &no_env,
        )
        .expect("build");
        assert!(env_get(&live, ENV_CHAIN_TARGET).is_none());
    }

    /// V23: a foreign Doorstop's target is `Doorstop.dll`; ours must not share its name.
    #[test]
    fn payload_assembly_is_not_named_doorstop() {
        let p = PayloadFiles::beside(Path::new("/app"));
        let name = p
            .dll
            .file_stem()
            .expect("stem")
            .to_string_lossy()
            .to_lowercase();
        assert_ne!(name, "doorstop");
    }

    #[test]
    fn prepend_skips_existing_head() {
        let head = OsStr::new("libdoorstop.so");
        let inherited = Some(OsString::from("libdoorstop.so:/overlay.so"));
        assert_eq!(
            prepend(head, inherited),
            OsString::from("libdoorstop.so:/overlay.so")
        );
        let other = Some(OsString::from("/overlay.so"));
        assert_eq!(
            prepend(head, other),
            OsString::from("libdoorstop.so:/overlay.so")
        );
        assert_eq!(
            prepend(head, Some(OsString::new())),
            OsString::from("libdoorstop.so")
        );
    }

    #[test]
    fn offline_missing_payload_files_fails_closed() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let p = PayloadFiles::beside(tmp.path());
        for os in [Os::Linux, Os::Windows] {
            let r = build(&stored(Mode::Offline), &linux_script(), &p, os, &no_env);
            assert!(
                matches!(r, Err(LaunchError::MissingFile(_))),
                "{os:?} {r:?}"
            );
        }
        assert!(
            build(&stored(Mode::Live), &linux_script(), &p, Os::Linux, &no_env).is_ok(),
            "LIVE needs no payload"
        );
    }

    #[test]
    fn offline_windows_passes_doorstop_cli_args() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let p = payload(tmp.path());
        let exe = cmd(&[
            r"C:\Steam\steamapps\common\RimWorld\RimWorldWin64.exe",
            "-popupwindow",
        ]);
        let plan = build(&stored(Mode::Offline), &exe, &p, Os::Windows, &no_env).expect("build");
        assert_eq!(plan.program, PathBuf::from(&exe[0]));
        assert_eq!(
            &plan.args[..4],
            &[
                OsString::from("--doorstop-enabled"),
                OsString::from("true"),
                OsString::from("--doorstop-target-assembly"),
                p.dll.clone().into_os_string(),
            ]
        );
        assert!(env_get(&plan, "LD_PRELOAD").is_none());
        assert!(has_arg(&plan, "-popupwindow"));
    }

    #[test]
    fn incoming_savedatafolder_and_logfile_args_are_replaced() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let incoming = cmd(&[
            &format!("{GAME}/start_RimWorld.sh"),
            "-SaveDataFolder=/stale/one",
            "savedatafolder=/stale/two",
            "-LOGFILE",
            "/stale/log",
            "-popupwindow",
        ]);
        for profile in [stored(Mode::Offline), default_profile()] {
            let plan = build(
                &profile,
                &incoming,
                &payload(tmp.path()),
                Os::Linux,
                &no_env,
            )
            .expect("build");
            let text: Vec<String> = plan
                .args
                .iter()
                .map(|a| a.to_string_lossy().into_owned())
                .collect();
            assert!(!text.iter().any(|a| a.contains("/stale/")), "{text:?}");
            assert!(has_arg(&plan, "-popupwindow"));
            assert_eq!(
                plan.replaced,
                cmd(&[
                    "-SaveDataFolder=/stale/one",
                    "savedatafolder=/stale/two",
                    "-LOGFILE",
                    "/stale/log"
                ])
            );
        }
    }

    /// Flatpak Steam's Steam Linux Runtime chain, as seen in `%command%` (?U3).
    fn wrapped_script() -> Vec<OsString> {
        cmd(&[
            "/rt/_v2-entry-point",
            "--verb",
            "waitforexitandrun",
            "--",
            "/rt/scout-on-soldier-entry-point-v2",
            "--",
            &format!("{GAME}/start_RimWorld.sh"),
            "-popupwindow",
            "-logFile",
            "/stale/log",
        ])
    }

    fn wrapped_plan() -> LaunchPlan {
        let tmp = tempfile::tempdir().expect("tempdir");
        let mut p = payload(tmp.path());
        p.trampoline = Some(PathBuf::from("/app/rwpm"));
        build(
            &stored(Mode::Offline),
            &wrapped_script(),
            &p,
            Os::Linux,
            &no_env,
        )
        .expect("build")
    }

    #[test]
    fn wrapped_offline_puts_rwpm_in_place_of_start_script() {
        let plan = wrapped_plan();
        assert_eq!(plan.program, PathBuf::from("/rt/_v2-entry-point"));
        assert_eq!(plan.arg0, None);
        assert_eq!(plan.cwd, None);
        let expected = cmd(&[
            "--verb",
            "waitforexitandrun",
            "--",
            "/rt/scout-on-soldier-entry-point-v2",
            "--",
            "/app/rwpm",
            ARG_INNER,
            "-popupwindow",
            "-savedatafolder=/profiles/run/SaveData",
            "-logFile",
            "/profiles/run/Player.log",
        ]);
        assert_eq!(plan.args, expected);
        assert_eq!(plan.replaced, cmd(&["-logFile", "/stale/log"]));
    }

    /// B3: the wrappers are shell scripts; Doorstop in their env breaks `$(…)` (R22).
    #[test]
    fn wrapped_offline_wrappers_get_no_doorstop_env() {
        let plan = wrapped_plan();
        for (k, _) in &plan.env_set {
            let k = k.to_string_lossy();
            assert!(
                !k.starts_with("DOORSTOP_") && k != "LD_PRELOAD" && k != ENV_MODS_DIR,
                "wrapper env has {k}"
            );
        }
        assert_eq!(env_get(&plan, ENV_RESTART), Some(&OsString::from("run")));
        assert_eq!(
            env_get(&plan, ENV_GAME_EXE),
            Some(&OsString::from(format!("{GAME}/start_RimWorld.sh")))
        );
        assert_eq!(
            env_get(&plan, "SteamAppId"),
            Some(&OsString::from(STEAM_APP_ID))
        );
        for g in DOORSTOP_GUARDS {
            assert!(plan.env_remove.iter().any(|k| k == g), "keeps {g}");
        }
    }

    #[test]
    fn wrapped_offline_fails_closed_and_needs_rwpm() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let mut missing = PayloadFiles::beside(tmp.path());
        missing.trampoline = Some(PathBuf::from("/app/rwpm"));
        let r = build(
            &stored(Mode::Offline),
            &wrapped_script(),
            &missing,
            Os::Linux,
            &no_env,
        );
        assert!(matches!(r, Err(LaunchError::MissingFile(_))), "{r:?}");
        let r = build(
            &stored(Mode::Offline),
            &wrapped_script(),
            &payload(tmp.path()),
            Os::Linux,
            &no_env,
        );
        assert!(matches!(r, Err(LaunchError::WrappedWithoutRwpm)), "{r:?}");
    }

    #[test]
    fn game_program_skips_wrappers() {
        let script = OsString::from(format!("{GAME}/start_RimWorld.sh"));
        assert_eq!(game_program(&wrapped_script()), Some(&script));
        let exe = cmd(&[r"C:\RimWorld\RimWorldWin64.exe"]);
        assert_eq!(game_program(&exe), exe.first());
        assert_eq!(game_program(&[]), None);
    }

    #[test]
    fn wrapped_live_runs_chain_unchanged() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let mut p = payload(tmp.path());
        p.trampoline = Some(PathBuf::from("/app/rwpm"));
        let plan = build(
            &stored(Mode::Live),
            &wrapped_script(),
            &p,
            Os::Linux,
            &no_env,
        )
        .expect("build");
        assert_eq!(plan.program, PathBuf::from("/rt/_v2-entry-point"));
        assert!(has_arg(&plan, &format!("{GAME}/start_RimWorld.sh")));
        assert!(!has_arg(&plan, ARG_INNER));
    }

    #[test]
    fn empty_command_rejected() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let r = build(
            &stored(Mode::Live),
            &[],
            &payload(tmp.path()),
            Os::Linux,
            &no_env,
        );
        assert!(matches!(r, Err(LaunchError::NoCommand)));
    }
}
