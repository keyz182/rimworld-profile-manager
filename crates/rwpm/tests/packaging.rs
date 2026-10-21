//! `.rimrim` backups and `.rimpack` modpacks open in this app on Windows and
//! Linux. The release files cannot be run in CI, so the installer scripts are checked as
//! text, and the Linux one is also run against a scratch home folder.

const NSI: &str = include_str!("../../../packaging/windows/rwpm.nsi");
const MIME_XML: &str = include_str!("../../../packaging/linux/rwpm-mime.xml");
const DESKTOP_INSTALLER: &str = include_str!("../../../packaging/linux/install-desktop-entry.sh");
const PACKAGE_SH: &str = include_str!("../../../scripts/package.sh");
const APPIMAGE_SH: &str = include_str!("../../../scripts/appimage.sh");
/// (mime type, file extension) of every file type this app opens.
const TYPES: [(&str, &str); 2] = [
    ("application/x-rimrim", "rimrim"),
    ("application/x-rimpack", "rimpack"),
];
const MIME_LINE: &str = "MimeType=application/x-rimrim;application/x-rimpack;";

#[test]
fn the_mime_types_are_matched_by_the_file_name() {
    for (mime, extension) in TYPES {
        assert!(MIME_XML.contains(&format!("type=\"{mime}\"")), "{mime}");
        assert!(
            MIME_XML.contains(&format!("<glob pattern=\"*.{extension}\"/>")),
            "{extension}"
        );
    }
}

#[test]
fn every_linux_desktop_entry_names_the_type_and_takes_a_file() {
    for (name, script) in [
        ("install-desktop-entry.sh", DESKTOP_INSTALLER),
        ("appimage.sh", APPIMAGE_SH),
    ] {
        assert!(script.contains(MIME_LINE), "{name}");
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
    for (extension, class) in [("rimrim", "BACKUP_CLASS"), ("rimpack", "PACK_CLASS")] {
        for line in [
            format!(r#"WriteRegStr HKCU "${{CLASSES}}\.{extension}" "" "${{{class}}}""#),
            format!(
                r#"WriteRegStr HKCU "${{CLASSES}}\${{{class}}}\shell\open\command" "" '"$INSTDIR\${{EXE}}" "%1"'"#
            ),
            format!(
                r#"WriteRegStr HKCU "${{CLASSES}}\${{{class}}}\DefaultIcon" "" "$INSTDIR\${{EXE}},0""#
            ),
            format!(r#"DeleteRegKey HKCU "${{CLASSES}}\${{{class}}}""#),
        ] {
            assert!(NSI.contains(&line), "rwpm.nsi lacks: {line}");
        }
        // Removal of the extension itself is guarded by who owns it.
        let guard = NSI
            .find(&format!(r#"StrCmp $0 "${{{class}}}" 0 +2"#))
            .expect("ownership check");
        let delete = NSI
            .find(&format!(r#"DeleteRegKey HKCU "${{CLASSES}}\.{extension}""#))
            .expect("delete");
        assert!(
            guard < delete,
            "check who owns .{extension} before deleting it"
        );
    }
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
    assert!(text.contains(MIME_LINE), "{text}");
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

const THIRD_PARTY_PY: &str = include_str!("../../../scripts/third_party.py");
const VALVE_NOTE: &str = include_str!("../../../licenses/Steamworks-redistributable.txt");

/// (helper, Valve library) per bundle, as `package.sh` sets them.
const STEAM_FILES: [(&str, &str); 3] = [
    ("rwpm-steam", "libsteam_api.so"),
    ("rwpm-steam.exe", "steam_api64.dll"),
    ("rwpm-steam", "libsteam_api.dylib"),
];

#[test]
fn every_bundle_builds_and_ships_the_steam_helper_and_valves_library() {
    for (exe, lib) in STEAM_FILES {
        assert!(
            PACKAGE_SH.contains(&format!("steam_exe=\"{exe}\"")),
            "{exe}"
        );
        assert!(
            PACKAGE_SH.contains(&format!("steam_lib=\"{lib}\"")),
            "{lib}"
        );
    }
    assert!(PACKAGE_SH.contains("-p rwpm -p rwpm-steam"));
    assert!(PACKAGE_SH.contains(r#"cp "$release_dir/$exe" "$release_dir/$steam_exe" "$bin_dir/""#));
    assert!(PACKAGE_SH.contains(r#"cp "$steam_src" "$bin_dir/""#));
    // The glibc floor applies to the helper too.
    assert!(
        PACKAGE_SH.contains(r#"check_glibc "$ROOT/target/soldier/$target/release/$steam_exe""#)
    );
}

#[test]
fn the_windows_installer_puts_the_helper_in_and_takes_it_out() {
    for define in [
        r#"!define STEAM_EXE "rwpm-steam.exe""#,
        r#"!define STEAM_DLL "steam_api64.dll""#,
    ] {
        assert!(NSI.contains(define), "{define}");
    }
    for name in ["STEAM_EXE", "STEAM_DLL"] {
        assert!(
            NSI.contains(&format!(r#"!insertmacro CheckFree "$INSTDIR\${{{name}}}""#)),
            "{name} is not checked for use"
        );
        assert!(
            NSI.contains(&format!(r#"Delete "$INSTDIR\${{{name}}}""#)),
            "{name} is not removed on uninstall"
        );
    }
}

#[test]
fn the_licence_files_cover_the_helper_and_name_valves_library() {
    assert!(THIRD_PARTY_PY.contains(r#"SHIPPED_BINARIES = ("rwpm", "rwpm-steam")"#));
    assert!(THIRD_PARTY_PY.contains(r#""steamworks-sys": "Noxime_steamworks-rs""#));
    for lib in ["libsteam_api.so", "libsteam_api.dylib", "steam_api64.dll"] {
        assert!(VALVE_NOTE.contains(lib), "{lib}");
    }
    assert!(VALVE_NOTE.contains("Valve"));
}
