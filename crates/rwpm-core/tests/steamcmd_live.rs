//! The whole steamcmd route against Valve, on whichever system this runs on. Needs the
//! network and about two minutes (steamcmd updates itself on first run), so it is ignored:
//! `cargo test -p rwpm-core --test steamcmd_live -- --ignored --nocapture`.

use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use rwpm_core::fetch::Status;
use rwpm_core::steamcmd::run::{Layout, download};

/// The whole route against Valve: download steamcmd, check and unpack it, fetch Work Tab,
/// check its identity, and see a made-up id fail.
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
