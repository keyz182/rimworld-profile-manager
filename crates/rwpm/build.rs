//! Windows: embeds the app icon (assets/icon/rwpm.ico) in rwpm.exe, so Explorer, the
//! Start menu and the taskbar show it. Uses mingw's `windres` (the Windows toolchain
//! this project builds with); no build dependency needed.

use std::path::PathBuf;
use std::process::Command;

fn main() {
    let ico = "assets/icon/rwpm.ico";
    println!("cargo:rerun-if-changed={ico}");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let rc = out.join("rwpm.rc");
    let ico_path = std::fs::canonicalize(ico).expect("rwpm.ico");
    // Resource ID 1 is the icon Windows shows for the .exe.
    let line = format!(
        "1 ICON \"{}\"\n",
        ico_path.display().to_string().replace('\\', "/")
    );
    std::fs::write(&rc, line).expect("write rwpm.rc");
    let res = out.join("rwpm-icon.o");
    let windres = std::env::var("WINDRES").unwrap_or_else(|_| "x86_64-w64-mingw32-windres".into());
    let status = Command::new(&windres)
        .arg(&rc)
        .args(["-O", "coff", "-o"])
        .arg(&res)
        .status()
        .unwrap_or_else(|e| panic!("cannot run `{windres}` (set WINDRES): {e}"));
    assert!(status.success(), "`{windres}` failed");
    println!("cargo:rustc-link-arg-bins={}", res.display());
}
