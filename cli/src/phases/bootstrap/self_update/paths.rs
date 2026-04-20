//! Path helpers and binary-location detection for self-update.

use std::path::{Path, PathBuf};

/// Staging file used on Windows until the wrapper can promote the update.
#[cfg(windows)]
const PENDING_BINARY_NAME: &str = ".dotfiles-update.pending";

/// Metadata file that stores the staged version tag.
#[cfg(windows)]
const PENDING_VERSION_NAME: &str = ".dotfiles-update.version";

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

/// Path to a staged binary update.
#[cfg(windows)]
pub(super) fn pending_binary_path(root: &Path) -> PathBuf {
    root.join("bin").join(PENDING_BINARY_NAME)
}

/// Path to the staged version metadata file.
#[cfg(windows)]
pub(super) fn pending_version_path(root: &Path) -> PathBuf {
    root.join("bin").join(PENDING_VERSION_NAME)
}

/// Path where the old Unix binary is backed up before an in-place update.
///
/// Written by [`super::install::replace_binary`] and restored by
/// [`super::install::download_and_install`] if the post-install smoke test
/// fails.
#[cfg(unix)]
pub(super) fn old_binary_path(root: &Path) -> PathBuf {
    root.join("bin").join(".dotfiles.old")
}

/// Check whether the current process is running from `$root/bin/dotfiles`.
pub(super) fn is_running_from_bin(root: &Path) -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let expected = binary_path(root);
    let resolved_exe = crate::fs::canonicalize(&exe).unwrap_or(exe);
    let resolved_expected = crate::fs::canonicalize(&expected).unwrap_or(expected);
    let matched = resolved_exe == resolved_expected;
    tracing::debug!(
        "is_running_from_bin: resolved_exe={resolved_exe:?} resolved_expected={resolved_expected:?} match={matched}"
    );
    matched
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn asset_name_is_non_empty() {
        assert!(!asset_name().is_empty());
    }
}
