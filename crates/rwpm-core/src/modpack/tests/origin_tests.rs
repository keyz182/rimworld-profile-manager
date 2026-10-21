use image::{DynamicImage, ImageFormat, RgbImage};

use super::*;
use crate::modpack::cover::PROFILE_COVER;
use crate::profile::file::{FILE_NAME as PROFILE_FILE, PACK_FILE, ProfileFile};

fn small_png() -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    DynamicImage::ImageRgb8(RgbImage::new(64, 48))
        .write_to(&mut out, ImageFormat::Png)
        .expect("png");
    out.into_inner()
}

fn packed(fx: &Fx, cover: bool) -> PathBuf {
    let mut s = spec(&["A.rws"]);
    s.description = "# Hard\n\n- no saves".into();
    s.cover = cover.then(small_png);
    let plan = plan_pack(&fx.source, &fx.id, s).expect("plan");
    let dest = fx.tmp.path().join("origin.rimpack");
    apply_pack(&plan, &dest, &Probe(false), &mut |_| {}).expect("pack");
    dest
}

#[test]
fn a_restored_profile_remembers_its_pack() {
    let fx = fixture();
    let dest = packed(&fx, false);
    let restored = fx.unpack(&dest).expect("unpack");
    let origin = restored.profile.pack.clone().expect("a pack origin");
    assert_eq!(origin.name, "Source");
    assert_eq!(origin.description, "# Hard\n\n- no saves");
    assert!(!origin.has_cover);

    let dir = restored.profile.save_data_dir();
    let dir = dir.parent().expect("profile dir");
    assert_eq!(
        std::fs::read(dir.join(PACK_FILE)).expect("kept pack"),
        std::fs::read(&dest).expect("source pack"),
        "the kept pack is the pack that was opened"
    );
    assert!(!dir.join(PROFILE_COVER).exists());
    let file = ProfileFile::read(&dir.join(PROFILE_FILE)).expect("profile.toml");
    assert_eq!(file.schema, 3);
    assert_eq!(file.pack, Some(origin));
}

#[test]
fn a_cover_is_flagged_and_kept_beside_the_profile() {
    let fx = fixture();
    let restored = fx.unpack(&packed(&fx, true)).expect("unpack");
    assert!(restored.profile.pack.as_ref().expect("origin").has_cover);
    let sd = restored.profile.save_data_dir();
    assert!(sd.parent().expect("dir").join(PROFILE_COVER).is_file());
}

#[test]
fn the_space_estimate_counts_the_kept_pack() {
    let fx = fixture();
    let dest = packed(&fx, false);
    let plan = plan_unpack(&dest, &fx.target).expect("plan");
    let archive = std::fs::metadata(&dest).expect("meta").len();
    assert!(plan.bytes_needed >= plan.manifest.uncompressed_bytes + archive);
}

#[test]
fn a_copy_of_a_pack_profile_is_its_own_profile() {
    let fx = fixture();
    let restored = fx.unpack(&packed(&fx, true)).expect("unpack");
    let copy = fx
        .target
        .clone_profile(&restored.profile.id, "Copy", &Probe(false), &mut |_| {})
        .expect("clone");
    assert!(copy.pack.is_none());
    let dir = copy.save_data_dir();
    let dir = dir.parent().expect("profile dir");
    assert!(!dir.join(PACK_FILE).exists());
    assert!(!dir.join(PROFILE_COVER).exists());
    let file = ProfileFile::read(&dir.join(PROFILE_FILE)).expect("profile.toml");
    assert_eq!(file.schema, 1, "no pack origin, so older builds can read it");
}

#[test]
fn not_now_is_remembered_and_costs_a_plain_profile_nothing() {
    let fx = fixture();
    let restored = fx.unpack(&packed(&fx, false)).expect("unpack");
    assert!(!restored.profile.pack.as_ref().expect("origin").declined_offline);
    let dir = restored.profile.save_data_dir();
    let path = dir.parent().expect("profile dir").join(PROFILE_FILE);
    let before = std::fs::read_to_string(&path).expect("read");
    assert!(!before.contains("declined_offline"), "not written until set");

    let declined = fx
        .target
        .decline_offline_prompt(&restored.profile.id)
        .expect("decline");
    assert!(declined.pack.expect("origin").declined_offline);
    let file = ProfileFile::read(&path).expect("profile.toml");
    assert_eq!(file.schema, 3);
    assert!(file.pack.expect("origin").declined_offline);

    let plain = fx.target.create("Plain").expect("create");
    let after = fx.target.decline_offline_prompt(&plain.id).expect("decline");
    assert!(after.pack.is_none());
}
