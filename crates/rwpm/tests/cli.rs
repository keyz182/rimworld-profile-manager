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
