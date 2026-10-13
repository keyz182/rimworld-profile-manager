use super::*;
use crate::consent::ConsentStore;
use crate::discovery::Engine;
use crate::discovery::{GameInstall, SteamInfo};
use crate::profile::{Location, ProfileId};

struct Probe(bool);

impl RimSortProbe for Probe {
    fn running(&self) -> bool {
        self.0
    }
}

/// RimSort-shaped settings: unrelated keys around `instances`, one Default instance.
const SETTINGS: &str = r#"{
    "theme_name": "RimPy",
    "current_instance": "Default",
    "instances": {
        "Default": {
            "name": "Default",
            "game_folder": "/games/RimWorld",
            "config_folder": "/cfg",
            "local_folder": "/games/RimWorld/Mods",
            "workshop_folder": "",
            "run_args": "",
            "steamcmd_auto_clear_depot_cache": true,
            "steamcmd_install_path": "/rs/instances/Default",
            "steamcmd_ignore": false,
            "steam_client_integration": false,
            "launch_via_steam_protocol": false,
            "instance_folder_override": "",
            "initial_setup": false
        }
    },
    "zz_last": [1, 2.5, "\u00e9"]
}"#;

const FIELDS: [(&str, bool); 13] = [
    ("name", false),
    ("game_folder", false),
    ("config_folder", false),
    ("local_folder", false),
    ("workshop_folder", false),
    ("run_args", false),
    ("steamcmd_auto_clear_depot_cache", true),
    ("steamcmd_install_path", false),
    ("steamcmd_ignore", true),
    ("steam_client_integration", true),
    ("launch_via_steam_protocol", true),
    ("instance_folder_override", false),
    ("initial_setup", true),
];

struct Fixture {
    tmp: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let f = Self {
            tmp: tempfile::tempdir().expect("tempdir"),
        };
        std::fs::create_dir_all(f.settings().parent().expect("dir")).expect("dir");
        std::fs::write(f.settings(), SETTINGS).expect("settings");
        f
    }

    fn settings(&self) -> PathBuf {
        self.tmp.path().join("RimSort/settings.json")
    }

    fn game(&self, steam: bool) -> EffectiveGame {
        EffectiveGame::steam(GameInstall {
            game_dir: PathBuf::from("/games/RimWorld"),
            exe: PathBuf::from("/games/RimWorld/RimWorldLinux"),
            version: None,
            engine: Engine::Unity,
            steam: steam.then(|| SteamInfo {
                library_dir: PathBuf::from("/steam"),
                workshop_dir: PathBuf::from("/steam/steamapps/workshop/content/294100"),
            }),
        })
    }

    fn consent(&self) -> Consent {
        ConsentStore::open(&self.tmp.path().join("app"))
            .record(Scope::RimsortSettings, &self.settings())
            .expect("consent")
    }

    fn plan(&self, profile: &Profile) -> ExportPlan {
        plan_export(
            profile,
            &self.game(true),
            Path::new("/opt/rwpm/rwpm"),
            &self.settings(),
        )
        .expect("plan")
    }

    fn apply(&self, plan: &ExportPlan) -> Result<PathBuf, RimSortError> {
        apply_export(
            plan,
            &self.consent(),
            &self.tmp.path().join("backups"),
            &Probe(false),
        )
    }

    fn json(&self) -> Value {
        serde_json::from_slice(&std::fs::read(self.settings()).expect("read")).expect("json")
    }
}

fn profile(name: &str, mode: Mode) -> Profile {
    Profile {
        id: ProfileId::parse(&name.to_lowercase().replace(' ', "-")).expect("id"),
        name: name.into(),
        mode,
        game_build: None,
        location: Location::Stored {
            dir: PathBuf::from("/profiles").join(name.to_lowercase().replace(' ', "-")),
        },
    }
}

