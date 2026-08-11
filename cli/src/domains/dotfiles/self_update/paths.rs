//! Path helpers and binary-location detection for self-update.

use std::path::{Path, PathBuf};

/// Detect the asset name for the current platform.
pub(super) const fn asset_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "dotfiles-windows-x86_64.exe"
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        "dotfiles-linux-aarch64"
    } else {
        "dotfiles-linux-x86_64"
    }
}

/// Return the path where the binary should live inside the repo.
pub(super) fn binary_path(root: &Path) -> PathBuf {
    let name = if cfg!(target_os = "windows") {
        "dotfiles.exe"
    } else {
        "dotfiles"
    };
    root.join("bin").join(name)
}

/// File name of the backup taken before the binary is replaced in place.
///
/// The Windows name keeps the `.exe` extension so the backup stays a runnable
/// image, which matters because the update renames the *running* binary.
pub(super) const fn old_binary_name() -> &'static str {
    if cfg!(target_os = "windows") {
        ".dotfiles-old.exe"
    } else {
        ".dotfiles.old"
    }
}

/// Path where the previous binary is backed up before an in-place update.
///
/// Written by [`super::install::replace_binary`] and restored by
/// [`super::install::download_and_install`] if the post-install smoke test
/// fails.
pub(super) fn old_binary_path(root: &Path) -> PathBuf {
    root.join("bin").join(old_binary_name())
}

/// Check whether the current process is running from `$root/bin/dotfiles`.
pub(super) fn is_running_from_bin(root: &Path) -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let expected = binary_path(root);
    let resolved_exe = crate::infra::fs::canonicalize(&exe).unwrap_or(exe);
    let resolved_expected = crate::infra::fs::canonicalize(&expected).unwrap_or(expected);
    let matched = resolved_exe == resolved_expected;
    tracing::debug!(
        "is_running_from_bin: resolved_exe={resolved_exe:?} resolved_expected={resolved_expected:?} match={matched}"
    );
    matched
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asset_name_is_non_empty() {
        assert!(!asset_name().is_empty());
    }

    #[test]
    fn old_binary_path_sits_next_to_the_installed_binary() {
        let root = Path::new("/repo");
        let old = old_binary_path(root);
        assert_eq!(old.parent(), binary_path(root).parent());
        assert_eq!(
            old.file_name().and_then(std::ffi::OsStr::to_str),
            Some(old_binary_name()),
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_backup_stays_executable() {
        assert_eq!(
            old_binary_name(),
            ".dotfiles-old.exe",
            "the Windows backup is the running image and must stay runnable"
        );
    }
}
