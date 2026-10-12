//! Launch builder: profile + `%command%` + OS → exact program, args, cwd and env.
//! Pure: spawning lives in `rwpm run` (T10). Recipes verified live in T2/T3 (R20, R21).

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use crate::discovery::{Engine, engine_of};
use crate::profile::{Mode, Profile};

pub const STEAM_APP_ID: &str = "294100";
pub const ENV_MODS_DIR: &str = "RWPM_MODS_DIR";
pub const ENV_PAYLOAD_LOG: &str = "RWPM_PAYLOAD_LOG";
/// V28: set on Linux OFFLINE games; the in-game Restart re-runs rwpm, which reads these.
pub const ENV_RESTART: &str = "RWPM_RESTART";
pub const ENV_GAME_EXE: &str = "RWPM_GAME_EXE";
/// A foreign Doorstop's own target assembly, started by our payload after the redirect.
/// .NET's startup-hook list; the Waylith payload goes first (V64).
pub const ENV_STARTUP_HOOKS: &str = "DOTNET_STARTUP_HOOKS";
pub const ENV_CHAIN_TARGET: &str = "RWPM_CHAIN_TARGET";
/// V29: first arg of the inner rwpm that takes the start script's place behind wrappers.
pub const ARG_INNER: &str = "--rwpm-inner";
const LINUX_START_SCRIPT: &str = "start_RimWorld.sh";
const LINUX_ELF: &str = "RimWorldLinux";
const LINUX_DOORSTOP_LIB: &str = "libdoorstop.so";
const MACOS_DOORSTOP_LIB: &str = "libdoorstop.dylib";
const MACOS_BUNDLE_EXT: &str = "app";
const PAYLOAD_LOG: &str = "payload.log";
const DOORSTOP_GUARDS: [&str; 2] = ["DOORSTOP_INITIALIZED", "DOORSTOP_DISABLE"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Linux,
    Windows,
    MacOs,
}

impl Os {
    pub fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::MacOs
        } else {
            Self::Linux
        }
    }
}

/// Payload + Doorstop files shipped beside the app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PayloadFiles {
    pub dll: PathBuf,
    /// The Waylith (1.7) startup hook, `Rwpm.Payload.Hook.dll` (V64).
    pub hook: PathBuf,
    /// Linux: dir holding `libdoorstop.so`. macOS: dir holding `libdoorstop.dylib` (R26).
    /// Windows: unused (winhttp.dll lives in the game dir, T12).
    pub doorstop_dir: PathBuf,
    /// Linux: the `rwpm` binary, given to the game as argv[0] so an in-game Restart
    /// re-enters rwpm (V28). `None`: no trampoline, Doorstop stays on for helpers.
    pub trampoline: Option<PathBuf>,
    /// Windows: the assembly a reused foreign Doorstop would have loaded (T12).
    pub chain_target: Option<PathBuf>,
}