#[test]
fn instance_fields_have_rimsort_types() {
    let f = Fixture::new();
    let plan = f.plan(&profile("My Run", Mode::Offline));
    f.apply(&plan).expect("apply");
    let inst = &f.json()["instances"]["My Run"];
    let keys: Vec<&str> = inst
        .as_object()
        .expect("object")
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        FIELDS.map(|(k, _)| k),
        "all 13 fields, in model order"
    );
    for (k, is_bool) in FIELDS {
        assert_eq!(inst[k].is_boolean(), is_bool, "{k}: {}", inst[k]);
        assert_eq!(inst[k].is_string(), !is_bool, "{k}: {}", inst[k]);
    }
    assert_eq!(
        inst["run_args"],
        "'/opt/rwpm/rwpm' run --from rimsort my-run -- %command%"
    );
    assert_eq!(inst["config_folder"], "/profiles/my-run/SaveData/Config");
    assert_eq!(inst["local_folder"], "/profiles/my-run/Mods");
    assert_eq!(
        inst["workshop_folder"],
        "/steam/steamapps/workshop/content/294100"
    );
    assert_eq!(inst["steam_client_integration"], true);
    assert_eq!(inst["launch_via_steam_protocol"], false);
    let dir = f.tmp.path().join("RimSort/instances/My Run");
    assert_eq!(inst["steamcmd_install_path"], dir.to_str().expect("utf-8"));
    assert!(dir.is_dir(), "instance folder created");
}

#[test]
fn other_keys_values_and_order_preserved() {
    let f = Fixture::new();
    let before: Value = serde_json::from_str(SETTINGS).expect("json");
    f.apply(&f.plan(&profile("My Run", Mode::Live)))
        .expect("apply");
    let after = f.json();
    let keys = |v: &Value| {
        v.as_object()
            .expect("object")
            .keys()
            .cloned()
            .collect::<Vec<_>>()
    };
    assert_eq!(keys(&after), keys(&before));
    for k in ["theme_name", "current_instance", "zz_last"] {
        assert_eq!(after[k], before[k], "{k}");
    }
    assert_eq!(
        after["instances"]["Default"],
        before["instances"]["Default"]
    );
    assert_eq!(keys(&after["instances"]), ["Default", "My Run"]);
    let text = std::fs::read_to_string(f.settings()).expect("text");
    assert!(
        text.contains("\n    \"theme_name\""),
        "4-space indent:\n{text}"
    );
}

#[test]
fn refuses_while_rimsort_runs() {
    let f = Fixture::new();
    let plan = f.plan(&profile("My Run", Mode::Live));
    let r = apply_export(
        &plan,
        &f.consent(),
        &f.tmp.path().join("backups"),
        &Probe(true),
    );
    assert!(matches!(r, Err(RimSortError::RimSortRunning)));
    assert_eq!(
        std::fs::read_to_string(f.settings()).expect("read"),
        SETTINGS
    );
}

#[test]
fn backup_written_before_change_and_matches_original() {
    let f = Fixture::new();
    let backup = f
        .apply(&f.plan(&profile("My Run", Mode::Live)))
        .expect("apply");
    assert!(backup.starts_with(f.tmp.path().join("backups")));
    assert_eq!(std::fs::read_to_string(&backup).expect("backup"), SETTINGS);
    let second = f
        .apply(&f.plan(&profile("Other", Mode::Live)))
        .expect("apply");
    assert_ne!(second, backup, "never overwrites an earlier backup");
}

#[test]
fn refuses_if_settings_changed_since_plan() {
    let f = Fixture::new();
    let plan = f.plan(&profile("My Run", Mode::Live));
    std::fs::write(f.settings(), SETTINGS.replace("RimPy", "Dark")).expect("edit");
    assert!(matches!(f.apply(&plan), Err(RimSortError::Changed)));
    assert!(!f.tmp.path().join("backups").exists());
}

#[test]
fn export_requires_recorded_consent() {
    let f = Fixture::new();
    let plan = f.plan(&profile("My Run", Mode::Live));
    let elsewhere = ConsentStore::open(&f.tmp.path().join("app"))
        .record(
            Scope::RimsortSettings,
            &f.tmp.path().join("other/settings.json"),
        )
        .expect("consent");
    let r = apply_export(
        &plan,
        &elsewhere,
        &f.tmp.path().join("backups"),
        &Probe(false),
    );
    assert!(matches!(r, Err(RimSortError::NoConsent(_))));
    assert_eq!(
        std::fs::read_to_string(f.settings()).expect("read"),
        SETTINGS
    );
}

#[test]
fn missing_settings_is_clear_error() {
    let f = Fixture::new();
    std::fs::remove_file(f.settings()).expect("rm");
    let r = plan_export(
        &profile("X", Mode::Live),
        &f.game(true),
        Path::new("/rwpm"),
        &f.settings(),
    );
    assert!(matches!(r, Err(RimSortError::NotFound(_))));
    std::fs::write(f.settings(), "{\"theme_name\": 1}").expect("w");
    let r = plan_export(
        &profile("X", Mode::Live),
        &f.game(true),
        Path::new("/rwpm"),
        &f.settings(),
    );
    assert!(matches!(r, Err(RimSortError::Parse { .. })));
}

