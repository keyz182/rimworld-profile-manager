//! Recursive copy that clones file data where the filesystem allows it (btrfs, XFS,
//! APFS, ReFS) and falls back to a plain copy elsewhere.

use std::io;
use std::io::Read;
use std::path::Path;

use crate::error::ProfileError;

/// Copies `src` into the new dir `dst`. `skip` gets each path relative to `src`.
/// Symlinks are recreated, never followed. `progress` gets the bytes copied so far.
pub fn copy_tree(
    src: &Path,
    dst: &Path,
    skip: &dyn Fn(&Path) -> bool,
    progress: &mut dyn FnMut(u64),
) -> io::Result<()> {
    let mut done = 0;
    copy_dir(src, dst, Path::new(""), skip, &mut |n| {
        done += n;
        progress(done);
    })
}

fn copy_dir(
    src: &Path,
    dst: &Path,
    rel: &Path,
    skip: &dyn Fn(&Path) -> bool,
    add: &mut dyn FnMut(u64),
) -> io::Result<()> {
    std::fs::create_dir(dst.join(rel))?;
    let mut entries: Vec<_> = std::fs::read_dir(src.join(rel))?.collect::<Result<_, _>>()?;
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let rel = rel.join(entry.file_name());
        if skip(&rel) {
            continue;
        }
        let kind = entry.file_type()?;
        let (from, to) = (src.join(&rel), dst.join(&rel));
        if kind.is_symlink() {
            copy_link(&from, &to)?;
        } else if kind.is_dir() {
            copy_dir(src, dst, &rel, skip, add)?;
        } else {
            reflink_copy::reflink_or_copy(&from, &to)?;
            add(entry.metadata()?.len());
        }
    }
    Ok(())
}

#[cfg(unix)]
fn copy_link(from: &Path, to: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(std::fs::read_link(from)?, to)
}

#[cfg(windows)]
fn copy_link(from: &Path, to: &Path) -> io::Result<()> {
    let target = std::fs::read_link(from)?;
    if std::fs::metadata(from).is_ok_and(|m| m.is_dir()) {
        std::os::windows::fs::symlink_dir(target, to)
    } else {
        std::os::windows::fs::symlink_file(target, to)
    }
}

/// V14, V60: `b` holds the same entries, kinds, link targets and bytes as `a`. `skip` gets
/// each path relative to the roots, exactly as `copy_tree` does, so a file left out of the
/// copy is left out of the check too.
pub(crate) fn same_tree(
    a: &Path,
    b: &Path,
    skip: &dyn Fn(&Path) -> bool,
) -> Result<(), ProfileError> {
    same_dir(a, b, Path::new(""), skip)
}

fn same_dir(
    a: &Path,
    b: &Path,
    rel: &Path,
    skip: &dyn Fn(&Path) -> bool,
) -> Result<(), ProfileError> {
    let differs = || ProfileError::VerifyFailed(b.join(rel));
    let list = |dir: &Path| -> Result<Vec<std::ffi::OsString>, ProfileError> {
        let mut v: Vec<_> = std::fs::read_dir(dir.join(rel))
            .map_err(ProfileError::io("list", dir.join(rel)))?
            .map(|e| e.map(|e| e.file_name()))
            .collect::<Result<_, _>>()
            .map_err(ProfileError::io("list", dir.join(rel)))?;
        v.retain(|name| !skip(&rel.join(name)));
        v.sort();
        Ok(v)
    };
    let (la, lb) = (list(a)?, list(b)?);
    if la != lb {
        return Err(differs());
    }
    for name in la {
        let rel = rel.join(name);
        let (pa, pb) = (a.join(&rel), b.join(&rel));
        let kind = |p: &Path| {
            std::fs::symlink_metadata(p)
                .map(|m| m.file_type())
                .map_err(ProfileError::io("read", p))
        };
        let (ta, tb) = (kind(&pa)?, kind(&pb)?);
        if ta.is_symlink() || tb.is_symlink() {
            if std::fs::read_link(&pa).ok() != std::fs::read_link(&pb).ok() {
                return Err(ProfileError::VerifyFailed(pb));
            }
        } else if ta.is_dir() && tb.is_dir() {
            same_dir(a, b, &rel, skip)?;
        } else if !(ta.is_file() && tb.is_file() && same_file(&pa, &pb)?) {
            return Err(ProfileError::VerifyFailed(pb));
        }
    }
    Ok(())
}

fn same_file(a: &Path, b: &Path) -> Result<bool, ProfileError> {
    let open = |p: &Path| std::fs::File::open(p).map_err(ProfileError::io("read", p));
    let (mut fa, mut fb) = (open(a)?, open(b)?);
    let (ma, mb) = (fa.metadata(), fb.metadata());
    if ma.map(|m| m.len()).ok() != mb.map(|m| m.len()).ok() {
        return Ok(false);
    }
    let (mut ba, mut bb) = (vec![0u8; 1 << 16], vec![0u8; 1 << 16]);
    loop {
        let na = Read::read(&mut fa, &mut ba).map_err(ProfileError::io("read", a))?;
        let nb = fb
            .read_exact(&mut bb[..na])
            .map(|()| na)
            .map_err(ProfileError::io("read", b))?;
        if ba[..na] != bb[..nb] {
            return Ok(false);
        }
        if na == 0 {
            return Ok(true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_tree_and_reports_progress_bytes() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let src = tmp.path().join("src");
        std::fs::create_dir_all(src.join("a/b")).expect("dirs");
        std::fs::write(src.join("top.txt"), b"12345").expect("w");
        std::fs::write(src.join("a/b/deep.bin"), [7u8; 1000]).expect("w");
        std::fs::write(src.join("skip.log"), b"x").expect("w");
        let dst = tmp.path().join("dst");
        let mut seen = Vec::new();
        copy_tree(&src, &dst, &|r| r == Path::new("skip.log"), &mut |n| {
            seen.push(n)
        })
        .expect("copy");
        assert_eq!(std::fs::read(dst.join("top.txt")).expect("r"), b"12345");
        assert_eq!(
            std::fs::read(dst.join("a/b/deep.bin")).expect("r"),
            [7u8; 1000]
        );
        assert!(!dst.join("skip.log").exists());
        assert_eq!(seen.last(), Some(&1005));
    }

    #[test]
    fn refuses_existing_destination() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let err = copy_tree(tmp.path(), tmp.path(), &|_| false, &mut |_| {}).expect_err("exists");
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
    }

    #[cfg(unix)]
    #[test]
    fn recreates_symlinks_without_following() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let src = tmp.path().join("src");
        let outside = tmp.path().join("outside");
        std::fs::create_dir_all(&src).expect("src");
        std::fs::create_dir_all(&outside).expect("outside");
        std::fs::write(outside.join("big.bin"), [0u8; 64]).expect("w");
        std::os::unix::fs::symlink(&outside, src.join("linked")).expect("link");
        // A loop must not recurse.
        std::os::unix::fs::symlink(&src, src.join("self")).expect("loop");
        let dst = tmp.path().join("dst");
        let mut bytes = 0;
        copy_tree(&src, &dst, &|_| false, &mut |n| bytes = n).expect("copy");
        assert_eq!(
            std::fs::read_link(dst.join("linked")).expect("link"),
            outside
        );
        assert_eq!(std::fs::read_link(dst.join("self")).expect("link"), src);
        assert_eq!(bytes, 0, "followed a link");
    }
}