impl PayloadFiles {
    /// `<app>/payload/Rwpm.Payload.dll`, `<app>/payload/Rwpm.Payload.Hook.dll`, `<app>/doorstop/`.
    pub fn beside(app_dir: &Path) -> Self {
        Self {
            dll: app_dir.join("payload").join("Rwpm.Payload.dll"),
            hook: app_dir.join("payload").join("Rwpm.Payload.Hook.dll"),
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
    #[error("`{}` contains `{sep}`, which breaks the .NET startup-hook list", .0.display(), sep = .1)]
    HookPathHasSeparator(PathBuf, char),
    #[error("profile `{0}` is OFFLINE but has no Mods folder")]
    NoModsDir(String),
    #[error(
        "the game is started through another program, and rwpm cannot find itself to go between them"
    )]
    WrappedWithoutRwpm,
    #[error("`{}` is a macOS app bundle rwpm could not read (missing or malformed Info.plist)", .0.display())]
    BadBundle(PathBuf),
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
    // V62: the engine decides the launch shape; Waylith never gets Doorstop.
    if waylith_program(command).is_some() {
        return build_waylith(profile, command, payload, os, env);
    }
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
    // V41/R33: Steam's %command% on macOS names the .app bundle DIRECTORY, not the inner
    // Mach-O, in every mode (live-confirmed, not Offline-only) — resolve it up front,
    // before any Doorstop/env work, so nothing downstream ever sees the bundle path.
    if os == Os::MacOs {
        plan.program = resolve_macos_bundle(&plan.program)?;
    }
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

/// The game's own program in `%command%`: the start script or the Waylith exe behind any
/// wrappers (V69), else the first.
pub fn game_program(command: &[OsString]) -> Option<&OsString> {
    command
        .iter()
        .find(|a| is_start_script(a))
        .or_else(|| waylith_program(command))
        .or_else(|| command.first())
}

/// V69: the entry of `%command%` that is a file beside `Waylith.dll` (RimWorld 1.7+).
fn waylith_program(command: &[OsString]) -> Option<&OsString> {
    command.iter().find(|a| {
        let path = Path::new(a);
        path.is_file()
            && path
                .parent()
                .is_some_and(|dir| !dir.as_os_str().is_empty() && engine_of(dir) == Engine::Waylith)
    })
}

/// RimWorld 1.7 (Waylith, R37). Steam's wrapper chain stays as given: the env reaches the
/// game through it (R39). No Doorstop, no restart trampoline, no `-logFile` (the engine
/// ignores it, V67); OFFLINE adds the startup hook (V64).
fn build_waylith(
    profile: &Profile,
    command: &[OsString],
    payload: &PayloadFiles,
    os: Os,
    env: &dyn Fn(&str) -> Option<OsString>,
) -> Result<LaunchPlan, LaunchError> {
    let (program, rest) = command.split_first().ok_or(LaunchError::NoCommand)?;
    let (game_args, replaced) = strip_profile_args(rest);
    let mut plan = LaunchPlan {
        program: PathBuf::from(program),
        replaced,
        ..LaunchPlan::default()
    };
    set(&mut plan, "SteamAppId", STEAM_APP_ID);
    if profile.mode == Mode::Offline {
        let mods = profile
            .mods_dir()
            .ok_or_else(|| LaunchError::NoModsDir(profile.id.to_string()))?;
        // V62: fail closed; the game must not start on the install's Mods folder.
        require(&payload.hook)?;
        let sep = if os == Os::Windows { ';' } else { ':' };
        if payload.hook.to_string_lossy().contains(sep) {
            return Err(LaunchError::HookPathHasSeparator(payload.hook.clone(), sep));
        }
        set(&mut plan, ENV_PAYLOAD_LOG, mods.with_file_name(PAYLOAD_LOG));
        set(&mut plan, ENV_MODS_DIR, mods);
        let hooks = prepend_with(payload.hook.as_os_str(), env(ENV_STARTUP_HOOKS), sep);
        set(&mut plan, ENV_STARTUP_HOOKS, hooks);
    }
    plan.args.extend(game_args);
    plan.args.extend(profile.savedatafolder_arg());
    Ok(plan)
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
            add_nix_doorstop(plan, payload, env, &LINUX_NIX)?;
            // V25 / R19: never preload into the bash start script; run the ELF as the
            // script would.
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
            if let Some(rwpm) = &payload.trampoline {
                // V28: GenCommandLine.Restart re-runs argv[0]; rwpm then re-adds Doorstop.
                plan.arg0 = Some(rwpm.clone().into_os_string());
                set(plan, ENV_RESTART, profile.id.as_str());
                let exe = plan.program.clone();
                set(plan, ENV_GAME_EXE, exe);
            }
            Ok(())
        }
        Os::MacOs => {
            // V41/R33: `plan.program` is already the resolved inner Mach-O (build()
            // resolved the bundle up front) — no shell/start-script substitution exists
            // or is needed here, unlike Linux.
            //
            // T42/V48: no restart trampoline, deliberately — never set `arg0`/
            // `ENV_RESTART` here even if `payload.trampoline` is populated. The payload's
            // `DisableDoorstopForHelpers()` only disables Doorstop for helper processes
            // when `RWPM_RESTART` is set, specifically to work around Linux's B1/B2/B3
            // shell-hook breakage (dup2/fclose). That breakage does not reproduce on
            // macOS (R28, sc_shellhook) — Doorstop simply stays enabled across an
            // in-game Restart with no intervention needed, confirmed live (R31,
            // sc_restart: redirect held on the restarted process with no trampoline at
            // all). Adding one here would cost real complexity for zero benefit.
            add_nix_doorstop(plan, payload, env, &MACOS_NIX)?;
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

/// A "nix" (Doorstop's own term, src/nix/) Unix-like loader: Linux's `LD_PRELOAD` /
/// `LD_LIBRARY_PATH` and macOS's `DYLD_INSERT_LIBRARIES` / `DYLD_LIBRARY_PATH` share the
/// same `DOORSTOP_ENABLED`/`DOORSTOP_TARGET_ASSEMBLY` contract (R26) and only differ in
/// variable names and in whether the inserted entry is a bare leaf name or an absolute path.
struct NixDoorstop {
    loader_env: &'static str,
    path_env: &'static str,
    lib_name: &'static str,
    /// macOS (V40): insert the absolute dylib path rather than relying on leaf-name
    /// resolution through `DYLD_LIBRARY_PATH` — sidesteps a dyld4 behaviour the spike
    /// didn't need to pin down (plan decision). Linux keeps preload-by-name (R20).
    absolute_loader_entry: bool,
}

const LINUX_NIX: NixDoorstop = NixDoorstop {
    loader_env: "LD_PRELOAD",
    path_env: "LD_LIBRARY_PATH",
    lib_name: LINUX_DOORSTOP_LIB,
    absolute_loader_entry: false,
};

const MACOS_NIX: NixDoorstop = NixDoorstop {
    loader_env: "DYLD_INSERT_LIBRARIES",
    path_env: "DYLD_LIBRARY_PATH",
    lib_name: MACOS_DOORSTOP_LIB,
    absolute_loader_entry: true,
};

fn add_nix_doorstop(
    plan: &mut LaunchPlan,
    payload: &PayloadFiles,
    env: &dyn Fn(&str) -> Option<OsString>,
    vars: &NixDoorstop,
) -> Result<(), LaunchError> {
    require(&payload.doorstop_dir.join(vars.lib_name))?;
    if payload.doorstop_dir.to_string_lossy().contains(':') {
        return Err(LaunchError::PathHasColon(payload.doorstop_dir.clone()));
    }
    set(plan, "DOORSTOP_ENABLED", "1");
    set(plan, "DOORSTOP_TARGET_ASSEMBLY", payload.dll.clone());
    // V18/V40: prepend, never replace.
    let lib_path = prepend(payload.doorstop_dir.as_os_str(), env(vars.path_env));
    set(plan, vars.path_env, lib_path);
    let entry: OsString = if vars.absolute_loader_entry {
        payload.doorstop_dir.join(vars.lib_name).into_os_string()
    } else {
        OsString::from(vars.lib_name)
    };
    set(
        plan,
        vars.loader_env,
        prepend(entry.as_os_str(), env(vars.loader_env)),
    );
    Ok(())
}

/// V41/R33: resolves a `%command%` entry naming a macOS `.app` bundle DIRECTORY to the
/// actual Mach-O binary inside it, by reading `CFBundleExecutable` out of its
/// `Contents/Info.plist` — never by spawning a shell (R27: SIP purges `DYLD_*` across a
/// shell exec, R30: app translocation makes a bundle-relative path unsafe to trust
/// anyway). Anything not ending in `.app` passes through unchanged, so this is safe to
/// call on every macOS launch, live or stored.
fn resolve_macos_bundle(path: &Path) -> Result<PathBuf, LaunchError> {
    if path.extension().and_then(|e| e.to_str()) != Some(MACOS_BUNDLE_EXT) {
        return Ok(path.to_path_buf());
    }
    let plist_path = path.join("Contents").join("Info.plist");
    let plist = std::fs::read_to_string(&plist_path)
        .map_err(|_| LaunchError::MissingFile(plist_path.clone()))?;
    let exe_name = extract_plist_string(&plist, "CFBundleExecutable")
        .ok_or_else(|| LaunchError::BadBundle(plist_path.clone()))?;
    Ok(path.join("Contents").join("MacOS").join(exe_name))
}

/// Minimal scan for one string-valued key in Apple's XML plist format — Info.plist is
/// always the simple `<key>k</key>\n<string>v</string>` shape (R33 live-confirmed on this
/// Mac), so a tiny string search avoids pulling in a full plist-parsing dependency for
/// the one field rwpm needs.
fn extract_plist_string(xml: &str, key: &str) -> Option<String> {
    let marker = format!("<key>{key}</key>");
    let after_key = xml.split_once(&marker)?.1;
    let after_open = after_key.split_once("<string>")?.1;
    let value = after_open.split_once("</string>")?.0;
    Some(value.trim().to_string())
}

fn require(path: &Path) -> Result<(), LaunchError> {
    if path.is_file() {
        Ok(())
    } else {
        Err(LaunchError::MissingFile(path.to_path_buf()))
    }
}

fn prepend(head: &OsStr, existing: Option<OsString>) -> OsString {
    prepend_with(head, existing, ':')
}

/// `head` + `sep` + `existing`, unless `existing` already lists `head`.
fn prepend_with(head: &OsStr, existing: Option<OsString>, sep: char) -> OsString {
    let existing = existing.filter(|v| !v.is_empty());
    // A restart child inherits our earlier prepend; never list the head twice.
    if let Some(rest) = &existing
        && rest
            .to_string_lossy()
            .split(sep)
            .any(|e| e == head.to_string_lossy())
    {
        return rest.clone();
    }
    let mut value = head.to_os_string();
    if let Some(rest) = existing {
        value.push(sep.to_string());
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

    /// Payload files that exist on disk, so OFFLINE builds pass the fail-closed check —
    /// for every OS, so the same fixture works in `[Os::Linux, Os::Windows, Os::MacOs]`
    /// loops without each test having to know which loader file it needs.
    fn payload(tmp: &Path) -> PayloadFiles {
        let p = PayloadFiles::beside(tmp);
        std::fs::create_dir_all(p.dll.parent().expect("parent")).expect("payload dir");
        std::fs::write(&p.dll, b"").expect("dll");
        std::fs::write(&p.hook, b"").expect("hook");
        std::fs::create_dir_all(&p.doorstop_dir).expect("doorstop dir");
        std::fs::write(p.doorstop_dir.join(LINUX_DOORSTOP_LIB), b"").expect("so");
        std::fs::write(p.doorstop_dir.join(MACOS_DOORSTOP_LIB), b"").expect("dylib");
        p
    }

    fn cmd(parts: &[&str]) -> Vec<OsString> {
        parts.iter().map(OsString::from).collect()
    }

    fn linux_script() -> Vec<OsString> {
        cmd(&[&format!("{GAME}/start_RimWorld.sh"), "-popupwindow"])
    }

    /// A macOS program path that is NOT a `.app` bundle (already-resolved inner binary).
    fn macos_script() -> Vec<OsString> {
        cmd(&[&format!("{GAME}/RimWorldMac"), "-popupwindow"])
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
        for os in [Os::Linux, Os::Windows, Os::MacOs] {
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
        for os in [Os::Linux, Os::Windows, Os::MacOs] {
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
        for os in [Os::Linux, Os::Windows, Os::MacOs] {
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
        for os in [Os::Linux, Os::Windows, Os::MacOs] {
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
        for os in [Os::Linux, Os::Windows, Os::MacOs] {
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

    /// V40/R31: DYLD_INSERT_LIBRARIES carries the absolute dylib path (plan decision,
    /// unlike Linux's leaf-name LD_PRELOAD), DYLD_LIBRARY_PATH still prepends the dir.
    #[test]
    fn macos_dyld_set_when_absent() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let p = payload(tmp.path());
        let plan = build(
            &stored(Mode::Offline),
            &linux_script(),
            &p,
            Os::MacOs,
            &no_env,
        )
        .expect("build");
        let expected_lib = p.doorstop_dir.join(MACOS_DOORSTOP_LIB).into_os_string();
        assert_eq!(env_get(&plan, "DYLD_INSERT_LIBRARIES"), Some(&expected_lib));
        assert_eq!(
            env_get(&plan, "DYLD_LIBRARY_PATH"),
            Some(&p.doorstop_dir.clone().into_os_string())
        );
        assert!(env_get(&plan, "LD_PRELOAD").is_none());
    }

    #[test]
    fn macos_dyld_prepends_existing() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let p = payload(tmp.path());
        let env = |k: &str| match k {
            "DYLD_INSERT_LIBRARIES" => Some(OsString::from("/steam/overlay.dylib")),
            "DYLD_LIBRARY_PATH" => Some(OsString::from("/steam/runtime/lib")),
            _ => None,
        };
        let plan =
            build(&stored(Mode::Offline), &linux_script(), &p, Os::MacOs, &env).expect("build");
        let expected_lib = format!(
            "{}:/steam/overlay.dylib",
            p.doorstop_dir.join(MACOS_DOORSTOP_LIB).display()
        );
        assert_eq!(
            env_get(&plan, "DYLD_INSERT_LIBRARIES"),
            Some(&OsString::from(expected_lib))
        );
        let expected_path = format!("{}:/steam/runtime/lib", p.doorstop_dir.display());
        assert_eq!(
            env_get(&plan, "DYLD_LIBRARY_PATH"),
            Some(&OsString::from(expected_path))
        );
    }

    fn write_info_plist(bundle: &Path, exe_name: &str) {
        std::fs::create_dir_all(bundle.join("Contents").join("MacOS")).expect("MacOS dir");
        let xml = format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<plist version=\"1.0\"><dict>\n  <key>CFBundleExecutable</key>\n  <string>{exe_name}</string>\n</dict></plist>\n"
        );
        std::fs::write(bundle.join("Contents").join("Info.plist"), xml).expect("Info.plist");
    }

    /// V41/R33: a `%command%` naming the `.app` bundle directory resolves to the Mach-O
    /// named by `CFBundleExecutable`, whose name may itself contain spaces (R29).
    #[test]
    fn macos_bundle_resolves_to_inner_binary() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let bundle = tmp.path().join("RimWorldMac.app");
        write_info_plist(&bundle, "RimWorld by Ludeon Studios");
        let command = cmd(&[bundle.to_str().expect("utf8"), "-disable-compute-shaders"]);
        let plan = build(
            &default_profile(),
            &command,
            &payload(tmp.path()),
            Os::MacOs,
            &no_env,
        )
        .expect("build");
        assert_eq!(
            plan.program,
            bundle
                .join("Contents")
                .join("MacOS")
                .join("RimWorld by Ludeon Studios")
        );
        assert!(has_arg(&plan, "-disable-compute-shaders"));
    }

    /// A path not ending in `.app` is left untouched — the common case on Linux/Windows,
    /// and for an already-resolved inner binary on macOS.
    #[test]
    fn non_bundle_path_passes_through_resolve() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let plan = build(
            &default_profile(),
            &macos_script(),
            &payload(tmp.path()),
            Os::MacOs,
            &no_env,
        )
        .expect("build");
        assert_eq!(plan.program, PathBuf::from(format!("{GAME}/RimWorldMac")));
    }

    #[test]
    fn macos_bundle_missing_info_plist_fails_closed() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let bundle = tmp.path().join("RimWorldMac.app");
        std::fs::create_dir_all(&bundle).expect("bundle dir");
        let command = cmd(&[bundle.to_str().expect("utf8")]);
        let r = build(
            &default_profile(),
            &command,
            &payload(tmp.path()),
            Os::MacOs,
            &no_env,
        );
        assert!(matches!(r, Err(LaunchError::MissingFile(_))), "{r:?}");
    }

    #[test]
    fn macos_bundle_malformed_info_plist_fails_closed() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let bundle = tmp.path().join("RimWorldMac.app");
        std::fs::create_dir_all(bundle.join("Contents")).expect("Contents dir");
        std::fs::write(bundle.join("Contents").join("Info.plist"), "not a plist")
            .expect("Info.plist");
        let command = cmd(&[bundle.to_str().expect("utf8")]);
        let r = build(
            &default_profile(),
            &command,
            &payload(tmp.path()),
            Os::MacOs,
            &no_env,
        );
        assert!(matches!(r, Err(LaunchError::BadBundle(_))), "{r:?}");
    }

    /// T42/V48: macOS never sets a restart trampoline — proven even when one is
    /// configured (eg a caller mistakenly reusing a Linux `PayloadFiles`), not just
    /// absent by omission. See the long comment on `Os::MacOs` in `add_offline`.
    #[test]
    fn macos_never_sets_restart_trampoline() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let mut p = payload(tmp.path());
        p.trampoline = Some(PathBuf::from("/app/rwpm"));
        let plan = build(
            &stored(Mode::Offline),
            &linux_script(),
            &p,
            Os::MacOs,
            &no_env,
        )
        .expect("build");
        assert_eq!(plan.arg0, None);
        assert!(env_get(&plan, ENV_RESTART).is_none());
        assert!(env_get(&plan, ENV_GAME_EXE).is_none());
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

    /// A 1.7 install under `tmp` (`RimWorld` + `Waylith.dll`) and Steam's Linux `%command%`
    /// for it: launch wrapper, reaper, runtime entry point, then the game (R37, R39).
    fn waylith_chain(tmp: &Path, extra: &[&str]) -> (PathBuf, Vec<OsString>) {
        let game = tmp.join("game");
        std::fs::create_dir_all(&game).expect("game dir");
        std::fs::write(game.join("RimWorld"), b"").expect("exe");
        std::fs::write(game.join("Waylith.dll"), b"").expect("marker");
        let exe = game.join("RimWorld");
        let mut parts = vec![
            "/steam/ubuntu12_32/steam-launch-wrapper".to_owned(),
            "--".into(),
            "/steam/ubuntu12_32/reaper".into(),
            "SteamLaunch".into(),
            "AppId=294100".into(),
            "--".into(),
            "/steam/steamapps/common/SteamLinuxRuntime_soldier/_v2-entry-point".into(),
            "--verb=waitforexitandrun".into(),
            "--".into(),
            exe.display().to_string(),
        ];
        parts.extend(extra.iter().map(|s| (*s).to_owned()));
        (exe, parts.iter().map(OsString::from).collect())
    }

    fn env_keys(plan: &LaunchPlan) -> Vec<String> {
        plan.env_set
            .iter()
            .map(|(k, _)| k.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn v62_waylith_chain_gets_no_doorstop() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let (_, command) = waylith_chain(tmp.path(), &["-popupwindow"]);
        let p = payload(tmp.path());
        for os in [Os::Linux, Os::Windows] {
            let plan = build(&stored(Mode::Offline), &command, &p, os, &no_env).expect("plan");
            let keys = env_keys(&plan);
            assert!(
                !keys.iter().any(|k| k.starts_with("DOORSTOP_")
                    || k == "LD_PRELOAD"
                    || k == "LD_LIBRARY_PATH"
                    || k == ENV_RESTART
                    || k == ENV_GAME_EXE),
                "{os:?}: {keys:?}"
            );
            assert!(plan.arg0.is_none() && plan.cwd.is_none(), "{plan:?}");
            assert!(!has_arg(&plan, "--doorstop-enabled"), "{plan:?}");
            // The whole chain is kept, the game last before its own args.
            assert_eq!(plan.program, PathBuf::from(&command[0]));
            assert_eq!(plan.args[..command.len() - 1], command[1..]);
        }
    }

    #[test]
    fn v64_waylith_offline_sets_startup_hook() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let (_, command) = waylith_chain(tmp.path(), &[]);
        let p = payload(tmp.path());
        let plan = build(&stored(Mode::Offline), &command, &p, Os::Linux, &no_env).expect("plan");
        assert_eq!(
            env_get(&plan, ENV_STARTUP_HOOKS),
            Some(&p.hook.clone().into_os_string())
        );
        assert_eq!(
            env_get(&plan, ENV_MODS_DIR),
            Some(&OsString::from("/profiles/run/Mods"))
        );
        assert_eq!(
            env_get(&plan, ENV_PAYLOAD_LOG),
            Some(&OsString::from("/profiles/run/payload.log"))
        );
        assert_eq!(
            env_get(&plan, "SteamAppId"),
            Some(&OsString::from(STEAM_APP_ID))
        );
    }

    #[test]
    fn v64_waylith_hook_prepends_existing() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let (_, command) = waylith_chain(tmp.path(), &[]);
        let p = payload(tmp.path());
        let hook = p.hook.display().to_string();
        for (os, sep, other) in [
            (Os::Linux, ':', "/x/other.dll"),
            (Os::Windows, ';', r"C:\x\other.dll"),
        ] {
            let existing = OsString::from(other);
            let env = |k: &str| (k == ENV_STARTUP_HOOKS).then(|| existing.clone());
            let plan = build(&stored(Mode::Offline), &command, &p, os, &env).expect("plan");
            let want = OsString::from(format!("{hook}{sep}{other}"));
            assert_eq!(env_get(&plan, ENV_STARTUP_HOOKS), Some(&want), "{os:?}");
            // A Restart child inherits that value: our hook is not listed twice.
            let again = |k: &str| (k == ENV_STARTUP_HOOKS).then(|| want.clone());
            let plan = build(&stored(Mode::Offline), &command, &p, os, &again).expect("plan");
            assert_eq!(
                env_get(&plan, ENV_STARTUP_HOOKS),
                Some(&want),
                "{os:?} restart"
            );
        }
    }

    #[test]
    fn v62_waylith_offline_missing_hook_fails_closed() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let (_, command) = waylith_chain(tmp.path(), &[]);
        let p = payload(tmp.path());
        std::fs::remove_file(&p.hook).expect("remove hook");
        let err = build(&stored(Mode::Offline), &command, &p, Os::Linux, &no_env)
            .expect_err("must refuse");
        assert!(
            matches!(err, LaunchError::MissingFile(ref f) if *f == p.hook),
            "{err}"
        );
    }

