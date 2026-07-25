//! Helpers for putting content into place at a target path.
//!
//! Resources that replace an existing path stage their content next to the
//! target and rename it over the top, so the window where the target is absent
//! is as small as the filesystem allows. The staging and rename mechanics live
//! here; deciding *whether* a target may be replaced remains resource policy.

use anyhow::{Context as _, Result};
use std::path::{Path, PathBuf};

use super::{TempPath, ensure_parent_dir};

/// Build a sibling temporary path by appending `suffix` to the target name.
///
/// Staging next to the target keeps the rename on one filesystem, which is what
/// makes the replacement atomic.
#[must_use]
pub fn sibling_temp_path(target: &Path, suffix: &str) -> PathBuf {
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    let name = target.file_name().map_or_else(
        || "dotfiles_tmp".to_string(),
        |n| format!("{}{suffix}", n.to_string_lossy()),
    );
    parent.join(name)
}

/// Rename `staged` over `target` with consistent path context.
///
/// # Errors
///
/// Returns an error if the rename fails. Callers that need to handle a
/// cross-filesystem rename should inspect the source
/// [`std::io::ErrorKind::CrossesDevices`].
pub fn rename_into_place(staged: &Path, target: &Path) -> Result<()> {
    std::fs::rename(staged, target)
        .with_context(|| format!("rename {} to {}", staged.display(), target.display()))
}

/// Write `content` to `path`, replacing any existing file or symlink atomically.
///
/// The content is staged at a sibling temporary path and renamed over the
/// target, so readers never observe a partially written file and the target is
/// never briefly absent. The parent directory is created if needed.
///
/// # Errors
///
/// Returns an error if the parent directory cannot be created, the staged file
/// cannot be written, or the rename fails.
pub fn write_atomic(path: &Path, content: impl AsRef<[u8]>) -> Result<()> {
    ensure_parent_dir(path)?;

    let staged = sibling_temp_path(path, ".dotfiles_tmp");
    super::write(&staged, content)?;

    let mut guard = TempPath::new(staged.clone());
    rename_into_place(&staged, path)?;
    guard.persist();
    Ok(())
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "test code uses panicking helpers"
)]
mod tests {
    use super::*;

    #[test]
    fn sibling_temp_path_appends_suffix_to_file_name() {
        let tmp = sibling_temp_path(Path::new("/home/test/.bashrc"), ".dotfiles_tmp");
        assert_eq!(
            tmp,
            Path::new("/home/test").join(".bashrc.dotfiles_tmp"),
            "temp path must stay beside the target"
        );
    }

    #[test]
    fn sibling_temp_path_falls_back_for_pathological_targets() {
        let tmp = sibling_temp_path(Path::new("/"), ".dotfiles_tmp");
        assert!(
            tmp.ends_with("dotfiles_tmp"),
            "a target without a file name must still get a temp path, got {}",
            tmp.display()
        );
    }

    #[test]
    fn write_atomic_creates_missing_parents() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("nested").join("deeper").join("file.txt");

        write_atomic(&target, "content").unwrap();

        assert_eq!(std::fs::read_to_string(&target).unwrap(), "content");
    }

    #[test]
    fn write_atomic_replaces_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("file.txt");
        std::fs::write(&target, "stale").unwrap();

        write_atomic(&target, "fresh").unwrap();

        assert_eq!(std::fs::read_to_string(&target).unwrap(), "fresh");
    }

    #[test]
    fn write_atomic_leaves_no_staging_file_behind() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("file.txt");

        write_atomic(&target, "content").unwrap();

        let staged = sibling_temp_path(&target, ".dotfiles_tmp");
        assert!(
            !staged.exists(),
            "staging path {} must not survive a successful write",
            staged.display()
        );
    }

    #[cfg(unix)]
    #[test]
    fn write_atomic_replaces_symlink_with_regular_file() {
        let dir = tempfile::tempdir().unwrap();
        let other = dir.path().join("other.txt");
        std::fs::write(&other, "other").unwrap();
        let target = dir.path().join("link");
        std::os::unix::fs::symlink(&other, &target).unwrap();

        write_atomic(&target, "content").unwrap();

        assert!(
            !std::fs::symlink_metadata(&target)
                .unwrap()
                .file_type()
                .is_symlink(),
            "target must be a regular file after an atomic write"
        );
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "content");
        assert_eq!(
            std::fs::read_to_string(&other).unwrap(),
            "other",
            "the symlink target must not be written through"
        );
    }

    #[test]
    fn rename_into_place_reports_both_paths_on_failure() {
        let dir = tempfile::tempdir().unwrap();
        let staged = dir.path().join("missing");
        let target = dir.path().join("target");

        let error = rename_into_place(&staged, &target).unwrap_err();

        let rendered = format!("{error:#}");
        assert!(rendered.contains("missing"), "got: {rendered}");
        assert!(rendered.contains("target"), "got: {rendered}");
    }
}
