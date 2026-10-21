//! The per-OS swap, by explicit paths, so every OS's shapes run on any host.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::super::{
    ENV_GAME_EXE, ENV_STARTUP_HOOKS, LINUX_DOORSTOP_LIB, LaunchPlan, MACOS_DOORSTOP_LIB, Os,
    PayloadFiles, build,
};
use super::*;
use crate::builds::effective::test_support::Games;
use crate::builds::effective_game;
use crate::profile::store::{ProfileStore, Trash};
use crate::profile::{Location, Mode, Profile, ProfileId};

const STEAM: &str = "/lib/steamapps/common/RimWorld";

fn cmd(parts: &[&str]) -> Vec<OsString> {
    parts.iter().map(OsString::from).collect()
}

fn os(path: &Path) -> OsString {
    path.as_os_str().to_os_string()
}

fn stored(mode: Mode) -> Profile {
    Profile {
        id: ProfileId::parse("pinned").expect("id"),
        name: "Pinned".into(),
        mode,
        game_build: Some("1.6.4871-rev598".into()),
        location: Location::Stored {
            dir: PathBuf::from("/profiles/pinned"),
        },
    }
}

/// Payload files on disk, with a trampoline, so OFFLINE plans build on every OS.
fn payload(tmp: &Path) -> PayloadFiles {
    let mut p = PayloadFiles::beside(tmp);
    std::fs::create_dir_all(p.dll.parent().expect("parent")).expect("payload dir");
    std::fs::write(&p.dll, b"").expect("dll");
    std::fs::write(&p.hook, b"").expect("hook");
    std::fs::create_dir_all(&p.doorstop_dir).expect("doorstop dir");
    std::fs::write(p.doorstop_dir.join(LINUX_DOORSTOP_LIB), b"").expect("so");
    std::fs::write(p.doorstop_dir.join(MACOS_DOORSTOP_LIB), b"").expect("dylib");
    p.trampoline = Some(tmp.join("rwpm"));
    p
}

fn no_env(_: &str) -> Option<OsString> {
    None
}

fn env_get<'a>(plan: &'a LaunchPlan, key: &str) -> Option<&'a OsString> {
    plan.env_set.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

/// A Linux 1.6 build: `RimWorldLinux` + `start_RimWorld.sh`, or the ELF alone.
fn unity_linux(dir: &Path, with_script: bool) -> GameInstall {
    std::fs::create_dir_all(dir).expect("build dir");
    std::fs::write(dir.join(LINUX_ELF), b"").expect("elf");
    if with_script {
        std::fs::write(dir.join(LINUX_START_SCRIPT), b"").expect("script");
    }
    install(dir, LINUX_ELF, Engine::Unity)
}

/// A 1.7 build: `exe` beside `Waylith.dll`.
fn waylith(dir: &Path, exe: &str) -> GameInstall {
    std::fs::create_dir_all(dir).expect("build dir");
    std::fs::write(dir.join(exe), b"").expect("exe");
    std::fs::write(dir.join("Waylith.dll"), b"").expect("marker");
    install(dir, exe, Engine::Waylith)
}

fn install(dir: &Path, exe: &str, engine: Engine) -> GameInstall {
    GameInstall {
        game_dir: dir.to_path_buf(),
        exe: dir.join(exe),
        version: None,
        steam: None,
        engine,
    }
}

/// The native Steam chain on 1.6, as Steam hands it to `rwpm run`.
fn native_chain(game: &str) -> Vec<OsString> {
    cmd(&[
        "/steam/ubuntu12_32/steam-launch-wrapper",
        "--",
        "/steam/ubuntu12_32/reaper",
        "SteamLaunch",
        "AppId=294100",
        "--",
        "/lib/steamapps/common/SteamLinuxRuntime_sniper/_v2-entry-point",
        "--verb=waitforexitandrun",
        "--",
        game,
        "-disable-compute-shaders",
    ])
}

#[test]
fn linux_native_chain_1_6_to_waylith_build_keeps_wrappers_and_args() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let build = waylith(&tmp.path().join("1.7.4976-rev1043"), "RimWorld");
    let incoming = native_chain(&format!("{STEAM}/start_RimWorld.sh"));
    let swapped = swap_for(&incoming, &build).expect("swap");
    let mut want = incoming.clone();
    want[9] = os(&build.exe);
    assert_eq!(swapped, want);
}

/// The Flatpak chain names the script behind `scout-on-soldier-entry-point-v2`.
#[test]
fn flatpak_chain_script_to_unity_build_script() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path().join("1.6.4871-rev598");
    let build = unity_linux(&dir, true);
    let incoming = cmd(&[
        "/lib/steamapps/common/SteamLinuxRuntime_sniper/_v2-entry-point",
        "--verb",
        "waitforexitandrun",
        "--",
        "/lib/steamapps/common/SteamLinuxRuntime/scout-on-soldier-entry-point-v2",
        "--",
        &format!("{STEAM}/start_RimWorld.sh"),
    ]);
    let swapped = swap_for(&incoming, &build).expect("swap");
    assert_eq!(swapped[..6], incoming[..6]);
    assert_eq!(swapped[6], os(&dir.join(LINUX_START_SCRIPT)));
}

