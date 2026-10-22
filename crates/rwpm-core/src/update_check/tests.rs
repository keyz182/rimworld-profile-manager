use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;

use super::*;

const DAY: u64 = 86_400;
const NOW: u64 = 1_800_000_000;

fn state(frequency: Frequency) -> UpdateState {
    UpdateState {
        schema: SCHEMA,
        frequency,
        ..UpdateState::default()
    }
}

fn release(tag: &str) -> Release {
    Release {
        version: tag.parse().expect("version"),
        url: format!("{RELEASES_PREFIX}tag/v{tag}"),
        assets: Vec::new(),
    }
}

#[test]
fn off_is_never_due() {
    let mut s = state(Frequency::Off);
    assert!(!s.is_due(NOW));
    s.last_checked = Some(0);
    assert!(!s.is_due(NOW));
}

#[test]
fn a_new_frequency_with_no_history_is_due() {
    for f in [Frequency::Daily, Frequency::Weekly, Frequency::Monthly] {
        assert!(state(f).is_due(NOW), "{f:?}");
    }
}

#[test]
fn each_frequency_waits_for_its_interval() {
    for (f, days) in [
        (Frequency::Daily, 1),
        (Frequency::Weekly, 7),
        (Frequency::Monthly, 30),
    ] {
        let mut s = state(f);
        s.last_checked = Some(NOW - days * DAY + 1);
        assert!(!s.is_due(NOW), "{f:?} one second early");
        s.last_checked = Some(NOW - days * DAY);
        assert!(s.is_due(NOW), "{f:?} on time");
    }
}

#[test]
fn a_clock_that_moved_back_counts_as_due() {
    let mut s = state(Frequency::Weekly);
    s.last_checked = Some(NOW + 10 * DAY);
    assert!(s.is_due(NOW));
}

#[test]
fn a_failed_try_waits_before_the_next_one() {
    let mut s = state(Frequency::Daily);
    s.record_attempt(NOW);
    assert!(!s.is_due(NOW + RETRY_SECS - 1));
    assert!(s.is_due(NOW + RETRY_SECS));
    assert_eq!(s.last_checked, None);
}

#[test]
fn a_success_resets_both_stamps() {
    let mut s = state(Frequency::Daily);
    s.record_success(NOW);
    assert_eq!((s.last_checked, s.last_attempt), (Some(NOW), Some(NOW)));
    assert!(!s.is_due(NOW + DAY - 1));
    assert!(s.is_due(NOW + DAY));
}

#[test]
fn versions_compare_by_number_not_by_text() {
    let newest = release("1.10.0");
    assert!(newest.version.is_newer_than("1.9.0"));
    assert!(!newest.version.is_newer_than("1.10.0"));
    assert!(!newest.version.is_newer_than("2.0.0"));
    assert!(!newest.version.is_newer_than("not a version"));
}

#[test]
fn only_plain_triples_are_versions() {
    assert_eq!("v1.2.3".parse::<Version>().expect("v"), Version(1, 2, 3));
    assert_eq!("1.2.3".parse::<Version>().expect("bare"), Version(1, 2, 3));
    for bad in [
        "",
        "v",
        "1.2",
        "1.2.3.4",
        "1.2.3-rc1",
        "1.x.3",
        "1..3",
        "+1.2.3",
    ] {
        assert!(
            matches!(bad.parse::<Version>(), Err(Error::BadTag(_))),
            "{bad:?}"
        );
    }
}

#[test]
fn the_banner_hides_for_a_dismissed_release_but_not_a_newer_one() {
    let mut s = state(Frequency::Daily);
    assert!(s.shows(&release("1.15.0"), "1.14.0"));
    assert!(!s.shows(&release("1.14.0"), "1.14.0"), "not newer");
    s.dismissed = Some("1.15.0".into());
    assert!(!s.shows(&release("1.15.0"), "1.14.0"));
    assert!(s.shows(&release("1.16.0"), "1.14.0"));
}

