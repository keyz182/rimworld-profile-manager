//! T81b: "Open modpack…". The core's own tests cover the file and every refusal; these cover
//! the preview (what is present, to get, to find, changed), the links (H1), the limit on what
//! is drawn (M2), and one real restore through the job thread.

use rwpm_core::modpack::{self, Includes, ModEntry, PackSpec};

use super::backups::{Idle, wait_for};
use super::builds::{clicked, found};
use super::*;
use crate::gui::modpack_open::{PackPreview, build_preview};
use crate::gui::support;

const PAGE: &str = "https://steamcommunity.com/sharedfiles/filedetails/?id=";
const HARMONY: u64 = 2009463077;

pub(super) fn entry(id: &str, name: &str, workshop_id: Option<u64>) -> ModEntry {
    ModEntry {
        package_id: id.into(),
        name: name.into(),
        workshop_id,
        url: None,
        time_updated: None,
        version: None,
    }
}

/// A pack made through core from a LIVE profile, listing `mods`.
pub(super) fn make_pack(
    sb: &Sandbox,
    mods: Vec<ModEntry>,
    game_version: Option<&str>,
    dlc: &[&str],
) -> PathBuf {
    let store = sb.ctx().store().expect("store");
    let id = store.create("Source").expect("create").id;
    let config = store
        .root()
        .join(id.as_str())
        .join("SaveData/Config/ModsConfig.xml");
    std::fs::create_dir_all(config.parent().expect("parent")).expect("dir");
    std::fs::write(
        config,
        "<ModsConfigData><activeMods><li>ludeon.rimworld</li></activeMods></ModsConfigData>",
    )
    .expect("list");
    let spec = PackSpec {
        description: "A test pack".into(),
        game_version: game_version.map(str::to_owned),
        dlc: dlc.iter().map(|d| (*d).to_owned()).collect(),
        includes: Includes {
            mod_settings: false,
            creations: false,
            game_settings: false,
            saves: vec![],
        },
        start_save: None,
        collection_url: None,
        mods,
    };
    let plan = modpack::plan_pack(&store, &id, spec).expect("plan");
    let dest = sb.path("made.rimpack");
    modpack::apply_pack(&plan, &dest, &Idle, &mut |_| {}).expect("pack");
    dest
}

pub(super) fn preview(sb: &Sandbox, pack: &Path, game: bool) -> PackPreview {
    let store = sb.ctx().store().expect("store");
    let plan = modpack::plan_unpack(pack, &store).expect("plan");
    let installed = sb.app().game.ok();
    build_preview(plan, installed.as_ref().filter(|_| game))
}

pub(super) fn shown(sb: &Sandbox, preview: PackPreview) -> App {
    let mut app = sb.app();
    let _ = app.update(Message::PackOpened(Box::new(preview)));
    app
}

fn ids(mods: &[crate::gui::modpack_open::PackMod]) -> Vec<&str> {
    mods.iter().map(|m| m.package_id.as_str()).collect()
}

/// The title and the Restore button stay on screen at the smallest window, with every note
/// showing and a long list; the notes and lists between them scroll.
#[test]
fn the_open_dialog_keeps_its_title_and_button_on_screen_at_the_smallest_window() {
    use super::modpacks::{assert_not_squeezed, assert_on_screen};
    let sb = Sandbox::new();
    sb.steam_install("1111");
    let mods: Vec<ModEntry> = (0..250)
        .map(|i| entry(&format!("m.mod{i}"), &format!("Mod {i}"), Some(1000 + i)))
        .collect();
    let pack = make_pack(&sb, mods, Some("0.0.1 rev1"), &["ludeon.rimworld.royalty"]);
    let mut p = preview(&sb, &pack, true);
    p.newer = vec!["Harmony".into()];
    assert!(p.game_note.is_some() && !p.missing_dlc.is_empty());
    let app = shown(&sb, p);
    assert_on_screen(&app, text::OPEN_PACK_TITLE);
    assert_on_screen(&app, text::OPEN_PACK_IT);
    assert_on_screen(&app, text::RESTORE_NAME);
    assert_not_squeezed(&app, &text::pack_section(text::PACK_MISSING, 250));
    assert_not_squeezed(&app, text::PACK_HINT);
}

