use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::time::Duration;

use super::unpack::install;
use super::{ArchiveKind, Error, MAX_ENTRIES, Seen, check_hash, download};

enum Item<'a> {
    File(&'a str, &'a [u8], u32),
    Dir(&'a str),
    Link(&'a str, &'a str),
    HardLink(&'a str, &'a str),
}

/// A tar.gz with the names written raw, so a hostile name (`..`, `/etc`) can be made.
fn tar_gz(items: &[Item<'_>]) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    for item in items {
        let mut header = tar::Header::new_old();
        let (name, kind, data, mode, link) = match item {
            Item::File(n, d, m) => (*n, tar::EntryType::Regular, *d, *m, None),
            Item::Dir(n) => (*n, tar::EntryType::Directory, &b""[..], 0o755, None),
            Item::Link(n, t) => (*n, tar::EntryType::Symlink, &b""[..], 0o777, Some(*t)),
            Item::HardLink(n, t) => (*n, tar::EntryType::Link, &b""[..], 0o644, Some(*t)),
        };
        header.as_old_mut().name[..name.len()].copy_from_slice(name.as_bytes());
        if let Some(target) = link {
            header.as_old_mut().linkname[..target.len()].copy_from_slice(target.as_bytes());
        }
        header.set_entry_type(kind);
        header.set_mode(mode);
        header.set_size(data.len() as u64);
        header.set_cksum();
        builder.append(&header, data).expect("append");
    }
    let tar = builder.into_inner().expect("tar");
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    gz.write_all(&tar).expect("gzip");
    gz.finish().expect("finish")
}

fn zip_of(items: &[(&str, &[u8], u32)]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, data, mode) in items {
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored)
            .unix_permissions(*mode);
        zip.start_file(*name, options).expect("start");
        zip.write_all(data).expect("write");
    }
    zip.finish().expect("finish").into_inner()
}

fn is_empty(dir: &Path) -> bool {
    !dir.exists() || std::fs::read_dir(dir).expect("read").next().is_none()
}

#[test]
fn the_first_checksum_is_recorded_and_a_change_is_refused() {
    let Seen::First(first) = check_hash(None, b"valve").expect("first") else {
        panic!("nothing was recorded, so this is the first");
    };
    assert_eq!(first.len(), 64);
    assert_eq!(
        check_hash(Some(&first), b"valve").expect("same"),
        Seen::Same
    );
    let err = check_hash(Some(&first), b"other").expect_err("changed");
    assert!(matches!(err, Error::Changed { .. }), "{err}");
    assert!(
        err.to_string()
            .contains("not the one downloaded the first time")
    );
}

#[cfg(unix)]
#[test]
fn a_good_tar_is_unpacked_with_its_exec_bit_and_downward_links() {
    use std::os::unix::fs::PermissionsExt;
    let bytes = tar_gz(&[
        Item::Dir("./Frameworks"),
        Item::File("./steamcmd.sh", b"#!/bin/sh\n", 0o755),
        Item::File("linux32/steamcmd", b"elf", 0o755),
        Item::File("data.txt", b"plain", 0o644),
        Item::Link("./Frameworks/Current", "Versions"),
    ]);
    let tmp = tempfile::tempdir().expect("tempdir");
    install(ArchiveKind::TarGz, &bytes, tmp.path()).expect("install");
    let exec = |p: &str| {
        std::fs::metadata(tmp.path().join(p))
            .expect("meta")
            .permissions()
            .mode()
            & 0o111
    };
    assert_ne!(exec("steamcmd.sh"), 0);
    assert_ne!(exec("linux32/steamcmd"), 0);
    assert_eq!(exec("data.txt"), 0);
    assert_eq!(
        std::fs::read(tmp.path().join("data.txt")).expect("read"),
        b"plain"
    );
    assert_eq!(
        std::fs::read_link(tmp.path().join("Frameworks/Current")).expect("link"),
        Path::new("Versions")
    );
}

