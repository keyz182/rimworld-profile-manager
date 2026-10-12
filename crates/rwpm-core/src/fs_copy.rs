//! Recursive copy that clones file data where the filesystem allows it (btrfs, XFS,
//! APFS, ReFS) and falls back to a plain copy elsewhere.

use std::io;
use std::path::Path;

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
