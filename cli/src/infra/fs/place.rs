//! Helpers for putting content into place at a target path.
//!
//! Resources that replace an existing path stage their content next to the
//! target and rename it over the top, so the window where the target is absent
//! is as small as the filesystem allows. The staging and rename mechanics live
//! here; deciding *whether* a target may be replaced remains resource policy.

use anyhow::{Context as _, Result};
#[cfg(not(windows))]
use std::io::Write as _;
#[cfg(all(unix, not(target_os = "linux")))]
use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;

use super::{TempGuard, ensure_parent_dir};

#[cfg(windows)]
mod windows;
#[cfg(all(windows, test))]
pub(crate) use windows::check_private_fixture;
#[cfg(windows)]
pub(super) use windows::{
    copy_destination_options, create_private_dir, verify_copy_access, verify_copy_entry_access,
    verify_private_child,
};

/// Move an entry without replacing an existing destination.
#[cfg(any(target_os = "linux", target_vendor = "apple"))]
pub(crate) fn rename_noreplace(source: &Path, target: &Path) -> std::io::Result<()> {
    use rustix::fs::{CWD, RenameFlags, renameat_with};
    renameat_with(CWD, source, CWD, target, RenameFlags::NOREPLACE)?;
    Ok(())
}

#[cfg(windows)]
#[allow(
    unsafe_code,
    reason = "Win32 provides the no-replace rename missing from std"
)]
pub(crate) fn rename_noreplace_windows(source: &Path, target: &Path) -> std::io::Result<()> {
    use std::io;
    use windows_sys::Win32::Storage::FileSystem::MoveFileExW;

    let source = windows::wide_path(source)?;
    let target = windows::wide_path(target)?;
    // SAFETY: Both buffers are NUL-terminated and live throughout the call.
    // Zero flags disallow replacing an entry or falling back to a copy.
    if unsafe { MoveFileExW(source.as_ptr(), target.as_ptr(), 0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(any(windows, target_os = "linux", target_vendor = "apple")))]
pub(crate) fn rename_noreplace_unsupported(_source: &Path, _target: &Path) -> std::io::Result<()> {
    Err(std::io::ErrorKind::Unsupported.into())
}

#[cfg(not(any(windows, target_os = "linux", target_vendor = "apple")))]
pub(crate) use rename_noreplace_unsupported as rename_noreplace;
#[cfg(windows)]
pub(crate) use rename_noreplace_windows as rename_noreplace;

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
/// Windows creates new files with a protected owner-only DACL. Replacements
/// must match the existing regular file's access descriptor before any bytes
/// are staged. Unsupported descriptors, reparse points and EFS files fail closed.
///
/// # Errors
///
/// Returns an error if target metadata cannot be read, the parent directory
/// cannot be created, the staged file cannot be written, its permissions cannot
/// be restored, or the rename fails.
pub fn write_atomic(path: &Path, content: impl AsRef<[u8]>) -> Result<()> {
    write_atomic_checked(path, content, || Ok(()))
}

/// Record both possible target contents before publication, and retire the old
/// hash only after success. A failed or interrupted publish leaves a record
/// that recognizes either outcome on retry.
pub(crate) fn publish_with_ownership<E: From<anyhow::Error>>(
    ledger: &Path,
    before: Option<&[u8]>,
    desired: &[u8],
    publish: impl FnOnce() -> Result<(), E>,
) -> Result<(), E> {
    let committed = format!("{}\n", super::content_hash(desired));
    let pending = before.map(|old| format!("{committed}{}\n", super::content_hash(old)));
    write_atomic(ledger, pending.as_deref().unwrap_or(&committed))?;
    publish()?;
    write_atomic(ledger, committed).map_err(Into::into)
}

/// Stage a replacement and verify the caller's baseline immediately before publication.
///
/// The check detects intervening edits but cannot lock out uncooperative writers.
///
/// # Errors
///
/// Returns staging, validation, or publication errors without replacing the target.
pub fn write_atomic_checked(
    path: &Path,
    content: impl AsRef<[u8]>,
    check: impl FnOnce() -> Result<()>,
) -> Result<()> {
    ensure_parent_dir(path)?;

    write_checked(path, content, check)
}

#[cfg(windows)]
use windows::write_checked;

#[cfg(all(unix, not(target_os = "linux")))]
fn non_linux_file_mode(metadata: &std::fs::Metadata) -> u32 {
    metadata.permissions().mode() & 0o777
}

#[cfg(target_os = "linux")]
const METADATA_SOURCE_OPEN_FLAGS: i32 = nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK;

#[cfg(not(windows))]
fn write_checked(
    path: &Path,
    content: impl AsRef<[u8]>,
    check: impl FnOnce() -> Result<()>,
) -> Result<()> {
    #[cfg(unix)]
    let existing = super::symlink_metadata_optional(path, "read atomic-write target permissions")?
        .filter(std::fs::Metadata::is_file);
    #[cfg(all(unix, not(target_os = "linux")))]
    let mode = existing.map(|metadata| non_linux_file_mode(&metadata));
    #[cfg(target_os = "linux")]
    let source = existing
        .map(|_| {
            use std::os::unix::fs::OpenOptionsExt as _;
            std::fs::OpenOptions::new()
                .read(true)
                .custom_flags(METADATA_SOURCE_OPEN_FLAGS)
                .open(path)
                .with_context(|| format!("open metadata source {}", path.display()))
        })
        .transpose()?;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    #[cfg(target_os = "linux")]
    let (guard, staged, mut staged_file) = {
        use std::os::unix::fs::OpenOptionsExt as _;
        // Set the verified destination policy on an EMPTY inode. Its enclosing
        // 0700 directory keeps even group-readable payloads private until rename.
        let guard = TempGuard::create_unique_dir_with_mode(parent, ".dotfiles-write", "tmp", 0o700)
            .with_context(|| {
                format!(
                    "reserve private staging directory beside {}",
                    path.display()
                )
            })?;
        let staged = guard.path().join("replacement");
        let file = std::fs::File::options()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&staged)?;
        (guard, staged, file)
    };
    #[cfg(not(target_os = "linux"))]
    let (mut guard, mut staged_file) =
        TempGuard::create_unique_file_with_mode(parent, ".dotfiles-write", "tmp", 0o600)
            .with_context(|| format!("create temporary file beside {}", path.display()))?;
    #[cfg(target_os = "linux")]
    if let Some(source) = source {
        anyhow::ensure!(
            source.metadata()?.is_file(),
            "metadata source is no longer a regular file: {}",
            path.display()
        );
        super::copy::preserve_replacement_metadata(&source, &staged_file).with_context(|| {
            format!(
                "preserve access permissions and attributes for {}",
                path.display()
            )
        })?;
    }
    staged_file
        .write_all(content.as_ref())
        .with_context(|| format!("write temporary file beside {}", path.display()))?;
    #[cfg(all(unix, not(target_os = "linux")))]
    if let Some(mode) = mode {
        staged_file
            .set_permissions(std::fs::Permissions::from_mode(mode))
            .with_context(|| format!("preserve access permissions for {}", path.display()))?;
    }
    drop(staged_file);

    check()?;
    #[cfg(target_os = "linux")]
    {
        rename_into_place(&staged, path)?;
        drop(guard);
    }
    #[cfg(not(target_os = "linux"))]
    {
        rename_into_place(guard.path(), path)?;
        guard.persist();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(target_os = "linux")]
    use std::os::unix::fs::PermissionsExt as _;

    #[test]
    fn ownership_spans_publication_and_retires_the_old_hash_only_on_success() {
        let dir = tempfile::tempdir().unwrap();
        let ledger = dir.path().join("ownership");
        let new = format!("{}\n", crate::infra::fs::content_hash(b"new"));
        let both = format!("{new}{}\n", crate::infra::fs::content_hash(b"old"));
        for fail in [true, false] {
            let result = publish_with_ownership(&ledger, Some(b"old"), b"new", || {
                assert_eq!(std::fs::read_to_string(&ledger).unwrap(), both);
                anyhow::ensure!(!fail, "publication failed");
                Ok(())
            });
            assert_eq!(result.is_err(), fail);
            let final_record = std::fs::read_to_string(&ledger).unwrap();
            assert_eq!(&final_record, if fail { &both } else { &new });
        }
    }

    #[test]
    fn cannot_publish_without_recording_ownership_first() {
        let dir = tempfile::tempdir().unwrap();
        let result: Result<()> = publish_with_ownership(dir.path(), None, b"new", || {
            panic!("must not publish when the ledger is unwritable")
        });
        assert!(result.is_err());
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
    fn failed_prepublication_check_preserves_the_target_and_cleans_staging() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("document");
        std::fs::write(&target, "external edit").unwrap();
        let error =
            write_atomic_checked(&target, "stale replacement", || anyhow::bail!("conflict"))
                .unwrap_err();
        assert_eq!(error.to_string(), "conflict");
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "external edit");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn write_atomic_stages_privately_and_preserves_existing_access_modes() {
        struct InspectStaging<'a> {
            parent: &'a Path,
            #[cfg(target_os = "linux")]
            mode: u32,
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
                #[cfg(target_os = "linux")]
                {
                    assert_eq!(
                        staged.metadata().unwrap().permissions().mode() & 0o777,
                        0o700
                    );
                    let metadata = staged.path().join("replacement").metadata().unwrap();
                    assert_eq!(metadata.len(), 0);
                    assert_eq!(
                        metadata.permissions().mode() & 0o777,
                        self.mode,
                        "final policy must already be established inside private staging"
                    );
                }
                #[cfg(not(target_os = "linux"))]
                assert_eq!(
                    staged.metadata().unwrap().permissions().mode() & 0o777,
                    0o600,
                    "staging must be private before any payload bytes are written"
                );
                b"replacement"
            }
        }

        let root = tempfile::tempdir_in(".").unwrap();
        for (name, mode) in [
            ("new", None),
            ("private", Some(0o600)),
            ("group-readable", Some(0o640)),
            ("executable", Some(0o750)),
            ("read-only", Some(0o400)),
        ] {
            let content = InspectStaging {
                parent: root.path(),
                #[cfg(target_os = "linux")]
                mode: mode.unwrap_or(0o600),
            };
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

        #[cfg(not(windows))]
        assert!(error.to_string().starts_with("rename "), "{error:#}");
        #[cfg(windows)]
        assert!(
            error.to_string().starts_with("inspect access for "),
            "{error:#}"
        );
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
    fn rename_into_place_preserves_staging_and_existing_targets_on_failure() {
        for staged_exists in [false, true] {
            let root = tempfile::tempdir_in(".").unwrap();
            let staged = root.path().join("staged");
            let target = root.path().join("target");
            let retained = if staged_exists {
                std::fs::write(&staged, "replacement").unwrap();
                std::fs::create_dir(&target).unwrap();
                target.join("keep")
            } else {
                target.clone()
            };
            std::fs::write(&retained, "existing content").unwrap();

            let error = rename_into_place(&staged, &target).unwrap_err();

            assert_eq!(
                error.to_string(),
                format!("rename {} to {}", staged.display(), target.display())
            );
            let source = error.downcast_ref::<std::io::Error>().unwrap();
            if staged_exists {
                assert_eq!(std::fs::read_to_string(&staged).unwrap(), "replacement");
                assert_eq!(std::fs::read_dir(&target).unwrap().count(), 1);
            } else {
                assert_eq!(source.kind(), std::io::ErrorKind::NotFound);
                assert!(!staged.exists());
            }
            assert_eq!(
                std::fs::read_to_string(&retained).unwrap(),
                "existing content"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn write_atomic_replaces_dangling_symlinks_without_creating_their_referents() {
        let root = tempfile::tempdir_in(".").unwrap();
        let missing = root.path().join("missing-parent").join("referent");
        let target = root.path().join("config");
        std::os::unix::fs::symlink(std::path::absolute(&missing).unwrap(), &target).unwrap();

        write_atomic(&target, [0, 0xff, b'\n']).unwrap();

        assert!(std::fs::symlink_metadata(&target).unwrap().is_file());
        assert_eq!(std::fs::read(&target).unwrap(), [0, 0xff, b'\n']);
        assert_eq!(
            target.metadata().unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(!missing.parent().unwrap().exists());
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
    }
}
