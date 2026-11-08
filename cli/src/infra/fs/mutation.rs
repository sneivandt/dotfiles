//! Open files for in-place mutation without following an unexpected final link.

use std::fs::{File, OpenOptions};
use std::path::Path;

use anyhow::{Context as _, Result};

/// Open a regular, singly linked file without truncating it during validation.
pub(crate) fn open_in_place(path: &Path, options: &mut OpenOptions) -> Result<File> {
    options.truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK);
        options.mode(0o600);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt as _;
        use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = options
        .open(path)
        .with_context(|| format!("opening in-place target {}", path.display()))?;
    let metadata = file.metadata()?;
    anyhow::ensure!(
        metadata.is_file(),
        "in-place target is not a regular file: {}",
        path.display()
    );
    ensure_single_link(&file).with_context(|| format!("validating {}", path.display()))?;
    Ok(file)
}

fn ensure_single_link(file: &File) -> Result<()> {
    #[cfg(unix)]
    return require_single_link(&file.metadata()?);
    #[cfg(windows)]
    return windows::ensure_single_link(file);
    #[cfg(not(any(unix, windows)))]
    anyhow::bail!("cannot verify in-place target link count on this platform");
}

/// Refuse an inode whose permissions or bytes are shared with other entries.
#[cfg(unix)]
pub(crate) fn require_single_link(metadata: &std::fs::Metadata) -> Result<()> {
    use std::os::unix::fs::MetadataExt as _;
    anyhow::ensure!(
        metadata.nlink() == 1,
        "refusing in-place mutation of a multiply linked file"
    );
    Ok(())
}

#[cfg(windows)]
#[allow(
    unsafe_code,
    reason = "query link count from the already-open Windows file handle"
)]
mod windows;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mutation_target_matrix_preserves_bytes_until_validation_finishes() {
        let fixture = tempfile::tempdir().unwrap();
        let target = fixture.path().join("target");
        let alias = fixture.path().join("alias");
        std::fs::write(&target, "must survive").unwrap();
        let mut options = OpenOptions::new();
        options.read(true).write(true).truncate(true);
        drop(open_in_place(&target, &mut options).unwrap());
        std::fs::hard_link(&target, &alias).unwrap();
        assert!(
            format!("{:#}", open_in_place(&alias, &mut options).unwrap_err())
                .contains("multiply linked")
        );
        std::fs::remove_file(&alias).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &alias).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(&target, &alias).unwrap();
        assert!(open_in_place(&alias, &mut options).is_err());
        std::fs::remove_file(&alias).unwrap();
        std::fs::create_dir(&alias).unwrap();
        let mut read_only = OpenOptions::new();
        read_only.read(true);
        assert!(open_in_place(&alias, &mut read_only).is_err());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "must survive");
        drop(open_in_place(&target, &mut options).unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn new_in_place_targets_are_created_with_owner_only_access() {
        use std::os::unix::fs::PermissionsExt as _;
        let fixture = tempfile::tempdir().unwrap();
        let target = fixture.path().join("new-lock");
        let mut options = OpenOptions::new();
        options.read(true).write(true).create_new(true);
        let file = open_in_place(&target, &mut options).unwrap();
        assert_eq!(file.metadata().unwrap().permissions().mode() & 0o777, 0o600);
    }
}
