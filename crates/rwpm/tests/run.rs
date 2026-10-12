//! `rwpm run` contract (I.cmd, V11, V17) against a fake game. Linux/Unix only.
//!
//! The process-scan scenarios share global state (any `RimWorldLinux` process counts),
//! so they run serially inside one test. They fail if a real RimWorld is open.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use rwpm_core::profile::{LastRun, Mode, ProfileFile, ProfileStore, RunOutcome, StartedBy, Trash};

const EX_USAGE: i32 = 64;
const EX_TEMPFAIL: i32 = 75;

struct NoTrash;

impl Trash for NoTrash {
    fn trash(&self, _: &Path) -> std::io::Result<()> {
        Ok(())
    }
}

/// Isolated XDG dirs so no real profiles or locks are touched.
struct Sandbox {
    tmp: tempfile::TempDir,
}

impl Sandbox {
    fn new() -> Self {
        let tmp = tempfile::tempdir().expect("tempdir");
        for d in ["data", "config"] {
            std::fs::create_dir_all(tmp.path().join(d)).expect("xdg dir");
        }
        Self { tmp }
    }

    fn path(&self) -> &Path {
        self.tmp.path()
    }

    fn app_dir(&self) -> PathBuf {
        self.path().join("data/RimWorld Profile Manager")
    }

    fn create_profile(&self, name: &str) -> String {
        let store = ProfileStore::open(
            self.app_dir().join("profiles"),
            self.path().join("game-save"),
            NoTrash,
        )
        .expect("store");
        store.create(name).expect("create").id.to_string()
    }

    fn rwpm(&self) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_rwpm"));
        c.env("XDG_DATA_HOME", self.path().join("data"))
            .env("XDG_CONFIG_HOME", self.path().join("config"))
            .env("HOME", self.path())
            // Who-started-it signals come only from each test.
            .env_remove("SteamAppId")
            .env_remove("SteamGameId")
            .env_remove("RWPM_STARTED_BY");
        c
    }

    /// The profile's `last-run.toml` (V32).
    fn last_run(&self, id: &str) -> LastRun {
        let path = self
            .app_dir()
            .join("profiles")
            .join(id)
            .join("last-run.toml");
        LastRun::read(&path).unwrap_or_else(|| panic!("no record at {}", path.display()))
    }

    /// Writes an executable `sh` script and returns its path.
    fn script(&self, name: &str, body: &str) -> PathBuf {
        let p = self.path().join(name);
        std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).expect("script");
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        p
    }

    /// A copy of `sleep` named like the game, so process-name scans see it.
    fn fake_game_binary(&self) -> PathBuf {
        let dir = self.path().join("bin");
        std::fs::create_dir_all(&dir).expect("bin dir");
        let p = dir.join("RimWorldLinux");
        std::fs::copy(which_sleep(), &p).expect("copy sleep");
        p
    }
}

fn which_sleep() -> PathBuf {
    ["/usr/bin/sleep", "/bin/sleep"]
        .into_iter()
        .map(PathBuf::from)
        .find(|p| p.is_file())
        .expect("sleep binary")
}

