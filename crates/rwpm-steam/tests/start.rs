//! The helper must start from a folder of its own with Valve's library
//! beside it and no library path set, the way it sits in a release bundle. It does not need
//! Steam to be running for this: `check` then says so in a line of its own.

#![cfg(target_os = "linux")]

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const LIBRARY: &str = "libsteam_api.so";

/// The copy of Valve's library that the build leaves under `target/<profile>/build`.
fn built_library() -> PathBuf {
    let exe = Path::new(env!("CARGO_BIN_EXE_rwpm-steam"));
    let build = exe.parent().expect("profile folder").join("build");
    std::fs::read_dir(&build)
        .expect("build folder")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("steamworks-sys-"))
        })
        .map(|p| p.join("out").join(LIBRARY))
        .find(|p| p.is_file())
        .expect("Valve's library in the build folder")
}

/// Runs `rwpm-steam check` from a fresh folder holding the helper, and the library if asked.
fn check_in_empty_folder(with_library: bool) -> Output {
    let tmp = tempfile::tempdir().expect("tempdir");
    let helper = tmp.path().join("rwpm-steam");
    std::fs::copy(env!("CARGO_BIN_EXE_rwpm-steam"), &helper).expect("copy helper");
    if with_library {
        std::fs::copy(built_library(), tmp.path().join(LIBRARY)).expect("copy library");
    }
    Command::new(&helper)
        .arg("check")
        .env_remove("LD_LIBRARY_PATH")
        .current_dir(tmp.path())
        .stdin(Stdio::null())
        .output()
        .expect("run helper")
}

fn loader_failed(out: &Output) -> bool {
    String::from_utf8_lossy(&out.stderr).contains("shared libraries")
}

#[test]
fn the_helper_starts_with_the_library_beside_it() {
    let out = check_in_empty_folder(true);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !loader_failed(&out),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    // Steam running → `ok`, exit 0. Steam not running → `error …`, exit 3.
    match out.status.code() {
        Some(0) => assert_eq!(stdout.trim(), "ok"),
        Some(3) => assert!(stdout.starts_with("error "), "{stdout}"),
        other => panic!("exit {other:?}, stdout {stdout:?}"),
    }
}

/// Without this the test above would prove nothing: the library really is needed.
#[test]
fn without_the_library_the_loader_refuses_to_start_it() {
    let out = check_in_empty_folder(false);
    assert!(
        loader_failed(&out),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!matches!(out.status.code(), Some(0 | 3)));
}