/// Steam's shape decides. A 1.7 exe or a script → the build's script; the ELF
/// (app Play) stays an ELF; a build without a script → its ELF.
#[test]
fn linux_unity_build_mirrors_steams_shape() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path().join("with");
    let build = unity_linux(&dir, true);
    let script = os(&dir.join(LINUX_START_SCRIPT));
    let elf = os(&dir.join(LINUX_ELF));
    let cases = [
        (format!("{STEAM}/RimWorld"), &script),
        (format!("{STEAM}/start_RimWorld.sh"), &script),
        (format!("{STEAM}/RimWorldLinux"), &elf),
    ];
    for (incoming, want) in cases {
        let swapped = swap_for(&cmd(&["/wrap", &incoming, "-x"]), &build).expect("swap");
        assert_eq!(
            swapped,
            [OsString::from("/wrap"), want.clone(), "-x".into()]
        );
    }
    let bare = tmp.path().join("bare");
    let no_script = unity_linux(&bare, false);
    let swapped = swap_for(&cmd(&[&format!("{STEAM}/RimWorld")]), &no_script).expect("swap");
    assert_eq!(swapped, [os(&bare.join(LINUX_ELF))]);
}

/// Windows paths, mixed case, both directions.
#[test]
fn windows_paths_swap_both_ways() {
    let steam = r"C:\Program Files (x86)\Steam\steamapps\common\RimWorld";
    let to_17 = Path::new(r"D:\Profiles\builds\1.7.4976-rev1043\RimWorld.exe");
    let to_16 = Path::new(r"D:\Profiles\builds\1.6.4871-rev598\RimWorldWin64.exe");
    let cases = [
        (format!(r"{steam}\rimworldwin64.EXE"), to_17),
        (format!(r"{steam}\RimWorld.exe"), to_16),
    ];
    for (incoming, exe) in cases {
        let command = cmd(&[&incoming, "-disable-compute-shaders"]);
        let at = game_target(&command).expect("target");
        let swapped = swap_to(&command, at, exe, None);
        assert_eq!(swapped, [os(exe), "-disable-compute-shaders".into()]);
    }
}

/// MacOS: Steam names the install's bundle; the build's own bundle is resolved through
/// ITS Info.plist (the 1.6 and 1.7 bundle names differ, so no prefix substitution).
#[test]
fn macos_bundle_resolves_inside_the_build() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let bundle = tmp.path().join("1.6.4871-rev595/RimWorldMac.app");
    std::fs::create_dir_all(bundle.join("Contents/MacOS")).expect("bundle");
    std::fs::write(
        bundle.join("Contents/Info.plist"),
        "<key>CFBundleExecutable</key>\n<string>RimWorldMac</string>",
    )
    .expect("plist");
    let incoming = cmd(&[
        "/Users/p/Library/Application Support/Steam/steamapps/common/RimWorld/RimWorldMac.app",
        "-disable-compute-shaders",
    ]);
    let at = game_target(&incoming).expect("target");
    let swapped = swap_to(&incoming, at, &bundle, None);
    let plan = build(
        &stored(Mode::Live),
        &swapped,
        &payload(tmp.path()),
        Os::MacOs,
        &no_env,
    )
    .expect("plan");
    assert_eq!(plan.program, bundle.join("Contents/MacOS/RimWorldMac"));
}

/// MacOS 1.7 is refused, also when it arrives as a pinned build.
#[test]
fn macos_waylith_build_is_refused() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let bundle = tmp.path().join("1.7.4976-rev1043/RimWorld.app");
    std::fs::create_dir_all(bundle.join("Contents/MacOS")).expect("bundle");
    std::fs::write(bundle.join("Contents/MacOS/Waylith.dll"), b"").expect("marker");
    let incoming = cmd(&["/Steam/common/RimWorld/RimWorldMac.app"]);
    let swapped = swap_to(&incoming, 0, &bundle, None);
    let refused = build(
        &stored(Mode::Live),
        &swapped,
        &payload(tmp.path()),
        Os::MacOs,
        &no_env,
    );
    assert!(matches!(refused, Err(LaunchError::MacOsWaylithUnsupported)));
}

