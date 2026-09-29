//! Helpers for putting content into place at a target path.
//!
//! Resources that replace an existing path stage their content next to the
//! target and rename it over the top, so the window where the target is absent
//! is as small as the filesystem allows. The staging and rename mechanics live
//! here; deciding *whether* a target may be replaced remains resource policy.

use anyhow::{Context as _, Result};
use std::io::Write as _;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt as _;
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
/// On Unix, staging files are private and existing regular-file access modes
/// are preserved. New files and replacements of symlinks are owner-only.
///
/// # Errors
///
/// Returns an error if target metadata cannot be read, the parent directory
/// cannot be created, the staged file cannot be written, its permissions cannot
/// be restored, or the rename fails.
pub fn write_atomic(path: &Path, content: impl AsRef<[u8]>) -> Result<()> {
    ensure_parent_dir(path)?;

    #[cfg(unix)]
    let mode = super::symlink_metadata_optional(path, "read atomic-write target permissions")?
        .filter(std::fs::Metadata::is_file)
        .map(|metadata| metadata.permissions().mode() & 0o777);
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let (mut guard, mut staged_file) =
        TempGuard::create_unique_file_with_mode(parent, ".dotfiles-write", "tmp", 0o600)
            .with_context(|| format!("create temporary file beside {}", path.display()))?;
    staged_file
        .write_all(content.as_ref())
        .with_context(|| format!("write temporary file beside {}", path.display()))?;
    #[cfg(unix)]
    if let Some(mode) = mode {
        staged_file
            .set_permissions(std::fs::Permissions::from_mode(mode))
            .with_context(|| format!("preserve access permissions for {}", path.display()))?;
    }
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

    #[cfg(unix)]
    #[test]
    fn write_atomic_stages_privately_and_preserves_existing_access_modes() {
        struct InspectStaging<'a> {
            parent: &'a Path,
        }

        impl AsRef<[u8]> for InspectStaging<'_> {
            fn as_ref(&self) -> &[u8] {
                let staged = std::fs::read_dir(self.parent)
                    .unwrap()
                    .map(std::result::Result::unwrap)
                    .find(|entry| {
                        entry
                            .file_name()
                            .to_string_lossy()
                            .starts_with(".dotfiles-write")
                    })
                    .expect("staging file must exist before reading payload");
                assert_eq!(
                    staged.metadata().unwrap().permissions().mode() & 0o777,
                    0o600,
                    "staging must be private before any payload bytes are written"
                );
                b"replacement"
            }
        }

        let root = tempfile::tempdir_in(".").unwrap();
        let content = InspectStaging {
            parent: root.path(),
        };
        for (name, mode) in [
            ("new", None),
            ("private", Some(0o600)),
            ("group-readable", Some(0o640)),
            ("executable", Some(0o750)),
            ("read-only", Some(0o400)),
        ] {
            let target = root.path().join(name);
            if let Some(mode) = mode {
                std::fs::write(&target, "old").unwrap();
                std::fs::set_permissions(&target, std::fs::Permissions::from_mode(mode)).unwrap();
            }
            for _ in 0..2 {
                write_atomic(&target, &content).unwrap();
                assert_eq!(std::fs::read_to_string(&target).unwrap(), "replacement");
                assert_eq!(
                    target.metadata().unwrap().permissions().mode() & 0o777,
                    mode.unwrap_or(0o600),
                    "{name}: replacement must retain the intended access mode"
                );
            }
        }
    }

    #[test]
    fn write_atomic_leaves_no_staging_file_behind() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("file.txt");

        write_atomic(&target, "content").unwrap();

        let staging_files = std::fs::read_dir(dir.path())
            .unwrap()
            .map(std::result::Result::unwrap)
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

    #[test]
    fn write_atomic_cleans_staging_and_preserves_contents_when_rename_fails() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("existing-directory");
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("keep"), "unchanged").unwrap();

        let error = write_atomic(&target, "must not replace directory").unwrap_err();

        assert!(error.to_string().starts_with("rename "), "{error:#}");
        assert!(error.downcast_ref::<std::io::Error>().is_some());
        assert_eq!(
            std::fs::read_to_string(target.join("keep")).unwrap(),
            "unchanged"
        );
        let entries: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        assert_eq!(
            entries,
            [target],
            "failed writes must remove the staged file"
        );
    }

    #[test]
    fn write_atomic_rejects_a_file_parent_without_altering_it() {
        let dir = tempfile::tempdir().unwrap();
        let parent = dir.path().join("file");
        std::fs::write(&parent, "keep parent").unwrap();
        let error = write_atomic(&parent.join("target"), "replacement").unwrap_err();
        assert_eq!(
            error.to_string(),
            format!("create parent: {}", parent.display())
        );
        assert_eq!(std::fs::read_to_string(&parent).unwrap(), "keep parent");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
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
        assert_eq!(
            target.metadata().unwrap().permissions().mode() & 0o777,
            0o600,
            "a replaced symlink must not inherit its referent's access mode"
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
