use std::process::Command;

fn rwpm() -> Command {
    Command::new(env!("CARGO_BIN_EXE_rwpm"))
}

#[test]
fn version_prints_crate_version() {
    let out = rwpm().arg("--version").output().expect("spawn rwpm");
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).expect("utf-8 stdout");
    assert_eq!(stdout.trim(), format!("rwpm {}", env!("CARGO_PKG_VERSION")));
}

/// A word that is not an existing `.rimrim` or `.rimpack` file is still an unknown
/// command, so a typo never opens the window.
#[test]
fn a_missing_backup_or_pack_or_other_file_is_an_unknown_command() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let notes = tmp.path().join("notes.txt");
    std::fs::write(&notes, "words").expect("write");
    for arg in [
        tmp.path().join("missing.rimrim"),
        tmp.path().join("missing.rimpack"),
        notes,
    ] {
        let out = rwpm().arg(&arg).output().expect("spawn rwpm");
        assert_eq!(out.status.code(), Some(2), "{}", arg.display());
        let stderr = String::from_utf8(out.stderr).expect("utf-8 stderr");
        assert!(stderr.contains("unknown command"), "{stderr}");
    }
}
