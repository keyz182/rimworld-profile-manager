use std::collections::BTreeMap;
use std::io::{Cursor, Read, Write};
use std::net::TcpListener;

use sha2::{Digest, Sha256};

use super::*;
use crate::update_check::{Asset, RELEASES_PREFIX};

const NO_WAIT: Duration = Duration::ZERO;
const VERSION: &str = "1.18.0";

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn on_x86_64() -> bool {
    std::env::consts::ARCH == "x86_64"
}

/// Serves each `(path, body)` for as long as the test runs. Returns the base address.
fn serve(routes: Vec<(String, Vec<u8>)>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut request = [0_u8; 2048];
            let n = stream.read(&mut request).unwrap_or(0);
            let text = String::from_utf8_lossy(&request[..n]).into_owned();
            let path = text.split_whitespace().nth(1).unwrap_or("");
            let (status, body) = routes
                .iter()
                .find(|(p, _)| p == path)
                .map_or(("404 Not Found", &[][..]), |(_, b)| {
                    ("200 OK", b.as_slice())
                });
            let head = format!(
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(body);
        }
    });
    format!("http://127.0.0.1:{port}")
}

fn release_at(
    base: &str,
    files: &[(&str, &[u8])],
    sums: &str,
) -> (Release, Vec<(String, Vec<u8>)>) {
    let mut routes = vec![("/SHA256SUMS".to_string(), sums.as_bytes().to_vec())];
    let mut assets = vec![Asset {
        name: SUMS_NAME.into(),
        url: format!("{base}/SHA256SUMS"),
    }];
    for (name, body) in files {
        routes.push((format!("/{name}"), body.to_vec()));
        assets.push(Asset {
            name: (*name).into(),
            url: format!("{base}/{name}"),
        });
    }
    let release = Release {
        version: VERSION.parse().expect("version"),
        url: format!("{RELEASES_PREFIX}tag/v{VERSION}"),
        assets,
    };
    (release, routes)
}

fn appimage_name() -> String {
    format!("rwpm-{VERSION}-x86_64.AppImage")
}

// ---- checksums ----

#[test]
fn the_checksum_for_a_name_is_found_in_sha256sum_output() {
    let a = "a".repeat(64);
    let b = "B".repeat(64);
    let sums = format!("{a}  one.zip\r\n{b} *two.zip\n");
    assert_eq!(checksum_for(&sums, "one.zip"), Some(a));
    assert_eq!(checksum_for(&sums, "two.zip"), Some("b".repeat(64)));
    assert_eq!(checksum_for(&sums, "three.zip"), None);
}

#[test]
fn a_line_that_is_not_a_full_hash_is_ignored() {
    let short = format!("{}  x.zip\n", "a".repeat(63));
    let not_hex = format!("{}  x.zip\n", "g".repeat(64));
    assert_eq!(checksum_for(&short, "x.zip"), None);
    assert_eq!(checksum_for(&not_hex, "x.zip"), None);
    let hash = "c".repeat(64);
    assert_eq!(checksum_for(&format!("{hash}  x.zip.sig\n"), "x.zip"), None);
}

// ---- download ----

#[test]
fn a_download_that_matches_its_checksum_is_staged() {
    if !on_x86_64() {
        return;
    }
    let body = b"new appimage".to_vec();
    let name = appimage_name();
    let base_sums = format!("{}  {name}\n", sha256(&body));
    let tmp = tempfile::tempdir().expect("tempdir");
    let kind = InstallKind::AppImage(tmp.path().join("rwpm.AppImage"));
    let (mut release, routes) = release_at("http://x", &[(&name, &body)], &base_sums);
    let base = serve(routes);
    for asset in &mut release.assets {
        asset.url = asset.url.replace("http://x", &base);
    }
    let staged = fetch(&release, &kind, Os::Linux, false).expect("fetch");
    assert_eq!(fs::read(&staged.file).expect("read"), body);
    assert_eq!(staged.version().to_string(), VERSION);
}