fn kill(mut child: Child) {
    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn usage_errors_exit_64() {
    let sb = Sandbox::new();
    let id = sb.create_profile("Run");
    let cases: [&[&str]; 5] = [
        &["run"],
        &["run", &id, "/bin/true"],
        &["run", "--from", "steam", &id, "--", "/bin/true"],
        &["run", "Bad Id", "--", "/bin/true"],
        &["run", "missing", "--", "/bin/true"],
    ];
    for args in cases {
        let out = sb.rwpm().args(args).output().expect("rwpm");
        assert_eq!(
            out.status.code(),
            Some(EX_USAGE),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(!out.stderr.is_empty(), "{args:?} printed no reason");
    }
}

#[test]
fn run_cli_contract_serial() {
    exit_code_is_first_game_process();
    passes_savedatafolder_and_logfile_to_game();
    waits_for_restart_child();
    refuses_while_any_game_runs();
    refuses_while_another_run_holds_lock();
    guard_cancel_does_not_launch();
    guard_launch_anyway_launches();
    guard_turn_off_edits_config_then_launches();
    guard_add_copies_then_launches();
    guard_dialog_unavailable_launches_and_logs();
    guard_clear_profile_launches_without_asking();
    wrapped_launch_runs_rwpm_inside_wrapper();
    records_who_started_each_run();
    records_a_failed_start();
    run_uses_moved_root();
}

fn records_who_started_each_run() {
    let sb = Sandbox::new();
    let id = sb.create_profile("Run");
    let game = sb.script("game.sh", "exit 0");
    // (flags, one environment variable, expected)
    type Case<'a> = (&'a [&'a str], Option<(&'a str, &'a str)>, StartedBy);
    let cases: [Case; 4] = [
        (&[], None, StartedBy::Other),
        (&[], Some(("SteamGameId", "294100")), StartedBy::Steam),
        (
            &["--from", "rimsort"],
            Some(("SteamAppId", "294100")),
            StartedBy::Rimsort,
        ),
        (
            &["--from", "rimsort"],
            Some(("RWPM_STARTED_BY", "app")),
            StartedBy::App,
        ),
    ];
    for (flags, var, want) in cases {
        let mut cmd = sb.rwpm();
        cmd.arg("run").args(flags).arg(&id).arg("--").arg(&game);
        if let Some((k, v)) = var {
            cmd.env(k, v);
        }
        assert_eq!(cmd.status().expect("rwpm").code(), Some(0));
        let run = sb.last_run(&id);
        assert_eq!(
            (run.started_by, run.outcome, run.exit),
            (want, RunOutcome::Ok, Some(0)),
            "{flags:?} {var:?}"
        );
    }
}

fn records_a_failed_start() {
    let sb = Sandbox::new();
    let id = sb.create_profile("Run");
    let missing = sb.path().join("no-such-game");
    let code = sb
        .rwpm()
        .arg("run")
        .arg(&id)
        .arg("--")
        .arg(&missing)
        .status()
        .expect("rwpm")
        .code();
    assert_eq!(code, Some(1));
    let run = sb.last_run(&id);
    assert_eq!(run.outcome, RunOutcome::Failed);
    assert!(run.reason.contains("cannot start"), "{}", run.reason);
}

fn exit_code_is_first_game_process() {
    let sb = Sandbox::new();
    let id = sb.create_profile("Run");
    let game = sb.script("game.sh", "exit 7");
    let status = sb
        .rwpm()
        .arg("run")
        .arg(&id)
        .arg("--")
        .arg(&game)
        .status()
        .expect("rwpm");
    assert_eq!(status.code(), Some(7), "exit code passthrough");
    let run = sb.last_run(&id);
    assert_eq!((run.outcome, run.exit), (RunOutcome::Ok, Some(7)));
}

fn passes_savedatafolder_and_logfile_to_game() {
    let sb = Sandbox::new();
    let id = sb.create_profile("Run");
    let argv = sb.path().join("argv.txt");
    let game = sb.script(
        "game.sh",
        &format!(
            "printf '%s\\n' \"$@\" > '{}'\necho \"$SteamAppId\" >> '{}'",
            argv.display(),
            argv.display()
        ),
    );
    let status = sb
        .rwpm()
        .arg("run")
        .arg(&id)
        .arg("--")
        .arg(&game)
        .arg("-popupwindow")
        .arg("-savedatafolder=/stale")
        .status()
        .expect("rwpm");
    assert_eq!(status.code(), Some(0));
    let lines: Vec<String> = std::fs::read_to_string(&argv)
        .expect("argv")
        .lines()
        .map(str::to_owned)
        .collect();
    let profile_dir = sb.app_dir().join("profiles").join(&id);
    let expected = [
        "-popupwindow".to_owned(),
        format!("-savedatafolder={}", profile_dir.join("SaveData").display()),
        "-logFile".to_owned(),
        profile_dir.join("Player.log").display().to_string(),
        "294100".to_owned(),
    ];
    assert_eq!(lines, expected);
}

fn waits_for_restart_child() {
    let sb = Sandbox::new();
    let id = sb.create_profile("Run");
    let fake = sb.fake_game_binary();
    // Mimics GenCommandLine.Restart: spawn the child, then the parent exits.
    let game = sb.script("game.sh", &format!("'{}' 2 &\nexit 3", fake.display()));
    let start = Instant::now();
    let status = sb
        .rwpm()
        .arg("run")
        .arg(&id)
        .arg("--")
        .arg(&game)
        .status()
        .expect("rwpm");
    let elapsed = start.elapsed();
    assert_eq!(status.code(), Some(3), "first process's exit code");
    assert!(
        elapsed >= Duration::from_millis(1900),
        "returned after {elapsed:?}, before the restart child ended"
    );
}

fn refuses_while_any_game_runs() {
    let sb = Sandbox::new();
    let id = sb.create_profile("Run");
    let running = Command::new(sb.fake_game_binary())
        .arg("10")
        .spawn()
        .expect("fake game");
    std::thread::sleep(Duration::from_millis(200));
    let marker = sb.path().join("started");
    let game = sb.script("game.sh", &format!("touch '{}'", marker.display()));
    let out = sb
        .rwpm()
        .arg("run")
        .arg(&id)
        .arg("--")
        .arg(&game)
        .output()
        .expect("rwpm");
    kill(running);
    assert_eq!(
        out.status.code(),
        Some(EX_TEMPFAIL),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!marker.exists(), "game started despite a running RimWorld");
}

fn refuses_while_another_run_holds_lock() {
    let sb = Sandbox::new();
    let id = sb.create_profile("Run");
    std::fs::create_dir_all(sb.app_dir()).expect("app dir");
    let lock = std::fs::File::create(sb.app_dir().join("run.lock")).expect("lock file");
    lock.try_lock().expect("take lock");
    let marker = sb.path().join("started");
    let game = sb.script("game.sh", &format!("touch '{}'", marker.display()));
    let out = sb
        .rwpm()
        .arg("run")
        .arg(&id)
        .arg("--")
        .arg(&game)
        .output()
        .expect("rwpm");
    drop(lock);
    assert_eq!(
        out.status.code(),
        Some(EX_TEMPFAIL),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let run = sb.last_run(&id);
    assert_eq!(
        run.outcome,
        RunOutcome::Refused,
        "a refused launch is recorded too"
    );
    assert!(
        run.reason.contains("already being launched"),
        "{}",
        run.reason
    );
    assert!(
        !marker.exists(),
        "game started while another rwpm run held the lock"
    );
}

/// A fake Steam library with a RimWorld whose "game" just touches a marker, one workshop
/// mod (Harmony), and an OFFLINE profile listing `active`. Runs a staged copy of rwpm
/// (payload + doorstop stubs beside it, as shipped) with `RWPM_GUARD_CHOICE=choice`.
struct GuardRun {
    sb: Sandbox,
    profile: PathBuf,
    code: Option<i32>,
}

impl GuardRun {
    fn new(active: &[&str], choice: &str) -> Self {
        let sb = Sandbox::new();
        let library = sb.path().join("lib/steamapps");
        let game = library.join("common/RimWorld");
        about(&game.join("Data/Core"), "ludeon.rimworld");
        about(
            &library.join("workshop/content/294100/2009463077"),
            "brrainz.harmony",
        );
        let marker = sb.path().join("started");
        let exe = game.join("RimWorldLinux");
        std::fs::write(&exe, format!("#!/bin/sh\ntouch '{}'\n", marker.display())).expect("game");
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        let id = sb.create_profile("Guarded");
        let profile = sb.app_dir().join("profiles").join(&id);
        let toml = profile.join("profile.toml");
        let mut file = ProfileFile::read(&toml).expect("toml");
        file.mode = Mode::Offline;
        file.write_atomic(&toml).expect("toml");
        let config = profile.join("SaveData/Config/ModsConfig.xml");
        std::fs::create_dir_all(config.parent().expect("parent")).expect("config dir");
        let items: String = active
            .iter()
            .map(|a| format!("\n    <li>{a}</li>"))
            .collect();
        std::fs::write(
            &config,
            format!(
                "<ModsConfigData>\n  <activeMods>{items}\n  </activeMods>\n</ModsConfigData>\n"
            ),
        )
        .expect("config");
        let mut cmd = Command::new(staged_rwpm(&sb));
        cmd.env("XDG_DATA_HOME", sb.path().join("data"))
            .env("XDG_CONFIG_HOME", sb.path().join("config"))
            .env("HOME", sb.path())
            .env("RWPM_GUARD_CHOICE", choice)
            .args(["run", &id, "--"])
            .arg(&exe);
        let code = cmd.status().expect("rwpm").code();
        Self { sb, profile, code }
    }

    fn started(&self) -> bool {
        self.sb.path().join("started").exists()
    }

    fn log(&self) -> String {
        std::fs::read_to_string(self.profile.join("rwpm.log")).unwrap_or_default()
    }

    fn config(&self) -> String {
        std::fs::read_to_string(self.profile.join("SaveData/Config/ModsConfig.xml"))
            .expect("config")
    }
}

fn about(dir: &Path, id: &str) {
    std::fs::create_dir_all(dir.join("About")).expect("about");
    std::fs::write(
        dir.join("About/About.xml"),
        format!("<ModMetaData><packageId>{id}</packageId></ModMetaData>"),
    )
    .expect("about");
}

const UNFROZEN: [&str; 2] = ["ludeon.rimworld", "brrainz.harmony"];

fn guard_cancel_does_not_launch() {
    let run = GuardRun::new(&UNFROZEN, "cancel");
    assert_eq!(run.code, Some(0));
    assert!(!run.started(), "game started after Cancel");
    assert!(
        run.log().contains("brrainz.harmony; player chose Cancel"),
        "{}",
        run.log()
    );
    let record = LastRun::read(&run.profile.join("last-run.toml")).expect("record");
    assert_eq!(record.outcome, RunOutcome::Refused);
    assert!(record.reason.contains("Cancel"), "{}", record.reason);
}

fn guard_launch_anyway_launches() {
    let run = GuardRun::new(&UNFROZEN, "launch");
    assert_eq!(run.code, Some(0));
    assert!(run.started());
    assert!(!run.profile.join("Mods/2009463077").exists());
    assert!(run.config().contains("brrainz.harmony"));
}

fn guard_turn_off_edits_config_then_launches() {
    let run = GuardRun::new(&UNFROZEN, "off");
    assert!(run.started());
    assert!(
        !run.config().contains("brrainz.harmony"),
        "{}",
        run.config()
    );
    assert!(run.config().contains("ludeon.rimworld"));
}

fn guard_add_copies_then_launches() {
    let run = GuardRun::new(&UNFROZEN, "add");
    assert!(run.started());
    assert!(
        run.profile
            .join("Mods/2009463077/About/About.xml")
            .is_file()
    );
    let file = ProfileFile::read(&run.profile.join("profile.toml")).expect("toml");
    assert_eq!(file.frozen_mods.len(), 1);
}

fn guard_dialog_unavailable_launches_and_logs() {
    let run = GuardRun::new(&UNFROZEN, "unavailable");
    assert!(run.started());
    assert!(
        run.log().contains("dialog unavailable") && run.log().contains("brrainz.harmony"),
        "{}",
        run.log()
    );
}

fn guard_clear_profile_launches_without_asking() {
    // `cancel` would stop the launch if the guard asked at all.
    let run = GuardRun::new(&["ludeon.rimworld"], "cancel");
    assert!(run.started());
    assert!(run.log().is_empty(), "{}", run.log());
}

/// V29 / B3: Steam Linux Runtime (Flatpak Steam) runs `%command%` through `/bin/sh`
/// wrappers. They must see no Doorstop env (their `$(…)` would break); rwpm takes the
/// start script's place and adds Doorstop for the game alone.
fn wrapped_launch_runs_rwpm_inside_wrapper() {
    let sb = Sandbox::new();
    let game = sb.path().join("lib/steamapps/common/RimWorld");
    about(&game.join("Data/Core"), "ludeon.rimworld");
    let seen = sb.path().join("seen.txt");
    std::fs::write(game.join("start_RimWorld.sh"), "#!/bin/sh\nexit 99\n").expect("script");
    std::fs::write(
        game.join("RimWorldLinux"),
        format!(
            "#!/bin/sh\n{{ printf '%s\\n' \"$@\"; echo \"enabled=$DOORSTOP_ENABLED\"; \
             echo \"mods=$RWPM_MODS_DIR\"; echo \"cwd=$(pwd)\"; }} >> '{}'\nexit 7\n",
            seen.display()
        ),
    )
    .expect("game");
    std::fs::set_permissions(
        game.join("RimWorldLinux"),
        std::fs::Permissions::from_mode(0o755),
    )
    .expect("chmod");
    let wrapper = sb.script(
        "entry-point",
        &format!(
            "me=\"$(readlink -f \"$0\")\"\n[ -n \"$me\" ] || exit 90\n\
             echo \"wrapper enabled=${{DOORSTOP_ENABLED-}} preload=${{LD_PRELOAD-}}\" >> '{}'\n\
             [ \"$1\" = -- ] && shift\nexec \"$@\"",
            seen.display()
        ),
    );
    let id = sb.create_profile("Wrapped");
    let profile = sb.app_dir().join("profiles").join(&id);
    let toml = profile.join("profile.toml");
    let mut file = ProfileFile::read(&toml).expect("toml");
    file.mode = Mode::Offline;
    file.write_atomic(&toml).expect("toml");
    let config = profile.join("SaveData/Config/ModsConfig.xml");
    std::fs::create_dir_all(config.parent().expect("parent")).expect("config dir");
    std::fs::write(
        &config,
        "<ModsConfigData><activeMods><li>ludeon.rimworld</li></activeMods></ModsConfigData>",
    )
    .expect("config");
    let status = Command::new(staged_rwpm(&sb))
        .env("XDG_DATA_HOME", sb.path().join("data"))
        .env("XDG_CONFIG_HOME", sb.path().join("config"))
        .env("HOME", sb.path())
        .env("RWPM_GUARD_CHOICE", "cancel")
        .env_remove("LD_PRELOAD")
        .args(["run", &id, "--"])
        .arg(&wrapper)
        .arg("--")
        .arg(game.join("start_RimWorld.sh"))
        .arg("-popupwindow")
        .status()
        .expect("rwpm");
    assert_eq!(
        status.code(),
        Some(7),
        "game's exit code through the wrapper"
    );
    let seen = std::fs::read_to_string(&seen).expect("game ran");
    let lines: Vec<&str> = seen.lines().collect();
    let expected = [
        "wrapper enabled= preload=".to_owned(),
        "-popupwindow".to_owned(),
        format!("-savedatafolder={}", profile.join("SaveData").display()),
        "-logFile".to_owned(),
        profile.join("Player.log").display().to_string(),
        "enabled=1".to_owned(),
        format!("mods={}", profile.join("Mods").display()),
        format!("cwd={}", game.display()),
    ];
    assert_eq!(lines, expected);
    // The guard found the game through the start script, so it neither skipped nor asked.
    let log = std::fs::read_to_string(profile.join("rwpm.log")).unwrap_or_default();
    assert!(log.is_empty(), "{log}");
}

/// Shows the real zenity dialog (no choice override). Manual: close or click it.
#[test]
#[ignore = "opens a real dialog on the desktop"]
fn guard_real_dialog_manual() {
    let run = GuardRun::new(&UNFROZEN, "");
    println!(
        "code={:?} started={} log={}",
        run.code,
        run.started(),
        run.log()
    );
}

/// rwpm copied into `<sandbox>/app` with stub payload + doorstop files beside it, as shipped.
fn staged_rwpm(sb: &Sandbox) -> PathBuf {
    let app = sb.path().join("app");
    for (dir, file) in [
        ("payload", "Rwpm.Payload.dll"),
        ("doorstop", "libdoorstop.so"),
    ] {
        std::fs::create_dir_all(app.join(dir)).expect("app dir");
        std::fs::write(app.join(dir).join(file), b"").expect("stub");
    }
    std::fs::copy(env!("CARGO_BIN_EXE_rwpm"), app.join("rwpm")).expect("stage rwpm");
    app.join("rwpm")
}

/// V28: the in-game Restart runs `rwpm <game args>` with the game's env, where the
/// payload has turned Doorstop off. rwpm must turn it back on and exec the game.
#[test]
fn restart_mode_rebuilds_launch_and_execs() {
    let sb = Sandbox::new();
    let id = sb.create_profile("Restarted");
    let profile = sb.app_dir().join("profiles").join(&id);
    let toml = profile.join("profile.toml");
    let mut file = ProfileFile::read(&toml).expect("toml");
    file.mode = Mode::Offline;
    file.write_atomic(&toml).expect("toml");
    let out = sb.path().join("seen.txt");
    let game = sb.script(
        "game.sh",
        &format!(
            "{{ printf '%s\\n' \"$@\"; echo \"enabled=$DOORSTOP_ENABLED\"; echo \"preload=$LD_PRELOAD\"; \
             echo \"restart=$RWPM_RESTART\"; }} > '{}'\nexit 5",
            out.display()
        ),
    );
    let rwpm = staged_rwpm(&sb);
    let save = profile.join("SaveData");
    let status = Command::new(&rwpm)
        .env("XDG_DATA_HOME", sb.path().join("data"))
        .env("XDG_CONFIG_HOME", sb.path().join("config"))
        .env("HOME", sb.path())
        .env("RWPM_RESTART", &id)
        .env("RWPM_GAME_EXE", &game)
        .env("LD_PRELOAD", "libdoorstop.so")
        .env_remove("DOORSTOP_ENABLED")
        .arg(format!("-savedatafolder={}", save.display()))
        .args(["-logFile", "/old/Player.log", "-popupwindow"])
        .status()
        .expect("rwpm restart");
    assert_eq!(status.code(), Some(5), "exec'd game's exit code");
    let seen = std::fs::read_to_string(&out).expect("game ran");
    let lines: Vec<&str> = seen.lines().collect();
    let expected = [
        "-popupwindow".to_owned(),
        format!("-savedatafolder={}", save.display()),
        "-logFile".to_owned(),
        profile.join("Player.log").display().to_string(),
        "enabled=1".to_owned(),
        "preload=libdoorstop.so".to_owned(),
        format!("restart={id}"),
    ];
    assert_eq!(lines, expected);
}

#[test]
fn restart_env_ignored_for_subcommands() {
    let sb = Sandbox::new();
    let out = sb
        .rwpm()
        .env("RWPM_RESTART", "whatever")
        .arg("--version")
        .output()
        .expect("rwpm");
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("rwpm "));
    let usage = sb
        .rwpm()
        .env("RWPM_RESTART", "whatever")
        .arg("run")
        .output()
        .expect("rwpm");
    assert_eq!(usage.status.code(), Some(EX_USAGE));
}

/// V29: the inner-launch marker without its env is refused, never the GUI.
#[test]
fn inner_marker_without_restart_env_is_refused() {
    let sb = Sandbox::new();
    let out = sb
        .rwpm()
        .env_remove("RWPM_RESTART")
        .args(["--rwpm-inner", "-popupwindow"])
        .output()
        .expect("rwpm");
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("unknown command"));
}

/// T14: after a Move, settings.toml names the profiles folder; rwpm run must use it.
/// A full launch: it scans for running games, so it runs inside the serial test.
fn run_uses_moved_root() {
    let sb = Sandbox::new();
    let moved = sb.path().join("elsewhere/profiles");
    std::fs::create_dir_all(sb.app_dir()).expect("app dir");
    std::fs::write(
        sb.app_dir().join("settings.toml"),
        format!("schema = 1\nprofiles_root = '{}'\n", moved.display()),
    )
    .expect("settings");
    let store =
        ProfileStore::open(moved.clone(), sb.path().join("game-save"), NoTrash).expect("store");
    let id = store.create("Moved").expect("create").id.to_string();
    let argv = sb.path().join("argv.txt");
    let game = sb.script(
        "game.sh",
        &format!("printf '%s\\n' \"$@\" > '{}'", argv.display()),
    );
    let status = sb
        .rwpm()
        .args(["run", &id, "--"])
        .arg(&game)
        .status()
        .expect("rwpm");
    assert_eq!(status.code(), Some(0));
    let seen = std::fs::read_to_string(&argv).expect("argv");
    assert!(
        seen.contains(&format!(
            "-savedatafolder={}",
            moved.join(&id).join("SaveData").display()
        )),
        "{seen}"
    );
}