#[test]
fn a_hostile_tar_entry_stops_everything_and_leaves_nothing() {
    let hostile: [(&str, Item<'_>); 7] = [
        ("climbs out", Item::File("../evil", b"x", 0o644)),
        ("climbs out later", Item::File("a/../../evil", b"x", 0o644)),
        ("starts at the root", Item::File("/etc/evil", b"x", 0o644)),
        ("odd character", Item::File("a\\b", b"x", 0o644)),
        ("link goes up", Item::Link("l", "../outside")),
        ("link starts at the root", Item::Link("l", "/etc")),
        ("hard link", Item::HardLink("h", "good")),
    ];
    for (why, bad) in hostile {
        // A good file first: it must not be left behind either.
        let bytes = tar_gz(&[Item::File("good", b"ok", 0o644), bad]);
        let tmp = tempfile::tempdir().expect("tempdir");
        let dest = tmp.path().join("steamcmd");
        let err = install(ArchiveKind::TarGz, &bytes, &dest).expect_err(why);
        assert!(matches!(err, Error::Unsafe(_)), "{why}: {err}");
        assert!(is_empty(&dest), "{why}: something was written");
        assert!(!tmp.path().join("evil").exists(), "{why}");
    }
}

#[test]
fn a_damaged_or_overfull_archive_is_refused() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let err = install(ArchiveKind::TarGz, b"not gzip", tmp.path()).expect_err("damaged");
    assert!(matches!(err, Error::Archive(_)), "{err}");
    let err = install(ArchiveKind::Zip, b"not zip", tmp.path()).expect_err("damaged zip");
    assert!(matches!(err, Error::Archive(_)), "{err}");
    let names: Vec<String> = (0..=MAX_ENTRIES).map(|i| format!("f{i}")).collect();
    let items: Vec<Item<'_>> = names.iter().map(|n| Item::File(n, b"", 0o644)).collect();
    let err = install(ArchiveKind::TarGz, &tar_gz(&items), tmp.path()).expect_err("too many");
    assert!(err.to_string().contains("too much"), "{err}");
    assert!(is_empty(tmp.path()));
}

#[test]
fn a_zip_is_unpacked_and_a_hostile_one_is_refused() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let good = zip_of(&[("steamcmd.exe", b"MZ", 0o755)]);
    install(ArchiveKind::Zip, &good, tmp.path()).expect("install");
    assert_eq!(
        std::fs::read(tmp.path().join("steamcmd.exe")).expect("read"),
        b"MZ"
    );

    let dest = tmp.path().join("second");
    let bad = zip_of(&[("good.txt", b"ok", 0o644), ("../evil", b"x", 0o644)]);
    let err = install(ArchiveKind::Zip, &bad, &dest).expect_err("hostile");
    assert!(matches!(err, Error::Unsafe(_)), "{err}");
    assert!(is_empty(&dest), "something was written");
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    zip.add_symlink("l", "target", zip::write::SimpleFileOptions::default())
        .expect("symlink");
    let link = zip.finish().expect("finish").into_inner();
    let err = install(ArchiveKind::Zip, &link, &dest).expect_err("link");
    assert!(matches!(err, Error::Unsafe(_)), "{err}");
}

#[cfg(unix)]
#[test]
fn a_file_is_never_written_through_a_link_that_was_already_there() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dest = tmp.path().join("steamcmd");
    std::fs::create_dir_all(&dest).expect("dest");
    let outside = tmp.path().join("outside.txt");
    std::fs::write(&outside, "mine").expect("outside");
    std::os::unix::fs::symlink(&outside, dest.join("steamcmd.sh")).expect("link");
    let bytes = tar_gz(&[Item::File("steamcmd.sh", b"new", 0o755)]);
    install(ArchiveKind::TarGz, &bytes, &dest).expect("install");
    assert_eq!(std::fs::read_to_string(&outside).expect("read"), "mine");
    assert_eq!(
        std::fs::read(dest.join("steamcmd.sh")).expect("read"),
        b"new"
    );
}

