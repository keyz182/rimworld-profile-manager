//! T82b: running steamcmd. Every test here sets the free space wanted to 0, so none depends
//! on the disk it runs on. Each test puts a fake `steamcmd.sh` where the real one would be, so
//! these start processes, and so they live in their own test program (a forked child can hold
//! another test's lock file open until it runs its program).

#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use rwpm_core::fetch::Status;
use rwpm_core::steamcmd::run::{Layout, download, download_with};

/// Prints coloured results with no line break between them, like the real one (R52).
/// Id 222 fails, 333 delivers another mod, 444 prints nothing, 555 needs 32-bit libraries,
/// 666 hangs with a child, anything else delivers `Good.Mod`.
const FAKE: &str = r#"#!/bin/sh
here="$(cd "$(dirname "$0")" && pwd)"
echo run >> "$here/runs.txt"
printf '%s\n' "$@" > "$here/args.txt"
printf '%s\n%s\n' "$HOME" "${XDG_DATA_HOME-unset}" > "$here/env.txt"
content="$here/install/steamapps/workshop/content/294100"
while [ $# -gt 0 ]; do
  if [ "$1" = "+workshop_download_item" ]; then
    id="$3"
    case "$id" in
      222) printf '\033[31mERROR! Download item 222 failed (File Not Found).\033[0m' ;;
      444) ;;
      555) echo "linux32/steamcmd: error while loading shared libraries: libc.so.6" >&2; exit 127 ;;
      666) sleep 60 & echo $! > "$here/child.pid"; wait ;;
      *)
        mkdir -p "$content/$id/About"
        pkg=Good.Mod
        [ "$id" = 333 ] && pkg=Evil.Other
        printf '<ModMetaData><packageId>%s</packageId></ModMetaData>' "$pkg" > "$content/$id/About/About.xml"
        printf '\033[1mSuccess.\033[0m Downloaded item %s to "%s/%s" (1 bytes)' "$id" "$content" "$id" ;;
    esac
    shift 3
  else
    shift
  fi
done
"#;

fn layout_with_fake(tmp: &Path) -> Layout {
    let layout = Layout::new(tmp);
    std::fs::create_dir_all(layout.root()).expect("root");
    std::fs::write(layout.program(), FAKE).expect("script");
    std::fs::set_permissions(layout.program(), std::fs::Permissions::from_mode(0o755))
        .expect("chmod");
    layout
}

fn wanted(ids: &[u64]) -> Vec<(u64, String)> {
    ids.iter().map(|id| (*id, "Good.Mod".to_owned())).collect()
}

fn run(layout: &Layout, ids: &[u64], quiet: Duration) -> Vec<Status> {
    let never = AtomicBool::new(false);
    download_with(layout, &wanted(ids), &never, quiet, 0, &mut |_, _| {})
        .into_iter()
        .map(|o| o.status)
        .collect()
}

fn alive(pid: &str) -> bool {
    Path::new("/proc").join(pid.trim()).exists()
}

#[test]
fn each_mod_gets_its_own_outcome_and_the_wrong_mod_is_thrown_away() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let layout = layout_with_fake(tmp.path());
    let out = run(&layout, &[111, 222, 333, 444], Duration::from_secs(30));
    assert!(matches!(out[0], Status::Done(_)), "{:?}", out[0]);
    assert!(
        matches!(&out[1], Status::Failed(why) if why.contains("no such mod")),
        "{:?}",
        out[1]
    );
    assert!(
        matches!(&out[2], Status::Mismatch { expected, found }
            if expected == "good.mod" && found == "evil.other"),
        "{:?}",
        out[2]
    );
    assert!(
        matches!(&out[3], Status::Failed(why) if why.contains("no answer")),
        "{:?}",
        out[3]
    );
    let content = layout.content_dir();
    assert!(content.join("111/About/About.xml").is_file());
    assert!(!content.join("333").exists(), "the wrong mod was kept");
}

#[test]
fn steamcmd_gets_one_command_with_its_own_home_and_no_xdg_folder() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let layout = layout_with_fake(tmp.path());
    run(&layout, &[111, 777], Duration::from_secs(30));
    let args = std::fs::read_to_string(layout.root().join("args.txt")).expect("args");
    let args: Vec<&str> = args.lines().collect();
    let install = layout.install_dir().to_string_lossy().into_owned();
    assert_eq!(
        args,
        [
            "+force_install_dir",
            install.as_str(),
            "+login",
            "anonymous",
            "+workshop_download_item",
            "294100",
            "111",
            "+workshop_download_item",
            "294100",
            "777",
            "+quit"
        ]
    );
    let env = std::fs::read_to_string(layout.root().join("env.txt")).expect("env");
    let home = layout.home().to_string_lossy().into_owned();
    assert_eq!(env.lines().collect::<Vec<_>>(), [home.as_str(), "unset"]);
}

#[test]
fn many_ids_go_in_batches_of_a_hundred() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let layout = layout_with_fake(tmp.path());
    let ids: Vec<u64> = (1000..1250).collect();
    let out = run(&layout, &ids, Duration::from_secs(60));
    assert_eq!(out.len(), 250);
    assert!(out.iter().all(|s| matches!(s, Status::Done(_))), "{out:?}");
    let runs = std::fs::read_to_string(layout.root().join("runs.txt")).expect("runs");
    assert_eq!(runs.lines().count(), 3);
}