#[test]
fn a_download_that_does_not_match_is_deleted_and_refused() {
    if !on_x86_64() {
        return;
    }
    let name = appimage_name();
    let sums = format!("{}  {name}\n", sha256(b"what the release promised"));
    let tmp = tempfile::tempdir().expect("tempdir");
    let target = tmp.path().join("rwpm.AppImage");
    let kind = InstallKind::AppImage(target.clone());
    let (mut release, routes) = release_at("http://x", &[(&name, b"something else")], &sums);
    let base = serve(routes);
    for asset in &mut release.assets {
        asset.url = asset.url.replace("http://x", &base);
    }
    let err = fetch(&release, &kind, Os::Linux, false).expect_err("mismatch");
    assert!(matches!(err, Error::Mismatch { .. }), "{err}");
    assert!(
        !sibling(&target, NEW_SUFFIX).exists(),
        "unchecked file left behind"
    );
}

#[test]
fn a_release_without_checksums_or_its_file_cannot_be_installed() {
    if !on_x86_64() {
        return;
    }
    let kind = InstallKind::AppImage("/x/rwpm.AppImage".into());
    let name = appimage_name();
    let url = |n: &str| format!("{DOWNLOAD_PREFIX}v{VERSION}/{n}");
    let asset = |n: &str| Asset {
        name: n.into(),
        url: url(n),
    };
    let release = |assets| Release {
        version: VERSION.parse().expect("version"),
        url: format!("{RELEASES_PREFIX}tag/v{VERSION}"),
        assets,
    };
    let no_sums = release(vec![asset(&name)]);
    let no_file = release(vec![asset(SUMS_NAME)]);
    let both = release(vec![asset(&name), asset(SUMS_NAME)]);
    assert!(!can_install(&no_sums, &kind));
    assert!(!can_install(&no_file, &kind));
    assert!(can_install(&both, &kind));
    assert!(!can_install(&both, &InstallKind::Managed));
    assert!(matches!(
        fetch(&no_sums, &kind, Os::Linux, false),
        Err(Error::NoAsset(n)) if n == SUMS_NAME
    ));
    assert!(matches!(
        fetch(&no_file, &kind, Os::Linux, false),
        Err(Error::NoAsset(n)) if n == name
    ));
}

#[test]
fn a_package_managed_install_never_downloads() {
    let release = Release {
        version: VERSION.parse().expect("version"),
        url: format!("{RELEASES_PREFIX}tag/v{VERSION}"),
        assets: Vec::new(),
    };
    assert!(matches!(
        download(&release, &InstallKind::Managed),
        Err(Error::Unsupported)
    ));
}

#[test]
fn a_download_address_outside_this_project_is_refused_before_any_request() {
    let release = Release {
        version: VERSION.parse().expect("version"),
        url: format!("{RELEASES_PREFIX}tag/v{VERSION}"),
        assets: vec![Asset {
            name: appimage_name(),
            url: "http://127.0.0.1:9/evil".into(),
        }],
    };
    let kind = InstallKind::AppImage("/x/rwpm.AppImage".into());
    assert!(matches!(download(&release, &kind), Err(Error::BadUrl(_))));
}

// ---- archives ----

fn tar_gz(root: &str, files: &[(&str, &[u8], u32)]) -> Vec<u8> {
    let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(
        Vec::new(),
        flate2::Compression::fast(),
    ));
    let mut dir = tar::Header::new_gnu();
    dir.set_entry_type(tar::EntryType::Directory);
    dir.set_mode(0o755);
    dir.set_size(0);
    builder
        .append_data(&mut dir, format!("{root}/"), std::io::empty())
        .expect("root");
    for (path, body, mode) in files {
        let mut header = tar::Header::new_gnu();
        header.set_size(body.len() as u64);
        header.set_mode(*mode);
        builder
            .append_data(&mut header, format!("{root}/{path}"), *body)
            .expect("entry");
    }
    builder.into_inner().expect("tar").finish().expect("gz")
}

fn zip_of(root: &str, files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    for (path, body) in files {
        zip.start_file(format!("{root}/{path}"), options)
            .expect("start");
        zip.write_all(body).expect("write");
    }
    zip.finish().expect("zip").into_inner()
}

/// Every file under `dir` and what it holds.
fn snapshot(dir: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    list_files(dir)
        .expect("list")
        .into_iter()
        .map(|rel| {
            let body = fs::read(dir.join(&rel)).expect("read");
            (rel, body)
        })
        .collect()
}