/// Budget: from the list to a restored profile in ≤ 3 steps (Open modpack…, then the
/// question's Restore). The first step opens the system's file dialog, which is not driven
/// here; the pack it would choose is handed over as the app does after the dialog.
#[test]
fn opening_takes_at_most_three_steps() {
    let sb = Sandbox::new();
    let pack = make_pack(&sb, vec![entry("a.b", "A", Some(5))], None, &[]);
    let mut steps = 1;
    let first = clicked(&sb.app(), text::OPEN_PACK_MENU);
    assert!(matches!(first.as_slice(), [Message::OpenPack]), "{first:?}");
    let app = shown(&sb, preview(&sb, &pack, true));
    let last = clicked(&app, text::OPEN_PACK_IT);
    assert!(matches!(last.as_slice(), [Message::Confirm]), "{last:?}");
    steps += 1;
    assert!(steps <= 3, "{steps} steps");
}

#[test]
fn open_modpack_is_on_the_list() {
    let sb = Sandbox::new();
    let messages = clicked(&sb.app(), text::OPEN_PACK_MENU);
    assert!(
        matches!(messages.as_slice(), [Message::OpenPack]),
        "{messages:?}"
    );
}

#[test]
fn the_preview_sorts_the_mods_and_notes_what_changed() {
    let sb = Sandbox::new();
    std::fs::write(
        sb.path("lib/steamapps/workshop/appworkshop_294100.acf"),
        format!(
            "\"AppWorkshop\"\n{{\n\t\"WorkshopItemsInstalled\"\n\t{{\n\t\t\"{HARMONY}\"\n\t\t{{\n\
             \t\t\t\"timeupdated\"\t\t\"2000\"\n\t\t}}\n\t}}\n}}\n"
        ),
    )
    .expect("acf");
    let mut harmony = entry("brrainz.harmony", "Harmony", Some(HARMONY));
    harmony.time_updated = Some(1000);
    let mut manual = entry("y.manual", "Find Me", None);
    manual.url = Some("http://evil.example/x".into());
    let mods = vec![harmony, entry("x.missing", "Get Me", Some(123)), manual];
    let pack = make_pack(&sb, mods, None, &["ludeon.rimworld.royalty"]);

    let p = preview(&sb, &pack, true);
    assert!(p.checked);
    assert_eq!(ids(&p.present), ["brrainz.harmony"]);
    assert_eq!(ids(&p.missing), ["x.missing"]);
    assert_eq!(ids(&p.manual), ["y.manual"]);
    assert_eq!(
        p.newer,
        ["Harmony"],
        "Steam's copy is newer than the pack's"
    );
    assert_eq!(p.missing_dlc, ["ludeon.rimworld.royalty"]);
    assert_eq!(p.game_note, None, "the pack names no game version");
}

#[test]
fn without_a_game_nothing_can_be_checked_and_the_dialog_says_so() {
    let sb = Sandbox::new();
    let mods = vec![
        entry("brrainz.harmony", "Harmony", Some(HARMONY)),
        entry("y.manual", "Find Me", None),
    ];
    let pack = make_pack(&sb, mods, None, &[]);
    let p = preview(&sb, &pack, false);
    assert!(!p.checked);
    assert!(p.present.is_empty());
    assert_eq!(ids(&p.missing), ["brrainz.harmony"]);
    assert_eq!(ids(&p.manual), ["y.manual"]);
    let app = shown(&sb, p);
    assert!(found(&app, text::PACK_NO_GAME));
}

#[test]
fn a_game_version_that_differs_is_noted_and_one_that_matches_is_not() {
    let sb = Sandbox::new();
    sb.steam_install("1111");
    let here = sb
        .app()
        .game
        .ok()
        .and_then(|g| g.version)
        .expect("game version");
    let different = make_pack(&sb, vec![entry("a.b", "A", None)], Some("0.0.1 rev1"), &[]);
    let note = preview(&sb, &different, true).game_note;
    assert_eq!(note, Some(text::pack_version_differs("0.0.1 rev1", &here)));

    let same_minor = {
        let major_minor: Vec<&str> = here
            .split_whitespace()
            .next()
            .unwrap_or("")
            .split('.')
            .take(2)
            .collect();
        format!("{}.99999 rev1", major_minor.join("."))
    };
    let sb = Sandbox::new();
    sb.steam_install("1111");
    let same = make_pack(&sb, vec![entry("a.b", "A", None)], Some(&same_minor), &[]);
    assert_eq!(preview(&sb, &same, true).game_note, None);
}

#[test]
fn a_file_that_is_not_a_pack_is_refused_in_words_and_opens_nothing() {
    let sb = Sandbox::new();
    let junk = sb.path("junk.rimpack");
    std::fs::write(&junk, "plain text").expect("write");
    let mut app = sb.app();
    let _task = app.open_pack(&junk);
    assert!(app.dialog.is_none());
    let Some(Notice::Error(words)) = &app.notice else {
        panic!("{:?}", app.notice);
    };
    assert!(words.contains("not a modpack"), "{words}");
}