    #[test]
    fn v67_waylith_never_gets_logfile() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let (_, command) = waylith_chain(tmp.path(), &["-logFile", "/x/Player.log"]);
        let p = payload(tmp.path());
        for profile in [stored(Mode::Live), stored(Mode::Offline), default_profile()] {
            let plan = build(&profile, &command, &p, Os::Linux, &no_env).expect("plan");
            assert!(
                !plan
                    .args
                    .iter()
                    .any(|a| a.to_string_lossy().eq_ignore_ascii_case("-logfile")),
                "{:?}: {plan:?}",
                profile.mode
            );
            assert_eq!(plan.replaced, cmd(&["-logFile", "/x/Player.log"]));
        }
    }

    #[test]
    fn waylith_live_sets_no_hook() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let (_, command) = waylith_chain(tmp.path(), &[]);
        let p = payload(tmp.path());
        let plan = build(&stored(Mode::Live), &command, &p, Os::Linux, &no_env).expect("plan");
        assert_eq!(env_keys(&plan), vec!["SteamAppId".to_owned()]);
        let last = plan.args.last().expect("savedatafolder").to_string_lossy();
        assert!(last.to_lowercase().contains("savedatafolder"), "{plan:?}");
    }

    #[test]
    fn v69_game_program_finds_waylith_exe() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let (exe, command) = waylith_chain(tmp.path(), &["-popupwindow"]);
        assert_eq!(game_program(&command), Some(&exe.into_os_string()));
    }
}