#[test]
fn the_state_file_round_trips_and_a_missing_one_means_off() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let fresh = UpdateState::load(tmp.path()).expect("load");
    assert_eq!(fresh.frequency, Frequency::Off);
    assert_eq!(fresh.schema, SCHEMA);
    let s = UpdateState {
        schema: SCHEMA,
        frequency: Frequency::Weekly,
        last_checked: Some(NOW),
        last_attempt: Some(NOW + 5),
        dismissed: Some("1.15.0".into()),
        pending: Some("1.16.0".into()),
    };
    s.save(tmp.path()).expect("save");
    let text = std::fs::read_to_string(tmp.path().join(FILE_NAME)).expect("read");
    assert!(text.contains("frequency = \"weekly\""), "{text}");
    assert_eq!(UpdateState::load(tmp.path()).expect("load"), s);
}

#[test]
fn a_newer_schema_or_an_unknown_key_is_refused() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join(FILE_NAME);
    std::fs::write(&path, "schema = 3\nfuture = 1\n").expect("write");
    assert!(matches!(
        UpdateState::load(tmp.path()),
        Err(ProfileError::SchemaTooNew { found: 3, .. })
    ));
    std::fs::write(&path, "schema = 2\nextra = 1\n").expect("write");
    assert!(matches!(
        UpdateState::load(tmp.path()),
        Err(ProfileError::Parse { .. })
    ));
    std::fs::write(&path, "frequency = \"daily\"\n").expect("write");
    assert!(matches!(
        UpdateState::load(tmp.path()),
        Err(ProfileError::Parse { .. })
    ));
}

#[test]
fn a_schema_1_file_still_loads_and_is_saved_as_the_current_schema() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join(FILE_NAME);
    std::fs::write(&path, "schema = 1\nfrequency = \"daily\"\n").expect("write");
    let s = UpdateState::load(tmp.path()).expect("load");
    assert_eq!(s.frequency, Frequency::Daily);
    s.save(tmp.path()).expect("save");
    let text = std::fs::read_to_string(&path).expect("read");
    assert!(text.contains(&format!("schema = {SCHEMA}")), "{text}");
}

#[test]
fn a_hand_over_that_did_not_install_is_reported_once() {
    let mut s = state(Frequency::Daily);
    assert_eq!(s.take_failed_install("1.17.0"), None, "nothing pending");
    s.pending = Some("1.18.0".into());
    assert_eq!(s.take_failed_install("1.17.0").as_deref(), Some("1.18.0"));
    assert_eq!(s.pending, None, "cleared");
    s.pending = Some("1.18.0".into());
    assert_eq!(s.take_failed_install("1.18.0"), None, "installed");
    assert_eq!(s.pending, None);
    s.pending = Some("garbage".into());
    assert_eq!(
        s.take_failed_install("1.17.0"),
        None,
        "unreadable is dropped"
    );
}

fn asset_json(name: &str, url: &str) -> String {
    format!(r#"{{"name":"{name}","browser_download_url":"{url}","size":1}}"#)
}

fn payload_with_assets(assets: &[String]) -> Vec<u8> {
    format!(
        r#"{{"tag_name":"v1.18.0","html_url":"{RELEASES_PREFIX}tag/v1.18.0","assets":[{}]}}"#,
        assets.join(",")
    )
    .into_bytes()
}

#[test]
fn release_assets_are_read_and_found_by_name() {
    let url = format!("{DOWNLOAD_PREFIX}v1.18.0/SHA256SUMS");
    let got = parse(&payload_with_assets(&[asset_json("SHA256SUMS", &url)])).expect("parse");
    assert_eq!(
        got.asset("SHA256SUMS").map(|a| a.url.as_str()),
        Some(url.as_str())
    );
    assert!(got.asset("other").is_none());
    assert!(
        parse(&payload(
            "v1.18.0",
            &format!("{RELEASES_PREFIX}tag/v1.18.0")
        ))
        .expect("no assets key")
        .assets
        .is_empty()
    );
}

#[test]
fn an_asset_outside_this_projects_downloads_is_refused() {
    for url in [
        "https://example.com/rwpm.AppImage",
        "https://github.com/someone-else/rimworld-profile-manager/releases/download/v1/x",
        &format!("{RELEASES_PREFIX}tag/v1.18.0"),
        &format!("{DOWNLOAD_PREFIX}v1 /x"),
    ] {
        assert!(
            matches!(
                parse(&payload_with_assets(&[asset_json("x", url)])),
                Err(Error::BadUrl(_))
            ),
            "{url:?}"
        );
    }
}

fn payload(tag: &str, url: &str) -> Vec<u8> {
    format!(r#"{{"tag_name":"{tag}","html_url":"{url}","name":"ignored","draft":false}}"#)
        .into_bytes()
}

/// Serves `body` once on a local port. Returns the URL and the request the server saw.
fn serve(body: Vec<u8>) -> (String, mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut request = [0_u8; 2048];
            let n = stream.read(&mut request).unwrap_or(0);
            let _ = tx.send(String::from_utf8_lossy(&request[..n]).into_owned());
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(&body);
        }
    });
    (format!("http://127.0.0.1:{port}/latest"), rx)
}

