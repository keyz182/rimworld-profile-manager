//! The cover image of a pack. A pack always holds one PNG, `cover.png`, that `prepare`
//! made: whatever the creator picked is decoded, scaled down and written again, so the
//! pack carries no foreign metadata. A pack from someone else is untrusted: `thumbnail`
//! decodes under hard limits and hands back a small PNG, and only that is kept or shown.

use std::io::Cursor;
use std::path::Path;

use image::imageops::FilterType;
use image::{DynamicImage, ImageFormat, ImageReader, Limits};

use super::bad;
use crate::error::ProfileError;

/// The one entry name a cover may have.
pub const COVER_ENTRY: &str = "cover.png";
/// Where a restored profile keeps the thumbnail, beside its `profile.toml`.
pub const PROFILE_COVER: &str = "cover.png";
/// Biggest cover entry, in bytes.
pub const MAX_COVER_BYTES: u64 = 2 << 20;
/// A picture the creator picks may be large; it is scaled down to `PACK_EDGE` on pack.
const MAX_SOURCE_BYTES: u64 = 32 << 20;
const MAX_SOURCE_EDGE: u32 = 8192;
const MAX_SOURCE_ALLOC: u64 = 512 << 20;
const PACK_EDGE: u32 = 1024;
/// A cover inside a pack is never wider or taller than this (`prepare` writes `PACK_EDGE`).
const MAX_PACK_EDGE: u32 = 2048;
const MAX_PACK_ALLOC: u64 = 64 << 20;
/// What the app keeps and shows.
pub const THUMB_EDGE: u32 = 512;

fn decode(bytes: &[u8], edge: u32, alloc: u64) -> Result<DynamicImage, String> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    if !matches!(
        reader.format(),
        Some(ImageFormat::Png | ImageFormat::Jpeg)
    ) {
        return Err("it is not a PNG or JPEG picture".into());
    }
    let mut limits = Limits::default();
    limits.max_image_width = Some(edge);
    limits.max_image_height = Some(edge);
    limits.max_alloc = Some(alloc);
    reader.limits(limits);
    reader.decode().map_err(|e| e.to_string())
}

fn encode(image: &DynamicImage) -> Result<Vec<u8>, String> {
    let mut out = Cursor::new(Vec::new());
    image
        .write_to(&mut out, ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    Ok(out.into_inner())
}

fn fit(image: DynamicImage, edge: u32) -> DynamicImage {
    if image.width() <= edge && image.height() <= edge {
        image
    } else {
        image.resize(edge, edge, FilterType::Lanczos3)
    }
}

/// Reads a picture the creator chose. `path` names it, for errors.
pub fn load_source(path: &Path) -> Result<Vec<u8>, ProfileError> {
    let meta = std::fs::metadata(path).map_err(ProfileError::io("read", path))?;
    if meta.len() > MAX_SOURCE_BYTES {
        return Err(bad(path, "the picture is larger than 32 MiB"));
    }
    std::fs::read(path).map_err(ProfileError::io("read", path))
}

/// The bytes of `cover.png` for a pack, made from a creator's PNG or JPEG.
pub fn prepare(source: &[u8]) -> Result<Vec<u8>, String> {
    let image = decode(source, MAX_SOURCE_EDGE, MAX_SOURCE_ALLOC)?;
    let png = encode(&fit(image, PACK_EDGE))?;
    if png.len() as u64 > MAX_COVER_BYTES {
        return Err("the picture is too detailed: it is over 2 MiB once scaled".into());
    }
    Ok(png)
}

/// A small PNG made from a cover read out of a pack, or why the cover is refused.
pub fn thumbnail(entry: &[u8]) -> Result<Vec<u8>, String> {
    if entry.len() as u64 > MAX_COVER_BYTES {
        return Err("the cover is larger than 2 MiB".into());
    }
    let image = decode(entry, MAX_PACK_EDGE, MAX_PACK_ALLOC)?;
    encode(&fit(image, THUMB_EDGE))
}