/// No match and more than one match are refused; flags never match.
#[test]
fn locator_refuses_none_and_ambiguous_and_skips_flags() {
    assert!(matches!(
        game_target(&cmd(&["/opt/launcher.sh", "--go"])),
        Err(LaunchError::NoGameInCommand)
    ));
    assert!(matches!(
        game_target(&cmd(&["/a/RimWorld/RimWorld", "/b/RimWorldLinux"])),
        Err(LaunchError::AmbiguousGameInCommand)
    ));
    let flags = cmd(&[
        "/wrap",
        "--cwd=/lib/common/RimWorld",
        "-savedatafolder=/x/RimWorld",
        "/lib/common/RimWorld/RimWorldLinux",
        "-logFile",
    ]);
    assert_eq!(game_target(&flags).expect("target"), 3);
    assert_eq!(
        game_target(&cmd(&["/b/RimWorldMac.app/"])).expect("trailing slash"),
        0
    );
}

/// Idempotent: app Play already hands over the build's own exe (decision 1).
#[test]
fn swap_of_the_builds_own_exe_is_unchanged() {
    let tmp = tempfile::tempdir().expect("tempdir");
    for build in [
        unity_linux(&tmp.path().join("u"), true),
        waylith(&tmp.path().join("w"), "RimWorld"),
    ] {
        let command = vec![os(&build.exe), "-popupwindow".into()];
        assert_eq!(swap_for(&command, &build).expect("swap"), command);
    }
}

struct NoTrash;
impl Trash for NoTrash {
    fn trash(&self, _: &Path) -> std::io::Result<()> {
        Ok(())
    }
}

/// Through the resolver: unpinned → untouched; pinned → the build's exe.
#[test]
fn swap_target_follows_the_effective_game() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().join("profiles");
    let games = Games::new(tmp.path(), &root);
    let store = ProfileStore::open(root.clone(), tmp.path().join("save"), NoTrash).expect("store");
    let plain = store.create("Plain").expect("create");
    let pinned = games.pin(&store, &store.create("Pinned").expect("create"));
    let command = vec![os(&games.live.exe), "-popupwindow".into()];
    let app = tmp.path().join("app");
    let steam = effective_game(&plain, Some(&games.live), &root, &app).expect("steam");
    assert_eq!(swap_target(&command, &steam).expect("swap"), command);
    let build = effective_game(&pinned, Some(&games.live), &root, &app).expect("build");
    let swapped = swap_target(&command, &build).expect("swap");
    assert_eq!(swapped[0], os(&build.exe));
    assert!(build.exe.starts_with(&games.build_dir));
    assert_eq!(swapped[1], command[1]);
}

/// In-game Restart re-runs the pinned build. Plain OFFLINE → the build's ELF;
/// wrapped OFFLINE → the build's start script (rwpm goes between).
#[test]
fn restart_env_names_the_build() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path().join("1.6.4871-rev598");
    let build_game = unity_linux(&dir, true);
    let p = payload(tmp.path());
    let plain =
        swap_for(&cmd(&[&format!("{STEAM}/start_RimWorld.sh")]), &build_game).expect("swap plain");
    let plan = build(&stored(Mode::Offline), &plain, &p, Os::Linux, &no_env).expect("plain");
    assert_eq!(
        env_get(&plan, ENV_GAME_EXE),
        Some(&os(&dir.join(LINUX_ELF)))
    );
    let wrapped = swap_for(
        &native_chain(&format!("{STEAM}/start_RimWorld.sh")),
        &build_game,
    )
    .expect("swap wrapped");
    let plan = build(&stored(Mode::Offline), &wrapped, &p, Os::Linux, &no_env).expect("wrapped");
    assert_eq!(
        env_get(&plan, ENV_GAME_EXE),
        Some(&os(&dir.join(LINUX_START_SCRIPT)))
    );
}

/// The result follows the build, not Steam: a Unity Steam command on a Waylith build gets the
/// startup hook and no Doorstop; a Waylith Steam command on a Unity build gets Doorstop.
#[test]
fn engine_follows_the_build() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let p = payload(tmp.path());
    let profile = stored(Mode::Offline);
    let w = waylith(&tmp.path().join("w"), "RimWorld");
    let to_waylith = swap_for(&cmd(&[&format!("{STEAM}/RimWorldLinux")]), &w).expect("swap to 1.7");
    let plan = build(&profile, &to_waylith, &p, Os::Linux, &no_env).expect("1.7 plan");
    assert!(env_get(&plan, ENV_STARTUP_HOOKS).is_some());
    assert!(env_get(&plan, "LD_PRELOAD").is_none());
    let u = unity_linux(&tmp.path().join("u"), false);
    let to_unity = swap_for(&cmd(&[&format!("{STEAM}/RimWorld")]), &u).expect("swap to 1.6");
    let plan = build(&profile, &to_unity, &p, Os::Linux, &no_env).expect("1.6 plan");
    assert!(env_get(&plan, "LD_PRELOAD").is_some());
    assert!(env_get(&plan, ENV_STARTUP_HOOKS).is_none());
}
