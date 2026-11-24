//! Same-account checkout locks shared by writable and read-only checkout views.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};

#[cfg(windows)]
#[allow(
    unsafe_code,
    reason = "Windows known folder lookup and directory identity use native handles"
)]
mod windows;

pub(super) fn checkout_lock_path(common_dir: &Path) -> Result<PathBuf> {
    let name = checkout_lock_name(common_dir)
        .with_context(|| format!("identifying Git common directory {}", common_dir.display()))?;
    let directory =
        account_lock_directory().context("preparing checkout coordination directory")?;
    Ok(directory.join(name))
}

#[cfg(unix)]
fn checkout_lock_name(common_dir: &Path) -> Result<String> {
    use std::os::unix::fs::MetadataExt as _;

    let metadata = std::fs::metadata(common_dir)?;
    anyhow::ensure!(metadata.is_dir(), "Git common directory is not a directory");
    Ok(format!(
        "{:016x}-{:016x}.lock",
        metadata.dev(),
        metadata.ino()
    ))
}

#[cfg(unix)]
fn account_lock_directory() -> Result<PathBuf> {
    let uid = nix::unistd::Uid::effective().as_raw();
    // A fixed host location keeps HOME, the XDG variables and TMPDIR out of
    // lock identity. The sticky /tmp parent protects existing user entries.
    let directory = Path::new("/tmp").join(format!(".dotfiles-checkout-locks-{uid}"));
    prepare_private_directory(&directory, uid)
        .with_context(|| format!("preparing {}", directory.display()))?;
    Ok(directory)
}

#[cfg(unix)]
fn prepare_private_directory(directory: &Path, uid: u32) -> Result<()> {
    use std::os::unix::fs::{DirBuilderExt as _, MetadataExt as _};

    match std::fs::DirBuilder::new().mode(0o700).create(directory) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.into()),
    }
    let metadata = std::fs::symlink_metadata(directory)?;
    anyhow::ensure!(
        metadata.is_dir() && metadata.uid() == uid && metadata.mode() & 0o777 == 0o700,
        "checkout coordination directory must be owned by the current user with mode 0700"
    );
    Ok(())
}

#[cfg(windows)]
use windows::{account_lock_directory, checkout_lock_name};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directory_identity_ignores_path_spelling() {
        let fixture = tempfile::tempdir().unwrap();
        let first = fixture.path().join("main repository");
        let other = fixture.path().join("other repository");
        std::fs::create_dir(&first).unwrap();
        std::fs::create_dir(&other).unwrap();
        let lock = checkout_lock_name(&first).unwrap();
        let spelling = other.join("..").join("main repository");
        assert_eq!(checkout_lock_name(&spelling).unwrap(), lock);
        assert_ne!(checkout_lock_name(&other).unwrap(), lock);
    }

    #[cfg(unix)]
    #[test]
    fn prepare_private_directory_propagates_non_already_exists_create_errors() {
        use std::os::unix::fs::PermissionsExt as _;
        if nix::unistd::Uid::effective().is_root() {
            return;
        }
        let fixture = tempfile::tempdir().unwrap();
        let ro_parent = fixture.path().join("readonly");
        std::fs::create_dir(&ro_parent).unwrap();
        std::fs::set_permissions(&ro_parent, std::fs::Permissions::from_mode(0o500)).unwrap();
        let uid = nix::unistd::Uid::effective().as_raw();
        let err = prepare_private_directory(&ro_parent.join("locks"), uid).unwrap_err();
        std::fs::set_permissions(&ro_parent, std::fs::Permissions::from_mode(0o700)).unwrap();
        let io_err = err.downcast_ref::<std::io::Error>().unwrap();
        assert_eq!(io_err.kind(), std::io::ErrorKind::PermissionDenied);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_to_read_only_git_metadata_keeps_its_identity() {
        use std::os::unix::fs::{PermissionsExt as _, symlink};

        let fixture = tempfile::tempdir().unwrap();
        let metadata = fixture.path().join("metadata");
        let alias = fixture.path().join("alias");
        std::fs::create_dir(&metadata).unwrap();
        std::fs::set_permissions(&metadata, std::fs::Permissions::from_mode(0o555)).unwrap();
        symlink(&metadata, &alias).unwrap();
        assert_eq!(
            checkout_lock_name(&metadata).unwrap(),
            checkout_lock_name(&alias).unwrap()
        );
        assert!(std::fs::read_dir(&metadata).unwrap().next().is_none());
        std::fs::set_permissions(&metadata, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn coordination_directory_must_be_real_private_and_owned() {
        use std::os::unix::fs::{PermissionsExt as _, symlink};

        let fixture = tempfile::tempdir().unwrap();
        let path = fixture.path().join("coordination");
        let alias = fixture.path().join("alias");
        let uid = nix::unistd::Uid::effective().as_raw();
        prepare_private_directory(&path, uid).unwrap();
        prepare_private_directory(&path, uid).unwrap();
        assert!(prepare_private_directory(&path, uid.wrapping_add(1)).is_err());
        symlink(&path, &alias).unwrap();
        assert!(prepare_private_directory(&alias, uid).is_err());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(prepare_private_directory(&path, uid).is_err());
    }
}