#[test]
fn only_a_steam_page_built_from_the_id_is_a_button() {
    let sb = Sandbox::new();
    let mut manual = entry("y.manual", "Find Me", None);
    manual.url = Some("http://evil.example/x".into());
    let mut tricky = entry("z.tricky", "Looks Safe", Some(456));
    tricky.url = Some("http://evil.example/steam".into());
    let pack = make_pack(
        &sb,
        vec![entry("x.missing", "Get Me", Some(123)), manual, tricky],
        None,
        &[],
    );
    let mut app = shown(&sb, preview(&sb, &pack, true));

    // The page button comes from the number, never from the pack's own link.
    let messages = clicked(&app, text::OPEN_STEAM_PAGE);
    assert_eq!(
        messages
            .iter()
            .map(|m| format!("{m:?}"))
            .collect::<Vec<_>>(),
        [format!("OpenUrl(\"{PAGE}123\")")],
    );
    // A pack's own link is text: it is on screen and clicking it does nothing.
    assert!(found(&app, "http://evil.example/x"));
    assert!(clicked(&app, "http://evil.example/x").is_empty());
    assert!(
        !found(&app, "http://evil.example/steam"),
        "not shown for a mod with an id"
    );

    // Even a message with another address is refused by the opener itself.
    let _ = app.update(Message::OpenUrl("http://evil.example/x".into()));
    assert!(matches!(app.notice, Some(Notice::Error(_))));
}

#[test]
fn the_opener_refuses_everything_but_steam_pages() {
    for url in [
        "http://evil.example/x",
        "https://steamcommunity.com.evil.example/x",
        "https://steamcommunity.com@evil.example/x",
        "javascript:alert(1)",
        "file:///etc/passwd",
        "steam://run/1",
        "https://steamcommunity.com/ x",
        "https://steamcommunity.com/\nx",
        "",
    ] {
        let refused =
            support::open_url(url).is_err_and(|e| e.kind() == std::io::ErrorKind::InvalidInput);
        assert!(refused, "{url:?} must be refused");
    }
}

#[test]
fn a_huge_pack_draws_a_capped_list() {
    let sb = Sandbox::new();
    let mods: Vec<ModEntry> = (0..250)
        .map(|i| entry(&format!("m.mod{i}"), &format!("Mod {i}"), Some(1000 + i)))
        .collect();
    let pack = make_pack(&sb, mods, None, &[]);
    let p = preview(&sb, &pack, true);
    assert_eq!(p.missing.len(), 250);
    let app = shown(&sb, p);
    assert!(found(&app, &text::and_more(50)));
    assert!(found(&app, "Mod 0"));
    assert!(!found(&app, "Mod 249"), "rows past the cap are not drawn");
}

/// The job thread is real; a restore needs no game and no probe.
#[test]
fn a_pack_opened_in_the_app_restores_into_a_new_live_profile() {
    let sb = Sandbox::new();
    let pack = make_pack(
        &sb,
        vec![
            entry("x.missing", "Get Me", Some(123)),
            entry("y.manual", "Find Me", None),
        ],
        None,
        &[],
    );
    let mut app = shown(&sb, preview(&sb, &pack, true));
    assert!(found(&app, text::SHARE_NOTICE));
    assert!(found(&app, text::PACK_HINT));

    let _ = app.update(Message::Input(Field::Name, "   ".into()));
    let _ = app.update(Message::Confirm);
    assert!(
        matches!(app.dialog, Some(Dialog::OpenPack(_))),
        "no name: stays open"
    );
    assert!(app.job.is_none());

    let _ = app.update(Message::Input(Field::Name, "From Pack".into()));
    let _ = app.update(Message::Confirm);
    assert!(app.job.is_some());
    let store = sb.ctx().store().expect("store");
    let restored = || {
        store
            .list()
            .ok()
            .and_then(|l| l.profiles.into_iter().find(|p| p.name == "From Pack"))
    };
    wait_for("the restored profile", || restored().is_some());
    let profile = restored().expect("restored");
    assert_eq!(profile.mode, Mode::Live);
    let config = store
        .root()
        .join(profile.id.as_str())
        .join("SaveData/Config/ModsConfig.xml");
    assert!(
        std::fs::read_to_string(config)
            .expect("list")
            .contains("ludeon.rimworld")
    );

    let line = text::pack_restored("From Pack", 2);
    let _ = app.update(Message::Done(Ok(Outcome::Restored {
        id: profile.id.clone(),
        line: line.clone(),
    })));
    assert_eq!(app.selected, profile.id);
    assert_eq!(app.notice, Some(Notice::Info(line)));
}