/// Serves `body` once on a local port; the URL is returned.
fn serve(body: Vec<u8>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request);
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(&body);
        }
    });
    format!("http://127.0.0.1:{port}/steamcmd.zip")
}

#[test]
fn the_download_returns_the_bytes_up_to_the_cap() {
    let url = serve(b"valve".to_vec());
    let got = download::fetch(&url, false, 100, Duration::from_secs(10)).expect("fetch");
    assert_eq!(got, b"valve");
}

#[test]
fn a_download_over_the_cap_is_thrown_away() {
    let url = serve(vec![7; 5000]);
    let err = download::fetch(&url, false, 1000, Duration::from_secs(10)).expect_err("cap");
    assert!(matches!(err, Error::TooBig), "{err}");
}

#[test]
fn the_real_download_refuses_a_plain_http_address() {
    let url = serve(b"valve".to_vec());
    let err = download::fetch(&url, true, 100, Duration::from_secs(10)).expect_err("https only");
    assert!(matches!(err, Error::Download(_)), "{err}");
}

#[test]
fn a_server_that_is_not_there_gives_a_plain_error() {
    let port = {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        listener.local_addr().expect("addr").port()
    };
    let url = format!("http://127.0.0.1:{port}/x");
    let err = download::fetch(&url, false, 100, Duration::from_secs(5)).expect_err("refused");
    assert!(
        err.to_string()
            .starts_with("Could not download steamcmd from Valve"),
        "{err}"
    );
}

/// Needs the network: `cargo test -p rwpm-core --lib -- --ignored real_valve`. Checks the pinned
/// URL, TLS, the cap and the unpacking against Valve's real file for this system.
#[test]
#[ignore = "downloads from Valve"]
fn real_valve_bootstrap_downloads_and_unpacks() {
    let bytes = download::bootstrap().expect("download");
    assert!(bytes.len() > 100_000, "{} bytes", bytes.len());
    let Seen::First(hash) = check_hash(None, &bytes).expect("hash") else {
        panic!("first");
    };
    eprintln!("sha256 {hash}, {} bytes", bytes.len());
    let tmp = tempfile::tempdir().expect("tempdir");
    install(super::SOURCE.kind, &bytes, tmp.path()).expect("install");
    let program = if cfg!(windows) {
        "steamcmd.exe"
    } else {
        "steamcmd.sh"
    };
    assert!(tmp.path().join(program).is_file(), "no {program}");
}

/// Valve's macOS archive holds symbolic links, the only one that does (R52). Point
/// `RWPM_TEST_OSX_ARCHIVE` at a copy of `steamcmd_osx.tar.gz` to check it passes.
#[cfg(unix)]
#[test]
#[ignore = "needs a copy of Valve's macOS archive"]
fn real_macos_archive_unpacks_with_its_links() {
    let path = std::env::var("RWPM_TEST_OSX_ARCHIVE").expect("RWPM_TEST_OSX_ARCHIVE");
    let bytes = std::fs::read(path).expect("read archive");
    let tmp = tempfile::tempdir().expect("tempdir");
    install(ArchiveKind::TarGz, &bytes, tmp.path()).expect("install");
    assert!(tmp.path().join("steamcmd.sh").is_file());
    let link = tmp
        .path()
        .join("Frameworks/Breakpad.framework/Versions/Current");
    assert_eq!(std::fs::read_link(link).expect("link"), Path::new("A"));
}

#[test]
fn a_tar_with_folders_is_unpacked_the_same_on_every_system() {
    let bytes = tar_gz(&[
        Item::Dir("./linux32"),
        Item::File("./linux32/steamcmd", b"elf", 0o755),
    ]);
    let tmp = tempfile::tempdir().expect("tempdir");
    install(ArchiveKind::TarGz, &bytes, tmp.path()).expect("install");
    assert_eq!(
        std::fs::read(tmp.path().join("linux32").join("steamcmd")).expect("read"),
        b"elf"
    );
}
