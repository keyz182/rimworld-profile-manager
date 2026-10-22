use std::sync::Barrier;
use std::sync::mpsc::{Receiver, channel};

use super::*;

const SOON: Duration = Duration::from_secs(5);

fn name_in(dir: &Path) -> Name<'static> {
    dir.join("t.sock")
        .to_fs_name::<GenericFilePath>()
        .expect("name")
}

fn pack_request(dir: &Path) -> Request {
    let pack = dir.join("a.rimpack");
    std::fs::write(&pack, "x").expect("write");
    Request::Open(OpenFile::Pack(pack))
}

/// Serves `listener` on its own thread; every request that arrives comes out of the channel.
fn serving(listener: Listener) -> Receiver<Request> {
    let (tx, rx) = channel();
    let sink: Sink = Arc::new(move |request| tx.send(request).is_ok());
    thread::spawn(move || serve(&listener, &sink));
    rx
}

fn first_window(name: &Name<'_>) -> Receiver<Request> {
    let Some(Won::Primary(listener)) = claim_at(name, &Request::Focus, false) else {
        panic!("the first launch is the window");
    };
    serving(listener)
}

#[test]
fn a_request_survives_the_wire() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let pack = pack_request(tmp.path());
    let line = encode(&pack).expect("line");
    assert_eq!(decode(&line), Some(pack));
    assert_eq!(decode("focus\n"), Some(Request::Focus));
}

#[test]
fn a_line_that_is_not_ours_is_dropped() {
    let tmp = tempfile::tempdir().expect("tempdir");
    std::fs::write(tmp.path().join("notes.txt"), "x").expect("write");
    let at = |name: &str| format!("open\t{}", tmp.path().join(name).display());
    let lines = [
        String::new(),
        "bogus".into(),
        "focus please".into(),
        "open\t".into(),
        "open\trelative.rimpack".into(),
        at("missing.rimpack"),
        at("notes.txt"),
    ];
    for line in lines {
        assert_eq!(decode(&line), None, "{line:?}");
    }
}

#[test]
fn a_name_with_a_line_break_is_not_sent() {
    let pack = Request::Open(OpenFile::Pack("/tmp/a\nb.rimpack".into()));
    assert_eq!(encode(&pack), None);
}

#[test]
fn nothing_listening_means_no_hand_over() {
    let tmp = tempfile::tempdir().expect("tempdir");
    assert!(!hand_over(&name_in(tmp.path()), &Request::Focus));
}

#[test]
fn the_first_launch_is_the_window_and_the_second_hands_over_to_it() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let name = name_in(tmp.path());
    let arrived = first_window(&name);
    let pack = pack_request(tmp.path());
    assert!(matches!(
        claim_at(&name, &pack, false),
        Some(Won::HandedOver)
    ));
    assert_eq!(arrived.recv_timeout(SOON), Ok(pack));
    assert!(matches!(
        claim_at(&name, &Request::Focus, false),
        Some(Won::HandedOver)
    ));
    assert_eq!(arrived.recv_timeout(SOON), Ok(Request::Focus));
}

#[test]
fn a_crashed_windows_socket_file_is_taken_over() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let name = name_in(tmp.path());
    // Dropped without removing its file, as a crash leaves it.
    let crashed = ListenerOptions::new()
        .name(name.borrow())
        .reclaim_name(false)
        .create_sync()
        .expect("listener");
    drop(crashed);
    assert!(tmp.path().join("t.sock").exists());
    let started = Instant::now();
    assert!(matches!(
        claim_at(&name, &Request::Focus, false),
        Some(Won::Primary(_))
    ));
    assert!(
        started.elapsed() >= STALE_AFTER,
        "it must wait and see first"
    );
}

#[test]
fn a_live_name_is_not_taken_until_its_window_is_gone() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let name = name_in(tmp.path());
    let live = bind(&name).expect("first");
    let (stop, gone) = channel::<()>();
    let _window = thread::spawn(move || {
        let sink: Sink = Arc::new(|_| true);
        thread::spawn(move || serve(&live, &sink));
        let _ = gone.recv();
    });
    assert_eq!(
        bind(&name).err().map(|e| e.kind()),
        Some(io::ErrorKind::AddrInUse)
    );
    drop(stop);
}

#[test]
fn the_launch_after_an_update_does_not_hand_over_to_the_closing_window() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let name = name_in(tmp.path());
    let arrived = first_window(&name);
    let pack = pack_request(tmp.path());
    assert!(claim_at(&name, &pack, true).is_none());
    assert!(
        arrived.try_recv().is_err(),
        "nothing may reach the old window"
    );
}

#[test]
fn launches_at_the_same_moment_end_with_one_window() {
    for _ in 0..5 {
        let tmp = tempfile::tempdir().expect("tempdir");
        let ready = Arc::new(Barrier::new(2));
        let launches: Vec<_> = (0..2)
            .map(|_| {
                let (dir, ready) = (tmp.path().to_path_buf(), Arc::clone(&ready));
                thread::spawn(move || {
                    let name = name_in(&dir);
                    ready.wait();
                    match claim_at(&name, &Request::Focus, false) {
                        Some(Won::Primary(listener)) => Some(serving(listener)),
                        Some(Won::HandedOver) => None,
                        None => panic!("a launch ended with neither"),
                    }
                })
            })
            .collect();
        let windows: Vec<_> = launches
            .into_iter()
            .filter_map(|l| l.join().expect("launch"))
            .collect();
        assert_eq!(windows.len(), 1, "exactly one window");
        assert_eq!(windows[0].recv_timeout(SOON), Ok(Request::Focus));
    }
}