#[test]
fn the_newest_release_is_read_and_the_request_names_this_app() {
    let page = format!("{RELEASES_PREFIX}tag/v1.15.0");
    let (url, seen) = serve(payload("v1.15.0", &page));
    let got = latest(&url, false).expect("latest");
    assert_eq!(got.version, Version(1, 15, 0));
    assert_eq!(got.url, page);
    let request = seen.recv().expect("request").to_ascii_lowercase();
    assert!(
        request.contains(&format!("user-agent: rwpm/{}", crate::VERSION)),
        "{request}"
    );
    assert!(request.contains("application/vnd.github+json"), "{request}");
}

#[test]
fn a_tag_that_is_not_a_version_is_an_error_not_no_update() {
    let page = format!("{RELEASES_PREFIX}tag/nightly");
    let (url, _) = serve(payload("nightly", &page));
    assert!(matches!(latest(&url, false), Err(Error::BadTag(_))));
}

#[test]
fn a_release_page_outside_this_project_is_refused() {
    for page in [
        "https://example.com/releases/tag/v9.9.9",
        "https://github.com/someone-else/rimworld-profile-manager/releases/tag/v9.9.9",
        "file:///etc/passwd",
        &format!("{RELEASES_PREFIX}tag/v9 .9.9"),
        // A JSON escape: it decodes to a real newline inside the address.
        &format!("{RELEASES_PREFIX}tag/v9\\n.9.9"),
    ] {
        let (url, _) = serve(payload("v9.9.9", page));
        assert!(
            matches!(latest(&url, false), Err(Error::BadUrl(_))),
            "{page:?}"
        );
    }
}

#[test]
fn an_answer_that_is_not_json_or_lacks_fields_is_an_error() {
    let (url, _) = serve(b"<html>rate limited</html>".to_vec());
    assert!(matches!(latest(&url, false), Err(Error::Parse(_))));
    let (url, _) = serve(br#"{"tag_name":"v1.0.0"}"#.to_vec());
    assert!(matches!(latest(&url, false), Err(Error::Parse(_))));
}

#[test]
fn an_oversized_answer_is_thrown_away() {
    let (url, _) = serve(vec![b' '; (MAX_BODY_BYTES + 1) as usize]);
    assert_eq!(latest(&url, false), Err(Error::TooBig));
}

#[test]
fn the_real_address_is_https_only() {
    let page = format!("{RELEASES_PREFIX}tag/v1.15.0");
    let (url, _) = serve(payload("v1.15.0", &page));
    assert!(matches!(latest(&url, true), Err(Error::Request(_))));
}

#[test]
fn a_server_that_is_not_there_gives_a_request_error() {
    let port = {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        listener.local_addr().expect("addr").port()
    };
    let url = format!("http://127.0.0.1:{port}/latest");
    assert!(matches!(latest(&url, false), Err(Error::Request(_))));
}

#[test]
fn the_built_in_addresses_point_at_this_project() {
    assert!(
        LATEST_URL.starts_with("https://api.github.com/repos/keyz182/rimworld-profile-manager/")
    );
    assert!(RELEASES_PREFIX.ends_with("/releases/"));
}