#[test]
fn offline_vs_live_local_folder() {
    let f = Fixture::new();
    f.apply(&f.plan(&profile("Live Run", Mode::Live)))
        .expect("live");
    f.apply(&f.plan(&profile("Off Run", Mode::Offline)))
        .expect("offline");
    let json = f.json();
    assert_eq!(
        json["instances"]["Live Run"]["local_folder"],
        "/games/RimWorld/Mods"
    );
    assert_eq!(
        json["instances"]["Off Run"]["local_folder"],
        "/profiles/off-run/Mods"
    );
}

#[test]
fn default_profile_config_folder_and_name_clash() {
    let f = Fixture::new();
    let default = Profile {
        id: ProfileId::default_id(),
        name: "Default".into(),
        mode: Mode::Live,
        game_build: None,
        location: Location::Default {
            save_dir: PathBuf::from(
                "/home/u/.config/unity3d/Ludeon Studios/RimWorld by Ludeon Studios",
            ),
        },
    };
    let plan = f.plan(&default);
    assert_eq!(
        plan.instance_name, "Default (rwpm)",
        "RimSort's own Default instance kept"
    );
    f.apply(&plan).expect("apply");
    let inst = &f.json()["instances"]["Default (rwpm)"];
    assert_eq!(
        inst["config_folder"],
        "/home/u/.config/unity3d/Ludeon Studios/RimWorld by Ludeon Studios/Config"
    );
    assert_eq!(
        inst["run_args"],
        "'/opt/rwpm/rwpm' run --from rimsort default -- %command%"
    );
}

#[test]
fn non_steam_install_disables_integration() {
    let f = Fixture::new();
    let plan = plan_export(
        &profile("Gog", Mode::Live),
        &f.game(false),
        Path::new("/rwpm"),
        &f.settings(),
    )
    .expect("plan");
    f.apply(&plan).expect("apply");
    let inst = &f.json()["instances"]["Gog"];
    assert_eq!(inst["workshop_folder"], "");
    assert_eq!(inst["steam_client_integration"], false);
}

#[test]
fn reexport_finds_instance_exported_before_the_rimsort_marker() {
    let f = Fixture::new();
    f.apply(&f.plan(&profile("My Run", Mode::Live)))
        .expect("first");
    let mut json = f.json();
    json["instances"]["My Run"]["run_args"] = "'/opt/rwpm/rwpm' run my-run -- %command%".into();
    std::fs::write(
        f.settings(),
        serde_json::to_vec_pretty(&json).expect("json"),
    )
    .expect("w");
    let plan = f.plan(&profile("My Run", Mode::Live));
    assert!(plan.updates_existing, "old run_args still counts as ours");
    f.apply(&plan).expect("second");
    assert_eq!(
        f.json()["instances"]["My Run"]["run_args"],
        "'/opt/rwpm/rwpm' run --from rimsort my-run -- %command%"
    );
}

#[test]
fn reexport_updates_own_instance_keeps_user_fields() {
    let f = Fixture::new();
    f.apply(&f.plan(&profile("My Run", Mode::Live)))
        .expect("first");
    // The player tweaks RimSort-only fields, and renames the profile.
    let mut json = f.json();
    json["instances"]["My Run"]["steamcmd_ignore"] = true.into();
    json["instances"]["My Run"]["initial_setup"] = true.into();
    std::fs::write(
        f.settings(),
        serde_json::to_vec_pretty(&json).expect("json"),
    )
    .expect("w");
    let mut renamed = profile("My Run", Mode::Offline);
    renamed.name = "Renamed".into();
    let plan = f.plan(&renamed);
    assert!(plan.updates_existing);
    assert_eq!(plan.instance_name, "My Run");
    f.apply(&plan).expect("second");
    let inst = &f.json()["instances"]["My Run"];
    assert_eq!(inst["steamcmd_ignore"], true);
    assert_eq!(inst["initial_setup"], true);
    assert_eq!(inst["local_folder"], "/profiles/my-run/Mods");
    assert_eq!(f.json()["instances"].as_object().expect("o").len(), 2);
}

