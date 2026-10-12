// T43: Linux only — fixtures assume `RimWorldLinux`, `start_RimWorld.sh`, zenity, a flat
// `Mods`/`Data` install layout. Real macOS GUI/launch coverage is T48's e2e against the
// actual Steam install, not a fake-binary port of this harness.
#![cfg(target_os = "linux")]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use rwpm_core::profile::guard::GuardReport;
use rwpm_core::profile::{Mode, ProfileFile};

use super::message::{Field, GuardChoice, Message, NameFor, Outcome};
use super::*;

/// A sandboxed app: fake Steam library with RimWorld and one workshop mod, settings
/// pointing at it, and a fake `rwpm` that records how it was started.
struct Sandbox {
    tmp: tempfile::TempDir,
}

impl Sandbox {
    fn new() -> Self {
        let sb = Self {
            tmp: tempfile::tempdir().expect("tempdir"),
        };
        let game = sb.game_dir();
        about(&game.join("Data/Core"), "ludeon.rimworld");
        about(
            &sb.path("lib/steamapps/workshop/content/294100/2009463077"),
            "brrainz.harmony",
        );
        std::fs::write(game.join("RimWorldLinux"), "").expect("exe");
        let settings = AppSettings {
            game_dir: Some(game),
            ..AppSettings::default()
        };
        settings.save(&sb.path("app")).expect("settings");
        let exe = sb.path("rwpm");
        std::fs::write(
            &exe,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" \"$RWPM_GUARD_CHOICE\" \"$RWPM_STARTED_BY\" > '{}'\n",
                sb.path("launched.txt").display()
            ),
        )
        .expect("fake rwpm");
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        sb
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.tmp.path().join(rel)
    }

    fn game_dir(&self) -> PathBuf {
        self.path("lib/steamapps/common/RimWorld")
    }

    fn ctx(&self) -> Ctx {
        Ctx {
            app_dir: self.path("app"),
            game_save_dir: self.path("game-save"),
            exe: self.path("rwpm"),
            flatpak: None,
            launcher_failed: false,
        }
    }

    fn app(&self) -> App {
        App::new(self.ctx())
    }

    /// An OFFLINE profile listing `active` mods, nothing frozen.
    fn offline_profile(&self, name: &str, active: &[&str]) -> ProfileId {
        let store = self.ctx().store().expect("store");
        let p = store.create(name).expect("create");
        let dir = store.root().join(p.id.as_str());
        let mut file = ProfileFile::read(&dir.join("profile.toml")).expect("toml");
        file.mode = Mode::Offline;
        file.write_atomic(&dir.join("profile.toml")).expect("toml");
        let config = dir.join("SaveData/Config/ModsConfig.xml");
        std::fs::create_dir_all(config.parent().expect("parent")).expect("dir");
        let items: String = active.iter().map(|a| format!("<li>{a}</li>")).collect();
        std::fs::write(
            config,
            format!("<ModsConfigData><activeMods>{items}</activeMods></ModsConfigData>"),
        )
        .expect("config");
        p.id
    }

    fn launched(&self) -> Option<Vec<String>> {
        // The fake rwpm runs detached; give it a moment.
        for _ in 0..50 {
            if let Ok(text) = std::fs::read_to_string(self.path("launched.txt"))
                && text.ends_with('\n')
            {
                return Some(text.lines().map(str::to_owned).collect());
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        None
    }
}

fn about(dir: &Path, id: &str) {
    std::fs::create_dir_all(dir.join("About")).expect("about");
    std::fs::write(
        dir.join("About/About.xml"),
        format!("<ModMetaData><packageId>{id}</packageId></ModMetaData>"),
    )
    .expect("about");
}

fn names(app: &App) -> Vec<&str> {
    app.profiles.iter().map(|p| p.name.as_str()).collect()
}

#[test]
fn boots_with_default_selected_and_game_found() {
    let sb = Sandbox::new();
    let app = sb.app();
    assert_eq!(names(&app), ["Default"]);
    assert!(app.selected.is_default());
    assert!(app.game.is_ok(), "{:?}", app.game);
}

#[test]
fn new_profile_flow() {
    let sb = Sandbox::new();
    let mut app = sb.app();
    let _ = app.update(Message::New);
    assert!(matches!(
        app.dialog,
        Some(Dialog::Name {
            purpose: NameFor::New,
            ..
        })
    ));
    let _ = app.update(Message::Input(Field::Name, "Modded Run".into()));
    let _ = app.update(Message::Confirm);
    assert!(app.dialog.is_none());
    assert_eq!(names(&app), ["Default", "Modded Run"]);
    assert_eq!(app.selected.as_str(), "modded-run");
    assert_eq!(
        app.notice,
        Some(Notice::Info("Created “Modded Run”.".into()))
    );
}

#[test]
fn rename_flow_and_default_is_protected() {
    let sb = Sandbox::new();
    let mut app = sb.app();
    for msg in [Message::Rename, Message::Delete] {
        let _ = app.update(msg);
        assert!(app.dialog.is_none(), "Default cannot be renamed or deleted");
    }
    let _ = app.update(Message::New);
    let _ = app.update(Message::Input(Field::Name, "Old".into()));
    let _ = app.update(Message::Confirm);
    let _ = app.update(Message::Rename);
    let _ = app.update(Message::Input(Field::Name, "New Name".into()));
    let _ = app.update(Message::Confirm);
    assert_eq!(names(&app), ["Default", "New Name"]);
    assert_eq!(app.selected.as_str(), "old", "id stays (V19)");
    let _ = app.update(Message::Delete);
    assert!(matches!(app.dialog, Some(Dialog::ConfirmDelete)));
    let _ = app.update(Message::Cancel);
    assert!(app.dialog.is_none());
    assert_eq!(names(&app).len(), 2);
}

#[test]
fn clicks_ignored_while_a_job_runs() {
    let sb = Sandbox::new();
    let mut app = sb.app();
    app.job = Some(Job {
        label: "Copying".into(),
        done: 0,
        total: 10,
    });
    let _ = app.update(Message::New);
    assert!(app.dialog.is_none());
    let _ = app.update(Message::Progress(4));
    assert_eq!(app.job.as_ref().map(|j| j.done), Some(4));
    let _ = app.update(Message::Done(Err("disk full".into())));
    assert!(app.job.is_none());
    assert_eq!(app.notice, Some(Notice::Error("disk full".into())));
}

#[test]
fn play_starts_rwpm_run_with_guard_already_answered() {
    let sb = Sandbox::new();
    let mut app = sb.app();
    let _ = app.update(Message::Launch);
    let args = sb.launched().expect("fake rwpm ran");
    let exe = sb.game_dir().join("RimWorldLinux").display().to_string();
    assert_eq!(
        args,
        ["run", "default", "--", exe.as_str(), "launch", "app"]
    );
    assert_eq!(
        app.notice,
        Some(Notice::Info("RimWorld is starting…".into()))
    );
}

/// V31: with Flatpak Steam, Play runs the launcher copy inside Steam's sandbox.
#[test]
fn play_with_flatpak_steam_runs_in_its_sandbox() {
    let sb = Sandbox::new();
    let flatpak = sb.path("flatpak");
    std::fs::write(
        &flatpak,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\n",
            sb.path("launched.txt").display()
        ),
    )
    .expect("fake flatpak");
    std::fs::set_permissions(&flatpak, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    let mut ctx = sb.ctx();
    ctx.flatpak = Some(flatpak);
    let mut app = App::new(ctx);
    let _ = app.update(Message::Launch);
    let args = sb.launched().expect("fake flatpak ran");
    let rwpm = format!("--command={}", sb.path("rwpm").display());
    let exe = sb.game_dir().join("RimWorldLinux").display().to_string();
    assert_eq!(
        args,
        [
            "run",
            rwpm.as_str(),
            "--env=RWPM_GUARD_CHOICE=launch",
            "--env=RWPM_STARTED_BY=app",
            "com.valvesoftware.Steam",
            "run",
            "default",
            "--",
            exe.as_str(),
        ]
    );
}

#[test]
fn offline_launch_with_unfrozen_mod_asks_first() {
    let sb = Sandbox::new();
    let id = sb.offline_profile("Frozen", &["ludeon.rimworld", "brrainz.harmony"]);
    let mut app = sb.app();
    let _ = app.update(Message::Select(id));
    let _ = app.update(Message::Launch);
    let Some(Dialog::Guard(report)) = &app.dialog else {
        panic!("expected the guard dialog, got {:?}", app.dialog)
    };
    assert_eq!(report.unfrozen[0].active_id, "brrainz.harmony");
    assert!(
        sb.launched().is_none(),
        "nothing started before the player answered"
    );
    let _ = app.update(Message::Guard(GuardChoice::PlayAnyway));
    assert!(sb.launched().is_some());
}

#[test]
fn turn_off_answer_edits_mod_list_then_plays() {
    let sb = Sandbox::new();
    let id = sb.offline_profile("Frozen", &["ludeon.rimworld", "brrainz.harmony"]);
    let mut app = sb.app();
    let _ = app.update(Message::Select(id.clone()));
    let _ = app.update(Message::Launch);
    let _ = app.update(Message::Guard(GuardChoice::TurnOff));
    assert!(sb.launched().is_some());
    let config = sb
        .ctx()
        .store()
        .expect("store")
        .root()
        .join(id.as_str())
        .join("SaveData/Config/ModsConfig.xml");
    assert!(
        !std::fs::read_to_string(config)
            .expect("config")
            .contains("brrainz.harmony")
    );
}

#[test]
fn offline_plan_opens_dialog_and_conflicts_gate_confirm() {
    let sb = Sandbox::new();
    let mut app = sb.app();
    let plan = rwpm_core::profile::offline::plan_offline(
        &sb.ctx().store().expect("store"),
        &sb.offline_profile("P", &["brrainz.harmony"]),
        app.game.as_ref().expect("game"),
        false,
    )
    .expect("plan");
    let _ = app.update(Message::Done(Ok(Outcome::OfflinePlanned(plan))));
    assert!(matches!(app.dialog, Some(Dialog::Offline { .. })));
    let _ = app.update(Message::Choose("x".into(), PathBuf::from("/a")));
    let _ = app.update(Message::Choose("x".into(), PathBuf::from("/b")));
    let Some(Dialog::Offline { choices, .. }) = &app.dialog else {
        panic!("dialog closed")
    };
    assert_eq!(
        choices,
        &[("x".to_owned(), PathBuf::from("/b"))],
        "one choice per mod"
    );
}

#[test]
fn settings_save_game_folder() {
    let sb = Sandbox::new();
    let mut app = sb.app();
    let _ = app.update(Message::OpenSettings);
    let Some(Dialog::Settings { game, .. }) = &app.dialog else {
        panic!("no settings dialog")
    };
    assert_eq!(game, &sb.game_dir().display().to_string());
    let _ = app.update(Message::Input(Field::GameDir, String::new()));
    let _ = app.update(Message::Input(Field::SteamDir, "/no/steam/here".into()));
    let _ = app.update(Message::Confirm);
    let saved = AppSettings::load(&sb.path("app")).expect("settings");
    assert_eq!(saved.game_dir, None);
    assert_eq!(saved.steam_dir, Some(PathBuf::from("/no/steam/here")));
    assert!(app.game.is_err(), "no game without a valid Steam folder");
    let _ = app.update(Message::Launch);
    assert!(matches!(&app.notice, Some(Notice::Error(e)) if e.contains("Settings")));
}

#[test]
fn view_buttons_send_messages() {
    let sb = Sandbox::new();
    let mut app = sb.app();
    let _ = app.update(Message::New);
    let _ = app.update(Message::Input(Field::Name, "Run".into()));
    let _ = app.update(Message::Confirm);
    let mut ui = iced_test::simulator(app.view());
    ui.click(text::NEW).expect("New button");
    ui.click(text::LAUNCH).expect("Play button");
    ui.click(text::GO_OFFLINE)
        .expect("OFFLINE button for a LIVE profile");
    ui.click("Default").expect("Default in the list");
    let messages: Vec<Message> = ui.into_messages().collect();
    assert!(
        matches!(
            messages.as_slice(),
            [Message::New, Message::Launch, Message::GoOffline, Message::Select(id)] if id.is_default()
        ),
        "{messages:?}"
    );
}

#[test]
fn guard_dialog_view_offers_all_choices() {
    let sb = Sandbox::new();
    let mut app = sb.app();
    app.dialog = Some(Dialog::Guard(GuardReport::default()));
    let mut ui = iced_test::simulator(app.view());
    for label in [
        text::ADD_TO_PROFILE,
        text::TURN_OFF,
        text::PLAY_ANYWAY,
        text::CANCEL,
    ] {
        ui.click(label).unwrap_or_else(|e| panic!("{label}: {e:?}"));
    }
    let messages: Vec<Message> = ui.into_messages().collect();
    assert_eq!(messages.len(), 4, "{messages:?}");
}

/// Manual: renders every screen and dialog headless at the smallest supported window
/// (960×600, V33), light and dark, to PNGs in `$RWPM_SNAPSHOT_DIR` for review.
#[test]
#[ignore = "writes PNG renders for manual review"]
fn render_screens_manual() {
    let dir = PathBuf::from(std::env::var("RWPM_SNAPSHOT_DIR").expect("RWPM_SNAPSHOT_DIR"));
    let shot = |app: &App, name: &str| {
        for (dark, tag) in [(false, "light"), (true, "dark")] {
            let settings = iced::Settings {
                fonts: super::icon::fonts(),
                default_font: super::icon::UI,
                ..iced::Settings::default()
            };
            let snap = iced_test::Simulator::with_size(settings, (960.0, 600.0), app.view())
                .snapshot(&super::theme::theme(dark))
                .expect("snapshot");
            let file = dir.join(format!("{name}-{tag}"));
            // iced names the file `<name>-<renderer>.png` and only writes it when missing.
            for renderer in ["wgpu", "tiny-skia"] {
                let _ = std::fs::remove_file(dir.join(format!("{name}-{tag}-{renderer}.png")));
            }
            assert!(
                snap.matches_image(file).expect("write png"),
                "{name}-{tag} not written"
            );
        }
    };
    let sb = Sandbox::new();
    played_default(&sb);
    let id = sb.offline_profile("Frozen Colony", &["ludeon.rimworld", "brrainz.harmony"]);
    let mut app = sb.app();
    shot(&app, "01-main-default");
    let _ = app.update(Message::New);
    shot(&app, "02-name-dialog");
    // Start empty: a copy from Default runs as a job, and the screens below need none.
    let _ = app.update(Message::CopyFrom(None));
    let _ = app.update(Message::Input(Field::Name, "Modded Run".into()));
    let _ = app.update(Message::Confirm);
    shot(&app, "03-main-live");
    let _ = app.update(Message::Delete);
    shot(&app, "04-confirm-delete");
    let _ = app.update(Message::Cancel);
    let _ = app.update(Message::Select(id.clone()));
    shot(&app, "05-main-offline");
    let _ = app.update(Message::Launch);
    shot(&app, "06-guard-dialog");
    let _ = app.update(Message::Cancel);
    let game = app.game.clone().expect("game");
    let plan = rwpm_core::profile::offline::plan_offline(
        &sb.ctx().store().expect("store"),
        &id,
        &game,
        false,
    )
    .expect("plan");
    app.dialog = Some(Dialog::Offline {
        plan,
        choices: Vec::new(),
    });
    shot(&app, "07-freeze-dialog");
    app.dialog = Some(Dialog::Consent(super::message::Ask::RimSort));
    shot(&app, "08-consent-rimsort");
    app.dialog = None;
    let _ = app.update(Message::OpenSettings);
    shot(&app, "09-settings");
    let _ = app.update(Message::Cancel);
    let _ = app.update(Message::Help);
    shot(&app, "10-help");
    let _ = app.update(Message::Cancel);
    app.job = Some(Job {
        label: "Freezing mods".into(),
        done: 300 << 20,
        total: 1200 << 20,
    });
    shot(&app, "11-busy");
    app.job = None;
    app.notice = Some(Notice::Error(
        "Could not start RimWorld: permission denied".into(),
    ));
    shot(&app, "12-error-notice");
    app.notice = None;
    app.game = Err("no Steam library has RimWorld".into());
    shot(&app, "13-no-game");
    app.game = Ok(game);
    app.menu_open = true;
    shot(&app, "15-more-menu");
    app.menu_open = false;
    app.drill = Some(super::message::Drill::SteamGuide);
    shot(&app, "20-steam-guide");
    app.drill = Some(super::message::Drill::Mods);
    let profile = app.current().cloned().expect("p");
    let rows = rwpm_core::profile::mod_list::mod_list(&profile, app.game.as_ref().expect("g"));
    app.mods = Some(super::mods::ModList {
        profile: profile.id.clone(),
        rows,
    });
    shot(&app, "21-mod-list");
    app.drill = None;
    health_now(&mut app, &sb);
    app.other_profiles = Some(sb.path("other-layout/profiles"));
    shot(&app, "22-health");
    app.other_profiles = None;
    let _ = app.update(Message::OpenCopySettings);
    shot(&app, "23-copy-settings");
    let _ = app.update(Message::Cancel);

    let many = Sandbox::new();
    let store = many.ctx().store().expect("store");
    store
        .create(&format!("A very long profile name {}", "x".repeat(75)))
        .expect("long");
    for i in 1..=50 {
        store.create(&format!("Colony {i:02}")).expect("create");
    }
    let mut app = many.app();
    shot(&app, "14-many-profiles");
    app.search = "colony 1".into();
    shot(&app, "16-search");

    let first = Sandbox::new();
    fresh(&first);
    let mut app = first.app();
    shot(&app, "17-setup-not-found");
    let folder = first.game_dir().display().to_string();
    let _ = app.update(Message::Setup(super::setup::SetupMsg::Folder(folder)));
    shot(&app, "18-setup-found");
    let _ = app.update(Message::Setup(super::setup::SetupMsg::Continue));
    shot(&app, "19-setup-profile");
}

#[test]
fn open_log_without_log_says_play_first() {
    let sb = Sandbox::new();
    let mut app = sb.app();
    let _ = app.update(Message::OpenLog);
    assert_eq!(
        app.notice,
        Some(Notice::Info(
            "There is no log yet. Play this profile once first.".into()
        ))
    );
}

#[test]
fn support_text_has_versions_logs_and_trims_long_ones() {
    let sb = Sandbox::new();
    let id = sb.offline_profile("Helped", &[]);
    let app = sb.app();
    let profile = app.profiles.iter().find(|p| p.id == id).expect("profile");
    let dir = profile.log_path().parent().expect("dir").to_path_buf();
    std::fs::write(dir.join("rwpm.log"), "guard: player chose Add\n").expect("w");
    let mut big = vec![b'a'; 300 * 1024];
    big.extend_from_slice(b"LAST LINE\n");
    std::fs::write(profile.log_path(), big).expect("w");
    let report = super::support::support_text(profile, app.game.as_ref().ok());
    assert!(report.starts_with(&format!("RimWorld Profile Manager {}", rwpm_core::VERSION)));
    assert!(
        report.contains("Profile: Helped (helped, Offline)"),
        "{}",
        &report[..200]
    );
    assert!(report.contains("===== rwpm.log =====\nguard: player chose Add"));
    assert!(report.contains("===== payload.log =====\n(none)"));
    assert!(report.contains("bytes left out]") && report.ends_with("LAST LINE\n"));
    assert!(report.len() < 270 * 1024, "trimmed to the end of the log");
}

#[test]
fn help_opens_and_ok_closes() {
    let sb = Sandbox::new();
    let mut app = sb.app();
    let mut ui = iced_test::simulator(app.view());
    ui.click(text::HELP).expect("Help button");
    for m in ui.into_messages() {
        let _ = app.update(m);
    }
    assert!(matches!(app.dialog, Some(Dialog::Help)));
    let mut ui = iced_test::simulator(app.view());
    ui.find(text::HELP_SECTIONS[2].1)
        .expect("OFFLINE explained");
    ui.click(text::OK).expect("OK");
    for m in ui.into_messages() {
        let _ = app.update(m);
    }
    assert!(app.dialog.is_none());
}

#[test]
fn steam_launch_options_shown_and_copyable() {
    let sb = Sandbox::new();
    let mut app = sb.app();
    let line = app.launch_options().expect("line");
    assert_eq!(
        line,
        format!("'{}' run default -- %command%", sb.path("rwpm").display())
    );
    assert!(
        iced_test::simulator(app.view())
            .find(line.as_str())
            .is_err(),
        "the raw line stays in the guide (M7)"
    );
    click(&mut app, text::SET_UP_STEAM);
    let mut ui = iced_test::simulator(app.view());
    ui.find(line.as_str()).expect("the guide shows the line");
    ui.find(text::GUIDE_WAITING).expect("no Steam start yet");
    ui.click(text::COPY_STEAM).expect("Copy button");
    let messages: Vec<Message> = ui.into_messages().collect();
    assert!(
        matches!(messages.as_slice(), [Message::CopyLaunchOptions]),
        "{messages:?}"
    );
    let _ = app.update(Message::CopyLaunchOptions);
    assert!(matches!(&app.notice, Some(Notice::Info(n)) if n.contains("Launch options")));
}

/// V38: a failed launcher copy (AppImage, Flatpak) never hands out a launch line or
/// writes RimSort's settings, since the path in them would not work.
#[test]
fn failed_launcher_copy_refuses_launch_line_and_rimsort() {
    let sb = Sandbox::new();
    let mut ctx = sb.ctx();
    ctx.launcher_failed = true;
    let mut app = App::new(ctx);
    let _ = app.update(Message::CopyLaunchOptions);
    assert_eq!(app.notice, Some(Notice::Error(text::NO_LAUNCHER.into())));
    app.notice = None;
    let _ = app.export_job(sb.path("rimsort/settings.json"));
    assert_eq!(app.notice, Some(Notice::Error(text::NO_LAUNCHER.into())));
    assert!(app.job.is_none(), "no RimSort job started");
    // The Steam guide shows why, not a line that would not work.
    app.notice = None;
    let line = app.launch_options().expect("line");
    app.drill = Some(super::message::Drill::SteamGuide);
    let mut ui = iced_test::simulator(app.view());
    ui.find(text::NO_LAUNCHER).expect("guide explains");
    assert!(ui.find(line.as_str()).is_err(), "no launch line shown");
    assert!(ui.find(text::COPY_STEAM).is_err(), "no Copy button");
}

/// V34 B5: set up a Steam launch in ≤ 3 steps (select, Set up Steam, Copy line).
#[test]
fn steam_setup_in_three_steps_then_guide_shows_it_worked() {
    use rwpm_core::profile::{LastRun, RunOutcome, StartedBy};
    let sb = Sandbox::new();
    let id = sb.offline_profile("Steamy", &[]);
    let mut app = sb.app();
    let mut steps = 0;
    for label in ["Steamy", text::SET_UP_STEAM, text::COPY_STEAM] {
        click(&mut app, label);
        steps += 1;
    }
    assert!(steps <= 3);
    assert_eq!(app.selected, id);
    assert!(matches!(&app.notice, Some(Notice::Info(n)) if n == text::STEAM_COPIED));
    // Steam starts it: rwpm run writes the record, the app notices the game.
    let p = app.current().cloned().expect("p");
    LastRun::now(StartedBy::Steam, RunOutcome::Ok, String::new(), None)
        .write(&LastRun::path(sb.ctx().store().expect("s").root(), &p))
        .expect("w");
    let _ = app.update(Message::GameRunning(true));
    iced_test::simulator(app.view())
        .find(text::steam_worked(Some(0)).as_str())
        .expect("the guide says it worked");
    let _ = app.update(Message::Key(
        iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
        iced::keyboard::Modifiers::empty(),
    ));
    assert!(app.drill.is_none(), "Escape leaves the guide");
    iced_test::simulator(app.view())
        .find(text::STEAM_WORKS)
        .expect("the Start section says Steam works");
}

#[test]
fn guide_shows_why_a_steam_start_was_refused_and_closes_on_select() {
    use rwpm_core::profile::{LastRun, RunOutcome, StartedBy};
    let sb = Sandbox::new();
    let mut app = sb.app();
    let p = app.current().cloned().expect("default");
    LastRun::now(
        StartedBy::Steam,
        RunOutcome::Refused,
        "RimWorld is already running; close it first".into(),
        None,
    )
    .write(&LastRun::path(sb.ctx().store().expect("s").root(), &p))
    .expect("w");
    app.reload_last_runs();
    let _ = app.update(Message::Drill(Some(super::message::Drill::SteamGuide)));
    iced_test::simulator(app.view())
        .find(text::steam_refused(Some(0), "RimWorld is already running; close it first").as_str())
        .expect("reason shown");
    let _ = app.update(Message::Select(p.id));
    assert!(app.drill.is_none(), "choosing a profile shows its overview");
}

/// Clicks `label` in the current view and feeds the resulting messages to `app`.
fn click(app: &mut App, label: &str) {
    let mut ui = iced_test::simulator(app.view());
    ui.click(label).unwrap_or_else(|e| panic!("{label}: {e:?}"));
    let messages: Vec<Message> = ui.into_messages().collect();
    for m in messages {
        let _ = app.update(m);
    }
}

#[test]
fn more_menu_holds_rare_actions_and_closes() {
    let sb = Sandbox::new();
    let id = sb.offline_profile("Menu Test", &[]);
    let mut app = sb.app();
    let _ = app.update(Message::Select(id));
    assert!(
        iced_test::simulator(app.view())
            .find(text::OPEN_LOG)
            .is_err(),
        "hidden until More"
    );
    click(&mut app, text::MORE);
    assert!(app.menu_open);
    {
        let mut ui = iced_test::simulator(app.view());
        for label in [
            text::CLONE,
            text::RENAME,
            text::OPEN_SAVES,
            text::OPEN_LOG,
            text::COPY_LOG,
            text::DELETE,
        ] {
            ui.find(label).unwrap_or_else(|e| panic!("{label}: {e:?}"));
        }
    }
    let messages: Vec<Message> = {
        let mut ui = iced_test::simulator(app.view());
        ui.click(text::DELETE).expect("Delete in menu");
        ui.into_messages().collect()
    };
    assert!(
        matches!(messages.as_slice(), [Message::Delete]),
        "a menu click is not also a click outside: {messages:?}"
    );
    for m in messages {
        let _ = app.update(m);
    }
    assert!(!app.menu_open, "an action closes the menu");
    assert!(matches!(app.dialog, Some(Dialog::ConfirmDelete)));
}

#[test]
fn default_menu_has_no_rename_or_delete() {
    let sb = Sandbox::new();
    let mut app = sb.app();
    let _ = app.update(Message::ToggleMenu);
    let mut ui = iced_test::simulator(app.view());
    ui.find(text::CLONE).expect("copy");
    assert!(ui.find(text::RENAME).is_err() && ui.find(text::DELETE).is_err());
}

#[test]
fn search_filters_and_enter_selects_first_match() {
    let sb = Sandbox::new();
    let store = sb.ctx().store().expect("store");
    for name in ["Alpha Colony", "Beta Base", "Alpine Run"] {
        store.create(name).expect("create");
    }
    let mut app = sb.app();
    let _ = app.update(Message::Search("alp".into()));
    let shown: Vec<&str> = app.shown().map(|p| p.name.as_str()).collect();
    assert_eq!(shown, ["Alpha Colony", "Alpine Run"]);
    let _ = app.update(Message::SearchSubmit);
    assert_eq!(app.current().expect("selected").name, "Alpha Colony");
    let _ = app.update(Message::Search("zzz".into()));
    iced_test::simulator(app.view())
        .find(text::NO_MATCH)
        .expect("empty search says so");
}

#[test]
fn keys_move_the_list_open_new_and_close_dialogs() {
    use iced::keyboard::key::Named;
    use iced::keyboard::{Key, Modifiers};
    let sb = Sandbox::new();
    let first = sb.offline_profile("One", &[]);
    let mut app = sb.app();
    let named = |n| Message::Key(Key::Named(n), Modifiers::empty());
    let _ = app.update(named(Named::ArrowDown));
    assert_eq!(app.selected, first, "down from Default");
    let _ = app.update(named(Named::ArrowDown));
    assert_eq!(app.selected, first, "stays on the last row");
    let _ = app.update(named(Named::ArrowUp));
    assert!(app.selected.is_default());
    let _ = app.update(Message::Key(Key::Character("n".into()), Modifiers::CTRL));
    assert!(matches!(
        app.dialog,
        Some(Dialog::Name {
            purpose: NameFor::New,
            ..
        })
    ));
    let _ = app.update(named(Named::Escape));
    assert!(app.dialog.is_none(), "Escape closes the dialog");
    let _ = app.update(Message::Help);
    let _ = app.update(named(Named::Enter));
    assert!(app.dialog.is_none(), "Enter = the Help dialog's OK");
}

#[test]
fn busy_job_blocks_changes_but_not_browsing() {
    let sb = Sandbox::new();
    let id = sb.offline_profile("Busy", &[]);
    let mut app = sb.app();
    app.job = Some(Job {
        label: "Freezing mods".into(),
        done: 0,
        total: 0,
    });
    let _ = app.update(Message::Select(id.clone()));
    assert_eq!(app.selected, id, "the list stays browsable");
    let _ = app.update(Message::New);
    assert!(app.dialog.is_none(), "New waits for the job");
    assert!(matches!(&app.notice, Some(Notice::Info(n)) if n.starts_with("Busy: freezing mods")));
    let mut ui = iced_test::simulator(app.view());
    ui.find(text::busy("Freezing mods").as_str())
        .expect("reason beside Play");
    ui.click(text::LAUNCH).expect("Play shown");
    assert_eq!(ui.into_messages().count(), 0, "Play is disabled");
}

#[test]
fn running_game_blocks_play_and_mod_changes_only() {
    let sb = Sandbox::new();
    let mut app = sb.app();
    let _ = app.update(Message::GameRunning(true));
    let _ = app.update(Message::Launch);
    assert!(sb.launched().is_none(), "no second game");
    assert!(matches!(&app.notice, Some(Notice::Info(n)) if n == text::GAME_RUNNING));
    let _ = app.update(Message::New);
    assert!(
        matches!(app.dialog, Some(Dialog::Name { .. })),
        "making profiles is fine"
    );
}

#[test]
fn long_names_are_cut_in_the_list_only() {
    let sb = Sandbox::new();
    let name = format!("A very long profile name {}", "x".repeat(75));
    let p = sb
        .ctx()
        .store()
        .expect("store")
        .create(&name)
        .expect("create");
    let mut app = sb.app();
    let _ = app.update(Message::Select(p.id));
    let mut ui = iced_test::simulator(app.view());
    ui.find(text::short(&name, 24).as_str())
        .expect("cut in the list");
    ui.find(name.as_str()).expect("whole in the heading");
}

#[test]
fn opens_on_last_played_profile_and_says_how_it_went() {
    use rwpm_core::profile::{LastRun, RunOutcome, StartedBy};
    let sb = Sandbox::new();
    let older = sb.offline_profile("Older", &[]);
    let newer = sb.offline_profile("Newer", &[]);
    let store = sb.ctx().store().expect("store");
    let app = sb.app();
    let profile = |id: &ProfileId| {
        app.profiles
            .iter()
            .find(|p| p.id == *id)
            .cloned()
            .expect("p")
    };
    let mut old_run = LastRun::now(StartedBy::App, RunOutcome::Ok, String::new(), Some(0));
    old_run.time = "2026-01-01T00:00:00Z".parse().expect("t");
    old_run
        .write(&LastRun::path(store.root(), &profile(&older)))
        .expect("w");
    LastRun::now(StartedBy::Steam, RunOutcome::Ok, String::new(), None)
        .write(&LastRun::path(store.root(), &profile(&newer)))
        .expect("w");
    let app = sb.app();
    assert_eq!(app.selected, newer, "B2: last played is selected");
    let mut ui = iced_test::simulator(app.view());
    ui.find("Last run just now: worked (started from Steam).")
        .expect("Play row says how it went");
}

/// A fresh install: no profiles, no chosen game folder, and a Steam folder without
/// RimWorld, so nothing is found automatically.
fn fresh(sb: &Sandbox) {
    AppSettings {
        steam_dir: Some(sb.path("no-steam")),
        ..AppSettings::default()
    }
    .save(&sb.path("app"))
    .expect("settings");
}

#[test]
fn established_install_skips_setup_and_remembers() {
    let sb = Sandbox::new();
    let app = sb.app();
    assert!(
        app.setup.is_none(),
        "a chosen game folder means already set up"
    );
    assert!(AppSettings::load(&sb.path("app")).expect("s").setup_done);
}

/// V34 B1: first open → Play a new profile in ≤ 3 steps when RimWorld is found.
#[test]
fn first_run_to_play_in_three_steps_when_game_found() {
    use super::setup::Setup;
    // The sandbox's settings name the game folder, so it is found; setup is shown anyway.
    let sb = Sandbox::new();
    let mut app = sb.app();
    app.setup = Some(Setup::new(&app.game));
    let mut steps = 0;
    for label in [text::CONTINUE, text::CREATE_PROFILE, text::LAUNCH] {
        click(&mut app, label);
        steps += 1;
    }
    assert!(sb.launched().is_some(), "the game was started");
    assert!(steps <= 3);
    assert_eq!(app.current().expect("p").name, text::FIRST_PROFILE_NAME);
    assert!(AppSettings::load(&sb.path("app")).expect("s").setup_done);
}

/// V34 B1b: not found → a folder is typed (one step) → ≤ 5 steps to Play.
#[test]
fn first_run_without_game_asks_for_the_folder() {
    use super::setup::SetupMsg;
    let sb = Sandbox::new();
    fresh(&sb);
    let mut app = sb.app();
    assert!(app.setup.is_some(), "fresh install shows setup");
    let mut ui = iced_test::simulator(app.view());
    ui.click(text::CONTINUE).expect("Continue shown");
    assert_eq!(ui.into_messages().count(), 0, "Continue waits for the game");
    let folder = sb.game_dir().display().to_string();
    let _ = app.update(Message::Setup(SetupMsg::Folder(folder)));
    let mut steps = 1;
    for label in [text::CONTINUE, text::CREATE_PROFILE, text::LAUNCH] {
        click(&mut app, label);
        steps += 1;
    }
    assert!(sb.launched().is_some());
    assert!(steps <= 5);
    let settings = AppSettings::load(&sb.path("app")).expect("s");
    assert_eq!(settings.game_dir, Some(sb.game_dir()), "the folder is kept");
}

#[test]
fn skip_uses_default_and_offline_choice_freezes() {
    use super::setup::{Setup, SetupMsg};
    let sb = Sandbox::new();
    fresh(&sb);
    let mut app = sb.app();
    let _ = app.update(Message::Setup(SetupMsg::Skip));
    assert!(app.setup.is_none() && app.selected.is_default());
    assert_eq!(app.profiles.len(), 1, "no profile made");
    assert!(AppSettings::load(&sb.path("app")).expect("s").setup_done);

    let sb = Sandbox::new();
    let mut app = sb.app();
    app.setup = Some(Setup::new(&app.game));
    let _ = app.update(Message::Setup(SetupMsg::Continue));
    let _ = app.update(Message::Setup(SetupMsg::Mode(Mode::Offline)));
    let _ = app.update(Message::Setup(SetupMsg::Create));
    assert!(
        app.job
            .as_ref()
            .is_some_and(|j| j.label == text::JOB_CHECKING),
        "OFFLINE starts freezing the new profile's mods"
    );
}

#[test]
fn mod_list_shows_load_order_sources_and_flags() {
    use super::message::Drill;
    let sb = Sandbox::new();
    let id = sb.offline_profile("Listed", &["ludeon.rimworld", "brrainz.harmony", "me.gone"]);
    let mut app = sb.app();
    let _ = app.update(Message::Select(id.clone()));
    click(&mut app, text::VIEW_MODS);
    assert_eq!(app.drill, Some(Drill::Mods));
    iced_test::simulator(app.view())
        .find(text::READING_MODS)
        .expect("reading");
    // What the background read hands back.
    let profile = app.current().cloned().expect("p");
    let rows = rwpm_core::profile::mod_list::mod_list(&profile, app.game.as_ref().expect("g"));
    let _ = app.update(Message::ModsRead(super::mods::ModList {
        profile: id,
        rows,
    }));
    let mut ui = iced_test::simulator(app.view());
    ui.find(text::mods_summary(3, 2).as_str()).expect("summary");
    for label in [
        text::SOURCE_GAME,
        text::SOURCE_STEAM,
        text::FLAG_NOT_FROZEN,
        text::FLAG_MISSING,
    ] {
        ui.find(label).unwrap_or_else(|e| panic!("{label}: {e:?}"));
    }
    drop(ui);
    let _ = app.update(Message::ModSearch("harm".into()));
    let mut ui = iced_test::simulator(app.view());
    assert!(ui.find("me.gone").is_err() && ui.find("brrainz.harmony").is_ok());
}

/// V33: a 1,000-mod list builds quickly and search narrows it.
#[test]
fn thousand_mod_list_stays_quick() {
    use rwpm_core::profile::mod_list::{ModRow, Source};
    let sb = Sandbox::new();
    let mut app = sb.app();
    let rows: Vec<ModRow> = (0..1000)
        .map(|i| ModRow {
            package_id: format!("me.mod{i}"),
            name: format!("Mod number {i}"),
            source: Some(Source::Steam),
            flag: None,
        })
        .collect();
    let _ = app.update(Message::Drill(Some(super::message::Drill::Mods)));
    let id = app.selected.clone();
    let _ = app.update(Message::ModsRead(super::mods::ModList {
        profile: id,
        rows: Ok(rows),
    }));
    // The first simulator pays iced's one-off renderer and font start-up (seconds);
    // measured 2026-10-12: the 1,000-row view itself builds in ~1 ms and lays out in
    // ~150 ms (debug) / ~50 ms (release).
    drop(iced_test::simulator(sb.app().view()));
    let start = std::time::Instant::now();
    let mut ui = iced_test::simulator(app.view());
    ui.find("Mod number 999").expect("last row");
    let took = start.elapsed();
    assert!(
        took < std::time::Duration::from_secs(1),
        "1,000 rows took {took:?}"
    );
    drop(ui);
    let _ = app.update(Message::ModSearch("number 42".into()));
    let mut ui = iced_test::simulator(app.view());
    assert!(ui.find("Mod number 42").is_ok() && ui.find("Mod number 7").is_err());
}

#[test]
fn guard_dialog_names_mods() {
    use rwpm_core::profile::guard::{Fix, Unfrozen};
    let sb = Sandbox::new();
    let mut app = sb.app();
    let report = GuardReport {
        unfrozen: vec![Unfrozen {
            active_id: "brrainz.harmony".into(),
            name: "Harmony".into(),
            fix: Fix::UseFrozen("brrainz.harmony".into()),
        }],
        missing: Vec::new(),
    };
    app.dialog = Some(Dialog::Guard(report));
    let mut ui = iced_test::simulator(app.view());
    ui.find(text::unfrozen_steam_copy("Harmony").as_str())
        .expect("the name, not the package id (M6)");
}

/// What the background health check would hand back for the selected profile.
fn health_now(app: &mut App, sb: &Sandbox) {
    let p = app.current().cloned().expect("p");
    let problems = rwpm_core::profile::health::check(
        &sb.ctx().store().expect("s"),
        &p,
        app.game.as_ref().expect("g"),
        &sb.path("app"),
        None,
        rwpm_core::launch::Os::Linux,
    );
    let _ = app.update(Message::HealthRead(p.id, problems));
}

/// V34 B6: fix a health problem in ≤ 2 steps (select, fix).
#[test]
fn health_shows_unfrozen_mods_and_fixes_in_two_steps() {
    let sb = Sandbox::new();
    let id = sb.offline_profile("Sickly", &["ludeon.rimworld", "brrainz.harmony"]);
    let mut app = sb.app();
    let _ = app.update(Message::Select(id.clone()));
    health_now(&mut app, &sb);
    let words = text::not_frozen(&["brrainz.harmony".to_owned()]);
    let mut ui = iced_test::simulator(app.view());
    ui.find(words.as_str()).expect("problem in plain words");
    drop(ui);
    // From the start: select (1), fix (2).
    let _ = app.update(Message::Select(ProfileId::default_id()));
    let mut steps = 0;
    for label in ["Sickly", text::FIX_FREEZE] {
        click(&mut app, label);
        steps += 1;
    }
    assert!(steps <= 2);
    assert!(
        app.job
            .as_ref()
            .is_some_and(|j| j.label == text::JOB_FREEZING_NEW),
        "freezing started"
    );
}

#[test]
fn healthy_profile_says_so_and_has_no_mark() {
    let sb = Sandbox::new();
    let mut app = sb.app();
    health_now(&mut app, &sb);
    iced_test::simulator(app.view())
        .find(text::HEALTH_OK)
        .expect("all clear");
}

#[test]
fn loader_fix_asks_first_and_does_not_start_the_game() {
    use super::message::Ask;
    let sb = Sandbox::new();
    let mut app = sb.app();
    let _ = app.update(Message::InstallLoader);
    assert!(matches!(
        app.dialog,
        Some(Dialog::Consent(Ask::Loader { then_play: false }))
    ));
    let _ = app.update(Message::GameRunning(true));
    let _ = app.update(Message::Cancel);
    let _ = app.update(Message::InstallLoader);
    assert!(app.dialog.is_none(), "waits while RimWorld runs");
}

#[test]
fn other_layout_profiles_get_a_banner_with_a_folder_button() {
    let sb = Sandbox::new();
    let mut app = sb.app();
    app.other_profiles = Some(sb.path("elsewhere/profiles"));
    let mut ui = iced_test::simulator(app.view());
    ui.click(text::OPEN_THAT_FOLDER).expect("button");
    assert!(matches!(
        ui.into_messages().collect::<Vec<_>>().as_slice(),
        [Message::OpenOtherProfiles]
    ));
}

/// V34 B2 + B3: the last played profile plays in 1 step; another in ≤ 2.
#[test]
fn play_last_played_in_one_step_and_another_in_two() {
    use rwpm_core::profile::{LastRun, RunOutcome, StartedBy};
    let sb = Sandbox::new();
    let store = sb.ctx().store().expect("s");
    let played = store.create("Played").expect("p");
    store.create("Other").expect("o");
    LastRun::now(StartedBy::App, RunOutcome::Ok, String::new(), Some(0))
        .write(&LastRun::path(store.root(), &played))
        .expect("w");
    let mut app = sb.app();
    click(&mut app, text::LAUNCH);
    let args = sb.launched().expect("B2: one click plays the last played");
    assert_eq!(args[1], played.id.as_str());

    let sb = Sandbox::new();
    sb.ctx().store().expect("s").create("Other").expect("o");
    let mut app = sb.app();
    let mut steps = 0;
    for label in ["Other", text::LAUNCH] {
        click(&mut app, label);
        steps += 1;
    }
    assert!(steps <= 2 && sb.launched().is_some(), "B3");
}

/// V34 B4: go OFFLINE in ≤ 3 steps (select, Freeze mods, confirm the plan).
#[test]
fn go_offline_in_three_steps() {
    let sb = Sandbox::new();
    let p = sb.ctx().store().expect("s").create("Live One").expect("p");
    let config = sb
        .ctx()
        .store()
        .expect("s")
        .root()
        .join(p.id.as_str())
        .join("SaveData/Config/ModsConfig.xml");
    std::fs::create_dir_all(config.parent().expect("p")).expect("dir");
    std::fs::write(
        &config,
        "<ModsConfigData><activeMods><li>brrainz.harmony</li></activeMods></ModsConfigData>",
    )
    .expect("config");
    let mut app = sb.app();
    let mut steps = 0;
    for label in ["Live One", text::GO_OFFLINE] {
        click(&mut app, label);
        steps += 1;
    }
    assert!(app.job.is_some(), "the plan is being made");
    // The background plan comes back (not a step).
    let plan = rwpm_core::profile::offline::plan_offline(
        &sb.ctx().store().expect("s"),
        &p.id,
        app.game.as_ref().expect("g"),
        false,
    )
    .expect("plan");
    let _ = app.update(Message::Done(Ok(super::message::Outcome::OfflinePlanned(
        plan,
    ))));
    click(&mut app, text::FREEZE);
    steps += 1;
    assert!(steps <= 3);
    assert!(
        app.job
            .as_ref()
            .is_some_and(|j| j.label == text::JOB_FREEZING),
        "freezing started"
    );
}

/// V36: a new profile with no mod list goes OFFLINE without a dialog (setup's OFFLINE).
#[test]
fn new_profile_goes_offline_without_asking() {
    let sb = Sandbox::new();
    let p = sb.ctx().store().expect("s").create("Brand New").expect("p");
    let mut app = sb.app();
    let _ = app.update(Message::Select(p.id.clone()));
    let plan = rwpm_core::profile::offline::plan_offline(
        &sb.ctx().store().expect("s"),
        &p.id,
        app.game.as_ref().expect("g"),
        false,
    )
    .expect("a new profile plans an empty freeze");
    let _ = app.update(Message::Done(Ok(super::message::Outcome::OfflinePlanned(
        plan,
    ))));
    assert!(app.dialog.is_none(), "nothing to ask");
    assert!(
        app.job
            .as_ref()
            .is_some_and(|j| j.label == text::JOB_FREEZING),
        "it goes straight to OFFLINE"
    );
}

/// V34 B7: find a profile among 50 in ≤ 2 steps (type in search, Enter).
#[test]
fn find_one_of_fifty_in_two_steps() {
    use iced::keyboard::key::Named;
    let sb = Sandbox::new();
    let store = sb.ctx().store().expect("s");
    for i in 1..=50 {
        store.create(&format!("Colony {i:02}")).expect("c");
    }
    let mut app = sb.app();
    let _ = app.update(Message::Search("colony 37".into()));
    let _ = app.update(Message::SearchSubmit);
    assert_eq!(app.current().expect("p").name, "Colony 37");
    let _ = Named::Enter; // Enter in the search field sends SearchSubmit.
}

/// M8 + M9: Settings checks the typed game folder at once; without RimWorld, actions
/// that need it wait.
#[test]
fn settings_checks_folder_and_game_needing_actions_wait() {
    let sb = Sandbox::new();
    let id = sb.offline_profile("Waits", &[]);
    let mut app = sb.app();
    let _ = app.update(Message::OpenSettings);
    iced_test::simulator(app.view())
        .find(text::GAME_FOLDER_OK)
        .expect("found");
    let _ = app.update(Message::Input(
        Field::GameDir,
        sb.path("nowhere").display().to_string(),
    ));
    let mut ui = iced_test::simulator(app.view());
    ui.click(text::SAVE).expect("Save shown");
    assert_eq!(
        ui.into_messages().count(),
        0,
        "Save waits for a good folder"
    );
    let _ = app.update(Message::Cancel);
    app.game = Err("gone".into());
    let _ = app.update(Message::Select(id));
    let mut ui = iced_test::simulator(app.view());
    ui.click(text::REFRESH).expect("shown");
    ui.click(text::EXPORT).expect("shown");
    assert_eq!(ui.into_messages().count(), 0, "they need RimWorld");
}

/// The game's own save folder (Default) with a mod list and settings, as on a played install.
fn played_default(sb: &Sandbox) {
    let cfg = sb.path("game-save/Config");
    std::fs::create_dir_all(&cfg).expect("dir");
    std::fs::write(
        cfg.join("ModsConfig.xml"),
        "<ModsConfigData><activeMods><li>ludeon.rimworld</li><li>brrainz.harmony</li></activeMods></ModsConfigData>",
    )
    .expect("w");
    std::fs::write(cfg.join("Prefs.xml"), "<PrefsData/>").expect("w");
}

#[test]
fn new_profile_starts_from_default_settings_unless_empty_is_chosen() {
    use rwpm_core::profile::copy_settings::Group;
    let sb = Sandbox::new();
    played_default(&sb);
    let mut app = sb.app();
    let _ = app.update(Message::New);
    let Some(Dialog::Name { copy: Some(c), .. }) = &app.dialog else {
        panic!("New offers Start from: {:?}", app.dialog);
    };
    assert!(c.from.as_ref().is_some_and(ProfileId::is_default));
    assert!(c.groups.contains(&Group::ModList) && !c.groups.contains(&Group::ModData));
    iced_test::simulator(app.view())
        .find(text::START_FROM)
        .expect("shown");
    let _ = app.update(Message::Input(Field::Name, "Seeded".into()));
    let _ = app.update(Message::Confirm);
    assert!(
        app.job
            .as_ref()
            .is_some_and(|j| j.label == text::JOB_COPYING_SETTINGS),
        "the copy runs as a job"
    );

    let sb = Sandbox::new();
    let mut app = sb.app();
    let _ = app.update(Message::New);
    let _ = app.update(Message::CopyFrom(None));
    let _ = app.update(Message::ToggleGroup(Group::ModList, false));
    let _ = app.update(Message::Input(Field::Name, "Blank".into()));
    let _ = app.update(Message::Confirm);
    assert!(app.job.is_none(), "Empty copies nothing");
    assert_eq!(app.current().expect("p").name, "Blank");
}

#[test]
fn copy_settings_from_menu_runs_a_job_and_is_not_for_default() {
    let sb = Sandbox::new();
    played_default(&sb);
    let id = sb.offline_profile("Target", &[]);
    let mut app = sb.app();
    let _ = app.update(Message::OpenCopySettings);
    assert!(app.dialog.is_none(), "Default is never a target");
    let _ = app.update(Message::Select(id));
    let _ = app.update(Message::ToggleMenu);
    click(&mut app, text::COPY_SETTINGS_MENU);
    assert!(matches!(app.dialog, Some(Dialog::CopySettings(_))));
    iced_test::simulator(app.view())
        .find(text::COPY_NOTE)
        .expect("says what happens to old files");
    let _ = app.update(Message::GameRunning(true));
    let _ = app.update(Message::Confirm);
    assert!(app.job.is_none(), "waits while RimWorld runs (V37)");
    let _ = app.update(Message::GameRunning(false));
    let _ = app.update(Message::OpenCopySettings);
    let _ = app.update(Message::Confirm);
    assert!(
        app.job
            .as_ref()
            .is_some_and(|j| j.label == text::JOB_COPYING_SETTINGS)
    );
}

#[test]
fn first_run_profile_gets_defaults_mod_list_without_an_extra_step() {
    use super::setup::{Setup, SetupMsg};
    let sb = Sandbox::new();
    played_default(&sb);
    let mut app = sb.app();
    app.setup = Some(Setup::new(&app.game));
    let _ = app.update(Message::Setup(SetupMsg::Continue));
    let _ = app.update(Message::Setup(SetupMsg::Create));
    let p = app.current().cloned().expect("p");
    assert!(!p.is_default());
    let copied = p.save_data_dir().join("Config/ModsConfig.xml");
    // V37 uses the real process check: with a RimWorld open on this machine the copy is
    // refused, and the player is told why.
    if rwpm_core::profile::any_game_running() {
        assert!(!copied.is_file());
        assert!(matches!(&app.notice, Some(Notice::Error(e)) if e.contains("running")));
        return;
    }
    assert!(copied.is_file(), "mod list came from Default");
    assert_eq!(
        app.mod_counts.get(&p.id),
        Some(&2),
        "list row shows its mods"
    );
}

#[test]
fn empty_profile_gets_a_health_line_that_opens_copy_settings() {
    let sb = Sandbox::new();
    played_default(&sb);
    let id = sb.offline_profile("Empty One", &["ludeon.rimworld"]);
    let mut app = sb.app();
    let _ = app.update(Message::Select(id));
    health_now(&mut app, &sb);
    iced_test::simulator(app.view())
        .find(text::ONLY_GAME_CONTENT)
        .expect("Health says the profile is empty");
    click(&mut app, text::COPY_SETTINGS_MENU);
    assert!(matches!(app.dialog, Some(Dialog::CopySettings(_))));
}
