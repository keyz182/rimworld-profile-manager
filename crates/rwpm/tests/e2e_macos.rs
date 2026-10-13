//! T48 macOS e2e: exercises the real product code paths (ProfileStore, discovery,
//! launch::build, the packaged bundle from `scripts/package.sh macos`) against this
//! machine's actual Steam + RimWorld install — not the spike's throwaway harness
//! (spike/macos/), and not a fake-binary fixture like `tests/run.rs` (Linux only, T43).
//!
//! Ignored: launches the real game. Run after `scripts/package.sh macos`:
//!   cargo test -p rwpm --test e2e_macos -- --ignored --nocapture
#![cfg(target_os = "macos")]

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use rwpm_core::discovery;
use rwpm_core::launch::{self, Os, PayloadFiles};
use rwpm_core::paths;
use rwpm_core::profile::offline::{apply_offline, plan_offline};
use rwpm_core::profile::{OsTrash, ProcessProbe, ProfileStore};

const PROFILE_NAME: &str = "rwpm e2e macos smoke";

/// Finds the newest `scripts/package.sh macos` bundle under `target/dist/<name>/*.app`.
fn newest_bundle() -> PathBuf {
    let dist = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/dist");
    std::fs::read_dir(&dist)
        .expect("target/dist — run scripts/package.sh macos first")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .flat_map(|d| {
            std::fs::read_dir(&d)
                .into_iter()
                .flatten()
                .filter_map(Result::ok)
        })
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("app"))
        .max_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok())
        .expect("no packaged .app under target/dist — run scripts/package.sh macos first")
}

#[test]
#[ignore = "launches the real game against the real Steam install"]
fn real_offline_launch_through_the_packaged_bundle() {
    let game = discovery::discover(None).expect("discover real RimWorld install");
    let bundle = newest_bundle();
    eprintln!("bundle: {}", bundle.display());

    let root = paths::default_profiles_root().expect("profiles root");
    let save_dir = paths::default_game_save_dir().expect("save dir");
    let store = ProfileStore::open(root, save_dir, OsTrash).expect("open store");

    // Clean slate: trash a leftover profile from a previous run of this test.
    if let Ok(listing) = store.list()
        && let Some(p) = listing.profiles.iter().find(|p| p.name == PROFILE_NAME)
    {
        let _ = store.delete(&p.id, &ProcessProbe);
    }
    let profile = store.create(PROFILE_NAME).expect("create profile");

    // V36: no mod list yet on a brand-new profile → nothing to freeze, trivial OFFLINE.
    let app_dir = paths::app_data_dir().expect("app data dir");
    let effective =
        rwpm_core::builds::effective_game(&profile, Some(&game), store.root(), &app_dir)
            .expect("effective game");
    let plan = plan_offline(&store, &profile.id, &effective, false).expect("plan_offline");
    let profile =
        apply_offline(&store, &plan, &[], &ProcessProbe, &mut |_| {}).expect("apply_offline");

    let payload = PayloadFiles {
        dll: bundle.join("Contents/MacOS/payload/Rwpm.Payload.dll"),
        hook: bundle.join("Contents/MacOS/payload/Rwpm.Payload.Hook.dll"),
        doorstop_dir: bundle.join("Contents/MacOS/doorstop"),
        trampoline: None,
        chain_target: None,
    };
    // Steam's %command% names the GAME bundle directory itself (R33, T37) — this is the
    // exact shape `rwpm run` sees from a real Steam launch, not a pre-resolved inner
    // binary. `game.exe` is the real RimWorldMac.app, discovered live; our OWN packaged
    // `bundle` only supplies the payload/doorstop files, via `payload` below.
    let command: Vec<OsString> = vec![game.exe.clone().into_os_string()];
    let no_env = |_: &str| None;
    let plan = launch::build(&profile, &command, &payload, Os::MacOs, &no_env).expect("build plan");

    let exe_name = std::process::Command::new("/usr/libexec/PlistBuddy")
        .args([
            "-c",
            "Print :CFBundleExecutable",
            game.exe.join("Contents/Info.plist").to_str().unwrap(),
        ])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned());
    assert_eq!(
        plan.program,
        game.exe
            .join("Contents/MacOS")
            .join(exe_name.expect("plist read")),
        "V41: %command% naming the game bundle resolved to its inner Mach-O"
    );
    let mods_dir = profile.mods_dir().expect("OFFLINE profile has a Mods dir");
    assert_eq!(
        env_get(&plan, "RWPM_MODS_DIR"),
        Some(mods_dir.as_os_str().to_owned())
    );
    assert!(env_get(&plan, "DYLD_INSERT_LIBRARIES").is_some());

    let mut cmd = Command::new(&plan.program);
    cmd.args(&plan.args);
    for (k, v) in &plan.env_set {
        cmd.env(k, v);
    }
    for k in &plan.env_remove {
        cmd.env_remove(k);
    }
    if let Some(cwd) = &plan.cwd {
        cmd.current_dir(cwd);
    }
    let mut child = cmd.spawn().expect("spawn game");
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if child.try_wait().ok().flatten().is_some() {
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            break;
        }
        std::thread::sleep(Duration::from_millis(500));
    }

    // V15: a stored (non-Default) profile's log sits beside SaveData, not inside it.
    let player_log = profile.log_path(rwpm_core::discovery::Engine::Unity);
    assert!(
        player_log.is_file(),
        "no Player.log at {}",
        player_log.display()
    );
    eprintln!(
        "Player.log: {} bytes",
        std::fs::metadata(&player_log).map(|m| m.len()).unwrap_or(0)
    );

    // Cleanup: leave no stray profile behind.
    let _ = store.delete(&profile.id, &ProcessProbe);
}

fn env_get(plan: &launch::LaunchPlan, key: &str) -> Option<OsString> {
    plan.env_set
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.clone())
}