/// A portable folder holding an old `rwpm`, two libraries and a file the player added.
fn old_install(dir: &Path) {
    fs::create_dir_all(dir.join("payload")).expect("payload");
    fs::write(dir.join("rwpm"), b"old exe").expect("exe");
    fs::write(dir.join("payload/a.dll"), b"old a").expect("a");
    fs::write(dir.join("payload/b.dll"), b"old b").expect("b");
    fs::write(dir.join("notes.txt"), b"mine").expect("notes");
}

fn archive_in(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(STAGE_DIR).join(name);
    fs::create_dir_all(path.parent().expect("parent")).expect("stage");
    fs::write(&path, bytes).expect("archive");
    path
}

const NEW_FILES: [(&str, &[u8], u32); 4] = [
    ("rwpm", b"new exe", 0o755),
    ("payload/a.dll", b"new a", 0o644),
    ("payload/b.dll", b"new b", 0o644),
    ("licenses/new.txt", b"added", 0o644),
];

#[test]
fn a_tar_gz_replaces_the_files_strips_the_root_and_keeps_the_players_own() {
    let tmp = tempfile::tempdir().expect("tempdir");
    old_install(tmp.path());
    let bytes = tar_gz("rwpm-1.18.0-linux-x86_64", &NEW_FILES);
    let archive = archive_in(tmp.path(), "rwpm-1.18.0-linux-x86_64.tar.gz", &bytes);

    let handoff = apply_portable(
        &archive,
        tmp.path(),
        OsStr::new("rwpm"),
        &|a, b| fs::rename(a, b),
        NO_WAIT,
    )
    .expect("apply");

    assert_eq!(handoff.program, tmp.path().join("rwpm"));
    let after = snapshot(tmp.path());
    let want: BTreeMap<PathBuf, Vec<u8>> = [
        ("rwpm", &b"new exe"[..]),
        ("payload/a.dll", b"new a"),
        ("payload/b.dll", b"new b"),
        ("licenses/new.txt", b"added"),
        ("notes.txt", b"mine"),
    ]
    .into_iter()
    .map(|(p, b)| (PathBuf::from(p), b.to_vec()))
    .collect();
    assert_eq!(
        after, want,
        "no .old files, no stage folder, no extra root folder"
    );
    assert!(!tmp.path().join(STAGE_DIR).exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(tmp.path().join("rwpm"))
            .expect("meta")
            .permissions()
            .mode();
        assert_eq!(mode & 0o111, 0o111, "the new program is still executable");
    }
}

#[test]
fn a_zip_installs_the_same_way() {
    let tmp = tempfile::tempdir().expect("tempdir");
    old_install(tmp.path());
    let files: Vec<(&str, &[u8])> = NEW_FILES.iter().map(|(p, b, _)| (*p, *b)).collect();
    let bytes = zip_of("rwpm-1.18.0-windows-x86_64", &files);
    let archive = archive_in(tmp.path(), "rwpm-1.18.0-windows-x86_64.zip", &bytes);

    apply_portable(
        &archive,
        tmp.path(),
        OsStr::new("rwpm"),
        &|a, b| fs::rename(a, b),
        NO_WAIT,
    )
    .expect("apply");

    let after = snapshot(tmp.path());
    assert_eq!(
        after.get(Path::new("rwpm")).map(Vec::as_slice),
        Some(&b"new exe"[..])
    );
    assert_eq!(
        after.get(Path::new("licenses/new.txt")).map(Vec::as_slice),
        Some(&b"added"[..])
    );
    assert!(after.contains_key(Path::new("notes.txt")));
    assert!(
        after
            .keys()
            .all(|p| p.extension().is_none_or(|e| e != "old"))
    );
}

#[test]
fn a_failed_move_puts_every_file_back() {
    let tmp = tempfile::tempdir().expect("tempdir");
    old_install(tmp.path());
    let before = snapshot(tmp.path());
    let bytes = tar_gz("rwpm-1.18.0-linux-x86_64", &NEW_FILES);
    let archive = archive_in(tmp.path(), "rwpm-1.18.0-linux-x86_64.tar.gz", &bytes);
    let program = tmp.path().join("rwpm");
    // Placing the new program is the last step; everything before it has to be undone.
    let flaky = |from: &Path, to: &Path| {
        if to == program {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "held by antivirus",
            ))
        } else {
            fs::rename(from, to)
        }
    };

    let err = apply_portable(&archive, tmp.path(), OsStr::new("rwpm"), &flaky, NO_WAIT)
        .expect_err("must fail");

    assert!(matches!(err, Error::RolledBack { .. }), "{err}");
    assert_eq!(
        snapshot(tmp.path()),
        before,
        "folder is byte-for-byte as before"
    );
    assert!(
        !tmp.path().join("licenses").exists(),
        "created folder removed"
    );
    assert!(!tmp.path().join(STAGE_DIR).exists());
}

