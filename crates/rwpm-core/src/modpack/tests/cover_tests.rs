use std::io::Cursor;

use image::{DynamicImage, ImageFormat, RgbImage};

use super::*;
use crate::modpack::cover::{COVER_ENTRY, PROFILE_COVER, THUMB_EDGE};

fn png(width: u32, height: u32) -> Vec<u8> {
    let mut out = Cursor::new(Vec::new());
    DynamicImage::ImageRgb8(RgbImage::new(width, height))
        .write_to(&mut out, ImageFormat::Png)
        .expect("png");
    out.into_inner()
}

fn size_of(bytes: &[u8]) -> (u32, u32) {
    let image = image::load_from_memory(bytes).expect("decode");
    (image.width(), image.height())
}

fn with_cover_spec(saves: &[&str]) -> PackSpec {
    let mut s = spec(saves);
    s.cover = Some(cover::prepare(&png(1600, 900)).expect("prepare"));
    s
}

/// A manifest for `archive()` that lists a cover and one entry, the mod list.
fn declares_cover() -> Manifest {
    let mut m = sample();
    m.schema = file::SCHEMA;
    m.cover = Some(COVER_ENTRY.into());
    m.start_save = None;
    m.includes = Includes {
        mod_settings: false,
        creations: false,
        game_settings: false,
        saves: vec![],
    };
    m.entry_count = 2;
    m.uncompressed_bytes = (MOD_LIST.len() + "not a picture".len()) as u64;
    m
}

#[test]
fn a_cover_round_trips_into_the_new_profile() {
    let fx = fixture();
    let plan = plan_pack(&fx.source, &fx.id, with_cover_spec(&["A.rws"])).expect("plan");
    assert_eq!(
        plan.entries, 5,
        "list, setting, creation, save and the cover"
    );
    let dest = fx.tmp.path().join("cover.rimpack");
    let report = apply_pack(&plan, &dest, &Probe(false), &mut |_| {}).expect("pack");
    assert_eq!(report.manifest.schema, file::SCHEMA);
    assert_eq!(report.manifest.cover.as_deref(), Some(COVER_ENTRY));

    let (manifest, thumb) = read_preview(&dest, true).expect("preview");
    assert_eq!(manifest, report.manifest);
    let (w, h) = size_of(&thumb.expect("a thumbnail"));
    assert!(w <= THUMB_EDGE && h <= THUMB_EDGE);

    let plan = plan_unpack(&dest, &fx.target).expect("plan");
    assert!(plan.cover.is_some(), "the Open dialog gets the cover");
    let restored = apply_unpack(&plan, &fx.target, &mut |_| {}).expect("unpack");
    let dir = restored.profile.save_data_dir();
    let kept =
        std::fs::read(dir.parent().expect("profile dir").join(PROFILE_COVER)).expect("cover");
    assert!(size_of(&kept).0 <= THUMB_EDGE);
}

#[test]
fn a_pack_without_a_cover_stays_schema_1() {
    let fx = fixture();
    let plan = plan_pack(&fx.source, &fx.id, spec(&[])).expect("plan");
    let dest = fx.tmp.path().join("plain.rimpack");
    let report = apply_pack(&plan, &dest, &Probe(false), &mut |_| {}).expect("pack");
    assert_eq!(report.manifest.schema, file::SCHEMA_BASE);
    let (_, thumb) = read_preview(&dest, true).expect("preview");
    assert!(thumb.is_none());
}

#[test]
fn prepare_scales_a_big_picture_down() {
    let made = cover::prepare(&png(3000, 2000)).expect("prepare");
    let (w, h) = size_of(&made);
    assert_eq!((w, h), (1024, 683));
    let small = cover::prepare(&png(40, 30)).expect("prepare");
    assert_eq!(size_of(&small), (40, 30), "a small picture is not enlarged");
}

#[test]
fn prepare_refuses_other_formats_and_huge_edges() {
    assert!(cover::prepare(b"GIF89a not here").is_err());
    assert!(cover::prepare(b"").is_err());
    assert!(cover::prepare(&png(8193, 1)).is_err());
    assert!(cover::prepare(&png(8192, 1)).is_ok());
}

#[test]
fn a_cover_read_from_a_pack_has_hard_limits() {
    assert!(cover::thumbnail(&png(2048, 1)).is_ok());
    assert!(cover::thumbnail(&png(2049, 1)).is_err());
    assert!(cover::thumbnail(&[0x89, b'P', b'N', b'G']).is_err());
    let oversize = vec![0u8; (cover::MAX_COVER_BYTES + 1) as usize];
    assert!(cover::thumbnail(&oversize).is_err());
}

#[test]
fn only_a_root_png_named_cover_is_a_cover_entry() {
    assert_eq!(classify(Path::new("cover.png")), Ok(Place::Cover));
    for name in [
        "cover.jpg",
        "sub/cover.png",
        "Cover.png",
        "SaveData/cover.png/x",
    ] {
        assert_ne!(classify(Path::new(name)), Ok(Place::Cover), "{name}");
    }
}

#[test]
fn a_manifest_cover_needs_schema_2_and_the_one_name() {
    let mut m = declares_cover();
    assert!(accepted(&m));
    m.schema = file::SCHEMA_BASE;
    assert!(refused(&m), "schema 1 has no cover field");
    let mut m = declares_cover();
    m.cover = Some("other.png".into());
    assert!(refused(&m));
}

#[test]
fn a_cover_that_is_not_a_picture_is_refused_before_anything_is_restored() {
    let fx = fixture();
    let path = archive(
        fx.tmp.path(),
        &declares_cover(),
        &[
            (COVER_ENTRY, EntryType::Regular, "not a picture"),
            (
                "SaveData/Config/ModsConfig.xml",
                EntryType::Regular,
                MOD_LIST,
            ),
        ],
    );
    assert!(matches!(
        plan_unpack(&path, &fx.target),
        Err(ProfileError::BadModpack { .. })
    ));
    assert!(fx.leftovers().is_empty());
}

#[test]
fn a_declared_cover_that_is_not_the_second_entry_is_refused() {
    let fx = fixture();
    let path = archive(
        fx.tmp.path(),
        &declares_cover(),
        &[
            (
                "SaveData/Config/ModsConfig.xml",
                EntryType::Regular,
                MOD_LIST,
            ),
            (COVER_ENTRY, EntryType::Regular, "not a picture"),
        ],
    );
    assert!(matches!(
        plan_unpack(&path, &fx.target),
        Err(ProfileError::BadModpack { .. })
    ));
}

#[test]
fn a_cover_the_manifest_does_not_declare_is_refused_on_restore() {
    let fx = fixture();
    let mut manifest = declares_cover();
    manifest.cover = None;
    manifest.schema = file::SCHEMA_BASE;
    let path = archive(
        fx.tmp.path(),
        &manifest,
        &[
            (COVER_ENTRY, EntryType::Regular, "not a picture"),
            (
                "SaveData/Config/ModsConfig.xml",
                EntryType::Regular,
                MOD_LIST,
            ),
        ],
    );
    let plan = plan_unpack(&path, &fx.target).expect("the manifest alone is fine");
    assert!(apply_unpack(&plan, &fx.target, &mut |_| {}).is_err());
    assert!(
        fx.leftovers().is_empty(),
        "the half-made profile is removed"
    );
}
