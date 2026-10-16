//! T75 (V83): `.rimrim` opens in this app on Windows and Linux. The release files cannot be
//! run in CI, so the installer scripts are checked as text, and the Linux one is also run
//! against a scratch home folder.

const NSI: &str = include_str!("../../../packaging/windows/rwpm.nsi");
const MIME_XML: &str = include_str!("../../../packaging/linux/rwpm-mime.xml");
const DESKTOP_INSTALLER: &str = include_str!("../../../packaging/linux/install-desktop-entry.sh");
const PACKAGE_SH: &str = include_str!("../../../scripts/package.sh");
const APPIMAGE_SH: &str = include_str!("../../../scripts/appimage.sh");
const MIME_TYPE: &str = "application/x-rimrim";

#[test]
fn the_mime_type_is_matched_by_the_file_name() {
    assert!(MIME_XML.contains(&format!("type=\"{MIME_TYPE}\"")));
    assert!(MIME_XML.contains("<glob pattern=\"*.rimrim\"/>"));
}

#[test]
fn every_linux_desktop_entry_names_the_type_and_takes_a_file() {
    for (name, script) in [
        ("install-desktop-entry.sh", DESKTOP_INSTALLER),
        ("appimage.sh", APPIMAGE_SH),
    ] {
        assert!(script.contains(&format!("MimeType={MIME_TYPE};")), "{name}");
        assert!(
            script
                .lines()
                .any(|l| l.starts_with("Exec=") && l.ends_with(" %f")),
            "{name}: Exec must end with %f so the file reaches rwpm"
        );
    }
}

#[test]
fn the_bundles_carry_the_type_definition() {
    assert!(PACKAGE_SH.contains("packaging/linux/rwpm-mime.xml"));
    assert!(APPIMAGE_SH.contains("usr/share/mime/packages/rwpm.xml"));
    assert!(DESKTOP_INSTALLER.contains("rwpm-mime.xml"));
}

#[test]
fn the_windows_installer_registers_and_removes_only_its_own_association() {
    for line in [
        r#"WriteRegStr HKCU "${CLASSES}\.rimrim" "" "${BACKUP_CLASS}""#,
        r#"WriteRegStr HKCU "${CLASSES}\${BACKUP_CLASS}\shell\open\command" "" '"$INSTDIR\${EXE}" "%1"'"#,
        r#"WriteRegStr HKCU "${CLASSES}\${BACKUP_CLASS}\DefaultIcon" "" "$INSTDIR\${EXE},0""#,
        r#"DeleteRegKey HKCU "${CLASSES}\${BACKUP_CLASS}""#,
    ] {
        assert!(NSI.contains(line), "rwpm.nsi lacks: {line}");
    }
    // Removal of `.rimrim` itself is guarded by who owns it.
    let guard = NSI
        .find(r#"StrCmp $0 "${BACKUP_CLASS}" 0 +2"#)
        .expect("ownership check");
    let delete = NSI
        .find(r#"DeleteRegKey HKCU "${CLASSES}\.rimrim""#)
        .expect("delete");
    assert!(guard < delete, "check who owns .rimrim before deleting it");
    assert!(
        NSI.contains(r#"!define CLASSES "Software\Classes""#),
        "per user, never HKLM"
    );
    assert!(!NSI.contains("HKLM"), "the installer is per user");
}

/// Runs the real script against a scratch `$HOME`: install, then `--remove`.
#[cfg(target_os = "linux")]
#[test]
fn the_linux_installer_adds_and_removes_the_entry_and_the_type() {
    use std::path::Path;
    use std::process::Command;

    let tmp = tempfile::tempdir().expect("tempdir");
    let bundle = tmp.path().join("bundle");
    let icons = bundle.join("icons");
    std::fs::create_dir_all(&icons).expect("icons dir");
    let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/icon");
    for n in [16, 32, 48, 64, 128, 256] {
        let file = format!("rwpm-{n}.png");
        std::fs::copy(assets.join(&file), icons.join(&file)).expect("icon");
    }
    std::fs::copy(assets.join("rwpm.svg"), icons.join("rwpm.svg")).expect("svg");
    let packaging = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packaging/linux");
    for file in ["install-desktop-entry.sh", "rwpm-mime.xml"] {
        std::fs::copy(packaging.join(file), bundle.join(file)).expect("copy");
    }
    let (home, data) = (tmp.path().join("home"), tmp.path().join("data"));
    let run = |args: &[&str]| {
        let out = Command::new("bash")
            .arg(bundle.join("install-desktop-entry.sh"))
            .args(args)
            .env("HOME", &home)
            .env("XDG_DATA_HOME", &data)
            .output()
            .expect("run script");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    };
    let entry = data.join("applications/rwpm.desktop");
    let mime = data.join("mime/packages/rwpm.xml");

    run(&[]);
    let text = std::fs::read_to_string(&entry).expect("entry");
    assert!(text.contains(&format!("MimeType={MIME_TYPE};")), "{text}");
    let exec = text.lines().find(|l| l.starts_with("Exec=")).expect("Exec");
    assert!(
        exec.contains(bundle.to_str().expect("utf-8")) && exec.ends_with("/rwpm\" %f"),
        "{exec}"
    );
    assert_eq!(
        std::fs::read_to_string(&mime).expect("mime"),
        MIME_XML,
        "the installed type is the shipped one"
    );
    assert!(data.join("icons/hicolor/256x256/apps/rwpm.png").is_file());

    run(&["--remove"]);
    assert!(!entry.exists() && !mime.exists());
    assert!(!data.join("icons/hicolor/256x256/apps/rwpm.png").exists());
    run(&["--remove"]); // nothing left: still fine
}
