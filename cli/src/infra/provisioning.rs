//! Installation environments and work deferred until the first user session.

use std::io;
use std::path::Path;

use super::env::Env;

/// Environment variable set by an operating-system installer around dotfiles.
pub(crate) const ENV_VAR: &str = "DOTFILES_PROVISIONING";

/// Value used while `install-arch` invokes dotfiles through `arch-chroot`.
pub(crate) const ARCH_CHROOT: &str = "arch-chroot";

/// Marker path relative to the user's home directory.
pub(crate) const DEFERRED_EXTENSIONS_MARKER: &str =
    ".local/state/dotfiles/deferred-vscode-extensions";
pub(crate) const FIRST_LOGIN_UNIT: &str = "dotfiles-first-login.service";
pub(crate) const FIRST_LOGIN_UNIT_PATH: &str = ".config/systemd/user/dotfiles-first-login.service";
pub(crate) const DEFERRED_EXTENSIONS_IDS: &str = "DOTFILES_DEFERRED_VSCODE_IDS";

pub(crate) fn resuming_extensions(env: &dyn Env) -> bool {
    env.var("DOTFILES_RESUME_VSCODE_EXTENSIONS").as_deref() == Some("1")
}

/// Filesystem state of the deferred extension provisioning marker.
#[derive(Debug)]
pub(crate) enum DeferredExtensionsState {
    /// A regular marker file signals pending work.
    Pending,
    /// The marker path does not exist.
    Absent,
    /// The marker path exists but is not a regular file.
    Invalid,
}

/// Inspect the marker without following a symlink at the marker path.
///
/// # Errors
///
/// Returns inspection errors other than a missing marker.
pub(crate) fn deferred_extensions_state(marker: &Path) -> io::Result<DeferredExtensionsState> {
    match std::fs::symlink_metadata(marker) {
        Ok(metadata) if metadata.is_file() => Ok(DeferredExtensionsState::Pending),
        Ok(_) => Ok(DeferredExtensionsState::Invalid),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            Ok(DeferredExtensionsState::Absent)
        }
        Err(error) => Err(error),
    }
}

/// Return whether dotfiles is configuring a target from an Arch chroot.
#[must_use]
pub(crate) fn is_arch_chroot(env: &dyn Env) -> bool {
    env.var(ENV_VAR).as_deref() == Some(ARCH_CHROOT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::env::MapEnv;

    #[test]
    fn detects_only_the_explicit_arch_chroot_value() {
        assert!(is_arch_chroot(&MapEnv::new().with(ENV_VAR, ARCH_CHROOT)));
        assert!(!is_arch_chroot(&MapEnv::new()));
        assert!(!is_arch_chroot(&MapEnv::new().with(ENV_VAR, "container")));
    }

    #[cfg(unix)]
    #[test]
    fn deferred_extensions_state_propagates_non_not_found_errors() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("not-a-directory");
        std::fs::write(&file, b"").unwrap();
        let err = deferred_extensions_state(&file.join("marker")).unwrap_err();
        assert_ne!(err.kind(), io::ErrorKind::NotFound);
    }
}
