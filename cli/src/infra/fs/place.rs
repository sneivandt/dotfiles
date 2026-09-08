//! Helpers for putting content into place at a target path.
//!
//! Resources that replace an existing path stage their content next to the
//! target and rename it over the top, so the window where the target is absent
//! is as small as the filesystem allows. The staging and rename mechanics live
//! here; deciding *whether* a target may be replaced remains resource policy.

use anyhow::{Context as _, Result};
use std::io::Write as _;
use std::path::Path;

use super::{TempGuard, ensure_parent_dir};

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

    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let (mut guard, mut staged_file) =
        TempGuard::create_unique_file(parent, ".dotfiles-write", "tmp")
            .with_context(|| format!("create temporary file beside {}", path.display()))?;
    staged_file
        .write_all(content.as_ref())
        .with_context(|| format!("write temporary file beside {}", path.display()))?;
    drop(staged_file);

    rename_into_place(guard.path(), path)?;
    guard.persist();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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

        let staging_files = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(std::result::Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".dotfiles-write")
            })
            .count();
        assert_eq!(
            staging_files, 0,
            "temporary files must not survive a successful write"
        );
    }

    #[cfg(unix)]
    #[test]
    fn write_atomic_does_not_follow_a_preexisting_staging_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("config");
        let unrelated = dir.path().join("unrelated");
        std::fs::write(&unrelated, "keep me").unwrap();
        std::os::unix::fs::symlink(&unrelated, dir.path().join("config.dotfiles_tmp")).unwrap();

        write_atomic(&target, "replacement").unwrap();

        assert_eq!(std::fs::read_to_string(target).unwrap(), "replacement");
        assert_eq!(
            std::fs::read_to_string(unrelated).unwrap(),
            "keep me",
            "a path planted at the former fixed staging name must not be opened"
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