#[test]
fn name_clash_with_foreign_instance_gets_suffix() {
    let f = Fixture::new();
    let mut json: Value = serde_json::from_str(SETTINGS).expect("json");
    json["instances"]["Run"] = json["instances"]["Default"].clone();
    json["instances"]["Run (rwpm)"] = json["instances"]["Default"].clone();
    std::fs::write(
        f.settings(),
        serde_json::to_vec_pretty(&json).expect("json"),
    )
    .expect("w");
    assert_eq!(
        f.plan(&profile("Run", Mode::Live)).instance_name,
        "Run (rwpm) 2"
    );
}

#[test]
fn run_args_quoting_linux_and_windows() {
    let linux = launch_options(Path::new("/home/a b/rwpm"), "run", false).expect("linux");
    assert_eq!(linux, "'/home/a b/rwpm' run run -- %command%");
    let windows =
        launch_options(Path::new(r"C:\Program Files\rwpm\rwpm.exe"), "run", true).expect("windows");
    assert_eq!(
        windows,
        r#""C:\Program Files\rwpm\rwpm.exe" run run -- %command%"#
    );
}

#[test]
fn rwpm_path_with_equals_or_quote_refused() {
    for (path, windows) in [
        ("/a=b/rwpm", false),
        ("/it's/rwpm", false),
        (r"C:\a=b\rwpm.exe", true),
    ] {
        assert!(
            matches!(
                launch_options(Path::new(path), "x", windows),
                Err(RimSortError::UnsafePath(_))
            ),
            "{path}"
        );
    }
}

/// Manual: export into a COPY of this machine's RimSort settings (never the real file).
#[test]
#[ignore = "reads the real RimSort settings and RimWorld install"]
fn real_settings_copy_export() {
    let real = settings_path().expect("data dir");
    let tmp = tempfile::tempdir().expect("tempdir");
    let copy = tmp.path().join("RimSort/settings.json");
    std::fs::create_dir_all(copy.parent().expect("dir")).expect("dir");
    std::fs::copy(&real, &copy).expect("copy");
    let game = EffectiveGame::steam(crate::discovery::discover(None).expect("discover"));
    let plan = plan_export(
        &profile("Smoke Run", Mode::Offline),
        &game,
        Path::new("/opt/rwpm/rwpm"),
        &copy,
    )
    .expect("plan");
    let consent = ConsentStore::open(tmp.path())
        .record(Scope::RimsortSettings, &copy)
        .expect("consent");
    apply_export(&plan, &consent, &tmp.path().join("backups"), &Probe(false)).expect("apply");
    let before: Value = serde_json::from_slice(&std::fs::read(&real).expect("real")).expect("json");
    let after: Value = serde_json::from_slice(&std::fs::read(&copy).expect("copy")).expect("json");
    let (b, a) = (
        before.as_object().expect("o"),
        after.as_object().expect("o"),
    );
    let same = b
        .iter()
        .filter(|(k, v)| *k != "instances" && a.get(*k) == Some(v))
        .count();
    println!(
        "keys before={} after={} unchanged_non_instance={}",
        b.len(),
        a.len(),
        same
    );
    println!("COPY={}", copy.display());
    std::mem::forget(tmp); // keep the copy for the Python type check
}

