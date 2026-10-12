//! Disk use: folder sizes and free space, for the go-OFFLINE estimate.

use std::path::{Path, PathBuf};

/// Total bytes of regular files under `path`. Symlinks are not followed; unreadable
/// entries count as 0 (this is an estimate, not a check).
pub fn dir_size(path: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    entries
        .filter_map(Result::ok)
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => dir_size(&e.path()),
            Ok(t) if t.is_file() => e.metadata().map_or(0, |m| m.len()),
            _ => 0,
        })
        .sum()
}

/// Free bytes on the disk holding `path` (or its nearest existing parent).
pub fn free_space(path: &Path) -> Option<u64> {
    let target = existing_ancestor(path)?.canonicalize().ok()?;
    let disks = sysinfo::Disks::new_with_refreshed_list();
    disks
        .list()
        .iter()
        .filter(|d| target.starts_with(d.mount_point()))
        .max_by_key(|d| d.mount_point().as_os_str().len())
        .map(sysinfo::Disk::available_space)
}

fn existing_ancestor(path: &Path) -> Option<PathBuf> {
    path.ancestors().find(|p| p.exists()).map(Path::to_path_buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dir_size_sums_nested_files() {
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(tmp.path().join("a/b")).expect("dirs");
        std::fs::write(tmp.path().join("x"), [0u8; 10]).expect("w");
        std::fs::write(tmp.path().join("a/b/y"), [0u8; 32]).expect("w");
        assert_eq!(dir_size(tmp.path()), 42);
        assert_eq!(dir_size(&tmp.path().join("missing")), 0);
    }

    #[test]
    fn free_space_found_for_missing_child_of_real_dir() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let free = free_space(&tmp.path().join("not/yet/made"));
        assert!(free.is_some_and(|b| b > 0), "{free:?}");
    }
}