#[test]
fn a_rename_that_fails_once_is_tried_again() {
    let tmp = tempfile::tempdir().expect("tempdir");
    old_install(tmp.path());
    let bytes = tar_gz("rwpm-1.18.0-linux-x86_64", &NEW_FILES);
    let archive = archive_in(tmp.path(), "rwpm-1.18.0-linux-x86_64.tar.gz", &bytes);
    let failed = std::cell::Cell::new(false);
    let once = |from: &Path, to: &Path| {
        if !failed.replace(true) {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "briefly held",
            ))
        } else {
            fs::rename(from, to)
        }
    };
    apply_portable(&archive, tmp.path(), OsStr::new("rwpm"), &once, NO_WAIT).expect("apply");
    assert_eq!(fs::read(tmp.path().join("rwpm")).expect("read"), b"new exe");
}

#[test]
fn an_archive_without_the_program_changes_nothing() {
    let tmp = tempfile::tempdir().expect("tempdir");
    old_install(tmp.path());
    let before = snapshot(tmp.path());
    let bytes = tar_gz(
        "rwpm-1.18.0-linux-x86_64",
        &[("payload/a.dll", b"x", 0o644)],
    );
    let archive = archive_in(tmp.path(), "rwpm-1.18.0-linux-x86_64.tar.gz", &bytes);
    let err = apply_portable(
        &archive,
        tmp.path(),
        OsStr::new("rwpm"),
        &|a, b| fs::rename(a, b),
        NO_WAIT,
    )
    .expect_err("no program");
    assert!(matches!(err, Error::Archive(_)), "{err}");
    assert_eq!(snapshot(tmp.path()), before);
}

#[test]
fn an_entry_that_climbs_out_of_the_folder_is_refused() {
    let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(
        Vec::new(),
        flate2::Compression::fast(),
    ));
    let mut header = tar::Header::new_gnu();
    header.set_size(4);
    header.set_mode(0o644);
    let name = b"rwpm-1.18.0/../evil";
    header.as_gnu_mut().expect("gnu").name[..name.len()].copy_from_slice(name);
    header.set_cksum();
    builder.append(&header, &b"evil"[..]).expect("append");
    let bytes = builder.into_inner().expect("tar").finish().expect("gz");

    let tmp = tempfile::tempdir().expect("tempdir");
    let install = tmp.path().join("install");
    old_install(&install);
    let archive = archive_in(&install, "x.tar.gz", &bytes);
    let err = apply_portable(
        &archive,
        &install,
        OsStr::new("rwpm"),
        &|a, b| fs::rename(a, b),
        NO_WAIT,
    )
    .expect_err("unsafe");
    assert!(matches!(err, Error::Archive(m) if m.contains("unsafe")));
    assert!(!tmp.path().join("evil").exists());
    assert!(!install.join("evil").exists());
}

#[test]
fn inner_paths_drop_the_top_folder_and_reject_escapes() {
    assert_eq!(inner_path(Path::new("root/")).expect("ok"), None);
    assert_eq!(inner_path(Path::new("root")).expect("ok"), None);
    assert_eq!(
        inner_path(Path::new("root/a/b.txt")).expect("ok"),
        Some(PathBuf::from("a/b.txt"))
    );
    assert!(inner_path(Path::new("../x")).is_err());
    assert!(inner_path(Path::new("root/../../x")).is_err());
    assert!(inner_path(Path::new("/abs/x")).is_err());
}

#[test]
fn an_archive_that_unpacks_past_the_limit_is_refused() {
    let mut budget = 10;
    let mut sink = Vec::new();
    let err = copy_within(&mut &[0_u8; 11][..], &mut sink, &mut budget).expect_err("too much");
    assert!(matches!(err, Error::Archive(_)));
    let mut budget = 10;
    copy_within(&mut &[0_u8; 10][..], &mut sink, &mut budget).expect("exactly the limit");
    assert_eq!(budget, 0);
}

