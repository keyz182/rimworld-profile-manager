use super::*;
use crate::consent::ConsentStore;

fn vendor_dll() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../vendor/unitydoorstop/4.5.0/win_x64/winhttp.dll")
}

struct Fixture {
    tmp: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let f = Self {
            tmp: tempfile::tempdir().expect("tempdir"),
        };
        std::fs::create_dir_all(f.game()).expect("game");
        f
    }

    fn game(&self) -> PathBuf {
        self.tmp.path().join("RimWorld")
    }

    fn app(&self) -> PathBuf {
        self.tmp.path().join("app")
    }

    fn consent(&self) -> Consent {
        ConsentStore::open(&self.app())
            .record(Scope::WinhttpDll, &self.game().join(DLL))
            .expect("consent")
    }

    fn state(&self) -> DllState {
        inspect(&self.game(), &self.app()).expect("inspect")
    }

    /// Someone else's UnityDoorstop 4.5: same version resource, different bytes.
    fn foreign_doorstop(&self, ini: Option<&str>) {
        let mut bytes = std::fs::read(vendor_dll()).expect("vendor");
        let last = bytes.len() - 1;
        bytes[last] ^= 0xFF;
        std::fs::write(self.game().join(DLL), bytes).expect("dll");
        if let Some(ini) = ini {
            std::fs::write(self.game().join(INI), ini).expect("ini");
        }
    }
}

#[test]
fn reads_vendored_version_info() {
    let (product, version) = version_info(&std::fs::read(vendor_dll()).expect("vendor"));
    assert_eq!(product.as_deref(), Some("NeighTools"));
    assert_eq!(version, Some([4, 5, 0, 0]));
}

#[test]
fn install_records_hash_and_is_ours() {
    let f = Fixture::new();
    assert_eq!(f.state(), DllState::Absent);
    install(&f.game(), &f.app(), &vendor_dll(), &f.consent()).expect("install");
    assert_eq!(f.state(), DllState::Ours);
    assert_eq!(
        std::fs::read(f.game().join(DLL)).expect("dll"),
        std::fs::read(vendor_dll()).expect("vendor")
    );
    let installs = std::fs::read_to_string(f.app().join(INSTALLS)).expect("record");
    assert!(
        installs.contains("8c6cdbc38836dee87e3368f5de1994d7c0ccebf29e4ce7aba3c0981f9375412c"),
        "{installs}"
    );
}

#[test]
fn install_needs_consent_for_this_path() {
    let f = Fixture::new();
    let other = ConsentStore::open(&f.app())
        .record(Scope::WinhttpDll, &f.tmp.path().join("Other/winhttp.dll"))
        .expect("consent");
    let r = install(&f.game(), &f.app(), &vendor_dll(), &other);
    assert!(matches!(r, Err(WinhttpError::NoConsent(_))));
    let rimsort = ConsentStore::open(&f.app())
        .record(Scope::RimsortSettings, &f.game().join(DLL))
        .expect("consent");
    assert!(matches!(
        install(&f.game(), &f.app(), &vendor_dll(), &rimsort),
        Err(WinhttpError::NoConsent(_))
    ));
    assert!(!f.game().join(DLL).exists());
}

#[test]
fn foreign_dll_never_overwritten_or_removed() {
    let f = Fixture::new();
    std::fs::write(f.game().join(DLL), b"some other proxy dll").expect("dll");
    let DllState::Foreign(foreign) = f.state() else {
        panic!("expected foreign")
    };
    assert!(!foreign.compatible);
    assert!(matches!(
        install(&f.game(), &f.app(), &vendor_dll(), &f.consent()),
        Err(WinhttpError::Foreign)
    ));
    assert!(matches!(
        remove(&f.game(), &f.app()),
        Err(WinhttpError::Foreign)
    ));
    assert_eq!(
        std::fs::read(f.game().join(DLL)).expect("dll"),
        b"some other proxy dll"
    );
}

#[test]
fn changed_after_install_counts_as_foreign() {
    let f = Fixture::new();
    install(&f.game(), &f.app(), &vendor_dll(), &f.consent()).expect("install");
    f.foreign_doorstop(None);
    assert!(matches!(f.state(), DllState::Foreign(_)));
    assert!(matches!(
        remove(&f.game(), &f.app()),
        Err(WinhttpError::Foreign)
    ));
}

#[test]
fn compatible_foreign_doorstop_gives_chain_target() {
    let f = Fixture::new();
    f.foreign_doorstop(Some(
        "[General]\nenabled=true\ntarget_assembly=Doorstop.dll\nharmony_library=x\n",
    ));
    let DllState::Foreign(foreign) = f.state() else {
        panic!("expected foreign")
    };
    assert!(foreign.compatible);
    assert_eq!(foreign.version, Some([4, 5, 0, 0]));
    assert_eq!(foreign.chain_target, Some(f.game().join("Doorstop.dll")));
    std::fs::write(
        f.game().join(INI),
        "enabled = false\ntarget_assembly = Doorstop.dll\n",
    )
    .expect("ini");
    let DllState::Foreign(off) = f.state() else {
        panic!("expected foreign")
    };
    assert_eq!(
        off.chain_target, None,
        "disabled ini loads nothing to chain"
    );
}

#[test]
fn missing_after_install_is_repairable() {
    let f = Fixture::new();
    install(&f.game(), &f.app(), &vendor_dll(), &f.consent()).expect("install");
    std::fs::remove_file(f.game().join(DLL)).expect("game update removed it");
    assert_eq!(f.state(), DllState::Missing);
    install(&f.game(), &f.app(), &vendor_dll(), &f.consent()).expect("repair");
    assert_eq!(f.state(), DllState::Ours);
}

#[test]
fn remove_deletes_ours_and_forgets_it() {
    let f = Fixture::new();
    install(&f.game(), &f.app(), &vendor_dll(), &f.consent()).expect("install");
    remove(&f.game(), &f.app()).expect("remove");
    assert!(!f.game().join(DLL).exists());
    assert_eq!(f.state(), DllState::Absent);
    remove(&f.game(), &f.app()).expect("remove again is a no-op");
}

#[test]
fn launch_check_per_state() {
    let f = Fixture::new();
    assert!(matches!(
        launch_check(&f.game(), &f.app()),
        Err(WinhttpError::NotInstalled)
    ));
    install(&f.game(), &f.app(), &vendor_dll(), &f.consent()).expect("install");
    assert_eq!(launch_check(&f.game(), &f.app()).expect("ours"), None);
    std::fs::remove_file(f.game().join(DLL)).expect("rm");
    assert!(matches!(
        launch_check(&f.game(), &f.app()),
        Err(WinhttpError::NotInstalled)
    ));
    f.foreign_doorstop(Some("enabled=true\ntarget_assembly=Doorstop.dll\n"));
    assert_eq!(
        launch_check(&f.game(), &f.app()).expect("reuse"),
        Some(f.game().join("Doorstop.dll"))
    );
    std::fs::write(f.game().join(DLL), b"not doorstop").expect("w");
    assert!(matches!(
        launch_check(&f.game(), &f.app()),
        Err(WinhttpError::Foreign)
    ));
}
