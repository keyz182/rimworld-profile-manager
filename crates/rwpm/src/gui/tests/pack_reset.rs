//! A profile made from a pack: the panel and "Reset to pack…". The core's own tests cover
//! the reset itself; these cover what the window adds: who is offered it, the box for the
//! saves, the offline refusal, and one real reset through the job thread.

use rwpm_core::modpack;
use rwpm_core::profile::{FrozenMod, ModSource, ProfileFile};

use super::backups::{selected, wait_for};
use super::builds::{clicked, found};
use super::pack_open::{entry, make_pack};
use super::*;

/// A profile restored from a pack that lists one mod; the app has it selected.
fn pack_profile(sb: &Sandbox) -> (App, ProfileId) {
    let pack = make_pack(sb, vec![entry("x.mod", "Some Mod", Some(123))], None, &[]);
    let store = sb.ctx().store().expect("store");
    let mut plan = modpack::plan_unpack(&pack, &store).expect("plan");
    plan.name = "From Pack".into();
    let id = modpack::apply_unpack(&plan, &store, &mut |_| {})
        .expect("unpack")
        .profile
        .id;
    let app = selected(sb, &id);
    (app, id)
}

fn config(sb: &Sandbox, id: &ProfileId) -> PathBuf {
    sb.ctx()
        .store()
        .expect("store")
        .root()
        .join(id.as_str())
        .join("SaveData/Config/ModsConfig.xml")
}

#[test]
fn only_a_pack_profile_shows_the_pack_and_the_reset() {
    let sb = Sandbox::new();
    let (app, _) = pack_profile(&sb);
    assert!(found(&app, text::PACK_SECTION));
    assert!(found(&app, &text::pack_origin("Source")));
    assert!(found(&app, text::PACK_RESET));

    let plain = sb
        .ctx()
        .store()
        .expect("store")
        .create("Plain")
        .expect("create");
    let app = selected(&sb, &plain.id);
    assert!(!found(&app, text::PACK_SECTION));
    assert!(!found(&app, text::PACK_RESET));
}

#[test]
fn the_reset_dialog_keeps_saves_off_until_asked() {
    let sb = Sandbox::new();
    let (mut app, _) = pack_profile(&sb);
    for message in clicked(&app, text::PACK_RESET) {
        let _ = app.update(message);
    }
    let Some(Dialog::ResetPack(plan)) = &app.dialog else {
        panic!("{:?}", app.dialog);
    };
    assert!(!plan.restore_saves);
    assert!(found(&app, text::RESET_TITLE));
    assert!(!found(&app, text::RESET_SAVES_NOTE));

    let _ = app.update(Message::ResetSaves(true));
    let Some(Dialog::ResetPack(plan)) = &app.dialog else {
        panic!("{:?}", app.dialog);
    };
    assert!(plan.restore_saves);
    assert!(found(&app, text::RESET_SAVES_NOTE));
}

/// The one test here that uses the real job thread and the real process probe: it fails if
/// a RimWorld is open.
#[test]
fn a_reset_in_the_app_puts_the_mod_list_back() {
    let sb = Sandbox::new();
    let (mut app, id) = pack_profile(&sb);
    let original = std::fs::read_to_string(config(&sb, &id)).expect("list");
    std::fs::write(config(&sb, &id), "<ModsConfigData/>").expect("edit");

    let _ = app.update(Message::ResetPack);
    let _ = app.update(Message::Confirm);
    assert!(app.job.is_some());
    wait_for("the mod list", || {
        std::fs::read_to_string(config(&sb, &id)).is_ok_and(|now| now == original)
    });
}

#[test]
fn an_offline_profile_lacking_the_mods_cannot_be_reset() {
    let sb = Sandbox::new();
    let (_, id) = pack_profile(&sb);
    let path = sb
        .ctx()
        .store()
        .expect("store")
        .root()
        .join(id.as_str())
        .join("profile.toml");
    let mut file = ProfileFile::read(&path).expect("read");
    file.mode = Mode::Offline;
    file.frozen_mods = vec![FrozenMod {
        folder: "other".into(),
        package_id: "other.mod".into(),
        source: ModSource::Workshop,
        source_dir: PathBuf::from("/elsewhere"),
        copied_at: "2026-10-19T00:00:00Z".parse().expect("datetime"),
    }];
    file.write_atomic(&path).expect("write");
    let mut app = selected(&sb, &id);

    let _ = app.update(Message::ResetPack);
    assert!(found(&app, &text::reset_missing(&["Some Mod"])));
    let _ = app.update(Message::Confirm);
    assert!(
        app.job.is_none(),
        "Enter must not start what the button refuses"
    );
    assert!(matches!(app.dialog, Some(Dialog::ResetPack(_))));
}