// ---- other install kinds ----

#[cfg(unix)]
#[test]
fn an_appimage_is_swapped_in_place_and_made_executable() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().expect("tempdir");
    let target = tmp.path().join("rwpm.AppImage");
    fs::write(&target, b"old").expect("old");
    let staged_file = sibling(&target, NEW_SUFFIX);
    fs::write(&staged_file, b"new").expect("new");
    let staged = Staged {
        kind: InstallKind::AppImage(target.clone()),
        version: VERSION.parse().expect("version"),
        file: staged_file.clone(),
        exe_name: "rwpm".into(),
    };
    let handoff = apply(&staged).expect("apply");
    assert_eq!(handoff.program, target);
    assert!(handoff.args.is_empty());
    assert_eq!(fs::read(&target).expect("read"), b"new");
    assert!(!staged_file.exists());
    assert_eq!(
        fs::metadata(&target).expect("meta").permissions().mode() & 0o777,
        0o755
    );
}

#[test]
fn the_installer_is_started_silently_and_asked_to_relaunch() {
    let staged = Staged {
        kind: InstallKind::WindowsInstaller("C:/rwpm".into()),
        version: VERSION.parse().expect("version"),
        file: "C:/Temp/setup.exe".into(),
        exe_name: "rwpm.exe".into(),
    };
    let handoff = apply(&staged).expect("apply");
    assert_eq!(handoff.program, PathBuf::from("C:/Temp/setup.exe"));
    assert_eq!(
        handoff.args,
        [OsString::from("/S"), OsString::from("/RELAUNCH")]
    );
}

#[test]
fn kinds_that_do_not_self_update_refuse_to_apply() {
    for kind in [InstallKind::Managed, InstallKind::MacOs] {
        let staged = Staged {
            kind,
            version: VERSION.parse().expect("version"),
            file: "/x".into(),
            exe_name: "rwpm".into(),
        };
        assert!(matches!(apply(&staged), Err(Error::Unsupported)));
    }
}

// ---- cleanup ----

#[test]
fn cleanup_removes_only_what_an_update_left() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path();
    old_install(dir);
    fs::create_dir_all(dir.join(STAGE_DIR)).expect("stage");
    fs::write(dir.join(STAGE_DIR).join("half.tar.gz"), b"x").expect("half");
    fs::write(dir.join("rwpm.old"), b"replaced").expect("old");
    fs::write(dir.join("payload/a.dll.old"), b"replaced").expect("old");
    fs::write(dir.join("orphan.old"), b"not ours").expect("orphan");

    cleanup(&InstallKind::Portable(dir.to_path_buf()));

    assert!(!dir.join(STAGE_DIR).exists());
    assert!(!dir.join("rwpm.old").exists());
    assert!(!dir.join("payload/a.dll.old").exists());
    assert!(
        dir.join("orphan.old").exists(),
        "a .old file with no partner is not ours"
    );
    assert!(dir.join("rwpm").exists() && dir.join("notes.txt").exists());
}

#[test]
fn cleanup_drops_an_unused_appimage_download() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let target = tmp.path().join("rwpm.AppImage");
    fs::write(&target, b"current").expect("current");
    fs::write(sibling(&target, NEW_SUFFIX), b"unused").expect("unused");
    cleanup(&InstallKind::AppImage(target.clone()));
    assert!(target.exists());
    assert!(!sibling(&target, NEW_SUFFIX).exists());
    cleanup(&InstallKind::Managed);
}

#[cfg(unix)]
#[test]
fn the_restarted_app_is_told_it_follows_an_update() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let seen = tmp.path().join("seen");
    spawn(&Handoff {
        program: "sh".into(),
        args: vec![
            "-c".into(),
            format!("printf %s \"${AFTER_UPDATE_ENV}\" > '{}'", seen.display()).into(),
        ],
    })
    .expect("spawn");
    for _ in 0..250 {
        if fs::read_to_string(&seen).is_ok_and(|s| !s.is_empty()) {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(fs::read_to_string(&seen).expect("seen"), "1");
}
