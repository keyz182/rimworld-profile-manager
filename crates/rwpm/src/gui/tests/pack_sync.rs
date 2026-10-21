//! "Steam now has every mod of this pack. Take this profile offline?" The core's own tests
//! cover how Steam's files are read; these cover when the window asks, and "Not now".

use rwpm_core::fetch::record;

use super::backups::selected;
use super::builds::{clicked, found};
use super::pack_reset::pack_profile;
use super::*;

/// A pack profile with one recorded Steam subscription, in a window that has looked.
fn waiting(sb: &Sandbox) -> (App, ProfileId) {
    let (app, id) = pack_profile(sb);
    let dir = app
        .current()
        .and_then(|p| p.stored_dir())
        .expect("dir")
        .to_path_buf();
    record::add(&dir, &[2009463077]).expect("record");
    (app, id)
}

#[test]
fn the_banner_waits_for_steam_to_have_everything() {
    let sb = Sandbox::new();
    let (mut app, id) = waiting(&sb);
    assert!(!found(&app, text::PACK_SYNCED), "nothing looked at yet");
    let _ = app.update(Message::SyncChecked(id.clone(), false));
    assert!(!found(&app, text::PACK_SYNCED));
    let _ = app.update(Message::SyncChecked(id, true));
    assert!(found(&app, text::PACK_SYNCED));
    assert_eq!(
        clicked(&app, text::PACK_SYNCED_GO)
            .iter()
            .map(|m| format!("{m:?}"))
            .collect::<Vec<_>>(),
        ["GoOffline"]
    );
}

#[test]
fn not_now_hides_the_banner_and_is_remembered() {
    let sb = Sandbox::new();
    let (mut app, id) = waiting(&sb);
    let _ = app.update(Message::SyncChecked(id.clone(), true));
    let _ = app.update(Message::DeclineOffline);
    assert!(!found(&app, text::PACK_SYNCED));
    assert!(
        app.current()
            .and_then(|p| p.pack.as_ref())
            .is_some_and(|p| p.declined_offline)
    );

    // Another look, even with everything there, does not bring it back.
    let _ = app.update(Message::SyncChecked(id.clone(), true));
    assert!(!found(&app, text::PACK_SYNCED));
    let fresh = selected(&sb, &id);
    assert!(
        fresh
            .current()
            .and_then(|p| p.pack.as_ref())
            .is_some_and(|p| p.declined_offline)
    );
}

#[test]
fn a_plain_profile_is_never_asked() {
    let sb = Sandbox::new();
    let plain = sb
        .ctx()
        .store()
        .expect("store")
        .create("Plain")
        .expect("create");
    let mut app = selected(&sb, &plain.id);
    let _ = app.update(Message::SyncChecked(plain.id, true));
    assert!(!found(&app, text::PACK_SYNCED));
}