/// Manual setup for the T57 e2e (docs/QA/T57-rimsort-pinned-export.md): stores a build
/// of this machine's Steam RimWorld with the real capture code, then makes a profile
/// "T57 Pinned" pinned to it. Refuses when that game version is already stored, so it
/// only ever pins a build it made. Writes to the REAL app data: the build is a full copy of
/// the game. Runs only with `RWPM_T57_E2E=1`, so a plain `--ignored` run cannot fill the
/// disk.
#[test]
#[ignore = "copies the real game into the real profiles folder"]
fn t57_e2e_capture_and_pin() {
    use crate::builds::BuildStore;
    use crate::builds::capture::{apply_capture, plan_capture};
    use crate::profile::{OsTrash, ProcessProbe, ProfileStore};
    if std::env::var("RWPM_T57_E2E").as_deref() != Ok("1") {
        println!("set RWPM_T57_E2E=1 to run");
        return;
    }
    let app_dir = crate::paths::app_data_dir().expect("app data dir");
    let root = crate::settings::AppSettings::load(&app_dir)
        .expect("settings")
        .profiles_root(&app_dir);
    let builds = BuildStore::open(&root, &app_dir).expect("build store");
    let live = crate::discovery::discover(None).expect("Steam RimWorld");
    println!(
        "STEAM_GAME={} WORKSHOP={:?}",
        live.game_dir.display(),
        live.steam.as_ref().map(|s| &s.workshop_dir)
    );
    let plan = plan_capture(&builds, &live).expect("plan capture");
    // The key comes from Version.txt, so it cannot be made unique. A build already stored
    // under it is the player's own: never pin, mark or let the rollback delete it.
    let dir = builds.dir_of(&plan.key);
    assert!(
        std::fs::symlink_metadata(&dir).is_err(),
        "REFUSED: a build {} is already stored at {}; it is not this helper's to use. \
         Run the e2e where this game version is not stored yet.",
        plan.key,
        dir.display()
    );
    println!(
        "KEY={} BYTES={} FREE={:?}",
        plan.key, plan.bytes, plan.free_bytes
    );
    apply_capture(&builds, &plan, &ProcessProbe, &mut |_| {}).expect("capture");
    println!("BUILD_DIR={}", builds.dir_of(&plan.key).display());
    let save = crate::paths::default_game_save_dir().expect("save dir");
    let store = ProfileStore::open(root, save, OsTrash).expect("profile store");
    let profile = store.create("T57 Pinned").expect("create T57 Pinned");
    let profile = store
        .set_pin(&profile.id, Some(plan.key.as_str()))
        .expect("pin");
    println!("PROFILE={} PINNED={:?}", profile.id, profile.game_build);
}

#[test]
fn runs_profile_knows_every_form_this_tool_wrote() {
    assert!(runs_profile("'/r/rwpm' run beta -- %command%", "beta"));
    assert!(runs_profile(
        "'/r/rwpm' run --from rimsort beta -- %command%",
        "beta"
    ));
    assert!(!runs_profile(
        "'/r/rwpm' run alpha-beta -- %command%",
        "beta"
    ));
    assert!(!runs_profile("gamemoderun %command%", "beta"));
    assert!(!runs_profile("'/r/rwpm' run beta -- %command% -x", "beta"));
}

/// V55 (+ T57): a pinned profile's instance names the build — its game folder and, LIVE,
/// its `Mods` — while the workshop stays Steam's.
#[test]
fn v55_pinned_export_points_at_the_build() {
    use crate::builds::effective::test_support::Games;
    use crate::builds::effective_game;
    let f = Fixture::new();
    let games = Games::new(f.tmp.path(), &f.tmp.path().join("profiles"));
    let resolve = |p: &Profile| {
        effective_game(
            p,
            Some(&games.live),
            &f.tmp.path().join("profiles"),
            f.tmp.path(),
        )
        .expect("resolve")
    };
    let pinned = Profile {
        game_build: Some(games.build_key.to_string()),
        ..profile("Pinned Run", Mode::Live)
    };
    let pinned_offline = Profile {
        game_build: Some(games.build_key.to_string()),
        ..profile("Pinned Off", Mode::Offline)
    };
    let plain = profile("Plain Run", Mode::Live);
    for p in [&pinned, &pinned_offline, &plain] {
        let plan =
            plan_export(p, &resolve(p), Path::new("/opt/rwpm/rwpm"), &f.settings()).expect("plan");
        f.apply(&plan).expect("apply");
    }
    let json = f.json();
    let field = |inst: &str, k: &str| PathBuf::from(json["instances"][inst][k].as_str().expect(k));
    let build = resolve(&pinned);
    assert_eq!(
        field("Pinned Run", "game_folder"),
        build.rimsort_game_folder()
    );
    assert_eq!(field("Pinned Run", "local_folder"), build.mods_dir());
    assert!(field("Pinned Run", "local_folder").starts_with(&games.build_dir));
    assert_eq!(
        field("Plain Run", "game_folder"),
        games.live.rimsort_game_folder()
    );
    assert_eq!(field("Plain Run", "local_folder"), games.live.mods_dir());
    let workshop = &games.live.steam.as_ref().expect("steam").workshop_dir;
    assert_eq!(&field("Pinned Run", "workshop_folder"), workshop);
    // OFFLINE: the build's game, the profile's own Mods.
    assert_eq!(
        field("Pinned Off", "game_folder"),
        build.rimsort_game_folder()
    );
    assert_eq!(
        Some(field("Pinned Off", "local_folder")),
        pinned_offline.mods_dir()
    );
    assert_eq!(&field("Pinned Off", "workshop_folder"), workshop);
}