#[test]
fn missing_32_bit_libraries_are_explained_in_plain_words() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let layout = layout_with_fake(tmp.path());
    let out = run(&layout, &[555], Duration::from_secs(30));
    assert!(
        matches!(&out[0], Status::Failed(why) if why.contains("32-bit")),
        "{out:?}"
    );
}

#[test]
fn a_missing_program_says_so() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let layout = Layout::new(tmp.path());
    let out = run(&layout, &[111], Duration::from_secs(5));
    assert!(
        matches!(&out[0], Status::Failed(why) if why.contains("could not be started")),
        "{out:?}"
    );
}

#[test]
fn cancel_stops_steamcmd_and_the_program_it_started() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let layout = layout_with_fake(tmp.path());
    let cancel = AtomicBool::new(false);
    let began = Instant::now();
    let out = std::thread::scope(|scope| {
        scope.spawn(|| {
            std::thread::sleep(Duration::from_millis(700));
            cancel.store(true, Ordering::Relaxed);
        });
        download_with(
            &layout,
            &wanted(&[666]),
            &cancel,
            Duration::from_secs(60),
            0,
            &mut |_, _| {},
        )
    });
    assert!(began.elapsed() < Duration::from_secs(20), "cancel was slow");
    assert!(
        matches!(&out[0].status, Status::Failed(why) if why == "cancelled"),
        "{out:?}"
    );
    let pid = std::fs::read_to_string(layout.root().join("child.pid")).expect("pid");
    // Given a moment to be reaped.
    let gone = (0..50).any(|_| {
        std::thread::sleep(Duration::from_millis(100));
        !alive(&pid)
    });
    assert!(
        gone,
        "the program steamcmd started is still running (pid {pid})"
    );
}

#[test]
fn silence_ends_it_the_same_way() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let layout = layout_with_fake(tmp.path());
    let out = run(&layout, &[666], Duration::from_millis(800));
    assert!(
        matches!(&out[0], Status::Failed(why) if why.contains("stopped answering")),
        "{out:?}"
    );
    let pid = std::fs::read_to_string(layout.root().join("child.pid")).expect("pid");
    let gone = (0..50).any(|_| {
        std::thread::sleep(Duration::from_millis(100));
        !alive(&pid)
    });
    assert!(
        gone,
        "the program steamcmd started is still running (pid {pid})"
    );
}

/// Needs the network and about two minutes (steamcmd updates itself on first run):
/// `cargo test -p rwpm-core --test steamcmd -- --ignored real_steamcmd`. The whole route
/// against Valve: download steamcmd, check and unpack it, fetch Work Tab, check its identity.
#[test]
#[ignore = "downloads from Valve"]
fn real_steamcmd_installs_and_downloads_a_mod() {
    use rwpm_core::consent::{ConsentStore, Scope};
    use rwpm_core::steamcmd::run::ensure_installed;
    let tmp = tempfile::tempdir().expect("tempdir");
    let layout = Layout::new(tmp.path());
    let consent = ConsentStore::open(tmp.path())
        .record(Scope::Steamcmd, layout.root())
        .expect("consent");
    let hash = ensure_installed(&layout, &consent, None).expect("install");
    eprintln!("first-seen checksum {hash:?}");
    assert!(layout.installed());
    let never = AtomicBool::new(false);
    let ask = vec![
        (3_552_152_340, "fluffy.worktab".to_owned()),
        (3_552_152_341, "nobody.nothing".to_owned()),
    ];
    let began = Instant::now();
    let out = download(
        &layout,
        &ask,
        &never,
        Duration::from_secs(300),
        &mut |d, t| {
            eprintln!("{d}/{t} after {:?}", began.elapsed());
        },
    );
    eprintln!("{out:?}");
    assert!(matches!(out[0].status, Status::Done(_)), "{:?}", out[0]);
    assert!(
        layout
            .content_dir()
            .join("3552152340/About/About.xml")
            .is_file()
    );
}

#[test]
fn what_an_earlier_run_left_is_gone_before_this_run_delivers() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let layout = layout_with_fake(tmp.path());
    let stale = layout.content_dir().join("999/About");
    std::fs::create_dir_all(&stale).expect("stale");
    std::fs::write(stale.join("About.xml"), "old").expect("old");
    run(&layout, &[111], Duration::from_secs(30));
    assert!(
        !layout.content_dir().join("999").exists(),
        "a leftover stayed"
    );
    assert!(layout.content_dir().join("111").is_dir());
}

#[test]
fn too_little_free_space_stops_before_steamcmd_starts() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let layout = layout_with_fake(tmp.path());
    let never = AtomicBool::new(false);
    let out = download_with(
        &layout,
        &wanted(&[111, 112]),
        &never,
        Duration::from_secs(30),
        u64::MAX,
        &mut |_, _| {},
    );
    assert_eq!(out.len(), 2);
    assert!(
        out.iter().all(|o| matches!(&o.status, Status::Failed(why) if why.contains("not enough free disk space"))),
        "{out:?}"
    );
    assert!(
        !layout.root().join("runs.txt").exists(),
        "steamcmd was started anyway"
    );
    // With room enough, the same call runs.
    let out = download_with(
        &layout,
        &wanted(&[111]),
        &never,
        Duration::from_secs(30),
        0,
        &mut |_, _| {},
    );
    assert!(matches!(out[0].status, Status::Done(_)), "{out:?}");
}
